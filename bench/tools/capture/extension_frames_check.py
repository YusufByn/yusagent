#!/usr/bin/env python3
"""Trames de l'extension VS Code contre le cas `acp` (YA-10, ticket 24, PR 2 ; preuve 2 du préfixe).

Usage : python3 -I extension_frames_check.py <run>   (écrit <run>/extension-frames-report.md et .json)

Le cas `acp-extension` de `run.py` rejoue le fixture des trames de l'extension
(`extensions/vscode/test/fixtures/acp-frames.json` : `initialize` avec les capacités de
l'extension, `session/new` avec `mcpServers: []`, prompt texte) contre le binaire, au même
scénario `basic` et avec le même prompt que le cas `acp`. Chaque requête au modèle doit être
identique à celle du cas `acp`, corps entier, UUID normalisés (`compare.all_requests`).

Vérifie aussi que les trames envoyées par le cas sont exactement celles du fixture.
Code de sortie 1 si un dossier manque, si le nombre de requêtes diffère ou si une requête
diffère. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from compare import all_requests, changes  # noqa: E402
from run import EXTENSION_FIXTURE, fixture_frames  # noqa: E402


def sent_frames(case_dir: Path) -> list[dict]:
    transcript = case_dir / "transcript.jsonl"
    if not transcript.is_file():
        return []
    return [entry["msg"] for entry in map(json.loads, transcript.read_text().splitlines())
            if entry["dir"] == "client->agent"]


def check(run: Path) -> dict:
    acp = all_requests(run / "acp")
    extension = all_requests(run / "acp-extension")
    run_info = json.loads((run / "run.json").read_text()) if (run / "run.json").is_file() else {}
    project = Path(run_info.get("project", "/private/tmp/ya-capture/project"))
    expected = fixture_frames(EXTENSION_FIXTURE, project)
    sent = sent_frames(run / "acp-extension")
    # Ticket 40 (X5) : la trame du prompt par sa méthode (4 trames depuis `_yus/providers/list`).
    prompt = next((frame for frame in sent if frame.get("method") == "session/prompt"), {})
    session_id = (prompt.get("params") or {}).get("sessionId")
    for frame in expected:
        if frame["method"] == "session/prompt":
            frame["params"]["sessionId"] = session_id
    differing = []
    for index, (a, b) in enumerate(zip(acp, extension), start=1):
        if a != b:
            differing.append({"requete": index, "changements": changes(
                json.dumps(json.loads(a), indent=1, ensure_ascii=False),
                json.dumps(json.loads(b), indent=1, ensure_ascii=False))})
    frames_ok = sent == expected and session_id is not None
    ok = bool(acp) and len(acp) == len(extension) and not differing and frames_ok
    return {
        "run": str(run),
        "fixture": str(EXTENSION_FIXTURE),
        "trames_du_fixture": frames_ok,
        "requetes_acp": len(acp),
        "requetes_extension": len(extension),
        "identiques": sum(a == b for a, b in zip(acp, extension)),
        "differentes": differing,
        "verdict": "TOUT IDENTIQUE" if ok else "DIFFÉRENCES",
    }


def render(report: dict) -> str:
    lines = [
        "# Trames de l'extension contre `acp` (ticket 24, PR 2)",
        "",
        f"- run : `{report['run']}`",
        f"- fixture : `{report['fixture']}`",
        f"- trames envoyées = fixture : {'oui' if report['trames_du_fixture'] else 'NON'}",
        "",
        "| chemin | requêtes | identiques à `acp` | différentes |",
        "|---|---|---|---|",
        f"| acp | {report['requetes_acp']} | | |",
        f"| acp-extension | {report['requetes_extension']} | {report['identiques']} "
        f"| {len(report['differentes'])} |",
        "",
    ]
    for diff in report["differentes"]:
        lines.append(f"## Requête {diff['requete']}")
        for change in diff["changements"]:
            lines.append(f"- retiré : `{change['retire'][:300]}` ; ajouté : `{change['ajoute'][:300]}`")
        lines.append("")
    lines.append(report["verdict"])
    return "\n".join(lines) + "\n"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__.splitlines()[2])
    run = Path(sys.argv[1]).resolve()
    report = check(run)
    (run / "extension-frames-report.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    text = render(report)
    (run / "extension-frames-report.md").write_text(text)
    print(text, end="")
    sys.exit(0 if report["verdict"] == "TOUT IDENTIQUE" else 1)


if __name__ == "__main__":
    main()
