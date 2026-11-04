#!/usr/bin/env python3
"""Minage des tâches candidates du smoke bench (T07, YA-14, phase A).

Usage :
    python3 -I bench/smoke/tools/mine.py discover [--since 2026-08-01]
    python3 -I bench/smoke/tools/mine.py validate --isolation netns --shard 1/10   # CI Linux
    python3 -I bench/smoke/tools/mine.py validate --only <liste d'URL de PR>       # macOS
    python3 -I bench/smoke/tools/mine.py csv --validated <shards>.json ...

Étapes :
  1. `discover` (API GitHub par `gh`) : dépôts Python, TypeScript et JavaScript sous
     licence MIT, Apache-2.0, BSD ou ISC, petits à moyens, actifs depuis `--since` ; puis
     leurs PR fusionnées depuis `--since` qui ressemblent à une correction (titre,
     libellé ou issue fermée) et modifient du code source et des tests, sans toucher
     aux manifestes ni aux verrous de dépendances. Sortie versionnée :
     `bench/smoke/tools/discovered.json` (liste des PR) ; l'état du parcours des
     dépôts reste dans le cache (`repos.json`).
  2. `validate` : par candidat, clone partiel, installation au commit parent
     (`uv sync` ou `npm ci --ignore-scripts`, réseau autorisé), puis réseau coupé :
       - 1 run rouge : parent + fichiers de test du commit de correction ;
       - 3 runs verts : commit de correction ;
       - suite complète sur l'état vert (durée seule, plafond `--suite-timeout`).
     Retenu si au moins un test des fichiers de test de la correction échoue au rouge
     et passe aux 3 verts, et si les 3 verts n'ont aucun échec et le même résultat.
     Type d'échec rouge noté : `assertion`, `exception`, `import`, `collecte`.
     Une sonde réseau précède la validation : une requête sortante doit échouer sous
     la coupure réseau, sinon arrêt.
  3. `csv` : fusionne les sorties de `validate` (shards) en `candidates.csv` (retenus
     seulement) et écrit un résumé (retenus par langage, minimums, échecs par cause).

Isolation (`--isolation`) du code des dépôts tiers (installation, build, tests) :
  - `sandbox-exec` (macOS, par défaut) : profil `mine.sb`, écriture limitée au cache,
    lecture de `$HOME` refusée sauf cache du minage et toolchain Node, réseau limité à
    localhost ; l'installation passe par le proxy CONNECT à liste blanche de
    `bench/smoke/proxies.py` (`mine-hosts.txt`). Dépendances en sdist refusées
    (`UV_NO_BUILD=1`).
  - `netns` (Linux, runner CI jetable : le runner est le sandbox, pas de
    `sandbox-exec`) : réseau coupé par `sudo unshare --net` (espace réseau vide,
    loopback seul), mémoire bornée par `systemd-run --scope -p MemoryMax=` (`--mem-gb`),
    puis retour à l'utilisateur du runner par `setpriv`.
`HOME` des commandes tierces : `<cache>/home`. Variables de jetons retirées.

Rien n'est cloné ni installé dans le dépôt yusAgent (cache `--cache`, par défaut
`~/.cache/yusagent-smoke/mine`). Bibliothèque standard + `gh` (discover seulement),
`git`, `uv`, `npm`, `curl`.
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
import xml.etree.ElementTree as ET
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
SMOKE = HERE.parent
SANDBOX = HERE / "mine.sb"
HOSTS = HERE / "mine-hosts.txt"
DISCOVERED = HERE / "discovered.json"
OUT_CSV = HERE / "candidates.csv"
PLATFORM = {"Darwin": "macos", "Linux": "linux"}.get(platform.system(), platform.system().lower())

LICENSES = {"mit", "apache-2.0", "bsd-2-clause", "bsd-3-clause", "isc"}
LANGS = {"python": "python", "typescript": "node", "javascript": "node"}
SRC_EXT = {"python": (".py",), "node": (".ts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".mts", ".cts")}
# Manifestes et verrous : une correction qui les touche change l'installation, exclue.
MANIFESTS = {"pyproject.toml", "setup.py", "setup.cfg", "uv.lock", "poetry.lock", "requirements.txt",
             "package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "tox.ini",
             "noxfile.py", "conftest.py"}
FIX_RE = re.compile(r"\b(fix(e[sd])?|bug|regression|incorrect|wrong|crash|broken|error)\b", re.I)
SKIP_RE = re.compile(r"\b(bump|deps?|dependabot|renovate|chore\(deps|release|typo|docs?)\b", re.I)
CLOSES_RE = re.compile(r"\b(?:fix(?:e[sd])?|close[sd]?|resolve[sd]?)\s+(?:([\w.-]+/[\w.-]+))?#(\d+)",
                       re.I)
HEAVY_RE = re.compile(r"[\"'](torch|tensorflow|jax|jaxlib|transformers|vllm|opencv-python\S*|"
                      r"onnxruntime\S*|triton|xformers|deepspeed|paddlepaddle)\b", re.I)
TEST_FILE_RE = re.compile(r"(test_.*\.py|_test\.py|\.(test|spec)\.[cm]?[jt]sx?)$")
IMPORT_RE = re.compile(r"ImportError|ModuleNotFoundError|cannot import name|Cannot find module|"
                       r"does not provide an export|is not exported|Failed to (load|resolve) ", re.I)
ASSERT_RE = re.compile(r"\bassert|AssertionError|ERR_ASSERTION|expected|toBe|toEqual|toThrow|"
                       r"deepEqual|strictEqual", re.I)

# Requêtes de dépôts : taille en Ko (API), étoiles pour écarter les dépôts personnels.
REPO_QUERY = "language:{lang} license:{lic} stars:300..40000 size:<60000 pushed:>{since} archived:false"
REPO_PAGES = 2  # 2 × 100 dépôts par (langage, licence), tri par étoiles

MAX_SRC_FILES = 8
MAX_SRC_LINES = 400
PER_REPO_PRS = 4  # PR candidates gardées par dépôt avant validation
MAX_FILE_CALLS = 6  # appels `pulls/<n>/files` par dépôt (limite de 5 000 appels/h)
GREEN_RUNS = 3

# Causes de rejet (résumé) : code -> libellé.
CAUSES = {
    "manifest": "le commit touche un manifeste",
    "no_setup": "installation non reconnue (uv, npm ci, runner)",
    "heavy": "dépendance lourde ou extension native",
    "install": "installation en échec",
    "no_test_file": "aucun fichier de test exécutable modifié",
    "build": "build en échec",
    "no_results": "aucun résultat de test",
    "green_failed": "échec à l'état vert",
    "flaky": "états verts différents (instable)",
    "no_f2p": "aucun test rouge -> vert",
    "deadline": "non traité (délai du shard)",
    "error": "erreur du mineur",
}


class Reject(Exception):
    def __init__(self, cause: str, detail: str = ""):
        super().__init__(detail)
        self.cause, self.detail = cause, detail


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log(msg: str) -> None:
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {msg}", file=sys.stderr, flush=True)


# --- API GitHub -------------------------------------------------------------------------

def gh_api(path: str, params: dict | None = None) -> object:
    """GET sur l'API GitHub par `gh api` ; nouvelle tentative sur limite de débit."""
    cmd = ["gh", "api", "-X", "GET", path]
    for key, value in (params or {}).items():
        cmd += ["-f", f"{key}={value}"]
    for attempt in range(5):
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode == 0:
            return json.loads(proc.stdout or "null")
        if "rate limit" in proc.stderr.lower() or "403" in proc.stderr:
            wait = 30 * (attempt + 1)
            log(f"limite de débit ({path}), attente {wait} s")
            time.sleep(wait)
            continue
        raise RuntimeError(f"gh api {path}: {proc.stderr.strip()[:300]}")
    raise RuntimeError(f"gh api {path}: limite de débit persistante")


def is_test_path(path: str) -> bool:
    parts = path.lower().split("/")
    name = parts[-1]
    if any(p in ("test", "tests", "__tests__", "spec", "testing") for p in parts[:-1]):
        return True
    return bool(re.match(r"(test_.*\.py|.*_test\.py|.*\.(test|spec)\.[cm]?[jt]sx?)$", name))


def classify_files(files: list[dict], lang: str) -> dict | None:
    """Fichiers de la PR : source et tests du langage, rien d'autre que doc ou fixtures."""
    src, tests, other = [], [], []
    for f in files:
        path = f["filename"]
        if Path(path).name in MANIFESTS:
            return None
        if is_test_path(path):
            tests.append(f)
        elif path.endswith(SRC_EXT[lang]):
            src.append(f)
        else:
            other.append(f)
    test_code = [f for f in tests if f["filename"].endswith(SRC_EXT[lang])]
    if not src or not test_code:
        return None
    lines = sum(f["additions"] + f["deletions"] for f in src)
    if len(src) > MAX_SRC_FILES or lines > MAX_SRC_LINES:
        return None
    return {"src_files": [f["filename"] for f in src],
            "src_added": sum(f["additions"] for f in src),
            "src_deleted": sum(f["deletions"] for f in src),
            "test_files": [f["filename"] for f in tests],
            "test_code_files": [f["filename"] for f in test_code],
            "test_lines": sum(f["additions"] + f["deletions"] for f in tests),
            "other_files": [f["filename"] for f in other]}


def scan_repo(name: str, r: dict, since: str, seen_prs: set[str]) -> list[dict]:
    """PR fusionnées depuis `since` d'un dépôt qui ressemblent à une correction testée."""
    try:
        prs = gh_api(f"repos/{name}/pulls", {"state": "closed", "sort": "updated",
                                             "direction": "desc", "per_page": "100",
                                             "base": r["default_branch"]})
    except RuntimeError as exc:
        log(f"{name} : {exc}")
        return []
    found: list[dict] = []
    file_calls = 0
    for pr in prs:
        if len(found) >= PER_REPO_PRS or file_calls >= MAX_FILE_CALLS:
            break
        merged = pr.get("merged_at")
        if not merged or merged < since or pr["html_url"] in seen_prs:
            continue
        title, body = pr["title"] or "", pr["body"] or ""
        labels = [lab["name"].lower() for lab in pr["labels"]]
        closes = CLOSES_RE.findall(body)
        fixish = FIX_RE.search(title) or any("bug" in lab for lab in labels) or closes
        if not fixish or SKIP_RE.search(title) or (pr["user"] or {}).get("type") == "Bot":
            continue
        file_calls += 1
        try:
            files = gh_api(f"repos/{name}/pulls/{pr['number']}/files", {"per_page": "100"})
        except RuntimeError as exc:
            log(f"{name}#{pr['number']} : {exc}")
            continue
        info = classify_files(files, r["lang"])
        if info is None:
            continue
        issue = ""
        if closes:
            other_repo, num = closes[0]
            issue = f"https://github.com/{other_repo or name}/issues/{num}"
        found.append({
            "repo": name, "lang": r["lang"], "license": r["license"], "stars": r["stars"],
            "size_kb": r["size_kb"], "clone_url": r["clone_url"], "pr": pr["number"],
            "pr_url": pr["html_url"], "issue_url": issue, "title": title,
            "labels": labels, "merged_at": merged, "fix_commit": pr["merge_commit_sha"],
            **info})
    return found


def load_discovered(cache: Path) -> tuple[dict, list[dict]]:
    """État des dépôts (cache) et liste des PR (versionnée)."""
    repos_path, legacy = cache / "repos.json", cache / "discovered.json"
    if not repos_path.exists() and legacy.exists():  # ancien format : un seul fichier
        old = json.loads(legacy.read_text())
        repos_path.write_text(json.dumps(old["repos"], indent=1))
        if not DISCOVERED.exists():
            DISCOVERED.write_text(json.dumps(old["candidates"], indent=1) + "\n")
    repos = json.loads(repos_path.read_text()) if repos_path.exists() else {}
    cands = json.loads(DISCOVERED.read_text()) if DISCOVERED.exists() else []
    return repos, cands


def discover(args: argparse.Namespace) -> None:
    cache = Path(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    repos, cands = load_discovered(cache)
    seen_prs = {c["pr_url"] for c in cands}

    def save() -> None:
        (cache / "repos.json").write_text(json.dumps(repos, indent=1))
        DISCOVERED.write_text(json.dumps(cands, indent=1, ensure_ascii=False) + "\n")

    if not repos or args.refresh_repos:
        for gh_lang in LANGS:
            for lic in sorted(LICENSES):
                q = REPO_QUERY.format(lang=gh_lang, lic=lic, since=args.since)
                for page in range(1, REPO_PAGES + 1):
                    res = gh_api("search/repositories", {"q": q, "sort": "stars", "order": "desc",
                                                         "per_page": "100", "page": str(page)})
                    for r in res["items"]:
                        spdx = ((r.get("license") or {}).get("spdx_id") or "").lower()
                        if spdx not in LICENSES or r["fork"] or r["archived"]:
                            continue
                        repos.setdefault(r["full_name"], {
                            "lang": LANGS[gh_lang], "gh_language": r["language"],
                            "license": r["license"]["spdx_id"], "stars": r["stargazers_count"],
                            "size_kb": r["size"], "default_branch": r["default_branch"],
                            "clone_url": r["clone_url"], "scanned": False})
                    log(f"dépôts {gh_lang}/{lic} page {page} : {len(res['items'])} (total {len(repos)})")
                    if len(res["items"]) < 100:
                        break
                    time.sleep(2.5)  # recherche : 30 requêtes/min
        save()

    # Petits dépôts d'abord (bibliothèques plus probables, installation courte) ; arrêt
    # quand chaque langage a atteint sa cible.
    todo = sorted((n for n, r in repos.items() if not r["scanned"]),
                  key=lambda n: repos[n]["size_kb"])
    targets = {"python": args.target_python, "node": args.target_node}
    guard = threading.Lock()

    def count(lang: str) -> int:
        return sum(1 for c in cands if c["lang"] == lang)

    def job(name: str) -> None:
        r = repos[name]
        if count(r["lang"]) >= targets[r["lang"]]:
            return
        found = scan_repo(name, r, args.since, seen_prs)
        with guard:
            for c in found:
                if c["pr_url"] not in seen_prs:
                    seen_prs.add(c["pr_url"])
                    cands.append(c)
            r["scanned"] = True
            if found:
                log(f"{name} : {len(found)} candidat(s) (python {count('python')}, "
                    f"node {count('node')})")
            save()

    log(f"{len(todo)} dépôts à parcourir, cibles {targets}")
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        list(pool.map(job, todo))
    save()
    log(f"candidats découverts : {len(cands)} {dict(Counter(c['lang'] for c in cands))} "
        f"-> {DISCOVERED}")


# --- Isolation --------------------------------------------------------------------------

class Isolation:
    """Enveloppe des commandes tierces selon `--isolation`."""

    def __init__(self, mode: str, cache: Path, mem_gb: float = 3):
        self.mode, self.cache, self.mem_gb = mode, cache.resolve(), mem_gb
        self.proxy_env: dict[str, str] = {}
        self.proxy = None
        if mode == "sandbox-exec":
            sys.path.insert(0, str(SMOKE))
            import proxies  # noqa: PLC0415 - proxy CONNECT du banc (T06)
            (self.cache / "logs").mkdir(parents=True, exist_ok=True)
            self.proxy = proxies.ConnectProxy(self.cache / "logs" / "connect.ndjson",
                                              proxies.load_hosts(HOSTS))
            proxies.serve(self.proxy)
            url = f"http://127.0.0.1:{self.proxy.port}"
            self.proxy_env = {"HTTPS_PROXY": url, "HTTP_PROXY": url, "https_proxy": url,
                              "http_proxy": url, "npm_config_https_proxy": url,
                              "npm_config_proxy": url, "NO_PROXY": "127.0.0.1,localhost"}
            node = shutil.which("node")
            self.node_home = str(Path(node).resolve().parents[1]) if node else "/nonexistent"

    def wrap(self, cmd: list[str], env: dict, network: bool) -> tuple[list[str], dict]:
        if self.mode == "sandbox-exec":
            # Réseau toujours limité à localhost ; l'installation passe par le proxy.
            env = {**env, **(self.proxy_env if network else {})}
            return ["/usr/bin/sandbox-exec", "-f", str(SANDBOX),
                    "-D", f"CACHE={self.cache}", "-D", f"HOME_DIR={Path.home().resolve()}",
                    "-D", f"NODE_HOME={self.node_home}", *cmd], env
        if network:
            return cmd, env
        # netns : espace réseau neuf (loopback seul), mémoire bornée (scope systemd : un
        # dépôt qui épuise la mémoire est tué seul, pas le runner), puis retour à
        # l'utilisateur courant.
        inner = ('ip link set lo up && exec setpriv --reuid="$MINE_UID" --regid="$MINE_GID" '
                 '--init-groups -- "$@"')
        env = {**env, "MINE_UID": str(os.getuid()), "MINE_GID": str(os.getgid())}
        return ["sudo", "-E", "env", f"PATH={env['PATH']}", f"HOME={env['HOME']}",
                "systemd-run", "--scope", "--quiet", "-p", f"MemoryMax={self.mem_gb}G",
                "-p", "MemorySwapMax=0", "unshare", "--net", "--", "/bin/sh", "-c", inner, "mine-netns", *cmd], env


ISO: Isolation | None = None


def sh(cmd: list[str] | str, cwd: Path, timeout: int, env: dict, network: bool,
       log_path: Path | None = None) -> dict:
    """Code tiers sous l'isolation choisie ; `network=False` : réseau coupé."""
    if isinstance(cmd, str):
        cmd = ["/bin/bash", "-c", cmd]
    cmd, env = ISO.wrap(cmd, env, network)
    started = time.monotonic()
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            start_new_session=True, errors="replace")
    try:
        out, _ = proc.communicate(timeout=timeout)
        code, timed_out = proc.returncode, False
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except PermissionError:  # groupe lancé par sudo : tuer par sudo
            subprocess.run(["sudo", "kill", "-KILL", f"-{proc.pid}"], capture_output=True)
        out, _ = proc.communicate()
        code, timed_out = None, True
    if log_path:
        with log_path.open("a") as fh:
            fh.write(f"\n$ {' '.join(cmd[-1:])}\n[exit={code} timeout={timed_out}]\n{out}\n")
    return {"exit": code, "timeout": timed_out, "seconds": round(time.monotonic() - started, 1),
            "tail": (out or "")[-1500:]}


def git(repo: Path, *args: str, timeout: int = 600) -> str:
    proc = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          timeout=timeout)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {proc.stderr.strip()[:300]}")
    return proc.stdout


def base_env(cache: Path, uv_no_build: bool) -> dict:
    cache = cache.resolve()
    for sub in ("tmp", "home"):
        (cache / sub).mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("GH_", "GITHUB_", "ACTIONS_", "VIRTUAL_ENV", "PYTHON",
                                "NODE_OPTIONS", "NPM_", "UV_"))
           and not re.search(r"TOKEN|SECRET|PASSWORD|API_KEY|CREDENTIAL", k, re.I)}
    env.update({"CI": "1", "DO_NOT_TRACK": "1", "HOME": str(cache / "home"),
                "TMPDIR": str(cache / "tmp"), "UV_CACHE_DIR": str(cache / "uv-cache"),
                "UV_PYTHON_INSTALL_DIR": str(cache / "uv-python"),
                "npm_config_cache": str(cache / "npm-cache"), "npm_config_audit": "false",
                "npm_config_fund": "false", "npm_config_update_notifier": "false",
                "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
                "PYTHONDONTWRITEBYTECODE": "1", "FORCE_COLOR": "0", "NO_COLOR": "1"})
    if uv_no_build:
        env["UV_NO_BUILD"] = "1"  # sdists des dépendances refusées ; le projet reste construit
    return env


def probe_network(cache: Path, env: dict) -> dict:
    """Une requête sortante doit échouer réseau coupé et réussir à l'installation."""
    url = "https://pypi.org/simple/pip/"
    curl = ["curl", "-sS", "-m", "15", "-o", "/dev/null", "-w", "%{http_code}", url]
    # L'enveloppe doit marcher (sinon curl « échoue » sans que le réseau soit coupé) et
    # curl doit échouer sur la résolution (6) ou la connexion (7).
    wrap_ok = sh(["true"], cache, 60, env, network=False)["exit"] == 0
    cut = sh(curl, cache, 60, env, network=False)
    open_ = sh(curl, cache, 60, env, network=True)
    probe = {"url": url, "isolation": ISO.mode, "wrapper_ok": wrap_ok, "cut_exit": cut["exit"],
             "cut_output": cut["tail"].strip()[-200:], "install_exit": open_["exit"],
             "install_output": open_["tail"].strip()[-200:],
             "network_cut": wrap_ok and cut["exit"] in (6, 7) and "curl: (" in cut["tail"]}
    log(f"sonde réseau coupé ({ISO.mode}) : curl {url} -> exit {cut['exit']} "
        f"« {probe['cut_output']} » (échec attendu)")
    log(f"sonde réseau d'installation : curl {url} -> exit {open_['exit']} "
        f"« {probe['install_output']} »")
    if not probe["network_cut"]:
        sys.exit(f"sonde : coupure réseau non prouvée (enveloppe {wrap_ok}, curl exit "
                 f"{cut['exit']}), arrêt")
    return probe


# --- Projets et rapports de test ----------------------------------------------------------

def detect_python(work: Path) -> dict:
    if not (work / "pyproject.toml").exists():
        raise Reject("no_setup", "pas de pyproject.toml")
    text = (work / "pyproject.toml").read_text(errors="replace")
    if "[tool.maturin]" in text or "setuptools-rust" in text or "scikit-build" in text:
        raise Reject("heavy", "extension native")
    if HEAVY_RE.search(text):
        raise Reject("heavy", HEAVY_RE.search(text).group(1))
    frozen = "--frozen " if (work / "uv.lock").exists() else ""
    install = (f"uv sync -q {frozen}--all-extras --all-groups --python 3.12"
               " && (.venv/bin/python -c 'import pytest' 2>/dev/null"
               " || uv pip install -q --python .venv/bin/python pytest)")
    return {"install": install, "runner": "pytest", "build": ""}


def detect_node(work: Path) -> dict:
    pkg_path = work / "package.json"
    if not pkg_path.exists() or not (work / "package-lock.json").exists():
        raise Reject("no_setup", "pas de package-lock.json")
    pkg = json.loads(pkg_path.read_text())
    if pkg.get("workspaces"):
        raise Reject("no_setup", "workspaces npm")
    deps = {**pkg.get("dependencies", {}), **pkg.get("devDependencies", {})}
    scripts = pkg.get("scripts") or {}
    script = scripts.get("test", "")
    if "vitest" in deps or "vitest" in script:
        runner = "vitest"
    elif "jest" in deps or "jest" in script:
        runner = "jest"
    elif "mocha" in deps or "mocha" in script:
        runner = "mocha"
    elif "node --test" in script or "node:test" in script:
        runner = "node"
    else:
        raise Reject("no_setup", f"runner non reconnu ({script[:60]!r})")
    return {"install": "npm ci --ignore-scripts --no-audit --no-fund --loglevel=error",
            "runner": runner, "build": "npm run -s build" if "build" in scripts else ""}


def test_cmd(runner: str, files: list[str], report: Path) -> str:
    q = " ".join(f"'{f}'" for f in files)
    if runner == "pytest":
        return (f".venv/bin/python -m pytest -q -p no:cacheprovider -o junit_family=xunit1 "
                f"--junitxml='{report}' {q}")
    if runner == "vitest":
        return f"npx vitest run --reporter=junit --outputFile='{report}' {q}"
    if runner == "jest":
        return f"npx jest --ci --json --outputFile='{report}' --runTestsByPath {q}"
    if runner == "mocha":
        return f"npx mocha --reporter json --reporter-option output='{report}' {q}"
    return f"node --test --test-reporter=junit --test-reporter-destination='{report}' {q}"


def parse_report(runner: str, report: Path, work: Path) -> dict[str, tuple[str, str]]:
    """Résultats par test : clé -> (passed | failed | skipped, message d'échec)."""
    results: dict[str, tuple[str, str]] = {}
    if not report.exists() or report.stat().st_size == 0:
        return results
    if runner in ("pytest", "vitest", "node"):
        try:
            root = ET.parse(report).getroot()
        except ET.ParseError:
            return results
        for suite in root.iter("testsuite"):
            for case in suite.iter("testcase"):
                name = case.get("name", "")
                if runner == "pytest":
                    key = f"{case.get('classname', '')}::{name}"
                else:
                    key = f"{case.get('file') or case.get('classname') or suite.get('name')}::{name}"
                bad = case.find("failure")
                if bad is None:
                    bad = case.find("error")
                if case.find("skipped") is not None:
                    results[key] = ("skipped", "")
                elif bad is not None:
                    results[key] = ("failed", f"{bad.get('message', '')}\n{bad.text or ''}"[:2000])
                else:
                    results[key] = ("passed", "")
        return results
    try:
        data = json.loads(report.read_text())
    except json.JSONDecodeError:
        return results
    if runner == "jest":
        for tr in data.get("testResults", []):
            rel = os.path.relpath(tr.get("name", ""), work)
            for a in tr.get("assertionResults", []):
                status = {"passed": "passed", "failed": "failed"}.get(a.get("status"), "skipped")
                results[f"{rel}::{a.get('fullName')}"] = (
                    status, "\n".join(a.get("failureMessages") or [])[:2000])
            if tr.get("status") == "failed" and not tr.get("assertionResults"):
                results[f"{rel}::<suite>"] = ("failed", (tr.get("message") or "")[:2000])
    else:  # mocha
        for kind, status in (("passes", "passed"), ("failures", "failed"), ("pending", "skipped")):
            for t in data.get(kind, []):
                rel = os.path.relpath(t.get("file", ""), work) if t.get("file") else ""
                msg = ((t.get("err") or {}).get("message") or "")[:2000]
                results[f"{rel}::{t.get('fullTitle')}"] = (status, msg)
    return results


def red_failure_type(f2p: list[str], red: dict[str, tuple[str, str]], red_tail: str) -> Counter:
    """Type d'échec rouge de chaque test rouge -> vert."""
    # Erreurs hors tests (collecte, chargement de fichier) : clés absentes du vert.
    loose = " ".join(msg for k, (st, msg) in red.items() if st == "failed" and k not in f2p)
    kinds: Counter = Counter()
    for key in f2p:
        status, msg = red.get(key, ("missing", ""))
        if status == "missing":
            kinds["import" if IMPORT_RE.search(loose + red_tail) else "collecte"] += 1
        elif IMPORT_RE.search(msg):
            kinds["import"] += 1
        elif ASSERT_RE.search(msg):
            kinds["assertion"] += 1
        else:
            kinds["exception"] += 1
    return kinds


# --- Validation -------------------------------------------------------------------------

_clone_locks: dict[str, threading.Lock] = {}
_clone_guard = threading.Lock()


def validate_one(c: dict, cache: Path, env: dict, suite_timeout: int) -> dict:
    rec = {k: c[k] for k in ("repo", "pr", "pr_url", "lang", "fix_commit")}
    rec["platform"] = PLATFORM
    slug = c["repo"].replace("/", "__")
    with _clone_guard:
        lock = _clone_locks.setdefault(slug, threading.Lock())
    with lock:
        try:
            _validate_locked(c, rec, cache, env, slug, suite_timeout)
            return {**rec, "ok": True, "cause": "", "reason": ""}
        except Reject as rej:
            return {**rec, "ok": False, "cause": rej.cause,
                    "reason": f"{CAUSES[rej.cause]}{' : ' + rej.detail if rej.detail else ''}"}
        except Exception as exc:  # noqa: BLE001 - un candidat cassé n'arrête pas le minage
            return {**rec, "ok": False, "cause": "error", "reason": f"erreur : {str(exc)[:200]}"}


def _validate_locked(c: dict, rec: dict, cache: Path, env: dict, slug: str,
                     suite_timeout: int) -> None:
    logs = cache / "logs"
    logs.mkdir(exist_ok=True)
    log_path = logs / f"{slug}-{c['pr']}.log"
    log_path.write_text(f"{c['pr_url']} {now()}\n")
    mirror = cache / "repos" / slug
    work = cache / "work" / f"{slug}-{c['pr']}"
    if not mirror.exists():
        mirror.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "-q", "--filter=blob:none", "--no-checkout",
                        c["clone_url"], str(mirror)], check=True, capture_output=True, timeout=900)
    fix = c["fix_commit"]
    try:
        git(mirror, "cat-file", "-e", f"{fix}^{{commit}}")
    except RuntimeError:
        git(mirror, "fetch", "-q", "origin", fix)
    parents = git(mirror, "rev-list", "--parents", "-n", "1", fix).split()
    parent = parents[1]
    rec["parent_commit"] = parent
    rec["fix_is_merge"] = len(parents) > 2
    changed = git(mirror, "diff", "--name-only", parent, fix).split()
    tests = [f for f in changed if is_test_path(f)]
    src = [f for f in changed if f not in tests and f.endswith(SRC_EXT[c["lang"]])]
    numstat = [ln.split("\t") for ln in git(mirror, "diff", "--numstat", parent, fix).splitlines()]
    rec["src_files"] = len(src)
    rec["src_added"] = sum(int(a) for a, d, p in numstat if p in src and a != "-")
    rec["src_deleted"] = sum(int(d) for a, d, p in numstat if p in src and d != "-")
    rec["test_files"] = len(tests)
    rec["test_lines"] = sum(int(a) + int(d) for a, d, p in numstat if p in tests and a != "-")
    if any(Path(f).name in MANIFESTS for f in changed):
        raise Reject("manifest")
    if work.exists():
        shutil.rmtree(work)
    git(mirror, "worktree", "prune")
    git(mirror, "worktree", "add", "-q", "--detach", str(work), parent)
    try:
        _run_states(c, rec, cache, env, slug, log_path, mirror, work, fix, tests, suite_timeout)
    finally:
        try:
            git(mirror, "worktree", "remove", "--force", str(work))
        except RuntimeError:
            shutil.rmtree(work, ignore_errors=True)


def _run_states(c: dict, rec: dict, cache: Path, env: dict, slug: str, log_path: Path,
                mirror: Path, work: Path, fix: str, tests: list[str], suite_timeout: int) -> None:
    rec["repo_files"] = len(git(work, "ls-files").splitlines())
    setup = detect_python(work) if c["lang"] == "python" else detect_node(work)
    rec["install"], rec["runner"] = setup["install"], setup["runner"]
    inst = sh(setup["install"], work, 600, env, network=True, log_path=log_path)
    rec["install_seconds"] = inst["seconds"]
    if inst["exit"] != 0:
        raise Reject("install", f"exit {inst['exit']}")

    runnable = [f for f in tests if f.endswith(SRC_EXT[c["lang"]])
                and (TEST_FILE_RE.search(f) or c["lang"] == "node" and "/test" in f)]
    if not runnable:
        raise Reject("no_test_file")

    def run_tests(label: str) -> tuple[dict, dict[str, tuple[str, str]]]:
        report = cache / "reports" / f"{slug}-{c['pr']}-{label}.out"
        report.parent.mkdir(exist_ok=True)
        report.unlink(missing_ok=True)
        present = [f for f in runnable if (work / f).exists()]
        res = sh(test_cmd(setup["runner"], present, report), work, 240, env, network=False,
                 log_path=log_path)
        return res, parse_report(setup["runner"], report, work)

    def build(label: str) -> None:
        if setup["build"]:
            b = sh(setup["build"], work, 300, env, network=False, log_path=log_path)
            if b["exit"] != 0:
                raise Reject("build", label)

    # Rouge : parent + tests du commit de correction (fichiers supprimés retirés).
    fix_tree = set(git(mirror, "ls-tree", "-r", "--name-only", fix).splitlines())
    for f in tests:
        if f in fix_tree:
            git(work, "checkout", fix, "--", f)
        elif (work / f).exists():
            (work / f).unlink()
    build("rouge")
    red_run, red = run_tests("red")
    git(work, "checkout", "-q", "-f", fix)
    build("vert")
    greens = []
    for i in range(GREEN_RUNS):
        green_run, green = run_tests(f"green{i + 1}")
        greens.append(green)
        rec.setdefault("green_seconds", []).append(green_run["seconds"])
        if not green:
            raise Reject("no_results", f"vert {i + 1}, exit {green_run['exit']}")
        failed = sorted(k for k, (st, _) in green.items() if st == "failed")
        if failed:
            rec["green_failed"] = failed[:20]
            raise Reject("green_failed", f"vert {i + 1} : {len(failed)} test(s)")
    passed = [{k for k, (st, _) in g.items() if st == "passed"} for g in greens]
    if any(p != passed[0] for p in passed[1:]):
        raise Reject("flaky", "tests passés différents entre les verts")
    f2p = sorted(k for k in passed[0] if red.get(k, ("missing", ""))[0] != "passed")
    p2p = sorted(k for k in passed[0] if red.get(k, ("", ""))[0] == "passed")
    rec["red_exit"] = red_run["exit"]
    rec["red_collected"] = len(red)
    rec["fail_to_pass"] = f2p
    rec["pass_to_pass_count"] = len(p2p)
    if not f2p:
        raise Reject("no_f2p")
    kinds = red_failure_type(f2p, red, red_run["tail"])
    rec["red_failure"] = kinds.most_common(1)[0][0]
    rec["red_failure_detail"] = ";".join(f"{k}:{n}" for k, n in sorted(kinds.items()))
    rec["targeted_seconds"] = max(rec["green_seconds"])

    # Suite complète sur l'état vert, réseau coupé : durée seule.
    suite = {"pytest": ".venv/bin/python -m pytest -q -p no:cacheprovider --no-header"}.get(
        setup["runner"], "npm test --silent")
    s = sh(suite, work, suite_timeout, env, network=False, log_path=log_path)
    rec["suite_seconds"] = None if s["timeout"] else s["seconds"]
    rec["suite_exit"] = s["exit"]


def shard_of(cands: list[dict], index: int, total: int) -> list[dict]:
    """Shards équilibrés et stables : dépôts entiers (un clone par dépôt), plus gros d'abord."""
    by_repo: dict[str, list[dict]] = {}
    for c in cands:
        by_repo.setdefault(c["repo"], []).append(c)
    loads = [0] * total
    chosen: list[dict] = []
    for repo in sorted(by_repo, key=lambda r: (-len(by_repo[r]), r)):
        i = loads.index(min(loads))
        loads[i] += len(by_repo[repo])
        if i == index - 1:
            chosen += by_repo[repo]
    return chosen


def validate(args: argparse.Namespace) -> None:
    global ISO
    cache = Path(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    if args.isolation == "sandbox-exec" and (PLATFORM != "macos"
                                             or not Path("/usr/bin/sandbox-exec").exists()):
        sys.exit("sandbox-exec indisponible : --isolation netns hors macOS")
    if args.isolation == "netns" and PLATFORM != "linux":
        sys.exit("--isolation netns : Linux seulement")
    ISO = Isolation(args.isolation, cache, args.mem_gb)
    env = base_env(cache, uv_no_build=args.uv_no_build)
    probe = probe_network(cache, env)

    _, cands = load_discovered(cache)
    if args.only:
        wanted = set(args.only)
        cands = [c for c in cands if c["pr_url"] in wanted]
    if args.lang:
        cands = [c for c in cands if c["lang"] == args.lang]
    if args.shard:
        index, total = (int(x) for x in args.shard.split("/"))
        cands = shard_of(cands, index, total)
    out = Path(args.out) if args.out else cache / "validated.json"
    done: dict[str, dict] = {}
    if out.exists() and not args.fresh:
        done = {r["pr_url"]: r for r in json.loads(out.read_text())["records"]}
    pending = [c for c in cands if c["pr_url"] not in done]
    # Petites corrections d'abord ; un dépôt à la fois par fil (verrou de clone).
    pending.sort(key=lambda c: (c["src_added"] + c["src_deleted"], c["size_kb"]))
    if args.limit:
        pending = pending[:args.limit]
    log(f"{len(pending)} candidats à valider ({len(done)} déjà faits), plateforme {PLATFORM}")
    deadline = time.monotonic() + args.deadline_min * 60 if args.deadline_min else None
    guard = threading.Lock()

    def save() -> None:
        out.write_text(json.dumps({"platform": PLATFORM, "isolation": args.isolation,
                                   "uv_no_build": args.uv_no_build, "probe": probe,
                                   "records": list(done.values())}, indent=1))

    def job(c: dict) -> None:
        if deadline and time.monotonic() > deadline:
            r = {k: c[k] for k in ("repo", "pr", "pr_url", "lang", "fix_commit")}
            r.update(platform=PLATFORM, ok=False, cause="deadline", reason=CAUSES["deadline"])
        else:
            r = validate_one(c, cache, env, args.suite_timeout)
        r["validated_at"] = now()
        with guard:
            done[c["pr_url"]] = r
            save()
            ok = sum(1 for x in done.values() if x["ok"])
            log(f"{'OK ' if r['ok'] else 'KO '} {c['repo']}#{c['pr']} {r['reason']} "
                f"(retenus : {ok})")

    save()
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        list(pool.map(job, pending))
    if ISO.proxy is not None:
        refused = sorted({e["host"] for e in __import__("proxies").read_log(
            cache / "logs" / "connect.ndjson") if not e.get("accepted") and e.get("host")})
        log(f"proxy CONNECT : hôtes refusés {refused or 'aucun'}")
        ISO.proxy.shutdown()


# --- Fusion et CSV ----------------------------------------------------------------------

CSV_FIELDS = ["platform", "lang", "repo", "fix_commit", "parent_commit", "license", "stars",
              "repo_size_kb", "repo_files", "src_files", "src_lines_added", "src_lines_deleted",
              "test_files", "test_lines", "f2p_count", "fail_to_pass", "p2p_count",
              "red_failure", "red_failure_detail", "runner", "install", "targeted_test_s",
              "suite_s", "suite_exit", "pr_url", "issue_url", "merged_at", "title"]


def write_csv(args: argparse.Namespace) -> None:
    cache = Path(args.cache)
    _, cands = load_discovered(cache)
    disc = {c["pr_url"]: c for c in cands}
    paths = [Path(p) for p in args.validated] or [cache / "validated.json"]
    records: dict[str, dict] = {}
    probes = []
    for p in paths:
        data = json.loads(p.read_text())
        probes.append((p.name, data["platform"], data["isolation"], data["probe"]))
        for r in data["records"]:
            records[r["pr_url"]] = r
    rows = []
    for r in records.values():
        if not r["ok"]:
            continue
        c = disc[r["pr_url"]]
        rows.append({
            "platform": r["platform"], "lang": r["lang"], "repo": r["repo"],
            "fix_commit": r["fix_commit"], "parent_commit": r["parent_commit"],
            "license": c["license"], "stars": c["stars"], "repo_size_kb": c["size_kb"],
            "repo_files": r["repo_files"], "src_files": r["src_files"],
            "src_lines_added": r["src_added"], "src_lines_deleted": r["src_deleted"],
            "test_files": r["test_files"], "test_lines": r["test_lines"],
            "f2p_count": len(r["fail_to_pass"]), "fail_to_pass": " | ".join(r["fail_to_pass"]),
            "p2p_count": r["pass_to_pass_count"], "red_failure": r["red_failure"],
            "red_failure_detail": r["red_failure_detail"], "runner": r["runner"],
            "install": r["install"].split(" && ")[0], "targeted_test_s": r["targeted_seconds"],
            "suite_s": r["suite_seconds"] if r["suite_seconds"] is not None
            else f">{args.suite_timeout}",
            "suite_exit": r["suite_exit"], "pr_url": r["pr_url"], "issue_url": c["issue_url"],
            "merged_at": c["merged_at"], "title": c["title"]})
    rows.sort(key=lambda x: (x["lang"], x["src_files"], x["src_lines_added"] + x["src_lines_deleted"]))
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=CSV_FIELDS)
        w.writeheader()
        w.writerows(rows)

    by_lang = Counter(x["lang"] for x in rows)
    total = len(records)
    causes = Counter(r["cause"] for r in records.values() if not r["ok"])
    lines = ["# Minage T07 : validation", "",
             f"Candidats validés : {total} ; retenus : {len(rows)} "
             f"(python {by_lang['python']}, node {by_lang['node']}).", "",
             "| Minimum du ticket | Valeur | Atteint |", "|---|---|---|",
             f"| 25 candidats | {len(rows)} | {'oui' if len(rows) >= 25 else 'non'} |",
             f"| 8 Python | {by_lang['python']} | {'oui' if by_lang['python'] >= 8 else 'non'} |",
             f"| 6 TS/JS | {by_lang['node']} | {'oui' if by_lang['node'] >= 6 else 'non'} |", "",
             "| Cause de rejet | Nombre | Part des validés |", "|---|---|---|"]
    for cause, n in causes.most_common():
        lines.append(f"| {CAUSES[cause]} | {n} | {100 * n / total:.1f} % |")
    lines += ["", "| Type d'échec rouge (retenus) | Nombre |", "|---|---|"]
    for kind, n in Counter(x["red_failure"] for x in rows).most_common():
        lines.append(f"| {kind} | {n} |")
    lines += ["", "| Shard | Plateforme | Isolation | Sonde réseau coupé |", "|---|---|---|---|"]
    for name, plat, iso, pr in probes:
        lines.append(f"| {name} | {plat} | {iso} | curl exit {pr['cut_exit']} "
                     f"({'coupé' if pr['network_cut'] else 'NON COUPÉ'}) |")
    summary = "\n".join(lines) + "\n"
    if args.summary:
        Path(args.summary).write_text(summary)
    print(summary)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--cache", default=str(Path.home() / ".cache" / "yusagent-smoke" / "mine"))
    sub = p.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("discover")
    d.add_argument("--since", default="2026-08-01")
    d.add_argument("--refresh-repos", action="store_true")
    d.add_argument("--workers", type=int, default=6)
    d.add_argument("--target-python", type=int, default=90)
    d.add_argument("--target-node", type=int, default=90)
    v = sub.add_parser("validate")
    v.add_argument("--isolation", choices=["sandbox-exec", "netns"], default="sandbox-exec",
                   help="sandbox-exec (macOS, mine.sb) ou netns (Linux, runner jetable)")
    v.add_argument("--uv-no-build", action=argparse.BooleanOptionalAction, default=True,
                   help="UV_NO_BUILD=1 : aucune dépendance construite depuis un sdist")
    v.add_argument("--workers", type=int, default=4)
    v.add_argument("--shard", help="I/N : I-ème shard sur N (dépôts entiers)")
    v.add_argument("--only", nargs="+", help="URL de PR à valider (shortlist)")
    v.add_argument("--limit", type=int, default=0)
    v.add_argument("--lang", choices=["python", "node"])
    v.add_argument("--out", help="sortie JSON (défaut : <cache>/validated.json)")
    v.add_argument("--fresh", action="store_true", help="ignore une sortie existante")
    v.add_argument("--deadline-min", type=float, default=0,
                   help="plus de nouveau candidat après N minutes")
    v.add_argument("--suite-timeout", type=int, default=120)
    v.add_argument("--mem-gb", type=float, default=3,
                   help="netns : mémoire max par commande réseau coupé (Go)")
    c = sub.add_parser("csv")
    c.add_argument("--validated", nargs="*", default=[])
    c.add_argument("--out", default=str(OUT_CSV))
    c.add_argument("--summary")
    c.add_argument("--suite-timeout", type=int, default=120)
    args = p.parse_args()
    {"discover": discover, "validate": validate, "csv": write_csv}[args.cmd](args)


if __name__ == "__main__":
    main()
