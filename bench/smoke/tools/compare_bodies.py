#!/usr/bin/env python3
"""Changements de requête entre deux chauffes (`proxy-bodies.ndjson`), lecture seule (YA-09).

Usage, depuis la racine du dépôt :
    python3 -I bench/smoke/tools/compare_bodies.py <référence>/proxy-bodies.ndjson <branche>/proxy-bodies.ndjson

Les deux fichiers viennent de runs lancés avec `--log-bodies` (`run.py` ou
`continue_session.py`). Affiche :
- pour le premier corps de chaque fichier, chaque clé de premier niveau : identique octet pour
  octet (JSON trié) ou différente, avec sa taille côté branche ; une clé présente d'un seul côté ;
- pour tous les corps : les jeux de clés de premier niveau et leur nombre ;
- la forme de l'`input` du premier corps (type et rôle de chaque élément, sans le contenu).

Deux tâches différentes ont un `input` différent (énoncé) et deux sessions un `prompt_cache_key`
différent : seules les autres clés disent si la requête a changé. Aucun en-tête n'est lu
(`--log-bodies` n'en écrit pas). Bibliothèque standard.
"""
import json
import sys
from collections import Counter


def load(path: str) -> list[dict]:
    with open(path) as handle:
        return [json.loads(line)["body"] for line in handle if line.strip()]


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[3].strip(), file=sys.stderr)
        return 2
    ref, new = load(argv[0]), load(argv[1])
    if not ref or not new:
        print("fichier de corps vide", file=sys.stderr)
        return 1
    first_ref, first_new = ref[0], new[0]
    print(f"corps : référence {len(ref)}, branche {len(new)}")
    print("premier corps :")
    for key in sorted(first_ref.keys() | first_new.keys()):
        if key not in first_ref or key not in first_new:
            side = "référence" if key in first_ref else "branche"
            print(f"  {key} : présent seulement dans {side}")
            continue
        same = (json.dumps(first_ref[key], sort_keys=True)
                == json.dumps(first_new[key], sort_keys=True))
        size = len(json.dumps(first_new[key]))
        print(f"  {key} : {'identique' if same else 'différent'} ({size} octets côté branche)")
    for name, bodies in (("référence", ref), ("branche", new)):
        sets = Counter(tuple(sorted(body.keys())) for body in bodies)
        print(f"jeux de clés ({name}) :")
        for keys, count in sets.most_common():
            print(f"  {count} × {', '.join(keys)}")
    for name, body in (("référence", first_ref), ("branche", first_new)):
        shape = [(item.get("type"), item.get("role")) for item in body.get("input", [])]
        print(f"input du premier corps ({name}) : {shape}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
