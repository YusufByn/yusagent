#!/usr/bin/env python3
"""YA-14, adaptateur Codex CLI : les changements communs aux deux bras ne changent rien aux
runs de YA-03 (preuve demandée avant la comparaison yusAgent contre Codex CLI).

Changements communs : le reverse proxy note `generate` et la fin de chaque réponse (journal
seulement), et le coût à cache théorique compte en rupture un appel de boucle dont la requête
WebSocket part en corps complet (`metrics.theoretical_cache`, `per_request`).

Méthode (aucun modèle, aucun binaire) : chaque bras des A/B 1 et 2 de YA-03 est recalculé deux
fois par `run.py --rereport`, sur deux copies (liens durs pour les journaux, rapports copiés) :
avec le code de `main` (worktree) et avec celui de la branche. Ce script compare les deux
`report.json`, résultat par résultat, sur tout ce qui sert au verdict ou à l'affichage d'un A/B.

Usage :
    python3 -I bench/smoke/ab/ya14-codex-rereport-check.py <copies main> <copies branche>
où chaque dossier contient `ab-<horodatage>/<bras>/report.json`. Sortie : nombre de résultats
comparés, champs identiques, écarts (aucun attendu). Bibliothèque standard uniquement.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

FIELDS = (
    ("status",), ("resolved",), ("tokens",), ("calls",), ("cache",), ("server_misses",),
    ("cost", "total"), ("cost", "pessimistic_total"), ("cost", "theoretical_total"),
    ("cost", "theoretical"), ("cost", "invalidations"), ("subagents",), ("cells",), ("edits",),
    ("transports",), ("per_request",), ("context_peak",), ("anomalies",),
)


def get(result: dict, path: tuple[str, ...]):
    value = result
    for key in path:
        value = (value or {}).get(key) if isinstance(value, dict) else None
    return value


def main() -> None:
    main_root, branch_root = Path(sys.argv[1]), Path(sys.argv[2])
    compared = 0
    diffs: list[str] = []
    bases = {"main": {}, "branche": {}}
    for report in sorted(main_root.glob("*/*/report.json")):
        rel = report.relative_to(main_root)
        other = branch_root / rel
        if not other.exists():
            diffs.append(f"{rel} : absent de la branche")
            continue
        a, b = json.loads(report.read_text()), json.loads(other.read_text())
        for ra, rb in zip(a["results"], b["results"], strict=True):
            compared += 1
            for path in FIELDS:
                if get(ra, path) != get(rb, path):
                    diffs.append(f"{rel.parent} {ra['task']} : {'.'.join(path)} "
                                 f"{json.dumps(get(ra, path))[:120]} → "
                                 f"{json.dumps(get(rb, path))[:120]}")
            for side, r in (("main", ra), ("branche", rb)):
                for basis, n in ((r["cost"].get("theoretical") or {}).get("calls") or {}).items():
                    bases[side][basis] = bases[side].get(basis, 0) + n
    print(f"résultats comparés : {compared} ; champs : "
          f"{', '.join('.'.join(p) for p in FIELDS)}")
    print(f"appels par base du cache théorique : main {bases['main']} ; branche {bases['branche']}")
    print(f"écarts : {len(diffs)}")
    for line in diffs[:50]:
        print(f"- {line}")
    raise SystemExit(1 if diffs else 0)


if __name__ == "__main__":
    main()
