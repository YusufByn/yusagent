//! yusAgent (YA-10, ticket 40): the extension's providers and sign-in screens on the bridge,
//! `_yus/providers/*` (`docs/tickets/40-login-extension.md`, §16). Methods, answers and the
//! `login_update` steps follow Unharnessed's `_unharnessed/providers/*`
//! (`crates/uh-daemon/src/acp/providers.rs` at `5494a33a5`), renamed for yusAgent; the local
//! servers (`local_*`) are not served (decision 6).
//!
//! The flows themselves are the TUI's (`pa-cli`'s `provider_login`), reached through
//! [`ProviderSignIn`], which `pa-cli` installs when the ACP mode starts ([`install`]): this
//! crate does not depend on `pa-cli` or `pa-tui`. After a credential changes, the session's
//! worker reloads its request auth (`reload`) and the model picker is read again, so the
//! running conversation uses the new key and lists the new models (decision 2).
//!
//! A failed flow answers a fixed text, never the flow's own text, which can carry the body of
//! a token response (decision 5). Nothing here logs.
//!
//! Listed in `docs/fork-changes.md`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, MutexGuard, OnceLock, PoisonError};

use pa_types::daemon::DaemonCommand;
use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, oneshot, watch, Mutex};

use super::{DaemonAcpState, DaemonLink};
use crate::acp::config_options::{model_value, PickerModel, SessionConfigOption};
use crate::acp::jsonrpc::{self, Incoming};
use crate::acp::producer::FrameSink;
use crate::acp::wire_config::{
    fetch_available_models, fetch_connection_state, refresh_wire_config,
};

const LIST: &str = "_yus/providers/list";
const RELOAD: &str = "_yus/providers/reload";
const SET_API_KEY: &str = "_yus/providers/set_api_key";
const LOGOUT: &str = "_yus/providers/logout";
const LOGIN: &str = "_yus/providers/login";
const LOGIN_ANSWER: &str = "_yus/providers/login_answer";
const LOGIN_OPEN: &str = "_yus/providers/login_open";
const LOGIN_CANCEL: &str = "_yus/providers/login_cancel";
const LOGIN_UPDATE: &str = "_yus/providers/login_update";

/// The requests [`serve`] answers.
const REQUESTS: [&str; 7] = [
    LIST,
    RELOAD,
    SET_API_KEY,
    LOGOUT,
    LOGIN,
    LOGIN_ANSWER,
    LOGIN_OPEN,
];

/// What a [`ProviderSignIn`] call resolves to.
pub type SignInFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// One row of the providers screen (Unharnessed's `list` row, `local` aside).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // the row's flags, as the screen reads them
pub struct ProviderView {
    pub id: String,
    pub name: String,
    /// A subscription (`OAuth`) credential; otherwise an API key.
    pub subscription: bool,
    /// The agent runs the sign-in (a browser or device flow); otherwise the screen asks for a key.
    pub browser: bool,
    pub connected: bool,
    /// The credential is in the agent's `auth.json`, so it can be removed.
    pub removable: bool,
    /// Where an outside credential comes from.
    pub note: Option<String>,
}

/// How a credential change settled. `Failed` carries a fixed text to show, never a flow's own
/// text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInOutcome {
    Done,
    Failed(String),
    Cancelled,
}

/// One team of a team picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInTeam {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
}

/// The answer to a team picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamAnswer {
    /// A team, or `None` for the personal account.
    Pick(Option<String>),
    /// The stored team stays.
    Keep,
}

/// One step of a running sign-in, sent to the screen as a `login_update`.
#[derive(Debug)]
pub enum SignInEvent {
    Progress(String),
    /// The flow waits on the browser or the device.
    Waiting(String),
    Url {
        url: String,
        instructions: Option<String>,
    },
    /// A text to paste; `None` answers a dismissed prompt or a cancelled sign-in.
    Prompt {
        message: String,
        secret: bool,
        allow_empty: bool,
        /// An alternative to the browser sign-in running beside it.
        optional: bool,
        reply: oneshot::Sender<Option<String>>,
    },
    Team {
        teams: Vec<SignInTeam>,
        current: Option<String>,
        reply: oneshot::Sender<TeamAnswer>,
    },
}

/// The channel a sign-in reports its steps on, and its cancel signal.
pub struct SignInEvents {
    events: mpsc::UnboundedSender<SignInEvent>,
    cancelled: watch::Receiver<bool>,
}

impl SignInEvents {
    /// Sends one step to the screen; a step after the sign-in settled is dropped.
    pub fn send(&self, event: SignInEvent) {
        let _ = self.events.send(event);
    }

    /// Turns `true` when the client cancels the sign-in.
    #[must_use]
    pub fn cancelled(&self) -> watch::Receiver<bool> {
        self.cancelled.clone()
    }
}

/// The provider credentials, as the TUI's `/login` and `/logout` change them.
pub trait ProviderSignIn: Send + Sync {
    /// The rows of the providers screen.
    fn rows(&self) -> SignInFuture<Vec<ProviderView>>;
    /// Stores an API key for `provider_id`.
    fn set_api_key(&self, provider_id: String, api_key: String) -> SignInFuture<SignInOutcome>;
    /// Removes the stored credential of `provider_id`.
    fn logout(&self, provider_id: String) -> SignInFuture<SignInOutcome>;
    /// Runs the sign-in of `provider_id` (a subscription, Prime Inference, or `mcp:<server>`).
    fn login(&self, provider_id: String, events: SignInEvents) -> SignInFuture<SignInOutcome>;
}

static SIGN_IN: OnceLock<Arc<dyn ProviderSignIn>> = OnceLock::new();

/// Installs the flows the bridge serves; the first call wins.
pub fn install(sign_in: Arc<dyn ProviderSignIn>) {
    let _ = SIGN_IN.set(sign_in);
}

/// Whether [`serve`] answers `method`.
pub(super) fn serves(method: &str) -> bool {
    REQUESTS.contains(&method)
}

/// The client's notifications: `login_cancel` cancels the running sign-in.
pub(super) fn notified(incoming: &Incoming) {
    if let Incoming::Notification { method, .. } = incoming {
        if method == LOGIN_CANCEL {
            logins().cancel();
        }
    }
}

/// The bridge's answer when no flows were installed (a build without `pa-cli`).
const NOT_AVAILABLE: &str = "Provider sign-in is not available in this build.";
/// One sign-in at a time.
const RUNNING: &str = "a sign-in is already running";

/// Answers one request.
pub(super) async fn serve(
    method: &str,
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    let Some(sign_in) = SIGN_IN.get().cloned() else {
        let _ = tx.send(crate::acp::internal_error(id, NOT_AVAILABLE));
        return;
    };
    if method == LOGIN {
        // A sign-in can wait on the browser for long: it answers from its own task, through a
        // weak sender, so the end of stdin never waits for it.
        let sink = tx.downgrade();
        let (id, params) = (id.clone(), params.clone());
        let (link, state) = (Arc::clone(link), Arc::clone(state));
        tokio::spawn(async move {
            let response = login(sign_in.as_ref(), &id, &params, &link, &state, &sink).await;
            if let Some(tx) = sink.upgrade() {
                let _ = tx.send(response);
            }
        });
        return;
    }
    let sign_in = sign_in.as_ref();
    let response = match method {
        RELOAD => {
            refresh_session(link, state).await;
            jsonrpc::response(id, &json!({ "providers": rows_value(sign_in).await }))
        }
        SET_API_KEY => match (
            text_param(params, "providerId"),
            text_param(params, "apiKey"),
        ) {
            (Some(provider_id), Some(api_key)) => {
                let outcome = sign_in
                    .set_api_key(provider_id.to_string(), api_key.to_string())
                    .await;
                settled(sign_in, id, outcome, link, state).await
            }
            _ => invalid(id, "providerId and apiKey must be non-empty strings"),
        },
        LOGOUT => match text_param(params, "providerId") {
            Some(provider_id) => {
                let outcome = sign_in.logout(provider_id.to_string()).await;
                settled(sign_in, id, outcome, link, state).await
            }
            None => invalid(id, "providerId must be a non-empty string"),
        },
        LOGIN_ANSWER => match params.get("promptId").and_then(Value::as_u64) {
            Some(prompt_id) => {
                let answer = params.get("answer").unwrap_or(&Value::Null);
                match logins().answer(prompt_id, answer) {
                    Ok(()) => jsonrpc::response(id, &json!({})),
                    Err(refusal) => invalid(id, refusal),
                }
            }
            None => invalid(id, "promptId must be a number"),
        },
        LOGIN_OPEN => match logins().open() {
            Ok(()) => jsonrpc::response(id, &json!({})),
            Err(refusal) => invalid(id, refusal),
        },
        _ => jsonrpc::response(id, &json!({ "providers": rows_value(sign_in).await })),
    };
    let _ = tx.send(response);
}

/// H2 (`docs/tickets/40-login-extension.md`, §4.1): the run guard's texts for a missing or
/// unusable credential (`agent_engine/turn/model.rs`) are an authentication failure, which no
/// classifier rule recognizes. X5 (D4, §22): so are the providers' own texts for a missing
/// credential, raised at the request (`pa-ai`'s `providers/bedrock/mod.rs`, and `No API key for
/// provider:` in its `OpenAI`, Azure, Codex, Mistral and Anthropic providers). Every other `kind`
/// stays.
#[must_use]
pub(crate) fn auth_failure_kind(failure: &str, kind: &'static str) -> &'static str {
    const TEXTS: [&str; 4] = [
        "No API key found for ",
        "Authentication failed for \"",
        "No API key for provider: ",
        "No AWS credentials available for Bedrock",
    ];
    if TEXTS.iter().any(|text| failure.contains(text)) {
        "authentication"
    } else {
        kind
    }
}

/// `login`: the sign-in, then its outcome like a key's.
async fn login(
    sign_in: &dyn ProviderSignIn,
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
    sink: &mpsc::WeakUnboundedSender<Value>,
) -> Value {
    let Some(provider_id) = text_param(params, "providerId") else {
        return invalid(id, "providerId must be a non-empty string");
    };
    match logins().run(sign_in, provider_id, sink).await {
        Ok(outcome) => settled(sign_in, id, outcome, link, state).await,
        Err(refusal) => crate::acp::internal_error(id, refusal),
    }
}

/// The answer to a credential change (Unharnessed's `{providers, outcome, message?}`), after
/// the running session took a done change.
async fn settled(
    sign_in: &dyn ProviderSignIn,
    id: &Value,
    outcome: SignInOutcome,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
) -> Value {
    if outcome == SignInOutcome::Done {
        refresh_session(link, state).await;
    }
    let mut answer = json!({ "providers": rows_value(sign_in).await });
    match outcome {
        SignInOutcome::Done => answer["outcome"] = json!("done"),
        SignInOutcome::Failed(message) => {
            answer["outcome"] = json!("failed");
            answer["message"] = json!(message);
        }
        SignInOutcome::Cancelled => answer["outcome"] = json!("cancelled"),
    }
    jsonrpc::response(id, &answer)
}

/// The running session takes the stored credentials (the worker's `reload`: request auth and
/// MCP auth), and its model picker lists what they open (`config_option_update` when it
/// changes), one config operation at a time like `session/set_config_option`. Before
/// `session/new`, nothing runs yet.
///
/// X5 (D3, §22): the announced model is a fresh resolution (`model_metadata`), the engine's agent
/// keeps the model it was built with, and `reload` never moves it. When the announced model
/// changes to one the session can now use, the engine is switched onto it, as the picker does
/// (`set_model`; it also saves the model as the settings default, the daemon's own rule).
async fn refresh_session(link: &Arc<DaemonLink>, state: &Arc<Mutex<DaemonAcpState>>) {
    let hosted = {
        let guard = state.lock().await;
        guard.session.as_ref().map(|hosted| {
            (
                hosted.daemon_active_session_id.clone(),
                Arc::clone(&hosted.config),
                Arc::clone(&hosted.producer),
            )
        })
    };
    let Some((session_id, config, producer)) = hosted else {
        return;
    };
    let _queue = config.queue.lock().await;
    let shown = announced_model(&config.published.lock().await);
    let _ = link
        .request(DaemonCommand::Reload {
            id: None,
            active_session_id: session_id.clone(),
            rest: Map::new(),
        })
        .await;
    if let Ok(models) = fetch_available_models(link, &session_id).await {
        *config.models.lock().await = models;
    }
    let announced = fetch_connection_state(link, &session_id)
        .await
        .and_then(|state| PickerModel::from_connection_state(&state["model"]));
    if let Some(model) = announced {
        let value = model_value(&model.provider, &model.id);
        let usable = config
            .models
            .lock()
            .await
            .iter()
            .any(|available| model_value(&available.provider, &available.id) == value);
        if usable && shown.as_deref() != Some(value.as_str()) {
            let _ = link
                .request(DaemonCommand::SetModel {
                    id: None,
                    active_session_id: session_id.clone(),
                    provider: model.provider,
                    model_id: model.id,
                    rest: Map::new(),
                })
                .await;
        }
    }
    let _ = refresh_wire_config(link, &session_id, &config, &producer).await;
}

/// The model the client was last shown: the published `model` option's value.
fn announced_model(published: &[SessionConfigOption]) -> Option<String> {
    published
        .iter()
        .find(|option| option.id == "model")
        .map(|option| option.current_value.clone())
}

/// The rows of the providers screen.
async fn rows_value(sign_in: &dyn ProviderSignIn) -> Value {
    Value::Array(sign_in.rows().await.iter().map(row_value).collect())
}

fn row_value(view: &ProviderView) -> Value {
    let method = if view.subscription {
        "subscription"
    } else {
        "apiKey"
    };
    let login = if view.browser { "browser" } else { "apiKey" };
    let mut row = json!({
        "id": view.id,
        "name": view.name,
        "method": method,
        "login": login,
        "connected": view.connected,
        "removable": view.removable,
    });
    if let Some(note) = &view.note {
        row["note"] = json!(note);
    }
    row
}

/// A non-empty string parameter, trimmed.
fn text_param<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn invalid(id: &Value, details: &str) -> Value {
    jsonrpc::error_response(
        id,
        jsonrpc::INVALID_PARAMS,
        "Invalid params",
        Some(&json!({ "details": details })),
    )
}

/// The sign-in of this bridge process.
fn logins() -> &'static Logins {
    static LOGINS: OnceLock<Logins> = OnceLock::new();
    LOGINS.get_or_init(|| Logins::new(Box::new(pa_core::platform::browser::open_in_browser)))
}

/// What a page opener does with the sign-in page.
type Opener = Box<dyn Fn(&str) + Send + Sync>;

/// A prompt the screen has not answered yet.
enum Pending {
    Text {
        allow_empty: bool,
        reply: oneshot::Sender<Option<String>>,
    },
    Team(oneshot::Sender<TeamAnswer>),
}

/// The running sign-in.
struct Run {
    url: Option<String>,
    prompts: HashMap<u64, Pending>,
    cancel: watch::Sender<bool>,
}

/// One sign-in at a time (Unharnessed's rule), its prompts and its page.
struct Logins {
    run: std::sync::Mutex<Option<Run>>,
    next_prompt: AtomicU64,
    opener: Opener,
}

impl Logins {
    fn new(opener: Opener) -> Logins {
        Logins {
            run: std::sync::Mutex::new(None),
            next_prompt: AtomicU64::new(1),
            opener,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Run>> {
        self.run.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs the sign-in of `provider_id`, its steps sent on `sink` as `login_update`
    /// notifications, until the flow settles. `Err` when a sign-in already runs.
    async fn run(
        &self,
        sign_in: &dyn ProviderSignIn,
        provider_id: &str,
        sink: &mpsc::WeakUnboundedSender<Value>,
    ) -> Result<SignInOutcome, &'static str> {
        let (cancel, cancelled) = watch::channel(false);
        {
            let mut run = self.lock();
            if run.is_some() {
                return Err(RUNNING);
            }
            *run = Some(Run {
                url: None,
                prompts: HashMap::new(),
                cancel,
            });
        }
        let (events, mut steps) = mpsc::unbounded_channel();
        let mut flow = sign_in.login(provider_id.to_string(), SignInEvents { events, cancelled });
        let outcome = loop {
            tokio::select! {
                outcome = &mut flow => break outcome,
                Some(step) = steps.recv() => self.forward(provider_id, step, sink),
            }
        };
        // What the flow said before it settled still reaches the screen; a prompt no longer
        // answers.
        while let Ok(step) = steps.try_recv() {
            if !matches!(step, SignInEvent::Prompt { .. } | SignInEvent::Team { .. }) {
                self.forward(provider_id, step, sink);
            }
        }
        *self.lock() = None;
        Ok(outcome)
    }

    /// One step to the screen; a prompt is kept until its answer.
    fn forward(
        &self,
        provider_id: &str,
        step: SignInEvent,
        sink: &mpsc::WeakUnboundedSender<Value>,
    ) {
        let update = match step {
            SignInEvent::Progress(message) => json!({ "kind": "progress", "message": message }),
            SignInEvent::Waiting(message) => json!({ "kind": "waiting", "message": message }),
            SignInEvent::Url { url, instructions } => {
                if let Some(run) = self.lock().as_mut() {
                    run.url = Some(url.clone());
                }
                json!({ "kind": "url", "url": url, "instructions": instructions })
            }
            SignInEvent::Prompt {
                message,
                secret,
                allow_empty,
                optional,
                reply,
            } => {
                let prompt_id = self.keep(Pending::Text { allow_empty, reply });
                json!({
                    "kind": "prompt", "promptId": prompt_id, "message": message, "secret": secret,
                    "allowEmpty": allow_empty, "optional": optional,
                })
            }
            SignInEvent::Team {
                teams,
                current,
                reply,
            } => {
                let prompt_id = self.keep(Pending::Team(reply));
                let teams: Vec<Value> = teams
                    .iter()
                    .map(|team| json!({ "id": team.id, "name": team.name, "role": team.role }))
                    .collect();
                json!({ "kind": "team", "promptId": prompt_id, "teams": teams, "current": current })
            }
        };
        if let Some(tx) = sink.upgrade() {
            let params = json!({ "providerId": provider_id, "update": update });
            let _ = tx.send(jsonrpc::notification(LOGIN_UPDATE, &params));
        }
    }

    /// Keeps a prompt of the running sign-in under a new number.
    fn keep(&self, pending: Pending) -> u64 {
        let prompt_id = self.next_prompt.fetch_add(1, Ordering::Relaxed);
        if let Some(run) = self.lock().as_mut() {
            run.prompts.insert(prompt_id, pending);
        }
        prompt_id
    }

    /// Answers prompt `prompt_id` with the screen's `answer` (Unharnessed's `text`, `team`,
    /// `dismiss`).
    fn answer(&self, prompt_id: u64, answer: &Value) -> Result<(), &'static str> {
        let mut guard = self.lock();
        let run = guard.as_mut().ok_or("no sign-in is running")?;
        let pending = run.prompts.remove(&prompt_id).ok_or("no such prompt")?;
        match (pending, answer.get("kind").and_then(Value::as_str)) {
            (Pending::Text { allow_empty, reply }, Some("text")) => {
                let value = answer
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if value.trim().is_empty() && !allow_empty {
                    run.prompts
                        .insert(prompt_id, Pending::Text { allow_empty, reply });
                    return Err("the answer is empty");
                }
                let _ = reply.send(Some(value.to_string()));
            }
            (Pending::Text { reply, .. }, Some("dismiss")) => {
                let _ = reply.send(None);
            }
            (Pending::Team(reply), Some("team")) => {
                let team = answer
                    .get("teamId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let _ = reply.send(TeamAnswer::Pick(team));
            }
            (Pending::Team(reply), Some("dismiss")) => {
                let _ = reply.send(TeamAnswer::Keep);
            }
            (pending, _) => {
                run.prompts.insert(prompt_id, pending);
                return Err("the answer does not fit the prompt");
            }
        }
        Ok(())
    }

    /// Opens the running sign-in's page, never a page the client names.
    fn open(&self) -> Result<(), &'static str> {
        let url = self
            .lock()
            .as_ref()
            .and_then(|run| run.url.clone())
            .ok_or("no sign-in page to open")?;
        (self.opener)(&url);
        Ok(())
    }

    /// Cancels the running sign-in: its flow sees the signal, its prompts answer `None`.
    fn cancel(&self) {
        if let Some(run) = self.lock().as_mut() {
            let _ = run.cancel.send(true);
            run.prompts.clear();
        }
    }
}

#[cfg(test)]
#[path = "ya_providers_tests.rs"]
mod tests;
