//! yusAgent (YA-09, ticket 11): the default `bash()` time limit of the kernel bootstrap.
//!
//! `bash(cmd)` without `timeout` takes `rlm.ya_timeout.DEFAULT_TIMEOUT` (`None`, no limit, in
//! the runtime). A version-2 session (`promptVersion: 2`, whose prompt documents
//! `timeout: float | None = 600`) gets a line right after the `read` binding of the bootstrap
//! that sets it to [`DEFAULT_TIMEOUT_SECONDS`]; a version-1 session (every session created
//! before the lot) keeps no limit, as its prompt says. The module is read from `sys.modules`:
//! `rlm.bash` imports it. The bootstrap is not part of the prompt.

/// The default limit of a version-2 session, in seconds (Unharnessed, commit 569284b36).
pub const DEFAULT_TIMEOUT_SECONDS: u32 = 600;

/// The upstream binding after which the default is set. A test pins it to the bootstrap.
const READ_BINDING: &str = "    read = _prime_agent_rlm_module.read\n";

/// The line that sets the default limit to `seconds`.
#[must_use]
pub fn bash_timeout_line(seconds: u32) -> String {
    format!("    __import__(\"sys\").modules[\"rlm.ya_timeout\"].DEFAULT_TIMEOUT = {seconds}.0\n")
}

/// The bootstrap code with the default `bash()` limit set to `seconds`; `None`: `code` as is.
#[must_use]
pub fn bash_timeout_bootstrap_code(code: &str, seconds: Option<u32>) -> String {
    match seconds {
        Some(seconds) if code.contains(READ_BINDING) => code.replacen(
            READ_BINDING,
            &format!("{READ_BINDING}{}", bash_timeout_line(seconds)),
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

    #[test]
    fn the_marker_matches_the_upstream_bootstrap() {
        for wired in [false, true] {
            let code = build_rlm_bootstrap_code(&[], wired);
            assert_eq!(code.matches(READ_BINDING).count(), 1, "{code}");
            let native = native_shell_bootstrap_code(&code);
            assert_eq!(native.matches(READ_BINDING).count(), 1, "{native}");
        }
    }

    #[test]
    fn a_version_two_session_sets_the_default_limit() {
        let code = build_rlm_bootstrap_code(&[], true);
        let with = bash_timeout_bootstrap_code(&code, Some(DEFAULT_TIMEOUT_SECONDS));
        assert_eq!(
            with.replacen(&bash_timeout_line(600), "", 1),
            code,
            "only the line is added"
        );
        assert!(with.contains(&format!(
            "{READ_BINDING}    __import__(\"sys\").modules[\"rlm.ya_timeout\"].DEFAULT_TIMEOUT = 600.0\n"
        )));
        assert_eq!(bash_timeout_bootstrap_code(&code, None), code);
    }
}
