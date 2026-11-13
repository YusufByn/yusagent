//! yusAgent (YA-03, ticket 09): kernel crash during a `python_repl` cell.
//!
//! Upstream restarts a dead kernel lazily, on the next cell, reviving the namespace from the
//! snapshot taken after successful cells (`kernel::provisioner`, `restore_state`), and tells the
//! model only on the next user turn, as if a previous session were resumed
//! (`state_restore_notice`, "revived from your previous session").
//!
//! Here, when the kernel is found dead after a cell, it is restarted at once (same restore) and
//! the result of that cell ends with a generic notice: kernel restarted, state restored from the
//! last successful cell (or started fresh when there was nothing to restore), this cell not
//! re-run. No cell is ever replayed. The upstream next-turn notice of that restart is dropped,
//! so the model is not told twice. Decision of 13/11/2026 (CDC journal), replacing "variables
//! lost" of 28/10.

use std::sync::{Arc, Mutex, PoisonError};

use pa_types::session::CustomMessage;

use crate::kernel::provisioner::IpythonKernelProvisioner;
use crate::tools::tool_definition::{ToolContentBlock, ToolDefinition, ToolExecutionResult};

use super::state_restore_notice::IPYTHON_STATE_RESTORED_CUSTOM_TYPE;

/// The notice when the namespace was revived from the snapshot.
pub const NOTICE_RESTORED: &str = "[python-kernel-restarted]\nThe Python kernel crashed during this cell and was restarted. Its state was restored from the last successful cell: anything this cell changed in memory is lost. The working directory and environment variables were reset to the session defaults. This cell was not re-run.";

/// The notice when there was no snapshot to revive.
pub const NOTICE_FRESH: &str = "[python-kernel-restarted]\nThe Python kernel crashed during this cell and was restarted with a fresh state: variables, imports, and loaded data are gone. The working directory and environment variables were reset to the session defaults. This cell was not re-run.";

/// The notice when the restart itself failed.
#[must_use]
pub fn notice_restart_failed(error: &anyhow::Error) -> String {
    format!(
        "[python-kernel-crashed]\nThe Python kernel crashed during this cell and could not be restarted ({error:#}); the next cell starts a new kernel. This cell was not re-run."
    )
}

/// Whether the provisioner's kernel died: its manager is still the published one but defunct
/// (a kill or a restart replaces or clears it).
fn kernel_died(provisioner: &IpythonKernelProvisioner) -> bool {
    provisioner
        .manager()
        .is_some_and(|manager| manager.is_defunct())
}

/// Whether the kernel is dead after a cell. A harness-level failure (`failed`) waits a little
/// for the exit watcher, which marks the manager defunct before it rejects the running cell.
async fn died_after(provisioner: &IpythonKernelProvisioner, failed: bool) -> bool {
    if kernel_died(provisioner) {
        return true;
    }
    if !failed {
        return false;
    }
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if kernel_died(provisioner) {
            return true;
        }
    }
    false
}

/// Restart the kernel now and return the notice. The restore notice the restart queued for the
/// next user turn is removed; its `restored` flag picks the notice.
async fn restart(
    provisioner: &IpythonKernelProvisioner,
    next_turn_rows: &Mutex<Vec<CustomMessage>>,
) -> String {
    if let Err(error) = provisioner.ensure(None, None).await {
        return notice_restart_failed(&error);
    }
    let mut restored = false;
    next_turn_rows
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|row| {
            if row.custom_type != IPYTHON_STATE_RESTORED_CUSTOM_TYPE {
                return true;
            }
            restored |= row
                .details
                .as_ref()
                .and_then(|details| details.get("restored"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            false
        });
    if restored {
        NOTICE_RESTORED.to_string()
    } else {
        NOTICE_FRESH.to_string()
    }
}

/// `result` with `notice` at its end: after the text of a result, after the message of an error.
fn with_notice(
    result: anyhow::Result<ToolExecutionResult>,
    notice: &str,
) -> anyhow::Result<ToolExecutionResult> {
    match result {
        Err(error) => Err(anyhow::anyhow!("{error}\n\n{notice}")),
        Ok(mut result) => {
            match result.content.first_mut() {
                Some(ToolContentBlock::Text { text }) if !text.is_empty() => {
                    text.push_str("\n\n");
                    text.push_str(notice);
                }
                Some(ToolContentBlock::Text { text }) => text.push_str(notice),
                _ => result.content.insert(0, ToolContentBlock::text(notice)),
            }
            Ok(result)
        }
    }
}

/// Wrap the `python_repl` executor: a cell after which the kernel is dead gets the restart and
/// the notice. `next_turn_rows` is the session's next-turn mailbox the provisioner's restore
/// callback writes to.
pub fn wrap_execute(
    definition: &mut ToolDefinition,
    provisioner: Arc<IpythonKernelProvisioner>,
    next_turn_rows: Arc<Mutex<Vec<CustomMessage>>>,
) {
    let inner = Arc::clone(&definition.execute);
    definition.execute = Arc::new(move |tool_call_id: &str, params, signal, on_update| {
        let inner = Arc::clone(&inner);
        let provisioner = Arc::clone(&provisioner);
        let next_turn_rows = Arc::clone(&next_turn_rows);
        let tool_call_id = tool_call_id.to_string();
        Box::pin(async move {
            let result = (*inner)(&tool_call_id, params, signal, on_update).await;
            if !died_after(&provisioner, result.is_err()).await {
                return result;
            }
            let notice = restart(&provisioner, &next_turn_rows).await;
            with_notice(result, &notice)
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_goes_after_the_text_or_the_error() {
        let ok = with_notice(Ok(ToolExecutionResult::text("out")), "N").unwrap();
        assert_eq!(ok.content[0].as_text(), Some("out\n\nN"));
        let empty = with_notice(Ok(ToolExecutionResult::text("")), "N").unwrap();
        assert_eq!(empty.content[0].as_text(), Some("N"));
        let error = with_notice(Err(anyhow::anyhow!("Kernel has been shut down")), "N");
        assert_eq!(
            error.unwrap_err().to_string(),
            "Kernel has been shut down\n\nN"
        );
    }

    /// The kernel Python with prime-agent-runtime installed, as in `tests/kernel_stop_revive.rs`;
    /// the live test is skipped when absent.
    fn kernel_python() -> Option<std::path::PathBuf> {
        if let Some(explicit) = std::env::var_os("PA_CORE_KERNEL_PYTHON") {
            return Some(std::path::PathBuf::from(explicit));
        }
        let home = std::env::var("HOME").ok()?;
        let candidate = std::path::PathBuf::from(home).join(".prime/agent/kernel-venv/bin/python");
        if candidate.exists() {
            return Some(candidate);
        }
        eprintln!("kernel python not found; skipping the live kernel crash test");
        None
    }

    /// A cell that kills its kernel: the kernel is restarted at once with the namespace of the
    /// last successful cell, the result ends with the notice, the cell is not re-run, and the
    /// upstream next-turn restore notice is dropped.
    #[tokio::test]
    async fn a_cell_that_kills_the_kernel_gets_the_notice_and_is_not_re_run() {
        use crate::kernel::provisioner::IpythonKernelProvisionerOptions;
        let Some(python) = kernel_python() else {
            return;
        };
        let dir = tempfile::TempDir::new().unwrap();
        let artifacts = dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let rows: Arc<Mutex<Vec<CustomMessage>>> = Arc::new(Mutex::new(Vec::new()));
        let on_restore = {
            let rows = Arc::clone(&rows);
            Arc::new(
                move |result: &crate::kernel::state_snapshot::RestoreResult| {
                    rows.lock()
                        .unwrap()
                        .push(super::super::state_restore_notice::notice_message(result));
                },
            ) as crate::kernel::provisioner::RestoreCallback
        };
        let provisioner = Arc::new(IpythonKernelProvisioner::new(
            dir.path(),
            IpythonKernelProvisionerOptions {
                python: Some(python),
                snapshot_dir: Some(artifacts.clone()),
                on_restore: Some(on_restore),
                ..Default::default()
            },
        ));
        let mut definition = crate::tools::ipython::create_ipython_tool_definition(
            &dir.path().to_string_lossy(),
            super::super::runtime_wiring::ipython_tool_options(Arc::clone(&provisioner), None),
        );
        wrap_execute(&mut definition, Arc::clone(&provisioner), Arc::clone(&rows));
        let run = |code: &str| {
            (*definition.execute)("call", serde_json::json!({ "code": code }), None, None)
        };

        let first = run("marker = 2483").await.unwrap();
        assert!(!first.is_error);
        // The debounced snapshot of the successful cell lands on disk.
        let snapshot = artifacts.join("kernel-state.dill");
        for _ in 0..100 {
            if snapshot.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(snapshot.exists(), "no snapshot after the successful cell");

        let counter = dir.path().join("runs.txt");
        let crash = format!(
            "import os, pathlib\np = pathlib.Path({:?})\np.write_text(str(int(p.read_text()) + 1) if p.exists() else '1')\nos.kill(os.getpid(), 9)",
            counter.display().to_string()
        );
        let started = std::time::Instant::now();
        let crashed = run(&crash).await;
        let elapsed = started.elapsed();
        let text = match &crashed {
            Ok(result) => result.content[0].as_text().unwrap_or_default().to_string(),
            Err(error) => error.to_string(),
        };
        assert!(text.ends_with(NOTICE_RESTORED), "{text}");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "crash and restart took {elapsed:?}"
        );
        assert!(
            provisioner.has_running_kernel(),
            "the kernel is restarted at once"
        );
        assert_eq!(
            std::fs::read_to_string(&counter).unwrap(),
            "1",
            "the cell ran once"
        );
        assert!(
            rows.lock()
                .unwrap()
                .iter()
                .all(|row| row.custom_type != IPYTHON_STATE_RESTORED_CUSTOM_TYPE),
            "the upstream next-turn restore notice is dropped"
        );

        let after = run("marker").await.unwrap();
        assert_eq!(after.content[0].as_text(), Some("2483"));
        assert_eq!(
            std::fs::read_to_string(counter).unwrap(),
            "1",
            "never re-run"
        );
    }

    #[test]
    fn notices_say_the_cell_was_not_re_run_and_list_no_names() {
        for notice in [
            NOTICE_RESTORED.to_string(),
            NOTICE_FRESH.to_string(),
            notice_restart_failed(&anyhow::anyhow!("boom")),
        ] {
            assert!(notice.ends_with("This cell was not re-run."), "{notice}");
        }
    }
}
