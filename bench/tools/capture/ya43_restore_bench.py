#!/usr/bin/env python3
"""Bench de la restauration des fichiers (ticket 43, PR 3a, §P.6) sur le binaire, enveloppe active.

YA-18 (`docs/tickets/43-retour-arriere.md`, §C, §P.6, §P.12.7, critère du CDC « restauration d'un
tour en moins de 2 s sur 10 000 fichiers »). Vrai moteur, vrai kernel ; pour modèle le serveur de
capture (scénario `ya43-control`, cellules `BENCH-EDIT` et `BENCH-MANY` de `server.py`), aucun vrai
modèle. Le workspace (10 000 fichiers d'environ 1 Ko dans 100 dossiers, comme l'annexe du ticket)
est un dépôt git de l'utilisateur avec deux commits, une branche et un stash. L'enveloppe est
active (`PRIME_AGENT_SECRET_GUARD` retirée de l'environnement du banc).

Mesures :
- `restore` : par répétition, un prompt `BENCH-EDIT` (50 fichiers modifiés, 20 créés, 10
  supprimés), `restore_preview`, puis `restore { files, conversation }`, chronométrés côté client
  (envoi de la requête, réception de la réponse) ; puis une fois `BENCH-MANY` (5 000 fichiers
  modifiés). Conforme si chaque restauration prend moins de 2 s, si le workspace revient à
  l'identique (empreinte de chaque fichier) et si le dépôt de l'utilisateur garde ses commits,
  refs, stash et statut ;
- `surcoût` (avec `--main`) : délai entre la réception du `turnStart` d'un prompt sans cellule et
  l'arrivée de sa première requête au serveur (l'instantané de début se place entre les deux),
  blocs alternés `main` et branche, session neuve par bloc ; médiane et p95 par binaire. Seuil
  (Q-P2) : surcoût médian inférieur à 150 ms ;
- le premier prompt d'un workspace neuf (premier instantané, à froid) est noté à part.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya43_restore_bench.py --binary <yusagent de la branche> \\
        [--main <yusagent de main>] --out <dossier neuf> [--files 10000] [--reps 5] [--blocks 2]

Code de sortie 0 si tout est conforme. macOS ; bibliothèque standard uniquement.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
GUARD = "PRIME_AGENT_SECRET_GUARD"
LIMIT_MS = 2000
OVERHEAD_LIMIT_MS = 150


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


control = load("ya43_control", "ya43_control.py")
run = control.run
_BASE_ENV = run.base_env


def base_env_guard_on(*args):
    """L'environnement du banc, enveloppe active (le banc ne lance pas son propre sandbox ici)."""
    env = _BASE_ENV(*args)
    env.pop(GUARD, None)
    return env


run.base_env = base_env_guard_on
GIT_ENV = {"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_AUTHOR_NAME": "u",
           "GIT_AUTHOR_EMAIL": "u@example.invalid", "GIT_COMMITTER_NAME": "u",
           "GIT_COMMITTER_EMAIL": "u@example.invalid"}


def git(ws: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(ws), *args], capture_output=True, text=True,
                          env=GIT_ENV, check=True).stdout


def make_workspace(ws: Path, count: int) -> None:
    line = "x" * 63 + "\n"
    for i in range(count):
        d = ws / f"d{i % 100:03d}"
        d.mkdir(parents=True, exist_ok=True)
        (d / f"f{i:05d}.txt").write_text(f"file {i}\n" + line * 16)
    git(ws, "init", "-q")
    git(ws, "add", "-A")
    git(ws, "commit", "-q", "-m", "one")
    (ws / "d000" / "f00000.txt").write_text("second commit\n")
    git(ws, "commit", "-q", "-am", "two")
    git(ws, "branch", "feature")
    (ws / "d000" / "f00000.txt").write_text("stashed\n")
    git(ws, "stash", "-q")


def digest(ws: Path) -> str:
    h = hashlib.sha256()
    for path in sorted(p for p in ws.rglob("*") if p.is_file() and ".git" not in p.parts):
        h.update(str(path.relative_to(ws)).encode() + b"\0" + path.read_bytes() + b"\0")
    return h.hexdigest()


def user_state(ws: Path) -> list[str]:
    return [git(ws, "rev-list", "--all", "--count"), git(ws, "for-each-ref"),
            git(ws, "stash", "list"), git(ws, "status", "--porcelain")]


def timed(bridge, method: str, params: dict) -> tuple[int, dict]:
    t0 = time.monotonic()
    answer = bridge.request(method, params)
    return round((time.monotonic() - t0) * 1000), answer


def request_times(capture) -> list[float]:
    """Les instants (horloge murale) d'arrivée des requêtes de la boucle gardées par le serveur."""
    times = []
    for path in sorted(capture.dir.glob("*.body.json")):
        try:
            body = json.loads(path.read_bytes())
        except (OSError, json.JSONDecodeError):
            continue
        if body.get("tools"):
            times.append(path.stat().st_mtime)
    return times


def prompt_overhead(bridge, capture, sid: str, text: str) -> float | None:
    """Délai (ms) entre la réception du `turnStart` du prompt et sa première requête au serveur."""
    known = len(request_times(capture))
    before = len(bridge.frames)
    bridge.request("session/prompt", {"sessionId": sid, "prompt": [{"type": "text", "text": text}]})
    start_at = None
    for frame in bridge.frames[before:]:
        meta = (((frame.get("params") or {}).get("update") or {}).get("_meta") or {}).get(
            control.META) or {}
        if (meta.get("turnStart") or {}).get("origin") == "prompt":
            start_at = frame.get("_received")
            break
    times = request_times(capture)
    if start_at is None or len(times) <= known:
        return None
    return (times[known] - start_at) * 1000


def stamp_frames(bridge) -> None:
    """Note l'heure murale de réception de chaque trame (pour le surcoût)."""
    original = bridge.wait

    def wait(pred, what):
        deadline = time.monotonic() + control.TIMEOUT_S
        for frame in bridge.frames:
            if pred(frame):
                return frame
        while time.monotonic() < deadline:
            try:
                line = bridge.lines.get(timeout=0.5)
            except Exception:  # noqa: BLE001 - file vide
                continue
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            frame["_received"] = time.time()
            bridge.frames.append(frame)
            if pred(frame):
                return frame
        return original(pred, what)

    bridge.wait = wait


def case_restore(binary: Path, cache: Path, out: Path, ws: Path, reps: int) -> dict:
    agent, sock, capture = control.case_dirs("restore", out)
    bridge = control.Bridge(binary, cache, out / "restore", agent, sock, ws)
    stamp_frames(bridge)
    rows, checks = [], {}
    try:
        sid, _, _ = bridge.open(ws)
        t0 = time.monotonic()
        bridge.prompt(sid, "premier prompt, sans cellule")
        first_ms = round((time.monotonic() - t0) * 1000)
        for rep in list(range(1, reps + 1)) + ["many"]:
            before_digest, before_user = digest(ws), user_state(ws)
            text = "BENCH-MANY" if rep == "many" else f"BENCH-EDIT {rep}"
            start, _ = bridge.prompt(sid, text)
            changed = digest(ws) != before_digest
            preview_ms, preview = timed(bridge, "_yus/session/restore_preview",
                                        {"sessionId": sid, "to": start.get("rewindTo")})
            files = len(((preview.get("result") or {}).get("files")) or [])
            restore_ms, answer = timed(bridge, "_yus/session/restore",
                                       {"sessionId": sid, "to": start.get("rewindTo"),
                                        "files": True, "conversation": True})
            row = {"rep": rep, "changed_by_cell": changed, "preview_ms": preview_ms,
                   "preview_files": files, "restore_ms": restore_ms,
                   "restored": "result" in answer, "error": answer.get("error"),
                   "same_files": digest(ws) == before_digest,
                   "same_user_repo": user_state(ws) == before_user}
            rows.append(row)
            print(f"rép. {rep} : cellule a changé le workspace {changed} ; aperçu {preview_ms} ms "
                  f"({files} fichiers) ; restauration {restore_ms} ms ; fichiers identiques "
                  f"{row['same_files']} ; dépôt de l'utilisateur identique {row['same_user_repo']}"
                  + (f" ; erreur {answer.get('error')}" if answer.get("error") else ""))
        checks["restaurations_sous_2_s"] = all(r["restore_ms"] < LIMIT_MS for r in rows)
        checks["restaurations_acceptees"] = all(r["restored"] for r in rows)
        checks["cellules_ont_change_le_workspace"] = all(r["changed_by_cell"] for r in rows)
        checks["workspace_restaure_a_l_identique"] = all(r["same_files"] for r in rows)
        checks["depot_utilisateur_intact"] = all(r["same_user_repo"] for r in rows)
    finally:
        bridge.close()
        control.stop_daemon(sock)
        capture.stop()
    edits = [r["restore_ms"] for r in rows if r["rep"] != "many"]
    return {"checks": checks, "rows": rows, "first_prompt_ms": first_ms,
            "restore_ms_max": max(edits) if edits else None,
            "restore_ms_median": statistics.median(edits) if edits else None}


def overhead_block(binary: Path, cache: Path, out: Path, ws: Path, name: str, prompts: int,
                   pause: float = 0.0):
    agent, sock, capture = control.case_dirs(name, out)
    bridge = control.Bridge(binary, cache, out / name, agent, sock, ws)
    stamp_frames(bridge)
    values = []
    try:
        sid, _, _ = bridge.open(ws)
        bridge.prompt(sid, "chauffe")
        for i in range(prompts):
            # Une pause laisse finir l'instantané de fin du tour précédent (même file).
            time.sleep(pause)
            value = prompt_overhead(bridge, capture, sid, f"mesure {i}")
            if value is not None:
                values.append(value)
    finally:
        bridge.close()
        control.stop_daemon(sock)
        capture.stop()
    return values


def p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(0.95 * len(ordered)))]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--main", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--files", type=int, default=10000)
    parser.add_argument("--reps", type=int, default=5)
    parser.add_argument("--blocks", type=int, default=2)
    parser.add_argument("--prompts", type=int, default=10)
    parser.add_argument("--pause", type=float, default=0.0,
                        help="secondes entre deux prompts de la mesure du surcoût")
    parser.add_argument("--overhead-only", action="store_true",
                        help="seulement la mesure du surcoût (pas les restaurations)")
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True)
    cache = args.cache.resolve()
    binary = args.binary.resolve()
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}
                             ).stdout.strip()
    run.prepare(binary, cache, version)
    shutil.rmtree(control.WORK, ignore_errors=True)
    (control.WORK / "tmp").mkdir(parents=True)
    ws = control.WORK / "workspace"
    make_workspace(ws, args.files)
    print(f"binaire {version} ; workspace {args.files} fichiers ; enveloppe active ; pause entre "
          f"prompts {args.pause} s")
    report = {"version": version, "files": args.files}
    checks = {}
    if not args.overhead_only:
        restore = case_restore(binary, cache, out, ws, args.reps)
        report["restore"] = restore
        print(f"premier prompt (instantané à froid) : {restore['first_prompt_ms']} ms ; restauration "
              f"BENCH-EDIT : médiane {restore['restore_ms_median']} ms, max "
              f"{restore['restore_ms_max']} ms")
        checks.update(restore["checks"])
    if args.main:
        main_binary = args.main.resolve()
        series = {"main": [], "branche": []}
        for block in range(args.blocks):
            for label, path in (("main", main_binary), ("branche", binary)):
                values = overhead_block(path, cache, out, ws, f"{label}-{block}", args.prompts,
                                        args.pause)
                series[label].extend(values)
                print(f"bloc {block} {label} : {[round(v) for v in values]}")
        summary = {label: {"median_ms": statistics.median(v), "p95_ms": p95(v), "n": len(v)}
                   for label, v in series.items() if v}
        report["overhead"] = summary
        if "main" in summary and "branche" in summary:
            diff = summary["branche"]["median_ms"] - summary["main"]["median_ms"]
            summary["surcout_median_ms"] = diff
            checks["surcout_median_sous_150_ms"] = diff < OVERHEAD_LIMIT_MS
            print(f"surcoût : main médiane {summary['main']['median_ms']:.0f} ms (p95 "
                  f"{summary['main']['p95_ms']:.0f}) ; branche médiane "
                  f"{summary['branche']['median_ms']:.0f} ms (p95 {summary['branche']['p95_ms']:.0f})"
                  f" ; écart des médianes {diff:.0f} ms")
    report["checks"] = checks
    report["conforming"] = all(checks.values())
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    for name, ok in checks.items():
        print(f"{name} : {'oui' if ok else 'NON'}")
    print("CONFORME" if report["conforming"] else "NON CONFORME")
    sys.exit(0 if report["conforming"] else 1)


if __name__ == "__main__":
    main()
