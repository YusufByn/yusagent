// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-18, ticket 43, PR 1): `_yus/session/rewind`, the conversation back to before a
//! prompt, to send it again changed (`docs/tickets/43-retour-arriere.md`, §I.5). The real binary
//! serves ACP over stdio on its own sandboxed daemon; the worker runs the real engine on the faux
//! provider (`"engine": "faux"`) and a real kernel, as `ya_acp_steer_e2e.rs`. The conversation the
//! model is sent is the session file's branch, from its last entry back to the root.
//! T1. A prompt's `turnStart` carries `rewindTo`, the session's leaf before the prompt.
//! T2, T7. A rewind then a prompt: the branch runs to the point, then the fixed text (D2), then
//!     the new prompt; the abandoned turn stays in the file, off the branch.
//! T3. Refused while a turn runs (`-32603`, Unharnessed's text); the turn runs to its end.
//! T4. An entry off the branch: `-32602`, `data.details` "… is not in the conversation".
//! T5. (a) A leaf before the prompt that is a custom row written outside a turn stays (Unharnessed
//!     stays on `to`; `navigate_tree` would go to its parent). (b) The `[skills-changed]` row of
//!     the abandoned turn comes back, once, before the new prompt. (c) The fixed text of an earlier
//!     rewind in the abandoned turn, then two rewinds in a row: one fixed text.
//! T8. The usage journal: `invalidation: {"cause": "rewind"}` on the next agent-loop call only.
//! T9. A resume after a rewind and a prompt (daemon restarted): the branch resumed is the new one.
//! T11. A `[bash-done]` of a command started in the abandoned turn is delivered after the rewind
//!     (K-a), on the new branch.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const META: &str = "ai.primeintellect.prime-agent";
const REWIND: &str = "_yus/session/rewind";
/// The fixed text the model gets after a rewind (D2, validated by Yusuf, ticket 43, §H).
const REWOUND: &str = "[conversation-rewound] The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.";
/// The custom type of the fixed text's row.
const REWOUND_TYPE: &str = "ya-conversation-rewound";
/// The custom type of a skills or MCP announcement row (ticket 31).
const RESOURCES_TYPE: &str = "ya-resources-changed";
/// Unharnessed's refusal while the agent works (`crates/uh-daemon/src/acp/rewind.rs:48-53`).
const WORKING: &str = "The agent is working: stop it before going back.";

/// A cell that marks its start, runs three seconds, then marks its end.
const SLOW_CELL: &str = "import time\nopen('started', 'w').write('x')\ntime.sleep(3)\nopen('ended', 'w').write('x')\nprint('cell done')";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_acp_steer_e2e.rs`.
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

fn is_update(frame: &Value) -> bool {
    frame["method"] == "session/update"
}

fn turn_start(frame: &Value) -> Option<&Value> {
    Some(&meta(frame)["turnStart"]).filter(|start| is_update(frame) && start.is_object())
}

fn origin_is(frame: &Value, origin: &str) -> bool {
    turn_start(frame).is_some_and(|start| start["origin"] == origin)
}

fn phase(frame: &Value) -> Option<&str> {
    meta(frame)["phase"].as_str().filter(|_| is_update(frame))
}

fn turn_of(frame: &Value) -> Option<u64> {
    meta(frame)["promptTurnId"].as_u64()
}

/// The text of a message's or a custom row's content.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// The entries of the session file (the header left out), in file order.
fn entries(file: &Path) -> Vec<Value> {
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["type"] != "session" && entry["id"].is_string())
        .collect()
}

/// The id of the session file's last entry: the leaf, between turns.
fn last_id(file: &Path) -> String {
    entries(file)
        .last()
        .and_then(|entry| entry["id"].as_str())
        .unwrap_or_else(|| panic!("an entry in {}", file.display()))
        .to_string()
}

/// The branch of the session file: from its last entry back to the root, in order.
fn branch(file: &Path) -> Vec<Value> {
    let all = entries(file);
    let mut path = Vec::new();
    let mut current = all.last().cloned();
    while let Some(entry) = current {
        let parent = entry["parentId"].as_str().map(str::to_string);
        path.push(entry);
        current = parent.and_then(|parent| {
            all.iter()
                .find(|other| other["id"].as_str() == Some(parent.as_str()))
                .cloned()
        });
    }
    path.reverse();
    path
}

/// One entry as the model sees it: `role:text` for a message, `custom:<type>` for a custom row;
/// `None` for the other kinds (model, effort, labels).
fn shown(entry: &Value) -> Option<String> {
    match entry["type"].as_str() {
        Some("message") => Some(format!(
            "{}:{}",
            entry["message"]["role"].as_str().unwrap_or_default(),
            text_of(&entry["message"]["content"])
        )),
        Some("custom_message") => Some(format!(
            "custom:{}",
            entry["customType"].as_str().unwrap_or_default()
        )),
        _ => None,
    }
}

/// The branch as the model sees it.
fn conversation(file: &Path) -> Vec<String> {
    branch(file).iter().filter_map(shown).collect()
}

/// The text of the fixed text's rows on the branch.
fn rewound_texts(file: &Path) -> Vec<String> {
    branch(file)
        .iter()
        .filter(|entry| entry["type"] == "custom_message" && entry["customType"] == REWOUND_TYPE)
        .map(|entry| text_of(&entry["content"]))
        .collect()
}

/// The usage journal next to the session file, line by line (as `ya_acp_resume_e2e.rs`).
fn journal_lines(file: &Path) -> Vec<Value> {
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let journal = file.with_file_name(format!("{stem}.usage.ndjson"));
    std::fs::read_to_string(&journal)
        .unwrap_or_else(|error| panic!("{}: {error}", journal.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Writes a skill under `~/.yusagent/skills` (as `ya_resources_acp_e2e.rs`).
fn write_skill(home: &Path) {
    let dir = home.join(".yusagent/skills/ya43-demo");
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: ya43-demo\ndescription: Demo skill for ticket 43.\n---\n\n# ya43-demo\n\nUsed by the ticket 43 tests.\n",
    )
    .expect("SKILL.md");
}

fn wait_file(path: &Path) {
    let deadline = Instant::now() + TIMEOUT;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One daemon command on the test's socket: its response frame (as `ya_acp_resume_e2e.rs`).
fn daemon_request(socket: &Path, id: &str, command: &Value) -> Value {
    let mut writer =
        pa_types::platform::transport::connect_blocking(socket).expect("daemon socket");
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

/// Stops the test's daemon and waits until its socket no longer answers.
fn shutdown_daemon(socket: &Path) {
    if let Ok(mut stream) = pa_types::platform::transport::connect_blocking(socket) {
        let frame = json!({
            "type": "command", "id": "shutdown-test",
            "protocol": { "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION },
            "command": { "type": "shutdown" },
        });
        let _ = writeln!(stream, "{frame}");
        let _ = stream.flush();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while pa_types::platform::transport::connect_blocking(socket).is_ok() {
        assert!(Instant::now() < deadline, "the daemon did not stop");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The ACP child on its own home and daemon socket (as `ya_acp_steer_e2e.rs`); every frame it
/// wrote is kept, in order.
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    home: Rc<tempfile::TempDir>,
    socket: PathBuf,
    seen: Vec<Value>,
}

impl Client {
    fn spawn(script: &Value) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join("worker-script.json"), script.to_string()).unwrap();
        Client::launch(Rc::new(home), &[])
    }

    /// Another ACP process on the same home and daemon socket: `--resume <file>`.
    fn resume(&self, file: &Path) -> Client {
        Client::launch(
            Rc::clone(&self.home),
            &["--resume", file.to_str().expect("a UTF-8 path")],
        )
    }

    fn launch(home: Rc<tempfile::TempDir>, extra: &[&str]) -> Client {
        let socket = home.path().join("daemon.sock");
        let script_path = home.path().join("worker-script.json");
        let mut child = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&socket)
            .args(extra)
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

    /// Closes stdin and waits for the process: the bridge detaches, the session stays resident.
    fn close(&mut self) {
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

    /// The response to `id`: its index in `seen`.
    fn wait_response(&mut self, id: u64) -> usize {
        self.wait_for(0, &format!("the response to {id}"), |frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        })
    }

    /// `initialize` then `session/new`: the session id and the session file.
    fn open_session(&mut self) -> (String, PathBuf) {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-rewind-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let at = self.wait_response(new);
        let result = &self.seen[at]["result"];
        let id = result["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {result}"))
            .to_string();
        let path = result["_meta"][META]["saved"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.path: {result}"));
        (id, PathBuf::from(path))
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    /// A prompt, then its `end_turn` answer.
    fn ask(&mut self, session_id: &str, text: &str) {
        let id = self.prompt(session_id, text);
        let at = self.wait_response(id);
        assert_eq!(
            self.seen[at]["result"]["stopReason"], "end_turn",
            "the prompt {text:?} is answered: {}",
            self.seen[at]
        );
    }

    fn rewind(&mut self, session_id: &str, to: &str) -> u64 {
        self.request(REWIND, &json!({ "sessionId": session_id, "to": to }))
    }

    /// The response to `id`, which must be `{}`.
    fn accepted(&mut self, id: u64) {
        let at = self.wait_response(id);
        assert_eq!(
            self.seen[at]["result"],
            json!({}),
            "the rewind is accepted: {}",
            self.seen[at]
        );
    }

    /// The error of the response to `id`: its code and its `data.details`.
    fn refused(&mut self, id: u64) -> (Option<i64>, String) {
        let at = self.wait_response(id);
        let error = &self.seen[at]["error"];
        (
            error["code"].as_i64(),
            error["data"]["details"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    /// The daemon's id of the only live session.
    fn daemon_session_id(&self) -> String {
        let list = daemon_request(&self.socket, "live", &json!({ "type": "list" }));
        let sessions = list["data"]["sessions"]
            .as_array()
            .unwrap_or_else(|| panic!("a session list: {list}"));
        assert_eq!(sessions.len(), 1, "one live session: {list}");
        sessions[0]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session id: {list}"))
            .to_string()
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

/// T1. A prompt's `turnStart` carries `rewindTo`, the session's leaf before the prompt.
#[test]
fn a_prompt_turn_start_carries_the_leaf_before_it() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let leaf = last_id(&file);
    let from = client.seen.len();
    let second = client.prompt(&session_id, "second");
    let at = client.wait_for(from, "the second prompt's turnStart", |frame| {
        origin_is(frame, "prompt")
    });
    let start = turn_start(&client.seen[at]).cloned().unwrap();
    let _ = client.wait_response(second);
    assert_eq!(
        start["rewindTo"],
        json!(leaf),
        "rewindTo is the leaf before the prompt: {start}"
    );
}

/// T2, T7. A rewind then a prompt continues from before the rewound prompt.
#[test]
fn a_rewind_then_a_prompt_continues_from_before_the_prompt() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![
            "user:first",
            "assistant:one",
            custom.as_str(),
            "user:second, changed",
            "assistant:three",
        ],
        "the branch runs to the point, then the fixed text, then the new prompt"
    );
    assert_eq!(rewound_texts(&file), vec![REWOUND.to_string()]);
    assert!(
        entries(&file)
            .iter()
            .filter_map(shown)
            .any(|row| row == "user:second"),
        "the abandoned turn stays in the file, off the branch"
    );
}

/// T3. Refused while a turn runs; the turn runs to its end.
#[test]
fn a_rewind_is_refused_while_a_turn_runs() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        cell("call-1", SLOW_CELL),
        { "text": "cell answered" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    let prompt = client.prompt(&session_id, "run the slow cell");
    wait_file(&client.path("started"));
    let rewind = client.rewind(&session_id, &to);
    let (code, details) = client.refused(rewind);
    assert_eq!(code, Some(-32603), "{details}");
    assert_eq!(details, WORKING);
    let answered = client.wait_response(prompt);
    assert_eq!(
        client.seen[answered]["result"]["stopReason"], "end_turn",
        "the turn was not stopped: {}",
        client.seen[answered]
    );
    assert!(client.path("ended").exists(), "the cell ran to its end");
}

/// T4. An entry off the branch is refused with `-32602`.
#[test]
fn a_rewind_to_an_entry_off_the_branch_is_invalid() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
    ] }));
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "first");
    let rewind = client.rewind(&session_id, "no-such-entry");
    let (code, details) = client.refused(rewind);
    assert_eq!(code, Some(-32602), "{details}");
    assert!(
        details.contains("no-such-entry is not in the conversation"),
        "{details}"
    );
}

/// T5 (a). A custom row written outside a turn, the leaf before the prompt, stays on the branch.
#[test]
fn a_custom_row_before_the_prompt_stays_after_the_rewind() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    // As `/refine` does after a turn (`session_custom.rs:286`): a custom row outside a turn.
    let daemon_session = client.daemon_session_id();
    let appended = daemon_request(
        &client.socket,
        "note",
        &json!({
            "type": "append_custom_message",
            "activeSessionId": daemon_session,
            "message": { "customType": "ya43-note", "content": "a note", "display": true },
        }),
    );
    assert_eq!(appended["success"], true, "the row is written: {appended}");
    let to = last_id(&file);
    assert_eq!(
        branch(&file).last().and_then(shown),
        Some("custom:ya43-note".to_string()),
        "the leaf before the prompt is the custom row"
    );
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![
            "user:first",
            "assistant:one",
            "custom:ya43-note",
            custom.as_str(),
            "user:second, changed",
            "assistant:three",
        ],
        "the custom row stays on the branch"
    );
}

/// T5 (b). The `[skills-changed]` row of the abandoned turn comes back once, before the prompt.
#[test]
fn a_skills_announcement_of_the_abandoned_turn_comes_back_once() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    write_skill(client.home.path());
    client.ask(&session_id, "second");
    let resources = format!("custom:{RESOURCES_TYPE}");
    assert_eq!(
        conversation(&file)
            .iter()
            .filter(|row| **row == resources)
            .count(),
        1,
        "the skill is announced with the second prompt"
    );
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![
            "user:first",
            "assistant:one",
            resources.as_str(),
            custom.as_str(),
            "user:second, changed",
            "assistant:three",
        ],
        "the announcement comes back once, then the fixed text"
    );
}

/// T5 (c). The fixed text of an earlier rewind in the abandoned turn, then two rewinds in a row:
/// one fixed text before the prompt.
#[test]
fn rewinds_in_a_row_leave_one_fixed_text() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
        { "text": "four" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, again");
    assert_eq!(rewound_texts(&file).len(), 1, "{:?}", conversation(&file));
    for _ in 0..2 {
        let rewind = client.rewind(&session_id, &to);
        client.accepted(rewind);
    }
    client.ask(&session_id, "second, once more");
    let custom = format!("custom:{REWOUND_TYPE}");
    assert_eq!(
        conversation(&file),
        vec![
            "user:first",
            "assistant:one",
            custom.as_str(),
            "user:second, once more",
            "assistant:four",
        ],
        "one fixed text"
    );
}

/// T8. The usage journal: `invalidation: {"cause": "rewind"}` on the next agent-loop call only.
#[test]
fn the_call_after_a_rewind_journals_the_invalidation() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
        { "text": "four" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    client.ask(&session_id, "third");
    let lines = journal_lines(&file);
    let mains: Vec<&Value> = lines.iter().filter(|line| line["kind"] == "main").collect();
    assert_eq!(mains.len(), 4, "{lines:?}");
    assert!(mains[0].get("invalidation").is_none(), "{lines:?}");
    assert!(mains[1].get("invalidation").is_none(), "{lines:?}");
    assert_eq!(
        mains[2]["invalidation"],
        json!({ "cause": "rewind" }),
        "{lines:?}"
    );
    assert!(mains[3].get("invalidation").is_none(), "{lines:?}");
}

/// T9. A resume after a rewind and a prompt, the daemon restarted: the branch is the new one.
#[test]
fn a_resume_after_a_rewind_keeps_the_new_branch() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        { "text": "two" },
        { "text": "three" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "second");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    client.ask(&session_id, "second, changed");
    client.close();
    shutdown_daemon(&client.socket);
    let mut resumed = client.resume(&file);
    let (resumed_id, resumed_file) = resumed.open_session();
    assert_eq!(resumed_file, file, "the same conversation");
    // A new worker reads the script from its start.
    resumed.ask(&resumed_id, "after");
    let rows = conversation(&file);
    let changed = rows
        .iter()
        .position(|row| row == "user:second, changed")
        .unwrap_or_else(|| panic!("the new prompt on the branch: {rows:?}"));
    assert!(
        !rows.iter().any(|row| row == "user:second"),
        "the abandoned prompt is off the branch: {rows:?}"
    );
    assert_eq!(
        rows.get(changed + 2).map(String::as_str),
        Some("user:after"),
        "the resumed prompt follows the new branch: {rows:?}"
    );
}

/// T11. A `[bash-done]` of a command started in the abandoned turn is delivered after the rewind.
#[test]
fn a_bash_notice_of_the_abandoned_turn_comes_after_the_rewind() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        { "text": "one" },
        cell("call-launch", "h = bash('sleep 3; echo trois')\nprint('launched')"),
        { "text": "launched" },
        { "text": "noticed" },
    ] }));
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let to = last_id(&file);
    client.ask(&session_id, "launch a command");
    let rewind = client.rewind(&session_id, &to);
    client.accepted(rewind);
    let from = client.seen.len();
    let wake = client.wait_for(from, "the wake's turnStart", |frame| {
        origin_is(frame, "wake")
    });
    let turn = turn_of(&client.seen[wake]).expect("a turn id");
    let _ = client.wait_for(wake, "the wake's boundary", |frame| {
        phase(frame) == Some("responseBoundary") && turn_of(frame) == Some(turn)
    });
    let rows = conversation(&file);
    assert!(
        rows.iter().any(|row| row == "custom:async_bash_completion"),
        "the notice is on the new branch: {rows:?}"
    );
    assert_eq!(
        rows.last().map(String::as_str),
        Some("assistant:noticed"),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row == "user:launch a command"),
        "the abandoned prompt is off the branch: {rows:?}"
    );
}
