#!/usr/bin/env python3
"""Miss et relectures partielles du cache serveur, croisés avec leurs causes possibles (YA-09, ticket 14).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-<horodatage>/ab.json [--tasks a,b]
    python3 -I bench/smoke/tools/cache_causes.py <run> [<run> ...]

Lit le journal du reverse proxy de chaque run de tâche (`proxy.ndjson`), seul : la mesure vaut
donc pour les deux harness (bras yusAgent et bras Codex CLI). Si le run a été lancé avec
`--log-bodies`, lit aussi `proxy-bodies.ndjson`.

Par session (un run de tâche) : les requêtes de génération, appariées dans l'ordre aux réponses
de génération. Une requête `generate: false` (prewarm de Codex CLI) et sa réponse sont comptées à
part et ne sont pas des appels. Prompt d'un appel : `usage.input_tokens` (cache compris).
Définitions de `cache_calls.py` : **miss** = `cached_tokens = 0` hors premier appel ;
**relecture partielle** = `0 < cached_tokens < ⌊prompt précédent⌋₁₂₈ − 256`, hors premier appel.

Pour chaque appel hors premier :
- **préfixe relu** : l'appel antérieur le plus récent dont le prompt arrondi au bloc de 128
  (à un bloc près) égale `cached_tokens` (1 = l'appel précédent, 2 = celui d'avant, ...) ;
- **âge** : début de la requête − début de la première requête de génération de la session ;
- **connexion** : neuve (premier appel de génération sur un WebSocket ouvert depuis l'appel
  précédent, prewarm non compté) ou réutilisée ; transport ; suite de conversation (`previous_response_id`) ;
- avec les corps : corps hors `input` et `previous_response_id` identique à l'appel précédent.

Sortie : par bras (ou par run), totaux, puis miss et relectures partielles par tranche de taille
du prompt précédent, d'âge et de rang, préfixe relu des relectures partielles, et premiers appels
qui relisent du cache. Lecture seule, bibliothèque standard.
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
BLOCK = 128
PARTIAL_MARGIN = 256
# Tranches [bas, haut[ et leur libellé.
SIZE_BANDS = [(0, 3000, "< 3k"), (3000, 6000, "3k-6k"), (6000, 9000, "6k-9k"),
              (9000, 16000, "9k-16k"), (16000, None, ">= 16k")]
AGE_BANDS = [(0, 10, "< 10"), (10, 30, "10-30"), (30, 60, "30-60"), (60, None, ">= 60")]
RANK_BANDS = [(2, 3, "2"), (3, 5, "3-4"), (5, 11, "5-10"), (11, None, ">= 11")]


def read_ndjson(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def band(value: float, bands: list[tuple]) -> str:
    for lo, hi, label in bands:
        if value >= lo and (hi is None or value < hi):
            return label
    return "?"


def session_calls(proxy: list[dict], bodies: list[dict] | None = None) -> dict:
    """Appels de génération d'une session, avec leurs mesures. `proxy` : lignes de proxy.ndjson."""
    requests, responses, opened = [], [], False
    prewarm = Counter()
    for row in proxy:
        event = row.get("event")
        if event == "ws_open":
            opened = True
        elif event == "request":
            if row.get("generate") is False:
                prewarm["requests"] += 1
                continue
            requests.append({**row, "new_connection": opened})
            opened = False
        elif event == "response":
            if row.get("generate") is False:
                prewarm["responses"] += 1
                continue
            responses.append(row)
    if bodies is not None:
        bodies = [b["body"] if isinstance(b["body"], dict) else json.loads(b["body"]) for b in bodies]
        bodies = [b for b in bodies if b.get("generate") is not False]
        if len(bodies) != len(requests):
            bodies = None
    calls, problems = [], []
    if len(requests) != len(responses):
        problems.append(f"{len(requests)} requêtes de génération pour {len(responses)} réponses")
    for i, (req, resp) in enumerate(zip(requests, responses)):
        usage = resp.get("usage") or {}
        prompt, read = usage.get("input_tokens"), usage.get("cached_tokens")
        if prompt is None or read is None:
            problems.append(f"appel {i + 1} sans usage")
            break
        call = {"rank": i + 1, "prompt": prompt, "read": read, "ts": req["ts"],
                "transport": req.get("transport"), "continuation": bool(req.get("continuation")),
                "new_connection": req["new_connection"], "first": i == 0, "miss": False,
                "partial": False, "back": None, "age": None, "prev_prompt": None, "same_body": None}
        if calls:
            prev = calls[-1]
            call["prev_prompt"] = prev["prompt"]
            call["age"] = (req["ts"] - calls[0]["ts"]) / 1000
            call["miss"] = read == 0
            call["partial"] = 0 < read < (prev["prompt"] // BLOCK) * BLOCK - PARTIAL_MARGIN
            for back, earlier in enumerate(reversed(calls), start=1):
                floor = (earlier["prompt"] // BLOCK) * BLOCK
                if read > 0 and floor - BLOCK <= read <= floor:
                    call["back"] = back
                    break
            if bodies is not None:
                strip = lambda b: {k: v for k, v in b.items() if k not in ("input", "previous_response_id")}
                call["same_body"] = strip(bodies[i]) == strip(bodies[i - 1])
        calls.append(call)
    return {"calls": calls, "prewarm": prewarm, "problems": problems}


def task_runs_from_plan(plan_path: Path, tasks: set[str] | None) -> list[tuple[str, Path]]:
    plan = json.loads(plan_path.read_text())
    out = []
    for arm in plan["arms"]:
        if tasks and arm["task"] not in tasks:
            continue
        for task_run in sorted((REPO / arm["run_dir"] / "tasks" / arm["task"]).glob("run-*")):
            out.append((arm["arm"], task_run))
    return out


def task_runs_from_dirs(runs: list[Path], tasks: set[str] | None) -> list[tuple[str, Path]]:
    out = []
    for run in runs:
        for task_run in sorted(run.glob("tasks/*/run-*")):
            if tasks and task_run.parent.name not in tasks:
                continue
            out.append((run.name, task_run))
    return out


def summarize(sessions: list[dict]) -> dict:
    later = [c for s in sessions for c in s["calls"] if not c["first"]]
    firsts = [c for s in sessions for c in s["calls"] if c["first"]]

    def table(key, bands):
        rows = defaultdict(lambda: [0, 0, 0])
        for c in later:
            k = band(c[key], bands)
            rows[k][0] += c["miss"]
            rows[k][1] += c["partial"]
            rows[k][2] += 1
        order = [label for _, _, label in bands]
        return [(k, *rows[k]) for k in order if k in rows]

    with_body = [c for c in later if c["same_body"] is not None]
    return {
        "sessions": len(sessions),
        "calls": len(later) + len(firsts),
        "later": len(later),
        "miss": sum(c["miss"] for c in later),
        "partial": sum(c["partial"] for c in later),
        "continuation": sum(c["continuation"] for c in later),
        "not_websocket": sum(c["transport"] != "websocket" for c in later + firsts),
        "new_connection_later": [sum(c["new_connection"] for c in later),
                                 sum(c["miss"] for c in later if c["new_connection"]),
                                 sum(c["partial"] for c in later if c["new_connection"])],
        "prewarm": sum(s["prewarm"]["requests"] for s in sessions),
        "first_read": sum(c["read"] > 0 for c in firsts),
        "by_size": table("prev_prompt", SIZE_BANDS),
        "by_age": table("age", AGE_BANDS),
        "by_rank": table("rank", RANK_BANDS),
        "partial_back": sorted(Counter(c["back"] for c in later if c["partial"]).items(),
                               key=lambda kv: (kv[0] is None, kv[0] or 0)),
        "bodies": [len(with_body), sum(not c["same_body"] for c in with_body),
                   sum((c["miss"] or c["partial"]) and c["same_body"] and c["continuation"] for c in with_body),
                   sum(c["miss"] or c["partial"] for c in with_body)],
        "problems": [p for s in sessions for p in s["problems"]],
    }


def pct(n: int, d: int) -> str:
    return f"{n}/{d} ({100 * n / d:.1f} %)" if d else f"{n}/0"


def show(name: str, s: dict) -> None:
    print(f"## {name}\n")
    print(f"- Sessions {s['sessions']}, appels {s['calls']}, hors premier {s['later']} ; "
          f"miss {pct(s['miss'], s['later'])}, relectures partielles {pct(s['partial'], s['later'])}")
    print(f"- Hors premier : en suite de conversation {s['continuation']}/{s['later']} ; "
          f"requêtes hors WebSocket {s['not_websocket']} ; sur une connexion neuve "
          f"{s['new_connection_later'][0]} (dont miss {s['new_connection_later'][1]}, "
          f"partiels {s['new_connection_later'][2]})")
    print(f"- Prewarm (`generate: false`) : {s['prewarm']} ; premiers appels qui relisent du cache : "
          f"{s['first_read']}/{s['sessions']}")
    if s["bodies"][0]:
        print(f"- Corps journalisés : {s['bodies'][0]} appels hors premier, corps hors `input` différent "
              f"du précédent : {s['bodies'][1]} ; miss ou partiels à corps identique et en suite de "
              f"conversation : {s['bodies'][2]}/{s['bodies'][3]}")
    for title, key in (("taille du prompt précédent (tokens)", "by_size"), ("âge dans la session (s)", "by_age"),
                       ("rang de l'appel", "by_rank")):
        print(f"\n| {title} | miss | partiels | appels | taux de miss |\n|---|---|---|---|---|")
        for k, miss, partial, n in s[key]:
            print(f"| {k} | {miss} | {partial} | {n} | {100 * miss / n:.1f} % |")
    print("\nPréfixe relu par les relectures partielles (1 = appel précédent, 2 = celui d'avant, "
          "aucun = pas d'appel antérieur à ce compte) : "
          + ", ".join(f"{'aucun' if k is None else k} : {v}" for k, v in s["partial_back"]))
    for p in s["problems"]:
        print(f"- problème : {p}")
    print()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", nargs="*", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--tasks", help="tâches retenues, séparées par des virgules")
    args = parser.parse_args(argv)
    tasks = set(args.tasks.split(",")) if args.tasks else None
    if bool(args.plan) == bool(args.runs):
        parser.error("--plan ou des dossiers de run, pas les deux")
    task_runs = task_runs_from_plan(args.plan, tasks) if args.plan else task_runs_from_dirs(args.runs, tasks)
    groups = defaultdict(list)
    for name, task_run in task_runs:
        bodies_path = task_run / "proxy-bodies.ndjson"
        bodies = read_ndjson(bodies_path) if bodies_path.exists() else None
        groups[name].append(session_calls(read_ndjson(task_run / "proxy.ndjson"), bodies))
    for name in sorted(groups):
        show(f"Bras {name}" if args.plan else name, summarize(groups[name]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
