#!/usr/bin/env python3
"""Mesures de l'A/B YA-09 `bashWaitSeconds` (`ya09-bash-plan.md`), hors ab_stats.py.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya09-bash-mesures.py bench/smoke/runs/ab-<horodatage>

A = `settings/default.json` (fenêtre de 1 s), B = `settings/bash-wait-30.json` (30 s).

Règle de décision (plan, règle 3, seulement si ni la résolution ni le coût n'ont tranché dans
`ab_stats.py`), population « toutes » (tâches avec au moins une paire valide) :
- **appels au modèle de la session principale par lancement** (`calls.by_kind.main` du résultat
  de la tâche, `report.json`) : moyenne par tâche sur ses passes valides dans chaque bras, écart
  B/A − 1 sur la somme des moyennes par tâche (apparié par tâche), IC 95 % par bootstrap sur les
  tâches (`ab_stats.bootstrap`, 10 000 tirages, graine 0). Condition : borne haute de l'IC sous 0
  et écart sous −10 % ;
- **garde-fou de durée** : phase agent (`agent.seconds`), même méthode ; condition : borne haute
  de l'IC à +10 % au plus.

Unités et paires écartées : `ab_stats.units_from_plan`, même règle d'écart qu'`ab_stats.py`.

Affiché sans décider (par bras, par lancement) :
- durée de la phase agent : moyenne et **médiane par lancement** ;
- cellules revenues avant la fin de leur commande (avis `still running after` ou repr
  `BashSnapshot running`, règle de `ya14-codex-20261114-mesures.py`), tours de relève (cellule
  qui ne fait que lire un handle, `classify` de ce script) et notifications `[bash-done]`
  (`async_bash_completion`), lus dans la session ;
- **silence sans requête sur le WebSocket** (reverse proxy, `proxy.ndjson`) : sur chaque
  connexion, temps entre la fin d'une réponse et la requête suivante, ou la fermeture de la
  connexion ; maximum par lancement, maximum du bras, nombre de silences de plus de 60 s ;
- lancements avec une bascule SSE (`transports.loop_on_sse` > 0) ou un échec de transport
  (`transport_failures_in_session` > 0) ;
- **bascules 1011** : diagnostics `provider_transport_failure` de code 1011 (`WebSocket closed
  1011 keepalive ping timeout`, constat du 15/11) portés par les messages de `out.jsonl`, comptés
  une fois par horodatage ;
- **requêtes en SSE** : entrées `per_request` du résultat de transport `sse`, et **taux de cache de
  ces requêtes** : somme de `cacheRead` / somme de (`input` + `cacheRead`) de leurs lignes du
  journal d'usage (`sessions/*.usage.ndjson`, jointure par session et `seq`).

Bibliothèque standard, lecture seule."""
from __future__ import annotations

import argparse
import json
import re
import statistics as st
import sys
from collections import Counter
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import ab_stats  # noqa: E402

DRAWS = 10_000
SEED = 0
CALLS_THRESHOLD_PCT = 10.0
DURATION_GUARD_PCT = 10.0
LONG_SILENCE_S = 60.0
# Règle « relève » de ya14-codex-20261114-mesures.py.
POLL = re.compile(r"\.handle\.(output|poll|wait)\(|\.running\b|\.poll\(\)|^print\(''\)$"
                  r"|^print\('started'\)$|__dict__")


def jl(path: Path) -> list[dict]:
    return [json.loads(x) for x in path.read_text().splitlines() if x.strip()]


def text(message: dict) -> str:
    return "".join(p.get("text", "") for p in message.get("content") or [] if isinstance(p, dict))


def is_poll(code: str) -> bool:
    c = code.strip()
    return bool(POLL.search(c)) and "bash(" not in c and "exec_command" not in c


def session_counts(task_run: Path) -> dict:
    counts = Counter({"notices": 0, "polls": 0, "snapshots": 0})
    for session in (task_run / "sessions").glob("*.jsonl"):
        for entry in jl(session):
            if entry.get("type") == "custom_message" and entry.get("customType") == "async_bash_completion":
                counts["notices"] += 1
            message = entry.get("message") or {}
            if entry.get("type") != "message":
                continue
            if message.get("role") == "assistant":
                for part in message.get("content") or []:
                    if part.get("type") == "toolCall":
                        counts["polls"] += is_poll((part.get("arguments") or {}).get("code") or "")
            elif message.get("role") == "toolResult":
                details = message.get("details") or {}
                body = text(message)
                counts["snapshots"] += ("still running after" in (details.get("stderr") or "")
                                        or "still running after" in body
                                        or "BashSnapshot running" in body)
    return counts


def silences(task_run: Path) -> list[float]:
    """Silences sans requête sur chaque connexion WebSocket, en secondes."""
    gaps, last_response = [], None
    for row in jl(task_run / "proxy.ndjson"):
        if row.get("transport") != "websocket":
            continue
        event = row.get("event")
        if event == "ws_open":
            last_response = None
        elif event == "request" and last_response is not None:
            gaps.append((row["ts"] - last_response) / 1000)
            last_response = None
        elif event == "response":
            last_response = row["ts"]
        elif event == "ws_close" and last_response is not None:
            gaps.append((row["ts"] - last_response) / 1000)
            last_response = None
    return gaps


def fallbacks_1011(task_run: Path) -> int:
    """Bascules sur fermeture 1011 du WebSocket, une par horodatage de diagnostic."""
    seen = set()
    out = task_run / "out.jsonl"
    if not out.exists():
        return 0
    for line in out.read_text(errors="ignore").splitlines():
        if "1011" not in line:
            continue
        message = (json.loads(line).get("message") or {})
        for diag in message.get("diagnostics") or []:
            if (diag.get("type") == "provider_transport_failure"
                    and (diag.get("error") or {}).get("code") == 1011):
                seen.add(diag.get("timestamp"))
    return len(seen)


def sse_requests(task_run: Path, result: dict) -> tuple[int, int, int]:
    """(requêtes en SSE, somme d'input, somme de cacheRead) de ces requêtes."""
    sse = {(r.get("session"), r.get("seq")) for r in result.get("per_request") or []
           if r.get("transport") == "sse"}
    fresh = cached = 0
    for journal in (task_run / "sessions").glob("*.usage.ndjson"):
        for row in jl(journal):
            if (row.get("sessionId"), row.get("seq")) in sse:
                fresh += row.get("input") or 0
                cached += row.get("cacheRead") or 0
    return len(sse), fresh, cached


def load(plan: Path) -> tuple[dict, list[dict], list[dict]]:
    units, ab = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    rows = []
    for unit in (u for u in units if not u["causes"]):
        for side in "AB":
            arm = unit["arms"][side]
            result = arm["result"]
            task_run = ab_stats.task_dir(arm["run"], result)
            sse, sse_input, sse_cached = sse_requests(task_run, result)
            rows.append({
                "pass": unit["pass"], "task": unit["task"], "arm": side,
                "ok": result["status"] == "resolved",
                "calls": (result["calls"].get("by_kind") or {}).get("main", result["calls"]["total"]),
                "agent": result["agent"]["seconds"],
                "loop_on_sse": (result.get("transports") or {}).get("loop_on_sse", 0) > 0,
                "transport_failures": result.get("transport_failures_in_session") or 0,
                "silences": silences(task_run),
                "fallbacks_1011": fallbacks_1011(task_run),
                "sse": sse, "sse_input": sse_input, "sse_cached": sse_cached,
                **session_counts(task_run),
            })
    return ab, units, rows


def paired(rows: list[dict], tasks: list[str], key: str, label: str, unit: str) -> dict:
    mean = {(t, arm): st.mean(x[key] for x in rows if x["task"] == t and x["arm"] == arm)
            for t in tasks for arm in "AB"}

    def ratio(sample: list[str]) -> float | None:
        a = sum(mean[t, "A"] for t in sample)
        return None if a == 0 else (sum(mean[t, "B"] for t in sample) / a - 1) * 100

    def mean_diff(sample: list[str]) -> float:
        return st.mean(mean[t, "B"] - mean[t, "A"] for t in sample)

    value = ratio(tasks)
    lo, hi = ab_stats.bootstrap(tasks, ratio, DRAWS, SEED)
    dlo, dhi = ab_stats.bootstrap(tasks, mean_diff, DRAWS, SEED)
    print(f"- **{label}, B/A − 1 : {value:+.1f} % [{lo:+.1f} ; {hi:+.1f}]** ; écart moyen par tâche "
          f"{mean_diff(tasks):+.1f} {unit} [{dlo:+.1f} ; {dhi:+.1f}]")
    return {"value": value, "lo": lo, "hi": hi}


def per_launch(rows: list[dict], key: str) -> str:
    return f"{st.mean(x[key] for x in rows):.1f}"


def section(rows: list[dict], tasks: list[str], title: str) -> tuple[dict, dict]:
    print(f"\n## {title} ({len(tasks)} tâches : {', '.join(tasks)})")
    for arm in "AB":
        r = [x for x in rows if x["arm"] == arm and x["task"] in tasks]
        gaps = [g for x in r for g in x["silences"]]
        print(f"- {arm} : {len(r)} lancements ; appels de la session principale par lancement "
              f"{per_launch(r, 'calls')} ; phase agent moyenne {per_launch(r, 'agent')} s, "
              f"**médiane par lancement {st.median(x['agent'] for x in r):.1f} s**")
        print(f"  - par lancement : cellules revenues avant la fin {per_launch(r, 'snapshots')}, "
              f"relèves {per_launch(r, 'polls')}, notifications `[bash-done]` {per_launch(r, 'notices')}")
        print(f"  - silence sans requête sur le WebSocket : maximum {max(gaps, default=0):.1f} s ; "
              f"médiane des maxima par lancement "
              f"{st.median(max(x['silences'], default=0) for x in r):.1f} s ; silences de plus de "
              f"{LONG_SILENCE_S:g} s : {sum(g > LONG_SILENCE_S for g in gaps)}")
        print(f"  - lancements avec bascule SSE : {sum(x['loop_on_sse'] for x in r)} ; avec un échec de "
              f"transport : {sum(x['transport_failures'] > 0 for x in r)}")
        sse_input = sum(x["sse_input"] for x in r)
        sse_cached = sum(x["sse_cached"] for x in r)
        rate = (f"{sse_cached / (sse_input + sse_cached):.3f} ({sse_cached} en cache sur "
                f"{sse_input + sse_cached})" if sse_input + sse_cached else "sans objet")
        print(f"  - **bascules 1011 : {sum(x['fallbacks_1011'] for x in r)}** ; **requêtes en SSE : "
              f"{sum(x['sse'] for x in r)}** ; **taux de cache des requêtes SSE : {rate}**")
    calls = paired(rows, tasks, "calls", "appels de la session principale", "appels")
    duration = paired(rows, tasks, "agent", "phase agent", "s")
    return calls, duration


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
    common = [t for t in tasks if all(x["ok"] for x in rows if x["task"] == t)]
    calls, duration = section(rows, tasks, "Toutes les tâches avec une paire valide (décision)")
    if common:
        section(rows, common, "Tâches résolues à toutes leurs passes valides par les deux bras "
                "(affiché sans décider)")

    print("\n## Condition de la règle 3 du plan (population « toutes »)")
    calls_ok = (calls["value"] is not None and calls["hi"] < 0
                and calls["value"] < -CALLS_THRESHOLD_PCT)
    guard_ok = duration["hi"] <= DURATION_GUARD_PCT
    print(f"- appels : IC sous 0 et écart sous −{CALLS_THRESHOLD_PCT:g} % : "
          f"{'oui' if calls_ok else 'non'}")
    print(f"- phase agent : borne haute de l'IC à +{DURATION_GUARD_PCT:g} % au plus : "
          f"{'oui' if guard_ok else 'non'}")
    print(f"- **condition remplie : {'oui' if calls_ok and guard_ok else 'non'}** "
          "(ne joue que si ni la résolution ni le coût n'ont tranché, ab_stats.py)")


if __name__ == "__main__":
    main()
