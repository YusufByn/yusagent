#!/usr/bin/env python3
"""Statistiques d'un A/B apparié du smoke bench : paires de runs (A = référence, B = variante).

Usage :
    python3 -I bench/smoke/tools/ab_stats.py --baseline <fichier> <A1> <B1> [<A2> <B2> ...]

Lit les `report.json` (run.py) et la liste des tâches stables de la baseline (`aggregate.tasks`).
Publie, en Markdown sur la sortie standard :
  - par paire : coût rapporté et pessimiste, somme des tâches stables et des tâches communes,
    écart relatif B vs A ; moyenne des écarts et IC 95 % par bootstrap sur les paires ;
  - par tâche : coût moyen de chaque bras sur les passes, écart relatif ; IC 95 % de l'écart
    agrégé (rapport des sommes) par bootstrap sur les tâches ;
  - par bras : résolution, tours de la session principale, tokens envoyés (input + cacheRead
    + cacheWrite), cache lu et écrit, miss serveur, part du coût des sous-agents ;
  - par run : invalidations du journal d'usage par cause, sessions touchées par le masquage ;
  - incidents par run : tâches hors `resolved`/`failed`, réponses du provider en erreur
    (`stopReason` `error`, message), requêtes proxy à statut autre que 101 ou 200.

Bootstrap : percentiles 2,5 et 97,5, 10 000 tirages, graine fixe (résultat reproductible).
Avec peu de paires, l'IC sur les paires ne prend que quelques valeurs distinctes : il est
publié pour mémoire. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from pathlib import Path

DRAWS = 10_000
SEED = 0


def load(run: Path) -> dict:
    report = json.loads((run / "report.json").read_text())
    report["_name"] = run.name
    return report


def pct(a: float, b: float) -> float:
    return (b - a) / a * 100


def ci(values: list[float]) -> tuple[float, float]:
    values = sorted(values)
    return values[int(0.025 * len(values))], values[int(0.975 * len(values)) - 1]


def bootstrap(items: list, stat) -> tuple[float, float]:
    rng = random.Random(SEED)
    return ci([stat([rng.choice(items) for _ in items]) for _ in range(DRAWS)])


def cost(report: dict, tasks: list[str], key: str) -> float:
    return sum(report["per_task"][t][key] for t in tasks)


def arm(reports: list[dict], tasks: list[str]) -> dict:
    """Mesures d'un bras, sommées sur les tâches puis moyennées sur les passes."""
    def per_pass(f):
        return statistics.mean(sum(f(r["per_task"][t]) for t in tasks) for r in reports)

    def tok(cat):
        return lambda t: t["tokens_by_category_mean"][cat] or 0

    return {
        "resolved": [sum(r["per_task"][t]["resolved_rate"] >= 0.5 for t in tasks) for r in reports],
        "turns": per_pass(lambda t: t["turns_mean"]),
        "sent": per_pass(lambda t: tok("input")(t) + tok("cacheRead")(t) + tok("cacheWrite")(t)),
        "input": per_pass(tok("input")),
        "cacheRead": per_pass(tok("cacheRead")),
        "cacheWrite": per_pass(tok("cacheWrite")),
        "misses": per_pass(lambda t: t["server_misses"]),
        "sub_share": statistics.mean(r["totals"]["subagents"]["cost_share"] for r in reports),
    }


def incidents(run: Path, report: dict) -> list[str]:
    """Tâches hors `resolved`/`failed`, erreurs du provider, statuts proxy inattendus."""
    found = [f"{r['task']} : statut `{r['status']}`" for r in report["results"]
             if r["status"] not in ("resolved", "failed")]
    for path in sorted(run.glob("tasks/*/run-*/sessions/**/*.jsonl")):
        for line in path.read_text().splitlines():
            message = json.loads(line).get("message")
            if isinstance(message, dict) and message.get("stopReason") == "error":
                kind = "sous-agent" if "subagents" in path.parts else "principale"
                found.append(f"{path.parts[-5 if kind == 'sous-agent' else -4]} : session {kind} "
                             f"`{path.stem[:13]}`, erreur provider « {message.get('errorMessage')} »")
    for path in sorted(run.glob("tasks/*/run-*/proxy.ndjson")):
        for line in path.read_text().splitlines():
            r = json.loads(line)
            if r["event"] == "request" and r.get("status") not in ("101", "200"):
                found.append(f"{path.parts[-3]} : requête proxy statut {r.get('status')}")
    return found


def invalidations(run: Path) -> str:
    """Invalidations journalisées par cause, et sessions touchées (principales, sous-agents)."""
    causes: dict[str, int] = {}
    touched = {"principales": [0, 0], "sous-agents": [0, 0]}
    for path in run.glob("tasks/*/run-*/sessions/**/*.usage.ndjson"):
        hit = False
        for line in path.read_text().splitlines():
            cause = (json.loads(line).get("invalidation") or {}).get("cause")
            if cause:
                causes[cause] = causes.get(cause, 0) + 1
                hit = hit or cause == "masking"
        side = touched["sous-agents" if "subagents" in path.parts else "principales"]
        side[0] += hit
        side[1] += 1
    return (", ".join(f"{c} {n}" for c, n in sorted(causes.items())) or "aucune") + " ; sessions " \
        "touchées par le masquage : " + ", ".join(f"{k} {v[0]}/{v[1]}" for k, v in touched.items())


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("runs", type=Path, nargs="+", help="A1 B1 A2 B2 ...")
    args = parser.parse_args()
    if len(args.runs) % 2:
        parser.error("nombre pair de runs attendu (A1 B1 A2 B2 ...)")
    reports = [load(r) for r in args.runs]
    pairs = list(zip(reports[0::2], reports[1::2]))
    stable = json.loads(args.baseline.read_text())["aggregate"]["tasks"]
    common = sorted(set.intersection(*(set(r["per_task"]) for r in reports)))
    stable = [t for t in stable if t in common]
    sets = {"stables": stable, "toutes": common}
    measures = {"rapporté": "cost_mean", "pessimiste": "cost_pessimistic_mean"}

    out = [f"Paires : {len(pairs)} ; tâches stables : {len(stable)} ; tâches communes : "
           f"{len(common)}", "", "## Par paire", "",
           "| Paire | A | B | Tâches | Coût | A $ | B $ | Écart % |", "|---|---|---|---|---|---|---|---|"]
    for i, (a, b) in enumerate(pairs, 1):
        for sname, tasks in sets.items():
            for mname, key in measures.items():
                ca, cb = cost(a, tasks, key), cost(b, tasks, key)
                out.append(f"| {i} | {a['_name'][:16]} | {b['_name'][:16]} | {sname} | {mname} | "
                           f"{ca:.4f} | {cb:.4f} | {pct(ca, cb):+.1f} |")
    out += ["", "Moyenne des écarts par paire, IC 95 % par bootstrap sur les paires :", ""]
    for sname, tasks in sets.items():
        for mname, key in measures.items():
            d = [pct(cost(a, tasks, key), cost(b, tasks, key)) for a, b in pairs]
            lo, hi = bootstrap(d, statistics.mean)
            out.append(f"- {sname}, {mname} : {statistics.mean(d):+.1f} % [{lo:+.1f} ; {hi:+.1f}] "
                       f"(paires : {', '.join(f'{x:+.1f}' for x in d)})")

    out += ["", "## Par tâche", "", "Coût moyen sur les passes de chaque bras.", "",
            "| Tâche | Stable | A $ | B $ | Écart % | A pess. $ | B pess. $ | Écart pess. % | "
            "Résolue A | Résolue B |", "|---|---|---|---|---|---|---|---|---|---|"]
    mean_cost = {}
    for t in common:
        row = {}
        for side, idx in (("A", 0), ("B", 1)):
            for mname, key in measures.items():
                row[side, mname] = statistics.mean(p[idx]["per_task"][t][key] for p in pairs)
            row[side, "res"] = sum(p[idx]["per_task"][t]["resolved_rate"] >= 0.5 for p in pairs)
        mean_cost[t] = row
        out.append(f"| {t} | {'oui' if t in stable else 'non'} | {row['A', 'rapporté']:.4f} | "
                   f"{row['B', 'rapporté']:.4f} | {pct(row['A', 'rapporté'], row['B', 'rapporté']):+.1f} | "
                   f"{row['A', 'pessimiste']:.4f} | {row['B', 'pessimiste']:.4f} | "
                   f"{pct(row['A', 'pessimiste'], row['B', 'pessimiste']):+.1f} | "
                   f"{row['A', 'res']}/{len(pairs)} | {row['B', 'res']}/{len(pairs)} |")
    out += ["", "Écart agrégé (somme B / somme A − 1), IC 95 % par bootstrap sur les tâches :", ""]
    for sname, tasks in sets.items():
        for mname in measures:
            def agg(sample, m=mname):
                return pct(sum(mean_cost[t]["A", m] for t in sample),
                           sum(mean_cost[t]["B", m] for t in sample))
            per_task = [pct(mean_cost[t]["A", mname], mean_cost[t]["B", mname]) for t in tasks]
            lo, hi = bootstrap(tasks, agg)
            out.append(f"- {sname}, {mname} : {agg(tasks):+.1f} % [{lo:+.1f} ; {hi:+.1f}] ; "
                       f"médiane des écarts par tâche {statistics.median(per_task):+.1f} %")

    out += ["", "## Par bras", "",
            "Sommes sur les tâches communes, moyennées sur les passes (résolution : par passe).", "",
            "| Mesure | A | B | Écart % |", "|---|---|---|---|"]
    ma = arm([a for a, _ in pairs], common)
    mb = arm([b for _, b in pairs], common)
    out.append(f"| Tâches résolues par passe | {', '.join(map(str, ma['resolved']))} (total "
               f"{sum(ma['resolved'])}) | {', '.join(map(str, mb['resolved']))} (total "
               f"{sum(mb['resolved'])}) | |")
    for key, label in (("turns", "Tours de la session principale"),
                       ("sent", "Tokens envoyés (input + cacheRead + cacheWrite)"),
                       ("input", "Input non caché"), ("cacheRead", "Cache lu"),
                       ("cacheWrite", "Cache écrit"), ("misses", "Miss serveur")):
        change = f"{pct(ma[key], mb[key]):+.1f}" if ma[key] else "n/a"
        out.append(f"| {label} | {ma[key]:.1f} | {mb[key]:.1f} | {change} |")
    out.append(f"| Part du coût des sous-agents | {ma['sub_share'] * 100:.1f} % | "
               f"{mb['sub_share'] * 100:.1f} % | |")

    out += ["", "## Invalidations journalisées", ""]
    for run in args.runs:
        out.append(f"- {run.name} : {invalidations(run)}")

    out += ["", "## Incidents", ""]
    for run, report in zip(args.runs, reports):
        found = incidents(run, report)
        out.append(f"- {run.name} : " + ("aucun" if not found else "; ".join(found)))
    print("\n".join(out))


if __name__ == "__main__":
    main()
