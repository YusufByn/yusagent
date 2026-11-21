#!/usr/bin/env python3
"""Préfixe des chemins avec démon contre -p (YA-10, ticket 24).

Usage : python3 -I prefix_paths_check.py <run>   (écrit <run>/prefix-paths-report.md et .json)

Compare, requête par requête, le cas `-p` `s1-anthropic` aux chemins qui passent par le démon :
`acp` (`--mode acp`) et `tui` (le TUI sur un pty), tous au scénario `basic`, même projet, mêmes
réglages. Toutes les clés du corps doivent être identiques (`messages` après normalisation des
dates), sauf `system`, dont l'écart est limité aux deux lignes liées au démon (écart accepté par
Yusuf le 21/11/2026, journal du CDC) :
  - en `-p` seulement : la ligne « This session has no daemon: … » (et la ligne vide avant) ;
  - avec le démon seulement : le skill `rlm-heartbeat` dans la liste des skills.

Code de sortie 1 si un chemin manque, si le nombre de requêtes diffère, ou si un écart sort de
ces deux lignes. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import difflib
import json
import re
import sys
from pathlib import Path

DAEMONLESS_PREFIX = "This session has no daemon: "
HEARTBEAT_PREFIX = "- `rlm-heartbeat` "
DATE = re.compile(r"\d{4}-\d{2}-\d{2}")
REFERENCE = ("-p", Path("cases") / "s1-anthropic" / "cap")
PATHS = (("acp", Path("acp") / "cap"), ("tui", Path("tui") / "cap"))


def bodies(directory: Path) -> list[dict]:
    return [json.loads(path.read_text()) for path in sorted(directory.glob("*.body.json"))]


def system_lines(body: dict) -> list[str]:
    system = body.get("system")
    if isinstance(system, list):
        system = "\n".join(block.get("text", "") for block in system)
    return (system or "").splitlines()


def allowed(removed: list[str], added: list[str]) -> list[str]:
    """Les lignes de l'écart qui sortent des deux lignes acceptées."""
    bad = [f"-p seulement : {line!r}" for line in removed
           if line.strip() and not line.startswith(DAEMONLESS_PREFIX)]
    bad += [f"démon seulement : {line!r}" for line in added
            if line.strip() and not line.startswith(HEARTBEAT_PREFIX)]
    if sum(1 for line in removed if line.strip()) > 1:
        bad.append("plus d'une ligne non vide propre à -p")
    if sum(1 for line in added if line.strip()) > 1:
        bad.append("plus d'une ligne non vide propre au démon")
    return bad


def compare(reference: list[dict], other: list[dict]) -> dict:
    result = {"requests": [len(reference), len(other)], "problems": [], "system_diff": []}
    if not other:
        result["problems"].append("aucune requête capturée")
        return result
    if len(reference) != len(other):
        result["problems"].append(f"nombre de requêtes : {len(reference)} contre {len(other)}")
    for index, (ref, oth) in enumerate(zip(reference, other), start=1):
        for key in sorted(set(ref) | set(oth)):
            if key == "system":
                continue
            left, right = ref.get(key), oth.get(key)
            if key == "messages":
                left = DATE.sub("<DATE>", json.dumps(left, ensure_ascii=False))
                right = DATE.sub("<DATE>", json.dumps(right, ensure_ascii=False))
            if left != right:
                result["problems"].append(f"requête {index} : clé `{key}` différente")
        removed, added = [], []
        diff = list(difflib.unified_diff(system_lines(ref), system_lines(oth), lineterm="", n=0))
        for line in diff:
            if line.startswith(("---", "+++", "@@")):
                continue
            (removed if line.startswith("-") else added).append(line[1:])
        if index == 1:
            result["system_diff"] = diff
        result["problems"] += [f"requête {index} : {problem}" for problem in allowed(removed, added)]
    return result


def main() -> None:
    run = Path(sys.argv[1])
    reference = bodies(run / REFERENCE[1])
    report = {"reference": REFERENCE[0], "reference_requests": len(reference), "paths": {}}
    lines = ["# Préfixe : chemins avec démon contre -p (ticket 24)", "",
             f"Référence `-p` : `{REFERENCE[1]}`, {len(reference)} requêtes.", ""]
    ok = bool(reference)
    if not reference:
        lines.append("**Référence absente.**")
    for name, directory in PATHS:
        result = compare(reference, bodies(run / directory))
        report["paths"][name] = result
        ok = ok and not result["problems"]
        lines += [f"## {name} (`{directory}`)", "",
                  f"- Requêtes : {result['requests'][1]}",
                  f"- Écarts hors des deux lignes acceptées : {len(result['problems'])}"]
        lines += [f"  - {problem}" for problem in result["problems"]]
        if result["system_diff"]:
            lines += ["", "Écart du prompt système (requête 1) :", "", "```diff",
                      *result["system_diff"], "```"]
        lines.append("")
    lines.append("CONFORME" if ok else "NON CONFORME")
    (run / "prefix-paths-report.md").write_text("\n".join(lines) + "\n")
    (run / "prefix-paths-report.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(lines[-1])
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
