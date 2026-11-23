#!/usr/bin/env python3
"""Relevé de `attachedClients` dans les lignes de `list` d'un vrai démon (ticket 29, vérification A).

Sur le vrai binaire, démon isolé, serveur de capture (modèle factice), `sandbox-exec`, HOME et venv du
banc (fonctions de `run.py` et `resume_probe.py`, sans les modifier) :

  1. P crée une conversation, envoie un prompt, puis ferme : son worker reste dans le démon, sans
     client ;
  2. A crée une conversation, envoie un prompt et reste ouvert : session attachée, au repos ;
  3. `list` sur le socket du démon (client brut, sans `attach`) : lignes relevées ;
  4. A ferme, puis `list` à nouveau.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I \\
      bench/tools/capture/attached_clients_probe.py --binary <yusagent> --out <dossier neuf>

Sortie : `attached-clients.json` (lignes complètes) et un résumé par ligne. Aucun modèle réel. macOS
(sandbox-exec) requis ; bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import socket as so
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ.setdefault("YA_RESUME_PROBE_WORK", "/private/tmp/ya-attached-probe")
SUMMARY_KEYS = ("attachedClients", "isSessionActive", "lifecycle", "activity", "messageCount",
                "hasRegisteredCronJob")


def _load(name: str, file: str):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


probe = _load("ya_resume_probe", "resume_probe.py")
run = probe.run
WORK = probe.WORK


def daemon_list(path: Path) -> tuple[dict, dict]:
    """Hello et réponse à `list`, par un client brut du socket (sans `attach`)."""
    sock = so.socket(so.AF_UNIX)
    sock.settimeout(30)
    sock.connect(str(path))
    reader = sock.makefile("r", encoding="utf-8")
    hello = json.loads(reader.readline())
    envelope = {"type": "command", "id": "probe-list", "protocol": hello.get("protocol") or {},
                "clientId": "attached-probe", "command": {"type": "list"}}
    sock.sendall((json.dumps(envelope) + "\n").encode())
    for line in reader:
        frame = json.loads(line)
        if frame.get("type") == "response" and frame.get("id") == "probe-list":
            sock.close()
            return hello, frame
    raise RuntimeError("pas de réponse à list")


def rows_of(response: dict) -> list[dict]:
    return ((response.get("data") or {}).get("sessions")) or []


def summary(rows: list[dict], files: dict[str, str]) -> list[dict]:
    out = []
    for row in rows:
        name = files.get(str(row.get("sessionFile")), "?")
        out.append({"qui": name, **{key: row.get(key) for key in SUMMARY_KEYS}})
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
    print(f"binaire : {binary} ({version})")
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    agent, env = probe.setup(cache, "attached")
    server = probe.serve(out, agent)
    sock = WORK / "d.sock"
    meta = "ai.primeintellect.prime-agent"
    info: dict = {"binaire": str(binary), "version": version}

    def new(tag: str):
        bridge = probe.Bridge(binary, sock, env, project, out, tag)
        bridge.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        response = bridge.request("session/new", {"cwd": str(project), "mcpServers": []}) or {}
        result = response.get("result") or {}
        saved = ((result.get("_meta") or {}).get(meta) or {}).get("saved") or {}
        return bridge, result.get("sessionId"), saved.get("path")

    try:
        p, psid, ppath = new("P")
        p.prompt(psid, "un")
        p.close()
        time.sleep(4)
        a, asid, apath = new("A")
        a.prompt(asid, "deux")
        files = {str(ppath): "P (fermé, sans client)", str(apath): "A (ouvert, attaché)"}
        hello, listed = daemon_list(sock)
        info["hello"] = {"appVersion": hello.get("appVersion"), "schemaId": hello.get("schemaId"),
                         "runtime": hello.get("runtime")}
        info["avec_A_ouvert"] = {"resume": summary(rows_of(listed), files), "list": listed}
        a.close()
        time.sleep(4)
        _, listed = daemon_list(sock)
        info["apres_fermeture_de_A"] = {"resume": summary(rows_of(listed), files), "list": listed}
    finally:
        probe.stop_daemon(sock)
        server.stop()

    (out / "attached-clients.json").write_text(json.dumps(info, indent=2, ensure_ascii=False) + "\n")
    print(f"hello : appVersion {info['hello']['appVersion']}")
    for moment in ("avec_A_ouvert", "apres_fermeture_de_A"):
        print(moment)
        for row in info[moment]["resume"]:
            print("  " + json.dumps(row, ensure_ascii=False))


if __name__ == "__main__":
    main()
