"""Bras Codex CLI du smoke bench (YA-14) : comparaison yusAgent contre Codex CLI.

Réglage d'un bras (CLAUDE.md règle 5, un fichier de `settings/`, jamais une branche) :
`{"harness": "codex", "codex": {"version": "0.169.0", "featureCuts": [...]}}`. Le binaire est
`codex` d'un dossier installé par `tools/install-codex.sh` (`~/.cache/yusagent-bin/codex-<v>/`).

Lancement : `codex exec --json` sous le même profil sandbox que yusAgent (`sandbox-agent.sb` :
`AGENT_DIR` = CODEX_HOME du banc, `BIN_DIR` = dossier de Codex), mêmes proxys : le reverse
proxy du banc sert de `base_url` à un provider personnalisé connecté par le login ChatGPT
(`codex/config.toml.in`), en WebSocket, chemin `/backend-api/codex/responses` seul.
Garde-fou (`check_launch`, appelé par `run.run_process` pour tout lancement de `codex`) :
refus hors `sandbox-exec` ou avec un CODEX_HOME autre que ceux du banc (jamais `~/.codex`).

Mesures, depuis les journaux de Codex 0.169.0 (lus dans le code du tag, plan YA-14) :
  - fichier de session (`$CODEX_HOME/sessions/AAAA/MM/JJ/rollout-*.jsonl`) : une ligne
    `token_usage_record` par réponse (usage brut de l'API Responses : `input_tokens` inclut le
    cache), convertie au format du journal d'usage du harness (`docs/usage.md`) : `input` =
    `input_tokens - cached_input_tokens - cache_write_input_tokens`, `cacheRead`, `cacheWrite`,
    `output` (raisonnement compris), `reasoning` ; `startMs` est l'heure de la ligne, donc la
    fin de l'appel (appariement au proxy par ordre, `metrics.match_transports_in_order`) ;
  - prewarm (`generate: false`, avant le premier appel d'un tour) : Codex lit sa réponse sans
    en garder l'usage (`core/src/client.rs:2569-2577` au tag) ; son usage vient du reverse
    proxy (`event: response`, `generate: false`), appel `prewarm` (annexe) ;
  - sortie `--json` : statut du tour (`turn.completed` ou `turn.failed`), erreurs (`error`,
    relancées ou non), edits (`file_change`, `completed` ou `failed`), commandes ;
  - lignes `SMOKE_*` des sorties d'outils (sondes des scénarios de capture).

Le journal converti est écrit à côté des fichiers de session copiés
(`sessions/<thread>.usage.ndjson`) : rapport, `--rereport`, `ab_stats.py` et `metrics.py` le
lisent comme celui du harness. Bibliothèque standard uniquement.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import pwd
import re
import tomllib
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
TEMPLATE = HERE / "codex" / "config.toml.in"
HOSTS = HERE / "hosts" / "codex.txt"
RESPONSES_PATH = "/backend-api/codex/responses"
# Fonctions de Codex que le banc peut couper si le premier corps envoyé au modèle reste
# identique (décision du 14/11/2026, preuve au banc de capture).
FEATURE_CUTS = ("apps", "plugins", "tool_suggest", "image_generation")
ROLLOUT = re.compile(r"^rollout-.+\.jsonl(?:\.zst)?$")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")


def b64(obj: dict) -> str:
    return base64.urlsafe_b64encode(json.dumps(obj).encode()).rstrip(b"=").decode()


# Identifiant factice du mode capture : JWT non signés (Codex lit `chatgpt_account_id` et
# l'expiration, `login/src/token_data.rs` au tag), offre « plus » (pas de config d'entreprise
# à télécharger), expiration en 2100 (pas de rafraîchissement).
FAKE_CLAIMS = {"https://api.openai.com/auth": {"chatgpt_plan_type": "plus",
                                               "chatgpt_account_id": "acct-smoke",
                                               "chatgpt_user_id": "user-smoke"},
               "exp": 4102444800}
FAKE_ID_TOKEN = ".".join([b64({"alg": "none"}), b64({"email": "smoke@localhost", **FAKE_CLAIMS}),
                          "smoke-fake-signature"])
FAKE_ACCESS_TOKEN = ".".join([b64({"alg": "none"}), b64(FAKE_CLAIMS), "smoke-fake-signature"])
FAKE_REFRESH_TOKEN = "smoke-fake-codex-refresh-not-a-token"
FAKE_VALUES = (FAKE_ID_TOKEN, FAKE_ACCESS_TOKEN, FAKE_REFRESH_TOKEN)


def settings_of(settings: dict) -> dict | None:
    """Réglage Codex d'un bras, `None` pour yusAgent."""
    if settings.get("harness", "yusagent") == "yusagent":
        return None
    if settings.get("harness") != "codex":
        raise ValueError(f"harness inconnu : {settings.get('harness')!r}")
    codex = dict(settings.get("codex") or {})
    unknown = sorted(set(codex.get("featureCuts") or []) - set(FEATURE_CUTS))
    if unknown:
        raise ValueError(f"codex.featureCuts : {unknown} hors de {list(FEATURE_CUTS)}")
    if not codex.get("version"):
        raise ValueError("codex.version requis")
    return codex


def manifest(binary: Path) -> dict:
    return json.loads((binary.parent / "manifest.json").read_text())


def check_install(binary: Path, version: str) -> dict:
    """Dossier installé par `tools/install-codex.sh` : version, sha256 de chaque fichier."""
    if not binary.is_file():
        raise ValueError(f"binaire Codex introuvable : {binary}")
    data = manifest(binary)
    if data.get("version") != version:
        raise ValueError(f"manifeste en version {data.get('version')}, réglage en {version}")
    for name, want in data["sha256"].items():
        got = hashlib.sha256((binary.parent / name).read_bytes()).hexdigest()
        if got != want:
            raise ValueError(f"sha256 de {name} {got} ≠ manifeste {want}")
    return data


def catalog(binary: Path) -> Path:
    return binary.parent / "models.json"


def catalog_ids(binary: Path) -> set[str]:
    return {m.get("slug") for m in json.loads(catalog(binary).read_text())["models"]}


def config_text(proxy_port: int, binary: Path, codex: dict) -> str:
    cuts = "\n".join(f"{name} = false" for name in codex.get("featureCuts") or [])
    return (TEMPLATE.read_text()
            .replace("{{BASE_URL}}", f"http://127.0.0.1:{proxy_port}/backend-api/codex")
            .replace("{{CATALOG}}", str(catalog(binary)))
            .replace("{{FEATURE_CUTS}}", cuts))


def write_config(codex_home: Path, proxy_port: int, binary: Path, codex: dict) -> str:
    """Écrit `config.toml` et rend son texte (copié dans le run, sha256 dans l'en-tête).

    Le fichier est remplacé en entier avant chaque session : ce qu'y écrit le login
    (`printf` du plan, même clé) ou Codex pendant la session (`--trust` y ajoute
    `[projects."<workspace>"]`, `exec/src/lib.rs:551-577` au tag) ne passe pas d'une session
    à l'autre. Refus si la config écrite ne garde pas les identifiants dans `auth.json`
    (`cli_auth_credentials_store = "file"`, jamais le trousseau)."""
    codex_home.mkdir(parents=True, exist_ok=True)
    text = config_text(proxy_port, binary, codex)
    store = tomllib.loads(text).get("cli_auth_credentials_store")
    if store != "file":
        raise ValueError(f"config de Codex : cli_auth_credentials_store = {store!r}, "
                         "\"file\" requis")
    (codex_home / "config.toml").write_text(text)
    return text


def write_fake_auth(codex_home: Path) -> None:
    codex_home.mkdir(parents=True, exist_ok=True)
    (codex_home / "auth.json").write_text(json.dumps({
        "auth_mode": "chatgpt", "OPENAI_API_KEY": None,
        "tokens": {"id_token": FAKE_ID_TOKEN, "access_token": FAKE_ACCESS_TOKEN,
                   "refresh_token": FAKE_REFRESH_TOKEN, "account_id": "acct-smoke"},
        "last_refresh": datetime.now(timezone.utc).isoformat()}))


def env(path: str, home: Path, tmp: Path, codex_home: Path, connect_port: int | None) -> dict:
    """Environnement de Codex : rien de plus que celui de yusAgent, sans ses variables propres
    (PI_*, PRIME_AGENT_*). `HTTPS_PROXY` (rafraîchissement du jeton) est retiré des commandes
    du modèle par `shell_environment_policy` (gabarit de config)."""
    out = {"PATH": path, "HOME": str(home), "TMPDIR": str(tmp), "LANG": "en_US.UTF-8",
           "DO_NOT_TRACK": "1", "CODEX_HOME": str(codex_home), "NO_PROXY": "127.0.0.1,localhost"}
    if connect_port is not None:
        out["HTTPS_PROXY"] = f"http://127.0.0.1:{connect_port}"
    return out


def cmd(binary: Path, model: str, effort: str, prompt: str) -> list[str]:
    """`codex exec` sans interaction (approbation `never` forcée par exec), sandbox de Codex
    coupé (imbriqué dans celui du banc, toutes ses commandes échoueraient), confiance au projet
    (charge son AGENTS.md, comme yusAgent), effort fixé."""
    return [str(binary), "exec", "--json", "--skip-git-repo-check", "--trust",
            "-s", "danger-full-access", "-m", model, "-c", f"model_reasoning_effort={effort}",
            prompt]


# --- Garde-fou de lancement -------------------------------------------------------------
# Incident du 14/11/2026 (passation YA-14) : un premier essai de Codex a tourné hors sandbox.
# Sans CODEX_HOME, Codex prend `~/.codex` (`utils/home-dir/src/lib.rs`, au tag) et s'en sert
# avant même de lire ses arguments, `--version` compris : `.env` lu (`arg0/src/lib.rs:134`),
# dossier `tmp/arg0-macos` créé (`arg0/src/lib.rs:322-352`).

SANDBOX_EXEC = "/usr/bin/sandbox-exec"
# CODEX_HOME du banc (`run.Cache`) : run réel, mode capture, sonde.
BENCH_HOMES = ("codex-home", "capture-codex-home", "probe-codex-home")


class LaunchRefused(RuntimeError):
    """Lancement de Codex refusé par le garde-fou du banc."""


def user_homes() -> set[Path]:
    """Dossier personnel de l'utilisateur, par `HOME` et par le compte (`getpwuid`)."""
    return {Path.home().resolve(), Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()}


def executable(argv: list[str]) -> str | None:
    """Programme lancé par `argv`, après le préfixe `sandbox-exec` et ses options."""
    if argv and argv[0] == SANDBOX_EXEC:
        i = 1
        while i < len(argv) and argv[i] in ("-f", "-p", "-n", "-D"):
            i += 2
        return argv[i] if i < len(argv) else None
    return argv[0] if argv else None


def is_codex(argv: list[str]) -> bool:
    exe = executable(argv)
    return exe is not None and Path(exe).name == "codex"


def check_launch(argv: list[str], env: dict, profile: Path, cache_root: Path) -> None:
    """Refuse un lancement de Codex hors `sandbox-exec` avec le profil de la phase agent
    (`profile`), ou avec un CODEX_HOME autre que ceux du banc sous `cache_root` (jamais
    `~/.codex`, ni sans CODEX_HOME), ou avec le vrai `HOME`. Le profil doit donner ce
    CODEX_HOME comme dossier agent (`AGENT_DIR`, seul dossier agent ouvert en écriture)."""
    problems = []
    if argv[:3] != [SANDBOX_EXEC, "-f", str(profile)]:
        problems.append(f"hors {SANDBOX_EXEC} -f {profile}")
    homes = user_homes()
    home = env.get("HOME")
    if not home or Path(home).resolve() in homes:
        problems.append(f"HOME absent ou réel ({home!r})")
    codex_home = env.get("CODEX_HOME")
    if not codex_home:
        problems.append("CODEX_HOME absent (Codex prendrait ~/.codex)")
    else:
        resolved = Path(codex_home).resolve()
        users = {h / ".codex" for h in homes}
        if any(resolved == u or u in resolved.parents for u in users):
            problems.append(f"CODEX_HOME dans ~/.codex ({resolved})")
        allowed = {(cache_root / name).resolve() for name in BENCH_HOMES}
        if resolved not in allowed:
            problems.append(f"CODEX_HOME {resolved} hors du banc ({sorted(map(str, allowed))})")
        if f"AGENT_DIR={resolved}" not in argv:
            problems.append(f"AGENT_DIR du profil ≠ CODEX_HOME {resolved}")
    if problems:
        raise LaunchRefused("lancement de Codex refusé : " + " ; ".join(problems))


def rollouts(codex_home: Path) -> dict[Path, int]:
    root = codex_home / "sessions"
    if not root.exists():
        return {}
    return {p: p.stat().st_size for p in root.rglob("*") if p.is_file() and ROLLOUT.match(p.name)}


# Masquage au ramassage (YA-14, chauffe du 14/11/2026) : Codex écrit l'id de compte et l'id
# utilisateur ChatGPT dans `session_meta` (`core/src/session/session.rs:1222` au tag).
MASK = "<masqué>"
META_IDS = ("creator_account_id", "creator_user_id")


def mask_text(text: str, secrets) -> tuple[str, int]:
    """Texte d'un fichier de session avec `META_IDS` de `session_meta` et toute valeur de
    `secrets` (filet : `run.secret_values`) remplacés par `MASK`, partout dans le fichier ;
    rend aussi le nombre de remplacements. Le reste du fichier est inchangé."""
    values = {v for v in secrets if v}
    for line in text.splitlines():
        if '"session_meta"' not in line:
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry.get("type") == "session_meta":
            payload = entry.get("payload") or {}
            values |= {payload[k] for k in META_IDS if isinstance(payload.get(k), str) and payload[k]}
    count = 0
    for value in sorted(values, key=len, reverse=True):
        count += text.count(value)
        text = text.replace(value, MASK)
    return text, count


def copy_rollouts(codex_home: Path, before: dict[Path, int], dest: Path,
                  secrets=()) -> list[Path]:
    """Copie les fichiers de session nouveaux ou agrandis, masqués (`mask_text`) ; les
    originaux, dans CODEX_HOME hors du dépôt, ne sont pas modifiés."""
    dest.mkdir(parents=True, exist_ok=True)
    copied = []
    for path, size in sorted(rollouts(codex_home).items()):
        if before.get(path) == size:
            continue
        if path.name.endswith(".zst"):
            raise ValueError(f"fichier de session compressé, non lisible sans zstd : {path}")
        target = dest / path.name
        target.write_text(mask_text(path.read_text(), secrets)[0])
        copied.append(target)
    return copied


def read_jsonl(path: Path) -> list[dict]:
    out = []
    for line in path.read_text().splitlines():
        if line.strip():
            try:
                out.append(json.loads(line))
            except ValueError:
                continue
    return out


def ts_ms(value) -> int | None:
    if not isinstance(value, str):
        return None
    try:
        return round(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)
    except ValueError:
        return None


def thread_of(entries: list[dict]) -> str | None:
    for e in entries:
        if e.get("type") == "session_meta":
            return (e.get("payload") or {}).get("id")
    return None


def usage_record(usage: dict, **fields) -> dict:
    """Usage brut de l'API Responses (Codex `TokenUsage`) au format du journal du harness."""
    total = usage.get("input_tokens") or 0
    read = usage.get("cached_input_tokens") or 0
    write = usage.get("cache_write_input_tokens") or 0
    prompt = max(total, read + write)
    return {**fields, "input": max(0, total - read - write), "cacheRead": read,
            "cacheWrite": write, "output": usage.get("output_tokens") or 0,
            "reasoning": usage.get("reasoning_output_tokens") or 0,
            "cacheRate": read / prompt if prompt else None}


def records(rollout_paths: list[Path], proxy_log: list[dict], main_thread: str | None) -> list[dict]:
    """Journal d'usage converti : appels de boucle (fichiers de session) et prewarms (proxy),
    numérotés par session dans l'ordre de leur fin."""
    by_thread: dict[str, list[dict]] = {}
    for path in rollout_paths:
        entries = read_jsonl(path)
        thread = thread_of(entries)
        if thread is None:
            continue
        kind = "main" if thread == main_thread else "subagent"
        model = effort = None
        for e in entries:
            payload = e.get("payload") or {}
            if e.get("type") == "turn_context":
                model, effort = payload.get("model"), payload.get("effort")
            elif e.get("type") == "token_usage_record":
                by_thread.setdefault(thread, []).append(usage_record(
                    payload.get("usage") or {}, sessionId=thread, kind=kind, model=model,
                    effort=effort, startMs=ts_ms(e.get("timestamp")),
                    responseId=payload.get("response_id"), source="codex-rollout"))
    for e in proxy_log:
        if e.get("event") == "response" and e.get("generate") is False and e.get("session"):
            u = e.get("usage") or {}
            by_thread.setdefault(e["session"], []).append(usage_record(
                {"input_tokens": u.get("input_tokens"), "cached_input_tokens": u.get("cached_tokens"),
                 "cache_write_input_tokens": u.get("cache_write_tokens"),
                 "output_tokens": u.get("output_tokens"),
                 "reasoning_output_tokens": u.get("reasoning_tokens")},
                sessionId=e["session"], kind="prewarm", model=None, effort=None,
                startMs=e.get("ts"), responseId=e.get("response_id"), source="proxy"))
    out = []
    for thread, rows in by_thread.items():
        rows.sort(key=lambda r: (r.get("startMs") or 0, r["kind"] != "prewarm"))
        # Le prewarm part avec le modèle et l'effort de sa session (corps non journalisé).
        loop = next((r for r in rows if r["kind"] != "prewarm"), {})
        for seq, row in enumerate(rows, 1):
            if row["kind"] == "prewarm":
                row = {**row, "model": loop.get("model"), "effort": loop.get("effort")}
            out.append({**row, "seq": seq})
    return out


def output_text(value) -> str:
    """Texte d'une sortie d'outil (`output` : texte ou liste de contenus)."""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(output_text(v) for v in value)
    if isinstance(value, dict):
        return output_text(value.get("text") or value.get("output") or "")
    return ""


def markers(rollout_paths: list[Path]) -> dict[str, list[str]]:
    """Lignes `SMOKE_*` des sorties d'outils (sondes des scénarios de capture)."""
    found: dict[str, list[str]] = {}
    for path in rollout_paths:
        for e in read_jsonl(path):
            payload = e.get("payload") or {}
            if e.get("type") != "response_item" or payload.get("type") not in (
                    "custom_tool_call_output", "function_call_output"):
                continue
            for line in output_text(payload.get("output")).splitlines():
                line = line.strip()
                if line.startswith("SMOKE_"):
                    name, _, value = line.partition(" ")
                    found.setdefault(name, []).append(value.strip())
    return found


def rollout_counts(rollout_paths: list[Path]) -> dict:
    """Appels d'outils de premier niveau, compactions et dernier état du quota."""
    calls: dict[str, int] = {}
    compactions = 0
    rate_limits = None
    for path in rollout_paths:
        for e in read_jsonl(path):
            payload = e.get("payload") or {}
            if e.get("type") == "compacted":
                compactions += 1
            elif e.get("type") == "response_item" and payload.get("type") in (
                    "custom_tool_call", "function_call"):
                calls[payload.get("name")] = calls.get(payload.get("name"), 0) + 1
            elif e.get("type") == "event_msg" and payload.get("type") == "token_count":
                limits = payload.get("rate_limits")
                if isinstance(limits, dict):
                    primary = limits.get("primary") or {}
                    secondary = limits.get("secondary") or {}
                    rate_limits = {"primary_used_percent": primary.get("used_percent"),
                                   "secondary_used_percent": secondary.get("used_percent"),
                                   "limit_reached": limits.get("rate_limit_reached_type")}
    return {"tool_calls": calls, "compactions": compactions, "rate_limits": rate_limits}


def events(out_jsonl: Path) -> dict:
    """Sortie `codex exec --json` : thread, statut du tour, erreurs, edits, commandes."""
    thread = status = error = None
    errors: list[str] = []
    changes = {"completed": 0, "failed": 0}
    commands = {"total": 0, "nonzero": 0}
    last_message = None
    rows = read_jsonl(out_jsonl) if out_jsonl.exists() else []
    for e in rows:
        kind = e.get("type")
        item = e.get("item") or {}
        if kind == "thread.started":
            thread = e.get("thread_id")
        elif kind == "turn.completed":
            status = "completed"
        elif kind == "turn.failed":
            status = "failed"
            error = (e.get("error") or {}).get("message")
        elif kind == "error":
            errors.append(str(e.get("message")))
        elif kind == "item.completed" and item.get("type") == "file_change":
            changes["completed" if item.get("status") == "completed" else "failed"] += 1
        elif kind == "item.completed" and item.get("type") == "command_execution":
            commands["total"] += 1
            commands["nonzero"] += item.get("exit_code") not in (0, None)
        elif kind == "item.completed" and item.get("type") == "agent_message":
            last_message = item.get("text")
    return {"thread_id": thread, "status": status, "error": error, "errors": errors,
            "file_changes": changes, "commands": commands, "last_message": last_message,
            "events": len(rows)}


def edits(ev: dict) -> dict:
    """Edits au premier essai (même forme que `metrics.edit_attempts`) depuis les items
    `file_change` de `--json`, un par appel d'`apply_patch` imbriqué dans `exec`. Limite : un
    patch refusé à l'analyse, avant l'application, peut ne produire aucun item (borne haute
    du taux, notée au plan)."""
    done, failed = ev["file_changes"]["completed"], ev["file_changes"]["failed"]
    attempts = done + failed
    return {"apply_patch": {"calls": attempts, "applied": done, "refused_outside_workspace": None},
            "edit": None, "attempts": attempts, "applied": done,
            "first_try_rate": done / attempts if attempts else None, "source": "file_change"}


def full_bodies(proxy_log: list[dict]) -> dict:
    """Requêtes de génération (`generate` différent de `false`, prewarm exclu) parties en
    corps complet, lues au reverse proxy : WebSocket sans `previous_response_id`, ou SSE (Codex
    n'en met que sur WebSocket, `core/src/client.rs:2356-2392` au tag). Biais contre Codex :
    avec un provider dont le nom n'est pas « OpenAI », un corps complet rejoue ses items sans
    leur `encrypted_content` (hors raisonnement et compaction, `core/src/client.rs:1231-1249`).
    `after_first` : hors première requête de génération de sa session (rien à rejouer avant)."""
    gens = sorted((e for e in proxy_log if e.get("event") == "request"
                   and e.get("transport") in ("websocket", "sse") and e.get("generate") is not False),
                  key=lambda e: e.get("ts") or 0)
    seen: set = set()
    full = after_first = 0
    for e in gens:
        is_full = e.get("transport") == "sse" or e.get("continuation") is False
        full += is_full
        after_first += is_full and e.get("session") in seen
        seen.add(e.get("session"))
    return {"generation_requests": len(gens), "full_body": full, "after_first": after_first}


def final(ev: dict) -> dict:
    """Dernier état de la session principale, au format de `metrics.final_assistant`."""
    stop = {"completed": "stop", "failed": "error"}.get(ev["status"])
    return {"stop_reason": stop, "error": ev["error"]}


def session(out: Path, proxy_log: list[dict]) -> dict:
    """Mesures d'une session Codex depuis son dossier de run (en direct ou `--rereport`) :
    `out.jsonl`, fichiers de session copiés dans `sessions/`, journal du reverse proxy.
    Écrit le journal d'usage converti (`sessions/<thread>.usage.ndjson`)."""
    ev = events(out / "out.jsonl")
    sessions = out / "sessions"
    paths = sorted(p for p in sessions.glob("rollout-*.jsonl")) if sessions.exists() else []
    recs = records(paths, proxy_log, ev["thread_id"])
    for old in sessions.glob("*.usage.ndjson") if sessions.exists() else []:
        old.unlink()
    for thread in sorted({r["sessionId"] for r in recs}):
        if not UUID.fullmatch(thread or ""):
            continue
        with (sessions / f"{thread}.usage.ndjson").open("w") as handle:
            for r in sorted((r for r in recs if r["sessionId"] == thread), key=lambda r: r["seq"]):
                handle.write(json.dumps(r) + "\n")
    counts = rollout_counts(paths)
    main = [r for r in recs if r["sessionId"] == ev["thread_id"]]
    subs = [r for r in recs if r["sessionId"] != ev["thread_id"]]
    return {"records": main, "sub_records": subs, "entries": [], "sub_entries": [],
            "codex": {"events": ev, "final": final(ev), "markers": markers(paths),
                      "edits": edits(ev), "full_bodies": full_bodies(proxy_log),
                      "rollouts": [p.name for p in paths],
                      "threads": sorted({r["sessionId"] for r in recs}), **counts}}


def cells(summary: dict) -> dict:
    """Appels d'`exec` (code JavaScript) : pas de mesure d'erreur comparable aux cellules
    `python_repl` de yusAgent (`errors` à `null`)."""
    calls = summary.get("tool_calls") or {}
    return {"cells": calls.get("exec", 0), "errors": None, "syntax_errors": None, "by_type": {},
            "tool_calls": calls}
