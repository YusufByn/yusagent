//! yusAgent (YA-19, ticket 39): `_yus/session/steer`, a message into the running prompt turn
//! (`docs/tickets/39-message-pendant-un-tour.md`, §17 and §18, path B).
//!
//! `_yus/session/steer` `{ sessionId, prompt, turn }` (Unharnessed's form, D2): the blocks of a
//! `session/prompt` and the running turn's `promptTurnId`. The message goes to the worker as a
//! `Steer` marked `yaInRun`: the worker passes it to the running agent's own queue, which the
//! loop reads after the tool results, before its next model call, in the same run
//! (`crates/pa-daemon/src/worker/ya_steer.rs`). The prompt's `prompt_and_wait` covers that run,
//! so the turn stays open until the message is answered. The run's settle drops what it did not
//! take; the client sends it again as a prompt.
//!
//! A message goes to the worker only once the turn's first assistant message went by: the run's
//! admitted rows are then past, so a prompt row never passes for a message's
//! (`crates/pa-daemon/src/agent_engine/ya_steer.rs`). Each user row of the turn after that opens
//! the `turnStart { origin: "steer" }` of the next message sent, under the prompt's turn id,
//! without `rewindTo` (D3).
//!
//! Refused with `-32603` ("The turn is over: send it as a prompt.") when the turn is not the
//! running prompt turn of this client, is a turn the agent started (D6), or no run takes the
//! message; with `-32602` (`data.details`) for invalid blocks, images, or a session command.
//!
//! Ticket 39, push 1 of path B: the signatures only; the behavior comes with the next push.
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
    /// The turn's first assistant message went by: a message may go to the worker.
    model_started: bool,
    /// Messages sent to the worker whose row did not go by yet: one `turnStart` each.
    pending: usize,
}

/// Whether a message may go to the worker now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Not into this turn: `-32603`.
    Refuse,
    /// The turn's first assistant message has not gone by yet.
    WaitModel,
    /// It goes now.
    Send,
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

    /// A message goes to the worker: its `turnStart` is due at its row.
    pub(crate) fn sent(&mut self) {}

    /// The worker refused the message sent into `turn`: no row will come for it.
    pub(crate) fn refused(&mut self, turn: u64) {
        let _ = turn;
    }

    /// A worker event of the bridge's active turn `active`: `Some(turn)` when it is the row of a
    /// message sent into the turn, which opens its `turnStart`.
    pub(crate) fn row(&mut self, event: &Value, active: u64) -> Option<u64> {
        let _ = (event, active);
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    fn message_start(role: &str, text: &str) -> Value {
        json!({ "type": "message_start", "message": {
            "role": role, "content": [{ "type": "text", "text": text }] } })
    }

    /// U1: no message goes to the worker before the turn's first assistant message: the run's
    /// admitted rows are not past yet.
    #[test]
    fn a_steer_waits_for_the_model_to_start() {
        let mut steers = Steers::default();
        steers.open(2);
        assert_eq!(
            steers.gate(Some(2), 2, false),
            Gate::WaitModel,
            "{steers:?}"
        );
        assert_eq!(steers.row(&message_start("user", "the prompt"), 2), None);
        assert_eq!(
            steers.gate(Some(2), 2, false),
            Gate::WaitModel,
            "the prompt's row is not the model: {steers:?}"
        );
        assert_eq!(steers.row(&message_start("assistant", ""), 2), None);
        assert_eq!(steers.gate(Some(2), 2, false), Gate::Send, "{steers:?}");
    }

    /// U2: once the model started, each user row of the turn opens the `turnStart` of the next
    /// message sent, in order; a custom row, or a user row with no message waiting, nothing.
    #[test]
    fn rows_after_the_model_start_open_one_turn_start_each() {
        let mut steers = Steers::default();
        steers.open(7);
        let _ = steers.row(&message_start("user", "the prompt"), 7);
        let _ = steers.row(&message_start("assistant", ""), 7);
        steers.sent();
        steers.sent();
        let custom = json!({ "type": "message_start", "message": { "role": "custom" } });
        assert_eq!(steers.row(&custom, 7), None, "a custom row is no message");
        assert_eq!(steers.row(&message_start("user", "one"), 7), Some(7));
        assert_eq!(steers.row(&message_start("user", "two"), 7), Some(7));
        assert_eq!(steers.row(&message_start("user", "a TUI row"), 7), None);
        let end = json!({ "type": "message_end", "message": { "role": "user" } });
        assert_eq!(steers.row(&end, 7), None, "only a row's start counts");
    }

    /// U3: a message the worker refused frees its `turnStart`; a row of another turn opens none.
    #[test]
    fn a_refused_message_frees_its_turn_start() {
        let mut steers = Steers::default();
        steers.open(3);
        let _ = steers.row(&message_start("assistant", ""), 3);
        steers.sent();
        steers.refused(3);
        assert_eq!(steers.row(&message_start("user", "a TUI row"), 3), None);
        steers.sent();
        assert_eq!(
            steers.row(&message_start("user", "a later turn's row"), 4),
            None,
            "not the active turn"
        );
        assert_eq!(
            steers.row(&message_start("user", "the message"), 3),
            Some(3)
        );
    }

    /// U4: refusals (`-32603`): a stale turn, a turn over, a turn the agent started (D6), no
    /// prompt turn; and `turnStart` without `rewindTo` (D3).
    #[test]
    fn the_gate_refuses_what_cannot_join_the_turn() {
        let mut steers = Steers::default();
        assert_eq!(
            steers.gate(Some(1), 1, false),
            Gate::Refuse,
            "no prompt turn"
        );
        steers.open(5);
        let _ = steers.row(&message_start("assistant", ""), 5);
        assert_eq!(steers.gate(Some(5), 5, false), Gate::Send);
        assert_eq!(steers.gate(Some(4), 5, false), Gate::Refuse, "stale turn");
        assert_eq!(steers.gate(None, 5, false), Gate::Refuse, "no turn given");
        assert_eq!(steers.gate(Some(5), 0, false), Gate::Refuse, "turn over");
        assert_eq!(steers.gate(Some(5), 5, true), Gate::Refuse, "D6");
        assert_eq!(
            serde_json::to_value(steer_start()).unwrap(),
            json!({ "origin": "steer" }),
            "no rewindTo (D3)"
        );
    }
}
