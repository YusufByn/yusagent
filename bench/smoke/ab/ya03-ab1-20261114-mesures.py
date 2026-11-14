#!/usr/bin/env python3
"""Mesures d'appoint de l'A/B 1 de YA-03 (`editTool`, 14/11/2026), hors ab_stats.py.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya03-ab1-20261114-mesures.py bench/smoke/runs/ab-20261114T120431Z \\
        bench/smoke/baseline/gpt-6.1-terra.json

- coût par tâche résolue par bras (théorique, rapporté, pessimiste), écart B / A − 1 et IC 95 %
  par bootstrap sur les tâches (10 000 tirages, graine 0) ;
- tours, tokens d'input envoyés, input non caché, taux de cache, par bras ;
- ruptures : requêtes en corps complet (`continuation` faux) après la première de chaque
  session, depuis `proxy.ndjson` ;
- échecs d'outils recomptés depuis les sessions, indépendamment de `metrics.py` :
  cellule `python_repl` en erreur (`details.status`), `apply_patch` dont le texte ne commence
  pas par `Exit code: 0`, exception levée par le skill `edit` ;
- contrôle de la règle d'écart du README : `stopReason` `error`, requêtes proxy hors 101/200 ;
- anomalies : bras dont le coût théorique sort de [0,5 ; 2] fois la médiane des 6 bras de sa
  tâche ; chauffes de plus de 3 s ;
- bras `kernel` (B) contre la baseline, aux seuils de `compare.py` (2 × CV de la tâche, au
  moins 5 % ; agrégat : 2 × `aggregate.cv`) ; `compare.py` ne prend qu'un rapport par côté.

Bibliothèque standard, lecture seule."""
import json
import random
import statistics as st
import sys
from collections import Counter, defaultdict
from pathlib import Path

R = Path(sys.argv[1])
BASE = json.loads(Path(sys.argv[2]).read_text())
ab = json.loads((R / "ab.json").read_text())
EDIT_SKILL_FRAME = "skills/edit/src/edit/__init__.py"


def text(m):
    return "".join(p.get("text", "") for p in m.get("content") or [] if isinstance(p, dict))


rows = []
for a in ab["arms"]:
    d = Path(a["run_dir"])
    t = json.loads((d / "report.json").read_text())["totals"]
    run = d / "tasks" / a["task"] / "run-1"
    log = [json.loads(x) for x in (run / "proxy.ndjson").read_text().splitlines() if x.strip()]
    by_session = defaultdict(list)
    for r in log:
        if r.get("event") == "request":
            by_session[r.get("session")].append(r)
    tools = Counter()
    for f in (run / "sessions").glob("*.jsonl"):
        for line in f.read_text().splitlines():
            m = json.loads(line).get("message") or {}
            if m.get("role") != "toolResult":
                continue
            name, det = m.get("toolName"), m.get("details") or {}
            tools[name] += 1
            tools[name + " isError"] += bool(m.get("isError"))
            if name == "python_repl":
                tools["edit diffs"] += len(det.get("diffs") or [])
                if det.get("status") == "error":
                    tools["cell errors"] += 1
                    tb = "".join(str(x) for x in (det.get("error") or {}).get("traceback") or [])
                    tools["edit failures"] += EDIT_SKILL_FRAME in tb
            if name == "apply_patch" and not text(m).startswith("Exit code: 0\n"):
                tools["patch failures"] += 1
    stop_err = 0
    for f in (run / "sessions").rglob("*.usage.ndjson"):
        stop_err += sum(json.loads(x).get("stopReason") == "error"
                        for x in f.read_text().splitlines() if x.strip())
    tok = t["tokens"]
    rows.append(dict(
        p=a["pass"], task=a["task"], arm=a["arm"], ok=a["status"] == ["resolved"],
        th=t["cost_theoretical"], rep=t["cost"], pess=t["cost_pessimistic"],
        turns=t["turns_median"], inp=tok["input"], cr=tok["cacheRead"], cw=tok["cacheWrite"],
        out=tok["output"], requests=sum(len(s) for s in by_session.values()),
        sessions=len(by_session), ws_open=sum(r.get("event") == "ws_open" for r in log),
        full=sum(not r.get("continuation") for s in by_session.values() for r in s[1:]),
        bad_status=sum(r.get("event") == "request" and r.get("status") not in ("101", "200")
                       for r in log),
        stop_err=stop_err, tools=tools, warm=a["timing_s"]["warmup"],
        ttft_warm=None, run=d.name))
    usage = sorted((d / "warmup" / "sessions").glob("*.usage.ndjson"))
    if usage:
        first = json.loads(usage[0].read_text().splitlines()[0])
        rows[-1]["ttft_warm"] = first.get("ttftMs")

print(f"bras : {len(rows)} ; ordres {dict(Counter(u['order'] for u in ab['units']))}")
print("\n## Par bras (totaux sur 30 sessions)")
for arm in "AB":
    r = [x for x in rows if x["arm"] == arm]
    ok = sum(x["ok"] for x in r)
    th, rep, pess = (sum(x[k] for x in r) for k in ("th", "rep", "pess"))
    inp, cr, cw = (sum(x[k] for x in r) for k in ("inp", "cr", "cw"))
    tools = sum((x["tools"] for x in r), Counter())
    print(f"{arm} : résolues {ok}/{len(r)} ; coût par résolue th {th / ok:.5f} $, "
          f"rapporté {rep / ok:.5f} $, pessimiste {pess / ok:.5f} $")
    print(f"   tours moyens {st.mean(x['turns'] for x in r):.2f} ; envoyés (input + cache lu "
          f"+ écrit) {inp + cr + cw} ; input non caché {inp} ; taux de cache "
          f"{cr / (inp + cr + cw):.4f}")
    print(f"   requêtes {sum(x['requests'] for x in r)}, sessions "
          f"{sum(x['sessions'] for x in r)}, ws_open {sum(x['ws_open'] for x in r)}, corps "
          f"complets après la première {sum(x['full'] for x in r)}")
    print(f"   écart README : stopReason error {sum(x['stop_err'] for x in r)}, requêtes hors "
          f"101/200 {sum(x['bad_status'] for x in r)}")
    print(f"   outils : {dict(sorted(tools.items()))}")

print("\n## Coût par tâche résolue, B / A − 1, IC 95 % (bootstrap sur les tâches)")
tasks = sorted({x["task"] for x in rows})


def ratio(ts, key):
    per = {}
    for arm in "AB":
        sel = [x for t in ts for x in rows if x["task"] == t and x["arm"] == arm]
        ok = sum(x["ok"] for x in sel)
        per[arm] = sum(x[key] for x in sel) / ok if ok else None
    return None if None in per.values() else per["B"] / per["A"] - 1


for key in ("th", "rep", "pess"):
    rng = random.Random(0)
    draws = sorted(v for v in (ratio([rng.choice(tasks) for _ in tasks], key)
                               for _ in range(10000)) if v is not None)
    lo, hi = draws[int(0.025 * len(draws))], draws[int(0.975 * len(draws)) - 1]
    print(f"{key} : {ratio(tasks, key) * 100:+.1f} % [{lo * 100:+.1f} ; {hi * 100:+.1f}] "
          f"({len(draws)} tirages valides)")

print("\n## Anomalies")
for t in tasks:
    r = [x for x in rows if x["task"] == t]
    med = st.median(x["th"] for x in r)
    top = max(r, key=lambda x: x["th"] / med)
    flag = [x for x in r if not 0.5 <= x["th"] / med <= 2]
    print(f"{t} : médiane th {med:.4f} $, max {top['th'] / med:.2f}x (passe {top['p']}, "
          f"bras {top['arm']}){' ; HORS BORNES ' + str([x['run'] for x in flag]) if flag else ''}")
print(f"chauffe : médiane {st.median(x['warm'] for x in rows):.2f} s")
for x in rows:
    if x["warm"] > 3:
        print(f"  passe {x['p']} {x['task']} bras {x['arm']} : {x['warm']} s, "
              f"TTFT {x['ttft_warm']} ms ({x['run']})")

print("\n## Bras B (kernel) contre la baseline, seuils de compare.py")
agg_base = agg_b = 0.0
for t, e in BASE["per_task"].items():
    r = [x for x in rows if x["task"] == t and x["arm"] == "B"]
    trees = set()
    for x in r:
        rep = json.loads((R / "arms" / x["run"] / "report.json").read_text())
        trees |= {(res.get("task_version") or {}).get("tree") for res in rep["results"]}
    cost = st.mean(x["rep"] for x in r)
    tok = st.mean(x["inp"] + x["cr"] + x["cw"] + x["out"] for x in r)
    dc, lc = (cost / e["cost"]["mean"] - 1) * 100, max(2 * e["cost"]["cv"] * 100, 5)
    dt, lt = (tok / e["tokens"]["mean"] - 1) * 100, max(2 * e["tokens"]["cv"] * 100, 5)
    ok = sum(x["ok"] for x in r)
    alerts = [n for n, hit in (("stable non résolue", e["stable"] and ok * 2 < len(r)),
                               ("coût", e["stable"] and abs(dc) > lc),
                               ("tokens", e["stable"] and abs(dt) > lt)) if hit]
    print(f"{t} : version {'identique' if trees == {e['task_tree']} else 'DIFFÉRENTE'}, "
          f"stable {e['stable']}, résolue {ok}/3 (base {e['resolved_rate']:.2f}) ; coût "
          f"{e['cost']['mean']:.4f} → {cost:.4f} $ ({dc:+.1f} %, seuil ±{lc:.1f}) ; tokens "
          f"{e['tokens']['mean']:.0f} → {tok:.0f} ({dt:+.1f} %, seuil ±{lt:.1f}) ; tours "
          f"{e['turns_mean']:.1f} → {st.mean(x['turns'] for x in r):.1f} ; alertes {alerts}")
    if e["stable"] and trees == {e["task_tree"]}:
        agg_base += e["cost"]["mean"]
        agg_b += cost
limit = max(2 * BASE["aggregate"]["cv"] * 100, 5)
print(f"agrégé des tâches stables (rapporté) : {agg_base:.4f} → {agg_b:.4f} $ "
      f"({(agg_b / agg_base - 1) * 100:+.1f} %, seuil ±{limit:.1f} %)")
