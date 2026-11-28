//! yusAgent (YA-18, ticket 43, PR 3a): the checkpoints' `git` (`docs/tickets/43-retour-arriere.md`,
//! §P.1, §P.12.3). Resolved once per process; launched under the guard's checkpoints variant
//! (`ya_guard::checkpoints_tokio`, answer 8), with an empty environment but for what it needs and
//! no configuration but the shadow repository's: no system or global file, no `excludesFile`, no
//! hook, no fsmonitor, no transport (the options upstream's `a3e261c27` gives its gate snapshots,
//! plus the configuration isolation). A filter or a diff driver a project's `.gitattributes`
//! names has no definition to run.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::io::AsyncWriteExt;

/// The test variable that replaces the resolution (answer Q-3a-1). Read here only; the kernel
/// never gets it (its environment is an allowlist, `pa_core::kernel::env_allowlist`).
pub(super) const TEST_GIT: &str = "PRIME_AGENT_TEST_CHECKPOINTS_GIT";
pub(super) const NO_GIT: &str = "Workspace snapshots are off: git was not found.";
pub(super) const NO_TOOLS: &str =
    "Workspace snapshots are off: the command line developer tools are not installed.";
const EMAIL: &str = "checkpoints@yusagent.invalid";
/// One git call at most (estimation: a first snapshot of 10 000 files takes about 3 s, §P.3).
const TIMEOUT: Duration = Duration::from_secs(60);
/// Command-line configuration, which outranks the shadow repository's.
const OVERRIDES: [&str; 14] = [
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.excludesFile=/dev/null",
    "-c",
    "gc.auto=0",
    "-c",
    "core.autocrlf=false",
    "-c",
    "core.symlinks=true",
    "-c",
    "advice.addEmbeddedRepo=false",
];

/// The git program, or why the snapshots are off; resolved once per process.
pub(super) fn program() -> Result<PathBuf, &'static str> {
    static RESOLVED: OnceLock<Result<PathBuf, &'static str>> = OnceLock::new();
    RESOLVED.get_or_init(resolve).clone()
}

fn resolve() -> Result<PathBuf, &'static str> {
    if let Some(test) = std::env::var_os(TEST_GIT) {
        let path = PathBuf::from(test);
        return if executable(&path) {
            Ok(path)
        } else {
            Err(NO_GIT)
        };
    }
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let found = std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .find(|candidate| executable(candidate))
        })
        .ok_or(NO_GIT)?;
    // macOS: `/usr/bin/git` without the command line tools opens their installer.
    if cfg!(target_os = "macos") && found == Path::new("/usr/bin/git") {
        let tools = pa_types::platform::ya_guard::command("/usr/bin/xcode-select")
            .arg("-p")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !tools.is_ok_and(|status| status.success()) {
            return Err(NO_TOOLS);
        }
    }
    Ok(found)
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// The git of one shadow repository and its work tree.
pub(super) struct Git<'a> {
    pub(super) program: &'a Path,
    pub(super) repo: &'a Path,
    pub(super) work: &'a Path,
}

impl Git<'_> {
    /// `git --git-dir --work-tree <overrides> <args>`, with `index` as the index and `input` on
    /// stdin: its stdout.
    pub(super) async fn run(
        &self,
        index: Option<&Path>,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        let mut command = pa_types::platform::ya_guard::checkpoints_tokio(self.program);
        command
            .arg(format!("--git-dir={}", self.repo.display()))
            .arg(format!("--work-tree={}", self.work.display()))
            .args(OVERRIDES)
            .args(args)
            .current_dir(self.work);
        environment(&mut command, index);
        output(command, args, input).await
    }
}

/// `git init -q --bare <repo>`.
pub(super) async fn init(program: &Path, repo: &Path) -> Result<(), String> {
    let mut command = pa_types::platform::ya_guard::checkpoints_tokio(program);
    let target = repo.display().to_string();
    let args = ["init", "-q", "--bare", target.as_str()];
    command.args(args);
    environment(&mut command, None);
    output(command, &args, None).await.map(|_| ())
}

fn environment(command: &mut tokio::process::Command, index: Option<&Path>) {
    command.env_clear();
    for name in ["HOME", "PATH", "TMPDIR", "SYSTEMROOT"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("LANG", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_AUTHOR_NAME", "yusagent")
        .env("GIT_AUTHOR_EMAIL", EMAIL)
        .env("GIT_COMMITTER_NAME", "yusagent")
        .env("GIT_COMMITTER_EMAIL", EMAIL);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
}

async fn output(
    mut command: tokio::process::Command,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let name = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .copied()
        .unwrap_or("git");
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("git {name}: {error}"))?;
    // Written apart from the wait: git may fill its stdout before it reads all of stdin.
    let writer = match (input, child.stdin.take()) {
        (Some(input), Some(mut stdin)) => {
            let input = input.to_vec();
            Some(tokio::spawn(async move {
                let _ = stdin.write_all(&input).await;
            }))
        }
        _ => None,
    };
    let done = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| format!("git {name}: timed out"))?
        .map_err(|error| format!("git {name}: {error}"))?;
    if let Some(writer) = writer {
        let _ = writer.await;
    }
    if done.status.success() {
        Ok(done.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&done.stderr);
        Err(format!(
            "git {name}: {}",
            stderr.trim().chars().take(500).collect::<String>()
        ))
    }
}
