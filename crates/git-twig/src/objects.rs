//! Compare private checkout bytes to raw objects, never clean-filtered index hashes.
use crate::{
    checked, git, native,
    tree::{Entry, Kind},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Stdio},
};

/// Hash raw PRIVATE regular files in one Git process, without inflating or copying
/// target blobs through a pipe. Git supplies its own object format and SHA-1 collision
/// detection. No -w, filters, index stat cache or source file is involved.
pub fn hash_private_files(root: &Path, data: &Path, entries: &[&Entry]) -> Result<Vec<bool>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = Vec::with_capacity(entries.len());
    let mut total = 0u64;
    for (index, &entry) in entries.iter().enumerate() {
        ensure!(
            matches!(entry.kind, Kind::File { .. }),
            "raw hash input is not a regular entry"
        );
        let path = data.join(&entry.path);
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file(),
            "private hash candidate is not a regular file"
        );
        // A dirty sparse/oversized donor must not turn validation into hashing
        // terabytes that cannot possibly match the target's known blob length.
        if Some(metadata.len()) != entry.blob_bytes {
            continue;
        }
        total = total.saturating_add(metadata.len());
        files.push(HashFile {
            index,
            entry,
            path,
            bytes: metadata.len(),
        });
    }
    if files.is_empty() {
        return Ok(vec![false; entries.len()]);
    }
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let workers = hash_worker_count(files.len(), total, cpus);
    let mut batches: Vec<_> = (0..workers)
        .map(|_| HashBatch {
            bytes: 0,
            files: Vec::new(),
        })
        .collect();
    // Largest-first balancing avoids putting all large media blobs in one worker.
    files.sort_unstable_by_key(|file| std::cmp::Reverse(file.bytes));
    for file in files {
        let batch = batches
            .iter_mut()
            .min_by_key(|batch| batch.bytes)
            .expect("at least one worker");
        batch.bytes = batch.bytes.saturating_add(file.bytes);
        batch.files.push(file);
    }
    crate::trace(&format!(
        "raw verification: {workers} Git hash workers, {total} bytes"
    ));
    // No other phase mutates these owned private paths until every worker joins.
    std::thread::scope(|scope| {
        let handles: Vec<_> = batches
            .iter()
            .map(|batch| scope.spawn(move || hash_batch(root, batch)))
            .collect();
        let mut matched = vec![false; entries.len()];
        for handle in handles {
            for (index, equal) in handle
                .join()
                .map_err(|_| anyhow::anyhow!("raw hash worker panicked"))??
            {
                matched[index] = equal;
            }
        }
        Ok(matched)
    })
}

struct HashFile<'a> {
    index: usize,
    entry: &'a Entry,
    path: PathBuf,
    bytes: u64,
}

struct HashBatch<'a> {
    bytes: u64,
    files: Vec<HashFile<'a>>,
}

fn hash_worker_count(files: usize, bytes: u64, cpus: usize) -> usize {
    if files >= 512 && bytes >= 64 * 1024 * 1024 {
        cpus.clamp(1, 4)
    } else {
        1
    }
}

fn hash_batch(root: &Path, batch: &HashBatch<'_>) -> Result<Vec<(usize, bool)>> {
    if batch.files.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = Vec::new();
    for file in &batch.files {
        quote_stdin_path(&native::path_bytes(&file.path), &mut input);
    }
    let output = crate::checked_with_input(
        git(root).args([
            "--no-replace-objects",
            "hash-object",
            "--no-filters",
            "--stdin-paths",
        ]),
        input,
    )?;
    let entries: Vec<_> = batch.files.iter().map(|file| file.entry).collect();
    let matches = parse_hashes(&output.stdout, &entries)?;
    Ok(batch
        .files
        .iter()
        .zip(matches)
        .map(|(file, equal)| (file.index, equal))
        .collect())
}

fn parse_hashes(output: &[u8], entries: &[&Entry]) -> Result<Vec<bool>> {
    ensure!(
        output.last() == Some(&b'\n'),
        "unterminated raw hash response"
    );
    let hashes: Vec<_> = output[..output.len() - 1]
        .split(|byte| *byte == b'\n')
        .collect();
    ensure!(
        hashes.len() == entries.len(),
        "wrong raw hash response count"
    );
    hashes
        .iter()
        .zip(entries)
        .map(|(hash, entry)| {
            ensure!(
                hash.len() == entry.object.as_str().len() && hash.iter().all(u8::is_ascii_hexdigit),
                "invalid raw object hash from Git"
            );
            Ok(*hash == entry.object.as_str().as_bytes())
        })
        .collect()
}

fn quote_stdin_path(path: &[u8], output: &mut Vec<u8>) {
    // hash-object --stdin-paths supports Git C quoting, not NUL separators on Git
    // 2.40. Always quote; octal escapes preserve newline, tabs and non-UTF8 bytes.
    output.push(b'"');
    for &byte in path {
        match byte {
            b' '..=b'~' if byte != b'"' && byte != b'\\' => output.push(byte),
            _ => output.extend_from_slice(format!("\\{byte:03o}").as_bytes()),
        }
    }
    output.extend_from_slice(b"\"\n");
}

pub struct Objects {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Objects {
    pub fn open(root: &Path) -> Result<Self> {
        // Object IDs alone are safe newline-delimited inputs on Git 2.40 too.
        let mut child = git(root)
            .args(["--no-replace-objects", "cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take().context("missing object stdin")?;
        let output = BufReader::new(child.stdout.take().context("missing object stdout")?);
        Ok(Self {
            child,
            input,
            output,
        })
    }

    pub fn matches(&mut self, data: &Path, entry: &Entry) -> Result<bool> {
        writeln!(self.input, "{}", entry.object.as_str())?;
        self.input.flush()?;
        let mut header = String::new();
        self.output.read_line(&mut header)?;
        let mut fields = header.trim_end_matches('\n').split(' ');
        ensure!(
            fields.next() == Some(entry.object.as_str()),
            "unexpected object ID from Git"
        );
        ensure!(
            fields.next() == Some("blob"),
            "expected a raw blob from Git"
        );
        let size: u64 = fields.next().context("missing blob size")?.parse()?;
        ensure!(fields.next().is_none(), "extra blob header fields");
        let path = data.join(&entry.path);
        let mut file: Box<dyn Read> = match entry.kind {
            Kind::File { .. } => {
                let mut options = fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    options.custom_flags(
                        windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
                    );
                }
                let file = options.open(&path)?;
                ensure!(
                    file.metadata()?.is_file(),
                    "private candidate is not a regular file"
                );
                Box::new(file)
            }
            Kind::Symlink => Box::new(std::io::Cursor::new(native::path_bytes(&fs::read_link(
                path,
            )?))),
            Kind::Submodule => anyhow::bail!("submodules require stock Git"),
        };
        let equal = compare(&mut self.output, &mut file, size)?;
        let mut delimiter = [0];
        self.output.read_exact(&mut delimiter)?;
        ensure!(delimiter == *b"\n", "invalid raw blob delimiter");
        Ok(equal)
    }
}

impl Drop for Objects {
    fn drop(&mut self) {
        // Reap on success and on malformed output/read errors, including early fallback.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn compare(object: &mut impl Read, candidate: &mut dyn Read, mut remaining: u64) -> Result<bool> {
    let mut expected = [0; 64 * 1024];
    let mut actual = [0; 64 * 1024];
    let mut equal = true;
    while remaining > 0 {
        let size = remaining.min(expected.len() as u64) as usize;
        object.read_exact(&mut expected[..size])?;
        // Always drain the complete object, even after discovering a mismatch.
        if equal {
            match candidate.read_exact(&mut actual[..size]) {
                Ok(()) => equal = expected[..size] == actual[..size],
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => equal = false,
                Err(error) => return Err(error.into()),
            }
        }
        remaining -= size as u64;
    }
    Ok(equal && candidate.read(&mut actual[..1])? == 0)
}

/// Fail closed when replace refs or grafts could make an object ID mean other bytes.
pub fn ensure_literal_objects(root: &Path) -> Result<()> {
    ensure!(
        checked(git(root).args(["for-each-ref", "--format=%(refname)", "refs/replace/"]))?
            .stdout
            .is_empty(),
        "replace objects require stock Git"
    );
    let grafts = crate::output_path(
        checked(git(root).args(["rev-parse", "--git-path", "info/grafts"]))?.stdout,
    )?;
    ensure!(!root.join(grafts).exists(), "grafts require stock Git");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hashing_is_bounded_and_small_checkouts_stay_single_process() {
        assert_eq!(hash_worker_count(500, 1024 * 1024 * 1024, 20), 1);
        assert_eq!(hash_worker_count(1000, 1024, 20), 1);
        assert_eq!(hash_worker_count(1000, 1024 * 1024 * 1024, 20), 4);
        assert_eq!(hash_worker_count(1000, 1024 * 1024 * 1024, 2), 2);
        assert_eq!(hash_worker_count(1000, 1024 * 1024 * 1024, 1), 1);
    }

    #[test]
    fn stdin_paths_preserve_every_byte_without_creating_extra_requests() {
        let mut input = Vec::new();
        quote_stdin_path(b" a\n\t\"\\\xff", &mut input);
        assert_eq!(input, b"\" a\\012\\011\\042\\134\\377\"\n");
        assert_eq!(input.iter().filter(|byte| **byte == b'\n').count(), 1);
    }

    #[test]
    fn hash_responses_are_complete_and_match_the_repository_object_format() -> Result<()> {
        for size in [40, 64] {
            let oid = "0".repeat(size);
            let entries = crate::tree::parse(format!("100644 blob {oid} 6\tfile\0").as_bytes())?;
            let references = [&entries[0]];
            assert_eq!(
                parse_hashes(format!("{oid}\n").as_bytes(), &references)?,
                vec![true]
            );
            assert_eq!(
                parse_hashes(format!("{}\n", "1".repeat(size)).as_bytes(), &references)?,
                vec![false]
            );
            for invalid in [
                oid.clone(),
                format!("{oid}\n{oid}\n"),
                format!("{}\n", "x".repeat(size)),
                "missing\n".into(),
                String::new(),
            ] {
                assert!(parse_hashes(invalid.as_bytes(), &references).is_err());
            }
        }
        Ok(())
    }

    #[test]
    fn compares_raw_bytes_and_drains_mismatches() -> Result<()> {
        for bytes in [b"".as_slice(), b"a", b"a\r\n", b"\0binary", &[7; 140000]] {
            let mut object = std::io::Cursor::new(bytes);
            assert!(compare(
                &mut object,
                &mut std::io::Cursor::new(bytes),
                bytes.len() as u64
            )?);
            assert_eq!(object.position(), bytes.len() as u64);
        }
        for candidate in [b"".as_slice(), b"x", b"longer"] {
            let mut object = std::io::Cursor::new(b"a");
            assert!(!compare(
                &mut object,
                &mut std::io::Cursor::new(candidate),
                1
            )?);
            assert_eq!(object.position(), 1);
        }
        Ok(())
    }
}
