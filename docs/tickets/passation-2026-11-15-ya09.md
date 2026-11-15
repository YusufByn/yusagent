# Passation du 15/11/2026 : YA-09, levier `reasoningSummary`

Note écrite pour une nouvelle session, à la fin de la session qui a mené le levier `reasoningSummary` de YA-09 : plan, réglage, A/B, rapport, défaut `off`. Ticket CDC : YA-09. Références : `bench/smoke/ab/ya09-summary-plan.md`, `bench/smoke/ab/ya09-summary-20261115.md`, journal du CDC (deux lignes du 15/11/2026), `docs/fork-changes.md` (« Réglages de requête (YA-09) »).

Les appels au vrai modèle (chauffes, A/B) sont lancés sur accord explicite de Yusuf seulement. L'A/B lui-même est lancé par Yusuf.

## État de `main`

`main` est à `e3b5aafa8`, le merge de #39. Les trois PR du levier sont mergées, chacune en commit de merge :

| PR | Contenu | Tête | Merge |
|---|---|---|---|
| #37 | Réglage `reasoningSummary` (`auto` / `off`), figé par session (en-tête `yaRequest`), plan de l'A/B, script de mesure, `check_warmup.py` | `ce4b24cd8` | `4480a7e9e` |
| #38 | Rapport de l'A/B, `ab_controls.py`, incidents non écartants d'`ab_stats.py`, constat 1011 au ticket 09, journal du CDC (verdict et décision) | `88042933d` | `d1ddfc069` |
| #39 | **Défaut produit `off`**, `auto` sur réglage explicite | `c770c5ae6` | `e3b5aafa8` |

Binaires (`ya-build-macos-arm64.yml`, à installer avec `bench/smoke/tools/install-bin.sh <run> <sha>`). Les sources livrées de `c770c5a` et de `e3b5aaf` sont identiques (`git diff c770c5ae6 e3b5aafa8 -- crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock` est vide). Leurs exécutables diffèrent, car le commit est embarqué dans la version :

| Commit | Rôle | sha256 de l'exécutable |
|---|---|---|
| `7086711263ebfcd44b33fe7e3381630337243bbe` | binaire de l'A/B (défaut `auto`) | `04c27dd29b2252e89dce47afb6c01bc3c70b9d7bc1ae6a4cae4d1178d57c393f` |
| `c770c5ae679955a09944ffa1378b486e4fca4b6f` | défaut `off` (PR #39), chauffe du 15/11 | `a2dcfb54999bb40f71d956a9f0b09ad78638ef9ba8f533dbbeac9a1443137718` |
| `e3b5aafa80c861854a7439fb0ea85566d0478a97` | `main` après les merges, run 43748486779 (archive `15f5eaca1e849ac53b5a93c1a73444dfc3146ac048a36d6af64cac75b32a2917`) | `0c65ce3bfe1bbca58ed8f75a9449c5397e00bb16ee1e5bb0ac63d9e054cecbac` |

## Verdict de l'A/B `reasoningSummary`

Rapport : `bench/smoke/ab/ya09-summary-20261115.md`. Run du 15/11 `bench/smoke/runs/ab-20261115T153500Z`, sur la machine de Yusuf, non versionné. Règle fixée avant le run.

- **B (`off`) retenu, sur la latence**, pour `gpt-6-luna` seulement, effort high, 10 tâches courtes, 3 passes.
  - Résolution non distinguable : 22 tâches résolues par passe contre 23.
  - Coût à cache théorique des tâches stables non distinguable : −5,7 % [−21,0 ; +15,7].
  - Phase agent : −26,1 % [−41,7 ; −8,1].
  - Latence fixe par appel avec raisonnement : −2,92 s [−3,81 ; −2,24]. Aucun écart sur les appels sans raisonnement (+0,13 s [−0,05 ; +0,32]).
- Cela confirme la corrélation relevée par YA-14 (environ 2,9 s par appel avec raisonnement, contre Codex CLI).
- **Affiché sans décider** : `off` fait 8,5 % d'appels en plus (24,3 contre 22,4 par lancement). Sur les 10 tâches, son coût penche vers plus cher (+10,1 %, IC qui contient 0).
- **Unité écartée** : passe 3 de `node-tough-cookie-630`, bras A, `server_error` Codex après 183 s. Aucun lien établi avec le réglage, et l'écart est neutre sur le verdict.
- **Incident non écartant** : une bascule SSE dans le bras B (`node-testsprite-356`, passe 2), sur un WebSocket fermé en `1011 keepalive ping timeout`. Voir les points ouverts.

## Défaut `off` livré (PR #39)

Décision de Yusuf, journal du CDC du 15/11/2026 :
- **réglage absent = `off`** : `reasoning.summary` est omis sur les trois API Responses (`openai-codex-responses`, `openai-responses`, `azure-openai-responses`). Seul `gpt-6-luna` est mesuré ;
- `"auto"` le remet, octet pour octet comme upstream ;
- **sessions existantes** :
  - un en-tête avec `yaRequest` garde sa valeur ;
  - un en-tête avec `yaTools` sans `yaRequest` (binaire antérieur au 15/11) reste `auto` ;
  - un `yaRequest` illisible est réécrit en `auto` ;
- **écart accepté** : une session antérieure à YA-03 (ni `yaTools` ni `yaRequest`) prend les réglages courants, donc `off` ;
- **le corps de requête change par défaut** ; le prompt système et les tools ne changent pas ;
- **écart avec YA-10** (« raisonnement visible ») : par défaut, aucun raisonnement n'est affiché en TUI ni en ACP ;
- `bench/smoke/settings/default.json` reste `{}` et vaut désormais `off`. `reasoning-summary-off.json` équivaut maintenant au défaut.

**Chauffe du 15/11 sur `c770c5a`** (hors A/B), `py-shortuuid-118`, `gpt-6-luna`, `default.json`, `--log-bodies`, puis `continue_session.py`. Run `bench/smoke/runs/20261115T183135Z-c770c5a-gpt-6-luna`, non versionné.
- Tâche résolue. 16 corps sur 16 avec `reasoning: {"effort": "high"}` sans `summary`, et l'en-tête `yaRequest {"reasoningSummary": "off"}`.
- 10 blocs de raisonnement avec `encrypted_content`. `reasoning_tokens` > 0 sur 10 réponses sur 16.
- Suite par `previous_response_id` sur les requêtes 2 à 16 ; 0 rupture, 0 invalidation, 0 compaction.
- Reprise acceptée : 10 items de raisonnement rejoués avec `encrypted_content`, 23 040 tokens en cache sur 23 328.
- Scans de secrets propres.

## Points ouverts

- **Baselines à refaire sur le nouveau défaut.** `bench/smoke/baseline/gpt-6-luna.json` vient des bras A de YA-14 (binaire `80e20ea`, `summary: "auto"`). `bench/smoke/baseline/gpt-6.1-terra.json` vient des bras A de l'A/B 2 de YA-03 (binaire `80e20ea`, effort high). Elles mesurent toutes deux l'ancien défaut. Les alertes de `compare.py` qui s'y comparent mêlent donc deux configurations. Proposition : reconstruire chaque baseline depuis les bras A (`default.json`) du prochain A/B du modèle. La liste des tâches stables reste utilisable comme population.
- **WebSocket fermé en 1011 pendant une attente** (ticket 09, constat du 15/11) :
  - cause probable côté client : entre deux requêtes, le worker ne lit pas le flux, donc ne répond pas aux pings (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:229-235`, code d'amont). C'est une hypothèse, non vérifiée : le proxy ne journalise pas les trames de contrôle ;
  - après la bascule, la session reste en SSE en corps complet (`session.rs:64`) ;
  - lien direct avec le levier `bash()` : une commande longue en arrière-plan crée ce silence.
- **Défaut par modèle** (piste notée, non décidée) : lire `default_reasoning_summary` du catalogue de Codex (`codex-rs/models-manager/models.json`).
- **Interrupteur du raisonnement visible dans l'extension** : avec YA-10.
- **Quota** : le relevé fait avant l'A/B du 15/11 n'est pas consigné dans le run. Le banc ne lit pas le quota côté yusAgent.

## Prochaines étapes proposées

Ce sont des propositions, à valider par Yusuf. Elles suivent la « Proposition pour YA-09 » du journal du CDC (14/11/2026) : diviser par deux les tours de la session principale sur `gpt-6-luna`, sans perte de résolution, chaque levier en réglage.

1. **Lecture d'Unharnessed**, puis **étape 1 du levier `bash()`**.
   - Unharnessed est une source à fournir par Yusuf : le dépôt n'y fait aucune référence.
   - Levier `bash()` (proposition du 14/11, levier 1) : `await bash(cmd)` rend la main après 1 s (`prime-agent-runtime/src/rlm/bash.py:69`, `:1076`), d'où des tours de relève et des réveils par `[bash-done]`. Les constats chiffrés sont au ticket 09 (« Constats de la comparaison YA-14 »).
   - L'étape 1 est à cadrer en plan, en lecture seule, avant tout code. Elle doit tenir compte du constat 1011 : une attente longue sans requête peut faire fermer le WebSocket.
2. **A/B `bash()`** : le réglage issu de l'étape 1 contre `default.json` (désormais `off`). Plan et règle versionnés avant le run, sur le modèle de `ya09-summary-plan.md`.
3. **A/B de la règle de regroupement** (leviers 2 et 3 de la proposition du 14/11) : lecture et exploration dans une même cellule, edit et test dans le même tour.
4. **Réduction du prompt** : critère d'acceptation de YA-09, prompt système de base sous 2 000 tokens hors tools. Il passe après la réduction des tours (écart noté au journal du 14/11).
5. **Piste `editTool: kernel` par défaut** (non décidée).
   - A/B 1 de YA-03 sur `gpt-6.1-terra` : coût +2,5 % [−8,8 ; +16,5] pour `kernel`, non distinguable. Edits au premier essai : 99,6 % pour `kernel`, 98,8 % pour `native`.
   - La justification de `native` a été révisée le 14/11 : Codex CLI n'expose pas `apply_patch` au premier niveau pour ses modèles (`code_mode_only`).
   - Un A/B sur `gpt-6-luna` trancherait.

## Outils du banc ajoutés pendant le levier

- `bench/smoke/ab/ya09-summary-mesures.py` : latence fixe par appel avec raisonnement, phase agent appariée par tâche.
- `bench/smoke/tools/check_warmup.py` : contrôles d'une chauffe lancée avec `--log-bodies`.
- `bench/smoke/tools/ab_controls.py` : contrôles indépendants d'un A/B, appels les plus longs.
- `bench/smoke/tools/ab_stats.py` reprend les anomalies de `report.json` (bascule SSE) dans les incidents non écartants.
