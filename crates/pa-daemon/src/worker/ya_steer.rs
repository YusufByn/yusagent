//! yusAgent (YA-19, ticket 39): a message into the running run (`_yus/session/steer`, path B,
//! `docs/tickets/39-message-pendant-un-tour.md`, §17 and §18).
//!
//! The bridge sends it as a `Steer` marked `yaInRun` (the supervisor forwards the field:
//! `crates/pa-daemon/src/supervisor/clients.rs:36-43`). Under the core lock, the worker checks
//! that a run is in flight and passes the message to the running agent's own queue
//! (`SessionEngine::ya_steer`); the run's settle drops what it did not take
//! (`SessionEngine::ya_drop_steers`, `turn.rs`) under the same lock, before the worker goes idle.
//! The message never enters the `steering` lane: it cannot outlive its run, nor cut it.

use serde_json::{json, Value};

use super::Worker;
use crate::protocol::{response_failure, response_success, DaemonResponse};

/// The `Steer` field that sends the message into the running run.
pub(crate) const IN_RUN: &str = "yaInRun";

/// The refusal when no run takes the message; the bridge answers it with `-32603`.
pub(crate) const OVER: &str = "The turn is over: send it as a prompt.";

/// A `Steer` for the running run.
pub(super) fn wanted(payload: &Value) -> bool {
    payload.get(IN_RUN).and_then(Value::as_bool) == Some(true)
}

impl Worker {
    /// The message into the running run, or a refusal. The core lock is held while the engine
    /// takes it: the settle drops the run's untaken messages under the same lock, before the
    /// worker goes idle, so a message never outlives the run it joined.
    pub(super) fn ya_steer(&self, payload: &Value) -> DaemonResponse {
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if message.is_empty() {
            return response_failure(None, "steer", "Steer cannot be empty", None);
        }
        let core = self.core.lock().unwrap();
        let running = core.busy
            && !core.abort_requested
            && !core.turn_cancelled
            && !core.compacting
            && !core.shutdown_requested;
        if !running || !self.engine.ya_steer(message) {
            return response_failure(None, "steer", OVER, None);
        }
        drop(core);
        response_success(None, "steer", Some(json!({ "inRun": true })))
    }
}
