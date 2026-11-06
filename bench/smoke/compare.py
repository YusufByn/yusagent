#!/usr/bin/env python3
"""Comparaison appariée de deux runs du smoke bench (T06, T07, YA-14).

Usage :
    python3 -I bench/smoke/compare.py <run A> <run B> [--baseline <fichier>] [--out <fichier>.md]

Par tâche : résolu A/B, Δ coût, Δ tokens par catégorie, Δ tours, miss serveur. Alertes
en tête (un run de PR en alerte est relancé 2 fois, décision du 04/11/2026) :
  - tâche stable en baseline (résolue à tous ses runs, 3 au moins) non résolue en B ;
  - tâche résolue en A et non en B ;
  - écart de coût ou de tokens de B par rapport à A au-delà du bruit mesuré de la tâche :
    2 fois son coefficient de variation en baseline, et au moins 5 %.
  - écart du coût agrégé (somme des coûts moyens des tâches stables de la baseline,
    présentes dans A et B et de même version) au-delà de max(2 × CV du total, 5 %), CV
    du total = `aggregate.cv` de la baseline (le plus grand de ses deux estimations).
La section du coût agrégé donne aussi le coût pessimiste des mêmes tâches (T08b : `input`
non caché au tarif d'écriture 30 min), avec le même seuil, à titre d'information : il ne
déclenche pas d'alerte.
Baseline : `baseline/<modèle de B>.json` (écrit par `baseline.py`) si `--baseline` n'est
pas donné. Sans baseline, seules les régressions de résolution sont signalées : les
écarts de coût et de tokens restent affichés, sans seuil.

Version des tâches : une tâche dont l'empreinte (`task_version`, run.py) diffère entre B
et A ou la baseline, ou y est inconnue (run sans empreinte : `run.py --rereport`), est
exclue de la comparaison, sans alerte, et signalée (« version de tâche différente »).
Les totaux restent ceux des runs complets.

Avec plusieurs runs par configuration, le taux de bascule et l'écart-type entre runs sont
publiés à côté de chaque écart ; une tâche compte comme résolue si elle l'est dans au
moins la moitié de ses runs.

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import metrics  # noqa: E402

CATEGORIES = ("input", "output", "cacheRead", "cacheWrite", "reasoning")
SAME_HEADER = ("provider", "model", "effort", "prices_version", "models_bundled_sha256", "tasks",
               "mode")
NOISE_FACTOR = 2.0  # écart signalé au-delà de 2 coefficients de variation de la baseline
NOISE_FLOOR_PCT = 5.0  # et au moins 5 %


def load(run: Path) -> dict:
    path = run / "report.json" if run.is_dir() else run
    report = json.loads(path.read_text())
    report["_dir"] = path.parent
    return report


def resolved(task: dict | None) -> bool | None:
    return None if task is None else task["resolved_rate"] >= 0.5


def delta(a, b) -> str:
    if a is None or b is None:
        return "null"
    return f"{b - a:+.4f}" if isinstance(a, float) or isinstance(b, float) else f"{b - a:+d}"


def pct(a, b) -> float | None:
    if a in (None, 0) or b is None:
        return None
    return (b - a) / a * 100


def fmt(value, digits: int = 4) -> str:
    if value is None:
        return "null"
    return f"{value:.{digits}f}" if isinstance(value, float) else str(value)


def total_tokens(report: dict) -> int:
    tokens = report["totals"]["tokens"]
    return sum(tokens[k] or 0 for k in ("input", "output", "cacheRead", "cacheWrite"))


def task_tokens_mean(task: dict) -> float:
    by = task["tokens_by_category_mean"]
    return sum(by.get(k) or 0 for k in ("input", "output", "cacheRead", "cacheWrite"))


def server_misses(report: dict, task: str) -> int:
    """Miss serveur d'une tâche : rapport, ou journaux du run s'il les précède (T07)."""
    found = report["per_task"].get(task, {}).get("server_misses")
    if found is not None:
        return found
    return sum(len(metrics.task_run_cache(d)["server_misses"])
               for d in sorted((report["_dir"] / "tasks" / task).glob("run-*")))


def task_trees(report: dict) -> dict[str, str | None]:
    """Empreinte de chaque tâche d'un run ; `None` si inconnue ou si ses runs en ont deux."""
    found: dict[str, set] = {}
    for r in report["results"]:
        found.setdefault(r["task"], set()).add((r.get("task_version") or {}).get("tree"))
    return {t: next(iter(v)) if len(v) == 1 else None for t, v in found.items()}


def version_exclusions(a: dict, b: dict, baseline: dict | None) -> dict[str, str]:
    """Tâches de B dont la version diffère de A ou de la baseline (ou y est inconnue)."""
    trees_a, trees_b = task_trees(a), task_trees(b)
    excluded = {}
    for task, tree_b in sorted(trees_b.items()):
        sides = [("A", trees_a[task])] if task in trees_a else []
        base = (baseline or {}).get("per_task", {}).get(task)
        if base is not None:
            sides.append(("baseline", base.get("task_tree")))
        if not sides:
            continue
        if tree_b is None:
            excluded[task] = "version inconnue en B"
            continue
        different = [f"{name} {tree[:12]}" for name, tree in sides
                     if tree is not None and tree != tree_b]
        unknown = [name for name, tree in sides if tree is None]
        if different:
            excluded[task] = (f"version de tâche différente ({', '.join(different)} ; "
                              f"B {tree_b[:12]})")
        elif unknown:
            excluded[task] = f"version inconnue en {', '.join(unknown)}"
    return excluded


def noise_threshold(baseline: dict | None, task: str, measure: str) -> tuple[float, str] | None:
    """Seuil en % d'une tâche pour `cost` ou `tokens` ; None sans baseline pour la tâche."""
    entry = (baseline or {}).get("per_task", {}).get(task)
    if entry is None:
        return None
    cv = (entry.get(measure) or {}).get("cv")
    if cv is None:
        return NOISE_FLOOR_PCT, "CV inconnu, plancher"
    return max(NOISE_FACTOR * cv * 100, NOISE_FLOOR_PCT), f"2 × CV {cv:.3f}"


def aggregate_check(pa: dict, pb: dict, baseline: dict | None,
                    excluded: dict[str, str]) -> dict | None:
    """Coût agrégé des tâches stables de la baseline, A contre B, et son seuil."""
    agg = (baseline or {}).get("aggregate")
    if not agg or agg.get("cv") is None:
        return None
    tasks = [t for t in agg["tasks"] if t in pa and t in pb and t not in excluded
             and pa[t]["cost_mean"] is not None and pb[t]["cost_mean"] is not None]
    if not tasks:
        return None
    cost_a = sum(pa[t]["cost_mean"] for t in tasks)
    cost_b = sum(pb[t]["cost_mean"] for t in tasks)
    limit = max(NOISE_FACTOR * agg["cv"] * 100, NOISE_FLOOR_PCT)
    # T08b : mêmes tâches, coût pessimiste ; `None` si un run n'en a pas (rapport antérieur :
    # `run.py --rereport`). Le seuil et le CV restent ceux du coût rapporté.
    pessimistic = [(pa[t].get("cost_pessimistic_mean"), pb[t].get("cost_pessimistic_mean"))
                   for t in tasks]
    known = all(a is not None and b is not None for a, b in pessimistic)
    pess_a = sum(a for a, _ in pessimistic) if known else None
    pess_b = sum(b for _, b in pessimistic) if known else None
    return {"tasks": tasks, "missing": [t for t in agg["tasks"] if t not in tasks],
            "cost_a": cost_a, "cost_b": cost_b, "change": pct(cost_a, cost_b), "limit": limit,
            "cv_independent": agg.get("cv_independent"), "cv_passes": agg.get("cv_passes"),
            "cv": agg["cv"], "pessimistic_a": pess_a, "pessimistic_b": pess_b,
            "pessimistic_change": pct(pess_a, pess_b),
            "pessimistic_missing": [t for t, (a, b) in zip(tasks, pessimistic)
                                    if a is None or b is None]}


def pessimistic_line(agg: dict) -> str:
    """Coût pessimiste agrégé (T08b), mêmes tâches et même seuil que le coût rapporté ;
    information seule, sans alerte."""
    if agg["pessimistic_change"] is None and agg["pessimistic_missing"]:
        return ("- Coût pessimiste : non calculé (sans coût pessimiste : "
                f"{', '.join(agg['pessimistic_missing'])} ; `run.py --rereport <run>`)")
    beyond = (agg["pessimistic_change"] is not None
              and abs(agg["pessimistic_change"]) > agg["limit"])
    return (f"- Coût pessimiste (input non caché au tarif `{metrics.PESSIMISTIC_INPUT_PRICE}`) : "
            f"{fmt(agg['pessimistic_a'])} → {fmt(agg['pessimistic_b'])} "
            f"({fmt(agg['pessimistic_change'], 1)} %), seuil {agg['limit']:.1f} % "
            f"(celui du coût rapporté) : {'au-delà' if beyond else 'en deçà'}, sans alerte")


def compare(a: dict, b: dict, baseline: dict | None, baseline_name: str | None) -> str:
    ha, hb = a["header"], b["header"]
    pa, pb = a["per_task"], b["per_task"]
    tasks = sorted(set(pa) | set(pb))
    excluded = version_exclusions(a, b, baseline)
    signals = []
    for t in tasks:
        if t in excluded:
            continue
        base = (baseline or {}).get("per_task", {}).get(t)
        if base and base.get("stable") and pb.get(t) is not None and pb[t]["resolved_rate"] < 1:
            signals.append(f"**Tâche stable non résolue** : {t} (résolue à {base['runs']}/"
                           f"{base['runs']} runs en baseline, taux {pb[t]['resolved_rate']:.2f} en B)")
        elif resolved(pa.get(t)) and resolved(pb.get(t)) is False:
            signals.append(f"**Régression** : {t} résolue en A, non résolue en B")
    for t in tasks:
        ta, tb = pa.get(t), pb.get(t)
        if ta is None or tb is None or t in excluded:
            continue
        for measure, va, vb in (("cost", ta["cost_mean"], tb["cost_mean"]),
                                ("tokens", task_tokens_mean(ta), task_tokens_mean(tb))):
            change, limit = pct(va, vb), noise_threshold(baseline, t, measure)
            if change is not None and limit is not None and abs(change) > limit[0]:
                name = "coût" if measure == "cost" else "tokens"
                way = "Hausse" if change > 0 else "Baisse"
                signals.append(f"**{way} de {name}** : {t} {change:+.1f} % (seuil {limit[0]:.1f} %,"
                               f" {limit[1]})")
    agg = aggregate_check(pa, pb, baseline, excluded)
    if agg and agg["change"] is not None and abs(agg["change"]) > agg["limit"]:
        way = "Hausse" if agg["change"] > 0 else "Baisse"
        signals.append(f"**{way} du coût agrégé** : {agg['change']:+.1f} % sur {len(agg['tasks'])} "
                       f"tâche(s) stable(s) (seuil {agg['limit']:.1f} %, 2 × CV {agg['cv']:.3f})")
    mismatches = [k for k in SAME_HEADER if ha.get(k) != hb.get(k)]
    if baseline and baseline.get("header", {}).get("model") != hb.get("model"):
        mismatches.append("modèle de la baseline")
    cost_a, cost_b = a["totals"]["cost"], b["totals"]["cost"]
    tok_a, tok_b = total_tokens(a), total_tokens(b)
    lines = ["# Comparaison smoke bench", "",
             f"- A : `{ha.get('commit')}` (binaire `{ha.get('binary_sha256', '')[:12]}`), "
             f"réglages {json.dumps(ha.get('settings_non_default'))}, {ha.get('runs')} run(s)",
             f"- B : `{hb.get('commit')}` (binaire `{hb.get('binary_sha256', '')[:12]}`), "
             f"réglages {json.dumps(hb.get('settings_non_default'))}, {hb.get('runs')} run(s)",
             f"- Modèle {ha.get('model')} / {hb.get('model')}, effort {ha.get('effort')} / "
             f"{hb.get('effort')}",
             f"- Baseline : {baseline_name or 'aucune (écarts de coût et de tokens sans seuil)'} ;"
             f" seuil d'une tâche : max({NOISE_FACTOR:g} × CV, {NOISE_FLOOR_PCT:g} %)",
             "", "## Alertes", ""]
    lines += [f"- {s}" for s in signals] or ["- aucune"]
    if mismatches:
        lines.append(f"- Attention : en-têtes différents ({', '.join(mismatches)}), comparaison "
                     "à lire avec prudence")
    lines += ["", "## Coût agrégé des tâches stables", ""]
    if agg is None:
        lines.append("- non calculé (pas de baseline avec agrégat, ou aucune tâche stable commune)")
    else:
        lines += [f"- Tâches : {len(agg['tasks'])} ({', '.join(agg['tasks'])})"
                  + (f" ; hors comparaison : {', '.join(agg['missing'])}" if agg["missing"] else ""),
                  f"- Coût : {fmt(agg['cost_a'])} → {fmt(agg['cost_b'])} "
                  f"({fmt(agg['change'], 1)} %), seuil {agg['limit']:.1f} %",
                  pessimistic_line(agg),
                  f"- CV du total en baseline : indépendance {fmt(agg['cv_independent'], 3)}, "
                  f"passes {fmt(agg['cv_passes'], 3)}, retenu {fmt(agg['cv'], 3)} (le plus grand)"]
        if agg["missing"]:
            lines.append("- Attention : le CV de la baseline porte sur toutes ses tâches stables, "
                         "le seuil est approximatif sur un sous-ensemble")
    lines += ["", "## Tâches exclues de la comparaison", "",
              "Version de la tâche différente entre B et A ou la baseline, ou inconnue : ni "
              "alerte ni écart.", ""]
    lines += [f"- {t} : {reason}" for t, reason in excluded.items()] or ["- aucune"]
    lines += ["", "## Totaux", "",
              "Runs complets, tâches exclues comprises.", "",
              f"- Résolues : {a['totals']['resolved']} / {a['totals']['task_runs']} → "
              f"{b['totals']['resolved']} / {b['totals']['task_runs']}",
              f"- Coût : {fmt(cost_a)} → {fmt(cost_b)} ({fmt(pct(cost_a, cost_b), 1)} %)",
              f"- Coût pessimiste : {fmt(a['totals'].get('cost_pessimistic'))} → "
              f"{fmt(b['totals'].get('cost_pessimistic'))} "
              f"({fmt(pct(a['totals'].get('cost_pessimistic'), b['totals'].get('cost_pessimistic')), 1)} %)",
              f"- Tokens : {tok_a} → {tok_b} ({fmt(pct(tok_a, tok_b), 1)} %)",
              f"- Tours médians : {fmt(a['totals']['turns_median'], 1)} → "
              f"{fmt(b['totals']['turns_median'], 1)}",
              f"- Miss serveur : {sum(server_misses(a, t) for t in pa)} → "
              f"{sum(server_misses(b, t) for t in pb)}", "", "## Par tâche", ""]
    multi = (ha.get("runs") or 1) > 1 or (hb.get("runs") or 1) > 1
    head = "| Tâche | Résolu A | Résolu B | Δ coût | Δ coût % (seuil) | " + " | ".join(
        f"Δ {c}" for c in CATEGORIES) + " | Δ tokens % (seuil) | Δ tours | Miss serveur A / B |"
    if multi:
        head += " Bascule A / B | σ coût A / B | σ tokens A / B |"
    lines += [head, "|" + "---|" * (head.count("|") - 1)]
    for task in tasks:
        ta, tb = pa.get(task), pb.get(task)
        if ta is None or tb is None:
            lines.append(f"| {task} | {resolved(ta)} | {resolved(tb)} | absente d'un des runs |")
            continue
        if task in excluded:
            lines.append(f"| {task} | {fmt(ta['resolved_rate'], 2)} | "
                         f"{fmt(tb['resolved_rate'], 2)} | exclue : {excluded[task]} |")
            continue
        ca, cb = ta["tokens_by_category_mean"], tb["tokens_by_category_mean"]
        limits = {m: noise_threshold(baseline, task, m) for m in ("cost", "tokens")}

        def pct_cell(va, vb, measure):
            limit = limits[measure]
            return f"{fmt(pct(va, vb), 1)} ({'-' if limit is None else f'{limit[0]:.1f}'})"

        row = (f"| {task} | {fmt(ta['resolved_rate'], 2)} | {fmt(tb['resolved_rate'], 2)} | "
               f"{delta(ta['cost_mean'], tb['cost_mean'])} | "
               f"{pct_cell(ta['cost_mean'], tb['cost_mean'], 'cost')} | "
               + " | ".join(delta(ca.get(c), cb.get(c)) for c in CATEGORIES)
               + f" | {pct_cell(task_tokens_mean(ta), task_tokens_mean(tb), 'tokens')}"
               f" | {delta(float(ta['turns_mean']), float(tb['turns_mean']))}"
               f" | {server_misses(a, task)} / {server_misses(b, task)} |")
        if multi:
            row += (f" {ta['flip']} / {tb['flip']} | {fmt(ta['cost_stdev'])} / "
                    f"{fmt(tb['cost_stdev'])} | {fmt(ta['tokens_stdev'], 1)} / "
                    f"{fmt(tb['tokens_stdev'], 1)} |")
        lines.append(row)
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("run_a", type=Path)
    parser.add_argument("run_b", type=Path)
    parser.add_argument("--baseline", type=Path,
                        help="défaut : bench/smoke/baseline/<modèle de B>.json s'il existe")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    a, b = load(args.run_a), load(args.run_b)
    path = args.baseline or HERE / "baseline" / f"{b['header'].get('model')}.json"
    baseline = json.loads(path.read_text()) if path.exists() else None
    if args.baseline and baseline is None:
        sys.exit(f"baseline introuvable : {path}")
    name = str(path.relative_to(HERE.parents[1])) if baseline and path.is_relative_to(
        HERE.parents[1]) else (str(path) if baseline else None)
    text = compare(a, b, baseline, name)
    if args.out:
        args.out.write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
