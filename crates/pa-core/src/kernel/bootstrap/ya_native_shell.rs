//! yusAgent (YA-03, ticket 09): the kernel bootstrap of a `shellTool: "native"` session, where
//! shell commands go through the `exec_command` tool and `bash` leaves the kernel namespace.
//!
//! Only the name the model sees is removed. The installed `rlm` package keeps `rlm.bash`, so
//! the runtime-ready probe (`venv/probe.rs`, `RUNTIME_READY_CHECK`), memoized per venv and
//! shared by both variants, is unchanged. Internal callers import the `rlm.bash` submodule
//! (`from .bash import ...` in `rlm/repl.py` and `rlm/factory.py`, resolved through
//! `sys.modules`), never the package attribute, so replacing that attribute leaves them the
//! real functions. The bundled `edit` skill does not use `bash`.
//!
//! The transformation applies to the code of `runtime_code::build_rlm_bootstrap_code`:
//! - `bash = _prime_agent_rlm_module.bash` is not run: `bash` is undefined in the namespace;
//! - the package attribute `rlm.bash` (what `from rlm import bash` reads) becomes a function
//!   that raises [`NATIVE_SHELL_BASH_ERROR`];
//! - the bounded-await toggle for `[bash-done]` notices and the fallback `def bash` of a
//!   kernel without the runtime are left out.

/// The error a `bash()` call raises in a native-shell session.
pub const NATIVE_SHELL_BASH_ERROR: &str =
    "bash() is not available in this session: run shell commands with the exec_command tool.";

const BIND_BASH: &str = "    bash = _prime_agent_rlm_module.bash\n";
const BASH_NOTICES: &str = "    _prime_agent_rlm_module._set_completion_notices_wired(True)\n";
const FALLBACK_BASH: &str =
    "    def bash(command, persistent=False, env=None):\n        rlm._raise_missing()\n\n";

/// The bootstrap code of a native-shell session, from the default one. A test pins the markers
/// to the upstream bootstrap, so an upstream change that moves them fails there.
#[must_use]
pub fn native_shell_bootstrap_code(default_code: &str) -> String {
    let stub = format!(
        "    def _prime_agent_no_bash(*args, **kwargs):\n        raise RuntimeError({NATIVE_SHELL_BASH_ERROR:?})\n    _prime_agent_rlm_module.bash = _prime_agent_no_bash\n"
    );
    default_code
        .replacen(BIND_BASH, &stub, 1)
        .replacen(BASH_NOTICES, "", 1)
        .replacen(FALLBACK_BASH, "", 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::bootstrap::build_rlm_bootstrap_code;

    #[test]
    fn markers_match_the_upstream_bootstrap() {
        for wired in [false, true] {
            let code = build_rlm_bootstrap_code(&[], wired);
            assert_eq!(code.matches(BIND_BASH).count(), 1);
            assert_eq!(code.matches(FALLBACK_BASH).count(), 1, "{code}");
            assert_eq!(code.matches(BASH_NOTICES).count(), usize::from(wired));
        }
    }

    #[test]
    fn native_bootstrap_binds_no_bash_and_keeps_read() {
        let code = native_shell_bootstrap_code(&build_rlm_bootstrap_code(&[], true));
        assert!(!code.contains("    bash = "), "{code}");
        assert!(!code.contains("def bash("), "{code}");
        assert!(!code.contains("_set_completion_notices_wired"), "{code}");
        assert!(code.contains("    read = _prime_agent_rlm_module.read\n"));
        assert!(code.contains("_prime_agent_rlm_module.bash = _prime_agent_no_bash\n"));
        assert!(code.contains(&format!("{NATIVE_SHELL_BASH_ERROR:?}")));
    }
}
