#!/usr/bin/env python3
"""Ticket 43, PR 3a (YA-18) : coût du listage des candidats et de leur `lstat` (plafonds de
workspace : 20 000 fichiers, 512 Mio). Lecture seule du workspace ; sept répétitions.

Usage : python3 -I docs/tickets/43-retour-arriere/listing_cost.py <workspace>
"""
import os, statistics, subprocess, sys, time
ws = os.path.realpath(sys.argv[1])
env = {"HOME": os.environ["HOME"], "PATH": "/usr/bin:/bin", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null"}
rows = []
for _ in range(7):
    t0 = time.monotonic()
    out = subprocess.run(["git", "-C", ws, "-c", "core.fsmonitor=false", "-c", "core.excludesFile=/dev/null", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], capture_output=True, check=True, env=env).stdout
    t1 = time.monotonic()
    paths = [p for p in out.split(b"\0") if p]
    total = 0
    for p in paths:
        try:
            total += os.lstat(os.path.join(ws.encode(), p)).st_size
        except OSError:
            pass
    t2 = time.monotonic()
    rows.append((round((t1 - t0) * 1000), round((t2 - t1) * 1000)))
print(f"{len(paths)} chemins, {total / 1048576:.1f} Mio ; ls-files ms {[r[0] for r in rows]} (médiane {statistics.median(r[0] for r in rows)}) ; lstat ms {[r[1] for r in rows]} (médiane {statistics.median(r[1] for r in rows)})")
