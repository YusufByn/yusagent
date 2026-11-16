//! System-prompt assembly: a **cached static prefix** of the human-editable
//! layer files ([`layers`]) that never varies per session (providers can
//! cache it), then a **dynamic tail** with everything session-specific, in
//! that order.

pub mod layers;

pub mod model_prompts;

// yusAgent (T03, YA-04): per-session paths read from the kernel, not the prompt.
pub mod session_paths;

// yusAgent (YA-06): `rlmMaxDepth` 0 drops the subagent surface from the prompt.
pub mod rlm_off;

// yusAgent (YA-03): prompt text of the session's native tools (`apply_patch`, `exec_command`).
pub mod ya_tools_prompt;

// yusAgent (YA-09): the `await bash(cmd)` window of the session (`bashWaitSeconds`).
pub mod ya_bash_wait;

// yusAgent (YA-09, ticket 11): the short system prompt (`promptVersion: 2`).
pub mod ya_prompt_v2;

pub mod system_prompt;

pub use system_prompt::{
    build_system_prompt, system_prompt_breakdown, BuildSystemPromptOptions, PromptSegment,
    SegmentKind, SystemPromptBreakdown, REFINE_SKILL_NAME,
};
