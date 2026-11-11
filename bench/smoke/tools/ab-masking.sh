#!/usr/bin/env bash
# A/B masquage (T05) : 3 alternances défaut / masquage, gpt-6.1-terra, effort high,
# RUNS=1, les 10 tâches du banc. Appels réels au modèle (CLAUDE.md règle 6) : lancé par Yusuf.
# Usage : bash bench/smoke/tools/ab-masking.sh <dossier du binaire> [<sha de référence>]
# Avec un sha de référence (par exemple main) : vérifie que le binaire a les mêmes sources
# livrées, et l'écrit dans le journal de la boucle et dans chaque rapport.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

BIN_DIR=${1:?dossier du binaire (contient prime-agent et manifest.json)}
BIN="$BIN_DIR/prime-agent"
TASKS=node-filesize-376,node-koa-2029,node-little-coder-102,node-testsprite-356,node-tough-cookie-630,py-graphiti-2030,py-langextract-584,py-shortuuid-118,py-tldextract-368,py-tldextract-383
REF=${2:-}
LOG="bench/smoke/runs/ab-masking-$(date -u +%Y%m%dT%H%M%SZ).txt"
# Sources livrées : ce que le binaire embarque (chemins de changelog-fragment.yml, plus Cargo.*).
SHIPPED=(crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock)

# Binaire : présent, version et manifeste au sha du dossier.
test -x "$BIN" || { echo "binaire absent : $BIN"; exit 1; }
SHA=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["commit"])' "$BIN_DIR/manifest.json")
"$BIN" --version | grep -q "$SHA" || { echo "version du binaire ≠ manifeste $SHA"; exit 1; }
WANT=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["binaries"][0]["executableSha256"])' "$BIN_DIR/manifest.json")
[ "$(shasum -a 256 "$BIN" | cut -d' ' -f1)" = "$WANT" ] || { echo "sha256 de l'exécutable ≠ manifeste"; exit 1; }
case "$BIN_DIR" in *"$SHA"*) ;; *) echo "le dossier ne porte pas le sha $SHA"; exit 1;; esac
NOTE=""
if [ -n "$REF" ]; then
  git cat-file -e "$REF^{commit}" || { echo "sha de référence inconnu : $REF"; exit 1; }
  git diff --quiet "$SHA" "$REF" -- "${SHIPPED[@]}" \
    || { echo "sources livrées différentes entre $SHA et $REF"; exit 1; }
  NOTE="Binaire ${SHA:0:7} : sources livrées identiques à ${REF:0:7} (git diff $SHA $REF -- ${SHIPPED[*]} vide)."
fi

# Réglages : masking-on.json ne diffère de default.json que par le masquage.
python3 -I - <<'PY'
import json, sys
def flat(d, p=""):
    out = {}
    for k, v in d.items():
        out.update(flat(v, p + k + ".") if isinstance(v, dict) else {p + k: v})
    return out
a = flat(json.load(open("bench/smoke/settings/default.json")))
b = flat(json.load(open("bench/smoke/settings/masking-on.json")))
diff = {k for k in a.keys() | b.keys() if a.get(k) != b.get(k)}
if not diff or any(k.split(".")[0] != "masking" for k in diff):
    sys.exit(f"réglages : écart hors masquage ou aucun écart : {sorted(diff)}")
print("réglages : seul écart", sorted(diff))
PY

# La mention dans un rapport (un `run.py --rereport` réécrit report.md : ab-note.txt reste).
annotate() {
  [ -n "$NOTE" ] && [ -d "$1" ] || return 0
  echo "$NOTE" > "$1/ab-note.txt"
  printf '\n## Note de l'"'"'A/B masquage\n\n- %s\n' "$NOTE" >> "$1/report.md"
}

export DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short
echo "binaire $SHA, tâches $TASKS" | tee "$LOG"
[ -z "$NOTE" ] || echo "$NOTE" | tee -a "$LOG"
for pass in 1 2 3; do
  for variant in default masking-on; do
    echo "== passe $pass : $variant" | tee -a "$LOG"
    make smoke BINARY="$BIN" PROVIDER=openai-codex MODEL=gpt-6.1-terra EFFORT=high RUNS=1 \
      TASKS="$TASKS" SETTINGS="bench/smoke/settings/$variant.json" 2>&1 | tee -a "$LOG"
    annotate "$(grep -o 'rapport : .*/report.md' "$LOG" | tail -1 | sed 's|rapport : ||; s|/report.md||')"
  done
done
# Dossiers de run, dans l'ordre (défaut, masquage, défaut, ...), pour compare.py.
grep -o 'rapport : .*/report.md' "$LOG" | sed 's|rapport : ||; s|/report.md||' | tee -a "$LOG"
