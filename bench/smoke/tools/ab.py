#!/usr/bin/env python3
"""A/B entrelacé par tâche du smoke bench (YA-14) : remplace `ab-masking.sh`.

Usage :
    python3 -I bench/smoke/tools/ab.py <dossier du binaire> <sha de référence> \\
        --a bench/smoke/settings/default.json --b bench/smoke/settings/<x>.json \\
        [--tasks id1,id2] [--passes 3] [--seed N] [--model gpt-6.1-terra] [--effort high] \\
        [--idle-timeout 300] [--capture] [--dry-run]

Appels réels au modèle (CLAUDE.md règle 6) : lancé par Yusuf, sauf `--capture` (serveur de
capture, aucun modèle) et `--dry-run` (contrôles et plan seuls, rien n'est lancé).

Contrôles (repris de `ab-masking.sh`) : binaire présent et exécutable, `--version` au sha du
manifeste, sha256 de l'exécutable égal à celui du manifeste, dossier qui porte ce sha ; sha
de référence connu de git et sources livrées identiques entre le sha du binaire et lui
(`git diff --quiet <sha> <ref> -- <SHIPPED>`). Réglages : l'écart entre A et B (clés à plat)
est quelconque, affiché, noté dans `ab.json`, `ab.md` et l'en-tête de chaque rapport
(`header.ab`) ; A = B est accepté (A/A, vérification du banc).

Déroulé : pour chaque passe, chaque tâche (ordre de `--tasks`) lance ses deux bras dos à
dos, A puis B ou B puis A, ordre tiré au sort (`random.Random(seed)`, un tirage par tâche et
par passe, graine notée ; tirée de `os.urandom` si absente). Chaque bras est un appel séparé
à `run.py` (`--runs 1 --tasks <tâche>`) : préparation (en cache), sonde, chauffe (un appel
au modèle), puis la tâche. Le surcoût par lancement (durée de l'appel à `run.py` moins la
durée de la tâche, `header.timing_s.tasks`) est mesuré, noté par bras et résumé dans `ab.md`.

Sortie : `runs/ab-<horodatage UTC>/` : `ab.json` (plan, graine, réglages, bras et dossiers
de run), `ab.md`, `log.txt`, et les runs dans `arms/`. Statistiques :
`python3 -I bench/smoke/tools/ab_stats.py --plan runs/ab-<horodatage>/ab.json`.
Variables fixées : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`.
Deux lancements de suite en échec (code non nul de `run.py`) arrêtent l'A/B.

Bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import statistics
import subprocess
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
SMOKE = HERE.parent
REPO = SMOKE.parents[1]
RUN_PY = SMOKE / "run.py"
# Sources livrées : ce que le binaire embarque (chemins de changelog-fragment.yml, plus Cargo.*).
SHIPPED = ("crates", "prime-agent-runtime", "skills", "vendor", "install-rust.sh", "Cargo.toml",
           "Cargo.lock")
ENV = {"DO_NOT_TRACK": "1", "PI_OFFLINE": "1", "PI_CACHE_RETENTION": "short"}
MAX_FAILED_LAUNCHES = 2
REPORT_LINE = re.compile(r"^rapport : (.+)/report\.md$", re.M)


def fail(message: str) -> None:
    sys.exit(f"ab.py : {message}")


def git(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check_binary(bin_dir: Path) -> str:
    """Contrôles du binaire ; rend le sha du manifeste."""
    binary = bin_dir / "prime-agent"
    if not (binary.is_file() and os.access(binary, os.X_OK)):
        fail(f"binaire absent : {binary}")
    manifest = json.loads((bin_dir / "manifest.json").read_text())
    sha = manifest["commit"]
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={**os.environ, **ENV, "PI_SKIP_VERSION_CHECK": "1"})
    if sha not in version.stdout + version.stderr:
        fail(f"version du binaire ≠ manifeste {sha}")
    if sha256(binary) != manifest["binaries"][0]["executableSha256"]:
        fail("sha256 de l'exécutable ≠ manifeste")
    if sha not in str(bin_dir):
        fail(f"le dossier ne porte pas le sha {sha}")
    return sha


def check_ref(sha: str, ref: str) -> str:
    """Sha de référence connu et sources livrées identiques ; rend la note du rapport."""
    if git("cat-file", "-e", f"{ref}^{{commit}}").returncode != 0:
        fail(f"sha de référence inconnu : {ref}")
    if git("diff", "--quiet", sha, ref, "--", *SHIPPED).returncode != 0:
        fail(f"sources livrées différentes entre {sha} et {ref}")
    return (f"Binaire {sha[:7]} : sources livrées identiques à {ref[:7]} "
            f"(git diff {sha} {ref} -- {' '.join(SHIPPED)} vide).")


def flat(d: dict, prefix: str = "") -> dict:
    out = {}
    for k, v in d.items():
        out.update(flat(v, prefix + k + ".") if isinstance(v, dict) else {prefix + k: v})
    return out


def settings_diff(a: dict, b: dict) -> dict[str, list]:
    """Écart à plat entre les réglages A et B : {clé: [valeur A, valeur B]} (absente : null)."""
    fa, fb = flat(a), flat(b)
    return {k: [fa.get(k), fb.get(k)] for k in sorted(fa.keys() | fb.keys())
            if fa.get(k) != fb.get(k)}


def bench_tasks() -> list[str]:
    """Tâches du banc : toutes celles de `tasks/` sauf les exemples."""
    found = []
    for toml in sorted((SMOKE / "tasks").glob("*/task.toml")):
        task = tomllib.loads(toml.read_text())
        if "exemple" not in task.get("tags", []):
            found.append(task["id"])
    return found


def plan(tasks: list[str], passes: int, seed: int) -> list[dict]:
    """Ordre des bras : un tirage par (passe, tâche), dans cet ordre, avec `Random(seed)`."""
    rng = random.Random(seed)
    units = []
    for p in range(1, passes + 1):
        for task in tasks:
            order = "AB" if rng.random() < 0.5 else "BA"
            units.append({"pass": p, "task": task, "order": order})
    return units


def rel(path: Path) -> str:
    return str(path.relative_to(REPO)) if path.is_relative_to(REPO) else str(path)


def launch(args, binary: Path, settings: Path, task: str, out: Path, note: dict,
           log) -> dict:
    """Un bras : un appel à run.py ; durée, code, dossier de run, surcoût."""
    cmd = [sys.executable, "-I", str(RUN_PY), "--binary", str(binary), "--provider",
           "openai-codex", "--model", args.model, "--effort", args.effort, "--settings",
           str(settings), "--runs", "1", "--tasks", task, "--out", str(out),
           "--idle-timeout", str(args.idle_timeout), "--ab", json.dumps(note, ensure_ascii=False)]
    if args.capture:
        cmd.append("--capture")
    started = time.monotonic()
    proc = subprocess.Popen(cmd, cwd=REPO, env={**os.environ, **ENV}, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True)
    output = []
    for line in proc.stdout:
        output.append(line)
        log.write(line)
        log.flush()
    code = proc.wait()
    wall = round(time.monotonic() - started, 2)
    found = REPORT_LINE.findall("".join(output))
    run_dir = Path(found[-1]) if found else None
    result = {"exit": code, "wall_s": wall, "run_dir": rel(run_dir) if run_dir else None,
              "timing_s": None, "overhead_s": None, "status": None}
    if run_dir is not None and (run_dir / "report.json").exists():
        report = json.loads((run_dir / "report.json").read_text())
        timing = report["header"].get("timing_s") or {}
        result["timing_s"] = timing
        if timing.get("tasks") is not None:
            result["overhead_s"] = round(wall - timing["tasks"], 2)
        result["status"] = [r["status"] for r in report["results"]]
    return result


def overhead_summary(arms: list[dict]) -> dict:
    values = [a["overhead_s"] for a in arms if a.get("overhead_s") is not None]
    walls = [a["wall_s"] for a in arms if a.get("wall_s") is not None]
    if not values:
        return {"launches": len(arms), "measured": 0}
    return {"launches": len(arms), "measured": len(values),
            "overhead_median_s": statistics.median(values),
            "overhead_min_s": min(values), "overhead_max_s": max(values),
            "overhead_total_s": round(sum(values), 2), "wall_total_s": round(sum(walls), 2),
            "overhead_share": round(sum(values) / sum(walls), 4) if sum(walls) else None}


def write_md(path: Path, ab: dict) -> None:
    o = ab.get("overhead") or {}
    lines = [f"# A/B entrelacé — {ab['started']}", "",
             f"- Binaire : `{ab['binary_sha']}` ; {ab['shipped_note'] or 'sans sha de référence'}",
             f"- Modèle {ab['model']}, effort {ab['effort']}, mode {ab['mode']}, garde-fou "
             f"d'inactivité {ab['idle_timeout_s']} s",
             f"- A : `{ab['settings']['A']['file']}` {json.dumps(ab['settings']['A']['value'])}",
             f"- B : `{ab['settings']['B']['file']}` {json.dumps(ab['settings']['B']['value'])}",
             "- Écart de réglages (A → B) : " + (", ".join(
                 f"`{k}` {json.dumps(v[0])} → {json.dumps(v[1])}"
                 for k, v in ab["settings_diff"].items()) or "aucun (A/A)"),
             f"- Tâches ({len(ab['tasks'])}) : {', '.join(ab['tasks'])} ; passes : {ab['passes']}",
             f"- Graine de l'ordre des bras : `{ab['seed']}` ({ab['seed_source']})",
             "", "## Surcoût par lancement", "",
             "Chaque bras est un appel séparé à `run.py` : préparation (en cache), sonde, "
             "chauffe (un appel au modèle), rapport. Surcoût = durée de l'appel − durée de la "
             "tâche (`timing_s.tasks`).", ""]
    if o.get("measured"):
        lines.append(f"- {o['measured']} lancement(s) mesuré(s) sur {o['launches']} : médiane "
                     f"{o['overhead_median_s']:.1f} s (min {o['overhead_min_s']:.1f}, max "
                     f"{o['overhead_max_s']:.1f}), total {o['overhead_total_s']:.1f} s sur "
                     f"{o['wall_total_s']:.1f} s ({(o['overhead_share'] or 0) * 100:.1f} %)")
    else:
        lines.append("- aucun lancement mesuré")
    lines += ["", "## Bras", "", "| Passe | Tâche | Ordre | Bras | Statut | Durée s | Surcoût s |"
              " Run |", "|---|---|---|---|---|---|---|---|"]
    for arm in ab["arms"]:
        status = ", ".join(arm.get("status") or []) or f"échec (code {arm['exit']})"
        run = Path(arm["run_dir"]).name if arm.get("run_dir") else "-"
        lines.append(f"| {arm['pass']} | {arm['task']} | {arm['order']} | {arm['arm']} | "
                     f"{status} | {arm['wall_s']} | {arm.get('overhead_s')} | `{run}` |")
    lines += ["", "Statistiques : `python3 -I bench/smoke/tools/ab_stats.py --plan "
              f"{ab['dir']}/ab.json [--baseline bench/smoke/baseline/{ab['model']}.json]`", ""]
    path.write_text("\n".join(lines))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("bin_dir", type=Path, help="dossier du binaire (prime-agent, manifest.json)")
    parser.add_argument("ref", help="sha de référence (sources livrées identiques au binaire)")
    parser.add_argument("--a", type=Path, default=SMOKE / "settings" / "default.json",
                        help="réglages du bras A")
    parser.add_argument("--b", type=Path, required=True, help="réglages du bras B")
    parser.add_argument("--tasks", default="", help="ids séparés par des virgules (défaut : "
                                                    "les tâches du banc, sans les exemples)")
    parser.add_argument("--passes", type=int, default=3)
    parser.add_argument("--seed", type=int, default=None)
    parser.add_argument("--model", default="gpt-6.1-terra")
    parser.add_argument("--effort", default="high")
    parser.add_argument("--idle-timeout", type=int, default=300)
    parser.add_argument("--out", type=Path, default=SMOKE / "runs")
    parser.add_argument("--capture", action="store_true",
                        help="serveur de capture en amont (aucun modèle)")
    parser.add_argument("--dry-run", action="store_true", help="contrôles et plan seuls")
    args = parser.parse_args()
    if args.passes < 1:
        fail("--passes doit valoir au moins 1")
    bin_dir = args.bin_dir.expanduser().resolve()
    sha = check_binary(bin_dir)
    note = check_ref(sha, args.ref)
    settings = {side: {"file": rel(path.resolve()), "value": json.loads(path.read_text())}
                for side, path in (("A", args.a), ("B", args.b))}
    diff = settings_diff(settings["A"]["value"], settings["B"]["value"])
    tasks = [t for t in args.tasks.split(",") if t] or bench_tasks()
    known = {p.name for p in (SMOKE / "tasks").iterdir() if p.is_dir()}
    unknown = [t for t in tasks if t not in known]
    if unknown:
        fail(f"tâches inconnues : {unknown}")
    seed_source = "--seed"
    if args.seed is None:
        args.seed, seed_source = int.from_bytes(os.urandom(4), "big"), "os.urandom"
    units = plan(tasks, args.passes, args.seed)
    started = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_dir = args.out.resolve() / f"ab-{started}"
    ab = {"started": started, "dir": rel(out_dir), "mode": "capture" if args.capture else "réel",
          "binary": str(bin_dir / "prime-agent"), "binary_sha": sha, "ref": args.ref,
          "shipped_note": note, "model": args.model, "effort": args.effort,
          "idle_timeout_s": args.idle_timeout, "settings": settings, "settings_diff": diff,
          "tasks": tasks, "passes": args.passes, "seed": args.seed, "seed_source": seed_source,
          "commit": git("rev-parse", "HEAD").stdout.strip(),
          "commit_dirty": bool(git("status", "--porcelain").stdout.strip()),
          "units": units, "arms": []}
    print(note)
    print("écart de réglages (A → B) : " + (json.dumps(diff, ensure_ascii=False) if diff
                                           else "aucun (A/A)"))
    print(f"graine {args.seed} ({seed_source}) ; ordre : "
          + " ".join(f"{u['pass']}:{u['task']}:{u['order']}" for u in units))
    if args.dry_run:
        return
    out_dir.mkdir(parents=True)
    arms_dir = out_dir / "arms"
    failed = 0
    with (out_dir / "log.txt").open("w") as log:
        log.write(f"{note}\nécart {json.dumps(diff, ensure_ascii=False)}\ngraine {args.seed}\n")
        for unit in units:
            for position, side in enumerate(unit["order"], 1):
                header = {"dir": ab["dir"], "pass": unit["pass"], "task": unit["task"],
                          "arm": side, "position": position, "order": unit["order"],
                          "seed": args.seed, "settings_diff": diff, "ref": args.ref,
                          "shipped_note": note}
                line = (f"== passe {unit['pass']}, {unit['task']}, bras {side} ({position}/2, "
                        f"ordre {unit['order']})")
                print(line, flush=True)
                log.write(line + "\n")
                result = launch(args, bin_dir / "prime-agent",
                                args.a if side == "A" else args.b, unit["task"], arms_dir,
                                header, log)
                ab["arms"].append({**unit, "arm": side, "position": position, **result})
                print(f"   {result['status'] or 'échec'} en {result['wall_s']} s (surcoût "
                      f"{result['overhead_s']} s) -> {result['run_dir']}", flush=True)
                ab["overhead"] = overhead_summary(ab["arms"])
                (out_dir / "ab.json").write_text(json.dumps(ab, indent=2, ensure_ascii=False)
                                                 + "\n")
                failed = failed + 1 if result["exit"] != 0 else 0
                if failed >= MAX_FAILED_LAUNCHES:
                    write_md(out_dir / "ab.md", ab)
                    fail(f"{failed} lancements de suite en échec : A/B arrêté ({out_dir})")
    write_md(out_dir / "ab.md", ab)
    print(f"A/B : {out_dir / 'ab.md'}")


if __name__ == "__main__":
    main()
