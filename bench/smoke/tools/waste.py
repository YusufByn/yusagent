#!/usr/bin/env python3
"""Gaspillage dans les sessions d'un run (ticket 17, YA-09) : comptage heuristique.

Usage, depuis la racine du dépôt (tiktoken n'est pas une dépendance du dépôt, `uv` l'installe
dans un environnement jetable ; sans lui, les colonnes de tokens et de coût valent `n.d.`) :

    uv run --no-project --with tiktoken==0.14.0 python -I bench/smoke/tools/waste.py \\
        [--prompt-version 2] [--json <fichier>] [--reread-covered X] [--similar-ratio X] \\
        <run ou dossier d'A/B> ...

Un argument est un run (`<run>/tasks/<tâche>/run-*/sessions/*.jsonl`) ou un dossier d'A/B
(`<ab>/arms/<run>/...`). `--prompt-version N` garde les sessions dont l'en-tête porte
`yaRequest.promptVersion == N` (absent : 1). `--reread-covered` et `--similar-ratio` remplacent
les seuils (tests de sensibilité). `--json` écrit chaque cas (cellule, fichier, code) pour la
relecture à la main. Lecture seule.

Trois comportements, comptés par run de tâche (session principale et sous-agents) sur les
cellules `python_repl` (et les appels `apply_patch`, lus comme écritures) :

1. **Relecture redondante** : lecture d'un fichier dont au moins `REREAD_MIN_COVERED` des
   lignes ont déjà été lues depuis sa dernière écriture. Lectures : `read(path, offset, limit)`,
   `open(path)` en lecture, `Path(path).read_text()`, et dans un `bash()` : `cat`, `nl`,
   `sed -n 'a,bp'`, `head`, `tail` (seuls ou en tête de tube suivis de `head`, `tail` ou
   `sed -n`). Plages en lignes ; un fichier entier vaut les lignes 1 à `WHOLE_FILE_LINES`
   (défaut de `read()`), `tail -n N` vaut les N dernières lignes (comparées aux autres `tail`
   et aux lectures entières seulement). Une lecture Python ne compte, comme première lecture ou
   comme relecture, que si sa valeur est affichée en entier : `print(read(...))`, variable
   passée telle quelle à `print`, ou dernière expression de la cellule (`s = read(p);
   print(s[i:j])` n'est pas une lecture vue). Les lectures faites par `bash()` comptent toujours
   comme vues.
2. **Cellules presque identiques** : une cellule dont le code, découpé en mots (`\\w+` ou un
   signe), a un ratio `difflib.SequenceMatcher` d'au moins `SIMILAR_RATIO` avec l'une des
   `SIMILAR_WINDOW` cellules `python_repl` précédentes de la session, les deux d'au moins
   `SIMILAR_MIN_TOKENS` mots. Comptées à part (colonne « écartées », hors gaspillage) : la
   précédente a échoué (erreur Python, `exit_code`/`exit` non nul ou `command not found` dans
   sa sortie : la suivante corrige), ou le workspace a été écrit depuis la précédente
   (relancer après un changement est attendu).
3. **Test relancé sans changement** : un `bash()` dont les commandes de test (`TEST_COMMAND`,
   redirections retirées) sont les mêmes qu'un lancement précédent de la session, sans
   écriture dans le workspace entre les deux.

Écritures : `edit()` / `edit.run()`, `apply_patch`, `open(path, 'w'|'a'|'x'|'+')`,
`.write_text()` / `.write_bytes()`, et dans un `bash()` : redirection `>`/`>>`, `tee`,
`sed -i`, `cp`, `mv`, `rm`, `touch`, `mkdir`, `ln`. Écriture « tout fichier » : chemin non
résolu (variable, f-string), `git checkout|restore|stash|apply|reset|...`, `patch`, formateurs
(`--write`, `--fix`, `black`, `ruff format`...), script inline (`python -c`, heredoc, `node -e`)
qui contient `write`. Écriture du workspace sans fichier source (efface l'état des tests, pas
celui des lectures) : installation de dépendances, build. `/tmp`, `/dev/null` et `/var/folders`
ne sont pas le workspace.

Ordre : les opérations d'une cellule sont prises dans l'ordre du code (AST), une cellule après
l'autre. Une cellule en erreur (`details.status == "error"`) garde ses écritures (prudence) mais
ses lectures et ses tests ne comptent pas comme faits (on ne sait pas où elle s'est arrêtée) ;
une cellule à sortie tronquée (`[... output truncated:`) ne compte pas ses lectures. Une cellule
qui ne se parse pas n'a rien exécuté : ignorée.

Tokens (tokenizer **supposé** `o200k_base` ; celui des modèles du banc n'est pas publié) : pour
chaque cellule touchée (union des trois comportements, puis chacun à part), code + texte du
résultat. Borne haute
du gain, estimée : la cellule entière compte, même si une partie seulement est redondante.
- Tokens d'entrée : (code + résultat) × nombre d'appels au modèle qui suivent dans la session
  (ils les portent dans leur prompt) ; part rapportée au prompt total des appels de la boucle
  (`input + cacheRead + cacheWrite`, journal d'usage).
- Coût à cache théorique (même modèle que `metrics.theoretical_cache`) : sortie de l'appel qui
  a écrit la cellule au tarif de sortie (sortie rapportée par le provider, raisonnement compris,
  si l'appel n'a que cette cellule ; sinon le code estimé), résultat au tarif d'entrée dans
  l'appel suivant, puis code et résultat au tarif de lecture du cache dans chaque appel
  suivant. Part rapportée au `cost.theoretical_total` du rapport du run (`report.json`).
"""
from __future__ import annotations

import argparse
import ast
import difflib
import json
import re
import shlex
import sys
import warnings
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import metrics  # noqa: E402

WHOLE_FILE_LINES = 2000  # défaut de `read(limit=2000)`
REREAD_MIN_COVERED = 0.5
SIMILAR_RATIO = 0.9
SIMILAR_WINDOW = 3
SIMILAR_MIN_TOKENS = 20
WORD = re.compile(r"\w+|[^\w\s]")
FAILED = re.compile(r"\bexit(?:_code=|\s+)-?[1-9]\d*|command not found|^Traceback", re.M)
TRUNCATED = "[... output truncated:"
OUTSIDE = ("/tmp/", "/dev/", "/var/folders/", "/private/")
ALL = "*"

TEST_COMMAND = re.compile(
    r"(?:^|[\s/])(?:py\.?test|tox|nox|jest|vitest|mocha|ava)(?:\s|$)"
    r"|python[\d.]*\s+-m\s+(?:pytest|unittest)\b"
    r"|\b(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?test(?::[\w:-]+)?\b"
    r"|\b(?:go|cargo)\s+test\b|\bnode\s+--test\b|\bmake\s+(?:test|check)\b")
REDIRECT = re.compile(r"(?<![<&\d])(?:\d?>>?|&>)\s*(?!&)([^\s;&|<>]+)")
STRIP_REDIRECT = re.compile(r"\s*(?:\d?>>?|&>)\s*&?[^\s;&|]*")
SED_RANGE = re.compile(r"^(\d+)(?:,(\d+|\$))?p$")
GIT_WRITE = {"checkout", "restore", "stash", "apply", "reset", "am", "cherry-pick", "merge",
             "rebase", "pull", "switch", "clean", "rm", "mv", "revert"}
FORMATTERS = re.compile(r"--write\b|--fix\b|\bblack\b|\bisort\b|\bruff\s+format\b|\bgofmt\s+-w\b"
                        r"|\b(?:cargo|go)\s+fmt\b|\brustfmt\b|\bprettier\b.*\s-w\b")
WORKSPACE_ONLY = re.compile(
    r"\b(?:npm|pnpm|yarn|bun)\s+(?:install|i|ci|add|run\s+build|build)\b|\bpip\d?\s+install\b"
    r"|\buv\s+(?:sync|add|pip\s+install|lock)\b|\bcargo\s+build\b|\bmake\b(?!\s+(?:test|check))"
    r"|\btsc\b(?!.*--noEmit)|\bsetup\.py\b")
INLINE_SCRIPT = re.compile(r"\b(?:python[\d.]*|node|ruby|perl)\s+(?:-[ce]\b|-\s|-$)|<<")


# ---------------------------------------------------------------- opérations d'une cellule

def in_workspace(path: str) -> bool:
    return not path.startswith(OUTSIDE)


def norm_path(path: str) -> str:
    path = path.strip().strip("'\"")
    while path.startswith("./"):
        path = path[2:]
    return path


def shell_ops(command: str) -> list[tuple]:
    """Opérations d'une commande shell : ("read", path, (a, b) | ("tail", n)),
    ("write", path | ALL), ("workspace",), ("test", key)."""
    ops: list[tuple] = []
    tests: list[str] = []
    for chain in re.split(r"&&|\|\||;|\n", command):
        stages = [s.strip() for s in chain.split("|") if s.strip()]
        if not stages:
            continue
        for stage in stages:
            ops += stage_writes(stage)
            if TEST_COMMAND.search(stage):
                tests.append(" ".join(STRIP_REDIRECT.sub("", stage).split()))
        ops += pipeline_reads(stages)
    if INLINE_SCRIPT.search(command) and "write" in command:
        ops.append(("write", ALL))
    if tests:
        ops.append(("test", tuple(tests)))
    return ops


def split_words(stage: str) -> list[str]:
    try:
        return shlex.split(stage, comments=False)
    except ValueError:
        return stage.split()


def stage_writes(stage: str) -> list[tuple]:
    ops = []
    for target in REDIRECT.findall(stage):
        target = norm_path(target)
        if target and in_workspace(target):
            ops.append(("write", target))
    words = split_words(stage)
    if not words:
        return ops
    cmd, args = words[0], [w for w in words[1:] if not w.startswith("-")]
    if cmd == "tee":
        ops += [("write", norm_path(a)) for a in args if in_workspace(a)]
    elif cmd == "sed" and any(w.startswith("-i") for w in words[1:]):
        ops += [("write", norm_path(a)) for a in args[1:] if in_workspace(a)] or [("write", ALL)]
    elif cmd in ("cp", "mv", "rm", "touch", "mkdir", "ln"):
        targets = args if cmd in ("mv", "rm", "touch", "mkdir") else args[-1:]
        ops += [("write", norm_path(a)) for a in targets if in_workspace(a)]
    elif cmd == "git" and len(words) > 1 and words[1] in GIT_WRITE:
        ops.append(("write", ALL))
    elif cmd == "patch" or FORMATTERS.search(stage):
        ops.append(("write", ALL))
    elif WORKSPACE_ONLY.search(stage):
        ops.append(("workspace",))
    return ops


def sed_ranges(script: str) -> list[tuple[int, int]] | None:
    out = []
    for part in script.split(";"):
        m = SED_RANGE.match(part.strip())
        if not m:
            return None
        a = int(m.group(1))
        b = WHOLE_FILE_LINES if m.group(2) == "$" else int(m.group(2) or a)
        out.append((a, b))
    return out


def count_arg(words: list[str], default: int) -> int | str:
    """`-n N`, `-nN`, `-N` ou `-n +K` (renvoyé "+K") de head/tail."""
    for i, w in enumerate(words):
        value = None
        if w == "-n" and i + 1 < len(words):
            value = words[i + 1]
        elif w.startswith("-n") and len(w) > 2:
            value = w[2:]
        elif re.fullmatch(r"-\d+", w):
            value = w[1:]
        if value is not None:
            return value if value.startswith("+") else int(value) if value.isdigit() else default
    return default


def files_of(words: list[str]) -> list[str]:
    out, skip = [], False
    for w in words[1:]:
        if skip:
            skip = False
            continue
        if w in ("-n", "-c", "-e"):
            skip = True
            continue
        if w.startswith("-") or w.startswith("+") or w.isdigit() and words[0] in ("head", "tail"):
            continue
        out.append(w)
    return out


def view_range(words: list[str]):
    """Plage vue à travers `head`, `tail` ou `sed -n` ; None si autre chose."""
    if not words:
        return None
    cmd = words[0]
    if cmd == "head":
        n = count_arg(words, 10)
        return [(1, n)] if isinstance(n, int) else None
    if cmd == "tail":
        n = count_arg(words, 10)
        if isinstance(n, str):
            return [(int(n[1:]), WHOLE_FILE_LINES)]
        return ("tail", n)
    if cmd == "sed" and "-n" in words:
        scripts = [w for w in words[1:] if not w.startswith("-")]
        return sed_ranges(scripts[0]) if scripts else None
    return None


def pipeline_reads(stages: list[str]) -> list[tuple]:
    first = split_words(stages[0])
    if not first:
        return []
    cmd = first[0]
    if cmd in ("cat", "nl"):
        files = files_of(first)
        rng = [(1, WHOLE_FILE_LINES)]
        if len(stages) > 1:
            rng = view_range(split_words(stages[1]))
            if rng is None:
                return []  # cat f | grep … : une recherche, pas une lecture
    elif cmd in ("head", "tail") or cmd == "sed" and "-n" in first:
        if cmd == "sed":
            scripts = [w for w in first[1:] if not w.startswith("-")]
            files = scripts[1:]
        else:
            files = files_of(first)
        rng = view_range(first)
        if rng is None:
            return []
    else:
        return []
    out = []
    for f in files:
        if any(c in f for c in "*?$`") or not in_workspace(f):
            continue
        if isinstance(rng, tuple):
            out.append(("read", norm_path(f), rng, True))
        else:
            out += [("read", norm_path(f), r, True) for r in rng]
    return out


def literal(node, env: dict) -> list[str] | None:
    """Valeurs possibles d'une expression : constante, nom lié à une constante dans la cellule,
    ou variable d'une boucle `for` sur une liste de constantes ; None sinon."""
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return [node.value]
    if isinstance(node, ast.Name) and node.id in env:
        return env[node.id]
    if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            and node.func.id in ("Path", "PurePath") and len(node.args) == 1):
        return literal(node.args[0], env)
    return None


def bound_names(tree) -> dict:
    env: dict[str, list[str]] = {}
    for node in ast.walk(tree):
        if (isinstance(node, ast.Assign) and len(node.targets) == 1
                and isinstance(node.targets[0], ast.Name)):
            value = literal(node.value, env)
            if value is not None:
                env[node.targets[0].id] = value
        elif isinstance(node, (ast.For, ast.AsyncFor)) and isinstance(node.target, ast.Name):
            if isinstance(node.iter, (ast.List, ast.Tuple)):
                values = [literal(e, env) for e in node.iter.elts]
                if values and all(v is not None and len(v) == 1 for v in values):
                    env[node.target.id] = [v[0] for v in values]
    return env


def unwrap(node, parents):
    """Remonte les `await`, `.output` et `.output()` autour d'une valeur."""
    while True:
        parent = parents.get(node)
        if isinstance(parent, ast.Await) or (
                isinstance(parent, ast.Attribute) and parent.attr == "output"):
            node = parent
        elif isinstance(parent, ast.Call) and parent.func is node:
            node = parent
        else:
            return node


def printed_names(tree) -> set[str]:
    """Noms affichés en entier : argument de `print`, ou dernière expression de la cellule."""
    names = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and call_name(node.func) == "print" and isinstance(
                node.func, ast.Name):
            for arg in node.args:
                while isinstance(arg, ast.Await) or (
                        isinstance(arg, ast.Attribute) and arg.attr == "output"):
                    arg = arg.value
                if isinstance(arg, ast.Name):
                    names.add(arg.id)
    body = getattr(tree, "body", [])
    if body and isinstance(body[-1], ast.Expr) and isinstance(body[-1].value, ast.Name):
        names.add(body[-1].value.id)
    return names


def shown(call, parents, printed: set[str], tree) -> bool:
    """La valeur de l'appel est-elle affichée en entier (et donc vue par le modèle) ?"""
    node = unwrap(call, parents)
    parent = parents.get(node)
    if (isinstance(parent, ast.Call) and isinstance(parent.func, ast.Name)
            and parent.func.id == "print" and node in parent.args):
        return True
    if (isinstance(parent, ast.Assign) and len(parent.targets) == 1
            and isinstance(parent.targets[0], ast.Name)):
        return parent.targets[0].id in printed
    body = getattr(tree, "body", [])
    return bool(body) and isinstance(body[-1], ast.Expr) and body[-1].value is node


def int_arg(call: ast.Call, index: int, name: str, default: int) -> int | None:
    node = call.args[index] if len(call.args) > index else next(
        (k.value for k in call.keywords if k.arg == name), None)
    if node is None:
        return default
    if isinstance(node, ast.Constant) and isinstance(node.value, int):
        return node.value
    return None


def call_name(func) -> str:
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return ""


def cell_ops(code: str) -> list[tuple] | None:
    """Opérations d'une cellule dans l'ordre du code ; None si elle ne se parse pas."""
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            tree = compile(code, "<cell>", "exec",
                           flags=ast.PyCF_ONLY_AST | ast.PyCF_ALLOW_TOP_LEVEL_AWAIT)
    except (SyntaxError, ValueError):
        return None
    env = bound_names(tree)
    parents = {child: node for node in ast.walk(tree) for child in ast.iter_child_nodes(node)}
    printed = printed_names(tree)
    found = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            found.append(((node.lineno, node.col_offset), node))
    ops: list[tuple] = []
    for _, call in sorted(found, key=lambda x: x[0]):
        name = call_name(call.func)
        is_attr = isinstance(call.func, ast.Attribute)
        first = call.args[0] if call.args else None
        if name == "read" and not is_attr and first is not None:
            paths = literal(first, env)
            offset, limit = int_arg(call, 1, "offset", 1), int_arg(call, 2, "limit",
                                                                    WHOLE_FILE_LINES)
            if paths and offset is not None and limit is not None:
                seen_ = shown(call, parents, printed, tree)
                ops += [("read", norm_path(p), (offset, offset + limit - 1), seen_)
                        for p in paths]
        elif name in ("edit", "run") and (not is_attr or (
                isinstance(call.func.value, ast.Name) and call.func.value.id == "edit")):
            if name == "run" and not is_attr:
                continue
            paths = literal(first, env) if first is not None else None
            ops += [("write", norm_path(p)) for p in paths] if paths else [("write", ALL)]
        elif name == "open" and not is_attr and first is not None:
            mode_node = call.args[1] if len(call.args) > 1 else next(
                (k.value for k in call.keywords if k.arg == "mode"), None)
            mode = mode_node.value if isinstance(mode_node, ast.Constant) else "r"
            paths = literal(first, env)
            if not isinstance(mode, str) or any(c in mode for c in "wax+"):
                ops += ([("write", norm_path(p)) for p in paths if in_workspace(p)] if paths
                        else [("write", ALL)])
            elif paths:
                outer = parents.get(parents.get(call))
                seen_ = (isinstance(parents.get(call), ast.Attribute)
                         and parents[call].attr == "read" and isinstance(outer, ast.Call)
                         and shown(outer, parents, printed, tree))
                ops += [("read", norm_path(p), (1, WHOLE_FILE_LINES), seen_)
                        for p in paths if in_workspace(p)]
        elif name in ("write_text", "write_bytes", "read_text") and is_attr:
            paths = literal(call.func.value, env)
            if name == "read_text":
                seen_ = shown(call, parents, printed, tree)
                ops += [("read", norm_path(p), (1, WHOLE_FILE_LINES), seen_)
                        for p in paths or [] if in_workspace(p)]
            else:
                ops += ([("write", norm_path(p)) for p in paths if in_workspace(p)] if paths
                        else [("write", ALL)])
        elif name in ("remove", "unlink", "rename", "replace", "rmtree", "move", "copy",
                      "copyfile", "copy2") and is_attr:
            ops.append(("write", ALL))
        elif name in ("bash", "system") or (name in ("run", "check_output", "Popen", "call")
                                            and is_attr and isinstance(call.func.value, ast.Name)
                                            and call.func.value.id == "subprocess"):
            commands = literal(first, env) if first is not None else None
            if commands:
                for command in commands:
                    ops += shell_ops(command)
            elif isinstance(first, ast.JoinedStr):
                text = "".join(v.value for v in first.values
                               if isinstance(v, ast.Constant) and isinstance(v.value, str))
                # Commande en partie variable : écritures et tests seulement, pas de lecture.
                ops += [op for op in shell_ops(text) if op[0] != "read"]
                if REDIRECT.search(text) or "sed -i" in text:
                    ops.append(("write", ALL))
            elif first is not None:
                ops.append(("workspace",))
    return ops


def patch_paths(text: str) -> list[str]:
    return [norm_path(m) for m in re.findall(
        r"^\*\*\* (?:Update|Add|Delete) File: (.+)$", text or "", re.M)] or [ALL]


# ---------------------------------------------------------------- session

def message_text(message: dict) -> str:
    return "".join(c.get("text", "") for c in message.get("content", [])
                   if isinstance(c, dict) and c.get("type") == "text")


def load_session(path: Path) -> dict:
    """Cellules et appels au modèle d'une session, dans l'ordre."""
    lines = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    header = lines[0] if lines and lines[0].get("type") == "session" else {}
    calls, cells, results = [], [], {}
    for entry in lines:
        message = entry.get("message") or {}
        role = message.get("role")
        if role == "assistant" and isinstance(message.get("usage"), dict):
            tool_calls = [c for c in message.get("content", [])
                          if isinstance(c, dict) and c.get("type") == "toolCall"]
            calls.append({"usage": message["usage"], "model": message.get("model"),
                          "tools": len(tool_calls)})
            for c in tool_calls:
                cells.append({"id": c.get("id"), "name": c.get("name"),
                              "args": c.get("arguments") or {}, "call": len(calls) - 1})
        elif role == "toolResult":
            results[message.get("toolCallId")] = message
    for cell in cells:
        result = results.get(cell["id"]) or {}
        cell["result"] = message_text(result)
        cell["error"] = (result.get("details") or {}).get("status") == "error"
        cell["code"] = cell["args"].get("code") or ""
    return {"path": str(path), "header": header, "calls": calls, "cells": cells}


def prompt_version(header: dict) -> int:
    return (header.get("yaRequest") or {}).get("promptVersion") or 1


def detect(session: dict) -> dict:
    """Cas des trois comportements d'une session ; chaque cas désigne une cellule (index)."""
    reread, similar, similar_excluded, retest = [], [], [], []
    seen: dict[str, list] = {}  # fichier -> (plage, cellule) lues depuis sa dernière écriture
    tests: dict[tuple, int] = {}  # clé de test -> cellule du dernier lancement
    py_cells: list[int] = []
    last_write = None  # dernière cellule qui écrit dans le workspace
    for index, cell in enumerate(session["cells"]):
        if cell["name"] == "apply_patch":
            last_write = index
            for p in patch_paths(cell["args"].get("input") or cell["args"].get("patch") or ""):
                seen.clear() if p == ALL else seen.pop(p, None)
            tests.clear()
            continue
        if cell["name"] != "python_repl":
            continue
        ops = cell_ops(cell["code"])
        if ops is None:
            continue
        counts = not cell["error"]
        reads_count = counts and TRUNCATED not in cell["result"]
        writes = False
        for op in ops:
            kind = op[0]
            if kind == "write":
                writes = writes or op[1] == ALL or in_workspace(op[1])
                seen.clear() if op[1] == ALL else seen.pop(op[1], None)
                tests.clear()
            elif kind == "workspace":
                writes = True
                tests.clear()
            elif kind == "read" and reads_count and op[3]:
                path, rng = op[1], op[2]
                earlier = seen.get(path, [])
                if covered([r for r, _ in earlier], rng) >= REREAD_MIN_COVERED:
                    reread.append({"cell": index, "path": path, "range": list(rng),
                                   "previous": sorted({c for _, c in earlier})})
                seen.setdefault(path, []).append((rng, index))
            elif kind == "test" and counts:
                if op[1] in tests:
                    retest.append({"cell": index, "previous": tests[op[1]],
                                   "command": list(op[1])})
                tests[op[1]] = index
        if writes:
            last_write = index
        words = WORD.findall(cell["code"])
        if len(words) >= SIMILAR_MIN_TOKENS:
            best = None
            for prev in py_cells[-SIMILAR_WINDOW:]:
                other = WORD.findall(session["cells"][prev]["code"])
                if len(other) < SIMILAR_MIN_TOKENS:
                    continue
                ratio = difflib.SequenceMatcher(None, other, words, autojunk=False).ratio()
                if ratio >= SIMILAR_RATIO and (best is None or ratio > best[1]):
                    best = (prev, ratio)
            if best:
                prev = session["cells"][best[0]]
                case = {"cell": index, "previous": best[0], "ratio": round(best[1], 3)}
                if prev["error"] or FAILED.search(prev["result"]):
                    similar_excluded.append({**case, "reason": "précédente en échec"})
                elif last_write is not None and last_write > best[0]:
                    similar_excluded.append({**case, "reason": "écriture depuis la précédente"})
                else:
                    similar.append(case)
        py_cells.append(index)
    return {"reread": reread, "similar": similar, "similar_excluded": similar_excluded,
            "retest": retest}


def covered(ranges: list, rng) -> float:
    """Part de `rng` déjà lue dans `ranges` (plages (a, b) ou ("tail", n))."""
    if not ranges:
        return 0.0
    whole = any(isinstance(r[0], int) and r[0] <= 1 and r[1] >= WHOLE_FILE_LINES
                for r in ranges)
    if rng[0] == "tail":
        if whole:
            return 1.0
        best = max((r[1] for r in ranges if r[0] == "tail"), default=0)
        return min(1.0, best / rng[1]) if rng[1] else 1.0
    a, b = rng
    lines = set()
    for r in ranges:
        if isinstance(r[0], int):
            lines.update(range(max(a, r[0]), min(b, r[1]) + 1))
    return len(lines) / (b - a + 1) if b >= a else 1.0


# ---------------------------------------------------------------- tokens et coût

def waste_cost(session: dict, cells: set[int], tokens, prices: dict) -> dict:
    """Tokens et coût à cache théorique des cellules `cells` (borne haute, estimation)."""
    calls = session["calls"]
    input_tokens = 0
    cost = 0.0
    priced = True
    for index in sorted(cells):
        cell = session["cells"][index]
        code_t, result_t = tokens(cell["code"]), tokens(cell["result"])
        k = cell["call"]
        later = calls[k + 1:]
        input_tokens += (code_t + result_t) * len(later)
        model = calls[k]["model"]
        out_t = (calls[k]["usage"].get("output") or 0) if calls[k]["tools"] == 1 else code_t
        rows = [metrics.price_row(prices, model, c["usage"])[0] for c in calls[k:]]
        if any(r is None for r in rows):
            priced = False
            continue
        cost += out_t * rows[0]["output"]
        for i, row in enumerate(rows[1:]):
            if i == 0:
                cost += result_t * row["input"] + code_t * row["cached_input"]
            else:
                cost += (code_t + result_t) * row["cached_input"]
    return {"input_tokens": input_tokens, "cost": cost / 1_000_000 if priced else None}


def find_sessions(arg: Path) -> list[tuple[Path, str, str]]:
    """(dossier du run, tâche, run-N) de chaque run de tâche."""
    runs = sorted((arg / "arms").iterdir()) if (arg / "arms").is_dir() else [arg]
    out = []
    for run in runs:
        for task_run in sorted(run.glob("tasks/*/run-*")):
            if (task_run / "sessions").is_dir():
                out.append((run, task_run.parent.name, task_run.name))
    return out


def report_cost(run: Path, task: str, run_name: str) -> float | None:
    path = run / "report.json"
    if not path.exists():
        return None
    number = int(run_name.split("-")[1])
    for r in json.loads(path.read_text()).get("results", []):
        if r.get("task") == task and r.get("run") == number:
            return (r.get("cost") or {}).get("theoretical_total")
    return None


KINDS = ("reread", "similar", "retest")
LABELS = {"all": "Union", "reread": "Relectures", "similar": "Presque identiques",
          "retest": "Tests relancés"}


def analyse_task_run(run: Path, task: str, run_name: str, version: int | None, tokens,
                     prices: dict) -> dict | None:
    task_dir = run / "tasks" / task / run_name
    session_paths = sorted((task_dir / "sessions").rglob("*.jsonl"))
    sessions = [load_session(p) for p in session_paths]
    if not sessions:
        return None
    main_version = prompt_version(sessions[0]["header"])
    if version is not None and main_version != version:
        return None
    records = metrics.read_ndjson(sorted((task_dir / "sessions").rglob("*.usage.ndjson")))
    prompt_total = sum(metrics.prompt_size(r) for r in records
                       if r.get("kind") in metrics.LOOP_KINDS)
    out = {"run": run.name, "task": task, "task_run": run_name, "prompt_version": main_version,
           "binary": (run.name.split("-") + ["", ""])[1], "sessions": len(sessions),
           "cells": 0, "reread": 0, "similar": 0, "similar_excluded": 0, "retest": 0,
           "prompt_tokens": prompt_total, "cost_theoretical": report_cost(run, task, run_name),
           "waste": {k: {"cells": 0, "input_tokens": 0 if tokens else None,
                         "cost": 0.0 if tokens else None} for k in ("all",) + KINDS},
           "cases": []}
    for session in sessions:
        found = detect(session)
        out["cells"] += sum(1 for c in session["cells"] if c["name"] == "python_repl")
        for key in KINDS + ("similar_excluded",):
            out[key] += len(found[key])
            out["cases"] += [{"kind": key, "session": Path(session["path"]).name, **c,
                              "code": session["cells"][c["cell"]]["code"][:300]}
                             for c in found[key]]
        groups = {k: {c["cell"] for c in found[k]} for k in KINDS}
        groups["all"] = set().union(*groups.values())
        for key, cells in groups.items():
            acc = out["waste"][key]
            acc["cells"] += len(cells)
            if tokens and cells:
                w = waste_cost(session, cells, tokens, prices)
                acc["input_tokens"] += w["input_tokens"]
                acc["cost"] = (None if w["cost"] is None or acc["cost"] is None
                               else acc["cost"] + w["cost"])
    return out


def share(num, den) -> str:
    if num is None or not den:
        return "n.d."
    return f"{100 * num / den:.1f} %"


def summed(group: list[dict], key: str, field: str):
    values = [r["waste"][key][field] for r in group]
    return None if any(v is None for v in values) else sum(values)


def denominators(group: list[dict]) -> tuple[int, float | None]:
    cost = [r["cost_theoretical"] for r in group]
    return (sum(r["prompt_tokens"] for r in group),
            None if any(c is None for c in cost) else sum(cost))


def render(rows: list[dict]) -> str:
    lines = ["| Tâche | Runs | Runs touchés | Cellules | Relectures | Presque identiques "
             "(écartées) | Tests relancés | Cellules en cause | Part tokens d'entrée "
             "| Part coût théorique |", "|---|---|---|---|---|---|---|---|---|---|"]
    by_task: dict[str, list[dict]] = {}
    for r in rows:
        by_task.setdefault(r["task"], []).append(r)

    def line(name: str, group: list[dict]) -> str:
        touched = sum(1 for r in group if r["waste"]["all"]["cells"])
        prompt, cost = denominators(group)
        return (f"| {name} | {len(group)} | {touched} | {sum(r['cells'] for r in group)} "
                f"| {sum(r['reread'] for r in group)} | {sum(r['similar'] for r in group)} "
                f"({sum(r['similar_excluded'] for r in group)}) "
                f"| {sum(r['retest'] for r in group)} "
                f"| {summed(group, 'all', 'cells')} "
                f"| {share(summed(group, 'all', 'input_tokens'), prompt)} "
                f"| {share(summed(group, 'all', 'cost'), cost)} |")

    for task in sorted(by_task):
        lines.append(line(task, by_task[task]))
    lines.append(line("**Total**", rows))
    prompt, cost = denominators(rows)
    lines += ["", "| Comportement (total) | Runs touchés | Cellules | Part tokens d'entrée "
              "| Part coût théorique |", "|---|---|---|---|---|"]
    for key in KINDS + ("all",):
        touched = sum(1 for r in rows if r["waste"][key]["cells"])
        lines.append(f"| {LABELS[key]} | {touched} | {summed(rows, key, 'cells')} "
                     f"| {share(summed(rows, key, 'input_tokens'), prompt)} "
                     f"| {share(summed(rows, key, 'cost'), cost)} |")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--prompt-version", type=int)
    parser.add_argument("--json", type=Path)
    parser.add_argument("--reread-covered", type=float, help="sensibilité : REREAD_MIN_COVERED")
    parser.add_argument("--similar-ratio", type=float, help="sensibilité : SIMILAR_RATIO")
    args = parser.parse_args(argv)
    global REREAD_MIN_COVERED, SIMILAR_RATIO
    if args.reread_covered is not None:
        REREAD_MIN_COVERED = args.reread_covered
    if args.similar_ratio is not None:
        SIMILAR_RATIO = args.similar_ratio
    try:
        import tiktoken
        enc = tiktoken.get_encoding("o200k_base")

        def tokens(text: str) -> int:
            return len(enc.encode(text, disallowed_special=()))
    except ImportError:
        tokens = None
        print("tiktoken absent : tokens et coût non calculés (n.d.)", file=sys.stderr)
    prices = json.loads((Path(metrics.__file__).parent / "prices.json").read_text())
    rows = []
    for arg in args.paths:
        for run, task, run_name in find_sessions(arg):
            row = analyse_task_run(run, task, run_name, args.prompt_version, tokens, prices)
            if row is not None:
                rows.append(row)
    binaries = sorted({r["binary"] for r in rows})
    print(f"Runs de tâche : {len(rows)} ; sessions : {sum(r['sessions'] for r in rows)} ; "
          f"binaires : {', '.join(binaries)} ; seuils : relecture {REREAD_MIN_COVERED}, "
          f"similarité {SIMILAR_RATIO} sur {SIMILAR_WINDOW} cellules")
    print()
    print(render(rows))
    if args.json:
        args.json.write_text(json.dumps(rows, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
