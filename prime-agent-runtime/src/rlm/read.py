"""yusAgent (YA-03, ticket 09): read a text file with line numbers, from the kernel.

The numbers let the model point at lines (for an edit, or a later `read(path, offset=...)`)
without a second read to locate them.

Ticket 15 (YA-09): the text is a `str` that refuses `await` with what to do (`await read(path)`
gave "object str can't be used in 'await' expression").
"""

from __future__ import annotations

import os

DEFAULT_LIMIT = 2000

AWAIT_HINT = "read() is not async: call read(path) without await"


class ReadText(str):
    """The text `read()` returns: a plain `str` everywhere, except under `await`."""

    __slots__ = ()

    def __await__(self):  # noqa: ANN204
        raise TypeError(AWAIT_HINT)

    def __reduce__(self):  # noqa: ANN204
        # Pickled (kernel snapshot) as a plain str: a restore never needs this class.
        return (str, (str(self),))


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
        return ReadText(f"({path} is empty)")
    if offset > total:
        return ReadText(f"({path} has {total} lines; offset {offset} is past the end)")
    window = lines[offset - 1 : offset - 1 + limit]
    out = [f"{number:>6}\t{line.removesuffix(chr(13))}" for number, line in enumerate(window, start=offset)]
    last = offset - 1 + len(window)
    if last < total:
        out.append(f"... {total - last} more lines; read({path!r}, offset={last + 1}) continues")
    return ReadText("\n".join(out))
