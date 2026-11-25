//! yusAgent (YA-19, ticket 39): a message into the running run (`_yus/session/steer`, path B,
//! `docs/tickets/39-message-pendant-un-tour.md`, §17 and §18).
//!
//! The bridge sends it as a `Steer` marked `yaInRun` (the supervisor forwards the field:
//! `crates/pa-daemon/src/supervisor/clients.rs:36-43`). Under the core lock, the worker checks
//! that a run is in flight and passes the message to the running agent's own queue
//! (`SessionEngine::ya_steer`); the run's settle drops what it did not take
//! (`SessionEngine::ya_drop_steers`, `turn.rs`) under the same lock, before the worker goes idle.
//! The message never enters the `steering` lane: it cannot outlive its run, nor cut it.
//!
//! Ticket 39, push 1 of path B: the signatures only; the behavior comes with the next push.
#![allow(dead_code)] // Ticket 39, push 1: the skeleton.

use serde_json::Value;

use super::Worker;
use crate::protocol::{response_failure, DaemonResponse};

/// The `Steer` field that sends the message into the running run.
pub(crate) const IN_RUN: &str = "yaInRun";

/// The refusal when no run takes the message; the bridge answers it with `-32603`.
pub(crate) const OVER: &str = "The turn is over: send it as a prompt.";

/// A `Steer` for the running run.
pub(super) fn wanted(payload: &Value) -> bool {
    let _ = payload;
    false
}

impl Worker {
    /// The message into the running run, or a refusal.
    #[allow(clippy::unused_self)] // Ticket 39, push 1.
    pub(super) fn ya_steer(&self, payload: &Value) -> DaemonResponse {
        let _ = payload;
        response_failure(None, "steer", OVER, None)
    }
}
