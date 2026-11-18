"""yusAgent (YA-09, ticket 15): a misuse of the `bash()` objects fails with what to do.

Cases taken from the failed cells of the smoke bench (`docs/tickets/15-ya09-erreurs-visibles.md`):
- a `BashResult` field read on a `BashHandle` (`h.exit_code` before `await h`): the plain
  AttributeError did not say the command may still run, nor how to get the result;
- a `BashHandle` attribute read on a `BashResult` (`r.command` after `await`): the plain
  AttributeError did not say the result is not the handle;
- `h.output` without the call: printed `<bound method BashHandle.output ...>` instead of
  failing (a wrong result, silent). The attribute stays a method: `h.output()` is unchanged; any
  use of the uncalled attribute as text raises.

Only the error messages change: every valid use behaves as before, and a missing attribute still
raises AttributeError (so `getattr(h, name, default)` and `hasattr` are unchanged).
"""

from __future__ import annotations

from typing import Any, Callable, NoReturn

# Fields of BashResult that a BashHandle does not have.
_RESULT_ONLY = ("exit_code", "duration")
# Public attributes of BashHandle that a BashResult does not have.
_HANDLE_ONLY = ("pid", "command", "tail", "poll", "kill")

OUTPUT_HINT = (
    "BashHandle.output is a method: call h.output() for the output so far, "
    "or read (await h).output"
)


def _missing(obj: object, name: str) -> NoReturn:
    # The interpreter's own message and attributes ("Did you mean" suggestions keep working).
    raise AttributeError(f"{type(obj).__name__!r} object has no attribute {name!r}", name=name, obj=obj)


def _handle_getattr(self: object, name: str) -> Any:
    if name in _RESULT_ONLY:
        raise AttributeError(
            f"BashHandle has no .{name}: the command may still run. "
            f"Use r = await h, then r.{name} (or h.poll(), None while running)"
        )
    _missing(self, name)


def _result_getattr(self: object, name: str) -> Any:
    if name in _HANDLE_ONLY:
        raise AttributeError(
            f"BashResult (a finished command: .exit_code, .output, .duration) has no .{name}: "
            f".{name} is on the BashHandle that bash() returned"
        )
    _missing(self, name)


class _BoundOutput:
    """`h.output` before the call: calling it is `h.output()`, any use as text raises."""

    __slots__ = ("_handle", "_method")

    def __init__(self, handle: object, method: Callable[..., str]) -> None:
        self._handle = handle
        self._method = method

    def __call__(self, *args: Any, **kwargs: Any) -> str:
        return self._method(self._handle, *args, **kwargs)

    def __repr__(self) -> str:
        return f"<{OUTPUT_HINT}>"

    def _refuse(self, *_args: Any, **_kwargs: Any) -> NoReturn:
        raise TypeError(OUTPUT_HINT)

    __str__ = __format__ = __getitem__ = __iter__ = __len__ = __contains__ = _refuse
    __add__ = __radd__ = __mod__ = __bool__ = _refuse

    def __getattr__(self, name: str) -> Any:
        if name.startswith("_"):
            # Introspection (`__doc__`, `__name__`, `__self__`, ...) reads the bound method. The
            # slots are read without this hook: unset (copy, unpickling), they raise, not recurse.
            try:
                method = object.__getattribute__(self, "_method")
                handle = object.__getattribute__(self, "_handle")
            except AttributeError:
                raise AttributeError(name) from None
            return getattr(method.__get__(handle), name)
        # `h.output.splitlines()`, `h.output.strip()`, ...
        raise AttributeError(OUTPUT_HINT)


class _OutputDescriptor:
    __slots__ = ("_method",)

    def __init__(self, method: Callable[..., str]) -> None:
        self._method = method

    def __get__(self, handle: object, owner: type | None = None) -> Any:
        if handle is None:
            return self._method
        return _BoundOutput(handle, self._method)


def install(handle_class: type, result_class: type) -> None:
    """Called once by `rlm.bash` at import, with its `BashHandle` and `BashResult`."""
    handle_class.__getattr__ = _handle_getattr  # type: ignore[attr-defined]
    result_class.__getattr__ = _result_getattr  # type: ignore[attr-defined]
    handle_class.output = _OutputDescriptor(handle_class.output)  # type: ignore[attr-defined]
