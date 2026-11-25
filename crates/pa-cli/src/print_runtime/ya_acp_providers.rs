//! yusAgent (YA-10, ticket 40): the bridge's provider sign-in
//! (`pa_daemon::acp::daemon::ya_providers`) over the TUI's own flows
//! ([`crate::provider_login::ProviderAuth`]), unchanged: the same rows, the same key store, the
//! same browser and device flows, driven through a fresh auth panel whose requests become the
//! screen's `login_update` steps (`docs/tickets/40-login-extension.md`, §16.2, option P-B).
//!
//! The Claude subscription row is not offered (decision 5, rule 7). A failed flow answers a
//! fixed text: the flows' errors can carry the body of a token response (decision 5, §13.1).
//!
//! Push 1 of R5: neutral bodies; the tests fail on their assertions.
//! Listed in `docs/fork-changes.md`.

// Push 1 of R5 (tests and skeleton): the bodies are neutral, removed with the code.
#![allow(dead_code, unused_variables)]

use std::path::PathBuf;
use std::sync::Arc;

use pa_daemon::acp::daemon::ya_providers::{
    ProviderSignIn, ProviderView, SignInEvents, SignInFuture, SignInOutcome,
};
use pa_tui::provider_auth::{ProviderAuthOutcome, ProviderRow};

use crate::mode::RuntimeConfig;
use crate::provider_login::ProviderAuth;

/// The flows the bridge serves, for the session's folder and agent dir.
pub(super) fn sign_in(config: &RuntimeConfig) -> Arc<dyn ProviderSignIn> {
    Arc::new(SignIn {
        auth: ProviderAuth::new(config.cwd.clone(), config.agent_dir.clone()),
        agent_dir: config.agent_dir.clone(),
    })
}

struct SignIn {
    auth: ProviderAuth,
    agent_dir: PathBuf,
}

impl ProviderSignIn for SignIn {
    fn rows(&self) -> SignInFuture<Vec<ProviderView>> {
        Box::pin(async { Vec::new() })
    }

    fn set_api_key(&self, provider_id: String, api_key: String) -> SignInFuture<SignInOutcome> {
        Box::pin(async { SignInOutcome::Cancelled })
    }

    fn logout(&self, provider_id: String) -> SignInFuture<SignInOutcome> {
        Box::pin(async { SignInOutcome::Cancelled })
    }

    fn login(&self, provider_id: String, events: SignInEvents) -> SignInFuture<SignInOutcome> {
        Box::pin(async { SignInOutcome::Cancelled })
    }
}

/// The screen's rows from the TUI's login rows (`login`) and stored-credential rows (`logout`).
fn views(login: &[ProviderRow], logout: &[ProviderRow]) -> Vec<ProviderView> {
    Vec::new()
}

/// A flow's outcome as the screen gets it.
fn settled(outcome: ProviderAuthOutcome) -> SignInOutcome {
    match outcome {
        ProviderAuthOutcome::Status(_) => SignInOutcome::Done,
        ProviderAuthOutcome::Error(error) => SignInOutcome::Failed(error),
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

    /// Test 4: the rows the screen gets; no Claude subscription row (decision 5).
    #[test]
    fn the_rows_leave_out_the_claude_subscription() {
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
        assert!(
            !views
                .iter()
                .any(|view| view.id == "anthropic" && view.subscription),
            "no Claude subscription row: {views:?}"
        );
        let anthropic = views
            .iter()
            .find(|view| view.id == "anthropic")
            .expect("the Anthropic API key row stays");
        assert!(!anthropic.subscription && !anthropic.browser);
        let openai = views
            .iter()
            .find(|view| view.id == "openai")
            .expect("the OpenAI API key row");
        assert!(openai.connected && !openai.removable);
        assert_eq!(openai.note.as_deref(), Some("env: TEST_KEY"));
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
