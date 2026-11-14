#!/usr/bin/env bash
# Installe Codex CLI à une version épinglée (YA-14, comparaison yusAgent contre Codex CLI)
# dans ~/.cache/yusagent-bin/codex-<version>/ : binaire `codex`, `codex-code-mode-host`
# (requis par gpt-6-luna, `tool_mode: code_mode_only`, sans repli) et le catalogue des
# modèles du tag (`models.json`, figé par `model_catalog_json`). Aucune compilation : archives
# de la release GitHub, sha256 épinglés ci-dessous (champ `digest` de l'API des releases,
# relevé le 14/11/2026), catalogue lu au commit du tag et vérifié par son sha256.
# Dossier déjà présent : s'il passe les contrôles, rien n'est modifié ; sinon erreur, et
# --force le remplace, une fois la nouvelle copie vérifiée à part. Affiche le dossier.
# Usage : bash bench/smoke/tools/install-codex.sh [--force] [<version>]
set -euo pipefail
FORCE=0
if [ "${1:-}" = --force ]; then FORCE=1; shift; fi
VERSION=${1:-0.169.0}
[ "$(uname -s)/$(uname -m)" = "Darwin/arm64" ] || { echo "cible non prise en charge : $(uname -s)/$(uname -m)" >&2; exit 1; }
# Versions épinglées : une ligne par version, jamais « latest ».
case "$VERSION" in
  0.169.0)
    TAG=rust-v0.169.0
    TAG_COMMIT=d32063f96c2dff1dc96af839b3952abb6acfeda4
    CODEX_SHA=7f4c49c80bae1d0b4e347acfcdf11faa3475d87ae5e65a5eb1261d3c358145ee
    HOST_SHA=d40b320f6f256835d409472b7d3bee618a27549b3f3e1cb8b71dc6c56d3827bf
    # Identique octet pour octet à models.json au SHA 69231e1 (ticket 09, YA-03).
    CATALOG_SHA=1384625c0b6b3e67d3c60da29becea2b017f2c283fc1e2d05b8edafb4698eb10
    ;;
  *) echo "version $VERSION non épinglée (ajouter ses sha256 dans ce script)" >&2; exit 1 ;;
esac
TARGET=aarch64-apple-darwin
URL=https://github.com/openai/codex/releases/download/$TAG
CATALOG_URL=https://raw.githubusercontent.com/openai/codex/$TAG_COMMIT/codex-rs/models-manager/models.json
BASE="$HOME/.cache/yusagent-bin"
DEST="$BASE/codex-$VERSION"

# Contrôles d'un dossier installé : fichiers présents, sha256 du manifeste, version.
check() {
  local d=$1 f want got
  for f in codex codex-code-mode-host models.json manifest.json; do
    [ -f "$d/$f" ] || { echo "$d : $f absent" >&2; return 1; }
  done
  for f in codex codex-code-mode-host models.json; do
    want=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["sha256"][sys.argv[2]])' "$d/manifest.json" "$f")
    got=$(shasum -a 256 "$d/$f" | cut -d' ' -f1)
    [ "$want" = "$got" ] || { echo "$d : sha256 de $f $got ≠ manifeste $want" >&2; return 1; }
  done
  [ "$(shasum -a 256 "$d/models.json" | cut -d' ' -f1)" = "$CATALOG_SHA" ] || { echo "$d : catalogue ≠ $CATALOG_SHA" >&2; return 1; }
  # CODEX_HOME et HOME dédiés, réseau coupé : sans CODEX_HOME, Codex écrit dans ~/.codex
  # avant même de lire --version (arg0/src/lib.rs:134, 322-352 au tag ; garde-fou du banc,
  # codex_arm.check_launch).
  local vhome="$BASE/.codex-version-home"
  mkdir -p "$vhome"
  env -i PATH=/usr/bin:/bin HOME="$vhome" CODEX_HOME="$vhome" \
    /usr/bin/sandbox-exec -p '(version 1) (allow default) (deny network*)' \
    "$d/codex" --version 2>/dev/null | grep -qx "codex-cli $VERSION" || { echo "$d : version ≠ codex-cli $VERSION" >&2; return 1; }
}

if [ -e "$DEST" ] && [ "$FORCE" = 0 ]; then
  if check "$DEST"; then
    echo "déjà installé, contrôles passés, rien de modifié : $DEST" >&2
    echo "$DEST"
    exit 0
  fi
  echo "$DEST existe et échoue aux contrôles ; relancer avec --force pour le remplacer" >&2
  exit 1
fi
TMP=$(mktemp -d)
STAGE=
OLD=
trap 'rm -rf "$TMP" ${STAGE:+"$STAGE"} ${OLD:+"$OLD"}' EXIT
fetch() {  # fetch <url> <fichier> <sha256 attendu>
  curl -fsSL --retry 2 -o "$2" "$1"
  local got
  got=$(shasum -a 256 "$2" | cut -d' ' -f1)
  [ "$got" = "$3" ] || { echo "$1 : sha256 $got ≠ épinglé $3" >&2; exit 1; }
}
fetch "$URL/codex-$TARGET.tar.gz" "$TMP/codex.tar.gz" "$CODEX_SHA"
fetch "$URL/codex-code-mode-host-$TARGET.tar.gz" "$TMP/host.tar.gz" "$HOST_SHA"
fetch "$CATALOG_URL" "$TMP/models.json" "$CATALOG_SHA"
mkdir -p "$TMP/codex" "$TMP/host"
tar -xzf "$TMP/codex.tar.gz" -C "$TMP/codex"
tar -xzf "$TMP/host.tar.gz" -C "$TMP/host"
# Chaque archive porte un seul exécutable, nommé d'après la cible.
one() { local n; n=$(find "$1" -type f | wc -l | tr -d ' '); [ "$n" = 1 ] || { echo "$1 : $n fichiers, 1 attendu" >&2; exit 1; }; find "$1" -type f; }
mkdir -p "$BASE"
STAGE=$(mktemp -d "$BASE/.codex-$VERSION.XXXXXX")
chmod 755 "$STAGE"
install -m 755 "$(one "$TMP/codex")" "$STAGE/codex"
# Cherché à côté de l'exécutable de codex (install-context/src/lib.rs:175-204, tag du script).
install -m 755 "$(one "$TMP/host")" "$STAGE/codex-code-mode-host"
install -m 644 "$TMP/models.json" "$STAGE/models.json"
python3 -I - "$STAGE" "$VERSION" "$TAG" "$TAG_COMMIT" "$CODEX_SHA" "$HOST_SHA" <<'PY'
import hashlib, json, sys
from pathlib import Path
stage, version, tag, commit, codex_archive, host_archive = sys.argv[1:7]
d = Path(stage)
sha = {f: hashlib.sha256((d / f).read_bytes()).hexdigest()
       for f in ("codex", "codex-code-mode-host", "models.json")}
(d / "manifest.json").write_text(json.dumps({
    "harness": "codex", "version": version, "tag": tag, "tag_commit": commit,
    "archives_sha256": {"codex": codex_archive, "codex-code-mode-host": host_archive},
    "catalog_source": f"openai/codex@{commit}:codex-rs/models-manager/models.json",
    "sha256": sha}, indent=2) + "\n")
PY
check "$STAGE" || { echo "copie extraite invalide, $DEST inchangé" >&2; exit 1; }
if [ -e "$DEST" ]; then
  OLD="$BASE/.codex-$VERSION.old.$$"
  mv "$DEST" "$OLD"
  mv "$STAGE" "$DEST" || { mv "$OLD" "$DEST"; OLD=; echo "remplacement échoué, $DEST restauré" >&2; exit 1; }
else
  mv "$STAGE" "$DEST"
fi
STAGE=
echo "$DEST"
