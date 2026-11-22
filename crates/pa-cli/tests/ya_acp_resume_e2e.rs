// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 28): resume and history on the ACP bridge
//! (`docs/tickets/28-reprise-historique.md`). The real binary serves ACP over stdio on its own
//! sandboxed daemon, shared by the clients of a test (same home, same socket); the worker runs
//! the real engine on the faux provider and a real kernel, permissions in `"auto"` (the tests'
//! default, `.cargo/config.toml`).
//! a. `session/new` answers with `saved { sessionId, path }`; the file exists before the first
//!    prompt. A second process on `--resume <path>` loads the session under the file's id: the
//!    transcript comes back turn by turn before the answer (`turnStart`, the prompt, the cell and
//!    its result, the answer, `turnEnd`), and the next prompt runs. Another id is refused.
//! b. `session/list` lists the folder's conversations, not a draft, the current one marked.
//! c. The delete refuses the current conversation and one open in another client; otherwise it
//!    stops the session in the daemon, then deletes its file and artifacts, its usage journal
//!    kept.
//! d. `session/load` of a conversation another client holds is refused (C12).
//! e. An ACP session writes its usage journal, and an effort change is journaled (J1).
//! f. A conversation opened new and left empty goes with its bridge: session stopped, file
//!    deleted (defect 2 of the X1 check).
//! g. An empty conversation held by another client, or one where something was asked, stays.
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

fn cell(id: &str, code: &str) -> Value {
    json!({ "content": [
        { "type": "toolCall", "id": id, "name": "python_repl", "arguments": { "code": code } },
    ] })
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

fn listed_ids(list: &Value) -> Vec<String> {
    list["result"]["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("a list: {list}"))
        .iter()
        .map(|row| row["sessionId"].as_str().unwrap().to_string())
        .collect()
}

/// a. The file in `session/new`, then a resume in another process: replay, then a prompt.
#[test]
fn a_resumed_session_replays_its_turns_then_takes_a_prompt() {
    let home = Home::new(&json!({ "engine": "faux", "responses": [
        cell("call-1", "x = 41\nprint(x + 1)"),
        { "text": "first done" },
        { "text": "second done" },
    ] }));
    let mut first = Client::spawn(&home, &[]);
    let opened = first.open();
    let acp_id = opened["sessionId"].as_str().unwrap().to_string();
    let (file_id, path) = saved(&opened);
    assert_ne!(acp_id, file_id, "a new session keeps a random ACP id (C1)");
    assert!(path.exists(), "the daemon writes the file at session/new");
    let answered = first.prompt(&acp_id, "compute");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    first.close();

    let mut second = Client::spawn(&home, &["--resume", path.to_str().unwrap()]);
    second.initialize();
    let wrong = second.call(
        "session/load",
        &json!({ "sessionId": "not-this-one", "mcpServers": [] }),
    );
    assert!(
        wrong["error"]["data"]["details"]
            .as_str()
            .is_some_and(|details| details.contains("--resume")),
        "{wrong}"
    );
    let from = second.seen.len();
    let loaded = second.call(
        "session/load",
        &json!({ "sessionId": file_id, "mcpServers": [] }),
    );
    assert_eq!(loaded["result"]["sessionId"], json!(file_id), "{loaded}");
    assert_eq!(saved(&loaded["result"]), (file_id.clone(), path));
    let answer_at = second
        .seen
        .iter()
        .position(|frame| frame["id"] == loaded["id"] && frame.get("result").is_some())
        .unwrap();
    // The replay, before the answer, all under the file's id and one turn id.
    let replay: Vec<&Value> = second.seen[from..answer_at]
        .iter()
        .filter(|frame| frame["method"] == "session/update")
        .collect();
    assert!(replay
        .iter()
        .all(|frame| frame["params"]["sessionId"] == json!(file_id)));
    let what: Vec<String> = replay
        .iter()
        .map(|frame| {
            let ours = meta(frame);
            if let Some(start) = ours.get("turnStart") {
                format!("start:{}", start["origin"].as_str().unwrap())
            } else if let Some(end) = ours.get("turnEnd") {
                format!("end:{}", end["state"].as_str().unwrap())
            } else {
                frame["params"]["update"]["sessionUpdate"]
                    .as_str()
                    .unwrap()
                    .to_string()
            }
        })
        .collect();
    assert_eq!(
        what,
        vec![
            "start:prompt",
            "user_message_chunk",
            "tool_call",
            "tool_call_update",
            "agent_message_chunk",
            "end:done",
        ],
        "{replay:?}"
    );
    assert_eq!(
        replay[1]["params"]["update"]["content"],
        json!({ "type": "text", "text": "compute" })
    );
    assert_eq!(
        replay[2]["params"]["update"]["rawInput"],
        json!({ "code": "x = 41\nprint(x + 1)" })
    );
    let turns: Vec<u64> = replay
        .iter()
        .map(|frame| meta(frame)["promptTurnId"].as_u64().unwrap())
        .collect();
    assert!(turns.iter().all(|turn| *turn == turns[0]), "{turns:?}");
    // The next prompt runs on the loaded session.
    let next = second.prompt(&file_id, "again");
    assert_eq!(next["result"]["stopReason"], "end_turn", "{next}");
}

/// b, c. The list, then the delete.
#[test]
fn the_history_lists_conversations_and_deletes_one_with_its_session() {
    let home = Home::new(&json!({ "engine": "faux", "responses": [
        cell("call-1", "y = 1\nprint(y)"),
        { "text": "one done" },
        { "text": "two done" },
    ] }));
    let mut one = Client::spawn(&home, &[]);
    let opened = one.open();
    let one_acp = opened["sessionId"].as_str().unwrap().to_string();
    let (one_id, one_path) = saved(&opened);
    let answered = one.prompt(&one_acp, "first conversation");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    one.close();

    let mut two = Client::spawn(&home, &[]);
    let opened = two.open();
    let two_acp = opened["sessionId"].as_str().unwrap().to_string();
    let (two_id, _) = saved(&opened);
    // A draft (nothing asked yet) is not listed.
    let list = two.call("session/list", &json!({}));
    assert_eq!(listed_ids(&list), vec![one_id.clone()], "{list}");
    let row = &list["result"]["sessions"][0];
    assert_eq!(row["title"], json!("first conversation"), "{row}");
    assert_eq!(
        row["_meta"][META]["path"],
        json!(one_path.to_str().unwrap())
    );
    assert!(row["updatedAt"].is_string(), "{row}");
    let answered = two.prompt(&two_acp, "second conversation");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    let list = two.call("session/list", &json!({}));
    let ids = listed_ids(&list);
    assert_eq!(ids.len(), 2, "{list}");
    let current: Vec<&Value> = list["result"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["_meta"][META]["current"] == json!(true))
        .collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["sessionId"], json!(two_id));
    // Another folder: nothing.
    let elsewhere = two.call("session/list", &json!({ "cwd": "/" }));
    assert!(listed_ids(&elsewhere).is_empty(), "{elsewhere}");

    // The current conversation is refused.
    let refused = two.call(
        "_unharnessed/sessions/delete",
        &json!({ "sessionId": two_id }),
    );
    assert_eq!(refused["error"]["code"], json!(-32602), "{refused}");
    // One open in another client is refused.
    let mut holder = Client::spawn(&home, &["--resume", one_path.to_str().unwrap()]);
    holder.initialize();
    let held = holder.call(
        "session/load",
        &json!({ "sessionId": one_id, "mcpServers": [] }),
    );
    assert!(held.get("result").is_some(), "{held}");
    let refused = two.call(
        "_unharnessed/sessions/delete",
        &json!({ "sessionId": one_id }),
    );
    assert_eq!(refused["error"]["code"], json!(-32602), "{refused}");
    holder.close();

    // Otherwise: the session stops in the daemon, its file and artifacts go, its journal stays.
    let journal = one_path.with_file_name(format!(
        "{}.usage.ndjson",
        one_path.file_stem().unwrap().to_str().unwrap()
    ));
    let journal_before = journal.exists();
    let deleted = two.call(
        "_unharnessed/sessions/delete",
        &json!({ "sessionId": one_id }),
    );
    assert_eq!(deleted["result"], json!({}), "{deleted}");
    assert!(!one_path.exists(), "the file is deleted");
    let artifacts = one_path
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("session-artifacts").join(&one_id));
    if let Some(artifacts) = artifacts {
        assert!(!artifacts.exists(), "{} is deleted", artifacts.display());
    }
    assert_eq!(journal.exists(), journal_before, "the usage journal stays");
    let list = two.call("session/list", &json!({}));
    assert_eq!(listed_ids(&list), vec![two_id], "{list}");
}

/// d. A conversation another client holds is not loaded twice (C12).
#[test]
fn a_conversation_open_in_another_client_is_not_loaded() {
    let home = Home::new(&json!({ "engine": "faux", "responses": [{ "text": "done" }] }));
    let mut holder = Client::spawn(&home, &[]);
    let opened = holder.open();
    let acp_id = opened["sessionId"].as_str().unwrap().to_string();
    let (file_id, path) = saved(&opened);
    let answered = holder.prompt(&acp_id, "hold it");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    let mut other = Client::spawn(&home, &["--resume", path.to_str().unwrap()]);
    other.initialize();
    let refused = other.call(
        "session/load",
        &json!({ "sessionId": file_id, "mcpServers": [] }),
    );
    assert!(
        refused["error"]["data"]["details"]
            .as_str()
            .is_some_and(|details| details.contains("another window")),
        "{refused}"
    );
}

/// The `<id>.usage.ndjson` lines next to a session file.
fn journal_lines(session_file: &Path) -> Vec<Value> {
    let stem = session_file.file_stem().unwrap().to_str().unwrap();
    let journal = session_file.with_file_name(format!("{stem}.usage.ndjson"));
    std::fs::read_to_string(&journal)
        .unwrap_or_else(|error| panic!("{}: {error}", journal.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// e. An ACP session (on the daemon) writes its usage journal next to its file, and an effort
/// change rides the next agent-loop call as an invalidation (decision of 21/11/2026, ticket 24;
/// J1 of ticket 28: the daemon path had no journal).
#[test]
fn an_acp_session_journals_its_calls_and_an_effort_change() {
    let home = Home::new(&json!({ "engine": "faux", "reasoning": true, "responses": [
        { "text": "first" },
        { "text": "second" },
    ] }));
    let mut client = Client::spawn(&home, &[]);
    let opened = client.open();
    let acp_id = opened["sessionId"].as_str().unwrap().to_string();
    let (_, path) = saved(&opened);
    let effort = opened["configOptions"]
        .as_array()
        .and_then(|options| {
            options
                .iter()
                .find(|option| option["id"] == "thought_level")
        })
        .unwrap_or_else(|| panic!("an effort picker: {opened}"))
        .clone();
    let current = effort["currentValue"].as_str().unwrap().to_string();
    let other = effort["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|choice| choice["value"].as_str())
        .find(|value| *value != current && *value != "off")
        .unwrap_or_else(|| panic!("another effort: {effort}"))
        .to_string();
    let answered = client.prompt(&acp_id, "one");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    let set = client.call(
        "session/set_config_option",
        &json!({ "sessionId": acp_id, "configId": "thought_level", "value": other }),
    );
    assert!(set.get("result").is_some(), "{set}");
    let answered = client.prompt(&acp_id, "two");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    let lines = journal_lines(&path);
    let mains: Vec<&Value> = lines.iter().filter(|line| line["kind"] == "main").collect();
    assert_eq!(mains.len(), 2, "{lines:?}");
    assert!(mains[0].get("invalidation").is_none(), "{lines:?}");
    assert!(mains[0]["prefix"]["system"].is_string(), "{lines:?}");
    assert_eq!(
        mains[1]["invalidation"],
        json!({ "cause": "thinking_level_change", "from": current, "to": other }),
        "{lines:?}"
    );
}

/// One daemon command on the test's socket: its response frame (as `acp_mode_e2e.rs`).
fn daemon_request(home: &Home, id: &str, command: &Value) -> Value {
    let mut writer =
        pa_types::platform::transport::connect_blocking(&home.socket).expect("daemon socket");
    let reader = writer.try_clone_box().expect("daemon socket clone");
    let _ = reader.set_read_timeout(TIMEOUT);
    let protocol = json!({ "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION });
    let frame = json!({ "type": "command", "id": id, "protocol": protocol, "command": command });
    writeln!(writer, "{frame}").expect("daemon frame");
    writer.flush().expect("daemon flush");
    BufReader::new(reader)
        .lines()
        .map(|line| serde_json::from_str::<Value>(&line.expect("daemon line")).expect("JSON"))
        .find(|frame| frame["id"] == json!(id))
        .unwrap_or_else(|| panic!("the daemon closed without answering {id}"))
}

/// The ids of the daemon's live sessions.
fn live_ids(home: &Home) -> Vec<String> {
    let list = daemon_request(home, "live", &json!({ "type": "list" }));
    list["data"]["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("a session list: {list}"))
        .iter()
        .filter_map(|row| row["sessionId"].as_str().map(str::to_string))
        .collect()
}

/// f. A conversation opened new and left with nothing asked goes with its bridge: at the end of
/// stdin its session stops in the daemon and its file goes (ticket 28, defect 2: VS Code closed
/// or reloaded with an empty conversation in the panel).
#[test]
fn an_empty_new_conversation_goes_with_its_bridge() {
    let home = Home::new(&json!({ "engine": "faux", "responses": [{ "text": "done" }] }));
    let mut client = Client::spawn(&home, &[]);
    let opened = client.open();
    let (id, path) = saved(&opened);
    assert!(path.exists(), "the file exists before the first prompt");
    assert!(live_ids(&home).contains(&id));
    client.close();
    let deadline = Instant::now() + Duration::from_secs(15);
    while (path.exists() || live_ids(&home).contains(&id)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!path.exists(), "the empty conversation's file is deleted");
    assert!(
        !live_ids(&home).contains(&id),
        "the empty conversation's session is stopped: {:?}",
        live_ids(&home)
    );
}

/// g. An empty conversation another client holds stays when its bridge ends; one where something
/// was asked stays too.
#[test]
fn an_empty_conversation_held_elsewhere_or_asked_in_stays() {
    let home = Home::new(&json!({ "engine": "faux", "responses": [{ "text": "done" }] }));
    let mut empty = Client::spawn(&home, &[]);
    let opened = empty.open();
    let (empty_id, empty_path) = saved(&opened);
    let mut holder = Client::spawn(&home, &["--resume", empty_path.to_str().unwrap()]);
    holder.initialize();
    empty.close();
    std::thread::sleep(Duration::from_secs(4));
    assert!(empty_path.exists(), "held elsewhere: the file stays");
    assert!(
        live_ids(&home).contains(&empty_id),
        "held elsewhere: the session stays"
    );
    holder.close();

    let mut asked = Client::spawn(&home, &[]);
    let opened = asked.open();
    let acp_id = opened["sessionId"].as_str().unwrap().to_string();
    let (asked_id, asked_path) = saved(&opened);
    let answered = asked.prompt(&acp_id, "something");
    assert_eq!(answered["result"]["stopReason"], "end_turn", "{answered}");
    asked.close();
    std::thread::sleep(Duration::from_secs(4));
    assert!(asked_path.exists(), "asked in: the file stays");
    assert!(
        live_ids(&home).contains(&asked_id),
        "asked in: the session stays"
    );
}
