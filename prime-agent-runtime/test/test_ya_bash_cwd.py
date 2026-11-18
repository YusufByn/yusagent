"""yusAgent (YA-03, ticket 16): shell state of `bash()`.

Each call is a new process started in the kernel's cwd and environment: `os.chdir()` in the
kernel applies to the next `bash()`, a `cd` or an `export` inside a command does not outlive it.
`os.environ` in the kernel is covered by `test_ya_read_env.py`.
"""

from __future__ import annotations

import os
import tempfile
import unittest

from rlm import bash


class BashCwdTest(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = os.path.realpath(self._tmp.name)
        os.mkdir(os.path.join(self.dir, "sub"))
        self._cwd = os.getcwd()
        self.addCleanup(self._tmp.cleanup)
        self.addCleanup(os.chdir, self._cwd)

    async def test_os_chdir_in_the_kernel_applies_to_the_next_bash(self):
        os.chdir(self.dir)
        result = await bash("pwd -P")
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(result.output.strip(), self.dir)

    async def test_cd_inside_a_command_does_not_outlive_it(self):
        os.chdir(self.dir)
        moved = await bash("cd sub && pwd -P")
        self.assertEqual(moved.output.strip(), os.path.join(self.dir, "sub"))
        after = await bash("pwd -P")
        self.assertEqual(after.output.strip(), self.dir)
        self.assertEqual(os.getcwd(), self.dir)

    async def test_export_inside_a_command_does_not_outlive_it(self):
        await bash("export YA_CWD_EXPORTED=1")
        after = await bash("printf '%s' \"${YA_CWD_EXPORTED:-unset}\"")
        self.assertEqual(after.output, "unset")
        self.assertNotIn("YA_CWD_EXPORTED", os.environ)


if __name__ == "__main__":
    unittest.main()
