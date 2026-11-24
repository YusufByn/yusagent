// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-10, ticket 31, test 13): the daemon command `append_custom_message` writes its
//! row to the session file but does not reach the context of the engine already built: the next
//! request to the model does not carry it (`docs/tickets/31-skills-mcp-reglages.md`, §1.6). That
//! is why the announcements of skills and MCP changes go through the engine's next-turn rows
//! (`queue_next_turn_row`) instead. Real supervisor and worker (`pa-daemon`), a real kernel, and
//! a loopback server that plays the model and records every request body (harness of
//! `ya_permissions_same_requests_e2e.rs`). If this test fails, the plan of ticket 31 is reviewed
//! before any code. Listed in `docs/fork-changes.md`.
//!
//! Ticket 35 (`docs/tickets/35-regression-r3-abort-queued.md`): a daemon delivery compares the
//! disk once, at the worker's call, never again at the admission, where a read would sit in the
//! window in which an abort is lost.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const TIMEOUT: Duration = Duration::from_mins(2);
const PROMPT: &str = "Run one cell, then answer.";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `crates/pa-cli/tests/ya_permissions_acp_e2e.rs`.
fn kernel_python() -> PathBuf {
    if let Some(explicit) = std::env::var_os("PA_E2E_KERNEL_PYTHON") {
        return PathBuf::from(explicit);
    }
    let candidate = PathBuf::from(std::env::var("HOME").map_or_else(
        |_| "/home/ubuntu/.yusagent/kernel-venv/bin/python".to_string(),
        |home| format!("{home}/.yusagent/kernel-venv/bin/python"),
    ));
    assert!(
        candidate.exists(),
        "kernel python {} not found: set PA_E2E_KERNEL_PYTHON",
        candidate.display()
    );
    candidate
}

// The model

/// Records every request body, in arrival order. The reply follows from the request alone: a
/// cell while the conversation has no tool result, then an answer. Its ids are fixed, so both
/// sessions get byte-identical replies.
struct ModelServer {
    port: u16,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the model server");
        let port = listener.local_addr().expect("server address").port();
        let bodies: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let recorded = Arc::clone(&bodies);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let recorded = Arc::clone(&recorded);
                std::thread::spawn(move || {
                    if let Some(body) = read_body(&mut stream) {
                        let reply = reply_events(&body);
                        recorded.lock().unwrap().push(body);
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.flush();
                    }
                });
            }
        });
        Self { port, bodies }
    }

    fn bodies(&self) -> Vec<Vec<u8>> {
        self.bodies.lock().unwrap().clone()
    }
}

/// The body of one request (`Content-Length` framing).
fn read_body(stream: &mut std::net::TcpStream) -> Option<Vec<u8>> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16_384];
    loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..end]).to_string();
        let length: usize = head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })?;
        if bytes.len() >= end + 4 + length {
            return Some(bytes[end + 4..end + 4 + length].to_vec());
        }
    }
}

fn sse(event: &str, data: &Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

/// The Responses stream of the reply to `body`.
fn reply_events(body: &[u8]) -> String {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let has_result = request["input"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["type"] == "function_call_output")
    });
    let (response_id, item) = if has_result {
        (
            "resp_answer",
            json!({
                "type": "message", "id": "msg_answer", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": "The cell ran.", "annotations": []}]
            }),
        )
    } else {
        (
            "resp_cell",
            json!({
                "type": "function_call", "id": "fc_cell", "call_id": "call_cell",
                "name": "python_repl", "status": "completed",
                "arguments": json!({"code": "print(6 * 7)"}).to_string()
            }),
        )
    };
    let mut pending = item.clone();
    pending["status"] = json!("in_progress");
    let response = |done: bool| {
        let mut object = json!({
            "id": response_id, "object": "response", "model": "same-model",
            "status": if done { "completed" } else { "in_progress" },
            "output": if done { json!([item.clone()]) } else { json!([]) },
        });
        if done {
            object["usage"] = json!({
                "input_tokens": 1000, "output_tokens": 8, "total_tokens": 1008,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens_details": {"reasoning_tokens": 0}
            });
        }
        object
    };
    let (created, completed) = (response(false), response(true));
    [
        sse(
            "response.created",
            &json!({"type": "response.created", "response": created}),
        ),
        sse(
            "response.output_item.added",
            &json!({"type": "response.output_item.added", "output_index": 0, "item": pending}),
        ),
        sse(
            "response.output_item.done",
            &json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
        ),
        sse(
            "response.completed",
            &json!({"type": "response.completed", "response": completed}),
        ),
    ]
    .concat()
}

// The daemon

struct Supervisor {
    child: Child,
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_supervisor(socket: &Path, agent_dir: &Path) -> Supervisor {
    spawn_supervisor_with(socket, agent_dir, None)
}

/// `trace`: the file of `PA_COMPACTION_TRACE`, which the workers inherit from the supervisor.
// The supervisor is killed and waited for by `Supervisor`'s drop (as `abort_kernel_e2e.rs`).
#[allow(clippy::zombie_processes)]
fn spawn_supervisor_with(socket: &Path, agent_dir: &Path, trace: Option<&Path>) -> Supervisor {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pa-daemon"));
    command.env_remove("PA_COMPACTION_TRACE");
    if let Some(trace) = trace {
        command.env("PA_COMPACTION_TRACE", trace);
    }
    let child = command
        .arg("supervisor")
        .arg("--socket")
        .arg(socket)
        .arg("--agent-dir")
        .arg(agent_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove(TEST_ENV)
        .env_remove("PRIME_API_KEY")
        .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
        .env_remove(pa_daemon::worker::WORKER_SCRIPT_ENV)
        .env("DO_NOT_TRACK", "1")
        .env("PI_OFFLINE", "1")
        .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
        .env(
            pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
            "15000",
        )
        .spawn()
        .expect("spawn pa-daemon supervisor");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return Supervisor { child };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("supervisor socket never came up");
}

/// A client on the supervisor socket; it keeps the session-event frames it reads.
struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    events: Vec<Value>,
    next_id: u64,
}

impl Client {
    fn connect(socket: &Path) -> Client {
        let stream = UnixStream::connect(socket).expect("connect supervisor");
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("set timeout");
        let writer = stream.try_clone().expect("clone socket");
        let mut client = Client {
            reader: BufReader::new(stream),
            writer,
            events: Vec::new(),
            next_id: 0,
        };
        let hello = client
            .read_line(Instant::now() + TIMEOUT)
            .expect("daemon hello");
        assert_eq!(hello["type"], "daemon_hello");
        client
    }

    /// The next line, or `None` at `deadline`.
    fn read_line(&mut self, deadline: Instant) -> Option<Value> {
        let mut line = String::new();
        loop {
            match self.reader.read_line(&mut line) {
                Ok(0) => panic!("supervisor closed the connection"),
                Ok(_) if line.trim().is_empty() => line.clear(),
                Ok(_) => {
                    let value: Value = serde_json::from_str(line.trim()).expect("parse line");
                    if value["type"] == "session_event" {
                        self.events.push(value.clone());
                    }
                    return Some(value);
                }
                Err(_) if Instant::now() >= deadline => return None,
                Err(_) => {}
            }
        }
    }

    fn send(&mut self, command: &Value) -> Value {
        self.next_id += 1;
        let id = format!("c{}", self.next_id);
        let envelope = json!({
            "type": "command",
            "id": id,
            "protocol": {
                "name": "prime-agent.daemon",
                "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION,
            },
            "command": command,
        });
        let mut line = serde_json::to_string(&envelope).expect("serialize");
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .expect("write command");
        self.writer.flush().expect("flush");
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let line = self
                .read_line(deadline)
                .unwrap_or_else(|| panic!("no response for {command}"));
            if line["id"] == json!(id) {
                assert_eq!(line["success"], true, "{command}: {line}");
                return line;
            }
        }
    }

    /// Creates a session of `cwd` (permissions unset: `"auto"`) and attaches to it; its id.
    fn open(&mut self, cwd: &Path, session_dir: &Path) -> String {
        let created = self.send(&json!({
            "type": "create",
            "config": {
                "cwd": cwd.to_string_lossy(),
                "sessionDir": session_dir.to_string_lossy(),
                "provider": "same",
                "model": "same-model",
            },
        }));
        let session_id = created["data"]["id"]
            .as_str()
            .or_else(|| created["data"]["sessionId"].as_str())
            .expect("session id")
            .to_string();
        self.send(&json!({ "type": "attach", "activeSessionId": session_id }));
        session_id
    }

    /// One prompt on `session_id`, until its `agent_end`.
    fn turn(&mut self, session_id: &str, message: &str) {
        self.events.clear();
        self.send(&json!({ "type": "prompt", "activeSessionId": session_id, "message": message }));
        let deadline = Instant::now() + TIMEOUT;
        let mut seen = 0;
        loop {
            while seen < self.events.len() {
                let frame = &self.events[seen];
                seen += 1;
                if frame["activeSessionId"] == json!(session_id)
                    && frame["event"]["type"] == "agent_end"
                {
                    return;
                }
            }
            assert!(
                self.read_line(deadline).is_some(),
                "the turn did not end; frames: {:#?}",
                self.events
            );
        }
    }
}

/// Waits until no request arrived for `quiet`; returns the bodies.
fn settled_bodies(server: &ModelServer, quiet: Duration) -> Vec<Vec<u8>> {
    let deadline = Instant::now() + TIMEOUT;
    let mut count = server.bodies().len();
    let mut since = Instant::now();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        let now = server.bodies().len();
        if now != count {
            count = now;
            since = Instant::now();
        } else if since.elapsed() >= quiet {
            break;
        }
    }
    server.bodies()
}

/// The session files under `dir` that contain `needle`.
fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle)) {
            found.push(path);
        }
    }
    found
}

const PROBE: &str = "YA31-APPEND-PROBE";

#[test]
fn append_custom_message_does_not_reach_the_live_context() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let server = ModelServer::start();
    let models = json!({ "providers": { "same": {
        "baseUrl": format!("http://127.0.0.1:{}/v1", server.port),
        "apiKey": "append-dummy",
        "api": "openai-responses",
        "models": [{
            "id": "same-model", "name": "Same", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }]
    }}});
    std::fs::write(agent_dir.join("models.json"), models.to_string()).expect("models.json");
    let socket = dir.path().join("ya-append.sock");
    let _supervisor = spawn_supervisor(&socket, &agent_dir);
    let mut client = Client::connect(&socket);

    let session_id = client.open(&cwd, &session_dir);
    // The first turn builds the engine: a cell, then an answer.
    client.turn(&session_id, PROMPT);
    let first = settled_bodies(&server, Duration::from_millis(1500)).len();
    client.send(&json!({
        "type": "append_custom_message",
        "activeSessionId": session_id,
        "message": {
            "customType": "ya31-probe",
            "content": PROBE,
            "display": true,
        },
    }));
    client.turn(&session_id, "Answer again.");
    let bodies = settled_bodies(&server, Duration::from_millis(1500));
    assert!(bodies.len() > first, "the second turn reached the model");
    assert!(
        !files_containing(&session_dir, PROBE).is_empty(),
        "the row is in the session file"
    );
    for (index, body) in bodies[first..].iter().enumerate() {
        assert!(
            !String::from_utf8_lossy(body).contains(PROBE),
            "request {} of the second turn carries the appended row: it reaches the live context",
            first + index + 1
        );
    }
}

/// The prompt of the second turn of the announcement test.
const AGAIN: &str = "Answer again.";
const ROW_TYPE: &str = "ya-resources-changed";

/// The texts of the user items of a Responses request, part by part, in order.
fn user_texts(body: &[u8]) -> Vec<String> {
    let request: Value = serde_json::from_slice(body).expect("a JSON request body");
    let mut texts = Vec::new();
    for item in request["input"].as_array().into_iter().flatten() {
        if item["role"] != "user" {
            continue;
        }
        match &item["content"] {
            Value::String(text) => texts.push(text.clone()),
            Value::Array(parts) => texts.extend(
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str().map(str::to_string)),
            ),
            _ => {}
        }
    }
    texts
}

/// Ticket 31, guard 1 of the option 1 (the daemon path, as the TUI and the extension use it): a
/// skill added between two turns is announced once in the request to the model, right before the
/// prompt (the worker's prefix row, then no second row from the admission), and the session file
/// keeps the announcement row before the prompt's row, where the request had it (guard 2).
#[test]
fn a_skill_change_reaches_the_model_once_before_the_prompt_and_the_file_in_order() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let server = ModelServer::start();
    let models = json!({ "providers": { "same": {
        "baseUrl": format!("http://127.0.0.1:{}/v1", server.port),
        "apiKey": "announce-dummy",
        "api": "openai-responses",
        "models": [{
            "id": "same-model", "name": "Same", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }]
    }}});
    std::fs::write(agent_dir.join("models.json"), models.to_string()).expect("models.json");
    let socket = dir.path().join("ya-announce.sock");
    let _supervisor = spawn_supervisor(&socket, &agent_dir);
    let mut client = Client::connect(&socket);

    let session_id = client.open(&cwd, &session_dir);
    client.turn(&session_id, PROMPT);
    let first = settled_bodies(&server, Duration::from_millis(1500)).len();
    let skill = agent_dir.join("skills").join("ya31-daemon");
    std::fs::create_dir_all(&skill).expect("skill dir");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: ya31-daemon\ndescription: Daemon skill for ticket 31.\n---\n\nRien.\n",
    )
    .expect("SKILL.md");
    client.turn(&session_id, AGAIN);
    let bodies = settled_bodies(&server, Duration::from_millis(1500));
    assert!(bodies.len() > first, "the second turn reached the model");
    for (index, body) in bodies[first..].iter().enumerate() {
        let texts = user_texts(body);
        let announced: Vec<usize> = texts
            .iter()
            .enumerate()
            .filter(|(_, text)| text.starts_with("[skills-changed]"))
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            announced.len(),
            1,
            "request {}: exactly one announcement; user texts: {texts:#?}",
            first + index + 1
        );
        let prompt = texts
            .iter()
            .position(|text| text == AGAIN)
            .expect("the second prompt");
        assert_eq!(
            announced[0] + 1,
            prompt,
            "request {}: the announcement comes right before the prompt; user texts: {texts:#?}",
            first + index + 1
        );
    }
    let files = files_containing(&session_dir, ROW_TYPE);
    assert_eq!(
        files.len(),
        1,
        "one session file holds the announcement: {files:?}"
    );
    let lines: Vec<String> = std::fs::read_to_string(&files[0])
        .expect("the session file")
        .lines()
        .map(str::to_string)
        .collect();
    let row_at: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(ROW_TYPE))
        .map(|(at, _)| at)
        .collect();
    let prompt_at = lines
        .iter()
        .position(|line| line.contains(&format!("\"{AGAIN}\"")))
        .expect("the second prompt's row");
    assert_eq!(row_at.len(), 1, "one announcement row in the session file");
    assert!(
        row_at[0] < prompt_at,
        "the announcement row comes before the prompt's row, as in the request"
    );
}

/// The phase of the comparison with the disk in the compaction trace (`ya_resource_notice.rs`).
const COMPARE_PHASE: &str = "ya.resource_notice.compare";

/// The comparisons with the disk written so far to the trace file.
fn compares(trace: &Path) -> usize {
    std::fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.strip_prefix("compaction-trace: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| line["phase"] == COMPARE_PHASE)
        .count()
}

/// Ticket 35, option A: in the daemon, the worker's comparison (`worker/turn.rs`, before the
/// prompt's row) is the only one of a delivery; the admission takes its token instead of reading
/// the disk again inside the window between the abort check and the run's registration. The
/// first turn is not counted: the session is built by its admission, so the worker has nothing
/// to compare yet.
#[test]
fn a_daemon_delivery_compares_with_the_disk_once() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let server = ModelServer::start();
    let models = json!({ "providers": { "same": {
        "baseUrl": format!("http://127.0.0.1:{}/v1", server.port),
        "apiKey": "announce-dummy",
        "api": "openai-responses",
        "models": [{
            "id": "same-model", "name": "Same", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }]
    }}});
    std::fs::write(agent_dir.join("models.json"), models.to_string()).expect("models.json");
    let trace = dir.path().join("trace.log");
    let socket = dir.path().join("ya-compare.sock");
    let _supervisor = spawn_supervisor_with(&socket, &agent_dir, Some(&trace));
    let mut client = Client::connect(&socket);

    let session_id = client.open(&cwd, &session_dir);
    client.turn(&session_id, PROMPT);
    settled_bodies(&server, Duration::from_millis(1500));
    let mut counted = Vec::new();
    for message in [AGAIN, "And once more."] {
        let before = compares(&trace);
        client.turn(&session_id, message);
        settled_bodies(&server, Duration::from_millis(1500));
        counted.push(compares(&trace) - before);
    }
    assert_eq!(
        counted,
        [1, 1],
        "comparisons with the disk per daemon delivery (turns 2 and 3), trace {}: {}",
        trace.display(),
        std::fs::read_to_string(&trace).unwrap_or_default()
    );
}
