#!/usr/bin/env python3
"""Contrôles d'une chauffe du smoke bench sur le vrai backend (YA-09), lecture seule.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/check_warmup.py <run>/tasks/<tâche>/run-1 [<run>/report.json]
    python3 -I bench/smoke/tools/check_warmup.py <run>/tasks/<tâche>/continue-1

Lit le dossier d'une tâche (`run.py`) ou d'une reprise (`continue_session.py`), lancé avec
`--log-bodies`, et affiche :
- corps (`proxy-bodies.ndjson`) : valeurs de `reasoning` et d'`include`, requêtes parties sans
  `previous_response_id` (corps complets) ;
- reverse proxy (`proxy.ndjson`) : transports, statuts, champ `continuation`, réponses,
  `reasoning_tokens` par réponse ;
- sessions (`sessions/*.jsonl`) : champs `yaRequest` et `yaTools` de l'en-tête, blocs de
  raisonnement (texte vide, signature, `encrypted_content`, résumé) ;
- `err.log` : taille et occurrences de `Warning` et `error` ;
- avec `report.json` : statut, appels en continuation et en invalidation, miss serveur,
  compactions, taux de cache de la session.

Aucune valeur de secret n'est lue ni affichée (ni en-têtes, ni jetons). Bibliothèque standard."""
import json, sys, glob
from collections import Counter
from pathlib import Path

d = Path(sys.argv[1])
jl = lambda p: [json.loads(l) for l in open(p) if l.strip()]
bodies_file = d / "proxy-bodies.ndjson"
if not bodies_file.exists():
    sys.exit(f"{bodies_file} absent : lancer la chauffe avec --log-bodies")
bodies = [b["body"] for b in jl(bodies_file)]
print(f"== {d}")
print(f"corps : {len(bodies)} ; reasoning : {dict(Counter(json.dumps(b.get('reasoning'), sort_keys=True) for b in bodies))}")
print(f"  include : {dict(Counter(json.dumps(b.get('include')) for b in bodies))}")
full = [i + 1 for i, b in enumerate(bodies) if not b.get("previous_response_id")]
print(f"  corps sans previous_response_id (complets) : requêtes {full} sur {len(bodies)}")
log = jl(d / "proxy.ndjson")
reqs = [x for x in log if x.get("event") == "request"]
resps = [x for x in log if x.get("event") == "response"]
other = Counter(x.get("event") for x in log if x.get("event") not in ("request", "response"))
print(f"proxy : requêtes {len(reqs)} ; transports {dict(Counter(x['transport'] for x in reqs))} ; statuts {dict(Counter(x['status'] for x in reqs))} ; "
      f"continuation {[x.get('continuation') for x in reqs]} ; autres événements {dict(other)}")
print(f"  réponses : {dict(Counter((x.get('kind'), x.get('status')) for x in resps))}")
rt = [x.get("usage", {}).get("reasoning_tokens") for x in resps]
print(f"  reasoning_tokens par réponse : {rt} ; > 0 : {sum((t or 0) > 0 for t in rt)}/{len(rt)}")
for f in sorted(glob.glob(str(d / "sessions" / "*.jsonl"))):
    entries = jl(f)
    header = entries[0]
    print(f"session {Path(f).name} : type {header.get('type')} ; yaRequest {header.get('yaRequest')} ; yaTools {header.get('yaTools')}")
    th = [c for e in entries if e.get("type") == "message" and (e.get("message") or {}).get("role") == "assistant"
          for c in e["message"].get("content") or [] if c.get("type") == "thinking"]
    sig = [json.loads(c["thinkingSignature"]) for c in th if c.get("thinkingSignature")]
    print(f"  blocs de raisonnement {len(th)} ; texte vide {sum(not c.get('thinking') for c in th)} ; signés {len(sig)} ; "
          f"type reasoning {sum(s.get('type') == 'reasoning' for s in sig)} ; avec encrypted_content {sum(bool(s.get('encrypted_content')) for s in sig)} ; "
          f"summary non vide {sum(bool(s.get('summary')) for s in sig)}")
for f in ("err.log",):
    p = d / f
    if p.exists():
        t = p.read_text(errors="ignore")
        print(f"  {f} : {len(t)} octets ; 'Warning' {t.count('Warning')} ; 'error' {t.lower().count('error')}")
if len(sys.argv) > 2:
    r = json.load(open(sys.argv[2]))["results"][0]
    c = r["cost"]["theoretical"]["calls"]
    print(f"rapport : statut {r['status']} ; appels continuation {c.get('continuation')}, invalidation {c.get('invalidation')} ; "
          f"invalidations {r['cost']['invalidations']['count']} ; miss serveur {r['server_misses']} ; compactions {r['compactions']} ; "
          f"taux de cache session {r['cache']['session_rate']:.3f}")
