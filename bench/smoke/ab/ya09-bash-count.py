#!/usr/bin/env python3
"""Tokens du prompt système et des tools : yusAgent (corps de requête capturé) et Unharnessed.

Usage, depuis la racine du dépôt (tiktoken n'est pas une dépendance du dépôt, `uv` l'installe
dans un environnement jetable) :

    uv run --no-project --with tiktoken==0.14.0 python -I bench/smoke/ab/ya09-bash-count.py \\
        --body bench/smoke/runs/20261115T183135Z-c770c5a-gpt-6-luna/tasks/py-shortuuid-118/run-1/proxy-bodies.ndjson \\
        [--usage bench/smoke/runs/20261115T183135Z-c770c5a-gpt-6-luna/tasks/py-shortuuid-118/run-1/sessions/<id>.usage.ndjson] \\
        [--unharnessed <clone de github.com/unharnessed-agent/Unharnessed>]

`--body` : un corps capturé (`capture/NNNN.body.json`) ou un journal `--log-bodies`
(`proxy-bodies.ndjson`, dont on prend la première requête).

**Tokenizer supposé** : `o200k_base`. Le tokenizer de `gpt-6-luna` n'est pas publié ; ces
comptes sont des estimations.

`--usage` (facultatif) : journal d'usage de la même session. Le script y lit, pour la première
requête, l'attribution que le backend Codex renvoie par champ de requête
(`undocumentedAttribution.request_fields`, `instructions` et `tools`,
`input_tokens`). C'est un champ **non documenté** : il sert de contrôle du compte local, et il
est absent des providers autres que Codex.

yusAgent : `instructions` du corps (le prompt système), découpé aux titres Markdown, et chaque
tool tel qu'envoyé (JSON compact, `ensure_ascii=False`). Instructions et tools sont comptés
séparément.

Unharnessed (facultatif) : le prompt système du golden de leurs tests
(`crates/uh-core/tests/golden/corpus/system-prompt.json`, valeurs de fixture pour la date, le
cwd et le journal), et le tool `ipython` rebâti avec la forme de notre `python_repl` sur le fil
(type, `strict`) et le nom, la description et le schéma de `crates/uh-core/src/tools/ipython.rs`.

Lecture seule.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

import tiktoken

ENCODING = "o200k_base"


def count(enc: tiktoken.Encoding, text: str) -> int:
    return len(enc.encode(text))


def compact(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def first_body(path: Path) -> dict:
    if path.suffix == ".ndjson":
        first = next(line for line in path.read_text().splitlines() if line.strip())
        return json.loads(first)["body"]
    return json.loads(path.read_text())


def yusagent(enc: tiktoken.Encoding, body_path: Path) -> dict:
    body = first_body(body_path)
    instructions = body["instructions"]
    heads = [(m.start(), m.group()) for m in re.finditer(r"^#+ .*$", instructions, re.M)]
    print(f"## yusAgent : {body_path}")
    print(f"- modèle {body.get('model')} ; instructions : {len(instructions)} caractères, "
          f"**{count(enc, instructions)} tokens**")
    ends = [h[0] for h in heads[1:]] + [len(instructions)]
    if heads and heads[0][0] > 0:
        print(f"  - {count(enc, instructions[:heads[0][0]]):>5}  (avant le premier titre)")
    for (start, title), end in zip(heads, ends):
        print(f"  - {count(enc, instructions[start:end]):>5}  {title}")
    tools = body.get("tools") or []
    total = 0
    for tool in tools:
        n = count(enc, compact(tool))
        total += n
        print(f"- tool `{tool.get('name')}` : {n} tokens")
    print(f"- **tools : {total} tokens** ; instructions + tools : {count(enc, instructions) + total}")
    return {"instructions": count(enc, instructions), "tools": total, "template": tools[0] if tools else None}


def provider_attribution(usage: Path) -> None:
    first = json.loads(next(line for line in usage.read_text().splitlines() if line.strip()))
    fields = (first.get("undocumentedAttribution") or {}).get("request_fields") or {}
    print(f"- attribution du provider (champ non documenté), première requête de `{usage.name}` : "
          f"modèle {first.get('model')}, entrée totale {first.get('input')} ; "
          + (" ; ".join(f"{name} {value.get('input_tokens')}" for name, value in fields.items())
             or "absente"))


def rust_str(source: str, function: str) -> str:
    match = re.search(rf"fn {function}\(\)[^{{]*\{{\s*\"((?:[^\"\\]|\\.)*)\"", source)
    if not match:
        raise SystemExit(f"{function} introuvable")
    return match.group(1)


def unharnessed(enc: tiktoken.Encoding, clone: Path, template: dict | None) -> None:
    head = subprocess.run(["git", "-C", str(clone), "rev-parse", "--short", "HEAD"],
                          capture_output=True, text=True, check=False).stdout.strip()
    golden = json.loads((clone / "crates/uh-core/tests/golden/corpus/system-prompt.json").read_text())
    prompt = golden["systemPrompt"]
    print(f"\n## Unharnessed : {clone} (HEAD {head or '?'})")
    print(f"- prompt système (golden) : {len(prompt)} caractères, **{count(enc, prompt)} tokens**")
    core = (clone / "crates/uh-core/src/prompts/layers/core.md").read_text().strip()
    print(f"  - dont `core.md` : {count(enc, core)} tokens")
    source = (clone / "crates/uh-core/src/tools/ipython.rs").read_text()
    description = rust_str(source, "ipython_tool_description")
    code = re.search(r'"code":\s*\{[^}]*"description":\s*"((?:[^"\\]|\\.)*)"', source)
    tool = {**(template or {"type": "function", "strict": False}),
            "name": "ipython", "description": description,
            "parameters": {"type": "object", "required": ["code"],
                           "properties": {"code": {"type": "string",
                                                   "description": code.group(1) if code else ""}}}}
    print(f"- **tool `ipython` : {count(enc, compact(tool))} tokens** (forme de notre "
          f"`python_repl` sur le fil)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--body", type=Path, required=True,
                        help="corps capturé (capture/NNNN.body.json) ou proxy-bodies.ndjson")
    parser.add_argument("--usage", type=Path, help="journal d'usage de la session (*.usage.ndjson)")
    parser.add_argument("--unharnessed", type=Path, help="clone du dépôt Unharnessed")
    args = parser.parse_args()
    enc = tiktoken.get_encoding(ENCODING)
    print(f"Tokenizer : {ENCODING} (supposé, estimation) ; tiktoken {tiktoken.__version__}\n")
    ours = yusagent(enc, args.body)
    if args.usage:
        provider_attribution(args.usage)
    if args.unharnessed:
        unharnessed(enc, args.unharnessed, ours["template"])


if __name__ == "__main__":
    main()
