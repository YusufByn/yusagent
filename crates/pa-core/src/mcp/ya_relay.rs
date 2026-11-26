//! yusAgent (YA-11, ticket 41, PR B): the kernel's MCP HTTP requests and web searches go
//! through the worker, which adds the credentials; the kernel never reads `auth.json` nor
//! holds a token.
//!
//! - `mcp.http`: one complete HTTP exchange with an MCP server. The URL must be the server's
//!   configured endpoint, or, for a server outside the settings (a Python `McpIntegration`),
//!   the endpoint its stored credential is bound to. The worker adds the configured headers and
//!   the bound token; the GET stream (server-initiated messages) is answered 405 without a call.
//! - `websearch.search`: one Serper search with the worker's key (`SERPER_API_KEY` of the
//!   worker, else the stored `serper` credential).
//! - [`strip_headers`]: `mcp.config` hands the kernel no literal header; `relay: true` tells the
//!   kernel to send that server's requests through `mcp.http`.
//!
//! Listed in `docs/fork-changes.md`.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{provider_id, AcpMcpServerConfig, McpManager, McpServerConfig};
use crate::auth::{AuthCredential, AuthStorage, SERPER_CREDENTIAL_ID};
use crate::kernel::shared::{host_handler, HostRequestHandlers};

/// Serper's search endpoint (the skill's fixed URL).
const SERPER_URL: &str = "https://google.serper.dev/search";
/// Test-only override of [`SERPER_URL`], read from the worker's environment.
const SERPER_URL_TEST_ENV: &str = "PRIME_AGENT_TEST_SERPER_URL";

/// Request headers never forwarded from the kernel: the worker sets the credential itself, and
/// hop-by-hop or encoding headers belong to each connection (the relay returns an identity body).
const DROPPED_REQUEST_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "host",
    "content-length",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "upgrade",
    "accept-encoding",
];

/// Response headers that describe the worker's connection, not the body handed back.
const DROPPED_RESPONSE_HEADERS: &[&str] = &[
    "content-length",
    "transfer-encoding",
    "content-encoding",
    "connection",
    "keep-alive",
];

/// How the bearer of one server's requests is found.
enum Bearer {
    /// ACP servers: their literal headers only, never host OAuth.
    Never,
    /// A pasted catalog token, bound to the endpoint, never resolved.
    StaticToken,
    /// An env var of the worker, else (with `oauth`) the bound stored OAuth access.
    Stored { env: Option<String>, oauth: bool },
    /// A server outside the settings: the bound stored credential, if any.
    Unconfigured,
}

/// What the worker adds to one server's requests.
struct Route {
    /// The configured endpoint; `None` for a server outside the settings.
    url: Option<String>,
    headers: HashMap<String, String>,
    bearer: Bearer,
}

/// `mcp.config` hands the kernel no literal header: drop them, and mark the server as relayed.
pub(super) fn strip_headers(config: &mut Value) {
    if let Value::Object(map) = config {
        let had_headers = map
            .remove("headers")
            .is_some_and(|headers| headers.as_object().is_some_and(|map| !map.is_empty()));
        if had_headers {
            map.insert("relay".to_string(), Value::Bool(true));
        }
    }
}

/// Registers `mcp.http` and `websearch.search` next to the other MCP host requests.
pub(super) fn register(
    manager: &Arc<std::sync::Mutex<McpManager>>,
    handlers: &mut HostRequestHandlers,
) {
    let (auth, acp_servers) = {
        let manager = manager.lock().unwrap();
        (
            manager.auth_storage_handle(),
            Arc::clone(&manager.acp_servers),
        )
    };
    let manager_for_http = Arc::clone(manager);
    let auth_for_http = Arc::clone(&auth);
    handlers.register(
        "mcp.http",
        host_handler(move |payload| {
            let manager = Arc::clone(&manager_for_http);
            let acp_servers = Arc::clone(&acp_servers);
            let auth = Arc::clone(&auth_for_http);
            Box::pin(async move { mcp_http(&manager, &acp_servers, &auth, &payload.data).await })
        }),
    );
    handlers.register(
        "websearch.search",
        host_handler(move |payload| {
            let auth = Arc::clone(&auth);
            Box::pin(async move { websearch(&auth, &payload.data).await })
        }),
    );
}

fn text_field<'a>(data: &'a Value, name: &str) -> &'a str {
    data.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn credentials_unavailable(server: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "MCP credentials for '{server}' are not available. Ask the user to connect it \
         (/plugins or /mcp login {server}); do not ask them to set environment variables."
    )
}

/// The route of a configured server (ACP, settings or catalog), `None` when not configured.
fn configured_route(
    manager: &std::sync::Mutex<McpManager>,
    acp_servers: &std::sync::Mutex<HashMap<String, AcpMcpServerConfig>>,
    server: &str,
) -> anyhow::Result<Option<Route>> {
    if let Some(acp) = acp_servers.lock().unwrap().get(server) {
        return match acp {
            AcpMcpServerConfig::Http { url, headers, .. } => Ok(Some(Route {
                url: Some(url.clone()),
                headers: headers.clone(),
                bearer: Bearer::Never,
            })),
            AcpMcpServerConfig::Stdio { .. } => Err(anyhow::anyhow!(
                "MCP server '{server}' is not an HTTP server"
            )),
        };
    }
    let manager = manager.lock().unwrap();
    let Some(integration) = manager.integrations.get(server) else {
        return Ok(None);
    };
    if integration.blocked_reason.is_some() {
        return Err(anyhow::anyhow!("MCP server '{server}' is not available"));
    }
    let McpServerConfig::Http {
        url,
        headers,
        bearer_token_env_var,
        oauth,
        ..
    } = &integration.config
    else {
        return Err(anyhow::anyhow!(
            "MCP server '{server}' is not an HTTP server"
        ));
    };
    let bearer = if integration.static_token_eligible {
        Bearer::StaticToken
    } else {
        Bearer::Stored {
            env: bearer_token_env_var.clone(),
            oauth: *oauth == Some(true),
        }
    };
    Ok(Some(Route {
        url: Some(url.clone()),
        headers: headers.clone().unwrap_or_default(),
        bearer,
    }))
}

/// The stored credential of `server`, only when bound to exactly `url`: a static token as
/// pasted, an OAuth access through `get_api_key` (which refreshes an expired one). A pasted
/// catalog token (`static_only`) accepts the static shape only.
async fn bound_token(
    auth: &tokio::sync::Mutex<AuthStorage>,
    server: &str,
    url: &str,
    static_only: bool,
) -> Option<String> {
    let provider = provider_id(server);
    let mut store = auth.lock().await;
    store.reload();
    let credential = store.get_all().credential(&provider)?;
    match credential {
        AuthCredential::McpStaticToken {
            bearer,
            endpoint: Some(endpoint),
        } if endpoint == url => {
            crate::auth::resolve_config_value::resolve_config_value(bearer.trim())
        } // MUTATION M3 (ticket 41): pasted bearer resolved
        AuthCredential::Oauth {
            endpoint: Some(endpoint),
            ..
        } if !static_only && !endpoint.is_empty() => store.get_api_key(&provider), // MUTATION M1 (ticket 41): any endpoint
        _ => None,
    }
}

/// The bearer for one request, or an error when the server needs one and none is usable.
async fn bearer_for(
    route: &Route,
    auth: &tokio::sync::Mutex<AuthStorage>,
    server: &str,
    url: &str,
) -> anyhow::Result<Option<String>> {
    match &route.bearer {
        Bearer::Never => Ok(None),
        Bearer::StaticToken => bound_token(auth, server, url, true)
            .await
            .map(Some)
            .ok_or_else(|| credentials_unavailable(server)),
        Bearer::Stored { env, oauth } => {
            let from_env = env
                .as_deref()
                .and_then(|name| std::env::var(name).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if from_env.is_some() {
                return Ok(from_env);
            }
            let stored = if *oauth {
                bound_token(auth, server, url, false).await
            } else {
                None
            };
            if stored.is_none() && (*oauth || env.is_some()) {
                return Err(credentials_unavailable(server));
            }
            Ok(stored)
        }
        Bearer::Unconfigured => Ok(bound_token(auth, server, url, false).await),
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        pa_ai::utils::tls::http_client_builder()
            // No redirects: a redirecting endpoint must not receive the credential.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .build()
            .expect("relay HTTP client")
    })
}

async fn mcp_http(
    manager: &std::sync::Mutex<McpManager>,
    acp_servers: &std::sync::Mutex<HashMap<String, AcpMcpServerConfig>>,
    auth: &tokio::sync::Mutex<AuthStorage>,
    data: &Value,
) -> anyhow::Result<Value> {
    let server = text_field(data, "server");
    let method = text_field(data, "method");
    let url = text_field(data, "url");
    if server.is_empty() || method.is_empty() || url.is_empty() {
        return Err(anyhow::anyhow!(
            "mcp.http requires a server, a method and a url"
        ));
    }
    // Complete responses only: the server-initiated message stream is not relayed.
    if method.eq_ignore_ascii_case("GET") {
        return Ok(json!({ "status": 405, "headers": {}, "body": "" }));
    }
    let method = match method.to_ascii_uppercase().as_str() {
        "POST" => reqwest::Method::POST,
        "DELETE" => reqwest::Method::DELETE,
        other => return Err(anyhow::anyhow!("mcp.http does not relay {other}")),
    };
    let route = configured_route(manager, acp_servers, server)?.unwrap_or_else(|| Route {
        url: None,
        headers: HashMap::new(),
        bearer: Bearer::Unconfigured,
    });
    let bearer = match &route.url {
        Some(configured) if configured != url => {
            return Err(anyhow::anyhow!(
                "mcp.http: {url} is not the configured endpoint of MCP server '{server}'"
            ));
        }
        Some(_) => bearer_for(&route, auth, server, url).await?,
        // Outside the settings, the bound credential is the only proof that this URL is the
        // server's: without one, nothing is sent.
        None => Some(
            bearer_for(&route, auth, server, url)
                .await?
                .ok_or_else(|| anyhow::anyhow!("MCP server '{server}' is not configured"))?,
        ),
    };

    let mut headers = reqwest::header::HeaderMap::new();
    let forwarded = data.get("headers").and_then(Value::as_object);
    for (name, value) in forwarded.into_iter().flatten() {
        let Some(value) = value.as_str() else {
            continue;
        };
        if DROPPED_REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            continue;
        }
        insert_header(&mut headers, name, value)?;
    }
    for (name, value) in &route.headers {
        insert_header(&mut headers, name, value)?;
    }
    if let Some(bearer) = bearer {
        insert_header(&mut headers, "authorization", &format!("Bearer {bearer}"))?;
    }
    let body = text_field(data, "body").to_string();
    let response = http_client()
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|error| {
            anyhow::anyhow!("mcp.http: request to MCP server '{server}' failed: {error}")
        })?;
    let status = response.status().as_u16();
    let mut reply_headers = Map::new();
    for (name, value) in response.headers() {
        if DROPPED_RESPONSE_HEADERS.contains(&name.as_str()) {
            continue;
        }
        if let Ok(value) = value.to_str() {
            reply_headers.insert(name.as_str().to_string(), Value::String(value.to_string()));
        }
    }
    let bytes = response.bytes().await.map_err(|error| {
        anyhow::anyhow!("mcp.http: reading the reply of MCP server '{server}' failed: {error}")
    })?;
    Ok(json!({
        "status": status,
        "headers": reply_headers,
        "body": String::from_utf8_lossy(&bytes),
    }))
}

fn insert_header(
    headers: &mut reqwest::header::HeaderMap,
    name: &str,
    value: &str,
) -> anyhow::Result<()> {
    let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| anyhow::anyhow!("mcp.http: invalid header name"))?;
    let value = reqwest::header::HeaderValue::from_str(value)
        .map_err(|_| anyhow::anyhow!("mcp.http: invalid header value"))?;
    headers.insert(name, value);
    Ok(())
}

/// The worker's Serper key: its own `SERPER_API_KEY`, else the stored `serper` credential
/// (a literal, an env var name or a `!command`, resolved off the async runtime).
async fn serper_key(auth: &tokio::sync::Mutex<AuthStorage>) -> anyhow::Result<Option<String>> {
    let ambient = std::env::var("SERPER_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if ambient.is_some() {
        return Ok(ambient);
    }
    let credential = {
        let mut store = auth.lock().await;
        store.reload();
        store.get_all().credential(SERPER_CREDENTIAL_ID)
    };
    let Some(AuthCredential::ApiKey { key, .. }) = credential else {
        return Ok(None);
    };
    let resolved = tokio::task::spawn_blocking(move || {
        crate::auth::resolve_config_value::resolve_config_value(&key)
    })
    .await?;
    Ok(resolved
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

async fn websearch(auth: &tokio::sync::Mutex<AuthStorage>, data: &Value) -> anyhow::Result<Value> {
    let query = text_field(data, "query").trim().to_string();
    if query.is_empty() {
        return Err(anyhow::anyhow!("websearch.search requires a query"));
    }
    let Some(key) = serper_key(auth).await? else {
        return Err(anyhow::anyhow!("websearch: no Serper key is configured"));
    };
    let timeout = data
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(45)
        .clamp(1, 120);
    let url = std::env::var(SERPER_URL_TEST_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| SERPER_URL.to_string());
    let response = http_client()
        .post(url)
        .header("X-API-KEY", key)
        .header("Content-Type", "application/json")
        .body(json!({ "q": query }).to_string())
        .timeout(Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Serper search failed: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| anyhow::anyhow!("Serper search failed: {error}"))?;
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "Serper search error ({}): {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| anyhow::anyhow!("Serper search: invalid reply: {error}"))
}
