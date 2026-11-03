//! yusAgent (T04, YA-13): Chat Completions usage normalization on wire fixtures (`OpenAI`,
//! `DeepSeek`, `OpenRouter`). Field semantics: `docs/usage.md`.

use serde_json::json;

use super::convert::parse_chunk_usage;
use crate::types::Model;

fn model() -> Model {
    serde_json::from_value(json!({
        "id": "m", "name": "m", "api": "openai-completions", "provider": "deepseek",
        "baseUrl": "http://localhost", "reasoning": true, "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 128_000, "maxTokens": 8192
    }))
    .unwrap()
}

/// `DeepSeek`: `prompt_tokens` = `prompt_cache_hit_tokens` + `prompt_cache_miss_tokens`
/// (<https://api-docs.deepseek.com/api/create-chat-completion>).
#[test]
fn deepseek_cache_hits_leave_input_and_reasoning_is_read() {
    let usage = parse_chunk_usage(
        &json!({"prompt_tokens": 1000, "prompt_cache_hit_tokens": 800,
                "prompt_cache_miss_tokens": 200, "completion_tokens": 50, "total_tokens": 1050,
                "completion_tokens_details": {"reasoning_tokens": 20}}),
        &model(),
        None,
    );
    assert_eq!(usage.input, 200);
    assert_eq!(usage.cache_read, 800);
    assert_eq!(usage.cache_write, 0);
    assert_eq!(usage.output, 50);
    assert_eq!(usage.reasoning, Some(20));
}

/// `OpenRouter` reports `cached_tokens` (read) and `cache_write_tokens` (written) under
/// `prompt_tokens_details` (<https://openrouter.ai/docs/use-cases/usage-accounting>).
#[test]
fn openrouter_cache_write_leaves_input() {
    let usage = parse_chunk_usage(
        &json!({"prompt_tokens": 1000,
                "prompt_tokens_details": {"cached_tokens": 600, "cache_write_tokens": 300},
                "completion_tokens": 10, "total_tokens": 1010}),
        &model(),
        None,
    );
    // The adapter treats `cached_tokens` as reads plus this request's writes (observed on
    // `OpenRouter`, `convert.rs`), hence 600 - 300 reads.
    assert_eq!(usage.cache_read, 300);
    assert_eq!(usage.cache_write, 300);
    assert_eq!(usage.input, 400);
    assert_eq!(usage.reasoning, None);
}
