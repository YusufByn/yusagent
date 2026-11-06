#!/usr/bin/env bash
# Installe le binaire d'un run CI (ya-build-macos-arm64 ou continuous) dans
# ~/.cache/yusagent-bin/<sha du commit>/, sommes vérifiées. Affiche le dossier.
# Usage : bash bench/smoke/tools/install-bin.sh <run id> [<sha attendu>]
set -euo pipefail
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
trap 'rm -rf "$TMP"' EXIT
gh run download "$RUN" --repo "$REPO" -n "$NAMES" --dir "$TMP" >/dev/null
ART=$TMP
[ -f "$ART/SHA256SUMS" ] && [ -f "$ART/manifest.json" ] || { echo "artefact $NAMES : SHA256SUMS ou manifest.json absent" >&2; exit 1; }
(cd "$ART" && shasum -a 256 -c SHA256SUMS) >&2
SHA=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["commit"])' "$ART/manifest.json")
[ -z "$EXPECTED" ] || [ "$SHA" = "$EXPECTED" ] || { echo "commit $SHA ≠ attendu $EXPECTED" >&2; exit 1; }
DEST="$HOME/.cache/yusagent-bin/$SHA"
rm -rf "$DEST" && mkdir -p "$DEST"
tar -xzf "$ART"/prime-agent-*-darwin-arm64.tar.gz -C "$DEST"
cp "$ART/manifest.json" "$ART/SHA256SUMS" "$DEST/"
WANT=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["binaries"][0]["executableSha256"])' "$DEST/manifest.json")
GOT=$(shasum -a 256 "$DEST/prime-agent" | cut -d' ' -f1)
[ "$WANT" = "$GOT" ] || { echo "sha256 de l'exécutable $GOT ≠ manifeste $WANT" >&2; exit 1; }
"$DEST/prime-agent" --version | grep -q "$SHA" || { echo "version ≠ $SHA" >&2; exit 1; }
echo "$DEST"
