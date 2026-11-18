// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines).
#![allow(clippy::large_futures, clippy::too_many_lines)]

//! yusAgent (YA-09, ticket 15): the error flag of a failed cell on the wire, and the misuse
//! messages of the kernel helpers in the tool outputs. A loopback HTTP server plays the model for
//! the Anthropic Messages and `OpenAI` Responses adapters and records every request body of a
//! real session (`create_session`, real provider adapter, real `python_repl` kernel). Four cells:
//! one raises, one succeeds, then two misuses (`h.exit_code` before `await`, `await read(...)`).
//! - Anthropic: each `tool_result` carries `is_error`, `true` for the three failed cells.
//! - Responses: each `function_call_output` keeps `type`, `call_id`, `output` and nothing else.
//! - Both: the system prompt, the tools and the parameters do not change between requests.
//!
//! The server machinery is a reduced copy of `crates/pa-core/tests/prefix_stability.rs` (left
//! unchanged). Listed in `docs/fork-changes.md`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pa_core::session::manager::SessionManager;
use pa_core::session_engine::engine::{create_session, SessionEngine, SessionEngineConfig};
use pa_core::session_engine::provider_adapter::real_stream_fn;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PROMPT: &str = "ERROR-FLAG-PROMPT: run four cells.";

/// The cells, in order, and whether each one fails.
const CELLS: [(&str, bool); 4] = [
    ("1/0", true),
    ("print('ok')", false),
    ("h = bash('true')\nh.exit_code", true),
    (
        "open('a.txt', 'w').write('x\\n')\nawait read('a.txt')",
        true,
    ),
];

/// Four tool calls then the final text.
const EXPECTED_REQUESTS: usize = CELLS.len() + 1;

/// What each cell's output must contain.
const OUTPUTS: [&str; 4] = [
    "ZeroDivisionError",
    "ok",
    "BashHandle has no .exit_code: the command may still run. Use r = await h, then r.exit_code",
    "read() is not async: call read(path) without await",
];

/// Process env is shared by the tests of this binary: one scenario at a time.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Api {
    AnthropicMessages,
    OpenAiResponses,
}

impl Api {
    fn name(self) -> &'static str {
        match self {
            Api::AnthropicMessages => "anthropic-messages",
            Api::OpenAiResponses => "openai-responses",
        }
    }

    fn base_url(self, port: u16) -> String {
        match self {
            Api::AnthropicMessages => format!("http://127.0.0.1:{port}"),
            Api::OpenAiResponses => format!("http://127.0.0.1:{port}/v1"),
        }
    }

    fn messages_key(self) -> &'static str {
        match self {
            Api::AnthropicMessages => "messages",
            Api::OpenAiResponses => "input",
        }
    }
}

/// The kernel Python with prime-agent-runtime installed (CI bootstraps it before the test
/// shards). Skipped with a note on a machine without one, never in CI.
fn kernel_python() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PA_CORE_KERNEL_PYTHON") {
        return Some(PathBuf::from(explicit));
    }
    let candidate = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".prime/agent/kernel-venv/bin/python");
    if candidate.exists() {
        return Some(candidate);
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "kernel python {} not found: the error-flag test must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping the error-flag test",
        candidate.display()
    );
    None
}

/// Scoped process-env overrides: applied on construction, restored on drop.
struct EnvOverride {
    saved: Vec<(String, Option<String>)>,
}

impl EnvOverride {
    fn apply(pairs: &[(&str, Option<String>)]) -> Self {
        let saved = pairs
            .iter()
            .map(|(key, _)| ((*key).to_string(), std::env::var(key).ok()))
            .collect();
        for (key, value) in pairs {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        EnvOverride { saved }
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

// Loopback model

enum Reply {
    Tool(&'static str),
    Text(&'static str),
}

struct ModelServer {
    port: u16,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
    _task: tokio::task::JoinHandle<()>,
}

impl ModelServer {
    async fn start(api: Api) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the model server");
        let port = listener.local_addr().expect("server address").port();
        let bodies: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let recorded = Arc::clone(&bodies);
        let task = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let recorded = Arc::clone(&recorded);
                tokio::spawn(async move {
                    if let Err(error) = serve(api, socket, &recorded).await {
                        eprintln!("model server: {error}");
                    }
                });
            }
        });
        Self {
            port,
            bodies,
            _task: task,
        }
    }

    fn bodies(&self) -> Vec<Vec<u8>> {
        self.bodies.lock().unwrap().clone()
    }
}

/// One request per connection: read the body, answer with SSE, close.
async fn serve(
    api: Api,
    mut socket: TcpStream,
    recorded: &Mutex<Vec<Vec<u8>>>,
) -> std::io::Result<()> {
    let body = read_body(&mut socket).await?;
    let reply = plan(api, &body);
    let seq = {
        let mut bodies = recorded.lock().unwrap();
        bodies.push(body);
        bodies.len()
    };
    let events = match api {
        Api::AnthropicMessages => anthropic_events(&reply, seq),
        Api::OpenAiResponses => responses_events(&reply, seq),
    };
    socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        )
        .await?;
    socket.write_all(events.as_bytes()).await?;
    socket.shutdown().await
}

async fn read_body(socket: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16_384];
    loop {
        let count = socket.read(&mut buffer).await?;
        if count == 0 {
            return Err(std::io::Error::other("connection closed before the body"));
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..end]);
        let length: usize = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())
                    .flatten()
            })
            .ok_or_else(|| std::io::Error::other(format!("no content-length in {head:?}")))?;
        if bytes.len() >= end + 4 + length {
            return Ok(bytes[end + 4..end + 4 + length].to_vec());
        }
    }
}

/// The reply follows from the number of tool results already in the conversation.
fn plan(api: Api, body: &[u8]) -> Reply {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let done = tool_results(
        api,
        request[api.messages_key()]
            .as_array()
            .map_or(&[][..], Vec::as_slice),
    )
    .len();
    CELLS
        .get(done)
        .map_or(Reply::Text("Four cells ran."), |&(code, _)| {
            Reply::Tool(code)
        })
}

fn sse(event: &str, data: &Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

fn anthropic_events(reply: &Reply, seq: usize) -> String {
    let usage = json!({
        "input_tokens": 1000, "output_tokens": 1,
        "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
    });
    let mut events = vec![sse(
        "message_start",
        &json!({"type": "message_start", "message": {
            "id": format!("msg_flag_{seq}"), "type": "message", "role": "assistant",
            "model": "flag-model", "content": [], "stop_reason": null,
            "stop_sequence": null, "usage": usage
        }}),
    )];
    let stop_reason = match reply {
        Reply::Tool(code) => {
            events.push(sse(
                "content_block_start",
                &json!({"type": "content_block_start", "index": 0, "content_block": {
                    "type": "tool_use", "id": format!("toolu_flag_{seq}"),
                    "name": "python_repl", "input": {}
                }}),
            ));
            events.push(sse(
                "content_block_delta",
                &json!({"type": "content_block_delta", "index": 0, "delta": {
                    "type": "input_json_delta", "partial_json": json!({"code": code}).to_string()
                }}),
            ));
            "tool_use"
        }
        Reply::Text(text) => {
            events.push(sse(
                "content_block_start",
                &json!({"type": "content_block_start", "index": 0,
                        "content_block": {"type": "text", "text": ""}}),
            ));
            events.push(sse(
                "content_block_delta",
                &json!({"type": "content_block_delta", "index": 0,
                        "delta": {"type": "text_delta", "text": text}}),
            ));
            "end_turn"
        }
    };
    events.push(sse(
        "content_block_stop",
        &json!({"type": "content_block_stop", "index": 0}),
    ));
    let mut final_usage = usage;
    final_usage["output_tokens"] = json!(8);
    events.push(sse(
        "message_delta",
        &json!({"type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                "usage": final_usage}),
    ));
    events.push(sse("message_stop", &json!({"type": "message_stop"})));
    events.concat()
}

fn responses_item(reply: &Reply, seq: usize, done: bool) -> Value {
    let status = if done { "completed" } else { "in_progress" };
    match reply {
        Reply::Tool(code) => json!({
            "type": "function_call", "id": format!("fc_flag_{seq}"),
            "call_id": format!("call_flag_{seq}"), "name": "python_repl",
            "arguments": if done { json!({"code": code}).to_string() } else { String::new() },
            "status": status
        }),
        Reply::Text(text) => json!({
            "type": "message", "id": format!("msg_flag_{seq}"), "role": "assistant",
            "status": status,
            "content": if done {
                json!([{"type": "output_text", "text": text, "annotations": []}])
            } else {
                json!([])
            }
        }),
    }
}

fn responses_object(reply: &Reply, seq: usize, done: bool) -> Value {
    let mut object = json!({
        "id": format!("resp_flag_{seq}"), "object": "response", "model": "flag-model",
        "status": if done { "completed" } else { "in_progress" },
        "output": if done { json!([responses_item(reply, seq, true)]) } else { json!([]) }
    });
    if done {
        object["usage"] = json!({
            "input_tokens": 1000, "output_tokens": 8, "total_tokens": 1008,
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens_details": {"reasoning_tokens": 0}
        });
    }
    object
}

fn responses_events(reply: &Reply, seq: usize) -> String {
    let mut events = vec![
        sse(
            "response.created",
            &json!({"type": "response.created", "response": responses_object(reply, seq, false)}),
        ),
        sse(
            "response.output_item.added",
            &json!({"type": "response.output_item.added", "output_index": 0,
                    "item": responses_item(reply, seq, false)}),
        ),
    ];
    match reply {
        Reply::Tool(code) => {
            let item_id = format!("fc_flag_{seq}");
            let arguments = json!({"code": code}).to_string();
            events.push(sse(
                "response.function_call_arguments.delta",
                &json!({"type": "response.function_call_arguments.delta", "item_id": item_id,
                        "output_index": 0, "delta": arguments}),
            ));
            events.push(sse(
                "response.function_call_arguments.done",
                &json!({"type": "response.function_call_arguments.done", "item_id": item_id,
                        "output_index": 0, "arguments": arguments}),
            ));
        }
        Reply::Text(text) => {
            let item_id = format!("msg_flag_{seq}");
            let part = json!({"type": "output_text", "text": "", "annotations": []});
            let mut done_part = part.clone();
            done_part["text"] = json!(text);
            events.push(sse(
                "response.content_part.added",
                &json!({"type": "response.content_part.added", "item_id": item_id,
                        "output_index": 0, "content_index": 0, "part": part}),
            ));
            events.push(sse(
                "response.output_text.delta",
                &json!({"type": "response.output_text.delta", "item_id": item_id,
                        "output_index": 0, "content_index": 0, "delta": text}),
            ));
            events.push(sse(
                "response.output_text.done",
                &json!({"type": "response.output_text.done", "item_id": item_id,
                        "output_index": 0, "content_index": 0, "text": text}),
            ));
            events.push(sse(
                "response.content_part.done",
                &json!({"type": "response.content_part.done", "item_id": item_id,
                        "output_index": 0, "content_index": 0, "part": done_part}),
            ));
        }
    }
    events.push(sse(
        "response.output_item.done",
        &json!({"type": "response.output_item.done", "output_index": 0,
                "item": responses_item(reply, seq, true)}),
    ));
    events.push(sse(
        "response.completed",
        &json!({"type": "response.completed", "response": responses_object(reply, seq, true)}),
    ));
    events.concat()
}

// Session

async fn open_session(
    api: Api,
    server: &ModelServer,
    agent_dir: &Path,
    cwd: &Path,
) -> SessionEngine {
    let model = json!({
        "id": "flag-model", "name": "Flag model", "api": api.name(),
        "provider": "flag-test", "baseUrl": api.base_url(server.port), "reasoning": false,
        "input": ["text"],
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 200_000, "maxTokens": 32_000
    });
    let provider_model: pa_types::ai::Model =
        serde_json::from_value(model.clone()).expect("provider model");
    let loop_model: pa_agent::types::Model = serde_json::from_value(model).expect("loop model");
    let mut session = SessionManager::in_memory(cwd);
    session.materialize_session_file(Some(agent_dir.join("sessions")));
    create_session(SessionEngineConfig {
        on_late_sent_agent_message: None,
        semantic_edges: None,
        cron_store: None,
        steering_mode: None,
        follow_up_mode: None,
        cwd: cwd.to_path_buf(),
        agent_dir: agent_dir.to_path_buf(),
        model: Some(loop_model),
        thinking_level: None,
        stream_fn: Some(real_stream_fn(
            Some("flag-test-placeholder".to_string()),
            provider_model,
        )),
        tools: Vec::new(),
        custom_system_prompt: None,
        prompt_guidelines: Vec::new(),
        generic_mcp_servers: Vec::new(),
        allow_recursion: None,
        session_manager: Some(session),
        extra_host_handlers: None,
        conversation_log_path: None,
        additional_skill_paths: Vec::new(),
        additional_prompt_paths: Vec::new(),
        extra_builtin_skill_overrides: Vec::new(),
        rlm_subagent_host: None,
        rlm_usage_store: None,
        progress_note_emit: None,
        rlm_depth: None,
        factory_child: false,
        daemonless: true,
        telemetry: None,
        model_info: None,
        mcp_manager: None,
        prewarm_ipython_kernel: None,
        on_background_work_settled: None,
        bash_notice_wake: None,
        queued_goal_context_purge: None,
        queued_steering_probe: None,
        should_stop_after_turn: None,
        image_model_router: None,
    })
    .await
    .expect("create_session")
}

/// The bodies of one scenario, or `None` without a kernel.
async fn scenario(api: Api) -> Option<Vec<Vec<u8>>> {
    let _guard = ENV_LOCK.lock().await;
    let kernel_python = kernel_python()?;
    let dir = tempfile::tempdir().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(agent_dir.join("sessions")).expect("sessions dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let _env = EnvOverride::apply(&[
        (
            "PRIME_AGENT_KERNEL_PYTHON",
            Some(kernel_python.display().to_string()),
        ),
        ("PRIME_AGENT_CODING_AGENT_DIR", None),
        ("PRIME_API_KEY", None),
        ("PI_CACHE_RETENTION", None),
        // The product defaults (`rlmMaxDepth` 0, prompt version 2), not the upstream ones
        // `.cargo/config.toml` sets for the upstream tests.
        ("RLM_MAX_DEPTH", None),
        ("PRIME_AGENT_PROMPT_VERSION", None),
    ]);
    let server = ModelServer::start(api).await;
    let engine = open_session(api, &server, &agent_dir, &cwd).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(240),
        engine.prompt(PROMPT, pa_core::session_engine::PromptOptions::default()),
    )
    .await
    .unwrap_or_else(|_| panic!("{}: the prompt did not settle in 240s", api.name()))
    .unwrap_or_else(|error| panic!("{}: the prompt failed: {error:#}", api.name()));
    drop(engine);
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        EXPECTED_REQUESTS,
        "{}: unexpected request count",
        api.name()
    );
    Some(bodies)
}

fn request(api: Api, body: &[u8]) -> Map<String, Value> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_else(|| panic!("{}: a request is not a JSON object", api.name()))
}

/// The tool results of a conversation: Anthropic `tool_result` blocks, Responses
/// `function_call_output` items.
fn tool_results(api: Api, conversation: &[Value]) -> Vec<Value> {
    match api {
        Api::AnthropicMessages => conversation
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .cloned()
            .collect(),
        Api::OpenAiResponses => conversation
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .cloned()
            .collect(),
    }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => String::new(),
    }
}

/// Everything but the conversation, and the Responses system item (the first `input` item).
fn fixed_part(api: Api, request: &Map<String, Value>) -> Value {
    let mut fixed = request.clone();
    let conversation = fixed.remove(api.messages_key());
    if api == Api::OpenAiResponses {
        let system = conversation
            .as_ref()
            .and_then(|items| items.get(0))
            .cloned()
            .unwrap_or(Value::Null);
        fixed.insert("input[0]".to_string(), system);
    }
    Value::Object(fixed)
}

/// The checks common to both adapters; returns the tool results of the last request.
fn check_outputs(api: Api, bodies: &[Vec<u8>]) -> Vec<Value> {
    let first = request(api, &bodies[0]);
    for (index, body) in bodies.iter().enumerate().skip(1) {
        assert_eq!(
            fixed_part(api, &request(api, body)),
            fixed_part(api, &first),
            "{}: request {} changed the system prompt, the tools or a parameter",
            api.name(),
            index + 1
        );
    }
    let last = request(api, bodies.last().expect("a request"));
    let results = tool_results(
        api,
        last[api.messages_key()]
            .as_array()
            .map_or(&[][..], Vec::as_slice),
    );
    assert_eq!(results.len(), CELLS.len(), "{}: {results:?}", api.name());
    for (result, expected) in results.iter().zip(OUTPUTS) {
        let text = match api {
            Api::AnthropicMessages => text_of(&result["content"]),
            Api::OpenAiResponses => text_of(&result["output"]),
        };
        assert!(
            text.contains(expected),
            "{}: {expected:?} not in {text:?}",
            api.name()
        );
    }
    results
}

#[tokio::test]
async fn anthropic_tool_results_carry_the_error_flag() {
    let api = Api::AnthropicMessages;
    let Some(bodies) = scenario(api).await else {
        return;
    };
    let results = check_outputs(api, &bodies);
    let flags: Vec<Value> = results
        .iter()
        .map(|result| result.get("is_error").cloned().unwrap_or(Value::Null))
        .collect();
    let expected: Vec<Value> = CELLS.iter().map(|(_, failed)| json!(failed)).collect();
    assert_eq!(flags, expected, "{results:?}");
}

#[tokio::test]
async fn responses_tool_outputs_have_no_error_field() {
    let api = Api::OpenAiResponses;
    let Some(bodies) = scenario(api).await else {
        return;
    };
    for result in check_outputs(api, &bodies) {
        let mut keys: Vec<&str> = result
            .as_object()
            .expect("an item")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["call_id", "output", "type"], "{result}");
    }
}
