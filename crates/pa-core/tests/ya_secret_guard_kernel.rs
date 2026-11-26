// Pedantic-gate dispositions as src/lib.rs (large_futures/too_many_lines).
#![allow(clippy::large_futures, clippy::too_many_lines)]
#![cfg(target_os = "macos")]

//! yusAgent (YA-11, ticket 41, PR A): the kernel and everything it launches cannot read or write
//! the credential files, nor reach the daemon's socket directory, nor write the running binary
//! (test 2 of the ticket, and condition 2 of §S.3).
//!
//! A real kernel (`IpythonKernelProvisioner`, the CI kernel venv) runs one cell that tries each
//! access and prints `denied` or `ok` for each. With the guard, every access is `denied`; the
//! test process (standing for the worker) still reads `auth.json`. The same cell with the guard
//! switched off (`PRIME_AGENT_SECRET_GUARD=off`) reads the file again (positive control).
//!
//! One test function: the switch is a process environment variable, read at each kernel spawn.
//! The guard is forced on explicitly (`.cargo/config.toml` switches it off for the other tests).

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use pa_core::kernel::provisioner::{IpythonKernelProvisioner, IpythonKernelProvisionerOptions};
use pa_core::kernel::shared::{ExecuteOptions, ExecuteStatus};
use serde_json::Value;

const SENTINEL: &str = "ya41-sentinel-auth-6d2e";
const SWITCH: &str = "PRIME_AGENT_SECRET_GUARD";

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
        "kernel python {} not found: the guard kernel test must run in CI",
        candidate.display()
    );
    eprintln!("kernel python not found; skipping the guard kernel test");
    None
}

fn uid() -> String {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// One cell that tries every access and prints a JSON map `check -> "denied" | "ok" | <error>`.
fn cell(agent: &Path, project: &Path, socket: &Path, outside: &Path, exe: &Path) -> String {
    let auth = agent.join("auth.json");
    let models = agent.join("models.json");
    let temp = agent.join("auth.json.tmp123");
    let link = project.join("auth-link");
    let package = exe.with_file_name("package.json");
    let moved = agent.with_file_name("agent-moved");
    format!(
        r#"import json, os, socket
results = {{}}
def check(name, action):
    try:
        action()
        results[name] = "ok"
    except PermissionError:
        results[name] = "denied"
    except Exception as error:
        results[name] = type(error).__name__
check("read auth.json", lambda: open({auth:?}).read())
check("read models.json", lambda: open({models:?}).read())
check("read auth.json.tmp123", lambda: open({temp:?}).read())
def via_link():
    os.symlink({auth:?}, {link:?})
    open({link:?}).read()
check("read through a symlink", via_link)
def replace_models():
    with open({project:?} + "/m.tmp", "w") as handle:
        handle.write("{{}}")
    os.replace({project:?} + "/m.tmp", {models:?})
check("replace models.json", replace_models)
def connect():
    client = socket.socket(socket.AF_UNIX)
    client.connect({socket:?})
    client.close()
check("connect to the daemon socket dir", connect)
def connect_outside():
    client = socket.socket(socket.AF_UNIX)
    client.connect({outside:?})
    client.close()
check("connect to a daemon socket outside the default dir", connect_outside)
check("write the running binary", lambda: open({exe:?}, "ab").close())
check("write package.json next to it", lambda: open({package:?}, "ab").close())
shell = await bash("cat " + {auth:?})
results["bash cat auth.json"] = "ok" if shell.exit_code == 0 else "denied"
check("rename the agent dir", lambda: os.rename({agent:?}, {moved:?}))
print("YA41=" + json.dumps(results))"#,
        auth = auth.display().to_string(),
        models = models.display().to_string(),
        temp = temp.display().to_string(),
        link = link.display().to_string(),
        project = project.display().to_string(),
        socket = socket.display().to_string(),
        outside = outside.display().to_string(),
        exe = exe.display().to_string(),
        package = package.display().to_string(),
        moved = moved.display().to_string(),
    )
}

async fn run_cell(python: &Path, project: &Path, agent: &Path, code: &str) -> Value {
    let kernel = IpythonKernelProvisioner::new(
        project,
        IpythonKernelProvisionerOptions {
            python: Some(python.to_path_buf()),
            env: std::collections::HashMap::from([(
                "PRIME_AGENT_CODING_AGENT_DIR".to_string(),
                agent.display().to_string(),
            )]),
            ..Default::default()
        },
    );
    let manager = kernel.ensure(None, None).await.expect("kernel must start");
    let result = manager
        .execute(code, ExecuteOptions::default())
        .await
        .expect("execute must not fail");
    kernel.dispose(None).await;
    assert_eq!(result.status, ExecuteStatus::Ok, "{}", result.stderr);
    let line = result
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("YA41="))
        .unwrap_or_else(|| panic!("no result line: {}", result.stdout));
    serde_json::from_str(line).unwrap()
}

#[tokio::test]
async fn the_kernel_cannot_reach_the_credentials_the_socket_or_the_binary() {
    let Some(python) = kernel_python() else {
        return;
    };
    let root = tempfile::TempDir::new().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let agent = root_path.join("agent");
    let project = root_path.join("project");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        agent.join("auth.json"),
        format!(r#"{{"serper":{{"type":"api_key","key":"{SENTINEL}"}}}}"#),
    )
    .unwrap();
    std::fs::write(
        agent.join("models.json"),
        format!(r#"{{"providers":{{"x":{{"apiKey":"{SENTINEL}"}}}}}}"#),
    )
    .unwrap();
    // The daemon's atomic write leaves a temp file for a moment (`settings/storage.rs:291`).
    std::fs::write(agent.join("auth.json.tmp123"), SENTINEL).unwrap();

    // A socket in the daemon's socket directory (`$TMPDIR/yusagent-<uid>`).
    let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let socket_dir = PathBuf::from(tmp).join(format!("yusagent-{}", uid()));
    std::fs::create_dir_all(&socket_dir).unwrap();
    let socket_dir = socket_dir.canonicalize().unwrap();
    let socket = socket_dir.join(format!("ya41-guard-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let _listener = UnixListener::bind(&socket).unwrap();
    // A daemon started with `--daemon-socket` elsewhere: its exact path is registered by the
    // worker at start (`register_sockets`), the test stands for it.
    let outside = root_path.join("custom").join("daemon.sock");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    let _outside_listener = UnixListener::bind(&outside).unwrap();
    pa_types::platform::ya_guard::register_sockets(vec![outside.clone()]);

    let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
    let package = exe.with_file_name("package.json");
    let package_existed = package.exists();
    let code = cell(&agent, &project, &socket, &outside, &exe);

    // The guard reads the agent dir of the launching process (the worker's in the product).
    std::env::set_var("PRIME_AGENT_CODING_AGENT_DIR", &agent);
    // Guard on (forced: the other tests switch it off through `.cargo/config.toml`).
    std::env::remove_var(SWITCH);
    let guarded = run_cell(&python, &project, &agent, &code).await;
    if !package_existed {
        let _ = std::fs::remove_file(&package);
    }
    let moved = agent.with_file_name("agent-moved");
    if moved.exists() {
        std::fs::rename(&moved, &agent).unwrap();
    }
    let _ = std::fs::remove_file(project.join("auth-link"));
    let not_denied: Vec<String> = guarded
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, outcome)| outcome.as_str() != Some("denied"))
        .map(|(check, outcome)| format!("{check}: {outcome}"))
        .collect();
    assert!(
        not_denied.is_empty(),
        "the kernel reached what the guard refuses:\n{}",
        not_denied.join("\n")
    );
    // The test process (the worker's side) still reads the file.
    assert!(std::fs::read_to_string(agent.join("auth.json"))
        .unwrap()
        .contains(SENTINEL));

    // Positive control: the same read succeeds with the guard switched off.
    std::env::set_var(SWITCH, "off");
    let control = run_cell(
        &python,
        &project,
        &agent,
        &format!(
            "import json\nprint('YA41=' + json.dumps({{'read auth.json': 'ok' if {SENTINEL:?} in open({:?}).read() else 'no'}}))",
            agent.join("auth.json").display().to_string()
        ),
    )
    .await;
    std::env::remove_var(SWITCH);
    let _ = std::fs::remove_file(&socket);
    assert_eq!(
        control["read auth.json"], "ok",
        "the guard switched off must not refuse"
    );
}
