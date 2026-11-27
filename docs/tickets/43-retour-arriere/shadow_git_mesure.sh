#!/bin/bash
# Mesure préliminaire (ticket 43, critère « < 2 s sur 10 000 fichiers ») : git seul, hors yusAgent.
# Dépôt shadow (GIT_DIR hors du workspace, --work-tree = workspace), instantané par tour,
# restauration d'un tour, contrôle que le dépôt de l'utilisateur n'a ni commit, ni branche, ni stash nouveaux.
# Usage : bash docs/tickets/43-retour-arriere/shadow_git_mesure.sh <dossier vide> [nombre de fichiers] [répétitions]
set -euo pipefail
ROOT=${1:?dossier}
N=${2:-10000}
REPS=${3:-5}
mkdir -p "$ROOT"
W="$ROOT/workspace"; S="$ROOT/shadow.git"
rm -rf "$W" "$S"; mkdir -p "$W"
now() { python3 -c 'import time; print(time.monotonic())'; }
ms() { python3 -c "print(round(($2-$1)*1000))"; }
sg() { git --git-dir="$S" --work-tree="$W" -c core.fsmonitor=false -c gc.auto=0 "$@"; }

# Workspace : N fichiers d'environ 1 Ko dans 100 dossiers, dépôt git de l'utilisateur avec un commit.
python3 - "$W" "$N" <<'PY'
import os, sys
w, n = sys.argv[1], int(sys.argv[2])
line = "x" * 63 + "\n"
for i in range(n):
    d = os.path.join(w, f"d{i % 100:03d}")
    os.makedirs(d, exist_ok=True)
    with open(os.path.join(d, f"f{i:05d}.txt"), "w") as f:
        f.write(f"file {i}\n" + line * 16)
PY
git -C "$W" init -q
git -C "$W" -c user.name=u -c user.email=u@example.invalid add -A
git -C "$W" -c user.name=u -c user.email=u@example.invalid commit -q -m user
user_state() { echo "commits=$(git -C "$W" rev-list --all --count) refs=$(git -C "$W" for-each-ref | wc -l | tr -d ' ') stash=$(git -C "$W" stash list | wc -l | tr -d ' ') status=$(git -C "$W" status --porcelain | wc -l | tr -d ' ')"; }
BEFORE=$(user_state)

git init -q --bare "$S"
snap() { # $1 = nom du tour
  sg add -A
  local tree; tree=$(sg write-tree)
  sg update-ref "refs/ya/$1" "$(echo "$1" | sg -c user.name=ya -c user.email=ya@example.invalid commit-tree "$tree")"
}
t0=$(now); snap t0; t1=$(now); echo "instantané initial (froid) : $(ms "$t0" "$t1") ms"

agent_turn() { # 50 modifiés, 20 créés, 10 supprimés
  python3 - "$W" "$1" <<'PY'
import os, sys
w, k = sys.argv[1], int(sys.argv[2])
for i in range(50):
    with open(os.path.join(w, f"d{i % 100:03d}", f"f{i:05d}.txt"), "a") as f: f.write(f"edit {k}\n")
for i in range(20):
    with open(os.path.join(w, "d000", f"new{k}_{i}.txt"), "w") as f: f.write("new\n")
for i in range(10):
    p = os.path.join(w, f"d{(i+50) % 100:03d}", f"f{1000*k + i + 50:05d}.txt")
    if os.path.exists(p): os.remove(p)
PY
}

for r in $(seq 1 "$REPS"); do
  t0=$(now); snap "idle$r"; t1=$(now)
  agent_turn "$r"
  t2=$(now); snap "a$r"; t3=$(now)
  # Fichiers changés depuis l'instantané du tour (contrôle « modifié par l'utilisateur »).
  t4=$(now); changed=$(sg diff-index --name-only "refs/ya/a$r" -- | wc -l | tr -d ' '); t5=$(now)
  # Restauration du tour : retour à l'instantané d'avant le tour de l'agent.
  t6=$(now); sg read-tree -u --reset "refs/ya/idle$r"; t7=$(now)
  left=$(sg diff-index --name-only "refs/ya/idle$r" -- | wc -l | tr -d ' ')
  untracked=$(sg ls-files --others --exclude-standard | wc -l | tr -d ' ')
  echo "rép. $r : instantané sans changement $(ms "$t0" "$t1") ms ; instantané après le tour $(ms "$t2" "$t3") ms ; contrôle des changements $(ms "$t4" "$t5") ms ($changed fichiers) ; restauration $(ms "$t6" "$t7") ms (écarts restants $left, non suivis $untracked)"
done
AFTER=$(user_state)
echo "dépôt de l'utilisateur avant : $BEFORE"
echo "dépôt de l'utilisateur après : $AFTER"
echo "taille du dépôt shadow : $(du -sk "$S" | cut -f1) Ko ; git $(git --version | cut -d' ' -f3)"
