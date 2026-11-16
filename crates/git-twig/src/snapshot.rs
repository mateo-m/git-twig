//! Clone tracked entries; macOS bulk-clones subtrees before pruning to the Git tree.
use crate::{
    native,
    tree::{Entry, Kind},
};
use anyhow::{Result, bail, ensure};
use std::{collections::BTreeMap, ffi::OsString, fs, path::Path};

#[derive(Default)]
struct Directory {
    children: BTreeMap<OsString, Node>,
}
enum Node {
    Directory(Directory),
    File { executable: bool },
    Symlink,
}
impl Directory {
    fn insert(&mut self, parts: &[OsString], kind: Kind) -> Result<()> {
        let (name, rest) = parts.split_first().expect("parsed paths are nonempty");
        if rest.is_empty() {
            ensure!(!self.children.contains_key(name), "duplicate tree path");
            let node = match kind {
                Kind::File { executable } => Node::File { executable },
                Kind::Symlink => Node::Symlink,
                Kind::Submodule => bail!("submodules require stock Git"),
            };
            self.children.insert(name.clone(), node);
        } else {
            let child = self
                .children
                .entry(name.clone())
                .or_insert_with(|| Node::Directory(Directory::default()));
            match child {
                Node::Directory(directory) => directory.insert(rest, kind)?,
                _ => bail!("tree file is also a directory"),
            }
        }
        Ok(())
    }
}

pub fn materialize(
    root: &Path,
    data: &Path,
    entries: &[Entry],
    modes: &native::FileModes,
) -> Result<usize> {
    let mut tree = Directory::default();
    for entry in entries {
        tree.insert(
            &entry.path.iter().map(OsString::from).collect::<Vec<_>>(),
            entry.kind,
        )?;
    }
    // Never snapshot the repository root: .git and unrelated root trees stay out entirely.
    for (name, node) in &tree.children {
        let source = root.join(name);
        let target = data.join(name);
        match node {
            Node::Directory(directory) => {
                ensure!(
                    fs::symlink_metadata(&source)?.is_dir(),
                    "source directory changed type"
                );
                #[cfg(any(target_os = "linux", windows))]
                {
                    clone_tracked_directory(&source, &target, directory, modes)?;
                    continue; // The file walker already establishes types and checkout modes.
                }
                #[cfg(not(any(target_os = "linux", windows)))]
                {
                    native::clone_path(&source, &target)?;
                    let _ = directory;
                }
            }
            Node::File { .. } => {
                ensure!(
                    fs::symlink_metadata(&source)?.is_file(),
                    "source is not a regular file"
                );
                native::clone_path(&source, &target)?;
            }
            Node::Symlink => native::symlink(&fs::read_link(&source)?, &target)?,
        }
        normalize(
            &target,
            node,
            fs::symlink_metadata(&target)?.file_type(),
            modes,
        )?;
    }
    Ok(if cfg!(target_os = "macos") {
        tree.children
            .values()
            .filter(|node| matches!(node, Node::Directory(_)))
            .count()
    } else {
        0
    })
}

// Linux FICLONE and Windows block cloning work on regular files, not directory trees.
// Walk only the committed inventory so ignored files and unrelated special files are never opened.
#[cfg(any(target_os = "linux", windows))]
fn clone_tracked_directory(
    source: &Path,
    target: &Path,
    directory: &Directory,
    modes: &native::FileModes,
) -> Result<()> {
    ensure!(
        fs::symlink_metadata(source)?.is_dir(),
        "source directory changed type"
    );
    // mkdir already applies umask/default ACL and inherits SGID/GID like Git.
    // Even chmod(mode | SGID) clears SGID when its group is not in our groups.
    fs::create_dir(target)?;
    for (name, node) in &directory.children {
        let source = source.join(name);
        let target = target.join(name);
        match node {
            Node::Directory(directory) => {
                clone_tracked_directory(&source, &target, directory, modes)?;
            }
            Node::File { executable } => {
                native::clone_path(&source, &target)?;
                modes.apply(&target, *executable)?;
            }
            Node::Symlink => native::symlink(&fs::read_link(&source)?, &target)?,
        }
    }
    Ok(())
}

fn normalize(
    path: &Path,
    node: &Node,
    actual: fs::FileType,
    modes: &native::FileModes,
) -> Result<()> {
    match node {
        Node::Directory(expected) => {
            ensure!(actual.is_dir(), "cloned directory changed type");
            // Source directories may be read-only; checkout permissions come from this umask.
            modes.apply_directory(path)?;
            let mut remaining = expected.children.len();
            for child in fs::read_dir(path)? {
                let child = child?;
                let actual = child.file_type()?;
                match expected.children.get(&child.file_name()) {
                    Some(node) => {
                        normalize(&child.path(), node, actual, modes)?;
                        remaining -= 1;
                    }
                    None => remove_private_tree(&child.path(), actual)?,
                }
            }
            ensure!(remaining == 0, "tracked path missing from cloned directory");
        }
        Node::File { executable } => {
            ensure!(actual.is_file(), "cloned regular file changed type");
            modes.apply(path, *executable)?;
        }
        Node::Symlink => ensure!(actual.is_symlink(), "cloned symlink changed type"),
    }
    Ok(())
}

pub(super) fn remove_private_tree(path: &Path, kind: fs::FileType) -> Result<()> {
    if kind.is_dir() {
        // Only owned staging/cloned entries reach here. Never follow a symlink.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        for child in fs::read_dir(path)? {
            let child = child?;
            remove_private_tree(&child.path(), child.file_type()?)?;
        }
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_file_directory_collisions() {
        let mut tree = Directory::default();
        tree.insert(&["a".into()], Kind::File { executable: false })
            .unwrap();
        assert!(
            tree.insert(&["a".into(), "b".into()], Kind::Symlink)
                .is_err()
        );
    }
    #[test]
    fn rejects_duplicate_paths() {
        let mut tree = Directory::default();
        tree.insert(&["a".into()], Kind::Symlink).unwrap();
        assert!(tree.insert(&["a".into()], Kind::Symlink).is_err());
    }
}
