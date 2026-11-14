"""Reprise (`--continue`) de la dernière session d'une tâche du smoke bench, dans un nouveau
processus, avec la sandbox, les proxies (reverse proxy journalisé, CONNECT à liste blanche) et
le dossier agent de `run.py`. Le workspace de la tâche n'est pas restauré : le harness retrouve
la session par son cwd. C'est un appel réel au modèle.

    DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/continue_session.py \\
        --binary <dossier>/prime-agent --task <id> --out <run>/tasks/<id>/continue-1 \\
        [--model gpt-6.1-terra] [--effort high] [--settings bench/smoke/settings/<x>.json] \\
        "<prompt>"

Sortie dans `--out` : `proxy.ndjson` (requêtes, champ `continuation`), `connect.ndjson`,
`out.jsonl`, `err.log`, `sessions/` (session et journal d'usage agrandis).
"""
import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import proxies  # noqa: E402
import run as R  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--task", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--model", default="gpt-6.1-terra")
    parser.add_argument("--effort", default=R.DEFAULT_EFFORT)
    parser.add_argument("--settings", type=Path, default=HERE / "settings" / "default.json")
    parser.add_argument("--cache", type=Path, default=R.DEFAULT_CACHE)
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--idle-timeout", type=int, default=R.IDLE_TIMEOUT_S)
    parser.add_argument("prompt")
    args = parser.parse_args()

    binary = args.binary.resolve()
    args.out.mkdir(parents=True)
    cache = R.Cache(args.cache.expanduser().resolve(), binary, False)
    if not (cache.agent / "auth.json").exists():
        R.fail(f"aucun login dans {cache.agent}")
    task = R.load_tasks([args.task], False)[0]
    workspace, _, _ = R.task_paths(task, cache)
    if not workspace.is_dir():
        R.fail(f"workspace absent : {workspace} (lancer d'abord la tâche avec run.py)")
    _, bundled_ids = R.catalog(binary)
    source = R.model_source(args.model, bundled_ids)
    settings = json.loads(args.settings.read_text())

    # Même commande que run.py, `--continue` avant le prompt.
    base_cmd = R.agent_cmd
    R.agent_cmd = lambda *a: [*base_cmd(*a)[:-1], "--continue", a[-1]]

    # Journal propre au proxy CONNECT ; `agent_session` en copie les entrées de la session
    # dans `connect.ndjson`.
    connect = proxies.ConnectProxy(args.out / "connect-proxy.ndjson",
                                   proxies.load_hosts(R.HOSTS))
    proxies.serve(connect)
    access = R.Access(args.out, False, "", connect.port)
    try:
        session = R.agent_session(binary, cache, cache.agent, args.out, args.model,
                                  args.effort, args.prompt, workspace, access, connect, source,
                                  settings, False, args.timeout, args.idle_timeout)
    finally:
        access.stop()
        connect.shutdown()
        connect.server_close()
    print(json.dumps({"process": session["process"], "workspace": str(workspace),
                      "usage_records": len(session["records"]),
                      "session_entries": len(session["entries"])},
                     default=str, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
