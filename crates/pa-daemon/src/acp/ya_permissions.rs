//! yusAgent (YA-11, ticket 20, PR 2): `session/request_permission` on the daemon-attached ACP
//! bridge, and the `permissions` selector of `session/set_config_option`.
//!
//! - The bridge creates its daemon session with `crate::ya_permissions::ASKER_FIELD`: it answers
//!   the requests of its session (`crate::print_runtime` of `pa-cli` builds the create).
//! - A worker request (`crate::ya_permissions::REQUEST_EVENT`) becomes a JSON-RPC request
//!   `session/request_permission` to the client (an agent-to-client request: the bridge keeps its
//!   id), with the tool call in `pending` and two options, `allow_once` and `reject_once`. No
//!   `allow_always`: the session-wide answer is the selector (decision D3).
//! - The client's response goes back to the worker (command `ya_permissions`, action `respond`):
//!   `selected` with the `allow-once` option allows; anything else (`reject-once`, `cancelled`,
//!   an unknown option or outcome, an error) is a refusal. A response to a request already closed
//!   (answered elsewhere, or withdrawn by `session/cancel`) is dropped.
//! - In `"manual"`, a tool call is published `pending` until its decision, then `in_progress`
//!   once allowed (`crate::ya_permissions::RESOLVED_EVENT`).
//! - The selector `permissions` (`manual`, `auto`) applies the user's choice to the session
//!   (action `set_mode`), journaled by the worker; it is only ever changed by the client.
//!
//! The JSON-RPC parser of the bridge (`super::jsonrpc::parse_line`) still refuses responses from
//! the client: the bridge reads the responses to its own requests before it. Listed in
//! `docs/fork-changes.md`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pa_types::daemon::DaemonCommand;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc::{UnboundedSender, WeakUnboundedSender};

use super::config_options::{SessionConfigOption, SessionConfigSelectOption};
use super::daemon::DaemonLink;
use super::types::{AcpSessionUpdate, AcpToolStatus};
use super::wire_config::{HostedConfig, WireConfigError};
use crate::ya_permissions::{COMMAND, REQUEST_EVENT, RESOLVED_EVENT, STATE_FIELD};

/// The agent-to-client request.
pub const METHOD: &str = "session/request_permission";
/// The option that allows the call.
pub const ALLOW_ONCE: &str = "allow-once";
/// The option that declines it.
pub const REJECT_ONCE: &str = "reject-once";
/// The id of the selector in `session/set_config_option`.
pub const CONFIG_ID: &str = "permissions";
/// The prefix of the ids of the bridge's requests to the client.
const ID_PREFIX: &str = "ya-permission-";

/// A request sent to the client, waiting for its response.
struct Sent {
    daemon_session_id: String,
    request_id: String,
}

/// The client's answer to one request, for the worker.
pub(crate) struct Answer {
    daemon_session_id: String,
    request_id: String,
    outcome: &'static str,
}

impl Answer {
    /// Sends the answer to the worker. A failure leaves the request waiting until the turn is
    /// cancelled or the client disconnects.
    pub(crate) async fn send(self, link: &DaemonLink) {
        let mut rest = Map::new();
        rest.insert("requestId".to_string(), json!(self.request_id));
        rest.insert("outcome".to_string(), json!(self.outcome));
        let _ = link
            .request(DaemonCommand::YaPermissions {
                id: None,
                active_session_id: self.daemon_session_id,
                action: "respond".to_string(),
                rest,
            })
            .await;
    }
}

/// What a line from the client is to the bridge's requests.
pub(crate) enum ClientLine {
    /// Not a response to one of them: the usual parser reads it.
    Other,
    /// A response to a request already closed: dropped.
    Stale,
    /// The answer to a waiting request.
    Answer(Answer),
}

/// The bridge's permission requests to its client.
pub(crate) struct AcpPermissions {
    /// The client's stdout queue. Weak: the writer ends when the bridge drops its senders.
    frames: WeakUnboundedSender<Value>,
    next_id: AtomicU64,
    sent: Mutex<HashMap<String, Sent>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The worker's outcome of the client's response: only `selected` with [`ALLOW_ONCE`] allows.
#[must_use]
pub fn outcome_of(response: &Value) -> &'static str {
    let outcome = &response["result"]["outcome"];
    if outcome["outcome"] == "selected" && outcome["optionId"] == ALLOW_ONCE {
        "allow"
    } else {
        "deny"
    }
}

impl AcpPermissions {
    pub(crate) fn new(frames: &UnboundedSender<Value>) -> Arc<Self> {
        Arc::new(Self {
            frames: frames.downgrade(),
            next_id: AtomicU64::new(0),
            sent: Mutex::default(),
        })
    }

    /// One worker event of the hosted session: a request goes to the client; an allowed call
    /// returns its `in_progress` update.
    pub(crate) fn on_event(
        &self,
        event: &Value,
        acp_session_id: &str,
        daemon_session_id: &str,
    ) -> Vec<AcpSessionUpdate> {
        let text = |key: &str| {
            event
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        match event.get("type").and_then(Value::as_str) {
            Some(REQUEST_EVENT) => {
                self.send_request(
                    acp_session_id,
                    daemon_session_id,
                    &text("requestId"),
                    &text("toolCallId"),
                );
                Vec::new()
            }
            Some(RESOLVED_EVENT) => {
                let request_id = text("requestId");
                lock(&self.sent).retain(|_, sent| sent.request_id != request_id);
                if text("outcome") == "allow" {
                    vec![AcpSessionUpdate::ToolCallUpdate {
                        tool_call_id: text("toolCallId"),
                        status: Some(AcpToolStatus::InProgress),
                        content: None,
                        meta: None,
                    }]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn send_request(
        &self,
        acp_session_id: &str,
        daemon_session_id: &str,
        request_id: &str,
        tool_call_id: &str,
    ) {
        let Some(frames) = self.frames.upgrade() else {
            return;
        };
        let id = format!(
            "{ID_PREFIX}{}",
            self.next_id.fetch_add(1, Ordering::SeqCst) + 1
        );
        lock(&self.sent).insert(
            id.clone(),
            Sent {
                daemon_session_id: daemon_session_id.to_string(),
                request_id: request_id.to_string(),
            },
        );
        let _ = frames.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": METHOD,
            "params": {
                "sessionId": acp_session_id,
                "toolCall": { "toolCallId": tool_call_id, "status": "pending" },
                "options": [
                    { "optionId": ALLOW_ONCE, "name": "Allow", "kind": "allow_once" },
                    { "optionId": REJECT_ONCE, "name": "Reject", "kind": "reject_once" },
                ],
            },
        }));
    }

    /// A line from the client, read before the JSON-RPC parser.
    pub(crate) fn client_line(&self, line: &str) -> ClientLine {
        let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(line.trim()) else {
            return ClientLine::Other;
        };
        let Some(id) = frame
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| id.starts_with(ID_PREFIX))
        else {
            return ClientLine::Other;
        };
        if frame.contains_key("method") {
            return ClientLine::Other;
        }
        let Some(sent) = lock(&self.sent).remove(id) else {
            return ClientLine::Stale;
        };
        ClientLine::Answer(Answer {
            daemon_session_id: sent.daemon_session_id,
            request_id: sent.request_id,
            outcome: outcome_of(&Value::Object(frame)),
        })
    }
}

/// Whether the session's published selector is `"manual"`.
async fn manual(config: &HostedConfig) -> bool {
    config
        .published
        .lock()
        .await
        .iter()
        .any(|option| option.id == CONFIG_ID && option.current_value == "manual")
}

/// In `"manual"`, a tool call starts `pending` (it waits for its decision).
pub(crate) async fn mark_pending(
    event: &Value,
    config: &HostedConfig,
    updates: &mut [AcpSessionUpdate],
) {
    if event.get("type").and_then(Value::as_str) != Some("tool_execution_start")
        || !manual(config).await
    {
        return;
    }
    for update in updates {
        if let AcpSessionUpdate::ToolCall { status, .. } = update {
            *status = AcpToolStatus::Pending;
        }
    }
}

/// The selector, when the worker's state carries the mode of a session that can ask.
pub(crate) fn push_option(options: &mut Vec<SessionConfigOption>, state: &Value) {
    let Some(mode) = state.get(STATE_FIELD).and_then(Value::as_str) else {
        return;
    };
    let choice = |value: &str, name: &str| SessionConfigSelectOption {
        value: value.to_string(),
        name: name.to_string(),
    };
    options.push(SessionConfigOption {
        id: CONFIG_ID.to_string(),
        name: "Permissions".to_string(),
        kind: "select",
        category: "mode".to_string(),
        current_value: mode.to_string(),
        options: vec![
            choice("manual", "Ask before each tool call"),
            choice("auto", "Run tool calls without asking (no sandbox)"),
        ],
    });
}

/// `session/set_config_option` on the selector: the user's choice, applied by the worker.
pub(super) async fn apply_mode(
    link: &DaemonLink,
    daemon_session_id: &str,
    value: &str,
) -> Result<(), WireConfigError> {
    if !matches!(value, "manual" | "auto") {
        return Err(WireConfigError::InvalidParams(format!(
            "Invalid permissions value: {value}"
        )));
    }
    let mut rest = Map::new();
    rest.insert("mode".to_string(), json!(value));
    let response = link
        .request(DaemonCommand::YaPermissions {
            id: None,
            active_session_id: daemon_session_id.to_string(),
            action: "set_mode".to_string(),
            rest,
        })
        .await
        .map_err(|error| WireConfigError::Internal(error.to_string()))?;
    if response.success {
        Ok(())
    } else {
        Err(WireConfigError::Internal(
            response
                .error
                .unwrap_or_else(|| format!("the {COMMAND} command failed")),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(id: &str, result: &Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
    }

    #[test]
    fn only_the_allow_option_allows() {
        let selected = |option: &str| json!({ "result": { "outcome": { "outcome": "selected", "optionId": option } } });
        assert_eq!(outcome_of(&selected(ALLOW_ONCE)), "allow");
        assert_eq!(outcome_of(&selected(REJECT_ONCE)), "deny");
        assert_eq!(outcome_of(&selected("allow-always")), "deny");
        assert_eq!(
            outcome_of(&json!({ "result": { "outcome": { "outcome": "cancelled" } } })),
            "deny"
        );
        assert_eq!(
            outcome_of(&json!({ "error": { "code": -32603, "message": "no" } })),
            "deny"
        );
        assert_eq!(outcome_of(&json!({ "result": {} })), "deny");
    }

    #[test]
    fn a_request_goes_to_the_client_and_its_response_back_to_the_worker() {
        let (frames, mut client) = tokio::sync::mpsc::unbounded_channel();
        let permissions = AcpPermissions::new(&frames);
        let updates = permissions.on_event(
            &json!({
                "type": REQUEST_EVENT, "requestId": "ya-permission-7", "toolCallId": "call-1",
                "toolName": "python_repl", "args": { "code": "print(1)" },
            }),
            "acp-1",
            "daemon-1",
        );
        assert!(updates.is_empty());
        let request = client.try_recv().expect("a request to the client");
        assert_eq!(request["method"], METHOD);
        assert_eq!(request["params"]["sessionId"], "acp-1");
        assert_eq!(
            request["params"]["toolCall"],
            json!({ "toolCallId": "call-1", "status": "pending" })
        );
        let kinds: Vec<&str> = request["params"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["allow_once", "reject_once"]);
        let id = request["id"].as_str().unwrap();

        // Not a response to the bridge: the usual parser reads it.
        assert!(matches!(
            permissions.client_line(r#"{"jsonrpc":"2.0","id":1,"method":"session/cancel"}"#),
            ClientLine::Other
        ));
        let answer = response(
            id,
            &json!({ "outcome": { "outcome": "selected", "optionId": ALLOW_ONCE } }),
        );
        let ClientLine::Answer(answer) = permissions.client_line(&answer) else {
            panic!("the answer of a waiting request");
        };
        assert_eq!(answer.daemon_session_id, "daemon-1");
        assert_eq!(answer.request_id, "ya-permission-7");
        assert_eq!(answer.outcome, "allow");
        // Answered once.
        let again = response(id, &json!({ "outcome": { "outcome": "cancelled" } }));
        assert!(matches!(permissions.client_line(&again), ClientLine::Stale));
    }

    #[test]
    fn a_closed_request_drops_the_late_response_and_an_allowed_call_runs() {
        let (frames, mut client) = tokio::sync::mpsc::unbounded_channel();
        let permissions = AcpPermissions::new(&frames);
        let _ = permissions.on_event(
            &json!({ "type": REQUEST_EVENT, "requestId": "r-1", "toolCallId": "call-1" }),
            "acp-1",
            "daemon-1",
        );
        let id = client.try_recv().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Withdrawn by the worker (`session/cancel`): the client's `cancelled` is dropped.
        let updates = permissions.on_event(
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-1", "toolCallId": "call-1", "outcome": "cancelled" }),
            "acp-1",
            "daemon-1",
        );
        assert!(updates.is_empty());
        let late = response(&id, &json!({ "outcome": { "outcome": "cancelled" } }));
        assert!(matches!(permissions.client_line(&late), ClientLine::Stale));
        // An allowed call goes `in_progress`.
        let updates = permissions.on_event(
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-2", "toolCallId": "call-2", "outcome": "allow" }),
            "acp-1",
            "daemon-1",
        );
        assert_eq!(
            updates,
            vec![AcpSessionUpdate::ToolCallUpdate {
                tool_call_id: "call-2".to_string(),
                status: Some(AcpToolStatus::InProgress),
                content: None,
                meta: None,
            }]
        );
    }

    #[test]
    fn the_selector_follows_the_worker_state() {
        let mut options = Vec::new();
        push_option(&mut options, &json!({ "thinkingLevel": "off" }));
        assert!(
            options.is_empty(),
            "no selector for a session that cannot ask"
        );
        let mut state = json!({ "thinkingLevel": "off" });
        state[STATE_FIELD] = json!("manual");
        push_option(&mut options, &state);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, CONFIG_ID);
        assert_eq!(options[0].current_value, "manual");
        let values: Vec<&str> = options[0]
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect();
        assert_eq!(values, vec!["manual", "auto"]);
    }
}
