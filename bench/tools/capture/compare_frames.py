#!/usr/bin/env python3
"""Compare, champ par champ, les trames de l'agent factice de l'extension à celles du vrai binaire.

YA-10 (ticket 24, PR 3). Vrai binaire : le dossier écrit par `acp_frames.py` (cas A `display` en
« auto », cas B `basic` en « manual » avec refus). Agent factice
(`extensions/vscode/test/fake-agent/fake-agent.mjs`) : lancé ici avec les prompts `carte`, `refus`,
`lent` et `réveil` (ticket 25 : `turnStart` d'un prompt et d'un tour de réveil, fin du réveil). Pour chaque paire de trames : chemins de clés (avec le type de la valeur) présents d'un
seul côté, types différents, valeurs des champs constants (statuts, genres, titres, identifiants
d'option, texte du refus, forme des identifiants de requête), trames de fin d'un tour de prompt.
Ticket 28 : aussi l'ouverture, capacités de la réponse d'`initialize` (dont la valeur de
`loadSession`) et `_meta` de la réponse de `session/new` (`saved`).
Ticket 31 (X3) : le cas C `C-resources` (écrans skills, MCP et réglages) rejoué contre l'agent
factice dans le même ordre : réponses de `_yus/settings/*`, `_yus/skills/*`, `_yus/mcp/list`, refus
de `permissions` (code et texte), option `permissions` après `set_config_option`, et la notice
`_yus/notice` (forme, place juste après `turnStart`, avant le contenu du tour).

Ticket 26 : `--against <dossier acp_frames d'un autre binaire>` compare deux binaires sans l'agent
factice : même suite de trames (sens, méthode, genre de mise à jour), puis, trame par trame, mêmes
chemins de clés et types, mêmes valeurs des champs constants. Chaque `--expect <chemin>` est un
chemin de clé ajouté attendu (présent dans `<dossier acp_frames>`, absent du dossier `--against`) :
il n'est pas compté comme écart, et il doit apparaître au moins une fois, sinon c'est un écart.

Usage :
    python3 -I bench/tools/capture/compare_frames.py <dossier acp_frames> [--fake <fake-agent.mjs>]
    python3 -I bench/tools/capture/compare_frames.py <dossier branche> --against <dossier main> \
      [--expect <chemin de clé>]...

Code de sortie 0 si aucun écart (`écarts : 0`), 1 sinon. Node requis (sauf avec `--against`).
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
_parser.add_argument("--against", type=Path, help="ticket 26 : dossier acp_frames d'un autre binaire (main)")
_parser.add_argument("--expect", action="append", default=[],
                     help="ticket 26, avec --against : chemin de clé ajouté attendu, ex. "
                          ".params.update.content[]._meta._yus/startColumn")
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
    # Ticket 28 : les réponses d'ouverture (capacités, `saved`), gardées à part.
    OPENING.setdefault(prompt, list(frames))
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


OPENING: dict[str, list[dict]] = {}

# Ticket 31 (X3) : la suite du cas C (`acp_frames.py`, `screens`), rejouée contre l'agent factice.
SKILL = "ya31-frames"


def fake_resources() -> dict:
    control = tempfile.mkdtemp(prefix="fake-acp-")
    (Path(control) / "scenario.json").write_text("{}")
    proc = subprocess.Popen(["node", str(fake)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, text=True, bufsize=1,
                            env={**os.environ, "FAKE_ACP_DIR": control}, cwd=control)
    lines: queue.Queue[str] = queue.Queue()
    threading.Thread(target=lambda: [lines.put(l) for l in proc.stdout], daemon=True).start()
    frames: list[dict] = []
    ids = iter(range(1, 100))

    def request(method: str, params: dict) -> dict:
        req_id = next(ids)
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}) + "\n")
        proc.stdin.flush()
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            try:
                message = json.loads(lines.get(timeout=0.2))
            except queue.Empty:
                continue
            frames.append(message)
            if message.get("id") == req_id and "method" not in message:
                return message
        return {}

    out: dict = {}
    request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
    sid = request("session/new", {"cwd": control, "mcpServers": []})["result"]["sessionId"]
    request("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": "bonjour"}]})
    out["_yus/settings/get"] = request("_yus/settings/get", {})
    out["_yus/settings/set"] = request("_yus/settings/set", {"key": "retry.enabled", "value": False})
    out["refus"] = request("_yus/settings/set", {"key": "permissions", "value": "manual"})
    out["_yus/skills/list"] = request("_yus/skills/list", {})
    out["_yus/skills/create"] = request("_yus/skills/create", {"name": SKILL, "description": "Skill du cas C (ticket 31)."})
    path = out["_yus/skills/create"].get("result", {}).get("created")
    out["_yus/skills/set"] = request("_yus/skills/set", {"path": path, "enabled": False})
    request("_yus/skills/set", {"path": path, "enabled": True})
    out["_yus/mcp/list"] = request("_yus/mcp/list", {})
    out["session/set_config_option"] = request("session/set_config_option",
                                               {"sessionId": sid, "configId": "permissions", "value": "manual"})
    start = len(frames)
    request("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": "bonjour"}]})
    out["prompt2"] = frames[start:]
    proc.stdin.close()
    proc.wait(timeout=5)
    return out


def real_resources() -> dict | None:
    folder = captured / "C-resources"
    if not folder.is_dir():
        return None
    rows = [json.loads(line) for line in (folder / "transcript.jsonl").read_text().splitlines()]
    asked = {row["msg"]["id"]: row["msg"] for row in rows
             if row["dir"] == "client->agent" and "method" in row["msg"] and "id" in row["msg"]}
    out: dict = {}
    prompts = 0
    for index, row in enumerate(rows):
        msg = row["msg"]
        if row["dir"] == "client->agent" and msg.get("method") == "session/prompt":
            prompts += 1
            if prompts == 2:
                out["prompt2"] = [r["msg"] for r in rows[index + 1:] if r["dir"] == "agent->client"]
        if row["dir"] != "agent->client" or "method" in msg or msg.get("id") not in asked:
            continue
        method, params = asked[msg["id"]]["method"], asked[msg["id"]].get("params") or {}
        if method == "_yus/settings/set" and params.get("key") == "permissions":
            out["refus"] = msg
        elif method == "_yus/skills/set" and params.get("enabled") is True:
            continue
        else:
            out.setdefault(method, msg)
    return out


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


def values_at(value, target: str, path: str = "") -> list:
    """Les valeurs au chemin de clé `target` (même notation que `schema`) ; ticket 28 : un objet
    ou une liste à ce chemin compte aussi (un écart attendu peut être un objet ajouté)."""
    if path == target:
        return [value]
    out = []
    if isinstance(value, dict):
        for key, item in value.items():
            out += values_at(item, target, f"{path}.{key}")
    elif isinstance(value, list):
        for item in value:
            out += values_at(item, target, f"{path}[]")
    return out


def against(branch: Path, main: Path, expected: list[str]) -> int:
    """Ticket 26 : deux binaires, trame par trame ; le nombre d'écarts."""
    def rows(folder: Path, case: str) -> list[dict]:
        return [json.loads(line) for line in (folder / case / "transcript.jsonl").read_text().splitlines()]

    def kind(row: dict) -> tuple:
        msg = row["msg"]
        u = (msg.get("params") or {}).get("update") or {}
        return (row["dir"], msg.get("method"), u.get("sessionUpdate"), "id" in msg, "result" in msg)

    total, seen = 0, {key: [] for key in expected}
    for case in ("A-display-auto", "B-basic-manual-reject", "C-resources"):
        if case == "C-resources" and not ((main / case).is_dir() and (branch / case).is_dir()):
            print(f"ABSENT : {case} (dossier d'un côté seulement, cas non comparé)")
            continue
        a, b = rows(main, case), rows(branch, case)
        if [kind(r) for r in a] != [kind(r) for r in b]:
            first = next((i for i, (x, y) in enumerate(zip(a, b)) if kind(x) != kind(y)), min(len(a), len(b)))
            print(f"DIFFÉRENT : {case}, suite des trames (main {len(a)}, branche {len(b)}), "
                  f"première différence à la trame {first}")
            total += 1
            continue
        lines, with_expected = [], 0
        for index, (x, y) in enumerate(zip(a, b)):
            sa, sb = schema(x["msg"]), schema(y["msg"])
            hit = False
            for key in sorted(set(sa) | set(sb)):
                if sa.get(key) == sb.get(key):
                    continue
                if key in seen and key not in sa:
                    seen[key] += values_at(y["msg"], key)
                    hit = True
                    continue
                side = "absent de la branche" if key not in sb else "en plus dans la branche" if key not in sa \
                    else f"type main {sa[key]}, branche {sb[key]}"
                lines.append(f"  trame {index} {kind(x)[1] or kind(x)[2]} : {key} ({side})")
            ca, cb = constants(x["msg"]), constants(y["msg"])
            for key in sorted(set(ca) & set(cb)):
                if ca[key] != cb[key]:
                    lines.append(f"  trame {index} : valeur {key} main {sorted(ca[key])}, branche {sorted(cb[key])}")
            with_expected += hit
        note = f" ; écart attendu dans {with_expected} trame(s)" if with_expected else ""
        print(f"{'IDENTIQUE' if not lines else 'DIFFÉRENT'} : {case} ({len(a)} trames{note})")
        for line in lines:
            print(line)
        total += len(lines)
    for key, values in seen.items():
        if values:
            print(f"écart attendu présent : {key} (valeurs {values})")
        else:
            print(f"écart attendu ABSENT : {key}")
            total += 1
    return total


if _args.against:
    _total = against(captured, _args.against.resolve(), _args.expect)
    print(f"\nécarts : {_total}")
    sys.exit(1 if _total else 0)
if _args.expect:
    sys.exit("--expect demande --against")

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
# Ticket 28 : ouverture. Capacités d'`initialize` (`loadSession`, `sessionCapabilities.list`) et
# `_meta` de la réponse de `session/new` (`saved { sessionId, path }`) ; les options de session
# (catalogue des modèles) ne sont pas comparées.
def response(frames, req_id):
    return next((f for f in frames if f.get("id") == req_id and "result" in f), {})
real_open = [row["msg"] for row in (json.loads(line) for line in
             (captured / "B-basic-manual-reject" / "transcript.jsonl").read_text().splitlines())
             if row["dir"] == "agent->client"]
mine_open = OPENING["carte"]
real_caps = response(real_open, 1).get("result", {}).get("agentCapabilities", {})
mine_caps = response(mine_open, 1).get("result", {}).get("agentCapabilities", {})
total += compare("initialize : agentCapabilities", real_caps, mine_caps)
load = (real_caps.get("loadSession"), mine_caps.get("loadSession"))
print(f"  loadSession réel {load[0]}, factice {load[1]} ({'IDENTIQUE' if load[0] == load[1] else 'DIFFÉRENT'})")
total += 0 if load[0] == load[1] else 1
total += compare("session/new : _meta", response(real_open, 2).get("result", {}).get("_meta", {}),
                 response(mine_open, 2).get("result", {}).get("_meta", {}))
# Message de l'agent.
total += compare("agent_message_chunk", update(A, "agent_message_chunk"), update(carte, "agent_message_chunk") or update(lent, "agent_message_chunk"),
                 ignore_values={".params.update.content.type"})
# Ticket 31 (X3) : cas C, écrans skills, MCP et réglages, et la notice.
REAL_C = real_resources()
if REAL_C is None:
    print("ABSENT : C-resources (capture d'avant le ticket 31, X3) : un écart")
    total += 1
else:
    MINE_C = fake_resources()
    names = {".result.skills[].name", ".result.skills[].path", ".result.servers[].label"}
    for method in ("_yus/settings/get", "_yus/settings/set", "_yus/skills/list", "_yus/skills/create",
                   "_yus/skills/set", "_yus/mcp/list"):
        total += compare(f"{method} : réponse", REAL_C.get(method, {}), MINE_C.get(method, {}), ignore_values=names)
    total += compare("_yus/settings/set permissions : refus", REAL_C.get("refus", {}), MINE_C.get("refus", {}))
    refusal = lambda msg: ((msg.get("error") or {}).get("code"), ((msg.get("error") or {}).get("data") or {}).get("details"))
    same = refusal(REAL_C.get("refus", {})) == refusal(MINE_C.get("refus", {}))
    print(f"  code et texte du refus {'identiques' if same else 'DIFFÉRENTS'} : réel {refusal(REAL_C.get('refus', {}))}")
    total += 0 if same else 1
    option = lambda msg: next((o for o in (msg.get("result") or {}).get("configOptions", []) if o.get("id") == "permissions"), {})
    real_option, mine_option = option(REAL_C.get("session/set_config_option", {})), option(MINE_C.get("session/set_config_option", {}))
    total += compare("session/set_config_option : option permissions", real_option, mine_option)
    same = real_option == mine_option
    print(f"  option permissions {'identique' if same else 'DIFFÉRENTE'} (valeur, noms des choix)")
    total += 0 if same else 1
    notice = lambda frames: next((f for f in frames if "_yus/notice" in meta((f.get("params") or {}).get("update") or {})), {})
    total += compare("session_info_update _yus/notice", notice(REAL_C.get("prompt2", [])), notice(MINE_C.get("prompt2", [])))
    details = lambda frames: meta((notice(frames).get("params") or {}).get("update") or {}).get("_yus/notice")
    same = details(REAL_C.get("prompt2", [])) == details(MINE_C.get("prompt2", []))
    print(f"  contenu de la notice {'identique' if same else 'DIFFÉRENT'} : réel {details(REAL_C.get('prompt2', []))}")
    total += 0 if same else 1

    def order(frames):
        out = []
        for f in frames:
            u = (f.get("params") or {}).get("update") or {}
            m = meta(u)
            if "_yus/notice" in m:
                out.append("notice")
            elif (m.get("ai.primeintellect.prime-agent") or {}).get("turnStart"):
                out.append("turnStart")
            elif u.get("sessionUpdate") == "agent_message_chunk":
                out.append("contenu")
        return out[:3]
    print(f"place de la notice : réel {order(REAL_C.get('prompt2', []))}, factice {order(MINE_C.get('prompt2', []))}"
          f" ({'IDENTIQUE' if order(REAL_C.get('prompt2', [])) == order(MINE_C.get('prompt2', [])) else 'DIFFÉRENT'})")
    total += 0 if order(REAL_C.get("prompt2", [])) == order(MINE_C.get("prompt2", [])) else 1
print(f"\nécarts : {total}")
sys.exit(1 if total else 0)
