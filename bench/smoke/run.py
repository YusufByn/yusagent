#!/usr/bin/env python3
"""Smoke bench local (T06, YA-14) : tâches courtes, vrai modèle par abonnement, rapport.

Usage (voir README.md) :
    python3 -I bench/smoke/run.py --binary <dossier>/prime-agent [--model gpt-6.1-sol] ...
    python3 -I bench/smoke/run.py --binary ... --capture        # validation sans modèle
    python3 -I bench/smoke/run.py --rereport <run>              # rapport recalculé

Déroulé :
  1. Contrôles : modèle dans la liste fermée, présent au catalogue du binaire
     (`models.bundled.json`) ou déclaré par une fiche de `models/openai-codex.json`.
  2. Préparation (réseau autorisé) : venv du kernel (une fois par binaire, contre le
     serveur de capture), puis chaque tâche (copie ou clone, patch, `setup`, contrôle
     que `fail_to_pass` échoue et `pass_to_pass` passe sur l'état cassé).
  3. Sonde réseau (sandbox, serveur de capture) : réseau coupé, CONNECT hors liste
     refusé, variables de proxy vues par le kernel (noms seuls).
  4. Run de chauffe (sandbox, même accès que la phase agent) : le modèle doit répondre,
     sinon la préparation échoue (id faux, jeton expiré) ; rafraîchit le jeton OAuth.
  5. Phase agent (sandbox, localhost seulement) puis notation (`hidden/`, réseau coupé).
  6. Rapport : `runs/<horodatage UTC>-<sha court>-<modèle>/`. Les sessions des
     sous-agents (`rlm.spawn`) comptent dans les totaux ; leur part est donnée à part.

Accès au modèle pendant les phases sous sandbox : `baseUrl` du provider vers le reverse
proxy local (`proxies.py`, cible fixe chatgpt.com), WebSocket et SSE ; `HTTPS_PROXY`
vers le proxy CONNECT à liste blanche (`hosts/<provider>.txt`) pour le rafraîchissement
du jeton. En mode `--capture`, la cible amont du reverse proxy est le serveur de capture.

Bibliothèque standard uniquement ; macOS (`sandbox-exec`) requis.
"""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import html
import importlib.util
import json
import os
import platform
import re
import shutil
import signal
import statistics
import subprocess
import sys
import time
import tomllib
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
CAPTURE = REPO / "bench" / "tools" / "capture"
SANDBOX = CAPTURE / "sandbox.sb"
# Phase agent : même réseau, plus aucune lecture de bench/smoke/ ni du cache des tâches
# hors workspace (tests cachés).
SANDBOX_AGENT = HERE / "sandbox-agent.sb"
# Code des dépôts de tâche (`setup`, `test`) : écriture et lecture de $HOME limitées au
# dossier de la tâche et aux caches, réseau par le proxy à liste blanche (T07).
SANDBOX_TASK = HERE / "sandbox-task.sb"
sys.path.insert(0, str(HERE))

import metrics  # noqa: E402
import proxies  # noqa: E402

PROVIDER = "openai-codex"
# Liste fermée (décision du 04/11/2026) : tout autre MODEL est refusé, même présent
# dans models.bundled.json. gpt-6-astra seulement sur demande explicite.
MODELS = ("gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-luna", "gpt-6-astra")
DEFAULT_MODELS = ("gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-luna")
DEFAULT_EFFORT = "high"
MODEL_FICHES = HERE / "models" / f"{PROVIDER}.json"
FICHE_FIELDS = ("contextWindow", "maxTokens", "thinkingLevelMap", "reasoning")
PRICES = HERE / "prices.json"
HOSTS = HERE / "hosts" / f"{PROVIDER}.txt"
SETUP_HOSTS = HERE / "hosts" / "setup.txt"
TASKS = HERE / "tasks"
RUNS = HERE / "runs"
DEFAULT_CACHE = Path.home() / ".cache" / "yusagent-smoke"
# TTL du cache fixé (CLAUDE.md, boucle de dev) : variable et réglage, comme le banc de
# capture. Le corps Codex ne porte pas de rétention (pas de `prompt_cache_retention`) :
# le tarif d'écriture 30 min est retenu pour le coût théorique, hypothèse notée au rapport.
CACHE_RETENTION = "short"
CACHE_WRITE_PRICE = "cache_write_30m"
WARMUP_PROMPT = "Réponds uniquement : OK."
PROBE_PROMPT = "Exécute la sonde réseau."
GRADE_TIMEOUT_S = 300
# Garde-fou d'inactivité (YA-14) : une tâche dont aucun octet ne passe par le reverse proxy
# pendant ce délai est arrêtée, statut `stalled` (flux bloqué, incident écarté des A/B).
# Réglable (`--idle-timeout`, 0 : désactivé), noté dans l'en-tête du rapport.
IDLE_TIMEOUT_S = 300
PREP_TIMEOUT_S = 900
VALIDATE_RUNS = 3  # exécutions de chaque vérification de --validate-gold (instabilité)
HARD_MIN_TURNS = 10  # tâche `difficile` résolue en moins de tours : signalée (T07)
# Fichiers d'une session : `<id>.jsonl` et `<id>.usage.ndjson` (id UUID du harness).
SESSION_FILE = re.compile(r"^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})"
                          r"\.(?:jsonl|usage\.ndjson)$")


def load_capture_run():
    """`bench/tools/capture/run.py` : valeurs factices partagées (test d'absence de secret)."""
    spec = importlib.util.spec_from_file_location("capture_run", CAPTURE / "run.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CAPTURE_RUN = load_capture_run()
FAKE_SECRETS = CAPTURE_RUN.FAKE_SECRETS


def b64(obj: dict) -> str:
    return base64.urlsafe_b64encode(json.dumps(obj).encode()).rstrip(b"=").decode()


# Identifiant OAuth factice du mode capture : un JWT non signé portant le compte
# ChatGPT que le provider relit (crates/pa-ai/src/providers/openai_codex_responses/request.rs:37).
FAKE_ACCESS = ".".join([b64({"alg": "none"}),
                        b64({"https://api.openai.com/auth": {"chatgpt_account_id": "acct-smoke"}}),
                        "smoke-fake-signature"])
FAKE_REFRESH = "smoke-fake-refresh-not-a-token"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_hash(root: Path) -> str:
    """Empreinte d'un dossier de tâche (version de la tâche, clé de sa préparation)."""
    return files_hash((p.relative_to(root), p.read_bytes()) for p in root.rglob("*")
                      if p.is_file() and "__pycache__" not in p.parts)


def files_hash(files) -> str:
    digest = hashlib.sha256()
    for rel, data in sorted(files, key=lambda item: item[0]):
        digest.update(str(rel).encode() + b"\0")
        digest.update(data + b"\0")
    return digest.hexdigest()


def tree_hash_at(commit: str, root: Path) -> str | None:
    """`tree_hash` d'un dossier du dépôt tel qu'il était au commit donné (fichiers suivis
    par git) ; `None` si le commit ou le dossier est introuvable."""
    rel = root.resolve().relative_to(REPO).as_posix()
    listed = subprocess.run(["git", "-C", str(REPO), "ls-tree", "-r", "-z", "--name-only",
                             commit, "--", rel], capture_output=True)
    names = [n for n in listed.stdout.decode().split("\0") if n]
    if listed.returncode != 0 or not names:
        return None
    files = []
    for name in names:
        path = Path(name).relative_to(rel)
        if "__pycache__" in path.parts:
            continue
        shown = subprocess.run(["git", "-C", str(REPO), "show", f"{commit}:{name}"],
                               capture_output=True, check=True)
        files.append((path, shown.stdout))
    return files_hash(files)


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def fail(message: str) -> None:
    sys.exit(f"smoke : {message}")


# --- Contrôles ---------------------------------------------------------------


def catalog(binary: Path) -> tuple[Path, set[str]]:
    bundled = binary.parent / "models.bundled.json"
    if not bundled.exists():
        fail(f"{bundled} introuvable (catalogue livré à côté du binaire)")
    data = json.loads(bundled.read_text())
    entries = data["models"] if isinstance(data["models"], list) else list(data["models"].values())
    return bundled, {m.get("id") for m in entries if m.get("provider") == PROVIDER}


def model_source(model: str, bundled_ids: set[str]) -> dict:
    """D'où vient la fiche du modèle. Jamais le repli sur la fiche du modèle par défaut."""
    if model not in MODELS:
        fail(f"MODEL={model} refusé : liste fermée {', '.join(MODELS)}")
    if model in bundled_ids:
        return {"source": "bundled"}
    fiches = json.loads(MODEL_FICHES.read_text())["models"] if MODEL_FICHES.exists() else []
    fiche = next((f for f in fiches if f.get("id") == model), None)
    if fiche is None or any(fiche.get(field) in (None, {}) for field in FICHE_FIELDS):
        fail(f"{model} absent de models.bundled.json et sans fiche complète dans "
             f"{MODEL_FICHES.relative_to(REPO)} ({', '.join(FICHE_FIELDS)})")
    return {"source": "declared", "fiche": {k: v for k, v in fiche.items() if k != "_source"},
            "fiche_source": fiche.get("_source")}


def load_tasks(selected: list[str] | None, capture: bool) -> list[dict]:
    tasks = []
    for toml_path in sorted(TASKS.glob("*/task.toml")):
        task = tomllib.loads(toml_path.read_text())
        task["dir"] = toml_path.parent
        tasks.append(task)
    if selected:
        known = {t["id"] for t in tasks}
        unknown = sorted(set(selected) - known)
        if unknown:
            fail(f"tâches inconnues : {', '.join(unknown)}")
        return [t for t in tasks if t["id"] in selected]
    # Par défaut : les tâches du banc ; les tâches d'exemple seulement en mode capture.
    chosen = [t for t in tasks if ("exemple" in t.get("tags", [])) == capture]
    if not chosen:
        fail("aucune tâche sélectionnée (TASKS=id1,id2)")
    return chosen


# --- Environnement et lancement ---------------------------------------------


def tool_path() -> str:
    dirs = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
    for tool in ("uv", "node", "git", "python3", "curl"):
        found = shutil.which(tool)
        if found and str(Path(found).parent) not in dirs:
            dirs.append(str(Path(found).parent))
    return ":".join(dirs)


class Cache:
    def __init__(self, root: Path, binary: Path | None, capture: bool):
        self.root = root
        self.binary_sha = sha256(binary) if binary else "aucun-binaire"
        self.home = root / "home"
        self.tmp = root / "tmp"
        self.kernel_venv = root / "kernel-venv" / self.binary_sha[:16]
        # Dossier agent dédié, login fait une fois par Yusuf (README). Le mode capture et
        # la sonde utilisent leurs propres dossiers, avec un identifiant factice.
        self.agent = root / ("capture-agent" if capture else "agent")
        self.probe_agent = root / "probe-agent"
        self.uv_cache = root / "uv-cache"
        self.uv_python = root / "uv-python"
        self.npm_cache = root / "npm-cache"
        for path in (self.home, self.tmp, self.uv_cache, self.uv_python, self.npm_cache):
            path.mkdir(parents=True, exist_ok=True)


def task_env(cache: Cache, **extra: str) -> dict:
    """Environnement du code des dépôts de tâche (`setup`, `test`)."""
    return {"PATH": tool_path(), "HOME": str(cache.home), "TMPDIR": str(cache.tmp),
            "LANG": "en_US.UTF-8", "DO_NOT_TRACK": "1", "UV_CACHE_DIR": str(cache.uv_cache),
            "UV_PYTHON_INSTALL_DIR": str(cache.uv_python), "npm_config_cache": str(cache.npm_cache),
            "npm_config_audit": "false", "npm_config_fund": "false",
            "npm_config_update_notifier": "false", "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1", **extra}


def task_profile(cache: Cache, task: dict) -> list[str]:
    """Arguments de sandbox-exec pour le code d'une tâche (`sandbox-task.sb`)."""
    node = shutil.which("node")
    params = {"HOME_DIR": Path.home().resolve(),
              "TASK_CACHE": (cache.root / "tasks" / task["id"]).resolve(),
              "UV_CACHE": cache.uv_cache.resolve(), "UV_PYTHON": cache.uv_python.resolve(),
              "NPM_CACHE": cache.npm_cache.resolve(), "SMOKE_HOME": cache.home.resolve(),
              "SMOKE_TMP": cache.tmp.resolve(),
              "NODE_HOME": Path(node).resolve().parents[1] if node else "/nonexistent"}
    return ["-f", str(SANDBOX_TASK), *[a for k, v in params.items() for a in ("-D", f"{k}={v}")]]


def agent_env(cache: Cache, agent_dir: Path, connect_port: int | None, capture: bool) -> dict:
    env = {
        "PATH": tool_path(),
        "HOME": str(cache.home),
        "TMPDIR": str(cache.tmp),
        "LANG": "en_US.UTF-8",
        "DO_NOT_TRACK": "1",
        "PI_OFFLINE": "1",
        "PI_SKIP_VERSION_CHECK": "1",
        "PI_CACHE_RETENTION": CACHE_RETENTION,
        "PRIME_AGENT_CODING_AGENT_DIR": str(agent_dir),
        "PRIME_AGENT_KERNEL_VENV": str(cache.kernel_venv),
        "NO_PROXY": "127.0.0.1,localhost",
    }
    if connect_port is not None:
        env["HTTPS_PROXY"] = f"http://127.0.0.1:{connect_port}"
    if capture:
        env.update(FAKE_SECRETS)
    return env


def write_fake_auth(agent_dir: Path) -> None:
    agent_dir.mkdir(parents=True, exist_ok=True)
    (agent_dir / "auth.json").write_text(json.dumps({PROVIDER: {
        "type": "oauth", "access": FAKE_ACCESS, "refresh": FAKE_REFRESH,
        "expires": 4102444800000, "accountId": "acct-smoke"}}))


def write_agent_config(agent_dir: Path, proxy_port: int, source: dict, settings: dict) -> None:
    """models.json (baseUrl vers le reverse proxy, fiche déclarée) et settings.json."""
    agent_dir.mkdir(parents=True, exist_ok=True)
    provider = {"baseUrl": f"http://127.0.0.1:{proxy_port}/backend-api"}
    if source["source"] == "declared":
        provider["models"] = [source["fiche"]]
    (agent_dir / "models.json").write_text(json.dumps({"providers": {PROVIDER: provider}},
                                                      indent=2) + "\n")
    (agent_dir / "settings.json").write_text(
        json.dumps({"cacheRetention": CACHE_RETENTION, **settings}, indent=2) + "\n")


def agent_profile(cache: "Cache", workspace: Path, binary: Path | None,
                  agent_dir: Path) -> list[str]:
    """Arguments de sandbox-exec pour une session d'agent dont le workspace est donné."""
    tasks = (cache.root / "tasks").resolve()
    workspace = workspace.resolve()
    task_dir = workspace.parent if workspace.parent.parent == tasks else tasks
    node = shutil.which("node")
    params = {"HOME_DIR": Path.home().resolve(), "WORKSPACE_DIR": workspace,
              "AGENT_DIR": agent_dir.resolve(),
              "BIN_DIR": binary.resolve().parent if binary else "/nonexistent",
              "SMOKE_HOME": cache.home.resolve(), "SMOKE_TMP": cache.tmp.resolve(),
              "KERNEL_VENV": cache.kernel_venv.resolve(), "UV_PYTHON": cache.uv_python.resolve(),
              "NODE_HOME": Path(node).resolve().parents[1] if node else "/nonexistent",
              "SMOKE_DIR": HERE, "TASKS_DIR": tasks, "TASK_DIR": task_dir,
              "SOURCES_DIR": (cache.root / "sources").resolve()}
    return ["-f", str(SANDBOX_AGENT), *[a for k, v in params.items() for a in ("-D", f"{k}={v}")]]


def run_process(cmd: list[str], env: dict, cwd: Path, out: Path, err: Path, timeout: int,
                sandboxed: bool, profile: list[str] | None = None,
                idle: tuple | None = None) -> dict:
    """Lance `cmd` ; sous sandbox, profil réseau du banc de capture sauf `profile` donné.

    `idle` : (fonction qui rend les secondes d'inactivité du reverse proxy, limite en s).
    Au-delà de la limite, le processus est arrêté et le résultat porte `stalled`.
    """
    if sandboxed:
        cmd = ["/usr/bin/sandbox-exec", *(profile or ["-f", str(SANDBOX)]), *cmd]
    started = time.monotonic()
    timed_out = stalled = False
    code = None
    with out.open("w") as stdout, err.open("w") as stderr:
        proc = subprocess.Popen(cmd, env=env, cwd=cwd, stdin=subprocess.DEVNULL, stdout=stdout,
                                stderr=stderr, start_new_session=True)
        while True:
            remaining = timeout - (time.monotonic() - started)
            try:
                code = proc.wait(timeout=max(0.01, min(1.0, remaining)))
                break
            except subprocess.TimeoutExpired:
                pass
            if time.monotonic() - started >= timeout:
                timed_out = True
            elif idle is not None and idle[1] and idle[0]() >= idle[1]:
                stalled = True
            if timed_out or stalled:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
                code = None
                break
    result = {"exit": code, "timeout": timed_out, "seconds": round(time.monotonic() - started, 2)}
    if idle is not None:
        result["stalled"] = stalled
        result["idle_timeout_s"] = idle[1]
    return result


def agent_cmd(binary: Path, model: str, effort: str, prompt: str) -> list[str]:
    return [str(binary), "-p", "--mode", "json", "--provider", PROVIDER, "--model", model,
            "--thinking", effort, prompt]


def session_files(agent_dir: Path) -> dict[Path, int]:
    sessions = agent_dir / "sessions"
    if not sessions.exists():
        return {}
    return {p: p.stat().st_size for p in sessions.rglob("*")
            if p.is_file() and (p.name.endswith(".usage.ndjson") or p.suffix == ".jsonl")}


def subagent_files(agent_dir: Path, session_ids: set[str]) -> list[Path]:
    """Sessions des sous-agents (`rlm.spawn`) des sessions données, à toute profondeur.

    Le harness ne les écrit pas sous `sessions/` mais sous
    `session-artifacts/<parent>/sub-<id>/<session>.jsonl` (et `.usage.ndjson`) ; les
    `session-artifacts/<enfant>/` imbriqués et ceux du dossier racine sont suivis aussi.
    """
    found: dict[Path, None] = {}
    pending, seen = sorted(session_ids), set()
    while pending:
        sid = pending.pop()
        if sid in seen:
            continue
        seen.add(sid)
        root = agent_dir / "session-artifacts" / sid
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            match = SESSION_FILE.match(path.name)
            if path.is_file() and match and path.parent.name.startswith("sub-"):
                found[path] = None
                pending.append(match.group(1))
    return list(found)


def copy_new_sessions(agent_dir: Path, before: dict[Path, int], dest: Path) -> dict:
    """Copie dans le run les sessions de `sessions/` créées ou agrandies, puis celles de
    leurs sous-agents (`subagent_files`) dans `subagents/`."""
    dest.mkdir(parents=True, exist_ok=True)
    usage, sessions = [], []
    for path, size in sorted(session_files(agent_dir).items()):
        if before.get(path) == size:
            continue
        target = dest / path.name
        shutil.copyfile(path, target)
        (usage if path.name.endswith(".usage.ndjson") else sessions).append(target)
    subs = copy_subagents(agent_dir, usage + sessions, dest / "subagents")
    return {"usage": usage, "sessions": sessions, **subs}


def copy_subagents(agent_dir: Path, main_files: list[Path], dest: Path) -> dict:
    ids = {m.group(1) for m in (SESSION_FILE.match(p.name) for p in main_files) if m}
    sub_usage, sub_sessions = [], []
    for path in subagent_files(agent_dir, ids):
        dest.mkdir(parents=True, exist_ok=True)
        target = dest / path.name
        shutil.copyfile(path, target)
        (sub_usage if path.name.endswith(".usage.ndjson") else sub_sessions).append(target)
    return {"sub_usage": sub_usage, "sub_sessions": sub_sessions}


class Capture:
    """Serveur de capture (`bench/tools/capture/server.py`), un par run de tâche."""

    def __init__(self, out: Path, scenario: str, connect_port: int, server_args: list[str]):
        out.mkdir(parents=True, exist_ok=True)
        port_file = out / "port"
        self.log = (out / "server.log").open("w")
        self.proc = subprocess.Popen(
            [sys.executable, "-I", str(CAPTURE / "server.py"), "--out", str(out / "capture"),
             "--scenario", scenario, "--port-file", str(port_file),
             "--connect-proxy-port", str(connect_port), *server_args],
            stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 10
        while not port_file.exists() or not port_file.read_text().strip():
            if time.monotonic() > deadline or self.proc.poll() is not None:
                fail("le serveur de capture n'a pas démarré")
            time.sleep(0.05)
        self.port = int(port_file.read_text())

    def stop(self) -> None:
        self.proc.send_signal(signal.SIGINT)
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.log.close()


class Access:
    """Reverse proxy (et serveur de capture en mode capture) pour une session d'agent."""

    def __init__(self, out: Path, capture: bool, scenario: str, connect_port: int,
                 server_args: list[str] | None = None):
        self.capture = (Capture(out, scenario, connect_port, server_args or []) if capture
                        else None)
        upstream = ("127.0.0.1", self.capture.port, False) if self.capture else None
        self.log_path = out / "proxy.ndjson"
        self.proxy = proxies.ReverseProxy(self.log_path, upstream)
        proxies.serve(self.proxy)

    @property
    def port(self) -> int:
        return self.proxy.port

    def stop(self) -> None:
        self.proxy.shutdown()
        self.proxy.server_close()
        if self.capture:
            self.capture.stop()


# --- Préparation --------------------------------------------------------------


def prepare_kernel(binary: Path, cache: Cache) -> None:
    """Venv du kernel (réseau autorisé), une fois par binaire, contre le serveur de capture."""
    marker = cache.kernel_venv.parent / f"{cache.binary_sha[:16]}.prepared"
    if marker.exists():
        return
    print("préparation : venv du kernel (réseau autorisé, serveur de capture)")
    out = cache.root / "kernel-warmup"
    shutil.rmtree(out, ignore_errors=True)
    (out / "project").mkdir(parents=True)
    write_fake_auth(cache.probe_agent)
    access = Access(out, True, "smoke-kernel", 9)
    try:
        write_agent_config(cache.probe_agent, access.port, {"source": "bundled"}, {})
        env = agent_env(cache, cache.probe_agent, None, capture=True)
        result = run_process(agent_cmd(binary, DEFAULT_MODELS[0], DEFAULT_EFFORT, PROBE_PROMPT),
                             env, out / "project", out / "out.jsonl", out / "err.log",
                             PREP_TIMEOUT_S, sandboxed=False)
    finally:
        access.stop()
    if result["exit"] != 0 or not (cache.kernel_venv / "pyvenv.cfg").exists():
        fail(f"venv du kernel en échec ({result}), voir {out}/err.log")
    marker.write_text(json.dumps({"binary_sha256": cache.binary_sha}) + "\n")


def junit_results(path: Path, workspace: Path) -> dict[str, str]:
    """Résultat par identifiant de test. Clés : `fichier::nom`, `classe::nom`, `nom`."""
    if not path.exists():
        return {}
    results = {}
    for case in ET.parse(path).getroot().iter("testcase"):
        # Node 24.21 (macOS) échappe deux fois les guillemets du nom : `&quot;` reste.
        name = html.unescape(case.get("name", ""))
        failed = any(child.tag in ("failure", "error") for child in case) or case.get("failure")
        skipped = any(child.tag == "skipped" for child in case)
        status = "failed" if failed else "skipped" if skipped else "passed"
        keys = {name}
        if case.get("classname"):
            keys.add(f"{case.get('classname')}::{name}")
        file = case.get("file")
        if file:
            rel = Path(file)
            if rel.is_absolute():
                try:
                    rel = rel.resolve().relative_to(workspace.resolve())
                except ValueError:
                    pass
            keys.add(f"{rel.as_posix()}::{name}")
        for key in keys:
            results[key] = status
    return results


def run_tests(task: dict, workspace: Path, out: Path, cache: Cache, sandboxed: bool,
              profile: list[str] | None = None) -> dict:
    """Copie `hidden/` dans le workspace et lance `test` ; résultat par identifiant."""
    hidden = task["dir"] / "hidden"
    if hidden.exists():
        shutil.copytree(hidden, workspace, dirs_exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    # Rapport dans le TMPDIR du banc (inscriptible sous les deux profils), copié ensuite.
    junit = cache.tmp / f"junit-{task['id']}.xml"
    junit.unlink(missing_ok=True)
    env = task_env(cache, SMOKE_JUNIT=str(junit), UV_OFFLINE="1")
    result = run_process(["/bin/sh", "-c", task["test"]], env, workspace, out / "test.out",
                         out / "test.err", GRADE_TIMEOUT_S, sandboxed,
                         profile=profile or task_profile(cache, task))
    if junit.exists():
        shutil.copyfile(junit, out / "junit.xml")
    found = junit_results(junit, workspace)
    tests = {tid: found.get(tid, "missing")
             for tid in task.get("fail_to_pass", []) + task.get("pass_to_pass", [])}
    return {"process": result, "tests": tests, "junit": junit.exists()}


def task_paths(task: dict, cache: Cache) -> tuple[Path, Path, Path]:
    key = tree_hash(task["dir"])[:16]
    base = cache.root / "tasks" / task["id"]
    # Le workspace garde le même chemin à chaque run : les venvs créés par `setup`
    # contiennent des chemins absolus.
    return base / "workspace", base / f"pristine-{key}", base / f"prepared-{key}.json"


def prepare_task(task: dict, cache: Cache) -> dict:
    workspace, pristine, marker = task_paths(task, cache)
    if marker.exists():
        return json.loads(marker.read_text())
    print(f"préparation : tâche {task['id']} (setup par le proxy à liste blanche)")
    shutil.rmtree(workspace, ignore_errors=True)
    shutil.rmtree(pristine, ignore_errors=True)
    source = task["source"]
    if "path" in source:
        shutil.copytree(task["dir"] / source["path"], workspace, symlinks=True)
    else:
        clone = cache.root / "sources" / task["id"]
        if not clone.exists():
            subprocess.run(["git", "clone", "--quiet", source["git"], str(clone)], check=True)
        subprocess.run(["git", "-C", str(clone), "fetch", "--quiet", "origin", source["commit"]],
                       check=False)
        shutil.copytree(clone, workspace, symlinks=True)
        subprocess.run(["git", "-C", str(workspace), "checkout", "--quiet", "--detach",
                        source["commit"]], check=True)
    if source.get("patch"):
        subprocess.run(["git", "apply", str(task["dir"] / source["patch"])], cwd=workspace,
                       check=True)
    # Fichiers de la tâche copiés avant `setup` (versions figées : uv.lock).
    for name in task.get("files", []):
        shutil.copyfile(task["dir"] / "files" / name, workspace / name)
    logs = cache.root / "tasks" / task["id"] / "prepare"
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "connect.ndjson").unlink(missing_ok=True)  # une préparation, un journal
    connect = proxies.ConnectProxy(logs / "connect.ndjson", proxies.load_hosts(SETUP_HOSTS))
    proxies.serve(connect)
    url = f"http://127.0.0.1:{connect.port}"
    env = task_env(cache, HTTPS_PROXY=url, HTTP_PROXY=url, https_proxy=url, http_proxy=url,
                   npm_config_https_proxy=url, npm_config_proxy=url,
                   NO_PROXY="127.0.0.1,localhost")
    try:
        setup = run_process(["/bin/sh", "-c", task.get("setup", "true")], env, workspace,
                            logs / "setup.out", logs / "setup.err", PREP_TIMEOUT_S,
                            sandboxed=True, profile=task_profile(cache, task))
    finally:
        connect.shutdown()
        connect.server_close()
    refused = sorted({e["host"] for e in proxies.read_log(logs / "connect.ndjson")
                      if not e.get("accepted") and e.get("host")})
    if setup["exit"] != 0:
        fail(f"tâche {task['id']} : setup en échec, voir {logs}/setup.err "
             f"(hôtes refusés : {refused or 'aucun'})")
    if "git" in source:
        reset_history(workspace)
    shutil.copytree(workspace, pristine, symlinks=True)
    leaks = history_leaks(task, cache, pristine) if "git" in source else None
    if leaks and leaks["leaks"]:
        fail(f"tâche {task['id']} : trace du correctif dans le workspace : {leaks['leaks']}")
    graded = run_tests(task, workspace, logs, cache, sandboxed=True)
    wrong = {tid: status for tid, status in graded["tests"].items()
             if status != ("failed" if tid in task.get("fail_to_pass", []) else "passed")}
    if wrong:
        fail(f"tâche {task['id']} invalide sur l'état cassé : {wrong} (attendu : fail_to_pass "
             f"en échec, pass_to_pass en succès), voir {logs}")
    info = {"task": task["id"], "tree": tree_hash(task["dir"]), "broken_state": graded["tests"],
            "setup_refused_hosts": refused, "history_check": leaks}
    marker.write_text(json.dumps(info, indent=2) + "\n")
    return info


def reset_history(workspace: Path) -> None:
    """Historique du dépôt remplacé par un seul commit : le commit de correction et tout ce
    qui le suit ne doivent pas être lisibles par l'agent (`git log --all`, `git show`)."""
    shutil.rmtree(workspace / ".git")
    quiet = {"cwd": workspace, "check": True, "capture_output": True}
    subprocess.run(["git", "init", "-q"], **quiet)
    (workspace / ".git" / "info" / "exclude").write_text(".venv/\nnode_modules/\n")
    subprocess.run(["git", "add", "-A"], **quiet)
    subprocess.run(["git", "-c", "user.name=smoke", "-c", "user.email=smoke@localhost",
                    "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null",
                    "-c", "gc.auto=0",  # pas de packfile : objets libres, vérifiables un à un
                    "commit", "-q", "-m", "base"], **quiet)


def history_leaks(task: dict, cache: Cache, workspace: Path) -> dict:
    """Le workspace ne doit contenir aucune trace du correctif : un seul commit atteignable
    (reflog compris), ni packfile, ni ORIG_HEAD/FETCH_HEAD/packed-refs, ni objet
    inaccessible, ni blob de la version corrigée d'un fichier du correctif (base d'objets
    ou fichier de l'arbre, `.venv` et `node_modules` compris)."""
    def git_ws(*args: str) -> subprocess.CompletedProcess:
        return subprocess.run(["git", "-C", str(workspace), *args], capture_output=True, text=True)
    leaks = []
    commits = git_ws("rev-list", "--all", "--reflog").stdout.split()
    if len(commits) != 1:
        leaks.append(f"{len(commits)} commits atteignables")
    for name in ("ORIG_HEAD", "FETCH_HEAD", "packed-refs"):
        if (workspace / ".git" / name).exists():
            leaks.append(f".git/{name}")
    packs = sorted((workspace / ".git" / "objects" / "pack").glob("*.pack"))
    if packs:
        leaks.append(f"{len(packs)} packfile(s)")
    unreachable = git_ws("fsck", "--unreachable", "--no-reflogs", "--no-progress").stdout.split("\n")
    unreachable = [line for line in unreachable if line.strip()]
    if unreachable:
        leaks.append(f"{len(unreachable)} objet(s) inaccessible(s)")
    source = task["source"]
    gold_blobs: dict[str, str] = {}
    if "gold" in source:
        clone = cache.root / "sources" / task["id"]
        changed = subprocess.run(["git", "-C", str(clone), "diff", "--name-only", "--diff-filter=AMR",
                                  source["commit"], source["gold"]], capture_output=True,
                                 text=True, check=True).stdout.split()
        for path in changed:
            oid = subprocess.run(["git", "-C", str(clone), "rev-parse", f"{source['gold']}:{path}"],
                                 capture_output=True, text=True).stdout.strip()
            if oid:
                gold_blobs[oid] = path
        for oid, path in gold_blobs.items():
            if git_ws("cat-file", "-e", oid).returncode == 0:
                leaks.append(f"objet git de la version corrigée de {path}")
        names = {Path(p).name for p in gold_blobs.values()}
        candidates = [f for f in workspace.rglob("*") if f.name in names and f.is_file()
                      and ".git" not in f.relative_to(workspace).parts[:1]]
        for f in candidates:
            oid = subprocess.run(["git", "hash-object", "--no-filters", str(f)], capture_output=True,
                                 text=True).stdout.strip()
            if oid in gold_blobs:
                leaks.append(f"fichier {f.relative_to(workspace)} = version corrigée de "
                             f"{gold_blobs[oid]}")
    return {"commits": len(commits), "gold_blobs_checked": len(gold_blobs), "leaks": leaks}


def restore(task: dict, cache: Cache) -> Path:
    workspace, pristine, _ = task_paths(task, cache)
    shutil.rmtree(workspace, ignore_errors=True)
    shutil.copytree(pristine, workspace, symlinks=True)
    return workspace


def apply_gold(task: dict, cache: Cache, workspace: Path) -> str | None:
    """Correction officielle : diff du commit parent au commit `gold`, tests cachés exclus."""
    source = task["source"]
    clone = cache.root / "sources" / task["id"]
    hidden = task["dir"] / "hidden"
    excluded = [f":(exclude){p.relative_to(hidden).as_posix()}"
                for p in sorted(hidden.rglob("*")) if p.is_file()] if hidden.exists() else []
    diff = subprocess.run(["git", "-C", str(clone), "diff", "--binary", source["commit"],
                           source["gold"], "--", ".", *excluded], capture_output=True, check=True)
    applied = subprocess.run(["git", "apply", "--whitespace=nowarn", "-"], input=diff.stdout,
                             cwd=workspace, capture_output=True)
    return None if applied.returncode == 0 else applied.stderr.decode(errors="replace")[-500:]


def validate_gold(tasks: list[dict], cache: Cache, out: Path, runs: int,
                  profile_name: str = "task") -> bool:
    """Sans modèle : état cassé (fail_to_pass en échec, pass_to_pass en succès) et
    correction officielle (tout en succès), `runs` fois chacun, tests sous sandbox :
    `sandbox-task.sb` (notation) ou `sandbox-agent.sb` (conditions de la phase agent)."""
    report = []
    for task in tasks:
        if "gold" not in task["source"]:
            fail(f"tâche {task['id']} : `source.gold` (commit de correction) absent")
        f2p = set(task.get("fail_to_pass", []))
        checks = []
        for state in ("broken", "gold"):
            for index in range(1, runs + 1):
                workspace = restore(task, cache)
                error = apply_gold(task, cache, workspace) if state == "gold" else None
                profile = (agent_profile(cache, workspace, None, cache.agent)
                           if profile_name == "agent" else None)
                graded = (run_tests(task, workspace, out / task["id"] / f"{state}-{index}", cache,
                                    sandboxed=True, profile=profile) if error is None else
                          {"tests": {}, "junit": False, "process": None})
                expected = {tid: ("failed" if state == "broken" and tid in f2p else "passed")
                            for tid in graded["tests"]}
                wrong = {tid: st for tid, st in graded["tests"].items() if st != expected[tid]}
                ok = error is None and graded["junit"] and not wrong
                checks.append({"state": state, "run": index, "ok": ok, "wrong": wrong,
                               "apply_error": error,
                               "seconds": (graded["process"] or {}).get("seconds")})
                print(f"  {task['id']} {state} {index}/{runs} : {'ok' if ok else 'ÉCHEC'}"
                      f"{'' if ok else f' {error or wrong}'}")
        report.append({"task": task["id"], "ok": all(c["ok"] for c in checks), "checks": checks})
    (out / "validate.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    lines = ["# Validation des tâches (--validate-gold)", "",
             f"{runs} exécutions de chaque vérification, tests sous "
             f"`sandbox-{profile_name}.sb`.", "",
             "| Tâche | État cassé | Correction officielle | Durée max des tests s | Valide |",
             "|---|---|---|---|---|"]
    for r in report:
        by = {s: [c for c in r["checks"] if c["state"] == s] for s in ("broken", "gold")}
        secs = [c["seconds"] for c in r["checks"] if c["seconds"] is not None]
        lines.append(f"| {r['task']} | {sum(c['ok'] for c in by['broken'])}/{runs} | "
                     f"{sum(c['ok'] for c in by['gold'])}/{runs} | {max(secs, default=None)} | "
                     f"{'oui' if r['ok'] else 'NON'} |")
    (out / "validate.md").write_text("\n".join(lines) + "\n")
    print(f"rapport : {out / 'validate.md'}")
    return all(r["ok"] for r in report)


# --- Sonde, chauffe, phase agent ---------------------------------------------


def agent_session(binary: Path, cache: Cache, agent_dir: Path, out: Path, model: str,
                  effort: str, prompt: str, cwd: Path, access: Access, connect: proxies.ConnectProxy,
                  source: dict, settings: dict, capture: bool, timeout: int,
                  idle_timeout: int | None = None) -> dict:
    """Une session d'agent sous sandbox, avec ses journaux copiés dans `out`. Avec
    `idle_timeout`, garde-fou d'inactivité sur le reverse proxy (`run_process`)."""
    out.mkdir(parents=True, exist_ok=True)
    write_agent_config(agent_dir, access.port, source, settings)
    before = session_files(agent_dir)
    connect_before = len(proxies.read_log(connect.log.path))
    env = agent_env(cache, agent_dir, connect.port, capture)
    process = run_process(agent_cmd(binary, model, effort, prompt), env, cwd,
                          out / "out.jsonl", out / "err.log", timeout, sandboxed=True,
                          profile=agent_profile(cache, cwd, binary, agent_dir),
                          idle=(access.proxy.idle_seconds, idle_timeout)
                          if idle_timeout is not None else None)
    copied = copy_new_sessions(agent_dir, before, out / "sessions")
    connect_entries = proxies.read_log(connect.log.path)[connect_before:]
    with (out / "connect.ndjson").open("w") as handle:
        for entry in connect_entries:
            handle.write(json.dumps(entry) + "\n")
    return {"process": process, **session_logs(copied),
            "proxy_log": proxies.read_log(access.log_path), "connect": connect_entries}


def session_logs(copied: dict) -> dict:
    """Journaux d'usage et entrées de session, session principale et sous-agents à part."""
    return {"records": metrics.read_ndjson(copied["usage"]),
            "entries": metrics.session_entries(copied["sessions"]),
            "sub_records": metrics.read_ndjson(copied["sub_usage"]),
            "sub_entries": metrics.session_entries(copied["sub_sessions"])}


def work_dir(cache: Cache, name: str) -> Path:
    """Projet vide hors de bench/smoke/ (refusé par le profil de la phase agent)."""
    path = cache.root / "work" / name
    shutil.rmtree(path, ignore_errors=True)
    path.mkdir(parents=True)
    return path


def read_probe_args(paths: list[Path]) -> list[str]:
    return [arg for path in paths for arg in ("--read-probe-path", str(path.resolve()))]


# Chemins réels de $HOME que la phase agent ne doit pas lire (T07) : clés, caches uv et npm.
HOME_TARGETS = (".ssh", ".cache/uv", ".npm")


def hidden_targets(task: dict, cache: Cache) -> list[Path]:
    """Chemins que la sonde d'isolation tente de lire : un test caché, la copie pristine,
    et ceux de HOME_TARGETS qui existent."""
    hidden = sorted(p for p in (task["dir"] / "hidden").rglob("*") if p.is_file())
    _, pristine, marker = task_paths(task, cache)
    home = [Path.home() / t for t in HOME_TARGETS if (Path.home() / t).exists()]
    return hidden[:1] + [pristine, marker] + home


def probe(binary: Path, cache: Cache, out: Path, connect: proxies.ConnectProxy,
          tasks: list[dict]) -> dict:
    """Sonde réseau sous sandbox, contre le serveur de capture (aucun modèle réel)."""
    write_fake_auth(cache.probe_agent)
    project = work_dir(cache, "probe")
    targets = (hidden_targets(tasks[0], cache) if tasks else
               [HERE / "README.md"] + [Path.home() / t for t in HOME_TARGETS
                                       if (Path.home() / t).exists()])
    access = Access(out, True, "smoke-probe", connect.port, read_probe_args(targets))
    try:
        session = agent_session(binary, cache, cache.probe_agent, out, DEFAULT_MODELS[0],
                                DEFAULT_EFFORT, PROBE_PROMPT, project, access, connect,
                                {"source": "bundled"}, {}, True, 240)
    finally:
        access.stop()
    markers = metrics.kernel_markers(session["entries"])
    connects = metrics.connect_summary(session["connect"])
    direct = metrics.last_marker(markers, "SMOKE_CURL_DIRECT")
    via_proxy = metrics.last_marker(markers, "SMOKE_CURL_PROXY")
    return {"exit": session["process"]["exit"],
            "kernel_proxy_vars": metrics.last_marker(markers, "SMOKE_KERNEL_PROXY_VARS"),
            "bash_proxy_vars": metrics.last_marker(markers, "SMOKE_BASH_PROXY_VARS"),
            "isolation": metrics.isolation(markers),
            "curl_direct_exit": direct, "curl_via_proxy_exit": via_proxy,
            "connect": connects,
            "network_cut": (direct not in (None, "0") and via_proxy not in (None, "0")
                            and "example.com" in connects["refused_hosts"])}


def warmup(binary: Path, cache: Cache, out: Path, model: str, effort: str, source: dict,
           settings: dict, connect: proxies.ConnectProxy, capture: bool,
           server_args: list[str]) -> dict:
    """Run de chauffe : le modèle doit répondre. Un id faux échoue ici, pas au run mesuré."""
    project = work_dir(cache, "warmup")
    access = Access(out, capture, "smoke-warmup", connect.port, server_args)
    try:
        session = agent_session(binary, cache, cache.agent, out, model, effort, WARMUP_PROMPT,
                                project, access, connect, source, settings, capture, 300)
    finally:
        access.stop()
    final = metrics.final_assistant(session["entries"])
    ok_records = [r for r in session["records"]
                  if r.get("stopReason") not in ("error", "aborted") and r.get("model") == model]
    transports = metrics.match_transports(session["records"], session["proxy_log"])
    result = {"exit": session["process"]["exit"], "stop_reason": final["stop_reason"],
              "error": final["error"], "calls": len(session["records"]),
              "transports": transports["requests"], "connect": metrics.connect_summary(
                  session["connect"]),
              "ok": session["process"]["exit"] == 0 and bool(ok_records)
              and final["stop_reason"] not in ("error", "aborted")}
    return result


def run_task(task: dict, index: int, binary: Path, cache: Cache, out: Path, model: str,
             effort: str, source: dict, settings: dict, connect: proxies.ConnectProxy,
             capture: bool, capture_fail: set[str], server_args: list[str],
             prices: dict, capture_spawn: bool = False,
             idle_timeout: int = IDLE_TIMEOUT_S) -> dict:
    workspace = restore(task, cache)
    prompt = (task["dir"] / "prompt.md").read_text()
    # Seules les tâches d'exemple ont un scénario de capture qui les résout. Avec
    # `--capture-spawn`, le scénario `spawn` lance un sous-agent (ramassage de ses sessions).
    solvable = "exemple" in task.get("tags", []) and task["id"] not in capture_fail
    scenario = ("spawn" if capture_spawn else
                f"smoke-{task['id'] if solvable else 'fail'}")
    access = Access(out, capture, scenario, connect.port,
                    server_args + read_probe_args(hidden_targets(task, cache)))
    try:
        session = agent_session(binary, cache, cache.agent, out, model, effort, prompt,
                                workspace, access, connect, source, settings, capture,
                                int(task.get("timeout_s", 1800)), idle_timeout)
    finally:
        access.stop()
    graded = run_tests(task, workspace, out / "grading", cache, sandboxed=True)
    return task_result(task, index, session["process"], session, graded, model, effort,
                       prices, {"tree": tree_hash(task["dir"]), "source": "run"})


def task_result(task: dict, index: int, process: dict, session: dict, graded: dict,
                model: str, effort: str, prices: dict, version: dict) -> dict:
    """Mesures d'un run de tâche. Totaux : session principale et sous-agents ; la part
    des sous-agents est donnée à part (`subagents`). Statut et dernier message : session
    principale seule. `version` : empreinte du dossier de la tâche (`tree_hash`) et sa
    source (`run`, ou `commit <sha>` pour un run recalculé), `tree` à `None` si inconnue."""
    entries = session["entries"]
    records = session["records"] + session["sub_records"]
    all_entries = entries + session["sub_entries"]
    final = metrics.final_assistant(entries)
    resolved = graded["junit"] and all(s == "passed" for s in graded["tests"].values())
    if process.get("stalled"):
        status = "stalled"
    elif not entries:
        status = "harness_error"
    elif process["timeout"]:
        status = "timeout"
    elif process["exit"] != 0 or final["stop_reason"] in ("error", "aborted"):
        status = "agent_error"
    elif not graded["junit"]:
        status = "harness_error"
    else:
        status = "resolved" if resolved else "failed"
    transports = metrics.match_transports(records, session["proxy_log"])
    misses = metrics.server_misses(records, transports["per_request"])
    calls = metrics.calls(records)
    anomalies = []
    if process.get("stalled"):
        anomalies.append(f"flux bloqué : aucun octet par le reverse proxy pendant "
                         f"{process.get('idle_timeout_s')} s, tâche arrêtée à "
                         f"{process['seconds']} s (statut `stalled`)")
    if transports["loop_on_sse"]:
        anomalies.append(f"{transports['loop_on_sse']} appel(s) de boucle en SSE (bascule)")
    if transports["unmatched_usage"] or transports["unmatched_proxy"]:
        anomalies.append(f"transport non prouvé : {transports['unmatched_usage']} appel(s) "
                         f"sans requête proxy, {transports['unmatched_proxy']} requête(s) "
                         "sans appel journalisé")
    if any(e != effort for e in calls["efforts"]):
        anomalies.append(f"effort journalisé {calls['efforts']} au lieu de {effort}")
    if any(m != model for m in calls["models"]):
        anomalies.append(f"modèle journalisé {calls['models']} au lieu de {model}")
    connects = metrics.connect_summary(session["connect"])
    markers = metrics.kernel_markers(all_entries)
    isolated = metrics.isolation(markers)
    if isolated and isolated["readable"]:
        anomalies.append(f"lecture hors workspace réussie : {isolated['readable']}")
    if "difficile" in task.get("tags", []) and status == "resolved" and calls["turns"] < HARD_MIN_TURNS:
        anomalies.append(f"tâche difficile résolue en {calls['turns']} tours (< {HARD_MIN_TURNS}) :"
                         " à signaler, pas de remplacement sans accord")
    return {
        "task": task["id"], "run": index, "status": status, "resolved": status == "resolved",
        "task_version": version,
        "tests": graded["tests"], "agent": process, "final": final,
        "tokens": metrics.token_totals(records), "calls": calls,
        "cache": {**metrics.cache_rates(records),
                  "avoidable_rate": metrics.avoidable_cache_rate(records, misses)},
        "server_misses": misses,
        "subagents": metrics.subagent_share(session["sub_records"], records, misses, prices,
                                            model, CACHE_WRITE_PRICE),
        "cost": metrics.costs(prices, model, records, CACHE_WRITE_PRICE),
        "cells": metrics.cell_errors(all_entries),
        "transports": {k: v for k, v in transports.items() if k != "per_request"},
        "transport_failures_in_session": metrics.session_transport_failures(all_entries),
        "per_request": transports["per_request"],
        "context_peak": metrics.context_peak(records),
        "isolation": isolated,
        "connect": connects, "kernel_markers": markers,
        "anomalies": anomalies,
    }


# --- Rapport ---------------------------------------------------------------------


def git(*args: str) -> str | None:
    proc = subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True)
    return proc.stdout.strip() if proc.returncode == 0 else None


def binary_manifest(binary: Path) -> dict | None:
    for candidate in [binary.parent / "manifest.json", *binary.parent.parent.glob("*/manifest.json")]:
        if candidate.is_file():
            try:
                data = json.loads(candidate.read_text())
                return {"commit": data.get("commit"), "version": data.get("version")}
            except ValueError:
                return None
    return None


def version_of(cmd: list[str]) -> str | None:
    env = dict(os.environ, DO_NOT_TRACK="1", PI_OFFLINE="1", PI_SKIP_VERSION_CHECK="1")
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=20,
                              stdin=subprocess.DEVNULL, env=env)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return (proc.stdout or proc.stderr).strip().splitlines()[0] if proc.returncode == 0 else None


def known_settings(settings: dict) -> dict:
    """Réglages de variante non par défaut (CLAUDE.md règle 5), plus le TTL fixé."""
    keys = ("masking", "rlm", "rlmMaxDepth", "cacheRetention", "transport")
    merged = {"cacheRetention": CACHE_RETENTION, **settings}
    return {k: merged[k] for k in keys if k in merged}


def aggregate(results: list[dict]) -> dict:
    per_task: dict[str, dict] = {}
    for task in sorted({r["task"] for r in results}):
        runs = [r for r in results if r["task"] == task]
        resolved = [r["resolved"] for r in runs]
        cost = [r["cost"]["total"] for r in runs if r["cost"]["total"] is not None]
        tokens = [sum(v or 0 for k, v in r["tokens"].items()
                      if k in ("input", "output", "cacheRead", "cacheWrite")) for r in runs]
        rate = sum(resolved) / len(resolved)
        per_task[task] = {
            "runs": len(runs), "resolved_rate": rate, "flip": 0 < rate < 1,
            "cost_mean": statistics.mean(cost) if cost else None,
            "cost_stdev": statistics.stdev(cost) if len(cost) > 1 else None,
            # T08b : `None` si un run n'a pas de coût pessimiste (rapport antérieur).
            "cost_pessimistic_mean": (
                statistics.mean(r["cost"]["pessimistic_total"] for r in runs)
                if runs and all(r["cost"].get("pessimistic_total") is not None for r in runs)
                else None),
            # YA-14 : coût à cache théorique, `None` si un run n'en a pas (rapport antérieur).
            "cost_theoretical_mean": (
                statistics.mean(r["cost"]["theoretical_total"] for r in runs)
                if runs and all(r["cost"].get("theoretical_total") is not None for r in runs)
                else None),
            "tokens_mean": statistics.mean(tokens),
            "tokens_stdev": statistics.stdev(tokens) if len(tokens) > 1 else None,
            "turns_mean": statistics.mean(r["calls"]["turns"] for r in runs),
            "server_misses": sum(len(r.get("server_misses", [])) for r in runs),
            "context_peak_max": max((r["context_peak"] for r in runs
                                     if r["context_peak"] is not None), default=None),
            "tokens_by_category_mean": {
                k: (statistics.mean(r["tokens"][k] for r in runs)
                    if all(r["tokens"][k] is not None for r in runs) else None)
                for k in metrics.TOKEN_FIELDS},
        }
    return per_task


def totals(results: list[dict]) -> dict:
    resolved = sum(r["resolved"] for r in results)
    costs = [r["cost"]["total"] for r in results]
    cost = sum(costs) if costs and None not in costs else None
    pessimistic = [r["cost"].get("pessimistic_total") for r in results]
    cost_pessimistic = sum(pessimistic) if pessimistic and None not in pessimistic else None
    theoretical = [r["cost"].get("theoretical_total") for r in results]
    cost_theoretical = sum(theoretical) if theoretical and None not in theoretical else None
    read = sum(r["tokens"]["cacheRead"] or 0 for r in results)
    tokens = {k: (sum(r["tokens"][k] for r in results)
                  if results and all(r["tokens"][k] is not None for r in results) else None)
              for k in metrics.TOKEN_FIELDS}
    prompt = sum((tokens[k] or 0) for k in ("input", "cacheRead", "cacheWrite"))
    return {"task_runs": len(results), "resolved": resolved,
            "cost": cost, "cost_per_resolved": cost / resolved if cost is not None and resolved
            else None,
            "cost_pessimistic": cost_pessimistic,
            "cost_pessimistic_per_resolved": (cost_pessimistic / resolved
                                              if cost_pessimistic is not None and resolved
                                              else None),
            "cost_theoretical": cost_theoretical,
            "cost_theoretical_per_resolved": (cost_theoretical / resolved
                                              if cost_theoretical is not None and resolved
                                              else None),
            "cache_theory_check": metrics.theory_check(
                [g for r in results
                 for g in ((r["cost"].get("theoretical") or {}).get("check_gaps") or [])]),
            "tokens": tokens,
            "turns_median": metrics.median([r["calls"]["turns"] for r in results]),
            "cache_rate_all_calls": read / prompt if prompt else None,
            "loop_on_sse": sum(r["transports"]["loop_on_sse"] for r in results),
            "server_misses": sum(len(r.get("server_misses", [])) for r in results),
            "connect_refused": sum(r["connect"]["refused"] for r in results),
            "anomalies": sum(len(r["anomalies"]) for r in results),
            "subagents": subagent_totals(results, cost)}


def subagent_totals(results: list[dict], cost: float | None) -> dict:
    """Part des sous-agents sur tout le run (comptée aussi dans les totaux)."""
    subs = [r.get("subagents") or {} for r in results]
    costs = [s.get("cost") for s in subs]
    sub_cost = sum(costs) if costs and None not in costs else None
    tokens = [s.get("tokens") or {} for s in subs]
    read = sum(t.get("cacheRead") or 0 for t in tokens)
    prompt = sum(t.get(k) or 0 for t in tokens for k in ("input", "cacheRead", "cacheWrite"))
    return {"sessions": sum(s.get("sessions", 0) for s in subs),
            "calls": sum(s.get("calls", 0) for s in subs),
            "calls_total": sum(r["calls"]["total"] for r in results),
            "cost": sub_cost,
            "cost_share": sub_cost / cost if sub_cost is not None and cost else None,
            "cache_rate_all_calls": read / prompt if prompt else None}


def pct(value: float | None) -> float | None:
    return None if value is None else 100 * value


def pct_change(a: float | None, b: float | None) -> float | None:
    """Écart de `b` par rapport à `a`, en % ; `None` si l'un manque ou si `a` est nul."""
    return None if a in (None, 0) or b is None else (b - a) / a * 100


def fmt(value, digits: int = 4) -> str:
    if value is None:
        return "null"
    if isinstance(value, float):
        return f"{value:.{digits}f}"
    return str(value)


def write_report(run_dir: Path, report: dict) -> None:
    (run_dir / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    fields = ["task", "run", "status", "cost", "cost_pessimistic", "cost_theoretical", "input", "output", "cacheRead",
              "cacheWrite", "reasoning", "context_peak", "turns", "calls", "cache_session_rate",
              "cache_continuation_median", "cache_avoidable_rate", "server_misses",
              "ttft_median_ms", "seconds", "websocket", "sse", "loop_on_sse", "connect_refused",
              "cell_errors", "syntax_errors", "truncated", "subagent_sessions", "subagent_calls",
              "subagent_cost", "task_tree"]
    with (run_dir / "tasks.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for r in report["results"]:
            writer.writerow({
                "task": r["task"], "run": r["run"], "status": r["status"],
                "cost": r["cost"]["total"],
                "cost_pessimistic": r["cost"].get("pessimistic_total"),
                "cost_theoretical": r["cost"].get("theoretical_total"),
                **{k: r["tokens"][k] for k in
                                               ("input", "output", "cacheRead", "cacheWrite",
                                                "reasoning")},
                "context_peak": r["context_peak"],
                "turns": r["calls"]["turns"], "calls": r["calls"]["total"],
                "cache_session_rate": r["cache"]["session_rate"],
                "cache_continuation_median": r["cache"]["continuation_median"],
                "cache_avoidable_rate": r["cache"].get("avoidable_rate"),
                "server_misses": len(r.get("server_misses", [])),
                "ttft_median_ms": r["calls"]["ttft_median_ms"], "seconds": r["agent"]["seconds"],
                "websocket": r["transports"]["requests"].get("websocket", 0),
                "sse": r["transports"]["requests"].get("sse", 0),
                "loop_on_sse": r["transports"]["loop_on_sse"],
                "connect_refused": r["connect"]["refused"], "cell_errors": r["cells"]["errors"],
                "syntax_errors": r["cells"]["syntax_errors"], "truncated": r["calls"]["truncated"],
                "subagent_sessions": (r.get("subagents") or {}).get("sessions"),
                "subagent_calls": (r.get("subagents") or {}).get("calls"),
                "subagent_cost": (r.get("subagents") or {}).get("cost"),
                "task_tree": (r.get("task_version") or {}).get("tree")})
    h, t = report["header"], report["totals"]
    sub = t.get("subagents") or {}
    lines = [f"# Smoke bench — {h['model']} — {h['started']}", "",
             "| Champ | Valeur |", "|---|---|"]
    for key in ("mode", "commit", "commit_dirty", "binary", "binary_sha256", "binary_manifest",
                "binary_version", "models_bundled_sha256", "provider", "model", "model_source",
                "model_fiche_hypotheses", "effort", "settings_file", "settings_non_default", "prices_version",
                "prices_sha256", "cache_write_price", "idle_timeout_s", "hosts", "runs", "tasks",
                "timing_s", "ab", "rereport"):
        lines.append(f"| {key} | `{json.dumps(h.get(key), ensure_ascii=False)}` |")
    p, w = report["probe"], report["warmup"]
    lines += ["", "## Sonde réseau (serveur de capture, sandbox)", "",
              f"- Réseau coupé prouvé : **{p['network_cut']}** (curl direct : code "
              f"{p['curl_direct_exit']}, curl par le proxy CONNECT : code {p['curl_via_proxy_exit']},"
              f" CONNECT refusés : {p['connect']['refused_hosts']})",
              f"- Variables de proxy vues par le kernel (noms) : {p['kernel_proxy_vars']} ; "
              f"par `bash()` : {p['bash_proxy_vars']}",
              f"- Isolation des tests cachés : lectures hors workspace tentées "
              f"{(p['isolation'] or {}).get('checked')}, réussies "
              f"{(p['isolation'] or {}).get('readable')}",
              "", "## Run de chauffe", "",
              f"- OK : **{w['ok']}** (arrêt `{w['stop_reason']}`, {w['calls']} appel(s), "
              f"transports {w['transports']}, CONNECT acceptés {w['connect']['accepted']}, "
              f"refusés {w['connect']['refused']})",
              "", "## Totaux", "",
              f"- Tâches résolues : **{t['resolved']} / {t['task_runs']}**",
              f"- Coût théorique : {fmt(t['cost'])} $ (rapporté) ; pessimiste : "
              f"{fmt(t.get('cost_pessimistic'))} $ "
              f"({fmt(pct_change(t['cost'], t.get('cost_pessimistic')), 1)} %)",
              f"- Par tâche résolue : {fmt(t['cost_per_resolved'])} $ (rapporté) ; pessimiste : "
              f"{fmt(t.get('cost_pessimistic_per_resolved'))} $",
              f"- Coût à cache théorique : {fmt(t.get('cost_theoretical'))} $ "
              f"({fmt(pct_change(t['cost'], t.get('cost_theoretical')), 1)} % du rapporté) ; par "
              f"tâche résolue : {fmt(t.get('cost_theoretical_per_resolved'))} $",
              f"- Tokens : {json.dumps(t['tokens'])}",
              f"- Tours médians : {fmt(t['turns_median'], 1)} ; taux de cache (tous appels) : "
              f"{fmt(t['cache_rate_all_calls'])} ; miss serveur : {t['server_misses']}",
              f"- Appels de boucle en SSE (anomalie) : **{t['loop_on_sse']}** ; CONNECT refusés :"
              f" {t['connect_refused']} ; anomalies : {t['anomalies']}",
              f"- Dont sous-agents (comptés dans les totaux) : {sub.get('sessions')} session(s), "
              f"{sub.get('calls')} appel(s) sur {sub.get('calls_total')}, coût "
              f"{fmt(sub.get('cost'))} $ ({fmt(pct(sub.get('cost_share')), 1)} % du coût), taux de "
              f"cache (tous appels) {fmt(sub.get('cache_rate_all_calls'))}",
              "", "## Tâches", "",
              "| Tâche | Run | Statut | Coût $ | Coût pess. $ | Coût th. $ | input | output |"
              " cacheRead |"
              " cacheWrite | reasoning |"
              " Pic de contexte | Tours |"
              " Cache session | Cache contin. (méd.) | Cache évitable | Miss serveur |"
              " TTFT méd. ms | Durée s | WS | SSE | Cellules en erreur (Syntax) | Coupées |",
              "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
              "---|---|---|"]
    for r in report["results"]:
        lines.append(
            f"| {r['task']} | {r['run']} | {r['status']} | {fmt(r['cost']['total'])} | "
            f"{fmt(r['cost'].get('pessimistic_total'))} | "
            f"{fmt(r['cost'].get('theoretical_total'))} | "
            f"{fmt(r['tokens']['input'])} | {fmt(r['tokens']['output'])} | "
            f"{fmt(r['tokens']['cacheRead'])} | {fmt(r['tokens']['cacheWrite'])} | "
            f"{fmt(r['tokens']['reasoning'])} | "
            f"{fmt(r['context_peak'])} | {r['calls']['turns']} | {fmt(r['cache']['session_rate'])} | "
            f"{fmt(r['cache']['continuation_median'])} | {fmt(r['cache'].get('avoidable_rate'))} | "
            f"{len(r.get('server_misses', []))} | {fmt(r['calls']['ttft_median_ms'], 1)} | "
            f"{r['agent']['seconds']} | {r['transports']['requests'].get('websocket', 0)} | "
            f"{r['transports']['requests'].get('sse', 0)} | {r['cells']['errors']} "
            f"({r['cells']['syntax_errors']}) | {r['calls']['truncated']} |")
    lines += ["", "## Sous-agents", "",
              "Sessions lancées par `rlm.spawn` : comptées dans les totaux et le tableau "
              "ci-dessus, détaillées ici.", "",
              "| Tâche | Run | Sessions | Appels | Coût $ | Part du coût % | Cache session |"
              " Miss serveur |", "|---|---|---|---|---|---|---|---|"]
    for r in report["results"]:
        s = r.get("subagents") or {}
        lines.append(f"| {r['task']} | {r['run']} | {s.get('sessions')} | {s.get('calls')} | "
                     f"{fmt(s.get('cost'))} | {fmt(pct(s.get('cost_share')), 1)} | "
                     f"{fmt(s.get('cache_session_rate'))} | {s.get('server_misses')} |")
    lines += ["", "## Version des tâches", "",
              "Empreinte du dossier de la tâche (`task.toml`, `prompt.md`, `hidden/`, fichiers) : "
              "`baseline.py` refuse de mélanger deux versions d'une tâche.", ""]
    versions = sorted({(r["task"], (r.get("task_version") or {}).get("tree"),
                        (r.get("task_version") or {}).get("source")) for r in report["results"]},
                      key=lambda v: (v[0], v[1] or ""))
    lines += [f"- {task} : `{tree[:16] if tree else 'inconnue'}` ({source})"
              for task, tree, source in versions]
    if report["header"]["runs"] > 1:
        lines += ["", "## Variabilité entre runs", "",
                  "| Tâche | Runs | Taux résolu | Bascule | Coût moyen | Écart-type coût |"
                  " Tokens moyens | Écart-type tokens |", "|---|---|---|---|---|---|---|---|"]
        for task, a in report["per_task"].items():
            lines.append(f"| {task} | {a['runs']} | {fmt(a['resolved_rate'], 2)} | {a['flip']} | "
                         f"{fmt(a['cost_mean'])} | {fmt(a['cost_stdev'])} | "
                         f"{fmt(a['tokens_mean'], 1)} | {fmt(a['tokens_stdev'], 1)} |")
    lines += theory_section(report)
    misses = [(r["task"], r["run"], m) for r in report["results"]
              for m in r.get("server_misses", [])]
    lines += ["", "## Miss serveur", "",
              "Appel de la boucle hors premier de session, `cacheRead = 0`, sans invalidation "
              "journalisée.", ""]
    lines += [f"- {task} (run {run}) : {m['kind']}, seq {m['seq']}, prompt {m['prompt']}, depuis l'appel "
              f"précédent {fmt(m['since_prev_ms'])} ms, transport {m['transport']}, "
              f"continuation {m['continuation']}" for task, run, m in misses] or ["- aucun"]
    anomalies = [(r["task"], r["run"], a) for r in report["results"] for a in r["anomalies"]]
    lines += ["", "## Anomalies", ""]
    lines += [f"- {task} (run {run}) : {a}" for task, run, a in anomalies] or ["- aucune"]
    scan = report.get("secret_scan")
    if scan is not None:
        lines += ["", "## Secrets", "",
                  f"- Valeurs recherchées dans tout le dossier de run : {scan['values_checked']} ;"
                  f" fichiers où une valeur apparaît : {sorted(scan['files_with_hits']) or 'aucun'}"]
    lines += ["", "## Notes", "", *[f"- {n}" for n in report["notes"]], ""]
    (run_dir / "report.md").write_text("\n".join(lines))


def theory_section(report: dict) -> list[str]:
    """Coût à cache théorique : appels par base et validation de la méthode (YA-14)."""
    check = report["totals"].get("cache_theory_check") or metrics.theory_check([])
    bases = {b: 0 for b in metrics.THEORY_BASES}
    for r in report["results"]:
        for b, n in ((r["cost"].get("theoretical") or {}).get("calls") or {}).items():
            bases[b] = bases.get(b, 0) + n
    lines = ["", "## Coût à cache théorique", "",
             "Cache qu'un serveur qui ne rate jamais aurait lu : préfixe commun avec la requête "
             "précédente de la même session (méthode dans les notes). Appels par base : "
             + ", ".join(f"{b} {n}" for b, n in bases.items()) + ".", ""]
    if not check["n"]:
        return lines + ["- Validation : aucun appel de continuation sans miss serveur"]
    q = check["quantiles"]
    lines += [f"- Validation (appels de continuation sans miss serveur, {check['n']}) : rapporté "
              f"> théorique {fmt(pct(check['above_share']), 2)} % (critère ≤ "
              f"{metrics.THEORY_MAX_ABOVE_SHARE * 100:g} %), écart médian (théorique − rapporté) "
              f"{fmt(pct(check['median_gap']), 2)} % du prompt (critère ≤ "
              f"{metrics.THEORY_MAX_MEDIAN_GAP * 100:g} %) : **{metrics.theory_verdict(check)}**",
              "- Distribution de l'écart, % du prompt : "
              + ", ".join(f"{k} {fmt(pct(v), 2)}" for k, v in q.items())
              + f" ; au-delà de {metrics.THEORY_PARTIAL_GAP * 100:g} % (miss partiels du serveur)"
              f" : {fmt(pct(check['partial_share']), 1)} %"]
    if report["header"].get("mode") == "capture":
        lines.append("- Mode capture : usage simulé par le serveur de capture (lecture de 90 % "
                     "du prompt précédent), la validation n'a pas de sens ici")
    return lines


def report_notes(prices: dict, effort: str) -> list[str]:
    notes = [
        "Tokens et usage : journal d'usage du harness (`<id>.usage.ndjson`), rien n'est "
        "recalculé. Pour l'API Responses, `cacheWrite` = `cache_write_tokens` du provider "
        "(T08, docs/usage.md), 0 quand le provider ne le renvoie pas : l'écriture est alors "
        "comptée au tarif d'entrée.",
        f"Coût théorique : tarif API Standard de prices.json (version {prices.get('version')}), "
        "écriture ventilée par le provider au tarif de son TTL (`cache_write_30m`, "
        f"`cache_write_12h`), sinon au tarif `{CACHE_WRITE_PRICE}` (le corps Codex ne porte pas "
        "de rétention), contexte long au-delà du seuil de prices.json.",
        f"Coût pessimiste (T08b) : même calcul, `input` non caché au tarif "
        f"`{metrics.PESSIMISTIC_INPUT_PRICE}` au lieu du tarif d'entrée, comme si tout ce qui "
        "n'a pas été lu en cache y avait été écrit. Plafond pour un provider qui n'annonce pas "
        "ses écritures (Codex : `cache_write_tokens` toujours à 0, docs/usage.md) ; trop haut "
        "pour un provider qui les annonce.",
        "Coût à cache théorique (YA-14) : décision du banc. Par appel, `cacheRead` = "
        "préfixe commun avec la requête précédente de la même session (chaque sous-agent dans "
        "la sienne), le reste du prompt au tarif d'entrée, sortie et raisonnement inchangés. "
        "Le corps WebSocket (`previous_response_id`) ne porte pas tout le contexte : préfixe "
        "approché depuis l'usage, min(prompt, prompt précédent + output précédent) sur une "
        "continuation append-only ; `cacheRead` rapporté gardé sur une invalidation "
        "journalisée, après un appel annexe ou si le prompt raccourcit (ruptures du harness) ; "
        "de même au premier appel de boucle d'une session et sur un appel annexe (théorique "
        "non calculable). Les miss serveur disparaissent, les "
        "ruptures causées par le harness restent comptées. Validation : section « Coût à "
        "cache théorique ».",
        "Transport : WebSocket attendu pour les appels de boucle (main, subagent) ; "
        "compaction, /refine et résumé de branche passent en SSE par construction.",
        "Taux de cache de session : cacheRead / (input + cacheRead + cacheWrite) sur les "
        "appels de boucle ; continuation : appels de boucle hors premier de leur session et "
        "hors invalidation journalisée ; évitable : mêmes appels, hors miss serveur.",
        "Sous-agents (`rlm.spawn`) : leurs sessions (`session-artifacts/<parent>/sub-*/` du "
        "dossier agent, copiées dans `sessions/subagents/`) comptent dans les totaux, le coût, "
        "le transport, le cache et les miss ; leur part est donnée à part. Tours : appels "
        "`main` de la session principale.",
        f"Garde-fou d'inactivité : une tâche dont aucun octet ne passe par le reverse proxy "
        f"pendant `idle_timeout_s` (en-tête) est arrêtée, statut `stalled` (flux bloqué).",
    ]
    if effort != DEFAULT_EFFORT:
        notes.append(f"Effort {effort} au lieu de {DEFAULT_EFFORT} (décision du 04/11).")
    return notes


def scan_secrets(run_dir: Path, values: list[str]) -> dict:
    """Recherche des valeurs secrètes dans tout le dossier de run (fichiers seuls notés)."""
    hits = {}
    for path in run_dir.rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes()
        found = sum(1 for v in values if v and v.encode() in data)
        if found:
            hits[str(path.relative_to(run_dir))] = found
    return {"values_checked": len(values), "files_with_hits": hits}


def auth_values(agent_dir: Path) -> list[str]:
    """Valeurs de `auth.json` à rechercher dans le run (jamais écrites nulle part)."""
    path = agent_dir / "auth.json"
    if not path.exists():
        return []
    values: list[str] = []

    def walk(value):
        if isinstance(value, dict):
            for item in value.values():
                walk(item)
        elif isinstance(value, str) and len(value) >= 16:
            values.append(value)

    walk(json.loads(path.read_text()))
    return values


def secret_values(cache: Cache, capture: bool) -> list[str]:
    """Valeurs factices du mode capture et, en run réel, celles de `auth.json`."""
    values = list(FAKE_SECRETS.values()) + [FAKE_ACCESS, FAKE_REFRESH]
    return values if capture else values + auth_values(cache.agent)


REPORT_FILES = ("report.json", "report.md", "tasks.csv")


def commit_version(header: dict, task: dict) -> dict:
    """Version d'une tâche pour un run sans empreinte : `tree_hash` au commit de l'en-tête,
    seulement si l'arbre était propre ; sinon inconnue (`baseline.py` refuse la tâche)."""
    commit = header.get("commit")
    if not commit or header.get("commit_dirty") is not False:
        return {"tree": None, "source": f"inconnue (commit {commit}, "
                                         f"commit_dirty {header.get('commit_dirty')})"}
    return {"tree": tree_hash_at(commit, task["dir"]), "source": f"commit {commit}"}


def rereport(run_dir: Path, cache_root: Path) -> None:
    """Recalcule le rapport d'un run déjà fait depuis ses journaux (sans binaire ni modèle).

    Résultat des tests cachés, sortie et durée du binaire restent ceux du run. Les sessions
    de sous-agents absentes du run (runs antérieurs à YA-07b) sont copiées depuis le
    dossier agent du banc. Pour un run sans empreinte des tâches, elle est recalculée au
    commit de l'en-tête (`commit_version`). Le rapport précédent est gardé (`*.initial.*`).
    """
    old = json.loads((run_dir / "report.json").read_text())
    header = old["header"]
    if sha256(PRICES) != header.get("prices_sha256"):
        fail("prices.json a changé depuis le run : recalcul refusé")
    capture = header.get("mode") == "capture"
    cache = Cache(cache_root, None, capture)
    prices = json.loads(PRICES.read_text())
    tasks = {t["id"]: t for t in load_tasks(sorted({r["task"] for r in old["results"]}),
                                             capture)}
    results, versions = [], {}
    for prev in old["results"]:
        out = run_dir / "tasks" / prev["task"] / f"run-{prev['run']}"
        sessions = out / "sessions"
        main_files = sorted(p for p in sessions.iterdir()
                            if p.is_file() and SESSION_FILE.match(p.name))
        if (sessions / "subagents").is_dir():
            files = sorted((sessions / "subagents").iterdir())
            subs = {"sub_usage": [p for p in files if p.name.endswith(".usage.ndjson")],
                    "sub_sessions": [p for p in files if p.suffix == ".jsonl"]}
        else:
            subs = copy_subagents(cache.agent, main_files, sessions / "subagents")
        copied = {"usage": [p for p in main_files if p.name.endswith(".usage.ndjson")],
                  "sessions": [p for p in main_files if p.suffix == ".jsonl"], **subs}
        session = {**session_logs(copied), "proxy_log": proxies.read_log(out / "proxy.ndjson"),
                   "connect": proxies.read_log(out / "connect.ndjson")}
        graded = {"tests": prev["tests"], "junit": (out / "grading" / "junit.xml").exists()}
        version = prev.get("task_version")
        if version is None:
            if prev["task"] not in versions:
                versions[prev["task"]] = commit_version(header, tasks[prev["task"]])
            version = versions[prev["task"]]
        results.append(task_result(tasks[prev["task"]], prev["run"], prev["agent"], session,
                                   graded, header["model"], header["effort"], prices, version))
        tree = results[-1]["task_version"]["tree"]
        print(f"  {prev['task']} run {prev['run']} : {results[-1]['status']}, "
              f"{results[-1]['subagents']['sessions']} sous-agent(s), version "
              f"{tree[:12] if tree else 'inconnue'} ({results[-1]['task_version']['source']})")
    for name in REPORT_FILES:
        backup = run_dir / name.replace(".", ".initial.", 1)
        if (run_dir / name).exists() and not backup.exists():
            shutil.copyfile(run_dir / name, backup)
    header = {**header, "rereport": {"at": utc_stamp(), "commit": git("rev-parse", "HEAD"),
                                     "commit_dirty": bool(git("status", "--porcelain"))}}
    report = {"header": header, "probe": old["probe"], "warmup": old["warmup"],
              "results": results, "totals": totals(results), "per_task": aggregate(results),
              "notes": report_notes(prices, header["effort"])}
    write_report(run_dir, report)
    report["secret_scan"] = scan_secrets(run_dir, secret_values(cache, capture))
    write_report(run_dir, report)
    if report["secret_scan"]["files_with_hits"]:
        print(f"ATTENTION : valeurs secrètes trouvées dans "
              f"{sorted(report['secret_scan']['files_with_hits'])}")
    print(f"rapport recalculé : {run_dir / 'report.md'}")


# --- Commande ----------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, help="requis sauf avec --validate-gold")
    parser.add_argument("--provider", default=PROVIDER)
    parser.add_argument("--model", default="", help="vide : " + ", ".join(DEFAULT_MODELS))
    parser.add_argument("--effort", default=DEFAULT_EFFORT)
    parser.add_argument("--settings", type=Path, default=HERE / "settings" / "default.json")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--tasks", default="", help="ids séparés par des virgules")
    parser.add_argument("--cache", type=Path, default=DEFAULT_CACHE)
    parser.add_argument("--out", type=Path, default=RUNS, help="dossier des runs")
    parser.add_argument("--capture", action="store_true",
                        help="validation sans modèle : serveur de capture en amont")
    parser.add_argument("--capture-fail", default="",
                        help="mode capture : tâches jouées par le scénario qui échoue")
    parser.add_argument("--capture-reject-model", default="",
                        help="mode capture : le serveur refuse cet id au run de chauffe")
    parser.add_argument("--capture-server-miss-at-turn", type=int, default=None,
                        help="mode capture : à ce tour, le serveur annonce cacheRead = 0 (miss "
                             "serveur simulé, T07)")
    parser.add_argument("--capture-spawn", action="store_true",
                        help="mode capture : chaque tâche joue le scénario `spawn` (un "
                             "sous-agent), pour valider le ramassage de ses sessions")
    parser.add_argument("--rereport", type=Path, default=None, metavar="RUN",
                        help="sans binaire ni modèle : recalcule le rapport d'un run déjà fait "
                             "depuis ses journaux")
    parser.add_argument("--capture-no-websocket", action="store_true",
                        help="mode capture : le serveur refuse le WebSocket (repli SSE, anomalie)")
    parser.add_argument("--validate-gold", action="store_true",
                        help="sans modèle ni binaire : état cassé et correction officielle "
                             f"des tâches, {VALIDATE_RUNS} fois chacun")
    parser.add_argument("--validate-runs", type=int, default=VALIDATE_RUNS)
    parser.add_argument("--validate-profile", choices=["task", "agent"], default="task",
                        help="profil des tests de --validate-gold (agent : phase agent)")
    parser.add_argument("--idle-timeout", type=int, default=IDLE_TIMEOUT_S, metavar="S",
                        help="arrête une tâche dont aucun octet ne passe par le reverse proxy "
                             f"pendant S secondes (statut stalled ; défaut {IDLE_TIMEOUT_S}, "
                             "0 : désactivé)")
    parser.add_argument("--ab", default=None, metavar="JSON",
                        help="A/B (tools/ab.py) : objet JSON noté dans l'en-tête du rapport "
                             "(bras, passe, graine, écart de réglages)")
    parser.add_argument("--record-hosts", action="store_true",
                        help="run de chauffe réel, proxy CONNECT en enregistrement : écrit "
                             f"{HOSTS.relative_to(REPO)}")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.rereport is not None:
        rereport(args.rereport.resolve(), args.cache.expanduser().resolve())
        return
    if platform.system() != "Darwin" or not Path("/usr/bin/sandbox-exec").exists():
        fail("macOS et sandbox-exec requis")
    if args.validate_gold:
        cache = Cache(args.cache.expanduser().resolve(), None, False)
        tasks = load_tasks([t for t in args.tasks.split(",") if t] or None, False)
        for task in tasks:
            prepare_task(task, cache)
        out = args.out / f"{utc_stamp()}-validate-{args.validate_profile}"
        out.mkdir(parents=True)
        sys.exit(0 if validate_gold(tasks, cache, out, args.validate_runs,
                                    args.validate_profile) else 1)
    if args.binary is None:
        fail("--binary requis")
    binary = args.binary.resolve()
    if not binary.is_file():
        fail(f"binaire introuvable : {binary}")
    if args.provider != PROVIDER:
        fail(f"provider {args.provider} non pris en charge (seul {PROVIDER})")
    if args.runs < 1:
        fail("RUNS doit valoir au moins 1")
    if args.idle_timeout < 0:
        fail("--idle-timeout doit valoir 0 ou plus")
    ab_note = json.loads(args.ab) if args.ab else None
    started = time.monotonic()
    if args.capture_spawn and not args.capture:
        fail("--capture-spawn demande --capture")
    if args.record_hosts and args.capture:
        fail("--record-hosts est un run réel, incompatible avec --capture")
    models = [args.model] if args.model else list(DEFAULT_MODELS)
    bundled, bundled_ids = catalog(binary)
    sources = {model: model_source(model, bundled_ids) for model in models}
    settings = json.loads(args.settings.read_text())
    cache = Cache(args.cache.expanduser().resolve(), binary, args.capture)
    if args.capture:
        write_fake_auth(cache.agent)
    elif not (cache.agent / "auth.json").exists():
        fail(f"aucun login dans {cache.agent} (README : login dans le dossier agent dédié)")
    if not args.capture and not args.record_hosts and not HOSTS.exists():
        fail(f"{HOSTS.relative_to(REPO)} absent : l'établir par un run de chauffe "
             "(--record-hosts, README)")
    hosts = proxies.load_hosts(HOSTS)
    tasks = [] if args.record_hosts else load_tasks(
        [t for t in args.tasks.split(",") if t] or None, args.capture)
    capture_fail = {t for t in args.capture_fail.split(",") if t}
    warmup_args = [arg for m in args.capture_reject_model.split(",") if m
                   for arg in ("--reject-model", m)]
    task_args = ["--no-websocket"] if args.capture_no_websocket else []
    if args.capture_server_miss_at_turn is not None:
        task_args += ["--server-miss-at-turn", str(args.capture_server_miss_at_turn)]
    prices = json.loads(PRICES.read_text())

    prepare_kernel(binary, cache)
    for task in tasks:
        prepare_task(task, cache)
    prepare_s = round(time.monotonic() - started, 2)

    manifest = binary_manifest(binary)
    short = (manifest or {}).get("commit") or git("rev-parse", "HEAD") or "nocommit"
    for model in models:
        run_dir = args.out / f"{utc_stamp()}-{short[:7]}-{model}"
        run_dir.mkdir(parents=True)
        print(f"run : {model} -> {run_dir}")
        connect = proxies.ConnectProxy(run_dir / "connect.ndjson", hosts,
                                       record=args.record_hosts)
        proxies.serve(connect)
        timing = {"prepare": prepare_s}
        try:
            # En enregistrement, le proxy CONNECT accepte tout hôte : pas de sonde.
            mark = time.monotonic()
            probe_result = None if args.record_hosts else probe(binary, cache,
                                                                run_dir / "probe", connect, tasks)
            timing["probe"] = round(time.monotonic() - mark, 2)
            mark = time.monotonic()
            warm = warmup(binary, cache, run_dir / "warmup", model, args.effort, sources[model],
                          settings, connect, args.capture, warmup_args)
            if args.record_hosts:
                accepted = sorted({e["host"] for e in proxies.read_log(connect.log.path)
                                   if e.get("event") == "connect" and e.get("accepted")
                                   and e.get("host") != "example.com"})
                HOSTS.parent.mkdir(parents=True, exist_ok=True)
                HOSTS.write_text(
                    f"# Établi par --record-hosts le {datetime.now(timezone.utc).isoformat()}\n"
                    f"# binaire {cache.binary_sha}, modèle {model}, run {run_dir.name}\n"
                    + "".join(f"{host}\n" for host in accepted))
                print(f"hôtes CONNECT observés au run de chauffe : {accepted or 'aucun'}"
                      f" -> {HOSTS.relative_to(REPO)} (warmup ok : {warm['ok']})")
                return
            if not warm["ok"]:
                fail(f"préparation en échec : le run de chauffe de {model} n'a pas abouti "
                     f"(arrêt {warm['stop_reason']}, erreur {warm['error']!r}), voir "
                     f"{run_dir}/warmup")
            timing["warmup"] = round(time.monotonic() - mark, 2)
            mark = time.monotonic()
            results = []
            for task in tasks:
                for index in range(1, args.runs + 1):
                    out = run_dir / "tasks" / task["id"] / f"run-{index}"
                    result = run_task(task, index, binary, cache, out, model, args.effort,
                                      sources[model], settings, connect, args.capture,
                                      capture_fail, task_args, prices,
                                      capture_spawn=args.capture_spawn,
                                      idle_timeout=args.idle_timeout)
                    print(f"  {task['id']} run {index} : {result['status']}")
                    results.append(result)
            timing["tasks"] = round(time.monotonic() - mark, 2)
        finally:
            connect.shutdown()
            connect.server_close()
        header = {
            "started": run_dir.name.split("-")[0], "mode": "capture" if args.capture else "réel",
            "commit": git("rev-parse", "HEAD"), "commit_dirty": bool(git("status", "--porcelain")),
            "binary": str(binary), "binary_sha256": cache.binary_sha,
            "binary_manifest": manifest, "binary_version": version_of([str(binary), "--version"]),
            "models_bundled_sha256": sha256(bundled), "provider": PROVIDER, "model": model,
            "model_source": sources[model].get("source"),
            "model_fiche_source": sources[model].get("fiche_source"),
            "model_fiche_hypotheses": (sources[model].get("fiche_source") or {}).get("hypotheses"),
            "effort": args.effort,
            "settings_file": str(args.settings.resolve().relative_to(REPO))
            if args.settings.resolve().is_relative_to(REPO) else str(args.settings),
            "settings": settings, "settings_non_default": known_settings(settings),
            "prices_version": prices.get("version"), "prices_sha256": sha256(PRICES),
            "cache_write_price": CACHE_WRITE_PRICE, "idle_timeout_s": args.idle_timeout,
            "timing_s": timing, "ab": ab_note, "hosts": sorted(hosts),
            "runs": args.runs, "tasks": [t["id"] for t in tasks],
            "tools": {"python": platform.python_version(), "node": version_of(["node", "--version"]),
                      "uv": version_of(["uv", "--version"])},
        }
        notes = report_notes(prices, args.effort)
        report = {"header": header, "probe": probe_result, "warmup": warm, "results": results,
                  "totals": totals(results), "per_task": aggregate(results), "notes": notes}
        write_report(run_dir, report)
        scan = scan_secrets(run_dir, secret_values(cache, args.capture))
        report["secret_scan"] = scan
        write_report(run_dir, report)
        if scan["files_with_hits"]:
            print(f"ATTENTION : valeurs secrètes trouvées dans {sorted(scan['files_with_hits'])}")
        print(f"rapport : {run_dir / 'report.md'}")


if __name__ == "__main__":
    main()
