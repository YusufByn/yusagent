//! yusAgent (YA-02, YA-10, ticket 42): the request auth of a live session, re-resolved before each
//! provider call (`docs/tickets/42-jeton-session-vivante.md`, option 2).
//!
//! The session's stream reads its key and headers from the provider-target slot, filled once at
//! the session build (`lifecycle.rs`): a token refreshed by another process, a key rewritten in
//! `auth.json`, or an `OAuth` access token that expires mid-conversation never reached the running
//! session (H1, ticket 40). Before each provider call, [`stream_fn`] re-resolves the slot's request
//! auth with the `/reload` rule (`reload.rs`): a credential the store serves replaces the pair, the
//! `models.json` fallback key never does, and a create-config key stays pinned.
//!
//! Push 1 (ticket 42, §G): the seam is wired, [`refresh_slot`] does nothing yet.
//!
//! Listed in `docs/fork-changes.md`.

use std::sync::Arc;

use pa_agent::stream::{LlmContext, StreamFn, StreamRequestOptions};
use pa_agent::types::Model as AgentModel;

use super::AgentSessionEngine;

/// The session's stream: `inner` (the slot-reading `switchable_stream_fn`), with the slot's
/// request auth re-resolved before each call. An engine never registered through
/// `register_arc` (the worker always registers its own) streams through `inner` unchanged.
pub(super) fn stream_fn(engine: &AgentSessionEngine, inner: StreamFn) -> StreamFn {
    let Some(engine) = engine
        .self_weak
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    else {
        return inner;
    };
    Arc::new(
        move |model: AgentModel, context: LlmContext, options: StreamRequestOptions| {
            if let Some(engine) = engine.upgrade() {
                refresh_slot(&engine);
            }
            inner(model, context, options)
        },
    )
}

/// Re-resolve the live slot's request auth from the credential store (push 1: nothing).
pub(super) fn refresh_slot(_engine: &AgentSessionEngine) {}

/// One scripted `OAuth` integration and the agent directory it serves.
#[cfg(test)]
type TestOAuth = (std::path::PathBuf, Arc<dyn pa_core::auth::OAuthIntegration>);

/// The `OAuth` integrations the tests script, by agent directory (the request-auth resolution
/// reads it from push 2 on).
#[cfg(test)]
static TEST_OAUTH: std::sync::Mutex<Vec<TestOAuth>> = std::sync::Mutex::new(Vec::new());

/// Refresh the `OAuth` credentials of `agent_dir` through `oauth` (a scripted transport) instead of
/// the providers' token endpoints.
#[cfg(test)]
pub(super) fn use_oauth_for_tests(
    agent_dir: &std::path::Path,
    oauth: Arc<dyn pa_core::auth::OAuthIntegration>,
) {
    let mut registry = TEST_OAUTH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    registry.retain(|(dir, _)| dir != agent_dir);
    registry.push((agent_dir.to_path_buf(), oauth));
}

#[cfg(test)]
#[path = "ya_live_auth_tests.rs"]
mod tests;
