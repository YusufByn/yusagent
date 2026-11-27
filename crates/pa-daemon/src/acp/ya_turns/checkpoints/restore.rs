//! yusAgent (YA-18, ticket 43, PR 3a): the preview and the restore of a workspace snapshot
//! (`docs/tickets/43-retour-arriere.md`, §P.6, §P.12.6).
//! - The preview takes a snapshot of the workspace as it is (into the session's index, no ref):
//!   what a restore would change (`diff-tree` from now to the snapshot before the prompt), what
//!   changed outside the agent since the last turn's end (`diff-tree` from `fin` to now), and what
//!   is not covered. With a path: that file as it was.
//! - The restore keeps the workspace as it is under `avant-restauration/<n>` (answer Q-P6), checks
//!   the changes outside the agent (refused without `force`), then `read-tree -u --reset` on the
//!   snapshot before the prompt; `fin` follows (answer Q-3a-2).

use serde_json::{json, Value};

use super::limits::FILE_CAP;
use super::repo::{fields, text, Repo};

/// Why a preview or a restore does not go on.
pub(super) enum Refusal {
    /// The snapshots are off here, with the message.
    Off(&'static str),
    /// No snapshot before that prompt.
    NoSnapshot,
    /// Files changed outside the agent since the last turn's end.
    Modified(Vec<String>),
    Failed(String),
}

fn start_ref(session: &str, to: &str) -> String {
    format!("refs/ya/{session}/debut/{to}")
}

impl Repo {
    /// The workspace as it is: a snapshot into the session's index, under `refname` if given.
    async fn now(&mut self, session: &str, refname: Option<&str>) -> Result<String, Refusal> {
        match self.snapshot(session, refname, "before a restore").await {
            Ok(Some(tree)) => Ok(tree),
            Ok(None) => Err(Refusal::Off(self.disabled.unwrap_or(super::git::NO_GIT))),
            Err(error) => Err(Refusal::Failed(error)),
        }
    }

    /// The paths changed outside the agent: from the last turn's end to `now`.
    async fn outside(&self, session: &str, now: &str) -> Result<Vec<String>, Refusal> {
        let Some(end) = self
            .resolve(&format!("refs/ya/{session}/fin^{{tree}}"))
            .await
        else {
            return Ok(Vec::new());
        };
        self.git()
            .run(
                None,
                &["diff-tree", "-r", "-z", "--name-only", &end, now],
                None,
            )
            .await
            .map(|names| fields(&names))
            .map_err(Refusal::Failed)
    }

    /// `_yus/session/restore_preview` without a path.
    pub(super) async fn preview(&mut self, session: &str, to: &str) -> Result<Value, Refusal> {
        if let Some(reason) = self.disabled {
            return Err(Refusal::Off(reason));
        }
        let Some(start) = self
            .resolve(&format!("{}^{{tree}}", start_ref(session, to)))
            .await
        else {
            return Err(Refusal::NoSnapshot);
        };
        let now = self.now(session, None).await?;
        let listed = self
            .git()
            .run(
                None,
                &[
                    "diff-tree",
                    "-r",
                    "-z",
                    "--name-status",
                    "--no-renames",
                    &now,
                    &start,
                ],
                None,
            )
            .await
            .map_err(Refusal::Failed)?;
        let fields = fields(&listed);
        let files: Vec<Value> = fields
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| json!({ "status": pair[0], "path": pair[1] }))
            .collect();
        let modified = self.outside(session, &now).await?;
        Ok(json!({
            "available": true,
            "files": files,
            "modifiedOutside": modified,
            "uncovered": self.uncovered(),
        }))
    }

    /// `_yus/session/restore_preview` with a path: the file as it was before the prompt.
    pub(super) async fn before(
        &self,
        session: &str,
        to: &str,
        path: &str,
    ) -> Result<Value, Refusal> {
        let Some(start) = self.resolve(&start_ref(session, to)).await else {
            return Err(Refusal::NoSnapshot);
        };
        let object = format!("{start}:{path}");
        let Ok(size) = self
            .git()
            .run(None, &["cat-file", "-s", &object], None)
            .await
        else {
            return Ok(json!({ "before": null, "binary": false }));
        };
        if text(&size).parse::<u64>().unwrap_or(u64::MAX) > FILE_CAP {
            return Ok(json!({ "before": null, "binary": true }));
        }
        let content = self
            .git()
            .run(None, &["cat-file", "blob", &object], None)
            .await
            .map_err(Refusal::Failed)?;
        if content.iter().take(8000).any(|byte| *byte == 0) {
            return Ok(json!({ "before": null, "binary": true }));
        }
        Ok(json!({ "before": String::from_utf8_lossy(&content), "binary": false }))
    }

    /// `_yus/session/restore` of the files: `{ restored, uncovered }`.
    pub(super) async fn restore(
        &mut self,
        session: &str,
        to: &str,
        force: bool,
    ) -> Result<Value, Refusal> {
        if let Some(reason) = self.disabled {
            return Err(Refusal::Off(reason));
        }
        let Some(start) = self.resolve(&start_ref(session, to)).await else {
            return Err(Refusal::NoSnapshot);
        };
        let kept = self
            .refs(&format!("refs/ya/{session}/avant-restauration/"))
            .await
            .len()
            + 1;
        let kept_ref = format!("refs/ya/{session}/avant-restauration/{kept}");
        let now = self.now(session, Some(kept_ref.as_str())).await?;
        let modified = self.outside(session, &now).await?;
        if !modified.is_empty() && !force {
            return Err(Refusal::Modified(modified));
        }
        let changed = self
            .git()
            .run(
                None,
                &["diff-tree", "-r", "-z", "--name-only", &now, &start],
                None,
            )
            .await
            .map_err(Refusal::Failed)?;
        let index = self.index(session);
        self.git()
            .run(Some(&index), &["read-tree", "-u", "--reset", &start], None)
            .await
            .map_err(Refusal::Failed)?;
        self.git()
            .run(
                None,
                &["update-ref", &format!("refs/ya/{session}/fin"), &start],
                None,
            )
            .await
            .map_err(Refusal::Failed)?;
        Ok(json!({ "restored": fields(&changed), "uncovered": self.uncovered() }))
    }
}
