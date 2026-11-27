//! yusAgent (YA-18, ticket 43): `_yus/session/rewind { sessionId, to }`, the conversation back to
//! where it stood before a prompt (its turn's `turnStart.rewindTo`), to send that prompt again,
//! changed (`docs/tickets/43-retour-arriere.md`, §I). Unharnessed's method
//! (`crates/uh-daemon/src/acp/rewind.rs`) under the yusAgent prefix (ticket 32).
//!
//! Push 1 (skeleton): the method answers as an unserved one; the tests of
//! `crates/pa-cli/tests/ya_acp_rewind_e2e.rs` are red on their assertions.
//!
//! Listed in `docs/fork-changes.md`.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::super::daemon::{DaemonAcpState, DaemonLink};
use super::super::jsonrpc;
use super::super::producer::FrameSink;

/// The method's name.
pub(crate) const METHOD: &str = "_yus/session/rewind";

/// Serves one `_yus/session/rewind` request.
// Push 1: nothing awaited yet (the worker's action comes with push 2).
#[allow(clippy::unused_async)]
pub(crate) async fn serve(
    id: &Value,
    _params: &Value,
    _link: &Arc<DaemonLink>,
    _state: &Arc<Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    let _ = tx.send(jsonrpc::error_response(
        id,
        jsonrpc::METHOD_NOT_FOUND,
        &format!("\"Method not found\": {METHOD}"),
        Some(&json!({ "method": METHOD })),
    ));
}

/// Answer 9 (ticket 43, §H): the refusal when another client that answers this session's
/// permission requests is attached (the C12 check, `ya_history::other_askers`); `None` when the
/// rewind may go on.
/// Push 1 (skeleton): never refuses.
#[allow(clippy::unused_async)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) async fn other_window_refusal(
    _link: &DaemonLink,
    _daemon_session_id: &str,
) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::daemon::{DaemonCommand, DaemonResponse};

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
