//! yusAgent (YA-03, ticket 09): the workspace of a session, the one definition of where the
//! harness's own edit tools may write.
//!
//! [`allowed_roots`] is the only place that says which directories are allowed (today the
//! session cwd alone); YA-11 (sandbox) and a multi-folder workspace extend [`WorkspaceScope`]
//! and this function, never a second definition.
//!
//! [`check_inside`] resolves a path the way the filesystem will: the longest existing prefix is
//! canonicalized (every symbolic link followed), the part that does not exist yet is appended
//! as is. A path whose resolution leaves every root, or that goes through a dangling link, is
//! refused. The check runs before the operation and is not atomic: a link created between the
//! check and the write is not covered, nor are hard links. It is a guard against mistakes, not
//! a security boundary (`bash()` and `edit()` write anywhere until YA-11).

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Stable marker at the start of a refusal; `bench/smoke/metrics.py` counts the tool results
/// that start with it.
pub const OUTSIDE_WORKSPACE_MARKER: &str = "[outside-workspace]";

/// What defines the workspace of one session.
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceScope<'a> {
    /// The session's working directory.
    pub session_cwd: &'a Path,
}

/// The directories the session's edit tools may write under, canonicalized: today the session
/// cwd alone.
///
/// # Errors
///
/// Fails when a root cannot be canonicalized (it does not exist, or is not readable).
pub fn allowed_roots(scope: &WorkspaceScope<'_>) -> io::Result<Vec<PathBuf>> {
    Ok(vec![std::fs::canonicalize(scope.session_cwd)?])
}

/// A path that resolves outside every allowed root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutsideWorkspace {
    /// The path as the tool received it (absolute, lexically normalized).
    pub path: PathBuf,
    /// Why it is refused.
    pub reason: String,
}

impl fmt::Display for OutsideWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{OUTSIDE_WORKSPACE_MARKER} {}: {}",
            self.path.display(),
            self.reason
        )
    }
}

impl std::error::Error for OutsideWorkspace {}

/// Whether the absolute `path` resolves under one of `roots` (from [`allowed_roots`]).
///
/// # Errors
///
/// [`OutsideWorkspace`] when the resolved path is under no root, when it goes through a
/// dangling symbolic link, or when it cannot be resolved.
pub fn check_inside(path: &Path, roots: &[PathBuf]) -> Result<(), OutsideWorkspace> {
    let refuse = |reason: String| OutsideWorkspace {
        path: path.to_path_buf(),
        reason,
    };
    if !path.is_absolute() {
        return Err(refuse("the path is not absolute".to_string()));
    }
    let resolved = resolve(path).map_err(refuse)?;
    if roots.iter().any(|root| resolved.starts_with(root)) {
        Ok(())
    } else {
        let roots = roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Err(refuse(format!(
            "resolves to {}, outside the workspace ({roots})",
            resolved.display()
        )))
    }
}

/// `path` with its longest existing prefix canonicalized and the missing rest appended.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    let mut existing = path;
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", existing.display())),
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name);
                existing = parent;
            }
            _ => return Err("no existing ancestor".to_string()),
        }
    }
    let mut resolved = std::fs::canonicalize(existing).map_err(|error| {
        format!(
            "{} cannot be resolved (dangling symbolic link?): {error}",
            existing.display()
        )
    })?;
    for name in missing.iter().rev() {
        resolved.push(name);
    }
    // The missing part comes from a lexically normalized path; refuse anything else.
    if resolved
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("the path is not normalized".to_string());
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dir: &Path) -> Vec<PathBuf> {
        allowed_roots(&WorkspaceScope { session_cwd: dir }).unwrap()
    }

    #[test]
    fn the_session_cwd_is_the_only_root_and_is_canonical() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        assert_eq!(roots, vec![std::fs::canonicalize(dir.path()).unwrap()]);
    }

    #[test]
    fn existing_and_new_paths_inside_are_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let roots = roots(dir.path());
        // `tempdir` may sit behind a link (`/var` -> `/private/var` on macOS): both spellings work.
        for base in [
            dir.path().to_path_buf(),
            std::fs::canonicalize(dir.path()).unwrap(),
        ] {
            check_inside(&base.join("a.txt"), &roots).unwrap();
            check_inside(&base.join("new/deeper/b.txt"), &roots).unwrap();
        }
    }

    #[test]
    fn absolute_path_outside_is_refused_with_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let error = check_inside(&other.path().join("x.txt"), &roots(dir.path())).unwrap_err();
        let text = error.to_string();
        assert!(text.starts_with(OUTSIDE_WORKSPACE_MARKER), "{text}");
        assert!(text.contains("outside the workspace"), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn link_to_a_directory_outside_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("out")).unwrap();
        let roots = roots(dir.path());
        assert!(check_inside(&dir.path().join("out/new.txt"), &roots).is_err());
        std::fs::write(other.path().join("f.txt"), "x").unwrap();
        assert!(check_inside(&dir.path().join("out/f.txt"), &roots).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn link_to_a_file_outside_is_refused_and_inside_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("f.txt"), "x").unwrap();
        std::fs::write(dir.path().join("g.txt"), "x").unwrap();
        std::os::unix::fs::symlink(other.path().join("f.txt"), dir.path().join("out.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("g.txt"), dir.path().join("in.txt")).unwrap();
        let roots = roots(dir.path());
        assert!(check_inside(&dir.path().join("out.txt"), &roots).is_err());
        check_inside(&dir.path().join("in.txt"), &roots).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/nonexistent/yusagent/target", dir.path().join("dangling"))
            .unwrap();
        let error = check_inside(&dir.path().join("dangling"), &roots(dir.path())).unwrap_err();
        assert!(error.reason.contains("cannot be resolved"), "{error}");
    }

    #[test]
    fn relative_path_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_inside(Path::new("a.txt"), &roots(dir.path())).is_err());
    }
}
