"""yusAgent (T05, YA-05): masked outputs read back from the log; failed bash() reads tagged."""

from __future__ import annotations

import asyncio
import json
import os
import sys
import tempfile
import unittest
from unittest import mock

import rlm.repl as repl_module
from rlm import bash
from rlm.masked import tool_output

bash_module = sys.modules["rlm.bash"]
EXIT_MIME = "application/vnd.prime-agent.bash-exit+json"


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


class FailedExitTest(unittest.IsolatedAsyncioTestCase):
    async def _exit_payloads(self, command: str) -> list[dict]:
        emitted = []
        task = asyncio.current_task()
        with mock.patch.object(bash_module, "_live_cell_owner", return_value=task), \
                mock.patch.object(bash_module, "_creating_cell_waits_for", return_value=True), \
                mock.patch.object(repl_module, "is_active", return_value=True), \
                mock.patch.object(repl_module, "emit", side_effect=emitted.append):
            await bash(command)
        return [data[EXIT_MIME] for data in emitted if EXIT_MIME in data]

    async def test_a_failed_result_read_by_the_cell_is_tagged(self):
        self.assertEqual(await self._exit_payloads("exit 3"), [{"exitCode": 3}])

    async def test_a_successful_result_is_not_tagged(self):
        self.assertEqual(await self._exit_payloads("true"), [])


if __name__ == "__main__":
    unittest.main()
