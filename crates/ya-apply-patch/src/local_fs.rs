// Added by yusAgent: not part of codex-rs/apply-patch. Synchronous `std::fs`
// replacement for the `codex_exec_server::ExecutorFileSystem` operations the
// crate used (read_file_text, write_file, create_directory, get_metadata,
// remove), and for the lexical `PathUri::join` used to resolve patch paths.
//
// `follow_symlinks: false` mirrors Codex's no-follow semantics (no symbolic
// link may appear in any component of the path) with `symlink_metadata`
// checks before each operation. Codex binds every component with
// `openat(O_NOFOLLOW)`; this check-then-act version is not atomic and does
// not protect against a concurrent swap between the check and the operation.

use std::io;
use std::io::Read;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Same ceiling as Codex's local file reads.
const MAX_READ_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The subset of `codex_exec_server::FileMetadata` the crate reads.
pub(crate) struct FileMetadata {
    pub(crate) is_directory: bool,
    pub(crate) is_file: bool,
    pub(crate) is_symlink: bool,
}

/// Joins `path` onto `base`, then resolves `.` and `..` lexically without
/// escaping the root, as `PathUri::join` does. An absolute `path` replaces
/// `base`.
pub(crate) fn join(base: &Path, path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in base.join(path).components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
        }
    }
    normalized
}

pub(crate) fn read_file_text(path: &Path, follow_symlinks: bool) -> io::Result<String> {
    let bytes = read_file(path, follow_symlinks)?;
    String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn read_file(path: &Path, follow_symlinks: bool) -> io::Result<Vec<u8>> {
    if follow_symlinks {
        // Checked before opening so a FIFO is never opened for reading.
        if !std::fs::metadata(path)?.is_file() {
            return Err(not_a_file(path));
        }
    } else {
        reject_symlinked_ancestors(path)?;
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_symlink() {
            return Err(symlink_error());
        }
        if !metadata.is_file() {
            return Err(not_a_regular_file());
        }
    }
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(not_a_file(path));
    }
    if metadata.len() > MAX_READ_FILE_BYTES {
        return Err(file_too_large_error());
    }
    let mut bytes = Vec::new();
    file.take(MAX_READ_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_READ_FILE_BYTES {
        return Err(file_too_large_error());
    }
    Ok(bytes)
}

pub(crate) fn write_file(path: &Path, contents: &[u8], follow_symlinks: bool) -> io::Result<()> {
    if !follow_symlinks {
        reject_symlinked_ancestors(path)?;
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_symlink() => return Err(symlink_error()),
            Ok(metadata) if !metadata.is_file() => return Err(not_a_regular_file()),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    std::fs::write(path, contents)
}

/// Recursive directory creation (the only mode the crate uses).
pub(crate) fn create_dir_all(path: &Path, follow_symlinks: bool) -> io::Result<()> {
    if follow_symlinks {
        return std::fs::create_dir_all(path);
    }
    require_normalized_absolute(path)?;
    for directory in top_down_ancestors(path).chain(std::iter::once(path)) {
        match std::fs::symlink_metadata(directory) {
            Ok(metadata) => ensure_plain_directory(&metadata)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                match std::fs::create_dir(directory) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(err) => return Err(err),
                }
                ensure_plain_directory(&std::fs::symlink_metadata(directory)?)?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub(crate) fn get_metadata(path: &Path, follow_symlinks: bool) -> io::Result<FileMetadata> {
    if !follow_symlinks {
        reject_symlinked_ancestors(path)?;
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_symlink() {
            return Err(symlink_error());
        }
        return Ok(FileMetadata {
            is_directory: metadata.is_dir(),
            is_file: metadata.is_file(),
            is_symlink: false,
        });
    }
    let symlink_metadata = std::fs::symlink_metadata(path)?;
    let is_symlink = symlink_metadata.is_symlink();
    let metadata = if is_symlink {
        std::fs::metadata(path)?
    } else {
        symlink_metadata
    };
    Ok(FileMetadata {
        is_directory: metadata.is_dir(),
        is_file: metadata.is_file(),
        is_symlink,
    })
}

/// Non-recursive, non-forced removal (the only mode the crate uses).
pub(crate) fn remove(path: &Path, follow_symlinks: bool) -> io::Result<()> {
    if !follow_symlinks {
        reject_symlinked_ancestors(path)?;
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !follow_symlinks && metadata.is_symlink() {
        return Err(symlink_error());
    }
    if metadata.file_type().is_dir() {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Fails when a proper ancestor of `path` is a symbolic link or not a
/// directory. A missing ancestor yields `NotFound`, like Codex's `openat`.
fn reject_symlinked_ancestors(path: &Path) -> io::Result<()> {
    require_normalized_absolute(path)?;
    for ancestor in top_down_ancestors(path) {
        ensure_plain_directory(&std::fs::symlink_metadata(ancestor)?)?;
    }
    Ok(())
}

/// The proper ancestors of `path` below the root, outermost first.
fn top_down_ancestors(path: &Path) -> impl Iterator<Item = &Path> {
    let mut ancestors: Vec<&Path> = path
        .ancestors()
        .skip(1)
        .filter(|ancestor| ancestor.parent().is_some())
        .collect();
    ancestors.reverse();
    ancestors.into_iter()
}

fn require_normalized_absolute(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no-follow filesystem operations require an absolute path",
        ));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no-follow filesystem operations require a normalized path",
        ));
    }
    Ok(())
}

fn ensure_plain_directory(metadata: &std::fs::Metadata) -> io::Result<()> {
    if metadata.is_symlink() {
        return Err(symlink_error());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path component is not a directory",
        ));
    }
    Ok(())
}

fn symlink_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "path contains a symbolic link")
}

fn not_a_file(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("path `{}` is not a file", path.display()),
    )
}

fn not_a_regular_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "path is not a regular file")
}

fn file_too_large_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("file is too large to read: limit is {MAX_READ_FILE_BYTES} bytes"),
    )
}
