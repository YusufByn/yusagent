#!/usr/bin/env python3
"""Compare les requêtes capturées de deux runs du smoke bench en mode capture (YA-06).

Usage :
    python3 -I bench/smoke/tools/compare_capture_prompts.py <run X> <run Y>

<run X> et <run Y> sont deux dossiers de run de `run.py --capture`. Les requêtes
(`capture/*.body.json`) sont appariées par nom. `instructions`, `tools` et les
autres clés (hors `input` et `prompt_cache_key`) sont comparés à l'identique ;
`input` après normalisation des UUID, des identifiants et des chemins de run.
Par requête : caractères et tokens des instructions, et nombre de mentions des
marqueurs des sous-agents (`rlm.spawn`, `## Subagents`) et de la messagerie
(`agent_message.send`, `rlm_heartbeat`, inventaire des skills). Dernière ligne :
`TOUT IDENTIQUE` ou `DIFFÉRENCES`.

Tokens : tiktoken `o200k_base` si le module est importable (estimation, pas le
tokenizer du provider), sinon `None`. Pas d'appel réseau.
"""
import json
import re
import sys
from pathlib import Path

try:
    import tiktoken

    ENC = tiktoken.get_encoding("o200k_base")
except Exception:
    ENC = None

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
IGNORED = ("instructions", "tools", "input", "prompt_cache_key")
MARKERS = {
    "rlm.spawn": "rlm.spawn",
    "'## Subagents'": "## Subagents",
    "agent_message.send": "agent_message.send",
    "rlm_heartbeat": "rlm_heartbeat",
    "agent-message(inv)": "<name>agent-message</name>",
    "rlm-heartbeat(inv)": "<name>rlm-heartbeat</name>",
}


def norm(text, run):
    text = text.replace(str(run), "<RUN>")
    text = re.sub(r"\d{8}T\d{6}Z-[0-9a-f]{7}", "<RUNID>", text)
    return UUID.sub("<UUID>", text)


def bodies(run):
    return {
        p.relative_to(run).as_posix(): json.loads(p.read_text())
        for p in sorted(run.rglob("capture/*.body.json"))
    }


def main(argv):
    if len(argv) != 2:
        sys.exit(__doc__)
    x, y = Path(argv[0]), Path(argv[1])
    bx, by = bodies(x), bodies(y)
    print("requêtes :", len(bx), len(by), "mêmes noms :", sorted(bx) == sorted(by))
    same_all = sorted(bx) == sorted(by)
    for k in sorted(set(bx) & set(by)):
        a, b = bx[k], by[k]
        ins = a.get("instructions") == b.get("instructions")
        tools = a.get("tools") == b.get("tools")
        inp = norm(json.dumps(a.get("input"), ensure_ascii=False), x) == norm(
            json.dumps(b.get("input"), ensure_ascii=False), y
        )
        other = {kk: v for kk, v in a.items() if kk not in IGNORED} == {
            kk: v for kk, v in b.items() if kk not in IGNORED
        }
        same_all &= ins and tools and inp and other
        ia, ib = a.get("instructions") or "", b.get("instructions") or ""
        tok = (len(ENC.encode(ia)), len(ENC.encode(ib))) if ENC else (None, None)
        counts = " ".join(
            f"{label} {ia.count(m)}/{ib.count(m)}" for label, m in MARKERS.items()
        )
        print(
            f"{k}: instructions={ins} tools={tools} input={inp} autres={other} "
            f"chars {len(ia)}/{len(ib)} tokens {tok[0]}/{tok[1]} {counts}"
        )
    print("TOUT IDENTIQUE" if same_all else "DIFFÉRENCES")


if __name__ == "__main__":
    main(sys.argv[1:])
