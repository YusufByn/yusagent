// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 25): the bridge announces the start and the end of every turn,
//! the turns the agent starts on its own included (`docs/tickets/25-tours-lances-par-agent.md`).
//! The real binary serves ACP over stdio on its own sandboxed daemon; the worker runs the real
//! engine on the faux provider (`"engine": "faux"`) and a real kernel, permissions in `"auto"`
//! (the tests' default, `.cargo/config.toml`). A `bash()` the model never reads wakes the
//! session with its `[bash-done]` notice once the prompt is over.
//! a. Each turn opens with `turnStart` (`origin: "prompt"` for a prompt of this client,
//!    `"wake"` with `wake: "bash"` for a notice); two wakes take two ids, each closed by its
//!    `responseBoundary` then `terminalQuiescence`, with no response.
//! b. A prompt sent during a wake is admitted after it (T3): its `turnStart` follows the wake's
//!    boundary, and it ends with `end_turn`.
//! c. Stop during a wake stops only that turn (T4, `abort_and_send_queued`): no boundary for it,
//!    the rest of its cell never runs, and the notice queued meanwhile runs as the next wake.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const META: &str = "ai.primeintellect.prime-agent";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_acp_cancel_e2e.rs`.
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

fn phase(frame: &Value) -> Option<&str> {
    meta(frame)["phase"].as_str().filter(|_| is_update(frame))
}

/// A content update (what a turn shows), as `extensions/vscode/src/ya/meta.ts` counts them.
fn is_content(frame: &Value) -> bool {
    is_update(frame)
        && matches!(
            frame["params"]["update"]["sessionUpdate"].as_str(),
            Some("agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update")
        )
}

/// An answer chunk of turn `turn`.
fn answer_of(frame: &Value, turn: u64) -> bool {
    is_update(frame)
        && frame["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
        && turn_of(frame) == Some(turn)
}

/// The ACP child on its own home and daemon socket (as `ya_acp_cancel_e2e.rs`); every frame it
/// wrote is kept, in order.
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    home: tempfile::TempDir,
    socket: PathBuf,
    seen: Vec<Value>,
}

impl Client {
    fn spawn(script: &Value) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        let socket = home.path().join("daemon.sock");
        let script_path = home.path().join("worker-script.json");
        std::fs::write(&script_path, script.to_string()).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&socket)
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

    /// `initialize` then `session/new`: the session id.
    fn open_session(&mut self) -> String {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-wake-turn-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let at = self.wait_response(new);
        self.seen[at]["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {}", self.seen[at]))
            .to_string()
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    /// The next wake's `turnStart` from `from` on: its index and turn id.
    fn wait_wake(&mut self, from: usize) -> (usize, u64) {
        let at = self.wait_for(from, "a wake's turnStart", |frame| {
            turn_start(frame).is_some_and(|start| start["origin"] == "wake")
        });
        let start = turn_start(&self.seen[at]).expect("a turnStart");
        assert_eq!(start["wake"], "bash", "{}", self.seen[at]);
        (at, turn_of(&self.seen[at]).expect("a turn id"))
    }

    /// The `phase` frame of turn `turn` from `from` on: its index.
    fn wait_phase(&mut self, from: usize, turn: u64, wanted: &str) -> usize {
        self.wait_for(from, &format!("{wanted} of turn {turn}"), |frame| {
            phase(frame) == Some(wanted) && turn_of(frame) == Some(turn)
        })
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

/// a. Each turn opens with `turnStart`; two wakes, two ids, each closed by its boundary.
#[test]
fn every_turn_opens_with_turn_start_and_each_wake_has_its_own_id() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-launch",
            "h1 = bash('sleep 1; echo un')\nh2 = bash('sleep 4; echo deux')\nprint('launched')",
        ),
        { "text": "prompt done" },
        { "text": "wake one done" },
        { "text": "wake two done" },
    ] }));
    let session_id = client.open_session();
    let prompt = client.prompt(&session_id, "launch two commands");
    let answered = client.wait_response(prompt);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "end_turn",
        "{}",
        client.seen[answered]
    );
    // The prompt's turn: `turnStart` first among its updates, then its boundary.
    let prompt_start = client.wait_for(0, "the prompt's turnStart", |frame| {
        turn_start(frame).is_some_and(|start| start["origin"] == "prompt")
    });
    let prompt_turn = turn_of(&client.seen[prompt_start]).expect("a turn id");
    assert!(
        turn_start(&client.seen[prompt_start]).unwrap()["wake"].is_null(),
        "{}",
        client.seen[prompt_start]
    );
    let first_content = client
        .seen
        .iter()
        .position(|frame| is_content(frame) && turn_of(frame) == Some(prompt_turn))
        .unwrap();
    assert!(
        prompt_start < first_content,
        "turnStart comes before the prompt's content"
    );

    let mut previous = prompt_turn;
    let mut from = answered;
    for _ in 0..2 {
        let (start, wake) = client.wait_wake(from);
        assert!(
            wake > previous,
            "a wake takes the next id: {wake} after {previous}"
        );
        assert!(
            client.seen[from..start]
                .iter()
                .all(|frame| !is_content(frame) || turn_of(frame) != Some(wake)),
            "turnStart comes before the wake's content"
        );
        let chunk = client.wait_for(start, "the wake's answer", |frame| answer_of(frame, wake));
        let boundary = client.wait_phase(start, wake, "responseBoundary");
        assert!(boundary > chunk, "the boundary closes the wake's content");
        assert_eq!(meta(&client.seen[boundary])["outcome"], "result");
        let terminal = client.wait_phase(boundary, wake, "terminalQuiescence");
        assert_eq!(meta(&client.seen[terminal])["outcome"], "result");
        assert!(
            client.seen[start..=boundary]
                .iter()
                .all(|frame| !is_content(frame) || turn_of(frame) == Some(wake)),
            "only the wake's content between its start and its boundary"
        );
        previous = wake;
        from = boundary;
    }
    // No response for a wake: the only responses are to this client's requests.
    client.drain_for(Duration::from_millis(500));
    assert_eq!(
        client
            .seen
            .iter()
            .filter(|frame| frame.get("result").is_some() || frame.get("error").is_some())
            .count(),
        3,
        "initialize, session/new, the prompt"
    );
}

/// b. A prompt sent during a wake is admitted once the wake ended (T3).
#[test]
fn a_prompt_sent_during_a_wake_runs_after_it() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell("call-launch", "h = bash('sleep 1; echo un')\nprint('launched')"),
        { "text": "prompt done" },
        cell(
            "call-wake",
            "import time\nopen('wake.txt', 'w').write('x')\ntime.sleep(3)\nprint('wake cell done')",
        ),
        { "text": "wake done" },
        { "text": "second prompt done" },
    ] }));
    let session_id = client.open_session();
    let prompt = client.prompt(&session_id, "launch a command");
    let answered = client.wait_response(prompt);
    let (_, wake) = client.wait_wake(answered);
    let _ = wait_file(&client.path("wake.txt"));
    let second = client.prompt(&session_id, "while the agent works");
    let answered = client.wait_response(second);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "end_turn",
        "the prompt waited, it was not refused: {}",
        client.seen[answered]
    );
    let boundary = client.wait_phase(0, wake, "responseBoundary");
    let second_start = client.wait_for(boundary, "the second prompt's turnStart", |frame| {
        turn_start(frame).is_some_and(|start| start["origin"] == "prompt")
    });
    let second_turn = turn_of(&client.seen[second_start]).unwrap();
    assert!(second_turn > wake, "{second_turn} after {wake}");
    assert!(
        client.seen[..boundary]
            .iter()
            .all(|frame| turn_of(frame) != Some(second_turn)),
        "nothing of the second prompt before the wake's boundary"
    );
    let _ = client.wait_for(second_start, "the second answer", |frame| {
        answer_of(frame, second_turn)
    });
}

/// c. Stop during a wake stops that turn only; the notice queued meanwhile runs next (T4).
#[test]
fn stop_during_a_wake_stops_only_that_turn() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-launch",
            "h1 = bash('sleep 1; echo un')\nh2 = bash('sleep 3; echo deux; touch h2.txt')\nprint('launched')",
        ),
        { "text": "prompt done" },
        cell(
            "call-wake",
            // `asyncio.sleep`, not `time.sleep`: the kernel loop must stay free to send the second
            // `bash.completed` while the cell runs (with a blocked loop, the notice only comes after
            // the Stop, on an empty queue: E8).
            "import asyncio\nopen('wake.txt', 'w').write('x')\nawait asyncio.sleep(20)\nopen('after.txt', 'w').write('x')",
        ),
        { "text": "second wake done" },
    ] }));
    let session_id = client.open_session();
    let prompt = client.prompt(&session_id, "launch two commands");
    let answered = client.wait_response(prompt);
    let (first_start, first) = client.wait_wake(answered);
    let _ = wait_file(&client.path("wake.txt"));
    // The second notice is queued once its command ended (the worker admits it on the kernel's
    // `bash.completed`, a few milliseconds later).
    let _ = wait_file(&client.path("h2.txt"));
    std::thread::sleep(Duration::from_secs(1));
    client.notify("session/cancel", &json!({ "sessionId": session_id }));
    let (second_start, second) = client.wait_wake(first_start + 1);
    assert!(second > first, "{second} after {first}");
    let boundary = client.wait_phase(second_start, second, "responseBoundary");
    assert_eq!(meta(&client.seen[boundary])["outcome"], "result");
    let _ = client.wait_for(second_start, "the second wake's answer", |frame| {
        answer_of(frame, second)
    });
    assert!(
        client
            .seen
            .iter()
            .all(|frame| !(turn_of(frame) == Some(first)
                && phase(frame).is_some_and(|p| p != "event"))),
        "the stopped wake has no boundary: {:?}",
        client.seen
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !client.path("after.txt").exists(),
        "the rest of the stopped cell did not run"
    );
}
