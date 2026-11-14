# Ticket 09 : YA-03 de base (edit natif, read numéroté, plafond de sortie, redémarrage du kernel, variante bash natif)

Ticket CDC : YA-03 (avec YA-14 pour la variante, YA-17 pour les formats natifs). Base : `main` à `c105243` (merge de #23). Plan validé par Yusuf le 13/11/2026.

## Périmètre

| Point | Livré dans | Réglage |
|---|---|---|
| Tool `apply_patch` au format libre de Codex (famille Codex seulement) | PR 1 (moteur, providers), PR 2 (tool enregistré, prompt) | `editTool` : `"native"` (défaut) ou `"kernel"` |
| `read()` numéroté dans le kernel | PR 1 (fonction), PR 2 (prompt) | aucun |
| Plafond de sortie des cellules, sortie complète en fichier | PR 1 | `kernel.maxOutputChars`, défaut 40 000 |
| Avis de redémarrage du kernel en fin de résultat de la cellule plantée | PR 1 | aucun |
| `bash(env=)` | PR 1 (fonction), PR 2 (prompt) | aucun |
| Variante bash natif | PR 2 | `shellTool` : `"kernel"` (défaut) ou `"native"` |

Hors périmètre, groupés avec YA-09 : `timeout=` et `cwd=` de `bash()`, shell principal persistant. Pas de tool d'edit natif hors famille Codex (écart YA-03, journal du CDC).

**Rupture de préfixe** : une seule, dans la PR 2. La PR 1 ne change ni le prompt système ni les définitions de tools (prouvé au banc de capture contre `main`) ; elle ne change que le contenu de certains résultats de tools (sortie au-delà du plafond, kernel planté), qui sont en fin de contexte.

## Sources (openai/codex au SHA `69231e119bfca0b14453515e97d99688ce1c47c9`, main du 13/11/2026)

- Grammaire : `codex-rs/core/assets/tools/apply_patch.lark`.
- Tool : `codex-rs/core/src/tools/handlers/apply_patch_spec.rs:9-28`, `create_apply_patch_freeform_tool`. Nom `apply_patch`, description « The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON. », `format: {type: "grammar", syntax: "lark", definition: <grammaire>}`.
- Forme sur le fil : `codex-rs/tools/src/tool_spec.rs:20-55` (`#[serde(tag = "type")]`, variante `custom`), `codex-rs/tools/src/responses_api.rs:17-30` (`FreeformTool`, `FreeformToolFormat`).
- Items : `codex-rs/protocol/src/models.rs:1282-1319` : `custom_tool_call` (`call_id`, `name`, `input` en texte) et `custom_tool_call_output` (`call_id`, `output` au même encodage que `function_call_output.output`).
- Événements : `codex-rs/codex-api/src/sse/responses.rs:411-568` : `response.output_item.added` / `.done` portent l'item, `response.custom_tool_call_input.delta` le flux de l'entrée.
- Config par modèle : `codex-rs/models-manager/models.json`. Pour `gpt-6.1-terra` (et les 11 autres modèles du fichier) : `apply_patch_tool_type: "freeform"`, `shell_type: "shell_command"`, `truncation_policy: {mode: "tokens", limit: 10000}`. L'enum `ApplyPatchToolType` n'a plus que `Freeform` (`codex-rs/protocol/src/openai_models.rs:338-340`) : il n'existe plus de variante JSON.
- Shell : `shell_command` est un alias de `UnifiedExec` (`codex-rs/protocol/src/openai_models.rs:330-334`), c'est-à-dire les tools `exec_command` et `write_stdin` (`codex-rs/core/src/tools/handlers/shell_spec.rs:14-152`). Source du tool `bash` de la variante (PR 2). Le sous-ensemble repris (PTY, `write_stdin`, sessions) est proposé à Yusuf avant d'être codé.
- Guide de prompt Codex (developers.openai.com, `cookbook/examples/gpt-5/codex_prompting_guide`) : « We strongly recommend using our exact `apply_patch` implementation as the model has been trained to excel at this diff format. »
- Plafond : 40 000 caractères ≈ 10 000 tokens, la troncature de Codex pour `gpt-6.1-terra` (`models.json` ci-dessus). Le rapport de 4 caractères par token est une **estimation** ; le ratio réel est mesuré au tiktoken (`o200k_base`) sur les sorties du banc.

## Analyse et application d'`apply_patch`

Crate séparé `crates/ya-apply-patch`, repris de `codex-rs/apply-patch` au SHA ci-dessus (Apache-2.0, `LICENSE` et `NOTICE` conservés). Les fonctions qui calculent le nouveau contenu (analyse, application des hunks, `seek_sequence`, correspondance tolérante) sont le code de Codex à l'identique ; seules les lectures et écritures disque passent de `ExecutorFileSystem` à `std::fs`. Retirés : `invocation.rs` (tree-sitter, détection d'`apply_patch` dans une commande shell) et le contexte de sandbox de `codex-exec-server`. Les tests du crate Codex (analyse et application) sont repris tels quels, adaptés seulement pour `PathBuf` et `std::fs` : ils prouvent que le comportement est identique. Écarts détaillés dans `crates/ya-apply-patch/README.md` et `docs/fork-changes.md`.

## Providers

`openai_responses_shared.rs` et `openai_responses_stream.rs` (communs aux API `openai-responses`, `openai-codex-responses`, `azure-openai-responses`, et aux deux transports de Codex, SSE et WebSocket, qui passent par `ResponsesStreamProcessor::handle_event`) :

- un tool marqué libre est envoyé en `{"type": "custom", "name", "description", "format"}` ;
- un `custom_tool_call` reçu devient un appel de tool dont l'argument `input` est le texte brut, marqué pour être renvoyé en `custom_tool_call` dans l'historique, avec son résultat en `custom_tool_call_output`.

Les autres providers ne voient jamais de tool libre (le tool n'est enregistré que pour ces API) ; un changement de modèle vers une autre famille en cours de session est refusé avec un message (PR 2).

## Plafond de sortie

`kernel.maxOutputChars` (défaut 40 000) plafonne le texte d'une cellule (stdout, stderr, résultat, traceback), au lieu des 65 536 caractères par flux d'upstream (`crates/pa-core/src/kernel/shared.rs:14`). Au-delà : début et fin de la sortie dans le budget, la sortie complète écrite dans un fichier de la session, et une ligne qui donne la taille et le chemin.

## Redémarrage du kernel

Restauration upstream conservée (namespace relu depuis le snapshot de la dernière cellule réussie, `crates/pa-core/src/kernel/provisioner.rs:1160-1168`). Quand le kernel meurt pendant une cellule, le résultat de cette cellule se termine par un avis générique : kernel redémarré, état restauré depuis la dernière cellule réussie, cette cellule n'a pas été relancée. Pas de liste de noms. Aucune cellule rejouée. Écart avec la décision du 28/10 (« variables perdues ») porté au journal du CDC.

## Tests

- `ya-apply-patch` : tests de Codex repris.
- Providers : sérialisation du tool `custom`, `custom_tool_call` lu en SSE et en WebSocket (événements `added`, `custom_tool_call_input.delta`, `done`), aller-retour dans l'historique.
- Kernel : sortie au-delà du plafond (début, fin, chemin, fichier complet) ; cellule qui tue le kernel (avis en fin de résultat, redémarrage, cellule à effet de bord non rejouée, durée < 5 s).
- Runtime Python, en local : `read()` (numérotation, `offset`, `limit`), `bash(env=)`.
- PR 2 : `prefix_stability` et contrôles de capture pour le défaut, `editTool: "kernel"` et `shellTool: "native"`.

## Mesures

- PR 1 : banc de capture, requêtes identiques à `main` (prompt, tools, entrées). Merge sur CI verte, sans smoke.
- PR 2 : banc de capture (un seul écart attendu par bras, comparaison des requêtes de l'agent, pas de la sonde qui ne lit pas les réglages du bras), puis :
  - run de chauffe d'une tâche sur le défaut ;
  - A/B 1 : `editTool` `native` contre `kernel` (`shellTool` `kernel`) ; le bras `kernel` est aussi comparé à la baseline avec `compare.py` (effet de la PR 1, non entrelacé, filet de régression, pas un verdict) ;
  - A/B 2 : `shellTool` `kernel` contre `native`, sur l'`editTool` retenu.

Les deux A/B : `gpt-6.1-terra`, effort high, 10 tâches, 3 passes entrelacées par tâche, graine tirée et notée, `--idle-timeout 300`, lancés par Yusuf.

### Règle de verdict de l'A/B 2 (validée le 13/11/2026)

Verdict par défaut : kernel (décision du 28/10). Le bras bash natif est retenu seulement si :
1. il résout au moins 3 tâches de plus sur les paires valides (seuil fixé avant le run, pas un test statistique), ou
2. il coûte moins en coût à cache théorique des tâches stables, avec un IC bootstrap sur les tâches qui exclut zéro, et il ne résout pas 3 tâches ou plus de moins.

Le rapport dit que la résolution ne se tranche pas à 10 tâches. La condition 2 est un écart avec YA-14 (qui ne prévoit que la résolution), porté au journal du CDC. Verdict provisoire : une seule famille de modèles.

### Règle de verdict de l'A/B 1

À proposer avant le run, sur le modèle de la règle `rlm`, charge de la preuve sur le bras `kernel`.

### Métriques du banc

`bench/smoke/metrics.py` doit compter les cellules en erreur d'appel et les edits appliqués au premier essai ; ajoutés au banc s'ils n'y sont pas (PR 2).

## Points ouverts

- `crates/pa-core/src/prompts/layers/bash-over-subprocess.md` (règle `glm-5.3*` de `model_prompts.toml`) parle de `bash()` ; laissé tel quel (décision du 13/11/2026). Sans effet sur la variante `shellTool: "native"`, réservée à la famille Codex.
- Réglages `editTool` et `shellTool` : figés dans l'en-tête du fichier de session (champ `yaTools`) depuis la correction 1 de la PR #26 ; à la reprise, les tools de la session sont gardés et un avertissement signale des réglages différents. Forks et session matérialisée reprennent le champ de la source ; le démon (mode ACP, TUI) l'écrit dans le fichier durable du worker et le relit à chaque build (dernier tour de la PR #26). Reste ouvert : une grosse session fenêtrée créée avant ce champ ne le garde qu'en mémoire (`docs/fork-changes.md`, « Tools natifs (YA-03, PR 2) »).
- `scripts/ci_pr_crates.py` ne connaît pas `ya-apply-patch` (dont dépend désormais `pa-core`) : une PR qui le touche lance la sélection complète (sans danger).
- **`isError` toujours à `false` sur les résultats d'outil (constat de l'A/B 1, 14/11/2026, lecture seule, pas de correctif).**
  - *Origine : l'amont, pas nos PR.*
    - Les deux outils calculent bien l'échec :
      - `python_repl` : `is_error` vaut vrai sur `Error` ou `Aborted` (`crates/pa-core/src/tools/ipython.rs:511`), comme en amont (`c83ac68b3:crates/pa-core/src/tools/ipython.rs:505`) ;
      - `apply_patch` : `is_error: !outcome.applied` (`crates/pa-core/src/tools/ya_apply_patch.rs:69`).
    - Le pont vers la boucle d'agent jette cette valeur (`crates/pa-core/src/session_engine/tool_bridge.rs:112-113`, `let _ = is_error;`). Ces lignes sont identiques à `c83ac68b3`, et aucun commit du fork ne touche ce fichier.
    - `AgentToolResult` n'a pas de champ `is_error` (`crates/pa-agent/src/types.rs:479-486`).
    - La boucle met `is_error: false` sur tout résultat `Ok` (`crates/pa-agent/src/agent_loop/tool_call.rs:196-199`). Elle ne met `true` que sur une erreur Rust, un abandon ou un échec du flux de mises à jour (`tool_call.rs:191-194`, `:214-217`).
    - Le crochet `after_tool_call`, qui pourrait corriger la valeur, n'est pas branché dans pa-core.
  - *Ce que voit le modèle : l'échec, dans le texte.*
    - `python_repl` : stdout, stderr puis le traceback (`crates/pa-core/src/tools/ipython.rs:259-263`).
    - `apply_patch` : `apply_patch verification failed: …` ou `Exit code: 1 …` (`ya_apply_patch.rs:100-104`, `:125-129`).
  - *Ce que voit l'ACP : `Completed`.* Le statut de l'appel ne dépend que de `isError` (`crates/pa-daemon/src/acp/wire_events.rs:169-181`, valeur reçue de `ToolExecutionEnd`, `crates/pa-daemon/src/agent_engine/turn/run_once.rs:312-318`). Une cellule en erreur ou un patch refusé s'affiche donc comme réussi dans l'extension.
  - *Providers qui portent le champ dans la requête* :
    - Anthropic : `is_error` (`crates/pa-ai/src/providers/anthropic/convert.rs:229`) ;
    - Google : `error` au lieu de `output` (`crates/pa-ai/src/providers/google_shared.rs:345-349`) ;
    - Bedrock : `status` (`crates/pa-ai/src/providers/bedrock/convert.rs:306`) ;
    - Mistral : préfixe `[tool error] ` (`crates/pa-ai/src/providers/mistral/convert.rs:276`) ;
    - aucun champ chez OpenAI Responses et Codex : `function_call_output` et `custom_tool_call_output` ne portent que `call_id` et `output` (`crates/pa-ai/src/providers/openai_responses_shared.rs:357-361`, `crates/pa-ai/src/providers/ya_freeform.rs:193-198`). Chat Completions n'en a pas non plus.
  - *Effet d'un correctif.*
    - Préfixe : inchangé, car `isError` ne figure ni dans le prompt système ni dans les définitions de tools. C'est une déduction.
    - Requêtes : les résultats en échec écrits après le correctif changeraient d'octets chez Anthropic, Google, Bedrock et Mistral, mais pas chez Codex et Responses. Les sessions existantes gardent `false`, donc leur historique ne change pas (déduction).
    - Autres lecteurs de `isError`, qui changeraient de comportement :
      - statut ACP ;
      - télémétrie (`crates/pa-core/src/session_engine/telemetry.rs:897-914`) ;
      - résumé de compaction (`crates/pa-core/src/session_engine/compaction_utils.rs:273-277`) ;
      - cartes de la TUI et export HTML.
    - Le masquage lit déjà `details.status` en premier (`crates/pa-core/src/session_engine/masking.rs:316`).
    - Un correctif toucherait des fichiers d'amont (`tool_bridge.rs`, `AgentToolResult`), à lister dans `docs/fork-changes.md`.
  - *Côté banc* : `bench/smoke/metrics.py` ne lit plus `isError` pour les échecs (commit `fe3dbfa25`).
- Rejeu des appels libres (ticket `ya-03-suite`, après la chauffe du 14/11/2026) : la marque `yaFreeformItemId` de `ToolCall.rest` se perdait dans la boucle de l'agent (`pa_agent::types::ToolCall` sans `rest`) ; un appel est désormais libre quand le tool qu'il nomme est déclaré libre dans les tools de la requête (`pa_ai::ya_freeform::FreeformReplay`). Défaut upstream des signatures de texte et de raisonnement (suite WebSocket cassée après un message avec texte) : points ouverts du ticket 03 (YA-04).
- **Tâches non stables du banc : l'agent réécrit un test existant (constat des A/B 1 et 2, 14/11/2026).** `node-filesize-376` et `node-little-coder-102` échouent pour la même raison dans les 22 bras en échec des deux A/B, toutes configurations confondues. Chaque fois, un seul test échoue. Il passait déjà avant le correctif (`pass_to_pass`), et l'agent a réécrit ou retiré le test visible qui l'exprime. Les énoncés restent tels quels.
  - `node-filesize-376` : le test est `should retain zero precision when a locale is requested` (`bench/smoke/tasks/node-filesize-376/task.toml:155`). L'agent change l'assertion visible `filesize(0, { locale: "de-DE", precision: 3 })` de `"0.00 B"` en `"0,00 B"`. L'énoncé dit seulement « Une précision qui produit zéro garde ses zéros (par exemple `0.00`) ».
  - `node-little-coder-102` : le test est `parseTextToolCalls > does not extract from nested-object bare JSON (matches Python behavior)` (`bench/smoke/tasks/node-little-coder-102/task.toml:59`). L'agent étend `firstBalancedObject` à l'extraction du JSON nu (`bareRe`), puis réécrit ou retire ce test. Le point 3 de l'énoncé ne vise que la réparation (`repairJson`).
  - **Contre-exemples** (A/B 2, `bench/smoke/runs/ab-20261114T140538Z`, résolus, aucun test en échec) :
    - `node-filesize-376`, passe 2, bras A (`20261114T143500Z-80e20ea-gpt-6.1-terra`) : l'assertion de zéro est gardée (« J'ai conservé le comportement existant pour une entrée nulle avec précision ») ;
    - `node-little-coder-102`, passe 1, bras B (`20261114T140920Z-80e20ea-gpt-6.1-terra`) : `firstBalancedObject` ne sert qu'à `repairJson`, et l'extraction du JSON nu et son test ne sont pas touchés.
  - Recompté par `bench/smoke/ab/ya03-ab2-20261114-mesures.py`, section « Tâches non stables ». C'est un constat sur la consigne de l'agent (ne pas réécrire un test qui passait), sans action dans ce ticket.
