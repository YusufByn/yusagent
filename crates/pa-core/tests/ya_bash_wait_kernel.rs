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
//! c. without the `bash.completed` handler, 30 seconds sets nothing (the await is unbounded).
//!
//! The kernel Python is the venv CI bootstraps before the test shards; the test fails in CI
//! when it is missing instead of skipping.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use pa_core::kernel::provisioner::{IpythonKernelProvisioner, IpythonKernelProvisionerOptions};
use pa_core::kernel::shared::{
    host_handler, ExecuteOptions, ExecuteResult, ExecuteStatus, HostRequestHandlers,
    BASH_COMPLETED_REQUEST_TYPE,
};
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
    notices: bool,
    seconds: Option<u32>,
) -> IpythonKernelProvisioner {
    let mut host_handlers = HostRequestHandlers::new();
    if notices {
        host_handlers.register(
            BASH_COMPLETED_REQUEST_TYPE,
            host_handler(|_payload| async move { Ok(json!({"status": "ok"})) }),
        );
    }
    IpythonKernelProvisioner::new(
        dir,
        IpythonKernelProvisionerOptions {
            python: Some(python),
            host_handlers,
            ya_bash_wait_seconds: seconds.and_then(NonZeroU32::new),
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

async fn probe(python: &Path, notices: bool, seconds: Option<u32>) -> String {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let kernel = provisioner(python.to_path_buf(), dir.path(), notices, seconds);
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
    assert_eq!(probe(&python, true, Some(30)).await, "BashResult 30.0");
    // c. No `bash.completed` handler: no bound, nothing set.
    assert_eq!(probe(&python, false, Some(30)).await, "BashResult 1.0");
}
