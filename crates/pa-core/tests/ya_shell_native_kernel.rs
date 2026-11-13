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

//! yusAgent (YA-03, ticket 09): a real kernel started for a `shellTool: "native"` session
//! (`IpythonKernelProvisionerOptions::ya_native_shell`):
//! a. the kernel starts and its bootstrap reports no failed import;
//! b. `bash` is not in the namespace, and `from rlm import bash` reaches a function that refuses;
//! c. `read()` and the bundled `edit` skill work without `bash`;
//! d. after the kernel is killed and restarted, `bash` is still absent;
//! e. control: in the default variant, `bash` is there.
//!
//! The kernel Python is the venv CI bootstraps before the test shards; the test fails in CI
//! when it is missing instead of skipping. The `edit` skill comes from the repository
//! (`skills/edit/src` on `PYTHONPATH`), imported by the session bootstrap as in a session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pa_core::kernel::bootstrap::ya_native_shell::NATIVE_SHELL_BASH_ERROR;
use pa_core::kernel::bootstrap::{KernelPythonSkill, UnavailablePythonSkills};
use pa_core::kernel::provisioner::{IpythonKernelProvisioner, IpythonKernelProvisionerOptions};
use pa_core::kernel::shared::{ExecuteOptions, ExecuteResult, ExecuteStatus, HostRequestHandlers};

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
        "kernel python {} not found: the native-shell kernel test must run in CI",
        candidate.display()
    );
    eprintln!(
        "kernel python {} not found; skipping the native-shell kernel test",
        candidate.display()
    );
    None
}

fn edit_skill() -> KernelPythonSkill {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/edit");
    KernelPythonSkill {
        name: "edit".to_string(),
        import_name: "edit".to_string(),
        pyproject_path: package.join("pyproject.toml"),
        package_path: package,
    }
}

struct Kernel {
    provisioner: IpythonKernelProvisioner,
    unavailable: Arc<Mutex<Vec<UnavailablePythonSkills>>>,
    dir: tempfile::TempDir,
}

fn provisioner(python: PathBuf, native_shell: bool) -> Kernel {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let unavailable: Arc<Mutex<Vec<UnavailablePythonSkills>>> = Arc::new(Mutex::new(Vec::new()));
    let on_unavailable_skills = {
        let unavailable = Arc::clone(&unavailable);
        Arc::new(move |errors: &UnavailablePythonSkills| {
            unavailable.lock().unwrap().push(errors.clone());
        }) as pa_core::kernel::provisioner::UnavailableSkillsCallback
    };
    let skill_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/edit/src");
    let env = HashMap::from([(
        "PYTHONPATH".to_string(),
        skill_src.to_string_lossy().to_string(),
    )]);
    let provisioner = IpythonKernelProvisioner::new(
        dir.path(),
        IpythonKernelProvisionerOptions {
            python: Some(python),
            env,
            host_handlers: HostRequestHandlers::new(),
            python_skills: vec![edit_skill()],
            on_unavailable_skills: Some(on_unavailable_skills),
            ya_native_shell: native_shell,
            ..Default::default()
        },
    );
    Kernel {
        provisioner,
        unavailable,
        dir,
    }
}

async fn run(kernel: &Kernel, code: &str) -> ExecuteResult {
    kernel
        .provisioner
        .ensure(None, None)
        .await
        .expect("kernel must start")
        .execute(code, ExecuteOptions::default())
        .await
        .expect("execute must not fail")
}

fn error_of(result: &ExecuteResult) -> (String, String) {
    let error = result.error.as_ref().expect("the cell must fail");
    (error.ename.clone(), error.evalue.clone())
}

async fn assert_no_bash(kernel: &Kernel) {
    let named = run(kernel, "bash").await;
    assert_eq!(named.status, ExecuteStatus::Error);
    let (ename, evalue) = error_of(&named);
    assert_eq!(ename, "NameError", "{evalue}");
    assert!(evalue.contains("'bash'"), "{evalue}");

    let imported = run(kernel, "from rlm import bash\nbash('true')").await;
    assert_eq!(imported.status, ExecuteStatus::Error);
    let (ename, evalue) = error_of(&imported);
    assert_eq!(ename, "RuntimeError");
    assert_eq!(evalue, NATIVE_SHELL_BASH_ERROR);
}

#[tokio::test]
async fn native_shell_kernel_has_no_bash_and_keeps_read_and_edit() {
    let Some(python) = kernel_python() else {
        return;
    };
    let kernel = provisioner(python, true);

    // a. The kernel starts; the bootstrap imported every skill.
    let started = run(&kernel, "1 + 1").await;
    assert_eq!(started.status, ExecuteStatus::Ok, "{}", started.stderr);
    assert!(
        kernel.unavailable.lock().unwrap().is_empty(),
        "{:?}",
        kernel.unavailable.lock().unwrap()
    );

    // b. No `bash`, by name or through the package.
    assert_no_bash(&kernel).await;

    // c. `read()` and `edit()` without `bash`.
    let file = kernel.dir.path().join("notes.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let path = file.display().to_string();
    let edited = run(
        &kernel,
        &format!(
            "await edit(path={path:?}, old_str='beta', new_str='gamma')\nprint(read({path:?}))"
        ),
    )
    .await;
    assert_eq!(edited.status, ExecuteStatus::Ok, "{}", edited.stderr);
    assert!(
        edited.stdout.contains("     1\talpha\n     2\tgamma"),
        "{}",
        edited.stdout
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma\n");

    // d. Killed and restarted: still no `bash`.
    let manager = kernel.provisioner.ensure(None, None).await.unwrap();
    let _ = manager
        .execute("import os\nos._exit(1)", ExecuteOptions::default())
        .await;
    for _ in 0..50 {
        if manager.is_defunct() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(manager.is_defunct(), "the kernel must be dead");
    drop(manager);
    let restarted = run(&kernel, "1 + 1").await;
    assert_eq!(restarted.status, ExecuteStatus::Ok, "{}", restarted.stderr);
    assert_no_bash(&kernel).await;
    let read_again = run(&kernel, &format!("print(read({path:?}, limit=1))")).await;
    assert_eq!(
        read_again.status,
        ExecuteStatus::Ok,
        "{}",
        read_again.stderr
    );
    assert!(
        read_again.stdout.contains("     1\talpha"),
        "{}",
        read_again.stdout
    );

    kernel.provisioner.dispose(None).await;
}

/// e. Control: the default variant keeps `bash`.
#[tokio::test]
async fn default_kernel_keeps_bash() {
    let Some(python) = kernel_python() else {
        return;
    };
    let kernel = provisioner(python, false);
    let result = run(
        &kernel,
        "r = await bash('echo native-check')\nprint(r.output.strip())",
    )
    .await;
    assert_eq!(result.status, ExecuteStatus::Ok, "{}", result.stderr);
    assert!(result.stdout.contains("native-check"), "{}", result.stdout);
    kernel.provisioner.dispose(None).await;
}
