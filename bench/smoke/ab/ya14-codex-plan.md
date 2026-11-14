# YA-14 : yusAgent contre Codex CLI au smoke bench, plan et règle de verdict

Règles fixées et versionnées **avant tout run réel** (journal du CDC, décision du 14/11/2026 : comparaison avant YA-09, `gpt-6-luna` des deux côtés). Aucune ligne de ce plan ne change après le premier run de l'A/B ; une correction se fait par un nouveau plan daté.

**Conclusions valables pour `gpt-6-luna` seulement.**

## Bras

| | A : yusAgent | B : Codex CLI |
|---|---|---|
| Binaire | artefact de `ya-build-macos-arm64.yml` à `80e20ea5f353ff3855e30d19c0df7cc7d1cc9632` (sources livrées identiques à `main` `91e821f7a` : `git diff --quiet 80e20ea origin/main -- crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock`), sha256 `099f54d354c3…` (`ya03-plan.md`) | Codex CLI **0.169.0** (tag `rust-v0.169.0`, commit `d32063f96c2d…`), installé par `tools/install-codex.sh` : `codex` sha256 `739cda86e15d30a6e2b40cf8dcbbfb988aaf48b305b27065573bb8e9fd41d002`, `codex-code-mode-host` sha256 `10feae2abe764bcf2273e241ace67aecc360eb8c28a82a2988bf1d8498a9bacf`, catalogue `models.json` sha256 `1384625c0b6b…` (identique à celui de `69231e1`, ticket 09) |
| Réglages | `settings/default.json` | `settings/codex-0.169.0.json` : `featureCuts` `apps`, `plugins`, `tool_suggest` ; config `codex/config.toml.in` |
| Outils vus par le modèle | `python_repl`, `apply_patch` (libre) | `exec` (JavaScript, libre), `wait`, `request_user_input`, `request_user_input_async`, `sleep`, espace `collaboration` (multi-agent) ; imbriqués dans `exec` : `apply_patch`, `exec_command`, `write_stdin`, `view_image`, buts, ressources MCP, `clock__curr_time`, `image_gen__imagegen` |

Version de Codex : dernière release stable au 14/11/2026 (GitHub, `latest` npm). Au tag, `models.json` met les 12 modèles en `tool_mode: code_mode_only` (`gpt-6-luna` : ligne 716), d'où `exec` et `wait` seuls au premier niveau (`codex-rs/code-mode-protocol/src/lib.rs:61-62`, `codex-rs/core/src/tools/mod.rs:66-76` au tag).

## Protocole

- `gpt-6-luna`, effort `high` (yusAgent : `--thinking high` ; Codex : `-c model_reasoning_effort=high`, `reasoning.effort` à `high` dans le corps, vérifié au banc de capture).
- Les 10 tâches du banc (`bench/smoke/tasks/`, sans les exemples), **3 passes entrelacées par tâche** (`tools/ab.py`) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort ; graine tirée de `os.urandom` et notée dans `ab.json`.
- Même runner pour les deux bras (`run.py`) : même préparation des tâches, même sandbox de la phase agent (`sandbox-agent.sb`), mêmes proxys, même notation, même garde-fou d'inactivité (300 s). `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short`.
- **Avant l'A/B** : run de chauffe d'une tâche sur le bras B avec `--log-bodies` (lancé par Yusuf), qui ne compte pas dans l'A/B. Il vérifie au vrai backend : suite de conversation (`previous_response_id`), prewarm (usage renvoyé ou non), aucun appel à `alpha/search` ni hors `/backend-api/codex/responses`, effort `high`, CONNECT refusés.
- Statistiques : `python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --verdict ya14-codex`.

### Unités écartées

Règle du README (« A/B : erreurs provider ») : une erreur provider sur un bras écarte la tâche de cette passe, ses deux bras. S'ajoutent, pour les runs postérieurs à l'adaptateur (`ab_stats.py`, `incidents`) :
- erreur ou réponse en échec lue au reverse proxy (`event: response_error`, `response.failed`), les deux bras ;
- bras B : tout événement `error` de la sortie `--json` de Codex, **relancé ou non** (Codex relance seul les flux coupés : `core/src/responses_retry.rs` au tag), et `turn.failed`.

Une compaction est un **incident** (anomalie du rapport, listée, non écartante) : aucune tâche du banc ne devrait l'atteindre (seuil de Codex à 90 % de 272 000 tokens).

## Règle de verdict (A = yusAgent, B = Codex CLI)

1. **Résolution d'abord.** Une tâche est résolue pour un bras si elle l'est à **au moins 2 passes valides**. Une tâche avec moins de 2 passes valides n'est pas jugée (listée). Un écart de **3 tâches ou plus** fait le verdict de résolution. Seuil fixé avant le run, pas un test statistique.
2. **Puis le coût**, s'il n'y a pas de verdict de résolution : **coût à cache théorique** (`cost.theoretical_total`), écart B/A − 1 (somme des moyennes par tâche), **IC 95 % par bootstrap sur les tâches** (10 000 tirages, graine 0). Population : les tâches **résolues à toutes leurs passes valides par les deux bras** (au moins une passe valide). **Au moins 5 tâches**, sinon pas de verdict de coût.
3. **Le coût décide seulement si l'IC exclut 0 et |écart| > 10 %** ; sinon « non distinguable ». Le seuil de 10 % vient de la sensibilité du banc (journal du CDC, constat du 14/11/2026 : le banc ne distingue pas un effet de moins de 10 % environ).
4. Avec un verdict de résolution, le coût est affiché sans décider.

Verdicts possibles : résolution pour A ou pour B ; sinon coût : A moins cher, B moins cher, non distinguable ; ou ni l'un ni l'autre (moins de 5 tâches communes).

### Affiché sans décider

Par bras (`ab_stats.py`, sections « Par bras » et « Affiché sans décider ») : coût rapporté et pessimiste (et sur les mêmes tâches que le coût de décision), coût rapporté par tâche résolue, tours, tokens par catégorie (input non caché, cache lu rapporté et théorique, cache écrit), taille du premier prompt, pic de contexte, taux de cache et miss serveur, ruptures du cache théorique, compactions, appels prewarm, part des sous-agents, durée de la phase agent, edits au premier essai, incidents, quota de l'abonnement (dernier relevé de Codex ; non mesuré côté yusAgent), écarts de config ci-dessous.

## Mesure du bras Codex

- **Usage par réponse** : ligne `token_usage_record` du fichier de session de Codex (`$CODEX_HOME/sessions/AAAA/MM/JJ/rollout-*.jsonl`, `codex-rs/core/src/session/mod.rs:5718`, `codex-rs/protocol/src/protocol.rs:2568-2577`), convertie au format du journal du harness : `input` = `input_tokens − cached_input_tokens − cache_write_input_tokens` (`docs/usage.md`). La sortie `--json` ne donne qu'un cumul du thread à `turn.completed` (`codex-rs/exec/src/event_processor_with_jsonl_output.rs:119-128`) : elle ne sert pas au coût.
- **Prewarm** (`generate: false`, entrée vide, avant le premier appel d'un tour) : Codex jette son usage (`codex-rs/core/src/client.rs:2569-2577`). Le reverse proxy lit la fin de chaque réponse (compteurs seuls) ; l'usage du prewarm devient un appel `prewarm` (annexe, coût rapporté). Au banc de capture, la requête suivante continue le prewarm (`previous_response_id`).
- **Cache théorique** : même méthode que yusAgent (README). Appariement au proxy par ordre (`metrics.match_transports_in_order`) : le journal converti porte l'heure de fin de chaque appel.
- **Rupture sur corps complet** (les deux bras) : un appel de boucle hors premier dont la requête WebSocket part sans `previous_response_id` garde son `cacheRead` rapporté. Recalcul des 120 bras des A/B de YA-03 avec et sans ce changement et la lecture des réponses au proxy : **0 écart** (commande ci-dessous).
- **Edits au premier essai** : items `file_change` de `--json` (`completed` ou `failed`), un par `apply_patch` imbriqué. Borne haute : un patch refusé à l'analyse, avant application, peut ne produire aucun item.
- **Cellules en erreur** : pas d'équivalent mesuré pour `exec` (`null`) ; le nombre d'appels d'`exec` est donné.

## Écarts avec chaque harness tel que livré (biais)

Codex 0.169.0 tourne tel que livré (multi-agent gardé, compaction distante non modifiée par la config), sauf :

1. **Provider personnalisé** `ya_bench` (nom `yusagent-bench`, `base_url` vers le reverse proxy, `requires_openai_auth`, WebSocket) au lieu du provider `openai`, dont seule l'URL du catalogue est réglable (`codex-rs/model-provider-info/src/lib.rs:802-821`). Comportements liés au nom « OpenAI » (`is_openai()`, `codex-rs/model-provider-info/src/lib.rs:709-711`), au tag :

   | Site | Effet de `is_openai()` | Notre montage (WebSocket, exec, Luna) |
   |---|---|---|
   | `model-provider-info/src/lib.rs:713-720`, `model-provider/src/provider.rs:256-282` | routage d'espace de travail (`wham/accounts/check`) avant chaque requête | **s'applique** : pas de routage. Compte personnel : sans effet attendu |
   | `model-provider/src/capabilities.rs:36` | compaction distante v2 par défaut | **s'applique** : compaction locale. Toute compaction est un incident |
   | `core/src/client.rs:1231-1249` | sinon, `encrypted_content` retiré des items rejoués (hors raisonnement et compaction) | **s'applique** en corps complet seulement ; en suite de conversation, seuls les nouveaux items partent |
   | `core/src/client.rs:1940` | compression zstd du corps HTTP | ne s'applique pas : WebSocket |
   | `core/src/client.rs:1384` | en-têtes `codex_responses_headers` | ne s'applique pas : remplis par le reviewer guardian seul (`ext/guardian-v2/src/sync_reviewer/mod.rs:101`) |
   | `core/src/client.rs:661` | changement d'effort en cours de session | ne s'applique pas : `supports_reasoning_effort_updates` faux pour Luna |
   | `core/src/client.rs:1253`, `core/src/session/turn.rs:2937` | résumés de raisonnement concurrents | ne s'applique pas : fonction `concurrent_reasoning_summaries` désactivée par défaut, résumé `none` pour Luna |
   | `core/src/client.rs:1259` | `output.encrypted_content` demandé | ne s'applique pas : fonction `output_token_replay` désactivée par défaut |
   | `ext/web-search/src/extension.rs:47` | recherche web disponible | ne s'applique pas : `web_search = "disabled"` |
   | `ext/image-generation/src/extension.rs:43` | génération d'images disponible (`is_openai` ou `requires_openai_auth`) | **s'applique** : le tool `image_gen__imagegen` est annoncé (gardé, voir 5) |
   | `ext/history-notes/src/extension.rs:60` | notes d'historique | ne s'applique pas : `use_history_notes_extension` faux par défaut |
   | `core/src/compact_model_fallback.rs:30` | repli de modèle pour la compaction | ne s'applique pas sans compaction (incident) |
   | `model-provider/src/models_endpoint.rs:56, 319` | catalogue distant | ne s'applique pas : catalogue figé (`model_catalog_json`) |
   | `model-provider/src/auth.rs:458` | FedRAMP | ne s'applique pas |
   | `core/src/realtime_conversation.rs:2161`, `app-server/src/turn_cost_worker.rs:121`, `app-server/src/thread_sample.rs:225`, `ext/guardian-v2/.../connection_pool.rs:459`, `tui/src/chatwidget/model_popups.rs:90` | temps réel, coût OTel, échantillons, guardian, TUI | ne s'appliquent pas à `codex exec` |

2. **Catalogue figé** : `models.json` du tag (le catalogue distant pourrait changer d'un run à l'autre).
3. **Recherche web coupée** (réseau coupé en phase agent ; le tool passerait par `{base_url}/alpha/search`), **analytics coupé** (événements et métriques vers `chatgpt.com`), **reconnexions sans fin coupées** et délai d'inactivité de Codex porté à 900 s (le garde-fou du banc, 300 s, décide seul d'un flux bloqué ; une boucle de reconnexion ferait passer des octets par le proxy et échapperait au garde-fou).
4. **Sandbox de Codex coupé** (`-s danger-full-access`) : imbriqué dans celui du banc, toutes ses commandes échoueraient (aucune détection, `codex-rs/sandboxing/src/seatbelt.rs:57-61`). L'isolement est celui du banc pour les deux bras. Le prompt de Codex annonce alors un accès complet (`permission_profile type="disabled"`).
5. **`apps`, `plugins`, `tool_suggest` coupés** : premier corps identique à celui de Codex livré au banc de capture (`tools/codex_first_body.py`, 2 tâches). Sous le réseau du banc, les apps échouent de toute façon (`chatgpt.com` refusé). **`image_generation` gardé** : le couper retire `image_gen__imagegen` de la description d'`exec`. Le tool reste annoncé ; son appel échouerait (chemin refusé par le proxy).
6. **Variables de proxy retirées des commandes du modèle** (`shell_environment_policy.exclude = ["*PROXY*"]`), comme pour le kernel et `bash()` de yusAgent (sonde : aucune variable de proxy des deux côtés).
7. **`--trust`** : charge l'AGENTS.md du dépôt (`codex-rs/core/src/agents_md.rs:65`), comme yusAgent (`crates/pa-core/src/resources/mod.rs:16`). Vérifié au banc de capture sur `py-tldextract-383` des deux côtés.
8. **Appels refusés au démarrage** : 3 CONNECT vers `chatgpt.com` par session, pendant `thread/start`, refusés par le proxy CONNECT (`chatgpt.com` hors liste). Chemins non journalisés (un CONNECT ne porte que l'hôte) : cause non identifiée. Codex livré les ferait aboutir ; effet inconnu.
9. **Différences de défaut, non corrigées** (chaque harness tel que livré) : résumé de raisonnement `auto` pour yusAgent, `none` pour Codex Luna ; `parallel_tool_calls` `true` pour yusAgent, `false` pour Codex (Responses Lite) ; Codex envoie ses tools dans un item d'entrée (`additional_tools`) et la date et le fuseau dans son contexte d'environnement.

## Table de prix

`gpt-6-luna` est dans `bench/smoke/prices.json` (version `2026-11-04`) : source `https://developers.openai.com/api/docs/models/gpt-6-luna`, relevée le 04/11. **Revérifiée le 14/11/2026** sur la même page : entrée 0,10 $, entrée en cache 0,01 $, écriture en cache 0,125 $, sortie 0,50 $ par million de tokens ; au-delà de 272 000 tokens d'entrée, 2 × l'entrée et le cache, 1,5 × la sortie. Tous les champs utilisés par le banc correspondent (court et long). `cache_write_12h` (0,25 et 0,50 $) n'est pas sur la page ; aucun des deux bras ne demande la rétention de 12 h, il n'entre dans aucun coût. Même tarif pour les deux bras : le coût mesure l'usage, pas la facturation de l'abonnement.

## Preuves sans modèle (14/11/2026)

Commandes, toutes au banc de capture ou sur des journaux déjà faits :

```bash
bash bench/smoke/tools/install-codex.sh 0.169.0
# bras B au banc de capture (tâches d'exemple), config du banc puis config livrée
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/codex-0.169.0/codex --model gpt-6-luna --settings bench/smoke/settings/codex-0.169.0.json --capture --out <X>
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/codex-0.169.0/codex --model gpt-6-luna --settings bench/smoke/settings/codex-0.169.0-shipped.json --capture --out <Y>
python3 -I bench/smoke/tools/codex_first_body.py <Y>/<run> <X>/<run>
# AGENTS.md, effort, tools du premier corps
python3 -I bench/smoke/tools/codex_first_body.py --show <run --capture --tasks py-tldextract-383>
# A/B entrelacé complet au banc de capture, puis la règle de verdict
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632 80e20ea5f353ff3855e30d19c0df7cc7d1cc9632 --b bench/smoke/settings/codex-0.169.0.json --codex-dir ~/.cache/yusagent-bin/codex-0.169.0 --model gpt-6-luna --tasks py-example-001,node-example-001 --passes 2 --seed 7 --capture
python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --verdict ya14-codex
# changements communs sans effet sur YA-03 : chaque bras recalculé avec main et avec la branche
python3 -I bench/smoke/ab/ya14-codex-rereport-check.py <copies recalculées par main> <copies recalculées par la branche>
python3 -I -m unittest discover -s bench/smoke/tests -v
```

Résultats : premier corps identique entre config livrée et config du banc (`apps`, `plugins`, `tool_suggest` coupés) ; différent si `image_generation` est coupé (tool imbriqué retiré) ; AGENTS.md chargé des deux côtés ; effort `high` ; WebSocket direct vers le reverse proxy (aucun CONNECT vers 127.0.0.1 : `NO_PROXY` respecté) ; prewarm puis 5 appels en suite de conversation sur les tâches d'exemple, 0 requête non appariée ; 120 bras de YA-03 recalculés, 0 écart.

## Commandes du run réel (Yusuf)

Login une fois, CODEX_HOME dédié (ne pas copier `~/.codex/auth.json` : le rafraîchissement fait tourner le jeton, l'autre copie serait refusée, `codex-rs/login/src/auth/manager.rs:235, 3742-3795`) :

```bash
mkdir -p ~/.cache/yusagent-smoke/codex-home
printf 'cli_auth_credentials_store = "file"\n' > ~/.cache/yusagent-smoke/codex-home/config.toml
CODEX_HOME=~/.cache/yusagent-smoke/codex-home ~/.cache/yusagent-bin/codex-0.169.0/codex login --device-auth
```

Chauffe (une tâche, corps journalisés, ne compte pas) :

```bash
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/codex-0.169.0/codex --model gpt-6-luna --effort high --settings bench/smoke/settings/codex-0.169.0.json --tasks py-shortuuid-118 --log-bodies
```

A/B (après relecture de la chauffe) :

```bash
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632 <sha de main> --a bench/smoke/settings/default.json --b bench/smoke/settings/codex-0.169.0.json --codex-dir ~/.cache/yusagent-bin/codex-0.169.0 --model gpt-6-luna --effort high --passes 3
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-<horodatage>/ab.json --verdict ya14-codex
```
