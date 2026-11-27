// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-18, ticket 43, PR 1): `_yus/session/rewind`, the conversation back to before a
//! prompt, to send it again changed (`docs/tickets/43-retour-arriere.md`, §I.5). The real binary
//! serves ACP over stdio on its own sandboxed daemon; the worker runs the real engine on the faux
//! provider (`"engine": "faux"`) and a real kernel, as `ya_acp_steer_e2e.rs`. The conversation the
//! model is sent is checked where it matters (ticket 43, §L): T7, T5b, T5c and T9 run on a loopback
//! `anthropic-messages` model that keeps each request's body ([`ModelServer`]), so they read what
//! the model received; the session file's branch (from its last entry back to the root) is what a
//! resumed worker rebuilds, not what the running turn sends (the worker writes prefix rows that the
//! model never gets, §L).
//! T1. A prompt's `turnStart` carries `rewindTo`, the session's leaf before the prompt.
//! T7 (and T2). A rewind then a prompt: the model's request runs to the point, then the fixed
//!     text (D2), then the new prompt; the abandoned turn stays in the file, off the branch.
//! T3. Refused while a turn runs (`-32603`, Unharnessed's text); the turn runs to its end.
//! T4. An entry off the branch: `-32602`, `data.details` "… is not in the conversation".
//! T5. (a) A leaf before the prompt that is a custom row written outside a turn stays (Unharnessed
//!     stays on `to`; `navigate_tree` would go to its parent). (b) The `[skills-changed]` row of
//!     the abandoned turn reaches the next request again, once. (c) Two rewinds in a row: one
//!     fixed text in the next request.
//! T8. The usage journal: `invalidation: {"cause": "rewind"}` on the next agent-loop call only.
//! T9. A resume after a rewind and a prompt (daemon restarted): the branch resumed is the new one,
//!     and the next request has the fixed text and the announcement once (not doubled by the file).
//! T11. A `[bash-done]` of a command started in the abandoned turn is delivered after the rewind
//!     (K-a), on the new branch.
//! T12. The first prompt of a session: `rewindTo` is the creation's last entry; a rewind there
//!     starts the conversation again.
//! T13, T14. The goal follows the rewind (push 9): the goal of the point is published before the
//!     rewind answers, cleared (T13) or back (T14).
//! T30. (PR 3a, L-b, answer Q-P4) A resume right after a rewind, no prompt in between: the fixed
//!     text is in the file from the rewind on, the child of the point; the replay shows the new
//!     branch with no wake turn, and the next request has the fixed text once.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const META: &str = "ai.primeintellect.prime-agent";
const REWIND: &str = "_yus/session/rewind";
/// The fixed text the model gets after a rewind (D2, validated by Yusuf, ticket 43, §H).
const REWOUND: &str = "[conversation-rewound] The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.";
/// The custom type of the fixed text's row.
const REWOUND_TYPE: &str = "ya-conversation-rewound";
/// The custom type of a skills or MCP announcement row (ticket 31).
const RESOURCES_TYPE: &str = "ya-resources-changed";
/// Unharnessed's refusal while the agent works (`crates/uh-daemon/src/acp/rewind.rs:48-53`).
const WORKING: &str = "The agent is working: stop it before going back.";

/// A cell that marks its start, runs three seconds, then marks its end.
const SLOW_CELL: &str = "import time\nopen('started', 'w').write('x')\ntime.sleep(3)\nopen('ended', 'w').write('x')\nprint('cell done')";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_acp_steer_e2e.rs`.
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

/// One scripted turn: a `python_repl` cell with the call id `id`.
fn cell(id: &str, code: &str) -> Value {
    json!({ "content": [
        { "type": "toolCall", "id": id, "name": "python_repl", "arguments": { "code": code } },
    ] })
}

/// The bridge's `_meta` payload of a `session/update`, or `Null`.
fn meta(frame: &Value) -> &Value {
    &frame["params"]["update"]["_meta"][META]
}

fn is_update(frame: &Value) -> bool {
    frame["method"] == "session/update"
}

fn turn_start(frame: &Value) -> Option<&Value> {
    Some(&meta(frame)["turnStart"]).filter(|start| is_update(frame) && start.is_object())
}

fn origin_is(frame: &Value, origin: &str) -> bool {
    turn_start(frame).is_some_and(|start| start["origin"] == origin)
}

fn phase(frame: &Value) -> Option<&str> {
    meta(frame)["phase"].as_str().filter(|_| is_update(frame))
}

fn turn_of(frame: &Value) -> Option<u64> {
    meta(frame)["promptTurnId"].as_u64()
}

/// The text of a message's or a custom row's content.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// The entries of the session file (the header left out), in file order.
fn entries(file: &Path) -> Vec<Value> {
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] != "session" && entry["id"].is_string())
        .collect()
}

/// The id of the session file's last entry: the leaf, between turns.
fn last_id(file: &Path) -> String {
    entries(file)
        .last()
        .and_then(|entry| entry["id"].as_str())
        .unwrap_or_else(|| panic!("an entry in {}", file.display()))
        .to_string()
}

/// The id in the session file's header (the conversation's id for `session/load`).
fn file_id(file: &Path) -> String {
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    let header: Value =
        serde_json::from_str(text.lines().next().expect("a header line")).expect("a JSON header");
    header["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a session id in {header}"))
        .to_string()
}

/// The branch of the session file: from its last entry back to the root, in order.
fn branch(file: &Path) -> Vec<Value> {
    let all = entries(file);
    let mut path = Vec::new();
    let mut current = all.last().cloned();
    while let Some(entry) = current {
        let parent = entry["parentId"].as_str().map(str::to_string);
        path.push(entry);
        current = parent.and_then(|parent| {
            all.iter()
                .find(|other| other["id"].as_str() == Some(parent.as_str()))
                .cloned()
        });
    }
    path.reverse();
    path
}

/// One entry as the model sees it: `role:text` for a message, `custom:<type>` for a custom row;
/// `None` for the other kinds (model, effort, labels).
fn shown(entry: &Value) -> Option<String> {
    match entry["type"].as_str() {
        Some("message") => Some(format!(
            "{}:{}",
            entry["message"]["role"].as_str().unwrap_or_default(),
            text_of(&entry["message"]["content"])
        )),
        Some("custom_message") => Some(format!(
            "custom:{}",
            entry["customType"].as_str().unwrap_or_default()
        )),
        _ => None,
    }
}

/// The branch as the model sees it.
fn conversation(file: &Path) -> Vec<String> {
    branch(file).iter().filter_map(shown).collect()
}

/// The text of the fixed text's rows on the branch.
fn rewound_texts(file: &Path) -> Vec<String> {
    branch(file)
        .iter()
        .filter(|entry| entry["type"] == "custom_message" && entry["customType"] == REWOUND_TYPE)
        .map(|entry| text_of(&entry["content"]))
        .collect()
}

/// The usage journal next to the session file, line by line (as `ya_acp_resume_e2e.rs`).
fn journal_lines(file: &Path) -> Vec<Value> {
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let journal = file.with_file_name(format!("{stem}.usage.ndjson"));
    std::fs::read_to_string(&journal)
        .unwrap_or_else(|error| panic!("{}: {error}", journal.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Writes a skill under `~/.yusagent/skills` (as `ya_resources_acp_e2e.rs`).
fn write_skill(home: &Path) {
    let dir = home.join(".yusagent/skills/ya43-demo");
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: ya43-demo\ndescription: Demo skill for ticket 43.\n---\n\n# ya43-demo\n\nUsed by the ticket 43 tests.\n",
    )
    .expect("SKILL.md");
}

fn wait_file(path: &Path) {
    let deadline = Instant::now() + TIMEOUT;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One daemon command on the test's socket: its response frame (as `ya_acp_resume_e2e.rs`).
fn daemon_request(socket: &Path, id: &str, command: &Value) -> Value {
    let mut writer =
        pa_types::platform::transport::connect_blocking(socket).expect("daemon socket");
    let reader = writer.try_clone_box().expect("daemon socket clone");
    let _ = reader.set_read_timeout(TIMEOUT);
    let protocol = json!({ "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION });
    let frame = json!({ "type": "command", "id": id, "protocol": protocol, "command": command });
    writeln!(writer, "{frame}").expect("daemon frame");
    writer.flush().expect("daemon flush");
    BufReader::new(reader)
        .lines()
        .map(|line| serde_json::from_str::<Value>(&line.expect("daemon line")).expect("JSON"))
        .find(|frame| frame["id"] == json!(id))
        .unwrap_or_else(|| panic!("the daemon closed without answering {id}"))
}

/// Stops the test's daemon and waits until its socket no longer answers. The connection stays
/// open until the shutdown's response (the supervisor exits after it, as `acp_mode_e2e.rs`).
fn shutdown_daemon(socket: &Path) {
    let answer = daemon_request(socket, "shutdown-t9", &json!({ "type": "shutdown" }));
    let deadline = Instant::now() + Duration::from_secs(30);
    while pa_types::platform::transport::connect_blocking(socket).is_ok() {
        assert!(
            Instant::now() < deadline,
            "the daemon did not stop: {answer}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The model of the request-level tests (ticket 43, §L): a loopback `anthropic-messages` server
/// that keeps each request's JSON body and answers each with one text block, `answer <n>`.
struct ModelServer {
    port: u16,
    bodies: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("local addr").port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let bodies = Arc::clone(&bodies);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut socket, _)) => answer_request(&mut socket, &bodies),
                        Err(_) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
            })
        };
        Self {
            port,
            bodies,
            stop,
            thread: Some(thread),
        }
    }

    /// The bodies of the agent-loop requests (those with tools), in order.
    fn requests(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .filter(|body| {
                body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            })
            .cloned()
            .collect()
    }
}

impl Drop for ModelServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One request: its body kept, a text stream answered (the events of `server.py`).
fn answer_request(socket: &mut TcpStream, bodies: &Mutex<Vec<Value>>) {
    use std::fmt::Write as _;
    let _ = socket.set_nonblocking(false);
    let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
    let mut data = Vec::new();
    let mut buffer = vec![0_u8; 65_536];
    let head_end = loop {
        if let Some(at) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while data.len() < head_end + length {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }
    let body: Value = serde_json::from_slice(&data[head_end..]).unwrap_or(Value::Null);
    let count = {
        let mut bodies = bodies.lock().unwrap();
        bodies.push(body);
        bodies.len()
    };
    let usage = json!({ "input_tokens": 10, "output_tokens": 2 });
    let events = [
        (
            "message_start",
            json!({ "type": "message_start", "message": {
            "id": format!("msg_ya43_{count}"), "type": "message", "role": "assistant",
            "model": "ya43-model", "content": [], "stop_reason": null, "stop_sequence": null,
            "usage": usage } }),
        ),
        (
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
            "content_block": { "type": "text", "text": "" } }),
        ),
        (
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
            "delta": { "type": "text_delta", "text": format!("answer {count}") } }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "message_delta",
            json!({ "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": usage }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];
    let payload = events
        .iter()
        .fold(String::new(), |mut payload, (event, data)| {
            let _ = write!(payload, "event: {event}\ndata: {data}\n\n");
            payload
        });
    let _ = write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
}

/// The value without its `cache_control` marks (they follow the end of each request).
fn without_cache_marks(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(key, item)| (key.clone(), without_cache_marks(item)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_cache_marks).collect()),
        other => other.clone(),
    }
}

/// The messages of a request as JSON text, for counting what the model got.
fn sent(body: &Value) -> String {
    body["messages"].to_string()
}

/// The position of `needle` in what the request sent, for the order of its rows.
fn at(body: &Value, needle: &str) -> usize {
    sent(body)
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not sent: {}", sent(body)))
}

/// The announcement header of a `[skills-changed]` row (ticket 31, §3.2).
const SKILLS_CHANGED: &str = "[skills-changed]";

/// The ACP child on its own home and daemon socket (as `ya_acp_steer_e2e.rs`); every frame it
/// wrote is kept, in order.
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    home: Rc<tempfile::TempDir>,
    socket: PathBuf,
    seen: Vec<Value>,
}

impl Client {
    fn spawn(script: &Value) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join("worker-script.json"), script.to_string()).unwrap();
        Client::launch(Rc::new(home), &[])
    }

    /// On the loopback model `server` (`models.json`, settings): no faux script, the real engine
    /// (a missing script file leaves the worker on it, `worker/config.rs:62-67`).
    fn spawn_model(server: &ModelServer) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        let agent_dir = home.path().join(".yusagent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let model = json!({
            "id": "ya43-model", "name": "ya43", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        });
        let providers = json!({ "providers": { "ya43": {
            "baseUrl": format!("http://127.0.0.1:{}", server.port), "apiKey": "ya43-dummy",
            "api": "anthropic-messages", "models": [model]
        } } });
        std::fs::write(agent_dir.join("models.json"), providers.to_string()).unwrap();
        std::fs::write(
            agent_dir.join("settings.json"),
            json!({ "defaultProvider": "ya43", "defaultModel": "ya43-model", "permissions": "auto" })
                .to_string(),
        )
        .unwrap();
        Client::launch(Rc::new(home), &[])
    }

    /// Another ACP process on the same home and daemon socket: `--resume <file>`.
    fn resume(&self, file: &Path) -> Client {
        Client::launch(
            Rc::clone(&self.home),
            &["--resume", file.to_str().expect("a UTF-8 path")],
        )
    }

    fn launch(home: Rc<tempfile::TempDir>, extra: &[&str]) -> Client {
        let socket = home.path().join("daemon.sock");
        let script_path = home.path().join("worker-script.json");
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&socket)
            .args(extra)
            .env("HOME", home.path())
            .env("PRIME_AGENT_FAUX_SCRIPT", &script_path)
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
                "15000",
            )
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            .current_dir(home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("binary present");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout piped");
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Client {
            child,
            stdin,
            lines,
            next_id: 0,
            home,
            socket,
            seen: Vec::new(),
        }
    }

    /// Closes stdin and waits for the process: the bridge detaches, the session stays resident.
    fn close(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn send(&mut self, frame: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{frame}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: &Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// The index in `seen` of the first frame from `from` on that matches `pred`, reading frames
    /// until one does.
    fn wait_for(&mut self, from: usize, what: &str, pred: impl Fn(&Value) -> bool) -> usize {
        if let Some(found) = self.seen.iter().skip(from).position(&pred) {
            return from + found;
        }
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for {what}; frames: {:?}",
                self.seen
            );
            let line = self.lines.recv_timeout(left).unwrap_or_else(|error| {
                panic!(
                    "no frame ({error}) waiting for {what}; frames: {:?}",
                    self.seen
                )
            });
            let frame: Value = serde_json::from_str(&line).expect("a JSON line");
            let matched = pred(&frame);
            self.seen.push(frame);
            if matched {
                return self.seen.len() - 1;
            }
        }
    }

    /// The response to `id`: its index in `seen`.
    fn wait_response(&mut self, id: u64) -> usize {
        self.wait_for(0, &format!("the response to {id}"), |frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        })
    }

    /// `initialize` then `session/new`: the session id and the session file.
    fn open_session(&mut self) -> (String, PathBuf) {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-rewind-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let at = self.wait_response(new);
        let result = &self.seen[at]["result"];
        let id = result["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {result}"))
            .to_string();
        let path = result["_meta"][META]["saved"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.path: {result}"));
        (id, PathBuf::from(path))
    }

    /// `initialize` then `session/load` of the conversation `id` (a process started with
    /// `--resume`): the frames of the replay, sent before the answer.
    fn load_session(&mut self, id: &str) -> Vec<Value> {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-rewind-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let from = self.seen.len();
        let load = self.request(
            "session/load",
            &json!({ "sessionId": id, "mcpServers": [] }),
        );
        let at = self.wait_response(load);
        assert!(
            self.seen[at].get("result").is_some(),
            "the conversation loads: {}",
            self.seen[at]
        );
        self.seen[from..at].to_vec()
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    /// A prompt, then its `end_turn` answer.
    fn ask(&mut self, session_id: &str, text: &str) {
        let id = self.prompt(session_id, text);
        let at = self.wait_response(id);
        assert_eq!(
            self.seen[at]["result"]["stopReason"], "end_turn",
            "the prompt {text:?} is answered: {}",
            self.seen[at]
        );
    }

    /// A prompt answered `end_turn`: its `turnStart`.
    fn ask_start(&mut self, session_id: &str, text: &str) -> Value {
        let from = self.seen.len();
        let id = self.prompt(session_id, text);
        let at = self.wait_for(from, "the prompt's turnStart", |frame| {
            origin_is(frame, "prompt")
        });
        let start = turn_start(&self.seen[at]).cloned().unwrap();
        let answered = self.wait_response(id);
        assert_eq!(
            self.seen[answered]["result"]["stopReason"], "end_turn",
            "the prompt {text:?} is answered: {}",
            self.seen[answered]
        );
        start
    }

    /// A rewind to `start`'s `rewindTo`, accepted.
    fn rewind_to(&mut self, session_id: &str, start: &Value) {
        let to = start["rewindTo"]
            .as_str()
            .unwrap_or_else(|| panic!("a rewindTo: {start}"))
            .to_string();
        let rewind = self.rewind(session_id, &to);
        self.accepted(rewind);
    }

    fn rewind(&mut self, session_id: &str, to: &str) -> u64 {
        self.request(REWIND, &json!({ "sessionId": session_id, "to": to }))
    }

    /// The response to `id`, which must be `{}`.
    fn accepted(&mut self, id: u64) {
        let at = self.wait_response(id);
        assert_eq!(
            self.seen[at]["result"],
            json!({}),
            "the rewind is accepted: {}",
            self.seen[at]
        );
    }

    /// The error of the response to `id`: its code and its `data.details`.
    fn refused(&mut self, id: u64) -> (Option<i64>, String) {
        let at = self.wait_response(id);
        let error = &self.seen[at]["error"];
        (
            error["code"].as_i64(),
            error["data"]["details"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    /// The daemon's id of the only live session.
    fn daemon_session_id(&self) -> String {
        let list = daemon_request(&self.socket, "live", &json!({ "type": "list" }));
        let sessions = list["data"]["sessions"]
            .as_array()
            .unwrap_or_else(|| panic!("a session list: {list}"));
        assert_eq!(sessions.len(), 1, "one live session: {list}");
        sessions[0]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session id: {list}"))
            .to_string()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The sandboxed supervisor goes with the test.
        if let Ok(mut stream) = pa_types::platform::transport::connect_blocking(&self.socket) {
            let frame = json!({
                "type": "command", "id": "shutdown-test",
                "protocol": { "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION },
                "command": { "type": "shutdown" },
            });
            let _ = writeln!(stream, "{frame}");
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

/// T1. A prompt's `turnStart` carries `rewindTo`, the session's leaf before the prompt.
#[test]
fn a_prompt_turn_start_carries_the_leaf_before_it() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let leaf = last_id(&file);
    let from = client.seen.len();
    let second = client.prompt(&session_id, "second");
    let at = client.wait_for(from, "the second prompt's turnStart", |frame| {
        origin_is(frame, "prompt")
    });
    let start = turn_start(&client.seen[at]).cloned().unwrap();
    let _ = client.wait_response(second);
    assert_eq!(
        start["rewindTo"],
        json!(leaf),
        "rewindTo is the leaf before the prompt: {start}"
    );
}

/// T7 (and T2). A rewind then a prompt: the model's request runs to the point, then the fixed
/// text, then the new prompt; nothing of the abandoned turn.
#[test]
fn a_rewind_then_a_prompt_continues_from_before_the_prompt() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server);
    let (session_id, file) = client.open_session();
    client.ask_start(&session_id, "ALPHA");
    let bravo = client.ask_start(&session_id, "BRAVO");
    client.rewind_to(&session_id, &bravo);
    client.ask_start(&session_id, "CHARLIE");
    let requests = server.requests();
    assert_eq!(requests.len(), 3, "three agent-loop requests");
    let (second, third) = (&requests[1], &requests[2]);
    let point = second["messages"].as_array().expect("messages").len() - 1;
    let before = |body: &Value| {
        without_cache_marks(&Value::Array(
            body["messages"].as_array().expect("messages")[..point].to_vec(),
        ))
    };
    assert_eq!(
        before(third),
        before(second),
        "the request runs to the point as before"
    );
    assert_eq!(
        sent(third).matches(REWOUND).count(),
        1,
        "the fixed text, once: {}",
        sent(third)
    );
    assert!(
        at(third, REWOUND) < at(third, "CHARLIE"),
        "the fixed text before the new prompt"
    );
    assert!(
        !sent(third).contains("BRAVO"),
        "nothing of the abandoned turn: {}",
        sent(third)
    );
    assert_eq!(
        rewound_texts(&file),
        vec![REWOUND.to_string()],
        "one row in the file"
    );
    assert!(
        entries(&file)
            .iter()
            .filter_map(shown)
            .any(|row| row == "user:BRAVO"),
        "the abandoned turn stays in the file, off the branch"
    );
}

/// T3. Refused while a turn runs; the turn runs to its end.
#[test]
fn a_rewind_is_refused_while_a_turn_runs() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        cell("call-1", SLOW_CELL),
        { "text": "cell answered" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    let prompt = client.prompt(&session_id, "run the slow cell");
    wait_file(&client.path("started"));
    let rewind = client.rewind(&session_id, &to);
    let (code, details) = client.refused(rewind);
    assert_eq!(code, Some(-32603), "{details}");
    assert_eq!(details, WORKING);
    let answered = client.wait_response(prompt);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "end_turn",
        "the turn was not stopped: {}",
        client.seen[answered]
    );
    assert!(client.path("ended").exists(), "the cell ran to its end");
}

/// T4. An entry off the branch is refused with `-32602`.
#[test]
fn a_rewind_to_an_entry_off_the_branch_is_invalid() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
    ] }));
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "first");
    let rewind = client.rewind(&session_id, "no-such-entry");
    let (code, details) = client.refused(rewind);
    assert_eq!(code, Some(-32602), "{details}");
    assert!(
        details.contains("no-such-entry is not in the conversation"),
        "{details}"
    );
}

/// T5 (a). A custom row written outside a turn, the leaf before the prompt, stays on the branch.
#[test]
fn a_custom_row_before_the_prompt_stays_after_the_rewind() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    // As `/refine` does after a turn (`session_custom.rs:286`): a custom row outside a turn.
    let daemon_session = client.daemon_session_id();
    let appended = daemon_request(
        &client.socket,
        "note",
        &json!({
            "type": "append_custom_message",
            "activeSessionId": daemon_session,
            "message": { "customType": "ya43-note", "content": "a note", "display": true },
        }),
    );
    assert_eq!(appended["success"], true, "the row is written: {appended}");
    let to = last_id(&file);
    assert_eq!(
        branch(&file).last().and_then(shown),
        Some("custom:ya43-note".to_string()),
        "the leaf before the prompt is the custom row"
    );
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![
            "user:first",
            "assistant:one",
            "custom:ya43-note",
            custom.as_str(),
            "user:second, changed",
            "assistant:three",
        ],
        "the custom row stays on the branch"
    );
}

/// T5 (b). The `[skills-changed]` row of the abandoned turn reaches the next request again, once,
/// before the fixed text and the new prompt.
#[test]
fn a_skills_announcement_of_the_abandoned_turn_comes_back_once() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server);
    let (session_id, file) = client.open_session();
    client.ask_start(&session_id, "ALPHA");
    write_skill(client.home.path());
    let bravo = client.ask_start(&session_id, "BRAVO");
    assert_eq!(
        sent(&server.requests()[1]).matches(SKILLS_CHANGED).count(),
        1,
        "precondition: the skill is announced with the second prompt"
    );
    client.rewind_to(&session_id, &bravo);
    client.ask_start(&session_id, "CHARLIE");
    let third = &server.requests()[2];
    assert_eq!(
        sent(third).matches(SKILLS_CHANGED).count(),
        1,
        "the announcement reaches the next request again, once: {}",
        sent(third)
    );
    assert_eq!(sent(third).matches(REWOUND).count(), 1, "{}", sent(third));
    assert!(
        at(third, SKILLS_CHANGED) < at(third, REWOUND) && at(third, REWOUND) < at(third, "CHARLIE"),
        "the announcement, the fixed text, then the new prompt: {}",
        sent(third)
    );
    let resources = format!("custom:{RESOURCES_TYPE}");
    assert_eq!(
        conversation(&file)
            .iter()
            .filter(|row| **row == resources)
            .count(),
        1,
        "one announcement row on the branch"
    );
}

/// T5 (c). Two rewinds in a row, after a turn that already had a fixed text: one fixed text in
/// the next request.
#[test]
fn rewinds_in_a_row_leave_one_fixed_text() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server);
    let (session_id, file) = client.open_session();
    client.ask_start(&session_id, "ALPHA");
    let bravo = client.ask_start(&session_id, "BRAVO");
    client.rewind_to(&session_id, &bravo);
    let charlie = client.ask_start(&session_id, "CHARLIE");
    assert_eq!(
        sent(&server.requests()[2]).matches(REWOUND).count(),
        1,
        "the first rewind's fixed text reaches its request"
    );
    client.rewind_to(&session_id, &charlie);
    client.rewind_to(&session_id, &charlie);
    client.ask_start(&session_id, "DELTA");
    let fourth = &server.requests()[3];
    assert_eq!(
        sent(fourth).matches(REWOUND).count(),
        1,
        "one fixed text: {}",
        sent(fourth)
    );
    assert!(
        !sent(fourth).contains("CHARLIE") && !sent(fourth).contains("BRAVO"),
        "nothing of the abandoned turns: {}",
        sent(fourth)
    );
    assert_eq!(rewound_texts(&file).len(), 1, "{:?}", conversation(&file));
}

/// T8. The usage journal: `invalidation: {"cause": "rewind"}` on the next agent-loop call only.
#[test]
fn the_call_after_a_rewind_journals_the_invalidation() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
        { "text": "four" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    client.ask(&session_id, "third");
    let lines = journal_lines(&file);
    let mains: Vec<&Value> = lines.iter().filter(|line| line["kind"] == "main").collect();
    assert_eq!(mains.len(), 4, "{lines:?}");
    assert!(mains[0].get("invalidation").is_none(), "{lines:?}");
    assert!(mains[1].get("invalidation").is_none(), "{lines:?}");
    assert_eq!(
        mains[2]["invalidation"],
        json!({ "cause": "rewind" }),
        "{lines:?}"
    );
    assert!(mains[3].get("invalidation").is_none(), "{lines:?}");
}

/// T9. A resume after a rewind and a prompt, the daemon restarted: the branch resumed is the new
/// one, and the next request has the fixed text and the announcement once (the resumed worker
/// rebuilds them from the file; the mailbox does not add them again).
#[test]
fn a_resume_after_a_rewind_keeps_the_new_branch() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server);
    let (session_id, file) = client.open_session();
    client.ask_start(&session_id, "ALPHA");
    write_skill(client.home.path());
    let bravo = client.ask_start(&session_id, "BRAVO");
    client.rewind_to(&session_id, &bravo);
    client.ask_start(&session_id, "CHARLIE");
    let third = &server.requests()[2];
    assert_eq!(
        (
            sent(third).matches(REWOUND).count(),
            sent(third).matches(SKILLS_CHANGED).count()
        ),
        (1, 1),
        "before the restart, the fixed text and the announcement once: {}",
        sent(third)
    );
    client.close();
    shutdown_daemon(&client.socket);
    let mut resumed = client.resume(&file);
    let (resumed_id, resumed_file) = resumed.open_session();
    assert_eq!(resumed_file, file, "the same conversation");
    resumed.ask_start(&resumed_id, "ECHO");
    let requests = server.requests();
    let fourth = requests.last().expect("the resumed request");
    assert_eq!(
        (
            sent(fourth).matches(REWOUND).count(),
            sent(fourth).matches(SKILLS_CHANGED).count()
        ),
        (1, 1),
        "after the restart, once each, not doubled: {}",
        sent(fourth)
    );
    assert!(
        at(fourth, "CHARLIE") < at(fourth, "ECHO") && !sent(fourth).contains("BRAVO"),
        "the resumed prompt follows the new branch: {}",
        sent(fourth)
    );
}

/// T30 (PR 3a, L-b, answer Q-P4). A resume right after a rewind, the daemon restarted with no
/// prompt in between: the fixed text is written at the rewind, the child of the point, so the
/// replay shows the new branch with no wake turn and the next request has the fixed text once.
#[test]
fn a_resume_right_after_a_rewind_keeps_the_new_branch_and_the_fixed_text() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server);
    let (session_id, file) = client.open_session();
    client.ask_start(&session_id, "ALPHA");
    let bravo = client.ask_start(&session_id, "BRAVO");
    client.rewind_to(&session_id, &bravo);
    let point = bravo["rewindTo"].clone();
    let last = entries(&file).last().cloned().expect("an entry");
    assert!(
        last["type"] == "custom_message"
            && last["customType"] == REWOUND_TYPE
            && last["parentId"] == point,
        "the fixed text is in the file from the rewind on, the child of {point}: {last}"
    );
    client.close();
    shutdown_daemon(&client.socket);
    let id = file_id(&file);
    let mut resumed = client.resume(&file);
    let replay = resumed.load_session(&id);
    let replayed = Value::Array(replay.clone()).to_string();
    assert!(
        !replayed.contains("BRAVO"),
        "the replay shows the new branch: {replayed}"
    );
    assert!(
        !replay.iter().any(|frame| origin_is(frame, "wake")),
        "no wake turn in the replay: {replayed}"
    );
    resumed.ask_start(&id, "CHARLIE");
    let request = server
        .requests()
        .last()
        .cloned()
        .expect("the resumed request");
    assert_eq!(
        sent(&request).matches(REWOUND).count(),
        1,
        "the fixed text, once: {}",
        sent(&request)
    );
    assert!(
        !sent(&request).contains("BRAVO") && sent(&request).contains("ALPHA"),
        "the request follows the new branch: {}",
        sent(&request)
    );
}

/// T11. A `[bash-done]` of a command started in the abandoned turn is delivered after the rewind.
#[test]
fn a_bash_notice_of_the_abandoned_turn_comes_after_the_rewind() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        cell("call-launch", "h = bash('sleep 3; echo trois')\nprint('launched')"),
        { "text": "launched" },
        { "text": "noticed" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "launch a command");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    let from = client.seen.len();
    let wake = client.wait_for(from, "the wake's turnStart", |frame| {
        origin_is(frame, "wake")
    });
    let turn = turn_of(&client.seen[wake]).expect("a turn id");
    let _ = client.wait_for(wake, "the wake's boundary", |frame| {
        phase(frame) == Some("responseBoundary") && turn_of(frame) == Some(turn)
    });
    let rows = conversation(&file);
    assert!(
        rows.iter().any(|row| row == "custom:async_bash_completion"),
        "the notice is on the new branch: {rows:?}"
    );
    assert_eq!(
        rows.last().map(String::as_str),
        Some("assistant:noticed"),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row == "user:launch a command"),
        "the abandoned prompt is off the branch: {rows:?}"
    );
}

/// T12. The first prompt of a session: its `rewindTo` is the last entry the worker writes at the
/// creation of a session with a file (`session_state` `active`, after the creation prefix,
/// `crates/pa-daemon/src/worker/create.rs:444-452`), the parent of the prompt's row; a rewind there
/// starts the conversation again, with the fixed text only.
#[test]
fn a_rewind_to_before_the_first_prompt_starts_the_conversation_again() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
    ] }));
    let (session_id, file) = client.open_session();
    let first = client.prompt(&session_id, "first");
    let at = client.wait_for(0, "the first prompt's turnStart", |frame| {
        origin_is(frame, "prompt")
    });
    let start = turn_start(&client.seen[at]).cloned().unwrap();
    let _ = client.wait_response(first);
    let all = entries(&file);
    let row = all
        .iter()
        .find(|entry| shown(entry).as_deref() == Some("user:first"))
        .unwrap_or_else(|| panic!("the first prompt's row: {all:?}"));
    let parent = row["parentId"].as_str().expect("a parent").to_string();
    let parent_type = all
        .iter()
        .find(|entry| entry["id"].as_str() == Some(parent.as_str()))
        .map(|entry| entry["type"].clone());
    assert_eq!(
        start["rewindTo"],
        json!(parent),
        "rewindTo is the parent of the first prompt's row: {start}"
    );
    assert_eq!(
        parent_type,
        Some(json!("session_state")),
        "the last entry of the creation"
    );
    let rewind = client.rewind(&session_id, &parent);
    client.accepted(rewind);
    client.ask(&session_id, "first, changed");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![custom.as_str(), "user:first, changed", "assistant:two"],
        "the conversation starts again"
    );
}

/// The goal of a bridge `_meta` frame (`session_info_update`, from the worker's `goal_update`).
fn goal_of(frame: &Value) -> Option<&Value> {
    Some(&meta(frame)["goal"]).filter(|goal| is_update(frame) && goal.is_object())
}

/// The last goal frame between `from` and `to` in `seen`.
fn last_goal(seen: &[Value], from: usize, to: usize) -> Option<Value> {
    seen[from..to].iter().rev().find_map(goal_of).cloned()
}

/// T13. A goal set in the abandoned turn: the rewind publishes the goal of the point, the empty
/// state (`pa_types::goal::empty_goal_state`: `status` `idle`, no objective), before its answer, as
/// `navigate_tree` does (`worker/commands.rs:199-215`).
#[test]
fn a_goal_of_the_abandoned_turn_is_cleared_before_the_rewind_answers() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        "one", "GOAL-PROGRESS", "WRAP-UP",
    ] }));
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "first");
    let from = client.seen.len();
    let goal = client.ask_start(
        &session_id,
        "/goal --budget 5 reply with exactly: GOAL-PROGRESS",
    );
    let shown = last_goal(&client.seen, from, client.seen.len());
    assert_eq!(
        shown.as_ref().map(|goal| goal["status"].clone()),
        Some(json!("budget_limited")),
        "precondition: the goal is shown after its turn: {shown:?}"
    );
    let from = client.seen.len();
    let to = goal["rewindTo"].as_str().expect("a rewindTo").to_string();
    let rewind = client.rewind(&session_id, &to);
    let answered = client.wait_response(rewind);
    assert_eq!(
        client.seen[answered]["result"],
        json!({}),
        "{}",
        client.seen[answered]
    );
    let published = last_goal(&client.seen, from, answered);
    assert_eq!(
        published
            .as_ref()
            .map(|goal| (goal["status"].clone(), goal["objective"].clone())),
        Some((json!("idle"), Value::Null)),
        "the goal frame clears the goal before the rewind answers: {published:?}"
    );
}

/// T14. The other way: the goal of the point (`budget_limited`) was cleared in the abandoned turn
/// (`/goal clear`); the rewind publishes it again before its answer.
#[test]
fn a_goal_cleared_in_the_abandoned_turn_comes_back_before_the_rewind_answers() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        "GOAL-PROGRESS", "WRAP-UP", "spare one", "spare two",
    ] }));
    let (session_id, _) = client.open_session();
    client.ask_start(
        &session_id,
        "/goal --budget 5 reply with exactly: GOAL-PROGRESS",
    );
    let from = client.seen.len();
    let clear = client.ask_start(&session_id, "/goal clear");
    let cleared = last_goal(&client.seen, from, client.seen.len());
    assert_eq!(
        cleared.as_ref().map(|goal| goal["status"].clone()),
        Some(json!("idle")),
        "precondition: `/goal clear` clears the shown goal: {cleared:?}"
    );
    let from = client.seen.len();
    let to = clear["rewindTo"].as_str().expect("a rewindTo").to_string();
    let rewind = client.rewind(&session_id, &to);
    let answered = client.wait_response(rewind);
    assert_eq!(
        client.seen[answered]["result"],
        json!({}),
        "{}",
        client.seen[answered]
    );
    let published = last_goal(&client.seen, from, answered);
    assert_eq!(
        published
            .as_ref()
            .map(|goal| (goal["status"].clone(), goal["objective"].clone())),
        Some((
            json!("budget_limited"),
            json!("reply with exactly: GOAL-PROGRESS")
        )),
        "the goal of the point comes back before the rewind answers: {published:?}"
    );
}
