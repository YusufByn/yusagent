// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(target_os = "macos")]
//! yusAgent (YA-11, ticket 41, PR A, test 3): the real binary in ACP mode, its own daemon, the
//! faux provider and a real kernel, with the guard forced on (no outer `sandbox-exec`).
//! - The bridge still stores a key (`_yus/providers/set_api_key`) and the daemon still serves
//!   the session: the credential processes keep their access (positive control).
//! - A `python_repl` cell of that session cannot read `auth.json` nor reach the daemon's socket.
//! - yusAgent (YA-18, ticket 43, PR 3a, T27 under the guard): the workspace snapshots' git runs
//!   under its profile variant, the agent directory inside the workspace and a link to `auth.json`
//!   in it: the snapshots are taken and no object holds the credential.
//!
//! Harness of `ya_resources_acp_e2e.rs`. Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const SENTINEL: &str = "ya41-sentinel-acp-auth-91c0";
const META: &str = "ai.primeintellect.prime-agent";
/// How long the test waits for a snapshot written off the turn.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(20);

fn kernel_python() -> PathBuf {
    if let Some(explicit) = std::env::var_os("PA_E2E_KERNEL_PYTHON") {
        return PathBuf::from(explicit);
    }
    let candidate = PathBuf::from(format!(
        "{}/.yusagent/kernel-venv/bin/python",
        std::env::var("HOME").unwrap_or_default()
    ));
    assert!(
        candidate.exists(),
        "kernel python {} not found: set PA_E2E_KERNEL_PYTHON",
        candidate.display()
    );
    candidate
}

struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    seen: Vec<Value>,
    socket: PathBuf,
}

impl Client {
    fn spawn(home: &std::path::Path, socket: &std::path::Path, script: &std::path::Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(socket)
            .env("HOME", home)
            .env("PRIME_AGENT_FAUX_SCRIPT", script)
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            // The guard is forced on: `.cargo/config.toml` switches it off for the other tests.
            .env_remove("PRIME_AGENT_SECRET_GUARD")
            .current_dir(home)
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
            socket: socket.to_path_buf(),
        }
    }

    fn call(&mut self, method: &str, params: &Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let stdin = self.stdin.as_mut().expect("stdin open");
        let frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{frame}").unwrap();
        stdin.flush().unwrap();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for {method}: {:?}",
                self.seen
            );
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|error| panic!("no frame ({error}) for {method}: {:?}", self.seen));
            let frame: Value = serde_json::from_str(&line).expect("a JSON line");
            let answered = frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some());
            self.seen.push(frame.clone());
            if answered {
                return frame;
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The test's own daemon goes with it.
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

#[test]
fn a_session_cell_cannot_read_the_credentials_while_the_bridge_still_can() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let agent = home.join(".yusagent");
    std::fs::create_dir_all(&agent).unwrap();
    let auth = agent.join("auth.json");
    std::fs::write(
        &auth,
        format!(r#"{{"serper":{{"type":"api_key","key":"{SENTINEL}"}}}}"#),
    )
    .unwrap();
    let socket = home.join("daemon.sock");
    let results = home.join("ya41-results.json");
    let cell = format!(
        r#"import json, socket
results = {{}}
try:
    results["read auth.json"] = "ok" if {SENTINEL:?} in open({auth:?}).read() else "empty"
except PermissionError:
    results["read auth.json"] = "denied"
try:
    client = socket.socket(socket.AF_UNIX)
    client.connect({socket:?})
    client.close()
    results["connect to the daemon socket"] = "ok"
except PermissionError:
    results["connect to the daemon socket"] = "denied"
open({results:?}, "w").write(json.dumps(results))
print(json.dumps(results))"#,
        auth = auth.display().to_string(),
        socket = socket.display().to_string(),
        results = results.display().to_string(),
    );
    let script = home.join("worker-script.json");
    std::fs::write(
        &script,
        json!({
            "engine": "faux",
            "responses": [
                { "content": [ { "type": "toolCall", "name": "python_repl", "arguments": { "code": cell } } ] },
                { "text": "done" },
            ],
        })
        .to_string(),
    )
    .unwrap();

    let mut client = Client::spawn(&home, &socket, &script);
    let _ = client.call(
        "initialize",
        &json!({ "protocolVersion": 1, "clientCapabilities": {} }),
    );
    // Positive control: the bridge still writes the credential file.
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "openai", "apiKey": "sk-ya41-openai" }),
    );
    assert_eq!(set["result"]["outcome"], "done", "{set}");
    assert!(std::fs::read_to_string(&auth)
        .unwrap()
        .contains("sk-ya41-openai"));
    let new = client.call("session/new", &json!({ "mcpServers": [] }));
    let session = new["result"]["sessionId"]
        .as_str()
        .expect("a session")
        .to_string();
    let reply = client.call(
        "session/prompt",
        &json!({ "sessionId": session, "prompt": [{ "type": "text", "text": "run the cell" }] }),
    );
    assert!(reply.get("result").is_some(), "{reply}");
    let outcome: Value = serde_json::from_str(
        &std::fs::read_to_string(&results).expect("the cell wrote its results"),
    )
    .unwrap();
    assert_eq!(
        outcome["read auth.json"], "denied",
        "a session cell read auth.json: {outcome}"
    );
    assert_eq!(
        outcome["connect to the daemon socket"], "denied",
        "a session cell reached the daemon socket: {outcome}"
    );
}

/// `git` on the shadow repository `repo` (the test's own, outside the guard).
fn shadow(repo: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg(format!("--git-dir={}", repo.display()))
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The shadow repository of `workspace` (`<agent>/checkpoints/<digest>/`, whose `workspace` file
/// names it) once it holds `name`.
fn wait_snapshot(workspace: &std::path::Path, name: &str) -> PathBuf {
    let root = workspace.join(".yusagent/checkpoints");
    let deadline = Instant::now() + SNAPSHOT_WAIT;
    loop {
        let found = std::fs::read_dir(&root).ok().and_then(|dir| {
            dir.filter_map(Result::ok)
                .map(|entry| std::fs::DirEntry::path(&entry))
                .find(|repo| {
                    std::fs::read_to_string(repo.join("workspace"))
                        .is_ok_and(|named| std::path::Path::new(named.trim()) == workspace)
                        && shadow(repo, &["rev-parse", "-q", "--verify", name]).is_some()
                })
        });
        if let Some(repo) = found {
            return repo;
        }
        assert!(
            Instant::now() < deadline,
            "no snapshot {name} for {} under {}",
            workspace.display(),
            root.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// T27 under the guard (ticket 43, PR 3a).
#[test]
fn the_snapshots_hold_no_credential_under_the_guard() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let agent = home.join(".yusagent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("auth.json"),
        format!(r#"{{"serper":{{"type":"api_key","key":"{SENTINEL}"}}}}"#),
    )
    .unwrap();
    std::os::unix::fs::symlink(".yusagent/auth.json", home.join("auth-link")).unwrap();
    std::fs::write(home.join("code.py"), "print(1)").unwrap();
    let socket = home.join("daemon.sock");
    let script = home.join("worker-script.json");
    std::fs::write(
        &script,
        json!({
            "engine": "faux",
            "responses": [
                { "content": [ { "type": "toolCall", "name": "python_repl",
                    "arguments": { "code": "open('code.py', 'w').write('print(2)')\nprint('ok')" } } ] },
                { "text": "done" },
            ],
        })
        .to_string(),
    )
    .unwrap();

    let mut client = Client::spawn(&home, &socket, &script);
    let _ = client.call(
        "initialize",
        &json!({ "protocolVersion": 1, "clientCapabilities": {} }),
    );
    let new = client.call("session/new", &json!({ "mcpServers": [] }));
    let session = new["result"]["sessionId"]
        .as_str()
        .expect("a session")
        .to_string();
    let file = PathBuf::from(
        new["result"]["_meta"][META]["saved"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.path: {new}")),
    );
    let header: Value = serde_json::from_str(
        std::fs::read_to_string(&file)
            .unwrap()
            .lines()
            .next()
            .expect("a header"),
    )
    .unwrap();
    let file_id = header["id"].as_str().expect("a session id").to_string();
    let reply = client.call(
        "session/prompt",
        &json!({ "sessionId": session, "prompt": [{ "type": "text", "text": "change code.py" }] }),
    );
    assert!(reply.get("result").is_some(), "{reply}");
    let repo = wait_snapshot(&home, &format!("refs/ya/{file_id}/fin"));
    let objects = shadow(&repo, &["cat-file", "--batch-all-objects", "--batch"])
        .expect("the objects are readable");
    assert!(
        !objects.contains(SENTINEL),
        "a credential reached the snapshots"
    );
}
