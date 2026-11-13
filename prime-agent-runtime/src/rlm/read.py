"""yusAgent (YA-03, ticket 09): read a text file with line numbers, from the kernel.

The numbers let the model point at lines (for an edit, or a later `read(path, offset=...)`)
without a second read to locate them.
"""

from __future__ import annotations

import os

DEFAULT_LIMIT = 2000


def read(path: str, offset: int = 1, limit: int = DEFAULT_LIMIT) -> str:
    """Return up to `limit` lines of the text file at `path`, from line `offset` (1-based).

    Each line is `<number, right-aligned on 6>\\t<text>` (the `cat -n` layout); lines are split
    on `\\n` only, so the numbers match `grep -n` and editors, and a trailing `\\r` is dropped.
    Undecodable bytes are replaced. When lines remain after the window, a last line gives how
    many and the offset to continue from. `path` is relative to the working directory, absolute,
    or `~`-prefixed.
    """
    if not isinstance(path, str) or not path:
        raise TypeError("path must be a non-empty str")
    if not isinstance(offset, int) or isinstance(offset, bool) or offset < 1:
        raise ValueError("offset must be an int >= 1")
    if not isinstance(limit, int) or isinstance(limit, bool) or limit < 1:
        raise ValueError("limit must be an int >= 1")
    with open(os.path.expanduser(path), encoding="utf-8", errors="replace", newline="") as f:
        text = f.read()
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    total = len(lines)
    if total == 0:
        return f"({path} is empty)"
    if offset > total:
        return f"({path} has {total} lines; offset {offset} is past the end)"
    window = lines[offset - 1 : offset - 1 + limit]
    out = [f"{number:>6}\t{line.removesuffix(chr(13))}" for number, line in enumerate(window, start=offset)]
    last = offset - 1 + len(window)
    if last < total:
        out.append(f"... {total - last} more lines; read({path!r}, offset={last + 1}) continues")
    return "\n".join(out)
