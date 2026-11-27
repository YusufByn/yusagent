#!/usr/bin/env python3
"""Contrôle du retour en arrière (ticket 43, §G.6 et §I.7) rejoué par ACP sur le binaire.

YA-18 (`docs/tickets/43-retour-arriere.md`, §G.6, §I.7, §L). Ce que Yusuf fera à la main dans
l'extension, rejoué ici au niveau du pont, sur le binaire donné : vrai moteur, vrai kernel, et pour
modèle le serveur de capture (`server.py`, scénario `ya43-control` : la réponse dépend du texte du
dernier prompt), aucun vrai modèle. Chaque cas a sa session, son démon et son dossier agent. Ce que
le modèle reçoit est lu dans les requêtes gardées par le serveur (ticket 43, §L : le fichier de
session porte des lignes que le modèle ne reçoit pas toujours).

- G1 (§G.6, 1 à 3) : trois messages, le deuxième pose `x = 1` dans le kernel ; retour avant le
  deuxième, renvoi modifié : la requête qui part repart de là, le texte fixe une fois avant le
  renvoi, rien des tours abandonnés ; une cellule suivante lit `x` : le kernel l'a gardé.
- G2 (§G.6, 4) : retour demandé pendant un tour : refusé (`-32603`, texte d'Unharnessed), le tour
  va au bout.
- G3 (§G.6, 5) : « Relancer » : un autre processus charge la conversation (`session/load`) ; les
  messages rejoués sont ceux de la nouvelle branche.
- I1 (§I.7, 1) : `/refine` puis modification du message suivant : la ligne de refine reste.
- I2 (§I.7, 2) : un autre client qui répond aux permissions est attaché : refus avec le texte
  validé ; il se détache : le retour passe.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya43_control.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si tous les cas sont conformes. macOS ; bibliothèque standard uniquement.
"""

import argparse
import importlib.util
import json
import os
import queue
import shutil
import socket as socketlib
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
META = "ai.primeintellect.prime-agent"
REWOUND = ("[conversation-rewound] The conversation went back to an earlier point; the turns "
           "after it are gone from your context. The Python kernel kept its state: variables, "
           "imports and background commands from those turns may still exist.")
WORKING = "The agent is working: stop it before going back."
OTHER_WINDOW = "This conversation is open in another window: close it there before going back."
TIMEOUT_S = 120


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = load("ya_capture_run", "run.py")
WORK = Path(os.environ.get("YA43_CONTROL_WORK", "/private/tmp/ya43-control"))


def text_of(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, dict):
        return content.get("text", "")
    if isinstance(content, list):
        return "".join(block.get("text", "") for block in content if isinstance(block, dict))
    return ""


def conversation(file: Path) -> list[str]:
    """La branche du fichier de session, comme le modèle la voit."""
    entries = [json.loads(line) for line in file.read_text().splitlines() if line.strip()]
    entries = [e for e in entries if e.get("type") != "session" and isinstance(e.get("id"), str)]
    by_id = {e["id"]: e for e in entries}
    path, current = [], entries[-1] if entries else None
    while current is not None:
        path.append(current)
        current = by_id.get(current.get("parentId"))
    shown = []
    for entry in reversed(path):
        if entry.get("type") == "message":
            message = entry.get("message") or {}
            shown.append(f"{message.get('role')}:{text_of(message.get('content'))}")
        elif entry.get("type") == "custom_message":
            shown.append(f"custom:{entry.get('customType')}:{text_of(entry.get('content'))}")
    return shown


def daemon_command(sock: Path, command: dict, keep: bool = False):
    """Une commande au démon ; avec `keep`, la connexion reste ouverte (renvoyée)."""
    conn = socketlib.socket(socketlib.AF_UNIX, socketlib.SOCK_STREAM)
    conn.connect(str(sock))
    frame = {"type": "command", "id": f"ctl-{time.monotonic_ns()}",
             "protocol": {"name": "prime-agent.daemon", "version": 7}, "command": command}
    conn.sendall((json.dumps(frame) + "\n").encode())
    reader = conn.makefile("r")
    conn.settimeout(TIMEOUT_S)
    while True:
        line = reader.readline()
        if not line:
            return (None, conn) if keep else None
        incoming = json.loads(line)
        if incoming.get("id") == frame["id"]:
            if keep:
                return incoming, conn
            conn.close()
            return incoming


class Bridge:
    def __init__(self, binary: Path, cache: Path, case: Path, agent: Path, sock: Path,
                 project: Path, extra: tuple[str, ...] = ()):
        case.mkdir(parents=True, exist_ok=True)
        env = run.base_env(cache / "home", agent, WORK / "tmp")
        self.lines: queue.Queue[str] = queue.Queue()
        self.frames: list[dict] = []
        self.err = (case / f"err-{len(list(case.glob('err-*')))}.log").open("w")
        self.proc = subprocess.Popen(
            [str(binary), "--mode", "acp", "--daemon-socket", str(sock), *extra],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err, env=env, cwd=project,
            text=True, bufsize=1)
        threading.Thread(target=lambda: [self.lines.put(line) for line in self.proc.stdout],
                         daemon=True).start()
        self.next_id = 0

    def send(self, method: str, params: dict) -> int:
        self.next_id += 1
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.next_id,
                                          "method": method, "params": params}) + "\n")
        self.proc.stdin.flush()
        return self.next_id

    def wait(self, pred, what: str) -> dict:
        for frame in self.frames:
            if pred(frame):
                return frame
        deadline = time.monotonic() + TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                frame = json.loads(self.lines.get(timeout=0.5))
            except (queue.Empty, json.JSONDecodeError):
                continue
            self.frames.append(frame)
            if pred(frame):
                return frame
        raise TimeoutError(what)

    def request(self, method: str, params: dict) -> dict:
        req = self.send(method, params)
        return self.wait(lambda f: f.get("id") == req and "method" not in f, f"réponse {method}")

    def open(self, project: Path) -> tuple[str, Path, str]:
        self.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        result = self.request("session/new", {"cwd": str(project), "mcpServers": []})["result"]
        saved = result["_meta"][META]["saved"]
        return result["sessionId"], Path(saved["path"]), saved["sessionId"]

    def prompt(self, sid: str, text: str) -> tuple[dict, dict]:
        """Un prompt : son `turnStart` et sa réponse."""
        before = len(self.frames)
        answer = self.request("session/prompt",
                              {"sessionId": sid, "prompt": [{"type": "text", "text": text}]})
        start = next((((f.get("params") or {}).get("update") or {}).get("_meta", {})
                      .get(META, {}).get("turnStart") for f in self.frames[before:]
                      if (((f.get("params") or {}).get("update") or {}).get("_meta", {})
                          .get(META, {}).get("turnStart") or {}).get("origin") == "prompt"), None)
        return start or {}, answer

    def rewind(self, sid: str, to: str) -> dict:
        return self.request("_yus/session/rewind", {"sessionId": sid, "to": to})

    def close(self) -> None:
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=15)
        except (subprocess.TimeoutExpired, BrokenPipeError):
            self.proc.kill()
        self.err.close()


def stop_daemon(sock: Path) -> None:
    subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {sock}"])
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and subprocess.run(
            ["pgrep", "-f", f"--daemon-socket {sock}"], capture_output=True).returncode == 0:
        time.sleep(0.2)


class Capture:
    """Le serveur de capture d'un cas (scénario `ya43-control`) et ses requêtes de la boucle."""

    def __init__(self, case: Path):
        case.mkdir(parents=True, exist_ok=True)
        self.dir = case / "cap"
        port_file = case / "port"
        self.proc = subprocess.Popen(
            [sys.executable, "-I", str(HERE / "server.py"), "--out", str(self.dir),
             "--scenario", "ya43-control", "--port-file", str(port_file)],
            stdout=subprocess.DEVNULL, stderr=(case / "server.log").open("w"))
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and not (
                port_file.exists() and port_file.read_text().strip()):
            time.sleep(0.05)
        self.port = int(port_file.read_text().strip())

    def requests(self) -> list[dict]:
        bodies = []
        for path in sorted(self.dir.glob("*.body.json")):
            try:
                body = json.loads(path.read_bytes())
            except (OSError, json.JSONDecodeError):
                continue
            if body.get("tools"):
                bodies.append(body)
        return bodies

    def stop(self) -> None:
        self.proc.terminate()
        self.proc.wait(timeout=10)


def last_user_text(body: dict) -> str:
    """Le texte du dernier message utilisateur d'une requête (blocs texte seulement)."""
    for message in reversed(body.get("messages") or []):
        if message.get("role") == "user":
            return text_of(message.get("content"))
    return ""


def case_dirs(name: str, out: Path) -> tuple[Path, Path, Capture]:
    agent = WORK / f"agent-{name}"
    shutil.rmtree(agent, ignore_errors=True)
    agent.mkdir(parents=True)
    capture = Capture(out / name)
    run.write_models_json(agent, "anthropic-messages", capture.port)
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS, **run.PERMISSIONS_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model"}) + "\n")
    sock = WORK / f"{name}.sock"
    sock.unlink(missing_ok=True)
    return agent, sock, capture


def g1_g3(binary, cache, out, project) -> dict:
    agent, sock, capture = case_dirs("g1", out)
    bridge = Bridge(binary, cache, out / "g1", agent, sock, project)
    checks = {}
    replayed = []
    loaded = None
    try:
        sid, file, file_id = bridge.open(project)
        bridge.prompt(sid, "premier")
        second, _ = bridge.prompt(sid, "deuxième : pose x")
        bridge.prompt(sid, "troisième")
        answer = bridge.rewind(sid, second.get("rewindTo"))
        checks["G1_retour_accepte"] = answer.get("result") == {}
        bridge.prompt(sid, "deuxième, modifié")
        _, last = bridge.prompt(sid, "x existe-t-il ?")
        requests = capture.requests()
        after = next((body for body in requests
                      if last_user_text(body).endswith("deuxième, modifié")), None)
        sent = json.dumps((after or {}).get("messages"), ensure_ascii=False)
        checks["G1_requete_apres_le_retour"] = after is not None
        checks["G1_texte_fixe_une_fois_dans_la_requete"] = sent.count(REWOUND) == 1
        checks["G1_texte_fixe_avant_le_renvoi"] = (
            REWOUND in sent and sent.index(REWOUND) < sent.index("deuxième, modifié"))
        checks["G1_rien_des_tours_abandonnes_dans_la_requete"] = not any(
            text in sent for text in ("pose x", "troisième", "X_SET"))
        read_back = [body for body in requests if "X_NOW" in json.dumps(body, ensure_ascii=False)]
        checks["G1_kernel_garde_x"] = any(
            "X_NOW 1" in json.dumps(body, ensure_ascii=False) for body in read_back)
        checks["G1_dernier_prompt_fini"] = (last.get("result") or {}).get("stopReason") == "end_turn"
    finally:
        bridge.close()
    # G3 : « Relancer » : un autre processus, lancé sur le fichier (`--resume`, comme l'extension,
    # ticket 28), charge la conversation (`session/load`).
    reloaded = Bridge(binary, cache, out / "g1", agent, sock, project, ("--resume", str(file)))
    try:
        reloaded.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        loaded = reloaded.request("session/load", {"sessionId": file_id, "cwd": str(project),
                                                    "mcpServers": []})
        replayed = [text_of(((f.get("params") or {}).get("update") or {}).get("content"))
                    for f in reloaded.frames
                    if ((f.get("params") or {}).get("update") or {}).get("sessionUpdate")
                    == "user_message_chunk"]
        checks["G3_chargement_accepte"] = "result" in loaded
        checks["G3_messages_rejoues"] = replayed == ["premier", "deuxième, modifié",
                                                     "x existe-t-il ?"]
    finally:
        reloaded.close()
        stop_daemon(sock)
        capture.stop()
    return {"checks": checks, "conversation": conversation(file), "replayed": replayed,
            "load_answer": loaded}


def g2(binary, cache, out, project) -> dict:
    agent, sock, capture = case_dirs("g2", out)
    bridge = Bridge(binary, cache, out / "g2", agent, sock, project)
    checks = {}
    try:
        sid, _, _ = bridge.open(project)
        first, _ = bridge.prompt(sid, "premier")
        req = bridge.send("session/prompt", {"sessionId": sid,
                                             "prompt": [{"type": "text", "text": "lent"}]})
        deadline = time.monotonic() + TIMEOUT_S
        while not (project / "g2-started").exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        refused = bridge.rewind(sid, first.get("rewindTo"))
        error = refused.get("error") or {}
        checks["G2_refus_pendant_le_tour"] = (error.get("code") == -32603
                                             and (error.get("data") or {}).get("details") == WORKING)
        answer = bridge.wait(lambda f: f.get("id") == req and "method" not in f, "fin du tour")
        checks["G2_tour_au_bout"] = (answer.get("result") or {}).get("stopReason") == "end_turn"
    finally:
        bridge.close()
        stop_daemon(sock)
        capture.stop()
        (project / "g2-started").unlink(missing_ok=True)
    return {"checks": checks}


def i1(binary, cache, out, project) -> dict:
    agent, sock, capture = case_dirs("i1", out)
    bridge = Bridge(binary, cache, out / "i1", agent, sock, project)
    checks = {}
    try:
        sid, file, _ = bridge.open(project)
        bridge.prompt(sid, "premier")
        _, refine = bridge.prompt(sid, "/refine")
        before = conversation(file)
        refine_rows = [row for row in before if row.startswith("custom:")]
        checks["I1_refine_ecrit_une_ligne"] = bool(refine_rows)
        second, _ = bridge.prompt(sid, "deuxième")
        answer = bridge.rewind(sid, second.get("rewindTo"))
        checks["I1_retour_accepte"] = answer.get("result") == {}
        bridge.prompt(sid, "deuxième, modifié")
        after = conversation(file)
        checks["I1_ligne_de_refine_gardee"] = bool(refine_rows) and all(
            row in after for row in refine_rows)
    finally:
        bridge.close()
        stop_daemon(sock)
        capture.stop()
    return {"checks": checks, "refine_answer": refine, "rows_before": before,
            "conversation": after}


def i2(binary, cache, out, project) -> dict:
    agent, sock, capture = case_dirs("i2", out)
    bridge = Bridge(binary, cache, out / "i2", agent, sock, project)
    checks = {}
    try:
        sid, _, _ = bridge.open(project)
        bridge.prompt(sid, "premier")
        second, _ = bridge.prompt(sid, "deuxième")
        listed = daemon_command(sock, {"type": "list"})
        daemon_sid = ((listed or {}).get("data") or {}).get("sessions", [{}])[0].get("sessionId")
        attached, other = daemon_command(sock, {"type": "attach", "activeSessionId": daemon_sid,
                                                "yaPermissionAsker": True}, keep=True)
        checks["I2_autre_client_attache"] = bool((attached or {}).get("success"))
        refused = bridge.rewind(sid, second.get("rewindTo"))
        error = refused.get("error") or {}
        checks["I2_refus_autre_fenetre"] = (error.get("code") == -32603
                                           and (error.get("data") or {}).get("details")
                                           == OTHER_WINDOW)
        other.close()
        time.sleep(1)
        accepted = bridge.rewind(sid, second.get("rewindTo"))
        checks["I2_retour_apres_detachement"] = accepted.get("result") == {}
    finally:
        bridge.close()
        stop_daemon(sock)
        capture.stop()
    return {"checks": checks, "refused": refused, "accepted": accepted}


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
    binary = args.binary.resolve()
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}
                             ).stdout.strip()
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    report = {"version": version, "cases": {}}
    for name, case in (("G1-G3", g1_g3), ("G2", g2), ("I1", i1), ("I2", i2)):
        try:
            report["cases"][name] = case(binary, cache, out, project)
        except Exception as error:  # noqa: BLE001 - un cas en erreur est un cas non conforme
            report["cases"][name] = {"checks": {"erreur": False}, "error": repr(error)}
    checks = {name: ok for case in report["cases"].values() for name, ok in case["checks"].items()}
    report["conforming"] = all(checks.values())
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    rows = [f"Binaire : `{version}`", "", "| Contrôle | Résultat |", "|---|---|"]
    rows += [f"| {name} | {'oui' if ok else 'NON'} |" for name, ok in checks.items()]
    rows += ["", "CONFORME" if report["conforming"] else "NON CONFORME"]
    text = "\n".join(rows) + "\n"
    (out / "report.md").write_text(text)
    print(text)
    sys.exit(0 if report["conforming"] else 1)


if __name__ == "__main__":
    main()
