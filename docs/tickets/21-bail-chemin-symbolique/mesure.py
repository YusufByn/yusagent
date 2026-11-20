#!/usr/bin/env python3
"""Ticket 21 : messages écrits dans le fichier de session d'une session ACP neuve.

Un tour ACP du provider factice (aucun appel de modèle) sur un démon isolé, dossier agent neuf
créé dans <base> ; la cellule éventuelle est acceptée. Affiche : dossier, réponse du tour, nombre
de lignes et de messages du fichier de session, `customType` des entrées `custom`, lignes
`pa-daemon:` du journal du worker.

Usage :
    python3 -I docs/tickets/21-bail-chemin-symbolique/mesure.py <binaire> <base> text|cell [--sessions-avant]

`<base>` : par exemple `$(python3 -c 'import tempfile; print(tempfile.gettempdir())')` (macOS :
sous `/var/folders`, lien symbolique) ou `/private/tmp/ya-bail` (canonique, à créer).
`--sessions-avant` crée le dossier `sessions` avant le lancement.
HOME : `~/.cache/yusagent-capture/home` (venv du kernel du banc de capture).
"""

import json
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

binary, base, kind = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
work = Path(tempfile.mkdtemp(prefix="ya-bail-", dir=base))
project, agent, tmp = work / "project", work / "agent", work / "tmp"
for directory in (project, agent, tmp):
    directory.mkdir()
if "--sessions-avant" in sys.argv:
    (agent / "sessions").mkdir()
(agent / "settings.json").write_text("{}\n")
cell = [{"content": [{"type": "toolCall", "id": "c1", "name": "python_repl",
                      "arguments": {"code": "print(1)"}}]}]
(work / "script.json").write_text(json.dumps(
    {"engine": "faux", "responses": (cell if kind == "cell" else []) + [{"text": "fin"}]}))
env = {
    "PATH": f"/usr/bin:/bin:{Path(shutil.which('uv') or '/opt/homebrew/bin/uv').parent}",
    "HOME": str(Path.home() / ".cache" / "yusagent-capture" / "home"),
    "TMPDIR": str(tmp), "DO_NOT_TRACK": "1", "PI_OFFLINE": "1", "PI_SKIP_VERSION_CHECK": "1",
    "PI_CACHE_RETENTION": "short", "PRIME_AGENT_CODING_AGENT_DIR": str(agent),
    "PRIME_AGENT_FAUX_SCRIPT": str(work / "script.json"),
}
sock = work / "d.sock"
proc = subprocess.Popen([str(binary), "--mode", "acp", "--daemon-socket", str(sock)],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                        env=env, cwd=project, text=True, bufsize=1)
lines: queue.Queue[str] = queue.Queue()
threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()


def send(message: dict) -> None:
    proc.stdin.write(json.dumps(message) + "\n")
    proc.stdin.flush()


def wait(request_id: int) -> dict:
    deadline = time.time() + 180
    while time.time() < deadline:
        try:
            frame = json.loads(lines.get(timeout=1))
        except queue.Empty:
            continue
        if frame.get("method") == "session/request_permission":
            send({"jsonrpc": "2.0", "id": frame["id"],
                  "result": {"outcome": {"outcome": "selected", "optionId": "allow-once"}}})
        if frame.get("id") == request_id and "method" not in frame:
            return frame
    sys.exit("délai dépassé")


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": 1, "clientCapabilities": {}}})
wait(1)
send({"jsonrpc": "2.0", "id": 2, "method": "session/new",
      "params": {"cwd": str(project), "mcpServers": []}})
session_id = wait(2)["result"]["sessionId"]
send({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
      "params": {"sessionId": session_id, "prompt": [{"type": "text", "text": "go"}]}})
answer = wait(3)
time.sleep(1)
proc.stdin.close()
proc.wait(timeout=20)
subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {sock}"])
time.sleep(3)
for session in (agent / "sessions").rglob("*.jsonl"):
    entries = [json.loads(line) for line in session.read_text().splitlines()]
    print(work, answer.get("result"), len(entries),
          sum(1 for entry in entries if entry.get("type") == "message"),
          [entry.get("customType") for entry in entries if entry.get("type") == "custom"])
for log in (agent / "logs").glob("worker-*.stderr.log"):
    print([line for line in log.read_text().splitlines() if line.startswith("pa-daemon:")])
