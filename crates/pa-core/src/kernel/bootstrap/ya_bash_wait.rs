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
//! Without the toggle (no `bash.completed` handler, or a native-shell session where
//! `bootstrap::ya_native_shell` removed it) the await is unbounded or `bash` is absent, and the
//! code is left as is. The bootstrap is not part of the prompt.

use std::num::NonZeroU32;

/// The upstream toggle line, after which the window is set. A test pins it to the bootstrap.
const BASH_NOTICES: &str = "    _prime_agent_rlm_module._set_completion_notices_wired(True)\n";

/// The line that sets the window to `seconds`.
#[must_use]
pub fn bash_wait_line(seconds: NonZeroU32) -> String {
    format!("    __import__(\"sys\").modules[\"rlm.bash\"]._YIELD_AFTER_SECONDS = {seconds}.0\n")
}

/// The bootstrap code with the `await bash(cmd)` window set to `seconds`. `None`, 1 second (the
/// upstream window) or a code without the toggle: `code` as is.
#[must_use]
pub fn bash_wait_bootstrap_code(code: &str, seconds: Option<NonZeroU32>) -> String {
    match seconds {
        Some(seconds) if seconds.get() != 1 && code.contains(BASH_NOTICES) => code.replacen(
            BASH_NOTICES,
            &format!("{BASH_NOTICES}{}", bash_wait_line(seconds)),
            1,
        ),
        _ => code.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::bootstrap::build_rlm_bootstrap_code;
    use crate::kernel::bootstrap::ya_native_shell::native_shell_bootstrap_code;

    fn seconds(value: u32) -> Option<NonZeroU32> {
        NonZeroU32::new(value)
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
