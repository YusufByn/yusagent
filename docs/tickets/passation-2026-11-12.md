# Passation du 12/11/2026

Note écrite avant le merge de la PR #23 (`ya-ab-rlm` → `main`), pour une nouvelle session qui part de `main` après ce merge. La PR attend la relecture de Yusuf ; le sha du merge n'est pas encore connu.

## État de `main` après le merge de #23

`main` avant #23 : `503c4418a` (merge de #21, YA-14). #23 apporte le ticket YA-06 et l'A/B `rlm.spawn` :

- **Sous-agents coupés par défaut.** `rlmMaxDepth` vaut 0 sans réglage (`crates/pa-core/src/settings/fork_defaults.rs`, `RLM_MAX_DEPTH`, lu par `DEFAULT_RLM_MAX_DEPTH`, `crates/pa-core/src/session_engine/rlm_host.rs:279-281`). Une session racine ne lance aucun sous-agent, `rlm.spawn` est refusé (porte de profondeur : `crates/pa-cli/src/print_rlm.rs:639`, `crates/pa-daemon/src/rlm_children/host.rs:85`), et son prompt n'a plus la surface des sous-agents (`crates/pa-core/src/prompts/rlm_off.rs`). `"rlmMaxDepth": 2` (ou toute valeur ≥ 1) dans `~/.prime/agent/settings.json`, `RLM_MAX_DEPTH=2`, ou `/rlm-max-depth` dans une session, réactive le spawn et le prompt upstream, octet pour octet.
- **Gardés** : `agent-message`, `rlm-heartbeat` et l'API du harnais continu (ils servent aussi entre sessions racines et à la session elle-même ; `docs/fork-changes.md`, « Sous-agents coupés (YA-06) »).
- **Tests** : `RLM_MAX_DEPTH = "2"` dans `.cargo/config.toml` (`[env]`), pour que les tests upstream lancés par `cargo test` gardent la borne upstream ; trois fichiers de tests upstream et cinq vérifications de cycle de vie (`scripts/check_*.py`) ajustés, un test e2e ajouté (`docs/fork-changes.md`, « Tests upstream touchés (YA-06) »). Un `cargo run` local démarre aussi à 2 ; les binaires de la CI et du banc ne lisent pas ce fichier.
- **A/B** : plan et règle de verdict (`bench/smoke/ab/rlm-spawn-plan.md`), rapport (`bench/smoke/ab/rlm-spawn-20261112.md`) : coût à cache théorique des tâches stables -65,2 % [-69,6 ; -56,6] sans sous-agents, 25 tâches résolues contre 25, verdict « coupé ».
- **Docs** : journal du CDC (décisions du 11/11 et verdict du 12/11), `docs/fork-changes.md`, fragments `.changes/ya-06-rlm-off.md` et `.changes/ya-06-rlm-default-off.md`, règle d'attente CI dans `CLAUDE.md`.

## Règles de travail

`CLAUDE.md` fait foi ; le journal des décisions en tête de `docs/yusAgent-cdc.md` aussi. Les points qui reviennent le plus :

- Un ticket à la fois, périmètre strict ; un blocage hors périmètre : s'arrêter et le signaler.
- Toute modification d'un fichier upstream : minimale, listée dans `docs/fork-changes.md` (fichier, raison, ticket). Ajouts dans des modules du fork quand c'est possible (exemple : les défauts du fork dans `fork_defaults.rs`).
- Préfixe de prompt : rien de volatile ; tout changement des requêtes envoyées est signalé et prouvé au banc de capture.
- Aucun appel à un vrai modèle sans demande explicite de Yusuf ; la CI n'utilise que le provider factice.
- Pas de build Rust local : binaires et tests en CI. `rustfmt --check` sur un fichier reste possible en local (formatage seul).
- Commits préfixés par le ticket, un sujet par commit ; branche `ya-<ticket>`.
- Merge seulement sur un `ci.yml` vert du dernier commit poussé. Un push annule le `ci.yml` en cours : regrouper les corrections.
- Attente CI : 30 min au plus sans démarrage (aucun job attribué), puis compte rendu. Un job en échec pour une panne d'infrastructure vérifiée : une seule relance du job, sans push.
- Mesures : `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION` fixé, chaque chiffre produit par une commande notée ; une estimation est marquée comme telle.

## Outils du banc et commandes

Depuis la racine du dépôt.

```bash
# Binaire d'une branche : build CI (ya-build-macos-arm64.yml, sur push d'un ya-* qui touche crates/, ou à la main)
gh workflow run ya-build-macos-arm64.yml --repo YusufByn/yusagent --ref <branche>
bash bench/smoke/tools/install-bin.sh <run id> <sha attendu>      # -> ~/.cache/yusagent-bin/<sha>/

# Banc de capture (sans modèle) : requêtes envoyées, par bras
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/<sha>/prime-agent --model gpt-6.1-terra --effort high --capture --tasks py-shortuuid-118 --settings bench/smoke/settings/default.json --out <dossier>
python3 -I bench/smoke/tools/compare_capture_prompts.py <run X> <run Y>   # instructions, tools, input ; tokens avec tiktoken si importable

# Smoke bench réel (lancé par Yusuf)
make smoke BINARY=<dossier>/prime-agent PROVIDER=openai-codex MODEL=gpt-6.1-terra EFFORT=high [SETTINGS=...] [RUNS=1] [TASKS=...]
python3 -I bench/smoke/compare.py <run A> <run B> --baseline bench/smoke/baseline/gpt-6.1-terra.json --out <fichier>.md

# A/B entrelacé par tâche (lancé par Yusuf), puis statistiques
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha de référence> --a <réglages A> --b <réglages B> --passes 3 --seed <graine> --model gpt-6.1-terra --effort high --idle-timeout 300
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-<horodatage>/ab.json --baseline bench/smoke/baseline/gpt-6.1-terra.json

# Baseline, recalcul, tests du banc sans modèle
python3 -I bench/smoke/baseline.py <run> [<run> ...] [--exclude <run>:<tâche>] [--out <fichier>]
python3 -I bench/smoke/run.py --rereport <run>
python3 -I -m unittest discover -s bench/smoke/tests -v
```

Tokens des instructions : `~/.cache/yusagent-capture/analysis-venv/bin/python3` avec `TIKTOKEN_CACHE_DIR=~/.cache/yusagent-capture/tiktoken` (tiktoken `o200k_base`, estimation). `bench/smoke/runs/` est ignoré par git : les runs n'existent que sur la machine de Yusuf. Détail : `bench/smoke/README.md`.

## Points ouverts

- **Baseline reconstruite** (commit `4024960`) : `bench/smoke/baseline/gpt-6.1-terra.json` vient des 30 bras B de `bench/smoke/runs/ab-20261112T185001Z/` (binaire `69cc894`, joués en alternance avec les bras A, pas sur `main` ; requêtes identiques au nouveau défaut, banc de capture du 12/11). 8 tâches stables (les mêmes qu'au 04/11), coût moyen 0,3953 $ (1,0654 $ au 04/11, rlm actif), CV retenu 0,060 (0,167) : seuil de l'alerte agrégée de `compare.py` 12,1 % (33,4 %). L'en-tête porte `rlmMaxDepth: 0` en réglage explicite ; `compare.py` ne compare que le modèle. L'ancienne baseline reste dans l'historique git (`59e93cb3b`).
- **Verdict `rlm` provisoire** : A/B à refaire au banc complet (YA-14) avec des tâches d'exploration sur gros dépôt, comme l'exige le CDC.
- **Masquage à rejuger** sur la nouvelle config (verdict du 11/11 obtenu avec `rlm.spawn` actif, `bench/smoke/ab/masquage-20261106.md`).
- **`bench/smoke/settings/rlm-off.json`** est redondant avec le nouveau défaut ; gardé pour relire l'A/B du 12/11. Proposition : le garder tel quel, ou le supprimer avec une note dans le rapport. Rien n'est supprimé.
- **Runs fantômes** : 6 runs du push de `69cc894` (11/11 20:18Z : 43095249092, 43095251157, 43095251164, 43095251116, 43095251115, 43095251139) restent `queued` sans job ; `gh run cancel`, `force-cancel` et `gh run delete` échouent (409, 409, 403). Ils n'ont pas bloqué les runs suivants. À voir avec le support GitHub si besoin.
- Rappels des passations précédentes : relecture des 64 derniers Kio du journal d'usage au redémarrage (`crates/pa-core/src/session_engine/usage_log.rs:33`) ; `cache_write_tokens` de Codex toujours à 0 ; `release.yml` sur `macos-15` non testé avant la prochaine release.

## Prochaine étape : YA-03 de base

Spécification : `docs/yusAgent-cdc.md`, « YA-03 : Tools de base » ; décision du 11/11 : variante bash natif en réglage dans le même ticket. Périmètre de base :

- `edit` en tool natif, au format de la famille du modèle, qui renvoie la zone modifiée avec ses numéros de ligne.
- `read` numéroté (contenu avec numéros de ligne).
- Plafond de sortie des cellules : 8 000 tokens par défaut, au-delà le chemin du fichier de sortie complète.
- Notice de redémarrage du kernel : après un crash, « kernel redémarré, variables perdues » en fin de contexte, aucune cellule rejouée.
- Variante bash natif : un réglage, pas une branche (CLAUDE.md, règle 5), pour un A/B smoke avant la réduction du prompt (YA-09).

Avant de coder : branche `ya-03` depuis `main` à jour, ticket dans `docs/tickets/`, et vérifier au banc de capture que le prompt par défaut ne change pas tant qu'un réglage n'est pas activé (ou signaler le changement de préfixe).
