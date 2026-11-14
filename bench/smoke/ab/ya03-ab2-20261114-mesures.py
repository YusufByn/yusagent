#!/usr/bin/env python3
"""Mesures d'appoint de l'A/B 2 de YA-03 (`shellTool`, 14/11/2026), hors ab_stats.py.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya03-ab2-20261114-mesures.py bench/smoke/runs/ab-20261114T140538Z \\
        bench/smoke/runs/ab-20261114T120431Z <baseline>

`<baseline>` ne sert qu'à la liste des tâches stables (`aggregate.tasks`) : la baseline d'avant
l'A/B 2 (`git show 67c05e592:bench/smoke/baseline/gpt-6.1-terra.json`) et celle écrite depuis ses
bras A donnent les mêmes 8 tâches.

- coût par tâche résolue par bras (théorique, rapporté, pessimiste), écart B / A − 1 et IC 95 %
  par bootstrap sur les tâches (10 000 tirages, graine 0) ;
- usage du shell : `bash()` dans le kernel (bras A, `details.bashCommands` et
  `details.bashFailedExit` de `python_repl`, une valeur par cellule) ; `exec_command` et
  `write_stdin` (bras B), classés par l'en-tête de Codex de leur texte (sortie 0, sortie non
  nulle, processus encore en cours, autre texte = erreur du tool) ; `yield_time_ms` et
  `max_output_tokens` demandés ; heredocs et boucles shell dans les commandes de B ;
- échecs d'outils recomptés depuis les sessions, indépendamment de `metrics.py` : cellule
  `python_repl` en erreur (`details.status`), `apply_patch` dont le texte ne commence pas par
  `Exit code: 0`, exception levée par le skill `edit` ; sorties tronquées ;
- tours, tokens envoyés, input non caché, taux de cache, miss serveur, durée des tâches ;
- ruptures : requêtes en corps complet (`continuation` faux) après la première de chaque
  session, depuis `proxy.ndjson` ; contrôle de la règle d'écart du README (`stopReason`
  `error`, requêtes proxy hors 101/200) ;
- reproductibilité : bras A de l'A/B 2 contre bras A de l'A/B 1 (même réglage, même binaire),
  par tâche, et A2 / A1 − 1 agrégé avec IC (même bootstrap) ;
- anomalies : bras dont le coût théorique sort de [0,5 ; 2] fois la médiane des 6 bras de sa
  tâche ; détail de trois bras ; tests en échec (`grading/junit.xml`) des deux tâches non
  stables dans les deux A/B, et réécriture par l'agent du test existant qui échoue.

Bibliothèque standard, lecture seule."""
import json
import random
import re
import statistics as st
import sys
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from pathlib import Path

R2, R1 = Path(sys.argv[1]), Path(sys.argv[2])
STABLE = set(json.loads(Path(sys.argv[3]).read_text())["aggregate"]["tasks"])
EDIT_SKILL_FRAME = "skills/edit/src/edit/__init__.py"
EXIT = re.compile(r"^Wall time: [\d.]+ seconds\nProcess exited with code (-?\d+)\n")
RUNNING = re.compile(r"^Wall time: [\d.]+ seconds\nProcess running with session ID \d+\n")
TRUNCATED = "output truncated"
SHELL = ("exec_command", "write_stdin")


def text(m):
    return "".join(p.get("text", "") for p in m.get("content") or [] if isinstance(p, dict))


def arm_dir(R, a):
    return R / "arms" / Path(a["run_dir"]).name


def load(R):
    ab = json.loads((R / "ab.json").read_text())
    rows = []
    for a in ab["arms"]:
        d = arm_dir(R, a)
        rep = json.loads((d / "report.json").read_text())
        t = rep["totals"]
        run = d / "tasks" / a["task"] / "run-1"
        log = [json.loads(x) for x in (run / "proxy.ndjson").read_text().splitlines() if x.strip()]
        by_s = defaultdict(list)
        for r in log:
            if r.get("event") == "request":
                by_s[r.get("session")].append(r)
        c, calls_text = Counter(), []
        for f in (run / "sessions").glob("*.jsonl"):
            args = {}
            for line in f.read_text().splitlines():
                m = json.loads(line).get("message") or {}
                if m.get("role") == "assistant":
                    for p in m.get("content") or []:
                        if p.get("type") != "toolCall":
                            continue
                        x = p.get("arguments") or {}
                        args[p.get("id")] = x
                        calls_text.append(x.get("input") or x.get("code") or x.get("cmd") or "")
                        if p["name"] in SHELL:
                            c[f"{p['name']} yield_time_ms={x.get('yield_time_ms')}"] += 1
                        if p["name"] == "exec_command":
                            cmd = x.get("cmd", "")
                            c["exec heredoc"] += "<<" in cmd
                            c["exec boucle"] += bool(re.search(r"\b(for|while|until)\b.*\bdo\b", cmd, re.S))
                            c["exec enchaînée (&&, ;, |)"] += any(s in cmd for s in ("&&", ";", "|"))
                if m.get("role") != "toolResult":
                    continue
                name, det, tx = m.get("toolName"), m.get("details") or {}, text(m)
                c[name] += 1
                c[name + " tronqué"] += TRUNCATED in tx
                if name == "python_repl":
                    b = det.get("bashCommands")
                    if b:
                        c["bash() commandes"] += b.get("count", 0)
                        c["cellules avec bash()"] += 1
                    c["cellules bash() sortie non nulle"] += "bashFailedExit" in det
                    c["cellules bash() encore en cours"] += "still running after" in (det.get("stderr") or "")
                    c["edit diffs"] += len(det.get("diffs") or [])
                    if det.get("status") == "error":
                        e = det.get("error") or {}
                        c["cellules en erreur"] += 1
                        c[f"cellules en erreur {e.get('ename')}"] += 1
                        tb = "".join(str(x) for x in e.get("traceback") or [])
                        c["échecs edit()"] += EDIT_SKILL_FRAME in tb
                if name in SHELL:
                    mm = EXIT.match(tx)
                    if mm:
                        c[f"{name} sortie {'0' if mm.group(1) == '0' else 'non nulle'}"] += 1
                    elif RUNNING.match(tx):
                        c[f"{name} encore en cours"] += 1
                    else:
                        c[f"{name} erreur du tool"] += 1
                    if TRUNCATED in tx:
                        c[f"tronqué, max_output_tokens={args.get(m.get('toolCallId'), {}).get('max_output_tokens')}"] += 1
                    body = tx.split("Output:\n", 1)[-1]
                    c["sortie shell octets"] += len(body.encode())
                    c["sortie shell caractères"] += len(body)
                if name == "apply_patch" and not tx.startswith("Exit code: 0\n"):
                    c["échecs apply_patch"] += 1
        stop_err = sum(json.loads(x).get("stopReason") == "error"
                       for f in (run / "sessions").rglob("*.usage.ndjson")
                       for x in f.read_text().splitlines() if x.strip())
        tok = t["tokens"]
        rows.append(dict(
            p=a["pass"], task=a["task"], arm=a["arm"], ok=a["status"] == ["resolved"],
            th=t["cost_theoretical"], rep=t["cost"], pess=t["cost_pessimistic"],
            turns=t["turns_median"], inp=tok["input"], cr=tok["cacheRead"], cw=tok["cacheWrite"],
            req=sum(len(s) for s in by_s.values()), sess=len(by_s),
            full=sum(not r.get("continuation") for s in by_s.values() for r in s[1:]),
            bad=sum(r.get("event") == "request" and r.get("status") not in ("101", "200") for r in log),
            stop_err=stop_err, misses=sum(len(r.get("server_misses", [])) for r in rep["results"]),
            c=c, calls="\n".join(calls_text), dur=a["timing_s"]["tasks"], run=d,
            junit=run / "grading" / "junit.xml"))
    return ab, rows


ab2, rows = load(R2)
ab1, rows1 = load(R1)
tasks = sorted({x["task"] for x in rows})


def boot(fn, ts, n=10000):
    rng = random.Random(0)
    v = sorted(x for x in (fn([rng.choice(ts) for _ in ts]) for _ in range(n)) if x is not None)
    return fn(ts), v[int(0.025 * len(v))], v[int(0.975 * len(v)) - 1], len(v)


def per_resolved(key):
    def f(ts):
        per = {}
        for arm in "AB":
            sel = [x for t in ts for x in rows if x["task"] == t and x["arm"] == arm]
            ok = sum(x["ok"] for x in sel)
            per[arm] = sum(x[key] for x in sel) / ok if ok else None
        return None if None in per.values() else per["B"] / per["A"] - 1
    return f


print(f"bras : {len(rows)} ; ordres {dict(Counter(u['order'] for u in ab2['units']))} ; réglages "
      f"A {ab2['settings']['A']['value']} B {ab2['settings']['B']['value']} ; binaire {ab2['binary_sha']}")

print("\n## Résolution et coût par tâche résolue")
for arm in "AB":
    r = [x for x in rows if x["arm"] == arm]
    ok = sum(x["ok"] for x in r)
    print(f"{arm} : résolues {ok}/{len(r)}, par passe {[sum(x['ok'] for x in r if x['p'] == p) for p in (1, 2, 3)]} ; "
          + " ; ".join(f"{k} {sum(x[k] for x in r) / ok:.5f} $" for k in ("th", "rep", "pess")))
for k in ("th", "rep", "pess"):
    v, lo, hi, n = boot(per_resolved(k), tasks)
    print(f"B / A − 1, {k} : {v * 100:+.1f} % [{lo * 100:+.1f} ; {hi * 100:+.1f}] ({n} tirages valides)")

print("\n## Par bras (totaux sur 30 sessions)")
for arm in "AB":
    r = [x for x in rows if x["arm"] == arm]
    c = sum((x["c"] for x in r), Counter())
    inp, cr, cw = (sum(x[k] for x in r) for k in ("inp", "cr", "cw"))
    print(f"{arm} : tours moyens {st.mean(x['turns'] for x in r):.2f} ; envoyés (input + cache lu + écrit) "
          f"{inp + cr + cw} ; input non caché {inp} ; taux de cache {cr / (inp + cr + cw):.4f} ; miss serveur "
          f"{sum(x['misses'] for x in r)} ; durée des tâches par passe {sum(x['dur'] for x in r) / 3:.1f} s")
    print(f"   requêtes {sum(x['req'] for x in r)}, sessions {sum(x['sess'] for x in r)}, corps complets après la "
          f"première {sum(x['full'] for x in r)} ; écart README : stopReason error "
          f"{sum(x['stop_err'] for x in r)}, requêtes hors 101/200 {sum(x['bad'] for x in r)}")
    print(f"   sessions qui utilisent le shell : "
          f"{sum(1 for x in r if x['c']['exec_command'] or x['c']['bash() commandes'])}/{len(r)}")
    for k, v in sorted(c.items()):
        if v:
            print(f"   {k} : {v}")

print("\n## Reproductibilité : bras A de l'A/B 2 contre bras A de l'A/B 1")
print(f"réglages A1 {ab1['settings']['A']['value']} A2 {ab2['settings']['A']['value']} ; binaires "
      f"{ab1['binary_sha'][:9]} {ab2['binary_sha'][:9]} ; commits du banc {ab1['commit'][:9]} {ab2['commit'][:9]}")
a1 = {(x["p"], x["task"]): x for x in rows1 if x["arm"] == "A"}
a2 = {(x["p"], x["task"]): x for x in rows if x["arm"] == "A"}
for t in tasks:
    x1, x2 = [a1[(p, t)] for p in (1, 2, 3)], [a2[(p, t)] for p in (1, 2, 3)]
    m1, m2 = st.mean(x["th"] for x in x1), st.mean(x["th"] for x in x2)
    print(f"{t} ({'stable' if t in STABLE else 'non stable'}) : résolue {sum(x['ok'] for x in x1)}/3 → "
          f"{sum(x['ok'] for x in x2)}/3 ; th {m1:.4f} → {m2:.4f} $ ({(m2 / m1 - 1) * 100:+.1f} %) ; rapporté "
          f"{st.mean(x['rep'] for x in x1):.4f} → {st.mean(x['rep'] for x in x2):.4f} $ ; tours "
          f"{st.mean(x['turns'] for x in x1):.1f} → {st.mean(x['turns'] for x in x2):.1f}")


def rerun(key):
    def f(ts):
        s1 = sum(a1[(p, t)][key] for t in ts for p in (1, 2, 3))
        return sum(a2[(p, t)][key] for t in ts for p in (1, 2, 3)) / s1 - 1
    return f


st_tasks = sorted(STABLE & set(tasks))
for lab, ts in (("stables", st_tasks), ("10 tâches", tasks)):
    for k in ("th", "rep"):
        v, lo, hi, _ = boot(rerun(k), ts)
        print(f"A2 / A1 − 1, {lab}, {k} : {v * 100:+.1f} % [{lo * 100:+.1f} ; {hi * 100:+.1f}]")
d = [abs(st.mean(a2[(p, t)]["th"] for p in (1, 2, 3)) / st.mean(a1[(p, t)]["th"] for p in (1, 2, 3)) - 1)
     for t in st_tasks]
print(f"écart absolu par tâche stable (th) : médiane {st.median(d) * 100:.1f} %, max {max(d) * 100:.1f} %")

print("\n## Anomalies : bras hors [0,5 ; 2] fois la médiane des 6 bras de la tâche (th)")
for t in tasks:
    r = [x for x in rows if x["task"] == t]
    med = st.median(x["th"] for x in r)
    top = max(r, key=lambda x: x["th"] / med)
    flag = [(x["p"], x["arm"], round(x["th"] / med, 2)) for x in r if not 0.5 <= x["th"] / med <= 2]
    print(f"{t} : médiane {med:.4f} $, max {top['th'] / med:.2f}x (passe {top['p']}, bras {top['arm']})"
          f"{' ; HORS BORNES ' + str(flag) if flag else ''}")
for p, t, arm in ((2, "node-testsprite-356", "B"), (2, "node-filesize-376", "A"), (1, "node-little-coder-102", "B")):
    x = next(x for x in rows if (x["p"], x["task"], x["arm"]) == (p, t, arm))
    print(f"{t}, passe {p}, bras {arm} ({x['run'].name}) : {'résolue' if x['ok'] else 'échec'}, th {x['th']:.4f} $, "
          f"tours {x['turns']}, durée {x['dur']} s ; {dict(sorted((k, v) for k, v in x['c'].items() if v))}")
for p in (1, 2, 3):
    x = next(x for x in rows if (x["p"], x["task"], x["arm"]) == (p, "node-testsprite-356", "B"))
    print(f"node-testsprite-356, passe {p}, bras B : suite complète `npm test` lancée : "
          f"{bool(re.search(r'(^|&& )npm test\b', x['calls'], re.M))}")

print("\n## Tâches non stables : tests en échec et test existant réécrit par l'agent")
REWRITE = {
    # assertion visible du dépôt, qui passait avant le correctif (pass_to_pass)
    "node-filesize-376": lambda s: bool(re.search(
        r'-\s*assert\.strictEqual\(filesize\(0, \{ locale: "de-DE", precision: 3 \}\), "0\.00 B"\)', s))
        or ('"0,00 B")' in s and 'de-DE", precision: 3 }' in s),
    # extraction du JSON nu, que le test existant `nested-object bare JSON` interdit
    "node-little-coder-102": lambda s: "bareRe" in s,
}
for lab, R, rs in (("A/B 1", R1, rows1), ("A/B 2", R2, rows)):
    for x in sorted((x for x in rs if x["task"] in REWRITE), key=lambda x: (x["task"], x["p"], x["arm"])):
        fails = [tc.get("name") for tc in ET.parse(x["junit"]).getroot().iter("testcase")
                 if tc.find("failure") is not None or tc.find("error") is not None]
        print(f"{lab} {x['task']} passe {x['p']} bras {x['arm']} : {'résolue' if x['ok'] else 'échec'} ; "
              f"tests en échec {fails} ; réécrit le comportement existant : {REWRITE[x['task']](x['calls'])}")
