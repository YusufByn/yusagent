//! yusAgent (YA-03, ticket 09): the prompt text of the session's tools, applied on top of the
//! upstream layers (`layers/core.md`, `layers/usage.md`) and of `rlm_off`, without editing the
//! layer files.
//!
//! Every session (any family, any setting):
//! - `bash()` documents `env=`; `read()` is documented; `read` joins the non-skill tools;
//! - one rule says an over-long `python_repl` output keeps its start and end and names the
//!   file holding all of it (no number).
//!
//! `apply_patch` registered (`editTool` native on the Codex family): the harness has two tools,
//! edits go through `apply_patch`, `edit()` is for batches driven by Python.
//!
//! `exec_command` registered (`shellTool: "native"`): the `bash` entry and its rules are
//! replaced by the `exec_command` / `write_stdin` rules, the session-role lines about
//! `bash()` completions are dropped, and the `python_repl` description points to
//! `exec_command`.

use std::borrow::Cow;

use crate::session_engine::ya_tool_family::ToolChoice;

// --- core layer markers (upstream text) ---
const CORE_ONE_TOOL: &str =
    "The prime-agent harness has one tool: `python_repl`, a persistent CPython REPL.";
const CORE_BASH_SIGNATURE: &str = "- `bash(command: str, persistent: bool = False) -> BashHandle`:";
const CORE_PERSISTENT_END: &str = "polled, so its result and notification lag by about a second\n";
const CORE_BASH_BLOCK_END: &str = "    - `.duration: float`\n";
const CORE_EDIT_ENTRY: &str = "- `edit.run(path: str, old_str: str, new_str: str) -> str`: the primary method for editing files. async, exact-unique-match;";
const CORE_NON_SKILLS: &str = "except for `bash`, `rlm.*`, and `mcp.*`";

// --- core layer text (yusAgent) ---
const CORE_BASH_SIGNATURE_ENV: &str = "- `bash(command: str, persistent: bool = False, env: dict[str, str] | None = None) -> BashHandle`:";
const CORE_ENV_ITEM: &str = "  - `env`: extra environment variables for this command only, added on top of the REPL's environment\n";
const CORE_READ_ENTRY: &str = "- `read(path: str, offset: int = 1, limit: int = 2000) -> str`: synchronous; returns up to `limit` lines from line `offset`, numbered like `cat -n`; when lines remain, a last line gives the offset to continue from\n";
const CORE_NON_SKILLS_READ: &str = "except for `bash`, `read`, `rlm.*`, and `mcp.*`";
const CORE_NON_SKILLS_NO_BASH: &str = "except for `read`, `rlm.*`, and `mcp.*`";
const CORE_TWO_TOOLS: &str = "The prime-agent harness has two tools: `python_repl`, a persistent CPython REPL, and `apply_patch`, which edits files.";
const CORE_TOOLS_SHELL: &str = "The prime-agent harness has these tools: `python_repl`, a persistent CPython REPL; `exec_command` and `write_stdin`, which run shell commands.";
const CORE_TOOLS_PATCH_SHELL: &str = "The prime-agent harness has these tools: `python_repl`, a persistent CPython REPL; `apply_patch`, which edits files; `exec_command` and `write_stdin`, which run shell commands.";
const CORE_EDIT_BATCH: &str = "- `edit.run(path: str, old_str: str, new_str: str) -> str`: async, exact-unique-match, for batches of edits driven by Python;";

// --- usage layer markers (upstream lines) ---
const USAGE_BASH_RULES: &str = "- Agents run shell commands with `bash()`, not `subprocess`/`os.system`: subprocess calls block the kernel, show the user nothing while they run, and spawn processes the harness cannot see or stop.\n- `bash(\"\"\"...\"\"\")` should be used over `bash(\"...\")` because it makes using quotation marks inside bash easy.\n- Shell state does not persist between calls, but agents can use `os.chdir(...)` for the working directory and `os.environ[...]` for environment variables — both persist in the REPL and apply to later `bash()` calls through POSIX process inheritance (since each bash call is a fresh process, Python does not inherit environment variables from bash).\n";
const USAGE_EDIT_RULE_START: &str =
    "- Edits of existing files must be performed using `edit` with exact old/new strings";
const USAGE_NAMED_VARIABLES: &str = "- Agents always assign read/search results to named variables so they can revisit them later.\n";
const USAGE_POLL_RULE: &str = "- Agents do not keep the turn open by polling with `time.sleep()`, shell `sleep`, or repeated awaits of a still-running handle; they end the turn and collect the result on the done notification.\n";
const USAGE_ORCHESTRATION_RULE: &str = "- Python is the orchestration language: agents use Python for loops, conditionals, parsing, and state. They use `bash()` to invoke programs, not to write shell programs, shell loops, or heredocs; those are done directly in Python.\n";

// --- usage layer text (yusAgent) ---
const USAGE_TRUNCATION: &str = "- Output of one `python_repl` call beyond a size limit keeps its beginning and end; the result names the file that holds the full output.\n";
const USAGE_TRUNCATION_SHELL: &str = "- Output of one `python_repl` or `exec_command` call beyond a size limit keeps its beginning and end; the result names the file that holds the full output.\n";
const USAGE_EDIT_PATCH: &str = "- Files are created and edited with the `apply_patch` tool. `edit` (exact old/new strings) is for batches of edits driven by Python, for example the same change across many files.\n";
const USAGE_SHELL_RULES: &str = "- Agents run shell commands with the `exec_command` tool, not `subprocess`/`os.system` in the REPL: subprocess calls block the kernel, show the user nothing while they run, and spawn processes the harness cannot see or stop.\n- Each `exec_command` call starts a fresh shell in the session working directory, or in `workdir`; `cd` and `export` do not persist between calls, and `os.chdir`/`os.environ` in the REPL do not apply to it.\n";
const USAGE_POLL_SHELL: &str = "- Agents do not poll with `time.sleep()` or shell `sleep`; a command still running when `exec_command` returns is followed with `write_stdin` and empty `chars`, which waits for new output.\n";
const USAGE_ORCHESTRATION_SHELL: &str = "- Python is the orchestration language: agents use Python for loops, conditionals, parsing, and state.\n";

// --- opinionated layer marker (upstream lines) ---
const OPINIONATED_BASH_PATTERN: &str = "- Agents can use the following patterns to make better use of the REPL:\n  - Use f-strings (`f\"\"\"...\"\"\"` or `f\"...\"`) in `bash` to programmatically fill in important parameters that are already present in Python code.\n";

// --- session role and tool description (upstream text) ---
const DAEMONLESS_NO_SPAWN_HOST: &str = "This session has no daemon: `rlm.spawn` is unavailable, heartbeats never fire, and background `bash()` completions wake you with a queued `[bash-done]` turn.";
const DAEMONLESS_NO_SPAWN_HOST_SHELL: &str =
    "This session has no daemon: `rlm.spawn` is unavailable and heartbeats never fire.";
const DAEMONLESS_SHELL_SPAWN: &str = "This session has no daemon: heartbeats never fire; the run waits for your subagents before it exits. Subagent replies and exit notices still wake you.";
const DAEMONLESS_SHELL_NO_SUBAGENT: &str = "This session has no daemon: heartbeats never fire.";
const SKILLS_NOTE_BASH: &str = "of the Python REPL and `bash()`";
const SKILLS_NOTE_SHELL: &str = "of the Python REPL and of `exec_command`";
const PYTHON_REPL_BASH: &str = "Run shell commands with `bash('cmd')` / `await bash('cmd')`.";
const PYTHON_REPL_SHELL: &str = "Run shell commands with the `exec_command` tool.";

fn replace_once(text: &str, old: &str, new: &str) -> String {
    text.replacen(old, new, 1)
}

/// The core layer for the session's tools.
#[must_use]
pub fn core_layer(layer: &str, choice: ToolChoice) -> String {
    let mut text = replace_once(layer, CORE_BASH_SIGNATURE, CORE_BASH_SIGNATURE_ENV);
    text = replace_once(
        &text,
        CORE_PERSISTENT_END,
        &format!("{CORE_PERSISTENT_END}{CORE_ENV_ITEM}"),
    );
    text = replace_once(
        &text,
        CORE_EDIT_ENTRY,
        &format!("{CORE_READ_ENTRY}{CORE_EDIT_ENTRY}"),
    );
    text = replace_once(&text, CORE_NON_SKILLS, CORE_NON_SKILLS_READ);
    if choice.apply_patch {
        text = replace_once(&text, CORE_ONE_TOOL, CORE_TWO_TOOLS);
        text = replace_once(&text, CORE_EDIT_ENTRY, CORE_EDIT_BATCH);
    }
    if choice.native_shell {
        if let (Some(start), Some(end)) = (
            text.find(CORE_BASH_SIGNATURE_ENV),
            text.find(CORE_BASH_BLOCK_END),
        ) {
            if start < end {
                text.replace_range(start..end + CORE_BASH_BLOCK_END.len(), "");
            }
        }
        text = replace_once(&text, CORE_NON_SKILLS_READ, CORE_NON_SKILLS_NO_BASH);
        let tools = if choice.apply_patch {
            (CORE_TWO_TOOLS, CORE_TOOLS_PATCH_SHELL)
        } else {
            (CORE_ONE_TOOL, CORE_TOOLS_SHELL)
        };
        text = replace_once(&text, tools.0, tools.1);
    }
    text
}

/// The usage layer for the session's tools.
#[must_use]
pub fn usage_layer(layer: &str, choice: ToolChoice) -> String {
    let truncation = if choice.native_shell {
        USAGE_TRUNCATION_SHELL
    } else {
        USAGE_TRUNCATION
    };
    let mut text = replace_once(
        layer,
        USAGE_NAMED_VARIABLES,
        &format!("{USAGE_NAMED_VARIABLES}{truncation}"),
    );
    if choice.apply_patch {
        if let Some(line) = whole_line(&text, USAGE_EDIT_RULE_START) {
            text = replace_once(&text, &line, USAGE_EDIT_PATCH);
        }
    }
    if choice.native_shell {
        text = replace_once(&text, USAGE_BASH_RULES, USAGE_SHELL_RULES);
        text = replace_once(&text, USAGE_POLL_RULE, USAGE_POLL_SHELL);
        text = replace_once(&text, USAGE_ORCHESTRATION_RULE, USAGE_ORCHESTRATION_SHELL);
    }
    text
}

/// The opinionated layer for the session's tools: without `bash()`, its `bash` pattern goes.
#[must_use]
pub fn opinionated_layer(layer: &str, choice: ToolChoice) -> Cow<'_, str> {
    if choice.native_shell {
        Cow::Owned(replace_once(layer, OPINIONATED_BASH_PATTERN, ""))
    } else {
        Cow::Borrowed(layer)
    }
}

/// The full line (with its newline) that starts with `start`.
fn whole_line(text: &str, start: &str) -> Option<String> {
    let at = text.find(start)?;
    let end = text[at..]
        .find('\n')
        .map_or(text.len(), |offset| at + offset + 1);
    Some(text[at..end].to_string())
}

/// The daemonless session-role line for the session's tools: without `bash()`, the
/// `[bash-done]` clauses go.
#[must_use]
pub fn daemonless_line(line: &str, choice: ToolChoice) -> Cow<'_, str> {
    if !choice.native_shell {
        return Cow::Borrowed(line);
    }
    match line {
        DAEMONLESS_NO_SPAWN_HOST => Cow::Borrowed(DAEMONLESS_NO_SPAWN_HOST_SHELL),
        super::rlm_off::DAEMONLESS_SPAWN_LINE => Cow::Borrowed(DAEMONLESS_SHELL_SPAWN),
        super::rlm_off::DAEMONLESS_NO_SUBAGENT_LINE => Cow::Borrowed(DAEMONLESS_SHELL_NO_SUBAGENT),
        other => Cow::Borrowed(other),
    }
}

/// The skills inventory for the session's tools: in a native-shell session, the bundled-skills
/// variable is documented for `exec_command` instead of `bash()` (`session_paths`).
#[must_use]
pub fn skills_inventory(text: &str, choice: ToolChoice) -> Cow<'_, str> {
    if choice.native_shell {
        Cow::Owned(replace_once(text, SKILLS_NOTE_BASH, SKILLS_NOTE_SHELL))
    } else {
        Cow::Borrowed(text)
    }
}

/// Whether the session role carries the `await bash(cmd)` contract line.
#[must_use]
pub fn has_bash_await_line(choice: ToolChoice) -> bool {
    !choice.native_shell
}

/// The `python_repl` description of a native-shell session.
#[must_use]
pub fn python_repl_description(description: &str) -> String {
    description.replacen(PYTHON_REPL_BASH, PYTHON_REPL_SHELL, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::layers::{CORE_LAYER, USAGE_LAYER};

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

    /// Every marker matches the upstream layers exactly once: an upstream edit that moves one
    /// fails here instead of silently keeping the old text.
    #[test]
    fn markers_match_the_layers_once() {
        for marker in [
            CORE_ONE_TOOL,
            CORE_BASH_SIGNATURE,
            CORE_PERSISTENT_END,
            CORE_BASH_BLOCK_END,
            CORE_EDIT_ENTRY,
            CORE_NON_SKILLS,
        ] {
            assert_eq!(CORE_LAYER.matches(marker).count(), 1, "{marker}");
        }
        for marker in [
            USAGE_BASH_RULES,
            USAGE_EDIT_RULE_START,
            USAGE_NAMED_VARIABLES,
            USAGE_POLL_RULE,
            USAGE_ORCHESTRATION_RULE,
        ] {
            assert_eq!(USAGE_LAYER.matches(marker).count(), 1, "{marker}");
        }
        assert_eq!(
            crate::prompts::layers::OPINIONATED_LAYER
                .matches(OPINIONATED_BASH_PATTERN)
                .count(),
            1
        );
        assert_eq!(
            crate::prompts::session_paths::BUNDLED_SKILLS_NOTE
                .matches(SKILLS_NOTE_BASH)
                .count(),
            1
        );
        assert_eq!(
            crate::tools::ipython::ipython_tool_description()
                .matches(PYTHON_REPL_BASH)
                .count(),
            1
        );
    }

    #[test]
    fn every_session_documents_read_env_and_truncation() {
        for choice in [KERNEL, PATCH] {
            let core = core_layer(CORE_LAYER, choice);
            assert!(core.contains(CORE_BASH_SIGNATURE_ENV));
            assert!(core.contains(CORE_ENV_ITEM));
            assert!(core.contains(CORE_READ_ENTRY));
            assert!(core.contains(CORE_NON_SKILLS_READ));
            assert!(usage_layer(USAGE_LAYER, choice).contains(USAGE_TRUNCATION));
        }
    }

    #[test]
    fn kernel_edit_keeps_the_main_edit_text() {
        let core = core_layer(CORE_LAYER, KERNEL);
        assert!(core.contains(CORE_ONE_TOOL));
        assert!(core.contains(CORE_EDIT_ENTRY));
        assert!(!core.contains("apply_patch"));
        let usage = usage_layer(USAGE_LAYER, KERNEL);
        assert!(usage.contains(USAGE_EDIT_RULE_START));
        assert!(!usage.contains("apply_patch"));
    }

    #[test]
    fn native_edit_documents_apply_patch() {
        let core = core_layer(CORE_LAYER, PATCH);
        assert!(core.contains(CORE_TWO_TOOLS));
        assert!(core.contains(CORE_EDIT_BATCH));
        assert!(!core.contains(CORE_EDIT_ENTRY));
        let usage = usage_layer(USAGE_LAYER, PATCH);
        assert!(usage.contains(USAGE_EDIT_PATCH));
        assert!(!usage.contains(USAGE_EDIT_RULE_START));
    }

    #[test]
    fn native_shell_drops_bash_and_documents_exec_command() {
        for (choice, tools) in [(SHELL, CORE_TOOLS_SHELL), (BOTH, CORE_TOOLS_PATCH_SHELL)] {
            let core = core_layer(CORE_LAYER, choice);
            assert!(core.contains(tools), "{choice:?}");
            assert!(!core.contains("`bash("), "{choice:?}");
            assert!(!core.contains("BashHandle"), "{choice:?}");
            assert!(core.contains(CORE_NON_SKILLS_NO_BASH));
            assert!(core.contains(CORE_READ_ENTRY));
            let usage = usage_layer(USAGE_LAYER, choice);
            assert!(!usage.contains("bash()"), "{usage}");
            assert!(usage.contains(USAGE_SHELL_RULES));
            assert!(usage.contains(USAGE_POLL_SHELL));
            assert!(usage.contains(USAGE_ORCHESTRATION_SHELL));
            assert!(!usage.contains("heredocs"));
            assert!(usage.contains(USAGE_TRUNCATION_SHELL));
            assert!(!core.contains("\n\n\n") && !usage.contains("\n\n\n"));
        }
        let opinionated = opinionated_layer(crate::prompts::layers::OPINIONATED_LAYER, SHELL);
        assert!(!opinionated.contains("`bash`"), "{opinionated}");
        assert!(opinionated.contains("- When writing or explaining something:"));
        assert_eq!(
            opinionated_layer(crate::prompts::layers::OPINIONATED_LAYER, PATCH),
            crate::prompts::layers::OPINIONATED_LAYER
        );
        let note = skills_inventory(crate::prompts::session_paths::BUNDLED_SKILLS_NOTE, SHELL);
        assert!(
            note.contains(SKILLS_NOTE_SHELL) && !note.contains("bash("),
            "{note}"
        );
        assert!(!has_bash_await_line(SHELL));
        assert!(has_bash_await_line(PATCH));
        assert_eq!(
            daemonless_line(super::super::rlm_off::DAEMONLESS_NO_SUBAGENT_LINE, SHELL),
            DAEMONLESS_SHELL_NO_SUBAGENT
        );
        assert_eq!(
            daemonless_line(super::super::rlm_off::DAEMONLESS_SPAWN_LINE, SHELL),
            DAEMONLESS_SHELL_SPAWN
        );
        assert_eq!(
            daemonless_line(DAEMONLESS_NO_SPAWN_HOST, SHELL),
            DAEMONLESS_NO_SPAWN_HOST_SHELL
        );
        assert_eq!(
            daemonless_line(DAEMONLESS_NO_SPAWN_HOST, PATCH),
            DAEMONLESS_NO_SPAWN_HOST
        );
        let description =
            python_repl_description(crate::tools::ipython::ipython_tool_description());
        assert!(description.contains(PYTHON_REPL_SHELL));
        assert!(!description.contains("bash("));
    }
}
