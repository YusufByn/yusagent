// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
// 64-bit-only targets; the narrowing casts sit at bounded OS boundaries.
#![allow(clippy::cast_possible_truncation)]
#![cfg(unix)]

//! yusAgent (YA-11, ticket 20, PR 3, decision T8): a client that answers the permission requests
//! and is attached by a direct worker link (as the TUI attaches) leaves while a request waits. The
//! supervisor never routed that attach, so it sends no `detach` for it: the worker must withdraw
//! the request when the link closes (refusal, the turn stops, D7), as it does when the last
//! answering client detaches.
//!
//! Real supervisor and worker (`pa-daemon`), faux provider and a real kernel, in `"manual"`
//! (`PRIME_AGENT_TEST_PERMISSIONS`, which `.cargo/config.toml` sets to `auto` for the upstream
//! tests, is removed). A second client stays attached through the supervisor without answering,
//! and watches the session events.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const TIMEOUT: Duration = Duration::from_mins(2);
/// How long the watcher waits for the request to be closed once the link is gone.
const WITHDRAW_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Turn one calls the kernel with a cell that would create `direct.txt`; the second response is
/// never requested (the turn stops after the refusal).
fn faux_script(dir: &Path) -> PathBuf {
    let path = dir.join("faux-script.json");
    std::fs::write(
        &path,
        json!({
            "engine": "faux",
            "modelId": "faux-1",
            "modelName": "Faux Model",
            "reasoning": false,
            "contextWindow": 128_000,
            "responses": [
                {"content": [
                    {"type": "toolCall", "name": "python_repl", "id": "call-direct",
                     "arguments": {"code": "open('direct.txt', 'w').write('x')"}}
                ]},
                {"content": [{"type": "text", "text": "never requested"}]}
            ]
        })
        .to_string(),
    )
    .expect("write faux script");
    path
}

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
fn spawn_supervisor(socket: &Path, agent_dir: &Path, script: &Path) -> Supervisor {
    std::fs::create_dir_all(agent_dir).expect("agent dir");
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
        .env(pa_daemon::worker::WORKER_SCRIPT_ENV, script)
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

/// A client on the supervisor socket; it keeps the session events it reads.
struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    events: Vec<Value>,
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
                        self.events.push(value["event"].clone());
                    }
                    return Some(value);
                }
                Err(_) if Instant::now() >= deadline => return None,
                Err(_) => {}
            }
        }
    }

    fn send(&mut self, id: &str, command: &Value) -> Value {
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
                .unwrap_or_else(|| panic!("no response for {id}"));
            if line["id"] == json!(id) {
                return line;
            }
        }
    }

    /// Reads until a session event of type `kind`, or `None` at `timeout`.
    fn wait_event(&mut self, kind: &str, timeout: Duration) -> Option<Value> {
        if let Some(event) = self.events.iter().find(|event| event["type"] == kind) {
            return Some(event.clone());
        }
        let deadline = Instant::now() + timeout;
        while let Some(line) = self.read_line(deadline) {
            if line["type"] == "session_event" && line["event"]["type"] == kind {
                return Some(line["event"].clone());
            }
        }
        None
    }
}

/// A direct link to the session's worker (as `direct_attach_e2e.rs`).
struct WorkerLink {
    stream: UnixStream,
}

impl WorkerLink {
    fn connect(socket: &Path) -> WorkerLink {
        let stream = UnixStream::connect(socket).expect("connect worker socket");
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("set timeout");
        let mut link = WorkerLink { stream };
        let (header, _) = link.read_frame();
        assert_eq!(header["outboundType"], "daemon_hello", "worker hello");
        link
    }

    fn send_frame(&mut self, header: &Value, payload: &Value) {
        let frame = pa_daemon::framing::encode_private_frame(
            header,
            &serde_json::to_vec(payload).expect("payload"),
            pa_daemon::framing::DEFAULT_PRIVATE_FRAME_LIMITS,
        )
        .expect("encode frame");
        self.stream.write_all(&frame).expect("write frame");
        self.stream.flush().expect("flush");
    }

    fn read_exact(&mut self, buffer: &mut [u8], deadline: Instant) {
        let mut read = 0usize;
        while read < buffer.len() {
            assert!(Instant::now() < deadline, "worker frame read timed out");
            match self.stream.read(&mut buffer[read..]) {
                Ok(0) => panic!("worker closed the connection mid-frame"),
                Ok(n) => read += n,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => panic!("worker read: {error}"),
            }
        }
    }

    fn read_frame(&mut self) -> (Value, Vec<u8>) {
        let deadline = Instant::now() + TIMEOUT;
        let mut prefix = [0u8; 8];
        self.read_exact(&mut prefix, deadline);
        let header_len = u32::from_be_bytes(prefix[0..4].try_into().unwrap()) as usize;
        let payload_len = u32::from_be_bytes(prefix[4..8].try_into().unwrap()) as usize;
        let mut header = vec![0u8; header_len];
        self.read_exact(&mut header, deadline);
        let mut payload = vec![0u8; payload_len];
        self.read_exact(&mut payload, deadline);
        let header: Value = serde_json::from_slice(&header).expect("frame header");
        (header, payload)
    }

    fn request(&mut self, request_id: &str, command_type: &str, payload: &Value) -> Value {
        self.send_frame(
            &json!({
                "kind": "command",
                "requestId": request_id,
                "commandType": command_type,
            }),
            payload,
        );
        loop {
            let (header, body) = self.read_frame();
            if header["outboundType"] == "response" && header["requestId"] == request_id {
                return serde_json::from_slice(&body).expect("response body");
            }
        }
    }
}

/// The worker's socket, from its descriptor.
fn worker_socket(agent_dir: &Path, socket: &Path, session_id: &str) -> PathBuf {
    let descriptor_dir = pa_daemon::descriptor::descriptor_dir(agent_dir, socket);
    let content = std::fs::read_to_string(descriptor_dir.join(format!("{session_id}.json")))
        .expect("descriptor file");
    let value: Value = serde_json::from_str(&content).expect("descriptor json");
    PathBuf::from(value["socketPath"].as_str().expect("socket path"))
}

/// The `data` of the `ya-permission` entries of the session files under `dir`.
fn journal(dir: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(journal(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            found.extend(
                text.lines()
                    .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                    .filter(|entry| {
                        entry["type"] == "custom" && entry["customType"] == "ya-permission"
                    })
                    .map(|entry| entry["data"].clone()),
            );
        }
    }
    found
}

#[test]
fn a_direct_link_that_closes_while_a_request_waits_withdraws_it() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    let session_dir = agent_dir.join("sessions");
    let cwd = dir.path().join("project");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    std::fs::create_dir_all(&cwd).expect("project dir");
    let script = faux_script(dir.path());
    let socket = dir.path().join("ya-direct.sock");
    let _supervisor = spawn_supervisor(&socket, &agent_dir, &script);

    // The session can ask: its creating client declares it answers (as the TUI and ACP do).
    let mut watcher = Client::connect(&socket);
    let created = watcher.send(
        "create",
        &json!({
            "type": "create",
            "config": {
                "cwd": cwd.to_string_lossy(),
                "sessionDir": session_dir.to_string_lossy(),
                "model": "faux-1",
                "yaPermissionAsker": true,
            },
        }),
    );
    assert_eq!(created["success"], true, "create failed: {created}");
    let session_id = created["data"]["id"]
        .as_str()
        .or_else(|| created["data"]["sessionId"].as_str())
        .expect("session id")
        .to_string();
    // The watcher attaches through the supervisor, without answering.
    let attached = watcher.send(
        "watch-attach",
        &json!({ "type": "attach", "activeSessionId": session_id }),
    );
    assert_eq!(
        attached["success"], true,
        "watcher attach failed: {attached}"
    );

    // The answering client attaches by a direct link (ticket, peer auth, attach).
    let ticket = watcher.send(
        "ticket",
        &json!({ "type": "get_direct_worker_transport", "activeSessionId": session_id }),
    );
    assert_eq!(ticket["success"], true, "ticket failed: {ticket}");
    let ticket = &ticket["data"];
    let mut link = WorkerLink::connect(&worker_socket(&agent_dir, &socket, &session_id));
    let auth = link.request(
        "auth",
        "peer_auth",
        &json!({
            "type": "peer_auth",
            "grantId": ticket["grantId"],
            "token": ticket["token"],
            "workerInstanceId": ticket["workerInstanceId"],
            "purpose": "session_client",
        }),
    );
    assert_eq!(auth["success"], true, "peer auth failed: {auth}");
    let direct = link.request(
        "direct-attach",
        "attach",
        &json!({
            "type": "attach",
            "activeSessionId": session_id,
            "clientId": "direct-asker",
            "capabilities": ["attach_snapshot", "event_sequence", "slim_attach"],
            "yaPermissionAsker": true,
        }),
    );
    assert_eq!(direct["success"], true, "direct attach failed: {direct}");

    let prompted = watcher.send(
        "prompt",
        &json!({ "type": "prompt", "activeSessionId": session_id, "message": "write the file" }),
    );
    assert_eq!(prompted["success"], true, "prompt failed: {prompted}");
    let request = watcher
        .wait_event("ya_permission_request", TIMEOUT)
        .unwrap_or_else(|| panic!("no request; events: {:#?}", watcher.events));
    assert_eq!(request["toolCallId"], "call-direct");

    // The answering client leaves without a detach: its link closes.
    link.stream.shutdown(std::net::Shutdown::Both).ok();
    drop(link);

    let resolved = watcher
        .wait_event("ya_permission_resolved", WITHDRAW_TIMEOUT)
        .unwrap_or_else(|| {
            panic!(
                "the request was not withdrawn after the direct link closed; events: {:?}",
                watcher
                    .events
                    .iter()
                    .map(|event| event["type"].clone())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(resolved["requestId"], request["requestId"]);
    assert_eq!(resolved["outcome"], "cancelled", "{resolved}");
    assert!(
        watcher.wait_event("agent_end", TIMEOUT).is_some(),
        "the turn did not stop; events: {:#?}",
        watcher.events
    );
    assert!(!cwd.join("direct.txt").exists(), "the cell ran");

    let deadline = Instant::now() + TIMEOUT;
    let mut decisions = Vec::new();
    while Instant::now() < deadline {
        decisions = journal(&session_dir)
            .iter()
            .filter_map(|data| data["decision"].as_str().map(str::to_string))
            .collect();
        if !decisions.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(decisions, ["cancelled"], "journal");
}
