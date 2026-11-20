// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-11, ticket 20, PR 3, decision T9): the requests sent to the model are the same in
//! `"manual"` and in `"auto"`. Real supervisor and worker (`pa-daemon`), a real kernel, and a
//! loopback server that plays the model (an `openai-responses` provider in `models.json`, as
//! `crates/pa-cli/tests/ya_permissions_e2e.rs`) and records every request body.
//!
//! Two sessions of the same daemon play the same turn (one cell, then an answer), one after the
//! other, in the same project:
//! - manual (the default: `PRIME_AGENT_TEST_PERMISSIONS`, which `.cargo/config.toml` sets to
//!   `auto` for the upstream tests, is removed): a client that answers (as the TUI) allows the
//!   cell;
//! - auto: the same client switches the session to `"auto"` first (`/permissions auto` in the
//!   TUI, the `permissions` selector in ACP); no request is asked.
//!
//! The bodies are compared whole, `prompt_cache_key` aside (it names the session). The asking
//! client is not the TUI itself: what reaches the model depends only on the worker's engine, which
//! every client shares. Listed in `docs/fork-changes.md`.

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

// The supervisor is killed and waited for by `Supervisor`'s drop (as `abort_kernel_e2e.rs`).
#[allow(clippy::zombie_processes)]
fn spawn_supervisor(socket: &Path, agent_dir: &Path) -> Supervisor {
    let child = Command::new(env!("CARGO_BIN_EXE_pa-daemon"))
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

    /// One turn of a new session, `auto` or manual; returns the number of permission requests
    /// the client allowed.
    fn play_turn(&mut self, cwd: &Path, session_dir: &Path, auto: bool) -> usize {
        let created = self.send(&json!({
            "type": "create",
            "config": {
                "cwd": cwd.to_string_lossy(),
                "sessionDir": session_dir.to_string_lossy(),
                "provider": "same",
                "model": "same-model",
                "yaPermissionAsker": true,
            },
        }));
        let session_id = created["data"]["id"]
            .as_str()
            .or_else(|| created["data"]["sessionId"].as_str())
            .expect("session id")
            .to_string();
        // The client answers, as the TUI does.
        self.send(&json!({
            "type": "attach",
            "activeSessionId": session_id,
            "yaPermissionAsker": true,
        }));
        if auto {
            let switched = self.send(&json!({
                "type": "ya_permissions",
                "activeSessionId": session_id,
                "action": "set_mode",
                "mode": "auto",
            }));
            assert_eq!(switched["data"]["mode"], "auto", "{switched}");
        }
        self.events.clear();
        self.send(&json!({ "type": "prompt", "activeSessionId": session_id, "message": PROMPT }));
        let deadline = Instant::now() + TIMEOUT;
        let mut allowed = 0;
        // Every frame read, during a response wait too, is looked at once, in order.
        let mut seen = 0;
        loop {
            while seen < self.events.len() {
                let frame = self.events[seen].clone();
                seen += 1;
                if frame["activeSessionId"] != json!(session_id) {
                    continue;
                }
                let event = &frame["event"];
                if event["type"] == "ya_permission_request" {
                    self.send(&json!({
                        "type": "ya_permissions",
                        "activeSessionId": session_id,
                        "action": "respond",
                        "requestId": event["requestId"],
                        "outcome": "allow",
                    }));
                    allowed += 1;
                }
                if event["type"] == "agent_end" {
                    return allowed;
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

/// A request body without `prompt_cache_key` (it names the session).
fn comparable(body: &[u8]) -> Value {
    let mut value: Value = serde_json::from_slice(body).expect("a JSON request body");
    if let Some(object) = value.as_object_mut() {
        object.remove("prompt_cache_key");
    }
    value
}

/// The first line where two pretty-printed values differ.
fn first_difference(manual: &Value, auto: &Value) -> String {
    let manual = serde_json::to_string_pretty(manual).unwrap_or_default();
    let auto = serde_json::to_string_pretty(auto).unwrap_or_default();
    manual
        .lines()
        .zip(auto.lines())
        .enumerate()
        .find(|(_, (left, right))| left != right)
        .map_or_else(
            || "one body is a prefix of the other".to_string(),
            |(line, (left, right))| format!("line {line}: manual {left:?}, auto {right:?}"),
        )
}

#[test]
fn manual_and_auto_send_the_same_requests() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let server = ModelServer::start();
    let models = json!({ "providers": { "same": {
        "baseUrl": format!("http://127.0.0.1:{}/v1", server.port),
        "apiKey": "permissions-dummy",
        "api": "openai-responses",
        "models": [{
            "id": "same-model", "name": "Same", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }]
    }}});
    std::fs::write(agent_dir.join("models.json"), models.to_string()).expect("models.json");
    let socket = dir.path().join("ya-same.sock");
    let _supervisor = spawn_supervisor(&socket, &agent_dir);
    let mut client = Client::connect(&socket);

    let allowed = client.play_turn(&cwd, &session_dir, false);
    assert_eq!(allowed, 1, "manual: the cell is asked once");
    let manual = settled_bodies(&server, Duration::from_millis(1500));

    let allowed = client.play_turn(&cwd, &session_dir, true);
    assert_eq!(allowed, 0, "auto: no request");
    let all = settled_bodies(&server, Duration::from_millis(1500));
    let auto = &all[manual.len()..];

    assert_eq!(manual.len(), 2, "manual: the cell, then the answer");
    assert_eq!(auto.len(), manual.len(), "the same number of requests");
    for (index, (manual, auto)) in manual.iter().zip(auto).enumerate() {
        let (manual, auto) = (comparable(manual), comparable(auto));
        assert!(
            manual == auto,
            "request {index} differs between manual and auto: {}",
            first_difference(&manual, &auto)
        );
    }
    // The second request carries the cell's result: the cell ran in both modes.
    assert!(
        String::from_utf8_lossy(&all[1]).contains("42"),
        "the cell's output reached the model"
    );
}
