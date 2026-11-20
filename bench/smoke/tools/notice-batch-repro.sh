#!/usr/bin/env bash
# Rejeu factice du regroupement des avis [bash-done] (YA-09, ticket 11, `bashNoticeBatchSeconds`).
# Aucun appel à un vrai modèle : provider factice (PRIME_AGENT_FAUX_SCRIPT), dossier jetable.
# Trois commandes détachées (1 s, 2,5 s, 4 s) lancées avant la fin du tour. Attendu avec le défaut
# (120 s) : les trois avis partent ensemble à la fin de la dernière commande, puis un seul appel
# de réveil (une seule ligne `assistant:WOKEN`). Avec `bashNoticeBatchSeconds: 0` (troisième
# argument `0`) : un réveil par avis.
#
# Usage : bash bench/smoke/tools/notice-batch-repro.sh <binaire yusagent> <python du kernel> [fenêtre]
# Python du kernel : un venv avec le runtime du binaire, par exemple celui que le banc prépare
# (~/.cache/yusagent-smoke/kernel-venv/<16 premiers caractères du sha256 du binaire>/bin/python).
set -euo pipefail
BIN=${1:?binaire yusagent}
PY=${2:?python du kernel}
WINDOW=${3:-}
DIR=$(mktemp -d)
trap 'rm -rf "$DIR"' EXIT
mkdir -p "$DIR/agent" "$DIR/home"
if [ -n "$WINDOW" ]; then
  printf '{"bashNoticeBatchSeconds": %s, "permissions": "auto"}\n' "$WINDOW" > "$DIR/agent/settings.json"
fi
SCRIPT='{"responses":[{"content":[{"type":"toolCall","name":"ipython","arguments":{"code":"a = bash(\"sleep 1\")\nb = bash(\"sleep 2.5\")\nc = bash(\"sleep 4\")\nprint(\"STARTED\")"}}]},"WAITING","WOKEN"],"repeatLastResponse":true}'
(
  cd "$DIR"
  env -u RLM_DEPTH -u PRIME_AGENT_SESSION_DIR -u PRIME_AGENT_CODING_AGENT_SESSION_DIR \
    HOME="$DIR/home" DO_NOT_TRACK=1 PI_OFFLINE=1 \
    PRIME_AGENT_CODING_AGENT_DIR="$DIR/agent" PRIME_AGENT_FAUX_SCRIPT="$SCRIPT" \
    PRIME_AGENT_KERNEL_PYTHON="$PY" "$BIN" -p "run the commands" > out.txt 2> err.txt
)
echo "stdout : $(cat "$DIR/out.txt")"
python3 -I -c '
import json, sys
for line in open(sys.argv[1]):
    entry = json.loads(line)
    if entry.get("type") == "custom_message" and entry.get("customType") == "async_bash_completion":
        print(entry["timestamp"], "done:" + entry["details"]["command"])
    elif entry.get("type") == "message" and entry["message"]["role"] == "assistant":
        text = "".join(b.get("text", "") for b in entry["message"]["content"] if b.get("type") == "text")
        print(entry["timestamp"], "assistant:" + text)
' "$(ls "$DIR"/agent/sessions/*.jsonl)"
