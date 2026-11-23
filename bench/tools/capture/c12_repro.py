#!/usr/bin/env python3
"""Contrôle C12 (YA-10, ticket 28, défaut 1 du contrôle de X1) : une conversation tenue par une
fenêtre est-elle refusée à une seconde fenêtre ?

Sur le vrai binaire, démon isolé, serveur de capture (modèle factice), `sandbox-exec`, HOME et venv
du banc (fonctions de `run.py` et `resume_probe.py`, sans les modifier). La fenêtre A tient la
session, puis la fenêtre B lance `--resume <fichier>` et `session/load` :

  1. A charge la conversation (`--resume` + `session/load`), sans prompt ;
  2. A la charge, puis envoie un prompt ;
  3. A la crée (`session/new`), puis envoie un prompt (le cas e2e) ;
  4. A la crée, sans prompt.

Avec `--daemon-binary`, le démon est lancé d'abord depuis ce binaire (un démon plus ancien que les
ponts, comme après un changement de binaire sans `yusagent shutdown`) ; les ponts restent ceux de
`--binary`. Depuis le ticket 29, le premier pont remplace ce démon, qui n'a pas de session : les
quatre cas tournent sur un démon de `--binary`.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I \\
      bench/tools/capture/c12_repro.py --binary <yusagent> [--daemon-binary <yusagent ancien>] \\
      --out <dossier neuf>

Verdict : CONFORME si B est refusé dans les quatre cas ; avec `--daemon-binary`, le démon doit en
plus avoir été remplacé par un démon de `--binary` (ticket 29 : `appVersion` du hello). Code de sortie 0 si
CONFORME, 1 sinon. Aucun modèle réel. macOS (sandbox-exec) requis ; bibliothèque standard
uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import socket as socket_module
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ.setdefault("YA_RESUME_PROBE_WORK", "/private/tmp/ya-c12-repro")
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


def refusal(response: dict | None) -> str | None:
    """Le texte du refus, `None` si la réponse est un succès."""
    if response is None:
        return "pas de réponse"
    if "error" not in response:
        return None
    error = response["error"]
    return str((error.get("data") or {}).get("details") or error.get("message"))


def hello_version(path: Path) -> str | None:
    """L'`appVersion` du hello du démon (ticket 29)."""
    sock = socket_module.socket(socket_module.AF_UNIX)
    sock.settimeout(10)
    try:
        sock.connect(str(path))
        return json.loads(sock.makefile("r", encoding="utf-8").readline()).get("appVersion")
    except OSError:
        return None
    finally:
        sock.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path, help="le binaire des ponts")
    parser.add_argument("--daemon-binary", type=Path, help="le binaire du démon, lancé d'abord")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    old = args.daemon_binary.resolve() if args.daemon_binary else None
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)

    def version_of(path: Path) -> str:
        return subprocess.run([str(path), "--version"], capture_output=True, text=True,
                              env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()

    version = version_of(binary)
    print(f"ponts : {version}")
    print(f"démon : {version_of(old) if old else version}")
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    agent, env = probe.setup(cache, "c12")
    server = probe.serve(out, agent)
    socket = WORK / "c12.sock"
    info: dict = {"ponts": version, "demon": version_of(old) if old else version, "cas": {}}
    if old:
        subprocess.Popen(["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(old), "--mode", "daemon",
                          "--daemon-socket", str(socket)], env=env, cwd=project,
                         stdout=subprocess.DEVNULL, stderr=(out / "err-daemon.log").open("w"),
                         start_new_session=True)
        time.sleep(3)

    def load_bridge(tag: str, path: str, sid: str):
        bridge = probe.Bridge(binary, socket, env, project, out, tag, ["--resume", path])
        bridge.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        return bridge, bridge.request("session/load",
                                      {"sessionId": sid, "cwd": str(project), "mcpServers": []})

    def new_bridge(tag: str):
        bridge = probe.Bridge(binary, socket, env, project, out, tag)
        bridge.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        response = bridge.request("session/new", {"cwd": str(project), "mcpServers": []})
        return bridge, response["result"]["sessionId"], saved_of(response)

    def record(case: str, holder_answer: dict | None, other_answer: dict | None) -> None:
        info["cas"][case] = {"A": refusal(holder_answer) or "accepté",
                             "B": refusal(other_answer) or "ACCEPTÉ"}
        print(f"{case} : A {info['cas'][case]['A']} ; B {info['cas'][case]['B']}")

    try:
        # Une conversation avec un tour, fermée (son worker reste dans le démon).
        p, psid, s = new_bridge("P")
        info["demon_apres_P"] = hello_version(socket)
        print(f"démon après le premier pont : {info['demon_apres_P']}")
        p.prompt(psid, "un")
        p.close()
        path, fid = s["path"], s["sessionId"]

        a, ra = load_bridge("1A", path, fid)
        b, rb = load_bridge("1B", path, fid)
        record("1. A charge, sans prompt", ra, rb)
        b.close()
        a.close()
        time.sleep(4)

        a, ra = load_bridge("2A", path, fid)
        a.prompt(fid, "deux")
        b, rb = load_bridge("2B", path, fid)
        record("2. A charge puis prompt", ra, rb)
        b.close()
        a.close()
        time.sleep(4)

        a, asid, sa = new_bridge("3A")
        a.prompt(asid, "trois")
        b, rb = load_bridge("3B", sa["path"], sa["sessionId"])
        record("3. A crée puis prompt", {"result": {}}, rb)
        b.close()
        a.close()
        time.sleep(4)

        a, asid, sa = new_bridge("4A")
        b, rb = load_bridge("4B", sa["path"], sa["sessionId"])
        record("4. A crée, sans prompt", {"result": {}}, rb)
        b.close()
        a.close()
    finally:
        probe.stop_daemon(socket)
        server.stop()

    def ok(case: dict) -> bool:
        return case["B"] != "ACCEPTÉ"

    replaced = not old or info.get("demon_apres_P") == version
    conforme = replaced and len(info["cas"]) == 4 and all(ok(case) for case in info["cas"].values())
    info["conforme"] = conforme
    (out / "c12-repro.json").write_text(json.dumps(info, indent=2, ensure_ascii=False) + "\n")
    print("CONFORME" if conforme else "NON CONFORME")
    sys.exit(0 if conforme else 1)


if __name__ == "__main__":
    main()
