# Ticket 24 : YA-10 minimal, extension VS Code (constat et plan)

Demande de Yusuf du 21/11/2026. Branche `ya-24-extension`, depuis `main` à `746a95559` (merge de la PR #71, ticket 23 clos). **Constat et plan seulement** : aucun code écrit, rien compilé, aucun appel de modèle. Pas de code avant les décisions de Yusuf (section 6).

Référence : YA-10 du CDC (`docs/yusAgent-cdc.md:365-391`).

## Décisions de Yusuf (21/11/2026)

- **E1** : `extensions/vscode/`.
- **E2** : client ACP écrit à la main.
- **E3** : **React en TypeScript** (CDC), pour ne pas réécrire l'interface quand elle grossira après les testeurs. CSP stricte, sans script inline, tout groupé par esbuild. Taille du .vsix mesurée dans la PR 2.
- **E4** : réglage `yusagent.binaryPath`, binaire hors du .vsix.
- **E5** : option B, plafond de **8 000 caractères par appel**, pour l'affichage seulement (rien de plus vers le modèle).
- **E6** : option (a), avec deux garde-fous :
  - « Rejeter » n'est disponible que lorsqu'aucun tour n'est en cours ;
  - la restauration est refusée, avec un message, si le contenu actuel du fichier n'est plus celui laissé par cette modification (modification plus récente, ou changement de Yusuf).
- **E7** : rien envoyé à l'ouverture.
- **E8** : bandeau calculé par l'extension.
- **E9** : `$/cancel_request` envoyé par le pont, et fermeture de la boîte par l'extension.
- **E10** : dans la PR 1.
- **E11** : ticket à part, avant les testeurs.
- **E12** : correctif dans ce ticket s'il reste dans le pont ou le worker.
- **E13** : `ubuntu-latest` + `xvfb` ; check requis à partir de la PR 3.
- **E14, E15** : reportés.
- **PR 1, en plus** :
  - mesurer par capture l'égalité du préfixe entre le chemin ACP et `-p`. **Si le préfixe diffère : arrêt et compte rendu à Yusuf avant d'aller plus loin** ;
  - vérifier que changer de modèle ou d'effort en cours de session est journalisé comme invalidation de cache (`docs/yusAgent-cdc.md:19`).
- **Journal du CDC** : écarts portés le 21/11/2026 (`docs/yusAgent-cdc.md:92`).
- PR 1 sur la même branche, pushs regroupés, pas de merge.

## 1. Inventaire

### 1.1 Aucun client éditeur, ni chez nous ni chez upstream

- **Notre dépôt** : aucun `package.json` hors des tâches du banc, aucun `.vsix`, aucun `extension.ts`. Commandes : `find . -path ./target -prune -o -path ./vendor -prune -o \( -name package.json -o -name "*.vsix" -o -name extension.ts \) -print` (seul résultat : `bench/smoke/tasks/node-example-001/repo/package.json`) et `git ls-files | grep -i -E "vscode|zed|extension|\.vsix|acp"`.
- **Upstream** : `upstream/main` récupéré le 21/11/2026 (`783acb5d7`) et point de fork `c83ac68b3`. `git ls-tree -r --name-only upstream/main | grep "\.ts$"` : 0 fichier. `git grep -i -E "\bzed\b|vs ?code|vscode" upstream/main` : seulement des en-têtes HTTP de GitHub Copilot (`crates/pa-ai/src/oauth/github_copilot.rs:19-21`) et la détection du terminal intégré de VS Code dans le TUI (`crates/pa-tui/src/hyperlinks.rs:131`, `crates/pa-tui/src/enhanced_keys.rs:705`). **Upstream n'a ni extension VS Code ni client ACP.**
- **Ce qui existe côté ACP** : seulement l'agent. Le corpus `crates/pa-daemon/testdata/acp/` capture l'ancien binaire TypeScript d'upstream parlant ACP (`README.md:1-11`) ; il sert de référence aux tests du pont Rust, pas de client.

L'extension est donc entièrement à écrire. Le pont ACP du binaire est le seul point d'appui.

### 1.2 Lancement et forme du pont

- **Lancement** : `yusagent --mode acp` (`crates/pa-cli/src/args.rs:49`, `:260-266`), ACP sur stdio, une trame JSON par ligne.
- **Le pont passe toujours par le démon** : `run_acp_mode` (`crates/pa-cli/src/print_runtime.rs:62`) crée une session du démon avec `executionMode: "acp"` (`:161`, télémétrie seulement) et se déclare répondant des permissions (`:164`).
  - Socket partagé par défaut. `--daemon-socket` l'isole (utilisé par le banc de capture, `bench/tools/capture/run.py:284-301`).
  - Un TUI peut donc s'attacher à la session de l'extension (K5, section 3).
- **Une session par processus** : un second `session/new` est refusé (`crates/pa-daemon/src/acp/daemon.rs:1127`). L'extension lance un processus par chat.
- **Le cwd est celui du processus.** Un `cwd` différent dans `session/new` est signalé dans `_meta.cwd` mais jamais adopté (`crates/pa-cli/tests/acp_mode_e2e.rs:454`, `acp_cwd_mismatch_is_reported_not_adopted`). L'extension lance donc le binaire avec `cwd` = dossier du workspace.

### 1.3 Méthodes du client vers l'agent

| Méthode | État | Preuve |
|---|---|---|
| `initialize` | servie ; `protocolVersion: 1`, `loadSession: false`, images et contexte embarqué acceptés, MCP HTTP, `session/close` ; `agentInfo` yusAgent ; **aucun `authMethods`** | `crates/pa-daemon/src/acp/daemon.rs:1052`, `crates/pa-daemon/src/acp/types.rs:387-410` |
| `session/new` | servie, une seule par processus | `daemon.rs:1055`, `:1127` |
| `session/prompt` | servie ; un second prompt pendant un tour est refusé | `daemon.rs:688`, `crates/pa-cli/tests/acp_mode_e2e.rs` (`acp_daemon_attached_overlapping_prompt_is_refused`) |
| `session/cancel` (notification) | servie ; arrête le travail du worker (`AbortAndClearQueue`), le prompt rend `stopReason: "cancelled"` | `daemon.rs:732`, `:886-894` ; test `acp_mode_e2e.rs:870` |
| `session/close` | servie | `daemon.rs:701` |
| `session/set_config_option` | servie : `model`, `thought_level` (effort, selon les niveaux du modèle), `permissions` (`manual`, `auto`) | `daemon.rs:1058`, `crates/pa-daemon/src/acp/wire_config.rs:31`, `crates/pa-daemon/src/acp/config_options.rs:124`, `:144`, `crates/pa-daemon/src/acp/ya_permissions.rs:299-318` |
| `session/load`, `authenticate`, `session/set_mode` | **absentes** : réponse `-32601` « Method not found » | `daemon.rs:1061-1068` |

### 1.4 De l'agent vers le client

- **`session/request_permission`** (ticket 20, PR 2) : `crates/pa-daemon/src/acp/ya_permissions.rs:42`, trame `:211-242`.
  - Deux options, `allow-once` (kind `allow_once`) et `reject-once` (kind `reject_once`). Pas d'`allow_always` (D3).
  - Réponse lue avant le parseur JSON-RPC (`:245-267`) : `selected` + `allow-once` autorise ; `cancelled` retire la demande ; tout le reste refuse (`:114-123`).
  - La demande attend la publication du `tool_call` de son appel (`:168-202`).
  - Un TUI attaché ne reçoit pas la demande ouverte (K5).
- **`session/update`** (`crates/pa-daemon/src/acp/wire_events.rs:69-406`) :
  - `agent_message_chunk` et `agent_thought_chunk` (`:108-115`). Le raisonnement est vide par défaut (`reasoningSummary: off`, décision du 15/11, `docs/yusAgent-cdc.md:65`).
  - `tool_call` pour une cellule (titre « Python cell », `rawInput: {code}`, `:130-162`), puis `tool_call_update` à la fin, `completed` ou `failed`, avec le texte du résultat (`:163-191`).
  - `bash_start`, `bash_output`, `bash_end` (`:194-249`) : le `!cmd` lancé par l'utilisateur, **pas** les `bash()` du kernel.
  - `session_info_update` sous `_meta` : objectif, compaction, sous-agents `rlm`, `/refine`, messages entre agents (`:250-404`).
- **Rien d'autre** :
  - pas de `plan` ;
  - pas d'usage ni de cache ;
  - pas de sortie partielle de cellule : le worker émet bien `tool_execution_update` (`crates/pa-daemon/src/worker/turn.rs:822`), mais le pont l'ignore (`wire_events.rs:404`) ;
  - pas de contenu `diff` : seul `diffCount` part sous `_meta` (`wire_events.rs:459-502`) ;
  - pas de terminal.

### 1.5 Ce que contient déjà le résultat d'une cellule (pour les cellules et les diffs)

`details` du résultat de `python_repl` (`crates/pa-core/src/tools/ipython.rs:443-506`) :

- `stdout`, `stderr`, `result`, `error`, `durationMs`, `status` ;
- **`bashCommands`** (`:463-469`) : seulement la **première** commande, le **nombre** de commandes et le total de lignes. L'hôte agrège une MIME d'affichage émise à chaque `bash()` (`prime-agent-runtime/src/rlm/bash.py:1385-1392`, agrégation `crates/pa-core/src/kernel/manager/events.rs:219-237`) ;
- **code de sortie** : seulement le dernier code non nul **lu** par la cellule (`bash.py:179-186`, `events.rs:239-246`, ajout du fork pour le masquage) ;
- **`diffs`** (`:484-499`) : `path` et un diff déjà rendu (`generate_diff_string`) pour chaque `edit()`. Ni `oldStr`/`newStr`, ni ligne de départ séparée.

Donc :

- « chaque `bash()` avec commande, code de sortie, sortie repliable » **n'existe pas** dans les données : ni la liste des commandes, ni le code de chaque appel, ni sa sortie (la sortie d'un `bash()` n'apparaît que si la cellule l'affiche) ;
- les diffs n'arrivent pas jusqu'au client ACP.

### 1.6 `edit()` écrit pendant la cellule

- Par défaut, le modèle n'a qu'un tool, `python_repl`. Les écritures passent par `edit()` dans le kernel (`crates/pa-core/src/session_engine/ya_tool_family.rs:9-22`).
  - `apply_patch` n'existe qu'avec `editTool: "native"` sur la famille Codex.
- En mode manuel, la cellule entière est approuvée **avant** de tourner (ticket 20, D2), mais son diff n'est connu **qu'après** (`details.diffs`).
- **Conséquence** : « accepter ou rejeter » un diff avant écriture est impossible sans arrêter le kernel à chaque `edit()`. « Rejeter » après coup veut dire restaurer le fichier, et le modèle doit le savoir (choix E6).

### 1.7 État des points notés au ticket 20

- **`pending` avant la décision : déjà corrigé côté ACP** (ticket 20, PR 2). En manuel, le `tool_call` part en `pending` (`crates/pa-daemon/src/acp/ya_permissions.rs:281-296`, appelé à `crates/pa-daemon/src/acp/daemon.rs:378`), puis passe en `in_progress` à l'accord (`ya_permissions.rs:146-164`). Test : `crates/pa-cli/tests/ya_permissions_acp_e2e.rs:387`.
  - La phrase du ticket 22 (`docs/tickets/22-carte-cellule-permission.md:79`, « annoncée `in_progress` avant la décision ») est **périmée**.
- **D4 sans événement dédié** : seul le libellé de l'option `auto` le dit (« Run tool calls without asking (no sandbox) », `crates/pa-daemon/src/acp/ya_permissions.rs:315`). Le pont n'envoie aucun `session_info_update` (ticket 20, `docs/tickets/20-ya11-permissions.md:396`).
- **Annulation d'une requête de l'agent** : le pont ne l'implémente pas (`grep -rn cancel_request crates` : rien). Sur un `ya_permission_resolved` venu d'un autre client, il oublie sa demande (`ya_permissions.rs:152`) sans rien envoyer au client ACP.

### 1.8 Spécification ACP (lue le 21/11/2026)

Pages lues : `agentclientprotocol.com/protocol/{overview,tool-calls,prompt-turn,session-config-options,schema}`, `/protocol/v1/{cancellation,terminals}.md`, `/rfds/request-cancellation.md`.

- **`$/cancel_request`** (`requestId`) : notification envoyée par le côté qui a émis la requête. Le receveur répond quand même : réponse valide (pour une permission, l'issue `cancelled`) ou erreur `-32800`. Le RFD indique « 2026-06-29: Moved to Completed and stabilized ». Son traitement reste **optionnel** pour les implémentations.
- **Après `session/cancel`** (v1) : le client « MUST respond to all pending `session/request_permission` requests with the `cancelled` outcome ». Notre pont l'accepte déjà (`ya_permissions.rs:118`).
- **`ToolCallStatus`** : `pending` (« awaiting approval »), `in_progress`, `completed`, `failed`.
  - La page du tour demande de marquer « `cancelled` » les appels non finis, valeur absente de l'énumération.
  - L'extension le fera en local, sans attendre de statut.
- **Contenu `diff`** : `path` (absolu obligatoire), `oldText` (absent pour un fichier neuf), `newText`. La spec ne dit pas si ce sont des fichiers entiers ou des fragments.
- **Catégories des `configOptions`** : `mode`, `model`, `model_config`, `thought_level`. Les noms qui commencent par `_` sont libres.
- **Terminal** : capacité client `terminal`, méthodes `terminal/*`, contenu `{"type":"terminal"}`. Reporté (section 2).
- **SDK TypeScript** officiel : `@agentclientprotocol/sdk`. Commande : `npm view @agentclientprotocol/sdk version dist.unpackedSize license time.modified` → `2.8.0`, `5030683` octets non compressés, `Apache-2.0`, modifié le `2026-11-18`. Compatibilité avec notre `protocolVersion: 1` **non vérifiée**.

## 2. Périmètre minimal proposé, comparé à YA-10

Base : la proposition de Yusuf. « Gardé » = dans ce ticket. « Reporté » = ticket ou version suivante, écart noté. « Écarté » = ne se fera pas sous cette forme.

| Point du CDC (`docs/yusAgent-cdc.md:367-390`) | Sort | Raison |
|---|---|---|
| Harness lancé par `child_process`, ACP sur stdio | **Gardé** | Seul point d'appui existant (1.2). |
| Redémarrage auto en cas de crash | **Gardé, réduit** | Le processus est relancé et une **nouvelle** session est ouverte, avec un message clair. Sans `session/load` (absent, 1.3), l'historique n'est pas réaffiché. Le fichier de session reste sur disque. |
| Webview avec CSP stricte | **Gardé** | CSP stricte (nonce, aucune source distante). React ou non : choix E3. |
| Streaming | **Gardé** | `agent_message_chunk` existe. |
| Plan | **Écarté** pour l'instant | L'agent n'émet pas de `plan` (1.4). Rien à afficher. |
| Raisonnement visible | **Gardé, passif** | Affiché s'il arrive (`agent_thought_chunk`). Vide par défaut depuis la décision du 15/11. L'interrupteur `reasoningSummary` est reporté. |
| Tool calls repliables | **Gardé** | Cellules repliables. |
| Coût et taux de cache en direct ; indicateur de contexte et de cache | **Reporté** | Le pont n'envoie aucun usage (1.4). Il faudrait un ajout au pont et au cœur (YA-13). **Écart** avec la décision du 31/10/2026 (`docs/yusAgent-cdc.md:18`), à noter au journal. |
| Sélecteur de modèle et d'effort | **Gardé** | `model` et `thought_level` existent dans `session/set_config_option` (1.3). Défaut xhigh : choix E7. |
| Cellules avec leur code, et pour chaque `bash()` : commande, code de sortie, sortie repliable | **Gardé, avec un ajout au cœur** | Le code de la cellule est déjà dans `rawInput`. Le détail par `bash()` n'existe pas (1.5) : niveau, choix E5. |
| Diffs via `vscode.diff`, accepter ou rejeter par hunk, `workspace.applyEdit` | **Gardé, réduit** | Diffs **après** application (1.6), « rejeter » le changement entier par restauration (choix E6). **Par hunk : reporté.** Le pont doit transmettre les diffs (1.4). |
| Permissions : une fois, toujours, refuser | **Gardé sans « toujours »** | D2 et D3 du ticket 20. « auto » pour la session par le sélecteur `permissions`. Écart déjà au journal (`docs/yusAgent-cdc.md:91`). |
| Avertissement persistant en « auto » sans sandbox (D4) | **Gardé** | Bandeau dans l'extension (choix E8). |
| `session/cancel` < 1 s, cellule et sous-processus `bash()` compris | **Gardé** | Bouton d'arrêt. **Aucun test ne mesure le délai** : `acp_mode_e2e.rs:870` annule une réponse lente du modèle, ni une cellule ni un `bash()`. Nouveau test e2e (section 5). |
| Aucune écriture disque sans flux diff/permission, sauf « auto » | **Gardé** | Tenu par le mode manuel : la cellule qui contient `edit()` est approuvée avant de tourner (D2). |
| Commandes dans un terminal en shell integration | **Reporté** | Demande de Yusuf. Le contenu `terminal` d'ACP demande des méthodes client `terminal/*` et un `bash()` qui passe par le client. |
| Clés dans `SecretStorage`, callback OAuth par `registerUriHandler` | **Reporté** | Section 4 : rien à construire pour les clés maintenant. |
| Captures d'écran et images | **Reporté** | Demande de Yusuf. Le pont accepte déjà les images (`types.rs:393`) ; il ne manque que l'interface. |
| Windows natif : exécution désactivée, WSL proposé | **Reporté** | Demande de Yusuf. Coût d'une détection minimale (message, pas de lancement) : faible, mais hors périmètre demandé. |
| .vsix < 5 Mo | **Gardé** | Vérifié en CI (section 5). |
| Fonctionne dans Zed (test manuel) | **Reporté** | Demande de Yusuf. |
| Avis `[bash-done]` groupés dans le démon | **À trancher** (E11) | Section 3. |

## 3. Points déjà notés pour YA-10

| Point | Constat | Proposition |
|---|---|---|
| **Client ACP répondant après relance du worker** | Le répondant est enregistré à l'`attach` (`crates/pa-daemon/src/ya_permissions.rs:107-115`). Le pont s'attache une fois, avec `asker_fields()` (`crates/pa-daemon/src/acp/daemon.rs:634`). Après une relance, il n'est plus répondant jusqu'au prochain `attach`, et les demandes sont refusées aussitôt (`docs/tickets/20-ya11-permissions.md:396`). **Non lu** : ce que le pont voit d'une relance, et si un worker de session ACP peut être passivé (refusé si un client en est propriétaire, `crates/pa-daemon/src/supervisor/worker_lifecycle.rs:681-689`). | **Traiter** dans la PR 1. Le pont se réattache comme répondant après une relance. Test e2e : on tue le worker, puis le prompt suivant reçoit sa demande de permission. Choix E10. |
| **Boîte ACP restée ouverte quand le TUI a répondu** | 1.7 : le pont oublie la demande sans prévenir le client. | **Traiter** : le pont envoie `$/cancel_request` (1.8) pour une demande qu'il a envoyée et qu'un autre client a réglée. L'extension ferme aussi sa boîte au passage du `tool_call` hors de `pending`, par défense. Choix E9. |
| **Cellule annoncée `in_progress` avant la décision** | **Déjà corrigé** en ACP (1.7). | Rien dans le pont. L'extension affiche `pending` comme « en attente d'accord ». Corriger la phrase périmée du ticket 22 (`:79`) avec le premier push de code. |
| **Annulation d'une requête de l'agent vers le client** | Existe dans la spec et stabilisée (1.8) ; absente du pont. | Couvert par la ligne « boîte restée ouverte ». L'extension accepte aussi `$/cancel_request` en entrée et répond `cancelled`, comme la spec le demande. |
| **Avis `[bash-done]` regroupés seulement sans démon** | Critère d'acceptation de YA-10 (`docs/yusAgent-cdc.md:390`). Le démon livre toujours une `custom_message` seule par tour (`crates/pa-daemon/src/worker/queue.rs:347-352`). Le regroupement reste réservé aux hôtes sans démon (`crates/pa-core/src/session_engine/ya_bash_notice_batch.rs:1-23`, `crates/pa-daemon/src/bash_notices.rs:1-4`). | **Ticket à part, avant les testeurs** : travail dans les files du worker, indépendant de l'interface. Le critère de YA-10 reste ouvert jusque-là. Choix E11. |
| **K5 du ticket 22** (TUI attaché pendant une demande) | Le worker ne rejoue pas la demande ouverte à l'`attach` (`crates/pa-daemon/src/ya_permissions.rs:107-115`). | **Ticket à part** (décision de Yusuf au ticket 22). Hors de ce ticket. |

## 4. Connexion

### En développement (« OAuth abonnement pour provider »)

- **L'extension ne s'occupe pas de l'authentification.**
  - `initialize` n'annonce aucun `authMethods` et `authenticate` n'existe pas (1.3).
  - Le worker lit les identifiants dans `<dossier agent>/auth.json`, soit `~/.yusagent/auth.json` (`crates/pa-daemon/src/agent_engine/model.rs:178`, `AuthStorage::create(&self.config.agent_dir)`).
- **Login, une fois, dans le TUI** : `yusagent`, puis `/login` (`crates/pa-tui/src/session_ui/commands.rs:286`). Les identifiants sont écrits par `crates/pa-cli/src/subscription_login.rs:261` (Claude) et `crates/pa-cli/src/codex_subscription_login.rs:109` (Codex).
  - Le login Claude hérité n'est pas touché (règle 7).
- **Sans identifiants**, le prompt échoue avec `kind: "authentication"` (`crates/pa-daemon/src/acp/mod.rs:40-66`). L'extension affiche alors un message fixe : « lancer `yusagent` dans un terminal, puis `/login` ».
- **Environnement du processus** : celui de VS Code. Les variables de clés (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`…, `crates/pa-ai/src/env_api_keys.rs:10-13`) passent au binaire si elles y sont. Le harness les retire déjà du kernel (décision du 31/10). L'extension ne journalise jamais l'environnement (règle 9).

### Avant publication (rien à construire maintenant)

- Clés saisies dans l'extension, rangées dans `context.secrets` (`SecretStorage`), jamais dans `settings.json`, les journaux ou la webview.
- Transmission au binaire, à trancher le moment venu :
  - (a) variable d'environnement du seul provider choisi, au lancement du processus ;
  - (b) `authMethods` + `authenticate` dans le pont (nouveau code du fork, la clé passe sur stdio).
- `registerUriHandler` : pas nécessaire pour les logins par abonnement actuels, qui attendent un rappel sur localhost ou un collage manuel (`crates/pa-cli/src/codex_subscription_login.rs:1-3`). À rejuger si un provider impose un rappel `vscode://`.

## 5. Découpage, tests, taille, préfixe

### 5.1 PR prévues

1. **PR 1, cœur et pont (Rust, modules du fork)**, sans interface :
   - `$/cancel_request` pour les demandes réglées ailleurs (E9) ;
   - réattache du répondant après une relance du worker (E10) ;
   - données des `bash()` par appel (E5) et des diffs (E6) dans `details`, transmises au client ACP :
     - diffs en contenu `diff` ACP, chemin rendu absolu ;
     - `bash()` sous `_meta`, espace `_yus/*` (décision du 19/11/2026, `docs/yusAgent-cdc.md:89`) ;
   - test e2e d'annulation < 1 s : une cellule `await bash("sleep 30")`, `session/cancel`, puis on mesure le délai jusqu'à la réponse `cancelled` et on vérifie que le `sleep` a disparu.
     - Si le test est rouge, correctif dans ce ticket ou ticket à part (E12).
   - Fichiers upstream touchés : listés dans `docs/fork-changes.md`. Fragment `.changes/`.
2. **PR 2, squelette de l'extension** (`extensions/vscode/`, E1) :
   - lancement du processus, client ACP (E2), chat en streaming, bouton d'arrêt ;
   - sélecteurs modèle et effort, bandeau D4 ;
   - message « `/login` » ;
   - workflow CI de l'extension.
3. **PR 3, cellules, permissions, diffs** :
   - cartes de cellule (code, `bash()`, sortie repliable) ;
   - boîte de permission (« Autoriser » / « Refuser », fermée sur `$/cancel_request` ou sur le changement de statut) ;
   - diffs `vscode.diff` et « Rejeter » (E6) ;
   - e2e contre le vrai binaire.

### 5.2 Tests

- **Unitaires de l'extension** : client ACP (trames, requêtes croisées, `$/cancel_request`) et état du chat. `node --test`, sans dépendance.
- **e2e de l'extension en CI** : `@vscode/test-electron` (VS Code téléchargé dans le job, jamais en local, règle 8).
  - **Contre un agent factice** (script Node qui rejoue des trames ACP) : rendu, boîte de permission, annulation, fermeture de la boîte sur `$/cancel_request`.
  - **Contre le vrai binaire** avec le provider factice (`PRIME_AGENT_FAUX_SCRIPT`, comme `crates/pa-cli/tests/acp_mode_e2e.rs:216-235`) et un `--daemon-socket` isolé :
    - cellule avec `bash()` ;
    - accord puis refus (fichier créé, fichier absent) ;
    - « auto » + bandeau ;
    - annulation.
  - Aucun vrai modèle (règle 6).
  - Runner et origine du binaire : choix E13.
- **Workflow** : nouveau fichier du fork, `.github/workflows/ya-extension.yml`. `ci.yml` (upstream) n'est pas touché. Il fait :
  - `tsc --noEmit` et lint ;
  - tests unitaires ;
  - `vsce package` et contrôle de taille ;
  - e2e.
  - Check requis ou non par la protection de `main` : choix E13.
- **Scripts de contrôle remis à Yusuf** (règle 13) : une démo ACP sur le modèle de `bench/tools/ya_permissions_acp_demo.py`, lancée d'abord par Claude Code sur le binaire de la branche.

### 5.3 Taille du .vsix

- Critère du CDC : < 5 Mo. Le job CI échoue si le `.vsix` dépasse 5 000 000 octets, et la taille est écrite dans le journal du job.
- **Le binaire n'est pas dans le .vsix** (E4) : il ferait seul environ 59 Mo (mesure de l'audit d'entrée, `docs/yusAgent-cdc.md:566`).
- Estimation, **non mesurée** : quelques centaines de Ko, extension et webview groupées par esbuild. Le SDK ACP pèse 5 030 683 octets non compressés (1.8), mais une fois groupé, seul le code utilisé reste. Mesure dans la PR 2.

### 5.4 Ce qui garantit que l'extension ne change pas ce qui part au modèle

**Ce que l'extension peut changer** :

- `mcpServers` de `session/new` (ajoute des tools) ;
- `session/set_config_option` sur `model` ou `thought_level` (change de modèle ou d'effort) ;
- le contenu des blocs du prompt (`resource`, `resource_link` et images s'ajoutent au message, `crates/pa-daemon/src/acp/types.rs:243`).

Le prompt système et les tools viennent du binaire seul.

**Règles de l'extension** :

- `mcpServers: []` ;
- des blocs `text` seulement dans cette version ;
- **aucun `session/set_config_option` sans action de l'utilisateur**, donc rien à l'ouverture (E7) ;
- « Rejeter » un diff ajoute un texte fixe **à la fin** du message suivant (E6), jamais au préfixe.

**Preuves** :

1. **Test de l'extension** : l'e2e contre l'agent factice enregistre toutes les trames envoyées à l'ouverture et pour un prompt. Il les compare à un fixture versionné (`initialize`, `session/new {cwd, mcpServers: []}`, `session/prompt` en texte). Toute trame en plus fait échouer le test.
2. **Capture** : nouveau cas `acp-extension` dans `bench/tools/capture/run.py`.
   - Il rejoue ce fixture contre le binaire sous le serveur de capture, à côté du cas `acp` existant (`run.py:281-355`), en répondant `allow` aux demandes.
   - `bench/smoke/tools/compare_capture_prompts.py` entre les deux doit dire **TOUT IDENTIQUE**.
   - Le T9 du ticket 20 montre déjà que manuel et auto envoient les mêmes requêtes (`crates/pa-daemon/tests/ya_permissions_same_requests_e2e.rs:430`).
3. **PR 1** (ajouts dans `details`, MIME d'affichage du runtime) : capture `main` contre la branche, **TOUT IDENTIQUE** exigé.
   - Les `details` ne doivent jamais partir au provider. Le masquage les lit (`crates/pa-core/src/session_engine/masking.rs:319`), donc il faudra vérifier qu'il n'en tire aucun texte nouveau.
   - Une MIME d'affichage sans `text/plain` ne doit pas atteindre la sortie de la cellule.
   - La capture le prouve.

**Non mesuré, à mesurer dans la PR 1** : l'égalité du préfixe entre le chemin ACP (celui de l'extension) et `-p` (celui du banc). Le cas `acp` de la capture existe, mais je n'ai pas lu si `analyze.py` le compare à `s1-anthropic`. Si le préfixe diffère, le banc ne mesure pas ce que verront les testeurs.

## 6. Choix à trancher

1. **E1, emplacement** : `extensions/vscode/` dans ce dépôt (recommandé : un seul dépôt, la CI voit le binaire et l'extension ensemble) ou dépôt séparé.
2. **E2, client ACP** : écrit à la main en TypeScript, JSON-RPC ligne par ligne, seulement les méthodes de 1.3 et 1.4 (recommandé : aucune dépendance, aucun risque d'incompatibilité entre le SDK 2.8.0 et notre `protocolVersion: 1`, quelques centaines de lignes, estimation) ; ou `@agentclientprotocol/sdk`.
3. **E3, webview** : TypeScript sans framework (recommandé pour cette version minimale : moins de dépendances, CSP simple, taille minimale) ou React (CDC).
4. **E4, binaire** : réglage `yusagent.binaryPath` (défaut : `yusagent` dans le `PATH`), aucun binaire dans le .vsix (recommandé). Binaire embarqué par plateforme reporté à YA-16.
5. **E5, détail des `bash()`** :
   - **(A)** liste des commandes et code de sortie de chaque appel, sans sortie propre (la sortie de la cellule seulement) ;
   - **(B)** commande, code de sortie et sortie plafonnée de chaque appel, par une MIME d'affichage émise à la fin de chaque `bash()` (`prime-agent-runtime/src/rlm/bash.py`, fichier upstream, ajout minimal) et agrégée par l'hôte.
   - Recommandé : **(B)**, c'est ce que demande la proposition. Plafond par appel à fixer (proposition : 8 000 caractères).
   - Les commandes détachées qui finissent après la cellule arrivent par l'avis `[bash-done]`, sur une carte à part.
6. **E6, diffs** :
   - **(a)** diff après application dans `vscode.diff`, bouton « Rejeter » qui restaure le fichier par `workspace.applyEdit` et ajoute un texte fixe visible (`[user-reverted] <chemin>`) à la fin du message suivant de l'utilisateur ;
   - **(b)** diff en lecture seule, rejet reporté ;
   - **(c)** accord par `edit()` avant l'écriture (le kernel attend une réponse) : lourd, touche le runtime et le protocole.
   - Recommandé : **(a)**. Le diff du pont passe en contenu `diff` ACP. Il faut ajouter `oldStr`/`newStr` et la ligne de départ dans `details.diffs`, pour que l'extension reconstruise l'avant à partir du fichier sur disque.
7. **E7, effort par défaut** : l'extension n'envoie rien à l'ouverture. Le défaut vient de `defaultThinkingLevel` des réglages (`crates/pa-core/src/settings/load.rs:19`), et « xhigh par défaut » (CDC, YA-12) se règle dans le cœur, ticket à part (recommandé : banc et extension partent du même défaut). Ou bien l'extension force xhigh à l'ouverture.
8. **E8, avertissement D4** : bandeau calculé par l'extension quand `permissions` vaut `auto`. Aucun sandbox n'existe encore, donc « auto » veut toujours dire « sans sandbox » (recommandé : rien dans le cœur). Ou un `session_info_update` dédié sous `_yus/*`, à ajouter avec le sandbox.
9. **E9, boîte restée ouverte** : `$/cancel_request` envoyé par le pont, plus fermeture par l'extension sur le changement de statut (recommandé). Ou l'extension seule.
10. **E10, relance du worker** : traitée dans la PR 1 (recommandé). Ou reportée, avec des demandes refusées aussitôt après une relance.
11. **E11, avis `[bash-done]` groupés dans le démon** : ticket à part avant les testeurs, critère de YA-10 gardé ouvert (recommandé). Ou dans ce ticket.
12. **E12, annulation < 1 s** : si le nouveau test est rouge, correctif dans ce ticket (recommandé si le correctif reste dans le pont ou le worker) ou ticket à part.
13. **E13, e2e de l'extension en CI** :
    - runner `ubuntu-latest` + `xvfb`, binaire Linux construit dans le job avec le cache cargo (recommandé : pas de dépendance à un autre workflow) ; ou macOS avec l'artefact de `ya-build-macos-arm64` ;
    - check requis par la protection de `main` ou non. Recommandé : non requis pendant la PR 2, requis à partir de la PR 3.
14. **E14, `session/load`** : reporté. Après un rechargement de la fenêtre, nouvelle session, l'ancienne reste sur disque (recommandé). Ou dans ce ticket.
15. **E15, sortie d'une cellule en cours** (`tool_execution_update`, ignoré par le pont) : reportée (recommandé ; les cartes se remplissent à la fin de la cellule). Ou transmise en `tool_call_update` `in_progress` dans la PR 1.

## Points ouverts

- Ce que le pont voit d'une relance du worker, et si un worker de session ACP peut être passivé : à lire au début de la PR 1 (section 3).
- Égalité du préfixe entre ACP et `-p` : non mesurée (5.4).
- Changement de modèle ou d'effort en cours de session « journalisé comme invalidation de cache » (`docs/yusAgent-cdc.md:19`) : non vérifié dans le pont.
- Écart à porter au journal du CDC après décision : indicateur de cache reporté (section 2). Les autres reports (Zed, Windows/WSL, terminal, captures, clés, par hunk) sont demandés par Yusuf ; à porter aussi.
- Ticket 22, `:79` : phrase périmée, **corrigée** dans le premier push de la PR 2.

## PR 1 : vérifications préalables (21/11/2026)

### Préfixe ACP contre `-p` : **différent, arrêt**

- **Binaire** : `main` (arbre de `746a95559`, identique à `5c17d0dc5` : `git diff --stat 5c17d0dc5 746a95559` vide), artefact du run `ya-build-macos-arm64` 44815536790, `SHA256SUMS` vérifié, `yusagent --version` = `0.10.2-continuous.5c17d0dc5…`.
- **Commande** : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py --binary <dossier>/yusagent --out <scratchpad>/cap-main`, puis comparaison des 4 requêtes du cas `s1-anthropic` (`-p`) avec les 4 du cas `acp` (`run.py:281-355`, même scénario `basic`, même projet, mêmes réglages, permissions « auto »), clé par clé (script de comparaison dans le scratchpad de Claude Code, à verser au banc si Yusuf le veut).
- **Correctif du banc nécessaire pour lancer la mesure** : la chauffe (`prepare`) n'écrivait pas `settings.json`, donc tournait en permissions « manual » et le binaire refusait de démarrer en `-p` (message de D6 du ticket 20). `bench/tools/capture/run.py` : `write_settings(warm / "agent")` ajouté dans `prepare`. Fichier du fork, sans effet sur ce qui est mesuré.
- **Résultat** : `tools`, `messages` et toutes les autres clés identiques sur les 4 requêtes ; **`system` diffère** sur les 4 (5 286 caractères en `-p`, 5 265 en ACP) :

  ```diff
  --- p (s1-anthropic)
  +++ acp
  @@ -41,2 +41,3 @@
   - `mcp`: call tools on the user's MCP services. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/mcp/SKILL.md`
  +- `rlm-heartbeat` (import `rlm_heartbeat`): schedule a recurring wake-up when the user asks for one. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/rlm-heartbeat/SKILL.md`

  @@ -45,3 +46 @@
   Image input: this model cannot see images; `attach_image` errors for it.
  -
  -This session has no daemon: heartbeats never fire. Commands that end after their cell wake you with a `[bash-done]` message; the run waits for running commands before it exits.
  ```

- **Origine** (comportement d'upstream, repris par le prompt v2) :
  - la ligne « no daemon » n'est ajoutée qu'aux sessions sans démon (`crates/pa-core/src/prompts/system_prompt.rs:378-391`, `options.daemonless`), au texte v2 de `crates/pa-core/src/prompts/ya_prompt_v2.rs:108` ;
  - le skill `rlm-heartbeat` n'est montré que si le gestionnaire `rlm_heartbeat.create` est enregistré (`crates/pa-core/src/session_engine/runtime_wiring.rs:196-216`, `model_visible_skills`) : c'est le cas dans le worker du démon, pas en `-p`.
- **Conséquence** : le banc (`-p`) ne mesure pas exactement le préfixe que verra l'extension (démon). Les deux sont stables chacun de son côté (aucun élément volatil dans le diff) ; l'écart est structurel : démon contre sans démon. Le TUI passe aussi par le démon : il a très probablement le préfixe d'ACP (non mesuré).
- **Arrêt** (décision de Yusuf) : rien d'autre dans la PR 1 avant sa décision.

### Changement de modèle ou d'effort : **pas journalisé comme invalidation**

- Le changement écrit une ligne `model_change` ou `thinking_level_change` dans le fichier de session (`crates/pa-core/src/session_engine/mod.rs:349-404`).
- Le journal d'usage écrit le provider, le modèle et l'effort de **chaque** appel (`crates/pa-core/src/session_engine/usage_log.rs:398-401`) : le changement s'y voit en comparant deux lignes.
- Mais aucun champ `invalidation` n'est écrit pour ce changement : le seul appel de `note_invalidation` est celui du masquage (`crates/pa-core/src/session_engine/masking.rs:177`). Le critère du CDC (`docs/yusAgent-cdc.md:19`, « changement en cours de session journalisé comme invalidation de cache ») **n'est pas tenu**.
- Pas de correctif sans décision : il toucherait `session_engine/mod.rs` (fichier upstream) ou le chemin de `set_model` du worker.

## Décisions de Yusuf (21/11/2026, suite)

- **Préfixe** : option (a). Écart porté au journal du CDC (`docs/yusAgent-cdc.md:93`) : deux lignes du prompt système liées au démon, chaque chemin stable, effet sous le seuil de détection du smoke bench. Comparaison permanente au banc de capture, qui échoue si l'écart dépasse ces deux lignes ; le TUI y entre s'il le peut sans gros chantier. Question « yusAgent garde-t-il les heartbeats ? » reportée après les testeurs.
- **Invalidation** : dans la PR 1. Entrée `invalidation` au prochain appel modèle après un `model_change` ou un `thinking_level_change`, avec la cause ; l'événement est journalisé sans présumer qu'un changement d'effort invalide le cache du provider. Rien de cela ne part au modèle : capture `main` contre branche, TOUT IDENTIQUE.

## PR 1 : cœur et pont ACP

Branche `ya-24-extension`. Pas de build ni de test Rust en local (règle 8) : formatage par `rustfmt`, compilation, clippy et tests par la CI. Détail des fichiers : `docs/fork-changes.md`, section « Extension VS Code, PR 1 ».

### Banc de capture : TUI et contrôle permanent du préfixe

- **Le TUI entre dans le banc sans gros chantier** : cas `tui` de `bench/tools/capture/run.py`. Le pilote de pty et l'émulateur d'écran de `bench/tools/ya_permissions_tui_demo.py` sont repris tels quels ; démon isolé, même scénario `basic`, sous `sandbox-exec`.
- `bench/tools/capture/prefix_paths_check.py`, lancé à la fin de `run.py` : `acp` et `tui` contre `s1-anthropic`, échec si l'écart dépasse les deux lignes.
- **Lancé par Claude Code sur le binaire de `main`** (règle 13 ; run 44815536790, même binaire que la mesure du préfixe) : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py --binary <dossier>/yusagent --out <scratchpad>/cap-main2`. Sortie : `tui : requêtes=4 erreur=None process restants=[]`, puis `CONFORME` (`acp` : 4 requêtes, 0 écart hors des deux lignes ; `tui` : 4 requêtes, 0 écart ; le TUI a exactement l'écart d'ACP).
- **Contrôle négatif** : une ligne ajoutée au prompt système d'une requête `acp` copiée (`Une ligne volatile 12:34`) donne `NON CONFORME`, code de sortie 1 (« requête 2 : démon seulement : 'Une ligne volatile 12:34' »).

### Invalidation sur changement de modèle ou d'effort

- Faite dans `crates/pa-core/src/session_engine/usage_log.rs` (fichier du fork), **sans fichier upstream** : chaque appel de la boucle est comparé au précédent (provider, modèle, effort envoyé). Cela couvre tous les chemins de changement : sélecteur ACP, `/model` et `/thinking` du TUI, bascule de secours et retour au modèle principal (`crates/pa-daemon/src/agent_engine/turn/model.rs:222`, `:313-314`, `:568-569`), que l'entrée de session `model_change` ne couvrait pas toujours.
- Forme : `{"cause": "model_change", "from": "provider/modèle", "to": "provider/modèle"}`, `{"cause": "thinking_level_change", "from": …, "to": …}` ; plusieurs causes (avec le masquage) : un tableau. Documenté dans `docs/usage.md`.
- Écart à la demande, à valider : la consigne visait un fichier upstream (`session_engine/mod.rs`) ; la détection à l'appel n'en touche aucun. Limite : un niveau d'effort ramené par le modèle au même effort envoyé n'est pas journalisé (rien ne change dans la requête).

### E9 et E10 : permissions

- **E9** : sur `ya_permission_resolved`, le pont envoie `$/cancel_request` (`requestId` = l'id de sa requête) pour une demande que son client tient encore ; la réponse tardive du client est ignorée. Tests unitaires dans `crates/pa-daemon/src/acp/ya_permissions.rs`.
- **E10** : avant chaque `session/prompt`, le pont envoie `ya_permissions` action `asker` (nouvelle) : le worker réenregistre le client routé comme répondant. Constat : à la relance, le superviseur rejoue la commande `create` durable (`crates/pa-daemon/src/supervisor/supervision.rs`, `relaunch_worker`) mais pas les `attach` des clients. Test e2e e (`a_relaunched_worker_still_asks_the_acp_client`) : worker tué par `SIGKILL` entre deux tours, la demande suivante arrive au client ACP.
  - Limite : une relance **au milieu** d'un tour n'a pas de répondant jusqu'au prompt suivant.

### E5 et diffs : données d'affichage des cellules

- **`bash()`** : le runtime émet `application/vnd.yusagent.bash-run+json` quand la cellule lit un résultat (même point que le code de sortie de T05, `_note_result_consumed`) : commande, code de sortie, sortie (8 000 caractères au plus, tête et queue avec le nombre omis), durée. L'hôte le range dans `details.bashRuns` ; le pont le passe sous `_meta` `_yus/bashRuns`.
  - Un `bash()` dont la cellule ne lit pas le résultat n'a pas d'enregistrement (son résultat arrive par l'avis `[bash-done]`).
- **Diffs** : `details.diffs` porte en plus `oldStr`, `newStr`, `startLine` ; le pont envoie un contenu ACP `diff` par `edit()` (`path` absolu, `oldText` absent pour un fichier créé, fragments et `_meta` `_yus/startLine`).
- `details` ne part pas au modèle ; les seuls lecteurs de `details.diffs` côté cœur lisent le chemin (`compaction_utils.rs:47-55`, `masking.rs:319-327`). Preuve attendue : capture `main` contre branche.
- **Tests Python lancés en local** (runtime, léger) : `uv run --python 3.11 python -m unittest discover -s test -p "test_ya_bash_run.py"` → 4 tests OK ; `-p "test_masked.py"` → 6 tests OK ; suite entière (`uv run --python 3.11 python -m unittest discover -s test`, commande du job `runtime suite` de `ci.yml`) → `Ran 819 tests`, `OK`.

### E12 : annulation en moins de 1 s

- Nouveau test `crates/pa-cli/tests/ya_acp_cancel_e2e.rs` : cellule bloquée sur `await bash('…; sleep 30')` (attente complète par défaut en prompt v2), `session/cancel` : réponse `cancelled` en moins de 1 s, commande disparue en moins de 1 s, suite de la cellule jamais exécutée, tour suivant servi par le même kernel ; cellule bloquée en Python (`time.sleep(60)`) : `cancelled` en moins de 1 s. Durée écrite sur stderr du test.
- Résultat attendu en CI. S'il est rouge : correctif dans ce ticket s'il reste dans le pont ou le worker (E12).

### CI et contrôles de la PR 1 (PR #72)

- **Premier run** `ci` 44883775053 sur `932f95b68` : `check + clippy`, `windows cross-check` et `test shard 3/8` en échec, une seule cause : erreur de compilation dans un test de `crates/pa-daemon/src/acp/ya_cell_display.rs` (`error[E0618]: expected function, found serde_json::Value`, variable locale `result` qui masquait la fonction de test du même nom). Corrigé dans `eaecae39c` (module `#[cfg(test)]` seulement : le binaire du produit ne change pas).
- Dans les shards verts de ce premier run : `ya_acp_cancel_e2e` (`a_cancel_stops_a_python_cell_within_the_bound ... ok`, `a_cancel_stops_an_awaited_bash_within_the_bound ... ok`, shard 2/8, job 136784609012) ; `a_relaunched_worker_still_asks_the_acp_client ... ok` (shard 5/8, job 136784609045) ; les trois tests d'invalidation du journal d'usage et `ipython_details_carry_the_cell_edit_diffs ... ok` (shard 2/8). **E12 : la borne de 1 s tient, pas de correctif nécessaire.**
- **Build macOS** de `932f95b68` : run 44883754650, succès, `SHA256SUMS` vérifié.
- **Captures `main` contre branche** (Claude Code, 21/11/2026, `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`) ; binaires `5c17d0dc5` (arbre de `main` `746a95559`, run 44815536790) et `932f95b68` (run 44883754650), extraits sous `~/.cache/yusagent-bin/<sha>/` :
  - smoke bench : `python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/<sha>/yusagent --model gpt-6.1-sol --tasks py-example-001 --capture --out <scratchpad>/smoke`, `resolved` des deux côtés ; `python3 -I bench/smoke/tools/compare_capture_prompts.py <run main> <run branche>` : **TOUT IDENTIQUE** ; `find ~/.yusagent ~/.prime/agent -newer <marque>` vide ;
  - banc de capture : `python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/<sha>/yusagent --out <scratchpad>/cap-…` pour les deux binaires, puis `python3 -I bench/tools/capture/compare.py <cap main> <cap branche> --all` : 13 cas, **toutes les requêtes identiques** (UUID normalisés, seul `prompt_cache_key` diffère, identifiant de session) ; cas `acp` et `tui` comparés clé par clé (4 requêtes chacun) : **TOUT IDENTIQUE** ;
  - contrôle des chemins sur la branche : `prefix-paths-report.md` **CONFORME** ;
  - journaux d'usage de la capture : 3 entrées `invalidation` (`masking`, cas `mask-on-anthropic`) des deux côtés, aucune autre : pas de fausse invalidation. Aucun cas de la capture ne change de modèle : le cas positif n'est couvert que par les tests unitaires.
- **Second run** `ci` 44884821338 sur `eaecae39c` : seul `test shard 3/8` en échec (job 136787783737), **défaut réel de E9** attrapé par un test existant du fork : `a_closed_request_drops_the_late_response_and_an_allowed_call_runs`, panique « nothing to ask » (`crates/pa-daemon/src/acp/ya_permissions.rs:531`). Une demande encore **retenue** par le pont (son `tool_call` pas encore publié) est déjà inscrite parmi les envoyées ; retirée avant d'être livrée, elle recevait quand même un `$/cancel_request`, pour une requête que le client n'a jamais vue. Correctif : la demande retenue retirée n'est pas annulée chez le client. Les autres jobs requis de ce run sont verts.
- **Troisième run** `ci` 44885855759 sur `3ffc2359c` (dernier commit poussé) : **14 checks requis verts** (`gh pr checks 72 --required`), aucun en skipping.
- **Build macOS** de `3ffc2359c` : run 44885851682, succès, `SHA256SUMS` vérifié.
- **Captures sur le binaire final** `3ffc2359c` contre `main` (`5c17d0dc5`), mêmes commandes que ci-dessus : smoke bench `py-example-001` `resolved`, `compare_capture_prompts.py` **TOUT IDENTIQUE** ; banc de capture, `compare.py --all` : 13 cas, toutes les requêtes identiques ; `acp` et `tui` : **TOUT IDENTIQUE** ; contrôle des chemins **CONFORME** ; `find ~/.yusagent ~/.prime/agent -newer <marque>` vide.
- **Préfixe inchangé.**

### Points ouverts de la PR 1

- Invalidation : faite dans le journal d'usage (fichier du fork), pas dans `session_engine/mod.rs` (upstream) comme le supposait la consigne. **Écart accepté par Yusuf (21/11/2026).**
- Banc de capture, venv du kernel partagé : tous les binaires utilisent le même venv (`~/.cache/yusagent-capture/home/.yusagent/kernel-venv`), reconstruit quand le hash des sources du runtime change (`crates/pa-core/src/kernel/bootstrap/venv/runtime_source.rs:63`). La PR modifie `bash.py` : passer de `main` à la branche (ou l'inverse) demande une reconstruction, qui peut échouer sous le sandbox sans réseau (vu une fois : 88 requêtes de `main` avec « kernel startup failed … uv venv … --seed failed with exit code 2 »). Parade : retirer le marqueur `prepared-<16 premiers caractères du sha256 du binaire>` avant chaque run, pour refaire la chauffe avec réseau. Les captures notées plus haut ne contiennent aucun échec du kernel (vérifié : 0 requête avec « kernel startup failed » dans chacune).
- Couverture des captures : aucune ne contient de `bash()` non lu ni d'`edit()` (les modifications de `smoke-py-example-001` passent par `pathlib`, `bench/tools/capture/server.py:193-195`) ; les nouveaux champs `oldStr`/`newStr`/`startLine` de `details.diffs` ne sont donc pas exercés par la comparaison avec `main`. Cas à ajouter proposé à Yusuf avant le merge.
- Relance du worker au milieu d'un tour : pas de répondant jusqu'au prompt suivant.
- `bash()` dont la cellule ne lit pas le résultat : pas d'enregistrement `bash-run` (résultat par l'avis `[bash-done]`).
- Sortie partielle d'une cellule en cours (E15) et `session/load` (E14) : reportés.
- Phrase périmée du ticket 22 (`:79`) : **corrigée** dans le premier push de la PR 2.

### Avant le merge : cas `display-anthropic` et comparaison `acp`/`tui` (demande de Yusuf, 21/11/2026)

- **Cas `display-anthropic`** (scénario `display`, `bench/tools/capture/server.py`) : `bash()` lu (`r = await bash('echo lu')`), `bash()` non lu (`h = bash('sleep 3; echo non-lu')`), `edit()` de création puis de modification dans `ya-display/`, retiré par la dernière cellule (sinon le `git status` du scénario `basic` des cas suivants, dont `acp` et `tui`, changerait).
- **Déterminisme** : une commande qu'aucune cellule ne lit reçoit toujours un avis `[bash-done]` après la fin de sa cellule (`prime-agent-runtime/src/rlm/bash.py:890-917`). S'il arrivait juste à la fin de la cellule, il serait en concurrence avec la requête suivante. Le scénario fait finir la commande (3 s) après la réponse finale : l'avis arrive toujours dans un **7ᵉ appel**, tour de réveil à part. Le texte de l'avis porte le pid (`[bash-done pid:N exit:0]`, `crates/pa-core/src/session_engine/messages.rs:225`), qui change à chaque lancement : `compare.py` le normalise comme les UUID (`<pid>`). Deux runs du cas sur le même binaire (`3ffc2359c`) : 7 requêtes, **7 identiques** (pids bruts 45801 et 45857).
- **`display_check.py`** (sur la branche) : une seule entrée `details.bashRuns` (`{"command": "echo lu", "exitCode": 0, "output": "lu\n", "durationMs": 6}`), rien pour le `bash()` non lu ; deux `details.diffs` (`oldStr` `""` → `newStr` `"un\n"`, `startLine` 1 ; `"un"` → `"deux"`, `startLine` 1) ; dernière requête = avis de la commande non lue. **CONFORME**. Sur `main` : NON CONFORME, attendu (pas de `bashRuns` ni de champs bruts).
- **`compare.py --all`** compare aussi `acp` et `tui` et finit par `TOUT IDENTIQUE` ou `DIFFÉRENCES`. Contrôle négatif : `max_tokens` + 1 dans une requête `acp` copiée → `| acp | 4 | 4 | 3 | 1 |`, `DIFFÉRENCES`.
- **Séquence complète relancée** (Claude Code, binaires `5c17d0dc5` et `3ffc2359c`, préparation forcée par `rm -f ~/.cache/yusagent-capture/prepared-<…>` avant chaque run) : `compare.py --all` : 16 lignes (13 cas, `display-anthropic`, `acp`, `tui`), **TOUT IDENTIQUE** ; `prefix_paths_check.py` **CONFORME** ; `display_check.py` **CONFORME** ; `grep -l "kernel startup failed"` : **0**.
- **Rien sous `crates/`** dans ces commits (outils du banc et doc) : pas de nouveau build, le binaire de contrôle reste `3ffc2359c` (run 44885851682).

### Contrôle de Yusuf sur `3ffc2359c` (branche à `96e767628`) et correctif du banc

- **Résultat de Yusuf** : smoke `py-example-001` `resolved` deux fois, `find` vide, `TOUT IDENTIQUE` (9 requêtes) ; banc : les deux `run.py` `CONFORME`, `compare.py --all` `TOUT IDENTIQUE` (16 lignes), `prefix_paths_check.py` `CONFORME`, `grep` 0 ; **`display_check.py` NON CONFORME** sur `ya24-branche`.
- **Cause** (constat de Claude Code, rien modifié avant) : les dossiers `--out` `ya24-main` et `ya24-branche` avaient déjà servi au run de Claude Code ; `run.py` réécrit les fichiers mais copie la session de `display-anthropic` sans vider le dossier (`copytree(..., dirs_exist_ok=True)`), et `display_check.py` lisait toutes les sessions : deux sessions (09:25 et 09:44 UTC), donc deux entrées `bashRuns` et quatre diffs, d'où les NON. La session du run de Yusuf, seule, est conforme (1 entrée `bashRuns`, 2 diffs avec `oldStr`, `newStr`, `startLine`). Les commandes remises ne contenaient pas le `rm -rf` des dossiers de run.
- **Hypothèse du venv périmé écartée** : `bash.py` de `main` `faf539ffb6db566f…`, de la branche `d0523d0d6badf640…` (sha256) ; empreinte du runtime (`runtime_source.rs:63`) `75da30aea5ba906d…` et `bd4201dcc134c59e…` ; le venv partagé porte celui de la branche (`.bootstrap-version` `sha256:bd4201dcc134c59e…`). Le run de branche a émis `bashRuns` (seul le `bash.py` de la branche le fait) ; le run de `main` n'a eu aucun échec du kernel, or un écart d'empreinte force une reconstruction (`crates/pa-core/src/kernel/bootstrap/mod.rs:237-279`) qui échoue sans réseau : il a donc tourné avec le runtime de `main` (déduction, rien ne journalise le runtime par run).
- **Comparaisons valables** : dans les deux runs de Yusuf, les 130 corps de requête sont tous postérieurs au début du run ; les runs du smoke sont dans des dossiers datés neufs (9 corps, 0 échec du kernel).
- **Binaire** : pas de runtime périmé pour un utilisateur ; l'empreinte est recalculée à chaque démarrage du kernel et le venv unique (`~/.yusagent/kernel-venv`, `venv/layout.rs:28`) est reconstruit quand elle change (environ 30 s, réseau nécessaire ; hors ligne, erreur explicite).
- **Correctif du banc** (décision de Yusuf) :
  - `run.py` refuse un `--out` existant et non vide, avec la commande `rm -rf` à lancer ; il ne supprime rien ;
  - `display_check.py` refuse un dossier qui contient plus d'une session (code de sortie 1, pas de rapport) ;
  - **contrôles négatifs lancés par Claude Code** sur `bench/tools/capture/runs/ya24-branche` (dossier de Yusuf, rempli, deux sessions) : `run.py` → « --out … existe déjà et n'est pas vide : le supprimer (rm -rf …) ou choisir un autre dossier », code 1, 384 fichiers avant et après ; `display_check.py` → « NON CONFORME : 2 sessions dans … une seule attendue (dossier de run réutilisé ?) : supprimer le run et le relancer », code 1, `display-report.md` inchangé.

### Commandes de contrôle (banc de capture)

```sh
git fetch origin && git checkout ya-24-extension && git pull --ff-only && git status --short --branch
export DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short
M=5c17d0dc543e746b71577a2ccc025cf1b7ef4b14; B=3ffc2359c516afa36d33fd2038ef86d37a595df5
rm -rf bench/tools/capture/runs/ya24-main bench/tools/capture/runs/ya24-branche
rm -f ~/.cache/yusagent-capture/prepared-36d9e7323c0ed353
python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/$M/yusagent --out bench/tools/capture/runs/ya24-main
rm -f ~/.cache/yusagent-capture/prepared-178b7fbdb65acf20
python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/$B/yusagent --out bench/tools/capture/runs/ya24-branche
python3 -I bench/tools/capture/compare.py bench/tools/capture/runs/ya24-main bench/tools/capture/runs/ya24-branche --all --out bench/tools/capture/runs/ya24-compare.md
sed -n '/Toutes les requêtes/,$p' bench/tools/capture/runs/ya24-compare.md
python3 -I bench/tools/capture/prefix_paths_check.py bench/tools/capture/runs/ya24-branche; echo "exit=$?"
python3 -I bench/tools/capture/display_check.py bench/tools/capture/runs/ya24-branche; echo "exit=$?"
grep -l "kernel startup failed" bench/tools/capture/runs/ya24-*/cases/*/cap/*.body.json bench/tools/capture/runs/ya24-*/acp/cap/*.body.json bench/tools/capture/runs/ya24-*/tui/cap/*.body.json | wc -l
```

Attendu : chaque `run.py` affiche `préparation (réseau autorisé)`, `display-anthropic: exit=[0]`, `acp : prompt={'stopReason': 'end_turn'} process restants=[]`, `tui : requêtes=4 erreur=None process restants=[]` et `CONFORME` ; `compare.py` : 16 lignes à 0 différence, `TOUT IDENTIQUE` ; `prefix_paths_check.py` `CONFORME`, `exit=0` ; `display_check.py` sept `oui`, `CONFORME`, `exit=0` ; `grep` `0`.

### Contrôle de Yusuf de la PR 1 (binaire `3ffc2359c`) et merge

- **Smoke bench** `py-example-001` : `resolved` deux fois, `find ~/.yusagent ~/.prime/agent -newer <marque>` vide, `compare_capture_prompts.py` **TOUT IDENTIQUE**.
- **Banc de capture** (commandes ci-dessus, dossiers `--out` neufs) : `compare.py --all` **TOUT IDENTIQUE** sur 16 lignes, `prefix_paths_check.py` **CONFORME**, `display_check.py` **CONFORME**, `grep "kernel startup failed"` : 0.
- **Premier essai `display_check.py` NON CONFORME** : expliqué par le `--out` réutilisé (deux sessions dans le même dossier, section précédente) ; corrigé dans le banc (`--out` rempli refusé, une seule session pour `display_check.py`).
- **Merge** : PR #72 mergée, `main` à `1b96bb645` ; build macOS `ya-build-macos-arm64` 44893185253 en succès (artefact `yusagent-aarch64-apple-darwin-1b96bb6457e51d010ad32a2293714f911fea2162`, `SHA256SUMS` vérifié par Claude Code, `yusagent --version` = `0.10.2-continuous.1b96bb6457e51d010ad32a2293714f911fea2162`).

## PR 2 : squelette de l'extension (21/11/2026)

Branche `ya-24-extension-pr2`, depuis `main` à `1b96bb645`. Périmètre : section 5.1, point 2. **Aucun fichier sous `crates/` ni upstream** : le binaire ne change pas, le préfixe non plus. Détail des fichiers : `docs/fork-changes.md`, section « Extension VS Code, PR 2 ».

### Décisions de Yusuf sur l'interface (21/11/2026)

1. Chat dans un **onglet d'éditeur** ; « yusAgent: Nouveau chat » ouvre un onglet et lance un processus.
2. Bandeau en haut, messages au centre, saisie en bas (Entrée envoie, Maj+Entrée saut de ligne) ; « Envoyer » devient « Arrêter » pendant un tour.
3. Texte de l'agent **brut** (espaces et retours gardés). **Markdown avant les testeurs.**
4. Sélecteurs modèle et effort sous la saisie, `<select>` natifs au thème, grisés pendant un tour.
5. Pas de sélecteur `permissions` (lecture seule). **Le bandeau suit la valeur annoncée par l'agent** (`configOptions` de `session/new`, puis `config_option_update` et la réponse de `session/set_config_option`), jamais un fichier lu par l'extension.
6. Bandeau permanent, non fermable, couleurs d'avertissement du thème : « ⚠ Mode auto sans sandbox : les cellules s'exécutent sans demander, avec vos droits. »
7. Demande de permission en PR 2 : aucune réponse, ligne « Demande d'autorisation en attente (boîte en PR 3) : Arrêter pour annuler » ; « Arrêter » répond `cancelled`.
8. Une ligne par appel de tool (« Python cell · en attente d'accord / en cours / terminé / échoué / annulé ») ; raisonnement replié et grisé s'il arrive.
9. Binaire absent ou non exécutable : état d'erreur dans le chat, chemin essayé, « Ouvrir le réglage yusagent.binaryPath » et « Réessayer ».
10. Plantage : relance automatique, nouvelle session, historique grisé, tour en cours marqué échoué ; deuxième arrêt en moins de 60 s : plus de relance, bouton « Réessayer ».
11. Message d'authentification fixe, **avec le chemin réel du binaire** (réglage `yusagent.binaryPath` résolu en chemin absolu) : « Authentification requise : lancez `<chemin>` dans un terminal, puis /login. »
12. Sans dossier ouvert : pas de lancement, « Ouvrez un dossier » ; plusieurs dossiers : le premier, nom dans l'en-tête.

- **Cas `acp-extension` de la capture (preuve 2)** : dans cette PR 2 (annoncé avant le code).
- **Merge et check non requis** : `ya-extension.yml` n'est pas requis par la protection de `main` pendant la PR 2 (E13). **Le merge d'une PR qui touche `extensions/vscode/` exige ce workflow vert sur la tête, vérifié à la main** (`gh run list --workflow ya-extension.yml --branch <branche>` : run `success` dont le `headSha` est le dernier commit poussé ; le filtre de chemins porte sur tout le diff de la PR, donc chaque push d'une PR qui touche `extensions/vscode/` le relance), jusqu'à ce qu'il devienne requis (PR 3).

### Ce qui est fait

- **Processus** (`extensions/vscode/src/session.ts`) : `spawn(<binaryPath résolu>, ["--mode", "acp"], {cwd: <premier dossier du workspace>})`, un processus par chat ; stderr vers le canal de sortie « yusAgent » (jamais l'environnement, règle 9) ; fermeture de l'onglet : `session/close` (2 s au plus), fin de stdin, SIGTERM, SIGKILL après 2 s.
- **`yusagent.binaryPath`** (E4) : défaut `yusagent`, cherché dans le `PATH` ; `~/` développé (`src/binary.ts`). **Portée `machine`** (réglage utilisateur seulement) : un dépôt ouvert ne peut pas choisir, par ses réglages de workspace, le binaire que l'extension lance. Ajout de Claude Code, à valider.
- **Client ACP écrit à la main** (E2, `src/acp/connection.ts`, `client.ts`) : JSON-RPC une trame par ligne, requêtes croisées, `$/cancel_request` reçu (rappel de la requête visée, réponse `cancelled`), une seule réponse par requête, méthode inconnue `-32601`. Envoyés : `initialize` (capacités client à `false`, pas de `clientInfo`), `session/new {cwd, mcpServers: []}`, `session/prompt` (bloc `text`), `session/cancel`, `session/set_config_option` (seulement sur un changement de l'utilisateur, E7), `session/close`.
- **Arrêter** : `session/cancel`, puis `cancelled` à chaque demande d'accord ouverte (spec v1) ; appels pas finis marqués « annulé » en local. Une demande se ferme aussi sur `$/cancel_request` (E9) ou quand son appel sort de `pending`.
- **Webview** (E3) : React 18 en TypeScript, groupée par esbuild (`dist/webview.js`, `dist/webview.css`), CSP `default-src 'none'; style-src <webview>; font-src <webview>; img-src <webview>; script-src 'nonce-…'`, aucun script ni style inline, aucune source distante. Couleurs du thème VS Code. Sonde de test (`probe`) : la webview renvoie à l'hôte son texte affiché et ses sélecteurs, pour l'e2e ; rien ne sort de VS Code.
- **CI** : `.github/workflows/ya-extension.yml` (`npm ci --ignore-scripts`, `tsc --noEmit`, eslint, tests unitaires, `vsce package` avec contrôle < 5 000 000 octets et liste des fichiers, `.vsix` en artefact `yusagent-vsix-<sha>`, e2e `xvfb-run` dans VS Code 1.146.0).
- **Dépendances** (développement seulement, rien dans le `.vsix` hors du code groupé) : versions publiées depuis plus de deux semaines (`typescript-eslint` 8.72.1 et non 8.73.0, publié le 16/11/2026). `npm install` en local sans scripts d'installation (npm 11 les bloque ; esbuild prend son binaire dans sa dépendance de plateforme).

### Tests

- **Unitaires** (`npm run test:unit`, `node --test`) : 34 tests. Client ACP (8), état du chat (7), résolution du binaire (5), `ChatSession` contre l'agent factice, vrai processus et vraies trames sur stdio (14) : fixture des trames, Arrêter, `$/cancel_request`, sélecteurs, bandeau, authentification (deux formes), plantages (relance, arrêt au deuxième en moins de 60 s, relance à chaque fois au-delà), binaire absent, aucun dossier, version ACP inattendue, fermeture.
  - Lancé par Claude Code en local : `cd extensions/vscode && npm run typecheck && npm run lint && npm run test:unit` → `ℹ tests 34`, `ℹ pass 34`, `ℹ fail 0`.
  - Contrôle négatif : `clientInfo` ajouté à `initialize` → le test du fixture échoue (exit 1, diff `+ clientInfo`), aucun processus restant. Défaut trouvé au passage : un test en échec laissait l'agent factice vivant et `node --test` ne rendait pas la main ; corrigé (fermeture de chaque session après son test, `--test-timeout`).
- **e2e** (`test/e2e/`, CI seulement : VS Code téléchargé) : 9 tests dans l'hôte d'extension contre l'agent factice : trames du fixture et CSP de la page, texte streamé affiché, bouton Arrêter et `cancelled`, sélecteurs affichés et `set_config_option` sur un changement seulement, message `/login`, plantage et relance, `session/close` à la fermeture de l'onglet, bandeau (annoncé à l'ouverture, puis par `config_option_update`), binaire absent. Rendu vérifié par la sonde.

### Préfixe : preuves 1 et 2

- **Preuve 1** : le fixture `extensions/vscode/test/fixtures/acp-frames.json` (3 trames : `initialize`, `session/new`, `session/prompt`) est comparé aux trames reçues par l'agent factice, par les tests unitaires et par l'e2e. Une trame en plus ou différente fait échouer.
- **Preuve 2**, banc de capture, cas `acp-extension` : le fixture rejoué contre le binaire de `main` (`1b96bb645`, run 44893185253), avec le prompt du cas `acp`. Commande (Claude Code, 21/11/2026) : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/1b96bb6457e51d010ad32a2293714f911fea2162/yusagent --out <scratchpad>/cap-pr2`. Sortie : `acp-extension : prompt={'stopReason': 'end_turn'} process restants=[]`, `prefix_paths_check` **CONFORME**, `extension_frames_check` : trames envoyées = fixture `oui`, `acp` 4 requêtes, `acp-extension` 4, **4 identiques, 0 différente, TOUT IDENTIQUE** ; `grep -l "kernel startup failed"` : 0.
  - Contrôle négatif : `max_tokens` + 1 dans la requête 2 d'une copie de `acp-extension` → `| acp-extension | 4 | 3 | 1 |`, `DIFFÉRENCES`, exit 1.
  - `spawn : ok=False` dans ce run : déjà présent dans les runs `ya24-main` et `ya24-branche` de Yusuf (`run.json`), sans lien avec cette PR.
- **Prompt système et tools** : inchangés (aucun fichier du binaire touché).

### Constat : identifiants absents, `kind: "other"` et non `authentication`

- **Sur le vrai binaire** (`main` `1b96bb645`, HOME et dossier d'agent vides, sous `sandbox-exec`), le prompt échoue avec `{"code": -32603, "data": {"details": "yusagent turn failed: No API key found for amazon-bedrock.\n\nUse /login …", "kind": "other", "category": "other", "retryable": false}}`. La section 4 supposait `kind: "authentication"`.
- **Cause** : le texte vient de `crates/pa-daemon/src/agent_engine/turn/model.rs:68` ; aucune règle du classifieur ne le reconnaît (`crates/pa-core/src/session_engine/error_classify.rs:260-357` : pas de statut HTTP, le jeton `missing_api_key` de `:135` n'y figure pas, pas de correspondance exacte ; la ligne `:9` n'est qu'une description). `kind: "authentication"` n'arrive que sur un 401/403 ou un jeton reconnu.
- **Dans l'extension** (périmètre de la PR 2) : le message `/login` s'affiche sur `kind: "authentication"` **ou** sur un `details` qui contient « No API key found for » (`isAuthFailure`, `src/session.ts`), avec un test sur la forme relevée. **À valider par Yusuf.**
- **Hors périmètre, non fait** : classer ce texte en `credential_missing` côté pont ou classifieur (fichier upstream `error_classify.rs`, ou `crates/pa-daemon/src/acp/mod.rs`). Proposition : ticket à part ; la reconnaissance du texte dans l'extension pourra alors partir.

### Contrôle sur le vrai binaire (règle 13)

`extensions/vscode/scripts/real-binary-check.mjs` lance `ChatSession` (le code de l'extension, hors VS Code) sur le binaire, en permissions « manual » puis « auto ». Aucun modèle joignable : HOME et dossier d'agent vides, environnement réduit sans clé, `sandbox-exec` (profil du banc, réseau limité à localhost), démon isolé.

- Lancé par Claude Code sur le binaire de `main` : `cd extensions/vscode && npm run compile-tests && node scripts/real-binary-check.mjs ~/.cache/yusagent-bin/1b96bb6457e51d010ad32a2293714f911fea2162/yusagent` → pour chaque mode : session ouverte, options `model`, `thought_level` (`medium`, 3 valeurs), `permissions` = le mode ; bandeau absent en manuel, affiché en auto ; effort `medium → low` par `set_config_option` ; dernier message « Authentification requise : lancez <chemin> dans un terminal, puis /login. » ; aucun processus restant. Verdict **CONFORME**, exit 0.
- Premier lancement, avant `isAuthFailure` : **NON CONFORME** (2), le message `/login` absent : c'est ce lancement qui a révélé le constat ci-dessus.

### CI de la PR 2 (PR #73)

- **Premier push** (`95572b5ed`) : `ci` 44899188163, tous les jobs verts. `ya-extension` 44899188141 : `npm ci`, tsc, lint, tests unitaires, `.vsix` et artefact verts ; `.vsix` de **59 338 octets** (8 fichiers : `extension.js` 17 926 octets, `webview.js` 147 007, `webview.css` 2 457, `package.json`, `readme.md`, `LICENSE.txt`, manifeste). L'e2e (VS Code 1.146.0 téléchargé, xvfb) : 7 tests sur 9 verts (Arrêter, sélecteurs, `/login`, plantage, fermeture, bandeau ×2 : rendu vérifié par la sonde), puis **blocage** ; run annulé par Claude Code après 12 min sur l'étape e2e (et le run `push` 44899166566 en double, annulé aussi). Trois défauts, tous dans l'e2e ou à la fermeture :
  1. test de la CSP faux : `webview.cspSource` est une source `https://*.vscode-cdn.net`, que le test refusait (« https? ») ; il vérifie maintenant que styles, polices et images n'ont que cette source, scripts par nonce seulement, ni `unsafe-`, ni `data:`, ni `*` ;
  2. « Webview is disposed » : à la fermeture, le dernier état (`closed`) était envoyé à la webview fermée ; plus rien n'est envoyé après la fermeture ;
  3. sonde sans délai : un message posté avant le chargement de la webview est perdu, le test « binaire absent » attendait sans fin ; la sonde rend `undefined` au bout de 1 s, et le test attend la webview montée.
- Déclencheur `push` retiré de `ya-extension.yml` (deux runs par push d'une PR).
- **Deuxième push** (`f9d4ba83d`) : `ya-extension` 44900324582, e2e **8 sur 9** ; « Webview is disposed » disparu du journal (0 occurrence). Seul échec : le test de la CSP, encore. Dans VS Code 1.146, `webview.cspSource` vaut deux sources, `'self'` et `https://*.vscode-cdn.net`, et le test n'en attendait qu'une. Il accepte maintenant exactement ces deux sources, les mêmes pour styles, polices et images.

### Procédure de contrôle (Yusuf)

Le `.vsix` vient de l'artefact du dernier run `ya-extension` vert de la branche. `install-vsix.sh` est d'abord lancé par Claude Code sur cet artefact, dans un dossier d'extensions isolé (règle 13) ; sa sortie est dans le compte rendu de la PR.

```sh
git fetch origin && git checkout ya-24-extension-pr2 && git pull --ff-only
RUN=$(gh run list --repo YusufByn/yusagent --workflow ya-extension.yml --branch ya-24-extension-pr2 --status success --limit 1 --json databaseId --jq '.[0].databaseId'); echo "$RUN"
extensions/vscode/scripts/install-vsix.sh "$RUN"
# Facultatif : client de l'extension contre le vrai binaire, sans modèle joignable.
(cd extensions/vscode && npm ci --ignore-scripts --no-audit --no-fund && npm run compile-tests && node scripts/real-binary-check.mjs ~/.cache/yusagent-bin/1b96bb6457e51d010ad32a2293714f911fea2162/yusagent)
```

Attendu :
- `install-vsix.sh` : taille < 5 000 000 octets, 8 fichiers (`[Content_Types].xml`, `extension.vsixmanifest`, `extension/package.json`, `extension/readme.md`, `extension/LICENSE.txt`, `extension/dist/extension.js`, `webview.js`, `webview.css`), puis `yusagent.yusagent@0.1.0` et `installé` ;
- `real-binary-check.mjs` : `CONFORME`, `exit 0`.

Dans VS Code, puis « Developer: Reload Window » :

1. **Réglage** : `yusagent.binaryPath` = chemin absolu de ton `yusagent` (réglages utilisateur ; le défaut `yusagent` le cherche dans le `PATH` de VS Code).
2. **Ouvrir un dossier jetable** (l'agent y travaille), puis « yusAgent: Nouveau chat » (palette de commandes). Attendu : onglet « yusAgent » à côté de l'éditeur ; en-tête avec le nom du dossier et « prêt » ; sous la saisie, les sélecteurs modèle et effort ; pas de bandeau en manuel.
3. **Prompt texte** (appel réel à ton modèle, fait par toi) : « Réponds seulement : bonjour ». Attendu : le message de l'utilisateur, puis la réponse qui s'écrit au fil de l'eau ; « Envoyer » devient « Arrêter » pendant le tour, puis revient. Sans identifiants : « Authentification requise : lancez `<chemin du binaire>` dans un terminal, puis /login. »
4. **Arrêter** : « Écris un texte de 1 000 mots sur les phares », puis « Arrêter » pendant l'écriture. Attendu : l'écriture s'arrête, « Arrêté. », le bouton redevient « Envoyer ». Avec un outil en mode manuel, la ligne « Demande d'autorisation en attente (boîte en PR 3) : Arrêter pour annuler » apparaît, et « Arrêter » la retire (appel « annulé »).
5. **Sélecteurs** : changer l'effort, puis le modèle. Attendu : la valeur reste affichée (réponse de l'agent) ; sélecteurs grisés pendant un tour ; rien n'est envoyé à l'ouverture.
6. **Bandeau en auto** : `"permissions": "auto"` dans `~/.yusagent/settings.json` (réglage global), puis un nouveau chat. Attendu : bandeau en haut, « ⚠ Mode auto sans sandbox : les cellules s'exécutent sans demander, avec vos droits. », impossible à fermer. Ne rien envoyer dans ce chat (les cellules tourneraient sans demander), puis remettre `"manual"`.
7. **Binaire absent** : `yusagent.binaryPath` = `/nulle-part/yusagent`, nouveau chat. Attendu : « Binaire yusAgent introuvable : /nulle-part/yusagent… » avec « Ouvrir le réglage yusagent.binaryPath » et « Réessayer ». Remettre le bon chemin.

Désinstaller : `code --uninstall-extension yusagent.yusagent`.

## Avant le merge de la PR #73 : comparaison avec l'extension d'Unharnessed (21/11/2026)

Source : `github.com/unharnessed-agent/Unharnessed`, commit `157dcdc1d9452cae6fdd0fc74c182617d4033ee2` (19/11/2026), dossier `vscode/` (dernier commit qui le touche : `61b017877`). Clone jetable dans le scratchpad de Claude Code, lecture seule. Licence MIT (« Copyright (c) 2026 Unharnessed », `LICENSE`). Unharnessed est aussi un fork de Prime Agent (crates `uh-*`), mais son mode ACP tourne **dans le processus de l'agent, sans démon** (`crates/uh-daemon/src/acp/in_process_config.rs`). Le nôtre passe par le démon (§1.2).

**Décision de Yusuf (21/11/2026)** : pour l'extension, on fait **comme Unharnessed sur tout le fonctionnel, emplacement compris**. Le visuel viendra plus tard (outil de design, journée de veille) : on ne reprend pas son `styles.css`. Option A ou B ci-dessous : à trancher.

### 1. Arrêt

- **Chez lui** (`vscode/src/agent.ts:134-154`) : fermeture de stdin, attente de 40 s au plus (`EXIT_WAIT_MS`, `:15` : « past the agent's own bounds (its turn 10 s, its kernel's save and shutdown about 25 s) »), puis SIGKILL. Rien n'est écrit avant l'événement `spawn` (`:118-123`). `initialize` borné à 30 s (`:9`).
- **Chez nous, lu** : à la fin de stdin, le pont annule le tour en cours (`cancel_owned_admission`), retire ses serveurs MCP et se détache (`crates/pa-daemon/src/acp/daemon.rs:1949-1990`, `teardown`). La session ACP est **`Resident`** (`crates/pa-cli/src/print_runtime.rs:153` ; `ClientOwned` seulement avec `--no-session`, `:128-133`) : le worker et son kernel vivent dans le démon, pas dans notre processus. Un worker sans client passe en veille après `idleEvictionMinutes`, **90 min par défaut** (`crates/pa-core/src/settings/manager.rs:17`, `crates/pa-daemon/src/worker/turn.rs:195-260`). Unharnessed endort aussi une conversation de fond après 90 min (`vscode/src/conversations.ts:11-12`).
- **Mesuré** (binaire `main` `1b96bb645`, serveur de capture en modèle factice, scénario `basic`, `sandbox-exec`, démon isolé ; script jetable `lifecycle.py` du scratchpad de Claude Code, à verser au banc si Yusuf le veut) :
  - A, fin de stdin après un tour : le pont sort en **0,0 s** ; démon, worker et **kernel toujours là à +0,5 s, +3 s et +10 s** ; arrêtés seulement par l'arrêt du démon ;
  - B, séquence actuelle de l'extension : `session/close` répond `{}` en 0,0 s, puis le pont sort dès SIGTERM ; **worker et kernel toujours là** à +0,5 s, +3 s et +10 s.
- **Conclusion** : notre SIGTERM puis SIGKILL à 2 s ne visent que le pont ; **ils n'arrêtent pas le kernel, et ce n'est pas un défaut** : session résidente, voulue pour le TUI attaché et la reprise. Non mesuré : la fin de stdin **pendant** un tour (durée du `teardown`, et effet d'un SIGTERM qui le couperait).

### 2. Plantage et reprise

- **Chez lui** : pas de relance automatique ; état `failed` avec les 20 dernières lignes de stderr et un bouton (`vscode/webview/Status.tsx`, `Failure`) ; la reprise relance le processus sur le fichier de la conversation (`vscode/src/conversations.ts:65-70`, `--resume <path>`, `vscode/src/agent.ts:93`) puis `session/load`, qui rejoue la conversation dans le chat (`vscode/src/session.ts:120-125`). Le chemin du fichier arrive dans `session/new` (`_meta.unharnessed.saved`, `:117-130`). Une ligne « restart » sépare l'avant de l'après (`vscode/src/chat.ts:186-193`).
- **Chez nous** : `--continue` et `--resume <sélecteur>` sont acceptés en mode ACP (`crates/pa-cli/src/args.rs:273-285`, `crates/pa-cli/src/print_runtime.rs:1230-1270`). **`session/load` est absent** (`-32601`, `crates/pa-daemon/src/acp/daemon.rs:1061-1068`). **`session/new` ne renvoie ni le chemin ni l'identifiant du fichier de session** (réponse relevée dans `cap-pr2/acp/transcript.jsonl` : `sessionId`, `configOptions` seulement). Sans lui, l'extension ne peut que `--continue`, qui prendrait la session la plus récente du dossier, peut-être celle d'un autre chat.
- **Mesuré, préfixe à la reprise** (même script) : C, `--continue` sur le même démon (worker encore vivant) ; D, `--resume <fichier>` après arrêt du démon (worker neuf) ; E, référence : deux prompts dans le même pont. Requêtes comparées corps entier, UUID et chemin du projet normalisés : **C = E et D = E sur les 5 requêtes**. Le préfixe se reconstruit à l'identique.

### 3. Client ACP : `@agentclientprotocol/sdk` 1.7.0

- Version épinglée chez lui : `vscode/package.json` (`devDependencies`). Publiée le 02/10/2026, Apache-2.0, dépendance pair `zod`.
- **Ce qu'il gère** (lu dans `dist/` du paquet 1.7.0) :
  - `$/cancel_request` reçu : interrompt le gestionnaire de la requête visée par un `AbortSignal`, réponse `-32800` ou valeur du gestionnaire (`dist/jsonrpc.js:852-862`) ;
  - `session/request_permission` (`dist/acp.js:1123`) ;
  - requêtes et notifications génériques dans les deux sens (`request`, `notify`, `onNotification`).
- **Nos trames passent sa validation zod** : `RequestPermissionRequest` (trame du pont, sans titre), `SessionConfigOption` (`permissions`, catégorie `mode`), `tool_call`, `session_info_update` (script de contrôle jetable `chk.mjs`).
- **Coût de la bascule** :
  - **taille** : un client minimal groupé et minifié par esbuild pèse **300 633 octets** (SDK 1.7.0 + zod 3.25.76), contre 17 811 pour tout l'hôte de notre extension ;
  - **code** : remplacer `src/acp/connection.ts` et `client.ts` (environ 330 lignes) par un adaptateur (estimation : 100 lignes) ; les tests unitaires du client deviennent ceux de l'adaptateur ;
  - **contrainte E2** (client écrit à la main), contredite.

### 4. Autres choix de son extension qui recoupent la PR 2 et diffèrent des nôtres

| Sujet | Unharnessed (`157dcdc1d`) | PR 2 |
|---|---|---|
| Emplacement | vue webview dans la **barre latérale secondaire** (`package.json`, `viewsContainers.secondarySidebar`), bouton dans la barre de titre de l'éditeur ; plusieurs conversations dans le même panneau, une par processus (`vscode/src/conversations.ts:73-80`), historique (`session/list`) | onglet d'éditeur par chat |
| Binaire | `UNHARNESSED_BIN`, sinon `bin/` du `.vsix` par plateforme, sinon `PATH` ; aucun réglage (`vscode/src/extension.ts:19-22`) | réglage `yusagent.binaryPath`, portée `machine` |
| Sans dossier | cwd = dossier personnel (`vscode/src/extension.ts:23`) | refus, « Ouvrez un dossier » (choix 12) |
| `initialize` | `clientInfo` avec la version de l'extension ; délai de 30 s | pas de `clientInfo` ; pas de délai |
| États | `starting` → `ready` → `failed` (avec la fin du stderr), retour à `starting` au redémarrage (`vscode/src/protocol.ts:8-11`) | phases `starting`, `ready`, `turn`, `stopping`, `stopped`, `closed` |
| Erreurs | `data.details` affiché tel quel (`errorText`, `vscode/src/agent.ts:249-257`) | message `/login` dédié, `details` dans le texte de l'échec |
| Journal | chaque trame dans le canal de sortie, clés et codes masqués pour 2 méthodes (`vscode/src/agent.ts:19-23`, `:199-219`) | stderr et erreurs de protocole seulement |
| Prompt | blocs `text`, `resource_link` (fichiers mentionnés), `image` ; `/skill:` réécrit (`vscode/src/chat.ts:591-601`) | blocs `text` seulement (§5.4) |
| Pendant un tour | message envoyé au tour en cours (`_unharnessed/session/steer`), file d'attente | « Envoyer » grisé |
| Message précédent | modifiable : retour en arrière (`_unharnessed/session/rewind`) puis renvoi | non |
| Texte | Markdown (`react-markdown` 10.1.0, `remark-gfm` 4.0.1) | texte brut (choix 3) |
| CSP | `style-src … 'unsafe-inline'`, `img-src … data:` (`vscode/src/view.ts:379-382`) | aucun style inline, aucun `data:` |
| Permissions | **aucune** : ni `session/request_permission`, ni bandeau | ligne d'attente (PR 2), boîte (PR 3), bandeau D4 |
| e2e | vrai binaire + provider factice (`UNHARNESSED_FAUX_SCRIPT`), un VS Code par scénario, contrôle des processus restants (`vscode/test/e2e/run.mjs:1-24`) | agent factice ; vrai binaire en PR 3 |
| Taille | hôte 4 415 lignes (`vscode/src/`), webview 8 774 lignes TS/TSX + `styles.css` 5 836, e2e 1 211 | hôte environ 900 lignes, webview environ 330 |

### 5. Fonctions de son extension et méthodes ACP

Méthodes utilisées par l'extension : `initialize`, `session/new`, `session/load`, `session/list`, `session/prompt`, `session/cancel`, `session/set_config_option`, et **31 méthodes `_unharnessed/*`** (`grep -rhoE '"_unharnessed/[a-z_/]+"' vscode/src vscode/webview`), implémentées dans `crates/uh-daemon/src/acp/` (10 324 lignes au total). Chez nous, le pont n'en sert aucune : toute méthode inconnue reçoit `-32601` (`crates/pa-daemon/src/acp/daemon.rs:1061-1068`). La plupart des commandes du démon existent déjà (héritage commun de Prime Agent, `crates/pa-types/src/daemon/command.rs`).

Tailles « chez nous » : **estimations** (pont + tests), non mesurées.

| Fonction | Méthodes | Commande du démon chez nous | Code Rust chez lui | Fichiers upstream touchés chez nous | Préfixe | Taille chez nous (estim.) |
|---|---|---|---|---|---|---|
| Reprise | `session/load`, `--resume` | `GetSessionMessages`, `--resume` déjà accepté | `mod.rs` (en partie), `history.rs` (en partie) | `acp/daemon.rs` (aiguillage), chemin du fichier dans `session/new` (`_meta` `_yus/*`) | **identique, mesuré** (C, D) ; `session/load` ne fait que rejouer vers le client | 300-400 lignes |
| Historique | `session/list`, `sessions/delete` | `ListSavedSessions`, `DeleteSavedSession` | `history.rs` (560) | aiguillage | aucun | 250 lignes |
| Message pendant le tour | `session/steer` | `Steer` (file `steering` du worker, `crates/pa-daemon/src/worker/turn.rs:220-226`) | `steer.rs` (106), `session.rs` | aiguillage | message ajouté à la fin avant l'appel suivant : prompt système et tools inchangés, historique en ajout seul (à vérifier par capture) | 150 lignes |
| Retour en arrière | `session/rewind` | `NavigateTree`, `Fork` | `rewind.rs` (67) + `rewind_to` du moteur | aiguillage, peut-être `session_engine` | la requête suivante part d'une branche : préfixe réutilisé jusqu'au point de retour, suite perdue pour le cache ; **le kernel garde son état** (`rewind.rs:7-8`), qui ne suit plus la conversation | 150 lignes + cas de capture |
| Kernel | `kernel/tail`, `kernel/kill`, `kernel/preview`, `kernel/changed`, `kernel/restored` (deux notifications) | `TailKernelBash`, `KillKernelBash`, `ListKernelBash` ; liste et aperçu des variables : **absents** (chez lui `unharnessed-runtime/src/rlm/variables.py`, 229 lignes) ; instantané du kernel : présent (`prime-agent-runtime/src/rlm/repl.py:772`, `:1123`) | `kernel.rs` (318) + `variables.py` | runtime (`repl.py`, upstream), gestionnaire du kernel | aucun si les MIME restent sans `text/plain` (règle de la PR 1), à prouver par capture | 600-800 lignes |
| Fournisseurs et login | `providers/list`, `reload`, `set_api_key`, `logout`, `login`, `login_answer`, `login_open`, `login_cancel`, `login_update` (notification), `local_scan`, `local_probe`, `local_save`, `local_remove` (13) | aucune (login du TUI : `crates/pa-tui/src/session_ui/commands.rs:286`, `crates/pa-cli/src/subscription_login.rs`, `codex_subscription_login.rs`) | `providers.rs` (625) + flux OAuth de `uh-ai` | couche providers `pa-ai` (règle 3), login Claude hérité (règle 7) | aucun direct ; le modèle disponible peut changer | 900-1 200 lignes |
| Réglages | `settings/get`, `settings/set` | aucune | `settings.rs` (383) | `settings` de `pa-core` (lecture) | selon la clé : compaction, masquage, effort par défaut… changent les requêtes ; invalidation à journaliser (`docs/yusAgent-cdc.md:19`) ; `permissions` à exclure ou encadrer (D1) | 400 lignes |
| Skills | `skills/list`, `set`, `create`, `add_folder`, `remove_folder` | `GetResourceSnapshot`, `Reload` (en partie) | `skills.rs` (368) | aiguillage | **la liste des skills est dans le prompt système** : un changement, appliqué au démarrage suivant, change le prompt système (invalidation complète, à journaliser et signaler) | 400 lignes |
| MCP | `mcp/list`, `mcp/set_token`, `mcp/logout` | `GetMcpConnections`, `SetMcpStaticToken`, `RemoveMcpConnection` | `mcp_servers.rs` (189) | aiguillage | les tools MCP changent la liste des tools : invalidation | 200 lignes |
| Option `fast_mode` | `session/set_config_option` | `SetServiceTier` | `config_options.rs` (en partie) | `acp/config_options.rs` | paramètre de requête, pas de texte du préfixe ; invalidation à journaliser | 80 lignes |
| Usage, commandes | `_meta` d'usage (`usage.rs`, 84), `available_commands_update` (`commands.rs`, 76) | `GetSessionStats` ; pas d'`available_commands_update` | 160 | `acp/wire_events.rs` | aucun | 250 lignes |

Total des méthodes portées, **estimation** : 3 700 à 4 600 lignes de Rust côté pont, sans compter le runtime des variables.

### 6. Ce qui chez lui contredit nos décisions (on garde les nôtres sauf avis contraire)

1. **Permissions** : aucune gestion. Contredit D2 (chaque appel confirmé en manuel), D4 (bandeau), D5 (testeurs en manuel), D7 (tour arrêté après un refus). On garde la boîte (PR 3) et le bandeau.
2. **Réglages globaux écrits par l'extension** (`settings/set`) : D1 (réglage `permissions` global seulement, changement par session via `set_config_option`). Le passage persistant en `auto` depuis l'écran des réglages serait à exclure.
3. **E2** : SDK au lieu d'un client écrit à la main (§3).
4. **E3** : CSP avec `'unsafe-inline'` pour les styles et `data:` pour les images.
5. **E4** : binaire dans le `.vsix` par plateforme, sans réglage (noté pour YA-16).
6. **E14** (`session/load` reporté) : au cœur de sa reprise. À reprendre si on suit son fonctionnel.
7. **E15** (sortie d'une cellule en cours reportée) : en partie couvert chez lui par `kernel/tail`.
8. **§4 et règles 3 et 7** : login dans l'extension (`providers/*`). Le login Claude par abonnement hérité ne doit pas être touché (Yusuf le retire), et la couche providers doit l'être le moins possible.
9. **§5.4 (préfixe)** : blocs `resource_link` et `image` dans le prompt (ajoutés au message, pas au préfixe) ; `clientInfo` avec version (trame d'ouverture variable d'une version à l'autre) ; `steer`, `rewind`, `skills`, `settings`, MCP : changements de requêtes à journaliser comme invalidations (`docs/yusAgent-cdc.md:19`) et à prouver par capture.
10. **Décision du 19/11/2026** (`_yus/*` pour nos ajouts, `docs/yusAgent-cdc.md:89`) : ses méthodes sont en `_unharnessed/*`.
11. **Choix 12** (refus sans dossier) : chez lui, dossier personnel.
12. **Règle 9** : il journalise chaque trame (prompts compris), avec masquage de deux méthodes seulement.

### Pour plus tard (notes de Yusuf)

- Markdown avec `react-markdown` + `remark-gfm` (versions d'Unharnessed : 10.1.0 et 4.0.1).
- `.vsix` par plateforme avec le binaire dedans (YA-16), comme `vscode/scripts/package.mjs` d'Unharnessed.
- Login dans l'extension avant les testeurs.
- Emplacement : barre latérale secondaire, comme Unharnessed (décision du 21/11/2026).

## Décisions de Yusuf sur la comparaison (21/11/2026)

- **Option (B)** : le code d'extension d'Unharnessed (`157dcdc1d`, `vscode/`) comme base, notice MIT conservée, **notre client ACP derrière une interface**. Écart à la règle du 15/11 (reprendre ses choix) accepté : 17 811 octets et client déjà testé, contre 300 633 octets pour le SDK.
- **PR #73** : mergée telle quelle, sans les points 1 à 4, qui arrivent avec l'import.
- **Point 1, arrêt** : mesure d'une fin de stdin pendant un tour avant l'import ; si le tour continue dans le démon, l'extension envoie `session/cancel` avant de fermer stdin. Délai de 10 s avant SIGKILL.
- **Points 2 et 3** (plantage sans relance automatique, fin du stderr et bouton ; `initialize` borné à 30 s, rien écrit avant le lancement du processus) : oui. **Point 4** (barre latérale secondaire) : avec l'import. **Point 5** : refus sans dossier ouvert, gardé.
- **Avant les testeurs** : reprise et historique (chemin du fichier dans `session/new`, `session/load`, `session/list`), message pendant un tour (capture à l'appui), login dans l'extension, Markdown, PR 3 (cartes, boîte, diffs), `.vsix` par plateforme. **Plus tard** : retour en arrière, panneau du kernel, réglages, skills, MCP.

### Point 1 : fin de stdin pendant un tour (mesure)

- **Script** : `eof_turn.py`, jetable, dans le scratchpad de Claude Code (à verser au banc si Yusuf le veut). Copie de `bench/tools/capture/server.py` avec un scénario `slow` : une cellule qui dort 10 s, puis écrit `FIN.txt` dans le projet. Binaire `main` `1b96bb645` sous `sandbox-exec`, démon isolé par cas, aucun modèle réel. Fin de stdin 2 s après le début de la cellule ; état relevé 15 s après.
- **Commande** : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 eof_turn.py ~/.cache/yusagent-bin/1b96bb6457e51d010ad32a2293714f911fea2162/yusagent <sortie> .` (et `ONLY_CONTROL=1` pour le contrôle positif).

| Cas | Sortie du pont | `FIN.txt` | Requêtes au modèle avant / 15 s après | Processus 15 s après |
|---|---|---|---|---|
| M1 manuel, demande d'accord en attente | 0,01 s | non | 1 / 1 | démon, worker, kernel |
| M2 manuel, accord donné, cellule en cours | 0,01 s | non | 1 / 1 | démon, worker, kernel |
| A1 auto, cellule en cours | 0,01 s | non | 1 / 1 | démon, worker, kernel |
| A2 auto, `session/cancel` puis fin de stdin | 0,01 s | non | 1 / 1 | démon, worker, kernel |
| **Contrôle** N, auto, sans fin de stdin | | **oui** | 1 / **2** | |
| **Contrôle** N2, manuel, accord, sans fin de stdin | | **oui** | 1 / **2** | |

- **Conclusion** : la fin de stdin arrête le tour, en manuel comme en auto : la cellule ne va pas au bout et le modèle ne reçoit rien de plus (`teardown`, `crates/pa-daemon/src/acp/daemon.rs:1949-1990`). **Le tour ne continue pas dans le démon : pas de `session/cancel` avant la fermeture de stdin.** Le worker et le kernel restent dans le démon, au repos, jusqu'à la veille à 90 min.
- **Arrêt de l'extension après l'import** : fin de stdin, attente de 10 s au plus, puis SIGKILL du pont ; ni `session/close` ni SIGTERM.

## Découpage (option B), validé par Yusuf le 21/11/2026

**Priorité (Yusuf, 21/11/2026)** : Yusuf est l'utilisateur principal, peut-être le seul ; l'ordre suit son utilité, pas les testeurs. Seuls le sandbox et le `.vsix` par plateforme restent liés à d'autres utilisateurs.

Chaque PR suit la boucle de dev : branche `ya-<ticket>`, captures `main` contre branche si le binaire change, CI verte sur la tête. Les PR Rust passent par des modules du fork. Elles modifient seulement l'aiguillage des méthodes du pont (`crates/pa-daemon/src/acp/daemon.rs:1052-1068`, fichier upstream, quelques lignes, listées dans `docs/fork-changes.md`), sauf mention contraire.

### Règle pour skills, MCP et réglages (Yusuf, 21/11/2026)

Un changement s'applique dès le message suivant, sans changer de conversation. Quand c'est possible, il est annoncé par un message en fin d'historique (préfixe intact) ; sinon, un seul miss, journalisé comme invalidation. `permissions` reste hors de ces écrans (D1).

Constat pour appliquer la règle :

- **Tools MCP : appelés depuis le REPL, pas dans la liste des tools.** Le prompt système le dit (« Generic MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL, not as top-level native tool namespaces », `crates/pa-core/src/prompts/system_prompt.rs:450`). Le skill `mcp` découvre les connexions en direct (`mcp.list_connections()`, `skills/mcp/SKILL.md`). La liste des tools ne change donc pas.
- **Mais le prompt système nomme les serveurs MCP activés** (segment « Generic MCP Connections », « Enabled generic MCP servers: … », `system_prompt.rs:263-271`, `:435-455`, en prompt v2 aussi) **et liste les skills** (inventaire compact en v2, `system_prompt.rs:238-259`, `crates/pa-core/src/prompts/ya_prompt_v2.rs:160`).
- **Donc, cas « annonce » pour MCP et skills** si le prompt système reste celui de l'ouverture de la session. Le changement est appliqué en direct (MCP : `SetMcpStaticToken`, `RemoveMcpConnection` ; skills : fichiers et chemins), puis annoncé par un message en fin d'historique, par la commande du démon `AppendCustomMessage`. Exemple de texte fixe : `[mcp-changed] enabled: slack` ; `[skills-changed] added: deploy (path)`.
  - Point à vérifier dans le constat de la PR : que rien ne reconstruit le prompt système en cours de session (commande `Reload`, rechargement des skills).
  - **Limite** : à la reprise d'une conversation, le prompt système est reconstruit avec l'état courant. Si skills ou MCP ont changé entre-temps, la première requête reprise diffère du préfixe d'avant : un miss, à journaliser comme invalidation à la reprise. La reprise mesurée identique le 21/11 (cas C, D) l'était sans changement.
- **Réglages** : à classer clé par clé dans le constat de la PR :
  - (a) lus à chaque usage, sans effet sur les requêtes : s'appliquent au message suivant, rien à annoncer ;
  - (b) qui changent les requêtes sans être figés : annonce si le modèle doit le savoir, sinon un miss journalisé ;
  - (c) **figés pour la session par conception**, pour le cache : `yaRequest` (`crates/pa-core/src/session_engine/ya_request.rs`, version du prompt, `existingTestsRule`, fenêtre d'`await bash`…) et tools natifs (`editTool`, `shellTool`, en-tête de session, `crates/pa-core/src/session_engine/ya_tool_family.rs:14-21`). La règle « dès le message suivant » demanderait de les rouvrir en cours de session (un miss chacun) ou de les laisser hors de l'écran. **À trancher par Yusuf** sur la liste des clés.

### Ordre des PR

1. **Import, Markdown, PR 3**
   - **PR #73** (en cours) : mergée telle quelle.
   - **PR I, import** (extension seule, aucun fichier sous `crates/`, préfixe inchangé) :
     - `vscode/` d'Unharnessed à `157dcdc1d` dans `extensions/vscode/`, à la place de notre interface ; notice MIT (`LICENSE` avec les deux copyrights, en-tête des fichiers repris, section « Code repris d'Unharnessed » de `docs/fork-changes.md` avec le commit) ;
     - **notre client ACP** derrière l'interface qu'utilise son `agent.ts` (`request`, `notify`, `onNotification`, plus le gestionnaire de `session/request_permission`) ; pas de SDK ;
     - points 2 à 4 : plantage sans relance automatique (fin du stderr, bouton « Relancer », nouvelle session tant que la reprise n'existe pas), `initialize` borné à 30 s, rien écrit avant le lancement du processus, barre latérale secondaire ; arrêt du point 1 (fin de stdin, 10 s, SIGKILL) ;
     - **gardé de la PR 2** : `yusagent.binaryPath` (portée `machine`), refus sans dossier ouvert, ligne d'attente d'une demande d'accord, bandeau D4 qui suit `permissions` annoncé par l'agent, message `/login` avec le chemin du binaire, CSP stricte (sans `'unsafe-inline'` ni `data:`), fixture des trames (preuve 1) et cas `acp-extension` (preuve 2), agent factice et e2e ;
     - **Markdown** avec l'import (`react-markdown` 10.1.0, `remark-gfm` 4.0.1) ;
     - **masqué** tant que le pont ne sert pas la méthode : historique, message pendant un tour, retour en arrière, panneau du kernel, fournisseurs et login, réglages, skills, MCP, usage ; mentions `@fichier` et images (blocs texte seulement, §5.4) ;
     - visuel : feuille de style minimale à nous, pas son `styles.css` ; ses éléments purement visuels (polices, logos, animations) ne sont pas repris.
   - **PR 3** : cartes de cellule (`_yus/bashRuns`), boîte de permission (« Autoriser » / « Refuser », fermée sur `$/cancel_request` ou au changement de statut), diffs `vscode.diff` et « Rejeter » (E6), e2e contre le vrai binaire (provider factice). `ya-extension` devient requis (E13).
2. **Reprise et historique**
   - **PR R1** (Rust) :
     - `_meta` `_yus/session` (`{id, path}`) dans la réponse de `session/new` ;
     - `session/load` pour la session ouverte par `--resume`, historique rejoué en `session/update` depuis `GetSessionMessages` ;
     - `session/list` (`ListSavedSessions`, filtré par `cwd`) ;
     - nouveau cas de capture `acp-resume` : préfixe identique après `--resume` (mesure du 21/11 : C = E, D = E).
   - **PR X1** (extension) : « Relancer » reprend par `--resume` et `session/load` ; plusieurs conversations et historique (comme Unharnessed).
3. **Skills, MCP, réglages**
   - **Constat d'abord** :
     - prompt système figé en cours de session ;
     - classement des clés de réglage en (a), (b), (c) ;
     - texte des annonces ;
     - journalisation des invalidations (à la reprise compris).
   - **PR R3** (Rust) : `_yus/skills/*`, `_yus/mcp/*`, `_yus/settings/*` sur les commandes existantes (`GetResourceSnapshot`, `GetMcpConnections`, `SetMcpStaticToken`, `RemoveMcpConnection`) et les réglages globaux ; annonce par `AppendCustomMessage` ; `permissions` refusé par `_yus/settings/set` (D1) ; cas de capture : préfixe intact après une annonce, et un seul miss journalisé dans le cas sans annonce.
   - **PR X3** (extension) : ses écrans skills, MCP et réglages.
4. **Message pendant un tour**
   - **PR R4** (Rust) : `_yus/session/steer` sur la commande `Steer` du démon. Cas de capture : prompt système et tools identiques, historique en ajout seul.
   - **PR X4** (extension) : envoi pendant le tour et file d'attente, comme Unharnessed.
5. **Login dans l'extension** : ticket à part, constat d'abord. Règle 3 (couche providers) et règle 7 (login Claude par abonnement hérité, que Yusuf retire). Méthodes de fournisseurs dans le pont, clés dans `SecretStorage` (CDC YA-10). Puis l'extension.
6. **Retour en arrière et panneau du kernel**
   - **D'abord, régler l'état du kernel** qui ne suit plus la conversation après un retour en arrière. Chez Unharnessed, « the kernel keeps what it holds » (`crates/uh-daemon/src/acp/rewind.rs:7-8`). Constat et ticket à part : instantané du kernel par tour (`prime-agent-runtime/src/rlm/repl.py:772`, `:1123`) ou annonce en fin d'historique.
   - Puis `_yus/session/rewind` (`NavigateTree`, `Fork`), le panneau du kernel (`TailKernelBash`, `KillKernelBash`, `ListKernelBash`, liste des variables à ajouter au runtime), et l'extension.

**Liés à d'autres utilisateurs, hors de cet ordre** : sandbox du kernel (seconde partie de YA-11) ; `.vsix` par plateforme (YA-16 : binaire de la plateforme dans le `.vsix`, comme `vscode/scripts/package.mjs` d'Unharnessed, construit en CI ; `yusagent.binaryPath` devient une surcharge).

## Écarts portés au journal du CDC (validés par Yusuf le 21/11/2026)

1. **YA-10, base de l'extension** : code d'extension d'Unharnessed (MIT, `157dcdc1d`) comme base, fonctionnel aligné sur le sien, visuel plus tard. Notre client ACP est gardé à la place de son SDK : écart à la règle du 15/11 accepté (17 811 contre 300 633 octets, client déjà testé).
2. **YA-10, emplacement** : vue dans la barre latérale secondaire, comme Unharnessed (le CDC ne fixait pas l'emplacement ; la PR 2 utilisait un onglet d'éditeur).
3. **YA-10, « redémarrage auto en cas de crash »** (`docs/yusAgent-cdc.md:372`) : remplacé par un bouton « Relancer » qui reprend la conversation (`--resume`, `session/load`), comme Unharnessed. Avant la PR R1, la relance ouvre une nouvelle session.
4. **YA-10, critère « .vsix < 5 Mo »** (`docs/yusAgent-cdc.md:389`) : ne tient plus avec le binaire dans le `.vsix` par plateforme (binaire seul environ 59 Mo, `docs/yusAgent-cdc.md:570`). **Décision : plafond de 5 Mo sur le `.vsix` hors binaire.**
5. **YA-10, login** : avant les testeurs, dans l'extension (CDC : clés dans `SecretStorage`, rappel OAuth par `registerUriHandler`). Écart au ticket 24, §4 (« l'extension ne s'occupe pas de l'authentification ») ; contraintes des règles 3 et 7, ticket à part.
6. **YA-10, ordre** : par utilité pour Yusuf, utilisateur principal, et non plus par les testeurs. (1) import, Markdown, PR 3 ; (2) reprise et historique ; (3) skills, MCP, réglages ; (4) message pendant un tour ; (5) login dans l'extension ; (6) retour en arrière (après l'état du kernel) et panneau du kernel. Sandbox et `.vsix` par plateforme restent liés à d'autres utilisateurs. E14 (`session/load` reporté) est rouvert.
7. **YA-10, arrêt d'un chat** : fermer un chat arrête le pont, pas le kernel. La session est résidente dans le démon et se met en veille après 90 min (`idleEvictionMinutes`). La fin de stdin arrête le tour en cours (mesure du point 1).
8. **YA-10, skills, MCP et réglages** : un changement s'applique dès le message suivant, sans changer de conversation ; annonce en fin d'historique quand c'est possible (préfixe intact), sinon un seul miss journalisé comme invalidation ; `permissions` hors de ces écrans (D1). Les réglages figés pour la session par conception (`yaRequest`, tools natifs) sont à trancher clé par clé.

## Décisions de Yusuf sur le découpage (21/11/2026)

- Découpage et écarts 1 à 8 **validés**, portés au journal du CDC (deux lignes du 21/11/2026).
- **Écart 4** : plafond de 5 Mo sur le `.vsix` hors binaire.
- **Clés figées pour la session** (`yaRequest`, `editTool`, `shellTool`) : hors de l'écran des réglages par défaut. Yusuf tranche sur la liste des clés (constat de l'étape 3).
- **Écart 7, point ouvert** : supprimer une conversation devrait arrêter sa session dans le démon ; sinon son worker et son kernel restent jusqu'à la veille, 90 min.
- **Check `ya-extension` requis** : un check requis filtré par chemins resterait en attente sur les autres PR. Le workflow devient requis seulement s'il tourne sur chaque PR, avec un job qui sort vite en succès quand `extensions/vscode/` n'a pas changé ; sinon il reste non requis.

### Merge de la PR #73

- **Contrôle de Yusuf** : fait ; **résultat non transmis** (le message disait « Contrôle de la PR #73 fait : <résultat>. Merge. »). À compléter.
- **Merge** : PR #73 mergée par Claude Code sur la demande de Yusuf, `main` à `8830b763d` ; tête `a8941b37f` avec `ci` 44900460119 (14 checks requis verts) et `ya-extension` 44900460206 vert.

## Points ouverts (comparaison et découpage)

- Supprimer une conversation devrait arrêter sa session dans le démon (écart 7).
- Fin de stdin pendant un tour : mesurée sans relance du worker ; une relance au milieu d'un tour n'a pas de répondant jusqu'au prompt suivant (PR 1).
- Reprise après un changement de skills ou de MCP : un miss, à journaliser comme invalidation (étape 3).
- Script de mesure `eof_turn.py` et `lifecycle.py` : jetables, à verser au banc si Yusuf le veut.

## PR I : import de l'extension d'Unharnessed (21/11/2026)

Branche `ya-24-extension-import`, depuis `main` à `8830b763d`. Extension seule : **aucun fichier sous `crates/` ni upstream, préfixe inchangé** (mêmes trames d'ouverture et de prompt, fixture inchangé). Détail des fichiers : `docs/fork-changes.md`, section « Extension VS Code, PR I ».

### Ce qui est fait

- **Import tel quel** (premier commit) : `vscode/src` et `vscode/webview` d'Unharnessed à `157dcdc1d`, sans `styles.css`, médias, polices, `seti.json`, scripts de paquet ni e2e ; notice MIT (`extensions/vscode/LICENSE`, avec les copyrights d'Unharnessed).
- **Adaptations** (commits suivants), marquées « yusAgent » dans les fichiers repris, le reste dans des modules à part (`src/ya/`, `webview/ya-*`) :
  - **notre client ACP** derrière l'interface de son `agent.ts` (`request`, `notify`, `onNotification`) ; `initialize` sans `clientInfo`, borné à 30 s ; rien écrit avant le lancement du processus ;
  - **méthodes non servies par le pont** (`_unharnessed/*`, `session/load`, `session/list`) : rejetées sur place, sans trame ; les entrées correspondantes du panneau sont masquées (fournisseurs, réglages, historique, mentions, images, « Send now » pendant un tour ; le retour en arrière reste inaccessible, notre pont n'envoyant pas `rewindTo`) ;
  - **`_meta` de notre pont** (`ai.primeintellect.prime-agent`) lu dans la forme d'Unharnessed : début de tour déduit du premier événement de contenu d'un `promptTurnId` inconnu ; fin d'un tour lancé par l'agent sur `responseBoundary`. **Défaut trouvé par le contrôle sur le vrai binaire** : le `config_option_update` d'un changement d'effort porte un `promptTurnId` ; la première version y voyait un tour de l'agent, le chat restait occupé et le prompt attendait en file. Corrigé (seuls les événements de contenu ouvrent un tour), avec un test ;
  - **gardé de la PR 2** : `yusagent.binaryPath` (portée `machine`), refus sans dossier ouvert, ligne d'attente d'une demande d'accord (« Arrêter » répond `cancelled` après `session/cancel` ; `$/cancel_request` ; sortie de `pending`), bandeau D4 sur `permissions` annoncé par l'agent, message `/login` avec le chemin du binaire (deux formes : `kind: "authentication"` et texte d'identifiant absent), CSP stricte (sans `'unsafe-inline'` ni `data:`) ;
  - **points 2 à 4** : plantage sans relance automatique (son état « Agent stopped », fin du stderr, « Restart ») ; arrêt du point 1 (fin de stdin, 10 s au plus, SIGKILL ; plus de `session/close` ni de SIGTERM) ; vue dans la barre latérale secondaire (`yusagent.chat`), bouton dans la barre de titre de l'éditeur ;
  - **Markdown** avec l'import (`react-markdown` 10.1.0, `remark-gfm` 4.0.1) ;
  - **visuel** : bouchons pour ses fichiers visuels (logos, icônes de fichiers, logo, écran d'accueil, animations, `thinking-orbs`), feuille minimale à nous (structure et couleurs du thème VS Code).
- **Workflow `ya-extension`** : tourne sur chaque PR ; l'étape « changements » sort vite en succès quand ni `extensions/vscode/` ni le workflow n'ont changé. Il reste non requis jusqu'à la PR 3 (E13) ; la protection de `main` est à changer par Yusuf à ce moment-là.

### Tests et contrôles (Claude Code, en local)

- `cd extensions/vscode && npm run typecheck && npm run lint && npm run test:unit` : types et lint sans erreur ; **25 tests, 25 réussis** : client ACP (8), binaire (5), code repris contre l'agent factice (12) :
  - trames du fixture à l'ouverture et pour un prompt, avec fournisseurs, réglages et historique branchés comme dans `activate` (aucune trame en plus) ;
  - méthodes non servies sans trame ; Arrêter et ligne d'attente ; `$/cancel_request` ; `/login` (deux formes) ;
  - plantage sans relance automatique, puis « Restart » ; arrêt par fin de stdin ;
  - binaire absent, aucun dossier, version ACP inattendue ; bandeau D4 ; `set_config_option` sur un changement seulement ; lecture du `_meta`.
- **Contrôle négatif du fixture** : `clientInfo` ajouté à `initialize` → le test des trames échoue (exit 1), aucun processus restant.
- **Contrôle sur le vrai binaire** (règle 13) : `npm run compile-tests && node scripts/real-binary-check.mjs ~/.cache/yusagent-bin/1b96bb6457e51d010ad32a2293714f911fea2162/yusagent` (code de l'extension hors VS Code, binaire `main` `1b96bb645` sous `sandbox-exec`, HOME vide, aucun modèle joignable). Pour « manual » et « auto » :
  - session ouverte ; `permissions` annoncé ; bandeau absent puis affiché ; effort `medium → low` ;
  - tour en échec avec « Authentification requise : lancez <chemin> dans un terminal, puis /login. » ;
  - arrêt par fin de stdin en 4 ms et 1 ms ; aucun processus restant.

  **CONFORME**, exit 0. Premier lancement : NON CONFORME, « délai dépassé : fin du tour » (le défaut du `_meta` ci-dessus).
- **e2e** (`test/e2e/suite.ts`, CI seulement) : 6 tests dans VS Code contre l'agent factice : vue ouverte, CSP, trames du fixture et rien d'autre à l'ouverture, texte streamé affiché ; Stop et ligne d'attente ; modèle affiché et `set_config_option` ; `/login` ; plantage et « Restart » ; bandeau D4 (nouvelle conversation en auto, puis `config_option_update`).

### Points ouverts de la PR I

- Rendu provisoire : la feuille minimale couvre la structure ; menus, info-bulles et pastilles d'Unharnessed restent sans style jusqu'au visuel.
- Fin des tours lancés par l'agent (avis `[bash-done]`) déduite de `responseBoundary` : non exercée contre le vrai binaire (e2e du vrai binaire en PR 3).
- Commandes `/…` du panneau : seule `/new` (du panneau) ; notre pont n'envoie pas `available_commands_update`.

