#!/usr/bin/env python3
"""Données d'affichage des cellules, cas `display-anthropic` (YA-10, ticket 24).

Usage : python3 -I display_check.py <run>   (écrit <run>/display-report.md)

À lancer sur un run d'un binaire qui porte le ticket 24 (un binaire antérieur n'a pas ces
champs : NON CONFORME attendu). Lit le fichier de session du cas, copié par `run.py` dans
`cases/display-anthropic/sessions/`, et vérifie :
  - `details.bashRuns` : une seule entrée dans toute la session, celle du `bash()` lu
    (`echo lu`, code de sortie 0, sortie `lu`) ; rien pour le `bash()` non lu ;
  - `details.diffs` : les deux `edit()` du scénario, chacun avec `oldStr`, `newStr` et
    `startLine` (création : `oldStr` vide) ; ticket 26 : `startColumn` 0 sur chacun (création, et
    `un` en tête de la ligne 1). Un binaire d'avant le ticket 26 : NON CONFORME sur ce point ;
  - la dernière requête capturée est le tour de réveil de l'avis `[bash-done …]` de la
    commande non lue.

Refuse (code de sortie 1, sans rapport) un dossier qui contient plus d'une session : un dossier de
run réutilisé mélangerait deux runs. Code de sortie 1 si une vérification échoue. Bibliothèque
standard uniquement.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

CASE = Path("cases") / "display-anthropic"


def details(run: Path) -> list[dict]:
    out = []
    for path in sorted((run / CASE / "sessions").rglob("*.jsonl")):
        for line in path.read_text().splitlines():
            entry = json.loads(line)
            message = entry.get("message")
            if isinstance(message, dict) and isinstance(message.get("details"), dict):
                out.append(message["details"])
    return out


def main() -> None:
    run = Path(sys.argv[1])
    # Une seule session : un dossier de run réutilisé en contiendrait plusieurs.
    sessions = sorted((run / CASE / "sessions").rglob("*.jsonl"))
    if len(sessions) > 1:
        print(f"NON CONFORME : {len(sessions)} sessions dans {run / CASE / 'sessions'}, une "
              "seule attendue (dossier de run réutilisé ?) : supprimer le run et le relancer")
        sys.exit(1)
    found = details(run)
    runs = [item for d in found for item in d.get("bashRuns", [])]
    diffs = [item for d in found for item in d.get("diffs", [])]
    bodies = sorted((run / CASE / "cap").glob("*.body.json"))
    last = bodies[-1].read_text() if bodies else ""
    checks = [
        ("fichier de session lu", bool(found)),
        ("une seule entrée bashRuns", len(runs) == 1),
        ("entrée du bash() lu (echo lu, 0, lu)",
         len(runs) == 1 and runs[0].get("command") == "echo lu"
         and runs[0].get("exitCode") == 0 and runs[0].get("output", "").strip() == "lu"),
        ("deux diffs", len(diffs) == 2),
        ("oldStr, newStr et startLine sur chaque diff",
         len(diffs) == 2 and all({"oldStr", "newStr", "startLine"} <= set(d) for d in diffs)),
        ("startColumn 0 sur chaque diff (ticket 26)",
         len(diffs) == 2 and all(d.get("startColumn") == 0 for d in diffs)),
        ("création puis modification",
         len(diffs) == 2 and diffs[0].get("oldStr") == "" and diffs[0].get("newStr") == "un\n"
         and diffs[1].get("oldStr") == "un" and diffs[1].get("newStr") == "deux"),
        ("dernière requête : avis [bash-done] de la commande non lue",
         "[bash-done pid:" in last and "sleep 3; echo non-lu" in last),
    ]
    ok = all(passed for _, passed in checks)
    lines = ["# Données d'affichage des cellules (ticket 24)", "",
             f"Run : `{run}`", "", "| Vérification | Résultat |", "|---|---|"]
    lines += [f"| {name} | {'oui' if passed else 'NON'} |" for name, passed in checks]
    lines += ["", f"- bashRuns : `{json.dumps(runs, ensure_ascii=False)}`",
              f"- diffs : `{json.dumps([{k: d.get(k) for k in ('oldStr', 'newStr', 'startLine', 'startColumn')} for d in diffs], ensure_ascii=False)}`",
              "", "CONFORME" if ok else "NON CONFORME"]
    (run / "display-report.md").write_text("\n".join(lines) + "\n")
    for name, passed in checks:
        print(f"{'oui' if passed else 'NON'} : {name}")
    print(lines[-1])
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
