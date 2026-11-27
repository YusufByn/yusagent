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
//! - The common case (nothing to refresh) re-resolves synchronously, with no await: a Stop cannot
//!   drop the call mid-resolution (§I.4).
//! - A stored `OAuth` token of a refreshable provider, expired or expiring within
//!   [`REFRESH_MARGIN_MS`], refreshes off the executor, raced with the Stop, under a cross-process
//!   lock (`auth.json.ya-refresh.<provider>.lock`, refused to the kernel by the secret guard's
//!   `auth.json` prefix): another worker waiting on it finds the written token and spends no
//!   refresh token. A Stop lets the refresh finish (§J.1, limit).
//! - The refresh goes through [`auth_storage`] (`model.rs` seam): the providers' own refresh in
//!   production, a scripted token endpoint in tests.
//!
//! Listed in `docs/fork-changes.md`.

use std::path::Path;
use std::sync::{Arc, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use pa_agent::abort::race_with_abort;
use pa_agent::stream::{LlmContext, ModelStream, StreamFn, StreamRequestOptions};
use pa_agent::types::Model as AgentModel;
use pa_agent::BoxFut;
use pa_core::auth::{
    AuthCredential, AuthStorage, NoOAuth, OAuthIntegration, ProviderOAuth, ANTHROPIC_PROVIDER_ID,
    GITHUB_COPILOT_PROVIDER_ID, OPENAI_CODEX_PROVIDER_ID, XAI_PROVIDER_ID,
};
use pa_types::ai::Model;

use super::AgentSessionEngine;

/// The providers whose stored `OAuth` tokens refresh (`pa-core`'s `auth/provider_oauth.rs`).
const REFRESHABLE: [&str; 4] = [
    OPENAI_CODEX_PROVIDER_ID,
    ANTHROPIC_PROVIDER_ID,
    GITHUB_COPILOT_PROVIDER_ID,
    XAI_PROVIDER_ID,
];
/// A token this close to its expiry takes the refresh path (lock, off the executor): the store
/// refreshes it only once expired, and the lock is then already held.
const REFRESH_MARGIN_MS: i64 = 30_000;
/// How long a worker waits for another one's refresh before giving up (the slot then keeps its
/// pair, as without this module).
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(50);

/// The session's stream: `inner` (the slot-reading `switchable_stream_fn`), with the slot's
/// request auth re-resolved before each call. An engine never registered through
/// `register_arc` (the worker always registers its own) streams through `inner` unchanged.
pub(super) fn stream_fn(engine: &AgentSessionEngine, inner: StreamFn) -> StreamFn {
    let Some(engine) = engine
        .self_weak
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    else {
        return inner;
    };
    Arc::new(
        move |model: AgentModel,
              context: LlmContext,
              options: StreamRequestOptions|
              -> BoxFut<'static, anyhow::Result<Box<dyn ModelStream>>> {
            let Some(live) = engine.upgrade() else {
                return inner(model, context, options);
            };
            // Nothing to refresh and no reload running: a synchronous re-read, no await.
            if refresh_provider(&live).is_none() && resolve_into_slot(&live, false) {
                return inner(model, context, options);
            }
            let inner = Arc::clone(&inner);
            Box::pin(async move {
                let signal = options.signal.clone();
                let refresh = tokio::task::spawn_blocking(move || refresh_slot(&live));
                // A Stop settles the call now; the refresh runs to its end. A panicked refresh
                // leaves the slot as it was.
                let _ = race_with_abort(refresh, &signal).await?;
                inner(model, context, options).await
            })
        },
    )
}

/// Re-resolve the live slot's request auth from the credential store, refreshing an expired
/// `OAuth` token under the cross-process lock (blocking: off the executor).
pub(super) fn refresh_slot(engine: &AgentSessionEngine) {
    let _refresh_lock = match refresh_provider(engine) {
        Some(provider) => match refresh_lock(&engine.config.agent_dir, &provider) {
            Ok(lock) => lock,
            // Another worker held the lock too long: nothing is resolved.
            Err(LockTimedOut) => return,
        },
        None => None,
    };
    resolve_into_slot(engine, true);
}

/// The store the request auth resolves from (`model.rs`, `resolve_request_key_and_headers_and_store_health`):
/// `AuthStorage::create`, with the test's scripted `OAuth` integration under `cfg(test)`.
pub(super) fn auth_storage(agent_dir: &Path) -> AuthStorage {
    AuthStorage::create_with_oauth(agent_dir, oauth_for(agent_dir))
}

/// The `OAuth` integration of `AuthStorage::create`.
#[cfg(not(test))]
fn oauth_for(_agent_dir: &Path) -> Arc<dyn OAuthIntegration> {
    Arc::new(ProviderOAuth::new())
}

/// The scripted integration registered for `agent_dir`, else the one of `AuthStorage::create`.
#[cfg(test)]
fn oauth_for(agent_dir: &Path) -> Arc<dyn OAuthIntegration> {
    TEST_OAUTH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(dir, _)| dir == agent_dir)
        .map_or_else(
            || Arc::new(ProviderOAuth::new()) as Arc<dyn OAuthIntegration>,
            |(_, oauth)| Arc::clone(oauth),
        )
}

/// The model the next call serves (the slot's).
fn slot_model(engine: &AgentSessionEngine) -> Option<Model> {
    engine
        .provider_target
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|target| target.model.clone())
}

/// The slot's provider when its stored credential is an `OAuth` token of a refreshable provider,
/// expired or expiring within [`REFRESH_MARGIN_MS`]; `None` otherwise, and with a pinned key.
/// Reads `auth.json` without refreshing anything.
fn refresh_provider(engine: &AgentSessionEngine) -> Option<String> {
    if engine.current_selection().api_key.is_some() {
        return None;
    }
    let provider = slot_model(engine)?.provider;
    let store = AuthStorage::create_with_oauth(&engine.config.agent_dir, Arc::new(NoOAuth));
    let Some(AuthCredential::Oauth { expires, .. }) = store.get_all().credential(&provider) else {
        return None;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(i64::MAX, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        });
    (REFRESHABLE.contains(&provider.as_str())
        && expires.saturating_sub(now_ms) <= REFRESH_MARGIN_MS)
        .then_some(provider)
}

/// Another worker held the refresh lock past [`LOCK_WAIT`].
struct LockTimedOut;

/// The cross-process refresh lock of `provider`, held while the file stays open (released at its
/// close or the process's death). A lock file that cannot be opened or locked (a file system
/// without locks) gives `Ok(None)`: the refresh then runs as without this module.
fn refresh_lock(agent_dir: &Path, provider: &str) -> Result<Option<std::fs::File>, LockTimedOut> {
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(agent_dir.join(format!("auth.json.ya-refresh.{provider}.lock")))
    else {
        return Ok(None);
    };
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::Error(_)) => return Ok(None),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() >= deadline => {
                return Err(LockTimedOut);
            }
            Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
        }
    }
}

/// The `/reload` rule on the live slot (`reload.rs`): a credential the store serves replaces the
/// pair, headers included, if the slot still serves `model`; otherwise the slot keeps its pair.
/// `wait`: block on a running reload or refresh (off the executor), or give up at once; `false`
/// only when it gave up (the caller then waits off the executor).
fn resolve_into_slot(engine: &AgentSessionEngine, wait: bool) -> bool {
    if engine.current_selection().api_key.is_some() {
        return true;
    }
    let Some(model) = slot_model(engine) else {
        return true;
    };
    let _serialized = if wait {
        engine
            .reload_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    } else {
        match engine.reload_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(poison)) => poison.into_inner(),
            // A reload or a refresh holds the slot right now.
            Err(TryLockError::WouldBlock) => return false,
        }
    };
    let (api_key, headers, store_auth) =
        engine.resolve_request_key_and_headers_and_store_health(&model);
    let Some(api_key) = api_key.filter(|_| store_auth) else {
        return true;
    };
    let mut slot = engine
        .provider_target
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(target) = slot.as_mut().filter(|target| target.model == model) {
        target.api_key = Some(api_key);
        target.headers = headers;
    }
    true
}

/// One scripted `OAuth` integration and the agent directory it serves.
#[cfg(test)]
type TestOAuth = (std::path::PathBuf, Arc<dyn OAuthIntegration>);

/// The `OAuth` integrations the tests script, by agent directory ([`oauth_for`]).
#[cfg(test)]
static TEST_OAUTH: std::sync::Mutex<Vec<TestOAuth>> = std::sync::Mutex::new(Vec::new());

/// Refresh the `OAuth` credentials of `agent_dir` through `oauth` (a scripted transport) instead of
/// the providers' token endpoints.
#[cfg(test)]
pub(super) fn use_oauth_for_tests(agent_dir: &Path, oauth: Arc<dyn OAuthIntegration>) {
    let mut registry = TEST_OAUTH.lock().unwrap_or_else(PoisonError::into_inner);
    registry.retain(|(dir, _)| dir != agent_dir);
    registry.push((agent_dir.to_path_buf(), oauth));
}

#[cfg(test)]
#[path = "ya_live_auth_tests.rs"]
mod tests;
