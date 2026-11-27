#!/usr/bin/env python3
"""Coût par appel de la résolution de la clé avant chaque appel au modèle (ticket 42, décision 6).

YA-02, YA-10 (`docs/tickets/42-jeton-session-vivante.md`, §I.5, §J.2). Compare deux binaires
(`main` et la branche) sur la phase `request-sent` du timing des requêtes (`PI_REQUEST_TIMING=1`) :
`phaseMs`, l'écart entre le prompt construit et la remise de la requête au client du fournisseur
(`crates/pa-core/src/session_engine/request_timing/clock.rs`), où tombe la résolution.

Déroulé :
- `--blocks` blocs par binaire, de `--requests` prompts chacun, ordre alterné (main puis branche,
  puis branche puis main, …) ;
- chaque bloc : une session ACP neuve (`--mode acp`, démon isolé, dossier agent neuf), sous
  `sandbox-exec` (réseau limité à localhost), avec le serveur de `server.py` (scénario
  `smoke-warmup` : chaque prompt reçoit une réponse texte, une requête) ;
- pas de clé imposée à la création ; le fournisseur `capture` a une entrée `api_key` dans
  `auth.json`. **Avant chaque prompt**, `auth.json` reçoit une clé neuve `ya42-<binaire>-<bloc>-<n>` ;
  le serveur relève l'en-tête `x-api-key` de chaque requête (clés factices : le masquage des
  en-têtes du serveur est levé dans ce processus seulement). Preuve que la requête est passée par
  la résolution : elle porte la clé écrite juste avant elle.

Sortie : `<out>/report.json` et `<out>/report.md` ; médiane, p95 (rang le plus proche) de
`phaseMs` par binaire, différence des médianes, et clés vues par requête.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya42_cost.py --main <yusagent main> --branch <yusagent branche> \\
        --out <dossier neuf>

macOS (sandbox-exec) requis ; bibliothèque standard uniquement. La préparation (venv du kernel)
de `run.py` est faite une fois par binaire, réseau autorisé.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import queue
import shutil
import statistics
import subprocess
import sys
import threading
import time
from http.server import ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = load("ya_capture_run", "run.py")
server = load("ya_capture_server", "server.py")
# Clés factices : le serveur garde `x-api-key` dans ses relevés (dans ce processus seulement).
server.REDACTED_HEADERS = set()

WORK = Path(os.environ.get("YA42_COST_WORK", "/private/tmp/ya42-cost"))
PROMPT = "Réponds en une ligne."


class Server:
    """Le serveur de capture, dans ce processus (pour lever le masquage des en-têtes)."""

    def __init__(self, out: Path):
        out.mkdir(parents=True, exist_ok=True)
        self.out = out
        state = server.CaptureState(out, "smoke-warmup", None, 0)
        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), server.make_handler(state))
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, kwargs={"poll_interval": 0.2},
                         daemon=True).start()

    def keys(self) -> list[str | None]:
        """`x-api-key` des requêtes de la boucle (avec tools), dans l'ordre."""
        seen = []
        for meta_path in sorted(self.out.glob("*.meta.json")):
            body_path = meta_path.with_name(meta_path.name.replace(".meta.json", ".body.json"))
            try:
                body = json.loads(body_path.read_bytes())
            except (OSError, json.JSONDecodeError):
                continue
            if not body.get("tools"):
                continue
            headers = {k.lower(): v for k, v in json.loads(meta_path.read_text())["headers"].items()}
            seen.append(headers.get("x-api-key"))
        return seen

    def stop(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()


def write_key(agent: Path, key: str) -> None:
    path = agent / "auth.json"
    tmp = agent / "auth.json.ya42tmp"
    tmp.write_text(json.dumps({"capture": {"type": "api_key", "key": key}}) + "\n")
    os.chmod(tmp, 0o600)
    os.replace(tmp, path)


def phase_ms(agent: Path) -> list[float]:
    """`phaseMs` des entrées `request-sent` du journal de timing, dans l'ordre."""
    log = agent / "logs" / "agent.jsonl"
    found = []
    if not log.exists():
        return found
    for line in log.read_text().splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        fields = entry.get("fields") if isinstance(entry.get("fields"), dict) else entry
        if fields.get("phase") == "request-sent" and isinstance(fields.get("phaseMs"), (int, float)):
            found.append(float(fields["phaseMs"]))
    return found


def block(binary: Path, cache: Path, out: Path, project: Path, label: str, index: int,
          requests: int) -> dict:
    name = f"{label}-{index:02d}"
    case = out / name
    case.mkdir()
    agent = WORK / f"agent-{name}"
    shutil.rmtree(agent, ignore_errors=True)
    socket = WORK / f"{name}.sock"
    socket.unlink(missing_ok=True)
    capture = Server(case / "cap")
    run.write_models_json(agent, "anthropic-messages", capture.port)
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS, **run.PERMISSIONS_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model"}) + "\n")
    expected = [f"ya42-{label}-{index:02d}-{n:03d}" for n in range(requests)]
    write_key(agent, expected[0])
    env = run.base_env(cache / "home", agent, WORK / "tmp")
    env["PI_REQUEST_TIMING"] = "1"
    lines: queue.Queue[str] = queue.Queue()
    err = (case / "err.log").open("w")
    proc = subprocess.Popen(
        ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode", "acp",
         "--daemon-socket", str(socket)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env, cwd=project,
        text=True, bufsize=1)
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()

    def request(req_id: int, method: str, params: dict) -> dict | None:
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "method": method,
                                     "params": params}) + "\n")
        proc.stdin.flush()
        deadline = time.monotonic() + run.CASE_TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(lines.get(timeout=0.5))
            except (queue.Empty, json.JSONDecodeError):
                continue
            if incoming.get("id") == req_id and "method" not in incoming:
                return incoming
        return None

    stops = []
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        for n, key in enumerate(expected):
            write_key(agent, key)
            answer = request(10 + n, "session/prompt", {
                "sessionId": sid, "prompt": [{"type": "text", "text": PROMPT}]})
            stops.append(((answer or {}).get("result") or {}).get("stopReason"))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        err.close()
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {socket}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and subprocess.run(
                ["pgrep", "-f", f"--daemon-socket {socket}"], capture_output=True).returncode == 0:
            time.sleep(0.2)
        capture.stop()
    seen = capture.keys()
    phases = phase_ms(agent)
    matched = sum(1 for got, want in zip(seen, expected) if got == want)
    result = {"block": name, "stops": stops, "keys_seen": seen, "keys_expected": expected,
              "matched": matched, "requests_seen": len(seen), "phase_ms": phases}
    print(f"{name} : requêtes={len(seen)} clés conformes={matched}/{requests} "
          f"phaseMs={len(phases)} fins={sorted(set(map(str, stops)))}")
    return result


def p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)]


def summary(blocks: list[dict]) -> dict:
    phases = [value for entry in blocks for value in entry["phase_ms"]]
    return {
        "blocks": len(blocks),
        "requests": sum(entry["requests_seen"] for entry in blocks),
        "keys_matched": sum(entry["matched"] for entry in blocks),
        "phase_samples": len(phases),
        "median_ms": statistics.median(phases) if phases else None,
        "p95_ms": p95(phases) if phases else None,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--main", required=True, type=Path)
    parser.add_argument("--branch", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--blocks", type=int, default=10)
    parser.add_argument("--requests", type=int, default=20)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True)
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    binaries = {"main": args.main.resolve(), "branch": args.branch.resolve()}
    versions = {}
    for label, binary in binaries.items():
        versions[label] = subprocess.run(
            [str(binary), "--version"], capture_output=True, text=True,
            env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
        run.prepare(binary, cache, versions[label])
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    results: dict[str, list[dict]] = {"main": [], "branch": []}
    for index in range(args.blocks):
        order = ["main", "branch"] if index % 2 == 0 else ["branch", "main"]
        for label in order:
            results[label].append(block(binaries[label], cache, out, project, label, index,
                                        args.requests))
    report = {"versions": versions, "blocks": args.blocks, "requests_per_block": args.requests,
              "summary": {label: summary(entries) for label, entries in results.items()},
              "results": results}
    main_median = report["summary"]["main"]["median_ms"]
    branch_median = report["summary"]["branch"]["median_ms"]
    report["median_difference_ms"] = (None if main_median is None or branch_median is None
                                      else round(branch_median - main_median, 3))
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    rows = ["| Binaire | Version | Requêtes | Clés conformes | Échantillons | Médiane (ms) | p95 (ms) |",
            "|---|---|---|---|---|---|---|"]
    for label, stats in report["summary"].items():
        rows.append(f"| {label} | `{versions[label]}` | {stats['requests']} | "
                    f"{stats['keys_matched']} | {stats['phase_samples']} | {stats['median_ms']} | "
                    f"{stats['p95_ms']} |")
    text = "\n".join(rows) + f"\n\nDifférence des médianes (branche − main) : " \
        f"{report['median_difference_ms']} ms\n"
    (out / "report.md").write_text(text)
    print(text)


if __name__ == "__main__":
    main()
