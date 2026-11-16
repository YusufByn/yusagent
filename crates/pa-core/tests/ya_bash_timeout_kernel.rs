// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines).
#![allow(clippy::large_futures, clippy::too_many_lines)]
#![cfg(unix)]

//! yusAgent (YA-09): `bash(cmd, timeout=...)` in a real kernel (`rlm/ya_timeout.py`), with the
//! kernel-notice handlers of a session (`bash_notices::register_kernel_notice_host_handlers`):
//! a. the kernel starts with the new `bash` signature (`venv/probe.rs`, `RUNTIME_READY_CHECK`);
//! b. a detached command past its limit is stopped, and its `[bash-done]` notice gives the reason
//!    (`ya_bash_timeout`);
//! c. a detached command that ends first sends a plain notice.
//!
//! The kernel Python is the venv CI bootstraps before the test shards; the test fails in CI
//! when it is missing instead of skipping.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pa_core::kernel::provisioner::{IpythonKernelProvisioner, IpythonKernelProvisionerOptions};
use pa_core::kernel::shared::{ExecuteOptions, ExecuteStatus, HostRequestHandlers};
use pa_core::session_engine::bash_notices::{
    register_kernel_notice_host_handlers, BashConsumedNotice, KernelNotice,
};
use pa_types::ai::UserContent;
use serde_json::json;

fn kernel_python() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PA_CORE_KERNEL_PYTHON") {
        return Some(PathBuf::from(explicit));
    }
    let candidate = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".prime/agent/kernel-venv/bin/python");
    if candidate.exists() {
        return Some(candidate);
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "kernel python {} not found: the bash-timeout kernel test must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping the bash-timeout kernel test",
        candidate.display()
    );
    None
}

fn text(notice: &KernelNotice) -> String {
    match &notice.row.content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(_) => panic!("a text notice"),
    }
}

/// The first notice for `pid`, waiting up to 20 s.
async fn notice_for(received: &Mutex<Vec<KernelNotice>>, pid: u64) -> KernelNotice {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let found = received
            .lock()
            .unwrap()
            .iter()
            .find(|notice| {
                notice
                    .row
                    .details
                    .as_ref()
                    .and_then(|details| details["pid"].as_u64())
                    == Some(pid)
            })
            .cloned();
        if let Some(notice) = found {
            return notice;
        }
        assert!(
            Instant::now() < deadline,
            "no [bash-done] notice for pid {pid}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_command_past_its_timeout_wakes_with_the_reason() {
    let Some(python) = kernel_python() else {
        return;
    };
    let dir = tempfile::TempDir::new().expect("temp dir");
    let received: Arc<Mutex<Vec<KernelNotice>>> = Arc::default();
    let mut host_handlers = HostRequestHandlers::new();
    let sink = Arc::clone(&received);
    register_kernel_notice_host_handlers(
        Arc::new(move |notice: KernelNotice| sink.lock().unwrap().push(notice)),
        Arc::new(|_: BashConsumedNotice| {}),
        &mut host_handlers,
    );
    let kernel = IpythonKernelProvisioner::new(
        dir.path(),
        IpythonKernelProvisionerOptions {
            python: Some(python),
            host_handlers,
            ..Default::default()
        },
    );
    let manager = kernel.ensure(None, None).await.expect("kernel must start");
    let started = manager
        .execute(
            "stopped = bash('sleep 30', timeout=1)\nplain = bash('sleep 0.2', timeout=30)\nprint(stopped.pid, plain.pid)",
            ExecuteOptions::default(),
        )
        .await
        .expect("execute must not fail");
    assert_eq!(started.status, ExecuteStatus::Ok, "{}", started.stderr);
    let pids: Vec<u64> = started
        .stdout
        .split_whitespace()
        .map(|pid| pid.parse().expect("pid"))
        .collect();

    let stopped = notice_for(&received, pids[0]).await;
    let stopped_text = text(&stopped);
    assert!(stopped_text.starts_with(&format!("[bash-done pid:{} exit:", pids[0])));
    assert!(
        stopped_text.ends_with("\n\nKilled: still running after its 1 s timeout."),
        "{stopped_text}"
    );
    let details = stopped.row.details.expect("details");
    assert_eq!(details["timeoutSeconds"], json!(1.0));
    assert_ne!(details["exitCode"], json!(0));

    let plain = notice_for(&received, pids[1]).await;
    assert!(!text(&plain).contains("Killed:"), "{}", text(&plain));
    assert!(plain
        .row
        .details
        .expect("details")
        .get("timeoutSeconds")
        .is_none());

    kernel.dispose(None).await;
}
