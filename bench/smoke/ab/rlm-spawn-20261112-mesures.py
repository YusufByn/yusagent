#!/usr/bin/env python3
"""Mesures d'appoint de l'A/B rlm.spawn du 12/11/2026 (YA-06), hors ab_stats.py.

Usage : python3 -I bench/smoke/ab/rlm-spawn-20261112-mesures.py bench/smoke/runs/ab-20261112T185001Z

Durée de la phase agent (`seconds` de tasks.csv, sans le surcoût de lancement),
sessions et appels de sous-agents, mentions de `rlm.spawn(` et du refus de
profondeur dans les fichiers de la tâche, et contrôle indépendant de la règle
d'écart du README (statuts, `stopReason` `error`, requêtes proxy hors 101/200).
Bibliothèque standard, lecture seule."""
import csv, json, sys, re, statistics as st
from collections import defaultdict
from pathlib import Path
R = Path(sys.argv[1]); ab = json.loads((R / 'ab.json').read_text())
rows = []
for a in ab['arms']:
    d = Path(a['run_dir'])
    t = next(csv.DictReader(open(d / 'tasks.csv')))
    run = d / 'tasks' / a['task'] / 'run-1'
    spawn = 0; stop_err = 0; depth = 0
    for p in run.rglob('*'):
        if p.is_file() and p.suffix in ('.jsonl', '.ndjson', '.json'):
            s = p.read_text(errors='replace')
            if p.name.endswith('.jsonl') or 'session' in str(p):
                spawn += s.count('rlm.spawn(')
            depth += s.count('RLM recursion depth limit')
            if p.name.endswith('.usage.ndjson'):
                for line in s.splitlines():
                    try:
                        if json.loads(line).get('stopReason') == 'error': stop_err += 1
                    except Exception: pass
    bad_proxy = 0
    for px in run.rglob('proxy.ndjson'):
        for line in px.read_text().splitlines():
            try: e = json.loads(line)
            except Exception: continue
            st_ = e.get('status')
            if e.get('event') == 'request' and st_ not in ('101', '200'): bad_proxy += 1
    rows.append(dict(p=a['pass'], task=a['task'], arm=a['arm'], status=t['status'], sec=float(t['seconds']),
                     wall=a['wall_s'], sub=int(t['subagent_sessions'] or 0), subcalls=int(t['subagent_calls'] or 0),
                     turns=int(t['turns']), miss=int(t['server_misses'] or 0), spawn=spawn, depth=depth,
                     stop_err=stop_err, bad_proxy=bad_proxy, exit=a['exit']))
print('bras', len(rows), 'unités', len({(r['p'], r['task']) for r in rows}))
print('statuts', sorted({r['status'] for r in rows}), 'exit≠0', sum(r['exit'] != 0 for r in rows))
print('stopReason error', sum(r['stop_err'] for r in rows), 'proxy ≠101/200', sum(r['bad_proxy'] for r in rows))
for arm in 'AB':
    rr = [r for r in rows if r['arm'] == arm]
    print(f"bras {arm}: durée tâches {sum(r['sec'] for r in rr):.1f} s (par passe {sum(r['sec'] for r in rr)/3:.1f}), wall {sum(r['wall'] for r in rr):.1f} s ; "
          f"sessions sous-agents {sum(r['sub'] for r in rr)} ; appels sous-agents {sum(r['subcalls'] for r in rr)} ; "
          f"unités avec sous-agents {sum(r['sub']>0 for r in rr)}/30 ; mentions de rlm.spawn( dans les fichiers de la tâche {sum(r['spawn'] for r in rr)} ; "
          f"limite de profondeur {sum(r['depth'] for r in rr)} ; miss {sum(r['miss'] for r in rr)}")
print('\n| Tâche | Durée A s | Durée B s | Écart % | Sous-agents A | Sous-agents B |')
by = defaultdict(list)
for r in rows: by[(r['task'], r['arm'])].append(r)
for task in ab['tasks']:
    a, b = by[(task, 'A')], by[(task, 'B')]
    ma, mb = st.mean(x['sec'] for x in a), st.mean(x['sec'] for x in b)
    print(f"| {task} | {ma:.1f} | {mb:.1f} | {100*(mb/ma-1):+.1f} | {sum(x['sub'] for x in a)} | {sum(x['sub'] for x in b)} |")
print('\nnon résolues :', [(r['p'], r['task'], r['arm'], r['status']) for r in rows if r['status'] != 'resolved'])
