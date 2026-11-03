"""yusAgent (T05, YA-05): masked outputs read back from the conversation log."""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from unittest import mock

from rlm.masked import tool_output



def _log(rows: list[dict]) -> str:
    handle = tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False, encoding="utf-8")
    with handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")
    return handle.name


def _result(call_id: str, *texts: str) -> dict:
    content = [{"type": "text", "text": text} for text in texts]
    content.append({"type": "image", "data": "AAAA", "mimeType": "image/png"})
    return {"type": "message", "id": call_id + "-e", "message": {
        "role": "toolResult", "toolCallId": call_id, "toolName": "python_repl",
        "content": content, "isError": False}}


class ToolOutputTest(unittest.TestCase):
    def setUp(self):
        self.path = _log([
            {"type": "session", "id": "s"},
            {"type": "message", "message": {"role": "user", "content": "go"}},
            _result("c1", "x" * 5000),
            "not an object",
            _result("c2", "première", "seconde"),
        ])
        with open(self.path, "a", encoding="utf-8") as handle:
            handle.write("{truncated\n")
        self.addCleanup(os.unlink, self.path)

    def test_reads_the_full_text_from_the_given_log(self):
        self.assertEqual(tool_output("c1", log=self.path), "x" * 5000)
        self.assertEqual(tool_output("c2", log=self.path), "première\nseconde")

    def test_defaults_to_the_session_conversation_log(self):
        with mock.patch.dict(os.environ, {"PRIME_AGENT_CONVERSATION_LOG": self.path}):
            self.assertEqual(len(tool_output("c1")), 5000)

    def test_the_last_record_wins(self):
        with open(self.path, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(_result("c1", "branch")) + "\n")
        self.assertEqual(tool_output("c1", log=self.path), "branch")

    def test_unknown_id_and_unpersisted_session_raise(self):
        with self.assertRaises(KeyError):
            tool_output("missing", log=self.path)
        with mock.patch.dict(os.environ, {"PRIME_AGENT_CONVERSATION_LOG": ""}):
            with self.assertRaises(RuntimeError):
                tool_output("c1")


if __name__ == "__main__":
    unittest.main()
