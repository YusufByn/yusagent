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
//! Push 1 of R5: the contract and neutral bodies; the tests fail on their assertions.
//! Listed in `docs/fork-changes.md`.

// Push 1 of R5 (tests and skeleton): the bodies are neutral, removed with the code.
#![allow(
    dead_code,
    unused_variables,
    clippy::unused_async,
    clippy::unused_self,
    clippy::unnecessary_wraps
)]

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch, Mutex};

use super::{DaemonAcpState, DaemonLink};
use crate::acp::jsonrpc::{self, Incoming};
use crate::acp::producer::FrameSink;

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

/// Answers one request.
pub(super) async fn serve(
    method: &str,
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    let answer = match method {
        LIST | RELOAD => json!({ "providers": [] }),
        SET_API_KEY | LOGOUT => json!({ "providers": [], "outcome": "done" }),
        LOGIN => json!({ "providers": [], "outcome": "cancelled" }),
        _ => json!({}),
    };
    let _ = tx.send(jsonrpc::response(id, &answer));
}

/// H2 (`docs/tickets/40-login-extension.md`, §4.1): the run guard's texts for a missing or
/// unusable credential (`agent_engine/turn/model.rs`) are an authentication failure, which no
/// classifier rule recognizes. Every other `kind` stays.
#[must_use]
pub(crate) fn auth_failure_kind(failure: &str, kind: &'static str) -> &'static str {
    kind
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

    /// Runs the sign-in of `provider_id`, its steps sent on `sink` as `login_update`
    /// notifications, until the flow settles. `Err` when a sign-in already runs.
    async fn run(
        &self,
        sign_in: &dyn ProviderSignIn,
        provider_id: &str,
        sink: &mpsc::WeakUnboundedSender<Value>,
    ) -> Result<SignInOutcome, &'static str> {
        Ok(SignInOutcome::Cancelled)
    }

    /// Answers prompt `prompt_id` with the screen's `answer`.
    fn answer(&self, prompt_id: u64, answer: &Value) -> Result<(), &'static str> {
        Ok(())
    }

    /// Opens the running sign-in's page, never a page the client names.
    fn open(&self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Cancels the running sign-in: its flow sees the signal, its prompts answer `None`.
    fn cancel(&self) {}

    fn next_prompt_id(&self) -> u64 {
        self.next_prompt.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
#[path = "ya_providers_tests.rs"]
mod tests;
