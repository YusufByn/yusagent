#!/usr/bin/env python3
"""Contrôle court de la PR 3a avant merge (ticket 43, §P.16) rejoué par ACP sur le binaire.

YA-18 (`docs/tickets/43-retour-arriere.md`, §P.16). Ce que Yusuf fera à la main dans l'extension :
un chat normal dans un projet git, puis vérifier que le dépôt shadow
`<dossier agent>/checkpoints/<empreinte>/` est créé et que le dépôt du projet n'a pas bougé. Ici, au
niveau du pont : vrai moteur, vrai kernel, enveloppe active, et pour modèle le serveur de capture
(scénario `ya43-control`, réponses texte), aucun vrai modèle.

Étapes :
1. projet git (deux commits, une branche, un stash, un fichier non suivi) ; relevé de
   `git status --porcelain`, `git for-each-ref`, `git stash list`, `git rev-list --all --count` ;
2. deux prompts sans cellule ; durée de chacun (envoi, réponse), le premier avec le premier
   instantané du workspace ;
3. le dossier `<agent>/checkpoints/<empreinte>/` existe (empreinte : 16 premiers hex du SHA-256 du
   chemin canonique du projet), son fichier `workspace` nomme le projet, et il porte les refs
   `refs/ya/<conversation>/debut/<rewindTo>` des deux prompts et `fin` ;
4. le relevé git du projet est identique à celui de l'étape 1.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/ya43_3a_control.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si tout est conforme. macOS ; bibliothèque standard uniquement.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
GIT_ENV = {"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_AUTHOR_NAME": "u",
           "GIT_AUTHOR_EMAIL": "u@example.invalid", "GIT_COMMITTER_NAME": "u",
           "GIT_COMMITTER_EMAIL": "u@example.invalid"}


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
    """L'environnement du banc, enveloppe active."""
    env = _BASE_ENV(*args)
    env.pop("PRIME_AGENT_SECRET_GUARD", None)
    return env


run.base_env = base_env_guard_on


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(cwd), *args], capture_output=True, text=True,
                          env=GIT_ENV, check=True).stdout


def shadow(repo: Path, *args: str) -> str:
    return subprocess.run(["git", f"--git-dir={repo}", *args], capture_output=True, text=True,
                          env=GIT_ENV).stdout


def make_project(project: Path) -> None:
    project.mkdir(parents=True)
    (project / "README.md").write_text("# projet de contrôle\n")
    (project / "src").mkdir()
    (project / "src" / "main.py").write_text("print('bonjour')\n")
    git(project, "init", "-q")
    git(project, "add", "-A")
    git(project, "commit", "-q", "-m", "un")
    (project / "src" / "main.py").write_text("print('bonjour, deux')\n")
    git(project, "commit", "-q", "-am", "deux")
    git(project, "branch", "fonction")
    (project / "README.md").write_text("# projet de contrôle, modifié\n")
    git(project, "stash", "-q")
    (project / "notes.txt").write_text("fichier non suivi\n")


def project_state(project: Path) -> dict:
    return {"status": git(project, "status", "--porcelain"),
            "refs": git(project, "for-each-ref"),
            "stash": git(project, "stash", "list"),
            "commits": git(project, "rev-list", "--all", "--count").strip()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
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
    project = (control.WORK / "projet").resolve()
    make_project(project)
    project = project.resolve()
    before = project_state(project)
    print(f"binaire {version} ; enveloppe active ; projet {project}")
    print(f"1. avant : status {before['status']!r} ; refs {len(before['refs'].splitlines())} ; "
          f"stash {before['stash'].strip()!r} ; commits {before['commits']}")
    agent, sock, capture = control.case_dirs("3a", out)
    bridge = control.Bridge(binary, cache, out / "3a", agent, sock, project)
    checks, report = {}, {"version": version}
    try:
        sid, _, file_id = bridge.open(project)
        t0 = time.monotonic()
        first, _ = bridge.prompt(sid, "Bonjour, que contient ce projet ?")
        first_ms = round((time.monotonic() - t0) * 1000)
        t0 = time.monotonic()
        second, _ = bridge.prompt(sid, "Merci.")
        second_ms = round((time.monotonic() - t0) * 1000)
        print(f"2. premier prompt {first_ms} ms (premier instantané compris) ; deuxième {second_ms} ms")
        digest = hashlib.sha256(str(project).encode()).hexdigest()[:16]
        repo = agent.resolve() / "checkpoints" / digest
        deadline = time.monotonic() + 10
        wanted = [f"refs/ya/{file_id}/debut/{first.get('rewindTo')}",
                  f"refs/ya/{file_id}/debut/{second.get('rewindTo')}", f"refs/ya/{file_id}/fin"]
        refs = []
        while time.monotonic() < deadline:
            refs = shadow(repo, "for-each-ref", "--format=%(refname)").split()
            if all(name in refs for name in wanted):
                break
            time.sleep(0.2)
        named = (repo / "workspace").read_text().strip() if (repo / "workspace").exists() else None
        mode = oct(repo.stat().st_mode & 0o777) if repo.exists() else None
        print(f"3. dépôt shadow {repo} : existe {repo.is_dir()} ; mode {mode} ; workspace {named!r} ;"
              f" refs {refs}")
        after = project_state(project)
        print(f"4. après : status {after['status']!r} ; refs {len(after['refs'].splitlines())} ; "
              f"stash {after['stash'].strip()!r} ; commits {after['commits']}")
        checks["depot_shadow_cree"] = repo.is_dir()
        checks["fichier_workspace_nomme_le_projet"] = named == str(project)
        checks["refs_des_deux_prompts_et_fin"] = all(name in refs for name in wanted)
        checks["projet_git_inchange"] = after == before
        report.update({"first_prompt_ms": first_ms, "second_prompt_ms": second_ms,
                       "repo": str(repo), "mode": mode, "refs": refs,
                       "before": before, "after": after})
    finally:
        bridge.close()
        control.stop_daemon(sock)
        capture.stop()
    report["checks"] = checks
    report["conforming"] = all(checks.values())
    (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    for name, ok in checks.items():
        print(f"{name} : {'oui' if ok else 'NON'}")
    print("CONFORME" if report["conforming"] else "NON CONFORME")
    sys.exit(0 if report["conforming"] else 1)


if __name__ == "__main__":
    main()
