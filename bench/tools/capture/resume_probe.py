#!/usr/bin/env python3
"""Mesures avant la reprise ACP (YA-10, ticket 28) : fichier de session, avis de kernel, veille.

Trois mesures sur le vrai binaire, chacune avec son démon isolé (`--daemon-socket`), le serveur de
capture (modèle factice, scénario `basic`), `sandbox-exec` (réseau limité à localhost), le HOME et
le venv du kernel du banc (fonctions de `run.py`, sans le modifier) :

  M1 fichier   `session/new`, puis : le fichier de session existe-t-il avant le premier prompt ?
               Relevé du dossier des sessions avant et après le prompt.
  M2 avis      reprise avec un worker neuf (R-neuf, cas D du ticket 24) : prompt 1, fin de stdin,
               arrêt du démon, `--resume <fichier>`, prompt 2. Où la ligne `[python-state-restored]`
               apparaît dans les requêtes du prompt 2. Contre-épreuve R-vivant (cas C) : même chose
               sans arrêter le démon.
  M3 veille    durée de vie du worker et du kernel après la fin de stdin, avec
               `idleEvictionMinutes: 1` (le défaut est 90) :
                 K1 fin de stdin juste après le tour ;
                 K2 pont gardé 90 s après le tour (plus que le seuil), puis fin de stdin.

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/resume_probe.py --binary <yusagent> --out <dossier neuf>

`--only M1,M2,M3` limite les mesures. Rapport : `<sortie>/resume-probe.md` et `.json`.
Aucun modèle réel. macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, file: str):
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = _load("ya_capture_run", "run.py")

WORK = Path(os.environ.get("YA_RESUME_PROBE_WORK", "/private/tmp/ya-resume-probe"))
NOTICE = "[python-state-restored]"


def processes() -> dict[int, tuple[int, str]]:
    out = subprocess.run(["ps", "-axo", "pid=,ppid=,command="], capture_output=True, text=True).stdout
    table = {}
    for line in out.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
            table[int(parts[0])] = (int(parts[1]), parts[2])
    return table


def daemon_tree(socket: Path, exclude: set[int]) -> dict[str, list[int]]:
    """Processus du démon de `socket` et leurs descendants, rangés : démon, worker, kernel."""
    table = processes()
    roots = {pid for pid, (_, cmd) in table.items()
             if str(socket) in cmd and pid not in exclude and "sandbox-exec" not in cmd}
    # Un descendant d'une racine n'est pas une racine (le worker peut citer le socket).
    def ancestors(pid: int) -> set[int]:
        seen = set()
        while pid in table and pid not in seen:
            seen.add(pid)
            pid = table[pid][0]
        return seen
    roots = {pid for pid in roots if not (ancestors(table[pid][0]) & roots)}
    tree = {"daemon": sorted(roots), "worker": [], "kernel": []}
    for pid, (_, cmd) in table.items():
        if pid in roots or not (ancestors(pid) & roots):
            continue
        tree["kernel" if "python" in cmd.lower() else "worker"].append(pid)
    tree["worker"].sort()
    tree["kernel"].sort()
    return tree


def session_files(agent: Path) -> list[Path]:
    root = agent / "sessions"
    if not root.exists():
        return []
    return sorted(p for p in root.rglob("*.jsonl") if not p.name.endswith(".usage.jsonl"))


def snapshots(agent: Path) -> list[Path]:
    return sorted(agent.rglob("kernel-state*"))


class Bridge:
    """Un `yusagent --mode acp` sur un démon isolé, ses trames dans `transcript`."""

    def __init__(self, binary: Path, socket: Path, env: dict, project: Path, case: Path,
                 tag: str, extra: list[str] | None = None):
        self.transcript: list[dict] = []
        self.lines: queue.Queue[str] = queue.Queue()
        self.case = case
        self.tag = tag
        self.err = (case / f"err-{tag}.log").open("w")
        self.proc = subprocess.Popen(
            ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode", "acp",
             "--daemon-socket", str(socket), *(extra or [])],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err, env=env, cwd=project,
            text=True, bufsize=1)
        threading.Thread(target=lambda: [self.lines.put(line) for line in self.proc.stdout],
                         daemon=True).start()
        self.next_id = 1

    def write(self, message: dict) -> None:
        self.transcript.append({"dir": "client->agent", "at": time.time(), "msg": message})
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def pump(self, until, seconds: float):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(self.lines.get(timeout=0.5))
            except (queue.Empty, json.JSONDecodeError):
                continue
            self.transcript.append({"dir": "agent->client", "at": time.time(), "msg": incoming})
            if until is not None and until(incoming):
                return incoming
        return None

    def request(self, method: str, params: dict) -> dict | None:
        req_id = self.next_id
        self.next_id += 1
        self.write({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
        return self.pump(lambda m: m.get("id") == req_id and "method" not in m, run.CASE_TIMEOUT_S)

    def open(self, project: Path) -> str | None:
        self.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = self.request("session/new", {"cwd": str(project), "mcpServers": []})
        return ((new or {}).get("result") or {}).get("sessionId")

    def prompt(self, sid: str, text: str) -> dict | None:
        answer = self.request("session/prompt", {"sessionId": sid,
                                                 "prompt": [{"type": "text", "text": text}]})
        return (answer or {}).get("result") or (answer or {}).get("error")

    def close(self) -> float:
        """Fin de stdin ; renvoie l'instant (time.time()) de la fermeture."""
        at = time.time()
        self.proc.stdin.close()
        try:
            self.proc.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.err.close()
        (self.case / f"transcript-{self.tag}.jsonl").write_text(
            "".join(json.dumps(e, ensure_ascii=False) + "\n" for e in self.transcript))
        return at


def stop_daemon(socket: Path, timeout: float = 30) -> bool:
    subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {socket}"])
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        tree = daemon_tree(socket, set())
        if not any(tree.values()):
            return True
        time.sleep(0.2)
    return False


def setup(cache: Path, name: str, settings: dict | None = None):
    return setup_in(WORK, cache, name, settings)


def setup_in(work: Path, cache: Path, name: str, settings: dict | None = None):
    """Dossier agent `<work>/agent-<name>` (permissions « auto », modèle de capture) et son env."""
    agent = work / f"agent-{name}"
    run.write_models_json(agent, "anthropic-messages", 0)  # port fixé par `serve`
    run.write_settings(agent, {"defaultProvider": "capture", "defaultModel": "capture-model",
                               **(settings or {})})
    env = run.base_env(cache / "home", agent, work / "tmp")
    return agent, env


def serve(case: Path, agent: Path) -> "run.CaptureServer":
    server = run.CaptureServer(case / "cap", "basic", [])
    run.write_models_json(agent, "anthropic-messages", server.port)
    return server


def requests(cap: Path) -> list[dict]:
    return [json.loads(p.read_text()) for p in sorted(cap.glob("*.body.json"))]


def text_of(message: dict) -> str:
    content = message.get("content")
    if isinstance(content, str):
        return content
    parts = []
    for block in content or []:
        if block.get("type") == "text":
            parts.append(block.get("text", ""))
        elif block.get("type") == "tool_result":
            inner = block.get("content")
            parts.append(inner if isinstance(inner, str) else
                         " ".join(b.get("text", "") for b in inner or [] if isinstance(b, dict)))
    return "\n".join(parts)


def strip_cc(value):
    if isinstance(value, dict):
        return {k: strip_cc(v) for k, v in value.items() if k != "cache_control"}
    if isinstance(value, list):
        return [strip_cc(v) for v in value]
    return value


def m1(binary: Path, cache: Path, out: Path, project: Path) -> dict:
    case = out / "M1-fichier"
    case.mkdir()
    agent, env = setup(cache, "m1")
    server = serve(case, agent)
    socket = WORK / "m1.sock"
    bridge = Bridge(binary, socket, env, project, case, "1")
    try:
        sid = bridge.open(project)
        time.sleep(2)
        before = [str(p) for p in session_files(agent)]
        result = bridge.prompt(sid, run.PROMPT)
        after = [str(p) for p in session_files(agent)]
    finally:
        bridge.close()
        stop_daemon(socket)
        server.stop()
    return {"session_new_ok": sid is not None, "prompt": result,
            "fichiers_avant_prompt": before, "fichiers_apres_prompt": after,
            "fichier_avant_prompt": bool(before)}


def first_prompt2_index(reqs: list[dict]) -> int | None:
    for i, body in enumerate(reqs):
        if any(m.get("role") == "user" and run.PROMPT_2 in text_of(m) for m in body["messages"]):
            return i
    return None


PROMPT_3 = "Troisième message : réponds sans outil."


def resume_case(binary: Path, cache: Path, out: Path, project: Path, name: str,
                restart_daemon: bool, wait_before_prompt2: float = 0.0) -> dict:
    case = out / name
    case.mkdir()
    agent, env = setup(cache, name.lower())
    server = serve(case, agent)
    socket = WORK / f"{name.lower()}.sock"
    info: dict = {}
    try:
        b1 = Bridge(binary, socket, env, project, case, "1")
        sid = b1.open(project)
        info["prompt1"] = b1.prompt(sid, run.PROMPT)
        time.sleep(4)  # instantané du kernel : environ 1,5 s après la dernière cellule réussie
        info["instantanes_apres_prompt1"] = [str(p.relative_to(agent)) for p in snapshots(agent)]
        b1.close()
        files = session_files(agent)
        info["fichier"] = str(files[0]) if files else None
        if restart_daemon:
            info["demon_arrete"] = stop_daemon(socket)
        else:
            time.sleep(1)
            info["arbre_avant_reprise"] = daemon_tree(socket, set())
        b2 = Bridge(binary, socket, env, project, case, "2", ["--resume", info["fichier"] or ""])
        sid2 = b2.open(project)
        info["attente_avant_prompt2_s"] = wait_before_prompt2
        time.sleep(wait_before_prompt2)
        info["prompt2"] = b2.prompt(sid2, run.PROMPT_2)
        info["prompt3"] = b2.prompt(sid2, PROMPT_3)
        b2.close()
    finally:
        stop_daemon(socket)
        server.stop()
    reqs = requests(case / "cap")
    info["requetes"] = len(reqs)
    k = first_prompt2_index(reqs)
    info["premiere_requete_prompt2"] = k
    if k is None or k == 0:
        return info
    prev, cur = reqs[k - 1], reqs[k]
    info["system_identique"] = strip_cc(prev.get("system")) == strip_cc(cur.get("system"))
    info["tools_identiques"] = strip_cc(prev.get("tools")) == strip_cc(cur.get("tools"))
    pm, cm = strip_cc(prev["messages"]), strip_cc(cur["messages"])
    # Les messages de la dernière requête du prompt 1, puis sa réponse : ajout seul ?
    info["messages_prompt1_prefixe"] = cm[:len(pm)] == pm
    added = cm[len(pm):]
    info["ajoutes"] = [{"index": len(pm) + i, "role": m.get("role"),
                        "debut": text_of(m)[:160]} for i, m in enumerate(added)]
    # Première requête qui porte l'avis, et le prompt qu'il précède.
    first = next((j for j, body in enumerate(reqs)
                  if any(NOTICE in text_of(m) for m in body["messages"])), None)
    info["avis_premiere_requete"] = first
    if first is not None:
        body = reqs[first]
        hits = [i for i, m in enumerate(body["messages"]) if NOTICE in text_of(m)]
        message = body["messages"][hits[0]]
        text = text_of(message)
        following = [text_of(m) for m in body["messages"][hits[0]:]]
        info["avis_index"] = hits
        info["avis_role"] = message.get("role")
        info["avis_messages_dans_la_requete"] = len(body["messages"])
        info["avis_precede"] = ("prompt 2" if any(run.PROMPT_2 in t for t in following) and
                                not any(run.PROMPT_2 in text_of(m) for m in body["messages"][:hits[0]])
                                else "prompt 3" if any(PROMPT_3 in t for t in following) else "autre")
        info["avis_meme_message_que_le_prompt"] = run.PROMPT_2 in text or PROMPT_3 in text
        info["avis_texte"] = text[text.index(NOTICE):text.index(NOTICE) + 400]
        prev = reqs[first - 1]
        info["avis_requete_precedente_prefixe"] = (
            strip_cc(prev["messages"]) == strip_cc(body["messages"][:len(prev["messages"])])
            and strip_cc(prev.get("system")) == strip_cc(body.get("system"))
            and strip_cc(prev.get("tools")) == strip_cc(body.get("tools")))
    return info


def watch(socket: Path, exclude: set[int], start: float, seconds: float) -> list[dict]:
    """Relevé toutes les 2 s : processus du démon (worker, kernel) et instant relatif à `start`."""
    samples = []
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        tree = daemon_tree(socket, exclude)
        samples.append({"t": round(time.time() - start, 1), "worker": tree["worker"],
                        "kernel": tree["kernel"]})
        if samples[-1]["worker"] == [] and samples[-1]["kernel"] == [] and len(samples) > 1:
            break
        time.sleep(2)
    return samples


def gone_at(samples: list[dict], key: str) -> float | None:
    for sample in samples:
        if not sample[key]:
            return sample["t"]
    return None


def lifetime_case(binary: Path, cache: Path, out: Path, project: Path, name: str,
                  hold: float, limit: float) -> dict:
    case = out / name
    case.mkdir()
    agent, env = setup(cache, name.lower(), {"idleEvictionMinutes": 1})
    server = serve(case, agent)
    socket = WORK / f"{name.lower()}.sock"
    info: dict = {"idleEvictionMinutes": 1, "pont_garde_s": hold}
    try:
        bridge = Bridge(binary, socket, env, project, case, "1")
        sid = bridge.open(project)
        info["prompt"] = bridge.prompt(sid, run.PROMPT)
        turn_end = time.time()
        info["arbre_fin_du_tour"] = daemon_tree(socket, {bridge.proc.pid})
        if hold:
            info["pendant_pont_attache"] = watch(socket, {bridge.proc.pid}, turn_end, hold)
        eof = bridge.close()
        info["fin_stdin_apres_tour_s"] = round(eof - turn_end, 1)
        info["snapshot_mtime_avant"] = [p.stat().st_mtime for p in snapshots(agent)]
        samples = watch(socket, set(), turn_end, limit)
        info["releves"] = samples
        info["worker_absent_a_s_apres_tour"] = gone_at(samples, "worker")
        info["kernel_absent_a_s_apres_tour"] = gone_at(samples, "kernel")
        info["snapshot_mtime_apres"] = [p.stat().st_mtime for p in snapshots(agent)]
    finally:
        stop_daemon(socket)
        server.stop()
    return info


def report(results: dict, version: str, binary: Path) -> str:
    lines = [f"# Mesures avant la reprise ACP (ticket 28)", "",
             f"Binaire : `{binary}` ({version})", ""]
    if "M1" in results:
        r = results["M1"]
        lines += ["## M1 : fichier de session avant le premier prompt", "",
                  f"- `session/new` : {'ok' if r['session_new_ok'] else 'échec'} ; prompt : {r['prompt']}",
                  f"- fichiers avant le prompt : {r['fichiers_avant_prompt'] or 'aucun'}",
                  f"- fichiers après le prompt : {r['fichiers_apres_prompt'] or 'aucun'}",
                  f"- **fichier écrit avant le premier prompt : {'oui' if r['fichier_avant_prompt'] else 'non'}**", ""]
    for key, title in (("M2-D", "M2 : reprise avec un worker neuf (démon arrêté), prompt 2 aussitôt"),
                       ("M2-D10", "M2 : reprise avec un worker neuf, prompt 2 10 s après `session/new`"),
                       ("M2-C", "M2, contre-épreuve : reprise avec le worker vivant")):
        if key not in results:
            continue
        r = results[key]
        lines += [f"## {title}", ""]
        for field in ("prompt1", "instantanes_apres_prompt1", "fichier", "demon_arrete",
                      "arbre_avant_reprise", "attente_avant_prompt2_s", "prompt2", "prompt3", "requetes", "premiere_requete_prompt2",
                      "system_identique", "tools_identiques", "messages_prompt1_prefixe",
                      "ajoutes", "avis_premiere_requete", "avis_index", "avis_role",
                      "avis_messages_dans_la_requete", "avis_precede",
                      "avis_meme_message_que_le_prompt", "avis_requete_precedente_prefixe",
                      "avis_texte"):
            if field in r:
                lines.append(f"- `{field}` : {json.dumps(r[field], ensure_ascii=False)}")
        lines.append("")
    for key in ("M3-K1", "M3-K2"):
        if key not in results:
            continue
        r = results[key]
        lines += [f"## {key} : veille du worker (`idleEvictionMinutes: 1`)", ""]
        for field in ("prompt", "pont_garde_s", "fin_stdin_apres_tour_s", "arbre_fin_du_tour",
                      "worker_absent_a_s_apres_tour", "kernel_absent_a_s_apres_tour",
                      "snapshot_mtime_avant", "snapshot_mtime_apres"):
            if field in r:
                lines.append(f"- `{field}` : {json.dumps(r[field], ensure_ascii=False)}")
        if r.get("pendant_pont_attache"):
            last = r["pendant_pont_attache"][-1]
            lines.append(f"- pont attaché, dernier relevé : {json.dumps(last)}")
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--only", default="M1,M2,M3")
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    only = set(args.only.split(","))
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
    print(f"binaire : {version}")
    run.prepare(binary, cache, version)
    shutil.rmtree(WORK, ignore_errors=True)
    (WORK / "tmp").mkdir(parents=True)
    project = WORK / "project"
    run.make_project(project)
    results: dict = {}
    if "M1" in only:
        results["M1"] = m1(binary, cache, out, project)
        print("M1 :", json.dumps(results["M1"], ensure_ascii=False))
    if "M2" in only:
        results["M2-D"] = resume_case(binary, cache, out, project, "M2-D", restart_daemon=True)
        print("M2-D :", json.dumps(results["M2-D"], ensure_ascii=False)[:2000])
        results["M2-D10"] = resume_case(binary, cache, out, project, "M2-D10",
                                        restart_daemon=True, wait_before_prompt2=10.0)
        print("M2-D10 :", json.dumps(results["M2-D10"], ensure_ascii=False)[:2000])
        results["M2-C"] = resume_case(binary, cache, out, project, "M2-C", restart_daemon=False)
        print("M2-C :", json.dumps(results["M2-C"], ensure_ascii=False)[:2000])
    if "M3" in only:
        results["M3-K1"] = lifetime_case(binary, cache, out, project, "M3-K1", 0, 240)
        print("M3-K1 :", results["M3-K1"]["worker_absent_a_s_apres_tour"],
              results["M3-K1"]["kernel_absent_a_s_apres_tour"])
        results["M3-K2"] = lifetime_case(binary, cache, out, project, "M3-K2", 90, 240)
        print("M3-K2 :", results["M3-K2"]["worker_absent_a_s_apres_tour"],
              results["M3-K2"]["kernel_absent_a_s_apres_tour"])
    (out / "resume-probe.json").write_text(json.dumps(
        {"binary": str(binary), "version": version, "results": results}, indent=2,
        ensure_ascii=False) + "\n")
    (out / "resume-probe.md").write_text(report(results, version, binary) + "\n")
    print(f"rapport : {out / 'resume-probe.md'}")


if __name__ == "__main__":
    main()
