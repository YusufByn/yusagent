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
        invalidation: None,
        provider_usage: None,
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
        "invalidation",
        "cacheWrite30m",
        "cacheWrite12h",
        "costWarning",
        "providerUsage",
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
    scripted_with_raw_usage(payload, usage, None)
}

/// [`scripted`], whose provider also reports `raw_usage` through the usage hook.
fn scripted_with_raw_usage(
    payload: Value,
    usage: pa_agent::types::Usage,
    raw_usage: Option<Value>,
) -> StreamFn {
    Arc::new(move |_model, _context, options| {
        let payload = payload.clone();
        let usage = usage.clone();
        let raw_usage = raw_usage.clone();
        Box::pin(async move {
            if let Some(hook) = options.on_payload.as_ref() {
                assert!(hook(payload, &agent_model()).is_none(), "body untouched");
            }
            if let (Some(hook), Some(raw_usage)) = (options.on_usage.as_ref(), raw_usage) {
                hook(&raw_usage, &agent_model());
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
    assert!(entry.get("providerUsage").is_none(), "no usage hook called");
}

// yusAgent (T08, YA-13): the raw usage the provider reports through the usage hook is
// journaled as `providerUsage`, unchanged, with the TTL split read from it and a cost warning
// for the 12 h share; an inner usage hook still runs.
#[tokio::test]
async fn the_raw_provider_usage_and_the_ttl_split_are_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(journal(dir.path(), None));
    let raw = json!({"input_tokens": 1000, "output_tokens": 20, "total_tokens": 1020,
        "input_tokens_details": {"cached_tokens": 600, "cache_write_tokens": 300,
            "cache_write_tokens_details": {"ttl_30m_tokens": 200, "ttl_12h_tokens": 100}},
        "output_tokens_details": {"reasoning_tokens": 5}});
    let usage = pa_agent::types::Usage {
        input: 100,
        output: 20,
        cache_read: 600,
        cache_write: 300,
        total_tokens: 1020,
        ..pa_agent::types::Usage::zero()
    };
    let stream_fn = instrument_stream_fn(
        Some(Arc::clone(&journal)),
        CallKind::Main,
        scripted_with_raw_usage(json!({}), usage, Some(raw.clone())),
    );
    let seen = Arc::new(Mutex::new(None));
    let inner_seen = Arc::clone(&seen);
    let options = StreamRequestOptions {
        on_usage: Some(Arc::new(
            move |usage: &Value, _model: &pa_agent::types::Model| {
                *lock(&inner_seen) = Some(usage.clone());
            },
        )),
        ..Default::default()
    };
    let mut stream = stream_fn(agent_model(), LlmContext::default(), options)
        .await
        .unwrap();
    while stream.next_event().await.is_some() {}
    drop(stream);
    let entry = &lines(journal.path())[0];
    assert_eq!(entry["providerUsage"], raw);
    assert_eq!(entry["cacheWrite"], json!(300));
    assert_eq!(entry["cacheWrite30m"], json!(200));
    assert_eq!(entry["cacheWrite12h"], json!(100));
    assert_eq!(entry["costWarning"], json!(CACHE_WRITE_12H_WARNING));
    assert!(entry.get("cacheWrite5m").is_none());
    assert!(entry.get("cacheWrite1h").is_none());
    assert_eq!(*lock(&seen), Some(raw), "the inner usage hook still runs");
}

// yusAgent (T08, YA-13): a split that does not add up to `cacheWrite` is written as received;
// no cost warning without 12 h writes.
#[test]
fn a_ttl_split_is_written_as_received() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    let mut call = record(CallKind::Main, SystemTime::now(), Some(1));
    call.provider_usage = Some(json!({"input_tokens_details": {"cache_write_tokens": 100,
        "cache_write_tokens_details": {"ttl_30m_tokens": 70, "ttl_12h_tokens": 0}}}));
    journal.append(&call);
    let entry = &lines(journal.path())[0];
    assert_eq!(entry["cacheWrite"], json!(100));
    assert_eq!(entry["cacheWrite30m"], json!(70));
    assert_eq!(entry["cacheWrite12h"], json!(0));
    assert!(entry.get("costWarning").is_none());
}

// yusAgent (T08, YA-13): a raw usage without the split (Anthropic, or Responses without it)
// writes no TTL field.
#[test]
fn a_raw_usage_without_a_split_writes_no_ttl_field() {
    let dir = tempfile::tempdir().unwrap();
    let journal = journal(dir.path(), None);
    let mut call = record(CallKind::Main, SystemTime::now(), Some(1));
    call.provider_usage = Some(json!({"input_tokens": 10, "cache_creation_input_tokens": 100}));
    journal.append(&call);
    let entry = &lines(journal.path())[0];
    assert!(entry["providerUsage"].is_object());
    for field in ["cacheWrite30m", "cacheWrite12h", "costWarning"] {
        assert!(entry.get(field).is_none(), "{field} must be absent");
    }
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

// yusAgent (T05, YA-05): the masking transform notes an invalidation; the next agent-loop
// call carries it, once, and a side call never takes it.
#[tokio::test]
async fn a_noted_invalidation_rides_the_next_main_call_only() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(journal(dir.path(), None));
    let run = |kind: CallKind| {
        let stream_fn = instrument_stream_fn(
            Some(Arc::clone(&journal)),
            kind,
            scripted(json!({}), pa_agent::types::Usage::zero()),
        );
        async move {
            let mut stream = stream_fn(
                agent_model(),
                LlmContext::default(),
                StreamRequestOptions::default(),
            )
            .await
            .unwrap();
            while stream.next_event().await.is_some() {}
        }
    };
    let noted = json!({"cause": "masking", "turn": 16, "frontier": 5, "masked": 5});
    journal.note_invalidation(noted.clone());
    run(CallKind::Other("side_question")).await;
    run(CallKind::Main).await;
    run(CallKind::Main).await;
    let entries = lines(journal.path());
    assert_eq!(entries.len(), 3);
    assert!(entries[0].get("invalidation").is_none());
    assert_eq!(entries[1]["invalidation"], noted);
    assert!(entries[2].get("invalidation").is_none());
}
