//! Per-invocation staging ownership. Only successful commands discard their scratch.
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub(super) struct Staging {
    container: Option<PathBuf>,
    bootstrap: Option<PathBuf>,
}

impl Staging {
    pub fn create(&mut self, root: &Path, destination_parent: &Path) -> Result<PathBuf> {
        let parent = crate::state_directory(root)?.join("staging");
        fs::create_dir_all(&parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            anyhow::ensure!(
                fs::metadata(&parent)?.dev() == fs::metadata(destination_parent)?.dev(),
                "Git metadata and destination are on different filesystems"
            );
        }
        let container = create_private(&parent)?;
        self.container = Some(container.clone());
        crate::trace(&format!(
            "private staging: {} (retained if creation fails)",
            container.display()
        ));

        // Inherit the destination's GID, SGID and default ACL before relocating.
        // Creating the checkout directly under .git would inherit different metadata.
        let bootstrap = create_private(destination_parent)?;
        self.bootstrap = Some(bootstrap.clone());
        let checkout = container.join("checkout");
        fs::rename(&bootstrap, &checkout).with_context(|| {
            format!(
                "cannot relocate empty staging {} into {}",
                bootstrap.display(),
                checkout.display()
            )
        })?;
        self.bootstrap = None;
        Ok(checkout)
    }

    pub fn cleanup(&mut self) {
        for path in self
            .bootstrap
            .take()
            .into_iter()
            .chain(self.container.take())
        {
            let result = fs::symlink_metadata(&path)
                .map_err(anyhow::Error::from)
                .and_then(|metadata| {
                    crate::snapshot::remove_private_tree(&path, metadata.file_type())
                });
            if let Err(error) = result {
                eprintln!(
                    "git-twig: command succeeded; could not remove staging at {}: {error:#}",
                    path.display()
                );
            }
        }
    }
}

fn create_private(parent: &Path) -> Result<PathBuf> {
    for _ in 0..1000 {
        let path = parent.join(format!(
            ".git-twig-stage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("unable to allocate private staging directory")
}
