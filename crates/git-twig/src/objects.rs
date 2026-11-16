//! Compare private checkout bytes to raw objects, never clean-filtered index hashes.
use crate::{
    checked, git, native,
    tree::{Entry, Kind},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Stdio},
};

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
