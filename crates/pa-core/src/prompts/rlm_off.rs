//! yusAgent (YA-06): with `rlmMaxDepth` 0 no session can spawn a subagent,
//! so the prompt drops what only concerns subagents: the subagent part of
//! the core layer's `## Subagents` section, the multi-agent block of the
//! usage layer, and the subagent clauses of the daemonless session-role
//! line. Nothing is added in their place.
//!
//! Kept on purpose: the agent-to-agent tools (`agent_message`,
//! `agent_observe`), because root sessions are siblings and message each
//! other through the daemon (`pa-daemon/src/agent_messaging.rs`,
//! `row_is_sibling`); the heartbeat section, because a heartbeat wakes the
//! session that owns it (`skills/rlm-heartbeat/SKILL.md`); the continual
//! harness API.
//!
//! With the default bound (or any bound above 0) every function here
//! returns the upstream text unchanged, byte for byte.

use std::borrow::Cow;

/// First line of the core layer's subagent part (removed).
pub const CORE_SUBAGENTS_START: &str = "## Subagents\n";
/// First line kept after it: the agent-to-agent tools.
pub const CORE_A2A_START: &str =
    "The following programmatic tools are available in the REPL for a2a communication:";
/// Head of the usage layer's multi-agent block (removed with its sub-items).
pub const USAGE_MULTI_AGENT_START: &str = "- Instructions to agents for multi-agent work:";
/// The usage-layer clause about subagents in the progress-update rule.
pub const USAGE_PROGRESS_WITH_SUBAGENTS: &str =
    "When work follows a plan, uses many subagents, or spans multiple turns";
/// The same clause without subagents.
pub const USAGE_PROGRESS_WITHOUT_SUBAGENTS: &str =
    "When work follows a plan or spans multiple turns";
/// Daemonless session-role line when the session can spawn (upstream text).
pub const DAEMONLESS_SPAWN_LINE: &str = "This session has no daemon: heartbeats never fire, and background `bash()` completions wake you with a queued `[bash-done]` turn; the run waits for live background commands and your subagents before it exits. Subagent replies and exit notices still wake you.";
/// The same line with the subagent clauses removed.
pub const DAEMONLESS_NO_SUBAGENT_LINE: &str = "This session has no daemon: heartbeats never fire, and background `bash()` completions wake you with a queued `[bash-done]` turn; the run waits for live background commands before it exits.";

/// Whether this session's prompt drops the subagent surface: a root
/// session whose resolved `rlmMaxDepth` is 0 (same resolver as the spawn
/// gate, `rlm_host::resolve_rlm_max_depth`). A child (depth ≥ 1) never
/// drops it: it only exists when the bound is above 0.
#[must_use]
pub fn spawn_off(
    rlm_depth: Option<u32>,
    session_file: Option<&str>,
    cwd: &std::path::Path,
    agent_dir: &std::path::Path,
) -> bool {
    rlm_depth.unwrap_or(0) == 0
        && crate::session_engine::rlm_host::resolve_rlm_max_depth(
            session_file,
            None,
            cwd,
            agent_dir,
        )
        .0 == 0
}

/// The core layer, without its subagent part when `spawn_off`.
#[must_use]
pub fn core_layer(layer: &'static str, spawn_off: bool) -> Cow<'static, str> {
    if !spawn_off {
        return Cow::Borrowed(layer);
    }
    match (layer.find(CORE_SUBAGENTS_START), layer.find(CORE_A2A_START)) {
        (Some(start), Some(end)) if start < end => {
            Cow::Owned(format!("{}{}", &layer[..start], &layer[end..]))
        }
        _ => Cow::Borrowed(layer),
    }
}

/// The usage layer, without its multi-agent block and subagent clause
/// when `spawn_off`.
#[must_use]
pub fn usage_layer(layer: &'static str, spawn_off: bool) -> Cow<'static, str> {
    if !spawn_off {
        return Cow::Borrowed(layer);
    }
    let mut out = String::with_capacity(layer.len());
    let mut in_block = false;
    for line in layer.split_inclusive('\n') {
        if line.trim_end() == USAGE_MULTI_AGENT_START {
            in_block = true;
            continue;
        }
        if in_block && line.starts_with("  ") {
            continue;
        }
        in_block = false;
        out.push_str(line);
    }
    Cow::Owned(out.replace(
        USAGE_PROGRESS_WITH_SUBAGENTS,
        USAGE_PROGRESS_WITHOUT_SUBAGENTS,
    ))
}

/// The daemonless session-role line of a session that has a spawn host.
#[must_use]
pub fn daemonless_spawn_line(spawn_off: bool) -> &'static str {
    if spawn_off {
        DAEMONLESS_NO_SUBAGENT_LINE
    } else {
        DAEMONLESS_SPAWN_LINE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::layers::{CORE_LAYER, USAGE_LAYER};
    use crate::prompts::system_prompt::{build_system_prompt, BuildSystemPromptOptions};

    /// The print-mode root as the bench runs it: daemonless, with the
    /// inline spawn host, `python_repl`, the switchable skills listed.
    fn print_root(spawn_off: bool) -> BuildSystemPromptOptions<'static> {
        BuildSystemPromptOptions {
            cwd: "/w".to_string(),
            messages_path: Some("/log.jsonl".to_string()),
            selected_tools: Some(vec![pa_types::ai::PYTHON_TOOL_NAME]),
            rlm_depth: Some(0),
            daemonless: true,
            can_spawn: true,
            rlm_spawn_off: spawn_off,
            ..Default::default()
        }
    }

    /// Default bound: every piece is the upstream text, unchanged.
    #[test]
    fn the_default_keeps_the_upstream_text() {
        assert!(matches!(core_layer(CORE_LAYER, false), Cow::Borrowed(text) if text == CORE_LAYER));
        assert!(
            matches!(usage_layer(USAGE_LAYER, false), Cow::Borrowed(text) if text == USAGE_LAYER)
        );
        assert_eq!(daemonless_spawn_line(false), DAEMONLESS_SPAWN_LINE);
    }

    /// The markers still match the layers: an upstream edit that moves one
    /// fails here instead of silently keeping the subagent surface.
    #[test]
    fn the_markers_match_the_layers() {
        let start = CORE_LAYER
            .find(CORE_SUBAGENTS_START)
            .expect("subagent heading");
        let end = CORE_LAYER.find(CORE_A2A_START).expect("a2a line");
        assert!(start < end);
        assert_eq!(USAGE_LAYER.matches(USAGE_MULTI_AGENT_START).count(), 1);
        assert_eq!(
            USAGE_LAYER.matches(USAGE_PROGRESS_WITH_SUBAGENTS).count(),
            1
        );
        assert!(build_system_prompt(&print_root(false)).contains(DAEMONLESS_SPAWN_LINE));
    }

    /// `rlmMaxDepth` 0: neither `rlm.spawn` nor the subagent section nor
    /// the subagent role clauses; agent messaging and heartbeats stay.
    #[test]
    fn spawn_off_prompt_has_no_subagent_surface() {
        let prompt = build_system_prompt(&print_root(true));
        for gone in [
            "rlm.spawn",
            "## Subagents",
            "rlm.list_subagents",
            "rlm.collect",
            "multi-agent work",
            "uses many subagents",
            "your subagents",
            "Subagent replies",
            "spawning is disabled",
        ] {
            assert!(!prompt.contains(gone), "{gone:?} still in the prompt");
        }
        assert!(prompt.contains(CORE_A2A_START));
        assert!(prompt.contains("agent_message.send("));
        assert!(prompt.contains("## Heartbeat"));
        assert!(prompt.contains(DAEMONLESS_NO_SUBAGENT_LINE));
        assert!(prompt.contains(USAGE_PROGRESS_WITHOUT_SUBAGENTS));
        // The removal leaves no blank-line run behind.
        assert!(!prompt.contains("\n\n\n"));
    }

    /// The default prompt still has the whole subagent surface.
    #[test]
    fn default_prompt_keeps_the_subagent_surface() {
        let prompt = build_system_prompt(&print_root(false));
        assert!(prompt.contains("rlm.spawn("));
        assert!(prompt.contains("## Subagents"));
        assert!(prompt.contains(DAEMONLESS_SPAWN_LINE));
    }

    fn agent_dir_with(settings_json: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(json) = settings_json {
            std::fs::write(dir.path().join("settings.json"), json).unwrap();
        }
        dir
    }

    /// The switch is the global `rlmMaxDepth` setting, read for roots only.
    #[test]
    fn spawn_off_follows_rlm_max_depth() {
        let cwd = tempfile::tempdir().unwrap();
        let off = agent_dir_with(Some(r#"{ "rlmMaxDepth": 0 }"#));
        let one = agent_dir_with(Some(r#"{ "rlmMaxDepth": 1 }"#));
        let unset = agent_dir_with(None);
        assert!(spawn_off(Some(0), None, cwd.path(), off.path()));
        assert!(spawn_off(None, None, cwd.path(), off.path()));
        assert!(!spawn_off(Some(1), None, cwd.path(), off.path()));
        assert!(!spawn_off(Some(0), None, cwd.path(), one.path()));
        assert!(!spawn_off(Some(0), None, cwd.path(), unset.path()));
    }
}
