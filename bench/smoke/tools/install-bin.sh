#!/usr/bin/env bash
# Installe le binaire d'un run CI (ya-build-macos-arm64 ou continuous) dans
# ~/.cache/yusagent-bin/<sha du commit>/, sommes vérifiées. Affiche le dossier.
# Dossier déjà présent : s'il passe les contrôles, rien n'est modifié ; sinon
# erreur, et --force le remplace, une fois la nouvelle copie vérifiée à part.
# Usage : bash bench/smoke/tools/install-bin.sh [--force] <run id> [<sha attendu>]
set -euo pipefail
FORCE=0
if [ "${1:-}" = --force ]; then FORCE=1; shift; fi
RUN=${1:?id du run CI}
EXPECTED=${2:-}
REPO=YusufByn/yusagent
# Cible de la machine : seul darwin arm64 est installable (archive prime-agent-*-darwin-arm64).
[ "$(uname -s)/$(uname -m)" = "Darwin/arm64" ] || { echo "cible non prise en charge : $(uname -s)/$(uname -m)" >&2; exit 1; }
TARGET=aarch64-apple-darwin
# Artefact de la cible, seul : artifacts-<cible> (continuous) ou prime-agent-<cible>-<sha> (ya-build-macos-arm64).
NAMES=$(gh api "repos/$REPO/actions/runs/$RUN/artifacts?per_page=100" \
  --jq ".artifacts[] | select(.expired | not) | .name | select(. == \"artifacts-$TARGET\" or startswith(\"prime-agent-$TARGET-\"))")
[ -n "$NAMES" ] || { echo "run $RUN : pas d'artefact $TARGET (absent ou expiré)" >&2; exit 1; }
[ "$(printf '%s\n' "$NAMES" | wc -l)" -eq 1 ] || { echo "run $RUN : plusieurs artefacts $TARGET : $NAMES" >&2; exit 1; }
TMP=$(mktemp -d)
STAGE=
OLD=
trap 'rm -rf "$TMP" ${STAGE:+"$STAGE"} ${OLD:+"$OLD"}' EXIT
gh run download "$RUN" --repo "$REPO" -n "$NAMES" --dir "$TMP" >/dev/null
ART=$TMP
[ -f "$ART/SHA256SUMS" ] && [ -f "$ART/manifest.json" ] || { echo "artefact $NAMES : SHA256SUMS ou manifest.json absent" >&2; exit 1; }
(cd "$ART" && shasum -a 256 -c SHA256SUMS) >&2
field() { python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1])); print(m["commit"] if sys.argv[2]=="commit" else m["binaries"][0]["executableSha256"])' "$1" "$2" 2>/dev/null; }
SHA=$(field "$ART/manifest.json" commit)
[ -n "$SHA" ] || { echo "artefact $NAMES : commit absent du manifeste" >&2; exit 1; }
[ -z "$EXPECTED" ] || [ "$SHA" = "$EXPECTED" ] || { echo "commit $SHA ≠ attendu $EXPECTED" >&2; exit 1; }
# Contrôles d'un dossier installé : fichiers présents, commit du manifeste,
# sha256 de l'exécutable, --version.
check() {
  local d=$1 want got
  [ -f "$d/prime-agent" ] && [ -f "$d/manifest.json" ] && [ -f "$d/SHA256SUMS" ] || { echo "$d : prime-agent, manifest.json ou SHA256SUMS absent" >&2; return 1; }
  got=$(field "$d/manifest.json" commit)
  [ "$got" = "$SHA" ] || { echo "$d : commit du manifeste '$got' ≠ $SHA" >&2; return 1; }
  want=$(field "$d/manifest.json" exe)
  got=$(shasum -a 256 "$d/prime-agent" | cut -d' ' -f1)
  [ -n "$want" ] && [ "$want" = "$got" ] || { echo "$d : sha256 de l'exécutable $got ≠ manifeste $want" >&2; return 1; }
  "$d/prime-agent" --version 2>/dev/null | grep -q "$SHA" || { echo "$d : version ≠ $SHA" >&2; return 1; }
}
BASE="$HOME/.cache/yusagent-bin"
DEST="$BASE/$SHA"
if [ -e "$DEST" ] && [ "$FORCE" = 0 ]; then
  if check "$DEST"; then
    echo "déjà installé, contrôles passés, rien de modifié : $DEST" >&2
    echo "$DEST"
    exit 0
  fi
  echo "$DEST existe et échoue aux contrôles ; relancer avec --force pour le remplacer" >&2
  exit 1
fi
# Copie extraite et vérifiée à côté (même volume), puis substituée à DEST.
mkdir -p "$BASE"
STAGE=$(mktemp -d "$BASE/.$SHA.XXXXXX")
chmod 755 "$STAGE"
tar -xzf "$ART"/prime-agent-*-darwin-arm64.tar.gz -C "$STAGE"
cp "$ART/manifest.json" "$ART/SHA256SUMS" "$STAGE/"
check "$STAGE" || { echo "copie extraite invalide, $DEST inchangé" >&2; exit 1; }
if [ -e "$DEST" ]; then
  OLD="$BASE/.$SHA.old.$$"
  mv "$DEST" "$OLD"
  mv "$STAGE" "$DEST" || { mv "$OLD" "$DEST"; OLD=; echo "remplacement échoué, $DEST restauré" >&2; exit 1; }
else
  mv "$STAGE" "$DEST"
fi
STAGE=
echo "$DEST"
