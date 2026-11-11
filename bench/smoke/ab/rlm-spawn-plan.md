# A/B `rlm.spawn` (YA-06, YA-14) : plan et règle de verdict

Règle fixée et versionnée avant tout run réel (étape 4 du plan, CDC, décision du 11/11/2026 : A/B `rlm.spawn` on/off, puis nouvelle baseline sur la config retenue).

## Bras

| Bras | Réglages | `rlm.spawn` | Prompt |
|---|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | actif (borne par défaut 2) | celui de `0fa8fad`, octet pour octet |
| B | `bench/smoke/settings/rlm-off.json` (`{"rlmMaxDepth": 0}`) | coupé : refusé à l'exécution (`crates/pa-cli/src/print_rlm.rs:639`) | sans la surface des sous-agents (`crates/pa-core/src/prompts/rlm_off.rs`, YA-06) |

Rien d'autre ne change entre les bras : même binaire, mêmes tâches, même modèle, même effort. `agent-message` et `rlm-heartbeat` restent dans les deux bras (ils servent aussi entre sessions racines et à la session elle-même, `docs/fork-changes.md`, « Sous-agents coupés (YA-06) »).

## Protocole

- `gpt-6.1-terra`, provider `openai-codex`, effort `high`.
- Les 10 tâches du banc (`bench/smoke/tasks/`, sans les exemples).
- 3 passes entrelacées par tâche (`tools/ab.py`) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort. Graine : **1782223764** (tirée de `os.urandom` le 11/11/2026, avant le run).
- Binaire **`f5e0cb4`** (`f5e0cb4254790b8e0e6345eeb1eec76fb80722a9`, branche `ya-ab-rlm`, artefact de `ya-build-macos-arm64.yml`), installé par `tools/install-bin.sh` dans `~/.cache/yusagent-bin/f5e0cb4254790b8e0e6345eeb1eec76fb80722a9/`. Il remplace `0fa8fad` : il est le seul à porter le réglage B complet.
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité 300 s.
- Statistiques : `python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --baseline bench/smoke/baseline/gpt-6.1-terra.json`. Tâches stables : `aggregate.tasks` de la baseline.

## Règle de verdict

**Verdict par défaut : `rlm.spawn` coupé.**

`rlm.spawn` reste actif seulement si l'une des deux conditions est remplie :

1. le bras coupé (B) coûte plus cher en **coût à cache théorique des tâches stables**, avec un IC bootstrap sur les tâches qui exclut zéro. Lecture : ligne « tâches stables, théorique (décision) » de la section « Écart agrégé » d'`ab_stats.py` (somme B / somme A − 1), borne basse de l'IC 95 % > 0 ;
2. le bras coupé **résout au moins 3 tâches de moins** que le bras actif, sur l'ensemble des paires valides. Lecture : « Tâches résolues par passe », totaux A et B de la section « Par bras ». Ce seuil est fixé avant le run ; ce n'est pas un test statistique.

**Affichés sans décider** : coût rapporté et pessimiste, tours de la session principale, durée, part des sous-agents (« Part du coût rapporté des sous-agents »), miss serveur.

**Paires écartées** : selon la règle d'écart du README (« A/B : erreurs provider ») : une erreur provider (quota, surcharge, flux bloqué, statut `stalled`) sur un bras écarte la tâche de cette passe, ses deux bras. `ab_stats.py` l'applique et liste les unités écartées avec leur cause.
