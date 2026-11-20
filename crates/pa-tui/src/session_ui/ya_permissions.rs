//! yusAgent (YA-11, ticket 20, PR 3): the permission requests of the attached session.
//!
//! The TUI declares that it answers (`pa_types::daemon::ya_permissions::ASKER_FIELD` on its
//! `create` and its `attach`). A request event opens the panel (`crate::ya_permission_panel`); the
//! answer goes back by the `ya_permissions` command (`respond`), routed by the supervisor. The
//! first answer wins: a request closed elsewhere (another client answered, or the worker withdrew
//! it) closes here with a note. Listed in `docs/fork-changes.md`.

use pa_types::daemon::ya_permissions::{COMMAND, REQUEST_EVENT, RESOLVED_EVENT};
use serde_json::json;

use super::{key_event_to_id, AgentView, DaemonCommand, Instant, KeyEvent, Map, SessionUi, Value};
use crate::ya_permission_panel::{PanelKey, PermissionPanel, PermissionRequest};

fn text<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or_default()
}

impl SessionUi {
    /// One session event: `true` when it belongs to the permission requests.
    pub(super) fn ya_permission_event(&mut self, event: &Value, view: &mut AgentView) -> bool {
        self.drop_stale_permission_panel(view);
        let now = Instant::now();
        match text(event, "type") {
            REQUEST_EVENT => {
                if let Some(request) = PermissionRequest::from_event(event) {
                    if let Some(panel) = view.ya_permission.as_mut() {
                        panel.push(request);
                    } else {
                        view.ya_permission = Some(PermissionPanel::new(
                            self.active_session_id.clone(),
                            request,
                            now,
                        ));
                    }
                    self.dirty = true;
                }
                true
            }
            RESOLVED_EVENT => {
                let request_id = text(event, "requestId");
                let closed = view
                    .ya_permission
                    .as_mut()
                    .is_some_and(|panel| panel.close(request_id, now));
                if closed {
                    // Still shown here: another client answered, or the worker withdrew it.
                    let outcome = text(event, "outcome");
                    self.note(
                        &format!("The permission request was closed elsewhere ({outcome})."),
                        view,
                    );
                    self.close_empty_permission_panel(view);
                    self.dirty = true;
                }
                true
            }
            _ => false,
        }
    }

    /// One key while the panel is open: `true` when the panel took it; otherwise the key goes on
    /// to its usual owner (the quiet window, `crate::ya_permission_panel`).
    pub(super) fn ya_permission_key(&mut self, key: &KeyEvent, view: &mut AgentView) -> bool {
        self.drop_stale_permission_panel(view);
        let Some(id) = key_event_to_id(key) else {
            return false;
        };
        let now = Instant::now();
        let Some(panel) = view.ya_permission.as_mut() else {
            return false;
        };
        let action = panel.handle_key(&id, now);
        self.dirty = true;
        let Some(outcome) = action.outcome() else {
            return action == PanelKey::Consumed;
        };
        if id == "ctrl+c" {
            // Ctrl+C was handled: the force-quit guard disarms (as the pickers do).
            self.exit_guard.note_ctrl_c_handled();
        }
        if let Some(request_id) = panel.current().map(|request| request.request_id.clone()) {
            panel.close(&request_id, now);
            self.answer_permission(&request_id, outcome);
        }
        self.close_empty_permission_panel(view);
        true
    }

    /// A paste went to the editor: the panel's quiet window restarts.
    pub(super) fn ya_permission_input(view: &mut AgentView) {
        if let Some(panel) = view.ya_permission.as_mut() {
            panel.note_input(Instant::now());
        }
    }

    /// Sends the answer to the worker; a request no longer waiting (answered elsewhere, or
    /// withdrawn) is reported as a note.
    fn answer_permission(&self, request_id: &str, outcome: &'static str) {
        let client = self.client.clone();
        let active_session_id = self.active_session_id.clone();
        let notes = self.notes.clone();
        let mut rest = Map::new();
        rest.insert("requestId".to_string(), json!(request_id));
        rest.insert("outcome".to_string(), json!(outcome));
        tokio::spawn(async move {
            let answered = client
                .request_ok(DaemonCommand::YaPermissions {
                    id: None,
                    active_session_id,
                    action: "respond".to_string(),
                    rest,
                })
                .await;
            match answered {
                Ok(data) if data.get("resolved").and_then(Value::as_bool) == Some(false) => {
                    let _ = notes.send(
                        "The permission request was no longer waiting; the answer was not used."
                            .to_string(),
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = notes.send(format!("The {COMMAND} answer failed: {error:#}"));
                }
            }
        });
    }

    /// A panel left from another session (the view switched) goes away.
    fn drop_stale_permission_panel(&mut self, view: &mut AgentView) {
        if view
            .ya_permission
            .as_ref()
            .is_some_and(|panel| panel.session_id() != self.active_session_id)
        {
            view.ya_permission = None;
            self.dirty = true;
        }
    }

    fn close_empty_permission_panel(&mut self, view: &mut AgentView) {
        if view
            .ya_permission
            .as_ref()
            .is_some_and(PermissionPanel::is_empty)
        {
            view.ya_permission = None;
            self.dirty = true;
        }
    }
}
