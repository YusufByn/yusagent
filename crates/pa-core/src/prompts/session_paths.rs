//! yusAgent (T03, YA-04): paths that vary between sessions or installs stay out of the system
//! prompt. The kernel carries them instead, and the prompt says where to read them. Listed in
//! `docs/fork-changes.md`.
//!
//! - The conversation log (`<agent>/sessions/<session id>.jsonl`): `PRIME_AGENT_CONVERSATION_LOG`.
//! - The bundled skills root (the directory the binary was extracted to):
//!   `PRIME_AGENT_BUNDLED_SKILLS_DIR`. A bundled skill's `<location>` reads
//!   `$PRIME_AGENT_BUNDLED_SKILLS_DIR/<name>/SKILL.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Kernel env var holding the conversation log path (empty when the session is not persisted).
pub const CONVERSATION_LOG_ENV: &str = "PRIME_AGENT_CONVERSATION_LOG";

/// Kernel env var holding the bundled skills root.
pub const BUNDLED_SKILLS_ENV: &str = "PRIME_AGENT_BUNDLED_SKILLS_DIR";

/// The skills-inventory line that explains the variable-relative locations.
pub const BUNDLED_SKILLS_NOTE: &str = "A location that starts with `$PRIME_AGENT_BUNDLED_SKILLS_DIR` is relative to that environment variable of the Python REPL and `bash()`: expand it with `os.path.expandvars(...)`.";

/// The environment-section lines that replace the per-session values.
pub const PROMPT_LINES: &[&str] = &[
    "The current date (UTC) and the working directory arrive in an `<environment>` block at the end of a user message; a later block with a new date supersedes the earlier date.",
    "The conversation log (this session's JSONL transcript) path is `os.environ[\"PRIME_AGENT_CONVERSATION_LOG\"]` in the Python REPL; it is empty when the session is not persisted.",
];

/// The bundled skills root this binary loads its built-in skills from.
#[must_use]
pub fn bundled_skills_root() -> PathBuf {
    crate::packages::get_bundled_skills_dir()
}

/// The kernel env entries: always set, so a session never sees another session's values.
pub fn insert_kernel_env<S: std::hash::BuildHasher>(
    env: &mut HashMap<String, String, S>,
    conversation_log: Option<&str>,
) {
    env.insert(
        CONVERSATION_LOG_ENV.to_string(),
        conversation_log.unwrap_or_default().to_string(),
    );
    env.insert(
        BUNDLED_SKILLS_ENV.to_string(),
        bundled_skills_root().to_string_lossy().to_string(),
    );
}

/// A skill file's `<location>`: relative to `$PRIME_AGENT_BUNDLED_SKILLS_DIR` when it lives
/// under the bundled skills root, the path itself otherwise.
#[must_use]
pub fn skill_location(path: &Path) -> String {
    location_under(path, &bundled_skills_root())
}

fn location_under(path: &Path, root: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let path = std::fs::canonicalize(path).ok()?;
            let root = std::fs::canonicalize(root).ok()?;
            path.strip_prefix(root).ok().map(Path::to_path_buf)
        });
    match relative {
        Some(relative) if !relative.as_os_str().is_empty() => format!(
            "${BUNDLED_SKILLS_ENV}/{}",
            relative.to_string_lossy().replace('\\', "/")
        ),
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skills_read_relative_to_the_variable() {
        let root = Path::new("/opt/prime/skills");
        assert_eq!(
            location_under(Path::new("/opt/prime/skills/edit/SKILL.md"), root),
            "$PRIME_AGENT_BUNDLED_SKILLS_DIR/edit/SKILL.md"
        );
        assert_eq!(
            location_under(Path::new("/home/u/.prime/agent/skills/x/SKILL.md"), root),
            "/home/u/.prime/agent/skills/x/SKILL.md"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_still_matches() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("edit")).unwrap();
        std::fs::write(real.join("edit/SKILL.md"), "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            location_under(&real.join("edit/SKILL.md"), &link),
            "$PRIME_AGENT_BUNDLED_SKILLS_DIR/edit/SKILL.md"
        );
    }

    #[test]
    fn the_kernel_env_is_always_set() {
        let mut env = HashMap::new();
        insert_kernel_env(&mut env, Some("/a/sessions/s.jsonl"));
        assert_eq!(env[CONVERSATION_LOG_ENV], "/a/sessions/s.jsonl");
        assert_eq!(
            env[BUNDLED_SKILLS_ENV],
            bundled_skills_root().to_string_lossy()
        );
        insert_kernel_env(&mut env, None);
        assert_eq!(env[CONVERSATION_LOG_ENV], "");
    }
}
