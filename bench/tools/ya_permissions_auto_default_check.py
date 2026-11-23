#!/usr/bin/env python3
"""Contrôle du ticket 30 : permissions en « auto » par défaut, sur un binaire donné.

Aucun appel de modèle. Chaque cas a son dossier agent temporaire (`PRIME_AGENT_CODING_AGENT_DIR`)
et son projet, sous un dossier de travail canonique (défaut du bail sous un lien symbolique,
`docs/tickets/21-bail-chemin-symbolique.md`). HOME est le dossier du banc de capture
(`~/.cache/yusagent-capture/home`, venv du kernel), comme `ya_permissions_acp_demo.py`.

1. `-p` sans réglage : un serveur local joue le modèle (fournisseur `openai-responses` de
   `models.json`, réponse 400) et compte ses connexions. Attendu : au moins une connexion,
   l'avertissement « no sandbox » sur stderr, pas de refus.
2. `-p` avec `"permissions": "manual"` dans les réglages globaux : code de sortie non nul, message
   de refus avec la ligne `"permissions": "auto"` et le chemin des réglages globaux, sans
   « (the default) », 0 connexion.
3. `-p` sans réglage global, `"permissions": "manual"` dans `<projet>/.yusagent/settings.json` :
   le projet n'est pas lu (D1) ; attendu comme en 1.
4. ACP sans réglage (provider factice) : sélecteur `permissions` à `auto` ; une cellule tourne
   sans `session/request_permission` ; passage en `manual` par `session/set_config_option` ; la
   cellule suivante est demandée, accordée, exécutée ; journal `auto->manual` puis `allow`.
5. Rien d'écrit dans `~/.yusagent` ni `~/.prime/agent` du vrai HOME.

Code de sortie 1 si une vérification échoue.

Usage :
    python3 -I bench/tools/ya_permissions_auto_default_check.py --binary <dossier>/yusagent
"""

from __future__ import annotations

import argparse
import http.server
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

TIMEOUT_S = 180
WARNING = 'permissions are "auto" and there is no sandbox'
REFUSAL = 'permissions are "manual"'
LINE = '\n  "permissions": "auto"\n'

failures: list[str] = []


def check(ok: bool, what: str) -> None:
    print(f"  [{'ok' if ok else 'ÉCHEC'}] {what}")
    if not ok:
        failures.append(what)


class ModelServer:
    """Compte les connexions et répond 400 (aucune nouvelle tentative)."""

    def __init__(self) -> None:
        server = self
        self.connections = 0

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                server.connections += 1
                length = int(self.headers.get("Content-Length") or 0)
                self.rfile.read(length)
                body = b'{"error":{"message":"auto default check","type":"invalid_request_error"}}'
                self.send_response(400)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args) -> None:
                pass

        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def close(self) -> None:
        self.httpd.shutdown()


def base_env(home: Path, agent: Path, tmp: Path) -> dict[str, str]:
    return {
        "PATH": f"/usr/bin:/bin:/usr/sbin:/sbin:{Path(shutil.which('uv') or '/opt/homebrew/bin/uv').parent}",
        "HOME": str(home),
        "TMPDIR": str(tmp),
        "LANG": "en_US.UTF-8",
        "DO_NOT_TRACK": "1",
        "PI_OFFLINE": "1",
        "PI_SKIP_VERSION_CHECK": "1",
        "PI_CACHE_RETENTION": "short",
        "PRIME_AGENT_CODING_AGENT_DIR": str(agent),
    }


def case_dirs(work: Path, name: str, glob: dict | None, project_settings: dict | None):
    root = work / name
    project, agent, tmp = root / "project", root / "agent", root / "tmp"
    for directory in (project, agent, tmp):
        directory.mkdir(parents=True)
    if glob is not None:
        (agent / "settings.json").write_text(json.dumps(glob) + "\n")
    if project_settings is not None:
        (project / ".yusagent").mkdir()
        (project / ".yusagent" / "settings.json").write_text(json.dumps(project_settings) + "\n")
    return root, project, agent, tmp


def print_case(binary: Path, home: Path, work: Path, name: str, glob: dict | None,
               project_settings: dict | None) -> tuple[int, str, int, Path]:
    root, project, agent, tmp = case_dirs(work, name, glob, project_settings)
    server = ModelServer()
    (agent / "models.json").write_text(json.dumps({"providers": {"capture": {
        "baseUrl": f"http://127.0.0.1:{server.port}/v1",
        "apiKey": "auto-default-dummy",
        "api": "openai-responses",
        "models": [{
            "id": "capture-model", "name": "Capture", "reasoning": False, "input": ["text"],
            "contextWindow": 200000, "maxTokens": 32000,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        }],
    }}}) + "\n")
    try:
        proc = subprocess.run(
            [str(binary), "-p", "--provider", "capture", "--model", "capture-model", "bonjour"],
            env=base_env(home, agent, tmp), cwd=project, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, timeout=TIMEOUT_S)
    finally:
        server.close()
    (root / "stderr.log").write_text(proc.stderr)
    return proc.returncode, proc.stderr, server.connections, agent


def show(code: int, stderr: str, connections: int) -> None:
    print(f"  code de sortie : {code} ; connexions au serveur : {connections}")
    for line in stderr.strip().splitlines()[:8]:
        print(f"  stderr | {line}")


def auto_case(binary: Path, home: Path, work: Path, name: str, project_settings: dict | None) -> None:
    code, stderr, connections, _ = print_case(binary, home, work, name, None, project_settings)
    show(code, stderr, connections)
    check(connections >= 1, "le modèle est atteint (au moins une connexion)")
    check(WARNING in stderr, "avertissement « no sandbox » sur stderr")
    check(REFUSAL not in stderr, "pas de refus au démarrage")


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
                    raise RuntimeError(f"le binaire s'est arrêté (code {self.proc.returncode})")
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue
        raise RuntimeError("délai dépassé")

    def wait(self, req_id: int) -> tuple[dict, list[dict]]:
        before = []
        for frame in self.frames():
            if frame.get("id") == req_id and "method" not in frame:
                return frame, before
            before.append(frame)
        raise AssertionError("unreachable")

    def close(self) -> None:
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except (subprocess.TimeoutExpired, BrokenPipeError):
            self.proc.kill()
        self.err.close()
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {self.socket}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and subprocess.run(
                ["pgrep", "-f", str(self.socket)], capture_output=True).returncode == 0:
            time.sleep(0.2)


def cell(call_id: str, name: str) -> dict:
    return {"content": [{"type": "toolCall", "id": call_id, "name": "python_repl",
                         "arguments": {"code": f"open({name!r}, 'w').write('ok')"}}]}


def permissions_value(options: list[dict]) -> str | None:
    found = [option for option in options if option.get("id") == "permissions"]
    return found[0].get("currentValue") if found else None


def acp_case(binary: Path, home: Path, work: Path) -> None:
    root, project, agent, tmp = case_dirs(work, "4-acp-sans-reglage", None, None)
    script = root / "faux-script.json"
    script.write_text(json.dumps({"engine": "faux", "responses": [
        cell("call-defaut", "defaut.txt"), {"text": "cellule du défaut exécutée"},
        cell("call-manuel", "manuel.txt"), {"text": "cellule demandée puis exécutée"},
    ]}))
    env = base_env(home, agent, tmp) | {"PRIME_AGENT_FAUX_SCRIPT": str(script)}
    acp = Acp(binary, env, project, root / "d.sock", root / "acp-stderr.log")
    try:
        acp.wait(acp.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}}))
        new, _ = acp.wait(acp.request("session/new", {"cwd": str(project), "mcpServers": []}))
        result = new.get("result") or {}
        session_id = result.get("sessionId")
        value = permissions_value(result.get("configOptions", []))
        print(f"  sélecteur permissions à l'ouverture : {value}")
        check(value == "auto", "session ouverte en auto")
        done, frames = acp.wait(acp.request("session/prompt", {
            "sessionId": session_id, "prompt": [{"type": "text", "text": "Crée defaut.txt."}]}))
        asked = [frame for frame in frames if frame.get("method") == "session/request_permission"]
        print(f"  tour 1 : {json.dumps(done.get('result') or done.get('error'))} ; demandes : {len(asked)}")
        check(not asked, "aucune demande en auto")
        check((project / "defaut.txt").exists(), "defaut.txt créé sans demande")
        switched, _ = acp.wait(acp.request("session/set_config_option", {
            "sessionId": session_id, "configId": "permissions", "value": "manual"}))
        value = permissions_value((switched.get("result") or {}).get("configOptions", []))
        print(f"  sélecteur après set_config_option : {value}")
        check(value == "manual", "passage en manuel")
        prompt = acp.request("session/prompt", {
            "sessionId": session_id, "prompt": [{"type": "text", "text": "Crée manuel.txt."}]})
        requests = 0
        for frame in acp.frames():
            if frame.get("method") == "session/request_permission":
                requests += 1
                print(f"  demande pour {frame['params']['toolCall']['toolCallId']} : accordée")
                acp.send({"jsonrpc": "2.0", "id": frame["id"], "result": {
                    "outcome": {"outcome": "selected", "optionId": "allow-once"}}})
            if frame.get("id") == prompt and "method" not in frame:
                print(f"  tour 2 : {json.dumps(frame.get('result') or frame.get('error'))}")
                break
        check(requests == 1, "une demande en manuel")
        check((project / "manuel.txt").exists(), "manuel.txt créé après l'accord")
    except RuntimeError as error:
        check(False, f"ACP : {error}")
    finally:
        acp.close()
    entries = [json.loads(line) for session in sorted((agent / "sessions").rglob("*.jsonl"))
               for line in session.read_text().splitlines()]
    journal = []
    for entry in entries:
        if entry.get("type") == "custom" and entry.get("customType") == "ya-permission":
            data = entry.get("data") or {}
            journal.append(data.get("decision") or f"{data.get('previousMode')}->{data.get('mode')}")
    print(f"  journal ya-permission : {journal}")
    check(journal == ["auto->manual", "allow"], "journal auto->manual puis allow")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--home", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture" / "home")
    args = parser.parse_args()
    binary = args.binary.resolve()
    home = args.home.resolve()
    home.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="ya-auto-default-")).resolve()
    marker = work / "debut"
    marker.write_text("")
    print(f"binaire : {binary}")
    print(f"version : {subprocess.run([str(binary), '--version'], capture_output=True, text=True).stdout.strip()}")
    print(f"dossier de travail : {work}")

    print("\n1. -p sans réglage")
    auto_case(binary, home, work, "1-p-sans-reglage", None)

    print("\n2. -p avec \"permissions\": \"manual\" dans les réglages globaux")
    code, stderr, connections, agent = print_case(
        binary, home, work, "2-p-manuel-global", {"permissions": "manual"}, None)
    show(code, stderr, connections)
    check(code != 0, "code de sortie non nul")
    check(REFUSAL in stderr and LINE in stderr, "refus avec la ligne exacte")
    check(str(agent / "settings.json") in stderr, "chemin des réglages globaux dans le message")
    check("(the default)" not in stderr, "sans « (the default) »")
    check(connections == 0, "aucune connexion au serveur")

    print("\n3. -p sans réglage global, \"manual\" dans le projet (non lu)")
    auto_case(binary, home, work, "3-p-manuel-projet", {"permissions": "manual"})

    print("\n4. ACP sans réglage, puis manuel par le sélecteur")
    acp_case(binary, home, work)

    print("\n5. Vrai HOME")
    written = subprocess.run(
        ["find", str(Path.home() / ".yusagent"), str(Path.home() / ".prime" / "agent"),
         "-newer", str(marker)], capture_output=True, text=True).stdout.strip()
    print(f"  find -newer : {written or '(vide)'}")
    check(written == "", "rien d'écrit dans ~/.yusagent ni ~/.prime/agent")

    print()
    if failures:
        print("ÉCHEC : " + " ; ".join(failures))
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
