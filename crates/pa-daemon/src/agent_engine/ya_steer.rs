//! yusAgent (YA-19, ticket 39): a message into the running agent run (path B,
//! `docs/tickets/39-message-pendant-un-tour.md`, §17 and §18).
//!
//! The worker passes a message sent into the running turn to the live agent's own steering
//! queue (`Agent::steer`), through `turn_agent`: never the core session's mutex, which a running
//! turn holds (`agent_engine.rs`, `turn_agent`). The loop takes it after the tool results,
//! before its next model call, in the same run. The run's settle drops what it did not take.
//!
//! The worker writes the session file from the engine's events, and the engine forwards a loop
//! user row of a delivery's first run only past its `AgentEnd` (`turn/run_once.rs`,
//! `boundary_passed`, which keeps the prompt's row from being written twice). `is_steered` lets
//! the rows of these messages through, each recognized once. No collision:
//! - two messages never share a key: each takes a timestamp strictly above the last one taken
//!   in the process (one worker process per session, `worker/env.rs`), whatever their text;
//! - a prompt's row never matches: the bridge sends a message only once the turn's first
//!   assistant message went by, and the loop emits the run's admitted rows before it, so they
//!   are past before any message is registered;
//! - the settle drops the registered messages with the agent's queue: none survives its run.
//!
//! Ticket 39, push 1 of path B: the signatures only; the behavior comes with the next push.
#![allow(dead_code)] // Ticket 39, push 1: the skeleton.

use pa_agent::types::AgentMessage;

use super::AgentSessionEngine;

/// The message into the live agent's queue; `false` when no agent takes it.
pub(super) fn steer(engine: &AgentSessionEngine, text: &str) -> bool {
    let _ = (engine, text);
    false
}

/// The run settled: the messages it did not take leave the agent's queue and the registry.
pub(super) fn drop_steers(engine: &AgentSessionEngine) {
    let _ = engine;
}

/// The row of a message passed to the agent's queue, recognized once.
pub(super) fn is_steered(message: &AgentMessage) -> bool {
    let _ = message;
    false
}

/// The registry's key of a new message: a timestamp strictly above the last one taken.
fn next_timestamp(now: i64) -> i64 {
    let _ = now;
    0
}

/// The message as the loop takes it, registered.
fn register(text: &str, now: i64) -> AgentMessage {
    let _ = now;
    AgentMessage::user(text)
}

/// The registered messages leave the registry.
fn forget_all() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str, timestamp: i64) -> AgentMessage {
        AgentMessage::Standard(pa_agent::types::Message::User(
            pa_agent::types::UserMessage {
                content: pa_agent::types::UserContent::Parts(vec![
                    pa_agent::types::UserPart::Text(pa_agent::types::TextContent {
                        text: text.to_string(),
                        text_signature: None,
                    }),
                ]),
                timestamp,
            },
        ))
    }

    fn timestamp_of(message: &AgentMessage) -> i64 {
        match message {
            AgentMessage::Standard(pa_agent::types::Message::User(user)) => user.timestamp,
            other => panic!("a user message: {other:?}"),
        }
    }

    /// U5: a registered message's row passes once; a row of the same text at another timestamp
    /// (a prompt's), or of another text, does not; two messages taken in the same millisecond
    /// get two timestamps; the settle forgets them all.
    #[test]
    fn steered_rows_are_recognized_once_and_only_theirs() {
        let _registry = REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        forget_all();
        let one = register("same text", 1_000);
        let two = register("same text", 1_000);
        let (t1, t2) = (timestamp_of(&one), timestamp_of(&two));
        assert!(t1 >= 1_000 && t2 > t1, "strictly increasing: {t1}, {t2}");
        assert!(!is_steered(&user("same text", t1 - 1)), "a prompt's row");
        assert!(!is_steered(&user("other text", t1)), "another text");
        assert!(is_steered(&one), "the first message's row");
        assert!(!is_steered(&one), "recognized once");
        forget_all();
        assert!(!is_steered(&two), "the settle forgot it");
    }

    /// The registry is process-wide: its tests run one at a time.
    static REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
