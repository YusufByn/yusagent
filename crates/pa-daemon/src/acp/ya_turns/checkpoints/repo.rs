//! yusAgent (YA-18, ticket 43, PR 3a): one workspace's shadow repository
//! (`docs/tickets/43-retour-arriere.md`, §P.4, §P.12.3): a bare repository under
//! `<agent>/checkpoints/<digest>/`, a `workspace` file naming the workspace, one index per session
//! (`ya-index/<session>`, a new one copied from the latest), an `info/exclude` written by the
//! bridge (the agent directory, the nested repositories, the default list without a root
//! `.gitignore`), and the snapshots: `add -A`, the changed paths read back for the caps, then a
//! commit and its ref.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::git::{self, Git};
use super::limits::{self, Limits, DEFAULT_EXCLUDES, FILE_CAP};

/// One workspace's shadow repository and what the bridge knows of it in this process.
pub(super) struct Repo {
    program: PathBuf,
    pub(super) dir: PathBuf,
    work: PathBuf,
    agent: PathBuf,
    nested: BTreeSet<String>,
    pub(super) limits: Limits,
    /// Why the snapshots are off in this workspace (a cap passed), told once.
    pub(super) disabled: Option<&'static str>,
    prepared: bool,
}

/// One line of `diff-index --raw -z`.
struct Change {
    src_mode: String,
    dst_mode: String,
    src_id: String,
    status: char,
    path: String,
}

/// The NUL-separated fields of `bytes`.
pub(super) fn fields(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect()
}

/// The trimmed text of `bytes`.
pub(super) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn changes(raw: &[u8]) -> Vec<Change> {
    let fields = fields(raw);
    let mut out = Vec::new();
    let mut iter = fields.iter();
    while let (Some(meta), Some(path)) = (iter.next(), iter.next()) {
        let parts: Vec<&str> = meta.trim_start_matches(':').split_whitespace().collect();
        if let [src_mode, dst_mode, src_id, _, status, ..] = parts.as_slice() {
            out.push(Change {
                src_mode: (*src_mode).to_string(),
                dst_mode: (*dst_mode).to_string(),
                src_id: (*src_id).to_string(),
                status: status.chars().next().unwrap_or('M'),
                path: path.clone(),
            });
        }
    }
    out
}

impl Repo {
    pub(super) fn new(program: PathBuf, dir: PathBuf, work: PathBuf, agent: PathBuf) -> Self {
        Repo {
            program,
            dir,
            work,
            agent,
            nested: BTreeSet::new(),
            limits: Limits::default(),
            disabled: None,
            prepared: false,
        }
    }

    pub(super) fn git(&self) -> Git<'_> {
        Git {
            program: &self.program,
            repo: &self.dir,
            work: &self.work,
        }
    }

    pub(super) fn index(&self, session: &str) -> PathBuf {
        self.dir.join("ya-index").join(session)
    }

    /// The files not covered: over the cap per file, and the nested repositories.
    pub(super) fn uncovered(&self) -> Vec<String> {
        self.limits
            .uncovered
            .iter()
            .chain(self.nested.iter())
            .cloned()
            .collect()
    }

    /// Turns the snapshots off in this workspace, the message told once on the bridge's stderr.
    pub(super) fn disable(&mut self, reason: &'static str) {
        if self.disabled.is_none() {
            eprintln!("yusagent: {reason}");
        }
        self.disabled = Some(reason);
    }

    async fn prepare(&mut self, session: &str) -> Result<(), String> {
        if !self.prepared {
            if !self.dir.join("HEAD").exists() {
                if let Some(parent) = self.dir.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                create_private_dir(&self.dir)?;
                git::init(&self.program, &self.dir).await?;
                if cfg!(target_os = "macos") {
                    self.git()
                        .run(None, &["config", "core.ignorecase", "true"], None)
                        .await?;
                    self.git()
                        .run(None, &["config", "core.precomposeunicode", "true"], None)
                        .await?;
                }
            }
            let named = format!("{}\n", self.work.display());
            if std::fs::read_to_string(self.dir.join("workspace"))
                .ok()
                .as_deref()
                != Some(named.as_str())
            {
                std::fs::write(self.dir.join("workspace"), named)
                    .map_err(|error| error.to_string())?;
            }
            if let Ok(listed) = std::fs::read_to_string(self.dir.join("ya-nested")) {
                self.nested.extend(
                    listed
                        .lines()
                        .filter(|line| !line.is_empty())
                        .map(str::to_string),
                );
            }
            self.prepared = true;
        }
        self.write_exclude()?;
        let index = self.index(session);
        if !index.exists() {
            let indexes = self.dir.join("ya-index");
            std::fs::create_dir_all(&indexes).map_err(|error| error.to_string())?;
            let latest = std::fs::read_dir(&indexes)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    Some((modified, entry.path()))
                })
                .max_by_key(|(modified, _)| *modified);
            if let Some((_, latest)) = latest {
                let _ = std::fs::copy(latest, &index);
            }
        }
        Ok(())
    }

    /// `info/exclude`, rewritten when it changes.
    fn write_exclude(&self) -> Result<(), String> {
        let mut lines = vec!["# yusAgent (ticket 43): written by the bridge.".to_string()];
        if let Ok(relative) = self.agent.strip_prefix(&self.work) {
            if !relative.as_os_str().is_empty() {
                lines.push(format!("/{}/", relative.display()));
            }
        }
        lines.extend(self.nested.iter().map(|nested| format!("/{nested}/")));
        if !self.work.join(".gitignore").exists() {
            lines.extend(
                DEFAULT_EXCLUDES
                    .iter()
                    .map(|pattern| (*pattern).to_string()),
            );
        }
        let content = lines.join("\n") + "\n";
        let info = self.dir.join("info");
        let path = info.join("exclude");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
            std::fs::create_dir_all(&info).map_err(|error| error.to_string())?;
            std::fs::write(&path, content).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// A snapshot of the workspace into `session`'s index, then under `refname` when given: its
    /// tree, or `None` when the snapshots are off here.
    pub(super) async fn snapshot(
        &mut self,
        session: &str,
        refname: Option<&str>,
        message: &str,
    ) -> Result<Option<String>, String> {
        if self.disabled.is_some() {
            return Ok(None);
        }
        self.prepare(session).await?;
        let index = self.index(session);
        let incremental = self.limits.known;
        if !incremental {
            self.list(&index).await?;
            if let Some(reason) = self.limits.over() {
                self.disable(reason);
                return Ok(None);
            }
        }
        let previous = text(&self.git().run(Some(&index), &["write-tree"], None).await?);
        let mut add = vec![
            "add".to_string(),
            "-A".to_string(),
            "--".to_string(),
            ".".to_string(),
        ];
        add.extend(
            self.limits
                .uncovered
                .iter()
                .map(|path| format!(":(exclude,literal){path}")),
        );
        let add: Vec<&str> = add.iter().map(String::as_str).collect();
        self.git().run(Some(&index), &add, None).await?;
        let raw = self
            .git()
            .run(
                Some(&index),
                &[
                    "diff-index",
                    "--cached",
                    "-z",
                    "--raw",
                    "--no-renames",
                    &previous,
                ],
                None,
            )
            .await?;
        self.account(&index, &changes(&raw), incremental).await?;
        if let Some(reason) = self.limits.over() {
            self.disable(reason);
            return Ok(None);
        }
        let tree = text(&self.git().run(Some(&index), &["write-tree"], None).await?);
        if let Some(refname) = refname {
            let commit = text(
                &self
                    .git()
                    .run(None, &["commit-tree", &tree, "-m", message], None)
                    .await?,
            );
            self.git()
                .run(None, &["update-ref", refname, &commit], None)
                .await?;
        }
        Ok(Some(tree))
    }

    /// The full listing of the candidates (the first snapshot of the workspace in the process).
    async fn list(&mut self, index: &Path) -> Result<(), String> {
        let listed = self
            .git()
            .run(
                Some(index),
                &[
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ],
                None,
            )
            .await?;
        let mut limits = Limits {
            known: true,
            ..Limits::default()
        };
        let mut nested = false;
        for path in fields(&listed) {
            // An untracked nested repository is listed as its directory.
            if let Some(directory) = path.strip_suffix('/') {
                nested |= self.nested.insert(directory.to_string());
                continue;
            }
            let Some(size) = limits::size(&self.work, &path) else {
                continue;
            };
            if size > FILE_CAP {
                limits.uncovered.insert(path);
            } else {
                limits.files += 1;
                limits.bytes += size;
            }
        }
        self.limits = limits;
        if nested {
            self.remember_nested()?;
        }
        Ok(())
    }

    /// The changed paths of a snapshot: nested repositories and files over the cap per file
    /// leave the index; the count and the size follow (when they are known from a listing).
    async fn account(
        &mut self,
        index: &Path,
        changes: &[Change],
        incremental: bool,
    ) -> Result<(), String> {
        let mut removed = Vec::new();
        let mut old_ids = Vec::new();
        let mut nested = false;
        for change in changes {
            if change.src_mode != "160000" && change.status != 'A' && incremental {
                old_ids.push(change.src_id.clone());
            }
            if change.dst_mode == "160000" {
                nested |= self.nested.insert(change.path.clone());
                removed.push(change.path.clone());
                continue;
            }
            if change.status == 'D' {
                if incremental && change.src_mode != "160000" {
                    self.limits.files = self.limits.files.saturating_sub(1);
                }
                continue;
            }
            let Some(size) = limits::size(&self.work, &change.path) else {
                continue;
            };
            if size > FILE_CAP {
                self.limits.uncovered.insert(change.path.clone());
                removed.push(change.path.clone());
                if incremental && change.status != 'A' {
                    self.limits.files = self.limits.files.saturating_sub(1);
                }
                continue;
            }
            if incremental {
                self.limits.bytes += size;
                if change.status == 'A' {
                    self.limits.files += 1;
                }
            }
        }
        if !old_ids.is_empty() {
            let input = old_ids.join("\n") + "\n";
            let sizes = self
                .git()
                .run(
                    None,
                    &["cat-file", "--batch-check=%(objectsize)"],
                    Some(input.as_bytes()),
                )
                .await?;
            let old: u64 = String::from_utf8_lossy(&sizes)
                .lines()
                .filter_map(|line| line.trim().parse::<u64>().ok())
                .sum();
            self.limits.bytes = self.limits.bytes.saturating_sub(old);
        }
        if !removed.is_empty() {
            let input = removed.join("\0");
            self.git()
                .run(
                    Some(index),
                    &[
                        "--literal-pathspecs",
                        "rm",
                        "--cached",
                        "-q",
                        "-r",
                        "--ignore-unmatch",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ],
                    Some(input.as_bytes()),
                )
                .await?;
        }
        if nested {
            self.remember_nested()?;
            self.write_exclude()?;
        }
        // A file back under the cap leaves the set: the next snapshot takes it.
        let work = self.work.clone();
        self.limits
            .uncovered
            .retain(|path| limits::size(&work, path).is_some_and(|size| size > FILE_CAP));
        Ok(())
    }

    fn remember_nested(&self) -> Result<(), String> {
        let listed: String = self.nested.iter().map(|path| format!("{path}\n")).collect();
        std::fs::write(self.dir.join("ya-nested"), listed).map_err(|error| error.to_string())
    }

    /// The object `rev` names, if any.
    pub(super) async fn resolve(&self, rev: &str) -> Option<String> {
        self.git()
            .run(None, &["rev-parse", "-q", "--verify", rev], None)
            .await
            .ok()
            .map(|id| text(&id))
            .filter(|id| !id.is_empty())
    }

    /// The refs under `prefix`.
    pub(super) async fn refs(&self, prefix: &str) -> Vec<String> {
        self.git()
            .run(None, &["for-each-ref", "--format=%(refname)", prefix], None)
            .await
            .map(|listed| {
                String::from_utf8_lossy(&listed)
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_diff_gives_each_change() {
        let raw = b":100644 100644 aaaa bbbb M\0a.txt\0:000000 160000 0000 cccc A\0sub\0:100644 000000 dddd 0000 D\0gone b.txt\0";
        let parsed = changes(raw);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            (
                parsed[0].status,
                parsed[0].path.as_str(),
                parsed[0].src_id.as_str()
            ),
            ('M', "a.txt", "aaaa")
        );
        assert_eq!(
            (parsed[1].dst_mode.as_str(), parsed[1].status),
            ("160000", 'A')
        );
        assert_eq!(
            (
                parsed[2].status,
                parsed[2].path.as_str(),
                parsed[2].src_mode.as_str()
            ),
            ('D', "gone b.txt", "100644")
        );
    }
}
