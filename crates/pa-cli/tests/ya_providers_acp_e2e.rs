// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]
//! yusAgent (YA-10, ticket 40, R5): providers and sign-in on the ACP bridge
//! (`docs/tickets/40-login-extension.md`, §16.3). The real binary serves ACP over stdio on its
//! own sandboxed daemon; the model is a loopback server that records the `Authorization` header
//! of each request and answers 400 (no retry), behind a `ya40` provider of `models.json`
//! (`openai-responses`) whose fallback key is `sk-ya40-fallback`: a stored credential wins over
//! it (`pa_core::models::ModelRegistry::get_api_key_and_headers`).
//! 1a. H1: an API key rewritten in `auth.json` outside the bridge reaches the next request.
//! 1b. H1: an `OAuth` token renewed in `auth.json` (as another process's refresh would) reaches
//!     the next request.
//! 2. `_yus/providers/set_api_key` pushes the key into the running session.
//! 3. After a key for a new provider, the model picker lists its models (`config_option_update`).
//! 3b. A session opened without any credential runs on the daemon's fallback model, the first of
//!     the catalog; a key set through the bridge moves the engine onto the model announced after
//!     it (X5, D3), not only the picker.
//! 4. `_yus/providers/list`: the rows of the providers screen, the Claude subscription among them
//!    (§10 decision 5 cancelled on 26/11/2026).
//! 5. `set_api_key` stores the key in `auth.json` (0600) and never answers it back.
//! 8. The key is nowhere under the home but `auth.json`: not on the bridge's stderr, not in the
//!    session files, not in the usage log.
//! 8b. A real `ChatGPT` subscription sign-in, up to its URL (offline: the URL and PKCE are made
//!     locally), then `login_cancel`: neither the URL nor its `state` is on the bridge's stderr
//!     or in a file under the home; the fake browser outside the home got them.
//! 8c. The same for the Claude subscription, whose `state` is its PKCE verifier.
//!
//! Listed in `docs/fork-changes.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_mins(2);
const FALLBACK: &str = "sk-ya40-fallback";

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_resources_acp_e2e.rs`.
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

/// The model: records the `Authorization` header of each request, answers 400 (no retry).
struct ModelServer {
    port: u16,
    authorizations: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("local addr").port();
        let authorizations = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let authorizations = Arc::clone(&authorizations);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut socket, _)) => {
                            let _ = socket.set_nonblocking(false);
                            let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
                            let head = read_head(&mut socket);
                            let authorization = head
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("authorization")
                                        .then(|| value.trim().to_string())
                                })
                                .unwrap_or_default();
                            authorizations.lock().unwrap().push(authorization);
                            let body = r#"{"error":{"message":"ya40 test","type":"invalid_request_error"}}"#;
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
            authorizations,
            stop,
            thread: Some(thread),
        }
    }

    fn authorizations(&self) -> Vec<String> {
        self.authorizations.lock().unwrap().clone()
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

/// The request head (up to the blank line, or what arrived within the read timeout).
fn read_head(socket: &mut std::net::TcpStream) -> String {
    let mut head = Vec::new();
    let mut buffer = [0_u8; 8192];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") && head.len() < 65_536 {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => head.extend_from_slice(&buffer[..read]),
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// One test's home: the agent dir (`<home>/.yusagent`) with its `models.json`, the daemon
/// socket, the bridge's stderr, and the model server.
struct Home {
    dir: tempfile::TempDir,
    socket: PathBuf,
    server: ModelServer,
}

impl Home {
    /// `openai` (a built-in provider) gets a `ya40-extra` model without a fallback key when
    /// `with_openai`: it is listed only once a key is stored (test 3).
    fn new(with_openai: bool) -> Home {
        Home::build(with_openai, true)
    }

    /// No credential anywhere (D3): `ya40` without its `models.json` fallback key, and the
    /// built-in `openai` with the local `ya40-extra` model, saved as the settings default (a
    /// `models.json` provider without a key is not offered a key sign-in; `openai` is).
    fn without_fallback() -> Home {
        let home = Home::build(true, false);
        std::fs::write(
            home.path().join(".yusagent/settings.json"),
            json!({ "defaultProvider": "openai", "defaultModel": "ya40-extra" }).to_string(),
        )
        .unwrap();
        home
    }

    fn build(with_openai: bool, fallback: bool) -> Home {
        let dir = tempfile::TempDir::new().unwrap();
        let server = ModelServer::start();
        let agent_dir = dir.path().join(".yusagent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let base_url = format!("http://127.0.0.1:{}/v1", server.port);
        let model = |id: &str| {
            json!({
                "id": id, "name": id, "reasoning": false, "input": ["text"],
                "contextWindow": 200_000, "maxTokens": 32_000,
                "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
            })
        };
        let mut providers = json!({ "ya40": {
            "baseUrl": base_url, "apiKey": FALLBACK, "api": "openai-responses",
            "models": [model("ya40-model")]
        }});
        if !fallback {
            providers["ya40"]
                .as_object_mut()
                .expect("the ya40 provider")
                .remove("apiKey");
        }
        if with_openai {
            providers["openai"] = json!({
                "baseUrl": base_url, "api": "openai-responses", "models": [model("ya40-extra")]
            });
        }
        std::fs::write(
            agent_dir.join("models.json"),
            json!({ "providers": providers }).to_string(),
        )
        .unwrap();
        Home {
            socket: dir.path().join("daemon.sock"),
            dir,
            server,
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn auth_file(&self) -> PathBuf {
        self.path().join(".yusagent/auth.json")
    }

    /// Writes `auth.json` (0600) through a rename, as the agent's own writes do.
    fn write_auth(&self, auth: &Value) {
        let staged = self.path().join(".yusagent/auth.json.staged");
        std::fs::write(&staged, auth.to_string()).unwrap();
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::rename(&staged, self.auth_file()).unwrap();
    }

    fn auth(&self) -> Value {
        std::fs::read_to_string(self.auth_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null)
    }

    fn stderr_file(&self) -> PathBuf {
        self.path().join("bridge-stderr.log")
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

/// One ACP process on `ya40/ya40-model`; every frame it wrote is kept, in order.
struct Client {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
    seen: Vec<Value>,
}

impl Client {
    fn spawn(home: &Home) -> Client {
        Client::spawn_with(home, |_| {})
    }

    /// [`Client::spawn`], with the command adjusted before it starts.
    fn spawn_with(home: &Home, adjust: impl FnOnce(&mut Command)) -> Client {
        Client::launch(home, true, adjust)
    }

    /// Like the extension: no `--provider` nor `--model`, the daemon picks the startup model (D3).
    fn spawn_bare(home: &Home) -> Client {
        Client::launch(home, false, |command| {
            for name in [
                "AWS_PROFILE",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_BEARER_TOKEN_BEDROCK",
            ] {
                command.env_remove(name);
            }
        })
    }

    fn launch(home: &Home, model_flags: bool, adjust: impl FnOnce(&mut Command)) -> Client {
        let stderr = std::fs::File::create(home.stderr_file()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_prime-agent"));
        command
            .args(["--mode", "acp", "--daemon-socket"])
            .arg(&home.socket);
        if model_flags {
            command.args(["--provider", "ya40", "--model", "ya40-model"]);
        }
        command
            .env("HOME", home.path())
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
                "15000",
            )
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            .env_remove("PRIME_AGENT_FAUX_SCRIPT")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .current_dir(home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr));
        adjust(&mut command);
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

    /// A notification: no id, no answer.
    fn notify(&mut self, method: &str, params: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        let frame = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        writeln!(stdin, "{frame}").unwrap();
        stdin.flush().unwrap();
    }

    /// The response to a request, reading (and keeping) every frame before it.
    fn call(&mut self, method: &str, params: &Value) -> Value {
        let id = self.request(method, params);
        self.response(id, method)
    }

    /// The response to request `id`, reading (and keeping) every frame before it.
    fn response(&mut self, id: u64, method: &str) -> Value {
        self.wait_for(method, |frame| {
            frame["id"] == json!(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
        })
    }

    /// The first frame that `wanted` takes, reading (and keeping) every frame up to it.
    fn wait_for(&mut self, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
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
            let found = wanted(&frame);
            self.seen.push(frame.clone());
            if found {
                return frame;
            }
        }
    }

    /// `initialize` then `session/new`: the session id.
    fn open(&mut self) -> String {
        let _ = self.call(
            "initialize",
            &json!({ "protocolVersion": 1, "clientCapabilities": {} }),
        );
        let new = self.call("session/new", &json!({ "mcpServers": [] }));
        new["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("session/new: {new}"))
            .to_string()
    }

    /// One prompt; the model answers 400, so the turn fails once its request went out.
    fn prompt(&mut self, session_id: &str, text: &str) -> Value {
        self.call(
            "session/prompt",
            &json!({ "sessionId": session_id, "prompt": [{ "type": "text", "text": text }] }),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
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

fn api_key(key: &str) -> Value {
    json!({ "type": "api_key", "key": key })
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// The `Authorization` of the last request the model got.
fn last_authorization(home: &Home) -> String {
    home.server
        .authorizations()
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("the model got no request"))
}

/// The requests' `Authorization` by name, for the assertion messages: the CI log masks a
/// `Bearer` value.
fn labels(home: &Home) -> Vec<&'static str> {
    home.server
        .authorizations()
        .iter()
        .map(
            |value| match value.strip_prefix("Bearer ").unwrap_or(value) {
                "sk-ya40-old" | "tok-ya40-old" => "old",
                "sk-ya40-renewed" | "tok-ya40-renewed" => "renewed",
                "sk-ya40-moved" => "moved",
                FALLBACK => "fallback",
                "" => "none",
                _ => "other",
            },
        )
        .collect()
}

/// Test 1a (H1): a key rewritten in `auth.json` outside the bridge, as `/login` in a terminal
/// does, reaches the running session's next request. Red while the session keeps the key it
/// was built with; outside the bridge, this is ticket E-C's.
#[test]
#[ignore = "H1, confirmed red (ticket 40, §18): the running session keeps its build-time credential; ticket E-C (upstream a05bcb695, dad2232cc)"]
fn h1_an_api_key_rewritten_on_disk_reaches_the_running_session() {
    let home = Home::new(false);
    home.write_auth(&json!({ "ya40": api_key("sk-ya40-old") }));
    let mut client = Client::spawn(&home);
    let session = client.open();
    let _ = client.prompt(&session, "first");
    assert_eq!(
        last_authorization(&home),
        "Bearer sk-ya40-old",
        "precondition: the stored key serves the first request"
    );
    home.write_auth(&json!({ "ya40": api_key("sk-ya40-renewed") }));
    let _ = client.prompt(&session, "second");
    assert_eq!(
        last_authorization(&home),
        "Bearer sk-ya40-renewed",
        "H1: the running session kept the key it was built with; requests: {:?}",
        labels(&home)
    );
}

/// Test 1b (H1): an `OAuth` token renewed in `auth.json`, as another process's refresh would
/// leave it, reaches the running session's next request. A token the agent refreshes itself is
/// not testable offline (the token URLs are fixed); that is ticket E-C's.
#[test]
#[ignore = "H1, confirmed red (ticket 40, §18): the running session keeps its build-time credential; ticket E-C (upstream a05bcb695, dad2232cc)"]
fn h1_an_oauth_token_renewed_on_disk_reaches_the_running_session() {
    let home = Home::new(false);
    let token = |access: &str| {
        json!({ "type": "oauth", "access": access, "refresh": "r-ya40",
                "expires": now_ms() + 3_600_000 })
    };
    home.write_auth(&json!({ "ya40": token("tok-ya40-old") }));
    let mut client = Client::spawn(&home);
    let session = client.open();
    let _ = client.prompt(&session, "first");
    assert_eq!(
        last_authorization(&home),
        "Bearer tok-ya40-old",
        "precondition: the stored token serves the first request"
    );
    home.write_auth(&json!({ "ya40": token("tok-ya40-renewed") }));
    let _ = client.prompt(&session, "second");
    assert_eq!(
        last_authorization(&home),
        "Bearer tok-ya40-renewed",
        "H1: the running session kept the token it was built with; requests: {:?}",
        labels(&home)
    );
}

/// Test 2: a key set through `_yus/providers/set_api_key` reaches the running session's next
/// request (the bridge reloads the worker's request auth).
#[test]
fn set_api_key_pushes_the_key_into_the_running_session() {
    let home = Home::new(false);
    home.write_auth(&json!({ "ya40": api_key("sk-ya40-old") }));
    let mut client = Client::spawn(&home);
    let session = client.open();
    let _ = client.prompt(&session, "first");
    assert_eq!(last_authorization(&home), "Bearer sk-ya40-old");
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "ya40", "apiKey": "sk-ya40-renewed" }),
    );
    assert_eq!(set["result"]["outcome"], "done", "{set}");
    let _ = client.prompt(&session, "second");
    assert_eq!(
        last_authorization(&home),
        "Bearer sk-ya40-renewed",
        "the key set through the bridge serves the next request; requests: {:?}",
        labels(&home)
    );
}

/// Test 3: a key for a provider the session could not use lists its models in the picker of
/// the running conversation (`config_option_update`), before the answer.
#[test]
fn a_new_key_lists_its_models_in_the_running_conversation() {
    let home = Home::new(true);
    home.write_auth(&json!({}));
    let mut client = Client::spawn(&home);
    let _session = client.open();
    let extra = json!(["openai", "ya40-extra"]).to_string();
    let lists_extra = |frame: &Value| {
        frame["method"] == "session/update"
            && frame["params"]["update"]["sessionUpdate"] == "config_option_update"
            && frame["params"]["update"]["configOptions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|option| option["id"] == "model")
                .flat_map(|option| option["options"].as_array().into_iter().flatten())
                .any(|choice| choice["value"] == json!(extra))
    };
    assert!(
        !client.seen.iter().any(&lists_extra),
        "precondition: no key, no openai model"
    );
    let from = client.seen.len();
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "openai", "apiKey": "sk-ya40-openai" }),
    );
    assert_eq!(set["result"]["outcome"], "done", "{set}");
    assert!(
        client.seen[from..].iter().any(&lists_extra),
        "the picker lists {extra} once its key is stored; frames: {:?}",
        &client.seen[from..]
    );
}

/// Test 3b (X5, D3): without any credential, the session opens on the daemon's fallback model,
/// the first of the catalog (`amazon-bedrock`, `agent_engine/model.rs`), and its agent is built
/// on it. A key for `openai` makes the saved default `openai/ya40-extra` the announced model; the
/// next prompt must reach it with the key, not the agent built on the fallback model.
#[test]
fn a_key_moves_a_session_opened_without_credentials_off_the_fallback_model() {
    let home = Home::without_fallback();
    home.write_auth(&json!({}));
    let mut client = Client::spawn_bare(&home);
    let _ = client.call(
        "initialize",
        &json!({ "protocolVersion": 1, "clientCapabilities": {} }),
    );
    let new = client.call("session/new", &json!({ "mcpServers": [] }));
    let session = new["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("session/new: {new}"))
        .to_string();
    let current = new["result"]["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["id"] == "model")
        .map_or(Value::Null, |option| option["currentValue"].clone());
    assert!(
        current
            .as_str()
            .is_some_and(|value| value.contains("amazon-bedrock")),
        "precondition: the session opens on the fallback model; model option: {current}"
    );
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "openai", "apiKey": "sk-ya40-moved" }),
    );
    assert_eq!(set["result"]["outcome"], "done", "{set}");
    let from = client.seen.len();
    let _ = client.prompt(&session, "after the key");
    // What the bridge did, for the message: the model announced after the key (the last
    // `config_option_update` before the prompt), and the session file's `model_change` rows,
    // which every `set_model` writes.
    let announced = client.seen[..from]
        .iter()
        .rev()
        .filter(|frame| frame["params"]["update"]["sessionUpdate"] == "config_option_update")
        .flat_map(|frame| {
            frame["params"]["update"]["configOptions"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .find(|option| option["id"] == "model")
        .map_or(Value::Null, |option| option["currentValue"].clone());
    assert_eq!(
        home.server.authorizations(),
        [String::from("Bearer sk-ya40-moved")],
        "the prompt after the key reaches openai/ya40-extra with it; requests: {:?}; announced \
         after the key: {announced}; model_change rows: {:?}",
        labels(&home),
        model_changes(&home)
    );
}

/// The `model_change` rows of the home's session files, as `provider/model`.
fn model_changes(home: &Home) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(home.path().join(".yusagent/sessions")) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            let Ok(row) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if row["type"] == "model_change" {
                rows.push(format!(
                    "{}/{}",
                    row["provider"].as_str().unwrap_or_default(),
                    row["modelId"].as_str().unwrap_or_default()
                ));
            }
        }
    }
    rows
}

/// Test 4: the rows of the providers screen (Unharnessed's shape): the `ChatGPT` subscription by
/// the browser, `ya40` by a key, connected and removable (its key is in `auth.json`; a
/// `models.json` key alone does not count as connected in the TUI's rows), the Claude subscription
/// by the browser (§10 decision 5 cancelled on 26/11/2026).
#[test]
fn list_gives_the_providers_screen_with_the_claude_subscription() {
    let home = Home::new(false);
    home.write_auth(&json!({ "ya40": api_key("sk-ya40-old") }));
    let mut client = Client::spawn(&home);
    let _session = client.open();
    let list = client.call("_yus/providers/list", &json!({}));
    let providers = list["result"]["providers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let row = |id: &str, method: &str| {
        providers
            .iter()
            .find(|row| row["id"] == id && row["method"] == method)
            .cloned()
    };
    let codex = row("openai-codex", "subscription")
        .unwrap_or_else(|| panic!("the ChatGPT subscription row: {list}"));
    assert_eq!(codex["login"], "browser", "{codex}");
    let ya40 = row("ya40", "apiKey").unwrap_or_else(|| panic!("the ya40 row: {list}"));
    assert_eq!(ya40["login"], "apiKey", "{ya40}");
    assert_eq!(ya40["connected"], true, "{ya40}");
    assert_eq!(ya40["removable"], true, "{ya40}");
    let claude = row("anthropic", "subscription")
        .unwrap_or_else(|| panic!("the Claude subscription row: {list}"));
    assert_eq!(claude["login"], "browser", "{claude}");
}

/// Test 5: `set_api_key` stores the key in `auth.json`, owner-only, and the answer never
/// carries it.
#[test]
fn set_api_key_stores_the_key_and_never_answers_it() {
    let home = Home::new(true);
    let mut client = Client::spawn(&home);
    let _session = client.open();
    let key = "sk-ya40-openai-stored";
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "openai", "apiKey": key }),
    );
    assert_eq!(
        home.auth()["openai"],
        api_key(key),
        "the key in auth.json; answer: {set}"
    );
    let mode = std::fs::metadata(home.auth_file())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "auth.json is owner-only");
    assert!(
        !set.to_string().contains(key),
        "the answer carries the key: {set}"
    );
}

/// Every file under `dir`, but `auth.json`, that contains `needle`.
fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if kind.is_file() && path.file_name().is_none_or(|name| name != "auth.json") {
            if let Ok(bytes) = std::fs::read(&path) {
                if String::from_utf8_lossy(&bytes).contains(needle) {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// Test 8: a key set through the bridge is stored and used, and found nowhere else under the
/// home: not on the bridge's stderr, not in the session files, not in the usage log.
#[test]
fn a_key_set_through_the_bridge_is_nowhere_but_auth_json() {
    let home = Home::new(false);
    home.write_auth(&json!({}));
    let secret = "sk-ya40-secret-7f3c9a";
    let mut client = Client::spawn(&home);
    let session = client.open();
    let set = client.call(
        "_yus/providers/set_api_key",
        &json!({ "providerId": "ya40", "apiKey": secret }),
    );
    assert_eq!(
        home.auth()["ya40"],
        api_key(secret),
        "precondition: the key is stored; answer: {set}"
    );
    let _ = client.prompt(&session, "with the secret key");
    assert_eq!(
        last_authorization(&home),
        format!("Bearer {secret}"),
        "precondition: the key serves the request"
    );
    drop(client);
    let found = files_containing(home.path(), secret);
    assert!(
        found.is_empty(),
        "the key is in {found:?} (stderr: {})",
        home.stderr_file().display()
    );
}

/// Test 8b: a real `ChatGPT` subscription sign-in in the binary, up to its URL, then
/// `login_cancel`. The URL and its PKCE are made locally (`pa-ai`'s `oauth/openai_codex.rs`,
/// `login_openai_codex`), so no network is needed; the callback server on 1455 is dead when the
/// port is taken, and the flow then waits on the paste, which the cancel ends too. Codex's
/// `state` is random; its PKCE verifier never leaves the process.
#[test]
fn a_sign_in_url_and_its_state_are_nowhere_under_the_home() {
    sign_in_url_is_nowhere_under_the_home("openai-codex");
}

/// Test 8c: the same for the Claude subscription (`pa-ai`'s `oauth/anthropic.rs`,
/// `login_anthropic`), whose `state` is its PKCE verifier. Its callback server binds 53692 first:
/// a taken port fails the sign-in before the URL (the precondition says so).
#[test]
fn a_claude_sign_in_url_and_its_verifier_are_nowhere_under_the_home() {
    sign_in_url_is_nowhere_under_the_home("anthropic");
}

/// Tests 8b and 8c: a real subscription sign-in of `provider_id`, up to its URL, then
/// `login_cancel`. The flow opens the page itself (`on_auth` of `codex_subscription_login.rs` and
/// `subscription_login.rs`): fake `open` and `xdg-open`, first on the bridge's `PATH`, write their
/// arguments to a file outside the home, so no browser runs. Neither the URL nor its `state` is on
/// the bridge's stderr or in a file under the home. The current code logs neither, so the test is
/// green from the start: the positive control is the same search finding `state` in the fake
/// opener's file.
fn sign_in_url_is_nowhere_under_the_home(provider_id: &str) {
    let home = Home::new(false);
    home.write_auth(&json!({}));
    let fake_bin = tempfile::TempDir::new().unwrap();
    assert!(
        !fake_bin.path().starts_with(home.path()),
        "the fake opener is outside the home"
    );
    let opened = fake_bin.path().join("opened.txt");
    for name in ["open", "xdg-open"] {
        let script = fake_bin.path().join(name);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                opened.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(fake_bin.path().to_path_buf()).chain(std::env::split_paths(&path)),
    )
    .unwrap();
    let mut client = Client::spawn_with(&home, |command| {
        command.env("PATH", &path);
    });
    let _session = client.open();
    let login = client.request(
        "_yus/providers/login",
        &json!({ "providerId": provider_id }),
    );
    let step = client.wait_for("the sign-in URL", |frame| {
        (frame["method"] == "_yus/providers/login_update"
            && frame["params"]["update"]["kind"] == "url")
            || frame["id"] == json!(login)
    });
    assert_eq!(
        step["method"], "_yus/providers/login_update",
        "precondition: the {provider_id} sign-in reached its URL; answer: {step}"
    );
    let url = step["params"]["update"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("a URL step: {step}"))
        .to_string();
    let state = url
        .split_once("state=")
        .map_or("", |(_, rest)| rest.split('&').next().unwrap_or(rest))
        .to_string();
    // The messages never print the URL: Claude's `state` is its PKCE verifier.
    assert!(
        state.len() >= 16,
        "precondition: the URL carries its state (state=), {} characters long",
        url.len()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(&opened).is_ok_and(|text| text.contains(&url)) {
        assert!(
            Instant::now() < deadline,
            "precondition: the fake opener got the URL; it wrote {:?} bytes",
            std::fs::metadata(&opened)
                .ok()
                .as_ref()
                .map(std::fs::Metadata::len)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    client.notify("_yus/providers/login_cancel", &json!({}));
    let answer = client.response(login, "the login answer");
    assert_eq!(answer["result"]["outcome"], "cancelled", "{answer}");
    drop(client);
    assert_eq!(
        files_containing(fake_bin.path(), &state),
        [opened],
        "positive control: the search finds the state in the fake opener's file"
    );
    let stderr = std::fs::read_to_string(home.stderr_file()).unwrap_or_default();
    assert!(
        !stderr.contains(&url) && !stderr.contains(&state),
        "the sign-in URL or its state is on the bridge's stderr ({})",
        home.stderr_file().display()
    );
    let found_url = files_containing(home.path(), &url);
    let found_state = files_containing(home.path(), &state);
    assert!(
        found_url.is_empty() && found_state.is_empty(),
        "the sign-in URL is in {found_url:?}, its state in {found_state:?}"
    );
}
