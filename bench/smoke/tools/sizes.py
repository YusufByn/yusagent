"""Estimation de la taille des changements de prompt de la PR 2 de YA-03 (tokens o200k_base).

Estimation seulement : les tailles qui font foi sont celles du banc de capture
(`bench/tools/capture/`), mesurées sur les requêtes réelles. Les textes ci-dessous recopient
ceux de `crates/pa-core/src/prompts/ya_tools_prompt.rs` ; le JSON des tools est compact et sans
l'enveloppe de la requête.

Usage (tiktoken requis, par exemple le venv d'analyse du banc de capture) :
    ~/.cache/yusagent-capture/analysis-venv/bin/python -I bench/smoke/tools/sizes.py .
"""
import json
import sys
from pathlib import Path

import tiktoken

ENC = tiktoken.get_encoding("o200k_base")
repo = Path(sys.argv[1])
CORE = (repo / "crates/pa-core/src/prompts/layers/core.md").read_text()
USAGE = (repo / "crates/pa-core/src/prompts/layers/usage.md").read_text()
OPINIONATED = (repo / "crates/pa-core/src/prompts/layers/opinionated.md").read_text()
OPINIONATED_BASH = OPINIONATED.splitlines(keepends=True)[2] + OPINIONATED.splitlines(keepends=True)[3]
assert "in `bash`" in OPINIONATED_BASH


def tok(s):
    return len(ENC.encode(s))


def rep(text, old, new):
    assert text.count(old) == 1, old[:60]
    return text.replace(old, new)


# --- morceaux communs (défaut et editTool kernel) ---
BASH_SIG_OLD = "- `bash(command: str, persistent: bool = False) -> BashHandle`:"
BASH_SIG_NEW = "- `bash(command: str, persistent: bool = False, env: dict[str, str] | None = None) -> BashHandle`:"
PERSIST_LINE_END = "polled, so its result and notification lag by about a second\n"
ENV_ITEM = "  - `env`: extra environment variables for this command only, added on top of the REPL's environment\n"
EDIT_OLD = "- `edit.run(path: str, old_str: str, new_str: str) -> str`: the primary method for editing files. async, exact-unique-match;"
READ_ITEM = "- `read(path: str, offset: int = 1, limit: int = 2000) -> str`: synchronous; returns up to `limit` lines from line `offset`, numbered like `cat -n`; when lines remain, a last line gives the offset to continue from\n"
SKILLS_OLD = "except for `bash`, `rlm.*`, and `mcp.*`"
SKILLS_READ = "except for `bash`, `read`, `rlm.*`, and `mcp.*`"
TRUNC_ANCHOR = "- Agents always assign read/search results to named variables so they can revisit them later.\n"
TRUNC_LINE = "- Output of one `python_repl` call beyond a size limit keeps its beginning and end; the result names the file that holds the full output.\n"

# --- editTool native ---
ONE_TOOL_OLD = "The prime-agent harness has one tool: `python_repl`, a persistent CPython REPL."
TWO_TOOLS = "The prime-agent harness has two tools: `python_repl`, a persistent CPython REPL, and `apply_patch`, which edits files."
EDIT_NATIVE = "- `edit.run(path: str, old_str: str, new_str: str) -> str`: async, exact-unique-match, for batches of edits driven by Python;"
USAGE_EDIT_OLD = USAGE.splitlines(keepends=True)[8]
assert USAGE_EDIT_OLD.startswith("- Edits of existing files must be performed using `edit`")
USAGE_EDIT_NATIVE = "- Files are created and edited with the `apply_patch` tool. `edit` (exact old/new strings) is for batches of edits driven by Python, for example the same change across many files.\n"


def kernel_edit(core, usage):
    core = rep(core, BASH_SIG_OLD, BASH_SIG_NEW)
    core = rep(core, PERSIST_LINE_END, PERSIST_LINE_END + ENV_ITEM)
    core = rep(core, EDIT_OLD, READ_ITEM + EDIT_OLD)
    core = rep(core, SKILLS_OLD, SKILLS_READ)
    usage = rep(usage, TRUNC_ANCHOR, TRUNC_ANCHOR + TRUNC_LINE)
    return core, usage


def native_edit(core, usage):
    core, usage = kernel_edit(core, usage)
    core = rep(core, ONE_TOOL_OLD, TWO_TOOLS)
    core = rep(core, EDIT_OLD, EDIT_NATIVE)
    usage = rep(usage, USAGE_EDIT_OLD, USAGE_EDIT_NATIVE)
    return core, usage


# --- shellTool native (appliqué après l'editTool) ---
BASH_BLOCK_START = BASH_SIG_NEW
BASH_BLOCK_END = "    - `.duration: float`\n"
USAGE_L6 = USAGE.splitlines(keepends=True)[5]
USAGE_L7 = USAGE.splitlines(keepends=True)[6]
USAGE_L8 = USAGE.splitlines(keepends=True)[7]
USAGE_L17 = USAGE.splitlines(keepends=True)[16]
USAGE_L20 = USAGE.splitlines(keepends=True)[19]
SHELL_RULES = (
    "- Agents run shell commands with the `exec_command` tool, not `subprocess`/`os.system` in the REPL: subprocess calls block the kernel, show the user nothing while they run, and spawn processes the harness cannot see or stop.\n"
    "- Each `exec_command` call starts a fresh shell in the session working directory, or in `workdir`; `cd` and `export` do not persist between calls, and `os.chdir`/`os.environ` in the REPL do not apply to it.\n"
)
POLL_RULE = "- Agents do not poll with `time.sleep()` or shell `sleep`; a command still running when `exec_command` returns is followed with `write_stdin` and empty `chars`, which waits for new output.\n"
ORCH_RULE = "- Python is the orchestration language: agents use Python for loops, conditionals, parsing, and state.\n"
TRUNC_NATIVE = "- Output of one `python_repl` or `exec_command` call beyond a size limit keeps its beginning and end; the result names the file that holds the full output.\n"


def native_shell(core, usage, edit_native):
    start = core.index(BASH_BLOCK_START)
    end = core.index(BASH_BLOCK_END) + len(BASH_BLOCK_END)
    core = core[:start] + core[end:]
    core = rep(core, SKILLS_READ, "except for `read`, `rlm.*`, and `mcp.*`")
    if edit_native:
        core = rep(core, TWO_TOOLS, "The prime-agent harness has these tools: `python_repl`, a persistent CPython REPL; `apply_patch`, which edits files; `exec_command` and `write_stdin`, which run shell commands.")
    else:
        core = rep(core, ONE_TOOL_OLD, "The prime-agent harness has these tools: `python_repl`, a persistent CPython REPL; `exec_command` and `write_stdin`, which run shell commands.")
    usage = rep(usage, USAGE_L6 + USAGE_L7 + USAGE_L8, SHELL_RULES)
    usage = rep(usage, USAGE_L17, POLL_RULE)
    usage = rep(usage, USAGE_L20, ORCH_RULE)
    usage = rep(usage, TRUNC_LINE, TRUNC_NATIVE)
    return core, usage


# Ligne de rôle et description de python_repl (system_prompt.rs:317, rlm_off.rs, ipython.rs:361)
ROLE_BASH = "`await bash(cmd)` waits at most 1 second. If the command is still running, it returns a snapshot: `r.running` is True, `r.exit_code` is None, `r.output` is the output so far, and `r.handle` is the live handle. A completed result has `r.running` False and an integer `r.exit_code`. The command keeps running after a snapshot; end the turn and collect the final result on the `[bash-done]` notification."
DAEMONLESS_OLD = "This session has no daemon: heartbeats never fire, and background `bash()` completions wake you with a queued `[bash-done]` turn; the run waits for live background commands before it exits."
DAEMONLESS_NEW = "This session has no daemon: heartbeats never fire."
DESC_OLD = "Run shell commands with `bash('cmd')` / `await bash('cmd')`."
DESC_NEW = "Run shell commands with the `exec_command` tool."

base = tok(CORE) + tok(USAGE)


def report(name, core, usage, extra=0):
    d = tok(core) + tok(usage) - base + extra
    dc = len(core) + len(usage) - len(CORE) - len(USAGE)
    print(f"{name:42s} couches core+usage : {dc:+6d} car., {tok(core)+tok(usage)-base:+5d} tok ; total avec extras {d:+5d} tok")


print(f"main : core.md {len(CORE)} car. {tok(CORE)} tok ; usage.md {len(USAGE)} car. {tok(USAGE)} tok")
ck, uk = kernel_edit(CORE, USAGE)
cn, un = native_edit(CORE, USAGE)
report("editTool kernel (shell kernel)", ck, uk)
report("défaut : editTool native (shell kernel)", cn, un)
role_delta = tok(DAEMONLESS_NEW) - tok(DAEMONLESS_OLD) - tok(ROLE_BASH) - 1  # une ligne en moins (\n)
role_delta -= tok(OPINIONATED) - tok(OPINIONATED.replace(OPINIONATED_BASH, ""))  # couche opinionated
desc_delta = tok(DESC_NEW) - tok(DESC_OLD)
cs, us = native_shell(cn, un, True)
report("shellTool native + editTool native", cs, us, role_delta + desc_delta)
cs2, us2 = native_shell(ck, uk, False)
report("shellTool native + editTool kernel", cs2, us2, role_delta + desc_delta)
print(f"  dont rôle de session et couche opinionated {role_delta:+d} tok, description python_repl {desc_delta:+d} tok")

# --- définitions de tools ---
grammar = (repo / "crates/ya-apply-patch/assets/apply_patch.lark").read_text()
apply_patch = {
    "type": "custom",
    "name": "apply_patch",
    "description": "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.",
    "format": {"type": "grammar", "syntax": "lark", "definition": grammar},
}
MOT = "Output token budget. Defaults to 10000 tokens; larger requests may be capped by policy."
exec_command = {
    "type": "function",
    "name": "exec_command",
    "description": "Runs a command in a PTY, returning output or a session ID for ongoing interaction.",
    "strict": False,
    "parameters": {
        "type": "object",
        "properties": {
            "cmd": {"type": "string", "description": "Shell command to execute."},
            "max_output_tokens": {"type": "number", "description": MOT},
            "workdir": {"type": "string", "description": "Working directory for the command. Defaults to the turn cwd."},
            "yield_time_ms": {"type": "number", "description": "Wait before yielding output. Defaults to 10000 ms; effective range is 250-30000 ms."},
        },
        "required": ["cmd"],
        "additionalProperties": False,
    },
}
write_stdin = {
    "type": "function",
    "name": "write_stdin",
    "description": "Writes characters to an existing unified exec session and returns recent output.",
    "strict": False,
    "parameters": {
        "type": "object",
        "properties": {
            "chars": {"type": "string", "description": "Bytes to write to stdin. Defaults to empty, which polls without writing."},
            "max_output_tokens": {"type": "number", "description": MOT},
            "session_id": {"type": "number", "description": "Identifier of the running unified exec session."},
            "yield_time_ms": {"type": "number", "description": "Wait before yielding output. Non-empty writes default to 250 ms and cap at 30000 ms; empty polls wait 5000-300000 ms by default."},
        },
        "required": ["session_id"],
        "additionalProperties": False,
    },
}
for name, t in [("apply_patch (custom)", apply_patch), ("exec_command", exec_command), ("write_stdin", write_stdin)]:
    s = json.dumps(t, separators=(",", ":"))
    print(f"tool {name:22s} JSON compact {len(s):5d} car., {tok(s):4d} tok")
