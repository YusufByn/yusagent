#!/usr/bin/env python3
"""Trames ACP du vrai binaire, pour comparer l'agent factice de l'extension VS Code.

YA-10 (ticket 24, PR 3). Réutilise les fonctions de `run.py` sans le modifier : serveur de
capture (modèle factice), `sandbox-exec` (réseau limité à localhost), démon isolé
(`--daemon-socket`), HOME et venv du kernel du banc (préparation avec réseau, une fois par
binaire, comme `run.py`).

Trois sessions ACP, chacune dans `<sortie>/<cas>/transcript.jsonl` (une ligne par trame, avec son
sens), plus les corps de requête du serveur de capture dans `<cas>/cap/` :
  A-display-auto          scénario `display` en « auto » : `bash()` lu, `bash()` non lu, `edit()`
                          qui crée puis modifie ; le client attend `--linger` secondes après la
                          réponse du prompt (tour de réveil `[bash-done]`, ticket 25) ;
  B-basic-manual-reject   scénario `basic` en « manual » : la demande d'accord est refusée ;
  C-resources             ticket 31 (X3), scénario `basic` en « auto » : prompt 1, puis les méthodes
                          des écrans (`_yus/settings/get`, `set` accepté, `set permissions` refusé,
                          `_yus/skills/list`, `create`, `set` éteint puis rallumé, `_yus/mcp/list`),
                          `session/set_config_option permissions manual`, puis prompt 2 (réponse
                          texte) : la notice `_yus/notice` du skill créé.
  D-steer                 ticket 39 (voie B), scénario `steer` en « auto » : pendant la cellule
                          longue, `_yus/session/steer` (accepté) puis un second au tour périmé
                          (refusé `-32603`). La requête qui suit la cellule finit par le résultat
                          de la cellule puis le message ; préfixe N → N+1 exact (`analyze.py`).
  E-steer-skills          ticket 39, scénario `steer-skills` : la cellule crée un skill du projet
                          avant le message. Avec la voie B, la requête du message ne porte pas
                          `[skills-changed]` ; l'annonce part au prompt suivant (prompt 2).

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/acp_frames.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si chaque dernier prompt finit par `end_turn` sans processus restant, et si le cas C
a sa notice et le refus de `permissions`, 1 sinon.
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
_spec = importlib.util.spec_from_file_location("ya_capture_analyze", HERE / "analyze.py")
analyze = importlib.util.module_from_spec(_spec)
sys.modules["ya_capture_analyze"] = analyze
_spec.loader.exec_module(analyze)

WORK = Path(os.environ.get("YA_ACP_FRAMES_WORK", "/private/tmp/ya-acp-frames"))


def binary_pids(binary: Path) -> set[str]:
    """Processus du binaire en cours (le démon partagé d'un autre usage compris)."""
    found = subprocess.run(["pgrep", "-f", str(binary)], capture_output=True, text=True)
    return set(found.stdout.split())


def session(binary: Path, cache: Path, out: Path, project: Path, name: str, scenario: str,
            permissions: str, answer: str, linger: float, resources: bool = False,
            steer: bool = False) -> dict:
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
    checks: dict = {}
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        if steer:
            prompt, checks = steer_turn(write, pump, request, transcript, project, sid,
                                        second=scenario == "steer-skills")
        else:
            prompt = request(3, "session/prompt", {
                "sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT}]})
        pump(None, linger)
        if resources:
            prompt, checks = screens(request, sid)
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
    extra = f" contrôles={checks}" if checks else ""
    print(f"{name} : prompt={result} trames={len(transcript)} processus restants={leftovers}{extra}")
    return {"prompt": result, "frames": len(transcript), "leftovers": leftovers, "checks": checks}


SKILL = "ya31-frames"
META = "ai.primeintellect.prime-agent"
STEER_TEXT = "Message pendant le tour : ajoute une ligne au résumé."


def steer_turn(write, pump, request, transcript: list[dict], project: Path, sid: str,
               second: bool) -> tuple[dict | None, dict]:
    """Cas D et E (ticket 39) : le prompt, puis `_yus/session/steer` pendant la cellule longue, puis
    un second au tour périmé ; pour E, le prompt 2. Rend la dernière réponse de prompt et les
    contrôles des trames."""
    started = project / "ya-steer-started"
    started.unlink(missing_ok=True)
    write({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
           "params": {"sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT}]}})
    deadline = time.monotonic() + run.CASE_TIMEOUT_S
    while time.monotonic() < deadline and not started.exists():
        pump(None, 0.5)
    received = [row["msg"] for row in transcript if row["dir"] == "agent->client"]
    meta = lambda m: (((m.get("params") or {}).get("update") or {}).get("_meta") or {}).get(META) or {}
    turn = next((meta(m).get("promptTurnId") for m in received
                 if (meta(m).get("turnStart") or {}).get("origin") == "prompt"), None)
    steered = request(4, "_yus/session/steer", {
        "sessionId": sid, "prompt": [{"type": "text", "text": STEER_TEXT}], "turn": turn}) or {}
    stale = request(5, "_yus/session/steer", {
        "sessionId": sid, "prompt": [{"type": "text", "text": "tour périmé"}],
        "turn": (turn or 0) + 1}) or {}
    answered = lambda m: m.get("id") == 3 and "method" not in m
    prompt = next((row["msg"] for row in transcript
                   if row["dir"] == "agent->client" and answered(row["msg"])), None)
    if prompt is None:
        prompt = pump(answered, run.CASE_TIMEOUT_S)
    received = [row["msg"] for row in transcript if row["dir"] == "agent->client"]
    checks = {
        "steer_accepte": steered.get("result") == {},
        "perime_refuse": (stale.get("error") or {}).get("code") == -32603,
        "turnStart_steer": any((meta(m).get("turnStart") or {}).get("origin") == "steer"
                               and meta(m).get("promptTurnId") == turn for m in received),
    }
    if second:
        prompt = request(6, "session/prompt", {
            "sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT_2}]})
    shutil.rmtree(project / ".yusagent" / "skills" / "ya39-steer", ignore_errors=True)
    started.unlink(missing_ok=True)
    return prompt, checks


def blocks(request) -> list[tuple[str, str, str]]:
    """Les blocs de la conversation d'une requête Anthropic : (rôle, type, texte)."""
    flat = []
    for message in request.conversation():
        content = message.get("content")
        if isinstance(content, str):
            flat.append((message.get("role"), "text", content))
            continue
        for block in content or []:
            text = block.get("text") or ""
            if block.get("type") == "tool_result":
                inner = block.get("content")
                text = inner if isinstance(inner, str) else "".join(
                    part.get("text", "") for part in inner or [])
            flat.append((message.get("role"), block.get("type"), text))
    return flat


def steer_prefix(case: Path, with_skills: bool) -> dict:
    """Préfixe N → N+1 des requêtes du cas (`analyze.compare_pair`) et place du message."""
    mains = analyze.main_requests(analyze.load_case(case))
    pairs = [analyze.compare_pair(a, b) for a, b in zip(mains, mains[1:])]
    report = {"requetes": len(mains),
              "paires": [{k: pair[k] for k in ("pair", "system_identique", "tools_identiques",
                                                 "params_identiques", "messages_n", "messages_n1",
                                                 "prefixe_exact", "prefixe_exact_hors_cache_control")}
                         for pair in pairs]}
    report["prefixe_ok"] = bool(pairs) and all(
        pair["params_identiques"] and pair["prefixe_exact_hors_cache_control"] for pair in pairs)
    # La requête qui suit la cellule : celle qui porte le message.
    carrier = next((index for index, req in enumerate(mains)
                    if any(STEER_TEXT in text for _, _, text in blocks(req))), None)
    report["requete_du_message"] = carrier
    if carrier is not None:
        tail = blocks(mains[carrier])[-2:]
        report["fin_de_la_requete"] = [(role, kind) for role, kind, _ in tail]
        report["message_apres_resultat"] = (
            len(tail) == 2 and tail[0][1] == "tool_result" and "STEER_CELL" in tail[0][2]
            and tail[1][1] == "text" and tail[1][2] == STEER_TEXT)
    if with_skills:
        announced = [index for index, req in enumerate(mains)
                     if any("[skills-changed]" in text for _, _, text in blocks(req))]
        report["annonce_dans_les_requetes"] = announced
        report["annonce_au_prompt_suivant"] = (
            carrier is not None and bool(announced) and announced[0] > carrier
            and any(run.PROMPT_2 in text for _, _, text in blocks(mains[announced[0]])))
    return report


def screens(request, sid: str) -> tuple[dict | None, dict]:
    """Cas C (ticket 31, X3) : les méthodes des écrans après le prompt 1 (le moteur est construit),
    puis le prompt 2. Rend la réponse du prompt 2 et les contrôles du cas."""
    ids = iter(range(10, 100))
    answer = lambda method, params: request(next(ids), method, params) or {}
    answer("_yus/settings/get", {})
    answer("_yus/settings/set", {"key": "retry.enabled", "value": False})
    refused = answer("_yus/settings/set", {"key": "permissions", "value": "manual"})
    answer("_yus/skills/list", {})
    created = answer("_yus/skills/create", {"name": SKILL, "description": "Skill du cas C (ticket 31)."})
    path = (created.get("result") or {}).get("created")
    if path:
        answer("_yus/skills/set", {"path": path, "enabled": False})
        answer("_yus/skills/set", {"path": path, "enabled": True})
    answer("_yus/mcp/list", {})
    mode = answer("session/set_config_option", {"sessionId": sid, "configId": "permissions", "value": "manual"})
    prompt = request(next(ids), "session/prompt", {
        "sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT_2}]})
    return prompt, {
        "refus_permissions": (refused.get("error") or {}).get("code") == -32602,
        "skill_cree": bool(path),
        "permissions_manual": any(o.get("id") == "permissions" and o.get("currentValue") == "manual"
                                  for o in (mode.get("result") or {}).get("configOptions", [])),
    }


def notice_count(case: Path) -> int:
    rows = [json.loads(line) for line in (case / "transcript.jsonl").read_text().splitlines()]
    return sum(1 for row in rows if row["dir"] == "agent->client"
               and "_yus/notice" in (((row["msg"].get("params") or {}).get("update") or {}).get("_meta") or {}))


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
        "C-resources": session(binary, cache, out, project, "C-resources", "basic", "auto",
                               "allow", 2.0, resources=True),
        "D-steer": session(binary, cache, out, project, "D-steer", "steer", "auto", "allow",
                           2.0, steer=True),
        "E-steer-skills": session(binary, cache, out, project, "E-steer-skills", "steer-skills",
                                  "auto", "allow", 2.0, steer=True),
    }
    (out / "acp-frames.json").write_text(json.dumps(
        {"binary": str(binary), "version": version, "cases": results}, indent=2) + "\n")
    ok = all(r["prompt"] == {"stopReason": "end_turn"} and not r["leftovers"]
             and all(r["checks"].values()) for r in results.values())
    notices = notice_count(out / "C-resources")
    print(f"C-resources : notices _yus/notice={notices}")
    ok = ok and notices == 1
    prefixes = {"D-steer": steer_prefix(out / "D-steer", False),
                "E-steer-skills": steer_prefix(out / "E-steer-skills", True)}
    (out / "steer-prefix.json").write_text(json.dumps(prefixes, indent=2, ensure_ascii=False) + "\n")
    for name, report in prefixes.items():
        print(f"{name} : préfixe N → N+1 {report['paires']}")
        print(f"{name} : requête du message {report.get('requete_du_message')}, fin "
              f"{report.get('fin_de_la_requete')}, message après le résultat "
              f"{report.get('message_apres_resultat')}"
              + (f", annonce {report.get('annonce_dans_les_requetes')}, au prompt suivant "
                 f"{report.get('annonce_au_prompt_suivant')}" if "annonce_dans_les_requetes" in report else ""))
        ok = ok and report["prefixe_ok"] and bool(report.get("message_apres_resultat"))
    ok = ok and bool(prefixes["E-steer-skills"].get("annonce_au_prompt_suivant"))
    print("CONFORME" if ok else "NON CONFORME")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
