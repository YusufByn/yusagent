#!/usr/bin/env python3
"""Serveur de capture : enregistre chaque requête modèle et renvoie une réponse factice.

Formats servis (streaming SSE, ou JSON si `stream` est faux) :
  POST .../messages   Anthropic Messages
  POST .../responses  OpenAI Responses

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

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
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
                 inflate_tokens: int, usage_split: bool = False):
        self.out_dir = out_dir
        self.usage = UsageModel()
        self.usage_split = usage_split
        self.cells = SCENARIOS[scenario]
        self.inflate_from_turn = inflate_from_turn
        self.inflate_tokens = inflate_tokens
        self.lock = threading.Lock()
        self.seq = 0

    def next_seq(self) -> int:
        with self.lock:
            self.seq += 1
            return self.seq

    def plan(self, tools: list[dict] | None, turn: int, child: bool = False) -> Plan:
        if not tools:
            # Une requête sans tools (résumé) signale une compaction : on cesse
            # d'annoncer un usage gonflé pour ne pas la redéclencher en boucle.
            self.inflate_from_turn = None
            return Plan("aux", text=AUX_TEXT)
        cells, final = (CHILD_CELLS, CHILD_FINAL_TEXT) if child else (self.cells, FINAL_TEXT)
        if turn < len(cells):
            return Plan("tool", tool_name=first_tool_name(tools), code=cells[turn])
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
    """

    def __init__(self):
        self.lock = threading.Lock()
        self.previous: dict[str, int] = {}

    def split(self, body: dict, total: int) -> dict:
        key = json.dumps([body.get("system") or body.get("instructions"), first_user_text(body)],
                         sort_keys=True, ensure_ascii=False)
        with self.lock:
            previous = self.previous.get(key)
            self.previous[key] = total
        read = min(total, previous * 9 // 10) if previous else 0
        write = (total - read) * 8 // 10
        return {"total": total, "read": read, "write": write, "input": total - read - write,
                "ttl_1h": asks_one_hour_ttl(body)}


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
            if self.path.rstrip("/").endswith("/health"):
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
            if api == "anthropic-messages":
                turn = count_tool_results_anthropic(body)
            else:
                turn = count_tool_results_responses(body)
            child = CHILD_MARKER in first_user_text(body)
            plan = state.plan(body.get("tools"), turn, child)
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
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    state = CaptureState(args.out, args.scenario, args.inflate_from_turn, args.inflate_tokens,
                         args.usage_split)
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
