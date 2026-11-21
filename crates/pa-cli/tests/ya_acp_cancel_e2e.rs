// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 24): `session/cancel` stops a running cell in less than 1 s (CDC,
//! YA-10 acceptance criteria), the kernel and the `bash()` subprocesses included. The real binary
//! serves ACP over stdio on its own sandboxed daemon; the worker runs the real engine on the faux
//! provider (`"engine": "faux"`) and a real kernel, permissions in `"auto"` (the tests' default,
//! `.cargo/config.toml`). Version-2 sessions wait for `await bash(cmd)` to end (`bashWaitSeconds`
//! unset is `"full"`), so the awaited command is still owned by the cell when the cancel lands.
//! a. A cell blocked on `await bash("… sleep 30")`: the prompt answers `cancelled` within 1 s of
//!    the cancel, the shell is gone within 1 s, the rest of the cell never runs; the next prompt
//!    runs a cell on the same kernel.
//! b. A cell blocked in Python (`time.sleep(60)`): the prompt answers `cancelled` within 1 s, the
//!    rest of the cell never runs.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
/// The CDC's bound (YA-10): `session/cancel` stops the turn in less than 1 s.
const CANCEL_BOUND: Duration = Duration::from_secs(1);

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_permissions_acp_e2e.rs`.
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

/// The ACP child on its own home and daemon socket (as `ya_permissions_acp_e2e.rs`).
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    home: tempfile::TempDir,
    socket: PathBuf,
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

    /// The response to `id`, with the frames before it.
    fn wait_response(&mut self, id: u64) -> (Value, Vec<Value>) {
        let deadline = Instant::now() + TIMEOUT;
        let mut before = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out; frames so far: {before:?}");
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|error| panic!("no frame ({error}); frames so far: {before:?}"));
            let frame: Value = serde_json::from_str(&line).expect("a JSON line");
            if frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
            {
                return (frame, before);
            }
            before.push(frame);
        }
    }

    /// `initialize` then `session/new`: the session id.
    fn open_session(&mut self) -> String {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-cancel-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let (response, _) = self.wait_response(new);
        response["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {response}"))
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

    /// Cancels the running turn of prompt `prompt`; the time from the cancel to its answer.
    fn cancel(&mut self, session_id: &str, prompt: u64) -> (Value, Duration) {
        let sent = Instant::now();
        self.notify("session/cancel", &json!({ "sessionId": session_id }));
        let (response, _) = self.wait_response(prompt);
        (response, sent.elapsed())
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

/// Polls until `path` exists with a non-empty content; returns it.
fn wait_file(path: &Path) -> String {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.trim().is_empty() {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether `pid` still runs: a zombie waiting for its parent's reap counts as gone.
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only probes the process.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    // Linux: the state follows the parenthesized command name in `/proc/<pid>/stat`.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                != Some("Z")
        }
        Err(_) => true,
    }
}

/// a. The cancel interrupts the cell and kills the awaited `bash()` command.
#[test]
fn a_cancel_stops_an_awaited_bash_within_the_bound() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-bash",
            "r = await bash('echo $$ > bash_pid.txt; sleep 30')\nopen('after_bash.txt', 'w').write('x')",
        ),
        cell("call-next", "open('next.txt', 'w').write('x')"),
        { "text": "next turn done" },
    ] }));
    let session_id = client.open_session();
    let prompt = client.prompt(&session_id, "run the long command");
    let pid: i32 = wait_file(&client.path("bash_pid.txt"))
        .trim()
        .parse()
        .expect("a pid");
    assert!(alive(pid), "the command runs");
    let (response, elapsed) = client.cancel(&session_id, prompt);
    assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    eprintln!("session/cancel answered after {elapsed:?}");
    assert!(
        elapsed < CANCEL_BOUND,
        "the turn stopped in {elapsed:?}, more than {CANCEL_BOUND:?}"
    );
    let deadline = Instant::now() + CANCEL_BOUND.saturating_sub(elapsed);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !alive(pid),
        "the bash() command {pid} still runs 1 s after the cancel"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !client.path("after_bash.txt").exists(),
        "the rest of the cell did not run"
    );

    // The kernel serves the next turn.
    let prompt = client.prompt(&session_id, "and now a short cell");
    let (response, _) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(client.path("next.txt").exists(), "the next cell ran");
}

/// b. The cancel interrupts a cell blocked in Python.
#[test]
fn a_cancel_stops_a_python_cell_within_the_bound() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        cell(
            "call-sleep",
            "import time\nopen('started.txt', 'w').write('x')\ntime.sleep(60)\nopen('after_sleep.txt', 'w').write('x')",
        ),
        { "text": "never requested" },
    ] }));
    let session_id = client.open_session();
    let prompt = client.prompt(&session_id, "sleep");
    let _ = wait_file(&client.path("started.txt"));
    let (response, elapsed) = client.cancel(&session_id, prompt);
    assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    eprintln!("session/cancel answered after {elapsed:?}");
    assert!(
        elapsed < CANCEL_BOUND,
        "the turn stopped in {elapsed:?}, more than {CANCEL_BOUND:?}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !client.path("after_sleep.txt").exists(),
        "the rest of the cell did not run"
    );
}
