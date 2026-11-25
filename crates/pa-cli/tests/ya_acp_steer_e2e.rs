// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-19, ticket 39): `_yus/session/steer`, a message into the running prompt turn
//! (`docs/tickets/39-message-pendant-un-tour.md`, §17 and §18, path B). The real binary serves ACP over
//! stdio on its own sandboxed daemon; the worker runs the real engine on the faux provider
//! (`"engine": "faux"`) and a real kernel, permissions in `"auto"` (as
//! `ya_acp_wake_turn_e2e.rs`). The session file is the conversation the model is sent.
//! E1. A message sent during a cell joins the prompt's turn: the cell runs to its end, the
//!     message row follows the cell's result, `turnStart { origin: "steer" }` under the prompt's
//!     turn id (no `rewindTo`), one response, after the message's answer.
//! E2. Two messages go in one model turn: two rows in a row, one answer.
//! E3. A message is delivered once: one row, no turn of its own; the prompt's row is written
//!     once (the `run_once.rs` gate); after `session/load` in another process, the replay shows
//!     the message once, at its place (after the cell's result, before its answer).
//! E4. A message sent as soon as the turn starts never goes before the prompt's row.
//! E5. Refused with `-32603` for a stale turn, after the turn, and during a wake (D6); with
//!     `-32602` for a session command and an image.
//! E6. Stop with a message accepted and not taken yet: the message never runs, the next prompt
//!     runs without it (the run's settle dropped it from the agent's queue).
//! E7. A failed turn with a message accepted and not taken: no row of it in the failed turn; the
//!     client sends it again as a prompt (as the extension does): one row, one answer.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const META: &str = "ai.primeintellect.prime-agent";
const STEER: &str = "_yus/session/steer";

/// A cell that marks its start, runs three seconds, then marks its end.
const SLOW_CELL: &str = "import time\nopen('started', 'w').write('x')\ntime.sleep(3)\nopen('ended', 'w').write('x')\nprint('cell done')";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_acp_wake_turn_e2e.rs`.
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

fn turn_of(frame: &Value) -> Option<u64> {
    meta(frame)["promptTurnId"].as_u64()
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

/// An answer chunk of turn `turn`.
fn answer_of(frame: &Value, turn: u64) -> bool {
    is_update(frame)
        && frame["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
        && turn_of(frame) == Some(turn)
}

/// An update of kind `kind` whose text block contains `text`.
fn shows(frame: &Value, kind: &str, text: &str) -> bool {
    frame["params"]["update"]["sessionUpdate"] == kind
        && frame["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|shown| shown.contains(text))
}

/// The JSON-RPC error code of a response frame.
fn error_code(frame: &Value) -> Option<i64> {
    frame["error"]["code"].as_i64()
}

/// One message of the session file: its role and its text.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    role: String,
    text: String,
}

/// The messages of the session file, in order.
fn rows(file: &Path) -> Vec<Row> {
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] == "message")
        .map(|entry| {
            let message = &entry["message"];
            let text = match &message["content"] {
                Value::String(text) => text.clone(),
                Value::Array(blocks) => blocks
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            };
            Row {
                role: message["role"].as_str().unwrap_or_default().to_string(),
                text,
            }
        })
        .collect()
}

/// The position of the user row `text` in `rows`.
fn user_row(rows: &[Row], text: &str) -> Option<usize> {
    rows.iter()
        .position(|row| row.role == "user" && row.text == text)
}

/// The ACP child on its own home and daemon socket (as `ya_acp_wake_turn_e2e.rs`); every frame
/// it wrote is kept, in order.
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
        Client::spawn_with(script, None)
    }

    /// With the agent's `settings.json` (`~/.yusagent`).
    fn spawn_with(script: &Value, settings: Option<&Value>) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join("worker-script.json"), script.to_string()).unwrap();
        if let Some(settings) = settings {
            let dir = home.path().join(".yusagent");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
        }
        Client::launch(Rc::new(home), &[])
    }

    /// Another ACP process on the same home and daemon: `--resume <file>`.
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

    /// Closes stdin and waits for the process: the bridge detaches, the session stays resident
    /// (as `ya_acp_resume_e2e.rs`).
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

    fn notify(&mut self, method: &str, params: &Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
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

    /// Reads every frame that arrives within `span`.
    fn drain_for(&mut self, span: Duration) {
        let deadline = Instant::now() + span;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(line) => self
                    .seen
                    .push(serde_json::from_str(&line).expect("a JSON line")),
                Err(_) => return,
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
        let (id, _, path) = self.open_saved_session();
        (id, path)
    }

    fn initialize(&mut self) {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-steer-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
    }

    /// `initialize` then `session/new`: the session id, the file's id and the file.
    fn open_saved_session(&mut self) -> (String, String, PathBuf) {
        self.initialize();
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let at = self.wait_response(new);
        let result = &self.seen[at]["result"];
        let id = result["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {result}"))
            .to_string();
        let saved = &result["_meta"][META]["saved"];
        let file_id = saved["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.sessionId: {result}"))
            .to_string();
        let path = saved["path"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.path: {result}"));
        (id, file_id, PathBuf::from(path))
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    fn steer_blocks(&mut self, session_id: &str, turn: u64, prompt: &Value) -> u64 {
        self.request(
            STEER,
            &json!({ "sessionId": session_id, "prompt": prompt, "turn": turn }),
        )
    }

    fn steer(&mut self, session_id: &str, turn: u64, text: &str) -> u64 {
        self.steer_blocks(session_id, turn, &json!([{ "type": "text", "text": text }]))
    }

    /// The response to `id`, which must be `{}`.
    fn accepted(&mut self, id: u64) {
        let at = self.wait_response(id);
        assert_eq!(
            self.seen[at]["result"],
            json!({}),
            "the message is accepted: {}",
            self.seen[at]
        );
    }

    /// The error code of the response to `id`.
    fn refused(&mut self, id: u64) -> Option<i64> {
        let at = self.wait_response(id);
        error_code(&self.seen[at])
    }

    /// The prompt turn's `turnStart` from `from` on: its turn id.
    fn prompt_turn(&mut self, from: usize) -> u64 {
        let at = self.wait_for(from, "the prompt's turnStart", |frame| {
            origin_is(frame, "prompt")
        });
        turn_of(&self.seen[at]).expect("a turn id")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    fn count(&self, pred: impl Fn(&Value) -> bool) -> usize {
        self.seen.iter().filter(|frame| pred(frame)).count()
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

/// Polls until `path` exists; returns its content.
fn wait_file(path: &Path) -> String {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// E1. A message sent during a cell joins the prompt's turn.
#[test]
fn a_steer_joins_the_running_prompt_turn() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell("call-1", SLOW_CELL),
        { "text": "steer answered" },
    ] }));
    let (session_id, file) = client.open_session();
    let prompt = client.prompt(&session_id, "run the slow cell");
    let turn = client.prompt_turn(0);
    let _ = wait_file(&client.path("started"));
    let steer = client.steer(&session_id, turn, "also say hello");
    client.accepted(steer);
    let answered = client.wait_response(prompt);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "end_turn",
        "{}",
        client.seen[answered]
    );
    assert!(client.path("ended").exists(), "the cell ran to its end");
    let joined = client
        .seen
        .iter()
        .position(|frame| origin_is(frame, "steer"))
        .unwrap_or_else(|| panic!("no turnStart origin steer; frames: {:?}", client.seen));
    assert_eq!(
        turn_of(&client.seen[joined]),
        Some(turn),
        "the prompt's turn"
    );
    assert!(
        turn_start(&client.seen[joined]).unwrap()["rewindTo"].is_null(),
        "no rewindTo (D3): {}",
        client.seen[joined]
    );
    let chunk = client
        .seen
        .iter()
        .position(|frame| answer_of(frame, turn))
        .unwrap_or_else(|| panic!("the message's answer; frames: {:?}", client.seen));
    assert!(
        joined < chunk,
        "turnStart comes before the message's answer"
    );
    let boundary = client
        .seen
        .iter()
        .position(|frame| phase(frame) == Some("responseBoundary") && turn_of(frame) == Some(turn))
        .expect("the prompt's boundary");
    assert!(
        chunk < boundary && boundary < answered,
        "the turn ends after the message's answer"
    );
    assert_eq!(
        client.count(|frame| origin_is(frame, "prompt")),
        1,
        "no turn of its own"
    );
    let rows = rows(&file);
    let result = rows
        .iter()
        .position(|row| row.role == "toolResult")
        .unwrap_or_else(|| panic!("the cell's result: {rows:?}"));
    let message =
        user_row(&rows, "also say hello").unwrap_or_else(|| panic!("the message's row: {rows:?}"));
    assert!(
        result < message,
        "the message follows the cell's result: {rows:?}"
    );
    assert_eq!(
        rows.last()
            .map(|row| (row.role.as_str(), row.text.as_str())),
        Some(("assistant", "steer answered")),
        "{rows:?}"
    );
}

/// E2. Two messages sent during a cell go in one model turn.
#[test]
fn two_steers_go_in_one_model_turn() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell("call-1", SLOW_CELL),
        { "text": "both answered" },
    ] }));
    let (session_id, file) = client.open_session();
    let prompt = client.prompt(&session_id, "run the slow cell");
    let turn = client.prompt_turn(0);
    let _ = wait_file(&client.path("started"));
    let one = client.steer(&session_id, turn, "first note");
    let two = client.steer(&session_id, turn, "second note");
    client.accepted(one);
    client.accepted(two);
    let _ = client.wait_response(prompt);
    let rows = rows(&file);
    let first = user_row(&rows, "first note").unwrap_or_else(|| panic!("first: {rows:?}"));
    assert_eq!(
        rows.get(first + 1)
            .map(|row| (row.role.as_str(), row.text.as_str())),
        Some(("user", "second note")),
        "the two messages in a row: {rows:?}"
    );
    assert_eq!(
        rows.get(first + 2)
            .map(|row| (row.role.as_str(), row.text.as_str())),
        Some(("assistant", "both answered")),
        "one answer for both: {rows:?}"
    );
    assert_eq!(
        client.count(|frame| origin_is(frame, "steer") && turn_of(frame) == Some(turn)),
        2,
        "one turnStart per message"
    );
}

/// E3. A message is delivered once, the turn's or the next prompt's; the prompt's row is written
/// once; after `session/load` in another process, the replay shows the message once, at its place.
#[test]
fn a_steer_is_delivered_once() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell("call-1", SLOW_CELL),
        { "text": "steer answered" },
        { "text": "second answered" },
    ] }));
    let (session_id, file_id, file) = client.open_saved_session();
    let prompt = client.prompt(&session_id, "run the slow cell");
    let turn = client.prompt_turn(0);
    let _ = wait_file(&client.path("started"));
    let steer = client.steer(&session_id, turn, "only once");
    client.accepted(steer);
    let answered = client.wait_response(prompt);
    let second = client.prompt(&session_id, "the next prompt");
    let _ = client.wait_response(second);
    client.drain_for(Duration::from_secs(1));
    let rows = rows(&file);
    let count = |text: &str| {
        rows.iter()
            .filter(|row| row.role == "user" && row.text == text)
            .count()
    };
    assert_eq!(count("only once"), 1, "one row for the message: {rows:?}");
    assert_eq!(
        count("run the slow cell"),
        1,
        "the prompt's row is written once: {rows:?}"
    );
    assert_eq!(
        client.count(|frame| origin_is(frame, "prompt")),
        2,
        "the two prompts only"
    );
    assert_eq!(client.count(|frame| origin_is(frame, "steer")), 1);
    assert!(
        client.seen[answered..]
            .iter()
            .all(|frame| !origin_is(frame, "steer")),
        "nothing of the message after its turn"
    );

    // The resume: another process loads the file and replays it.
    client.close();
    let mut resumed = client.resume(&file);
    resumed.initialize();
    let from = resumed.seen.len();
    let load = resumed.request(
        "session/load",
        &json!({ "sessionId": file_id, "mcpServers": [] }),
    );
    let loaded = resumed.wait_response(load);
    assert!(
        resumed.seen[loaded].get("result").is_some(),
        "{}",
        resumed.seen[loaded]
    );
    let replay: Vec<&Value> = resumed.seen[from..loaded]
        .iter()
        .filter(|frame| is_update(frame))
        .collect();

    let messages: Vec<usize> = replay
        .iter()
        .enumerate()
        .filter(|(_, frame)| shows(frame, "user_message_chunk", "only once"))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(messages.len(), 1, "the message replays once: {replay:?}");
    let result = replay
        .iter()
        .position(|frame| {
            frame["params"]["update"]["sessionUpdate"] == "tool_call_update"
                && frame["params"]["update"]["toolCallId"] == "call-1"
        })
        .unwrap_or_else(|| panic!("the cell's result: {replay:?}"));
    let answer = replay
        .iter()
        .position(|frame| shows(frame, "agent_message_chunk", "steer answered"))
        .unwrap_or_else(|| panic!("the message's answer: {replay:?}"));
    assert!(
        result < messages[0] && messages[0] < answer,
        "at its place, after the cell's result and before its answer: {replay:?}"
    );
    assert_eq!(
        replay
            .iter()
            .filter(|frame| shows(frame, "user_message_chunk", "run the slow cell"))
            .count(),
        1,
        "the prompt replays once"
    );
}

/// E4. A message sent as soon as the turn starts never goes before the prompt's row.
#[test]
fn a_steer_never_goes_before_the_prompt_row() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell("call-1", "import time\ntime.sleep(2)\nprint('cell done')"),
        { "text": "steer answered" },
    ] }));
    let (session_id, file) = client.open_session();
    let prompt = client.prompt(&session_id, "the prompt itself");
    let turn = client.prompt_turn(0);
    let steer = client.steer(&session_id, turn, "early note");
    client.accepted(steer);
    let _ = client.wait_response(prompt);
    let rows = rows(&file);
    let note = user_row(&rows, "early note").unwrap_or_else(|| panic!("the note: {rows:?}"));
    let own = user_row(&rows, "the prompt itself").unwrap_or_else(|| panic!("{rows:?}"));
    assert!(own < note, "the prompt's row first: {rows:?}");
}

/// E5. Refusals: `-32603` for a stale turn, after the turn and during a wake (D6); `-32602` for
/// a session command and an image.
#[test]
fn steer_refusals() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-1",
            "h = bash('sleep 8; echo x')\nimport time\nopen('started', 'w').write('x')\ntime.sleep(3)\nprint('cell done')",
        ),
        { "text": "prompt done" },
        cell(
            "call-wake",
            "import time\nopen('wake.txt', 'w').write('x')\ntime.sleep(3)\nprint('wake cell done')",
        ),
        { "text": "wake done" },
    ] }));
    let (session_id, _) = client.open_session();
    let prompt = client.prompt(&session_id, "launch a command");
    let turn = client.prompt_turn(0);
    let _ = wait_file(&client.path("started"));
    let stale = client.steer(&session_id, turn + 1, "a stale turn");
    assert_eq!(client.refused(stale), Some(-32603), "a stale turn");
    let command = client.steer(&session_id, turn, "/goal status");
    assert_eq!(client.refused(command), Some(-32602), "a session command");
    let image = client.steer_blocks(
        &session_id,
        turn,
        &json!([
            { "type": "text", "text": "look" },
            { "type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo=" },
        ]),
    );
    assert_eq!(client.refused(image), Some(-32602), "an image");
    let answered = client.wait_response(prompt);
    let after = client.steer(&session_id, turn, "after the turn");
    assert_eq!(client.refused(after), Some(-32603), "after the turn");
    let wake_at = client.wait_for(answered, "the wake's turnStart", |frame| {
        origin_is(frame, "wake")
    });
    let wake = turn_of(&client.seen[wake_at]).expect("a wake id");
    let _ = wait_file(&client.path("wake.txt"));
    let during = client.steer(&session_id, wake, "during the wake");
    assert_eq!(client.refused(during), Some(-32603), "during a wake (D6)");
}

/// E6. Stop with a message accepted and not taken yet: the message never runs, the next prompt
/// runs without it.
#[test]
fn stop_with_a_steer_waiting() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-1",
            // `asyncio.sleep`, as `ya_acp_wake_turn_e2e.rs`: the Stop interrupts the cell.
            "import asyncio\nopen('started', 'w').write('x')\nawait asyncio.sleep(20)\nopen('after', 'w').write('x')",
        ),
        { "text": "second answered" },
    ] }));
    let (session_id, file) = client.open_session();
    let prompt = client.prompt(&session_id, "a long cell");
    let turn = client.prompt_turn(0);
    let _ = wait_file(&client.path("started"));
    let steer = client.steer(&session_id, turn, "waiting note");
    client.accepted(steer);
    client.notify("session/cancel", &json!({ "sessionId": session_id }));
    let answered = client.wait_response(prompt);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "cancelled",
        "{}",
        client.seen[answered]
    );
    client.drain_for(Duration::from_secs(3));
    assert!(
        client.seen[answered..]
            .iter()
            .all(|frame| turn_start(frame).is_none()),
        "no turn after the Stop: {:?}",
        &client.seen[answered..]
    );
    let second = client.prompt(&session_id, "after the stop");
    let at = client.wait_response(second);
    assert_eq!(client.seen[at]["result"]["stopReason"], "end_turn");
    let rows = rows(&file);
    assert_eq!(
        user_row(&rows, "waiting note"),
        None,
        "the message never runs: {rows:?}"
    );
    assert_eq!(
        rows.last()
            .map(|row| (row.role.as_str(), row.text.as_str())),
        Some(("assistant", "second answered")),
        "{rows:?}"
    );
    assert!(!client.path("after").exists(), "the cell stopped");
}

/// E7. A failed turn with a message accepted and not taken: no row of it in the failed turn; the
/// client sends it again as a prompt (as the extension does): one row, one answer. The provider
/// fails mid-answer (paced stream, `stopReason: "error"`); automatic retries are off, or the
/// retry would take the message.
#[test]
fn a_failed_turn_drops_the_message_it_did_not_take() {
    let mut client = Client::spawn_with(
        &json!({ "engine": "faux", "tokensPerSecond": 4, "responses": [
            {
                "content": [{ "type": "text", "text": "a long answer that fails half way through" }],
                "stopReason": "error",
                "errorMessage": "scripted provider failure",
            },
            { "text": "resent answered" },
        ] }),
        Some(&json!({ "retry": { "enabled": false } })),
    );
    let (session_id, file) = client.open_session();
    let prompt = client.prompt(&session_id, "a failing turn");
    let turn = client.prompt_turn(0);
    let _ = client.wait_for(0, "the failing answer", |frame| answer_of(frame, turn));
    let steer = client.steer(&session_id, turn, "failed note");
    client.accepted(steer);
    let failed = client.wait_response(prompt);
    assert!(
        client.seen[failed].get("error").is_some(),
        "the turn failed: {}",
        client.seen[failed]
    );
    assert_eq!(
        client.count(|frame| origin_is(frame, "steer")),
        0,
        "the message did not join the failed turn"
    );
    assert_eq!(
        user_row(&rows(&file), "failed note"),
        None,
        "no row of the message in the failed turn"
    );
    let resent = client.prompt(&session_id, "failed note");
    let resent_turn = client.prompt_turn(failed);
    let at = client.wait_response(resent);
    assert_eq!(client.seen[at]["result"]["stopReason"], "end_turn");
    client.drain_for(Duration::from_secs(1));
    let rows = rows(&file);
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "user" && row.text == "failed note")
            .count(),
        1,
        "the message runs once: {rows:?}"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == "assistant" && row.text == "resent answered")
            .count(),
        1,
        "{rows:?}"
    );
    let shown: String = client
        .seen
        .iter()
        .filter(|frame| answer_of(frame, resent_turn))
        .filter_map(|frame| frame["params"]["update"]["content"]["text"].as_str())
        .collect();
    assert_eq!(shown, "resent answered", "its answer shows once");
}
