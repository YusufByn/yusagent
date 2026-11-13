"""yusAgent (YA-03, ticket 09): `read()` with line numbers and `bash(env=...)`."""

from __future__ import annotations

import os
import sys
import tempfile
import unittest

import rlm
from rlm import bash, read

bash_module = sys.modules["rlm.bash"]


class ReadTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = self._tmp.name

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _write(self, name: str, data: bytes) -> str:
        path = os.path.join(self.dir, name)
        with open(path, "wb") as f:
            f.write(data)
        return path

    def test_lines_are_numbered_like_cat_n(self):
        path = self._write("a.py", b"first\n\tsecond\nthird\n")
        self.assertEqual(read(path), "     1\tfirst\n     2\t\tsecond\n     3\tthird")

    def test_last_line_without_newline_and_crlf(self):
        path = self._write("b.txt", b"one\r\ntwo")
        self.assertEqual(read(path), "     1\tone\n     2\ttwo")

    def test_window_and_continuation(self):
        path = self._write("c.txt", "".join(f"l{i}\n" for i in range(1, 11)).encode())
        self.assertEqual(
            read(path, offset=4, limit=3),
            f"     4\tl4\n     5\tl5\n     6\tl6\n... 4 more lines; read({path!r}, offset=7) continues",
        )
        self.assertEqual(read(path, offset=9), "     9\tl9\n    10\tl10")

    def test_numbers_split_on_newline_only(self):
        # \x0c and   are line breaks for str.splitlines() but not for grep -n.
        path = self._write("d.txt", "a\x0cb c\nd\n".encode())
        self.assertEqual(read(path), "     1\ta\x0cb c\n     2\td")

    def test_empty_file_and_offset_past_the_end(self):
        empty = self._write("e.txt", b"")
        self.assertEqual(read(empty), f"({empty} is empty)")
        path = self._write("f.txt", b"x\n")
        self.assertEqual(read(path, offset=5), f"({path} has 1 lines; offset 5 is past the end)")

    def test_undecodable_bytes_are_replaced(self):
        path = self._write("g.bin", b"ok\xff\n")
        self.assertEqual(read(path), "     1\tok�")

    def test_relative_path_and_bad_arguments(self):
        self._write("h.txt", b"rel\n")
        cwd = os.getcwd()
        os.chdir(self.dir)
        try:
            self.assertEqual(read("h.txt"), "     1\trel")
        finally:
            os.chdir(cwd)
        with self.assertRaises(ValueError):
            read("h.txt", offset=0)
        with self.assertRaises(ValueError):
            read("h.txt", limit=0)
        with self.assertRaises(TypeError):
            read("")
        with self.assertRaises(FileNotFoundError):
            read(os.path.join(self.dir, "missing.txt"))

    def test_read_is_exported(self):
        self.assertIs(rlm.read, read)
        self.assertIn("read", rlm.__all__)


class BashEnvTest(unittest.IsolatedAsyncioTestCase):
    async def test_env_applies_to_this_command_only(self):
        result = await bash("printf '%s|%s' \"$YA_ENV_ONE\" \"$PAGER\"", env={"YA_ENV_ONE": "1", "PAGER": "less"})
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(result.output, "1|less")
        self.assertNotIn("YA_ENV_ONE", os.environ)
        again = await bash("printf '%s|%s' \"${YA_ENV_ONE:-unset}\" \"$PAGER\"")
        self.assertEqual(again.output, "unset|cat")

    async def test_env_keeps_the_kernel_environment(self):
        os.environ["YA_ENV_KERNEL"] = "k"
        try:
            result = await bash("printf '%s|%s' \"$YA_ENV_KERNEL\" \"$YA_ENV_TWO\"", env={"YA_ENV_TWO": "2"})
        finally:
            del os.environ["YA_ENV_KERNEL"]
        self.assertEqual(result.output, "k|2")

    def test_env_must_map_str_to_str(self):
        for bad in ({"A": 1}, {1: "a"}, [("A", "a")], "A=a"):
            with self.assertRaises(TypeError):
                bash("true", env=bad)

    def test_child_env_merges_extra_last(self):
        env = bash_module._child_env({"GIT_PAGER": "less", "YA_X": "x"})
        self.assertEqual(env["GIT_PAGER"], "less")
        self.assertEqual(env["YA_X"], "x")
        self.assertEqual(env["PAGER"], "cat")
        self.assertEqual(bash_module._child_env(), bash_module._child_env(None))


if __name__ == "__main__":
    unittest.main()
