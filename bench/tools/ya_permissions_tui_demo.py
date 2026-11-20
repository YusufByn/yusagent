#!/usr/bin/env python3
"""Démo TUI des permissions (YA-11, ticket 20, PR 3 ; ticket 22) : autoriser, refuser, puis « auto ».

Lance un démon isolé (`--mode daemon --daemon-socket` dans un dossier temporaire) puis le TUI
du binaire, avec le provider factice (`PRIME_AGENT_FAUX_SCRIPT`, `"engine": "faux"`) : aucun
appel de modèle. Dossier agent temporaire sans réglage `permissions` : mode « manual » (le
défaut). Le provider factice joue, dans l'ordre, une cellule qui crée `autorise.txt`, un texte,
une cellule qui crée `refuse.txt`, une cellule qui crée `auto.txt`, un texte. Un tour arrêté par
un refus ne demande rien de plus au modèle : la cellule `auto.txt` sert donc au tour suivant.

À faire dans le TUI (mode normal) :
  1. taper `Crée autorise.txt` puis Entrée. Le panneau « Permission · python_repl » s'ouvre en
     bas de la transcription avec la cellule entière et « pause typing to answer ». Taper quelques
     lettres (par exemple `any`) : elles vont dans l'éditeur, le panneau reste ouvert. Pendant
     la demande, le chargeur dit « Awaiting permission » et la carte de la cellule n'a ni `✓` ni
     minuteur (ticket 22). S'arrêter une seconde : la ligne du bas devient « y allow once · n or Esc deny · Ctrl+C
     withdraw ». Effacer les lettres tapées (elles restent dans l'éditeur), puis attendre la même
     pause et taper `y` : la cellule tourne, le texte « Tour terminé après la cellule qui crée
     autorise.txt. » s'affiche ;
  2. taper `Crée refuse.txt` puis Entrée ; au panneau, après la pause, taper `n` : le panneau se
     ferme, la cellule finit en erreur `[permission-denied] …`, aucun autre message d'agent ;
  3. taper `/permissions auto` puis Entrée (deux fois si la liste de complétion est ouverte) :
     la note « Permissions: auto. … (no sandbox). » s'affiche et la barre sous l'éditeur montre
     « auto · no sandbox » à droite ;
  4. taper `Crée auto.txt` puis Entrée : la cellule tourne sans panneau ;
  5. quitter (`/quit`).

Mode `--pilote` (le lancement de Claude Code, règle 13) : le même scénario joué sur un pty, l'écran
lu par l'émulateur de `scripts/lifecycle_lib.py`, avec des extraits d'écran imprimés. Il vérifie
aussi l'écran (ticket 22) : pendant chaque demande, « Awaiting permission » présent, « Executing »
absent, pas de `✓` sur la carte de la cellule ; après `y`, la carte finit en `✓` ; après `n`, en
`✗` ; il s'arrête avec le code 1 sinon.

Vérifications à la fin (code de sortie 1 si l'une échoue) : `autorise.txt` et `auto.txt` créés,
`refuse.txt` absent ; des messages dans le fichier de session ; journal `ya-permission` égal à
`allow`, `deny`, puis le changement `manual->auto` ; aucune ligne `pa-daemon:` dans le journal du
worker.

HOME est le dossier préparé par le banc de capture (`~/.cache/yusagent-capture/home`, venv du
kernel) : rien n'est écrit dans `~/.yusagent`. Le dossier de travail est pris sous son chemin
canonique (`docs/tickets/21-bail-chemin-symbolique.md`).

Usage :
    python3 -I bench/tools/ya_permissions_tui_demo.py --binary <dossier>/yusagent [--pilote]
"""

from __future__ import annotations

import argparse
import fcntl
import importlib.util
import json
import os
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TIMEOUT_S = 180
ROWS, COLS = 40, 120
PANEL = "Permission · python_repl"
WAITING = "pause typing to answer"
ARMED = "y allow once"
AWAITING = "Awaiting permission"
BADGE = "auto · no sandbox"
EXPECTED_JOURNAL = ["allow", "deny", "manual->auto"]


def cell(call_id: str, name: str) -> dict:
    return {"content": [{
        "type": "toolCall", "id": call_id, "name": "python_repl",
        "arguments": {"code": f"open({name!r}, 'w').write('ok')\nprint('écrit : {name}')"},
    }]}


def faux_script() -> dict:
    return {"engine": "faux", "responses": [
        cell("call-autorise", "autorise.txt"),
        {"text": "Tour terminé après la cellule qui crée autorise.txt."},
        cell("call-refuse", "refuse.txt"),
        # Jamais demandée pour le tour refusé : le tour suivant la reçoit.
        cell("call-auto", "auto.txt"),
        {"text": "Tour terminé après la cellule qui crée auto.txt."},
    ]}


def card_line(screen: str, name: str) -> str | None:
    """La ligne de la carte de la cellule qui crée `name` (`<marque> python · open('name'…`) ; le
    panneau montre le même code sans l'étiquette `python · `."""
    return next((line for line in screen.splitlines() if "python · " in line and name in line), None)


def check_waiting(screen: str, name: str, moment: str, chargeur_visible: bool = True) -> None:
    """Ticket 22 : pendant une demande, rien ne dit que l'appel tourne. Le panneau recouvre le bas
    de la transcription : une fois la fenêtre remplie, le chargeur et la carte sont dessous
    (`chargeur_visible` faux)."""
    card = card_line(screen, name)
    print(f"[{moment}] chargeur « {AWAITING} » : {'oui' if AWAITING in screen else 'non'} ; "
          f"« Executing » : {'oui' if 'Executing' in screen else 'non'} ; carte : {card!r}")
    if (chargeur_visible and AWAITING not in screen) or "Executing" in screen or (card and "✓" in card):
        sys.exit(f"carte ou chargeur faux pendant la demande ({moment}) ; écran :\n{screen}")


def wait_card(tui, name: str, mark: str) -> None:
    screen = tui.wait(f"la carte {mark} de {name}",
                      lambda text: (card_line(text, name) or "").find(mark) >= 0)
    card = card_line(screen, name)
    print(f"carte de {name} : {card!r}")
    if mark != "✓" and "✓" in card:
        sys.exit(f"la carte de {name} montre un succès : {card!r}")


def load_screen():
    spec = importlib.util.spec_from_file_location("lifecycle_lib", REPO / "scripts" / "lifecycle_lib.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Screen


class Pilot:
    """Le TUI sur un pty dont il est le terminal de contrôle ; l'écran est émulé."""

    def __init__(self, command: list[str], env: dict[str, str], cwd: Path):
        self.screen = load_screen()(COLS, ROWS)
        master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

        def claim_tty() -> None:
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.proc = subprocess.Popen(command, env=env, cwd=cwd, stdin=slave, stdout=slave,
                                     stderr=slave, preexec_fn=claim_tty)
        os.close(slave)
        self.master = master

    def pump(self, seconds: float = 0.05) -> None:
        ready, _, _ = select.select([self.master], [], [], seconds)
        if not ready:
            return
        try:
            data = os.read(self.master, 65536)
        except OSError:
            return
        for reply in self.screen.feed(data):
            os.write(self.master, reply)

    def text(self) -> str:
        return self.screen.text()

    def wait(self, label: str, predicate) -> str:
        deadline = time.monotonic() + TIMEOUT_S
        while time.monotonic() < deadline:
            self.pump()
            if predicate(self.text()):
                return self.text()
            if self.proc.poll() is not None:
                sys.exit(f"le TUI s'est arrêté (code {self.proc.returncode}) en attendant {label}")
        sys.exit(f"délai dépassé en attendant {label} ; écran :\n{self.text()}")

    def wait_for(self, needle: str) -> str:
        return self.wait(needle, lambda text: needle in text)

    def type(self, keys: str, gap: float = 0.03) -> None:
        for key in keys:
            os.write(self.master, key.encode())
            deadline = time.monotonic() + gap
            while time.monotonic() < deadline:
                self.pump(0.01)

    def submit_until(self, line: str, label: str, predicate) -> None:
        """Une ligne, puis Entrée jusqu'à l'effet : Entrée sur une liste de complétion ouverte
        applique la complétion sans soumettre."""
        self.type(line)
        for _ in range(3):
            self.type("\r")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                self.pump()
                if predicate(self.text()):
                    return
        self.wait(label, predicate)

    def show(self, title: str) -> None:
        lines = [line for line in self.text().splitlines() if line.strip()]
        print(f"\n--- écran : {title} ---")
        print("\n".join(lines[-14:]))

    def close(self) -> None:
        if self.proc.poll() is None:
            self.type("/quit\r")
            deadline = time.monotonic() + 15
            while self.proc.poll() is None and time.monotonic() < deadline:
                self.pump(0.1)
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.wait()
        os.close(self.master)


def pilot(command: list[str], env: dict[str, str], project: Path) -> None:
    tui = Pilot(command, env, project)
    try:
        tui.wait_for("project")
        tui.pump(1.0)
        if BADGE in tui.text():
            sys.exit("l'étiquette « auto » est visible en manuel")

        print("\n=== 1. autoriser ===")
        tui.type("Crée autorise.txt\r")
        check_waiting(tui.wait_for(WAITING), "autorise.txt", "demande ouverte")
        tui.show("demande ouverte, fenêtre de calme")
        tui.type("any", gap=0.12)
        screen = tui.text()
        if PANEL not in screen or (project / "autorise.txt").exists():
            sys.exit(f"la frappe a répondu au panneau ; écran :\n{screen}")
        tui.show("« any » tapé pendant la demande : dans l'éditeur, panneau ouvert")
        check_waiting(tui.wait_for(ARMED), "autorise.txt", "panneau prêt")
        tui.show("après la pause : le panneau répond")
        tui.type("\x7f\x7f\x7f", gap=0.05)
        tui.wait_for(ARMED)
        tui.type("y")
        tui.wait_for("Tour terminé après la cellule qui crée autorise.txt.")
        wait_card(tui, "autorise.txt", "✓")
        tui.show("après `y`")

        print("\n=== 2. refuser ===")
        tui.type("Crée refuse.txt\r")
        tui.wait_for(WAITING)
        check_waiting(tui.wait_for(ARMED), "refuse.txt", "second panneau prêt", chargeur_visible=False)
        tui.type("n")
        tui.wait("la fermeture du panneau", lambda text: PANEL not in text)
        wait_card(tui, "refuse.txt", "✗")
        tui.pump(2.0)
        tui.show("après `n`")

        print("\n=== 3. /permissions auto ===")
        tui.submit_until("/permissions auto", BADGE, lambda text: BADGE in text)
        tui.show("en auto : l'étiquette à droite de la barre")

        print("\n=== 4. une cellule sans panneau ===")
        tui.type("Crée auto.txt\r")
        seen_panel = False
        deadline = time.monotonic() + TIMEOUT_S
        while "Tour terminé après la cellule qui crée auto.txt." not in tui.text():
            if time.monotonic() > deadline:
                sys.exit(f"délai dépassé au tour auto ; écran :\n{tui.text()}")
            tui.pump()
            seen_panel |= WAITING in tui.text()
        if seen_panel:
            sys.exit("un panneau s'est ouvert en auto")
        tui.show("après le tour en auto")
    finally:
        tui.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--pilote", action="store_true",
                        help="jouer le scénario sur un pty (lancement de Claude Code)")
    parser.add_argument("--home", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture" / "home")
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.home.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="ya-perm-tui-")).resolve()
    project, agent, tmp = work / "project", work / "agent", work / "tmp"
    for directory in (project, agent, tmp):
        directory.mkdir()
    # Pas d'écran d'accueil ; pas de réglage `permissions` : « manual ».
    (agent / "settings.json").write_text(json.dumps({"onboardingShown": True}) + "\n")
    script = work / "faux-script.json"
    script.write_text(json.dumps(faux_script()))
    socket = work / "d.sock"
    env = {
        "PATH": f"/usr/bin:/bin:/usr/sbin:/sbin:{Path(shutil.which('uv') or '/opt/homebrew/bin/uv').parent}",
        "HOME": str(args.home.resolve()),
        "TMPDIR": str(tmp),
        "LANG": os.environ.get("LANG", "en_US.UTF-8"),
        "DO_NOT_TRACK": "1",
        "PI_OFFLINE": "1",
        "PI_SKIP_VERSION_CHECK": "1",
        "PI_CACHE_RETENTION": "short",
        "PRIME_AGENT_CODING_AGENT_DIR": str(agent),
        "PRIME_AGENT_FAUX_SCRIPT": str(script),
    }
    print(f"dossier de travail : {work}")
    daemon_log = (work / "daemon-stderr.log").open("w")
    daemon = subprocess.Popen([str(binary), "--mode", "daemon", "--daemon-socket", str(socket)],
                              env=env, cwd=project, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=daemon_log)
    deadline = time.monotonic() + 30
    while not socket.exists():
        if time.monotonic() > deadline or daemon.poll() is not None:
            sys.exit(f"le démon n'a pas ouvert {socket}")
        time.sleep(0.1)
    command = [str(binary), "--daemon-socket", str(socket)]
    try:
        if args.pilote:
            pilot(command, env | {"TERM": "xterm-256color"}, project)
        else:
            print(__doc__.split("À faire dans le TUI (mode normal) :")[1].split("Mode `--pilote`")[0])
            input("Entrée pour lancer le TUI…")
            terminal = {key: os.environ[key] for key in ("TERM", "COLORTERM", "TERM_PROGRAM")
                        if key in os.environ}
            subprocess.run(command, env=env | terminal, cwd=project)
    finally:
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {socket}"])
        try:
            daemon.wait(timeout=30)
        except subprocess.TimeoutExpired:
            daemon.kill()
        daemon_log.close()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and subprocess.run(
                ["pgrep", "-f", str(socket)], capture_output=True).returncode == 0:
            time.sleep(0.2)

    print("\n=== résultat ===")
    failures = []
    for name, wanted in (("autorise.txt", True), ("refuse.txt", False), ("auto.txt", True)):
        created = (project / name).exists()
        print(f"{name} : {'créé' if created else 'absent'}")
        if created != wanted:
            failures.append(f"{name} {'créé' if created else 'absent'}")
    entries = [json.loads(line) for session in sorted((agent / "sessions").rglob("*.jsonl"))
               for line in session.read_text().splitlines()]
    messages = sum(1 for entry in entries if entry.get("type") == "message")
    print(f"messages dans le fichier de session : {messages}")
    if messages == 0:
        failures.append("aucun message dans le fichier de session")
    journal = []
    print("entrées ya-permission :")
    for entry in entries:
        if entry.get("type") == "custom" and entry.get("customType") == "ya-permission":
            data = entry.get("data") or {}
            print(f"  {json.dumps(data, ensure_ascii=False)}")
            journal.append(data.get("decision")
                           or f"{data.get('previousMode')}->{data.get('mode')}")
    if journal != EXPECTED_JOURNAL:
        failures.append(f"journal {journal}, attendu {EXPECTED_JOURNAL}")
    errors = [line for log in sorted((agent / "logs").glob("worker-*.stderr.log"))
              for line in log.read_text().splitlines() if line.startswith("pa-daemon:")]
    for line in errors:
        print(f"journal du worker : {line}")
    if errors:
        failures.append(f"{len(errors)} erreur(s) pa-daemon dans le journal du worker")
    if failures:
        print("ÉCHEC : " + " ; ".join(failures))
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
