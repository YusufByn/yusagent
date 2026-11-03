// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines/casts).
#![allow(
    clippy::large_futures,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

//! yusAgent (YA-04, ticket 03): prompt-cache prefix guard. A loopback HTTP server plays the
//! model for the Anthropic Messages and `OpenAI` Responses adapters and records every request
//! body of a real session (`create_session`, real provider adapter, real `python_repl` kernel)
//! over three prompts with tool calls. Between request N and N+1:
//! - the system prompt, the tools and every request parameter are byte-identical;
//! - the messages of N are an exact prefix of those of N+1, where only the `cache_control`
//!   marker may move (`crates/pa-ai/src/providers/anthropic/convert.rs`).
//!
//! A failure prints the first differing byte. Listed in `docs/fork-changes.md`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pa_core::session::manager::SessionManager;
use pa_core::session_engine::engine::{create_session, SessionEngine, SessionEngineConfig};
use pa_core::session_engine::provider_adapter::real_stream_fn;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The three prompts of the scenario. The server reads the stage from the marker.
const PROMPTS: [&str; 3] = [
    "PREFIX-PROMPT-1: run two cells.",
    "PREFIX-PROMPT-2: answer without a tool.",
    "PREFIX-PROMPT-3: run one more cell.",
];

/// The third cell: reads `x` from the second cell, then the conversation log path and a
/// bundled skill through the kernel env.
const THIRD_CELL: &str = "import os\nprint(x + 1)\nprint(os.environ['PRIME_AGENT_CONVERSATION_LOG'].endswith('.jsonl'), os.path.isfile(os.path.expandvars('$PRIME_AGENT_BUNDLED_SKILLS_DIR/edit/SKILL.md')))";

/// Requests the scenario sends: two tool calls then text, text, one tool call then text.
const EXPECTED_REQUESTS: usize = 6;

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

    /// The body key that carries the conversation.
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
        "kernel python {} not found: the prefix guard must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping the prefix guard",
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

/// One scripted model reply.
enum Reply {
    Tool(&'static str),
    Text(&'static str),
}

/// Records every request body, in arrival order.
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

/// The reply follows from the request alone: the prompt stage (latest marker present) and the
/// number of tool results already in the conversation.
fn plan(api: Api, body: &[u8]) -> Reply {
    let text = String::from_utf8_lossy(body);
    let stage = (1..=PROMPTS.len())
        .rev()
        .find(|stage| text.contains(&format!("PREFIX-PROMPT-{stage}:")))
        .unwrap_or(0);
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let conversation = request[api.messages_key()]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let tool_results = match api {
        Api::AnthropicMessages => conversation
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .count(),
        Api::OpenAiResponses => conversation
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .count(),
    };
    match (stage, tool_results) {
        (1, 0) => Reply::Tool("print(1)"),
        (1, 1) => Reply::Tool("x = 2\nprint(x)"),
        // yusAgent (T03, YA-04): the kernel carries what the prompt no longer does.
        (3, 2) => Reply::Tool(THIRD_CELL),
        (1, _) => Reply::Text("Two cells ran."),
        (3, _) => Reply::Text("One more cell ran."),
        _ => Reply::Text("Answer without a tool."),
    }
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
            "id": format!("msg_prefix_{seq}"), "type": "message", "role": "assistant",
            "model": "prefix-model", "content": [], "stop_reason": null,
            "stop_sequence": null, "usage": usage
        }}),
    )];
    let stop_reason = match reply {
        Reply::Tool(code) => {
            events.push(sse(
                "content_block_start",
                &json!({"type": "content_block_start", "index": 0, "content_block": {
                    "type": "tool_use", "id": format!("toolu_prefix_{seq}"),
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
            "type": "function_call", "id": format!("fc_prefix_{seq}"),
            "call_id": format!("call_prefix_{seq}"), "name": "python_repl",
            "arguments": if done { json!({"code": code}).to_string() } else { String::new() },
            "status": status
        }),
        Reply::Text(text) => json!({
            "type": "message", "id": format!("msg_prefix_{seq}"), "role": "assistant",
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
        "id": format!("resp_prefix_{seq}"), "object": "response", "model": "prefix-model",
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
            let item_id = format!("fc_prefix_{seq}");
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
            let item_id = format!("msg_prefix_{seq}");
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

fn model_json(api: Api, port: u16) -> Value {
    json!({
        "id": "prefix-model", "name": "Prefix model", "api": api.name(),
        "provider": "prefix-test", "baseUrl": api.base_url(port), "reasoning": false,
        "input": ["text"],
        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
        "contextWindow": 200_000, "maxTokens": 32_000
    })
}

/// A persisted session in `agent_dir/sessions`, served by `server`.
async fn open_session(
    api: Api,
    server: &ModelServer,
    agent_dir: &Path,
    cwd: &Path,
) -> SessionEngine {
    let mut session = SessionManager::in_memory(cwd);
    session.materialize_session_file(Some(agent_dir.join("sessions")));
    let provider_model: pa_types::ai::Model =
        serde_json::from_value(model_json(api, server.port)).expect("provider model");
    let loop_model: pa_agent::types::Model =
        serde_json::from_value(model_json(api, server.port)).expect("loop model");
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
        // A loopback-only placeholder: the server ignores it.
        stream_fn: Some(real_stream_fn(
            Some("prefix-test-placeholder".to_string()),
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

async fn run_prompt(engine: &SessionEngine, prompt: &str) {
    tokio::time::timeout(
        std::time::Duration::from_secs(240),
        engine.prompt(prompt, pa_core::session_engine::PromptOptions::default()),
    )
    .await
    .unwrap_or_else(|_| panic!("prompt {prompt:?} did not settle in 240s"))
    .unwrap_or_else(|error| panic!("prompt {prompt:?} failed: {error:#}"));
}

/// Temp dirs and env overrides for one scenario. Fields drop in order: the env is restored
/// before the lock is released.
struct Fixture {
    dir: tempfile::TempDir,
    _env: EnvOverride,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let guard = ENV_LOCK.lock().await;
        let kernel_python = kernel_python()?;
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("agent/sessions")).expect("sessions dir");
        std::fs::create_dir_all(dir.path().join("project")).expect("project dir");
        let env = EnvOverride::apply(&[
            (
                "PRIME_AGENT_KERNEL_PYTHON",
                Some(kernel_python.display().to_string()),
            ),
            ("PRIME_AGENT_CODING_AGENT_DIR", None),
            ("PRIME_API_KEY", None),
            ("PI_CACHE_RETENTION", None),
        ]);
        Some(Self {
            dir,
            _env: env,
            _guard: guard,
        })
    }

    fn agent_dir(&self) -> PathBuf {
        self.dir.path().join("agent")
    }

    fn cwd(&self) -> PathBuf {
        self.dir.path().join("project")
    }
}

// Comparison

/// The request as an ordered object. With `preserve_order`, re-serializing a value reproduces its
/// wire bytes, so comparing re-serialized values compares the bytes the provider received.
fn request_object(api: Api, seq: usize, body: &[u8]) -> Map<String, Value> {
    let value: Value = serde_json::from_slice(body)
        .unwrap_or_else(|error| panic!("{} request {seq}: invalid JSON: {error}", api.name()));
    assert!(
        serde_json::to_vec(&value).expect("re-serialize") == body,
        "{} request {seq}: the body is not compact JSON, so value bytes are not wire bytes",
        api.name()
    );
    value
        .as_object()
        .cloned()
        .unwrap_or_else(|| panic!("{} request {seq}: not a JSON object", api.name()))
}

/// The fixed part of a request (system prompt, tools, parameters) and its messages. The
/// Responses system prompt is the first `input` item.
fn split_request(api: Api, request: &Map<String, Value>) -> (Map<String, Value>, Vec<Value>) {
    let mut fixed = request.clone();
    let mut messages = fixed
        .remove(api.messages_key())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    if api == Api::OpenAiResponses && !messages.is_empty() {
        fixed.insert("input[0] (system)".to_string(), messages.remove(0));
    }
    (fixed, messages)
}

fn strip_cache_control(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(key, value)| (key.clone(), strip_cache_control(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_cache_control).collect()),
        other => other.clone(),
    }
}

fn char_floor(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The first differing byte, with context on both sides.
fn first_difference(before: &str, after: &str) -> String {
    let at = before
        .bytes()
        .zip(after.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| before.len().min(after.len()));
    let excerpt = |text: &str| {
        let start = char_floor(text, at.saturating_sub(80));
        let end = char_floor(text, at + 160);
        format!("{:?}", &text[start..end])
    };
    format!(
        "first difference at byte {at} (lengths {} -> {}):\n  N:   {}\n  N+1: {}",
        before.len(),
        after.len(),
        excerpt(before),
        excerpt(after)
    )
}

fn assert_prefix_stable(api: Api, bodies: &[Vec<u8>]) {
    let requests: Vec<Map<String, Value>> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| request_object(api, index + 1, body))
        .collect();
    for (index, pair) in requests.windows(2).enumerate() {
        let (n, next) = (index + 1, index + 2);
        let label = format!("{} request {n} -> {next}", api.name());
        let (fixed_before, messages_before) = split_request(api, &pair[0]);
        let (fixed_after, messages_after) = split_request(api, &pair[1]);
        let keys: Vec<&String> = fixed_before
            .keys()
            .chain(
                fixed_after
                    .keys()
                    .filter(|key| !fixed_before.contains_key(*key)),
            )
            .collect();
        for key in keys {
            let render = |object: &Map<String, Value>| {
                object
                    .get(key)
                    .map_or_else(|| "<absent>".to_string(), Value::to_string)
            };
            let (before, after) = (render(&fixed_before), render(&fixed_after));
            assert!(
                before == after,
                "{label}: `{key}` changed\n{}",
                first_difference(&before, &after)
            );
        }
        assert!(
            messages_before.len() <= messages_after.len(),
            "{label}: the conversation shrank ({} -> {} messages)",
            messages_before.len(),
            messages_after.len()
        );
        for (position, (before, after)) in messages_before.iter().zip(&messages_after).enumerate() {
            if before == after || strip_cache_control(before) == strip_cache_control(after) {
                continue;
            }
            panic!(
                "{label}: message {position} changed (not a cache_control move)\n{}",
                first_difference(&before.to_string(), &after.to_string())
            );
        }
    }
}

// Scenarios

async fn prefix_scenario(api: Api) {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let server = ModelServer::start(api).await;
    let engine = open_session(api, &server, &fixture.agent_dir(), &fixture.cwd()).await;
    for prompt in PROMPTS {
        run_prompt(&engine, prompt).await;
    }
    drop(engine);
    let bodies = server.bodies();
    assert_eq!(
        bodies.len(),
        EXPECTED_REQUESTS,
        "{}: unexpected request count",
        api.name()
    );
    assert_prefix_stable(api, &bodies);
    // The cells really ran in one kernel: `x` set by the second cell is read by the third, which
    // also finds the log path and a bundled skill through the kernel env.
    let last = request_object(api, bodies.len(), bodies.last().expect("a request"));
    assert_eq!(
        tool_outputs(api, &last),
        ["1", "2", "3\nTrue True"],
        "{}: kernel outputs",
        api.name()
    );
}

/// The text of every tool result in a request, trimmed.
fn tool_outputs(api: Api, request: &Map<String, Value>) -> Vec<String> {
    fn text_of(value: &Value) -> String {
        match value {
            Value::String(text) => text.trim().to_string(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<String>()
                .trim()
                .to_string(),
            _ => String::new(),
        }
    }
    let (_, messages) = split_request(api, request);
    match api {
        Api::AnthropicMessages => messages
            .iter()
            .filter_map(|message| message["content"].as_array())
            .flatten()
            .filter(|block| block["type"] == "tool_result")
            .map(|block| text_of(&block["content"]))
            .collect(),
        Api::OpenAiResponses => messages
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .map(|item| text_of(&item["output"]))
            .collect(),
    }
}

#[tokio::test]
async fn anthropic_messages_prefix_is_stable_across_requests() {
    prefix_scenario(Api::AnthropicMessages).await;
}

#[tokio::test]
async fn openai_responses_prefix_is_stable_across_requests() {
    prefix_scenario(Api::OpenAiResponses).await;
}

/// The first request of a one-prompt session (a text answer: no kernel cell).
async fn first_request_of_a_session(api: Api, fixture: &Fixture) -> Map<String, Value> {
    let server = ModelServer::start(api).await;
    let engine = open_session(api, &server, &fixture.agent_dir(), &fixture.cwd()).await;
    run_prompt(&engine, PROMPTS[1]).await;
    drop(engine);
    let bodies = server.bodies();
    assert_eq!(bodies.len(), 1, "{}: unexpected request count", api.name());
    request_object(api, 1, &bodies[0])
}

/// The texts of the first user message of a request.
fn first_user_texts(api: Api, request: &Map<String, Value>) -> Vec<String> {
    let (_, messages) = split_request(api, request);
    let first = messages.first().expect("a user message");
    match &first["content"] {
        Value::String(text) => vec![text.clone()],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str().map(str::to_string))
            .collect(),
        other => panic!("{}: unexpected user content {other}", api.name()),
    }
}

/// yusAgent (T03, YA-04): two successive sessions of one project send a byte-identical system
/// prompt and tools. The prompt carries no date, cwd or session id; the first user message
/// carries the `<environment>` block with the date and the cwd.
async fn two_sessions_scenario(api: Api) {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let first = first_request_of_a_session(api, &fixture).await;
    let second = first_request_of_a_session(api, &fixture).await;
    let (fixed_first, _) = split_request(api, &first);
    let (fixed_second, _) = split_request(api, &second);
    let system_key = match api {
        Api::AnthropicMessages => "system",
        Api::OpenAiResponses => "input[0] (system)",
    };
    for key in [system_key, "tools"] {
        let render = |object: &Map<String, Value>| {
            object
                .get(key)
                .map_or_else(|| "<absent>".to_string(), Value::to_string)
        };
        let (before, after) = (render(&fixed_first), render(&fixed_second));
        assert!(
            before == after,
            "{}: session 1 -> session 2: `{key}` changed\n{}",
            api.name(),
            first_difference(&before, &after)
        );
    }

    let system = fixed_first[system_key].to_string();
    let cwd = fixture.cwd().display().to_string();
    assert!(
        !system.contains(&cwd),
        "{}: the system prompt carries the cwd",
        api.name()
    );
    for entry in std::fs::read_dir(fixture.agent_dir().join("sessions")).expect("sessions dir") {
        let path = entry.expect("session entry").path();
        let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        assert!(
            !system.contains(session_id),
            "{}: the system prompt carries the session id {session_id}",
            api.name()
        );
    }
    assert!(
        !system.contains("Current date:"),
        "{}: the system prompt carries the date",
        api.name()
    );

    let texts = first_user_texts(api, &first);
    assert_eq!(texts[0], PROMPTS[1], "{}: user text changed", api.name());
    let block = texts.last().expect("environment block");
    let date = block
        .strip_prefix("<environment>\nCurrent date: ")
        .and_then(|rest| rest.get(..10))
        .unwrap_or_else(|| panic!("{}: no environment block in {texts:?}", api.name()));
    assert!(
        date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
        "{}: not a date: {date:?}",
        api.name()
    );
    assert_eq!(
        *block,
        format!("<environment>\nCurrent date: {date}\nWorking directory: {cwd}\n</environment>"),
        "{}: environment block",
        api.name()
    );
}

#[tokio::test]
async fn anthropic_messages_system_prompt_is_identical_across_sessions() {
    two_sessions_scenario(Api::AnthropicMessages).await;
}

#[tokio::test]
async fn openai_responses_system_prompt_is_identical_across_sessions() {
    two_sessions_scenario(Api::OpenAiResponses).await;
}

/// The `cache_control` markers of an Anthropic request, wherever they sit.
fn cache_controls(value: &Value, found: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "cache_control" {
                    found.push(child.clone());
                } else {
                    cache_controls(child, found);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| cache_controls(item, found)),
        _ => {}
    }
}

/// yusAgent (T03, YA-04): `cacheRetention` reaches the provider request. Unset: the 5-minute
/// marker (no `ttl`); `"long"`: `ttl: 1h` on every marker.
#[tokio::test]
async fn anthropic_cache_retention_follows_the_setting() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let api = Api::AnthropicMessages;
    let markers = |request: &Map<String, Value>| {
        let mut found = Vec::new();
        cache_controls(&Value::Object(request.clone()), &mut found);
        assert!(!found.is_empty(), "no cache_control marker");
        found
    };
    let default = first_request_of_a_session(api, &fixture).await;
    for marker in markers(&default) {
        assert_eq!(marker, json!({"type": "ephemeral"}));
    }
    std::fs::write(
        fixture.agent_dir().join("settings.json"),
        json!({"cacheRetention": "long"}).to_string(),
    )
    .expect("settings.json");
    let long = first_request_of_a_session(api, &fixture).await;
    for marker in markers(&long) {
        assert_eq!(marker, json!({"type": "ephemeral", "ttl": "1h"}));
    }
}
