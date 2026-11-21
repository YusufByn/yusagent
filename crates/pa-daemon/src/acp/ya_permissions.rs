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
//!   `selected` with the `allow-once` option allows; `cancelled` withdraws the request (the
//!   client answers it after its `session/cancel`); anything else (`reject-once`, an unknown
//!   option or outcome, an error) is a refusal. Both stop the turn. A response to a request already closed
//!   (answered elsewhere, or withdrawn by `session/cancel`) is dropped.
//! - In `"manual"`, a tool call is published `pending` until its decision, then `in_progress`
//!   once allowed (`crate::ya_permissions::RESOLVED_EVENT`).
//! - The worker's request can overtake the `tool_execution_start` of its call (the worker
//!   publishes the engine's events from its own queue): the bridge holds the request until the
//!   `tool_call` is published, so the client always knows the call it is asked about.
//! - yusAgent (YA-10, ticket 24): a request closed elsewhere while the client still holds it
//!   (another client answered, or the worker withdrew it; never one the bridge still held) is
//!   cancelled on the client with the
//!   ACP notification [`CANCEL_REQUEST`] (`requestId`), so its permission box closes. The client
//!   still answers it (`cancelled` or the error `-32800`, ACP cancellation): the answer is
//!   dropped as stale.
//! - yusAgent (YA-10, ticket 24): before each prompt the bridge declares again that its client
//!   answers ([`rearm_asker`]): a worker relaunched after a crash only replays the `create`.
//! - The selector `permissions` (`manual`, `auto`) applies the user's choice to the session
//!   (action `set_mode`), journaled by the worker; it is only ever changed by the client.
//!
//! The JSON-RPC parser of the bridge (`super::jsonrpc::parse_line`) still refuses responses from
//! the client: the bridge reads the responses to its own requests before it. Listed in
//! `docs/fork-changes.md`.

use std::collections::{HashMap, HashSet};
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
/// yusAgent (YA-10, ticket 24): the ACP notification that cancels a request the bridge sent.
pub const CANCEL_REQUEST: &str = "$/cancel_request";
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
    /// The tool calls whose `tool_call` is published and not ended yet.
    started: Mutex<HashSet<String>>,
    /// Requests waiting for the `tool_call` of their call, by tool call id.
    held: Mutex<HashMap<String, Value>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The worker's outcome of the client's response: only `selected` with [`ALLOW_ONCE`] allows;
/// `cancelled` withdraws; anything else declines.
#[must_use]
pub fn outcome_of(response: &Value) -> &'static str {
    let outcome = &response["result"]["outcome"];
    if outcome["outcome"] == "selected" && outcome["optionId"] == ALLOW_ONCE {
        "allow"
    } else if outcome["outcome"] == "cancelled" {
        "cancelled"
    } else {
        "deny"
    }
}

fn text(event: &Value, key: &str) -> String {
    event
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

impl AcpPermissions {
    pub(crate) fn new(frames: &UnboundedSender<Value>) -> Arc<Self> {
        Arc::new(Self {
            frames: frames.downgrade(),
            next_id: AtomicU64::new(0),
            sent: Mutex::default(),
            started: Mutex::default(),
            held: Mutex::default(),
        })
    }

    /// The updates one worker event adds (before they are published): an allowed call goes
    /// `in_progress`. A closed request is forgotten; one the client still holds is cancelled on
    /// the client ([`CANCEL_REQUEST`]).
    pub(crate) fn updates(&self, event: &Value) -> Vec<AcpSessionUpdate> {
        if event.get("type").and_then(Value::as_str) != Some(RESOLVED_EVENT) {
            return Vec::new();
        }
        let request_id = text(event, "requestId");
        let tool_call_id = text(event, "toolCallId");
        // A request still held (its `tool_call` not published yet) never reached the client:
        // nothing to cancel there.
        let held_id = lock(&self.held)
            .remove(&tool_call_id)
            .and_then(|frame| frame["id"].as_str().map(str::to_string));
        let still_open: Vec<String> = {
            let mut sent = lock(&self.sent);
            let ids = sent
                .iter()
                .filter(|(id, sent)| {
                    sent.request_id == request_id && held_id.as_deref() != Some(id.as_str())
                })
                .map(|(id, _)| id.clone())
                .collect();
            sent.retain(|_, sent| sent.request_id != request_id);
            ids
        };
        for id in still_open {
            self.send(json!({
                "jsonrpc": "2.0",
                "method": CANCEL_REQUEST,
                "params": { "requestId": id },
            }));
        }
        if text(event, "outcome") == "allow" {
            vec![AcpSessionUpdate::ToolCallUpdate {
                tool_call_id,
                status: Some(AcpToolStatus::InProgress),
                content: None,
                meta: None,
            }]
        } else {
            Vec::new()
        }
    }

    /// One worker event, once its updates are published: a request goes to the client after the
    /// `tool_call` of its call.
    pub(crate) fn after_publish(
        &self,
        event: &Value,
        acp_session_id: &str,
        daemon_session_id: &str,
    ) {
        let tool_call_id = text(event, "toolCallId");
        match event.get("type").and_then(Value::as_str) {
            Some("tool_execution_start") => {
                lock(&self.started).insert(tool_call_id.clone());
                let held = lock(&self.held).remove(&tool_call_id);
                if let Some(frame) = held {
                    self.send(frame);
                }
            }
            Some("tool_execution_end") => {
                lock(&self.started).remove(&tool_call_id);
            }
            Some(REQUEST_EVENT) => {
                let frame = self.request_frame(
                    acp_session_id,
                    daemon_session_id,
                    &text(event, "requestId"),
                    &tool_call_id,
                );
                let started = lock(&self.started).contains(&tool_call_id);
                if started {
                    self.send(frame);
                } else {
                    lock(&self.held).insert(tool_call_id, frame);
                }
            }
            _ => {}
        }
    }

    fn send(&self, frame: Value) {
        if let Some(frames) = self.frames.upgrade() {
            let _ = frames.send(frame);
        }
    }

    /// The request to the client, registered as sent.
    fn request_frame(
        &self,
        acp_session_id: &str,
        daemon_session_id: &str,
        request_id: &str,
        tool_call_id: &str,
    ) -> Value {
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
        json!({
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
        })
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

/// yusAgent (YA-10, ticket 24): declares again that this client answers the session's permission
/// requests (action `asker`). Sent before each prompt: a worker relaunched after a crash replays
/// the durable `create` but not the clients' `attach`, and would decline the next request at once
/// for want of a client to answer. Best-effort: a failure leaves the worker as it was.
pub(super) async fn rearm_asker(link: &DaemonLink, daemon_session_id: &str) {
    let _ = link
        .request(DaemonCommand::YaPermissions {
            id: None,
            active_session_id: daemon_session_id.to_string(),
            action: "asker".to_string(),
            rest: crate::ya_permissions::asker_fields(),
        })
        .await;
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

    /// The events the consumer hands over: the updates, then the post-publish step.
    fn feed(permissions: &AcpPermissions, event: &Value) -> Vec<AcpSessionUpdate> {
        let updates = permissions.updates(event);
        permissions.after_publish(event, "acp-1", "daemon-1");
        updates
    }

    fn start(tool_call_id: &str) -> Value {
        json!({ "type": "tool_execution_start", "toolCallId": tool_call_id })
    }

    #[test]
    fn only_the_allow_option_allows() {
        let selected = |option: &str| json!({ "result": { "outcome": { "outcome": "selected", "optionId": option } } });
        assert_eq!(outcome_of(&selected(ALLOW_ONCE)), "allow");
        assert_eq!(outcome_of(&selected(REJECT_ONCE)), "deny");
        assert_eq!(outcome_of(&selected("allow-always")), "deny");
        assert_eq!(
            outcome_of(&json!({ "result": { "outcome": { "outcome": "cancelled" } } })),
            "cancelled"
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
        assert!(feed(&permissions, &start("call-1")).is_empty());
        let updates = feed(
            &permissions,
            &json!({
                "type": REQUEST_EVENT, "requestId": "ya-permission-7", "toolCallId": "call-1",
                "toolName": "python_repl", "args": { "code": "print(1)" },
            }),
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

    /// The worker's request overtook the `tool_execution_start` of its call: held until then.
    #[test]
    fn a_request_waits_for_the_tool_call_of_its_call() {
        let (frames, mut client) = tokio::sync::mpsc::unbounded_channel();
        let permissions = AcpPermissions::new(&frames);
        let _ = feed(
            &permissions,
            &json!({ "type": REQUEST_EVENT, "requestId": "r-1", "toolCallId": "call-1" }),
        );
        assert!(client.try_recv().is_err(), "held");
        let _ = feed(&permissions, &start("call-1"));
        let request = client.try_recv().expect("released after the tool call");
        assert_eq!(request["params"]["toolCall"]["toolCallId"], "call-1");
    }

    #[test]
    fn a_closed_request_drops_the_late_response_and_an_allowed_call_runs() {
        let (frames, mut client) = tokio::sync::mpsc::unbounded_channel();
        let permissions = AcpPermissions::new(&frames);
        let _ = feed(&permissions, &start("call-1"));
        let _ = feed(
            &permissions,
            &json!({ "type": REQUEST_EVENT, "requestId": "r-1", "toolCallId": "call-1" }),
        );
        let id = client.try_recv().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Withdrawn by the worker (`session/cancel`): cancelled on the client (ticket 24), and
        // the client's `cancelled` is dropped.
        let updates = feed(
            &permissions,
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-1", "toolCallId": "call-1", "outcome": "cancelled" }),
        );
        assert!(updates.is_empty());
        let cancel = client
            .try_recv()
            .expect("the request cancelled on the client");
        assert_eq!(cancel["method"], CANCEL_REQUEST);
        assert_eq!(cancel["params"]["requestId"], json!(id));
        let late = response(&id, &json!({ "outcome": { "outcome": "cancelled" } }));
        assert!(matches!(permissions.client_line(&late), ClientLine::Stale));
        // A request withdrawn while held is never sent.
        let _ = feed(
            &permissions,
            &json!({ "type": REQUEST_EVENT, "requestId": "r-3", "toolCallId": "call-3" }),
        );
        let _ = feed(
            &permissions,
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-3", "toolCallId": "call-3", "outcome": "cancelled" }),
        );
        let _ = feed(&permissions, &start("call-3"));
        assert!(client.try_recv().is_err(), "nothing to ask");
        // An allowed call goes `in_progress`.
        let updates = feed(
            &permissions,
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-2", "toolCallId": "call-2", "outcome": "allow" }),
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

    /// yusAgent (YA-10, ticket 24): another client (the TUI) answered: the bridge cancels the
    /// request on its client; a request its own client answered is not cancelled.
    #[test]
    fn a_request_answered_elsewhere_is_cancelled_on_the_client() {
        let (frames, mut client) = tokio::sync::mpsc::unbounded_channel();
        let permissions = AcpPermissions::new(&frames);
        let _ = feed(&permissions, &start("call-1"));
        let _ = feed(
            &permissions,
            &json!({ "type": REQUEST_EVENT, "requestId": "r-1", "toolCallId": "call-1" }),
        );
        let id = client.try_recv().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let updates = feed(
            &permissions,
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-1", "toolCallId": "call-1", "outcome": "allow" }),
        );
        assert_eq!(updates.len(), 1, "the call goes in_progress");
        assert_eq!(
            client.try_recv().expect("a cancel notification"),
            json!({ "jsonrpc": "2.0", "method": CANCEL_REQUEST, "params": { "requestId": id } })
        );
        // The client's answer to the cancelled request is stale.
        let late = response(&id, &json!({ "outcome": { "outcome": "cancelled" } }));
        assert!(matches!(permissions.client_line(&late), ClientLine::Stale));

        // Answered by this client: nothing to cancel when the worker resolves it.
        let _ = feed(&permissions, &start("call-2"));
        let _ = feed(
            &permissions,
            &json!({ "type": REQUEST_EVENT, "requestId": "r-2", "toolCallId": "call-2" }),
        );
        let id = client.try_recv().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let answer = response(
            &id,
            &json!({ "outcome": { "outcome": "selected", "optionId": REJECT_ONCE } }),
        );
        assert!(matches!(
            permissions.client_line(&answer),
            ClientLine::Answer(_)
        ));
        let _ = feed(
            &permissions,
            &json!({ "type": RESOLVED_EVENT, "requestId": "r-2", "toolCallId": "call-2", "outcome": "deny" }),
        );
        assert!(
            client.try_recv().is_err(),
            "no cancel for an answered request"
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
