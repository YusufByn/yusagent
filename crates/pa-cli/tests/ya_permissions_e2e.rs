// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-11, ticket 20, PR 1): the permission mode at startup, with the real binary and a
//! loopback server that plays the model (an `openai-responses` provider in `models.json`, as the
//! capture bench does) and counts its connections. [`TEST_ENV`] is removed: `.cargo/config.toml`
//! sets it to `auto` for the upstream tests.
//! a. No setting: `-p` is manual and cannot ask, so it fails at startup with the exact line to
//!    add and the path of the global settings, before any request to the model.
//! b. A project's `.yusagent/settings.json` with `"permissions": "auto"` does not change that.
//! c. RPC in manual fails the same way.
//! d. Control: `"auto"` in the global settings reaches the model (the server gets a connection)
//!    and prints the warning on stderr.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const LINE: &str = "\n  \"permissions\": \"auto\"\n";
const AUTO_WARNING: &str = "permissions are \"auto\" and there is no sandbox";

/// Counts the connections of the model's endpoint and answers each with a 400 (no retry).
struct ModelServer {
    port: u16,
    connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("local addr").port();
        let connections = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let connections = Arc::clone(&connections);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut socket, _)) => {
                            connections.fetch_add(1, Ordering::SeqCst);
                            let _ = socket.set_nonblocking(false);
                            let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
                            let mut buffer = vec![0_u8; 65_536];
                            let read = socket.read(&mut buffer).unwrap_or(0);
                            assert!(read <= buffer.len());
                            let body = r#"{"error":{"message":"permissions test","type":"invalid_request_error"}}"#;
                            let _ = write!(
                                socket,
                                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
            })
        };
        Self {
            port,
            connections,
            stop,
            thread: Some(thread),
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
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

/// An isolated home: the global settings in `<home>/.yusagent`, the project in `<home>/project`.
struct Lane {
    home: tempfile::TempDir,
    server: ModelServer,
}

impl Lane {
    fn new(global: Option<&serde_json::Value>, project: Option<&serde_json::Value>) -> Self {
        let home = tempfile::TempDir::new().unwrap();
        let server = ModelServer::start();
        let agent_dir = home.path().join(".yusagent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let models = serde_json::json!({ "providers": { "capture": {
            "baseUrl": format!("http://127.0.0.1:{}/v1", server.port),
            "apiKey": "permissions-dummy",
            "api": "openai-responses",
            "models": [{
                "id": "capture-model", "name": "Capture", "reasoning": false, "input": ["text"],
                "contextWindow": 200_000, "maxTokens": 32_000,
                "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
            }]
        }}});
        std::fs::write(agent_dir.join("models.json"), models.to_string()).unwrap();
        if let Some(global) = global {
            std::fs::write(agent_dir.join("settings.json"), global.to_string()).unwrap();
        }
        let project_dir = home.path().join("project");
        std::fs::create_dir_all(project_dir.join(".yusagent")).unwrap();
        if let Some(project) = project {
            std::fs::write(
                project_dir.join(".yusagent/settings.json"),
                project.to_string(),
            )
            .unwrap();
        }
        Self { home, server }
    }

    fn global_settings(&self) -> PathBuf {
        self.home.path().join(".yusagent/settings.json")
    }

    /// Runs the binary with `args` in the project; returns (stderr, exit code).
    fn run(&self, args: &[&str], kernel_python: Option<&Path>) -> (String, i32) {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_prime-agent"));
        command
            .args(args)
            .env("HOME", self.home.path())
            .env("DO_NOT_TRACK", "1")
            .env("PI_OFFLINE", "1")
            .env_remove(TEST_ENV)
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            .env_remove("PRIME_AGENT_SESSION_DIR")
            .env_remove("PRIME_AGENT_CODING_AGENT_SESSION_DIR")
            .env_remove("PRIME_AGENT_FAUX_SCRIPT")
            .env_remove("RLM_DEPTH")
            .current_dir(self.home.path().join("project"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        if let Some(python) = kernel_python {
            command.env("PRIME_AGENT_KERNEL_PYTHON", python);
        }
        let child = command.spawn().expect("binary present");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(120))
            .unwrap_or_else(|_| panic!("the run did not end in 120 s: {args:?}"))
            .expect("wait for the binary");
        (
            String::from_utf8_lossy(&output.stderr).to_string(),
            output.status.code().unwrap_or(-1),
        )
    }
}

const PRINT: [&str; 6] = [
    "-p",
    "--provider",
    "capture",
    "--model",
    "capture-model",
    "hello",
];
const RPC: [&str; 7] = [
    "--mode",
    "rpc",
    "--provider",
    "capture",
    "--model",
    "capture-model",
    "--no-session",
];

fn assert_refused(lane: &Lane, stderr: &str, code: i32) {
    assert_ne!(code, 0, "refused at startup; stderr: {stderr}");
    assert!(
        stderr.contains("permissions are \"manual\""),
        "names the mode; stderr: {stderr}"
    );
    assert!(stderr.contains(LINE), "the exact line; stderr: {stderr}");
    assert!(
        stderr.contains(&lane.global_settings().display().to_string()),
        "the path of the global settings; stderr: {stderr}"
    );
    assert_eq!(
        lane.server.connections(),
        0,
        "no request to the model; stderr: {stderr}"
    );
}

/// a. No setting: manual, refused before any request.
#[test]
fn print_without_setting_is_refused_before_any_request() {
    let lane = Lane::new(None, None);
    let (stderr, code) = lane.run(&PRINT, None);
    assert_refused(&lane, &stderr, code);
}

/// b. A project's settings cannot turn the session to auto.
#[test]
fn project_settings_cannot_turn_the_session_to_auto() {
    let auto = serde_json::json!({ "permissions": "auto" });
    let lane = Lane::new(None, Some(&auto));
    let (stderr, code) = lane.run(&PRINT, None);
    assert_refused(&lane, &stderr, code);
    // A global "manual" with the same project file: still manual.
    let manual = serde_json::json!({ "permissions": "manual" });
    let lane = Lane::new(Some(&manual), Some(&auto));
    let (stderr, code) = lane.run(&PRINT, None);
    assert_refused(&lane, &stderr, code);
}

/// c. RPC in manual: refused at startup.
#[test]
fn rpc_without_setting_is_refused_before_any_request() {
    let lane = Lane::new(None, None);
    let (stderr, code) = lane.run(&RPC, None);
    assert_refused(&lane, &stderr, code);
}

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_prompt_v2_e2e.rs`; the control's session may start its kernel.
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

/// d. Control: the global "auto" reaches the model and warns on stderr; the project's "manual"
/// does not change it.
#[test]
fn global_auto_reaches_the_model_and_warns() {
    let auto = serde_json::json!({ "permissions": "auto" });
    let manual = serde_json::json!({ "permissions": "manual" });
    let lane = Lane::new(Some(&auto), Some(&manual));
    let python = kernel_python();
    let (stderr, _code) = lane.run(&PRINT, Some(&python));
    assert!(
        lane.server.connections() >= 1,
        "the model was reached; stderr: {stderr}"
    );
    assert!(
        stderr.contains(AUTO_WARNING),
        "the warning; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("permissions are \"manual\""),
        "not refused; stderr: {stderr}"
    );
}
