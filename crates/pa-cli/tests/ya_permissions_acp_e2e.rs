// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-11, ticket 20, PR 2): `session/request_permission` end to end. The real binary
//! serves ACP over stdio on its own sandboxed daemon; the worker runs the real engine on the faux
//! provider (`"engine": "faux"`) and a real kernel, in `"manual"` (the default: [`TEST_ENV`],
//! which `.cargo/config.toml` sets to `auto` for the upstream tests, is removed).
//! a. Allowed then declined: the tool call is published `pending`, the request carries the
//!    `allow_once` and `reject_once` options; allowed, the cell runs and goes `in_progress`;
//!    declined, it does not run, its result is the `[permission-denied]` text and the turn ends.
//!    Both decisions are journaled in the worker's session file.
//! b. `session/cancel` while a request waits: the turn answers `cancelled`, the cell does not run,
//!    its tool call ends `failed`, the decision is journaled as `cancelled`. The same answer
//!    without `session/cancel` withdraws the request too: the turn ends, the call ends `failed`.
//! c. The client disconnects while a request waits: the cell does not run, its result is an error
//!    (the refusal text, or the abort text when the bridge's teardown aborts the turn first) and
//!    the turn stops (the model gets no other request); journaled as `cancelled`. A second client
//!    stays attached without answering (as the TUI does): it does not keep the request waiting.
//! d. The `permissions` selector: `auto` runs a cell without a request, back to `manual` asks
//!    again; both changes are journaled.
//! e. yusAgent (YA-10, ticket 24): the worker is killed between two turns and relaunched by the
//!    supervisor: the next request still reaches the ACP client.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const TIMEOUT: Duration = Duration::from_mins(2);
const DENIED: &str = "[permission-denied] The user declined this cell; nothing was executed.";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_permissions_e2e.rs`.
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

/// A cell that creates `name` in the session's cwd.
fn touch(id: &str, name: &str) -> Value {
    cell(id, &format!("open({name:?}, 'w').write('x')"))
}

/// The ACP child on its own home and daemon socket (as `acp_mode_e2e.rs`).
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
            .env_remove(TEST_ENV)
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

    /// Reads frames until one matches; returns it with the frames before it.
    fn wait_for(&mut self, mut matches: impl FnMut(&Value) -> bool) -> (Value, Vec<Value>) {
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
            if matches(&frame) {
                return (frame, before);
            }
            before.push(frame);
        }
    }

    fn wait_response(&mut self, id: u64) -> (Value, Vec<Value>) {
        self.wait_for(|frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        })
    }

    /// The next `session/request_permission`, with the frames before it.
    fn wait_permission_request(&mut self) -> (Value, Vec<Value>) {
        self.wait_for(|frame| frame["method"] == "session/request_permission")
    }

    /// The final status (`completed` or `failed`) of the tool call `id`, from `frames` or the
    /// frames that follow.
    fn wait_final_status(&mut self, frames: &[Value], id: &str) -> String {
        fn final_status(frame: &Value, id: &str) -> Option<String> {
            let update = &frame["params"]["update"];
            if update["sessionUpdate"] != "tool_call_update" || update["toolCallId"] != id {
                return None;
            }
            match update["status"].as_str() {
                Some(status @ ("completed" | "failed")) => Some(status.to_string()),
                _ => None,
            }
        }
        for frame in frames {
            if let Some(status) = final_status(frame, id) {
                return status;
            }
        }
        let (frame, _) = self.wait_for(|frame| final_status(frame, id).is_some());
        final_status(&frame, id).unwrap_or_default()
    }

    /// Answers a `session/request_permission` with `outcome`.
    fn answer(&mut self, request: &Value, outcome: &Value) {
        let id = request["id"].clone();
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": outcome } }));
    }

    /// `initialize` then `session/new`: the session id and its config options.
    fn open_session(&mut self) -> (String, Value) {
        let init = self.request(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-permissions-e2e", "version": "0.0.1" },
            }),
        );
        let _ = self.wait_response(init);
        let new = self.request("session/new", &json!({ "mcpServers": [] }));
        let (response, _) = self.wait_response(new);
        let session_id = response["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {response}"))
            .to_string();
        (session_id, response["result"]["configOptions"].clone())
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    fn set_permissions(&mut self, session_id: &str, value: &str) -> Value {
        let id = self.request(
            "session/set_config_option",
            &json!({ "sessionId": session_id, "configId": "permissions", "value": value }),
        );
        self.wait_response(id).0
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    /// The `data` of the `ya-permission` entries of the session files under the home.
    fn journal(&self) -> Vec<Value> {
        session_lines(&self.home.path().join(".yusagent/sessions"))
            .into_iter()
            .filter(|entry| entry["type"] == "custom" && entry["customType"] == "ya-permission")
            .map(|entry| entry["data"].clone())
            .collect()
    }

    /// Polls [`Self::journal`] until `done` holds.
    fn journal_until(&self, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let journal = self.journal();
            if done(&journal) || Instant::now() >= deadline {
                return journal;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
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

/// A second client on the daemon socket, attached to the (only) session without
/// `yaPermissionAsker`, as the TUI attaches. The connection stays open while it is held.
fn attach_watcher(
    socket: &Path,
) -> Box<dyn pa_types::platform::transport::BlockingTransportStream> {
    let mut writer =
        pa_types::platform::transport::connect_blocking(socket).expect("daemon socket");
    let reader = writer.try_clone_box().expect("daemon socket clone");
    let _ = reader.set_read_timeout(TIMEOUT);
    let mut lines = BufReader::new(reader).lines();
    let mut call = |id: &str, command: Value| -> Value {
        let frame = json!({
            "type": "command", "id": id, "clientId": "watcher",
            "protocol": { "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION },
            "command": command,
        });
        writeln!(writer, "{frame}").expect("daemon frame");
        writer.flush().expect("daemon flush");
        loop {
            let line = lines
                .next()
                .expect("the daemon answers")
                .expect("a daemon line");
            let frame: Value = serde_json::from_str(&line).expect("daemon JSON");
            if frame["id"] == json!(id) {
                return frame;
            }
        }
    };
    let list = call(
        "watch-list",
        json!({ "type": "list", "includeClientOwned": true }),
    );
    let active_session_id = list["data"]["sessions"][0]["activeSessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("a live session: {list}"))
        .to_string();
    let attached = call(
        "watch-attach",
        json!({ "type": "attach", "activeSessionId": active_session_id }),
    );
    assert_eq!(attached["success"], true, "{attached}");
    writer
}

fn decisions(journal: &[Value]) -> Vec<String> {
    journal
        .iter()
        .filter_map(|data| data["decision"].as_str().map(str::to_string))
        .collect()
}

/// The `sessionUpdate`s of `kind` for the tool call `id`.
fn tool_updates<'a>(frames: &'a [Value], kind: &str, id: &str) -> Vec<&'a Value> {
    frames
        .iter()
        .map(|frame| &frame["params"]["update"])
        .filter(|update| update["sessionUpdate"] == kind && update["toolCallId"] == id)
        .collect()
}

fn permissions_option(options: &Value) -> Value {
    options
        .as_array()
        .and_then(|options| options.iter().find(|option| option["id"] == "permissions"))
        .cloned()
        .unwrap_or_else(|| panic!("a permissions selector: {options}"))
}

/// a. Allowed, then declined.
#[test]
fn a_call_is_allowed_then_a_call_is_declined() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-allow", "allowed.txt"),
        { "text": "the first cell ran" },
        touch("call-deny", "declined.txt"),
        { "text": "never requested: the turn stops after a refusal" },
    ] }));
    let (session_id, options) = client.open_session();
    assert_eq!(permissions_option(&options)["currentValue"], "manual");

    let prompt = client.prompt(&session_id, "create allowed.txt");
    let (request, before) = client.wait_permission_request();
    let started = tool_updates(&before, "tool_call", "call-allow");
    assert_eq!(started.len(), 1, "{before:?}");
    assert_eq!(
        started[0]["status"], "pending",
        "pending until the decision"
    );
    let params = &request["params"];
    assert_eq!(params["sessionId"], session_id.as_str());
    assert_eq!(params["toolCall"]["toolCallId"], "call-allow");
    assert_eq!(params["toolCall"]["status"], "pending");
    let kinds: Vec<&str> = params["options"]
        .as_array()
        .expect("options")
        .iter()
        .filter_map(|option| option["kind"].as_str())
        .collect();
    assert_eq!(kinds, vec!["allow_once", "reject_once"]);
    let allow = params["options"][0]["optionId"].clone();
    client.answer(
        &request,
        &json!({ "outcome": "selected", "optionId": allow }),
    );
    let (response, frames) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    let progress = tool_updates(&frames, "tool_call_update", "call-allow");
    assert_eq!(
        progress.first().map(|update| &update["status"]),
        Some(&json!("in_progress"))
    );
    assert_eq!(
        progress.last().map(|update| &update["status"]),
        Some(&json!("completed"))
    );
    assert!(client.path("allowed.txt").exists(), "the allowed cell ran");

    let prompt = client.prompt(&session_id, "create declined.txt");
    let (request, _) = client.wait_permission_request();
    assert_eq!(request["params"]["toolCall"]["toolCallId"], "call-deny");
    let reject = request["params"]["options"][1]["optionId"].clone();
    client.answer(
        &request,
        &json!({ "outcome": "selected", "optionId": reject }),
    );
    let (response, frames) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    let ended = tool_updates(&frames, "tool_call_update", "call-deny");
    let last = ended.last().expect("the declined call's end");
    assert_eq!(last["status"], "failed");
    assert!(last.to_string().contains("[permission-denied]"), "{last}");
    assert!(
        !client.path("declined.txt").exists(),
        "the declined cell did not run"
    );
    assert!(
        !frames
            .iter()
            .any(|frame| frame.to_string().contains("never requested")),
        "the turn stopped after the refusal: {frames:?}"
    );

    let journal = client.journal_until(|journal| decisions(journal).len() >= 2);
    assert_eq!(decisions(&journal), vec!["allow", "deny"], "{journal:?}");
}

/// b. `session/cancel` while a request waits.
#[test]
fn a_cancel_while_a_request_waits_stops_the_turn() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-cancel", "cancelled.txt"),
        { "text": "never requested" },
    ] }));
    let (session_id, _) = client.open_session();
    let prompt = client.prompt(&session_id, "create cancelled.txt");
    let (request, _) = client.wait_permission_request();
    client.notify("session/cancel", &json!({ "sessionId": session_id }));
    // The client answers `cancelled` to a request still pending after its cancel (ACP).
    client.answer(&request, &json!({ "outcome": "cancelled" }));
    let (response, frames) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "cancelled", "{response}");
    assert_eq!(
        client.wait_final_status(&frames, "call-cancel"),
        "failed",
        "the call reaches a final state"
    );
    assert!(
        !client.path("cancelled.txt").exists(),
        "the cell did not run"
    );
    let journal = client.journal_until(|journal| !decisions(journal).is_empty());
    assert_eq!(decisions(&journal), vec!["cancelled"], "{journal:?}");
}

/// b'. The client answers `cancelled` without `session/cancel`: the request is withdrawn.
#[test]
fn a_cancelled_answer_withdraws_the_request() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-withdrawn", "withdrawn.txt"),
        { "text": "never requested" },
    ] }));
    let (session_id, _) = client.open_session();
    let prompt = client.prompt(&session_id, "create withdrawn.txt");
    let (request, _) = client.wait_permission_request();
    client.answer(&request, &json!({ "outcome": "cancelled" }));
    let (response, frames) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert_eq!(
        client.wait_final_status(&frames, "call-withdrawn"),
        "failed",
        "the call reaches a final state"
    );
    assert!(
        !frames
            .iter()
            .any(|frame| frame.to_string().contains("never requested")),
        "the turn stopped: {frames:?}"
    );
    assert!(
        !client.path("withdrawn.txt").exists(),
        "the cell did not run"
    );
    let journal = client.journal_until(|journal| !decisions(journal).is_empty());
    assert_eq!(decisions(&journal), vec!["cancelled"], "{journal:?}");
}

/// c. The client disconnects while a request waits.
#[test]
fn a_disconnect_while_a_request_waits_declines_the_call() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-gone", "gone.txt"),
        { "text": "never requested" },
    ] }));
    let (session_id, _) = client.open_session();
    // A second client stays attached through the disconnect, without answering (as the TUI).
    let _watcher = attach_watcher(&client.socket);
    let _prompt = client.prompt(&session_id, "create gone.txt");
    let _ = client.wait_permission_request();
    // EOF: the editor went away.
    client.stdin = None;
    let journal = client.journal_until(|journal| !decisions(journal).is_empty());
    assert_eq!(decisions(&journal), vec!["cancelled"], "{journal:?}");
    let messages = || {
        session_lines(&client.home.path().join(".yusagent/sessions"))
            .into_iter()
            .filter(|entry| entry["type"] == "message")
            .map(|entry| entry["message"].clone())
            .collect::<Vec<_>>()
    };
    let result = || {
        messages()
            .into_iter()
            .find(|message| message["role"] == "toolResult" && message["toolCallId"] == "call-gone")
    };
    let deadline = Instant::now() + TIMEOUT;
    while result().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    let result = result().expect("the call's result");
    assert_eq!(result["isError"], true, "{result}");
    // Two paths race on a disconnect, both a refusal that stops the turn: the worker withdraws
    // the request when the client detaches (the fixed refusal text), or the bridge's teardown
    // aborts the turn first (the abort text). Seen both ways in CI (runs 44721542873 and
    // 44723659629).
    let text = result["content"].to_string();
    assert!(
        text.contains(DENIED) || text.contains("Request was aborted"),
        "{result}"
    );
    assert!(!client.path("gone.txt").exists(), "the cell did not run");
    // The turn stopped: the model's next answer was never requested.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !messages()
            .iter()
            .any(|message| message.to_string().contains("never requested")),
        "the turn stopped after the call"
    );
}

/// d. The selector: `auto` runs without a request, back to `manual` asks again.
#[test]
fn the_permissions_selector_switches_to_auto_and_back() {
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-auto", "auto.txt"),
        { "text": "ran without a request" },
        touch("call-manual", "manual.txt"),
        { "text": "ran after the request" },
    ] }));
    let (session_id, options) = client.open_session();
    assert_eq!(permissions_option(&options)["currentValue"], "manual");
    let invalid = client.set_permissions(&session_id, "always");
    assert!(invalid.get("error").is_some(), "{invalid}");

    let switched = client.set_permissions(&session_id, "auto");
    assert_eq!(
        permissions_option(&switched["result"]["configOptions"])["currentValue"],
        "auto",
        "{switched}"
    );
    let prompt = client.prompt(&session_id, "create auto.txt");
    let (response, frames) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(
        !frames
            .iter()
            .any(|frame| frame["method"] == "session/request_permission"),
        "no request in auto: {frames:?}"
    );
    assert_eq!(
        tool_updates(&frames, "tool_call", "call-auto")[0]["status"],
        "in_progress"
    );
    assert!(client.path("auto.txt").exists());

    let switched = client.set_permissions(&session_id, "manual");
    assert_eq!(
        permissions_option(&switched["result"]["configOptions"])["currentValue"],
        "manual",
        "{switched}"
    );
    let prompt = client.prompt(&session_id, "create manual.txt");
    let (request, _) = client.wait_permission_request();
    assert_eq!(request["params"]["toolCall"]["toolCallId"], "call-manual");
    client.answer(
        &request,
        &json!({ "outcome": "selected", "optionId": "allow-once" }),
    );
    let (response, _) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(client.path("manual.txt").exists());

    let journal = client.journal_until(|journal| journal.len() >= 3);
    let changes: Vec<(String, String)> = journal
        .iter()
        .filter(|data| data.get("previousMode").is_some())
        .map(|data| {
            (
                data["previousMode"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                data["mode"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        vec![
            ("manual".to_string(), "auto".to_string()),
            ("auto".to_string(), "manual".to_string()),
        ],
        "{journal:?}"
    );
    assert_eq!(decisions(&journal), vec!["allow"], "{journal:?}");
}

/// The pid of the session's worker, from its persisted descriptor under the agent dir
/// (`daemon-workers/<socket hash>/*.json`, as `pa-daemon/tests/orphan_journal_e2e.rs`); `None`
/// while no descriptor carries a live pid other than `not`.
fn worker_pid(agent_dir: &Path, not: Option<i32>) -> Option<i32> {
    let entries = std::fs::read_dir(agent_dir.join("daemon-workers")).ok()?;
    for dir in entries.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Ok(descriptor) = std::fs::read_to_string(&path)
                .map_err(|_| ())
                .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|_| ()))
            else {
                continue;
            };
            let pid = descriptor["pid"]
                .as_i64()
                .and_then(|pid| i32::try_from(pid).ok());
            if let Some(pid) = pid.filter(|pid| *pid > 0 && Some(*pid) != not) {
                return Some(pid);
            }
        }
    }
    None
}

/// Polls [`worker_pid`] until it finds one.
fn wait_worker_pid(agent_dir: &Path, not: Option<i32>) -> i32 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(pid) = worker_pid(agent_dir, not) {
            return pid;
        }
        assert!(Instant::now() < deadline, "no worker pid (not {not:?})");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// e. yusAgent (YA-10, ticket 24): the worker dies between two turns and the supervisor relaunches
/// it. The relaunch replays the durable `create`, not the clients' `attach`: without the bridge's
/// `asker` action before each prompt, the next cell's request had no client to answer and was
/// declined at once. With it, the request reaches the ACP client.
#[test]
fn a_relaunched_worker_still_asks_the_acp_client() {
    // The relaunched worker reads the script again: whether it starts over or goes on, its next
    // answer is a cell.
    let mut client = Client::spawn(&json!({ "engine": "faux", "responses": [
        touch("call-relaunch", "relaunch.txt"),
        { "text": "first turn done" },
        touch("call-relaunch", "relaunch.txt"),
        { "text": "second turn done" },
    ] }));
    let (session_id, _) = client.open_session();
    let prompt = client.prompt(&session_id, "create relaunch.txt");
    let (request, _) = client.wait_permission_request();
    client.answer(
        &request,
        &json!({ "outcome": "selected", "optionId": "allow-once" }),
    );
    let (response, _) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(client.path("relaunch.txt").exists(), "the first cell ran");
    std::fs::remove_file(client.path("relaunch.txt")).unwrap();

    let agent_dir = client.home.path().join(".yusagent");
    let first = wait_worker_pid(&agent_dir, None);
    // SAFETY: a signal to the test's own worker process.
    assert_eq!(
        unsafe { libc::kill(first, libc::SIGKILL) },
        0,
        "kill {first}"
    );
    let second = wait_worker_pid(&agent_dir, Some(first));
    assert_ne!(first, second);

    let prompt = client.prompt(&session_id, "create relaunch.txt again");
    let (frame, before) = client.wait_for(|frame| {
        frame["method"] == "session/request_permission"
            || (frame["id"] == json!(prompt)
                && (frame.get("result").is_some() || frame.get("error").is_some()))
    });
    assert_eq!(
        frame["method"], "session/request_permission",
        "the relaunched worker asked the ACP client: {frame}; before: {before:?}"
    );
    client.answer(
        &frame,
        &json!({ "outcome": "selected", "optionId": "allow-once" }),
    );
    let (response, _) = client.wait_response(prompt);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(client.path("relaunch.txt").exists(), "the second cell ran");
}
