#!/usr/bin/env python3
"""Ticket 43, PR 3 (YA-18) : mesures préalables au plan de code, git seul, hors yusAgent.

Sur un vrai workspace, sans jamais l'écrire (git n'y fait que lire ; dépôt shadow et index
dans <scratch>) :
  1. inventaire des fichiers candidats : `.gitignore` respecté, puis liste par défaut seule
     (cas « projet sans .gitignore ») ;
  2. instantanés dans un dépôt shadow (froid, puis sans changement), sans et avec un profil
     Seatbelt reconstitué d'après `crates/pa-types/src/platform/ya_guard.rs:278-327` ;
  3. sur une copie des fichiers candidats : tour simulé (50 modifiés, 20 créés, 10 supprimés),
     contrôle « modifié hors de l'agent » (instantané temporaire + diff-tree), restauration
     (`read-tree -u --reset`) et vérification ;
  4. profil : variante « kernel » (écriture du dossier des instantanés refusée) contre variante
     « git des instantanés » ; faux dossier agent dans le workspace, avec et sans exclusion ;
  5. index d'une nouvelle session : neuf, ou copié de celui d'une autre session.

N'appelle aucun modèle. Usage :
  python3 -I docs/tickets/43-retour-arriere/shadow_git_workspace.py <workspace> <dossier vide> [répétitions]
"""

import os
import random
import shutil
import statistics
import subprocess
import sys
import time

WS = os.path.realpath(sys.argv[1])
ROOT = os.path.realpath(sys.argv[2])
REPS = int(sys.argv[3]) if len(sys.argv) > 3 else 5
os.makedirs(ROOT, exist_ok=True)
assert not os.listdir(ROOT), "le dossier de travail doit être vide"

# Liste par défaut proposée (projet sans .gitignore à la racine).
DEFAULT_EXCLUDES = [
    ".git/", "node_modules/", "target/", ".venv/", "venv/", "__pycache__/", ".mypy_cache/",
    ".pytest_cache/", ".ruff_cache/", ".tox/", ".gradle/", ".next/", ".nuxt/", ".cache/",
    "dist/", "build/", "coverage/", ".DS_Store", "*.pyc",
]
DEFAULT_DIRS = {e.rstrip("/") for e in DEFAULT_EXCLUDES if e.endswith("/")}

AGENT = os.path.join(ROOT, "agent")
CHECKPOINTS = os.path.join(AGENT, "checkpoints")
os.makedirs(CHECKPOINTS)
SENTINEL = "ya43-sentinel-auth-5f1c"
with open(os.path.join(AGENT, "auth.json"), "w") as f:
    f.write('{"key": "%s"}\n' % SENTINEL)


def regex(path):
    out = ""
    for ch in path:
        if ch in "\\.^$|()[]{}*+?":
            out += "\\"
        out += ch
    return '(regex #"^%s")' % out


def profile(agent, kernel):
    """Profil `Full` de ya_guard.rs:278-327 (sockets et binaire factices) ; `kernel` ajoute le
    refus d'écriture du dossier des instantanés proposé à la réponse 8."""
    sock = os.path.join(ROOT, "sock")
    binary = os.path.join(ROOT, "bin", "yusagent")
    text = "(version 1)\n(allow default)\n"
    text += "(deny file-read* file-write* %s %s)\n" % (
        regex(os.path.join(agent, "auth.json")), regex(os.path.join(agent, "models.json")))
    text += '(deny file-write-unlink (literal "%s") (literal "%s") (literal "%s"))\n' % (
        agent, os.path.dirname(agent), os.path.dirname(binary))
    text += '(deny network-outbound (remote unix-socket (subpath "%s")))\n' % sock
    text += '(deny file-write* (subpath "%s") (literal "%s") (literal "%s"))\n' % (
        sock, binary, os.path.join(os.path.dirname(binary), "package.json"))
    text += ('(deny mach-lookup (global-name-prefix "com.apple.coreservices.") '
             '(global-name-prefix "com.apple.lsd."))\n')
    if kernel:
        text += '(deny file-write* (subpath "%s"))\n' % os.path.join(agent, "checkpoints")
    text += '(allow process-exec (literal "/bin/ps") (literal "/usr/bin/top") (with no-sandbox))\n'
    return text


GIT_ENV = {
    "HOME": os.environ.get("HOME", "/"),
    "PATH": "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_ALLOW_PROTOCOL": "",
    "GIT_NO_LAZY_FETCH": "1",
    "GIT_AUTHOR_NAME": "yusagent", "GIT_AUTHOR_EMAIL": "checkpoints@yusagent.invalid",
    "GIT_COMMITTER_NAME": "yusagent", "GIT_COMMITTER_EMAIL": "checkpoints@yusagent.invalid",
}
GIT_OVERRIDES = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "gc.auto=0",
                 "-c", "core.autocrlf=false", "-c", "core.symlinks=true"]


class Shadow:
    def __init__(self, name, work, guard, agent=AGENT):
        self.dir = os.path.join(CHECKPOINTS, name)
        self.work = work
        self.guard = profile(agent, kernel=False) if guard else None
        self.index = os.path.join(self.dir, "ya-index", "s1")
        subprocess.run(["git", "init", "-q", "--bare", self.dir], check=True, env=GIT_ENV)
        os.makedirs(os.path.dirname(self.index), exist_ok=True)

    def git(self, *args, stdin=None, check=True):
        env = dict(GIT_ENV, GIT_INDEX_FILE=self.index)
        cmd = ["git", "--git-dir=" + self.dir, "--work-tree=" + self.work, *GIT_OVERRIDES, *args]
        if self.guard:
            cmd = ["/usr/bin/sandbox-exec", "-p", self.guard, *cmd]
        r = subprocess.run(cmd, input=stdin, capture_output=True, text=True, env=env)
        if check and r.returncode != 0:
            raise RuntimeError(f"{' '.join(args)}: {r.stderr.strip()}")
        return r

    def snapshot(self, ref):
        self.git("add", "-A")
        tree = self.git("write-tree").stdout.strip()
        commit = self.git("commit-tree", tree, stdin=ref + "\n").stdout.strip()
        self.git("update-ref", "refs/ya/s1/" + ref, commit)
        return tree

    def size_kb(self):
        return int(subprocess.run(["du", "-sk", self.dir], capture_output=True, text=True).stdout.split()[0])


def timed(fn, *args):
    t = time.monotonic()
    out = fn(*args)
    return round((time.monotonic() - t) * 1000), out


def inventory_default(root):
    count = size = 0
    big = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in DEFAULT_DIRS]
        for name in filenames:
            if name == ".DS_Store" or name.endswith(".pyc"):
                continue
            p = os.path.join(dirpath, name)
            try:
                st = os.lstat(p)
            except OSError:
                continue
            count += 1
            size += st.st_size
            big.append(st.st_size)
    big.sort()
    return count, size, big


def quant(sizes):
    n = len(sizes)
    return {k: sizes[min(n - 1, int(n * q))] for k, q in (("p50", .5), ("p99", .99), ("p99.9", .999))} | {"max": sizes[-1]}


print(f"workspace {WS} ; git {subprocess.run(['git', '--version'], capture_output=True, text=True).stdout.split()[-1]} ; répétitions {REPS}")

# 1. Inventaire.
ls = subprocess.run(["git", "-C", WS, "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                    capture_output=True, check=True).stdout.split(b"\0")
cand = [p.decode() for p in ls if p and os.path.isfile(os.path.join(WS, p.decode()))]
sizes = sorted(os.lstat(os.path.join(WS, p)).st_size for p in cand)
print(f"[1] .gitignore respecté : {len(cand)} fichiers, {sum(sizes) / 1048576:.1f} Mio, {quant(sizes)}")
c, s, b = inventory_default(WS)
print(f"[1] liste par défaut seule (sans .gitignore) : {c} fichiers, {s / 1048576:.1f} Mio, {quant(b)} ; "
      f"> 8 Mio : {sum(1 for x in b if x > 8 << 20)} ; > 64 Mio : {sum(1 for x in b if x > 64 << 20)}")

# 2. Instantanés sur le workspace réel (lecture seule pour lui).
for guard in (False, True):
    label = "avec profil" if guard else "sans profil"
    sh = Shadow("ws-guard" if guard else "ws-plain", WS, guard)
    cold, _ = timed(sh.snapshot, "froid")
    warm = [timed(sh.snapshot, f"t{i}")[0] for i in range(REPS)]
    print(f"[2] workspace réel, {label} : premier instantané {cold} ms ; sans changement {warm} ms "
          f"(médiane {statistics.median(warm)}) ; dépôt shadow {sh.size_kb()} Kio")

# 3. Tour simulé, contrôle et restauration, sur une copie des candidats.
COPY = os.path.join(ROOT, "copy")
for p in cand:
    dst = os.path.join(COPY, p)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copy2(os.path.join(WS, p), dst)
rng = random.Random(43)
for guard in (False, True):
    label = "avec profil" if guard else "sans profil"
    sh = Shadow("copy-guard" if guard else "copy-plain", COPY, guard)
    cold, _ = timed(sh.snapshot, "froid")
    rows = []
    for r in range(1, REPS + 1):
        files = sorted(os.path.relpath(os.path.join(d, f), COPY) for d, _, fs in os.walk(COPY) for f in fs)
        t_start, start = timed(sh.snapshot, f"{r}/debut")
        for p in rng.sample(files, 50):
            with open(os.path.join(COPY, p), "a") as f:
                f.write(f"\nedit {r}\n")
        for i in range(20):
            with open(os.path.join(COPY, f"ya43_new_{r}_{i}.txt"), "w") as f:
                f.write("new\n")
        for p in rng.sample(files, 10):
            os.remove(os.path.join(COPY, p))
        t_end, end = timed(sh.snapshot, f"{r}/fin")
        # Modification hors de l'agent, puis contrôle : instantané temporaire et diff-tree.
        with open(os.path.join(COPY, f"ya43_user_{r}.txt"), "w") as f:
            f.write("user\n")

        def check():
            now_tree = sh.snapshot(f"{r}/avant-restauration")
            return sh.git("diff-tree", "-r", "--name-only", end, now_tree).stdout.split()

        t_check, changed = timed(check)
        before_restore = f"refs/ya/s1/{r}/avant-restauration^{{tree}}"
        t_prev, preview = timed(lambda: sh.git("diff-tree", "-r", "--name-status", start,
                                               before_restore).stdout.splitlines())
        t_rest, _ = timed(lambda: sh.git("read-tree", "-u", "--reset", start))
        left = sh.snapshot(f"{r}/verif")
        diff = sh.git("diff-tree", "-r", "--name-only", start, left).stdout.split()
        rows.append((t_start, t_end, t_check, len(changed), t_prev, len(preview), t_rest, len(diff)))
        print(f"[3] copie, {label}, rép. {r} : instantané début {t_start} ms ; fin {t_end} ms ; "
              f"contrôle {t_check} ms ({len(changed)} fichier(s) hors agent) ; aperçu {t_prev} ms "
              f"({len(preview)} lignes) ; restauration {t_rest} ms ; écart restant {len(diff)} "
              f"(le fichier de l'utilisateur, non suivi au début, est retiré : {diff})")
    print(f"[3] copie, {label} : premier instantané {cold} ms ; dépôt shadow {sh.size_kb()} Kio")

# 4. Profil : variante kernel contre variante git, faux dossier agent dans le workspace.
probe = os.path.join(CHECKPOINTS, "probe")
for kernel in (True, False):
    r = subprocess.run(["/usr/bin/sandbox-exec", "-p", profile(AGENT, kernel), "/usr/bin/touch", probe],
                       capture_output=True, text=True)
    print(f"[4] écriture de checkpoints/probe, variante {'kernel' if kernel else 'git des instantanés'} : "
          f"code {r.returncode} {r.stderr.strip()}")
    if os.path.exists(probe):
        os.remove(probe)
WS2 = os.path.join(ROOT, "ws-with-agent")
FAKE = os.path.join(WS2, ".yusagent")
os.makedirs(FAKE)
with open(os.path.join(WS2, "code.py"), "w") as f:
    f.write("print(1)\n")
with open(os.path.join(FAKE, "auth.json"), "w") as f:
    f.write('{"key": "%s"}\n' % SENTINEL)
with open(os.path.join(FAKE, "settings.json"), "w") as f:
    f.write("{}\n")
for excluded in (False, True):
    sh = Shadow("agent-excl" if excluded else "agent-raw", WS2, guard=True, agent=FAKE)
    if excluded:
        with open(os.path.join(sh.dir, "info", "exclude"), "a") as f:
            f.write("/.yusagent/\n")
    r = sh.git("add", "-A", check=False)
    blobs = sh.git("cat-file", "--batch-all-objects", "--batch").stdout if r.returncode == 0 else ""
    print(f"[4] faux dossier agent dans le workspace, {'exclu' if excluded else 'non exclu'} : add code {r.returncode} "
          f"{r.stderr.strip()[:160]!r} ; secret dans les objets : {SENTINEL in blobs} ; "
          f"index : {sh.git('ls-files').stdout.split()}")

# 5. Index d'une nouvelle session : neuf, ou copié de l'index d'une autre session du workspace.
sh = Shadow("ws-plain", WS, False)  # dépôt déjà rempli à l'étape 2 (git init réutilise le dossier)
for i in range(3):
    seeded = os.path.join(sh.dir, "ya-index", f"copie{i}")
    shutil.copy2(os.path.join(sh.dir, "ya-index", "s1"), seeded)
    t_seed, _ = timed(lambda: subprocess.run(["git", "--git-dir=" + sh.dir, "--work-tree=" + WS, *GIT_OVERRIDES, "add", "-A"],
                                             check=True, env=dict(GIT_ENV, GIT_INDEX_FILE=seeded)))
    fresh = os.path.join(sh.dir, "ya-index", f"neuf{i}")
    t_fresh, _ = timed(lambda: subprocess.run(["git", "--git-dir=" + sh.dir, "--work-tree=" + WS, *GIT_OVERRIDES, "add", "-A"],
                                              check=True, env=dict(GIT_ENV, GIT_INDEX_FILE=fresh)))
    print(f"[5] nouvelle session, essai {i + 1} : index copié {t_seed} ms ; index neuf (objets déjà dans le dépôt) {t_fresh} ms")
