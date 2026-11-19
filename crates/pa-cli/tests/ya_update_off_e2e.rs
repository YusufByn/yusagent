//! yusAgent (YA-16, ticket 19): `update` refuses before any download, whatever its flags.

use std::process::Command;

#[test]
fn update_is_refused_before_any_download() {
    let dir = tempfile::tempdir().expect("temp dir");
    for args in [
        &["update"][..],
        &["update", "--check"],
        &["update", "--nightly"],
        &["update", "--rollback"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_prime-agent"))
            .args(args)
            .env("PRIME_AGENT_CODING_AGENT_DIR", dir.path().join("agent"))
            .env("HOME", dir.path())
            // An installer fetch would hit this unreachable origin and fail differently.
            .env(
                "PRIME_AGENT_RUST_INSTALLER_URL",
                "http://127.0.0.1:9/install.sh",
            )
            .env("PRIME_AGENT_DOWNLOAD_BASE_URL", "http://127.0.0.1:9")
            .current_dir(dir.path())
            .output()
            .expect("spawn the binary");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stderr}");
        assert_eq!(
            stderr.trim_end(),
            "Error: Self-update is disabled in yusAgent. Install a newer yusAgent build to update.",
            "{args:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
