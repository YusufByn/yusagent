// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 31): settings on the ACP bridge
//! (`docs/tickets/31-skills-mcp-reglages.md`, decision 5). The real binary serves ACP over stdio
//! on its own sandboxed daemon, the worker runs the real engine on the faux provider and a real
//! kernel (harness of `ya_acp_resume_e2e.rs`).
//! 9. `_yus/settings/set` refuses `permissions` (D1) with its exact text; the file is untouched.
//! 10. It refuses a key frozen for the session and an unknown key with their exact texts, and
//!     writes an allowed key to the global settings file.
//! 11. `compaction.enabled` turned off in the global settings file applies to the running session
//!     at its next turn: no compaction, where `acp_mode_e2e.rs`
//!     (`acp_threshold_auto_compaction_publishes_the_compaction_meta`) compacts with the same
//!     script and settings left on.
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

/// The bridge's `_meta` payload of a frame's update, or `Null`.
fn meta(frame: &Value) -> &Value {
    &frame["params"]["update"]["_meta"][META]
}

/// One test's home, daemon socket and faux script, shared by its clients.
struct Home {
    dir: tempfile::TempDir,
    socket: PathBuf,
    script: PathBuf,
}

impl Home {
    fn new(script: &Value) -> Home {
        let dir = tempfile::TempDir::new().unwrap();
        let socket = dir.path().join("daemon.sock");
        let script_path = dir.path().join("worker-script.json");
        std::fs::write(&script_path, script.to_string()).unwrap();
        Home {
            dir,
            socket,
            script: script_path,
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
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

/// One ACP process; every frame it wrote is kept, in order.
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    seen: Vec<Value>,
}

impl Client {
    fn spawn(home: &Home, extra: &[&str]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&home.socket)
            .args(extra)
            .env("HOME", home.path())
            .env("PRIME_AGENT_FAUX_SCRIPT", &home.script)
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
            seen: Vec::new(),
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let stdin = self.stdin.as_mut().expect("stdin open");
        let frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{frame}").unwrap();
        stdin.flush().unwrap();
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

    /// The response to `id`.
    fn call(&mut self, method: &str, params: &Value) -> Value {
        let id = self.request(method, params);
        let at = self.wait_for(0, &format!("the response to {method}"), |frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        });
        self.seen[at].clone()
    }

    fn initialize(&mut self) {
        let _ = self.call(
            "initialize",
            &json!({ "protocolVersion": 1, "clientCapabilities": {} }),
        );
    }

    /// `initialize` then `session/new`: the response's result.
    fn open(&mut self) -> Value {
        self.initialize();
        let new = self.call("session/new", &json!({ "mcpServers": [] }));
        new["result"].clone()
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> Value {
        self.call(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    /// Closes stdin and waits for the process: the bridge detaches, the session stays resident.
    fn close(mut self) {
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
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The refusal of `permissions` (D1: global setting only, changed per conversation by the
/// selector).
const PERMISSIONS_REFUSAL: &str = "permissions is not set here: change it for this conversation with the permissions menu, or in ~/.yusagent/settings.json for every new session.";

fn faux(responses: &[&str]) -> Value {
    json!({ "engine": "faux", "responses": responses.iter().map(|text| json!({ "text": text })).collect::<Vec<_>>() })
}

fn settings_file(home: &Home) -> PathBuf {
    home.path().join(".yusagent/settings.json")
}

fn write_settings(home: &Home, settings: &Value) {
    std::fs::create_dir_all(home.path().join(".yusagent")).expect("agent dir");
    std::fs::write(settings_file(home), settings.to_string()).expect("settings.json");
}

/// The global settings file, `Null` when absent.
fn read_settings(home: &Home) -> Value {
    std::fs::read_to_string(settings_file(home))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// The `details` of a refused call, after checking it is an invalid-params refusal.
fn refusal(answer: &Value) -> String {
    assert_eq!(
        answer["error"]["code"], -32602,
        "an invalid-params refusal: {answer}"
    );
    answer["error"]["data"]["details"]
        .as_str()
        .unwrap_or_else(|| panic!("refusal details: {answer}"))
        .to_string()
}

/// 9. `permissions` is refused, the file untouched.
#[test]
fn settings_set_refuses_permissions() {
    let home = Home::new(&faux(&["unused"]));
    let mut client = Client::spawn(&home, &[]);
    let _ = client.open();
    let refused = client.call(
        "_yus/settings/set",
        &json!({ "key": "permissions", "value": "manual" }),
    );
    assert_eq!(refusal(&refused), PERMISSIONS_REFUSAL);
    assert!(
        read_settings(&home).get("permissions").is_none(),
        "the file is untouched: {}",
        read_settings(&home)
    );
    client.close();
}

/// 10. A frozen key and an unknown key are refused; an allowed key is written.
#[test]
fn settings_set_refuses_frozen_and_unknown_keys() {
    let home = Home::new(&faux(&["unused"]));
    let mut client = Client::spawn(&home, &[]);
    let _ = client.open();
    let frozen = client.call(
        "_yus/settings/set",
        &json!({ "key": "promptVersion", "value": 1 }),
    );
    assert_eq!(
        refusal(&frozen),
        "promptVersion is fixed for each conversation when it starts: it is not set here."
    );
    let unknown = client.call(
        "_yus/settings/set",
        &json!({ "key": "noSuchSetting", "value": true }),
    );
    assert_eq!(refusal(&unknown), "unknown setting: noSuchSetting");
    let written = client.call(
        "_yus/settings/set",
        &json!({ "key": "retry.enabled", "value": false }),
    );
    assert_eq!(
        written["result"]["values"]["retry.enabled"], false,
        "{written}"
    );
    assert_eq!(
        written["result"]["restart"],
        json!([]),
        "no key of the screen waits for a restart: {written}"
    );
    assert_eq!(
        read_settings(&home)["retry"]["enabled"],
        false,
        "written to the global settings file"
    );
    assert!(
        read_settings(&home).get("promptVersion").is_none(),
        "the frozen key is not written"
    );
}

/// The compaction metas among frames (a ran compaction carries `summary`), as `acp_mode_e2e.rs`.
fn ran_compactions(frames: &[Value]) -> Vec<Value> {
    frames
        .iter()
        .filter_map(|frame| {
            Some(meta(frame)["compaction"].clone()).filter(|value| !value.is_null())
        })
        .filter(|compaction| compaction["summary"].is_string())
        .collect()
}

/// 11. `compaction.enabled: false` written to the file mid-session: the next turn does not compact.
#[test]
fn compaction_disabled_in_the_settings_file_applies_without_restart() {
    let mut script = faux(&["turn one reply", "turn two reply", "the auto summary"]);
    script["contextWindow"] = json!(128_000);
    script["maxTokens"] = json!(4_096);
    let home = Home::new(&script);
    let compaction = |enabled: bool| {
        json!({ "compaction": {
            "enabled": enabled,
            "reserveTokens": 128_000 - 4_096 - 500,
            "keepRecentTokens": 10,
        } })
    };
    write_settings(&home, &compaction(true));
    let mut client = Client::spawn(&home, &[]);
    let opened = client.open();
    let session_id = opened["sessionId"].as_str().unwrap().to_string();
    let first = client.prompt(&session_id, &format!("turn one {}", "x".repeat(8_000)));
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    write_settings(&home, &compaction(false));
    let from = client.seen.len();
    let second = client.prompt(&session_id, "turn two");
    assert_eq!(second["result"]["stopReason"], "end_turn", "{second}");
    let ran = ran_compactions(&client.seen[from..]);
    assert!(
        ran.is_empty(),
        "compaction.enabled is false in the settings file: no compaction ran; ran: {ran:?}"
    );
}
