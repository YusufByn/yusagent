#!/usr/bin/env python3
"""Baseline du smoke bench (T07, YA-14) : bruit mesuré sur plusieurs runs de `main`.

Usage :
    python3 -I bench/smoke/baseline.py <run> [<run> ...] [--out bench/smoke/baseline/<modèle>.json]

Lit les `report.json` des runs donnés (un run à `RUNS=3`, ou plusieurs runs à `RUNS=1`),
tous du même modèle, effort, réglages, catalogue et tarif, et écrit par tâche : taux de
résolution, stabilité (résolue à tous les runs), moyenne, écart-type et coefficient de
variation du coût et des tokens, tours moyens, miss serveur et taux de cache évitable.
Miss serveur et taux évitable sont relus dans les journaux de chaque run de tâche
(`metrics.task_run_cache`) : un run fait avant leur ajout au rapport est donc couvert.

`compare.py` lit ce fichier pour ses seuils de bruit. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import metrics  # noqa: E402

SAME = ("mode", "provider", "model", "effort", "settings_non_default", "models_bundled_sha256",
        "prices_version")
MIN_RUNS = 3


def metrics_fmt(value: float | None) -> str:
    return "null" if value is None else f"{value:.3f}"


def total_tokens(result: dict) -> int:
    return sum(result["tokens"][k] or 0 for k in ("input", "output", "cacheRead", "cacheWrite"))


def spread(values: list[float]) -> dict:
    mean = statistics.mean(values) if values else None
    stdev = statistics.stdev(values) if len(values) > 1 else None
    cv = stdev / mean if stdev is not None and mean else None
    return {"mean": mean, "stdev": stdev, "cv": cv, "values": values}


def build(runs: list[Path]) -> dict:
    reports = [(run, json.loads((run / "report.json").read_text())) for run in runs]
    first = reports[0][1]["header"]
    for run, report in reports[1:]:
        diff = [k for k in SAME if report["header"].get(k) != first.get(k)]
        if diff:
            sys.exit(f"{run.name} : en-tête différent du premier run ({', '.join(diff)})")
    if first.get("mode") != "réel":
        print("attention : runs en mode capture, baseline de test seulement", file=sys.stderr)
    tasks: dict[str, list[tuple[Path, dict]]] = {}
    for run, report in reports:
        for result in report["results"]:
            tasks.setdefault(result["task"], []).append((run, result))
    per_task = {}
    for task, items in sorted(tasks.items()):
        results = [r for _, r in items]
        caches = [metrics.task_run_cache(run / "tasks" / task / f"run-{r['run']}")
                  for run, r in items]
        resolved = [r["resolved"] for r in results]
        costs = [r["cost"]["total"] for r in results if r["cost"]["total"] is not None]
        avoidable = [c["cache_avoidable_rate"] for c in caches
                     if c["cache_avoidable_rate"] is not None]
        per_task[task] = {
            "runs": len(results),
            "resolved_rate": sum(resolved) / len(resolved),
            "stable": len(resolved) >= MIN_RUNS and all(resolved),
            "cost": spread(costs) if len(costs) == len(results) else None,
            "tokens": spread([total_tokens(r) for r in results]),
            "turns_mean": statistics.mean(r["calls"]["turns"] for r in results),
            "context_peak_max": max((r["context_peak"] for r in results
                                     if r["context_peak"] is not None), default=None),
            "server_misses": sum(len(c["server_misses"]) for c in caches),
            "cache_avoidable_rate_mean": statistics.mean(avoidable) if avoidable else None,
        }
    short = [t for t, a in per_task.items() if a["runs"] < MIN_RUNS]
    if short:
        print(f"attention : moins de {MIN_RUNS} runs pour {', '.join(short)}", file=sys.stderr)
    return {
        "written": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "command": "python3 -I bench/smoke/baseline.py " + " ".join(str(r) for r in runs),
        "runs": [{"run": run.name, "commit": r["header"].get("commit"),
                  "binary_sha256": r["header"].get("binary_sha256"),
                  "runs_per_task": r["header"].get("runs")} for run, r in reports],
        "header": {k: first.get(k) for k in SAME},
        "per_task": per_task,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", nargs="+", type=Path)
    parser.add_argument("--out", type=Path, help="défaut : bench/smoke/baseline/<modèle>.json")
    args = parser.parse_args()
    baseline = build(args.runs)
    out = args.out or HERE / "baseline" / f"{baseline['header']['model']}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(baseline, indent=2, ensure_ascii=False) + "\n")
    for task, a in baseline["per_task"].items():
        cv_cost = metrics_fmt((a["cost"] or {}).get("cv"))
        cv_tokens = metrics_fmt(a["tokens"]["cv"])
        stable = " (stable)" if a["stable"] else ""
        print(f"{task} : {a['runs']} run(s), résolue {a['resolved_rate']:.2f}{stable}, "
              f"CV coût {cv_cost}, CV tokens {cv_tokens}, miss serveur {a['server_misses']}")
    print(f"baseline : {out}")


if __name__ == "__main__":
    main()
