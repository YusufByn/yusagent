# Ticket 24 : YA-10 minimal, extension VS Code (constat et plan)

Demande de Yusuf du 21/11/2026. Branche `ya-24-extension`, depuis `main` à `746a95559` (merge de la PR #71, ticket 23 clos). **Constat et plan seulement** : aucun code écrit, rien compilé, aucun appel de modèle. Pas de code avant les décisions de Yusuf (section 6).

Référence : YA-10 du CDC (`docs/yusAgent-cdc.md:365-391`).

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
- Ticket 22, `:79` : phrase périmée, à corriger avec le premier push de code.
