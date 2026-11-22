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
| `Cargo.lock`, `.changes/url-2-5-9-advisory-bump.md` | modif. (4 lignes) | `url` 2.5.8 → 2.5.9 pour RUSTSEC-2026-0592 (dépassement de pile dans `Url::origin()` sur des URL `blob:` imbriquées), qui fait échouer le job requis `deny` de `ci.yml` depuis le 20/11/2026. Upstream n'a pas encore monté `url` (`upstream/main` au 22/11/2026 : 2.5.8), donc pas de commit à reprendre. `cargo update -p url --precise 2.5.9`, qui monte aussi `form_urlencoded` 1.2.2 → 1.2.3 (dépendance de `url` 2.5.9) ; aucune autre dépendance ne bouge. Pas d'exception dans `deny.toml`. | YA-00 |
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
| `crates/pa-ai/src/providers/ya_freeform.rs` | ajout | Tools libres (`custom`) de l'API Responses : marque d'un tool libre dans `parameters`, items `custom_tool_call` et `custom_tool_call_output`. Rejeu (`FreeformReplay`) : un appel est libre quand le tool qu'il nomme est déclaré libre dans les tools de la requête (aucune marque sur l'appel, que la boucle de l'agent perdrait) ; l'id d'item vient de l'id de l'appel (`<call_id>|<item_id>`), que la conversion ne normalise pas pour un appel du même modèle (`transform_messages.rs`, seul cas où l'id est renvoyé). `freeform_tools_of_request` : les tools libres d'un corps de requête. Voir « Tools libres côté providers (YA-03) ». | YA-03 |
| `crates/pa-ai/src/providers/mod.rs` | modif. (2 lignes) | Déclare `ya_freeform`. | YA-03 |
| `crates/pa-ai/src/lib.rs` | modif. (2 lignes) | Exporte `providers::ya_freeform` (`providers` est privé) : `pa-core` construit le tool `apply_patch` avec (PR 2). | YA-03 |
| `crates/pa-ai/src/providers/openai_responses_shared.rs` | modif. | `convert_responses_tools` : un tool libre part en `custom`. `convert_responses_messages` : `FreeformReplay` construit depuis le contexte ; un appel d'un tool libre de la requête est rejoué en `custom_tool_call`, son résultat en `custom_tool_call_output`. | YA-03 |
| `crates/pa-ai/src/providers/openai_responses_stream.rs` | modif. | `handle_event` : item `custom_tool_call` (`added`, `done`) et événement `response.custom_tool_call_input.delta`. | YA-03 |
| `crates/pa-ai/src/providers/openai_codex_responses/mod.rs` | modif. (2 lignes ; 5 lignes + commentaire ; 1 ligne + 2 de commentaire) | Déclare le module de tests `tests/ya_freeform.rs` dans `mod tests` (accès aux helpers privés du serveur scripté). Suite de conversation WebSocket (`last_response_items`) : le filtre des résultats retire aussi `custom_tool_call_output`. Sans cela, le résultat factice qu'ajoute `transform_messages` à l'appel libre encore sans réponse restait dans la base comparée, et le tour suivant repartait en corps complet, sans `previous_response_id` (trouvé par le test du 2e tour, dernier tour de la PR #26). Même conversion : `tools` de la base de la suite tirés du corps de la requête (`ya_freeform::freeform_tools_of_request`), pour que l'appel libre y soit converti comme dans la requête complète suivante. | YA-03 |
| `crates/pa-ai/src/providers/openai_codex_responses/tests/ya_freeform.rs` | ajout | Tests : appel libre en SSE (`process_sse_chunk`, événement coupé entre deux morceaux), item `done` seul, tour WebSocket de bout en bout et requête de suite, 2e tour réellement envoyé par la suite de conversation WebSocket (`previous_response_id`, entrée réduite au `custom_tool_call_output`, tools inchangés). | YA-03 |
| `crates/ya-apply-patch/` | ajout | Crate repris de `codex-rs/apply-patch` d'openai/codex au SHA `69231e119bfca0b14453515e97d99688ce1c47c9` (Apache-2.0, `LICENSE` et `NOTICE` conservés). Analyse et application d'`apply_patch` ; pas encore utilisé par `pa-core` (PR 2). Voir « Crate `ya-apply-patch` (YA-03) ». | YA-03 |
| `Cargo.toml` | modif. (1 ligne) | `crates/ya-apply-patch` dans les `members` du workspace. | YA-03 |
| `Cargo.lock` | modif. (ajouts seuls) | Entrées de `ya-apply-patch`, `similar` 2.7.0 et des dépendances de test (`assert_cmd`, `pretty_assertions` et leurs dépendances) ; `regex-automata` ajouté aux dépendances de `bstr` (fonction `unicode` activée par `assert_cmd`). Aucune version existante changée. | YA-03 |
| `crates/pa-core/src/tools/ya_output_cap.rs` | ajout | Plafond du texte d'une cellule `python_repl` (`kernel.maxOutputChars`, 40 000 par défaut), sortie complète en fichier. Voir « Plafond de sortie (YA-03) ». | YA-03 |
| `crates/pa-core/src/tools/mod.rs` | modif. (2 lignes) | Déclare `ya_output_cap`. | YA-03 |
| `crates/pa-core/src/kernel/env_allowlist.rs` | modif. (fichier du fork, YA-02) | `KernelSettings` : champ `max_output_chars` (`kernel.maxOutputChars`). | YA-03 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | Lit `kernel.maxOutputChars`, calcule le dossier des sorties complètes, enveloppe l'`execute` du tool `python_repl` avec `ya_output_cap::wrap_execute`. | YA-03 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (1 ligne + commentaire) | `KernelManagerExecutor::execute` : `max_output_chars` à `ya_output_cap::KERNEL_BUFFER_CHARS` (2 000 000 par flux) au lieu du défaut upstream de 65 536, pour que la sortie complète soit gardée puis enregistrée. |  YA-03 |
| `crates/pa-core/src/session_engine/ya_kernel_restart.rs` | ajout | Kernel mort pendant une cellule : redémarrage immédiat, avis en fin de résultat, avis upstream du tour suivant retiré. Voir « Redémarrage du kernel (YA-03) ». | YA-03 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Déclare `ya_kernel_restart`. | YA-03 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | Enveloppe aussi l'`execute` de `python_repl` avec `ya_kernel_restart::wrap_execute`, après le plafond (l'avis n'est jamais coupé), avec la boîte des messages du tour suivant (`boot_notice_rows`). | YA-03 |
| `prime-agent-runtime/src/rlm/read.py` | ajout | `read(path, offset=1, limit=2000)` : contenu d'un fichier texte avec numéros de ligne (format `cat -n`, coupure sur `\n` seul), ligne de suite quand il reste des lignes. Pas encore documenté dans le prompt (PR 2). | YA-03 |
| `prime-agent-runtime/src/rlm/__init__.py` | modif. (3 lignes) | Exporte `read` (import et `__all__`). | YA-03 |
| `prime-agent-runtime/src/rlm/bash.py` | modif. | `bash(command, persistent=False, env=None)` : variables pour cette commande seule, par-dessus l'environnement du kernel (`os.environ` du kernel inchangé), clés et valeurs `str` (`_checked_env`). `_child_env(extra)` les ajoute en dernier ; `BashHandle` et `_spawn_persistent` les reçoivent. Écart noté au T02 (« `bash()` n'a pas de paramètre `env=` »). Pas encore documenté dans le prompt (PR 2). | YA-03 |
| `prime-agent-runtime/src/rlm/repl.py` | modif. (1 ligne + commentaire) | `_ALWAYS_SKIP` : `read`, comme `bash`, n'est pas mis dans le snapshot (le bootstrap le relie). | YA-03 |
| `crates/pa-core/src/kernel/bootstrap/runtime_code.rs` | modif. | Le bootstrap relie `read` à côté de `bash` ; le repli sans runtime accepte `env=` et définit `read`. | YA-03 |
| `crates/pa-core/src/kernel/bootstrap/venv/probe.rs` | modif. (1 ligne + commentaire) | `RUNTIME_READY_CHECK` : la signature attendue de `rlm.bash` devient `['command', 'persistent', 'env']`, et `rlm.read` doit être appelable. Sans ce changement, le runtime de la branche est jugé « pas prêt » et le kernel ne démarre pas (run CI 43479147466 : 13 binaires de test en échec). | YA-03 |
| `prime-agent-runtime/test/test_ya_read_env.py` | ajout | Tests de `read()` et de `bash(env=)`. | YA-03 |
| `prime-agent-runtime/test/test_ya_bash_cwd.py` | ajout | Critère de YA-03 révisé le 18/11/2026 (ticket 16, option A0) : un `os.chdir()` du kernel vaut pour le `bash()` suivant ; un `cd` ou un `export` dans une commande ne lui survit pas. `os.environ` est couvert par `test_ya_read_env.py`. Aucun code du runtime modifié. | YA-03, YA-09 |
| `crates/pa-core/src/session_engine/ya_tool_family.rs` | ajout | Famille de modèle (provider, API et modèle ; tableau des 12 modèles de `codex-rs/models-manager/models.json` au SHA `69231e1`), réglages `editTool` et `shellTool` figés dans l'en-tête du fichier de session (champ `yaTools`, relu à la reprise, avertissement si les réglages courants diffèrent), refus d'un natif explicite hors Codex et d'un changement de famille en cours de session. Voir « Tools natifs (YA-03, PR 2) ». | YA-03 |
| `crates/pa-core/src/session/manager/ya_header.rs` | ajout | `SessionManager::ya_header_field`, `ya_set_header_field` (champ de l'en-tête, réécrit sur disque si le fichier existe déjà, sans effet sur une session fenêtrée), `ya_inherit_header_fields` et `inherit_header_fields` (copie des champs de `INHERITED_HEADER_KEYS`, aujourd'hui `yaTools`, depuis l'en-tête d'une autre session). Tests unitaires (reprise, fork, session matérialisée). | YA-03 |
| `crates/pa-core/src/session/manager.rs` | modif. (2 lignes + commentaire) | Déclare `ya_header` (`pub`). | YA-03 |
| `crates/pa-core/src/session/manager/lifecycle.rs` | modif. (5 lignes + 2 commentaires) | `fork_from` et `materialize_session_file` : le nouvel en-tête reprend `yaTools` de la session source. | YA-03 |
| `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | ajout | Démon : écriture de `yaTools` dans l'en-tête du fichier durable du worker (`write_header_field`, `core_header_sink`), synchro après chaque build (`ya_sync_frozen_tools`). Tests (écriture, build qui remet la valeur au worker, build qui garde l'en-tête durable contre les réglages, fork par branche). | YA-03 |
| `crates/pa-daemon/src/agent_engine.rs` | modif. (4 lignes + 2 commentaires) | Déclare `ya_frozen_tools` ; champ `ya_header_sink` d'`AgentSessionEngine`. | YA-03 |
| `crates/pa-daemon/src/agent_engine/lifecycle.rs` | modif. (5 lignes + 2 commentaires) | Constructeur : `ya_header_sink` vide. `build_session` : la session en mémoire reprend `yaTools` de l'en-tête durable (à côté de l'id), puis la valeur figée est remise au worker (`ya_sync_frozen_tools`). | YA-03 |
| `crates/pa-daemon/src/worker.rs` | modif. (3 lignes + commentaire) | Installe le crochet `core_header_sink` sur le moteur, à côté de `set_rlm_usage_store`. | YA-03 |
| `crates/pa-daemon/src/branch_navigation.rs`, `crates/pa-daemon/src/session_tree.rs` | modif. (4 lignes + commentaire chacun) | Fork du démon (à la racine, et `create_branched_file`) : le nouvel en-tête reprend `yaTools` de la source. | YA-03 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (8 lignes) | Déclare `ya_tool_family` ; champ `ya_tool_family` d'`AgentSession` (initialisé vide) ; `set_model` et `set_model_and_thinking_level` appellent `ya_check_model_change` en tête. | YA-03 |
| `crates/pa-core/src/settings/types.rs` | modif. (5 lignes) | Champs `edit_tool` et `shell_tool` (`editTool`, `shellTool`). | YA-03 |
| `crates/pa-core/src/settings/load.rs` | modif. (3 lignes) | `editTool` et `shellTool` dans `KNOWN_FIELDS`. | YA-03 |
| `crates/pa-core/src/tools/ya_workspace.rs` | ajout | Seule définition du workspace d'une session (`WorkspaceScope`, `allowed_roots` : aujourd'hui le cwd seul) et contrôle `check_inside` (préfixe existant résolu, liens suivis), marqueur `[outside-workspace]`. | YA-03 |
| `crates/pa-core/src/tools/ya_apply_patch.rs` | ajout | Tool `apply_patch` (format libre de Codex), contrôle de workspace de chaque chemin avant toute écriture, vérification de Codex avant toute écriture, application par `ya_apply_patch`, résultat identique à Codex au SHA `69231e1` (sans ajout). | YA-03 |
| `crates/pa-core/src/tools/ya_exec.rs` | ajout | Tools `exec_command` et `write_stdin` de la variante `shellTool: "native"` (sous-ensemble d'`exec_command` de Codex sans PTY), environnement filtré du kernel, budget de sortie de Codex en octets. | YA-03 |
| `crates/pa-core/src/tools/mod.rs` | modif. (4 lignes) | Déclare `ya_apply_patch`, `ya_exec`, `ya_workspace`. | YA-03 |
| `crates/pa-core/src/tools/ya_output_cap.rs` | modif. (fichier du fork) | `save_full_output` sorti de `OutputCap::save` pour `ya_exec`. | YA-03 |
| `crates/pa-core/src/prompts/ya_tools_prompt.rs` | ajout | Texte du prompt pour les tools de la session, appliqué par-dessus les couches upstream et `rlm_off` : `read()`, `bash(env=)`, troncature, `apply_patch`, variante `exec_command` ; marqueurs vérifiés par test. | YA-03 |
| `crates/pa-core/src/prompts/mod.rs` | modif. (3 lignes) | Déclare `ya_tools_prompt`. | YA-03 |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. | Champ `ya_tool_choice` de `BuildSystemPromptOptions` ; couches `core`, `usage` et `opinionated` passées par `ya_tools_prompt` ; inventaire des skills passé par `ya_tools_prompt::skills_inventory` ; ligne sans démon passée par `ya_tools_prompt::daemonless_line` ; ligne `await bash(cmd)` absente en `shellTool: "native"`. | YA-03 |
| `crates/pa-core/src/kernel/bootstrap/ya_native_shell.rs` | ajout | Bootstrap d'une session `shellTool: "native"` : `bash` absent du namespace, `rlm.bash` remplacé par une fonction qui refuse. | YA-03 |
| `crates/pa-core/src/kernel/bootstrap/mod.rs` | modif. (2 lignes) | Déclare `ya_native_shell`. | YA-03 |
| `crates/pa-core/src/kernel/provisioner.rs` | modif. (9 lignes) | Champ `ya_native_shell` d'`IpythonKernelProvisionerOptions` (`false` par défaut) ; le code du bootstrap passe par `ya_native_shell` quand il est vrai, à chaque démarrage et redémarrage. `probe.rs` inchangé. | YA-03 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (3 lignes) | Paramètre `ya_native_shell` de `kernel_provisioner`, passé aux options. | YA-03 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. | Lit `editTool` et `shellTool`, résout les tools natifs de la famille (figés dans l'en-tête de session par `ya_tool_family::freeze_session_tools`), enregistre `apply_patch` puis `exec_command` et `write_stdin` après `python_repl`, adapte la description de `python_repl` en variante native, passe le choix au prompt et au provisioner, fige la famille de la session. | YA-03 |
| `crates/pa-core/Cargo.toml`, `Cargo.lock` | modif. (2 lignes, 1 ligne) | Dépendance `ya-apply-patch` (chemin). | YA-03 |
| `crates/ya-apply-patch/src/tool_spec.rs`, `crates/ya-apply-patch/assets/apply_patch.lark` | ajout | Nom, description et grammaire Lark du tool de Codex (`apply_patch_spec.rs:5,20`, `core/assets/tools/apply_patch.lark`, hash de blob identique), exportés par `lib.rs` (4 lignes). | YA-03 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Scénarios Codex (`gpt-6.1-terra` sur `openai`) : défaut, `editTool: "kernel"`, `shellTool: "native"`, avec l'ordre des tools et le texte du prompt ; reprise d'une session après changement des réglages (mêmes tools, préfixe prolongé, en-tête inchangé).; Codex sur WebSocket (`openai-codex`, serveur WebSocket scripté) : vrai appel `apply_patch` par la boucle de l'agent, tour suivant en suite de conversation (`previous_response_id`, seul le `custom_tool_call_output`), même résultat après reprise (rejeu en `custom_tool_call` avec son id d'item). | YA-03 |
| `crates/pa-core/tests/ya_shell_native_kernel.rs` | ajout | Vrai kernel en `shellTool: "native"` : démarrage, `bash` absent (nom et `from rlm import bash`), `read()` et `edit()`, absence après redémarrage, témoin avec `bash`. Échoue en CI si le venv manque. | YA-03 |
| `bench/smoke/metrics.py`, `bench/smoke/run.py`, `bench/smoke/tools/ab_stats.py`, `bench/smoke/tests/test_bench.py` | modif. (fichiers du fork) | Edits appliqués au premier essai (`apply_patch` et `edit()`), patchs refusés hors workspace, cellules en erreur d'appel par bras. | YA-03 |
| `bench/smoke/settings/edit-kernel.json`, `bash-native.json`, `bash-native-edit-kernel.json` | ajout | Réglages des bras des A/B YA-03. | YA-03 |
| `bench/smoke/tools/sizes.py` | ajout | Estimation de la taille des changements de prompt (tiktoken) ; la capture fait foi. | YA-03 |
| `bench/tools/capture/run.py` | modif. (fichier du fork) | Cas `codex-default`, `codex-edit-kernel`, `codex-shell-native` : provider intégré `openai` surchargé vers le serveur local (`models.json`), modèle `gpt-6.1-terra`, clé factice ; provider et modèle passés à `write_models_json` et `agent_cmd`. Les cas existants ne changent pas. | YA-03 |
| `crates/pa-ai/src/providers/openai_responses_stream.rs`, `.changes/fix-responses-reasoning-replay.md`, `.changes/fix-responses-fn-call-args.md` | cherry-pick upstream | `c12acadaa` (#3976) puis `42a07c13c` (#4014), `git cherry-pick -x`, sans conflit ni retouche. Voir « Correctifs upstream repris (YA-04) ». | YA-04 |
| `crates/pa-ai/src/providers/openai_codex_responses/mod.rs` | modif. (2 lignes ; 1 ligne + 2 de commentaire) | Déclare le module de tests `tests/ya_signatures.rs` dans `mod tests`. Helper de test upstream `codex_response_events` : le `done` porte `output_index` (voir « Tests upstream touchés (YA-04) »). | YA-04 |
| `crates/pa-ai/src/providers/openai_codex_responses/tests/ya_signatures.rs` | ajout | Tour réel sur le WebSocket Codex scripté (raisonnement, texte, appel `python_repl`, chaque `done` avec `output_index`) : item de raisonnement et signature de texte enregistrés, rejeu dans l'ordre du flux, tour suivant en suite de conversation avec le seul `function_call_output`. | YA-04 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Reprise sur WebSocket Codex (`codex_websocket_apply_patch_replays_as_custom_and_continues`) : la première requête après reprise doit partir en suite de conversation (`previous_response_id` = `resp_2`, seul le nouveau prompt). La forme en corps complet n'est plus acceptée. | YA-04 |
| `crates/pa-core/src/session_engine/ya_request.rs` | ajout | Réglage `reasoningSummary` (`"auto"` par défaut, `"off"`), figé dans l'en-tête de session (champ `yaRequest`, relu à la reprise, avertissement si les réglages courants diffèrent ; en-tête avec `yaTools` sans `yaRequest` = `"auto"` ; `yaRequest` illisible réécrit en `"auto"`). En `"off"`, un hook `on_payload` autour du `stream_fn` de la session retire `reasoning.summary` du corps des API Responses ; en `"auto"`, le `stream_fn` est rendu tel quel. Tests unitaires. Voir « Réglages de requête (YA-09) ». | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (9 lignes) | Lit `reasoningSummary`, fige les réglages de requête (`ya_request::freeze_session_request`, avant les tools) et enveloppe le `stream_fn` (`ya_request::scoped_stream_fn`) après `cache_retention`. | YA-09 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Déclare `ya_request`. | YA-09 |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Champ `reasoning_summary` (`reasoningSummary`). | YA-09 |
| `crates/pa-core/src/settings/load.rs` | modif. (2 lignes) | `reasoningSummary` dans `KNOWN_FIELDS`. | YA-09 |
| `crates/pa-core/src/session/manager/ya_header.rs` | modif. (fichier du fork) | `yaRequest` ajouté à `INHERITED_HEADER_KEYS` (fork, session matérialisée, fork du démon) ; test du fork. | YA-09 |
| `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | modif. (fichier du fork) | Le démon copie désormais tous les champs hérités : `ya_sync_frozen_tools` remet au worker chaque champ d'`INHERITED_HEADER_KEYS` (aujourd'hui `yaTools` et `yaRequest`), au lieu de `yaTools` seul ; `FROZEN_TOOLS_KEY` ne sert plus qu'aux tests ; tests (nouvelle session, en-tête durable gardé contre les réglages, en-tête de YA-03 sans `yaRequest` = `"auto"`). | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Champ `reasoning` des variantes (modèle qui raisonne, effort high) ; scénarios `reasoningSummary` absent et `"off"` (préfixe stable, `reasoning` attendu à chaque requête, tools et paramètres identiques entre les bras hors `reasoning`) ; même vérification sur le WebSocket Codex avec suite de conversation. `engine_with_model` prend le niveau de raisonnement. | YA-09 |
| `bench/smoke/settings/reasoning-summary-off.json` | ajout | Réglage du bras B de l'A/B `reasoningSummary`. | YA-09 |
| `bench/smoke/ab/ya09-summary-plan.md` | ajout | Plan et règle de verdict de l'A/B `reasoningSummary`, graine. | YA-09 |
| `.changes/ya-09-summary.md` | ajout | Fragment de changelog (check « Changelog fragment »). | YA-09 |
| `bench/smoke/ab/ya09-summary-mesures.py` | ajout | Mesures de latence de l'A/B (latence fixe par appel avec raisonnement, phase agent appariée par tâche, IC bootstrap sur les tâches, condition de latence de la règle de verdict), validées sur le run YA-14. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | **Défaut produit `"off"`** (décision du journal du CDC du 15/11/2026) : réglage absent = `"off"` ; `"auto"` le remet. `FrozenRequest::LEGACY` (`"auto"`) pour un en-tête avec `yaTools` sans `yaRequest` et pour un `yaRequest` illisible (un objet sans `reasoningSummary` compte désormais comme illisible). Tests unitaires mis à jour (défaut, gel, ancien en-tête, champ illisible). | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Scénarios `reasoningSummary` : absent = corps sans `summary`, `"auto"` explicite = `summary: "auto"` (préfixe stable, seul `reasoning` diffère entre les deux) ; WebSocket Codex : absent, `"off"` et `"auto"` ; reprise d'une session à l'en-tête sans `yaRequest` (`resumed_legacy_session_keeps_summary_auto`). | YA-09 |
| `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | modif. (fichier du fork) | Tests : un build sans réglage gèle `"off"` ; un en-tête durable avec `yaTools` sans `yaRequest` reste `"auto"`, réglages par défaut compris. | YA-09 |
| `.changes/ya-09-summary-off.md` | ajout | Fragment de changelog du défaut `"off"`. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | Réglage `bashWaitSeconds` (fenêtre de `await bash(cmd)`, secondes entières, au moins 1 ; absent = 1 s comme upstream), figé dans `yaRequest`, écrit seulement s'il ne vaut pas 1 ; avertissement de reprise sur les deux réglages ; tests (défaut, en-tête, gel, valeurs refusées au chargement). Voir « Fenêtre de `bash()` (YA-09) ». | YA-09 |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Champ `bash_wait_seconds` (`bashWaitSeconds`, `NonZeroU32`). | YA-09 |
| `crates/pa-core/src/settings/load.rs` | modif. (1 ligne) | `bashWaitSeconds` dans `KNOWN_FIELDS`. | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (5 lignes) | Lit `bashWaitSeconds`, le passe au gel (`freeze_session_request`), au provisionneur du kernel et aux options du prompt. | YA-09 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (3 lignes) | `kernel_provisioner` prend la fenêtre et la met dans les options du provisionneur. | YA-09 |
| `crates/pa-core/src/kernel/provisioner.rs` | modif. (8 lignes) | Champ `ya_bash_wait_seconds` des options ; le code de bootstrap passe par `bootstrap::ya_bash_wait` après la transformation `ya_native_shell`. | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/ya_bash_wait.rs` | ajout | Ligne de bootstrap qui pose `_YIELD_AFTER_SECONDS` du module `rlm.bash` après `_set_completion_notices_wired(True)`, si la fenêtre ne vaut pas 1 ; tests (repère épinglé au bootstrap upstream, défaut identique, sans gestionnaire ou en shell natif rien n'est posé). | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/mod.rs` | modif. (1 ligne) | Déclare `ya_bash_wait`. | YA-09 |
| `crates/pa-core/src/prompts/ya_bash_wait.rs` | ajout | Fenêtre de la ligne `await bash(cmd)` du prompt ; 1 s ou absent : ligne upstream octet pour octet ; tests. | YA-09 |
| `crates/pa-core/src/prompts/mod.rs` | modif. (3 lignes) | Déclare `ya_bash_wait`. | YA-09 |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. (8 lignes) | Champ `ya_bash_wait_seconds` des options ; la ligne `await bash(cmd)` passe par `ya_bash_wait::bash_await_line` (texte upstream inchangé dans le source). | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Scénario `codex_bash_wait_changes_only_the_await_line` : deux sessions d'un même projet (défaut puis 30 s), préfixes stables, prompts qui ne diffèrent que par la fenêtre, tools et paramètres identiques, en-têtes `yaRequest`. | YA-09 |
| `crates/pa-core/tests/ya_bash_wait_kernel.rs` | ajout | Vrai kernel avec le gestionnaire `bash.completed` : instantané après 1 s par défaut, résultat complet à 30 s, rien de posé sans gestionnaire. | YA-09 |
| `prime-agent-runtime/test/test_ya_bash_wait.py` | ajout | Tests du runtime : la ligne du bootstrap pose la fenêtre ; commande dans et au-delà de la fenêtre ; annulation d'un `await` dans une fenêtre de 30 s en moins de 1 s (SIGTERM respecté ou ignoré). Aucun fichier du runtime modifié. | YA-09 |
| `bench/smoke/settings/bash-wait-30.json` | ajout | Réglage du bras B de l'A/B `bashWaitSeconds`. | YA-09 |
| `bench/smoke/ab/ya09-bash-plan.md` | ajout | Plan et règle de verdict de l'A/B `bashWaitSeconds`, graine. | YA-09 |
| `bench/smoke/ab/ya09-bash-mesures.py` | ajout | Mesures de la règle 3 du plan (appels de la session principale, garde-fou de durée) et affichages (médiane par lancement, relèves, notifications, silences WebSocket, bascules), validées sur l'A/B `reasoningSummary`. | YA-09 |
| `bench/smoke/ab/ya09-bash-durees.py` | ajout | Durées des commandes du run YA-14 (Codex et yusAgent), couverture par fenêtre, compte 59 + 11 = 70 cellules. | YA-09 |
| `bench/smoke/ab/ya09-bash-count.py` | ajout | Tokens du prompt système et des tools (yusAgent depuis un corps capturé, Unharnessed depuis un clone), tokenizer `o200k_base` supposé ; tiktoken installé par `uv run --with`, pas de dépendance du dépôt. | YA-09 |
| `bench/smoke/tools/check_warmup.py` | modif. (fichier du fork) | Contrôles de `bash()` : fenêtre de la ligne du prompt, cellules revenues avant la fin, attentes complètes de plus de 1 s, notifications, plus long silence WebSocket, mentions de 1011. | YA-09 |
| `.changes/ya-09-bash-wait.md` | ajout | Fragment de changelog du réglage `bashWaitSeconds`. | YA-09 |
| `crates/pa-cli/src/prompt_command.rs` | modif. (2 lignes + test) | `prime-agent prompt` affiche la fenêtre `await bash(cmd)` des réglages courants (`bashWaitSeconds`) ; test `shows_the_bash_wait_of_the_settings`. Les autres réglages de session (`yaTools`, démon) restent ignorés par cette commande, comme avant. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | **Défaut de `bashWaitSeconds` à 30 s** (décision de Yusuf du 15/11/2026, journal du CDC). Un `yaRequest` sans le champ (toute session créée avant) vaut 1 s : son prompt ne change pas à la reprise. 1 n'est pas écrit dans l'en-tête, toute autre valeur l'est (30 par défaut). `resolve_bash_wait_seconds` pour `prime-agent prompt`. Tests mis à jour. | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Défaut à 30 s : `codex_bash_wait_changes_only_the_await_line` compare le défaut (30 s) à `bashWaitSeconds: 1` ; nouveau scénario `resumed_session_from_before_bash_wait_keeps_one_second` (en-tête sans le champ, reprise avec le défaut : ligne à 1 s, préfixe stable, en-tête inchangé) ; en-tête attendu de `resumed_legacy_session_keeps_summary_auto`. | YA-09 |
| `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | modif. (fichier du fork) | Tests : en-tête `yaRequest` d'une nouvelle session avec `bashWaitSeconds: 30`. | YA-09 |
| `crates/pa-daemon/tests/autobg_headless_e2e.rs` | modif. (test upstream, 4 lignes + commentaire) | Le test vérifie la dégradation de `await bash()` à 1 s : le `settings.json` de son dossier d'agent remet `bashWaitSeconds: 1` (défaut du fork : 30 s). | YA-09 |
| `prime-agent-runtime/src/rlm/ya_timeout.py` | ajout | `bash(timeout=...)` : limite par appel, défaut de session `DEFAULT_TIMEOUT` (`None` tant que le bootstrap ne le pose pas), aucune limite par défaut pour un shell persistant, minuteur qui arrête la commande (`BashHandle.kill` : SIGTERM puis SIGKILL après 5 s), ligne dans la sortie, champ `timeoutSeconds` de l'avis. Repris d'Unharnessed (`569284b36`), voir « Code repris d'Unharnessed ». Voir « Timeout et avis groupés de `bash()` (YA-09) ». | YA-09 |
| `prime-agent-runtime/src/rlm/bash.py` | modif. (fichier upstream, 26 lignes) | Paramètre `timeout` en dernier de `bash()` (`ya_timeout.UNSET` par défaut), résolu puis armé par `ya_timeout` ; `timeoutSeconds` ajouté au payload `bash.completed` des deux chemins d'avis (`_notify_background_completion`, `PersistentBashHandle._notify_completion`) ; import de `ya_timeout` ; docstring. | YA-09 |
| `prime-agent-runtime/test/test_ya_bash_timeout.py` | ajout | Tests du runtime : résolution du défaut et valeurs refusées, arrêt après la limite avec la ligne, groupe de processus arrêté, SIGKILL après 5 s pour une commande qui ignore SIGTERM, minuteur annulé par une commande finie, défaut de session, champ `timeoutSeconds` de l'avis (REPL réel). | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/venv/probe.rs` | modif. (1 ligne + commentaire) | `RUNTIME_READY_CHECK` : signature attendue de `rlm.bash` `['command', 'persistent', 'env', 'timeout']` (sans cela, le runtime de la branche est jugé « pas prêt »). | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/runtime_code.rs` | modif. (1 ligne) | Repli sans runtime : `def bash(command, persistent=False, env=None, timeout=None)`. | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/ya_native_shell.rs` | modif. (fichier du fork, 1 ligne) | Repère `FALLBACK_BASH` aligné sur la nouvelle ligne du repli. | YA-09 |
| `crates/pa-core/src/session_engine/ya_bash_timeout.rs` | ajout | Avis `[bash-done]` d'une commande arrêtée par sa limite : `Killed: still running after its N s timeout.` en fin de texte, `timeoutSeconds` dans les `details` ; valeur invalide refusée. Tests unitaires. | YA-09 |
| `crates/pa-core/src/session_engine/ya_bash_notice_batch.rs` | ajout | Réglage `bashNoticeBatchSeconds` (défaut 120, 0 = upstream) : avis `[bash-done]` d'une session à l'arrêt retenus puis mis en file ensemble avec un seul réveil (fin de la dernière commande, aucune commande en cours, ou fenêtre écoulée) ; avis pendant une exécution inchangé ; retrait d'un avis retenu par `bash.consumed`. Hôte sans démon seulement. Tests unitaires (horloge figée). | YA-09 |
| `crates/pa-core/src/session_engine/bash_notices.rs` | modif. (fichier upstream, ~15 lignes) | `validate_completion` passe l'avis par `ya_bash_timeout::mark_timeout`. Les sinks en processus de `register_in_process_kernel_notice_handlers` sortent dans `in_process_kernel_notice_sinks` (`pub(super)`, corps inchangé) ; `bound_agent` devient `pub(super)`. | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (12 lignes) | Lit `bashNoticeBatchSeconds` ; enregistre les sinks par `ya_bash_notice_batch::register_in_process_handlers` au lieu de `register_in_process_kernel_notice_handlers` ; enveloppe le callback de fin des commandes du kernel (`settled_callback`) ; branche la sonde des commandes en cours sur le provisionneur (`bind_running_probe`). | YA-09 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (4 lignes) | Déclare `ya_bash_notice_batch` et `ya_bash_timeout`. | YA-09 |
| `crates/pa-core/src/settings/types.rs` | modif. (4 lignes) | Champ `bash_notice_batch_seconds` (`bashNoticeBatchSeconds`). | YA-09 |
| `crates/pa-core/src/settings/load.rs` | modif. (1 ligne) | `bashNoticeBatchSeconds` dans `KNOWN_FIELDS`. | YA-09 |
| `crates/pa-core/tests/ya_bash_timeout_kernel.rs` | ajout | Vrai kernel avec les gestionnaires d'avis d'une session : le kernel démarre avec la nouvelle signature, une commande détachée arrêtée par sa limite donne un avis avec la raison, une commande finie à temps un avis normal. | YA-09 |
| `crates/pa-cli/tests/ya_bash_notice_batch_e2e.rs` | ajout | Binaire réel en `-p`, provider factice, vrai kernel : trois avis après le tour = un réveil ; fenêtre 0 = un réveil par avis ; un avis pendant l'exécution rejoint l'appel suivant ; fenêtre courte = avis relâché pendant qu'une commande tourne encore. | YA-09 |
| `docs/tickets/11-ya09-unharnessed.md`, `docs/tickets/11-ya09-unharnessed/` | ajout | Plan du lot Unharnessed, décisions du 16/11/2026, brouillons du prompt v2 comptés. | YA-09 |
| `.changes/ya-09-bash-runtime.md` | ajout | Fragment de changelog. | YA-09 |
| `bench/smoke/tools/edits.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Appels à `edit()`, échecs et écritures de fichiers hors `edit()` par tâche d'un run (vérification des chauffes de la PR B du ticket 11) ; ligne du tableau des outils. | YA-09 |
| `bench/smoke/tools/notice-batch-repro.sh`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Rejeu factice du regroupement des avis `[bash-done]` avec un binaire donné (provider factice, aucun appel réel) ; ligne du tableau des outils. | YA-09 |
| `crates/pa-core/src/prompts/ya_prompt_v2.rs`, `crates/pa-core/src/prompts/ya_prompt_v2/core.md` | ajout | Prompt système version 2 (`promptVersion: 2`, défaut) : préfixe statique court (texte de départ : `core.md` d'Unharnessed), variantes par tools (`apply_patch`, `exec_command`), inventaire compact des skills (skills fournis masqués : `edit`, `refine`, `skill-creator`, `prime-intellect`, et `agent-message`/`agent-observe` sans sous-agents ; descriptions courtes des skills fournis affichés), ligne « no daemon » v2, description courte de `python_repl`. Tests unitaires. Voir « Prompt version 2 (YA-09, ticket 11) ». | YA-09 |
| `crates/pa-core/src/prompts/mod.rs` | modif. (3 lignes) | Déclare `ya_prompt_v2`. | YA-09 |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. (fichier upstream, ~45 lignes) | Champ `ya_prompt_version` de `BuildSystemPromptOptions` (`None` = version 1, défaut des tests upstream) ; en version 2 : préfixe statique de `ya_prompt_v2` au lieu des couches `core`, `usage`, `opinionated` ; inventaire compact (section omise si vide) ; pas de ligne `Recursive agent depth` pour une racine ; ligne « no daemon » v2. Version 1 inchangée octet pour octet. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | `PromptVersion` (nombre 1 ou 2, tout autre refusé), réglage `promptVersion` figé dans `yaRequest` au champ `promptVersion`, écrit sauf s'il vaut 1 ; **un `yaRequest` sans le champ vaut 1** (option A du ticket 11 : sessions créées avant le lot) ; `LEGACY` en version 1 ; avertissement de reprise sur les trois réglages. Variable `PRIME_AGENT_PROMPT_VERSION` lue sous le réglage (`prompt_version_setting`). **Refus d'une session version 2 qui peut avoir des sous-agents** (enfant, ou racine à `rlmMaxDepth` ≥ 1 ; `check_subagents`, message qui indique `"promptVersion": 1`), vérifié avant le gel des réglages, sur la version lue sans écrire l'en-tête (`session_prompt_version`). Tests. | YA-09 |
| `crates/pa-core/src/settings/types.rs`, `crates/pa-core/src/settings/load.rs` | modif. (4 et 1 lignes) | Champ `prompt_version` (`promptVersion`), clé connue. | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (26 lignes) | Lit `promptVersion` (et la variable d'environnement), refuse une session version 2 avec sous-agents avant le gel, le fige avec la requête, le passe au prompt ; description courte de `python_repl` en version 2 ; limite `bash()` par défaut de 600 s passée au provisionneur en version 2. | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/ya_bash_timeout.rs` | ajout | Ligne de bootstrap qui pose `rlm.ya_timeout.DEFAULT_TIMEOUT` (600 s) après la liaison de `read`, pour une session version 2 ; tests (repère épinglé au bootstrap upstream et au shell natif). | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/mod.rs` | modif. (1 ligne) | Déclare `ya_bash_timeout`. | YA-09 |
| `crates/pa-core/src/kernel/provisioner.rs` | modif. (8 lignes) | Champ `ya_bash_timeout_seconds` des options ; le bootstrap passe par `ya_bash_timeout` après `ya_bash_wait`. | YA-09 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (3 lignes) | `kernel_provisioner` prend la limite par défaut et la met dans les options. | YA-09 |
| `crates/pa-core/src/session_engine/ya_tool_family.rs` | modif. (fichier du fork) | **`editTool` absent = `edit()` du kernel sur toutes les familles** (était `apply_patch` sur Codex) : un seul tool par défaut ; `"native"` garde `apply_patch` (Codex seulement). Les sessions déjà figées gardent leurs tools. Tests. Écart au CDC, journal du 16/11/2026. | YA-09 |
| `skills/edit/src/edit/__init__.py`, `skills/edit/SKILL.md` | modif. (skill upstream ; corps du `SKILL.md` seulement, en-tête inchangé car l'inventaire v1 l'affiche) | `old_str` vide : création du fichier (dossiers parents créés, fichier existant refusé) ; retour : chemin et lignes modifiées avec 3 lignes de contexte, numérotées comme `read()` ; refus explicites (texte absent avec « 0 occurrences », N occurrences, `old_str` vide sur un fichier existant, fichier absent avec la consigne de création) ; chemin hors du workspace (dossier de départ du kernel) refusé par `PermissionError`, comme garde-fou et non comme sécurité. Le retour n'est plus le seul « Edited <path> » que la TUI reconnaît (`crates/pa-tui/src/tool_card/ipython_details.rs:161-171`) : la carte affiche alors le texte complet. | YA-09 |
| `prime-agent-runtime/test/test_ya_edit.py` | ajout | Tests du skill `edit` : retour numéroté avec contexte, création, refus et leurs messages (motif `EDIT_FAILURE` du banc), chemin hors du workspace. | YA-09 |
| `bench/smoke/metrics.py` | modif. (fichier du fork) | `EDIT_FAILURE` reconnaît les nouveaux messages d'échec d'`edit()` (fichier absent avec consigne, `old_str` vide, hors workspace) ; les anciens restent lus. | YA-09 |
| `bench/smoke/settings/prompt-v1.json` | ajout | `{"promptVersion": 1}` : bras du prompt d'avant le lot, pour une mesure future (Terminal-Bench). | YA-09 |
| `crates/pa-cli/src/prompt_command.rs` | modif. (7 lignes + test) | `prime-agent prompt` affiche la version de prompt des réglages courants (réglage, sinon `PRIME_AGENT_PROMPT_VERSION`, sinon 2) ; test `shows_the_prompt_version_of_the_settings`. | YA-09 |
| `.cargo/config.toml` | modif. (5 lignes) | `PRIME_AGENT_PROMPT_VERSION = "1"` dans `[env]`, à côté de `RLM_MAX_DEPTH = "2"` : les tests upstream gardent le prompt upstream (version 1) avec le spawn à 2, que la version 2 refuse. Lu sous le réglage `promptVersion` ; un test qui pose ou retire la variable garde sa valeur (`prefix_stability.rs` retire les deux pour les défauts du produit). Effet de bord : un `cargo run` local démarre aussi en version 1 ; les binaires de la CI et du banc ne lisent pas ce fichier. | YA-09 |
| `bench/smoke/README.md` | modif. (fichier du fork) | Note : « edits au premier essai » n'est pas comparable d'avant en après la PR B (défaut `edit()`, nouveaux refus, motif `EDIT_FAILURE` élargi). | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Défaut Codex à un tool et prompt v2 (texte, description courte) ; `editTool: "native"` en variante ; shell natif sans `apply_patch` ; en-têtes `yaRequest` avec `promptVersion: 2` ; reprise et fork créés avec `editTool: "native"` ; WebSocket `apply_patch` avec `editTool: "native"` ; nouveau scénario `resumed_session_from_before_prompt_v2_keeps_the_v1_prompt` (option A) ; nouveau scénario `prompt_version_two_refuses_subagents` ; le Fixture retire `RLM_MAX_DEPTH` et `PRIME_AGENT_PROMPT_VERSION` (défauts du produit). | YA-09 |
| `crates/pa-core/tests/ya_bash_timeout_kernel.rs` | modif. (fichier du fork) | Vrai kernel : la ligne de bootstrap d'une session version 2 pose la limite par défaut (600 s), rien sans elle. | YA-09 |
| `.changes/ya-09-prompt-v2.md` | ajout | Fragment de changelog. | YA-09 |
| `crates/pa-cli/tests/ya_prompt_v2_e2e.rs` | ajout | Binaire réel en `-p`, provider factice, vrai kernel, sur les défauts du produit (`RLM_MAX_DEPTH` et `PRIME_AGENT_PROMPT_VERSION` retirés) : session version 2 qui crée puis édite un fichier avec `edit()` et lance un `bash()` (limite par défaut 600 s), prompt v2 et en-tête vérifiés ; `rlmMaxDepth: 1` refusé au démarrage avec le message. | YA-09 |
| `docs/usage.md` | modif. (fichier du fork) | Section « Variables d'environnement qui changent la requête » : `RLM_MAX_DEPTH` et `PRIME_AGENT_PROMPT_VERSION` (réglage prioritaire, valeurs, effet sur le prompt, combinaison refusée, valeurs de `.cargo/config.toml`). | YA-09 |
| `scripts/check_ledger_liveness.py`, `check_dead_child_delete.py`, `check_observe_selectors.py`, `check_factory_resumption.py`, `check_late_message_receipt.py`, `check_restore_notice.py` | modif. (scripts upstream, 1 à 2 lignes chacun) | Vérifications de cycle de vie (CI, shard 1) qui lancent le binaire hors de `cargo`, avec un environnement en liste blanche (la variable de `.cargo/config.toml` n'y arrive pas) : `promptVersion: 1` dans leur `settings.json`, à côté de `rlmMaxDepth: 2` pour les cinq qui font un spawn (refusé en version 2), et pour `check_restore_notice.py` qui lit l'inventaire XML des skills de la version 1. Constat du run 43835681084 (`ledger_liveness`, `restore_notice`). | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/mod.rs`, `.changes/fix-codex-ws-release-on-send-error.md` | cherry-pick d'upstream | Commit upstream `c4800c8eb` (#4010), repris en entier avec `-x` : un `send_request` en échec (abandon juste avant l'envoi) libère la connexion en cache au lieu de la laisser occupée, et le test upstream `an_abort_before_the_send_releases_the_cached_connection`. Le prochain cherry-pick de ce commit est vide. Voir « Keepalive du WebSocket Codex » ci-dessous. | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/ya_idle.rs` | ajout | Lecture du socket pendant le silence entre deux requêtes : pongs, fin du worker sur fermeture, comptage des messages reçus hors requête et leur diagnostic, retrait de la connexion morte du cache. Tests unitaires. | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs` | modif. (fichier upstream, ~30 lignes) | `connection_worker` lit le flux pendant le silence (`tokio::select!`, priorité aux commandes) et reçoit l'id de sa connexion ; `acquire_websocket` retire d'abord une connexion dont le worker est arrêté. | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/mod.rs` | modif. (fichier upstream, 8 lignes) | Déclare `ya_idle` ; pose le diagnostic `websocket_idle_messages_ignored` après `acquire_websocket` ; déclare le module de tests `ya_keepalive`. | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/tests/ya_keepalive.rs` | ajout | Serveur factice WebSocket et SSE : pongs pendant le silence, reconnexion en WebSocket après une fermeture pendant le silence, message égaré compté et non livré, non-régression du repli SSE. | YA-09 |
| `.changes/ya-09-ws-keepalive.md` | ajout | Fragment de changelog. | YA-09 |
| `bench/smoke/tasks/py-silence-001/`, `bench/smoke/run.py`, `bench/smoke/tools/ab.py`, `bench/smoke/tests/test_bench.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Tâche de chauffe `py-silence-001` (commande de 180 s), tag `chauffe` exclu des tâches tirées par défaut, test de la sélection. | YA-09 |
| `bench/smoke/tools/compare_bodies.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Changements de requête entre deux chauffes (`proxy-bodies.ndjson`) : clés du premier corps identiques ou différentes, jeux de clés, forme de l'`input`. Lecture seule. | YA-09 |
| `bench/smoke/tools/cache_calls.py`, `bench/smoke/ab/ya09-cache-v2-plan.md`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Miss de cache par appel, relectures partielles, taux de cache, mesure par bras d'un A/B ; plan et règle de l'A/B du cache du prompt v2 (prompt v2 contre v1). | YA-09 |
| `docs/tickets/12-ya09-ws-keepalive.md` | ajout | Ticket 12 : constat, recherche upstream, cause, décisions du 16/11/2026, tests, résultats des deux chauffes, note pour le ticket de l'attente complète de `bash()`. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | Ticket 13 : `bashWaitSeconds` devient `BashWait` (nombre de secondes ou `"full"`, sérialisé tel quel dans les réglages et `yaRequest`) ; réglage absent : `"full"` en prompt v2 (D2), 30 s en prompt v1 (D6, `resolve_bash_wait`) ; `check_bash_wait` refuse une session qui tournerait en v1 avec `"full"` (donc un `"full"` explicite, D3 et D6), avant le gel, message qui nomme `"bashWaitSeconds": 30` ; tests. | YA-09 |
| `crates/pa-core/src/kernel/bootstrap/ya_bash_wait.rs` | modif. (fichier du fork) | Ticket 13 : `"full"` pose `_YIELD_AFTER_SECONDS = None` après le basculement des avis (chemin sans borne du runtime) ; test. | YA-09 |
| `crates/pa-core/src/prompts/ya_bash_wait.rs` | modif. (fichier du fork) | Ticket 13 : en `"full"`, **pas de ligne `await bash(cmd)`** (`bash_await_line` rend `None`), comme Unharnessed qui n'en a pas (décision A du 17/11/2026) ; ligne upstream épinglée ; note du watchdog de cellule réécrite en `"full"` (D5, `watchdog_note`, phrase upstream épinglée sur le source d'`execution.rs`) ; tests. | YA-09 |
| `crates/pa-core/src/settings/types.rs` | modif. (1 ligne + commentaire) | Ticket 13 : type de `bash_wait_seconds` (`BashWait`). | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (7 lignes) | Ticket 13 : appel de `check_bash_wait` avant le gel des réglages de requête. | YA-09 |
| `crates/pa-core/src/kernel/provisioner.rs`, `crates/pa-core/src/prompts/system_prompt.rs` | modif. (type, 1 ligne chacun ; accesseur de 6 lignes dans `provisioner.rs` ; dans `system_prompt.rs`, `lines.push` devient `lines.extend` sur la ligne `await bash(cmd)` + 1 ligne de commentaire) | Ticket 13 : champ `ya_bash_wait_seconds` en `BashWait` ; accesseur `ya_bash_wait_seconds()` du provisionneur ; en `"full"`, la ligne `await bash(cmd)` n'est pas poussée (A.1, 17/11/2026). | YA-09 |
| `crates/pa-core/src/prompts/ya_prompt_v2/core.md`, `crates/pa-core/src/prompts/ya_prompt_v2.rs` | modif. (fichiers du fork, 2 lignes et 2 constantes) | Ticket 13, A.2 et A.3 (17/11/2026), alignement sur Unharnessed `fe2cf2815` (`core.md:8`, `:19`) : `and \`await handle -> BashResult\`` ; règle « Do not hold the turn with `sleep` or long blocking awaits: end the turn and wait for the notification. ». **Rupture de préfixe acceptée par Yusuf** : toute session v2 existante voit son préfixe changer une fois à la reprise. | YA-09 |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (8 lignes) | Ticket 13 : type du paramètre ; `KernelManagerExecutor` garde l'attente de la session et passe le stderr de chaque cellule par `ya_bash_wait::watchdog_note` (D5). `kernel/manager/execution.rs` n'est pas modifié. | YA-09 |
| `crates/pa-cli/src/prompt_command.rs` | modif. (fichier upstream, 8 lignes) | Ticket 13 : `prime-agent prompt` résout l'attente avec la version du prompt des réglages (`resolve_bash_wait`). Hors de la liste du plan du ticket : imposé par le changement de type. | YA-09 |
| `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs` | modif. (1 ligne + commentaire) | Ticket 13, D4 : `SESSION_WEBSOCKET_CACHE_TTL_MS` à 15 min au lieu de 5, pour qu'une cellule attendue jusqu'au timeout de 600 s ne ferme pas la connexion. Test dans `ya_idle.rs`. | YA-09 |
| `crates/pa-core/tests/ya_bash_wait_kernel.rs`, `crates/pa-core/tests/prefix_stability.rs`, `crates/pa-cli/tests/ya_prompt_v2_e2e.rs`, `prime-agent-runtime/test/test_ya_bash_wait.py` | modif. (fichiers du fork) | Ticket 13 : vrai kernel en `"full"` (global `None`, résultat complet, aucun avis pour une commande attendue, avis pour une commande laissée en cours) ; défaut v2 `"full"` dans les scénarios de préfixe ; E2E `-p` (cellule de 3 s complète, sans `[bash-done]`) ; runtime (attente complète, annulation sous 1 s, aucun avis). | YA-09 |
| `bench/smoke/settings/prompt-v1.json`, `bench/smoke/tasks/py-silence-002/`, `bench/smoke/tools/check_warmup.py`, `bench/smoke/tests/test_bench.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Ticket 13 : bras v1 à 30 s (D3) ; tâche de chauffe de 400 s attendue dans la cellule ; contrôles de chauffe (ligne `full`, cellules complètes après 30 s, connexions, préfixe) ; garde-fou d'inactivité et longues cellules. | YA-09 |
| `.changes/ya-09-bash-full-wait.md`, `docs/tickets/13-ya09-bash-full-wait.md` | ajout, modif. | Fragment de changelog ; ticket 13. | YA-09 |
| `crates/pa-core/src/session_engine/ya_session_id_header.rs` | ajout | Ticket 14, option A : en-tête `session-id` (tiret, comme Codex CLI) égal à l'id de session, ajouté aux en-têtes de requête des options du `stream_fn` quand `codexSessionIdHeader` est vrai et que le modèle est servi par `openai-codex` sur `openai-codex-responses`. Jamais pour un autre provider. Tests unitaires. Voir « En-tête `session-id` (YA-09, ticket 14) ». | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | Ticket 14 : champ `codexSessionIdHeader` de `yaRequest` (toujours écrit, `true` ou `false` ; absent = session d'avant le champ = coupé), `FrozenRequest::with_codex_session_id_header`, `freeze_current_request` (gel avec des réglages déjà résolus ; `freeze_session_request` garde sa signature), libellé de l'avertissement de reprise ; `scoped_stream_fn` enveloppe aussi l'en-tête ; tests. | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (9 lignes) | Ticket 14 : lit `codexSessionIdHeader` avant le déplacement des réglages (`:291`) et le gèle avec les autres réglages de requête (`freeze_current_request`, `:358-367`). | YA-09 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Ticket 14 : déclare `ya_session_id_header` (`:66-67`). | YA-09 |
| `crates/pa-core/src/settings/types.rs` | modif. (3 lignes) | Ticket 14 : champ `codex_session_id_header` (`codexSessionIdHeader`, `:344-346`). | YA-09 |
| `crates/pa-core/src/settings/load.rs` | modif. (2 lignes) | Ticket 14 : `codexSessionIdHeader` dans `KNOWN_FIELDS` (`:79-80`). | YA-09 |
| `crates/pa-cli/tests/ya_prompt_v2_e2e.rs`, `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | modif. (fichiers du fork) | Ticket 14 : `yaRequest` attendu avec `"codexSessionIdHeader": false`. | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Ticket 14 : `yaRequest` attendu avec `"codexSessionIdHeader": false` ; les serveurs locaux gardent aussi les en-têtes reçus (`ModelServer::heads`, `CodexWsServer::handshakes`) ; tests sur le fil : en-tête présent seulement réglage activé sur le WebSocket Codex, trames identiques octet pour octet hors `prompt_cache_key`, absent pour Anthropic et `OpenAI` Responses. | YA-09 |
| `bench/smoke/settings/session-id-header.json`, `bench/smoke/ab/ya09-session-id-plan.md`, `bench/smoke/ab/ya09-session-id-mesures.py`, `bench/smoke/tools/power.py`, `bench/smoke/tools/drift_se.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Ticket 14 : bras B de l'A/B, plan et règle fixés avant le run, mesures, contrôle sur le fil en mode capture (`--fil`), puissance estimée par simulation, bruit des relectures partielles pour le contrôle de la dérive v1. | YA-09 |
| `.changes/ya-09-session-id.md`, `docs/tickets/14-ya09-cache-miss.md` | ajout, modif. | Fragment de changelog ; ticket 14. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | Ticket 18, phase 3 : champ `existingTestsRule` de `yaRequest` (toujours écrit, `true` ou `false` ; absent = session d'avant le champ = coupé), `FrozenRequest::with_existing_tests_rule`, `check_existing_tests_rule` (refus d'une nouvelle session en prompt v1 avec la règle, avant le gel), libellé de l'avertissement de reprise ; tests. | YA-09 |
| `crates/pa-core/src/prompts/ya_prompt_v2.rs` | modif. (fichier du fork) | Ticket 18, phase 3 : `static_prefix(choice, existing_tests_rule)` ajoute la ligne `RULE_EXISTING_TESTS` après `RULE_SMALLEST_CHANGE` ; `core.md` inchangé ; tests : préfixe sans la règle égal à celui de `main` (`0c0c11a63`, longueur et SHA-256 pour les quatre choix de tools), une seule ligne en plus avec la règle. | YA-09 |
| `crates/pa-core/src/prompts/system_prompt.rs` | modif. (fichier upstream, 8 lignes ajoutées, 1 retirée) | Ticket 18, phase 3 : champ `ya_existing_tests_rule: Option<bool>` de `BuildSystemPromptOptions` (un 4ᵉ `bool` ferait échouer `clippy::struct_excessive_bools`), passé à `ya_prompt_v2::static_prefix` en version 2. Version 1 inchangée. | YA-09 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (16 lignes ajoutées, 6 retirées) | Ticket 18, phase 3 : lit `existingTestsRule` avant le déplacement des réglages, le gèle avec les autres réglages de requête après `check_existing_tests_rule`, passe la valeur figée aux options du prompt. | YA-09 |
| `crates/pa-core/src/settings/types.rs`, `crates/pa-core/src/settings/load.rs` | modif. (3 et 2 lignes) | Ticket 18, phase 3 : champ `existing_tests_rule` (`existingTestsRule`), clé connue. | YA-09 |
| `crates/pa-cli/src/prompt_command.rs` | modif. (fichier upstream, 3 lignes + test de 19 lignes) | Ticket 18, phase 3 : `prime-agent prompt` montre la règle quand les réglages courants l'activent (version 2) ; test `shows_the_existing_tests_rule_of_the_settings`. | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs`, `crates/pa-cli/tests/ya_prompt_v2_e2e.rs`, `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` | modif. (fichiers du fork) | Ticket 18, phase 3 : `yaRequest` attendu avec `"existingTestsRule": false` ; scénario `codex_existing_tests_rule_adds_only_its_line` (défaut puis règle : préfixes stables, prompts qui ne diffèrent que par la ligne, tools et paramètres identiques, en-têtes `false` puis `true`) ; `existing_tests_rule_is_refused_in_prompt_version_one`. | YA-09 |
| `bench/smoke/tools/test_edits.py`, `bench/smoke/tests/test_test_edits.py`, `bench/smoke/settings/existing-tests-rule.json`, `bench/smoke/ab/ya18-tests-plan.md`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Ticket 18, phase 3 : exclusions de tests dans les commandes de test et `.only` ajouté (détecteur et tests) ; bras B de l'A/B ; plan et règle fixés avant le run. | YA-09 |
| `.changes/ya-09-existing-tests-rule.md`, `docs/tickets/18-ya09-tests-existants.md`, `CLAUDE.md` | ajout, modif. | Fragment de changelog ; ticket 18, section « Phase 3 » ; règle de relance CI sur panne d'infrastructure. | YA-09 |
| `crates/pa-core/src/session_engine/ya_request.rs` | modif. (fichier du fork) | Ticket 18, verdict de l'A/B (`bench/smoke/ab/ya18-tests-20261119.md`) : `existingTestsRule` **activé par défaut en prompt v2**, coupé par défaut en v1 (`resolve_existing_tests_rule`, appelé par `FrozenRequest::with_existing_tests_rule`) ; un `true` explicite en v1 reste refusé ; un `yaRequest` sans le champ reste coupé. Tests : défaut par version, gel, reprise d'une session figée sans la règle (avertissement). | YA-09 |
| `crates/pa-cli/src/prompt_command.rs` | modif. (fichier upstream, 8 lignes ajoutées, 2 retirées, + test) | Ticket 18, verdict de l'A/B : `prime-agent prompt` résout le défaut de `existingTestsRule` avec la version du prompt des réglages (`resolve_existing_tests_rule`), comme `bashWaitSeconds` ; test `shows_the_existing_tests_rule_of_the_settings` inversé (réglage absent : règle présente ; `false` : absente). | YA-09 |
| `crates/pa-core/tests/prefix_stability.rs`, `crates/pa-cli/tests/ya_prompt_v2_e2e.rs` | modif. (fichiers du fork) | Ticket 18, verdict de l'A/B : `yaRequest` attendu avec `"existingTestsRule": true` pour une nouvelle session v2 ; scénario `codex_existing_tests_rule_adds_only_its_line` joué avec le défaut (règle) puis `existingTestsRule: false` ; l'e2e v2 vérifie la ligne dans le prompt. | YA-09 |
| `.changes/ya-09-existing-tests-rule-on.md` | ajout | Fragment de changelog. Le bras de contrôle `bench/smoke/settings/existing-tests-rule-off.json` et la note du README du banc (`default.json` = bras B, baseline sans la règle) sont dans la PR du rapport (#57). | YA-09 |



## Crate `ya-apply-patch` (YA-03)

Ticket 09, option A validée par Yusuf le 13/11/2026. Source : openai/codex, `codex-rs/apply-patch`, SHA `69231e119bfca0b14453515e97d99688ce1c47c9`. Le crate d'origine dépend de `codex-exec-server` (environ 25 crates internes de Codex, `axum`, `opentelemetry`, `prost`) et de `tree-sitter`/`tree-sitter-bash` (build C) : incompatible avec la règle 8. Détail fichier par fichier : `crates/ya-apply-patch/README.md`.

- **À l'identique** (hash de blob git égal à celui de Codex) : `src/seek_sequence.rs`, `src/streaming_parser.rs`, `src/text_file.rs`, `tests/all.rs`, `tests/suite/mod.rs`, `tests/fixtures/**` (fichiers CRLF compris), `rustfmt.toml`.
- **Modifiés** (en-tête `// Modified by yusAgent: …` dans chaque fichier) : accès disque passés de `ExecutorFileSystem` (async) à `std::fs` (sync, nouveau `src/local_fs.rs`), `PathUri` remplacé par `Path`/`PathBuf`, plus de paramètre `sandbox`. Le calcul du nouveau contenu ne change pas : dans `file_update.rs`, la lecture du fichier et le calcul sont séparés, le calcul passe sans changement dans `derive_new_contents_from_text`. Dans `lib.rs`, `apply_hunks_to_files` ne change que par ses appels disque.
- **Retirés** : `src/invocation.rs` (détection d'`apply_patch` dans une commande shell, tree-sitter) et les types qu'il produisait (`MaybeApplyPatchVerified`, `ApplyPatchAction`, `ApplyPatchFileChange`, `ApplyPatchError::ImplicitInvocation`, `ApplyPatchError::PathUri`), `BUILD.bazel`.
- **Tests** : 67 tests de Codex repris (39 unitaires, 28 d'intégration dont 25 scénarios de fixtures), adaptés seulement pour `Path` et `std::fs` (sync au lieu d'async) ; chemins du binaire et des fixtures par `CARGO_BIN_EXE_apply_patch` / `CARGO_MANIFEST_DIR` au lieu de `codex_utils_cargo_bin` (crate interne de Codex) ; dans `no_follow_rechecks_paths_after_verification`, l'étape `verify_apply_patch_args` (d'`invocation.rs`) est remplacée par `parse_patch`. 28 tests retirés, tous dans `invocation.rs`. Le binaire `apply_patch` est gardé : les tests `cli`, `tool` et `scenarios` le lancent.
- **Mode sans suivi des liens** (`follow_symlinks: false`) : Codex l'applique de façon atomique (`openat(O_NOFOLLOW)` avec `rustix`) ; ici, vérification `symlink_metadata` de chaque composant avant l'opération, donc **non atomique** (un échange de lien entre la vérification et l'écriture n'est pas couvert). Pas d'`O_NONBLOCK` à l'ouverture : `is_file()` est vérifié avant. À reprendre avec le sandbox (YA-11) si ce mode sert.
- **Manifeste** : édition 2024 (celle de Codex), table de lints propre au crate (les `[workspace.lints.clippy]` de `codex-rs/Cargo.toml`, plus `unsafe_code = "forbid"`) au lieu du gate pedantic du workspace ; `clippy.toml` (sous-ensemble de `codex-rs/clippy.toml` : `unwrap`/`expect` permis dans les tests) ; `rustfmt.toml` de Codex. Dépendances : `anyhow`, `thiserror` (workspace), `similar` 2.7.0 ; tests : `assert_cmd`, `pretty_assertions`, `tempfile`.

## Tools natifs (YA-03, PR 2)

Ticket 09, sous-ensemble et textes validés par Yusuf le 13/11/2026.

- **Famille** (`session_engine/ya_tool_family.rs`) : Codex quand le modèle est dans `CODEX_MODELS` (les 12 modèles de `models.json` au SHA `69231e1`) **et** servi par `openai`/`openai-responses`, `openai-codex`/`openai-codex-responses` ou `azure-openai-responses` ; tout le reste, dont un modèle inconnu en `openai-responses` ou un déploiement Azure au nom libre, est « autre ». Aucun format câblé hors de ce tableau.
- **Réglages figés à la création** : `editTool` absent = `apply_patch` sur Codex, `edit()` du kernel ailleurs ; `"native"` explicite hors Codex refusé. `shellTool: "native"` hors Codex refusé. Un changement de modèle vers une autre famille est refusé (`set_model`, `set_model_and_thinking_level`).
- **Gel dans le fichier de session** (`ya_tool_family::freeze_session_tools`, `session/manager/ya_header.rs`) : à la création, les valeurs effectives (`{"editTool": "native"|"kernel", "shellTool": "native"|"kernel"}`, un réglage absent étant écrit avec sa valeur résolue) vont dans l'en-tête du fichier de session, champ `yaTools` (le `rest` aplati de `SessionHeader`, qui accepte déjà les champs inconnus). L'en-tête est lu sans parcourir l'historique (une ouverture fenêtrée le garde), n'ajoute aucun nœud à l'arbre et n'entre pas dans le contexte du modèle. À la reprise, les tools viennent de l'en-tête ; si les réglages courants donnent d'autres tools, un avertissement part sur stderr et les tools de la session sont gardés. Une session créée avant ce champ est figée à sa première reprise (en-tête réécrit). Une session native reprise sur un modèle hors Codex est refusée. Une session faite d'une autre garde son `yaTools` : fork (`fork_from`, forks du démon) et session matérialisée (`materialize_session_file`).
- **Démon** (`pa-daemon`, chemin du mode ACP et de la TUI ; le smoke bench et la capture passent par `-p`, sans démon) : le worker tient le fichier durable (`SessionCore::store`) et le moteur tourne sur une session en mémoire. Chaque build recopie `yaTools` de l'en-tête durable dans la session en mémoire ; après le build, la valeur figée est remise au worker par un crochet (`agent_engine/ya_frozen_tools.rs`, installé comme `set_rlm_usage_store`), qui l'écrit dans l'en-tête quand il n'en a pas et réécrit le fichier. Le verrou du worker est pris sans bloquer le build (sinon l'écriture attend sur un thread) ; l'écriture vérifie que le store est toujours celui de la session construite. Limite : un store fenêtré (reprise d'une grosse session créée avant ce champ) garde la valeur en mémoire seulement, une réécriture demandant tout l'historique ; un build suivant de cette session ancienne relit alors les réglages.
- **`apply_patch`** : tool `custom` (`pa_ai::ya_freeform`), nom, description et grammaire de Codex. Chaque chemin du patch (et la destination d'un `Move to`) passe par `ya_workspace::check_inside` avant toute écriture ; un seul chemin hors du workspace refuse le patch entier. Application par `ya_apply_patch::apply_patch` (liens suivis, mode par défaut du crate ; le mode sans suivi n'est pas utilisé). Résultat identique à Codex au SHA `69231e1`, sans ajout (décision du 13/11, correction 2 de la PR #26 ; `edit()` du kernel garde sa zone numérotée) : un patch qui ne s'analyse pas ou ne s'applique pas (fichier absent, lignes introuvables, deux opérations sur un chemin) échoue à la vérification de Codex avant toute écriture, avec pour seul texte `apply_patch verification failed: <erreur>` (`core/src/tools/handlers/apply_patch.rs:477-519`, `apply-patch/src/invocation.rs:179-251`, reprise dans `verify_hunks` sur `std::fs`) ; sinon `format_exec_output_for_model` (`Exit code`, `Wall time`, `Output:`, puis stdout et stderr du crate, `core/src/tools/mod.rs:92-117`, `core/src/tools/runtimes/apply_patch.rs:202-238`). Ordre : analyse, contrôle de workspace (aucun fichier hors workspace n'est lu), vérification, application. Plafond `kernel.maxOutputChars` comme `python_repl`.
- **`exec_command` / `write_stdin`** (`tools/ya_exec.rs`) : noms, descriptions et paramètres repris mot pour mot de `shell_spec.rs` ; retirés : `tty`, `shell`, `login`, paramètres de sandbox et d'approbation (biais possible contre bash natif, noté au plan de l'A/B 2). Sans PTY : stdin fermé, `write_stdin` sonde (`chars` vide) ou interrompt (`"\u0003"`, SIGINT au groupe), sinon `stdin is closed for this session`. Attente, bornes, limite de 64 sessions et élagage repris de `unified_exec`. Sortie : en-tête de Codex sans `Chunk ID` ; budget de Codex en octets (4 octets par token, `max_output_tokens` et politique de session, marge de 20 % pour l'en-tête), la politique de session étant `kernel.maxOutputChars` lu en octets ; au-delà, le milieu est remplacé par une ligne qui donne le fichier de la sortie complète. Identifiants de session séquentiels à partir de 1000 (Codex : aléatoires en production, séquentiels en mode déterministe).
- **Kernel en variante native** : le bootstrap (`kernel/bootstrap/ya_native_shell.rs`) ne relie pas `bash` et remplace l'attribut `rlm.bash` par une fonction qui lève une erreur ; le paquet installé garde `rlm.bash`, donc `RUNTIME_READY_CHECK` (`venv/probe.rs`), mémorisé par venv et partagé par les deux variantes, ne change pas. Appelants internes vérifiés : `rlm/repl.py` et `rlm/factory.py` importent le sous-module (`from .bash import ...`, par `sys.modules`), jamais l'attribut ; le skill `edit` n'utilise pas `bash`.
- **Prompt** (`prompts/ya_tools_prompt.rs`, sans toucher aux fichiers de couches) : rupture de préfixe pour toutes les sessions (`read()`, `bash(env=)`, ligne de troncature), plus `apply_patch` sur Codex par défaut, plus la variante `exec_command`. En variante native, deux ajouts au texte validé, signalés à Yusuf : la ligne « Use f-strings … in `bash` » d'`opinionated.md` est retirée, et la note des skills embarqués (`session_paths::BUNDLED_SKILLS_NOTE`, queue dynamique) dit « of the Python REPL and of `exec_command` » au lieu de « … and `bash()` » ; `exec_command` reçoit les deux variables de session que `bash()` héritait du kernel (`PRIME_AGENT_CONVERSATION_LOG`, `PRIME_AGENT_BUNDLED_SKILLS_DIR`, `session_paths::insert_kernel_env`).

### Tests upstream touchés (YA-03, PR 2)

| Fichier | Test | Nature | Modification |
|---|---|---|---|
| `crates/pa-core/tests/prompt_guards.rs` | `cached_prefix_is_stable_across_sessions` | défaut | le préfixe attendu passe les couches `core` et `usage` par `ya_tools_prompt` (texte de toute session : `read()`, `bash(env=)`, troncature) |
| `crates/pa-core/src/prompts/system_prompt.rs` | `default_prompt_is_layered_with_static_prefix_first` | défaut | idem |

## Tools libres côté providers (YA-03)

Ticket 09. Forme sur le fil d'openai/codex au SHA `69231e1` : tool `{"type": "custom", "name", "description", "format": {type, syntax, definition}}` (`codex-rs/tools/src/tool_spec.rs:54-55`, `responses_api.rs:17-30`) ; items `custom_tool_call` (`call_id`, `name`, `input` en texte) et `custom_tool_call_output` (`codex-rs/protocol/src/models.rs:1282-1319`) ; événements `response.output_item.added` / `.done` et `response.custom_tool_call_input.delta` (`codex-rs/codex-api/src/sse/responses.rs:411-568`).

- **Types partagés inchangés** (`pa_types::ai::Tool`, `ToolCall`, et `pa_agent::types::ToolCall`) : un tool libre porte son `format` sous la clé `x-yusagent-freeform` de `parameters`, à côté d'un schéma objet `{input: string}` requis, que la validation des arguments (`crates/pa-agent/src/validation.rs`) accepte sans changement. Un appel libre a pour arguments `{input: <texte brut>}` et pour id `<call_id>|<item_id>`. Il n'est pas marqué : le `ToolCall` de la boucle de l'agent (`crates/pa-agent/src/types.rs:119-131`) n'a pas de champ pour des métadonnées de provider, et la marque `yaFreeformItemId` de la PR 1 s'y perdait (chauffe du 14/11 : appel rejoué en `function_call`, suite WebSocket cassée). Un appel est libre quand le tool qu'il nomme est déclaré libre dans les tools de la requête : ceux de la session, figés par session (`yaTools`, reprise et fork compris).
- **Rejeu** : l'id d'item d'origine n'est renvoyé que pour un appel du même modèle et du même provider (comme l'id `fc_` d'un `function_call`, `openai_responses_shared.rs`) ; sinon il est omis. Le résultat d'un appel libre part en `custom_tool_call_output` (même encodage de `output` que `function_call_output`).
- **Flux** : SSE et WebSocket de Codex passent par `ResponsesStreamProcessor::handle_event` (`openai_codex_responses/mod.rs`, `run_websocket_attempt` et `run_sse_stream`). L'entrée arrive par deltas, l'item `done` porte l'entrée complète, qui fait foi. Le bloc vivant est retrouvé par son id (le slot est déjà retiré au `done`, `openai_responses_stream.rs`) ; sans événement `added`, l'appel est créé au `done`.
- **Portée** : seules les API Responses (`openai-responses`, `openai-codex-responses`, `azure-openai-responses`) savent envoyer un tool libre. Le tool `apply_patch` n'est enregistré que pour elles (PR 2) ; aucun autre provider n'en reçoit.


## Plafond de sortie (YA-03)

Ticket 09. Avant : le gestionnaire du kernel tronque chaque flux (stdout, stderr, résultat) à 65 536 caractères en gardant le début (`crates/pa-core/src/kernel/shared.rs:14`, `kernel/manager/execution.rs`), sans fichier.

- **Plafond** : `kernel.maxOutputChars` (défaut 40 000 ≈ 10 000 tokens, estimation à 4 caractères par token : troncature de Codex pour `gpt-6.1-terra`, `codex-rs/models-manager/models.json` au SHA `69231e1`) s'applique au texte complet de la cellule tel que le modèle le reçoit (le premier bloc texte du résultat du tool). `0` le coupe.
- **Au-delà** : début et fin du texte, moitié chacun, autour d'une ligne `[... output truncated: <N> characters in total; full output in <chemin> ...]`. Le total, ligne comprise, ne dépasse pas le plafond. Le fichier est `<dossier d'artefacts de la session>/outputs/<id de l'appel>.txt` (sans dossier d'artefacts : `<tmp>/yusagent-outputs/<id de session>/`). Si l'écriture échoue, la ligne donne l'erreur.
- **Tampon du kernel** : 2 000 000 caractères par flux pour les cellules du modèle, pour que le fichier soit complet jusqu'à cette taille. Le runtime Python ne tronque pas les flux (trames de 64 Kio, `prime-agent-runtime/src/rlm/repl.py:50`) ; il plafonne le résultat à 1 Mio (`repl.py:53`). Les cellules internes du harness gardent le défaut upstream.
- **Préfixe** : inchangé. Seul le contenu d'un résultat qui dépasse le plafond change, en fin de contexte.


## Redémarrage du kernel (YA-03)

Ticket 09, décision du 13/11/2026 (journal du CDC). Upstream : un kernel mort est relancé au `ensure()` de la cellule suivante (`crates/pa-core/src/kernel/provisioner.rs`, manager `is_defunct`), qui relit le namespace depuis le snapshot pris après les cellules réussies (`restore_state`), et l'avis `[python-state-restored]` (« revived from your previous session », `session_engine/state_restore_notice.rs`) n'arrive qu'au message utilisateur suivant (`admission.rs`, `take_next_turn_rows`).

- **Détection** : après une cellule, le manager publié par le provisioner est `defunct`. Sur un échec côté harness (`Err` du tool), attente d'au plus 500 ms que le guetteur de sortie marque le manager (il le marque avant de rejeter la cellule, `kernel/manager/startup.rs`, `teardown.rs`). Un kill choisi par l'utilisateur sur un kernel occupé (`tools/ipython.rs`, `<ipython_kernel_reset>`) remplace le manager : il n'est pas pris pour un crash.
- **Redémarrage** : `ensure()` tout de suite, même restauration qu'upstream. Aucune cellule n'est rejouée.
- **Avis** en fin de résultat de la cellule plantée (après le texte, ou après le message d'erreur) : `[python-kernel-restarted]`, kernel redémarré, état restauré depuis la dernière cellule réussie, répertoire et variables d'environnement remis aux défauts de la session, cette cellule n'a pas été relancée. Sans snapshot à relire : même avis avec « fresh state ». Redémarrage en échec : `[python-kernel-crashed]` avec l'erreur. Pas de liste de noms.
- **Doublon** : la ligne `ipython_state_restored` que ce redémarrage met dans la boîte du tour suivant est retirée ; son drapeau `restored` choisit l'avis.
- **Préfixe** : inchangé. Seul le résultat de la cellule plantée change, en fin de contexte.
- **Test** : `ya_kernel_restart::tests::a_cell_that_kills_the_kernel_gets_the_notice_and_is_not_re_run` (vrai kernel, sauté s'il est absent ; la CI le prépare) : avis en fin de résultat, redémarrage en moins de 5 s, compteur sur disque à 1, `marker` relu depuis le snapshot.

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
| 43583710607 | `fe8fa8b1e` | 1 | échec du shard 5 (même symptôme, `clipboard_auth_commands_e2e.rs:989`) |
| 43583710607 | `fe8fa8b1e` | 2 | réussi, aucun changement de code |

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

## Instabilité probable : `rlm_children_delete_e2e` (YA-03)

`crates/pa-daemon/tests/rlm_children_delete_e2e.rs`, test `a_deleted_running_child_records_the_cancel_edge` (`:313`), shard 6/8 de `ci.yml`. Fichier identique à `upstream/main` (`git diff --stat upstream/main -- crates/pa-daemon/tests/rlm_children_delete_e2e.rs` vide au 13/11/2026, `upstream/main` à `ad1613303`).

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 43499374052 | `bd9cccd18` (`ya-03-outils`) | 1 | échec du shard 6/8 |
| 43501970149 | `25c56570c` (`ya-03-outils`) | 1 | réussi, aucun changement du daemon ni du test |
| 43504367477 | `58f2542e4` (`ya-03-outils`) | 1 | réussi |
| 44080414273 | `ac8315ba7` (`ya-14-session-id`, PR #50) | 1 (job 134029453827) | échec du shard 6/8, même symptôme (`rlm_children_delete_e2e.rs:195`) |
| 44080414273 | `ac8315ba7` (`ya-14-session-id`, PR #50) | 2 (job 134044803529, relance du job seul, sans push, autorisée par Yusuf le 17/11) | réussi, aucun changement de code ; relance notée dans un commentaire de la PR #50 |

Symptôme : `kernel cell receipt never appeared at /tmp/.../receipts/delete.json` à `rlm_children_delete_e2e.rs:195` : `await_receipt` (`:186-199`) attend une minute le reçu écrit par une cellule du kernel de l'enfant. Hypothèse : démarrage du kernel de l'enfant trop lent sur un runner chargé (le même shard échouait aussi sur `prefix_stability`). Les commits de la PR 2 de YA-03 ne touchent ni le daemon, ni les sous-agents, ni ce test.

Si ce test retombe : relancer le shard une fois, et noter le run ici.

## Instabilité probable : `login_panel_pty_e2e` (YA-09)

`crates/pa-cli/tests/login_panel_pty_e2e.rs`, test `prime_login_renders_the_team_picker_without_a_terminal_takeover` (`:633`), shard 1/8 de `ci.yml`, sélectionné quand une PR touche `pa-cli`. Fichier identique à `upstream/main` (`git diff --stat upstream/main -- crates/pa-cli/tests/login_panel_pty_e2e.rs` vide au 15/11/2026).

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 43751270454 | `49f6bc27c` (`main`, push) | 1 | réussi (1,6 s) |
| 43756666927 | `7f3f8eafd` (`ya-09-bash`, PR #41) | 1 | échec du shard 1/8 |
| 43756666927 | `7f3f8eafd` (`ya-09-bash`, PR #41) | 2 (relance du job seul, sans push, autorisée par Yusuf) | réussi (1,6 s) |

Symptôme : `Timeout waiting for the provider selector panel (needle "Search providers")` à `login_panel_pty_e2e.rs:633`, après 30 s : le panneau des providers n'apparaît pas dans le PTY. La PR #41 ne touche dans `pa-cli` que `prompt_command.rs` (commande `prime-agent prompt`), hors du TUI de login. Hypothèse : rendu du PTY trop lent sur un runner chargé ; non vérifiée.

Si ce test retombe : relancer le shard une fois, et noter le run ici.

## Instabilité probable : `herdr::tests::the_seq_never_restarts_below_a_used_value` (YA-09)

`crates/pa-daemon/src/herdr.rs`, test unitaire `the_seq_never_restarts_below_a_used_value` (`:1094`), shard 3/8 de `ci.yml` (unité `pa-daemon#lib:pa_daemon`). Test identique à `upstream/main` : `git diff upstream/main -- crates/pa-daemon/src/herdr.rs` (`upstream/main` à `b14426151` le 18/11/2026) ne montre que `temp_socket` (`:1189-1190`), hors de ce test.

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 44288407647 | `920593a8b` (`ya-15-erreurs-visibles`, PR #52) | 1 (job 134747100221) | échec du shard 3/8 (1191 réussis, 1 échec) |
| 44288407647 | `920593a8b` (`ya-15-erreurs-visibles`, PR #52) | 2 (job 134755788965, relance du job seul, sans push, autorisée par Yusuf le 18/11/2026) | réussi, aucun changement de code ; relance notée dans un commentaire de la PR #52 |

Symptôme : `panicked at crates/pa-daemon/src/herdr.rs:1101:9: seeded at now_ms*1000`. Le commit `920593a8b` ne change qu'un fichier de `docs/` ; le même code avait passé ce shard sur `4fd1130c6` et `3e7294faf`. Hypothèse : course de temps sur la milliseconde. `next_report_seq()` prend son plancher `now_ms_x1000()` au moment de l'appel (`herdr.rs:180-194`) ; le test relit `now_ms_x1000()` après les trois appels et exige `first >= now_ms_x1000() - 1` (`:1101`). Si l'horloge passe à la milliseconde suivante entre les deux lectures et qu'aucun autre test du processus n'a avancé le compteur global, l'écart vaut 1 000 et l'assertion échoue. Non reproduit.

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

## Instabilité probable : `rpc_mode_e2e` (YA-16)

`crates/pa-cli/tests/rpc_mode_e2e.rs`, test `rpc_compact_flush_is_bounded_against_a_stalled_reader` (`:1368`), shard 8 de `ci.yml`.

| Run | Commit | Tentative | Résultat |
|---|---|---|---|
| 44593768562 | `897b0f758` | 1 | test réussi (le run a échoué sur d'autres tests) |
| 44601054912 | `8efe27bdf` | 1 | échec du shard 8 (job 135832457352) |
| 44604335224 | `74137ca47` | 1 | réussi, aucun changement de code (le push de la passation compte comme la relance, accord de Yusuf, commenté dans la PR #63) |

Symptôme : « the compaction never ran behind the stalled reader: the flush wedged the command on the reader » à `rpc_mode_e2e.rs:1399`. Le test attend au plus 4 s la ligne `"compaction"` dans le fichier de session, lecteur de sortie bloqué (`:1385-1397`). Hypothèse : borne de temps trop courte sur un runner chargé. Entre `897b0f758` et `8efe27bdf`, aucun code du produit ne change. Dans ce fichier, la PR #63 ne change que le chemin du venv de test (`:61-62`).

Si ce test retombe : relancer le shard une fois, et noter le run ici.

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

## Correctifs upstream repris (YA-04)

Défaut de notre base de fork (`c83ac68b3`) : au `response.output_item.done`, `ResponsesStreamProcessor` retirait le slot de l'item avant de le relire, donc ni la signature de texte ni l'item de raisonnement n'étaient enregistrés quand l'événement porte `output_index` (cas réel). Un texte se rejouait en `msg_<index>`, la base de la suite WebSocket ne correspondait plus et le tour suivant repartait en corps complet. Constaté dans la chauffe du 14/11 (requête 2), détail au ticket 03 (points ouverts).

- `c12acadaa` (#3976, « Preserve Responses reasoning when finalizing streamed items ») : `content_index` lu avant `self.slots.remove(&index)` (`openai_responses_stream.rs:539-549`). Ajoute le test upstream `completed_reasoning_is_preserved_for_replay`.
- `42a07c13c` (#4014) : les arguments accumulés sont lus en même temps que `content_index` et repris quand le `done` arrive avec des arguments vides. Ajoute le test upstream `function_call_done_keeps_accumulated_arguments`.
- Les deux commits s'appliquent sans conflit sur `main` (`b661396d5`) : nos modifications du fichier (YA-03 : `custom_tool_call`, YA-13 : usage) sont dans d'autres blocs. Entre `c83ac68b3` et `42a07c13c`, aucun autre commit upstream ne touche ce fichier. Notre branche `custom_tool_call` du `done` (`:755-792`) retrouve son bloc par l'id, sans passer par le slot, et ne change pas.
- Effet sur les requêtes (2e tour et suivants) : les items de raisonnement sont rejoués tels que reçus (`encrypted_content` compris), avant le texte et les appels du même tour. Un texte est rejoué sous son id d'item (`msg_...`) avec sa `phase`. La première requête d'une session ne change pas.

## Tests upstream touchés (YA-04)

| Fichier | Test(s) | Nature | Modification |
|---|---|---|---|
| `crates/pa-ai/src/providers/openai_codex_responses/mod.rs` | helper `codex_response_events` (`recovers_stale_chain_after_metadata_events`, `retry_failure_surfaces_no_stale_response_id`, `reused_connection_follow_up_ignores_the_first_requests_cancelled_signal`, et le 2e tour de `ya_freeform.rs`) | renfort | le `done` du message porte `output_index: 0` comme le vrai backend ; commentaire de contournement retiré. Aucune assertion modifiée |

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

## Réglages de requête (YA-09)

- **Réglage** : `reasoningSummary` dans `settings.json`, `"off"` (défaut depuis la décision du journal du CDC du 15/11/2026) ou `"auto"` (identique à upstream). Upstream n'a pas de réglage équivalent : `reasoning.summary` vaut `"auto"` en dur (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs:95-99`, `openai_responses.rs:254-257`). **Le défaut `"off"` s'applique aux trois API Responses** (`openai-codex-responses`, `openai-responses`, `azure-openai-responses`), **mais seul `gpt-6-luna` est mesuré** : A/B du 15/11/2026, `bench/smoke/ab/ya09-summary-20261115.md` (latence fixe par appel avec raisonnement −2,92 s [−3,81 ; −2,24], phase agent −26,1 %, résolution et coût non distinguables). Les autres modèles (`gpt-6.1-terra`, `gpt-6.1-sol`, modèles OpenAI et Azure) prennent ce défaut sans mesure.
- **Couche providers non touchée** : en `"off"`, le champ est retiré par un hook `on_payload` posé autour du `stream_fn` de la session (`ya_request::scoped_stream_fn`). Le provider Codex applique ce hook au corps avant de choisir WebSocket ou SSE (`openai_codex_responses/mod.rs:198-203`). Le hook passe avant ceux de `request_timing` et du journal d'usage, qui voient le corps envoyé.
- **Figé par session** : champ `yaRequest` de l'en-tête (`{"reasoningSummary": "auto" | "off"}`), séparé de `yaTools` ; il accueillera d'autres réglages de requête. Hérité au fork (`INHERITED_HEADER_KEYS`). **Le démon copie désormais tous les champs hérités** (`ya_sync_frozen_tools`), et plus seulement `yaTools`. Un en-tête qui a `yaTools` sans `yaRequest` vient d'une session créée avant ce champ (binaire antérieur au 15/11/2026) : `"auto"`, ce qu'elle envoyait, **même avec le défaut `"off"`**. **Un `yaRequest` illisible** (valeur hors `"auto"`/`"off"`, objet sans `reasoningSummary`, ou qui n'est pas un objet) **est réécrit en `"auto"`** dans l'en-tête de la session. Côté démon, le fichier durable n'est écrit que si le champ y manque : il garde la valeur illisible, que chaque build relit en `"auto"`. **Écart accepté** (décision de Yusuf du 15/11/2026) : une session d'avant YA-03 (aucun des deux champs) prend les réglages courants, comme pour ses tools, donc `"off"` par défaut, alors qu'elle a été envoyée en `"auto"`. Au gel, rien ne la distingue d'une session neuve : le démon gèle sur une copie en mémoire sans historique (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1076-1086`).
- **Préfixe** : prompt système et tools inchangés. **Par défaut (`"off"`), le corps change par rapport à upstream** : `reasoning` vaut `{"effort": …}` sans `summary` à chaque requête de la session. En `"auto"`, le corps est celui d'upstream octet pour octet (le `stream_fn` n'est pas enveloppé).
- **Effets produit en `"off"`, donc par défaut** : les blocs de raisonnement gardent leur signature (rejeu intact) avec un texte vide ; rien n'est affiché comme raisonnement dans la TUI ni par ACP (`agent_thought_chunk`). **Écart avec YA-10** (« raisonnement visible ») inscrit au journal du CDC (15/11/2026) : seul `"auto"` le rend ; un interrupteur dans l'extension viendra avec YA-10. Piste notée, non décidée : un défaut par modèle lu dans `default_reasoning_summary` du catalogue de Codex.

## Fenêtre de `bash()` (YA-09)

- **Réglage** : `bashWaitSeconds` dans `settings.json`, secondes entières, au moins 1, ou `"full"` (ticket 13 : pas de fenêtre, `await bash(cmd)` rend la main quand la commande se termine ou que son `timeout` la tue, comme Unharnessed). **Absent : `"full"` en prompt v2** (D2 du 16/11/2026, sans A/B) **et 30 s en prompt v1** (D6 : v1 n'a pas de timeout par défaut ; sessions reprises d'avant le lot, sous-agents). **Un `"full"` explicite en prompt v1 est refusé au démarrage** (D3), avec un message qui indique `"bashWaitSeconds": 30`. Avant le ticket 13 : absent valait 30 s (décision de Yusuf du 15/11/2026, choix d'Unharnessed repris sans A/B, journal du CDC) ; upstream attend 1 s (`prime-agent-runtime/src/rlm/bash.py`, `_YIELD_AFTER_SECONDS`, upstream #3523 et #3828) et n'a pas de réglage équivalent ; `1` remet le comportement d'upstream. Plan d'A/B préparé puis non exécuté : `bench/smoke/ab/ya09-bash-plan.md`.
- **Runtime Python non modifié** : le bootstrap du kernel pose la globale du module `rlm.bash` après l'appel `_set_completion_notices_wired(True)` (`kernel/bootstrap/ya_bash_wait.rs`) ; en `"full"`, elle vaut `None`, et l'attente prend le chemin sans borne d'upstream (`_bounded_wait`). Une commande attendue jusqu'au bout par sa cellule ne produit pas d'avis `[bash-done]`. Un changement upstream de cette ligne de bootstrap fait échouer le test `the_marker_matches_the_upstream_bootstrap`. Si upstream renomme la globale, la ligne posée n'a plus d'effet : le test `ya_bash_wait_kernel.rs` le détecte (résultat attendu à 30 s).
- **Figé par session** dans `yaRequest`, à côté de `reasoningSummary`, écrit sauf s'il vaut 1 (30 par défaut). **Un `yaRequest` sans le champ vaut 1 s** : toute session créée avant le réglage garde la ligne à 1 s et son préfixe à la reprise. Un en-tête avec `yaTools` sans `yaRequest` (avant le 15/11) vaut `"auto"` et 1 s. **Écart accepté** (comme pour `reasoningSummary`) : une session d'avant YA-03 (aucun des deux champs) prend les réglages courants, donc 30 s, et son prompt change à la reprise. Une valeur illisible dans l'en-tête rend tout `yaRequest` illisible, réécrit en `"auto"` et 1 s (règle existante).
- **`prime-agent prompt`** affiche la fenêtre des réglages courants (pas la valeur figée d'une session).
- **Préfixe** : **par défaut (`"full"` en v2), le prompt système n'a pas de ligne `await bash(cmd)`** (`prompts/ya_bash_wait.rs`, comme Unharnessed), stable dans la session ; le préfixe statique v2 donne `await handle -> BashResult` et la règle contre les longues attentes. Avec un nombre, seule la fenêtre change (« waits at most 30 seconds. »). À 1 s (réglage ou session ancienne), ligne d'upstream octet pour octet. Tools et corps de requête inchangés. Le bootstrap du kernel gagne une ligne par défaut.
- **Watchdog de cellule** (démon seulement) : en `"full"`, sa note ne dit plus que `bash()` passe en arrière-plan au-delà de sa fenêtre, mais de lancer la commande longue sans `await` et de la relever sur l'avis `[bash-done]` (D5). Réécrite après coup par le fork (`runtime_wiring.rs`), `execution.rs` intact ; un test épingle la phrase upstream.

## Timeout et avis groupés de `bash()` (YA-09)

Lot Unharnessed, PR A (`docs/tickets/11-ya09-unharnessed.md`). Repris sans A/B (décision de Yusuf du 15/11/2026, journal du CDC).

- **`bash(cmd, timeout=...)`** (`prime-agent-runtime/src/rlm/ya_timeout.py`) : une commande encore en cours après `timeout` secondes reçoit SIGTERM sur son groupe, puis SIGKILL 5 s plus tard (`BashHandle.kill`) ; la ligne `[yusagent: killed, still running after its N s timeout]` s'ajoute à sa sortie et `timeoutSeconds` à son avis `bash.completed`, dont le texte finit alors par `Killed: still running after its N s timeout.` (`ya_bash_timeout.rs`). `None` : pas de limite. Sans argument : `ya_timeout.DEFAULT_TIMEOUT`, **`None` dans cette PR** (comportement inchangé) ; le prompt v2 (PR B) le posera à 600 s par le bootstrap pour ses sessions seulement. Un shell persistant n'a pas de limite par défaut ; une limite explicite s'applique, mais son minuteur ne survit pas à un redémarrage du kernel. Le minuteur s'annule à la fin de la commande (Unharnessed l'annule au réapage du groupe). La variante `shellTool: "native"` (`exec_command`) n'a pas de limite de durée (écart noté).
- **Avis groupés** (`ya_bash_notice_batch.rs`, `bashNoticeBatchSeconds`, défaut 120, `0` = upstream) : un avis `[bash-done]` qui arrive pendant une exécution suit le chemin upstream (file de pilotage, appel modèle suivant). Un avis qui arrive quand l'agent a fini son tour est retenu ; les avis retenus partent ensemble, avec un seul réveil, à la fin de la dernière commande (callback de fin du kernel), tout de suite si aucune commande ne tourne (shell persistant), ou quand le plus ancien a attendu la fenêtre. **Écart à Unharnessed** (décision de Yusuf du 16/11/2026) : chez eux, un avis ne rejoint jamais une exécution en cours. **Hôte sans démon seulement** : le démon garde le comportement upstream (voir le ticket 11).
- **Préfixe** : prompt système, tools et paramètres de requête inchangés. La ligne du prompt « background `bash()` completions wake you with a queued `[bash-done]` turn » reste vraie. Seule la conversation change : moment et regroupement des lignes `[bash-done]`, texte `Killed: …` après un dépassement.

## Prompt version 2 (YA-09, ticket 11)

Lot Unharnessed, PR B (`docs/tickets/11-ya09-unharnessed.md`). Repris sans A/B (décision de Yusuf du 15/11/2026).

- **Réglage** `promptVersion` (`2` absent, `1`), figé dans `yaRequest` (`promptVersion`, écrit sauf s'il vaut 1). **Un `yaRequest` sans le champ vaut 1** : toute session créée avant le lot garde son prompt octet pour octet à la reprise (option A, décision de Yusuf du 16/11/2026). Un en-tête avec `yaTools` sans `yaRequest` (avant le 15/11) vaut `LEGACY`, donc version 1. Écart déjà accepté : une session d'avant YA-03 (aucun des deux champs) prend les réglages courants, donc la version 2.
- **Version 2** (`prompts/ya_prompt_v2.rs`) : préfixe statique court à la place des couches `core`, `usage`, `opinionated` d'upstream (non modifiées : la version 1 les garde) ; queue dynamique d'upstream (paquets, contexte du projet, environnement, MCP) avec un inventaire compact des skills, la ligne « no daemon » sans « queued turn », et sans `Recursive agent depth` pour une racine. Description courte de `python_repl`. Limite `bash()` par défaut de 600 s (bootstrap) ; la version 1 n'en a pas, comme son prompt le dit.
- **Taille** (comptes `o200k_base` supposés, estimations : `docs/tickets/11-ya09-unharnessed.md`, §4) : environ 1 300 tokens d'instructions par défaut (6 230 sur `c770c5a`), tools 171 → environ 25 pour `python_repl` ; à confirmer par l'attribution du backend Codex sur les chauffes.
- **Sous-agents** : la version 2 ne les documente pas (`rlm.spawn`). Une session version 2 qui peut en avoir (enfant, ou racine à `rlmMaxDepth` ≥ 1) est **refusée au démarrage**, avec un message qui indique `"promptVersion": 1` (demande de Yusuf du 16/11/2026). Le refus tombe avant l'écriture de l'en-tête : la session refusée ne garde pas de version figée.
- **Tests** : `.cargo/config.toml` pose `PRIME_AGENT_PROMPT_VERSION = "1"` (lu sous le réglage), comme `RLM_MAX_DEPTH = "2"` : les tests upstream tournent avec le prompt upstream et le spawn, sans retouche ; les tests du fork qui vérifient la version 2 retirent les deux variables.
- **Préfixe** : **le prompt système et les tools changent pour toute nouvelle session** (une rupture de cache par session neuve, déjà le cas d'une session neuve) ; stables dans la session ; sessions anciennes inchangées.
- **Ticket 13 (17/11/2026)** : le texte v2 n'est pas figé par valeur, seule la version l'est. L'alignement sur Unharnessed (`await handle -> BashResult`, « long blocking awaits ») change donc le préfixe de **toute session v2 existante, une fois à sa reprise**, puis il reste stable. Rupture acceptée par Yusuf, signalée dans le journal du CDC.

## Keepalive du WebSocket Codex (YA-09, ticket 12)

**Bug upstream, candidat à une PR upstream après les testeurs** (décision de Yusuf du 16/11/2026). Dans `connection_worker` (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs`, identique à `c83ac68b3` et à `upstream/main` `ad1613303`), le worker attend la commande suivante sans lire le socket. Les pings du serveur restent sans pong pendant un silence entre deux requêtes, et le backend Codex ferme en `1011 keepalive ping timeout` après 74 à 79 s environ (3 cas au banc, `docs/tickets/12-ya09-ws-keepalive.md`). La requête suivante lit cette fermeture avant tout événement, repasse en SSE et fixe la session en SSE jusqu'à sa fin (corps complets).

- Correctif du fork, dans `ya_idle.rs`, avec un point d'appel dans `websocket.rs` : lecture du socket pendant le silence (pongs), fin du worker sur fermeture, connexion morte remplacée par un WebSocket neuf à la requête suivante, messages de données reçus hors requête comptés (diagnostic `websocket_idle_messages_ignored`) au lieu d'être livrés à la requête suivante.
- Pour une PR upstream : le changement tient dans `connection_worker` et `acquire_websocket`. Le diagnostic est propre au fork (demande de Yusuf) et peut rester hors de la PR.
- Requêtes et préfixe inchangés : seules des trames de contrôle s'ajoutent.
- `c4800c8eb` (#4010) est repris tel quel (cherry-pick `-x`) : même zone, autre défaut (connexion occupée pour toujours après un abandon juste avant l'envoi).
- **TTL client à 15 min au lieu de 5** (ticket 13, D4, `SESSION_WEBSOCKET_CACHE_TTL_MS`) : avec l'attente complète de `bash()`, une cellule peut durer jusqu'à environ 605 s (timeout de 600 s du prompt v2, puis SIGKILL 5 s plus tard). Avec 5 min, la connexion se fermait côté client pendant la cellule et l'appel suivant partait en corps complet sur un WebSocket neuf. Écart de comportement avec upstream, sans effet sur les requêtes.

## En-tête `session-id` (YA-09, ticket 14)

Option A du ticket 14 (décision de Yusuf du 17/11/2026, 12h05).

- **Réglage** : `codexSessionIdHeader` dans `settings.json`, booléen, **coupé par défaut** tant que l'A/B n'a pas tranché (`bench/smoke/ab/ya09-session-id-plan.md`).
- **Effet** : à `true`, chaque requête du provider `openai-codex` (API `openai-codex-responses`, « OAuth abonnement pour provider ») porte l'en-tête `session-id` (tiret), égal à l'id de session, donc à `prompt_cache_key`. Il part sur la poignée de main WebSocket et sur la requête SSE, en plus de `session_id` (tiret bas) et de `x-client-request-id` d'upstream.
- **Source** : Codex CLI `rust-v0.169.0` (`d32063f96c2d`), `codex-rs/codex-api/src/requests/headers.rs:5-14` et `codex-rs/core/src/client.rs:734-742` (« ChatGPT derives cache affinity from the Responses session-id header »). Inchangé sur `main` de Codex à `59ac39add` (17/11/2026). Le ticket et le plan citent les extraits mot à mot.
- **Aucun autre provider** : ni l'API `OpenAI` par clé, ni Azure, ni Anthropic, ni un autre provider sur une API compatible. Le test porte sur le provider et l'API (`is_chatgpt`).
- **Couche providers non touchée** : l'en-tête passe par `StreamRequestOptions::headers`. `provider_adapter::stream_once` les fusionne sur les en-têtes du provider, et `openai_codex_responses` les envoie déjà sur les deux transports (`request.rs`, `build_sse_headers` et `build_websocket_headers`, paramètre `additional_headers`).
- **Figé par session** dans `yaRequest`, toujours écrit (`true` ou `false`), pour qu'un run dise quel bras il a joué. **Toute nouvelle session a donc `"codexSessionIdHeader": false` dans son en-tête par défaut** ; les assertions exactes sur `yaRequest` des tests du fork (`ya_request.rs`, `prefix_stability.rs`, `ya_prompt_v2_e2e.rs`, `pa-daemon/src/agent_engine/ya_frozen_tools.rs`) l'incluent. Un `yaRequest` sans le champ (session d'avant) vaut « coupé », ce qu'il a envoyé, et n'est pas réécrit. Une reprise garde la valeur figée, avec l'avertissement habituel si les réglages diffèrent.
- **Préfixe et corps inchangés** : prompt système, tools et corps des requêtes identiques avec ou sans le réglage. Vérifié par `codex_websocket_session_id_header_follows_the_setting` (`crates/pa-core/tests/prefix_stability.rs`) : trames identiques octet pour octet hors `prompt_cache_key`, suite de conversation conservée. Le seul changement est un en-tête HTTP de plus.
- **Non repris** : `thread-id`, que Codex envoie aussi (hors de l'option A).

## Drapeau `isError` et messages des helpers (YA-09, ticket 15)

Détail, inventaire et effet sur les requêtes : `docs/tickets/15-ya09-erreurs-visibles.md`.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-agent/src/types.rs` | modif. (fichier upstream, 6 lignes) | `AgentToolResult.is_error` (`#[serde(skip)]`), `false` dans `text()`, `true` dans `error()`. **Candidat à une PR upstream** (commit séparé). Forme reprise d'Unharnessed (`43f131ac6`). | YA-09 |
| `crates/pa-agent/src/agent_loop/tool_call.rs` | modif. (fichier upstream, 1 ligne) | Un outil qui rend un résultat en échec donne `isError: true` (au lieu de `false` en dur). **Candidat à une PR upstream**. | YA-09 |
| `crates/pa-core/src/session_engine/tool_bridge.rs` | modif. (fichier upstream, 3 lignes) | Le pont transmet `ToolExecutionResult.is_error` au lieu de le jeter (`let _ = is_error;`). **Candidat à une PR upstream**. | YA-09 |
| `crates/pa-core/src/session_engine/telemetry/tests.rs` | modif. (test upstream, 1 ligne) | Littéral `AgentToolResult` : champ `is_error` ajouté, aucune assertion changée. | YA-09 |
| `prime-agent-runtime/src/rlm/ya_hints.py` | ajout | Messages des mauvais usages de `BashHandle`/`BashResult` : `h.exit_code` avant `await`, attribut du handle lu sur un résultat, `h.output` utilisé sans appel. Usages valides et autres `AttributeError` inchangés. | YA-09 |
| `prime-agent-runtime/src/rlm/bash.py` | modif. (fichier upstream, 2 lignes + commentaire) | Import de `ya_hints` et `ya_hints.install(BashHandle, BashResult)` en fin de module. | YA-09 |
| `prime-agent-runtime/src/rlm/read.py` | modif. (fichier du fork) | `read()` rend un `ReadText` (`str` qui refuse `await` avec un message, sérialisé en `str` simple). | YA-09 |
| `skills/edit/src/edit/__init__.py` | modif. (fichier upstream déjà modifié au ticket 11) | `run` vérifie ses arguments et nomme la forme d'appel (`edit(path, old_str, new_str): ...`) ; corps renommé `_edit`, signature publique inchangée (`functools.wraps`). | YA-09 |
| `prime-agent-runtime/test/test_ya_hints.py` | ajout | Tests du runtime des messages de B. | YA-09 |
| `crates/pa-core/tests/ya_tool_error_wire.rs` | ajout | Session réelle, vrai kernel, serveur local : `is_error` des `tool_result` Anthropic, `function_call_output` Responses sans champ d'erreur, messages de B dans les sorties, prompt/tools/paramètres stables. Machinerie copiée de `prefix_stability.rs` (non modifié). | YA-09 |
| `bench/smoke/tools/helper_misuse.py`, `bench/smoke/README.md` | ajout, modif. (fichiers du fork) | Inventaire reproductible des mauvais usages des helpers par config de session ; note sur `isError` du README mise à jour. | YA-09 |
| `.changes/ya-09-errors-visible.md` | ajout | Fragment de changelog. | YA-09 |
| `bench/smoke/tools/check_ya15.py` | ajout | Contrôle de la chauffe du ticket 15 : premier corps identique à une référence hors `prompt_cache_key`, `isError` des cellules `python_repl` cohérent avec `details.status`, aucune mention de 1011. Lecture seule. | YA-09 |

## Renommage du CLI, PR 1 : interface (YA-16, ticket 19)

Décisions et inventaire : `docs/tickets/19-ya16-cli-yusagent.md`. Ni prompt, ni tools, ni requêtes modèle touchés. Unharnessed a retiré télémétrie et traces dans `de437b6e2` (voir plus bas) ; ici, elles sont coupées.

**Partage de traces coupé.** Fichier du fork : `crates/pa-core/src/agent_traces/ya_sharing_off.rs` (constantes `TRACE_SHARING = false`, `TRACE_SHARING_OFF`, et deux tests : `agentTraces.enabled: true` + identifiant + serveur local, envoi manuel et automatique refusés sans connexion ni nouvelle tentative, une ligne de journal par envoi ; mode continu avec consentement, trois tours, aucune connexion, aucune ligne de journal).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-core/src/agent_traces/ya_sharing_off.rs` | ajout | Voir ci-dessus. | YA-16 |
| `crates/pa-core/src/agent_traces.rs` | modif. (fichier upstream, 3 lignes + commentaire) | Déclare `ya_sharing_off`, réexporte les deux constantes. | YA-16 |
| `crates/pa-core/src/agent_traces/http.rs` | modif. (fichier upstream, 6 lignes + commentaires) | **Point de coupure** : `ReqwestTraceHttp::put`, seul chemin par lequel une trace quitte la machine, refuse sans ouvrir de connexion (`TraceHttpError::SharingOff`, non rejouable : `is_retriable_transport_error` ne retient que `TimedOut` et `Transport`). | YA-16 |
| `crates/pa-core/src/agent_traces/continuous.rs` | modif. (fichier upstream, 3 lignes + commentaire) | `run_service` ne démarre ni la livraison ni la reprise : sans cela, un `agentTraces.enabled: true` relancerait l'envoi toutes les 60 s et écrirait une ligne de journal à chaque tour (`Schedule::settle` rejoue un `Failed` sans statut). | YA-16 |
| `crates/pa-cli/src/client_traces.rs` | modif. (fichier upstream, 7 lignes) | `/traces` : l'état lu est faux quel que soit le réglage, `on` est refusé avec `TRACE_SHARING_OFF`. | YA-16 |
| `crates/pa-cli/src/interactive_mode/onboarding.rs` | modif. (fichier upstream, 2 lignes) | La question de partage de traces n'est plus posée. | YA-16 |

**Télémétrie distante coupée.** Comme les traces, sans réglage pour la rallumer. Unharnessed a retiré la télémétrie et l'envoi de traces dans `de437b6e2d3e945d4f56dbb675f5da692c5726ab` (01/10/2026, « unharnessed step 6: drop telemetry and trace upload »). Ici, coupure au lieu de suppression, pour garder les cherry-picks faciles.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-telemetry/src/sinks/analytics.rs` | modif. (fichier upstream, 7 lignes + commentaires) | **Point de coupure** : constante `REMOTE_TELEMETRY = false` ; `AnalyticsSink::send_batch`, seul envoi vers `api.primeintellect.ai/api/v1/agent-analytics/events`, rend `SinkOutcome::Dropped` sans ouvrir de connexion. `telemetry.enabled` ne la lit pas. Le miroir local (`FileSink`, `telemetry.jsonl`) suit toujours le réglage, inchangé. Le journal d'usage YA-13 (`<id>.usage.ndjson`) ne passe pas par les sinks : écriture directe (`crates/pa-core/src/session_engine/usage_log.rs:539-547`). | YA-16 |
| `crates/pa-core/tests/ya_remote_telemetry_off.rs` | ajout | `telemetry.enabled: true`, sink pointé sur un serveur local : le sink seul et le client à lots (gaté sur le réglage) n'ouvrent aucune connexion. Le client lit le réglage directement (`DO_NOT_TRACK=1` de `.cargo/config.toml` couperait sinon l'interrupteur à lui seul). | YA-16 |

**Marque et logo.** Remplacements de texte, une ligne chacun sauf mention :

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-tui/src/chrome.rs` | modif. (fichier upstream) | Splash : titre `yusAgent` ; papillon remplacé par des espaces de même largeur, ligne par ligne (géométrie du splash inchangée). | YA-16 |
| `crates/pa-tui/src/onboarding.rs` | modif. (fichier upstream) | Écran d'accueil : « Welcome to **yusAgent** » ; les cellules du papillon restent posées (même ton, même priorité) mais vides, donc mêmes spans. Question d'équipe : « Which account should yusAgent use? ». | YA-16 |
| `install-rust.sh` | modif. (script upstream, 1 ligne + commentaire) | Bannière de l'installateur (port awk du même écran, `BANNER_AWK_BEGIN`) : cellules du papillon vides, comme dans `onboarding.rs`. Le test `onboarding::tests::the_installer_banner_matches_the_splash` impose l'égalité des deux rendus (échec du shard 5/8, run 44541464183, avant ce changement). Les autres textes de l'installateur relèvent de la PR 2 (distribution). | YA-16 |
| `crates/pa-daemon/src/acp/types.rs` | modif. (fichier upstream) | `agentInfo` : `yusagent` / `yusAgent`. Espace de noms `_meta` inchangé. Fixtures `crates/pa-daemon/testdata/acp/` non modifiées : aucun test ne les lit (enregistrements historiques). | YA-16 |
| `crates/pa-cli/src/daemon_client.rs`, `crates/pa-cli/src/interactive_mode/daemon.rs`, `crates/pa-tui/src/daemon_client.rs`, `crates/pa-tui/src/daemon_client/errors.rs`, `crates/pa-tui/src/session_ui.rs`, `crates/pa-tui/src/session_ui/{apply,factory,heartbeats,prompt}.rs`, `crates/pa-tui/src/interactive/{reconnect,run}.rs`, `crates/pa-tui/src/update_restart_wait.rs` | modif. (fichiers upstream) | « Prime Agent daemon » → « yusAgent daemon » dans les messages **et** dans les fonctions qui les reconnaissent (début de texte, ou texte en minuscules : « yusagent daemon »). Motifs d'incident (`crates/pa-types/src/incident/patterns.rs`) non modifiés : ils lisent d'anciens journaux, aucun code Rust actuel n'émet ces lignes. | YA-16 |
| `crates/pa-daemon/src/journal.rs`, `crates/pa-tui/src/exit_guard.rs`, `crates/pa-tui/src/exit_restore.rs`, `crates/pa-types/src/platform/terminal.rs` | modif. (fichiers upstream) | Préfixe « yusAgent: » des messages du terminal et du journal. | YA-16 |
| `crates/pa-cli/src/command_registry.rs`, `crates/pa-cli/src/telemetry_notice.rs`, `crates/pa-tui/src/session_ui/notes.rs` | modif. (fichiers upstream) | Texte de la télémétrie. | YA-16 |
| `crates/pa-types/src/slash_commands.rs` | modif. (fichier upstream) | Description de `/quit`. | YA-16 |
| `crates/pa-cli/src/incident/report.rs` | modif. (fichier upstream) | En-tête du rapport d'incident. | YA-16 |
| `crates/pa-daemon/src/hold_refusal.rs` | modif. (fichier upstream) | « another build of yusAgent » (le cas TypeScript nomme toujours Prime Agent : c'est bien ce produit). | YA-16 |
| `crates/pa-core/src/kernel/bootstrap/{mod,dir_lock}.rs`, `crates/pa-core/src/kernel/bootstrap/venv/runtime_source.rs` | modif. (fichiers upstream) | Messages d'installation du kernel. | YA-16 |
| `crates/pa-core/src/mcp/oauth_callback.rs`, `crates/pa-core/src/mcp/oauth_discovery.rs` | modif. (fichiers upstream) | Pages de fin de login MCP ; `client_name` de l'enregistrement dynamique auprès des serveurs MCP. | YA-16 |
| `crates/pa-ai/src/oauth/callback.rs` | modif. (couche providers, 2 lignes) | Pages de fin de login OAuth générique. `anthropic_callback.rs` non modifié (règle 7). | YA-16 |
| `prime-agent-runtime/src/rlm/mcp_base.py`, `skills/websearch/src/websearch/websearch.py`, `skills/websearch/SKILL.md` | modif. (fichiers upstream) | Consignes « in yusAgent » (sortie d'outil et corps du `SKILL.md`, hors prompt : l'inventaire ne reprend que la description, inchangée). Le changement du runtime reconstruit le venv du kernel une fois. | YA-16 |
| `scripts/check_factory_navigation.py` | modif. (script upstream, 1 ligne) | Attend le nouveau message de délai du démon. | YA-16 |
| `.changes/ya-16-interface-yusagent.md` | ajout | Fragment de changelog. | YA-16 |

Laissés tels quels : messages de mise à jour (PR 2, `update` désactivée), nom du service « Prime Agent Traces » de Prime Intellect, login Claude (règle 7), prompts de `/refine` et de reprise après mise à jour (texte envoyé au modèle), docstrings Python, `README.md` (hors ticket).

### Tests upstream touchés (YA-16, ticket 19, PR 1)

| Fichier | Test | Nature | Changement |
|---|---|---|---|
| `crates/pa-telemetry/tests/analytics_wire.rs` | `batch_posts_the_exact_ts_request`, `the_client_batches_through_the_sink`, `failures_drop_the_batch` | ignoré (`#[ignore]`, 1 ligne chacun) | Ils exigent une vraie requête vers un serveur local ; la coupure n'en émet plus. `an_unreachable_endpoint_times_out_as_a_drop` passe toujours (`Dropped`). |
| `crates/pa-core/src/session_engine/telemetry/tests.rs` | `legacy_events_reach_the_analytics_endpoint_in_the_ts_shape` | ignoré (`#[ignore]`, 1 ligne) | Même raison. Les autres tests de télémétrie utilisent des sinks factices ou le miroir local. |
| `crates/pa-cli/src/client_traces.rs` | `the_setting_writes_and_reads_the_agent_traces_flag` | défaut | `set_enabled(true)` refusé avec `TRACE_SHARING_OFF` ; un réglage écrit à la main ne rallume pas l'état. |
| `crates/pa-cli/src/interactive_mode/tests.rs` | `settings_sink_persists_the_fresh_home_answer_with_the_flag` | défaut | Un dossier neuf déclare le choix fait : la question n'est plus posée. Reste du test inchangé. |
| `crates/pa-tui/src/chrome/tests.rs` | `splash_renders_logo_version_model_and_cwd` | marque | `yusAgent v0.0.0` ; vérifie l'**absence** du papillon et de « prime agent ». |
| `crates/pa-tui/src/onboarding.rs` | `trace_question_render_is_unchanged` | marque | Golden transformé mécaniquement : chaque glyphe du papillon (76) devient une espace dans son span, « PRIME » + « Agent » devient un span « yusAgent » en gras. Aucun autre span ne bouge. |
| `crates/pa-cli/tests/acp_mode_e2e.rs` | `acp_initialize_matches_the_ts_golden` | marque | `agentInfo` attendu `yusagent` / `yusAgent`. |
| `crates/pa-tui/src/view/{tests,lazy_tests}.rs`, `crates/pa-tui/tests/chat_open_first_frame_headless.rs`, `crates/pa-cli/tests/{chat_open_first_frame_pty_e2e,input_live_open_pty_e2e}.rs` | splash présent ou absent | marque | Cherchent `yusAgent` au lieu de `prime agent`. |
| `crates/pa-cli/tests/{daemon_commands_e2e,interactive_daemon_e2e,continue_guard_e2e}.rs`, `crates/pa-daemon/tests/hold_refusal_e2e.rs`, `crates/pa-tui/tests/{daemon_rejection_headless,daemon_shutdown_reconnect_headless,daemon_shutdown_turn_end_headless,telemetry_command_headless}.rs`, `crates/pa-tui/src/agents_view/tests/notices.rs`, `crates/pa-cli/src/incident/report_tests.rs` | messages attendus | marque | Même remplacement que dans le code. |

## Renommage du CLI, PR 2 : binaire (YA-16, ticket 19)

Décisions : `docs/tickets/19-ya16-cli-yusagent.md`. Ni prompt, ni tools, ni requêtes modèle touchés.

**Textes de télémétrie alignés sur la coupure de la PR 1** (premier commit). La télémétrie distante ne part plus (`REMOTE_TELEMETRY`, PR 1) ; ces textes le disent.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-core/src/session_engine/telemetry/status.rs` | modif. (fichier upstream, 11 lignes + commentaires) | `/telemetry status` et `prime-agent telemetry` : « Telemetry is off (disabled in yusAgent). » et l'id d'installation, sans endpoint. `on` refusé avec « Remote telemetry is disabled in yusAgent. », rien n'est écrit. `off` inchangé. | YA-16 |
| `crates/pa-cli/src/telemetry_notice.rs` | modif. (fichier upstream, 1 ligne + commentaire) | La notice des modes sans TUI ne s'imprime plus. | YA-16 |
| `crates/pa-cli/src/client_settings.rs` | modif. (fichier upstream, 1 ligne + commentaire) | La notice du TUI ne s'affiche plus. | YA-16 |
| `crates/pa-telemetry/src/sinks/mod.rs`, `crates/pa-telemetry/src/lib.rs` | modif. (fichiers upstream, 1 réexport chacun) | Exportent `REMOTE_TELEMETRY`. | YA-16 |

Limite : un `telemetry.enabled: true` écrit à la main active toujours le miroir local `telemetry.jsonl` (rien ne sort), alors que l'état affiché est « off ».

**Binaire `yusagent`.** La cible Cargo reste `prime-agent` (les 62 `CARGO_BIN_EXE_prime-agent` des tests upstream ne bougent pas) ; le fichier livré dans l'archive s'appelle `yusagent`. L'archive garde son nom (`prime-agent-<version>-<plateforme>.tar.gz`) et les workflows de release upstream (`continuous.yml`, `release.yml`) sont inchangés : sans l'option, rien ne change.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `scripts/release/assemble_artifacts.py` | modif. (script upstream, option + 5 lignes) | `--binary-name` : nom du binaire dans l'archive (`.exe` ajouté sous Windows), gardé exécutable (`EXECUTABLE_ENTRIES`). Absente : comportement upstream. | YA-16 |
| `scripts/release/verify_release.py` | modif. (script upstream, option + 7 lignes) | `--binary-name` : contenu attendu et `--version` lus sur ce nom ; le contrôle du contrat de l'installateur TS natif (`validate_compatibility_archive`, qui exige `prime-agent`) est sauté pour une archive renommée, jamais servie à cet installateur. Vérifié en local : archive assemblée avec un faux binaire, avec et sans l'option. | YA-16 |
| `.github/workflows/ya-build-macos-arm64.yml` | modif. (fichier du fork) | `--binary-name yusagent` à l'assemblage et à la vérification ; artefact `yusagent-aarch64-apple-darwin-<sha>`. | YA-16 |
| `crates/pa-daemon/src/boot_reap.rs` | modif. (fichier upstream, 1 ligne + commentaire, 1 test ajouté) | `is_product_binary` reconnaît `yusagent` : les workers orphelins d'un démon `yusagent` sont récupérés au démarrage. | YA-16 |
| `crates/pa-daemon/src/herdr.rs` | modif. (fichier upstream, 1 ligne + commentaire) | Commande de reprise transmise à herdr : `yusagent --resume …` (le nom d'agent `prime-agent` du protocole herdr est inchangé). | YA-16 |
| `crates/pa-cli/src/config.rs` | modif. (fichier upstream, 1 ligne) | `APP_NAME = "yusagent"` : aide, usages, messages. | YA-16 |
| `crates/pa-cli/src/daemon_discovery/mod.rs` | modif. (fichier upstream, 5 lignes + commentaire) | La découverte des démons (`doctor`, `shutdown`) filtre les processus par nom de commande ; elle prenait `APP_NAME`. Elle prend maintenant le nom de fichier du binaire courant (`current_exe`), celui sous lequel le démon tourne : `yusagent` livré, `prime-agent` en build Cargo (sans cela, `daemon_discovery_e2e` ne trouvait plus les démons de test, run 44552470671). | YA-16 |
| `crates/pa-cli/src/{public_command,args,daemon_command,factory_command,prompt_command,print_runtime}.rs`, `crates/pa-cli/src/daemon_discovery/{plan,stop}.rs`, `crates/pa-cli/src/interactive_mode/daemon.rs`, `crates/pa-core/src/models/resolver.rs`, `crates/pa-daemon/src/acp/{daemon,mod}.rs`, `crates/pa-daemon/src/hold_refusal.rs`, `crates/pa-tui/src/{session_open_error,incident_notices}.rs`, `crates/pa-tui/src/interactive/run.rs`, `crates/pa-tui/src/session_ui/stream.rs`, `crates/pa-tui/src/settings_menu/mod.rs` | modif. (fichiers upstream) | Commande citée dans les messages : `prime-agent <commande>` devient `yusagent <commande>` (usages, « Use … », « Attach instead: … », `shutdown --force`, `attach`, `incident`, mode ACP). Remplacement limité aux lignes hors commentaires et aux mots de commande ; prompts, identifiants de protocole (`prime-agent.*`, `clientInfo` MCP, agent herdr), cas du produit TypeScript et code de mise à jour exclus. | YA-16 |
| `crates/pa-cli/src/ya_update_off.rs` | ajout | `SELF_UPDATE = false` et le message de refus. Unharnessed a retiré la commande (`75cfc357fe501a46f4a01a345457c0897eb88d89`) et le module (`09f760dfa4b9fbb205443f6fcf7736256ccc226f`). | YA-16 |
| `crates/pa-cli/src/lib.rs` | modif. (fichier upstream, 1 ligne + commentaire) | Déclare `ya_update_off`. | YA-16 |
| `crates/pa-cli/src/public_command.rs` | modif. (fichier upstream, 3 lignes + commentaire) | `update` (toutes options, `--check` compris) refusé avant tout téléchargement, code 1. | YA-16 |
| `crates/pa-cli/src/client_update.rs` | modif. (fichier upstream, 3 lignes + commentaire) | `/update` du TUI refusé avant tout téléchargement (sinon, hors installation gérée, il lançait l'installateur upstream de `app.primeintellect.ai`). La confirmation du TUI garde son texte (un test e2e le fige) ; le refus arrive après. | YA-16 |
| `crates/pa-cli/src/command_registry.rs`, `crates/pa-types/src/slash_commands.rs` | modif. (fichiers upstream) | Aide de `update` et description de `/update` : « Self-update (disabled in yusAgent) ». | YA-16 |
| `crates/pa-cli/tests/ya_update_off_e2e.rs` | ajout | `update`, `update --check`, `--nightly`, `--rollback` : code 1, message exact, rien sur la sortie standard. | YA-16 |
| `bench/smoke/tools/install-bin.sh`, `bench/smoke/tools/ab.py` | modif. (fichiers du fork) | Artefact `yusagent-<cible>-<sha>` ou `prime-agent-<cible>-<sha>` ; binaire `yusagent`, sinon `prime-agent` (builds d'avant le ticket 19, pour les A/B sur d'anciens SHA). | YA-16 |
| `CLAUDE.md`, `Makefile`, `bench/smoke/README.md`, `bench/smoke/run.py`, `bench/smoke/continue_session.py`, `bench/smoke/tools/notice-batch-repro.sh`, `bench/tools/capture/{README.md,run.py,analyze.py}` | modif. | Chemin du binaire : `<dossier>/yusagent`. Options de `run.py` inchangées. | YA-16 |

Non modifiés : `package update` (sa partie « mise à jour de soi » refuse déjà une installation non gérée par l'installateur upstream ; `package_e2e` compare cette commande avec le binaire TS), le texte de la confirmation de `/update`, le code de mise à jour lui-même (inatteignable).

### Tests upstream touchés (YA-16, ticket 19, PR 2)

| Fichier | Test | Nature | Changement |
|---|---|---|---|
| `crates/pa-core/src/session_engine/telemetry/status.rs` | `on_and_off_round_trip_through_the_ts_settings_key` | défaut | `on` refusé avec le message, le réglage reste `false`. |
| `crates/pa-core/src/session_engine/telemetry/status.rs` | `status_names_the_endpoint_and_the_installation_id` | défaut | État « disabled in yusAgent », aucune ligne `Endpoint:`. |
| `crates/pa-core/src/session_engine/telemetry/tests.rs` | `a_disable_lands_even_when_the_telemetry_state_is_unusable` | défaut | La réactivation est refusée et laisse la désactivation en place. |
| `crates/pa-cli/src/telemetry_notice.rs` | `headless_discloses_immediately_and_once` | défaut | La notice ne s'imprime plus. |
| `crates/pa-cli/tests/telemetry_command_e2e.rs` | `status_on_and_off_round_trip` | défaut | État « disabled in yusAgent » sans endpoint ; `telemetry on` sort en code 1 et n'écrit rien. |
| `crates/pa-cli/tests/installer_update_e2e.rs` | 8 tests qui lancent `update` | ignoré (`#[ignore]`, 1 ligne chacun) | La commande est désactivée. `archive_check_answers_synchronously_without_publishing` (qui lance `install-rust.sh` seul) tourne toujours. |
| `crates/pa-cli/tests/daemon_commands_e2e.rs` | `normalize_token` (différentiel contre le binaire TS) | normalisation (3 lignes) | Le mot `yusagent` est ramené à `prime-agent` avant la comparaison avec la CLI TS. Usages attendus de `schedule_usage_errors_and_valid_invocations` : `yusagent`. |
| `crates/pa-daemon/src/herdr.rs`, `crates/pa-daemon/tests/herdr_pane_e2e.rs` | `the_resume_argv_rides_only_valid_targets`, reprise herdr | marque | `yusagent --resume`. |
| `crates/pa-daemon/src/hold_refusal.rs`, `crates/pa-tui/src/session_open_error.rs`, `crates/pa-daemon/src/acp/mod.rs`, `crates/pa-cli/src/{factory_command,interactive_mode/tests}.rs`, `crates/pa-tui/src/{interactive/tests,agents_view/tests/notices}.rs`, `crates/pa-cli/tests/{acp_mode_e2e,continue_guard_e2e,daemon_discovery_e2e,print_runtime_e2e}.rs`, `crates/pa-daemon/tests/hold_refusal_e2e.rs`, `crates/pa-tui/tests/agents_view_incident_notice_e2e.rs` | messages attendus | marque | Même remplacement que dans le code. Dans `hold_refusal.rs`, l'assertion du cas TypeScript (« no bare `prime-agent --resume` ») est gardée telle quelle. |

## Renommage du CLI, PR 3 : dossier (YA-16, ticket 19)

Décisions : `docs/tickets/19-ya16-cli-yusagent.md`. Ni prompt, ni tools, ni paramètres de requête touchés dans le code ; le préfixe ne change que pour une session qui lit des fichiers du dossier (voir le ticket).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-cli/src/config.rs`, `crates/pa-core/src/settings/storage.rs`, `crates/pa-core/src/skills/loader.rs`, `crates/pa-daemon/src/paths.rs`, `crates/pa-types/src/platform/dirs.rs` | modif. (fichiers upstream, 1 ligne chacun) | `CONFIG_DIR_NAME = ".yusagent"` : dossier de l'agent `~/.yusagent` (surcharge `PRIME_AGENT_CODING_AGENT_DIR` inchangée) et dossier de projet `<cwd>/.yusagent/`. Cinq définitions (l'inventaire du ticket en comptait quatre ; `pa-types` est la cinquième). Pas de migration. | YA-16 |
| `crates/pa-core/src/kernel/bootstrap/venv/layout.rs` | modif. (fichier upstream, 2 lignes + commentaire) | Venv du kernel : `~/.yusagent/kernel-venv` (surcharge `PRIME_AGENT_KERNEL_VENV` inchangée), repli XDG `~/.local/share/yusagent/kernel-venv`. Le venv reste lié au dossier personnel et non à `PRIME_AGENT_CODING_AGENT_DIR`, comme upstream : les tests qui posent cette variable sur un dossier temporaire réutilisent le venv amorcé par la CI. Écart à la décision initiale, accepté par Yusuf le 19/11/2026 (journal du CDC). | YA-16 |
| `crates/pa-core/src/kernel/bootstrap/runtime_code.rs` | modif. (fichier upstream, 1 ligne) | Message d'erreur : `~/.yusagent/kernel-venv`, `yusagent`. | YA-16 |
| `crates/pa-daemon/src/platform/paths.rs` | modif. (fichier upstream, 1 ligne + commentaire) | Dossier des sockets `$TMPDIR/yusagent-<uid>` : plus de socket partagé avec un Prime Agent upstream installé à côté. Noms Windows (`prime-agent-user`, pipe `prime-agent-daemon`) inchangés : pas d'exécution Windows native (CDC, WSL2). | YA-16 |
| `prime-agent-runtime/src/rlm/{harness,mcp,mcp_base,factory}.py`, `skills/websearch/src/websearch/websearch.py` | modif. (fichiers upstream, 1 ligne chacun) | Repli sans `PRIME_AGENT_CODING_AGENT_DIR` : `~/.yusagent` au lieu de `~/.prime/agent`. L'hôte pose toujours cette variable dans le kernel (`crates/pa-core/src/session_engine/runtime_wiring.rs:272`) ; le repli ne sert que hors de l'hôte. Le changement du runtime reconstruit le venv une fois. | YA-16 |
| `crates/pa-cli/src/config_command.rs`, `crates/pa-tui/src/session_ui/commands.rs`, `crates/pa-tui/src/bin/pa-tui-replay.rs` | modif. (fichiers upstream) | Textes : « User (~/.yusagent/) », confirmation de `/update`, outil de relecture. | YA-16 |
| `skills/skill-creator/SKILL.md` | modif. (corps, 2 lignes) | Emplacements des skills : `.yusagent/skills/<name>/`, `~/.yusagent/skills/<name>/`. La description (ligne 3, reprise dans l'inventaire du prompt v1) est inchangée. | YA-16 |
| `.github/workflows/ci.yml` | modif. (1 ligne) | `PRIME_AGENT_KERNEL_PYTHON` des contrôles sur le vrai binaire : `~/.yusagent/kernel-venv`, là où `--prime-agent-bootstrap` amorce maintenant le venv. | YA-16 |
| `scripts/check_shutdown_scope.py` | modif. (script upstream, 1 ligne) | Socket par défaut reconstruit : `yusagent-<uid>`. | YA-16 |

Non modifiés : `~/.prime/config.json` (CLI `prime` de Prime Intellect), `~/.prime/supervisor-owners` (migration depuis le produit TS), code et tests de mise à jour et de release, fixtures `testdata/`, garde-fou `NEVER_TOUCH_SOCKET_DIRS` (infrastructure de test d'upstream), `scripts/package_release.py`.

### Tests upstream touchés (YA-16, ticket 19, PR 3)

Remplacement mécanique de `.prime/agent` par `.yusagent` (et `join(".prime").join("agent")`, `Path.home() / ".prime" / "agent"`) dans les lignes hors commentaires de 62 fichiers de tests : chemins de fixtures, dossiers de projet créés par les tests, venv amorcé par la CI (`{home}/.yusagent/kernel-venv/bin/python` dans une trentaine de tests e2e du démon et du cœur), dossier par défaut attendu (`crates/pa-types/src/platform/dirs.rs`, `agent_dir_default_under_home`). Dossier des sockets attendu : `crates/pa-daemon/src/platform/paths.rs` (deux tests), `crates/pa-cli/tests/daemon_discovery_e2e.rs:735`. Aucune assertion changée sur le fond. `crates/pa-types/tests/{daemon_wire_roundtrip,session_jsonl_roundtrip}.rs` : balayages du vrai dossier, seulement avec `PA_TYPES_LIVE_*=1`.

Corrigés après le run `ci` 44593768562 :

| Fichier | Test | Nature | Changement |
|---|---|---|---|
| `crates/pa-core/src/packages/manager.rs` | `project_scope_persists_against_project_settings_base` | profondeur | Chemin relatif du paquet depuis `.yusagent/` (un niveau) : `../project-pkg` au lieu de `../../project-pkg`. |
| `crates/pa-cli/tests/package_e2e.rs` | `package_manager_corpus_matches_ts_binary` (moitié Rust) | profondeur | Même raison : `../local-pkg`. |
| `crates/pa-cli/tests/acp_mode_e2e.rs` | `acp_daemon_attached_default_session_persists_and_resumes` | dossier | Le dossier des sessions par défaut se termine par `.yusagent/sessions`. |
| `crates/pa-cli/tests/daemon_commands_e2e.rs` | `ts_daemon_differential_cli_output` | socket | Dans le `TMPDIR` jetable du test, le démon TS écoute dans le vrai dossier `prime-agent-<uid>` (son défaut, où il crée aussi ses workers) et `yusagent-<uid>` (défaut de la CLI Rust) est un lien vers ce dossier : les deux CLI joignent le même démon. Le lien dans l'autre sens échouait : les workers TS ne créaient pas leur socket dans un dossier lié (`connect ENOENT …/prime-agent-1001/worker-….sock`, run 44598183340). |

## Permissions, PR 1 : cœur et banc (YA-11, ticket 20)

Décisions : `docs/tickets/20-ya11-permissions.md`. Réglage global `permissions` (`"manual"` par défaut, `"auto"`), lu dans le seul `<dossier agent>/settings.json` ; en manuel, chaque appel de tool est confirmé avant de s'exécuter, un refus arrête le tour ; une session manuelle sans moyen de demander (`-p`, RPC, et le démon jusqu'à la PR 2) est refusée à sa création. **Ni prompt, ni tools, ni paramètres de requête touchés : préfixe inchangé** (test `permissions_manual_and_auto_send_the_same_prefix`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-core/src/session_engine/ya_permissions.rs` | nouveau | Mode (réglage global, jamais le réglage de projet), refus d'une session manuelle sans demandeur avec la ligne exacte et le chemin, avertissement sur stderr en « auto » (session sans démon, racine), hook `before_tool_call` (demande, journal `ya-permission`, texte fixe `[permission-denied] …`), arrêt du tour par le hook de fin de tour. Tests unitaires (D7). | YA-11 |
| `crates/pa-core/src/session_engine/mod.rs` | modif. (2 lignes) | Déclaration du module. | YA-11 |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (fichier upstream) | `create_session` délègue à `create_session_with(config, PermissionsWiring)` (nouvelle fonction, même corps) ; dans le corps : résolution du mode juste après le chargement des réglages, création du gate après le câblage de la session, enveloppe de `config.should_stop_after_turn` (le hook de fin de tour de l'embedding), champ `before_tool_call` d'`AgentOptions`. Le bloc upstream de `should_stop_after_turn` n'est pas modifié. `SessionEngineConfig` n'a pas de champ de plus (les tests upstream l'écrivent champ par champ). | YA-11 |
| `.cargo/config.toml` | modif. (1 ligne + commentaire) | `PRIME_AGENT_TEST_PERMISSIONS = "auto"` pour les tests upstream, qui lancent des sessions sans demandeur ; lu par les builds de debug seulement (`cfg!(debug_assertions)`), ignoré en release. Même principe que `PRIME_AGENT_PROMPT_VERSION`. | YA-11 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | `try_engine_with_permissions` ; test `permissions_manual_and_auto_send_the_same_prefix` (empreintes SHA-256 du prompt système et des tools et paramètres identiques en manuel et en auto, chaque requête). | YA-11 |
| `crates/pa-cli/tests/ya_permissions_e2e.rs` | nouveau | Vrai binaire et serveur local qui compte ses connexions : `-p` et RPC sans réglage refusés au démarrage, avant toute requête, avec la ligne exacte et le chemin ; un `settings.json` de projet en « auto » ne change rien ; contrôle : « auto » global atteint le modèle et affiche l'avertissement. | YA-11 |
| `scripts/lifecycle_lib.py`, `scripts/check_custom_ca.py`, `scripts/check_kernel_env.py`, `scripts/check_live_mcp.py`, `scripts/check_local_harness.py`, `scripts/check_restore_notice.py`, `scripts/check_minimum_reasoning.py`, `scripts/check_observe_selectors.py` | modif. (scripts upstream, 1 mot chacun) | `PRIME_AGENT_TEST_PERMISSIONS` ajoutée à la liste blanche de l'environnement transmis au binaire de debug. Ces contrôles lancent le binaire hors de `cargo` (sans `.cargo/config.toml`) avec un environnement filtré ; sans la variable, leurs sessions étaient refusées en manuel (runs `ci` 44703584290 et 44706678374, shard 1). | YA-11 |
| `.github/workflows/ci.yml` | modif. (1 ligne + commentaire) | `export PRIME_AGENT_TEST_PERMISSIONS=auto` dans l'étape « Run the rust lifecycle checks ». | YA-11 |
| `crates/pa-perf/src/corpus/mod.rs` | modif. (fichier upstream, 1 ligne + commentaire) | `"permissions": "auto"` dans le `settings.json` du corpus : le perf nocturne (`perf.yml`, binaire de release, qui ignore la variable de test) lance des sessions sans humain. | YA-11 |
| `.changes/ya-11-permissions.md` | nouveau | Fragment de changelog (contrôle « Changelog fragment »). | YA-11 |
| `bench/smoke/run.py`, `bench/tools/capture/run.py`, `bench/smoke/tools/notice-batch-repro.sh` | modif. (fichiers du fork) | `"permissions": "auto"` écrit dans le `settings.json` du dossier agent du banc, de la capture et du script de reproduction (le banc tourne sans humain). Les binaires d'avant le réglage l'ignorent. | YA-11 |

Non modifiés : `pa-agent` (le hook `before_tool_call` et l'arrêt par `should_stop_after_turn` existent : `crates/pa-agent/src/agent_loop/tool_call.rs:76-98`, `crates/pa-agent/src/agent_loop/run.rs:132-152`), les réglages (`crates/pa-core/src/settings/`, la clé est lue dans le document global brut), le démon, ACP et le TUI (PR 2 et 3).

## Permissions, PR 2 : démon et ACP (YA-11, ticket 20)

Décisions : `docs/tickets/20-ya11-permissions.md`, section « PR 2 ». En manuel, le worker demande chaque appel de tool à ses clients (événement de session `ya_permission_request`, réponse par la commande `ya_permissions`) ; le pont ACP le traduit en `session/request_permission` (options `allow_once` et `reject_once`). Une demande en attente est retirée (refus, le tour s'arrête) sur `session/cancel` ou quand le client qui répond se déconnecte. Les décisions et les changements de mode sont journalisés dans le fichier de session du worker (`ya-permission`). Sélecteur `permissions` (`manual`, `auto`) dans `session/set_config_option`. Seul le pont ACP déclare savoir répondre : une session du TUI en manuel reste refusée à sa création (PR 3). **Ni prompt, ni tools, ni paramètres de requête touchés : préfixe inchangé.**

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-core/src/session_engine/ya_permissions.rs` | modif. (fichier du fork) | Journal injectable (`PermissionsWiring::journal`, le moteur du démon tourne sur un gestionnaire de session en mémoire) ; une demande abandonnée par une annulation est journalisée `cancelled` ; mode modifiable à chaud (`PermissionSwitch`, `PermissionsWiring::switch`) : avec un demandeur et un sélecteur, le gate est posé dans les deux modes et lit le mode avant chaque appel ; `configured_mode`, `mode_change_entry`. Tests unitaires. | YA-11 |
| `crates/pa-core/tests/prefix_stability.rs` | modif. (fichier du fork) | Test `permissions_journal_does_not_change_the_requests_of_a_resumed_session` : session reprise avec des entrées `ya-permission` (manuel) et sans (auto), deux serveurs, corps entiers identiques hors `prompt_cache_key`, avant et après `SessionManager::open`. | YA-11 |
| `crates/pa-daemon/src/ya_permissions.rs` | nouveau | Demandeur du worker : table d'attente, événements `ya_permission_request` et `ya_permission_resolved`, commande `ya_permissions` (`respond`, `set_mode`, `get`), retrait sur annulation et sur départ du dernier client qui répond, journal persisté par le worker (`persist_entry("custom", …)`), champ `yaPermissions` de `get_connection_state`. Tests unitaires, dont la reprise du démon (`SessionFile::branch_file_entries` puis `build_session_context`, avec et sans entrées). | YA-11 |
| `crates/pa-daemon/src/acp/ya_permissions.rs` | nouveau | Pont ACP : `session/request_permission` (requête agent → client, table des ids envoyés), lecture des réponses du client avant `jsonrpc::parse_line` (qui les refuse toujours), `pending` puis `in_progress`, sélecteur `permissions`. Tests unitaires. | YA-11 |
| `crates/pa-daemon/src/lib.rs`, `crates/pa-daemon/src/acp/mod.rs` | modif. (2 lignes chacun) | Déclaration des modules. | YA-11 |
| `crates/pa-daemon/src/agent_engine.rs` | modif. (fichier upstream, 2 lignes) | Champ `ya_permissions` d'`AgentSessionEngine` (survit aux reconstructions du moteur). | YA-11 |
| `crates/pa-daemon/src/agent_engine/lifecycle.rs` | modif. (fichier upstream, 6 lignes) | Initialisation du champ ; `build_session` appelle `create_session_with(config, wiring)` par une fermeture locale (le bloc de configuration n'est pas réindenté). | YA-11 |
| `crates/pa-daemon/src/worker.rs` | modif. (fichier upstream, 2 lignes) | Installe les puits (événements de session, journal) à côté de `set_ya_header_sink`. | YA-11 |
| `crates/pa-daemon/src/worker/create.rs`, `crates/pa-daemon/src/worker/connection.rs` | modif. (fichiers upstream, 2 lignes pour `create.rs`, 4 pour `connection.rs`) | `create` : la session sait-elle demander ; `attach` : ce client répond-il ; `detach` : retrait des demandes quand plus aucun client ne répond. | YA-11 |
| `crates/pa-daemon/src/worker/commands.rs` | modif. (fichier upstream, 2 lignes) | Bras de dispatch `ya_permissions`. | YA-11 |
| `crates/pa-daemon/src/state_getters.rs` | modif. (fichier upstream, 2 lignes) | `get_connection_state` porte `yaPermissions` pour une session qui sait demander (source du sélecteur ACP). | YA-11 |
| `crates/pa-daemon/src/supervisor/worker_lifecycle.rs` | modif. (fichier upstream, 2 lignes) | `yaPermissionAsker` ajouté à la liste blanche des champs de la commande `create` durable : le worker la reçoit, et un worker relancé la rejoue. | YA-11 |
| `crates/pa-types/src/daemon/command.rs` | modif. (fichier upstream, 11 lignes) | Variante `DaemonCommand::YaPermissions` (`activeSessionId`, `action`, le reste dans `rest`). | YA-11 |
| `crates/pa-daemon/src/protocol.rs` | modif. (fichier upstream, 6 lignes) | `ya_permissions` dans `KNOWN_COMMAND_TYPES` (fin de liste, après les types natifs Rust), `command_active_session_id` (routage générique vers le worker), `command_type_name`. Pas dans les tables de plan (`plane.rs`) : aucun lien direct TUI → worker ne l'envoie avant la PR 3. | YA-11 |
| `crates/pa-daemon/src/acp/daemon.rs` | modif. (fichier upstream, 30 lignes) | Création de l'objet du fork ; consommateur d'événements : `pending` en manuel et requêtes au client ; boucle stdin : réponses du client lues avant `parse_line` ; `attach` de démarrage avec `yaPermissionAsker`. | YA-11 |
| `crates/pa-daemon/src/acp/wire_config.rs` | modif. (fichier upstream, 8 lignes) | Bras `permissions` de `apply_wire_config` ; le sélecteur ajouté en dernier par `picker_options_from_state`. | YA-11 |
| `crates/pa-cli/src/print_runtime.rs` | modif. (fichier upstream, 2 lignes) | La commande `create` du mode ACP porte `yaPermissionAsker: true`. | YA-11 |
| `crates/pa-daemon/tests/command_breadth.rs` | modif. (test upstream, 7 lignes) | Fixture de `ya_permissions` et ajout à la liste des types natifs admis. | YA-11 |
| `crates/pa-cli/tests/acp_config_option_e2e.rs` | modif. (test upstream, 4 assertions) | Le nombre d'options compte le sélecteur `permissions`, ajouté en dernier (les index des sélecteurs `model` et `thought_level` ne changent pas). | YA-11 |
| `crates/pa-cli/tests/ya_permissions_acp_e2e.rs` | nouveau | Vrai binaire en ACP, provider factice et vrai kernel, mode manuel (variable de test retirée) : accord puis refus ; `session/cancel` pendant une demande (appel fini en `failed`) ; réponse `cancelled` seule ; déconnexion du client pendant une demande, un second client restant attaché sans répondre (comme le TUI) ; sélecteur aller-retour. | YA-11 |
| `bench/tools/ya_permissions_acp_demo.py` | nouveau (outil du fork) | Démo du contrôle : binaire en ACP sur un démon isolé, provider factice, le terminal autorise ou refuse chaque demande ; vérifie fichiers, messages du fichier de session, journal `ya-permission` et journal du worker (code de sortie 1 sinon). Dossier de travail sous son chemin canonique (`docs/tickets/21-bail-chemin-symbolique.md`). | YA-11 |
| `.changes/ya-20-permissions-acp.md` | nouveau | Fragment de changelog. | YA-11 |

Limites connues : un client qui s'attache sans `yaPermissionAsker` (TUI) voit les événements de demande sans pouvoir y répondre ; après une relance du worker, le client ACP n'est plus enregistré comme répondant tant qu'il ne s'est pas rattaché, et les demandes sont retirées aussitôt (refus). Depuis le ticket 24, le pont se redéclare avant chaque prompt (section « Extension VS Code, PR 1 »).

## Permissions, PR 3 : TUI (YA-11, ticket 20)

Décisions : `docs/tickets/20-ya11-permissions.md`, section « PR 3 » (T1 à T10). Le TUI se déclare répondant (`yaPermissionAsker` sur son `create` et son `attach`) : ses sessions ne sont plus refusées en manuel. Une demande ouvre un panneau qui montre l'appel entier et prend `y` (autoriser une fois), `n` ou `Esc` (refuser une fois) après 0,5 s sans frappe (1 s depuis le ticket 22) ; avant, les touches vont à l'éditeur. `Ctrl+C` retire la demande aussitôt. `/permissions [manual|auto]` passe par la même commande que le sélecteur ACP (journalisée) ; la barre montre « auto · no sandbox » tant que la session est en « auto » (D4). Le worker annonce chaque changement de mode (`ya_permission_mode`) et retire les demandes d'un client parti avec sa connexion directe (T8). **Ni prompt, ni tools, ni paramètres de requête touchés : préfixe inchangé** (test `manual_and_auto_send_the_same_requests`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-types/src/daemon/ya_permissions.rs` | nouveau | Noms du protocole des permissions (champ `yaPermissionAsker`, événements `ya_permission_request`, `ya_permission_resolved`, `ya_permission_mode`, commande `ya_permissions`, champ d'état `yaPermissions`), partagés par le worker, le pont ACP et le TUI (qui ne dépend que de `pa-types`). | YA-11 |
| `crates/pa-types/src/daemon/mod.rs` | modif. (fichier upstream, 2 lignes) | Déclaration du module. | YA-11 |
| `crates/pa-types/src/slash_commands.rs` | modif. (fichier upstream, 2 lignes) | Commande `permissions` (`[manual\|auto]`) dans la table des commandes slash. | YA-11 |
| `crates/pa-daemon/src/ya_permissions.rs` | modif. (fichier du fork) | Noms repris de `pa-types` ; `set_mode` émet `ya_permission_mode` quand le mode change. Test unitaire étendu. | YA-11 |
| `crates/pa-daemon/src/worker/connection.rs` | modif. (fichier upstream, 11 lignes) | **T8** : `release_session_attachments` (fin d'une connexion, dont un lien direct, qui n'a jamais de `detach` routé par le superviseur) retire de la liste des répondants les clients qui partent avec elle ; sans cela, une demande restait en attente jusqu'à l'annulation du tour. `handle_attach` : l'état du snapshot porte `yaPermissions`, comme `get_connection_state` (la barre du TUI). | YA-11 |
| `crates/pa-tui/src/ya_permission_panel.rs` | nouveau | Panneau : texte entier de l'appel, replié à la largeur, caractères de contrôle et de mise en forme invisibles montrés échappés ; fenêtre de calme (T3) ; défilement ; file des demandes ; étiquette `AUTO_BADGE`. Tests unitaires (frappe continue avec `y` sans effet, `y` et `n` après une pause, `Ctrl+C` dans la fenêtre, repli, échappement, rendu). | YA-11 |
| `crates/pa-tui/src/session_ui/ya_permissions.rs` | nouveau | Événements de demande, de clôture et de mode ; touches du panneau ; réponse `respond` ; `/permissions` (choix ou argument) par `set_mode` ; état « auto » de la barre. | YA-11 |
| `crates/pa-tui/src/lib.rs` | modif. (fichier upstream, 2 lignes) | Déclaration du module `ya_permission_panel`, sans quoi le panneau du fork ne compile pas dans la crate. | YA-11 |
| `crates/pa-tui/src/session_ui.rs` | modif. (fichier upstream, 5 lignes) | Déclaration du module `session_ui::ya_permissions` (sous-module, pour lire les champs privés de `SessionUi`) et champ `ya_permission_auto`, l'état « auto » gardé d'un attach à la reconstruction de la vue. | YA-11 |
| `crates/pa-tui/src/view.rs` | modif. (fichier upstream, 3 lignes) | Champ `ya_permission` d'`AgentView`. | YA-11 |
| `crates/pa-tui/src/view/frame.rs` | modif. (fichier upstream, 13 lignes) | Seul endroit où le cadre est composé : le panneau y est posé sur les dernières lignes de la fenêtre de transcription (comme les toasts sur les premières) et ces lignes sont masquées aux clics ; l'éditeur, le curseur et les zones de clic du dock restent où ils sont, ce qui permet à la frappe de continuer dans l'éditeur pendant la fenêtre de calme (T3). | YA-11 |
| `crates/pa-tui/src/session_ui/apply.rs` | modif. (fichier upstream, 4 lignes) | Les événements de permission passent par le module du fork avant `event_to_update` (qui les réduisait à un rafraîchissement du statut). | YA-11 |
| `crates/pa-tui/src/session_ui/keys.rs` | modif. (fichier upstream, 9 lignes) | Le panneau voit chaque touche en premier et la rend si la fenêtre de calme court ; un collage relance la fenêtre ; bras `Permissions` du choix. | YA-11 |
| `crates/pa-tui/src/session_ui/lifecycle.rs` | modif. (fichier upstream, 12 lignes) | `attach` avec `yaPermissionAsker` ; lecture de `yaPermissions` dans l'état du snapshot ; la reconstruction recopie l'état « auto » dans la barre. | YA-11 |
| `crates/pa-tui/src/session_ui/commands.rs` | modif. (fichier upstream, 2 lignes) | Bras `permissions`. | YA-11 |
| `crates/pa-tui/src/interactive.rs` | modif. (fichier upstream, 2 lignes) | `create_config` porte `yaPermissionAsker` (repris par la reprise depuis la vue Agents). | YA-11 |
| `crates/pa-tui/src/interactive/run.rs` | modif. (fichier upstream, 18 lignes) | Le réveil de calme (`quiet_tick`) tourne tant que le panneau n'a pas été dessiné prêt à répondre, et redessine quand il l'est. | YA-11 |
| `crates/pa-tui/src/chrome.rs` | modif. (fichier upstream, 10 lignes) | Champ `ya_auto_no_sandbox` de `ChromeState` ; étiquette en tête du côté droit de la barre, couleur d'avertissement. | YA-11 |
| `crates/pa-tui/src/choice_picker.rs` | modif. (fichier upstream, 21 lignes) | `/permissions` sans argument ouvre un choix `manual` / `auto` (T4) : le sélecteur de `/effort` est réutilisé plutôt que recopié, d'où la variante `ChoicePurpose::Permissions` et le constructeur `ChoicePicker::permissions`. | YA-11 |
| `crates/pa-tui/src/config_selector.rs` | modif. (fichier upstream, 7 lignes) | Variante `SelectorKind::Permissions` : sans elle, le choix s'afficherait sous le titre « Thinking Level » et avec les textes de `/effort` (titre, invite de filtre, ligne « aucun résultat »). | YA-11 |
| `crates/pa-cli/Cargo.toml`, `Cargo.lock` | modif. (1 dépendance de test) | `vt100 = "0.15"` en dépendance de test de `pa-cli` (version déjà résolue pour les tests de `pa-tui`) : le TUI dessine par différence de cellules (ratatui), l'e2e pty doit émuler le terminal pour lire l'écran. Hors du binaire livré (voir sous le tableau). | YA-11 |
| `crates/pa-daemon/tests/ya_permissions_direct_link_e2e.rs` | nouveau | **T8** : un client répondant attaché par lien direct ferme son lien pendant une demande ; la demande est retirée (`cancelled`), le tour s'arrête, la cellule ne tourne pas. Rouge sans le correctif : run `ci` 44754613332, `test shard 1/8`. | YA-11 |
| `crates/pa-daemon/tests/ya_permissions_same_requests_e2e.rs` | nouveau | **T9** : vrai démon, vrai kernel, serveur local qui joue le modèle ; le même tour en manuel (accord) et en « auto » envoie des corps de requête identiques, `prompt_cache_key` mis à part. | YA-11 |
| `crates/pa-cli/tests/ya_permissions_tui_pty_e2e.rs` | nouveau | Vrai binaire, TUI sur pty (terminal de contrôle), démon, faux provider, vrai kernel, mode manuel : frappe pendant la demande renvoyée à l'éditeur, accord par `y` après une pause, `/permissions auto` (étiquette, cellule sans panneau), retour en manuel, refus par `n` ; journal `allow`, deux changements de mode, `deny`. | YA-11 |
| `bench/tools/ya_permissions_tui_demo.py` | nouveau (outil du fork) | Démo du contrôle : démon isolé, TUI du binaire, provider factice ; une cellule autorisée, une refusée, `/permissions auto` et une cellule sans panneau ; vérifie fichiers, messages du fichier de session, journal `ya-permission` et journal du worker (code de sortie 1 sinon). Mode `--pilote` (pty, écran lu par l'émulateur de `scripts/lifecycle_lib.py`) pour le lancement de Claude Code (règle 13). | YA-11 |
| `.changes/ya-20-permissions-tui.md` | nouveau | Fragment de changelog. | YA-11 |

**`vt100` n'entre pas dans le binaire livré.** Dans `crates/pa-cli/Cargo.toml`, il est déclaré à la ligne 45, dans la section `[dev-dependencies]` qui commence à la ligne 26 (les dépendances du binaire sont sous `[dependencies]`, ligne 8 ; le binaire `prime-agent`, renommé `yusagent` à l'empaquetage, est déclaré lignes 22-24). Cargo ne compile une dépendance de test que pour les tests, exemples et benchs du paquet, jamais pour `cargo build --release`. Vérifié par la résolution, sans compilation :

```sh
cargo tree -p pa-cli -e normal,build -i vt100 --target all --offline   # « warning: nothing to print. »
cargo tree -p pa-cli -e dev -i vt100 --offline --depth 1               # vt100 v0.15.2 <- [dev-dependencies] pa-cli
```

Les deux autres déclarations (`crates/pa-tui/Cargo.toml:36`, section `[dev-dependencies]` ligne 33 ; `crates/pa-perf/Cargo.toml:21`, outil de perf, absent de l'arbre normal de `pa-cli` : `cargo tree -p pa-cli -e normal,build --target all --offline | grep -c pa-perf` donne `0`) existaient avant ce ticket.

### Tests upstream touchés (YA-11, ticket 20, PR 3)

| Fichier | Changement | Raison |
|---|---|---|
| `crates/pa-tui/src/interactive/tests.rs` | `create_config_binds_a_new_child_to_its_parent` : le JSON attendu porte `"yaPermissionAsker": true`. | Le test compare la config du `create` en entier. |

Limites connues : une demande émise avant l'attach du TUI (session ouverte ailleurs) n'est pas rejouée à l'attach ; le panneau ne la voit pas, un autre client répondant ou l'annulation du tour la tranche. L'étiquette « auto » ne s'affiche que pour une session qui sait demander (`yaPermissions` absent sinon).

## Carte de la cellule pendant une demande de permission (YA-11, ticket 22)

Constat, plan et décisions : `docs/tickets/22-carte-cellule-permission.md` (K1 à K5). Pendant qu'une demande attend, la carte de l'appel n'est pas lancée (ni `✓`, ni minuteur) et le chargeur dit « Awaiting permission » ; l'appel part à l'accord du worker (`ya_permission_resolved`, `allow`), son minuteur aussi. La fenêtre de calme du panneau passe de 0,5 s à 1 s. **Ni prompt, ni tools, ni requêtes touchés : préfixe inchangé** (TUI seulement).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-tui/src/session_ui/ya_permissions.rs` | modif. (fichier du fork) | À la demande, la carte de son `toolCallId` est remise « non lancée » et le chargeur passe à « Awaiting permission » ; à `ya_permission_resolved` (toujours lu, plus seulement pour fermer le panneau), un `allow` lance la carte à cet instant (K3). | YA-11 (ticket 22) |
| `crates/pa-tui/src/ya_permission_panel.rs` | modif. (fichier du fork) | `waits_for` (une demande ouverte porte-t-elle cet appel) ; `QUIET` à 1 s. Tests unitaires ajustés à `QUIET` et test de `waits_for`. | YA-11 (ticket 22) |
| `crates/pa-tui/src/session_ui/apply.rs` | modif. (fichier upstream, 3 lignes) | La carte d'un appel en attente de permission reste non lancée même si `tool_execution_start` arrive après la demande : le worker publie la demande hors de la file des événements du moteur, elle peut doubler le départ de l'appel (K4). | YA-11 (ticket 22) |
| `crates/pa-tui/tests/ya_permission_card_headless.rs` | nouveau | Démon simulé, sans kernel ni modèle : carte non lancée et chargeur « Awaiting permission » pendant la demande, dans les deux ordres ; accord après 2 s d'attente, minuteur parti de l'accord, `✓` ; refus, `✗`, jamais de `✓` ni de minuteur. Rouge sans le correctif : run `ci` 44797069532 sur `2e8a0731a`, `test shard 6/8`, les trois tests en échec sur `✓ python · …` pendant l'attente. | YA-11 (ticket 22) |
| `crates/pa-cli/tests/ya_permissions_tui_pty_e2e.rs` | modif. (fichier du fork) | Pendant chaque demande, « Awaiting permission » présent, « Executing » absent, carte sans `✓` ; carte autorisée en `✓`, refusée en `✗`. | YA-11 (ticket 22) |
| `bench/tools/ya_permissions_tui_demo.py` | modif. (outil du fork) | Mêmes vérifications d'écran en `--pilote` (code de sortie 1 sinon) ; texte de la démo à 1 s. | YA-11 (ticket 22) |
| `.changes/ya-22-carte-permission.md` | nouveau | Fragment de changelog. | YA-11 (ticket 22) |

Limite connue : si `tool_execution_start` arrive avant la demande, une image peut être dessinée entre les deux avec la carte lancée (le temps de deux événements consécutifs) ; la demande la remet aussitôt « non lancée ». Le compteur de télémétrie des minuteurs vivants (`track_live_tool_timer`) compte aussi un appel retenu.

## Bail de session sous un chemin symbolique (ticket 21)

Constat et décision : `docs/tickets/21-bail-chemin-symbolique.md`.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-daemon/src/lease.rs` | report partiel d'upstream (fichier upstream) | Commit upstream `12a5081aca4248e8651f9f24ee4b01c249de6492` (#3871), partie « pa-daemon: keep one lease path for a session created in a new directory » seule, repris à l'identique par `git apply` du diff de ce fichier (aucune retouche) : `canonical_session_path` canonicalise le plus proche parent existant et rejoint la fin manquante, et le test unitaire `a_lease_taken_before_its_sessions_dir_exists_owns_the_created_file`. Corrige la perte silencieuse des messages de la première session d'un dossier agent neuf sous un lien symbolique. Les 70 autres fichiers du commit ne sont pas repris : pour un `git cherry-pick` futur de `12a5081ac`, `lease.rs` est déjà au contenu d'upstream. | YA-11 (ticket 21) |
| `crates/pa-cli/tests/ya_session_lease_symlink_e2e.rs` | nouveau | Vrai binaire en ACP, `HOME` en lien symbolique, dossier agent neuf, provider factice et vrai kernel en manuel : une cellule acceptée ; le fichier de session contient les 4 messages du tour et l'entrée `ya-permission`, le journal du worker aucune ligne `pa-daemon:`. Échoue sans le correctif, sous Linux aussi. | YA-11 (ticket 21) |
| `.changes/ya-21-bail.md` | nouveau | Fragment de changelog. | YA-11 (ticket 21) |

## Passivation différée dans les tests (ticket 23)

Constat et décisions : `docs/tickets/23-passivation-differee.md`.

Le superviseur répond `success` à `worker_idle_passivation` aussi quand il **diffère** l'arrêt : une requête routée vers le worker tient un permis (`crates/pa-daemon/src/supervisor/worker_lifecycle.rs:713-717`, report voulu). Le test `passivated_child_recent_messages_reads_its_file_end_to_end` prenait `success` pour « arrêté » et échouait par intermittence (run 44604335224, `agent_family_e2e.rs:1500`). L'ancienne hypothèse, une réponse `success` envoyée avant le retrait du worker du registre, est fausse : sur un arrêt effectif, `success` part après `registry.remove` (`worker_lifecycle.rs:797`). Ce test ne figure plus parmi les tests instables. Le produit ne change pas.

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-daemon/tests/agent_family_e2e.rs` | modif. (fichier upstream) | Aide `passivate_until_gone` : redemande la passivation toutes les 100 ms, jusqu'à 30 s, tant que la session est résidente. Elle remplace la demande unique suivie d'un `list` immédiat dans `passivated_child_recent_messages_reads_its_file_end_to_end`. | YA-16 (ticket 23) |
| `crates/pa-daemon/tests/ya_idle_passivation_defer_e2e.rs` | nouveau | Vrai démon, vrai kernel, provider factice. Une session racine au repos ; un second client y tient `execute_bash_and_wait` (`sleep 3`) pendant la demande. La première demande est différée, une demande après la fin de la commande arrête le worker (au moins 2 demandes). Avec l'ancien enchaînement, le test échoue. | YA-16 (ticket 23) |

**Pour un `git cherry-pick` futur de `c8cbb5ba7`** (upstream #3769, absent de notre historique) : ce commit ajoute à `agent_family_e2e.rs` un second test avec le même enchaînement (`upstream/main:crates/pa-daemon/tests/agent_family_e2e.rs:1895-1909`). Lui appliquer `passivate_until_gone` (décision P3 du ticket 23).

## Extension VS Code, PR 1 : cœur et pont ACP (YA-10, ticket 24)

Constat, décisions et plan : `docs/tickets/24-ya10-extension-minimal.md`. Ce qui part au modèle ne change pas : les ajouts sont des données d'affichage (`details` des résultats de cellule, `_meta` ACP), le journal d'usage local et le pont ACP. Contrôle par capture `main` contre la branche (ticket 24).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-core/src/session_engine/usage_log.rs` | modif. (fichier du fork) | Entrée `invalidation` sur l'appel de la boucle qui suit un changement de modèle (`model_change`, `from`/`to` en `provider/modèle`) ou d'effort (`thinking_level_change`), détecté en comparant l'appel au précédent (dans le processus, ou la dernière ligne `main` du journal après une reprise) : tous les chemins de changement (choix de l'utilisateur, bascule de secours, retour au modèle principal) sans toucher de fichier upstream. Plusieurs causes sur un même appel : un tableau. Tests unitaires. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/ya_permissions.rs` | modif. (fichier du fork) | `$/cancel_request` au client pour une demande réglée ailleurs (autre client, retrait par le worker) qu'il tient encore (E9) ; `rearm_asker` (action `asker` avant chaque prompt, E10). Tests unitaires. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/ya_permissions.rs` | modif. (fichier du fork) | Action `asker` de la commande `ya_permissions` : le client routé répond de nouveau (un worker relancé rejoue le `create`, pas les `attach`). Test unitaire. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/ya_cell_display.rs` | nouveau | Diffs de la cellule en contenu ACP `diff` (`_meta` `_yus/startLine`), résultats des `bash()` sous `_meta` `_yus/bashRuns`. Tests unitaires. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/mod.rs` | modif. (2 lignes) | Déclaration du module. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/daemon.rs` | modif. (fichier upstream, 3 lignes) | Appel de `rearm_asker` avant `PromptAndWait`. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/wire_events.rs` | modif. (fichier upstream, environ 15 lignes) | `tool_execution_end` : contenu `diff` et `_meta` `_yus/bashRuns` ajoutés par `ya_cell_display`. | YA-10 (ticket 24) |
| `crates/pa-daemon/src/acp/types.rs` | modif. (fichier upstream, environ 20 lignes) | `ToolCallContent` devient une énumération (`content`, `diff`) ; `ToolCallContent::new` inchangé. | YA-10 (ticket 24) |
| `prime-agent-runtime/src/rlm/bash.py` | modif. (fichier upstream, environ 40 lignes) | MIME d'affichage `application/vnd.yusagent.bash-run+json` émise quand la cellule lit un résultat de `bash()` (même point que le code de sortie de T05) : commande, code de sortie, sortie plafonnée à 8 000 caractères (tête et queue), durée. | YA-10 (ticket 24) |
| `prime-agent-runtime/test/test_ya_bash_run.py` | nouveau | Enregistrement émis pour un résultat lu dans une cellule vivante, pas hors cellule ; plafond tête et queue. | YA-10 (ticket 24) |
| `crates/pa-core/src/kernel/shared.rs` | modif. (fichier upstream, environ 25 lignes) | `YA_BASH_RUN_DISPLAY_MIME`, `KernelBashRun`, `parse_ya_bash_run_display`, champ `bash_runs` d'`ExecuteResult`. | YA-10 (ticket 24) |
| `crates/pa-core/src/kernel/manager/mod.rs`, `events.rs`, `execution.rs` | modif. (fichiers upstream, 3, 7 et 2 lignes) | Collecte des enregistrements `bash-run` de la cellule. | YA-10 (ticket 24) |
| `crates/pa-core/src/session_engine/runtime_wiring.rs` | modif. (fichier upstream, 1 ligne) | Transmission de `bash_runs` à l'outil. | YA-10 (ticket 24) |
| `crates/pa-core/src/tools/ipython.rs` | modif. (fichier upstream, environ 25 lignes) | `details.bashRuns` ; `oldStr`, `newStr` et `startLine` dans chaque entrée de `details.diffs`. Rien dans le texte du résultat. | YA-10 (ticket 24) |
| `crates/pa-core/src/tools/golden_replay.rs` | modif. (test upstream) | `ipython_details_carry_the_cell_edit_diffs` attend les trois champs ajoutés. | YA-10 (ticket 24) |
| `crates/pa-cli/tests/ya_permissions_acp_e2e.rs` | modif. (fichier du fork) | Test e : worker tué entre deux tours et relancé, la demande suivante arrive au client ACP. | YA-10 (ticket 24) |
| `crates/pa-cli/tests/ya_acp_cancel_e2e.rs` | nouveau | `session/cancel` en moins de 1 s pendant une cellule bloquée sur `await bash(...)` (commande tuée en moins de 1 s, la suite de la cellule ne tourne pas, le kernel sert le tour suivant) et pendant une cellule bloquée en Python. | YA-10 (ticket 24) |
| `bench/tools/capture/run.py` | modif. (outil du fork) | Chauffe en permissions « auto » ; cas `tui` (le TUI sur un pty, démon isolé) ; appel de `prefix_paths_check.py` ; cas `display-anthropic`, dont le fichier de session est copié pour `display_check.py`. | YA-10 (ticket 24) |
| `bench/tools/capture/server.py` | modif. (outil du fork) | Scénario `display` : `bash()` lu, `bash()` non lu dont l'avis `[bash-done]` arrive dans un tour de réveil à part (déterministe), deux `edit()` dans un dossier retiré à la fin. | YA-10 (ticket 24) |
| `bench/tools/capture/compare.py` | modif. (outil du fork) | `--all` compare aussi `acp` et `tui`, normalise le pid des avis `[bash-done]`, et finit par `TOUT IDENTIQUE` ou `DIFFÉRENCES`. | YA-10 (ticket 24) |
| `bench/tools/capture/display_check.py` | nouveau (outil du fork) | Contrôle du cas `display-anthropic` sur un binaire du ticket 24 : une seule entrée `details.bashRuns` (le `bash()` lu), deux `details.diffs` avec `oldStr`, `newStr`, `startLine`, avis `[bash-done]` dans la dernière requête. | YA-10 (ticket 24) |
| `bench/tools/capture/prefix_paths_check.py` | nouveau (outil du fork) | Contrôle permanent : `acp` et `tui` contre `-p`, échec si l'écart dépasse les deux lignes du prompt système liées au démon (écart accepté le 21/11/2026, journal du CDC). | YA-10 (ticket 24) |
| `.changes/ya-24-cellule-acp.md` | nouveau | Fragment de changelog. | YA-10 (ticket 24) |

Limites connues : un `bash()` dont la cellule ne lit pas le résultat (commande détachée) n'a pas d'enregistrement `bash-run` : son résultat arrive par l'avis `[bash-done]`. Un worker relancé au milieu d'un tour n'a pas de répondant jusqu'au prompt suivant (la demande est refusée aussitôt). Un changement d'effort qui ne change pas l'effort envoyé (niveau ramené au même par le modèle) n'est pas journalisé.

## Extension VS Code, PR 2 : squelette de l'extension (YA-10, ticket 24)

Décisions, contrôles et résultats : `docs/tickets/24-ya10-extension-minimal.md`, section « PR 2 ». **Aucun fichier upstream ni sous `crates/`** : le binaire et ce qui part au modèle ne changent pas. Trames de l'extension figées par un fixture (preuve 1) et rejouées contre le binaire par le banc de capture (preuve 2, `TOUT IDENTIQUE` avec le cas `acp`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `extensions/vscode/` | nouveau | Extension : lancement du binaire (`yusagent.binaryPath`, portée `machine`), client ACP écrit à la main (`src/acp/`), état du chat (`src/shared/state.ts`), session et relance (`src/session.ts`), onglet et webview React groupée par esbuild sous CSP stricte (`src/panel.ts`, `webview/`). Tests unitaires (`node --test`), agent factice et fixture des trames (`test/`), e2e dans VS Code (`test/e2e/`), scripts de contrôle (`scripts/`). | YA-10 (ticket 24) |
| `.github/workflows/ya-extension.yml` | nouveau | tsc, lint, tests unitaires, `vsce package` avec contrôle < 5 Mo et `.vsix` en artefact, e2e (`ubuntu-latest` + xvfb, VS Code 1.146.0) contre l'agent factice. Check non requis pendant la PR 2. | YA-10 (ticket 24) |
| `bench/tools/capture/run.py` | modif. (outil du fork) | Cas `acp-extension` : `acp_check` rejoue le fixture de l'extension (mêmes trames, prompt du cas `acp`, agent et socket à part) ; appel de `extension_frames_check.py`. | YA-10 (ticket 24) |
| `bench/tools/capture/compare.py` | modif. (outil du fork) | `--all` compare aussi `acp-extension`. | YA-10 (ticket 24) |
| `bench/tools/capture/extension_frames_check.py` | nouveau (outil du fork) | Preuve 2 : chaque requête de `acp-extension` identique à celle de `acp`, trames envoyées = fixture ; code de sortie 1 sinon. | YA-10 (ticket 24) |
| `bench/tools/capture/README.md` | modif. (doc du fork) | Cas `acp-extension` et contrôle. | YA-10 (ticket 24) |

## Extension VS Code, PR I : import d'Unharnessed (YA-10, ticket 24)

Décisions et résultats : `docs/tickets/24-ya10-extension-minimal.md`, sections « Décisions de Yusuf sur la comparaison », « Découpage » et « PR I ». **Aucun fichier sous `crates/` ni upstream** : le binaire et ce qui part au modèle ne changent pas (fixture des trames inchangé, preuve 1 ; mêmes `initialize`, `session/new`, `session/prompt`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `extensions/vscode/src/*.ts`, `extensions/vscode/webview/*.tsx`, `*.ts` | repris d'Unharnessed (`157dcdc1d`, `vscode/src`, `vscode/webview`), premier commit tel quel | Base de l'extension (option B). Adaptations marquées « yusAgent » dans les fichiers repris : `agent.ts` (notre client ACP, `binaryPath`, refus sans dossier, rejet sur place des méthodes non servies, demandes d'accord, journal sans paramètres, arrêt fin de stdin + 10 s + SIGKILL), `chat.ts` (`_meta` de notre pont, prompt en texte seul, message `/login`, ligne d'attente d'accord), `session.ts` et `protocol.ts` (valeur `permissions`, `Features`, sonde), `view.ts` (CSP stricte, identifiants `yusagent.*`, pas d'écran d'accueil), `extension.ts`, `App.tsx`, `Status.tsx` (bandeau D4, entrées masquées), `Composer.tsx` (mentions, images, « Send now » masqués), `Chat.tsx` (bouchon de `thinking-orbs`), `main.tsx` (sonde). | YA-10 (ticket 24) |
| `extensions/vscode/webview/logos.tsx`, `FileIcon.tsx`, `Wordmark.tsx`, `Splash.tsx`, `wave.ts`, `powder.ts`, `motion.ts`, `ya-orb.tsx` | bouchons | Mêmes exports que les fichiers visuels d'Unharnessed, rien de dessiné (visuel plus tard) ; `AsciiField.tsx` retiré. | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/` (`methods.ts`, `meta.ts`, `auth.ts`, `permissions.ts`), `webview/ya-features.tsx`, `webview/ya-probe.ts`, `webview/styles.css` | nouveaux | Adaptation à notre pont dans des modules à part : méthodes servies et fonctions montrées, `_meta` (début et fin de tour déduits), authentification, demandes d'accord, contexte des fonctions, sonde e2e, feuille minimale provisoire (pas le `styles.css` d'Unharnessed). | YA-10 (ticket 24) |
| `extensions/vscode/package.json`, `esbuild.mjs`, `eslint.config.mjs`, `tsconfig.json`, `webview/tsconfig.json`, `.vscodeignore`, `LICENSE`, `README.md`, `media/icon.svg` | modif. | Vue dans la barre latérale secondaire, commande `yusagent.open`, React 19.3.0, `react-markdown` 10.1.0, `remark-gfm` 4.0.1, `@types/vscode` 1.110.0 ; groupage `dist/webview/main.{js,css}` ; notice MIT avec les copyrights d'Unharnessed. | YA-10 (ticket 24) |
| `extensions/vscode/test/` | modif. | Tests unitaires du code adapté contre l'agent factice (`agent.test.ts`, bouchon du module `vscode`), e2e réécrit pour la vue ; l'agent factice porte le `_meta` de notre pont. | YA-10 (ticket 24) |
| `extensions/vscode/scripts/real-binary-check.mjs`, `install-vsix.sh` | modif. | Contrôle sur le vrai binaire avec le code importé ; liste des fichiers du `.vsix`. | YA-10 (ticket 24) |
| `.github/workflows/ya-extension.yml` | modif. (fichier du fork) | Tourne sur chaque PR ; l'étape « changements » sort vite en succès quand l'extension n'a pas changé (décision de Yusuf du 21/11/2026). | YA-10 (ticket 24) |

## Extension VS Code, PR 3 : cartes, boîte, diffs (YA-10, ticket 24)

Décisions et résultats : `docs/tickets/24-ya10-extension-minimal.md`, sections « PR 3 : constat et plan » et « Décisions de Yusuf sur la PR 3 ». Extension seule : **aucun fichier sous `crates/` ni upstream** ; fixture des trames inchangé (preuve 1).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `extensions/vscode/webview/Composer.tsx`, `src/tabs.ts`, `webview/SettingsSkills.tsx`, `webview/Settings.tsx`, `src/protocol.ts` (commentaire) | modif. (repris d'Unharnessed) | Étape 1, défaut 1 : plus de « Unharnessed » visible (invite « Ask yusAgent », schéma des onglets `yusagent-tool`, type MIME du glisser `application/x-yusagent-queued`, dossiers de skills et de réglages `~/.yusagent`, `.yusagent`). Défaut 2 : `KernelTray` affiché seulement si `features.kernel`. | YA-10 (ticket 24) |
| `extensions/vscode/src/chat.ts`, `src/protocol.ts`, `webview/Chat.tsx` | modif. (repris d'Unharnessed) | Étape 1, défaut 3 : statut `pending` d'un `tool_call` (« Awaiting permission », pas de « Working »), `running` au `tool_call_update` `in_progress`, « Not run » si le tour finit sans que l'appel sorte de `pending`. | YA-10 (ticket 24) |
| `extensions/vscode/src/cell.ts` | modif. (repris d'Unharnessed) | Étape 2, C7 : `readCell` reconnaît `edit()` et `read()` du kernel (`yaKernelFiles`, arguments positionnels ou nommés ; `write` si chaque `edit()` crée son fichier) ; `fragmentLines` exporté (lignes d'un fragment d'`edit()`). | YA-10 (ticket 24) |
| `extensions/vscode/src/chat.ts`, `src/protocol.ts`, `webview/Chat.tsx`, `webview/styles.css` | modif. | Étape 2 : `toolDone` exclut les contenus `diff` de la sortie et lit les données de carte du pont (`yaCellData`) ; types `BashRun`, `FileEdit`, champs `runs`, `edits`, `declined` d'un appel ; carte : « Declined » (C5), `exit N` dans l'en-tête (C6), `bash()` et vrais diffs dans la feuille (C6, C7), vrais diffs comptés même si la cellule a échoué ensuite ; règles CSS provisoires (lignes ajoutées et retirées, `bash()`). | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/cells.ts`, `webview/ya-cells.tsx` | nouveaux | Étape 2 : lecture de `_yus/bashRuns`, des contenus ACP `diff` (fragments, `_yus/startLine`) et du texte fixe du refus ; carte d'après les vraies données (`cellOfRuns`, `cellOfEdits`) ; composants des `bash()` (sortie repliée) et des diffs par fichier. | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/permissions.ts` | modif. (fichier du fork) | Étape 3 : options de chaque demande gardées, `asks` (demandes ouvertes, dans l'ordre), `answer(toolCallId, optionId)` (réponse `selected`, seulement une option de la demande, une seule fois). | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/permission-keys.ts`, `webview/ya-permission.tsx` | nouveaux | Étape 3 : touches de la boîte (C3 : `y`, `n`, `Esc`, fenêtre de calme de 1 s relancée par chaque frappe, jamais Entrée ; comme `crates/pa-tui/src/ya_permission_panel.rs`), code montré caractères invisibles échappés ; la boîte au-dessus de la saisie (C2), pas avant la carte de son appel, une action par option du pont, boutons inactifs 1 s après l'ouverture sans relance par les frappes, focus pris seulement si la zone de message est vide. | YA-10 (ticket 24) |
| `extensions/vscode/src/chat.ts`, `src/view.ts`, `src/protocol.ts`, `src/extension.ts`, `webview/Status.tsx`, `webview/styles.css` | modif. | Étape 3 : ligne d'attente d'accord retirée (`chat.ts`) ; messages `yaPermissions` (hôte → webview) et `permission` (webview → hôte), badge de la vue tant qu'une demande attend, toutes conversations (C4), sondes e2e `probeKeys` et `badge` (`view.ts`, `extension.ts`) ; boîte rendue au-dessus de la saisie (`Status.tsx`) ; règles CSS provisoires. | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/diffs.ts`, `reverts.ts`, `diff-files.ts`, `diff-files-vscode.ts` | nouveaux | Étape 4 (E6 a) : avant retrouvé en défaisant les fragments (contrôle de chaque fragment à sa ligne, ambiguïté C14 a), « Rejeter » par fichier et par cellule avec ses garde-fous (tour en cours, fichier changé, session précédente), texte fixe `[user-reverted]` pour le message suivant ; accès aux fichiers (Node, VS Code : `applyEdit` + enregistrement, corbeille, `vscode.diff` en lecture seule). | YA-10 (ticket 24) |
| `extensions/vscode/src/chat.ts`, `src/view.ts`, `src/extension.ts`, `src/protocol.ts`, `webview/ya-cells.tsx`, `webview/Chat.tsx`, `webview/Status.tsx`, `webview/styles.css` | modif. | Étape 4 : le chat note l'après et l'avant des cellules qui éditent, sert « Rejeter » et l'ouverture du diff, ajoute le texte fixe au message suivant (`start`), expire les cellules d'une autre session ; messages `yaOpenDiff` et `yaRevert` ; accès VS Code branchés à l'activation ; statut `revert` d'un fichier modifié ; nom du fichier cliquable, bouton « Revert » grisé pendant un tour (contexte `YaBusyContext`). | YA-10 (ticket 24) |
| `.github/workflows/ya-extension.yml` | modif. (fichier du fork) | Étape 6 (option D) : job `real-binary` : binaire debug construit dans le job (`cargo build --locked --workspace --bins`), cache cargo des shards de `ci.yml` en lecture seule (`shared-key: test-shards`, `save-if: false`), venv du kernel, e2e contre le vrai binaire ; seulement sur le périmètre de C15, sortie rapide sinon. Aucune ligne de `ci.yml`. | YA-10 (ticket 24) |
| `extensions/vscode/test/e2e/run-real.ts`, `suite-real.ts`, `package.json` (`test:e2e:real`), `src/extension.ts` (commande de test `stopAll`) | nouveaux, modif. | Étape 6 : e2e dans VS Code contre le vrai binaire (provider factice, HOME et TMPDIR isolés), relevé des processus restants. | YA-10 (ticket 24) |
| `extensions/vscode/scripts/real-binary-check.mjs` | modif. | Étape 7 : cas « cellules, boîte, diffs » sur le vrai binaire (provider factice, venv du banc par `PRIME_AGENT_KERNEL_VENV`). | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/cells.ts`, `test/unit/cells.test.ts` | modif. | Chemin montré relatif aussi quand le dossier de l'agent est ouvert par un lien (le pont donne le chemin résolu) ; trouvé par `real-binary-check.mjs` sur macOS. | YA-10 (ticket 24) |
| `extensions/vscode/src/ya/env.ts` (nouveau), `src/agent.ts` (repris d'Unharnessed, accroche dans `spawn`), `test/fake-agent/fake-agent.mjs`, `test/helpers.ts`, `test/unit/agent.test.ts` | nouveau, modif. | `PRIME_AGENT_KERNEL_PYTHON` retirée de l'environnement passé au binaire (avec elle, le binaire désactive les skills Python absents de ce Python, dont `edit`) ; Unharnessed ne la filtre pas (`vscode/src/agent.ts:96` à `157dcdc1d`) ; décision de Yusuf, sans réglage. Test : l'agent factice note les variables du kernel reçues. | YA-10 (ticket 24) |
| `extensions/vscode/webview/styles.css`, `src/ya/cells.ts`, `webview/ya-cells.tsx`, `webview/ya-probe.ts`, `src/protocol.ts`, `test/` | modif. | Contrôle de Yusuf : replis fermés masqués (le code repris les rend `inert` ; affichés, ils ignoraient les clics, cartes impossibles à déplier) ; en-tête avec la commande sortie en erreur ; séparateurs avec espaces dans la ligne d'un `bash()` ; sondes `userClick` (élément sous le pointeur) et `visibleInert`, e2e par clics d'utilisateur. | YA-10 (ticket 24) |
| `bench/tools/capture/acp_frames.py`, `compare_frames.py`, `README.md` | nouveaux, modif. | Trames ACP du vrai binaire (fonctions de `run.py`, sans le modifier) et comparaison champ par champ avec l'agent factice de l'extension ; section du README. | YA-10 (ticket 24) |
| `extensions/vscode/webview/ya-probe.ts`, `test/` | modif. | Sonde : nom de la zone de saisie ; tests unitaires (`pending` puis `running`, carte jamais lancée après Arrêter) et e2e (aucun « Unharnessed », panneau du kernel masqué, « Awaiting permission », « Not run ») ; l'agent factice laisse la cellule tourner 150 ms après `in_progress`. Étape 2 : `test/unit/cells.test.ts` (7 tests), scénarios `carte` et `refus` de l'agent factice (forme des trames du pont), test unitaire et e2e des cartes. Trames de l'agent factice alignées sur une capture du binaire de `main` (`ipython.diffCount`, `ya-permission-N`, fin de tour `responseBoundary`/`terminalQuiescence`, `messageId`). Étape 3 : `test/unit/permission-keys.test.ts`, scénario `accord` (attend la réponse de la boîte), tests unitaires et e2e de la boîte ; sonde `probeInput` (saisie, focus, touches, clic sur un bouton de la boîte ; focus et état des boutons rendus). Étape 4 : `test/unit/diffs.test.ts`, scénarios `diff` et `diffsed` (fichiers réels écrits par l'agent factice), tests unitaires et e2e des diffs. | YA-10 (ticket 24) |

## Tours lancés par l'agent : début et fin dans le pont ACP (YA-10, ticket 25)

Constat, décisions (T1 à T4, E1 à E8) et mesure : `docs/tickets/25-tours-lances-par-agent.md`. Trames ACP seulement : rien de ce qui part au modèle ne change (contrôle par capture `main` contre la branche, `compare.py --all`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-daemon/src/acp/ya_turns.rs` | nouveau | Un `agent_start` du worker hors prompt du pont ouvre un tour avec son identifiant (compteur de `begin_prompt`) ; `turnStart` (`origin`, `wake`, noms d'Unharnessed) publié au premier message du tour, qui en donne l'origine (`async_bash_completion` → `bash`, `heartbeat_prompt` → `heartbeat`, `goal_context` → `goal`, reprise après limite → `quota`, message utilisateur d'un autre client → `prompt`) ; à `agent_end`, `responseBoundary` puis l'enveloppe de fin (lue au démon, hors de la boucle d'événements), rien pour un tour arrêté ; un prompt du pont attend la fin d'un tel tour (T3) ; Stop pendant un tel tour : `abort_and_send_queued` (T4). Tests unitaires. | YA-10 (ticket 25) |
| `crates/pa-daemon/src/acp/daemon.rs` | modif. (fichier upstream, environ 45 lignes) | Champ `ya_turns` de `HostedSession` ; trois accroches dans la boucle d'événements (`open` avant l'identifiant de tour, `announce` avant la publication, `close` après) ; `turnStart` d'un prompt et attente d'un tour de l'agent avant `begin_prompt` ; variante `CancelOrder::AgentTurnStop` et son bras dans `arm_cancel_locked` ; `fetch_autonomous_status`, `fetch_rlm_children` et `quiescence_meta` en `pub(super)`. | YA-10 (ticket 25) |
| `crates/pa-daemon/src/acp/meta.rs` | modif. (fichier upstream, 3 lignes) | Champ `turn_start` de `PrimeAgentSessionMeta` (sinon `correlate` le perdrait en relisant le `_meta`). | YA-10 (ticket 25) |
| `crates/pa-daemon/src/acp/mod.rs` | modif. (2 lignes) | Déclaration du module. | YA-10 (ticket 25) |
| `crates/pa-cli/tests/acp_mode_e2e.rs` | modif. (test upstream, 3 assertions) | `turnStart` ouvre le tour du prompt : `acp_prompt_stream_completion_envelope_and_stop_reason_match_ts` attend `turnStart` (`origin: "prompt"`) en première trame, puis le chunk ; `acp_prompt_chunk_carries_the_assistant_message_id` attend `eventSequence` 2 pour le chunk ; `acp_daemon_attached_cancels_mid_turn` admet le `turnStart` publié avant l'annulation (course : il passe ou non selon que le cancel arrive avant ou après sa publication), toujours aucune frontière. | YA-10 (ticket 25) |
| `crates/pa-cli/tests/ya_acp_wake_turn_e2e.rs` | nouveau | Vrai binaire en ACP, provider factice, vrai kernel : `turnStart` de chaque tour, deux réveils avec deux identifiants fermés par leur frontière puis `terminalQuiescence`, sans réponse ; prompt envoyé pendant un réveil admis après sa frontière ; Stop pendant un réveil : pas de frontière pour lui, la suite de sa cellule ne tourne pas, l'avis resté en file réveille l'agent de nouveau. | YA-10 (ticket 25) |
| `extensions/vscode/src/ya/meta.ts` | modif. (fichier du fork) | Déduction du début de tour (PR I) et `turnEnd` retirés : le pont donne `turnStart`, `chat.ts` lit la frontière. | YA-10 (ticket 25) |
| `extensions/vscode/src/chat.ts`, `src/protocol.ts`, `webview/Chat.tsx` | modif. (repris d'Unharnessed) | Appel de `yaMeta` sans contexte, `turnStart` traité comme chez Unharnessed ; genre de réveil `heartbeat` (E2). | YA-10 (ticket 25) |
| `extensions/vscode/test/fake-agent/fake-agent.mjs`, `test/unit/agent.test.ts`, `test/e2e/suite.ts`, `test/e2e/suite-real.ts` | modif. | `turnStart` de chaque prompt ; scénarios `réveil` et `réveil-lent` ; tests : tour de l'agent ouvert par `turnStart` et fermé sur sa frontière, Stop pendant un réveil, cas e2e vrai binaire « tour lancé par l'agent » (venu de l'étape 6 de la PR 3 du ticket 24). | YA-10 (ticket 25) |
| `bench/tools/capture/wake_turns.py`, `server.py` (scénarios `wake-stop-avant`, `wake-stop-apres`), `compare_frames.py`, `README.md` | nouveau, modif. (outils du fork) | Mesure E6 au niveau du démon (événements du worker, file, préfixe des requêtes après le Stop) ; comparaison de `turnStart` et du tour de réveil avec l'agent factice. | YA-10 (ticket 25) |
| `.changes/ya-25-tours-reveil.md` | nouveau | Fragment de changelog. | YA-10 (ticket 25) |

Limites connues (ticket 25, §7) : un tour lancé par un autre client pendant qu'un prompt de l'extension attend son début serait pris pour ce prompt (`chat.ts`, `begin`) ; un avis arrivé après un Stop, file vide au Stop, attend le prompt suivant (E8) ; les genres `heartbeat`, `goal` et `quota` sont lus sur le premier message du tour, non mesurés sur le binaire.

## Colonne de départ dans le contenu `diff` du pont ACP (YA-10, ticket 26)

Constat, décisions (K1 a, K2 a, K3 a) et mesures : `docs/tickets/26-colonne-diff.md`. Données d'affichage seulement : le texte `text/plain` du MIME (`Edited <path>`) et le retour d'`edit()` ne changent pas, `details` ne part pas au provider ; rien de ce qui part au modèle ne change (contrôle par capture `main` contre la branche, `compare.py --all`).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `skills/edit/src/edit/__init__.py` | modif. (skill upstream déjà modifié, 6 lignes) | `start_column` dans le MIME diff : colonne d'`old_str` sur sa ligne, à partir de 0, en points de code (`match_index` moins le début de la ligne) ; création : 0. | YA-10 (ticket 26) |
| `crates/pa-core/src/kernel/shared.rs` | modif. (fichier upstream, 4 lignes) | Champ `start_column: Option<u64>` de `KernelDiffDisplay`, lu par `parse_diff_display` quand il est là (un kernel plus ancien ne l'envoie pas). | YA-10 (ticket 26) |
| `crates/pa-core/src/tools/ipython.rs` | modif. (fichier upstream, environ 6 lignes) | `startColumn` dans `details.diffs`, seulement quand le kernel l'a envoyé (pas de valeur par défaut). Les `details` ne sont pas envoyés au modèle. | YA-10 (ticket 26) |
| `crates/pa-core/src/tools/golden_replay.rs` | modif. (test upstream) | `start_column` dans le `KernelDiffDisplay` du test `ipython_details_carry_the_cell_edit_diffs` et dans son assertion exacte ; test `diff_display_reads_the_start_column_when_present`. | YA-10 (ticket 26) |
| `crates/pa-daemon/src/acp/types.rs` | modif. (fichier upstream, commentaire) | Doc du contenu `diff` : `_yus/startColumn`. | YA-10 (ticket 26) |
| `crates/pa-daemon/src/acp/ya_cell_display.rs` | modif. (fichier du fork) | `_meta._yus/startColumn` à côté de `_yus/startLine`, chacun quand il est là ; test avec et sans colonne. | YA-10 (ticket 26) |
| `prime-agent-runtime/test/test_ya_edit.py` | modif. (test du fork) | Colonne émise : suppression pure après un emoji (colonne 4 en points de code), remplacement, création. | YA-10 (ticket 26) |
| `extensions/vscode/src/ya/cells.ts`, `src/ya/diffs.ts` | modif. (fichiers du fork) | Lecture de `_yus/startColumn` ; avec la colonne, une seule place essayée, convertie en unités UTF-16 sur la ligne, toujours vérifiée (`new_str` à cette place, `old_str` une seule fois) ; sans colonne, C14 (a). | YA-10 (ticket 26) |
| `extensions/vscode/test/unit/cells.test.ts`, `test/unit/diffs.test.ts`, `test/fake-agent/fake-agent.mjs`, `test/e2e/suite-real.ts` | modif. | Tests : emoji avant une suppression pure, colonne fausse sur un remplacement refusée, colonne invalide ignorée ; l'agent factice envoie `_yus/startColumn` ; e2e vrai binaire : suppression pure après un emoji, « Revert » par un clic, fichier revenu octet pour octet. | YA-10 (ticket 26) |
| `bench/tools/capture/compare_frames.py`, `display_check.py`, `README.md` | modif. (outils du fork) | `--against <main>` et `--expect <chemin>` : deux binaires trame par trame, écarts attendus listés, échec si l'un manque ; `display_check.py` vérifie `startColumn`. | YA-10 (ticket 26) |
| `.changes/ya-26-colonne-diff.md` | nouveau | Fragment de changelog. | YA-10 (ticket 26) |

## Reprise et historique dans le pont ACP, PR R1 (YA-10, ticket 28)

Constat, décisions (C1 à C13, 22/11/2026) et mesures : `docs/tickets/28-reprise-historique.md`. Trames ACP et journal d'usage seulement : rien de ce qui part au modèle ne change pour une session neuve ni pour une reprise sans changement (cas de capture `acp-resume`, `compare.py --all` `main` contre la branche).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `crates/pa-daemon/src/acp/ya_history.rs`, `ya_history_tests.rs` | nouveau | `saved { sessionId, path }` dans les réponses de `session/new` et `session/load` (C2) ; `session/load` : refus d'un autre identifiant que celui du fichier ouvert par `--resume`, refus si un autre client qui répond aux permissions tient la session (C12, après 3 s d'attente pour un client qui vient de fermer) ; rejeu du transcript (`get_session_messages`) tour par tour par le mappeur du direct (C6) : `turnStart`, blocs du prompt (`user_message_chunk`), texte, raisonnement, cellules et résultats, compaction, `turnEnd` (`done`, `cancelled`, `failed`) ; un tour que le worker fait encore reste ouvert (C9) ; `session/list` (`list_saved_sessions` du dossier, sans sous-agents ni brouillons, titre = nom ou début du premier prompt, 80 caractères, plus récent d'abord, `path` et `current` dans le `_meta` du pont, C10, C11) ; `_unharnessed/sessions/delete` : refus pour la session du pont et pour une session ouverte dans un autre client, sinon `kill` de la session vivante, attente de sa sortie, puis `delete_saved_session` (fichier et `session-artifacts/<id>`), journal d'usage gardé (C3, C4). Tests unitaires. | YA-10 (ticket 28) |
| `crates/pa-daemon/src/acp/daemon.rs` | modif. (fichier upstream, environ 70 lignes) | Champ `file` de `DaemonBinding` (lu dans le résumé du `create`) ; aiguillage de `session/load`, `session/list` et de la suppression ; `session/load` réglé avant la trame suivante comme `session/new` ; argument `load` de `handle_session_new` (`#[allow(clippy::too_many_arguments)]`) : identifiant ACP du fichier (C1), rejeu avant l'installation de la session et publication des mises à jour retenues avant la réponse ; `saved` dans la réponse. | YA-10 (ticket 28) |
| `crates/pa-daemon/src/acp/types.rs` | modif. (fichier upstream, environ 15 lignes) | `loadSession: true`, `sessionCapabilities.list` ; variante `UserMessageChunk` d'`AcpSessionUpdate` ; test `initialize_result_matches_the_served_surface`. | YA-10 (ticket 28) |
| `crates/pa-daemon/src/acp/meta.rs` | modif. (fichier upstream, 6 lignes) | Champs `turn_end` et `saved` de `PrimeAgentSessionMeta` (sinon `correlate` perdrait `turnEnd`). | YA-10 (ticket 28) |
| `crates/pa-daemon/src/acp/mod.rs` | modif. (2 lignes) | Déclaration du module. | YA-10 (ticket 28) |
| `crates/pa-daemon/src/acp/ya_turns.rs` | modif. (fichier du fork) | `AgentTurns::adopt` (tour en cours au chargement, C9) ; `TurnStartMeta::new` et `origin_of` en `pub(crate)` pour le rejeu. | YA-10 (ticket 28) |
| `crates/pa-daemon/src/ya_permissions.rs` | modif. (fichier du fork) | Action `others` de `ya_permissions` : nombre de clients attachés qui répondent, hors le client de la commande (C12). | YA-10 (ticket 28) |
| `crates/pa-core/src/session_engine/engine.rs` | modif. (fichier upstream, lignes du fork T04, 5 lignes) | `UsageJournal::for_session` reçoit `conversation_log_path` : le moteur d'une session du démon, en mémoire, journalise à côté du fichier durable du worker (J1 ; avant, aucune session ACP ou TUI n'avait de journal). | YA-10 (ticket 28) |
| `crates/pa-core/src/session_engine/usage_log.rs`, `usage_log/tests.rs` | modif. (fichiers du fork) | `for_session` : repli sur le fichier durable d'une session en mémoire (J1). Champ `prefix` (empreintes du prompt système et des tools) des appels de la boucle ; après une reprise, le premier appel compare au dernier `prefix` journalisé et pose une seule `invalidation` `resume_prefix_change` (`parts`) s'il diffère (C8). Tests. Format décrit dans `docs/usage.md`. | YA-10 (ticket 28) |
| `crates/pa-cli/tests/acp_mode_e2e.rs` | modif. (test upstream, 2 assertions) | `acp_initialize_matches_the_ts_golden` : `loadSession: true` et `sessionCapabilities` `{ close, list }`. | YA-10 (ticket 28) |
| `crates/pa-cli/tests/ya_acp_resume_e2e.rs` | nouveau | Vrai binaire en ACP, provider factice, vrai kernel, démon partagé par les clients d'un test : `saved` et fichier présent dès `session/new` ; reprise dans un autre processus (`--resume`) : autre identifiant refusé, rejeu avant la réponse (`turnStart`, prompt, cellule, résultat, réponse, `turnEnd`), prompt suivant ; `session/list` (brouillon absent, `current`, autre dossier vide) ; suppression refusée pour la session courante et une session tenue par un autre client, sinon fichier et artefacts supprimés, journal gardé ; `session/load` refusé pour une conversation tenue ailleurs ; journal d'usage d'une session ACP et changement d'effort journalisé (J1). | YA-10 (ticket 28) |
| `bench/tools/capture/acp_resume.py`, `resume_probe.py`, `run.py`, `compare_frames.py`, `README.md` | nouveau, modif. (outils du fork) | Cas `acp-resume` (E, C, D, F, G, trames, journal d'usage des sessions ACP ; le run échoue s'il est `NON CONFORME`) ; mesures d'avant R1 (M1 à M3) ; `compare_frames.py --expect` accepte un objet comme écart attendu. | YA-10 (ticket 28) |
| `.changes/ya-28-reprise.md` | nouveau | Fragment de changelog. | YA-10 (ticket 28) |

Limites connues : le rejeu ne montre ni la durée du raisonnement ni les avis (`[bash-done]`, `[python-state-restored]`), comme Unharnessed ; un tour coupé puis suivi d'un prompt est montré `done` (comme un message envoyé pendant le tour) ; l'extension ne sert ces méthodes qu'avec la PR X1.

## Code repris d'Unharnessed

Unharnessed (`https://github.com/unharnessed-agent/Unharnessed`, lu au commit `fe2cf2815f78463464b91c598fc0c31c0af98f4c` du 15/11/2026) est un fork de Prime Agent, sous licence MIT. Le réglage `bashWaitSeconds` est propre à yusAgent, et `bench/smoke/ab/ya09-bash-count.py` ne fait que lire leurs fichiers (prompt golden, description du tool) pour les compter. Les reprises du lot Unharnessed (YA-09, ticket 11) sont des reports à la main, sans cherry-pick (leurs crates sont renommées `uh-*`). Tout fichier ou extrait repris plus tard est listé dans le tableau ci-dessous, avec le commit source, et garde la notice suivante (fichier `LICENSE` d'Unharnessed au commit cité) :

```text
The MIT License (MIT)

Copyright (c) 2026 Unharnessed
Copyright (c) 2025-2026 Prime Intellect Ltd.
Portions Copyright (c) 2025 Mario Zechner

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

| Fichier yusAgent | Source Unharnessed (commit, fichier) | Ticket |
|---|---|---|
| `prime-agent-runtime/src/rlm/ya_timeout.py`, `prime-agent-runtime/src/rlm/bash.py` (paramètre `timeout`, `timeoutSeconds`) | `569284b36`, `unharnessed-runtime/src/rlm/bash.py` (`_DEFAULT_TIMEOUT`, `_arm_timeout`, `_time_out`, validation de `timeout`) ; réécrit dans un module séparé | YA-09 |
| `crates/pa-core/src/session_engine/ya_bash_timeout.rs` | `569284b36`, `crates/uh-core/src/session_engine/messages.rs` (`mark_async_bash_timeout`) et `wakes.rs` (contrôle de `timeoutSeconds`) ; texte de l'avis repris, code réécrit | YA-09 |
| `crates/pa-agent/src/types.rs`, `crates/pa-agent/src/agent_loop/tool_call.rs`, `crates/pa-core/src/session_engine/tool_bridge.rs` (drapeau `is_error`) | `43f131ac6` (lu à `53bdd8784`), `crates/uh-agent/src/types.rs`, `crates/uh-agent/src/agent_loop/tool_call.rs`, `crates/uh-core/src/session_engine/tool_bridge.rs` ; forme reprise telle quelle | YA-09 |
| `crates/pa-core/src/prompts/ya_prompt_v2/core.md` | `fe2cf2815`, `crates/uh-core/src/prompts/layers/core.md` (structure, phrases de l'intro, de `BashHandle`/`BashResult` et des règles) ; adapté à notre API et complété (ticket 11, §3) | YA-09 |
| `crates/pa-core/src/session_engine/ya_bash_notice_batch.rs` | `7dc73199e`, `crates/uh-core/src/session_engine/wakes.rs` (`NOTICE_BATCH_MS`, avis groupés à la fin de la dernière commande) ; règle reprise, code réécrit (écart : avis pendant une exécution gardés) | YA-09 |
| `extensions/vscode/src/*.ts` (sauf `binary.ts`, `acp/`, `ya/`), `extensions/vscode/webview/*.tsx`, `*.ts` (sauf `ya-*`) | `157dcdc1d`, `vscode/src/`, `vscode/webview/` ; importés tels quels (premier commit de la PR I), puis adaptés (adaptations marquées « yusAgent »), fichiers visuels remplacés par des bouchons ; `esbuild.mjs` repris en partie | YA-10 |
