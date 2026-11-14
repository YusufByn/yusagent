#!/usr/bin/env python3
"""Mesures d'appoint de la comparaison YA-14, yusAgent contre Codex CLI (14/11/2026), hors ab_stats.py.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/ab/ya14-codex-20261114-mesures.py bench/smoke/runs/ab-20261114T201248Z

A = yusAgent (binaire 80e20ea, `settings/default.json`), B = Codex CLI 0.169.0.

- complétude : statuts, sorties de `run.py`, `stopReason` `error`, requêtes proxy hors 101/200,
  chaînes d'erreur de quota (`usage_limit_reached`, `rate_limit_exceeded`, `429`) ;
- quota : relevés `codex.rate_limits` des rapports du bras B, dans l'ordre des lancements ;
- durée de la phase agent, sur les tâches de la population de coût (résolues à toutes les passes
  par les deux bras) : appels, temps modèle (requête → `response.completed` au reverse proxy),
  temps entre deux appels, exécution des tools (A : `details.durationMs` de chaque résultat ;
  B : `Wall time` de chaque sortie d'`exec`), bords (phase agent − premier appel → dernière fin),
  tokens de sortie, TTFT de A (`ttftMs` du journal d'usage) ;
- latence par appel : moindres carrés du temps modèle sur les tokens de sortie, par bras, appels
  avec et sans tokens de raisonnement ;
- `bash()` du kernel (A) : cellules revenues avant la fin de la commande (`still running after`
  dans le stderr de la cellule, ou résultat `BashSnapshot`), tours de relève (cellule qui ne fait
  que lire un handle ou un instantané), notifications `async_bash_completion` ; `yield_time_ms`
  des `exec_command` imbriqués dans `exec` (B) ;
- classification des appels de tools (heuristique, mêmes motifs pour les deux bras) : lecture et
  exploration, edit, test et vérification, relève ;
- échecs de `py-graphiti-2030` et `node-testsprite-356` : tests en échec (`grading/junit.xml`) et
  repères dans les appels de l'agent, avec le tour (rang de l'appel au modèle dans la session).

Bibliothèque standard, lecture seule."""
import json
import re
import statistics as st
import sys
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict
from pathlib import Path

R = Path(sys.argv[1])
AB = json.loads((R / "ab.json").read_text())
WALL = re.compile(r"Wall time ([0-9.]+) seconds")
QUOTA = ("usage_limit_reached", "rate_limit_exceeded")
POLL = re.compile(r"\.handle\.(output|poll|wait)\(|\.running\b|\.poll\(\)|^print\(''\)$|^print\('started'\)$|__dict__")
EDIT = re.compile(r"apply_patch|write_text|\.write\(|open\([^)]*['\"]w")
TEST = re.compile(r"pytest|npm (run )?test|vitest|node --test|jest|mocha|typecheck|ruff|prettier|eslint|\btsc\b|uv run|node -e|python3? -c")


def jl(path):
    return [json.loads(x) for x in Path(path).read_text().splitlines() if x.strip()]


def text(m):
    return "".join(p.get("text", "") for p in m.get("content") or [] if isinstance(p, dict))


def classify(code):
    c = code.strip()
    if POLL.search(c) and "bash(" not in c and "exec_command" not in c:
        return "relève"
    if EDIT.search(c):
        return "edit"
    if TEST.search(c):
        return "test/vérif"
    return "lecture/explo"


def load():
    rows = []
    for a in AB["arms"]:
        d = R / "arms" / Path(a["run_dir"]).name
        rep = json.loads((d / "report.json").read_text())
        res = rep["results"][0]
        run = d / "tasks" / a["task"] / "run-1"
        log = jl(run / "proxy.ndjson")
        reqs = [r for r in log if r.get("event") == "request"]
        resps = [r for r in log if r.get("event") == "response"]
        gen = [(q, s) for q, s in zip(reqs, resps) if q.get("generate") is not False]
        per_call = [((s["ts"] - q["ts"]) / 1000, s["usage"].get("output_tokens", 0),
                     s["usage"].get("reasoning_tokens", 0)) for q, s in gen]
        x = dict(p=a["pass"], task=a["task"], arm=a["arm"], ok=res["status"] == "resolved",
                 status=res["status"], exit=a["exit"], run=d, junit=run / "grading" / "junit.xml",
                 agent=res["agent"]["seconds"], timeout=res["agent"]["timeout"], stalled=res["agent"]["stalled"],
                 calls=len(gen), model=sum(c[0] for c in per_call),
                 gaps=sum(gen[i + 1][0]["ts"] - gen[i][1]["ts"] for i in range(len(gen) - 1)) / 1000,
                 span=(gen[-1][1]["ts"] - gen[0][0]["ts"]) / 1000 if gen else 0.0,
                 out=sum(c[1] for c in per_call), per_call=per_call,
                 bad=sum(r.get("status") not in ("101", "200") for r in reqs),
                 quota_hits=sum(any(q in f.read_text(errors="ignore") for q in QUOTA) or '"status": "429"' in f.read_text(errors="ignore")
                                for f in (run / "out.jsonl", run / "err.log", run / "proxy.ndjson") if f.exists()),
                 rate=(res.get("codex") or {}).get("rate_limits"), tool=0.0, tools=0, ttft=[], stop_err=0,
                 snap=0, poll=0, notif=0, kinds=Counter(), marks=[], calls_text=[])
        if a["arm"] == "A":
            sess = [f for f in (run / "sessions").glob("*.jsonl")]
            for f in sess:
                turn = 0
                for e in jl(f):
                    m = e.get("message") or {}
                    if e.get("type") == "custom_message" and e.get("customType") == "async_bash_completion":
                        x["notif"] += 1
                    if e.get("type") != "message":
                        continue
                    if m.get("role") == "assistant":
                        turn += 1
                        for c in m.get("content") or []:
                            if c.get("type") != "toolCall":
                                continue
                            args = c.get("arguments") or {}
                            code = args.get("code") or args.get("input") or ""
                            kind = "edit" if c.get("name") == "apply_patch" else classify(code)
                            x["kinds"][kind] += 1
                            x["poll"] += kind == "relève"
                            x["calls_text"].append((turn, code))
                    elif m.get("role") == "toolResult":
                        det = m.get("details") or {}
                        x["tools"] += 1
                        x["tool"] += (det.get("durationMs") or 0) / 1000
                        tx = text(m)
                        x["snap"] += ("still running after" in (det.get("stderr") or "")
                                      or "still running after" in tx or "BashSnapshot running" in tx)
            for f in (run / "sessions").glob("*.usage.ndjson"):
                for u in jl(f):
                    x["stop_err"] += u.get("stopReason") == "error"
                    if u.get("kind") == "main" and u.get("ttftMs") is not None:
                        x["ttft"].append(u["ttftMs"] / 1000)
        else:
            for f in (run / "sessions").glob("rollout-*.jsonl"):
                turn = 0
                for e in jl(f):
                    p = e.get("payload")
                    if not isinstance(p, dict):
                        continue
                    if e.get("type") == "event_msg" and p.get("type") == "token_count":
                        turn += 1
                    if e.get("type") != "response_item":
                        continue
                    if p.get("type") in ("custom_tool_call", "function_call"):
                        code = p.get("input") or p.get("arguments") or ""
                        kind = "relève" if "write_stdin" in code and "exec_command" not in code else classify(code)
                        x["kinds"][kind] += 1
                        x["calls_text"].append((turn + 1, code))
                    elif p.get("type") in ("custom_tool_call_output", "function_call_output"):
                        x["tools"] += 1
                        mm = WALL.search(str(p.get("output")))
                        x["tool"] += float(mm.group(1)) if mm else 0.0
        x["edge"] = x["agent"] - x["span"]
        rows.append(x)
    return rows


rows = load()
tasks = sorted({x["task"] for x in rows})
cell = {(x["p"], x["task"], x["arm"]): x for x in rows}
common = [t for t in tasks if all(cell[(p, t, a)]["ok"] for p in (1, 2, 3) for a in "AB")]

print(f"bras : {len(rows)} ; unités : {len(AB['units'])} ; ordres {dict(Counter(u['order'] for u in AB['units']))} ; "
      f"graine {AB['seed']} ; binaire A {AB['binary_sha']} ; commit du banc {AB['commit']} (sale : {AB['commit_dirty']})")

print("\n## Complétude")
print(f"statuts {dict(Counter(x['status'] for x in rows))} ; sorties de run.py non nulles {sum(x['exit'] != 0 for x in rows)} ; "
      f"timeouts {sum(x['timeout'] for x in rows)} ; stalled {sum(x['stalled'] for x in rows)} ; "
      f"stopReason error (A) {sum(x['stop_err'] for x in rows)} ; requêtes proxy hors 101/200 {sum(x['bad'] for x in rows)} ; "
      f"fichiers avec erreur de quota {sum(x['quota_hits'] for x in rows)}")

print("\n## Quota (relevés de Codex, bras B, ordre des lancements : 5 h / semaine, %)")
b = [x for x in rows if x["arm"] == "B"]
print(" ".join(f"{x['run'].name[9:15]}:{x['rate']['primary_used_percent']:g}/{x['rate']['secondary_used_percent']:g}" for x in b))
print(f"limit_reached : {sorted({str(x['rate'].get('limit_reached')) for x in b})}")

print(f"\n## Durée de la phase agent, tâches communes ({len(common)} : {', '.join(common)}), moyenne par lancement")
for arm in "AB":
    r = [x for x in rows if x["task"] in common and x["arm"] == arm]
    n = len(r)
    m = lambda k: sum(x[k] for x in r) / n  # noqa: E731
    calls = [c for x in r for c in x["per_call"]]
    ttft = [t for x in r for t in x["ttft"]]
    print(f"{arm} ({n} lancements) : phase agent {m('agent'):.1f} s ; appels {m('calls'):.1f} ; temps modèle {m('model'):.1f} s ; "
          f"entre appels {m('gaps'):.1f} s, dont tools {m('tool'):.1f} s ; bords {m('edge'):.1f} s ; tokens de sortie {m('out'):.0f} ; "
          f"temps modèle par appel : moyenne {m('model') / m('calls'):.2f} s, médiane {st.median(c[0] for c in calls):.2f} s ; "
          + (f"TTFT médian {st.median(ttft):.2f} s" if ttft else "TTFT non mesuré"))


def ols(pts):
    xs, ys = [p[1] for p in pts], [p[0] for p in pts]
    mx, my = st.mean(xs), st.mean(ys)
    slope = sum((a - mx) * (b - my) for a, b in zip(xs, ys)) / sum((a - mx) ** 2 for a in xs)
    return my - slope * mx, slope


print("\n## Latence par appel, tâches communes (temps modèle = fixe + pente × tokens de sortie, moindres carrés)")
for arm in "AB":
    pts = [c for x in rows if x["task"] in common and x["arm"] == arm for c in x["per_call"]]
    for lab, sel in (("tous", pts), ("avec raisonnement", [c for c in pts if c[2] > 0]),
                     ("sans raisonnement", [c for c in pts if c[2] == 0])):
        a0, s = ols(sel)
        print(f"{arm}, {lab} : {len(sel)} appels ; {a0:.2f} s + {s * 1000:.1f} ms/token ; médiane {st.median(c[0] for c in sel):.2f} s, "
              f"sortie médiane {st.median(c[1] for c in sel):g} tokens")

print("\n## bash() du kernel (A) : retours avant la fin de la commande, tours de relève, notifications")
for lab, sel in (("tâches communes", common), ("toutes", tasks)):
    r = [x for x in rows if x["arm"] == "A" and x["task"] in sel]
    print(f"{lab} ({len(r)} lancements) : retours avant la fin {sum(x['snap'] for x in r)} ; tours de relève {sum(x['poll'] for x in r)} ; "
          f"notifications {sum(x['notif'] for x in r)} ; tours moyens {st.mean(x['calls'] for x in r):.1f}")
for t in ("node-testsprite-356", "py-graphiti-2030"):
    for p in (1, 2, 3):
        x = cell[(p, t, "A")]
        print(f"   {t} passe {p} : {x['status']}, {x['calls']} appels, {x['agent']:.0f} s ; retours avant la fin {x['snap']}, "
              f"relève {x['poll']}, notifications {x['notif']}")

yields, nested = Counter(), 0
for x in rows:
    if x["arm"] == "B":
        for _, code in x["calls_text"]:
            nested += len(re.findall(r"exec_command\(", code))
            yields.update(int(v) for v in re.findall(r"exec_command\(\{[^}]*?yield_time_ms:\s*(\d+)", code))
print(f"B : exec_command imbriqués dans exec {nested} ; yield_time_ms demandé {sorted(yields.items())} "
      f"(les autres : défaut de Codex)")

print("\n## Classification des appels de tools (heuristique), moyenne par lancement")
for lab, sel in (("tâches communes", common), ("toutes", tasks)):
    for arm in "AB":
        r = [x for x in rows if x["arm"] == arm and x["task"] in sel]
        k = sum((x["kinds"] for x in r), Counter())
        print(f"{lab}, {arm} : " + ", ".join(f"{n} {v / len(r):.1f}" for n, v in sorted(k.items()))
              + f" ; total {sum(k.values()) / len(r):.1f}")

print("\n## Échecs de py-graphiti-2030 et node-testsprite-356")
MARKS = {
    "py-graphiti-2030": [
        ("fusionne extracted_edge.episodes", lambda c: "extracted_edge.episodes" in c),
        ("patch d'edge_operations.py avec episode.uuid, sans extracted_edge.episodes",
         lambda c: "episode.uuid" in c and "edge_operations.py" in c and "extracted_edge.episodes" not in c),
        ("utilise store_raw_episode_content", lambda c: "self.store_raw_episode_content" in c),
    ],
    "node-testsprite-356": [
        ("change la clé TESTSPRITE_API_KEY d'un test (fixture)",
         lambda c: "doctor.test.ts" in c and re.search(r"^-.*TESTSPRITE_API_KEY: '", c, re.M)
         and re.search(r"^\+.*TESTSPRITE_API_KEY: '", c, re.M)),
        ("modifie src/lib/config.ts", lambda c: "Update File: src/lib/config.ts" in c),
        ("test ajouté avec loadConfigFn simulé", lambda c: "doctor.test.ts" in c and "loadConfigFn:" in c),
        ("test avec un vrai fichier illisible (chmodSync)", lambda c: "doctor.test.ts" in c and "chmodSync(" in c),
        ("lance la suite complète (npm test sans fichier)", lambda c: bool(re.search(r"npm test\"\"\"|npm test'''|'npm test'|\"npm test\"", c))),
    ],
}
for t, marks in MARKS.items():
    for p in (1, 2, 3):
        for arm in "AB":
            x = cell[(p, t, arm)]
            root = ET.parse(x["junit"]).getroot()
            cases = list(root.iter("testcase"))
            fails = [tc.get("name") for tc in cases if tc.find("failure") is not None or tc.find("error") is not None]
            hits = {lab: [turn for turn, c in x["calls_text"] if f(c)] for lab, f in marks}
            print(f"{t} passe {p} bras {arm} ({x['run'].name}) : {x['status']}, {x['calls']} appels ; tests {len(cases)}, "
                  f"en échec {len(fails)} {fails[:4]} ; repères (tours) {hits}")
