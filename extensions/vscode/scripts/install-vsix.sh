#!/bin/sh
# Installe le .vsix produit par la CI (workflow ya-extension) dans VS Code (ticket 24, PR 2 et PR I).
#
# Usage : extensions/vscode/scripts/install-vsix.sh <id du run ya-extension> [dossier d'extensions]
#
# Télécharge l'artefact `yusagent-vsix-<sha>` dans un dossier neuf, affiche taille, sha256 et
# contenu, refuse un .vsix de 5 Mo ou plus ou qui contient autre chose que les fichiers attendus,
# puis l'installe avec `code --install-extension`. Avec un dossier d'extensions, l'installation y
# est isolée (`--extensions-dir`), sans toucher au VS Code de l'utilisateur.
set -eu

RUN=${1:?usage : install-vsix.sh <id du run ya-extension> [dossier d extensions]}
EXT_DIR=${2:-}
REPO=YusufByn/yusagent

DIR=$(mktemp -d "${TMPDIR:-/tmp}/yusagent-vsix-XXXXXX")
gh run download "$RUN" --repo "$REPO" --dir "$DIR" --pattern 'yusagent-vsix-*'
VSIX=$(find "$DIR" -name yusagent.vsix | head -n 1)
[ -n "$VSIX" ] || { echo "aucun yusagent.vsix dans l'artefact du run $RUN" >&2; exit 1; }

SIZE=$(wc -c < "$VSIX" | tr -d ' ')
echo "vsix : $VSIX"
echo "taille : $SIZE octets (limite 5000000)"
echo "sha256 : $(shasum -a 256 "$VSIX" | cut -d' ' -f1)"
[ "$SIZE" -lt 5000000 ] || { echo "ÉCHEC : .vsix trop gros" >&2; exit 1; }

FILES=$(unzip -Z1 "$VSIX" | sort)
echo "contenu :"
echo "$FILES" | sed 's/^/  /'
UNEXPECTED=$(echo "$FILES" | grep -v -x -E '\[Content_Types\]\.xml|extension\.vsixmanifest|extension/package\.json|extension/readme\.md|extension/README\.md|extension/LICENSE(\.txt)?|extension/dist/extension\.js|extension/dist/webview/main\.js|extension/dist/webview/main\.css|extension/media/icon\.svg' || true)
[ -z "$UNEXPECTED" ] || { echo "ÉCHEC : fichiers inattendus :" >&2; echo "$UNEXPECTED" >&2; exit 1; }

if [ -n "$EXT_DIR" ]; then
  mkdir -p "$EXT_DIR"
  code --extensions-dir "$EXT_DIR" --install-extension "$VSIX" --force
  code --extensions-dir "$EXT_DIR" --list-extensions --show-versions | grep '^yusagent\.yusagent@'
else
  code --install-extension "$VSIX" --force
  code --list-extensions --show-versions | grep '^yusagent\.yusagent@'
fi
echo "installé"
