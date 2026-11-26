// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines).
#![allow(clippy::large_futures, clippy::too_many_lines)]

//! yusAgent (YA-11, ticket 41, PR B): the credentials stay in the worker.
//!
//! The kernel no longer reads `auth.json` nor holds a token: its HTTP requests to an MCP server
//! go through the worker (`mcp.http`), which adds the credential bound to the configured
//! endpoint, and a web search goes through `websearch.search`, which adds the Serper key.
//! - test 4: `mcp.http` adds the bound token, refuses another URL and refuses the GET stream;
//!   no response carries the token;
//! - test 5 (part): `mcp.config` never hands the kernel a literal header;
//! - websearch: `websearch.search` adds the stored key and returns no secret;
//! - `SERPER_API_KEY` never reaches the kernel, even named in `kernel.envAllowlist`.
//!
//! The servers are local fakes (`127.0.0.1`), the secrets are sentinels: no real model, no
//! real service.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pa_core::auth::{parse_storage_data, AuthStorage, NoOAuth};
use pa_core::kernel::shared::{HostRequestHandlers, HostRequestPayload};
use pa_core::mcp::{AcpMcpServerConfig, McpManager, McpManagerOptions, McpServerConfig};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SENTINEL: &str = "ya41-sentinel-token-7f3a";
const ACP_SENTINEL: &str = "ya41-sentinel-acp-header-2c9d";
const USER_SENTINEL: &str = "ya41-sentinel-user-header-5e1b";
const SERPER_SENTINEL: &str = "ya41-sentinel-serper-key-8a4c";

/// The requests one fake server received (raw head and body, as text).
type Seen = Arc<Mutex<Vec<String>>>;

/// A local HTTP/1.1 server answering every request with `body`, recording each request.
async fn fake_server(path: &str, content_type: &'static str, body: &'static str) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let request = read_request(&mut socket).await;
                log.lock().unwrap().push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}{path}"), seen)
}

/// One request: the head, then as many body bytes as `content-length` says.
async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let Ok(read) = socket.read(&mut chunk).await else {
            break;
        };
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&buffer).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if buffer.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buffer).to_string()
}

fn http_server(
    url: &str,
    oauth: Option<bool>,
    headers: Option<HashMap<String, String>>,
) -> McpServerConfig {
    McpServerConfig::Http {
        url: url.to_string(),
        headers,
        bearer_token_env_var: None,
        oauth,
        enabled: None,
        enabled_tools: None,
        disabled_tools: None,
        startup_timeout_ms: None,
        call_timeout_ms: None,
        client_id: None,
        callback_port: None,
        client_secret: None,
        scopes: None,
    }
}

/// The MCP host handlers of a manager whose auth store holds `auth` (`auth.json` shape).
async fn handlers_for(
    user_servers: HashMap<String, McpServerConfig>,
    auth: &Value,
) -> (HostRequestHandlers, Arc<Mutex<McpManager>>) {
    let data = parse_storage_data(Some(&auth.to_string())).unwrap();
    let manager = Arc::new(Mutex::new(McpManager::new(McpManagerOptions {
        auth_storage: AuthStorage::in_memory(&data, Arc::new(NoOAuth)),
        get_user_servers: Box::new(move || Some(user_servers.clone())),
        begin_login: None,
        agent_dir: None,
        get_catalog_sources: None,
        remote_source: None,
        probe_override: None,
    })));
    // The same warm-up as the session: the integrations exist before the kernel asks.
    let warm = Arc::clone(&manager);
    tokio::task::spawn_blocking(move || {
        warm.lock()
            .unwrap()
            .get_enabled_persistent_generic_servers()
    })
    .await
    .unwrap();
    let mut handlers = HostRequestHandlers::default();
    McpManager::register_host_handlers(&manager, &mut handlers);
    (handlers, manager)
}

/// A credential bound to `endpoint`, valid for decades.
fn bound_oauth(endpoint: &str) -> Value {
    json!({
        "type": "oauth",
        "access": SENTINEL,
        "refresh": "ya41-refresh",
        "expires": 4_102_444_800_000_u64,
        "endpoint": endpoint,
    })
}

async fn call(handlers: &HostRequestHandlers, request: &str, data: Value) -> anyhow::Result<Value> {
    let handler = handlers.get(request).unwrap().clone();
    handler(HostRequestPayload { data }).await
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18"}}"#;

#[tokio::test]
async fn mcp_http_relay_adds_the_bound_token_and_returns_no_secret() {
    let (url, seen) = fake_server("/mcp", "application/json", INITIALIZED).await;
    let servers = HashMap::from([("svc".to_string(), http_server(&url, Some(true), None))]);
    let (handlers, _manager) =
        handlers_for(servers, &json!({ "mcp:svc": bound_oauth(&url) })).await;
    assert!(
        handlers.get("mcp.http").is_some(),
        "relay mcp.http absent: the kernel still calls the MCP server itself, with the token"
    );
    let response = call(
        &handlers,
        "mcp.http",
        json!({
            "server": "svc",
            "method": "POST",
            "url": url,
            "headers": {
                "content-type": "application/json",
                "accept": "application/json, text/event-stream",
            },
            "body": INITIALIZE,
        }),
    )
    .await
    .unwrap();
    assert_eq!(response["status"], 200);
    assert_eq!(response["body"], INITIALIZED);
    let received = seen.lock().unwrap().clone();
    assert_eq!(received.len(), 1, "one request reaches the server");
    let head = received[0].to_ascii_lowercase();
    assert!(
        head.contains(&format!("authorization: bearer {SENTINEL}")),
        "the worker adds the bound token: {head}"
    );
    assert!(
        head.contains(r#""method":"initialize""#),
        "the body is forwarded as is"
    );
    assert!(
        !response.to_string().contains(SENTINEL),
        "the relay hands no token back to the kernel"
    );
}

#[tokio::test]
async fn mcp_http_relay_refuses_a_url_other_than_the_configured_one() {
    let (url, seen) = fake_server("/mcp", "application/json", INITIALIZED).await;
    let (other, other_seen) = fake_server("/elsewhere", "application/json", INITIALIZED).await;
    let servers = HashMap::from([("svc".to_string(), http_server(&url, Some(true), None))]);
    let (handlers, _manager) =
        handlers_for(servers, &json!({ "mcp:svc": bound_oauth(&url) })).await;
    assert!(handlers.get("mcp.http").is_some(), "relay mcp.http absent");
    let refused = call(
        &handlers,
        "mcp.http",
        json!({
            "server": "svc",
            "method": "POST",
            "url": other,
            "headers": { "content-type": "application/json" },
            "body": INITIALIZE,
        }),
    )
    .await;
    assert!(
        refused.is_err(),
        "a URL other than the configured endpoint is refused"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing reaches the configured server"
    );
    assert!(
        other_seen.lock().unwrap().is_empty(),
        "nothing reaches the other URL"
    );
}

#[tokio::test]
async fn mcp_http_relay_answers_the_get_stream_with_405_without_calling_the_server() {
    let (url, seen) = fake_server("/mcp", "text/event-stream", "").await;
    let servers = HashMap::from([("svc".to_string(), http_server(&url, Some(true), None))]);
    let (handlers, _manager) =
        handlers_for(servers, &json!({ "mcp:svc": bound_oauth(&url) })).await;
    assert!(handlers.get("mcp.http").is_some(), "relay mcp.http absent");
    let response = call(
        &handlers,
        "mcp.http",
        json!({
            "server": "svc",
            "method": "GET",
            "url": url,
            "headers": { "accept": "text/event-stream" },
            "body": "",
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        response["status"], 405,
        "complete responses only: no GET stream"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the GET never reaches the server"
    );
}

#[tokio::test]
async fn mcp_config_never_hands_the_kernel_a_literal_header() {
    let (url, _seen) = fake_server("/mcp", "application/json", INITIALIZED).await;
    let servers = HashMap::from([(
        "user-svc".to_string(),
        http_server(
            &url,
            None,
            Some(HashMap::from([(
                "X-Api-Key".to_string(),
                USER_SENTINEL.to_string(),
            )])),
        ),
    )]);
    let (handlers, manager) = handlers_for(servers, &json!({})).await;
    manager
        .lock()
        .unwrap()
        .replace_acp_servers(
            &[AcpMcpServerConfig::Http {
                name: "acp-svc".to_string(),
                url: url.clone(),
                headers: HashMap::from([(
                    "Authorization".to_string(),
                    format!("Bearer {ACP_SENTINEL}"),
                )]),
            }],
            "client-a",
        )
        .unwrap();
    let user = call(&handlers, "mcp.config", json!({ "server": "user-svc" }))
        .await
        .unwrap();
    let acp = call(&handlers, "mcp.config", json!({ "server": "acp-svc" }))
        .await
        .unwrap();
    assert_eq!(
        user["url"], url,
        "the kernel still gets the user server's URL"
    );
    assert_eq!(
        acp["url"], url,
        "the kernel still gets the ACP server's URL"
    );
    assert!(
        !user.to_string().contains(USER_SENTINEL),
        "mcp.config hands the kernel a user header: {user}"
    );
    assert!(
        !acp.to_string().contains(ACP_SENTINEL),
        "mcp.config hands the kernel an ACP header: {acp}"
    );
}

const SERPER_RESULT: &str =
    r#"{"organic":[{"title":"ya41 result","link":"https://example.invalid/","snippet":"ok"}]}"#;

#[tokio::test]
async fn websearch_relay_adds_the_stored_key_and_returns_no_secret() {
    let (url, seen) = fake_server("/search", "application/json", SERPER_RESULT).await;
    // The relay's fixed endpoint, redirected for this test only (the worker's environment).
    std::env::set_var("PRIME_AGENT_TEST_SERPER_URL", &url);
    let auth = json!({ "serper": { "type": "api_key", "key": SERPER_SENTINEL } });
    let (handlers, _manager) = handlers_for(HashMap::new(), &auth).await;
    assert!(
        handlers.get("websearch.search").is_some(),
        "relay websearch.search absent: the kernel still reads the Serper key itself"
    );
    let response = call(
        &handlers,
        "websearch.search",
        json!({ "query": "ya41", "num": 3 }),
    )
    .await
    .unwrap();
    assert_eq!(response["organic"][0]["title"], "ya41 result");
    let received = seen.lock().unwrap().clone();
    assert_eq!(received.len(), 1, "one search reaches Serper");
    let head = received[0].to_ascii_lowercase();
    assert!(
        head.contains(&format!("x-api-key: {SERPER_SENTINEL}")),
        "the worker adds the stored key: {head}"
    );
    assert!(head.contains(r#""q":"ya41""#), "the query is forwarded");
    assert!(
        !response.to_string().contains(SERPER_SENTINEL),
        "the relay hands no key back to the kernel"
    );
}

#[test]
fn serper_api_key_never_reaches_the_kernel_even_when_named() {
    use pa_core::kernel::env_allowlist::{
        apply_user_allowlist, is_harness_credential, strip_harness_credentials,
    };
    assert!(
        is_harness_credential("SERPER_API_KEY"),
        "SERPER_API_KEY is a harness credential (ticket 41, answer 13)"
    );
    let mut env = HashMap::from([("SERPER_API_KEY".to_string(), SERPER_SENTINEL.to_string())]);
    apply_user_allowlist(
        &mut env,
        &["SERPER_API_KEY".to_string()],
        [("SERPER_API_KEY".to_string(), SERPER_SENTINEL.to_string())],
    );
    strip_harness_credentials(&mut env);
    assert!(
        !env.contains_key("SERPER_API_KEY"),
        "SERPER_API_KEY reached the kernel environment"
    );
}
