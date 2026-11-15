#!/usr/bin/env python3
"""Mesures de latence de l'A/B YA-09 `reasoningSummary` (`ya09-summary-plan.md`), hors ab_stats.py.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya09-summary-mesures.py bench/smoke/runs/ab-<horodatage>

A = `settings/default.json` (`reasoning.summary: "auto"`), B = `settings/reasoning-summary-off.json`.

Méthode (celle de `ya14-codex-20261114-mesures.py`, fonctions `load` et `ols`) :
- unités (passe, tâche) et paires écartées : `ab_stats.units_from_plan`, même règle d'écart que
  `ab_stats.py` (une paire écartée sort des deux bras) ;
- appels : requêtes de génération au reverse proxy (`proxy.ndjson` du run de la tâche), chaque
  `request` appariée à sa `response` dans l'ordre, prewarm exclu (`generate: false`). Temps
  modèle = horodatage de la réponse − horodatage de la requête ; tokens de sortie et de
  raisonnement lus dans l'usage de la réponse. Appel avec raisonnement : `reasoning_tokens` > 0 ;
- latence fixe d'un bras : ordonnée à l'origine des moindres carrés du temps modèle sur les tokens
  de sortie, sur ses appels avec raisonnement. Écart B − A en secondes ;
- phase agent : `agent.seconds` du résultat de la tâche, moyenne par tâche sur ses passes
  valides, dans chaque bras. Écart B/A − 1 sur la somme des moyennes par tâche (appariée par
  tâche), et écart moyen par tâche en secondes ;
- IC 95 % : bootstrap sur les tâches (`ab_stats.bootstrap`, 10 000 tirages, graine 0). À chaque
  tirage, les tâches sont tirées avec remise et l'ajustement est refait sur tous les appels des
  tâches tirées ;
- populations : « toutes » (tâches avec au moins une paire valide, celle de la décision) et
  « communes » (tâches résolues à toutes leurs passes valides par les deux bras, affichée sans
  décider ; c'est la population de la section « Latence par appel » du rapport YA-14).

La condition de latence de la règle de verdict (plan, règle 3) est affichée sur la population
« toutes ». Elle ne joue que si ni la résolution ni le coût n'ont tranché (`ab_stats.py`).

Bibliothèque standard, lecture seule."""
from __future__ import annotations

import argparse
import json
import statistics as st
import sys
from collections import Counter
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import ab_stats  # noqa: E402

DRAWS = 10_000
SEED = 0
DURATION_THRESHOLD_PCT = 10.0


def jl(path: Path) -> list[dict]:
    return [json.loads(x) for x in path.read_text().splitlines() if x.strip()]


def calls_of(task_run: Path) -> list[tuple[float, int, int]]:
    """(temps modèle en s, tokens de sortie, tokens de raisonnement) des appels de génération."""
    log = jl(task_run / "proxy.ndjson")
    reqs = [r for r in log if r.get("event") == "request"]
    resps = [r for r in log if r.get("event") == "response"]
    gen = [(q, s) for q, s in zip(reqs, resps) if q.get("generate") is not False]
    return [((s["ts"] - q["ts"]) / 1000, s["usage"].get("output_tokens", 0),
             s["usage"].get("reasoning_tokens", 0)) for q, s in gen]


def load(plan: Path) -> tuple[dict, list[dict], list[dict]]:
    units, ab = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    kept = [u for u in units if not u["causes"]]
    rows = []
    for unit in kept:
        for side in "AB":
            arm = unit["arms"][side]
            result = arm["result"]
            rows.append({
                "pass": unit["pass"], "task": unit["task"], "arm": side,
                "ok": result["status"] == "resolved",
                "agent": result["agent"]["seconds"],
                "calls": calls_of(ab_stats.task_dir(arm["run"], result)),
            })
    return ab, units, rows


def sums(points: list[tuple[float, int, int]]) -> tuple[float, float, float, float, float]:
    """Sommes suffisantes des moindres carrés (n, Σx, Σy, Σx², Σxy), x = tokens de sortie."""
    n = sx = sy = sxx = sxy = 0.0
    for y, x, _ in points:
        n += 1
        sx += x
        sy += y
        sxx += x * x
        sxy += x * y
    return n, sx, sy, sxx, sxy


def ols(total: tuple[float, ...]) -> tuple[float, float] | None:
    """(ordonnée à l'origine en s, pente en s/token), ou None si l'ajustement est dégénéré."""
    n, sx, sy, sxx, sxy = total
    den = n * sxx - sx * sx
    if n < 2 or den == 0:
        return None
    slope = (n * sxy - sx * sy) / den
    return (sy - slope * sx) / n, slope


def add(parts: list[tuple[float, ...]]) -> tuple[float, ...]:
    return tuple(sum(p[i] for p in parts) for i in range(5))


def by_task(rows: list[dict], tasks: list[str], arm: str, select) -> dict[str, tuple[float, ...]]:
    return {t: sums([c for x in rows if x["task"] == t and x["arm"] == arm
                     for c in x["calls"] if select(c)]) for t in tasks}


def fixed_latency(rows: list[dict], tasks: list[str], select, label: str) -> dict:
    per = {arm: by_task(rows, tasks, arm, select) for arm in "AB"}
    fits = {arm: ols(add(list(per[arm].values()))) for arm in "AB"}
    counts = {arm: int(add(list(per[arm].values()))[0]) for arm in "AB"}

    def diff(sample: list[str]) -> float | None:
        a = ols(add([per["A"][t] for t in sample]))
        b = ols(add([per["B"][t] for t in sample]))
        return None if a is None or b is None else b[0] - a[0]

    value = diff(tasks)
    lo, hi = ab_stats.bootstrap(tasks, diff, DRAWS, SEED)
    for arm in "AB":
        fit = fits[arm]
        text = (f"{fit[0]:.2f} s + {fit[1] * 1000:.1f} ms/token" if fit else "ajustement impossible")
        print(f"- {label}, {arm} : {counts[arm]} appels ; {text}")
    if value is None:
        print(f"- {label}, B − A : ajustement impossible")
    else:
        print(f"- **{label}, latence fixe B − A : {value:+.2f} s [{lo:+.2f} ; {hi:+.2f}]**")
    return {"value": value, "lo": lo, "hi": hi}


def agent_phase(rows: list[dict], tasks: list[str]) -> dict:
    mean = {(t, arm): st.mean(x["agent"] for x in rows if x["task"] == t and x["arm"] == arm)
            for t in tasks for arm in "AB"}

    def ratio(sample: list[str]) -> float | None:
        a = sum(mean[t, "A"] for t in sample)
        return None if a == 0 else (sum(mean[t, "B"] for t in sample) / a - 1) * 100

    def mean_diff(sample: list[str]) -> float:
        return st.mean(mean[t, "B"] - mean[t, "A"] for t in sample)

    value = ratio(tasks)
    lo, hi = ab_stats.bootstrap(tasks, ratio, DRAWS, SEED)
    dlo, dhi = ab_stats.bootstrap(tasks, mean_diff, DRAWS, SEED)
    a = st.mean(mean[t, "A"] for t in tasks)
    b = st.mean(mean[t, "B"] for t in tasks)
    print(f"- phase agent, moyenne par tâche : A {a:.1f} s, B {b:.1f} s")
    print(f"- **phase agent, B/A − 1 : {value:+.1f} % [{lo:+.1f} ; {hi:+.1f}]** ; écart moyen par "
          f"tâche {mean_diff(tasks):+.1f} s [{dlo:+.1f} ; {dhi:+.1f}]")
    return {"value": value, "lo": lo, "hi": hi}


def section(rows: list[dict], tasks: list[str], title: str) -> tuple[dict, dict]:
    print(f"\n## {title} ({len(tasks)} tâches : {', '.join(tasks)})")
    for arm in "AB":
        r = [x for x in rows if x["arm"] == arm and x["task"] in tasks]
        calls = [c for x in r for c in x["calls"]]
        print(f"- {arm} : {len(r)} lancements ; appels par lancement {len(calls) / len(r):.1f} "
              f"(avec raisonnement {sum(c[2] > 0 for c in calls) / len(r):.1f}) ; temps modèle "
              f"médian par appel {st.median(c[0] for c in calls):.2f} s ; tokens de sortie par "
              f"lancement {sum(c[1] for c in calls) / len(r):.0f}, de raisonnement "
              f"{sum(c[2] for c in calls) / len(r):.0f}")
    latency = fixed_latency(rows, tasks, lambda c: c[2] > 0, "avec raisonnement")
    fixed_latency(rows, tasks, lambda c: c[2] == 0, "sans raisonnement (contrôle)")
    duration = agent_phase(rows, tasks)
    return latency, duration


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("ab_dir", type=Path, help="dossier de l'A/B (contient ab.json)")
    args = parser.parse_args()
    ab, units, rows = load(args.ab_dir / "ab.json")
    dropped = [u for u in units if u["causes"]]
    print(f"A/B {args.ab_dir.name} ; graine {ab['seed']} ; binaire {ab['binary_sha']} ; modèle "
          f"{ab['model']}, effort {ab['effort']} ; unités {len(units)}, retenues "
          f"{len(units) - len(dropped)}, écartées {len(dropped)}")
    for unit in dropped:
        print(f"- écartée : passe {unit['pass']}, {unit['task']} : {'; '.join(unit['causes'])}")
    print(f"Statuts : {dict(Counter((x['arm'], x['ok']) for x in rows))}")

    tasks = sorted({x["task"] for x in rows})
    passes = {t: {x["pass"] for x in rows if x["task"] == t} for t in tasks}
    common = [t for t in tasks
              if all(x["ok"] for x in rows if x["task"] == t) and passes[t]]
    latency, duration = section(rows, tasks, "Toutes les tâches avec une paire valide (décision)")
    section(rows, common, "Tâches résolues à toutes leurs passes valides par les deux bras "
            "(affiché sans décider)")

    print("\n## Condition de latence de la règle de verdict (plan, règle 3 ; population « toutes »)")
    duration_ok = (duration["hi"] < 0 and duration["value"] is not None
                   and abs(duration["value"]) > DURATION_THRESHOLD_PCT)
    latency_ok = latency["value"] is not None and latency["hi"] < 0
    print(f"- phase agent : IC sous 0 et |écart| > {DURATION_THRESHOLD_PCT:g} % : "
          f"{'oui' if duration_ok else 'non'}")
    print(f"- latence fixe avec raisonnement : IC sous 0 : {'oui' if latency_ok else 'non'}")
    print(f"- **condition remplie : {'oui' if duration_ok and latency_ok else 'non'}** "
          "(ne joue que si ni la résolution ni le coût n'ont tranché, ab_stats.py)")


if __name__ == "__main__":
    main()
