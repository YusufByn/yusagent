#!/usr/bin/env python3
"""Statistiques d'un A/B apparié du smoke bench (A = référence, B = variante).

Usage :
    python3 -I bench/smoke/tools/ab_stats.py --plan <runs/ab-…/ab.json> [--baseline <fichier>]
    python3 -I bench/smoke/tools/ab_stats.py [--baseline <fichier>] <A1> <B1> [<A2> <B2> ...]

Deux formes d'A/B :
  - entrelacé par tâche (`tools/ab.py`, `--plan`) : un run par bras, par tâche et par passe ;
    une unité = (passe, tâche), ses deux bras lancés dos à dos ;
  - par passes complètes (forme du 06/11, runs donnés par paires A B) : une unité = (paire,
    tâche), chaque run porte toutes les tâches.

Métrique de décision : coût à cache théorique (`cost.theoretical_total` de run.py : miss
serveur retirés, ruptures de préfixe du harness comptées). Coût rapporté et pessimiste,
résolution et tours sont publiés à côté.

Règle d'écart sur erreur provider (README, « A/B : erreurs provider », validée le
11/11/2026), appliquée automatiquement : en A/B entrelacé, une erreur provider sur un bras
écarte l'unité (la tâche de cette passe, ses deux bras) ; par passes complètes, elle écarte
la paire entière. Erreurs provider : statut `stalled` (garde-fou d'inactivité), `timeout`
sans activité des journaux (`proxy.ndjson`, `.usage.ndjson`) pendant au moins
`--stall-gap` secondes avant l'arrêt (flux bloqué), réponse en `stopReason` `error` (session
principale ou sous-agent), requête proxy à statut autre que 101 ou 200 (429…), lancement de
`run.py` en échec (A/B entrelacé). Les autres incidents (`timeout` d'une tâche longue,
`agent_error` ou `harness_error` sans erreur provider) sont listés sans écarter l'unité.

Publie, en Markdown sur la sortie standard :
  - unités retenues et écartées (avec la cause) ;
  - par tâche : moyenne de chaque bras sur ses unités retenues, écart apparié B − A ;
  - écart agrégé (somme B / somme A − 1) et moyenne des écarts appariés en $, IC 95 % par
    bootstrap sur les tâches, pour le coût théorique, rapporté et pessimiste, sur les tâches
    stables de la baseline (si donnée) et sur toutes ;
  - par passe : sommes des unités retenues, écart ;
  - par bras : résolution, tours, tokens, cache lu rapporté et théorique, miss serveur ;
  - invalidations journalisées par cause ; incidents ; surcoût par lancement (A/B entrelacé).

Bootstrap : percentiles 2,5 et 97,5, `--draws` tirages (10 000), graine `--seed` (0) :
sortie reproductible. `--no-exclusion` (sensibilité) garde les unités écartées.
Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
import random
import statistics
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
DRAWS = 10_000
SEED = 0
STALL_GAP_S = 300  # timeout « flux bloqué » : aucune activité des journaux pendant ce délai
MEASURES = {"théorique": "theoretical_total", "rapporté": "total",
            "pessimiste": "pessimistic_total"}
DECISION = "théorique"


def pct(a: float, b: float) -> float | None:
    return (b - a) / a * 100 if a else None


def fmt_pct(value: float | None) -> str:
    return "n/a" if value is None else f"{value:+.1f}"


def ci(values: list[float]) -> tuple[float, float]:
    values = sorted(values)
    return values[int(0.025 * len(values))], values[int(0.975 * len(values)) - 1]


def bootstrap(items: list, stat, draws: int, seed: int) -> tuple[float, float]:
    rng = random.Random(seed)
    samples = []
    for _ in range(draws):
        value = stat([rng.choice(items) for _ in items])
        if value is not None:
            samples.append(value)
    return ci(samples) if samples else (float("nan"), float("nan"))


def read_ndjson(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def load_report(run: Path) -> dict:
    report = json.loads((run / "report.json").read_text())
    report["_dir"] = run
    return report


def require_theoretical(report: dict) -> None:
    missing = [r["task"] for r in report["results"]
               if r["cost"].get("theoretical_total") is None and r["calls"]["total"]]
    if missing:
        raise SystemExit(f"{report['_dir'].name} : pas de coût à cache théorique ({missing}) : "
                         f"python3 -I bench/smoke/run.py --rereport {report['_dir']}")


# --- Incidents -------------------------------------------------------------------------


def task_dir(run: Path, result: dict) -> Path:
    return run / "tasks" / result["task"] / f"run-{result['run']}"


def last_activity(out: Path) -> tuple[float | None, float | None]:
    """(dernière activité, fin) d'un run de tâche en ms : dernière requête proxy ou fin
    d'appel du journal d'usage ; fin = dernier événement du journal du proxy."""
    proxy = read_ndjson(out / "proxy.ndjson") if (out / "proxy.ndjson").exists() else []
    usage = [r for p in sorted((out / "sessions").rglob("*.usage.ndjson")) for r in read_ndjson(p)]
    marks = [e["ts"] for e in proxy if e.get("event") == "request"]
    marks += [r["startMs"] + (r.get("durationMs") or 0) for r in usage if r.get("startMs")]
    end = max((e["ts"] for e in proxy), default=None)
    return (max(marks) if marks else None), end


def incidents(run: Path, result: dict, stall_gap: float) -> list[tuple[str, bool]]:
    """Incidents d'un run de tâche : (cause, erreur provider ?)."""
    out = task_dir(run, result)
    found: list[tuple[str, bool]] = []
    status = result["status"]
    if status == "stalled":
        found.append(("flux bloqué (garde-fou d'inactivité, statut `stalled`)", True))
    elif status == "timeout":
        last, end = last_activity(out)
        gap = (end - last) / 1000 if last is not None and end is not None else None
        if gap is not None and gap >= stall_gap:
            found.append((f"flux bloqué (timeout, aucune activité des journaux pendant "
                          f"{gap:.0f} s avant l'arrêt)", True))
        else:
            found.append((f"timeout, tâche longue (dernière activité {gap if gap is None else round(gap)}"
                          " s avant l'arrêt)", False))
    for path in sorted((out / "sessions").rglob("*.jsonl")):
        for entry in read_ndjson(path):
            message = entry.get("message")
            if isinstance(message, dict) and message.get("stopReason") == "error":
                kind = "sous-agent" if "subagents" in path.parts else "principale"
                found.append((f"session {kind} `{path.stem[:13]}`, erreur provider "
                              f"« {message.get('errorMessage')} »", True))
    if (out / "proxy.ndjson").exists():
        for entry in read_ndjson(out / "proxy.ndjson"):
            if entry.get("event") == "request" and entry.get("status") not in ("101", "200"):
                found.append((f"requête proxy statut {entry.get('status')}", True))
            # YA-14 : fin de réponse lue au proxy (runs postérieurs à l'adaptateur Codex CLI).
            elif entry.get("event") == "response_error":
                found.append((f"erreur du serveur au proxy (statut {entry.get('status')}, "
                              f"{entry.get('error_type')}, {entry.get('error_code')})", True))
            elif entry.get("event") == "response" and entry.get("kind") == "response.failed":
                found.append(("réponse en échec au proxy (`response.failed`)", True))
    # Bras Codex CLI (YA-14) : erreurs de la sortie `--json`, relancées ou non, et tour en échec.
    if result.get("harness") == "codex" and (out / "out.jsonl").exists():
        for event in read_ndjson(out / "out.jsonl"):
            if event.get("type") == "error":
                found.append((f"Codex, erreur « {str(event.get('message'))[:120]} »", True))
            elif event.get("type") == "turn.failed":
                message = (event.get("error") or {}).get("message")
                found.append((f"Codex, tour en échec « {str(message)[:120]} »", True))
    if status in ("agent_error", "harness_error") and not any(p for _, p in found):
        found.append((f"statut `{status}` sans erreur provider", False))
    return found


# --- Unités ------------------------------------------------------------------------------


def one_result(report: dict, task: str) -> dict:
    found = [r for r in report["results"] if r["task"] == task]
    if len(found) != 1:
        raise SystemExit(f"{report['_dir'].name} : {len(found)} résultat(s) pour {task}, 1 attendu")
    return found[0]


def units_from_plan(plan_path: Path, stall_gap: float) -> tuple[list[dict], dict]:
    """Unités (passe, tâche) d'un A/B entrelacé ; écart par unité."""
    ab = json.loads(plan_path.read_text())
    units: dict[tuple[int, str], dict] = {}
    for arm in ab["arms"]:
        key = (arm["pass"], arm["task"])
        unit = units.setdefault(key, {"pass": arm["pass"], "task": arm["task"],
                                      "order": arm["order"], "arms": {}, "causes": [],
                                      "notes": []})
        run_dir = REPO / arm["run_dir"] if arm.get("run_dir") else None
        if arm["exit"] != 0 or run_dir is None or not (run_dir / "report.json").exists():
            unit["causes"].append(f"bras {arm['arm']} : lancement en échec (code {arm['exit']})")
            continue
        report = load_report(run_dir)
        require_theoretical(report)
        result = one_result(report, arm["task"])
        unit["arms"][arm["arm"]] = {"result": result, "run": run_dir, "report": report}
        for cause, provider in incidents(run_dir, result, stall_gap):
            (unit["causes"] if provider else unit["notes"]).append(f"bras {arm['arm']} : {cause}")
    for key, unit in units.items():
        missing = {"A", "B"} - set(unit["arms"]) - {c.split()[1] for c in unit["causes"]}
        for side in sorted(missing):
            unit["causes"].append(f"bras {side} : absent du plan (A/B interrompu)")
    return sorted(units.values(), key=lambda u: (u["pass"], u["task"])), ab


def units_from_pairs(runs: list[Path], stall_gap: float) -> tuple[list[dict], None]:
    """Unités (paire, tâche) d'un A/B par passes complètes ; écart par paire entière."""
    reports = [load_report(r) for r in runs]
    for report in reports:
        require_theoretical(report)
    common = sorted(set.intersection(*({r["task"] for r in rep["results"]} for rep in reports)))
    units = []
    for i, (a, b) in enumerate(zip(reports[0::2], reports[1::2]), 1):
        pair_causes = []
        pair_units = []
        for task in common:
            unit = {"pass": i, "task": task, "order": "AB", "arms": {}, "causes": [],
                    "notes": []}
            for side, rep in (("A", a), ("B", b)):
                result = one_result(rep, task)
                unit["arms"][side] = {"result": result, "run": rep["_dir"], "report": rep}
                for cause, provider in incidents(rep["_dir"], result, stall_gap):
                    text = f"bras {side} : {task} : {cause}"
                    (pair_causes if provider else unit["notes"]).append(text)
            pair_units.append(unit)
        for unit in pair_units:
            unit["causes"] = [f"paire {i} écartée entière ({c})" for c in pair_causes]
        units += pair_units
    return units, None


# --- Mesures -----------------------------------------------------------------------------


def cost(result: dict, measure: str) -> float:
    value = result["cost"].get(MEASURES[measure])
    return value if value is not None else 0.0


def tokens(result: dict, cat: str) -> float:
    return result["tokens"].get(cat) or 0


def per_task(units: list[dict]) -> dict[str, dict]:
    """Moyenne de chaque bras sur les unités retenues d'une tâche."""
    tasks: dict[str, dict] = {}
    for task in sorted({u["task"] for u in units}):
        kept = [u for u in units if u["task"] == task]
        row = {"units": len(kept)}
        for side in ("A", "B"):
            results = [u["arms"][side]["result"] for u in kept]
            for m in MEASURES:
                row[side, m] = statistics.mean(cost(r, m) for r in results)
            row[side, "resolved"] = sum(r["resolved"] for r in results)
            row[side, "turns"] = statistics.mean(r["calls"]["turns"] for r in results)
        tasks[task] = row
    return tasks


def aggregate_lines(table: dict[str, dict], tasks: list[str], label: str, draws: int,
                    seed: int) -> list[str]:
    lines = []
    if not tasks:
        return [f"- {label} : aucune tâche retenue"]
    for m in MEASURES:
        def ratio(sample, m=m):
            return pct(sum(table[t]["A", m] for t in sample), sum(table[t]["B", m] for t in sample))

        def mean_diff(sample, m=m):
            return statistics.mean(table[t]["B", m] - table[t]["A", m] for t in sample)

        lo, hi = bootstrap(tasks, ratio, draws, seed)
        dlo, dhi = bootstrap(tasks, mean_diff, draws, seed)
        rels = [pct(table[t]["A", m], table[t]["B", m]) for t in tasks]
        rels = [r for r in rels if r is not None]
        bold = "**" if m == DECISION else ""
        lines.append(f"- {bold}{label}, {m}{' (décision)' if m == DECISION else ''} : "
                     f"{fmt_pct(ratio(tasks))} % [{lo:+.1f} ; {hi:+.1f}]{bold} ; écart apparié "
                     f"moyen {mean_diff(tasks):+.4f} $ [{dlo:+.4f} ; {dhi:+.4f}] ; médiane des "
                     f"écarts par tâche {fmt_pct(statistics.median(rels) if rels else None)} %")
    return lines


def arm_measures(units: list[dict], side: str) -> dict:
    """Sommes sur les unités retenues, moyennées par passe."""
    by_pass: dict[int, list[dict]] = {}
    for u in units:
        by_pass.setdefault(u["pass"], []).append(u["arms"][side]["result"])

    def per_pass(f):
        return statistics.mean(sum(f(r) for r in rs) for rs in by_pass.values())

    def sent(r):
        return tokens(r, "input") + tokens(r, "cacheRead") + tokens(r, "cacheWrite")

    return {"resolved": {p: sum(r["resolved"] for r in rs) for p, rs in sorted(by_pass.items())},
            "turns": per_pass(lambda r: r["calls"]["turns"]),
            "sent": per_pass(sent), "input": per_pass(lambda r: tokens(r, "input")),
            "cacheRead": per_pass(lambda r: tokens(r, "cacheRead")),
            "cacheRead_th": per_pass(lambda r: (r["cost"].get("theoretical") or {})
                                     .get("cacheRead") or 0),
            "cacheWrite": per_pass(lambda r: tokens(r, "cacheWrite")),
            "misses": per_pass(lambda r: len(r.get("server_misses", []))),
            "sub_cost": per_pass(lambda r: (r.get("subagents") or {}).get("cost") or 0),
            "cost": per_pass(lambda r: cost(r, "rapporté"))}


def edit_totals(units: list[dict], side: str) -> dict:
    """Sommes du bras sur les unités retenues (YA-03) : edits (`metrics.edit_attempts`),
    patchs refusés hors workspace, cellules en erreur d'appel. Un rapport antérieur à YA-03
    n'a pas `edits` : compté 0."""
    totals = {"attempts": 0, "applied": 0, "refused": 0, "cell_errors": 0, "syntax_errors": 0}
    for u in units:
        r = u["arms"][side]["result"]
        edits = r.get("edits") or {}
        totals["attempts"] += edits.get("attempts") or 0
        totals["applied"] += edits.get("applied") or 0
        totals["refused"] += (edits.get("apply_patch") or {}).get("refused_outside_workspace") or 0
        totals["cell_errors"] += (r.get("cells") or {}).get("errors") or 0
        totals["syntax_errors"] += (r.get("cells") or {}).get("syntax_errors") or 0
    return totals


def first_try(totals: dict) -> str:
    if not totals["attempts"]:
        return "aucun edit"
    rate = totals["applied"] / totals["attempts"] * 100
    return f"{totals['applied']}/{totals['attempts']} ({rate:.1f} %)"


def invalidations(root: Path) -> tuple[dict[str, int], dict[str, list[int]]]:
    """Invalidations journalisées sous `root`, par cause, et sessions touchées par le
    masquage (principales, sous-agents : [touchées, total])."""
    causes: dict[str, int] = {}
    touched = {"principales": [0, 0], "sous-agents": [0, 0]}
    for path in sorted(root.rglob("*.usage.ndjson")):
        hit = False
        for record in read_ndjson(path):
            cause = (record.get("invalidation") or {}).get("cause")
            if cause:
                causes[cause] = causes.get(cause, 0) + 1
                hit = hit or cause == "masking"
        side = touched["sous-agents" if "subagents" in path.parts else "principales"]
        side[0] += hit
        side[1] += 1
    return causes, touched


def merge_invalidations(parts: list[tuple[dict, dict]]) -> str:
    causes: dict[str, int] = {}
    touched = {"principales": [0, 0], "sous-agents": [0, 0]}
    for c, t in parts:
        for k, n in c.items():
            causes[k] = causes.get(k, 0) + n
        for k, (hit, total) in t.items():
            touched[k][0] += hit
            touched[k][1] += total
    return ((", ".join(f"{c} {n}" for c, n in sorted(causes.items())) or "aucune")
            + " ; sessions touchées par le masquage : "
            + ", ".join(f"{k} {v[0]}/{v[1]}" for k, v in touched.items()))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plan", type=Path, help="ab.json d'un A/B entrelacé (tools/ab.py)")
    parser.add_argument("--baseline", type=Path, help="tâches stables : `aggregate.tasks`")
    parser.add_argument("--draws", type=int, default=DRAWS)
    parser.add_argument("--seed", type=int, default=SEED)
    parser.add_argument("--stall-gap", type=float, default=STALL_GAP_S,
                        help=f"timeout classé « flux bloqué » au-delà (s, défaut {STALL_GAP_S})")
    parser.add_argument("--no-exclusion", action="store_true",
                        help="sensibilité : garde les unités touchées par une erreur provider")
    parser.add_argument("--verdict", choices=["ya14-codex"], default=None,
                        help="règle de verdict fixée avant le run (ya14-codex : "
                             "bench/smoke/ab/ya14-codex-plan.md)")
    parser.add_argument("runs", type=Path, nargs="*", help="A1 B1 A2 B2 ... (passes complètes)")
    args = parser.parse_args()
    if bool(args.plan) == bool(args.runs):
        parser.error("--plan ou des paires de runs (A1 B1 ...), pas les deux")
    if args.runs and len(args.runs) % 2:
        parser.error("nombre pair de runs attendu (A1 B1 A2 B2 ...)")
    if args.plan:
        units, ab = units_from_plan(args.plan, args.stall_gap)
        form = "entrelacé par tâche"
    else:
        units, ab = units_from_pairs(args.runs, args.stall_gap)
        form = "par passes complètes"
    dropped = [u for u in units if u["causes"]]
    kept = units if args.no_exclusion else [u for u in units if not u["causes"]]
    stable = (json.loads(args.baseline.read_text())["aggregate"]["tasks"] if args.baseline
              else None)
    table = per_task(kept)
    tasks = sorted(table)
    sets = [("toutes", tasks)]
    if stable is not None:
        sets.insert(0, ("stables", [t for t in stable if t in table]))

    out = [f"A/B {form} ; unités (passe, tâche) : {len(units)}, retenues {len(kept)}, écartées "
           f"{0 if args.no_exclusion else len(dropped)} ; tâches retenues : {len(tasks)}"
           + (f" (stables : {len(sets[0][1])})" if stable is not None else "")]
    if ab is not None:
        out += [f"Graine de l'ordre des bras : `{ab['seed']}` ({ab['seed_source']}) ; écart de "
                "réglages (A → B) : " + (", ".join(f"`{k}` {json.dumps(v[0])} → "
                                                   f"{json.dumps(v[1])}"
                                                   for k, v in ab["settings_diff"].items())
                                         or "aucun (A/A)")
                + f" ; garde-fou d'inactivité {ab.get('idle_timeout_s')} s"]
    out += [f"Métrique de décision : coût à cache théorique. Bootstrap sur les tâches, "
            f"{args.draws} tirages, graine {args.seed}.", "", "## Unités écartées (erreur provider)", ""]
    if args.no_exclusion:
        out.append("- **sensibilité (`--no-exclusion`)** : unités gardées malgré leurs erreurs "
                   "provider, listées ci-dessous")
    groups: dict[tuple, list[str]] = {}
    for u in dropped:
        groups.setdefault((u["pass"], tuple(u["causes"])), []).append(u["task"])
    for (p, causes), names in groups.items():
        whole = ab is None and len(names) == len([u for u in units if u["pass"] == p])
        where = "toutes ses tâches" if whole else ", ".join(names)
        out.append(f"- passe {p} ({where}) : " + "; ".join(causes))
    if not groups:
        out.append("- aucune")

    out += ["", "## Par tâche", "",
            "Moyenne de chaque bras sur les unités retenues de la tâche.", "",
            "| Tâche | Stable | Unités | A th. $ | B th. $ | Écart th. % | A $ | B $ | Écart % | "
            "A pess. $ | B pess. $ | Écart pess. % | Résolue A | Résolue B | Tours A | Tours B |",
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for t in tasks:
        r = table[t]
        cells = []
        for m in MEASURES:
            cells += [f"{r['A', m]:.4f}", f"{r['B', m]:.4f}", fmt_pct(pct(r["A", m], r["B", m]))]
        out.append(f"| {t} | {'-' if stable is None else ('oui' if t in stable else 'non')} | "
                   f"{r['units']} | " + " | ".join(cells)
                   + f" | {r['A', 'resolved']}/{r['units']} | {r['B', 'resolved']}/{r['units']} | "
                   f"{r['A', 'turns']:.1f} | {r['B', 'turns']:.1f} |")

    out += ["", "## Écart agrégé", "",
            "Somme B / somme A − 1 sur les tâches (moyennes par tâche ci-dessus), et moyenne des "
            "écarts appariés B − A en $ ; IC 95 % par bootstrap sur les tâches.", ""]
    for name, subset in sets:
        out += aggregate_lines(table, subset, name, args.draws, args.seed)

    out += ["", "## Par passe", "", "Sommes sur les unités retenues de la passe.", "",
            "| Passe | Unités | Coût | A $ | B $ | Écart % |", "|---|---|---|---|---|---|"]
    for p in sorted({u["pass"] for u in kept}):
        pu = [u for u in kept if u["pass"] == p]
        for m in MEASURES:
            ca = sum(cost(u["arms"]["A"]["result"], m) for u in pu)
            cb = sum(cost(u["arms"]["B"]["result"], m) for u in pu)
            out.append(f"| {p} | {len(pu)} | {m} | {ca:.4f} | {cb:.4f} | {fmt_pct(pct(ca, cb))} |")

    if kept:
        ma, mb = arm_measures(kept, "A"), arm_measures(kept, "B")
        out += ["", "## Par bras", "",
                "Sommes sur les unités retenues, moyennées par passe (résolution : par passe).", "",
                "| Mesure | A | B | Écart % |", "|---|---|---|---|",
                f"| Tâches résolues par passe | {', '.join(map(str, ma['resolved'].values()))} "
                f"(total {sum(ma['resolved'].values())}) | "
                f"{', '.join(map(str, mb['resolved'].values()))} (total "
                f"{sum(mb['resolved'].values())}) | |"]
        for key, label in (("turns", "Tours de la session principale"),
                           ("sent", "Tokens envoyés (input + cacheRead + cacheWrite)"),
                           ("input", "Input non caché"), ("cacheRead", "Cache lu (rapporté)"),
                           ("cacheRead_th", "Cache lu (théorique)"),
                           ("cacheWrite", "Cache écrit"), ("misses", "Miss serveur")):
            out.append(f"| {label} | {ma[key]:.1f} | {mb[key]:.1f} | "
                       f"{fmt_pct(pct(ma[key], mb[key]))} |")
        share = [m["sub_cost"] / m["cost"] * 100 if m["cost"] else 0 for m in (ma, mb)]
        out.append(f"| Part du coût rapporté des sous-agents | {share[0]:.1f} % | "
                   f"{share[1]:.1f} % | |")
        ea, eb = edit_totals(kept, "A"), edit_totals(kept, "B")
        out += [f"| Edits appliqués au premier essai (total) | {first_try(ea)} | "
                f"{first_try(eb)} | |",
                f"| Patchs refusés hors workspace (total) | {ea['refused']} | {eb['refused']} | |",
                f"| Cellules en erreur d'appel (total, dont SyntaxError) | {ea['cell_errors']} "
                f"({ea['syntax_errors']}) | {eb['cell_errors']} ({eb['syntax_errors']}) | |"]

    out += ["", "## Invalidations journalisées", ""]
    if ab is None:
        runs: list[Path] = []
        for u in units:
            for side in ("A", "B"):
                if u["arms"][side]["run"] not in runs:
                    runs.append(u["arms"][side]["run"])
        out += [f"- {run.name} : {merge_invalidations([invalidations(run / 'tasks')])}"
                for run in runs]
    else:
        for side in ("A", "B"):
            parts = [invalidations(task_dir(u["arms"][side]["run"], u["arms"][side]["result"]))
                     for u in units if side in u["arms"]]
            out.append(f"- bras {side} (toutes unités) : {merge_invalidations(parts)}")

    out += ["", "## Incidents", "",
            "Écartants (erreur provider) : section « Unités écartées ». Non écartants :", ""]
    notes = [f"- passe {u['pass']}, {u['task']} : " + "; ".join(u["notes"])
             for u in units if u["notes"]]
    out += notes or ["- aucun"]

    if ab is not None:
        o = ab.get("overhead") or {}
        out += ["", "## Surcoût par lancement", ""]
        out.append(f"- {o.get('measured', 0)} lancement(s) mesuré(s) : médiane "
                   f"{o.get('overhead_median_s')} s, total {o.get('overhead_total_s')} s sur "
                   f"{o.get('wall_total_s')} s ({(o.get('overhead_share') or 0) * 100:.1f} %)"
                   if o.get("measured") else "- aucun lancement mesuré")
    if args.verdict == "ya14-codex":
        out += ya14_codex_verdict(units, kept, args.draws, args.seed, ab)
    print("\n".join(out))


# --- Verdict YA-14 : yusAgent contre Codex CLI ------------------------------------------

YA14_MIN_RESOLVED_PASSES = 2   # tâche résolue pour un bras : au moins 2 passes valides résolues
YA14_RESOLUTION_GAP = 3        # écart de tâches résolues qui fait un verdict de résolution
YA14_MIN_COST_TASKS = 5        # tâches communes minimum pour un verdict de coût
YA14_COST_THRESHOLD = 10.0     # |écart| en % au-delà duquel le coût peut décider


def ya14_tables(kept: list[dict]) -> dict[str, dict]:
    """Par tâche : passes valides, passes résolues par bras."""
    rows: dict[str, dict] = {}
    for u in kept:
        row = rows.setdefault(u["task"], {"passes": 0, "A": 0, "B": 0, "units": []})
        row["passes"] += 1
        row["units"].append(u)
        for side in ("A", "B"):
            row[side] += bool(u["arms"][side]["result"]["resolved"])
    return rows


def ya14_codex_verdict(units: list[dict], kept: list[dict], draws: int, seed: int,
                       ab: dict | None) -> list[str]:
    """Règle fixée avant le run (`bench/smoke/ab/ya14-codex-plan.md`) : résolution d'abord
    (tâche résolue pour un bras à au moins 2 passes valides ; écart de 3 tâches ou plus), puis
    coût à cache théorique sur les tâches résolues à toutes leurs passes valides par les deux
    bras (5 au moins), B/A − 1 avec IC bootstrap sur les tâches ; le coût décide seulement si
    l'IC exclut 0 et |écart| > 10 %. A = yusAgent, B = Codex CLI. Vaut pour le modèle du run."""
    rows = ya14_tables(kept)
    judged = {t: r for t, r in rows.items() if r["passes"] >= YA14_MIN_RESOLVED_PASSES}
    short = sorted(set(rows) - set(judged))
    solved = {side: sorted(t for t, r in judged.items() if r[side] >= YA14_MIN_RESOLVED_PASSES)
              for side in ("A", "B")}
    gap = len(solved["B"]) - len(solved["A"])
    model = (ab or {}).get("model")
    lines = ["", "## Verdict YA-14 (yusAgent = A, Codex CLI = B)", "",
             f"Règle fixée avant le run : `bench/smoke/ab/ya14-codex-plan.md`. Vaut pour "
             f"`{model}` seulement.", "",
             "### Résolution", "",
             f"- Tâche résolue pour un bras : résolue à au moins {YA14_MIN_RESOLVED_PASSES} passes "
             f"valides. Tâches jugées (au moins {YA14_MIN_RESOLVED_PASSES} passes valides) : "
             f"{len(judged)} ; exclues (moins de passes valides) : {short or 'aucune'}",
             f"- A résout {len(solved['A'])} tâche(s) : {', '.join(solved['A']) or 'aucune'}",
             f"- B résout {len(solved['B'])} tâche(s) : {', '.join(solved['B']) or 'aucune'}",
             "", "| Tâche | Passes valides | Résolue A | Résolue B |", "|---|---|---|---|"]
    lines += [f"| {t} | {r['passes']} | {r['A']}/{r['passes']} | {r['B']}/{r['passes']} |"
              for t, r in sorted(rows.items())]
    if abs(gap) >= YA14_RESOLUTION_GAP:
        resolution = (f"B (Codex CLI) résout {gap} tâche(s) de plus" if gap > 0 else
                      f"A (yusAgent) résout {-gap} tâche(s) de plus")
    else:
        resolution = None
    lines += ["", f"- Écart B − A : {gap:+d} tâche(s) ; seuil {YA14_RESOLUTION_GAP} (fixé avant "
              "le run, pas un test) : " + (f"**{resolution}**" if resolution else
                                          "pas de verdict de résolution")]
    common = sorted(t for t, r in rows.items()
                    if r["A"] == r["passes"] and r["B"] == r["passes"] and r["passes"] >= 1)
    table = per_task([u for u in kept if u["task"] in common]) if common else {}
    lines += ["", "### Coût à cache théorique", "",
              f"- Tâches résolues à toutes leurs passes valides par les deux bras : {len(common)}"
              f" ({', '.join(common) or 'aucune'})"]
    cost_verdict = None
    if len(common) < YA14_MIN_COST_TASKS:
        lines.append(f"- Moins de {YA14_MIN_COST_TASKS} tâches : **pas de verdict de coût**")
    else:
        def ratio(sample):
            return pct(sum(table[t]["A", DECISION] for t in sample),
                       sum(table[t]["B", DECISION] for t in sample))
        value = ratio(common)
        lo, hi = bootstrap(common, ratio, draws, seed)
        excludes = lo > 0 or hi < 0
        decides = excludes and value is not None and abs(value) > YA14_COST_THRESHOLD
        if decides:
            cost_verdict = "B (Codex CLI) moins cher" if value < 0 else "A (yusAgent) moins cher"
        lines.append(f"- B/A − 1 : {fmt_pct(value)} % [{lo:+.1f} ; {hi:+.1f}] (IC 95 %, bootstrap "
                     f"sur les tâches, {draws} tirages, graine {seed}) ; IC qui exclut 0 : "
                     f"{'oui' if excludes else 'non'} ; |écart| > {YA14_COST_THRESHOLD:g} % : "
                     f"{'oui' if value is not None and abs(value) > YA14_COST_THRESHOLD else 'non'}"
                     f" : **{cost_verdict or 'non distinguable'}**")
        for m in MEASURES:
            if m == DECISION:
                continue
            lines.append(f"- {m} (affiché, ne décide pas) : {fmt_pct(pct(sum(table[t]['A', m] for t in common), sum(table[t]['B', m] for t in common)))} %")
    if resolution:
        final = f"résolution : {resolution} (coût affiché ci-dessus, ne décide pas)"
    elif len(common) < YA14_MIN_COST_TASKS:
        final = "pas de verdict de résolution, pas de verdict de coût"
    else:
        final = f"pas de verdict de résolution ; coût : {cost_verdict or 'non distinguable'}"
    lines += ["", f"**Verdict : {final}.**"]
    lines += ya14_display(kept)
    return lines


def ya14_display(kept: list[dict]) -> list[str]:
    """Affiché sans décider (plan YA-14) : par bras, sur les unités retenues."""
    def collect(side, f):
        return [v for u in kept for v in [f(u["arms"][side]["result"])] if v is not None]

    def med(values):
        return f"{statistics.median(values):.1f}" if values else "n/a"

    rows = [
        ("Coût rapporté par tâche résolue $",
         lambda side: (lambda c, n: f"{c / n:.4f}" if n else "n/a")(
             sum(cost(u["arms"][side]["result"], "rapporté") for u in kept),
             sum(u["arms"][side]["result"]["resolved"] for u in kept))),
        ("Premier prompt, tokens (médiane)",
         lambda side: med(collect(side, lambda r: r.get("first_prompt")))),
        ("Pic de contexte, tokens (médiane ; max)",
         lambda side: (lambda v: f"{med(v)} ; {max(v) if v else 'n/a'}")(
             collect(side, lambda r: r.get("context_peak")))),
        ("Tours de la session principale (médiane)",
         lambda side: med(collect(side, lambda r: r["calls"]["turns"]))),
        ("Durée de la phase agent s (médiane)",
         lambda side: med(collect(side, lambda r: (r.get("agent") or {}).get("seconds")))),
        ("Ruptures du cache théorique (total)",
         lambda side: str(sum(((r["cost"].get("theoretical") or {}).get("calls") or {})
                              .get("rupture", 0) for r in collect(side, lambda r: r)))),
        ("Compactions (total)",
         lambda side: str(sum(collect(side, lambda r: r.get("compactions") or 0)))),
        ("Appels prewarm (total)",
         lambda side: str(sum((r["calls"].get("by_kind") or {}).get("prewarm", 0)
                              for r in collect(side, lambda r: r)))),
        ("Quota de l'abonnement, dernier relevé % (Codex seul)",
         lambda side: (lambda v: f"{v[-1]}" if v else "n/a")(collect(
             side, lambda r: ((r.get("codex") or {}).get("rate_limits") or {})
             .get("primary_used_percent")))),
    ]
    lines = ["", "### Affiché sans décider", "", "| Mesure | A (yusAgent) | B (Codex CLI) |",
             "|---|---|---|"]
    lines += [f"| {label} | {f('A')} | {f('B')} |" for label, f in rows]
    lines.append("- Edits au premier essai, cache, miss serveur, sous-agents, incidents : sections "
                 "ci-dessus. Écarts de config avec chaque harness tel que livré : plan YA-14.")
    return lines


if __name__ == "__main__":
    main()
