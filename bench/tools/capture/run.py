#!/usr/bin/env python3
"""Mesures de l'audit YA-00 : lance le harness contre le serveur de capture et analyse.

Usage (commande unique) :
    python3 bench/tools/capture/run.py --binary /chemin/vers/prime-agent

Deux phases bien séparées :
  1. Préparation (réseau autorisé, une fois par binaire) : création du venv du
     kernel par un run de chauffe, et récupération de tiktoken + o200k_base pour
     l'estimation des tokens. Rien de cette phase n'entre dans les mesures.
  2. Mesures (réseau limité à localhost par sandbox-exec, voir sandbox.sb) :
     les cas ci-dessous, puis l'analyse (analyze.py) qui écrit report.md et
     report.json dans le dossier de sortie.

Bibliothèque standard uniquement ; macOS (sandbox-exec) requis.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
SANDBOX = HERE / "sandbox.sb"
TIKTOKEN_REQ = "tiktoken==0.12.0"
PROMPT = "Exécute le scénario de capture."
PROMPT_2 = "Deuxième message : réponds sans outil."
CASE_TIMEOUT_S = 240

# (nom, api, scénario, arguments serveur en plus, dossier agent, prompts, settings.json)
# Chaque prompt après le premier relance le binaire avec `--continue` (même
# session). s1 et s2 partagent le dossier agent et le projet : deux sessions
# successives d'un même utilisateur. compact-anthropic annonce un usage proche
# de la fenêtre pour déclencher la compaction ; son scénario « long » produit
# assez de sortie pour dépasser keepRecentTokens (défaut 20 000), sinon la
# compaction est sautée (« Session is too short to compact »).
CASES = [
    ("s1-anthropic", "anthropic-messages", "basic", [], "agent-a", [PROMPT], None),
    ("s2-anthropic", "anthropic-messages", "basic", [], "agent-a", [PROMPT], None),
    ("s1-responses", "openai-responses", "basic", [], "agent-r", [PROMPT], None),
    ("crash-anthropic", "anthropic-messages", "crash", [], "agent-c", [PROMPT], None),
    ("resume-anthropic", "anthropic-messages", "basic", [], "agent-m", [PROMPT, PROMPT_2], None),
    ("compact-anthropic", "anthropic-messages", "long",
     ["--inflate-from-turn", "1", "--inflate-tokens", "199000"], "agent-k", [PROMPT, PROMPT_2],
     None),
    # T04 (YA-13) : message_delta ne porte que output_tokens, l'entrée vient du
    # message_start (correctif de l'usage Anthropic).
    ("usage-split-anthropic", "anthropic-messages", "basic", ["--usage-split"], "agent-u",
     [PROMPT], None),
    # T04 (YA-13) : compaction avec /refine automatique réactivé (coupé par défaut
    # depuis YA-01), pour journaliser un appel `refine`.
    ("refine-anthropic", "anthropic-messages", "long",
     ["--inflate-from-turn", "1", "--inflate-tokens", "199000"], "agent-f", [PROMPT, PROMPT_2],
     {"harnessRefinements": True, "autoRefine": {"enabled": True, "compact": True}}),
]

# Variables factices : le tour 2 du scénario vérifie si le kernel voit des
# noms de secrets. Aucune n'est une vraie clé.
FAKE_SECRETS = {
    "ANTHROPIC_API_KEY": "capture-fake-not-a-key",
    "YA_CAPTURE_FAKE_TOKEN": "capture-fake-not-a-token",
}
# Variable non sensible déclarée dans `kernel.envAllowlist` : le kernel doit
# la voir (YA-02). ANTHROPIC_API_KEY y est aussi déclarée : identifiant de
# provider, il ne doit jamais passer.
ALLOWED_VARS = {"YA_CAPTURE_ALLOWED": "visible"}
KERNEL_SETTINGS = {"kernel": {"envAllowlist": ["YA_CAPTURE_ALLOWED", "ANTHROPIC_API_KEY"]}}
# TTL du cache de prompt fixé explicitement (T03) : la variable, prioritaire et
# lue aussi par un binaire antérieur à T03, et le réglage `cacheRetention`.
CACHE_RETENTION = "short"
CACHE_SETTINGS = {"cacheRetention": CACHE_RETENTION}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def base_env(home: Path, agent_dir: Path, tmp: Path) -> dict[str, str]:
    env = {
        "PATH": f"/usr/bin:/bin:/usr/sbin:/sbin:{Path(shutil.which('uv') or '/opt/homebrew/bin/uv').parent}",
        "HOME": str(home),
        "TMPDIR": str(tmp),
        "LANG": "en_US.UTF-8",
        "DO_NOT_TRACK": "1",
        "PI_OFFLINE": "1",
        "PI_SKIP_VERSION_CHECK": "1",
        "PI_CACHE_RETENTION": CACHE_RETENTION,
        "PRIME_AGENT_CODING_AGENT_DIR": str(agent_dir),
    }
    env.update(FAKE_SECRETS)
    env.update(ALLOWED_VARS)
    return env


def write_settings(agent_dir: Path, settings: dict | None = None) -> None:
    agent_dir.mkdir(parents=True, exist_ok=True)
    (agent_dir / "settings.json").write_text(
        json.dumps({**KERNEL_SETTINGS, **CACHE_SETTINGS, **(settings or {})}) + "\n")


def write_models_json(agent_dir: Path, api: str, port: int) -> None:
    agent_dir.mkdir(parents=True, exist_ok=True)
    config = {"providers": {"capture": {
        "baseUrl": f"http://127.0.0.1:{port}" + ("/v1" if api == "openai-responses" else ""),
        "apiKey": "capture-dummy",
        "api": api,
        "models": [{
            "id": "capture-model", "name": "Capture", "reasoning": False, "input": ["text"],
            "contextWindow": 200000, "maxTokens": 32000,
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        }],
    }}}
    (agent_dir / "models.json").write_text(json.dumps(config, indent=2) + "\n")


def make_project(project: Path) -> None:
    """Projet vide hors du dépôt (aucun AGENTS.md parent), en dépôt git jetable."""
    project.mkdir(parents=True, exist_ok=True)
    (project / "README.md").write_text("# capture\n")
    git = ["git", "-C", str(project), "-c", "user.name=capture",
           "-c", "user.email=capture@localhost", "-c", "commit.gpgsign=false"]
    dates = {"GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"}
    subprocess.run(["git", "init", "-q", "-b", "main", str(project)], check=True)
    subprocess.run(git + ["add", "README.md"], check=True)
    subprocess.run(git + ["commit", "-q", "-m", "init"], check=True, env={**os.environ, **dates})


class CaptureServer:
    def __init__(self, out: Path, scenario: str, extra: list[str]):
        out.mkdir(parents=True, exist_ok=True)
        port_file = out.parent / "port"
        port_file.unlink(missing_ok=True)
        self.log = (out.parent / "server.log").open("w")
        self.proc = subprocess.Popen(
            [sys.executable, "-I", str(HERE / "server.py"), "--out", str(out),
             "--scenario", scenario, "--port-file", str(port_file), *extra],
            stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 10
        while not port_file.exists() or not port_file.read_text().strip():
            if time.monotonic() > deadline or self.proc.poll() is not None:
                raise RuntimeError("le serveur de capture n'a pas démarré")
            time.sleep(0.05)
        self.port = int(port_file.read_text())

    def stop(self) -> None:
        self.proc.send_signal(signal.SIGINT)
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.log.close()


def journal_sizes(agent_dir: Path) -> dict[Path, int]:
    """Journaux d'usage (T04, YA-13) du dossier agent, avec leur taille."""
    return {path: path.stat().st_size for path in agent_dir.rglob("*.usage.ndjson")}


def copy_journals(agent_dir: Path, before: dict[Path, int], case_dir: Path) -> list[str]:
    """Copie dans `<cas>/usage/` les journaux créés ou agrandis pendant le cas."""
    copied = []
    for path, size in sorted(journal_sizes(agent_dir).items()):
        if before.get(path) == size:
            continue
        (case_dir / "usage").mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, case_dir / "usage" / path.name)
        copied.append(path.name)
    return copied


def run_binary(cmd: list[str], env: dict[str, str], cwd: Path, case_dir: Path,
               sandboxed: bool, suffix: str = "") -> dict:
    if sandboxed:
        cmd = ["/usr/bin/sandbox-exec", "-f", str(SANDBOX), *cmd]
    started = time.monotonic()
    with (case_dir / f"out{suffix}.jsonl").open("w") as out, \
            (case_dir / f"err{suffix}.log").open("w") as err:
        try:
            proc = subprocess.run(cmd, env=env, cwd=cwd, stdout=out, stderr=err,
                                  timeout=CASE_TIMEOUT_S)
            code = proc.returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
    return {"exit": code, "seconds": round(time.monotonic() - started, 2)}


def agent_cmd(binary: Path, prompt: str = PROMPT, resume: bool = False) -> list[str]:
    return [str(binary), "-p", *(["--continue"] if resume else []), "--mode", "json",
            "--provider", "capture", "--model", "capture-model", prompt]


def prepare(binary: Path, cache: Path, version: str) -> None:
    """Phase réseau : venv du kernel et tiktoken. Idempotente par binaire."""
    marker = cache / f"prepared-{sha256(binary)[:16]}"
    if marker.exists():
        print(f"préparation : déjà faite pour ce binaire ({marker.name})")
        prepare_analysis_venv(cache)
        return
    print("préparation (réseau autorisé) : venv du kernel, tiktoken")
    warm = cache / "warmup"
    shutil.rmtree(warm, ignore_errors=True)
    (warm / "tmp").mkdir(parents=True)
    make_project(warm / "project")
    server = CaptureServer(warm / "cap", "basic", [])
    try:
        write_models_json(warm / "agent", "anthropic-messages", server.port)
        env = base_env(cache / "home", warm / "agent", warm / "tmp")
        result = run_binary(agent_cmd(binary), env, warm / "project", warm, sandboxed=False)
    finally:
        server.stop()
    if result["exit"] != 0:
        sys.exit(f"run de chauffe en échec ({result}), voir {warm}/err.log")
    prepare_analysis_venv(cache)
    marker.write_text(f"{version}\n")


def prepare_analysis_venv(cache: Path) -> None:
    """Venv d'analyse avec tiktoken et le fichier o200k_base en cache local."""
    venv = cache / "analysis-venv"
    if (venv / "bin" / "python").exists():
        return
    uv_env = {**os.environ, "UV_CACHE_DIR": str(cache / "uv")}
    subprocess.run(["uv", "venv", "-q", "--python", sys.executable, str(venv)],
                   check=True, env=uv_env)
    subprocess.run(["uv", "pip", "install", "-q", "--python", str(venv / "bin" / "python"),
                    TIKTOKEN_REQ], check=True, env=uv_env)
    subprocess.run([str(venv / "bin" / "python"), "-I", "-c",
                    "import tiktoken; tiktoken.get_encoding('o200k_base')"],
                   check=True, env={**os.environ, "TIKTOKEN_CACHE_DIR": str(cache / "tiktoken")})


def network_control(out: Path) -> dict:
    """Preuve que le profil sandbox bloque le réseau hors localhost."""
    proc = subprocess.run(["/usr/bin/sandbox-exec", "-f", str(SANDBOX), "/usr/bin/curl", "-sS",
                           "-m", "5", "-o", "/dev/null", "https://example.com"],
                          capture_output=True, text=True)
    return {"command": "sandbox-exec -f sandbox.sb curl https://example.com",
            "exit": proc.returncode, "stderr": proc.stderr.strip()}


def acp_check(binary: Path, cache: Path, work: Path, project: Path, out: Path) -> dict:
    """Échange ACP minimal (initialize, session/new, session/prompt) sur un daemon isolé.

    Le mode ACP passe par le daemon : `--daemon-socket` le garde hors du socket
    partagé par défaut (/tmp/prime-agent-<uid>/daemon.sock), et il est arrêté
    à la fin quoi qu'il arrive.
    """
    acp_dir = out / "acp"
    acp_dir.mkdir(parents=True, exist_ok=True)
    agent = work / "agent-acp"
    socket = work / "d.sock"
    server = CaptureServer(acp_dir / "cap", "basic", [])
    write_models_json(agent, "anthropic-messages", server.port)
    write_settings(agent, {"defaultProvider": "capture", "defaultModel": "capture-model"})
    env = base_env(cache / "home", agent, work / "tmp")
    lines: queue.Queue[str] = queue.Queue()
    transcript: list[dict] = []
    err = (acp_dir / "err.log").open("w")
    proc = subprocess.Popen(
        ["/usr/bin/sandbox-exec", "-f", str(SANDBOX), str(binary), "--mode", "acp",
         "--daemon-socket", str(socket)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env, cwd=project,
        text=True, bufsize=1)
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()

    def request(req_id: int, method: str, params: dict) -> dict | None:
        message = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
        transcript.append({"dir": "client->agent", "msg": message})
        proc.stdin.write(json.dumps(message) + "\n")
        proc.stdin.flush()
        deadline = time.monotonic() + CASE_TIMEOUT_S
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(lines.get(timeout=1))
            except queue.Empty:
                continue
            except json.JSONDecodeError:
                continue
            transcript.append({"dir": "agent->client", "msg": incoming})
            if incoming.get("id") == req_id and "method" not in incoming:
                return incoming
        return None

    try:
        init = request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        session_id = ((new or {}).get("result") or {}).get("sessionId")
        prompt = request(3, "session/prompt", {
            "sessionId": session_id, "prompt": [{"type": "text", "text": PROMPT}]})
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        err.close()
        # `prime-agent shutdown` ne vise que le socket par défaut : on termine le
        # superviseur de ce socket, qui arrête ses workers et leurs kernels.
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {socket}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and subprocess.run(
                ["pgrep", "-f", str(binary)], capture_output=True).returncode == 0:
            time.sleep(0.2)
        server.stop()
    (acp_dir / "transcript.jsonl").write_text(
        "".join(json.dumps(entry, ensure_ascii=False) + "\n" for entry in transcript))
    leftovers = subprocess.run(["pgrep", "-f", str(binary)], capture_output=True, text=True)
    return {
        "initialize": (init or {}).get("result"),
        "session_new_ok": session_id is not None,
        "prompt_result": (prompt or {}).get("result") or (prompt or {}).get("error"),
        "agent_requests": sorted({e["msg"]["method"] for e in transcript
                                  if e["dir"] == "agent->client" and "method" in e["msg"]
                                  and "id" in e["msg"]}),
        "notifications": sorted({f"{e['msg']['method']}:"
                                 f"{(e['msg'].get('params') or {}).get('update', {}).get('sessionUpdate')}"
                                 for e in transcript if e["dir"] == "agent->client"
                                 and "method" in e["msg"] and "id" not in e["msg"]}),
        "leftover_processes": leftovers.stdout.split(),
    }


def spawn_check(binary: Path, cache: Path, work: Path, project: Path, out: Path) -> dict:
    """Sous-agent de bout en bout : `rlm.spawn`, réponse de l'enfant par
    `agent_message.send`, puis `rlm.collect` côté parent (scénario `spawn`).

    Hors de CASES : les requêtes de l'enfant, mêlées à celles du parent,
    fausseraient l'analyse de préfixe N -> N+1.
    """
    spawn_dir = out / "spawn"
    spawn_dir.mkdir(parents=True, exist_ok=True)
    agent = work / "agent-s"
    server = CaptureServer(spawn_dir / "cap", "spawn", [])
    before = journal_sizes(agent)
    try:
        write_models_json(agent, "anthropic-messages", server.port)
        write_settings(agent)
        env = base_env(cache / "home", agent, work / "tmp")
        result = run_binary(agent_cmd(binary), env, project, spawn_dir, sandboxed=True)
    finally:
        server.stop()
    result["usage_journals"] = copy_journals(agent, before, spawn_dir)
    parent_texts, child_texts = [], []
    for meta_path in sorted((spawn_dir / "cap").glob("*.meta.json")):
        meta = json.loads(meta_path.read_text())
        body = (spawn_dir / "cap" / meta_path.name.replace(".meta.json", ".body.json")).read_text()
        (child_texts if meta.get("child") else parent_texts).append(body)
    collect = next((line for text in parent_texts for line in _texts(json.loads(text))
                    if line.startswith("COLLECT")), None)
    reply_seen = any("YA_CHILD_RESULT" in text and "agent-message" in text
                     for text in parent_texts)
    child_env = next((line for text in child_texts for line in _texts(json.loads(text))
                      if line.startswith("CHILD_ENV")), None)
    return {
        **result,
        "parent_requests": len(parent_texts),
        "child_requests": len(child_texts),
        "collect_line": collect,
        "child_reply_in_parent_context": reply_seen,
        "child_env_line": child_env,
        "ok": result["exit"] == 0 and bool(collect) and "True" in collect
              and "YA_CHILD_DONE" in collect and reply_seen,
    }


def ps_check(binary: Path, cache: Path, work: Path, project: Path, out: Path) -> dict:
    """Scénario `psenv`, hors sandbox : `/bin/ps` est setuid et le profil
    sandbox-exec en refuse l'exécution (code 126), ce qui casse aussi le shell
    persistant sur macOS. Le provider reste le serveur local, avec
    PI_OFFLINE=1 et DO_NOT_TRACK=1, comme la phase de préparation.
    - Relevé pour YA-11 : `ps eww` lancé depuis le kernel sur son parent (le
      harness) affiche-t-il l'environnement du harness ? Seuls des noms de
      variables factices sortent.
    - YA-02 : noms sensibles vus par `printenv` dans un shell persistant.
    """
    ps_dir = out / "psenv"
    ps_dir.mkdir(parents=True, exist_ok=True)
    agent = work / "agent-p"
    server = CaptureServer(ps_dir / "cap", "psenv", [])
    try:
        write_models_json(agent, "anthropic-messages", server.port)
        write_settings(agent)
        env = base_env(cache / "home", agent, work / "tmp")
        result = run_binary(agent_cmd(binary), env, project, ps_dir, sandboxed=False)
    finally:
        server.stop()
    found: dict[str, str | None] = {"PS_PARENT": None, "PERSISTENT": None}
    for body_path in sorted((ps_dir / "cap").glob("*.body.json")):
        for item in _texts(json.loads(body_path.read_text())):
            for prefix in found:
                if item.startswith(prefix + " "):
                    found[prefix] = item
    return {**result, "sandboxed": False, "ps_line": found["PS_PARENT"],
            "persistent_line": found["PERSISTENT"]}


def _texts(value) -> list[str]:
    """Lignes de texte d'un corps de requête (résultats de tool compris)."""
    lines: list[str] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key in ("text", "content", "output") and isinstance(item, str):
                lines += item.splitlines()
            else:
                lines += _texts(item)
    elif isinstance(value, list):
        for item in value:
            lines += _texts(item)
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", type=Path,
                        default=HERE / "runs" / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    # Hors du dépôt : venv du kernel, Python uv et cache uv pèsent ~530 Mo.
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary = args.binary.resolve()
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
    prepare(binary, cache, version)

    # Projet hors du dépôt : la découverte d'AGENTS.md remonte les dossiers parents.
    work = Path(os.environ.get("YA_CAPTURE_WORK", "/private/tmp/ya-capture"))
    shutil.rmtree(work, ignore_errors=True)
    (work / "tmp").mkdir(parents=True)
    project = work / "project"
    make_project(project)

    run_info = {
        "date_utc": datetime.now(timezone.utc).isoformat(),
        "binary": str(binary),
        "binary_sha256": sha256(binary),
        "binary_version": version,
        "repo_commit": subprocess.run(["git", "-C", str(HERE), "rev-parse", "HEAD"],
                                      capture_output=True, text=True).stdout.strip(),
        "project": str(project),
        "env_names": sorted(base_env(Path("/h"), Path("/a"), Path("/t"))),
        "fake_secret_names": sorted(FAKE_SECRETS),
        "allowed_var_names": sorted(ALLOWED_VARS),
        "kernel_settings": KERNEL_SETTINGS,
        "cache_retention": {"PI_CACHE_RETENTION": CACHE_RETENTION, **CACHE_SETTINGS},
        "network_control": network_control(out),
        "cases": {},
    }
    print(f"contrôle réseau sandbox : exit={run_info['network_control']['exit']}")

    # Ventilation du prompt système par couche (commande upstream `prompt`).
    prompt_env = base_env(cache / "home", work / "agent-a", work / "tmp")
    write_models_json(work / "agent-a", "anthropic-messages", 9)
    dump = subprocess.run(["/usr/bin/sandbox-exec", "-f", str(SANDBOX), str(binary), "prompt",
                           "--json", "--model", "capture/capture-model"],
                          capture_output=True, text=True, env=prompt_env, cwd=project)
    (out / "prompt-breakdown.json").write_text(dump.stdout)
    run_info["prompt_command_exit"] = dump.returncode

    for name, api, scenario, extra, agent, prompts, settings in CASES:
        case_dir = out / "cases" / name
        case_dir.mkdir(parents=True, exist_ok=True)
        server = CaptureServer(case_dir / "cap", scenario, extra)
        runs = []
        before = journal_sizes(work / agent)
        try:
            write_models_json(work / agent, api, server.port)
            write_settings(work / agent, settings)
            env = base_env(cache / "home", work / agent, work / "tmp")
            for index, prompt in enumerate(prompts):
                runs.append(run_binary(agent_cmd(binary, prompt, resume=index > 0), env, project,
                                       case_dir, sandboxed=True,
                                       suffix="" if index == 0 else f"-{index + 1}"))
        finally:
            server.stop()
        result = {"exit": [r["exit"] for r in runs], "seconds": [r["seconds"] for r in runs],
                  "api": api, "scenario": scenario, "server_args": extra, "agent_dir": agent,
                  "prompts": prompts, "settings": settings,
                  "usage_journals": copy_journals(work / agent, before, case_dir)}
        run_info["cases"][name] = result
        print(f"{name}: exit={result['exit']} en {result['seconds']} s")

    run_info["spawn"] = spawn_check(binary, cache, work, project, out)
    print(f"spawn : ok={run_info['spawn']['ok']} collect={run_info['spawn']['collect_line']!r} "
          f"requêtes parent/enfant={run_info['spawn']['parent_requests']}"
          f"/{run_info['spawn']['child_requests']}")

    run_info["psenv"] = ps_check(binary, cache, work, project, out)
    print(f"psenv : {run_info['psenv']['ps_line']!r} {run_info['psenv']['persistent_line']!r}")

    run_info["acp"] = acp_check(binary, cache, work, project, out)
    print(f"acp : prompt={run_info['acp']['prompt_result']} "
          f"process restants={run_info['acp']['leftover_processes']}")

    (out / "run.json").write_text(json.dumps(run_info, indent=2, ensure_ascii=False) + "\n")
    analysis_env = {"PATH": "/usr/bin:/bin", "HOME": str(cache / "home"),
                    "TIKTOKEN_CACHE_DIR": str(cache / "tiktoken")}
    subprocess.run(["/usr/bin/sandbox-exec", "-f", str(SANDBOX),
                    str(cache / "analysis-venv" / "bin" / "python"), "-I",
                    str(HERE / "analyze.py"), str(out)],
                   check=True, env=analysis_env, cwd=cache)
    print(f"rapport : {out / 'report.md'}")
    # T04 (YA-13) : journal d'usage et usage enregistré confrontés à l'usage renvoyé.
    subprocess.run([sys.executable, "-I", str(HERE / "usage_check.py"), str(out)], check=True)
    print(f"usage : {out / 'usage-report.md'}")


if __name__ == "__main__":
    main()
