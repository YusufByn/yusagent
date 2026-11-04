#!/usr/bin/env python3
"""Serveur de capture : enregistre chaque requête modèle et renvoie une réponse factice.

Formats servis (streaming SSE, ou JSON si `stream` est faux) :
  POST .../messages   Anthropic Messages
  POST .../responses  OpenAI Responses (et Codex, `.../codex/responses`)
  GET  .../responses  WebSocket Codex (T06, YA-14) : une trame texte `response.create`
                      par requête, les événements Responses en trames texte. Avec
                      `previous_response_id`, l'entrée reçue est le delta : la requête
                      est reconstruite depuis la précédente de la même connexion.

Chaque requête est écrite telle que reçue (octets bruts) dans `<out>/NNNN.body.json`,
avec ses métadonnées dans `<out>/NNNN.meta.json` (horodatage, chemin, en-têtes sans
valeur d'authentification, tour détecté, réponse renvoyée).

Le scénario est sans état : le tour est déduit du nombre de résultats de tool déjà
présents dans la requête, ce qui rend les reprises et les sessions multiples
déterministes. Une requête sans tools (résumé de compaction, titre de session...)
reçoit une réponse texte courte.

Usage renvoyé (T04, YA-13) : un modèle de cache déterministe (voir `UsageModel`),
avec lecture, écriture ventilée 5 min / 1 h et tokens de raisonnement, dans le
format de chaque API. L'usage exact renvoyé est noté dans `NNNN.meta.json`
(`reported_usage`). Avec `--usage-split`, le `message_delta` Anthropic ne porte que
`output_tokens` (et le détail de raisonnement) : le reste vient du `message_start`.
Avec `--prefix-cache` (T05, YA-05), la lecture du cache suit le préfixe réellement
commun avec la requête précédente de la conversation (voir `UsageModel`).

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import struct
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

# Valeurs d'en-têtes jamais écrites sur disque (CLAUDE.md règle 9).
REDACTED_HEADERS = {"authorization", "x-api-key", "api-key", "cookie", "proxy-authorization"}

# Code des cellules envoyées en tool call, par scénario et par tour.
# Le tour 2 du scénario de base liste les NOMS des variables d'environnement
# sensibles vues par le kernel (jamais leurs valeurs) : première ligne pour
# `os.environ`, puis `env` dans `bash()`, enfin la variable non sensible
# déclarée dans `kernel.envAllowlist` (YA-02). Le tour 3 vérifie que uv, git
# et python3 tournent toujours dans `bash()`. Le shell persistant est relevé
# dans `psenv` : sur macOS il lit son identité par `ps`, que le sandbox du
# banc interdit (setuid).
SENSITIVE_NAMES = (
    "import os\n"
    "def ya_sensitive(names):\n"
    "    return sorted(n for n in names if any(s in n.upper() for s in ('KEY', 'TOKEN', 'SECRET')))\n"
    "def ya_names(output):\n"
    "    return [line.split('=', 1)[0] for line in output.splitlines() if '=' in line]\n"
)
# Masquage (T05, YA-05) : 25 cellules d'environ 5 000 caractères, une erreur au tour 12.
# La cellule 25 est écrite à la volée (`reread_cell`) : elle relit depuis le kernel la
# première sortie masquée de la requête, ou n'imprime rien de plus sans masquage.
MASK_TURNS = 25
MASK_ERROR_TURN = 12
MASK_ERROR_MARK = "YA_MASK_ERROR"
REREAD = "<reread>"
PLACEHOLDER_CALL = re.compile(r'tool_output\("([^"]+)"\)')


def mask_cell(turn: int) -> str:
    if turn == MASK_ERROR_TURN:
        return f"print('e' * 5000)\nraise RuntimeError('{MASK_ERROR_MARK} tour {turn}')"
    if turn == MASK_TURNS:
        return REREAD
    return f"print('{turn:02d}' + 'x' * 4998)"


def reread_cell(body: dict) -> str:
    """Cellule 25 : relit la première sortie masquée (identifiant du placeholder)."""
    call_id = next((m.group(1) for text in strings(body) for m in [PLACEHOLDER_CALL.search(text)]
                    if m), None)
    tail = f"print('{MASK_TURNS}' + 'x' * 4998)"
    if call_id is None:
        return f"print('REREAD', None, 0, '')\n{tail}"
    return ("from rlm.masked import tool_output\n"
            f"_ya_text = tool_output({call_id!r})\n"
            f"print('REREAD', {call_id!r}, len(_ya_text), _ya_text[:2])\n{tail}")


def strings(value) -> list[str]:
    """Chaînes d'un corps de requête, dans l'ordre."""
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [s for item in value.values() for s in strings(item)]
    if isinstance(value, list):
        return [s for item in value for s in strings(item)]
    return []


# Sonde réseau du smoke bench (T06, YA-14) : noms des variables de proxy vues par le
# kernel et par `bash()` (jamais leurs valeurs), `curl` direct (doit échouer : réseau
# coupé), puis `curl` par le proxy CONNECT du banc (doit être refusé et journalisé :
# example.com n'est pas dans la liste). Le port du proxy est fixé au lancement
# (`--connect-proxy-port`), le kernel ne voit pas HTTPS_PROXY.
CONNECT_PROXY_PORT = "<connect-proxy-port>"
NET_PROBE = (
    "import os\n"
    "print('SMOKE_KERNEL_PROXY_VARS', sorted(n for n in os.environ if 'PROXY' in n.upper()))\n"
    "r = await bash('env')\n"
    "print('SMOKE_BASH_PROXY_VARS', sorted(line.split('=', 1)[0] for line in r.output.splitlines()"
    " if '=' in line and 'PROXY' in line.split('=', 1)[0].upper()))\n"
    "r = await bash('curl -sS -m 5 -o /dev/null https://example.com')\n"
    "print('SMOKE_CURL_DIRECT', r.exit_code)\n"
    f"r = await bash('curl -sS -m 5 -o /dev/null -x http://127.0.0.1:{CONNECT_PROXY_PORT}"
    " https://example.com')\n"
    "print('SMOKE_CURL_PROXY', r.exit_code)"
)

SCENARIOS: dict[str, list[str]] = {
    "basic": [
        "print(1)",
        SENSITIVE_NAMES
        + "print(ya_sensitive(os.environ))\n"
        "r = await bash('env')\n"
        "print('BASH', ya_sensitive(ya_names(r.output)))\n"
        "print('ALLOWED', os.environ.get('YA_CAPTURE_ALLOWED'))",
        "for c in ('uv --version', 'git status --short', \"python3 -c 'print(1)'\"):\n"
        "    r = await bash(c)\n"
        "    print('NONREG', repr(c), r.exit_code, r.output.strip()[:60])",
    ],
    # Sorties volumineuses (~40 000 caractères par cellule) : la session dépasse
    # keepRecentTokens (20 000 par défaut), la compaction a de quoi résumer.
    "long": ["print('x' * 40000)"] * 4,
    # Tour 2 tue le process du kernel ; tour 3 force son redémarrage.
    "crash": [
        "print(1)",
        "import os\nos._exit(1)",
        "print(2)",
    ],
    # Lancé hors sandbox (`ps` est setuid). Relevé pour YA-11, hors périmètre
    # YA-02 : depuis le kernel, `ps eww` sur le process parent (le harness)
    # montre-t-il son environnement ? Seuls les noms des variables factices
    # trouvées sont imprimés. Puis `printenv` dans un shell persistant (YA-02).
    "psenv": [
        SENSITIVE_NAMES
        + "r = await bash('ps eww -o command= -p ' + str(os.getppid()))\n"
        "words = r.output.split()\n"
        "print('PS_PARENT', os.path.basename(words[0]) if words else None, r.exit_code,"
        " sorted(n for n in ('ANTHROPIC_API_KEY', 'YA_CAPTURE_FAKE_TOKEN') if n + '=' in r.output))\n"
        "p = await bash('printenv', persistent=True)\n"
        "p = await p.handle if p.running else p\n"
        "print('PERSISTENT', p.exit_code, ya_sensitive(ya_names(p.output)),"
        " 'YA_CAPTURE_ALLOWED' in ya_names(p.output))",
    ],
    # Sous-agent : le parent lance un enfant puis attend son résultat. Les
    # requêtes de l'enfant arrivent sur le même serveur ; il est reconnu à
    # CHILD_MARKER dans son premier message utilisateur et joue CHILD_CELLS.
    "spawn": [
        "h = await rlm.spawn('YA_CHILD_TASK : réponds au parent.', name='kid')\n"
        "print('SPAWNED', h.name)",
        "r = await rlm.collect(['kid'], timeout_ms=120000)\n"
        "print('COLLECT', r[0].status, r[0].settled, r[0].answer_text)",
    ],
    "mask": [mask_cell(turn) for turn in range(1, MASK_TURNS + 1)],
    # Smoke bench (T06, YA-14) : chaque scénario résout sa tâche d'exemple en
    # quelques cellules, après la sonde réseau ; `smoke-fail` ne corrige rien.
    "smoke-probe": [NET_PROBE],
    "smoke-warmup": [],
    # Préparation du venv du kernel, hors sandbox : aucune sortie réseau dans la cellule.
    "smoke-kernel": ["print('SMOKE_KERNEL_READY', 1)"],
    "smoke-py-example-001": [
        "print(open('textstats/__init__.py').read())",
        NET_PROBE,
        "import pathlib\np = pathlib.Path('textstats/__init__.py')\n"
        "p.write_text(p.read_text().replace(\"text.split(' ')\", 'text.split()'))\n"
        "print('SMOKE_EDIT', 'text.split()' in p.read_text())",
    ],
    "smoke-node-example-001": [
        "print(open('src/slug.js').read())",
        NET_PROBE,
        "import pathlib\np = pathlib.Path('src/slug.js')\n"
        "p.write_text(p.read_text().replace(\"/[^a-z0-9]/g, '-')\","
        " \"/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '')\"))\n"
        "print('SMOKE_EDIT', '+/g' in p.read_text())",
    ],
    "smoke-fail": ["print('SMOKE_NOOP')", NET_PROBE],
}
CHILD_MARKER = "YA_CHILD_TASK"
# L'enfant relève aussi ce que voit son propre kernel (YA-02).
CHILD_CELLS = [
    SENSITIVE_NAMES
    + "print('CHILD_ENV', ya_sensitive(os.environ), os.environ.get('YA_CAPTURE_ALLOWED'))\n"
    "await agent_message.send('YA_CHILD_RESULT', receiver_role='parent')\nprint('SENT')",
]
CHILD_FINAL_TEXT = "YA_CHILD_DONE"
FINAL_TEXT = "Capture : fin du scénario."
AUX_TEXT = (
    "## Goal\n- capture\n\n## Constraints & Preferences\n- (none)\n\n## Progress\n### Done\n"
    "- [x] capture\n\n## Next Steps\n1. (none)"
)


def count_tool_results_anthropic(body: dict) -> int:
    count = 0
    for message in body.get("messages", []):
        content = message.get("content")
        if message.get("role") == "user" and isinstance(content, list):
            count += sum(1 for block in content if block.get("type") == "tool_result")
    return count


def count_tool_results_responses(body: dict) -> int:
    items = body.get("input")
    if not isinstance(items, list):
        return 0
    return sum(1 for item in items if item.get("type") == "function_call_output")


def first_user_text(body: dict) -> str:
    """Texte du premier message utilisateur (Anthropic `messages` ou Responses `input`)."""
    for message in body.get("messages") or body.get("input") or []:
        if not isinstance(message, dict) or message.get("role") != "user":
            continue
        content = message.get("content")
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            return "".join(block.get("text", "") for block in content
                           if isinstance(block, dict))
        return ""
    return ""


def first_tool_name(tools: list[dict]) -> str:
    tool = tools[0]
    # Responses : {"type": "function", "name": ...} ; Anthropic : {"name": ...}.
    return tool.get("name") or tool.get("function", {}).get("name", "python_repl")


def sse(event: str, data: dict) -> bytes:
    return f"event: {event}\ndata: {json.dumps(data)}\n\n".encode()


def sse_data(event: bytes) -> str:
    """JSON d'un événement SSE (ligne `data:`), envoyé tel quel en trame WebSocket."""
    return event.decode().split("\ndata: ", 1)[1].rstrip("\n")


def model_not_found(model: str) -> dict:
    """Erreur renvoyée pour un modèle refusé (`--reject-model`) : id faux simulé."""
    return {"error": {"type": "invalid_request_error", "code": "model_not_found",
                      "message": f"The requested model '{model}' does not exist."}}


# WebSocket (RFC 6455), le strict nécessaire pour le client Codex du harness : trames
# texte, fragmentation, ping et fermeture. Les trames du client sont masquées.
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def ws_read_frame(rfile) -> tuple[bool, int, bytes] | None:
    head = rfile.read(2)
    if len(head) < 2:
        return None
    fin, opcode = bool(head[0] & 0x80), head[0] & 0x0F
    masked, length = bool(head[1] & 0x80), head[1] & 0x7F
    if length == 126:
        length = struct.unpack(">H", rfile.read(2))[0]
    elif length == 127:
        length = struct.unpack(">Q", rfile.read(8))[0]
    mask = rfile.read(4) if masked else b""
    payload = rfile.read(length)
    if masked:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return fin, opcode, payload


def ws_send(wfile, opcode: int, payload: bytes) -> None:
    length = len(payload)
    if length < 126:
        head = struct.pack(">BB", 0x80 | opcode, length)
    elif length < 1 << 16:
        head = struct.pack(">BBH", 0x80 | opcode, 126, length)
    else:
        head = struct.pack(">BBQ", 0x80 | opcode, 127, length)
    wfile.write(head + payload)
    wfile.flush()


def ws_send_text(wfile, text: str) -> None:
    ws_send(wfile, 0x1, text.encode())


def ws_read_message(rfile, wfile) -> bytes | None:
    """Prochain message texte ou binaire complet ; None à la fermeture."""
    parts: list[bytes] = []
    while True:
        frame = ws_read_frame(rfile)
        if frame is None:
            return None
        fin, opcode, payload = frame
        if opcode == 0x8:
            try:
                ws_send(wfile, 0x8, payload[:2])
            except OSError:
                pass
            return None
        if opcode == 0x9:
            ws_send(wfile, 0xA, payload)
            continue
        if opcode == 0xA:
            continue
        parts.append(payload)
        if fin:
            return b"".join(parts)


class Plan:
    """Réponse à renvoyer : un tool call (code) ou un texte."""

    def __init__(self, kind: str, text: str = "", tool_name: str = "", code: str = ""):
        self.kind = kind
        self.text = text
        self.tool_name = tool_name
        self.code = code

    def describe(self) -> dict:
        if self.kind == "tool":
            return {"kind": "tool", "tool": self.tool_name, "code": self.code}
        return {"kind": self.kind, "text": self.text}


class CaptureState:
    def __init__(self, out_dir: Path, scenario: str, inflate_from_turn: int | None,
                 inflate_tokens: int, usage_split: bool = False, prefix_cache: bool = False,
                 connect_proxy_port: int | None = None, reject_models: tuple[str, ...] = ()):
        self.out_dir = out_dir
        self.usage = UsageModel(prefix_cache)
        self.usage_split = usage_split
        self.cells = [cell.replace(CONNECT_PROXY_PORT, str(connect_proxy_port or 9))
                      for cell in SCENARIOS[scenario]]
        self.reject_models = set(reject_models)
        self.no_websocket = False
        self.inflate_from_turn = inflate_from_turn
        self.inflate_tokens = inflate_tokens
        self.lock = threading.Lock()
        self.seq = 0

    def next_seq(self) -> int:
        with self.lock:
            self.seq += 1
            return self.seq

    def plan(self, tools: list[dict] | None, turn: int, child: bool = False,
             body: dict | None = None) -> Plan:
        if not tools:
            # Une requête sans tools (résumé) signale une compaction : on cesse
            # d'annoncer un usage gonflé pour ne pas la redéclencher en boucle.
            self.inflate_from_turn = None
            return Plan("aux", text=AUX_TEXT)
        cells, final = (CHILD_CELLS, CHILD_FINAL_TEXT) if child else (self.cells, FINAL_TEXT)
        if turn < len(cells):
            code = reread_cell(body or {}) if cells[turn] == REREAD else cells[turn]
            return Plan("tool", tool_name=first_tool_name(tools), code=code)
        return Plan("final", text=final)

    def input_tokens(self, raw_len: int, turn: int) -> int:
        if self.inflate_from_turn is not None and turn >= self.inflate_from_turn:
            return self.inflate_tokens
        # Estimation grossière (octets / 4) : seul l'ordre de grandeur compte ici.
        return max(1, raw_len // 4)


# Tokens de sortie annoncés, dont tokens de raisonnement (sous-ensemble, comme
# `output_tokens_details` chez Anthropic et OpenAI).
OUTPUT_TOKENS = 8
REASONING_TOKENS = 3


class UsageModel:
    """Cache simulé, déterministe pour une suite de requêtes donnée.

    Une conversation est identifiée par son prompt système et son premier message
    utilisateur. Sa première requête écrit 80 % du prompt en cache ; les suivantes
    lisent 90 % du prompt de la requête précédente (borné par le prompt courant) et
    écrivent 80 % du reste. Le TTL d'écriture suit la requête : 1 h si un
    `cache_control` demande `"ttl": "1h"`, 5 min sinon.

    Avec `prefix_cache` (T05, YA-05) : la lecture vaut le préfixe commun avec la requête
    précédente de la conversation, dans l'ordre du cache (tools, système, messages, hors
    `cache_control`), en octets / 4 ; le reste du prompt est écrit. Une requête qui change
    un ancien message relit moins et écrit d'autant plus. Simulation, pas un tokenizer.
    """

    def __init__(self, prefix_cache: bool = False):
        self.lock = threading.Lock()
        self.previous: dict[str, int] = {}
        self.prefix_cache = prefix_cache
        self.previous_units: dict[str, list[str]] = {}

    def split(self, body: dict, total: int) -> dict:
        key = json.dumps([body.get("system") or body.get("instructions"), first_user_text(body)],
                         sort_keys=True, ensure_ascii=False)
        if self.prefix_cache:
            units = prefix_units(body)
            with self.lock:
                previous_units = self.previous_units.get(key)
                self.previous_units[key] = units
            read = min(total, common_prefix_bytes(previous_units, units) // 4) if previous_units else 0
            return {"total": total, "read": read, "write": total - read, "input": 0,
                    "ttl_1h": asks_one_hour_ttl(body)}
        with self.lock:
            previous = self.previous.get(key)
            self.previous[key] = total
        read = min(total, previous * 9 // 10) if previous else 0
        write = (total - read) * 8 // 10
        return {"total": total, "read": read, "write": write, "input": total - read - write,
                "ttl_1h": asks_one_hour_ttl(body)}


def compact(value) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def strip_cache_control(value):
    if isinstance(value, dict):
        return {k: strip_cache_control(v) for k, v in value.items() if k != "cache_control"}
    if isinstance(value, list):
        return [strip_cache_control(v) for v in value]
    return value


def prefix_units(body: dict) -> list[str]:
    """Morceaux du prompt dans l'ordre du cache : tools, système, puis chaque message."""
    conversation = body.get("messages") if "messages" in body else body.get("input")
    head = [body.get("tools"), body.get("system") or body.get("instructions")]
    return [compact(strip_cache_control(item)) for item in head + list(conversation or [])]


def common_prefix_bytes(a: list[str], b: list[str]) -> int:
    total = 0
    for x, y in zip(a, b):
        if x == y:
            total += len(x.encode())
            continue
        same = 0
        while same < min(len(x), len(y)) and x[same] == y[same]:
            same += 1
        return total + len(x[:same].encode())
    return total


def asks_one_hour_ttl(value) -> bool:
    """Un `cache_control` de la requête demande-t-il le TTL d'une heure ?"""
    if isinstance(value, dict):
        control = value.get("cache_control")
        if isinstance(control, dict) and control.get("ttl") == "1h":
            return True
        return any(asks_one_hour_ttl(item) for item in value.values())
    if isinstance(value, list):
        return any(asks_one_hour_ttl(item) for item in value)
    return False


def anthropic_usage(split: dict, output_tokens: int) -> dict:
    """Usage Anthropic : `input_tokens` hors cache, écriture ventilée par TTL."""
    one_hour = split["write"] if split["ttl_1h"] else 0
    return {
        "input_tokens": split["input"],
        "cache_creation_input_tokens": split["write"],
        "cache_read_input_tokens": split["read"],
        "cache_creation": {"ephemeral_5m_input_tokens": split["write"] - one_hour,
                           "ephemeral_1h_input_tokens": one_hour},
        "output_tokens": output_tokens,
    }


def responses_usage(split: dict) -> dict:
    """Usage OpenAI Responses : `input_tokens` inclut `cached_tokens`, pas d'écriture."""
    return {"input_tokens": split["total"], "output_tokens": OUTPUT_TOKENS,
            "total_tokens": split["total"] + OUTPUT_TOKENS,
            "input_tokens_details": {"cached_tokens": split["read"]},
            "output_tokens_details": {"reasoning_tokens": REASONING_TOKENS}}


def anthropic_events(plan: Plan, model: str, seq: int, split: dict,
                     usage_split: bool) -> tuple[list[bytes], dict]:
    usage = anthropic_usage(split, 1)
    # Comme l'API réelle, message_delta porte l'usage cumulé ; le détail de
    # raisonnement n'arrive que dans le message_delta final.
    final = dict(anthropic_usage(split, OUTPUT_TOKENS),
                 output_tokens_details={"thinking_tokens": REASONING_TOKENS})
    delta_usage = ({"output_tokens": OUTPUT_TOKENS,
                    "output_tokens_details": {"thinking_tokens": REASONING_TOKENS}}
                   if usage_split else final)
    events = [sse("message_start", {
        "type": "message_start",
        "message": {"id": f"msg_cap_{seq}", "type": "message", "role": "assistant",
                    "model": model, "content": [], "stop_reason": None,
                    "stop_sequence": None, "usage": usage},
    })]
    if plan.kind == "tool":
        events += [
            sse("content_block_start", {
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "tool_use", "id": f"toolu_cap_{seq}",
                                  "name": plan.tool_name, "input": {}},
            }),
            sse("content_block_delta", {
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta",
                          "partial_json": json.dumps({"code": plan.code})},
            }),
        ]
        stop_reason = "tool_use"
    else:
        events += [
            sse("content_block_start", {
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""},
            }),
            sse("content_block_delta", {
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": plan.text},
            }),
        ]
        stop_reason = "end_turn"
    events += [
        sse("content_block_stop", {"type": "content_block_stop", "index": 0}),
        sse("message_delta", {
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason, "stop_sequence": None},
            "usage": delta_usage,
        }),
        sse("message_stop", {"type": "message_stop"}),
    ]
    return events, {"message_start": usage, "message_delta": delta_usage}


def anthropic_json(plan: Plan, model: str, seq: int, split: dict) -> dict:
    if plan.kind == "tool":
        content = [{"type": "tool_use", "id": f"toolu_cap_{seq}", "name": plan.tool_name,
                    "input": {"code": plan.code}}]
        stop_reason = "tool_use"
    else:
        content = [{"type": "text", "text": plan.text}]
        stop_reason = "end_turn"
    return {"id": f"msg_cap_{seq}", "type": "message", "role": "assistant", "model": model,
            "content": content, "stop_reason": stop_reason, "stop_sequence": None,
            "usage": dict(anthropic_usage(split, OUTPUT_TOKENS),
                          output_tokens_details={"thinking_tokens": REASONING_TOKENS})}


def responses_item(plan: Plan, seq: int, done: bool) -> dict:
    status = "completed" if done else "in_progress"
    if plan.kind == "tool":
        return {"type": "function_call", "id": f"fc_cap_{seq}", "call_id": f"call_cap_{seq}",
                "name": plan.tool_name,
                "arguments": json.dumps({"code": plan.code}) if done else "",
                "status": status}
    content = [{"type": "output_text", "text": plan.text, "annotations": []}] if done else []
    return {"type": "message", "id": f"msg_cap_{seq}", "role": "assistant", "status": status,
            "content": content}


def responses_object(plan: Plan, model: str, seq: int, split: dict | None, done: bool) -> dict:
    obj = {"id": f"resp_cap_{seq}", "object": "response", "model": model,
           "status": "completed" if done else "in_progress",
           "output": [responses_item(plan, seq, done=True)] if done else []}
    if done:
        obj["usage"] = responses_usage(split)
    return obj


def responses_events(plan: Plan, model: str, seq: int, split: dict) -> list[bytes]:
    item_id = f"fc_cap_{seq}" if plan.kind == "tool" else f"msg_cap_{seq}"
    events = [
        sse("response.created", {"type": "response.created",
                                 "response": responses_object(plan, model, seq, None,
                                                              done=False)}),
        sse("response.output_item.added", {"type": "response.output_item.added",
                                           "output_index": 0,
                                           "item": responses_item(plan, seq, done=False)}),
    ]
    if plan.kind == "tool":
        arguments = json.dumps({"code": plan.code})
        events += [
            sse("response.function_call_arguments.delta", {
                "type": "response.function_call_arguments.delta", "item_id": item_id,
                "output_index": 0, "delta": arguments}),
            sse("response.function_call_arguments.done", {
                "type": "response.function_call_arguments.done", "item_id": item_id,
                "output_index": 0, "arguments": arguments}),
        ]
    else:
        part = {"type": "output_text", "text": "", "annotations": []}
        events += [
            sse("response.content_part.added", {
                "type": "response.content_part.added", "item_id": item_id, "output_index": 0,
                "content_index": 0, "part": part}),
            sse("response.output_text.delta", {
                "type": "response.output_text.delta", "item_id": item_id, "output_index": 0,
                "content_index": 0, "delta": plan.text}),
            sse("response.output_text.done", {
                "type": "response.output_text.done", "item_id": item_id, "output_index": 0,
                "content_index": 0, "text": plan.text}),
            sse("response.content_part.done", {
                "type": "response.content_part.done", "item_id": item_id, "output_index": 0,
                "content_index": 0, "part": dict(part, text=plan.text)}),
        ]
    events += [
        sse("response.output_item.done", {"type": "response.output_item.done",
                                          "output_index": 0,
                                          "item": responses_item(plan, seq, done=True)}),
        sse("response.completed", {"type": "response.completed",
                                   "response": responses_object(plan, model, seq, split,
                                                                done=True)}),
    ]
    return events


def make_handler(state: CaptureState):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt: str, *args) -> None:  # silence stderr
            pass

        def do_GET(self) -> None:
            path = self.path.split("?", 1)[0].rstrip("/")
            if (self.headers.get("Upgrade", "").lower() == "websocket"
                    and path.endswith("/responses") and not state.no_websocket):
                self.serve_websocket()
            elif self.path.rstrip("/").endswith("/health"):
                self.reply_json(200, {"ok": True})
            else:
                self.reply_json(404, {"error": {"type": "not_found", "message": self.path}})

        def do_POST(self) -> None:
            raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            seq = state.next_seq()
            path = self.path.split("?", 1)[0].rstrip("/")
            try:
                body = json.loads(raw)
            except json.JSONDecodeError:
                body = None
            if path.endswith("/messages"):
                api = "anthropic-messages"
            elif path.endswith("/responses"):
                api = "openai-responses"
            else:
                api = "unknown"
            meta = {
                "seq": seq,
                "ts": datetime.now(timezone.utc).isoformat(),
                "path": self.path,
                "api": api,
                "transport": "http",
                "headers": {k: ("<redacted>" if k.lower() in REDACTED_HEADERS else v)
                            for k, v in self.headers.items()},
                "bytes": len(raw),
            }
            (state.out_dir / f"{seq:04d}.body.json").write_bytes(raw)
            if body is None or api == "unknown":
                meta["response"] = {"kind": "error"}
                self.write_meta(seq, meta)
                self.reply_json(404, {"error": {"type": "not_found", "message": self.path}})
                return
            if body.get("model") in state.reject_models:
                meta["response"] = {"kind": "model_not_found"}
                self.write_meta(seq, meta)
                self.reply_json(400, model_not_found(body["model"]))
                return
            if api == "anthropic-messages":
                turn = count_tool_results_anthropic(body)
            else:
                turn = count_tool_results_responses(body)
            child = CHILD_MARKER in first_user_text(body)
            plan = state.plan(body.get("tools"), turn, child, body)
            input_tokens = state.input_tokens(len(raw), turn)
            split = state.usage.split(body, input_tokens)
            model = body.get("model", "capture-model")
            if not body.get("stream"):
                reply = (anthropic_json(plan, model, seq, split) if api == "anthropic-messages"
                         else responses_object(plan, model, seq, split, done=True))
                meta.update({"turn": turn, "child": child, "response": plan.describe(),
                             "reported_input_tokens": input_tokens,
                             "reported_usage": {"json": reply["usage"]}})
                self.write_meta(seq, meta)
                self.reply_json(200, reply)
                return
            if api == "anthropic-messages":
                events, reported = anthropic_events(plan, model, seq, split, state.usage_split)
            else:
                events = responses_events(plan, model, seq, split)
                reported = {"response.completed": responses_usage(split)}
            meta.update({"turn": turn, "child": child, "response": plan.describe(),
                         "reported_input_tokens": input_tokens, "reported_usage": reported})
            self.write_meta(seq, meta)
            payload = b"".join(events)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def serve_websocket(self) -> None:
            """WebSocket Codex : une requête par trame `response.create` (T06, YA-14)."""
            key = self.headers.get("Sec-WebSocket-Key", "")
            accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
            self.send_response(101, "Switching Protocols")
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()
            self.wfile.flush()
            self.close_connection = True
            # Dernière requête complète de la connexion, et sortie renvoyée : la base
            # d'une continuation (`previous_response_id`).
            last = {"id": None, "input": [], "output": []}
            while True:
                raw = ws_read_message(self.rfile, self.wfile)
                if raw is None:
                    return
                seq = state.next_seq()
                (state.out_dir / f"{seq:04d}.body.json").write_bytes(raw)
                body = json.loads(raw)
                body.pop("type", None)
                previous = body.pop("previous_response_id", None)
                meta = {"seq": seq, "ts": datetime.now(timezone.utc).isoformat(),
                        "path": self.path, "api": "openai-responses", "transport": "websocket",
                        "continuation": previous is not None,
                        "headers": {k: ("<redacted>" if k.lower() in REDACTED_HEADERS else v)
                                    for k, v in self.headers.items()},
                        "bytes": len(raw)}
                if previous is not None:
                    if previous != last["id"]:
                        meta["response"] = {"kind": "unknown_previous_response"}
                        self.write_meta(seq, meta)
                        ws_send_text(self.wfile, json.dumps({
                            "type": "error", "status": 400,
                            "error": {"type": "invalid_request_error",
                                      "code": "previous_response_not_found",
                                      "message": f"Previous response {previous} not found."}}))
                        continue
                    body["input"] = last["input"] + last["output"] + list(body.get("input") or [])
                if body.get("model") in state.reject_models:
                    meta["response"] = {"kind": "model_not_found"}
                    self.write_meta(seq, meta)
                    ws_send_text(self.wfile, json.dumps(dict(model_not_found(body["model"]),
                                                             type="error", status=400)))
                    continue
                turn = count_tool_results_responses(body)
                child = CHILD_MARKER in first_user_text(body)
                plan = state.plan(body.get("tools"), turn, child, body)
                full = json.dumps(body).encode()
                input_tokens = state.input_tokens(len(full), turn)
                split = state.usage.split(body, input_tokens)
                model = body.get("model", "capture-model")
                events = responses_events(plan, model, seq, split)
                meta.update({"turn": turn, "child": child, "response": plan.describe(),
                             "reported_input_tokens": input_tokens,
                             "reported_usage": {"response.completed": responses_usage(split)}})
                self.write_meta(seq, meta)
                for event in events:
                    ws_send_text(self.wfile, sse_data(event))
                last.update(id=f"resp_cap_{seq}", input=list(body.get("input") or []),
                            output=[responses_item(plan, seq, done=True)])

        def write_meta(self, seq: int, meta: dict) -> None:
            (state.out_dir / f"{seq:04d}.meta.json").write_text(
                json.dumps(meta, indent=2, ensure_ascii=False) + "\n")

        def reply_json(self, status: int, obj: dict) -> None:
            data = json.dumps(obj).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    return Handler


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=Path, help="dossier des captures")
    parser.add_argument("--port", type=int, default=0, help="0 = port libre choisi par l'OS")
    parser.add_argument("--scenario", choices=sorted(SCENARIOS), default="basic")
    parser.add_argument("--inflate-from-turn", type=int, default=None,
                        help="à partir de ce tour, l'usage renvoyé annonce --inflate-tokens"
                             " tokens d'entrée (pour déclencher la compaction)")
    parser.add_argument("--inflate-tokens", type=int, default=190_000)
    parser.add_argument("--port-file", type=Path, help="écrit le port effectif dans ce fichier")
    parser.add_argument("--usage-split", action="store_true",
                        help="Anthropic : message_delta ne porte que output_tokens (T04)")
    parser.add_argument("--prefix-cache", action="store_true",
                        help="lecture du cache = préfixe commun avec la requête précédente (T05)")
    parser.add_argument("--connect-proxy-port", type=int, default=None,
                        help="port du proxy CONNECT du smoke bench, pour la sonde réseau (T06)")
    parser.add_argument("--reject-model", action="append", default=[],
                        help="répond « modèle inconnu » pour cet id (T06, id faux simulé)")
    parser.add_argument("--no-websocket", action="store_true",
                        help="refuse l'upgrade WebSocket (404) : force le repli SSE (T06)")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    state = CaptureState(args.out, args.scenario, args.inflate_from_turn, args.inflate_tokens,
                         args.usage_split, args.prefix_cache, args.connect_proxy_port,
                         tuple(args.reject_model))
    state.no_websocket = args.no_websocket
    server = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(state))
    port = server.server_address[1]
    if args.port_file:
        args.port_file.write_text(f"{port}\n")
    print(f"capture: http://127.0.0.1:{port} -> {args.out}", flush=True)
    try:
        server.serve_forever(poll_interval=0.2)
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
