use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub struct UploadRoot {
    declared: PathBuf,
    canonical: PathBuf,
    directory: File,
}
impl UploadRoot {
    pub fn from_env() -> Result<Option<Self>> {
        let Some(root) = std::env::var_os("BEWEFT_UPLOAD_ROOT") else {
            return Ok(None);
        };
        let declared = if Path::new(&root).is_absolute() {
            PathBuf::from(root)
        } else {
            std::env::current_dir()?.join(root)
        };
        let canonical = declared
            .canonicalize()
            .context("Cannot resolve BEWEFT_UPLOAD_ROOT")?;
        let directory = open_root(&canonical)?;
        Ok(Some(Self {
            declared,
            canonical,
            directory,
        }))
    }
}

pub fn open(path: &Path, mcp: bool, root: Option<&UploadRoot>) -> Result<File> {
    let file = if mcp {
        let root = root.context("MCP uploads require BEWEFT_UPLOAD_ROOT")?;
        open_beneath(root, path)?
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        options.open(path).context("Cannot open upload file")?
    };
    ensure!(file.metadata()?.is_file(), "Upload must be a regular file");
    Ok(file)
}

// Resolve components from already-open directories and retain the final file
// descriptor for streaming. A path check followed by an open can be defeated
// by replacing a directory or symlink between those operations.
#[cfg(unix)]
fn open_component(directory: &File, name: &std::ffi::OsStr, is_directory: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let name = CString::new(name.as_bytes()).context("Upload path contains NUL")?;
    let mut flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    if is_directory {
        flags |= libc::O_DIRECTORY;
    }
    // SAFETY: the directory and name remain valid for the call. On success the
    // newly owned descriptor is immediately wrapped in File.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    ensure!(
        fd >= 0,
        "Cannot open upload path without following symlinks"
    );
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn open_root(root: &Path) -> Result<File> {
    use std::path::Component;
    let mut directory = File::open("/")?;
    for component in root.components() {
        if let Component::Normal(name) = component {
            directory = open_component(&directory, name, true)?;
        }
    }
    Ok(directory)
}

#[cfg(unix)]
fn open_beneath(root: &UploadRoot, path: &Path) -> Result<File> {
    use std::path::Component;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let relative = absolute
        .strip_prefix(&root.canonical)
        .or_else(|_| absolute.strip_prefix(&root.declared))
        .context("Upload path is outside BEWEFT_UPLOAD_ROOT")?;
    ensure!(
        !relative
            .components()
            .any(|part| part == Component::ParentDir),
        "Upload path must not contain parent traversal"
    );
    let parts: Vec<_> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    ensure!(!parts.is_empty(), "Upload path needs a file name");
    let mut directory = root.directory.try_clone()?;
    for (index, name) in parts.iter().enumerate() {
        directory = open_component(&directory, name, index + 1 < parts.len())?;
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open_root(_root: &Path) -> Result<File> {
    anyhow::bail!("MCP upload confinement requires a Unix host")
}
#[cfg(not(unix))]
fn open_beneath(_root: &UploadRoot, _path: &Path) -> Result<File> {
    anyhow::bail!("MCP upload confinement requires a Unix host")
}
