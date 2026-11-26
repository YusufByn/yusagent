//! yusAgent (YA-11, ticket 41, PR A): the secret guard. Every process yusAgent launches runs
//! under a Seatbelt profile (macOS) that refuses the credential files, the daemon's sockets, the
//! running binary and `LaunchServices`; the rest of the system stays open.
//!
//! - [`command`]: every launch (macOS: wrapped by `sandbox-exec`; elsewhere unchanged until the
//!   Linux mechanism, PR C).
//! - [`model_command`]: the model's executors (the kernel, `exec_command`). Where the guard has
//!   no mechanism, they refuse to start with a message naming the switch (answer 8).
//! - [`browser`]: the browser opener, `LaunchServices` left open (it launches the
//!   browser), after [`browser_url_allowed`] (`https://`, `http://` to localhost).
//! - Switch: `PRIME_AGENT_SECRET_GUARD=off` in the environment of the process that launches;
//!   never passed to the kernel (`env_allowlist.rs`).
//!
//! Listed in `docs/fork-changes.md`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// The switch: `off` turns the guard off.
pub const SWITCH_ENV: &str = "PRIME_AGENT_SECRET_GUARD";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// The program a refused browser URL gets: it does not exist, the launch fails.
const REFUSED_URL: &str = "/nonexistent/yusagent-refused-url";
/// The refusal printed when the guard cannot wrap a launch.
pub const UNAVAILABLE: &str = "yusAgent: the secret guard has no mechanism here (ticket 41); \
the model's code is not started. Set PRIME_AGENT_SECRET_GUARD=off in the daemon's environment to \
run without it.";
/// The warning printed when the guard is off.
pub const OFF_WARNING: &str = "yusAgent: PRIME_AGENT_SECRET_GUARD=off: the secret guard is off, \
the model's code can read the credential files.";

/// Which profile a launch gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Every launch: credential files, socket paths, the running binary, `LaunchServices`.
    Full,
    /// The browser opener: the same refusals, `LaunchServices` left open (it launches the browser).
    Browser,
}

/// The paths the profile protects, canonical.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardPaths {
    /// The agent directory (`auth.json`, `models.json`).
    pub agent_dir: PathBuf,
    /// The exact socket paths in use (the supervisor's and the worker's).
    pub sockets: Vec<PathBuf>,
    /// The default socket directories (`$TMPDIR/yusagent-<uid>`, `/tmp/yusagent-<uid>`).
    pub socket_dirs: Vec<PathBuf>,
    /// The running binary (`current_exe()`).
    pub binary: PathBuf,
    /// The manifest next to it (`package.json`).
    pub manifest: PathBuf,
}

static SOCKETS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Records the socket paths this process uses (supervisor, worker), refused to every launch.
///
/// # Panics
///
/// Panics if the registry lock is poisoned.
pub fn register_sockets(paths: Vec<PathBuf>) {
    SOCKETS.lock().unwrap().extend(paths);
}

/// Whether the guard is on: unless the switch says `off`.
#[must_use]
pub fn enabled() -> bool {
    !matches!(
        std::env::var(SWITCH_ENV).as_deref().map(str::trim),
        Ok("off")
    )
}

/// The warning to print at start when the guard is off.
#[must_use]
pub fn off_warning() -> Option<&'static str> {
    (!enabled()).then_some(OFF_WARNING)
}

/// A launch of `program` under the full profile (macOS); unchanged elsewhere until PR C.
#[must_use]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    wrapped(program.as_ref(), Variant::Full, false)
}

/// A launch of the model's executor `program` (the kernel, `exec_command`): refused where the
/// guard has no mechanism.
#[must_use]
pub fn model_command(program: impl AsRef<OsStr>) -> Command {
    wrapped(program.as_ref(), Variant::Full, true)
}

/// [`command`] as a `tokio` command.
#[must_use]
pub fn tokio(program: impl AsRef<OsStr>) -> tokio::process::Command {
    tokio::process::Command::from(command(program))
}

/// [`model_command`] as a `tokio` command (the kernel, `exec_command`).
#[must_use]
pub fn model_tokio(program: impl AsRef<OsStr>) -> tokio::process::Command {
    tokio::process::Command::from(model_command(program))
}

/// The browser opener `program` for `url`, under the browser variant (macOS). A refused `url`
/// ([`browser_url_allowed`]) gets a program that does not exist: the launch fails, nothing opens.
#[must_use]
pub fn browser(program: impl AsRef<OsStr>, url: &str) -> Command {
    if !browser_url_allowed(url) {
        return Command::new(REFUSED_URL);
    }
    wrapped(program.as_ref(), Variant::Browser, false)
}

/// Prints [`OFF_WARNING`] on stderr when the guard is off (the extension copies the bridge's
/// stderr to its Output).
pub fn warn_if_off() {
    if let Some(warning) = off_warning() {
        eprintln!("{warning}");
    }
}

/// The supervisor's start: the off warning, the hard links to the credentials broken, its
/// socket registered.
pub fn supervisor_start(agent_dir: &Path, socket: &Path) {
    warn_if_off();
    break_credential_links(agent_dir);
    register_sockets(vec![socket.to_path_buf()]);
}

/// The worker's start: its socket and the supervisor's registered.
pub fn sockets(worker: &Path, supervisor: &Path) {
    register_sockets(vec![worker.to_path_buf(), supervisor.to_path_buf()]);
}

/// Whether the browser may open `url`: `https://`, or `http://` to localhost (a login callback).
#[must_use]
pub fn browser_url_allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(name, _)| name);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn refusal() -> Command {
    let mut refused = Command::new("/bin/sh");
    refused
        .arg("-c")
        .arg("printf '%s\\n' \"$0\" >&2; exit 126")
        .arg(UNAVAILABLE);
    refused
}

fn wrapped(program: &OsStr, variant: Variant, model: bool) -> Command {
    if !enabled() {
        return Command::new(program);
    }
    if !cfg!(target_os = "macos") {
        return if model {
            refusal()
        } else {
            Command::new(program)
        };
    }
    if !Path::new(SANDBOX_EXEC).exists() {
        return refusal();
    }
    let mut command = Command::new(SANDBOX_EXEC);
    command
        .arg("-p")
        .arg(profile(&current_paths(), variant))
        .arg(program);
    command
}

/// The canonical form of `path`, resolving its longest existing ancestor (Seatbelt compares
/// resolved paths: `/var` is `/private/var` on macOS).
fn canonical(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(std::ffi::OsStr::to_os_string) else {
            return path.to_path_buf();
        };
        rest.push(name);
        if !existing.pop() {
            return path.to_path_buf();
        }
    }
    let mut resolved = existing.canonicalize().unwrap_or(existing);
    for name in rest.into_iter().rev() {
        resolved.push(name);
    }
    resolved
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn uid() -> u32 {
    0
}

/// The paths of this process: agent directory, registered sockets, default socket directories,
/// running binary and its manifest.
///
/// # Panics
///
/// Panics if the socket registry lock is poisoned.
#[must_use]
pub fn current_paths() -> GuardPaths {
    let agent_dir = crate::platform::dirs::agent_dir().unwrap_or_default();
    let tmp = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let name = format!("yusagent-{}", uid());
    let binary = std::env::current_exe()
        .map(|exe| canonical(&exe))
        .unwrap_or_default();
    let manifest = binary.with_file_name("package.json");
    GuardPaths {
        agent_dir: canonical(&agent_dir),
        sockets: SOCKETS
            .lock()
            .unwrap()
            .iter()
            .map(|path| canonical(path.as_path()))
            .collect(),
        socket_dirs: vec![
            canonical(&tmp.join(&name)),
            canonical(&Path::new("/tmp").join(&name)),
        ],
        binary,
        manifest,
    }
}

/// A Seatbelt string literal.
fn quoted(path: &Path) -> String {
    let text = path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{text}\"")
}

/// A Seatbelt regex matching paths that start with `path`.
fn prefix_regex(path: &Path) -> String {
    let mut escaped = String::new();
    for ch in path.display().to_string().chars() {
        if "\\.^$|()[]{}*+?".contains(ch) {
            escaped.push('\\');
        }
        if ch == '"' {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    format!("(regex #\"^{escaped}\")")
}

/// The Seatbelt profile for `paths`.
#[must_use]
pub fn profile(paths: &GuardPaths, variant: Variant) -> String {
    let mut text = String::from("(version 1)\n(allow default)\n");
    let agent = &paths.agent_dir;
    text.push_str(&format!(
        "(deny file-read* file-write* {} {})\n",
        prefix_regex(&agent.join("auth.json")),
        prefix_regex(&agent.join("models.json"))
    ));
    let mut renames = vec![format!("(literal {})", quoted(agent))];
    if let Some(parent) = agent.parent() {
        renames.push(format!("(literal {})", quoted(parent)));
    }
    if let Some(parent) = paths.binary.parent() {
        renames.push(format!("(literal {})", quoted(parent)));
    }
    text.push_str(&format!("(deny file-write-unlink {})\n", renames.join(" ")));
    let mut remote = Vec::new();
    let mut socket_writes = Vec::new();
    for socket in &paths.sockets {
        remote.push(format!(
            "(remote unix-socket (path-literal {}))",
            quoted(socket)
        ));
        socket_writes.push(format!("(literal {})", quoted(socket)));
    }
    for dir in &paths.socket_dirs {
        remote.push(format!("(remote unix-socket (subpath {}))", quoted(dir)));
        socket_writes.push(format!("(subpath {})", quoted(dir)));
    }
    text.push_str(&format!("(deny network-outbound {})\n", remote.join(" ")));
    text.push_str(&format!(
        "(deny file-write* {} (literal {}) (literal {}))\n",
        socket_writes.join(" "),
        quoted(&paths.binary),
        quoted(&paths.manifest)
    ));
    if variant == Variant::Full {
        text.push_str(
            "(deny mach-lookup (global-name-prefix \"com.apple.coreservices.\") \
             (global-name-prefix \"com.apple.lsd.\"))\n",
        );
    }
    text.push_str(
        "(allow process-exec (literal \"/bin/ps\") (literal \"/usr/bin/top\") (with no-sandbox))\n",
    );
    text
}

/// At start: a credential file with more than one link (a hard link made before the guard) is
/// rewritten into a new inode, so the old link no longer follows it (test 7).
pub fn break_credential_links(agent_dir: &Path) {
    #[cfg(unix)]
    for name in ["auth.json", "models.json"] {
        use std::os::unix::fs::MetadataExt;
        let path = agent_dir.join(name);
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.nlink() <= 1 {
            continue;
        }
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        let temp = path.with_file_name(format!("{name}.relink{}", std::process::id()));
        if std::fs::write(&temp, &content).is_ok()
            && std::fs::set_permissions(&temp, metadata.permissions()).is_ok()
        {
            let _ = std::fs::rename(&temp, &path);
        }
        let _ = std::fs::remove_file(&temp);
    }
    #[cfg(not(unix))]
    let _ = agent_dir;
}
