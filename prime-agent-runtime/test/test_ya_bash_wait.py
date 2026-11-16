"""yusAgent (YA-09, lever `bash()`): the `await bash(cmd)` window set by the kernel bootstrap.

`bashWaitSeconds` (`crates/pa-core/src/kernel/bootstrap/ya_bash_wait.rs`) adds, after
`_set_completion_notices_wired(True)`, the line
`__import__("sys").modules["rlm.bash"]._YIELD_AFTER_SECONDS = <n>.0`, or `= None` for `"full"`
(ticket 13: no window, the await returns when the command ends). These tests run that line and
check what `await bash(cmd)` does with it, and how long a cancel takes inside a long or unbounded
window (CDC, YA-10: `session/cancel` stops the turn in under 1 s, `bash()` subprocesses
included).
"""

from __future__ import annotations

import asyncio
import sys
import time
import unittest

from rlm import bash
from test_repl import ReplProcess, one, reply_ok, wait_for_host_request

bash_module = sys.modules["rlm.bash"]


def _set_window(seconds: float | None) -> None:
    # The statement the bootstrap runs, verbatim apart from the value.
    exec(f'__import__("sys").modules["rlm.bash"]._YIELD_AFTER_SECONDS = {seconds!r}', {})


class BashWaitTest(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        upstream = bash_module._YIELD_AFTER_SECONDS
        self.addCleanup(setattr, bash_module, "_YIELD_AFTER_SECONDS", upstream)
        bash_module._set_completion_notices_wired(True)
        self.addCleanup(bash_module._set_completion_notices_wired, False)
        hints = bash_module._degrade_hints_written
        self.addCleanup(setattr, bash_module, "_degrade_hints_written", hints)

    async def test_the_upstream_window_is_one_second(self):
        self.assertEqual(bash_module._YIELD_AFTER_SECONDS, 1.0)

    async def test_the_bootstrap_line_sets_the_window(self):
        _set_window(30.0)
        self.assertEqual(bash_module._YIELD_AFTER_SECONDS, 30.0)

    async def test_a_command_inside_the_window_returns_its_result(self):
        _set_window(5.0)
        result = await bash("sleep 1.5; printf done")
        self.assertIsInstance(result, bash_module.BashResult)
        self.assertEqual(result.exit_code, 0)
        self.assertIn("done", result.output)

    async def test_a_command_past_the_window_still_returns_a_snapshot(self):
        _set_window(1.0)
        returned = await bash("sleep 1.4")
        self.assertIsInstance(returned, bash_module.BashSnapshot)
        self.assertTrue(returned.running)
        result = await returned.handle
        self.assertEqual(result.exit_code, 0)

    async def test_the_bootstrap_line_sets_no_window_for_full(self):
        _set_window(None)
        self.assertIsNone(bash_module._YIELD_AFTER_SECONDS)

    async def test_full_waits_past_the_old_windows_for_the_result(self):
        _set_window(None)
        result = await bash("sleep 3; printf done")
        self.assertIsInstance(result, bash_module.BashResult)
        self.assertEqual(result.exit_code, 0)
        self.assertIn("done", result.output)
        self.assertGreaterEqual(result.duration, 3.0)

    async def _cancel_after(self, command: str, delay: float) -> float:
        """Seconds from the cancel of an `await bash(command)` to the end of its task."""
        task = asyncio.ensure_future(_await(command))
        await asyncio.sleep(delay)
        started = time.monotonic()
        task.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await task
        return time.monotonic() - started

    async def test_a_cancel_inside_a_long_window_stops_the_command_in_under_one_second(self):
        _set_window(30.0)
        elapsed = await self._cancel_after("sleep 30", 0.5)
        self.assertLess(elapsed, 1.0)

    async def test_a_cancel_of_a_command_ignoring_sigterm_takes_the_kill_grace(self):
        # The cancel sends SIGTERM, waits _CANCEL_TERM_GRACE (0.5 s), then SIGKILL: the window
        # does not change this path.
        _set_window(30.0)
        elapsed = await self._cancel_after("trap '' TERM; sleep 30", 0.5)
        self.assertGreaterEqual(elapsed, bash_module._CANCEL_TERM_GRACE)
        self.assertLess(elapsed, 1.0)


    async def test_a_cancel_of_a_full_wait_stops_the_command_in_under_one_second(self):
        _set_window(None)
        elapsed = await self._cancel_after("sleep 30", 0.5)
        self.assertLess(elapsed, 1.0)

    async def test_a_cancel_of_a_full_wait_ignoring_sigterm_takes_the_kill_grace(self):
        _set_window(None)
        elapsed = await self._cancel_after("trap '' TERM; sleep 30", 0.5)
        self.assertGreaterEqual(elapsed, bash_module._CANCEL_TERM_GRACE)
        self.assertLess(elapsed, 1.0)


class FullWaitNoticeTest(unittest.TestCase):
    """In a kernel with the notices wired and no window, a command awaited to its end by its cell
    sends no `bash.completed` notice (no `[bash-done]` wake); one left running by its cell does."""

    def setUp(self) -> None:
        self.repl = ReplProcess()
        self.addCleanup(self.repl.close)
        self.repl.ready()
        wired = self.repl.execute(
            "wire",
            "import rlm\nrlm._set_completion_notices_wired(True)\n"
            '__import__("sys").modules["rlm.bash"]._YIELD_AFTER_SECONDS = None',
        )
        self.assertIsNone(one(wired, "error"), wired)

    def test_an_awaited_command_sends_no_notice(self):
        awaited = self.repl.execute(
            "full-await",
            "from rlm import bash\nr = await bash('sleep 3; printf done')\n(type(r).__name__, r.exit_code, r.output)",
        )
        self.assertEqual(one(awaited, "result")["text"], "('BashResult', 0, 'done')")
        later = self.repl.execute("later", "import asyncio\nawait asyncio.sleep(1)")
        self.assertIsNone(one(awaited + later, "host_request"), awaited + later)
        # Control: a command its cell leaves running still notifies.
        started = self.repl.execute("background", "h = bash('sleep 0.2')\nh.pid")
        pid = int(one(started, "result")["text"])
        request = wait_for_host_request(self.repl, started)
        self.assertEqual(request["data"]["type"], "bash.completed")
        self.assertEqual(request["data"]["pid"], pid)
        reply_ok(self.repl, request)


async def _await(command: str):
    return await bash(command)


if __name__ == "__main__":
    unittest.main()
