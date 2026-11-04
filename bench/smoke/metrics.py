"""Mesures d'un run de tâche (T06, YA-14), depuis les journaux du harness et des proxys.

Sources, sans rien recalculer qui existe déjà :
  - `<id>.usage.ndjson` (T04, sémantique dans `docs/usage.md`) : tokens, type d'appel,
    raison d'arrêt, effort, TTFT, invalidations ;
  - JSONL de session : cellules `python_repl` en erreur, échecs de transport notés par
    le harness (`diagnostics`, `provider_transport_failure`) ;
  - journal du reverse proxy : transport de chaque requête modèle ;
  - journal du proxy CONNECT : CONNECT refusés.

Un champ absent est écrit `null`, jamais estimé. Les mesures d'un run de tâche portent
sur la session principale et ses sous-agents (`rlm.spawn`) ; `subagent_share` en donne
la part.

Miss serveur (T07) : appel de la boucle qui n'est pas le premier de sa session, sans
invalidation journalisée, avec un prompt non vide et `cacheRead = 0`. Le harness n'a pas
changé le préfixe : le cache a manqué côté serveur. Taux « évitable » : cacheRead / prompt
sur les appels de la boucle hors premier de session, hors invalidations et hors miss
serveur. Calculés depuis les journaux seuls : `python3 -I bench/smoke/metrics.py <run>`
les recalcule pour un run déjà fait.
"""

from __future__ import annotations

import json
import re
import statistics
import sys
from pathlib import Path

TOKEN_FIELDS = ("input", "output", "cacheRead", "cacheWrite", "cacheWrite5m", "cacheWrite1h",
                "cacheWrite30m", "cacheWrite12h", "reasoning")
# Appels de la boucle : WebSocket attendu. Les appels annexes passent en SSE par
# construction (compaction : crates/pa-core/src/session_engine/compaction_exec.rs:177,
# /refine : refine.rs:664, résumé de branche : branch_summarization.rs:468).
LOOP_KINDS = {"main", "subagent"}
# Coût pessimiste (T08b, YA-13) : l'`input` non caché au tarif d'écriture 30 min, comme si
# tout ce qui n'a pas été lu en cache y avait été écrit. Plafond pour un provider qui
# n'annonce pas ses écritures (Codex : `cache_write_tokens` toujours à 0, docs/usage.md) ;
# trop haut pour un provider qui les annonce. 30 min et non 12 h : le harness ne demande
# jamais 12 h.
PESSIMISTIC_INPUT_PRICE = "cache_write_30m"
ERROR_LINE = re.compile(r"^(\w+(?:Error|Exception|Exit|Interrupt))\b")


def read_ndjson(paths: list[Path]) -> list[dict]:
    rows = []
    for path in paths:
        rows += [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return rows


def median(values: list[float]) -> float | None:
    return statistics.median(values) if values else None


def token_totals(records: list[dict]) -> dict:
    """Somme par catégorie ; `null` si aucun appel ne donne le champ."""
    totals = {}
    for field in TOKEN_FIELDS:
        values = [r[field] for r in records if isinstance(r.get(field), (int, float))]
        totals[field] = sum(values) if values else None
    return totals


def prompt_size(record: dict) -> int:
    return sum(record.get(f) or 0 for f in ("input", "cacheRead", "cacheWrite"))


def cache_rates(records: list[dict]) -> dict:
    """Taux de session et médiane par requête de continuation (CDC, YA-04 ; docs/usage.md).

    Session : cacheRead / (input + cacheRead + cacheWrite) sur les appels de la boucle.
    Continuation : appel de la boucle qui n'est pas le premier de sa session, sans
    invalidation journalisée ; médiane de son `cacheRate`.
    """
    loop = [r for r in records if r.get("kind") in LOOP_KINDS]
    total = sum(prompt_size(r) for r in loop)
    read = sum(r.get("cacheRead") or 0 for r in loop)
    first_seq: dict[str, int] = {}
    for r in loop:
        sid = r.get("sessionId")
        first_seq[sid] = min(first_seq.get(sid, r.get("seq", 0)), r.get("seq", 0))
    continuation = [r["cacheRate"] for r in loop
                    if r.get("seq") != first_seq.get(r.get("sessionId"))
                    and "invalidation" not in r and isinstance(r.get("cacheRate"), (int, float))]
    return {"session_rate": read / total if total else None,
            "continuation_median": median(continuation),
            "continuation_requests": len(continuation)}


def first_seqs(loop: list[dict]) -> dict[str, int]:
    first: dict[str, int] = {}
    for r in loop:
        sid = r.get("sessionId")
        first[sid] = min(first.get(sid, r.get("seq", 0)), r.get("seq", 0))
    return first


def server_misses(records: list[dict], per_request: list[dict]) -> list[dict]:
    """Miss complets côté serveur, avec délai, transport et continuation de chacun."""
    loop = [r for r in records if r.get("kind") in LOOP_KINDS]
    first = first_seqs(loop)
    by_call = {(p["session"], p["seq"]): p for p in per_request}
    misses = []
    for r in sorted(loop, key=lambda r: (r.get("sessionId") or "", r.get("seq", 0))):
        if (r.get("seq") == first.get(r.get("sessionId")) or "invalidation" in r
                or not prompt_size(r) or r.get("cacheRead") != 0):
            continue
        proxy = by_call.get((r.get("sessionId"), r.get("seq")), {})
        misses.append({"session": r.get("sessionId"), "seq": r.get("seq"), "kind": r.get("kind"),
                       "prompt": prompt_size(r), "since_prev_ms": r.get("sincePrevMs"),
                       "transport": proxy.get("transport"),
                       "continuation": proxy.get("continuation")})
    return misses


def avoidable_cache_rate(records: list[dict], misses: list[dict]) -> float | None:
    """cacheRead / prompt, hors premier appel de session, invalidations et miss serveur."""
    loop = [r for r in records if r.get("kind") in LOOP_KINDS]
    first = first_seqs(loop)
    missed = {(m["session"], m["seq"]) for m in misses}
    kept = [r for r in loop if r.get("seq") != first.get(r.get("sessionId"))
            and "invalidation" not in r and (r.get("sessionId"), r.get("seq")) not in missed]
    total = sum(prompt_size(r) for r in kept)
    return sum(r.get("cacheRead") or 0 for r in kept) / total if total else None


def price_row(prices: dict, model: str, record: dict) -> tuple[dict | None, str | None]:
    entry = prices.get("models", {}).get(model)
    if entry is None:
        return None, None
    threshold = prices.get("long_context_threshold_input_tokens")
    tier = "long" if threshold is not None and prompt_size(record) > threshold else "short"
    return entry.get(tier), tier


def write_cost(row: dict, record: dict, write_key: str) -> float:
    """Écriture en cache d'un appel, en USD × 10⁶ (T08, YA-13).

    Avec la ventilation du provider (`cacheWrite30m`, `cacheWrite12h`), chaque part au tarif
    de son TTL dans prices.json (`cache_write_30m`, `cache_write_12h`), le reste éventuel de
    `cacheWrite` au tarif `write_key` ; sans ventilation, tout `cacheWrite` au tarif `write_key`.
    """
    total = record.get("cacheWrite") or 0
    thirty = record.get("cacheWrite30m")
    twelve = record.get("cacheWrite12h")
    if thirty is None and twelve is None:
        return total * row[write_key]
    thirty, twelve = thirty or 0, twelve or 0
    rest = max(0, total - thirty - twelve)
    return thirty * row["cache_write_30m"] + twelve * row["cache_write_12h"] + rest * row[write_key]


def request_cost(prices: dict, model: str, record: dict, write_key: str) -> float | None:
    """Coût théorique d'un appel au tarif API (YA-13). `None` sans tarif pour le modèle.

    `reasoning` est inclus dans `output` (docs/usage.md) : il n'est pas compté deux fois.
    """
    row, _ = price_row(prices, model, record)
    if row is None:
        return None
    per = 1_000_000
    return ((record.get("input") or 0) * row["input"]
            + (record.get("cacheRead") or 0) * row["cached_input"]
            + write_cost(row, record, write_key)
            + (record.get("output") or 0) * row["output"]) / per


def pessimistic_request_cost(prices: dict, model: str, record: dict,
                             write_key: str) -> float | None:
    """`request_cost` avec l'`input` non caché au tarif `PESSIMISTIC_INPUT_PRICE` (T08b).

    Mêmes termes sinon : `cacheRead` au tarif de lecture, `cacheWrite` au tarif de son TTL,
    `output` au tarif de sortie, contexte long au-delà du seuil. `None` sans tarif.
    """
    row, _ = price_row(prices, model, record)
    if row is None:
        return None
    per = 1_000_000
    return ((record.get("input") or 0) * row[PESSIMISTIC_INPUT_PRICE]
            + (record.get("cacheRead") or 0) * row["cached_input"]
            + write_cost(row, record, write_key)
            + (record.get("output") or 0) * row["output"]) / per


def costs(prices: dict, model: str, records: list[dict], write_key: str) -> dict:
    per_request = [request_cost(prices, model, r, write_key) for r in records]
    known = [c for c in per_request if c is not None]
    pessimistic = [pessimistic_request_cost(prices, model, r, write_key) for r in records]
    invalidated = [r for r in records if "invalidation" in r]
    invalidation_cost = None
    if invalidated and model in prices.get("models", {}):
        invalidation_cost = sum(
            write_cost(price_row(prices, model, r)[0], r, write_key) / 1_000_000
            for r in invalidated)
    tiers = sorted({price_row(prices, model, r)[1] for r in records} - {None})
    return {"total": sum(known) if known and len(known) == len(per_request) else None,
            "pessimistic_total": (sum(pessimistic) if pessimistic and None not in pessimistic
                                  else None),
            "missing_price": model not in prices.get("models", {}),
            "tiers": tiers,
            "invalidations": {"count": len(invalidated),
                              "cache_write": sum(r.get("cacheWrite") or 0 for r in invalidated),
                              "cost": invalidation_cost}}


def subagent_share(sub_records: list[dict], records: list[dict], misses: list[dict],
                   prices: dict, model: str, write_key: str) -> dict:
    """Part des sous-agents (`rlm.spawn`) dans un run de tâche : `records` porte tous les
    appels (session principale et sous-agents), `sub_records` ceux des sous-agents."""
    sessions = {r.get("sessionId") for r in sub_records}
    cost = costs(prices, model, sub_records, write_key)["total"] if sub_records else 0.0
    total = costs(prices, model, records, write_key)["total"]
    return {"sessions": len(sessions), "calls": len(sub_records),
            "tokens": token_totals(sub_records), "cost": cost,
            "cost_share": cost / total if cost is not None and total else None,
            "cache_session_rate": cache_rates(sub_records)["session_rate"],
            "server_misses": sum(1 for m in misses if m["session"] in sessions)}


def calls(records: list[dict]) -> dict:
    by_kind: dict[str, int] = {}
    for r in records:
        by_kind[r.get("kind", "?")] = by_kind.get(r.get("kind", "?"), 0) + 1
    return {"total": len(records), "by_kind": by_kind,
            "turns": sum(1 for r in records if r.get("kind") == "main"),
            "stop_reasons": sorted({r.get("stopReason") for r in records} - {None}),
            "truncated": sum(1 for r in records if r.get("stopReason") == "length"),
            "efforts": sorted({str(r.get("effort")) for r in records}),
            "models": sorted({str(r.get("model")) for r in records}),
            "ttft_median_ms": median([r["ttftMs"] for r in records
                                      if isinstance(r.get("ttftMs"), (int, float))])}


def session_entries(paths: list[Path]) -> list[dict]:
    return read_ndjson(paths)


def cell_errors(entries: list[dict]) -> dict:
    """Cellules `python_repl` en erreur, par type (SyntaxError à part)."""
    cells = errors = syntax = 0
    by_type: dict[str, int] = {}
    for entry in entries:
        message = entry.get("message") or {}
        if message.get("role") != "toolResult" or message.get("toolName") != "python_repl":
            continue
        cells += 1
        if not message.get("isError"):
            continue
        errors += 1
        text = "".join(part.get("text", "") for part in message.get("content") or []
                       if isinstance(part, dict))
        kind = "kernel_startup" if text.startswith("kernel startup failed") else "other"
        for line in reversed(text.splitlines()):
            found = ERROR_LINE.match(line.strip())
            if found:
                kind = found.group(1)
                break
        if kind == "SyntaxError":
            syntax += 1
        else:
            by_type[kind] = by_type.get(kind, 0) + 1
    return {"cells": cells, "errors": errors, "syntax_errors": syntax, "by_type": by_type}


def session_transport_failures(entries: list[dict]) -> int:
    """Échecs de transport notés par le harness sur les messages assistant (repli SSE)."""
    count = 0
    for entry in entries:
        message = entry.get("message") or {}
        if message.get("role") == "assistant":
            count += sum(1 for d in message.get("diagnostics") or []
                         if d.get("type") == "provider_transport_failure")
    return count


def final_assistant(entries: list[dict]) -> dict:
    last = None
    for entry in entries:
        message = entry.get("message") or {}
        if message.get("role") == "assistant":
            last = message
    if last is None:
        return {"stop_reason": None, "error": None}
    return {"stop_reason": last.get("stopReason"), "error": last.get("errorMessage")}


def kernel_markers(entries: list[dict]) -> dict[str, list[str]]:
    """Lignes `SMOKE_*` imprimées par les cellules (sondes des scénarios de capture)."""
    found: dict[str, list[str]] = {}
    for entry in entries:
        message = entry.get("message") or {}
        if message.get("role") != "toolResult":
            continue
        for part in message.get("content") or []:
            for line in (part.get("text", "") if isinstance(part, dict) else "").splitlines():
                if line.startswith("SMOKE_"):
                    name, _, value = line.partition(" ")
                    found.setdefault(name, []).append(value.strip())
    return found


def last_marker(markers: dict[str, list[str]], name: str) -> str | None:
    values = markers.get(name)
    return values[-1] if values else None


def isolation(markers: dict[str, list[str]]) -> dict | None:
    """Sonde d'isolation : lectures hors workspace tentées et réussies (doit être vide)."""
    reads = ([f"python {v}" for v in markers.get("SMOKE_READ_PY", [])]
             + [f"bash {v}" for v in markers.get("SMOKE_READ_BASH", [])])
    if not reads:
        return None
    return {"checked": len(reads), "readable": [r for r in reads if r.endswith(" ok")],
            "results": reads}


def context_peak(records: list[dict]) -> int | None:
    """Plus gros prompt envoyé (input + cacheRead + cacheWrite), en tokens."""
    return max((prompt_size(r) for r in records), default=None)


def match_transports(records: list[dict], proxy_log: list[dict]) -> dict:
    """Transport de chaque appel modèle : appariement journal d'usage ↔ reverse proxy.

    Une requête du proxy (`event: request`) porte l'id de session lu dans son corps
    (`prompt_cache_key`). Pour chaque appel du journal d'usage, dans l'ordre de début,
    on prend la requête non appariée de la même session la plus proche dans le temps.
    """
    requests = [dict(e, matched=False) for e in proxy_log if e.get("event") == "request"]
    per_request = []
    for record in sorted(records, key=lambda r: r.get("startMs", 0)):
        candidates = [q for q in requests if not q["matched"]
                      and q.get("session") in (record.get("sessionId"), None)]
        best = min(candidates, key=lambda q: abs(q["ts"] - record.get("startMs", 0)),
                   default=None)
        if best is not None:
            best["matched"] = True
        transport = best["transport"] if best else None
        expected = "websocket" if record.get("kind") in LOOP_KINDS else "sse"
        per_request.append({"session": record.get("sessionId"), "seq": record.get("seq"),
                            "kind": record.get("kind"), "transport": transport,
                            "expected": expected,
                            "continuation": best.get("continuation") if best else None})
    counts: dict[str, int] = {}
    for q in requests:
        counts[q["transport"]] = counts.get(q["transport"], 0) + 1
    return {"requests": counts,
            "unmatched_usage": sum(1 for p in per_request if p["transport"] is None),
            "unmatched_proxy": sum(1 for q in requests if not q["matched"]),
            "loop_on_sse": sum(1 for p in per_request
                               if p["expected"] == "websocket" and p["transport"] == "sse"),
            "refused": sum(1 for e in proxy_log if e.get("event") == "refused"),
            "upstream_errors": sum(1 for e in proxy_log if e.get("event") == "upstream_error"),
            "per_request": per_request}


def connect_summary(entries: list[dict]) -> dict:
    connects = [e for e in entries if e.get("event") == "connect"]
    return {"accepted": sorted({e.get("host") for e in connects if e.get("accepted")} - {None}),
            "refused": sum(1 for e in connects if not e.get("accepted")),
            "refused_hosts": sorted({str(e.get("host")) for e in connects
                                     if not e.get("accepted")})}


def task_run_cache(run_task_dir: Path) -> dict:
    """Miss serveur et taux de cache d'un run de tâche, relus dans ses journaux (session
    principale et sous-agents, `sessions/subagents/`)."""
    records = read_ndjson(sorted((run_task_dir / "sessions").rglob("*.usage.ndjson")))
    proxy = run_task_dir / "proxy.ndjson"
    proxy_log = read_ndjson([proxy]) if proxy.exists() else []
    misses = server_misses(records, match_transports(records, proxy_log)["per_request"])
    return {"server_misses": misses, "cache_avoidable_rate": avoidable_cache_rate(records, misses),
            "cache_session_rate": cache_rates(records)["session_rate"]}


def main() -> None:
    """Recalcule les miss serveur et le taux évitable de chaque run de tâche d'un run."""
    for run in (Path(a) for a in sys.argv[1:]):
        print(f"# {run.name}")
        for task_dir in sorted(run.glob("tasks/*/run-*")):
            m = task_run_cache(task_dir)
            rate, session = m["cache_avoidable_rate"], m["cache_session_rate"]
            print(f"{task_dir.parent.name} {task_dir.name} : {len(m['server_misses'])} miss serveur,"
                  f" taux évitable {'null' if rate is None else f'{rate:.4f}'},"
                  f" taux de session {'null' if session is None else f'{session:.4f}'}")
            for x in m["server_misses"]:
                print(f"  seq {x['seq']} ({x['kind']}) : prompt {x['prompt']}, depuis l'appel "
                      f"précédent {x['since_prev_ms']} ms, transport {x['transport']}, "
                      f"continuation {x['continuation']}")


if __name__ == "__main__":
    main()
