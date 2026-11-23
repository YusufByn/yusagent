#![cfg(unix)]

//! yusAgent (ticket 21): the first session of a fresh agent dir reached through a symlink keeps
//! its messages. `HOME` (so the agent dir `~/.yusagent`, whose `sessions` dir does not exist yet)
//! is a symlink to a real directory, as macOS's `/var/folders` and `/tmp`. The real binary serves
//! ACP on its own daemon; the worker runs the faux provider and a real kernel in `"manual"` (ticket
//! 30: unset is `"auto"`; set by `PRIME_AGENT_TEST_PERMISSIONS`, debug builds only, so that the
//! agent dir stays fresh, without a `settings.json`): one turn, one cell, allowed. The session file must hold the turn's messages and the `ya-permission`
//! entry, and the worker's stderr log no `pa-daemon:` line.
//!
//! Before the lease fix (upstream `12a5081ac`, `crates/pa-daemon/src/lease.rs`), the lease kept
//! the raw symlinked path of the not-yet-created file while each append compared the canonical
//! one: every append failed with "session lease does not own append target", silently for the
//! messages. Linux canonicalizes the symlink too, so the test fails there without the fix.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const TIMEOUT: Duration = Duration::from_mins(2);
const SETTLE: Duration = Duration::from_secs(30);

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

/// The ACP child with `HOME` on the symlink `<root>/home` -> `<root>/real`.
struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    _root: tempfile::TempDir,
    home: PathBuf,
    socket: PathBuf,
}

impl Client {
    fn spawn(script: &Value) -> Client {
        let root = tempfile::TempDir::new().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let home = root.path().join("home");
        std::os::unix::fs::symlink(&real, &home).unwrap();
        assert_ne!(
            home.canonicalize().unwrap(),
            home,
            "HOME is reached through a symlink"
        );
        assert!(!home.join(".yusagent/sessions").exists(), "fresh agent dir");
        let socket = root.path().join("daemon.sock");
        let script_path = root.path().join("worker-script.json");
        std::fs::write(&script_path, script.to_string()).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&socket)
            .env("HOME", &home)
            .env("PRIME_AGENT_FAUX_SCRIPT", &script_path)
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
                "15000",
            )
            .env(TEST_ENV, "manual")
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            .env_remove("PRIME_AGENT_SESSION_DIR")
            .current_dir(&home)
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
            _root: root,
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

    /// Reads frames until the response to `id`, allowing every `session/request_permission` on
    /// the way; returns the response and the number of requests allowed.
    fn wait_response_allowing(&mut self, id: u64) -> (Value, usize) {
        let deadline = Instant::now() + TIMEOUT;
        let mut allowed = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out waiting for response {id}");
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|error| panic!("no frame ({error})"));
            let frame: Value = serde_json::from_str(&line).expect("a JSON line");
            if frame["method"] == "session/request_permission" {
                let Some(allow) = frame["params"]["options"].as_array().and_then(|options| {
                    options.iter().find(|option| option["kind"] == "allow_once")
                }) else {
                    panic!("an allow_once option: {frame}");
                };
                let option = allow["optionId"].clone();
                let request_id = frame["id"].clone();
                self.send(&json!({ "jsonrpc": "2.0", "id": request_id, "result": {
                    "outcome": { "outcome": "selected", "optionId": option },
                } }));
                allowed += 1;
                continue;
            }
            if frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
            {
                return (frame, allowed);
            }
        }
    }

    fn agent_dir(&self) -> PathBuf {
        self.home.join(".yusagent")
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

/// Every JSON line of the `.jsonl` files under `dir`.
fn session_lines(dir: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            lines.extend(session_lines(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            lines.extend(
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok()),
            );
        }
    }
    lines
}

/// The `pa-daemon:` lines of the worker stderr logs under `<agent>/logs`.
fn daemon_log_lines(agent_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(agent_dir.join("logs")) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("worker-") && name.ends_with(".stderr.log") {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            lines.extend(
                text.lines()
                    .filter(|line| line.starts_with("pa-daemon:"))
                    .map(str::to_string),
            );
        }
    }
    lines
}

fn roles(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter(|entry| entry["type"] == "message")
        .filter_map(|entry| entry["message"]["role"].as_str().map(str::to_string))
        .collect()
}

fn permission_decisions(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter(|entry| entry["type"] == "custom" && entry["customType"] == "ya-permission")
        .filter_map(|entry| entry["data"]["decision"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn the_first_session_under_a_symlinked_home_keeps_its_messages() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "content": [
            { "type": "toolCall", "id": "call-1", "name": "python_repl",
              "arguments": { "code": "print(1)" } },
        ] },
        { "text": "lease turn done" },
    ] }));
    let init = client.request(
        "initialize",
        &json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": { "name": "ya-lease-symlink-e2e", "version": "0.0.1" },
        }),
    );
    let _ = client.wait_response_allowing(init);
    let new = client.request("session/new", &json!({ "mcpServers": [] }));
    let (response, _) = client.wait_response_allowing(new);
    let session_id = response["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("a session: {response}"))
        .to_string();

    let prompt = client.request(
        "session/prompt",
        &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": "run one cell" }] }),
    );
    let (response, allowed) = client.wait_response_allowing(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert_eq!(allowed, 1, "manual mode: one request for the one cell");

    let sessions = client.agent_dir().join("sessions");
    let deadline = Instant::now() + SETTLE;
    let lines = loop {
        let lines = session_lines(&sessions);
        let done = roles(&lines).len() >= 4 && !permission_decisions(&lines).is_empty();
        if done || Instant::now() >= deadline {
            break lines;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(
        roles(&lines),
        vec!["user", "assistant", "toolResult", "assistant"],
        "the turn's messages are in the session file: {lines:?}"
    );
    assert!(
        lines.iter().any(|entry| entry["type"] == "message"
            && entry["message"].to_string().contains("lease turn done")),
        "the final answer is persisted: {lines:?}"
    );
    assert_eq!(
        permission_decisions(&lines),
        vec!["allow"],
        "the permission is journaled: {lines:?}"
    );
    assert_eq!(
        daemon_log_lines(&client.agent_dir()),
        Vec::<String>::new(),
        "no persistence error in the worker log"
    );
}
