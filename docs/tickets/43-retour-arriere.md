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
- **PR 3 (instantanés du workspace)** : constat, mesures sur `~/Desktop/yusagent` et plan de code au §P, questions au §P.10.

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
5. ~~Après le succès : le goal est republié, comme Unharnessed (`rewind.rs:64`), par `fetch_autonomous_status` (déjà `pub(super)`).~~ **Corrigé au push 9 (§N)** : le goal est republié **par le worker**, comme `handle_navigate_tree` (`crates/pa-daemon/src/worker/commands.rs:199-215`) : après la reconstruction, `goal_update_after_rebuild()` donne le goal rechargé depuis la branche, publié en événement `goal_update` avant la réponse au retour, puis en `_meta.goal` par le pont (`acp/wire_events.rs:260`). `fetch_autonomous_status` ne convenait pas : il attend la fin d'un travail sans tête. Rien pour l'usage : l'indicateur de contexte est reporté (ligne du 21/11/2026).

**Worker, action `rewind { to }`** (sous-module `ya_permissions/rewind.rs`) :
1. Sous le verrou du cœur :
   - refus « The agent is working: stop it before going back. » si `core.busy` ou `core.compacting` ;
   - refus « {to} is not in the conversation » si `to` n'est pas sur la branche active (`store.branch_file_entries()`, `session_tree.rs:210`).
2. ~~Toujours sous le verrou : `core.queued_input_suspended` est mis à vrai (`worker/session_core.rs:90-94`), en gardant l'ancienne valeur.~~ **Corrigé au push 9 (§N)** : l'action tient la pause de file d'upstream du début à la réponse, `hold_queued_work_pause("navigate")` (`crates/pa-daemon/src/session_input_pause.rs:131-144`), comme `handle_navigate_tree`. Aucun tour (réveil, file) ne démarre pendant le déplacement (`worker/turn.rs:59`). C'est l'équivalent du verrou d'Unharnessed (`rewind.rs:34-36`).
3. `store.branch_to(Some(to))` (`session_tree.rs:140`), en mémoire comme Unharnessed (réponse 10) ; entrées de la branche relevées.
4. Entrées abandonnées : celles de l'ancienne branche après `to`.
5. **Remise en file** dans `core.pending_next_turn`, dans l'ordre :
   - les lignes abandonnées de type `ya-resources-changed` (`ya_resource_notice.rs:36`), pour la cohérence de `Tracker.known` (§I.1) ;
   - les lignes D2 déjà en file sont retirées (deux retours de suite : un seul texte) ;
   - puis le texte D2.
6. Verrou rendu. `engine.rebuild_session_context(entrées, GoalBranchReload::FaithfulBranch)` sur un fil bloquant (`engine.rs:170`, comme `branch_navigation.rs:532-542`).
7. ~~`queued_input_suspended` remis à son ancienne valeur.~~ **Corrigé au push 9 (§N)** : la pause est rendue quand la garde tombe, à la réponse ou à tout retour anticipé (`Drop`, `session_input_pause.rs:247-255`). La levée réveille le runner et réévalue la suite de goal due (`:156-170`). `queued_input_suspended` n'est jamais touché. Test T11.
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
- ~~**limite** : un abort reçu pendant la fenêtre du retour (session au repos) verrait sa suspension levée par la fin du retour.~~ **Retirée au push 9 (§N)** : l'action ne touche plus `queued_input_suspended` ; elle tient la pause de file d'upstream, indépendante de la suspension des aborts.

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

## J. PR 1 (PR #97) : relevé du push 1 (27/11/2026)

Push 1 : `53a0b6ebf` sur `ya-43-retour-arriere-pr1`. Tests et squelette : `turnStart` sans `rewindTo`, et `_yus/session/rewind` répondu comme non servi (`-32601`). C'est le comportement de `main`.

**Lignes upstream poussées : +5 / −1**, toutes dans `crates/pa-daemon/src/acp/daemon.rs` :
- `:1175-1178` (+4) : aiguillage de `_yus/session/rewind`, après celui de `steer` ;
- `:1637` (−1 +1) : `publish_prompt_start(&producer, turn_id, link, state)`.

| Run | Résultat |
|---|---|
| `ci` 45987538022 | `fmt`, `check + clippy (workspace)`, `windows cross-check + clippy`, `deny` verts. Shards 1, 4, 5, 6, 7 verts. **Shard 2/8** : 1 478 réussis, 1 en échec. **Shard 3/8** : 1 268 réussis, 1 en échec. **Shard 8/8** : `ya_acp_rewind_e2e`, 0 réussi, 10 en échec ; rien d'autre en échec |
| `ya-extension` 45987538056 | **extension** : `# tests 87`, `# pass 85`, `# fail 2`. **e2e vrai binaire** : 1 test en échec |
| `ya-secret-guard-macos` 45987538229 | vert, scan des lancements compris |
| `ya-build-macos-arm64` 45987502857 | vert |
| `Changelog fragment` 45987538337 | en échec, attendu : le fragment arrive au push 2 |

**Rouges, chacun sur son assertion** :

| Test | Job | Ligne | Assertion |
|---|---|---|---|
| fusion `notes_of_different_causes_are_journaled_together` | shard 2/8 | `usage_log/tests.rs:710` | `left: {"cause": "rewind"}`, `right: [{"cause": "rewind"}, {"cause": "masking", "turn": 5, …}]` : la note remplace la précédente |
| T6 `another_window_refuses_the_rewind` | shard 3/8 | `ya_turns/rewind.rs:80` | `left: None`, `right: Some("This conversation is open in another window: close it there before going back.")` |
| T1 `a_prompt_turn_start_carries_the_leaf_before_it` | shard 8/8 | `ya_acp_rewind_e2e.rs:506` | « rewindTo is the leaf before the prompt: {"origin":"prompt"} », `left: Null`, `right: "7a09ec70"` |
| T2/T7, T5a, T5b, T5c, T8, T9, T11 | shard 8/8 | `:432` (`accepted`) | « the rewind is accepted: … "code":-32601 … » |
| T3 `a_rewind_is_refused_while_a_turn_runs` | shard 8/8 | `:565` | `left: Some(-32601)`, `right: Some(-32603)` |
| T4 `a_rewind_to_an_entry_off_the_branch_is_invalid` | shard 8/8 | `:586` | `left: Some(-32601)`, `right: Some(-32602)` |
| E1 « servi (features().rewind) » | extension | — | `actual: false`, `expected: true` |
| E2 « modifier un message envoyé… » | extension | — | `actual: 0`, `expected: 1` : aucun rewind sur le fil, l'appel est refusé sur place |
| E3 « retour en arrière (ticket 43)… » | e2e vrai binaire | — | « rewindTo du deuxième tour (turnStart) : {… "prompt":{"text":"second"…} …} » : absent |

**Préconditions passées** :
- T5a : la ligne custom écrite par `append_custom_message` sur le socket du démon est acceptée, et la feuille avant le prompt est bien cette ligne ;
- T5b : l'annonce `[skills-changed]` part avec le deuxième prompt.

Les deux tests s'arrêtent ensuite sur le refus du rewind.

**Verts attendus, verts** : T6 `no_other_window_lets_the_rewind_go_on`, et tous les tests existants.

**À noter pour le push 2** : le job `extension` s'arrête aux tests unitaires. L'e2e de l'extension contre l'agent factice (`test/e2e/suite.ts`), qui reçoit maintenant `rewindTo` dans chaque `turnStart`, n'a donc pas tourné à ce push. Il tournera quand E1 et E2 seront verts.

## K. PR 1 (PR #97) : relevé du push 2 (27/11/2026), arrêt

Push 2 : `29a78549e`, avec le relevé `c8b90d142`. Implémentation, fragment `.changes/ya-43-retour-arriere.md`, `docs/usage.md`, `docs/fork-changes.md`.

**Lignes upstream du diff contre `main`** (`git diff --numstat origin/main...HEAD`) : `crates/pa-daemon/src/acp/daemon.rs` +5 −1, conforme. Tous les autres fichiers touchés sont du fork.

| Run | Résultat |
|---|---|
| `ci` 45990806492 | `fmt`, `check + clippy`, `windows cross-check`, `deny` verts. Shards 1, 2, 4, 5, 6, 7 verts. **Shard 3/8** : `acp_mode_e2e`, 45 réussis, 2 en échec. **Shard 8/8** : `ya_acp_rewind_e2e`, 9 réussis, 1 en échec |
| `ya-extension` 45990806431 | **extension** vert : unitaires 87/87 ; **e2e contre l'agent factice (`test/e2e/suite.ts`) : a tourné, « e2e : 21/21 réussis »**. **e2e vrai binaire** vert : « e2e vrai binaire : 20/20 réussis », E3 compris |
| `ya-secret-guard-macos`, `ya-build-macos-arm64`, `Changelog fragment` | verts |

**Les 14 tests du ticket** :
- **13 verts** : fusion des notes (shard 2/8), T6 (shard 3/8), T1, T2/T7, T3, T4, T5a, T5b, T5c, T8, T11 (shard 8/8), E1, E2, E3 ;
- **T9 rouge**, `ya_acp_rewind_e2e.rs:244`, « the daemon did not stop ». C'est l'aide d'arrêt du test qui est en cause, pas le retour en arrière :
  - elle fermait la connexion juste après avoir envoyé `shutdown`, et le démon répondait encore 15 s après ;
  - l'aide upstream (`acp_mode_e2e.rs:1672-1685`) garde la connexion ouverte jusqu'à la réponse (« The supervisor exits after the shutdown response ») ;
  - correction préparée dans le fichier du fork, non poussée : attendre la réponse de `shutdown` (`daemon_request`), puis jusqu'à 30 s que la socket ne réponde plus.

**Deux tests existants rouges, dans un fichier upstream** :
- **où** : `crates/pa-cli/tests/acp_mode_e2e.rs`, assertions ajoutées par le fork au ticket 25 (`docs/fork-changes.md:1337`) ;
- **pourquoi** : elles comparent le `turnStart` entier à `{"origin": "prompt"}`, et il porte maintenant `rewindTo` :
  - `acp_prompt_stream_completion_envelope_and_stop_reason_match_ts` (`:390`) : `left: {"origin": "prompt", "rewindTo": "3d5bbc21"}`, `right: {"origin": "prompt"}` ;
  - `acp_daemon_attached_cancels_mid_turn` (`:910`) : même cause ;
- **correction préparée (non poussée), +4 −2** : comparer `turnStart.origin` à `"prompt"`, même sens qu'avant pour l'origine :

```diff
@@ -390,9 +390,10 @@ fn acp_prompt_stream_completion_envelope_and_stop_reason_match_ts() {
             update["params"]["update"]["_meta"]["ai.primeintellect.prime-agent"]["turnStart"]
+                ["origin"]
                 .clone()
         }),
-        Some(json!({ "origin": "prompt" }))
+        Some(json!("prompt"))
@@ -910,7 +911,8 @@ fn acp_daemon_attached_cancels_mid_turn() {
             update["params"]["update"]["_meta"]["ai.primeintellect.prime-agent"]["turnStart"]
-                == json!({ "origin": "prompt" })
+                ["origin"]
+                == "prompt"
```

**Lignes upstream après cette correction : +9 −3** :
- `daemon.rs` +5 −1 ;
- `acp_mode_e2e.rs` +4 −2 (test upstream, lignes du fork du ticket 25).

C'est différent des +5 −1 annoncés : **arrêt, en attente de Yusuf** avant le push 3.

**Décision de Yusuf (27/11/2026)** : +4 −2 dans `acp_mode_e2e.rs` validés, soit un total upstream de +9 −3. Ligne ajoutée dans la section du ticket 43 de `docs/fork-changes.md`, qui renvoie à la ligne du ticket 25. Au push 3 : les deux corrections, et T12 (premier prompt d'une session).

**Ce que vérifient T9 et T11 maintenant que la méthode est servie** :
- **T9** (une fois l'aide d'arrêt corrigée) : après un retour et un prompt, le démon est arrêté pour de bon. Un nouveau worker relit le fichier de session au `--resume`, et le prompt suivant part de la nouvelle branche ; le prompt abandonné en est absent.
  - Il prouve que la feuille déplacée en mémoire est inscrite dans le fichier par l'ajout suivant (le nouveau prompt a pour parent le point de retour), et qu'une vraie reprise la suit.
  - Il ne couvre pas un retour **sans** prompt suivant avant la reprise (limite acceptée, réponse 10).
- **T11** (vert) : un `bash()` lancé dans le tour abandonné (3 s) finit après le retour. Son avis `[bash-done]` réveille quand même la session : tour `wake` jusqu'à sa frontière, avis et réponse sur la nouvelle branche, prompt abandonné absent. C'est K-a.
  - Il vérifie aussi l'étape 7 : si la suspension de la file posée pendant le retour n'était pas levée et le runner réveillé, l'avis resterait en file et le test échouerait faute de tour `wake`.
  - Il ne vérifie pas où se place le texte D2 par rapport à l'avis.

Mesures et contrôles (`ya43_rewindto_cost.py`, `ya43_rewind_check.py`, `compare.py --all`, `compare_frames.py`, scripts des §G.6 et §I.7) : pas lancés, en attente du push 3.

## L. Push 3 à 5 : T9, premier prompt, défaut du texte fixe (27/11/2026)

### L.1 Push 3 (`3a22a6f5c`) et push 4 (`623f15bae`)

- **T9 vert** au push 3 (aide d'arrêt corrigée).
- **T12** (premier prompt d'une session) :
  - `rewindTo` est le parent de la ligne du premier prompt (assertion verte) ;
  - **mon affirmation tirée du code était fausse** : cette entrée n'est pas `service_tier_change` mais `session_state` (`status: "active"`), que le worker écrit juste après le préfixe de création d'une session adossée à un fichier (`crates/pa-daemon/src/worker/create.rs:444-452`) ;
  - type corrigé au push 4 ; un retour vers ce point fait repartir la conversation.
- **Checks requis du push 4** (ruleset `protect-main`, `gh pr checks 97 --required`) : **17/17 verts**, `test summary` compris. Ils passaient avec le défaut du §L.3, que les tests ne voyaient pas.

### L.2 Coût de `rewindTo` (binaire du push 3, code produit identique au push 4)

Commande :

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/ya43_rewindto_cost.py --main <main f7c4b3fb7> --branch <branche 3a22a6f5c> --out <dossier>
```

10 blocs alternés de 20 prompts par binaire :

| Binaire | Version | Échantillons | Avec rewindTo | Médiane (ms) | p95 (ms) |
|---|---|---|---|---|---|
| main | `0.10.2-continuous.f7c4b3fb79daaa9614dcffe78223cf548ab7f014` | 200 | 0 | 0.069 | 0.114 |
| branch | `0.10.2-continuous.3a22a6f5c6d18f54c8051c20f87ea7a171594c06` | 200 | 200 | 0.111 | 0.174 |

**Différence des médianes (branche − main) : 0.042 ms**, sous le seuil de 10 ms.

Contrôle de vraisemblance (trames horodatées d'une session, branche) : le `turnStart` avec `rewindTo` arrive 0,1 à 0,4 ms après l'envoi du prompt, la réponse vers 30 ms.

### L.3 Défaut : le texte fixe n'atteint pas le modèle

`ya43_rewind_check.py` sur le binaire du push 3 donne **NON CONFORME** :
- C4 « texte fixe une fois » et « avant le prompt modifié » : NON. La 4e requête ne porte, après le point de retour, que le prompt modifié ;
- les autres contrôles sont conformes : quatre requêtes, hors `messages` identique, préfixe identique jusqu'au point (marques comprises), rien du prompt d'origine, `rewindTo` partout, retour accepté, `invalidation: rewind` sur la 4e seulement.

**Cause** :
- l'action `rewind` rangeait le texte fixe dans les lignes en attente du worker (`pending_next_turn`), que le tour écrit dans le fichier de session (`worker/turn.rs:1036-1047`), mais qui n'entrent pas dans le contexte du modèle ;
- l'annonce du ticket 31 passe aussi par la file des lignes du tour suivant du moteur (`queue_next_turn_row`, `ya_resource_notice.rs:390-440`), que l'admission envoie au modèle (`admission.rs:143-149`) ;
- même défaut pour l'annonce `[skills-changed]` remise en file.

**Pourquoi T7 vérifiait le fichier** :
- le fournisseur factice scripté des e2e (`"engine": "faux"`) ne garde pas les requêtes ;
- j'ai pris la branche du fichier comme ce que reçoit le modèle, une hypothèse reprise de l'en-tête de l'e2e du ticket 39, « The session file is the conversation the model is sent » ;
- elle tient pour les lignes que la boucle écrit, pas pour celles que le worker range lui-même ; je ne l'ai pas vérifiée.

**Pourquoi E3 était vert** :
- le §I.5 disait « la troisième requête reçue par le serveur », mais l'e2e de l'extension sur le vrai binaire tourne avec le fournisseur factice, sans serveur ;
- j'ai lu le fichier de session à la place, sans le dire ; le worker y écrit bien le texte fixe.

### L.4 Correctif (option C, décision de Yusuf) et tests au niveau de la requête

**Correctif, aucune ligne upstream (total toujours +9 −3)** :
- `crates/pa-core/src/session_engine/ya_resource_notice/rewind.rs` (sous-module du fichier du fork) : `AgentSession::ya_queue_rewind_rows` range les lignes dans la file des lignes du tour suivant du moteur ; un texte fixe en attente est remplacé ;
- `crates/pa-daemon/src/agent_engine/ya_steer/rewind.rs` (sous-module du fichier du fork) : accès du worker à cette file ;
- l'action `rewind` y range le texte fixe et les annonces abandonnées. Elle les garde aussi dans ses lignes en attente, pour le fichier, comme le ticket 31.

Visibilité (sans ligne upstream) : `pending_next_turn_rows` est un champ privé d'`AgentSession` (`pa-core/src/session_engine/mod.rs:146-176`), visible depuis tout sous-module de `session_engine` ; `AgentSessionEngine.session` est `pub(crate)` (`agent_engine.rs:210`), et `session` du moteur cœur est `pub` (`engine.rs:115`).

**Tests qui lisent la requête** :
- **e2e Rust T7, T5b, T5c, T9** : sur un modèle `anthropic-messages` local (`ModelServer`) qui garde le corps de chaque requête. Sans fichier de script, le worker prend le vrai moteur (`worker/config.rs:62-67`). T9 lit la requête avant et après le redémarrage du démon ;
- **test unitaire `pa-core`** sur `ScriptedProvider` : la requête porte les lignes une fois, avant le prompt ; deux retours, un seul texte fixe ;
- **E3** : modèle `anthropic-messages` local en Node, script du fournisseur factice retiré pour la conversation ; la troisième requête porte le texte fixe une fois, avant le renvoi ;
- **`ya43_control.py`** : sur le serveur de capture, scénario `ya43-control` ajouté à `server.py` (réponse selon le texte du dernier prompt) ; G1 lit la requête qui suit le retour. `ya43_rewind_check.py` la lisait déjà.

**Vus rouges sur le binaire actuel, avant correctif** :
- push 5 (`830e55a37`, tests seuls, `ya_queue_rewind_rows` en squelette sans effet), run `ci` 45997753953 :
  - `pa-core` `the_rows_of_a_rewind_reach_the_next_request_once` (`rewind.rs:99`) : `left: 0`, `right: 1`, la requête ne porte que `THE-PROMPT` ;
  - T7 (`:766`) : « the fixed text, once », `left: 0` ;
  - T5b (`:903`) : « the announcement reaches the next request again, once », `left: 0` ;
  - T5c (`:937`) : « the first rewind's fixed text reaches its request », `left: 0` ;
  - T9 (`:1004`) : « before the restart, the fixed text and the announcement once », `left: (0, 0)`, `right: (1, 1)` ;
- E3, run `ya-extension` du push 5 : « le texte fixe une fois dans la troisième requête », la requête ne porte que `ALPHA`, `answer 1`, `CHARLIE` ; « e2e vrai binaire : 19/20 réussis » ;
- en local, sur le binaire du push 3 : `ya43_rewind_check.py` C4 NON ; `ya43_control.py` G1 « texte fixe une fois dans la requête » et « avant le renvoi » NON. Tous ses autres contrôles sont conformes (G1 à G3, G2, I1, I2).

**Hors assertion au push 5** : `check + clippy` en échec sur le harnais HTTP du test (tableau de 64 Kio sur la pile, `format!` dans un `collect`). C'est du code de test seulement, corrigé au push 6.

**Erreurs du script de contrôle, corrigées avant ce relevé** : G3 relançait sans `--resume` (« Relancer » passe par `--resume <fichier>`, ticket 28), et lisait mal le texte des messages rejoués (objet et non liste). Ce n'étaient pas des défauts du binaire.

## M. Push 6 à 8 : correctif vert, comparaisons, contrôles (27/11/2026)

### M.1 CI

- **Push 6** (`6f573814e`, correctif) : e2e Rust (shard 8/8) et e2e de l'extension sur le vrai binaire verts, E3 compris. Le texte fixe et l'annonce arrivent maintenant dans la requête au modèle.
  - Rouge restant : le test unitaire `pa-core` (`rewind.rs:103`, `left: 0`). Le test était mal monté : avec un `Agent` nu, la conversion par défaut ne garde que les messages user, assistant et résultat d'outil (`crates/pa-agent/src/agent_loop/mod.rs:150-160`) et retire les lignes custom avant la requête.
- **Push 7** (`4a4382723`) : conversion des lignes custom donnée au test, mais avec les messages de session au lieu des messages de l'agent. Erreur de compilation (`E0308`, deux types `AgentMessage` homonymes), donc `check + clippy` et `windows cross-check` rouges.
- **Push 8** (`ba5c1eb96`) : le test prend `messages::engine_convert_to_llm()` (`crates/pa-core/src/session_engine/messages.rs:464`), le convertisseur que le moteur donne à la boucle de l'agent.
  - **Checks requis (`gh pr checks 97 --required`, ruleset `protect-main`) : 17/17 verts**, `test summary` compris ; `bench`, `build release binaries`, `report + regression gate` et `windows battery (on-demand)` sont en skipping (hors liste requise).
  - Run `ci` 46002986992, tous verts :
    - `pa-core` `the_rows_of_a_rewind_reach_the_next_request_once` et `notes_of_different_causes_are_journaled_together` (shard 2/8) ;
    - T6 (shard 3/8) ;
    - `acp_mode_e2e` corrigés (`acp_prompt_stream_completion_envelope_and_stop_reason_match_ts`, `acp_daemon_attached_cancels_mid_turn`) ;
    - les 11 e2e de `ya_acp_rewind_e2e` : T1, T3, T4, T5a, T5b, T5c, T7, T8, T9, T11, T12 (shard 8/8).
  - `ya-extension` : unitaires et e2e contre l'agent factice verts ; e2e vrai binaire vert, E3 compris.

### M.2 Contrôles sur le binaire de la branche

Binaire du push 6 (`0.10.2-continuous.6f573814e…`), au code produit identique aux pushs 7 et 8 : ces derniers ne changent qu'un test `pa-core`.

| Commande (`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I …`) | Résultat |
|---|---|
| `bench/tools/capture/ya43_rewind_check.py --binary <branche> --out <dossier>` | **CONFORME** : C1 à C6 « oui » ; texte fixe une fois et avant le prompt modifié ; préfixe identique jusqu'au point, marques comprises |
| `bench/tools/capture/ya43_control.py --binary <branche> --out <dossier>` | **CONFORME** : G1 (texte fixe une fois dans la requête qui suit le retour, avant le renvoi ; rien des tours abandonnés ; le kernel garde `x`, `X_NOW 1`), G2 (refus pendant le tour, tour au bout), G3 (après « Relancer », rejeu de `premier`, `deuxième, modifié`, `x existe-t-il ?`), I1 (ligne de `/refine` gardée), I2 (refus avec le texte validé quand un autre client attaché répond, retour accepté une fois détaché) |
| `bench/tools/capture/run.py --binary <main f7c4b3fb7>` puis `--binary <branche>`, puis `bench/tools/capture/compare.py <run main> <run branche> --all` | **TOUT IDENTIQUE** : 17 cas (dont `acp`, `acp-extension`, `tui`), chaque requête identique (UUID normalisés). Sans retour en arrière, rien ne change dans ce qui part au modèle |
| `bench/tools/capture/acp_frames.py --binary <main>` puis `--binary <branche>`, puis `compare_frames.py <trames branche> --against <trames main> --expect '.params.update._meta.ai.primeintellect.prime-agent.turnStart.rewindTo'` | **écarts : 0** ; seul écart, attendu : `turnStart.rewindTo`, présent dans 4 trames (cas A, B, C) |
| `compare_frames.py <trames branche>` (contre l'agent factice de l'extension) | **écarts : 0** ; notice, steer et `turnStart` identiques |

Avant le correctif (binaire du push 3), les mêmes outils donnaient : `ya43_rewind_check.py` C4 NON ; `ya43_control.py` G1 « texte fixe une fois dans la requête » et « avant le renvoi » NON (§L.4).

**Préfixe** : prompt système, tools et `yaRequest` inchangés (`compare.py --all`). Après un retour en arrière, la requête reprend le préfixe d'avant jusqu'au point de retour, octet pour octet, marques de cache comprises. Le texte fixe s'ajoute après, puis le prompt.

### M.3 Contrôle manuel pour Yusuf (§G.6 et §I.7)

Préparation (deux pièces de la PR : le binaire **et** l'extension, car `src/ya/methods.ts` change ; l'extension déjà installée refuse `_yus/session/rewind` sur place) :
1. **Extension** : installer le `.vsix` du run `ya-extension` du dernier commit poussé, puis recharger la fenêtre :

   ```
   extensions/vscode/scripts/install-vsix.sh <id du run ya-extension du dernier commit>
   ```

   - Au push 9b (`84fecf29d`), c'est le run **46010873742**. Son artefact porte le sha du commit de merge de la PR, normal pour un run `pull_request`. Avant lui : 46005389185 (`467c8e19c`).
   - Le script vérifie la taille (< 5 Mo) et le contenu attendu avant d'installer.
2. **Binaire** : télécharger le build macOS du dernier commit de code, vérifier `SHA256SUMS` et extraire :
   - `gh run download 46010862163 --repo YusufByn/yusagent --dir <dossier>` (`84fecf29d`, push 9b ; binaire `0.10.2-continuous.84fecf29d…`) ;
   - dans VS Code, régler `yusagent.binaryPath` sur ce binaire, puis recharger la fenêtre ;
   - un démon d'un autre build est remplacé s'il ne tient rien ; sinon, `yusagent shutdown` d'abord (ticket 29).
3. Vérifier que l'édition est servie : un clic sur un message envoyé doit le remettre dans la boîte, avec le bandeau « Editing an earlier message ». Si l'envoi répond « It could not go back to the message: _yus/session/rewind : méthode pas encore servie par yusAgent », c'est l'ancienne extension.

Lancement préalable par Claude Code (règle 13), dans un dossier d'extensions isolé, pour ne pas toucher au VS Code de Yusuf :

```
$ sh extensions/vscode/scripts/install-vsix.sh 46010873742 <dossier isolé>
taille : 202181 octets (limite 5000000)
sha256 : 43acc6f6a0a8e61d01551b5e70709b872956b71ca2e1bffa057d12e1f662fd52
contenu :
  [Content_Types].xml
  extension.vsixmanifest
  extension/LICENSE.txt
  extension/dist/extension.js
  extension/dist/webview/main.css
  extension/dist/webview/main.js
  extension/media/icon.svg
  extension/package.json
  extension/readme.md
Extension 'yusagent.vsix' was successfully installed.
yusagent.yusagent@0.1.0
installé
```

`extension/dist/extension.js` installé liste `"_yus/session/rewind"` parmi les méthodes servies, juste après `"_yus/session/steer"`.

Étapes, avec ton modèle habituel :
1. Nouvelle conversation. Envoyer « Réponds juste : un. »
2. Envoyer « Crée une variable x = 1 dans le REPL Python et confirme. » : une cellule pose `x`.
3. Envoyer « Réponds juste : trois. »
4. Cliquer sur le deuxième message. Le bandeau « Editing an earlier message » apparaît, et les tours suivants sont atténués. Écrire « Ne crée rien, réponds juste : deux. », puis envoyer.
   - Attendu : le deuxième et le troisième tour disparaissent, la nouvelle réponse s'affiche, et aucune ligne « conversation-rewound » n'est visible.
5. Envoyer « La variable x existe-t-elle encore dans le REPL ? Vérifie avec une cellule, sans la recréer. »
   - Attendu : la cellule affiche `x = 1`. Le modèle ne doit pas s'en étonner : il sait que le kernel a gardé son état (texte fixe).
6. Envoyer « Lance une cellule qui dort 20 secondes. » Pendant la cellule, cliquer sur un message précédent : le clic ne fait rien tant que le tour tourne. Arrêter avec Stop.
7. Recharger la fenêtre, puis rouvrir la conversation depuis l'historique.
   - Attendu : l'historique est la nouvelle branche (« deux », pas « trois »).
   - Limite de la PR 1 : les messages d'avant le rechargement ne sont plus modifiables, seuls les nouveaux le sont.
8. Ouvrir la même conversation dans un terminal (`yusagent --resume <chemin du fichier de la conversation>`). Puis, dans l'extension, modifier un message envoyé depuis ce rechargement.
   - Attendu : refus « This conversation is open in another window: close it there before going back. »
   - Quitter le TUI, puis recommencer : le retour passe.
9. (§I.7, 1) Taper `/refine`, puis modifier le message suivant : la ligne de résultat de `/refine` reste dans la conversation (`ya43_control.py`, I1, l'a vérifié par ACP).
10. (Goal, push 9) Nouvelle conversation :
    - envoyer « Réponds juste : un. », puis « /goal --budget 3000 Écris un haïku sur la mer, puis arrête. » ;
    - la ligne du goal apparaît en haut de la boîte (objectif, minuteur, pause et fin), et elle reste après le tour (`complete` ou `budget_limited`) ;
    - cliquer sur le message `/goal`, le remplacer par « Réponds juste : deux. », puis envoyer.
    - Attendu : la ligne du goal disparaît dès le retour, avant la réponse à « deux ». La conversation ne reste pas « occupée » : un autre chat ouvert, celui-ci se met en veille après le délai habituel.
    - Cas inverse (facultatif) : goal posé, puis `/goal clear` (la ligne disparaît), puis modifier le message `/goal clear` : la ligne du goal revient, dans son état d'avant (`ya_acp_rewind_e2e`, T14).

Ce que le contrôle manuel ne peut pas voir : la requête elle-même. Le texte fixe une fois et le préfixe sont vérifiés par `ya43_rewind_check.py` et par les e2e T7, T5b, T5c et T9.

## N. Push 9 : goal republié et pause de file d'upstream (27/11/2026)

### N.1 L'écart et sa correction

- **L'écart** : le §I.3, étape 5, validé, prévoyait de republier le goal après un retour, comme Unharnessed (`rewind.rs:64`). Au push 2, je l'ai retiré :
  - j'avais seulement vu que `fetch_autonomous_status` attend la fin d'un travail sans tête, et j'en avais conclu « le pont n'a pas de lecture du goal à republier » ;
  - je l'ai noté dans une phrase d'un message d'attente et dans `docs/fork-changes.md`, mais pas posé comme changement au plan à valider, ni écrit dans un relevé ; le §I.3 disait toujours l'inverse.
- **La conclusion était fausse** : le worker avait déjà le mécanisme, utilisé par `navigate_tree` :
  - la reconstruction recharge le goal depuis la branche et met de côté le `goal_update` (`agent_engine.rs:221-223`, `session_engine_impl.rs:815-835`) ;
  - `handle_navigate_tree` le publie avant la réponse (`worker/commands.rs:199-215`) ;
  - le pont le passe en `_meta.goal` (`acp/wire_events.rs:260`).
- **Effet pour l'utilisateur, sans la correction** :
  - la ligne du goal des tours abandonnés restait affichée, minuteur et boutons compris ;
  - la conversation restait « occupée » (`conversations.ts:58-60`) et ne se mettait pas en veille en arrière-plan (`:262-266`).
- **Correction (push 9b)** : comme `handle_navigate_tree`, dans `ya_permissions/rewind.rs` (fichier du fork), aucune ligne upstream :
  - `goal_update_after_rebuild()`, puis un événement `goal_update` émis avant la réponse au retour ;
  - et la pause de file d'upstream (§N.3) ;
  - §I.3 (étapes 2, 5 et 7) et §I.9 corrigés et barrés ; ligne de `docs/fork-changes.md` corrigée.

### N.2 Le goal après un retour

- **Forme quand le goal disparaît** (établie au code) :
  - c'est l'état vide, `empty_goal_state()` (`crates/pa-types/src/goal.rs:78-90`) : `{"active": false, "status": "idle", "tokensUsed": 0, "timeUsedSeconds": 0, "continuationsUsed": 0}`, sans `objective` ;
  - il vient de `reload_from_branch` (`goal_driver.rs:125`, `latest_persisted_state` → `empty_goal_state` sans entrée de goal sur la branche) ;
  - il est publié par `publish_goal_state` (`goal_continuation.rs:413-429`), qui déduplique contre le dernier publié ;
  - le pont donne `_meta.goal` avec `status: "idle"`, et l'extension efface la ligne (`chat.ts:734-737`).
- **T13** : `/goal --budget 5 …` dans un tour, puis retour avant ce tour. La trame `_meta.goal` (`idle`, sans objectif) arrive avant la réponse au retour.
- **T14, cas inverse, sans course** :
  - le goal du point est `budget_limited`, effacé par `/goal clear` dans le tour abandonné ; après le retour, `budget_limited` revient, avec son objectif, avant la réponse ;
  - **le cas « goal actif au point de retour » n'est pas testé** : il n'est pas faisable sans course. Un goal actif enchaîne seul ses tours de suite, et la levée de la pause réévalue la suite due (`session_input_pause.rs:163-165`) : un tour de goal peut partir pendant qu'on relève les trames. `budget_limited` est un état stable, que le goal ne quitte pas seul.

### N.3 Q-I5 : la pause de file d'upstream à la place de la bascule

- **Avant** : l'action posait `core.queued_input_suspended` et le remettait à la fin, ce qui pouvait lever la suspension d'un abort reçu pendant la fenêtre (limite du §I.9).
- **Maintenant** : `let _queued_work = self.hold_queued_work_pause("navigate");` du début à la réponse, comme `handle_navigate_tree`.
- **Pourquoi la limite du §I.9 est retirée**, prouvé au code (pas par un test : un abort dans la fenêtre du retour serait une course) :
  - `ya_permissions/rewind.rs` n'écrit plus `queued_input_suspended` (le nom n'y figure que dans un commentaire) ;
  - la garde d'admission du runner bloque si une pause est tenue **ou** si la suspension est posée, deux conditions indépendantes (`worker/turn.rs:59`) ;
  - quand la garde de la pause tombe (`Drop`, `session_input_pause.rs:247-255`), elle retire seulement sa pause de la table, puis réveille le runner (`work_notify.notify_one()`) et réévalue la suite de goal (`:156-170`) ; elle ne touche pas la suspension.
- **T11 vert** : l'avis `[bash-done]` d'une commande du tour abandonné réveille la session après le retour. La file reprend quand la pause tombe.

### N.4 Rouges puis verts

- **Push 9a** (`7eb4d9859`, tests seuls, code du push 8), run `ci` 46008919359, shard 8/8 :
  - T13 (`ya_acp_rewind_e2e.rs:1171`) « the goal frame clears the goal before the rewind answers: None », `left: None`, `right: Some(("idle", Null))` ;
  - T14 (`:1211`) `left: None`, `right: Some(("budget_limited", "reply with exactly: GOAL-PROGRESS"))` ;
  - préconditions vertes (goal affiché après son tour, effacé par `/goal clear`) ; les 11 autres e2e verts, T11 compris.
- **Push 9b** (`84fecf29d`, correctif et doc), run `ci` 46010873856 : les 13 e2e de `ya_acp_rewind_e2e` verts, T11, T13 et T14 compris.

### N.5 Relevé

- **Checks requis** (`gh pr checks 97 --required`, ruleset `protect-main`) : **17/17 verts**, `test summary` compris.
- **Lignes upstream** (`git diff --numstat origin/main...HEAD`) : `crates/pa-daemon/src/acp/daemon.rs` +5 −1 et `crates/pa-cli/tests/acp_mode_e2e.rs` +4 −2, soit **+9 −3**, inchangé.
- **Runs du push 9b** :
  - `ya-extension` **46010873742** (unitaires, e2e contre l'agent factice, e2e vrai binaire, `.vsix`) ;
  - `ya-build-macos-arm64` **46010862163** (binaire `0.10.2-continuous.84fecf29dde96888e0305001c093d2704823a90a`).
- **Sur ce binaire** (`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I …`) :
  - `bench/tools/capture/ya43_rewind_check.py` : **CONFORME** (C1 à C6) ;
  - `bench/tools/capture/ya43_control.py` : **CONFORME** (G1, G2, G3, I1, I2) ;
  - `bench/tools/capture/run.py --binary <branche 84fecf29d>` puis `compare.py <run main f7c4b3fb7> <run branche> --all` : **TOUT IDENTIQUE** (17 cas, chaque requête identique).
- **`.vsix` du push 9b** : `install-vsix.sh 46010873742` lancé dans un dossier d'extensions isolé (règle 13), sortie au §M.3 ; `extension.js` installé sert `_yus/session/rewind`.
- **Contrôle manuel** : étape 10 (goal) ajoutée au §M.3, et préparation sur les runs du push 9b.

## O. Contrôle manuel de Yusuf et merge de la PR #97 (27/11/2026)

**Contrôle manuel** (§M.3), sur le binaire `84fecf29d` (run `ya-build-macos-arm64` 46010862163) et le `.vsix` du run `ya-extension` 46010873742 :
- **étapes 1 à 8 et 10 : conformes** ;
- **étape 6** : faite avec un calcul long (`sum(i*i for i in range(400_000_000))`), car le modèle a refusé deux fois une cellule bloquante `time.sleep(60)` / `time.sleep(90)` (« Je ne peux pas bloquer le tour ») ;
- **étape 8** : un second refus juste après la fermeture du TUI, puis le retour passe. C'est le délai de détachement d'un client, comme pour C12 (ticket 28) ;
- **étape 9 sautée** : `/refine` est désactivé dans les réglages de Yusuf (« Harness refinements are disabled in /settings ») ; le cas est couvert par T5a (`ya_acp_rewind_e2e`) et I1 (`ya43_control.py`).

Compte rendu en commentaire de la PR : https://github.com/YusufByn/yusagent/pull/97#issuecomment-7221537488.

**CI avant le merge** :
- sur `e1b6c4d34` (commit de doc), `runtime suite (python 3.11)` a échoué une fois sur `test_harness.HarnessStateTest.test_concurrent_subprocess_creates_all_land` (`prime-agent-runtime/test/test_harness.py:555`, verrou de l'état du harness non obtenu entre sous-processus) ;
- relance unique du job par Yusuf, en exception à la règle (option 1), notée en commentaire de la PR avec le journal (https://github.com/YusufByn/yusagent/pull/97#issuecomment-7220753143) ;
- relance verte (job 140561850267), et 17/17 checks requis verts (https://github.com/YusufByn/yusagent/pull/97#issuecomment-7220790528).

**Merge** : PR #97 mergée le 27/11/2026 à 15:01:38 UTC, avec `--match-head-commit e1b6c4d347ffca7e8a69d0c21beb4040b1d118d9`. Le premier essai a reçu une erreur GraphQL de GitHub (`E2E3:C8AC8:142083D:1E2CE52:6B099B40`) sans effet ; le second a mergé. **`main` = `c6f0b82785618f0e92e7fcc731e24d3bb8ef0749`.**

**Build macOS de `main`** : run `ya-build-macos-arm64` **46021226712**, lancé à la main (`workflow_dispatch`) sur `c6f0b8278`.

**Tickets à part ouverts par ce ticket** :
1. **Le modèle refuse une cellule bloquante** (`time.sleep(60)`, « Je ne peux pas bloquer le tour ») : observation du contrôle manuel. Constat à faire : texte du prompt (règles du kernel sur les attentes longues) ou garde du runtime qui rejette les longs `sleep` (upstream `3085b841d` « async L1: reject long sleeps and direct process calls before a cell runs », et `b0c4359df` « sleep re-allowed » dans `upstream/main`). Non mesuré.
2. **`test_concurrent_subprocess_creates_all_land`** (test upstream du runtime) : un échec sur onze runs de la branche, même code vert avant et après. À surveiller ; s'il échoue de nouveau, candidat à « Instabilité probable » de `docs/fork-changes.md`.
3. **`rewindTo` au rejeu de `session/load`** (Q-I4) : après « Relancer » ou une reprise depuis l'historique, seuls les nouveaux messages sont modifiables.
4. **`ab_stats.py:339`** : `.get("cause")` échoue sur un tableau d'invalidations (forme possible depuis le ticket 24), défaut existant du banc.

**Suite du ticket 43** : PR 3 (instantanés du workspace dans un dépôt git shadow, YA-18), puis PR 4 (panneau du kernel), selon les décisions du §H.

## P. PR 3 : instantanés du workspace (YA-18), constat et plan de code, sans code (27/11/2026)

Branche `ya-43-instantanes`, depuis `main` à `c6f0b8278`. Premier commit : `b3b0528a7` (§O) reporté par cherry-pick, sans conflit (`7925aa0b3`).

Écarts de mise en place, signalés :
- la `main` locale était restée à `1ece0ff04` (merge de la PR #90), ancêtre de `origin/main`. Un premier essai de cherry-pick sur cette base a donné un conflit (le ticket n'y existe pas encore). Je l'ai annulé, puis j'ai avancé la `main` locale en fast-forward sur `c6f0b8278` et refait la branche : cherry-pick propre ;
- **la suppression de la branche locale `ya-43-doc-a-reporter` a été refusée** par le contrôle des permissions de la session. Elle existe toujours, et son seul commit est maintenant dans cette branche. À supprimer à la main : `git branch -D ya-43-doc-a-reporter`.

**Constat et plan seulement** : aucun code produit, rien compilé, aucun appel de modèle, rien dans `docs/fork-changes.md`. Une pièce jointe : le script de mesure `docs/tickets/43-retour-arriere/shadow_git_workspace.py`, lancé par Claude Code (§P.3). Les lignes citées sont celles de `main` à `c6f0b8278`. Upstream récupéré le 27/11/2026 : `upstream/main` à `4b347d96d`.

### P.1 Mécanisme d'upstream, cherché d'abord

- **Aucun mécanisme d'instantané ni d'annulation chez upstream.** Méthode :
  - `git log c83ac68b3..upstream/main | grep -iE 'checkpoint|snapshot|undo|rewind|restore|shadow|revert|rollback'` ;
  - `git grep -il -E 'shadow.?(git|repo)|checkpoint' upstream/main` ;
  - aux deux, seulement les points de reprise de compaction et de file, le noyau rechargé, `workspace_snapshot` (supprimé, `20b4ad74b`) et l'empreinte des gates autonomes.
- **À reprendre : `a3e261c27`** (précisé au §P.11 : ses options git sont reprises dans le module du fork, sans cherry-pick du commit) (11/11/2026, après la base du fork, absent chez nous : `grep fsmonitor crates/pa-core/src/autonomous/gates.rs` ne trouve rien). Il protège les appels git d'upstream contre les programmes que la config d'un dépôt peut lancer (`upstream/main:crates/pa-core/src/autonomous/gates.rs:1-9`, `:284-299`, `:354-367`) :
  - `-c core.fsmonitor=false` ;
  - pas de hook (`diff.autoRefreshIndex=false`, `--no-optional-locks`) ;
  - `GIT_ALLOW_PROTOCOL=""` et `GIT_NO_LAZY_FETCH=1`.
  
  Upstream laisse un risque ouvert : les filtres `clean`/`process` de `.gitattributes` (`gates.rs:7-9`). **Chez nous, ce risque est fermé par construction** : un pilote de filtre n'existe que dans une config git. Or le git des instantanés ne lit aucune config hors de celle du dépôt shadow :
  - `GIT_CONFIG_NOSYSTEM=1` et `GIT_CONFIG_GLOBAL=/dev/null` ;
  - `core.excludesFile=/dev/null` ;
  - `GIT_DIR` = le dépôt shadow, donc le `.git/config` du projet n'est jamais lu.
  
  `core.hooksPath=/dev/null` est ajouté par prudence, même si le dépôt shadow n'a pas de hook.
- **Variables d'environnement** : `workspace_snapshot/git.rs:111-126` retire 7 variables `GIT_*`. Le plan va plus loin : `env_clear()`, puis seulement `HOME`, `PATH`, `TMPDIR`, `LANG=C` et les variables ci-dessus. Aucune variable héritée (`GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`, …) ne passe.
- **Plafonds de référence** : `workspace_snapshot` bornait à 20 000 entrées, 512 Mio au total et 64 Mio par fichier (`crates/pa-core/src/workspace_snapshot/mod.rs:66-78`). Ces bornes servent de point de départ au §P.3.
- **Feuille de session** : le seul moyen d'upstream d'inscrire une feuille tout de suite est `append_branch_summary` (`crates/pa-daemon/src/session_tree.rs:160`). Il ajoute un résumé, qui change ce que reçoit le modèle. Écarté (§P.7).
- **Réponse 11** : `workspace_snapshot` reste tel quel. Rien n'en est importé : seules l'idée du nettoyage de l'environnement et les bornes sont reprises.

### P.2 Accroches dans le pont : zéro ligne upstream pour les instantanés

- **Instantané de début** (réponse 3) : dans `publish_prompt_start` (fork, `crates/pa-daemon/src/acp/ya_turns.rs:254-271`), après la lecture de la feuille.
  - Cette fonction est attendue avant l'envoi de `PromptAndWait` au worker (`crates/pa-daemon/src/acp/daemon.rs:1637`, puis `:1641`). L'instantané est donc pris **avant toute cellule du tour**.
  - Il attend la fin de l'instantané, sans délai de garde ; le coût est mesuré au §P.6. Q-P2.
  - Si l'instantané échoue (git absent, plafond dépassé, erreur), le tour part quand même. Ce tour n'a alors pas de point de restauration des fichiers.
- **Instantané de fin** : dans `AgentTurns::close` (fork, `ya_turns.rs:154-198`), à chaque `agent_end`, avant `self.running.take()?` (`:161-163`). `close` voit tous les `agent_end` :
  - ceux d'un prompt de ce client (alors `running` est vide) ;
  - ceux d'un réveil ;
  - ceux d'un prompt du TUI.
  
  L'instantané de fin suit donc le dernier tour, quel qu'il soit.
- **Ordre** : une file série par workspace (un canal et une tâche dans le pont). `close` y range le travail de fin **de façon synchrone**, avant que le prompt suivant puisse ranger son début. Le contrôle et la restauration passent par la même file.
- **Limites de la réponse 3** :
  - un prompt du TUI ou de `-p` n'a pas d'instantané de début. Il n'a pas non plus de `rewindTo` : il n'est pas un point de retour ;
  - un `bash()` en arrière-plan qui écrit après la fin du tour est pris par l'instantané suivant (§C) ;
  - pendant une restauration, il peut encore écrire (K-a, réponse 2).
- **Dossier du workspace et identifiant de session** : `publish_prompt_start` n'a ni l'un ni l'autre, et le pont ne les range pas dans `state` (`daemon.rs:307-314`). L'action `leaf` du worker (fork, `crates/pa-daemon/src/ya_permissions/rewind.rs:92-100`) renverra `{ leafId, sessionId, cwd }` :
  - `store.session_id()` : l'id d'en-tête du fichier, stable d'un chargement à l'autre (§I.9, étape 8) ;
  - `core.cwd` (`crates/pa-daemon/src/worker/session_core.rs:18`).
  
  Pas d'aller-retour en plus : `leaf` est déjà lu à chaque prompt. Un registre du processus (id de session du démon → contexte) sert à `close` et aux méthodes.

### P.3 Mesures sur `~/Desktop/yusagent`, exclusions et plafonds (réponse 6)

Script : `docs/tickets/43-retour-arriere/shadow_git_workspace.py`. Il n'écrit jamais le workspace mesuré : git n'y fait que lire, et le dépôt shadow et l'index sont dans le dossier de travail.
- La restauration est mesurée sur une copie des fichiers candidats.
- Le profil Seatbelt est **reconstitué à la main** d'après `ya_guard.rs:278-327`, avec des sockets et un binaire factices. C'est une approximation du binaire : la mesure sur le binaire est au §P.6.
- Pas de modèle.

Commande et sortie du lancement par Claude Code (27/11/2026, macOS 27.0.1, Apple M5, git 2.55.0) :

```
$ D=$(mktemp -d) && python3 -I docs/tickets/43-retour-arriere/shadow_git_workspace.py ~/Desktop/yusagent "$D" 5
workspace /Users/yusufyuss/Desktop/yusagent ; git 2.55.0 ; répétitions 5
[1] .gitignore respecté : 2285 fichiers, 40.9 Mio, {'p50': 10575, 'p99': 125796, 'p99.9': 1250694, 'max': 1881634}
[1] liste par défaut seule (sans .gitignore) : 47736 fichiers, 4811.0 Mio, {'p50': 1973, 'p99': 2148387, 'p99.9': 15078443, 'max': 195780624} ; > 8 Mio : 119 ; > 64 Mio : 2
[2] workspace réel, sans profil : premier instantané 1451 ms ; sans changement [43, 42, 44, 41, 42] ms (médiane 42) ; dépôt shadow 19700 Kio
[2] workspace réel, avec profil : premier instantané 1234 ms ; sans changement [66, 63, 64, 63, 64] ms (médiane 64) ; dépôt shadow 19700 Kio
[3] copie, sans profil, rép. 1 : instantané début 47 ms ; fin 103 ms ; contrôle 69 ms (1 fichier(s) hors agent) ; aperçu 12 ms (81 lignes) ; restauration 45 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, sans profil, rép. 2 : instantané début 61 ms ; fin 109 ms ; contrôle 80 ms (1 fichier(s) hors agent) ; aperçu 12 ms (80 lignes) ; restauration 47 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, sans profil, rép. 3 : instantané début 72 ms ; fin 127 ms ; contrôle 64 ms (1 fichier(s) hors agent) ; aperçu 14 ms (81 lignes) ; restauration 49 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, sans profil, rép. 4 : instantané début 60 ms ; fin 115 ms ; contrôle 80 ms (1 fichier(s) hors agent) ; aperçu 12 ms (81 lignes) ; restauration 55 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, sans profil, rép. 5 : instantané début 70 ms ; fin 118 ms ; contrôle 90 ms (1 fichier(s) hors agent) ; aperçu 12 ms (81 lignes) ; restauration 50 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, sans profil : premier instantané 1187 ms ; dépôt shadow 23176 Kio
[3] copie, avec profil, rép. 1 : instantané début 67 ms ; fin 130 ms ; contrôle 102 ms (1 fichier(s) hors agent) ; aperçu 17 ms (81 lignes) ; restauration 49 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, avec profil, rép. 2 : instantané début 85 ms ; fin 135 ms ; contrôle 79 ms (1 fichier(s) hors agent) ; aperçu 17 ms (80 lignes) ; restauration 47 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, avec profil, rép. 3 : instantané début 82 ms ; fin 136 ms ; contrôle 108 ms (1 fichier(s) hors agent) ; aperçu 17 ms (81 lignes) ; restauration 51 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, avec profil, rép. 4 : instantané début 95 ms ; fin 142 ms ; contrôle 80 ms (1 fichier(s) hors agent) ; aperçu 17 ms (81 lignes) ; restauration 47 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, avec profil, rép. 5 : instantané début 82 ms ; fin 135 ms ; contrôle 107 ms (1 fichier(s) hors agent) ; aperçu 16 ms (81 lignes) ; restauration 52 ms ; écart restant 0 (le fichier de l'utilisateur, non suivi au début, est retiré : [])
[3] copie, avec profil : premier instantané 1232 ms ; dépôt shadow 23248 Kio
[4] écriture de checkpoints/probe, variante kernel : code 1 touch: /Users/yusufyuss/.claude/jobs/3acd71ae/tmp/ya43ws.uhIT/agent/checkpoints/probe: Operation not permitted
[4] écriture de checkpoints/probe, variante git des instantanés : code 0 
[4] faux dossier agent dans le workspace, non exclu : add code 128 "fatal: unable to stat '.yusagent/auth.json': Operation not permitted" ; secret dans les objets : False ; index : []
[4] faux dossier agent dans le workspace, exclu : add code 0 '' ; secret dans les objets : False ; index : ['code.py']
[5] nouvelle session, essai 1 : index copié 17 ms ; index neuf (objets déjà dans le dépôt) 186 ms
[5] nouvelle session, essai 2 : index copié 18 ms ; index neuf (objets déjà dans le dépôt) 164 ms
[5] nouvelle session, essai 3 : index copié 17 ms ; index neuf (objets déjà dans le dépôt) 164 ms
```

Le chemin `$D` (`mktemp -d` dans le dossier temporaire du job) apparaît dans la ligne `[4]`.
- **Workspace intact** : `git status --porcelain | wc -l` donne 6 avant et après. Les 6 lignes sont les 5 fichiers locaux non suivis de la passation et le script lui-même.
- **Ligne `[3]`, lecture** : « écart restant 0 » signifie que l'arbre après la restauration égale l'instantané de début. Le fichier créé par « l'utilisateur » après la fin du tour (`ya43_user_<r>.txt`) **est supprimé par la restauration**, parce qu'il n'était pas dans l'instantané de début. C'est le cas `force` (§P.5) : il reste récupérable dans la ref `avant-restauration`. Le libellé de la sortie est maladroit ; il sera corrigé si le script est relancé.

Inventaire complémentaire des dossiers ignorés (`find <d> -type f | wc -l` et `du -sh <d>`, même jour) :

| Dossier ignoré | Fichiers | Taille |
|---|---|---|
| `bench/smoke/runs` | 22 261 | 3,2 Go |
| `bench/tools/capture/runs` | 20 574 | 775 Mo |
| `extensions/vscode/node_modules` | 17 081 | 191 Mo |
| `extensions/vscode/.vscode-test` | 3 436 | 1,0 Go |
| `target` | 2 294 | 959 Mo |
| `prime-agent-runtime/.venv` | 1 684 | 37 Mo |

Six `.gitignore` sont suivis (`git ls-files '*.gitignore'`). Aucun dépôt imbriqué (`find . -path ./.git -prune -o -name .git -print` : vide).

**Ce que les mesures disent** :
- Avec `.gitignore` : 2 285 fichiers, 40,9 Mio, le plus gros à 1,9 Mo. Un tour complet (début, tour, fin, contrôle, aperçu, restauration) reste sous 500 ms avec le profil.
- **Sans `.gitignore`, la liste par défaut ne suffit pas** : 47 736 fichiers et 4,8 Go, à cause des sorties propres au projet (`bench/*/runs`, `.vscode-test`). Les plafonds sont indispensables.
- **Coût du profil** : +22 ms en médiane sur l'instantané sans changement (42 → 64 ms). Pas d'écart visible sur la restauration (45 à 55 ms sans le profil, 47 à 52 ms avec).
- **Premier instantané** : 1,2 à 1,5 s ici, environ 3 s sur 10 000 fichiers (annexe). **Copier l'index d'une autre session** ramène le premier instantané d'une nouvelle session à 17-18 ms, contre 164 à 186 ms pour un index neuf.

**Valeurs proposées** (Q-P3) :

| Réglage | Valeur | Justification |
|---|---|---|
| `.gitignore` | ceux du workspace, à tous les niveaux (git natif) | Réponse 6. Le `.git/info/exclude` du dépôt de l'utilisateur et l'`excludesFile` global ne sont pas lus (`core.excludesFile=/dev/null`) : le résultat ne dépend que du workspace |
| Liste par défaut | seulement si le workspace **n'a pas de `.gitignore` à sa racine** : `node_modules/ target/ .venv/ venv/ __pycache__/ .mypy_cache/ .pytest_cache/ .ruff_cache/ .tox/ .gradle/ .next/ .nuxt/ .cache/ dist/ build/ coverage/ .DS_Store *.pyc` | Couvre `node_modules`, `target` et `.venv` mesurés ici (environ 21 000 fichiers et 1,2 Go à eux trois) |
| Toujours exclus (`info/exclude` du dépôt shadow) | le dossier agent s'il est dans le workspace (chemin relatif), et le dossier `checkpoints` | Ligne `[4]` : sans exclusion, `git add` **échoue en entier** sur `auth.json` refusé par le profil |
| Par fichier | **8 Mio** : un fichier plus gros n'est ni capturé ni supprimé par une restauration. ~~L'aperçu le liste comme « non suivi (trop gros) »~~ **Corrigé (réponse Q-P3)** : l'aperçu le liste comme « non couvert » (`uncovered`) | Le plus gros fichier ici fait 1,9 Mo. Sans `.gitignore`, 119 fichiers dépassent 8 Mio. Les 64 Mio d'upstream coûteraient jusqu'à 64 Mio d'objets par tour pour un fichier modifié à chaque tour (estimation) |
| Nombre de fichiers candidats | **20 000** (borne d'upstream, deux fois le critère du CDC) : au-delà, instantanés désactivés pour ce workspace, avec un message | Ici 2 285. Sans `.gitignore`, 47 736 : désactivé, ce qui est voulu. Le compte se fait par `git ls-files -z --cached --others --exclude-standard` avant le premier instantané, coût mesuré au §P.6 |
| Taille totale | ~~pas de plafond à part~~ **Corrigé (réponse Q-P3) : 512 Mio** (borne d'upstream) ; au-delà, instantanés désactivés avec un message | Ici 41,0 Mio. Le compte demande la taille de chaque candidat : listage et `lstat` mesurés à 17 + 4 ms ici (§P.12), tenus à jour de façon incrémentale ensuite (§P.12) |
| Conservation | 30 jours (CDC) | §P.5 |

Le plafond par fichier se vérifie avant `git add` :
- les chemins changés viennent de git (`ls-files -o -m --exclude-standard -z`) ;
- le pont lit leur taille (`lstat`, métadonnées seulement, sans lire le contenu) ;
- ceux qui dépassent sont passés en `:(exclude)` à `git add`.

**Dépôts imbriqués** (réponse 6, limite à documenter) : git enregistre un dépôt imbriqué comme un *gitlink*, sans son contenu. Le plan :
- après chaque `add`, `git ls-files -s` repère les entrées `160000` ;
- il les retire de l'index (`rm --cached`) et les ajoute à `info/exclude` ;
- leur contenu n'est donc ni capturé ni restauré, et une restauration ne touche pas leur dossier. C'est le test T34.

**Liens symboliques** : `core.symlinks=true`. Git garde le texte du lien, pas sa cible. Un lien vers `auth.json` ne fait pas lire le secret (T27).

### P.4 Réponses prises et traduction au code

- **R3, instantanés dans le pont** : §P.2. Zéro ligne upstream pour les instantanés.
- **R4, un dépôt shadow par workspace, un index par session** :
  - dépôt nu `<dossier agent>/checkpoints/<16 premiers hex du SHA-256 du chemin canonique du workspace>/` ;
  - un fichier `workspace` y garde le chemin, pour la purge ;
  - index `ya-index/<id de session>` ;
  - **index d'une nouvelle session** : copie de l'index le plus récent du dépôt, s'il y en a un (17-18 ms au lieu de 164-186 ms, §P.3) ;
  - **refs** :
    - `refs/ya/<session>/debut/<rewindTo>` : l'instantané de début, sous la clé du point de retour ; `restore { to }` le trouve directement ;
    - `refs/ya/<session>/fin` : la dernière fin de tour, pour le contrôle ;
    - `refs/ya/<session>/avant-restauration/<n>`.
  - chaque ref vise un commit (`commit-tree`, auteur fixe `yusagent <checkpoints@yusagent.invalid>`, date du moment). La date de commit sert à la purge des 30 jours.
  - **Écart au §G.2, à valider (Q-P9)** :
    - le §G.2 prévoyait `refs/ya/<session>/<tour>/{debut,fin}` ;
    - ici, la clé est le `rewindTo`, et il n'y a qu'une seule ref `fin` ;
    - conséquence : après un retour en arrière, un prompt envoyé depuis le même point **remplace** l'instantané de ce point. L'ancien appartenait à la branche abandonnée, que l'extension retire et qu'on ne peut plus cliquer (`extensions/vscode/src/chat.ts:609-613`).
  - deux fenêtres sur le même workspace sont deux processus : index et refs séparés par session, objets partagés. Git sait écrire des objets en parallèle.
- **R5, sans git** :
  - le pont résout `git` dans son `PATH`, une fois ;
  - sous macOS, si c'est `/usr/bin/git` (le relais qui ouvre l'installateur des outils en ligne de commande), il lance d'abord `xcode-select -p` par `ya_guard`. En cas d'échec, les instantanés sont désactivés ;
  - message sur la sortie d'erreur du pont (onglet Output de l'extension), une fois par processus ;
  - et `available: false, reason` dans l'aperçu, que l'extension affiche ;
  - le retour de la conversation reste possible.
- **R6** : §P.3.
- **R8** : §P.5.
- **R10** : rouverte au §P.7.
- **R11** : §P.1.

### P.5 Profil : variante pour git, et refus d'écriture pour le kernel (réponse 8)

**Fichier du fork `crates/pa-types/src/platform/ya_guard.rs` seulement, zéro ligne upstream. Environ +17 −1 au total.** C'est sous le seuil de « quelques lignes » de la réponse 8 : pas d'arrêt.

1. `Variant` (`:37-42`), +4 :
   ```rust
       /// yusAgent (YA-18, ticket 43, answer 8): the checkpoints' `git`: the full profile with the
       /// checkpoints directory left writable; every other launch (the kernel included) is refused
       /// writing it.
       Checkpoints,
   ```
2. Constante, +2 :
   ```rust
   /// The checkpoints directory under the agent directory (ticket 43).
   pub const CHECKPOINTS_DIR: &str = "checkpoints";
   ```
3. Dans `profile` (`:278`), après le refus des fichiers d'identifiants (`:281-286`), +7 :
   ```rust
       if variant != Variant::Checkpoints {
           let _ = writeln!(
               text,
               "(deny file-write* (subpath {}))",
               quoted(&agent.join(CHECKPOINTS_DIR))
           );
       }
   ```
   `subpath` couvre le dossier lui-même : son renommage et sa suppression sont refusés. Le renommage du dossier agent est déjà refusé (`:290-294`).
4. `LaunchServices` refusé aussi à la variante : `:312`, **−1 +1** :
   ```rust
   -    if variant == Variant::Full {
   +    if variant != Variant::Browser {
   ```
5. Lanceur, +5 (après `tokio`, `:100-103`) :
   ```rust
   /// The checkpoints' `git` (ticket 43, answer 8): [`Variant::Checkpoints`].
   #[must_use]
   pub fn checkpoints_tokio(program: impl AsRef<OsStr>) -> tokio::process::Command {
       tokio::process::Command::from(wrapped(program.as_ref(), Variant::Checkpoints, false))
   }
   ```

Effets :
- le kernel, `exec_command`, le navigateur et tous les autres lancements gardent les profils `Full` ou `Browser`, qui refusent l'écriture de `checkpoints` ;
- seul le git des instantanés, lancé par le pont, l'obtient ;
- le pont n'est pas lui-même sous l'enveloppe : il est lancé par l'extension ;
- le scan des lancements (`crates/pa-core/tests/ya_process_launch_scan.rs`) reconnaît `ya_guard::` sans nouvelle exception ;
- interrupteur `off` : git est lancé sans profil, comme tout le reste (`wrapped`, `:165-168`).

La ligne `[4]` du §P.3 montre le mécanisme : `touch` dans `checkpoints` est refusé sous la variante « kernel » (code 1) et passe sous la variante « git » (code 0). La lecture reste permise au kernel : le dépôt ne contient que des fichiers du workspace, et la réponse 8 ne porte que sur l'écriture.

### P.6 Restauration, aperçu, contrôle « modifié hors de l'agent », bench des 2 s

**Méthodes** (noms du §G.2), servies par un nouveau module du fork, `crates/pa-daemon/src/acp/ya_turns/checkpoints.rs`, déclaré dans `ya_turns.rs` comme `steer` et `rewind` (`:32-34`) :

- **`_yus/session/restore_preview { sessionId, to, path? }`** :
  - **sans `path`** : `{ available, reason?, files: [{ path, status }], modifiedOutside: [path], uncovered: [path] }` (~~`skipped`~~ renommé, réponse Q-P3) ;
    - un instantané temporaire `T` de l'état actuel (le même que `avant-restauration`, non gardé) ;
    - `files` = `diff-tree -r --name-status T <debut>` : ce que la restauration changerait. `M` modifié, `A` recréé, `D` supprimé ;
    - `modifiedOutside` = `diff-tree -r --name-only <fin> T` ;
    - `uncovered` (« non couverts ») = les fichiers au-delà du plafond par fichier, et les dépôts imbriqués ;
    - sans instantané pour `to` (tour d'un binaire antérieur, ou feuille illisible) : `available: false`, avec la raison ;
  - **avec `path`** : `{ before: <texte> | null, binary }` par `cat-file blob <debut>:<path>`, sous la variante. L'extension ouvre `vscode.diff` entre ce texte et le fichier sur disque. Elle réutilise `DiffDocuments` et `showDiff` (`extensions/vscode/src/ya/diff-files-vscode.ts:15-36`, `:64-72`, ticket 24).
- **`_yus/session/restore { sessionId, to, files, conversation, force? }`** :
  1. Refus repris de `_yus/session/rewind` (`crates/pa-daemon/src/acp/ya_turns/rewind.rs`), mêmes textes :
     - session inconnue ;
     - un tour en cours ;
     - une autre fenêtre (réponse 9).
     
     Si `files` est demandé sans instantané : `-32602`, `data.details` « no snapshot for {to} ». Si les instantanés sont désactivés : `-32603`, avec la raison.
  2. Si `files` :
     - dans la file du workspace, instantané `avant-restauration/<n>` (gardé 30 jours : c'est l'annulation possible de la restauration). **Rangement (réponse Q-P6)** : ref `refs/ya/<session>/avant-restauration/<n>` du dépôt shadow `<dossier agent>/checkpoints/<empreinte>/`, `n` croissant par session, pris dans la file du workspace juste avant **chaque** restauration, même sans `force` ;
     - si `modifiedOutside` n'est pas vide et que `force` est absent : refus `-32603`, « Files changed outside the agent since its last turn. », `data.modifiedOutside` ;
     - sinon `read-tree -u --reset <debut>` avec l'index de la session ;
     - les fichiers ignorés, hors plafond ou non suivis au moment du contrôle ne sont pas touchés : `--reset` ne retire que ce qui est dans l'index.
  3. Si `conversation` : action `rewind { to, files }` du worker. Le texte D2 prend la phrase validée au §D, « Workspace files were restored to how they were at that point. », quand `files` a réussi.
  4. **Fichiers seuls** : nouvelle action `restored` du worker, qui range un texte pour le modèle. Proposition, Q-P5 :
     > `[workspace-restored]` The user restored the workspace files to how they were before an earlier turn; files may no longer match what this conversation shows.
  5. **Ordre** : les fichiers d'abord, puis la conversation. Si le retour de la conversation échoue après la restauration des fichiers (course avec un réveil), le pont range le texte « fichiers seuls », pour que le modèle sache que les fichiers ont changé. La réponse porte l'erreur avec `data.filesRestored: true`.
  6. Réponse : `{ restored: [path], uncovered: [path] }`.
- **Contrôle « modifié hors de l'agent »** : il compare l'état actuel à la **dernière fin de tour**, celle de n'importe quel tour (§P.2). La ligne `[3]` le mesure à 64 à 108 ms (1 fichier détecté sur 1).

~~**Aiguillage : +4 lignes upstream**~~ **Corrigé (réponse Q-P1) : zéro ligne upstream de plus**, le bras de `rewind` devient une garde (§P.11, §P.12.1). Proposition initiale, abandonnée : +4 lignes dans `crates/pa-daemon/src/acp/daemon.rs`, après le bras de `rewind` (`:1175-1178`), sur le modèle de `ya_history` (`:1149`) :
```rust
        // yusAgent (YA-18, ticket 43): workspace snapshots, preview and restore.
        other if super::ya_turns::checkpoints::serves(other) => {
            super::ya_turns::checkpoints::serve(other, &id, &params, link, state, &tx).await;
        }
```
~~Variante à zéro ligne (Q-P1) : des paramètres facultatifs `files`, `force` et `preview` sur `_yus/session/rewind`, déjà aiguillé. C'est moins clair pour l'extension, et ça s'écarte des noms du §G.2.~~

**Nettoyage (30 jours)**, dans le pont, sans ligne upstream :
- au premier travail de la file d'un processus, une tâche de fond parcourt `<dossier agent>/checkpoints/*` ;
- elle supprime les refs dont la date de commit a plus de 30 jours ;
- elle supprime le dossier entier si le workspace noté n'existe plus, ou s'il ne reste aucune ref ;
- puis `gc --prune=now --quiet` si des refs sont parties (`gc.auto=0` partout ailleurs) ;
- la suppression d'une conversation (`_yus/sessions/delete`, fork, `crates/pa-daemon/src/acp/ya_history.rs:574-584`, `Bound` a `actual_cwd`) supprime ses refs et son index.

**Bench des 2 s sur le binaire** (test 16, critère du CDC), nouveau `bench/tools/capture/ya43_restore_bench.py`, sur le modèle de `ya43_control.py` :
- vrai moteur, vrai kernel ; le modèle est `server.py`, avec un nouveau scénario `ya43-restore` : le prompt `EDIT <k>` donne une cellule qui modifie 50 fichiers, en crée 20 et en supprime 10 ; `EDIT-MANY` en modifie 5 000 ;
- workspace de 10 000 fichiers (générés comme à l'annexe), dans un dépôt git de l'utilisateur, avec un commit, une branche et un stash ;
- 5 répétitions : prompt, `restore_preview`, puis `restore { files: true, conversation: true }`. Chronométré côté client, de l'envoi à la réponse ;
- **conforme si** :
  - chaque restauration (le maximum, pas la médiane) prend moins de 2 s, aussi pour `EDIT-MANY` ;
  - les fichiers sont identiques à l'état d'avant le prompt (empreinte de chaque fichier) ;
  - dans le dépôt de l'utilisateur, `rev-list --all --count`, `for-each-ref`, `stash list` et `status --porcelain` sont identiques à leur état d'avant le prompt ;
- **coût de l'instantané de début** : blocs alternés `main` et branche, comme `ya43_rewindto_cost.py`. Délai côté client entre l'envoi de `session/prompt` et l'arrivée de la première requête au serveur. Proposition de seuil, Q-P2 : surcoût médian inférieur à 150 ms sur 10 000 fichiers (git seul, annexe : 91 à 130 ms sans changement). Le premier prompt d'un workspace neuf est mesuré à part ;
- toujours `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`. Lancé par Claude Code sur le binaire de la branche, commande et sortie au ticket (règle 13).

### P.7 Feuille non inscrite avant le prompt suivant (réponse 10, rouverte)

**Pourquoi la rouvrir** : avec la PR 3, le défaut s'aggrave. Après une restauration des fichiers et de la conversation, une reprise avant le prompt suivant (« Relancer », démon redémarré) revient à l'ancienne branche (§A.2), alors que les fichiers sont au point de retour. Le modèle voit des tours qui ont modifié des fichiers revenus en arrière, sans aucun texte pour le dire.

| Option | Effet | Coût |
|---|---|---|
| **L-a** garder (Unharnessed) | Défaut ci-dessus | Aucun |
| **L-b** (recommandée) inscrire D2 au retour | L'action `rewind` écrit **tout de suite** les lignes custom (annonces `ya-resources-changed` remises, puis D2) **dans le fichier**, comme enfants de `to`. Même forme que `append_custom_message` (`crates/pa-daemon/src/session_custom.rs:17-31`) et `/refine`, mais **sans diffusion d'événement** : pas d'annonce remontrée par `ya_notice`. Puis le contexte est reconstruit depuis la branche, qui les contient. La feuille est inscrite : c'est la dernière ligne écrite | Fichiers du fork seulement (`ya_permissions/rewind.rs`, `ya_history.rs`). La forme du fichier ne change pas : les lignes `custom_message` existent déjà |
| L-c `append_branch_summary` d'upstream | Inscrit la feuille avec un résumé de branche | Change ce que reçoit le modèle, et demande un résumé : écarté |

**Détail de L-b, et écarts au code de la PR 1, à valider** :
- Les lignes ne passent plus par `pending_next_turn` ni par la boîte du moteur (`ya_queue_rewind_rows`, `ya_permissions/rewind.rs:141-147`, `:168-170`). Elles sont dans la branche avant `rebuild_session_context`. T5a montre qu'une ligne custom de la branche atteint la requête (§I.5). Les tests T5 et T7 vérifient qu'il n'y a pas de doublon.
- Un retour vers une ligne D2 (deux retours depuis le même point) n'écrit rien de plus : `branch_to` seul, un seul texte.
- **Rejeu de `session/load`** : une ligne custom finale, sans prompt après, ouvre un tour de réveil (`ya_history.rs:270-276`). Le rejeu ignorera les lignes `ya-conversation-rewound` et `ya-workspace-restored` (fichier du fork). C'est le test T30.
- Le texte est écrit au retour, et non plus au prompt suivant. Un retour sans prompt après laisse donc D2 en fin de branche.

### P.8 Tests, rouges sur leur assertion avant le code

Principe (leçon du §L) : tester ce qui est **écrit sur disque** (fichiers du workspace, objets et refs du dépôt shadow, fichier de session) et ce qui est **envoyé au modèle** (corps gardés par `ModelServer`, `crates/pa-cli/tests/ya_acp_rewind_e2e.rs`). Les tests sont écrits contre l'interface ACP et le profil existants, pour qu'ils **compilent avant le code et soient rouges sur leur assertion** (`-32601`, dossier absent, `ok` au lieu de `denied`). Ils sont poussés seuls d'abord, comme au push 9a.

Rust :
- **T19** texte du profil (`crates/pa-types/tests/ya_secret_guard_profile.rs`) : `Full` et `Browser` contiennent `(deny file-write* (subpath ".../checkpoints"))`. Rouge : absent. La partie `Variant::Checkpoints` (pas de refus, mais les identifiants et `LaunchServices` refusés) vient avec le code : elle ne compile pas avant.
- **T19b** kernel réel (`crates/pa-core/tests/ya_secret_guard_kernel.rs`, CI macOS) : la cellule écrit `<agent>/checkpoints/x`. Rouge : `ok` au lieu de `denied`.
- Nouveau `crates/pa-cli/tests/ya_acp_checkpoints_e2e.rs` (vrai binaire, `ModelServer` dont la réponse contient une cellule `python_repl`) :
  - **T20** disque : après un prompt dont la cellule écrit, `refs/ya/<session>/debut/<rewindTo>` vise l'arbre d'avant le prompt, et `fin` celui d'après. Le test lit par `git ls-tree`. Rouge : dossier absent.
  - **T21** (test 10 du §G.5) : écritures par `open()`, `bash("echo x > f")` et `edit()`, plus une suppression. `restore { files: true, conversation: false }` : octets identiques à l'état d'avant, fichier supprimé revenu, fichier créé retiré. Un fichier ignoré et un fichier de 8 Mio + 1 octet ne sont pas touchés. Rouge : `-32601`.
  - **T22** (test 11) : un fichier écrit par le test après le tour figure dans `modifiedOutside`. `restore` sans `force` est refusé avec `data.modifiedOutside`. Avec `force` : restauré, et l'arbre de `avant-restauration/1` contient le fichier du test. Rouge.
  - **T23** modèle : `restore { files, conversation }` puis prompt. La requête reçue contient la branche jusqu'à `to`, puis D2 **avec** la phrase des fichiers, une fois, puis le prompt. Fichiers seuls : `[workspace-restored]` une fois. Prompt système et tools identiques à la requête d'avant. Rouge.
  - **T24** (test 9, critère 1) : le workspace est un dépôt git avec deux commits, une branche et un stash. Après deux prompts et une restauration : `rev-list --all --count`, `for-each-ref`, `stash list` identiques, et `status --porcelain` égal à celui d'avant le premier prompt. Rouge : `-32601`.
  - **T25** (test 12) : T21 dans un dossier sans `.git`. Rouge.
  - **T26** (test 13) : `PATH` sans git. L'aperçu donne `available: false` avec la raison, et `_yus/session/rewind` passe toujours. Rouge : `-32601`.
  - **T27** (test 14) : le dossier agent du test est **dans** le workspace (variable du dossier agent), avec un `auth.json` sentinelle et un lien symbolique vers lui. Les instantanés réussissent ; aucun objet ne contient la sentinelle (`cat-file --batch-all-objects --batch`) ; une restauration n'écrit pas `auth.json`. Rouge.
  - **T28** (test 15) : refs antidatées de 31 jours (`GIT_COMMITTER_DATE`) supprimées, refs récentes gardées, dossier d'un workspace disparu supprimé. Rouge.
  - **T29** : `_yus/sessions/delete` supprime les refs et l'index de la session. Rouge.
  - **T30** (L-b) : retour en arrière, puis `session/load` par un autre processus sans prompt entre les deux. Le rejeu montre la nouvelle branche sans tour de réveil, et la requête suivante porte D2 une fois. Rouge aujourd'hui : ancienne branche.
  - **T31** plafonds : un workspace de 20 001 fichiers candidats donne `available: false`. Rouge.
  - **T32** config isolée : `.gitattributes` `* filter=evil`, `HOME` du test avec un `.gitconfig` où `filter.evil.clean` crée un témoin, et un `.git/config` du projet avec `core.fsmonitor` vers un script témoin. Après un prompt : l'instantané existe et aucun témoin n'est créé. Rouge : pas d'instantané.
  - **T33** refus : pendant un tour, avec une autre fenêtre ; textes de `rewind`. Rouge : `-32601`.
  - **T34** dépôt imbriqué : `sub/.git` avec un fichier. Le fichier n'est pas dans l'arbre, et une restauration ne touche pas `sub/`. Rouge.
- Unitaires du module (file série, clé `rewindTo`, plafond, liste par défaut seulement sans `.gitignore` racine) : `scripted_link`, comme `rewind.rs`.

Extension :
- **X1** `features().restore` vrai quand les deux méthodes sont servies. Le drapeau est ajouté à `Features` et `NO_FEATURES` (`extensions/vscode/src/protocol.ts:527-536`, `webview/ya-features.tsx:6-14`).
- **X2** (agent factice) : clic sur un vrai `<button>` « Restore to here » sous le message. `userClick` ne vise que les `<button>` (`webview/ya-probe.ts:104-107`). Puis `restore_preview`, puis la boîte de confirmation qui liste `modifiedOutside` et propose « fichiers / conversation / les deux ». Puis `_yus/session/restore` avec les bons paramètres, et `vscode.diff` ouvert par fichier. Rouge : méthode refusée sur place.
- **X3** (e2e vrai binaire) : trois prompts qui écrivent des fichiers, restauration au deuxième. Fichiers sur disque identiques, et requête au modèle conforme à T23.

Capture : `compare.py --all` contre `main` donne `TOUT IDENTIQUE` (les instantanés ne touchent pas le préfixe), et `compare_frames.py` montre les trames inchangées.

### P.9 Fichiers

| Fichier | Statut | Changement |
|---|---|---|
| `crates/pa-types/src/platform/ya_guard.rs` | fork | §P.5, environ +17 −1 |
| `crates/pa-daemon/src/acp/daemon.rs` | **upstream** | ~~**+4**~~ **Corrigé (Q-P1)** : 2 lignes du fork (bras de la PR 1) modifiées, aucune ligne upstream de plus (§P.12.1) |
| `crates/pa-daemon/src/acp/ya_turns.rs` | fork | `mod checkpoints;` ; instantané de début dans `publish_prompt_start`, de fin dans `close` |
| `crates/pa-daemon/src/acp/ya_turns/checkpoints.rs` et `checkpoints/{git,refs,restore}.rs` | nouveaux | file série, lancement de git, refs, purge, méthodes |
| `crates/pa-daemon/src/acp/ya_turns/rewind.rs` | fork | contexte `{ leafId, sessionId, cwd }` lu par `leaf` |
| `crates/pa-daemon/src/ya_permissions/rewind.rs` | fork | `leaf` étendu ; `rewind { to, files }` ; action `restored` ; L-b |
| `crates/pa-core/src/session_engine/ya_resource_notice/rewind.rs` | fork | type et texte `ya-workspace-restored`, phrase des fichiers de D2 |
| `crates/pa-daemon/src/acp/ya_history.rs` | fork | suppression des refs avec la conversation ; rejeu sans les lignes D2 (L-b) |
| `crates/pa-types/tests/ya_secret_guard_profile.rs`, `crates/pa-core/tests/ya_secret_guard_kernel.rs`, `crates/pa-cli/tests/ya_acp_checkpoints_e2e.rs` | fork / nouveau | §P.8 |
| `extensions/vscode/src/{ya/methods.ts,protocol.ts,chat.ts,view.ts}`, `webview/{Chat.tsx,ya-features.tsx}`, `test/fake-agent/fake-agent.mjs`, tests | fork | bouton, aperçu, confirmation, choix, diff |
| `bench/tools/capture/ya43_restore_bench.py`, `server.py` (scénario `ya43-restore`) | nouveau / fork | §P.6 |
| `docs/fork-changes.md`, `docs/usage.md` si besoin | doc | une ligne par fichier, lignes upstream numérotées |

~~**Lignes upstream : +4** dans `daemon.rs` (ou 0, Q-P1).~~ **Corrigé (Q-P1)** : aucune ligne upstream de plus ; 2 lignes du fork modifiées dans `daemon.rs` (total du fichier depuis la PR 1 inchangé, +5 −1). Rien dans la couche providers, rien dans le worker upstream.

**Préfixe** : prompt système, tools et `yaRequest` inchangés. Rien de volatil n'entre dans le prompt (ni chemin, ni date, ni état git). Les seuls textes nouveaux arrivent en fin d'historique, après une restauration (D2 avec sa phrase, ou `[workspace-restored]`).

### P.10 Questions

- **Q-P1** : aiguillage des deux méthodes `_yus/session/restore` et `restore_preview` (+4 lignes upstream, recommandé), ou paramètres en plus sur `_yus/session/rewind` (0 ligne) ?
- **Q-P2** : instantané de début **attendu avant l'envoi du prompt au worker** (couvre la première cellule à coup sûr), avec un seuil de surcoût médian inférieur à 150 ms sur 10 000 fichiers mesuré sur le binaire ? Ou lancé en parallèle de l'appel au modèle, avec une course possible sur la première cellule (seule voie sans attente sans toucher le worker, réponse 3) ?
- **Q-P3** : valeurs du §P.3 :
  - 8 Mio par fichier ;
  - 20 000 fichiers candidats, au-delà de quoi les instantanés sont désactivés pour ce workspace ;
  - la liste par défaut seulement sans `.gitignore` à la racine ;
  - pas de plafond de taille totale, ou les 512 Mio d'upstream ?
- **Q-P4** : réponse 10 rouverte : **L-b** (D2 inscrit au retour, recommandé) ou L-a ? L-b change le code de la PR 1 (fichiers du fork seulement, §P.7).
- **Q-P5** : texte pour une restauration des fichiers seuls : « `[workspace-restored]` The user restored the workspace files to how they were before an earlier turn; files may no longer match what this conversation shows. » ?
- **Q-P6** : `force` écrase les changements faits hors de l'agent. Ils restent 30 jours dans `avant-restauration/<n>`. Faut-il un « annuler la restauration » dans l'extension dès cette PR (non recommandé : PR suivante si besoin), ou une simple mention dans la boîte de confirmation ?
- **Q-P7** : premier instantané d'un workspace au premier prompt (1,2 à 1,5 s ici, environ 3 s sur 10 000 fichiers, une seule fois par workspace grâce à la copie d'index) : accepté ? L'alternative, le lancer à l'ouverture de la session, demande une accroche dans `handle_session_new` (upstream).
- **Q-P8** : une seule PR (pont et extension, comme la PR 1), ou 3a (pont) puis 3b (extension) ?
- **Q-P9** : écart au §G.2 : refs sous la clé du `rewindTo`, une seule ref `fin`, et l'instantané d'un point remplacé par un nouveau prompt depuis ce point (§P.4) ?

### P.11 Réponses de Yusuf au §P.10 (27/11/2026)

- **Q-P1 : zéro ligne upstream de plus si possible.** Le bras `_yus/session/rewind` de `daemon.rs` (lignes du fork, PR 1) devient une garde qui couvre les trois méthodes ; l'aiguillage se fait dans le module du fork. Sinon, +4 accepté. **Résultat : la garde tient, zéro ligne upstream de plus** (détail ci-dessous).
- **Q-P2 : instantané de début attendu avant l'envoi du prompt.** Surcoût mesuré au bench (médiane et p95), seuil de 150 ms en médiane sur 10 000 fichiers.
- **Q-P3** :
  - 8 Mio par fichier ;
  - 20 000 fichiers ;
  - liste par défaut seulement sans `.gitignore` à la racine ;
  - **plafond total de 512 Mio** ;
  - au-delà d'un plafond de workspace : instantanés désactivés avec un message ;
  - fichiers exclus par le plafond par fichier : listés comme **« non couverts »** dans l'aperçu.
  
  §P.3 et §P.6 corrigés.
- **Q-P4 : L-b**, avec ces tests : D2 une seule fois dans la requête au modèle, en session vivante **et** après reprise ; préfixe intact (`compare.py`).
- **Q-P5 : texte validé** : « `[workspace-restored]` The user restored the workspace files to how they were before an earlier turn; files may no longer match what this conversation shows. »
- **Q-P6 : mention dans la boîte de confirmation** (3b), à condition qu'un instantané de l'état courant soit pris juste avant chaque restauration. **Rangement** : ref `refs/ya/<session>/avant-restauration/<n>` du dépôt shadow `<dossier agent>/checkpoints/<empreinte du workspace>/`, gardée 30 jours, prise dans la file du workspace avant chaque `read-tree`, avec ou sans `force` (§P.6, corrigé). « Annuler la restauration » : ticket à part.
- **Q-P7 : accepté** (premier instantané payé au premier prompt, une fois par workspace).
- **Q-P8 : 3a (pont) puis 3b (extension).** Contrôle manuel à 3b.
- **Q-P9 : accepté.** Écart au §G.2 noté : refs sous la clé du `rewindTo`, une seule ref `fin`, et l'instantané d'un point remplacé par un nouveau prompt depuis ce point (§P.4).

**Q-P1, la garde** (`crates/pa-daemon/src/acp/daemon.rs:1175-1178`, lignes de la PR 1) :
```rust
        // yusAgent (YA-18, ticket 43): going back to before a prompt.
-        super::ya_turns::rewind::METHOD => {
-            super::ya_turns::rewind::serve(&id, &params, link, state, &tx).await;
+        other if super::ya_turns::rewind::serves(other) => {
+            super::ya_turns::rewind::serve(other, &id, &params, link, state, &tx).await;
         }
```
- La deuxième ligne fait 88 caractères : rustfmt la garde sur une ligne (largeur 100).
- La garde reste à la même place, avant `"session/set_config_option"`. Les gardes placées avant elle sont des listes exactes de méthodes, et aucune ne contient `_yus/session/restore*` :
  - `ya_history::serves` : `session/list` et la suppression (`ya_history.rs:582-584`) ;
  - `ya_resources::serves` : les constantes des skills, du MCP et des réglages (`ya_resources.rs:58`) ;
  - `ya_providers::serves` : `REQUESTS.contains` (`daemon/ya_providers.rs:168-170`).
- `rewind::serves` couvre `_yus/session/rewind`, `_yus/session/restore` et `_yus/session/restore_preview`. `rewind::serve(method, …)` aiguille les deux dernières vers `ya_turns/checkpoints.rs`.
- Le commentaire de `:1175` n'est pas touché.
- **Bilan** : dans `daemon.rs`, deux lignes du fork modifiées. L'écart du fichier à upstream reste celui de la PR 1 (+5 −1 pour le ticket 43). `docs/fork-changes.md` (ligne du ticket 43 sur `daemon.rs`) est mis à jour en conséquence.

**`a3e261c27`, les faits demandés** :
- **Ce que le §P.1 voulait dire par « à reprendre »** : reprendre ses **options git** dans le module du fork des instantanés, pas cherry-picker le commit. Elles portent sur le git des gates autonomes, une autre fonction. Lignes exactes dans `a3e261c27:crates/pa-core/src/autonomous/gates.rs` :
  - `:293-299`, `SNAPSHOT_GIT_OVERRIDES` : `-c core.fsmonitor=false` (`:295`), `-c diff.autoRefreshIndex=false` (`:297`), `--no-optional-locks` (`:298`) ;
  - `:366` : `GIT_ALLOW_PROTOCOL=""` et `GIT_NO_LAZY_FETCH=1` ;
  - `:319-324`, propres au `diff` des gates : `--no-ext-diff`, `--no-textconv`, `--submodule=short`, `--ignore-submodules=none`. Sans objet ici : les instantanés ne lancent pas de `diff` vers le worktree, seulement `diff-tree` entre deux arbres du dépôt shadow, sans textconv puisque aucune config n'en déclare.
  
  Le module des instantanés reprend `core.fsmonitor=false`, `GIT_ALLOW_PROTOCOL=""` et `GIT_NO_LAZY_FETCH=1`, et ajoute `core.hooksPath=/dev/null`, `core.excludesFile=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null` et `env_clear()` (§P.1). `diff.autoRefreshIndex` et `--no-optional-locks` ne s'appliquent pas : les instantanés **écrivent** leur propre index, dans le dépôt shadow, et il n'y a aucun hook.
- **Le commit lui-même, s'il était cherry-pické** :
  - fichiers : `.changes/security-autonomous-gates-git-safe-path.md` (nouveau, +1) et `crates/pa-core/src/autonomous/gates.rs` (+304 −32 : +77 −32 hors du module de tests, +227 dans `mod tests`), mesuré par `git show --numstat a3e261c27` ;
  - il ne s'applique pas tel quel : `git show a3e261c27 | git apply --check` échoue à `gates.rs:393`, sur le seul hunk qui touche la ligne du fork du ticket 41 (`ya_guard::tokio(program)`, `gates.rs:403`). La résolution garde cette ligne et ajoute `.envs(envs.iter().copied())` ;
  - aucun commit d'upstream sur `gates.rs` entre la base du fork et lui (`git log c83ac68b3..a3e261c27 -- gates.rs` ne montre que lui). Un seul commit après (`2eebeb4f9`, dépendances, +14 −2 sur ce fichier), non requis.
- **Total upstream de la PR 3a** :
  - **sans le cherry-pick** (plan) : 0 ligne upstream de plus, 2 lignes du fork modifiées dans `daemon.rs` ;
  - **avec le cherry-pick** : en plus, le diff d'upstream lui-même dans `gates.rs` (+304 −32) et `.changes/…` (+1), qui **rapproche** le fork d'upstream au lieu de l'en écarter. L'écart propre au fork dans `gates.rs` reste 1 ligne (`ya_guard`).
  - **Recommandation** : pas dans 3a. Les instantanés n'en dépendent pas, et la règle 1 limite le périmètre au ticket. Cherry-pick dans un ticket à part, comme `20b4ad74b` (réponse 11). À trancher (Q-P10).

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
