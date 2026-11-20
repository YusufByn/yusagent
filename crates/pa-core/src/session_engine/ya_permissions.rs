//! yusAgent (YA-11, ticket 20, PR 1): permissions of the tool calls.
//!
//! The setting `"permissions"` (`"manual"` or `"auto"`) is read from the **global** settings
//! only (`<agent dir>/settings.json`), never from the project's `.yusagent/settings.json`: a
//! cloned repository must not be able to turn the confirmations off (the merged settings let the
//! project win, `crate::settings::SettingsManager::from_storage`). Unset is `"manual"`.
//!
//! - `"manual"`: every tool call (`python_repl` cell, `apply_patch`, the native shell tools) is
//!   put to a [`PermissionAsker`] before it runs, with no classification (decision of Yusuf,
//!   20/11/2026, D2 revised). A declined call does not run: its result is the fixed text
//!   [`DENIED_TEXT`] and **the turn stops** after the tool batch (D7). The other calls of the same
//!   assistant message are not asked and get the same text. Each decision is journaled in the
//!   session file as a custom entry [`JOURNAL_TYPE`] (never sent to the model).
//! - `"auto"`: the calls run without asking, as upstream. Without a sandbox (none before the
//!   second part of YA-11), a session without a daemon prints [`AUTO_WARNING`] on stderr at
//!   startup when the mode comes from the user's settings (D4).
//!
//! A session in `"manual"` whose embedding cannot ask (no asker: `-p`, RPC, and the daemon until
//! PR 2 of the ticket) is refused at creation, before any model request, with a message that
//! gives the exact line to add and the path of the global settings (D6).
//!
//! Nothing here touches the system prompt, the tool definitions or the request settings frozen
//! in `yaRequest`: the prefix sent to the model is the same in both modes. A declined call only
//! appends a tool result with a fixed text to the history.
//!
//! Debug builds only: [`TEST_ENV`] overrides the setting, so that the upstream tests keep running
//! tools without an asker (`.cargo/config.toml` sets it to `auto`, as it sets
//! `PRIME_AGENT_PROMPT_VERSION`). Release builds ignore it. Listed in `docs/fork-changes.md`.
//!
//! PR 2 of the ticket (the daemon and ACP): an embedding that can ask may also
//! - journal elsewhere than the engine's session manager ([`PermissionsWiring::journal`]: the
//!   daemon's engine runs on an in-memory manager, the worker owns the durable file);
//! - switch the mode of the session while it runs ([`PermissionsWiring::switch`], ACP
//!   `session/set_config_option`). With a switch and an asker, the gate is installed in both
//!   modes and reads the live mode before each call; `"auto"` lets the call run without asking
//!   and without journaling it. Each change is journaled by the embedding
//!   ([`mode_change_entry`]).
//!
//! A request dropped before its answer (the turn is aborted while it waits: ACP
//! `session/cancel`) is journaled as `"cancelled"`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pa_agent::abort::AbortSignal;
use pa_agent::agent_loop::{BeforeToolCallFn, ShouldStopAfterTurnFn};
use pa_agent::types::{BeforeToolCallContext, BeforeToolCallResult};
use pa_agent::BoxFut;
use serde_json::{json, Value};

use crate::session::manager::SessionManager;

/// The key in the global `settings.json`.
pub const SETTING_KEY: &str = "permissions";

/// Debug builds only: overrides the setting (`manual` or `auto`) for the tests.
pub const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";

/// The result of a declined (or cancelled) tool call, as the model reads it. Fixed: no variable
/// data, so the history stays byte-identical across requests. The marker lets the bench count
/// refusals.
pub const DENIED_TEXT: &str =
    "[permission-denied] The user declined this cell; nothing was executed.";

/// The custom entry type of a decision in the session file.
pub const JOURNAL_TYPE: &str = "ya-permission";

/// Printed on stderr at startup by a session without a daemon in `"auto"` (no sandbox yet).
pub const AUTO_WARNING: &str = "yusAgent: permissions are \"auto\" and there is no sandbox: every tool call runs without confirmation, with your user's rights.";

/// The permission mode of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Every tool call is confirmed before it runs (the default).
    Manual,
    /// Tool calls run without confirmation.
    Auto,
}

impl PermissionMode {
    /// The value of the setting (`"manual"`, `"auto"`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }

    /// The mode of a setting value; `None` for anything else.
    #[must_use]
    pub fn from_label(value: &str) -> Option<Self> {
        parse_mode(value)
    }
}

/// Where the mode of a session comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeSource {
    /// No setting: `"manual"`.
    Default,
    /// The global `settings.json`.
    GlobalSettings,
    /// [`TEST_ENV`], debug builds only.
    TestOverride,
    /// Set by the embedding ([`PermissionsWiring::mode`]).
    Embedding,
}

/// One tool call put to the user.
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    /// The validated arguments (the code of a cell, the patch of `apply_patch`).
    pub args: Value,
}

/// The answer to a [`PermissionRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Run the call.
    Allow,
    /// Do not run it; the turn stops.
    Deny,
    /// The request was withdrawn (the turn is being cancelled): treated as a refusal.
    Cancelled,
}

impl PermissionDecision {
    fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Asks the user about one tool call. The embedding provides it (ACP and the TUI in the next
/// PRs of the ticket); `-p` and RPC have none.
pub trait PermissionAsker: Send + Sync {
    fn ask(
        &self,
        request: PermissionRequest,
        signal: AbortSignal,
    ) -> BoxFut<'static, PermissionDecision>;
}

/// Writes one journal entry (the `data` of a [`JOURNAL_TYPE`] custom entry) where the embedding
/// keeps the session file.
pub type PermissionJournal = Arc<dyn Fn(Value) + Send + Sync>;

/// The live mode of a session that can ask, switched by the user (ACP
/// `session/set_config_option`). Shared by the embedding and the gate; it outlives a rebuild of
/// the engine, so a choice made before (or between) builds holds.
#[derive(Debug, Default)]
pub struct PermissionSwitch {
    state: Mutex<SwitchState>,
}

#[derive(Debug, Default, Clone, Copy)]
struct SwitchState {
    /// The user's choice for this session; `None` follows the global setting.
    chosen: Option<PermissionMode>,
    /// The mode in force, once the engine is built.
    current: Option<PermissionMode>,
}

impl PermissionSwitch {
    fn state(&self) -> SwitchState {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The mode in force; `None` before the first build.
    #[must_use]
    pub fn current(&self) -> Option<PermissionMode> {
        self.state().current
    }

    /// The user's choice for this session, if any.
    #[must_use]
    pub fn chosen(&self) -> Option<PermissionMode> {
        self.state().chosen
    }

    /// The user's choice: in force at once, and for the next builds of this session. Returns the
    /// mode in force before (or the earlier choice, before the first build).
    #[must_use]
    pub fn choose(&self, mode: PermissionMode) -> Option<PermissionMode> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = state.current.or(state.chosen);
        state.chosen = Some(mode);
        state.current = Some(mode);
        previous
    }

    /// A build resolved `mode`.
    fn start(&self, mode: PermissionMode) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current = Some(mode);
    }
}

/// What the embedding passes to [`crate::session_engine::engine::create_session_with`].
#[derive(Clone, Default)]
pub struct PermissionsWiring {
    /// Overrides the setting (tests). `None` reads the user's choice ([`Self::switch`]), then
    /// the global settings.
    pub mode: Option<PermissionMode>,
    /// Who answers in `"manual"`. `None`: a manual session is refused at creation.
    pub asker: Option<Arc<dyn PermissionAsker>>,
    /// Where the decisions are journaled. `None`: the engine's session manager (its file when it
    /// has one).
    pub journal: Option<PermissionJournal>,
    /// The live mode, switchable while the session runs. With an asker, the gate is installed
    /// in both modes.
    pub switch: Option<Arc<PermissionSwitch>>,
}

/// The value of [`TEST_ENV`], in debug builds only.
fn test_override() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(TEST_ENV).ok()
    } else {
        None
    }
}

fn parse_mode(value: &str) -> Option<PermissionMode> {
    match value {
        "manual" => Some(PermissionMode::Manual),
        "auto" => Some(PermissionMode::Auto),
        _ => None,
    }
}

/// The global settings file of `agent_dir`.
#[must_use]
pub fn global_settings_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("settings.json")
}

/// The mode from the global settings document (`global_raw`) and the test override.
///
/// # Errors
///
/// A value other than `"manual"` or `"auto"`, with the path of the file.
pub fn mode_from(
    global_raw: Option<&Value>,
    test_override: Option<&str>,
    settings_path: &Path,
) -> Result<(PermissionMode, ModeSource), String> {
    if let Some(value) = test_override {
        return parse_mode(value)
            .map(|mode| (mode, ModeSource::TestOverride))
            .ok_or_else(|| format!("{TEST_ENV} must be \"manual\" or \"auto\", got {value:?}"));
    }
    match global_raw.and_then(|raw| raw.get(SETTING_KEY)) {
        None => Ok((PermissionMode::Manual, ModeSource::Default)),
        Some(value) => value
            .as_str()
            .and_then(parse_mode)
            .map(|mode| (mode, ModeSource::GlobalSettings))
            .ok_or_else(|| {
                format!(
                    "invalid \"{SETTING_KEY}\" in {}: expected \"manual\" or \"auto\", got {value}",
                    settings_path.display()
                )
            }),
    }
}

/// The refusal of a manual session that cannot ask (D6).
#[must_use]
pub fn no_asker_message(agent_dir: &Path) -> String {
    format!(
        "permissions are \"manual\" (the default) and this mode cannot ask for confirmation. \
         To run tool calls without confirmation (there is no sandbox yet), add this line to {}:\n  \
         \"{SETTING_KEY}\": \"auto\"\n\
         A project's .yusagent/settings.json is not read for this setting.",
        global_settings_path(agent_dir).display()
    )
}

/// The mode the global settings give (and the test override, in debug builds), as [`resolve`]
/// reads it, without an embedding's choice.
///
/// # Errors
///
/// An invalid setting, with the path of the file.
pub fn configured_mode(
    global_raw: Option<&Value>,
    agent_dir: &Path,
) -> Result<PermissionMode, String> {
    mode_from(
        global_raw,
        test_override().as_deref(),
        &global_settings_path(agent_dir),
    )
    .map(|(mode, _)| mode)
}

/// The journal entry of a change of mode by the user.
#[must_use]
pub fn mode_change_entry(previous: PermissionMode, mode: PermissionMode) -> Value {
    json!({
        "mode": mode.label(),
        "previousMode": previous.label(),
        "change": "user",
    })
}

/// The mode of a new session: refuses a manual session without an asker, and prints
/// [`AUTO_WARNING`] when `announce` and the mode comes from the user's settings.
///
/// # Errors
///
/// [`no_asker_message`], or an invalid setting.
pub fn resolve(
    global_raw: Option<&Value>,
    agent_dir: &Path,
    wiring: &PermissionsWiring,
    announce: bool,
) -> anyhow::Result<PermissionMode> {
    let chosen = wiring
        .mode
        .or_else(|| wiring.switch.as_deref().and_then(PermissionSwitch::chosen));
    let (mode, source) = match chosen {
        Some(mode) => (mode, ModeSource::Embedding),
        None => mode_from(
            global_raw,
            test_override().as_deref(),
            &global_settings_path(agent_dir),
        )
        .map_err(anyhow::Error::msg)?,
    };
    if mode == PermissionMode::Manual && wiring.asker.is_none() {
        anyhow::bail!(no_asker_message(agent_dir));
    }
    if mode == PermissionMode::Auto && source == ModeSource::GlobalSettings && announce {
        eprintln!("{AUTO_WARNING}");
    }
    if let Some(switch) = &wiring.switch {
        switch.start(mode);
    }
    Ok(mode)
}

/// The confirmation of the tool calls of one manual session (or of a session whose mode can be
/// switched, [`PermissionsWiring::switch`]).
#[derive(Clone)]
pub struct PermissionGate {
    asker: Arc<dyn PermissionAsker>,
    session: Arc<tokio::sync::Mutex<SessionManager>>,
    journal: Option<PermissionJournal>,
    switch: Option<Arc<PermissionSwitch>>,
    /// The tool calls declined (or skipped after a refusal) whose turn has not ended yet.
    declined: Arc<Mutex<HashSet<String>>>,
}

/// Journals `"cancelled"` when a request is dropped before its answer (the turn was aborted
/// while it waited).
struct Unanswered<'a> {
    gate: &'a PermissionGate,
    tool_call_id: &'a str,
    tool_name: &'a str,
    answered: bool,
}

impl Drop for Unanswered<'_> {
    fn drop(&mut self) {
        if !self.answered {
            self.gate.journal_detached(journal_data(
                self.tool_call_id,
                self.tool_name,
                PermissionDecision::Cancelled.label(),
            ));
        }
    }
}

/// The `data` of the journal entry of one decision.
fn journal_data(tool_call_id: &str, tool_name: &str, decision: &str) -> Value {
    json!({
        "mode": PermissionMode::Manual.label(),
        "toolCallId": tool_call_id,
        "toolName": tool_name,
        "decision": decision,
    })
}

impl PermissionGate {
    /// The gate of a session in `mode`: `None` when there is nothing to install (`"auto"` without
    /// a switch, or no asker).
    #[must_use]
    pub fn new(
        mode: PermissionMode,
        wiring: &PermissionsWiring,
        session: Arc<tokio::sync::Mutex<SessionManager>>,
    ) -> Option<Self> {
        let asker = wiring.asker.as_ref()?;
        if mode == PermissionMode::Auto && wiring.switch.is_none() {
            return None;
        }
        Some(Self {
            asker: Arc::clone(asker),
            session,
            journal: wiring.journal.clone(),
            switch: wiring.switch.clone(),
            declined: Arc::default(),
        })
    }

    /// Whether the user switched the session to `"auto"`.
    fn switched_to_auto(&self) -> bool {
        self.switch
            .as_ref()
            .is_some_and(|switch| switch.current() == Some(PermissionMode::Auto))
    }

    /// The `before_tool_call` hook: asks, journals, and blocks a declined call.
    #[must_use]
    pub fn before_tool_call(&self) -> BeforeToolCallFn {
        let gate = self.clone();
        Arc::new(move |context: BeforeToolCallContext, signal: AbortSignal| {
            let gate = gate.clone();
            Box::pin(async move { Ok(gate.decide(&context, signal).await) })
        })
    }

    async fn decide(
        &self,
        context: &BeforeToolCallContext,
        signal: AbortSignal,
    ) -> Option<BeforeToolCallResult> {
        let call = &context.tool_call;
        if self.switched_to_auto() {
            return None;
        }
        // A refusal ends the turn: the other calls of the same message are not asked.
        let sibling_declined = {
            let declined = self.declined.lock().expect("declined lock");
            context
                .assistant_message
                .tool_calls()
                .iter()
                .any(|other| other.id != call.id && declined.contains(&other.id))
        };
        let decision = if sibling_declined {
            None
        } else {
            let mut unanswered = Unanswered {
                gate: self,
                tool_call_id: &call.id,
                tool_name: &call.name,
                answered: false,
            };
            let decision = self
                .asker
                .ask(
                    PermissionRequest {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        args: context.args.clone(),
                    },
                    signal,
                )
                .await;
            unanswered.answered = true;
            Some(decision)
        };
        self.journal(journal_data(
            &call.id,
            &call.name,
            decision.map_or("skipped", PermissionDecision::label),
        ))
        .await;
        if decision == Some(PermissionDecision::Allow) {
            return None;
        }
        self.declined
            .lock()
            .expect("declined lock")
            .insert(call.id.clone());
        Some(BeforeToolCallResult {
            block: true,
            reason: Some(DENIED_TEXT.to_string()),
        })
    }

    async fn journal(&self, data: Value) {
        if let Some(journal) = &self.journal {
            journal(data);
            return;
        }
        // A journal that cannot be written does not change the decision.
        let _ = self
            .session
            .lock()
            .await
            .append_custom_entry(JOURNAL_TYPE, Some(data));
    }

    /// [`Self::journal`] from a destructor: the engine's session manager is written by a task.
    fn journal_detached(&self, data: Value) {
        if let Some(journal) = &self.journal {
            journal(data);
            return;
        }
        let session = Arc::clone(&self.session);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = session
                    .lock()
                    .await
                    .append_custom_entry(JOURNAL_TYPE, Some(data));
            });
        }
    }

    /// Whether the batch that just ended had a declined call (and forgets its calls).
    fn take_declined(&self, tool_call_ids: impl IntoIterator<Item = String>) -> bool {
        let mut declined = self.declined.lock().expect("declined lock");
        let mut any = false;
        for id in tool_call_ids {
            any |= declined.remove(&id);
        }
        any
    }
}

/// `inner` (the embedding's turn boundary), preceded by a stop when the batch that just ended had
/// a declined call.
#[must_use]
pub fn wrap_should_stop(
    gate: Option<&PermissionGate>,
    inner: Option<ShouldStopAfterTurnFn>,
) -> Option<ShouldStopAfterTurnFn> {
    let Some(gate) = gate.cloned() else {
        return inner;
    };
    Some(Arc::new(move |context| {
        let declined = gate.take_declined(
            context
                .tool_results
                .iter()
                .map(|result| result.tool_call_id.clone()),
        );
        let inner = inner.clone();
        Box::pin(async move {
            if declined {
                return Ok(true);
            }
            match inner {
                Some(hook) => hook(context).await,
                None => Ok(false),
            }
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        PathBuf::from("/home/u/.yusagent/settings.json")
    }

    #[test]
    fn unset_is_manual() {
        assert_eq!(
            mode_from(None, None, &path()),
            Ok((PermissionMode::Manual, ModeSource::Default))
        );
        assert_eq!(
            mode_from(Some(&json!({"other": 1})), None, &path()),
            Ok((PermissionMode::Manual, ModeSource::Default))
        );
    }

    #[test]
    fn global_values() {
        assert_eq!(
            mode_from(Some(&json!({"permissions": "auto"})), None, &path()),
            Ok((PermissionMode::Auto, ModeSource::GlobalSettings))
        );
        assert_eq!(
            mode_from(Some(&json!({"permissions": "manual"})), None, &path()),
            Ok((PermissionMode::Manual, ModeSource::GlobalSettings))
        );
        let error = mode_from(Some(&json!({"permissions": "yes"})), None, &path()).unwrap_err();
        assert!(error.contains("/home/u/.yusagent/settings.json"), "{error}");
        assert!(mode_from(Some(&json!({"permissions": true})), None, &path()).is_err());
    }

    #[test]
    fn test_override_wins() {
        assert_eq!(
            mode_from(
                Some(&json!({"permissions": "manual"})),
                Some("auto"),
                &path()
            ),
            Ok((PermissionMode::Auto, ModeSource::TestOverride))
        );
        assert!(mode_from(None, Some("never"), &path()).is_err());
    }

    #[test]
    fn manual_without_asker_is_refused_with_the_exact_line_and_path() {
        let wiring = PermissionsWiring {
            mode: Some(PermissionMode::Manual),
            ..PermissionsWiring::default()
        };
        let error = resolve(None, Path::new("/home/u/.yusagent"), &wiring, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("/home/u/.yusagent/settings.json"), "{error}");
        assert!(error.contains("\n  \"permissions\": \"auto\"\n"), "{error}");
    }

    #[test]
    fn auto_installs_no_gate() {
        let session = Arc::new(tokio::sync::Mutex::new(SessionManager::in_memory(
            Path::new("/tmp"),
        )));
        let wiring = PermissionsWiring::default();
        assert!(PermissionGate::new(PermissionMode::Auto, &wiring, session).is_none());
    }

    // The turn of a manual session (D7), on the scripted provider of the engine tests.

    use crate::session_engine::engine::{create_session_with, SessionEngine, SessionEngineConfig};
    use crate::session_engine::tool_bridge::bridge_tool;
    use crate::session_engine::PromptOptions;
    use crate::tools::tool_definition::{ExecutionMode, ToolDefinition, ToolExecutionResult};
    use pa_agent::scripted::ScriptedProvider;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Answers every request with `decision` and records the requests.
    struct ScriptedAsker {
        decision: PermissionDecision,
        asked: Mutex<Vec<PermissionRequest>>,
    }

    impl PermissionAsker for ScriptedAsker {
        fn ask(
            &self,
            request: PermissionRequest,
            _signal: AbortSignal,
        ) -> BoxFut<'static, PermissionDecision> {
            self.asked.lock().unwrap().push(request);
            let decision = self.decision;
            Box::pin(async move { decision })
        }
    }

    fn model() -> pa_agent::types::Model {
        pa_agent::types::Model {
            id: "m".into(),
            name: "m".into(),
            api: "test".into(),
            provider: "test".into(),
            base_url: "http://localhost".into(),
            reasoning: false,
            cost: pa_agent::types::UsageCost::default(),
            context_window: 1_000,
            max_tokens: 100,
            max_tokens_explicit: false,
        }
    }

    /// A tool that counts its runs.
    fn counting_tool(runs: Arc<AtomicUsize>) -> Arc<dyn pa_agent::types::AgentTool> {
        bridge_tool(ToolDefinition {
            name: "echo".to_string(),
            label: "Echo".to_string(),
            description: "Echoes its input".to_string(),
            prompt_snippet: String::new(),
            parameters: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
            execution_mode: Some(ExecutionMode::Sequential),
            prepare_arguments: None,
            execute: Arc::new(move |_id, _params, _signal, _on_update| {
                runs.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(ToolExecutionResult::text("echoed")) })
            }),
        })
    }

    struct ManualSession {
        engine: SessionEngine,
        provider: Arc<ScriptedProvider>,
        asker: Arc<ScriptedAsker>,
        runs: Arc<AtomicUsize>,
        _dir: tempfile::TempDir,
    }

    async fn manual_session(
        decision: PermissionDecision,
        turns: impl FnOnce(&ScriptedProvider),
    ) -> ManualSession {
        session_with(
            decision,
            PermissionsWiring {
                mode: Some(PermissionMode::Manual),
                ..PermissionsWiring::default()
            },
            turns,
        )
        .await
    }

    /// A session on `wiring`, whose asker answers `decision`.
    async fn session_with(
        decision: PermissionDecision,
        mut wiring: PermissionsWiring,
        turns: impl FnOnce(&ScriptedProvider),
    ) -> ManualSession {
        let provider = Arc::new(ScriptedProvider::new(model()));
        turns(&provider);
        let asker = Arc::new(ScriptedAsker {
            decision,
            asked: Mutex::default(),
        });
        let runs = Arc::new(AtomicUsize::new(0));
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        wiring.asker = Some(Arc::clone(&asker) as Arc<dyn PermissionAsker>);
        let engine = create_session_with(
            SessionEngineConfig {
                cwd,
                agent_dir: dir.path().join("agent"),
                model: Some(model()),
                stream_fn: Some(provider.stream_fn()),
                tools: vec![counting_tool(Arc::clone(&runs))],
                daemonless: true,
                ..Default::default()
            },
            wiring,
        )
        .await
        .unwrap();
        ManualSession {
            engine,
            provider,
            asker,
            runs,
            _dir: dir,
        }
    }

    async fn prompt(engine: &SessionEngine, text: &str) {
        engine.prompt(text, PromptOptions::default()).await.unwrap();
        engine.session.agent().wait_for_idle().await;
    }

    /// The tool results of the session, as `(tool call id, text, is error)`.
    async fn tool_results(engine: &SessionEngine) -> Vec<(String, String, bool)> {
        engine
            .session
            .agent()
            .state()
            .await
            .messages
            .iter()
            .filter_map(|message| match message {
                pa_agent::types::AgentMessage::Standard(pa_agent::types::Message::ToolResult(
                    result,
                )) => Some((
                    result.tool_call_id.clone(),
                    result
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            pa_agent::types::ToolResultContent::Text(text) => {
                                Some(text.text.clone())
                            }
                            pa_agent::types::ToolResultContent::Image(_) => None,
                        })
                        .collect::<String>(),
                    result.is_error,
                )),
                _ => None,
            })
            .collect()
    }

    /// The journaled decisions, as `(tool call id, decision)`.
    async fn journal(engine: &SessionEngine) -> Vec<(String, String)> {
        engine
            .session
            .entries()
            .await
            .into_iter()
            .filter_map(|entry| match entry {
                pa_types::session::FileEntry::Custom { payload, .. }
                    if payload.custom_type == JOURNAL_TYPE =>
                {
                    let data = payload.data.unwrap_or_default();
                    Some((
                        data["toolCallId"].as_str().unwrap_or_default().to_string(),
                        data["decision"].as_str().unwrap_or_default().to_string(),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// D7: a declined call does not run, its result is the fixed text, and the turn stops: the
    /// provider gets no other request in that turn. The next prompt resumes normally.
    #[tokio::test]
    async fn a_declined_call_does_not_run_and_stops_the_turn() {
        let session = manual_session(PermissionDecision::Deny, |provider| {
            provider.push_tool_call_turn(None, vec![("call-1", "echo", json!({ "text": "hi" }))]);
            provider.push_text_turn("second prompt answer");
        })
        .await;
        prompt(&session.engine, "run the echo tool").await;

        assert_eq!(
            session.provider.calls().len(),
            1,
            "no request after the refusal"
        );
        assert_eq!(
            session.runs.load(Ordering::SeqCst),
            0,
            "the tool did not run"
        );
        let asked = session.asker.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].tool_name, "echo");
        assert_eq!(asked[0].args, json!({ "text": "hi" }));
        assert_eq!(
            tool_results(&session.engine).await,
            vec![("call-1".to_string(), DENIED_TEXT.to_string(), true)]
        );
        assert_eq!(
            journal(&session.engine).await,
            vec![("call-1".to_string(), "deny".to_string())]
        );

        prompt(&session.engine, "go on").await;
        assert_eq!(
            session.provider.calls().len(),
            2,
            "the next prompt reaches the model"
        );
    }

    /// A cancelled request is a refusal: same text, same stop.
    #[tokio::test]
    async fn a_cancelled_request_stops_the_turn_like_a_refusal() {
        let session = manual_session(PermissionDecision::Cancelled, |provider| {
            provider.push_tool_call_turn(None, vec![("call-1", "echo", json!({ "text": "hi" }))]);
            provider.push_text_turn("not requested");
        })
        .await;
        prompt(&session.engine, "run the echo tool").await;

        assert_eq!(session.provider.calls().len(), 1);
        assert_eq!(session.runs.load(Ordering::SeqCst), 0);
        assert_eq!(
            journal(&session.engine).await,
            vec![("call-1".to_string(), "cancelled".to_string())]
        );
    }

    /// An allowed call runs and the turn goes on to the next model request.
    #[tokio::test]
    async fn an_allowed_call_runs_and_the_turn_goes_on() {
        let session = manual_session(PermissionDecision::Allow, |provider| {
            provider.push_tool_call_turn(None, vec![("call-1", "echo", json!({ "text": "hi" }))]);
            provider.push_text_turn("done");
        })
        .await;
        prompt(&session.engine, "run the echo tool").await;

        assert_eq!(session.provider.calls().len(), 2);
        assert_eq!(session.runs.load(Ordering::SeqCst), 1);
        assert_eq!(
            tool_results(&session.engine).await,
            vec![("call-1".to_string(), "echoed".to_string(), false)]
        );
        assert_eq!(
            journal(&session.engine).await,
            vec![("call-1".to_string(), "allow".to_string())]
        );
    }

    /// After a refusal, the other calls of the same message are not asked and do not run.
    #[tokio::test]
    async fn the_other_calls_of_a_declined_message_are_skipped() {
        let session = manual_session(PermissionDecision::Deny, |provider| {
            provider.push_tool_call_turn(
                None,
                vec![
                    ("call-1", "echo", json!({ "text": "one" })),
                    ("call-2", "echo", json!({ "text": "two" })),
                ],
            );
            provider.push_text_turn("not requested");
        })
        .await;
        prompt(&session.engine, "run the echo tool twice").await;

        assert_eq!(session.provider.calls().len(), 1);
        assert_eq!(session.runs.load(Ordering::SeqCst), 0);
        assert_eq!(session.asker.asked.lock().unwrap().len(), 1, "asked once");
        assert_eq!(
            tool_results(&session.engine).await,
            vec![
                ("call-1".to_string(), DENIED_TEXT.to_string(), true),
                ("call-2".to_string(), DENIED_TEXT.to_string(), true),
            ]
        );
        assert_eq!(
            journal(&session.engine).await,
            vec![
                ("call-1".to_string(), "deny".to_string()),
                ("call-2".to_string(), "skipped".to_string()),
            ]
        );
    }

    /// A journal that collects the entries (the daemon's worker writes them to its file).
    fn collecting_journal() -> (PermissionJournal, Arc<Mutex<Vec<Value>>>) {
        let entries: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = Arc::clone(&entries);
        let journal: PermissionJournal =
            Arc::new(move |data: Value| sink.lock().unwrap().push(data));
        (journal, entries)
    }

    #[test]
    fn the_switch_reports_the_mode_before_the_choice() {
        let switch = PermissionSwitch::default();
        assert_eq!(switch.current(), None);
        assert_eq!(switch.choose(PermissionMode::Auto), None);
        assert_eq!(switch.chosen(), Some(PermissionMode::Auto));
        assert_eq!(
            switch.choose(PermissionMode::Manual),
            Some(PermissionMode::Auto)
        );
        assert_eq!(switch.current(), Some(PermissionMode::Manual));
    }

    /// PR 2: with a switch, the gate follows the live mode. In "auto" the call runs without a
    /// request and nothing is journaled; back in "manual", the next call is asked again. The
    /// decisions go to the embedding's journal, not to the engine's session manager.
    #[tokio::test]
    async fn the_switch_turns_the_requests_off_and_on_while_the_session_runs() {
        let switch = Arc::new(PermissionSwitch::default());
        let _ = switch.choose(PermissionMode::Auto);
        let (journal_sink, journaled) = collecting_journal();
        let session = session_with(
            PermissionDecision::Allow,
            PermissionsWiring {
                journal: Some(journal_sink),
                switch: Some(Arc::clone(&switch)),
                ..PermissionsWiring::default()
            },
            |provider| {
                provider
                    .push_tool_call_turn(None, vec![("call-1", "echo", json!({ "text": "a" }))]);
                provider.push_text_turn("first done");
                provider
                    .push_tool_call_turn(None, vec![("call-2", "echo", json!({ "text": "b" }))]);
                provider.push_text_turn("second done");
            },
        )
        .await;
        prompt(&session.engine, "run the echo tool").await;
        assert_eq!(session.runs.load(Ordering::SeqCst), 1);
        assert!(
            session.asker.asked.lock().unwrap().is_empty(),
            "not asked in auto"
        );
        assert!(
            journaled.lock().unwrap().is_empty(),
            "nothing journaled in auto"
        );

        assert_eq!(
            switch.choose(PermissionMode::Manual),
            Some(PermissionMode::Auto)
        );
        prompt(&session.engine, "run it again").await;
        assert_eq!(session.runs.load(Ordering::SeqCst), 2);
        let asked = session.asker.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].tool_call_id, "call-2");
        let journaled = journaled.lock().unwrap().clone();
        assert_eq!(journaled.len(), 1);
        assert_eq!(journaled[0]["toolCallId"], "call-2");
        assert_eq!(journaled[0]["decision"], "allow");
        assert!(
            journal(&session.engine).await.is_empty(),
            "the engine's session manager is not written"
        );
    }

    /// Never answers; signals when it is asked.
    struct SilentAsker(Arc<tokio::sync::Notify>);

    impl PermissionAsker for SilentAsker {
        fn ask(
            &self,
            _request: PermissionRequest,
            _signal: AbortSignal,
        ) -> BoxFut<'static, PermissionDecision> {
            self.0.notify_one();
            Box::pin(std::future::pending())
        }
    }

    /// PR 2 (D7): a request still waiting when the turn is aborted (ACP `session/cancel`) is
    /// journaled as "cancelled", the call does not run, and the model gets no other request.
    #[tokio::test]
    async fn a_request_dropped_by_an_abort_is_journaled_as_cancelled() {
        let provider = Arc::new(ScriptedProvider::new(model()));
        provider.push_tool_call_turn(None, vec![("call-1", "echo", json!({ "text": "hi" }))]);
        provider.push_text_turn("not requested");
        let asked = Arc::new(tokio::sync::Notify::new());
        let (journal_sink, journaled) = collecting_journal();
        let runs = Arc::new(AtomicUsize::new(0));
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let engine = create_session_with(
            SessionEngineConfig {
                cwd,
                agent_dir: dir.path().join("agent"),
                model: Some(model()),
                stream_fn: Some(provider.stream_fn()),
                tools: vec![counting_tool(Arc::clone(&runs))],
                daemonless: true,
                ..Default::default()
            },
            PermissionsWiring {
                mode: Some(PermissionMode::Manual),
                asker: Some(Arc::new(SilentAsker(Arc::clone(&asked))) as Arc<dyn PermissionAsker>),
                journal: Some(journal_sink),
                switch: None,
            },
        )
        .await
        .unwrap();
        let abort = async {
            asked.notified().await;
            engine.session.agent().abort();
        };
        // The aborted turn may answer with an error: only its effects are checked.
        let turn = async {
            let _ = engine
                .prompt("run the echo tool", PromptOptions::default())
                .await;
            engine.session.agent().wait_for_idle().await;
        };
        let ((), ()) = tokio::join!(turn, abort);

        assert_eq!(runs.load(Ordering::SeqCst), 0, "the tool did not run");
        assert_eq!(provider.calls().len(), 1, "no request after the abort");
        let journaled = journaled.lock().unwrap().clone();
        assert_eq!(journaled.len(), 1, "{journaled:?}");
        assert_eq!(journaled[0]["toolCallId"], "call-1");
        assert_eq!(journaled[0]["decision"], "cancelled");
    }
}
