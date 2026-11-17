//! Target-index checkout policy and per-path recovery inside private staging.
use crate::{
    checked, checked_with_input as with_input, git, native,
    objects::{self, Objects},
    output_path,
    tree::{Entry, Kind},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
};

/// Git copies config.worktree into a new Git directory, but not sibling include
/// files. Relative includes there can therefore change policy after registration.
pub fn ensure_relocatable_worktree_config(root: &Path) -> Result<()> {
    let enabled = git(root)
        .args(["config", "--bool", "--get", "extensions.worktreeConfig"])
        .output()?;
    if enabled.status.code() == Some(1) {
        return Ok(());
    }
    let enabled = checked_output(enabled)?;
    if enabled.stdout != b"true\n" {
        return Ok(());
    }
    let includes = git(root)
        .args([
            "config",
            "--worktree",
            "--no-includes",
            "--path",
            "--null",
            "--get-all",
            "include.path",
        ])
        .output()?;
    if includes.status.code() == Some(1) {
        return Ok(());
    }
    let includes = checked_output(includes)?.stdout;
    ensure!(
        includes.last() == Some(&0),
        "unterminated worktree config includes"
    );
    for path in includes[..includes.len() - 1].split(|b| *b == 0) {
        // --path expands ~/ and %(prefix)/, but leaves ordinary relative paths
        // relative. Absolute includes keep their own nested-include anchoring.
        ensure!(
            PathBuf::from(native::os_string(path.to_vec())).is_absolute(),
            "relative config.worktree includes require stock Git"
        );
    }
    Ok(())
}

pub struct Checkout<'a> {
    git_dir: PathBuf,
    data: &'a Path,
    index: &'a Path,
    symlinks: bool,
}

impl<'a> Checkout<'a> {
    pub fn new(root: &Path, data: &'a Path, index: &'a Path, symlinks: bool) -> Result<Self> {
        let git_dir =
            output_path(checked(git(root).args(["rev-parse", "--absolute-git-dir"]))?.stdout)?;
        Ok(Self {
            git_dir,
            data,
            index,
            symlinks,
        })
    }

    fn command(&self) -> Command {
        let mut command = git(self.data);
        command
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_WORK_TREE", self.data)
            .env("GIT_INDEX_FILE", self.index)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.sparseCheckout=false",
                "-c",
                "core.filemode=true",
                "-c",
                if self.symlinks {
                    "core.symlinks=true"
                } else {
                    "core.symlinks=false"
                },
            ]);
        command
    }

    pub fn initialize(&self, commit: &str) -> Result<()> {
        checked(self.command().args(["read-tree", commit]))?;
        Ok(())
    }

    pub fn policy(&self, entries: &[Entry]) -> Result<Policy> {
        let config = checked(self.command().args(["config", "--null", "--list"]))?.stdout;
        let configuration = Configuration::parse(&config)?;
        let input = paths(entries.iter().map(|entry| entry.path.as_path()));
        let attributes = with_input(
            self.command()
                .args(["check-attr", "--cached", "--all", "-z", "--stdin"]),
            input,
        )?
        .stdout;
        let actions = parse_attributes(entries, &attributes, &configuration)?;
        Ok(Policy {
            config,
            attributes,
            actions,
        })
    }

    fn write_paths(&self, paths: &[&Path]) -> Result<()> {
        if !paths.is_empty() {
            with_input(
                self.command()
                    .args(["checkout-index", "--force", "-z", "--stdin"]),
                self::paths(paths.iter().copied()),
            )?;
        }
        Ok(())
    }

    pub fn repair(&self, root: &Path, entries: &[Entry], policy: &Policy) -> Result<()> {
        let regular: Vec<_> = entries
            .iter()
            .zip(&policy.actions)
            .filter(|(entry, action)| {
                **action == Action::VerifyClone && matches!(entry.kind, Kind::File { .. })
            })
            .map(|(entry, _)| entry)
            .collect();
        let mut matches = objects::hash_private_files(root, self.data, &regular)?.into_iter();
        let mut symlink_objects = None;
        let mut attributes = Vec::new();
        let mut files = Vec::new();
        let mut verified = 0;
        for (entry, action) in entries.iter().zip(&policy.actions) {
            let equal = match (*action, entry.kind) {
                (Action::VerifyClone, Kind::File { .. }) => {
                    matches.next().expect("one hash verdict per regular file")
                }
                (Action::VerifyClone, Kind::Symlink) => {
                    if symlink_objects.is_none() {
                        symlink_objects = Some(Objects::open(root)?);
                    }
                    symlink_objects
                        .as_mut()
                        .expect("opened object stream")
                        .matches(self.data, entry)?
                }
                _ => false,
            };
            if equal {
                if matches!(entry.kind, Kind::File { .. }) {
                    verified += 1;
                }
                continue;
            }
            if entry
                .path
                .file_name()
                .is_some_and(|name| name == ".gitattributes")
            {
                attributes.push(entry.path.as_path());
            } else {
                files.push(entry.path.as_path());
            }
        }
        // Git may consult working-tree attributes while writing. Restore target policies
        // first; untracked policies were already removed by the snapshot inventory.
        self.write_paths(&attributes)?;
        self.write_paths(&files)?;
        policy.ensure_same(&self.policy(entries)?)?;
        crate::trace(&format!(
            "checkout: {verified} verified file clones, {} Git-written paths",
            attributes.len() + files.len()
        ));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    VerifyClone,
    GitWrite,
}

pub struct Policy {
    config: Vec<u8>,
    attributes: Vec<u8>,
    actions: Vec<Action>,
}

impl Policy {
    pub fn ensure_same(&self, other: &Self) -> Result<()> {
        ensure!(
            self.config == other.config && self.attributes == other.attributes,
            "checkout configuration or attributes changed during creation"
        );
        Ok(())
    }
}

#[derive(Default)]
struct Attributes {
    text: Option<Vec<u8>>,
    eol: Option<Vec<u8>>,
    legacy_crlf: bool,
}

fn parse_attributes(
    entries: &[Entry],
    bytes: &[u8],
    configuration: &Configuration,
) -> Result<Vec<Action>> {
    ensure!(
        bytes.is_empty() || bytes.last() == Some(&0),
        "unterminated Git attributes"
    );
    let fields: Vec<_> = if bytes.is_empty() {
        Vec::new()
    } else {
        bytes[..bytes.len() - 1].split(|b| *b == 0).collect()
    };
    ensure!(fields.len() % 3 == 0, "invalid Git attribute response");
    let inventory: BTreeMap<_, _> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| (native::path_bytes(&entry.path), i))
        .collect();
    let mut attributes: Vec<_> = entries.iter().map(|_| Attributes::default()).collect();
    let mut seen = std::collections::BTreeSet::new();
    for row in fields.as_chunks::<3>().0 {
        let index = *inventory
            .get(row[0])
            .context("attributes for unknown path")?;
        ensure!(seen.insert((index, row[1])), "duplicate Git attribute");
        let attribute = &mut attributes[index];
        match row[1] {
            // --all reports explicitly present values, including literal sentinel names.
            // Conservative: even an explicit -filter/-ident requires stock Git for now.
            b"filter" | b"ident" | b"working-tree-encoding" => {
                anyhow::bail!(
                    "checkout attribute {}={:?} on {:?} requires stock Git",
                    String::from_utf8_lossy(row[1]),
                    String::from_utf8_lossy(row[2]),
                    native::os_string(row[0].to_vec())
                )
            }
            b"text" => attribute.text = Some(row[2].to_vec()),
            b"eol" => attribute.eol = Some(row[2].to_vec()),
            b"crlf" => attribute.legacy_crlf = true,
            _ => {}
        }
    }
    Ok(entries
        .iter()
        .zip(attributes)
        .map(|(entry, attrs)| {
            if entry.kind == Kind::Symlink || attrs.identity(configuration) {
                Action::VerifyClone
            } else {
                Action::GitWrite
            }
        })
        .collect())
}

impl Attributes {
    fn identity(&self, config: &Configuration) -> bool {
        if self.legacy_crlf {
            return false;
        }
        let text = self.text.as_deref();
        if !matches!(
            text,
            None | Some(b"set" | b"auto" | b"unset" | b"unspecified")
        ) {
            return false;
        }
        match self.eol.as_deref() {
            Some(b"lf") => return true,
            Some(_) => return false,
            None => {}
        }
        match config.autocrlf.as_slice() {
            b"input" => true,
            b"false" | b"no" | b"off" | b"0" => {
                // "unset" could be -text OR literal text=unset. Both are identity
                // with autocrlf=false and no eol/crlf, regardless of core.eol.
                // The native line ending is CRLF on Windows.
                matches!(text, None | Some(b"unset" | b"unspecified"))
                    || config.eol == b"lf"
                    || (config.eol == b"native" && !cfg!(windows))
            }
            _ => false,
        }
    }
}

struct Configuration {
    autocrlf: Vec<u8>,
    eol: Vec<u8>,
}

impl Configuration {
    fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.is_empty() || bytes.last() == Some(&0),
            "unterminated Git config"
        );
        let mut result = Self {
            autocrlf: b"false".to_vec(),
            eol: b"native".to_vec(),
        };
        for field in bytes.split(|b| *b == 0).filter(|field| !field.is_empty()) {
            let split = field.iter().position(|b| *b == b'\n');
            let (key, value) =
                split.map_or((field, b"".as_slice()), |i| (&field[..i], &field[i + 1..]));
            ensure!(
                !key.starts_with(b"includeif."),
                "conditional config includes require stock Git"
            );
            match key {
                b"core.autocrlf" => result.autocrlf = value.to_ascii_lowercase(),
                b"core.eol" => result.eol = value.to_ascii_lowercase(),
                b"core.attributesfile" => {
                    // ~ expansion and relative paths depend on Git context. Keep the
                    // first implementation to explicit absolute external policy paths.
                    ensure!(
                        PathBuf::from(native::os_string(value.to_vec())).is_absolute(),
                        "relative attribute files require stock Git"
                    );
                }
                _ => {}
            }
        }
        Ok(result)
    }
}

fn paths<'a>(paths: impl Iterator<Item = &'a Path>) -> Vec<u8> {
    paths
        .flat_map(|path| {
            let mut bytes = native::path_bytes(path);
            bytes.push(0);
            bytes
        })
        .collect()
}

fn checked_output(output: Output) -> Result<Output> {
    ensure!(
        output.status.success(),
        "Git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absence_is_not_a_literal_attribute_state() -> Result<()> {
        let entries =
            crate::tree::parse(b"100644 blob 0000000000000000000000000000000000000000 6\tfile\0")?;
        let config = Configuration::parse(b"")?;
        for value in ["unset", "unspecified", "set", "upper"] {
            assert!(
                parse_attributes(
                    &entries,
                    format!("file\0filter\0{value}\0").as_bytes(),
                    &config
                )
                .is_err()
            );
        }
        assert_eq!(
            parse_attributes(&entries, b"", &config)?,
            vec![Action::VerifyClone]
        );
        assert_eq!(
            parse_attributes(&entries, b"file\0text\0auto\0file\0eol\0lf\0", &config)?,
            vec![Action::VerifyClone]
        );
        assert_eq!(
            parse_attributes(&entries, b"file\0text\0unset\0file\0eol\0crlf\0", &config)?,
            vec![Action::GitWrite]
        );
        assert!(parse_attributes(&entries, b"other\0text\0auto\0", &config).is_err());
        assert!(parse_attributes(&entries, b"file\0text\0auto", &config).is_err());
        Ok(())
    }
}
