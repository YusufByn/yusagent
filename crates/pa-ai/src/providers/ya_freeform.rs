//! yusAgent (YA-03, ticket 09): freeform (`custom`) tools on the `OpenAI` Responses API, the
//! wire shape Codex uses for `apply_patch` (openai/codex `69231e1`:
//! `codex-rs/tools/src/tool_spec.rs` variant `custom`, `codex-rs/tools/src/responses_api.rs`
//! `FreeformTool`, `codex-rs/protocol/src/models.rs` items `custom_tool_call` and
//! `custom_tool_call_output`, `codex-rs/codex-api/src/sse/responses.rs` events).
//!
//! The shared [`Tool`] and [`ToolCall`] types are left unchanged:
//! - a freeform tool is a [`Tool`] whose `parameters` carry [`FREEFORM_FORMAT_KEY`] next to a
//!   plain `{input: string}` object schema, so argument validation keeps working;
//! - a freeform call is a [`ToolCall`] whose `arguments` are `{input: <raw text>}` and whose id
//!   is `<call_id>|<item_id>`, as for a function call.
//!
//! Replay ([`FreeformReplay`]): a call is freeform when the tool it names is declared freeform in
//! the request's tools. Nothing on the call itself says so: the agent loop's tool call
//! (`pa_agent::types::ToolCall`) has no field for provider metadata, so a mark set by the
//! stream would be lost before the next request. The request's tools are the session's,
//! frozen per session (resume and fork included), so a history replays its calls the way they
//! were made.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::types::{AssistantContent, Context, Message, Tool, ToolCall};

/// Key of the freeform `format` object inside a tool's `parameters`.
pub const FREEFORM_FORMAT_KEY: &str = "x-yusagent-freeform";

/// The argument holding a freeform call's raw input.
pub const FREEFORM_INPUT_ARG: &str = "input";

/// `parameters` of a freeform tool: an object schema with one required `input` string (what the
/// executor receives) plus the `format` sent on the wire (`{type, syntax, definition}`).
#[must_use]
pub fn freeform_tool_parameters(format: &Value) -> Value {
    json!({
        "type": "object",
        "properties": {
            FREEFORM_INPUT_ARG: { "type": "string" },
        },
        "required": [FREEFORM_INPUT_ARG],
        FREEFORM_FORMAT_KEY: format,
    })
}

/// The Responses `custom` tool for a freeform [`Tool`], `None` for a function tool.
#[must_use]
pub fn responses_custom_tool(tool: &Tool) -> Option<Value> {
    let format = tool.parameters.get(FREEFORM_FORMAT_KEY)?;
    Some(json!({
        "type": "custom",
        "name": tool.name,
        "description": tool.description,
        "format": format,
    }))
}

/// A freeform call built from a `custom_tool_call` item (`call_id`, `id`, `name`, `input`).
#[must_use]
pub fn tool_call_from_item(item: &Value, input: &str) -> ToolCall {
    let item_id = str_field(item, "id");
    let mut arguments = Map::new();
    arguments.insert(FREEFORM_INPUT_ARG.into(), json!(input));
    ToolCall {
        id: format!("{}|{item_id}", str_field(item, "call_id")),
        name: str_field(item, "name").to_string(),
        arguments,
        thought_signature: None,
        rest: Map::new(),
    }
}

fn str_field<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The input text of a freeform call.
#[must_use]
pub fn freeform_input(call: &ToolCall) -> &str {
    call.arguments
        .get(FREEFORM_INPUT_ARG)
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Append a streamed input delta to a freeform call.
pub fn append_input(call: &mut ToolCall, delta: &str) {
    let mut input = freeform_input(call).to_string();
    input.push_str(delta);
    call.arguments
        .insert(FREEFORM_INPUT_ARG.into(), Value::String(input));
}

/// The freeform tools of a request, read back from its serialized `tools` (the `custom`
/// entries): a request body carries them where the conversation context is not at hand (the
/// Codex WebSocket continuation, `openai_codex_responses`).
#[must_use]
pub fn freeform_tools_of_request(body: &Value) -> Option<Vec<Tool>> {
    let tools: Vec<Tool> = body
        .get("tools")?
        .as_array()?
        .iter()
        .filter(|tool| tool.get("type").and_then(Value::as_str) == Some("custom"))
        .map(|tool| Tool {
            name: str_field(tool, "name").to_string(),
            description: str_field(tool, "description").to_string(),
            parameters: freeform_tool_parameters(tool.get("format").unwrap_or(&Value::Null)),
        })
        .collect();
    Some(tools)
}

/// How a request replays its freeform calls: which calls are freeform (by the tool they name),
/// their original item ids, and the call ids whose results go back as `custom_tool_call_output`.
#[derive(Debug, Default)]
pub struct FreeformReplay {
    /// Names of the tools declared freeform in the request.
    tools: HashSet<String>,
    /// Item id of each freeform call, by its tool-call id. The id is sent back only for a call
    /// of the same model (see [`FreeformReplay::custom_tool_call_item`]), whose id the
    /// conversion leaves as is (`transform_messages` normalizes only other models' calls).
    item_ids: HashMap<String, String>,
    /// Call ids replayed as `custom_tool_call` so far.
    call_ids: HashSet<String>,
}

impl FreeformReplay {
    /// The replay of `context`: its freeform tools, and the item id of each freeform call of its
    /// history.
    #[must_use]
    pub fn new(context: &Context) -> Self {
        let tools: HashSet<String> = context
            .tools
            .iter()
            .flatten()
            .filter(|tool| tool.parameters.get(FREEFORM_FORMAT_KEY).is_some())
            .map(|tool| tool.name.clone())
            .collect();
        let mut item_ids = HashMap::new();
        for message in &context.messages {
            let Message::Assistant(assistant) = message else {
                continue;
            };
            for block in &assistant.content {
                let AssistantContent::ToolCall(call) = block else {
                    continue;
                };
                if !tools.contains(&call.name) {
                    continue;
                }
                if let Some((_, item_id)) = call.id.split_once('|') {
                    if !item_id.is_empty() {
                        item_ids.insert(call.id.clone(), item_id.to_string());
                    }
                }
            }
        }
        FreeformReplay {
            tools,
            item_ids,
            call_ids: HashSet::new(),
        }
    }

    /// Whether `call` names a tool declared freeform in the request.
    #[must_use]
    pub fn is_freeform(&self, call: &ToolCall) -> bool {
        self.tools.contains(&call.name)
    }

    /// The replayed `custom_tool_call` item of `call` (its normalized form), noting its call id
    /// for [`FreeformReplay::output_type`]. The original item id is sent back only for a call of
    /// the same model and provider (as the `fc_` id of a function call); otherwise it is
    /// omitted. `call_id` is the part of the tool-call id before the `|`.
    pub fn custom_tool_call_item(&mut self, call: &ToolCall, keep_item_id: bool) -> Value {
        let call_id = call.id.split('|').next().unwrap_or_default();
        self.call_ids.insert(call_id.to_string());
        let mut entry = Map::new();
        entry.insert("type".into(), json!("custom_tool_call"));
        if keep_item_id {
            if let Some(item_id) = self.item_ids.get(&call.id) {
                entry.insert("id".into(), json!(item_id));
            }
        }
        entry.insert("call_id".into(), json!(call_id));
        entry.insert("name".into(), json!(call.name));
        entry.insert("input".into(), json!(freeform_input(call)));
        Value::Object(entry)
    }

    /// The output item type for the result of `call_id`.
    #[must_use]
    pub fn output_type(&self, call_id: &str) -> &'static str {
        if self.call_ids.contains(call_id) {
            "custom_tool_call_output"
        } else {
            "function_call_output"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssistantMessage;

    fn format() -> Value {
        json!({ "type": "grammar", "syntax": "lark", "definition": "start: x" })
    }

    #[test]
    fn freeform_tool_goes_on_the_wire_as_custom() {
        let tool = Tool {
            name: "apply_patch".into(),
            description: "Edit files.".into(),
            parameters: freeform_tool_parameters(&format()),
        };
        assert_eq!(
            responses_custom_tool(&tool),
            Some(json!({
                "type": "custom",
                "name": "apply_patch",
                "description": "Edit files.",
                "format": format(),
            }))
        );
    }

    #[test]
    fn function_tool_is_not_custom() {
        let tool = Tool {
            name: "python_repl".into(),
            description: "Run code.".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        };
        assert_eq!(responses_custom_tool(&tool), None);
    }

    fn apply_patch_tool() -> Tool {
        Tool {
            name: "apply_patch".into(),
            description: "Edit files.".into(),
            parameters: freeform_tool_parameters(&format()),
        }
    }

    fn assistant_with(calls: Vec<ToolCall>) -> AssistantMessage {
        AssistantMessage {
            content: calls.into_iter().map(AssistantContent::ToolCall).collect(),
            api: "openai-codex-responses".into(),
            provider: "openai-codex".into(),
            model: "gpt-6.1-terra".into(),
            response_model: None,
            response_id: None,
            diagnostics: None,
            usage: crate::types::Usage::default(),
            stop_reason: crate::types::StopReason::ToolUse,
            stop_reason_raw: None,
            error_message: None,
            timestamp: 0,
            rest: Map::new(),
        }
    }

    fn function_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: Map::new(),
            thought_signature: None,
            rest: Map::new(),
        }
    }

    #[test]
    fn streamed_call_keeps_its_input_and_ids() {
        let item = json!({
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_1",
            "name": "apply_patch",
            "input": "*** Begin Patch\n",
        });
        let mut call = tool_call_from_item(&item, "*** Begin");
        append_input(&mut call, " Patch\n");
        assert_eq!(call.id, "call_1|ctc_1");
        assert_eq!(freeform_input(&call), "*** Begin Patch\n");
        assert!(call.rest.is_empty(), "no mark on the call: {:?}", call.rest);
    }

    /// The call is freeform because `apply_patch` is declared freeform in the request; it keeps
    /// its item id; its result goes back as a custom output.
    #[test]
    fn calls_of_declared_freeform_tools_replay_as_custom() {
        let freeform = tool_call_from_item(
            &json!({ "id": "ctc_1", "call_id": "call_1", "name": "apply_patch" }),
            "*** Begin Patch\n",
        );
        let python = function_call("call_2|fc_2", "python_repl");
        let assistant = assistant_with(vec![freeform.clone(), python.clone()]);
        let context = Context {
            system_prompt: None,
            messages: vec![Message::Assistant(assistant)],
            tools: Some(vec![apply_patch_tool()]),
        };
        let mut replay = FreeformReplay::new(&context);
        assert!(replay.is_freeform(&freeform));
        assert!(!replay.is_freeform(&python));
        assert_eq!(
            replay.custom_tool_call_item(&freeform, true),
            json!({
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n",
            })
        );
        assert_eq!(
            replay.custom_tool_call_item(&freeform, false).get("id"),
            None
        );
        assert_eq!(replay.output_type("call_1"), "custom_tool_call_output");
        assert_eq!(replay.output_type("call_2"), "function_call_output");
    }

    /// Without the tool in the request (no tools, or a function tool of that name), the call
    /// replays as a function call.
    #[test]
    fn calls_of_undeclared_tools_are_not_freeform() {
        let call = function_call("call_1|ctc_1", "apply_patch");
        for tools in [
            None,
            Some(vec![Tool {
                name: "apply_patch".into(),
                description: String::new(),
                parameters: json!({ "type": "object" }),
            }]),
        ] {
            let context = Context {
                system_prompt: None,
                messages: vec![Message::Assistant(assistant_with(vec![call.clone()]))],
                tools,
            };
            assert!(!FreeformReplay::new(&context).is_freeform(&call));
        }
    }

    #[test]
    fn freeform_tools_are_read_back_from_a_request_body() {
        let body = json!({
            "tools": [
                responses_custom_tool(&apply_patch_tool()).unwrap(),
                { "type": "function", "name": "python_repl", "parameters": {} },
            ],
        });
        let tools = freeform_tools_of_request(&body).expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "apply_patch");
        assert_eq!(
            responses_custom_tool(&tools[0]),
            responses_custom_tool(&apply_patch_tool())
        );
        assert_eq!(freeform_tools_of_request(&json!({})), None);
    }
}
