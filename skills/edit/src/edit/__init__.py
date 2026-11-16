"""Exact single-occurrence string replacement for existing files.

yusAgent (YA-09, ticket 11; listed in docs/fork-changes.md): an empty ``old_str`` creates a new
file, the result shows the changed lines with a few lines of context, numbered like ``read()``,
and every refusal says why (missing text, several occurrences, a path outside the workspace).
The workspace is the directory the kernel started in (the session's working directory); the
check is a clear error, not a security boundary (the sandbox of YA-11 is).
"""

from __future__ import annotations

from pathlib import Path

# yusAgent (YA-09): the session's working directory, where the kernel starts.
_WORKSPACE = Path.cwd().resolve()
# yusAgent (YA-09): lines of context shown around the change.
_CONTEXT_LINES = 3


async def run(path: str, old_str: str, new_str: str) -> str:
    """Replace a unique string in a file, or create a new file.

    ``old_str`` must appear exactly once in the file at ``path``; that match is
    replaced with ``new_str`` and the file is written back in place. Prefer this
    over rewriting a whole file for targeted edits. An empty ``old_str`` creates
    the file at ``path`` with ``new_str`` as its content; the file must not exist.

    Args:
        path: File to edit, relative to the working directory, absolute, or
            `~`-prefixed (the leading `~`/`~user` is expanded to the home dir).
        old_str: Exact text to find. Must occur exactly once in the file; empty
            to create a new file.
        new_str: Replacement text, or the content of a new file.

    Returns:
        The path and the changed lines with a few lines of context, numbered.

    Raises:
        FileNotFoundError: If ``path`` does not exist and ``old_str`` is not empty.
        ValueError: If ``old_str`` is absent or matches more than once, or is empty
            for an existing file.
        PermissionError: If ``path`` is outside the workspace.
    """
    filepath = Path(path).expanduser()
    resolved = filepath.resolve()
    if not resolved.is_relative_to(_WORKSPACE):
        raise PermissionError(
            f"{path} is outside the workspace {_WORKSPACE}: edit() only writes inside it"
        )
    if not filepath.exists():
        if old_str:
            raise FileNotFoundError(f"{path} not found; to create it, pass old_str=''")
        filepath.parent.mkdir(parents=True, exist_ok=True)
        filepath.write_text(new_str, encoding="utf-8")
        _emit_diff(str(resolved), "", new_str, 1)
        return f"Created {resolved}\n{_numbered(new_str, 1, new_str.count(chr(10)) + 1)}"
    if not old_str:
        raise ValueError(
            f"old_str is empty but {path} exists: pass text that occurs once in it "
            "(an empty old_str creates a new file)"
        )
    content = filepath.read_text(encoding="utf-8")
    count = content.count(old_str)
    if count == 0:
        raise ValueError(f"string not found in {path} (0 occurrences): copy old_str from the file as it is now")
    if count > 1:
        raise ValueError(
            f"found {count} occurrences in {path}, need exactly 1 — "
            "widen the snippet to make it unique"
        )
    match_index = content.index(old_str)
    start_line = content.count("\n", 0, match_index) + 1
    updated = content.replace(old_str, new_str, 1)
    filepath.write_text(updated, encoding="utf-8")
    resolved_path = str(resolved)
    _emit_diff(resolved_path, old_str, new_str, start_line)
    end_line = start_line + new_str.count("\n")
    return f"Edited {resolved_path}\n{_numbered(updated, start_line, end_line)}"


def _numbered(text: str, first: int, last: int) -> str:
    """Lines ``first``..``last`` of ``text`` with ``_CONTEXT_LINES`` around them, in the
    ``cat -n`` layout of ``read()``."""
    lines = text.split("\n")
    start = max(1, first - _CONTEXT_LINES)
    end = min(len(lines), last + _CONTEXT_LINES)
    return "\n".join(
        f"{number:>6}\t{lines[number - 1].removesuffix(chr(13))}" for number in range(start, end + 1)
    )


# Keep in sync with DIFF_DISPLAY_MIME in src/core/kernel/index.ts.
_DIFF_DISPLAY_MIME = "application/vnd.prime-agent.diff+json"


def _emit_diff(path: str, old_str: str, new_str: str, start_line: int) -> None:
    """Stream a diff to the host as a display event; best-effort outside the kernel."""
    try:
        from rlm import emit

        emit(
            {
                _DIFF_DISPLAY_MIME: {
                    "path": path,
                    "old_str": old_str,
                    "new_str": new_str,
                    "start_line": start_line,
                },
                "text/plain": f"Edited {path}",
            }
        )
    except Exception:
        pass
