#!/usr/bin/env python3
"""Mesures de l'A/B YA-09 du cache v2 (`ya09-cache-v2-plan.md`) hors `cache_calls.py` et `ab_stats.py`.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya09-cache-v2-20261116-mesures.py bench/smoke/runs/ab-<horodatage>

A = `settings/default.json` (prompt v2), B = `settings/prompt-v1.json` (prompt v1).

Analyses **a posteriori** (non prévues par la règle du plan, affichées sans décider) :
1. Écart du taux de miss A − B par sous-population d'appels, avec la méthode de la règle : appels
   hors premier des unités retenues (`ab_stats.units_from_plan`), miss = `cacheRead = 0`
   (`cache_calls.session_calls`), écart des taux poolés, IC 95 % par bootstrap sur les unités
   (10 000 tirages, graine 0). Sous-populations : toutes (doit redonner la règle), prompt
   précédent ≥ 6 000 tokens, prompt précédent ≥ 6 000 tokens et âge < 30 s (début de l'appel −
   début du premier appel de la session, `startMs`), prompt précédent < 6 000 tokens.
2. Coût rapporté au-dessus du coût à cache théorique, par bras : Σ coût rapporté / Σ coût
   théorique − 1 sur les unités retenues (champs `total` et `theoretical_total` du résultat de
   la tâche), et part du coût rapporté due au cache imparfait : 1 − théorique / rapporté.
3. Tokens de cache perdus par classe d'appel (miss, relecture partielle, autre).
4. Miss croisés avec le silence depuis l'appel précédent (`cache_calls`), le nombre d'appels de
   la session dans les 60 s précédentes (`startMs`) et le rang × la taille du prompt précédent.
5. Perte des miss par taille du prompt précédent et part estimée du coût rapporté.

Bibliothèque standard, lecture seule."""
from __future__ import annotations

import importlib.util
import random
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import cache_calls  # noqa: E402

SPLIT = 6000
EARLY_S = 30
DRAWS, SEED = 10_000, 0


def unit_calls(units, ab_stats) -> dict:
    """{(passe, tâche): {bras: [appels hors premier, avec l'âge]}}."""
    out = {}
    for unit in units:
        per_arm = {}
        for side in ("A", "B"):
            arm = unit["arms"][side]
            calls = []
            for session in cache_calls.session_calls(ab_stats.task_dir(arm["run"], arm["result"])):
                prev = None
                for c in session:
                    if prev is not None:
                        c["prev_prompt"] = prev["prompt"]
                        calls.append(c)
                    prev = c
            per_arm[side] = calls
        out[(unit["pass"], unit["task"])] = per_arm
    return out


def diff(data: dict, keys: list) -> tuple:
    a = [sum(c["miss"] for c in data[k]["A"]) for k in keys], [len(data[k]["A"]) for k in keys]
    b = [sum(c["miss"] for c in data[k]["B"]) for k in keys], [len(data[k]["B"]) for k in keys]
    am, an, bm, bn = sum(a[0]), sum(a[1]), sum(b[0]), sum(b[1])
    d = am / an - bm / bn if an and bn else None
    return (am, an), (bm, bn), d


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print(__doc__.splitlines()[3].strip(), file=sys.stderr)
        return 2
    plan = Path(argv[0]) / "ab.json"
    ab_stats = cache_calls.load_ab_stats()
    units, _ = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    kept = [u for u in units if not u["causes"]]
    print(f"Unités : {len(units)}, retenues {len(kept)}\n")

    calls = unit_calls(kept, ab_stats)
    age = {}
    for unit in kept:
        for side in ("A", "B"):
            arm = unit["arms"][side]
            task_run = ab_stats.task_dir(arm["run"], arm["result"])
            paths = sorted((task_run / "sessions").rglob("*.usage.ndjson"))
            for path in paths:
                rows = sorted(cache_calls.read_ndjson(path), key=lambda r: r["seq"])
                for r in rows:
                    age[(unit["pass"], unit["task"], side, r["seq"])] = (r["startMs"] - rows[0]["startMs"]) / 1000
            if len(paths) != 1:
                print(f"attention : {len(paths)} sessions dans {task_run} (âge par seq ambigu)")
    for (p, t), per_arm in calls.items():
        for side, cs in per_arm.items():
            for c in cs:
                c["age"] = age[(p, t, side, c["seq"])]

    pops = [("toutes (règle du plan)", lambda c: True),
            (f"prompt précédent >= {SPLIT}", lambda c: c["prev_prompt"] >= SPLIT),
            (f"prompt précédent >= {SPLIT} et âge < {EARLY_S} s",
             lambda c: c["prev_prompt"] >= SPLIT and c["age"] < EARLY_S),
            (f"prompt précédent < {SPLIT}", lambda c: c["prev_prompt"] < SPLIT)]
    keys = sorted(calls)
    print("## 1. Écart du taux de miss A − B par sous-population (a posteriori)\n")
    print("| Sous-population | Miss A | Miss B | d (points) | IC 95 % |\n|---|---|---|---|---|")
    for name, keep in pops:
        data = {k: {s: [c for c in calls[k][s] if keep(c)] for s in ("A", "B")} for k in keys}
        (am, an), (bm, bn), d = diff(data, keys)
        rng = random.Random(SEED)
        draws = []
        for _ in range(DRAWS):
            sample = [rng.choice(keys) for _ in keys]
            x = diff(data, sample)[2]
            if x is not None:
                draws.append(x)
        draws.sort()
        ci = (f"[{100 * draws[int(0.025 * len(draws))]:+.1f} ; {100 * draws[int(0.975 * len(draws)) - 1]:+.1f}]"
              if draws else "n/a")
        ra = f"{am}/{an} ({100 * am / an:.1f} %)" if an else "0/0"
        rb = f"{bm}/{bn} ({100 * bm / bn:.1f} %)" if bn else "0/0"
        print(f"| {name} | {ra} | {rb} | {'n/a' if d is None else f'{100 * d:+.1f}'} | {ci} |")

    print("\n## 2. Coût rapporté au-dessus du coût à cache théorique (unités retenues)\n")
    rep_th = {}
    print("| Bras | Σ rapporté $ | Σ théorique $ | rapporté / théorique − 1 | part due au cache imparfait |")
    print("|---|---|---|---|---|")
    for side in ("A", "B"):
        rep = sum(ab_stats.cost(u["arms"][side]["result"], "rapporté") for u in kept)
        th = sum(ab_stats.cost(u["arms"][side]["result"], "théorique") for u in kept)
        rep_th[side] = (rep, th)
        print(f"| {side} | {rep:.4f} | {th:.4f} | {100 * (rep / th - 1):+.1f} % | {100 * (1 - th / rep):.1f} % |")

    print("\n## 3. Tokens de cache perdus par classe d'appel (unités retenues)\n")
    print("Perte d'un appel = cacheRead théorique (`metrics.theoretical_cache`) − cacheRead rapporté. "
          "Classes de `cache_calls.py` ; « autre » = ni miss ni relecture partielle.\n")
    print("| Bras | Perte totale (tokens) | miss | relectures partielles | autre |\n|---|---|---|---|---|")
    spec = importlib.util.spec_from_file_location("metrics", TOOLS.parent / "metrics.py")
    metrics = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(metrics)
    losses = {}
    for side in ("A", "B"):
        loss = {"miss": 0, "partial": 0, "autre": 0}
        for unit in kept:
            arm = unit["arms"][side]
            task_run = ab_stats.task_dir(arm["run"], arm["result"])
            cls = {c["seq"]: "miss" if c["miss"] else "partial" if c["partial"] else "autre"
                   for c in calls[(unit["pass"], unit["task"])][side]}
            for path in sorted((task_run / "sessions").rglob("*.usage.ndjson")):
                for c in metrics.theoretical_cache(cache_calls.read_ndjson(path)):
                    if c["seq"] in cls:
                        loss[cls[c["seq"]]] += c["theoretical"] - c["reported"]
        total = sum(loss.values())
        print(f"| {side} | {total} | " + " | ".join(
            f"{loss[k]} ({100 * loss[k] / total:.0f} %)" for k in ("miss", "partial", "autre")) + " |")
        losses[side] = (total, rep_th[side])

    print("\n## 4. Croisements des miss (miss / appels hors premier, unités retenues)\n")
    for (p, t), per_arm in calls.items():
        for side, cs in per_arm.items():
            for i, c in enumerate(cs):
                c["rank"] = i + 2
                earlier = [age[(p, t, side, s)] for s in range(1, c["seq"]) if (p, t, side, s) in age]
                c["recent"] = sum(1 for a in earlier if c["age"] - a <= 60)
    tables = [
        ("silence depuis la fin de l'appel précédent (s)", "silence",
         [(None, 1, "< 1"), (1, 5, "1-5"), (5, 15, "5-15"), (15, None, ">= 15")]),
        ("appels de la session dans les 60 s précédentes", "recent",
         [(None, 6, "1-5"), (6, 11, "6-10"), (11, 16, "11-15"), (16, None, ">= 16")]),
    ]
    for title, key, bands in tables:
        print(f"| {title} | A | B |\n|---|---|---|")
        for lo, hi, label in bands:
            cells = []
            for side in ("A", "B"):
                g = [c for k in keys for c in calls[k][side]
                     if (lo is None or c[key] >= lo) and (hi is None or c[key] < hi)]
                cells.append(f"{sum(c['miss'] for c in g)}/{len(g)}")
            print(f"| {label} | {cells[0]} | {cells[1]} |")
        print()
    print(f"| rang × prompt précédent | A | B |\n|---|---|---|")
    for rlabel, rsel in (("rang 2-4", lambda c: c["rank"] <= 4), ("rang >= 5", lambda c: c["rank"] >= 5)):
        for slabel, ssel in ((f"< {SPLIT}", lambda c: c["prev_prompt"] < SPLIT),
                             (f">= {SPLIT}", lambda c: c["prev_prompt"] >= SPLIT)):
            cells = []
            for side in ("A", "B"):
                g = [c for k in keys for c in calls[k][side] if rsel(c) and ssel(c)]
                cells.append(f"{sum(c['miss'] for c in g)}/{len(g)}")
            print(f"| {rlabel}, {slabel} | {cells[0]} | {cells[1]} |")

    print("\n## 5. Perte des miss par taille du prompt précédent (estimation)\n")
    print("Perte d'un miss estimée à ⌊prompt précédent⌋₁₂₈ ; part du coût rapporté = part de la "
          "perte × (rapporté − théorique) / rapporté (le coût du cache imparfait est linéaire en "
          "tokens perdus, hors tarif contexte long).\n")
    print("| Bras | miss < 6000 (tokens) | miss >= 6000 | part estimée du coût rapporté, miss < 6000 | tous les miss |")
    print("|---|---|---|---|---|")
    for side in ("A", "B"):
        small = sum(c["prev_prompt"] // 128 * 128 for k in keys for c in calls[k][side]
                    if c["miss"] and c["prev_prompt"] < SPLIT)
        large = sum(c["prev_prompt"] // 128 * 128 for k in keys for c in calls[k][side]
                    if c["miss"] and c["prev_prompt"] >= SPLIT)
        total, (rep, th) = losses[side]
        gap = (rep - th) / rep
        print(f"| {side} | {small} | {large} | {100 * small / total * gap:.1f} % | "
              f"{100 * (small + large) / total * gap:.1f} % |")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
