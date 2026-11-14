"""Tests sans modèle du banc (YA-14) : données synthétiques, processus et sockets locaux.

    python3 -I -m unittest discover -s bench/smoke/tests -v

Couvre le coût à cache théorique (`metrics.py`), le garde-fou d'inactivité (`run.py`,
`proxies.py`), le plan de l'A/B entrelacé (`tools/ab.py`), l'écart sur erreur provider
d'`tools/ab_stats.py` et, pour la comparaison contre Codex CLI (YA-14) : la fin de réponse lue
au proxy, la rupture sur corps complet, l'appariement par ordre, le bras Codex (`codex_arm.py`),
les scénarios Codex du serveur de capture et la règle de verdict. Bibliothèque standard
uniquement.
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

import codex_arm  # noqa: E402
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



SKILL = "/x/skills/edit/src/edit/__init__.py"


class EditAttempts(unittest.TestCase):
    """YA-03 : edits appliqués au premier essai et patchs refusés hors workspace."""

    @staticmethod
    def call(call_id: str, code: str) -> dict:
        return {"message": {"role": "assistant", "content": [
            {"type": "toolCall", "id": call_id, "name": "python_repl",
             "arguments": {"code": code}}]}}

    @staticmethod
    def result(call_id: str, tool: str, text: str, error: bool, diffs: int = 0) -> dict:
        details = {"diffs": [{"path": "f"}] * diffs} if diffs else {}
        return {"message": {"role": "toolResult", "toolCallId": call_id, "toolName": tool,
                            "isError": error, "details": details,
                            "content": [{"type": "text", "text": text}]}}

    def test_counts(self):
        entries = [
            self.result("p1", "apply_patch", "Exit code: 0\nOutput:\nSuccess.", False),
            self.result("p2", "apply_patch", "Exit code: 1\nOutput:\n"
                        "[outside-workspace] /x: resolves to /x, outside the workspace (/w)", True),
            self.result("p3", "apply_patch", "Exit code: 1\nOutput:\nFailed to find", True),
            self.call("c1", "await edit(path='a', old_str='x', new_str='y')\n"
                            "await edit.run('b', 'x', 'y')"),
            self.result("c1", "python_repl", "", False, diffs=2),
            self.call("c2", "await edit(path='a', old_str='zz', new_str='y')"),
            self.result("c2", "python_repl", f"Traceback\n  File \"{SKILL}\", line 34\n"
                        "ValueError: string not found in a", True),
            self.call("c3", "await edit(path='q', old_str='x', new_str='y')"),
            self.result("c3", "python_repl", f"  File \"{SKILL}\", line 30\n"
                        "FileNotFoundError: q not found", True),
            # Message du skill levé hors du skill (pas de frame du skill) : pas un échec d'edit().
            self.call("c6", "await edit(path='a', old_str='x', new_str='y')\n"
                            "raise ValueError('string not found in a')"),
            self.result("c6", "python_repl", "ValueError: string not found in a", True, diffs=1),
            # Une erreur sans rapport avec edit() ne compte pas.
            self.call("c4", "await edit(path='a', old_str='y', new_str='z')\n1/0"),
            self.result("c4", "python_repl", "ZeroDivisionError: division by zero", True, diffs=1),
            # Pas d'appel d'edit() : pas d'essai, même avec un message semblable.
            self.call("c5", "raise ValueError('string not found in a')"),
            self.result("c5", "python_repl", "ValueError: string not found in a", True),
        ]
        got = metrics.edit_attempts(entries)
        self.assertEqual(got["apply_patch"],
                         {"calls": 3, "applied": 1, "refused_outside_workspace": 1})
        self.assertEqual(got["edit"], {"applied": 4, "failed": 2})
        self.assertEqual(got["attempts"], 9)
        self.assertEqual(got["applied"], 5)
        self.assertAlmostEqual(got["first_try_rate"], 5 / 9)

    def test_no_edit(self):
        self.assertIsNone(metrics.edit_attempts([])["first_try_rate"])
        self.assertEqual(ab_stats.first_try({"attempts": 0, "applied": 0}), "aucun edit")


class RealFailures(unittest.TestCase):
    """Échecs réels de l'A/B 1 de YA-03 (ab-20261114T120431Z), `isError` à `false` : cellule
    `python_repl` en erreur (`details.status`), `edit()` arrêté par le skill, `apply_patch`
    refusé par la vérification de Codex, `apply_patch` appliqué. Fixture tirée des sessions
    (champ `source`), chemin du dossier personnel remplacé par `/Users/u`."""

    FIXTURE = Path(__file__).resolve().parent / "fixtures" / "ya03-ab1-echecs.jsonl"

    def entries(self, case: str | None = None) -> list[dict]:
        rows = [json.loads(line) for line in self.FIXTURE.read_text().splitlines() if line]
        return [r for r in rows if case is None or r["case"] == case]

    def results(self, case: str) -> list[dict]:
        return [r["message"] for r in self.entries(case) if r["message"]["role"] == "toolResult"]

    def test_is_error_is_false_on_every_failure(self):
        self.assertEqual({r["message"]["isError"] for r in self.entries()
                          if r["message"]["role"] == "toolResult"}, {False})

    def test_cell_failures(self):
        for case in ("edit_failure", "cell_failure"):
            self.assertTrue(metrics.cell_failed(self.results(case)[0]), case)
        got = metrics.cell_errors(self.entries())
        self.assertEqual(got, {"cells": 2, "errors": 2, "syntax_errors": 0,
                               "by_type": {"ValueError": 1, "IndexError": 1}})

    def test_patch_outcomes(self):
        self.assertFalse(metrics.patch_applied(self.results("patch_failure")[0]))
        self.assertTrue(metrics.patch_applied(self.results("patch_applied")[0]))

    def test_edit_failure_from_the_skill_only(self):
        got = metrics.edit_attempts(self.entries())
        self.assertEqual(got["apply_patch"],
                         {"calls": 2, "applied": 1, "refused_outside_workspace": 0})
        # Six edits appliqués dans la cellule avant celui qui échoue (2 occurrences) ; la
        # cellule IndexError n'appelle pas edit().
        self.assertEqual(got["edit"], {"applied": 6, "failed": 1})
        self.assertEqual((got["attempts"], got["applied"]), (9, 7))


class BodyLog(unittest.TestCase):
    """`--log-bodies` : corps JSON des requêtes, jamais les en-têtes ; rien sans l'option."""

    def post_through_proxy(self, tmp: Path, bodies: Path | None) -> None:
        upstream = socket.socket()
        upstream.bind(("127.0.0.1", 0))
        upstream.listen()

        def answer():
            conn, _ = upstream.accept()
            with conn:
                conn.recv(65536)
                conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")

        threading.Thread(target=answer, daemon=True).start()
        proxy = proxies.ReverseProxy(tmp / "proxy.ndjson",
                                     ("127.0.0.1", upstream.getsockname()[1], False), bodies)
        proxies.serve(proxy)
        body = json.dumps({"prompt_cache_key": "s1", "input": [{"role": "user"}]}).encode()
        try:
            with socket.create_connection(("127.0.0.1", proxy.port)) as client:
                client.sendall(b"POST /backend-api/codex/responses HTTP/1.1\r\n"
                               b"Authorization: Bearer SECRET-TOKEN\r\n"
                               b"chatgpt-account-id: SECRET-ACCOUNT\r\n"
                               b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n"
                               + body)
                client.recv(65536)
            time.sleep(0.2)
        finally:
            proxy.shutdown()
            proxy.server_close()
            upstream.close()

    def test_bodies_written_without_headers(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self.post_through_proxy(tmp, tmp / "proxy-bodies.ndjson")
            text = (tmp / "proxy-bodies.ndjson").read_text()
            entries = [json.loads(line) for line in text.splitlines()]
            self.assertEqual(len(entries), 1)
            self.assertEqual(entries[0]["session"], "s1")
            self.assertEqual(entries[0]["body"]["input"], [{"role": "user"}])
            for secret in ("SECRET-TOKEN", "SECRET-ACCOUNT", "Authorization", "Bearer"):
                self.assertNotIn(secret, text)
                self.assertNotIn(secret, (tmp / "proxy.ndjson").read_text())

    def test_no_bodies_by_default(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            self.post_through_proxy(tmp, None)
            self.assertEqual(sorted(p.name for p in tmp.iterdir()), ["proxy.ndjson"])

    def test_websocket_message_reaches_the_body_callback(self):
        seen = []
        counter = proxies.FrameCounter(lambda *args: seen.append(args))
        message = json.dumps({"type": "response.create", "prompt_cache_key": "s2",
                              "previous_response_id": "resp_1", "input": []}).encode()
        mask = b"\x01\x02\x03\x04"
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(message))
        counter.feed(bytes([0x81, 0x80 | len(message)]) + mask + masked)
        self.assertEqual(seen, [("s2", True, len(message), message, None)])

    def test_refused_during_ab(self):
        argv = sys.argv
        sys.argv = ["run.py", "--binary", sys.executable, "--ab", "{}", "--log-bodies"]
        try:
            with self.assertRaises(SystemExit) as stop:
                run.main()
        finally:
            sys.argv = argv
        self.assertEqual(stop.exception.code,
                         "smoke : --log-bodies est refusé pendant un A/B (--ab)")


capture_server = load("capture_server", SMOKE.parents[1] / "bench" / "tools" / "capture" / "server.py")


def ws_frame(message: bytes, masked: bool) -> bytes:
    """Trame texte WebSocket (longueur < 126), masquée côté client."""
    if not masked:
        return bytes([0x81, len(message)]) + message
    mask = b"\x01\x02\x03\x04"
    return (bytes([0x81, 0x80 | len(message)]) + mask
            + bytes(b ^ mask[i % 4] for i, b in enumerate(message)))


class ResponseOutcome(unittest.TestCase):
    """YA-14 : `generate` et fin de chaque réponse au reverse proxy, des nombres seulement."""

    def test_generate_is_noted(self):
        seen = []
        counter = proxies.FrameCounter(lambda *args: seen.append(args))
        message = json.dumps({"type": "response.create", "prompt_cache_key": "s",
                              "generate": False, "input": []}).encode()
        counter.feed(ws_frame(message, masked=True))
        self.assertEqual(seen[0][4], False)

    def test_websocket_completed_usage_without_text(self):
        seen = []
        watcher = proxies.ResponseWatcher(seen.append, websocket=True)
        event = {"type": "response.completed", "response": {
            "id": "resp_1", "status": "completed", "output": [{"type": "message",
                                                               "content": "SECRET-TEXT"}],
            "usage": {"input_tokens": 100, "output_tokens": 7,
                      "input_tokens_details": {"cached_tokens": 60, "cache_write_tokens": 30},
                      "output_tokens_details": {"reasoning_tokens": 2}}}}
        data = ws_frame(json.dumps({"type": "response.output_text.delta"}).encode(), False)
        raw = json.dumps(event).encode()
        data += bytes([0x81, 126]) + len(raw).to_bytes(2, "big") + raw
        watcher.feed(data[:7])
        watcher.feed(data[7:])
        self.assertEqual(len(seen), 1)
        self.assertEqual(seen[0]["usage"], {"input_tokens": 100, "cached_tokens": 60,
                                            "cache_write_tokens": 30, "output_tokens": 7,
                                            "reasoning_tokens": 2})
        self.assertNotIn("SECRET-TEXT", json.dumps(seen))

    def test_sse_error_and_completed(self):
        seen = []
        watcher = proxies.ResponseWatcher(seen.append, websocket=False)
        stream = (b"event: error\ndata: " + json.dumps({"type": "error", "status": 429,
                  "error": {"type": "rate_limit", "code": "usage_limit_reached",
                            "message": "SECRET-TEXT"}}).encode() + b"\n\n"
                  + b"event: response.completed\ndata: " + json.dumps(
                      {"type": "response.completed", "response": {"id": "r", "usage": {}}}).encode()
                  + b"\n\n")
        for i in range(0, len(stream), 9):
            watcher.feed(stream[i:i + 9])
        self.assertEqual([o["event"] for o in seen], ["response_error", "response"])
        self.assertEqual((seen[0]["status"], seen[0]["error_code"]), (429, "usage_limit_reached"))
        self.assertNotIn("SECRET-TEXT", json.dumps(seen))


class FullBodyRupture(unittest.TestCase):
    """YA-14 : un appel de boucle en corps complet sur WebSocket, hors premier, est une rupture."""

    def test_rupture_only_with_websocket_full_body(self):
        records = [rec(1, 1000, 0, 50), rec(2, 100, 900, 50), rec(3, 100, 1000, 50)]
        per_request = [{"session": "s", "seq": 1, "transport": "websocket", "continuation": False},
                       {"session": "s", "seq": 2, "transport": "websocket", "continuation": False},
                       {"session": "s", "seq": 3, "transport": "sse", "continuation": None}]
        bases = [c["basis"] for c in metrics.theoretical_cache(records, per_request)]
        self.assertEqual(bases, ["premier", "rupture", "continuation"])
        self.assertEqual([c["basis"] for c in metrics.theoretical_cache(records)],
                         ["premier", "continuation", "continuation"])

    def test_in_order_matching(self):
        records = [rec(1, 0, 0, 0, kind="prewarm", session="t"), rec(2, 10, 0, 1, session="t"),
                   rec(3, 10, 5, 1, session="t")]
        log = [{"event": "request", "ts": 1, "transport": "websocket", "session": "t",
                "generate": False, "continuation": False},
               {"event": "request", "ts": 2, "transport": "websocket", "session": "t",
                "continuation": True},
               {"event": "request", "ts": 3, "transport": "websocket", "session": "t",
                "continuation": True}]
        m = metrics.match_transports_in_order(records, log)
        self.assertEqual([(p["kind"], p["continuation"]) for p in m["per_request"]],
                         [("prewarm", False), ("main", True), ("main", True)])
        self.assertEqual((m["unmatched_usage"], m["unmatched_proxy"], m["loop_on_sse"]), (0, 0, 0))


class CodexArm(unittest.TestCase):
    """YA-14 : réglage, config, conversion de l'usage et sortie `--json` du bras Codex."""

    def test_settings(self):
        self.assertIsNone(codex_arm.settings_of({}))
        self.assertEqual(codex_arm.settings_of({"harness": "codex", "codex": {"version": "1"}}),
                         {"version": "1"})
        with self.assertRaises(ValueError):
            codex_arm.settings_of({"harness": "codex", "codex": {"version": "1",
                                                                  "featureCuts": ["memories"]}})

    def test_config_is_valid_toml(self):
        import tomllib
        text = codex_arm.config_text(4321, Path("/x/codex"), {"version": "1",
                                                              "featureCuts": ["apps", "plugins"]})
        self.assertNotIn("{{", text)
        conf = tomllib.loads(text)
        self.assertEqual(conf["model_providers"]["ya_bench"]["base_url"],
                         "http://127.0.0.1:4321/backend-api/codex")
        self.assertNotEqual(conf["model_providers"]["ya_bench"]["name"], "OpenAI")
        self.assertEqual(conf["web_search"], "disabled")
        self.assertEqual(conf["features"], {"unbounded_connection_retries": False,
                                            "apps": False, "plugins": False})
        self.assertEqual(conf["model_catalog_json"], "/x/models.json")

    def test_records(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rollout-x.jsonl"
            rows = [{"timestamp": "2026-11-14T10:00:00.000Z", "type": "session_meta",
                     "payload": {"id": "th"}},
                    {"timestamp": "2026-11-14T10:00:00.500Z", "type": "turn_context",
                     "payload": {"model": "gpt-6-luna", "effort": "high"}}]
            for i, (inp, read, write) in enumerate(((1000, 0, 900), (1100, 1000, 50)), 1):
                rows.append({"timestamp": f"2026-11-14T10:00:0{i}.000Z",
                             "type": "token_usage_record", "payload": {
                                 "response_id": f"r{i}", "usage": {
                                     "input_tokens": inp, "cached_input_tokens": read,
                                     "cache_write_input_tokens": write, "output_tokens": 20,
                                     "reasoning_output_tokens": 5}}})
            path.write_text("".join(json.dumps(r) + "\n" for r in rows))
            prewarm = {"event": "response", "generate": False, "session": "th", "ts": 1794650400100,
                       "response_id": "r0", "usage": {"input_tokens": 3, "cached_tokens": 0,
                                                      "cache_write_tokens": 0, "output_tokens": 0,
                                                      "reasoning_tokens": 0}}
            out = codex_arm.records([path], [prewarm], "th")
        self.assertEqual([(r["seq"], r["kind"]) for r in out],
                         [(1, "prewarm"), (2, "main"), (3, "main")])
        self.assertEqual((out[1]["input"], out[1]["cacheRead"], out[1]["cacheWrite"]), (100, 0, 900))
        self.assertEqual((out[2]["input"], out[2]["cacheRead"], out[2]["output"]), (50, 1000, 20))
        self.assertEqual((out[0]["model"], out[0]["effort"]), ("gpt-6-luna", "high"))

    def test_events(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "out.jsonl"
            rows = [{"type": "thread.started", "thread_id": "th"},
                    {"type": "error", "message": "Reconnecting... 1/5"},
                    {"type": "item.completed", "item": {"type": "file_change", "status": "completed"}},
                    {"type": "item.completed", "item": {"type": "file_change", "status": "failed"}},
                    {"type": "turn.completed", "usage": {}}]
            path.write_text("".join(json.dumps(r) + "\n" for r in rows))
            ev = codex_arm.events(path)
        self.assertEqual((ev["thread_id"], ev["status"], ev["errors"]),
                         ("th", "completed", ["Reconnecting... 1/5"]))
        self.assertEqual(codex_arm.edits(ev)["first_try_rate"], 0.5)
        self.assertEqual(codex_arm.final(ev)["stop_reason"], "stop")


class CodexLaunchGuard(unittest.TestCase):
    """YA-14 : Codex ne se lance que sous sandbox-exec, avec un CODEX_HOME du banc (jamais
    `~/.codex`). Un faux `codex` (script) note son lancement : il ne doit jamais tourner."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name).resolve()
        self.cache = run.Cache(self.root / "cache", None, True)
        self.marker = self.root / "launched"
        self.binary = self.root / "bin" / "codex"
        self.binary.parent.mkdir()
        self.binary.write_text(f"#!/bin/sh\ntouch {self.marker}\n")
        self.binary.chmod(0o755)

    def tearDown(self):
        self.tmp.cleanup()

    def launch(self, codex_home, sandboxed=True, check=True, agent_dir=None, home=None):
        env = codex_arm.env("/usr/bin:/bin", home or self.cache.home, self.cache.tmp,
                            codex_home, None)
        if codex_home is None:
            del env["CODEX_HOME"]
        work = self.root / "work"
        work.mkdir(exist_ok=True)
        profile = run.agent_profile(self.cache, work, self.binary,
                                    agent_dir or codex_home or self.cache.codex_home)
        return run.run_process([str(self.binary), "--version"], env, work, self.root / "out",
                               self.root / "err", 10, sandboxed=sandboxed, profile=profile,
                               check=run.codex_guard(self.cache) if check else None)

    def assertRefused(self, pattern, **kwargs):
        with self.assertRaisesRegex(codex_arm.LaunchRefused, pattern):
            self.launch(**kwargs)
        self.assertFalse(self.marker.exists())

    def test_refusals(self):
        bench = self.cache.codex_home
        self.assertRefused("sans garde-fou", codex_home=bench, check=False)
        self.assertRefused("hors /usr/bin/sandbox-exec", codex_home=bench, sandboxed=False)
        self.assertRefused("CODEX_HOME absent", codex_home=None)
        self.assertRefused(r"CODEX_HOME dans ~/\.codex", codex_home=Path.home() / ".codex")
        self.assertRefused("hors du banc", codex_home=self.root / "elsewhere")
        self.assertRefused("AGENT_DIR du profil", codex_home=bench,
                           agent_dir=self.cache.probe_codex_home)
        self.assertRefused("HOME absent ou réel", codex_home=bench, home=Path.home())

    def test_accepted_launch_runs(self):
        for home in (self.cache.codex_home, self.cache.probe_codex_home):
            home.mkdir(parents=True, exist_ok=True)
            # Lever l'exception est le seul effet du garde-fou : il ne lance rien lui-même.
            codex_arm.check_launch(
                ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX_AGENT), "-D",
                 f"AGENT_DIR={home.resolve()}", str(self.binary)],
                {"HOME": str(self.cache.home), "CODEX_HOME": str(home)},
                run.SANDBOX_AGENT, self.cache.root)

    def test_other_programs_untouched(self):
        self.assertFalse(codex_arm.is_codex(["/usr/bin/sandbox-exec", "-f", "p", "-D", "A=codex",
                                             "/x/prime-agent", "codex"]))
        self.assertTrue(codex_arm.is_codex(["/usr/bin/sandbox-exec", "-f", "p", "-D", "A=b",
                                            "/x/codex", "exec"]))


class CodexConfig(unittest.TestCase):
    """YA-14 : la config du banc remplace celle du login et garde `auth.json` (pas de trousseau)."""

    def test_rewrite_keeps_file_store(self):
        import tomllib
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            # Config laissée par le login du plan, puis par `--trust` pendant une session.
            (home / "config.toml").write_text(
                'cli_auth_credentials_store = "file"\n\n[projects."/w"]\ntrust_level = "trusted"\n')
            text = codex_arm.write_config(home, 4321, Path("/x/codex"), {"version": "1"})
            written = (home / "config.toml").read_text()
        self.assertEqual(written, text)
        conf = tomllib.loads(written)
        self.assertEqual(conf["cli_auth_credentials_store"], "file")
        self.assertNotIn("projects", conf)
        self.assertEqual(written.count("cli_auth_credentials_store"), 1)

    def test_refuses_template_without_file_store(self):
        with tempfile.TemporaryDirectory() as tmp:
            template = Path(tmp) / "config.toml.in"
            template.write_text(codex_arm.TEMPLATE.read_text().replace(
                'cli_auth_credentials_store = "file"', 'cli_auth_credentials_store = "auto"'))
            saved, codex_arm.TEMPLATE = codex_arm.TEMPLATE, template
            try:
                with self.assertRaisesRegex(ValueError, "cli_auth_credentials_store"):
                    codex_arm.write_config(Path(tmp) / "home", 1, Path("/x/codex"),
                                           {"version": "1"})
            finally:
                codex_arm.TEMPLATE = saved
            self.assertFalse((Path(tmp) / "home" / "config.toml").exists())


class CaptureCodex(unittest.TestCase):
    """YA-14 : le serveur de capture reconnaît Codex (`additional_tools`, tool libre `exec`)."""

    def test_codex_plan(self):
        body = {"input": [{"type": "additional_tools", "tools": [
            {"type": "namespace", "name": "functions",
             "tools": [{"type": "custom", "name": "exec"}, {"type": "function", "name": "wait"}]}]},
            {"type": "message", "role": "user", "content": "x"}]}
        tools = capture_server.codex_tools(body)
        self.assertTrue(capture_server.is_codex(tools))
        state = capture_server.CaptureState(Path(tempfile.gettempdir()), "smoke-py-example-001",
                                            None, 0, connect_proxy_port=7, read_probe_paths=("/p",))
        plan = state.plan(tools, 2, body=body)
        self.assertEqual((plan.kind, plan.tool_name, plan.freeform), ("tool", "exec", True))
        self.assertIn("tools.apply_patch", plan.code)
        self.assertEqual(state.plan(tools, 4, body=body).kind, "final")
        item = capture_server.responses_item(plan, 9, done=True)
        self.assertEqual((item["type"], item["name"], item["input"]),
                         ("custom_tool_call", "exec", plan.code))
        body["input"].append({"type": "custom_tool_call_output", "call_id": "c", "output": "x"})
        self.assertEqual(capture_server.count_tool_results_responses(body), 1)


class Ya14Verdict(unittest.TestCase):
    """Règle fixée avant le run (plan YA-14) : résolution d'abord, puis coût."""

    def verdict(self, failing_b: set[str], tasks: int = 6, cost_b: float = 0.5) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            arms = []
            for p in (1, 2, 3):
                for t in range(tasks):
                    task = f"t{t}"
                    for side in "AB":
                        status = "failed" if side == "B" and task in failing_b else "resolved"
                        run_dir = write_arm(root, f"p{p}-{task}-{side}", task,
                                            1.0 if side == "A" else cost_b, status=status)
                        arms.append({"pass": p, "task": task, "order": "AB", "arm": side,
                                     "exit": 0, "run_dir": str(run_dir)})
            plan = root / "ab.json"
            plan.write_text(json.dumps({"arms": arms, "seed": 1, "seed_source": "test",
                                        "settings_diff": {}, "idle_timeout_s": 300,
                                        "model": "gpt-6-luna"}))
            return AbStats.run_stats(self, ["--plan", str(plan), "--draws", "200",
                                            "--verdict", "ya14-codex"])

    def test_cost_decides_without_resolution_gap(self):
        text = self.verdict(set())
        self.assertIn("**Verdict : pas de verdict de résolution ; coût : B (Codex CLI) moins cher.**",
                      text)

    def test_small_cost_gap_is_not_distinguishable(self):
        text = self.verdict(set(), cost_b=0.95)
        self.assertIn("coût : non distinguable", text)

    def test_resolution_first(self):
        text = self.verdict({"t0", "t1", "t2"})
        self.assertIn("**Verdict : résolution : A (yusAgent) résout 3 tâche(s) de plus", text)

    def test_too_few_common_tasks(self):
        text = self.verdict({"t0", "t1"}, tasks=6)
        self.assertIn("Moins de 5 tâches : **pas de verdict de coût**", text)


if __name__ == "__main__":
    unittest.main()
