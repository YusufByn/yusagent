#!/usr/bin/env python3
"""Ticket 41 : mesures macOS du constat (Seatbelt, environnement et mémoire du parent, socket).

Lancer : `python3 -I docs/tickets/41-protection-secrets/mesure_macos.py`
Tout se passe dans un dossier temporaire jetable, avec des secrets factices. Aucun fichier de
`~/.yusagent` n'est lu, aucun modèle n'est appelé. macOS seulement (`/usr/bin/sandbox-exec`).

Chaque ligne : nom de l'essai, attendu, obtenu, `ok` si l'obtenu est l'attendu.
"""

from __future__ import annotations

import ctypes
import os
import re
import socket
import subprocess
import sys
import tempfile
from pathlib import Path

SB = "/usr/bin/sandbox-exec"
PY = "/usr/bin/python3"
ROWS: list[tuple[str, str, str]] = []


def row(name: str, expected: str, got: str) -> None:
    ROWS.append((name, expected, got))


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, timeout=30, **kw)


def denied(proc: subprocess.CompletedProcess) -> str:
    err = proc.stderr + proc.stdout
    if proc.returncode != 0 and ("Operation not permitted" in err or "PermissionError" in err):
        return "refusé"
    return f"permis (rc={proc.returncode})"


def rx(path: Path) -> str:
    return re.escape(str(path)).replace('"', '\\"')


def profile(agent: Path, home: Path, *, mode: str = "regex", ancestors: bool = True,
            sockdir: Path | None = None) -> str:
    """mode : `regex` (préfixe auth.json* et models.json*), `literal` (auth.json seul)."""
    if mode == "regex":
        deny = (f'(regex #"^{rx(agent / "auth.json")}") '
                f'(regex #"^{rx(agent / "models.json")}")')
    else:
        deny = f'(literal "{agent / "auth.json"}")'
    text = f"(version 1)(allow default)\n(deny file-read* file-write* {deny})\n"
    if ancestors:
        text += f'(deny file-write-unlink (literal "{agent}") (literal "{home}"))\n'
    if sockdir is not None:
        text += (f'(deny network-outbound (remote unix-socket (subpath "{sockdir}")))\n'
                 f'(deny file-write* (subpath "{sockdir}"))\n')
    return text


def setup(base: Path) -> tuple[Path, Path]:
    home = base / "home"
    agent = home / ".ya"
    (agent / "kernel-venv").mkdir(parents=True)
    (agent / "auth.json").write_text("SECRET-1\n")
    (agent / "models.json").write_text('{"providers":{}}\n')
    (agent / "settings.json").write_text("{}\n")
    (agent / "kernel-venv" / "f").write_text("ok\n")
    return home.resolve(), agent.resolve()


def seatbelt(base: Path) -> None:
    home, agent = setup(base / "regex")
    prof = profile(agent, home)
    sb = [SB, "-p", prof]
    auth, models = agent / "auth.json", agent / "models.json"
    row("cat auth.json", "refusé", denied(run(sb + ["/bin/cat", str(auth)])))
    row("python open(auth.json)", "refusé", denied(run(sb + [PY, "-I", "-c", f"open({str(auth)!r}).read()"])))
    row("cat models.json", "refusé", denied(run(sb + ["/bin/cat", str(models)])))
    row("lire kernel-venv/f", "permis (rc=0)", denied(run(sb + ["/bin/cat", str(agent / "kernel-venv" / "f")])))
    row("écrire settings.json", "permis (rc=0)", denied(run(sb + ["/bin/sh", "-c", f"echo '{{}}' > {agent}/settings.json"])))
    row("créer un fichier dans le dossier", "permis (rc=0)", denied(run(sb + ["/usr/bin/touch", str(agent / "autre")])))
    row("planter auth.json.tmp123", "refusé", denied(run(sb + ["/bin/sh", "-c", f"echo x > {agent}/auth.json.tmp123"])))
    row("créer le verrou auth.json.lock", "refusé", denied(run(sb + ["/bin/mkdir", str(agent / "auth.json.lock")])))
    row("remplacer models.json par renommage", "refusé",
        denied(run(sb + ["/bin/sh", "-c", f"echo '{{}}' > {agent}/m.tmp && mv {agent}/m.tmp {models}"])))
    row("renommer le dossier agent", "refusé", denied(run(sb + ["/bin/mv", str(agent), str(home / ".yb")])))
    row("renommer son parent", "refusé", denied(run(sb + ["/bin/mv", str(home), str(home.parent / "home2")])))
    row("lien symbolique puis cat", "refusé",
        denied(run(sb + ["/bin/sh", "-c", f"ln -s {auth} {home}/l && cat {home}/l"])))
    row("lien physique créé dans la sandbox", "refusé", denied(run(sb + ["/bin/ln", str(auth), str(home / "hard")])))
    for variant in (str(auth).replace("auth.json", "AUTH.JSON"), f"{agent}/../.ya/auth.json",
                    f"/System/Volumes/Data{auth}"):
        row(f"variante de chemin {variant.replace(str(base), '…')}", "refusé", denied(run(sb + ["/bin/cat", variant])))
    st = os.stat(auth)
    row("accès par inode /.vol/<dev>/<ino>", "refusé", denied(run(sb + ["/bin/cat", f"/.vol/{st.st_dev}/{st.st_ino}"])))
    # Écriture atomique du démon : fichier temporaire puis renommage (settings/storage.rs:291, :304).
    tmp = agent / f"auth.json.tmp{os.getpid()}"
    tmp.write_text("SECRET-2\n")
    row("pendant l'écriture : cat du temporaire", "refusé", denied(run(sb + ["/bin/cat", str(tmp)])))
    row("pendant l'écriture : lien physique du temporaire", "refusé", denied(run(sb + ["/bin/ln", str(tmp), str(home / "vole")])))
    os.rename(tmp, auth)
    row("après renommage : cat auth.json", "refusé", denied(run(sb + ["/bin/cat", str(auth)])))
    row("descendant : setsid + sh -c cat", "refusé",
        denied(run(sb + [PY, "-I", "-c",
                         "import os,subprocess,sys;os.setsid();"
                         f"r=subprocess.run(['/bin/sh','-c','cat {auth}'],capture_output=True,text=True);"
                         "sys.stderr.write(r.stderr);sys.exit(r.returncode)"])))
    row("sandbox imbriquée (sandbox-exec dans sandbox-exec)", "refusé (rc=71)",
        "refusé (rc=71)" if (p := run(sb + [SB, "-p", "(version 1)(allow default)", "/usr/bin/true"])).returncode == 71
        else f"rc={p.returncode} {p.stderr.strip()[:60]}")
    p = run(sb + ["/bin/ps", "-p", str(os.getpid())])
    row("exécuter /bin/ps (setuid)", "refusé", "refusé" if "Operation not permitted" in p.stderr else f"permis (rc={p.returncode})")
    # Exécution différée : fichiers qu'un processus non protégé exécutera plus tard (faux HOME).
    for rel in (".zshrc", "projet/.git/hooks/pre-commit", "Library/LaunchAgents/x.plist",
                "Library/Application Support/Code/User/settings.json"):
        target = home / rel
        got = run(sb + ["/bin/sh", "-c", f'mkdir -p "{target.parent}" && echo x >> "{target}"'])
        row(f"exécution différée : écrire ~/{rel}", "permis (rc=0)", denied(got))
    p = run([SB, "-p", prof + '(allow process-exec (literal "/bin/ps") (with no-sandbox))\n',
             "/bin/ps", "-p", str(os.getpid())])
    row("exception (with no-sandbox) pour /bin/ps", "permis (rc=0)", denied(p))
    row("contrôle positif : cat auth.json hors sandbox", "permis (rc=0)", denied(run(["/bin/cat", str(auth)])))

    # Variantes qui justifient la forme du profil.
    home, agent = setup(base / "literal")
    sb = [SB, "-p", profile(agent, home, mode="literal")]
    tmp = agent / "auth.json.tmp1"
    tmp.write_text("SECRET-3\n")
    got = run(sb + ["/bin/sh", "-c", f"ln {tmp} {home}/vole && cat {home}/vole"])
    row("literal : lien physique du temporaire pendant l'écriture", "permis (rc=0)", denied(got))
    home, agent = setup(base / "sans-ancetres")
    sb = [SB, "-p", profile(agent, home, ancestors=False)]
    got = run(sb + ["/bin/sh", "-c", f"mv {agent} {home}/.yb && cat {home}/.yb/auth.json"])
    row("sans règle sur les ancêtres : mv du dossier puis cat", "permis (rc=0)", denied(got))
    home, agent = setup(base / "lien-avant")
    os.link(agent / "auth.json", home / "lien-avant")
    sb = [SB, "-p", profile(agent, home)]
    row("lien physique créé AVANT, hors sandbox", "permis (rc=0)", denied(run(sb + ["/bin/cat", str(home / "lien-avant")])))


PROBE_ENV = r"""
import ctypes, sys
libc = ctypes.CDLL("/usr/lib/libSystem.dylib", use_errno=True)
pid = int(sys.argv[1])
mib = (ctypes.c_int * 3)(1, 49, pid)          # CTL_KERN, KERN_PROCARGS2
size = ctypes.c_size_t(1 << 20)
buf = ctypes.create_string_buffer(size.value)
if libc.sysctl(mib, 3, buf, ctypes.byref(size), None, 0) != 0:
    print("erreur", ctypes.get_errno()); sys.exit(0)
print("visible" if b"YA41_FAUX_SECRET=" in buf.raw[:size.value] else "absent")
"""

PROBE_TFP = r"""
import ctypes, sys
libc = ctypes.CDLL("/usr/lib/libSystem.dylib")
task = ctypes.c_uint(0)
kr = libc.task_for_pid(libc.mach_task_self(), int(sys.argv[1]), ctypes.byref(task))
print("accès" if kr == 0 else f"refusé (kr={kr})")
"""


def parent_probes(base: Path) -> None:
    """Le parent (non sandboxé) porte YA41_FAUX_SECRET dans son environnement initial ; l'enfant
    ne la reçoit pas et tente de la lire chez le parent."""
    probe_env = base / "probe_env.py"
    probe_env.write_text(PROBE_ENV)
    probe_tfp = base / "probe_tfp.py"
    probe_tfp.write_text(PROBE_TFP)
    parent = base / "parent.py"
    parent.write_text(
        "import os, subprocess, sys\n"
        "pre = sys.argv[1:]\n"
        "env = {k: v for k, v in os.environ.items() if k != 'YA41_FAUX_SECRET'}\n"
        "me = str(os.getpid())\n"
        f"r = subprocess.run(pre + ['{PY}', '-I', '{probe_env}', me], capture_output=True, text=True, env=env)\n"
        "print('sysctl', r.stdout.strip())\n"
        "r = subprocess.run(pre + ['/bin/ps', 'eww', '-o', 'command=', '-p', me], capture_output=True, text=True, env=env)\n"
        "print('ps', 'visible' if 'YA41_FAUX_SECRET=' in r.stdout else ('refusé' if 'not permitted' in r.stderr else 'absent'))\n"
        f"r = subprocess.run(pre + ['{PY}', '-I', '{probe_tfp}', me], capture_output=True, text=True, env=env)\n"
        "print('tfp', r.stdout.strip())\n"
    )
    for label, pre in (("hors sandbox", []), ("sous sandbox", [SB, "-p", "(version 1)(allow default)"])):
        out = run([PY, "-I", str(parent), *pre], env={**os.environ, "YA41_FAUX_SECRET": "x"}).stdout
        got = dict(line.split(" ", 1) for line in out.strip().splitlines())
        row(f"{label} : env du parent par sysctl KERN_PROCARGS2", "visible", got.get("sysctl", "?"))
        row(f"{label} : env du parent par ps eww", "visible" if not pre else "refusé", got.get("ps", "?"))
        row(f"{label} : mémoire du parent par task_for_pid", "refusé (kr=5)", got.get("tfp", "?"))
    out = run([PY, "-I", str(parent), SB, "-p",
               "(version 1)(allow default)(deny process-info* (target others))"],
              env={**os.environ, "YA41_FAUX_SECRET": "x"}).stdout
    row("(deny process-info* (target others)) : env par sysctl", "visible",
        dict(line.split(" ", 1) for line in out.strip().splitlines()).get("sysctl", "?"))


def socket_probe(base: Path) -> None:
    (base / "sock").mkdir()
    sockdir = (base / "sock").resolve()
    path = sockdir / "daemon.sock"
    server = socket.socket(socket.AF_UNIX)
    server.bind(str(path))
    server.listen(4)
    client = ("import socket,sys\ns=socket.socket(socket.AF_UNIX)\n"
              f"try:\n s.connect({str(path)!r}); print('permis')\n"
              "except OSError as e: print('refusé' if e.errno==1 else e)\n")
    home, agent = setup(base / "sockprof")
    prof = profile(agent, home, sockdir=sockdir)
    row("connexion au socket hors sandbox", "permis", run([PY, "-I", "-c", client]).stdout.strip())
    row("connexion au socket sous le profil", "refusé", run([SB, "-p", prof, PY, "-I", "-c", client]).stdout.strip())
    bind = ("import socket\ns=socket.socket(socket.AF_UNIX)\n"
            f"try:\n s.bind({str(sockdir / 'faux.sock')!r}); print('permis')\n"
            "except OSError as e: print('refusé' if e.errno==1 else e)\n")
    row("faux socket dans le dossier des sockets", "refusé", run([SB, "-p", prof, PY, "-I", "-c", bind]).stdout.strip())
    server.close()


def main() -> int:
    if sys.platform != "darwin":
        print("macOS seulement")
        return 2
    print(run(["/usr/bin/sw_vers"]).stdout.strip().replace("\n", " ; "))
    with tempfile.TemporaryDirectory(prefix="ya41-") as tmp:
        base = Path(tmp).resolve()
        seatbelt(base)
        parent_probes(base)
        socket_probe(base)
    width = max(len(r[0]) for r in ROWS)
    bad = 0
    for name, expected, got in ROWS:
        ok = got == expected
        bad += not ok
        print(f"{'ok ' if ok else 'ÉCART'} {name:<{width}}  attendu : {expected:<15} obtenu : {got}")
    print(f"{len(ROWS)} essais, {bad} écart(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
