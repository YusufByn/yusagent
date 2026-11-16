#!/usr/bin/env python3
"""Appels à `edit()`, échecs et écritures de fichiers hors `edit()` dans les sessions d'un run.

Usage, depuis la racine du dépôt :

    python3 -I bench/smoke/tools/edits.py bench/smoke/runs/<run>

Pour chaque tâche (`<run>/tasks/<tâche>/run-1/sessions/*.jsonl`, session principale et
éventuels sous-agents) : nombre de cellules `python_repl`, appels à `edit()` (motif `EDIT_CALL`
de `bench/smoke/metrics.py`), cellules avec `edit()` en erreur et leur message, et les lignes de
code qui écrivent un fichier sans `edit()` : `open(...)` en écriture, `write_text` /
`write_bytes`, ou dans un `bash(...)` une redirection `>`, `tee`, `sed -i` ou un heredoc. Les
écritures sont repérées par motif sur le code : à relire, une ligne signalée n'est pas toujours
une écriture (texte d'un `old_str`, par exemple), et un formateur lancé par `bash()`
(`prettier --write`) n'est pas repéré.

Écrit pour la vérification des chauffes de la PR B du ticket 11 (YA-09). Lecture seule.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

EDIT_CALL = re.compile(r"(?<![\w.])edit(?:\.run)?\s*\(")
WRITES = {
    "open(..., 'w'/'a')": re.compile(r"open\([^)]*['\"][wa]b?\+?['\"]"),
    "write_text/write_bytes": re.compile(r"\.write_(?:text|bytes)\("),
    "shell >/tee/sed -i/heredoc": re.compile(
        r"bash\([^)]*?(?:[^2&]>\s*[\w./'\"$]|\btee\b|sed\s+-i|<<)", re.S
    ),
}


def line_of(code: str, start: int, end: int) -> str:
    first = code.rfind("\n", 0, start) + 1
    last = code.find("\n", end)
    return code[first : last if last >= 0 else None].strip()


def session_report(path: Path) -> tuple[int, int, list[str], list[str]]:
    calls: dict[str, str] = {}
    results: dict[str, dict] = {}
    for line in path.read_text().splitlines():
        entry = json.loads(line)
        if entry.get("type") != "message":
            continue
        message = entry["message"]
        if message["role"] == "assistant":
            for block in message.get("content", []):
                if block.get("type") == "toolCall" and block.get("name") == "python_repl":
                    calls[block["id"]] = block.get("arguments", {}).get("code", "")
        elif message["role"] == "toolResult":
            results[message.get("toolCallId")] = message.get("details", {}) or {}
    edits = 0
    failures: list[str] = []
    writes: list[str] = []
    for call_id, code in calls.items():
        count = len(EDIT_CALL.findall(code))
        edits += count
        details = results.get(call_id, {})
        if count and details.get("status") == "error":
            error = details.get("error") or {}
            failures.append(f"{error.get('ename')}: {str(error.get('evalue'))[:160]}")
        for label, pattern in WRITES.items():
            for hit in pattern.finditer(code):
                writes.append(f"{label}: {line_of(code, hit.start(), hit.end())[:140]}")
    return len(calls), edits, failures, writes


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("run", type=Path, help="dossier d'un run du banc (bench/smoke/runs/<run>)")
    args = parser.parse_args()
    for task_dir in sorted((args.run / "tasks").iterdir()):
        for path in sorted(task_dir.glob("run-*/sessions/*.jsonl")):
            cells, edits, failures, writes = session_report(path)
            print(
                f"== {task_dir.name} ({path.name}) : cellules {cells}, appels edit() {edits}, "
                f"cellules avec edit() en erreur {len(failures)}"
            )
            for failure in failures:
                print("   échec :", failure)
            for write in writes:
                print("   écriture hors edit() :", write)


if __name__ == "__main__":
    main()
