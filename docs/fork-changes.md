# Écarts avec upstream

Toute modification d'un fichier upstream, et tout ajout dans une zone gérée par upstream, est listé ici (CLAUDE.md règle 2).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `.github/workflows/ya-build-macos-arm64.yml` | ajout | Binaire macOS arm64 de HEAD en artefact CI pour les mesures du banc de capture, sans build natif local. Copie réduite du job `build-darwin` de `continuous.yml` (qui ne tourne que sur `main` et construit 4 cibles). Autorisé par Yusuf pour ce seul but. | YA-00 |
| `.github/workflows/continuous.yml` | modif. (1 ligne + commentaire) | Job `build-darwin`, cible `aarch64-apple-darwin` : `runs-on: macos-15` au lieu de `macos-14`, image retirée de GitHub Actions le 02/11/2026 (job annulé au démarrage, « The configuration 'macos-14-arm64-us-default' is not supported », run 42283339842). `macos-15` est arm64 et sert déjà au job `x86_64-apple-darwin` du même workflow (réussi sur `db8b1ff`). À reprendre si upstream change ce label. | YA-00 |
| `.github/workflows/ya-build-macos-arm64.yml` | modif. (1 ligne + commentaire) | `runs-on: macos-15` au lieu de `macos-14`, même raison (run 42283492883 annulé). | YA-00 |
| `.github/workflows/release.yml` | modif. (1 ligne + commentaire) | Job de build darwin, cible `aarch64-apple-darwin` (`:331`) : `runs-on: macos-15` au lieu de `macos-14`, même raison. Non testable avant la prochaine release (le job ne tourne que sur un tag de release) ; même image et mêmes étapes que le job `x86_64-apple-darwin` du même workflow et que `continuous.yml`. | YA-00 |
| `.github/workflows/ya-mine-validate.yml` | ajout | Validation rouge -> vert des PR candidates du smoke bench (`bench/smoke/tools/mine.py`) sur runners Linux jetables, réseau coupé pendant les tests, sans secret (`contents: read`). Déclenché par push sur `ya-07` limité à `bench/smoke/tools/`. Aucun workflow upstream touché. Décision de Yusuf du 04/11/2026. | T07 (YA-14) |
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
| `crates/pa-types/src/ai/mod.rs` | modif. | `Usage` : champs optionnels `reasoning` et `cacheWrite1h`, absents à la sérialisation quand ils valent `None`. Voir « Usage (T04) » ci-dessous. | T04 (YA-13) |
| `crates/pa-agent/src/types.rs` | modif. | Même champs sur le miroir `Usage` de la boucle (la frontière `pa-ai` / `pa-agent` passe par un aller-retour JSON), `Usage::zero()` complété. | T04 (YA-13) |
| `crates/pa-ai/src/providers/anthropic/stream.rs` | modif. | Correctif : l'usage du `message_start` est lu sous `message.usage` (`:297`) ; nouvelle fonction `read_usage_details` (`:711`) appelée au `message_start` et au `message_delta` (raisonnement, part 1 h de l'écriture du cache). | T04 (YA-13) |
| `crates/pa-ai/src/providers/openai_responses_stream.rs` | modif. (5 lignes) | `reasoning` = `output_tokens_details.reasoning_tokens`. | T04 (YA-13) |
| `crates/pa-ai/src/providers/openai_completions/convert.rs` | modif. (5 lignes) | `reasoning` = `completion_tokens_details.reasoning_tokens`. | T04 (YA-13) |
| `crates/pa-ai/src/providers/google_stream.rs` | modif. (4 lignes) | `reasoning` = `thoughtsTokenCount`. | T04 (YA-13) |
| `crates/pa-ai/src/providers/anthropic/usage_tests.rs`, `providers/usage_tests.rs`, `providers/openai_completions/usage_tests.rs` | ajout | Tests de normalisation de l'usage sur fixtures (Anthropic, Responses, Gemini, DeepSeek, OpenRouter). | T04 (YA-13) |
| `crates/pa-ai/src/providers/anthropic/mod.rs`, `providers/mod.rs`, `providers/openai_completions/mod.rs` | modif. (3 lignes chacun) | Déclarent les modules de tests ci-dessus. | T04 (YA-13) |
| Littéraux `Usage` (19 fichiers) | modif. (2 lignes par littéral) | `reasoning: None, cache_write_1h: None` ajoutés aux littéraux complets de `Usage`, exigés par le compilateur. Liste ci-dessous. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/usage_log.rs`, `usage_log/tests.rs` | ajout | Journal d'usage par appel (`<id>.usage.ndjson`) et ses tests. Voir `docs/usage.md`. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Déclare `usage_log`. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | Ouvre le journal de la session (`:338-341`) ; enveloppe le `stream_fn` de la boucle (`main`/`subagent`) et celui de la question annexe (`other`) par `usage_log::instrument_stream_fn` (`:707-717`). | T04 (YA-13) |
| `crates/pa-core/src/session_engine/compaction_exec.rs` | modif. | `complete_summary_call` : les deux chemins (avec ou sans deltas) journalisent l'appel en `compaction`. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/refine.rs` | modif. (7 lignes) | `default_refiner_call` : `usage_log::complete_simple` (`refine`) au lieu de `pa_ai::complete_simple`. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/branch_summarization.rs` | modif. (8 lignes) | Résumé de branche : `usage_log::complete_simple` (`other`, `branch_summary`). | T04 (YA-13) |
| `crates/pa-core/tests/usage_backcompat.rs` | ajout | Une session écrite avant T04 se relit ; sans valeur, l'usage s'écrit avec les mêmes champs qu'avant ; les champs traversent la frontière `pa-ai` / `pa-agent`. | T04 (YA-13) |
| `docs/usage.md` | ajout | Sémantique des champs d'usage par provider, taux de cache, format du journal. | T04 (YA-13) |
| `.changes/ya-04.md` | ajout | Fragment de changelog. | T04 (YA-13) |
| `crates/pa-core/src/session_engine/masking.rs`, `masking/tests.rs` | ajout | Masquage des anciennes sorties de `python_repl` sur le hook `transform_context`, réglage `masking` (coupé par défaut), garde-fous, invalidation journalisée, et leurs tests. Voir « Masquage (T05) » ci-dessous. | T05 (YA-05) |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Déclare `masking` (`:31-32`). | T05 (YA-05) |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | Lit `masking` avant le déplacement des réglages (`:285-287`) ; `transform_context` reçoit `masking::transform_context` (`:717`, `:761`) au lieu de `pass_through_transform()`, qui reste ce qu'il renvoie quand le réglage est coupé. | T05 (YA-05) |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Champ `masking` de `Settings` (`:321-323`). | T05 (YA-05) |
| `crates/pa-core/src/settings/load.rs` | modif. (2 lignes) | `masking` ajouté aux clés connues (`:69-70`). | T05 (YA-05) |
| `crates/pa-core/src/session_engine/usage_log.rs`, `usage_log/tests.rs` | modif. (fichiers du fork) | Champ `invalidation` : noté par le masquage (`note_invalidation`, `:323`), pris par l'appel `main` suivant (`instrument_stream_fn`), absent sinon. Test ajouté. | T05 (YA-05) |
| `prime-agent-runtime/src/rlm/bash.py` | modif. (14 lignes) | `_emit_failed_exit` (`:179`) : une cellule qui lit un résultat `bash()` de code non nul émet `application/vnd.prime-agent.bash-exit+json` (`:54`), appelé à la lecture du résultat (`_note_result_consumed`, `:825`). Sans cela l'hôte ne sait pas qu'un `bash()` a échoué : `bash()` ne lève pas d'exception. | T05 (YA-05) |
| `crates/pa-core/src/kernel/shared.rs` | modif. (5 lignes) | Constante `BASH_EXIT_DISPLAY_MIME` (`:72`), champ `bash_failed_exit` d'`ExecuteResult` (`:213`). | T05 (YA-05) |
| `crates/pa-core/src/kernel/manager/events.rs` | modif. (8 lignes) | `dispatch_display` lit le code de sortie de ce MIME (`:239-246`). | T05 (YA-05) |
| `crates/pa-core/src/kernel/manager/mod.rs`, `execution.rs`, `session_engine/runtime_wiring.rs` | modif. (1 à 2 lignes chacun) | `bash_failed_exit` transmis jusqu'au résultat du tool (`mod.rs:95`, `:139`, `execution.rs:227`, `runtime_wiring.rs:471`). | T05 (YA-05) |
| `crates/pa-core/src/tools/ipython.rs` | modif. (6 lignes) | Champ `bash_failed_exit` (`:69`) et clé `bashFailedExit` des `details` du résultat (`:470-472`). Les `details` ne sont pas envoyés au modèle. | T05 (YA-05) |
| `prime-agent-runtime/src/rlm/masked.py`, `prime-agent-runtime/test/test_masked.py` | ajout | `tool_output(id)` relit une sortie masquée dans le journal de session ; tests de la relecture et de l'émission du code de sortie. | T05 (YA-05) |
| `crates/pa-core/tests/masking_bash_exit.rs` | ajout | Vrai kernel : une cellule qui lit un `bash()` en échec remonte son code de sortie. | T05 (YA-05) |
| `bench/tools/capture/server.py`, `run.py`, `masking_check.py`, `README.md` | modif. / ajout | Scénario `mask`, cache simulé sur le préfixe réel (`--prefix-cache`), cas `mask-off-anthropic` et `mask-on-anthropic`, rapport `masking-report.md`. | T05 (YA-05) |
| `docs/usage.md` | modif. | Champ `invalidation` du journal. | T05 (YA-05) |
| `.changes/ya-05.md` | ajout | Fragment de changelog. | T05 (YA-05) |
| `Makefile` | modif. (11 lignes, en fin de fichier) | Cible `smoke` : appelle `bench/smoke/run.py` avec `BINARY`, `PROVIDER`, `MODEL`, `EFFORT`, `SETTINGS`, `RUNS`, `TASKS`, `CAPTURE`, `CAPTURE_FAIL`, `CAPTURE_SPAWN` (T07b). Aucune cible upstream touchée. Voir « Smoke bench (T06) » ci-dessous. | T06 (YA-14) |
| `.github/pull_request_template.md` | modif. (2 lignes) | Case « Smoke bench lancé sur ce commit (lien ou chemin du rapport) » sous « Test/e2e evidence ». | T06 (YA-14) |
| `crates/pa-ai/src/providers/openai_responses_stream.rs` | modif. (14 lignes) | Usage de `response.completed` : `cacheWrite` = `input_tokens_details.cache_write_tokens`, retiré de `input` (`:784-801`) ; appel du hook `on_usage` avec `response.usage` (`:771-774`). Voir « Usage (T08) » ci-dessous. | T08 (YA-13) |
| `crates/pa-ai/src/types.rs` | modif. (8 lignes) | Type `OnUsageHook` (`:70`) et champ `on_usage` de `StreamOptions` (`:86`), Debug complété. | T08 (YA-13) |
| `crates/pa-ai/src/providers/simple_options.rs` | modif. (1 ligne) | `on_usage` recopié vers les options du provider (`:60`). | T08 (YA-13) |
| `crates/pa-ai/src/providers/openai_responses_hooks.rs` | modif. (2 lignes) | Champ `on_usage` de `ResponsesStreamHooks` (`:37`). | T08 (YA-13) |
| `crates/pa-ai/src/providers/openai_responses.rs`, `azure_openai_responses.rs`, `openai_codex_responses/mod.rs` | modif. (1 à 4 lignes, 4 sites) | `on_usage` des options passé aux hooks du processeur (`openai_responses.rs:358`, `azure_openai_responses.rs:391-394`, `openai_codex_responses/mod.rs:447`, `:559`). | T08 (YA-13) |
| `crates/pa-agent/src/stream.rs` | modif. (8 lignes) | Type `OnUsageHook` (`:135`) et champ `on_usage` de `StreamRequestOptions` (`:154`), Debug et Default complétés. | T08 (YA-13) |
| `crates/pa-agent/src/agent_loop/response.rs` | modif. (1 ligne) | `on_usage: None` dans le littéral complet des options (`:147`), comme `on_payload`. | T08 (YA-13) |
| `crates/pa-core/src/session_engine/provider_adapter.rs` | modif. (12 lignes) | `agent_usage_hook_to_ai` (`:84`) et passage du hook de la boucle à `pa_ai` (`:180`), comme `on_response`. | T08 (YA-13) |
| `crates/pa-ai/src/providers/usage_tests.rs` | modif. (fichier du fork) | Fixtures Responses : écriture avec et sans ventilation, sans écriture, saturation de `input`, hook `on_usage`, coût des écritures au tarif du catalogue. | T08 (YA-13) |
| `crates/pa-core/src/session_engine/usage_log.rs`, `usage_log/tests.rs` | modif. (fichiers du fork) | `providerUsage` sans `attribution` et `undocumentedAttribution` (compteurs agrégés `request_fields`) (`:447-456`, `split_attribution` `:464`), `cacheWrite30m` / `cacheWrite12h` lus dans l'usage brut et `costWarning` si du 12 h apparaît (`:413-431`) ; la sonde branche `on_usage` comme `on_payload`. Tests ajoutés. | T08 (YA-13) |
| `bench/tools/capture/server.py`, `usage_check.py`, `README.md` | modif. (fichiers du fork) | Usage Responses et Codex avec `cache_write_tokens` et ventilation 30 min / 12 h ; vérification de `cacheWrite`, de la ventilation, de `costWarning`, de `providerUsage` (sans `attribution`) et de `undocumentedAttribution`. | T08 (YA-13) |
| `bench/smoke/metrics.py`, `run.py`, `README.md` | modif. (fichiers du fork) | `request_cost` et coût des invalidations : `cacheWrite30m` et `cacheWrite12h` tarifés séparément depuis `prices.json` (`cache_write_30m`, `cache_write_12h`), sans ventilation au tarif 30 min comme avant ; notes du rapport mises à jour. | T08 (YA-13) |
| `docs/usage.md` | modif. | Ligne Responses, `cacheRetention: long` et TTL 12 h, format du journal (`cacheWrite30m`, `cacheWrite12h`, `costWarning`, `providerUsage`). | T08 (YA-13) |
| `docs/tickets/08-usage-ecriture-cache.md` | modif. | Décision du 04/11/2026 après la CI : ventilation hors de `Usage`. | T08 (YA-13) |
| `.changes/ya-08.md` | ajout | Fragment de changelog. | T08 (YA-13) |
| `bench/smoke/metrics.py`, `run.py`, `compare.py`, `README.md` | modif. (fichiers du fork) | Coût pessimiste (`input` non caché au tarif `cache_write_30m`) à côté du coût rapporté : `pessimistic_request_cost`, totaux, colonne du tableau des tâches, `tasks.csv`, `report.json` ; section du coût agrégé de `compare.py` avec le même détail, sans alerte. Aucun changement du harness. | T08b (YA-13) |
| `Cargo.lock`, `.changes/rustix-1-1-6-advisory-bump.md` | cherry-pick d'upstream | Commit upstream `450314627` (#4030), repris en entier : `rustix` 1.1.4 → 1.1.6 pour RUSTSEC-2026-0486 (xattr *unsound*), qui fait échouer le job `deny` de `ci.yml` ; le fragment de changelog est celui d'upstream, exigé par `changelog-fragment.yml`. Aucune autre dépendance ne bouge (l'entrée `rustix` 0.38.44 de crossterm est hors de la plage touchée). Le prochain cherry-pick de ce commit est vide. | YA-00 |
| `.github/workflows/ci.yml` | modif. (1 ligne → 9 lignes + commentaire) | Job `windows-cross`, étape `Install the mingw-w64 cross toolchain` : `apt-get update` puis `apt-get install` en boucle, 3 tentatives espacées de 30 s, chaque commande bornée à 5 min par `timeout 300` (pire cas 31 min, sous les 60 min du job ; l'étape prend de 16 à 97 s sur les runs verts). Le miroir apt du runner échoue : « Temporary failure resolving 'azure.archive.ubuntu.com' » (jobs 130611043822, 130619875377, 130652212887), ou `apt-get update` reste bloqué 59 min jusqu'à l'annulation du job (job 130673633872). Upstream n'a pas de correctif : même ligne unique (`ci.yml:482` d'upstream `a05bcb695`), aucun commit sur apt dans son historique. | YA-00 |
| `crates/pa-core/src/prompts/rlm_off.rs` | ajout | Avec `rlmMaxDepth` 0, retrait du prompt de ce qui ne concerne que les sous-agents, et ses tests. Voir « Sous-agents coupés (YA-06) » ci-dessous. | YA-06 |
| `crates/pa-core/src/prompts/mod.rs` | modif. (2 lignes) | Déclare `rlm_off`. | YA-06 |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. | Champ `rlm_max_depth: Option<u64>` de `BuildSystemPromptOptions` (`None` par défaut ; un 4ᵉ booléen ferait échouer `clippy::struct_excessive_bools`) ; couches `core` et `usage` passées par `rlm_off::core_layer` / `usage_layer` ; ligne de rôle sans démon lue dans `rlm_off::daemonless_spawn_line` (texte upstream déplacé tel quel). | YA-06 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (7 lignes) | `rlm_max_depth` = `rlm_off::root_max_depth(...)` dans les options du prompt. | YA-06 |
| `.changes/ya-06-rlm-off.md` | ajout | Fragment de changelog. | YA-06 |
| `crates/pa-core/src/settings/fork_defaults.rs` | modif. (fichier du fork) | `RLM_MAX_DEPTH = 0` : défaut de `rlmMaxDepth` du fork, verdict de l'A/B du 12/11/2026 (`bench/smoke/ab/rlm-spawn-20261112.md`). Voir « Sous-agents coupés (YA-06) ». | YA-06 |
| `crates/pa-core/src/session_engine/rlm_host.rs` | modif. (1 ligne + commentaire) | `DEFAULT_RLM_MAX_DEPTH` lit `fork_defaults::RLM_MAX_DEPTH` (0) au lieu du littéral `2` (`:279-281`). Le résolveur, le statut du démon et l'identité initiale des enfants (`crates/pa-daemon/src/rlm_children.rs:109`) suivent. | YA-06 |
| `.cargo/config.toml` | modif. (4 lignes) | `RLM_MAX_DEPTH = "2"` dans `[env]`, à côté de `DO_NOT_TRACK` : les tests upstream lancés par `cargo test` exercent le spawn à la borne upstream. Comme `[env]` ne remplace jamais une variable déjà posée, un test qui fixe sa valeur la garde. Effet de bord : un `cargo run` local démarre aussi à 2 ; les binaires de la CI et du banc ne lisent pas ce fichier. | YA-06 |
| Tests upstream | modif. | Voir « Tests upstream touchés (YA-06) » ci-dessous. | YA-06 |
| `bench/smoke/README.md` | modif. (fichier du fork) | `settings/rlm-off.json` noté redondant avec le nouveau défaut ; gardé pour relire l'A/B du 12/11. | YA-06 |
| `.changes/ya-06-rlm-default-off.md` | ajout | Fragment de changelog. | YA-06 |

## Sous-agents coupés (YA-06)

Réglage unique : `rlmMaxDepth`. À 0, il coupait déjà `rlm.spawn` à l'exécution (`crates/pa-cli/src/print_rlm.rs:639`, `crates/pa-daemon/src/rlm_children/host.rs:85`) ; depuis YA-06, il retire aussi du prompt ce qui ne concerne que les sous-agents. Aucune ligne n'est ajoutée à la place (pas de « spawning is disabled »).

**Défaut du fork : 0** depuis le verdict de l'A/B du 12/11/2026 (`bench/smoke/ab/rlm-spawn-20261112.md` : coût à cache théorique des tâches stables -65.2 % [-69.6 ; -56.6], 25 tâches résolues contre 25). Constante `fork_defaults::RLM_MAX_DEPTH`, lue par `rlm_host::DEFAULT_RLM_MAX_DEPTH`. Réactivation : `"rlmMaxDepth": 2` (ou toute valeur ≥ 1) dans le `settings.json` global, `RLM_MAX_DEPTH=2` dans l'environnement (≥ 1 seulement, 0 y est ignoré), ou `/rlm-max-depth` dans une session.

- **Condition** (`rlm_off::root_max_depth`, puis `spawn_off` : borne `Some(0)`) : session racine (`rlm_depth` absent ou 0) et borne résolue à 0 par le même résolveur que la porte du spawn (`rlm_host::resolve_rlm_max_depth` : réglage de chat > hérité > global > `RLM_MAX_DEPTH` (≥ 1 seulement) > défaut 0). Une session enfant n'existe que si la borne dépasse 0 : son prompt ne change jamais. Évaluée à la création de la session, comme le reste du préfixe.
- **Retiré** : dans `core.md`, de `## Subagents` jusqu'à la phrase des outils a2a exclue (description de l'arbre d'agents, `rlm.spawn`, `rlm.create_session`, `rlm.find_models`, `rlm.list_subagents`, `rlm.delete_subagent`, `rlm.rename`, `rlm.collect`, `rlm.progress_note` et leurs types) ; dans `usage.md`, le bloc « Instructions to agents for multi-agent work » et la mention « uses many subagents » de la règle des points d'avancement ; dans la ligne de rôle sans démon, « and your subagents » et « Subagent replies and exit notices still wake you. ».
- **Gardé** : les outils a2a (`agent_message`, `agent_observe`) et le skill `agent-message`, parce que les sessions racines sont sœurs et s'écrivent par le démon (`crates/pa-daemon/src/agent_messaging.rs:157-168`, `skills/agent-message/SKILL.md` : « Roots are siblings ») ; la section Heartbeat et le skill `rlm-heartbeat`, parce qu'un heartbeat réveille la session qui le crée (`skills/rlm-heartbeat/SKILL.md`, `crates/pa-daemon/src/scheduled_jobs.rs:295`) ; l'API du harnais continu, dont les entrées de type `subagent` (`rlm.harness.*_subagent`) et les « subagent specs » de `usage.md`.
- **Borne ≥ 1, ou `None`** quand l'appelant ne la donne pas : chaque fonction de `rlm_off` rend le texte upstream inchangé (`Cow::Borrowed`), donc le prompt est identique octet pour octet à celui de `0fa8fad`. Tests : `rlm_off::tests`.
- **Borne non réglée** : défaut 0 depuis le 12/11, donc prompt sans sous-agents. Requêtes envoyées : celles du réglage `{"rlmMaxDepth": 0}` de `69cc894`, octet pour octet (instructions, tools, input et autres clés ; banc de capture du 12/11, binaire `ee02e61`, requêtes de la tâche et de la chauffe). Les requêtes de la sonde, faites sans les réglages du bras, perdent aussi la surface des sous-agents. Contre le défaut de `69cc894` : 48 lignes retirées et 2 réécrites dans les instructions, 30 405 → 26 236 caractères, environ 7 155 → 6 135 tokens (tiktoken `o200k_base`, estimation).
- **Cherry-pick** : si upstream déplace l'un des repères (`## Subagents`, phrase a2a, bloc multi-agent, phrase des points d'avancement, ligne de rôle), `the_markers_match_the_layers` échoue au lieu de laisser passer la surface des sous-agents.

## Tests upstream touchés (YA-06)

Même règle qu'en YA-01 : aucun test supprimé ni affaibli ; chaque modification porte un commentaire `yusAgent (YA-06)`.

Les tests upstream lancés par `cargo test` reçoivent `RLM_MAX_DEPTH=2` de `.cargo/config.toml` : ceux qui passent par le résolveur (démon, mode print, ACP, `prefix_stability`) gardent la borne et le prompt upstream sans modification. Ceux qui lisent la constante elle-même suivent le nouveau défaut sans changement (exemple : `state_getters_tests.rs`, `rlm_max_depth_status_matches_the_ts_shape`, statut lu avant toute résolution : `DEFAULT_RLM_MAX_DEPTH` et `default`). Restent ceux qui retirent la variable ou lancent un spawn avec l'identité par défaut, sans le résolveur :

| Fichier de test | Test(s) | Nature | Changement |
|---|---|---|---|
| `crates/pa-cli/tests/print_inline_subagents_e2e.rs` | `PrintLane::run` (helper de toutes les voies du fichier) | setup | `env("RLM_MAX_DEPTH", "2")` au lieu de `env_remove("RLM_MAX_DEPTH")` ; l'`env_extra` d'une voie la remplace toujours (`RLM_MAX_DEPTH=1` du test de la porte de profondeur) |
| `crates/pa-cli/tests/print_inline_subagents_e2e.rs` | `print_root_spawn_is_refused_at_the_default_bound` | nouveau | binaire réel, sans réglage (`RLM_MAX_DEPTH=0` est ignoré, donc défaut) : `rlm.spawn` refusé avec « RLM recursion depth limit reached (RLM_DEPTH=0, RLM_MAX_DEPTH=0) », aucun dossier d'enfant, run terminé |
| `crates/pa-daemon/src/rlm_children/watch_tests.rs` | `sessions_with_fake_child_subagents` et `sessions_with_roster_peers` (helpers) | setup | `rlm_max_depth: 2` dans l'identité du parent (`with_default_depth` vaut 0) |
| `crates/pa-daemon/src/rlm_children/spawn_name_reservation_tests.rs` | `sessions_with_gated_supervisor` (helper), `self_rename_targets_the_caller_without_the_parent_marker` | setup | `rlm_max_depth: 2` dans l'identité du parent |

Tests du fork (`crates/pa-core/src/prompts/rlm_off.rs`) : `the_default_bound_drops_the_subagent_surface` (nouveau : la borne par défaut retire `rlm.spawn` et `## Subagents`) ; `spawn_off_follows_rlm_max_depth` (sans réglage : couche `env` si posée, sinon 0) ; `a_bound_above_zero_keeps_the_upstream_text` et `spawn_on_prompt_keeps_the_subagent_surface` (renommés : la borne upstream n'est plus le défaut).

Vérifications de cycle de vie sur le vrai binaire (`scripts/check_*.py`, étape « Run the rust lifecycle checks » du shard 1 de `ci.yml`) : elles lancent le binaire sans `cargo`, donc sans `.cargo/config.toml`. Les cinq qui lancent des enfants (`rlm.spawn` ou enfants de factory) fixent `rlmMaxDepth: 2` dans le `settings.json` qu'elles écrivent (setup, commentaire `yusAgent (YA-06)`) : `check_dead_child_delete.py`, `check_factory_resumption.py`, `check_late_message_receipt.py`, `check_ledger_liveness.py`, `check_observe_selectors.py`. Échecs observés sans ce réglage : run 43307208293, reçus `lifecycle-check-receipts-attempt-1` (« actual spawn/tool result/child request never arrived », etc.).

## Smoke bench (T06, YA-14)

Outillage dans `bench/smoke/` (nouveau dossier, hors zone upstream) et scénarios ajoutés au banc de capture (`bench/tools/capture/server.py`, fichier du fork). T07 : option `--server-miss-at-turn` du serveur de capture (miss serveur simulé). Aucun changement du harness : ni `crates/`, ni `prime-agent-runtime/`, ni la couche providers.

- Accès au modèle sous sandbox : `baseUrl` du provider `openai-codex` (surcharge de provider intégré dans `models.json`, `crates/pa-core/src/models/custom.rs:197-206`) vers un reverse proxy local à cible fixe. Le client WebSocket du harness ne passe pas par `HTTPS_PROXY` (`tokio_tungstenite::connect_async_tls_with_config`, `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:206`) ; le client HTTP oui (`crates/pa-ai/src/utils_inner/tls.rs:53`, sans `.no_proxy()`), il sert au rafraîchissement du jeton.
- `gpt-6.1-terra`, absent de `models.bundled.json`, est déclaré dans le `models.json` du dossier agent (provider intégré, `custom.rs:207`, `:410-420`) ; vérifié contre le serveur de capture : coût du journal d'usage au tarif de la fiche, pas au repli sur la fiche de `gpt-5.5` (`crates/pa-core/src/models/resolver.rs:129-166`).

## Masquage (T05, YA-05)

Coupé par défaut. Réglage dans `settings.json` (global ou projet) :

```json
{"masking": {"enabled": true, "window": 10, "step": 5, "minChars": 500}}
```

Réglage coupé : `transform_context` reste `pass_through_transform()` (`masking.rs:168-170`), donc les requêtes sont celles d'avant T05 octet pour octet (test `disabled_the_transform_returns_the_same_bytes`, et banc : `compare.py --all` entre `main` et `ya-05`).

Réglage actif, à chaque requête de la boucle (`masking.rs:198`) :

- **Tours** : le tour `k` est le `k`-ième message assistant du contexte ; ses résultats de tool lui appartiennent. La requête en cours est le tour `N`.
- **Paliers** : frontière `F = step × ⌊(N − window − 1) / step⌋` (`masking.rs:142`). Les sorties des tours `1..F` sont candidates. Avec les défauts, `F` passe à 5 au tour 16, 10 au tour 21, 15 au tour 26, etc.
- **Décisions figées** : la liste masquée pour `F` est calculée sur le contexte tel qu'il était quand `F` a été atteint, c'est-à-dire les messages des tours `1..F + window` (`masking.rs:268`). Entre deux paliers, rien ne bouge dans le préfixe, même si une nouvelle erreur arrive. Au palier, le préfixe change au plus une fois.
- **Placeholder** : `[sortie masquée : tour <k>, <n> caractères, relire avec from rlm.masked import tool_output; print(tool_output("<id du tool call>"))]` (`masking.rs:228`). Il dépend seulement du tour, du nombre de caractères et de l'identifiant du tool call. Les `details` du résultat restent tels quels (ils ne partent pas au modèle).
- **Relecture** : `rlm.masked.tool_output(id)` (`prime-agent-runtime/src/rlm/masked.py`) lit la sortie complète dans `PRIME_AGENT_CONVERSATION_LOG`. Le JSONL de session n'est jamais modifié.
- **Périmètre** : seuls les résultats de `python_repl`. Jamais les messages utilisateur ou assistant, ni le code des cellules, ni les autres tools.

Garde-fous (`masking.rs:352`). Ne sont jamais masqués :

| Garde-fou | Règle |
|---|---|
| Sortie courte | moins de `minChars` caractères (défaut 500) |
| Image | résultat qui contient une image (pas de relecture texte possible) |
| Dernière sortie en erreur | `details.status == "error"` (exception Python) ou `details.bashFailedExit` (un `bash()` lu avec un code non nul) |
| Dernier échec de test | dernière sortie en erreur dont la cellule lance des tests (`pytest`, `cargo test`, `npm test`, `go test`, …, `masking.rs:45`), même si une erreur plus récente existe |
| Diagnostics actifs | dernière exécution de chaque outil de diagnostic (`ruff`, `mypy`, `pyright`, `eslint`, `tsc`, `cargo check`, `cargo clippy`, …, `masking.rs:72`) |
| Fichiers en cours d'édition | toute sortie dont la cellule cite (chemin ou nom de fichier) un fichier modifié par `edit()` dans les `window` derniers tours (`details.diffs`) |
| Plan | dernière sortie qui lit ou modifie `plan.md`, `progress.md` ou `todo.md` (sans casse, `masking.rs:90`) |

Ces règles sont des heuristiques sur le code de la cellule. Elles protègent trop plutôt que pas assez : une protection en trop coûte des tokens, une protection en moins coûte une relecture.

Invalidation : quand la liste masquée change, la requête est notée dans le journal d'usage (`invalidation`, voir `docs/usage.md`), avec son `cacheWrite`.

Compaction : le seuil part de l'usage réel du dernier appel, plus une estimation des messages qui le suivent (`compaction.rs:237-246`). L'usage renvoyé par le provider porte sur la requête masquée : le seuil en tient compte sans changement. La compaction elle-même résume l'historique complet (non masqué).

## Instabilité probable : `clipboard_auth_commands_e2e` (T05)

`crates/pa-cli/tests/clipboard_auth_commands_e2e.rs`, test `tui_prime_login_escape_keeps_the_default_team_status`, shard 5 de `ci.yml`.

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 41702752565 | `aa4b128b7` | 1 | échec du shard 5 |
| 41702752565 | `aa4b128b7` | 2 | réussi, aucun changement de code |

Symptôme : `the team picker mounted` échoue à `clipboard_auth_commands_e2e.rs:989`. Le rendu ne montre que l'écran d'accueil : le sélecteur d'équipe n'apparaît pas pendant les attentes fixes du plan headless (`WaitMs(400)` après `/login`, `:975-979`). Hypothèse : délai trop court sur un runner chargé (même tentative : le shard 6 est tombé sur « No space left on device »). Le test ne touche à rien de ce que modifient les tickets du fork (login headless sur le provider factice).

Le run `ci.yml` de `main` après le merge de T04 (41707124985, `e76dfc0ad`) a aussi un shard 5 en échec, mais l'annotation dit « No space left on device » et le log n'est plus téléchargeable : cause non attribuable à ce test.

Si ce test retombe : relancer le shard une fois, et noter le run ici.

## Instabilité probable : `interactive_daemon_e2e` (T08)

`crates/pa-cli/tests/interactive_daemon_e2e.rs`, test `tui_ctrl_s_stashes_and_restores_the_prompt_draft`, shard 4 de `ci.yml`.

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 41903362499 | `6eed2da` | 1 | échec du shard 4 (41 réussis, 1 échec) |
| 41903362499 | `6eed2da` | 2 | réussi, aucun changement de code |

Symptôme : `connect supervisor: Failed to connect to the Prime Agent daemon` avec `Connection refused (os error 111)` à `interactive_daemon_e2e.rs:424`. Le test attend que le fichier du socket existe (`:405-407`) puis se connecte : le socket existe déjà mais le superviseur n'écoute pas encore. Hypothèse : course au démarrage du superviseur. T08 ne touche ni le daemon, ni la TUI, ni ce test.

Si ce test retombe : relancer le shard une fois, et noter le run ici.

## Instabilité probable : `daemon_discovery_e2e` (YA-00)

`crates/pa-cli/tests/daemon_discovery_e2e.rs`, test `a_daemon_launched_with_an_inherited_worker_env_comes_up_as_a_supervisor` (`:974`), shard 7/8 de `ci.yml`. Fichier identique à `upstream/main` (`git diff --stat upstream/main -- crates/pa-cli/tests/daemon_discovery_e2e.rs` vide au 11/11/2026).

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 42290071142 | `7257c4b47` (`main`, merge de #17) | 1 | échec du shard 7/8 (7 réussis, 1 échec) |
| 42287699470 | `5e6439cbe` (`ya-ci-runner`, même arbre que `7257c4b47`) | 1 | réussi |

Symptôme : `connect the supervisor` avec `ConnectionRefused` (os error 111) à `daemon_discovery_e2e.rs:1043`. Comme pour `interactive_daemon_e2e` : le test attend que le fichier du socket existe (`:1033-1035`) puis se connecte, et le superviseur n'écoute pas encore. Hypothèse : même course au démarrage du superviseur.

Si ce test retombe : relancer le shard une fois, et noter le run ici.

## Désactivations par défaut (YA-01)

Session lancée sans `settings.json` ni variable d'environnement. Chaque ligne se réactive dans `settings.json` (global, `~/.prime/agent/settings.json`).

| Fonction | Défaut fork | Réactivation | Où |
|---|---|---|---|
| Télémétrie produit (et notice au premier lancement) | coupée | `"telemetry": {"enabled": true}` (ou `PRIME_AGENT_TELEMETRY=1`) | `fork_defaults.rs:17`, `manager.rs:850`, `status.rs:29` |
| Continual Harness (création de raffinements, digest du harness dans le contexte) | coupé | `"harnessRefinements": true` | `fork_defaults.rs:7`, `interactive_settings.rs:17` |
| `/refine` automatique (par intervalle de tours) | coupé | `"autoRefine": {"enabled": true}` (et `harnessRefinements`) | `fork_defaults.rs:10`, `refine.rs:42` |
| `/refine` automatique après compaction | coupé | `"autoRefine": {"compact": true}` (et `harnessRefinements`) | `fork_defaults.rs:13`, `refine.rs:44` |
| Sous-agents (`rlm.spawn`) et leur surface dans le prompt (YA-06, 12/11/2026) | coupés | `"rlmMaxDepth": 2` (ou toute valeur ≥ 1 ; `RLM_MAX_DEPTH=2`) | `fork_defaults.rs` (`RLM_MAX_DEPTH`), `rlm_host.rs:281` |

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

## Usage (T04, YA-13)

- **Champs ajoutés** : `reasoning` (sous-ensemble de `output`) et `cacheWrite1h` (part 1 h de `cacheWrite`), `Option<u64>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`. Absents quand le provider ne les donne pas, jamais 0 par défaut. Sémantique complète : `docs/usage.md`.
- **Pourquoi deux crates** : la boucle (`pa-agent`) reçoit le message du provider (`pa-ai`) par aller-retour JSON (`provider_adapter.rs`, `json_round_trip`) ; sans le champ côté `pa-agent`, il serait perdu avant le journal et la session.
- **Littéraux complétés** (2 lignes `reasoning: None, cache_write_1h: None` par littéral complet, aucun autre changement) : `pa-ai` : `providers/faux/mod.rs`, `utils_inner/overflow.rs` ; `pa-core` : `session/manager/tests.rs`, `session/window_tests.rs` (2), `session_engine/compact_session/tests.rs`, `compact_session/tests/cut_math.rs`, `compact_session/tests/split_turn.rs` (2), `session_engine/compaction.rs` (4, tests), `session_engine/compaction_exec.rs` (2, tests), `session_engine/harness_digest/direction.rs`, `harness_digest/tests.rs`, `session_engine/request_timing/tests.rs`, `session_engine/rlm_usage.rs`, `session_engine/telemetry/tests.rs`, `session_engine/turn_boundary/tests.rs` (2), `tests/turn_boundary_kernel.rs` ; `pa-daemon` : `session_store/tests.rs`, `session_usage.rs` (2) ; `pa-perf` : `corpus/rows.rs`. Les littéraux qui utilisent `..Default::default()` ne changent pas. Un cherry-pick upstream qui ajoute un littéral complet de `Usage` échouera à la compilation avec un message clair (champ manquant).
- **Journal** : `<id>.usage.ndjson` (le ticket disait `<id>.usage.jsonl` ; extension changée avec l'accord de Yusuf, sinon une quinzaine de tests upstream qui prennent le premier `.jsonl` du dossier de sessions pour la session devenaient faux). Local uniquement, aucun puits réseau ; une session en mémoire n'en a pas. Les lignes n'ont ni `id`, ni `timestamp`, ni `cwd` : les scans du dossier de sessions (qui lisent la première ligne de chaque `.jsonl`) ne les prennent pas pour une session.
- **Prompt et requêtes** : inchangés. Le journal observe le corps par le hook `on_payload` et le rend tel quel ; sans journal, `usage_log::complete_simple` appelle `pa_ai::complete_simple` sans rien toucher. Vérifié au banc de capture, `main` contre `ya-04`, toutes les requêtes de tous les cas (`compare.py --all`).
- **Effet du correctif Anthropic** : avec un `message_delta` qui ne porte que `output_tokens`, `totalTokens` devient juste, et la compaction par seuil (qui lit `totalTokens`, `compaction.rs:50-55`) se déclenche quand elle le doit. Ce n'est pas un changement du préfixe, mais un changement du moment de la compaction sur ces flux.

### Banc de capture (T04)

- `server.py` : usage réaliste dans les deux formats (cache lu et écrit, écriture ventilée 5 min / 1 h selon le `cache_control` de la requête, tokens de raisonnement), noté dans `NNNN.meta.json` (`reported_usage`) ; option `--usage-split` (le `message_delta` Anthropic ne porte que `output_tokens`).
- `run.py` : cas `usage-split-anthropic` et `refine-anthropic` (compaction avec `/refine` automatique réactivé) ; copie des journaux d'usage de chaque cas dans `<cas>/usage/` ; lance `usage_check.py`.
- `usage_check.py` (nouveau) : confronte l'usage des messages assistant (`message_end`) et chaque ligne de journal à l'usage renvoyé par le serveur, champ par champ, avec le taux de cache et le type d'appel.
- `compare.py` : option `--all`, qui compare chaque requête de chaque cas (UUID normalisés).

## Usage (T08, YA-13)

- **Aucun champ ajouté à `Usage`** (décision de Yusuf du 04/11/2026, après un premier essai) : deux `Option<u64>` de plus dans `pa_types::ai::Usage` faisaient passer l'écart entre `Assistant` et la variante suivante de 184 à 216 octets dans `pa_types::ai::Message`, `pa_types::session::AgentMessage` et l'enum de messages de `pa-agent` (seuil de `clippy::large_enum_variant` : 200, CI rouge). La ventilation 30 min / 12 h n'est que dans le journal d'usage, lue dans `providerUsage`. `Usage`, ses littéraux et `calculate_cost_values` (`crates/pa-ai/src/models.rs`) sont inchangés : toute écriture est tarifée au tarif d'écriture du catalogue, `costWarning` signale une écriture 12 h.
- **Hook `on_usage`** : générique sur `pa_ai::types::StreamOptions` (option A du ticket), il reçoit l'objet usage brut du provider et le modèle. Seul le processeur Responses l'appelle (Responses, Azure, Codex). Écartés : un champ sur `AssistantMessage` (persisté dans les sessions) et une entrée `diagnostics` (lue par la logique de reprise et de bascule).
- **Littéraux d'options complétés** : `on_usage` dans les littéraux complets de `pa_ai::types::StreamOptions` (`simple_options.rs`, `provider_adapter.rs`), de `StreamRequestOptions` (`pa-agent/src/stream.rs` Default, `agent_loop/response.rs`) et de `ResponsesStreamHooks` (`openai_responses.rs`, `openai_codex_responses/mod.rs` ×2) ; `azure_openai_responses.rs` passe de `ResponsesStreamHooks::default()` à un littéral `..Default`. Un cherry-pick upstream qui ajoute un littéral complet de l'un de ces types échouera à la compilation avec un message clair.
- **Prompt et requêtes** : inchangés. Le hook observe l'usage de la réponse ; rien n'est ajouté au corps envoyé ni aux fichiers de session.
