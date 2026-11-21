#!/usr/bin/env python3
"""Compare, champ par champ, les trames de l'agent factice de l'extension à celles du vrai binaire.

YA-10 (ticket 24, PR 3). Vrai binaire : le dossier écrit par `acp_frames.py` (cas A `display` en
« auto », cas B `basic` en « manual » avec refus). Agent factice
(`extensions/vscode/test/fake-agent/fake-agent.mjs`) : lancé ici avec les prompts `carte`, `refus`,
`lent` et `réveil` (ticket 25 : `turnStart` d'un prompt et d'un tour de réveil, fin du réveil). Pour chaque paire de trames : chemins de clés (avec le type de la valeur) présents d'un
seul côté, types différents, valeurs des champs constants (statuts, genres, titres, identifiants
d'option, texte du refus, forme des identifiants de requête), trames de fin d'un tour de prompt.

Usage :
    python3 -I bench/tools/capture/compare_frames.py <dossier acp_frames> [--fake <fake-agent.mjs>]

Code de sortie 0 si aucun écart (`écarts : 0`), 1 sinon. Node requis.
"""

from __future__ import annotations

import json
import os
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import argparse

_REPO = Path(__file__).resolve().parents[3]
_parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
_parser.add_argument("captured", type=Path, help="dossier écrit par acp_frames.py")
_parser.add_argument("--fake", type=Path, default=_REPO / "extensions/vscode/test/fake-agent/fake-agent.mjs")
_args = _parser.parse_args()
fake, captured = _args.fake.resolve(), _args.captured.resolve()


def real_frames(case: str) -> list[dict]:
    rows = [json.loads(line) for line in (captured / case / "transcript.jsonl").read_text().splitlines()]
    return [row["msg"] for row in rows if row["dir"] == "agent->client"]


def fake_frames(prompt: str) -> list[dict]:
    control = tempfile.mkdtemp(prefix="fake-acp-")
    (Path(control) / "scenario.json").write_text("{}")
    proc = subprocess.Popen(["node", str(fake)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, text=True, bufsize=1,
                            env={**os.environ, "FAKE_ACP_DIR": control}, cwd=control)
    lines: queue.Queue[str] = queue.Queue()
    threading.Thread(target=lambda: [lines.put(l) for l in proc.stdout], daemon=True).start()
    frames: list[dict] = []

    def send(message: dict) -> None:
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
        proc.stdin.flush()

    def wait(req_id, seconds=5.0, on_request=None):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                message = json.loads(lines.get(timeout=0.2))
            except queue.Empty:
                continue
            frames.append(message)
            if "method" in message and "id" in message and on_request:
                on_request(message)
            if message.get("id") == req_id and "method" not in message:
                return message
        return None

    send({"id": 1, "method": "initialize", "params": {"protocolVersion": 1, "clientCapabilities": {}}})
    wait(1)
    send({"id": 2, "method": "session/new", "params": {"cwd": control, "mcpServers": []}})
    sid = wait(2)["result"]["sessionId"]
    frames.clear()
    send({"id": 3, "method": "session/prompt", "params": {"sessionId": sid, "prompt": [{"type": "text", "text": prompt}]}})
    if prompt == "réveil":
        # Ticket 25 : le tour de réveil suit la réponse du prompt.
        wait(3)
        wait(None, 1.5)
    elif prompt == "lent":
        wait(None, 1.0)
        send({"method": "session/cancel", "params": {"sessionId": sid}})
        wait(3, 4.0, on_request=lambda m: send({"id": m["id"], "result": {"outcome": {"outcome": "cancelled"}}}))
    else:
        wait(3)
    proc.stdin.close()
    proc.wait(timeout=5)
    return frames


def schema(value, path="") -> dict[str, str]:
    out: dict[str, str] = {}
    if isinstance(value, dict):
        out[path or "."] = "object"
        for key, item in value.items():
            out.update(schema(item, f"{path}.{key}"))
    elif isinstance(value, list):
        out[path or "."] = "array"
        for item in value:
            for key, kind in schema(item, f"{path}[]").items():
                out.setdefault(key, kind)
    else:
        out[path] = type(value).__name__
    return out


CONSTANT = re.compile(r"(\.sessionUpdate|\.title|\.kind|\.status|\.type|\.optionId|\.name|\.phase|\.outcome|\.method|\.jsonrpc)$")


def constants(value, path="") -> dict[str, set]:
    out: dict[str, set] = {}

    def walk(v, p):
        if isinstance(v, dict):
            for k, i in v.items():
                walk(i, f"{p}.{k}")
        elif isinstance(v, list):
            for i in v:
                walk(i, f"{p}[]")
        elif CONSTANT.search(p):
            out.setdefault(p, set()).add(json.dumps(v, ensure_ascii=False))
    walk(value, path)
    return out


def compare(label: str, real: dict, mine: dict, ignore_values: set[str] = frozenset()) -> int:
    a, b = schema(real), schema(mine)
    diffs = []
    for key in sorted(set(a) | set(b)):
        if key not in b:
            diffs.append(f"  absent du factice : {key} ({a[key]})")
        elif key not in a:
            diffs.append(f"  en trop dans le factice : {key} ({b[key]})")
        elif a[key] != b[key]:
            diffs.append(f"  type : {key} réel {a[key]}, factice {b[key]}")
    ca, cb = constants(real), constants(mine)
    for key in sorted(set(ca) & set(cb)):
        if key in ignore_values:
            continue
        if not cb[key] <= ca[key] and not ca[key] <= cb[key]:
            diffs.append(f"  valeur : {key} réel {sorted(ca[key])}, factice {sorted(cb[key])}")
    print(f"{'IDENTIQUE' if not diffs else 'DIFFÉRENT'} : {label}")
    for line in diffs:
        print(line)
    return len(diffs)


def update(frames, kind, pred=lambda u: True):
    for frame in frames:
        u = (frame.get("params") or {}).get("update") or {}
        if u.get("sessionUpdate") == kind and pred(u):
            return frame
    return None


A, B = real_frames("A-display-auto"), real_frames("B-basic-manual-reject")
carte, refus, lent = fake_frames("carte"), fake_frames("refus"), fake_frames("lent")
reveil = fake_frames("réveil")
meta = lambda u: u.get("_meta") or {}
total = 0

# Cellule en auto : `tool_call`.
total += compare("tool_call (auto, in_progress)", update(A, "tool_call"), update(carte, "tool_call"))
# `bash()` lus : le `tool_call_update` qui porte `_yus/bashRuns` (réel : cellule 1, sans diff).
# Seulement `_yus/bashRuns` : la cellule réelle n'a pas de diff, celle du factice en a (le pont met
# alors `ipython.diffCount` dans le même `_meta`, `wire_events.rs:190-197`, comparé plus bas).
total += compare("tool_call_update : _meta._yus/bashRuns",
                 {"_yus/bashRuns": meta(update(A, "tool_call_update", lambda u: "_yus/bashRuns" in meta(u))["params"]["update"])["_yus/bashRuns"]},
                 {"_yus/bashRuns": meta(update(carte, "tool_call_update", lambda u: "_yus/bashRuns" in meta(u))["params"]["update"])["_yus/bashRuns"]})
# Diffs : le `tool_call_update` d'un `edit()` qui modifie (réel : cellule 4), puis d'une création (cellule 3).
real_edit = update(A, "tool_call_update", lambda u: any(c.get("type") == "diff" and "oldText" in c for c in u.get("content", [])))
real_new = update(A, "tool_call_update", lambda u: any(c.get("type") == "diff" and "oldText" not in c for c in u.get("content", [])))
mine = update(carte, "tool_call_update", lambda u: any(c.get("type") == "diff" for c in u.get("content", [])))
mine_u = mine["params"]["update"]
mine_edit = {**mine, "params": {**mine["params"], "update": {**mine_u, "_meta": {k: v for k, v in meta(mine_u).items() if k != "_yus/bashRuns"},
             "content": [c for c in mine_u["content"] if c.get("type") != "diff" or "oldText" in c]}}}
mine_new = {**mine, "params": {**mine["params"], "update": {**mine_u, "_meta": {k: v for k, v in meta(mine_u).items() if k != "_yus/bashRuns"},
            "content": [c for c in mine_u["content"] if c.get("type") != "diff" or "oldText" not in c]}}}
total += compare("tool_call_update, edit() qui modifie (contenu diff, _meta)", real_edit, mine_edit)
total += compare("tool_call_update, edit() qui crée (contenu diff sans oldText, _meta)", real_new, mine_new)
# Manuel : `tool_call` en attente, demande d'accord, refus.
total += compare("tool_call (manuel, pending)", update(B, "tool_call"), update(refus, "tool_call"))
real_req = next(f for f in B if f.get("method") == "session/request_permission")
mine_req = next(f for f in lent if f.get("method") == "session/request_permission")
total += compare("session/request_permission", real_req, mine_req)
print(f"  id réel {real_req['id']!r}, factice {mine_req['id']!r}"
      f" ({'même forme' if re.sub(r'\\d+$', '', real_req['id']) == re.sub(r'\\d+$', '', mine_req['id']) else 'FORME DIFFÉRENTE'})")
total += compare("tool_call_update refusé (failed, texte fixe)",
                 update(B, "tool_call_update", lambda u: u.get("status") == "failed"),
                 update(refus, "tool_call_update", lambda u: u.get("status") == "failed"))
real_text = update(B, "tool_call_update", lambda u: u.get("status") == "failed")["params"]["update"]["content"][0]["content"]["text"]
mine_text = update(refus, "tool_call_update", lambda u: u.get("status") == "failed")["params"]["update"]["content"][0]["content"]["text"]
print(f"  texte du refus {'identique' if real_text == mine_text else 'DIFFÉRENT'}")
# Fin d'un tour de prompt : trames entre le dernier contenu et la réponse.
def tail(frames):
    end = next(i for i, f in enumerate(frames) if f.get("id") == 3 and "result" in f)
    out = []
    for f in frames[:end][::-1]:
        u = (f.get("params") or {}).get("update") or {}
        if u.get("sessionUpdate") != "session_info_update":
            break
        out.insert(0, meta(u).get("ai.primeintellect.prime-agent", {}).get("phase"))
    return out
print(f"fin de tour avant la réponse : réel {tail(B)}, factice {tail(refus)}"
      f" ({'IDENTIQUE' if tail(B) == tail(refus) else 'DIFFÉRENT'})")
total += 0 if tail(B) == tail(refus) else 1
# Ticket 25 : `turnStart` d'un prompt, premier `_meta` du tour.
starts = lambda frames, origin: [f for f in frames if (meta((f.get("params") or {}).get("update") or {})
                                 .get("ai.primeintellect.prime-agent", {}).get("turnStart") or {}).get("origin") == origin]
real_prompt_start, mine_prompt_start = starts(A, "prompt"), starts(carte, "prompt")
total += compare("session_info_update turnStart (prompt)", real_prompt_start[0] if real_prompt_start else {},
                 mine_prompt_start[0] if mine_prompt_start else {})
# Ticket 25 : le tour de réveil (avis `[bash-done]`) : `turnStart` wake, puis sa fin.
real_wake, mine_wake = starts(A, "wake"), starts(reveil, "wake")
total += compare("session_info_update turnStart (wake)", real_wake[0] if real_wake else {},
                 mine_wake[0] if mine_wake else {})
def wake_end(frames, start):
    if not start:
        return None
    turn = meta(start[0]["params"]["update"])["ai.primeintellect.prime-agent"]["promptTurnId"]
    out = []
    for f in frames[frames.index(start[0]) + 1:]:
        u = (f.get("params") or {}).get("update") or {}
        m = meta(u).get("ai.primeintellect.prime-agent", {})
        if m.get("promptTurnId") == turn and u.get("sessionUpdate") == "session_info_update":
            out.append(m.get("phase"))
    return out
print(f"fin du tour de réveil : réel {wake_end(A, real_wake)}, factice {wake_end(reveil, mine_wake)}"
      f" ({'IDENTIQUE' if wake_end(A, real_wake) == wake_end(reveil, mine_wake) else 'DIFFÉRENT'})")
total += 0 if real_wake and wake_end(A, real_wake) == wake_end(reveil, mine_wake) else 1
# Message de l'agent.
total += compare("agent_message_chunk", update(A, "agent_message_chunk"), update(carte, "agent_message_chunk") or update(lent, "agent_message_chunk"),
                 ignore_values={".params.update.content.type"})
print(f"\nécarts : {total}")
sys.exit(1 if total else 0)
