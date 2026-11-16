"""yusAgent (YA-09): `bash(cmd, timeout=...)` (`rlm/ya_timeout.py`).

A command still running after its limit gets SIGTERM, then SIGKILL 5 s later; a line in its
output and the `timeoutSeconds` field of its `bash.completed` notice say so. The session default
(`ya_timeout.DEFAULT_TIMEOUT`) is None unless the kernel bootstrap sets it; a persistent shell has
no default limit.
"""

from __future__ import annotations

import math
import os
import sys
import threading
import time
import unittest

from rlm import bash
from rlm import ya_timeout
from test_repl import ReplProcess, one, reply_ok, wait_for_host_request

bash_module = sys.modules["rlm.bash"]

_KILLED_LINE = "[yusagent: killed, still running after its 1 s timeout]"


def _gone(pid: int, within: float) -> bool:
    deadline = time.monotonic() + within
    while time.monotonic() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        time.sleep(0.05)
    return False


class ResolveTest(unittest.TestCase):
    def setUp(self) -> None:
        self.addCleanup(setattr, ya_timeout, "DEFAULT_TIMEOUT", ya_timeout.DEFAULT_TIMEOUT)

    def test_unset_takes_the_session_default(self):
        self.assertIsNone(ya_timeout.DEFAULT_TIMEOUT)
        self.assertIsNone(ya_timeout.resolve(ya_timeout.UNSET, persistent=False))
        ya_timeout.DEFAULT_TIMEOUT = 600.0
        self.assertEqual(ya_timeout.resolve(ya_timeout.UNSET, persistent=False), 600.0)

    def test_a_persistent_shell_has_no_default_limit_but_keeps_an_explicit_one(self):
        ya_timeout.DEFAULT_TIMEOUT = 600.0
        self.assertIsNone(ya_timeout.resolve(ya_timeout.UNSET, persistent=True))
        self.assertEqual(ya_timeout.resolve(30, persistent=True), 30.0)

    def test_none_lifts_the_limit(self):
        ya_timeout.DEFAULT_TIMEOUT = 600.0
        self.assertIsNone(ya_timeout.resolve(None, persistent=False))

    def test_invalid_values_are_refused(self):
        for value in (True, "600", [1]):
            with self.assertRaises(TypeError, msg=repr(value)):
                ya_timeout.resolve(value, persistent=False)
        for value in (0, -1, math.nan, math.inf):
            with self.assertRaises(ValueError, msg=repr(value)):
                ya_timeout.resolve(value, persistent=False)
        with self.assertRaises(ValueError):
            bash("true", timeout=0)


class TimeoutTest(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.addCleanup(setattr, ya_timeout, "DEFAULT_TIMEOUT", ya_timeout.DEFAULT_TIMEOUT)

    async def test_a_command_past_its_limit_is_stopped_and_says_so(self):
        started = time.monotonic()
        handle = bash("sleep 30", timeout=1)
        handle.pid  # a background handle: awaits only wait
        result = await handle
        self.assertLess(time.monotonic() - started, 4.0)
        self.assertNotEqual(result.exit_code, 0)
        self.assertIn(_KILLED_LINE, handle.output())
        self.assertEqual(ya_timeout.notice_fields(handle), {"timeoutSeconds": 1.0})

    async def test_the_whole_process_group_is_stopped(self):
        handle = bash("sleep 30 & echo child=$!; wait", timeout=1)
        await handle
        child = int(handle.output().split("child=")[1].split()[0])
        self.assertTrue(_gone(child, 3.0), f"background child {child} survived the timeout")

    async def test_a_command_ignoring_sigterm_is_killed_after_the_grace(self):
        started = time.monotonic()
        handle = bash("trap '' TERM; sleep 30", timeout=1)
        handle.pid
        await handle
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 1.0 + 5.0 - 0.5)
        self.assertLess(elapsed, 10.0)

    async def test_a_command_that_ends_first_cancels_its_timer(self):
        handle = bash("printf ok", timeout=5)
        result = await handle
        self.assertEqual(result.exit_code, 0)
        timer = handle._ya_timeout_timer
        timer.join(1.0)
        self.assertFalse(timer.is_alive())
        self.assertEqual(ya_timeout.notice_fields(handle), {})
        self.assertNotIn("yusagent: killed", handle.output())

    async def test_without_a_limit_no_timer_is_armed(self):
        handle = bash("printf ok")
        await handle
        self.assertFalse(hasattr(handle, "_ya_timeout_timer"))
        handle = bash("printf ok", timeout=None)
        await handle
        self.assertFalse(hasattr(handle, "_ya_timeout_timer"))

    async def test_the_session_default_applies_without_an_argument(self):
        ya_timeout.DEFAULT_TIMEOUT = 1.0
        handle = bash("sleep 30")
        handle.pid
        await handle
        self.assertIn(_KILLED_LINE, handle.output())

    async def test_timers_are_daemon_threads(self):
        handle = bash("sleep 30", timeout=60)
        self.addCleanup(handle.kill)
        self.assertTrue(handle._ya_timeout_timer.daemon)
        self.assertIn(handle._ya_timeout_timer, threading.enumerate())


class TimeoutNoticeTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repl = ReplProcess()
        self.addCleanup(self.repl.close)
        self.repl.ready()

    def test_the_completion_notice_carries_the_limit(self):
        started = self.repl.execute(
            "bash-timeout", "from rlm import bash\nhandle = bash('sleep 30', timeout=1)\nhandle.pid"
        )
        pid = int(one(started, "result")["text"])
        request = wait_for_host_request(self.repl, started)
        data = request["data"]
        self.assertEqual(data["type"], "bash.completed")
        self.assertEqual(data["pid"], pid)
        self.assertEqual(data["command"], "sleep 30")
        self.assertNotEqual(data["exitCode"], 0)
        self.assertEqual(data["timeoutSeconds"], 1.0)
        reply_ok(self.repl, request)

    def test_a_notice_without_timeout_has_no_field(self):
        started = self.repl.execute(
            "bash-plain", "from rlm import bash\nhandle = bash('sleep 0.05', timeout=30)\nhandle.pid"
        )
        request = wait_for_host_request(self.repl, started)
        self.assertNotIn("timeoutSeconds", request["data"])
        reply_ok(self.repl, request)


if __name__ == "__main__":
    unittest.main()
