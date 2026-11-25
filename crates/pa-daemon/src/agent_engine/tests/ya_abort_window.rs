//! yusAgent (YA-10, ticket 36): an abort armed right after the admission consult, before the
//! agent run registers, cancels the registered run instead of waiting for the provider's reply.
use super::*;

/// The provider holds its reply closed (60 s, a backstop only, never the measure). The probe
/// stands for the worker's cancel flag: it reads clear at the turn's first consult
/// (`turn/model.rs`) and at the admission future's head (`turn/run_once.rs`), and the second
/// one arms the abort the way the worker does (flag up, then `abort_in_flight_turn`, a no-op
/// while no run is registered). The turn must settle aborted without the held reply.
#[test]
fn abort_armed_after_the_admission_consult_cancels_the_registered_run() {
    let _faux = FAUX_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (engine, _engine_dir) = faux_engine_with_settings(
        &serde_json::json!({ "responses": [{ "text": "held reply", "delayMs": 60000 }] }),
        1,
    );
    let consults = std::sync::atomic::AtomicU32::new(0);
    let armed = std::sync::atomic::AtomicBool::new(false);
    let probe = || {
        if armed.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
        // Consult 1: `turn/model.rs`; consult 2: the admission future's head.
        if consults.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 == 2 {
            armed.store(true, std::sync::atomic::Ordering::SeqCst);
            engine.abort_in_flight_turn();
        }
        false
    };
    let mut events: Vec<EngineEvent> = Vec::new();
    engine.run_prompt(
        0,
        PromptRequest {
            batch: Vec::new(),
            images: Vec::new(),
            message: "a turn whose abort lands before its run registers".to_string(),
            source: "user".to_string(),
            agent_message_id: None,
            custom_message: None,
        },
        &probe,
        &mut |event| {
            events.push(event);
            true
        },
    );
    assert!(
        armed.load(std::sync::atomic::Ordering::SeqCst),
        "the abort was armed at the admission consult: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EngineEvent::DoneAborted)),
        "the turn settles aborted once its run registers: {events:?}"
    );
    assert!(
        events.iter().all(|event| !matches!(
            event,
            EngineEvent::AssistantMessage(message)
                if message["content"]
                    == serde_json::json!([{ "type": "text", "text": "held reply" }])
        )),
        "the held reply never reached the turn: {events:?}"
    );
}
