#!/usr/bin/env python3
"""Coût de `rewindTo` par prompt : la lecture de la feuille avant le `turnStart` (ticket 43, §I.6).

YA-18 (`docs/tickets/43-retour-arriere.md`, §I.6, §K). Compare deux binaires (`main` et la branche)
sur le délai, côté client ACP, entre l'envoi de `session/prompt` et la réception du
`session_info_update` qui porte le `turnStart` du prompt. Sur `main`, ce `turnStart` part sans
aller-retour au worker ; sur la branche, le pont lit d'abord la feuille (action `leaf` de
`ya_permissions`). L'écart des médianes isole cette lecture. Règle du ticket 42 : surcoût médian
< 10 ms.

Déroulé :
- `--blocks` blocs par binaire, de `--requests` prompts chacun, ordre alterné (main puis branche,
  puis branche puis main, …) ;
- chaque bloc : une session ACP neuve (`--mode acp`, démon isolé, dossier agent neuf), sous
  `sandbox-exec` (réseau limité à localhost), avec le serveur de `server.py` (scénario
  `smoke-warmup` : chaque prompt reçoit une réponse texte, une requête) ;
- chaque délai est relevé avec la présence de `rewindTo` dans le `turnStart` (attendue sur la
  branche, absente sur `main`).

Sortie : `<out>/report.json` et `<out>/report.md` ; médiane, p95 (rang le plus proche) par binaire,
différence des médianes, nombre de `turnStart` avec `rewindTo`.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya43_rewindto_cost.py --main <yusagent main> \\
        --branch <yusagent branche> --out <dossier neuf>

macOS (sandbox-exec) requis ; bibliothèque standard uniquement. La préparation (venv du kernel)
de `run.py` est faite une fois par binaire, réseau autorisé.
"""

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
from pathlib import Path

HERE = Path(__file__).resolve().parent
META = "ai.primeintellect.prime-agent"


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = load("ya_capture_run", "run.py")

WORK = Path(os.environ.get("YA43_COST_WORK", "/private/tmp/ya43-cost"))
PROMPT = "Réponds en une ligne."


def capture_server(case: Path, port_file: Path) -> subprocess.Popen:
    """Le serveur de capture (`server.py`, scénario `smoke-warmup`) ; attend son port."""
    proc = subprocess.Popen(
        [sys.executable, "-I", str(HERE / "server.py"), "--out", str(case / "cap"),
         "--scenario", "smoke-warmup", "--port-file", str(port_file)],
        stdout=subprocess.DEVNULL, stderr=(case / "server.log").open("w"))
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and not (
            port_file.exists() and port_file.read_text().strip()):
        time.sleep(0.05)
    return proc


def turn_start(frame: dict) -> dict | None:
    if frame.get("method") != "session/update":
        return None
    update = (frame.get("params") or {}).get("update") or {}
    start = ((update.get("_meta") or {}).get(META) or {}).get("turnStart")
    return start if isinstance(start, dict) else None


def block(binary: Path, cache: Path, out: Path, project: Path, label: str, index: int,
          requests: int) -> dict:
    name = f"{label}-{index:02d}"
    case = out / name
    case.mkdir()
    agent = WORK / f"agent-{name}"
    shutil.rmtree(agent, ignore_errors=True)
    socket = WORK / f"{name}.sock"
    socket.unlink(missing_ok=True)
    port_file = case / "port"
    server = capture_server(case, port_file)
    port = int(port_file.read_text().strip())
    run.write_models_json(agent, "anthropic-messages", port)
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS, **run.PERMISSIONS_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model"}) + "\n")
    env = run.base_env(cache / "home", agent, WORK / "tmp")
    lines: queue.Queue[tuple[float, str]] = queue.Queue()
    err = (case / "err.log").open("w")
    proc = subprocess.Popen(
        ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode", "acp",
         "--daemon-socket", str(socket)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env, cwd=project,
        text=True, bufsize=1)
    threading.Thread(
        target=lambda: [lines.put((time.monotonic(), line)) for line in proc.stdout],
        daemon=True).start()

    def request(req_id: int, method: str, params: dict) -> tuple[float, list, dict | None]:
        """Envoie une requête ; l'heure d'envoi, les trames reçues jusqu'à sa réponse, la réponse."""
        seen = []
        sent = time.monotonic()
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "method": method,
                                     "params": params}) + "\n")
        proc.stdin.flush()
        deadline = time.monotonic() + run.CASE_TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                at, line = lines.get(timeout=0.5)
                incoming = json.loads(line)
            except (queue.Empty, json.JSONDecodeError):
                continue
            seen.append((at, incoming))
            if incoming.get("id") == req_id and "method" not in incoming:
                return sent, seen, incoming
        return sent, seen, None

    delays = []
    with_rewind_to = 0
    stops = []
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        _, _, new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        for n in range(requests):
            sent, seen, answer = request(10 + n, "session/prompt", {
                "sessionId": sid, "prompt": [{"type": "text", "text": PROMPT}]})
            stops.append(((answer or {}).get("result") or {}).get("stopReason"))
            for at, frame in seen:
                start = turn_start(frame)
                if start and start.get("origin") == "prompt":
                    delays.append(round((at - sent) * 1000, 3))
                    with_rewind_to += "rewindTo" in start
                    break
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
        server.terminate()
        server.wait(timeout=10)
    result = {"block": name, "stops": stops, "delays_ms": delays, "with_rewind_to": with_rewind_to}
    print(f"{name} : turnStart={len(delays)}/{requests} avec rewindTo={with_rewind_to} "
          f"médiane={statistics.median(delays) if delays else None} ms "
          f"fins={sorted(set(map(str, stops)))}")
    return result


def p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)]


def summary(blocks: list[dict]) -> dict:
    delays = [value for entry in blocks for value in entry["delays_ms"]]
    return {
        "blocks": len(blocks),
        "samples": len(delays),
        "with_rewind_to": sum(entry["with_rewind_to"] for entry in blocks),
        "median_ms": statistics.median(delays) if delays else None,
        "p95_ms": p95(delays) if delays else None,
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
    rows = ["| Binaire | Version | Échantillons | Avec rewindTo | Médiane (ms) | p95 (ms) |",
            "|---|---|---|---|---|---|"]
    for label, stats in report["summary"].items():
        rows.append(f"| {label} | `{versions[label]}` | {stats['samples']} | "
                    f"{stats['with_rewind_to']} | {stats['median_ms']} | {stats['p95_ms']} |")
    text = "\n".join(rows) + f"\n\nDifférence des médianes (branche − main) : " \
        f"{report['median_difference_ms']} ms\n"
    (out / "report.md").write_text(text)
    print(text)


if __name__ == "__main__":
    main()
