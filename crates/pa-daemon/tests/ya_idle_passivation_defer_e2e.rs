// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (ticket 23): an idle passivation asked while a request is routed to the worker is
//! deferred, and the supervisor answers `success` all the same
//! (`crates/pa-daemon/src/supervisor/worker_lifecycle.rs`, the eviction fence). A test that reads
//! `success` as "stopped" then sees the session still resident: the intermittent failure of
//! `passivated_child_recent_messages_reads_its_file_end_to_end` (`agent_family_e2e.rs`).
//!
//! Real supervisor and worker (`pa-daemon`), a real kernel, the faux provider. One idle root
//! session; a second client holds an `execute_bash_and_wait` on it (its route keeps an in-flight
//! permit until the command ends) while the test asks for the passivation, with the session's own
//! worker token. Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Daemon {
    child: Child,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_supervisor(socket: &Path, agent_dir: &Path, kernel_python: &Path) -> Daemon {
    let binary = env!("CARGO_BIN_EXE_pa-daemon");
    let log_file = std::fs::File::create(socket.with_extension("daemon.log")).expect("log file");
    let log_err = log_file.try_clone().expect("clone log file");
    let child = Command::new(binary)
        .arg("supervisor")
        .arg("--socket")
        .arg(socket)
        .arg("--agent-dir")
        .arg(agent_dir)
        .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("spawn pa-daemon supervisor");
    Daemon { child }
}

fn wait_socket_ready(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "supervisor socket never came up");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    fn connect(socket: &Path) -> Self {
        let writer = UnixStream::connect(socket).expect("connect");
        let reader = BufReader::new(writer.try_clone().expect("clone"));
        let mut client = Self { reader, writer };
        let hello = client.read_line();
        assert_eq!(hello["type"], "daemon_hello", "{hello}");
        client
    }

    fn send_command(&mut self, id: &str, command: &Value) {
        let envelope = json!({
            "type": "command",
            "id": id,
            "protocol": {"name": "prime-agent.daemon", "version": 7},
            "command": command,
        });
        let mut line = serde_json::to_string(&envelope).expect("serialize");
        line.push('\n');
        self.writer.write_all(line.as_bytes()).expect("write");
        self.writer.flush().expect("flush");
    }

    fn read_line(&mut self) -> Value {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => panic!("daemon closed the socket"),
            Ok(_) => serde_json::from_str(line.trim()).expect("line json"),
            Err(error) => panic!("read failed: {error}"),
        }
    }

    fn read_response(&mut self, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let line = self.read_line();
            if line.get("id").and_then(Value::as_str) == Some(id) {
                return line;
            }
            assert!(Instant::now() < deadline, "no response for {id}");
        }
    }

    /// The live residents' durable session ids (`list` without `all`).
    fn resident_session_ids(&mut self, id: &str) -> Vec<String> {
        self.send_command(id, &json!({ "type": "list" }));
        let response = self.read_response(id);
        assert_eq!(response["success"], true, "list failed: {response}");
        response["data"]["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .filter_map(|row| row["sessionId"].as_str().map(str::to_string))
            .collect()
    }
}

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_permissions_same_requests_e2e.rs`.
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

/// The worker's token from its persisted descriptor (the lookup `idle_passivation_e2e.rs` uses).
fn worker_token(agent_dir: &Path, active_session_id: &str) -> Option<String> {
    let instances = std::fs::read_dir(agent_dir.join("daemon-workers")).ok()?;
    for instance in instances.flatten() {
        let descriptor_path = instance.path().join(format!("{active_session_id}.json"));
        let Ok(content) = std::fs::read_to_string(&descriptor_path) else {
            continue;
        };
        let Ok(descriptor) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        if let Some(token) = descriptor
            .get("authenticationToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
        {
            return Some(token.to_string());
        }
    }
    None
}

#[test]
fn a_passivation_asked_behind_a_routed_request_is_deferred_then_retried() {
    let kernel_python = kernel_python();
    let dir = tempfile::TempDir::new().expect("temp dir");
    let socket = dir.path().join("daemon.sock");
    let agent_dir = dir.path().join("agent");
    let sessions_dir = agent_dir.join("sessions");
    std::fs::create_dir_all(&sessions_dir).expect("sessions dir");
    // The threshold the supervisor re-reads; the ask below reports the same value.
    std::fs::write(
        agent_dir.join("settings.json"),
        json!({ "idleEvictionMinutes": 1 }).to_string(),
    )
    .expect("write settings");
    let script = dir.path().join("root.json");
    std::fs::write(
        &script,
        json!({ "engine": "faux", "responses": [{ "text": "root turn done" }] }).to_string(),
    )
    .expect("write faux script");

    let _daemon = spawn_supervisor(&socket, &agent_dir, &kernel_python);
    wait_socket_ready(&socket);
    let mut client = Client::connect(&socket);

    // One unowned root session, settled after one turn.
    client.send_command(
        "create-root",
        &json!({
            "type": "create",
            "name": "root",
            "config": {
                "cwd": dir.path().to_string_lossy(),
                "sessionDir": sessions_dir.to_string_lossy(),
                "script": script.to_string_lossy(),
            },
        }),
    );
    let created = client.read_response("create-root");
    assert_eq!(created["success"], true, "create root failed: {created}");
    let active_session_id = created["data"]["activeSessionId"]
        .as_str()
        .or_else(|| created["data"]["id"].as_str())
        .expect("root active session id")
        .to_string();
    let session_id = created["data"]["sessionId"]
        .as_str()
        .expect("root durable session id")
        .to_string();
    client.send_command(
        "first-turn",
        &json!({
            "type": "prompt_and_wait",
            "activeSessionId": active_session_id,
            "message": "first turn",
        }),
    );
    let first = client.read_response("first-turn");
    assert_eq!(first["success"], true, "the first turn failed: {first}");
    let token = worker_token(&agent_dir, &active_session_id).expect("the root worker's token");

    // A second client holds a routed request on the root: the command writes its marker, then
    // sleeps; its route keeps an in-flight permit until the reply.
    let marker = dir.path().join("bash-started");
    let mut holder = Client::connect(&socket);
    holder.send_command(
        "held-bash",
        &json!({
            "type": "execute_bash_and_wait",
            "activeSessionId": active_session_id,
            "command": format!("touch '{}' && sleep 3", marker.display()),
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the held command never started");
        std::thread::sleep(Duration::from_millis(20));
    }

    // The ask while the request is in flight, read as "stopped" (the pattern of
    // `agent_family_e2e.rs`).
    client.send_command(
        "passivate",
        &json!({
            "type": "worker_idle_passivation",
            "workerToken": token,
            "idleMinutes": 1,
        }),
    );
    let passivated = client.read_response("passivate");
    assert_eq!(passivated["success"], true, "passivation: {passivated}");
    assert!(
        !client.resident_session_ids("list").contains(&session_id),
        "the passivation stopped the root's worker"
    );

    let held = holder.read_response("held-bash");
    assert_eq!(held["success"], true, "the held command failed: {held}");
}
