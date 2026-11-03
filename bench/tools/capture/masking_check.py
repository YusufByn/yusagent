#!/usr/bin/env python3
"""Masquage (T05, YA-05) : paliers, tokens envoyés et invalidations, cas `mask-*`.

Usage : python3 -I masking_check.py <run>   (écrit <run>/masking-report.md et .json)

Pour chaque cas `mask-*` du run (scénario `mask` : 25 tours d'environ 5 000 caractères,
erreur au tour 12, cache simulé sur le préfixe réel, `server.py --prefix-cache`) :
  - chaque requête de la boucle : tour N, sorties masquées (placeholders), octets et
    tokens est. du corps, présence de la sortie en erreur du tour 12 ;
  - chaque rupture de préfixe N -> N+1 (hors déplacement de `cache_control`), avec la
    ligne du journal d'usage de la requête : `cacheRead`, `cacheWrite`, `invalidation` ;
  - la relecture depuis le kernel (ligne `REREAD` de la cellule 25) confrontée au
    nombre de caractères annoncé par le placeholder.

Tokens : tiktoken o200k_base sur le corps JSON quand il est importable (ESTIMATION) ;
`cacheRead` / `cacheWrite` sont ceux du serveur (octets / 4, simulation).
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from analyze import ENCODER, compare_pair, load_case, main_requests, tokens  # noqa: E402
from usage_check import journal_lines, requests as usage_requests  # noqa: E402

PLACEHOLDER = re.compile(
    r"\[sortie masquée : tour (\d+), (\d+) caractères, relire avec "
    r"from rlm\.masked import tool_output; print\(tool_output\(\"([^\"]+)\"\)\)\]")
ERROR_MARK = "YA_MASK_ERROR"
ERROR_TURN = 12


def tool_results(req) -> list[tuple[str, str]]:
    """(identifiant, texte) de chaque résultat de tool de la requête, dans l'ordre."""
    out = []
    for message in req.conversation():
        content = message.get("content")
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    inner = block.get("content")
                    text = inner if isinstance(inner, str) else "".join(
                        part.get("text", "") for part in inner or [] if isinstance(part, dict))
                    out.append((block.get("tool_use_id"), text))
        if message.get("type") == "function_call_output":
            out.append((message.get("call_id"), str(message.get("output"))))
    return out


def journal_by_seq(case_dir: Path) -> dict[int, dict]:
    """Ligne du journal d'usage de chaque requête (même appariement que usage_check)."""
    reqs = [r for r in usage_requests(case_dir) if r["expected"] is not None]
    return {req["seq"]: entry for req, entry in zip(reqs, journal_lines(case_dir))}


def check_case(case_dir: Path) -> dict:
    mains = main_requests(load_case(case_dir))
    journal = journal_by_seq(case_dir)
    rows, ruptures = [], []
    previous = None
    for req in mains:
        results = tool_results(req)
        masked = [PLACEHOLDER.fullmatch(text) for _, text in results]
        masked = [m for m in masked if m]
        entry = journal.get(req.seq, {})
        text = req.raw.decode(errors="replace")
        row = {
            "seq": req.seq,
            "turn": len(results) + 1,
            "bytes": len(req.raw),
            "tokens_est": tokens(text),
            "masked": len(masked),
            "masked_turns": [int(m.group(1)) for m in masked],
            "error_visible": any(ERROR_MARK in out for _, out in results),
            "cacheRead": entry.get("cacheRead"),
            "cacheWrite": entry.get("cacheWrite"),
            "invalidation": entry.get("invalidation"),
        }
        if previous is not None:
            pair = compare_pair(previous, req)
            row["prefix_kept"] = pair["prefixe_exact_hors_cache_control"]
            if not row["prefix_kept"]:
                first = next((d["index"] for d in pair["diffs_messages"]
                              if not d["cause"].startswith("déplacement")), None)
                ruptures.append({**{k: row[k] for k in ("seq", "turn", "masked", "cacheRead",
                                                         "cacheWrite", "invalidation")},
                                 "first_changed_message": first})
        rows.append(row)
        previous = req
    reread = reread_check(mains)
    with_error = [row for row in rows if row["turn"] > ERROR_TURN]
    return {
        "requests": rows,
        "ruptures": ruptures,
        "invalidations": [row["seq"] for row in rows if row["invalidation"]],
        "ruptures_match_invalidations": [r["seq"] for r in ruptures]
                                        == [row["seq"] for row in rows if row["invalidation"]],
        "journal_lines_matched": sum(1 for row in rows if row["cacheWrite"] is not None),
        "error_visible_after_turn_12": all(row["error_visible"] for row in with_error),
        "totals": {
            "requests": len(rows),
            "bytes": sum(row["bytes"] for row in rows),
            "tokens_est": sum(row["tokens_est"] for row in rows) if ENCODER else None,
            "cacheRead": sum(row["cacheRead"] or 0 for row in rows),
            "cacheWrite": sum(row["cacheWrite"] or 0 for row in rows),
        },
        "reread": reread,
    }


def reread_check(mains) -> dict | None:
    """Ligne REREAD de la cellule 25 et placeholder de la sortie relue."""
    for index, req in enumerate(mains):
        for call_id, text in tool_results(req):
            line = next((l for l in text.splitlines() if l.startswith("REREAD ")), None)
            if line is None:
                continue
            parts = line.split()
            read_id = parts[1].strip("'")
            announced = None
            source = mains[index - 1] if index else req
            for _, earlier in tool_results(source):
                match = PLACEHOLDER.fullmatch(earlier)
                if match and match.group(3) == read_id:
                    announced = int(match.group(2))
            read_chars = int(parts[2]) if len(parts) > 2 and parts[2].isdigit() else None
            return {"line": line, "id": None if read_id == "None" else read_id,
                    "read_chars": read_chars, "announced_chars": announced,
                    "ok": read_id != "None" and announced is not None
                          and read_chars == announced}
    return None


def fmt(value) -> str:
    return "n/a" if value is None else str(value)


def render(report: dict) -> str:
    out = ["# Masquage : paliers, tokens et invalidations (T05, YA-05)", "",
           f"Run : `{report['run']}`, binaire `{report.get('binary_version')}`. "
           f"Tokens : {report['tokenizer']}. `cacheRead` / `cacheWrite` : journal d'usage, "
           "valeurs du serveur de capture (préfixe commun en octets / 4, simulation).", "",
           "| Cas | Requêtes | Octets envoyés | Tokens est. envoyés | cacheRead | cacheWrite | "
           "Ruptures | Invalidations journalisées | Ruptures = invalidations | "
           "Erreur du tour 12 visible | Relecture |",
           "|---|---|---|---|---|---|---|---|---|---|---|"]
    for name, case in report["cases"].items():
        t = case["totals"]
        reread = case["reread"] or {}
        out.append(f"| {name} | {t['requests']} | {t['bytes']} | {fmt(t['tokens_est'])} | "
                   f"{t['cacheRead']} | {t['cacheWrite']} | {len(case['ruptures'])} | "
                   f"{len(case['invalidations'])} | {case['ruptures_match_invalidations']} | "
                   f"{case['error_visible_after_turn_12']} | {fmt(reread.get('ok'))} |")
    off, on = report["cases"].get("mask-off-anthropic"), report["cases"].get("mask-on-anthropic")
    if off and on and off["totals"]["tokens_est"] and on["totals"]["tokens_est"]:
        ratio = on["totals"]["tokens_est"] / off["totals"]["tokens_est"]
        out += ["", f"Tokens est. envoyés, avec / sans masquage : {ratio:.3f} "
                    f"({on['totals']['tokens_est']} / {off['totals']['tokens_est']})."]
    for name, case in report["cases"].items():
        out += ["", f"## {name} : ruptures de préfixe", ""]
        if not case["ruptures"]:
            out.append("Aucune.")
        else:
            out += ["| Requête | Tour N | Sorties masquées | Premier message changé | "
                    "cacheRead | cacheWrite | invalidation (journal) |", "|---|---|---|---|---|---|---|"]
            for r in case["ruptures"]:
                out.append(f"| {r['seq']} | {r['turn']} | {r['masked']} | "
                           f"{fmt(r['first_changed_message'])} | {fmt(r['cacheRead'])} | "
                           f"{fmt(r['cacheWrite'])} | `{json.dumps(r['invalidation'])}` |")
        out += ["", "| Requête | Tour N | Octets | Tokens est. | Masquées | Tours masqués | "
                "Erreur 12 visible | cacheRead | cacheWrite |", "|---|---|---|---|---|---|---|---|---|"]
        for row in case["requests"]:
            turns = row["masked_turns"]
            span = f"{min(turns)}-{max(turns)}" if turns else "-"
            out.append(f"| {row['seq']} | {row['turn']} | {row['bytes']} | {fmt(row['tokens_est'])} "
                       f"| {row['masked']} | {span} | {row['error_visible']} | "
                       f"{fmt(row['cacheRead'])} | {fmt(row['cacheWrite'])} |")
        if case["reread"]:
            out += ["", f"Relecture (cellule 25) : `{case['reread']['line']}` ; caractères annoncés "
                        f"par le placeholder : {fmt(case['reread']['announced_chars'])}."]
    return "\n".join(out) + "\n"


def main() -> None:
    run = Path(sys.argv[1])
    info = json.loads((run / "run.json").read_text())
    report = {"run": str(run), "binary_version": info.get("binary_version"),
              "tokenizer": "tiktoken o200k_base (estimation)" if ENCODER else "absent",
              "cases": {name: check_case(run / "cases" / name)
                        for name in info["cases"] if name.startswith("mask-")}}
    (run / "masking-report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    (run / "masking-report.md").write_text(render(report))


if __name__ == "__main__":
    main()
