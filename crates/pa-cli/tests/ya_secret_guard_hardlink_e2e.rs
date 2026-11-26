#![cfg(unix)]
//! yusAgent (YA-11, ticket 41, PR A, test 7): a hard link to `auth.json` made before the guard
//! (by an older, unguarded kernel) would keep reading the credentials. At start, the daemon
//! rewrites `auth.json` into a new inode when it has more than one link, so the old link no
//! longer follows it.
//!
//! The real binary in daemon mode on its own socket and home; no kernel, no model. Listed in
//! `docs/fork-changes.md`.

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

#[test]
fn the_daemon_breaks_a_hard_link_to_the_credentials_at_start() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let agent = home.join(".yusagent");
    std::fs::create_dir_all(&agent).unwrap();
    let auth = agent.join("auth.json");
    std::fs::write(
        &auth,
        r#"{"serper":{"type":"api_key","key":"ya41-sentinel-link-3b7f"}}"#,
    )
    .unwrap();
    let link = home.join("old-link");
    std::fs::hard_link(&auth, &link).unwrap();
    assert_eq!(std::fs::metadata(&auth).unwrap().nlink(), 2);

    let socket = home.join("daemon.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
        .args(["--mode", "daemon", "--daemon-socket"])
        .arg(&socket)
        .env("HOME", &home)
        .env_remove("PRIME_AGENT_CODING_AGENT_DIR")
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("binary present");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !socket.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(socket.exists(), "the daemon did not start");
    // The start-up check runs before the socket listens; a short grace for slow runners.
    std::thread::sleep(Duration::from_millis(500));
    let auth_meta = std::fs::metadata(&auth).unwrap();
    let link_meta = std::fs::metadata(&link).unwrap();

    if let Ok(mut stream) = pa_types::platform::transport::connect_blocking(&socket) {
        let frame = json!({
            "type": "command", "id": "shutdown-test",
            "protocol": { "name": "prime-agent.daemon", "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION },
            "command": { "type": "shutdown" },
        });
        let _ = writeln!(stream, "{frame}");
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(300));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();

    assert_ne!(
        auth_meta.ino(),
        link_meta.ino(),
        "auth.json still shares its inode with a link made before the guard"
    );
    assert_eq!(
        auth_meta.nlink(),
        1,
        "auth.json has a single link after the daemon's start"
    );
    assert!(
        std::fs::read_to_string(&auth)
            .unwrap()
            .contains("ya41-sentinel-link-3b7f"),
        "the rewrite keeps the credentials"
    );
}
