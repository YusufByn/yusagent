//! yusAgent (YA-03, ticket 09): freeform (`custom`) tools on the `OpenAI` Responses API, the
//! wire shape Codex uses for `apply_patch` (openai/codex `69231e1`:
//! `codex-rs/tools/src/tool_spec.rs` variant `custom`, `codex-rs/tools/src/responses_api.rs`
//! `FreeformTool`, `codex-rs/protocol/src/models.rs` items `custom_tool_call` and
//! `custom_tool_call_output`, `codex-rs/codex-api/src/sse/responses.rs` events).
//!
//! The shared [`Tool`] and [`ToolCall`] types are left unchanged:
//! - a freeform tool is a [`Tool`] whose `parameters` carry [`FREEFORM_FORMAT_KEY`] next to a
//!   plain `{input: string}` object schema, so argument validation keeps working;
//! - a freeform call is a [`ToolCall`] whose `arguments` are `{input: <raw text>}` and whose
//!   `rest` carries [`FREEFORM_CALL_KEY`] (the provider item id, empty when unknown). `rest` is
//!   flattened into the session file, so a reloaded session replays the call as `custom`.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::types::{Tool, ToolCall};

/// Key of the freeform `format` object inside a tool's `parameters`.
pub const FREEFORM_FORMAT_KEY: &str = "x-yusagent-freeform";

/// Key marking a freeform call in [`ToolCall::rest`]; its value is the provider item id.
pub const FREEFORM_CALL_KEY: &str = "yaFreeformItemId";

/// The argument holding a freeform call's raw input.
pub const FREEFORM_INPUT_ARG: &str = "input";

/// `parameters` of a freeform tool: an object schema with one required `input` string (what the
/// executor receives) plus the `format` sent on the wire (`{type, syntax, definition}`).
#[must_use]
pub fn freeform_tool_parameters(format: Value) -> Value {
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

/// Whether `call` is a freeform call.
#[must_use]
pub fn is_freeform_call(call: &ToolCall) -> bool {
    call.rest.contains_key(FREEFORM_CALL_KEY)
}

/// A freeform call built from a `custom_tool_call` item (`call_id`, `id`, `name`, `input`).
#[must_use]
pub fn tool_call_from_item(item: &Value, input: &str) -> ToolCall {
    let item_id = str_field(item, "id");
    let mut arguments = Map::new();
    arguments.insert(FREEFORM_INPUT_ARG.into(), json!(input));
    let mut rest = Map::new();
    rest.insert(FREEFORM_CALL_KEY.into(), json!(item_id));
    ToolCall {
        id: format!("{}|{item_id}", str_field(item, "call_id")),
        name: str_field(item, "name").to_string(),
        arguments,
        thought_signature: None,
        rest,
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

/// The replayed `custom_tool_call` item. The original item id is sent back only for a call of
/// the same model and provider (as the `fc_` id of a function call); otherwise it is omitted.
/// `call_id` is the part of the (possibly normalized) tool-call id before the `|`.
#[must_use]
pub fn custom_tool_call_item(call: &ToolCall, keep_item_id: bool) -> Value {
    let call_id = call.id.split('|').next().unwrap_or_default();
    let mut entry = Map::new();
    entry.insert("type".into(), json!("custom_tool_call"));
    if keep_item_id {
        if let Some(item_id) = call
            .rest
            .get(FREEFORM_CALL_KEY)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            entry.insert("id".into(), json!(item_id));
        }
    }
    entry.insert("call_id".into(), json!(call_id));
    entry.insert("name".into(), json!(call.name));
    entry.insert("input".into(), json!(freeform_input(call)));
    Value::Object(entry)
}

/// Call ids of the freeform calls replayed so far, so their results go back as
/// `custom_tool_call_output` instead of `function_call_output`.
#[derive(Debug, Default)]
pub struct FreeformCallIds(HashSet<String>);

impl FreeformCallIds {
    pub fn record(&mut self, call: &ToolCall) {
        if is_freeform_call(call) {
            let call_id = call.id.split('|').next().unwrap_or_default();
            self.0.insert(call_id.to_string());
        }
    }

    /// The output item type for the result of `call_id`.
    #[must_use]
    pub fn output_type(&self, call_id: &str) -> &'static str {
        if self.0.contains(call_id) {
            "custom_tool_call_output"
        } else {
            "function_call_output"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format() -> Value {
        json!({ "type": "grammar", "syntax": "lark", "definition": "start: x" })
    }

    #[test]
    fn freeform_tool_goes_on_the_wire_as_custom() {
        let tool = Tool {
            name: "apply_patch".into(),
            description: "Edit files.".into(),
            parameters: freeform_tool_parameters(format()),
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

    #[test]
    fn call_round_trips_to_a_custom_item() {
        let item = json!({
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_1",
            "name": "apply_patch",
            "input": "*** Begin Patch\n",
        });
        let mut call = tool_call_from_item(&item, "*** Begin");
        append_input(&mut call, " Patch\n");
        assert!(is_freeform_call(&call));
        assert_eq!(call.id, "call_1|ctc_1");
        assert_eq!(freeform_input(&call), "*** Begin Patch\n");
        assert_eq!(
            custom_tool_call_item(&call, true),
            json!({
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n",
            })
        );
        assert_eq!(custom_tool_call_item(&call, false).get("id"), None);
    }

    #[test]
    fn results_of_freeform_calls_are_custom_outputs() {
        let mut ids = FreeformCallIds::default();
        let item = json!({ "id": "ctc_1", "call_id": "call_1", "name": "apply_patch" });
        ids.record(&tool_call_from_item(&item, ""));
        ids.record(&ToolCall {
            id: "call_2|fc_2".into(),
            name: "python_repl".into(),
            arguments: Map::new(),
            thought_signature: None,
            rest: Map::new(),
        });
        assert_eq!(ids.output_type("call_1"), "custom_tool_call_output");
        assert_eq!(ids.output_type("call_2"), "function_call_output");
    }
}
