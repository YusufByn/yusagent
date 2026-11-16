// too_many_lines: style gate, not correctness. Casts: 64-bit targets; the pid narrowing sits at
// an OS boundary.
#![allow(clippy::too_many_lines, clippy::cast_possible_wrap)]
#![cfg(unix)]

//! yusAgent (YA-09): `[bash-done]` notices in print mode (`ya_bash_notice_batch`), with the real
//! binary, the scripted faux provider, and a real Python kernel.
//! a. Default window: three detached commands that end after the turn wake the model once, with
//!    their three notices together, and `-p` waits for that wake before it exits.
//! b. `bashNoticeBatchSeconds: 0`: the upstream path, one wake per notice.
//! c. A notice that arrives while the agent runs rides its next model call, even with another
//!    command still running (no hold, no extra wake).
//! d. A short window releases a held notice while a longer command still runs.

use std::path::{Path, PathBuf};

struct Lane {
    home: tempfile::TempDir,
    agent_dir: PathBuf,
}

fn lane(settings: Option<&serde_json::Value>) -> Lane {
    let home = tempfile::TempDir::new().unwrap();
    let agent_dir = home.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    if let Some(settings) = settings {
        std::fs::write(agent_dir.join("settings.json"), settings.to_string()).unwrap();
    }
    Lane { home, agent_dir }
}

/// The kernel Python with prime-agent-runtime installed (`PA_E2E_KERNEL_PYTHON` overrides); a
/// missing kernel fails the lane.
fn kernel_python() -> PathBuf {
    if let Some(explicit) = std::env::var_os("PA_E2E_KERNEL_PYTHON") {
        return PathBuf::from(explicit);
    }
    let candidate = PathBuf::from(std::env::var("HOME").map_or_else(
        |_| "/home/ubuntu/.prime/agent/kernel-venv/bin/python".to_string(),
        |home| format!("{home}/.prime/agent/kernel-venv/bin/python"),
    ));
    assert!(
        candidate.exists(),
        "kernel python {} not found: set PA_E2E_KERNEL_PYTHON",
        candidate.display()
    );
    candidate
}

impl Lane {
    fn run(&self, script: &serde_json::Value) -> (String, String, i32) {
        let bin = env!("CARGO_BIN_EXE_prime-agent");
        let mut command = std::process::Command::new(bin);
        command
            .args(["-p", "run the commands"])
            .env("HOME", self.home.path())
            .env("PRIME_AGENT_CODING_AGENT_DIR", &self.agent_dir)
            .env("PRIME_AGENT_FAUX_SCRIPT", script.to_string())
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env_remove("RLM_DEPTH")
            .env_remove("PRIME_AGENT_SESSION_DIR")
            .env_remove("PRIME_AGENT_CODING_AGENT_SESSION_DIR")
            .current_dir(self.home.path());
        let stdout_path = self.home.path().join("stdout.log");
        let stderr_path = self.home.path().join("stderr.log");
        command
            .stdout(std::fs::File::create(&stdout_path).unwrap())
            .stderr(std::fs::File::create(&stderr_path).unwrap());
        pa_core::platform::set_new_process_group(&mut command);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let status = runtime.block_on(async {
            let mut child = tokio::process::Command::from(command)
                .kill_on_drop(true)
                .spawn()
                .expect("binary present");
            let pid = child.id().expect("running child") as i32;
            if let Ok(status) =
                tokio::time::timeout(std::time::Duration::from_secs(120), child.wait()).await
            {
                Some(status.expect("print run exits"))
            } else {
                let _ = pa_core::platform::kill_process_group_or_pid(pid);
                let _ = child.wait().await;
                None
            }
        });
        let stdout = std::fs::read_to_string(stdout_path).unwrap();
        let stderr = std::fs::read_to_string(stderr_path).unwrap();
        let status = status.unwrap_or_else(|| {
            panic!("print run exceeded 120s; stderr: {stderr}\nstdout: {stdout}")
        });
        (stdout, stderr, status.code().unwrap_or(-1))
    }

    /// The entries of the one session file the run wrote.
    fn entries(&self) -> Vec<serde_json::Value> {
        let sessions = self.agent_dir.join("sessions");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&sessions)
            .unwrap_or_else(|error| panic!("sessions dir {}: {error}", sessions.display()))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .collect();
        files.sort();
        assert_eq!(files.len(), 1, "one session file: {files:?}");
        read_entries(&files[0])
    }
}

fn read_entries(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("the session file is readable")
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .expect("the session file is valid jsonl")
}

/// What one entry is, for the order assertions: `done:<command>` for a `[bash-done]` row,
/// `assistant:<text>` for an assistant row (text blocks joined, empty for a tool call alone).
fn kinds(entries: &[serde_json::Value]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| {
            if entry.get("type").and_then(|t| t.as_str()) == Some("custom_message")
                && entry.get("customType").and_then(|t| t.as_str()) == Some("async_bash_completion")
            {
                let command = entry
                    .pointer("/details/command")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default();
                return Some(format!("done:{command}"));
            }
            if entry.get("type").and_then(|t| t.as_str()) == Some("message")
                && entry.pointer("/message/role").and_then(|r| r.as_str()) == Some("assistant")
            {
                let text: String = entry
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                    .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                    .collect();
                return Some(format!("assistant:{text}"));
            }
            None
        })
        .collect()
}

fn tool_call(code: &str) -> serde_json::Value {
    serde_json::json!({ "content": [
        { "type": "toolCall", "name": "ipython", "arguments": { "code": code } },
    ] })
}

const THREE_COMMANDS: &str =
    "a = bash(\"sleep 1\")\nb = bash(\"sleep 2.5\")\nc = bash(\"sleep 4\")\nprint(\"STARTED\")";

fn three_commands_script() -> serde_json::Value {
    serde_json::json!({
        "responses": [tool_call(THREE_COMMANDS), "WAITING", "WOKEN"],
        "repeatLastResponse": true,
    })
}

#[test]
fn notices_after_the_turn_wake_the_model_once() {
    let lane = lane(None);
    let (stdout, stderr, code) = lane.run(&three_commands_script());
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert_eq!(stdout.trim_end(), "WOKEN", "stderr: {stderr}");
    assert_eq!(
        kinds(&lane.entries()),
        [
            "assistant:",
            "assistant:WAITING",
            "done:sleep 1",
            "done:sleep 2.5",
            "done:sleep 4",
            "assistant:WOKEN",
        ],
    );
}

#[test]
fn window_zero_keeps_one_wake_per_notice() {
    let lane = lane(Some(&serde_json::json!({ "bashNoticeBatchSeconds": 0 })));
    let (stdout, stderr, code) = lane.run(&three_commands_script());
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert_eq!(
        kinds(&lane.entries()),
        [
            "assistant:",
            "assistant:WAITING",
            "done:sleep 1",
            "assistant:WOKEN",
            "done:sleep 2.5",
            "assistant:WOKEN",
            "done:sleep 4",
            "assistant:WOKEN",
        ],
    );
}

#[test]
fn a_notice_during_the_run_rides_the_next_call() {
    let lane = lane(None);
    let script = serde_json::json!({
        "responses": [
            tool_call("short = bash(\"sleep 1\")\nlong = bash(\"sleep 6\")\nprint(\"STARTED\")"),
            tool_call("import asyncio\nawait asyncio.sleep(3)\nprint(\"SLEPT\")"),
            "DONE",
        ],
        "repeatLastResponse": true,
    });
    let (stdout, stderr, code) = lane.run(&script);
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert_eq!(
        kinds(&lane.entries()),
        [
            "assistant:",
            "assistant:",
            // The short command ended during the second cell: its notice joined the next call.
            "done:sleep 1",
            "assistant:DONE",
            // The long one ended after the turn: one wake.
            "done:sleep 6",
            "assistant:DONE",
        ],
    );
}

#[test]
fn a_short_window_releases_a_notice_while_a_command_still_runs() {
    let lane = lane(Some(&serde_json::json!({ "bashNoticeBatchSeconds": 2 })));
    let script = serde_json::json!({
        "responses": [
            tool_call("short = bash(\"sleep 1\")\nlong = bash(\"sleep 8\")\nprint(\"STARTED\")"),
            "WAITING",
            "WOKEN",
        ],
        "repeatLastResponse": true,
    });
    let (stdout, stderr, code) = lane.run(&script);
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert_eq!(
        kinds(&lane.entries()),
        [
            "assistant:",
            "assistant:WAITING",
            "done:sleep 1",
            "assistant:WOKEN",
            "done:sleep 8",
            "assistant:WOKEN",
        ],
    );
}
