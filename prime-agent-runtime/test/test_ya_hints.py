"""yusAgent (YA-09, ticket 15): a misuse of the kernel helpers fails with what to do.

Each case comes from a failed cell of the smoke bench (`docs/tickets/15-ya09-erreurs-visibles.md`):
`h.exit_code` before `await h`, `r.command` on a BashResult, `h.output` without the call,
`await read(path)`, `edit(old=..., new=...)`. Valid uses are unchanged, and a missing attribute
still raises AttributeError with the interpreter's message.
"""

from __future__ import annotations

import asyncio
import copy
import inspect
import os
import pickle
import sys
import tempfile
import unittest
from pathlib import Path

import dill

from rlm import bash, read
from rlm import ya_hints
from rlm.bash import BashHandle, BashResult
from rlm.read import AWAIT_HINT, ReadText

_SKILL_SRC = Path(__file__).resolve().parents[2] / "skills" / "edit" / "src"
sys.path.insert(0, str(_SKILL_SRC))
import edit as edit_skill  # noqa: E402

sys.path.remove(str(_SKILL_SRC))


def _finished(command: str = "echo hi") -> tuple[BashHandle, BashResult]:
    async def go() -> tuple[BashHandle, BashResult]:
        handle = bash(command)
        return handle, await handle

    return asyncio.run(go())


class HandleTest(unittest.TestCase):
    def test_exit_code_before_await_says_to_await(self):
        handle = bash("sleep 5")
        self.addCleanup(handle.kill)
        with self.assertRaises(AttributeError) as caught:
            handle.exit_code  # noqa: B018
        message = str(caught.exception)
        self.assertIn("BashHandle has no .exit_code", message)
        self.assertIn("r = await h", message)
        self.assertIn("h.poll()", message)
        with self.assertRaisesRegex(AttributeError, r"BashHandle has no \.duration"):
            handle.duration  # noqa: B018

    def test_other_missing_attributes_keep_the_interpreter_message(self):
        handle, _ = _finished()
        with self.assertRaises(AttributeError) as caught:
            handle.nope  # noqa: B018
        self.assertEqual(str(caught.exception), "'BashHandle' object has no attribute 'nope'")
        self.assertEqual(caught.exception.name, "nope")
        self.assertIs(caught.exception.obj, handle)
        self.assertFalse(hasattr(handle, "exit_code"))
        self.assertIsNone(getattr(handle, "exit_code", None))
        self.assertIsNone(getattr(handle, "_not_set", None))

    def test_valid_uses_are_unchanged(self):
        handle, result = _finished("echo hi")
        self.assertEqual(handle.output(), "hi\n")
        self.assertEqual(handle.tail(1), "hi")
        self.assertIs(handle.poll(), result)
        self.assertEqual(handle.command, "echo hi")
        # On the class, `output` is still the plain function.
        self.assertTrue(inspect.isfunction(BashHandle.output))

    def test_output_without_the_call_cannot_pass_as_text(self):
        handle, _ = _finished("echo hi")
        uncalled = handle.output
        self.assertEqual(uncalled(), "hi\n")
        self.assertEqual(repr(uncalled), f"<{ya_hints.OUTPUT_HINT}>")
        for use in (
            lambda: print(uncalled),
            lambda: f"{uncalled}",
            lambda: str(uncalled),
            lambda: uncalled[-10:],
            lambda: "hi" in uncalled,
            lambda: len(uncalled),
            lambda: bool(uncalled),
            lambda: uncalled + "x",
            lambda: "x" + uncalled,
        ):
            with self.assertRaises(TypeError) as caught:
                use()
            self.assertEqual(str(caught.exception), ya_hints.OUTPUT_HINT)
        with self.assertRaisesRegex(AttributeError, "call h.output()"):
            uncalled.splitlines()
        # Introspection still reads the method, and copying never recurses.
        self.assertEqual(uncalled.__name__, "output")
        self.assertIs(uncalled.__self__, handle)
        self.assertEqual(copy.copy(uncalled)(), "hi\n")


class ResultTest(unittest.TestCase):
    def test_handle_attribute_on_a_result_says_where_it_is(self):
        _, result = _finished()
        for name in ("command", "pid", "tail", "poll", "kill"):
            with self.assertRaises(AttributeError) as caught:
                getattr(result, name)
            message = str(caught.exception)
            self.assertIn(f"BashResult (a finished command: .exit_code, .output, .duration) has no .{name}", message)
            self.assertIn("BashHandle that bash() returned", message)

    def test_result_is_unchanged(self):
        _, result = _finished("echo hi")
        self.assertEqual(result.exit_code, 0)
        self.assertEqual(result.output, "hi\n")
        self.assertFalse(result.running)
        self.assertTrue(repr(result).startswith("BashResult(exit_code=0, output='hi\\n', duration="))
        self.assertEqual(pickle.loads(pickle.dumps(result)), result)
        self.assertEqual(dill.loads(dill.dumps(result)), result)
        self.assertEqual(copy.copy(result), result)
        with self.assertRaises(AttributeError) as caught:
            result.nope  # noqa: B018
        self.assertEqual(str(caught.exception), "'BashResult' object has no attribute 'nope'")


class ReadTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.path = os.path.join(directory.name, "notes.txt")
        Path(self.path).write_text("alpha\nbeta\n", encoding="utf-8")

    def test_await_says_read_is_not_async(self):
        async def awaited():
            return await read(self.path)

        with self.assertRaises(TypeError) as caught:
            asyncio.run(awaited())
        self.assertEqual(str(caught.exception), AWAIT_HINT)

        async def gathered():
            return await asyncio.gather(read(self.path))

        with self.assertRaisesRegex(TypeError, "read\\(\\) is not async"):
            asyncio.run(gathered())

    def test_the_text_is_a_plain_str_otherwise(self):
        text = read(self.path)
        self.assertIsInstance(text, str)
        self.assertEqual(text, "     1\talpha\n     2\tbeta")
        self.assertEqual(repr(text), repr(str(text)))
        self.assertIs(type(text.splitlines()[0]), str)
        self.assertIs(type(text + ""), str)
        for restored in (pickle.loads(pickle.dumps(text)), dill.loads(dill.dumps(text))):
            self.assertIs(type(restored), str)
            self.assertEqual(restored, text)
        self.assertIsInstance(read(self.path, offset=9), ReadText)


class EditTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(dir=os.getcwd())
        self.addCleanup(directory.cleanup)
        self.path = os.path.join(directory.name, "notes.txt")
        Path(self.path).write_text("alpha\n", encoding="utf-8")

    def test_a_wrong_keyword_names_the_call_shape(self):
        with self.assertRaises(TypeError) as caught:
            asyncio.run(edit_skill.run(self.path, old="alpha", new="beta"))
        self.assertEqual(
            str(caught.exception),
            "edit(path, old_str, new_str): got an unexpected keyword argument 'old'",
        )
        with self.assertRaisesRegex(TypeError, r"^edit\(path, old_str, new_str\): missing a required argument: 'new_str'$"):
            asyncio.run(edit_skill.run(self.path, "alpha"))
        self.assertEqual(Path(self.path).read_text(encoding="utf-8"), "alpha\n")

    def test_valid_calls_and_signature_are_unchanged(self):
        self.assertEqual(list(inspect.signature(edit_skill.run).parameters), ["path", "old_str", "new_str"])
        self.assertIn("Replace a unique string", edit_skill.run.__doc__)
        out = asyncio.run(edit_skill.run(path=self.path, old_str="alpha", new_str="beta"))
        self.assertTrue(out.startswith("Edited "), out)
        self.assertEqual(Path(self.path).read_text(encoding="utf-8"), "beta\n")

    def test_the_shell_cli_still_parses_the_signature(self):
        import tyro

        result = tyro.cli(edit_skill.run, args=["--path", self.path, "--old-str", "alpha", "--new-str", "gamma"])
        asyncio.run(result)
        self.assertEqual(Path(self.path).read_text(encoding="utf-8"), "gamma\n")


if __name__ == "__main__":
    unittest.main()
