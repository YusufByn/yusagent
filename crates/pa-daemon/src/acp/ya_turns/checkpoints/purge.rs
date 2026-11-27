//! yusAgent (YA-18, ticket 43, PR 3a): the cleanup of the workspace snapshots
//! (`docs/tickets/43-retour-arriere.md`, §P.6): once per bridge process, in the background, refs
//! older than 30 days go, a repository whose workspace no longer exists goes, and so does one left
//! with no ref after the cleanup (never the current workspace's); `gc --prune=now` after deletions.

use std::path::{Path, PathBuf};

use super::git::{self, Git};

const KEPT_SECONDS: u64 = 30 * 24 * 3600;

pub(super) async fn run(root: PathBuf, current: PathBuf) {
    let Ok(program) = git::program() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let cutoff = now.saturating_sub(KEPT_SECONDS);
    for repo in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        let Ok(named) = std::fs::read_to_string(repo.join("workspace")) else {
            continue;
        };
        let workspace = PathBuf::from(named.trim());
        if workspace == current {
            purge_refs(&program, &repo, &workspace, cutoff).await;
            continue;
        }
        if !workspace.exists() {
            let _ = std::fs::remove_dir_all(&repo);
            continue;
        }
        if purge_refs(&program, &repo, &workspace, cutoff).await == Some(0) {
            let _ = std::fs::remove_dir_all(&repo);
        }
    }
}

/// The refs older than `cutoff` deleted; the refs left, when any were deleted.
async fn purge_refs(program: &Path, repo: &Path, work: &Path, cutoff: u64) -> Option<usize> {
    let git = Git {
        program,
        repo,
        work,
    };
    let listed = git
        .run(
            None,
            &[
                "for-each-ref",
                "--format=%(refname) %(committerdate:unix)",
                "refs/ya/",
            ],
            None,
        )
        .await
        .ok()?;
    let listed = String::from_utf8_lossy(&listed).into_owned();
    let mut old = String::new();
    let mut left = 0;
    for line in listed.lines() {
        let Some((name, date)) = line.rsplit_once(' ') else {
            continue;
        };
        if date.parse::<u64>().is_ok_and(|date| date < cutoff) {
            old.push_str("delete ");
            old.push_str(name);
            old.push('\n');
        } else {
            left += 1;
        }
    }
    if old.is_empty() {
        return None;
    }
    git.run(None, &["update-ref", "--stdin"], Some(old.as_bytes()))
        .await
        .ok()?;
    let _ = git.run(None, &["gc", "--prune=now", "--quiet"], None).await;
    Some(left)
}
