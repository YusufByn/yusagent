#!/usr/bin/env python3
"""Relevé YA-11 (T03) : un secret retiré par unsetenv au démarrage reste-t-il visible
par `ps eww` depuis un process enfant (le kernel) ? Rien n'est implémenté côté harness.

Usage : python3 -I bench/tools/capture/unsetenv_ps.py

Un parent (rôle du harness) démarre avec YA_PS_SECRET dans son environnement,
puis, selon le mode :
- none     : ne fait rien ;
- unsetenv : unsetenv(3) de la libc (ce que fait `std::env::remove_var` en Rust) ;
- zero     : écrase la valeur en place dans la zone d'environnement initiale, puis unsetenv.
Il lance ensuite un enfant (rôle du kernel) avec un environnement sans la variable,
qui lit l'environnement du parent avec `ps eww -o command= -p <ppid>`.
Valeur factice, aucun vrai secret.
"""

from __future__ import annotations

import ctypes
import os
import subprocess
import sys

NAME = "YA_PS_SECRET"
VALUE = "fake-ps-value-123"

CHILD = (
    "import os, subprocess\n"
    "r = subprocess.run(['/bin/ps', 'eww', '-o', 'command=', '-p', str(os.getppid())],\n"
    "                   capture_output=True, text=True)\n"
    f"print('enfant : variable dans son env', {NAME!r} in os.environ, '| ps exit', r.returncode,\n"
    f"      '| nom visible', {NAME + '='!r} in r.stdout, '| valeur visible', {VALUE!r} in r.stdout)\n"
)


def parent(mode: str) -> None:
    libc = ctypes.CDLL(None)
    libc.getenv.restype = ctypes.c_void_p
    if mode == "zero":
        pointer = libc.getenv(NAME.encode())
        ctypes.memset(pointer, ord("x"), len(os.environb[NAME.encode()]))
    if mode in ("unsetenv", "zero"):
        assert libc.unsetenv(NAME.encode()) == 0
        os.environ.pop(NAME, None)
    print(f"parent ({mode}) : getenv après le mode = {libc.getenv(NAME.encode()) is not None}",
          flush=True)
    env = {k: v for k, v in os.environ.items() if k != NAME}
    subprocess.run([sys.executable, "-I", "-c", CHILD], env=env, check=True)


def main() -> None:
    if len(sys.argv) > 1:
        parent(sys.argv[1])
        return
    for mode in ("none", "unsetenv", "zero"):
        subprocess.run(["/usr/bin/env", "-i", "PATH=/usr/bin:/bin", f"{NAME}={VALUE}",
                        sys.executable, "-I", __file__, mode], check=True)


if __name__ == "__main__":
    main()
