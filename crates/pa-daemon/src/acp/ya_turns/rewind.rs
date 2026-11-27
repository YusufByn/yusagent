//! yusAgent (YA-18, ticket 43): `_yus/session/rewind { sessionId, to }`, the conversation back to
//! where it stood before a prompt (its turn's `turnStart.rewindTo`), to send that prompt again,
//! changed (`docs/tickets/43-retour-arriere.md`, §I.3, §I.9). Unharnessed's method
//! (`crates/uh-daemon/src/acp/rewind.rs`) under the yusAgent prefix (ticket 32). It answers `{}`,
//! or is refused:
//! - an unknown session: `-32603`, "Unknown ACP session: {id}" (Unharnessed's text);
//! - a turn running (a prompt of this client, a turn the agent started): `-32603`, Unharnessed's
//!   text; the worker refuses too while a turn or a compaction runs;
//! - another client that answers this session's permission requests attached (answer 9, the C12
//!   check of `ya_history`): `-32603`, the text validated by Yusuf (Q-I3);
//! - `to` off the branch: `-32602`, `data.details` "{to} is not in the conversation".
//!
//! The worker moves the leaf (`crate::ya_permissions::rewind`); the fixed text and the
//! invalidation ride the next turn. Listed in `docs/fork-changes.md`.

use std::sync::Arc;

use pa_types::daemon::DaemonCommand;
use serde_json::{json, Map, Value};
use tokio::sync::Mutex;

use super::super::daemon::{DaemonAcpState, DaemonLink};
use super::super::jsonrpc;
use super::super::producer::FrameSink;
use crate::ya_permissions::rewind::{NOT_IN_CONVERSATION, WORKING};
use crate::ya_permissions::COMMAND;

/// The method's name.
pub(crate) const METHOD: &str = "_yus/session/rewind";

/// yusAgent (YA-18, ticket 43, PR 3a): the methods the bridge's arm routes here, the rewind and
/// the workspace snapshots' (`super::checkpoints`), so that the bridge keeps one arm.
pub(crate) fn serves(method: &str) -> bool {
    method == METHOD || super::checkpoints::serves(method)
}

/// The refusal when another window answers this session (answer 9, text of Q-I3).
const OTHER_WINDOW: &str =
    "This conversation is open in another window: close it there before going back.";

fn invalid(id: &Value, details: &str) -> Value {
    jsonrpc::error_response(
        id,
        jsonrpc::INVALID_PARAMS,
        "Invalid params",
        Some(&json!({ "details": details })),
    )
}

/// One action of the worker's fork command (`leaf`, `rewind`).
async fn worker_action(
    link: &DaemonLink,
    daemon_session_id: &str,
    action: &str,
    rest: Map<String, Value>,
) -> Result<Value, String> {
    let response = link
        .request(DaemonCommand::YaPermissions {
            id: None,
            active_session_id: daemon_session_id.to_string(),
            action: action.to_string(),
            rest,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.success {
        Ok(response.data.unwrap_or(Value::Null))
    } else {
        Err(response
            .error
            .unwrap_or_else(|| format!("{COMMAND} {action} failed")))
    }
}

/// The session's leaf (`rewindTo` of a prompt's `turnStart`); `None` when the worker cannot tell.
pub(super) async fn leaf(link: &DaemonLink, daemon_session_id: &str) -> Option<String> {
    let data = worker_action(link, daemon_session_id, "leaf", Map::new())
        .await
        .ok()?;
    data.get("leafId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Answer 9 (ticket 43, §H): the refusal when another client that answers this session's
/// permission requests is attached (the C12 check, `ya_history::other_askers`), or when the daemon
/// cannot tell (its C12 refusal); `None` when the rewind may go on.
pub(super) async fn other_window_refusal(
    link: &DaemonLink,
    daemon_session_id: &str,
) -> Option<String> {
    match super::super::ya_history::other_askers(link, daemon_session_id).await {
        Ok(0) => None,
        Ok(_) => Some(OTHER_WINDOW.to_string()),
        Err(refusal) => Some(refusal),
    }
}

/// Serves one request of [`serves`]: `_yus/session/rewind` here, the snapshots' in
/// `super::checkpoints`.
pub(crate) async fn serve(
    method: &str,
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    if super::checkpoints::serves(method) {
        super::checkpoints::serve(method, id, tx);
        return;
    }
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let to = params.get("to").and_then(Value::as_str).unwrap_or_default();
    let daemon_session_id = {
        let guard = state.lock().await;
        let Some(hosted) = guard
            .session
            .as_ref()
            .filter(|hosted| hosted.acp_session_id == session_id)
        else {
            let _ = tx.send(super::super::internal_error(
                id,
                &format!("Unknown ACP session: {session_id}"),
            ));
            return;
        };
        if hosted.producer.active_prompt_turn().await != 0 || hosted.ya_turns.running.is_some() {
            let _ = tx.send(super::super::internal_error(id, WORKING));
            return;
        }
        hosted.daemon_active_session_id.clone()
    };
    if let Some(refusal) = other_window_refusal(link, &daemon_session_id).await {
        let _ = tx.send(super::super::internal_error(id, &refusal));
        return;
    }
    let mut rest = Map::new();
    rest.insert("to".to_string(), json!(to));
    let frame = match worker_action(link, &daemon_session_id, "rewind", rest).await {
        Ok(_) => jsonrpc::response(id, &json!({})),
        Err(error) if error.ends_with(NOT_IN_CONVERSATION) => invalid(id, &error),
        Err(error) => super::super::internal_error(id, &error),
    };
    let _ = tx.send(frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::daemon::DaemonResponse;

    fn answer(data: Option<Value>) -> DaemonResponse {
        DaemonResponse {
            id: None,
            command: "ya_permissions".to_string(),
            success: true,
            data,
            error: None,
            error_info: None,
        }
    }

    /// T6 (answer 9): another window answers this session: the rewind is refused, with the text
    /// validated by Yusuf (Q-I3).
    #[tokio::test]
    async fn another_window_refuses_the_rewind() {
        let link = super::super::super::daemon::scripted_link(|command| match command {
            DaemonCommand::YaPermissions { action, .. } if action == "others" => {
                answer(Some(json!({ "others": 1 })))
            }
            other => panic!("unexpected command: {other:?}"),
        });
        assert_eq!(
            other_window_refusal(&link, "daemon-1").await.as_deref(),
            Some("This conversation is open in another window: close it there before going back.")
        );
    }

    /// No other window: the rewind goes on.
    #[tokio::test]
    async fn no_other_window_lets_the_rewind_go_on() {
        let link = super::super::super::daemon::scripted_link(|command| match command {
            DaemonCommand::YaPermissions { action, .. } if action == "others" => {
                answer(Some(json!({ "others": 0 })))
            }
            other => panic!("unexpected command: {other:?}"),
        });
        assert_eq!(other_window_refusal(&link, "daemon-1").await, None);
    }
}
