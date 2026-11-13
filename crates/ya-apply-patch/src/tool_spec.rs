// Added by yusAgent: the model-facing spec of Codex's freeform `apply_patch`
// tool, from openai/codex codex-rs/core/src/tools/handlers/apply_patch_spec.rs
// (lines 5 and 20, `create_apply_patch_freeform_tool` without the
// `environment_id` variant) at 69231e119bfca0b14453515e97d99688ce1c47c9.
// `assets/apply_patch.lark` is codex-rs/core/assets/tools/apply_patch.lark at
// the same commit, byte for byte.

/// Name of the freeform tool.
pub const APPLY_PATCH_TOOL_NAME: &str = "apply_patch";

/// Description of the freeform tool, word for word.
pub const APPLY_PATCH_TOOL_DESCRIPTION: &str = "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.";

/// Lark grammar of the patch format (`format.definition` of the tool).
pub const APPLY_PATCH_LARK_GRAMMAR: &str = include_str!("../assets/apply_patch.lark");
