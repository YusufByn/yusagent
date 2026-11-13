//! yusAgent (YA-03, ticket 09): the native shell of the `shellTool: "native"` variant, Codex's
//! unified exec tools `exec_command` and `write_stdin` (openai/codex `69231e1`).
//!
//! Model-facing contract, word for word from Codex (`codex-rs/core/src/tools/handlers/
//! shell_spec.rs:27-152`, non-Windows text): names, descriptions, parameter descriptions,
//! `required`, `additionalProperties: false`. Kept parameters: `cmd`, `workdir`,
//! `yield_time_ms`, `max_output_tokens` (`exec_command`); `session_id`, `chars`,
//! `yield_time_ms`, `max_output_tokens` (`write_stdin`). Dropped: `tty` (no PTY; Codex's
//! default is `false` too), `shell`, `login` (a login shell would re-read the user's profile
//! and bring back what the environment filter removes), the sandbox and approval parameters
//! (no sandbox before YA-11).
//!
//! Behavior, as Codex without a PTY (`codex-rs/core/src/unified_exec/`):
//! - one shell process per call (`<shell> -c <cmd>`), in its own process group, stdin closed,
//!   stdout and stderr merged;
//! - the call waits until the process exits and its output closes, or until `yield_time_ms`
//!   (default 10 000, clamped to 250-30 000); a process still running keeps a session id the
//!   model passes to `write_stdin`;
//! - `write_stdin` with empty `chars` polls (wait clamped to 5 000-300 000 ms), with `"\u0003"`
//!   interrupts the process group (SIGINT), with anything else fails: stdin is closed
//!   (`process_manager.rs:1018-1022`, 1463);
//! - at most 64 live sessions; past that, the least recently used one outside the 8 most
//!   recent is dropped, exited ones first (`process_manager.rs:1719-1792`); every process
//!   group is killed when the session's tools are dropped.
//!
//! Result text: Codex's header (`codex-rs/core/src/tools/context.rs:545-569`, without the
//! `Chunk ID` line) then the output. Output budget, as Codex (`context.rs:527-596`): the
//! requested `max_output_tokens` (default 10 000) and the session policy, the smaller one, at
//! 4 bytes per token (`codex-rs/utils/string/src/truncate.rs:6`, 94-107); the whole response
//! fits the session policy plus 20 % (`codex-rs/utils/output-truncation/src/lib.rs:22-24`),
//! the output budget shrinking until it does. The session policy is `kernel.maxOutputChars`,
//! read as bytes here. `Original token count` is the byte count of the collected output over
//! 4, rounded up (`process_manager.rs:675-678`). Over budget, the middle is replaced by one
//! line naming the file that holds the full output (`tools::ya_output_cap`).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use crate::tools::tool_definition::{
    AbortSignal, ToolContentBlock, ToolDefinition, ToolExecutionResult,
};

pub const EXEC_COMMAND_TOOL_NAME: &str = "exec_command";
pub const WRITE_STDIN_TOOL_NAME: &str = "write_stdin";

const EXEC_COMMAND_DESCRIPTION: &str =
    "Runs a command in a PTY, returning output or a session ID for ongoing interaction.";
const WRITE_STDIN_DESCRIPTION: &str =
    "Writes characters to an existing unified exec session and returns recent output.";
const MAX_OUTPUT_TOKENS_DESCRIPTION: &str =
    "Output token budget. Defaults to 10000 tokens; larger requests may be capped by policy.";

const DEFAULT_EXEC_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_WRITE_STDIN_YIELD_TIME_MS: u64 = 250;
const MIN_YIELD_TIME_MS: u64 = 250;
const MIN_EMPTY_YIELD_TIME_MS: u64 = 5_000;
const MAX_YIELD_TIME_MS: u64 = 30_000;
const MAX_EMPTY_YIELD_TIME_MS: u64 = 300_000;
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
const APPROX_BYTES_PER_TOKEN: usize = 4;
const MAX_PROCESSES: usize = 64;
const PROTECTED_RECENT_PROCESSES: usize = 8;
const POST_EXIT_CLOSE_WAIT: Duration = Duration::from_millis(50);
const FIRST_SESSION_ID: i32 = 1000;
const INTERRUPT: &str = "\u{3}";

/// What every command of one session runs with.
#[derive(Debug, Clone)]
pub struct ExecConfig {
    /// The session cwd; `workdir` resolves against it.
    pub cwd: PathBuf,
    /// The shell run as `<shell> -c <command>` (the kernel's `bash()` shell).
    pub shell: PathBuf,
    /// `shellCommandPrefix`, prepended on its own line as the kernel does.
    pub command_prefix: Option<String>,
    /// The full child environment (see [`shell_env`]).
    pub env: HashMap<String, String>,
    /// Session truncation policy in bytes (`kernel.maxOutputChars`); `0`: none.
    pub policy_bytes: usize,
    /// Where full outputs over budget are written.
    pub output_dir: PathBuf,
}

/// The child environment: the kernel's filter (`kernel::env_allowlist`: the built-in
/// allowlist, `kernel.envAllowlist`, never a provider credential) plus the non-interactive
/// guard of the kernel's `bash()` (`prime-agent-runtime/src/rlm/bash.py`, `_child_env`).
#[must_use]
pub fn shell_env(allowlist: &[String]) -> HashMap<String, String> {
    let mut env = crate::kernel::env_allowlist::inherited_env(std::env::vars());
    crate::kernel::env_allowlist::apply_user_allowlist(&mut env, allowlist, std::env::vars());
    crate::kernel::env_allowlist::strip_harness_credentials(&mut env);
    for (key, value) in [
        ("NO_COLOR", "1"),
        ("TERM", "dumb"),
        ("CLICOLOR", "0"),
        ("FORCE_COLOR", "0"),
        ("GIT_EDITOR", "true"),
        ("GIT_SEQUENCE_EDITOR", "true"),
        ("GIT_TERMINAL_PROMPTS", "0"),
        ("GIT_ASKPASS", "true"),
        ("SSH_ASKPASS_REQUIRE", "never"),
        ("EDITOR", "true"),
        ("VISUAL", "true"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("DEBIAN_FRONTEND", "noninteractive"),
    ] {
        env.insert(key.to_string(), value.to_string());
    }
    env
}

/// Output collected from a process, shared with its reader and waiter tasks.
#[derive(Default)]
struct OutputState {
    pending: Vec<u8>,
    open_streams: u8,
    exit_code: Option<i32>,
    exited: bool,
}

struct Process {
    pid: Option<u32>,
    state: Arc<Mutex<OutputState>>,
    notify: Arc<tokio::sync::Notify>,
    last_used: Mutex<Instant>,
}

impl Process {
    fn has_exited(&self) -> bool {
        lock(&self.state).exited
    }

    fn interrupt(&self) {
        let Some(pid) = self.pid else { return };
        #[cfg(unix)]
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGINT,
            );
        }
        #[cfg(not(unix))]
        {
            let _ = crate::platform::process::kill_process_group_or_pid(pid as i32);
        }
    }

    fn kill(&self) {
        if self.has_exited() {
            return;
        }
        if let Some(pid) = self.pid {
            let _ = crate::platform::process::kill_process_group_or_pid(pid as i32);
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The live processes of one session; dropping it kills them.
pub struct ExecManager {
    config: ExecConfig,
    processes: Mutex<HashMap<i32, Arc<Process>>>,
    next_id: Mutex<i32>,
}

/// One tool answer: the text and whether it is a tool error.
struct Answer {
    text: String,
    is_error: bool,
}

impl Answer {
    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

impl ExecManager {
    #[must_use]
    pub fn new(config: ExecConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            processes: Mutex::new(HashMap::new()),
            next_id: Mutex::new(FIRST_SESSION_ID),
        })
    }

    async fn exec_command(
        &self,
        call_id: &str,
        args: &Value,
        signal: Option<AbortSignal>,
    ) -> Answer {
        let Some(cmd) = args.get("cmd").and_then(Value::as_str) else {
            return Answer::error("missing command line for unified exec request");
        };
        let workdir = match args.get("workdir").and_then(Value::as_str) {
            Some(dir) if !dir.is_empty() => self.config.cwd.join(dir),
            _ => self.config.cwd.clone(),
        };
        let yield_time_ms = number(args, "yield_time_ms")
            .unwrap_or(DEFAULT_EXEC_YIELD_TIME_MS)
            .clamp(MIN_YIELD_TIME_MS, MAX_YIELD_TIME_MS);
        let max_output_tokens = number(args, "max_output_tokens").map(|n| n as usize);
        let start = Instant::now();
        let process = match self.spawn(cmd, &workdir) {
            Ok(process) => process,
            Err(error) => {
                return Answer::error(format!("Failed to create unified exec process: {error}"))
            }
        };
        let deadline = start + Duration::from_millis(yield_time_ms);
        let output = collect_until(&process, deadline, signal.as_ref()).await;
        let (exit_code, exited) = {
            let state = lock(&process.state);
            (state.exit_code, state.exited)
        };
        let session_id = if exited {
            None
        } else {
            Some(self.store(process))
        };
        self.answer(
            call_id,
            start,
            &output,
            exit_code,
            session_id,
            max_output_tokens,
        )
    }

    async fn write_stdin(
        &self,
        call_id: &str,
        args: &Value,
        signal: Option<AbortSignal>,
    ) -> Answer {
        let Some(session_id) = number(args, "session_id").map(|n| n as i32) else {
            return Answer::error("session_id is required");
        };
        let chars = args.get("chars").and_then(Value::as_str).unwrap_or("");
        let Some(process) = lock(&self.processes).get(&session_id).cloned() else {
            return Answer::error(format!("Unknown process id {session_id}"));
        };
        *lock(&process.last_used) = Instant::now();
        if !chars.is_empty() && !process.has_exited() {
            if chars == INTERRUPT {
                process.interrupt();
            } else {
                return Answer::error("stdin is closed for this session");
            }
        }
        let requested = number(args, "yield_time_ms")
            .unwrap_or(DEFAULT_WRITE_STDIN_YIELD_TIME_MS)
            .max(MIN_YIELD_TIME_MS);
        let yield_time_ms = if chars.is_empty() {
            requested.clamp(MIN_EMPTY_YIELD_TIME_MS, MAX_EMPTY_YIELD_TIME_MS)
        } else {
            requested.min(MAX_YIELD_TIME_MS)
        };
        let max_output_tokens = number(args, "max_output_tokens").map(|n| n as usize);
        let start = Instant::now();
        let output = collect_until(
            &process,
            start + Duration::from_millis(yield_time_ms),
            signal.as_ref(),
        )
        .await;
        let (exit_code, exited) = {
            let state = lock(&process.state);
            (state.exit_code, state.exited)
        };
        let session = if exited {
            lock(&self.processes).remove(&session_id);
            None
        } else {
            Some(session_id)
        };
        self.answer(
            call_id,
            start,
            &output,
            exit_code,
            session,
            max_output_tokens,
        )
    }

    fn spawn(&self, cmd: &str, workdir: &Path) -> std::io::Result<Arc<Process>> {
        let script = match &self.config.command_prefix {
            Some(prefix) => format!("{prefix}\n{cmd}"),
            None => cmd.to_string(),
        };
        let mut command = tokio::process::Command::new(&self.config.shell);
        command
            .arg("-c")
            .arg(script)
            .current_dir(workdir)
            .env_clear()
            .envs(&self.config.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let state = Arc::new(Mutex::new(OutputState {
            open_streams: 2,
            ..OutputState::default()
        }));
        let notify = Arc::new(tokio::sync::Notify::new());
        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, Arc::clone(&state), Arc::clone(&notify));
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, Arc::clone(&state), Arc::clone(&notify));
        }
        let pid = child.id();
        {
            let state = Arc::clone(&state);
            let notify = Arc::clone(&notify);
            tokio::spawn(async move {
                let code = match child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };
                let mut guard = lock(&state);
                guard.exit_code = Some(code);
                guard.exited = true;
                drop(guard);
                notify.notify_waiters();
            });
        }
        Ok(Arc::new(Process {
            pid,
            state,
            notify,
            last_used: Mutex::new(Instant::now()),
        }))
    }

    /// Keep a still-running process under a new session id, pruning past the limit.
    fn store(&self, process: Arc<Process>) -> i32 {
        let id = {
            let mut next = lock(&self.next_id);
            let id = *next;
            *next += 1;
            id
        };
        let mut processes = lock(&self.processes);
        if processes.len() >= MAX_PROCESSES {
            if let Some(victim) = prune_candidate(&processes) {
                processes.remove(&victim);
            }
        }
        processes.insert(id, process);
        id
    }

    fn answer(
        &self,
        call_id: &str,
        start: Instant,
        output: &[u8],
        exit_code: Option<i32>,
        session_id: Option<i32>,
        max_output_tokens: Option<usize>,
    ) -> Answer {
        let wall_time = start.elapsed().as_secs_f64();
        let mut header = format!("Wall time: {wall_time:.4} seconds");
        if let Some(code) = exit_code.filter(|_| session_id.is_none()) {
            let _ = write!(header, "\nProcess exited with code {code}");
        }
        if let Some(id) = session_id {
            let _ = write!(header, "\nProcess running with session ID {id}");
        }
        let _ = write!(
            header,
            "\nOriginal token count: {}",
            approx_tokens_from_byte_count(output.len())
        );
        header.push_str("\nOutput:");
        let text = String::from_utf8_lossy(output);
        let body = budgeted_output(
            &text,
            &header,
            self.config.policy_bytes,
            max_output_tokens,
            |full| {
                crate::tools::ya_output_cap::save_full_output(
                    &self.config.output_dir,
                    call_id,
                    full,
                )
            },
        );
        Answer {
            text: format!("{header}\n{body}"),
            is_error: false,
        }
    }
}

/// The least recently used session outside the most recent ones, exited first (Codex's
/// `process_id_to_prune_from_meta`).
fn prune_candidate(processes: &HashMap<i32, Arc<Process>>) -> Option<i32> {
    let mut by_use: Vec<(i32, Instant, bool)> = processes
        .iter()
        .map(|(id, process)| (*id, *lock(&process.last_used), process.has_exited()))
        .collect();
    by_use.sort_by_key(|(_, used, _)| *used);
    let protected: Vec<i32> = by_use
        .iter()
        .rev()
        .take(PROTECTED_RECENT_PROCESSES)
        .map(|(id, _, _)| *id)
        .collect();
    by_use
        .iter()
        .find(|(id, _, exited)| *exited && !protected.contains(id))
        .or_else(|| by_use.iter().find(|(id, _, _)| !protected.contains(id)))
        .map(|(id, _, _)| *id)
}

fn spawn_reader<R>(mut reader: R, state: Arc<Mutex<OutputState>>, notify: Arc<tokio::sync::Notify>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 8192];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    lock(&state).pending.extend_from_slice(&buffer[..read]);
                    notify.notify_waiters();
                }
            }
        }
        {
            let mut guard = lock(&state);
            guard.open_streams = guard.open_streams.saturating_sub(1);
        }
        notify.notify_waiters();
    });
}

/// Drain the process output until it exited and its output closed (at most 50 ms after the
/// exit), until `deadline`, or until the call is cancelled (Codex's
/// `collect_output_until_deadline`).
async fn collect_until(
    process: &Process,
    deadline: Instant,
    signal: Option<&AbortSignal>,
) -> Vec<u8> {
    let mut collected = Vec::new();
    let mut post_exit_deadline: Option<Instant> = None;
    loop {
        let notified = process.notify.notified();
        tokio::pin!(notified);
        let _ = notified.as_mut().enable();
        let (exited, closed) = {
            let mut state = lock(&process.state);
            collected.append(&mut state.pending);
            (state.exited, state.open_streams == 0)
        };
        if exited && closed {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let mut wait_until = deadline;
        if exited {
            let close_by = *post_exit_deadline
                .get_or_insert_with(|| now + (deadline - now).min(POST_EXIT_CLOSE_WAIT));
            if now >= close_by {
                break;
            }
            wait_until = close_by;
        }
        let cancelled = async {
            match signal {
                Some(signal) => signal.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = &mut notified => {}
            () = tokio::time::sleep_until(tokio::time::Instant::from_std(wait_until)) => {}
            () = cancelled => break,
        }
    }
    collected
}

fn number(args: &Value, key: &str) -> Option<u64> {
    let value = args.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
}

/// `ceil(bytes / 4)` (Codex's `approx_tokens_from_byte_count`).
fn approx_tokens_from_byte_count(bytes: usize) -> usize {
    bytes.saturating_add(APPROX_BYTES_PER_TOKEN - 1) / APPROX_BYTES_PER_TOKEN
}

/// The output as Codex budgets it (`context.rs:527-596`): the smaller of the requested and the
/// session budgets, then shrunk until header and output fit the session budget plus 20 %.
fn budgeted_output(
    output: &str,
    header: &str,
    policy_bytes: usize,
    max_output_tokens: Option<usize>,
    save: impl Fn(&str) -> std::io::Result<PathBuf>,
) -> String {
    let requested = max_output_tokens
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
        .saturating_mul(APPROX_BYTES_PER_TOKEN);
    let policy = if policy_bytes == 0 {
        usize::MAX
    } else {
        policy_bytes
    };
    let mut budget = requested.min(policy);
    if output.len() <= budget {
        return output.to_string();
    }
    // Codex: `with_serialization_allowance` (x 1.2) minus the header and its newline.
    let response_budget = if policy == usize::MAX {
        usize::MAX
    } else {
        (policy as f64 * 1.2) as usize
    }
    .saturating_sub(header.len() + 1);
    let marker = match save(output) {
        Ok(path) => format!(
            "\n[... output truncated: {} bytes in total; full output in {} ...]\n",
            output.len(),
            path.display()
        ),
        Err(error) => format!(
            "\n[... output truncated: {} bytes in total; the full output could not be saved: {error} ...]\n",
            output.len()
        ),
    };
    let mut rendered = truncate_middle(output, budget, &marker);
    while rendered.len() > response_budget && budget > 0 {
        budget = budget.saturating_sub(rendered.len() - response_budget);
        rendered = truncate_middle(output, budget, &marker);
    }
    rendered
}

/// The start and end of `text` around `marker`, `budget` bytes in all (char boundaries kept).
fn truncate_middle(text: &str, budget: usize, marker: &str) -> String {
    let keep = budget.saturating_sub(marker.len());
    let head = floor_boundary(text, keep / 2);
    let tail = ceil_boundary(text, text.len() - (keep - keep / 2).min(text.len()));
    let tail = tail.max(head);
    format!("{}{marker}{}", &text[..head], &text[tail..])
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// `exec_command` and `write_stdin`, sharing one manager, in Codex's order.
#[must_use]
pub fn create_exec_tool_definitions(config: ExecConfig) -> Vec<ToolDefinition> {
    let manager = ExecManager::new(config);
    let exec_manager = Arc::clone(&manager);
    let exec_command = ToolDefinition {
        name: EXEC_COMMAND_TOOL_NAME.to_string(),
        label: EXEC_COMMAND_TOOL_NAME.to_string(),
        description: EXEC_COMMAND_DESCRIPTION.to_string(),
        prompt_snippet: "exec_command - run a shell command".to_string(),
        parameters: exec_command_schema(),
        execution_mode: None,
        prepare_arguments: None,
        execute: Arc::new(move |tool_call_id: &str, params, signal, _on_update| {
            let manager = Arc::clone(&exec_manager);
            let tool_call_id = tool_call_id.to_string();
            Box::pin(async move {
                let answer = manager.exec_command(&tool_call_id, &params, signal).await;
                Ok(to_result(answer))
            })
        }),
    };
    let write_stdin = ToolDefinition {
        name: WRITE_STDIN_TOOL_NAME.to_string(),
        label: WRITE_STDIN_TOOL_NAME.to_string(),
        description: WRITE_STDIN_DESCRIPTION.to_string(),
        prompt_snippet: "write_stdin - poll or interrupt a running command".to_string(),
        parameters: write_stdin_schema(),
        execution_mode: None,
        prepare_arguments: None,
        execute: Arc::new(move |tool_call_id: &str, params, signal, _on_update| {
            let manager = Arc::clone(&manager);
            let tool_call_id = tool_call_id.to_string();
            Box::pin(async move {
                let answer = manager.write_stdin(&tool_call_id, &params, signal).await;
                Ok(to_result(answer))
            })
        }),
    };
    vec![exec_command, write_stdin]
}

fn to_result(answer: Answer) -> ToolExecutionResult {
    ToolExecutionResult {
        content: vec![ToolContentBlock::text(answer.text)],
        details: None,
        is_error: answer.is_error,
    }
}

/// Codex's `exec_command` schema (keys in Codex's `BTreeMap` order).
fn exec_command_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cmd": {"type": "string", "description": "Shell command to execute."},
            "max_output_tokens": {"type": "number", "description": MAX_OUTPUT_TOKENS_DESCRIPTION},
            "workdir": {"type": "string", "description": "Working directory for the command. Defaults to the turn cwd."},
            "yield_time_ms": {"type": "number", "description": "Wait before yielding output. Defaults to 10000 ms; effective range is 250-30000 ms."},
        },
        "required": ["cmd"],
        "additionalProperties": false,
    })
}

/// Codex's `write_stdin` schema (keys in Codex's `BTreeMap` order).
fn write_stdin_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chars": {"type": "string", "description": "Bytes to write to stdin. Defaults to empty, which polls without writing."},
            "max_output_tokens": {"type": "number", "description": MAX_OUTPUT_TOKENS_DESCRIPTION},
            "session_id": {"type": "number", "description": "Identifier of the running unified exec session."},
            "yield_time_ms": {"type": "number", "description": "Wait before yielding output. Non-empty writes default to 250 ms and cap at 30000 ms; empty polls wait 5000-300000 ms by default."},
        },
        "required": ["session_id"],
        "additionalProperties": false,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn config(dir: &Path) -> ExecConfig {
        ExecConfig {
            cwd: dir.to_path_buf(),
            shell: PathBuf::from("/bin/sh"),
            command_prefix: None,
            env: shell_env(&[]),
            policy_bytes: 40_000,
            output_dir: dir.join("outputs"),
        }
    }

    #[test]
    fn schemas_are_codex_word_for_word() {
        let exec = exec_command_schema();
        let keys: Vec<&String> = exec["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["cmd", "max_output_tokens", "workdir", "yield_time_ms"]
        );
        assert_eq!(exec["required"], json!(["cmd"]));
        let stdin = write_stdin_schema();
        let keys: Vec<&String> = stdin["properties"].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["chars", "max_output_tokens", "session_id", "yield_time_ms"]
        );
        assert_eq!(stdin["required"], json!(["session_id"]));
    }

    #[tokio::test]
    async fn finished_command_reports_exit_code_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ExecManager::new(config(dir.path()));
        let answer = manager
            .exec_command("c1", &json!({"cmd": "echo hi; echo err >&2; exit 3"}), None)
            .await;
        assert!(!answer.is_error, "{}", answer.text);
        assert!(answer.text.starts_with("Wall time: "), "{}", answer.text);
        assert!(
            answer.text.contains("\nProcess exited with code 3\n"),
            "{}",
            answer.text
        );
        assert!(
            answer.text.contains("\nOriginal token count: 2\nOutput:\n"),
            "{}",
            answer.text
        );
        assert!(answer.text.contains("hi\n") && answer.text.contains("err\n"));
        assert!(!answer.text.contains("session ID"));
    }

    #[tokio::test]
    async fn workdir_resolves_against_the_session_cwd() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let manager = ExecManager::new(config(dir.path()));
        let answer = manager
            .exec_command("c1", &json!({"cmd": "pwd", "workdir": "sub"}), None)
            .await;
        assert!(answer.text.trim_end().ends_with("/sub"), "{}", answer.text);
    }

    #[tokio::test]
    async fn long_command_yields_a_session_then_polls_to_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ExecManager::new(config(dir.path()));
        let answer = manager
            .exec_command(
                "c1",
                &json!({"cmd": "echo start; sleep 1; echo done", "yield_time_ms": 250}),
                None,
            )
            .await;
        assert!(
            answer.text.contains("Process running with session ID 1000"),
            "{}",
            answer.text
        );
        assert!(!answer.text.contains("Process exited"));
        let polled = manager
            .write_stdin("c2", &json!({"session_id": 1000, "chars": ""}), None)
            .await;
        assert!(
            polled.text.contains("Process exited with code 0"),
            "{}",
            polled.text
        );
        assert!(polled.text.contains("done\n"), "{}", polled.text);
        let gone = manager
            .write_stdin("c3", &json!({"session_id": 1000}), None)
            .await;
        assert!(gone.is_error);
        assert_eq!(gone.text, "Unknown process id 1000");
    }

    #[tokio::test]
    async fn stdin_is_closed_but_interrupt_works() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ExecManager::new(config(dir.path()));
        let answer = manager
            .exec_command(
                "c1",
                &json!({"cmd": "sleep 30", "yield_time_ms": 250}),
                None,
            )
            .await;
        assert!(answer.text.contains("session ID 1000"), "{}", answer.text);
        let refused = manager
            .write_stdin("c2", &json!({"session_id": 1000, "chars": "y\n"}), None)
            .await;
        assert!(refused.is_error);
        assert_eq!(refused.text, "stdin is closed for this session");
        let interrupted = manager
            .write_stdin(
                "c3",
                &json!({"session_id": 1000, "chars": "\u{3}", "yield_time_ms": 2000}),
                None,
            )
            .await;
        assert!(
            interrupted.text.contains("Process exited with code"),
            "{}",
            interrupted.text
        );
    }

    #[tokio::test]
    async fn environment_is_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config(dir.path());
        config.env.remove("OPENAI_API_KEY");
        let manager = ExecManager::new(config);
        let answer = manager
            .exec_command(
                "c1",
                &json!({"cmd": "echo \"[${OPENAI_API_KEY:-unset}] $PAGER\""}),
                None,
            )
            .await;
        assert!(answer.text.contains("[unset] cat"), "{}", answer.text);
    }

    #[test]
    fn small_output_is_untouched() {
        let out = budgeted_output("abc", "Output:", 40_000, None, |_| unreachable!());
        assert_eq!(out, "abc");
    }

    #[test]
    fn requested_budget_truncates_and_saves_the_full_output() {
        let dir = tempfile::tempdir().unwrap();
        let output: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let out = budgeted_output(&output, "Output:", 40_000, Some(100), |full| {
            crate::tools::ya_output_cap::save_full_output(dir.path(), "c", full)
        });
        assert!(out.len() <= 400, "{}", out.len());
        assert!(out.starts_with("line 0\n"));
        assert!(out.ends_with("line 1999\n"));
        assert!(out.contains("bytes in total; full output in"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("c.txt")).unwrap(),
            output
        );
    }

    #[test]
    fn response_fits_the_policy_plus_twenty_percent() {
        let output = "x".repeat(100_000);
        let header = "h".repeat(30);
        // Requested budget above the policy: the policy (1 000 bytes) wins.
        let out = budgeted_output(&output, &header, 1_000, Some(100_000), |_| {
            Ok(PathBuf::from("/o/c.txt"))
        });
        assert!(out.len() <= 1_000, "{}", out.len());
        assert!(header.len() + 1 + out.len() <= 1_200);
    }

    #[test]
    fn token_count_rounds_up() {
        assert_eq!(approx_tokens_from_byte_count(0), 0);
        assert_eq!(approx_tokens_from_byte_count(1), 1);
        assert_eq!(approx_tokens_from_byte_count(8), 2);
        assert_eq!(approx_tokens_from_byte_count(9), 3);
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        let text = "é".repeat(500);
        let out = truncate_middle(&text, 101, "|");
        assert!(out.starts_with('é') && out.ends_with('é'));
        assert!(out.len() <= 101);
    }
}
