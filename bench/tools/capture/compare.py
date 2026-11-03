#!/usr/bin/env python3
"""Compare deux runs du banc de capture : chaque octet qui change dans le préfixe.

Usage : python3 -I compare.py <run A> <run B> [--out <fichier .md>]

Pour chaque cas présent dans les deux runs, compare la première requête :
prompt système, tools, paramètres (hors conversation) et premier message
utilisateur. Chaque bloc de lignes modifié est listé avec sa position en octets (UTF-8)
dans A et dans B, les octets retirés et les octets ajoutés (préfixe et suffixe
communs du bloc exclus). Les deux binaires
doivent être extraits au même chemin (voir README) : sinon un binaire antérieur
à T03 diffère aussi par le chemin des skills embarqués.

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import difflib
import json
from pathlib import Path


def compact(value) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def first_request(case_dir: Path) -> dict | None:
    bodies = sorted((case_dir / "cap").glob("*.body.json"))
    return json.loads(bodies[0].read_text()) if bodies else None


def conversation_key(body: dict) -> str:
    return "messages" if "messages" in body else "input"


def parts(body: dict) -> dict[str, str]:
    """Les morceaux comparés, en texte : le système, chaque clé hors conversation, le premier
    message utilisateur."""
    key = conversation_key(body)
    conversation = list(body.get(key) or [])
    out: dict[str, str] = {}
    if key == "messages":
        system = body.get("system") or []
        out["system (texte)"] = system if isinstance(system, str) else "\n".join(
            block.get("text", "") for block in system)
    elif conversation and conversation[0].get("role") in ("system", "developer"):
        content = conversation.pop(0).get("content")
        out["system (texte)"] = content if isinstance(content, str) else "".join(
            c.get("text", "") for c in content)
    for name, value in body.items():
        if name not in (key, "system"):
            out[f"{name} (JSON)"] = compact(value)
    first_user = next((m for m in conversation if m.get("role") == "user"), None)
    out["premier message utilisateur (JSON)"] = compact(first_user)
    return out


def byte_offset(text: str, index: int) -> int:
    return len(text[:index].encode())


def changes(a: str, b: str) -> list[dict]:
    """Une entrée par bloc de lignes modifié, réduite à l'écart entre le préfixe et le
    suffixe communs du bloc : les octets listés sont exactement ceux qui changent."""
    out = []
    lines_a, lines_b = a.splitlines(keepends=True), b.splitlines(keepends=True)
    start_a, start_b = [0], [0]
    for line in lines_a:
        start_a.append(start_a[-1] + len(line))
    for line in lines_b:
        start_b.append(start_b[-1] + len(line))
    matcher = difflib.SequenceMatcher(None, lines_a, lines_b, autojunk=False)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        chunk_a, chunk_b = "".join(lines_a[i1:i2]), "".join(lines_b[j1:j2])
        prefix = 0
        while prefix < min(len(chunk_a), len(chunk_b)) and chunk_a[prefix] == chunk_b[prefix]:
            prefix += 1
        suffix = 0
        while (suffix < min(len(chunk_a), len(chunk_b)) - prefix
               and chunk_a[-1 - suffix] == chunk_b[-1 - suffix]):
            suffix += 1
        removed = chunk_a[prefix:len(chunk_a) - suffix]
        added = chunk_b[prefix:len(chunk_b) - suffix]
        out.append({
            "octet_a": byte_offset(a, start_a[i1] + prefix),
            "octet_b": byte_offset(b, start_b[j1] + prefix),
            "retire": removed,
            "ajoute": added,
            "octets_retires": len(removed.encode()),
            "octets_ajoutes": len(added.encode()),
        })
    return out


def compare(run_a: Path, run_b: Path) -> dict:
    report = {"run_a": str(run_a), "run_b": str(run_b), "cases": {}}
    for meta_name, run in (("meta_a", run_a), ("meta_b", run_b)):
        info = json.loads((run / "run.json").read_text())
        report[meta_name] = {k: info.get(k) for k in
                             ("binary", "binary_sha256", "binary_version", "repo_commit",
                              "date_utc", "cache_retention")}
    names = sorted(p.name for p in (run_a / "cases").iterdir()
                   if (run_b / "cases" / p.name / "cap").is_dir())
    for name in names:
        body_a = first_request(run_a / "cases" / name)
        body_b = first_request(run_b / "cases" / name)
        if body_a is None or body_b is None:
            continue
        parts_a, parts_b = parts(body_a), parts(body_b)
        case = {}
        for part in list(parts_a) + [p for p in parts_b if p not in parts_a]:
            text_a, text_b = parts_a.get(part, ""), parts_b.get(part, "")
            case[part] = {
                "octets_a": len(text_a.encode()),
                "octets_b": len(text_b.encode()),
                "identique": text_a == text_b,
                "changements": changes(text_a, text_b),
            }
        report["cases"][name] = case
    return report


def render(report: dict) -> str:
    out = ["# Comparaison de deux runs du banc de capture", ""]
    for label, key in (("A", "meta_a"), ("B", "meta_b")):
        meta = report[key]
        out.append(f"- {label} : `{report['run_' + label.lower()]}`, binaire "
                   f"`{meta.get('binary_version')}` (sha256 `{(meta.get('binary_sha256') or '')[:16]}…`), "
                   f"commit du dépôt `{meta.get('repo_commit')}`, "
                   f"TTL `{compact(meta.get('cache_retention'))}`")
    out.append("")
    for name, case in report["cases"].items():
        out += [f"## {name}", "", "| Morceau | Octets A | Octets B | Écart | Identique | Changements |",
                "|---|---|---|---|---|---|"]
        for part, data in case.items():
            out.append(f"| {part} | {data['octets_a']} | {data['octets_b']} | "
                       f"{data['octets_b'] - data['octets_a']:+d} | {data['identique']} | "
                       f"{len(data['changements'])} |")
        out.append("")
        for part, data in case.items():
            if not data["changements"]:
                continue
            out += [f"### {name} : {part}", ""]
            for change in data["changements"]:
                out.append(f"- octet {change['octet_a']} (A) / {change['octet_b']} (B) : "
                           f"−{change['octets_retires']} +{change['octets_ajoutes']}")
                if change["retire"]:
                    out.append(f"  - retiré : `{json.dumps(change['retire'], ensure_ascii=False)}`")
                if change["ajoute"]:
                    out.append(f"  - ajouté : `{json.dumps(change['ajoute'], ensure_ascii=False)}`")
            out.append("")
    return "\n".join(out) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("run_a", type=Path)
    parser.add_argument("run_b", type=Path)
    parser.add_argument("--out", type=Path, help="rapport Markdown (sinon sortie standard)")
    args = parser.parse_args()
    report = compare(args.run_a, args.run_b)
    text = render(report)
    if args.out:
        args.out.write_text(text)
        args.out.with_suffix(".json").write_text(
            json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
