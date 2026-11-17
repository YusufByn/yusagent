#!/usr/bin/env python3
"""Mesures et verdict de l'A/B YA-09 de l'en-tête `session-id` (`ya09-session-id-plan.md`).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya09-session-id-mesures.py bench/smoke/runs/ab-<horodatage>
    python3 -I bench/smoke/ab/ya09-session-id-mesures.py --sans-trace <dossier d'un autre A/B>
    python3 -I bench/smoke/ab/ya09-session-id-mesures.py --fil bench/smoke/runs/ab-<horodatage>

A = `settings/default.json` (réglage absent : en-tête coupé), B = `settings/session-id-header.json`
(`codexSessionIdHeader: true`). Écrit avant le run, avec la règle du plan.

Appels : appels de génération hors premier de chaque session de tâche des unités retenues
(`ab_stats.units_from_plan`, règle « A/B : erreurs provider » du README), lus au journal du
reverse proxy avec les définitions de `cache_causes.py` (`session_calls`) : miss =
`cached_tokens = 0`, relecture partielle = `0 < cached_tokens < ⌊prompt précédent⌋₁₂₈ − 256`,
prompt précédent = `input_tokens` de l'appel précédent de la session.

Sorties :
1. Métrique principale : taux de miss des appels dont le prompt précédent fait moins de 6 000
   tokens, par bras ; d = A − B en points (positif : moins de miss avec l'en-tête) ; IC 95 % par
   bootstrap sur les unités (10 000 tirages avec remise, graine 0) ; verdict de la règle.
2. Secondaires, même méthode : miss global (garde-fou de la règle), relectures partielles
   (affichées sans décider), et ces deux taux sur les appels à prompt précédent ≥ 6 000.
3. Coût rapporté par bras (Σ sur les unités retenues) et B / A − 1, affiché sans décider.
4. Trace : `yaRequest.codexSessionIdHeader` de l'en-tête de chaque session (première ligne de
   `sessions/*.jsonl`) : `true` dans toutes les sessions de B, `false` dans toutes celles de A.
   Sinon le verdict est suspendu (code 1). `--sans-trace` saute ce contrôle, pour vérifier
   l'outil sur un A/B antérieur au réglage.

`--fil` (A/B lancé avec `--capture`, aucun modèle) : en-têtes reçus par le serveur de capture
pour chaque requête des tâches (`tasks/*/run-*/capture/*.meta.json`, valeurs en clair sauf
l'authentification). Attendu : dans B, `session-id` présent et égal à `session_id` sur chaque
requête ; dans A, absent partout. Code 1 sinon.

Bibliothèque standard, lecture seule."""
from __future__ import annotations

import importlib.util
import json
import random
import sys
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"

SPLIT = 6000
THRESHOLD = 0.10  # 10 points, règle du plan
GUARD = 0.03  # 3 points, garde-fou du plan (miss global)
DRAWS, SEED = 10_000, 0


def load(name: str):
    spec = importlib.util.spec_from_file_location(f"smoke_{name}", TOOLS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def unit_calls(kept: list[dict], ab_stats, cache_causes) -> dict:
    """{(passe, tâche): {bras: [appels hors premier]}}, toutes sessions de la tâche confondues."""
    out = {}
    for unit in kept:
        per_arm = {}
        for side in ("A", "B"):
            arm = unit["arms"][side]
            task_run = ab_stats.task_dir(arm["run"], arm["result"])
            session = cache_causes.session_calls(
                cache_causes.read_ndjson(task_run / "proxy.ndjson"))
            per_arm[side] = [c for c in session["calls"] if not c["first"]]
        out[(unit["pass"], unit["task"])] = per_arm
    return out


def rates(data: dict, keys: list, field: str) -> tuple:
    """((n A, appels A), (n B, appels B), d = taux A − taux B ou None)."""
    an = sum(sum(c[field] for c in data[k]["A"]) for k in keys)
    ad = sum(len(data[k]["A"]) for k in keys)
    bn = sum(sum(c[field] for c in data[k]["B"]) for k in keys)
    bd = sum(len(data[k]["B"]) for k in keys)
    d = an / ad - bn / bd if ad and bd else None
    return (an, ad), (bn, bd), d


def bootstrap(data: dict, keys: list, field: str) -> tuple[float, float] | None:
    rng = random.Random(SEED)
    draws = []
    for _ in range(DRAWS):
        sample = [rng.choice(keys) for _ in keys]
        d = rates(data, sample, field)[2]
        if d is not None:
            draws.append(d)
    if not draws:
        return None
    draws.sort()
    return draws[int(0.025 * len(draws))], draws[int(0.975 * len(draws)) - 1]


def fmt(count: tuple) -> str:
    n, d = count
    return f"{n}/{d} ({100 * n / d:.1f} %)" if d else "0/0"


def row(name: str, data: dict, keys: list, field: str) -> tuple:
    a, b, d = rates(data, keys, field)
    ci = bootstrap(data, keys, field)
    ci_text = f"[{100 * ci[0]:+.1f} ; {100 * ci[1]:+.1f}]" if ci else "n/a"
    d_text = "n/a" if d is None else f"{100 * d:+.1f}"
    print(f"| {name} | {fmt(a)} | {fmt(b)} | {d_text} | {ci_text} |")
    return d, ci


def header_flag(task_run: Path) -> list:
    """`yaRequest.codexSessionIdHeader` de chaque session principale du run de tâche."""
    flags = []
    for path in sorted((task_run / "sessions").glob("*.jsonl")):
        first = path.read_text().split("\n", 1)[0]
        header = json.loads(first) if first.strip() else {}
        flags.append((header.get("yaRequest") or {}).get("codexSessionIdHeader"))
    return flags


def wire(plan: Path) -> int:
    """Contrôle `--fil` : `session-id` dans les requêtes reçues par le serveur de capture."""
    ab = json.loads(plan.read_text())
    repo = Path(__file__).resolve().parents[3]
    ok = True
    print(f"# En-tête session-id sur le fil (capture) : {plan}\n")
    print("| Bras | Requêtes | avec `session-id` | égal à `session_id` | attendu |")
    print("|---|---|---|---|---|")
    for side, expected in (("A", False), ("B", True)):
        metas = []
        for arm in ab["arms"]:
            if arm["arm"] == side and arm.get("run_dir"):
                metas += sorted((repo / arm["run_dir"]).glob("tasks/*/run-*/capture/*.meta.json"))
        heads = [{k.lower(): v for k, v in (json.loads(m.read_text()).get("headers") or {}).items()}
                 for m in metas]
        sent = [h for h in heads if "session-id" in h]
        equal = [h for h in sent if h["session-id"] == h.get("session_id")]
        good = bool(heads) and (len(equal) == len(heads) if expected else not sent)
        ok &= good
        print(f"| {side} | {len(heads)} | {len(sent)} | {len(equal)} | "
              f"{'toutes' if expected else 'aucune'} ({'ok' if good else 'ÉCART'}) |")
    return 0 if ok else 1


def verdict(d, ci, guard_d, guard_ci) -> str:
    """Règle du plan, dans l'ordre : garde-fou, puis les quatre cas de la métrique principale."""
    if guard_d is not None and guard_ci is not None and guard_d <= -GUARD and guard_ci[1] < 0:
        return ("dégradation (garde-fou) : miss global d ≤ −3 points et borne haute de son IC < 0, "
                "les deux à la fois : le réglage reste coupé, quel que soit le résultat principal")
    if d is None or ci is None:
        return "pas de verdict : un bras n'a aucun appel sous 6 000 tokens"
    if d >= THRESHOLD and ci[0] > 0:
        return "gain net : proposer l'activation par défaut (décision de Yusuf)"
    if ci[0] > 0:
        return ("non concluant : effet probable (borne basse > 0) mais sous le seuil de "
                "10 points ; Yusuf décide")
    if d >= THRESHOLD:
        return "non concluant : d ≥ 10 points mais l'IC contient 0 ; Yusuf décide"
    return (f"pas de gain net détecté : le réglage reste coupé ; gain réel plausible jusqu'à "
            f"{100 * ci[1]:+.1f} points (borne haute)")


def main(argv: list[str]) -> int:
    check_trace = "--sans-trace" not in argv
    args = [a for a in argv if a not in ("--sans-trace", "--fil")]
    if len(args) != 1:
        print(__doc__.splitlines()[3].strip(), file=sys.stderr)
        return 2
    plan = Path(args[0]) / "ab.json"
    if "--fil" in argv:
        return wire(plan)
    ab_stats, cache_causes = load("ab_stats"), load("cache_causes")
    units, ab = ab_stats.units_from_plan(plan, ab_stats.STALL_GAP_S)
    kept = [u for u in units if not u["causes"]]
    print(f"# A/B en-tête session-id : {plan}\n")
    print(f"- A : `{ab['settings']['A']['file']}` ; B : `{ab['settings']['B']['file']}` ; "
          f"binaire `{ab['binary_sha']}`")
    print(f"- Unités : {len(units)}, retenues {len(kept)}")
    for u in units:
        if u["causes"]:
            print(f"  - écartée : passe {u['pass']}, {u['task']} : {'; '.join(u['causes'])}")

    trace_ok = True
    if check_trace:
        print("\n## Trace (`yaRequest.codexSessionIdHeader`)\n")
        for side, expected in (("A", False), ("B", True)):
            flags = [f for u in kept for f in header_flag(
                ab_stats.task_dir(u["arms"][side]["run"], u["arms"][side]["result"]))]
            wrong = [f for f in flags if f != expected]
            trace_ok &= bool(flags) and not wrong
            print(f"- Bras {side} : {len(flags)} sessions, attendu {expected!r}, "
                  f"différentes {len(wrong)}")

    calls = unit_calls(kept, ab_stats, cache_causes)
    keys = sorted(calls)
    small = {k: {s: [c for c in calls[k][s] if c["prev_prompt"] < SPLIT] for s in "AB"}
             for k in keys}
    large = {k: {s: [c for c in calls[k][s] if c["prev_prompt"] >= SPLIT] for s in "AB"}
             for k in keys}

    print("\n## 1. Métrique principale\n")
    print("| Population | A (coupé) | B (en-tête) | d = A − B (points) | IC 95 % |")
    print("|---|---|---|---|---|")
    d, ci = row(f"miss, prompt précédent < {SPLIT}", small, keys, "miss")

    print("\n## 2. Secondaires\n")
    print("| Population | A (coupé) | B (en-tête) | d = A − B (points) | IC 95 % |")
    print("|---|---|---|---|---|")
    guard_d, guard_ci = row("miss, tous les appels (garde-fou)", calls, keys, "miss")
    row("relectures partielles, tous les appels", calls, keys, "partial")
    row(f"miss, prompt précédent ≥ {SPLIT}", large, keys, "miss")
    row(f"relectures partielles, prompt précédent < {SPLIT}", small, keys, "partial")
    row(f"relectures partielles, prompt précédent ≥ {SPLIT}", large, keys, "partial")

    print("\n## 3. Coût rapporté (unités retenues, affiché sans décider)\n")
    cost = {s: sum(ab_stats.cost(u["arms"][s]["result"], "rapporté") for u in kept) for s in "AB"}
    ratio = f"{100 * (cost['B'] / cost['A'] - 1):+.1f} %" if cost["A"] else "n/a"
    print(f"- A {cost['A']:.4f} $, B {cost['B']:.4f} $, B / A − 1 = {ratio} "
          "(IC : `ab_stats.py --plan`)")

    print("\n## Verdict\n")
    if check_trace and not trace_ok:
        print("- suspendu : la trace ne correspond pas aux bras (voir « Trace »)")
        return 1
    print(f"- {verdict(d, ci, guard_d, guard_ci)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
