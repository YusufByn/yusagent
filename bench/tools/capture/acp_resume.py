#!/usr/bin/env python3
"""Cas de capture `acp-resume` (YA-10, ticket 28) : reprise ACP, préfixe, miss journalisé, trames.

Quatre sessions ACP sur le vrai binaire, chacune avec son démon isolé (`--daemon-socket`), son
dossier agent et son serveur de capture (scénario `basic`, trois cellules), sous `sandbox-exec`,
avec le même projet ; fonctions de `run.py` et de `resume_probe.py`, sans les modifier :

  E  référence : prompt 1 puis prompt 2 dans le même pont ;
  C  reprise, worker vivant : prompt 1, fin de stdin, `--resume <fichier>`, `session/load`,
     prompt 2 ;
  D  reprise, worker neuf : comme C, démon arrêté avant la reprise ;
  F  comme D, avec un skill ajouté dans le dossier agent avant la reprise.

Contrôles (verdict `CONFORME` ou `NON CONFORME`) :
  - C : chaque requête identique à celle de E, corps entier (UUID normalisés) ;
  - D : prompt système et tools identiques à E ; messages identiques une fois retiré le message
    `[python-state-restored]` (au plus un, juste avant un prompt : il arrive au premier prompt
    admis après la restauration du kernel, ticket 28, §12, M2) ;
  - F : requêtes du prompt 1 identiques à E ; après la reprise, tools identiques, prompt système
    différent seulement par les lignes du skill, messages identiques hors avis ; dans le journal
    d'usage, **une seule** entrée `invalidation` de cause `resume_prefix_change` (`parts:
    ["system"]`), sur le premier appel après la reprise ; aucune dans E, C et D ;
  - trames : `session/load` répond avec `saved` (identifiant et chemin du fichier) ; son rejeu
    arrive avant sa réponse, sous l'identifiant du fichier, entre `turnStart` (`prompt`) et
    `turnEnd` (`done`), avec le prompt ; ses mises à jour (texte, cellules et leurs résultats)
    sont celles du prompt 1 en direct (C et D).

Un binaire sans `loadSession` (avant le ticket 28) : E seulement, verdict `SANS OBJET`.

Usage seul (dossier neuf) :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/acp_resume.py --binary <yusagent> --out <dossier neuf>
Rapport : `<sortie>/acp-resume/resume-report.md` et `.json`. Code de sortie 1 si NON CONFORME.
Lancé aussi par `run.py` (le run échoue si NON CONFORME). macOS (sandbox-exec) requis ;
bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
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
probe = _load("ya_resume_probe", "resume_probe.py")
compare = _load("ya_capture_compare", "compare.py")

META = "ai.primeintellect.prime-agent"
NOTICE = probe.NOTICE
SKILL = "ya-resume-skill"
CORRELATION = ("promptTurnId", "eventSequence", "phase", "outcome")
CONTENT = ("agent_message_chunk", "agent_thought_chunk", "tool_call", "tool_call_update")


def write_skill(agent: Path) -> None:
    skill = agent / "skills" / SKILL
    skill.mkdir(parents=True, exist_ok=True)
    (skill / "SKILL.md").write_text(
        f"---\nname: {SKILL}\ndescription: Skill ajouté entre deux reprises (capture, ticket 28).\n"
        "---\n\nRien à faire.\n")


def saved_of(response: dict | None) -> dict:
    return (((response or {}).get("result") or {}).get("_meta") or {}).get(META, {}).get("saved") or {}


def sub_case(binary: Path, cache: Path, work: Path, project: Path, root: Path, name: str) -> dict:
    case = root / name
    case.mkdir(parents=True)
    agent, env = probe.setup_in(work, cache, f"resume-{name}")
    server = probe.serve(case, agent)
    socket = work / f"resume-{name}.sock"
    before = run.journal_sizes(agent)
    info: dict = {}
    try:
        b1 = probe.Bridge(binary, socket, env, project, case, "1")
        init = b1.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        caps = ((init or {}).get("result") or {}).get("agentCapabilities") or {}
        info["loadSession"] = bool(caps.get("loadSession"))
        new = b1.request("session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        info["saved"] = saved_of(new)
        info["prompt1"] = b1.prompt(sid, run.PROMPT)
        info["prompt1_frames"] = [len(b1.transcript)]
        if name == "E" or not info["loadSession"]:
            if name == "E":
                info["prompt2"] = b1.prompt(sid, run.PROMPT_2)
            b1.close()
            return info
        time.sleep(4)  # instantané du kernel (environ 1,5 s après la dernière cellule réussie)
        b1.close()
        if name in ("D", "F"):
            info["demon_arrete"] = probe.stop_daemon(socket)
        if name == "F":
            write_skill(agent)
        path = info["saved"].get("path") or ""
        b2 = probe.Bridge(binary, socket, env, project, case, "2", ["--resume", path])
        b2.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        load_at = len(b2.transcript)
        loaded = b2.request("session/load", {"sessionId": info["saved"].get("sessionId"),
                                             "cwd": str(project), "mcpServers": []})
        info["load"] = (loaded or {}).get("result") is not None
        info["load_error"] = (loaded or {}).get("error")
        info["load_saved"] = saved_of(loaded)
        info["load_frames"] = [load_at, len(b2.transcript)]
        info["prompt2"] = b2.prompt(info["saved"].get("sessionId"), run.PROMPT_2)
        b2.close()
    finally:
        probe.stop_daemon(socket)
        server.stop()
        info["usage_journals"] = run.copy_journals(agent, before, case)
    return info


def bodies(case: Path) -> list[dict]:
    return [json.loads(text) for text in compare.all_requests(case)]


def message_text(message: dict) -> str:
    return probe.text_of(message)


def without_notice(body: dict) -> tuple[dict, list[int]]:
    """The body without its `[python-state-restored]` messages, and where they were."""
    messages = body.get("messages") or []
    hits = [i for i, m in enumerate(messages) if NOTICE in message_text(m)]
    kept = [m for i, m in enumerate(messages) if i not in hits]
    return {**body, "messages": kept}, hits


def notice_well_placed(body: dict, hits: list[int]) -> bool:
    """At most one notice, a user message followed by a user message (the prompt)."""
    if not hits:
        return True
    messages = body["messages"]
    at = hits[0]
    return (len(hits) == 1 and messages[at].get("role") == "user" and at + 1 < len(messages)
            and messages[at + 1].get("role") == "user")


def system_lines(body: dict) -> list[str]:
    system = body.get("system")
    if isinstance(system, list):
        system = "\n".join(block.get("text", "") for block in system if isinstance(block, dict))
    return (system or "").splitlines()


def strip(value):
    return probe.strip_cc(value)


def journal(case: Path) -> list[dict]:
    lines = []
    for path in sorted((case / "usage").glob("*.usage.ndjson")):
        lines += [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return sorted(lines, key=lambda entry: (entry["startMs"], entry["seq"]))


def resume_causes(entries: list[dict]) -> list[tuple[int, dict]]:
    found = []
    for index, entry in enumerate(entries):
        invalidation = entry.get("invalidation")
        for item in invalidation if isinstance(invalidation, list) else [invalidation]:
            if isinstance(item, dict) and item.get("cause") == "resume_prefix_change":
                found.append((index, item))
    return found


def transcript(case: Path, tag: str) -> list[dict]:
    path = case / f"transcript-{tag}.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines()] if path.is_file() else []


def content_view(frames: list[dict]) -> list[dict]:
    """Content updates of a turn, chunks of one message merged, correlation fields dropped."""
    view: list[dict] = []
    for frame in frames:
        update = (frame.get("params") or {}).get("update") or {}
        kind = update.get("sessionUpdate")
        if kind not in CONTENT:
            continue
        if kind == "tool_call_update" and update.get("status") not in ("completed", "failed"):
            continue
        item = {key: value for key, value in update.items() if key not in ("messageId", "_meta")}
        ours = {key: value for key, value in ((update.get("_meta") or {}).get(META) or {}).items()
                if key not in CORRELATION}
        if ours:
            item["_meta"] = ours
        if kind in ("agent_message_chunk", "agent_thought_chunk"):
            text = (update.get("content") or {}).get("text", "")
            last = view[-1] if view else None
            if last and last.get("_message") == (kind, update.get("messageId")):
                last["content"]["text"] += text
                continue
            item = {"sessionUpdate": kind, "content": {"type": "text", "text": text},
                    "_message": (kind, update.get("messageId"))}
        view.append(item)
    for item in view:
        item.pop("_message", None)
    return view


def frames_between(entries: list[dict], start: int, end: int) -> list[dict]:
    return [e["msg"] for e in entries[start:end]
            if e["dir"] == "agent->client" and e["msg"].get("method") == "session/update"]


def check_frames(case: Path, info: dict) -> dict:
    first, second = transcript(case, "1"), transcript(case, "2")
    prompt_end = info["prompt1_frames"][0]
    prompt_at = next((i for i, e in enumerate(first)
                      if e["dir"] == "client->agent" and e["msg"].get("method") == "session/prompt"), 0)
    live = content_view(frames_between(first, prompt_at, prompt_end))
    load_at, load_end = info["load_frames"]
    window = second[load_at:load_end]
    replay_frames = [e["msg"] for e in window
                     if e["dir"] == "agent->client" and e["msg"].get("method") == "session/update"]
    response_index = next((i for i, e in enumerate(window)
                           if e["dir"] == "agent->client" and "method" not in e["msg"]
                           and e["msg"].get("result") is not None), None)
    replay_before = response_index is not None and all(
        i < response_index for i, e in enumerate(window)
        if e["dir"] == "agent->client" and e["msg"].get("method") == "session/update")
    sid = info["saved"].get("sessionId")
    metas = [((f["params"]["update"].get("_meta") or {}).get(META) or {}) for f in replay_frames]
    starts = [m["turnStart"] for m in metas if "turnStart" in m]
    ends = [m["turnEnd"] for m in metas if "turnEnd" in m]
    users = [f["params"]["update"]["content"] for f in replay_frames
             if f["params"]["update"].get("sessionUpdate") == "user_message_chunk"]
    replay = content_view(replay_frames)
    result = {
        "load_saved_identique": info.get("load_saved") == info["saved"],
        "rejeu_avant_reponse": replay_before,
        "rejeu_sous_id_fichier": bool(replay_frames) and all(
            f["params"]["sessionId"] == sid for f in replay_frames),
        "turnStart": starts,
        "turnEnd": ends,
        "prompt_rejoue": users,
        "mises_a_jour_direct": len(live),
        "mises_a_jour_rejeu": len(replay),
        "rejeu_egal_direct": replay == live,
    }
    if replay != live:
        result["direct"] = live
        result["rejeu"] = replay
    result["ok"] = (result["load_saved_identique"] and replay_before
                    and result["rejeu_sous_id_fichier"]
                    and starts and starts[0].get("origin") == "prompt"
                    and ends == [{"state": "done"}]
                    and users == [{"type": "text", "text": run.PROMPT}]
                    and result["rejeu_egal_direct"])
    return result


def check(root: Path, infos: dict) -> dict:
    report: dict = {"cas": infos}
    if not infos["E"].get("loadSession"):
        report["verdict"] = "SANS OBJET"
        report["raison"] = "le binaire n'annonce pas loadSession"
        return report
    e = bodies(root / "E")
    checks: dict = {}
    for name in ("C", "D", "F"):
        info = infos[name]
        checks[name] = {"load": info.get("load"), "load_error": info.get("load_error"),
                        "prompt2": info.get("prompt2")}
    # C : corps entier.
    c = bodies(root / "C")
    checks["C"]["requetes"] = [len(e), len(c)]
    checks["C"]["identiques"] = len(c) == len(e) and all(a == b for a, b in zip(e, c))
    # D : préfixe identique, messages identiques hors avis.
    d = bodies(root / "D")
    notices, same = [], len(d) == len(e)
    for a, b in zip(e, d):
        stripped, hits = without_notice(b)
        notices.append(hits)
        same = same and stripped == a and notice_well_placed(b, hits)
    checks["D"]["requetes"] = [len(e), len(d)]
    checks["D"]["avis_par_requete"] = notices
    checks["D"]["identiques_hors_avis"] = same
    # F : prompt 1 identique, puis prompt système changé par le skill seulement.
    f = bodies(root / "F")
    first_resumed = probe.first_prompt2_index([json.loads(json.dumps(b)) for b in f])
    checks["F"]["requetes"] = [len(e), len(f)]
    checks["F"]["premiere_requete_reprise"] = first_resumed
    f_ok = len(f) == len(e) and first_resumed is not None
    system_changes: list[str] = []
    if f_ok:
        for index, (a, b) in enumerate(zip(e, f)):
            stripped, hits = without_notice(b)
            if index < first_resumed:
                f_ok = f_ok and a == b
                continue
            f_ok = (f_ok and strip(a.get("tools")) == strip(b.get("tools"))
                    and {**stripped, "system": None} == {**a, "system": None}
                    and notice_well_placed(b, hits))
            added = sorted(set(system_lines(b)) - set(system_lines(a)))
            removed = sorted(set(system_lines(a)) - set(system_lines(b)))
            system_changes = added + removed
            f_ok = f_ok and bool(added) and all(SKILL in line for line in added + removed)
    checks["F"]["lignes_systeme_changees"] = system_changes
    checks["F"]["identiques_hors_skill_et_avis"] = f_ok
    # Journaux : une seule invalidation de reprise, dans F.
    causes = {name: resume_causes(journal(root / name)) for name in ("E", "C", "D", "F")}
    f_journal = journal(root / "F")
    mains = [i for i, entry in enumerate(f_journal) if entry.get("kind") == "main"]
    # Le 1er appel principal après la reprise : celui de la 1re requête du prompt 2.
    resumed_index = None
    if first_resumed is not None and len(mains) > first_resumed:
        resumed_index = mains[first_resumed]
    checks["journal"] = {
        "invalidations_reprise": {name: [item for _, item in found] for name, found in causes.items()},
        "F_ligne": [index for index, _ in causes["F"]],
        "F_premier_appel_apres_reprise": resumed_index,
    }
    journal_ok = (not causes["E"] and not causes["C"] and not causes["D"]
                  and len(causes["F"]) == 1
                  and causes["F"][0][1].get("parts") == ["system"]
                  and causes["F"][0][0] == resumed_index)
    checks["journal"]["ok"] = journal_ok
    # Trames du rejeu (C et D).
    checks["trames"] = {name: check_frames(root / name, infos[name]) for name in ("C", "D")}
    ends = all(infos[name].get("prompt2") == {"stopReason": "end_turn"} for name in infos)
    ok = (ends and all(checks[name]["load"] for name in ("C", "D", "F"))
          and checks["C"]["identiques"] and checks["D"]["identiques_hors_avis"]
          and f_ok and journal_ok and all(t["ok"] for t in checks["trames"].values()))
    report["controles"] = checks
    report["verdict"] = "CONFORME" if ok else "NON CONFORME"
    return report


def render(report: dict) -> str:
    lines = ["# Cas `acp-resume` (ticket 28)", ""]
    if report["verdict"] == "SANS OBJET":
        return "\n".join(lines + [f"- {report['raison']}", "", "SANS OBJET"]) + "\n"
    checks = report["controles"]
    lines += [
        "| Contrôle | Résultat |", "|---|---|",
        f"| C : requêtes identiques à E (corps entier) | {checks['C']['identiques']} "
        f"({checks['C']['requetes'][1]}/{checks['C']['requetes'][0]}) |",
        f"| D : identiques à E hors avis `[python-state-restored]` | "
        f"{checks['D']['identiques_hors_avis']} (avis par requête : {checks['D']['avis_par_requete']}) |",
        f"| F : prompt 1 identique, puis seul le skill change le prompt système | "
        f"{checks['F']['identiques_hors_skill_et_avis']} |",
        f"| Journal : une seule `resume_prefix_change`, dans F, au 1er appel repris | "
        f"{checks['journal']['ok']} ({checks['journal']['invalidations_reprise']}) |",
    ]
    for name, frames in checks["trames"].items():
        lines.append(f"| Trames {name} : rejeu avant la réponse, = direct ({frames['mises_a_jour_rejeu']}"
                     f"/{frames['mises_a_jour_direct']}), début et fin de tour | {frames['ok']} |")
    lines += ["", f"Lignes du prompt système changées dans F : {checks['F']['lignes_systeme_changees']}",
              "", report["verdict"]]
    return "\n".join(lines) + "\n"


def resume_cases(binary: Path, cache: Path, work: Path, project: Path, out: Path) -> dict:
    """Les quatre sessions et le rapport ; renvoie le rapport (verdict compris)."""
    root = out / "acp-resume"
    root.mkdir(parents=True)
    infos = {"E": sub_case(binary, cache, work, project, root, "E")}
    for name in ("C", "D", "F"):
        if infos["E"].get("loadSession"):
            infos[name] = sub_case(binary, cache, work, project, root, name)
    report = check(root, infos)
    (root / "resume-report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    (root / "resume-report.md").write_text(render(report))
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--cache", type=Path,
                        default=Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
                        / "yusagent-capture")
    args = parser.parse_args()
    binary, out, cache = args.binary.resolve(), args.out.resolve(), args.cache.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f"--out {out} existe déjà et n'est pas vide : le supprimer ou choisir un autre dossier")
    out.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True,
                             env={"PATH": "/usr/bin:/bin", "HOME": str(cache / "home")}).stdout.strip()
    print(f"binaire : {version}")
    run.prepare(binary, cache, version)
    work = Path(os.environ.get("YA_ACP_RESUME_WORK", "/private/tmp/ya-acp-resume"))
    shutil.rmtree(work, ignore_errors=True)
    (work / "tmp").mkdir(parents=True)
    project = work / "project"
    run.make_project(project)
    report = resume_cases(binary, cache, work, project, out)
    print(render(report), end="")
    sys.exit(1 if report["verdict"] == "NON CONFORME" else 0)


if __name__ == "__main__":
    main()
