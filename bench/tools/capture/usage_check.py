#!/usr/bin/env python3
"""Vérifie l'usage d'un run du banc de capture contre l'usage renvoyé par le serveur (T04, YA-13).

Usage : python3 -I usage_check.py <run>   (écrit <run>/usage-report.md et .json)

Pour chaque cas, l'usage attendu de chaque requête est recalculé depuis
`reported_usage` (méta du serveur), normalisé comme le harness doit le faire
(docs/usage.md) :
  - Anthropic : message_start puis message_delta, comptes cumulés (un champ du
    message_delta remplace celui du message_start) ; `input` hors cache ;
  - OpenAI Responses : `input` = input_tokens - cached_tokens - cache_write_tokens,
    `cacheWrite` = cache_write_tokens ; au journal, ventilation 30 min / 12 h lue dans
    `providerUsage` (égal à l'usage renvoyé) et `costWarning` si du 12 h apparaît (T08, YA-13).

Deux sources sont confrontées à cet attendu :
  1. l'usage du message assistant (événements `message_end` de `out*.jsonl`),
     pour les appels de la boucle (requêtes avec tools), quel que soit le binaire ;
  2. le journal `<id>.usage.ndjson` (binaire T04 et suivants), ligne par ligne dans
     l'ordre des requêtes : chaque champ, le taux de cache et le type d'appel.

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

FIELDS = ("input", "output", "cacheRead", "cacheWrite")
# `costWarning` d'une ligne dont l'usage brut a des écritures 12 h (T08) :
# CACHE_WRITE_12H_WARNING, crates/pa-core/src/session_engine/usage_log.rs.
COST_WARNING_12H = "cacheWrite12h priced at the catalog write rate"


def expected_usage(meta: dict) -> dict | None:
    reported = meta.get("reported_usage")
    if not reported:
        return None
    if "message_start" in reported:
        final = dict(reported["message_start"])
        final.update(reported["message_delta"])
        return anthropic_expected(final)
    if "json" in reported and "cache_read_input_tokens" in reported["json"]:
        return anthropic_expected(reported["json"])
    usage = reported.get("response.completed") or reported.get("json")
    details = usage["input_tokens_details"]
    cached = details["cached_tokens"]
    write = details.get("cache_write_tokens", 0)
    split = details.get("cache_write_tokens_details") or {}
    return {"input": max(0, usage["input_tokens"] - cached - write),
            "output": usage["output_tokens"], "cacheRead": cached, "cacheWrite": write,
            "cacheWrite1h": None, "cacheWrite30m": split.get("ttl_30m_tokens"),
            "cacheWrite12h": split.get("ttl_12h_tokens"),
            "costWarning": (COST_WARNING_12H if (split.get("ttl_12h_tokens") or 0) > 0
                            else None),
            "reasoning": usage["output_tokens_details"]["reasoning_tokens"],
            # Le journal retire `attribution` (Codex, non documenté) et n'en garde que
            # `request_fields`, sous `undocumentedAttribution` (T08).
            "providerUsage": {k: v for k, v in usage.items() if k != "attribution"},
            "undocumentedAttribution": (
                {"request_fields": usage["attribution"]["request_fields"]}
                if isinstance(usage.get("attribution"), dict)
                and "request_fields" in usage["attribution"] else None)}


def anthropic_expected(usage: dict) -> dict:
    return {"input": usage["input_tokens"], "output": usage["output_tokens"],
            "cacheRead": usage["cache_read_input_tokens"],
            "cacheWrite": usage["cache_creation_input_tokens"],
            "cacheWrite1h": usage["cache_creation"]["ephemeral_1h_input_tokens"],
            "reasoning": (usage.get("output_tokens_details") or {}).get("thinking_tokens")}


def requests(case_dir: Path) -> list[dict]:
    out = []
    for meta_path in sorted((case_dir / "cap").glob("*.meta.json")):
        meta = json.loads(meta_path.read_text())
        body_path = case_dir / "cap" / meta_path.name.replace(".meta.json", ".body.json")
        try:
            body = json.loads(body_path.read_text())
        except (OSError, json.JSONDecodeError):
            body = {}
        meta["has_tools"] = bool(body.get("tools"))
        meta["expected"] = expected_usage(meta)
        out.append(meta)
    return out


def message_end_usages(case_dir: Path) -> list[dict]:
    """Usage des messages assistant terminés, dans l'ordre des sorties `--mode json`."""
    usages = []
    for out in sorted(case_dir.glob("out*.jsonl"), key=lambda p: (len(p.name), p.name)):
        for line in out.read_text(errors="replace").splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            message = event.get("message") if isinstance(event, dict) else None
            if (event.get("type") == "message_end" and isinstance(message, dict)
                    and message.get("role") == "assistant"):
                usages.append(message.get("usage") or {})
    return usages


def journal_lines(case_dir: Path) -> list[dict]:
    lines = []
    for path in sorted((case_dir / "usage").glob("*.usage.ndjson")):
        for line in path.read_text().splitlines():
            lines.append(json.loads(line))
    return sorted(lines, key=lambda entry: (entry["startMs"], entry["seq"]))


def compare(expected: dict, actual: dict, fields) -> list[str]:
    return [f"{field}: attendu {expected.get(field)!r}, lu {actual.get(field)!r}"
            for field in fields if expected.get(field) != actual.get(field)]


def check_case(case_dir: Path) -> dict:
    reqs = [r for r in requests(case_dir) if r["expected"] is not None]
    loop_reqs = [r for r in reqs if r["has_tools"]]
    ends = message_end_usages(case_dir)
    result = {"requests": len(reqs), "loop_requests": len(loop_reqs),
              "message_end": len(ends), "message_end_mismatches": []}
    for index, (req, usage) in enumerate(zip(loop_reqs, ends)):
        diff = compare(req["expected"], usage, FIELDS)
        if diff:
            result["message_end_mismatches"].append({"request": req["seq"], "index": index,
                                                     "diff": diff})
    if len(ends) != len(loop_reqs):
        result["message_end_count_mismatch"] = True
    journal = journal_lines(case_dir)
    result["journal_lines"] = len(journal)
    if not journal:
        return result
    result["journal_mismatches"] = []
    result["kinds"] = sorted({entry["kind"] for entry in journal})
    result["journal_count_ok"] = len(journal) == len(reqs)
    for req, entry in zip(reqs, journal):
        expected = dict(req["expected"])
        diff = compare(expected, entry, FIELDS)
        for optional in ("reasoning", "cacheWrite1h", "cacheWrite30m", "cacheWrite12h",
                         "costWarning", "providerUsage", "undocumentedAttribution"):
            if expected.get(optional) is not None and entry.get(optional) != expected[optional]:
                diff.append(f"{optional}: attendu {expected[optional]!r}, "
                            f"lu {entry.get(optional)!r}")
        # Présents seulement quand le provider les donne (T08, YA-13).
        for absent in ("cacheWrite30m", "cacheWrite12h", "costWarning", "providerUsage",
                       "undocumentedAttribution"):
            if expected.get(absent) is None and absent in entry:
                diff.append(f"{absent}: absent attendu, lu {entry[absent]!r}")
        if expected["cacheWrite1h"] is not None:
            five = expected["cacheWrite"] - expected["cacheWrite1h"]
            if entry.get("cacheWrite5m") != five:
                diff.append(f"cacheWrite5m: attendu {five}, lu {entry.get('cacheWrite5m')!r}")
        prompt = expected["input"] + expected["cacheRead"] + expected["cacheWrite"]
        rate = expected["cacheRead"] / prompt if prompt else None
        if (rate is None) != (entry.get("cacheRate") is None) or (
                rate is not None and abs(rate - entry["cacheRate"]) > 1e-12):
            diff.append(f"cacheRate: attendu {rate!r}, lu {entry.get('cacheRate')!r}")
        loop_kinds = ("main", "subagent")
        if req["has_tools"] != (entry["kind"] in loop_kinds):
            diff.append(f"kind: {entry['kind']!r} pour une requête "
                        f"{'avec' if req['has_tools'] else 'sans'} tools")
        if diff:
            result["journal_mismatches"].append({"request": req["seq"],
                                                 "journal_seq": entry["seq"],
                                                 "kind": entry["kind"], "diff": diff})
    result["sample"] = journal[min(1, len(journal) - 1)]
    return result


def check_spawn(spawn_dir: Path) -> dict:
    journal = journal_lines(spawn_dir)
    by_session: dict[str, list[dict]] = {}
    for entry in journal:
        by_session.setdefault(entry["sessionId"], []).append(entry)
    parents = {sid for sid, entries in by_session.items()
               if all(e["parentSessionId"] is None for e in entries)}
    children = {sid: entries[0]["parentSessionId"] for sid, entries in by_session.items()
                if sid not in parents}
    return {
        "journal_lines": len(journal),
        "sessions": {sid: sorted({e["kind"] for e in entries})
                     for sid, entries in by_session.items()},
        "children_point_to_parent": bool(children)
                                    and all(parent in parents for parent in children.values()),
    }


def render(report: dict) -> str:
    out = ["# Usage : journal et usage enregistré contre l'usage renvoyé (T04, YA-13)", "",
           f"Run : `{report['run']}`, binaire `{report.get('binary_version')}`", "",
           "| Cas | Requêtes | Boucle | message_end | Écarts message_end | Lignes de journal "
           "| Écarts journal | Types |", "|---|---|---|---|---|---|---|---|"]
    for name, case in report["cases"].items():
        out.append(
            f"| {name} | {case['requests']} | {case['loop_requests']} | {case['message_end']} | "
            f"{len(case['message_end_mismatches'])} | {case['journal_lines']} | "
            f"{len(case.get('journal_mismatches', [])) if case['journal_lines'] else '-'} | "
            f"{', '.join(case.get('kinds', []))} |")
    out.append("")
    spawn = report.get("spawn")
    if spawn:
        out += ["## Sous-agent", "", f"- lignes : {spawn['journal_lines']}",
                f"- types par session : `{json.dumps(spawn['sessions'])}`",
                f"- enfants rattachés au parent : {spawn['children_point_to_parent']}", ""]
    for name, case in report["cases"].items():
        mismatches = case["message_end_mismatches"] + case.get("journal_mismatches", [])
        if not mismatches:
            continue
        out += [f"## Écarts : {name}", ""]
        for item in case["message_end_mismatches"][:5]:
            out.append(f"- message_end, requête {item['request']} : {'; '.join(item['diff'])}")
        for item in case.get("journal_mismatches", [])[:5]:
            out.append(f"- journal, requête {item['request']} ({item['kind']}) : "
                       f"{'; '.join(item['diff'])}")
        out.append("")
    sample = next((c["sample"] for c in report["cases"].values() if c.get("sample")), None)
    if sample:
        out += ["## Exemple de ligne de journal", "", "```json",
                json.dumps(sample, indent=2, ensure_ascii=False), "```", ""]
    return "\n".join(out) + "\n"


def main() -> None:
    run = Path(sys.argv[1])
    info = json.loads((run / "run.json").read_text())
    report = {"run": str(run), "binary_version": info.get("binary_version"), "cases": {}}
    for name in info["cases"]:
        report["cases"][name] = check_case(run / "cases" / name)
    if (run / "spawn").is_dir():
        report["spawn"] = check_spawn(run / "spawn")
    (run / "usage-report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    (run / "usage-report.md").write_text(render(report))


if __name__ == "__main__":
    main()
