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
- Rejeu des appels libres (ticket `ya-03-suite`, après la chauffe du 14/11/2026) : la marque `yaFreeformItemId` de `ToolCall.rest` se perdait dans la boucle de l'agent (`pa_agent::types::ToolCall` sans `rest`) ; un appel est désormais libre quand le tool qu'il nomme est déclaré libre dans les tools de la requête (`pa_ai::ya_freeform::FreeformReplay`). Défaut upstream des signatures de texte et de raisonnement (suite WebSocket cassée après un message avec texte) : points ouverts du ticket 03 (YA-04).
