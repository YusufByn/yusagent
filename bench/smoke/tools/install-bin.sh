#!/usr/bin/env bash
# Installe le binaire d'un run CI (ya-build-macos-arm64 ou continuous) dans
# ~/.cache/yusagent-bin/<sha du commit>/, sommes vérifiées. Affiche le dossier.
# Usage : bash bench/smoke/tools/install-bin.sh <run id> [<sha attendu>]
set -euo pipefail
RUN=${1:?id du run CI}
EXPECTED=${2:-}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
gh run download "$RUN" --repo YusufByn/yusagent --dir "$TMP" >/dev/null
SUMS=$(find "$TMP" -name SHA256SUMS | head -1)
ART=$(dirname "$SUMS")
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
