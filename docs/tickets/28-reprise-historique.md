# Ticket 28 : reprise et historique dans l'extension (R1 côté Rust, X1 côté extension)

Constat et plan du 22/11/2026, sur `main` à `d2f54743e` (merge de la PR #77). **Constat et plan seulement, aucun code.** Branche `ya-28-reprise`.

Références : `docs/tickets/24-ya10-extension-minimal.md` (découpage, étape 2 « Reprise et historique » : PR R1, PR X1), journal du CDC du 21/11/2026 (`docs/yusAgent-cdc.md:94-95`). Décision du 21/11 : **le fonctionnel de l'extension suit Unharnessed**, notre client ACP est gardé, Yusuf est l'utilisateur principal.

Unharnessed lu à `157dcdc1d9452cae6fdd0fc74c182617d4033ee2` (19/11/2026). C'est aussi la tête de `origin/HEAD` le 22/11 (`git log -1 --format='%H %cs' origin/HEAD`), donc les références valent pour les deux. Lecture faite dans un clone jetable du scratchpad de Claude Code, sans rien construire ni lancer. Chemins relatifs à la racine de son dépôt, préfixés « UH ».

Binaire de référence : `~/.cache/yusagent-bin/d2f54743e1b4ac98f40a6a72f3e098c94bf5fe39/yusagent`. Aucune mesure n'a été faite dans ce constat. Les mesures citées viennent du ticket 24 ; les autres points sont lus dans le code et marqués « à vérifier » quand la lecture ne suffit pas.

## 1. Ce que l'extension attend, et ce que fait Unharnessed

### 1.1 Extension importée (`extensions/vscode/`, code d'Unharnessed)

Le code de reprise, de conversations multiples et d'historique **est déjà dans l'extension**. Il est inerte parce que le pont ne sert pas les méthodes :

- `src/ya/methods.ts:9-18` : seules `initialize`, `session/new`, `session/prompt`, `session/set_config_option`, `session/close` et `session/cancel` partent ; le reste est rejeté sur place (`-32601`, sans trame).
- `src/ya/methods.ts:23` : l'historique n'apparaît que si `session/list` est servie.

Ce que le code attend, méthode par méthode :

| Fonction | Ce que l'extension envoie ou lit | Fichier:ligne |
|---|---|---|
| Fichier de la session | `result._meta.unharnessed.saved = { sessionId, path }` dans la réponse de `session/new` (et de `session/load`) ; gardé quand l'agent s'arrête | `src/session.ts:118`, `:129-130`, `:73-79` |
| Fichier présent ? | une conversation n'a de fichier que s'il existe sur disque ; sans fichier et sans travail, elle est « vide » et fermée quand on la quitte | `src/conversations.ts:49-53`, `:60-63`, `:220` |
| Reprise | relance du processus : `--mode acp --resume <path>` | `src/agent.ts:127`, `:168-174` |
| Chargement | `session/load { cwd, mcpServers: [], sessionId: saved.id }` ; `this.id` fixé **avant** la requête pour accepter les updates rejouées | `src/session.ts:110-125` |
| Rejeu | mode `loading` : chaque tour ouvert par `turnStart`, fermé par `turnEnd { state, error? }` ; prompt en `user_message_chunk` ; texte, raisonnement, cellules comme en direct ; journaux tus ; « Rejeter » non enregistré pour un diff rejoué | `src/chat.ts:60-74` (`Meta`), `:499-528`, `:708-711`, `:755-764`, `:943`, `:1004-1007` |
| Kernel restauré | notification `_unharnessed/kernel/restored { sessionId, restored }`, ligne posée après la conversation | `src/chat.ts:36`, `:531-541` |
| « Relancer » | redémarre sur le fichier ; sans fichier, nouvelle session | `src/conversations.ts:65-70`, `:162-166` |
| Historique | `session/list { cwd }` ; lit `sessionId`, `title`, `updatedAt`, `_meta.unharnessed.path` ; relu à chaque ouverture du menu | `src/history.ts:11-17`, `:62-84` |
| Ouvrir | conversation déjà ouverte : affichée telle quelle ; sinon nouvel agent sur son fichier | `src/conversations.ts:155-160`, `:183-191` |
| Supprimer | `release(id)` arrête l'agent de cette conversation (si ouverte et non affichée), puis `_unharnessed/sessions/delete { sessionId }` part par l'agent **affiché** | `src/view.ts:325-329`, `src/conversations.ts:168-173`, `src/history.ts:8-9`, `:86-96` |
| Plusieurs conversations | un processus par conversation ; une conversation quittée continue de travailler ; veille après 90 min sans travail (arrêt de l'agent) | `src/conversations.ts:11-12`, `:15-42`, `:224-271` |

Deux identifiants coexistent, comme chez Unharnessed : l'`sessionId` ACP sert aux requêtes, `saved.sessionId` (identifiant du fichier) sert à l'historique et à `session/load` (`src/session.ts:128-130`, `src/conversations.ts:44-47`).

### 1.2 Unharnessed côté agent (`157dcdc1d`)

Son mode ACP tourne dans le processus de l'agent, sans démon (ticket 24, §1.2). Une seule session par processus (UH `crates/uh-daemon/src/acp/mod.rs:567-576`).

- **`session/new`** : renvoie `{ sessionId, configOptions, _meta.unharnessed: { cwd?, saved: { sessionId, path? } } }` (UH `mod.rs:795-812`, `meta.rs:106-117`). L'`sessionId` ACP d'une session neuve est un UUID v4 tiré au hasard (UH `mod.rs:766`) ; `saved.sessionId` est l'identifiant du fichier. Chemin : `<session_dir>/<id>.jsonl` (UH `crates/uh-core/src/session/manager/ids.rs:26-27`). Le fichier n'est écrit qu'à la première réponse de l'assistant (UH `crates/uh-core/src/session/manager/persist.rs:125-134`).
- **`session/load`** : paramètres de `session/new` plus `sessionId` (UH `types.rs:342-359`). Le processus doit avoir été lancé avec `--resume` : un autre `sessionId` est refusé, « This process holds session {held}: start unharnessed with --resume <its file> to load {requested}. » (UH `mod.rs:638-656`). L'id ACP vaut alors l'identifiant du fichier (UH `mod.rs:656`). Ordre : admission, réponse de `session/new` validée dans le producteur, relecture du fichier, rejeu tour par tour entre `begin_prompt` et `finish_prompt`, notification `kernel/restored`, puis réponse (UH `mod.rs:667-689`).
- **Forme du rejeu** (UH `crates/uh-daemon/src/acp/history.rs`) :
  - chaque tour : `turnStart { origin: prompt | wake, wake? }`, `user_message_chunk`, contenu, puis `turnEnd` (`:11-15`, `:369-381`, `:442-456`) ;
  - un tour resté ouvert finit en `cancelled` (`:445`) ;
  - le contenu passe par **le même mappeur que le direct**, `acp_updates_for_event` (`:427-433`) ;
  - pas de marqueurs `thinking`, donc pas de durée de raisonnement (`:14`) ;
  - réveils `bash` reconnus aux lignes `async_bash_completion` (`:341-344`) ;
  - non rejoués : la ligne `ipython_state_restored` et les autres lignes custom (`:352`) ;
  - aucun drapeau « replay » dans les trames : le client sait qu'il charge.
- **`session/list`** : `cwd?` seulement, pas de pagination (UH `mod.rs:708`). Lignes `{ sessionId, cwd, title?, updatedAt, _meta.unharnessed: { path, current? } }` (UH `history.rs:68-84`). Titre : début du premier prompt, coupé à 80 caractères (`:111-150`). Pas de fichier sans prompt. Tri par date de modification décroissante (`:88`).
- **`_unharnessed/sessions/delete { sessionId }`** (UH `history.rs:466-536`) :
  - refusé pour la session du processus, « This is the conversation in progress: start a new one to delete it. » (`:499-506`) ;
  - refusé pour un fichier tenu par un autre processus (bail), « This conversation is open in another window. » (`:541-551`) ;
  - supprime le `.jsonl` et `session-artifacts/<id>/`, qui contient l'instantané du kernel (`:513-519`) ;
  - n'arrête aucun processus : c'est l'extension qui arrête l'agent avant (`release`).
- **Capacités annoncées** : `loadSession: true`, `sessionCapabilities { close, list }` (UH `types.rs:429-437`, `:462-465`).
- **Rechargement de la fenêtre** : aucune restauration des conversations ; `activate` ouvre une conversation neuve (UH `vscode/src/extension.ts:51`) ; on rouvre par l'historique.

## 2. Notre pont ACP aujourd'hui

### 2.1 Ce qui existe

- **Méthodes servies** : `initialize`, `session/new` et `session/set_config_option` dans l'aiguillage (`crates/pa-daemon/src/acp/daemon.rs:1097-1114`) ; `session/prompt`, `session/close` et `session/cancel` dans l'ordre des trames (`:727`, `:740`, `:771`). Toute autre méthode : `-32601` (`:1107-1113`).
- **Capacités** : `loadSession: false`, `sessionCapabilities { close }` (`crates/pa-daemon/src/acp/types.rs:404-413`).
- **Une session du démon par processus du pont**, créée et attachée au démarrage, avant tout `session/new` (`daemon.rs:627-690`, `bind_daemon_session`). Le `create` vient des options de la ligne de commande : `--resume <chemin ou id>` et `--continue` ouvrent un fichier existant, sinon session neuve (`crates/pa-cli/src/print_runtime.rs:118-170`, `:1239-1275`). Une session d'un autre projet est refusée avec un renvoi à `--fork` (`print_runtime.rs:1259-1270`).
- **Réutilisation d'un worker vivant** : un `create` sur un fichier qu'un worker du démon sert déjà **se rattache à ce worker** au lieu d'en lancer un second (`crates/pa-daemon/src/supervisor/sessions.rs:609-616`). C'est le cas C du ticket 24 (§2, mesure du 21/11).
- **`session/new`** : l'`sessionId` ACP est un UUID v4 tiré au hasard (`daemon.rs:1215`). La réponse ne porte que `sessionId`, `configOptions` et, si le `cwd` demandé diffère, `_meta…cwd` (`daemon.rs:1287-1302`). `bind_daemon_session` ne garde que `activeSessionId` du résumé du `create` (`daemon.rs:644-653`). Pourtant le résumé contient `sessionFile` (`sessions.rs:677-680`).
- **Commandes du démon déjà disponibles** :
  - `ListSavedSessions { cwd, limit, cursor }` (`crates/pa-types/src/daemon/command.rs:139-157`). Ses lignes portent `sessionId`, `sessionFile`, `sessionName`, `cwd`, `messageCount`, `modified`, `firstMessage`, `lifecycle` (`draft`, `live`, `archived`), `rlmDepth` et `parentSessionPath` (`sessions.rs:774-805`).
  - `GetSessionMessages { start, limit, roles }` : tranche du transcript, `{ messages: [{ index, id, message }], total }` (`command.rs:682-695`, `crates/pa-daemon/src/session_store.rs:126-142`).
  - `Kill`, l'arrêt d'une session vivante (`command.rs:325-331`, `crates/pa-daemon/src/worker/commands.rs:720-770`).
  - `DeleteSavedSession { session_path }` (`command.rs:1164-1172`, `crates/pa-daemon/src/saved_session_commands.rs:552-700`).
- **Attach** : la réponse contient déjà le transcript (`snapshot.messages`, et une copie au premier niveau hors `slim_attach`, `crates/pa-daemon/src/worker/connection.rs:883-916`). Le pont l'ignore aujourd'hui.

### 2.2 Ce qui manque

1. **Le fichier de session dans la réponse de `session/new`** : identifiant et chemin. Il faut les lire dans le résumé du `create` (`sessionId`, `sessionFile`), au lieu de les jeter (`daemon.rs:644-653`).
2. **`session/load`** :
   - même admission que `session/new` (MCP, options, producteur) ;
   - refus si `sessionId` n'est pas l'identifiant du fichier ouvert au démarrage, comme Unharnessed ;
   - rejeu du transcript en `session/update`, sous l'`sessionId` demandé ;
   - capacité `loadSession: true`.
3. **`session/list`** : `ListSavedSessions` filtré par `cwd`, mis à la forme ACP (titre, `updatedAt`, chemin) ; capacité `sessionCapabilities.list`.
4. **Suppression** : méthode à nommer (choix C3), qui arrête la session dans le démon puis supprime le fichier (§5).
5. **Kernel restauré** : aucun moyen aujourd'hui de dire au client qu'un kernel neuf a relu son instantané (§3, choix C7).
6. **Miss de cache à la reprise** : rien ne le détecte (§4).

Le fichier existe-t-il avant le premier prompt dans notre démon ? Le gestionnaire hérité suit `flushed` et `has_assistant_entry` (`crates/pa-core/src/session/manager.rs:72-73`, `session/manager/append.rs:11-27`), comme Unharnessed, mais le worker écrit par `session_store`. **À vérifier dans R1.** L'extension tolère les deux cas (`src/conversations.ts:49-53`).

## 3. Le kernel à la reprise

Chez nous, deux cas, selon que le worker de la session vit encore dans le démon :

| Cas | Quand | Kernel | Ce qui est perdu | Avis au modèle |
|---|---|---|---|---|
| **R-vivant** (cas C du ticket 24) | « Relancer » après plantage du pont ; réouverture moins de 90 min après la fermeture du chat ; tout `--resume` tant que le worker n'est pas en veille | **le même** : le `create` se rattache au worker (`sessions.rs:609-616`) | rien : variables, `os.chdir`, environnement, jobs `bash()` en cours | aucun (rien n'a changé) |
| **R-neuf** (cas D) | worker en veille (`idleEvictionMinutes`, 90 min, `crates/pa-core/src/settings/manager.rs:801-802`), démon arrêté, machine redémarrée | **neuf**, préchauffé si un instantané existe (`crates/pa-core/src/session_engine/engine.rs:529-533`, `:733`) | objets non sérialisables, `os.chdir`, `os.environ`, jobs `bash()` (tués avec l'ancien kernel), fichiers et sockets ouverts | ligne custom `ipython_state_restored`, `[python-state-restored]`, livrée **en tête du tour suivant, avant le prompt** (`crates/pa-core/src/session_engine/state_restore_notice.rs:1-4`, `:21-50`, `engine.rs:539-551`) ; seulement si un instantané existait |

- L'instantané est pris environ 1,5 s après chaque cellule réussie (`crates/pa-core/src/kernel/shared.rs:374`, `kernel/manager/snapshot.rs:345`).
- **Unharnessed** : toujours le cas R-neuf. Un kernel neuf à chaque reprise ; instantané final à la fermeture de stdin (UH `mod.rs:288-296`) ; même avis `[python-state-restored]`, livré au tour suivant ; notification `_unharnessed/kernel/restored` au client (UH `crates/uh-daemon/src/acp/kernel.rs:134-158`, `mod.rs:688`).
- **Écart** : notre démon garde tout dans le cas R-vivant, ce qu'Unharnessed ne peut pas faire. Dans le cas R-neuf, même comportement, sauf la notification au client. Notre restauration a lieu au démarrage du worker et sa ligne attend le tour suivant dans une boîte du moteur (`engine.rs:534-551`). Le pont n'a pas de commande pour la lire, alors que le fournisseur du kernel garde `last_restore` (`crates/pa-core/src/kernel/provisioner.rs:1331`). Choix C7.
- Notre dernier instantané : pris après la dernière cellule réussie. La fermeture du pont n'arrête pas le kernel (le worker reste), donc rien ne manque au passage en veille. **À vérifier** que la veille du worker écrit un instantané final (passivation, ticket 23).

## 4. Effet sur le cache

### 4.1 Le préfixe après une reprise

- **R-vivant** : même moteur, même prompt système, mêmes tools. Rien ne change. Mesuré le 21/11 (C = E, corps entier, 5 requêtes, ticket 24, §2).
- **R-neuf** : le prompt système est **reconstruit** à la création du moteur avec les ressources du moment (`engine.rs:776-820`) : fichiers AGENTS (`context_files`), skills, serveurs MCP activés (`generic_mcp_servers`), ajouts du prompt (`append_system_prompt`), extras du modèle. Ce qui est figé dans l'en-tête de la session et relu à chaque reprise ne bouge pas : famille de tools (`crates/pa-core/src/session_engine/ya_tool_family.rs:14-21`), réglages `yaRequest` (`session_engine/ya_request.rs:1-2`). Le `cwd` est celui de la session (`print_runtime.rs:147-149`). **Pas de date** dans notre prompt système (test `crates/pa-core/src/prompts/system_prompt.rs:607`). Sans changement entre-temps, le préfixe est identique (mesuré le 21/11 : D = E).
- **Une seule chose s'ajoute en R-neuf** : la ligne `[python-state-restored]`, en fin d'historique, avant le prompt suivant. Le préfixe reste intact ; le message s'ajoute à la fin. La mesure D = E du 21/11 a été faite avec le scénario `basic`, et le constat ne dit pas si un instantané existait. **À établir par le cas `acp-resume`** (§7) : où la ligne apparaît, et que rien d'autre ne bouge.
- **Changement de skills, de MCP ou d'AGENTS entre-temps, en R-neuf** : le prompt système diffère, donc un miss complet sur la première requête reprise (règle du 21/11 : un seul miss, journalisé). En R-vivant, l'ancien prompt système reste, sans miss. L'annonce du changement relève de l'étape 3 (skills, MCP, réglages).
- **Unharnessed** : prompt système reconstruit à chaque démarrage, **avec `Current date: <UTC>`** (UH `crates/uh-core/src/prompts/system_prompt.rs:276`) : une reprise un autre jour UTC casse le bloc système. Il ne détecte aucun changement de préfixe et ne journalise aucun miss. Il n'a qu'une télémétrie de réutilisation calculée après coup (UH `crates/uh-core/src/session/manager/queries.rs:75-114`).

### 4.2 Où le miss est journalisé

- Journal d'usage de la session, `<id>.usage.ndjson` à côté du fichier de session (`crates/pa-core/src/session_engine/usage_log.rs:576-581`), champ `invalidation` de l'appel concerné (`usage_log.rs:540-541`).
- Ce qui est déjà vu à la reprise : un changement de modèle ou d'effort, comparé à la dernière ligne du journal d'avant (`usage_log.rs:316-345`, `:407-435`, causes `model_change` et `thinking_level_change`).
- **Ce qui ne l'est pas** : un prompt système ou des tools différents. Le journal ne garde pas d'empreinte du préfixe (`usage_log.rs:479-545`).
- Proposition (choix C8) : chaque appel principal écrit dans le journal une empreinte du prompt système et une des définitions de tools. À la première requête après une reprise, l'empreinte est comparée à la dernière du journal. Si elle diffère, une seule entrée `invalidation { cause: "resume_prefix_change", parts: ["system"] | ["tools"] | ["system","tools"] }` est écrite. Les appels suivants comparent à la nouvelle empreinte, donc un seul miss journalisé. Le format du journal change (`docs/usage.md` à mettre à jour), mais **rien de ce qui part au modèle**.

## 5. Supprimer une conversation

- **Aujourd'hui**, chez nous : fermer un chat arrête le pont, pas le worker, qui reste avec son kernel jusqu'à la veille (90 min) (ticket 24, écart 7). Le point ouvert du 21/11 demande que la suppression arrête la session dans le démon.
- **Ce que fait le démon** :
  - `DeleteSavedSession` **refuse une session vivante**, « Cannot delete the currently active session » (`saved_session_commands.rs:581-597`), et passe par le worker qui tient le fichier quand il y en a un (`:600-639`) ;
  - sinon, il supprime le fichier (par `trash` s'il existe, puis `unlink`) et `session-artifacts/<id>` (`saved_session_commands.rs:106-150`).
- **Le TUI** fait l'arrêt en deux temps : une session vivante s'arrête par `Kill`, une session sauvegardée se supprime par `DeleteSavedSession` (`crates/pa-tui/src/agents_view/delete.rs:37-47`, `:405-423`).
- **`Kill`** (raison `killed`) annule les tâches planifiées et les heartbeats de la session, ferme ses sous-agents, archive le fichier (`session_state: archived`) et arrête le tour (`worker/commands.rs:720-770`).
- **Proposition pour la suppression par le pont** (choix C4) :
  1. refuser la session du pont lui-même (comme Unharnessed) ;
  2. chercher la session vivante qui tient le fichier (roster du démon) ;
  3. si un autre client y est attaché (TUI, autre fenêtre), refuser, « ouverte ailleurs » (équivalent du bail d'Unharnessed) ;
  4. sinon, `Kill`, puis `DeleteSavedSession` ;
  5. répondre `{}`.
  L'extension ferme déjà le pont de cette conversation avant la requête (`src/conversations.ts:168-173`). Le pont n'a besoin d'aucune autre commande que celles du démon.
- **Restes** : le journal `<id>.usage.ndjson` n'est pas retiré par `delete_session_file` (`saved_session_commands.rs:106-150`). **À vérifier** dans R1 ; le retirer aussi est proposé (choix C4).
- **Écart avec Unharnessed** : chez lui, arrêter le processus suffit ; chez nous, la suppression arrête aussi le worker du démon.

## 6. Tours lancés par l'agent dans une conversation reprise

- **Chez nous, le worker vit sans client** (cas R-vivant) : pendant que le chat est fermé, il peut encore lancer des tours. Un avis `[bash-done]` d'un job resté en cours, un heartbeat ou une relance après limite déclenchent un tour (ticket 25, E1). Ces tours sont dans le fichier : le rejeu de `session/load` doit les montrer comme tours `wake`.
- **Genre du réveil au rejeu** : déduit du premier message du tour, comme en direct (ticket 25, E2, `crates/pa-daemon/src/acp/ya_turns.rs:1-16`).
- **Tour en cours au moment du `session/load`** : `ya_turns` n'ouvre un tour que sur un `agent_start` vu par le pont (`ya_turns.rs:88-135`). Un pont qui s'attache au milieu d'un réveil ne voit pas ce début. Proposition (choix C9) : après le rejeu, si le worker est occupé, publier `turnStart { origin: "wake" }` pour ce tour, puis le laisser finir comme un réveil. Cas de test dans R1.
- **R-neuf** : les jobs `bash()` de l'ancien kernel sont morts, donc aucun `[bash-done]` ne viendra. Le modèle n'en est pas averti, comme chez Unharnessed (son rapport : jobs tués avec le kernel, UH `crates/uh-core/src/kernel/manager/teardown.rs:69-70`). Les heartbeats et les tâches planifiées sont persistés par le démon ; ils survivent à une veille (`Kill` en raison `shutdown` les garde, `worker/commands.rs:732-740`).
- **Unharnessed** : réveils en mémoire seulement, perdus à la sortie (UH `crates/uh-core/src/session_engine/wakes.rs:60-80`) ; tours `wake: bash` passés rejoués depuis les lignes `async_bash_completion`.
- **Écart** : nos réveils survivent à la fermeture du chat (démon). Une conversation reprise peut donc recevoir un tour `wake` juste après le chargement. L'extension le gère déjà : tour de l'agent au-dessus d'un prompt en attente, `src/chat.ts:781-794` (insertion `:785-788`).

## 7. Plan

### PR R1 (Rust, `ya-28-reprise`)

- **Nouveau module** `crates/pa-daemon/src/acp/ya_history.rs` : rejeu, liste, suppression.
- **Fichiers upstream touchés**, ajouts minimaux listés dans `docs/fork-changes.md` :
  - `acp/daemon.rs` : aiguillage de `session/load`, `session/list` et de la suppression ; le résumé du `create` est gardé (`sessionId`, `sessionFile`) ; l'admission de `session/new` est partagée avec `session/load` (un paramètre `sessionId` imposé) ;
  - `acp/types.rs` : capacités `loadSession: true`, `sessionCapabilities.list` ;
  - `acp/meta.rs` : champ `saved { sessionId, path }` (choix C2).
- `crates/pa-core/src/session_engine/usage_log.rs` (module du fork) : empreinte du préfixe et cause `resume_prefix_change` (choix C8) ; branchement dans `engine.rs`, quelques lignes.
- **Contenu** :
  1. `_meta…saved` dans les réponses de `session/new` et de `session/load` ;
  2. `session/load` : refus d'un autre `sessionId` (« Ce processus tient la session {id} : lancez yusagent avec --resume <son fichier> ») ; rejeu tour par tour depuis `GetSessionMessages` (choix C5), par le mappeur du direct (choix C6) ; tour ouvert à la fin marqué `cancelled` ; tour en cours après le rejeu (choix C9) ;
  3. `session/list` (choix C10, C11) ;
  4. suppression (choix C3, C4) ;
  5. journal : empreinte et invalidation à la reprise (C8).

**Tests Rust** (provider factice, aucun modèle réel) :

- unitaires du rejeu : stored messages → trames, champ par champ (prompt, texte, raisonnement, cellule avec `_yus/bashRuns`, diff avec `_yus/startColumn`, compaction, réveil `bash`, tour coupé) ;
- e2e du pont :
  - `session/new` renvoie un chemin qui existe après le premier tour ;
  - `session/load` sur un autre id est refusé ;
  - `session/list` filtre par `cwd`, exclut sous-agents et brouillons ;
  - suppression : refus de la session courante, refus si un autre client est attaché ; sinon worker arrêté (absent du `list` du démon), fichier et `session-artifacts/<id>` retirés ;
  - `usage_log` : une seule `invalidation` `resume_prefix_change` après un changement de skill, aucune sans changement.

**Cas de capture `acp-resume`** (`bench/tools/capture/run.py`, script de contrôle `resume_check.py`), sur le vrai binaire, démon isolé, `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION` fixé :

| Sous-cas | Déroulé | Attendu |
|---|---|---|
| E (référence) | deux prompts dans le même pont, scénario avec une cellule (instantané présent) | — |
| C, R-vivant | prompt 1, fin de stdin, `--resume <chemin>` sur le même démon, `session/load`, prompt 2 | requêtes = E, corps entier (UUID et chemins normalisés) |
| D, R-neuf | prompt 1, arrêt du démon, `--resume`, `session/load`, prompt 2 | prompt système et tools = E ; historique de E en ajout seul ; seul écart permis : la ligne `[python-state-restored]` avant le prompt 2 (à confirmer par la mesure, puis figé) |
| F, R-neuf avec un skill ajouté entre-temps | comme D, avec un skill de plus dans le projet | prompt système différent, tools identiques ; **une seule** ligne du journal avec `invalidation.cause = resume_prefix_change` |
| Trames | trames du rejeu de C, comparées aux trames en direct du prompt 1 | mêmes `session/update` (hors `thinking` et chronométrage), plus `turnStart` et `turnEnd` |

- Le banc tourne d'abord sur le binaire de `main` (`d2f54743e`) : `session/load` y répond `-32601`, donc C et D ne passent pas encore par `session/load` (contrôle négatif). Puis sur le binaire de la branche.
- Les cas existants (`acp`, `tui`, `acp-extension`) restent identiques ; seule la réponse d'`initialize` change dans les trames (capacités). `compare_frames.py --against <main> --expect` vérifie que seules ces trames changent.

### PR X1 (extension)

- `src/ya/methods.ts` : `session/load`, `session/list` et la suppression deviennent servies ; l'historique apparaît.
- Lecture de `saved` et du chemin de liste dans notre `_meta` (adaptateur `src/ya/meta.ts`, comme `turnStart`), nom de la méthode de suppression (C2, C3).
- « Relancer » reprend par `--resume` et `session/load` (déjà dans le code repris, `src/conversations.ts:65-70`).
- Notification `kernel/restored` selon C7.
- Fixture des trames (`test/fixtures/acp-frames.json`) : ouverture inchangée côté client ; trames de chargement ajoutées.

**Tests de l'extension** :

- unitaires, agent factice : chargement (mode `loading`, tours rejoués, journaux tus), liste, suppression (agent de la conversation arrêté avant la requête, refus affiché) ;
- e2e sur le vrai binaire, provider factice :
  1. nouveau chat, prompt ;
  2. « Relancer » : le tour revient, prompt suivant accepté ;
  3. deuxième conversation, retour à la première par l'historique (affichée telle quelle) ;
  4. suppression de la conversation de fond : fichier retiré, aucun processus restant pour elle (relevé des processus comme `acp_frames.py`).

**Contrôle de Yusuf** (après R1 et X1) : script de contrôle lancé d'abord par Claude Code sur le binaire de la branche (règle 13), sortie jointe.

### Ordre et CI

R1, puis X1. Chaque PR suit la boucle de dev (binaire de la branche, capture `main` contre branche, `ci.yml` vert sur la tête). X1 se teste contre le binaire de R1 mergé.

## 8. Choix à trancher

| # | Choix | Options | Recommandation |
|---|---|---|---|
| C1 | `sessionId` ACP | (a) comme Unharnessed : UUID tiré au hasard pour `session/new`, identifiant du fichier pour `session/load` ; (b) identifiant du fichier dans les deux cas | **(a)** : c'est ce que l'extension reprise attend, elle garde les deux identifiants (`src/session.ts:121-130`), et rien ne change pour `session/new` |
| C2 | Où mettre le fichier de session dans la réponse | (a) `saved { sessionId, path }` dans le `_meta` du pont (`ai.primeintellect.prime-agent`), sous le nom d'Unharnessed, comme `turnStart` (T1 du ticket 25) ; (b) `_yus/session { id, path }` (plan du ticket 24) ; (c) `_meta.unharnessed` tel quel | **(a)** : même raison que T1, l'extension lit déjà ce nom ; écart à la convention du 19/11 (« ajouts sous `_yus/*` »), comme T1. Idem pour `path` des lignes de `session/list` |
| C3 | Nom de la méthode de suppression | (a) `_yus/sessions/delete` ; (b) `_unharnessed/sessions/delete` tel quel | **(a)**, convention du 19/11 pour les méthodes (le plan du ticket 24 nomme déjà `_yus/session/steer`) ; une seule ligne à changer dans `src/history.ts:9` |
| C4 | Ce que fait la suppression | (a) `Kill` de la session vivante, puis `DeleteSavedSession` (fichier, `session-artifacts`), journal d'usage retiré aussi ; refus pour la session courante et si un autre client est attaché ; (b) comme Unharnessed : fichier seulement, refusé tant que le worker vit | **(a)** : c'est le point ouvert du 21/11. Écart à Unharnessed, imposé par le démon |
| C5 | Source du rejeu | (a) `GetSessionMessages` au moment du `session/load`, par pages ; (b) le transcript de la réponse d'attach, déjà reçu au démarrage | **(a)** : état lu au moment du chargement, pagination possible ; (b) garderait tout le transcript en mémoire, et des tours peuvent s'ajouter entre l'attach et le load (R-vivant) |
| C6 | Mise en forme du rejeu | (a) messages stockés passés par le mappeur du direct (`wire_events.rs`, cartes `ya_cell_display.rs`), comme Unharnessed ; (b) mappeur à part | **(a)** : mêmes trames qu'en direct, preuve par la comparaison des trames du cas `acp-resume` |
| C7 | Dire au client qu'un kernel neuf a relu son instantané (R-neuf) | (a) notification `_yus/kernel/restored { sessionId, restored }` après le rejeu, le pont la lisant dans l'état du worker (une commande ou un champ à ajouter au worker, fichier upstream) ; (b) reportée : le modèle est prévenu, l'extension n'affiche pas la ligne | **(b)** dans ce ticket : (a) touche le worker du démon pour une ligne d'affichage ; à reprendre avec le panneau du kernel (étape 6). Écart à Unharnessed, noté |
| C8 | Miss de cache à la reprise | (a) empreinte du prompt système et des tools dans le journal, `invalidation { cause: "resume_prefix_change", parts }` à la première requête reprise si elle change ; (b) prompt système stocké dans le fichier et réutilisé à la reprise | **(a)** : c'est la règle du 21/11 (un seul miss, journalisé) ; (b) garderait une liste de skills périmée, ce que contredit la même règle |
| C9 | Tour de l'agent en cours au moment du `session/load` | (a) `turnStart { origin: "wake" }` publié après le rejeu si le worker est occupé ; (b) rien : les updates arrivent sans tour ouvert | **(a)**, avec un test ; sinon l'extension perd ces updates (`src/chat.ts:712-713`) |
| C10 | Lignes de `session/list` | (a) comme Unharnessed : titre = premier prompt coupé à 80 caractères (nom de session s'il existe) ; sans sous-agents (`parentSessionPath`) ni brouillons (`messageCount` 0) ; tri par `modified` décroissant ; (b) toutes les lignes du démon | **(a)** |
| C11 | Pagination de `session/list` | (a) aucune, comme Unharnessed (sessions d'un dossier) ; (b) `limit`/`cursor` du démon exposés | **(a)** ; à revoir si la liste devient lente (aucune mesure faite) |
| C12 | Même conversation ouverte dans deux fenêtres (deux ponts attachés au même worker, ce que le démon permet) | (a) refus du second `session/load`, « ouverte dans une autre fenêtre », comme le bail d'Unharnessed ; (b) accepté, comme un TUI attaché (E4 du ticket 25) | **(a)** : deux ponts répondraient aux mêmes demandes de permission. Comment reconnaître un autre client ACP attaché : **à vérifier dans R1** |
| C13 | Découpage de R1 | (a) une PR (load, list, suppression, journal) ; (b) R1a (fichier dans `session/new`, `session/load`, `acp-resume`), puis R1b (liste, suppression, invalidation) | **(a)** si la PR reste lisible ; sinon (b). X1 a besoin des trois méthodes pour l'historique |

## 9. Écarts avec Unharnessed, et pourquoi

1. **Kernel gardé quand le worker vit encore** (R-vivant) : rien n'est perdu à la reprise. Chez lui, toujours un kernel neuf. Raison : session résidente du démon.
2. **Suppression qui arrête la session dans le démon** (C4) : chez lui, arrêter le processus suffit.
3. **Noms** : `saved` dans le `_meta` du pont (C2, comme T1) et `_yus/sessions/delete` (C3), au lieu de `_meta.unharnessed` et `_unharnessed/*`. Raison : convention du 19/11, sauf pour les champs que l'extension lit déjà.
4. **Pas de notification `kernel/restored`** dans ce ticket (C7, si (b)).
5. **Réveils persistants** : un tour de l'agent peut avoir eu lieu pendant que le chat était fermé, ou arriver juste après le chargement (§6). Chez lui, les réveils sont perdus à la sortie.
6. **Miss journalisé** (C8) : Unharnessed ne détecte pas de changement de préfixe ; son prompt système contient la date (UH `system_prompt.rs:276`), le nôtre non.
7. **Veille** : chez lui, la veille de 90 min arrête l'agent et le kernel. Chez nous, elle arrête le pont ; le worker suit sa propre veille sans client. Point ouvert ci-dessous.

## 10. Effet sur le préfixe

- **R1** : rien de ce qui part au modèle ne change pour une session neuve ni pour une reprise sans changement. Ce sont des trames ACP en plus (capacités d'`initialize`, `_meta` de `session/new`, rejeu) et une empreinte de plus dans le journal d'usage.
- Le cas `acp-resume` le prouve (C = E ; D = E hors la ligne `[python-state-restored]`, à mesurer) ; les cas `acp`, `tui` et `acp-extension` restent identiques.
- **X1** : extension seule.

## Points ouverts

- **Veille en double** : l'extension ferme le pont d'une conversation de fond après 90 min sans travail ; le worker a sa propre fenêtre de veille, mesurée depuis la dernière activité, qui ne s'arme que sans client attaché (`crates/pa-daemon/src/worker/turn.rs:113-140`). Durée réelle de vie du kernel après la fermeture du pont : non mesurée.
- **Fichier avant le premier prompt** dans notre démon (§2.2) : à vérifier dans R1.
- **Instantané à la veille du worker** (§3) : à vérifier.
- **Ligne `[python-state-restored]` dans le cas D** : à mesurer (§4.1).
- **Ligne « restart »** de l'extension (« The agent restarted: it no longer remembers the messages above. ») : d'après la lecture d'Unharnessed, les relances actuelles vident le chat avant de relancer, donc la ligne semble ne plus apparaître. Sans effet sur ce ticket.

## 11. Décisions de Yusuf (22/11/2026)

- **C2** : oui, `saved { sessionId, path }` dans le `_meta` du pont, sous le nom d'Unharnessed, même raison que `turnStart` (ticket 25, T1). Ligne au journal du CDC (22/11/2026).
- **C3** : `_yus/sessions/delete`, **sauf si l'extension d'Unharnessed appelle déjà une méthode pour supprimer** : elle appelle `_unharnessed/sessions/delete` (`extensions/vscode/src/history.ts:8-9`, `:88`). **Le pont sert donc `_unharnessed/sessions/delete`**, nom d'Unharnessed, comme pour C2.
- **C4** : oui, la suppression arrête la session dans le démon, puis supprime le fichier et son dossier d'instantané. **Le journal d'usage d'une conversation supprimée est gardé** (données de mesure) : `delete_session_file` ne le retire pas (`crates/pa-daemon/src/saved_session_commands.rs:106-150`), rien à ajouter.
- **C7** : oui, ligne « kernel restauré » reportée.
- **C8** : oui, empreinte du prompt système et des tools dans le journal d'usage, une seule entrée `resume_prefix_change` à la reprise.
- **C12** : oui, refus d'ouvrir la même conversation dans deux fenêtres.
- **C1, C5, C6, C9, C10, C11, C13** : comme recommandé (C13 : une seule PR R1).
- **Avant le code de R1** : trois mesures sur le binaire de référence `d2f54743e`, avec un script commité (§12).

## 12. Mesures avant R1 (22/11/2026), binaire de référence `d2f54743e`

Script commité : `bench/tools/capture/resume_probe.py`. Il tourne sur le vrai binaire, avec un démon isolé par cas, le serveur de capture en modèle factice (scénario `basic`, trois cellules), `sandbox-exec`, et le HOME et le venv du banc. Aucun modèle réel.

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/resume_probe.py \
  --binary ~/.cache/yusagent-bin/d2f54743e1b4ac98f40a6a72f3e098c94bf5fe39/yusagent --out <dossier neuf> [--only M1|M2|M3]
```

Rapport : `<sortie>/resume-probe.md` et `.json`.

### M1 : le démon écrit le fichier de session avant le premier prompt

`session/new`, attente de 2 s, relevé de `<agent>/sessions/` : **le fichier existe déjà** (`01a20312-27b5-70b4-807e-dc8256d05785.jsonl`), le même après le prompt.

- **Écart avec Unharnessed**, qui n'écrit le fichier qu'à la première réponse (UH `persist.rs:125-134`).
- **Effet sur R1** : aucun. `session/list` exclut les sessions sans message (C10, `messageCount` 0, `lifecycle: "draft"`, `crates/pa-daemon/src/supervisor/sessions.rs:778-783`).
- **Effet sur X1** : l'extension juge une conversation « vide », et la ferme quand on la quitte, si son fichier n'existe pas (`extensions/vscode/src/conversations.ts:49-53`, `:60-63`, `:220`). Avec notre démon, le fichier existe dès l'ouverture. Une conversation où rien n'a été demandé ne serait donc plus fermée, et son pont resterait ouvert. **X1 devra juger le vide autrement** : aucun tour dans le chat. À trancher dans X1.

### M2 : où apparaît `[python-state-restored]` après une reprise avec un worker neuf

Déroulé : prompt 1 (trois cellules), 4 s d'attente (instantané présent, `session-artifacts/<id>/kernel-state.dill` et `.json`), fin de stdin, **arrêt du démon**, `--resume <fichier>`, `session/new`, prompt 2, prompt 3.

| Lancement | Prompt 2 envoyé | Première requête qui porte l'avis |
|---|---|---|
| 1 (version du script sans prompt 3) | aussitôt | **aucune** des requêtes du prompt 2 |
| 2, 3, 4 | aussitôt | celle du prompt 2 |
| 2, 3, 4 | 10 s après `session/new` | celle du prompt 2 |

Dans tous les cas où il apparaît :
- c'est un **message utilisateur à part** (`role: user`), placé **juste avant** le message du prompt, et pas dans le même message (messages 8 et 9 de la requête) ;
- **le préfixe est intact** : prompt système et tools identiques à ceux de la dernière requête du prompt 1, messages du prompt 1 en tête sans changement ; ne s'ajoutent que la réponse finale du prompt 1, l'avis et le prompt 2 ;
- texte : `[python-state-restored]` … `These names are available again: agent_message, agent_observe, c, goal, os, r, rlm_heartbeat, ya_names, ya_sensitive.` … `The working directory and environment variables were reset to the session defaults; …`.

Contre-épreuve, **worker vivant** (fin de stdin sans arrêter le démon, puis `--resume`) : démon, worker et kernel toujours là avant la reprise ; **aucun avis** ; requête du prompt 2 en ajout seul. Trois lancements, mêmes résultats.

**Lecture** :
- au lancement 1, la restauration du kernel neuf (préchauffé à la création du worker, `crates/pa-core/src/session_engine/engine.rs:733`) a fini après l'admission du prompt 2 ;
- la ligne attend alors le tour suivant (`adopt_next_turn_rows`, `crates/pa-core/src/session_engine/admission.rs:214-234`, prise à l'admission, `:145`) ;
- c'est une course entre la restauration et le premier prompt. L'avis reste en ajout seul dans les deux cas.

**Effet sur le plan** : le cas D de `acp-resume` ne peut pas exiger l'avis devant le prompt 2. Le contrôle sera : prompt système et tools identiques à E ; messages identiques à E **une fois retiré le message de l'avis, s'il est là** ; au plus un message d'avis, toujours juste avant un prompt. Pas de contradiction avec le plan : le préfixe reste intact.

### M3 : durée de vie du worker et du kernel après la fermeture du chat

`idleEvictionMinutes: 1` (le défaut est 90). Processus du démon relevés toutes les 2 s.

| Cas | Fin de stdin après le tour | Worker et kernel arrêtés | Délai après la fin de stdin |
|---|---|---|---|
| K1 | 0,0 s | 61,6 s après le tour | 61,6 s |
| K2 : pont gardé 90 s après le tour | 90,2 s | 151,7 s après le tour | **61,5 s** |

- Pendant les 90 s où le pont reste attaché (K2), worker et kernel restent là, alors que le seuil d'une minute est passé : **pas de veille tant qu'un client est attaché** (`idle_passivation_window` rend `None` avec un client, `crates/pa-daemon/src/worker/turn.rs:196-198`).
- Après la fin de stdin, **la veille repart pour un délai complet**, quel que soit le temps d'inactivité passé avant. Sans ce redémarrage, K2 aurait dû s'arrêter aussitôt.
- L'instantané du kernel est sur disque après la veille, ce qui permet la reprise avec un worker neuf (M2).

**Durée de vie réelle avec les défauts : déduite, pas mesurée à l'échelle** (la mesure prend un seuil d'une minute) :
- la veille de l'extension ferme le pont d'une conversation de fond **90 min** après son dernier travail (`extensions/vscode/src/conversations.ts:11-12`, `:243-250`) ;
- le worker et le kernel restent alors **90 min de plus** (`idleEvictionMinutes`, K2) ;
- donc **environ 3 h** après le dernier travail pour une conversation laissée en fond ;
- **90 min** après la fermeture pour une conversation dont le pont est fermé autrement : fermeture de VS Code, conversation quittée vide, suppression exceptée (C4 arrête le worker).

Pas de contradiction avec le plan : point ouvert inchangé (veille en double), à traiter avec l'étape 6 ou un ticket à part si Yusuf le veut.

## 13. Réalisation de R1 (branche `ya-28-reprise`)

Aucune mesure ne contredit le plan (§12). Code selon les décisions du §11.

- **Pont** (`crates/pa-daemon/src/acp/ya_history.rs`, nouveau ; ajouts minimaux dans `daemon.rs`, `types.rs`, `meta.rs`, `mod.rs`) :
  - `saved { sessionId, path }` dans les réponses de `session/new` et `session/load`, lus dans le résumé du `create` ;
  - `session/load` : autre identifiant refusé (« This process holds session … »), conversation tenue par un autre client qui répond aux permissions refusée (« This conversation is open in another window. », après 3 s d'attente pour un client qui vient de fermer, cas « Relancer ») ;
  - rejeu tour par tour depuis `get_session_messages`, par le mappeur du direct, avant la réponse, sous l'identifiant du fichier ; tour du worker en cours gardé ouvert (`AgentTurns::adopt`) ;
  - `session/list` et `_unharnessed/sessions/delete` (`kill` de la session vivante, attente de sa sortie, puis `delete_saved_session`) ;
  - capacités `loadSession: true` et `sessionCapabilities.list`.
- **Journal d'usage** (`usage_log.rs`) : champ `prefix` ; une seule `invalidation` `resume_prefix_change` au premier appel après une reprise dont le préfixe a changé.
- **Démon** (`ya_permissions.rs`) : action `others`.
- **Tests** :
  - unitaires : rejeu, liste, `saved`, publication, journal ;
  - e2e : `crates/pa-cli/tests/ya_acp_resume_e2e.rs` ;
  - test upstream `acp_initialize_matches_the_ts_golden` mis à jour (capacités).
- **Banc** : cas `acp-resume` (`bench/tools/capture/acp_resume.py`, lancé par `run.py`). Sur le binaire de `main` `d2f54743e` : `SANS OBJET` (E seulement, pas de `loadSession`), commande et sortie ci-dessous.

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_resume.py \
  --binary ~/.cache/yusagent-bin/d2f54743e1b4ac98f40a6a72f3e098c94bf5fe39/yusagent --out <neuf>
…
- le binaire n'annonce pas loadSession

SANS OBJET
```

**Préfixe envoyé au modèle** : inchangé pour une session neuve et pour une reprise sans changement (à prouver par la capture du binaire de la branche). Un seul ajout, en fin d'historique, déjà présent sur `main` : `[python-state-restored]` après une reprise avec un worker neuf (§12, M2).

## 14. Arrêt : un constat contredit C8 (22/11/2026)

Premier run de la PR #78, sur le commit `cca4e7ea9` : `ci` 45020777967 ; build macOS 45020773876 vert. Sur les échecs, deux corrigés en local, commités et pas encore poussés (`YA-10: ticket 28 : tests internes du pont…`) :
- **compilation des tests internes de `daemon.rs`** : littéral `DaemonBinding` sans `file`, deux appels de `handle_session_new` sans `load`. Jobs `check + clippy`, `windows cross-check + clippy` et `test shard 3/8` ;
- **`session/list` vide** dans `ya_acp_resume_e2e` (`test shard 2/8`, job 137203052876). Les lignes de `list_saved_sessions` s'appellent `id`, `path` et `name`, pas `sessionId`, `sessionFile` et `sessionName` (`crates/pa-daemon/src/supervisor/sessions.rs:843-857`, `saved_session_row`). Vérifié sur le démon de `d2f54743e` par un script jetable (scratchpad) : liste, puis `kill` de la session vivante, puis `delete_saved_session` (`{"ok": true, "method": "trash"}`, fichier et `session-artifacts/<id>` retirés). Les deux autres tests e2e (reprise, C12) passent.

### Le constat : aucun journal d'usage sur le chemin du démon

- Le moteur d'une session du démon (ACP, TUI) tourne sur un gestionnaire de session **en mémoire** : « The engine session stays non-persisted (the worker owns the durable file) » (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1056-1071`).
- Le journal d'usage ne s'ouvre que pour une session qui a un fichier (`UsageJournal::for_session`, `crates/pa-core/src/session_engine/usage_log.rs:446-447`, appelé par `engine.rs:439-442`) ; `docs/usage.md` le dit : « Une session en mémoire, sans fichier, n'a pas de journal ».
- **Mesuré** :
  - aucun `*.usage.ndjson` sous les dossiers agent des sessions ACP de `resume_probe.py` (`find /private/tmp/ya-resume-probe -name '*.usage.ndjson'` : rien ; `agent-m2-d/sessions/` ne contient que `<id>.jsonl`, `.info-cache.json`, `.window-cache.json`) ;
  - ni sous celui du script jetable ci-dessus ;
  - les journaux du banc viennent tous des cas `-p`.

**Effets** :
- **C8 ne tient pas sur le chemin de l'extension** : l'empreinte et `resume_prefix_change` sont codées dans le journal, mais une session ACP n'en écrit pas. Le contrôle F du cas `acp-resume` (« une seule entrée `resume_prefix_change` ») échouerait sur le binaire de la branche.
- **Même trou pour la décision du 21/11 (ticket 24)** : « un changement de modèle ou d'effort en cours de session est journalisé comme `invalidation` ». Ce n'est vrai qu'en `-p`. Lecture du code et mesure ci-dessus ; pas de cas de capture qui l'ait vérifié en ACP.

### Choix à trancher

| # | Option | Effet | Fichiers |
|---|---|---|---|
| J1 (a) | **Le moteur du démon ouvre le journal sur le fichier durable** : `<id>.usage.ndjson` à côté de `<id>.jsonl`, même format, même identifiant (l'en-tête du fichier, déjà repris par le gestionnaire en mémoire, `lifecycle.rs:1072-1080`) | C8 tient, et les causes du ticket 24 sont journalisées pour l'extension et le TUI ; les sessions ACP et TUI ont leur usage par appel comme `-p` | une option de configuration du moteur (chemin du journal) dans `pa-core` et son passage dans `pa-daemon/src/agent_engine/lifecycle.rs` (fichier upstream) ; **estimation** : 20 à 40 lignes, plus un test |
| J1 (b) | Garder C8 dans le journal, sans journal côté démon | rien de journalisé pour l'extension : la règle du 21/11 n'est pas tenue | aucun |
| J1 (c) | Journaliser le miss de reprise ailleurs, par exemple une entrée `custom` du fichier de session, jamais envoyée au modèle | règle tenue pour la reprise seulement ; pas d'usage par appel en ACP | worker du démon |

**Recommandation : (a)**, dans cette PR R1 : elle est déjà ouverte, et c'est le seul moyen de tenir C8 pour l'extension. Sinon, dans une PR à part avant X1, et F retiré du cas `acp-resume` d'ici là.

### Autre blocage, indépendant de cette PR

- Le check requis `deny (cargo-deny advisories + licenses)` échoue sur un nouvel avis : **RUSTSEC-2026-0592**, `url` 2.5.8 (`Cargo.lock:272`, « Stack overflow in `Url::origin()` on nested `blob:` URLs »).
- `Cargo.lock` n'est pas touché par cette PR. L'échec vaut pour toute PR tant que `url` n'est pas monté.
- Correctif hors du périmètre du ticket 28 (montée de version d'une dépendance) : à trancher, PR à part ou dans celle-ci.

## 15. Décisions de Yusuf sur le §14 (22/11/2026) et réalisation de J1

- **`deny`** : PR à part, d'abord.
  - Branche `ya-deps-url`, PR #79 : `url` 2.5.8 → 2.5.9, version corrigée selon l'avis (`patched = [">= 2.5.9"]`, `RUSTSEC-2026-0592.md` de `rustsec/advisory-db`) ; `form_urlencoded` 1.2.2 → 1.2.3, dépendance de `url` 2.5.9.
  - Upstream n'avait pas monté `url` (`git show upstream/main:Cargo.lock` : 2.5.8), donc pas de cherry-pick. Aucune exception dans `deny.toml`.
  - Après le merge : build macOS de `main`, puis rebase de `ya-28-reprise` sur `main`.
- **J1 (a)**, dans la PR #78. Le journal d'usage est un ajout de yusAgent (T04, `docs/fork-changes.md:56-58`) : chemin oublié par notre fonction. Unharnessed n'a pas de journal d'usage : pas de décision à reprendre.
  - **Réalisé** : `UsageJournal::for_session` reçoit le `conversation_log_path` du moteur, que seul le démon pose (le fichier durable du worker, `crates/pa-daemon/src/agent_engine/lifecycle.rs:1184`) et que le moteur utilise déjà en repli pour le journal de conversation et le dossier d'artefacts (`crates/pa-core/src/session_engine/engine.rs:225-236`, `:494-503`). Le parent d'une session enfant est lu dans l'en-tête du fichier durable. **Aucun fichier du démon touché.**
  - **Tests** :
    - e2e `an_acp_session_journals_its_calls_and_an_effort_change` (`ya_acp_resume_e2e.rs`) : journal présent à côté du fichier, `prefix` posé, changement d'effort journalisé ;
    - cas `acp-resume` : journal de chaque session ACP (une ligne `main` par requête de la boucle) et sous-cas G (changement de modèle, `model_change`, seule invalidation).
  - **Vu en échec sans le correctif** : le contrôle du journal du cas `acp-resume` sur le binaire de `main` `d2f54743e` (sortie ci-dessous). Le test e2e Rust n'a pas été lancé sans le correctif : pas de build Rust local (règle 8), et un seul push demandé pour la PR #78.

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_resume.py \
  --binary ~/.cache/yusagent-bin/d2f54743e1b4ac98f40a6a72f3e098c94bf5fe39/yusagent --out <neuf>
# Cas `acp-resume` (ticket 28)

- le binaire n'annonce pas loadSession
- journal d'usage de la session ACP E : False

SANS OBJET
```

## Notes pour X1

- **M1** (§12) : le démon écrit le fichier de session dès `session/new`. L'extension juge une conversation « vide », et la ferme quand on la quitte, si son fichier n'existe pas (`extensions/vscode/src/conversations.ts:49-53`, `:60-63`, `:220`). **X1 doit juger autrement qu'une conversation est vide** : aucun tour dans le chat, par exemple.
- Méthode de suppression : `_unharnessed/sessions/delete` (C3), nom déjà appelé par `src/history.ts:9`. Champs à lire dans notre `_meta` (`ai.primeintellect.prime-agent`) : `saved`, et `path` et `current` des lignes de `session/list` (C2).

## 16. Contrôles du binaire de la branche (22/11/2026)

- **Merge et builds** :
  - PR #79 (`url` 2.5.9) mergée sur ton « merge » : `main` à `10b16ae91`. CI avant le merge : 16 checks requis verts sur `e7bbad09b`, `deny` compris.
  - Build macOS de `main` lancé à la main (le workflow ne se déclenche pas sur `main`) : run 45023602727, vert.
  - `ya-28-reprise` rebasée sur `main` sans conflit, poussée une fois (tête `b1e350ab4`) ; build macOS de la branche : run 45023614487, vert.
- **Binaires** : SHA256 vérifiés (`shasum -a 256 -c SHA256SUMS`), extraits dans `~/.cache/yusagent-bin/<sha>/`.

### Capture complète, `main` contre la branche

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py \
  --binary ~/.cache/yusagent-bin/10b16ae911255c4544c3b50403d17489866d4aa7/yusagent --out runs/ya28-main-10b16ae91
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py \
  --binary ~/.cache/yusagent-bin/b1e350ab423b49d3f7be4b3fe4bae52d224e6788/yusagent --out runs/ya28-branch-b1e350ab4
python3 -I bench/tools/capture/compare.py --all runs/ya28-main-10b16ae91 runs/ya28-branch-b1e350ab4 \
  --out runs/ya28-compare-main-branch.md
```

- Les deux runs finissent avec `exit=0` (contrôles des chemins avec démon, des trames de l'extension, de l'usage et du masquage passés).
- `spawn : ok=False` dans les deux runs, comme dans les captures du ticket 26 (`runs/ya26-prefix-main-ea3908688.log`) : préexistant, sans lien avec ce ticket.
- **`compare.py --all` : TOUT IDENTIQUE.** 17 chemins, dont `acp`, `acp-extension` et `tui`, toutes les requêtes identiques corps entier (UUID normalisés). Seul écart des premières requêtes : `prompt_cache_key` des cas Codex (un UUID par lancement).
- **`acp-resume`** : `SANS OBJET` sur `main` (pas de `loadSession`), **CONFORME** sur la branche :

| Contrôle | Résultat |
|---|---|
| C, reprise worker vivant : requêtes identiques à E, corps entier | oui (5/5) |
| D, reprise worker neuf : identiques à E hors avis `[python-state-restored]` | oui (avis seul, message 8 de la 5ᵉ requête, juste avant le prompt 2) |
| F, skill ajouté : prompt 1 identique, puis seule la ligne du skill change le prompt système | oui |
| Journal : une seule `resume_prefix_change`, dans F, au 1er appel repris (`parts: ["system"]`) ; aucune dans E, C, D | oui |
| Journal d'usage de chaque session ACP (E, C, D, F, G) | oui |
| G : `model_change` (`capture/capture-model` → `capture/capture-model-2`) au 1er appel après le changement, seule invalidation | oui |
| Trames C et D : `saved` dans la réponse de `session/load`, rejeu avant la réponse, sous l'identifiant du fichier, entre `turnStart` et `turnEnd`, mises à jour = direct (7/7) | oui |

### Trames ACP, `main` contre la branche

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_frames.py --binary <main> --out frames-main
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_frames.py --binary <branche> --out frames-branch
P=.result._meta.ai.primeintellect.prime-agent
python3 -I bench/tools/capture/compare_frames.py frames-branch --against frames-main \
  --expect .result.agentCapabilities.sessionCapabilities.list --expect .result._meta --expect $P \
  --expect $P.saved --expect $P.saved.path --expect $P.saved.sessionId
```

- Résultat : **`écarts : 0`**. `A-display-auto` (26 trames) et `B-basic-manual-reject` (14 trames) identiques hors les écarts attendus, présents chacun dans deux trames : `sessionCapabilities.list` (réponse d'`initialize`) et `saved` (réponse de `session/new`).
- `loadSession` passe de `false` à `true` dans la réponse d'`initialize` : même type, l'outil ne le compte pas comme écart ; relevé à part dans les deux transcripts.
- `compare_frames.py` retouché (`values_at`) : un écart attendu peut être un objet (`list: {}`, `_meta`, `saved`) ; avant, seule une valeur simple comptait comme présente.

**Préfixe envoyé au modèle** : inchangé pour une session neuve (`acp`, `acp-extension`, `tui`, tous les cas `-p` : TOUT IDENTIQUE) et pour une reprise sans changement (C = E ; D = E hors l'avis déjà présent sur `main`).

## 17. Réalisation de X1 (branche `ya-28-x1`, PR #80)

Point de départ : `main` à `9937e4ff8` (merge de la PR #78, R1). Binaire de référence : `~/.cache/yusagent-bin/9937e4ff81689db8918a910816d1c12692fb93be/yusagent`.

### Décisions de Yusuf (22/11/2026)

- **D1** : vide = aucun tour dans le chat, aucun message en attente, rien en cours. Une conversation vide se ferme quand on la quitte, comme chez Unharnessed.
- **D2 (b)** : `_unharnessed/sessions/delete` sur la conversation vide quittée, une fois la session de la nouvelle conversation ouverte. Vérifié d'abord que la suppression accepte une session sans message (ci-dessous).
- **D3 (a)** : ligne claire avec le message du pont et un bouton « Restart » qui réessaie, agent arrêté ; même affichage pour tout échec de `session/new` ou `session/load`.
- **D4** : tour affiché en cours et arrêtable, message tapé pendant ce temps mis en attente jusqu'à la fin du tour, sans durée affichée.

### Contrôle avant D2 : suppression d'une session sans message

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/empty_delete_probe.py \
  --binary ~/.cache/yusagent-bin/9937e4ff81689db8918a910816d1c12692fb93be/yusagent \
  --out bench/tools/capture/runs/ya28-x1-empty-delete-9937e4ff8
…
A_fichier_avant : true
liste_ids : []
arbre_avant : {"daemon": [56125], "worker": [56126, 56129], "kernel": [56142, 56148]}
suppression : {"result": {}}
suppression_ms : 124
A_fichier_apres : false
A_artifacts_apres : false
arbre_apres : {"daemon": [56125], "worker": [56129], "kernel": [56148]}
CONFORME
```

La session vide (pont A, `session/new` sans prompt, puis fin de stdin) est absente de `session/list`, la suppression demandée par le pont B répond `{}`, son fichier est retiré et son worker et son kernel sont arrêtés. Chaque session neuve a son kernel dès `session/new` (deux kernels pour deux sessions vides, `crates/pa-daemon/src/agent_engine/lifecycle.rs:1201`).

### Code

- **Méthodes et `_meta`** : `session/load`, `session/list` et `_unharnessed/sessions/delete` servies (`extensions/vscode/src/ya/methods.ts`), l'historique apparaît ; `yaResultMeta` lit `saved`, `path` et `current` dans le `_meta` du pont (`src/ya/meta.ts`), utilisé par `src/session.ts` et `src/history.ts` (`current` sert tant que la conversation du panneau n'a pas d'identifiant).
- **D1** : `Conversation.empty` = rien en cours ni en attente et aucun tour (`src/conversations.ts`, `Chat.hasTurns` dans `src/chat.ts`).
- **D2** : `Conversation.fresh` (ouverte neuve) ; quittée vide, elle se ferme, puis `dropEmpty` (`src/ya/empty.ts`) demande la suppression à l'agent de la conversation qui la remplace, une fois sa session ouverte. Une conversation ouverte depuis un fichier (historique, refus) n'est jamais supprimée ainsi.
- **D3** : un échec de `session/new` ou `session/load` appelle `Agent.abandon` (`src/agent.ts`) : état `failed` avec `session`, processus arrêté (il ne reste pas attaché au worker). Le panneau affiche « Conversation not opened », la raison du pont et « Restart » (`webview/Status.tsx`) ; « Restart » relance sur le fichier (`--resume`, `session/load`).
- **D4** : à la fin du chargement, un tour rejoué encore ouvert (C9) devient le tour en cours (`live`) : « Stop » affiché, messages en attente jusqu'à sa frontière, sans durée (`src/chat.ts`, `loaded`).
- **Menu de l'historique** : règles CSS provisoires (lignes, corbeille), la feuille minimale n'en avait pas.

### Tests

- **Unitaires** (`test/unit/meta.test.ts`) : `saved`, `path`, `current` lus dans notre `_meta` ou dans `_meta.unharnessed` ; méthodes du ticket servies, historique affiché. `test/unit/agent.test.ts` : le cas « méthode non servie » prend `_unharnessed/session/rewind`, `session/list` étant servie. `npm run test:unit` : `ℹ tests 59`, `ℹ pass 59`.
- **e2e vrai binaire** (`test/e2e/suite-real.ts`, clics sur l'élément réellement sous le pointeur) :
  1. conversation neuve quittée vide : sa session absente de `list --json` du démon, son worker arrêté, son fichier retiré (D1, D2) ;
  2. « Restart » après un SIGKILL du pont : la conversation revient par `session/load`, avec le tour `wake: bash` fait par le worker pendant que le chat était fermé, puis prompt suivant accepté ;
  3. tour de l'agent en cours au chargement : affiché `running`, chat occupé, message tapé en attente jusqu'à la frontière du tour, tour fini `done` sans durée (D4) ;
  4. même chose, puis « Stop » par un clic : tour `cancelled`, la suite du tour ne vient pas (cellule de 8 s, rien 12 s après) ;
  5. historique : deuxième conversation, puis retour à la première par sa ligne, telle quelle (même processus) ;
  6. double ouverture : un second client ACP (le binaire lancé par le test) tient la conversation ; ouverte par l'historique, « Conversation not opened » et « This conversation is open in another window. », agent arrêté ; le client fermé, « Restart » la reprend depuis son fichier (C12, D3) ;
  7. suppression d'une conversation de fond par la corbeille puis « Delete » : ligne retirée, fichier et `session-artifacts/<id>` retirés, session absente du démon, worker et agent arrêtés.
- **Vus en échec sans le correctif** : premier push de la PR #80, tests seuls (commit `5e326d730`, extension de `main`). `ya-extension` run 45033767797, job `e2e vrai binaire` 137242853737 : `e2e vrai binaire : 7/13 réussis`, les 6 cas du ticket en échec, `processus restants : aucun`. Les échecs tombent au premier manque : `saved` non lu (`fichier de la conversation vide`, `fichier de la conversation`), bouton de l'historique masqué (`clic sur « History »`), réponses du script décalées par les cas précédents (`réponse avant le plantage`). Le job `extension` (agent factice réaligné, sans le correctif) : vert. Le cas 4 (Stop après chargement) a été ajouté après ce push : sans le correctif, le chat n'est pas occupé après le chargement (constat du cas 3), donc pas de bouton « Stop » ; il n'a pas été vu en échec en CI.

### Agent factice réaligné

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_frames.py \
  --binary ~/.cache/yusagent-bin/9937e4ff81689db8918a910816d1c12692fb93be/yusagent \
  --out bench/tools/capture/runs/ya28-x1-frames-9937e4ff8
python3 -I bench/tools/capture/compare_frames.py bench/tools/capture/runs/ya28-x1-frames-9937e4ff8
```

- `compare_frames.py` compare maintenant aussi l'ouverture : capacités d'`initialize` (et la valeur de `loadSession`), `_meta` de la réponse de `session/new`.
- Agent factice de `main` : `écarts : 13` (`loadSession` faux, `sessionCapabilities`, `promptCapabilities`, `mcpCapabilities` et `saved` absents). Agent factice réaligné : `écarts : 0`.
