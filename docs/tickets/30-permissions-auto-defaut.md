# Ticket 30 : permissions en « auto » par défaut (constat et plan)

Demande de Yusuf du 23/11/2026, sur sa décision du 22/11/2026 : **« auto » par défaut**, l'utilisateur passe en manuel s'il le souhaite. Remplace « manuel par défaut » et D5 (testeurs en manuel) du ticket 20 (`docs/tickets/20-ya11-permissions.md:20-23`, `:16` ; journal du CDC, `docs/yusAgent-cdc.md:91`, daté du 20/11 : aucune entrée du 18/11 sur les permissions dans le journal).

Branche `ya-30-auto-defaut`, depuis `main` à `60b05a595`. Sections 1 à 7 : constat et plan du 23/11/2026, écrits avant tout code ; les décisions ci-dessous les remplacent quand elles diffèrent.

## Décisions de Yusuf sur le constat (23/11/2026)

- **C1** : option **B**. Étiquette discrète comme celle du TUI, **non cliquable** dans ce ticket.
- **C2** : oui, l'avertissement sur stderr s'imprime aussi quand « auto » vient du défaut.
- **C3** : `PRIME_AGENT_TEST_PERMISSIONS` gardée (`.cargo/config.toml`, listes blanches de `scripts/`, `ci.yml`).
- **C4** : non. D1 inchangé : le réglage n'est lu que dans les réglages globaux. **À rouvrir avant publication.**
- **C5** : message D6 actuel, en retirant seulement « (the default) ».
- **C6** : sélecteur `permissions` dans l'extension plus tard, avec X3.
- **C7** : oui, l'agent factice de l'extension passe en `auto` par défaut.
- **C8** : oui, le banc continue d'écrire `"auto"`.
- **C9** : ligne au journal du CDC (décision du 22/11, remplace celle notée au 20/11) et mise à jour des écarts à `docs/yusAgent-cdc.md:409` et `:492`. **Point ouvert, pas une décision** : le défaut du build publié est à rouvrir avec le sandbox (seconde partie de YA-11).

Déroulé demandé : une seule PR qui touche `extensions/vscode/` (job `real-binary` lancé). Push 1 : tests seuls, rouges sur leurs assertions, seuls échecs du run. Push 2 : nos tests existants en manuel explicite, cœur, extension, agent factice, doc. Contrôle de préfixe et script de contrôle lancés par Claude Code sur le binaire de la branche avant la remise.

## 1. Où le défaut est lu et fixé

**Un seul point fixe le défaut** : `mode_from`, réglage absent → `(PermissionMode::Manual, ModeSource::Default)` (`crates/pa-core/src/session_engine/ya_permissions.rs:269`). Tous les chemins y passent.

| Chemin | Lecture | Où |
|---|---|---|
| Réglages globaux (D1) | `"permissions"` lu dans le seul document brut global (`SettingsManager::global_raw`, `crates/pa-core/src/settings/manager.rs:113`), jamais dans `<cwd>/.yusagent/` | `ya_permissions.rs:253-278` |
| Surcharge de test | `PRIME_AGENT_TEST_PERMISSIONS`, builds de debug seulement (`cfg!(debug_assertions)`), prioritaire sur le réglage | `ya_permissions.rs:58`, `:231-237`, `:263-267` |
| Création de toute session (`-p`, RPC, worker du démon) | `resolve` : choix de l'embedding (`wiring.mode`, sélecteur) sinon `mode_from` ; refus D6 si manuel sans demandeur ; avertissement D4 | appel `crates/pa-core/src/session_engine/engine.rs:241-246`, corps `ya_permissions.rs:329-358` |
| `-p` et RPC (D6) | sessions sans démon (`daemonless: true`, `crates/pa-cli/src/print_runtime.rs:853`) et sans demandeur : refus au démarrage en manuel | `ya_permissions.rs:347-349` |
| Démon, état d'une session qui sait demander (TUI, ACP) | `mode()` : choix de l'utilisateur, sinon `global_mode` → `configured_mode` → `mode_from` | `crates/pa-daemon/src/ya_permissions.rs:171-179`, `:255-260` ; état `yaPermissions` dans `get_connection_state` `:412-419` |
| ACP | sélecteur `permissions` construit depuis cet état, `currentValue` = mode du worker | `crates/pa-daemon/src/acp/ya_permissions.rs:332-351` ; écriture `apply_mode` `:369-380` |
| TUI | étiquette depuis le snapshot d'attach (`state_is_auto`) puis `ya_permission_mode` et `/permissions` | `crates/pa-tui/src/session_ui/ya_permissions.rs:38-45`, `:162-167` ; `crates/pa-tui/src/session_ui/lifecycle.rs:644-645` |
| Extension | **aucun défaut propre** : lit `currentValue` de l'option `permissions` du pont | `extensions/vscode/src/session.ts:168-174`, `extensions/vscode/src/protocol.ts:126-129` |

Constats annexes :

- **L'extension n'a pas de sélecteur `permissions`** : seuls `model`, `thought_level` et `fast_mode` sont modifiables (`extensions/vscode/src/session.ts:211-219` ; `extensions/vscode/webview/Composer.tsx:1009`, `:1034` ; `extensions/vscode/webview/Effort.tsx:107`). Avec « auto » par défaut, passer en manuel depuis l'extension n'est possible que par `~/.yusagent/settings.json` (pour toutes les sessions). Le TUI a `/permissions` (`crates/pa-types/src/slash_commands.rs:47`).
- Textes qui disent « manuel par défaut » : doc du module `ya_permissions.rs:6`, `:75`, `:101` ; message D6 `:287` ; `.cargo/config.toml:14` ; `.changes/ya-11-permissions.md:1` ; `bench/smoke/run.py:320` ; `bench/tools/ya_permissions_acp_demo.py:6` ; `bench/tools/ya_permissions_tui_demo.py:6` ; `docs/fork-changes.md:1054` ; commentaires de tests (section 2).
- CDC : le passage à « auto » par défaut s'écarte aussi de « Réglages par défaut sûrs pour tous : auto-approve désactivé » (`docs/yusAgent-cdc.md:409`) et de « Build publié : … auto-approve désactivé par défaut » (YA-16, `docs/yusAgent-cdc.md:492`). La gate de publication (sandbox obligatoire avant toute distribution, `:490`) ne change pas.

## 2. Tests, agent factice et scripts qui supposent le manuel par défaut

### `.cargo/config.toml`

`PRIME_AGENT_TEST_PERMISSIONS = "auto"` (`.cargo/config.toml:14-18`) met les tests upstream en « auto » sous `cargo` ; repris hors de `cargo` par les contrôles du cycle de vie (`scripts/lifecycle_lib.py:49` et 7 scripts `scripts/check_*.py`, liste blanche) et par `ci.yml` (`.github/workflows/ci.yml:377`). Nos tests le **retirent** (`env_remove(TEST_ENV)`) pour tomber sur le défaut, donc sur le manuel.

Une fois l'auto par défaut :

- **Pour les tests upstream, la variable ne change plus le mode** : sans elle, ils tournent aussi en « auto » (le défaut).
- **Elle garde un effet : l'avertissement.** La source `TestOverride` n'imprime rien (`ya_permissions.rs:350`). Si l'avertissement D4 s'imprime aussi pour la source `Default` (section 4), les tests upstream sans la variable le recevraient sur stderr en `-p`. Deux fichiers de tests vérifient un stderr vide (`git grep -l -E "stderr(\.trim\(\))?\.is_empty\(\)" -- 'crates/*/tests/*'` : `crates/pa-cli/tests/eval_composition_e2e.rs`, `crates/pa-cli/tests/print_runtime_e2e.rs`) ; les comparaisons de stderr plus larges ne sont pas recensées.
- **Pour nos tests, `env_remove` ne donne plus le manuel** : ils doivent le fixer eux-mêmes.

### Nos tests Rust qui obtiennent le manuel par le défaut (en échec après le changement)

| Fichier | Appui sur le défaut |
|---|---|
| `crates/pa-core/src/session_engine/ya_permissions.rs:580-589` | `unset_is_manual` : réglage absent → `Manual`, `Default` |
| `crates/pa-cli/tests/ya_permissions_e2e.rs:206-213`, `:228-236` | a et c : `-p` et RPC sans réglage refusés au démarrage (D6) |
| `crates/pa-cli/tests/ya_permissions_e2e.rs:188-204` | `assert_refused` cherche `permissions are "manual"` ; le message dit « (the default) » (`ya_permissions.rs:287`) |
| `crates/pa-cli/tests/ya_permissions_acp_e2e.rs:95`, `:382` | `env_remove`, puis `currentValue == "manual"` et demandes attendues |
| `crates/pa-cli/tests/ya_permissions_tui_pty_e2e.rs:133-138`, `:164` | réglages sans `permissions`, « no badge » à l'ouverture (`:404`) |
| `crates/pa-cli/tests/ya_session_lease_symlink_e2e.rs:84`, `:277` | une demande attendue pour la cellule |
| `crates/pa-daemon/tests/ya_permissions_direct_link_e2e.rs:101` | demande ouverte attendue |
| `crates/pa-daemon/tests/ya_permissions_same_requests_e2e.rs:216`, `:454` | manuel par le défaut, auto par la commande |

Restent verts sans retouche : `global_values`, `test_override_wins`, `manual_without_asker_is_refused_with_the_exact_line_and_path` (mode fixé par `wiring.mode`, `ya_permissions.rs:621-631`), `project_settings_cannot_turn_the_session_to_auto` (manuel global explicite, `ya_permissions_e2e.rs:215-226`), `global_auto_reaches_the_model_and_warns` (`:254-275`), les tests de `prefix_stability.rs` (mode passé explicitement, `crates/pa-core/tests/prefix_stability.rs:2434-2440`, `:2540-2545`) et le test du sélecteur ACP (`crates/pa-daemon/src/acp/ya_permissions.rs:610-625`).

### Extension

- **Agent factice** : son défaut est le sien, `manual` (`extensions/vscode/test/fake-agent/fake-agent.mjs:6`, `:59`). Les tests sur l'agent factice ne cassent pas, mais ne reflètent plus le pont : `suite.ts:179` (« pas de bandeau en manuel » à l'ouverture), `suite.ts:369-382`, `test/unit/agent.test.ts:336-350`.
- **e2e sur le vrai binaire** : dossier `.yusagent` vide (`extensions/vscode/test/e2e/run-real.ts:29`), donc manuel par défaut aujourd'hui ; les cas d'accord et de refus en dépendent (`suite-real.ts:141-143`, `:190`, `:207`, `:224`) ; le cas « auto » écrit le réglage lui-même (`:239`). Binaire de debug lancé hors de `cargo`, sans `PRIME_AGENT_TEST_PERMISSIONS` (`.github/workflows/ya-extension.yml:130`, `:153`). **En échec après le changement.**
- **Piège CI** : le job `real-binary` ne tourne que si `extensions/vscode/`, `crates/pa-daemon/src/acp/`, `bash.py` ou `skills/edit/` changent (`.github/workflows/ya-extension.yml:115`). Un changement limité à `crates/pa-core/src/session_engine/ya_permissions.rs` passerait au vert sans lancer ces e2e, et la casse n'apparaîtrait qu'à la PR suivante de l'extension.

### Banc et scripts

| Script | Réglage | Après le changement |
|---|---|---|
| `bench/smoke/run.py:89`, `:320-323` | écrit toujours `"auto"` | inchangé (utile pour les A/B sur binaires d'avant ce ticket) ; commentaire à corriger |
| `bench/tools/capture/run.py:110`, `:138-142` (`write_settings`, repris par `resume_probe.py:189`, `c12_repro.py`, le cas TUI `:516`) | écrit toujours `"auto"` | inchangé |
| `bench/tools/capture/acp_frames.py:63-67` | mode passé par cas | inchangé |
| `bench/tools/capture/wake_turns.py:263`, `bench/smoke/tools/notice-batch-repro.sh:20` | `"auto"` | inchangé |
| `crates/pa-perf/src/corpus/mod.rs:243-244` (perf nocturne) | `"auto"` | inchangé |
| `bench/tools/ya_permissions_acp_demo.py:170` | `{}` → manuel par le défaut | **à corriger** : aucune demande n'arriverait, la démo échoue |
| `bench/tools/ya_permissions_tui_demo.py:278-279` | sans `permissions` → manuel | **à corriger**, même raison |

## 3. D6 une fois l'auto par défaut

- **Sans réglage** : `-p` et RPC tournent en « auto », sans demander ; le refus au démarrage ne se produit plus. `ya_permissions_e2e.rs` a et c s'inversent.
- **Avec `"permissions": "manual"` dans les réglages globaux** : le refus au démarrage reste (`ya_permissions.rs:347-349`), avant toute requête. C'est le seul cas où D6 s'applique encore.
- **Le message est faux une fois le défaut changé** : « permissions are "manual" (the default) … add this line … "permissions": "auto" » (`ya_permissions.rs:285-292`). Le mode vient alors d'une ligne écrite par l'utilisateur ; la consigne utile est de la passer à `"auto"` ou de la retirer.
- **Sessions du démon sans demandeur** (clients autres que le pont ACP et le TUI, qui posent `yaPermissionAsker` : `crates/pa-cli/src/print_runtime.rs:163-164`, `crates/pa-tui/src/interactive.rs`) : refusées en manuel aujourd'hui, elles tourneront en « auto ». Non recensées ici.

## 4. Avertissement « auto sans sandbox » (D4)

| Où | Texte exact | Quand |
|---|---|---|
| stderr | `yusAgent: permissions are "auto" and there is no sandbox: every tool call runs without confirmation, with your user's rights.` (`AUTO_WARNING`, `crates/pa-core/src/session_engine/ya_permissions.rs:67`) | Au démarrage d'une session racine sans démon (`-p`, RPC : `announce`, `engine.rs:245`), **seulement si « auto » vient du réglage global** (`ModeSource::GlobalSettings`, `ya_permissions.rs:350`). Jamais en ACP ni TUI. **Avec l'auto par défaut et cette condition inchangée, un `-p` sans réglage tournerait en « auto » sans aucun avertissement.** |
| ACP, libellé du sélecteur | option `auto` : `Run tool calls without asking (no sandbox)` ; option `manual` : `Ask before each tool call` ; nom `Permissions` (`crates/pa-daemon/src/acp/ya_permissions.rs:341-349`) | Dans `configOptions` de toute session qui sait demander, dans les deux modes. C'est un libellé de choix, pas un état. L'extension ne l'affiche pas (pas de sélecteur). Pas de `session_info_update` dédié (point ouvert du ticket 20). |
| TUI, étiquette | `auto · no sandbox` (`AUTO_BADGE`, `crates/pa-tui/src/ya_permission_panel.rs:32`), en couleur d'avertissement, premier élément à droite de la barre sous l'éditeur (`crates/pa-tui/src/chrome.rs:546-552`) | Dès que la session est en « auto » : snapshot d'attach, `ya_permission_mode`, `/permissions` (`crates/pa-tui/src/session_ui/ya_permissions.rs:162-167`). **Avec l'auto par défaut : permanente pour toute session.** |
| TUI, note au changement | `Permissions: auto. Tool calls run without asking, with your user's rights (no sandbox).` (`crates/pa-tui/src/session_ui/ya_permissions.rs:274-277`) ; choix du sélecteur `/permissions` : `Run tool calls without asking (no sandbox)` (`crates/pa-tui/src/choice_picker.rs:66`) ; description de la commande (`crates/pa-types/src/slash_commands.rs:47`) | Une fois, au passage en « auto » par `/permissions` ; à l'ouverture du sélecteur. |
| Extension, bandeau | `⚠ Mode auto sans sandbox : les cellules s'exécutent sans demander, avec vos droits.` (`extensions/vscode/webview/Status.tsx:134-138`), `role="alert"`, pleine largeur sous l'en-tête, fond et bordure d'avertissement (`extensions/vscode/webview/styles.css:93-97`) ; décrit dans `extensions/vscode/README.md:12` | Tant que `session.config.permissions === "auto"` (`Status.tsx:60`), à l'ouverture ou après `config_option_update`. **Avec l'auto par défaut : permanent pour toute conversation.** |

### Bandeau de l'extension : deux options (non tranché)

- **Option A, permanent comme aujourd'hui.** Aucun changement de l'extension. Le bandeau reste visible dans toute conversation. Pour : le critère « Avertissement persistant en mode sans sandbox » (`docs/yusAgent-cdc.md:418`) reste tenu tel quel, sans nouveau code. Contre : un `role="alert"` affiché en permanence cesse d'être lu et prend une ligne de hauteur dans la barre latérale.
- **Option B, rappel discret en « auto ».** Le bandeau devient une étiquette courte et persistante, comme celle du TUI (par exemple « auto · sans sandbox » dans l'en-tête ou près du compositeur), avec le texte complet en infobulle et `role="status"` au lieu d'`alert`. Variante possible : un clic ouvre le choix `manual` / `auto`, par le sélecteur ACP existant (`set_config_option`, `permissions`). Pour : toujours visible sans gêner, aligné sur le TUI. Contre : code et tests de l'extension en plus ; avec la variante, nouveau contrôle dans l'interface.

## 5. Préfixe de prompt

**Le mode n'entre pas dans les requêtes au modèle.**

- Ni dans le prompt système, ni dans les définitions de tools, ni dans `yaRequest` (`ya_permissions.rs:21-23` ; recherche sans résultat de `declin|approv|confirm|permission` dans `crates/pa-core/src/prompts/ya_prompt_v2.rs` et `system_prompt.rs`).
- Les entrées `ya-permission` du journal sont ignorées à la reprise (ticket 20, PR 2).
- Tests existants : `permissions_manual_and_auto_send_the_same_prefix` (`crates/pa-core/tests/prefix_stability.rs:2425`), `permissions_journal_does_not_change_the_requests_of_a_resumed_session` (`:2530`), `manual_and_auto_send_the_same_requests` au niveau du démon (`crates/pa-daemon/tests/ya_permissions_same_requests_e2e.rs:430`).
- Seuls effets du mode sur ce qui part : en manuel, un refus ajoute `[permission-denied] …` en fin d'historique et arrête le tour (D7) ; l'attente d'une réponse peut dépasser le TTL du cache. En « auto », ni l'un ni l'autre. Aucun des deux ne touche le préfixe.
- **Contrôle** : les tests ci-dessus restent verts ; capture `py-example-001` contre `main`, `compare_capture_prompts.py` « TOUT IDENTIQUE » attendu. **Limite** : le banc de capture force `"auto"` des deux côtés (`bench/tools/capture/run.py:110`) ; la capture prouve que le changement ne touche pas les requêtes, pas l'égalité entre modes, qui repose sur les tests.

## 6. Plan

### PR (proposé)

Une seule PR, branche `ya-30-auto-defaut`, commits séparés. Elle touche `extensions/vscode/`, ce qui lance le job `real-binary` (section 2, piège CI).

1. **Tests d'abord** (commit seul, poussé pour les voir en échec) :
   - `unset_is_auto` (remplace `unset_is_manual`) ;
   - `-p` sans réglage : atteint le modèle, avertissement D4 sur stderr ;
   - RPC sans réglage : atteint le modèle ;
   - `-p` avec `"manual"` global : refusé avant toute requête, avec le nouveau message ;
   - ACP sans réglage : `currentValue` `auto`, cellule exécutée sans `session/request_permission` ;
   - TUI sans réglage : étiquette `auto · no sandbox` à l'ouverture ;
   - extension, vrai binaire : sans réglage, `permissions` = `auto` à l'ouverture.
2. **Nos tests existants passés au manuel explicite** (`"permissions": "manual"` dans leurs réglages globaux, à la place de `env_remove(TEST_ENV)`) : les 6 fichiers e2e de la section 2, `run-real.ts` ou `suite-real.ts` pour les cas d'accord et de refus, démos `ya_permissions_acp_demo.py` et `ya_permissions_tui_demo.py`.
3. **Cœur** (`crates/pa-core/src/session_engine/ya_permissions.rs`, module du fork) : défaut `Auto` (`:269`) ; avertissement aussi pour la source `Default` (`:350`) ; message D6 sans « (the default) » (`:285-292`) ; docs du module (`:6`, `:75`, `:101`).
4. **Extension** : agent factice par défaut en `auto` (miroir du pont), tests de `suite.ts` et `agent.test.ts` adaptés ; bandeau selon le choix A ou B ; `README.md:12`, `:16`.
5. **Doc** : ligne du journal du CDC (écarts à `docs/yusAgent-cdc.md:409`, `:492`, D5) ; `docs/fork-changes.md` ; fragment `.changes/` ; commentaires de `.cargo/config.toml:14-17` et `bench/smoke/run.py:320`.

**Fichiers upstream** : aucun si la variable de `.cargo/config.toml` reste (seul son commentaire change) ; voir le choix C3.

### Tests à voir en échec avant le correctif

Ceux du commit 1 (sur le cœur de `main`) : `unset_is_auto`, `-p` et RPC sans réglage, ACP et TUI sans réglage, l'e2e réel de l'extension sans réglage. Run `ci` (et `ya-extension`) du seul commit de tests, noté dans le ticket, puis vert avec le correctif.

### Contrôle manuel prévu pour Yusuf

Script `bench/tools/ya_permissions_auto_default_check.py --binary <dossier>/yusagent`, **lancé d'abord par Claude Code** sur le binaire de la branche (règle 13), sortie jointe. Dossier agent et projet temporaires, serveur de capture local, aucun modèle réel :

1. `-p` sans réglage : code 0, avertissement sur stderr, au moins une connexion au serveur ;
2. `-p` avec `"permissions": "manual"` global : code non nul, nouveau message, 0 connexion ;
3. ACP sans réglage : `permissions` = `auto`, cellule exécutée sans demande ; `set_config_option` `manual` → la cellule suivante est demandée ; journal `auto->manual` ;
4. un `<projet>/.yusagent/settings.json` ne change pas le mode (sauf décision contraire, C4) ;
5. `find ~/.yusagent ~/.prime/agent -newer` vide.

Puis, à la main : extension (bandeau ou étiquette selon A ou B, puis `"manual"` dans `~/.yusagent/settings.json` et nouvelle conversation : plus de bandeau, demande à la première cellule) ; TUI (étiquette à l'ouverture, `/permissions manual` la retire) ; capture `py-example-001` contre `main` (« TOUT IDENTIQUE »).

## 7. Choix à trancher

- **C1, bandeau de l'extension** : option A (permanent) ou B (rappel discret ; avec ou sans clic vers le choix du mode). Section 4.
- **C2, avertissement stderr en `-p` et RPC** : l'imprimer aussi quand « auto » vient du défaut (recommandé : sinon `-p` tourne en « auto » sans rien dire), ou seulement quand le réglage est écrit (comportement actuel).
- **C3, `PRIME_AGENT_TEST_PERMISSIONS`** : garder `"auto"` dans `.cargo/config.toml` et les listes blanches (les tests upstream restent sans avertissement sur stderr), ou la retirer avec ses reprises dans `scripts/` et `ci.yml` (moins d'écarts upstream, mais l'avertissement C2 arrive dans le stderr des tests upstream, dont 2 fichiers qui exigent un stderr vide).
- **C4, réglage de projet** : D1 inchangé (le projet n'est jamais lu, ni pour « auto » ni pour « manual »), ou un projet peut **resserrer** en `"manual"` sans pouvoir passer en « auto ».
- **C5, message D6** quand `"manual"` est écrit et que le mode ne peut pas demander : remplacer « (the default) » par une consigne « passer la ligne à "auto" ou la retirer » ; texte exact à valider.
- **C6, sélecteur `permissions` dans l'extension** : aujourd'hui absent ; le passage en manuel se fait par `~/.yusagent/settings.json` seulement. Dans ce ticket (lié à B), ou plus tard.
- **C7, agent factice** de l'extension : défaut aligné sur le pont (`auto`, recommandé) ou gardé en `manual`.
- **C8, banc** : garder l'écriture explicite de `"auto"` (recommandé : les A/B sur binaires d'avant ce ticket en dépendent) ou la retirer.
- **C9, CDC** : ligne du journal dans la PR de code, avec les écarts à `docs/yusAgent-cdc.md:409` (« auto-approve désactivé » par défaut) et `:492` (YA-16, build publié). À confirmer pour le build publié : « auto » par défaut aussi, ou manuel pour les builds distribués.
