#!/usr/bin/env python3
"""Trames ACP du vrai binaire, pour comparer l'agent factice de l'extension VS Code.

YA-10 (ticket 24, PR 3). Réutilise les fonctions de `run.py` sans le modifier : serveur de
capture (modèle factice), `sandbox-exec` (réseau limité à localhost), démon isolé
(`--daemon-socket`), HOME et venv du kernel du banc (préparation avec réseau, une fois par
binaire, comme `run.py`).

Deux sessions ACP, chacune dans `<sortie>/<cas>/transcript.jsonl` (une ligne par trame, avec son
sens), plus les corps de requête du serveur de capture dans `<cas>/cap/` :
  A-display-auto          scénario `display` en « auto » : `bash()` lu, `bash()` non lu, `edit()`
                          qui crée puis modifie ; le client attend `--linger` secondes après la
                          réponse du prompt (tour de réveil `[bash-done]`, ticket 25) ;
  B-basic-manual-reject   scénario `basic` en « manual » : la demande d'accord est refusée.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/acp_frames.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si les deux prompts finissent par `end_turn` sans processus restant, 1 sinon.
Comparaison avec l'agent factice : `compare_frames.py <dossier>`.
macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("ya_capture_run", HERE / "run.py")
run = importlib.util.module_from_spec(_spec)
sys.modules["ya_capture_run"] = run
_spec.loader.exec_module(run)

WORK = Path(os.environ.get("YA_ACP_FRAMES_WORK", "/private/tmp/ya-acp-frames"))


def binary_pids(binary: Path) -> set[str]:
    """Processus du binaire en cours (le démon partagé d'un autre usage compris)."""
    found = subprocess.run(["pgrep", "-f", str(binary)], capture_output=True, text=True)
    return set(found.stdout.split())


def session(binary: Path, cache: Path, out: Path, project: Path, name: str, scenario: str,
            permissions: str, answer: str, linger: float) -> dict:
    case = out / name
    case.mkdir()
    agent = WORK / f"agent-{name}"
    socket = WORK / f"{name}.sock"
    before = binary_pids(binary)
    server = run.CaptureServer(case / "cap", scenario, [])
    run.write_models_json(agent, "anthropic-messages", server.port)
    # Pas `run.write_settings` : il force « auto » ; ici, le mode du cas.
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model",
        "permissions": permissions}) + "\n")
    env = run.base_env(cache / "home", agent, WORK / "tmp")
    lines: queue.Queue[str] = queue.Queue()
    transcript: list[dict] = []
    err = (case / "err.log").open("w")
    proc = subprocess.Popen(
        ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode", "acp",
         "--daemon-socket", str(socket)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env, cwd=project,
        text=True, bufsize=1)
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()

    def write(message: dict) -> None:
        transcript.append({"dir": "client->agent", "msg": message})
        proc.stdin.write(json.dumps(message) + "\n")
        proc.stdin.flush()

    def pump(until, seconds: float):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(lines.get(timeout=0.5))
            except (queue.Empty, json.JSONDecodeError):
                continue
            transcript.append({"dir": "agent->client", "msg": incoming})
            if incoming.get("method") == "session/request_permission":
                option = "allow-once" if answer == "allow" else "reject-once"
                write({"jsonrpc": "2.0", "id": incoming["id"],
                       "result": {"outcome": {"outcome": "selected", "optionId": option}}})
            if until is not None and until(incoming):
                return incoming
        return None

    def request(req_id: int, method: str, params: dict) -> dict | None:
        write({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
        return pump(lambda m: m.get("id") == req_id and "method" not in m, run.CASE_TIMEOUT_S)

    prompt = None
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        prompt = request(3, "session/prompt", {
            "sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT}]})
        pump(None, linger)
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        err.close()
        # Le superviseur de ce socket arrête ses workers et leurs kernels.
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {socket}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and binary_pids(binary) - before:
            time.sleep(0.2)
        server.stop()
    (case / "transcript.jsonl").write_text(
        "".join(json.dumps(entry, ensure_ascii=False) + "\n" for entry in transcript))
    result = (prompt or {}).get("result") or (prompt or {}).get("error")
    leftovers = sorted(binary_pids(binary) - before)
    print(f"{name} : prompt={result} trames={len(transcript)} processus restants={leftovers}")
    return {"prompt": result, "frames": len(transcript), "leftovers": leftovers}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--linger", type=float, default=40.0,
                        help="secondes d'attente après la réponse du prompt du cas A (défaut 40)")
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer (rm -rf {out}) "
                 "ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
    print(f"binaire : {version}")
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    results = {
        "A-display-auto": session(binary, cache, out, project, "A-display-auto", "display",
                                  "auto", "allow", args.linger),
        "B-basic-manual-reject": session(binary, cache, out, project, "B-basic-manual-reject",
                                         "basic", "manual", "reject", 2.0),
    }
    (out / "acp-frames.json").write_text(json.dumps(
        {"binary": str(binary), "version": version, "cases": results}, indent=2) + "\n")
    ok = all(r["prompt"] == {"stopReason": "end_turn"} and not r["leftovers"] for r in results.values())
    print("CONFORME" if ok else "NON CONFORME")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
