#!/usr/bin/env python3
"""Bruit du taux de relectures partielles d'un bras, par rééchantillonnage des sessions (YA-09, ticket 14).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/drift_se.py --plan bench/smoke/runs/ab-20261114T201248Z/ab.json --arm A \\
        --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383

Sessions : runs de tâche du bras `--arm` de `--plan` (`cache_causes.task_runs_from_plan`, toutes
les unités, comme `cache_causes.py --plan`), restreints à `--tasks`. Relecture partielle :
définition de `cache_causes.py`, `0 < cached_tokens < ⌊prompt précédent⌋₁₂₈ − 256`, hors premier
appel. Taux = Σ relectures partielles / Σ appels hors premier.

Sortie : taux observé ; distribution du taux d'un run de `--sessions` sessions tirées avec remise
parmi les sessions observées (`--draws` tirages, graine `--seed`) : écart type, intervalle à 95 %,
et pour chaque seuil de `--thresholds`, la part des tirages sous le seuil. Sert à fixer les
bornes de la règle du contrôle de la dérive (`bench/smoke/ab/ya09-session-id-plan.md`).

Bibliothèque standard, lecture seule.
"""
from __future__ import annotations

import argparse
import importlib.util
import random
from pathlib import Path

TOOLS = Path(__file__).resolve().parent


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--arm", required=True)
    parser.add_argument("--tasks", default="")
    parser.add_argument("--sessions", type=int, default=20)
    parser.add_argument("--draws", type=int, default=20_000)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--thresholds", default="0.054,0.084,0.114")
    args = parser.parse_args()

    spec = importlib.util.spec_from_file_location("smoke_cache_causes", TOOLS / "cache_causes.py")
    cc = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cc)
    tasks = {t for t in args.tasks.split(",") if t} or None
    runs = [path for arm, path in cc.task_runs_from_plan(args.plan, tasks) if arm == args.arm]
    sessions = [[c["partial"] for c in cc.session_calls(cc.read_ndjson(r / "proxy.ndjson"))["calls"]
                 if not c["first"]] for r in runs]
    n, x = sum(map(len, sessions)), sum(map(sum, sessions))
    rng = random.Random(args.seed)
    draws = []
    for _ in range(args.draws):
        sample = [rng.choice(sessions) for _ in range(args.sessions)]
        draws.append(sum(map(sum, sample)) / sum(map(len, sample)))
    draws.sort()
    mean = sum(draws) / len(draws)
    se = (sum((d - mean) ** 2 for d in draws) / len(draws)) ** 0.5
    print(f"# Relectures partielles : {args.plan}, bras {args.arm}\n")
    print(f"- Observé : {len(sessions)} sessions, {x}/{n} ({100 * x / n:.1f} %)")
    print(f"- Run de {args.sessions} sessions ({args.draws} tirages, graine {args.seed}) : "
          f"écart type {100 * se:.1f} points, intervalle à 95 % "
          f"[{100 * draws[int(0.025 * len(draws))]:.1f} ; "
          f"{100 * draws[int(0.975 * len(draws)) - 1]:.1f}] %")
    for threshold in (float(t) for t in args.thresholds.split(",")):
        below = sum(d < threshold for d in draws) / len(draws)
        print(f"- Part des tirages sous {100 * threshold:.1f} % : {100 * below:.1f} %")


if __name__ == "__main__":
    main()
