#!/usr/bin/env python3
"""Construction d'une tâche du smoke bench depuis un candidat miné (T07, YA-14, phase B).

Usage :
    python3 -I bench/smoke/tools/make_task.py --pr <URL de PR> --id <id> [--tags a,b] \
        [--test-prefix "npm run -s build && "]

Écrit `bench/smoke/tasks/<id>/` :
  - `task.toml` : source git (commit parent, `gold` = commit de correction), licence,
    `setup` et `test`, `fail_to_pass` et `pass_to_pass` ;
  - `hidden/` : fichiers de test ajoutés ou modifiés par la correction (commit de correction) ;
  - `files/` : versions figées de la revalidation macOS (`uv.lock` produit quand le dépôt
    n'en a pas, `freeze.txt`), déplacées depuis `tools/locks/`.
`prompt.md` n'est pas écrit : l'énoncé est réécrit à la main (symptôme, sans indice).

Identifiants des tests : rapports JUnit de la revalidation macOS (`mine.py validate
--isolation sandbox-exec`, cache du minage) ; `fail_to_pass` = en succès aux 3 runs verts
et pas au rouge, `pass_to_pass` = en succès au rouge et aux 3 verts. Clé `fichier::nom`
(`classe::nom` si le nom se répète dans un fichier) ; un test dont la clé reste ambiguë
est écarté.

Bibliothèque standard + `git`.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
TASKS = HERE.parent / "tasks"
LOCKS = HERE / "locks"
MINE_CACHE = Path.home() / ".cache" / "yusagent-smoke" / "mine"

# Interpréteur figé (T07) : 3.12.14 géré par uv, téléchargé depuis python-build-standalone
# sur GitHub (`--mirror` : pas de détour par releases.astral.sh, hors liste blanche).
PY_SETUP = ("uv python install -q --mirror "
            "https://github.com/astral-sh/python-build-standalone/releases/download 3.12.14"
            " && UV_NO_BUILD=1 uv sync -q --frozen --all-extras --all-groups --python 3.12.14"
            " --managed-python --no-python-downloads"
            " && (.venv/bin/python -c 'import pytest' 2>/dev/null"
            " || UV_NO_BUILD=1 uv pip install -q --python .venv/bin/python pytest=={pytest})")
NODE_SETUP = "npm ci --ignore-scripts --no-audit --no-fund --loglevel=error"
TESTS = {
    "pytest": (".venv/bin/python -m pytest -q -p no:cacheprovider -o junit_family=xunit1 "
               "--junitxml=\"$SMOKE_JUNIT\" {files}"),
    "node": "node --test --test-reporter=junit --test-reporter-destination=\"$SMOKE_JUNIT\" {files}",
    "vitest": "./node_modules/.bin/vitest run --reporter=junit --outputFile=\"$SMOKE_JUNIT\" {files}",
}


def report_results(path: Path, work: str) -> dict[str, str]:
    """Résultat par clé `fichier::nom` (ou `classe::nom`), clés ambiguës écartées."""
    cases = []
    for case in ET.parse(path).getroot().iter("testcase"):
        name = html.unescape(case.get("name", ""))
        file = case.get("file") or ""
        if file and os.path.isabs(file):
            file = os.path.relpath(file, work)
        failed = case.find("failure") is not None or case.find("error") is not None
        skipped = case.find("skipped") is not None
        status = "failed" if failed else "skipped" if skipped else "passed"
        cases.append((file, case.get("classname", ""), name, status))
    first = Counter(f"{f or c}::{n}" for f, c, n, _ in cases)
    second = Counter(f"{c}::{n}" for f, c, n, _ in cases)
    out = {}
    for f, c, n, status in cases:
        key = f"{f or c}::{n}"
        if first[key] > 1:
            key = f"{c}::{n}"
            if second[key] > 1:
                continue
        out[key] = status
    return out


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          check=True).stdout


def toml_list(values: list[str]) -> str:
    return "[\n" + "".join(f"  {json.dumps(v, ensure_ascii=False)},\n" for v in values) + "]"


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--pr", required=True)
    p.add_argument("--id", required=True)
    p.add_argument("--tags", default="")
    p.add_argument("--test-prefix", default="", help="avant la commande de test (build)")
    p.add_argument("--timeout", type=int, default=1800)
    args = p.parse_args()

    disc = {c["pr_url"]: c for c in json.loads((HERE / "discovered.json").read_text())}
    mac = {r["pr_url"]: r for r in
           json.loads((MINE_CACHE / "validated-macos.json").read_text())["records"]}
    c, r = disc[args.pr], mac[args.pr]
    if not r["ok"]:
        sys.exit(f"{args.pr} : non retenu à la revalidation macOS ({r['reason']})")
    slug = c["repo"].replace("/", "__")
    mirror = MINE_CACHE / "repos" / slug
    reports = MINE_CACHE / "reports"
    work = str(MINE_CACHE / "work" / f"{slug}-{c['pr']}")
    red = report_results(reports / f"{slug}-{c['pr']}-red.out", work)
    greens = [report_results(reports / f"{slug}-{c['pr']}-green{i}.out", work) for i in (1, 2, 3)]
    passed = set.intersection(*({k for k, v in g.items() if v == "passed"} for g in greens))
    f2p = sorted(k for k in passed if red.get(k) != "passed")
    p2p = sorted(k for k in passed if red.get(k) == "passed")
    if not f2p:
        sys.exit("aucun test rouge -> vert dans les rapports")

    task = TASKS / args.id
    if task.exists():
        shutil.rmtree(task)
    (task / "hidden").mkdir(parents=True)
    tests = [f for f in c["test_files"]
             if f in git(mirror, "ls-tree", "-r", "--name-only", r["fix_commit"]).split("\n")]
    for f in tests:
        dest = task / "hidden" / f
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(subprocess.run(["git", "-C", str(mirror), "show", f"{r['fix_commit']}:{f}"],
                                        capture_output=True, check=True).stdout)

    files = []
    locks = LOCKS / f"{slug}-{c['pr']}"
    if locks.exists():
        (task / "files").mkdir()
        for item in sorted(locks.iterdir()):
            shutil.move(str(item), task / "files" / item.name)
        locks.rmdir()
        if (task / "files" / "uv.lock").exists():
            files.append("uv.lock")
    lang = c["lang"]
    if lang == "python":
        freeze = (task / "files" / "freeze.txt").read_text().splitlines()
        pytest = next(ln.split("==", 1)[1] for ln in freeze if ln.lower().startswith("pytest=="))
        setup = PY_SETUP.format(pytest=pytest)
    else:
        setup = NODE_SETUP
    runnable = [f for f in tests if f.endswith((".py", ".ts", ".js", ".mjs", ".cjs", ".tsx"))]
    test = args.test_prefix + TESTS[r["runner"]].format(files=" ".join(runnable))
    tags = [lang] + [t for t in args.tags.split(",") if t]
    issue = c["issue_url"]
    toml = [
        f'id = "{args.id}"',
        f'lang = "{lang}"',
        f'license = "{c["license"]}"',
        f'source = {{ git = "{c["clone_url"]}", commit = "{r["parent_commit"]}", '
        f'gold = "{r["fix_commit"]}" }}',
        f'upstream = {{ repo = "{c["repo"]}", pr = "{c["pr_url"]}", issue = "{issue}" }}',
        f"files = {json.dumps(files)}",
        f"setup = {json.dumps(setup)}",
        f"test = {json.dumps(test)}",
        f"fail_to_pass = {toml_list(f2p)}",
        f"pass_to_pass = {toml_list(p2p)}",
        f"timeout_s = {args.timeout}",
        f"tags = {json.dumps(tags)}",
    ]
    (task / "task.toml").write_text("\n".join(toml) + "\n")
    print(f"{args.id} : {len(f2p)} fail_to_pass, {len(p2p)} pass_to_pass, {len(tests)} test(s) "
          f"cachés, fichiers {files or 'aucun'}")


if __name__ == "__main__":
    main()
