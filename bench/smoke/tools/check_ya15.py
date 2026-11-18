#!/usr/bin/env python3
"""Contrôle de la chauffe du ticket 15 (YA-09), lecture seule.

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/check_ya15.py <référence main>/tasks/<tâche>/run-1 <branche>/tasks/<tâche>/run-1

Les deux dossiers viennent de runs lancés avec `--log-bodies` sur la même tâche (la référence
peut être un run `--capture` : le premier corps part avant toute réponse du modèle).
1. Premier corps (`proxy-bodies.ndjson`) : chaque clé de premier niveau, sauf `prompt_cache_key`,
   identique en JSON trié ; pour `input`, le premier élément qui diffère est montré.
2. Session de la branche (`sessions/*.jsonl`) : chaque résultat `python_repl` a `isError` égal à
   « `details.status` vaut `error` ou `aborted` ». Sans cellule en erreur, le critère n'est pas
   vérifiable sur ce run (dit tel quel).
3. « 1011 » : mentions dans `out.jsonl` et `err.log` de la branche (même règle que
   `check_warmup.py`), et transports des requêtes au proxy.

Code de sortie : 0 si 1, 2 et 3 passent ; 1 si l'un échoue ; 3 si 1 et 3 passent mais que 2 n'est
pas vérifiable. Bibliothèque standard.
"""
import glob
import json
import re
import sys
from collections import Counter
from pathlib import Path

IGNORED = {"prompt_cache_key"}


def jl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(errors="ignore").splitlines() if line.strip()]


def canon(value) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


def first_body(run: Path) -> dict:
    bodies = jl(run / "proxy-bodies.ndjson")
    if not bodies:
        sys.exit(f"{run}/proxy-bodies.ndjson absent ou vide : run lancé sans --log-bodies ?")
    return bodies[0]["body"]


def check_first_body(ref: Path, new: Path) -> bool:
    a, b = first_body(ref), first_body(new)
    differing = []
    for key in sorted((a.keys() | b.keys()) - IGNORED):
        if key not in a or key not in b:
            differing.append(key)
            print(f"  {key} : présent seulement dans {'la référence' if key in a else 'la branche'}")
        elif canon(a[key]) != canon(b[key]):
            differing.append(key)
            print(f"  {key} : différent")
    if "input" in differing and isinstance(a.get("input"), list) and isinstance(b.get("input"), list):
        for index, (x, y) in enumerate(zip(a["input"], b["input"])):
            if canon(x) != canon(y):
                xs, ys = canon(x), canon(y)
                at = next((i for i, (c, d) in enumerate(zip(xs, ys)) if c != d), min(len(xs), len(ys)))
                print(f"    input[{index}] diffère à l'octet {at} : "
                      f"référence …{xs[max(0, at - 60):at + 60]!r}… / branche …{ys[max(0, at - 60):at + 60]!r}…")
                break
        else:
            print(f"    input : {len(a['input'])} éléments contre {len(b['input'])}")
    same = sorted((a.keys() & b.keys()) - IGNORED - set(differing))
    print(f"  clés identiques : {', '.join(same)}")
    print(f"  prompt_cache_key ignorée : {a.get('prompt_cache_key')!r} / {b.get('prompt_cache_key')!r}")
    return not differing


def check_is_error(new: Path) -> bool | None:
    failed = ok = 0
    wrong = []
    for name in sorted(glob.glob(str(new / "sessions" / "*.jsonl"))):
        for entry in jl(Path(name)):
            message = entry.get("message") or {}
            if message.get("role") != "toolResult" or message.get("toolName") != "python_repl":
                continue
            status = (message.get("details") or {}).get("status")
            expected = status in ("error", "aborted")
            failed += expected
            ok += not expected
            if message.get("isError") is not expected:
                ename = ((message.get("details") or {}).get("error") or {}).get("ename")
                wrong.append(f"{Path(name).name} {message.get('toolCallId', '')[:24]} status={status} "
                             f"ename={ename} isError={message.get('isError')}")
    print(f"  cellules python_repl : {failed} en erreur (status error ou aborted), {ok} sans erreur")
    for line in wrong:
        print(f"  isError incohérent : {line}")
    if wrong:
        return False
    if failed == 0:
        print("  aucune cellule en erreur : critère non vérifiable sur ce run")
        return None
    return True


def check_1011(new: Path) -> bool:
    mentions = {f: len(re.findall(r"\b1011\b", (new / f).read_text(errors="ignore")))
                for f in ("out.jsonl", "err.log") if (new / f).exists()}
    requests = [x for x in jl(new / "proxy.ndjson") if x.get("event") == "request"]
    print(f"  mentions de 1011 : {mentions} ; transports au proxy : {dict(Counter(x.get('transport') for x in requests))}")
    return sum(mentions.values()) == 0


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[3].strip(), file=sys.stderr)
        return 2
    ref, new = Path(argv[0]), Path(argv[1])
    print(f"référence : {ref}\nbranche   : {new}")
    print("1. premier corps, hors prompt_cache_key :")
    body = check_first_body(ref, new)
    print(f"   -> {'OK' if body else 'ÉCHEC'}")
    print("2. isError des cellules en erreur (session de la branche) :")
    flag = check_is_error(new)
    print(f"   -> {'OK' if flag else 'ÉCHEC' if flag is False else 'NON VÉRIFIABLE'}")
    print("3. aucun 1011 :")
    close = check_1011(new)
    print(f"   -> {'OK' if close else 'ÉCHEC'}")
    if not body or flag is False or not close:
        return 1
    return 3 if flag is None else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
