//! yusAgent (YA-09, lever `bash()`): the `await bash(cmd)` line of the system prompt names the
//! session's window (`bashWaitSeconds`, `session_engine::ya_request`).
//!
//! Unset or 1: the upstream line, byte for byte. Another value replaces only the window in it
//! (`waits at most 30 seconds.`). The value is frozen per session, so the prompt stays stable
//! within a session; two sessions with different values have different prompts.

use std::num::NonZeroU32;

/// The window as upstream writes it in the line (`system_prompt.rs`). A test pins it.
pub const UPSTREAM_WINDOW: &str = "`await bash(cmd)` waits at most 1 second.";

/// `line` (the upstream `await bash(cmd)` line) with the window of `seconds`.
#[must_use]
pub fn bash_await_line(line: &str, seconds: Option<NonZeroU32>) -> String {
    match seconds {
        Some(seconds) if seconds.get() != 1 => line.replacen(
            UPSTREAM_WINDOW,
            &format!("`await bash(cmd)` waits at most {seconds} seconds."),
            1,
        ),
        _ => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::system_prompt::{build_system_prompt, BuildSystemPromptOptions};

    fn prompt(seconds: Option<u32>) -> String {
        build_system_prompt(&BuildSystemPromptOptions {
            selected_tools: Some(vec!["python_repl"]),
            daemonless: true,
            ya_bash_wait_seconds: seconds.and_then(NonZeroU32::new),
            ..Default::default()
        })
    }

    #[test]
    fn the_upstream_line_carries_the_window_once() {
        let upstream = prompt(None);
        assert_eq!(upstream.matches(UPSTREAM_WINDOW).count(), 1, "{upstream}");
        assert_eq!(prompt(Some(1)), upstream);
    }

    #[test]
    fn another_window_changes_only_the_window() {
        let upstream = prompt(None);
        let waited = prompt(Some(30));
        assert!(
            waited.contains("`await bash(cmd)` waits at most 30 seconds. If the command"),
            "{waited}"
        );
        assert!(!waited.contains(UPSTREAM_WINDOW), "{waited}");
        assert_eq!(
            waited.replacen(
                "`await bash(cmd)` waits at most 30 seconds.",
                UPSTREAM_WINDOW,
                1
            ),
            upstream
        );
    }
}
