//! yusAgent (YA-10, ticket 28): resume and history on the ACP bridge.
//!
//! Unharnessed (`157dcdc1d`) serves `session/load`, `session/list` and
//! `_unharnessed/sessions/delete` (`crates/uh-daemon/src/acp/mod.rs:638-697`, `history.rs`); the
//! VS Code extension taken from it calls them as they are (`extensions/vscode/src/session.ts`,
//! `history.ts`). Here the bridge serves the same methods over the daemon
//! (`docs/tickets/28-reprise-historique.md`, decisions of 22/11/2026):
//! - the `session/new` and `session/load` responses carry the session's file, `saved
//!   { sessionId, path }` in the bridge `_meta` (C2);
//! - `session/load` loads only the session this process opened (`--resume`), under the file's
//!   id (C1), and replays its transcript, turn by turn, through the live mapping
//!   (`wire_events::wire_updates`, C6): `turnStart`, the prompt's blocks, the agent's work, then
//!   `turnEnd`. A turn the worker still runs stays open and goes on live (C9). A second ACP client
//!   of the same session is refused (C12);
//! - `session/list` lists the saved sessions of the session's folder, without subagents nor
//!   drafts, newest first (C10, C11);
//! - `_unharnessed/sessions/delete` stops the session in the daemon, then deletes its file and its
//!   artifacts folder; the usage journal stays (C3, C4).
//!
//! Nothing here reaches the model's requests: ACP frames only.

use std::path::Path;
use std::sync::Arc;

use pa_types::daemon::DaemonCommand;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::daemon::DaemonLink;
use super::meta::{self, PrimeAgentEventPhase, PrimeAgentSessionMeta};
use super::producer::UpdateProducer;
use super::types::AcpSessionUpdate;
use super::wire_events::{self, WireMappingState};
use super::ya_turns::{self, TurnStartMeta};

/// The delete method, Unharnessed's name, which the extension calls (decision C3).
pub(super) const DELETE_METHOD: &str = "_unharnessed/sessions/delete";

/// Transcript rows read per `get_session_messages` page.
const PAGE: u64 = 200;

/// How long the delete waits for a stopped session to leave the daemon, and how often it looks.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(100);
/// How long a client that just closed may still be attached to its session.
const RELEASE_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// A title is the start of the first prompt, this many characters at most (Unharnessed
/// `history.rs:48`).
const TITLE_CHARS: usize = 80;

/// `saved` in the bridge `_meta` (Unharnessed's `_meta.unharnessed.saved`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMeta {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// `turnEnd` of a replayed turn (Unharnessed's names): `done`, `cancelled` or `failed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnEndMeta {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The session's file, read from the startup `create` summary (`sessionId`, `sessionFile`).
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionFile {
    pub(crate) id: Option<String>,
    pub(crate) path: Option<String>,
}

impl SessionFile {
    pub(crate) fn from_summary(summary: Option<&Value>) -> Self {
        let field = |name: &str| {
            summary
                .and_then(|summary| summary.get(name))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        SessionFile {
            id: field("sessionId"),
            path: field("sessionFile"),
        }
    }

    /// The response's `_meta` payload; `None` without an id.
    pub(crate) fn saved(&self) -> Option<SavedMeta> {
        Some(SavedMeta {
            session_id: self.id.clone()?,
            path: self.path.clone(),
        })
    }
}

/// Adds `saved` to a `session/new` or `session/load` result.
pub(crate) fn add_saved(result: &mut Value, file: &SessionFile) {
    let Some(saved) = file.saved() else {
        return;
    };
    let saved = serde_json::to_value(saved).expect("saved serializes");
    let meta = result
        .as_object_mut()
        .expect("the result is an object")
        .entry("_meta")
        .or_insert_with(|| meta::prime_agent_meta(&PrimeAgentSessionMeta::default()));
    if let Some(namespace) = meta
        .as_object_mut()
        .and_then(|meta| meta.get_mut(meta::PRIME_AGENT_META_NAMESPACE))
        .and_then(Value::as_object_mut)
    {
        namespace.insert("saved".to_string(), saved);
    }
}

// session/load

/// Why `session/load` is refused before the admission, or `None`.
pub(crate) async fn load_refusal(
    link: &DaemonLink,
    daemon_session_id: &str,
    file: &SessionFile,
    requested: &str,
) -> Option<String> {
    let held = file.id.as_deref().unwrap_or_default();
    if requested != held {
        return Some(format!(
            "This process holds session {held}: start yusagent with --resume <its file> to load {requested}."
        ));
    }
    // C12: another ACP client (another window) answers this session's permission requests. A
    // client that just closed (the extension's « Relancer ») may still be attached for a moment:
    // the check looks again until `RELEASE_WAIT` has passed.
    let deadline = tokio::time::Instant::now() + RELEASE_WAIT;
    loop {
        match other_askers(link, daemon_session_id).await {
            Ok(0) => return None,
            Ok(_) => {}
            // Unknown is refused: a load that cannot be checked could open the session twice.
            Err(refusal) => return Some(refusal),
        }
        if tokio::time::Instant::now() >= deadline {
            return Some("This conversation is open in another window.".to_string());
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

/// The refusal when the daemon cannot count the other windows: its worker does not serve the
/// action `others` (C12), so the daemon was started by an older binary and kept running after the
/// binary changed; the bridge does not detect a daemon of another build (ticket 29).
pub(crate) const OLDER_DAEMON: &str = "The yusAgent daemon is older than this yusagent: it cannot tell whether this conversation is open in another window. Run `yusagent shutdown`, then Restart.";

/// The session's attached clients that answer, other than this bridge; `Err` is the refusal when
/// the daemon cannot tell (defect 1 of the X1 check: it was read as `0`, and the load went on).
async fn other_askers(link: &DaemonLink, daemon_session_id: &str) -> Result<u64, String> {
    let response = link
        .request(DaemonCommand::YaPermissions {
            id: None,
            active_session_id: daemon_session_id.to_string(),
            action: "others".to_string(),
            rest: crate::ya_permissions::asker_fields(),
        })
        .await
        .map_err(|error| {
            format!("Could not check whether this conversation is open in another window: {error}")
        })?;
    if !response.success {
        return Err(OLDER_DAEMON.to_string());
    }
    Ok(response
        .data
        .as_ref()
        .and_then(|data| data.get("others"))
        .and_then(Value::as_u64)
        .unwrap_or(0))
}

/// The `sessionId` a `session/load` asks for.
pub(crate) fn requested_id(params: &Value) -> String {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Replay the session's transcript into `producer` (held until the load's answer); a turn the
/// worker still runs (`state`, its connection state) stays open and goes on live (C9).
pub(crate) async fn replay(
    link: &DaemonLink,
    daemon_session_id: &str,
    state: Option<&Value>,
    producer: &UpdateProducer,
    agent_turns: &mut ya_turns::AgentTurns,
) -> anyhow::Result<()> {
    let messages = transcript(link, daemon_session_id).await?;
    let turns = replay_turns(&messages, is_streaming(state));
    if let Some(turn_id) = publish(turns, producer).await {
        agent_turns.adopt(turn_id);
    }
    Ok(())
}

/// The session's transcript, oldest first (`get_session_messages`, every role).
async fn transcript(link: &DaemonLink, daemon_session_id: &str) -> anyhow::Result<Vec<Value>> {
    let mut messages = Vec::new();
    loop {
        let response = link
            .request(DaemonCommand::GetSessionMessages {
                id: None,
                active_session_id: daemon_session_id.to_string(),
                start: i64::try_from(messages.len()).unwrap_or(i64::MAX),
                limit: PAGE,
                roles: vec!["*".to_string()],
                session_file: None,
                rest: Map::default(),
            })
            .await?;
        if !response.success {
            anyhow::bail!(response
                .error
                .unwrap_or_else(|| "the transcript could not be read".to_string()));
        }
        let data = response.data.unwrap_or(Value::Null);
        let rows = data
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let total = data
            .get("total")
            .and_then(Value::as_u64)
            .and_then(|total| usize::try_from(total).ok())
            .unwrap_or(0);
        let read = rows.len();
        messages.extend(
            rows.into_iter()
                .filter_map(|row| row.get("message").cloned()),
        );
        if read == 0 || messages.len() >= total {
            return Ok(messages);
        }
    }
}

/// One replayed turn: what started it, its updates, how it ended (`None`: still running).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayTurn {
    pub(crate) start: TurnStartMeta,
    pub(crate) updates: Vec<AcpSessionUpdate>,
    pub(crate) end: Option<TurnEndMeta>,
}

/// The turns of a transcript (Unharnessed `history::replay`). A user message starts a prompt turn
/// (a quota resume, a wake); a custom message starts a wake turn when no run is going on and the
/// next message is not a prompt (the notices that ride a prompt belong to it); assistant messages
/// and tool results go through the live mapping. `running`: the worker still runs the last turn,
/// which stays open.
pub(crate) fn replay_turns(messages: &[Value], running: bool) -> Vec<ReplayTurn> {
    Replay::default().run(messages, running)
}

#[derive(Default)]
struct Replay {
    turns: Vec<ReplayTurn>,
    current: Option<ReplayTurn>,
    /// A custom message after a finished run: the start of a wake, unless a prompt follows.
    pending_wake: Option<Value>,
    /// Compaction rows seen before any turn: they ride the next one.
    pending_updates: Vec<AcpSessionUpdate>,
    mapping: WireMappingState,
    /// The last assistant message of the current turn asked for tools.
    run_open: bool,
    last_stop: Option<String>,
    last_error: Option<String>,
    user_messages: u64,
}

impl Replay {
    fn run(mut self, messages: &[Value], running: bool) -> Vec<ReplayTurn> {
        for message in messages {
            match message.get("role").and_then(Value::as_str) {
                Some("user") => {
                    self.pending_wake = None;
                    // A message sent into a running turn (steer) ends it as done (Unharnessed
                    // `history.rs:229-234`).
                    if self.run_open {
                        self.run_open = false;
                        self.last_stop = None;
                    }
                    let start = ya_turns::origin_of(message);
                    let prompt = start.origin == "prompt";
                    self.begin(start);
                    if prompt {
                        self.prompt_blocks(message);
                    }
                    self.run_open = true;
                }
                Some("custom") => {
                    if !self.run_open && self.pending_wake.is_none() {
                        self.pending_wake = Some(message.clone());
                    }
                }
                Some("assistant") => {
                    self.wake_if_pending();
                    if self.current.is_none() {
                        self.begin(TurnStartMeta::new("wake", None));
                    }
                    self.assistant(message);
                }
                Some("toolResult") => {
                    self.wake_if_pending();
                    self.tool_result(message);
                }
                Some("compactionSummary") => {
                    let update = AcpSessionUpdate::SessionInfoUpdate {
                        meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                            compaction: Some(meta::PrimeAgentCompactionMeta {
                                tokens_before: message.get("tokensBefore").and_then(Value::as_u64),
                                summary: message
                                    .get("summary")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                            }),
                            ..Default::default()
                        }),
                    };
                    if let Some(turn) = self.current.as_mut() {
                        turn.updates.push(update);
                    } else {
                        self.pending_updates.push(update);
                    }
                }
                // `bashExecution` (a user `!command`), `branchSummary`: not replayed
                // (Unharnessed `history.rs:300`).
                _ => {}
            }
        }
        if running {
            // The worker runs a turn: a wake whose notice is the last row, or the last turn.
            self.wake_if_pending();
            if let Some(turn) = self.current.take() {
                self.turns.push(turn);
            }
        } else {
            self.finish();
        }
        self.turns
    }

    fn begin(&mut self, start: TurnStartMeta) {
        self.finish();
        self.current = Some(ReplayTurn {
            start,
            updates: std::mem::take(&mut self.pending_updates),
            end: None,
        });
        self.run_open = false;
        self.last_stop = None;
        self.last_error = None;
    }

    fn wake_if_pending(&mut self) {
        if let Some(notice) = self.pending_wake.take() {
            self.begin(ya_turns::origin_of(&notice));
            self.run_open = true;
        }
    }

    /// The current turn ends: `done`, `cancelled` (stopped, or cut while it ran), or `failed`.
    fn finish(&mut self) {
        let Some(mut turn) = self.current.take() else {
            return;
        };
        let stop = self.last_stop.as_deref();
        let (state, error) = if stop == Some("error") {
            ("failed", self.last_error.take())
        } else if stop == Some("aborted") || self.run_open {
            ("cancelled", None)
        } else {
            ("done", None)
        };
        turn.end = Some(TurnEndMeta {
            state: state.to_string(),
            error,
        });
        self.turns.push(turn);
    }

    fn push(&mut self, updates: Vec<AcpSessionUpdate>) {
        if let Some(turn) = self.current.as_mut() {
            turn.updates.extend(updates);
        }
    }

    fn prompt_blocks(&mut self, message: &Value) {
        self.user_messages += 1;
        let message_id = format!("prime-agent-user-{}", self.user_messages);
        let blocks: Vec<Value> = match message.get("content") {
            Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("text") => Some(json!({ "type": "text", "text": block.get("text")? })),
                    Some("image") => Some(json!({
                        "type": "image",
                        "data": block.get("data")?.as_str()?,
                        "mimeType": block.get("mimeType")?.as_str()?,
                    })),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let updates = blocks
            .into_iter()
            .map(|content| AcpSessionUpdate::UserMessageChunk {
                message_id: message_id.clone(),
                content,
            })
            .collect();
        self.push(updates);
    }

    /// Text and reasoning as the live deltas would stream them, then the tool calls.
    fn assistant(&mut self, message: &Value) {
        let role = json!({ "role": "assistant" });
        let mut updates = wire_events::wire_updates(
            &json!({ "type": "message_start", "message": role }),
            &mut self.mapping,
        );
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut calls = Vec::new();
        for block in &blocks {
            let delta = match block.get("type").and_then(Value::as_str) {
                Some("text") => block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| ("text_delta", text)),
                Some("thinking") => block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .map(|text| ("thinking_delta", text)),
                Some("toolCall") => {
                    calls.push(block.clone());
                    None
                }
                _ => None,
            };
            if let Some((kind, text)) = delta {
                updates.extend(wire_events::wire_updates(
                    &json!({ "type": "message_update", "message": role,
                             "assistantMessageEvent": { "type": kind, "delta": text } }),
                    &mut self.mapping,
                ));
            }
        }
        updates.extend(wire_events::wire_updates(
            &json!({ "type": "message_end", "message": role }),
            &mut self.mapping,
        ));
        for call in calls {
            updates.extend(wire_events::wire_updates(
                &json!({
                    "type": "tool_execution_start",
                    "toolCallId": call.get("id").cloned().unwrap_or(Value::Null),
                    "toolName": call.get("name").cloned().unwrap_or(Value::Null),
                    "args": call.get("arguments").cloned().unwrap_or(Value::Null),
                }),
                &mut self.mapping,
            ));
        }
        self.push(updates);
        let stop = message
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.run_open = stop.as_deref() == Some("toolUse");
        self.last_error = message
            .get("errorMessage")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.last_stop = stop;
    }

    fn tool_result(&mut self, message: &Value) {
        let event = json!({
            "type": "tool_execution_end",
            "toolCallId": message.get("toolCallId").cloned().unwrap_or(Value::Null),
            "isError": message.get("isError").cloned().unwrap_or(Value::Bool(false)),
            "result": {
                "content": message.get("content").cloned().unwrap_or(Value::Null),
                "details": message.get("details").cloned().unwrap_or(Value::Null),
            },
        });
        let updates = wire_events::wire_updates(&event, &mut self.mapping);
        self.push(updates);
    }
}

/// Publish the replayed turns, each under its own turn id; the open one (`end: None`) keeps its id
/// active and is returned.
pub(crate) async fn publish(turns: Vec<ReplayTurn>, producer: &UpdateProducer) -> Option<u64> {
    let mut open = None;
    for turn in turns {
        let turn_id = producer.begin_prompt().await;
        ya_turns::publish_turn_start(producer, turn_id, turn.start).await;
        for update in &turn.updates {
            let _ = producer
                .publish(update, turn_id, PrimeAgentEventPhase::Event, None)
                .await;
        }
        if let Some(end) = turn.end {
            let _ = producer
                .publish(
                    &AcpSessionUpdate::SessionInfoUpdate {
                        meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                            turn_end: Some(end),
                            ..Default::default()
                        }),
                    },
                    turn_id,
                    PrimeAgentEventPhase::Event,
                    None,
                )
                .await;
            producer.finish_prompt(turn_id).await;
        } else {
            open = Some(turn_id);
        }
    }
    open
}

/// Whether the worker runs a turn, from its connection state.
fn is_streaming(state: Option<&Value>) -> bool {
    state
        .and_then(|state| state.get("isStreaming"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

// session/list and the delete

/// What the bridge is bound to, for `session/list` and the delete.
pub(crate) struct Bound<'a> {
    pub(crate) link: &'a Arc<DaemonLink>,
    pub(crate) daemon_session_id: &'a str,
    pub(crate) file: &'a SessionFile,
    pub(crate) actual_cwd: &'a std::path::Path,
}

/// Whether [`serve`] answers `method`.
pub(crate) fn serves(method: &str) -> bool {
    matches!(method, "session/list" | DELETE_METHOD)
}

/// Answer `session/list` or the delete on `tx`.
pub(crate) async fn serve(
    bound: &Bound<'_>,
    method: &str,
    id: &Value,
    params: &Value,
    tx: &super::producer::FrameSink,
) {
    let response = if method == DELETE_METHOD {
        match delete(bound.link, bound.daemon_session_id, bound.file, params).await {
            Ok(()) => super::jsonrpc::response(id, &json!({})),
            Err(DeleteRefusal::Invalid(details)) => super::jsonrpc::error_response(
                id,
                super::jsonrpc::INVALID_PARAMS,
                "Invalid params",
                Some(&json!({ "details": details })),
            ),
            Err(DeleteRefusal::Failed(details)) => super::internal_error(id, &details),
        }
    } else {
        let requested_cwd = params.get("cwd").and_then(Value::as_str);
        match list(
            bound.link,
            bound.daemon_session_id,
            bound.file,
            requested_cwd,
            bound.actual_cwd,
        )
        .await
        {
            Ok(result) => super::jsonrpc::response(id, &result),
            Err(error) => super::internal_error(id, &error.to_string()),
        }
    };
    let _ = tx.send(response);
}

/// The saved sessions of the bound session's folder (`list_saved_sessions`, current cwd): rows
/// with `id`, `path`, `name`, `cwd`, `modified`, `messageCount`, `firstMessage`, `rlmDepth`,
/// `parentSessionPath` (`crates/pa-daemon/src/supervisor/sessions.rs`, `saved_session_row`).
async fn saved_rows(link: &DaemonLink, daemon_session_id: &str) -> anyhow::Result<Vec<Value>> {
    let response = link
        .request(DaemonCommand::ListSavedSessions {
            id: None,
            active_session_id: Some(daemon_session_id.to_string()),
            cwd: None,
            session_dir: None,
            scope: json!("current"),
            limit: None,
            cursor: None,
            rest: Map::default(),
        })
        .await?;
    if !response.success {
        anyhow::bail!(response
            .error
            .unwrap_or_else(|| "the saved sessions could not be listed".to_string()));
    }
    Ok(response
        .data
        .as_ref()
        .and_then(|data| data.get("sessions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn text_field<'a>(row: &'a Value, name: &str) -> Option<&'a str> {
    row.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// A conversation of the history: a top-level session with at least one message.
fn is_conversation(row: &Value) -> bool {
    row.get("parentSessionPath").is_none()
        && row.get("rlmDepth").and_then(Value::as_u64).unwrap_or(0) == 0
        && row.get("messageCount").and_then(Value::as_u64).unwrap_or(0) > 0
}

/// The session's name, else the start of its first prompt, spaces folded, cut at 80 characters.
fn title(row: &Value) -> Option<String> {
    let text = text_field(row, "name").or_else(|| text_field(row, "firstMessage"))?;
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.is_empty() {
        return None;
    }
    if folded.chars().count() <= TITLE_CHARS {
        return Some(folded);
    }
    let cut: String = folded.chars().take(TITLE_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// The `session/list` rows (Unharnessed `history.rs:68-84`): `sessionId`, `cwd`, `title`,
/// `updatedAt`, and `path` and `current` in the bridge `_meta`; newest first.
pub(crate) fn list_rows(rows: &[Value], current: Option<&str>) -> Vec<Value> {
    let mut listed: Vec<&Value> = rows.iter().filter(|row| is_conversation(row)).collect();
    listed.sort_by_key(|row| std::cmp::Reverse(text_field(row, "modified").map(str::to_string)));
    listed
        .into_iter()
        .filter_map(|row| {
            let session_id = text_field(row, "id")?;
            let mut ours = Map::new();
            if let Some(path) = text_field(row, "path") {
                ours.insert("path".to_string(), json!(path));
            }
            if current == Some(session_id) {
                ours.insert("current".to_string(), json!(true));
            }
            let mut out = json!({
                "sessionId": session_id,
                "cwd": text_field(row, "cwd").unwrap_or_default(),
                "_meta": { meta::PRIME_AGENT_META_NAMESPACE: ours },
            });
            if let Some(title) = title(row) {
                out["title"] = json!(title);
            }
            if let Some(updated) = text_field(row, "modified") {
                out["updatedAt"] = json!(updated);
            }
            Some(out)
        })
        .collect()
}

/// `session/list`: `{ sessions }`. A `cwd` other than the session's lists nothing: the bridge
/// serves its own folder.
async fn list(
    link: &DaemonLink,
    daemon_session_id: &str,
    file: &SessionFile,
    requested_cwd: Option<&str>,
    actual_cwd: &std::path::Path,
) -> anyhow::Result<Value> {
    if requested_cwd
        .filter(|cwd| !cwd.is_empty())
        .is_some_and(|cwd| !super::same_cwd(std::path::Path::new(cwd), actual_cwd))
    {
        return Ok(json!({ "sessions": [] }));
    }
    let rows = saved_rows(link, daemon_session_id).await?;
    // yusAgent (the X1 check): after the catalog scan, which may write a cache of a deleted file.
    if let Some(dir) = sessions_dir(file) {
        sweep_orphan_caches(dir);
    }
    Ok(json!({ "sessions": list_rows(&rows, file.id.as_deref()) }))
}

// _unharnessed/sessions/delete

/// How a delete ended: `Ok` (`{}`), or the refusal shown to the user.
enum DeleteRefusal {
    /// Bad parameters, the current session, a session open elsewhere: `-32602`.
    Invalid(String),
    /// A daemon failure: `-32603`.
    Failed(String),
}

/// Stop the session in the daemon if it runs, then delete its file and its artifacts (C4).
async fn delete(
    link: &Arc<DaemonLink>,
    daemon_session_id: &str,
    file: &SessionFile,
    params: &Value,
) -> Result<(), DeleteRefusal> {
    let Some(target) = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    else {
        return Err(DeleteRefusal::Invalid("sessionId is required".to_string()));
    };
    if file.id.as_deref() == Some(target) {
        return Err(DeleteRefusal::Invalid(
            "This is the conversation in progress: start a new one to delete it.".to_string(),
        ));
    }
    let rows = saved_rows(link, daemon_session_id)
        .await
        .map_err(|error| DeleteRefusal::Failed(error.to_string()))?;
    let Some(path) = rows
        .iter()
        .find(|row| text_field(row, "id") == Some(target))
        .and_then(|row| text_field(row, "path"))
        .map(str::to_string)
    else {
        return Err(DeleteRefusal::Invalid(format!(
            "No saved conversation {target} in this folder."
        )));
    };
    if let Some(live) = released_session(link, target, &path).await? {
        if live.attached_clients > 0 {
            return Err(DeleteRefusal::Invalid(
                "This conversation is open in another window.".to_string(),
            ));
        }
        let killed = link
            .request(DaemonCommand::Kill {
                id: None,
                active_session_id: live.active_session_id,
                rest: Map::default(),
            })
            .await
            .map_err(|error| DeleteRefusal::Failed(error.to_string()))?;
        if !killed.success {
            return Err(DeleteRefusal::Failed(
                killed
                    .error
                    .unwrap_or_else(|| "the session could not be stopped".to_string()),
            ));
        }
        // The worker leaves the daemon's live sessions once stopped; the delete waits for it.
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        while live_session(link, target, &path).await?.is_some() {
            if tokio::time::Instant::now() >= deadline {
                return Err(DeleteRefusal::Failed(
                    "the session did not stop in time".to_string(),
                ));
            }
            tokio::time::sleep(STOP_POLL).await;
        }
        // Its process writes the scan-state sidecar as it releases the file: then it is gone.
        if !worker_exited(live.worker_pid, deadline).await {
            return Err(DeleteRefusal::Failed(
                "the session did not stop in time".to_string(),
            ));
        }
    }
    let deleted = link
        .request(DaemonCommand::DeleteSavedSession {
            id: None,
            active_session_id: None,
            session_path: path.clone(),
            rest: Map::default(),
        })
        .await
        .map_err(|error| DeleteRefusal::Failed(error.to_string()))?;
    let ok = deleted.success
        && deleted
            .data
            .as_ref()
            .and_then(|data| data.get("ok"))
            .and_then(Value::as_bool)
            == Some(true);
    if ok {
        remove_sidecars(Path::new(&path));
        if let Some(dir) = sessions_dir(file) {
            sweep_orphan_caches(dir);
        }
        return Ok(());
    }
    let reason = deleted
        .data
        .as_ref()
        .and_then(|data| data.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(deleted.error)
        .unwrap_or_else(|| "the conversation could not be deleted".to_string());
    Err(DeleteRefusal::Failed(reason))
}

/// The live session of a saved file, once the clients that just closed have left it (the
/// extension stops a conversation's process before it asks for the delete): attached clients
/// are looked at again until `RELEASE_WAIT` has passed.
async fn released_session(
    link: &DaemonLink,
    session_id: &str,
    path: &str,
) -> Result<Option<LiveSession>, DeleteRefusal> {
    let deadline = tokio::time::Instant::now() + RELEASE_WAIT;
    loop {
        let live = live_session(link, session_id, path).await?;
        let attached = live.as_ref().is_some_and(|live| live.attached_clients > 0);
        if !attached || tokio::time::Instant::now() >= deadline {
            return Ok(live);
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

// The bridge's end

/// yusAgent (YA-10, ticket 28, defect 2 of the X1 check): at the end of stdin, a conversation this
/// bridge opened new where nothing was asked goes with it: its session stops in the daemon, then
/// its file and artifacts are deleted, as the history's delete does (C4). VS Code closed or
/// reloaded with a new conversation in the panel left one behind: invisible in the history (no
/// message), a worker and a kernel until the daemon's idle eviction (90 min), then a file for
/// good. It stays when its transcript has a message, when another client is attached (another
/// window), and when the daemon cannot tell. A bridge killed by a signal leaves it: the
/// extension's « Restart » resumes that file. Best-effort: a failure leaves the session as it is.
pub(crate) async fn drop_if_empty(link: &DaemonLink, daemon_session_id: &str, file: &SessionFile) {
    let (Some(id), Some(path)) = (file.id.as_deref(), file.path.as_deref()) else {
        return;
    };
    if !transcript(link, daemon_session_id)
        .await
        .is_ok_and(|messages| messages.is_empty())
    {
        return;
    }
    if other_askers(link, daemon_session_id).await != Ok(0) {
        return;
    }
    // This bridge is still attached (until its link drops): one client, itself.
    let Ok(Some(live)) = live_session(link, id, path).await else {
        return;
    };
    if live.attached_clients > 1 {
        return;
    }
    let stopped = link
        .request(DaemonCommand::Kill {
            id: None,
            active_session_id: live.active_session_id,
            rest: Map::default(),
        })
        .await
        .is_ok_and(|response| response.success);
    if !stopped {
        return;
    }
    let deadline = tokio::time::Instant::now() + STOP_WAIT;
    while matches!(live_session(link, id, path).await, Ok(Some(_))) {
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(STOP_POLL).await;
    }
    if !worker_exited(live.worker_pid, deadline).await {
        return;
    }
    let deleted = link
        .request(DaemonCommand::DeleteSavedSession {
            id: None,
            active_session_id: None,
            session_path: path.to_string(),
            rest: Map::default(),
        })
        .await
        .is_ok_and(|response| {
            response.success
                && response
                    .data
                    .as_ref()
                    .and_then(|data| data.get("ok"))
                    .and_then(Value::as_bool)
                    == Some(true)
        });
    if deleted {
        remove_sidecars(Path::new(path));
    }
}

/// yusAgent (YA-10, ticket 28, the X1 check): after a delete, nothing named after the session is
/// left but its usage journal (`<stem>.usage.ndjson`, kept: C4). `delete_saved_session` removes the
/// file and `session-artifacts/<id>` (`crate::saved_session_commands::delete_session_file`), not
/// the rest. Removed here, whoever wrote them (`docs/tickets/28-reprise-historique.md`, §18):
/// - the file `<stem>.jsonl`, should a writer have put it back, and its write temporaries
///   `<stem>.jsonl.tmp-<pid>` (`crate::session_store` rewrite, `crate::journal`) and
///   `<stem>.jsonl.tmp<pid>` (`pa-core` `session/manager/persist.rs`, `repair.rs`);
/// - the window snapshot `<stem>.window-cache.json` (`pa-core` `session/window_cache.rs`) and the
///   scan state `<stem>.info-cache.json` (`crate::session_store::persist_info_sidecar`), and their
///   temporaries `<stem>.window-cache-<uuid>.tmp`, `<stem>.info-cache-<uuid>.tmp`;
/// - `session-artifacts/<stem>` (`crate::saved_session_commands::remove_session_artifacts`).
///
/// The callers run it once no worker of the session runs: none was live, or its process exited.
/// Disposable or already deleted: removed, not trashed.
fn remove_sidecars(session_file: &Path) {
    crate::saved_session_commands::remove_session_artifacts(session_file);
    let (Some(dir), Some(stem)) = (
        session_file.parent(),
        session_file.file_stem().and_then(|stem| stem.to_str()),
    ) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(stem))
            .and_then(|rest| rest.strip_prefix('.'))
        else {
            continue;
        };
        if is_session_leftover(rest) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Whether `<stem>.<rest>` is one of the files `remove_sidecars` removes (not `usage.ndjson`).
fn is_session_leftover(rest: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let temporary = |prefix: &str| {
        rest.strip_prefix(prefix).is_some_and(|tail| {
            std::path::Path::new(tail)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("tmp"))
        })
    };
    matches!(rest, "jsonl" | "window-cache.json" | "info-cache.json")
        || rest
            .strip_prefix("jsonl.tmp")
            .is_some_and(|tail| digits(tail.strip_prefix('-').unwrap_or(tail)))
        || temporary("window-cache-")
        || temporary("info-cache-")
}

/// yusAgent (YA-10, ticket 28, the X1 check): remove the orphan caches of `sessions_dir`, the
/// exact names `<uuid>.window-cache.json` and `<uuid>.info-cache.json` whose `<uuid>.jsonl` is not
/// there (not even a dangling link). A cache lives only next to its file, which is never absent
/// while a session lives (atomic renames; §18); an orphan is what a delete left, or what a catalog
/// scan that read the file before the delete wrote after it
/// (`crate::session_scan` → `persist_info_sidecar`). Never a temporary, a usage journal, or a name
/// that is not a session id.
fn sweep_orphan_caches(sessions_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(stem) = name
            .strip_suffix(".window-cache.json")
            .or_else(|| name.strip_suffix(".info-cache.json"))
        else {
            continue;
        };
        let session_id = stem.len() == 36 && uuid::Uuid::parse_str(stem).is_ok();
        if session_id
            && std::fs::symlink_metadata(sessions_dir.join(format!("{stem}.jsonl"))).is_err()
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The folder of the bridge's session file, where `session/list` and the delete look.
fn sessions_dir(file: &SessionFile) -> Option<&Path> {
    file.path
        .as_deref()
        .and_then(|path| Path::new(path).parent())
}

/// Whether the worker process `pid` has exited by `deadline` (`true` when the daemon gave none).
async fn worker_exited(pid: Option<u32>, deadline: tokio::time::Instant) -> bool {
    let Some(pid) = pid else {
        return true;
    };
    loop {
        if !pa_types::platform::process::is_process_alive(pid).unwrap_or(false) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(STOP_POLL).await;
    }
}

struct LiveSession {
    active_session_id: String,
    attached_clients: u64,
    /// The worker's process (`workerPid`), when the daemon gives it.
    worker_pid: Option<u32>,
}

/// The daemon's live session for a saved file, if a worker serves it.
async fn live_session(
    link: &DaemonLink,
    session_id: &str,
    path: &str,
) -> Result<Option<LiveSession>, DeleteRefusal> {
    let response = link
        .request(DaemonCommand::List {
            id: None,
            all: None,
            cwd: None,
            session_dir: None,
            include_client_owned: None,
            rest: Map::default(),
        })
        .await
        .map_err(|error| DeleteRefusal::Failed(error.to_string()))?;
    let rows = response
        .data
        .as_ref()
        .and_then(|data| data.get("sessions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(rows
        .iter()
        .find(|row| {
            text_field(row, "sessionId") == Some(session_id)
                || text_field(row, "sessionFile") == Some(path)
        })
        .and_then(|row| {
            Some(LiveSession {
                active_session_id: text_field(row, "activeSessionId")?.to_string(),
                attached_clients: row
                    .get("attachedClients")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                worker_pid: row
                    .get("workerPid")
                    .and_then(Value::as_u64)
                    .and_then(|pid| u32::try_from(pid).ok()),
            })
        }))
}

#[cfg(test)]
#[path = "ya_history_tests.rs"]
mod tests;
