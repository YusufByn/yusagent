#![cfg(unix)]

//! yusAgent (YA-11, ticket 41, PR A, condition 1 of §S.3): the browser opener launches only
//! `https://` URLs, and `http://` to localhost (a login callback). Any other scheme, file path or
//! application is refused: otherwise the kernel could make the worker open, outside the guard,
//! an application it wrote (`mcp.begin_login`, `crates/pa-daemon/src/mcp_login.rs:49`).
//!
//! A fake `open` / `xdg-open` first on `PATH` records each launch. One test function: `PATH` is
//! process-wide.

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use pa_core::platform::browser::open_in_browser;

fn launches(log: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn wait_for(log: &std::path::Path, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while launches(log).len() < count && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_browser_opens_only_https_and_localhost_http() {
    let dir = tempfile::TempDir::new().unwrap();
    let log = dir.path().join("launches.log");
    for name in ["open", "xdg-open"] {
        let fake = dir.path().join(name);
        std::fs::write(
            &fake,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", dir.path().display()));

    let refused = [
        "file:///tmp/ya41/Esc.app",
        "/tmp/ya41/Esc.app",
        "ya41/Esc.app",
        "-a Calculator",
        "x-apple.systempreferences:com.apple.preference.security",
        "http://example.com/login",
        "ftp://example.com/",
    ];
    for target in refused {
        open_in_browser(target);
    }
    let allowed = [
        "https://example.com/authorize?state=ya41",
        "http://localhost:1455/auth/callback",
        "http://127.0.0.1:53692/callback",
    ];
    for target in allowed {
        open_in_browser(target);
    }
    wait_for(&log, allowed.len());
    std::thread::sleep(Duration::from_millis(300));
    std::env::set_var("PATH", path);

    let seen = launches(&log);
    let wrongly_opened: Vec<&String> = seen
        .iter()
        .filter(|line| refused.iter().any(|target| line.contains(target)))
        .collect();
    assert!(
        wrongly_opened.is_empty(),
        "the opener launched what it must refuse: {wrongly_opened:?}"
    );
    for target in allowed {
        assert!(
            seen.iter().any(|line| line.contains(target)),
            "the opener must still launch {target}: {seen:?}"
        );
    }
}
