// too_many_lines: style gate, not correctness.
#![allow(clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-11, ticket 20, PR 3): the permission panel of the TUI, end to end. The real
//! binary runs the TUI on a pty (its controlling terminal) against its own daemon; the worker runs
//! the faux provider (`PRIME_AGENT_FAUX_SCRIPT`) and a real kernel, in `"manual"`: written in the
//! global settings (ticket 30: unset is `"auto"`); `PRIME_AGENT_TEST_PERMISSIONS`, which
//! `.cargo/config.toml` sets to `auto` for the upstream tests, is removed. The screen is read through a terminal emulator (`vt100`).
//!
//! One session, three turns:
//! 1. a cell asks; text typed while the panel waits (`any`: an `n` and a `y`) goes to the
//!    editor and answers nothing (the quiet window, decision T3); after a pause, `y` allows: the
//!    file is created;
//! 2. `/permissions auto`: the tray shows `auto · no sandbox` (D4); a cell runs without a panel;
//! 3. `/permissions manual`: the badge goes; a cell asks; after a pause, `n` denies: no file, the
//!    turn stops (the next scripted reply is never requested).
//!
//! The journal of the session holds `allow`, the two changes of mode, then `deny`.
//!
//! Ticket 22: while a request waits, the loader says `Awaiting permission` (never `Executing`)
//! and the cell's card shows no success mark; the allowed card ends in `✓`, the denied one in
//! `✗`.
//!
//! Ticket 30 (decision of Yusuf, 22/11/2026): without a setting, the session opens in `auto`: the
//! tray shows `auto · no sandbox` at once and a cell runs without a panel.
//!
//! Listed in `docs/fork-changes.md`.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::pty::{openpty, Winsize};
use serde_json::{json, Value};

const TEST_ENV: &str = "PRIME_AGENT_TEST_PERMISSIONS";
const TIMEOUT: Duration = Duration::from_mins(2);
const ROWS: u16 = 40;
const COLS: u16 = 120;
const BADGE: &str = "auto · no sandbox";
/// The panel's title (the notes of `/permissions` say "Permissions:").
const PANEL: &str = "Permission · python_repl";
const WAITING: &str = "pause typing to answer";
const ARMED: &str = "y allow once";
/// The loader while a request waits (ticket 22).
const AWAITING: &str = "Awaiting permission";

/// The summary line of the cell card that creates `name` (`<mark> python · open("name", …`); the
/// panel shows the same code without the `python · ` label.
fn card_line(screen: &str, name: &str) -> Option<String> {
    screen
        .lines()
        .find(|line| line.contains("python · ") && line.contains(name))
        .map(str::to_string)
}

/// Ticket 22: while a request waits, nothing says the call runs. The panel covers the bottom of
/// the transcript: once the transcript fills the window, the loader and the card sit under it
/// (`loader_shown` false).
fn assert_waiting(screen: &str, name: &str, loader_shown: bool) {
    if loader_shown {
        assert!(
            screen.contains(AWAITING),
            "the loader awaits the permission:\n{screen}"
        );
    }
    assert!(
        !screen.contains("Executing"),
        "the loader does not say Executing:\n{screen}"
    );
    if let Some(card) = card_line(screen, name) {
        assert!(
            !card.contains('✓'),
            "the waiting card is not a success: {card}\n{screen}"
        );
    }
}

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides), as
/// `ya_permissions_acp_e2e.rs`.
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

/// One scripted reply: a `python_repl` cell that creates `name` in the project.
fn touch(id: &str, name: &str) -> Value {
    json!({ "content": [{
        "type": "toolCall", "id": id, "name": "python_repl",
        "arguments": { "code": format!("open({name:?}, 'w').write('x')") },
    }] })
}

/// The worker env the daemon must not inherit from the test process.
const WORKER_ENV: [&str; 9] = [
    pa_daemon::worker::WORKER_ROLE_ENV,
    pa_daemon::worker::WORKER_TOKEN_ENV,
    pa_daemon::worker::WORKER_ACTIVE_SESSION_ID_ENV,
    pa_daemon::worker::WORKER_RECOVERY_JOURNAL_ENV,
    pa_daemon::worker::WORKER_SUPERVISOR_SOCKET_ENV,
    pa_daemon::worker::WORKER_SOCKET_ENV,
    pa_daemon::worker::WORKER_INSTANCE_ID_ENV,
    pa_daemon::worker::WORKER_SCRIPT_ENV,
    pa_daemon::lease::SESSION_LEASE_OWNER_ID_ENV,
];

/// An isolated home: `<home>/.yusagent` (`"permissions": "manual"` by [`Lane::new`]),
/// `<home>/project`.
struct Lane {
    home: tempfile::TempDir,
    socket: PathBuf,
    daemon: Option<Child>,
    tui: Option<Child>,
}

impl Lane {
    fn new() -> Self {
        // No onboarding screen; manual, written (ticket 30: unset is `"auto"`).
        Self::with_settings(&json!({ "onboardingShown": true, "permissions": "manual" }))
    }

    /// With `settings` as the global `settings.json`.
    fn with_settings(settings: &Value) -> Self {
        let home = tempfile::TempDir::new().expect("temp home");
        let agent_dir = home.path().join(".yusagent");
        std::fs::create_dir_all(&agent_dir).expect("agent dir");
        std::fs::create_dir_all(home.path().join("project")).expect("project");
        std::fs::write(agent_dir.join("settings.json"), settings.to_string())
            .expect("settings.json");
        let socket = home.path().join("daemon.sock");
        Lane {
            home,
            socket,
            daemon: None,
            tui: None,
        }
    }

    fn project(&self) -> PathBuf {
        self.home.path().join("project")
    }

    /// The common env of the daemon and the TUI.
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_prime-agent"));
        command
            .env("HOME", self.home.path())
            .env("DO_NOT_TRACK", "1")
            .env("PI_OFFLINE", "1")
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                pa_daemon::worker::WORKER_SUPERVISOR_LOST_EXIT_MS_ENV,
                "15000",
            )
            .env_remove(TEST_ENV)
            .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
            .env_remove("PRIME_AGENT_SESSION_DIR")
            .env_remove("RLM_DEPTH")
            .env_remove("TMUX")
            .current_dir(self.project());
        for var in WORKER_ENV {
            command.env_remove(var);
        }
        command
    }

    fn start_daemon(&mut self) {
        let mut command = self.command();
        command
            .args(["--mode", "daemon", "--daemon-socket"])
            .arg(&self.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.daemon = Some(command.spawn().expect("spawn the daemon"));
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.socket.exists() {
            assert!(Instant::now() < deadline, "the daemon socket never came up");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The TUI on a new pty, with `script` as the faux provider's replies.
    fn start_tui(&mut self, script: &Value) -> Screen {
        let script_path = self.home.path().join("script.json");
        std::fs::write(&script_path, script.to_string()).expect("script");
        let pty = openpty(
            Some(&Winsize {
                ws_row: ROWS,
                ws_col: COLS,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            None,
        )
        .expect("open pty");
        let mut command = self.command();
        command
            .arg("--daemon-socket")
            .arg(&self.socket)
            .env("PRIME_AGENT_FAUX_SCRIPT", &script_path)
            .env("TERM", "xterm-256color")
            .stdin(slave_stdio(&pty.slave))
            .stdout(slave_stdio(&pty.slave))
            .stderr(slave_stdio(&pty.slave));
        let slave_fd = pty.slave.as_raw_fd();
        // SAFETY: the pre_exec hook runs post-fork, pre-exec, in the child only; it makes the pty
        // the child's controlling terminal (crossterm reads `/dev/tty`), as
        // `exit_backfill_pty_e2e.rs` does.
        unsafe {
            command.pre_exec(move || claim_controlling_tty(slave_fd));
        }
        self.tui = Some(command.spawn().expect("spawn the TUI"));
        drop(pty.slave);
        Screen::new(pty.master)
    }

    /// The `data` of the `ya-permission` entries of the session files.
    fn journal(&self) -> Vec<Value> {
        let Ok(entries) = std::fs::read_dir(self.home.path().join(".yusagent/sessions")) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path
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
}

impl Drop for Lane {
    fn drop(&mut self) {
        for mut child in [self.tui.take(), self.daemon.take()].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // The daemon's workers go with a protocol shutdown.
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&self.socket) {
            let frame = json!({
                "type": "command", "id": "shutdown-test",
                "protocol": {
                    "name": "prime-agent.daemon",
                    "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION,
                },
                "command": { "type": "shutdown" },
            });
            let _ = writeln!(stream, "{frame}");
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

fn claim_controlling_tty(fd: i32) -> std::io::Result<()> {
    nix::unistd::setsid()?;
    // SAFETY: a plain ioctl on the pty slave the child inherited.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCSCTTY as libc::c_ulong, 0) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn slave_stdio(slave: &OwnedFd) -> Stdio {
    slave.try_clone().expect("clone the pty slave").into()
}

/// The TUI's screen: a reader thread feeds the pty's output to a terminal emulator.
struct Screen {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: File,
}

impl Screen {
    fn new(master: OwnedFd) -> Self {
        let mut reader = File::from(master);
        let writer = reader.try_clone().expect("clone the pty master");
        let parser = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let fed = Arc::clone(&parser);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    return;
                }
                fed.lock().unwrap().process(&buffer[..count]);
            }
        });
        Screen { parser, writer }
    }

    fn contents(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    /// Waits until the screen satisfies `ready`; returns it.
    fn wait(&self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let contents = self.contents();
            if ready(&contents) {
                return contents;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; screen:\n{contents}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_for(&self, needle: &str) -> String {
        self.wait(needle, |contents| contents.contains(needle))
    }

    fn wait_gone(&self, needle: &str) -> String {
        self.wait(&format!("{needle} to go"), |contents| {
            !contents.contains(needle)
        })
    }

    /// Bytes typed one key at a time, `gap` apart.
    fn type_slowly(&mut self, text: &str, gap: Duration) {
        for byte in text.bytes() {
            self.writer.write_all(&[byte]).expect("type");
            self.writer.flush().expect("flush");
            std::thread::sleep(gap);
        }
    }

    /// A prompt, then one Enter.
    fn submit(&mut self, line: &str) {
        self.type_slowly(line, Duration::from_millis(30));
        self.type_slowly("\r", Duration::from_millis(30));
    }

    /// A command line, then Enter until `done` holds: an Enter on an open completion list applies the
    /// completion instead of submitting (`pa_tui::editor`), so a second one may be needed.
    fn submit_until(&mut self, line: &str, what: &str, done: impl Fn(&str) -> bool) {
        self.type_slowly(line, Duration::from_millis(30));
        for _ in 0..3 {
            self.type_slowly("\r", Duration::from_millis(30));
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if done(&self.contents()) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.wait(what, done);
    }
}

/// Waits until `path` exists (or not, after a settle) and returns whether it does.
fn exists_within(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    path.exists()
}

#[test]
fn the_tui_answers_switches_to_auto_and_back() {
    let mut lane = Lane::new();
    lane.start_daemon();
    let mut screen = lane.start_tui(&json!({ "engine": "faux", "responses": [
        touch("call-allow", "allowed.txt"),
        { "text": "the first cell ran" },
        touch("call-auto", "auto.txt"),
        { "text": "the auto cell ran" },
        touch("call-deny", "denied.txt"),
        { "text": "never requested: the turn stops after a refusal" },
    ] }));
    let project = lane.project();

    // The session opened (the top bar names the project) in manual: no badge.
    screen.wait_for("project");
    std::thread::sleep(Duration::from_secs(1));
    assert!(!screen.contents().contains(BADGE));

    // 1. The request; typing goes to the editor; after a pause, `y` allows.
    screen.submit("create allowed.txt");
    screen.wait_for(WAITING);
    screen.type_slowly("any", Duration::from_millis(120));
    let typed = screen.contents();
    assert!(typed.contains(PANEL), "the panel stays open:\n{typed}");
    assert!(
        typed.contains("any"),
        "the typed text is in the editor:\n{typed}"
    );
    assert!(
        !project.join("allowed.txt").exists(),
        "typing answered nothing"
    );
    assert_waiting(&typed, "allowed.txt", true);
    let armed = screen.wait_for(ARMED);
    assert_waiting(&armed, "allowed.txt", true);
    screen.type_slowly("y", Duration::from_millis(50));
    screen.wait_for("the first cell ran");
    assert!(project.join("allowed.txt").exists(), "the allowed cell ran");
    screen.wait("the allowed card to succeed", |screen| {
        card_line(screen, "allowed.txt").is_some_and(|card| card.contains('✓'))
    });
    screen.wait_gone(WAITING);
    // The typed text stayed in the editor: erase it.
    screen.type_slowly("\x7f\x7f\x7f", Duration::from_millis(50));

    // 2. Auto: the badge shows, a cell runs without a panel.
    screen.submit_until("/permissions auto", BADGE, |screen| screen.contains(BADGE));
    screen.submit("create auto.txt");
    screen.wait_for("the auto cell ran");
    assert!(project.join("auto.txt").exists(), "the auto cell ran");
    assert!(!screen.contents().contains(PANEL), "no panel in auto");

    // 3. Back to manual: the badge goes, a cell asks, `n` denies.
    screen.submit_until("/permissions manual", "the badge to go", |screen| {
        !screen.contains(BADGE)
    });
    screen.submit("create denied.txt");
    screen.wait_for(WAITING);
    let armed = screen.wait_for(ARMED);
    assert_waiting(&armed, "denied.txt", false);
    screen.type_slowly("n", Duration::from_millis(50));
    screen.wait_gone(PANEL);
    let denied = screen.wait("the denied card to fail", |screen| {
        card_line(screen, "denied.txt").is_some_and(|card| card.contains('✗'))
    });
    let card = card_line(&denied, "denied.txt").unwrap_or_default();
    assert!(
        !card.contains('✓'),
        "the denied card is not a success: {card}"
    );
    assert!(
        !exists_within(&project.join("denied.txt"), Duration::from_secs(3)),
        "the denied cell did not run"
    );
    let after = screen.contents();
    assert!(
        !after.contains("never requested"),
        "the turn stopped after the refusal:\n{after}"
    );

    // The journal: the two decisions and the two changes of mode, in order.
    let deadline = Instant::now() + TIMEOUT;
    let mut journal = lane.journal();
    while journal.len() < 4 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        journal = lane.journal();
    }
    let summary: Vec<String> = journal
        .iter()
        .map(|data| {
            if let Some(decision) = data["decision"].as_str() {
                decision.to_string()
            } else {
                format!(
                    "{}->{}",
                    data["previousMode"].as_str().unwrap_or_default(),
                    data["mode"].as_str().unwrap_or_default()
                )
            }
        })
        .collect();
    assert_eq!(
        summary,
        ["allow", "manual->auto", "auto->manual", "deny"],
        "{journal:?}"
    );
}

/// Ticket 30: no `permissions` key, the session opens in auto: the badge shows at once and a cell
/// runs without a panel.
#[test]
fn without_a_setting_the_tui_opens_in_auto() {
    let mut lane = Lane::with_settings(&json!({ "onboardingShown": true }));
    lane.start_daemon();
    let mut screen = lane.start_tui(&json!({ "engine": "faux", "responses": [
        touch("call-default", "default.txt"),
        { "text": "the default cell ran" },
    ] }));
    let project = lane.project();

    screen.wait_for("project");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !screen.contents().contains(BADGE) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let opened = screen.contents();
    assert!(
        opened.contains(BADGE),
        "the badge shows at the opening:\n{opened}"
    );

    screen.submit("create default.txt");
    screen.wait_for("the default cell ran");
    assert!(project.join("default.txt").exists(), "the cell ran");
    let after = screen.contents();
    assert!(!after.contains(PANEL), "no panel in auto:\n{after}");
    assert!(lane.journal().is_empty(), "nothing journaled in auto");
}
