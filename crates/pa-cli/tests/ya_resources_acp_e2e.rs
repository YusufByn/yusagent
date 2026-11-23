// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 31): skills and MCP on the ACP bridge
//! (`docs/tickets/31-skills-mcp-reglages.md`). The real binary serves ACP over stdio on its own
//! sandboxed daemon, the worker runs the real engine on the faux provider and a real kernel
//! (harness of `ya_acp_resume_e2e.rs`).
//! 6. `_yus/skills/list` answers the shape of the extension's skills screen.
//! 7. `_yus/skills/set` turns a skill off; the next prompt announces it once (`_yus/notice`, one
//!    `ya-resources-changed` row in the session file), the one after announces nothing.
//! 8. `_yus/mcp/list` lists the local catalog and the servers of `mcpServers`; `_yus/mcp/logout`
//!    answers the list.
//! 12. The notice of a change is published live and replayed by `session/load`, before the prompt
//!     it came with.
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

/// `saved` of a `session/new` or `session/load` result: `(id, path)`.
fn saved(result: &Value) -> (String, PathBuf) {
    let saved = &result["_meta"][META]["saved"];
    let id = saved["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("saved.sessionId: {result}"));
    let path = saved["path"]
        .as_str()
        .unwrap_or_else(|| panic!("saved.path: {result}"));
    (id.to_string(), PathBuf::from(path))
}

const SKILL: &str = "ya31-demo";
const DESCRIPTION: &str = "Demo skill for ticket 31.";
/// The `_meta` key of the bridge's notice (`session_info_update`, as `turnStart`).
const NOTICE: &str = "_yus/notice";
/// The custom type of an announcement row in the session file.
const ROW_TYPE: &str = "\"ya-resources-changed\"";

/// Writes `~/.yusagent/skills/<SKILL>/SKILL.md` in the test's home; returns its path.
fn write_skill(home: &Home) -> PathBuf {
    let dir = home.path().join(".yusagent/skills").join(SKILL);
    std::fs::create_dir_all(&dir).expect("skill dir");
    let file = dir.join("SKILL.md");
    std::fs::write(
        &file,
        format!("---\nname: {SKILL}\ndescription: {DESCRIPTION}\n---\n\n# {SKILL}\n\nUsed by the ticket 31 tests.\n"),
    )
    .expect("SKILL.md");
    file
}

/// The notices among `frames`, in order.
fn notices(frames: &[Value]) -> Vec<Value> {
    frames
        .iter()
        .filter(|frame| frame["method"] == "session/update")
        .filter_map(|frame| {
            Some(frame["params"]["update"]["_meta"][NOTICE].clone())
                .filter(|notice| !notice.is_null())
        })
        .collect()
}

/// The announcement rows of a session file.
fn announcement_rows(session_file: &Path) -> usize {
    std::fs::read_to_string(session_file)
        .expect("the session file")
        .lines()
        .filter(|line| line.contains(ROW_TYPE))
        .count()
}

/// The row of `SKILL` in a skills answer (`_yus/skills/*`).
fn skill_row(answer: &Value) -> Value {
    answer["result"]["skills"]
        .as_array()
        .unwrap_or_else(|| panic!("a skills list: {answer}"))
        .iter()
        .find(|row| row["name"] == SKILL)
        .unwrap_or_else(|| panic!("{SKILL} listed: {answer}"))
        .clone()
}

fn faux(responses: usize) -> Value {
    json!({ "engine": "faux", "responses": (1..=responses).map(|turn| json!({ "text": format!("answer {turn}") })).collect::<Vec<_>>() })
}

/// 6. The list answers the shape of the extension's skills screen.
#[test]
fn skills_list_answers_the_screen_shape() {
    let home = Home::new(&faux(1));
    write_skill(&home);
    let mut client = Client::spawn(&home, &[]);
    let _ = client.open();
    let listed = client.call("_yus/skills/list", &json!({}));
    let row = skill_row(&listed);
    assert_eq!(row["description"], DESCRIPTION, "{row}");
    assert!(
        row["path"]
            .as_str()
            .is_some_and(|path| path.ends_with(&format!(".yusagent/skills/{SKILL}/SKILL.md"))),
        "{row}"
    );
    assert_eq!(
        row["location"],
        format!("~/.yusagent/skills/{SKILL}"),
        "{row}"
    );
    assert_eq!(row["source"], "user", "{row}");
    assert_eq!(row["enabled"], true, "{row}");
    assert_eq!(row["active"], true, "{row}");
    assert!(listed["result"]["folders"].is_array(), "{listed}");
    assert!(listed["result"]["problems"].is_array(), "{listed}");
}

/// 7. A skill turned off is announced once, at the next prompt.
#[test]
fn skills_set_then_prompt_announces_once() {
    let home = Home::new(&faux(3));
    let file = write_skill(&home);
    let mut client = Client::spawn(&home, &[]);
    let opened = client.open();
    let session_id = opened["sessionId"].as_str().unwrap().to_string();
    let (_, session_file) = saved(&opened);
    let first = client.prompt(&session_id, "first");
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    let set = client.call(
        "_yus/skills/set",
        &json!({ "path": file.display().to_string(), "enabled": false }),
    );
    let row = skill_row(&set);
    assert_eq!(row["enabled"], false, "turned off: {row}");
    assert_eq!(
        row["active"], true,
        "the running session still has it: {row}"
    );
    let from = client.seen.len();
    for text in ["second", "third"] {
        let answered = client.prompt(&session_id, text);
        assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    }
    let published = notices(&client.seen[from..]);
    assert_eq!(
        published.len(),
        1,
        "one notice over two prompts: {published:?}"
    );
    assert_eq!(published[0]["kind"], "resources", "{published:?}");
    assert_eq!(
        published[0]["details"]["skills"]["removed"],
        json!([SKILL]),
        "{published:?}"
    );
    assert_eq!(
        announcement_rows(&session_file),
        1,
        "one announcement row in the session file"
    );
}

/// 8. The MCP list: the local catalog and the servers of `mcpServers`; the logout answers it.
#[test]
fn mcp_list_and_logout() {
    let home = Home::new(&faux(1));
    std::fs::create_dir_all(home.path().join(".yusagent")).expect("agent dir");
    std::fs::write(
        home.path().join(".yusagent/settings.json"),
        json!({ "mcpServers": { "ya31srv": { "type": "stdio", "command": "ya31-mcp-never-run" } } })
            .to_string(),
    )
    .expect("settings.json");
    let mut client = Client::spawn(&home, &[]);
    let _ = client.open();
    let listed = client.call("_yus/mcp/list", &json!({}));
    let servers = listed["result"]["servers"]
        .as_array()
        .unwrap_or_else(|| panic!("a servers list: {listed}"));
    let source = |id: &str| {
        servers
            .iter()
            .find(|server| server["id"] == id)
            .map(|server| server["source"].clone())
    };
    assert_eq!(source("ya31srv"), Some(json!("user")), "{listed}");
    assert_eq!(source("linear"), Some(json!("catalog")), "{listed}");
    let out = client.call("_yus/mcp/logout", &json!({ "id": "ya31srv" }));
    assert!(out["result"]["servers"].is_array(), "{out}");
}

/// 12. The notice is published live, then replayed by `session/load` before its prompt.
#[test]
fn notice_is_replayed_by_session_load() {
    let home = Home::new(&faux(3));
    let file = write_skill(&home);
    let mut first = Client::spawn(&home, &[]);
    let opened = first.open();
    let session_id = opened["sessionId"].as_str().unwrap().to_string();
    let (file_id, path) = saved(&opened);
    let answered = first.prompt(&session_id, "first");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    std::fs::remove_dir_all(file.parent().expect("skill dir")).expect("remove the skill");
    let from = first.seen.len();
    let answered = first.prompt(&session_id, "second");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    let live = notices(&first.seen[from..]);
    first.close();

    let mut second = Client::spawn(&home, &["--resume", path.to_str().unwrap()]);
    second.initialize();
    let from = second.seen.len();
    let loaded = second.call(
        "session/load",
        &json!({ "sessionId": file_id, "mcpServers": [] }),
    );
    assert_eq!(loaded["result"]["sessionId"], json!(file_id), "{loaded}");
    let replay = &second.seen[from..];
    let replayed = notices(replay);
    assert_eq!(replayed.len(), 1, "the notice is replayed: {replay:?}");
    assert_eq!(
        replayed[0]["details"]["skills"]["removed"],
        json!([SKILL]),
        "{replayed:?}"
    );
    let notice_at = replay
        .iter()
        .position(|frame| !frame["params"]["update"]["_meta"][NOTICE].is_null())
        .expect("the notice frame");
    let prompt_at = replay
        .iter()
        .position(|frame| {
            frame["params"]["update"]["content"] == json!({ "type": "text", "text": "second" })
        })
        .expect("the second prompt replayed");
    assert!(
        notice_at < prompt_at,
        "the notice comes before its prompt: {replay:?}"
    );
    assert_eq!(live.len(), 1, "the notice is published live: {live:?}");
    assert_eq!(live[0], replayed[0], "the replay repeats the live notice");
}
