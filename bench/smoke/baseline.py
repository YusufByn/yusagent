#!/usr/bin/env python3
"""Baseline du smoke bench (T07, YA-14) : bruit mesuré sur plusieurs runs de `main`.

Usage :
    python3 -I bench/smoke/baseline.py <run> [<run> ...] [--exclude <run>:<tâche>[,<tâche>...]]
        [--out bench/smoke/baseline/<modèle>.json]

Lit les `report.json` des runs donnés (un run à `RUNS=3`, ou plusieurs runs à `RUNS=1`),
tous du même modèle, effort, réglages, catalogue et tarif, et écrit par tâche : taux de
résolution, stabilité (résolue à tous les runs), moyenne, écart-type et coefficient de
variation du coût et des tokens, tours moyens, miss serveur et taux de cache évitable.
Miss serveur et taux évitable sont relus dans les journaux de chaque run de tâche
(`metrics.task_run_cache`) : un run fait avant leur ajout au rapport est donc couvert.

Version des tâches : chaque résultat porte l'empreinte du dossier de sa tâche
(`task_version`, run.py). Une tâche dont les runs ont deux versions, ou une version
inconnue (run fait avant l'empreinte et non recalculé par `run.py --rereport`, ou arbre
modifié au moment du run), est refusée : rien n'est écrit. `--exclude <run>:<tâches>`
retire des tâches d'un run (par exemple celles dont l'énoncé a changé depuis).

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


def parse_excludes(values: list[str], reports: list[tuple[Path, dict]]) -> dict[str, set[str]]:
    """`<run>:<tâche>[,<tâche>...]` -> {nom du run: tâches}. Run ou tâche absents : erreur."""
    excluded: dict[str, set[str]] = {}
    known = {run.name: {r["task"] for r in report["results"]} for run, report in reports}
    for value in values:
        run, sep, tasks = value.rpartition(":")
        name = Path(run).name
        if not sep or not run or not tasks:
            sys.exit(f"--exclude {value} : format attendu <run>:<tâche>[,<tâche>...]")
        if name not in known:
            sys.exit(f"--exclude {value} : run {name} absent des runs donnés")
        ids = {t for t in tasks.split(",") if t}
        unknown = sorted(ids - known[name])
        if unknown:
            sys.exit(f"--exclude {value} : tâches absentes du run {name} : {', '.join(unknown)}")
        excluded.setdefault(name, set()).update(ids)
    return excluded


def task_versions(task: str, items: list[tuple[Path, dict]]) -> tuple[str | None, str | None]:
    """(empreinte commune, motif de refus) des runs retenus d'une tâche."""
    trees = {run.name: (r.get("task_version") or {}).get("tree") for run, r in items}
    unknown = sorted(name for name, tree in trees.items() if tree is None)
    if unknown:
        return None, f"version inconnue dans {', '.join(unknown)}"
    if len(set(trees.values())) > 1:
        detail = ", ".join(f"{name} {tree[:12]}" for name, tree in sorted(trees.items()))
        return None, f"versions mélangées : {detail}"
    return next(iter(trees.values())), None


def build(runs: list[Path], exclude: list[str] | None = None) -> dict:
    names = [run.name for run in runs]
    if len(set(names)) != len(names):
        sys.exit("deux runs portent le même nom : --exclude serait ambigu")
    reports = [(run, json.loads((run / "report.json").read_text())) for run in runs]
    excluded = parse_excludes(exclude or [], reports)
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
            if result["task"] not in excluded.get(run.name, set()):
                tasks.setdefault(result["task"], []).append((run, result))
    versions = {task: task_versions(task, items) for task, items in tasks.items()}
    refused = {task: reason for task, (_, reason) in versions.items() if reason}
    if refused:
        sys.exit("tâches refusées, aucune baseline écrite :\n" + "\n".join(
            f"- {task} : {reason}" for task, reason in sorted(refused.items())))
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
            "task_tree": versions[task][0],
            "runs": len(results),
            "from_runs": [f"{run.name}/run-{r['run']}" for run, r in items],
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
        "command": "python3 -I bench/smoke/baseline.py " + " ".join(
            [str(r) for r in runs] + [f"--exclude {e}" for e in exclude or []]),
        "excluded": {name: sorted(ids) for name, ids in sorted(excluded.items())},
        "runs": [{"run": run.name, "commit": r["header"].get("commit"),
                  "binary_sha256": r["header"].get("binary_sha256"),
                  "runs_per_task": r["header"].get("runs")} for run, r in reports],
        "header": {k: first.get(k) for k in SAME},
        "per_task": per_task,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", nargs="+", type=Path)
    parser.add_argument("--exclude", action="append", default=[],
                        metavar="RUN:TÂCHE[,TÂCHE...]",
                        help="retire ces tâches de ce run (répétable)")
    parser.add_argument("--out", type=Path, help="défaut : bench/smoke/baseline/<modèle>.json")
    args = parser.parse_args()
    baseline = build(args.runs, args.exclude)
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
