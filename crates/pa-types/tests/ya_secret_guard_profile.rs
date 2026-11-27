//! yusAgent (YA-11, ticket 41, PR A, test 1): the text of the guard's Seatbelt profile.
//! - credential files: `auth.json*` and `models.json*` by prefix regex (temp and lock files
//!   included), dots and other regex characters escaped;
//! - renames: `file-write-unlink` on the agent directory and its parent;
//! - sockets: the exact paths in use and the default directories, for connect and creation;
//! - the running binary and its `package.json`, and their directory;
//! - `LaunchServices` refused in the full profile, left open in the browser variant;
//! - `/bin/ps` and `/usr/bin/top` (setuid) allowed outside the sandbox, nothing else.
//!
//! Text only: runs on every platform. Listed in `docs/fork-changes.md`.

use std::path::PathBuf;

use pa_types::platform::ya_guard::{profile, GuardPaths, Variant};

fn paths() -> GuardPaths {
    GuardPaths {
        agent_dir: PathBuf::from("/Users/u/.yus+agent"),
        sockets: vec![
            PathBuf::from("/Users/u/custom/daemon.sock"),
            PathBuf::from("/private/var/folders/a/T/yusagent-501/worker-0a-1b.sock"),
        ],
        socket_dirs: vec![
            PathBuf::from("/private/var/folders/a/T/yusagent-501"),
            PathBuf::from("/private/tmp/yusagent-501"),
        ],
        binary: PathBuf::from("/Users/u/bin/yusagent"),
        manifest: PathBuf::from("/Users/u/bin/package.json"),
    }
}

fn assert_has(profile: &str, part: &str) {
    assert!(
        profile.contains(part),
        "missing {part:?} in the profile:\n{profile}"
    );
}

#[test]
fn the_full_profile_refuses_the_credentials_the_sockets_the_binary_and_launchservices() {
    let full = profile(&paths(), Variant::Full);
    assert!(
        full.starts_with("(version 1)"),
        "not a Seatbelt profile:\n{full}"
    );
    assert_has(&full, "(allow default)");
    assert_has(&full, r#"(regex #"^/Users/u/\.yus\+agent/auth\.json")"#);
    assert_has(&full, r#"(regex #"^/Users/u/\.yus\+agent/models\.json")"#);
    assert_has(&full, "(deny file-read* file-write*");
    assert_has(&full, "(deny file-write-unlink");
    assert_has(&full, r#"(literal "/Users/u/.yus+agent")"#);
    assert_has(&full, r#"(literal "/Users/u")"#);
    assert_has(&full, "(deny network-outbound");
    for socket in &paths().sockets {
        assert_has(&full, &format!(r#"(path-literal "{}")"#, socket.display()));
    }
    for dir in &paths().socket_dirs {
        assert_has(&full, &format!(r#"(subpath "{}")"#, dir.display()));
    }
    assert_has(&full, r#"(literal "/Users/u/bin/yusagent")"#);
    assert_has(&full, r#"(literal "/Users/u/bin/package.json")"#);
    assert_has(&full, r#"(literal "/Users/u/bin")"#);
    assert_has(&full, r#"(global-name-prefix "com.apple.coreservices.")"#);
    assert_has(&full, r#"(global-name-prefix "com.apple.lsd.")"#);
    assert_eq!(
        full.matches("with no-sandbox").count(),
        1,
        "one setuid exception only:\n{full}"
    );
    assert_has(&full, r#"(literal "/bin/ps")"#);
    assert_has(&full, r#"(literal "/usr/bin/top")"#);
}

#[test]
fn the_browser_variant_keeps_the_refusals_but_leaves_launchservices_open() {
    let browser = profile(&paths(), Variant::Browser);
    assert!(
        browser.starts_with("(version 1)"),
        "not a Seatbelt profile:\n{browser}"
    );
    assert_has(&browser, r#"(regex #"^/Users/u/\.yus\+agent/auth\.json")"#);
    assert_has(&browser, r#"(literal "/Users/u/bin/yusagent")"#);
    assert!(
        !browser.contains("com.apple.coreservices."),
        "the browser variant must leave LaunchServices open:\n{browser}"
    );
}
