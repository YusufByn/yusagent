#!/usr/bin/env python3
"""Contrôle du ticket 29 : le pont ACP face à un démon lancé par un autre build.

Sur les vrais binaires, chaque cas avec son démon isolé, son serveur de capture (modèle factice),
`sandbox-exec`, HOME et venv du banc (fonctions de `run.py` et `resume_probe.py`, sans les
modifier). Trois builds :

  - B : `--binary`, le binaire de la branche (ponts testés) ;
  - O : `--other-binary`, un build d'avant le ticket (sans l'action `holds`), par exemple `main` ;
  - B' : une copie du paquet de B dont le `package.json` annonce une autre version (même code,
    donc l'action `holds`, mais un autre `appVersion`).

Cas :
  1. démon O sans session : le pont B le remplace ;
  2. démon O, une conversation fermée au repos (P1a) : refus, avec les deux versions et
     `yusagent shutdown`, démon inchangé ;
  3. démon B', une conversation fermée au repos : remplacé, et la conversation reprend sur le
     nouveau démon (`--resume`, `session/load`) ;
  4. démon B', une fenêtre B' ouverte (client attaché) : refus après l'attente, la fenêtre B'
     répond toujours ;
  5. démon B', une conversation fermée au repos avec un `bash()` en arrière-plan (40 s, scénario
     `ya29-bash-long`) : refus (`holds`) ;
  6. démon B, même build : gardé.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I \\
      bench/tools/capture/daemon_build_check.py --binary <yusagent branche> \\
      --other-binary <yusagent main> --out <dossier neuf>

Verdict : CONFORME si les six cas le sont (code de sortie 0), NON CONFORME sinon (1). Rapport :
`daemon-build-check.json`. Aucun modèle réel. macOS (sandbox-exec) requis ; bibliothèque standard
uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import queue
import shutil
import socket as so
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
os.environ.setdefault("YA_RESUME_PROBE_WORK", "/private/tmp/ya-build-check")
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


def version_of(binary: Path, cache: Path) -> str:
    return subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                          env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()


def daemon_request(path: Path, command: dict | None) -> tuple[dict, dict | None]:
    """Hello du démon et, si `command`, sa réponse (client brut, sans `attach`)."""
    sock = so.socket(so.AF_UNIX)
    sock.settimeout(30)
    sock.connect(str(path))
    reader = sock.makefile("r", encoding="utf-8")
    hello = json.loads(reader.readline())
    if command is None:
        sock.close()
        return hello, None
    envelope = {"type": "command", "id": "check", "protocol": hello.get("protocol") or {},
                "clientId": "build-check", "command": command}
    sock.sendall((json.dumps(envelope) + "\n").encode())
    for line in reader:
        frame = json.loads(line)
        if frame.get("type") == "response" and frame.get("id") == "check":
            sock.close()
            return hello, frame
    raise RuntimeError(f"pas de réponse à {command}")


def daemon_id(path: Path) -> dict | None:
    """`appVersion` et pid du démon qui répond sur `path`, `None` s'il n'y en a pas."""
    try:
        hello, _ = daemon_request(path, None)
    except OSError:
        return None
    return {"appVersion": hello.get("appVersion"), "pid": hello.get("supervisorPid")}


def wait_daemon(path: Path, seconds: float = 30) -> dict | None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        found = daemon_id(path)
        if found:
            return found
        time.sleep(0.2)
    return None


def rows(path: Path) -> list[dict]:
    _, listed = daemon_request(path, {"type": "list"})
    return ((listed or {}).get("data") or {}).get("sessions") or []


def wait_quiet(path: Path, seconds: float = 60) -> bool:
    """Attend qu'aucune session ne travaille (tour, outils) ni n'ait de client."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        live = [r for r in rows(path) if r.get("activeSessionId")]
        if all(not r.get("isSessionActive") and not r.get("isRunningTools")
               and not r.get("attachedClients") for r in live):
            return True
        time.sleep(0.3)
    return False


class Case:
    """Un cas : dossier, agent, serveur de capture, socket."""

    def __init__(self, out: Path, cache: Path, name: str, scenario: str = "basic"):
        self.name = name
        self.dir = out / name
        self.dir.mkdir(parents=True)
        self.work = WORK / name
        (self.work / "tmp").mkdir(parents=True)
        self.project = self.work / "project"
        run.make_project(self.project)
        self.agent, self.env = probe.setup_in(self.work, cache, name)
        self.server = run.CaptureServer(self.dir / "cap", scenario, [])
        run.write_models_json(self.agent, "anthropic-messages", self.server.port)
        self.socket = self.work / "d.sock"
        self.bridges: list = []

    def bridge(self, binary: Path, tag: str, extra: list[str] | None = None):
        bridge = probe.Bridge(binary, self.socket, self.env, self.project, self.dir, tag, extra)
        self.bridges.append(bridge)
        return bridge

    def start(self, binary: Path, tag: str, extra: list[str] | None = None,
              seconds: float = 60):
        """Lance un pont et `initialize`. Renvoie (pont, None) s'il répond, (pont, stderr) s'il
        sort avant (refus au démarrage)."""
        bridge = self.bridge(binary, tag, extra)
        bridge.write({"jsonrpc": "2.0", "id": 0, "method": "initialize",
                      "params": {"protocolVersion": 1, "clientCapabilities": {}}})
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(bridge.lines.get(timeout=0.3))
                bridge.transcript.append({"dir": "agent->client", "at": time.time(),
                                          "msg": incoming})
                if incoming.get("id") == 0 and "method" not in incoming:
                    return bridge, None
            except (queue.Empty, json.JSONDecodeError):
                pass
            if bridge.proc.poll() is not None:
                time.sleep(0.2)
                bridge.err.flush()
                return bridge, (self.dir / f"err-{tag}.log").read_text()
        return bridge, "pas de réponse à initialize"

    def daemon(self, binary: Path) -> None:
        """Démarre un démon de `binary` sur le socket du cas."""
        subprocess.Popen(["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode",
                          "daemon", "--daemon-socket", str(self.socket)], env=self.env,
                         cwd=self.project, stdout=subprocess.DEVNULL,
                         stderr=(self.dir / "err-daemon.log").open("w"), start_new_session=True)

    def conversation(self, binary: Path, tag: str, keep_open: bool = False):
        """Une conversation avec un tour ; fermée (worker sans client) sauf `keep_open`."""
        bridge, refused = self.start(binary, tag)
        if refused:
            raise RuntimeError(f"{self.name} : le pont {tag} a refusé : {refused}")
        new = bridge.request("session/new", {"cwd": str(self.project), "mcpServers": []}) or {}
        result = new.get("result") or {}
        sid = result.get("sessionId")
        saved = ((result.get("_meta") or {}).get(META) or {}).get("saved") or {}
        bridge.prompt(sid, "un")
        if not keep_open:
            bridge.close()
        return bridge, sid, saved

    def stop(self) -> None:
        for bridge in self.bridges:
            if bridge.proc.poll() is None:
                try:
                    bridge.close()
                except Exception:  # noqa: BLE001 - nettoyage
                    bridge.proc.kill()
        probe.stop_daemon(self.socket)
        self.server.stop()


def refusal_ok(text: str | None, versions: tuple[str, str], *words: str) -> bool:
    return bool(text) and all(v in text for v in versions) and "yusagent shutdown" in text \
        and all(w in text for w in words)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path, help="le binaire de la branche (B)")
    parser.add_argument("--other-binary", required=True, type=Path,
                        help="un build d'avant le ticket (O), sans l'action holds")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, other = args.binary.resolve(), args.other_binary.resolve()
    out, cache = args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(WORK, ignore_errors=True)
    WORK.mkdir(parents=True)

    # B' : le paquet de B, autre version annoncée.
    copy = WORK / "b-prime"
    shutil.copytree(binary.parent, copy, symlinks=True)
    manifest = json.loads((copy / "package.json").read_text())
    manifest["version"] = manifest["version"] + "-copie"
    (copy / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    prime = copy / binary.name

    v_b, v_o, v_p = version_of(binary, cache), version_of(other, cache), version_of(prime, cache)
    print(f"B  : {binary} ({v_b})\nO  : {other} ({v_o})\nB' : {prime} ({v_p})")
    if len({v_b, v_o, v_p}) != 3:
        sys.exit("les trois builds doivent annoncer trois versions différentes")
    run.prepare(binary, cache, v_b)
    run.prepare(other, cache, v_o)
    info: dict = {"B": v_b, "O": v_o, "B'": v_p, "cas": {}}

    def record(name: str, ok: bool, **facts) -> None:
        info["cas"][name] = {"ok": ok, **facts}
        print(f"{name} : {'OK' if ok else 'ÉCHEC'} {json.dumps(facts, ensure_ascii=False)[:600]}")

    # 1. Démon O sans session : remplacé.
    case = Case(out, cache, "1-o-sans-session")
    try:
        case.daemon(other)
        before = wait_daemon(case.socket)
        bridge, refused = case.start(binary, "B")
        new = None if refused else bridge.request(
            "session/new", {"cwd": str(case.project), "mcpServers": []})
        after = daemon_id(case.socket)
        record(case.name, bool(before and after and not refused and new and "result" in new
                               and after["appVersion"] == v_b and after["pid"] != before["pid"]),
               avant=before, apres=after, refus=refused)
    finally:
        case.stop()

    # 2. Démon O, conversation fermée au repos : refus (P1a).
    case = Case(out, cache, "2-o-p1a")
    try:
        case.daemon(other)
        wait_daemon(case.socket)
        case.conversation(other, "P")
        time.sleep(4)
        before = daemon_id(case.socket)
        _, refused = case.start(binary, "B")
        after = daemon_id(case.socket)
        record(case.name, refusal_ok(refused, (v_o, v_b), "cannot tell") and before == after,
               avant=before, apres=after, refus=refused)
    finally:
        case.stop()

    # 3. Démon B', conversation fermée au repos : remplacé, la conversation reprend.
    case = Case(out, cache, "3-bprime-repos")
    try:
        _, _, saved = case.conversation(prime, "P")
        time.sleep(4)
        quiet = wait_quiet(case.socket)
        before = daemon_id(case.socket)
        bridge, refused = case.start(binary, "B", ["--resume", saved.get("path", "")])
        load = None if refused else bridge.request(
            "session/load", {"sessionId": saved.get("sessionId"), "cwd": str(case.project),
                             "mcpServers": []})
        after = daemon_id(case.socket)
        record(case.name, bool(quiet and before and after and not refused and load
                               and "result" in load and after["appVersion"] == v_b
                               and after["pid"] != before["pid"]),
               avant=before, apres=after, refus=refused,
               load=(load or {}).get("error") or ("ok" if load else None))
    finally:
        case.stop()

    # 4. Démon B', une fenêtre B' ouverte : refus, la fenêtre répond toujours.
    case = Case(out, cache, "4-bprime-attache")
    try:
        holder, sid, _ = case.conversation(prime, "A", keep_open=True)
        before = daemon_id(case.socket)
        started = time.monotonic()
        _, refused = case.start(binary, "B")
        waited = round(time.monotonic() - started, 1)
        answer = holder.prompt(sid, "deux")
        after = daemon_id(case.socket)
        record(case.name, refusal_ok(refused, (v_p, v_b), "attached") and before == after
               and bool(answer) and "code" not in (answer or {}),
               avant=before, apres=after, refus=refused, attente_s=waited,
               reponse_A="ok" if answer and "code" not in answer else answer)
    finally:
        case.stop()

    # 5. Démon B', bash() en arrière-plan : refus (holds).
    case = Case(out, cache, "5-bprime-bash", scenario="ya29-bash-long")
    try:
        case.conversation(prime, "P")
        # Le bash() de 40 s tourne : pas de tour, pas de client, mais du travail (holds).
        quiet = wait_quiet(case.socket)
        before = daemon_id(case.socket)
        _, refused = case.start(binary, "B")
        after = daemon_id(case.socket)
        record(case.name, quiet and refusal_ok(refused, (v_p, v_b), "holds work")
               and before == after, avant=before, apres=after, refus=refused)
    finally:
        case.stop()

    # 6. Démon B, même build : gardé.
    case = Case(out, cache, "6-meme-build")
    try:
        case.conversation(binary, "P")
        time.sleep(4)
        before = daemon_id(case.socket)
        bridge, refused = case.start(binary, "B")
        after = daemon_id(case.socket)
        record(case.name, bool(before and not refused and before == after),
               avant=before, apres=after, refus=refused)
    finally:
        case.stop()

    conforme = len(info["cas"]) == 6 and all(c["ok"] for c in info["cas"].values())
    info["conforme"] = conforme
    (out / "daemon-build-check.json").write_text(json.dumps(info, indent=2, ensure_ascii=False)
                                                 + "\n")
    print("CONFORME" if conforme else "NON CONFORME")
    sys.exit(0 if conforme else 1)


if __name__ == "__main__":
    main()
