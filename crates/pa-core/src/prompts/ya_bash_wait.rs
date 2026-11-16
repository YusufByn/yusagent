//! yusAgent (YA-09, lever `bash()`): the `await bash(cmd)` line of the system prompt names the
//! session's window (`bashWaitSeconds`, `session_engine::ya_request`).
//!
//! `None` (no value given to the builder) or 1: the upstream line, byte for byte. A session always
//! passes its resolved value (`"full"` by default in version 2, 30 in version 1). Another number
//! replaces only the window in it (`waits at most 30 seconds.`). `"full"` (ticket 13) replaces
//! the whole line, which is about the window and its snapshot: [`FULL_WAIT_LINE`]. The value is
//! frozen per session, so the prompt stays stable within a session; two sessions with different
//! values have different prompts.

use crate::session_engine::ya_request::BashWait;

/// The window as upstream writes it in the line (`system_prompt.rs`). A test pins it.
pub const UPSTREAM_WINDOW: &str = "`await bash(cmd)` waits at most 1 second.";

/// The whole upstream line, replaced in `"full"`. A test pins it.
pub const UPSTREAM_LINE: &str = "`await bash(cmd)` waits at most 1 second. If the command is still running, it returns a snapshot: `r.running` is True, `r.exit_code` is None, `r.output` is the output so far, and `r.handle` is the live handle. A completed result has `r.running` False and an integer `r.exit_code`. The command keeps running after a snapshot; end the turn and collect the final result on the `[bash-done]` notification.";

/// The line of a `"full"` session. Unharnessed has no such line: its prompt gives
/// `await handle -> BashResult` and the default `timeout` (`crates/uh-core/src/prompts/layers/core.md:6-8`
/// at `fe2cf2815`), as our version 2 does; this line says the await is not bounded and how to
/// keep working meanwhile.
pub const FULL_WAIT_LINE: &str = "`await bash(cmd)` waits until the command ends or its `timeout` kills it, and returns its `BashResult`. To keep working while a command runs, start it without `await` and collect the result on its `[bash-done]` notification. Never `await` a server: keep its handle, with `timeout=None` or `persistent=True` so the time limit does not kill it.";

/// The advice of the upstream cell watchdog note (`kernel/manager/execution.rs`,
/// `cell_watchdog_note`), false in `"full"`: there the await does not background a command.
pub const UPSTREAM_WATCHDOG_ADVICE: &str =
    "Move long shell work to bash() (it auto-backgrounds past its yield window)";

/// The advice of the watchdog note in a `"full"` session (ticket 13, D5).
pub const FULL_WAIT_WATCHDOG_ADVICE: &str = "Start long shell work with bash() without await and collect it on its [bash-done] notification";

/// `stderr` of a cell with the watchdog advice of the session's wait: in `"full"`, the upstream
/// advice is replaced once; otherwise, or without a note, `stderr` as is. The watchdog runs in
/// daemon sessions only (`session_engine::runtime_wiring::arm_cell_timeout_ms`).
#[must_use]
pub fn watchdog_note(stderr: String, wait: Option<BashWait>) -> String {
    if wait == Some(BashWait::Full) && stderr.contains(UPSTREAM_WATCHDOG_ADVICE) {
        stderr.replacen(UPSTREAM_WATCHDOG_ADVICE, FULL_WAIT_WATCHDOG_ADVICE, 1)
    } else {
        stderr
    }
}

/// `line` (the upstream `await bash(cmd)` line) with the wait of the session.
#[must_use]
pub fn bash_await_line(line: &str, wait: Option<BashWait>) -> String {
    match wait {
        Some(BashWait::Full) => line.replacen(UPSTREAM_LINE, FULL_WAIT_LINE, 1),
        Some(BashWait::Seconds(seconds)) if seconds.get() != 1 => line.replacen(
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
        prompt_with(seconds.and_then(BashWait::seconds))
    }

    fn prompt_with(wait: Option<BashWait>) -> String {
        build_system_prompt(&BuildSystemPromptOptions {
            selected_tools: Some(vec!["python_repl"]),
            daemonless: true,
            ya_bash_wait_seconds: wait,
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

    /// Ticket 13: `"full"` replaces the whole line once; the rest of the prompt is upstream,
    /// byte for byte, in version 1 and in version 2.
    #[test]
    fn full_replaces_the_whole_line_only() {
        let upstream = prompt(None);
        assert_eq!(upstream.matches(UPSTREAM_LINE).count(), 1, "{upstream}");
        let full = prompt_with(Some(BashWait::Full));
        assert_eq!(full.matches(FULL_WAIT_LINE).count(), 1, "{full}");
        assert!(!full.contains("waits at most"), "{full}");
        assert_eq!(full.replacen(FULL_WAIT_LINE, UPSTREAM_LINE, 1), upstream);
        for version in [
            crate::session_engine::ya_request::PromptVersion::V1,
            crate::session_engine::ya_request::PromptVersion::V2,
        ] {
            let options = |wait| BuildSystemPromptOptions {
                selected_tools: Some(vec!["python_repl"]),
                daemonless: true,
                ya_bash_wait_seconds: wait,
                ya_prompt_version: Some(version),
                ..Default::default()
            };
            let one = build_system_prompt(&options(BashWait::seconds(1)));
            let full = build_system_prompt(&options(Some(BashWait::Full)));
            assert_eq!(one.matches(UPSTREAM_LINE).count(), 1, "{version:?}: {one}");
            assert_eq!(
                full.replacen(FULL_WAIT_LINE, UPSTREAM_LINE, 1),
                one,
                "{version:?}"
            );
        }
    }

    /// D5: the note of the watchdog in `"full"`. The upstream advice is pinned to the source of
    /// the note (one line of it there, the string continues across lines).
    #[test]
    fn the_watchdog_note_follows_the_wait() {
        let source = include_str!("../kernel/manager/execution.rs");
        assert!(
            source.contains("Move long shell work to \\\nbash() (it auto-backgrounds past its yield window) and split"),
            "the upstream watchdog note changed"
        );
        let note = format!(
            "boom\n[watchdog] Cell exceeded the 60s python_repl budget (ran 3m). {UPSTREAM_WATCHDOG_ADVICE} and split long python work into smaller cells. Intentional long awaits (rlm.collect timeouts) can ignore this note."
        );
        let full = watchdog_note(note.clone(), Some(BashWait::Full));
        assert_eq!(
            full,
            note.replacen(UPSTREAM_WATCHDOG_ADVICE, FULL_WAIT_WATCHDOG_ADVICE, 1)
        );
        assert!(full.contains("[bash-done]"), "{full}");
        assert!(!full.contains("auto-backgrounds"), "{full}");
        for wait in [None, BashWait::seconds(30), BashWait::seconds(1)] {
            assert_eq!(watchdog_note(note.clone(), wait), note);
        }
        assert_eq!(
            watchdog_note("plain".to_string(), Some(BashWait::Full)),
            "plain"
        );
    }
}
