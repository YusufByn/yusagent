#!/usr/bin/env python3
"""Smoke bench local (T06, YA-14) : tâches courtes, vrai modèle par abonnement, rapport.

Usage (voir README.md) :
    python3 -I bench/smoke/run.py --binary <dossier>/prime-agent [--model gpt-6.1-sol] ...
    python3 -I bench/smoke/run.py --binary ... --capture        # validation sans modèle

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
  6. Rapport : `runs/<horodatage UTC>-<sha court>-<modèle>/`.

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
import importlib.util
import json
import os
import platform
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
PREP_TIMEOUT_S = 900


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
    digest = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file() and "__pycache__" not in p.parts):
        digest.update(str(path.relative_to(root)).encode() + b"\0")
        digest.update(path.read_bytes() + b"\0")
    return digest.hexdigest()


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
    def __init__(self, root: Path, binary: Path, capture: bool):
        self.root = root
        self.binary_sha = sha256(binary)
        self.home = root / "home"
        self.tmp = root / "tmp"
        self.kernel_venv = root / "kernel-venv" / self.binary_sha[:16]
        # Dossier agent dédié, login fait une fois par Yusuf (README). Le mode capture et
        # la sonde utilisent leurs propres dossiers, avec un identifiant factice.
        self.agent = root / ("capture-agent" if capture else "agent")
        self.probe_agent = root / "probe-agent"
        for path in (self.home, self.tmp):
            path.mkdir(parents=True, exist_ok=True)


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


def agent_profile(cache: "Cache", workspace: Path) -> list[str]:
    """Arguments de sandbox-exec pour une session d'agent dont le workspace est donné."""
    tasks = (cache.root / "tasks").resolve()
    workspace = workspace.resolve()
    task_dir = workspace.parent if workspace.parent.parent == tasks else tasks
    params = {"SMOKE_DIR": HERE, "TASKS_DIR": tasks, "TASK_DIR": task_dir,
              "SOURCES_DIR": (cache.root / "sources").resolve(), "WORKSPACE_DIR": workspace}
    return ["-f", str(SANDBOX_AGENT), *[a for k, v in params.items() for a in ("-D", f"{k}={v}")]]


def run_process(cmd: list[str], env: dict, cwd: Path, out: Path, err: Path, timeout: int,
                sandboxed: bool, profile: list[str] | None = None) -> dict:
    """Lance `cmd` ; sous sandbox, profil réseau du banc de capture sauf `profile` donné."""
    if sandboxed:
        cmd = ["/usr/bin/sandbox-exec", *(profile or ["-f", str(SANDBOX)]), *cmd]
    started = time.monotonic()
    with out.open("w") as stdout, err.open("w") as stderr:
        proc = subprocess.Popen(cmd, env=env, cwd=cwd, stdin=subprocess.DEVNULL, stdout=stdout,
                                stderr=stderr, start_new_session=True)
        try:
            code = proc.wait(timeout=timeout)
            timed_out = False
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            code, timed_out = None, True
    return {"exit": code, "timeout": timed_out, "seconds": round(time.monotonic() - started, 2)}


def agent_cmd(binary: Path, model: str, effort: str, prompt: str) -> list[str]:
    return [str(binary), "-p", "--mode", "json", "--provider", PROVIDER, "--model", model,
            "--thinking", effort, prompt]


def session_files(agent_dir: Path) -> dict[Path, int]:
    sessions = agent_dir / "sessions"
    if not sessions.exists():
        return {}
    return {p: p.stat().st_size for p in sessions.rglob("*")
            if p.is_file() and (p.name.endswith(".usage.ndjson") or p.suffix == ".jsonl")}


def copy_new_sessions(agent_dir: Path, before: dict[Path, int], dest: Path) -> dict:
    """Copie les sessions créées ou agrandies (sous-agents compris) dans le run."""
    dest.mkdir(parents=True, exist_ok=True)
    usage, sessions = [], []
    for path, size in sorted(session_files(agent_dir).items()):
        if before.get(path) == size:
            continue
        target = dest / path.name
        shutil.copyfile(path, target)
        (usage if path.name.endswith(".usage.ndjson") else sessions).append(target)
    return {"usage": usage, "sessions": sessions}


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
        name = case.get("name", "")
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


def run_tests(task: dict, workspace: Path, out: Path, cache: Cache, sandboxed: bool) -> dict:
    """Copie `hidden/` dans le workspace et lance `test` ; résultat par identifiant."""
    hidden = task["dir"] / "hidden"
    if hidden.exists():
        shutil.copytree(hidden, workspace, dirs_exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    junit = out / "junit.xml"
    junit.unlink(missing_ok=True)
    env = {"PATH": tool_path(), "HOME": str(cache.home), "TMPDIR": str(cache.tmp),
           "LANG": "en_US.UTF-8", "SMOKE_JUNIT": str(junit), "UV_OFFLINE": "1"}
    result = run_process(["/bin/sh", "-c", task["test"]], env, workspace, out / "test.out",
                         out / "test.err", GRADE_TIMEOUT_S, sandboxed)
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
    print(f"préparation : tâche {task['id']} (réseau autorisé)")
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
    logs = cache.root / "tasks" / task["id"] / "prepare"
    logs.mkdir(parents=True, exist_ok=True)
    env = {"PATH": tool_path(), "HOME": str(cache.home), "TMPDIR": str(cache.tmp),
           "LANG": "en_US.UTF-8", "UV_CACHE_DIR": str(cache.root / "uv-cache")}
    setup = run_process(["/bin/sh", "-c", task.get("setup", "true")], env, workspace,
                        logs / "setup.out", logs / "setup.err", PREP_TIMEOUT_S, sandboxed=False)
    if setup["exit"] != 0:
        fail(f"tâche {task['id']} : setup en échec, voir {logs}/setup.err")
    shutil.copytree(workspace, pristine, symlinks=True)
    graded = run_tests(task, workspace, logs, cache, sandboxed=True)
    wrong = {tid: status for tid, status in graded["tests"].items()
             if status != ("failed" if tid in task.get("fail_to_pass", []) else "passed")}
    if wrong:
        fail(f"tâche {task['id']} invalide sur l'état cassé : {wrong} (attendu : fail_to_pass "
             f"en échec, pass_to_pass en succès), voir {logs}")
    info = {"task": task["id"], "tree": tree_hash(task["dir"]), "broken_state": graded["tests"]}
    marker.write_text(json.dumps(info, indent=2) + "\n")
    return info


def restore(task: dict, cache: Cache) -> Path:
    workspace, pristine, _ = task_paths(task, cache)
    shutil.rmtree(workspace, ignore_errors=True)
    shutil.copytree(pristine, workspace, symlinks=True)
    return workspace


# --- Sonde, chauffe, phase agent ---------------------------------------------


def agent_session(binary: Path, cache: Cache, agent_dir: Path, out: Path, model: str,
                  effort: str, prompt: str, cwd: Path, access: Access, connect: proxies.ConnectProxy,
                  source: dict, settings: dict, capture: bool, timeout: int) -> dict:
    """Une session d'agent sous sandbox, avec ses journaux copiés dans `out`."""
    out.mkdir(parents=True, exist_ok=True)
    write_agent_config(agent_dir, access.port, source, settings)
    before = session_files(agent_dir)
    connect_before = len(proxies.read_log(connect.log.path))
    env = agent_env(cache, agent_dir, connect.port, capture)
    process = run_process(agent_cmd(binary, model, effort, prompt), env, cwd,
                          out / "out.jsonl", out / "err.log", timeout, sandboxed=True,
                          profile=agent_profile(cache, cwd))
    copied = copy_new_sessions(agent_dir, before, out / "sessions")
    records = metrics.read_ndjson(copied["usage"])
    entries = metrics.session_entries(copied["sessions"])
    connect_entries = proxies.read_log(connect.log.path)[connect_before:]
    with (out / "connect.ndjson").open("w") as handle:
        for entry in connect_entries:
            handle.write(json.dumps(entry) + "\n")
    return {"process": process, "records": records, "entries": entries,
            "proxy_log": proxies.read_log(access.log_path), "connect": connect_entries}


def work_dir(cache: Cache, name: str) -> Path:
    """Projet vide hors de bench/smoke/ (refusé par le profil de la phase agent)."""
    path = cache.root / "work" / name
    shutil.rmtree(path, ignore_errors=True)
    path.mkdir(parents=True)
    return path


def read_probe_args(paths: list[Path]) -> list[str]:
    return [arg for path in paths for arg in ("--read-probe-path", str(path.resolve()))]


def hidden_targets(task: dict, cache: Cache) -> list[Path]:
    """Chemins que la sonde d'isolation tente de lire : un test caché, la copie pristine."""
    hidden = sorted(p for p in (task["dir"] / "hidden").rglob("*") if p.is_file())
    _, pristine, marker = task_paths(task, cache)
    return hidden[:1] + [pristine, marker]


def probe(binary: Path, cache: Cache, out: Path, connect: proxies.ConnectProxy,
          tasks: list[dict]) -> dict:
    """Sonde réseau sous sandbox, contre le serveur de capture (aucun modèle réel)."""
    write_fake_auth(cache.probe_agent)
    project = work_dir(cache, "probe")
    targets = hidden_targets(tasks[0], cache) if tasks else [HERE / "README.md"]
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
             prices: dict) -> dict:
    workspace = restore(task, cache)
    prompt = (task["dir"] / "prompt.md").read_text()
    scenario = f"smoke-{'fail' if task['id'] in capture_fail else task['id']}"
    access = Access(out, capture, scenario, connect.port,
                    server_args + read_probe_args(hidden_targets(task, cache)))
    try:
        session = agent_session(binary, cache, cache.agent, out, model, effort, prompt,
                                workspace, access, connect, source, settings, capture,
                                int(task.get("timeout_s", 1800)))
    finally:
        access.stop()
    graded = run_tests(task, workspace, out / "grading", cache, sandboxed=True)
    records, entries = session["records"], session["entries"]
    process = session["process"]
    final = metrics.final_assistant(entries)
    resolved = graded["junit"] and all(s == "passed" for s in graded["tests"].values())
    if not entries:
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
    calls = metrics.calls(records)
    anomalies = []
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
    markers = metrics.kernel_markers(entries)
    isolated = metrics.isolation(markers)
    if isolated and isolated["readable"]:
        anomalies.append(f"lecture hors workspace réussie : {isolated['readable']}")
    return {
        "task": task["id"], "run": index, "status": status, "resolved": status == "resolved",
        "tests": graded["tests"], "agent": process, "final": final,
        "tokens": metrics.token_totals(records), "calls": calls,
        "cache": metrics.cache_rates(records),
        "cost": metrics.costs(prices, model, records, CACHE_WRITE_PRICE),
        "cells": metrics.cell_errors(entries),
        "transports": {k: v for k, v in transports.items() if k != "per_request"},
        "transport_failures_in_session": metrics.session_transport_failures(entries),
        "per_request": transports["per_request"],
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
    keys = ("masking", "rlm", "cacheRetention", "transport")
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
            "tokens_mean": statistics.mean(tokens),
            "tokens_stdev": statistics.stdev(tokens) if len(tokens) > 1 else None,
            "turns_mean": statistics.mean(r["calls"]["turns"] for r in runs),
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
    read = sum(r["tokens"]["cacheRead"] or 0 for r in results)
    tokens = {k: (sum(r["tokens"][k] for r in results)
                  if results and all(r["tokens"][k] is not None for r in results) else None)
              for k in metrics.TOKEN_FIELDS}
    prompt = sum((tokens[k] or 0) for k in ("input", "cacheRead", "cacheWrite"))
    return {"task_runs": len(results), "resolved": resolved,
            "cost": cost, "cost_per_resolved": cost / resolved if cost is not None and resolved
            else None,
            "tokens": tokens,
            "turns_median": metrics.median([r["calls"]["turns"] for r in results]),
            "cache_rate_all_calls": read / prompt if prompt else None,
            "loop_on_sse": sum(r["transports"]["loop_on_sse"] for r in results),
            "connect_refused": sum(r["connect"]["refused"] for r in results),
            "anomalies": sum(len(r["anomalies"]) for r in results)}


def fmt(value, digits: int = 4) -> str:
    if value is None:
        return "null"
    if isinstance(value, float):
        return f"{value:.{digits}f}"
    return str(value)


def write_report(run_dir: Path, report: dict) -> None:
    (run_dir / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    fields = ["task", "run", "status", "cost", "input", "output", "cacheRead", "cacheWrite",
              "reasoning", "turns", "calls", "cache_session_rate", "cache_continuation_median",
              "ttft_median_ms", "seconds", "websocket", "sse", "loop_on_sse", "connect_refused",
              "cell_errors", "syntax_errors", "truncated"]
    with (run_dir / "tasks.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for r in report["results"]:
            writer.writerow({
                "task": r["task"], "run": r["run"], "status": r["status"],
                "cost": r["cost"]["total"], **{k: r["tokens"][k] for k in
                                               ("input", "output", "cacheRead", "cacheWrite",
                                                "reasoning")},
                "turns": r["calls"]["turns"], "calls": r["calls"]["total"],
                "cache_session_rate": r["cache"]["session_rate"],
                "cache_continuation_median": r["cache"]["continuation_median"],
                "ttft_median_ms": r["calls"]["ttft_median_ms"], "seconds": r["agent"]["seconds"],
                "websocket": r["transports"]["requests"].get("websocket", 0),
                "sse": r["transports"]["requests"].get("sse", 0),
                "loop_on_sse": r["transports"]["loop_on_sse"],
                "connect_refused": r["connect"]["refused"], "cell_errors": r["cells"]["errors"],
                "syntax_errors": r["cells"]["syntax_errors"], "truncated": r["calls"]["truncated"]})
    h, t = report["header"], report["totals"]
    lines = [f"# Smoke bench — {h['model']} — {h['started']}", "",
             "| Champ | Valeur |", "|---|---|"]
    for key in ("mode", "commit", "commit_dirty", "binary", "binary_sha256", "binary_manifest",
                "binary_version", "models_bundled_sha256", "provider", "model", "model_source",
                "effort", "settings_file", "settings_non_default", "prices_version",
                "prices_sha256", "cache_write_price", "hosts", "runs", "tasks"):
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
              f"- Coût théorique : {fmt(t['cost'])} $ ; par tâche résolue : "
              f"{fmt(t['cost_per_resolved'])} $",
              f"- Tokens : {json.dumps(t['tokens'])}",
              f"- Tours médians : {fmt(t['turns_median'], 1)} ; taux de cache (tous appels) : "
              f"{fmt(t['cache_rate_all_calls'])}",
              f"- Appels de boucle en SSE (anomalie) : **{t['loop_on_sse']}** ; CONNECT refusés :"
              f" {t['connect_refused']} ; anomalies : {t['anomalies']}",
              "", "## Tâches", "",
              "| Tâche | Run | Statut | Coût $ | input | output | cacheRead | reasoning | Tours |"
              " Cache session | Cache contin. (méd.) | TTFT méd. ms | Durée s | WS | SSE |"
              " Cellules en erreur (Syntax) | Coupées |",
              "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in report["results"]:
        lines.append(
            f"| {r['task']} | {r['run']} | {r['status']} | {fmt(r['cost']['total'])} | "
            f"{fmt(r['tokens']['input'])} | {fmt(r['tokens']['output'])} | "
            f"{fmt(r['tokens']['cacheRead'])} | {fmt(r['tokens']['reasoning'])} | "
            f"{r['calls']['turns']} | {fmt(r['cache']['session_rate'])} | "
            f"{fmt(r['cache']['continuation_median'])} | {fmt(r['calls']['ttft_median_ms'], 1)} | "
            f"{r['agent']['seconds']} | {r['transports']['requests'].get('websocket', 0)} | "
            f"{r['transports']['requests'].get('sse', 0)} | {r['cells']['errors']} "
            f"({r['cells']['syntax_errors']}) | {r['calls']['truncated']} |")
    if report["header"]["runs"] > 1:
        lines += ["", "## Variabilité entre runs", "",
                  "| Tâche | Runs | Taux résolu | Bascule | Coût moyen | Écart-type coût |"
                  " Tokens moyens | Écart-type tokens |", "|---|---|---|---|---|---|---|---|"]
        for task, a in report["per_task"].items():
            lines.append(f"| {task} | {a['runs']} | {fmt(a['resolved_rate'], 2)} | {a['flip']} | "
                         f"{fmt(a['cost_mean'])} | {fmt(a['cost_stdev'])} | "
                         f"{fmt(a['tokens_mean'], 1)} | {fmt(a['tokens_stdev'], 1)} |")
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


# --- Commande ----------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
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
    parser.add_argument("--capture-no-websocket", action="store_true",
                        help="mode capture : le serveur refuse le WebSocket (repli SSE, anomalie)")
    parser.add_argument("--record-hosts", action="store_true",
                        help="run de chauffe réel, proxy CONNECT en enregistrement : écrit "
                             f"{HOSTS.relative_to(REPO)}")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if platform.system() != "Darwin" or not Path("/usr/bin/sandbox-exec").exists():
        fail("macOS et sandbox-exec requis")
    binary = args.binary.resolve()
    if not binary.is_file():
        fail(f"binaire introuvable : {binary}")
    if args.provider != PROVIDER:
        fail(f"provider {args.provider} non pris en charge (seul {PROVIDER})")
    if args.runs < 1:
        fail("RUNS doit valoir au moins 1")
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
    prices = json.loads(PRICES.read_text())

    prepare_kernel(binary, cache)
    for task in tasks:
        prepare_task(task, cache)

    manifest = binary_manifest(binary)
    short = (manifest or {}).get("commit") or git("rev-parse", "HEAD") or "nocommit"
    for model in models:
        run_dir = args.out / f"{utc_stamp()}-{short[:7]}-{model}"
        run_dir.mkdir(parents=True)
        print(f"run : {model} -> {run_dir}")
        connect = proxies.ConnectProxy(run_dir / "connect.ndjson", hosts,
                                       record=args.record_hosts)
        proxies.serve(connect)
        try:
            # En enregistrement, le proxy CONNECT accepte tout hôte : pas de sonde.
            probe_result = None if args.record_hosts else probe(binary, cache,
                                                                run_dir / "probe", connect, tasks)
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
            results = []
            for task in tasks:
                for index in range(1, args.runs + 1):
                    out = run_dir / "tasks" / task["id"] / f"run-{index}"
                    result = run_task(task, index, binary, cache, out, model, args.effort,
                                      sources[model], settings, connect, args.capture,
                                      capture_fail, task_args, prices)
                    print(f"  {task['id']} run {index} : {result['status']}")
                    results.append(result)
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
            "effort": args.effort,
            "settings_file": str(args.settings.resolve().relative_to(REPO))
            if args.settings.resolve().is_relative_to(REPO) else str(args.settings),
            "settings": settings, "settings_non_default": known_settings(settings),
            "prices_version": prices.get("version"), "prices_sha256": sha256(PRICES),
            "cache_write_price": CACHE_WRITE_PRICE, "hosts": sorted(hosts),
            "runs": args.runs, "tasks": [t["id"] for t in tasks],
            "tools": {"python": platform.python_version(), "node": version_of(["node", "--version"]),
                      "uv": version_of(["uv", "--version"])},
        }
        notes = [
            "Tokens et usage : journal d'usage du harness (`<id>.usage.ndjson`), rien n'est "
            "recalculé. Pour l'API Responses, `cacheWrite` vaut 0 après normalisation "
            "(docs/usage.md) : l'écriture en cache est comptée au tarif d'entrée.",
            f"Coût théorique : tarif API Standard de prices.json (version {prices.get('version')}), "
            f"écriture au tarif `{CACHE_WRITE_PRICE}` (le corps Codex ne porte pas de rétention), "
            "contexte long au-delà du seuil de prices.json.",
            "Transport : WebSocket attendu pour les appels de boucle (main, subagent) ; "
            "compaction, /refine et résumé de branche passent en SSE par construction.",
            "Taux de cache de session : cacheRead / (input + cacheRead + cacheWrite) sur les "
            "appels de boucle ; continuation : appels de boucle hors premier de leur session et "
            "hors invalidation journalisée.",
        ]
        if args.effort != DEFAULT_EFFORT:
            notes.append(f"Effort {args.effort} au lieu de {DEFAULT_EFFORT} (décision du 04/11).")
        report = {"header": header, "probe": probe_result, "warmup": warm, "results": results,
                  "totals": totals(results), "per_task": aggregate(results), "notes": notes}
        write_report(run_dir, report)
        secret_values = list(FAKE_SECRETS.values()) + [FAKE_ACCESS, FAKE_REFRESH]
        if not args.capture:
            secret_values += auth_values(cache.agent)
        scan = scan_secrets(run_dir, secret_values)
        report["secret_scan"] = scan
        write_report(run_dir, report)
        if scan["files_with_hits"]:
            print(f"ATTENTION : valeurs secrètes trouvées dans {sorted(scan['files_with_hits'])}")
        print(f"rapport : {run_dir / 'report.md'}")


if __name__ == "__main__":
    main()
