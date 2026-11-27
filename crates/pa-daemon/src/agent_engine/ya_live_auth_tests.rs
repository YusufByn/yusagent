//! yusAgent (YA-02, YA-10, ticket 42): the request auth of a live session, re-resolved before each
//! provider call (`docs/tickets/42-jeton-session-vivante.md`, §G, tests 9 to 14).
//!
//! No network: the `ChatGPT` refresh runs `pa-ai`'s own code (`refresh_openai_codex_token`) over a
//! scripted token endpoint (`ProviderOAuth::with_http`), registered for the test's agent
//! directory. The slot is installed by hand, as `tests/reload.rs` does: no session is built, so
//! nothing resolves the request auth but the code under test.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pa_agent::stream::{LlmContext, StreamFn, StreamRequestOptions};
use pa_ai::oauth::{CodexHttp, CodexHttpResponse};
use pa_core::auth::ProviderOAuth;
use pa_core::session_engine::provider_adapter::ProviderTarget;
use serde_json::{json, Value};

use super::{refresh_slot, stream_fn, use_oauth_for_tests};
use crate::agent_engine::{AgentEngineConfig, AgentSessionEngine};

/// The access token the session was built with.
const OLD: &str = "tok-ya42-old";
/// The access token the endpoint answers: an unsigned JWT carrying the `ChatGPT` account id
/// (`acct-ya42`), the shape the refresh decodes (`pa-ai`'s `oauth/openai_codex.rs`,
/// `account_id_of`).
const FRESH: &str = "eyJhbGciOiJSUzI1NiJ9.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdC15YTQyIn19.not-a-signature";
/// The key a create config pins.
const PINNED: &str = "sk-ya42-pinned";
/// The cross-process test's child (test 13) reads its agent directory and exchange log here.
const CHILD_AGENT_DIR: &str = "YA42_CHILD_AGENT_DIR";
const CHILD_LOG: &str = "YA42_CHILD_LOG";

/// The `ChatGPT` token endpoint, scripted: answers `status` (the fresh token on 200) after
/// `delay`, and counts its exchanges in memory and, when `log` is set, as lines of that file.
struct TokenEndpoint {
    status: u16,
    delay: Duration,
    exchanges: Arc<AtomicUsize>,
    log: Option<PathBuf>,
}

impl CodexHttp for TokenEndpoint {
    fn post_form<'a>(
        &'a self,
        _url: &'a str,
        _body: &'a str,
        _timeout_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<CodexHttpResponse, String>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.exchanges.fetch_add(1, Ordering::SeqCst);
            if let Some(log) = &self.log {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log)
                    .map_err(|error| error.to_string())?;
                writeln!(file, "exchange by {}", std::process::id())
                    .map_err(|error| error.to_string())?;
            }
            tokio::time::sleep(self.delay).await;
            let body = if self.status == 200 {
                json!({ "access_token": FRESH, "refresh_token": "r-ya42-new", "expires_in": 3600 })
                    .to_string()
            } else {
                r#"{"error":"invalid_grant"}"#.to_string()
            };
            Ok(CodexHttpResponse {
                status: self.status,
                body,
            })
        })
    }
}

/// Script the token endpoint of `agent_dir`; the exchange counter comes back.
fn token_endpoint(
    agent_dir: &Path,
    status: u16,
    delay: Duration,
    log: Option<PathBuf>,
) -> Arc<AtomicUsize> {
    let exchanges = Arc::new(AtomicUsize::new(0));
    let endpoint = TokenEndpoint {
        status,
        delay,
        exchanges: Arc::clone(&exchanges),
        log,
    };
    use_oauth_for_tests(
        agent_dir,
        Arc::new(ProviderOAuth::with_http(Arc::new(endpoint))),
    );
    exchanges
}

/// A compiled `ChatGPT` subscription model.
fn chatgpt_model() -> pa_types::ai::Model {
    pa_ai::models_generated::get_models("openai-codex")
        .into_iter()
        .next()
        .cloned()
        .expect("a compiled ChatGPT model")
}

/// `auth.json` with an expired `ChatGPT` credential whose access token is `access`.
fn write_expired_chatgpt(agent_dir: &Path, access: &str) {
    std::fs::create_dir_all(agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("auth.json"),
        json!({
            "openai-codex": {
                "type": "oauth",
                "access": access,
                "refresh": "r-ya42-old",
                "expires": 1,
                "accountId": "acct-ya42",
            }
        })
        .to_string(),
    )
    .unwrap();
}

/// The stored `ChatGPT` access token.
fn stored_access(agent_dir: &Path) -> Value {
    let text = std::fs::read_to_string(agent_dir.join("auth.json")).unwrap();
    serde_json::from_str::<Value>(&text).unwrap()["openai-codex"]["access"].clone()
}

/// An engine over `dir/agent`, registered like the worker registers its own (`worker.rs`),
/// with the slot set to the `ChatGPT` model and `key`, as a session build leaves it.
fn engine_serving(dir: &Path, pinned: Option<&str>, key: &str) -> Arc<AgentSessionEngine> {
    let engine = Arc::new(unregistered_engine(dir, pinned));
    engine.register_arc();
    install(&engine, key);
    engine
}

fn unregistered_engine(dir: &Path, pinned: Option<&str>) -> AgentSessionEngine {
    AgentSessionEngine::new(AgentEngineConfig {
        cwd: dir.to_path_buf(),
        agent_dir: dir.join("agent"),
        provider: None,
        model: None,
        api_key: pinned.map(str::to_string),
        thinking: None,
        session_dir: None,
        session_file: None,
        faux_script: None,
        supervisor_link: None,
        telemetry_disabled: Some(true),
        cron_store: None,
        queued_steering_probe: None,
    })
    .expect("engine")
}

fn install(engine: &AgentSessionEngine, key: &str) {
    *engine
        .provider_target
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ProviderTarget {
        api_key: Some(key.to_string()),
        model: chatgpt_model(),
        service_tier: None,
        headers: None,
    });
}

/// The key the next provider call reads from the slot.
fn slot_key(engine: &AgentSessionEngine) -> Option<String> {
    engine
        .provider_target
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .and_then(|target| target.api_key)
}

/// Test 9: an expired `ChatGPT` access token is refreshed before the next call: the slot serves
/// the fresh token, `auth.json` stores it, and the endpoint saw one exchange.
#[test]
fn an_expired_chatgpt_token_is_refreshed_before_the_call() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_expired_chatgpt(&agent_dir, OLD);
    let exchanges = token_endpoint(&agent_dir, 200, Duration::ZERO, None);
    let engine = engine_serving(dir.path(), None, OLD);
    refresh_slot(&engine);
    assert_eq!(
        slot_key(&engine).as_deref(),
        Some(FRESH),
        "the next call serves the refreshed token; exchanges: {}",
        exchanges.load(Ordering::SeqCst)
    );
    assert_eq!(exchanges.load(Ordering::SeqCst), 1, "one token exchange");
    assert_eq!(
        stored_access(&agent_dir),
        json!(FRESH),
        "auth.json stores the refreshed token"
    );
}

/// Test 10: a failed refresh leaves the slot's pair (no fallback key in its place, the `/reload`
/// rule); the next call fails on the provider, as today.
#[test]
fn a_failed_refresh_keeps_the_pair() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_expired_chatgpt(&agent_dir, OLD);
    let _exchanges = token_endpoint(&agent_dir, 400, Duration::ZERO, None);
    let engine = engine_serving(dir.path(), None, OLD);
    refresh_slot(&engine);
    assert_eq!(slot_key(&engine).as_deref(), Some(OLD));
    assert_eq!(stored_access(&agent_dir), json!(OLD));
}

/// Test 11: a key the create config pins is never re-resolved: no exchange, the pinned key serves.
#[test]
fn a_pinned_key_is_never_re_resolved() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_expired_chatgpt(&agent_dir, OLD);
    let exchanges = token_endpoint(&agent_dir, 200, Duration::ZERO, None);
    let engine = engine_serving(dir.path(), Some(PINNED), PINNED);
    refresh_slot(&engine);
    assert_eq!(slot_key(&engine).as_deref(), Some(PINNED));
    assert_eq!(exchanges.load(Ordering::SeqCst), 0, "no token exchange");
}

/// Test 12: a Stop during a refresh settles the call at once, and no provider request leaves.
#[test]
fn a_stop_during_a_refresh_settles_the_call_at_once() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_expired_chatgpt(&agent_dir, OLD);
    let _exchanges = token_endpoint(&agent_dir, 200, Duration::from_secs(5), None);
    let engine = engine_serving(dir.path(), None, OLD);
    let requests = Arc::new(AtomicUsize::new(0));
    let inner: StreamFn = {
        let requests = Arc::clone(&requests);
        Arc::new(move |_model, _context, _options| {
            requests.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Err(anyhow::anyhow!("the provider request")) })
        })
    };
    let stream = stream_fn(&engine, inner);
    let controller = pa_agent::abort::AbortController::new();
    let options = StreamRequestOptions {
        signal: controller.signal(),
        ..StreamRequestOptions::default()
    };
    let model: pa_agent::types::Model = serde_json::from_value(json!({
        "id": "ya42",
        "api": "openai-codex-responses",
        "provider": "openai-codex",
    }))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let started = Instant::now();
    let failed = runtime.block_on(async move {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            controller.abort();
        });
        stream(model, LlmContext::default(), options).await.is_err()
    });
    let elapsed = started.elapsed();
    runtime.shutdown_background();
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "no provider request after the Stop"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "the call settles at the Stop, not after the refresh: {elapsed:?}"
    );
    assert!(failed, "the stopped call fails");
}

/// Test 13 (child): one worker's refresh, in its own process (`CHILD_AGENT_DIR`, `CHILD_LOG`);
/// passes untouched outside the cross-process test.
#[test]
fn a_worker_process_refreshes() {
    let (Some(agent_dir), Some(log)) = (
        std::env::var_os(CHILD_AGENT_DIR),
        std::env::var_os(CHILD_LOG),
    ) else {
        return;
    };
    let agent_dir = PathBuf::from(agent_dir);
    let dir = agent_dir
        .parent()
        .expect("the agent dir's parent")
        .to_path_buf();
    let _exchanges = token_endpoint(
        &agent_dir,
        200,
        Duration::from_millis(500),
        Some(PathBuf::from(log)),
    );
    let engine = engine_serving(&dir, None, OLD);
    refresh_slot(&engine);
    assert_eq!(slot_key(&engine).as_deref(), Some(FRESH));
}

/// Test 13: two workers (processes) refresh the same expired token at once: one exchange
/// between them, and both serve the stored token.
#[cfg(unix)]
#[test]
fn two_workers_spend_the_refresh_token_once() {
    let dir = tempfile::TempDir::new().unwrap();
    let agent_dir = dir.path().join("agent");
    write_expired_chatgpt(&agent_dir, OLD);
    let log = dir.path().join("exchanges.log");
    let child_test = format!(
        "{}::a_worker_process_refreshes",
        module_path!()
            .split_once("::")
            .map_or(module_path!(), |(_, path)| path)
    );
    let children: Vec<std::process::Child> = (0..2)
        .map(|_| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([child_test.as_str(), "--exact", "--nocapture"])
                .env(CHILD_AGENT_DIR, &agent_dir)
                .env(CHILD_LOG, &log)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .expect("spawn a worker process")
        })
        .collect();
    let codes: Vec<Option<i32>> = children
        .into_iter()
        .map(|mut child| child.wait().expect("the worker process exits").code())
        .collect();
    let exchanges = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .count();
    assert_eq!(
        exchanges, 1,
        "one token exchange between the two workers; exit codes: {codes:?}"
    );
    assert_eq!(codes, [Some(0), Some(0)], "both workers serve the token");
    assert_eq!(stored_access(&agent_dir), json!(FRESH));
}

/// Test 14: an engine never registered streams through the inner stream function itself.
#[test]
fn an_unregistered_engine_streams_as_before() {
    let dir = tempfile::TempDir::new().unwrap();
    let engine = unregistered_engine(dir.path(), None);
    let inner: StreamFn = Arc::new(|_model, _context, _options| {
        Box::pin(async move { Err(anyhow::anyhow!("not called")) })
    });
    assert!(Arc::ptr_eq(&stream_fn(&engine, Arc::clone(&inner)), &inner));
}
