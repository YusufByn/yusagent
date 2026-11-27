//! yusAgent (YA-18, ticket 43, PR 3a): what a snapshot covers (`docs/tickets/43-retour-arriere.md`,
//! §P.3, answer Q-P3). A file over `8 MiB` is not covered (neither captured nor removed by a
//! restore, listed as not covered); a workspace over 20 000 covered files or `512 MiB` has its
//! snapshots turned off, with a message. The count and the size come from a full listing at the
//! first snapshot of a workspace in the process, then from the changed paths of each snapshot.

use std::collections::BTreeSet;
use std::path::Path;

/// The cap per file.
pub(super) const FILE_CAP: u64 = 8 * 1024 * 1024;
const MAX_FILES: u64 = 20_000;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const TOO_MANY_FILES: &str =
    "Workspace snapshots are off in this folder: more than 20000 files.";
pub(super) const TOO_LARGE: &str =
    "Workspace snapshots are off in this folder: more than 512 MiB of files.";
/// Excluded when the workspace has no `.gitignore` at its root (§P.3).
pub(super) const DEFAULT_EXCLUDES: [&str; 19] = [
    "node_modules/",
    "target/",
    ".venv/",
    "venv/",
    "__pycache__/",
    ".mypy_cache/",
    ".pytest_cache/",
    ".ruff_cache/",
    ".tox/",
    ".gradle/",
    ".next/",
    ".nuxt/",
    ".cache/",
    "dist/",
    "build/",
    "coverage/",
    ".DS_Store",
    "*.pyc",
    ".git/",
];

/// The covered files of a workspace and the ones over the cap per file.
#[derive(Debug, Default)]
pub(super) struct Limits {
    /// The count and the size come from a full listing (then they are kept up to date).
    pub(super) known: bool,
    pub(super) files: u64,
    pub(super) bytes: u64,
    pub(super) uncovered: BTreeSet<String>,
}

impl Limits {
    /// The message when a workspace cap is passed.
    pub(super) fn over(&self) -> Option<&'static str> {
        if self.files > MAX_FILES {
            Some(TOO_MANY_FILES)
        } else if self.bytes > MAX_BYTES {
            Some(TOO_LARGE)
        } else {
            None
        }
    }
}

/// The size of `path` under `work` (a link's own size), `None` when it is gone or a directory.
pub(super) fn size(work: &Path, path: &str) -> Option<u64> {
    std::fs::symlink_metadata(work.join(path))
        .ok()
        .filter(|metadata| !metadata.is_dir())
        .map(|metadata| metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cap_passed_gives_its_message() {
        let mut limits = Limits {
            known: true,
            files: MAX_FILES,
            bytes: MAX_BYTES,
            ..Limits::default()
        };
        assert_eq!(limits.over(), None);
        limits.files += 1;
        assert_eq!(limits.over(), Some(TOO_MANY_FILES));
        limits.files = 1;
        limits.bytes += 1;
        assert_eq!(limits.over(), Some(TOO_LARGE));
    }
}
