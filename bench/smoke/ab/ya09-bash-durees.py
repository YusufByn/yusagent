#!/usr/bin/env python3
"""Durées des commandes shell d'un A/B yusAgent contre Codex CLI (run YA-14), pour la fenêtre de `bash()`.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya09-bash-durees.py bench/smoke/runs/ab-20261114T201248Z

Sert au choix de la valeur de `bashWaitSeconds` (`bench/smoke/ab/ya09-bash-plan.md`).

- **Codex CLI** (bras dont le dossier contient `codex`) : `Wall time X seconds` de chaque sortie
  de tool du rollout (`sessions/rollout-*.jsonl`), arrondi par Codex au dixième de seconde. C'est
  la durée de la cellule `exec`, donc de ses commandes, tant qu'elles finissent avant leur
  `yield_time_ms`. Les sorties `Process running with session ID` (commande pas finie au yield)
  sont comptées à part.
- **yusAgent** (autres bras) : cellules revenues avant la fin de leur commande, règle de
  `ya14-codex-20261114-mesures.py` : avis `still running after` dans le stderr de la cellule ou
  dans son texte, **ou** repr `BashSnapshot running` dans le texte. L'avis du stderr n'est écrit
  que 3 fois par kernel (`prime-agent-runtime/src/rlm/bash.py`, `_MAX_DEGRADE_HINTS`) : les
  suivantes ne se voient qu'à la repr. Les deux comptes sont affichés.
  Durée d'une commande revenue avant la fin : de `tool_started` de sa cellule à l'horodatage de
  la notification `async_bash_completion` de la même commande (commande de l'avis contenue dans
  le code de la cellule, la plus récente d'abord). C'est une **borne haute** : l'avis est daté à
  son insertion dans la session. Les avis sans cellule appariée sont comptés.
- Couverture : part des commandes mesurées qui finissent sous chaque fenêtre candidate.

Bibliothèque standard, lecture seule."""
from __future__ import annotations

import argparse
import json
import re
from datetime import datetime
from pathlib import Path

WALL = re.compile(r"Wall time ([0-9.]+) seconds")
RUNNING = "Process running with session ID"
WINDOWS = (1, 5, 10, 20, 30, 45, 60)


def jl(path: Path) -> list[dict]:
    return [json.loads(x) for x in path.read_text().splitlines() if x.strip()]


def stamp(value: str) -> float:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()


def text(message: dict) -> str:
    return "".join(p.get("text", "") for p in message.get("content") or [] if isinstance(p, dict))


def quantiles(values: list[float]) -> str:
    if not values:
        return "aucune valeur"
    ordered = sorted(values)
    pick = lambda p: ordered[min(len(ordered) - 1, int(p / 100 * len(ordered)))]  # noqa: E731
    return (f"médiane {pick(50):.1f} s, p75 {pick(75):.1f}, p90 {pick(90):.1f}, p95 {pick(95):.1f}, "
            f"p99 {pick(99):.1f}, max {ordered[-1]:.1f}")


def coverage(values: list[float]) -> str:
    return " ; ".join(f"≤ {w} s : {sum(v <= w for v in values)}/{len(values)}" for w in WINDOWS)


def codex(run: Path) -> None:
    walls, running = [], 0
    for rollout in sorted(run.glob("arms/*codex*/tasks/*/run-1/sessions/rollout-*.jsonl")):
        for line in rollout.read_text().splitlines():
            walls += [float(x) for x in WALL.findall(line)]
            running += line.count(RUNNING)
    print("## Codex CLI : `Wall time` des sorties de tools")
    print(f"- {len(walls)} sorties ; commandes pas finies au yield : {running}")
    print(f"- {quantiles(walls)}")
    print(f"- au-dessus de 1 s : {sum(w > 1 for w in walls)} ; de 10 s : {sum(w > 10 for w in walls)} ; "
          f"de 30 s : {sum(w > 30 for w in walls)} ; de 60 s : {sum(w > 60 for w in walls)}")
    print(f"- couverture : {coverage(walls)}")


def yusagent(run: Path) -> None:
    hinted = repr_only = unmatched = 0
    durations: list[float] = []
    for session in sorted(run.glob("arms/*/tasks/*/run-1/sessions/*.jsonl")):
        if "codex" in session.parts[-6] or session.name.endswith(".usage.ndjson"):
            continue
        started: dict[str, float] = {}
        code: dict[str, str] = {}
        pending: list[tuple[float, str]] = []
        for entry in jl(session):
            if entry.get("type") == "tool_started":
                started[entry.get("toolCallId")] = stamp(entry["timestamp"])
                code[entry.get("toolCallId")] = (entry.get("args") or {}).get("code", "")
            if entry.get("type") == "custom_message" and entry.get("customType") == "async_bash_completion":
                command = (entry.get("details") or {}).get("command", "")
                hits = [p for p in pending if command and command[:200] in p[1]]
                if hits:
                    pending.remove(hits[-1])
                    durations.append(stamp(entry["timestamp"]) - hits[-1][0])
                else:
                    unmatched += 1
            message = entry.get("message") or {}
            if entry.get("type") != "message" or message.get("role") != "toolResult":
                continue
            details = message.get("details") or {}
            body = text(message)
            hint = "still running after" in (details.get("stderr") or "") or "still running after" in body
            snapshot = "BashSnapshot running" in body
            if hint or snapshot:
                hinted += hint
                repr_only += snapshot and not hint
                call = message.get("toolCallId")
                pending.append((started.get(call, stamp(entry["timestamp"])), code.get(call, "")))
    print("\n## yusAgent : commandes revenues avant leur fin (fenêtre de 1 s)")
    print(f"- cellules revenues avant la fin : **{hinted + repr_only}** = {hinted} avec l'avis "
          f"`still running after` + {repr_only} avec la seule repr `BashSnapshot running`")
    print(f"- durées appariées à leur notification : {len(durations)} ; notifications sans cellule "
          f"appariée : {unmatched}")
    print(f"- borne haute : {quantiles(durations)}")
    print(f"- couverture : {coverage(durations)}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("ab_dir", type=Path, help="dossier de l'A/B (contient arms/)")
    args = parser.parse_args()
    codex(args.ab_dir)
    yusagent(args.ab_dir)


if __name__ == "__main__":
    main()
