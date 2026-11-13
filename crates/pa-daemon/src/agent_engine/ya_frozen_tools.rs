//! yusAgent (YA-03): the frozen tools of a daemon session (`editTool`, `shellTool`), kept in the
//! header of the durable session file the worker owns.
//!
//! The engine runs on an in-memory session manager; `pa_core` freezes the tools in that
//! manager's header (`session_engine::ya_tool_family::freeze_session_tools`). The daemon carries
//! the field both ways:
//! - read: each build copies the durable header's field into the in-memory header
//!   (`lifecycle.rs`, next to the header id), so a resumed or rebuilt session keeps its tools;
//! - write: after a build, the frozen value goes to the worker's store through the sink the
//!   worker installs ([`AgentSessionEngine::set_ya_header_sink`]), written only when the
//!   durable header has none.
//!
//! Listed in `docs/fork-changes.md`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::{AgentSessionEngine, CoreSessionEngine};
use crate::session_store::SessionFile;
use crate::worker::SessionCore;

/// The header field the frozen tools live in.
pub(crate) const FROZEN_TOOLS_KEY: &str =
    pa_core::session_engine::ya_tool_family::FROZEN_TOOLS_HEADER_KEY;

/// Writes a header field of the durable session at a path, when absent.
pub(crate) type YaHeaderSink = Arc<dyn Fn(&Path, &str, &Value) + Send + Sync>;

/// Set the header field `key` of `store` when it has none, and persist it: a store on disk is
/// rewritten (header and entries, as `SessionFile::rewrite`); a store not written yet gets it
/// with its first write; a windowed store keeps it in memory only (a rewrite needs the whole
/// history), so a later build of a legacy session may freeze it again.
///
/// Returns whether the field was set.
///
/// # Errors
///
/// The rewrite's error; the field stays set in memory.
pub(crate) fn write_header_field(
    store: &mut SessionFile,
    key: &str,
    value: &Value,
) -> anyhow::Result<bool> {
    if store.header.rest.contains_key(key) {
        return Ok(false);
    }
    store.header.rest.insert(key.to_string(), value.clone());
    if store.window.is_none() && store.path.exists() {
        store.rewrite()?;
    }
    Ok(true)
}

/// The worker's sink: writes into the live store when it is still the session at `path`. The
/// core lock is taken without blocking the build; when it is held, the write waits on a thread.
pub(crate) fn core_header_sink(core: Arc<Mutex<SessionCore>>) -> YaHeaderSink {
    Arc::new(move |path: &Path, key: &str, value: &Value| {
        let write = {
            let path = path.to_path_buf();
            let key = key.to_string();
            let value = value.clone();
            move |core: &mut SessionCore| {
                let Some(store) = core.store.as_mut().filter(|store| store.path == path) else {
                    return;
                };
                if let Err(error) = write_header_field(store, &key, &value) {
                    eprintln!("pa-daemon: session header field {key} not persisted: {error:#}");
                }
            }
        };
        match core.try_lock() {
            Ok(mut guard) => write(&mut *guard),
            Err(std::sync::TryLockError::Poisoned(poison)) => write(&mut *poison.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                let core = Arc::clone(&core);
                std::thread::spawn(move || {
                    let mut guard = core
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    write(&mut *guard);
                });
            }
        }
    })
}

impl AgentSessionEngine {
    /// Install the sink that writes the frozen tools into the durable session header.
    pub(crate) fn set_ya_header_sink(&self, sink: YaHeaderSink) {
        *self
            .ya_header_sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
    }

    /// After a build: hand the session's frozen tools to the sink (the worker writes them when
    /// the durable header has none). No sink, no durable file, or no frozen value: nothing.
    pub(super) fn ya_sync_frozen_tools(
        &self,
        engine: &CoreSessionEngine,
        session_file: Option<&Path>,
    ) {
        let Some(path) = session_file else {
            return;
        };
        let sink = self
            .ya_header_sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(sink) = sink else {
            return;
        };
        // The manager was just built and is not shared yet: the lock is free.
        let persistence = engine.session.shared_persistence();
        let Ok(manager) = persistence.try_lock() else {
            return;
        };
        let value = manager.ya_header_field(FROZEN_TOOLS_KEY).cloned();
        drop(manager);
        if let Some(value) = value {
            sink(path, FROZEN_TOOLS_KEY, &value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_engine::{AgentEngineConfig, FAUX_TEST_LOCK};
    use serde_json::json;

    fn kernel_tools() -> Value {
        json!({"editTool": "kernel", "shellTool": "kernel"})
    }

    /// A durable store on disk, with `rest` merged into its header.
    fn durable_store(dir: &Path, rest: Option<Value>) -> SessionFile {
        let mut store = SessionFile::create(dir.to_str().unwrap_or("/tmp"), None, 0);
        let path = dir.join(crate::session_store::session_file_name(store.session_id()));
        store.set_path(path);
        if let Some(Value::Object(rest)) = rest {
            store.header.rest.extend(rest);
        }
        store.rewrite().unwrap();
        store
    }

    fn faux_engine(dir: &Path, session_file: &Path) -> AgentSessionEngine {
        AgentSessionEngine::new(AgentEngineConfig {
            cwd: dir.to_path_buf(),
            agent_dir: dir.join("agent"),
            provider: None,
            model: None,
            api_key: None,
            thinking: None,
            session_dir: None,
            session_file: Some(session_file.to_path_buf()),
            faux_script: Some(
                json!({ "engine": "faux", "responses": [{ "text": "ok" }] }).to_string(),
            ),
            supervisor_link: None,
            telemetry_disabled: Some(true),
            cron_store: None,
            queued_steering_probe: None,
        })
        .unwrap()
    }

    /// The frozen tools of the built session's in-memory header.
    fn built_frozen_tools(engine: &AgentSessionEngine) -> Option<Value> {
        engine.runtime.block_on(async {
            let built = engine.session.lock().await.clone().expect("built session");
            let persistence = built.session.shared_persistence();
            let manager = persistence.lock().await;
            let value = manager.ya_header_field(FROZEN_TOOLS_KEY).cloned();
            drop(manager);
            value
        })
    }

    #[test]
    fn write_sets_an_absent_field_on_disk_and_keeps_a_present_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut store = durable_store(dir.path(), None);
        assert!(write_header_field(&mut store, FROZEN_TOOLS_KEY, &kernel_tools()).unwrap());
        let on_disk = pa_core::session::manager::read_session_header(&store.path).unwrap();
        assert_eq!(on_disk.rest.get(FROZEN_TOOLS_KEY), Some(&kernel_tools()));
        let other = json!({"editTool": "native", "shellTool": "native"});
        assert!(!write_header_field(&mut store, FROZEN_TOOLS_KEY, &other).unwrap());
        let on_disk = pa_core::session::manager::read_session_header(&store.path).unwrap();
        assert_eq!(on_disk.rest.get(FROZEN_TOOLS_KEY), Some(&kernel_tools()));
    }

    /// A branched fork (`create_branched_file`) keeps its source's frozen tools.
    #[test]
    fn a_branched_fork_keeps_the_frozen_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let codex = json!({"editTool": "native", "shellTool": "kernel"});
        let mut store = durable_store(dir.path(), Some(json!({ FROZEN_TOOLS_KEY: codex })));
        let leaf = store.append_message(&json!({
            "role": "user",
            "content": "hi",
            "timestamp": 0,
        }));
        let forked = store.create_branched_file(&leaf, dir.path()).unwrap();
        assert_ne!(forked.path, store.path);
        assert_eq!(forked.header.rest.get(FROZEN_TOOLS_KEY), Some(&codex));
    }

    /// A durable header without frozen tools: the build freezes them and the sink receives
    /// them for that file.
    #[test]
    fn a_build_hands_the_frozen_tools_to_the_sink() {
        let _faux = FAUX_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::TempDir::new().unwrap();
        let store = durable_store(dir.path(), None);
        let engine = faux_engine(dir.path(), &store.path);
        let received: Arc<Mutex<Vec<(std::path::PathBuf, String, Value)>>> = Arc::default();
        let sink_received = Arc::clone(&received);
        engine.set_ya_header_sink(Arc::new(move |path: &Path, key: &str, value: &Value| {
            sink_received.lock().unwrap().push((
                path.to_path_buf(),
                key.to_string(),
                value.clone(),
            ));
        }));
        let model = engine.resolve_model().expect("faux model");
        engine
            .runtime
            .block_on(engine.ensure_core_session_async(&model))
            .expect("build");
        assert_eq!(
            *received.lock().unwrap(),
            vec![(
                store.path.clone(),
                FROZEN_TOOLS_KEY.to_string(),
                kernel_tools()
            )]
        );
    }

    /// A durable header with frozen kernel tools, and settings asking for `editTool: "native"`
    /// (refused outside the Codex family): the build keeps the session's tools instead of
    /// failing on the settings.
    #[test]
    fn a_build_keeps_the_durable_frozen_tools_over_the_settings() {
        let _faux = FAUX_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::TempDir::new().unwrap();
        let store = durable_store(
            dir.path(),
            Some(json!({ FROZEN_TOOLS_KEY: kernel_tools() })),
        );
        std::fs::create_dir_all(dir.path().join("agent")).unwrap();
        std::fs::write(
            dir.path().join("agent/settings.json"),
            r#"{"editTool": "native"}"#,
        )
        .unwrap();
        let engine = faux_engine(dir.path(), &store.path);
        let model = engine.resolve_model().expect("faux model");
        engine
            .runtime
            .block_on(engine.ensure_core_session_async(&model))
            .expect("the frozen tools win over the settings");
        assert_eq!(built_frozen_tools(&engine), Some(kernel_tools()));
    }
}
