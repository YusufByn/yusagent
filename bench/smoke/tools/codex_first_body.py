#!/usr/bin/env python3
"""Premier corps envoyé au modèle par Codex CLI, comparé entre deux runs `--capture` (YA-14).

Usage :
    python3 -I bench/smoke/tools/codex_first_body.py <run X> <run Y> [--tasks id1,id2]
    python3 -I bench/smoke/tools/codex_first_body.py --show <run> [--tasks id1]

Preuve demandée avant de couper une fonction de Codex (`codex.featureCuts`) : le prompt et les
tools du premier corps de génération (hors prewarm `generate: false`) restent identiques entre
la config livrée et celle du banc. Lu dans `tasks/<tâche>/run-*/capture/*.body.json` (serveur de
capture, mode `--capture`).

Normalisation, seulement ce qui change d'une session à l'autre sans venir de la config :
identifiants d'items (`id` : dérivés de l'id du thread), `prompt_cache_key` et
`client_metadata` (ids de session, de tour et d'installation, horodatage). Tout le reste est
comparé tel quel : champs de la requête, chaque item d'entrée (texte des messages, tools de
l'item `additional_tools` avec leurs descriptions). `--show` affiche la structure d'un corps
(taille de chaque item, noms des tools, effort, verbosité) et cherche AGENTS.md.
Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import difflib
import json
from pathlib import Path

VOLATILE = ("prompt_cache_key", "client_metadata")


def first_body(task_run: Path) -> dict | None:
    for meta_path in sorted((task_run / "capture").glob("*.meta.json")):
        meta = json.loads(meta_path.read_text())
        if (meta.get("response") or {}).get("kind") == "prewarm" or meta.get("generate") is False:
            continue
        return json.loads(meta_path.with_name(meta_path.name.replace(".meta.", ".body.")).read_bytes())
    return None


def strip_ids(value):
    if isinstance(value, dict):
        return {k: strip_ids(v) for k, v in value.items() if k != "id"}
    if isinstance(value, list):
        return [strip_ids(v) for v in value]
    return value


def normalize(body: dict) -> dict:
    return strip_ids({k: v for k, v in body.items() if k not in VOLATILE})


def tool_names(body: dict) -> list[str]:
    names = []

    def walk(tools, prefix=""):
        for t in tools or []:
            if t.get("type") == "namespace":
                walk(t.get("tools"), prefix + t.get("name", "") + ".")
            else:
                names.append(prefix + str(t.get("name")))

    for item in body.get("input") or []:
        if item.get("type") == "additional_tools":
            walk(item.get("tools"))
    walk(body.get("tools"))
    return names


def nested_tools(body: dict) -> list[str]:
    """Tools imbriqués annoncés dans la description d'`exec` (sections `### `nom``)."""
    for item in body.get("input") or []:
        if item.get("type") != "additional_tools":
            continue
        stack = list(item.get("tools") or [])
        while stack:
            t = stack.pop()
            if t.get("type") == "namespace":
                stack += t.get("tools") or []
            elif t.get("name") == "exec":
                return [line[5:-1] for line in (t.get("description") or "").splitlines()
                        if line.startswith("### `") and line.endswith("`")]
    return []


def task_runs(run: Path, tasks: list[str] | None) -> dict[str, Path]:
    found = {}
    for path in sorted(run.glob("tasks/*/run-*")):
        if tasks is None or path.parent.name in tasks:
            found.setdefault(path.parent.name, path)
    return found


def show(run: Path, tasks: list[str] | None) -> None:
    for task, path in task_runs(run, tasks).items():
        body = first_body(path)
        if body is None:
            print(f"{task} : pas de corps de génération")
            continue
        print(f"## {task} ({len(json.dumps(body))} caractères)")
        print(f"- effort {body.get('reasoning')}, texte {body.get('text')}, include "
              f"{body.get('include')}, service_tier {body.get('service_tier')}, "
              f"parallel_tool_calls {body.get('parallel_tool_calls')}, champ tools "
              f"{'absent' if 'tools' not in body else len(body['tools'])}")
        print(f"- tools : {tool_names(body)}")
        print(f"- tools imbriqués dans exec : {nested_tools(body)}")
        for i, item in enumerate(body.get("input") or []):
            text = json.dumps(item, ensure_ascii=False)
            print(f"- entrée {i} : {item.get('type')} {item.get('role') or ''} "
                  f"({len(text)} caractères) {text[:90]}")
        agents = [i for i, item in enumerate(body.get("input") or [])
                  if "AGENTS.md" in json.dumps(item, ensure_ascii=False)]
        print(f"- items qui citent AGENTS.md : {agents or 'aucun'}")


def compare(x: Path, y: Path, tasks: list[str] | None) -> bool:
    same = True
    xs, ys = task_runs(x, tasks), task_runs(y, tasks)
    for task in sorted(set(xs) | set(ys)):
        if task not in xs or task not in ys:
            print(f"{task} : absente d'un des runs")
            same = False
            continue
        bx, by = first_body(xs[task]), first_body(ys[task])
        if bx is None or by is None:
            print(f"{task} : pas de corps de génération")
            same = False
            continue
        nx, ny = normalize(bx), normalize(by)
        if nx == ny:
            print(f"{task} : premier corps identique (hors {', '.join(VOLATILE)} et id d'items ;"
                  f" {len(json.dumps(bx))} et {len(json.dumps(by))} caractères bruts)")
            continue
        same = False
        print(f"{task} : premier corps DIFFÉRENT")
        print(f"  tools X {tool_names(bx)}\n  tools Y {tool_names(by)}")
        print(f"  imbriqués X {nested_tools(bx)}\n  imbriqués Y {nested_tools(by)}")
        for key in sorted(set(nx) | set(ny)):
            if key != "input" and nx.get(key) != ny.get(key):
                print(f"  champ {key} : {json.dumps(nx.get(key))} -> {json.dumps(ny.get(key))}")
        ix, iy = nx.get("input") or [], ny.get("input") or []
        for i in range(max(len(ix), len(iy))):
            a = json.dumps(ix[i], ensure_ascii=False, indent=1) if i < len(ix) else ""
            b = json.dumps(iy[i], ensure_ascii=False, indent=1) if i < len(iy) else ""
            if a != b:
                print(f"  entrée {i} :")
                for line in list(difflib.unified_diff(a.splitlines(), b.splitlines(), "X", "Y",
                                                      lineterm="", n=0))[:40]:
                    print("    " + line[:200])
    return same


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", type=Path, nargs="+")
    parser.add_argument("--tasks", default="")
    parser.add_argument("--show", action="store_true")
    args = parser.parse_args()
    tasks = [t for t in args.tasks.split(",") if t] or None
    if args.show:
        for run in args.runs:
            show(run, tasks)
        return
    if len(args.runs) != 2:
        parser.error("deux runs attendus (X Y)")
    raise SystemExit(0 if compare(args.runs[0], args.runs[1], tasks) else 1)


if __name__ == "__main__":
    main()
