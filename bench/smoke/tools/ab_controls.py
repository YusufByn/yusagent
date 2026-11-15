#!/usr/bin/env python3
"""Contrôles indépendants d'un A/B entrelacé du smoke bench (YA-09), lecture seule.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/ab_controls.py bench/smoke/runs/ab-<horodatage> [--top 4]

Recompte, par bras et sur tous les lancements du plan (unités écartées comprises), ce que
`ab_stats.py` écarte ou affiche, sans passer par lui :
- `report.json` : statuts, `stalled` et timeouts, compactions, ruptures du cache théorique,
  miss serveur, bascules SSE (`loop_on_sse`), échecs de transport de la session, appels non
  appariés, anomalies ;
- `proxy.ndjson` de la tâche et de la chauffe : requêtes, statuts hors 101 et 200,
  `response_error`, réponses non `completed`, corps complets (requête WebSocket sans suite ou
  requête SSE) hors premier appel, réponses avec `reasoning_tokens` > 0 ; appels les plus longs
  (de la requête à la réponse, au reverse proxy) ;
- sessions : réponses en `stopReason: error` du journal d'usage ;
- tous les fichiers texte des runs (hors `workspace`) : erreurs de quota (`usage_limit_reached`,
  `rate_limit_exceeded`, `"limit_reached": true`, statut 429).

Aucune valeur de secret n'est lue ni affichée : seuls des compteurs, des statuts et des codes
d'erreur sortent. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
# `limit_reached` seul est un champ des relevés de quota de Codex (`null` ou `false`) : seule la
# valeur `true` compte.
QUOTA = re.compile(r'usage_limit_reached|rate_limit_exceeded|"limit_reached":\s*true'
                   r'|"status":\s*"?429\b')
TEXT = (".ndjson", ".jsonl", ".log", ".json", ".md", ".err", ".out")

ROWS = [
    ("lancements", "Lancements"),
    ("statuts", "Statuts"),
    ("stalled_timeout", "`stalled` ou timeout"),
    ("stop_error", "Réponses en `stopReason: error` (journal d'usage)"),
    ("response_error", "`response_error` au proxy (tâche, chauffe)"),
    ("non_completed", "Réponses non `completed` (tâche)"),
    ("hors_101_200", "Requêtes proxy hors 101 et 200"),
    ("quota", "Fichiers avec une erreur de quota"),
    ("compactions", "Compactions"),
    ("ruptures", "Ruptures du cache théorique"),
    ("miss", "Miss serveur"),
    ("full_after_first", "Corps complets hors premier appel (WebSocket sans suite, SSE)"),
    ("sse", "Bascules SSE (`loop_on_sse`), échecs de transport de la session"),
    ("unmatched", "Appels non appariés (usage, proxy)"),
    ("reasoning", "Réponses avec `reasoning_tokens` > 0 (somme des tokens)"),
    ("anomalies", "Anomalies de `report.json`"),
]


def ndjson(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(errors="replace").splitlines() if line.strip()]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("ab_dir", type=Path, help="dossier runs/ab-<horodatage>")
    parser.add_argument("--top", type=int, default=4, help="appels les plus longs par bras")
    args = parser.parse_args()
    plan = json.loads((args.ab_dir / "ab.json").read_text())

    count: dict[str, Counter] = defaultdict(Counter)
    statuses: dict[str, Counter] = defaultdict(Counter)
    durations: dict[str, list] = defaultdict(list)
    details: list[str] = []
    for arm in plan["arms"]:
        side, run_dir = arm["arm"], REPO / arm["run_dir"]
        where = f"passe {arm['pass']}, {arm['task']}, bras {side} (`{run_dir.name}`)"
        c = count[side]
        report = json.loads((run_dir / "report.json").read_text())
        for r in report["results"]:
            c["lancements"] += 1
            statuses[side][r["status"]] += 1
            agent = r.get("agent") or {}
            c["stalled_timeout"] += bool(agent.get("stalled") or agent.get("timeout"))
            c["compactions"] += r.get("compactions") or 0
            calls = ((r["cost"].get("theoretical") or {}).get("calls") or {})
            c["ruptures"] += calls.get("rupture", 0)
            if calls.get("rupture"):
                details.append(f"{where} : {calls['rupture']} rupture(s) du cache théorique")
            c["miss"] += len(r.get("server_misses") or [])
            tr = r.get("transports") or {}
            c["loop_on_sse"] += tr.get("loop_on_sse", 0)
            c["transport_failures"] += r.get("transport_failures_in_session") or 0
            c["unmatched_usage"] += tr.get("unmatched_usage", 0)
            c["unmatched_proxy"] += tr.get("unmatched_proxy", 0)
            for a in r.get("anomalies") or []:
                c["anomalies"] += 1
                details.append(f"{where} : anomalie « {a} »")
        logs = sorted(run_dir.glob("tasks/*/run-*/proxy.ndjson"))
        for path in logs + [run_dir / "warmup" / "proxy.ndjson"]:
            task_log = path in logs
            entries = ndjson(path)
            requests = [e for e in entries if e.get("event") == "request"]
            c["hors_101_200"] += sum(str(e.get("status")) not in ("101", "200") for e in requests)
            for e in entries:
                if e.get("event") == "response_error":
                    c["response_error"] += 1
                    details.append(f"{where} : `response_error` {e.get('error_type')}/"
                                   f"{e.get('error_code')} ({'tâche' if task_log else 'chauffe'})")
            if not task_log:
                continue
            full = [e for e in requests if e.get("generate") is not False
                    and (e.get("transport") == "sse" or e.get("continuation") is False)]
            c["full_after_first"] += max(0, len(full) - 1)
            if len(full) > 1:
                details.append(f"{where} : {len(full) - 1} corps complet(s) hors premier appel "
                               f"(transports {dict(Counter(e.get('transport') for e in full[1:]))})")
            pending = None
            for e in entries:
                if e.get("event") == "request":
                    pending = e["ts"]
                elif e.get("event") == "response":
                    if pending is not None:
                        durations[side].append(((e["ts"] - pending) / 1000, where, e.get("status")))
                        pending = None
                    if e.get("status") != "completed":
                        c["non_completed"] += 1
                        details.append(f"{where} : réponse {e.get('kind')} ({e.get('status')})")
                        continue
                    tokens = (e.get("usage") or {}).get("reasoning_tokens")
                    if tokens:
                        c["reasoning_n"] += 1
                        c["reasoning_sum"] += tokens
                    c["completed"] += 1
        for path in sorted(run_dir.glob("tasks/*/run-*/sessions/**/*.usage.ndjson")):
            c["stop_error"] += sum(e.get("stopReason") == "error" for e in ndjson(path))
        for path in sorted(run_dir.rglob("*")):
            if "workspace" in path.parts or not path.is_file() or path.suffix not in TEXT:
                continue
            if QUOTA.search(path.read_text(errors="replace")):
                c["quota"] += 1
                details.append(f"{where} : erreur de quota dans `{path.relative_to(run_dir)}`")

    sides = sorted(count)
    cell = {
        "statuts": lambda s: ", ".join(f"{k} {v}" for k, v in sorted(statuses[s].items())),
        "sse": lambda s: f"{count[s]['loop_on_sse']}, {count[s]['transport_failures']}",
        "unmatched": lambda s: f"{count[s]['unmatched_usage']}, {count[s]['unmatched_proxy']}",
        "reasoning": lambda s: (f"{count[s]['reasoning_n']} sur {count[s]['completed']} "
                                f"({count[s]['reasoning_sum']})"),
    }
    print(f"# Contrôles de {args.ab_dir.name} ({len(plan['arms'])} bras, toutes unités)\n")
    print("| Contrôle | " + " | ".join(sides) + " |")
    print("|---|" + "---|" * len(sides))
    for key, label in ROWS:
        print(f"| {label} | " + " | ".join(cell.get(key, lambda s, k=key: str(count[s][k]))(s)
                                          for s in sides) + " |")
    print("\n## Détails\n")
    print("\n".join(f"- {d}" for d in details) or "- aucun")
    print("\n## Appels les plus longs (requête → réponse, au proxy)\n")
    for side in sides:
        top = sorted(durations[side], key=lambda x: -x[0])[: args.top]
        print(f"- bras {side} ({len(durations[side])} appels) : "
              + " ; ".join(f"{t:.1f} s, {w}, {st}" for t, w, st in top))


if __name__ == "__main__":
    main()
