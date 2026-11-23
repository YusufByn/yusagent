#!/usr/bin/env python3
"""Contrôle (YA-10, ticket 28, contrôle manuel de X1) : une conversation supprimée ne laisse aucun
fichier à son nom, sauf son journal d'usage (C4).

Sur le vrai binaire, démon isolé, serveur de capture (modèle factice), `sandbox-exec`, HOME et venv
du banc (fonctions de `run.py` et `resume_probe.py`, sans les modifier). Deux suppressions :

  1. corbeille de l'historique : une conversation avec un tour, fermée, puis
     `_unharnessed/sessions/delete` depuis un autre pont (C4) ;
  2. conversation neuve vide : son pont finit (fin de stdin, O1).

Après chacune : dans le dossier des sessions, plus aucun `<id>.*` hors `<id>.usage.ndjson` (fichier,
`<id>.window-cache.json`, `<id>.info-cache.json`, temporaires), plus de `session-artifacts/<id>`.
Avant chacune, le relevé montre les fichiers présents.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I \\
      bench/tools/capture/delete_files_check.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si CONFORME, 1 sinon. Aucun modèle réel. macOS (sandbox-exec) requis ;
bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ.setdefault("YA_RESUME_PROBE_WORK", "/private/tmp/ya-delete-files-check")
META = "ai.primeintellect.prime-agent"


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


def saved_of(response: dict | None) -> dict:
    return (((response or {}).get("result") or {}).get("_meta") or {}).get(META, {}).get("saved") or {}


def named_after(path: Path) -> list[str]:
    """Les fichiers `<id>.*` à côté du fichier de session, `<id>` remplacé."""
    stem = path.stem
    return sorted(f.name.replace(stem, "<id>", 1) for f in path.parent.iterdir()
                  if f.name.startswith(f"{stem}."))


def left_after(path: Path, agent: Path, sid: str) -> list[str]:
    left = [name for name in named_after(path) if name != "<id>.usage.ndjson"]
    if (agent / "session-artifacts" / sid).exists():
        left.append(f"session-artifacts/{sid}")
    return left


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
    print(f"binaire : {version}")
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    agent, env = probe.setup(cache, "del")
    server = probe.serve(out, agent)
    socket = WORK / "del.sock"
    info: dict = {"binary": str(binary), "version": version, "cas": {}}

    def bridge(tag: str):
        b = probe.Bridge(binary, socket, env, project, out, tag)
        b.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        response = b.request("session/new", {"cwd": str(project), "mcpServers": []})
        return b, response["result"]["sessionId"], saved_of(response)

    try:
        # 1. Corbeille de l'historique.
        a, asid, sa = bridge("A")
        a.prompt(asid, "un")
        a.close()
        time.sleep(2)
        path_a = Path(sa["path"])
        before = named_after(path_a)
        b, _, _ = bridge("B")
        deleted = b.request("_unharnessed/sessions/delete", {"sessionId": sa["sessionId"]})
        time.sleep(1)
        info["cas"]["1. corbeille"] = {
            "avant": before,
            "suppression": (deleted or {}).get("result", (deleted or {}).get("error")),
            "reste": left_after(path_a, agent, sa["sessionId"]),
        }
        b.close()
        # 2. Conversation neuve vide, son pont finit.
        c, _, sc = bridge("C")
        time.sleep(1)
        path_c = Path(sc["path"])
        before = named_after(path_c)
        c.close()
        time.sleep(2)
        info["cas"]["2. vide, fin du pont"] = {
            "avant": before,
            "reste": left_after(path_c, agent, sc["sessionId"]),
        }
    finally:
        probe.stop_daemon(socket)
        server.stop()

    for case, found in info["cas"].items():
        print(f"{case} : avant {found['avant']} ; reste {found['reste']}"
              + (f" ; suppression {found['suppression']}" if "suppression" in found else ""))
    conforme = (info["cas"]["1. corbeille"]["suppression"] == {}
                and all(not found["reste"] for found in info["cas"].values()))
    info["conforme"] = conforme
    (out / "delete-files-check.json").write_text(json.dumps(info, indent=2, ensure_ascii=False) + "\n")
    print("CONFORME" if conforme else "NON CONFORME")
    sys.exit(0 if conforme else 1)


if __name__ == "__main__":
    main()
