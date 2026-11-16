//! yusAgent (YA-09, ticket 11): the short system prompt, `promptVersion: 2` (the default since
//! the lot Unharnessed; `session_engine::ya_request::PromptVersion`).
//!
//! The static prefix replaces the upstream `core`, `usage` and `opinionated` layers. Its text
//! starts from `core.md` of Unharnessed (MIT, commit `fe2cf2815`,
//! `crates/uh-core/src/prompts/layers/core.md`; notice in `docs/fork-changes.md`, "Code repris
//! d'Unharnessed"), adapted to this fork's API (`python_repl`, `bash(persistent, env,
//! timeout)`, `read()`, `edit()`) and to the rules kept by the audit of the ticket. The upstream
//! layers stay untouched: version 1 (sessions created before the lot, `promptVersion: 1`) is
//! built from them as before, byte for byte.
//!
//! The dynamic tail keeps the upstream sections (packages, project context, environment, MCP),
//! with three changes for version 2: a compact skills inventory without the bundled skills the
//! prompt or the fork's defaults make useless, the daemonless line without the "queued turn"
//! wording, and no `Recursive agent depth: 0 (root)` line for a root session.

use std::borrow::Cow;
use std::path::Path;

use crate::session_engine::ya_tool_family::ToolChoice;
use crate::skills::Skill;

/// The static prefix of the default tools (`python_repl` alone, `bash()` in the kernel).
const CORE: &str = include_str!("ya_prompt_v2/core.md");

// --- markers in `CORE`, each present once (tested) ---
const INTRO_ONE_TOOL: &str = "You have one tool, `python_repl`: a persistent CPython REPL where top-level `await` works. Every other capability is a Python function in that REPL.";
const INTRO_PATCH: &str = "You have two tools: `python_repl`, a persistent CPython REPL where top-level `await` works, and `apply_patch`, which creates and edits files. Every other capability is a Python function in the REPL.";
const INTRO_SHELL: &str = "You have three tools: `python_repl`, a persistent CPython REPL where top-level `await` works; `exec_command` and `write_stdin`, which run shell commands. Every other capability is a Python function in the REPL.";
const INTRO_PATCH_SHELL: &str = "You have four tools: `python_repl`, a persistent CPython REPL where top-level `await` works; `apply_patch`, which creates and edits files; `exec_command` and `write_stdin`, which run shell commands. Every other capability is a Python function in the REPL.";

const BASH_BLOCK: &str = "- `bash(command: str, persistent: bool = False, env: dict[str, str] | None = None, timeout: float | None = 600) -> BashHandle`: start a shell command in the background and return at once; each call is a new process; `env` adds variables for this command only; a command still running after `timeout` seconds is killed (`None`: no limit)
  - `persistent=True`: the shell survives kernel and harness restarts and has no default timeout; use it only for a server or job that must outlive a restart; after a restart, `from rlm import persistent_shells; persistent_shells()` returns the handles
  - `BashHandle`: `.pid`, `.running`, `.command`, `.tail(n=50)`, `.output()`, `.poll() -> BashResult | None`, `.kill(sig=SIGTERM, grace=5.0)`, and `await handle`
  - `BashResult`: `.exit_code`, `.output`, `.duration`
";

const EDIT_FUNCTION: &str = "- `await edit(path, old_str, new_str) -> str`: replaces the one occurrence of `old_str` in a file and returns the changed lines with their numbers; `old_str=\"\"` creates a new file; raises if `old_str` is missing or found more than once, or if the path is outside the workspace\n";
const EDIT_FUNCTION_PATCH: &str = "- `await edit(path, old_str, new_str) -> str`: replaces the one occurrence of `old_str` in a file, for batches of edits driven by Python; raises if `old_str` is missing or found more than once\n";

const GROUPING_KERNEL: &str = " (several `bash()` under `asyncio.gather`), and an edit together with the command that tests it.";
const GROUPING_PATCH: &str = " (several `bash()` under `asyncio.gather`), and an edit together with the command that tests it (an `apply_patch` call, then the `python_repl` call that tests it, in the same response: they run in order).";
const GROUPING_SHELL: &str = ", and an edit together with the command that tests it (the `python_repl` call that edits, then the `exec_command` call that tests, in the same response: they run in order).";
const GROUPING_PATCH_SHELL: &str = ", and an edit together with the command that tests it (an `apply_patch` call, then the `exec_command` call that tests it, in the same response: they run in order).";

const RULES_BASH: &str = "- Run programs with `bash()`, not `subprocess` or `os.system`. Write logic, loops and parsing in Python; use `bash()` only to call programs. Prefer `bash(\"\"\"...\"\"\")`: quotes inside need no escaping.
- `os.chdir()` and `os.environ` changes persist and apply to later `bash()` calls; `cd` and `export` inside a `bash()` call do not.
";
const RULES_SHELL: &str = "- Run shell commands with `exec_command`, not `subprocess` or `os.system` in the REPL. Write logic and parsing in Python.
- Each `exec_command` call starts a fresh shell in the working directory or in `workdir`; `cd`, `export`, `os.chdir()` and `os.environ` do not carry over to it.
";

const RULE_EDIT: &str =
    "- Create and edit files with `edit()`, with `old_str` taken from text you have read.\n";
const RULE_EDIT_PATCH: &str =
    "- Create and edit files with `apply_patch`; use `edit()` for the same change across many files.\n";

const RULE_OUTPUT: &str = "- Output of one call beyond";
const RULE_OUTPUT_SHELL: &str = "- Output of one `python_repl` or `exec_command` call beyond";

const RULE_POLL: &str = "- Do not hold the turn with `sleep` or repeated awaits of a running command: end the turn and wait for the notification.";
const RULE_POLL_SHELL: &str = "- Do not poll with `sleep`; follow a command still running when `exec_command` returns with `write_stdin` and empty `chars`.";

// --- tail ---
const SKILLS_HEADER: &str = "## Skills\n\nWhen a task matches a skill, read its SKILL.md first. A skill with an import name is already imported in the REPL. Resolve a relative path in a skill against the skill's directory.";
const SKILLS_BUNDLED_NOTE: &str =
    " Expand `$PRIME_AGENT_BUNDLED_SKILLS_DIR` with `os.path.expandvars`.";

/// Bundled skills left out of the inventory (they stay importable): `edit` is in the prompt,
/// `refine` serves the continual harness, off by default (`settings::fork_defaults`),
/// `skill-creator` and `prime-intellect` are upstream product skills.
const HIDDEN_BUNDLED: &[&str] = &["edit", "refine", "skill-creator", "prime-intellect"];
/// Hidden too unless the session has subagents (`rlmMaxDepth` 0 by default).
const SUBAGENT_BUNDLED: &[&str] = &["agent-message", "agent-observe"];

/// Short descriptions of the bundled skills the inventory shows (their `SKILL.md` stays as
/// upstream: version 1 lists it).
const BUNDLED_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "attach-image",
        "see an image file (PNG, JPEG, GIF, WebP); needs a vision-capable model.",
    ),
    (
        "compact",
        "check context usage, or compact the conversation when much work remains.",
    ),
    (
        "goal",
        "start a goal when the user asks for one, check it, or mark it complete.",
    ),
    ("mcp", "call tools on the user's MCP services."),
    (
        "rlm-heartbeat",
        "schedule a recurring wake-up when the user asks for one.",
    ),
    (
        "agent-message",
        "message your parent, siblings or children.",
    ),
];

const DAEMONLESS_V2: &str = "This session has no daemon: heartbeats never fire. Commands that end after their cell wake you with a `[bash-done]` message; the run waits for running commands before it exits.";

const PYTHON_REPL_DESCRIPTION: &str =
    "Run Python code in the persistent REPL. Top-level `await` works; variables persist across calls.";
const PYTHON_REPL_DESCRIPTION_SHELL: &str = "Run Python code in the persistent REPL. Top-level `await` works; variables persist across calls. Run shell commands with the `exec_command` tool.";

/// The static prefix of version 2 for the session's tools.
#[must_use]
pub fn static_prefix(choice: ToolChoice) -> String {
    let mut text = CORE.trim().to_string();
    let intro = match (choice.apply_patch, choice.native_shell) {
        (false, false) => INTRO_ONE_TOOL,
        (true, false) => INTRO_PATCH,
        (false, true) => INTRO_SHELL,
        (true, true) => INTRO_PATCH_SHELL,
    };
    text = text.replacen(INTRO_ONE_TOOL, intro, 1);
    if choice.apply_patch {
        text = text
            .replacen(EDIT_FUNCTION, EDIT_FUNCTION_PATCH, 1)
            .replacen(RULE_EDIT, RULE_EDIT_PATCH, 1);
    }
    let grouping = match (choice.apply_patch, choice.native_shell) {
        (false, false) => GROUPING_KERNEL,
        (true, false) => GROUPING_PATCH,
        (false, true) => GROUPING_SHELL,
        (true, true) => GROUPING_PATCH_SHELL,
    };
    text = text.replacen(GROUPING_KERNEL, grouping, 1);
    if choice.native_shell {
        text = text
            .replacen(BASH_BLOCK, "", 1)
            .replacen(RULES_BASH, RULES_SHELL, 1)
            .replacen(RULE_OUTPUT, RULE_OUTPUT_SHELL, 1)
            .replacen(RULE_POLL, RULE_POLL_SHELL, 1);
    }
    text
}

/// The compact skills inventory of version 2; empty when no skill is shown. `subagents`: the
/// session can spawn or is a child (`agent-message` is listed then). `location` gives a skill
/// file's location (`session_paths::skill_location`); one under
/// `$PRIME_AGENT_BUNDLED_SKILLS_DIR` is a bundled skill.
#[must_use]
pub fn skills_inventory(
    skills: &[Skill],
    subagents: bool,
    location: impl Fn(&Path) -> String,
) -> String {
    let mut lines = Vec::new();
    let mut bundled_shown = false;
    for skill in skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
    {
        let place = location(&skill.file_path);
        let bundled = place.starts_with('$');
        let name = skill.name.as_str();
        if bundled
            && (HIDDEN_BUNDLED.contains(&name) || (!subagents && SUBAGENT_BUNDLED.contains(&name)))
        {
            continue;
        }
        let description = bundled
            .then(|| {
                BUNDLED_DESCRIPTIONS
                    .iter()
                    .find(|(bundled_name, _)| *bundled_name == name)
                    .map(|(_, description)| *description)
            })
            .flatten()
            .unwrap_or(skill.description.as_str());
        let import = skill
            .python
            .as_ref()
            .map(|python| format!(" (import `{}`)", python.import_name))
            .unwrap_or_default();
        bundled_shown |= bundled;
        lines.push(format!("- `{name}`{import}: {description} `{place}`"));
    }
    if lines.is_empty() {
        return String::new();
    }
    let note = if bundled_shown {
        SKILLS_BUNDLED_NOTE
    } else {
        ""
    };
    format!("{SKILLS_HEADER}{note}\n\n{}", lines.join("\n"))
}

/// The daemonless session-role line of version 2: the kernel-shell line without subagents
/// becomes [`DAEMONLESS_V2`]; any other line is kept.
#[must_use]
pub fn daemonless_line(line: &str) -> Cow<'_, str> {
    if line == super::rlm_off::DAEMONLESS_NO_SUBAGENT_LINE {
        Cow::Borrowed(DAEMONLESS_V2)
    } else {
        Cow::Borrowed(line)
    }
}

/// The `python_repl` description of version 2.
#[must_use]
pub fn python_repl_description(choice: ToolChoice) -> &'static str {
    if choice.native_shell {
        PYTHON_REPL_DESCRIPTION_SHELL
    } else {
        PYTHON_REPL_DESCRIPTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{create_synthetic_source_info, SkillKind, SourceScope};
    use std::path::PathBuf;

    const KERNEL: ToolChoice = ToolChoice {
        apply_patch: false,
        native_shell: false,
    };
    const PATCH: ToolChoice = ToolChoice {
        apply_patch: true,
        native_shell: false,
    };
    const SHELL: ToolChoice = ToolChoice {
        apply_patch: false,
        native_shell: true,
    };
    const BOTH: ToolChoice = ToolChoice {
        apply_patch: true,
        native_shell: true,
    };

    #[test]
    fn every_marker_is_in_the_layer_once() {
        for marker in [
            INTRO_ONE_TOOL,
            BASH_BLOCK,
            EDIT_FUNCTION,
            GROUPING_KERNEL,
            RULES_BASH,
            RULE_EDIT,
            RULE_OUTPUT,
            RULE_POLL,
        ] {
            assert_eq!(CORE.matches(marker).count(), 1, "{marker}");
        }
    }

    #[test]
    fn the_default_documents_one_tool_and_the_kernel_functions() {
        let text = static_prefix(KERNEL);
        assert!(text.starts_with("# yusAgent"));
        assert!(text.contains("You have one tool, `python_repl`"));
        assert!(text.contains("timeout: float | None = 600"));
        assert!(text.contains("`old_str=\"\"` creates a new file"));
        assert!(text.contains("outside the workspace"));
        assert!(text.contains("an edit together with the command that tests it."));
        assert!(!text.contains("apply_patch"));
        assert!(!text.contains("exec_command"));
    }

    #[test]
    fn apply_patch_edits_files_and_edit_is_for_batches() {
        let text = static_prefix(PATCH);
        assert!(text.contains(INTRO_PATCH));
        assert!(text.contains(EDIT_FUNCTION_PATCH));
        assert!(text.contains(RULE_EDIT_PATCH));
        assert!(text.contains(GROUPING_PATCH));
        assert!(text.contains("bash(command: str"));
        assert!(!text.contains(EDIT_FUNCTION));
    }

    #[test]
    fn a_native_shell_has_no_bash() {
        for choice in [SHELL, BOTH] {
            let text = static_prefix(choice);
            assert!(!text.contains("bash("), "{text}");
            assert!(text.contains(RULES_SHELL));
            assert!(text.contains(RULE_OUTPUT_SHELL));
            assert!(text.contains(RULE_POLL_SHELL));
            assert!(text.contains("`exec_command` and `write_stdin`, which run shell commands"));
            assert!(
                text.contains("searches and commands together, and an edit"),
                "{text}"
            );
        }
        assert!(static_prefix(SHELL).contains(GROUPING_SHELL));
        assert!(static_prefix(SHELL).contains(EDIT_FUNCTION));
        assert!(static_prefix(BOTH).contains(GROUPING_PATCH_SHELL));
        assert!(static_prefix(BOTH).contains(INTRO_PATCH_SHELL));
    }

    /// The static prefix stays far under the YA-09 bound (2,000 tokens of the base prompt):
    /// about 4 characters per `o200k_base` token on this text (estimate, ticket 11).
    #[test]
    fn the_static_prefix_is_short() {
        for choice in [KERNEL, PATCH, SHELL, BOTH] {
            let length = static_prefix(choice).chars().count();
            assert!(length < 4_000, "{choice:?}: {length} chars");
        }
    }

    fn skill(name: &str, import: Option<&str>, file: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: format!("Upstream description of {name}."),
            file_path: PathBuf::from(file),
            base_dir: PathBuf::from("/skills"),
            source_info: create_synthetic_source_info("/skills", "user", SourceScope::User, None),
            disable_model_invocation: false,
            kind: if import.is_some() {
                SkillKind::Python
            } else {
                SkillKind::Markdown
            },
            python: import.map(|import_name| crate::skills::SkillPythonMetadata {
                import_name: import_name.to_string(),
                package_path: PathBuf::from("/skills"),
                pyproject_path: PathBuf::from("/skills/pyproject.toml"),
            }),
        }
    }

    fn bundled(name: &str, import: Option<&str>) -> Skill {
        skill(name, import, &format!("/bundled/{name}/SKILL.md"))
    }

    fn location(path: &Path) -> String {
        let text = path.display().to_string();
        text.strip_prefix("/bundled/").map_or(text.clone(), |rest| {
            format!("$PRIME_AGENT_BUNDLED_SKILLS_DIR/{rest}")
        })
    }

    fn all_skills() -> Vec<Skill> {
        vec![
            bundled("agent-message", Some("agent_message")),
            bundled("attach-image", Some("attach_image")),
            bundled("compact", Some("compact")),
            bundled("edit", Some("edit")),
            bundled("goal", Some("goal")),
            bundled("mcp", None),
            bundled("prime-intellect", None),
            bundled("refine", Some("refine")),
            bundled("skill-creator", None),
            skill(
                "deploy",
                None,
                "/home/user/.prime/agent/skills/deploy/SKILL.md",
            ),
        ]
    }

    #[test]
    fn the_inventory_hides_the_useless_bundled_skills() {
        let text = skills_inventory(&all_skills(), false, location);
        for hidden in [
            "agent-message",
            "`edit`",
            "prime-intellect",
            "refine",
            "skill-creator",
        ] {
            assert!(!text.contains(hidden), "{hidden}: {text}");
        }
        assert!(text.contains("- `attach-image` (import `attach_image`): see an image file (PNG, JPEG, GIF, WebP); needs a vision-capable model. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/attach-image/SKILL.md`"), "{text}");
        assert!(text.contains("- `mcp`: call tools on the user's MCP services. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/mcp/SKILL.md`"), "{text}");
        // A user skill keeps its own description.
        assert!(text.contains("- `deploy`: Upstream description of deploy. `/home/user/.prime/agent/skills/deploy/SKILL.md`"), "{text}");
        assert!(text.contains(SKILLS_BUNDLED_NOTE));
        // With subagents, `agent-message` is listed.
        let text = skills_inventory(&all_skills(), true, location);
        assert!(
            text.contains("- `agent-message` (import `agent_message`)"),
            "{text}"
        );
    }

    #[test]
    fn no_visible_skill_gives_no_section() {
        let only_hidden = vec![bundled("edit", Some("edit")), bundled("refine", None)];
        assert_eq!(skills_inventory(&only_hidden, false, location), "");
        let user_only = vec![skill("deploy", None, "/skills/deploy/SKILL.md")];
        let text = skills_inventory(&user_only, false, location);
        assert!(!text.contains(SKILLS_BUNDLED_NOTE), "{text}");
    }

    #[test]
    fn the_daemonless_line_drops_the_queued_turn_wording() {
        assert_eq!(
            daemonless_line(super::super::rlm_off::DAEMONLESS_NO_SUBAGENT_LINE),
            DAEMONLESS_V2
        );
        assert_eq!(
            daemonless_line(super::super::rlm_off::DAEMONLESS_SPAWN_LINE),
            super::super::rlm_off::DAEMONLESS_SPAWN_LINE
        );
    }

    #[test]
    fn the_python_repl_description_is_short() {
        assert!(!python_repl_description(KERNEL).contains("exec_command"));
        assert!(python_repl_description(SHELL).contains("the `exec_command` tool"));
    }
}
