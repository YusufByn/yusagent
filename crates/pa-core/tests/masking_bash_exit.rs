//! yusAgent (T05, YA-05): a cell that reads a failed `bash()` result reports its exit code
//! (`bash_failed_exit`, `bashFailedExit` on the tool-result details), which the masking guard
//! treats as an error. Real `python -m rlm.repl` kernel, like `kernel_lifecycle.rs`: skipped
//! with a note when no kernel venv is found, required in CI.
#![cfg(unix)]
// Pedantic-gate disposition as `kernel_lifecycle.rs`.
#![allow(clippy::large_futures)]

use std::collections::HashMap;
use std::path::PathBuf;

use pa_core::kernel::bootstrap::build_rlm_bootstrap_code;
use pa_core::kernel::manager::{KernelStartOptions, ReplKernelManager};
use pa_core::kernel::shared::{
    ExecuteOptions, ExecuteStatus, HostRequestHandlers, KernelManagerOptions, KernelShutdownOptions,
};

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
        "kernel python {} not found: the bash exit test must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping live kernel test",
        candidate.display()
    );
    None
}

async fn started_manager() -> Option<ReplKernelManager> {
    let bootstrap = build_rlm_bootstrap_code(&[], false);
    let manager = ReplKernelManager::new(KernelManagerOptions {
        python: Some(kernel_python()?),
        cwd: Some(std::env::temp_dir()),
        env: HashMap::new(),
        session_id: Some("masking-bash-exit".to_string()),
        host_handlers: HostRequestHandlers::new(),
        python_skills: Vec::new(),
        on_background_work_settled: None,
        snapshot: None,
        bootstrap_code: Some(bootstrap.clone()),
        stderr_log_path: None,
    });
    manager
        .start(KernelStartOptions::default())
        .await
        .expect("kernel must start");
    let result = manager
        .execute(&bootstrap, ExecuteOptions::default())
        .await
        .expect("bootstrap execute must not fail");
    assert_eq!(result.status, ExecuteStatus::Ok, "{}", result.stderr);
    Some(manager)
}

async fn failed_exit(manager: &ReplKernelManager, code: &str) -> Option<i64> {
    let result = manager
        .execute(code, ExecuteOptions::default())
        .await
        .expect("execute must not fail");
    assert_eq!(result.status, ExecuteStatus::Ok, "{}", result.stderr);
    result.bash_failed_exit
}

#[tokio::test]
async fn a_cell_that_reads_a_failed_bash_result_reports_its_exit_code() {
    let Some(manager) = started_manager().await else {
        return;
    };
    failed_exit(&manager, "from rlm import bash").await;
    assert_eq!(
        failed_exit(&manager, "r = await bash('exit 3')\nprint(r.exit_code)").await,
        Some(3)
    );
    assert_eq!(failed_exit(&manager, "await bash('true')").await, None);
    // The last failed read of the cell wins.
    assert_eq!(
        failed_exit(
            &manager,
            "await bash('exit 1')\nawait bash('true')\nawait bash('exit 7')"
        )
        .await,
        Some(7)
    );
    // Only the cell that reads the result is tagged.
    assert_eq!(failed_exit(&manager, "print(1)").await, None);
    manager
        .shutdown(KernelShutdownOptions::default())
        .await
        .expect("shutdown");
}
