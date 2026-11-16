#!/usr/bin/env python3
"""cacheRead par appel et miss de cache, sur des runs du banc ou un A/B entrelacé (YA-09).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/cache_calls.py <run> [<run> ...]
    python3 -I bench/smoke/tools/cache_calls.py --plan <ab.json> [--draws 10000] [--seed 0]

Lit les journaux d'usage des sessions de tâche (`<run>/tasks/<tâche>/run-*/sessions/**/
*.usage.ndjson`, sous-agents compris, chaque session à part). Définitions, par appel d'une
session, l'appel précédent étant celui de `seq` inférieur dans la même session :
- prompt : `input + cacheRead + cacheWrite` ;
- silence : début de l'appel − fin de l'appel précédent (`startMs`, `durationMs`) ;
- **miss** : `cacheRead = 0` hors premier appel de la session ;
- **relecture partielle** : `0 < cacheRead < ⌊prompt précédent⌋₁₂₈ − 256`, hors premier
  appel. Le backend relit d'ordinaire le prompt précédent arrondi au bloc de 128 tokens, parfois
  un bloc de moins ; la marge de 256 tokens (deux blocs) écarte ce décalage ;
- taux de cache : `Σ cacheRead / Σ prompt`.

Sans `--plan` : liste par appel (seq, kind, silence, `sincePrevMs`, prompt, cacheRead, marque
MISS ou PARTIEL), puis par tâche et par tranche de silence.

Avec `--plan` : unités (passe, tâche) d'un A/B entrelacé, écartées par la règle de
`ab_stats.py` (erreur provider sur un bras : l'unité entière). Par bras, sur les unités
retenues : appels, appels hors premier, miss, relectures partielles, taux de cache de la
session (global et médiane par unité), coût rapporté, tâches résolues, compactions. Écart du
taux de miss A − B, IC 95 % par bootstrap sur les unités (`--draws` tirages, graine `--seed`,
sortie reproductible), et verdict de la règle de `bench/smoke/ab/ya09-cache-v2-plan.md`.

Lecture seule, bibliothèque standard.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BLOCK = 128
PARTIAL_MARGIN = 256
BUCKETS = [(0, 10), (10, 60), (60, 120), (120, None)]
# Règle fixée avant le run (ya09-cache-v2-plan.md) : écart du taux de miss A − B.
THRESHOLD = 0.03


def read_ndjson(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def session_calls(task_run: Path) -> list[list[dict]]:
    """Appels de chaque session d'un run de tâche, classés par `seq`, avec leurs mesures."""
    sessions = []
    for path in sorted((task_run / "sessions").rglob("*.usage.ndjson")):
        rows = sorted(read_ndjson(path), key=lambda r: r["seq"])
        calls, prev = [], None
        for r in rows:
            prompt = r["input"] + r["cacheRead"] + r["cacheWrite"]
            call = {"seq": r["seq"], "kind": r["kind"], "prompt": prompt, "read": r["cacheRead"],
                    "since": r.get("sincePrevMs"), "first": prev is None, "silence": None,
                    "miss": False, "partial": False}
            if prev is not None:
                call["silence"] = (r["startMs"] - prev["startMs"] - (prev["durationMs"] or 0)) / 1000
                call["miss"] = r["cacheRead"] == 0
                floor = (calls[-1]["prompt"] // BLOCK) * BLOCK
                call["partial"] = 0 < r["cacheRead"] < floor - PARTIAL_MARGIN
            calls.append(call)
            prev = r
        if calls:
            sessions.append(calls)
    return sessions


def totals(sessions: list[list[dict]]) -> dict:
    calls = [c for s in sessions for c in s]
    later = [c for c in calls if not c["first"]]
    prompt = sum(c["prompt"] for c in calls)
    return {"calls": len(calls), "later": len(later),
            "misses": sum(c["miss"] for c in later),
            "partials": sum(c["partial"] for c in later),
            "read": sum(c["read"] for c in calls), "prompt": prompt,
            "rate": sum(c["read"] for c in calls) / prompt if prompt else None}


def bucket(seconds: float) -> str:
    for low, high in BUCKETS:
        if seconds >= low and (high is None or seconds < high):
            return f"{low}-{high} s" if high is not None else f"≥ {low} s"
    return "?"


def ratio(n: int, d: int) -> str:
    return f"{n}/{d} ({n / d:.1%})" if d else f"{n}/0"


# --- Runs ------------------------------------------------------------------------------


def show_runs(runs: list[Path]) -> None:
    by_bucket: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for run in runs:
        print(f"== {run}")
        for task_run in sorted(run.glob("tasks/*/run-*")):
            sessions = session_calls(task_run)
            if not sessions:
                continue
            t = totals(sessions)
            print(f"  {task_run.parent.name}/{task_run.name} : {t['calls']} appels, taux de cache "
                  f"{t['rate']:.3f}, miss {ratio(t['misses'], t['later'])}, relectures partielles "
                  f"{ratio(t['partials'], t['later'])}")
            for calls in sessions:
                for c in calls:
                    silence = "-" if c["silence"] is None else f"{c['silence']:.1f}"
                    mark = " MISS" if c["miss"] else " PARTIEL" if c["partial"] else ""
                    print(f"    seq {c['seq']:>3} {c['kind']:<8} silence {silence:>7} s  "
                          f"sincePrevMs {str(c['since']):>7}  prompt {c['prompt']:>6}  "
                          f"cacheRead {c['read']:>6}{mark}")
                    if not c["first"]:
                        b = by_bucket[bucket(c["silence"])]
                        b[0] += 1
                        b[1] += c["miss"]
    print("== miss par tranche de silence (tous les runs donnés)")
    for low, high in BUCKETS:
        key = f"{low}-{high} s" if high is not None else f"≥ {low} s"
        n, m = by_bucket.get(key, [0, 0])
        print(f"  {key:>10} : {ratio(m, n)}")


# --- A/B -------------------------------------------------------------------------------


def load_ab_stats():
    path = REPO / "bench" / "smoke" / "tools" / "ab_stats.py"
    spec = importlib.util.spec_from_file_location("smoke_ab_stats", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def unit_measures(unit: dict, side: str, ab_stats) -> dict:
    arm = unit["arms"][side]
    result = arm["result"]
    sessions = session_calls(ab_stats.task_dir(arm["run"], result))
    t = totals(sessions)
    t["cost"] = ab_stats.cost(result, "rapporté")
    t["resolved"] = result["status"] == "resolved"
    t["compactions"] = result.get("compactions") or 0
    return t


def pooled_diff(pairs: list[tuple[dict, dict]]) -> float | None:
    la = sum(a["later"] for a, _ in pairs)
    lb = sum(b["later"] for _, b in pairs)
    if not la or not lb:
        return None
    return sum(a["misses"] for a, _ in pairs) / la - sum(b["misses"] for _, b in pairs) / lb


def show_plan(plan: Path, draws: int, seed: int) -> None:
    ab_stats = load_ab_stats()
    units, ab = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    kept = [u for u in units if not u["causes"]]
    print(f"# Cache par bras : {plan}\n")
    print(f"- Bras A : `{ab['settings']['A']['file']}` ; bras B : `{ab['settings']['B']['file']}`")
    print(f"- Binaire : `{ab['binary_sha']}` ; modèle `{ab['model']}`, effort `{ab['effort']}` ; "
          f"graine du plan {ab['seed']}")
    print(f"- Unités : {len(units)}, retenues {len(kept)}, écartées {len(units) - len(kept)}")
    for u in units:
        if u["causes"]:
            print(f"  - écartée : passe {u['pass']}, {u['task']} : {'; '.join(u['causes'])}")
    pairs = [(unit_measures(u, "A", ab_stats), unit_measures(u, "B", ab_stats)) for u in kept]
    print("\n| Bras | Appels | Hors premier | Miss | Relectures partielles | Taux de cache (global) "
          "| Taux de cache (médiane par unité) | Coût rapporté $ | Résolues | Compactions |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for index, side in enumerate("AB"):
        rows = [p[index] for p in pairs]
        later = sum(r["later"] for r in rows)
        read = sum(r["read"] for r in rows)
        prompt = sum(r["prompt"] for r in rows)
        rates = [r["rate"] for r in rows if r["rate"] is not None]
        print(f"| {side} | {sum(r['calls'] for r in rows)} | {later} "
              f"| {ratio(sum(r['misses'] for r in rows), later)} "
              f"| {ratio(sum(r['partials'] for r in rows), later)} "
              f"| {read / prompt:.3f} | {statistics.median(rates):.3f} "
              f"| {sum(r['cost'] for r in rows):.4f} "
              f"| {sum(r['resolved'] for r in rows)}/{len(rows)} "
              f"| {sum(r['compactions'] for r in rows)} |" if prompt and rates else f"| {side} | - |")
    print("\n| Passe | Tâche | Miss A | Miss B | Partiels A | Partiels B | Cache A | Cache B |")
    print("|---|---|---|---|---|---|---|---|")
    for u, (a, b) in zip(kept, pairs):
        print(f"| {u['pass']} | {u['task']} | {ratio(a['misses'], a['later'])} "
              f"| {ratio(b['misses'], b['later'])} | {a['partials']} | {b['partials']} "
              f"| {a['rate']:.3f} | {b['rate']:.3f} |")
    diff = pooled_diff(pairs)
    if diff is None:
        print("\nÉcart non calculable (aucun appel hors premier).")
        return
    low, high = ab_stats.bootstrap(pairs, lambda sample: pooled_diff(sample), draws, seed)
    print(f"\nÉcart du taux de miss A − B : {diff:+.1%}, IC 95 % [{low:+.1%}, {high:+.1%}] "
          f"(bootstrap sur {len(pairs)} unités, {draws} tirages, graine {seed})")
    worse = diff >= THRESHOLD and low > 0
    if worse:
        verdict = "A (prompt v2) a plus de miss que B (prompt v1) : écart net, ticket prioritaire"
    elif diff >= THRESHOLD:
        verdict = "écart au-dessus du seuil mais IC qui contient 0 : non concluant"
    else:
        verdict = f"écart sous le seuil de {THRESHOLD:.0%} : pas d'écart net"
    print(f"Verdict (règle de ya09-cache-v2-plan.md, seuil {THRESHOLD:.0%} et borne basse > 0) : "
          f"{verdict}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", nargs="*", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--draws", type=int, default=10_000)
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args()
    if args.plan:
        show_plan(args.plan, args.draws, args.seed)
    elif args.runs:
        show_runs(args.runs)
    else:
        parser.error("un run au moins, ou --plan")
    return 0


if __name__ == "__main__":
    sys.exit(main())
