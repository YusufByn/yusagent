//! yusAgent (YA-09, lever `bash()`): the `await bash(cmd)` window of the kernel bootstrap.
//!
//! Upstream bounds `await bash(cmd)` to `_YIELD_AFTER_SECONDS` (1 second) once the bootstrap
//! turns the bound on (`_set_completion_notices_wired(True)`, emitted only when the host
//! registered the `bash.completed` handler, `runtime_code.rs`). With `bashWaitSeconds` other than
//! 1 (`session_engine::ya_request`), a line right after that call sets the module global the
//! await reads (`prime-agent-runtime/src/rlm/bash.py`, `__await__`), so the runtime itself is
//! left as upstream. The global lives in the `rlm.bash` submodule, read from `sys.modules`: the
//! package attribute `rlm.bash` is the `bash` function.
//!
//! `"full"` (ticket 13) sets the global to `None`: the await then takes the unbounded path the
//! runtime already has for a kernel without the toggle (`bound = _YIELD_AFTER_SECONDS if wired
//! else None`, then `_bounded_wait` with `bound` `None` is a plain `await`), and returns when the
//! command ends or its `timeout` kills it. The completion notices stay wired for the commands
//! started without `await`.
//!
//! Without the toggle (no `bash.completed` handler, or a native-shell session where
//! `bootstrap::ya_native_shell` removed it) the await is unbounded or `bash` is absent, and the
//! code is left as is. The bootstrap is not part of the prompt.

use crate::session_engine::ya_request::BashWait;

/// The upstream toggle line, after which the window is set. A test pins it to the bootstrap.
const BASH_NOTICES: &str = "    _prime_agent_rlm_module._set_completion_notices_wired(True)\n";

/// The line that sets the window to `wait`: `N.0` seconds, or `None` for `"full"`.
#[must_use]
pub fn bash_wait_line(wait: BashWait) -> String {
    let value = match wait {
        BashWait::Seconds(seconds) => format!("{seconds}.0"),
        BashWait::Full => "None".to_string(),
    };
    format!("    __import__(\"sys\").modules[\"rlm.bash\"]._YIELD_AFTER_SECONDS = {value}\n")
}

/// The bootstrap code with the `await bash(cmd)` window set to `wait`. `None`, 1 second (the
/// upstream window) or a code without the toggle: `code` as is.
#[must_use]
pub fn bash_wait_bootstrap_code(code: &str, wait: Option<BashWait>) -> String {
    match wait {
        Some(wait)
            if wait != BashWait::Seconds(std::num::NonZeroU32::MIN)
                && code.contains(BASH_NOTICES) =>
        {
            code.replacen(
                BASH_NOTICES,
                &format!("{BASH_NOTICES}{}", bash_wait_line(wait)),
                1,
            )
        }
        _ => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::bootstrap::build_rlm_bootstrap_code;
    use crate::kernel::bootstrap::ya_native_shell::native_shell_bootstrap_code;

    fn seconds(value: u32) -> Option<BashWait> {
        BashWait::seconds(value)
    }

    #[test]
    fn the_marker_matches_the_upstream_bootstrap() {
        let wired = build_rlm_bootstrap_code(&[], true);
        assert_eq!(wired.matches(BASH_NOTICES).count(), 1, "{wired}");
        let unwired = build_rlm_bootstrap_code(&[], false);
        assert_eq!(unwired.matches(BASH_NOTICES).count(), 0, "{unwired}");
    }

    #[test]
    fn the_default_leaves_the_bootstrap_as_upstream() {
        for wired in [false, true] {
            let code = build_rlm_bootstrap_code(&[], wired);
            assert_eq!(bash_wait_bootstrap_code(&code, None), code);
            assert_eq!(bash_wait_bootstrap_code(&code, seconds(1)), code);
        }
    }

    #[test]
    fn another_window_follows_the_toggle() {
        let code = build_rlm_bootstrap_code(&[], true);
        let waited = bash_wait_bootstrap_code(&code, seconds(30));
        let line = "    __import__(\"sys\").modules[\"rlm.bash\"]._YIELD_AFTER_SECONDS = 30.0\n";
        assert_eq!(waited.matches(line).count(), 1, "{waited}");
        assert!(
            waited.contains(&format!("{BASH_NOTICES}{line}")),
            "{waited}"
        );
        assert_eq!(waited.replacen(line, "", 1), code);
    }

    /// Ticket 13: `"full"` sets the global to `None`, right after the toggle.
    #[test]
    fn full_sets_no_window() {
        let code = build_rlm_bootstrap_code(&[], true);
        let waited = bash_wait_bootstrap_code(&code, Some(BashWait::Full));
        let line = "    __import__(\"sys\").modules[\"rlm.bash\"]._YIELD_AFTER_SECONDS = None\n";
        assert_eq!(waited.matches(line).count(), 1, "{waited}");
        assert!(
            waited.contains(&format!("{BASH_NOTICES}{line}")),
            "{waited}"
        );
        assert_eq!(waited.replacen(line, "", 1), code);
        let unwired = build_rlm_bootstrap_code(&[], false);
        assert_eq!(
            bash_wait_bootstrap_code(&unwired, Some(BashWait::Full)),
            unwired
        );
        let native = native_shell_bootstrap_code(&code);
        assert_eq!(
            bash_wait_bootstrap_code(&native, Some(BashWait::Full)),
            native
        );
    }

    /// Without the toggle the await is unbounded (no `bash.completed` handler) or `bash` is
    /// absent (native shell): nothing to set.
    #[test]
    fn no_toggle_no_window() {
        let unwired = build_rlm_bootstrap_code(&[], false);
        assert_eq!(bash_wait_bootstrap_code(&unwired, seconds(30)), unwired);
        let native = native_shell_bootstrap_code(&build_rlm_bootstrap_code(&[], true));
        assert_eq!(bash_wait_bootstrap_code(&native, seconds(30)), native);
    }
}
