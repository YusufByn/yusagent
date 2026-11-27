//! yusAgent (YA-18, ticket 43, R-b0): the worker side of `_yus/session/rewind` and of the
//! workspace restore, actions of the fork's command [`COMMAND`]
//! (`docs/tickets/43-retour-arriere.md`, §I.3, §I.9, §P.12.5):
//! - `leaf`: `{ leafId, sessionId, cwd }`, the session's leaf, its file's id and its working
//!   directory; the ACP bridge reads it before a prompt's `turnStart` (`rewindTo`) and for the
//!   workspace snapshots;
//! - `rewind { to }`: the conversation goes back to `to`, an entry of the current branch, where
//!   it stood before a prompt. As Unharnessed's `rewind_to` (`compaction_arms.rs:433-449`): the
//!   leaf moves, the engine's context is rebuilt from the branch, what came after stays in the
//!   file on a branch of its own, and the kernel keeps what it holds. The announcement rows
//!   (`ya-resources-changed`) of the abandoned turns come back first (the tracker of what the
//!   model was told lives in memory, Q-I2), then the fixed text (D2). PR 3a, L-b (answer Q-P4,
//!   answer 10 reopened): these rows are written to the session file at the rewind, children of
//!   `to`, without a broadcast, so the leaf is written and survives a resume; the rebuilt context
//!   carries them to the model. The next agent-loop call journals
//!   `invalidation: {"cause": "rewind"}`;
//! - `restored` (PR 3a, answer Q-3a-3): the bridge restored the workspace files only; the row of
//!   Q-P5 is written the same way.
//!
//! Refused while a turn runs or a compaction is going on, and for an entry off the branch. As
//! `navigate_tree` (`worker/commands.rs:199-215`, push 9 of the ticket):
//! - the whole rewind holds upstream's queued-work pause (`hold_queued_work_pause("navigate")`,
//!   TS `acquireQueuedWorkPause`): the runner admits no queued work meanwhile (`worker/turn.rs:59`)
//!   and is woken when the guard drops (`session_input_pause.rs:156-170`); the abort suspension
//!   (`queued_input_suspended`) is never touched;
//! - the goal reloaded from the branch is announced (`goal_update_after_rebuild`) before the
//!   answer, so the client never shows the goal of the abandoned turns.
//!
//! Nothing changes the system prompt or the tools.

use std::sync::Arc;

use pa_core::session_engine::ya_resource_notice::rewind::{
    FILES_RESTORED, RESTORED_TEXT, RESTORED_TYPE,
};
use pa_types::session::CustomMessage;
use serde_json::{json, Value};

use super::COMMAND;
use crate::protocol::{response_failure, response_success, DaemonResponse};
use crate::worker::Worker;

/// Unharnessed's refusal while the agent works (`crates/uh-daemon/src/acp/rewind.rs:48-53`).
pub(crate) const WORKING: &str = "The agent is working: stop it before going back.";
/// The end of the refusal for an entry off the branch; the bridge answers it with `-32602`.
pub(crate) const NOT_IN_CONVERSATION: &str = "is not in the conversation";
/// The custom type of the fixed text's row.
pub(crate) const REWOUND_TYPE: &str =
    pa_core::session_engine::ya_resource_notice::rewind::REWOUND_TYPE;
/// The fixed text the model gets after a rewind (D2, validated by Yusuf, ticket 43, §H).
pub(crate) const REWOUND_TEXT: &str = "[conversation-rewound] The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.";

fn custom_row(custom_type: &str, text: String, details: Option<Value>) -> CustomMessage {
    CustomMessage {
        custom_type: custom_type.to_string(),
        content: pa_types::ai::UserContent::Text(text),
        display: true,
        details,
        timestamp: crate::util::now_ms(),
        rest: serde_json::Map::default(),
    }
}

/// The text of a persisted row's content (a string, or text blocks).
fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

/// An announcement entry of the abandoned turns, to ride the next turn again.
fn row_of(fields: &Value) -> CustomMessage {
    custom_row(
        pa_core::session_engine::ya_resource_notice::CUSTOM_TYPE,
        text_of(fields.get("content")),
        fields
            .get("details")
            .filter(|details| !details.is_null())
            .cloned(),
    )
}

/// A row as the worker parks it for the next turn's prefix rows (written to the session file).
fn parked(row: &CustomMessage) -> Option<Value> {
    serde_json::to_value(pa_types::session::AgentMessage::Custom(row.clone())).ok()
}

fn failure(error: &str) -> DaemonResponse {
    response_failure(None, COMMAND, error, None)
}

impl Worker {
    /// The action `leaf`: `{ leafId, sessionId, cwd }`. The session file's id and the session's
    /// working directory serve the bridge's workspace snapshots (ticket 43, PR 3a, §P.12.5).
    pub(crate) fn ya_leaf(&self) -> DaemonResponse {
        let core = self.core.lock().unwrap();
        match core.store.as_ref() {
            Some(store) => response_success(
                None,
                COMMAND,
                Some(json!({
                    "leafId": store.leaf_id(),
                    "sessionId": store.session_id(),
                    "cwd": core.cwd,
                })),
            ),
            None => failure("Session is still initializing"),
        }
    }

    /// The action `rewind { to, files? }`: `files` says the bridge restored the workspace files
    /// too (D2 then carries the files sentence).
    pub(crate) async fn ya_rewind(&self, payload: &Value) -> DaemonResponse {
        let to = payload
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let files = payload
            .get("files")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // Held to the answer: dropped on every return, which wakes the runner.
        let _queued_work = self.hold_queued_work_pause("navigate");
        let (branch, session_id) = {
            let mut guard = self.core.lock().unwrap();
            if guard.busy || guard.compacting {
                return failure(WORKING);
            }
            let (requeued, to_is_rewound) = {
                let Some(store) = guard.store.as_mut() else {
                    return failure("Session is still initializing");
                };
                // The current branch, root to leaf; the abandoned entries come after `to`.
                let (requeued, to_is_rewound) = {
                    let branch = store.branch();
                    let Some(at) = branch.iter().position(|entry| entry.id == to) else {
                        return failure(&format!("{to} {NOT_IN_CONVERSATION}"));
                    };
                    let is_rewound = branch[at].type_ == "custom_message"
                        && branch[at].fields.get("customType").and_then(Value::as_str)
                            == Some(REWOUND_TYPE);
                    let requeued: Vec<CustomMessage> = branch[at + 1..]
                        .iter()
                        .filter(|entry| {
                            entry.type_ == "custom_message"
                                && entry.fields.get("customType").and_then(Value::as_str)
                                    == Some(
                                        pa_core::session_engine::ya_resource_notice::CUSTOM_TYPE,
                                    )
                        })
                        .map(|entry| row_of(&entry.fields))
                        .collect();
                    (requeued, is_rewound)
                };
                if let Err(error) = store.branch_to(Some(to.as_str())) {
                    return failure(&error.to_string());
                }
                (requeued, to_is_rewound)
            };
            // L-b (ticket 43, PR 3a, answer Q-P4): the rows are written to the file now, children
            // of `to`, without a broadcast (the announcement is not shown again); the last one is
            // the leaf written. A rewind to a fixed text's row (twice from the same point) writes
            // no second one.
            let mut rows = requeued;
            if !to_is_rewound {
                let text = if files {
                    format!("{REWOUND_TEXT} {FILES_RESTORED}")
                } else {
                    REWOUND_TEXT.to_string()
                };
                rows.push(custom_row(REWOUND_TYPE, text, None));
            } else if files {
                rows.push(custom_row(RESTORED_TYPE, RESTORED_TEXT.to_string(), None));
            }
            for row in rows.iter().filter_map(parked) {
                crate::worker::persist_custom_row(&mut guard, &row);
            }
            let Some(store) = guard.store.as_ref() else {
                return failure("Session is still initializing");
            };
            (store.branch_file_entries(), store.session_id().to_string())
        };
        if let Err(error) = self.ya_rebuild(branch).await {
            return failure(&error);
        }
        if let Some(journal) = pa_core::session_engine::usage_log::lookup(&session_id) {
            journal.note_invalidation(
                json!({ "cause": pa_core::session_engine::usage_log::CAUSE_REWIND }),
            );
        }
        response_success(None, COMMAND, Some(json!({})))
    }

    /// The action `restored` (ticket 43, PR 3a, answer Q-3a-3): the bridge restored the workspace
    /// files only; the row of Q-P5 is written to the file, then the context is rebuilt from the
    /// branch, so that it reaches the model and survives a resume. The prefix does not change: no
    /// invalidation note.
    pub(crate) async fn ya_restored(&self) -> DaemonResponse {
        let _queued_work = self.hold_queued_work_pause("navigate");
        let branch = {
            let mut guard = self.core.lock().unwrap();
            if guard.busy || guard.compacting {
                return failure(WORKING);
            }
            if guard.store.is_none() {
                return failure("Session is still initializing");
            }
            if let Some(row) = parked(&custom_row(RESTORED_TYPE, RESTORED_TEXT.to_string(), None)) {
                crate::worker::persist_custom_row(&mut guard, &row);
            }
            let Some(store) = guard.store.as_ref() else {
                return failure("Session is still initializing");
            };
            store.branch_file_entries()
        };
        if let Err(error) = self.ya_rebuild(branch).await {
            return failure(&error);
        }
        response_success(None, COMMAND, Some(json!({})))
    }

    /// The engine's context rebuilt from `branch` off the runtime threads, then the goal of the
    /// branch announced before the answer (`handle_navigate_tree`).
    async fn ya_rebuild(&self, branch: Vec<pa_types::session::FileEntry>) -> Result<(), String> {
        let engine = Arc::clone(&self.engine);
        let rebuilt = tokio::task::spawn_blocking(move || {
            engine.rebuild_session_context(
                branch,
                pa_core::session_engine::goal_driver::GoalBranchReload::FaithfulBranch,
            )
        })
        .await;
        match rebuilt {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(format!("{error:#}")),
            Err(error) => return Err(format!("context rebuild failed: {error}")),
        }
        if let Some(goal) = self.engine.goal_update_after_rebuild() {
            self.emit_worker_event(json!({ "type": "goal_update", "goal": goal }));
        }
        Ok(())
    }
}
