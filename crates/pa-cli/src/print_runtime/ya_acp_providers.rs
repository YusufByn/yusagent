//! yusAgent (YA-10, ticket 40): the bridge's provider sign-in
//! (`pa_daemon::acp::daemon::ya_providers`) over the TUI's own flows
//! ([`crate::provider_login::ProviderAuth`]), unchanged: the same rows, the same key store, the
//! same browser and device flows, driven through a fresh auth panel whose requests become the
//! screen's `login_update` steps (`docs/tickets/40-login-extension.md`, §16.2, option P-B).
//!
//! Every row is offered, the Claude subscription too (ticket 40, §10 decision 5 cancelled on
//! 26/11/2026: Yusuf handles that sign-in himself). A failed flow answers a fixed text: the flows'
//! errors can carry the body of a token response (§15 decision 5, §13.1).
//! The flows open the sign-in page themselves, as in the TUI (decision 2). A cancel raises the
//! panel's flag, which the Claude and Codex flows read; `FlowCancel::mark` is `pa-tui`'s own, so
//! an MCP sign-in waiting on the browser stops at its next prompt or its end (limit).
//!
//! Listed in `docs/fork-changes.md`.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use pa_daemon::acp::daemon::ya_providers::{
    ProviderSignIn, ProviderView, SignInEvent, SignInEvents, SignInFuture, SignInOutcome,
    SignInTeam, TeamAnswer,
};
use pa_tui::auth_panel::{
    AuthPanelHandle, AuthPanelRequest, PastePromptTone, PasteStyle, PrimeTeamOption, PrimeTeamPick,
};
use pa_tui::provider_auth::{
    AuthFlow, AuthType, ProviderAuthCommands, ProviderAuthOutcome, ProviderRow,
};
use tokio::sync::oneshot;

use crate::mode::RuntimeConfig;
use crate::provider_login::ProviderAuth;

const SIGN_IN_FAILED: &str = "Sign-in failed. Try again.";
const KEY_NOT_SAVED: &str = "The key was not saved.";
const NOT_REMOVED: &str = "The credential was not removed.";
const UNKNOWN: &str = "This provider cannot be signed in to here.";

/// The flows the bridge serves, for the session's folder and agent dir.
pub(super) fn sign_in(config: &RuntimeConfig) -> Arc<dyn ProviderSignIn> {
    Arc::new(SignIn {
        auth: ProviderAuth::new(config.cwd.clone(), config.agent_dir.clone()),
        agent_dir: config.agent_dir.clone(),
    })
}

#[derive(Clone)]
struct SignIn {
    auth: ProviderAuth,
    agent_dir: PathBuf,
}

impl ProviderSignIn for SignIn {
    fn rows(&self) -> SignInFuture<Vec<ProviderView>> {
        let auth = self.auth.clone();
        Box::pin(async move {
            let login = auth.login_options().await;
            let logout = auth.logout_options().await;
            views(&login, &logout)
        })
    }

    fn set_api_key(&self, provider_id: String, api_key: String) -> SignInFuture<SignInOutcome> {
        let auth = self.auth.clone();
        Box::pin(async move {
            let row = auth
                .login_options()
                .await
                .into_iter()
                .find(|row| row.id == provider_id && row.flow == AuthFlow::ApiKeyPrompt);
            let Some(row) = row else {
                return SignInOutcome::Failed(UNKNOWN.to_string());
            };
            match auth.login(&row, Some(&api_key)).await {
                ProviderAuthOutcome::Error(_) => SignInOutcome::Failed(KEY_NOT_SAVED.to_string()),
                outcome => settled(outcome),
            }
        })
    }

    fn logout(&self, provider_id: String) -> SignInFuture<SignInOutcome> {
        let auth = self.auth.clone();
        Box::pin(async move {
            let row = auth
                .logout_options()
                .await
                .into_iter()
                .find(|row| row.id == provider_id);
            let Some(row) = row else {
                return SignInOutcome::Failed(UNKNOWN.to_string());
            };
            match auth.logout(&row).await {
                ProviderAuthOutcome::Error(_) => SignInOutcome::Failed(NOT_REMOVED.to_string()),
                outcome => settled(outcome),
            }
        })
    }

    fn login(&self, provider_id: String, events: SignInEvents) -> SignInFuture<SignInOutcome> {
        let sign_in = self.clone();
        Box::pin(async move { sign_in.run_login(&provider_id, &events).await })
    }
}

impl SignIn {
    /// The row a sign-in runs on: a browser or device flow of the login rows, or an MCP
    /// server's (`mcp:<server>`, as the TUI's `/mcp` view builds it).
    async fn login_row(&self, provider_id: &str) -> Option<ProviderRow> {
        if let Some(server) = provider_id.strip_prefix("mcp:") {
            return Some(ProviderRow {
                id: provider_id.to_string(),
                name: server.to_string(),
                auth_type: AuthType::Oauth,
                status: None,
                flow: AuthFlow::TerminalFlow,
                configured: false,
                available: true,
            });
        }
        self.auth
            .login_options()
            .await
            .into_iter()
            .find(|row| row.id == provider_id && row.flow == AuthFlow::TerminalFlow)
    }

    async fn run_login(&self, provider_id: &str, events: &SignInEvents) -> SignInOutcome {
        let Some(row) = self.login_row(provider_id).await else {
            return SignInOutcome::Failed(UNKNOWN.to_string());
        };
        let (panel_tx, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let panel = AuthPanelHandle::new(panel_tx);
        let cancel_flag = panel.cancel_flag();
        // The flow runs on a thread of its own, with its own runtime: a sign-in waiting on the
        // browser never holds the bridge's runtime when the bridge stops.
        let (done_tx, mut done) = oneshot::channel();
        let auth = self.auth.clone();
        std::thread::spawn(move || {
            let outcome = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_or_else(
                    |error| ProviderAuthOutcome::Error(error.to_string()),
                    |runtime| runtime.block_on(auth.login_on_panel(&row, panel)),
                );
            let _ = done_tx.send(outcome);
        });
        let mut cancelled = events.cancelled();
        let outcome = loop {
            tokio::select! {
                outcome = &mut done => break outcome.unwrap_or(ProviderAuthOutcome::Cancelled),
                Some(request) = requests.recv() => forward(request, events),
                Ok(()) = cancelled.changed() => {
                    if *cancelled.borrow() {
                        cancel_flag.store(true, Ordering::Relaxed);
                    }
                }
            }
        };
        if provider_id.starts_with("mcp:") {
            return mcp_settled(
                &outcome,
                cancel_flag.load(Ordering::Relaxed),
                stored(&self.agent_dir, provider_id),
            );
        }
        settled(outcome)
    }
}

/// An MCP sign-in's outcome: its flow answers its failures as a status too, so the store tells.
/// X5 (§22.11): a cancel the client asked for ends the flow on its own error (the cancelled
/// paste: `pa-core`'s `mcp/oauth.rs`, "Missing authorization code"), which answers `Cancelled`
/// when nothing was stored.
fn mcp_settled(
    outcome: &ProviderAuthOutcome,
    cancel_requested: bool,
    stored: bool,
) -> SignInOutcome {
    match outcome {
        ProviderAuthOutcome::Cancelled => SignInOutcome::Cancelled,
        _ if stored => SignInOutcome::Done,
        _ if cancel_requested => SignInOutcome::Cancelled,
        _ => SignInOutcome::Failed(SIGN_IN_FAILED.to_string()),
    }
}

/// Whether `auth.json` holds a credential for `provider_id`.
fn stored(agent_dir: &Path, provider_id: &str) -> bool {
    pa_core::auth::AuthStorage::create(agent_dir).has(provider_id)
}

/// One panel request as a step of the screen; the TUI's own settle requests are not steps.
fn forward(request: AuthPanelRequest, events: &SignInEvents) {
    let step = match request {
        AuthPanelRequest::Progress { message, .. } => SignInEvent::Progress(message),
        AuthPanelRequest::Waiting { message } => SignInEvent::Waiting(message),
        AuthPanelRequest::AuthUrl { url, instructions } => SignInEvent::Url { url, instructions },
        AuthPanelRequest::PastePrompt {
            prompt,
            tone,
            style,
            allow_empty,
            reply,
        } => SignInEvent::Prompt {
            message: prompt,
            secret: style == PasteStyle::Masked,
            allow_empty,
            optional: tone == PastePromptTone::Muted,
            reply,
        },
        AuthPanelRequest::SelectTeam {
            teams,
            current,
            reply,
        } => team_step(teams, current, reply),
        AuthPanelRequest::ProviderSettled { .. }
        | AuthPanelRequest::McpSettled { .. }
        | AuthPanelRequest::TracesSettled { .. } => return,
    };
    events.send(step);
}

/// The Prime team picker as a step: the screen's answer comes back as the panel's pick.
fn team_step(
    teams: Vec<PrimeTeamOption>,
    current: Option<String>,
    reply: oneshot::Sender<PrimeTeamPick>,
) -> SignInEvent {
    let (answer_tx, answer) = oneshot::channel();
    let shown = teams
        .iter()
        .map(|team| SignInTeam {
            id: team.team_id.clone(),
            name: team.name.clone(),
            role: team.role.clone(),
        })
        .collect();
    tokio::spawn(async move {
        let pick = match answer.await {
            Ok(TeamAnswer::Pick(None)) => PrimeTeamPick::PersonalAccount,
            Ok(TeamAnswer::Pick(Some(id))) => teams
                .into_iter()
                .find(|team| team.team_id == id)
                .map_or(PrimeTeamPick::Cancelled, PrimeTeamPick::Team),
            Ok(TeamAnswer::Keep) | Err(_) => PrimeTeamPick::Cancelled,
        };
        let _ = reply.send(pick);
    });
    SignInEvent::Team {
        teams: shown,
        current,
        reply: answer_tx,
    }
}

/// The screen's rows from the TUI's login rows (`login`) and stored-credential rows (`logout`):
/// removable when stored, the source's label as the note of a credential from elsewhere.
fn views(login: &[ProviderRow], logout: &[ProviderRow]) -> Vec<ProviderView> {
    login
        .iter()
        .map(|row| {
            let removable = logout.iter().any(|stored| stored.id == row.id);
            let note = if row.configured && !removable {
                row.status.as_ref().map(|status| status.label.clone())
            } else {
                None
            };
            ProviderView {
                id: row.id.clone(),
                name: row.name.clone(),
                subscription: row.auth_type == AuthType::Oauth,
                browser: row.flow == AuthFlow::TerminalFlow,
                connected: row.configured,
                removable,
                note,
            }
        })
        .collect()
}

/// A flow's outcome as the screen gets it: a failure is a fixed text, never the flow's own.
#[allow(clippy::needless_pass_by_value)] // the flow's outcome ends here
fn settled(outcome: ProviderAuthOutcome) -> SignInOutcome {
    match outcome {
        ProviderAuthOutcome::Status(_) => SignInOutcome::Done,
        ProviderAuthOutcome::Error(_) => SignInOutcome::Failed(SIGN_IN_FAILED.to_string()),
        ProviderAuthOutcome::Cancelled => SignInOutcome::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use pa_tui::provider_auth::{AuthFlow, AuthStatusIndicator, AuthStatusStyle, AuthType};

    use super::*;

    fn row(id: &str, auth_type: AuthType, flow: AuthFlow, configured: bool) -> ProviderRow {
        ProviderRow {
            id: id.to_string(),
            name: id.to_string(),
            auth_type,
            status: configured.then(|| AuthStatusIndicator {
                style: AuthStatusStyle::Success,
                label: "env: TEST_KEY".to_string(),
            }),
            flow,
            configured,
            available: true,
        }
    }

    /// Test 4: the rows the screen gets, the Claude subscription beside the Anthropic API key
    /// (§10 decision 5 cancelled on 26/11/2026).
    #[test]
    fn the_rows_offer_the_claude_subscription() {
        let login = [
            row("anthropic", AuthType::Oauth, AuthFlow::TerminalFlow, false),
            row(
                "openai-codex",
                AuthType::Oauth,
                AuthFlow::TerminalFlow,
                true,
            ),
            row("anthropic", AuthType::ApiKey, AuthFlow::ApiKeyPrompt, false),
            row("openai", AuthType::ApiKey, AuthFlow::ApiKeyPrompt, true),
        ];
        let logout = [row(
            "openai-codex",
            AuthType::Oauth,
            AuthFlow::TerminalFlow,
            true,
        )];
        let views = views(&login, &logout);
        let codex = views
            .iter()
            .find(|view| view.id == "openai-codex")
            .expect("the ChatGPT subscription row");
        assert!(codex.subscription && codex.browser && codex.connected && codex.removable);
        let claude = views
            .iter()
            .find(|view| view.id == "anthropic" && view.subscription)
            .unwrap_or_else(|| panic!("the Claude subscription row: {views:?}"));
        assert!(claude.browser && !claude.connected);
        let anthropic = views
            .iter()
            .find(|view| view.id == "anthropic" && !view.subscription)
            .expect("the Anthropic API key row stays");
        assert!(!anthropic.browser);
        let openai = views
            .iter()
            .find(|view| view.id == "openai")
            .expect("the OpenAI API key row");
        assert!(openai.connected && !openai.removable);
        assert_eq!(openai.note.as_deref(), Some("env: TEST_KEY"));
    }

    /// X5 (§22.11): an MCP sign-in the client cancelled answers `Cancelled`, even when its flow
    /// ended on its own error (a cancelled paste: `pa-core`'s `mcp/oauth.rs`, "Missing
    /// authorization code"); a stored credential still answers `Done`.
    #[test]
    fn a_cancelled_mcp_sign_in_answers_cancelled() {
        let flow_error = ProviderAuthOutcome::Status("Missing authorization code".to_string());
        assert_eq!(
            mcp_settled(&flow_error, true, false),
            SignInOutcome::Cancelled,
            "cancel asked, nothing stored"
        );
        assert_eq!(
            mcp_settled(&flow_error, false, false),
            SignInOutcome::Failed(SIGN_IN_FAILED.to_string()),
            "no cancel: a failure"
        );
        assert_eq!(
            mcp_settled(&flow_error, true, true),
            SignInOutcome::Done,
            "a stored credential wins"
        );
        assert_eq!(
            mcp_settled(&ProviderAuthOutcome::Cancelled, false, false),
            SignInOutcome::Cancelled,
            "the flow's own cancel"
        );
    }

    /// Test 8: a failed flow never hands the screen its own text, which can echo a token
    /// response (`pa-ai`'s `oauth/openai_codex.rs`, `missing fields: {json}`).
    #[test]
    fn a_failed_flow_answers_a_fixed_text() {
        let echoed = r#"OpenAI Codex token exchange response missing fields: {"access_token":"sk-ya40-echoed"}"#;
        match settled(ProviderAuthOutcome::Error(echoed.to_string())) {
            SignInOutcome::Failed(text) => {
                assert!(!text.contains("sk-ya40-echoed"), "the token leaked: {text}");
                assert!(!text.contains("missing fields"), "the flow's text: {text}");
            }
            other => panic!("a failed flow answers Failed: {other:?}"),
        }
        assert_eq!(
            settled(ProviderAuthOutcome::Cancelled),
            SignInOutcome::Cancelled
        );
    }
}
