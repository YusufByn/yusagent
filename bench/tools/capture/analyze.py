#!/usr/bin/env python3
"""Analyse des captures de run.py : préambule, préfixe N/N+1, variations entre sessions.

Usage : python3 analyze.py <dossier de run>   (écrit report.md et report.json)

Les tokens sont estimés avec tiktoken o200k_base quand il est importable : c'est
une ESTIMATION (aucun des providers capturés n'utilise ce tokenizer pour
facturer). Sans tiktoken, la colonne tokens reste vide.
"""

from __future__ import annotations

import difflib
import json
import sys
from pathlib import Path

try:
    import tiktoken

    ENCODER = tiktoken.get_encoding("o200k_base")
except Exception:  # tokenizer absent : on ne fabrique pas de chiffre
    ENCODER = None

VOLATILE_MARKERS = {
    "Current date:": "date du jour",
    "Working directory:": "cwd",
    "Conversation log:": "chemin du journal de session (contient l'id de session)",
}


def tokens(text: str) -> int | None:
    return len(ENCODER.encode(text)) if ENCODER else None


def compact(value) -> str:
    """Sérialisation compacte, ordre des clés conservé (comme serde preserve_order)."""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def strip_cache_control(value):
    if isinstance(value, dict):
        return {k: strip_cache_control(v) for k, v in value.items() if k != "cache_control"}
    if isinstance(value, list):
        return [strip_cache_control(v) for v in value]
    return value


class Request:
    def __init__(self, body_path: Path):
        self.raw = body_path.read_bytes()
        self.meta = json.loads(body_path.with_name(body_path.name.replace(".body", ".meta"))
                               .read_text())
        self.body = json.loads(self.raw)
        self.api = self.meta["api"]
        self.seq = self.meta["seq"]

    @property
    def has_tools(self) -> bool:
        return bool(self.body.get("tools"))

    def system_text(self) -> str:
        if self.api == "anthropic-messages":
            system = self.body.get("system") or []
            if isinstance(system, str):
                return system
            return "\n".join(block.get("text", "") for block in system)
        if self.body.get("instructions"):
            return self.body["instructions"]
        parts = []
        for item in self.messages():
            if item.get("role") in ("system", "developer"):
                content = item.get("content")
                parts.append(content if isinstance(content, str)
                             else "".join(c.get("text", "") for c in content))
        return "\n".join(parts)

    def system_value(self):
        if self.api == "anthropic-messages":
            return self.body.get("system")
        return [m for m in self.messages() if m.get("role") in ("system", "developer")]

    def messages(self) -> list:
        key = "messages" if self.api == "anthropic-messages" else "input"
        value = self.body.get(key)
        return value if isinstance(value, list) else []

    def conversation(self) -> list:
        """Messages hors prompt système (pour Responses, le système est dans input)."""
        if self.api == "anthropic-messages":
            return self.messages()
        return [m for m in self.messages() if m.get("role") not in ("system", "developer")]

    def params(self) -> dict:
        skip = {"messages", "input", "system", "tools", "instructions"}
        return {k: v for k, v in self.body.items() if k not in skip}

    def reserialization_exact(self) -> bool:
        return compact(self.body).encode() == self.raw


def load_case(case_dir: Path) -> list[Request]:
    return [Request(p) for p in sorted((case_dir / "cap").glob("*.body.json"))]


def first_diff(a: str, b: str) -> int:
    limit = min(len(a), len(b))
    for index in range(limit):
        if a[index] != b[index]:
            return index
    return limit


def describe_message_diff(a, b) -> dict:
    ca, cb = compact(a), compact(b)
    if compact(strip_cache_control(a)) == compact(strip_cache_control(b)):
        return {"cause": "déplacement de cache_control (contenu identique sans le marqueur)"}
    offset = first_diff(ca, cb)
    return {"cause": "contenu modifié", "offset": offset,
            "avant": ca[max(0, offset - 80):offset + 160],
            "après": cb[max(0, offset - 80):offset + 160]}


def compare_pair(a: Request, b: Request) -> dict:
    result = {
        "pair": f"{a.seq:04d} -> {b.seq:04d}",
        "system_identique": compact(a.system_value()) == compact(b.system_value()),
        "tools_identiques": compact(a.body.get("tools")) == compact(b.body.get("tools")),
        "params_identiques": compact(a.params()) == compact(b.params()),
        "messages_n": len(a.conversation()),
        "messages_n1": len(b.conversation()),
        "diffs_messages": [],
    }
    if not result["params_identiques"]:
        result["params_diff"] = {k: [a.params().get(k), b.params().get(k)]
                                 for k in set(a.params()) | set(b.params())
                                 if a.params().get(k) != b.params().get(k)}
    if not result["system_identique"]:
        result["system_diff"] = system_line_diff(a.system_text(), b.system_text())
    for index, (ma, mb) in enumerate(zip(a.conversation(), b.conversation())):
        if compact(ma) != compact(mb):
            result["diffs_messages"].append({"index": index, **describe_message_diff(ma, mb)})
    shorter = len(b.conversation()) < len(a.conversation())
    result["prefixe_exact"] = (result["system_identique"] and result["tools_identiques"]
                               and not result["diffs_messages"] and not shorter)
    result["prefixe_exact_hors_cache_control"] = (
        result["system_identique"] and result["tools_identiques"] and not shorter
        and all(d["cause"].startswith("déplacement") for d in result["diffs_messages"]))
    return result


def system_line_diff(a: str, b: str) -> list[dict]:
    changes = []
    for line in difflib.unified_diff(a.splitlines(), b.splitlines(), lineterm="", n=0):
        if line.startswith(("---", "+++", "@@")):
            continue
        cause = next((label for marker, label in VOLATILE_MARKERS.items() if marker in line),
                     "autre")
        changes.append({"ligne": line, "cause": cause})
    return changes


def preamble(req: Request) -> dict:
    system = req.system_text()
    tools_json = compact(req.body.get("tools"))
    conv = req.conversation()
    digest = next((m for m in conv if "[harness-digest]" in compact(m)), None)
    digest_text = ""
    if digest is not None:
        content = digest.get("content")
        digest_text = content if isinstance(content, str) else "".join(
            c.get("text", "") for c in content if isinstance(c, dict))
    breakpoints = compact(req.body).count('"cache_control"')
    return {
        "request": req.seq,
        "api": req.api,
        "body_bytes": len(req.raw),
        "system_chars": len(system),
        "system_tokens_est": tokens(system),
        "tools": [t.get("name") for t in req.body.get("tools") or []],
        "tools_json_chars": len(tools_json),
        "tools_tokens_est": tokens(tools_json),
        "digest_chars": len(digest_text),
        "digest_tokens_est": tokens(digest_text),
        "conversation_messages": len(conv),
        "cache_control_count": breakpoints,
        "params": req.params(),
        "reserialisation_octet_identique": req.reserialization_exact(),
    }


def main_requests(reqs: list[Request]) -> list[Request]:
    return [r for r in reqs if r.has_tools]


def analyze_case(name: str, reqs: list[Request]) -> dict:
    mains = main_requests(reqs)
    return {
        "requests": [{"seq": r.seq, "api": r.api, "turn": r.meta.get("turn"),
                      "tools": r.has_tools, "bytes": len(r.raw),
                      "response": r.meta.get("response", {}).get("kind")} for r in reqs],
        "pairs": [compare_pair(a, b) for a, b in zip(mains, mains[1:])],
        "preamble": preamble(mains[0]) if mains else None,
    }


def session_compare(a: Request, b: Request) -> dict:
    digest_a = [compact(m) for m in a.conversation()[:-1]]
    digest_b = [compact(m) for m in b.conversation()[:-1]]
    return {
        "system_identique": compact(a.system_value()) == compact(b.system_value()),
        "system_diff": system_line_diff(a.system_text(), b.system_text()),
        "tools_identiques": compact(a.body.get("tools")) == compact(b.body.get("tools")),
        "params_a": a.params(),
        "params_b": b.params(),
        "messages_avant_prompt_identiques": digest_a == digest_b,
    }


def tool_results_text(req: Request) -> list[str]:
    texts = []
    for message in req.conversation():
        content = message.get("content")
        if isinstance(content, list):
            for block in content:
                if block.get("type") == "tool_result":
                    texts.append(compact(block.get("content")))
        if message.get("type") == "function_call_output":
            texts.append(compact(message.get("output")))
    return texts


def fmt(value) -> str:
    return "n/a" if value is None else str(value)


def render(report: dict) -> str:
    out = ["# Rapport de capture", ""]
    info = report["run"]
    out += [f"- Binaire : `{info['binary_version']}` sha256 `{info['binary_sha256'][:16]}…`",
            f"- Commit du dépôt : `{info['repo_commit']}`",
            f"- Date (UTC) : {info['date_utc']}",
            f"- Tokenizer : {report['tokenizer']}",
            f"- Contrôle réseau sandbox : `{info['network_control']['command']}` → exit "
            f"{info['network_control']['exit']} ({info['network_control']['stderr']})", ""]
    out += ["## Cas", "", "| Cas | API | Scénario | Exit | Durée (s) | Requêtes (avec tools / total) |",
            "|---|---|---|---|---|---|"]
    for name, case in report["cases"].items():
        meta = info["cases"][name]
        reqs = case["requests"]
        out.append(f"| {name} | {meta['api']} | {meta['scenario']} | {meta['exit']} | "
                   f"{meta['seconds']} | {sum(r['tools'] for r in reqs)} / {len(reqs)} |")
    out += ["", "## Préambule (premier appel de chaque cas)", "",
            "| Cas | Corps (octets) | Système (car.) | Système (tokens est.) | Tools | "
            "Tools JSON (car.) | Tools (tokens est.) | Digest (car.) | Digest (tokens est.) | "
            "cache_control | Re-sérialisation identique |",
            "|---|---|---|---|---|---|---|---|---|---|---|"]
    for name, case in report["cases"].items():
        p = case["preamble"]
        if p:
            out.append(f"| {name} | {p['body_bytes']} | {p['system_chars']} | "
                       f"{fmt(p['system_tokens_est'])} | {', '.join(p['tools'])} | "
                       f"{p['tools_json_chars']} | {fmt(p['tools_tokens_est'])} | "
                       f"{p['digest_chars']} | {fmt(p['digest_tokens_est'])} | "
                       f"{p['cache_control_count']} | {p['reserialisation_octet_identique']} |")
    if report.get("layers"):
        out += ["", "## Couches du prompt système (`prime-agent prompt --json`)", "",
                "| Couche | Type | Source | Octets | Tokens est. |", "|---|---|---|---|---|"]
        for layer in report["layers"]:
            out.append(f"| {layer['name']} | {layer['kind']} | {layer['source']} | "
                       f"{layer['bytes']} | {fmt(layer['tokens_est'])} |")
    out += ["", "## Préfixe N → N+1", ""]
    for name, case in report["cases"].items():
        out += [f"### {name}", "", "| Paire | Système = | Tools = | Params = | Messages N/N+1 | "
                "Préfixe exact | Exact hors cache_control |", "|---|---|---|---|---|---|---|"]
        for pair in case["pairs"]:
            out.append(f"| {pair['pair']} | {pair['system_identique']} | "
                       f"{pair['tools_identiques']} | {pair['params_identiques']} | "
                       f"{pair['messages_n']}/{pair['messages_n1']} | {pair['prefixe_exact']} | "
                       f"{pair['prefixe_exact_hors_cache_control']} |")
        for pair in case["pairs"]:
            for diff in pair["diffs_messages"]:
                out.append(f"- {pair['pair']} message {diff['index']} : {diff['cause']}")
                if "avant" in diff:
                    out += ["  ```", f"  avant : {diff['avant']}", f"  après : {diff['après']}",
                            "  ```"]
            for change in pair.get("system_diff", []):
                out.append(f"- {pair['pair']} système ({change['cause']}) : `{change['ligne'][:200]}`")
            if pair.get("params_diff"):
                out.append(f"- {pair['pair']} params : `{compact(pair['params_diff'])[:300]}`")
        out.append("")
    sc = report["sessions"]
    out += ["## Session 1 vs session 2 (même projet, même dossier agent)", "",
            f"- Système identique : {sc['system_identique']}",
            f"- Tools identiques : {sc['tools_identiques']}",
            f"- Messages avant le prompt utilisateur identiques : "
            f"{sc['messages_avant_prompt_identiques']}"]
    for change in sc["system_diff"]:
        out.append(f"- ({change['cause']}) `{change['ligne'][:200]}`")
    if report.get("responses_cache_key"):
        out += ["", f"- `prompt_cache_key` (Responses) : {report['responses_cache_key']}"]
    acp = report["run"].get("acp")
    if acp:
        init = acp.get("initialize") or {}
        out += ["", "## ACP (`--mode acp`, daemon isolé)", "",
                f"- `initialize` : protocolVersion {init.get('protocolVersion')}, "
                f"agentCapabilities `{compact(init.get('agentCapabilities'))}`",
                f"- `session/new` réussi : {acp['session_new_ok']}",
                f"- `session/prompt` : `{compact(acp['prompt_result'])}`",
                f"- Requêtes agent → client (ex. `session/request_permission`) : "
                f"{acp['agent_requests'] or 'aucune'}",
                f"- Notifications : {', '.join(acp['notifications'])}",
                f"- Process restants après arrêt : {acp['leftover_processes'] or 'aucun'}"]
    spawn = report["run"].get("spawn")
    if spawn:
        out += ["", "## Sous-agent (`rlm.spawn` puis `rlm.collect`, scénario `spawn`)", "",
                f"- Résultat : {'OK' if spawn['ok'] else 'ÉCHEC'} (exit {spawn['exit']}, "
                f"{spawn['seconds']} s)",
                f"- Requêtes parent / enfant : {spawn['parent_requests']} / "
                f"{spawn['child_requests']}",
                f"- Ligne `rlm.collect` vue par le parent : `{spawn['collect_line']}`",
                f"- Réponse de l'enfant (`agent_message.send`) dans le contexte du parent : "
                f"{spawn['child_reply_in_parent_context']}"]
    out += ["", "## Environnement vu par le kernel (s1-anthropic, tour 2)", "",
            "Variables factices transmises au binaire : "
            f"{', '.join(report['run'].get('fake_secret_names', []))}.", ""]
    for line in report["env_visibility"]:
        out.append(f"- résultat de cellule : `{line[:300]}`")
    out += ["", "## Crash du kernel (résultats de tool du dernier appel)", ""]
    for line in report["crash"]["tool_results"]:
        out.append(f"- `{line[:400]}`")
    out += ["", "## Reprise de session (`--continue`)", ""]
    resume = report["resume"]
    out += [f"- Requêtes : {resume['requests']}",
            f"- Dernier appel du prompt 1 → premier appel du prompt 2 : système identique "
            f"{resume['pair']['system_identique'] if resume['pair'] else 'n/a'}, préfixe exact "
            f"hors cache_control {resume['pair']['prefixe_exact_hors_cache_control'] if resume['pair'] else 'n/a'}"]
    for change in (resume["pair"] or {}).get("system_diff", []):
        out.append(f"- ({change['cause']}) `{change['ligne'][:200]}`")
    for diff in (resume["pair"] or {}).get("diffs_messages", []):
        out.append(f"- message {diff['index']} : {diff['cause']}")
    out += ["", "## Compaction", ""]
    comp = report["compaction"]
    out += [f"- Requêtes : {comp['requests']}",
            f"- Requêtes sans tools (résumé) : {comp['aux_requests']}",
            f"- Système identique avant / après : {fmt(comp['system_identique_apres'])}",
            f"- Prompt utilisateur 1 retrouvé mot pour mot après compaction : "
            f"{fmt(comp['prompt_1_conserve'])}",
            f"- Messages de la conversation après compaction : {fmt(comp['messages_apres'])}"]
    if comp.get("premier_message_apres"):
        out.append(f"- Début du premier message après compaction : "
                   f"`{comp['premier_message_apres'][:300]}`")
    for change in comp.get("system_diff", []):
        out.append(f"- ({change['cause']}) `{change['ligne'][:200]}`")
    return "\n".join(out) + "\n"


def layers_from_dump(path: Path) -> list[dict]:
    """Découpe le prompt assemblé selon les segments (`chars` = octets UTF-8, joints par \\n\\n)."""
    try:
        dump = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return []
    prompt = dump.get("prompt", "").encode()
    layers, offset = [], 0
    for segment in dump.get("segments", []):
        size = segment.get("chars", 0)
        text = prompt[offset:offset + size].decode(errors="replace")
        offset += size + 2
        layers.append({"name": segment.get("name"), "kind": segment.get("kind"),
                       "source": segment.get("source"), "bytes": size,
                       "tokens_est": tokens(text)})
    return layers


def main() -> None:
    run_dir = Path(sys.argv[1])
    info = json.loads((run_dir / "run.json").read_text())
    cases = {name: load_case(run_dir / "cases" / name) for name in info["cases"]}
    report = {
        "run": info,
        "tokenizer": "tiktoken o200k_base (estimation)" if ENCODER else "absent (pas d'estimation)",
        "cases": {name: analyze_case(name, reqs) for name, reqs in cases.items()},
        "layers": layers_from_dump(run_dir / "prompt-breakdown.json"),
    }
    s1, s2 = main_requests(cases["s1-anthropic"]), main_requests(cases["s2-anthropic"])
    report["sessions"] = session_compare(s1[0], s2[0])
    responses = main_requests(cases.get("s1-responses", []))
    if responses:
        report["responses_cache_key"] = sorted({r.body.get("prompt_cache_key", "absent")
                                                for r in responses})
    report["env_visibility"] = tool_results_text(s1[-1])[1:2] if s1 else []
    crash = main_requests(cases["crash-anthropic"])
    report["crash"] = {"tool_results": tool_results_text(crash[-1]) if crash else []}

    resume_reqs = main_requests(cases["resume-anthropic"])
    second = [r for r in resume_reqs if "Deuxième message" in compact(r.conversation())]
    first_of_second = second[0] if second else None
    before = [r for r in resume_reqs if first_of_second is None or r.seq < first_of_second.seq]
    report["resume"] = {
        "requests": [r.seq for r in resume_reqs],
        "pair": compare_pair(before[-1], first_of_second) if before and first_of_second else None,
    }

    compact_reqs = cases["compact-anthropic"]
    mains = main_requests(compact_reqs)
    aux = [r.seq for r in compact_reqs if not r.has_tools]
    after = [r for r in mains if aux and r.seq > aux[0]]
    first_after = compact(after[0].conversation()[0]) if after and after[0].conversation() else None
    report["compaction"] = {
        "requests": [{"seq": r.seq, "tools": r.has_tools,
                      "reported_input_tokens": r.meta.get("reported_input_tokens")}
                     for r in compact_reqs],
        "aux_requests": aux,
        "system_identique_apres": (compact(mains[0].system_value()) ==
                                   compact(after[0].system_value())) if after else None,
        "prompt_1_conserve": (any("Exécute le scénario de capture." in compact(m)
                                  for m in after[0].conversation()) if after else None),
        "messages_apres": len(after[0].conversation()) if after else None,
        "premier_message_apres": first_after,
    }
    if after and not report["compaction"]["system_identique_apres"]:
        report["compaction"]["system_diff"] = system_line_diff(mains[0].system_text(),
                                                               after[0].system_text())
    (run_dir / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    (run_dir / "report.md").write_text(render(report))


if __name__ == "__main__":
    main()
