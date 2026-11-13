//! yusAgent (YA-03, ticket 09): freeform `custom_tool_call` on both Codex transports (SSE and
//! WebSocket share `ResponsesStreamProcessor::handle_event`), and its replay in the follow-up
//! request. Event shapes from openai/codex `69231e1`, `codex-rs/codex-api/src/sse/responses.rs`
//! and `codex-rs/protocol/src/models.rs`.

use super::*;
use crate::providers::ya_freeform::{freeform_tool_parameters, FREEFORM_CALL_KEY};
use crate::types::{AssistantContent, Tool};

const PATCH: &str = "*** Begin Patch\n*** Add File: a.txt\n+hello\n*** End Patch\n";

fn apply_patch_tool() -> Tool {
    Tool {
        name: "apply_patch".into(),
        description: "The `apply_patch` tool can be used to edit files.".into(),
        parameters: freeform_tool_parameters(json!({
            "type": "grammar",
            "syntax": "lark",
            "definition": "start: begin_patch hunk+ end_patch",
        })),
    }
}

fn custom_call_item(input: &str) -> Value {
    json!({
        "type": "custom_tool_call",
        "id": "ctc_1",
        "call_id": "call_p",
        "name": "apply_patch",
        "input": input,
        "status": "completed",
    })
}

/// A turn whose only output is one freeform call, its input streamed in two deltas.
fn custom_call_events(response_id: &str) -> Vec<Value> {
    let (head, tail) = PATCH.split_at(20);
    vec![
        json!({ "type": "response.created", "response": { "id": response_id } }),
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": custom_call_item(""),
        }),
        json!({
            "type": "response.custom_tool_call_input.delta",
            "output_index": 0,
            "item_id": "ctc_1",
            "delta": head,
        }),
        json!({
            "type": "response.custom_tool_call_input.delta",
            "output_index": 0,
            "item_id": "ctc_1",
            "delta": tail,
        }),
        json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": custom_call_item(PATCH),
        }),
        json!({
            "type": "response.completed",
            "response": {
                "id": response_id,
                "status": "completed",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens_details": { "reasoning_tokens": 0 },
                },
            },
        }),
    ]
}

fn empty_output(model: &Model) -> AssistantMessage {
    AssistantMessage {
        content: Vec::new(),
        api: "openai-codex-responses".into(),
        provider: "openai-codex".into(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        diagnostics: None,
        usage: Usage::default(),
        stop_reason: StopReason::Stop,
        stop_reason_raw: None,
        error_message: None,
        timestamp: 0,
        rest: Map::default(),
    }
}

fn only_tool_call(message: &AssistantMessage) -> &crate::types::ToolCall {
    let calls: Vec<_> = message
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContent::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "one tool call expected: {:?}",
        message.content
    );
    calls[0]
}

fn assert_freeform_call(message: &AssistantMessage) {
    let call = only_tool_call(message);
    assert_eq!(call.id, "call_p|ctc_1");
    assert_eq!(call.name, "apply_patch");
    assert_eq!(call.arguments.get("input"), Some(&json!(PATCH)));
    assert_eq!(call.rest.get(FREEFORM_CALL_KEY), Some(&json!("ctc_1")));
}

/// SSE: the events arrive as `data:` frames, one of them split across two network chunks.
#[test]
fn sse_chunks_carry_a_freeform_call() {
    let model = codex_wire_model();
    let mut output = empty_output(&model);
    let (writer, _stream) = AssistantMessageEventStream::new();
    let mut processor = ResponsesStreamProcessor::new(
        &model,
        &mut output,
        &writer,
        ResponsesStreamHooks::default(),
    );
    let body: String = custom_call_events("resp_sse")
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect();
    let split = body.find("custom_tool_call_input.delta").unwrap() + 10;
    let mut decoder = SseDecoder::new();
    process_sse_chunk(&body[..split], &mut decoder, &model, &mut processor).expect("first chunk");
    process_sse_chunk(&body[split..], &mut decoder, &model, &mut processor).expect("second chunk");
    processor.finish().expect("stream finish");
    assert_freeform_call(&output);
}

/// The input of a freeform call whose `added` event was missed still comes from the done item.
#[test]
fn done_item_alone_yields_the_freeform_call() {
    let model = codex_wire_model();
    let mut output = empty_output(&model);
    let (writer, _stream) = AssistantMessageEventStream::new();
    let mut processor = ResponsesStreamProcessor::new(
        &model,
        &mut output,
        &writer,
        ResponsesStreamHooks::default(),
    );
    for event in custom_call_events("resp_done").into_iter().filter(|event| {
        let kind = event["type"].as_str().unwrap_or("");
        kind != "response.output_item.added" && !kind.starts_with("response.custom_tool_call")
    }) {
        let mapped = map_codex_event(event).expect("codex event mapping");
        processor.handle_event(&mapped.event).expect("stream event");
    }
    processor.finish().expect("stream finish");
    assert_freeform_call(&output);
}

/// WebSocket: the request sends the freeform tool as `custom`, the streamed call is recorded,
/// and the follow-up request replays it as `custom_tool_call` with a `custom_tool_call_output`.
#[tokio::test]
async fn websocket_turn_round_trips_a_freeform_call() {
    let server = spawn_scripted_codex_server(vec![vec![custom_call_events("resp_ws")]]).await;
    let model = codex_test_model(server.port);
    let session_id = format!("session-ya-freeform-{}", std::process::id());
    let options = codex_test_options(&server, &session_id);
    let mut context = codex_test_context("Create a.txt");
    context.tools = Some(vec![apply_patch_tool()]);
    let first = stream_openai_codex_responses(&model, &context, Some(&options))
        .result()
        .await;
    assert_freeform_call(&first);

    let bodies = server.sent_bodies.lock().expect("bodies").clone();
    let tools = bodies[0]
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .expect("tools in the request");
    assert_eq!(
        tools,
        vec![json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "The `apply_patch` tool can be used to edit files.",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: begin_patch hunk+ end_patch",
            },
        })]
    );

    let followup = Context {
        system_prompt: context.system_prompt.clone(),
        messages: vec![
            context.messages[0].clone(),
            Message::Assistant(first.clone()),
            Message::ToolResult(ToolResultMessage {
                tool_call_id: "call_p|ctc_1".into(),
                tool_name: "apply_patch".into(),
                content: vec![UserOrToolContent::Text(TextContent {
                    text: "Success.".into(),
                    text_signature: None,
                    rest: Map::default(),
                })],
                details: None,
                is_error: false,
                timestamp: 0,
                rest: Map::default(),
            }),
        ],
        tools: context.tools.clone(),
    };
    let body = build_request_body(&model, &followup, &OpenAICodexResponsesOptions::default());
    let input = body
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .expect("request input items");
    let call = input
        .iter()
        .find(|item| item["type"] == "custom_tool_call")
        .unwrap_or_else(|| panic!("no custom_tool_call in {input:?}"));
    assert_eq!(
        call,
        &json!({
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_p",
            "name": "apply_patch",
            "input": PATCH,
        })
    );
    let output = input
        .iter()
        .find(|item| item["type"] == "custom_tool_call_output")
        .unwrap_or_else(|| panic!("no custom_tool_call_output in {input:?}"));
    assert_eq!(output["call_id"], json!("call_p"));
    assert_eq!(output["output"], json!("Success."));
    assert!(
        !input
            .iter()
            .any(|item| item["type"] == "function_call" || item["type"] == "function_call_output"),
        "a freeform call must not be replayed as a function call: {input:?}"
    );
}
