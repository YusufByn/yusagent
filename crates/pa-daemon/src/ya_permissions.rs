//! yusAgent (YA-11, ticket 20, PR 2): the permission requests of a daemon session.
//!
//! The engine's gate (`pa_core::session_engine::ya_permissions`) asks a [`PermissionAsker`]
//! before each tool call of a manual session. In the daemon, the asker is the worker:
//! - the creating client declares that it answers ([`ASKER_FIELD`] on `create`, carried by the
//!   durable create so a respawned worker keeps it; only the ACP bridge does, until the TUI asks
//!   in PR 3 of the ticket). Without it, a manual session is refused at creation, as in PR 1.
//!   The clients that answer say so again on `attach` (the worker's `create` carries no client
//!   id); a client that attaches without it only watches. yusAgent (YA-10, ticket 24): a worker
//!   relaunched after a crash gets the `create` again but no `attach`: the ACP bridge declares
//!   its client again before each prompt (action `asker`);
//! - a request is a session event [`REQUEST_EVENT`] (`requestId`, `toolCallId`, `toolName`,
//!   `args`) to every attached client; the first answer wins (command [`COMMAND`], action
//!   `respond`, `outcome` `"allow"`, `"cancelled"` for a withdrawn request, anything else for a
//!   refusal). The worker then sends
//!   [`RESOLVED_EVENT`] (`requestId`, `toolCallId`, `outcome`) so the other clients close it;
//! - a request still waiting when the turn is aborted (`session/cancel`), or when the last
//!   client that answers detaches (the client disconnected), is withdrawn: the call does not
//!   run and the turn stops (D7). A request with no client to answer is withdrawn at once;
//! - the decisions are journaled in the worker's durable session file (the engine runs on an
//!   in-memory session manager), as `custom` entries of type
//!   `pa_core::session_engine::ya_permissions::JOURNAL_TYPE`, never sent to the model;
//! - the mode can be switched while the session runs (action `set_mode`, the ACP selector
//!   `permissions`, the TUI's `/permissions`), back to `"manual"` too; each change is journaled
//!   and announced to the clients ([`MODE_EVENT`]). A resumed session follows the global setting
//!   again (decision D1).
//!
//! Nothing here reaches the model's requests: the prefix is unchanged. Listed in
//! `docs/fork-changes.md`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pa_agent::abort::AbortSignal;
use pa_agent::BoxFut;
use pa_core::session_engine::ya_permissions::{
    configured_mode, mode_change_entry, PermissionAsker, PermissionDecision, PermissionJournal,
    PermissionMode, PermissionRequest, PermissionSwitch, PermissionsWiring, JOURNAL_TYPE,
};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::agent_engine::AgentSessionEngine;
use crate::protocol::{response_failure, response_success, DaemonResponse};
use crate::worker::{EventPump, SessionCore, Worker};

/// The wire names, shared with the TUI (`pa_types::daemon::ya_permissions`).
pub use pa_types::daemon::ya_permissions::{
    ASKER_FIELD, COMMAND, MODE_EVENT, REQUEST_EVENT, RESOLVED_EVENT, STATE_FIELD,
};

// yusAgent (YA-18, ticket 43, Q-I1): this fork command also carries actions that are not about
// permissions, so that they cost no upstream line: `holds` (ticket 29), `rewind` and `leaf`
// (`_yus/session/rewind`, submodule `rewind`).
pub(crate) mod rewind;

/// Sends one session event to the attached clients.
type Emit = Arc<dyn Fn(Value) + Send + Sync>;

/// The permission state of one worker's session. Held by the engine, so that it outlives the
/// rebuilds of the core session.
#[derive(Default)]
pub(crate) struct WorkerPermissions {
    /// A client declared at `create` that it answers: the engine gets an asker.
    can_ask: AtomicBool,
    /// The attached clients that answer.
    askers: Mutex<HashSet<String>>,
    /// The requests waiting for an answer.
    pending: Mutex<HashMap<String, oneshot::Sender<PermissionDecision>>>,
    next_request: AtomicU64,
    switch: Arc<PermissionSwitch>,
    emit: Mutex<Option<Emit>>,
    journal: Mutex<Option<PermissionJournal>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn asks(payload: &Value) -> bool {
    payload.get(ASKER_FIELD).and_then(Value::as_bool) == Some(true)
}

/// The fields a client that answers adds to its `attach`.
#[must_use]
pub fn asker_fields() -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    fields.insert(ASKER_FIELD.to_string(), Value::Bool(true));
    fields
}

fn decision_label(decision: PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow => "allow",
        PermissionDecision::Deny => "deny",
        PermissionDecision::Cancelled => "cancelled",
    }
}

impl WorkerPermissions {
    /// A `create` payload: the session can ask when it carries [`ASKER_FIELD`].
    pub(crate) fn declare(&self, payload: &Value) {
        if asks(payload) {
            self.can_ask.store(true, Ordering::SeqCst);
        }
    }

    /// An `attach` payload: its client answers when it carries [`ASKER_FIELD`] and the session
    /// can ask (an engine built without an asker cannot be given one later).
    pub(crate) fn client_attached(&self, payload: &Value) {
        if !asks(payload) || !self.can_ask.load(Ordering::SeqCst) {
            return;
        }
        if let Some(client_id) = payload.get("clientId").and_then(Value::as_str) {
            lock(&self.askers).insert(client_id.to_string());
        }
    }

    /// yusAgent (YA-10, ticket 24): whether the client of `payload` (`clientId`) answers.
    pub(crate) fn has_asker(&self, payload: &Value) -> bool {
        payload
            .get("clientId")
            .and_then(Value::as_str)
            .is_some_and(|client_id| lock(&self.askers).contains(client_id))
    }

    /// yusAgent (YA-10, ticket 28, C12): how many attached clients that answer are not the client
    /// of `payload` (`clientId`): another window holding the session.
    pub(crate) fn other_askers(&self, payload: &Value) -> usize {
        let client_id = payload.get("clientId").and_then(Value::as_str);
        lock(&self.askers)
            .iter()
            .filter(|asker| Some(asker.as_str()) != client_id)
            .count()
    }

    /// A client detached (or disconnected): when no client that answers is left, the waiting
    /// requests are withdrawn.
    pub(crate) fn client_detached(&self, client_id: &str) {
        let mut askers = lock(&self.askers);
        if !askers.remove(client_id) || !askers.is_empty() {
            return;
        }
        let withdrawn: Vec<_> = lock(&self.pending).drain().collect();
        drop(askers);
        for (_, answer) in withdrawn {
            let _ = answer.send(PermissionDecision::Cancelled);
        }
    }

    fn install(&self, emit: Emit, journal: PermissionJournal) {
        *lock(&self.emit) = Some(emit);
        *lock(&self.journal) = Some(journal);
    }

    /// What the build passes to `create_session_with`: an asker, the worker's journal and the
    /// switch when a client answers; nothing otherwise (a manual session is refused, PR 1).
    pub(crate) fn wiring(self: &Arc<Self>) -> PermissionsWiring {
        if !self.can_ask.load(Ordering::SeqCst) {
            return PermissionsWiring::default();
        }
        PermissionsWiring {
            mode: None,
            asker: Some(Arc::new(WorkerAsker(Arc::clone(self)))),
            journal: lock(&self.journal).clone(),
            switch: Some(Arc::clone(&self.switch)),
        }
    }

    /// The mode of a session that can ask: the user's choice or the mode in force, else the
    /// global setting. `None` when no client answers.
    pub(crate) fn mode(&self, agent_dir: &Path) -> Option<PermissionMode> {
        if !self.can_ask.load(Ordering::SeqCst) {
            return None;
        }
        self.switch
            .current()
            .or_else(|| self.switch.chosen())
            .or_else(|| global_mode(agent_dir))
    }

    fn emit(&self, event: Value) {
        let emit = lock(&self.emit).clone();
        if let Some(emit) = emit {
            emit(event);
        }
    }

    fn journal(&self, data: Value) {
        let journal = lock(&self.journal).clone();
        if let Some(journal) = journal {
            journal(data);
        }
    }

    async fn ask(&self, request: &PermissionRequest, signal: &AbortSignal) -> PermissionDecision {
        let (answer, answered) = oneshot::channel();
        let request_id = {
            // Under the askers lock: a detach cannot slip between the check and the insertion.
            let askers = lock(&self.askers);
            if askers.is_empty() {
                return PermissionDecision::Cancelled;
            }
            let request_id = format!(
                "ya-permission-{}",
                self.next_request.fetch_add(1, Ordering::SeqCst) + 1
            );
            lock(&self.pending).insert(request_id.clone(), answer);
            request_id
        };
        let mut waiting = Waiting {
            permissions: self,
            request_id: &request_id,
            tool_call_id: &request.tool_call_id,
            decision: None,
        };
        self.emit(json!({
            "type": REQUEST_EVENT,
            "requestId": request_id,
            "toolCallId": request.tool_call_id,
            "toolName": request.tool_name,
            "args": request.args,
        }));
        let decision = tokio::select! {
            answer = answered => answer.unwrap_or(PermissionDecision::Cancelled),
            () = signal.aborted() => PermissionDecision::Cancelled,
        };
        waiting.decision = Some(decision);
        decision
    }

    /// The first answer to `request_id`; `false` when it is no longer waiting.
    fn respond(&self, request_id: &str, outcome: &str) -> bool {
        let Some(answer) = lock(&self.pending).remove(request_id) else {
            return false;
        };
        let decision = match outcome {
            "allow" => PermissionDecision::Allow,
            "cancelled" => PermissionDecision::Cancelled,
            _ => PermissionDecision::Deny,
        };
        answer.send(decision).is_ok()
    }

    /// The user's choice of mode; when it changes the mode, journaled and announced to the
    /// clients ([`MODE_EVENT`], PR 3: the TUI shows "auto" without a sandbox, decision D4).
    fn set_mode(&self, mode: PermissionMode, agent_dir: &Path) {
        let before = self.switch.choose(mode).or_else(|| global_mode(agent_dir));
        if let Some(before) = before.filter(|before| *before != mode) {
            self.journal(mode_change_entry(before, mode));
            self.emit(json!({ "type": MODE_EVENT, "mode": mode.label() }));
        }
    }
}

/// The mode of the global settings, as a new session would read it.
fn global_mode(agent_dir: &Path) -> Option<PermissionMode> {
    // The project settings are never read for this setting: the global document only.
    let settings = pa_core::settings::SettingsManager::create(agent_dir, agent_dir);
    configured_mode(settings.global_raw(), agent_dir).ok()
}

/// Withdraws its request when dropped unanswered (the turn was aborted while it waited), and
/// closes it for the clients either way.
struct Waiting<'a> {
    permissions: &'a WorkerPermissions,
    request_id: &'a str,
    tool_call_id: &'a str,
    decision: Option<PermissionDecision>,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        lock(&self.permissions.pending).remove(self.request_id);
        let outcome = decision_label(self.decision.unwrap_or(PermissionDecision::Cancelled));
        self.permissions.emit(json!({
            "type": RESOLVED_EVENT,
            "requestId": self.request_id,
            "toolCallId": self.tool_call_id,
            "outcome": outcome,
        }));
    }
}

/// The engine's asker: the worker's clients.
struct WorkerAsker(Arc<WorkerPermissions>);

impl PermissionAsker for WorkerAsker {
    fn ask(
        &self,
        request: PermissionRequest,
        signal: AbortSignal,
    ) -> BoxFut<'static, PermissionDecision> {
        let permissions = Arc::clone(&self.0);
        Box::pin(async move { permissions.ask(&request, &signal).await })
    }
}

/// Wires the session events and the durable journal of `engine` to the worker's core.
pub(crate) fn install(
    engine: &AgentSessionEngine,
    core: &Arc<Mutex<SessionCore>>,
    events: &Arc<EventPump>,
) {
    let emit: Emit = {
        let core = Arc::clone(core);
        let events = Arc::clone(events);
        Arc::new(move |event: Value| {
            crate::user_bash::emit_session_event_frame(&core, &events, event);
        })
    };
    let journal: PermissionJournal = {
        let core = Arc::clone(core);
        Arc::new(move |data: Value| {
            let mut core = lock(&core);
            if let Some(store) = core.store.as_mut() {
                if let Err(error) = store.persist_entry(
                    "custom",
                    json!({ "customType": JOURNAL_TYPE, "data": data }),
                ) {
                    eprintln!("pa-daemon: permission journal entry not persisted: {error:#}");
                }
            }
        })
    };
    engine.ya_permissions.install(emit, journal);
}

/// yusAgent (YA-10, ticket 29): whether the session holds work a daemon stop would lose (action
/// `holds`). It is the passivation's own gate (`SessionEngine::can_passivate_worker`): a live
/// background `bash()`, unsettled RLM work (a goal continuation waits behind it), an armed cron
/// or heartbeat, an RLM child record. The ACP bridge asks before replacing a daemon of another
/// build. The goal states kept only in memory are outside it, as for the passivation.
pub(crate) async fn holds_work(engine: &dyn crate::engine::SessionEngine) -> bool {
    !engine.can_passivate_worker().await
}

impl Worker {
    /// The command [`COMMAND`].
    pub(crate) async fn handle_ya_permissions(&self, payload: &Value) -> DaemonResponse {
        if let Err(response) = self.require_created(COMMAND) {
            return response;
        }
        // yusAgent (YA-10, ticket 29): what a daemon stop would lose, for any session.
        if payload.get("action").and_then(Value::as_str) == Some("holds") {
            let holds = holds_work(self.engine.as_ref()).await;
            return response_success(None, COMMAND, Some(json!({ "holds": holds })));
        }
        // yusAgent (YA-18, ticket 43): the conversation back to before a prompt, and the leaf
        // the bridge reads before a prompt (`rewind`).
        match payload.get("action").and_then(Value::as_str) {
            Some("rewind") => return self.ya_rewind(payload).await,
            Some("leaf") => return self.ya_leaf(),
            _ => {}
        }
        let Some(engine) = self.agent_engine.as_ref() else {
            return response_failure(
                None,
                COMMAND,
                "this session cannot ask for permission",
                None,
            );
        };
        let permissions = &engine.ya_permissions;
        let agent_dir = &self.config.agent_dir;
        match payload.get("action").and_then(Value::as_str) {
            Some("respond") => {
                let request_id = payload
                    .get("requestId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let outcome = payload
                    .get("outcome")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let resolved = permissions.respond(request_id, outcome);
                response_success(None, COMMAND, Some(json!({ "resolved": resolved })))
            }
            Some("set_mode") => {
                let Some(mode) = payload
                    .get("mode")
                    .and_then(Value::as_str)
                    .and_then(PermissionMode::from_label)
                else {
                    return response_failure(
                        None,
                        COMMAND,
                        "the mode must be \"manual\" or \"auto\"",
                        None,
                    );
                };
                if permissions.mode(agent_dir).is_none() {
                    return response_failure(
                        None,
                        COMMAND,
                        "this session cannot ask for permission",
                        None,
                    );
                }
                permissions.set_mode(mode, agent_dir);
                response_success(None, COMMAND, Some(json!({ "mode": mode.label() })))
            }
            Some("get") => {
                let mode = permissions.mode(agent_dir).map(PermissionMode::label);
                response_success(None, COMMAND, Some(json!({ "mode": mode })))
            }
            // yusAgent (YA-10, ticket 24): the routed client answers again (the ACP bridge before
            // each prompt): a relaunched worker replays the durable `create`, not the clients'
            // `attach`, so it no longer knew its answering client.
            Some("asker") => {
                permissions.client_attached(payload);
                response_success(
                    None,
                    COMMAND,
                    Some(json!({ "asker": permissions.has_asker(payload) })),
                )
            }
            // yusAgent (YA-10, ticket 28, C12): the other clients that answer (the ACP bridge
            // refuses `session/load` of a conversation open in another window).
            Some("others") => response_success(
                None,
                COMMAND,
                Some(json!({ "others": permissions.other_askers(payload) })),
            ),
            _ => response_failure(
                None,
                COMMAND,
                "the action must be \"respond\", \"set_mode\", \"get\", \"asker\", \"others\", \"holds\", \"rewind\" or \"leaf\"",
                None,
            ),
        }
    }

    /// Adds [`STATE_FIELD`] to `get_connection_state` for a session that can ask.
    pub(crate) fn ya_permissions_state(&self, state: &mut Value) {
        let mode = self
            .agent_engine
            .as_ref()
            .and_then(|engine| engine.ya_permissions.mode(&self.config.agent_dir));
        if let (Some(mode), Some(state)) = (mode, state.as_object_mut()) {
            state.insert(STATE_FIELD.to_string(), json!(mode.label()));
        }
    }

    /// A `create`: records whether the session can ask.
    pub(crate) fn ya_permissions_declare(&self, payload: &Value) {
        if let Some(engine) = self.agent_engine.as_ref() {
            engine.ya_permissions.declare(payload);
        }
    }

    /// An `attach`: records whether its client answers.
    pub(crate) fn ya_permissions_attached(&self, payload: &Value) {
        if let Some(engine) = self.agent_engine.as_ref() {
            engine.ya_permissions.client_attached(payload);
        }
    }

    /// A `detach`: withdraws the waiting requests when no client that answers is left.
    pub(crate) fn ya_permissions_detached(&self, client_id: &str) {
        if let Some(engine) = self.agent_engine.as_ref() {
            engine.ya_permissions.client_detached(client_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asking(client: &str) -> Arc<WorkerPermissions> {
        let permissions = Arc::new(WorkerPermissions::default());
        let mut create = json!({ "cwd": "/tmp" });
        create[ASKER_FIELD] = json!(true);
        permissions.declare(&create);
        let mut attach = json!({ "clientId": client });
        attach[ASKER_FIELD] = json!(true);
        permissions.client_attached(&attach);
        permissions
    }

    /// yusAgent (YA-10, ticket 24): the `asker` action registers its client, only in a session
    /// that can ask.
    #[test]
    fn a_client_can_answer_again() {
        let permissions = asking("bridge");
        permissions.client_detached("bridge");
        let mut again = json!({ "clientId": "bridge" });
        assert!(!permissions.has_asker(&again));
        again[ASKER_FIELD] = json!(true);
        permissions.client_attached(&again);
        assert!(permissions.has_asker(&again));

        let cannot = WorkerPermissions::default();
        cannot.client_attached(&again);
        assert!(
            !cannot.has_asker(&again),
            "a session created without an asker"
        );
    }

    /// One request through the engine's asker.
    fn ask(
        permissions: &Arc<WorkerPermissions>,
        tool_call_id: &str,
        signal: AbortSignal,
    ) -> tokio::task::JoinHandle<PermissionDecision> {
        let asker = WorkerAsker(Arc::clone(permissions));
        tokio::spawn(asker.ask(request(tool_call_id), signal))
    }

    /// What a test records: session events or journal entries.
    type Recorded = Arc<Mutex<Vec<Value>>>;

    /// Records the session events and the journal entries.
    fn recorded(permissions: &WorkerPermissions) -> (Recorded, Recorded) {
        let events: Recorded = Arc::default();
        let journal: Recorded = Arc::default();
        let (events_sink, journal_sink) = (Arc::clone(&events), Arc::clone(&journal));
        permissions.install(
            Arc::new(move |event: Value| lock(&events_sink).push(event)),
            Arc::new(move |data: Value| lock(&journal_sink).push(data)),
        );
        (events, journal)
    }

    fn request(id: &str) -> PermissionRequest {
        PermissionRequest {
            tool_call_id: id.to_string(),
            tool_name: "python_repl".to_string(),
            args: json!({ "code": "print(1)" }),
        }
    }

    /// The request id of the latest request event.
    fn requested(events: &Mutex<Vec<Value>>) -> String {
        lock(events)
            .iter()
            .rev()
            .find(|event| event["type"] == REQUEST_EVENT)
            .and_then(|event| event["requestId"].as_str())
            .expect("a request event")
            .to_string()
    }

    async fn settle() {
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn a_session_without_a_declared_client_gets_no_asker() {
        let permissions = Arc::new(WorkerPermissions::default());
        permissions.declare(&json!({ "cwd": "/tmp" }));
        // A client that says it answers cannot give an asker to such a session.
        let mut attach = json!({ "clientId": "acp:2" });
        attach[ASKER_FIELD] = json!(true);
        permissions.client_attached(&attach);
        assert!(lock(&permissions.askers).is_empty());
        let wiring = permissions.wiring();
        assert!(wiring.asker.is_none() && wiring.switch.is_none());
        assert_eq!(permissions.mode(Path::new("/nonexistent")), None);

        let wiring = asking("acp:1").wiring();
        assert!(wiring.asker.is_some() && wiring.switch.is_some());
    }

    #[tokio::test]
    async fn the_first_answer_wins_and_closes_the_request() {
        let permissions = asking("acp:1");
        let (events, _) = recorded(&permissions);
        let asked = ask(&permissions, "call-1", AbortSignal::never());
        settle().await;
        let id = requested(&events);
        let event = lock(&events)[0].clone();
        assert_eq!(event["toolCallId"], "call-1");
        assert_eq!(event["toolName"], "python_repl");
        assert_eq!(event["args"], json!({ "code": "print(1)" }));
        assert!(permissions.respond(&id, "allow"));
        assert!(!permissions.respond(&id, "deny"), "answered once");
        assert_eq!(asked.await.unwrap(), PermissionDecision::Allow);
        let resolved = lock(&events).last().cloned().unwrap();
        assert_eq!(resolved["type"], RESOLVED_EVENT);
        assert_eq!(resolved["requestId"], id.as_str());
        assert_eq!(resolved["outcome"], "allow");
    }

    #[tokio::test]
    async fn an_unknown_outcome_is_a_refusal() {
        let permissions = asking("acp:1");
        let (events, _) = recorded(&permissions);
        let asked = ask(&permissions, "call-1", AbortSignal::never());
        settle().await;
        assert!(permissions.respond(&requested(&events), "maybe"));
        assert_eq!(asked.await.unwrap(), PermissionDecision::Deny);
    }

    /// The client that answers disconnects while a request waits: withdrawn (a refusal).
    #[tokio::test]
    async fn the_last_answering_client_leaving_withdraws_the_request() {
        let permissions = asking("acp:1");
        let (events, _) = recorded(&permissions);
        let asked = ask(&permissions, "call-1", AbortSignal::never());
        settle().await;
        permissions.client_detached("tui-2");
        settle().await;
        assert!(
            !asked.is_finished(),
            "another client leaving changes nothing"
        );
        permissions.client_detached("acp:1");
        assert_eq!(asked.await.unwrap(), PermissionDecision::Cancelled);
        assert_eq!(lock(&events).last().unwrap()["outcome"], "cancelled");
        // No client left: the next request is withdrawn at once.
        assert_eq!(
            ask(&permissions, "call-2", AbortSignal::never())
                .await
                .unwrap(),
            PermissionDecision::Cancelled
        );
    }

    /// The turn is aborted while a request waits (`session/cancel`).
    #[tokio::test]
    async fn an_abort_withdraws_the_request() {
        let permissions = asking("acp:1");
        let (events, _) = recorded(&permissions);
        let controller = pa_agent::abort::AbortController::new();
        let asked = ask(&permissions, "call-1", controller.signal());
        settle().await;
        let id = requested(&events);
        controller.abort();
        assert_eq!(asked.await.unwrap(), PermissionDecision::Cancelled);
        assert!(!permissions.respond(&id, "allow"), "no longer waiting");
        assert_eq!(lock(&events).last().unwrap()["outcome"], "cancelled");
    }

    #[test]
    fn a_change_of_mode_is_journaled_and_manual_comes_back() {
        let permissions = asking("acp:1");
        let (events, journal) = recorded(&permissions);
        let agent_dir = Path::new("/nonexistent");
        // The session runs in "manual" (as a build would leave it).
        let _ = permissions.switch.choose(PermissionMode::Manual);
        permissions.set_mode(PermissionMode::Auto, agent_dir);
        assert_eq!(permissions.mode(agent_dir), Some(PermissionMode::Auto));
        permissions.set_mode(PermissionMode::Auto, agent_dir);
        permissions.set_mode(PermissionMode::Manual, agent_dir);
        assert_eq!(permissions.mode(agent_dir), Some(PermissionMode::Manual));
        let journal = lock(&journal).clone();
        assert_eq!(journal.len(), 2, "{journal:?}");
        assert_eq!(journal[0]["mode"], "auto");
        assert_eq!(journal[1]["mode"], "manual");
        assert_eq!(journal[1]["previousMode"], "auto");
        // Each change is announced to the clients (PR 3), the repeated choice is not.
        let modes: Vec<Value> = lock(&events)
            .iter()
            .filter(|event| event["type"] == MODE_EVENT)
            .map(|event| event["mode"].clone())
            .collect();
        assert_eq!(modes, [json!("auto"), json!("manual")]);
    }

    /// The daemon's resume reads the worker's file (`SessionFile::branch_file_entries`, as
    /// `agent_engine/lifecycle.rs` does) and rebuilds the model's context from it: the
    /// `ya-permission` entries the worker journals change nothing in it.
    #[test]
    fn journal_entries_change_nothing_in_a_resumed_context() {
        use crate::session_store::SessionFile;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, journal: bool| {
            let path = dir.path().join(name);
            let mut store = SessionFile::create("/tmp", None, 0);
            store.set_path(path.clone());
            for (index, text) in ["one", "two", "three"].into_iter().enumerate() {
                if journal && index > 0 {
                    let decision = json!({
                        "mode": "manual",
                        "toolCallId": format!("call-{index}"),
                        "toolName": "python_repl",
                        "decision": "allow",
                    });
                    store
                        .persist_entry(
                            "custom",
                            json!({ "customType": JOURNAL_TYPE, "data": decision }),
                        )
                        .unwrap();
                }
                let message = json!({ "role": "user", "content": text, "timestamp": 0 });
                store
                    .persist_entry("message", json!({ "message": message }))
                    .unwrap();
            }
            if journal {
                let change = mode_change_entry(PermissionMode::Manual, PermissionMode::Auto);
                store
                    .persist_entry(
                        "custom",
                        json!({ "customType": JOURNAL_TYPE, "data": change }),
                    )
                    .unwrap();
            }
            path
        };
        let context = |path: &Path| {
            let entries = SessionFile::open(path).unwrap().branch_file_entries();
            serde_json::to_value(pa_core::session::build_session_context(&entries, None).messages)
                .unwrap()
        };
        let with = write("with.jsonl", true);
        let without = write("without.jsonl", false);
        let journaled = std::fs::read_to_string(&with)
            .unwrap()
            .lines()
            .filter(|line| line.contains(JOURNAL_TYPE))
            .count();
        assert_eq!(journaled, 3, "the journal is in the file");
        let resumed = context(&with);
        assert_eq!(resumed.as_array().map(Vec::len), Some(3), "{resumed}");
        assert_eq!(resumed, context(&without));
    }

    /// yusAgent (YA-10, ticket 29): `holds` answers with the passivation's gate.
    mod holds {
        use std::sync::Arc;

        use crate::agent_engine::tests::{
            admit, faux_engine_with_settings, goal_admission_collector, FAUX_TEST_LOCK,
        };
        use crate::agent_engine::{AgentEngineConfig, AgentSessionEngine};
        use crate::engine::EngineEvent;

        use super::super::holds_work;

        fn holds(engine: &Arc<AgentSessionEngine>) -> bool {
            engine.runtime.block_on(holds_work(&**engine))
        }

        fn warm_engine() -> (Arc<AgentSessionEngine>, tempfile::TempDir) {
            let (engine, dir) = faux_engine_with_settings(
                &serde_json::json!({ "responses": [{"text": "warm ok"}, {"text": "turn reply"}] }),
                u64::MAX,
            );
            let engine = Arc::new(engine);
            let mut events: Vec<EngineEvent> = Vec::new();
            admit(&engine, "warm".to_string(), &mut events);
            (engine, dir)
        }

        fn set_background_bash(engine: &AgentSessionEngine, live: bool) {
            *engine
                .background_bash_probe
                .lock()
                .expect("background bash probe lock") = Some(Arc::new(move || live));
        }

        #[test]
        fn a_session_at_rest_holds_nothing() {
            let _faux = FAUX_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (engine, _dir) = warm_engine();
            assert!(!holds(&engine));
        }

        #[test]
        fn a_background_bash_holds_the_session() {
            let _faux = FAUX_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (engine, _dir) = warm_engine();
            set_background_bash(&engine, true);
            assert!(
                holds(&engine),
                "a live background bash() is lost by a daemon stop"
            );
            set_background_bash(&engine, false);
            assert!(!holds(&engine), "a finished bash() holds nothing");
        }

        #[test]
        fn an_armed_cron_or_heartbeat_holds_the_session() {
            let _faux = FAUX_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (engine, _dir) = warm_engine();
            engine.set_registered_jobs_probe(Arc::new(|| true));
            assert!(
                holds(&engine),
                "an armed job would not fire until the resume"
            );
        }

        #[test]
        fn a_goal_waiting_behind_a_background_bash_holds_the_session() {
            let _faux = FAUX_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (engine, _dir) = faux_engine_with_settings(
                &serde_json::json!({ "responses": [{"text": "warm ok"}, {"text": "turn reply"}] }),
                u64::MAX,
            );
            let engine = Arc::new(engine);
            let goal_work = goal_admission_collector(&engine);
            let mut events: Vec<EngineEvent> = Vec::new();
            admit(&engine, "warm".to_string(), &mut events);
            set_background_bash(&engine, true);
            admit(
                &engine,
                "/goal ship behind the bash handle".to_string(),
                &mut events,
            );
            assert!(
                goal_work.lock().unwrap().is_empty(),
                "the goal continuation waits behind the bash handle"
            );
            assert!(
                holds(&engine),
                "the owed goal continuation is lost by a daemon stop"
            );
        }

        #[test]
        fn a_running_rlm_child_holds_the_session() {
            let dir = tempfile::TempDir::new().unwrap();
            let engine = Arc::new(
                AgentSessionEngine::new(AgentEngineConfig {
                    cwd: dir.path().to_path_buf(),
                    agent_dir: dir.path().join("agent"),
                    provider: None,
                    model: None,
                    api_key: None,
                    thinking: None,
                    session_dir: None,
                    session_file: None,
                    faux_script: None,
                    supervisor_link: Some(crate::agent_engine::SupervisorLinkConfig {
                        socket_path: dir.path().join("dead.sock"),
                        active_session_id: "parent-session".to_string(),
                        worker_token: "token".to_string(),
                    }),
                    telemetry_disabled: None,
                    cron_store: None,
                    queued_steering_probe: None,
                })
                .unwrap(),
            );
            let children = engine.children.clone().expect("children registry");
            engine.runtime.block_on(async {
                children
                    .push_test_child(crate::rlm_children::RlmChildIdentity {
                        rlm_child_id: "child-1".to_string(),
                        active_session_id: "child-session".to_string(),
                        session_id: None,
                        session_name: "worker-1".to_string(),
                    })
                    .await;
            });
            assert!(
                holds(&engine),
                "a running RLM child is lost by a daemon stop"
            );
        }
    }
}
