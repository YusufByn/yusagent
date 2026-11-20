//! yusAgent (YA-11, ticket 22): the card of a tool call while its permission request waits.
//!
//! The engine emits `tool_execution_start` before it asks (`pa-agent` `agent_loop/tools.rs`), and
//! the worker can publish the request before that start (`pa-daemon` `acp/ya_permissions.rs`: the
//! engine's events go through the worker's own queue). Whatever the order, while the request
//! waits the card is not started (no checkmark, no timer) and the loader says "Awaiting
//! permission", never "Executing"; an allowed call starts at the answer (its live timer does not
//! count the wait); a denied call ends in error without ever showing a success or a duration.
//! A mock daemon plays the frames: no worker, no kernel, no model. Listed in
//! `docs/fork-changes.md`.
// The mock's serve loop is one flat request table, as `tool_card_live_timer_headless.rs`.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use pa_tui::interactive::{
    run_interactive, HeadlessPlan, HeadlessStep, InteractiveOptions, ModelSelection,
    SessionSelection, UiMode,
};
use serde_json::{json, Value};

/// The panel's title.
const PANEL: &str = "Permission · python_repl";
const AWAITING: &str = "Awaiting permission";
const DRAINED: &str = "ya-card-drained";
/// The note of a request the worker withdrew.
const WITHDRAWN: &str = "closed elsewhere (cancelled)";
const WAITING_CODE: &str = "print('awaiting')";
const ALLOWED_CODE: &str = "print('allowed')";
const DENIED_CODE: &str = "print('denied')";
/// How long a request waits before its answer: longer than any timer the card may show after.
const WAIT_MS: u64 = 2000;

#[derive(Clone, Copy)]
enum Scenario {
    /// `tool_execution_start`, then the request; withdrawn after a wait (the turn must end).
    StartThenRequest,
    /// The request overtakes the start; withdrawn after a wait.
    RequestThenStart,
    /// A call allowed after a wait, then a call denied.
    AllowThenDeny,
}

struct MockSupervisor {
    listener: UnixListener,
    scenario: Scenario,
}

impl MockSupervisor {
    fn bind(socket: &std::path::Path, scenario: Scenario) -> Self {
        MockSupervisor {
            listener: UnixListener::bind(socket).expect("bind mock socket"),
            scenario,
        }
    }

    /// Serves one client connection until it closes (bounded wait for the connection).
    fn serve(self) {
        self.listener
            .set_nonblocking(true)
            .expect("nonblocking mock listener");
        let idle_until = std::time::Instant::now() + Duration::from_millis(1500);
        let (stream, _) = loop {
            match self.listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= idle_until {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        };
        // macOS inherits O_NONBLOCK from the listener on accept; the request loop blocks.
        stream.set_nonblocking(false).expect("blocking mock stream");
        let mut writer = stream.try_clone().expect("clone mock socket");
        let mut reader = BufReader::new(stream);
        write_json(
            &mut writer,
            &json!({
                "type": "daemon_hello",
                "protocol": { "name": "prime-agent.daemon", "version": 7 },
                "serverCapabilities": [],
                "clientId": "mock",
            }),
        );
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let Ok(envelope) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            let id = envelope.get("id").and_then(Value::as_str).unwrap_or("");
            let command_type = envelope
                .pointer("/command/type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match command_type.as_str() {
                "create" => write_json(
                    &mut writer,
                    &json!({
                        "type": "response",
                        "id": id,
                        "command": "create",
                        "success": true,
                        "data": {
                            "activeSessionId": "s1",
                            "id": "s1",
                            "sessionId": "sess-1",
                            "sessionFile": "/tmp/sess-1.jsonl",
                        },
                    }),
                ),
                "attach" => {
                    write_json(&mut writer, &attach_data(id));
                    // The frames queue on the socket behind the attach response, from a side
                    // thread (the reader loop keeps answering the client's requests).
                    let scenario = self.scenario;
                    let mut w = writer.try_clone().expect("clone event socket");
                    std::thread::spawn(move || play(scenario, &mut w));
                }
                _ => write_json(
                    &mut writer,
                    &json!({
                        "type": "response",
                        "id": id,
                        "command": command_type,
                        "success": true,
                        "data": {},
                    }),
                ),
            }
        }
    }
}

fn play(scenario: Scenario, w: &mut UnixStream) {
    match scenario {
        Scenario::StartThenRequest => {
            tool_start(w, "call_wait", WAITING_CODE);
            request(w, "ya-permission-1", "call_wait", WAITING_CODE);
            withdraw_after_the_wait(w);
        }
        Scenario::RequestThenStart => {
            request(w, "ya-permission-1", "call_wait", WAITING_CODE);
            tool_start(w, "call_wait", WAITING_CODE);
            withdraw_after_the_wait(w);
        }
        Scenario::AllowThenDeny => {
            tool_start(w, "call_allow", ALLOWED_CODE);
            request(w, "ya-permission-1", "call_allow", ALLOWED_CODE);
            std::thread::sleep(Duration::from_millis(WAIT_MS));
            resolved(w, "ya-permission-1", "call_allow", "allow");
            event(
                w,
                &json!({
                    "type": "tool_execution_update",
                    "toolCallId": "call_allow",
                    "partialResult": { "content": [{ "type": "text", "text": "allowed output" }] },
                }),
            );
            std::thread::sleep(Duration::from_millis(600));
            tool_end(w, "call_allow", "allowed output", false);
            tool_start(w, "call_deny", DENIED_CODE);
            request(w, "ya-permission-2", "call_deny", DENIED_CODE);
            std::thread::sleep(Duration::from_millis(600));
            resolved(w, "ya-permission-2", "call_deny", "deny");
            tool_end(
                w,
                "call_deny",
                "[permission-denied] The user declined this cell; nothing was executed.",
                true,
            );
            event(w, &json!({ "type": "turn_end" }));
            drain_barrier(w);
        }
    }
}

/// The wait, then the worker withdraws the request and the turn ends (the headless run waits for
/// the turn to settle).
fn withdraw_after_the_wait(w: &mut UnixStream) {
    std::thread::sleep(Duration::from_millis(WAIT_MS));
    resolved(w, "ya-permission-1", "call_wait", "cancelled");
    tool_end(
        w,
        "call_wait",
        "[permission-denied] The user declined this cell; nothing was executed.",
        true,
    );
    event(w, &json!({ "type": "turn_end" }));
    drain_barrier(w);
}

fn write_json(writer: &mut UnixStream, value: &Value) {
    let mut line = serde_json::to_string(value).expect("serialize mock frame");
    line.push('\n');
    writer.write_all(line.as_bytes()).expect("write mock frame");
    writer.flush().expect("flush mock frame");
}

fn event(writer: &mut UnixStream, payload: &Value) {
    write_json(
        writer,
        &json!({ "type": "session_event", "activeSessionId": "s1", "event": payload }),
    );
}

fn tool_start(writer: &mut UnixStream, call_id: &str, code: &str) {
    event(
        writer,
        &json!({
            "type": "tool_execution_start",
            "toolCallId": call_id,
            "toolName": "python_repl",
            "args": { "code": code },
        }),
    );
}

fn request(writer: &mut UnixStream, request_id: &str, call_id: &str, code: &str) {
    event(
        writer,
        &json!({
            "type": "ya_permission_request",
            "requestId": request_id,
            "toolCallId": call_id,
            "toolName": "python_repl",
            "args": { "code": code },
        }),
    );
}

fn resolved(writer: &mut UnixStream, request_id: &str, call_id: &str, outcome: &str) {
    event(
        writer,
        &json!({
            "type": "ya_permission_resolved",
            "requestId": request_id,
            "toolCallId": call_id,
            "outcome": outcome,
        }),
    );
}

fn tool_end(writer: &mut UnixStream, call_id: &str, text: &str, is_error: bool) {
    event(
        writer,
        &json!({
            "type": "tool_execution_end",
            "toolCallId": call_id,
            "toolName": "python_repl",
            "result": { "content": [{ "type": "text", "text": text }], "isError": is_error },
            "isError": is_error,
        }),
    );
}

/// A visible user row: once rendered, the frames written before it were processed.
fn drain_barrier(writer: &mut UnixStream) {
    event(
        writer,
        &json!({
            "type": "message_start",
            "message": { "role": "user", "content": [{ "type": "text", "text": DRAINED }] },
        }),
    );
}

/// A streaming mid-turn snapshot: the turn (and the loader) is live when the call starts.
fn attach_data(id: &str) -> Value {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_millis();
    json!({
        "type": "response",
        "id": id,
        "command": "attach",
        "success": true,
        "data": {
            "protocol": { "name": "prime-agent.daemon", "version": 7 },
            "activeSessionId": "s1",
            "snapshot": {
                "activeSessionId": "s1",
                "summary": { "id": "s1", "cwd": "/tmp" },
                "state": {
                    "activeSessionId": "s1",
                    "cwd": "/tmp",
                    "sessionId": "sess-1",
                    "sessionName": "permission card session",
                    "model": null,
                    "isStreaming": true,
                    "isCompacting": false,
                    "sessionActions": { "queuedCount": 0, "steering": [], "followUps": [] },
                },
                "messages": [{
                    "role": "user",
                    "content": [{ "type": "text", "text": "run the cell" }],
                    "timestamp": now_ms,
                }],
                "lastEventSequence": 0,
                "lastEventCursor": null,
            },
            "client": { "id": "mock", "capabilities": [] },
            "lastEventSequence": 0,
            "lastEventCursor": null,
        },
    })
}

fn options(socket: PathBuf) -> InteractiveOptions {
    InteractiveOptions {
        models: None,
        socket_path: socket,
        cwd: PathBuf::from("/tmp"),
        session_dir: None,
        script_path: None,
        model_selection: ModelSelection::default(),
        model_catalog: Vec::new(),
        model_configured_providers: std::collections::HashSet::default(),
        model_recent_models: Vec::new(),
        default_thinking_level: None,
        no_session: false,
        session: SessionSelection::New,
        initial_message: None,
        show_images: true,
        fullscreen_mouse: false,
        theme: "prime".to_string(),
        code_block_indent: "  ".to_string(),
        tree_filter_mode: String::new(),
        branch_summary_skip_prompt: false,
        version: "0.0.0".to_string(),
        onboarding: None,
        telemetry_disabled: None,
        client_auth: None,
        traces: None,
        provider_auth: None,
        update_commands: None,
        telemetry: None,
        keybindings: pa_tui::keybindings::KeybindingsManager::new(),
        session_rlm_depth: None,
        prompt_stash: std::sync::Arc::default(),
        session_has_children: false,
        restore_dock_focus: false,
        client_settings: None,
    }
}

/// Runs the TUI headless against the mock; the frames it painted.
fn run(scenario: Scenario, needles: &[&str]) -> Vec<String> {
    // The ambient TMUX variable adds a startup notice to the transcript.
    std::env::remove_var("TMUX");
    let dir = tempfile::TempDir::new().expect("temp dir");
    let socket = dir.path().join("tui.sock");
    let supervisor = MockSupervisor::bind(&socket, scenario);
    let handle = std::thread::spawn(move || supervisor.serve());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let mut steps: Vec<HeadlessStep> = needles
        .iter()
        .map(|needle| HeadlessStep::WaitRender {
            needle: (*needle).to_string(),
            timeout_ms: 10_000,
        })
        .collect();
    steps.push(HeadlessStep::WaitIdle { timeout_ms: 5000 });
    let plan = HeadlessPlan {
        steps,
        width: 120,
        height: 40,
    };
    let outcome = runtime
        .block_on(run_interactive(options(socket), UiMode::Headless(plan)))
        .expect("interactive run");
    handle.join().expect("mock supervisor finished");
    // A timed-out WaitRender only notes: the barrier must have rendered.
    assert!(
        outcome.frames.iter().any(|frame| frame.contains(DRAINED)),
        "the drain barrier rendered:\n{}",
        outcome.frames.join("\n===\n")
    );
    outcome.frames
}

/// The card's summary line (`<marker> python · <code> …`) in one frame.
fn card_line<'a>(frame: &'a str, code: &str) -> Option<&'a str> {
    let needle = format!("python · {code}");
    frame.lines().find(|line| line.contains(&needle))
}

/// The durations a card line shows (`123ms`, `1.2s`), in milliseconds.
fn durations_ms(line: &str) -> Vec<f64> {
    line.split(" · ")
        .filter_map(|part| {
            let part = part.trim();
            if let Some(ms) = part.strip_suffix("ms") {
                ms.parse::<f64>().ok()
            } else {
                part.strip_suffix('s')
                    .filter(|seconds| seconds.contains('.'))
                    .and_then(|seconds| seconds.parse::<f64>().ok())
                    .map(|seconds| seconds * 1000.0)
            }
        })
        .collect()
}

/// While the request waits (from the frame that opened the panel to its withdrawal): the card
/// is not started, the loader awaits the permission.
fn assert_not_started_while_waiting(frames: &[String]) {
    let opened = frames
        .iter()
        .position(|frame| frame.contains(PANEL))
        .unwrap_or_else(|| panic!("the panel opened:\n{}", frames.join("\n===\n")));
    let withdrawn = frames
        .iter()
        .position(|frame| frame.contains(WITHDRAWN))
        .unwrap_or_else(|| panic!("the request was withdrawn:\n{}", frames.join("\n===\n")));
    let waiting = &frames[opened..withdrawn];
    let all = waiting.join("\n===\n");
    for frame in waiting {
        // A request that overtakes its start opens the panel before the card exists.
        if let Some(line) = card_line(frame, WAITING_CODE) {
            assert!(
                !line.contains('✓'),
                "a waiting call never shows the success mark: {line}\n{all}"
            );
            assert!(
                durations_ms(line).is_empty(),
                "a waiting call shows no timer: {line}\n{all}"
            );
        }
        assert!(
            !frame.contains("Executing"),
            "the loader never says Executing while the request waits:\n{frame}"
        );
    }
    let last = waiting.last().expect("a frame while the request waits");
    let line =
        card_line(last, WAITING_CODE).unwrap_or_else(|| panic!("the card is on screen:\n{last}"));
    assert!(line.contains('◇'), "the card is not started: {line}");
    assert!(
        last.contains(AWAITING),
        "the loader says {AWAITING:?}:\n{last}"
    );
}

#[test]
fn a_call_waiting_for_permission_is_not_shown_as_started() {
    let frames = run(Scenario::StartThenRequest, &[PANEL, WITHDRAWN, DRAINED]);
    assert_not_started_while_waiting(&frames);
}

#[test]
fn a_request_that_overtakes_its_start_keeps_the_card_not_started() {
    let frames = run(Scenario::RequestThenStart, &[PANEL, WITHDRAWN, DRAINED]);
    assert_not_started_while_waiting(&frames);
}

#[test]
fn an_allowed_call_starts_at_the_answer_and_a_denied_one_never_runs() {
    let frames = run(
        Scenario::AllowThenDeny,
        &[PANEL, "closed elsewhere (allow)", DRAINED],
    );
    let all = frames.join("\n===\n");

    // The wait of the first call: not started.
    let opened = frames
        .iter()
        .position(|frame| frame.contains(PANEL))
        .expect("the first panel opened");
    let allowed = frames
        .iter()
        .position(|frame| frame.contains("closed elsewhere (allow)"))
        .expect("the allow closed the panel");
    for frame in &frames[opened..allowed] {
        let line = card_line(frame, ALLOWED_CODE).expect("the first card is on screen");
        assert!(!line.contains('✓'), "not started while waiting: {line}");
        assert!(
            durations_ms(line).is_empty(),
            "no timer while waiting: {line}"
        );
    }

    // After the allow: the live timer counts from the answer, not from the start (K3).
    let ticks: Vec<f64> = frames[allowed..]
        .iter()
        .filter_map(|frame| card_line(frame, ALLOWED_CODE))
        .flat_map(durations_ms)
        .collect();
    assert!(
        !ticks.is_empty(),
        "the allowed call shows its live timer:\n{all}"
    );
    assert!(
        ticks.iter().all(|ms| *ms < 1500.0),
        "the timer starts at the allow, after a {WAIT_MS} ms wait: {ticks:?}\n{all}"
    );
    let last = frames.last().expect("a last frame");
    let line = card_line(last, ALLOWED_CODE).expect("the allowed card at the end");
    assert!(line.contains('✓'), "the allowed call succeeded: {line}");

    // The denied call: an error, never a success, never a timer.
    let second = allowed
        + frames[allowed..]
            .iter()
            .position(|frame| frame.contains(PANEL))
            .expect("the second panel opened");
    for frame in &frames[second..] {
        if let Some(line) = card_line(frame, DENIED_CODE) {
            assert!(
                !line.contains('✓'),
                "a denied call never succeeds: {line}\n{all}"
            );
            assert!(
                durations_ms(line).is_empty(),
                "a denied call shows no timer: {line}\n{all}"
            );
        }
    }
    let line = card_line(last, DENIED_CODE).expect("the denied card at the end");
    assert!(line.contains('✗'), "the denied call ends in error: {line}");
}
