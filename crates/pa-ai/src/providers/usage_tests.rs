//! yusAgent (T04, YA-13): usage normalization per provider, on wire fixtures. After
//! normalization, `input` excludes the cache traffic, so `input + cache_read + cache_write` is
//! the whole prompt; `reasoning` is a subset of `output`, absent when not reported.
//! Field semantics: `docs/usage.md`.

use serde_json::{json, Value};

use crate::event_stream::{create_assistant_message_event_stream, initial_assistant_message};
use crate::providers::google_stream::GoogleStreamState;
use crate::providers::openai_responses_shared::{ResponsesStreamHooks, ResponsesStreamProcessor};
use crate::types::{Model, Usage};

fn model(api: &str, provider: &str) -> Model {
    serde_json::from_value(json!({
        "id": "m", "name": "m", "api": api, "provider": provider,
        "baseUrl": "http://localhost", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 128_000, "maxTokens": 8192
    }))
    .unwrap()
}

/// The usage after one `response.completed` event carrying `usage`.
fn responses_usage(usage: &Value) -> Usage {
    responses_usage_with(
        &model("openai-responses", "openai"),
        ResponsesStreamHooks::default(),
        usage,
    )
}

fn responses_usage_with(model: &Model, hooks: ResponsesStreamHooks, usage: &Value) -> Usage {
    let mut output = initial_assistant_message("openai-responses", "openai", "m");
    let (writer, _stream) = create_assistant_message_event_stream();
    let mut processor = ResponsesStreamProcessor::new(model, &mut output, &writer, hooks);
    processor
        .handle_event(&json!({"type": "response.completed",
            "response": {"id": "resp_1", "status": "completed", "output": [], "usage": usage}}))
        .unwrap();
    output.usage
}

/// `OpenAI` Responses: `input_tokens` includes `cached_tokens`
/// (<https://developers.openai.com/api/docs/guides/prompt-caching>), `reasoning_tokens` is part
/// of `output_tokens` (<https://developers.openai.com/api/docs/guides/reasoning>).
#[test]
fn responses_subtracts_cached_tokens_and_reads_reasoning_tokens() {
    let usage = responses_usage(&json!({
        "input_tokens": 1000, "input_tokens_details": {"cached_tokens": 900},
        "output_tokens": 1186, "output_tokens_details": {"reasoning_tokens": 1024},
        "total_tokens": 2186}));
    assert_eq!(usage.input, 100);
    assert_eq!(usage.cache_read, 900);
    assert_eq!(usage.cache_write, 0);
    assert_eq!(usage.input + usage.cache_read + usage.cache_write, 1000);
    assert_eq!(usage.output, 1186);
    assert_eq!(usage.reasoning, Some(1024));
}

#[test]
fn responses_without_output_details_has_no_reasoning() {
    let usage = responses_usage(&json!({
        "input_tokens": 10, "output_tokens": 5, "total_tokens": 15}));
    assert_eq!(usage.input, 10);
    assert_eq!(usage.reasoning, None);
}

// yusAgent (T08, YA-13): cache writes. `input_tokens_details.cache_write_tokens` is part of
// `input_tokens` and excluded from `cached_tokens`; its TTL split
// (`cache_write_tokens_details`) stays in the raw usage, for the usage journal
// (<https://developers.openai.com/api/docs/guides/prompt-caching>).

fn responses_write_usage(split: Option<Value>) -> Value {
    let mut details = json!({"cached_tokens": 600, "cache_write_tokens": 300});
    if let Some(split) = split {
        details["cache_write_tokens_details"] = split;
    }
    json!({"input_tokens": 1000, "input_tokens_details": details,
        "output_tokens": 20, "total_tokens": 1020})
}

#[test]
fn responses_reads_cache_writes() {
    for split in [
        None,
        Some(json!({"ttl_30m_tokens": 200, "ttl_12h_tokens": 100})),
    ] {
        let usage = responses_usage(&responses_write_usage(split));
        assert_eq!(
            usage.input, 100,
            "input_tokens - cached_tokens - cache_write_tokens"
        );
        assert_eq!(usage.cache_read, 600);
        assert_eq!(usage.cache_write, 300);
        assert_eq!(usage.input + usage.cache_read + usage.cache_write, 1000);
        assert_eq!(usage.cache_write_1h, None, "1 h stays Anthropic's");
        assert_eq!(usage.total_tokens, 1020);
    }
}

#[test]
fn responses_without_cache_writes_is_unchanged() {
    let usage = responses_usage(&json!({
        "input_tokens": 1000, "input_tokens_details": {"cached_tokens": 900},
        "output_tokens": 5, "total_tokens": 1005}));
    assert_eq!(usage.input, 100);
    assert_eq!(usage.cache_write, 0);
}

#[test]
fn responses_input_saturates_at_zero() {
    let usage = responses_usage(&json!({
        "input_tokens": 100, "output_tokens": 1, "total_tokens": 101,
        "input_tokens_details": {"cached_tokens": 80, "cache_write_tokens": 50}}));
    assert_eq!(usage.input, 0);
}

#[test]
fn responses_reports_the_raw_usage_to_the_usage_hook() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook_seen = std::sync::Arc::clone(&seen);
    let hooks = ResponsesStreamHooks {
        on_usage: Some(std::sync::Arc::new(move |usage: &Value, model: &Model| {
            hook_seen
                .lock()
                .unwrap()
                .push((usage.clone(), model.id.clone()));
        })),
        ..ResponsesStreamHooks::default()
    };
    let raw = responses_write_usage(Some(json!({"ttl_30m_tokens": 300, "ttl_12h_tokens": 0})));
    responses_usage_with(&model("openai-responses", "openai"), hooks, &raw);
    assert_eq!(*seen.lock().unwrap(), vec![(raw, "m".to_string())]);
}

/// Every cache write is priced at the catalog write rate, whatever its TTL (`docs/usage.md`):
/// with Terra's rates (input 1.00, cache read 0.05, cache write 1.25, output 5.00 per million
/// tokens), 300 written tokens cost 300 × 1.25 / M with or without a 12 h share.
#[test]
fn cache_writes_cost_the_catalog_write_rate() {
    let mut priced = model("openai-responses", "openai");
    priced.cost = serde_json::from_value(
        json!({"input": 1.0, "output": 5.0, "cacheRead": 0.05, "cacheWrite": 1.25}),
    )
    .unwrap();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
    for split in [
        None,
        Some(json!({"ttl_30m_tokens": 200, "ttl_12h_tokens": 100})),
    ] {
        let usage = responses_usage_with(
            &priced,
            ResponsesStreamHooks::default(),
            &responses_write_usage(split),
        );
        assert!(close(usage.cost.cache_write.as_f64(), 300.0 * 1.25e-6));
        // 100 input at 1.00, 600 read at 0.05, 300 written at 1.25, 20 output at 5.00.
        let total = 100.0e-6 + 600.0 * 0.05e-6 + 300.0 * 1.25e-6 + 20.0 * 5.0e-6;
        assert!(close(usage.cost.total.as_f64(), total));
    }
}

/// Gemini: `promptTokenCount` includes `cachedContentTokenCount`, `totalTokenCount` adds
/// `thoughtsTokenCount` as its own term
/// (<https://aistudio.google.com/api-reference/generate-content>); the adapter folds thoughts
/// into `output`.
#[test]
fn google_subtracts_cached_content_and_reads_thoughts() {
    let model = model("google-generative-ai", "google");
    let mut output = initial_assistant_message("google-generative-ai", "google", "m");
    let (writer, _stream) = create_assistant_message_event_stream();
    let mut state = GoogleStreamState::new();
    state
        .handle_chunk(
            &json!({"usageMetadata": {"promptTokenCount": 1000, "cachedContentTokenCount": 600,
                "candidatesTokenCount": 40, "thoughtsTokenCount": 25, "totalTokenCount": 1065}}),
            &model,
            &mut output,
            &writer,
        )
        .unwrap();
    assert_eq!(output.usage.input, 400);
    assert_eq!(output.usage.cache_read, 600);
    assert_eq!(output.usage.cache_write, 0);
    assert_eq!(output.usage.output, 65);
    assert_eq!(output.usage.reasoning, Some(25));
}

#[test]
fn google_without_thoughts_has_no_reasoning() {
    let model = model("google-generative-ai", "google");
    let mut output = initial_assistant_message("google-generative-ai", "google", "m");
    let (writer, _stream) = create_assistant_message_event_stream();
    let mut state = GoogleStreamState::new();
    state
        .handle_chunk(
            &json!({"usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5,
                "totalTokenCount": 15}}),
            &model,
            &mut output,
            &writer,
        )
        .unwrap();
    assert_eq!(output.usage.input, 10);
    assert_eq!(output.usage.output, 5);
    assert_eq!(output.usage.reasoning, None);
}
