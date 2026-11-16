# Passation du 16/11/2026 : YA-09, ticket 12 (WebSocket 1011), A/B du cache v2, plan du ticket 13

Note écrite pour une nouvelle session, à la fin de la session qui a mené le ticket 12, préparé l'A/B du cache du prompt v2 et écrit le plan du ticket 13. Ticket CDC : YA-09. Références : `docs/tickets/12-ya09-ws-keepalive.md`, `bench/smoke/ab/ya09-cache-v2-plan.md`, `docs/tickets/13-ya09-bash-full-wait.md`.

Les appels au vrai modèle (chauffes, A/B) sont lancés sur accord explicite de Yusuf seulement. Les A/B sont lancés par Yusuf.

## Ordre pour la prochaine session (consigne de Yusuf)

1. **D'abord : analyse et rapport de l'A/B du cache v2.**
   - Run `bench/smoke/runs/ab-20261116T124625Z` (non versionné, sur la machine de Yusuf), fini : 40 lancements sur 40, tous `resolved` (`grep -c "| resolved |" bench/smoke/runs/ab-20261116T124625Z/ab.md` : 40). Unités écartées et verdict : à calculer.
   - Commandes du plan :
     ```bash
     python3 -I bench/smoke/tools/cache_calls.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json
     python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json --baseline bench/smoke/baseline/gpt-6-luna.json
     ```
   - Verdict selon la règle fixée avant le run (`ya09-cache-v2-plan.md`, « Règle de verdict ») : d ≥ 3 points et borne basse de l'IC > 0, alors ticket prioritaire ; d ≥ 3 points et IC qui contient 0, alors non concluant (Yusuf décide) ; d < 3 points, alors « pas d'écart net détecté » avec la borne haute de l'IC.
   - Rapport versionné sur le modèle de `ya09-summary-20261115.md` (par exemple `bench/smoke/ab/ya09-cache-v2-20261116.md`), section « Résultats » du plan remplie, ligne au journal du CDC pour le verdict.
2. **Ensuite : ticket 13**, attente complète de `bash()` (`docs/tickets/13-ya09-bash-full-wait.md`). Plan et décisions D1 à D5 validés le 16/11. Chauffes `py-silence-001` et `py-silence-002` autorisées d'avance. Première étape : relire `bash.py` et le prompt d'Unharnessed au commit `fe2cf2815`, dans un clone jetable, sans rien exécuter.

## État de `main`

Trois PR mergées le 16/11, en commit de merge :

| PR | Contenu | Tête | Merge |
|---|---|---|---|
| #44 | Clôture du ticket 11 (lot Unharnessed) | `8ff116711` | `ded459d13` |
| #45 | Keepalive du WebSocket Codex (ticket 12), cherry-pick de `c4800c8eb` | `877812b93` | `318f3e118` |
| #46 | Plan de l'A/B du cache v2, `cache_calls.py` | `371d00973` | `5d8bb92b3` |

Plus la PR du plan du ticket 13 et de cette passation (branche `ya-09-ticket13-plan`, docs seulement).

Binaires (`ya-build-macos-arm64.yml`, à installer avec `bench/smoke/tools/install-bin.sh <run> <sha>`) :

| Commit | Rôle | Run | sha256 de l'exécutable |
|---|---|---|---|
| `cce48e6d5` | binaire de la PR #45, chauffes du ticket 12 | 43850337252 | `f0b99ce94b9508a82082f3a3e699480e069a1c04a3771408d4b1efb037c20f52` |
| `318f3e118b71bc5734ac81507d001ecf82adaf87` | `main` après #45, binaire de l'A/B du cache v2 | 43857318671 | `1c1923832681f6179e485a4797a3de85942644078b2e95a35f15ca4f35685dcb` |

Entre `318f3e118` et `5d8bb92b3`, seuls des documents et des outils du banc changent (PR #46).

## Ticket 12 : WebSocket 1011 (clos)

Détail : `docs/tickets/12-ya09-ws-keepalive.md`.
- **Cause** : entre deux requêtes, le worker ne lisait pas le flux, donc aucun pong ne partait. Le serveur fermait en `1011 keepalive ping timeout` après 74 à 79 s, puis la session restait en SSE en corps complet.
- **Correctif (a)** : le worker lit le flux pendant le silence (`ya_idle.rs`, environ 30 lignes dans `websocket.rs`, 8 dans `mod.rs`). Une connexion fermée pendant le silence est remplacée par un WebSocket neuf. Les messages de données reçus hors requête sont comptés (diagnostic `websocket_idle_messages_ignored`).
- **Chauffe 2** (`py-silence-001`, `sleep 180`) : 3 appels en WebSocket sur une connexion, 0 bascule SSE, 0 « 1011 », `sincePrevMs` 180 050 ms, appel suivant en suite de conversation.
- **Corps de requête** inchangés (`compare_bodies.py`).
- **Note dans `fork-changes.md`** : bug upstream, candidat à une PR upstream après les testeurs.

## A/B du cache v2 : origine

- Les chauffes du 16/11 montraient des miss serveur (`cacheRead = 0` en suite de conversation) sur les binaires du prompt v2 : 7 sur 55 appels hors premier, dont 4 après 0 s de silence. Les longs silences ne les expliquent donc pas.
- Le prompt v1 a lui aussi environ 2,7 à 2,8 % de miss sur les 5 tâches retenues (baseline du 14/11, A/B du 15/11).
- D'où l'A/B v2 contre v1, même binaire et même jour, 4 passes, graine `145634427`.

## Points ouverts

- **Baselines** : `bench/smoke/baseline/gpt-6-luna.json` mesure un défaut ancien (`summary: "auto"`, prompt v1, fenêtre `bash()` de 1 s). Le ticket 13 change encore le défaut (`"full"`). Proposition inchangée depuis le 15/11 : reconstruire depuis les bras A du prochain A/B du modèle.
- **Avis `[bash-done]` groupés dans le démon** : critère de YA-10, non fait.
- **Ticket 12, ligne « Relu par Yusuf avant le merge : à faire »** (section « Changements de requête ») : la PR #45 a été mergée sur le « merge » de Yusuf, mais la ligne n'a pas été mise à jour.
- **Ping du serveur** : intervalle inconnu, le proxy ne journalise pas les trames de contrôle. Il compte pour le garde-fou d'inactivité du banc pendant les longues cellules (ticket 13).

## Outils du banc ajoutés

- `bench/smoke/tasks/py-silence-001` (tag `chauffe`, jamais tirée par défaut ; `TASKS=py-silence-001`).
- `bench/smoke/tools/compare_bodies.py` : changements de requête entre deux chauffes `--log-bodies`.
- `bench/smoke/tools/cache_calls.py` : miss et relectures partielles par appel ; `--plan` pour un A/B, avec le verdict de la règle du cache v2.
