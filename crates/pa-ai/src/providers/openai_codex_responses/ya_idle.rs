//! yusAgent (YA-09, ticket 12): a parked Codex WebSocket keeps reading its socket between requests.
//!
//! Upstream's worker waited for the next command without polling the stream, so the server's
//! keepalive pings went unanswered during a silence (a long `bash()` wait) and the server closed
//! the socket with `1011 keepalive ping timeout` after roughly 75 s. The next request then failed
//! before its first event and pinned the session to SSE, with full request bodies until the end
//! of the session (`docs/tickets/12-ya09-ws-keepalive.md`).
//!
//! While parked, the worker now reads the socket: tungstenite answers each ping with a pong, a
//! close or a read error ends the worker, and the session's next request opens a fresh connection
//! instead of falling back to SSE (`evict_closed_connection`). A data frame that arrives with no
//! request in flight is ignored but counted; the count lands as a diagnostic on the next request
//! sent on that connection (`take_idle_messages_diagnostic`).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use crate::providers::openai_codex_responses::session::CachedConnection;
use crate::types::AssistantMessageDiagnostic;
use crate::utils_inner::diagnostics::create_assistant_message_diagnostic;

/// Diagnostic type for data frames received with no request in flight.
pub(crate) const IDLE_MESSAGES_DIAGNOSTIC: &str = "websocket_idle_messages_ignored";

/// What the parked worker does with one read from its socket.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IdleRead {
    /// A ping: tungstenite queued the pong; flush it now.
    Ping,
    /// Keep waiting (a pong, a raw frame, or an ignored data frame).
    Continue,
    /// The socket is closed or broken: the worker ends.
    Closed,
}

#[derive(Debug, Default, Clone)]
struct IdleCounts {
    text: u64,
    binary: u64,
    /// The `type` field of the last JSON text message, if any.
    last_type: Option<String>,
}

fn idle_counts() -> &'static Mutex<HashMap<u64, IdleCounts>> {
    static COUNTS: OnceLock<Mutex<HashMap<u64, IdleCounts>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Classify one read of a parked connection, counting data frames.
pub(crate) fn on_idle_read(connection_id: u64, read: Option<Result<Message, WsError>>) -> IdleRead {
    match read {
        Some(Ok(Message::Ping(_))) => IdleRead::Ping,
        Some(Ok(Message::Pong(_) | Message::Frame(_))) => IdleRead::Continue,
        Some(Ok(Message::Text(text))) => {
            let last_type = serde_json::from_str::<Value>(text.as_str())
                .ok()
                .and_then(|event| {
                    event
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            if let Ok(mut counts) = idle_counts().lock() {
                let entry = counts.entry(connection_id).or_default();
                entry.text += 1;
                if last_type.is_some() {
                    entry.last_type = last_type;
                }
            }
            IdleRead::Continue
        }
        Some(Ok(Message::Binary(_))) => {
            if let Ok(mut counts) = idle_counts().lock() {
                counts.entry(connection_id).or_default().binary += 1;
            }
            IdleRead::Continue
        }
        Some(Ok(Message::Close(_)) | Err(_)) | None => IdleRead::Closed,
    }
}

/// Drop the counts of a connection whose worker ended: no request will be sent on it again.
pub(crate) fn forget_connection(connection_id: u64) {
    if let Ok(mut counts) = idle_counts().lock() {
        counts.remove(&connection_id);
    }
}

/// The diagnostic for the data frames a connection ignored while parked, if any (resets them).
pub(crate) fn take_idle_messages_diagnostic(
    connection_id: u64,
) -> Option<AssistantMessageDiagnostic> {
    let counts = idle_counts().lock().ok()?.remove(&connection_id)?;
    let mut details = json!({
        "textMessages": counts.text,
        "binaryMessages": counts.binary,
    });
    if let Some(last_type) = counts.last_type {
        details["lastType"] = Value::String(last_type);
    }
    Some(create_assistant_message_diagnostic(
        IDLE_MESSAGES_DIAGNOSTIC,
        None,
        Some(details),
    ))
}

/// Remove the session's cached connection when its worker has ended (socket closed while
/// parked), so the request opens a fresh connection instead of sending into a dead one.
pub(crate) fn evict_closed_connection(
    connections: &mut HashMap<String, CachedConnection>,
    session_id: &str,
) {
    if connections
        .get(session_id)
        .is_some_and(|entry| !entry.busy && entry.worker.is_closed())
    {
        connections.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    #[test]
    fn control_frames_keep_the_worker_and_close_ends_it() {
        let id = u64::MAX - 1;
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Ping(vec![1u8].into())))),
            IdleRead::Ping
        );
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Pong(vec![1u8].into())))),
            IdleRead::Continue
        );
        let close = CloseFrame {
            code: CloseCode::Error,
            reason: "keepalive ping timeout".into(),
        };
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Close(Some(close))))),
            IdleRead::Closed
        );
        assert_eq!(
            on_idle_read(id, Some(Err(WsError::ConnectionClosed))),
            IdleRead::Closed
        );
        assert_eq!(on_idle_read(id, None), IdleRead::Closed);
        assert!(
            take_idle_messages_diagnostic(id).is_none(),
            "control frames are not counted"
        );
    }

    #[test]
    fn data_frames_are_counted_once_into_a_diagnostic() {
        let id = u64::MAX - 2;
        let text = json!({ "type": "response.done" }).to_string();
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Text(text.into())))),
            IdleRead::Continue
        );
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Text("not json".into())))),
            IdleRead::Continue
        );
        assert_eq!(
            on_idle_read(id, Some(Ok(Message::Binary(vec![0u8, 1].into())))),
            IdleRead::Continue
        );
        let diagnostic = take_idle_messages_diagnostic(id).expect("diagnostic");
        assert_eq!(diagnostic.type_, IDLE_MESSAGES_DIAGNOSTIC);
        let details = diagnostic.details.expect("details");
        assert_eq!(details.get("textMessages"), Some(&json!(2)));
        assert_eq!(details.get("binaryMessages"), Some(&json!(1)));
        assert_eq!(details.get("lastType"), Some(&json!("response.done")));
        assert!(take_idle_messages_diagnostic(id).is_none(), "taken once");
    }
}
