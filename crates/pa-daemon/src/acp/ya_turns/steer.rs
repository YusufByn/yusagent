//! yusAgent (YA-19, ticket 39): `_yus/session/steer`, a message into the running prompt turn
//! (`docs/tickets/39-message-pendant-un-tour.md`, §11 and §12).
//!
//! `_yus/session/steer` `{ sessionId, prompt, turn }` (Unharnessed's form, D2): the blocks of a
//! `session/prompt` and the running turn's `promptTurnId`. The message goes to the worker's
//! `steering` lane (`Steer`, path A of the ticket): the worker cuts its run at the next boundary,
//! after the tool results, and runs the message in a new run (`queued_steering_probe`,
//! `crates/pa-daemon/src/worker.rs:332-343`). That run belongs to the prompt's turn: the bridge's
//! wait for an idle worker with empty lanes (`fetch_autonomous_status`, `daemon.rs:1708`) holds
//! the turn open until it ran. `seal` closes the turn to new messages and waits again for those
//! that joined after that wait. Each message row after the prompt's own row opens a
//! `turnStart { origin: "steer" }` under the prompt's turn id, without `rewindTo` (D3).
//!
//! Refused with `-32603` ("The turn is over: send it as a prompt.") when the turn is not the
//! running prompt turn of this client, is closed, or is a turn the agent started (D6); with
//! `-32602` (`data.details`) for invalid blocks, images, or a session command.
//!
//! Ticket 39, push 1: the signatures only; the behavior comes with the next push.
#![allow(dead_code)] // Ticket 39, push 1: the skeleton.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::super::daemon::{DaemonAcpState, DaemonLink};
use super::super::jsonrpc;
use super::super::producer::FrameSink;
use super::TurnStartMeta;

/// The method (ticket 32 name).
pub(crate) const METHOD: &str = "_yus/session/steer";

/// The messages sent into one prompt turn of this client.
#[derive(Debug, Default)]
pub(crate) struct Steers {
    /// The prompt turn the messages may join (0: none).
    turn: u64,
    /// The prompt's own user row went by: a message may go to the worker after it.
    prompt_row_seen: bool,
    /// Messages sent to the worker, not acknowledged yet.
    sending: usize,
    /// Messages acknowledged since the last wait for an idle worker.
    since_wait: usize,
    /// Accepted messages whose row did not go by yet, in order (one `turnStart` each).
    pending: std::collections::VecDeque<u64>,
    /// The last message id of the turn.
    next: u64,
    /// No message joins the turn any more.
    closed: bool,
}

/// Whether a message may go to the worker now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Not into this turn: `-32603`.
    Refuse,
    /// The prompt's own row has not gone by yet.
    WaitRow,
    /// It goes now.
    Send,
}

/// What `seal` does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SealStep {
    /// A message is on its way to the worker.
    WaitSending,
    /// Messages joined after the last wait: wait for an idle worker again.
    Rewait,
    /// No message joins any more.
    Closed,
}

#[allow(clippy::unused_self)] // Ticket 39, push 1.
impl Steers {
    /// A prompt turn of this client starts.
    pub(crate) fn open(&mut self, turn: u64) {
        let _ = turn;
    }

    /// A message for `turn` (the request's `turn`), the bridge's active turn being `active`.
    pub(crate) fn gate(&self, turn: Option<u64>, active: u64, agent_turn: bool) -> Gate {
        let _ = (turn, active, agent_turn);
        Gate::Send
    }

    /// A message goes to the worker: its id.
    pub(crate) fn begin_send(&mut self) -> u64 {
        0
    }

    /// The worker answered for message `id`.
    pub(crate) fn end_send(&mut self, id: u64, sent: bool) {
        let _ = (id, sent);
    }

    /// One step of the turn's close.
    pub(crate) fn seal_step(&mut self) -> SealStep {
        SealStep::Closed
    }

    /// A worker event of the bridge's active turn `active`: `Some(turn)` when it is the row of a
    /// message sent into the turn, which opens its `turnStart`.
    pub(crate) fn row(&mut self, event: &Value, active: u64) -> Option<u64> {
        let _ = (event, active);
        None
    }

    /// Message `id` is still waiting for its row.
    pub(crate) fn waiting(&self, id: u64) -> bool {
        let _ = id;
        false
    }

    /// The turn `turn` ended without its close (Stop, failure): its waiting messages leave the
    /// worker's lane.
    pub(crate) fn orphaned(&self, turn: u64, active: u64) -> bool {
        let _ = (turn, active);
        false
    }
}

/// `turnStart` of a message that joined the turn.
pub(crate) fn steer_start() -> TurnStartMeta {
    TurnStartMeta::new("steer", None)
}

/// `_yus/session/steer`.
#[allow(clippy::unused_async)] // Ticket 39, push 1.
pub(crate) async fn serve(
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    let _ = (params, link, state);
    let _ = tx.send(jsonrpc::response(id, &json!({})));
}

/// After the worker's first wait in the prompt turn (`daemon.rs`, before the boundary): no
/// message joins the turn any more, and those that joined after that wait ran. Returns the
/// status of the last wait.
#[allow(clippy::unused_async)] // Ticket 39, push 1.
pub(crate) async fn seal(
    state: &Arc<Mutex<DaemonAcpState>>,
    link: &Arc<DaemonLink>,
    status: Option<pa_core::autonomous::AgentAutonomousStatus>,
) -> Option<pa_core::autonomous::AgentAutonomousStatus> {
    let _ = (state, link);
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_row(text: &str) -> Value {
        json!({ "type": "message_start", "message": {
            "role": "user", "content": [{ "type": "text", "text": text }] } })
    }

    /// U1: the window between the last wait and `finish_prompt`. A message the worker
    /// acknowledged after the wait makes `seal` wait once more, then the turn closes and a later
    /// message is refused.
    #[test]
    fn seal_waits_again_for_a_message_accepted_after_the_wait() {
        let mut steers = Steers::default();
        steers.open(4);
        assert_eq!(steers.row(&user_row("the prompt"), 4), None);
        let id = steers.begin_send();
        steers.end_send(id, true);
        assert_eq!(steers.seal_step(), SealStep::Rewait, "{steers:?}");
        assert_eq!(steers.seal_step(), SealStep::Closed, "{steers:?}");
        assert_eq!(steers.gate(Some(4), 4, false), Gate::Refuse, "{steers:?}");
    }

    /// U2: no message goes to the worker before the prompt's own row.
    #[test]
    fn a_steer_waits_for_the_prompt_row() {
        let mut steers = Steers::default();
        steers.open(2);
        assert_eq!(steers.gate(Some(2), 2, false), Gate::WaitRow, "{steers:?}");
        assert_eq!(steers.row(&user_row("the prompt"), 2), None);
        assert_eq!(steers.gate(Some(2), 2, false), Gate::Send, "{steers:?}");
    }

    /// U3: `seal` does not close while a message is on its way to the worker.
    #[test]
    fn seal_waits_for_a_message_in_flight() {
        let mut steers = Steers::default();
        steers.open(1);
        let _ = steers.row(&user_row("the prompt"), 1);
        let id = steers.begin_send();
        assert_eq!(steers.seal_step(), SealStep::WaitSending, "{steers:?}");
        assert_eq!(steers.gate(Some(1), 1, false), Gate::Send, "still open");
        steers.end_send(id, true);
        assert_eq!(steers.seal_step(), SealStep::Rewait);
        assert_eq!(steers.seal_step(), SealStep::Closed);
    }

    /// U4: the prompt's own row opens nothing; each later user row of the turn opens the
    /// `turnStart` of the next message sent, in order; a row with no message waiting, nothing.
    #[test]
    fn announce_maps_rows_to_steers() {
        let mut steers = Steers::default();
        steers.open(7);
        assert_eq!(steers.row(&user_row("the prompt"), 7), None);
        let one = steers.begin_send();
        let two = steers.begin_send();
        steers.end_send(one, true);
        steers.end_send(two, true);
        assert!(steers.waiting(one) && steers.waiting(two), "{steers:?}");
        let custom = json!({ "type": "message_start", "message": { "role": "custom" } });
        assert_eq!(steers.row(&custom, 7), None, "a custom row is no message");
        assert_eq!(steers.row(&user_row("one"), 7), Some(7));
        assert!(!steers.waiting(one) && steers.waiting(two), "{steers:?}");
        assert_eq!(steers.row(&user_row("two"), 7), Some(7));
        assert_eq!(steers.row(&user_row("a TUI row"), 7), None);
        assert_eq!(
            serde_json::to_value(steer_start()).unwrap(),
            json!({ "origin": "steer" }),
            "no rewindTo (D3)"
        );
    }

    /// U5: a turn that ended without its close (Stop, failure) leaves its waiting messages
    /// orphaned, to withdraw from the worker's lane; a closed turn, a refused one and a turn the
    /// agent started take none.
    #[test]
    fn a_turn_ended_without_its_close_orphans_its_messages() {
        let mut steers = Steers::default();
        steers.open(3);
        let _ = steers.row(&user_row("the prompt"), 3);
        let id = steers.begin_send();
        steers.end_send(id, true);
        assert!(!steers.orphaned(3, 3), "the turn runs");
        assert!(steers.orphaned(3, 0), "the turn ended without its close");
        assert_eq!(steers.gate(Some(3), 0, false), Gate::Refuse, "over");
        assert_eq!(steers.gate(Some(3), 3, true), Gate::Refuse, "D6");
        assert_eq!(steers.gate(Some(9), 3, false), Gate::Refuse, "stale turn");
        while steers.seal_step() != SealStep::Closed {}
        assert!(!steers.orphaned(3, 0), "closed: the messages ran");
    }
}
