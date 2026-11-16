"""yusAgent (YA-09): `bash(cmd, timeout=...)`, a command stopped after its time limit.

Ported by hand from Unharnessed (MIT, commit 569284b36, `unharnessed-runtime/src/rlm/bash.py`;
notice in `docs/fork-changes.md`, "Code repris d'Unharnessed"). A command whose process is still
running after `timeout` seconds gets SIGTERM, then SIGKILL 5 s later (`BashHandle.kill`). A line in
its output says so, and its `bash.completed` notice carries `timeoutSeconds`, so the model is
woken with the reason.

Defaults: `DEFAULT_TIMEOUT` (module global) is the limit of a `bash()` call that names none. It is
None (no limit) unless the kernel bootstrap sets it for the session. A persistent shell
(`persistent=True`) has no limit by default; an explicit `timeout` applies to it too, but its
timer lives in this kernel only and does not survive a kernel restart.
"""

from __future__ import annotations

import math
import threading
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .bash import BashHandle

# The limit of a bash() call without `timeout`, None for no limit; set by the kernel bootstrap.
DEFAULT_TIMEOUT: float | None = None


class _Unset:
    def __repr__(self) -> str:
        return "<session default>"


UNSET: Any = _Unset()


def resolve(timeout: object, persistent: bool) -> float | None:
    """The limit of one bash() call: the session default when unset (none for a persistent
    shell), else the given number of seconds, or None for no limit."""
    if timeout is UNSET:
        return None if persistent else DEFAULT_TIMEOUT
    if timeout is None:
        return None
    if isinstance(timeout, bool) or not isinstance(timeout, (int, float)):
        raise TypeError("timeout must be a number of seconds, or None for no limit")
    # Also refuses NaN and inf: threading.Timer cannot wait past TIMEOUT_MAX.
    if math.isnan(timeout) or not 0 < timeout <= threading.TIMEOUT_MAX:
        raise ValueError("timeout must be a positive number of seconds, or None for no limit")
    return float(timeout)


def arm(handle: BashHandle, seconds: float) -> None:
    """Start the timer of a new handle; a command that ends first cancels it."""
    timer = threading.Timer(seconds, _expire, args=(handle, seconds))
    timer.daemon = True
    handle._ya_timeout_timer = timer
    timer.start()
    handle._add_done_callback(timer.cancel)


def _expire(handle: BashHandle, seconds: float) -> None:
    if handle._done.is_set() or handle._reaped:
        return
    handle._ya_timed_out = seconds
    from .bash import _BoundedBuffer

    if isinstance(handle._buffer, _BoundedBuffer):
        handle._buffer.write(
            f"\n[yusagent: killed, still running after its {seconds:g} s timeout]\n".encode()
        )
    handle.kill()


def notice_fields(handle: BashHandle) -> dict[str, float]:
    """Extra fields of the `bash.completed` notice: `timeoutSeconds` for a command stopped by its
    limit, nothing otherwise."""
    seconds = getattr(handle, "_ya_timed_out", None)
    return {} if seconds is None else {"timeoutSeconds": seconds}
