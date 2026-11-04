#!/usr/bin/env python3
"""Comparaison appariée de deux runs du smoke bench (T06, YA-14).

Usage :
    python3 -I bench/smoke/compare.py <run A> <run B> [--out <fichier>.md] [--threshold 10]

Par tâche : résolu A/B, Δ coût, Δ tokens par catégorie, Δ tours. Signale en tête toute
tâche résolue en A et non en B, et une hausse de tokens ou de coût au-delà du seuil
sans tâche résolue en plus (CDC, YA-14). Seuil par défaut provisoire (10 %), en
attendant la mesure du bruit (ticket 07). Avec plusieurs runs par configuration, le
taux de bascule et l'écart-type entre runs sont publiés à côté de chaque écart ; une
tâche compte comme résolue si elle l'est dans au moins la moitié de ses runs.

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

CATEGORIES = ("input", "output", "cacheRead", "cacheWrite", "reasoning")
SAME_HEADER = ("provider", "model", "effort", "prices_version", "models_bundled_sha256", "tasks",
               "mode")


def load(run: Path) -> dict:
    path = run / "report.json" if run.is_dir() else run
    return json.loads(path.read_text())


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


def compare(a: dict, b: dict, threshold: float) -> str:
    ha, hb = a["header"], b["header"]
    pa, pb = a["per_task"], b["per_task"]
    tasks = sorted(set(pa) | set(pb))
    regressions = [t for t in tasks if resolved(pa.get(t)) and resolved(pb.get(t)) is False]
    gained = sum(1 for t in tasks if resolved(pb.get(t)) and not resolved(pa.get(t)))
    cost_a, cost_b = a["totals"]["cost"], b["totals"]["cost"]
    tok_a, tok_b = total_tokens(a), total_tokens(b)
    signals = [f"**Régression** : {t} résolue en A, non résolue en B" for t in regressions]
    for name, va, vb in (("coût", cost_a, cost_b), ("tokens", tok_a, tok_b)):
        change = pct(va, vb)
        if change is not None and change > threshold and gained == 0:
            signals.append(f"**Hausse de {name}** : {change:+.1f} % (seuil {threshold:g} %) "
                           "sans tâche résolue en plus")
    mismatches = [k for k in SAME_HEADER if ha.get(k) != hb.get(k)]
    lines = ["# Comparaison smoke bench", "",
             f"- A : `{ha.get('commit')}` (binaire `{ha.get('binary_sha256', '')[:12]}`), "
             f"réglages {json.dumps(ha.get('settings_non_default'))}, {ha.get('runs')} run(s)",
             f"- B : `{hb.get('commit')}` (binaire `{hb.get('binary_sha256', '')[:12]}`), "
             f"réglages {json.dumps(hb.get('settings_non_default'))}, {hb.get('runs')} run(s)",
             f"- Modèle {ha.get('model')} / {hb.get('model')}, effort {ha.get('effort')} / "
             f"{hb.get('effort')}", "", "## Signaux", ""]
    lines += [f"- {s}" for s in signals] or ["- aucun"]
    if mismatches:
        lines.append(f"- Attention : en-têtes différents ({', '.join(mismatches)}), comparaison "
                     "à lire avec prudence")
    lines += ["", "## Totaux", "",
              f"- Résolues : {a['totals']['resolved']} / {a['totals']['task_runs']} → "
              f"{b['totals']['resolved']} / {b['totals']['task_runs']}",
              f"- Coût : {fmt(cost_a)} → {fmt(cost_b)} ({fmt(pct(cost_a, cost_b), 1)} %)",
              f"- Tokens : {tok_a} → {tok_b} ({fmt(pct(tok_a, tok_b), 1)} %)",
              f"- Tours médians : {fmt(a['totals']['turns_median'], 1)} → "
              f"{fmt(b['totals']['turns_median'], 1)}", "", "## Par tâche", ""]
    multi = (ha.get("runs") or 1) > 1 or (hb.get("runs") or 1) > 1
    head = "| Tâche | Résolu A | Résolu B | Δ coût | " + " | ".join(
        f"Δ {c}" for c in CATEGORIES) + " | Δ tours |"
    if multi:
        head += " Bascule A / B | σ coût A / B | σ tokens A / B |"
    lines += [head, "|" + "---|" * (head.count("|") - 1)]
    for task in tasks:
        ta, tb = pa.get(task), pb.get(task)
        if ta is None or tb is None:
            lines.append(f"| {task} | {resolved(ta)} | {resolved(tb)} | absente d'un des runs |")
            continue
        ca, cb = ta["tokens_by_category_mean"], tb["tokens_by_category_mean"]
        row = (f"| {task} | {fmt(ta['resolved_rate'], 2)} | {fmt(tb['resolved_rate'], 2)} | "
               f"{delta(ta['cost_mean'], tb['cost_mean'])} | "
               + " | ".join(delta(ca.get(c), cb.get(c)) for c in CATEGORIES)
               + f" | {delta(float(ta['turns_mean']), float(tb['turns_mean']))} |")
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
    parser.add_argument("--out", type=Path)
    parser.add_argument("--threshold", type=float, default=10.0,
                        help="hausse de coût ou de tokens signalée, en %% (défaut provisoire 10)")
    args = parser.parse_args()
    text = compare(load(args.run_a), load(args.run_b), args.threshold)
    if args.out:
        args.out.write_text(text)
    print(text, end="")


if __name__ == "__main__":
    main()
