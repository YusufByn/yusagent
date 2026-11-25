# Ticket 36 : abandon perdu entre la vérification du drapeau et l'enregistrement du run (constat et plan)

Demande de Yusuf du 25/11/2026. Branche `ya-36-constat`, depuis `main` à `75bdd0c10` (merge de la PR #85, ticket 35). **Constat et plan seulement, aucun code du produit**, aucun fichier upstream touché, rien compilé en local (règle 8), aucun appel de modèle.

Upstream lu à `31bd73967` (référence du ticket 35) et à la tête du jour, `e44e9ae9d` (`git fetch upstream` du 25/11/2026). Suite de l'option C du ticket 35 (`docs/tickets/35-regression-r3-abort-queued.md`, §7.1, §8, décisions du §10).

## Résumé

- **Upstream a déjà écrit ce correctif** : commit `5778afa02` (branche `upstream/fix/abort-lost-before-run-registration`, PR #3872 du 31/10/2026). Il relit le drapeau d'abandon quand le run est enregistré (à son `AgentStart`).
  - La PR a été **fermée sans merge** le 12/11/2026 : « Closing per the triage: the interrupt/abort semantics re-land with the async port's interrupt work ».
  - La série async est mergée (L1 à L5, R1 à R8, jusqu'à `e44e9ae9d`), **sans ce correctif** : `run_once.rs` d'`upstream/main` n'a aucune relecture à l'enregistrement, et aucun commit n'a touché ce fichier depuis `31bd73967` (§2).
- **Il s'applique tel quel sur notre `main`** : notre `run_once.rs` est le même blob (`7777bb36c`) que celui de sa base `c6d4da942`, `git apply --check` passe, `git merge-tree` est sans conflit (§3.3).
- **Sa limite** : la relecture se fait au premier `Pending` qui suit `AgentStart` (un `Notify`). Le Stop n'attend plus la réponse du provider, mais la requête peut déjà être partie. Son test est rouge par un délai de 10 s, pas par une assertion (§3.1, §4).
- **Le test du §7.1 sans horloge** demande que l'abandon arrive avant l'appel du provider. Il faut pour cela une variante où le run attend la relecture : 15 lignes ajoutées et 8 modifiées en plus de `5778afa02` dans `run_once.rs` (§3.2).
- **Nos deux échecs « instables » de `rlm_children_delete_e2e` ont la signature que la PR #3872 attribue à cette fenêtre** : 91,86 s et 92,28 s, soit 30 + 60 + ~2 s (§5). L'hypothèse notée dans `docs/fork-changes.md` (démarrage lent du kernel) est probablement fausse.
- **Correction du ticket 35, §11.3** : dans le démon, les continuations autonomes et d'objectif sont des livraisons séparées de la file du worker. Elles repassent par la comparaison du worker. Sur les chemins lus, la lecture du disque de R3 n'est plus dans la fenêtre du démon (§1.2).
- **RPC** : pas de drapeau d'abandon. Un abandon perdu n'est rattrapé par rien, le tour va à son terme (§1.3). **`-p`** : pas de commande Stop (§1.4). Ni `5778afa02` ni la variante ne couvrent ces deux chemins.

## 1. Où la fenêtre existe, et ce que voit l'utilisateur

### 1.1 La fenêtre

- **Vérification** : en tête du futur d'admission, `crates/pa-daemon/src/agent_engine/turn/run_once.rs:336-340` (`if aborted()` en `:337`), après celle de `crates/pa-daemon/src/agent_engine/turn/model.rs:93`.
- **Enregistrement du run** : `crates/pa-agent/src/agent.rs:603-611` (`*run = Some(ActiveRun …)` en `:608`), dans `run_with_lifecycle` (`:586`).
- **`Agent::abort` sans run** : ne fait rien (`agent.rs:1108-1112`).
- **Entre l'enregistrement et l'appel du provider**, dans le même `poll` sauf entrée-sortie réelle :
  - `AgentStart`, `TurnStart`, puis les messages du prompt (`crates/pa-agent/src/agent_loop/entry.rs:31-42`). Les abonnés sont attendus un par un (`agent.rs:430-433`) ;
  - vérifications du signal du run (`crates/pa-agent/src/agent_loop/run.rs:31-32`, `:45`, `:49` ; `crates/pa-agent/src/agent_loop/response.rs:85`) ;
  - appel du provider (`response.rs:151-155`). Le provider factice compte l'appel et consomme sa réponse scriptée dès cet appel (`crates/pa-ai/src/providers/faux/mod.rs:731-732`).
- Un abandon arrivé après `:337` et avant `agent.rs:608` lève le drapeau du worker, mais `Agent::abort` ne trouve rien. Rien ne relit le drapeau avant le premier événement transmis à `emit`.

### 1.2 Démon (worker)

| Chemin | Où | Fenêtre |
|---|---|---|
| **Première admission de chaque livraison** : prompt (`TurnPrompt::User`) ou ligne injectée (`TurnPrompt::Injected` : `[bash-done]`, suite autonome, suite d'objectif) | `run_once.rs:337` → `prompt_with_images` (`crates/pa-core/src/session_engine/admission.rs:79`) ou `prompt_injected_message` (`admission.rs:31`) → `agent.rs:608` | oui |
| **Reprises par `continue_run`** : tentatives suivantes du pilote de reprise (`first_attempt` à faux, `model.rs:356-358`), reprise après dépassement de fenêtre (`TurnAdmission::Continue`, `crates/pa-daemon/src/agent_engine/turn/run_loop.rs:57-64`) | `run_once.rs:337` → `agent.continue_run()` (`run_once.rs:391`, `agent.rs:1224`) → `run_with_lifecycle` | oui, sans code de pa-core (aucune admission) |

**La lecture du disque de R3 dans la fenêtre, après le ticket 35** :

- Première admission : `ya_announce_resources` prend le jeton levé par la comparaison du worker et ne lit rien (`crates/pa-core/src/session_engine/ya_resource_notice.rs:376-386`, appel du worker en `:394-400`).
- `continue_run` ne passe pas par l'admission de pa-core : pas d'appel de R3.
- **Continuations autonomes et d'objectif : ce ne sont pas des admissions dans le même `run_prompt`.**
  - `run_turns` ne reboucle que sur la reprise après dépassement (`run_loop.rs:98-101`, seul `continue` de la boucle).
  - La suite autonome est remise à la file du worker : `run_loop.rs:188-203`, `crates/pa-daemon/src/autonomous_continuation.rs:258-265`, puis le puits du worker (`crates/pa-daemon/src/worker.rs:467-475`), qui la pousse dans `follow_up` (`crates/pa-daemon/src/worker/queue.rs:428-447`).
  - La suite d'objectif passe par `goal_admission_sink` (`crates/pa-daemon/src/goal_continuation.rs:442-448`), installé par le worker (`worker.rs:567`).
  - Chacune est donc une livraison avec sa propre prise (`crates/pa-daemon/src/worker/turn.rs:69-81`, qui remet aussi le drapeau d'abandon à faux), sa propre comparaison (`turn.rs:1041`) et son jeton.
- **Conséquence pour le ticket 35** : le §11.3 (« continuations autonomes du démon : la lecture du disque reste dans la fenêtre ») et le cas résiduel des §11.1 et §13.2 (`continue_run` suivi d'une continuation autonome dans le même `run_prompt`) ne se produisent pas sur ce code. À corriger aussi dans le constat du ticket 37, qui reprend ce chemin (ticket 35, §13.3).
- **Ce qui reste dans la fenêtre** : le code d'upstream seul. Lecture de l'état de l'agent, ligne de digest (`admission.rs:44`), lignes du tour suivant (`:48`), routage d'image (`:54`), puis `agent.prompt`. Non audité en entier, non mesuré.

**Ce que voit l'utilisateur quand un Stop se perd** :

- **Le rattrapage** : la porte d'`emit` du worker rend `false` dès qu'`abort_requested` est levé (`turn.rs:513-523`). `run_once` passe alors `aborted` à vrai (`run_once.rs:402-403`) et appelle `agent.abort()` (`:427-432`).
- **Premier run d'une livraison** : le premier événement transmis est le début du message de l'assistant (`run_once.rs:164-178`). `AgentStart`, `TurnStart` et la ligne de l'utilisateur ne sont pas transmis (`:226-239`, `:248-257`). Ce début n'est émis qu'au premier événement du provider (`response.rs:175-182`).
  - **Le Stop est donc tenu jusqu'à ce que le provider commence à répondre**, et la requête est partie.
  - Avec le provider factice retenu 60 s : 60 s (ticket 35, §2.3).
  - Si le provider ne répond jamais : jusqu'à son délai d'expiration (non lu).
- **Reprises** : `boundary_passed` est déjà vrai (posé par l'`AgentEnd` ou le `TurnEnd` du run précédent, `run_once.rs:263`, `:279`). `AgentStart` est transmis, et le rattrapage a lieu au premier `Pending` après l'enregistrement. Déduit, non testé.
- **Extension VS Code** :
  - « Arrêter » envoie `session/cancel` (`extensions/vscode/src/chat.ts:304-312`). Un tour envoyé par l'utilisateur reste affiché en cours jusqu'à la réponse de `session/prompt` (`chat.ts:609-618`).
  - Côté ACP, `arm_cancel_locked` passe la session en `cancelling` (`crates/pa-daemon/src/acp/daemon.rs:859-893`), puis `stop_session_work` (`:959-981`) envoie `AbortAndClearQueue` (`crates/pa-daemon/src/worker/commands.rs:684-698`), puis `CancelPromptAdmission` avec `cancelOwned` (`daemon.rs:970-972`).
  - Ce second envoi rappelle `abort_in_flight_turn` si l'admission tourne encore (`crates/pa-daemon/src/prompt_admission.rs:586-597`). C'est une seconde chance si le run s'est enregistré entre-temps. Déduit, non mesuré.
  - Pendant `cancelling`, un second « Arrêter » est ignoré (`daemon.rs:867-869`), et un nouveau prompt est refusé (« ACP session is cancelling », `daemon.rs:826-828`).
  - L'utilisateur voit donc le tour continuer après « Arrêter », sans pouvoir réessayer, jusqu'au premier événement du provider. Le tour finit alors annulé.
- **TUI** :
  - Échap ou Ctrl+C (`crates/pa-tui/src/session_ui/apply.rs:676-695`) → `abort_turn` (`:22-58`) → `abort_and_send_queued` (`commands.rs:299-322`). Aucun message transitoire (`apply.rs:686-687`).
  - Le tour reste affiché en cours jusqu'au premier événement du provider.
  - Un second Échap renvoie l'abandon, qui agit si le run est enregistré entre-temps.

### 1.3 RPC (`pa-daemon --rpc`, sans démon)

- **Pas de drapeau** : `abort` appelle directement `agent.abort()` et suspend la pompe (`crates/pa-daemon/src/rpc/commands.rs:216-220`). Aucune relecture trouvée ensuite.
- **Les fenêtres** :
  - **commande `prompt`** : `prompt_with_images` avec `return_after_accepted` (`crates/pa-daemon/src/rpc/prompt_commands.rs:27-49`). La réponse part une fois le run enregistré. Les prompts sont chaînés, les autres commandes passent sans attendre (`crates/pa-daemon/src/rpc/mod.rs:238-252`, `:271-282`). Un `abort` envoyé avant la réponse du prompt peut donc tomber avant l'enregistrement. Cette fenêtre contient encore la lecture du disque de R3 (`admission.rs:148`) : le jeton n'est levé que par le worker du démon ;
  - **pompe de la file** : `pump_suspended` est lu en `commands.rs:166-171`, puis `continue_run` en `:175`. Un `abort` entre les deux est perdu ;
  - **suite d'une commande de session** : `prompt_injected_message` (`prompt_commands.rs:194-199`), avec la lecture de R3 (`admission.rs:47`).
- **Effet** : aucune porte d'`emit` comme celle du worker. **Le tour va à son terme**, outils compris.
- Les abandons des signaux SIGTERM et SIGHUP (`rpc/mod.rs:174-190`, `:195-213`) et du remplacement de session (`crates/pa-daemon/src/rpc/session.rs:321-324`) ont la même forme. Non suivis plus loin.

### 1.4 `-p`

- **Aucune commande Stop** : pas d'abandon dans `crates/pa-cli/src/print_runtime.rs` ni dans `print_goal.rs`, et aucun gestionnaire de SIGINT trouvé dans `crates/pa-cli/src` (`grep -rln "ctrl_c\|SignalKind::interrupt" crates/pa-cli/src` vide). Un SIGINT termine le processus.
- **Seul abandon** : l'élimination des sous-agents en ligne à la sortie, `dispose_child` (`crates/pa-cli/src/print_rlm.rs:590-606`, `agent().abort()` en `:601`).
  - Le pilote de l'enfant vérifie `is_disposed` (`print_rlm.rs:375`), puis lance `continue_run` (`:392-396`).
  - Un abandon perdu entre les deux fait attendre à `driver.await` (`:603`) tout le tour de l'enfant avant la sortie. Lu, non testé.

## 2. Upstream depuis `31bd73967`, et la réécriture de `be86884dd`

- **Depuis `31bd73967`** (`git log 31bd73967..upstream/main -- <fichier>`) :
  - aucun commit sur `run_once.rs`, `model.rs`, `agent_loop/run.rs`, `entry.rs`, `response.rs` ;
  - `agent.rs` : `cf91e5d2d` (async L4) et `c95d65c41` (async R6), 25 lignes ajoutées puis retirées, aucune ligne sur `abort`, `run_with_lifecycle`, `ActiveRun` ni `signal` (`git show <c> -- crates/pa-agent/src/agent.rs | grep '^[-+]' | grep -ci 'abort\|run_with_lifecycle\|ActiveRun\|fn signal'` : 0 et 0) ;
  - à `upstream/main` : `Agent::abort` ne fait toujours rien sans run (`agent.rs:1209-1213` chez eux). La vérification est toujours en tête du futur (`run_once.rs:347-352` chez eux).
- **`be86884dd`** (#4124, 21/11, absent de notre `main`) :
  - garde de fin de run dans `pa-agent` ;
  - `TurnSubscription` (drapeau `live`) dans `run_once.rs` ;
  - `AgentEnd { aborted }`.
  - **La fenêtre reste** : rien n'est relu après l'enregistrement.
  - Il ne se reprend pas seul : sur notre `main`, il est déjà en conflit sur 7 fichiers, dont 3 blocs dans `run_once.rs` (§3.3).
- **`6679f881d`** (#4073, absent chez nous) : un abandon avant le flux du provider rend `TurnOnce::Aborted` au lieu d'une erreur (`run_once.rs:460-466` chez eux). Il n'est **pas** nécessaire à la correction : la relecture pose `aborted`, et `run_once.rs:440-442` rend `Aborted` avant la lecture de l'erreur d'admission (`:448`). C'est un écart à part (§7).
- **`5778afa02`** (PR #3872, Sebastian Müller, 31/10) : le correctif de ce ticket.
  - **Le changement** : un `Notify` levé à `AgentStart`, puis une branche du `select!` qui relit le drapeau une fois. S'il est levé, la branche passe `aborted` à vrai, puis le chemin existant fait `agent.abort()` et attend l'admission.
  - Il renomme aussi `aborted` en `cancel_requested`, ajoute un test, modifie l'e2e `rlm_children_delete_e2e` et ajoute un fragment de changelog.
  - **Preuves données dans la PR** (non refaites ici) : leur test échoue 5 fois sur 5 sur `main` et passe 50 fois sur 50 corrigé. L'e2e de suppression a échoué 11 fois en CI du 22/10 au 30/10, binaire à environ 92 s.
  - **Fermée le 12/11** par un mainteneur, renvoyée au travail d'interruption du portage async. Ce travail est mergé sans elle.
- **PR ouverte #4042** (`upstream/sebastian/user-row-after-admission`, `6b66c7481`) : l'abonné transmet toute ligne de l'utilisateur admise (bras `User` de `run_once.rs`, nos lignes `:226-239`), et le worker n'écrit plus la ligne acceptée.
  - Si elle est mergée, la ligne de l'utilisateur du premier run atteindra la porte d'`emit`. Un abandon perdu serait alors rattrapé au premier `Pending` après l'enregistrement (déduit). Elle recouvre en partie `5778afa02`.
  - Zone voisine de nos lignes, sans recouvrement attendu (non simulé).

**Réponse** : la correction **se reprend de chez eux**, par `git cherry-pick -x 5778afa02`, sans conflit. Il ne faut écrire en plus que la variante du §3.2, si on veut que l'abandon arrive avant la requête au provider et que le test du §7.1 soit sans horloge.

## 3. Correction proposée

### 3.1 C-a : `git cherry-pick -x 5778afa02` (code d'upstream, tel quel)

Lignes de notre `main`, issues des en-têtes de hunk (`git show 5778afa02 -- <fichier> | grep '^@@'`, `git show 5778afa02 --numstat`) :

| Fichier (upstream) | Lignes | Ce qui change |
|---|---|---|
| `crates/pa-daemon/src/agent_engine/turn/run_once.rs` | **+20 / −5** | `:78` : `aborted` renommé `cancel_requested` ; après `:89` : `Notify` (+3) ; après `:91` et `:103` : son clone (+1, +1) ; après `:253` : `run_registered.notify_one()` à `AgentStart` (+1) ; `:332-334` : commentaire (3 → 4 lignes) ; `:337` : renommage ; après `:396` : `registration_consulted` (+1) ; après `:398` : branche du `select!` (+7) |
| `crates/pa-daemon/src/agent_engine/turn/model.rs` | +2 / −4 | `:89-92` : commentaire |
| `crates/pa-daemon/src/agent_engine/tests/abort.rs` | +59 | après `:236` : `abort_flagged_before_the_run_registers_cancels_the_registered_run` |
| `crates/pa-daemon/tests/rlm_children_delete_e2e.rs` | +14 / −31 | le reçu de la cellule est lu une fois le tour terminé, sans sondage de 60 s ; trace de la cellule en cas d'échec |
| `.changes/abort-before-run-registration.md` | +1 (nouveau) | fragment d'upstream |

Pour une PR sur `upstream/main` (`e44e9ae9d`), mêmes ancres, décalées : paramètre `:77`, `let boundary_passed` `:88` (suivi de leur `let live` en `:89`), `let tx = tx.clone()` `:92` et `:105`, `AgentStart` `:258`, commentaire `:344-346`, vérification `:349`, `let mut settled` `:408`, `select!` `:410`, commentaire de `model.rs` `:95-98`.

**Ce que C-a garantit** : un abandon tombé dans la fenêtre est vu au premier `Pending` de l'admission après `AgentStart`. Le Stop n'attend plus la réponse du provider.

**Ce qu'il ne garantit pas** : que la requête ne parte pas. Le `Notify` ne fait que réveiller la tâche. Tant que l'admission ne rend pas `Pending`, elle continue dans le même `poll` jusqu'à l'appel du provider (§1.1). Dans leur test, la relecture a lieu pendant l'attente de 60 s du provider factice, après l'appel.

### 3.2 C-b : la relecture attendue, au-dessus de C-a (à écrire, fork)

- **Le changement** : à `AgentStart`, l'abonné envoie un `oneshot` dans un canal et attend la réponse. La branche du `select!` relit le drapeau, appelle `agent.abort()` s'il est levé, **puis** répond.
  - Le run reprend avec son signal déjà annulé. `run.rs:31-32` ou `:45` lève l'erreur d'abandon avant l'appel du provider.
  - `handle_run_failure` émet la ligne de l'assistant annulée (`agent.rs:436` et suivantes). `run_once` rend `Aborted` par `aborted` (`run_once.rs:440-442`).
- **Diff exact par rapport à C-a** : `run_once.rs`, **+15 / −8**, soit **+27 / −5** au total contre notre `main`. Rédigé dans le scratchpad de la tâche, **non compilé** :

```diff
-        // re-consult the cancel flag.
-        let run_registered = std::sync::Arc::new(tokio::sync::Notify::new());
+        // re-consult the cancel flag; the run waits for that consult.
+        let (run_registered, mut registration_consults) =
+            tokio::sync::mpsc::unbounded_channel::<tokio::sync::oneshot::Sender<()>>();
 ...
-            let run_registered = std::sync::Arc::clone(&run_registered);
+            let run_registered = run_registered.clone();
 ...  (même changement pour le clone par événement)
                             AgentEvent::AgentStart => {
-                                run_registered.notify_one();
+                                // Before the provider call: wait out the consult.
+                                let (consulted, consult) = tokio::sync::oneshot::channel();
+                                if run_registered.send(consulted).is_ok() {
+                                    let _ = consult.await;
+                                }
 ...
-        let mut registration_consulted = false;
 ...
-                () = run_registered.notified(), if !registration_consulted => {
-                    registration_consulted = true;
+                Some(consulted) = registration_consults.recv() => {
                     if cancel_requested() {
+                        agent.abort();
                         aborted = true;
                     }
+                    let _ = consulted.send(());
                 }
 ...
+        // No consult is served past the loop: a waiting run goes on.
+        drop(registration_consults);
         if aborted && !settled {
```

- **Pourquoi pas de blocage** :
  - la boucle sert le canal tant que l'admission tourne ;
  - dès sa sortie, le récepteur est lâché. Les `oneshot` en attente sont lâchés avec lui, et `consult.await` rend une erreur : l'abonné repart ;
  - un `AgentStart` suivant voit `send` échouer et ne s'arrête pas.
  - La garde `if !registration_consulted` d'upstream est retirée : avec une réponse attendue, toute demande doit être servie, sinon un second `AgentStart` dans la même admission bloquerait. Chaque enregistrement est donc relu.
- **Coût** : un aller-retour de tâche par démarrage de run (estimation : de l'ordre de la µs, non mesuré).
- **Risque** : un abonné qui attend la boucle de transfert. C'est un motif nouveau dans ce fichier. Les abonnés sont déjà attendus par `process_events` (`agent.rs:430-433`), mais aucun n'y attendait un autre acteur.

### 3.3 Risque de conflit

Commandes, sans toucher à l'arbre de travail (`git merge-tree` et un `commit-tree` jetable, sans ref) :

```sh
P=5778afa02
git merge-tree --write-tree --merge-base=$P^ origin/main $P          # code 0 : sans conflit
T=$(git merge-tree --write-tree --merge-base=$P^ origin/main $P | head -1)
C=$(git commit-tree $T -p origin/main -m tmp)                         # f8e84954c
for base in origin/main $C; do
  T=$(git merge-tree --write-tree $base upstream/main | head -n1)
  git cat-file -p "${T}:crates/pa-daemon/src/agent_engine/turn/run_once.rs" | grep -c '^<<<<<<<'
done                                                                  # 0, puis 1
for base in origin/main $C; do
  T=$(git merge-tree --write-tree --merge-base=be86884dd^ $base be86884dd | head -n1)
  git cat-file -p "${T}:crates/pa-daemon/src/agent_engine/turn/run_once.rs" | grep -c '^<<<<<<<'
done                                                                  # 3, puis 3
```

- **Merge de tout `upstream/main`** :
  - sans C-a : 0 bloc dans `run_once.rs` ;
  - avec C-a : **1 bloc**, trivial : leurs `let live` contre nos 3 lignes du `Notify`, lignes 89 à 95 du résultat. Il faut garder les deux ;
  - `tests/abort.rs` a déjà un bloc sans C-a (notre `KernelEnvOverride`). C-a n'en ajoute pas ;
  - `model.rs` et l'e2e : aucun.
- **Cherry-pick de `be86884dd` seul** : 3 blocs dans `run_once.rs` avec ou sans C-a. Avec C-a, le premier bloc contient en plus nos 3 lignes.
- **Si upstream remerge `5778afa02` tel quel** : changement identique, pas de conflit avec C-a. Avec C-b, les 8 lignes modifiées entrent en conflit (`Notify` contre canal) : petit, à refaire à la main.
- **#4042, si elle est mergée** : elle touche le bras `User` (nos `:226-239`) et `tests/abort.rs`. Nos insertions sont à `:253` et plus loin. Non simulé.

### 3.4 Effets

- **Préfixe de prompt** : inchangé. Ni le prompt système, ni les tools, ni le texte des annonces ne sont touchés.
- **Comportement** :
  - un Stop tombé dans la fenêtre termine le tour annulé à l'enregistrement : au premier `Pending` avec C-a, avant l'appel du provider avec C-b ;
  - la ligne de l'utilisateur et une ligne de l'assistant annulée restent dans l'historique, comme pour tout abandon après l'enregistrement. Le chemin de `:337`, lui, n'a pas de run.
- **Chemins couverts** : toutes les admissions et reprises du démon (§1.2). **Pas le RPC ni `-p`** (§1.3, §1.4), qui ne passent pas par `run_once`.

## 4. Le test du §7.1 : rouge sur son assertion, sans horloge

**Écarts avec le §7.1 du ticket 35, à valider** :

1. **La sonde** : celle de `5778afa02`, pas une sonde qui compte ses appels.
   - Elle dit « drapeau levé » exactement quand un run est enregistré : `engine.turn_agent` a un signal (`agent.signal().is_some()`, `agent.rs:1100-1103`). `turn_agent` est posé à la construction de la session (`crates/pa-daemon/src/agent_engine/lifecycle.rs:426-427`).
   - Elle rend faux à `model.rs:93` et à `run_once.rs:337`, vrai à la relecture d'`AgentStart`.
   - Elle ne compte rien et n'appelle pas `abort_in_flight_turn` : celui-ci ne ferait rien avant l'enregistrement.
2. **La preuve « aucun appel au provider »** : la réponse scriptée intacte, pas `registration.call_count()`. `faux_engine_with_settings` (`crates/pa-daemon/src/agent_engine/tests.rs:100-123`) n'expose pas l'enregistrement. Le provider factice consomme une réponse à chaque appel (`faux/mod.rs:731`). C'est la preuve qu'emploie déjà `abort.rs:174-236`.
3. **`emit` rend toujours `true`**, comme les tests d'`abort.rs`, au lieu de `!drapeau`. Le résultat ne change pas, et le test n'imite pas la porte du worker.

**Esquisse** (non compilée) :

```rust
//! yusAgent (YA-10, ticket 36): an abort flagged between the admission consult and the run's
//! registration lands before the provider call.
use super::*;

#[test]
fn abort_flagged_before_the_run_registers_never_reaches_the_provider() {
    let _faux = FAUX_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (engine, _dir) = faux_engine_with_settings(
        &serde_json::json!({ "responses": ["raced reply", "next reply"] }),
        1,
    );
    // Set exactly once a run is registered (the probe of upstream 5778afa02).
    let run_registered = || {
        engine.turn_agent.lock().unwrap().as_ref()
            .is_some_and(|agent| agent.signal().is_some())
    };
    let mut events = Vec::new();
    engine.run_prompt(0, PromptRequest { /* "held turn for the late abort" */ },
        &run_registered, &mut |event| { events.push(event); true });
    assert!(events.iter().any(|e| matches!(e, EngineEvent::DoneAborted)), "{events:?}");
    // The provider was never called: the first scripted reply serves the next turn.
    let mut next = Vec::new();
    admit(&engine, "the next turn".to_string(), &mut next);
    assert!(next.iter().any(|e| matches!(e, EngineEvent::AssistantMessage(m)
        if m["content"] == serde_json::json!([{ "type": "text", "text": "raced reply" }]))),
        "{next:?}");
}
```

- **Sans horloge** : ni thread, ni `sleep`, ni délai. Réponses immédiates : `run_prompt` rend la main dans tous les cas.
- **Attendu** :
  - **rouge aujourd'hui sur son assertion** : la requête part, le tour se termine sur « raced reply », pas de `DoneAborted`, et le tour suivant reçoit « next reply » ;
  - **rouge aussi avec C-a seul**, de façon déterministe : la réponse est consommée à l'appel du provider (`faux/mod.rs:731`), avant tout `Pending` possible. Le test prouve donc C-b ;
  - **vert avec C-b**.
- **Avec C-a seul**, ce test ne peut pas passer. Il ne reste que le test d'upstream : attente de 60 s, rouge par `recv_timeout` de 10 s, donc par l'horloge. Il passe avec C-a et avec C-b.
- **Fichier** : nouveau fichier du fork, `crates/pa-daemon/src/agent_engine/tests/ya_abort_window.rs`.
- **Ligne `mod`** : dans `crates/pa-daemon/src/agent_engine/tests.rs` (upstream), `mod ya_abort_window;` après `mod streaming;` (`:22`), donc nouvelle ligne `:23`. **1 ligne.**

**Plan des pushs proposé** :

| Push | Contenu | Attendu |
|---|---|---|
| 1 | ce ticket ; `ya_abort_window.rs` et sa ligne `mod` | **rouge sur son assertion** (« raced reply » absent du tour suivant) ; tout le reste vert |
| 2 | `git cherry-pick -x 5778afa02` (commit à part, message d'upstream gardé) ; C-b (commit `YA-10: ticket 36 : …`) ; doc (`docs/fork-changes.md`, fragment `.changes/`, ce ticket) | tout vert, y compris le test d'upstream et `rlm_children_delete_e2e` |

## 5. `rlm_children_delete_e2e` : la même fenêtre

- `docs/fork-changes.md:489-504` liste `a_deleted_running_child_records_the_cancel_edge` comme instable, avec l'hypothèse d'un démarrage lent du kernel de l'enfant.
- La PR #3872 décrit le même symptôme (`kernel cell receipt never appeared … delete.json`), causé par cette fenêtre : le kill de l'enfant attend le premier événement du provider, `KILL_TIMEOUT_MS` vaut 30 s, puis l'e2e sonde 60 s. Total : environ 92 s.
- **Nos deux échecs**, lus le 25/11/2026 :
  - job 132140999318 (run 43499374052, 13/11) : `finished in 92.28s`, ligne 1585 du journal ;
  - job 134029453827 (run 44080414273, 17/11) : `finished in 91.86s`, ligne 1675 du journal ;
  - commande : `gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/<job>/logs | grep -an "finished in"`.
- **Conclusion** : même signature. Cause très probable : la fenêtre de ce ticket. Ce n'est pas une preuve : `delete.error` n'est pas lu par notre version de l'e2e.
- C-a change l'e2e pour lire ce reçu.
- **Proposé pour le push 2** : garder l'entrée dans la liste, avec la cause corrigée, jusqu'à quelques runs verts après la correction.

## 6. PR chez upstream

- **C-a** est leur propre PR #3872, fermée par un mainteneur au tri. La reproposer telle quelle reviendrait à contester ce tri.
- **Plus adapté** : un commentaire sur #3872, ou une issue, qui signale :
  - que le portage async est mergé sans cette relecture (`upstream/main` `e44e9ae9d`, `run_once.rs:347-352`) ;
  - nos preuves : `queue.rs:725` (ticket 35, 2 échecs) et les deux échecs de `rlm_children_delete_e2e` à environ 92 s.
- **C-b** pourrait partir en PR, avec leur test et le nôtre (déplacé dans `abort.rs`, sans préfixe `ya_`), comme suite de #3872. L'argument : aucune requête au provider après un Stop, et un test sans horloge. Le point faible : un abonné qui attend la boucle (§3.2).
- **Rien n'est envoyé** sans accord de Yusuf.

## 7. Points ouverts

- **RPC** : pas de drapeau d'abandon, un abandon perdu n'est jamais rattrapé (§1.3). Défaut d'upstream, hors de `run_once`. Ticket à part si on utilise le RPC.
- **`-p`** : sortie retardée par un sous-agent en ligne dont l'abandon s'est perdu (§1.4). Lu, non testé.
- **`6679f881d`** absent chez nous : un abandon tombé entre l'enregistrement et le flux du provider, hors relecture, finit en erreur au lieu d'un abandon si la porte du worker laisse passer la ligne annulée (`turn.rs:513-523`). Non vérifié en test ; indépendant de ce ticket.
- **Ticket 35, §11.3, §11.1 et §13.2, et ticket 37** : le chemin « continuation autonome admise dans le même `run_prompt` » n'existe pas sur ce code (§1.2). À corriger dans le ticket 35 et à reprendre dans le constat du 37.
- **#4042** ouverte chez upstream : à suivre, elle change la ligne acceptée et recouvre en partie ce correctif (§2).

## 8. Décisions attendues de Yusuf

1. **C-a seul** (cherry-pick de `5778afa02`, test d'upstream rouge par délai) ou **C-a + C-b** (test du §7.1 sans horloge, aucune requête après un Stop).
2. Les trois écarts du §4 avec le §7.1 (sonde, preuve par la réponse scriptée, `emit` à `true`).
3. La ligne `mod ya_abort_window;` dans `crates/pa-daemon/src/agent_engine/tests.rs:23` (1 ligne upstream).
4. Le traitement de `rlm_children_delete_e2e` dans `docs/fork-changes.md` (§5).
5. Commentaire ou issue chez upstream (§6), ou rien.

## 9. Décisions de Yusuf (25/11/2026)

1. **C-a seul** : `git cherry-pick -x 5778afa02`, avec son test, sa reprise de `rlm_children_delete_e2e` et son fragment de changelog. **C-b n'est pas retenu** : son gain (aucune requête partie après un Stop tombé dans une fenêtre de quelques ms) n'est pas mesurable, et il fait diverger `run_once.rs`. Le diff esquissé reste au §3.2, au cas où.
2. **Test** : le §7.1 vérifiait la garantie de C-b, pas celle de C-a. Il est adapté :
   - le provider factice garde sa réponse fermée ;
   - l'abandon est armé juste après la vérification de `run_once.rs:337` ;
   - le test vérifie que le tour se termine en abandon sans la réponse scriptée ;
   - le délai ne sert que de filet ;
   - rouge sur son assertion sur `main`, vert avec C-a.
   Les écarts du §4 sont acceptés là où ils s'appliquent encore.
3. **Ligne `mod ya_abort_window;`** à `crates/pa-daemon/src/agent_engine/tests.rs:23` : accord.
4. **`rlm_children_delete_e2e`** : retiré d'« Instabilité probable » dans `docs/fork-changes.md`, dans la même PR. Cause : la fenêtre d'abandon (91,86 s et 92,28 s, signature attribuée par la PR #3872 d'upstream). L'hypothèse du kernel lent est abandonnée.
5. **Upstream** : texte d'un commentaire pour la PR #3872 au §10.4. Rien n'est publié.
6. **RPC** : dire si C-a le couvre ; sinon, le noter en limite, avec la lecture du disque de R3 qui y reste (§10.3).

## 10. Réponses avant le code

### 10.1 Le test adapté

`crates/pa-daemon/src/agent_engine/tests/ya_abort_window.rs`, test `abort_armed_after_the_admission_consult_cancels_the_registered_run`.

- **Provider** : `faux_engine_with_settings` (`tests.rs:100-123`), une réponse « held reply » retenue 60 s (`delayMs`).
- **Sonde** : à état, comme au §7.1 du ticket 35.
  - Elle rend faux au premier appel (`model.rs:93`) et au second (`run_once.rs:337`).
  - Au second, avant de rendre faux, elle arme l'abandon comme le worker (`commands.rs:328-374`) : drapeau levé, puis `engine.abort_in_flight_turn()`, qui ne trouve aucun run.
  - Tous les appels suivants rendent vrai.
  - L'écart 1 du §4 (sonde d'upstream, « run enregistré ») ne s'applique donc plus : la demande est d'armer juste après `:337`.
- **`emit`** rend toujours `true` (écart 3 du §4, toujours valable).
- **Assertions, sans délai** :
  - l'abandon a été armé (la sonde a bien atteint la vérification de l'admission) ;
  - le tour se termine par `DoneAborted` ;
  - aucun `AssistantMessage` ne porte « held reply ».
- **Ce que veut dire « réponse non consommée » ici** : la réponse n'est jamais livrée au tour.
  - Le provider factice retire sa réponse de la file dès l'appel (`crates/pa-ai/src/providers/faux/mod.rs:731`), avant de la retenir (`:784-799`).
  - Avec C-a, l'appel part, puis il est annulé pendant l'attente : la réponse est retirée, mais jamais livrée.
  - « Jamais retirée » (écart 2 du §4, preuve par le tour suivant) est la garantie de C-b. Cet écart ne s'applique plus.
- **Sans horloge** :
  - **Sur `main`** : rien ne relit le drapeau après `:337`. Le tour attend la fin de la retenue, reçoit « held reply » et se termine normalement : rouge sur l'assertion `DoneAborted`. La retenue fixe seulement le moment où `main` échoue, pas l'issue.
  - **Avec C-a** : la relecture a lieu au premier `Pending` de l'admission après `AgentStart`. Ce `Pending` arrive au plus tard quand le flux du provider attend la réponse retenue (`response.rs:157-166`, `faux/mod.rs:787-799`). L'abandon annule alors la retenue (`faux/mod.rs:794-797`) : vert, quelle que soit la vitesse de la machine.
- **Fragilité** : la sonde compte ses appels. Si upstream ajoute une vérification avant `:337`, l'abandon serait armé plus tôt, et le test passerait sur `main` par l'ancien chemin. L'assertion `armed` ne le détecte pas. À revoir à chaque reprise d'upstream qui touche `model.rs` ou `run_once.rs`.

### 10.2 C-b, archivé

Non retenu (§9, point 1). Le diff du §3.2 est gardé tel quel, non compilé.

### 10.3 RPC : non couvert par C-a

- C-a est dans `crates/pa-daemon/src/agent_engine/turn/run_once.rs`, que le RPC n'emploie pas : il appelle directement le moteur de pa-core (`rpc/prompt_commands.rs:27-49`, `rpc/commands.rs:111-180`).
- Le RPC n'a pas de drapeau d'abandon : `abort` appelle `agent.abort()` et suspend la pompe (`rpc/commands.rs:216-220`).
- **Limite** : un abandon RPC arrivé avant l'enregistrement du run est perdu, et le tour va à son terme (§1.3).
- La lecture du disque de R3 reste dans cette fenêtre (`admission.rs:47`, `:148`) : le jeton du ticket 35 n'est levé que par le worker du démon (`ya_resource_notice.rs:394-400`).

### 10.4 Commentaire proposé pour la PR #3872 d'upstream (non publié)

```text
The async port has merged (L1–L5, R1–R8, up to e44e9ae9d), but this fix did not re-land with it:
on main, run_turn_once still consults the cancel flag only at the admission future's head
(crates/pa-daemon/src/agent_engine/turn/run_once.rs:347-352), and Agent::abort is still a no-op
before the run registers (crates/pa-agent/src/agent.rs:1209-1213). An abort landing in
[admission consult, registration] still waits for the provider's first streamed event.

Evidence from a fork that carries the same code:
- rlm_children_delete_e2e::a_deleted_running_child_records_the_cancel_edge failed twice in CI with
  the signature described above: "kernel cell receipt never appeared ... delete.json" at
  finished in 92.28s and 91.86s (30 s kill timeout + 60 s receipt poll).
- worker::turn_stream_tests::queue::abort_and_send_queued_acks_before_the_follow_up_delivery failed
  twice at queue.rs:725 ("the follow-up's turn never settled on the bare abort") once the fork put
  a disk read inside that window; the window itself is upstream's.
- We cherry-picked 5778afa02 unchanged. A test that arms the flag right after the admission
  consult (and calls abort_in_flight_turn, like the worker) is red on its assertion without the
  fix and green with it: <runs to fill in after CI>.

Would you consider re-landing 5778afa02 as is?
```

Les identifiants de runs sont à remplir après le push 2. Le texte cite des runs du dépôt du fork : à Yusuf de décider si on les donne.

## 11. Réalisation

| Push | Contenu | Attendu |
|---|---|---|
| 1 | ce ticket ; `ya_abort_window.rs` et la ligne `mod` | **rouge sur son assertion** (`DoneAborted` absent, après la retenue de 60 s) ; tous les autres checks requis verts. `changelog fragment` (non requis) rouge : `tests.rs` compte comme livré pour ce check, et le fragment arrive avec le cherry-pick |
| 2 | `git cherry-pick -x 5778afa02` ; doc (`docs/fork-changes.md` : fichiers repris, ligne `mod`, retrait de `rlm_children_delete_e2e`) | tout vert ; `compare_capture_prompts.py` : TOUT IDENTIQUE |

### 11.1 Push 1 : test seul (`f270c7113`, run `ci` 45571446031)

- **Seul échec, rouge sur son assertion** : `abort_armed_after_the_admission_consult_cancels_the_registered_run` (shard 3/8, job 139058948486), `ya_abort_window.rs:53`, assertion `DoneAborted`.
  - L'assertion `armed` passe : l'abandon a bien été armé à la vérification de l'admission.
  - D'après les événements, le tour a attendu la fin de la retenue et reçu « held reply » : 60,05 s entre la ligne de l'utilisateur (`1795593561073`) et le début de l'assistant (`1795593621122`).
- `pa-daemon#lib` : 1 246 réussis, 1 échec. Les autres checks requis sont verts.
- `Check changelog fragment` (non requis) : rouge, comme prévu (§11).

### 11.2 Push 2 : la reprise

- `a56e9ac89` : `git cherry-pick -x 5778afa02`, sans conflit. Seul le sujet reçoit le préfixe `YA-10: ticket 36 :` (règle 12). Le corps d'upstream et la ligne `cherry picked from` sont gardés, et le `patch-id` est identique (`c8f4dae47e46`, `git patch-id`).
- `beba6631a` : `docs/fork-changes.md` (section du ticket 36 ; entrée « Instabilité probable » de `rlm_children_delete_e2e` retirée).
- **CI** :
  - run `ci` 45573576193 : 16 checks requis verts, aucun sauté (`gh pr checks 86 --required`) ;
  - `ya_abort_window`, le test d'upstream et `rlm_children_delete_e2e` (3,5 s) : `ok` ;
  - `ya-build-macos-arm64` 45573567590 vert, SHA256 vérifié.
- **Préfixe** (`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`, commandes dans la PR #86) : `compare_capture_prompts.py`, smoke bench en capture, `py-example-001`, `main` `75bdd0c10` contre `3765b36ca` :
  - `resolved` des deux côtés ;
  - 9 requêtes, **TOUT IDENTIQUE** ;
  - rien d'écrit dans `~/.yusagent` ni `~/.prime/agent`.
- **Préfixe de prompt : inchangé.**
