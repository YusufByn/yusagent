//! System-prompt assembly: a **cached static prefix** of the human-editable
//! layer files ([`layers`]) that never varies per session (providers can
//! cache it), then a **dynamic tail** with everything session-specific, in
//! that order.

pub mod layers;

pub mod model_prompts;

// yusAgent (T03, YA-04): per-session paths read from the kernel, not the prompt.
pub mod session_paths;

pub mod system_prompt;

pub use system_prompt::{
    build_system_prompt, system_prompt_breakdown, BuildSystemPromptOptions, PromptSegment,
    SegmentKind, SystemPromptBreakdown, REFINE_SKILL_NAME,
};
