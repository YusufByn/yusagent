//! yusAgent (YA-11, ticket 41, PR A): the secret guard. Every process yusAgent launches runs
//! under a Seatbelt profile (macOS) that refuses the credential files, the daemon's sockets, the
//! running binary and `LaunchServices`; the rest of the system stays open.
//!
//! Push 1 skeleton: [`profile`] returns an empty profile; the profile, the wrapped commands and
//! the switch (`PRIME_AGENT_SECRET_GUARD=off`) come with push 2. Listed in
//! `docs/fork-changes.md`.

use std::path::PathBuf;
use std::sync::Mutex;

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

/// The Seatbelt profile for `paths` (push 1 skeleton: empty).
#[must_use]
pub fn profile(paths: &GuardPaths, variant: Variant) -> String {
    let _ = (paths, variant);
    String::new()
}
