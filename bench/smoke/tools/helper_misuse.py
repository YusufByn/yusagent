"""Ticket 15 (YA-09) : mauvais usages des helpers du kernel dans les sessions du banc.

Cellules `python_repl` en erreur (`details.status == "error"`) classées par cas, et mauvais usage
silencieux (`<bound method BashHandle.output`), avec la config de la session (`yaRequest` :
`promptVersion`, `bashWaitSeconds` ; absents = 1). Usage :
`python3 -I bench/smoke/tools/helper_misuse.py bench/smoke/runs 20261111` (runs datés du 11/11 au moins).
"""
import json, pathlib, re, sys, collections
root = pathlib.Path(sys.argv[1]); since = sys.argv[2]
CASES = [
  ("h.exit_code/.duration sur BashHandle", r"'(Persistent)?BashHandle' object has no attribute '(exit_code|duration)'"),
  ("attribut de handle sur BashResult", r"'BashResult' object has no attribute '(command|pid|tail|poll|kill)'"),
  (".handle sur BashResult/BashHandle", r"'Bash(Result|Handle)' object has no attribute 'handle'"),
  ("attribut absent sur BashSnapshot (fenêtre numérique)", r"'BashSnapshot' object has no attribute"),
  ("await sur BashSnapshot (fenêtre numérique)", r"object BashSnapshot can't be used in 'await'"),
  ("await read()", r"object str can't be used in 'await'"),
  (".output() sur un str (résultat/instantané)", r"'str' object is not callable"),
  ("edit(old=, new=)", r"run\(\) got an unexpected keyword"),
  ("sous-agents (v1)", r"agent_observe|RLMChildResult|got coroutine|'coroutine' object"),
]
SILENT = [("h.output sans appel (silencieux)", r"<bound method (Persistent)?BashHandle\.output")]
out = collections.defaultdict(list)
for f in sorted(root.rglob("*.jsonl")):
    if "sessions" not in f.parts: continue
    run = f.relative_to(root).parts[0]
    key = run.split("-")[1] if run.startswith("ab-") else run
    if key[:8] < since: continue
    hdr = {}
    for line in f.open():
        try: d = json.loads(line)
        except Exception: continue
        if d.get("type") == "session":
            ya = d.get("yaRequest") or {}
            hdr = (ya.get("promptVersion", 1), ya.get("bashWaitSeconds", 1))
            continue
        m = d.get("message") or {}
        if m.get("role") != "toolResult" or m.get("toolName") != "python_repl": continue
        det = m.get("details") or {}
        text = "".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text")
        probe = ""
        if det.get("status") == "error":
            e = det.get("error") or {}
            probe = f"{e.get('ename')}: {e.get('evalue')}"
            for name, pat in CASES:
                if re.search(pat, probe): out[name].append((run, hdr)); break
        for name, pat in SILENT:
            if re.search(pat, text): out[name].append((run, hdr))
for name, _ in CASES + SILENT:
    l = out.get(name, [])
    cfg = collections.Counter(f"v{h[0]}/{h[1]}" if h else "?" for _, h in l)
    print(f"{len(l):3d}  {name:45s} {dict(cfg)}  dernier: {max((r for r,_ in l), default='-')}")
