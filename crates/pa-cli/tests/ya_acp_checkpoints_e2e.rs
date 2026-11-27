// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-18, ticket 43, PR 3a): workspace snapshots in a shadow git repository, their
//! preview and their restore (`docs/tickets/43-retour-arriere.md`, §P, §P.12.7). The real binary
//! serves ACP over stdio on its own sandboxed daemon; the worker runs the real engine on the faux
//! provider and a real kernel, on the product's defaults (prompt version 2, as
//! `ya_prompt_v2_e2e.rs`), so that the cells may call `bash()` and `edit()`. T23 runs on a loopback
//! `anthropic-messages` model that keeps each request's body, as `ya_acp_rewind_e2e.rs`: what the
//! model received is checked there, not the session file (ticket 43, §L).
//!
//! The workspace is the test's home, which holds the agent directory (`.yusagent`): every test
//! runs with the agent directory inside the workspace (T27's case). What is written on disk is
//! read with `git` on the shadow repository (`<agent>/checkpoints/<digest>/`, refs
//! `refs/ya/<session>/{debut/<rewindTo>,fin,avant-restauration/<n>}`, §P.4, §P.12.3).
//!
//! T20. A prompt: the snapshot before it (key: its `rewindTo`) and the one after its turn.
//! T21. Restore of the files only: writes by `open()`, `bash()` and `edit()` and a removal undone;
//!      an ignored file and a file over the cap per file untouched, the latter listed as uncovered.
//! T22. A file changed outside the agent: listed, the restore refused without `force`; with it,
//!      the state just before the restore kept in `avant-restauration/<n>`.
//! T22b. Right after a restore, nothing is listed as changed outside the agent (answer Q-3a-2).
//! T23. What the model receives after a restore (files and conversation; files only).
//! T24. The user's repository: no commit, branch or stash; its status back to before the prompt.
//! T26. No git (`PRIME_AGENT_TEST_CHECKPOINTS_GIT`, answer Q-3a-1): snapshots off with the
//!      message; the rewind of the conversation still works.
//! T27. The agent directory inside the workspace: no credential in the objects, none restored.
//! T28. Refs older than 30 days and the repository of a vanished workspace are purged.
//! T29. Deleting a conversation deletes its refs and its index.
//! T31. Workspace caps at the first snapshot: 20 000 files, then the total size (`512 MiB`).
//! T32. The workspace's and the user's git configuration start no program.
//! T33. A restore is refused while a turn runs; the turn runs to its end.
//! T33b. (push B, accepted change of §P.14) A restore is refused while another client that
//!      answers the session's permission requests is attached (the I2 control's attach).
//! T34. A nested repository is neither captured nor touched by a restore.
//! T35. The count kept up to date from the changed paths: a folder created by hand between two
//!      turns takes the workspace over a cap; snapshots go off with the message (answer of §P.13).
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
/// How long a test waits for a snapshot written off the turn (the end of a turn is queued).
const SNAPSHOT_WAIT: Duration = Duration::from_secs(20);
const META: &str = "ai.primeintellect.prime-agent";
const REWIND: &str = "_yus/session/rewind";
const RESTORE: &str = "_yus/session/restore";
const PREVIEW: &str = "_yus/session/restore_preview";
/// The test variable that replaces the bridge's git (answer Q-3a-1).
const TEST_GIT: &str = "PRIME_AGENT_TEST_CHECKPOINTS_GIT";
/// The fixed text the model gets after a rewind (D2, ticket 43, §H).
const REWOUND: &str = "[conversation-rewound] The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.";
/// D2's sentence when the files were restored too (§D, §P.12.5).
const FILES_RESTORED: &str = "Workspace files were restored to how they were at that point.";
/// The text after a restore of the files only (Q-P5).
const WORKSPACE_RESTORED: &str = "[workspace-restored] The user restored the workspace files to how they were before an earlier turn; files may no longer match what this conversation shows.";
/// Unharnessed's refusal while the agent works.
const WORKING: &str = "The agent is working: stop it before going back.";
/// The refusal while another window answers the session (answer 9, Q-I3).
const OTHER_WINDOW: &str =
    "This conversation is open in another window: close it there before going back.";
const MODIFIED_OUTSIDE: &str = "Files changed outside the agent since its last turn.";
const NO_GIT: &str = "Workspace snapshots are off: git was not found.";
const TOO_MANY_FILES: &str = "Workspace snapshots are off in this folder: more than 20000 files.";
const TOO_LARGE: &str = "Workspace snapshots are off in this folder: more than 512 MiB of files.";
/// The cap per file (§P.3).
const FILE_CAP: u64 = 8 * 1024 * 1024;
const SENTINEL: &str = "ya43-sentinel-checkpoints-auth-3b7e";

/// A cell that marks its start, runs three seconds, then marks its end.
const SLOW_CELL: &str = "import time\nopen('started', 'w').write('x')\ntime.sleep(3)\nopen('ended', 'w').write('x')\nprint('cell done')";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides).
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

fn meta(frame: &Value) -> &Value {
    &frame["params"]["update"]["_meta"][META]
}

fn turn_start(frame: &Value) -> Option<&Value> {
    Some(&meta(frame)["turnStart"])
        .filter(|start| frame["method"] == "session/update" && start.is_object())
}

fn origin_is(frame: &Value, origin: &str) -> bool {
    turn_start(frame).is_some_and(|start| start["origin"] == origin)
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

/// The id in the session file's header (the conversation's id, the key of its refs).
fn file_id(file: &Path) -> String {
    let text =
        std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    let header: Value =
        serde_json::from_str(text.lines().next().expect("a header line")).expect("a JSON header");
    header["id"]
        .as_str()
        .unwrap_or_else(|| panic!("a session id in {header}"))
        .to_string()
}

/// `git` for the test's own setup, on `dir`, with no system or global configuration.
fn git(dir: &Path, args: &[&str]) -> String {
    run_git(Command::new("git").arg("-C").arg(dir), args)
        .unwrap_or_else(|error| panic!("git {args:?} in {}: {error}", dir.display()))
}

/// `git` on the shadow repository `repo`; `Err` with its stderr when it fails.
fn shadow_try(repo: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg(format!("--git-dir={}", repo.display()));
    run_git(&mut command, args)
}

fn shadow(repo: &Path, args: &[&str]) -> String {
    shadow_try(repo, args)
        .unwrap_or_else(|error| panic!("git {args:?} on {}: {error}", repo.display()))
}

fn run_git(command: &mut Command, args: &[&str]) -> Result<String, String> {
    let output = command
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .map_err(|error| format!("git does not start: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// The shadow repository of `workspace`: the one under `<agent>/checkpoints` whose `workspace`
/// file names it (§P.4). Waits for it.
fn wait_repo(workspace: &Path) -> PathBuf {
    let root = workspace.join(".yusagent/checkpoints");
    let wanted = workspace.canonicalize().expect("a canonical workspace");
    let deadline = Instant::now() + SNAPSHOT_WAIT;
    loop {
        let found = std::fs::read_dir(&root).ok().and_then(|dir| {
            dir.filter_map(Result::ok)
                .map(|entry| std::fs::DirEntry::path(&entry))
                .find(|repo| {
                    std::fs::read_to_string(repo.join("workspace"))
                        .is_ok_and(|named| Path::new(named.trim()) == wanted)
                })
        });
        if let Some(repo) = found {
            return repo;
        }
        assert!(
            Instant::now() < deadline,
            "no checkpoints repository for {} under {}",
            wanted.display(),
            root.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits for `name` in `repo`: its commit id.
fn wait_ref(repo: &Path, name: &str) -> String {
    let deadline = Instant::now() + SNAPSHOT_WAIT;
    loop {
        if let Ok(id) = shadow_try(repo, &["rev-parse", "-q", "--verify", name]) {
            return id.trim().to_string();
        }
        assert!(
            Instant::now() < deadline,
            "no ref {name} in {}; refs: {:?}",
            repo.display(),
            refs_under(repo, "refs/")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The refs under `prefix`.
fn refs_under(repo: &Path, prefix: &str) -> Vec<String> {
    shadow_try(repo, &["for-each-ref", "--format=%(refname)", prefix])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The paths of the tree of `rev`.
fn tree(repo: &Path, rev: &str) -> Vec<String> {
    shadow(repo, &["ls-tree", "-r", "-z", "--name-only", rev])
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// The content of `path` in `rev`.
fn blob(repo: &Path, rev: &str, path: &str) -> String {
    shadow(repo, &["show", &format!("{rev}:{path}")])
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The `result` of a response frame, which must be there.
fn result_of<'a>(frame: &'a Value, what: &str) -> &'a Value {
    assert!(frame.get("result").is_some(), "{what} is answered: {frame}");
    &frame["result"]
}

/// The paths of a JSON list of strings.
fn strings(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Empty files `<dir>/f<i>`, `count` of them.
fn many_files(dir: &Path, count: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for index in 0..count {
        std::fs::write(dir.join(format!("f{index}")), "").unwrap();
    }
}

/// A sparse file of `size` bytes.
fn sparse(path: &Path, size: u64) {
    std::fs::File::create(path).unwrap().set_len(size).unwrap();
}

/// The model of T23: a loopback `anthropic-messages` server that keeps each request's JSON body
/// and answers each with one text block, `answer <n>` (as `ya_acp_rewind_e2e.rs`).
struct ModelServer {
    port: u16,
    bodies: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("local addr").port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let bodies = Arc::clone(&bodies);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut socket, _)) => answer_request(&mut socket, &bodies),
                        Err(_) => std::thread::sleep(Duration::from_millis(20)),
                    }
                }
            })
        };
        Self {
            port,
            bodies,
            stop,
            thread: Some(thread),
        }
    }

    /// The bodies of the agent-loop requests (those with tools), in order.
    fn requests(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .filter(|body| {
                body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            })
            .cloned()
            .collect()
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

/// One request: its body kept, a text stream answered.
fn answer_request(socket: &mut TcpStream, bodies: &Mutex<Vec<Value>>) {
    use std::fmt::Write as _;
    let _ = socket.set_nonblocking(false);
    let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
    let mut data = Vec::new();
    let mut buffer = vec![0_u8; 65_536];
    let head_end = loop {
        if let Some(at) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while data.len() < head_end + length {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }
    let body: Value = serde_json::from_slice(&data[head_end..]).unwrap_or(Value::Null);
    let count = {
        let mut bodies = bodies.lock().unwrap();
        bodies.push(body);
        bodies.len()
    };
    let usage = json!({ "input_tokens": 10, "output_tokens": 2 });
    let events = [
        (
            "message_start",
            json!({ "type": "message_start", "message": {
            "id": format!("msg_ya43_{count}"), "type": "message", "role": "assistant",
            "model": "ya43-model", "content": [], "stop_reason": null, "stop_sequence": null,
            "usage": usage } }),
        ),
        (
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
            "content_block": { "type": "text", "text": "" } }),
        ),
        (
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
            "delta": { "type": "text_delta", "text": format!("answer {count}") } }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "message_delta",
            json!({ "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null }, "usage": usage }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];
    let payload = events
        .iter()
        .fold(String::new(), |mut payload, (event, data)| {
            let _ = write!(payload, "event: {event}\ndata: {data}\n\n");
            payload
        });
    let _ = write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
}

/// The value without its `cache_control` marks (they follow the end of each request).
fn without_cache_marks(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(key, item)| (key.clone(), without_cache_marks(item)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_cache_marks).collect()),
        other => other.clone(),
    }
}

/// The messages of a request as JSON text, for counting what the model got.
fn sent(body: &Value) -> String {
    body["messages"].to_string()
}

/// The ACP child on its own home (the workspace) and daemon socket; every frame it wrote is kept.
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
    /// A new home: `setup` writes the workspace before the binary starts.
    fn spawn(script: &Value, setup: impl FnOnce(&Path)) -> Client {
        Client::spawn_with(script, &[], setup)
    }

    fn spawn_with(script: &Value, env: &[(&str, &str)], setup: impl FnOnce(&Path)) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        std::fs::write(home.path().join("worker-script.json"), script.to_string()).unwrap();
        setup(home.path());
        Client::launch(Rc::new(home), env)
    }

    /// On the loopback model `server`: no faux script, the real engine.
    fn spawn_model(server: &ModelServer, setup: impl FnOnce(&Path)) -> Client {
        let home = tempfile::TempDir::new().unwrap();
        let agent_dir = home.path().join(".yusagent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let model = json!({
            "id": "ya43-model", "name": "ya43", "reasoning": false, "input": ["text"],
            "contextWindow": 200_000, "maxTokens": 32_000,
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        });
        let providers = json!({ "providers": { "ya43": {
            "baseUrl": format!("http://127.0.0.1:{}", server.port), "apiKey": "ya43-dummy",
            "api": "anthropic-messages", "models": [model]
        } } });
        std::fs::write(agent_dir.join("models.json"), providers.to_string()).unwrap();
        std::fs::write(
            agent_dir.join("settings.json"),
            json!({ "defaultProvider": "ya43", "defaultModel": "ya43-model", "permissions": "auto" })
                .to_string(),
        )
        .unwrap();
        setup(home.path());
        Client::launch(Rc::new(home), &[])
    }

    /// Another ACP process on the same home and daemon socket.
    fn again(&self) -> Client {
        Client::launch(Rc::clone(&self.home), &[])
    }

    fn launch(home: Rc<tempfile::TempDir>, env: &[(&str, &str)]) -> Client {
        let socket = home.path().join("daemon.sock");
        let script_path = home.path().join("worker-script.json");
        let mut command = Command::new(env!("CARGO_BIN_EXE_prime-agent"));
        command
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&socket)
            .env("HOME", home.path())
            .env("PRIME_AGENT_FAUX_SCRIPT", &script_path)
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
                "15000",
            )
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            // The product's defaults (prompt version 2), as `ya_prompt_v2_e2e.rs`.
            .env_remove("RLM_MAX_DEPTH")
            .env_remove("RLM_DEPTH")
            .env_remove("PRIME_AGENT_PROMPT_VERSION")
            .env_remove(TEST_GIT)
            .current_dir(home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("binary present");
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

    /// The index in `seen` of the first frame from `from` on that matches `pred`.
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
    fn wait_response(&mut self, id: u64) -> Value {
        let at = self.wait_for(0, &format!("the response to {id}"), |frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        });
        self.seen[at].clone()
    }

    fn call(&mut self, method: &str, params: &Value) -> Value {
        let id = self.request(method, params);
        self.wait_response(id)
    }

    /// `initialize` then `session/new`: the session id and the session file.
    fn open_session(&mut self) -> (String, PathBuf) {
        let _ = self.call(
            "initialize",
            &json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "ya-acp-checkpoints-e2e", "version": "0.0.1" },
            }),
        );
        let new = self.call("session/new", &json!({ "mcpServers": [] }));
        let result = &new["result"];
        let id = result["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("a session: {new}"))
            .to_string();
        let path = result["_meta"][META]["saved"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("saved.path: {new}"));
        (id, PathBuf::from(path))
    }

    fn prompt(&mut self, session_id: &str, text: &str) -> u64 {
        self.request(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }

    /// A prompt answered `end_turn`: the `rewindTo` of its `turnStart`.
    fn ask(&mut self, session_id: &str, text: &str) -> String {
        let from = self.seen.len();
        let id = self.prompt(session_id, text);
        let at = self.wait_for(from, "the prompt's turnStart", |frame| {
            origin_is(frame, "prompt")
        });
        let start = turn_start(&self.seen[at]).cloned().unwrap();
        let answered = self.wait_response(id);
        assert_eq!(
            answered["result"]["stopReason"], "end_turn",
            "the prompt {text:?} is answered: {answered}"
        );
        start["rewindTo"]
            .as_str()
            .unwrap_or_else(|| panic!("a rewindTo: {start}"))
            .to_string()
    }

    fn preview(&mut self, session_id: &str, to: &str) -> Value {
        self.call(PREVIEW, &json!({ "sessionId": session_id, "to": to }))
    }

    fn restore(
        &mut self,
        session_id: &str,
        to: &str,
        files: bool,
        conversation: bool,
        force: bool,
    ) -> Value {
        self.call(
            RESTORE,
            &json!({
                "sessionId": session_id, "to": to, "files": files,
                "conversation": conversation, "force": force,
            }),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }

    fn workspace(&self) -> &Path {
        self.home.path()
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

/// T20. A prompt: the snapshot before it, under its `rewindTo`, and the one after its turn.
#[test]
fn a_prompt_snapshots_the_workspace_before_and_after_its_turn() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('made.txt', 'w').write('agent')\nprint('made')"),
            { "text": "done" },
        ] }),
        |home| std::fs::write(home.join("before.txt"), "user").unwrap(),
    );
    let (session_id, file) = client.open_session();
    let to = client.ask(&session_id, "make a file");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let start = wait_ref(&repo, &format!("refs/ya/{session}/debut/{to}"));
    let end = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    let before = tree(&repo, &start);
    let after = tree(&repo, &end);
    assert!(
        before.iter().any(|path| path == "before.txt")
            && !before.iter().any(|path| path == "made.txt"),
        "the snapshot before the prompt: {before:?}"
    );
    assert!(
        after.iter().any(|path| path == "made.txt"),
        "the snapshot after the turn: {after:?}"
    );
    assert_eq!(blob(&repo, &end, "made.txt"), "agent");
    assert!(
        !after.iter().any(|path| path.starts_with(".yusagent")),
        "the agent directory is never captured: {after:?}"
    );
}

/// T21. Restore of the files only: what the cell wrote by `open()`, `bash()` and `edit()` and
/// what it removed come back; an ignored file and a file over the cap are left as they are.
#[test]
fn a_restore_brings_the_files_back_and_leaves_the_rest() {
    let code = "import os\nopen('keep.txt', 'w').write('v2')\nos.remove('gone.txt')\nr = await bash('echo shell > shell.txt')\nprint(r.exit_code)\nprint(await edit('notes.txt', 'alpha', 'beta'))\nopen('ignored.log', 'a').write('+agent')\nwith open('big.bin', 'r+b') as handle:\n    handle.write(b'agent')\nprint('cell done')";
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", code),
            { "text": "done" },
        ] }),
        |home| {
            std::fs::write(home.join("keep.txt"), "v1").unwrap();
            std::fs::write(home.join("gone.txt"), "bye").unwrap();
            std::fs::write(home.join("notes.txt"), "alpha\n").unwrap();
            std::fs::write(home.join(".gitignore"), "ignored.log\n").unwrap();
            std::fs::write(home.join("ignored.log"), "i1").unwrap();
            sparse(&home.join("big.bin"), FILE_CAP + 1);
        },
    );
    let (session_id, _) = client.open_session();
    let to = client.ask(&session_id, "change the files");
    assert_eq!(read(&client.path("keep.txt")), "v2", "the cell ran");
    // Push B (Yusuf's request): the writes of `bash()` and `edit()` in a version-2 cell, seen
    // before the restore.
    assert_eq!(read(&client.path("shell.txt")), "shell\n", "bash() wrote");
    assert_eq!(read(&client.path("notes.txt")), "beta\n", "edit() wrote");
    assert!(
        !client.path("gone.txt").exists(),
        "the cell removed gone.txt"
    );
    let answer = client.restore(&session_id, &to, true, false, false);
    let result = result_of(&answer, "the restore");
    assert_eq!(read(&client.path("keep.txt")), "v1", "open() undone");
    assert_eq!(read(&client.path("gone.txt")), "bye", "the removal undone");
    assert!(!client.path("shell.txt").exists(), "bash() undone");
    assert_eq!(read(&client.path("notes.txt")), "alpha\n", "edit() undone");
    assert_eq!(
        read(&client.path("ignored.log")),
        "i1+agent",
        "an ignored file is not touched"
    );
    let big = std::fs::read(client.path("big.bin")).unwrap();
    assert!(
        u64::try_from(big.len()).unwrap() == FILE_CAP + 1 && big.starts_with(b"agent"),
        "a file over the cap is not touched"
    );
    assert!(
        strings(&result["uncovered"]).contains(&"big.bin".to_string()),
        "the file over the cap is listed as not covered: {answer}"
    );
}

/// T22 and T22b. A file changed outside the agent after the turn is listed and refuses the
/// restore without `force`; with it, the state just before is kept; right after, nothing is listed.
#[test]
fn a_change_outside_the_agent_is_listed_and_kept_before_a_forced_restore() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('keep.txt', 'w').write('v2')\nprint('ok')"),
            { "text": "done" },
        ] }),
        |home| std::fs::write(home.join("keep.txt"), "v1").unwrap(),
    );
    let (session_id, file) = client.open_session();
    let to = client.ask(&session_id, "change keep.txt");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let _ = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    std::fs::write(client.path("mine.txt"), "user").unwrap();

    let preview = client.preview(&session_id, &to);
    let listed = result_of(&preview, "the preview");
    assert_eq!(listed["available"], true, "{preview}");
    assert_eq!(
        strings(&listed["modifiedOutside"]),
        vec!["mine.txt".to_string()],
        "{preview}"
    );
    assert!(
        listed["files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|file| file["path"] == "keep.txt")),
        "the preview lists what the restore changes: {preview}"
    );

    let refused = client.restore(&session_id, &to, true, false, false);
    assert_eq!(refused["error"]["code"], -32603, "{refused}");
    assert_eq!(
        refused["error"]["data"]["details"], MODIFIED_OUTSIDE,
        "{refused}"
    );
    assert_eq!(
        strings(&refused["error"]["data"]["modifiedOutside"]),
        vec!["mine.txt".to_string()],
        "{refused}"
    );
    assert_eq!(read(&client.path("keep.txt")), "v2", "nothing restored");

    let forced = client.restore(&session_id, &to, true, false, true);
    let _ = result_of(&forced, "the forced restore");
    assert_eq!(read(&client.path("keep.txt")), "v1");
    assert!(!client.path("mine.txt").exists(), "{forced}");
    let kept = refs_under(&repo, &format!("refs/ya/{session}/avant-restauration/"));
    assert!(
        kept.iter()
            .any(|name| blob(&repo, name, "mine.txt") == "user"),
        "the state just before the restore is kept: {kept:?}"
    );

    // T22b (answer Q-3a-2): `fin` follows the restore.
    let again = client.preview(&session_id, &to);
    assert_eq!(
        strings(&result_of(&again, "the preview after the restore")["modifiedOutside"]),
        Vec::<String>::new(),
        "{again}"
    );
}

/// T23. What the model receives: after a restore of the files and the conversation, D2 with the
/// files sentence once and nothing of the abandoned turn; after a restore of the files only, the
/// text of Q-P5 once and the conversation kept. System prompt and tools unchanged.
#[test]
fn the_model_is_told_once_after_a_restore() {
    let server = ModelServer::start();
    let mut client = Client::spawn_model(&server, |home| {
        std::fs::write(home.join("a.txt"), "one").unwrap();
    });
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "ALPHA");
    let bravo = client.ask(&session_id, "BRAVO");
    std::fs::write(client.path("a.txt"), "two").unwrap();
    let both = client.restore(&session_id, &bravo, true, true, true);
    let _ = result_of(&both, "the restore of the files and the conversation");
    assert_eq!(read(&client.path("a.txt")), "one");
    let charlie = client.ask(&session_id, "CHARLIE");
    let requests = server.requests();
    assert_eq!(requests.len(), 3, "three agent-loop requests");
    let (second, third) = (&requests[1], &requests[2]);
    assert_eq!(
        (
            sent(third).matches(REWOUND).count(),
            sent(third).matches(FILES_RESTORED).count()
        ),
        (1, 1),
        "D2 and its files sentence, once: {}",
        sent(third)
    );
    assert!(!sent(third).contains("BRAVO"), "{}", sent(third));
    assert_eq!(
        without_cache_marks(&third["system"]),
        without_cache_marks(&second["system"]),
        "the system prompt is unchanged"
    );
    assert_eq!(
        without_cache_marks(&third["tools"]),
        without_cache_marks(&second["tools"]),
        "the tools are unchanged"
    );

    std::fs::write(client.path("a.txt"), "three").unwrap();
    let files = client.restore(&session_id, &charlie, true, false, true);
    let _ = result_of(&files, "the restore of the files only");
    client.ask(&session_id, "DELTA");
    let fourth = server.requests().last().cloned().expect("a fourth request");
    assert_eq!(
        sent(&fourth).matches(WORKSPACE_RESTORED).count(),
        1,
        "the files-only text, once: {}",
        sent(&fourth)
    );
    assert!(
        sent(&fourth).contains("CHARLIE"),
        "the conversation is kept: {}",
        sent(&fourth)
    );
}

/// The user's repository as criterion 1 reads it. The status covers the tracked files: the
/// binary writes its own state under the test's home, which is the workspace.
fn user_state(workspace: &Path) -> Vec<String> {
    vec![
        git(workspace, &["rev-list", "--all", "--count"]),
        git(workspace, &["for-each-ref"]),
        git(workspace, &["stash", "list"]),
        git(
            workspace,
            &["status", "--porcelain", "--untracked-files=no"],
        ),
    ]
}

/// T24. The user's repository: after two prompts and a restore, no commit, branch or stash, and
/// its status as before the first prompt.
#[test]
fn the_user_repository_keeps_its_history_and_gets_its_status_back() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('u1.txt', 'w').write('agent')\nopen('agent.txt', 'w').write('new')\nprint('ok')"),
            { "text": "done" },
            { "text": "second" },
        ] }),
        |home| {
            git(home, &["init", "-q"]);
            std::fs::write(home.join("u1.txt"), "one").unwrap();
            git(home, &["add", "u1.txt"]);
            git(home, &["commit", "-q", "-m", "one"]);
            std::fs::write(home.join("u2.txt"), "two").unwrap();
            git(home, &["add", "u2.txt"]);
            git(home, &["commit", "-q", "-m", "two"]);
            git(home, &["branch", "feature"]);
            std::fs::write(home.join("u1.txt"), "stashed").unwrap();
            git(home, &["stash", "-q"]);
        },
    );
    let before = user_state(client.workspace());
    let (session_id, _) = client.open_session();
    let to = client.ask(&session_id, "change u1.txt");
    client.ask(&session_id, "again");
    let answer = client.restore(&session_id, &to, true, false, false);
    let _ = result_of(&answer, "the restore");
    assert_eq!(
        user_state(client.workspace()),
        before,
        "commits, refs, stash and status as before the first prompt"
    );
    assert!(
        !client.path("agent.txt").exists(),
        "the file the agent made is gone"
    );
}

/// T26. No git (answer Q-3a-1): the preview says snapshots are off, with the message; the rewind
/// of the conversation still works.
#[test]
fn without_git_snapshots_are_off_and_the_rewind_still_works() {
    let mut client = Client::spawn_with(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        &[(TEST_GIT, "/nonexistent/ya43/git")],
        |_| {},
    );
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "first");
    let to = client.ask(&session_id, "second");
    let preview = client.preview(&session_id, &to);
    let result = result_of(&preview, "the preview");
    assert_eq!(result["available"], false, "{preview}");
    assert_eq!(result["reason"], NO_GIT, "{preview}");
    let rewind = client.call(REWIND, &json!({ "sessionId": session_id, "to": to }));
    assert_eq!(
        rewind["result"],
        json!({}),
        "the rewind still works: {rewind}"
    );
}

/// T27. The agent directory inside the workspace, with a credential and a link to it: no object
/// holds the credential, and a restore does not write it.
#[test]
fn no_credential_reaches_the_snapshots() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('code.py', 'w').write('print(2)')\nprint('ok')"),
            { "text": "done" },
        ] }),
        |home| {
            let agent = home.join(".yusagent");
            std::fs::create_dir_all(&agent).unwrap();
            std::fs::write(
                agent.join("auth.json"),
                format!(r#"{{"serper":{{"type":"api_key","key":"{SENTINEL}"}}}}"#),
            )
            .unwrap();
            std::os::unix::fs::symlink(".yusagent/auth.json", home.join("auth-link")).unwrap();
            std::fs::write(home.join("code.py"), "print(1)").unwrap();
        },
    );
    let auth = client.path(".yusagent/auth.json");
    let written = std::fs::metadata(&auth).unwrap().modified().unwrap();
    let (session_id, file) = client.open_session();
    let to = client.ask(&session_id, "change code.py");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let end = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    let objects = shadow(&repo, &["cat-file", "--batch-all-objects", "--batch"]);
    assert!(
        !objects.contains(SENTINEL),
        "a credential reached the snapshots"
    );
    let captured = tree(&repo, &end);
    assert!(
        captured.iter().any(|path| path == "auth-link")
            && !captured.iter().any(|path| path.starts_with(".yusagent")),
        "{captured:?}"
    );
    let answer = client.restore(&session_id, &to, true, false, false);
    let _ = result_of(&answer, "the restore");
    assert_eq!(read(&client.path("code.py")), "print(1)");
    assert!(read(&auth).contains(SENTINEL));
    assert_eq!(
        std::fs::metadata(&auth).unwrap().modified().unwrap(),
        written,
        "the restore did not write auth.json"
    );
}

/// T28. A ref older than 30 days goes, a recent one stays, and the repository of a workspace
/// that no longer exists goes, at the first snapshot of a bridge process.
#[test]
fn old_refs_and_vanished_workspaces_are_purged() {
    let mut first = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        |_| {},
    );
    let (session_id, _) = first.open_session();
    first.ask(&session_id, "first");
    let repo = wait_repo(first.workspace());
    let empty = shadow(&repo, &["hash-object", "-t", "tree", "-w", "/dev/null"]);
    let empty = empty.trim();
    let old_date = "@".to_string()
        + &(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 31 * 24 * 3600)
            .to_string();
    let mut old = Command::new("git");
    old.arg(format!("--git-dir={}", repo.display()))
        .env("GIT_COMMITTER_DATE", &old_date)
        .env("GIT_AUTHOR_DATE", &old_date);
    let old_commit =
        run_git(&mut old, &["commit-tree", empty, "-m", "old"]).expect("an old commit");
    shadow(
        &repo,
        &["update-ref", "refs/ya/old-session/fin", old_commit.trim()],
    );
    let recent = shadow(&repo, &["commit-tree", empty, "-m", "recent"]);
    shadow(
        &repo,
        &["update-ref", "refs/ya/recent-session/fin", recent.trim()],
    );
    let vanished = first.path(".yusagent/checkpoints/0123456789abcdef");
    git(
        first.workspace(),
        &["init", "-q", "--bare", vanished.to_str().unwrap()],
    );
    std::fs::write(vanished.join("workspace"), "/nonexistent/ya43-vanished\n").unwrap();
    first.close();

    let mut second = first.again();
    let (second_id, _) = second.open_session();
    second.ask(&second_id, "second");
    let deadline = Instant::now() + SNAPSHOT_WAIT;
    while !refs_under(&repo, "refs/ya/old-session/").is_empty() || vanished.exists() {
        assert!(
            Instant::now() < deadline,
            "not purged: refs {:?}, vanished workspace's repository there: {}",
            refs_under(&repo, "refs/ya/"),
            vanished.exists()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        refs_under(&repo, "refs/ya/recent-session/"),
        vec!["refs/ya/recent-session/fin".to_string()],
        "a recent ref stays"
    );
}

/// T29. Deleting a conversation deletes its refs and its index.
#[test]
fn deleting_a_conversation_deletes_its_snapshots() {
    let mut first = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        |_| {},
    );
    let (session_id, file) = first.open_session();
    first.ask(&session_id, "first");
    let deleted = file_id(&file);
    let repo = wait_repo(first.workspace());
    let _ = wait_ref(&repo, &format!("refs/ya/{deleted}/fin"));
    assert!(repo.join("ya-index").join(&deleted).exists(), "its index");
    first.close();

    let mut second = first.again();
    let _ = second.open_session();
    let answer = second.call("_yus/sessions/delete", &json!({ "sessionId": deleted }));
    let _ = result_of(&answer, "the delete");
    let deadline = Instant::now() + SNAPSHOT_WAIT;
    while !refs_under(&repo, &format!("refs/ya/{deleted}/")).is_empty()
        || repo.join("ya-index").join(&deleted).exists()
    {
        assert!(
            Instant::now() < deadline,
            "the deleted conversation's snapshots remain: {:?}",
            refs_under(&repo, &format!("refs/ya/{deleted}/"))
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// T31: a workspace over a cap at its first snapshot; the preview says snapshots are off.
fn snapshots_off_from_the_first_prompt(setup: impl FnOnce(&Path), message: &str) {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        setup,
    );
    let (session_id, _) = client.open_session();
    client.ask(&session_id, "first");
    let to = client.ask(&session_id, "second");
    let preview = client.preview(&session_id, &to);
    let result = result_of(&preview, "the preview");
    assert_eq!(result["available"], false, "{preview}");
    assert_eq!(result["reason"], message, "{preview}");
}

/// T31 (a). More than 20 000 files at the first snapshot.
#[test]
fn more_than_twenty_thousand_files_turn_snapshots_off() {
    snapshots_off_from_the_first_prompt(
        |home| many_files(&home.join("many"), 20_001),
        TOO_MANY_FILES,
    );
}

/// T31 (b). More than the total cap of covered files at the first snapshot: 65 sparse files of
/// the cap per file each (covered), 520 of the 512 allowed.
#[test]
fn more_than_the_total_cap_turns_snapshots_off() {
    snapshots_off_from_the_first_prompt(
        |home| {
            std::fs::create_dir_all(home.join("large")).unwrap();
            for index in 0..65 {
                sparse(&home.join("large").join(format!("l{index}.bin")), FILE_CAP);
            }
        },
        TOO_LARGE,
    );
}

/// T32. Neither the workspace's `.gitattributes` with the user's global filter nor the user's
/// repository configuration (`core.fsmonitor`) starts a program during a snapshot.
#[test]
fn the_git_configuration_of_the_project_starts_no_program() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('a.txt', 'w').write('changed')\nprint('ok')"),
            { "text": "done" },
        ] }),
        |home| {
            git(home, &["init", "-q"]);
            let filter_witness = home.join("witness-filter");
            let monitor_witness = home.join("witness-fsmonitor");
            let monitor = home.join("fsmonitor.sh");
            std::fs::write(
                &monitor,
                format!("#!/bin/sh\ntouch '{}'\n", monitor_witness.display()),
            )
            .unwrap();
            std::fs::set_permissions(
                &monitor,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )
            .unwrap();
            git(
                home,
                &["config", "core.fsmonitor", monitor.to_str().unwrap()],
            );
            std::fs::write(
                home.join(".gitconfig"),
                format!(
                    "[filter \"evil\"]\n\tclean = sh -c \"touch '{}'; cat\"\n",
                    filter_witness.display()
                ),
            )
            .unwrap();
            std::fs::write(home.join(".gitattributes"), "* filter=evil\n").unwrap();
            std::fs::write(home.join("a.txt"), "one").unwrap();
        },
    );
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "change a.txt");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let end = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    assert_eq!(blob(&repo, &end, "a.txt"), "changed");
    assert!(
        !client.path("witness-filter").exists(),
        "the user's clean filter ran"
    );
    assert!(
        !client.path("witness-fsmonitor").exists(),
        "the repository's fsmonitor ran"
    );
}

/// T33. A restore is refused while a turn runs (Unharnessed's text); the turn runs to its end.
#[test]
fn a_restore_is_refused_while_a_turn_runs() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            cell("call-1", SLOW_CELL),
            { "text": "cell answered" },
        ] }),
        |_| {},
    );
    let (session_id, _) = client.open_session();
    let to = client.ask(&session_id, "first");
    let prompt = client.prompt(&session_id, "run the slow cell");
    wait_file(&client.path("started"));
    let refused = client.restore(&session_id, &to, true, true, true);
    assert_eq!(refused["error"]["code"], -32603, "{refused}");
    assert_eq!(refused["error"]["data"]["details"], WORKING, "{refused}");
    let answered = client.wait_response(prompt);
    assert_eq!(
        answered["result"]["stopReason"], "end_turn",
        "the turn was not stopped: {answered}"
    );
    assert!(client.path("ended").exists(), "the cell ran to its end");
}

/// The daemon's id of the only live session on `socket`.
fn daemon_session_id(socket: &Path) -> String {
    let (_, list) = daemon_command(socket, &json!({ "type": "list" }));
    list["data"]["sessions"][0]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("a live session: {list}"))
        .to_string()
}

/// One daemon command on `socket`: the open connection and the response frame.
fn daemon_command(
    socket: &Path,
    command: &Value,
) -> (
    Box<dyn pa_types::platform::transport::BlockingTransportStream>,
    Value,
) {
    let mut writer =
        pa_types::platform::transport::connect_blocking(socket).expect("daemon socket");
    let reader = writer.try_clone_box().expect("daemon socket clone");
    let _ = reader.set_read_timeout(TIMEOUT);
    let protocol = json!({ "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION });
    let frame =
        json!({ "type": "command", "id": "ya43-test", "protocol": protocol, "command": command });
    writeln!(writer, "{frame}").expect("daemon frame");
    writer.flush().expect("daemon flush");
    let answer = BufReader::new(reader)
        .lines()
        .map(|line| serde_json::from_str::<Value>(&line.expect("daemon line")).expect("JSON"))
        .find(|frame| frame["id"] == json!("ya43-test"))
        .unwrap_or_else(|| panic!("the daemon closed without answering"));
    (writer, answer)
}

/// T33b. While another client that answers the session's permission requests is attached (the
/// I2 control of `ya43_control.py`), a restore is refused with the text of answer 9; once it
/// leaves, the restore goes on.
#[test]
fn a_restore_is_refused_while_another_window_answers_the_session() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        |_| {},
    );
    let (session_id, _) = client.open_session();
    let to = client.ask(&session_id, "first");
    client.ask(&session_id, "second");
    let daemon_id = daemon_session_id(&client.socket);
    let (other, attached) = daemon_command(
        &client.socket,
        &json!({ "type": "attach", "activeSessionId": daemon_id, "yaPermissionAsker": true }),
    );
    assert_eq!(attached["success"], true, "{attached}");
    let refused = client.restore(&session_id, &to, true, true, true);
    assert_eq!(refused["error"]["code"], -32603, "{refused}");
    assert_eq!(
        refused["error"]["data"]["details"], OTHER_WINDOW,
        "{refused}"
    );
    drop(other);
    std::thread::sleep(Duration::from_secs(1));
    let accepted = client.restore(&session_id, &to, true, true, true);
    let _ = result_of(&accepted, "the restore once the other window left");
}

/// T34. A nested repository is neither captured nor touched by a restore.
#[test]
fn a_nested_repository_is_left_alone() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            cell("call-1", "open('top.txt', 'w').write('changed')\nprint('ok')"),
            { "text": "done" },
        ] }),
        |home| {
            let sub = home.join("sub");
            std::fs::create_dir_all(&sub).unwrap();
            git(&sub, &["init", "-q"]);
            std::fs::write(sub.join("inner.txt"), "inner").unwrap();
            git(&sub, &["add", "inner.txt"]);
            git(&sub, &["commit", "-q", "-m", "inner"]);
            std::fs::write(home.join("top.txt"), "one").unwrap();
        },
    );
    let (session_id, file) = client.open_session();
    let to = client.ask(&session_id, "change top.txt");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let end = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    let captured = tree(&repo, &end);
    assert!(
        !captured
            .iter()
            .any(|path| path == "sub" || path.starts_with("sub/")),
        "the nested repository is not captured: {captured:?}"
    );
    std::fs::write(client.path("sub/inner.txt"), "inner, changed").unwrap();
    let answer = client.restore(&session_id, &to, true, false, true);
    let _ = result_of(&answer, "the restore");
    assert_eq!(read(&client.path("top.txt")), "one");
    assert_eq!(read(&client.path("sub/inner.txt")), "inner, changed");
    assert!(client.path("sub/.git").exists());
}

/// T35. The count kept up to date from the changed paths (§P.12.3, answer of §P.13): after a first
/// prompt, a folder created by hand takes the workspace over 20 000 files; the next prompt takes
/// no snapshot and the preview says snapshots are off, with the message.
#[test]
fn a_folder_created_between_two_turns_turns_snapshots_off() {
    let mut client = Client::spawn(
        &json!({ "engine": "faux", "responses": [
            { "text": "one" },
            { "text": "two" },
        ] }),
        |_| {},
    );
    let (session_id, file) = client.open_session();
    client.ask(&session_id, "first");
    let session = file_id(&file);
    let repo = wait_repo(client.workspace());
    let _ = wait_ref(&repo, &format!("refs/ya/{session}/fin"));
    many_files(&client.path("bulk"), 20_001);
    let to = client.ask(&session_id, "second");
    let preview = client.preview(&session_id, &to);
    let result = result_of(&preview, "the preview");
    assert_eq!(result["available"], false, "{preview}");
    assert_eq!(result["reason"], TOO_MANY_FILES, "{preview}");
    assert!(
        refs_under(&repo, &format!("refs/ya/{session}/debut/{to}")).is_empty(),
        "no snapshot past the cap"
    );
}
