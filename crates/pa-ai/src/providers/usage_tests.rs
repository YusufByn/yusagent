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
    let model = model("openai-responses", "openai");
    let mut output = initial_assistant_message("openai-responses", "openai", "m");
    let (writer, _stream) = create_assistant_message_event_stream();
    let mut processor = ResponsesStreamProcessor::new(
        &model,
        &mut output,
        &writer,
        ResponsesStreamHooks::default(),
    );
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
