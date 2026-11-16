"""yusAgent (YA-09, ticket 11): the bundled `edit` skill (`skills/edit/src/edit/__init__.py`).

An empty `old_str` creates a file; the result shows the changed lines with context, numbered
like `read()`; every refusal says why: missing text (0 occurrences), several occurrences, an
empty `old_str` on an existing file, a missing file, a path outside the workspace. The refusal
messages keep the prefixes `bench/smoke/metrics.py` (`EDIT_FAILURE`) counts.
"""

from __future__ import annotations

import asyncio
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

_SKILL_SRC = Path(__file__).resolve().parents[2] / "skills" / "edit" / "src"
sys.path.insert(0, str(_SKILL_SRC))
import edit as edit_skill  # noqa: E402

sys.path.remove(str(_SKILL_SRC))

# bench/smoke/metrics.py, EDIT_FAILURE (kept in sync by this test).
EDIT_FAILURE = re.compile(
    r"^(?:ValueError: (?:string not found in |found \d+ occurrences in |old_str is empty but )"
    r"|FileNotFoundError: .+ not found(?:$|; to create it)"
    r"|PermissionError: .+ is outside the workspace )"
)


def run(path: str, old_str: str, new_str: str) -> str:
    return asyncio.run(edit_skill.run(path, old_str, new_str))


def failure_line(error: BaseException) -> str:
    return f"{type(error).__name__}: {error}"


class EditTest(unittest.TestCase):
    def setUp(self) -> None:
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.addCleanup(setattr, edit_skill, "_WORKSPACE", edit_skill._WORKSPACE)
        edit_skill._WORKSPACE = self.root
        cwd = os.getcwd()
        self.addCleanup(os.chdir, cwd)
        os.chdir(self.root)

    def write(self, name: str, text: str) -> Path:
        path = self.root / name
        path.write_text(text, encoding="utf-8")
        return path

    def test_an_edit_returns_the_changed_lines_with_context(self):
        path = self.write("f.py", "".join(f"line {n}\n" for n in range(1, 11)))
        result = run("f.py", "line 5\n", "five\nfive bis\n")
        self.assertEqual(path.read_text(), "".join(
            f"line {n}\n" if n != 5 else "five\nfive bis\n" for n in range(1, 11)
        ))
        lines = result.split("\n")
        self.assertEqual(lines[0], f"Edited {path}")
        self.assertEqual(
            lines[1:],
            ["     2\tline 2", "     3\tline 3", "     4\tline 4", "     5\tfive", "     6\tfive bis",
             "     7\tline 6", "     8\tline 7", "     9\tline 8", "    10\tline 9"],
        )

    def test_the_context_stops_at_the_file_edges(self):
        self.write("g.txt", "a\nb\n")
        result = run("g.txt", "a", "A")
        self.assertEqual(result.split("\n")[1:], ["     1\tA", "     2\tb", "     3\t"])

    def test_an_empty_old_str_creates_a_file(self):
        result = run("pkg/new.py", "", "x = 1\ny = 2\n")
        path = self.root / "pkg" / "new.py"
        self.assertEqual(path.read_text(), "x = 1\ny = 2\n")
        self.assertTrue(result.startswith(f"Created {path}\n"), result)
        self.assertIn("     1\tx = 1", result)
        self.assertIn("     2\ty = 2", result)

    def test_refusals_say_why(self):
        self.write("h.txt", "same\nsame\nother\n")
        cases = [
            (("h.txt", "absent", "x"), ValueError, "string not found in h.txt (0 occurrences)"),
            (("h.txt", "same", "x"), ValueError, "found 2 occurrences in h.txt"),
            (("h.txt", "", "x"), ValueError, "old_str is empty but h.txt exists"),
            (("missing.txt", "a", "b"), FileNotFoundError, "missing.txt not found; to create it"),
        ]
        for args, kind, message in cases:
            with self.subTest(args=args):
                with self.assertRaises(kind) as raised:
                    run(*args)
                self.assertIn(message, str(raised.exception))
                self.assertRegex(failure_line(raised.exception), EDIT_FAILURE)
        self.assertEqual((self.root / "h.txt").read_text(), "same\nsame\nother\n")
        self.assertFalse((self.root / "missing.txt").exists())

    def test_a_path_outside_the_workspace_is_refused(self):
        with tempfile.TemporaryDirectory() as other:
            outside = Path(other) / "o.txt"
            outside.write_text("keep\n")
            for args in ((str(outside), "keep", "x"), (str(Path(other) / "n.txt"), "", "x")):
                with self.subTest(args=args):
                    with self.assertRaises(PermissionError) as raised:
                        run(*args)
                    self.assertIn("is outside the workspace", str(raised.exception))
                    self.assertRegex(failure_line(raised.exception), EDIT_FAILURE)
            self.assertEqual(outside.read_text(), "keep\n")
            self.assertFalse((Path(other) / "n.txt").exists())
        with self.assertRaises(PermissionError):
            run("../escape.txt", "", "x")


if __name__ == "__main__":
    unittest.main()
