// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines/casts).
#![allow(
    clippy::large_futures,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#![cfg(unix)]

//! yusAgent (YA-09, lever `bash()`): a real kernel started with `bashWaitSeconds`
//! (`IpythonKernelProvisionerOptions::ya_bash_wait_seconds`, `bootstrap::ya_bash_wait`), with the
//! `bash.completed` handler registered as in a session (the bounded await is on):
//! a. default (`None`): a 3-second command returns a snapshot after the upstream window (1 s);
//! b. 30 seconds: the same command returns its result, and the runtime global reads 30;
//! c. without the `bash.completed` handler, 30 seconds sets nothing (the await is unbounded);
//! d. `"full"` (ticket 13): the runtime global reads `None`, the command returns its result and
//!    sends no `bash.completed` notice, while a command started without `await` still does.
//!
//! The kernel Python is the venv CI bootstraps before the test shards; the test fails in CI
//! when it is missing instead of skipping.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use pa_core::kernel::provisioner::{IpythonKernelProvisioner, IpythonKernelProvisionerOptions};
use pa_core::kernel::shared::{
    host_handler, ExecuteOptions, ExecuteResult, ExecuteStatus, HostRequestHandlers,
    BASH_COMPLETED_REQUEST_TYPE,
};
use pa_core::session_engine::ya_request::BashWait;
use serde_json::json;

fn kernel_python() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PA_CORE_KERNEL_PYTHON") {
        return Some(PathBuf::from(explicit));
    }
    let candidate = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".yusagent/kernel-venv/bin/python");
    if candidate.exists() {
        return Some(candidate);
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "kernel python {} not found: the bash-wait kernel test must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping the bash-wait kernel test",
        candidate.display()
    );
    None
}

fn provisioner(
    python: PathBuf,
    dir: &Path,
    notices: Option<Arc<AtomicUsize>>,
    wait: Option<BashWait>,
) -> IpythonKernelProvisioner {
    let mut host_handlers = HostRequestHandlers::new();
    if let Some(count) = notices {
        host_handlers.register(
            BASH_COMPLETED_REQUEST_TYPE,
            host_handler(move |_payload| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"status": "ok"}))
                }
            }),
        );
    }
    IpythonKernelProvisioner::new(
        dir,
        IpythonKernelProvisionerOptions {
            python: Some(python),
            host_handlers,
            ya_bash_wait_seconds: wait,
            ..Default::default()
        },
    )
}

async fn run(kernel: &IpythonKernelProvisioner, code: &str) -> ExecuteResult {
    kernel
        .ensure(None, None)
        .await
        .expect("kernel must start")
        .execute(code, ExecuteOptions::default())
        .await
        .expect("execute must not fail")
}

/// The type of what `await bash(...)` returned for a 3-second command, and the window global.
const PROBE: &str = "import sys\nr = await bash('sleep 3; echo done')\nprint(type(r).__name__, sys.modules['rlm.bash']._YIELD_AFTER_SECONDS)";

async fn probe(python: &Path, notices: bool, wait: Option<BashWait>) -> String {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let notices = notices.then(|| Arc::new(AtomicUsize::new(0)));
    let kernel = provisioner(python.to_path_buf(), dir.path(), notices, wait);
    let result = run(&kernel, PROBE).await;
    assert_eq!(result.status, ExecuteStatus::Ok, "{}", result.stderr);
    kernel.dispose(None).await;
    result.stdout.trim().to_string()
}

#[tokio::test]
async fn bash_wait_sets_the_await_window_of_the_kernel() {
    let Some(python) = kernel_python() else {
        return;
    };
    // a. Upstream window: the 3-second command is still running after 1 s.
    assert_eq!(probe(&python, true, None).await, "BashSnapshot 1.0");
    // b. 30 seconds: the await returns the result.
    assert_eq!(
        probe(&python, true, BashWait::seconds(30)).await,
        "BashResult 30.0"
    );
    // c. No `bash.completed` handler: no bound, nothing set.
    assert_eq!(
        probe(&python, false, BashWait::seconds(30)).await,
        "BashResult 1.0"
    );
    // d. `"full"`: no bound.
    assert_eq!(
        probe(&python, true, Some(BashWait::Full)).await,
        "BashResult None"
    );
}

/// Ticket 13: in `"full"`, a command awaited to its end by its cell sends no `bash.completed`
/// notice (no `[bash-done]` wake); a command started without `await` still sends one.
#[tokio::test]
async fn full_wait_sends_no_notice_for_an_awaited_command() {
    let Some(python) = kernel_python() else {
        return;
    };
    let dir = tempfile::TempDir::new().expect("temp dir");
    let count = Arc::new(AtomicUsize::new(0));
    let kernel = provisioner(
        python.clone(),
        dir.path(),
        Some(count.clone()),
        Some(BashWait::Full),
    );
    let awaited = run(
        &kernel,
        "r = await bash('sleep 3; echo done')\nprint(r.exit_code, r.output.strip())",
    )
    .await;
    assert_eq!(awaited.status, ExecuteStatus::Ok, "{}", awaited.stderr);
    assert_eq!(awaited.stdout.trim(), "0 done");
    // A later cell gives a late notice the time to arrive.
    let later = run(&kernel, "import asyncio\nawait asyncio.sleep(1)").await;
    assert_eq!(later.status, ExecuteStatus::Ok, "{}", later.stderr);
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "no notice for an awaited command"
    );
    // Control: a command left running by its cell notifies on completion.
    let started = run(&kernel, "h = bash('sleep 0.5')").await;
    assert_eq!(started.status, ExecuteStatus::Ok, "{}", started.stderr);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while count.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a background command still notifies"
    );
    kernel.dispose(None).await;
}
