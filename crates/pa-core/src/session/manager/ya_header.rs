//! yusAgent (YA-03): a field of the session header, set by the engine at creation.
//!
//! The header is the session file's first line and is read without walking the history (a
//! windowed open keeps it too), so a per-session fact stored there is found on every resume
//! without adding a row to the session tree or to the model context. Listed in
//! `docs/fork-changes.md`.

use super::{FileEntry, SessionManager};

impl SessionManager {
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
        let Some(header) = self.file_entries.iter_mut().find_map(|entry| match entry {
            FileEntry::Header { header } => Some(header),
            _ => None,
        }) else {
            return false;
        };
        header.rest.insert(key.to_string(), value);
        let on_disk = self.session_file.as_ref().is_some_and(|path| path.exists());
        if self.persist && self.flushed && on_disk {
            self.rewrite_file();
        }
        true
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
}
