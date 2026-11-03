//! The usage journal: line shape, cache rate, turns, TTL delay, restore after a restart, the
//! history-reasoning scan, and the stream-fn wrapper end to end on a scripted stream.

use super::*;
use pa_agent::stream::{event_stream, LlmContext, StreamRequestOptions};
use pa_agent::types::{Message, StopReason, UsageCost, UserContent, UserMessage};

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn record(kind: CallKind, started_at: SystemTime, user_timestamp: Option<i64>) -> CallRecord {
    CallRecord {
        kind,
        provider: "anthropic".to_string(),
        api: "anthropic-messages".to_string(),
        model: "claude-test".to_string(),
        effort: Some("medium".to_string()),
        started_at,
        ttft_ms: Some(120.04),
        duration_ms: 950.0,
        stop_reason: "toolUse".to_string(),
        usage: UsageCounts {
            input: 100,
            output: 50,
            cache_read: 800,
            cache_write: 100,
            cache_write_1h: None,
            reasoning: None,
            total_tokens: 1050,
            cost: 0.0125,
        },
        history_reasoning: None,
        user_timestamp,
    }
}

fn journal(dir: &Path, parent: Option<&str>) -> UsageJournal {
    UsageJournal::open(
        dir.join("s1.usage.ndjson"),
        "s1".to_string(),
        parent.map(str::to_string),
    )
}

#[test]
fn a_line_carries_every_field_and_the_cache_rate() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    let mut call = record(CallKind::Main, SystemTime::now(), Some(1));
    call.usage.cache_write_1h = Some(40);
    call.usage.reasoning = Some(12);
    call.history_reasoning = Some(HistoryReasoning {
        blocks: 2,
        text_chars: 10,
        opaque_bytes: 7,
    });
    journal.append(&call);
    let entries = lines(journal.path());
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["v"], json!(USAGE_LOG_VERSION));
    assert_eq!(entry["sessionId"], json!("s1"));
    assert_eq!(entry["parentSessionId"], Value::Null);
    assert_eq!(entry["seq"], json!(1));
    assert_eq!(entry["turn"], json!(1));
    assert_eq!(entry["kind"], json!("main"));
    assert_eq!(entry["provider"], json!("anthropic"));
    assert_eq!(entry["model"], json!("claude-test"));
    assert_eq!(entry["effort"], json!("medium"));
    assert_eq!(entry["input"], json!(100));
    assert_eq!(entry["output"], json!(50));
    assert_eq!(entry["cacheRead"], json!(800));
    assert_eq!(entry["cacheWrite"], json!(100));
    assert_eq!(entry["cacheWrite5m"], json!(60));
    assert_eq!(entry["cacheWrite1h"], json!(40));
    assert_eq!(entry["reasoning"], json!(12));
    // 800 / (100 + 800 + 100)
    assert_eq!(entry["cacheRate"], json!(0.8));
    assert_eq!(entry["cost"], json!(0.0125));
    assert_eq!(entry["ttftMs"], json!(120.0));
    assert_eq!(entry["durationMs"], json!(950.0));
    assert_eq!(entry["sincePrevMs"], Value::Null);
    assert_eq!(
        entry["historyReasoning"],
        json!({"blocks": 2, "textChars": 10, "textTokensEst": 3, "opaqueBytes": 7})
    );
}

#[test]
fn unreported_fields_are_absent_never_zero() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    journal.append(&record(CallKind::Main, SystemTime::now(), None));
    let entry = &lines(journal.path())[0];
    for field in [
        "reasoning",
        "cacheWrite5m",
        "cacheWrite1h",
        "historyReasoning",
        "purpose",
    ] {
        assert!(entry.get(field).is_none(), "{field} must be absent");
    }
}

#[test]
fn a_line_is_never_read_as_a_session_header() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    journal.append(&record(CallKind::Main, SystemTime::now(), Some(1)));
    assert!(read_session_header(journal.path()).is_none());
}

#[test]
fn turns_follow_the_last_user_message_and_the_delay_is_per_kind() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    let t0 = UNIX_EPOCH + std::time::Duration::from_hours(500_000);
    let at = |ms: u64| t0 + std::time::Duration::from_millis(ms);
    journal.append(&record(CallKind::Main, at(0), Some(10)));
    // A tool-result round trip: same user message, same turn.
    journal.append(&record(CallKind::Main, at(4_000), Some(10)));
    journal.append(&record(CallKind::Compaction, at(5_000), None));
    journal.append(&record(CallKind::Main, at(65_000), Some(20)));
    journal.append(&record(CallKind::Other("side_question"), at(66_000), None));
    let entries = lines(journal.path());
    let column = |name: &str| entries.iter().map(|e| e[name].clone()).collect::<Vec<_>>();
    assert_eq!(column("seq"), [1, 2, 3, 4, 5].map(|n| json!(n)));
    assert_eq!(column("turn"), [1, 1, 1, 2, 2].map(|n| json!(n)));
    assert_eq!(
        column("kind"),
        ["main", "main", "compaction", "main", "other"].map(|k| json!(k))
    );
    assert_eq!(
        column("sincePrevMs"),
        [
            Value::Null,
            json!(4_000),
            Value::Null,
            json!(61_000),
            Value::Null
        ]
    );
    assert_eq!(entries[4]["purpose"], json!("side_question"));
}

#[test]
fn a_child_session_journals_main_calls_as_subagent() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), Some("parent-1"));
    journal.append(&record(CallKind::Main, SystemTime::now(), Some(1)));
    journal.append(&record(CallKind::Compaction, SystemTime::now(), None));
    let entries = lines(journal.path());
    assert_eq!(entries[0]["kind"], json!("subagent"));
    assert_eq!(entries[0]["parentSessionId"], json!("parent-1"));
    assert_eq!(entries[1]["kind"], json!("compaction"));
}

#[test]
fn a_reopened_journal_continues_its_counters() {
    let dir = tempfile::tempdir().unwrap();
    let t0 = UNIX_EPOCH + std::time::Duration::from_hours(500_000);
    {
        let first = journal(dir.path(), None);
        first.append(&record(CallKind::Main, t0, Some(1)));
        first.append(&record(CallKind::Main, t0, Some(2)));
    }
    let reopened = journal(dir.path(), None);
    reopened.append(&record(
        CallKind::Main,
        t0 + std::time::Duration::from_mins(5),
        Some(3),
    ));
    let last = lines(reopened.path()).pop().unwrap();
    assert_eq!(last["seq"], json!(3));
    assert_eq!(last["turn"], json!(3));
    assert_eq!(last["sincePrevMs"], json!(300_000));
}

#[test]
fn the_journal_sits_next_to_the_session_file() {
    assert_eq!(
        usage_log_path(Path::new("/a/sessions/0190-abc.jsonl")),
        PathBuf::from("/a/sessions/0190-abc.usage.ndjson")
    );
    // Not `.jsonl`: session scans and tests take the first `.jsonl` of the directory for the
    // session file (upstream's `<stem>.info-cache.json` sidecar avoids it the same way).
    assert_ne!(
        usage_log_path(Path::new("/a/s.jsonl")).extension(),
        Path::new("x.jsonl").extension()
    );
}

#[test]
fn history_reasoning_reads_each_wire_format() {
    let anthropic = json!({"messages": [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "abcdef", "signature": "sig1"},
            {"type": "redacted_thinking", "data": "xyz"},
            {"type": "tool_use", "id": "t", "name": "python_repl", "input": {}}]}]});
    assert_eq!(
        HistoryReasoning::from_payload(&anthropic),
        HistoryReasoning {
            blocks: 2,
            text_chars: 6,
            opaque_bytes: 7
        }
    );
    let responses = json!({"input": [
        {"role": "user", "content": "hi"},
        {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "abcd"}],
         "encrypted_content": "enc12"}]});
    assert_eq!(
        HistoryReasoning::from_payload(&responses),
        HistoryReasoning {
            blocks: 1,
            text_chars: 4,
            opaque_bytes: 5
        }
    );
    let gemini = json!({"contents": [{"role": "model", "parts": [
        {"text": "abc", "thought": true},
        {"functionCall": {"name": "f"}, "thoughtSignature": "sg"}]}]});
    assert_eq!(
        HistoryReasoning::from_payload(&gemini),
        HistoryReasoning {
            blocks: 1,
            text_chars: 3,
            opaque_bytes: 2
        }
    );
    let completions = json!({"messages": [
        {"role": "assistant", "content": "ok", "reasoning_content": "abcde"}]});
    assert_eq!(
        HistoryReasoning::from_payload(&completions),
        HistoryReasoning {
            blocks: 1,
            text_chars: 5,
            opaque_bytes: 0
        }
    );
    assert_eq!(
        HistoryReasoning::from_payload(&json!({"messages": [{"role": "user", "content": "hi"}]})),
        HistoryReasoning::default()
    );
}

fn agent_model() -> pa_agent::types::Model {
    pa_agent::types::Model {
        id: "claude-test".to_string(),
        name: "Claude".to_string(),
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        base_url: "http://127.0.0.1".to_string(),
        reasoning: true,
        cost: UsageCost::default(),
        context_window: 200_000,
        max_tokens: 32_000,
        max_tokens_explicit: false,
    }
}

fn agent_message(usage: pa_agent::types::Usage) -> pa_agent::types::AssistantMessage {
    pa_agent::types::AssistantMessage {
        content: Vec::new(),
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-test".to_string(),
        response_model: None,
        response_id: None,
        diagnostics: None,
        usage,
        stop_reason: StopReason::ToolUse,
        stop_reason_raw: None,
        error_message: None,
        timestamp: 0,
    }
}

/// A provider that calls the payload hook with `payload`, then streams start, one text delta
/// and done with `usage`.
fn scripted(payload: Value, usage: pa_agent::types::Usage) -> StreamFn {
    Arc::new(move |_model, _context, options| {
        let payload = payload.clone();
        let usage = usage.clone();
        Box::pin(async move {
            if let Some(hook) = options.on_payload.as_ref() {
                assert!(hook(payload, &agent_model()).is_none(), "body untouched");
            }
            let (handle, consumer) = event_stream();
            let partial = agent_message(pa_agent::types::Usage::zero());
            handle.push(AssistantMessageEvent::Start {
                partial: partial.clone(),
            });
            handle.push(AssistantMessageEvent::TextDelta {
                content_index: 0,
                delta: "ok".to_string(),
                partial,
            });
            handle.push(AssistantMessageEvent::Done {
                reason: StopReason::ToolUse,
                message: agent_message(usage),
            });
            Ok(Box::new(consumer) as Box<dyn ModelStream>)
        })
    })
}

#[tokio::test]
async fn the_wrapper_journals_a_streamed_call() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(journal(dir.path(), None));
    let usage = pa_agent::types::Usage {
        input: 25,
        output: 348,
        cache_read: 900,
        cache_write: 75,
        total_tokens: 1348,
        cost: UsageCost::default(),
        reasoning: Some(312),
        cache_write_1h: None,
    };
    let payload = json!({"messages": [{"role": "assistant", "content": [
        {"type": "thinking", "thinking": "abcdefgh", "signature": "s"}]}]});
    let stream_fn = instrument_stream_fn(
        Some(Arc::clone(&journal)),
        CallKind::Main,
        scripted(payload, usage),
    );
    let context = LlmContext {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("go".to_string()),
            timestamp: 42,
        })],
        tools: Vec::new(),
    };
    let options = StreamRequestOptions {
        reasoning: pa_agent::types::ThinkingLevel::High,
        session_id: Some("s1".to_string()),
        ..Default::default()
    };
    let mut stream = stream_fn(agent_model(), context, options).await.unwrap();
    while stream.next_event().await.is_some() {}
    drop(stream);
    let entries = lines(journal.path());
    assert_eq!(entries.len(), 1, "one line per call, the drop adds none");
    let entry = &entries[0];
    assert_eq!(entry["kind"], json!("main"));
    assert_eq!(entry["effort"], json!("high"));
    assert_eq!(entry["stopReason"], json!("toolUse"));
    assert_eq!(entry["input"], json!(25));
    assert_eq!(entry["cacheRead"], json!(900));
    assert_eq!(entry["cacheWrite"], json!(75));
    assert_eq!(entry["reasoning"], json!(312));
    assert_eq!(entry["cacheRate"], json!(0.9));
    assert_eq!(entry["turn"], json!(1));
    assert!(entry["ttftMs"].is_number());
    assert_eq!(entry["historyReasoning"]["blocks"], json!(1));
    assert_eq!(entry["historyReasoning"]["textChars"], json!(8));
}

#[test]
fn without_a_journal_the_stream_fn_is_unchanged() {
    let inner = scripted(json!({}), pa_agent::types::Usage::zero());
    let wrapped = instrument_stream_fn(None, CallKind::Main, Arc::clone(&inner));
    assert!(Arc::ptr_eq(&inner, &wrapped));
}

#[tokio::test]
async fn a_dropped_stream_journals_aborted_with_the_last_usage() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(journal(dir.path(), None));
    let stream_fn = instrument_stream_fn(
        Some(Arc::clone(&journal)),
        CallKind::Main,
        scripted(json!({}), pa_agent::types::Usage::zero()),
    );
    let mut stream = stream_fn(
        agent_model(),
        LlmContext::default(),
        StreamRequestOptions::default(),
    )
    .await
    .unwrap();
    stream.next_event().await;
    drop(stream);
    let entries = lines(journal.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["stopReason"], json!("aborted"));
}

#[test]
fn a_registered_journal_is_found_by_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(UsageJournal::open(
        dir.path().join("reg.usage.ndjson"),
        "usage-log-registry-test".to_string(),
        None,
    ));
    register(&journal);
    assert!(lookup("usage-log-registry-test").is_some_and(|found| Arc::ptr_eq(&found, &journal)));
    drop(journal);
    assert!(lookup("usage-log-registry-test").is_none());
}
