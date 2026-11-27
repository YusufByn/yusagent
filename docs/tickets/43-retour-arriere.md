# Ticket 43 : retour en arrière et panneau du kernel : constat et plan

Ticket CDC : YA-18 (checkpoints et annulation), avec YA-10 (extension). Point 6 de l'ordre par utilité du ticket 24 (`docs/tickets/24-ya10-extension-minimal.md:725-727`, `:738`) : retour en arrière, après l'état du kernel qui ne suit plus la conversation, puis panneau du kernel.

Branche `ya-43-retour-arriere`, depuis `main` à `f7c4b3fb7` (merge de la PR #96). Premier commit : le commit de doc `5ea4f58b5` du ticket 42, reporté par cherry-pick sans conflit (`b03f1da5c`). Les branches locales `ya-42-doc-a-reporter` et `ya-42-jeton-session-vivante` sont supprimées.

**Constat et plan seulement** : aucun code produit, rien dans `docs/fork-changes.md`, rien compilé, aucun appel de modèle. Les lignes citées sont celles de `main` à `f7c4b3fb7`. Une seule pièce jointe : un script de mesure de git seul (annexe), lancé par Claude Code.

Sources :
- **Upstream** : `upstream/main` à `32f83edb1` (récupéré au ticket 42). Base du fork : `c83ac68b3` (02/11/2026).
- **Unharnessed** : clone jetable dans le dossier temporaire de la session, à **`93b4baf623ce70d28752588454a13100c23cbd7f`** (26/11/2026, « The extension reaches the Marketplace through Trusted Publishing »), lecture seule, rien exécuté. Chemins préfixés `uh:`. Rien n'a changé depuis `157dcdc1d` (19/11, base de l'extension reprise) pour le retour en arrière ni pour le panneau du kernel : `git diff --stat 157dcdc1d..HEAD` est vide sur `uh:crates/uh-daemon/src/acp/rewind.rs`, `kernel.rs`, `uh:vscode/src/kernel.ts`, `uh:vscode/webview/Kernel.tsx`, `uh:vscode/src/chat.ts` et `uh:unharnessed-runtime/src/rlm/variables.py`.

## Résumé

- **Unharnessed ne restaure aucun fichier.** Son « retour en arrière » sert à modifier un message déjà envoyé, puis à le renvoyer :
  - le pointeur de feuille de la session revient avant le prompt ;
  - le kernel garde son état ;
  - aucun texte n'est envoyé au modèle ;
  - il n'y a ni aperçu de diff ni choix « code / conversation ».
  
  YA-18 (instantanés du workspace, restauration, détection des fichiers modifiés, nettoyage) n'a donc **aucune référence chez Unharnessed** à reprendre.
- **Chez nous, tout le nécessaire existe pour la conversation, rien pour les fichiers** :
  - le worker sait déplacer la feuille (`navigate_tree`) ;
  - l'extension reprise envoie déjà `_yus/session/rewind { sessionId, to }` à partir du `rewindTo` de `turnStart` ;
  - le pont ne remplit jamais `rewindTo` et ne sert pas la méthode.
- **Le module `workspace_snapshot` d'upstream ne convient pas** :
  - il capture un worktree git vers un dossier, pour le cloud ;
  - il ne restaure rien, n'est branché nulle part et refuse les projets sans git ;
  - **upstream l'a supprimé le 12/11/2026** (`20b4ad74b`, « drop the unused workspace_snapshot module »).
- **Mesure préliminaire, git seul** (dépôt shadow hors du workspace, 10 000 fichiers, Apple M5, git 2.55, sans l'enveloppe) :
  - restauration d'un tour en 88 à 93 ms ;
  - instantané d'un tour en 91 à 157 ms ;
  - premier instantané en environ 3 s ;
  - dépôt de l'utilisateur inchangé.
  
  Le critère des 2 s paraît tenable. À confirmer dans yusAgent, avec l'enveloppe.
- **Recommandation**, en quatre PR :
  1. pont : retour de la conversation comme Unharnessed, avec un texte fixe au modèle sur l'état du kernel ;
  2. extension : activer l'édition d'un message envoyé ;
  3. instantanés du workspace par tour dans un dépôt git shadow (YA-18), avec restauration et extension ;
  4. panneau du kernel.
  
  Détail au §G. Questions au §G.7 ; réponses de Yusuf au §H ; plan de code de la PR 1 au §I.

## A. Ce qui existe déjà

### A.1 Instantanés du workspace d'upstream (`crates/pa-core/src/workspace_snapshot/`)

- **Quoi** : capture « portable, vérifiable » de l'état de travail d'un worktree git (`crates/pa-core/src/workspace_snapshot/mod.rs:1-24`) :
  - écart à HEAD (fichiers modifiés, indexés, supprimés) et fichiers non suivis, plus en option l'arbre de HEAD (`BaselineMode`, `mod.rs:80-98`) ;
  - blobs adressés par SHA-256 et un `manifest.json` (`mod.rs:167-172`) ;
  - bornes : 20 000 entrées, 512 Mio, 64 Mio par fichier, 10 s par appel git (`mod.rs:66-78`).
- **Quand** : jamais. Seule référence hors du module : `pub mod workspace_snapshot;` (`crates/pa-core/src/lib.rs:66`). L'en-tête le dit : « nothing wires it to a user-facing toggle yet, and there is no transport here - staging and verification only » (`mod.rs:21-24`).
- **Où** : dans un dossier de staging choisi par l'appelant, hors du worktree, en 0700 (`mod.rs:167-169`). Ni refs git, ni dépôt à part.
- **Limites pour YA-18** :
  - pas de restauration (il n'y a pas de « materializer » dans le dépôt, seulement des mentions dans `manifest.rs:31-51`) ;
  - erreur `NotAWorktree` hors d'un dépôt git (`mod.rs:207`) ;
  - Unix seulement (`mod.rs:204-206`) ;
  - exclut par nom `.env`, `.envrc`, `.npmrc`, `*.pem`, `*.key`, etc. (`mod.rs:942-962`) : un retour en arrière ne restaurerait pas un `.env` modifié par l'agent.
- **Lancement de git** : déjà par l'enveloppe (`pa_types::platform::ya_guard::tokio("git")`, `crates/pa-core/src/workspace_snapshot/git.rs:121`, ticket 41).
- **Upstream** : module supprimé par `20b4ad74b` (12/11/2026, 9 fichiers, −3 783 lignes). Construire dessus irait contre upstream. Le retirer chez nous est un cherry-pick à part, hors de ce ticket.

### A.2 Session JSONL : branche et pointeur de feuille

- **Arbre** : chaque entrée a un `parentId`. La feuille est l'entrée d'où part le prochain ajout.
- **`navigate_tree { targetId, summarize?, label? }`** (worker, upstream, `crates/pa-daemon/src/branch_navigation.rs:131-328`) :
  - sans effet si la cible est déjà la feuille (`:172-175`) ;
  - **interrompt le tour en cours** avant de bouger (`wait_turn_end`, `:178` ; `abort_requested`, `:497-510`) ;
  - point d'arrivée (`navigation_point`, `:546-555`) :
    - si la cible est un message user ou un `custom_message`, la feuille va à son **parent** et le texte est rendu (`editorText`) ;
    - sinon, la feuille va à la cible ;
  - sans résumé : `store.branch_to(new_leaf)` (`:298`), puis contexte du moteur reconstruit (`rebuild_engine_context`, `:316`, `:532-542`) ;
  - « the kernel stays warm (`fork` is the only teardown flow) » (`:131-133`).
- **`branch_to` ne fait que changer un champ en mémoire** (`crates/pa-daemon/src/session_tree.rs:140-151`) :
  - rien n'est écrit dans le fichier ;
  - la nouvelle feuille n'est inscrite qu'au prochain ajout, par son parent (`session_store/index.rs:143`) ;
  - à la relecture, la feuille est la dernière entrée du fichier (lecteur à rebours `crates/pa-core/src/session/window.rs:37`, première id vue `:368-370`, reprise `session_store/read.rs:242`) ;
  - conséquence : **après un retour en arrière et avant le prompt suivant, une reprise (« Relancer », redémarrage du démon) revient à l'ancienne branche**. Même chose chez Unharnessed (§B.1, déduit).
- **Lire la feuille** :
  - `leafId` est dans l'état du worker (`crates/pa-daemon/src/types.rs:231-232`, rempli `worker/summary.rs:119`) ;
  - `get_session_tree` donne aussi tout l'arbre (`branch_navigation.rs:63-75`), ce qui est lourd sur une longue session.
- **`fork`** : copie la branche dans un nouveau fichier et redémarre le runtime (`prepare_fork`, `branch_navigation.rs:334` ; `session_tree.rs:226`). Ce n'est pas ce que fait Unharnessed.

### A.3 Instantané du namespace du kernel (reprise après crash ou veille)

- **Un seul fichier par session, réécrit** :
  - `kernel-state.dill` et `kernel-state.json` (`crates/pa-core/src/kernel/state_snapshot.rs:44-55`) ;
  - dans `<dossier agent>/session-artifacts/<id>/` (`crates/pa-core/src/session_engine/rlm_host.rs:319-322`, configuré `crates/pa-core/src/kernel/provisioner.rs:1110-1116`).
- **Quand** : 1 500 ms après chaque cellule réussie (`debounce_ms`, `crates/pa-core/src/kernel/shared.rs:374-375` ; `schedule_snapshot`, `kernel/manager/snapshot.rs:347-365`), et au dispose (`snapshot.rs:390`).
- **Quoi** : sérialisation dill variable par variable, avec plafonds de 256 Mio au total et 16 Mio par variable (`shared.rs:370-373`) :
  - côté Python : `_snapshot_state` (`prime-agent-runtime/src/rlm/repl.py:772`) et `_restore_state` (`repl.py:1123`) ;
  - objets non sérialisables listés et non restaurés ; répertoire courant et environnement remis aux défauts.
- **Restauration** : `perform_restore` (`snapshot.rs:204-260`) au démarrage d'un kernel neuf. Le modèle reçoit `[python-state-restored]` (`crates/pa-core/src/session_engine/state_restore_notice.rs:24`, texte complet `:101`, `:117`), qui dit « revived from your previous session ».
- **Fork, kernel mort pendant une cellule** (`crates/pa-core/src/session_engine/ya_kernel_restart.rs:1-12`) :
  - redémarrage immédiat avec la même restauration, et avis en fin de résultat de la cellule (`:25`, `:28`) ;
  - c'est la brique existante pour « redémarrer le kernel et restaurer un instantané » (option D3, §D).
- **Aucun instantané par tour.** Rien ne relie un instantané à une entrée de la session.

### A.4 Commandes du worker pour le panneau du kernel

- `list_kernel_bash`, `tail_kernel_bash`, `kill_kernel_bash` (`crates/pa-daemon/src/protocol.rs:156-158`) :
  - aiguillées dans `worker/commands.rs:130-133`, servies par `handle_kernel_bash_activity` (`crates/pa-daemon/src/user_bash.rs:130-185`) ;
  - elles passent à côté de la file des cellules (`crates/pa-core/src/kernel/manager/mod.rs:636-643`, délai de 3 s).
- Lignes de `list` (`prime-agent-runtime/src/rlm/bash.py:1889-1900`) : `id`, `command` (512 caractères), `pid`, `startedAt`, `durationMs`, `status`, `exitCode`, `persistent`. **Absents** par rapport à la forme d'Unharnessed : `killed`, `ports` et le filtre « en arrière-plan » (§B.5).
- **Liste et aperçu des variables : absents** (aucune requête `variables` dans `prime-agent-runtime/src/rlm/`).
- **Aucun événement** du kernel vers le pont : rien d'équivalent au `watch_kernel` d'Unharnessed (§B.5).

### A.5 Pont ACP

- `turnStart.rewindTo` est déclaré avec le nom d'Unharnessed (`crates/pa-daemon/src/acp/ya_turns.rs:37-50`), mais « the bridge has no rewind yet and never sets it » (`:47`). `prompt_start()` le laisse vide (`:244-246`), à l'appel de `crates/pa-daemon/src/acp/daemon.rs:1633`.
- Méthodes `_yus/*` servies : skills, MCP, réglages, providers, `sessions/delete`, `session/steer`. **Ni `_yus/session/rewind` ni `_yus/kernel/*`.** Les modules du fork s'aiguillent dans `daemon.rs:1149-1165`, environ 4 lignes upstream par famille (tickets 31, 39, 40).
- `restore_next_turn { messages }` (worker, upstream, `crates/pa-daemon/src/session_custom.rs:33-60`) range des lignes custom que le tour suivant livre « as prefix rows, in order, before the accepted prompt ». C'est une voie sans ligne upstream pour un texte fixe après un retour en arrière, **à prouver par un test** : la ligne doit atteindre la requête au fournisseur (au ticket 31, `AppendCustomMessage` ne l'atteignait pas).

### A.6 Extension reprise d'Unharnessed (`extensions/vscode/`)

- **Retour en arrière = modifier un message envoyé, puis le renvoyer** :
  - clic sur le message user (`aria-label` « Edit this message », `webview/Chat.tsx:307-319`), possible si `!chat.busy && entry.rewindTo !== undefined` (`Chat.tsx:184`) ;
  - à l'envoi, `_yus/session/rewind { sessionId, to: turn.rewindTo }` (`src/chat.ts:657-670`), puis retrait du tour modifié et des suivants (`entries.splice`), puis `session/prompt` (`chat.ts:600-612`) ;
  - textes d'échec : « It could not go back to the message: {error} » (`chat.ts:606`) et « it is no longer in the agent's conversation. » (`chat.ts:660`) ;
  - `rewindTo` lu dans `turnStart` (`chat.ts:67`, `:799`, `:813-814`, `:849`), effacé quand le processus de l'agent change (`chat.ts:186-190`).
- **Masqué aujourd'hui** :
  - `features().rewind = served("_yus/session/rewind")` est faux (`src/ya/methods.ts:55-58`) ; l'appel est refusé sur place (`methods.ts:1-4`, test `test/unit/agent.test.ts:85-87`) ;
  - en pratique, c'est l'absence de `rewindTo` qui bloque : le clic dépend de `rewindTo`, pas de `features.rewind` (`Chat.tsx:184`).
- **Panneau du kernel** :
  - client `src/kernel.ts` : notification `_yus/kernel/changed`, requêtes `_yus/kernel/tail`, `kill` et `preview` (`:6-11`) ; 7 lignes de suite (`:13`) ; listes vidées quand le processus de l'agent change (`:34-39`) ;
  - formes `KernelCommand`, `KernelVariable` et `VariablePreview` (`src/protocol.ts:452-498`) ;
  - vue `webview/Kernel.tsx`, titres « Running servers » (`:231`) et « Agent's REPL variables » (`:247`), « Shown once the agent's code is done » (`:442`) ; rendue seulement si `features.kernel` (`webview/Composer.tsx:385`).
- **Ligne « kernel restauré »** :
  - notification `_yus/kernel/restored { sessionId, restored }` (`chat.ts:38`, `:170`, `:557-575`) ;
  - textes « Python kernel restored » et « The Python kernel could not be restored » (`Chat.tsx:213-216`) ;
  - reportée au ticket 28 (C7).
- **Rien pour restaurer des fichiers** : ni aperçu de diff de tour, ni choix code / conversation. Le seul « restaurer » est le « Rejeter » d'un diff de cellule (ticket 24, PR 3).

## B. Unharnessed (`93b4baf62`)

### B.1 Retour de la conversation

- **Méthode** : `_unharnessed/session/rewind { sessionId, to }`, réponse `{}` (`uh:crates/uh-daemon/src/acp/rewind.rs:1-9`, `:19`, `:66`).
- **Refus** :
  - session inconnue (`-32603`, `:42-45`) ;
  - **tour en cours** : `-32603`, « The agent is working: stop it before going back. » (`:48-53`) ;
  - `to` absent de la branche : `-32602`, avec `data.details` « {id} is not in the conversation » (`:55-62`, `uh:crates/uh-core/src/session_engine/compaction_arms.rs:439`).
  
  Le verrou de la connexion est tenu pendant l'opération (`rewind.rs:34-36`).
- **Effet** (`compaction_arms.rs:433-449`) : vérifie que `to` est sur la branche, puis `session.branch(to)` (feuille en mémoire), puis `agent.set_messages(...)` :
  - l'état du goal suit la branche (`uh:.../engine.rs:756-764`) ;
  - pas de troncature, pas de fork, pas d'entrée ajoutée ;
  - « What came after stays in the session file on a branch of its own » (`rewind.rs:5-6`).
- **`rewindTo`** = la feuille juste avant le prompt (`get_leaf_id()`), publiée dans le `session_info_update` qui ouvre le tour (`uh:crates/uh-daemon/src/acp/prompt.rs:320-350`) :
  - absent pour un réveil (`:328`) ;
  - présent pour un steer : feuille avant le message (`uh:.../acp/session.rs:315-333`) ;
  - au rejeu de `session/load` : parent de l'entrée du prompt (`uh:.../acp/history.rs:175`, `:367-388`).
- **Feuille non inscrite avant l'ajout suivant** : même mécanisme que chez nous. Le rejeu prend la dernière entrée (`uh:.../acp/history.rs:155-160`) (déduit, non mesuré).

### B.2 Retour du code : absent

- Le même module `workspace_snapshot` est présent et inutilisé (`uh:crates/uh-core/src/lib.rs:68`).
- `capture_git_worktree_snapshot` (`uh:crates/uh-core/src/autonomous/gates.rs:282-325`) est une empreinte qui évite de relancer un gate qualité tant que l'arbre n'a pas changé. Rien à voir avec un retour en arrière.
- Recherches sans résultat côté fichiers :
  - `GIT_INDEX_FILE`, `write-tree`, `read-tree`, `checkout-index`, `shadow`, `checkpoint`, `backup`, `undo`, `revert` ;
  - côté extension, `restore|checkpoint|snapshot|undo`, en dehors de « kernel restored ».

### B.3 Kernel au retour en arrière

- « the kernel keeps what it holds » (`rewind.rs:7`) : ni redémarrage, ni restauration, ni avis au modèle.
- Instantané dill : un par session, comme chez nous, pour la reprise seulement (`uh:crates/uh-core/src/kernel/state_snapshot.rs:1-4`).
- **Commandes `bash()` en arrière-plan** :
  - rien ne les tue ;
  - les avis `[bash-done]` en attente ne sont pas vidés (`uh:crates/uh-core/src/session_engine/wakes.rs:62-80`, `drop_notices` seulement après un goal terminé, `:192-194`) ;
  - une commande lancée dans un tour abandonné réveille donc la session sur la nouvelle branche (déduit).

### B.4 Textes au modèle après un retour en arrière : aucun

Recherche de `rewound`, `went back`, `workspace-restored` et `python-state-restored` : seul `[python-state-restored]` existe, et il sert à la reprise de session (`uh:crates/uh-core/src/session_engine/state_restore_notice.rs:44-60`).

### B.5 Panneau du kernel

- **Spécification** en tête de `uh:crates/uh-daemon/src/acp/kernel.rs:1-21`.
- **`_unharnessed/kernel/changed { sessionId, commands?, variables? }`**, chaque liste envoyée seule quand elle change (`kernel.rs:92-98`, `:123-127`) :
  - commande : `{ id, command, startedAt, status, exitCode, killed, ports, durationMs? }`, seulement les handles en arrière-plan (`:166-195`) ;
  - ports : `lsof -nP -a -iTCP -sTCP:LISTEN -g <pgids> -Fgn`, délai de 1,5 s (`:203-245`).
- **`kernel/restored { sessionId, restored }`** : une fois par restauration (`:134-158`).
- **Requêtes** :
  - `tail { id, lines }` → `{ tail }` ;
  - `kill { id }` → `{ killed }` ;
  - `preview { name }` → `{ preview, style, truncated }`, erreur `busy` pendant une cellule (`:283-316`).
- **Surveillance** (`:42-129`) :
  - événements du kernel regroupés pendant 40 ms ;
  - variables relues après chaque cellule ;
  - sondes des ports toutes les 2 s pendant 60 s pour une commande jeune, puis un battement de 20 s.
- **Runtime** :
  - `variables` et `variable_preview` passent à côté de la file des cellules (`uh:crates/uh-core/src/kernel/manager/mod.rs:725-797`) ;
  - `uh:unharnessed-runtime/src/rlm/variables.py` (229 lignes) : au plus 200 noms, valeur courte, signature, forme des tableaux, `rev = hex(id(value))` ;
  - aperçu de 40 lignes ou 6 000 caractères, en 2 s au plus (`variables.py:21-33`, `:69-104`, `:188-229`).
- **Extension** : la nôtre est la sienne, renommée (§A.6). Polling de `tail` toutes les 1 000 ms pour une commande ouverte (`uh:vscode/webview/Kernel.tsx:343-350`). Listes reçues par notification.

## C. Écart aux critères de YA-18

| Critère (`docs/yusAgent-cdc.md:525-534`) | Aujourd'hui | Avec la recommandation (§G) | Comment le mesurer |
|---|---|---|---|
| Aucun commit, branche ni stash dans le dépôt de l'utilisateur | Tenu, rien n'est écrit | Tenu par construction : le dépôt shadow est hors du workspace (`--git-dir` à part) | Test : dépôt de l'utilisateur avant et après un tour et une restauration : `rev-list --all --count`, `for-each-ref`, `stash list`, `status --porcelain` identiques. Déjà fait par le script de l'annexe : `commits=1 refs=1 stash=0 status=0` avant comme après |
| Restauration d'un tour en moins de 2 s sur 10 000 fichiers | Non tenu, pas de restauration | **À mesurer.** Git seul (annexe) : 88 à 93 ms pour 50 fichiers modifiés, 20 créés et 10 supprimés | Bench dans la PR 3, sur le binaire : dépôt de 10 000 fichiers, `_yus/session/restore` chronométré côté client, 5 répétitions, enveloppe active. Cas aussi de 5 000 fichiers changés |
| Fichiers modifiés par l'utilisateur entre-temps signalés avant restauration | Non tenu | Tenu si un instantané est aussi pris **en fin de tour** : avant de restaurer, l'écart entre le disque et le dernier instantané de fin de tour donne les fichiers changés hors de l'agent (`diff-index`, 72 à 76 ms dans l'annexe) | Test rouge : fichier modifié hors de l'agent après le tour, la restauration le liste et refuse sans confirmation |
| Nettoyage des instantanés de plus de 30 jours | Non tenu | À construire : refs datées, purge au démarrage du démon puis `git gc --prune` en arrière-plan | Test : refs antidatées supprimées, refs récentes gardées ; `count-objects` en baisse |
| Instantané de toute cellule exécutée, pas seulement des éditions | Non tenu | Tenu au niveau du tour : l'instantané pris avant la première cellule couvre tout ce qu'écrivent les cellules, `bash()`, `edit()` et `apply_patch` du worker. Un `bash()` en arrière-plan qui écrit après la fin du tour est pris par l'instantané du tour suivant | Test : cellule qui écrit par `open()`, par `bash("echo > f")` et par `edit()`, puis restauration : les trois reviennent |
| Projets sans git | Non tenu | Tenu tant que le binaire `git` est présent : le dépôt shadow ne demande pas de dépôt utilisateur. Sans `.gitignore`, une liste d'exclusions par défaut et des plafonds de taille sont nécessaires (Q6). Sans binaire `git` (macOS sans outils en ligne de commande, où `/usr/bin/git` ouvre une fenêtre d'installation) : instantanés désactivés avec un message | Test dans un dossier sans `.git` ; test « git absent » avec un `PATH` sans git |

Critères de la description, hors de la liste :
- « Bouton revenir à ce tour sur chaque message, avec aperçu du diff » et « code seul, conversation seule, ou les deux » : **non couverts par l'extension reprise** (§A.6, §B.2), à construire (PR 3). Unharnessed ne propose que la conversation, au clic sur un message user.

## D. État du kernel après un retour en arrière

| Option | Ce que fait le modèle ensuite | Coût | Unharnessed |
|---|---|---|---|
| **D1** rien | Le kernel garde les variables des tours abandonnés, que l'historique ne mentionne plus. Si les fichiers sont restaurés, des données lues avant restent en mémoire | Aucun | **Oui** |
| **D2** texte fixe en fin d'historique, kernel gardé | Le modèle sait que le kernel a gardé son état et, si c'est le cas, que les fichiers ont été restaurés | Une ligne custom, envoyée par `restore_next_turn` (§A.5), préfixe intact (§E) | Non (écart : un texte en plus) |
| **D3** instantané du namespace par tour, kernel redémarré et restauré | Le kernel revient à l'état du point de retour. Ce qui ne se sérialise pas est perdu, le répertoire courant et l'environnement sont remis aux défauts, les `bash()` en arrière-plan sont tués avec le kernel | Copie de `kernel-state.dill` au début de chaque tour, jusqu'à 256 Mio chacune (clonage APFS à mesurer) ; instantané forcé avant la copie (le délai de 1,5 s peut ne pas être écoulé) ; redémarrage avec restauration (`ya_kernel_restart.rs`), durée à mesurer ; avis du type `[python-state-restored]` avec un texte propre au retour | Non |
| **D4** kernel redémarré, neuf | Tout est perdu ; le modèle est prévenu | Redémarrage ; perte des données chargées | Non |

**Commandes `bash()` en arrière-plan et avis `[bash-done]` d'un tour annulé** :
- **chez nous** : l'avis porte le `pid`, le code de sortie et la commande, sans référence de tour (`crates/pa-core/src/session_engine/messages.rs:217-235`, `bash_notices.rs:36-48`). Il arrive donc sur la nouvelle branche et réveille le modèle ou rejoint son tour suivant, comme chez Unharnessed (§B.3). Le modèle voit la commande dans l'avis et peut s'y retrouver ;
- **option K-a** (Unharnessed) : ne rien faire ;
- **option K-b** : au retour en arrière, tuer les commandes dont `startedAt` (`bash.py:1895`) est postérieur au début du premier tour abandonné, avec `kill_kernel_bash`, sans ligne upstream. Un avis de fin peut quand même partir : à vérifier, avec un test ;
- avec D3, les commandes meurent avec le kernel.

**Recommandation** : D2 avec K-a. C'est le plus simple, au plus près d'Unharnessed, et le texte fixe corrige le seul défaut relevé au ticket 24. Le texte dit aussi que des commandes lancées dans les tours abandonnés peuvent encore tourner. Proposition de texte, à valider (Q1) :

> `[conversation-rewound]` The conversation went back to an earlier point; the turns after it are gone from your context. The Python kernel kept its state: variables, imports and background commands from those turns may still exist.

S'y ajoute, quand les fichiers sont aussi restaurés (PR 3) : « Workspace files were restored to how they were at that point. »

## E. Préfixe

- **Prompt système, tools, `yaRequest`** : inchangés. Le retour en arrière ne passe ni par le prompt système, ni par la liste des tools, ni par les réglages figés. `rebuild_engine_context` reconstruit seulement les messages (`branch_navigation.rs:532-542`).
- **Historique** :
  - la requête suivante part de la branche : messages jusqu'au point de retour, puis le nouveau prompt ;
  - le préfixe commun avec les requêtes déjà envoyées reste éligible au cache du fournisseur, dans son TTL ; ce qui suivait est perdu pour le cache ;
  - ce qui sera réellement relu dépend du fournisseur (granularité et points de coupure du cache) : **à mesurer**.
- **Codex en websocket** : la continuation par `previous_response_id` ne s'applique que si l'entrée est la précédente prolongée (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:579-610`). Après un retour en arrière, l'entrée est plus courte ou différente : la requête part entière, sans `previous_response_id` (`:626-634`). Il n'y a rien à ajouter.
- **Texte fixe de D2** : ligne custom ajoutée **après** le point de retour, avant le nouveau prompt. Il ne change donc pas le préfixe commun, il s'y ajoute en fin d'historique.
- **Journal d'usage** : noter `invalidation { cause: "rewind" }` sur l'appel suivant (`note_invalidation`, `crates/pa-core/src/session_engine/usage_log.rs:491`), comme `model_change` et `resume_prefix_change` (point 9 du ticket 24, `24-ya10-extension-minimal.md:634`). Point d'accroche à trouver au code : le pont n'atteint pas le moteur. Si l'accroche impose une ligne upstream (moteur ou `branch_navigation.rs`), je m'arrête et je la signale.
- **Instantanés du workspace et namespace (PR 3, D3)** : rien dans le prompt.
- **Preuve par capture** (banc, provider factice) :
  - nouveau cas `rewind` : trois prompts, retour avant le troisième, renvoi modifié ;
  - `compare.py` : prompt système, tools et `yaRequest` identiques ;
  - nouvelle vérification : les messages de la requête après retour égalent le début de la requête du troisième prompt d'origine, jusqu'au point de retour, puis le texte de D2, puis le nouveau prompt.
  
  La part réellement servie par le cache ne se mesure qu'avec un vrai modèle, sur demande de Yusuf (règle 6).

## F. Protection des secrets

- **Où ranger les instantanés** : sous le dossier agent, dans un dossier dédié (`<dossier agent>/checkpoints/<empreinte du workspace>/` ou `session-artifacts/<id>/`, Q4). Jamais dans le workspace. Le profil refuse seulement `auth.json` et `models.json` (`crates/pa-types/src/platform/ya_guard.rs:283-286`) : un git enveloppé peut écrire ce dossier.
- **`auth.json` et `models.json` jamais dans un instantané** :
  - le worker n'est pas sous l'enveloppe : il lit les identifiants (ticket 42). Une capture **dans le processus du worker** (lecture de fichiers en Rust) sur un workspace qui contient `~/.yusagent` (dossier personnel ouvert comme projet, cas du point ouvert `apply_patch`, ticket 41 §J) **copierait `auth.json`** dans le dépôt shadow ;
  - recommandé : capturer et restaurer **par `git` lancé par l'enveloppe**. Le système refuse alors la lecture de `auth.json` et `models.json` à git ;
  - et exclure explicitement le dossier agent et le dossier du dépôt shadow (`info/exclude` du dépôt shadow), pour que `git add` ne bute pas sur un refus et ne capture rien de yusAgent.
  
  Test : un workspace qui contient un faux dossier agent (variable du dossier agent pointée dedans) : aucun blob ne contient le faux secret (`git cat-file --batch-all-objects`), et la restauration n'écrit pas `auth.json`.
- **Restauration** : également par git enveloppé. Le refus d'écriture du profil protège `auth.json` et `models.json` même si un instantané altéré les nommait.
- **Lancements ajoutés** : `git` seulement (et `lsof` pour les ports du panneau, PR 4), par `ya_guard::tokio`, comme `workspace_snapshot/git.rs:121`. Le scan `crates/pa-core/tests/ya_process_launch_scan.rs` reste vert sans nouvelle exception (`ya_secret_guard/launch_exceptions.txt` inchangé).
- **Limite à documenter** : le kernel peut lire et écrire le dépôt shadow, qui n'est pas refusé. Il pourrait altérer un instantané pour qu'une restauration ultérieure écrive un fichier de son choix dans le workspace. C'est la classe « exécution différée » (ticket 41, §J). Option : refuser au kernel l'écriture du dossier des instantanés dans le profil (`ya_guard.rs`, fichier du fork, sans ligne upstream) (Q8).
- **Variables d'environnement de git** : `GIT_DIR`, `GIT_INDEX_FILE` et voisines déjà retirées par `workspace_snapshot/git.rs:111-126`. Même précaution à reprendre, pour qu'un environnement hérité ne vise pas le dépôt de l'utilisateur.

## G. Options, recommandation, découpage, tests, contrôle

### G.1 Retour de la conversation (pont)

| Option | Ce qu'elle fait | Lignes upstream (estimation) | Écart à Unharnessed |
|---|---|---|---|
| **R-a** `navigate_tree` existant | Le pont refuse pendant un tour (texte d'Unharnessed), puis envoie `navigate_tree { targetId: to }` | `daemon.rs` : +4 (aiguillage), 1 modifiée à `:1633` (`prompt_start` lit la feuille avant le prompt) | Si `to` est un message user ou un `custom_message`, `navigate_tree` va à son **parent** (`branch_navigation.rs:546-555`). Unharnessed reste sur `to`. En temps normal, la feuille avant un prompt est la réponse de l'assistant, donc pas d'écart. **À couvrir par un test** : un tour qui finit sans réponse (abandon), et un premier prompt (y a-t-il une entrée avant le premier message ? à vérifier) |
| **R-b** nouvelle commande du worker `ya_rewind` | `branch_to(to)` et `rebuild_engine_context`, sémantique exacte d'Unharnessed, refus si occupé | R-a, plus `protocol.rs` (nom, variante, aiguillage) et `worker/commands.rs` (aiguillage) : environ +8 | Aucun |

**Recommandation : R-a**, avec les tests ci-dessus en premier. Si un test montre l'écart de `navigation_point`, je m'arrête et je propose R-b avec ses lignes exactes.

Commun aux deux :
- `rewindTo` = `leafId` de l'état du worker, lu juste avant de publier `turnStart` (un aller-retour par prompt, coût à mesurer) ; absent pour un réveil ; présent pour un steer (feuille avant le message, comme Unharnessed) ;
- au rejeu de `session/load`, `rewindTo` = parent de l'entrée du prompt, si le transcript porte les ids des entrées. À vérifier au code (`ya_history.rs:204-212` rejoue des messages, pas des entrées) ; sinon, pas de `rewindTo` au rejeu, à signaler ;
- refus d'un retour en arrière quand un autre client est attaché (TUI) : à décider (Q9).

### G.2 Instantanés du workspace (YA-18)

| Option | Pour | Contre |
|---|---|---|
| **W-a** dépôt git shadow hors du workspace, refs `refs/ya/<session>/<tour>/{debut,fin}` | Aucune écriture dans le dépôt de l'utilisateur ; marche sans dépôt utilisateur ; l'index stat rend l'instantané sans changement rapide (91 à 130 ms sur 10 000 fichiers) ; `.gitignore` respecté ; restauration et contrôle par commandes git (annexe) ; un seul lancement à envelopper | Dépend du binaire `git` ; premier instantané d'environ 3 s sur 10 000 fichiers (à lancer à l'ouverture, en arrière-plan) ; dépôts imbriqués et sous-modules à exclure ; taille (43 Mio pour 10 000 fichiers de 1 Ko, annexe) |
| **W-b** refs cachées dans le dépôt de l'utilisateur (`refs/yusagent/...`, option du CDC) | Objets partagés | Écrit dans son `.git` (objets, refs visibles par `for-each-ref`) ; contraire à l'esprit du premier critère ; ne marche pas sans git |
| **W-c** copie des fichiers modifiés (manifeste mtime et taille, copies dans un dossier) | Pas de git | Parcours complet à écrire, et à rendre rapide ; recrée ce que fait l'index git ; lecture des fichiers dans le worker, hors enveloppe (risque du §F) |
| **W-d** `workspace_snapshot` d'upstream | Code existant | Pas de restauration, pas sans git, `.env` exclus, supprimé upstream (§A.1) |

**Recommandation : W-a**, dans un module du fork (`crates/pa-daemon/src/ya_checkpoints/` ou `pa-core`, à fixer au plan de code).

**Instantanés pris** :
- au début de chaque tour, avant la première cellule : lancé au début du tour, en parallèle de l'appel au modèle ; la première cellule attend qu'il finisse ;
- à la fin de chaque tour (sert au contrôle « modifié par l'utilisateur »).

**Déclencheurs et lignes upstream** :
- début et fin de tour : le pont les voit déjà (`turnStart`, `turnEnd`). Mais le tour d'un TUI attaché ou d'un `-p` ne passe pas par le pont. Les prendre dans le pont limite donc YA-18 à l'extension, sans ligne upstream ;
- les prendre dans le worker couvre tout, avec une accroche dans `worker/turn.rs` ou `queue.rs` (upstream, quelques lignes) ;
- recommandation : le pont d'abord (Q3).

**Restauration** : `_yus/session/restore { sessionId, to, files: bool, conversation: bool, force?: bool }`, en trois étapes :
1. contrôle des fichiers changés depuis la dernière fin de tour ; s'il y en a et que `force` est absent, refus avec la liste ;
2. `read-tree -u --reset` sur l'instantané de début du tour visé, avec un index à part ;
3. retour de la conversation (G.1) si demandé, puis texte de D2.

**Aperçu** : `_yus/session/restore_preview { sessionId, to }` renvoie la liste des fichiers et leur état (`diff-tree --name-status`), et l'extension ouvre `vscode.diff` par fichier.

Nommage : à défaut de méthode chez Unharnessed, noms yusAgent en `_yus/*` (décision du 23/11/2026).

**Nettoyage** : refs de plus de 30 jours supprimées au démarrage du démon, `gc --prune` en arrière-plan. La suppression d'une conversation (C4, ticket 28) supprime ses refs.

### G.3 Panneau du kernel

Reprendre les formes d'Unharnessed (§B.5) sous `_yus/kernel/*`, le client existant ne change pas (§A.6).

**Pont (fork)** :
- `tail` et `kill` sur `tail_kernel_bash` et `kill_kernel_bash` existants ;
- `changed` à partir de `list_kernel_bash`, appelé à la fin de chaque cellule (`tool_call_update` final de `python_repl`), à chaque avis `[bash-done]`, et toutes les 20 s tant qu'une commande tourne (battement d'Unharnessed) ;
- ports par `lsof` enveloppé ;
- `killed` et le filtre « en arrière-plan » à ajouter aux lignes de `list` dans `bash.py` (fichier upstream, quelques lignes) ou à déduire dans le pont (à voir au code).

**Variables** :
- nouveau module Python du fork (`prime-agent-runtime/src/rlm/ya_variables.py`, repris de `uh:variables.py`, MIT) ;
- requêtes `variables` et `variable_preview` à côté de la file des cellules, comme `bash_activity`, ce qui touche des fichiers upstream :
  - `repl.py` (routage) ;
  - `kernel/manager/mod.rs` (requête) ;
  - le moteur et le worker (commande, aiguillage) ;
- estimation : 40 à 70 lignes upstream, à chiffrer exactement au plan de code de la PR 4, avant de coder.

**`kernel/restored`** (C7 du ticket 28) : dans la même PR, si le worker expose la restauration sans ligne upstream ; sinon reporté encore, à signaler.

### G.4 Découpage en PR (pont puis extension, comme R5 et X5)

1. **PR 1, pont, retour de la conversation** :
   - `rewindTo` dans `turnStart` (prompt, steer, rejeu si possible) ;
   - `_yus/session/rewind`, refus pendant un tour, D2 par `restore_next_turn`, `invalidation: rewind` ;
   - cas `rewind` au banc de capture ;
   - upstream : environ +4 et 1 modifiée (R-a).
2. **PR 2, extension** : `_yus/session/rewind` dans les méthodes servies (`methods.ts`), test du clic et du renvoi contre l'agent factice, e2e vrai binaire. Peut être fusionnée à la PR 1 si Yusuf préfère (la partie extension est petite).
3. **PR 3, instantanés du workspace et restauration (YA-18)** :
   - pont et module du fork ;
   - extension : bouton « revenir à ce tour » sur chaque message, aperçu, choix fichiers / conversation / les deux, liste des fichiers modifiés hors de l'agent ;
   - bench des 2 s sur le binaire.
   
   Peut se couper en 3a (pont) et 3b (extension).
4. **PR 4, panneau du kernel** : pont et runtime, puis extension (feature `kernel`). C'est la PR qui touche le plus d'upstream.

### G.5 Tests (rouges sur leur assertion avant le code)

PR 1 :
1. `turnStart` d'un prompt porte `rewindTo` = feuille avant le prompt. Aujourd'hui absent : rouge sur l'égalité.
2. `_yus/session/rewind` puis prompt : la requête au fournisseur factice contient les messages jusqu'au point de retour, puis le texte de D2, puis le nouveau prompt, et rien des tours abandonnés. Aujourd'hui `-32601` : rouge sur la réponse.
3. Refus pendant un tour : `-32603` avec le texte d'Unharnessed, et le tour n'est pas interrompu (`navigate_tree` l'interromprait).
4. `to` hors de la branche : `-32602` avec `data.details`.
5. Cas limites de R-a : tour abandonné sans réponse ; premier prompt.
6. Journal d'usage : `invalidation.cause == "rewind"` sur l'appel suivant, et sur lui seul.
7. Capture : prompt système, tools et `yaRequest` identiques avant et après (`compare.py`).
8. Reprise après retour en arrière et prompt : la branche reprise est la nouvelle.

PR 3 :

9. Dépôt de l'utilisateur intact (critère 1).
10. Restauration des écritures par `open()`, `bash()` et `edit()`.
11. Fichier modifié hors de l'agent : refus avec liste, puis `force`.
12. Projet sans `.git`.
13. `git` absent : instantanés désactivés, retour de la conversation toujours possible.
14. Faux dossier agent dans le workspace : aucun secret dans les objets.
15. Purge des refs de plus de 30 jours.
16. Bench des 2 s sur 10 000 fichiers.

PR 4 :

17. `changed` après une commande `h = bash("python -m http.server 0")` : ligne `running` avec un port.
18. `kill` puis ligne `finished` avec `killed: true`.
19. Variables après une cellule, `preview` refusé avec `busy` pendant une cellule.
20. Scan des lancements vert.

### G.6 Contrôle manuel prévu pour Yusuf

**PR 1 et 2**, dans l'extension, sur un build de la branche :
1. trois messages, dont un qui crée une variable `x = 1` au deuxième ;
2. clic sur le deuxième message, modification, renvoi ;
3. vérifier que le fil repart de là, que le modèle ne cite plus la réponse abandonnée, et qu'il sait que `x` peut encore exister (texte de D2) ;
4. Stop pendant un tour : le clic n'est pas possible ;
5. « Relancer » : l'historique repris est le nouveau.

**PR 3** :
1. une demande qui modifie trois fichiers et en crée un ;
2. modifier un autre fichier à la main ;
3. « revenir à ce tour » : la liste montre le fichier modifié à la main, l'aperçu montre les quatre fichiers ;
4. restaurer avec confirmation ;
5. `git status` du dépôt : seuls restent les changements de l'utilisateur, aucun commit, aucune branche, aucun stash.

**PR 4** : lancer un serveur depuis le kernel, le voir dans « Running servers » avec son port, l'arrêter ; ouvrir une variable.

Les scripts de contrôle seront d'abord lancés par Claude Code sur le binaire (règle 13).

### G.7 Questions

1. **Kernel après un retour en arrière** : D2 (kernel gardé, texte fixe, recommandé), D1 (comme Unharnessed, sans texte) ou D3 (instantané du namespace par tour, kernel redémarré) ? Texte de D2 à valider (§D).
2. **Commandes en arrière-plan des tours abandonnés** : K-a (laisser, comme Unharnessed, recommandé) ou K-b (tuer celles lancées après le point de retour) ?
3. **Portée et ordre** : PR 1 et 2 (conversation, comme Unharnessed) d'abord, puis PR 3 (YA-18), puis PR 4 ? **Écart au CDC à valider** : tant que la PR 3 n'est pas faite, le retour en arrière ne restaure pas les fichiers, contrairement à la description de YA-18. Pour la PR 3, instantanés pris dans le pont (extension seulement, sans upstream) ou dans le worker (tous les clients, quelques lignes upstream) ?
4. **Emplacement du dépôt shadow** : un par workspace (`<dossier agent>/checkpoints/<empreinte>/`, premier instantané payé une fois par projet) ou un par conversation (`session-artifacts/<id>/`, supprimé avec elle, premier instantané d'environ 3 s par conversation sur 10 000 fichiers) ?
5. **Sans binaire `git`** : instantanés désactivés avec un message (recommandé), ou repli W-c ?
6. **Exclusions** : `.gitignore` de l'utilisateur respecté, plus une liste par défaut quand il n'y en a pas (`node_modules`, `target`, `.venv`, `dist`, `build`…) et des plafonds (taille par fichier, nombre de fichiers) ? Valeurs à fixer.
7. **Panneau du kernel** : accepter environ 40 à 70 lignes upstream pour les variables (runtime et manager), chiffrées exactement avant la PR 4 ?
8. **Dépôt shadow modifiable par le kernel** : limite documentée (classe « exécution différée »), ou refus d'écriture pour le kernel dans le profil (`ya_guard.rs`, fichier du fork) ?
9. **Autre client attaché** (TUI) pendant un retour en arrière : refuser, ou laisser faire comme pour un prompt ?
10. **Feuille non inscrite avant le prompt suivant** (§A.2) : accepter comme Unharnessed, ou inscrire la feuille tout de suite (entrée ajoutée, qui changerait la forme du fichier de session) ?
11. **`workspace_snapshot` mort chez nous** : le laisser (règle 2 : pas de changement upstream sans raison), ou reprendre `20b4ad74b` dans un ticket à part ? La ligne du ticket 41 (`git.rs:121`) disparaîtrait avec lui (`docs/fork-changes.md` à mettre à jour).

## H. Réponses de Yusuf (27/11/2026)

Reportées au journal du CDC (ligne du 27/11/2026, ticket 43).

1. **D2**, texte validé tel quel (§D).
2. **K-a** : les commandes `bash()` des tours abandonnés continuent.
3. **Découpage** :
   - PR 1 et 2 fusionnées en une seule PR (pont et extension), puis PR 3, puis PR 4 ;
   - **écart au CDC accepté** : pas de restauration des fichiers avant la PR 3 ;
   - pour la PR 3, instantanés pris dans le pont.
4. **Un dépôt shadow par workspace**, avec un index par session.
5. **Sans git** : instantanés désactivés, avec un message.
6. **Exclusions** :
   - `.gitignore` respecté, liste par défaut quand il n'y en a pas, plafonds ;
   - valeurs à proposer au plan de code de la PR 3, mesurées sur `~/Desktop/yusagent` ;
   - dépôts imbriqués : limite à documenter.
7. **Pas de validation d'avance** pour le panneau du kernel : lignes upstream exactes au plan de code de la PR 4. Si c'est trop, la PR 4 se coupe (commandes et ports, puis variables).
8. **Refus d'écriture du dossier des instantanés pour le kernel**, et git des instantanés lancé sous une variante du profil. Si cela coûte plus de quelques lignes, je m'arrête et je chiffre.
9. **Retour en arrière refusé** quand un autre client est attaché.
10. **Feuille non inscrite avant le prompt suivant** : acceptée pour la PR 1, à rouvrir à la PR 3.
11. **`workspace_snapshot` laissé tel quel.**

Ajouts :

- **Test supplémentaire au point 5 du §G.5** : feuille avant le prompt = ligne custom (annonce `[skills-changed]` du ticket 31, texte D2 d'un retour précédent). `navigation_point` remonte au parent (`branch_navigation.rs:550-552`). Si le test est rouge : arrêt, et R-b proposé avec ses lignes exactes. **Résultat au §I.1 : rouge par lecture du code**, R-b proposé au §I.2.
- **Mesure de l'aller-retour de `rewindTo`** par prompt, règle du ticket 42 : surcoût médian < 10 ms (§I.6).
- Renvoi « Questions au §G.6 » corrigé en §G.7 (Résumé).

## I. Plan de code de la PR 1 (pont et extension), sans code

Branche de code : `ya-43-retour-arriere-pr1`, depuis `main`, avec les commits de doc de cette branche. Lignes citées : `main` à `f7c4b3fb7`.

### I.1 Test ajouté (point 5) : rouge par lecture du code, R-a abandonnée

Le test n'est pas lancé : aucun code n'est écrit à ce stade. Le résultat se lit dans le code.

- **Les deux cas cités ne mettent pas la feuille sur une ligne custom** :
  - les lignes rangées pour le tour suivant (`pending_next_turn`, où va le texte D2) et l'annonce `[skills-changed]` (`ya_resource_notice`) partent en « prefix rows » du tour du prompt lui-même, avant la ligne acceptée (`crates/pa-daemon/src/worker/turn.rs:1036-1047`) ;
  - le pont lit la feuille avant d'envoyer `PromptAndWait` (`crates/pa-daemon/src/acp/daemon.rs:1633`, puis `:1637`) ;
  - ces lignes sont donc **après** `rewindTo`. Elles partent avec le tour abandonné, comme chez Unharnessed.
- **D'autres lignes `custom_message` s'écrivent hors d'un tour** :
  - `append_custom_message` (`crates/pa-daemon/src/session_custom.rs:17-31`) ;
  - `/refine` (`session_custom.rs:286`, `:296`) ;
  - l'auto-refine en fin de tour (`crates/pa-daemon/src/worker/lifecycle.rs:591`, `:602`).
  
  Après l'une d'elles, la feuille avant le prompt suivant est une `custom_message`. `navigate_tree` irait alors à son parent (`branch_navigation.rs:550-552`) et la retirerait de la branche, alors qu'Unharnessed reste sur `to` (`uh:crates/uh-core/src/session_engine/compaction_arms.rs:442`). **Le test serait rouge sous R-a.**
- Deuxième écart de R-a : `navigate_tree` **interrompt** un tour en cours (`branch_navigation.rs:178`, `:497-510`) au lieu de refuser. Un refus dans le pont laisse une course avec un tour de réveil qui démarre entre le refus et la commande.
- **Défaut commun à R-a et R-b** : l'annonce `[skills-changed]` retirée par un retour en arrière ne serait pas refaite.
  - Ce que le modèle sait des skills et des serveurs est tenu en mémoire (`Tracker.known`, `crates/pa-core/src/session_engine/ya_resource_notice.rs:87-97`), pas relu dans l'historique.
  - Après le retour, le moteur croit le modèle informé ; le modèle ne l'est plus.
  - Correction dans le plan : remettre l'annonce en file (§I.3, étape 5).

### I.2 R-b : lignes exactes

| Option | Worker | Lignes upstream |
|---|---|---|
| **R-b0** (recommandée) | Deux actions, `rewind` et `leaf`, de la commande du fork `ya_permissions`, comme `holds` au ticket 29 (`crates/pa-daemon/src/ya_permissions.rs:343-347`). Code dans un sous-module du fork, `crates/pa-daemon/src/ya_permissions/rewind.rs`, déclaré dans `ya_permissions.rs`. La variante `YaPermissions { action, rest }` existe déjà (`crates/pa-types/src/daemon/command.rs:671-678`) | **Worker : 0.** Pont : `daemon.rs` +5 −1 (ci-dessous). **Total : +5 −1** |
| R-b1 | Commande dédiée `ya_rewind` | R-b0, plus : `crates/pa-types/src/daemon/command.rs` +8 (variante), `crates/pa-daemon/src/protocol.rs` +5 (`KNOWN_COMMAND_TYPES`, routage `command_active_session_id` sur 3 lignes, `command_type_name`, comme `ya_permissions`, `docs/fork-changes.md:1142`), `crates/pa-daemon/src/worker/commands.rs` +1 (aiguillage). **Total : environ +19 −1** |

Le nom `ya_permissions` est trompeur pour un retour en arrière : c'est le prix des 0 ligne, et le ticket 29 a fait le même choix pour `holds`. Le doc-commentaire de la variante (`command.rs:668-670`, lignes du fork dans un fichier upstream) n'est pas touché : les actions sont décrites dans `ya_permissions.rs`, comme `holds` et `others`.

**Lignes upstream de R-b0, toutes dans `crates/pa-daemon/src/acp/daemon.rs`** :

1. Aiguillage de `_yus/session/rewind`, **+4**, inséré après le bras de `steer` (`:1171-1174`, bras à `:1172`), même forme :
   ```rust
           // yusAgent (YA-18, ticket 43): going back to before a prompt.
           super::ya_turns::rewind::METHOD => {
               super::ya_turns::rewind::serve(&id, &params, link, state, &tx).await;
           }
   ```
2. `turnStart` d'un prompt avec `rewindTo`, **−1 +1** à `:1633` :
   ```rust
   -    super::ya_turns::publish_turn_start(&producer, turn_id, super::ya_turns::prompt_start()).await;
   +    super::ya_turns::publish_prompt_start(&producer, turn_id, link, state).await;
   ```
   La ligne fait 82 caractères : rustfmt la garde sur une ligne (largeur 100 par défaut, pas de `rustfmt.toml` dans le dépôt). `publish_prompt_start` (fork, `ya_turns.rs`) lit l'identifiant de session du démon dans `state` et la feuille par l'action `leaf`, puis appelle `publish_turn_start`.

Aucune autre ligne upstream : `ya_turns.rs`, `ya_history.rs`, `ya_permissions.rs`, `usage_log.rs`, l'extension et les outils du banc sont des fichiers du fork.

### I.3 Comportement

**Pont, `_yus/session/rewind { sessionId, to }`** (nouveau `crates/pa-daemon/src/acp/ya_turns/rewind.rs`, déclaré dans `ya_turns.rs` comme `steer`) :
1. Session inconnue : `-32603`, « Unknown ACP session: {id} » (texte d'Unharnessed).
2. Un tour en cours (prompt de ce client, tour de l'agent ou steer) : `-32603`, « The agent is working: stop it before going back. » (texte d'Unharnessed).
3. **Réponse 9**, autre client attaché qui répond (même vérification que C12, `ya_history::other_askers`, `ya_history.rs:170-187`, passée en `pub(super)`) : `-32603`, « This conversation is open in another window: close it there before going back. » (proposition, sur le modèle de C12, `ya_history.rs:149`). Démon trop ancien : refus de C12 (`ya_history.rs:162`).
4. Action `rewind` du worker. Réponse `{}`, ou erreur reportée (`-32602` avec `data.details` pour une entrée hors de la branche, `-32603` sinon).
5. Après le succès : le goal est republié, comme Unharnessed (`rewind.rs:64`), par `fetch_autonomous_status` (déjà `pub(super)`). Rien pour l'usage : l'indicateur de contexte est reporté (ligne du 21/11/2026).

**Worker, action `rewind { to }`** (sous-module `ya_permissions/rewind.rs`) :
1. Sous le verrou du cœur :
   - refus « The agent is working: stop it before going back. » si `core.busy` ou `core.compacting` ;
   - refus « {to} is not in the conversation » si `to` n'est pas sur la branche active (`store.branch_file_entries()`, `session_tree.rs:210`).
2. Toujours sous le verrou : `core.queued_input_suspended` est mis à vrai (`worker/session_core.rs:90-94`), en gardant l'ancienne valeur. Aucun tour (réveil, file) ne démarre pendant le déplacement. C'est l'équivalent du verrou d'Unharnessed (`rewind.rs:34-36`).
3. `store.branch_to(Some(to))` (`session_tree.rs:140`), en mémoire comme Unharnessed (réponse 10) ; entrées de la branche relevées.
4. Entrées abandonnées : celles de l'ancienne branche après `to`.
5. **Remise en file** dans `core.pending_next_turn`, dans l'ordre :
   - les lignes abandonnées de type `ya-resources-changed` (`ya_resource_notice.rs:36`), pour la cohérence de `Tracker.known` (§I.1) ;
   - les lignes D2 déjà en file sont retirées (deux retours de suite : un seul texte) ;
   - puis le texte D2.
6. Verrou rendu. `engine.rebuild_session_context(entrées, GoalBranchReload::FaithfulBranch)` sur un fil bloquant (`engine.rs:170`, comme `branch_navigation.rs:532-542`).
7. `queued_input_suspended` remis à son ancienne valeur. Le runner est réveillé si la file n'est pas vide (mécanisme exact à établir au code ; test T11).
8. `usage_log::lookup(session_id)` (`usage_log.rs:728-732`), puis `note_invalidation({"cause": "rewind"})`.
   - `note_invalidation` remplace aujourd'hui la note en attente (`:491-493`) : il fusionnera, pour que la note du masquage ne l'efface pas (fichier du fork).
   - **À vérifier au code** : le journal du worker est-il enregistré dans le registre (`for_session`, `:451-470`) ? S'il ne l'est pas, je m'arrête et je propose une autre voie.

**Écart au §D et au §A.5** : le texte D2 ne passe plus par la commande `restore_next_turn`. L'action `rewind` le range elle-même dans `core.pending_next_turn` (étape 5), dans le même passage sous le verrou que le déplacement : pas de seconde requête, pas de fenêtre entre les deux. C'est la même file : `restore_next_turn` y écrit aussi (`session_custom.rs:59`).

**Ligne D2** : `{ role: "custom", customType: "ya-conversation-rewound", content: <texte validé>, display: true, details: null }`, même forme que l'annonce du ticket 31 (`ya_resource_notice.rs:435`).
- Le pont ne montre pas les lignes custom (seules les annonces passent par `ya_notice.rs:18-27`) ; au rejeu, une ligne custom suivie d'un prompt n'ouvre pas de tour de réveil (`ya_history.rs:282`).
- À prouver par l'e2e : rien d'affiché, pas de tour en plus.

**Worker, action `leaf`** : `{ leafId }` sous le verrou du cœur. C'est plus léger que `get_state` (résumé complet, `worker/summary.rs:119`).

**`rewindTo`** :
- prompt de ce client : feuille lue par `leaf` juste avant `turnStart`. En cas d'échec de la lecture, `rewindTo` est absent et le tour part quand même ;
- tour de l'agent : absent, comme Unharnessed (`uh:.../acp/prompt.rs:328`) ;
- steer : absent, D3 du ticket 39 inchangé (écart déjà validé : Unharnessed le met) ;
- rejeu de `session/load` : **reporté dans un ticket à part** (Q-I4, §I.9) ; pas de `rewindTo` au rejeu dans la PR 1.

**Extension** :
- `"_yus/session/rewind"` ajouté aux méthodes servies (`src/ya/methods.ts:10-41`), d'où `features().rewind` vrai (`:57`) ;
- le test du « méthode non servie » prend `_yus/kernel/tail` à la place de `_yus/session/rewind` (`test/unit/agent.test.ts:85-87`) ;
- agent factice : sert `_yus/session/rewind` et envoie `rewindTo` (`test/fake-agent/fake-agent.mjs:113-119`) ;
- aucun autre changement : le clic, l'appel et le renvoi existent (`src/chat.ts:600-612`, `:657-670`).

### I.4 Fichiers

| Fichier | Statut | Changement |
|---|---|---|
| `crates/pa-daemon/src/acp/daemon.rs` | upstream | +5 −1 (§I.2) |
| `crates/pa-daemon/src/acp/ya_turns.rs` | fork | `mod rewind;`, `publish_prompt_start`, `rewindTo` |
| `crates/pa-daemon/src/acp/ya_turns/rewind.rs` | nouveau | méthode `_yus/session/rewind` |
| `crates/pa-daemon/src/acp/ya_history.rs` | fork | `other_askers` en `pub(super)` |
| `crates/pa-daemon/src/ya_permissions.rs` | fork | `mod rewind;`, aiguillage des actions `rewind` et `leaf` |
| `crates/pa-daemon/src/ya_permissions/rewind.rs` | nouveau | actions `rewind` et `leaf` |
| `crates/pa-core/src/session_engine/usage_log.rs` | fork | `CAUSE_REWIND`, fusion dans `note_invalidation` |
| `extensions/vscode/src/ya/methods.ts`, `test/unit/agent.test.ts`, `test/fake-agent/fake-agent.mjs`, `test/e2e/suite-real.ts` | fork | §I.3 |
| `bench/tools/capture/ya43_rewind_check.py`, `ya43_rewindto_cost.py` | nouveaux | §I.5, §I.6 |
| `docs/fork-changes.md` | doc | une ligne par fichier, la ligne upstream de `daemon.rs` avec ses numéros |

### I.5 Tests (rouges sur leur assertion avant le code)

Rust (`pa-daemon`, lien simulé du pont et worker de test) :
- **T1** `turnStart` d'un prompt : `rewindTo` = feuille avant le prompt. Rouge : absent.
- **T2** action `rewind` : feuille sur `to`, contexte reconstruit sans les tours abandonnés. Rouge : action inconnue.
- **T3** refus pendant un tour (pont et worker) : texte exact, tour non interrompu. Rouge : `-32601`.
- **T4** `to` hors de la branche : `-32602`, `data.details` « … is not in the conversation ». Rouge.
- **T5** (ajout de Yusuf), trois volets :
  - (a) feuille avant le prompt = `custom_message` écrite hors tour (`append_custom_message`, comme `/refine`) : après le retour, la feuille est cette ligne, elle reste dans la requête suivante. Sous R-a, le parent ;
  - (b) `[skills-changed]` en ligne d'en-tête du tour abandonné : retirée avec le tour, puis remise en file, et présente une fois dans la requête suivante ;
  - (c) D2 d'un retour précédent dans le tour abandonné, puis deux retours de suite : un seul D2 dans la requête suivante.
  
  Rouge : action inconnue.
- **T6** (réponse 9) autre client attaché : refus avec le texte. Rouge.
- **T7** requête au fournisseur factice après un retour et un prompt : messages = branche jusqu'à `to`, puis D2, puis le nouveau prompt ; rien des tours abandonnés ; prompt système et tools identiques à la requête d'avant. Rouge.
- **T8** journal d'usage : `invalidation.cause == "rewind"` sur l'appel `main` suivant, et sur lui seul ; avec le masquage actif, les deux causes sont présentes. Rouge.
- **T9** reprise (redémarrage du démon) après un retour et un prompt : branche reprise = nouvelle. Contrôle, pas forcément rouge.
- ~~T10~~ `rewindTo` au rejeu : retiré de la PR 1 (Q-I4, ticket à part).
- **T11** avis `[bash-done]` arrivé pendant le retour : livré après, pas perdu (K-a). Contrôle de course.

Extension :
- **E1** `features().rewind` vrai ; le cas « méthode non servie » passe sur `_yus/kernel/tail`.
- **E2** (agent factice) modifier le deuxième message puis le renvoyer : `_yus/session/rewind { sessionId, to }`, puis `session/prompt`, dans cet ordre, avec le `rewindTo` du tour. Rouge : la méthode est refusée sur place.
- **E3** (e2e, vrai binaire, serveur scripté) trois prompts, modification du deuxième : la troisième requête reçue par le serveur suit T7 ; aucune ligne affichée pour D2.

### I.6 Mesures et capture

- **Aller-retour de `rewindTo`** (règle du ticket 42, surcoût médian < 10 ms) :
  - `bench/tools/capture/ya43_rewindto_cost.py`, sur le modèle de `ya42_cost.py` : blocs alternés `main` et branche, session ACP neuve par bloc, scénario `smoke-warmup` ;
  - métrique : délai côté client entre l'envoi de `session/prompt` et la réception du `turnStart` du tour. L'écart des médianes isole la lecture de `leaf` : sur `main`, `turnStart` part sans aller-retour ;
  - médiane et p95 par binaire ; commande et sortie au ticket ;
  - au-delà de 10 ms : je m'arrête et je propose de lire la feuille dans le résumé déjà renvoyé au pont, ou plus tôt.
- **Préfixe** :
  - `bench/tools/capture/ya43_rewind_check.py` (cas `rewind` : trois prompts, retour avant le troisième, renvoi modifié) ;
  - plus `compare.py --all` contre `main` sur les scénarios existants : `TOUT IDENTIQUE` attendu, la PR ne change rien sans retour en arrière ;
  - et `compare_frames.py --against <main> --expect` : seules les trames `turnStart` (champ `rewindTo`) diffèrent.
- Toujours `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`.

### I.7 Contrôle manuel pour Yusuf

Celui du §G.6 (PR 1 et 2), d'abord lancé par Claude Code sur le binaire de la branche (règle 13), plus deux cas :
1. `/refine` puis modification du message suivant : la ligne de refine reste.
2. Conversation ouverte dans le TUI en même temps : refus avec le texte du point 3 du §I.3.

### I.8 Questions sur ce plan

- **Q-I1** : R-b0 (actions sur la commande du fork `ya_permissions`, 0 ligne upstream côté worker, précédent `holds`) ou R-b1 (commande dédiée, environ +19 −1 au total) ? Recommandé : R-b0.
- **Q-I2** : remettre aussi en file `ipython_state_restored` quand il est retiré par un retour en arrière, ou seulement les annonces `ya-resources-changed` (nécessaires à la cohérence du `Tracker`) ?
- **Q-I3** : texte du refus de la réponse 9 : « This conversation is open in another window: close it there before going back. »
- **Q-I4** : `rewindTo` au rejeu de `session/load` (un `get_session_tree` par chargement) dans cette PR, ou reporté ? Sans lui, après « Relancer », seuls les nouveaux messages sont modifiables.
- **Q-I5** : suspension de la file pendant le retour (`queued_input_suspended`, étape 2), ou course acceptée comme `navigate_tree` d'upstream ?

### I.9 Réponses de Yusuf sur le plan (27/11/2026)

- **Q-I1 : R-b0.** Un commentaire dans `ya_permissions.rs` dira que cette commande du fork porte aussi des actions hors permissions (`holds`, `rewind`, `leaf`).
- **Q-I2 : seulement les lignes `ya-resources-changed`.** `ipython_state_restored` n'est pas remis en file (§I.3, étape 5, corrigée en conséquence).
- **Q-I3 : texte validé** : « This conversation is open in another window: close it there before going back. »
- **Q-I4 : reporté dans un ticket à part** (appariement fragile avec les steers). T10 est retiré de la PR 1.
  - **Limite de la PR 1** : après « Relancer » ou une reprise depuis l'historique, seuls les nouveaux messages sont modifiables. Les tours rejoués n'ont pas de `rewindTo`, et l'extension ne rend pas leur message cliquable (`webview/Chat.tsx:184`).
- **Q-I5 : suspension de la file pendant le retour.** Le mécanisme de réveil de l'étape 7 est établi au code (ci-dessous) : pas d'arrêt.

**Étape 7, réveil de la file (établi au code)** :
- `resume_queued_input()` (`crates/pa-daemon/src/worker/lifecycle.rs:426-437`) lève `queued_input_suspended` puis appelle `wake_queued_input()` (`:439-445`) ;
- `wake_queued_input()` fait `work_notify.notify_one()` (réveil du runner) et `retry_owed_goal_continuation()` (suite de goal due). C'est le chemin de `resume_queue` (`:368-423`) ;
- l'action `rewind` :
  - si la suspension était levée avant le retour, appelle `resume_queued_input()` à la fin ;
  - si elle était déjà posée (après un abort), la laisse posée et ne réveille rien ;
- aucun point de contrôle n'est écrit au journal de reprise (`record_queue_checkpoint`, `:387`) : la suspension du retour ne vit qu'en mémoire. Un crash pendant le retour reprend l'état durable d'avant, non suspendu ;
- **limite** : un abort reçu pendant la fenêtre du retour (session au repos) verrait sa suspension levée par la fin du retour. Le pont n'envoie pas de Stop au repos, et un autre client attaché fait refuser le retour (réponse 9).

**Étape 8, journal d'usage (établi au code)** : le registre est indexé par l'id d'en-tête du fichier (`for_session`, `usage_log.rs:451-475`, `register` `:722-726`). Le moteur du worker reprend cet id (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1074-1086`), le même que `store.session_id()`. `usage_log::lookup(store.session_id())` trouve donc le journal de la session.

**Fusion dans `note_invalidation` : forme exacte**

- **Forme du champ `invalidation` inchangée**, celle documentée depuis le ticket 24 (`docs/usage.md:108`, `merge_invalidations`, `usage_log.rs:382-390`) :
  - absent sans cause ;
  - **l'objet seul** pour une cause : `{"cause": "rewind"}` ;
  - **un tableau de ces objets** pour plusieurs causes sur le même appel.
- Ordre du tableau : changements de modèle ou d'effort (`changes_since`), puis les notes dans l'ordre où elles ont été posées. Par exemple, un retour en arrière puis un nouveau palier de masquage au même appel :

  ```json
  [{"cause": "rewind"}, {"cause": "masking", "turn": 9, "frontier": 4, "masked": 3}]
  ```
- **Ce qui change dans le code** (fichier du fork) :
  - la note en attente devient une liste (`Mutex<Vec<Value>>`) ;
  - `note_invalidation` **remplace une note de même `cause`** et ajoute les autres. Une seconde note de masquage avant l'appel remplace donc la première, comme aujourd'hui ; deux retours de suite donnent un seul `rewind` ;
  - `take_invalidation()` garde sa signature (`Option<Value>`, l'objet seul ou le tableau), utilisée par les tests du masquage ;
  - `take_main_invalidation` ajoute les notes une par une à ses entrées, pour ne pas imbriquer de tableau ;
  - nouvelle constante `CAUSE_REWIND = "rewind"`.

**Lecteurs de `invalidation`** (recherche de `invalidation` dans `bench/`, `crates/`, `extensions/`, `scripts/`) et effet :

| Lecteur | Ce qu'il lit | Effet |
|---|---|---|
| `bench/smoke/metrics.py:109`, `:130`, `:147` | présence du champ (`"invalidation" in r`) | Aucun changement : présence seulement. Un appel après un retour en arrière sort des taux de cache, ce qui est voulu ; le banc (`-p`) n'en fait jamais |
| `bench/smoke/tests/test_bench.py:69`, `:82` | objet `masking` construit par le test | Aucun |
| `bench/smoke/tools/ab_stats.py:339` | `(record.get("invalidation") or {}).get("cause")` | **Aucun changement de comportement** : le banc ne fait pas de retour en arrière, et une seule note de masquage reste un objet. **Défaut existant, hors de ce ticket** : un tableau (déjà possible depuis le ticket 24, `model_change` et `thinking_level_change` au même appel) ferait échouer `.get` sur une liste. À corriger dans un ticket à part : lire l'objet ou chaque élément du tableau, comme `acp_resume.py:220-221` |
| `bench/smoke/tools/check_warmup.py:132-133`, `bench/smoke/ab/ya14-codex-rereport-check.py:29` | compteurs du rapport (`report.json`) | Aucun |
| `bench/tools/capture/acp_live_changes.py:183-187`, `acp_resume.py:220-221`, `:393-394` | objet ou tableau, chaque `cause` | Aucun : les deux formes sont déjà lues ; leurs scénarios ne font pas de retour en arrière |
| `bench/tools/capture/masking_check.py:81`, `:99-101` | présence, valeur affichée | Aucun : cas `mask-*` sans retour en arrière |
| `crates/pa-cli/tests/ya_acp_resume_e2e.rs:565-571` | objet `thinking_level_change` exact | Aucun : une seule cause |
| `crates/pa-core/src/session_engine/usage_log/tests.rs:588-596` (une note), `:675-689` (trois causes, un tableau) | formes exactes | Aucun : une note reste un objet, et l'ordre `changes_since` puis notes est gardé |
| `crates/pa-core/src/session_engine/masking/tests.rs:377-383` | `take_invalidation()` = l'objet `masking` | Aucun : une note, même signature |
| Tableau de bord | — | Il n'y en a pas : l'indicateur de cache de l'extension est reporté (ligne du 21/11/2026) ; l'extension ne lit pas le journal d'usage |
| `compare.py` | — | Ne lit pas `invalidation` (absent de la recherche) |

**Aucun lecteur existant ne change de comportement** : pas d'arrêt. `docs/usage.md:108` gagne la cause `rewind`, et le défaut d'`ab_stats.py` part dans un ticket à part.

## Annexe : mesure préliminaire, git seul

Script : `docs/tickets/43-retour-arriere/shadow_git_mesure.sh`.
- Il crée un workspace de 10 000 fichiers d'environ 1 Ko dans 100 dossiers, avec un dépôt git de l'utilisateur et un commit.
- Il crée un dépôt shadow nu à côté (`--git-dir` à part, `--work-tree` = workspace).
- Puis, à chaque répétition :
  - instantané sans changement ;
  - « tour de l'agent » (50 fichiers modifiés, 20 créés, 10 supprimés) ;
  - instantané après le tour ;
  - contrôle des changements depuis cet instantané (`diff-index`, 0 attendu, mesure le coût du contrôle) ;
  - restauration de l'instantané d'avant le tour (`read-tree -u --reset`) et vérification qu'il ne reste aucun écart.
- Il compare enfin l'état du dépôt de l'utilisateur avant et après.

Il n'utilise ni yusAgent ni l'enveloppe (`sandbox-exec` non mesuré), et n'appelle aucun modèle. **Ce sont des chiffres de git seul, sur un cache disque chaud et des petits fichiers synthétiques : une estimation pour le choix de l'option, pas la mesure du critère.**

Commande et sortie du lancement par Claude Code (27/11/2026, macOS 27.0.1, Apple M5, git 2.55.0) :

```
$ D=$(mktemp -d) && bash docs/tickets/43-retour-arriere/shadow_git_mesure.sh "$D" 10000 5
instantané initial (froid) : 3048 ms
rép. 1 : instantané sans changement 91 ms ; instantané après le tour 157 ms ; contrôle des changements 76 ms (0 fichiers) ; restauration 93 ms (écarts restants 0, non suivis 0)
rép. 2 : instantané sans changement 130 ms ; instantané après le tour 155 ms ; contrôle des changements 74 ms (0 fichiers) ; restauration 91 ms (écarts restants 0, non suivis 0)
rép. 3 : instantané sans changement 124 ms ; instantané après le tour 156 ms ; contrôle des changements 73 ms (0 fichiers) ; restauration 89 ms (écarts restants 0, non suivis 0)
rép. 4 : instantané sans changement 124 ms ; instantané après le tour 156 ms ; contrôle des changements 73 ms (0 fichiers) ; restauration 88 ms (écarts restants 0, non suivis 0)
rép. 5 : instantané sans changement 126 ms ; instantané après le tour 153 ms ; contrôle des changements 72 ms (0 fichiers) ; restauration 89 ms (écarts restants 0, non suivis 0)
dépôt de l'utilisateur avant : commits=1 refs=1 stash=0 status=0
dépôt de l'utilisateur après : commits=1 refs=1 stash=0 status=0
taille du dépôt shadow : 43584 Ko ; git 2.55.0
```

Un premier lancement, identique, donnait le même ordre de grandeur : 3 104 ms à froid, restauration de 89 à 93 ms.
