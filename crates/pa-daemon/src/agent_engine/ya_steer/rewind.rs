//! yusAgent (YA-18, ticket 43): the rows a rewind gives the model with the next turn, into the
//! core session's next-turn mailbox (`pa_core::session_engine::ya_resource_notice::rewind`), the
//! one the next admission sends to the model (`docs/tickets/43-retour-arriere.md`, §L). The worker
//! writes the same rows to the session file with the turn's prefix rows
//! (`crate::ya_permissions::rewind`), as ticket 31 does for an announcement.
//!
//! Declared in this fork module (option C of the ticket): no upstream line; the core session is a
//! field of the engine this module sees as a descendant of `agent_engine`. Listed in
//! `docs/fork-changes.md`.

use pa_types::session::CustomMessage;

use super::super::AgentSessionEngine;

impl AgentSessionEngine {
    /// After a rewind: `rows` ride the next turn to the model. Nothing before the core session is
    /// built (no conversation to rewind then).
    pub(crate) async fn ya_queue_rewind_rows(&self, rows: Vec<CustomMessage>) {
        let core = self.session.lock().await.clone();
        if let Some(core) = core {
            core.session.ya_queue_rewind_rows(rows);
        }
    }
}
