"""Tests sans modèle du détecteur de gaspillage (`bench/smoke/tools/waste.py`, ticket 17).

    python3 -I -m unittest discover -s bench/smoke/tests -p 'test_waste.py' -v
"""
from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

SMOKE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SMOKE))
spec = importlib.util.spec_from_file_location("waste", SMOKE / "tools" / "waste.py")
waste = importlib.util.module_from_spec(spec)
spec.loader.exec_module(waste)

PRICES = {"long_context_threshold_input_tokens": 272000,
          "models": {"m": {"short": {"input": 1.0, "cached_input": 0.1, "output": 10.0,
                                     "cache_write_30m": 1.25, "cache_write_12h": 2.5}}}}


def cell(code: str, result: str = "ok", error: bool = False, name: str = "python_repl",
         call: int = 0, args: dict | None = None) -> dict:
    return {"name": name, "code": code, "result": result, "error": error, "call": call,
            "args": args if args is not None else {"code": code}}


def session(*cells: dict) -> dict:
    cells = [dict(c, call=i) for i, c in enumerate(cells)]
    return {"path": "s.jsonl", "header": {}, "cells": cells,
            "calls": [{"usage": {"input": 100, "cacheRead": 0, "cacheWrite": 0, "output": 7},
                       "model": "m", "tools": 1} for _ in range(len(cells) + 1)]}


def kinds(found: dict) -> dict:
    return {k: [c["cell"] for c in v] for k, v in found.items() if v}


class ShellOps(unittest.TestCase):
    def test_reads(self):
        ops = waste.shell_ops('sed -n "10,20p" a.py; cat b.py | head -5; tail -n 3 c.py; '
                              "nl -ba d.py | sed -n '4,8p'; head -40 e.py f.py")
        reads = [(o[1], o[2]) for o in ops if o[0] == "read"]
        self.assertEqual(reads, [("a.py", (10, 20)), ("b.py", (1, 5)), ("c.py", ("tail", 3)),
                                 ("d.py", (4, 8)), ("e.py", (1, 40)), ("f.py", (1, 40))])

    def test_search_is_not_a_read(self):
        self.assertEqual(waste.shell_ops("cat a.py | grep foo; grep -n x a.py"), [])

    def test_writes(self):
        ops = waste.shell_ops("sed -i 's/a/b/' x.py && echo hi > y.txt; npm test > /tmp/t.log 2>&1")
        self.assertIn(("write", "x.py"), ops)
        self.assertIn(("write", "y.txt"), ops)
        self.assertNotIn(("write", "/tmp/t.log"), ops)
        self.assertIn(("test", ("npm test",)), ops)

    def test_git_and_build(self):
        self.assertIn(("write", waste.ALL), waste.shell_ops("git checkout -- ."))
        self.assertIn(("workspace",), waste.shell_ops("npm run build && npm test"))

    def test_test_key_ignores_redirections(self):
        a = [o for o in waste.shell_ops(".venv/bin/pytest -q tests 2>&1 | tail -30")
             if o[0] == "test"]
        b = [o for o in waste.shell_ops(".venv/bin/pytest -q tests > /tmp/x") if o[0] == "test"]
        self.assertEqual(a, b)


class CellOps(unittest.TestCase):
    def reads(self, code: str) -> list:
        return [(o[1], o[2], o[3]) for o in waste.cell_ops(code) if o[0] == "read"]

    def test_read_shown(self):
        self.assertEqual(self.reads("print(read('a.py', 10, 5))"), [("a.py", (10, 14), True)])
        self.assertEqual(self.reads("t = read('a.py')\nprint(t)"),
                         [("a.py", (1, waste.WHOLE_FILE_LINES), True)])
        self.assertEqual(self.reads("read('a.py', limit=3)"), [("a.py", (1, 3), True)])

    def test_read_not_shown(self):
        self.assertEqual(self.reads("s = read('a.py')\nprint(s[:10])"),
                         [("a.py", (1, waste.WHOLE_FILE_LINES), False)])

    def test_loop_and_names(self):
        code = "p = 'a.py'\nprint(read(p, 1, 2))\nfor f in ['b.py', 'c.py']:\n    print(read(f))"
        self.assertEqual([r[0] for r in self.reads(code)], ["a.py", "b.py", "c.py"])

    def test_python_reads_and_writes(self):
        ops = waste.cell_ops("print(open('a.py').read())\nPath('b.py').write_text('x')\n"
                             "print(Path('c.py').read_text())")
        self.assertEqual([o[:2] for o in ops], [("read", "a.py"), ("write", "b.py"),
                                                 ("read", "c.py")])

    def test_edit_and_unresolved(self):
        self.assertEqual(waste.cell_ops("await edit('a.py', 'x', 'y')"), [("write", "a.py")])
        self.assertEqual(waste.cell_ops("await edit(path_of(), 'x', 'y')"),
                         [("write", waste.ALL)])

    def test_bash_output_and_order(self):
        ops = waste.cell_ops("await edit('a.py', 'x', 'y')\n"
                             "print((await bash('sed -n \"1,9p\" a.py')).output)")
        self.assertEqual([o[:2] for o in ops], [("write", "a.py"), ("read", "a.py")])

    def test_syntax_error(self):
        self.assertIsNone(waste.cell_ops("print(("))


class Detect(unittest.TestCase):
    def test_reread_and_write(self):
        s = session(cell("print(read('a.py', 1, 50))"),
                    cell("print(read('a.py', 10, 11))"),
                    cell("await edit('a.py', 'x', 'y')"),
                    cell("print(read('a.py', 10, 11))"))
        self.assertEqual(kinds(waste.detect(s)), {"reread": [1]})

    def test_partial_overlap_threshold(self):
        s = session(cell("print(read('a.py', 1, 10))"), cell("print(read('a.py', 8, 10))"))
        self.assertEqual(kinds(waste.detect(s)), {})  # 3 lignes sur 10 déjà lues

    def test_unshown_read_does_not_count(self):
        s = session(cell("s = read('a.py')\nprint(s[:10])"), cell("print(read('a.py', 1, 20))"))
        self.assertEqual(kinds(waste.detect(s)), {})

    def test_error_and_truncated_cells(self):
        s = session(cell("print(read('a.py'))", error=True),
                    cell("print(read('a.py'))", result="x[... output truncated: 9 ..."),
                    cell("print(read('a.py'))"))
        self.assertEqual(kinds(waste.detect(s)), {})

    def test_apply_patch_is_a_write(self):
        s = session(cell("print(read('a.py'))"),
                    cell("", name="apply_patch", args={"input": "*** Update File: a.py\n"}),
                    cell("print(read('a.py'))"))
        self.assertEqual(kinds(waste.detect(s)), {})

    def test_retest(self):
        s = session(cell("print(await bash('.venv/bin/pytest -q'))"),
                    cell("print(await bash('.venv/bin/pytest -q 2>&1 | tail -5'))"),
                    cell("await edit('a.py', 'x', 'y')"),
                    cell("print(await bash('.venv/bin/pytest -q'))"),
                    cell("print(await bash('npm run build'))"),
                    cell("print(await bash('.venv/bin/pytest -q'))"))
        found = waste.detect(s)
        self.assertEqual([c["cell"] for c in found["retest"]], [1])

    def test_test_log_in_tmp_is_not_a_change(self):
        s = session(cell("print(await bash('npm test > /tmp/a.log; tail /tmp/a.log'))"),
                    cell("print(await bash('npm test'))"))
        self.assertEqual([c["cell"] for c in waste.detect(s)["retest"]], [1])

    def test_similar(self):
        base = "r = await bash('grep -RIn \"alpha|beta\" lib tests | head -50'); print(r.output)"
        s = session(cell(base), cell(base.replace("50", "60")))
        self.assertEqual([c["cell"] for c in waste.detect(s)["similar"]], [1])

    def test_similar_exclusions(self):
        base = "r = await bash('grep -RIn \"alpha|beta\" lib tests | head -50'); print(r.output)"
        failed = session(cell(base, result="bash: rg: command not found"), cell(base))
        found = waste.detect(failed)
        self.assertEqual(found["similar"], [])
        self.assertEqual(found["similar_excluded"][0]["reason"], "précédente en échec")
        edited = session(cell(base), cell("await edit('a.py', 'x', 'y')"), cell(base))
        found = waste.detect(edited)
        self.assertEqual(found["similar"], [])
        self.assertEqual(found["similar_excluded"][0]["reason"], "écriture depuis la précédente")

    def test_short_cells_are_not_compared(self):
        s = session(cell("print(read('a.py', 1, 35))"), cell("print(read('b.py', 1, 35))"))
        self.assertEqual(waste.detect(s)["similar"], [])


class Cost(unittest.TestCase):
    def test_waste_cost(self):
        s = session(cell("abcd", result="ef"), cell("x"), cell("y"))

        def tokens(text: str) -> int:
            return len(text)

        w = waste.waste_cost(s, {0}, tokens, PRICES)
        # 4 appels : la cellule 0 vient de l'appel 0 et pèse sur les 3 suivants.
        self.assertEqual(w["input_tokens"], (4 + 2) * 3)
        # sortie de l'appel (7) au tarif de sortie ; puis résultat au tarif d'entrée et code en
        # cache ; puis code et résultat en cache, deux fois.
        expected = (7 * 10.0 + 2 * 1.0 + 4 * 0.1 + 2 * 6 * 0.1) / 1_000_000
        self.assertAlmostEqual(w["cost"], expected)

    def test_unknown_model(self):
        s = session(cell("abcd"), cell("x"))
        for c in s["calls"]:
            c["model"] = "inconnu"
        self.assertIsNone(waste.waste_cost(s, {0}, len, PRICES)["cost"])


class EndToEnd(unittest.TestCase):
    def test_run_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            run = Path(tmp) / "20261118T000000Z-abc1234-m"
            sessions = run / "tasks" / "t1" / "run-1" / "sessions"
            sessions.mkdir(parents=True)
            lines = [{"type": "session", "yaRequest": {"promptVersion": 2}}]
            codes = ["print(read('a.py'))", "print(read('a.py', 3, 4))"]
            for i, code in enumerate(codes):
                lines.append({"type": "message", "message": {
                    "role": "assistant", "model": "m",
                    "usage": {"input": 50, "cacheRead": 0, "cacheWrite": 0, "output": 5},
                    "content": [{"type": "toolCall", "id": f"c{i}", "name": "python_repl",
                                 "arguments": {"code": code}}]}})
                lines.append({"type": "message", "message": {
                    "role": "toolResult", "toolCallId": f"c{i}", "details": {"status": "ok"},
                    "content": [{"type": "text", "text": "line\n"}]}})
            lines.append({"type": "message", "message": {
                "role": "assistant", "model": "m", "content": [{"type": "text", "text": "fin"}],
                "usage": {"input": 10, "cacheRead": 90, "cacheWrite": 0, "output": 3}}})
            (sessions / "s.jsonl").write_text("\n".join(json.dumps(x) for x in lines))
            (sessions / "s.usage.ndjson").write_text("\n".join(json.dumps(
                {"kind": "main", "input": 50, "cacheRead": 0, "cacheWrite": 0, "output": 5})
                for _ in range(3)))
            (run / "report.json").write_text(json.dumps({"results": [
                {"task": "t1", "run": 1, "cost": {"theoretical_total": 0.001}}]}))
            row = waste.analyse_task_run(run, "t1", "run-1", 2, len, PRICES)
            self.assertEqual((row["cells"], row["reread"], row["binary"]), (2, 1, "abc1234"))
            self.assertEqual(row["prompt_tokens"], 150)
            self.assertEqual(row["cost_theoretical"], 0.001)
            self.assertEqual(row["waste"]["all"]["input_tokens"],
                             (len(codes[1]) + len("line\n")) * 1)
            self.assertIsNone(waste.analyse_task_run(run, "t1", "run-1", 1, len, PRICES))
            table = waste.render([row])
            self.assertIn("| t1 | 1 | 1 | 2 | 1 | 0 (0) | 0 | 1 |", table)


if __name__ == "__main__":
    unittest.main()
