//! yusAgent (YA-04, ticket 03): signatures of a streamed Responses turn when every
//! `response.output_item.done` carries its `output_index`, as the real backend sends it.
//!
//! Before the upstream fix (`c12acadaa`, `42a07c13c`), the processor retired the item's slot
//! before reading it back: neither the text signature nor the reasoning item were recorded, a
//! text replayed as `msg_<index>`, and the turn after a text fell back to a full request
//! without `previous_response_id` (chauffe of 14/11/2026, request 2).

use super::*;
use crate::types::AssistantContent;

fn reasoning_item() -> Value {
    json!({
        "id": "rs_r1",
        "type": "reasoning",
        "status": "completed",
        "summary": [{ "type": "summary_text", "text": "Look at the file first." }],
        "encrypted_content": "opaque-reasoning",
    })
}

fn message_item(text: &str, status: &str) -> Value {
    let content = if status == "completed" {
        json!([{ "type": "output_text", "text": text, "annotations": [] }])
    } else {
        json!([])
    };
    json!({
        "type": "message",
        "id": "msg_t1",
        "role": "assistant",
        "phase": "commentary",
        "status": status,
        "content": content,
    })
}

fn function_call_item(arguments: &str, status: &str) -> Value {
    json!({
        "type": "function_call",
        "id": "fc_r1",
        "call_id": "call_r",
        "name": "python_repl",
        "arguments": arguments,
        "status": status,
    })
}

const TEXT: &str = "Let me look at the file.";
const ARGUMENTS: &str = "{\"code\":\"print(1)\"}";

/// A turn shaped like the real backend's: reasoning, a short text, then a `python_repl` call,
/// each `done` with its `output_index`.
fn reasoning_text_call_events(response_id: &str) -> Vec<Value> {
    vec![
        json!({ "type": "response.created", "response": { "id": response_id } }),
        json!({
            "type": "response.output_item.added", "output_index": 0,
            "item": { "id": "rs_r1", "type": "reasoning", "summary": [] },
        }),
        json!({
            "type": "response.reasoning_summary_part.added", "output_index": 0,
            "summary_index": 0, "part": { "type": "summary_text", "text": "" },
        }),
        json!({
            "type": "response.reasoning_summary_text.delta", "output_index": 0,
            "summary_index": 0, "delta": "Look at the file first.",
        }),
        json!({ "type": "response.output_item.done", "output_index": 0, "item": reasoning_item() }),
        json!({
            "type": "response.output_item.added", "output_index": 1,
            "item": message_item("", "in_progress"),
        }),
        json!({
            "type": "response.content_part.added", "output_index": 1, "content_index": 0,
            "part": { "type": "output_text", "text": "" },
        }),
        json!({
            "type": "response.output_text.delta", "output_index": 1, "content_index": 0,
            "delta": TEXT,
        }),
        json!({
            "type": "response.output_item.done", "output_index": 1,
            "item": message_item(TEXT, "completed"),
        }),
        json!({
            "type": "response.output_item.added", "output_index": 2,
            "item": function_call_item("", "in_progress"),
        }),
        json!({
            "type": "response.function_call_arguments.delta", "output_index": 2,
            "delta": ARGUMENTS,
        }),
        json!({
            "type": "response.output_item.done", "output_index": 2,
            "item": function_call_item(ARGUMENTS, "completed"),
        }),
        json!({
            "type": "response.completed",
            "response": {
                "id": response_id,
                "status": "completed",
                "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 },
            },
        }),
    ]
}

/// The follow-up context: the first user row, the recorded turn, then the call's result.
fn followup_context(first: &AssistantMessage) -> Context {
    let mut context = codex_test_context("Run it");
    context.messages.push(Message::Assistant(first.clone()));
    context
        .messages
        .push(Message::ToolResult(ToolResultMessage {
            tool_call_id: "call_r|fc_r1".into(),
            tool_name: "python_repl".into(),
            content: vec![UserOrToolContent::Text(TextContent {
                text: "1".into(),
                text_signature: None,
                rest: Map::default(),
            })],
            details: None,
            is_error: false,
            timestamp: 2,
            rest: Map::default(),
        }));
    context
}

/// The recorded turn: reasoning item, text signature, call with its streamed arguments.
fn assert_recorded_turn(first: &AssistantMessage) {
    assert_eq!(first.content.len(), 3, "{:?}", first.content);
    match &first.content[0] {
        AssistantContent::Thinking(block) => {
            let signature = block
                .thinking_signature
                .as_deref()
                .expect("the reasoning item is recorded");
            assert_eq!(
                serde_json::from_str::<Value>(signature).expect("reasoning item json"),
                reasoning_item()
            );
        }
        other => panic!("expected reasoning, got {other:?}"),
    }
    match &first.content[1] {
        AssistantContent::Text(block) => {
            assert_eq!(block.text, TEXT);
            let signature = block
                .text_signature
                .as_deref()
                .expect("the text signature is set");
            assert_eq!(
                serde_json::from_str::<Value>(signature).expect("text signature json"),
                json!({ "v": 1, "id": "msg_t1", "phase": "commentary" })
            );
        }
        other => panic!("expected text, got {other:?}"),
    }
    match &first.content[2] {
        AssistantContent::ToolCall(call) => {
            assert_eq!(call.id, "call_r|fc_r1");
            assert_eq!(
                Value::Object(call.arguments.clone()),
                json!({ "code": "print(1)" })
            );
        }
        other => panic!("expected tool call, got {other:?}"),
    }
}

/// The next full request: same order as the stream, reasoning item as received, text under its
/// own item id.
fn assert_full_replay(model: &Model, followup: &Context) {
    let body = build_request_body(model, followup, &OpenAICodexResponsesOptions::default());
    let input = body["input"].as_array().expect("request input items");
    let kinds: Vec<&str> = input
        .iter()
        .map(|item| {
            item["type"]
                .as_str()
                .or_else(|| item["role"].as_str())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user",
            "reasoning",
            "message",
            "function_call",
            "function_call_output"
        ],
        "{input:#?}"
    );
    assert_eq!(input[1], reasoning_item());
    assert_eq!(
        input[2],
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": TEXT, "annotations": [] }],
            "status": "completed",
            "id": "msg_t1",
            "phase": "commentary",
        })
    );
    assert_eq!(input[3]["id"], json!("fc_r1"));
    assert_eq!(input[3]["call_id"], json!("call_r"));
    assert_eq!(input[4]["call_id"], json!("call_r"));
}

/// The recorded turn carries the reasoning item and the text signature; the next full request
/// replays reasoning, text (with its item id and phase), call, then result, in stream order;
/// the next turn on the same connection is a continuation that sends only the result.
#[tokio::test]
async fn done_items_with_output_index_keep_signatures_and_the_continuation() {
    let server = spawn_scripted_codex_server(vec![vec![
        reasoning_text_call_events("resp_r1"),
        codex_response_events("resp_r2", "msg_r2", "Done"),
    ]])
    .await;
    let model = codex_test_model(server.port);
    let session_id = format!("session-ya-signatures-{}", std::process::id());
    let first = stream_openai_codex_responses(
        &model,
        &codex_test_context("Run it"),
        Some(&codex_test_options(&server, &session_id)),
    )
    .result()
    .await;

    assert_recorded_turn(&first);
    let followup = followup_context(&first);
    assert_full_replay(&model, &followup);

    // The next turn rides the connection: only the call's result after previous_response_id.
    let second = stream_openai_codex_responses(
        &model,
        &followup,
        Some(&codex_test_options(&server, &session_id)),
    )
    .result()
    .await;
    assert_eq!(
        codex_message_text(&second).as_deref(),
        Some("Done"),
        "the second turn completes: {:?}",
        second.error_message
    );
    let bodies = server.sent_bodies.lock().expect("bodies").clone();
    assert_eq!(
        bodies.len(),
        2,
        "both turns ride one connection: {bodies:?}"
    );
    assert_eq!(
        bodies[1].get("previous_response_id"),
        Some(&json!("resp_r1")),
        "the turn after a text continues the previous response: {:#}",
        bodies[1]
    );
    assert_eq!(
        bodies[1]["input"],
        json!([{ "type": "function_call_output", "call_id": "call_r", "output": "1" }]),
        "only the call's result goes after previous_response_id"
    );
}
