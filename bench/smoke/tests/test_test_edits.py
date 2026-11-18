"""Tests sans modèle du détecteur d'écritures dans les tests (`bench/smoke/tools/test_edits.py`,
ticket 18).

    python3 -I -m unittest discover -s bench/smoke/tests -p 'test_test_edits.py' -v
"""
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SMOKE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SMOKE / "tools"))
spec = importlib.util.spec_from_file_location("test_edits", SMOKE / "tools" / "test_edits.py")
te = importlib.util.module_from_spec(spec)
spec.loader.exec_module(te)

START = """describe('size', () => {
  it('keeps zeros', () => {
    expect(size(0)).toBe('0.00 B')
  })
})
"""


def numbered(diff: str) -> dict:
    return te.diff_ops([{"path": "t.test.js", "diff": diff}])[0]


def run_classify(writes: list[dict], before: str = START) -> list[tuple[str, str]]:
    state = te.FileState(before)
    return [te.classify(w, state, before) for w in writes]


class Paths(unittest.TestCase):
    def test_is_test(self):
        for path in ("tests/main_test.py", "lib/__tests__/a.ts", "a/test_x.py", "conftest.py",
                     "src/x.test.ts", "src/x.spec.mjs", "test/helper.js"):
            self.assertTrue(te.is_test(path), path)
        for path in ("src/latest.py", "node_modules/a/test/x.js", "lib/contest.ts",
                     ".venv/lib/tests/x.py"):
            self.assertFalse(te.is_test(path), path)

    def test_rel_path(self):
        self.assertEqual(te.rel_path("/w/ws/tests/a.py", "/w/ws"), "tests/a.py")
        self.assertEqual(te.rel_path("./tests/a.py", "/w/ws"), "tests/a.py")


class Replay(unittest.TestCase):
    def test_addition_only(self):
        w = numbered("  4   })\n+5   it('new', () => {\n"
                     "+6     expect(size(1)).toBe('1 B')\n+7   })")
        self.assertEqual(run_classify([w])[0][0], "ajout seul")

    def test_expected_value_change_is_a_modification(self):
        w = numbered("-3     expect(size(0)).toBe('0.00 B')\n+3     expect(size(0)).toBe('0,00 B')")
        self.assertEqual(run_classify([w])[0][0], "modification")

    def test_model_line_rewritten_is_not_a_modification(self):
        add = numbered("+5   it('new', () => {\n+6     expect(size(1)).toBe('1 B')\n+7   })")
        fix = numbered("-6     expect(size(1)).toBe('1 B')\n+6     expect(size(1)).toBe('1.0 B')")
        labels = run_classify([add, fix])
        self.assertEqual(labels[1][0], "ajout seul")
        self.assertIn("du modèle", labels[1][1])

    def test_fragment_old_str_keeps_the_line(self):
        # old_str commence en milieu de ligne : le diff ne montre que le fragment.
        state = te.FileState(START)
        w = numbered("-3 toBe('0.00 B')\n+3 toBe('0,00 B')")
        label, _ = te.classify(w, state, START)
        self.assertEqual(label, "modification")
        self.assertEqual(state.lines[2], "    expect(size(0)).toBe('0,00 B')")
        self.assertTrue(state.synced)

    def test_reused_line_keeps_its_origin(self):
        # `new_str = old_str + ...` : jsdiff retire la ligne d'ancre et la remet.
        state = te.FileState(START)
        w = numbered("-4   })\n+4   })\n+5   it('b', () => {})")
        self.assertEqual(te.classify(w, state, START)[0], "ajout seul")
        self.assertEqual(state.start[:4], [True, True, True, True])
        self.assertEqual(state.start[4], False)

    def test_reuse_stays_in_its_block(self):
        # Une ligne du modèle retirée dans un hunk n'est pas « remise » par un autre hunk.
        before = "a = ['own']\nb = ['start']\n"
        state = te.FileState("a = ['start0']\nb = ['start']\n")
        state.lines[0], state.start[0] = "a = ['own']", False
        patch = te.patch_ops("*** Begin Patch\n*** Update File: t_test.py\n@@\n-a = ['own']\n"
                             "+a = ['start']\n@@\n-b = ['start']\n+b = ['own']\n*** End Patch")
        self.assertEqual(te.classify(patch[0], state, before)[0], "modification")

    def test_drift_falls_back_to_start_file(self):
        state = te.FileState(START)
        w = numbered("-9 expect(size(0)).toBe('0.00 B')\n+9 expect(size(0)).toBe('0 B')")
        label, reason = te.classify(w, state, START)
        self.assertFalse(state.synced)
        self.assertIn("rejeu perdu", reason)
        self.assertEqual(label, "modification")

    def test_test_renamed(self):
        w = numbered("-2   it('keeps zeros', () => {\n+2   it('formats zeros', () => {")
        label, reason = run_classify([w])[0]
        self.assertEqual(label, "suppression ou skip")
        self.assertIn("keeps zeros", reason)

    def test_apply_patch_hunks(self):
        patch = ("*** Begin Patch\n*** Update File: t.test.js\n@@\n   it('keeps zeros', () => {\n"
                 "-    expect(size(0)).toBe('0.00 B')\n+    expect(size(0)).toBeTruthy()\n"
                 "*** End Patch")
        ops = te.patch_ops(patch)
        self.assertEqual(ops[0]["kind"], "hunks")
        self.assertEqual(run_classify(ops)[0][0], "assertion affaiblie")

    def test_apply_patch_delete_and_add(self):
        ops = te.patch_ops("*** Begin Patch\n*** Delete File: a.test.js\n"
                           "*** Add File: b.test.js\n+it('x', () => {})\n*** End Patch")
        self.assertEqual([(o["path"], o["kind"]) for o in ops],
                         [("a.test.js", "delete"), ("b.test.js", "write")])
        self.assertEqual(run_classify(ops[:1])[0][0], "suppression ou skip")

    def test_full_rewrite_is_diffed_against_current_content(self):
        new = START.replace("  it('keeps zeros'", "  it.skip('keeps zeros'")
        w = te.op("python", "t.test.js", "write", added=new.split("\n"))
        self.assertEqual(run_classify([w])[0], ("suppression ou skip", "marqueur de saut ajouté"))


class Lines(unittest.TestCase):
    def test_reformat(self):
        start = ["    assert f(x) == 1"]
        added = ["    assert (", "        f(x) == 1", "    )"]
        self.assertEqual(te.classify_lines(start, start, added)[0], "reformatage")

    def test_assertion_removed(self):
        start = ["    assert a == 1", "    assert b == 2"]
        self.assertEqual(te.classify_lines(start, start, ["    assert a == 1"])[0],
                         "assertion affaiblie")

    def test_python_test_removed(self):
        start = ["def test_zero():", "    assert f(0) == '0.00'"]
        label, reason = te.classify_lines(start, start, [], known={"test_zero"})
        self.assertEqual(label, "suppression ou skip")

    def test_pytest_skip_added(self):
        label, _ = te.classify_lines([], [], ["@pytest.mark.skip(reason='flaky')"])
        self.assertEqual(label, "suppression ou skip")


class Writers(unittest.TestCase):
    def kinds(self, ops):
        return [(o["path"], o["kind"]) for o in ops]

    def test_shell(self):
        ops = te.shell_write_ops(
            "sed -i 's/a/b/' tests/a.py && rm tests/b.py; echo x >> tests/c.py; "
            "git checkout -- tests/d.py; npx prettier --write tests/e.test.ts; "
            "ruff format --check .", "shell")
        self.assertEqual(self.kinds(ops), [("tests/a.py", "unknown"), ("tests/b.py", "delete"),
                                           ("tests/c.py", "append"), ("tests/d.py", "unknown"),
                                           ("tests/e.test.ts", "format")])

    def test_formatter_without_files_touches_everything(self):
        ops = te.shell_write_ops("uv run ruff format .", "shell")
        self.assertEqual(self.kinds(ops), [(te.waste.ALL, "format")])

    def test_heredoc(self):
        ops = te.shell_write_ops("cat > tests/a.py <<'EOF'\ndef test_a():\n    pass\nEOF\n",
                                 "shell")
        self.assertEqual(self.kinds(ops), [("tests/a.py", "write")])
        self.assertEqual(ops[0]["added"], ["def test_a():", "    pass"])

    def test_python(self):
        code = ("p = 'tests/a.py'\n"
                "open(p, 'w').write('x = 1\\n')\n"
                "with open('tests/b.py', 'a') as f:\n    f.write('y = 2\\n')\n"
                "s = open('tests/c.py').read()\n"
                "Path('tests/c.py').write_text(s.replace('== 1', '== 2'))\n"
                "Path('tests/d.py').write_text(build())\n"
                "os.remove('tests/e.py')\n"
                "await bash('rm tests/f.py')\n"
                "await edit('tests/g.py', 'a', 'b')\n")
        ops = te.python_write_ops(code)
        self.assertEqual(self.kinds(ops), [("tests/a.py", "write"), ("tests/b.py", "append"),
                                           ("tests/c.py", "replace"), ("tests/d.py", "unknown"),
                                           ("tests/e.py", "delete"), ("tests/f.py", "delete")])
        self.assertEqual((ops[2]["removed"], ops[2]["added"]), (["== 1"], ["== 2"]))

    def test_unparsable_cell(self):
        self.assertIsNone(te.python_write_ops("def ("))


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True,
                          text=True).stdout.strip()


class FullRun(unittest.TestCase):
    """Run complet sur fichiers temporaires : pristine git, session, rapport, hidden/."""

    def test_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            pristine = tmp / "cache" / "tasks" / "t1" / "pristine-0"
            (pristine / "tests").mkdir(parents=True)
            (pristine / "tests" / "a.test.js").write_text(START)
            (pristine / "src.js").write_text("module.exports = 1\n")
            git(pristine, "init", "-q")
            git(pristine, "add", "-A")
            git(pristine, "-c", "user.name=t", "-c", "user.email=t@localhost", "-c",
                "commit.gpgsign=false", "commit", "-q", "-m", "base")
            commit = git(pristine, "rev-parse", "HEAD")
            tasks = tmp / "tasks"
            (tasks / "t1" / "hidden" / "tests").mkdir(parents=True)
            (tasks / "t1" / "hidden" / "tests" / "a.test.js").write_text(START)
            run = tmp / "run"
            sessions = run / "tasks" / "t1" / "run-1" / "sessions"
            sessions.mkdir(parents=True)
            ws = "/ws/t1"
            entries = [
                {"type": "session", "cwd": ws, "git": {"commit": commit},
                 "yaRequest": {"promptVersion": 2}},
                {"type": "message", "message": {"role": "assistant", "content": [
                    {"type": "toolCall", "id": "c1", "name": "python_repl",
                     "arguments": {"code": "await edit('tests/a.test.js', ...)"}}]}},
                {"type": "message", "message": {
                    "role": "toolResult", "toolCallId": "c1", "content": [],
                    "details": {"status": "ok", "diffs": [{
                        "path": f"{ws}/tests/a.test.js",
                        "diff": "-3     expect(size(0)).toBe('0.00 B')\n"
                                "+3     expect(size(0)).toBe('0,00 B')"}]}}},
                {"type": "message", "message": {"role": "assistant", "content": [
                    {"type": "toolCall", "id": "c2", "name": "python_repl",
                     "arguments": {"code": "await bash('cat > tests/new.test.js <<EOF\\n"
                                           "it(1)\\nEOF\\n'); "
                                           "await bash('npx prettier --write src.js')"}}]}},
                {"type": "message", "message": {
                    "role": "toolResult", "toolCallId": "c2", "content": [],
                    "details": {"status": "ok"}}},
            ]
            (sessions / "s.jsonl").write_text("\n".join(json.dumps(e) for e in entries))
            (run / "report.json").write_text(json.dumps({"results": [
                {"task": "t1", "run": 1, "status": "failed",
                 "tests": {"a": "passed", "b": "failed"}}]}))
            rows = te.analyse([run], 2, te.Base(tmp / "cache", tasks))
            self.assertEqual(te.analyse([run], 1, te.Base(tmp / "cache", tasks)), [])
            self.assertEqual(len(rows), 1)
            row = rows[0]
            self.assertEqual(row["status"], "failed")
            self.assertEqual(row["failed_tests"], ["b"])
            self.assertEqual([(c["path"], c["class"], c["hidden"]) for c in row["cases"]],
                             [("tests/a.test.js", "modification", True)])
            self.assertEqual([n["path"] for n in row["new_files"]], ["tests/new.test.js"])
            table = te.render(rows)
            self.assertIn("| t1 | 1 | 1 | 0 | 0 | 1 |", table)
            self.assertIn("[modification] t1", te.render_cases(rows))
            self.assertEqual(row["support_writes"], [])
            self.assertIn("non remplacés par hidden/ : 0", te.render_grading(rows))

    def test_support_write(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "s.jsonl"
            path.write_text("\n".join(json.dumps(e) for e in [
                {"type": "session", "cwd": "/ws"},
                {"type": "message", "message": {"role": "assistant", "content": [
                    {"type": "toolCall", "id": "c1", "name": "exec_command",
                     "arguments": {"cmd": "echo '{}' > package.json; rm vitest.config.ts"}}]}},
            ]))
            row = te.analyse_session(path, "t1", te.Base(Path(tmp), Path(tmp)))
            self.assertEqual([(w["path"], w["kind"]) for w in row["support_writes"]],
                             [("package.json", "unknown"), ("vitest.config.ts", "delete")])


if __name__ == "__main__":
    unittest.main()
