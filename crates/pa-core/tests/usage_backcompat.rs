//! yusAgent (T04, YA-13): the optional `reasoning` and `cacheWrite1h` usage fields keep session
//! files compatible both ways. A session written before them reads back with both absent, and a
//! usage without them serializes exactly as before.

use std::path::Path;

use pa_core::session::manager::SessionManager;
use pa_types::session::AgentMessage;

/// One assistant row as written before T04: no `reasoning`, no `cacheWrite1h`.
const OLD_USAGE: &str = r#"{"input":120,"output":8,"cacheRead":900,"cacheWrite":40,"totalTokens":1068,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}"#;

fn session_file(usage: &str) -> String {
    [
        r#"{"type":"session","version":3,"id":"s-old","timestamp":"2026-10-01T00:00:00.000Z","cwd":"/w"}"#.to_string(),
        r#"{"type":"message","id":"u1","parentId":null,"timestamp":"2026-10-01T00:00:01.000Z","message":{"role":"user","content":"hi","timestamp":0}}"#.to_string(),
        format!(
            r#"{{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-10-01T00:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"ok"}}],"api":"anthropic-messages","provider":"anthropic","model":"m","usage":{usage},"stopReason":"stop","timestamp":0}}}}"#
        ),
    ]
    .join("\n")
        + "\n"
}

fn read_back_usage(content: &str) -> pa_types::ai::Usage {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s-old.jsonl");
    std::fs::write(&path, content).unwrap();
    let manager = SessionManager::open(Path::new("/w"), dir.path(), &path);
    let messages = manager.active_context().messages;
    assert_eq!(messages.len(), 2, "both rows read back");
    match &messages[1] {
        AgentMessage::Assistant(assistant) => assistant.usage,
        other => panic!("expected the assistant row, got {other:?}"),
    }
}

#[test]
fn a_session_written_before_the_fields_reads_back() {
    let usage = read_back_usage(&session_file(OLD_USAGE));
    assert_eq!(usage.input, 120);
    assert_eq!(usage.cache_read, 900);
    assert_eq!(usage.cache_write, 40);
    assert_eq!(usage.reasoning, None);
    assert_eq!(usage.cache_write_1h, None);
}

#[test]
fn a_session_with_the_fields_reads_them() {
    let usage = OLD_USAGE.replacen(r#""cost""#, r#""reasoning":5,"cacheWrite1h":40,"cost""#, 1);
    let usage = read_back_usage(&session_file(&usage));
    assert_eq!(usage.reasoning, Some(5));
    assert_eq!(usage.cache_write_1h, Some(40));
}

/// Field names of a serialized usage, in order.
fn keys(value: &serde_json::Value) -> Vec<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn absent_fields_serialize_as_before() {
    let old: serde_json::Value = serde_json::from_str(OLD_USAGE).unwrap();
    let usage: pa_types::ai::Usage = serde_json::from_str(OLD_USAGE).unwrap();
    assert_eq!(keys(&serde_json::to_value(usage).unwrap()), keys(&old));
    let agent: pa_agent::types::Usage = serde_json::from_str(OLD_USAGE).unwrap();
    assert_eq!(agent.reasoning, None);
    assert_eq!(keys(&serde_json::to_value(&agent).unwrap()), keys(&old));
}

/// The engine crosses the pa-ai / pa-agent boundary by JSON round trip
/// (`provider_adapter::json_round_trip`): the fields must survive it.
#[test]
fn the_fields_cross_the_crate_boundary() {
    let usage = pa_types::ai::Usage {
        reasoning: Some(7),
        cache_write_1h: Some(3),
        ..pa_types::ai::Usage::default()
    };
    let agent: pa_agent::types::Usage =
        serde_json::from_value(serde_json::to_value(usage).unwrap()).unwrap();
    assert_eq!(agent.reasoning, Some(7));
    assert_eq!(agent.cache_write_1h, Some(3));
}
