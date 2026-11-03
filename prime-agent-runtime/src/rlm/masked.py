"""yusAgent (T05, YA-05): read back a `python_repl` output the harness masked.

With `masking.enabled`, the harness replaces old cell outputs in the context sent to the model
by a placeholder that names this call:

    from rlm.masked import tool_output; print(tool_output("<tool call id>"))

Masking never touches the session file: the conversation log
(`PRIME_AGENT_CONVERSATION_LOG`) keeps every output in full, and `tool_output` reads it there.
"""

from __future__ import annotations

import json
import os

__all__ = ["tool_output"]


def tool_output(call_id: str, log: str | None = None) -> str:
    """Text of the tool result recorded for `call_id`, text blocks joined by newlines.

    `log` defaults to this session's conversation log. The last record wins when the id
    appears more than once (a branch of the session tree).
    """
    path = log or os.environ.get("PRIME_AGENT_CONVERSATION_LOG") or ""
    if not path:
        raise RuntimeError("this session is not persisted: there is no conversation log to read")
    found = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            message = entry.get("message") if isinstance(entry, dict) else None
            if (
                isinstance(message, dict)
                and message.get("role") == "toolResult"
                and message.get("toolCallId") == call_id
            ):
                found = message
    if found is None:
        raise KeyError(f"no tool output {call_id!r} in {path}")
    return "\n".join(
        block.get("text", "")
        for block in found.get("content") or []
        if isinstance(block, dict) and block.get("type") == "text"
    )
