//! yusAgent (YA-02; CDC YA-03, YA-11): the kernel env is an allowlist, not the
//! harness's whole env. Decision of 31/10/2026: secrets never reach the
//! kernel nor `bash()`. Listed in `docs/fork-changes.md`.
//!
//! The kernel env is built in three steps:
//! 1. at spawn, [`inherited_env`] keeps the ambient vars named in
//!    [`BUILTIN_ALLOWLIST`] (system base, what the runtime reads, uv);
//! 2. the harness's own injections (`KernelManagerOptions::env`) go on top,
//!    filtered at session wiring by [`apply_user_allowlist`], which also
//!    adds the ambient vars the user lists in `kernel.envAllowlist`;
//! 3. [`strip_harness_credentials`] runs last: the provider credentials the
//!    harness authenticates with never pass, whatever their source.
//!
//! A wildcard entry never matches a secret-looking name ([`looks_secret`]):
//! such a var passes only when the user names it exactly. `bash()` and the
//! persistent shells inherit the kernel's `os.environ` and nothing else
//! (`prime-agent-runtime/src/rlm/bash.py`, `_child_env`), and `rlm.spawn`
//! children are sessions whose kernels start through the same spawn path.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// The `kernel` settings block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KernelSettings {
    /// Extra ambient vars passed to the kernel and `bash()` (`DATABASE_URL`,
    /// `DATABASE_*`): names, `*` wildcards accepted. Project settings replace
    /// the global list (arrays replace in `deep_merge`).
    pub env_allowlist: Option<Vec<String>>,
}

/// Ambient vars the kernel always inherits when set. Every name the kernel
/// runtime and its skills read is here (`grep` of `os.environ` / `getenv`
/// in `prime-agent-runtime/src` and `skills/*/src`); the harness injects
/// most of them itself through `KernelManagerOptions::env`.
pub const BUILTIN_ALLOWLIST: &[&str] = &[
    // System base.
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "LC_*",
    "TZ",
    "TMPDIR",
    // Windows counterparts (`bash.py` `_system32`, `mcp.py` `_SAFE_ENV`).
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    // Read by the kernel runtime and its skills.
    "PRIME_AGENT_CODING_AGENT_DIR",
    "PI_CODING_AGENT_DIR",
    "PRIME_AGENT_MACHINES_DIR",
    "PRIME_AGENT_BASH_SHELL",
    "PRIME_AGENT_BASH_COMMAND_PREFIX",
    "PRIME_AGENT_INTERNAL_ORPHAN_PROCESS_JOURNAL",
    "PRIME_AGENT_KERNEL_OWNER_PID",
    "RLM_SESSION_ID",
    "RLM_SESSION_DIR",
    "RLM_HARNESS_STATE_DIR",
    "RLM_GLOBAL_HARNESS_STATE_DIR",
    // Python: an explicit kernel interpreter (`PRIME_AGENT_KERNEL_PYTHON`)
    // skips the skill install and finds the skills on `PYTHONPATH`.
    "PYTHONPATH",
    "PYTHONUTF8",
    // uv in `bash()`: cache, Python and config locations, TLS bundles.
    // Index URLs can embed credentials: they pass through
    // `kernel.envAllowlist` only.
    "UV_CACHE_DIR",
    "UV_PYTHON_INSTALL_DIR",
    "UV_PYTHON",
    "UV_PYTHON_PREFERENCE",
    "UV_PYTHON_DOWNLOADS",
    "UV_MANAGED_PYTHON",
    "UV_NO_MANAGED_PYTHON",
    "UV_TOOL_DIR",
    "UV_TOOL_BIN_DIR",
    "UV_CONFIG_FILE",
    "UV_NO_CONFIG",
    "UV_OFFLINE",
    "UV_NATIVE_TLS",
    "UV_LINK_MODE",
    "XDG_*",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Name fragments that mark a var as secret-looking (upper-cased compare).
const SECRET_MARKERS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "AUTH",
    "PRIVATE",
    "COOKIE",
];

/// Credentials the harness authenticates with that no provider catalog
/// entry names: ambient cloud sources (`env_api_keys::get_env_api_key`),
/// the trace upload key and the daemon worker token.
const EXTRA_HARNESS_CREDENTIALS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    "AWS_WEB_IDENTITY_TOKEN_FILE",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "PRIME_AGENT_TRACES_API_KEY",
    "PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN",
];

/// Every provider credential var (upper-cased): the env key names of each
/// catalog provider plus [`EXTRA_HARNESS_CREDENTIALS`].
static HARNESS_CREDENTIALS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut names: HashSet<String> = EXTRA_HARNESS_CREDENTIALS
        .iter()
        .copied()
        .map(str::to_ascii_uppercase)
        .collect();
    for provider in pa_ai::models_generated::get_providers() {
        if let Some(vars) = pa_ai::env_api_keys::get_api_key_env_vars(provider) {
            names.extend(vars.into_iter().map(str::to_ascii_uppercase));
        }
    }
    names
});

/// `kernel.envAllowlist` from the merged settings (empty when unset).
#[must_use]
pub fn env_allowlist(settings: &crate::settings::Settings) -> Vec<String> {
    settings
        .kernel
        .as_ref()
        .and_then(|kernel| kernel.env_allowlist.clone())
        .unwrap_or_default()
}

/// True for a name that may hold a secret (`*_API_KEY`, `*_TOKEN`,
/// `*_SECRET`, passwords, auth sockets...).
#[must_use]
pub fn looks_secret(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_MARKERS.iter().any(|marker| upper.contains(marker))
}

/// True for a provider credential the harness authenticates with
/// (case-insensitive).
#[must_use]
pub fn is_harness_credential(name: &str) -> bool {
    HARNESS_CREDENTIALS.contains(&name.to_ascii_uppercase())
}

/// The ambient part of the kernel env: the [`BUILTIN_ALLOWLIST`] vars only.
#[must_use]
pub fn inherited_env(
    ambient: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    ambient
        .into_iter()
        .filter(|(name, _)| allows(BUILTIN_ALLOWLIST, name))
        .collect()
}

/// Session wiring: filters the harness's own kernel env injections and adds
/// the ambient vars `kernel.envAllowlist` allows.
/// - A secret-looking injection (the websearch `SERPER_API_KEY`) stays only
///   when the user names it exactly.
/// - An allowed ambient var joins unless the harness already set it.
/// - A provider credential never joins.
///
/// Both refusals warn on stderr (the harness log; a daemon worker's lands in
/// the daemon log) with the var name, never its value.
pub fn apply_user_allowlist(
    env: &mut HashMap<String, String>,
    allowlist: &[String],
    ambient: impl IntoIterator<Item = (String, String)>,
) {
    env.retain(|name, _| {
        let keep = !looks_secret(name) || names_exactly(allowlist, name);
        if !keep {
            eprintln!(
                "pa-core: kernel env: {name} withheld from the kernel; name it in kernel.envAllowlist to pass it"
            );
        }
        keep
    });
    for (name, value) in ambient {
        if env.contains_key(&name) || !allows(allowlist, &name) {
            continue;
        }
        if is_harness_credential(&name) {
            eprintln!(
                "pa-core: kernel env: {name} is a provider credential; never passed to the kernel, even from kernel.envAllowlist"
            );
            continue;
        }
        env.insert(name, value);
    }
}

/// Last step at spawn: drops every provider credential, whatever its source.
pub fn strip_harness_credentials(env: &mut HashMap<String, String>) {
    env.retain(|name, _| !is_harness_credential(name));
}

/// An entry allows `name` when it names it exactly, or when it is a
/// wildcard that matches it and `name` does not look secret.
fn allows<S: AsRef<str>>(entries: &[S], name: &str) -> bool {
    entries.iter().any(|entry| {
        let entry = entry.as_ref();
        if entry.contains('*') {
            !looks_secret(name) && glob_matches(entry, name)
        } else {
            same_name(entry, name)
        }
    })
}

fn names_exactly(entries: &[String], name: &str) -> bool {
    entries
        .iter()
        .any(|entry| !entry.contains('*') && same_name(entry, name))
}

/// Env names compare case-insensitively on Windows only (`Path`, `PATH`).
fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// `*` matches any run of characters, including none.
fn glob_matches(pattern: &str, name: &str) -> bool {
    if cfg!(windows) {
        glob(&pattern.to_ascii_uppercase(), &name.to_ascii_uppercase())
    } else {
        glob(pattern, name)
    }
}

fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((&last, middle)) = parts.split_last() else {
        // No wildcard at all: the whole name must match.
        return rest.is_empty();
    };
    for &part in middle {
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }
    rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    fn names(env: &HashMap<String, String>) -> Vec<&str> {
        let mut names: Vec<&str> = env.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    fn allowlist(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_string()).collect()
    }

    #[test]
    fn glob_matches_prefixes_suffixes_and_inner_runs() {
        assert!(glob("LC_*", "LC_ALL"));
        assert!(glob("LC_*", "LC_"));
        assert!(!glob("LC_*", "XLC_ALL"));
        assert!(glob("*_URL", "DATABASE_URL"));
        assert!(!glob("*_URL", "DATABASE_URLS"));
        assert!(glob("A*B*C", "AxxBxxC"));
        assert!(!glob("A*B*C", "AxxC"));
        assert!(!glob("A*A", "A"));
        assert!(glob("*", "ANYTHING"));
        assert!(glob("PATH", "PATH"));
        assert!(!glob("PATH", "PATHX"));
    }

    #[test]
    fn secret_looking_names() {
        for name in [
            "ANTHROPIC_API_KEY",
            "YA_CAPTURE_FAKE_TOKEN",
            "MY_SECRET",
            "DATABASE_PASSWORD",
            "SSH_AUTH_SOCK",
            "github_token",
        ] {
            assert!(looks_secret(name), "{name}");
        }
        for name in ["PATH", "DATABASE_URL", "LC_ALL", "RLM_SESSION_ID"] {
            assert!(!looks_secret(name), "{name}");
        }
    }

    #[test]
    fn the_inherited_env_keeps_the_builtin_allowlist_only() {
        let env = inherited_env(vars(&[
            ("PATH", "/usr/bin"),
            ("HOME", "/home/u"),
            ("LC_ALL", "C"),
            ("XDG_CACHE_HOME", "/c"),
            ("UV_CACHE_DIR", "/uv"),
            ("PRIME_AGENT_INTERNAL_ORPHAN_PROCESS_JOURNAL", "/j"),
            ("PYTHONPATH", "/p"),
            ("ANTHROPIC_API_KEY", "sk-test"),
            ("YA_CAPTURE_FAKE_TOKEN", "t"),
            ("PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN", "w"),
            ("UV_PUBLISH_TOKEN", "u"),
            ("UV_INDEX_URL", "https://user:pw@index"),
            ("DATABASE_URL", "postgres://"),
            ("RLM_DEPTH", "1"),
        ]));
        assert_eq!(
            names(&env),
            vec![
                "HOME",
                "LC_ALL",
                "PATH",
                "PRIME_AGENT_INTERNAL_ORPHAN_PROCESS_JOURNAL",
                "PYTHONPATH",
                "UV_CACHE_DIR",
                "XDG_CACHE_HOME",
            ]
        );
    }

    #[test]
    fn a_builtin_wildcard_never_passes_a_secret_looking_name() {
        let env = inherited_env(vars(&[
            ("LC_ALL", "C"),
            ("LC_FAKE_TOKEN", "t"),
            ("XDG_RUNTIME_DIR", "/run"),
            ("XDG_SECRET", "s"),
        ]));
        assert_eq!(names(&env), vec!["LC_ALL", "XDG_RUNTIME_DIR"]);
    }

    #[test]
    fn the_user_allowlist_adds_names_and_patterns() {
        let mut env = HashMap::new();
        apply_user_allowlist(
            &mut env,
            &allowlist(&["DATABASE_*", "YA_CAPTURE_ALLOWED"]),
            vars(&[
                ("DATABASE_URL", "postgres://"),
                ("DATABASE_PASSWORD", "pw"),
                ("YA_CAPTURE_ALLOWED", "visible"),
                ("UNLISTED", "x"),
            ]),
        );
        assert_eq!(names(&env), vec!["DATABASE_URL", "YA_CAPTURE_ALLOWED"]);
        assert_eq!(env["YA_CAPTURE_ALLOWED"], "visible");
    }

    #[test]
    fn an_exact_user_entry_passes_a_secret_looking_name() {
        let mut env = HashMap::new();
        apply_user_allowlist(
            &mut env,
            &allowlist(&["DATABASE_PASSWORD", "*"]),
            vars(&[("DATABASE_PASSWORD", "pw"), ("OTHER_TOKEN", "t")]),
        );
        assert_eq!(names(&env), vec!["DATABASE_PASSWORD"]);
    }

    #[test]
    fn provider_credentials_never_pass_even_when_listed() {
        let mut env = HashMap::new();
        apply_user_allowlist(
            &mut env,
            &allowlist(&[
                "ANTHROPIC_API_KEY",
                "OPENAI_API_KEY",
                "GEMINI_API_KEY",
                "AWS_*",
                "PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN",
                "AWS_REGION",
            ]),
            vars(&[
                ("ANTHROPIC_API_KEY", "sk-test"),
                ("OPENAI_API_KEY", "sk-test"),
                ("GEMINI_API_KEY", "g"),
                ("AWS_PROFILE", "p"),
                ("AWS_REGION", "eu-west-3"),
                ("PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN", "w"),
            ]),
        );
        assert_eq!(names(&env), vec!["AWS_REGION"]);
    }

    #[test]
    fn every_catalog_provider_key_is_a_harness_credential() {
        for provider in pa_ai::models_generated::get_providers() {
            for var in pa_ai::env_api_keys::get_api_key_env_vars(provider).unwrap_or_default() {
                assert!(is_harness_credential(var), "{provider}: {var}");
            }
        }
        assert!(is_harness_credential("ANTHROPIC_OAUTH_TOKEN"));
        assert!(is_harness_credential("anthropic_api_key"));
        assert!(!is_harness_credential("DATABASE_URL"));
    }

    #[test]
    fn harness_injections_win_and_secret_injections_need_an_exact_entry() {
        let harness = || {
            HashMap::from([
                ("RLM_SESSION_ID".to_string(), String::new()),
                ("SERPER_API_KEY".to_string(), "resolved".to_string()),
            ])
        };
        let ambient = || {
            vars(&[
                ("RLM_SESSION_ID", "inherited"),
                ("SERPER_API_KEY", "ambient"),
            ])
        };

        let mut env = harness();
        apply_user_allowlist(&mut env, &allowlist(&["RLM_*"]), ambient());
        assert_eq!(names(&env), vec!["RLM_SESSION_ID"]);
        assert_eq!(env["RLM_SESSION_ID"], "");

        let mut env = harness();
        apply_user_allowlist(&mut env, &allowlist(&["SERPER_API_KEY"]), ambient());
        assert_eq!(env["SERPER_API_KEY"], "resolved");
    }

    #[test]
    fn the_spawn_strip_drops_provider_credentials_from_any_source() {
        let mut env = HashMap::from([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("ANTHROPIC_API_KEY".to_string(), "sk-test".to_string()),
            (
                "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
                "/adc".to_string(),
            ),
        ]);
        strip_harness_credentials(&mut env);
        assert_eq!(names(&env), vec!["PATH"]);
    }

    #[test]
    fn the_setting_reads_kernel_env_allowlist() {
        let settings: crate::settings::Settings = serde_json::from_value(serde_json::json!({
            "kernel": { "envAllowlist": ["DATABASE_URL", "PG*"] }
        }))
        .unwrap();
        assert_eq!(
            env_allowlist(&settings),
            allowlist(&["DATABASE_URL", "PG*"])
        );
        assert!(env_allowlist(&crate::settings::Settings::default()).is_empty());
    }
}
