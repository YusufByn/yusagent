#!/usr/bin/env python3
"""Tours de réveil vus par le pont, et Stop pendant un réveil (ticket 25, E6, choix T4).

YA-10 (ticket 25). Mesure au niveau du démon, sur un binaire tel quel (aucune ligne du pont) :
le pont ACP (`--mode acp`) mène la session ; un second client du socket du démon (`attach`, comme
un TUI attaché) relève les événements du worker et envoie la commande d'arrêt. Réutilise
`run.py` (serveur de capture, `sandbox-exec`, démon isolé, HOME et venv du kernel du banc) et
`analyze.py` (comparaison de deux requêtes), sans les modifier.

Six cas, `<commande>-<moment>`, chacun dans `<sortie>/<cas>/` :
  commandes : `abort_and_clear_queue` (Stop d'un prompt dans le pont), `abort`,
              `abort_and_send_queued` (Stop du TUI) ;
  moments   : `avant` (scénario `wake-stop-avant` : le second `bash()` finit pendant le premier
              réveil, la commande part quand son avis est dans la file) ; `apres` (scénario
              `wake-stop-apres` : la commande part 2 s après le début du premier réveil, file
              vide ; le second `bash()` finit environ 20 s après son lancement).
Déroulé : prompt (deux `bash()` non lus) ; premier réveil (`[bash-done]`, cellule de 15 s) ;
commande d'arrêt ; 25 s d'observation ; prompt « suite » ; 8 s d'observation.

Relevés par cas (`wake-turns.json`, plus `events.jsonl`, `transcript.jsonl`, `cap/`) :
  (a) les `agent_start`/`agent_end` du worker reçus par le client du démon, dans l'ordre, avec
      les autres types d'événements entre eux, et l'identifiant de tour (`promptTurnId`) des
      trames ACP du pont ;
  (b) la file (`get_queue`) avant et après la commande et à la fin de l'observation, les tours
      démarrés après la commande, le nombre d'avis `[bash-done]` vus par le provider ;
      pour chaque requête envoyée au provider après la commande, comparée à la précédente :
      prompt système et tools identiques, historique en ajout seul (`analyze.compare_pair`).

Usage :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/wake_turns.py --binary <yusagent> --out <dossier neuf>

Code de sortie 0 si chaque cas a mené ses deux prompts à `end_turn` sans processus restant et
si chaque requête après la commande garde le préfixe ; 1 sinon (le détail est dans le rapport).
macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import queue
import shutil
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


run = _load("ya_capture_run", "run.py")
analyze = _load("ya_capture_analyze", "analyze.py")

WORK = Path(os.environ.get("YA_WAKE_TURNS_WORK", "/private/tmp/ya-wake-turns"))
COMMANDS = ("abort_and_clear_queue", "abort", "abort_and_send_queued")
MOMENTS = ("avant", "apres")
OBSERVE_S = 25.0
LINGER_S = 8.0
FOLLOW_UP = "suite"
BASH_DONE = "[bash-done"
TURN_EVENTS = ("agent_start", "agent_end")


def now_utc() -> datetime:
    return datetime.now(timezone.utc)


def binary_pids(binary: Path) -> set[str]:
    found = subprocess.run(["pgrep", "-f", str(binary)], capture_output=True, text=True)
    return set(found.stdout.split())


def find_session_ids(value) -> list[str]:
    """Les `activeSessionId` d'une réponse `list`, dans l'ordre."""
    found: list[str] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key == "activeSessionId" and isinstance(item, str) and item not in found:
                found.append(item)
            else:
                found.extend(i for i in find_session_ids(item) if i not in found)
    elif isinstance(value, list):
        for item in value:
            found.extend(i for i in find_session_ids(item) if i not in found)
    return found


class DaemonClient:
    """Client JSONL du socket du démon : enveloppes de commande, réponses par id, événements
    de session relevés avec leur heure d'arrivée."""

    def __init__(self, path: Path, t0: float):
        self.t0 = t0
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.connect(str(path))
        self.reader = self.sock.makefile("r", encoding="utf-8")
        self.hello = json.loads(self.reader.readline())
        if self.hello.get("type") != "daemon_hello":
            raise RuntimeError(f"pas de daemon_hello : {self.hello}")
        self.protocol = self.hello.get("protocol") or {}
        self.responses: dict[str, dict] = {}
        self.events: list[dict] = []
        self.cond = threading.Condition()
        self.next_id = 0
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self) -> None:
        for line in self.reader:
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            with self.cond:
                if frame.get("type") == "response":
                    self.responses[str(frame.get("id"))] = frame
                elif frame.get("type") == "session_event":
                    self.events.append({"t": round(time.monotonic() - self.t0, 3), "frame": frame})
                self.cond.notify_all()

    def request(self, command: dict, timeout: float = 30.0) -> dict:
        with self.cond:
            self.next_id += 1
            request_id = f"wake-turns-{self.next_id}"
        envelope = {"type": "command", "id": request_id, "protocol": self.protocol,
                    "clientId": "wake-turns", "command": command}
        self.sock.sendall((json.dumps(envelope) + "\n").encode())
        deadline = time.monotonic() + timeout
        with self.cond:
            while request_id not in self.responses:
                left = deadline - time.monotonic()
                if left <= 0:
                    raise TimeoutError(f"pas de réponse à {command.get('type')}")
                self.cond.wait(left)
            return self.responses.pop(request_id)

    def event_types(self, since: int = 0) -> list[tuple[float, str]]:
        with self.cond:
            return [(e["t"], (e["frame"].get("event") or {}).get("type", "?"))
                    for e in self.events[since:]]

    def wait_event(self, kind: str, since: int, timeout: float) -> int | None:
        """Indice du premier événement `kind` à partir de `since`, ou None."""
        deadline = time.monotonic() + timeout
        with self.cond:
            while True:
                for index in range(since, len(self.events)):
                    if (self.events[index]["frame"].get("event") or {}).get("type") == kind:
                        return index
                left = deadline - time.monotonic()
                if left <= 0:
                    return None
                self.cond.wait(left)

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass


def bash_done_count(body: dict) -> int:
    return sum(text.count(BASH_DONE) for message in body.get("messages") or []
               if message.get("role") == "user" for text in analyze_strings(message.get("content")))


def analyze_strings(value) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, dict):
        return [s for item in value.values() for s in analyze_strings(item)]
    if isinstance(value, list):
        return [s for item in value for s in analyze_strings(item)]
    return []


def request_rows(cap: Path, stop_at: datetime | None) -> list[dict]:
    """Chaque requête reçue par le provider, et, après la commande, la comparaison avec la
    précédente (prompt système, tools, historique en ajout seul)."""
    requests = analyze.load_case(cap.parent)
    rows = []
    for index, req in enumerate(requests):
        at = datetime.fromisoformat(req.meta["ts"])
        texts = analyze_strings(req.body.get("messages") or [])
        last = (req.body.get("messages") or [{}])[-1]
        row = {
            "seq": req.seq,
            "ts": req.meta["ts"],
            "apres_commande": stop_at is not None and at >= stop_at,
            "tools": req.has_tools,
            "messages": len(req.conversation()),
            "bash_done": bash_done_count(req.body),
            "suite": any(text.strip() == FOLLOW_UP for text in texts),
            "dernier_role": last.get("role"),
            "reponse": (req.meta.get("response") or {}).get("kind"),
            "code": ((req.meta.get("response") or {}).get("code") or "").splitlines()[:1],
        }
        if row["apres_commande"] and index > 0:
            pair = analyze.compare_pair(requests[index - 1], req)
            row["compare"] = {k: pair[k] for k in (
                "pair", "system_identique", "tools_identiques", "params_identiques",
                "messages_n", "messages_n1", "prefixe_exact", "prefixe_exact_hors_cache_control")}
            row["compare"]["diffs_messages"] = pair["diffs_messages"]
            if not pair["params_identiques"]:
                row["compare"]["params_diff"] = pair.get("params_diff")
        rows.append(row)
    return rows


def turns_summary(events: list[dict]) -> list[dict]:
    """Les tours du worker (agent_start … agent_end), avec les types d'événements reçus entre
    les deux."""
    turns: list[dict] = []
    current: dict | None = None
    for entry in events:
        t = entry["t"]
        kind = (entry["frame"].get("event") or {}).get("type", "?")
        if kind == "agent_start":
            current = {"start": t, "end": None, "types": {}}
            turns.append(current)
        elif kind == "agent_end":
            if current is None:
                turns.append({"start": None, "end": t, "types": {}})
            else:
                current["end"] = t
                current = None
        elif current is not None:
            current["types"][kind] = current["types"].get(kind, 0) + 1
    return turns


def queue_rows(response: dict) -> dict:
    data = response.get("data") or {}
    return {"ok": response.get("success"), "steering": data.get("steering"),
            "followUp": data.get("followUp"), "error": response.get("error")}


def case(binary: Path, cache: Path, out: Path, project: Path, command: str, moment: str) -> dict:
    name = f"{command}-{moment}"
    case_dir = out / name
    case_dir.mkdir()
    agent = WORK / f"agent-{name}"
    sock = WORK / f"{name}.sock"
    before = binary_pids(binary)
    server = run.CaptureServer(case_dir / "cap", f"wake-stop-{moment}", [])
    run.write_models_json(agent, "anthropic-messages", server.port)
    (agent / "settings.json").write_text(json.dumps({
        **run.KERNEL_SETTINGS, **run.CACHE_SETTINGS,
        "defaultProvider": "capture", "defaultModel": "capture-model",
        "permissions": "auto"}) + "\n")
    env = run.base_env(cache / "home", agent, WORK / "tmp")
    lines: queue.Queue[str] = queue.Queue()
    transcript: list[dict] = []
    t0 = time.monotonic()
    err = (case_dir / "err.log").open("w")
    proc = subprocess.Popen(
        ["/usr/bin/sandbox-exec", "-f", str(run.SANDBOX), str(binary), "--mode", "acp",
         "--daemon-socket", str(sock)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=env, cwd=project,
        text=True, bufsize=1)
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()

    def stamp() -> float:
        return round(time.monotonic() - t0, 3)

    def write(message: dict) -> None:
        transcript.append({"t": stamp(), "dir": "client->agent", "msg": message})
        proc.stdin.write(json.dumps(message) + "\n")
        proc.stdin.flush()

    def pump(until, seconds: float):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                incoming = json.loads(lines.get(timeout=0.2))
            except (queue.Empty, json.JSONDecodeError):
                continue
            transcript.append({"t": stamp(), "dir": "agent->client", "msg": incoming})
            if until is not None and until(incoming):
                return incoming
        return None

    def request(req_id: int, method: str, params: dict, seconds: float = run.CASE_TIMEOUT_S):
        write({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params})
        return pump(lambda m: m.get("id") == req_id and "method" not in m, seconds)

    def pump_for(seconds: float) -> None:
        pump(None, seconds)

    result: dict = {"case": name, "command": command, "moment": moment}
    client: DaemonClient | None = None
    stop_at: datetime | None = None
    try:
        request(1, "initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = request(2, "session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        client = DaemonClient(sock, t0)
        listed = client.request({"type": "list"})
        ids = find_session_ids(listed.get("data"))
        result["daemon_sessions"] = len(ids)
        if len(ids) != 1:
            raise RuntimeError(f"sessions du démon : {ids}")
        active = ids[0]
        attached = client.request({"type": "attach", "activeSessionId": active})
        result["attach"] = attached.get("success")
        prompt = request(3, "session/prompt", {
            "sessionId": sid, "prompt": [{"type": "text", "text": run.PROMPT}]})
        result["prompt"] = (prompt or {}).get("result") or (prompt or {}).get("error")
        t_prompt_end = stamp()
        result["t_prompt_end"] = t_prompt_end
        mark = len(client.events)
        # Premier réveil : agent_start après la réponse du prompt (le pont pompe pendant l'attente).
        wake_index = None
        deadline = time.monotonic() + 30
        while wake_index is None and time.monotonic() < deadline:
            pump_for(0.3)
            wake_index = client.wait_event("agent_start", mark, 0.01)
        if wake_index is None:
            raise RuntimeError("aucun agent_start de réveil dans les 30 s après le prompt")
        result["t_wake1_start"] = client.events[wake_index]["t"]
        if moment == "avant":
            deadline = time.monotonic() + 20
            waited = None
            while time.monotonic() < deadline:
                pump_for(0.2)
                waited = queue_rows(client.request({"type": "get_queue", "activeSessionId": active}))
                if any(BASH_DONE in item for item in waited.get("steering") or []):
                    break
            else:
                raise RuntimeError(f"second avis jamais en file : {waited}")
        else:
            pump_for(2.0)
        result["queue_before"] = queue_rows(
            client.request({"type": "get_queue", "activeSessionId": active}))
        state = client.request({"type": "get_state", "activeSessionId": active})
        result["streaming_before"] = (state.get("data") or {}).get("isStreaming")
        stop_at = now_utc()
        result["t_command"] = stamp()
        response = client.request({"type": command, "activeSessionId": active})
        result["command_response"] = {"success": response.get("success"),
                                      "data": response.get("data"),
                                      "error": response.get("error")}
        result["queue_after"] = queue_rows(
            client.request({"type": "get_queue", "activeSessionId": active}))
        pump_for(OBSERVE_S)
        result["t_follow_up"] = stamp()
        result["queue_end"] = queue_rows(
            client.request({"type": "get_queue", "activeSessionId": active}))
        follow = request(4, "session/prompt", {
            "sessionId": sid, "prompt": [{"type": "text", "text": FOLLOW_UP}]}, 90)
        result["follow_up"] = (follow or {}).get("result") or (follow or {}).get("error")
        pump_for(LINGER_S)
        result["t_end"] = stamp()
    except Exception as error:  # noqa: BLE001 — le cas est rapporté, les suivants tournent.
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        if client is not None:
            client.close()
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        err.close()
        subprocess.run(["pkill", "-TERM", "-f", "--", f"--daemon-socket {sock}"])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and binary_pids(binary) - before:
            time.sleep(0.2)
        server.stop()
    events = client.events if client is not None else []
    (case_dir / "transcript.jsonl").write_text(
        "".join(json.dumps(entry, ensure_ascii=False) + "\n" for entry in transcript))
    (case_dir / "events.jsonl").write_text(
        "".join(json.dumps(entry, ensure_ascii=False) + "\n" for entry in events))
    result["leftovers"] = sorted(binary_pids(binary) - before)
    t_command = result.get("t_command")
    t_follow = result.get("t_follow_up")
    turns = turns_summary(events)
    for turn in turns:
        start = turn["start"] if turn["start"] is not None else turn["end"]
        turn["debut"] = ("après « suite »" if t_follow is not None and start >= t_follow
                         else "après la commande" if t_command is not None and start >= t_command
                         else "avant la commande")
    result["turns"] = turns
    result["turns_started_after_command"] = sum(
        1 for turn in turns if turn["debut"] == "après la commande")
    # Les identifiants de tour des trames ACP du pont (session/update), dans l'ordre.
    acp_turns = []
    for entry in transcript:
        msg = entry["msg"]
        if entry["dir"] != "agent->client" or msg.get("method") != "session/update":
            continue
        update = (msg.get("params") or {}).get("update") or {}
        meta = (update.get("_meta") or {}).get("ai.primeintellect.prime-agent") or {}
        acp_turns.append([entry["t"], update.get("sessionUpdate"), meta.get("promptTurnId"),
                          meta.get("phase")])
    result["acp_updates"] = acp_turns
    result["requests"] = request_rows(case_dir / "cap", stop_at)
    after = [row for row in result["requests"] if row.get("compare")]
    result["prefix_after_command"] = {
        "requetes": len(after),
        "toutes_gardent_le_prefixe": all(
            row["compare"]["system_identique"] and row["compare"]["tools_identiques"]
            and row["compare"]["prefixe_exact_hors_cache_control"] for row in after),
    }
    (case_dir / "case.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(f"{name} : prompt={result.get('prompt')} suite={result.get('follow_up')} "
          f"erreur={result.get('error')} restants={result['leftovers']}")
    return result


def render(results: list[dict]) -> str:
    lines = []
    for r in results:
        lines.append(f"## {r['case']}")
        if r.get("error"):
            lines.append(f"ERREUR : {r['error']}")
        lines.append(f"prompt={r.get('prompt')} suite={r.get('follow_up')} "
                     f"restants={r.get('leftovers')}")
        lines.append(f"t : fin du prompt {r.get('t_prompt_end')}, début réveil 1 "
                     f"{r.get('t_wake1_start')}, commande {r.get('t_command')}, "
                     f"prompt « suite » {r.get('t_follow_up')}")
        lines.append(f"file avant : {r.get('queue_before')} ; occupé : {r.get('streaming_before')}")
        lines.append(f"réponse : {r.get('command_response')}")
        lines.append(f"file après : {r.get('queue_after')}")
        lines.append(f"file à la fin de l'observation : {r.get('queue_end')}")
        lines.append("tours du worker (agent_start → agent_end, début, événements entre) :")
        for turn in r.get("turns") or []:
            lines.append(f"  [{turn['start']} → {turn['end']}] {turn['debut']} {turn['types']}")
        lines.append(f"tours démarrés entre la commande et « suite » : "
                     f"{r.get('turns_started_after_command')}")
        ids: dict = {}
        for _, _, turn_id, phase in r.get("acp_updates") or []:
            ids.setdefault(str(turn_id), {}).setdefault(str(phase), 0)
            ids[str(turn_id)][str(phase)] += 1
        lines.append(f"trames ACP du pont par promptTurnId et phase : {ids}")
        lines.append("requêtes (seq, après commande, avis, « suite », dernier rôle, réponse, "
                     "système =, tools =, ajout seul) :")
        for row in r.get("requests") or []:
            comp = row.get("compare") or {}
            lines.append(
                f"  {row['seq']:04d} {'après' if row['apres_commande'] else 'avant'} "
                f"avis={row['bash_done']} suite={row['suite']} dernier={row['dernier_role']} "
                f"{row['reponse']} {row['code']} "
                + (f"sys={comp.get('system_identique')} tools={comp.get('tools_identiques')} "
                   f"ajout={comp.get('prefixe_exact_hors_cache_control')} "
                   f"exact={comp.get('prefixe_exact')} diffs={comp.get('diffs_messages')}"
                   if comp else ""))
        lines.append(f"préfixe après la commande : {r.get('prefix_after_command')}")
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--only", nargs="*", default=None,
                        help="cas à lancer (`<commande>-<moment>`), tous par défaut")
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer (rm -rf {out}) "
                 "ou choisir un autre dossier")
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
    results = []
    for command in COMMANDS:
        for moment in MOMENTS:
            if args.only and f"{command}-{moment}" not in args.only:
                continue
            results.append(case(binary, cache, out, project, command, moment))
    (out / "wake-turns.json").write_text(json.dumps(
        {"binary": str(binary), "version": version, "cases": results},
        ensure_ascii=False, indent=2) + "\n")
    report = render(results)
    (out / "wake-turns.md").write_text(report + "\n")
    print(report)
    ok = all(r.get("prompt") == {"stopReason": "end_turn"}
             and r.get("follow_up") == {"stopReason": "end_turn"}
             and not r.get("error") and not r.get("leftovers")
             and r["prefix_after_command"]["toutes_gardent_le_prefixe"] for r in results)
    print("CONFORME" if ok else "NON CONFORME")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
