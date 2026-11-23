//! yusAgent (YA-10, ticket 29): a daemon started by another build of yusagent.
//!
//! `probe_daemon` keeps any daemon that speaks the same protocol and schema
//! (`interactive_mode/daemon.rs:37-41`): after a binary change, the bridges of the new build talked
//! to the workers of the old one, which lack what was added to the worker since (C12's `others`,
//! the usage journal). Before `ensure_daemon_running`, the ACP mode compares the daemon's
//! `appVersion` with its own: a daemon of another build is shut down when nothing holds it, and
//! refused otherwise. The TUI keeps upstream's behavior.

use std::path::Path;
use std::time::{Duration, Instant};

use pa_tui::daemon_client::DaemonClient;
use pa_types::daemon::{DaemonCommand, DAEMON_PROTOCOL_VERSION, DAEMON_SCHEMA_ID};
use serde_json::{json, Map, Value};

/// How long a client still attached may take to go (a bridge that just closed; C12 waits as long).
const ATTACHED_GRACE: Duration = Duration::from_secs(3);
/// The pause between two `list` while a client may still go.
const POLL: Duration = Duration::from_millis(100);
/// The stop pass snapshots every kernel before the socket goes (the supervisor keeps accepting
/// until its workers are stopped), so a bridge that started now would join the dying daemon.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(30);
/// The `list` fields of a live session that keep the daemon when true: a turn, a compaction or
/// queued work (`isSessionActive`), the user's `!` bash, tools running, RLM children, a quota park.
const BUSY_FIELDS: [&str; 5] = [
    "isSessionActive",
    "isBashRunning",
    "isRunningTools",
    "hasRunningSubagents",
    "isQuotaParked",
];

/// Replace a daemon of another build on `socket_path` when nothing holds it.
///
/// # Errors
/// Returns the refusal when the daemon of another build still holds work, or does not stop.
pub(crate) async fn replace_other_build(socket_path: &Path) -> Result<(), String> {
    replace_other_build_as(socket_path, crate::config::version(), ATTACHED_GRACE).await
}

/// [`replace_other_build`] for the build `own_version`, waiting `grace` for attached clients.
async fn replace_other_build_as(
    socket_path: &Path,
    own_version: &str,
    grace: Duration,
) -> Result<(), String> {
    // No daemon: `ensure_daemon_running` starts one.
    let Ok((client, _events)) = DaemonClient::connect(socket_path).await else {
        return Ok(());
    };
    let hello = client.hello().clone();
    // Another protocol or schema is upstream's stale daemon (`probe_daemon`); no version, nothing
    // to compare.
    let same_schema = hello.get("protocol").and_then(|p| p.get("version"))
        == Some(&json!(DAEMON_PROTOCOL_VERSION))
        && hello.get("schemaId") == Some(&json!(DAEMON_SCHEMA_ID));
    let Some(daemon_version) = hello
        .get("appVersion")
        .and_then(Value::as_str)
        .filter(|version| same_schema && *version != own_version)
    else {
        client.close();
        return Ok(());
    };
    let deadline = Instant::now() + grace;
    let holders = loop {
        let holders = holders(&client).await;
        if holders.is_empty() || Instant::now() >= deadline {
            break holders;
        }
        tokio::time::sleep(POLL).await;
    };
    if !holders.is_empty() {
        client.close();
        return Err(format!(
            "A yusAgent daemon from another build is running on {}.\n  daemon:        {daemon_version}\n  this yusagent: {own_version}\nIt was kept because it still holds work:\n{}\n\nRun:\n  yusagent shutdown\n\nThen Restart (stopping the daemon ends that work).",
            socket_path.display(),
            holders
                .iter()
                .map(|holder| format!("  - {holder}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let _ = client
        .request_ok(DaemonCommand::Shutdown {
            id: None,
            force: None,
            rest: Map::default(),
        })
        .await;
    client.close();
    if !socket_gone(socket_path).await {
        return Err(format!(
            "The yusAgent daemon from another build ({daemon_version}) on {} did not stop within {} s.\n\nRun:\n  yusagent shutdown\n\nThen Restart.",
            socket_path.display(),
            SHUTDOWN_WAIT.as_secs()
        ));
    }
    Ok(())
}

/// The live sessions that hold the daemon, each with its reasons. A `list` that fails holds it.
async fn holders(client: &DaemonClient) -> Vec<String> {
    let listed = client
        .request_ok(DaemonCommand::List {
            id: None,
            all: None,
            cwd: None,
            session_dir: None,
            include_client_owned: None,
            rest: Map::default(),
        })
        .await;
    let Some(rows) = listed
        .ok()
        .and_then(|data| data.get("sessions").and_then(Value::as_array).cloned())
    else {
        return vec!["the daemon did not list its sessions".to_string()];
    };
    let mut holders = Vec::new();
    for row in rows {
        // A passivated row has no worker left, so nothing to lose: it has no `activeSessionId`
        // (`crates/pa-daemon/src/supervisor_roster.rs`, `passivated_summary`).
        let Some(id) = row.get("activeSessionId").and_then(Value::as_str) else {
            continue;
        };
        let mut reasons = Vec::new();
        if row
            .get("attachedClients")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            reasons.push("a client is attached".to_string());
        }
        reasons.extend(
            BUSY_FIELDS
                .iter()
                .filter(|field| row.get(**field) == Some(&Value::Bool(true)))
                .map(|field| (*field).to_string()),
        );
        if reasons.is_empty() {
            match holds(client, id).await {
                Some(false) => {}
                Some(true) => reasons.push(
                    "it holds work (a background bash(), a scheduled job or RLM work)".to_string(),
                ),
                None => reasons.push(
                    "its worker cannot tell what it holds (a build from before this check)"
                        .to_string(),
                ),
            }
        }
        if !reasons.is_empty() {
            holders.push(format!("{id}: {}", reasons.join(", ")));
        }
    }
    holders
}

/// The worker's answer to `holds` (the passivation's gate); `None` when it does not serve it.
async fn holds(client: &DaemonClient, active_session_id: &str) -> Option<bool> {
    let response = client
        .request(DaemonCommand::YaPermissions {
            id: None,
            active_session_id: active_session_id.to_string(),
            action: "holds".to_string(),
            rest: Map::default(),
        })
        .await
        .ok()?;
    if !response.success {
        return None;
    }
    response.data.as_ref()?.get("holds")?.as_bool()
}

/// Wait until nothing accepts connections on the socket (bounded by [`SHUTDOWN_WAIT`]).
async fn socket_gone(socket_path: &Path) -> bool {
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    while Instant::now() < deadline {
        if !pa_daemon::socket::can_connect(socket_path, Duration::from_millis(250)).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

// The fake supervisor listens on a Unix socket.
#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::*;

    const OWN: &str = "0.10.2-continuous.new";
    const OTHER: &str = "0.10.2-continuous.old";
    const GRACE: Duration = Duration::from_millis(400);

    /// What the fake supervisor answers.
    struct Script {
        app_version: Option<&'static str>,
        schema_id: &'static str,
        /// The `sessions` of each `list` answer, in order; the last one repeats.
        lists: Vec<Value>,
        /// The `holds` of `ya_permissions`; `None` is a daemon that does not serve the action.
        holds: Option<bool>,
    }

    impl Script {
        fn other(lists: Vec<Value>, holds: Option<bool>) -> Self {
            Self {
                app_version: Some(OTHER),
                schema_id: pa_types::daemon::DAEMON_SCHEMA_ID,
                lists,
                holds,
            }
        }
    }

    /// A live session at rest, without client.
    fn row(id: &str) -> Value {
        json!({
            "id": id, "activeSessionId": id, "lifecycle": "live", "activity": "idle",
            "isSessionActive": false, "isBashRunning": false, "isRunningTools": false,
            "hasRunningSubagents": false, "isQuotaParked": false, "attachedClients": 0,
            "workerState": "ready",
        })
    }

    fn with(mut row: Value, field: &str, value: Value) -> Value {
        row[field] = value;
        row
    }

    /// A fake supervisor on a socket in `dir`: the hello of `script`, then its answers to `list`,
    /// `ya_permissions` and `shutdown`. A `shutdown` removes the socket. Every command received
    /// is recorded.
    fn fake_supervisor(dir: &Path, script: Script) -> (PathBuf, Arc<Mutex<Vec<Value>>>) {
        let socket = dir.join("d.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        let path = socket.clone();
        let lists_served = AtomicUsize::new(0);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (read, mut write) = stream.into_split();
                let mut hello = json!({
                    "type": "daemon_hello",
                    "socketPath": path.to_string_lossy(),
                    "protocol": {
                        "name": pa_types::daemon::DAEMON_PROTOCOL_NAME,
                        "version": pa_types::daemon::DAEMON_PROTOCOL_VERSION,
                    },
                    "schemaId": script.schema_id,
                    "clientId": "fake",
                    "serverCapabilities": [],
                });
                if let Some(version) = script.app_version {
                    hello["appVersion"] = json!(version);
                }
                write
                    .write_all(format!("{hello}\n").as_bytes())
                    .await
                    .unwrap();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let envelope: Value = serde_json::from_str(&line).unwrap();
                    let command = envelope["command"].clone();
                    let kind = command["type"].as_str().unwrap_or_default().to_string();
                    log.lock().unwrap().push(command);
                    let mut response = json!({
                        "type": "response", "id": envelope["id"], "command": kind, "success": true,
                    });
                    match kind.as_str() {
                        "list" => {
                            let served = lists_served.fetch_add(1, Ordering::SeqCst);
                            let sessions = script.lists[served.min(script.lists.len() - 1)].clone();
                            response["data"] = json!({ "sessions": sessions });
                        }
                        "ya_permissions" => {
                            if let Some(holds) = script.holds {
                                response["data"] = json!({ "holds": holds });
                            } else {
                                response["success"] = json!(false);
                                response["error"] = json!("the action must be \"respond\", \"set_mode\", \"get\", \"asker\" or \"others\"");
                            }
                        }
                        "shutdown" => {}
                        other => panic!("unexpected command: {other}"),
                    }
                    write
                        .write_all(format!("{response}\n").as_bytes())
                        .await
                        .unwrap();
                    if kind == "shutdown" {
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });
        (socket, received)
    }

    fn types(received: &Arc<Mutex<Vec<Value>>>) -> Vec<String> {
        received
            .lock()
            .unwrap()
            .iter()
            .map(|command| command["type"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    fn assert_refused(result: &Result<(), String>, received: &Arc<Mutex<Vec<Value>>>) -> String {
        assert!(
            result.is_err(),
            "the daemon of another build must be refused"
        );
        let refusal = result.clone().unwrap_err();
        assert!(refusal.contains(OTHER), "{refusal}");
        assert!(refusal.contains(OWN), "{refusal}");
        assert!(refusal.contains("yusagent shutdown"), "{refusal}");
        assert!(
            !types(received).contains(&"shutdown".to_string()),
            "a refused daemon is not shut down"
        );
        refusal
    }

    fn assert_replaced(result: &Result<(), String>, received: &Arc<Mutex<Vec<Value>>>) {
        assert_eq!(result, &Ok(()));
        assert!(
            types(received).contains(&"shutdown".to_string()),
            "the daemon of another build must be shut down: {:?}",
            types(received)
        );
    }

    /// A daemon of another build whose only session is at rest, without client and holding
    /// nothing: shut down (A3).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_idle_daemon_of_another_build_is_shut_down() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, received) = fake_supervisor(
            dir.path(),
            Script::other(vec![json!([row("a")])], Some(false)),
        );
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        assert_replaced(&result, &received);
        let asked = received
            .lock()
            .unwrap()
            .iter()
            .find(|command| command["type"] == "ya_permissions")
            .cloned()
            .expect("the worker is asked what it holds");
        assert_eq!(asked["action"], "holds");
        assert_eq!(asked["activeSessionId"], "a");
    }

    /// A turn running (or any busy field of the row): refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_busy_session_keeps_the_daemon_of_another_build() {
        for field in [
            "isSessionActive",
            "isBashRunning",
            "isRunningTools",
            "hasRunningSubagents",
            "isQuotaParked",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (socket, received) = fake_supervisor(
                dir.path(),
                Script::other(
                    vec![json!([with(row("a"), field, json!(true))])],
                    Some(false),
                ),
            );
            let result = replace_other_build_as(&socket, OWN, GRACE).await;
            let refusal = assert_refused(&result, &received);
            assert!(refusal.contains(field), "{field}: {refusal}");
        }
    }

    /// A client attached for longer than the wait: refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_attached_client_keeps_the_daemon_of_another_build() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, received) = fake_supervisor(
            dir.path(),
            Script::other(
                vec![json!([with(row("a"), "attachedClients", json!(1))])],
                Some(false),
            ),
        );
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        let refusal = assert_refused(&result, &received);
        assert!(refusal.contains("attached"), "{refusal}");
    }

    /// A bridge that just closed detaches during the wait: the daemon is shut down.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_client_leaving_during_the_wait_lets_the_daemon_go() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, received) = fake_supervisor(
            dir.path(),
            Script::other(
                vec![
                    json!([with(row("a"), "attachedClients", json!(1))]),
                    json!([row("a")]),
                ],
                Some(false),
            ),
        );
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        assert_replaced(&result, &received);
    }

    /// The worker holds work the row does not show (a background `bash()`, a scheduled job, RLM
    /// work): refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_session_holding_work_keeps_the_daemon_of_another_build() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, received) = fake_supervisor(
            dir.path(),
            Script::other(vec![json!([row("a")])], Some(true)),
        );
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        let refusal = assert_refused(&result, &received);
        assert!(refusal.contains("holds work"), "{refusal}");
    }

    /// P1a: a worker that does not serve `holds` (a daemon from before ticket 29) cannot tell
    /// what it holds, so its live session keeps the daemon.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_without_holds_counts_every_live_session_as_busy() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, received) =
            fake_supervisor(dir.path(), Script::other(vec![json!([row("a")])], None));
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        let refusal = assert_refused(&result, &received);
        assert!(refusal.contains("cannot tell"), "{refusal}");
    }

    /// A passivated row (no worker left, no `activeSessionId`) holds nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_passivated_session_does_not_keep_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let passivated = json!({
            "id": "p", "lifecycle": "live", "activity": "idle", "isSessionActive": false,
            "attachedClients": 0,
        });
        let (socket, received) =
            fake_supervisor(dir.path(), Script::other(vec![json!([passivated])], None));
        let result = replace_other_build_as(&socket, OWN, GRACE).await;
        assert_replaced(&result, &received);
        assert!(!types(&received).contains(&"ya_permissions".to_string()));
    }

    /// The same build: kept, nothing asked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_of_the_same_build_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut script = Script::other(vec![json!([row("a")])], Some(false));
        script.app_version = Some(OWN);
        let (socket, received) = fake_supervisor(dir.path(), script);
        assert_eq!(replace_other_build_as(&socket, OWN, GRACE).await, Ok(()));
        assert!(types(&received).is_empty(), "{:?}", types(&received));
    }

    /// No `appVersion`, or another schema: left to upstream's schema check (`probe_daemon`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_without_version_or_of_another_schema_is_left_to_upstream() {
        for (app_version, schema_id) in [
            (None, pa_types::daemon::DAEMON_SCHEMA_ID),
            (Some(OTHER), "protocol-7-schema-0-other"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut script = Script::other(vec![json!([row("a")])], Some(false));
            script.app_version = app_version;
            script.schema_id = schema_id;
            let (socket, received) = fake_supervisor(dir.path(), script);
            assert_eq!(replace_other_build_as(&socket, OWN, GRACE).await, Ok(()));
            assert!(types(&received).is_empty(), "{:?}", types(&received));
        }
    }

    /// No daemon: nothing to do (`ensure_daemon_running` starts one).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_daemon_is_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            replace_other_build_as(&dir.path().join("none.sock"), OWN, GRACE).await,
            Ok(())
        );
    }
}
