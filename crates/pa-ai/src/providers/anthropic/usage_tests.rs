//! yusAgent (T04, YA-13): usage read from a streamed Anthropic response.
//!
//! Fixtures follow the official docs: `message_start` carries the input-side counts under
//! `message.usage`, `message_delta` carries cumulative counts, often `output_tokens` alone
//! (<https://platform.claude.com/docs/en/build-with-claude/streaming>); `cache_creation` splits
//! the cache write by TTL (<https://platform.claude.com/docs/en/build-with-claude/prompt-caching>);
//! `output_tokens_details.thinking_tokens` rides the final `message_delta`
//! (<https://platform.claude.com/docs/en/build-with-claude/extended-thinking>).

use serde_json::json;

use crate::event_stream::AssistantMessageEventExt;
use crate::providers::anthropic::{stream_anthropic, AnthropicOptions};
use crate::types::{Context, Model, StreamOptions, Usage};

/// Streams `message_start` (with `start_usage`), one text block, then `message_delta` (with
/// `delta_usage`), and returns the usage of the terminal event.
async fn final_usage(start_usage: serde_json::Value, delta_usage: serde_json::Value) -> Usage {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let start = json!({"type": "message_start", "message": {
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-test",
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": start_usage}});
    let delta = json!({"type": "message_delta",
        "delta": {"stop_reason": "end_turn", "stop_sequence": null}, "usage": delta_usage});
    let mut sse = [
        format!("event: message_start\ndata: {start}"),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#
            .to_string(),
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#
            .to_string(),
        format!("event: message_delta\ndata: {delta}"),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
    .join("\n\n");
    sse.push_str("\n\n");
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 8192];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{sse}",
                    sse.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let model: Model = serde_json::from_value(json!({
        "id": "claude-test", "name": "Claude Test", "api": "anthropic-messages",
        "provider": "anthropic", "baseUrl": format!("http://{addr}"), "reasoning": false,
        "input": ["text"],
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": 128_000, "maxTokens": 8192
    }))
    .unwrap();
    let options = AnthropicOptions::from_base(StreamOptions {
        api_key: Some("sk-ant-api-test".into()),
        ..Default::default()
    });
    let mut reader = stream_anthropic(
        &model,
        &Context {
            system_prompt: None,
            messages: vec![],
            tools: None,
        },
        Some(&options),
    );
    let mut last = None;
    while let Some(event) = reader.next_event().await {
        last = Some(event);
    }
    let last = last.expect("a terminal event");
    assert_eq!(last.event_type(), "done");
    last.partial().usage
}

/// The audit's case: `message_delta` carries `output_tokens` only, so the input side must come
/// from `message_start`.
#[tokio::test]
async fn input_side_comes_from_message_start_when_message_delta_has_output_only() {
    let usage = final_usage(
        json!({"input_tokens": 2048, "cache_read_input_tokens": 1800,
               "cache_creation_input_tokens": 248, "output_tokens": 1,
               "cache_creation": {"ephemeral_5m_input_tokens": 148,
                                  "ephemeral_1h_input_tokens": 100}}),
        json!({"output_tokens": 503}),
    )
    .await;
    assert_eq!(usage.input, 2048);
    assert_eq!(usage.cache_read, 1800);
    assert_eq!(usage.cache_write, 248);
    assert_eq!(usage.cache_write_1h, Some(100));
    assert_eq!(usage.output, 503);
    assert_eq!(usage.total_tokens, 2048 + 1800 + 248 + 503);
    assert_eq!(usage.reasoning, None);
}

/// A `message_delta` with the full cumulative usage overwrites, never adds: no double count.
#[tokio::test]
async fn cumulative_message_delta_overwrites_without_double_counting() {
    let usage = final_usage(
        json!({"input_tokens": 25, "cache_read_input_tokens": 0,
               "cache_creation_input_tokens": 0, "output_tokens": 1}),
        json!({"input_tokens": 25, "cache_read_input_tokens": 0,
               "cache_creation_input_tokens": 0, "output_tokens": 348,
               "output_tokens_details": {"thinking_tokens": 312}}),
    )
    .await;
    assert_eq!(usage.input, 25);
    assert_eq!(usage.cache_read, 0);
    assert_eq!(usage.cache_write, 0);
    assert_eq!(usage.output, 348);
    assert_eq!(usage.total_tokens, 25 + 348);
    assert_eq!(usage.reasoning, Some(312));
    assert_eq!(usage.cache_write_1h, None);
}

/// No `cache_creation` split and no thinking breakdown: both optional fields stay absent.
#[tokio::test]
async fn optional_fields_stay_absent_when_not_reported() {
    let usage = final_usage(
        json!({"input_tokens": 25, "output_tokens": 1}),
        json!({"output_tokens": 15}),
    )
    .await;
    assert_eq!(usage.input, 25);
    assert_eq!(usage.output, 15);
    assert_eq!(usage.reasoning, None);
    assert_eq!(usage.cache_write_1h, None);
    let wire = serde_json::to_value(usage).unwrap();
    assert!(wire.get("reasoning").is_none());
    assert!(wire.get("cacheWrite1h").is_none());
}
