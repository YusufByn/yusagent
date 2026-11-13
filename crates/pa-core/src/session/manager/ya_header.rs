//! yusAgent (YA-03): fields of the session header, set by the engine at creation.
//!
//! The header is the session file's first line and is read without walking the history (a
//! windowed open keeps it too), so a per-session fact stored there is found on every resume
//! without adding a row to the session tree or to the model context. A session made from
//! another one (a fork, a materialized in-memory session, the daemon engine's in-memory copy
//! of a durable session) keeps the fields of [`INHERITED_HEADER_KEYS`]. Listed in
//! `docs/fork-changes.md`.

use super::{FileEntry, SessionHeader, SessionManager};

/// The header fields a session made from another one keeps: its frozen tools, which its
/// copied history was written with (`session_engine::ya_tool_family`).
pub const INHERITED_HEADER_KEYS: &[&str] =
    &[crate::session_engine::ya_tool_family::FROZEN_TOOLS_HEADER_KEY];

/// Copy the [`INHERITED_HEADER_KEYS`] present in `source_rest` (the `rest` of another
/// session's header) into `target`; absent ones stay as they are.
pub fn inherit_header_fields(target: &mut SessionHeader, source_rest: &pa_types::JsonMap) {
    for key in INHERITED_HEADER_KEYS {
        if let Some(value) = source_rest.get(*key) {
            target.rest.insert((*key).to_string(), value.clone());
        }
    }
}

impl SessionManager {
    fn ya_header_mut(&mut self) -> Option<&mut SessionHeader> {
        self.file_entries.iter_mut().find_map(|entry| match entry {
            FileEntry::Header { header } => Some(header),
            _ => None,
        })
    }

    /// The value of the header field `key`, if the session has a header carrying it.
    #[must_use]
    pub fn ya_header_field(&self, key: &str) -> Option<&serde_json::Value> {
        self.get_header()?.rest.get(key)
    }

    /// Set the header field `key`. A session file already on disk is rewritten so the field is
    /// durable at once; one not written yet gets it with its first flush. A windowed session (a
    /// daemon resume of a large file) is left as is: rewriting it would load its whole history.
    ///
    /// Returns whether the field was set.
    pub fn ya_set_header_field(&mut self, key: &str, value: serde_json::Value) -> bool {
        if self.window.is_some() {
            return false;
        }
        let Some(header) = self.ya_header_mut() else {
            return false;
        };
        header.rest.insert(key.to_string(), value);
        let on_disk = self.session_file.as_ref().is_some_and(|path| path.exists());
        if self.persist && self.flushed && on_disk {
            self.rewrite_file();
        }
        true
    }

    /// [`inherit_header_fields`] into this session's header, in memory: written with the next
    /// write of the header (the caller's flush or rewrite).
    pub fn ya_inherit_header_fields(&mut self, source_rest: &pa_types::JsonMap) {
        if let Some(header) = self.ya_header_mut() {
            inherit_header_fields(header, source_rest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NewSessionOptions, SessionManager};
    use serde_json::json;

    #[test]
    fn field_survives_reopen_of_a_flushed_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = SessionManager::in_memory(dir.path());
        let path = manager.materialize_session_file(Some(dir.path().join("sessions")));
        assert!(path.exists(), "materialize flushes the header");
        assert!(manager.ya_set_header_field("yaTest", json!({"a": 1})));
        assert_eq!(manager.ya_header_field("yaTest"), Some(&json!({"a": 1})));
        let reopened = SessionManager::open(dir.path(), &dir.path().join("sessions"), &path);
        assert_eq!(reopened.ya_header_field("yaTest"), Some(&json!({"a": 1})));
        let header = super::super::read_session_header(&path).unwrap();
        assert_eq!(header.rest.get("yaTest"), Some(&json!({"a": 1})));
    }

    #[test]
    fn field_of_an_unflushed_session_is_written_with_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let mut manager = SessionManager::persisted(dir.path(), &sessions);
        let path = manager
            .new_session(&NewSessionOptions::default())
            .expect("a persisted session has a file");
        assert!(manager.ya_set_header_field("yaTest", json!("x")));
        assert!(!path.exists(), "nothing is written before the first flush");
        manager.flush_now().unwrap();
        let header = super::super::read_session_header(&path).unwrap();
        assert_eq!(header.rest.get("yaTest"), Some(&json!("x")));
    }

    fn codex_tools() -> serde_json::Value {
        json!({"editTool": "native", "shellTool": "kernel"})
    }

    /// A fork's new header keeps the source's frozen tools.
    #[test]
    fn fork_keeps_the_frozen_tools_of_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let mut source = SessionManager::in_memory(dir.path());
        let source_path = source.materialize_session_file(Some(sessions.clone()));
        assert!(source.ya_set_header_field("yaTools", codex_tools()));
        let forked = SessionManager::fork_from(&source_path, dir.path(), &sessions).unwrap();
        let forked_path = forked.get_session_file().unwrap().to_path_buf();
        assert_ne!(forked_path, source_path);
        assert_eq!(forked.ya_header_field("yaTools"), Some(&codex_tools()));
        let header = super::super::read_session_header(&forked_path).unwrap();
        assert_eq!(header.rest.get("yaTools"), Some(&codex_tools()));
        assert_eq!(
            header.parent_session.as_deref(),
            Some(source_path.display().to_string().as_str())
        );
    }

    /// An in-memory session materialized into a file keeps its frozen tools.
    #[test]
    fn materialized_session_keeps_its_frozen_tools() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = SessionManager::in_memory(dir.path());
        assert!(manager.ya_set_header_field("yaTools", codex_tools()));
        let path = manager.materialize_session_file(Some(dir.path().join("sessions")));
        assert_eq!(manager.ya_header_field("yaTools"), Some(&codex_tools()));
        let header = super::super::read_session_header(&path).unwrap();
        assert_eq!(header.rest.get("yaTools"), Some(&codex_tools()));
    }

    /// Only the inherited keys are copied, and an absent one leaves the target as is.
    #[test]
    fn inheritance_copies_only_the_listed_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = SessionManager::in_memory(dir.path());
        let mut source = pa_types::JsonMap::new();
        source.insert("other".into(), json!(1));
        manager.ya_inherit_header_fields(&source);
        assert_eq!(manager.ya_header_field("other"), None);
        assert_eq!(manager.ya_header_field("yaTools"), None);
        source.insert("yaTools".into(), codex_tools());
        manager.ya_inherit_header_fields(&source);
        assert_eq!(manager.ya_header_field("yaTools"), Some(&codex_tools()));
    }
}
