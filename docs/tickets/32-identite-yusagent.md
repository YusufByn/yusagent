# Ticket 32 : identité yusAgent (constat et plan)

Branche `ya-32-constat`, depuis `main` à `085d06962`. Constat seul : aucun code du produit, aucun fichier upstream touché.

**Règle** (décision 1 de Yusuf du 23/11/2026, `docs/tickets/31-skills-mcp-reglages.md:445` ; ticket annoncé au `:467` et au journal du CDC, `docs/yusAgent-cdc.md:102`) : tout ce qui relève de l'identité dans le code repris d'Unharnessed passe au nom de yusAgent (méthodes `_yus/*`, clés, chaînes, identifiants, variables CSS `--ya-*`). La notice MIT, l'attribution de `docs/fork-changes.md` et les commentaires qui citent la source restent. C9 (`31-skills-mcp-reglages.md`, §9.2) : le code de l'onglet Instructions masqué se supprime au ticket 32 ou avec le visuel.

**Commandes de l'inventaire** (sur `origin/main` à `085d06962`) :

```sh
git grep -n -i unharnessed origin/main -- . ':!docs'                       # 166 lignes
git grep -n -i unharnessed origin/main -- docs                             # 283 lignes, 19 fichiers
git grep -n -P -- "(?<![A-Za-z0-9])(uh|UH)[-_.](?=[a-zA-Z])" origin/main -- . ':!docs'
git grep -n -E "globalState|workspaceState|localStorage|sessionStorage|getState\(|setState\(" origin/main -- extensions/vscode
```

Les 283 lignes de `docs/` (tickets, CDC, passations, `fork-changes.md`) sont des archives et ne sont pas renommées. Les noms de Prime Agent (`ai.primeintellect.prime-agent`, `PRIME_AGENT_*`) ne sont pas des noms d'Unharnessed. Ils restent par la décision du 19/11/2026 (`docs/yusAgent-cdc.md:89`, « `_meta` gardé ») et sont hors de ce ticket.

## 1. Inventaire

### 1.1 Noms sur le fil entre l'extension et le pont

**Méthode servie par le pont (une seule).**

| Nom | Pont | Extension | Tests, agent factice, banc |
|---|---|---|---|
| `_unharnessed/sessions/delete` | constante `crates/pa-daemon/src/acp/ya_history.rs:37`, aiguillage `:578`, `:589` (appelé par `daemon.rs:1145`, `:1152`) | `src/history.ts:10` (appel `:96`), `src/ya/empty.ts:18` (appel `:38`), liste des méthodes servies `src/ya/methods.ts:18` | `crates/pa-cli/tests/ya_acp_resume_e2e.rs:427`, `:440`, `:467` ; `test/fake-agent/fake-agent.mjs:520` ; `test/unit/meta.test.ts:32` ; `bench/tools/capture/delete_files_check.py:115`, `empty_delete_probe.py:105` |

**Méthodes que l'extension appelle mais que le pont ne sert pas.** L'appel est rejeté sur place, sans trame : `src/agent.ts:97` pour une requête, `:105-107` pour une notification (`served`, `src/ya/methods.ts:33-39`).

| Nom | Genre | Extension |
|---|---|---|
| `_unharnessed/session/steer` | requête | `src/chat.ts:39`, appel `:465` ; drapeau `src/ya/methods.ts:45` |
| `_unharnessed/session/rewind` | requête | `src/chat.ts:647` ; drapeau `src/ya/methods.ts:46` ; test `test/unit/agent.test.ts:85` |
| `_unharnessed/kernel/tail`, `kill`, `preview` | requêtes | `src/kernel.ts:8-10`, appels `:59`, `:71`, `:92` ; drapeau `src/ya/methods.ts:47` (sur `tail`) |
| `_unharnessed/providers/list`, `reload`, `set_api_key`, `logout`, `login`, `login_answer`, `login_open`, `local_scan`, `local_probe`, `local_save`, `local_remove` | requêtes | `src/providers.ts:8-14`, `:17-20` ; appels `:74`, `:85`, `:89`, `:95`, `:107`, `:123`, `:127`, `:143`, `:166`, `:189` ; `reload` diffusé aux autres agents `:149`, `:232` (`src/shown.ts:73-78`) ; drapeau `src/ya/methods.ts:48` (sur `list`) |
| `_unharnessed/providers/login_cancel` | notification envoyée | `src/providers.ts:15`, `:198` |

**Notifications que l'extension écoute et que le pont n'envoie jamais.**

| Nom | Extension |
|---|---|
| `_unharnessed/kernel/restored` | `src/chat.ts:37`, écoute `:168` |
| `_unharnessed/kernel/changed` | `src/kernel.ts:7`, écoute `:32` |
| `_unharnessed/providers/login_update` | `src/providers.ts:16`, écoute `:47` |

**Clé `_meta.unharnessed`, lue par l'extension et envoyée par personne.** Le pont met ses champs sous `ai.primeintellect.prime-agent` (`crates/pa-daemon/src/acp/meta.rs`). L'agent factice fait de même (`fake-agent.mjs:93`, `:514`).

| Où | Lecture |
|---|---|
| `src/ya/meta.ts:17` | `yaMeta` prend `_meta.unharnessed` avant notre espace |
| `src/ya/meta.ts:37` | `yaResultMeta`, idem |
| `src/chat.ts:93`, `:95` (types), `:1112`, `:1116` | `kind` et `source` des `availableCommands`. Le pont n'envoie aucun `available_commands_update` : `grep -rln "available_commands_update\|availableCommands" crates` est vide. Code mort avec notre pont. |
| `src/session.ts:17`, `:190` | `_meta.unharnessed.model` d'une valeur du sélecteur de modèle. Les options du pont n'ont pas ce `_meta` (`crates/pa-daemon/src/acp/config_options.rs`). Code mort lui aussi. |
| tests : `test/unit/meta.test.ts:25-26`, `test/unit/agent.test.ts:371` | vérifient que `_meta.unharnessed` est lu tel quel |

**Noms de forme repris d'Unharnessed, sans marque, dans notre espace `_meta`** (servis) :
- `turnStart` (`origin`, `wake`, `rewindTo`) : `ya_turns.rs:35-47`, champ `meta.rs:93-95` ;
- `turnEnd` : `ya_history.rs:61-67`, `meta.rs:96-98` ;
- `saved` : `ya_history.rs:52-59`, `:100-105`, `meta.rs:99-102`.

Ils ont été retenus sous ces noms en connaissance de cause : T1 du ticket 25 (`docs/yusAgent-cdc.md:96`) et C2 du ticket 28 (`:99`). Le ticket 31 laisse à Yusuf la question de les ranger dans l'identité (`31-skills-mcp-reglages.md:467`), voir §2.4.

Noms déjà en `_yus/*` sur le fil : `_yus/startLine`, `_yus/startColumn`, `_yus/bashRuns` (`ya_cell_display.rs:19-23`), `_yus/notice` (`ya_notice.rs:15`), `_yus/skills/*`, `_yus/mcp/*` (`ya_resources.rs:28-35`), `_yus/settings/*` (`ya_resources/settings.rs:23-24`).

### 1.2 Noms internes à l'extension

**Variables CSS.**
- `--uh-accent-rgb` est lue sur `body` dans `webview/Chat.tsx:518` (`readInk`, couleur de l'orbe `:489`). Aucune feuille ne la définit sur `body` : `ya-settings.css:58`, `:83` définit `--ya-accent-rgb` sous `body .ya-settings` (`:33`), donc seulement dans l'écran des réglages. La valeur lue est vide aujourd'hui, et elle le restera après renommage en `--ya-accent-rgb` : aucun effet visible.
- `--uh-glow-angle` est posée par `webview/Goal.tsx:241` et retirée en `:280`. Aucune feuille ne la lit : `git grep glow-angle -- '*.css'` est vide. Aucun effet visible non plus.

**Commentaires qui nomment l'identité au lieu de citer la source** (à renommer avec le code) :
- `src/protocol.ts:5` (« The `unharnessed --mode acp` process ») ;
- noms de méthode ou de clé en doc : `src/protocol.ts:14`, `:36`, `:63`, `:417`, `:456`, `src/session.ts:20`, `:75`, `src/chat.ts:61`, `src/kernel.ts:6`, `src/providers.ts:6`, `src/ya/empty.ts:5`, `src/ya/meta.ts:1-4`, `:13`, `:22-26`, `src/ya/methods.ts:2-3` ;
- côté tests : `test/unit/meta.test.ts:3`, `test/unit/agent.test.ts:84`.

**Tests.**
- `test/unit/agent.test.ts:85-87` : `_unharnessed/session/rewind`, `steer`, `whatever`, pour le rejet sur place.
- `test/unit/meta.test.ts:32` : liste des méthodes servies.
- `test/e2e/suite.ts:218-219` (« aucun « Unharnessed » visible ») : garde à garder.

**Agent factice** : `test/fake-agent/fake-agent.mjs:520` (suppression). Rien d'autre.

**Rien n'est stocké sous un nom d'identité.** L'extension n'utilise ni `globalState` ni `workspaceState`. La webview ne garde que `{ draft }` (`webview/vscode.ts:15-20`). Le manifeste est déjà en `yusagent.*` (`package.json:23-69` : commande, vue, réglage `yusagent.binaryPath`).

### 1.3 Côté Rust et dans `bench/`

| Nom | Où | Nature |
|---|---|---|
| `DELETE_METHOD = "_unharnessed/sessions/delete"` | `crates/pa-daemon/src/acp/ya_history.rs:37`, doc `:36` | fil (§1.1) |
| doc du module et repère de section | `ya_history.rs:17`, `:730` | décrivent notre méthode : suivent le renommage |
| test `turn_start_serializes_with_unharnessed_names` | `crates/pa-daemon/src/acp/ya_turns.rs:411` | identifiant de test (fichier du fork) |
| chaînes de méthode du test e2e | `crates/pa-cli/tests/ya_acp_resume_e2e.rs:427`, `:440`, `:467` | fil (fichier du fork) |
| sondes du ticket 28 | `bench/tools/capture/delete_files_check.py:9`, `:115` ; `empty_delete_probe.py:11`, `:105` | fil |
| option `--unharnessed` et fonction `unharnessed()` | `bench/smoke/ab/ya09-bash-count.py:10`, `:101`, `:127`, `:134-135` | désignent le dépôt d'Unharnessed qu'on mesure, pas notre produit : **proposé : garder** |

`acp_frames.py`, `compare_frames.py`, `acp_resume.py`, `acp_live_changes.py` et le fixture `extensions/vscode/test/fixtures/acp-frames.json` ne contiennent aucun nom d'Unharnessed (grep ci-dessus).

### 1.4 Attribution à garder

- **Notices MIT** : `extensions/vscode/LICENSE:4` ; `extensions/vscode/webview/ya-settings.css:1-24` ; `docs/fork-changes.md:1521-1558` (« Code repris d'Unharnessed », notice et tableau).
- **Mentions de base** :
  - `extensions/vscode/README.md:5` ;
  - `extensions/vscode/package.json:4`. Cette description s'affiche dans le gestionnaire d'extensions de VS Code ; c'est une attribution, gardée.
- **Commentaires qui citent la source**, extension :
  - en-têtes et écarts : `src/agent.ts:1`, `src/extension.ts:1`, `:23`, `src/view.ts:20`, `:469`, `src/settings.ts:9`, `src/ya/empty.ts:2`, `src/ya/env.ts:5` ;
  - chemins `crates/uh-daemon/...` : `src/chat.ts:36`, `:38`, `src/history.ts:9`, `src/providers.ts:7` ;
  - outillage : `esbuild.mjs:3`, `eslint.config.mjs:9` ;
  - bouchons visuels, ligne 1 : `webview/FileIcon.tsx`, `Splash.tsx`, `Wordmark.tsx`, `logos.tsx`, `motion.ts`, `powder.ts`, `wave.ts` ;
  - webview : `webview/main.tsx:6`, `Settings.tsx:234`, `styles.css:3`, `:359`, `ya-cells.tsx:2`, `ya-orb.tsx:2`, `ya-settings.css:25-30` ;
  - tests : `test/unit/agent.test.ts:1`, `test/unit/setup.ts:1`, `test/e2e/suite.ts:382`.
- **Commentaires qui citent la source**, Rust et runtime :
  - `crates/pa-core/src/prompts/ya_prompt_v2.rs:2`, `:5-7`, `:66` ; `ya_bash_wait.rs:7`, `:9` ;
  - `crates/pa-core/src/session_engine/ya_request.rs:24`, `:34`, `:217` ; `ya_bash_timeout.rs:3-5` ; `ya_bash_notice_batch.rs:12-14`, `:35` ; `ya_tool_family.rs:18` ;
  - `crates/pa-core/src/kernel/bootstrap/ya_bash_timeout.rs:10` ; `crates/pa-core/tests/prefix_stability.rs:1108` ;
  - `crates/pa-daemon/src/acp/daemon.rs:1279` ; `meta.rs:93`, `:96`, `:100` ;
  - `ya_history.rs:3-4`, `:48`, `:52`, `:61`, `:267`, `:298`, `:354`, `:675` ;
  - `ya_resources.rs:3`, `:5`, `:37`, `:297` ; `ya_resources/settings.rs:10` ;
  - `ya_turns.rs:3`, `:32`, `:35`, `:44`, `:140` ;
  - `crates/pa-cli/src/ya_update_off.rs:6` ; `crates/pa-telemetry/src/sinks/analytics.rs:23` ;
  - `prime-agent-runtime/src/rlm/ya_timeout.py:3-4` ;
  - `bench/smoke/ab/ya09-bash-count.py:2`, `:28-31`, `:104-110` (chemins de leur dépôt).
- **Archives** :
  - fragments de changelog `.changes/ya-09-bash-full-wait.md:1`, `.changes/ya-28-reprise.md:1`. Ce dernier cite `_unharnessed/sessions/delete` : c'est l'état d'alors, et le fragment du ticket 32 dira le renommage ;
  - `bench/smoke/ab/ya09-bash-plan.md:3`.

## 2. Noms sur le fil : émetteur, lecteur, même PR, disque

### 2.1 Tableau

| Nom | Envoie | Lit | Même PR ? |
|---|---|---|---|
| `_unharnessed/sessions/delete` | extension (`history.ts:96`, `ya/empty.ts:38`) ; sondes du banc | pont (`ya_history.rs:578`) | **Oui.** Le pont répond `-32601` à une méthode inconnue (`daemon.rs:1166-1172`), et l'extension refuse sur place une méthode qu'elle ne croit pas servie (`agent.ts:97`). L'e2e sur le vrai binaire supprime par l'interface (`test/e2e/suite-real.ts:481`, `:650-657`) : il ne passe que si les deux côtés changent ensemble. |
| méthodes non servies (§1.1) | extension, rejet sur place | personne | Extension seule |
| notifications écoutées (§1.1) | personne | extension | Extension seule |
| `_meta.unharnessed` | personne | extension | Extension seule |
| `turnStart`, `turnEnd`, `saved` | pont (et agent factice) | extension, banc | Oui, si renommés (§2.4) |

### 2.2 Ce qui est écrit sur disque

**Aucun nom d'Unharnessed, ni aucun nom de forme du fil, n'est écrit sur disque.**
- **Fichiers de session** (`.jsonl`) : ils gardent les lignes de la conversation. `session/load` reconstruit `turnStart`, `turnEnd` et la notice à la relecture (`ya_history.rs:267` et suivantes). La ligne d'annonce est stockée sous le type `ya-resources-changed`, déjà à notre nom (`ya_notice.rs:1-3`, `:18-27`). `turnStart` et `turnEnd` n'apparaissent nulle part dans `crates/pa-core` (`git grep -c turnStart origin/main -- crates/pa-core` est vide).
- **Instantané de fenêtre et info-cache** : écrits par le cœur (`crates/pa-core/src/session/window.rs:587`, `session_scan.rs:61-72`), sans `_meta` ACP.
- **Réglages** (`~/.yusagent/settings.json`) : des clés du cœur (`ya_resources/settings.rs:53-71`), aucune propre à Unharnessed.
- **Extension** : seulement le brouillon de la webview (§1.2).

**Conséquences d'un renommage sur les conversations existantes.**
- **Relecture** : aucune ; rien de stocké ne porte ces noms.
- **Rejeu** : aucun changement, puisque le pont reconstruit les trames.
- **Suppression** : seule la méthode change de nom. Le fichier, son dossier d'instantané et le journal d'usage ne bougent pas (C4 du ticket 28).

### 2.3 Lecture de l'ancien nom : proposition (à décider par Yusuf)

- **Option A (proposée) : coupure nette.**
  - Le pont ne sert que `_yus/sessions/delete`. L'extension ne lit plus `_meta.unharnessed`, que personne n'envoie.
  - Conséquence d'un mélange de versions : un `.vsix` de `main` avec le binaire de la branche, ou l'inverse, donne `-32601` à la suppression. L'historique affiche l'erreur, la conversation reste, aucune donnée n'est perdue.
  - Pour une conversation neuve quittée vide, le pont la supprime de lui-même à la fin de stdin (`drop_if_empty`, `src/ya/empty.ts:9-10`).
  - Utilisateur unique, binaire et `.vsix` installés ensemble (contrôle §9.7 du ticket 31, étape 0).
- **Option B : les deux noms servis un temps.**
  - Une alternative dans `serves` (`ya_history.rs:578`), retirée plus tard dans un ticket daté.
  - Coût : un nom d'Unharnessed reste dans le code, contre la règle, et il faut un ticket de retrait.
- Lecture de `_meta.unharnessed` dans l'extension : aucune raison de la garder, puisque personne ne l'envoie. Retrait proposé dans les deux options.

### 2.4 `turnStart`, `turnEnd`, `saved` : dans l'identité ou non ? (à décider par Yusuf)

- **Option 1 (proposée) : garder ces noms.**
  - Ils ne portent aucune marque.
  - Ils sont dans notre espace `_meta`, à côté des noms d'upstream (`phase`, `outcome`, `promptTurnId`).
  - L'écart à la convention `_yus/*` est déjà accepté (T1 du 21/11/2026, C2 du 22/11/2026).
- **Option 2 : passer à des clés `_yus/*` à la racine du `_meta`**, comme `_yus/notice`.
  - Code à toucher :
    - pont : `ya_turns.rs`, `ya_history.rs`, `meta.rs`, fichier upstream (ses 9 lignes du fork, `docs/fork-changes.md:1295`, `:1334`, pourraient alors partir) ;
    - test upstream `crates/pa-cli/tests/acp_mode_e2e.rs` (5 lignes `turnStart`, `docs/fork-changes.md:1297`) ;
    - tests du fork `ya_acp_wake_turn_e2e.rs` (12), `ya_acp_resume_e2e.rs`, `ya_resources_acp_e2e.rs`, `ya_history_tests.rs` ;
    - extension : `chat.ts`, `ya/meta.ts`, tests ;
    - agent factice ;
    - banc : `acp_resume.py`, `acp_live_changes.py`, `compare_frames.py` (9 lignes), `wake_turns.py`.
  - À vérifier avant : est-ce que `correlate` garde les clés racine du `_meta` ? C'est la raison des champs de `meta.rs`.
  - `compare_frames.py --against main` donnerait alors des écarts sur toutes les trames de tour, attendus mais non couverts par `--expect` (qui ne sait que des clés ajoutées).

Le reste du plan suppose l'option 1.

## 3. Méthodes non servies : noms cibles

Règle du ticket 31 (décision 1) : formes et suffixes d'Unharnessed, préfixe `_yus/`.

| Aujourd'hui | Cible | Drapeau de `features()` (`methods.ts:42-53`) |
|---|---|---|
| `_unharnessed/session/steer` | `_yus/session/steer` (R4, fixé) | `steer` |
| `_unharnessed/session/rewind` | `_yus/session/rewind` | `rewind` |
| `_unharnessed/kernel/tail`, `kill`, `preview` | `_yus/kernel/tail`, `kill`, `preview` | `kernel` (sur `tail`) |
| `_unharnessed/kernel/changed`, `restored` (notifications) | `_yus/kernel/changed`, `_yus/kernel/restored` | — |
| `_unharnessed/providers/*` (13 noms, §1.1) | `_yus/providers/*`, mêmes suffixes | `providers` (sur `list`) |
| `_unharnessed/sessions/delete` (servie) | `_yus/sessions/delete` | — |

**Question de forme, à décider** : `sessions/delete` (pluriel d'Unharnessed) ou `session/delete` (comme `_yus/session/steer` et l'ACP `session/*`) ? Proposé : garder `sessions/delete`, même suffixe que la source, comme pour `_yus/skills/*` au ticket 31.

Les drapeaux restent faux tant que le pont ne sert pas ces méthodes. Renommer ne montre rien de plus dans le panneau.

## 4. Code de l'onglet Instructions (C9)

**État.**
- L'onglet n'est plus dans `TABS` (`webview/Settings.tsx:15-21`). Son code reste :
  - `webview/SettingsInstructions.tsx` (300 lignes) et `src/instructions.ts` (50 lignes) ;
  - `setInstructions` (`src/settings.ts:97-127`) ;
  - le message `setInstructions` (`src/view.ts:224-232`, `src/protocol.ts:707-711`) ;
  - le type `ModelInstructions` (`src/protocol.ts:158-161`) ;
  - le cas `"instructions"` (`webview/Settings.tsx:13`, `:289-296`) ;
  - l'import de `connectedModels` (`src/view.ts:7`).
- `connectedModels` et `modelKey` ne servent qu'à cet onglet (grep `connectedModels\|modelKey\|savedGroups\|withGroup`).

**La fonction n'existe pas derrière.**
- Le cœur ne connaît pas `modelInstructions` : `grep -rln -i "modelInstructions\|model_instructions\|MODEL_INSTRUCTIONS" crates` est vide.
- Le pont refuse la clé (`unknown setting`, `ya_resources/settings.rs:278` ; `KEYS` `:53-71`).
- La fonction ajoute du texte au prompt système selon le modèle. Elle changerait donc le préfixe (règle 4) et demanderait une décision de cache avant tout retour.

**Supprimer maintenant.**
- Gain : environ 400 lignes mortes en moins (taille du `.vsix` à mesurer au push 2), plus de vocabulaire `modelInstructions` à maintenir au fil des renommages.
- Coût : la base d'Unharnessed pour la passe de design se relit dans git, à `085d06962` ou chez Unharnessed `157dcdc1d`.
- Le test e2e « Instructions absent » (`test/e2e/suite.ts:442`) reste vert.

**Garder pour la passe de design.**
- Coût : du code mort, compilé et vérifié par `tsc` à chaque build, à renommer avec le reste.
- Risque nul aujourd'hui : l'onglet est hors de `TABS`, et la clé serait refusée par le pont.

**Recommandation : supprimer maintenant.** Ce n'est pas qu'un écran sans style : il dépend d'une fonction du cœur qui n'existe pas et qui toucherait au préfixe.

## 5. Préfixe de prompt

**Aucun nom renommé n'entre dans le texte reçu par le modèle.**
- **Méthodes et clés `_meta`** : ces noms n'existent que dans le pont ACP (`crates/pa-daemon/src/acp/`) et dans l'extension. Le modèle reçoit :
  - le prompt système (`crates/pa-core/src/prompts/system_prompt.rs:130-299`, `ya_prompt_v2/core.md`) ;
  - les définitions de tools ;
  - les lignes de l'historique.
- **Occurrences d'« Unharnessed » sous `crates/pa-core`** : toutes en commentaires `//!` ou `///` (§1.4), aucune dans une chaîne. `git grep -n -i -E "unharnessed|uh-|_yus/" origin/main -- crates/pa-core/src/prompts/ crates/pa-core/src/tools/` ne renvoie que des lignes de commentaire (`ya_bash_wait.rs:7`, `:9` ; `ya_prompt_v2.rs:2`, `:5-7`, `:66`).
- **Le docstring de `prime-agent-runtime/src/rlm/ya_timeout.py:3-4`** : c'est une attribution, gardée, hors prompt. Elle n'est visible que si le modèle appelle `help()` sur ce module.
- **Annonces** : texte fixe `[skills-changed]` et `[mcp-changed]`. Leur type `ya-resources-changed` n'est jamais envoyé au modèle comme nom.

**Contrôle prévu au push 2** :
- `run.py --capture` sur le binaire de `main` et sur celui de la branche ;
- puis `python3 -I bench/smoke/tools/compare_capture_prompts.py <run main> <run branche>`, attendu `TOUT IDENTIQUE`.

## 6. Trames et e2e

**`acp_frames.py`**
- Les cas `A-display-auto`, `B-basic-manual-reject` et `C-resources` n'appellent pas la suppression et ne contiennent aucun nom d'Unharnessed.
- Avec l'option 1 (§2.4), leurs trames ne changent pas.

**`compare_frames.py`**
- Contre l'agent factice : celui-ci ne change que sa ligne `case` de la suppression (`fake-agent.mjs:520`), qu'aucun cas ne joue. Attendu : `écarts : 0`, comme aujourd'hui.
- `--against` la capture de `main` : A, B et C `IDENTIQUE`, `écarts : 0`. C'est la preuve que seule la suppression change sur le fil.
- **Pas de cas de suppression ajouté** à `acp_frames.py`. Elle est couverte par :
  - le test e2e Rust (`ya_acp_resume_e2e.rs:420-470`) ;
  - l'e2e de l'extension sur le vrai binaire (`suite-real.ts:481`, `:650-657`) ;
  - les sondes `delete_files_check.py` et `empty_delete_probe.py`, lancées sur le binaire de la branche.

**Fixture des trames** (`test/fixtures/acp-frames.json`) : il ne contient ni la suppression ni aucun nom d'Unharnessed. Inchangé.

**e2e**
- Agent factice (`test/e2e/suite.ts`) : aucun cas de suppression. Les gardes `:218-219` et `:442` restent vertes.
- Vrai binaire (`test/e2e/suite-real.ts`) : les deux cas de suppression passent au push 2 seulement si pont et extension changent ensemble (§2.1).

## 7. Plan en pushs

Branche `ya-32-identite`, depuis `main` après le merge de ce constat. PR ouverte prête dès le push 1 (un run en brouillon saute les jobs lourds). **Aucun fichier upstream** avec l'option 1 : `ya_history.rs`, `ya_turns.rs`, `ya_history_tests.rs` et `ya_acp_resume_e2e.rs` sont du fork, `extensions/` est repris d'Unharnessed ou du fork, et les sondes du banc sont du fork.

### 7.1 Push 1 : tests seuls

Avec le ticket et le fragment `.changes/ya-32-identite.md` (exigé : des tests sous `crates/` changent). L'agent factice ne change pas encore : les tests de l'extension ne passent pas par la suppression.

| Test | Fichier | Rouge au push 1 sur |
|---|---|---|
| T1 « suppression sous `_yus/sessions/delete` » | `crates/pa-daemon/src/acp/ya_history_tests.rs` (nouveau cas) | `serves("_yus/sessions/delete")` faux ; avec l'option A, `serves("_unharnessed/sessions/delete")` vrai |
| T2 e2e de l'historique | `crates/pa-cli/tests/ya_acp_resume_e2e.rs:427`, `:440`, `:467` passés à `_yus/sessions/delete` | code `-32601` au lieu de `-32602` (premier refus attendu) |
| T3 « méthodes servies » | `extensions/vscode/test/unit/meta.test.ts:32` | `served("_yus/sessions/delete")` : `false !== true` ; avec l'option A, l'ancien nom encore servi |
| T4 « `_meta.unharnessed` n'est plus lu » | `test/unit/meta.test.ts:25-26` (inversé), `test/unit/agent.test.ts:371` | `{ path: "/u" }` au lieu de `{}` |
| T5 « aucun nom d'identité d'Unharnessed dans le code de l'extension » (nouveau `test/unit/identity.test.ts`) | parcourt `src/`, `webview/`, `test/fake-agent/` sans les commentaires, et cherche `"_unharnessed/`, l'accès à une clé `unharnessed`, `--uh-`, `uh-rise`, `uh-pop`, et `modelInstructions` si C9 = suppression | liste `fichier:ligne` des noms restants |
| T6 « méthodes non servies sous `_yus/*` » | `test/unit/agent.test.ts:85-87` passés à `_yus/session/rewind`, `steer`, `whatever` | reste vert (rejet sur place, quel que soit le nom) : **pas un test rouge**, c'est un renommage seul, donc au push 2 |

Restent verts au push 1 : les autres unitaires, l'e2e de l'agent factice (15/15), l'e2e du vrai binaire (18/18) et `compare_frames.py`.

### 7.2 Push 2 : le code, un sujet par commit

1. **Suppression `_yus/sessions/delete`** :
   - pont : `ya_history.rs:36-37`, doc `:17`, repère `:730` ; ancien nom selon §2.3 ;
   - extension : `history.ts:10`, `ya/empty.ts:5`, `:18`, `ya/methods.ts:18` ;
   - agent factice : `fake-agent.mjs:520` ;
   - sondes du banc : `delete_files_check.py:9`, `:115`, `empty_delete_probe.py:11`, `:105`.
2. **Méthodes et notifications non servies en `_yus/*`** (§3) :
   - `chat.ts:37`, `:39`, `:647` ;
   - `kernel.ts:6-10` ;
   - `providers.ts:6`, `:8-20` ;
   - `ya/methods.ts:45-48` ;
   - `protocol.ts:14`, `:36`, `:63`, `:456` ;
   - `agent.test.ts:84-87`.
3. **`_meta.unharnessed` plus lu** :
   - `ya/meta.ts:1-4`, `:13`, `:17`, `:22-26`, `:37` ;
   - types et lectures de `chat.ts:61`, `:93`, `:95`, `:1112`, `:1116` ;
   - `session.ts:17`, `:20`, `:75`, `:190` ;
   - `protocol.ts:417`.
4. **Variables CSS** : `--uh-accent-rgb` devient `--ya-accent-rgb` (`Chat.tsx:518`), `--uh-glow-angle` devient `--ya-glow-angle` (`Goal.tsx:241`, `:280`).
5. **Commentaires et identifiants restants** : `protocol.ts:5` ; test Rust `ya_turns.rs:411`, renommé `turn_start_serializes_with_its_wire_names`.
6. **Onglet Instructions supprimé** (si C9 = suppression) : fichiers et lignes du §4 ; règles CSS propres à l'onglet dans `ya-settings.css`, si elles ne servent à rien d'autre.
7. **Doc** :
   - `docs/fork-changes.md`, nouvelle section « Identité yusAgent (YA-10, ticket 32) », fichiers du fork et repris seulement ;
   - ligne du journal du CDC ;
   - ce ticket ;
   - `extensions/vscode/README.md`, si une méthode y est citée.

**Contrôles lancés par Claude Code sur le binaire de la branche** (règle 13, sortie jointe au ticket), avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short` :
- `acp_frames.py`, puis `compare_frames.py` (agent factice) et `compare_frames.py --against` la capture de `main` : attendu `écarts : 0` deux fois ;
- `delete_files_check.py` et `empty_delete_probe.py` : `CONFORME`, exit 0 ;
- `run.py --capture` sur les deux binaires, puis `compare_capture_prompts.py` : `TOUT IDENTIQUE` ;
- `git grep -n -i unharnessed -- extensions/vscode/src extensions/vscode/webview crates bench prime-agent-runtime` : il ne doit rester que les lignes d'attribution du §1.4.

### 7.3 Contrôle manuel de Yusuf (VS Code)

0. **Avant de commencer**
   - binaire de la branche : `B=~/.cache/yusagent-bin/<sha du push 2>/yusagent` ;
   - `.vsix` : `extensions/vscode/scripts/install-vsix.sh <run ya-extension du push 2>` ;
   - dans VS Code : `"yusagent.binaryPath": "<valeur de $B>"`, puis « Reload Window » ;
   - dossier de test : `mkdir -p /tmp/ya32 && code /tmp/ya32`.
1. **Nouvelle conversation, `bonjour`, puis ferme le chat.** Ouvre l'historique et supprime cette conversation. Attendu :
   - elle disparaît de la liste ;
   - `ls ~/.yusagent/sessions/ | grep <id>` ne montre que `<id>.usage.ndjson` ;
   - dans Output > yusAgent, aucune ligne « pas encore servie ».
2. **Nouvelle conversation, quittée vide.** Attendu : aucune trace dans l'historique.
3. **Rouvre depuis l'historique une conversation créée avant la branche**, par exemple celle de `/tmp/ya31-x3` si elle existe encore, ou une autre. Attendu :
   - rejeu identique : messages, cellules, lignes `Skills updated: …` à leur place ;
   - un nouveau message reçoit une réponse.
4. **Settings.** Attendu : onglets General, MCP et Skills, sans Instructions ; écrans stylés comme au §9.15 du ticket 31.
5. **Output > yusAgent** : `_unharnessed` ne doit apparaître dans aucune ligne.
6. **(Facultatif, si l'option A est retenue.)** `.vsix` de `main` avec le binaire de la branche, puis suppression depuis l'historique. Attendu : l'erreur s'affiche, la conversation reste et son fichier est intact.
7. **Remise en état** : `rm -rf /tmp/ya32` ; remettre `yusagent.binaryPath` à sa valeur d'avant.

## 8. Décisions à prendre par Yusuf

1. Ancien nom de la suppression : option A (coupure nette, proposée) ou B (les deux un temps) (§2.3).
2. `turnStart`, `turnEnd`, `saved` : garder (option 1, proposée) ou passer en `_yus/*` (option 2) (§2.4).
3. `_yus/sessions/delete` ou `_yus/session/delete` (§3 ; proposé : `sessions`).
4. Onglet Instructions : supprimer maintenant (proposé) ou garder pour la passe de design (§4).
5. `--unharnessed` de `ya09-bash-count.py` : garder (proposé : il désigne le dépôt mesuré) (§1.3).

## 9. Décisions de Yusuf (25/11/2026)

1. **A** : coupure nette. Le pont ne sert que `_yus/sessions/delete`, sans l'ancien nom.
2. **Option 1** : `turnStart`, `turnEnd` et `saved` gardés. Aucun fichier upstream touché.
3. **`_yus/sessions/delete`** (pluriel). Les méthodes non servies passent aux noms `_yus/*` du §3.
4. **Onglet Instructions supprimé maintenant**, code et tests compris.
5. **Option `--unharnessed` de `ya09-bash-count.py` gardée.**
6. **Ajouts** :
   - suppression du code mort qui lit `_meta.unharnessed` (`src/ya/meta.ts:17`, `:37`, `src/chat.ts:1112`, `src/session.ts:190`) ;
   - `--uh-accent-rgb` et `--uh-glow-angle` passent à `--ya-accent-rgb` et `--ya-glow-angle`.
7. **T5** : liste d'exceptions explicite et commentée dans le test (attribution, `turnStart`, `turnEnd`, `saved`, l'option du banc), pour qu'une citation légitime ne le fasse pas passer au rouge.
8. **Contrôles du push 2** : `compare_capture_prompts.py` (préfixe inchangé) et `compare_frames.py` (0 écart). Compte rendu à chaque push. **Le merge attend le contrôle de Yusuf dans VS Code** (§7.3).

**Écart au §7, signalé à Yusuf** : la branche `ya-32-identite` part de `ya-32-constat` (constat non mergé, non poussé) et non de `main` après son merge. La PR porte donc le constat, ces décisions, les tests et le code.

## 10. Réalisation (PR #88)

### 10.1 Push 1 : tests seuls (`ca33f9f82`)

- **`ci`** run 45590734542 : **2 rouges, attendus et sur leur assertion** ; fmt, clippy (Linux et Windows), deny, runtime et les 6 autres shards verts.
  - shard 3 : T1 `the_delete_is_served_under_the_yus_name_only` (`ya_history_tests.rs:340`, « the yusAgent name is served ») ;
  - shard 2 : T2 `the_history_lists_conversations_and_deletes_one_with_its_session` (`ya_acp_resume_e2e.rs:430`). L'ancien nom répond `-32602` (« This is the conversation in progress ») au lieu de `-32601` : il est encore servi.
- **`ya-extension`** run 45590734560 :
  - unitaires : 70 tests, 65 verts, **5 rouges, attendus et sur leur assertion** :
    - T4 `yaMeta` : `{ promptTurnId: 1 }` au lieu de `{}` ;
    - T4 `yaResultMeta` : `{ path: '/u' }` au lieu de `{}` ;
    - T3 méthodes servies : `_yus/sessions/delete`, `false` au lieu de `true` ;
    - T5 identité : la liste `fichier:ligne` des noms restants (méthodes, clé `_meta.unharnessed`, `--uh-*`, onglet Instructions, sondes du banc, constante du pont, nom du test de `ya_turns.rs`) ;
    - T5 fichiers de l'onglet Instructions : encore présents ;
  - e2e de l'agent factice : 15/15 ;
  - e2e du vrai binaire : 18/18, aucun processus restant.
- Changelog fragment, codebase-health, contribution gate : verts.
- En local (Claude Code) : `npm run typecheck && npm run lint && npm run test:unit` donne les mêmes 5 rouges.

**Écart au §7.1, signalé à Yusuf** : T5 parcourt aussi `crates/`, `bench/` et le runtime Python, et pas seulement l'extension. L'exception de l'option du banc (décision 7) suppose `bench/` dans le périmètre. Limite : le job `ya-extension` ne tourne que si l'extension ou son workflow change. Un nom d'Unharnessed réintroduit par un changement Rust seul ne serait vu qu'au prochain changement de l'extension.

### 10.2 Push 2 : le code

Commits, un sujet chacun :
- `37715c83c` : suppression sous `_yus/sessions/delete` seule (pont, extension, agent factice, banc) ;
- `d247ec65a` : méthodes et notifications non servies sous `_yus/*` ;
- `d21848449` : plus de lecture de `_meta.unharnessed`. Les commandes de l'agent sont toutes des commandes : notre pont n'envoie pas de `available_commands_update` ;
- `e6e84afeb` : `--uh-*` passent à `--ya-*` ;
- `62c4f5970` : commentaire `protocol.ts:5` et nom de test de `ya_turns.rs` ;
- `9e519b296` : onglet Instructions supprimé (code, types, règles CSS `.group-body .row-end` et `.input.is-area`, propres à l'onglet) ;
- doc : `docs/fork-changes.md` (section « Identité yusAgent »), journal du CDC, ce ticket.

En local (Claude Code) : typecheck, lint et build OK. `npm run test:unit` : `ℹ tests 70`, `ℹ pass 70`, `ℹ fail 0`. Le Rust est seulement formaté (`rustfmt --check`) ; il est compilé en CI (règle 8).

**Références de `main` pour les comparaisons.** Le binaire en cache `8e0ea28d4` a des `crates/` et un runtime identiques à `main` (`git diff --stat 8e0ea28d4 085d06962 -- crates prime-agent-runtime` vide). Avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short` :
- `python3 -I bench/smoke/run.py --binary <8e0ea28d4>/yusagent --model gpt-6.1-sol --tasks py-example-001 --capture` : `resolved`, dans `bench/smoke/runs/20261125T100418Z-8e0ea28-gpt-6.1-sol` ;
- `python3 -I bench/tools/capture/acp_frames.py --binary <8e0ea28d4>/yusagent --out <dossier>` : A 26 trames, B 14, C 44, `CONFORME`.
