//! yusAgent (YA-18, ticket 43, R-b0): the worker side of `_yus/session/rewind`, two actions of the
//! fork's command [`COMMAND`] (`docs/tickets/43-retour-arriere.md`, §I.3, §I.9):
//! - `leaf`: `{ leafId }`, the session's leaf; the ACP bridge reads it before a prompt's
//!   `turnStart` (`rewindTo`);
//! - `rewind { to }`: the conversation goes back to `to`, an entry of the current branch, where
//!   it stood before a prompt. As Unharnessed's `rewind_to` (`compaction_arms.rs:433-449`): the
//!   leaf moves in memory (written by the next append, answer 10), the engine's context is
//!   rebuilt from the branch, what came after stays in the file on a branch of its own, and the
//!   kernel keeps what it holds. Then (D2) the fixed text rides the next turn as a prefix row; an
//!   announcement row (`ya-resources-changed`) of the abandoned turns rides it again first (the
//!   tracker of what the model was told lives in memory, Q-I2); the next agent-loop call
//!   journals `invalidation: {"cause": "rewind"}`.
//!
//! Refused while a turn runs or a compaction is going on, and for an entry off the branch. The
//! queued input is suspended while the leaf moves (Q-I5): no wake or queued turn starts meanwhile;
//! the suspension is lifted and the runner woken after (`resume_queued_input`), unless it was on
//! before. Nothing changes the system prompt or the tools.

use std::sync::Arc;

use serde_json::{json, Value};

use super::COMMAND;
use crate::protocol::{response_failure, response_success, DaemonResponse};
use crate::worker::Worker;

/// Unharnessed's refusal while the agent works (`crates/uh-daemon/src/acp/rewind.rs:48-53`).
pub(crate) const WORKING: &str = "The agent is working: stop it before going back.";
/// The end of the refusal for an entry off the branch; the bridge answers it with `-32602`.
pub(crate) const NOT_IN_CONVERSATION: &str = "is not in the conversation";
/// The custom type of the fixed text's row.
pub(crate) const REWOUND_TYPE: &str = "ya-conversation-rewound";
/// The fixed text the model gets after a rewind (D2, validated by Yusuf, ticket 43, §H).
pub(crate) const REWOUND_TEXT: &str = "[conversation-rewound] The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.";

/// The custom row (`role: "custom"`) of the fixed text.
fn rewound_row() -> Value {
    json!({
        "role": "custom",
        "customType": REWOUND_TYPE,
        "content": REWOUND_TEXT,
        "display": true,
        "timestamp": crate::util::now_ms(),
    })
}

/// The custom row of an announcement entry of the abandoned turns, to ride the next turn again.
fn row_of(fields: &Value) -> Value {
    let mut row = json!({
        "role": "custom",
        "customType": pa_core::session_engine::ya_resource_notice::CUSTOM_TYPE,
        "content": fields.get("content").cloned().unwrap_or(Value::Null),
        "display": fields.get("display").and_then(Value::as_bool).unwrap_or(true),
        "timestamp": crate::util::now_ms(),
    });
    if let Some(details) = fields.get("details").filter(|details| !details.is_null()) {
        row["details"] = details.clone();
    }
    row
}

fn failure(error: &str) -> DaemonResponse {
    response_failure(None, COMMAND, error, None)
}

impl Worker {
    /// The action `leaf`: `{ leafId }`.
    pub(crate) fn ya_leaf(&self) -> DaemonResponse {
        let core = self.core.lock().unwrap();
        match core.store.as_ref() {
            Some(store) => {
                response_success(None, COMMAND, Some(json!({ "leafId": store.leaf_id() })))
            }
            None => failure("Session is still initializing"),
        }
    }

    /// The action `rewind { to }`.
    pub(crate) async fn ya_rewind(&self, payload: &Value) -> DaemonResponse {
        let to = payload
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (branch, suspended_before, session_id) = {
            let mut guard = self.core.lock().unwrap();
            let core = &mut *guard;
            if core.busy || core.compacting {
                return failure(WORKING);
            }
            let Some(store) = core.store.as_mut() else {
                return failure("Session is still initializing");
            };
            // The current branch, root to leaf; the abandoned entries come after `to`.
            let requeued: Vec<Value> = {
                let branch = store.branch();
                let Some(at) = branch.iter().position(|entry| entry.id == to) else {
                    return failure(&format!("{to} {NOT_IN_CONVERSATION}"));
                };
                branch[at + 1..]
                    .iter()
                    .filter(|entry| {
                        entry.type_ == "custom_message"
                            && entry.fields.get("customType").and_then(Value::as_str)
                                == Some(pa_core::session_engine::ya_resource_notice::CUSTOM_TYPE)
                    })
                    .map(|entry| row_of(&entry.fields))
                    .collect()
            };
            let suspended_before = core.queued_input_suspended;
            core.queued_input_suspended = true;
            if let Err(error) = store.branch_to(Some(to.as_str())) {
                core.queued_input_suspended = suspended_before;
                return failure(&error.to_string());
            }
            let branch = store.branch_file_entries();
            let session_id = store.session_id().to_string();
            // One fixed text: the one of an earlier rewind still waiting goes.
            core.pending_next_turn
                .retain(|row| row.get("customType").and_then(Value::as_str) != Some(REWOUND_TYPE));
            core.pending_next_turn.extend(requeued);
            core.pending_next_turn.push(rewound_row());
            (branch, suspended_before, session_id)
        };
        let engine = Arc::clone(&self.engine);
        let rebuilt = tokio::task::spawn_blocking(move || {
            engine.rebuild_session_context(
                branch,
                pa_core::session_engine::goal_driver::GoalBranchReload::FaithfulBranch,
            )
        })
        .await;
        // Suspended before the rewind (an abort): it stays so, nothing is woken.
        if !suspended_before {
            self.resume_queued_input();
        }
        match rebuilt {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return failure(&format!("{error:#}")),
            Err(error) => return failure(&format!("context rebuild failed: {error}")),
        }
        if let Some(journal) = pa_core::session_engine::usage_log::lookup(&session_id) {
            journal.note_invalidation(
                json!({ "cause": pa_core::session_engine::usage_log::CAUSE_REWIND }),
            );
        }
        response_success(None, COMMAND, Some(json!({})))
    }
}
