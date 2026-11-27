#!/usr/bin/env python3
"""Retour en arrière (ticket 43) : ce que le modèle reçoit avant et après, requête par requête.

YA-18 (`docs/tickets/43-retour-arriere.md`, §E, §I.6). Une session ACP sur le binaire donné, avec
le serveur de capture (`server.py`, scénario `smoke-warmup`, API `anthropic-messages`) :
trois prompts, retour en arrière avant le troisième (`_yus/session/rewind`, `to` = `rewindTo` de
son `turnStart`), puis le troisième renvoyé modifié. Contrôles, sur les requêtes de la boucle
(celles qui portent des tools), dans l'ordre d'arrivée :
- C1 : quatre requêtes ;
- C2 : prompt système, tools et paramètres (tout le corps hors `messages`) identiques d'une
  requête à l'autre, UUID remplacés comme `compare.py` ;
- C3 : les messages de la 4e requête commencent par ceux de la 3e jusqu'au point de retour (tous
  ses messages sauf le dernier prompt), marques `cache_control` retirées ; on note aussi si
  l'égalité tient octet pour octet, marques comprises ;
- C4 : la suite de la 4e requête porte le texte fixe (D2) une fois, puis le prompt modifié, et
  rien du troisième prompt d'origine ;
- C5 : chaque `turnStart` de prompt porte `rewindTo` ; le retour répond `{}` ;
- C6 : le journal d'usage porte `invalidation: {"cause": "rewind"}` sur la 4e ligne `main`
  seulement.

Sortie : `<out>/report.json`, `<out>/report.md`, les captures sous `<out>/cap`. Code de sortie 0
si tout est conforme (`CONFORME`), 1 sinon.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya43_rewind_check.py --binary <yusagent> --out <dossier neuf>

macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
"""

import argparse
import importlib.util
import json
import os
import queue
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
META = "ai.primeintellect.prime-agent"
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
REWOUND = ("[conversation-rewound] The conversation went back to an earlier point; the turns "
           "after it are gone from your context. The Python kernel kept its state: variables, "
           "imports and background commands from those turns may still exist.")
PROMPTS = ["Premier message.", "Deuxième message.", "Troisième message."]
CHANGED = "Troisième message, modifié."


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = load("ya_capture_run", "run.py")
WORK = Path(os.environ.get("YA43_CHECK_WORK", "/private/tmp/ya43-check"))


def strip_cache(value):
    """La valeur sans les marques `cache_control` (elles suivent la fin de chaque requête)."""
    if isinstance(value, dict):
        return {key: strip_cache(item) for key, item in value.items() if key != "cache_control"}
    if isinstance(value, list):
        return [strip_cache(item) for item in value]
    return value


def normalized(value) -> str:
    return UUID.sub("<uuid>", json.dumps(value, sort_keys=True, ensure_ascii=False))


def turn_start(frame: dict) -> dict | None:
    if frame.get("method") != "session/update":
        return None
    update = (frame.get("params") or {}).get("update") or {}
    start = ((update.get("_meta") or {}).get(META) or {}).get("turnStart")
    return start if isinstance(start, dict) else None


def texts(message: dict) -> str:
    content = message.get("content")
    if isinstance(content, str):
        return content
    return "".join(block.get("text", "") for block in content or [] if isinstance(block, dict))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True)
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve()
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}
                             ).stdout.strip()
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    agent = WORK / "agent"
    socket = WORK / "daemon.sock"
    port_file = out / "port"
    server = subprocess.Popen(
        [sys.executable, "-I", str(HERE / "server.py"), "--out", str(out / "cap"),
         "--scenario", "smoke-warmup", "--port-file", str(port_file)],
        stdout=subprocess.DEVNULL, stderr=(out / "server.log").open("w"))
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and not (
            port_file.exists() and port_file.read_text().strip()):
        time.sleep(0.05)
    run.write_models_json(agent, "anthropic-messages", int(port_file.read_text().strip()))
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS, **run.PERMISSIONS_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model"}) + "\n")
    env = run.base_env(cache / "home", agent, WORK / "tmp")
    lines: queue.Queue[str] = queue.Queue()
    frames: list[dict] = []
    err = (out / "err.log").open("w")
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
            frames.append(incoming)
            if incoming.get("id") == req_id and "method" not in incoming:
                return incoming
        return None

    starts: list[dict] = []
    rewind_answer = None
    session_file = None
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        result = (new or {}).get("result") or {}
        sid = result.get("sessionId")
        session_file = ((result.get("_meta") or {}).get(META) or {}).get("saved", {}).get("path")
        for n, text in enumerate([*PROMPTS, None]):
            if text is None:
                to = (starts[2] if len(starts) > 2 else {}).get("rewindTo")
                rewind_answer = request(20, "_yus/session/rewind", {"sessionId": sid, "to": to})
                text = CHANGED
            before = len(frames)
            request(10 + n, "session/prompt",
                    {"sessionId": sid, "prompt": [{"type": "text", "text": text}]})
            start = next((turn_start(frame) for frame in frames[before:]
                          if (turn_start(frame) or {}).get("origin") == "prompt"), None)
            starts.append(start or {})
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

    bodies = []
    for path in sorted((out / "cap").glob("*.body.json")):
        try:
            body = json.loads(path.read_bytes())
        except (OSError, json.JSONDecodeError):
            continue
        if body.get("tools"):
            bodies.append(body)
    checks = {}
    checks["C1_quatre_requetes"] = len(bodies) == 4
    if len(bodies) == 4:
        rest = [normalized({k: v for k, v in body.items() if k != "messages"}) for body in bodies]
        checks["C2_hors_messages_identiques"] = len(set(rest)) == 1
        third, fourth = bodies[2]["messages"], bodies[3]["messages"]
        point = len(third) - 1
        checks["C3_prefixe_jusqu_au_point"] = (
            normalized(strip_cache(fourth[:point])) == normalized(strip_cache(third[:point])))
        checks["C3_octet_pour_octet_marques_comprises"] = (
            normalized(fourth[:point]) == normalized(third[:point]))
        tail = "\n".join(texts(message) for message in fourth[point:])
        checks["C4_texte_fixe_une_fois"] = tail.count(REWOUND) == 1
        checks["C4_prompt_modifie_apres"] = (CHANGED in tail and REWOUND in tail
                                             and tail.index(REWOUND) < tail.index(CHANGED))
        checks["C4_rien_du_prompt_d_origine"] = PROMPTS[2] not in json.dumps(
            bodies[3], ensure_ascii=False)
    checks["C5_rewindTo_dans_chaque_turnStart"] = (
        len(starts) == 4 and all(isinstance(start.get("rewindTo"), str) for start in starts))
    checks["C5_retour_accepte"] = ((rewind_answer or {}).get("result") == {})
    mains = []
    if session_file:
        journal = Path(session_file).with_name(Path(session_file).stem + ".usage.ndjson")
        if journal.exists():
            mains = [entry for entry in map(json.loads, journal.read_text().splitlines())
                     if entry.get("kind") == "main"]
    checks["C6_invalidation_rewind_sur_la_4e_seulement"] = (
        len(mains) == 4
        and [entry.get("invalidation") for entry in mains]
        == [None, None, None, {"cause": "rewind"}])
    # La marque `cache_control` suit la fin de chaque requête : C3 octet pour octet est relevé,
    # pas exigé (le cache du fournisseur relit le préfixe commun quelle que soit sa place).
    required = {name: ok for name, ok in checks.items()
                if name != "C3_octet_pour_octet_marques_comprises"}
    conforming = bool(required) and all(required.values()) and len(bodies) == 4
    report = {"version": version, "checks": checks, "conforming": conforming,
              "requests": len(bodies), "turn_starts": starts,
              "rewind_answer": rewind_answer,
              "invalidations": [entry.get("invalidation") for entry in mains]}
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    rows = [f"Binaire : `{version}`", "", "| Contrôle | Résultat |", "|---|---|"]
    rows += [f"| {name} | {'oui' if ok else 'NON'} |" for name, ok in checks.items()]
    rows += ["", "CONFORME" if conforming else "NON CONFORME"]
    text = "\n".join(rows) + "\n"
    (out / "report.md").write_text(text)
    print(text)
    sys.exit(0 if conforming else 1)


if __name__ == "__main__":
    main()
