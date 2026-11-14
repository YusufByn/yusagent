# Passation du 14/11/2026 : YA-14, comparaison yusAgent contre Codex CLI

Note écrite pour une nouvelle session, à la fin de la session qui a écrit l'adaptateur Codex CLI du smoke bench. Ticket CDC : YA-14 (journal du CDC, décision du 14/11/2026 « Comparaison yusAgent contre Codex CLI au smoke bench, avant YA-09 »). Plan et règle de verdict : `bench/smoke/ab/ya14-codex-plan.md` (sur la branche de la PR #34). **Aucun appel au modèle n'a été fait, aucune chauffe n'est lancée.** La chauffe et l'A/B sont lancés par Yusuf, jamais par Claude.

## État de la PR #34

- Branche `ya-14-codex`, **brouillon**, vers `main` (`91e821f7a`, merge de #33).
- Dernier commit : `70b95ba8cbdda682ea2e3f8b1e90d28508b9fcd8` (« YA-14: journal du CDC, adaptateur Codex CLI et règle de verdict de la comparaison »).
- 8 commits, un sujet chacun :
  - serveur de capture : requêtes de Codex ;
  - reverse proxy et cache théorique : changements communs aux deux bras ;
  - bras Codex CLI ;
  - A/B et verdict ;
  - tests ;
  - plan et README ;
  - journal du CDC, une entrée YA-03 et une entrée YA-14.
- Aucun changement du binaire yusAgent, du prompt ni des tools : pas d'impact sur le préfixe.
- Aucun fichier upstream touché (`bench/` et `docs/` du fork) : rien dans `docs/fork-changes.md`.
- `ci.yml` ne lance pas les tests du banc.

## Fait et vérifié (sans modèle)

- **Codex CLI 0.169.0** (tag `rust-v0.169.0`, commit `d32063f96c2dff1dc96af839b3952abb6acfeda4`, dernière stable GitHub et `latest` npm au 14/11) :
  - installé par `bench/smoke/tools/install-codex.sh` dans `~/.cache/yusagent-bin/codex-0.169.0/` ;
  - contenu : `codex`, `codex-code-mode-host`, `models.json` du tag, `manifest.json` ;
  - sha256 des archives et du catalogue épinglés dans le script ;
  - `models.json` du tag identique octet pour octet à celui de `69231e1`.
- **Bras Codex** (`bench/smoke/codex_arm.py`, `bench/smoke/codex/config.toml.in`, réglage `bench/smoke/settings/codex-0.169.0.json`, `"harness": "codex"`, `featureCuts` `apps`, `plugins`, `tool_suggest`) :
  - lancement : `codex exec --json --skip-git-repo-check --trust -s danger-full-access -m <modèle> -c model_reasoning_effort=<effort>` ;
  - sandbox : `sandbox-agent.sb` (`AGENT_DIR` = CODEX_HOME, `BIN_DIR` = dossier de Codex), mêmes proxys et garde-fou que yusAgent ;
  - reverse proxy limité à `/backend-api/codex/responses` ;
  - usage par réponse lu dans le fichier de session (`token_usage_record`), converti en `sessions/<thread>.usage.ndjson` ;
  - prewarm lu au proxy (appel `prewarm`, annexe) ;
  - statut, erreurs et edits (`file_change`) lus dans la sortie `--json`.
- **Changements communs aux deux bras** :
  - le reverse proxy note `generate` et la fin de chaque réponse (`event: response`, compteurs seuls ; `response_error`) ;
  - le cache théorique compte en rupture un appel de boucle en corps complet sur WebSocket.
  - Preuve : les 120 bras des A/B de YA-03 ont été recalculés par `--rereport` avec `main` puis avec la branche (`bench/smoke/ab/ya14-codex-rereport-check.py`). **0 écart** ; 120 appels `premier` et 1 543 `continuation` des deux côtés, 0 rupture.
- **`ab.py` et `ab_stats.py`** :
  - bras Codex (`--codex-dir`) ;
  - incidents propres à Codex : tout `error` de `--json`, relancé ou non, et `turn.failed` ;
  - erreur ou réponse en échec lue au proxy, pour les deux bras ;
  - `--verdict ya14-codex` (règle du plan).
- **Tests** : 35 tests sans modèle (`python3 -I -m unittest discover -s bench/smoke/tests -v`), verts sous Python 3.11 et 3.14.
- **Prix** : la ligne `gpt-6-luna` de `prices.json`, revérifiée le 14/11 sur `https://developers.openai.com/api/docs/models/gpt-6-luna`, correspond pour tous les champs utilisés. `cache_write_12h` est absent de la page et n'est utilisé par aucun bras.

## Résultats du serveur de capture

- **Diff du premier corps** (`tools/codex_first_body.py`, config livrée contre config du banc, 2 tâches d'exemple) :
  - identique avec `apps`, `plugins` et `tool_suggest` coupés ;
  - **différent** avec `image_generation` coupé : le tool imbriqué `image_gen__imagegen` est retiré. `image_generation` est donc gardé.
- **Tools vus par le modèle (Luna)** :
  - premier niveau, dans un item d'entrée `additional_tools` (pas de champ `tools`) : `exec` (JavaScript, libre), `wait`, `request_user_input`, `request_user_input_async`, `sleep`, espace `collaboration` (multi-agent) ;
  - imbriqués dans `exec` : `apply_patch`, `exec_command`, `write_stdin`, `view_image`, buts, ressources MCP, `clock__curr_time`, `image_gen__imagegen`.
- **Prewarm** :
  - forme : `generate: false`, entrée vide, aucun tool ;
  - sur les tâches d'exemple, la requête suivante continue le prewarm (`previous_response_id`) en portant tout le contexte, puis 4 appels en deltas ;
  - son usage n'est pas dans le fichier de session (`codex-rs/core/src/client.rs:2569-2577` au tag) ;
  - 0 requête non appariée.
- **AGENTS.md** : chargé avec `--trust` sur `py-tldextract-383` (item utilisateur « AGENTS.md instructions for … »). yusAgent le charge aussi (vérifié avec le binaire `80e20ea`).
- **Effort** : `reasoning.effort` à `high`, `context: all_turns`.
- **`NO_PROXY`** : le WebSocket va directement au reverse proxy, aucun CONNECT vers 127.0.0.1.
- **Sonde** :
  - réseau coupé (curl direct : code 6 ; par le proxy CONNECT : 56) ;
  - aucune variable de proxy dans le shell de Codex ;
  - aucune lecture hors workspace.
- **3 CONNECT vers `chatgpt.com` par session**, refusés, pendant `thread/start` : cause non identifiée (un CONNECT ne porte que l'hôte, chemins absents des journaux de debug).
- **Premier corps** : environ 31 Ko pour yusAgent, 57 Ko pour Codex.
- **Écarts de défaut** :
  - résumé de raisonnement `auto` (yusAgent) contre `none` (Codex Luna) ;
  - appels d'outils parallèles activés contre désactivés.
- **A/B entrelacé complet** au banc de capture (2 tâches × 2 passes, graine 7) jusqu'au verdict : la chaîne tourne.

## Liste `is_openai()` (nom de provider « OpenAI », `codex-rs/model-provider-info/src/lib.rs:709-711` au tag)

Notre provider s'appelle `yusagent-bench` : `is_openai()` vaut faux.

| Site (au tag) | Effet | Notre montage (WebSocket, exec, Luna) |
|---|---|---|
| `model-provider-info/src/lib.rs:713-720`, `model-provider/src/provider.rs:256-282` | routage d'espace de travail avant chaque requête | **s'applique** : pas de routage |
| `model-provider/src/capabilities.rs:36` | compaction distante v2 | **s'applique** : compaction locale (incident) |
| `core/src/client.rs:1231-1249` | `encrypted_content` gardé dans les items rejoués | **s'applique** en corps complet : retiré |
| `ext/image-generation/src/extension.rs:43` | génération d'images (aussi avec `requires_openai_auth`) | **s'applique** : tool annoncé, appel refusé par le proxy |
| `core/src/client.rs:1940` | zstd du corps HTTP | non : WebSocket |
| `core/src/client.rs:1384` | `codex_responses_headers` | non : guardian seul |
| `core/src/client.rs:661` | changement d'effort en cours | non pour Luna |
| `core/src/client.rs:1253`, `core/src/session/turn.rs:2937` | résumés concurrents | non : fonction désactivée par défaut |
| `core/src/client.rs:1259` | `output.encrypted_content` | non : fonction désactivée par défaut |
| `ext/web-search/src/extension.rs:47` | recherche web | non : coupée |
| `ext/history-notes/src/extension.rs:60` | notes d'historique | non : désactivé par défaut |
| `core/src/compact_model_fallback.rs:30` | repli de compaction | non sans compaction |
| `model-provider/src/models_endpoint.rs:56, 319` | catalogue distant | non : catalogue figé |
| `model-provider/src/auth.rs:458` | FedRAMP | non |
| `core/src/realtime_conversation.rs:2161`, `app-server/src/turn_cost_worker.rs:121`, `app-server/src/thread_sample.rs:225`, `ext/guardian-v2/src/async_scorer/sampler/connection_pool.rs:459`, `tui/src/chatwidget/model_popups.rs:90` | temps réel, coût OTel, échantillons, guardian, TUI | non pour `codex exec` |

## Biais connus (détail : plan, « Écarts avec chaque harness tel que livré »)

1. Provider personnalisé : les quatre lignes « s'applique » du tableau ci-dessus.
2. Catalogue figé ; recherche web et analytics coupés ; reconnexions sans fin coupées ; délai d'inactivité de Codex à 900 s (le garde-fou du banc, 300 s, décide).
3. Sandbox de Codex coupé (`danger-full-access`) : le prompt de Codex annonce un accès complet. L'isolement est celui du banc pour les deux bras.
4. `image_gen__imagegen` annoncé mais inutilisable (chemin refusé).
5. 3 CONNECT `chatgpt.com` refusés au démarrage, effet inconnu.
6. Edits au premier essai côté Codex : comptés depuis les items `file_change`, une borne haute. Cellules en erreur côté Codex : non mesurées (`null`).
7. Différences de défaut non corrigées : résumé de raisonnement, appels d'outils parallèles, tools en item d'entrée, date et fuseau dans le contexte de Codex.
8. Codex 0.169.0 met les 12 modèles en `tool_mode: code_mode_only` (porté au journal du CDC : justification d'`editTool: native` révisée, verdict de l'A/B 1 inchangé, pas de changement de réglage).

## Incident de la session

Un premier essai de Codex contre le serveur de capture a tourné **hors sandbox**. Codex a contacté le MCP Apps sur `chatgpt.com`, puis `auth.openai.com` (rafraîchissement du jeton), avec l'identifiant factice seulement ; les deux ont été refusés en 401. Aucun vrai jeton, aucun appel au modèle. Toutes les sessions suivantes ont tourné sous `sandbox-exec`. Le point 1 du prochain travail en vient.

## Points ouverts

- Cause des 3 CONNECT `chatgpt.com` au démarrage.
- Usage et facturation du prewarm au vrai backend : à lire dans `proxy.ndjson` de la chauffe (`event: response`, `generate: false`).
- `hosts/codex.txt` (`auth.openai.com`) vient de la lecture du code (`codex-rs/login/src/auth/manager.rs:241` au tag), pas d'une observation.
- Pas de baseline `gpt-6-luna` ; les 30 bras A de l'A/B pourront en servir.

## Emplacements

- **Clone de Codex** (sources, lecture seule, sans build) : `/private/tmp/claude-501/-Users-yusufyuss-Desktop-yusagent/340a1bb4-663d-44d2-8176-712ad391f867/scratchpad/codex/src` (`git clone --filter=blob:none` de `openai/codex`, tag `rust-v0.169.0` extrait). Catalogue de `69231e1` : `.../scratchpad/codex/models-69231e1.json`. Le scratchpad est propre à la session : une nouvelle session refait le clone (`git clone --filter=blob:none https://github.com/openai/codex.git` puis `git checkout rust-v0.169.0`).
- **Runs de capture de la session** (hors dépôt) : `.../scratchpad/runs*` et `.../scratchpad/ab/`.
- **CODEX_HOME du banc** :
  - capture : `~/.cache/yusagent-smoke/capture-codex-home` ;
  - sonde : `probe-codex-home` ;
  - réel : `codex-home`, pas encore créé (login de Yusuf).

## Prochain travail

Prochain travail, sur ya-14-codex, PR toujours en brouillon :
1. Garde-fou dans codex_arm.py / run.py : refuser de lancer Codex hors sandbox-exec, ou avec un CODEX_HOME autre que celui du banc (jamais ~/.codex). Test sans modèle.
2. codex_arm.py réécrit-il le config.toml du CODEX_HOME ? Si oui, vérifier qu'il garde cli_auth_credentials_store = "file" et que le printf du login n'entre pas en conflit.
3. Au rapport, par lancement du bras Codex : nombre de requêtes en corps complet (encrypted_content retiré dans ce cas, biais contre Codex).
4. Donner la commande de chauffe yusAgent sur gpt-6-luna avec default.json (une tâche avec --log-bodies, puis continue_session.py).
Puis s'arrêter avant la chauffe.
