#!/usr/bin/env python3
"""Contrôle avant X1 (YA-10, ticket 28, D2) : la suppression accepte-t-elle une session sans message ?

Sur le vrai binaire, démon isolé, serveur de capture (modèle factice), `sandbox-exec`, HOME et venv
du banc (fonctions de `run.py` et `resume_probe.py`, sans les modifier) :

  1. pont A : `session/new`, aucun prompt (la conversation vide de l'extension) ;
  2. pont B : `session/new` (la conversation qu'on vient d'ouvrir) ;
  3. fin de stdin de A (l'extension arrête son pont en la quittant) ;
  4. B : `session/list` (A, brouillon, ne doit pas y être), puis
     `_yus/sessions/delete { sessionId: <saved.sessionId de A> }` ;
  5. relevé : réponse, fichier de A, dossier `session-artifacts/<id>`, workers du démon.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/empty_delete_probe.py --binary <yusagent> --out <dossier neuf>

Rapport : `<sortie>/empty-delete-probe.json`, résumé sur la sortie standard. Code de sortie 0 si la
suppression répond `{}`, que le fichier est retiré et qu'aucun worker de A ne reste ; 1 sinon.
Aucun modèle réel. macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
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
os.environ.setdefault("YA_RESUME_PROBE_WORK", "/private/tmp/ya-empty-delete-probe")


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
META = "ai.primeintellect.prime-agent"


def saved_of(response: dict | None) -> dict:
    return (((response or {}).get("result") or {}).get("_meta") or {}).get(META, {}).get("saved") or {}


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
    case = out / "vide"
    case.mkdir()
    agent, env = probe.setup(cache, "vide")
    server = probe.serve(case, agent)
    socket = WORK / "vide.sock"
    info: dict = {"binary": str(binary), "version": version}
    try:
        a = probe.Bridge(binary, socket, env, project, case, "A")
        a.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new_a = a.request("session/new", {"cwd": str(project), "mcpServers": []})
        saved_a = saved_of(new_a)
        info["A_saved"] = saved_a
        b = probe.Bridge(binary, socket, env, project, case, "B")
        b.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new_b = b.request("session/new", {"cwd": str(project), "mcpServers": []})
        info["B_saved"] = saved_of(new_b)
        time.sleep(2)
        path_a = Path(saved_a.get("path", "/nonexistent"))
        info["A_fichier_avant"] = path_a.exists()
        info["arbre_avant"] = probe.daemon_tree(socket, {a.proc.pid, b.proc.pid})
        a.close()
        listed = b.request("session/list", {"cwd": str(project)})
        info["liste_ids"] = [row.get("sessionId") for row in
                             ((listed or {}).get("result") or {}).get("sessions", [])]
        started = time.monotonic()
        deleted = b.request("_yus/sessions/delete", {"sessionId": saved_a.get("sessionId")})
        info["suppression_ms"] = round((time.monotonic() - started) * 1000)
        info["suppression"] = {k: v for k, v in (deleted or {}).items() if k in ("result", "error")}
        time.sleep(1)
        info["A_fichier_apres"] = path_a.exists()
        artifacts = agent / "session-artifacts" / str(saved_a.get("sessionId"))
        info["A_artifacts_apres"] = artifacts.exists()
        info["arbre_apres"] = probe.daemon_tree(socket, {b.proc.pid})
        b.close()
    finally:
        probe.stop_daemon(socket)
        server.stop()
    ok = (info.get("suppression", {}).get("result") == {} and info.get("A_fichier_avant") is True
          and info.get("A_fichier_apres") is False
          and len(info["arbre_apres"]["worker"]) == len(info["arbre_avant"]["worker"]) - 1)
    info["conforme"] = ok
    (out / "empty-delete-probe.json").write_text(json.dumps(info, indent=2, ensure_ascii=False) + "\n")
    for key in ("A_saved", "A_fichier_avant", "liste_ids", "arbre_avant", "suppression",
                "suppression_ms", "A_fichier_apres", "A_artifacts_apres", "arbre_apres"):
        print(f"{key} : {json.dumps(info.get(key), ensure_ascii=False)}")
    print("CONFORME" if ok else "NON CONFORME")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
