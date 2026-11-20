#!/usr/bin/env python3
"""Démo ACP des permissions (YA-11, ticket 20, PR 2) : autoriser puis refuser un appel.

Lance le binaire en mode ACP sur un démon isolé (`--daemon-socket` dans un dossier temporaire),
avec le provider factice (`PRIME_AGENT_FAUX_SCRIPT`, `"engine": "faux"`) : aucun appel de modèle.
Réglages globaux vides dans un dossier agent temporaire : mode « manual » (le défaut). Deux tours
scriptés, chacun une cellule `python_repl` qui crée un fichier dans le projet temporaire. À chaque
`session/request_permission`, le script affiche la cellule et demande au terminal : `a`
(autoriser une fois) ou `r` (refuser une fois). À la fin : fichiers créés, réponses des tours,
entrées `ya-permission` du fichier de session.

HOME est le dossier préparé par le banc de capture (`~/.cache/yusagent-capture/home`, venv du
kernel) : rien n'est écrit dans `~/.yusagent`. Si le venv du kernel n'y est pas encore, le binaire
le crée au premier tour (réseau).

Le dossier de travail est pris sous son chemin canonique : sous un chemin qui passe par un lien
symbolique (`/var/folders` sur macOS), le worker n'écrit rien dans le fichier de session d'une
session neuve (défaut du bail, `docs/tickets/21-bail-chemin-symbolique.md`).

Vérifications à la fin (code de sortie 1 si l'une échoue) : un fichier créé seulement pour les
appels autorisés ; des messages dans le fichier de session ; une entrée `ya-permission` par
réponse, dans l'ordre (`allow`, `deny`) ; aucune ligne `pa-daemon:` dans le journal du worker.

Usage :
    python3 -I bench/tools/ya_permissions_acp_demo.py --binary <dossier>/yusagent
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

TIMEOUT_S = 300
CELLS = [
    ("autorise.txt", "call-autorise"),
    ("refuse.txt", "call-refuse"),
]


def faux_script() -> dict:
    responses = []
    for name, call_id in CELLS:
        responses.append({"content": [{
            "type": "toolCall", "id": call_id, "name": "python_repl",
            "arguments": {"code": f"open({name!r}, 'w').write('ok')\nprint('écrit : {name}')"},
        }]})
        responses.append({"text": f"Tour terminé après la cellule qui crée {name}."})
    return {"engine": "faux", "responses": responses}


class Acp:
    def __init__(self, binary: Path, env: dict[str, str], cwd: Path, socket: Path, log: Path):
        self.socket = socket
        self.err = log.open("w")
        self.proc = subprocess.Popen(
            [str(binary), "--mode", "acp", "--daemon-socket", str(socket)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err, env=env, cwd=cwd,
            text=True, bufsize=1)
        self.lines: queue.Queue[str] = queue.Queue()
        threading.Thread(target=lambda: [self.lines.put(line) for line in self.proc.stdout],
                         daemon=True).start()
        self.next_id = 0

    def send(self, message: dict) -> None:
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def request(self, method: str, params: dict) -> int:
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        return self.next_id

    def frames(self):
        deadline = time.monotonic() + TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                line = self.lines.get(timeout=1)
            except queue.Empty:
                if self.proc.poll() is not None:
                    sys.exit(f"le binaire s'est arrêté (code {self.proc.returncode})")
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue
        sys.exit("délai dépassé")

    def wait(self, req_id: int) -> dict:
        for frame in self.frames():
            if frame.get("id") == req_id and "method" not in frame:
                return frame
        raise AssertionError("unreachable")

    def close(self, binary: Path) -> None:
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except (subprocess.TimeoutExpired, BrokenPipeError):
            self.proc.kill()
        self.err.close()
        # Le superviseur de ce socket arrête ses workers et leurs kernels.
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {self.socket}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and subprocess.run(
                ["pgrep", "-f", str(self.socket)], capture_output=True).returncode == 0:
            time.sleep(0.2)


def ask(cell: str) -> str:
    print("\n--- session/request_permission ---")
    print(cell or "(cellule inconnue)")
    while True:
        answer = input("autoriser (a) ou refuser (r) ? ").strip().lower()
        if answer in ("a", "r"):
            return "allow-once" if answer == "a" else "reject-once"


def turn(acp: Acp, session_id: str, text: str) -> list[str]:
    prompt = acp.request("session/prompt", {
        "sessionId": session_id, "prompt": [{"type": "text", "text": text}]})
    cells: dict[str, str] = {}
    answers: list[str] = []
    for frame in acp.frames():
        if frame.get("id") == prompt and "method" not in frame:
            print(f"réponse du tour : {json.dumps(frame.get('result') or frame.get('error'))}")
            return answers
        update = (frame.get("params") or {}).get("update") or {}
        kind = update.get("sessionUpdate")
        if kind == "tool_call":
            cells[update.get("toolCallId", "")] = (update.get("rawInput") or {}).get("code", "")
            print(f"tool_call {update.get('toolCallId')} : {update.get('status')}")
        elif kind == "tool_call_update" and update.get("status"):
            print(f"tool_call_update {update.get('toolCallId')} : {update.get('status')}")
        elif kind == "agent_message_chunk":
            print(f"agent : {(update.get('content') or {}).get('text', '')}")
        if frame.get("method") == "session/request_permission":
            params = frame["params"]
            call_id = params["toolCall"]["toolCallId"]
            kinds = [option["kind"] for option in params["options"]]
            print(f"demande pour {call_id}, statut {params['toolCall'].get('status')}, "
                  f"options {kinds}")
            option = ask(cells.get(call_id, ""))
            answers.append("allow" if option == "allow-once" else "deny")
            acp.send({"jsonrpc": "2.0", "id": frame["id"],
                      "result": {"outcome": {"outcome": "selected", "optionId": option}}})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--home", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture" / "home")
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.home.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="ya-perm-acp-")).resolve()
    project, agent, tmp = work / "project", work / "agent", work / "tmp"
    for directory in (project, agent, tmp):
        directory.mkdir()
    (agent / "settings.json").write_text("{}\n")
    script = work / "faux-script.json"
    script.write_text(json.dumps(faux_script()))
    env = {
        "PATH": f"/usr/bin:/bin:/usr/sbin:/sbin:{Path(shutil.which('uv') or '/opt/homebrew/bin/uv').parent}",
        "HOME": str(args.home.resolve()),
        "TMPDIR": str(tmp),
        "LANG": "en_US.UTF-8",
        "DO_NOT_TRACK": "1",
        "PI_OFFLINE": "1",
        "PI_SKIP_VERSION_CHECK": "1",
        "PI_CACHE_RETENTION": "short",
        "PRIME_AGENT_CODING_AGENT_DIR": str(agent),
        "PRIME_AGENT_FAUX_SCRIPT": str(script),
    }
    print(f"dossier de travail : {work}")
    acp = Acp(binary, env, project, work / "d.sock", work / "acp-stderr.log")
    try:
        acp.wait(acp.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}}))
        new = acp.wait(acp.request("session/new", {"cwd": str(project), "mcpServers": []}))
        result = new.get("result") or sys.exit(f"session/new en échec : {new}")
        session_id = result["sessionId"]
        selector = [option for option in result.get("configOptions", [])
                    if option.get("id") == "permissions"]
        print(f"sélecteur permissions : {selector[0]['currentValue'] if selector else 'absent'}")
        answers: list[str] = []
        for name, _ in CELLS:
            print(f"\n=== tour : créer {name} ===")
            answers += turn(acp, session_id, f"Crée {name}.")
    finally:
        acp.close(binary)
    print("\n=== résultat ===")
    failures = []
    for (name, _), answer in zip(CELLS, answers):
        created = (project / name).exists()
        print(f"{name} : {'créé' if created else 'absent'} (réponse : {answer})")
        if created != (answer == "allow"):
            failures.append(f"{name} : {'créé' if created else 'absent'} après {answer}")
    entries = [json.loads(line) for session in sorted((agent / "sessions").rglob("*.jsonl"))
               for line in session.read_text().splitlines()]
    messages = sum(1 for entry in entries if entry.get("type") == "message")
    print(f"messages dans le fichier de session : {messages}")
    if messages == 0:
        failures.append("aucun message dans le fichier de session")
    decisions = []
    print("entrées ya-permission :")
    for entry in entries:
        if entry.get("type") == "custom" and entry.get("customType") == "ya-permission":
            print(f"  {json.dumps(entry.get('data'), ensure_ascii=False)}")
            decisions.append((entry.get("data") or {}).get("decision"))
    if decisions != answers:
        failures.append(f"journal {decisions}, réponses {answers}")
    errors = [line for log in sorted((agent / "logs").glob("worker-*.stderr.log"))
              for line in log.read_text().splitlines() if line.startswith("pa-daemon:")]
    for line in errors:
        print(f"journal du worker : {line}")
    if errors:
        failures.append(f"{len(errors)} erreur(s) pa-daemon dans le journal du worker")
    print(f"journal du binaire : {work / 'acp-stderr.log'}")
    if failures:
        print("ÉCHEC : " + " ; ".join(failures))
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
