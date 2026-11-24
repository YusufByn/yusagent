#!/usr/bin/env python3
"""Cas de capture `acp-live` (YA-10, ticket 31) : skills, MCP et réglages changés en cours de session.

Six sessions ACP sur le vrai binaire, chacune avec son démon isolé (`--daemon-socket`), son dossier
agent et son serveur de capture (scénario `basic` : trois cellules au prompt 1, puis des réponses
texte), sous `sandbox-exec`, avec le même projet ; fonctions de `run.py`, `resume_probe.py` et
`compare.py`, sans les modifier (`docs/tickets/31-skills-mcp-reglages.md`, §3.3) :

  L1 skill créé par `_yus/skills/create` entre le prompt 1 et le prompt 2 ;
  L2 serveur MCP ajouté par `_yus/settings/set mcpServers` entre les deux prompts ;
  L3 skill existant éteint puis rallumé par `_yus/skills/set` avant le prompt 2 ;
  L4 comme L1, puis fin de stdin, `--resume <fichier>`, `session/load` sur le worker vivant,
     prompt 3 ;
  L5 skill écrit sur disque entre les deux prompts, puis fin de stdin, démon arrêté, reprise,
     prompt 3 et prompt 4 ;
  L6 `_yus/settings/set permissions`.

Contrôles (verdict `CONFORME` ou `NON CONFORME`) :
  - L1, L2 : l'appel réussit ; la première requête du prompt 2 garde le prompt système, les tools
    et les paramètres de la dernière du prompt 1, ses messages la prolongent, et le seul ajout de
    l'utilisateur avant le prompt 2 est l'annonce (`[skills-changed]`, `[mcp-changed]`) ; une
    seule notice `_yus/notice` dans les trames ; aucune `invalidation` au journal d'usage ;
  - L3 : les deux appels réussissent ; aucune annonce, aucune notice, requête prolongée ;
  - L4 : L1, puis la requête du prompt 3 prolonge celle du prompt 2 (préfixe et annonce
    identiques) ; aucune `invalidation` ;
  - L5 : **une seule** `invalidation`, de cause `resume_prefix_change` et `parts: ["system"]`, au
    premier appel repris ; la requête du prompt 4 prolonge celle du prompt 3 ; l'annonce, une
    seule fois juste avant le prompt 2, est relue après la reconstruction du worker à la même
    place (les messages de la dernière requête du prompt 2 ouvrent la première du prompt 3) ;
  - L6 : refus `-32602`, fichier des réglages inchangé.
Durée de la comparaison avec le disque : lignes `ya.resource_notice.compare` de la trace
(`PA_COMPACTION_TRACE=<fichier>`, `detail.micros`), médiane et maximum sur tous les cas ;
« absente » pour un binaire qui n'écrit pas cette ligne.

Usage seul (dossier neuf) :
    DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \\
      python3 -I bench/tools/capture/acp_live_changes.py --binary <yusagent> --out <dossier neuf>
Rapport : `<sortie>/acp-live/live-report.md` et `.json`. Code de sortie 1 si NON CONFORME.
macOS (sandbox-exec) requis ; bibliothèque standard uniquement.
"""

from __future__ import annotations

import argparse
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
NOTICE = "_yus/notice"
ANNOUNCEMENTS = ("[skills-changed]", "[mcp-changed]")
SKILL = "ya31-live"
DESCRIPTION = "Skill du cas acp-live (capture, ticket 31)."
MCP_SERVERS = {"ya31srv": {"type": "stdio", "command": "ya31-mcp-never-run"}}
TRACE_PHASE = "ya.resource_notice.compare"
CASES = ("L1", "L2", "L3", "L4", "L5", "L6")


def write_skill(agent: Path) -> Path:
    skill = agent / "skills" / SKILL
    skill.mkdir(parents=True, exist_ok=True)
    path = skill / "SKILL.md"
    path.write_text(f"---\nname: {SKILL}\ndescription: {DESCRIPTION}\n---\n\nRien à faire.\n")
    return path


def saved_of(response: dict | None) -> dict:
    return (((response or {}).get("result") or {}).get("_meta") or {}).get(META, {}).get("saved") or {}


def answered(response: dict | None) -> bool:
    return (response or {}).get("result") is not None


def request_count(case: Path) -> int:
    return len(list((case / "cap").glob("*.body.json")))


def case_run(binary: Path, cache: Path, work: Path, project: Path, root: Path, name: str) -> dict:
    case = root / name
    case.mkdir(parents=True)
    agent, env = probe.setup_in(work, cache, f"live-{name}")
    env["PA_COMPACTION_TRACE"] = str(case / "trace.log")
    skill_file = write_skill(agent) if name == "L3" else None
    server = probe.serve(case, agent)
    socket = work / f"live-{name}.sock"
    before = run.journal_sizes(agent)
    info: dict = {}
    try:
        b1 = probe.Bridge(binary, socket, env, project, case, "1")
        b1.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        new = b1.request("session/new", {"cwd": str(project), "mcpServers": []})
        sid = ((new or {}).get("result") or {}).get("sessionId")
        info["saved"] = saved_of(new)
        info["prompt1"] = b1.prompt(sid, run.PROMPT)
        info["requetes_prompt1"] = request_count(case)
        if name in ("L1", "L4"):
            created = b1.request("_yus/skills/create", {"name": SKILL, "description": DESCRIPTION})
            info["appels"] = [answered(created)]
            info["erreur"] = (created or {}).get("error")
        elif name == "L2":
            written = b1.request("_yus/settings/set", {"key": "mcpServers", "value": MCP_SERVERS})
            info["appels"] = [answered(written)]
            info["erreur"] = (written or {}).get("error")
        elif name == "L3":
            off = b1.request("_yus/skills/set", {"path": str(skill_file), "enabled": False})
            on = b1.request("_yus/skills/set", {"path": str(skill_file), "enabled": True})
            info["appels"] = [answered(off), answered(on)]
            info["erreur"] = (off or {}).get("error") or (on or {}).get("error")
        elif name == "L5":
            write_skill(agent)
            info["appels"] = []
        elif name == "L6":
            settings_path = agent / "settings.json"
            before_text = settings_path.read_text() if settings_path.is_file() else None
            refused = b1.request("_yus/settings/set", {"key": "permissions", "value": "manual"})
            info["refus"] = (refused or {}).get("error")
            info["fichier_inchange"] = (settings_path.read_text() if settings_path.is_file()
                                        else None) == before_text
            b1.close()
            return info
        info["prompt2"] = b1.prompt(sid, run.PROMPT_2)
        info["requetes_prompt2"] = request_count(case)
        if name not in ("L4", "L5"):
            b1.close()
            return info
        time.sleep(4)  # instantané du kernel (environ 1,5 s après la dernière cellule réussie)
        b1.close()
        if name == "L5":
            info["demon_arrete"] = probe.stop_daemon(socket)
        path = info["saved"].get("path") or ""
        b2 = probe.Bridge(binary, socket, env, project, case, "2", ["--resume", path])
        b2.request("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
        loaded = b2.request("session/load", {"sessionId": info["saved"].get("sessionId"),
                                             "cwd": str(project), "mcpServers": []})
        info["load"] = answered(loaded)
        info["prompt3"] = b2.prompt(info["saved"].get("sessionId"), run.PROMPT_2)
        info["requetes_prompt3"] = request_count(case)
        if name == "L5":
            info["prompt4"] = b2.prompt(info["saved"].get("sessionId"), run.PROMPT_2)
        b2.close()
    finally:
        probe.stop_daemon(socket)
        server.stop()
        info["usage_journals"] = run.copy_journals(agent, before, case)
    return info


def bodies(case: Path) -> list[dict]:
    return [json.loads(text) for text in compare.all_requests(case)]


def journal(case: Path) -> list[dict]:
    lines = []
    for path in sorted((case / "usage").glob("*.usage.ndjson")):
        lines += [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return sorted(lines, key=lambda entry: (entry["startMs"], entry["seq"]))


def invalidations(entries: list[dict]) -> list[dict]:
    found = []
    for entry in entries:
        invalidation = entry.get("invalidation")
        for item in invalidation if isinstance(invalidation, list) else [invalidation]:
            if isinstance(item, dict):
                found.append(item)
    return found


def fixed(body: dict) -> dict:
    """Tout le corps sauf les messages (prompt système, tools, paramètres), sans `cache_control`."""
    return probe.strip_cc({k: v for k, v in body.items() if k != "messages"})


def user_texts(messages: list[dict]) -> list[str]:
    texts = []
    for message in messages:
        if message.get("role") != "user":
            continue
        content = message.get("content")
        if isinstance(content, str):
            texts.append(content)
            continue
        texts += [block.get("text", "") for block in content or []
                  if isinstance(block, dict) and block.get("type") == "text"]
    return texts


def extends(before: dict, after: dict) -> bool:
    """`after` garde le préfixe de `before` : corps hors messages identique, messages prolongés."""
    old = probe.strip_cc(before.get("messages") or [])
    new = probe.strip_cc(after.get("messages") or [])
    return fixed(before) == fixed(after) and new[:len(old)] == old


def messages_extend(before: dict, after: dict) -> bool:
    """Les messages de `after` commencent par ceux de `before`, à l'identique (hors `cache_control`)."""
    old = probe.strip_cc(before.get("messages") or [])
    new = probe.strip_cc(after.get("messages") or [])
    return bool(old) and new[:len(old)] == old


def added_user_texts(before: dict, after: dict) -> list[str]:
    """Les textes de l'utilisateur que `after` ajoute après les messages de `before`."""
    return user_texts((after.get("messages") or [])[len(before.get("messages") or []):])


def notices(case: Path, tag: str) -> list[dict]:
    path = case / f"transcript-{tag}.jsonl"
    frames = [json.loads(line) for line in path.read_text().splitlines()] if path.is_file() else []
    found = []
    for frame in frames:
        msg = frame.get("msg") or {}
        if frame.get("dir") == "agent->client" and msg.get("method") == "session/update":
            notice = (((msg.get("params") or {}).get("update") or {}).get("_meta") or {}).get(NOTICE)
            if notice is not None:
                found.append(notice)
    return found


def announced(case: Path, info: dict, prefix: str) -> dict:
    """L1, L2 : préfixe gardé, une annonce juste avant le prompt 2, une notice, pas d'invalidation."""
    reqs = bodies(case)
    n1 = info.get("requetes_prompt1") or 0
    if not all(info.get("appels") or [False]) or n1 < 1 or len(reqs) <= n1:
        return {"ok": False, "appels": info.get("appels"), "erreur": info.get("erreur"),
                "requetes": len(reqs)}
    before, after = reqs[n1 - 1], reqs[n1]
    added = added_user_texts(before, after)
    announcements = [text for text in added if text.startswith(ANNOUNCEMENTS)]
    well_placed = (len(announcements) == 1 and announcements[0].startswith(prefix)
                   and len(added) >= 2 and added[-2] == announcements[0])
    live = notices(case, "1")
    causes = invalidations(journal(case))
    return {"ok": extends(before, after) and well_placed and len(live) == 1 and not causes,
            "prefixe_garde": extends(before, after), "annonce": announcements,
            "annonce_avant_le_prompt": well_placed, "notices": len(live),
            "invalidations": causes}


def check(root: Path, infos: dict) -> dict:
    checks: dict = {}
    for name, prefix in (("L1", "[skills-changed]"), ("L2", "[mcp-changed]")):
        checks[name] = announced(root / name, infos[name], prefix)

    case, info = root / "L3", infos["L3"]
    reqs = bodies(case)
    n1 = info.get("requetes_prompt1") or 0
    calls_ok = bool(info.get("appels")) and all(info["appels"])
    if calls_ok and n1 >= 1 and len(reqs) > n1:
        added = added_user_texts(reqs[n1 - 1], reqs[n1])
        quiet = (extends(reqs[n1 - 1], reqs[n1])
                 and not any(text.startswith(ANNOUNCEMENTS) for text in added)
                 and not notices(case, "1") and not invalidations(journal(case)))
    else:
        quiet = False
    checks["L3"] = {"ok": quiet, "appels": info.get("appels"), "erreur": info.get("erreur")}

    case, info = root / "L4", infos["L4"]
    first = announced(case, info, "[skills-changed]")
    reqs = bodies(case)
    n2, n3 = info.get("requetes_prompt2") or 0, info.get("requetes_prompt3") or 0
    resumed = (info.get("load") and 1 <= n2 < n3 <= len(reqs)
               and extends(reqs[n2 - 1], reqs[n2]) and not invalidations(journal(case)))
    checks["L4"] = {"ok": bool(first["ok"] and resumed), "avant_reprise": first,
                    "reprise_prolongee": bool(resumed)}

    case, info = root / "L5", infos["L5"]
    reqs = bodies(case)
    n1, n2 = info.get("requetes_prompt1") or 0, info.get("requetes_prompt2") or 0
    n3 = info.get("requetes_prompt3") or 0
    causes = invalidations(journal(case))
    resume = [c for c in causes if c.get("cause") == "resume_prefix_change"]
    continued = bool(info.get("load")) and 1 <= n3 < len(reqs) and extends(reqs[n3 - 1], reqs[n3])
    # Garde-fou 2 (option 1) : l'annonce, une seule fois juste avant le prompt 2, est relue
    # après la reconstruction du worker à la même place : la première requête du prompt 3 garde
    # les messages de la dernière du prompt 2 (prompt système différent, attendu : le skill).
    announced_once = False
    if 1 <= n1 < len(reqs):
        added = added_user_texts(reqs[n1 - 1], reqs[n1])
        found = [text for text in added if text.startswith("[skills-changed]")]
        announced_once = len(found) == 1 and len(added) >= 2 and added[-2] == found[0]
    reloaded = 1 <= n2 < len(reqs) and messages_extend(reqs[n2 - 1], reqs[n2])
    checks["L5"] = {"ok": len(causes) == 1 and len(resume) == 1
                    and resume[0].get("parts") == ["system"] and continued
                    and announced_once and reloaded,
                    "invalidations": causes, "prompt4_prolonge_prompt3": continued,
                    "annonce_une_fois_avant_le_prompt2": announced_once,
                    "historique_relu_a_l_identique": reloaded}

    info = infos["L6"]
    refusal = info.get("refus") or {}
    checks["L6"] = {"ok": refusal.get("code") == -32602 and bool(info.get("fichier_inchange")),
                    "refus": refusal, "fichier_inchange": info.get("fichier_inchange")}

    micros = []
    for name in CASES:
        trace = root / name / "trace.log"
        for line in trace.read_text().splitlines() if trace.is_file() else []:
            payload = line.partition("compaction-trace: ")[2]
            try:
                entry = json.loads(payload)
            except json.JSONDecodeError:
                continue
            if entry.get("phase") == TRACE_PHASE:
                micros.append((entry.get("detail") or {}).get("micros"))
    micros = [value for value in micros if isinstance(value, (int, float))]
    duration = ({"mesures": len(micros), "mediane_us": statistics.median(micros),
                 "max_us": max(micros)} if micros else {"mesures": 0})
    verdict = "CONFORME" if all(check["ok"] for check in checks.values()) else "NON CONFORME"
    return {"controles": checks, "duree_comparaison": duration, "verdict": verdict}


def render(report: dict) -> str:
    checks = report["controles"]
    lines = ["# Cas `acp-live` (ticket 31)", "", "| Cas | Résultat | Détail |", "|---|---|---|"]
    for name in CASES:
        detail = {k: v for k, v in checks[name].items() if k != "ok"}
        lines.append(f"| {name} | {'oui' if checks[name]['ok'] else 'NON'} | "
                     f"`{json.dumps(detail, ensure_ascii=False)}` |")
    duration = report["duree_comparaison"]
    if duration["mesures"]:
        lines += ["", f"Durée de la comparaison avec le disque : médiane {duration['mediane_us']} µs, "
                      f"maximum {duration['max_us']} µs, sur {duration['mesures']} prompts."]
    else:
        lines += ["", "Durée de la comparaison avec le disque : absente (aucune ligne "
                      f"`{TRACE_PHASE}` dans la trace)."]
    return "\n".join(lines + ["", report["verdict"]]) + "\n"


def live_cases(binary: Path, cache: Path, work: Path, project: Path, out: Path) -> dict:
    """Les six sessions et le rapport ; renvoie le rapport (verdict compris)."""
    root = out / "acp-live"
    root.mkdir(parents=True)
    infos = {name: case_run(binary, cache, work, project, root, name) for name in CASES}
    (root / "infos.json").write_text(json.dumps(infos, indent=2, ensure_ascii=False, default=str) + "\n")
    report = check(root, infos)
    (root / "live-report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    (root / "live-report.md").write_text(render(report))
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
    work = Path(os.environ.get("YA_ACP_LIVE_WORK", "/private/tmp/ya-acp-live"))
    shutil.rmtree(work, ignore_errors=True)
    (work / "tmp").mkdir(parents=True)
    project = work / "project"
    run.make_project(project)
    report = live_cases(binary, cache, work, project, out)
    print(render(report), end="")
    sys.exit(1 if report["verdict"] == "NON CONFORME" else 0)


if __name__ == "__main__":
    main()
