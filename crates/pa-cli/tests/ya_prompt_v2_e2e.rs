// too_many_lines: style gate, not correctness. Casts: 64-bit targets; the pid narrowing sits at
// an OS boundary.
#![allow(clippy::too_many_lines, clippy::cast_possible_wrap)]
#![cfg(unix)]

//! yusAgent (YA-09, ticket 11): the product defaults in print mode, with the real binary, the
//! scripted faux provider and a real Python kernel. `RLM_MAX_DEPTH` and
//! `PRIME_AGENT_PROMPT_VERSION` are removed (`.cargo/config.toml` sets them for the upstream
//! tests), as in `crates/pa-core/tests/prefix_stability.rs`.
//! a. A version-2 session: the short prompt, `edit()` creates then edits a file, `bash()` runs
//!    with its default limit of 600 s, and the header freezes version 2 and the kernel tools.
//! b. `rlmMaxDepth: 1` with version 2 is refused at start, with the setting to change.
//! c. Ticket 13: the default `bashWaitSeconds` (`"full"`): a cell that awaits a 3-second command
//!    gets its complete result, and no `[bash-done]` wake follows.

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
        // The edit skill of the checkout: an explicitly selected kernel skips skill installation.
        let skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut python_paths = vec![skills.join("edit/src")];
        if let Some(existing) = std::env::var_os("PYTHONPATH") {
            python_paths.extend(std::env::split_paths(&existing));
        }
        let mut command = std::process::Command::new(bin);
        command
            .args(["-p", "edit a file and run a command"])
            .env("HOME", self.home.path())
            .env("PRIME_AGENT_CODING_AGENT_DIR", &self.agent_dir)
            .env("PRIME_AGENT_FAUX_SCRIPT", script.to_string())
            .env("PRIME_AGENT_KERNEL_PYTHON", kernel_python())
            .env(
                "PYTHONPATH",
                std::env::join_paths(python_paths).expect("skill import paths"),
            )
            // The product defaults: `rlmMaxDepth` 0 and prompt version 2.
            .env_remove("RLM_MAX_DEPTH")
            .env_remove("PRIME_AGENT_PROMPT_VERSION")
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

    /// The entries of the one session file the run wrote (the header first).
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
        std::fs::read_to_string(&files[0])
            .expect("the session file is readable")
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .expect("the session file is valid jsonl")
    }
}

const CELL: &str = r#"print(await edit("notes.txt", "", "alpha\n"))
print(await edit("notes.txt", "alpha", "beta"))
r = await bash("cat notes.txt")
print("BASH", r.exit_code, r.output.strip())
import sys
print("TIMEOUT", sys.modules["rlm.ya_timeout"].DEFAULT_TIMEOUT)"#;

#[test]
fn a_version_two_session_edits_and_runs_commands() {
    let lane = lane(None);
    let script = serde_json::json!({
        "responses": [
            { "content": [
                { "type": "toolCall", "name": "ipython", "arguments": { "code": CELL } },
            ] },
            { "systemPrompt": true },
        ],
    });
    let (stdout, stderr, code) = lane.run(&script);
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    // The last answer is the system prompt of the request: version 2.
    assert!(stdout.starts_with("# yusAgent"), "{stdout}");
    assert!(
        stdout.contains("You have one tool, `python_repl`"),
        "{stdout}"
    );
    assert!(stdout.contains("timeout: float | None = 600"), "{stdout}");
    assert!(!stdout.contains("Recursive agent depth: 0"), "{stdout}");
    assert!(!stdout.contains("<available_skills>"), "{stdout}");
    // The rule on existing tests, on by default in version 2 (ticket 18, 19/11/2026).
    assert_eq!(
        stdout
            .matches("- Existing tests encode expected behavior:")
            .count(),
        1,
        "{stdout}"
    );

    assert_eq!(
        std::fs::read_to_string(lane.home.path().join("notes.txt")).unwrap(),
        "beta\n"
    );
    let entries = lane.entries();
    let header = &entries[0];
    assert_eq!(header["type"], "session", "{header}");
    assert_eq!(
        header["yaRequest"],
        serde_json::json!({"reasoningSummary": "off", "bashWaitSeconds": "full", "promptVersion": 2, "codexSessionIdHeader": false, "existingTestsRule": true})
    );
    assert_eq!(
        header["yaTools"],
        serde_json::json!({"editTool": "kernel", "shellTool": "kernel"})
    );
    let output = entries
        .iter()
        .filter(|entry| {
            entry.get("type").and_then(|t| t.as_str()) == Some("message")
                && entry.pointer("/message/role").and_then(|r| r.as_str()) == Some("toolResult")
        })
        .filter_map(|entry| entry.pointer("/message/content").and_then(|c| c.as_array()))
        .flatten()
        .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
        .collect::<String>();
    for expected in [
        "Created ",
        "     1\talpha",
        "Edited ",
        "     1\tbeta",
        "BASH 0 beta",
        "TIMEOUT 600.0",
    ] {
        assert!(output.contains(expected), "{expected}: {output}");
    }
}

const FULL_WAIT_CELL: &str = r#"r = await bash("sleep 3; echo fini")
print("WAIT", type(r).__name__, r.exit_code, r.output.strip())
import sys
print("YIELD", sys.modules["rlm.bash"]._YIELD_AFTER_SECONDS)"#;

#[test]
fn the_default_wait_returns_the_complete_result() {
    let lane = lane(None);
    let script = serde_json::json!({
        "responses": [
            { "content": [
                { "type": "toolCall", "name": "ipython", "arguments": { "code": FULL_WAIT_CELL } },
            ] },
            "done",
        ],
    });
    let (stdout, stderr, code) = lane.run(&script);
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert_eq!(stdout.trim(), "done", "stderr: {stderr}");
    let entries = lane.entries();
    assert_eq!(
        entries[0]["yaRequest"]["bashWaitSeconds"],
        serde_json::json!("full")
    );
    let text = serde_json::Value::Array(entries).to_string();
    assert!(text.contains("WAIT BashResult 0 fini"), "{text}");
    assert!(text.contains("YIELD None"), "{text}");
    assert!(!text.contains("[bash-done]"), "no wake: {text}");
}

#[test]
fn version_two_with_subagents_is_refused_at_start() {
    let lane = lane(Some(&serde_json::json!({ "rlmMaxDepth": 1 })));
    let script = serde_json::json!({ "responses": ["never reached"] });
    let (stdout, stderr, code) = lane.run(&script);
    assert_ne!(code, 0, "stdout: {stdout}\nstderr: {stderr}");
    assert!(stderr.contains("\"promptVersion\": 1"), "stderr: {stderr}");
    assert!(!stdout.contains("never reached"), "stdout: {stdout}");
}
