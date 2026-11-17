use crate::native;
use anyhow::{Context, Result, bail, ensure};
use std::path::{Component, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File { executable: bool },
    Symlink,
    Submodule,
}

pub struct Entry {
    pub path: PathBuf,
    pub kind: Kind,
    pub object: ObjectId,
    pub blob_bytes: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ObjectId(String);

impl ObjectId {
    fn parse(value: &str) -> Result<Self> {
        ensure!(
            matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid tree object ID"
        );
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    ensure!(
        bytes.is_empty() || bytes.last() == Some(&0),
        "unterminated Git tree"
    );
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let separator = record
                .iter()
                .position(|byte| *byte == b'\t')
                .context("missing tree separator")?;
            let header = std::str::from_utf8(&record[..separator])?;
            let mut fields = header.split_ascii_whitespace();
            let mode = fields.next().context("missing tree mode")?;
            let object_type = fields.next().context("missing tree object type")?;
            let object = ObjectId::parse(fields.next().context("missing tree object ID")?)?;
            let size = fields.next().context("missing tree object size")?;
            let blob_bytes = if mode == "160000" {
                ensure!(size == "-", "unexpected gitlink object size");
                None
            } else {
                Some(size.parse::<u64>().context("invalid blob size")?)
            };
            ensure!(fields.next().is_none(), "extra tree header fields");
            ensure!(
                object_type == if mode == "160000" { "commit" } else { "blob" },
                "unexpected tree object type"
            );
            let kind = match mode {
                "100644" => Kind::File { executable: false },
                "100755" => Kind::File { executable: true },
                "120000" => Kind::Symlink,
                "160000" => Kind::Submodule,
                _ => bail!("unsupported Git mode: {mode}"),
            };
            let path = PathBuf::from(native::os_string(record[separator + 1..].to_vec()));
            ensure!(!path.as_os_str().is_empty() && path.components().all(|part|
            matches!(part, Component::Normal(name) if !name.eq_ignore_ascii_case(".git"))),
            "unsafe tree path: {}", path.display());
            Ok(Entry {
                path,
                kind,
                object,
                blob_bytes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_tabs_and_newlines() {
        let entries =
            parse(b"100644 blob 0000000000000000000000000000000000000000 6\todd\tname\n.txt\0")
                .unwrap();
        assert_eq!(entries[0].path, PathBuf::from("odd\tname\n.txt"));
    }
    #[test]
    fn refuses_unsafe_paths_and_modes() {
        let object = "0".repeat(40);
        for path in ["../escape", ".git/config", "/absolute", "a/.Git/config"] {
            assert!(parse(format!("100644 blob {object} 6\t{path}\0").as_bytes()).is_err());
        }
        for header in [
            format!("999999 blob {object}"),
            format!("100644 commit {object}"),
            format!("100644 blob {object} extra"),
            "100644 blob abc".into(),
        ] {
            assert!(parse(format!("{header}\tx\0").as_bytes()).is_err());
        }
        assert!(parse(format!("100644 blob {object} 6\tx").as_bytes()).is_err());
        assert!(parse(format!("100644 blob {} 6\tx\0", "0".repeat(64)).as_bytes()).is_ok());
    }
}
