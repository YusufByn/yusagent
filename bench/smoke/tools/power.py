#!/usr/bin/env python3
"""Puissance estimée d'un A/B de miss du cache serveur, par simulation sur un bras mesuré (YA-09, ticket 14).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/power.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json --arm A

Base : les appels hors premier du bras `--arm` des unités retenues de `--plan`
(`ab_stats.units_from_plan`), lus au journal du reverse proxy avec les définitions de
`cache_causes.py` (miss = `cached_tokens = 0`), dont le prompt précédent fait moins de `--split`
tokens. Une unité = la liste des miss de ses appels.

1. Taux de base, écart type par bootstrap sur les unités (corrélation intra-session comprise),
   écart type binomial, effet de grappe (rapport des variances).
2. Simulation de l'A/B, pour chaque nombre de passes de `--passes` (5 unités par passe : les
   5 tâches du plan) et chaque réduction réelle `r` de `--reductions` : le bras A tire ses unités
   avec remise parmi celles de la base ; le bras B aussi, puis garde chaque miss avec la
   probabilité 1 − r. d = taux A − taux B ; IC 95 % par bootstrap sur les unités (`--boot`
   tirages, comme la règle mais allégé). Règle principale de `bench/smoke/ab/ya09-session-id-plan.md`
   (le garde-fou sur le miss global n'est pas simulé) :
   - gain net : d ≥ seuil et borne basse > 0 ;
   - non concluant : borne basse > 0 et d < seuil, ou d ≥ seuil et borne basse ≤ 0 ;
   - pas de gain net détecté : sinon.
   Sortie : part des simulations dans chaque verdict.

Graine fixe (`--seed`) : sortie reproductible. Bibliothèque standard, lecture seule.
"""
from __future__ import annotations

import argparse
import importlib.util
import random
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
TASKS_PER_PASS = 5


def load(name: str):
    spec = importlib.util.spec_from_file_location(f"smoke_{name}", TOOLS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def base_units(plan: Path, arm: str, split: int) -> list[list[bool]]:
    ab_stats, cache_causes = load("ab_stats"), load("cache_causes")
    units, _ = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    out = []
    for unit in sorted((u for u in units if not u["causes"]), key=lambda u: (u["pass"], u["task"])):
        side = unit["arms"][arm]
        task_run = ab_stats.task_dir(side["run"], side["result"])
        calls = cache_causes.session_calls(cache_causes.read_ndjson(task_run / "proxy.ndjson"))["calls"]
        out.append([c["miss"] for c in calls if not c["first"] and c["prev_prompt"] < split])
    return out


def rate(units: list[list[bool]]) -> float | None:
    n = sum(map(len, units))
    return sum(map(sum, units)) / n if n else None


def diff(a: list[list[bool]], b: list[list[bool]], keys: list[int]) -> float | None:
    ra, rb = rate([a[k] for k in keys]), rate([b[k] for k in keys])
    return None if ra is None or rb is None else ra - rb


def simulate(base, units, r, sims, boot, threshold, rng) -> dict[str, float]:
    counts = {"gain net": 0, "non concluant": 0, "pas de gain net détecté": 0}
    keys = list(range(units))
    for _ in range(sims):
        a = [rng.choice(base) for _ in keys]
        b = [[m and rng.random() >= r for m in rng.choice(base)] for _ in keys]
        d = diff(a, b, keys)
        draws = sorted(x for x in (diff(a, b, [rng.choice(keys) for _ in keys]) for _ in range(boot))
                       if x is not None)
        low = draws[int(0.025 * len(draws))] if draws else None
        if d is not None and low is not None and d >= threshold and low > 0:
            counts["gain net"] += 1
        elif d is not None and low is not None and (low > 0 or d >= threshold):
            counts["non concluant"] += 1
        else:
            counts["pas de gain net détecté"] += 1
    return {k: v / sims for k, v in counts.items()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--arm", default="A")
    parser.add_argument("--split", type=int, default=6000)
    parser.add_argument("--threshold", type=float, default=0.10)
    parser.add_argument("--passes", default="4,6,8")
    parser.add_argument("--reductions", default="0,0.5,0.75,0.9")
    parser.add_argument("--sims", type=int, default=600)
    parser.add_argument("--boot", type=int, default=400)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    base = base_units(args.plan, args.arm, args.split)
    n, x = sum(map(len, base)), sum(map(sum, base))
    p = x / n
    rng = random.Random(args.seed)
    boots = [v for v in (rate([rng.choice(base) for _ in base]) for _ in range(20_000)) if v is not None]
    mean = sum(boots) / len(boots)
    se = (sum((v - mean) ** 2 for v in boots) / len(boots)) ** 0.5
    se_bin = (p * (1 - p) / n) ** 0.5
    print(f"# Puissance : {args.plan}, bras {args.arm}, prompt précédent < {args.split}\n")
    print(f"- Base : {x}/{n} ({100 * p:.1f} %) sur {len(base)} unités, "
          f"{sum(1 for u in base if u)} avec au moins un appel")
    print(f"- Écart type par bootstrap sur les unités {100 * se:.1f} points, binomial "
          f"{100 * se_bin:.1f} points, effet de grappe {(se / se_bin) ** 2:.2f}")
    print(f"- Simulation : {args.sims} A/B par cas, bootstrap de {args.boot} tirages, seuil "
          f"{100 * args.threshold:.0f} points, graine {args.seed}\n")
    print("| Passes | Réduction réelle (d réel) | gain net | non concluant | pas de gain net détecté |")
    print("|---|---|---|---|---|")
    for passes in (int(v) for v in args.passes.split(",")):
        for r in (float(v) for v in args.reductions.split(",")):
            share = simulate(base, passes * TASKS_PER_PASS, r, args.sims, args.boot,
                             args.threshold, rng)
            print(f"| {passes} | {100 * r:.0f} % ({100 * p * r:.1f} points) | "
                  f"{100 * share['gain net']:.0f} % | {100 * share['non concluant']:.0f} % | "
                  f"{100 * share['pas de gain net détecté']:.0f} % |")


if __name__ == "__main__":
    main()
