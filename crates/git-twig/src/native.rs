use anyhow::Result;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
use anyhow::bail;
use std::{ffi::OsString, path::Path};

#[cfg(target_os = "macos")]
pub fn clone_path(source: &Path, destination: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    // Darwin sys/clonefile.h; libc exposes clonefile but not these flag constants.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    const CLONE_NOFOLLOW_ANY: u32 = 0x0008;
    // SAFETY: paths are live NUL-terminated strings; flags reject symlink traversal.
    let result = unsafe {
        libc::clonefile(
            source.as_ptr(),
            destination.as_ptr(),
            CLONE_NOFOLLOW | CLONE_NOFOLLOW_ANY,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn clone_path(source: &Path, destination: &Path) -> Result<()> {
    use std::{
        fs::OpenOptions,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    };
    // Never follow a leaf symlink or overwrite an existing destination.
    let source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source)?;
    anyhow::ensure!(source.metadata()?.is_file(), "source is not a regular file");
    let destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    // SAFETY: both descriptors are live; FICLONE takes a source fd, not a pointer.
    let result = unsafe { libc::ioctl(destination.as_raw_fd(), libc::FICLONE, source.as_raw_fd()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(windows)]
pub fn clone_path(source: &Path, destination: &Path) -> Result<()> {
    use std::{
        fs::OpenOptions,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE, FILE_FLAG_OPEN_REPARSE_POINT,
            GetVolumeInformationByHandleW,
        },
        System::{
            Ioctl::{
                CHECKSUM_TYPE_NONE, DUPLICATE_EXTENTS_DATA, FSCTL_DUPLICATE_EXTENTS_TO_FILE,
                FSCTL_GET_INTEGRITY_INFORMATION, FSCTL_GET_INTEGRITY_INFORMATION_BUFFER,
                FSCTL_SET_INTEGRITY_INFORMATION, FSCTL_SET_INTEGRITY_INFORMATION_BUFFER,
                FSCTL_SET_SPARSE,
            },
            SystemServices::FILE_SUPPORTS_BLOCK_REFCOUNTING,
        },
    };
    // Never follow a leaf reparse point or overwrite an existing destination.
    let source = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)?;
    let metadata = source.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "source is not a regular file"
    );
    let mut flags = 0;
    // SAFETY: the handle is live; zero-length name buffers are never written.
    let result = unsafe {
        GetVolumeInformationByHandleW(
            source.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            null_mut(),
            0,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    anyhow::ensure!(
        flags & FILE_SUPPORTS_BLOCK_REFCOUNTING != 0,
        "volume does not support block cloning"
    );
    let mut integrity = FSCTL_GET_INTEGRITY_INFORMATION_BUFFER::default();
    // SAFETY: this control code takes no input and writes this output type.
    unsafe {
        control(
            &source,
            FSCTL_GET_INTEGRITY_INFORMATION,
            &(),
            &mut integrity,
        )?
    };
    let destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    // ReFS clones only between files with the same sparse and integrity settings.
    if metadata.file_attributes() & FILE_ATTRIBUTE_SPARSE_FILE != 0 {
        // SAFETY: an empty input buffer sets the sparse flag; there is no output.
        unsafe { control(&destination, FSCTL_SET_SPARSE, &(), &mut ())? };
    }
    if integrity.ChecksumAlgorithm != CHECKSUM_TYPE_NONE as u16 {
        let settings = FSCTL_SET_INTEGRITY_INFORMATION_BUFFER {
            ChecksumAlgorithm: integrity.ChecksumAlgorithm,
            Reserved: integrity.Reserved,
            Flags: integrity.Flags,
        };
        // SAFETY: this control code reads this input type; there is no output.
        unsafe {
            control(
                &destination,
                FSCTL_SET_INTEGRITY_INFORMATION,
                &settings,
                &mut (),
            )?
        };
    }
    // Clone regions must end on a cluster boundary and stay below 4 GiB. A last
    // partial cluster may round up past the end of file once the length is set.
    let length = metadata.file_size();
    destination.set_len(length)?;
    let cluster = u64::from(integrity.ClusterSizeInBytes);
    anyhow::ensure!(cluster.is_power_of_two(), "invalid cluster size");
    let end = length.next_multiple_of(cluster);
    let step = (4 << 30) - cluster;
    for offset in (0..end).step_by(step as usize) {
        let extents = DUPLICATE_EXTENTS_DATA {
            FileHandle: source.as_raw_handle(),
            SourceFileOffset: offset as i64,
            TargetFileOffset: offset as i64,
            ByteCount: step.min(end - offset) as i64,
        };
        // SAFETY: this control code reads this input type; both handles are live.
        unsafe {
            control(
                &destination,
                FSCTL_DUPLICATE_EXTENTS_TO_FILE,
                &extents,
                &mut (),
            )?
        };
    }
    Ok(())
}

/// # Safety
/// The control code must read `I` and write `O`.
#[cfg(windows)]
unsafe fn control<I, O>(
    file: &std::fs::File,
    code: u32,
    input: &I,
    output: &mut O,
) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let mut returned = 0;
    // SAFETY: the handle is live and both buffers stay valid for their sizes.
    let result = unsafe {
        windows_sys::Win32::System::IO::DeviceIoControl(
            file.as_raw_handle(),
            code,
            std::ptr::from_ref(input).cast(),
            size_of::<I>() as u32,
            std::ptr::from_mut(output).cast(),
            size_of::<O>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn clone_path(_: &Path, _: &Path) -> Result<()> {
    bail!("native cloning requires macOS, Linux or Windows")
}

/// Discover effective creation permissions without changing the process-global umask.
pub struct FileModes {
    regular: std::fs::Permissions,
    executable: std::fs::Permissions,
}

impl FileModes {
    pub fn new(stage: &Path) -> Result<Self> {
        Ok(Self {
            regular: creation_permissions(&stage.join("regular-mode"), 0o666)?,
            executable: creation_permissions(&stage.join("executable-mode"), 0o777)?,
        })
    }

    pub fn apply_directory(&self, path: &Path) -> Result<()> {
        std::fs::set_permissions(path, self.executable.clone())?;
        Ok(())
    }

    pub fn apply(&self, path: &Path, executable: bool) -> Result<()> {
        let permissions = if executable {
            &self.executable
        } else {
            &self.regular
        };
        std::fs::set_permissions(path, permissions.clone())?;
        Ok(())
    }
}

fn creation_permissions(path: &Path, mode: u32) -> Result<std::fs::Permissions> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(options.open(path)?.metadata()?.permissions())
}

pub fn symlink(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, destination)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        bail!("symlinks require Unix")
    }
}

pub fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

pub fn os_string(bytes: Vec<u8>) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes)
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::symlink,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn reflink_rejects_links_special_files_and_existing_destinations() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-fixtures")
            .join(format!(
                "native-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&root)?;
        let source = root.join("source");
        let destination = root.join("destination");
        fs::write(&source, "source bytes")?;
        fs::write(&destination, "keep destination")?;
        assert!(clone_path(&source, &destination).is_err());
        assert_eq!(fs::read(&destination)?, b"keep destination");
        symlink(&source, root.join("link"))?;
        assert!(clone_path(&root.join("link"), &root.join("link-copy")).is_err());
        assert!(!root.join("link-copy").exists());
        assert!(clone_path(&root, &root.join("directory-copy")).is_err());
        assert!(!root.join("directory-copy").exists());
        // A tracked regular file replaced by a FIFO must fail without blocking.
        let fifo = std::ffi::CString::new(path_bytes(&root.join("fifo")))?;
        // SAFETY: fifo is a live NUL-terminated path; mode is a valid permission mask.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(clone_path(&root.join("fifo"), &root.join("fifo-copy")).is_err());
        assert!(!root.join("fifo-copy").exists());
        assert_eq!(fs::read(&source)?, b"source bytes");
        eprintln!("fixture retained: {}", root.display());
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn block_clone_keeps_partial_clusters_and_existing_destinations() -> Result<()> {
        let root = std::env::var_os("TWIG_TEST_FIXTURE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixtures")
            })
            .join(format!(
                "native-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
        fs::create_dir_all(&root)?;
        let source = root.join("source");
        // Longer than a 64 KiB cluster, and ends inside a cluster.
        let bytes: Vec<u8> = (0..70_001u32).map(|index| (index % 251) as u8).collect();
        fs::write(&source, &bytes)?;
        let destination = root.join("destination");
        fs::write(&destination, "keep destination")?;
        assert!(clone_path(&source, &destination).is_err());
        assert_eq!(fs::read(&destination)?, b"keep destination");
        assert!(clone_path(&root, &root.join("directory-copy")).is_err());
        assert!(!root.join("directory-copy").exists());
        let copy = root.join("copy");
        match clone_path(&source, &copy) {
            Ok(()) => {
                assert_eq!(fs::read(&copy)?, bytes);
                fs::write(&copy, "private")?;
            }
            Err(error) => {
                assert!(
                    std::env::var_os("TWIG_TEST_REQUIRE_NATIVE").is_none(),
                    "{error:#}"
                );
                assert_eq!(error.to_string(), "volume does not support block cloning");
                assert!(!copy.exists());
            }
        }
        assert_eq!(fs::read(&source)?, bytes);
        eprintln!("fixture retained: {}", root.display());
        Ok(())
    }
}
