//! yusAgent (YA-11, ticket 20, PR 3): the permission requests of the attached session.
//!
//! The TUI declares that it answers (`pa_types::daemon::ya_permissions::ASKER_FIELD` on its
//! `create` and its `attach`). A request event opens the panel (`crate::ya_permission_panel`); the
//! answer goes back by the `ya_permissions` command (`respond`), routed by the supervisor. The
//! first answer wins: a request closed elsewhere (another client answered, or the worker withdrew
//! it) closes here with a note. Listed in `docs/fork-changes.md`.

use pa_types::daemon::ya_permissions::{
    COMMAND, MODE_EVENT, REQUEST_EVENT, RESOLVED_EVENT, STATE_FIELD,
};
use serde_json::json;

use super::{
    key_event_to_id, AgentView, DaemonCommand, Duration, Instant, KeyEvent, Map, SessionUi, Value,
    UI_REQUEST_TIMEOUT_MS,
};
use crate::choice_picker::ChoicePicker;
use crate::ya_permission_panel::{PanelKey, PermissionPanel, PermissionRequest};

fn text<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Whether an attach snapshot's state reports the session in "auto" (the field is there only
/// for a session that can ask).
pub(super) fn state_is_auto(state: Option<&Value>) -> bool {
    state
        .and_then(|state| state.get(STATE_FIELD))
        .and_then(Value::as_str)
        == Some("auto")
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
            MODE_EVENT => {
                // A change of mode, from this client or another one (the ACP selector).
                self.show_permission_mode(text(event, "mode") == "auto", view);
                true
            }
            _ => false,
        }
    }

    /// The tray's "auto · no sandbox" badge follows the session's mode (decision D4).
    fn show_permission_mode(&mut self, auto: bool, view: &mut AgentView) {
        self.ya_permission_auto = auto;
        view.chrome.ya_auto_no_sandbox = auto;
        self.dirty = true;
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

    /// `/permissions [manual|auto]` (decision T4): without an argument, the picker with the
    /// current mode checked; with one, the switch.
    pub(super) async fn permissions_command(&mut self, args: &str, view: &mut AgentView) {
        view.editor.set_text("");
        match args.trim() {
            "" => {
                let Some(mode) = self.permission_mode(view).await else {
                    return;
                };
                view.choice_picker = Some(ChoicePicker::permissions(Some(&mode)));
                self.dirty = true;
            }
            mode @ ("manual" | "auto") => self.apply_permission_mode(mode, view).await,
            _ => self.error_row("Usage: /permissions [manual|auto]", view),
        }
    }

    /// The user's choice of mode for the session: the worker's `set_mode`, the path of the ACP
    /// selector, journaled by the worker when the mode changes. A request already waiting stays
    /// to answer (decision T7).
    pub(super) async fn apply_permission_mode(&mut self, mode: &str, view: &mut AgentView) {
        let mut rest = Map::new();
        rest.insert("mode".to_string(), json!(mode));
        let switched = self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::YaPermissions {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    action: "set_mode".to_string(),
                    rest,
                },
            )
            .await;
        match switched {
            Ok(_) => {
                self.show_permission_mode(mode == "auto", view);
                let text = if mode == "auto" {
                    "Permissions: auto. Tool calls run without asking, with your user's rights (no sandbox)."
                } else {
                    "Permissions: manual. Each tool call asks first."
                };
                self.note(text, view);
            }
            Err(error) => self.error_row(&format!("{error:#}"), view),
        }
        self.dirty = true;
    }

    /// The mode of the session (`get`); `None`, with a note, when it cannot ask.
    async fn permission_mode(&mut self, view: &mut AgentView) -> Option<String> {
        let answered = self
            .bounded_request(
                Duration::from_millis(UI_REQUEST_TIMEOUT_MS),
                DaemonCommand::YaPermissions {
                    id: None,
                    active_session_id: self.active_session_id.clone(),
                    action: "get".to_string(),
                    rest: Map::new(),
                },
            )
            .await;
        match answered {
            Ok(data) => {
                let mode = data.get("mode").and_then(Value::as_str).map(str::to_string);
                if mode.is_none() {
                    self.note(
                        "This session cannot ask for permission: it was not created by a client that answers.",
                        view,
                    );
                }
                mode
            }
            Err(error) => {
                self.error_row(&format!("{error:#}"), view);
                None
            }
        }
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
