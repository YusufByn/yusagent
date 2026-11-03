# Écarts avec upstream

Toute modification d'un fichier upstream, et tout ajout dans une zone gérée par upstream, est listé ici (CLAUDE.md règle 2).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `.github/workflows/ya-build-macos-arm64.yml` | ajout | Binaire macOS arm64 de HEAD en artefact CI pour les mesures du banc de capture, sans build natif local. Copie réduite du job `build-darwin` de `continuous.yml` (qui ne tourne que sur `main` et construit 4 cibles). Autorisé par Yusuf pour ce seul but. | YA-00 |
| `crates/pa-core/src/settings/fork_defaults.rs` | ajout | Défauts du fork pour les fonctions hors MVP (constantes, réglage pour couper les skills `agent-message` / `rlm-heartbeat`) et leurs tests. Les getters upstream lisent ces constantes au lieu de leurs littéraux : un cherry-pick qui touche l'un d'eux entre en conflit de façon visible. | YA-01 |
| `crates/pa-core/src/settings/mod.rs` | modif. (1 ligne) | Déclare `fork_defaults`. | YA-01 |
| `crates/pa-core/src/settings/interactive_settings.rs` | modif. | `get_harness_refinements` : défaut `fork_defaults::HARNESS_REFINEMENTS` au lieu de `true`. | YA-01 |
| `crates/pa-core/src/session_engine/refine.rs` | modif. | `AutoRefineGates::default` : `enabled` et `compact` lus dans `fork_defaults`. Test des défauts mis à jour. | YA-01 |
| `crates/pa-core/src/settings/manager.rs` | modif. | `get_telemetry_enabled` : sans aucun réglage, renvoie `fork_defaults::TELEMETRY_ENABLED`. Dès qu'une portée fixe la valeur, la règle upstream s'applique (toutes les portées qui la fixent doivent dire oui). | YA-01 |
| `crates/pa-core/src/session_engine/telemetry/status.rs` | modif. | `TelemetrySwitch::Default` suit `fork_defaults::TELEMETRY_ENABLED` ; le motif affiché devient « off by default ». | YA-01 |
| `crates/pa-core/src/settings/types.rs` | modif. | `BundledSkillsSettings` : champs `agentMessage` et `rlmHeartbeat` (actifs par défaut, `false` coupe le skill). | YA-01 |
| `crates/pa-core/src/packages/resolve/auto.rs` | modif. (5 lignes) | Ajoute l'exclusion des skills coupés par réglage à côté de celle de websearch. | YA-01 |
| Tests upstream | modif. | Voir « Tests upstream touchés » ci-dessous. | YA-01 |
| `.github/workflows/ci.yml` | modif. (1 ligne + commentaire) | Étape `Prepare pinned TS frame comparison binary` : la release `v0.9.8` est téléchargée depuis `PrimeIntellect-ai/prime-agent` au lieu du dépôt courant. Voir ci-dessous. | YA-01 |
| `.changes/ya-01.md` | ajout | Fragment de changelog exigé par `changelog-fragment.yml` pour tout changement livré. | YA-01 |
| `crates/pa-core/src/kernel/env_allowlist.rs` | ajout | Environnement du kernel en liste blanche : base intégrée, réglage `kernel.envAllowlist`, refus des identifiants provider, tests unitaires. Voir « Environnement du kernel » ci-dessous. | YA-02 |
| `crates/pa-core/src/kernel/mod.rs` | modif. (1 ligne) | Déclare `env_allowlist`. | YA-02 |
| `crates/pa-core/src/kernel/manager/startup.rs` | modif. | `kernel_spawn_env` part de `env_allowlist::inherited_env` au lieu de `std::env::vars()` (`:535`) et retire les identifiants provider en dernier (`:546`). Test `kernel_spawn_env_is_allowlisted` ajouté. | YA-02 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (4 lignes) | `kernel_provisioner` reçoit `env_allowlist` (`:257`) et appelle `apply_user_allowlist` sur l'environnement qu'il injecte (`:292`). | YA-02 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (3 lignes) | Lit `kernel.envAllowlist` avant le déplacement des réglages (`:281`) et le passe à `kernel_provisioner` (`:512`). | YA-02 |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Champ `kernel` de `Settings` (`:317`). | YA-02 |
| `crates/pa-core/src/settings/load.rs` | modif. (2 lignes) | `kernel` ajouté aux clés connues (`:66`) : une valeur de mauvais type compte comme non réglée, comme les autres clés. | YA-02 |
| Tests et checks upstream | modif. | Voir « Tests upstream touchés (YA-02) » ci-dessous. | YA-02 |
| `.changes/ya-02.md` | ajout | Fragment de changelog. | YA-02 |
| `crates/pa-core/tests/prefix_stability.rs` | ajout | Test de préfixe du cache de prompt : session réelle (`create_session`, adaptateur provider réel, kernel réel) contre un serveur HTTP local, Anthropic Messages et OpenAI Responses, trois prompts avec tool calls. Voir « Test de préfixe » ci-dessous. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/environment_block.rs` | ajout | Bloc `<environment>` (date UTC, cwd) ajouté à la conversion vers le LLM, jamais stocké, et ses tests. Voir « Prompt système stable » ci-dessous. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (4 lignes) | Déclare `environment_block` et `cache_retention`. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | `convert_to_llm` enveloppé par `environment_block::wrap_convert_to_llm` ; `conversation_log` passé à `kernel_provisioner` ; `cacheRetention` lu avant le déplacement des réglages et `stream_fn` enveloppé par `cache_retention::scoped_stream_fn`. | T03 (YA-04) |
| `crates/pa-core/src/prompts/session_paths.rs` | ajout | Variables du kernel `PRIME_AGENT_CONVERSATION_LOG` et `PRIME_AGENT_BUNDLED_SKILLS_DIR`, lignes statiques du prompt, `<location>` des skills embarqués relative à la variable, et leurs tests. | T03 (YA-04) |
| `crates/pa-core/src/prompts/mod.rs` | modif. (2 lignes) | Déclare `session_paths`. | T03 (YA-04) |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. | `environment_section` : les lignes `Current date`, `Working directory`, `Conversation log` remplacées par les lignes fixes de `session_paths` ; `today()` retiré (plus d'appelant). Tests adaptés. | T03 (YA-04) |
| `crates/pa-core/src/skills/mod.rs` | modif. | `format_skills_for_prompt` : `<location>` par `session_paths::skill_location`, et une ligne d'explication quand une location commence par la variable. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (5 lignes) | `kernel_provisioner` reçoit `conversation_log` et appelle `session_paths::insert_kernel_env`. | T03 (YA-04) |
| Tests upstream | modif. | Voir « Tests upstream touchés (T03) » ci-dessous. | T03 (YA-04) |
| `.changes/ya-03.md` | ajout | Fragment de changelog. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/cache_retention.rs` | ajout | Réglage `cacheRetention` (`"short"`, `"long"`), priorité à `PI_CACHE_RETENTION`, portée de session par variable de tâche, et ses tests. Voir « Réglage du TTL » ci-dessous. | T03 (YA-04) |
| `crates/pa-core/src/session_engine/provider_adapter.rs` | modif. (3 lignes) | `stream_once` : `cache_retention` lu par `cache_retention::current()` au lieu de `None` en dur. | T03 (YA-04) |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Champ `cache_retention` de `Settings`. | T03 (YA-04) |
| `crates/pa-core/src/settings/load.rs` | modif. (2 lignes) | `cacheRetention` ajouté aux clés connues : une valeur invalide compte comme non réglée. | T03 (YA-04) |

## Désactivations par défaut (YA-01)

Session lancée sans `settings.json` ni variable d'environnement. Chaque ligne se réactive dans `settings.json` (global, `~/.prime/agent/settings.json`).

| Fonction | Défaut fork | Réactivation | Où |
|---|---|---|---|
| Télémétrie produit (et notice au premier lancement) | coupée | `"telemetry": {"enabled": true}` (ou `PRIME_AGENT_TELEMETRY=1`) | `fork_defaults.rs:17`, `manager.rs:850`, `status.rs:29` |
| Continual Harness (création de raffinements, digest du harness dans le contexte) | coupé | `"harnessRefinements": true` | `fork_defaults.rs:7`, `interactive_settings.rs:17` |
| `/refine` automatique (par intervalle de tours) | coupé | `"autoRefine": {"enabled": true}` (et `harnessRefinements`) | `fork_defaults.rs:10`, `refine.rs:42` |
| `/refine` automatique après compaction | coupé | `"autoRefine": {"compact": true}` (et `harnessRefinements`) | `fork_defaults.rs:13`, `refine.rs:44` |

Skills `agent-message` et `rlm-heartbeat` : **actifs par défaut**, comme upstream. La couche statique `core.md` les documente (`crates/pa-core/src/prompts/layers/core.md:68`, `:156`), et les enfants de `rlm.spawn` répondent au parent par `agent_message.send` (`prime-agent-runtime/src/rlm/harness.py:1219-1221`). Leur sort sera décidé dans YA-09. Un réglage permet déjà de les couper : `"bundledSkills": {"agentMessage": false}` ou `{"rlmHeartbeat": false}` (`fork_defaults.rs`, `disabled_bundled_skill_overrides`). Un skill coupé disparaît de l'inventaire et des imports du kernel.

Ce qui n'émet plus rien quand le réglage est faux :

- Télémétrie : le client n'envoie rien tant que le commutateur est faux (`crates/pa-core/src/session_engine/telemetry.rs:1162-1164`, porte de livraison sur tous les puits, miroir local compris) ; les compteurs ne comptent pas (`telemetry.rs:1185-1191`, `crates/pa-daemon/src/supervisor/notes.rs:42-48`) ; le mode print ne crée même pas de client (`crates/pa-cli/src/print_runtime.rs:730`) ; la notice n'est ni imprimée (`crates/pa-cli/src/telemetry_notice.rs:15`) ni affichée dans le TUI (`crates/pa-cli/src/client_settings.rs:297`).
- Raffinements : porte unique `crates/pa-core/src/session_engine/refinement_gate.rs:6-8`, lue à chaque requête, à la planification, à l'application et à la compaction.

Réglages cherchés sans résultat :

- **Vérification de version au lancement** : il n'y en a pas dans le portage Rust. `latest_release` (`crates/pa-core/src/update/release.rs:71`) n'est appelé que par les commandes explicites `prime-agent update` (`crates/pa-cli/src/update_flow/plan.rs:140`) et `prime-agent update --check` (`crates/pa-cli/src/installer_update.rs:269`). La vérification des mises à jour de packages (`crates/pa-core/src/packages/update.rs:54`) n'a aucun appelant. Aucun réglage ajouté.
- **Commande `schedule`** : aucun réglage dédié dans `Settings` (`crates/pa-core/src/settings/types.rs`). C'est une sous-commande CLI explicite (`crates/pa-cli/src/command_registry.rs:92`), qui ne s'exécute que si on l'appelle. Rien de modifié.

## Tests upstream touchés (YA-01)

Règle : aucun test supprimé ni affaibli.

- **Setup** : un test qui exerce une fonction désormais coupée l'active explicitement dans son setup (même scénario, mêmes assertions).
- **Défaut** : seuls les tests qui vérifient la valeur par défaut elle-même changent leur valeur attendue.
- **Renfort** : quand le nouveau défaut rendait une assertion non discriminante (elle passerait même si le code ignorait le réglage), un cas est ajouté pour la rendre de nouveau discriminante.

Chaque modification porte un commentaire `yusAgent (YA-01)`.

| Fichier de test | Test(s) | Nature | Réglage activé ou valeur attendue |
|---|---|---|---|
| `crates/pa-core/src/settings/persistence_tests.rs` | `harness_refinements_persist_without_clobbering_other_settings` | défaut + renfort | attendu initial `false` ; ajout d'un retour à `false` après `true` pour prouver la relecture |
| `crates/pa-core/src/settings/persistence_tests.rs` | `harness_refinements_ignore_project_overrides_and_resolve_global_wrong_types` | défaut + renfort | mauvais type → `false` ; ajout du cas `"true"` (chaîne) → `false` |
| `crates/pa-core/src/session_engine/refine.rs` | `auto_refine_gates_resolve_the_ts_defaults_and_clamps` | défaut + renfort | `enabled`/`compact` par défaut `false` ; ajout du cas `Some(true)` |
| `crates/pa-core/src/session_engine/refinement_gate.rs` | `live_request_gate_filters_saved_rows_and_preserves_quoted_user_content` | setup | `harnessRefinements: true` |
| `crates/pa-core/src/session_engine/telemetry/tests.rs` | `a_disable_on_a_fresh_install_never_mints_the_install_id` | renfort | ajout : la valeur enregistrée est `Some(false)` |
| `crates/pa-cli/src/mode.rs` | `telemetry_defaults_on` → `telemetry_defaults_off` | défaut | désactivée par défaut ; vérifie aussi la réactivation par `telemetry.enabled` |
| `crates/pa-cli/tests/telemetry_command_e2e.rs` | `status_on_and_off_round_trip` | défaut | « Telemetry is off (off by default). » |
| `crates/pa-cli/src/interactive_mode/tests.rs` | `settings_sink_reports_an_aborted_flow_once` | setup | `telemetry.enabled: true` |
| `crates/pa-cli/src/interactive_mode/telemetry.rs` | `each_tui_exit_reports_only_its_own_run` | setup | `telemetry.enabled: true` |
| `crates/pa-daemon/src/agent_engine/tests.rs` | `write_compaction_settings`, helper de `faux_engine_with_settings` (45 appels dans `pa-daemon`, dont `agent_engine/tests/compaction.rs`, `agent_engine/tests/abort.rs`, `agent_engine/turn/boundary.rs`, `compact_autorefine.rs`) | setup | `telemetry.enabled`, `harnessRefinements`, `autoRefine.enabled`, `autoRefine.compact` à `true` : les conditions upstream de ces tests. La télémétrie reste coupée hors `telemetry_opt_in` par `DO_NOT_TRACK=1` (`.cargo/config.toml`) |
| `crates/pa-daemon/src/agent_engine/tests/abort.rs` | `retried_run_restarts_with_its_own_agent_frames` (setup propre) ; `settled_turn_emits_the_run_agent_end_payload` (via le helper) | setup | `harnessRefinements: true` (ligne du digest attendue) |
| `crates/pa-daemon/src/agent_engine/turn/boundary.rs` | `requested_refinement_emits_its_durable_outcome_before_completion` | setup (via le helper, fichier non modifié) | `harnessRefinements: true` |
| `crates/pa-daemon/src/compact_autorefine.rs` | `threshold_compaction_arms_the_review_off_the_turn_path`, `the_declining_review_stamps_the_cooldown`, `manual_compaction_arms_and_consumes_the_round`, `refine_session_command_emits_the_refine_events` | setup (via le helper, fichier non modifié) | `harnessRefinements`, `autoRefine.enabled`, `autoRefine.compact` |
| `crates/pa-daemon/src/worker/turn_stream_tests/broadcast.rs` | `a_retried_turn_broadcasts_one_agent_end_per_run` | setup | `harnessRefinements: true` |
| `crates/pa-daemon/tests/provider_failure_e2e.rs` | `setup_with_rejection` (helper ; échec observé : `provider_failure_recovered_by_retry_settles_the_turn`) | setup | `harnessRefinements: true` |
| `crates/pa-cli/tests/print_runtime_e2e.rs` | `print_mode_json_streams_the_harness_digest_pair` | setup | `harnessRefinements: true` |
| `crates/pa-cli/src/print_boundary/tests.rs` | nouveau helper `refining_settings()` pour `requested_refinement_streams_rows_and_refine_complete`, `compact_trigger_auto_refine_streams_at_the_next_boundary`, `overflow_retry_compact_trigger_streams_the_ts_surface`, `compact_trigger_drains_at_disposal_off_the_stream` | setup | `harnessRefinements`, `autoRefine.enabled`, `autoRefine.compact` |
| `crates/pa-core/src/session_engine/auto_refine_trigger.rs` | `session()` (helper des tests du déclencheur) | setup | portes `enabled` et `compact` explicites au lieu de `AutoRefineGates::default()` |
| `crates/pa-core/src/session_engine/compaction_outcome_tests.rs` | `refine_factory_edits_follow_the_agent_dir_opt_in_setting` | setup | `harnessRefinements: true` dans le même `settings.json` que `factory.enabled` |
| `crates/pa-core/src/session_engine/refine.rs` | `factory_edits_in_a_refinement_respect_the_opt_in_gate`, `factory_gate_decides_on_the_apply_time_setting_not_a_planning_snapshot` | setup | `harnessRefinements: true` dans chaque écriture de `settings.json` (seul `factory.enabled` bascule) |
| `crates/pa-core/tests/turn_boundary_kernel.rs` | `turn_boundary_host_requests_round_trip_through_a_real_kernel` | setup | `harnessRefinements: true` |
| `crates/pa-daemon/src/model_allowlist.rs` | `refusal_telemetry_honors_both_opt_outs`, `refusal_telemetry_dedupes_repeated_resolves`, `refusal_telemetry_rebinds_when_the_cwd_moves` | setup | `telemetry.enabled: true` |
| `crates/pa-daemon/src/supervisor/tests.rs` | `the_exit_flush_sends_the_partial_daemon_event_summary` | setup | `telemetry.enabled: true` |
| `scripts/check_refine_errors.py` | check de cycle de vie | setup | `harnessRefinements: true` |
| `scripts/check_refine_request_headers.py` | check de cycle de vie | setup | `harnessRefinements: true` |
| `scripts/check_minimum_reasoning.py` | check de cycle de vie | setup | `harnessRefinements: true` |

Nouveaux tests du fork : `crates/pa-core/src/settings/fork_defaults.rs` (défauts sans `settings.json`, bascule de chaque fonction par réglage, réglages des skills indépendants, portées de la télémétrie).

Les tests qui activaient `agent-message` ou `rlm-heartbeat` dans un état intermédiaire de YA-01 ont été remis à l'état upstream quand ces skills sont redevenus actifs par défaut : `session_engine/engine/tests.rs`, `print_inline_subagents_e2e.rs`, `rlm_children_replacement_e2e.rs`, `check_late_message_receipt.py`, `check_restore_notice.py`. Ils ne figurent plus dans les écarts.

## Instabilité probable : `agent_family_e2e` (YA-01)

`crates/pa-daemon/tests/agent_family_e2e.rs`, tests `parent_child_agent_message_round_trip_end_to_end` et `family_edges_never_cross_families_end_to_end`.

Historique des runs :

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 41609183862 | `f4809b6` | 1 | réussi |
| 41612185670 | `d5a18c7` | 1 | échec |
| 41612185670 | `d5a18c7` | 2 | réussi, aucun changement de code |

Symptômes de l'échec :
- `{"messages":[]}` pour le parent, à `agent_family_e2e.rs:538` (délai de 20 s) ;
- un reçu jamais écrit, à `:310` (délai de 30 s) ;
- `Broken pipe (os error 32)` dans le log du daemon.

Aucune dépendance de la messagerie aux réglages coupés par YA-01 :
- Aucune référence à harness, refine ou digest dans `crates/pa-core/src/session_engine/agent_messaging.rs`, `agent_messaging/`, `crates/pa-daemon/src/agent_messaging.rs`, `agent_message_ingest.rs`, `agent_message_broadcast.rs` ou `peer_tickets.rs`. Le « digest » de `peer.rs:202-207` est un hachage de jeton.
- Le `host_request` `agent_message.send` (`crates/pa-core/src/session_engine/runtime_wiring.rs:198`, `:546`) n'a pas de porte de raffinement.
- Seul point commun : l'admission de chaque prompt (`crates/pa-core/src/session_engine/admission.rs:43`, `:142`) appelle `pending_digest_prompt_row`. Raffinements coupés, la fonction renvoie `Ok(None)` aussitôt (`harness_digest.rs:463-465`).

Si ce test retombe sur une PR suivante : enquête.

## CI : comparaison de frames avec le binaire TS (YA-01)

Le shard 1 de `ci.yml` téléchargeait la release `v0.9.8` depuis `${GITHUB_REPOSITORY}`. Le fork n'a pas de release, l'étape échouait et faisait échouer tout le shard.

Choix : télécharger la même archive depuis le dépôt upstream public (`PrimeIntellect-ai/prime-agent`, release `v0.9.8`, asset `prime-agent-0.9.8-linux-x64.tar.gz`). Le sha256 épinglé (`83fb0912…235e2`) reste vérifié dans `ci.yml` et dans les deux checks (`scripts/check_link_frames.py:170`, `scripts/check_ipython_diff_frames.py:192`).

- Aucun test retiré : les checks de frames TS/Rust continuent de tourner.
- Pas de release à publier ni à maintenir sur le fork.
- Dépendance : la CI suppose que la release upstream `v0.9.8` reste publique. Si elle disparaît, le sha256 ne correspondra plus ou le téléchargement échouera, de façon visible.
- Alternative écartée : sauter l'étape et les deux checks hors du dépôt upstream. Plus de lignes modifiées, et deux tests en moins.

## Environnement du kernel (YA-02)

Décision du 31/10/2026 : les secrets ne sont jamais transmis au kernel ni à `bash()`. Avant YA-02, `kernel_spawn_env` partait de `std::env::vars()`, donc de tout l'environnement du harness.

Construction, dans l'ordre :

1. **Au lancement** (`startup.rs:535`) : seules les variables ambiantes de `BUILTIN_ALLOWLIST` (`env_allowlist.rs:39`).
2. **Au câblage de session** (`runtime_wiring.rs:292`) : les variables que le harness injecte lui-même, puis les variables ambiantes autorisées par `kernel.envAllowlist`. Une injection du harness l'emporte sur la variable ambiante de même nom.
3. **En dernier** (`startup.rs:546`) : retrait de tout identifiant provider, quelle qu'en soit la source.

Règles :

- **Base intégrée**.
  - Système : `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TERM`, `LANG`, `LC_*`, `TZ`, `TMPDIR`, et les équivalents Windows.
  - Lues par le runtime et ses skills : `PRIME_AGENT_CODING_AGENT_DIR`, `PI_CODING_AGENT_DIR`, `PRIME_AGENT_MACHINES_DIR`, `PRIME_AGENT_BASH_SHELL`, `PRIME_AGENT_BASH_COMMAND_PREFIX`, `PRIME_AGENT_INTERNAL_ORPHAN_PROCESS_JOURNAL`, `PRIME_AGENT_KERNEL_OWNER_PID`, `RLM_SESSION_ID`, `RLM_SESSION_DIR`, `RLM_HARNESS_STATE_DIR`, `RLM_GLOBAL_HARNESS_STATE_DIR`.
  - Python : `PYTHONPATH` (un kernel explicite, `PRIME_AGENT_KERNEL_PYTHON`, trouve les skills par là), `PYTHONUTF8`.
  - uv : emplacements de cache, de Python et de configuration (`UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, …), `XDG_*`, `SSL_CERT_FILE`, `SSL_CERT_DIR`. Les URL d'index uv ne sont pas dans la base : elles peuvent contenir des identifiants.
- **Motifs** : un motif avec `*` (base ou réglage) ne laisse jamais passer un nom d'allure secrète (`KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `PASSWD`, `CREDENTIAL`, `AUTH`, `PRIVATE`, `COOKIE`, `env_allowlist.rs:100`). Une telle variable ne passe que nommée exactement dans `kernel.envAllowlist`.
- **Identifiants provider** : clés d'environnement de chaque provider du catalogue (`pa_ai::env_api_keys`), sources AWS et Google ambiantes, `PRIME_AGENT_TRACES_API_KEY`, jeton du worker daemon (`env_allowlist.rs:115`, `:131`). Jamais transmis, même nommés : avertissement sur stderr (le journal du worker daemon), avec le nom seul.
- **Injections secrètes du harness** : la clé Serper que le harness résout pour le skill websearch (`runtime_wiring.rs:289-291`) n'est transmise que si `SERPER_API_KEY` est nommée dans `kernel.envAllowlist`. Sinon elle est retenue, avec avertissement. Le skill relit de toute façon une clé littérale dans `auth.json` (`skills/websearch/src/websearch/websearch.py:46`).
- **Proxy et index uv** : `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` (et leurs formes minuscules, distinctes hors Windows), `UV_INDEX`, `UV_INDEX_URL`, `UV_DEFAULT_INDEX`, `UV_EXTRA_INDEX_URL` et `PIP_INDEX_URL` ne sont pas dans la base : derrière un proxy ou avec un index privé, les déclarer dans `kernel.envAllowlist`, de préférence par leur nom exact (une URL peut contenir des identifiants).
- **Portée** : `kernel.envAllowlist` se lit dans les réglages fusionnés. Le réglage projet (`.prime/agent/settings.json`) remplace le global (les tableaux remplacent dans `deep_merge`, `merge.rs:7`). Comme `shellPath`, un dépôt non fiable peut donc élargir la liste : relire les réglages du projet avant d'y lancer l'agent.

Ce qui hérite de cet environnement, et de rien d'autre :

- `bash()`, ses sous-process et les shells persistants : `_child_env()` = `os.environ` du kernel plus des réglages non interactifs fixes (`prime-agent-runtime/src/rlm/bash.py:1410-1437`, appelé `:449`, `:457`, `:1553`).
- Les sous-agents `rlm.spawn` : ce sont des sessions dont le kernel passe par le même `kernel_provisioner` → `kernel_spawn_env` (`engine.rs:503`, seul appelant). Le process worker de l'enfant garde l'environnement complet du harness, dont il a besoin pour ses appels provider.
- `bash()` n'a pas de paramètre `env=` (`bash.py:1298`, signature `bash(command, persistent=False)`). Une variable posée par le modèle (`os.environ[...]` ou `VAR=x cmd`) est une valeur qu'il connaît déjà.

### Points relevés pour le sandbox (YA-11), non corrigés

- **Jetons et clés au repos** : `<agentDir>/auth.json`, créé en `0o600` avec écritures atomiques (`crates/pa-core/src/auth/manager.rs:275`, `storage.rs:1`, `:73`). `agentDir` vaut `~/.prime/agent` par défaut (`crates/pa-cli/src/config.rs:7`, `:104-111`). On y trouve les clés API, les jetons OAuth provider et les jetons MCP (`mcp:*`). Le kernel connaît ce chemin (`PRIME_AGENT_CODING_AGENT_DIR`) et le skill websearch lit déjà ce fichier.
- **Clés en variables d'environnement** : le process du harness les garde. Sous Linux, `/proc/<pid>/environ` du harness reste lisible par un process du même utilisateur. Sur macOS, voir la mesure `psenv` du banc de capture (rapport de la PR YA-02).
- **`unsetenv` au démarrage ne suffit pas sur macOS** (relevé T03, macOS 27.0.1, `python3 -I bench/tools/capture/unsetenv_ps.py`) : après `unsetenv` dans le parent, `ps eww <pid du parent>` lancé par un enfant affiche toujours le nom et la valeur. `ps` lit la zone d'environnement initiale du process, que `unsetenv` ne modifie pas. Écraser la valeur en place avant `unsetenv` la masque ; le nom reste visible. Le harness ne fait aujourd'hui ni l'un ni l'autre. Piste pour YA-11 : sandbox qui interdit `ps` sur le harness, ou harness relancé sans les secrets dans son environnement (lus depuis `auth.json`).
- **MCP stdio lancés depuis le kernel** : une référence d'environnement dans `mcpServers.<nom>.env` (`prime-agent-runtime/src/rlm/mcp.py:995-997`) ou un jeton porteur lu depuis l'environnement (`mcp.py:923`, `:970`, `mcp_base.py:149`) n'est plus résolu, sauf si la variable est nommée dans `kernel.envAllowlist`.

## Tests upstream touchés (YA-02)

Même règle que YA-01 : aucun test supprimé ni affaibli. Chaque modification porte un commentaire `yusAgent (YA-02)`.

| Fichier | Test(s) | Nature | Modification |
|---|---|---|---|
| `crates/pa-core/src/kernel/provisioner.rs` | `a_stored_serper_key_reaches_the_kernel_env` | setup + défaut | `kernel.envAllowlist: ["SERPER_API_KEY"]` ; une clé ambiante passe désormais par le câblage, donc l'attendu est cette clé au lieu de `None` |
| `crates/pa-core/src/kernel/provisioner.rs` | `a_stored_serper_key_is_withheld_without_the_allowlist` | renfort (nouveau) | sans réglage, la clé résolue n'est pas transmise |
| `crates/pa-daemon/tests/launch_env_e2e.rs` | `the_kernel_runs_in_the_client_launch_env` | setup + renfort | `kernel.envAllowlist: ["PA_MARKER"]` ; ajout de `PA_FAKE_TOKEN` au `launchEnv`, qui doit rester absent du kernel |
| `scripts/check_kernel_env.py` | check de cycle de vie | setup + renfort | `kernel.envAllowlist` avec l'identifiant de fixture et `SERPER_API_KEY` ; deux cas ajoutés sans `SERPER_API_KEY` : clé résolue et clé ambiante retenues |
| `scripts/check_launch_env.py` | check de cycle de vie | setup | `kernel.envAllowlist: ["PA_ENV_*", "PRIME_AGENT_INTERNAL_PARITY_CANARY"]` ; le canari reste listé pour que son absence prouve toujours le filtrage côté worker |
| `scripts/lifecycle_lib.py` | `OwnedProcesses.ownership` | setup | un kernel ne porte plus `PA_GOAL_FIXTURE_ID` ; il est reconnu à `PRIME_AGENT_CODING_AGENT_DIR` = `<racine>/agent`, en plus du pid propriétaire déjà exigé. Workers et process directs exigent toujours l'étiquette |
| `scripts/test_lifecycle_lib.py` | `test_untagged_kernel_owned_through_its_agent_dir`, `test_untagged_worker_never_owned_through_its_agent_dir` | renfort (nouveaux) | le kernel d'un autre dossier agent n'est pas reconnu ; un worker sans étiquette non plus |

## Test de préfixe (T03, YA-04)

`crates/pa-core/tests/prefix_stability.rs`, lancé par les shards de `ci.yml` (le venv du kernel y est préparé avant les tests ; sans kernel, le test échoue en CI et est sauté ailleurs avec une note).

- Un serveur HTTP local joue le modèle, au format Anthropic Messages ou OpenAI Responses, et garde chaque corps de requête octet pour octet. La réponse ne dépend que de la requête : étape du prompt (marqueur `PREFIX-PROMPT-n`) et nombre de résultats de tool déjà présents.
- Session réelle : `create_session` avec `real_stream_fn` (`provider_adapter.rs`), donc prompt système, tools, conversion des messages et adaptateur du harness. Les tool calls passent par le vrai `python_repl`.
- Scénario : trois prompts, six requêtes (deux tool calls puis texte ; texte ; un tool call puis texte).
- Les sorties des cellules prouvent que le vrai kernel a tourné : `1`, `2`, puis `3` (variable de la cellule 2) et `True True` (`PRIME_AGENT_CONVERSATION_LOG` se termine par `.jsonl`, `$PRIME_AGENT_BUNDLED_SKILLS_DIR/edit/SKILL.md` existe).
- Entre la requête N et N+1 : chaque clé hors conversation (prompt système, tools, paramètres) est identique octet par octet ; pour Responses, le premier élément d'`input` (le prompt système) aussi. Les messages de N sont un préfixe exact de ceux de N+1, seul le déplacement de `cache_control` est toléré.
- Le corps reçu doit être du JSON compact : sa re-sérialisation (`preserve_order`) redonne les mêmes octets. Comparer les valeurs re-sérialisées revient donc à comparer les octets reçus.
- En cas d'échec : l'adaptateur, la paire de requêtes, la clé ou le message en cause, l'octet de la première différence et le contexte des deux côtés.

## Prompt système stable (T03, YA-04)

Avant T03, le prompt système changeait entre deux sessions du même projet (identifiant de session dans `Conversation log:`), chaque jour (`Current date:`), d'un projet à l'autre (`Working directory:`) et selon le dossier d'extraction du binaire (chemin absolu de chaque `SKILL.md` embarqué).

| Valeur | Avant | Après |
|---|---|---|
| Date (UTC) | `Current date:` dans le prompt | bloc `<environment>` à la fin du premier message utilisateur ; nouvelle date à la fin du premier message utilisateur d'un autre jour |
| Répertoire de travail | `Working directory:` dans le prompt | même bloc, premier message utilisateur |
| Journal de session (contient l'id de session) | `Conversation log:` dans le prompt | variable du kernel `PRIME_AGENT_CONVERSATION_LOG` (vide si la session n'est pas persistée) ; le prompt dit comment la lire |
| Racine des skills embarqués (dossier d'extraction du binaire) | chemin absolu dans chaque `<location>` | `<location>$PRIME_AGENT_BUNDLED_SKILLS_DIR/<nom>/SKILL.md</location>` ; variable du kernel, visible aussi par `bash()` |

Bloc `<environment>` :

- Calculé à la conversion vers le LLM (`environment_block.rs`), après `refinement_gate::convert_to_llm`. Rien n'est écrit dans le JSONL de session.
- La date vient de l'horodatage du message lui-même, pas de l'horloge : chaque requête répète les blocs déjà envoyés octet pour octet.
- Le bloc est une partie texte ajoutée après le contenu du message, qui reste inchangé.
- Les messages custom convertis en tours utilisateur comptent aussi : la tâche `[task from parent]` d'un enfant `rlm` porte le bloc complet, comme le premier prompt du parent.
- Après une compaction, le premier tour utilisateur du contexte reconstruit porte de nouveau le bloc complet (le préfixe est de toute façon reconstruit).
- C'est le point d'accueil de l'instantané d'environnement complet de YA-09.

Ce qui reste propre au projet dans le prompt : les chemins des `AGENTS.md` (`## <chemin>`) et des skills de l'utilisateur ou du projet (hors racine embarquée). Ils sont identiques d'une session à l'autre du même projet.

## Réglage du TTL (T03, YA-04)

`settings.json` : `"cacheRetention": "short"` ou `"long"` (global ou projet, le projet l'emporte).

| `PI_CACHE_RETENTION` | `cacheRetention` | Option envoyée aux providers | TTL effectif (Anthropic) |
|---|---|---|---|
| définie | quelconque | `None` | celui de la variable, comme upstream (`long` → 1 h, toute autre valeur → 5 min) |
| absente | absent | `None` | 5 min (défaut upstream, inchangé) |
| absente | `"short"` | `Short` | 5 min |
| absente | `"long"` | `Long` | 1 h si le modèle l'accepte (`supportsLongCacheRetention`, vrai par défaut) |

- Une valeur invalide (`"1h"`, `true`) compte comme non réglée.
- Côté OpenAI Responses, `Long` n'ajoute `prompt_cache_retention: "24h"` que si le modèle déclare `supportsLongCacheRetention`.
- Transport : `engine.rs` enveloppe le `stream_fn` de la session (`scoped_stream_fn`), qui pose la valeur dans une variable de tâche tokio autour de chaque appel et du futur renvoyé ; `stream_once` la lit (`cache_retention::current()`). Les signatures upstream (`StreamRequestOptions`, `ProviderTarget`) et la couche providers (`pa-ai`) ne changent pas. Hors session (appels annexes sans ce `stream_fn`), la valeur est `None`, comme avant.
- Test : `anthropic_cache_retention_follows_the_setting` (`prefix_stability.rs`) vérifie les marqueurs `cache_control` de la requête réelle, sans réglage puis avec `"long"`.

## Tests upstream touchés (T03)

Même règle : aucun test supprimé ni affaibli. Chaque modification porte un commentaire `yusAgent (T03, YA-04)`.

| Fichier | Test(s) | Nature | Modification |
|---|---|---|---|
| `crates/pa-core/src/prompts/system_prompt.rs` | `default_prompt_is_layered_with_static_prefix_first` | défaut + renfort | la fin du prompt contient les lignes fixes ; ajout : ni `Working directory:`, ni `Current date:`, ni le chemin du journal |
| `crates/pa-core/src/prompts/system_prompt.rs` | `custom_prompt_replaces_layers_keeps_tail` | défaut | la ligne fixe sur `<environment>` au lieu de `Working directory: /w` ; plus de `Current date: ` |
| `crates/pa-cli/src/prompt_command.rs` | `assembles_the_layered_prompt_for_a_directory` | défaut + renfort | la ligne fixe du journal au lieu de `Conversation log: not persisted` ; ajout : aucune ligne `Conversation log:` |
| `crates/pa-core/tests/prompt_guards.rs` | `cached_prefix_is_stable_across_sessions` | défaut + renfort | la fin du prompt ne contient plus la date, le cwd ni le journal (les deux sessions) ; ajout : les lignes fixes sont présentes |
| `crates/pa-core/tests/prompt_guards.rs` | `whole_prompt_is_identical_across_sessions_of_one_project` | renfort (nouveau) | prompt entier identique quand seuls le cwd et le journal changent ; racine des skills embarqués absente, `<location>` relative à la variable |
| `crates/pa-daemon/tests/queue_delivery_e2e.rs` | mock du provider (3 tests : `multi_item_queue_delivers_every_item_in_lane_order`, `queue_pickup_projection_reaches_clients_before_the_delivered_turn_starts`, `queue_delivery_projects_the_active_action_phases_around_the_turn`) | setup | le texte du prompt est la dernière partie texte qui n'est pas le bloc `<environment>` (le mock prenait la dernière partie, devenue le bloc sur le premier message) |
| `scripts/check_custom_ca.py` | check de cycle de vie, `request_contract` | setup | le texte d'un message utilisateur exclut la partie `<environment>` avant la comparaison exacte au prompt de fixture |
| `scripts/check_observe_selectors.py` | check de cycle de vie, `child_task` | setup | même exclusion pour la tâche `[task from parent]` de l'enfant, qui porte désormais le bloc |
