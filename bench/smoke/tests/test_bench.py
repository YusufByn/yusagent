"""Tests sans modèle du banc (YA-14) : données synthétiques, processus et sockets locaux.

    python3 -I -m unittest discover -s bench/smoke/tests -v

Couvre le coût à cache théorique (`metrics.py`), le garde-fou d'inactivité (`run.py`,
`proxies.py`), le plan de l'A/B entrelacé (`tools/ab.py`) et l'écart sur erreur provider
d'`tools/ab_stats.py`. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import socket
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

SMOKE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SMOKE))

import metrics  # noqa: E402
import proxies  # noqa: E402


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


run = load("smoke_run", SMOKE / "run.py")
ab = load("smoke_ab", SMOKE / "tools" / "ab.py")
ab_stats = load("smoke_ab_stats", SMOKE / "tools" / "ab_stats.py")

PRICES = {"long_context_threshold_input_tokens": 272_000,
          "models": {"m": {"short": {"input": 1.0, "cached_input": 0.1, "output": 10.0,
                                     "cache_write_30m": 1.25, "cache_write_12h": 2.0}}}}


def rec(seq: int, inp: int, read: int, out: int, kind: str = "main", session: str = "s",
        **extra) -> dict:
    return {"sessionId": session, "seq": seq, "kind": kind, "input": inp, "cacheRead": read,
            "cacheWrite": 0, "output": out, "startMs": 1000 * seq, "durationMs": 500, **extra}


class TheoreticalCache(unittest.TestCase):
    def theo(self, records):
        return {(c["session"], c["seq"]): (c["basis"], c["theoretical"])
                for c in metrics.theoretical_cache(records)}

    def test_bases(self):
        records = [
            rec(1, 1000, 7000, 50),            # premier : rapporté
            rec(2, 1500, 7000, 40),            # continuation : min(8500, 8000 + 50)
            rec(3, 8740, 0, 30),               # miss serveur : min(8740, 8500 + 40)
            rec(4, 6000, 2000, 20, invalidation={"cause": "masking"}),  # rapporté
            rec(5, 100, 1000, 10),             # prompt 1100 < 8000 : rupture, rapporté
            rec(6, 0, 0, 0),                   # prompt vide : rapporté (0), ignoré ensuite
            rec(7, 100, 1100, 5),              # continuation sur l'appel 5 : min(1200, 1110)
            rec(8, 1500, 0, 5, kind="compaction"),  # annexe : rapporté
            rec(9, 100, 1100, 5),              # après une annexe : rupture, rapporté
            rec(1, 900, 0, 7, kind="subagent", session="sub"),  # autre session : premier, rapporté
            rec(2, 950, 0, 7, kind="subagent", session="sub"),  # min(950, 907)
        ]
        got = self.theo(records)
        self.assertEqual(got["s", 1], ("premier", 7000))
        self.assertEqual(got["s", 2], ("continuation", 8050))
        self.assertEqual(got["s", 3], ("continuation", 8540))
        self.assertEqual(got["s", 4], ("invalidation", 2000))
        self.assertEqual(got["s", 5], ("rupture", 1000))
        self.assertEqual(got["s", 6], ("rupture", 0))
        self.assertEqual(got["s", 7], ("continuation", 1110))
        self.assertEqual(got["s", 8], ("annexe", 0))
        self.assertEqual(got["s", 9], ("rupture", 1100))
        self.assertEqual(got["sub", 1], ("premier", 0))
        self.assertEqual(got["sub", 2], ("continuation", 907))

    def test_cost_and_gaps(self):
        records = [rec(1, 600, 400, 100), rec(2, 200, 900, 10)]
        summary = metrics.theory_summary(PRICES, "m", records)
        # Appel 1 (premier) : 400 lus (rapporté), 600 × 1 + 100 × 10 ; appel 2 : théorique
        # min(1100, 1000 + 100) = 1100 lu.
        expected = (400 * 0.1 + 600 * 1.0 + 100 * 10.0 + 1100 * 0.1 + 0 * 1.0 + 10 * 10.0) / 1e6
        self.assertAlmostEqual(summary["total"], expected)
        self.assertEqual(summary["cacheRead"], 400 + 1100)
        self.assertEqual(summary["input"], 2100 - 1500)
        self.assertEqual(summary["check_gaps"], [round(200 / 1100, 6)])
        costs = metrics.costs(PRICES, "m", records, "cache_write_30m")
        self.assertAlmostEqual(costs["theoretical_total"], expected)
        self.assertIsNone(metrics.theory_summary({"models": {}}, "m", records)["total"])

    def test_check_verdict(self):
        few = metrics.theory_check([0.01] * 5)
        self.assertIsNone(few["valid"])
        ok = metrics.theory_check([0.01] * 40)
        self.assertTrue(ok["valid"])
        above = metrics.theory_check([0.01] * 39 + [-0.1])  # 2,5 % au-dessus : borne fausse
        self.assertFalse(above["valid"])
        wide = metrics.theory_check([0.03] * 40)
        self.assertFalse(wide["valid"])
        self.assertEqual(metrics.theory_check([])["n"], 0)


class IdleWatchdog(unittest.TestCase):
    def test_run_process_stalls(self):
        with tempfile.TemporaryDirectory() as tmp:
            out, err = Path(tmp) / "o", Path(tmp) / "e"
            started = time.monotonic()
            result = run.run_process([sys.executable, "-c", "import time; time.sleep(30)"], {},
                                     Path(tmp), out, err, 20, sandboxed=False,
                                     idle=(lambda: time.monotonic() - started, 1))
            self.assertTrue(result["stalled"])
            self.assertFalse(result["timeout"])
            self.assertLess(result["seconds"], 10)
            normal = run.run_process([sys.executable, "-c", "pass"], {}, Path(tmp), out, err, 20,
                                     sandboxed=False, idle=(lambda: 0.0, 1))
            self.assertEqual((normal["exit"], normal["stalled"]), (0, False))
            timeout = run.run_process([sys.executable, "-c", "import time; time.sleep(30)"], {},
                                      Path(tmp), out, err, 1, sandboxed=False)
            self.assertTrue(timeout["timeout"])
            self.assertNotIn("stalled", timeout)

    def test_reverse_proxy_idle(self):
        """Amont qui accepte la connexion et ne répond jamais : le proxy n'a plus d'octet à
        relayer, `idle_seconds` monte, `run_process` arrête le client."""
        upstream = socket.socket()
        upstream.bind(("127.0.0.1", 0))
        upstream.listen()
        held = []
        threading.Thread(target=lambda: held.append(upstream.accept()), daemon=True).start()
        with tempfile.TemporaryDirectory() as tmp:
            proxy = proxies.ReverseProxy(Path(tmp) / "proxy.ndjson",
                                         ("127.0.0.1", upstream.getsockname()[1], False))
            proxies.serve(proxy)
            client = ("import socket, time; s = socket.create_connection(('127.0.0.1', %d)); "
                      "s.sendall(b'POST /backend-api/codex/responses HTTP/1.1\\r\\n"
                      "Content-Length: 2\\r\\n\\r\\n{}'); time.sleep(30)" % proxy.port)
            try:
                result = run.run_process([sys.executable, "-c", client], {}, Path(tmp),
                                         Path(tmp) / "o", Path(tmp) / "e", 20, sandboxed=False,
                                         idle=(proxy.idle_seconds, 2))
            finally:
                proxy.shutdown()
                proxy.server_close()
                upstream.close()
                for conn, _ in held:
                    conn.close()
            self.assertTrue(result["stalled"])
            self.assertGreaterEqual(result["seconds"], 2)
            self.assertLess(result["seconds"], 10)

    def test_task_status_stalled(self):
        task = {"id": "t", "tags": []}
        process = {"exit": None, "timeout": False, "stalled": True, "idle_timeout_s": 300,
                   "seconds": 400.0}
        session = {"entries": [{"message": {"role": "assistant", "stopReason": "toolUse"}}],
                   "records": [rec(1, 100, 0, 5)], "sub_records": [], "sub_entries": [],
                   "proxy_log": [], "connect": []}
        graded = {"junit": True, "tests": {"a": "passed"}}
        result = run.task_result(task, 1, process, session, graded, "m", "high", PRICES,
                                 {"tree": None, "source": "test"})
        self.assertEqual(result["status"], "stalled")
        self.assertTrue(any("flux bloqué" in a for a in result["anomalies"]))


class AbPlan(unittest.TestCase):
    def test_plan_seeded(self):
        a = ab.plan(["t1", "t2", "t3"], 4, 123)
        self.assertEqual(a, ab.plan(["t1", "t2", "t3"], 4, 123))
        self.assertEqual([(u["pass"], u["task"]) for u in a[:3]], [(1, "t1"), (1, "t2"), (1, "t3")])
        self.assertTrue({u["order"] for u in a} <= {"AB", "BA"})
        orders = [u["order"] for u in ab.plan([f"t{i}" for i in range(200)], 1, 5)]
        self.assertGreater(orders.count("AB"), 60)
        self.assertGreater(orders.count("BA"), 60)

    def test_settings_diff(self):
        self.assertEqual(ab.settings_diff({}, {}), {})
        self.assertEqual(ab.settings_diff({"rlm": {"spawn": True}, "x": 1},
                                          {"rlm": {"spawn": False}, "x": 1}),
                         {"rlm.spawn": [True, False]})
        self.assertEqual(ab.settings_diff({}, {"masking": {"enabled": True}}),
                         {"masking.enabled": [None, True]})


def write_arm(root: Path, name: str, task: str, theo: float, status: str = "resolved",
              session_error: str | None = None, proxy_status: str = "101") -> Path:
    """Run synthétique d'une tâche : report.json, journaux de proxy et de session."""
    run_dir = root / name
    out = run_dir / "tasks" / task / "run-1"
    (out / "sessions").mkdir(parents=True)
    (out / "proxy.ndjson").write_text(json.dumps({"ts": 1000, "event": "request",
                                                  "status": proxy_status}) + "\n")
    entries = [{"message": {"role": "assistant", "stopReason": "toolUse"}}]
    if session_error:
        entries.append({"message": {"role": "assistant", "stopReason": "error",
                                    "errorMessage": session_error}})
    (out / "sessions" / "s.jsonl").write_text("".join(json.dumps(e) + "\n" for e in entries))
    (out / "sessions" / "s.usage.ndjson").write_text(json.dumps(rec(1, 10, 0, 1)) + "\n")
    result = {"task": task, "run": 1, "status": status, "resolved": status == "resolved",
              "calls": {"turns": 3, "total": 3},
              "tokens": {"input": 10, "cacheRead": 0, "cacheWrite": 0},
              "cost": {"total": theo * 1.2, "pessimistic_total": theo * 1.3,
                       "theoretical_total": theo, "theoretical": {"cacheRead": 0}},
              "server_misses": [], "subagents": {"cost": 0}}
    (run_dir / "report.json").write_text(json.dumps({"header": {}, "results": [result]}))
    return run_dir


class AbStats(unittest.TestCase):
    def run_stats(self, argv: list[str]) -> str:
        buffer = io.StringIO()
        old = sys.argv
        sys.argv = ["ab_stats.py", *argv]
        try:
            with contextlib.redirect_stdout(buffer):
                ab_stats.main()
        finally:
            sys.argv = old
        return buffer.getvalue()

    def test_plan_excludes_unit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            arms = []
            costs = {("t1", "A"): 1.0, ("t1", "B"): 0.5, ("t2", "A"): 2.0, ("t2", "B"): 1.0}
            for p in (1, 2):
                for task in ("t1", "t2"):
                    for side in "AB":
                        error = ("Our servers are currently overloaded"
                                 if (p, task, side) == (2, "t2", "B") else None)
                        run_dir = write_arm(root, f"p{p}-{task}-{side}", task,
                                            costs[task, side], session_error=error)
                        arms.append({"pass": p, "task": task, "order": "AB", "arm": side,
                                     "exit": 0, "run_dir": str(run_dir)})
            plan = root / "ab.json"
            plan.write_text(json.dumps({"arms": arms, "seed": 1, "seed_source": "test",
                                        "settings_diff": {}, "idle_timeout_s": 300}))
            text = self.run_stats(["--plan", str(plan), "--draws", "200"])
            self.assertIn("retenues 3, écartées 1", text)
            self.assertIn("passe 2 (t2) : bras B : session principale", text)
            self.assertIn("**toutes, théorique (décision) : -50.0 %", text)
            self.assertEqual(text, self.run_stats(["--plan", str(plan), "--draws", "200"]))

    def test_pairs_exclude_whole_pair(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runs = []
            for p in (1, 2):
                for side, value in (("A", 1.0), ("B", 0.8)):
                    run_dir = root / f"{p}{side}"
                    for task in ("t1", "t2"):
                        status = "stalled" if (p, side, task) == (1, "B", "t1") else "resolved"
                        write_arm(root / "tmp", f"{p}{side}{task}", task, value, status=status)
                    # Un run par passe complète : les deux tâches dans le même rapport.
                    results = []
                    for task in ("t1", "t2"):
                        src = root / "tmp" / f"{p}{side}{task}"
                        (run_dir / "tasks").mkdir(parents=True, exist_ok=True)
                        (src / "tasks" / task).rename(run_dir / "tasks" / task)
                        results += json.loads((src / "report.json").read_text())["results"]
                    (run_dir / "report.json").write_text(json.dumps({"header": {},
                                                                     "results": results}))
                    runs.append(str(run_dir))
            text = self.run_stats([*runs, "--draws", "200"])
            self.assertIn("retenues 2, écartées 2", text)
            self.assertIn("passe 1 (toutes ses tâches) : paire 1 écartée entière", text)
            self.assertIn("flux bloqué (garde-fou d'inactivité", text)
            self.assertIn("**toutes, théorique (décision) : -20.0 %", text)

    def test_timeout_classification(self):
        with tempfile.TemporaryDirectory() as tmp:
            run_dir = write_arm(Path(tmp), "r", "t", 1.0, status="timeout")
            out = run_dir / "tasks" / "t" / "run-1"
            with (out / "proxy.ndjson").open("a") as handle:
                handle.write(json.dumps({"ts": 1000 + 400_000, "event": "ws_close"}) + "\n")
            result = json.loads((run_dir / "report.json").read_text())["results"][0]
            found = ab_stats.incidents(run_dir, result, 300)
            self.assertTrue(found[0][1] and "flux bloqué" in found[0][0])
            self.assertFalse(ab_stats.incidents(run_dir, result, 600)[0][1])


if __name__ == "__main__":
    unittest.main()
