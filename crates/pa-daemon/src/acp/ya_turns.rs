//! yusAgent (YA-10, ticket 25): the start and the end of every turn on the ACP bridge.
//!
//! Unharnessed (`157dcdc1d`) publishes `turnStart` first in each turn (`origin: "prompt"` or
//! `"wake"`, with the wake's kind) and ends a wake turn like a prompt turn: the response
//! boundary, then the completion envelope, with no JSON-RPC response
//! (`crates/uh-daemon/src/acp/prompt.rs:217-245`, `:332-357`, `settle_turn` `:857`). Here the
//! daemon worker runs the wakes from its own queue (a `[bash-done]` notice, a heartbeat, a quota
//! resume, a goal continuation, a prompt of an attached TUI): the bridge sees them as a worker
//! `agent_start` outside its own prompt, takes a turn id from the prompt counter, and announces the
//! turn at its first message, which tells its origin (`docs/tickets/25-tours-lances-par-agent.md`,
//! E1, E2, E4). A prompt of this client waits for such a turn to end before it takes its id
//! (T3, E7). Stop during such a turn stops only that turn (`abort_and_send_queued`, T4).
//!
//! The prompt waits for the turn's end (`agent_end`), not for its completion envelope: the
//! envelope's status read waits for an idle worker with an empty queue (`wait_until_idle`,
//! `crates/pa-daemon/src/worker/lifecycle.rs:638-654`), and a queue parked by an abort of another
//! client only resumes at the next prompt. The client ends the turn on its boundary.

use std::sync::Arc;

use pa_types::daemon::DaemonCommand;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

use super::daemon::{DaemonAcpState, DaemonLink};
use super::meta::{self, PrimeAgentEventPhase, PrimeAgentOutcome, PrimeAgentSessionMeta};
use super::producer::UpdateProducer;
use super::types::AcpSessionUpdate;

// yusAgent (YA-19, ticket 39): `_yus/session/steer`, a message into the running prompt turn.
pub(super) mod steer;
// yusAgent (YA-18, ticket 43): `_yus/session/rewind`, back to before a prompt.
pub(super) mod rewind;
// yusAgent (YA-18, ticket 43, PR 3a): workspace snapshots, their preview and their restore.
pub(super) mod checkpoints;

/// How often a prompt of this client checks whether a turn it did not start ended (the
/// Unharnessed admission retry, `prompt.rs:38-61`).
const PROMPT_WAIT_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// `turnStart` in the bridge `_meta`, with Unharnessed's field names (decision of 21/11/2026, T1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartMeta {
    /// `prompt` (a prompt of this client, or of another client of the session) or `wake`.
    pub origin: String,
    /// The wake's kind: `bash`, `quota`, `goal`, `heartbeat`; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<String>,
    /// Unharnessed's rewind point: the session's leaf before a prompt of this client (ticket 43).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewind_to: Option<String>,
}

impl TurnStartMeta {
    pub(crate) fn new(origin: &str, wake: Option<&str>) -> Self {
        TurnStartMeta {
            origin: origin.to_string(),
            wake: wake.map(str::to_string),
            rewind_to: None,
        }
    }
}

/// The turns of one hosted session that the bridge did not start.
#[derive(Debug, Default)]
pub(crate) struct AgentTurns {
    /// The worker turn running outside a prompt of this client.
    running: Option<AgentTurn>,
    /// yusAgent (YA-19, ticket 39): the messages sent into the running prompt turn.
    steers: steer::Steers,
}

#[derive(Debug)]
struct AgentTurn {
    turn_id: u64,
    /// `turnStart` is published: at the turn's first message, or before its first update.
    announced: bool,
    /// Stop reached this turn: it ends with no boundary and no envelope.
    stopped: bool,
}

/// The completion envelope of an ended turn, published off the event loop (it reads the daemon).
pub(crate) struct Settle {
    turn_id: u64,
    producer: Arc<UpdateProducer>,
    daemon_session_id: String,
}

impl AgentTurns {
    /// A prompt of this client must wait: a turn it did not start runs.
    pub(crate) fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Before the event's turn id is read: a worker `agent_start` outside a prompt of this client
    /// opens a turn with its own id (same counter as the prompts). The caller holds the session
    /// state lock, as a prompt does when it takes its id, so the two never interleave.
    pub(crate) async fn open(
        &mut self,
        event: &Value,
        producer: &UpdateProducer,
        assistant_stop_reason: &mut Option<pa_types::ai::StopReason>,
    ) {
        if event_type(event) != Some("agent_start") || self.running.is_some() {
            return;
        }
        if producer.active_prompt_turn().await != 0 {
            // A turn the worker runs inside a prompt of this client belongs to that prompt.
            return;
        }
        let turn_id = producer.begin_prompt().await;
        // The outcome reads this turn's own last assistant stop reason.
        *assistant_stop_reason = None;
        self.running = Some(AgentTurn {
            turn_id,
            announced: false,
            stopped: false,
        });
    }

    /// Before the event's updates are published: `turnStart` comes first in the turn. The turn's
    /// first message tells its origin; any other update before it gets a wake of unknown kind.
    pub(crate) async fn announce(
        &mut self,
        event: &Value,
        has_updates: bool,
        producer: &UpdateProducer,
    ) {
        // yusAgent (YA-19, ticket 39): the row of a message sent into the prompt turn.
        let active = producer.active_prompt_turn().await;
        if let Some(turn) = self.steers.row(event, active) {
            publish_turn_start(producer, turn, steer::steer_start()).await;
        }
        let Some(turn) = self.running.as_mut().filter(|turn| !turn.announced) else {
            return;
        };
        let kind = event_type(event);
        let start = if kind == Some("message_start") {
            origin_of(event.get("message").unwrap_or(&Value::Null))
        } else if has_updates || kind == Some("agent_end") {
            TurnStartMeta::new("wake", None)
        } else {
            return;
        };
        turn.announced = true;
        publish_turn_start(producer, turn.turn_id, start).await;
    }

    /// After the event's updates are published: the worker `agent_end` ends the turn. A completed
    /// turn publishes its response boundary here, in event order, and returns its envelope to
    /// publish off the loop; a failed one publishes the error boundary only (like a failed
    /// prompt); a stopped one publishes nothing (Unharnessed `settle_turn`, cancel path). The id
    /// is released at once: the next turn may start before the envelope is out.
    pub(crate) async fn close(
        &mut self,
        event: &Value,
        producer: &Arc<UpdateProducer>,
        assistant_stop_reason: Option<pa_types::ai::StopReason>,
        daemon_session_id: &str,
    ) -> Option<Settle> {
        if event_type(event) != Some("agent_end") {
            return None;
        }
        // yusAgent (YA-18, ticket 43, PR 3a): the workspace at the end of any turn, queued.
        checkpoints::end(daemon_session_id);
        let turn = self.running.take()?;
        if turn.stopped {
            producer.finish_prompt(turn.turn_id).await;
            return None;
        }
        let failed = assistant_stop_reason == Some(pa_types::ai::StopReason::Error);
        let boundary = AcpSessionUpdate::SessionInfoUpdate {
            meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                terminal_quiescence_expected: Some(!failed),
                ..Default::default()
            }),
        };
        let outcome = if failed {
            PrimeAgentOutcome::Error
        } else {
            PrimeAgentOutcome::Result
        };
        let _ = producer
            .publish(
                &boundary,
                turn.turn_id,
                PrimeAgentEventPhase::ResponseBoundary,
                Some(outcome),
            )
            .await;
        producer.finish_prompt(turn.turn_id).await;
        if failed {
            return None;
        }
        Some(Settle {
            turn_id: turn.turn_id,
            producer: Arc::clone(producer),
            daemon_session_id: daemon_session_id.to_string(),
        })
    }

    /// yusAgent (YA-10, ticket 28, C9): the turn the worker was running when `session/load`
    /// replayed the session goes on live under the replay's open turn id, already announced; its
    /// `agent_end` closes it like any turn the bridge did not start.
    pub(crate) fn adopt(&mut self, turn_id: u64) {
        self.running = Some(AgentTurn {
            turn_id,
            announced: true,
            stopped: false,
        });
    }

    /// Stop during a turn this client did not start: only that turn stops (T4). Returns whether
    /// a stop is due.
    pub(crate) fn arm_stop(&mut self) -> bool {
        match self.running.as_mut() {
            Some(turn) if !turn.stopped => {
                turn.stopped = true;
                true
            }
            _ => false,
        }
    }
}

/// Publish `turnStart` at the start of a turn (a prompt of this client: at its admission).
pub(crate) async fn publish_turn_start(
    producer: &UpdateProducer,
    turn_id: u64,
    start: TurnStartMeta,
) {
    let _ = producer
        .publish(
            &AcpSessionUpdate::SessionInfoUpdate {
                meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                    turn_start: Some(start),
                    ..Default::default()
                }),
            },
            turn_id,
            PrimeAgentEventPhase::Event,
            None,
        )
        .await;
}

/// `turnStart` of a prompt of this client.
pub(crate) fn prompt_start() -> TurnStartMeta {
    TurnStartMeta::new("prompt", None)
}

/// yusAgent (YA-18, ticket 43): publishes the `turnStart` of a prompt of this client, with
/// `rewindTo`, the session's leaf before the prompt (Unharnessed's `prompt.rs:320-350`), read by
/// the worker's action `leaf`. Without the leaf (a worker that does not serve it, a failed read),
/// the turn starts without `rewindTo`: its message cannot be changed. Then (PR 3a) the snapshot of
/// the workspace before the prompt, waited for.
pub(crate) async fn publish_prompt_start(
    producer: &UpdateProducer,
    turn_id: u64,
    link: &Arc<DaemonLink>,
    state: &Arc<Mutex<DaemonAcpState>>,
) {
    let daemon_session_id = state
        .lock()
        .await
        .session
        .as_ref()
        .map(|hosted| hosted.daemon_active_session_id.clone());
    let mut start = prompt_start();
    let context = match daemon_session_id.as_deref() {
        Some(daemon_session_id) => rewind::context(link, daemon_session_id).await,
        None => None,
    };
    start.rewind_to = context.as_ref().and_then(|context| context.leaf_id.clone());
    publish_turn_start(producer, turn_id, start).await;
    // yusAgent (YA-18, ticket 43, PR 3a): the workspace before the prompt, waited for: the prompt
    // goes to the worker after it, so its cells run after the snapshot.
    if let (Some(daemon_session_id), Some(context)) = (daemon_session_id, context) {
        checkpoints::start(&daemon_session_id, &context).await;
    }
}

/// A prompt of this client takes its turn id once no turn it did not start runs (T3, E7); the
/// check and the id are taken under the session state lock the event loop holds when it opens a
/// turn.
pub(crate) async fn begin_prompt(
    state: &Arc<Mutex<DaemonAcpState>>,
    producer: &UpdateProducer,
) -> u64 {
    loop {
        {
            let mut guard = state.lock().await;
            let busy = guard
                .session
                .as_ref()
                .is_some_and(|hosted| hosted.ya_turns.busy());
            if !busy {
                let turn = producer.begin_prompt().await;
                // yusAgent (YA-19, ticket 39): the messages this turn may take.
                if let Some(hosted) = guard.session.as_mut() {
                    hosted.ya_turns.steers.open(turn);
                }
                return turn;
            }
        }
        tokio::time::sleep(PROMPT_WAIT_POLL).await;
    }
}

/// The completion envelope of an ended turn, as after a prompt (`daemon.rs`, the quiescence
/// event then `terminalQuiescence`): one status read at the RLM-quiescence barrier and one roster
/// read (E5). A failed read publishes no envelope; the boundary already ended the turn.
pub(crate) async fn settle(settle: Settle, link: Arc<DaemonLink>) {
    let Settle {
        turn_id,
        producer,
        daemon_session_id,
    } = settle;
    let status = super::daemon::fetch_autonomous_status(&link, &daemon_session_id, true).await;
    let children = super::daemon::fetch_rlm_children(&link, &daemon_session_id).await;
    match (status, children) {
        (Ok(status), Ok(children)) => {
            let quiescence = super::daemon::quiescence_meta(Some(&status), &children);
            let autonomous = status.enabled.then(|| meta::autonomous_meta(&status));
            let completion = AcpSessionUpdate::SessionInfoUpdate {
                meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                    autonomous: autonomous.clone(),
                    quiescence: Some(quiescence.clone()),
                    ..Default::default()
                }),
            };
            let _ = producer
                .publish(&completion, turn_id, PrimeAgentEventPhase::Event, None)
                .await;
            let terminal = AcpSessionUpdate::SessionInfoUpdate {
                meta: meta::prime_agent_meta(&PrimeAgentSessionMeta {
                    autonomous,
                    quiescence: Some(quiescence),
                    ..Default::default()
                }),
            };
            let _ = producer
                .publish(
                    &terminal,
                    turn_id,
                    PrimeAgentEventPhase::TerminalQuiescence,
                    Some(PrimeAgentOutcome::Result),
                )
                .await;
        }
        (status, children) => {
            let error = status.err().or(children.err()).map(|e| e.to_string());
            eprintln!(
                "pa-daemon: acp agent turn {turn_id} envelope skipped: {}",
                error.unwrap_or_default()
            );
        }
    }
}

/// Stop the running turn only, keep and deliver the queued work (the TUI's Stop,
/// `crates/pa-tui/src/session_ui/apply.rs:27-33`). Measured on `1b96bb645`: the queued
/// `[bash-done]` notice runs as the next wake, every later request keeps the prefix
/// (`docs/tickets/25-tours-lances-par-agent.md`, section 6).
pub(crate) async fn stop(link: &DaemonLink, daemon_session_id: &str) {
    let stopped = link
        .request_ok(DaemonCommand::AbortAndSendQueued {
            id: None,
            active_session_id: daemon_session_id.to_string(),
            rest: Map::default(),
        })
        .await;
    if let Err(error) = stopped {
        eprintln!("pa-daemon: acp agent turn stop failed: {error:#}");
    }
}

fn event_type(event: &Value) -> Option<&str> {
    event.get("type").and_then(Value::as_str)
}

/// The origin of a turn from its first message (E2, E4): a `[bash-done]` notice, a heartbeat, a
/// goal continuation and a quota resume are wakes; any other user message is a prompt of another
/// client of the session (an attached TUI, a scheduled prompt).
pub(crate) fn origin_of(message: &Value) -> TurnStartMeta {
    use pa_core::session_engine::messages::{
        ASYNC_BASH_COMPLETION_CUSTOM_TYPE, HEARTBEAT_PROMPT_CUSTOM_TYPE,
    };
    match message.get("role").and_then(Value::as_str) {
        Some("custom") => {
            let kind = match message.get("customType").and_then(Value::as_str) {
                Some(ASYNC_BASH_COMPLETION_CUSTOM_TYPE) => Some("bash"),
                Some(HEARTBEAT_PROMPT_CUSTOM_TYPE) => Some("heartbeat"),
                Some(pa_core::goals::GOAL_CONTEXT_CUSTOM_TYPE) => Some("goal"),
                _ => None,
            };
            TurnStartMeta::new("wake", kind)
        }
        Some("user") if user_text(message).contains(QUOTA_RESUME_TAG) => {
            TurnStartMeta::new("wake", Some("quota"))
        }
        Some("user") => TurnStartMeta::new("prompt", None),
        _ => TurnStartMeta::new("wake", None),
    }
}

/// The opening tag of `QUOTA_RESUME_MARKER_TEXT` (`pa_core::session_engine::provider_park`).
const QUOTA_RESUME_TAG: &str = "<provider_quota_resumed>";

fn user_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quota_tag_matches_the_marker() {
        assert!(
            pa_core::session_engine::provider_park::QUOTA_RESUME_MARKER_TEXT
                .starts_with(QUOTA_RESUME_TAG)
        );
    }

    #[test]
    fn the_first_message_tells_the_origin() {
        let custom = |kind: &str| json!({ "role": "custom", "customType": kind, "content": "x" });
        assert_eq!(
            origin_of(&custom("async_bash_completion")),
            TurnStartMeta::new("wake", Some("bash"))
        );
        assert_eq!(
            origin_of(&custom("heartbeat_prompt")),
            TurnStartMeta::new("wake", Some("heartbeat"))
        );
        assert_eq!(
            origin_of(&custom("goal_context")),
            TurnStartMeta::new("wake", Some("goal"))
        );
        assert_eq!(
            origin_of(&custom("agent_message")),
            TurnStartMeta::new("wake", None)
        );
        let quota = json!({ "role": "user", "content": [{ "type": "text",
            "text": pa_core::session_engine::provider_park::QUOTA_RESUME_MARKER_TEXT }] });
        assert_eq!(origin_of(&quota), TurnStartMeta::new("wake", Some("quota")));
        let tui = json!({ "role": "user", "content": "bonjour" });
        assert_eq!(origin_of(&tui), TurnStartMeta::new("prompt", None));
    }

    #[test]
    fn turn_start_serializes_with_its_wire_names() {
        let wrapped = meta::prime_agent_meta(&PrimeAgentSessionMeta {
            turn_start: Some(TurnStartMeta::new("wake", Some("bash"))),
            ..Default::default()
        });
        assert_eq!(
            wrapped,
            json!({ "ai.primeintellect.prime-agent": {
                "turnStart": { "origin": "wake", "wake": "bash" } } })
        );
    }

    fn producer() -> (
        Arc<UpdateProducer>,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
    ) {
        let (sink, frames) = tokio::sync::mpsc::unbounded_channel();
        (UpdateProducer::new("acp-1", sink), frames)
    }

    fn drain(frames: &mut tokio::sync::mpsc::UnboundedReceiver<Value>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(frame) = frames.try_recv() {
            out.push(frame["params"]["update"]["_meta"]["ai.primeintellect.prime-agent"].clone());
        }
        out
    }

    #[tokio::test]
    async fn a_wake_takes_its_own_id_and_ends_on_its_boundary() {
        let (producer, mut frames) = producer();
        producer.commit_session_new_response().await;
        let mut turns = AgentTurns::default();
        let mut stop = Some(pa_types::ai::StopReason::Stop);
        let start = json!({ "type": "agent_start" });
        let first = json!({ "type": "message_start", "message":
            { "role": "custom", "customType": "async_bash_completion", "content": "[bash-done]" } });
        let end = json!({ "type": "agent_end", "messages": [] });
        for wake in 1..=2u64 {
            turns.open(&start, &producer, &mut stop).await;
            assert_eq!(stop, None, "a wake reads its own stop reason");
            assert!(turns.busy());
            assert_eq!(producer.active_prompt_turn().await, wake);
            turns.announce(&start, false, &producer).await;
            turns.announce(&first, false, &producer).await;
            let settle = turns.close(&end, &producer, None, "daemon-1").await;
            assert!(settle.is_some());
            assert_eq!(producer.active_prompt_turn().await, 0);
            let published = drain(&mut frames);
            assert_eq!(published.len(), 2);
            assert_eq!(published[0]["promptTurnId"], wake);
            assert_eq!(
                published[0]["turnStart"],
                json!({ "origin": "wake", "wake": "bash" })
            );
            assert_eq!(published[1]["promptTurnId"], wake);
            assert_eq!(published[1]["phase"], "responseBoundary");
            assert_eq!(published[1]["outcome"], "result");
            assert!(!turns.busy(), "the envelope does not hold a prompt");
        }
        assert!(!turns.busy());
    }

    #[tokio::test]
    async fn a_turn_inside_a_prompt_of_this_client_is_not_a_wake() {
        let (producer, _frames) = producer();
        let mut turns = AgentTurns::default();
        let prompt = producer.begin_prompt().await;
        turns
            .open(&json!({ "type": "agent_start" }), &producer, &mut None)
            .await;
        assert!(!turns.busy());
        assert_eq!(producer.active_prompt_turn().await, prompt);
    }

    #[tokio::test]
    async fn a_stopped_wake_ends_with_no_frame_and_a_failed_one_with_the_error_boundary() {
        let (producer, mut frames) = producer();
        producer.commit_session_new_response().await;
        let mut turns = AgentTurns::default();
        let start = json!({ "type": "agent_start" });
        let end = json!({ "type": "agent_end" });
        turns.open(&start, &producer, &mut None).await;
        turns.announce(&end, false, &producer).await;
        assert!(turns.arm_stop());
        assert!(!turns.arm_stop(), "one stop per turn");
        assert!(turns.close(&end, &producer, None, "d").await.is_none());
        let published = drain(&mut frames);
        assert_eq!(published.len(), 1, "turnStart only: {published:?}");
        assert_eq!(published[0]["turnStart"], json!({ "origin": "wake" }));
        assert!(!turns.busy());
        assert!(!turns.arm_stop(), "no turn, no stop");

        turns.open(&start, &producer, &mut None).await;
        turns
            .announce(&json!({ "type": "message_update" }), true, &producer)
            .await;
        let failed = turns
            .close(&end, &producer, Some(pa_types::ai::StopReason::Error), "d")
            .await;
        assert!(failed.is_none());
        let published = drain(&mut frames);
        assert_eq!(published[1]["phase"], "responseBoundary");
        assert_eq!(published[1]["outcome"], "error");
        assert_eq!(published[1]["terminalQuiescenceExpected"], false);
        assert!(!turns.busy());
    }
}
