"""yusAgent (YA-10, ticket 24): the bash-run display record of a bash() result the cell read."""

from __future__ import annotations

import asyncio
import sys
import unittest
from unittest import mock

import rlm.repl as repl_module
from rlm import bash

bash_module = sys.modules["rlm.bash"]
RUN_MIME = "application/vnd.yusagent.bash-run+json"


class BashRunRecordTest(unittest.IsolatedAsyncioTestCase):
    async def _runs(self, command: str, live_cell: bool = True) -> list[dict]:
        emitted = []
        task = asyncio.current_task() if live_cell else None
        with mock.patch.object(bash_module, "_live_cell_owner", return_value=task), \
                mock.patch.object(bash_module, "_creating_cell_waits_for", return_value=True), \
                mock.patch.object(repl_module, "is_active", return_value=True), \
                mock.patch.object(repl_module, "emit", side_effect=emitted.append):
            await bash(command)
        return [data[RUN_MIME] for data in emitted if RUN_MIME in data]

    async def test_a_result_read_by_the_cell_is_recorded(self):
        runs = await self._runs("echo hello; exit 3")
        self.assertEqual(len(runs), 1)
        run = runs[0]
        self.assertEqual(run["command"], "echo hello; exit 3")
        self.assertEqual(run["exitCode"], 3)
        self.assertEqual(run["output"].strip(), "hello")
        self.assertIsInstance(run["durationMs"], int)

    async def test_a_successful_result_is_recorded_too(self):
        runs = await self._runs("true")
        self.assertEqual([run["exitCode"] for run in runs], [0])

    async def test_no_record_outside_a_live_cell(self):
        self.assertEqual(await self._runs("true", live_cell=False), [])

    def test_the_output_is_capped_with_head_and_tail(self):
        output = "a" * 6000 + "b" * 6000
        capped = bash_module._run_output(output)
        self.assertLessEqual(len(capped), bash_module._BASH_RUN_OUTPUT_CAP)
        self.assertTrue(capped.startswith("a"))
        self.assertTrue(capped.endswith("b"))
        kept = len(capped) - len(capped.split("\n")[1]) - 2
        omitted = int(capped.split("[")[1].split(" ")[0])
        self.assertEqual(kept + omitted, len(output))
        self.assertEqual(bash_module._run_output("short"), "short")


if __name__ == "__main__":
    unittest.main()
