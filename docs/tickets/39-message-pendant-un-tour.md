# Ticket 39 : R4, message pendant un tour (constat et plan)

Demande de Yusuf du 25/11/2026. CDC YA-19 (`docs/yusAgent-cdc.md:529-539`) ; étape 4 du découpage du ticket 24 (`docs/tickets/24-ya10-extension-minimal.md:721-723`) ; nom de méthode fixé au ticket 32 (`docs/yusAgent-cdc.md:103`, `docs/tickets/32-identite-yusagent.md:188`).

Branche `ya-39-constat`, depuis `main` à `5661bdebc` (merge de la PR #89). **Constat et plan seulement** : aucun code, rien dans `docs/fork-changes.md`, rien compilé, aucun appel de modèle.

Toutes les lignes citées sont celles de `main` à `5661bdebc`, sauf mention contraire. Unharnessed : clone jetable de `github.com/unharnessed-agent/Unharnessed` à `f8e4658dd89f845a4c3f16dd2658bf16df65b969` (22/11/2026, le commit lu au ticket 32), dans le dossier temporaire de la session, en lecture seule, rien exécuté. Ses chemins sont préfixés `uh:`.

## Résumé

- **Le cœur sait déjà faire le message pendant un tour, par la file `steering` du worker** :
  - un message en file **coupe le run à la frontière suivante, après les résultats des tools**, sans interrompre le tool ;
  - le worker le livre ensuite **dans un nouveau run** ;
  - plusieurs messages en file partent **dans un seul tour** (mode `all`, défaut du worker), sous forme de **N messages user**, pas d'un seul bloc de texte.
  - Le TUI s'en sert : Entrée pendant un tour envoie un `steer`, Échap envoie `abort_and_send_queued` (§1).
- **Le pont ACP ne sert pas `_yus/session/steer`** : réponse `-32601`. Un second `session/prompt` pendant un tour est refusé en `-32603` (§2).
- **Unharnessed injecte le message dans le run en cours** (`Agent::steer`, agent dans le processus, sans démon). Le message rejoint **le même tour ACP** ; l'extension l'apprend par un `turnStart` avec `origin: "steer"`. Un message que le tour n'a pas pris est jeté à la fin du tour, et le client le renvoie (§3).
- **Notre extension contient déjà tout le côté client d'Unharnessed.** La méthode est rejetée sur place et le bouton « Send now » est caché. Cmd/Ctrl+Entrée retombe donc sur la file du client, envoyée en prompt après le tour (§3.3).
- **Cache** : dans les deux façons de faire (file du worker, injection dans le run), le message est ajouté après les résultats des tools du tour du modèle qui vient de finir. Ni le prompt système ni les tools ne changent. Rien n'est encore mesuré (§4).
- **Point dur pour R4** : avec la file du worker, le `PromptAndWait` du pont répond dès la fin du premier run, avant le run du message. L'extension reprise, elle, attend que ce run fasse partie du même tour ACP. Sinon, elle remet le message en tête de sa file et le renvoie : **le message partirait deux fois** (§2.3, §6).

## 1. Ce que le cœur offre déjà

### 1.1 Les deux files du worker

- `Lane::Steering` et `Lane::FollowUp` (`crates/pa-daemon/src/worker/queue.rs:10-12`). Commandes du démon : `Steer` et `FollowUp` (`crates/pa-types/src/daemon/command.rs:377-396`), traitées par `handle_queue` (`crates/pa-daemon/src/worker/commands.rs:43-44`, `crates/pa-daemon/src/worker/input.rs:164-243`) :
  - ajout en file avec la politique `Queued`, visible dans la projection de la file ;
  - réponse `{ queued: true }` ;
  - reprise de la file suspendue après un abort (`resume_queued_input`, `input.rs:170`).
- Un `prompt` ou un `prompt_and_wait` qui porte `streamingBehavior: "steer"` va aussi dans la file `steering` (`input.rs:98-110`). Si la session est occupée, sa politique est `Queued` (`input.rs:129-133`).
  - Avec `prompt_and_wait`, la réponse part quand le run qui livre ce message est réglé (`crates/pa-daemon/src/worker/turn.rs:396-398`, `:1182-1184`).
  - Sans `streamingBehavior`, un prompt sur une session occupée va dans la file `follow_up` (`input.rs:103-109`).

### 1.2 Coupure du run en cours (frontière d'un tour du modèle)

- La sonde `queued_steering_probe` est vraie tant que la file `steering` n'est pas vide (`crates/pa-daemon/src/worker.rs:332-343`). Elle porte le nom de la logique TS `_steeringStopPending` (`crates/pa-daemon/src/agent_engine/config.rs:37-41`).
- Elle est branchée sur `should_stop_after_turn` (`crates/pa-core/src/session_engine/engine.rs:981-996`). La boucle de l'agent n'appelle ce hook qu'après trois étapes :
  - l'exécution des tool calls (`crates/pa-agent/src/agent_loop/run.rs:101`) ;
  - l'ajout de leurs résultats au contexte (`run.rs:105-109`) ;
  - `TurnEnd` (`run.rs:113-117`).
- Elle appelle ensuite le hook (`run.rs:132-153`), puis termine le run (`AgentEnd`).
- **Le tool n'est donc pas interrompu** : la sonde n'est lue qu'une fois le lot de tool calls terminé. C'est le critère 1 de YA-19, au niveau du tour du modèle.
- La même sonde fait aussi céder la compaction automatique au seuil (`crates/pa-daemon/src/agent_engine/turn/model.rs:405-415`, `:434-440`).
- Lecture annexe, sans effet sur ce ticket : `engine.rs:982` prend la sonde (`take()`), puis `:998` clone le champ déjà vidé. `should_stop_before_turn` reçoit donc `None`. Le champ `should_stop_after_turn` couvre le même cas. Non mesuré.

### 1.3 Livraison au tour suivant

- Une fois le run réglé, le `TurnRunner` du worker prend la file `steering` avant la file `follow_up` (`turn.rs:62-84`). Il forme le lot (`gather_delivery_batch`, `queue.rs:324-370`) et lance **un nouveau run** du moteur (nouvel `agent_start`).
- Le nouveau run passe par l'admission d'un prompt. Les lignes de préfixe éventuelles sont émises avant les lignes du lot (`turn.rs:1036-1047`) :
  - annonce de ressources du ticket 31 ;
  - lignes en attente du worker.

### 1.4 Regroupement

- Le mode `steering` du worker vaut `all` par défaut, et `follow_up` vaut `one-at-a-time` (`worker.rs:317-318`). Le réglage est modifiable (`crates/pa-daemon/src/setting_switches.rs:15`, `:495`).
- En `all`, les messages en tête de file forment un seul lot s'ils ont la même politique, sans ligne personnalisée et sans commande de session (`queue.rs:347-367`). Ce lot donne **N messages user consécutifs dans un seul tour**, avec une seule réponse du modèle.
- Un avis `[bash-done]`, qui est une ligne personnalisée, part seul (`queue.rs:347-353`).
- Test d'upstream e2e : `multi_steer_parked_mid_run_co_delivers_as_one_batched_turn` (`crates/pa-daemon/tests/steer_boundary_batch_e2e.rs:245-388`).
  - Il pose trois `steer` pendant une cellule qui dort.
  - Il vérifie : un seul `agent_start` pour le lot après le premier `agent_end` (`:346-352`) ; les trois lignes user à la suite (`:360-368`) ; une seule réponse (`:369-373`).
  - L'en-tête du test note l'écart d'upstream à TS, qui livre un message à la fois (`:1-4`).
- Tests unitaires : `crates/pa-daemon/src/worker/turn_stream_tests/queue.rs:147`, `:194`, `:239`.

### 1.5 `abort_and_send_queued` (ticket 25)

- `crates/pa-daemon/src/worker/commands.rs:291-323` :
  - arme le lot forcé (`arm_forced_all_steering`, `:274-289`) ;
  - abandonne le run en cours (`request_abort`, `:328-` ; le tool est interrompu) ;
  - relance la file : le lot `steering` part tout de suite dans un nouveau run, puis la file `follow_up`.
- Test d'upstream : `turn_stream_tests/queue.rs:415`.
- Pour comparaison :
  - `abort_and_clear_queue` vide les deux files (`commands.rs:684-698`, via `handle_clear_queue` `:593-640`) ;
  - `abort` garde la file suspendue jusqu'au prompt suivant (`:376-379`, `:328-336`).

### 1.6 L'autre voie : la file de l'agent, dans le processus

- `Agent::steer` (`crates/pa-agent/src/agent.rs:994-1001`) met le message dans la file de l'agent.
- La boucle lit cette file **dans le même run** :
  - après les résultats des tools (`run.rs:163-177`) ;
  - au début d'un run (`run.rs:31-32`).
- Test d'upstream : `steering_message_injects_before_next_turn` (`crates/pa-agent/tests/agent_loop_scripted.rs:588`).
- Qui s'en sert :
  - `SessionEngine::prompt` sur un agent occupé avec `Steer` (`crates/pa-core/src/session_engine/admission.rs:117-131`) ;
  - le mode `rpc` (`crates/pa-daemon/src/rpc/prompt_commands.rs:210-225`) ;
  - les avis bash en processus (`crates/pa-core/src/session_engine/bash_notices.rs:213`, `ya_bash_notice_batch.rs:224`).
- **Le worker du démon ne passe pas par cette voie** : il garde ses propres files et coupe le run (1.2, 1.3). C'est en revanche la voie d'Unharnessed (§3).

### 1.7 Le TUI

- **Entrée pendant un tour** : `prompt` avec `streamingBehavior: steer` et `queueIfBusy: true` (`crates/pa-tui/src/session_ui/keys.rs:899-907`, `crates/pa-tui/src/session_ui/prompt.rs:737-760`).
- **Raccourci `app.message.followUp`** : `follow_up` (`keys.rs:800-818`).
- **Stop (interruption)** :
  - `abort_and_send_queued` si le démon l'annonce, sinon `abort` (`crates/pa-tui/src/session_ui/apply.rs:21-50`, `:676-689`) ;
  - donc les messages en file partent tout de suite après l'arrêt.
- La file est visible et modifiable (`keys.rs:901-903`).

## 2. Le pont ACP aujourd'hui

### 2.1 Un prompt pendant un tour

- **Pendant un prompt du pont** : refusé à l'admission, « A prompt turn is already running for this ACP session » (`crates/pa-daemon/src/acp/daemon.rs:832-834`). Réponse `internal_error` (`-32603`, `daemon.rs:585-587`).
- **Pendant un tour lancé par l'agent** : le prompt attend la fin du tour, puis il est admis (ticket 25, T3 ; `crates/pa-daemon/src/acp/ya_turns.rs:238-258`).
- **`_yus/session/steer`** : absent du dispatch, réponse `-32601` « Method not found » (`daemon.rs:1165-1172`).

### 2.2 Ce que le pont envoie au démon

- Chaque prompt du pont est un `PromptAndWait` avec `streamingBehavior: FollowUp`, `queueIfBusy: true` et un identifiant d'admission (`daemon.rs:1625-1657`). La réponse arrive au règlement du run qui livre ce prompt (`turn.rs:1182-1184`). Le pont publie ensuite la frontière et l'enveloppe de fin, puis il répond (`daemon.rs:1658-1720`).
- **Stop d'un prompt du pont** : `stop_session_work`, qui envoie `AbortAndClearQueue` puis `WaitForIdle` (`daemon.rs:959-980`).
- **Stop d'un tour lancé par l'agent** : `AbortAndSendQueued` (`ya_turns.rs:315-326` ; ticket 25, T4).

### 2.3 Ce qu'il se passerait avec la file du worker telle quelle (lecture, non mesuré)

On suppose qu'un `steer` arrive dans la file du worker pendant un prompt du pont, par exemple depuis un TUI attaché (E4 du ticket 25). La suite, d'après le code :

1. Le run est coupé après les résultats des tools (1.2).
2. Le `PromptAndWait` du pont répond au règlement de ce premier run. Le message du `steer` n'appartient pas à ce run.
3. Le run du `steer` démarre. `AgentTurns::open` le rattache au prompt du pont si celui-ci n'a pas encore appelé `finish_prompt` (`ya_turns.rs:100-103`). Sinon, il ouvre un nouveau tour avec `origin: "prompt"` (`ya_turns.rs:104-111`, `:332-352`).
4. L'issue dépend donc de l'ordre entre la fin du prompt du pont et le premier événement du run suivant : **course non mesurée**.

## 3. Unharnessed et notre extension

### 3.1 Le pont d'Unharnessed (`uh:crates/uh-daemon/src/acp/steer.rs`, 106 lignes, introduit par `814d67695` du 08/11/2026)

- **Requête** : `_unharnessed/session/steer` `{ sessionId, prompt, turn }`, où `prompt` porte les blocs d'un `session/prompt` et `turn` le `promptTurnId` du tour en cours (`steer.rs:3-5`, `:32-33`).
- **Refus `-32603`** « The turn is over: send it as a prompt. » (`steer.rs:48-58`) dans quatre cas :
  - session en fermeture ;
  - aucun tour lancé par un prompt ;
  - Stop demandé ;
  - `turn` différent du tour actif.
- **Refus `-32602`** avec `data.details` (`steer.rs:59-77` ; `uh:crates/uh-core/src/session_engine/admission.rs:224-246`) :
  - blocs invalides ;
  - images que le modèle ne lit pas ;
  - commande de session.
- **Sinon** :
  - le message est mis dans **la file de l'agent du processus**, `agent().steer(message)` (`steer.rs:99`) ;
  - la réponse est `{}` (`steer.rs:100`) ;
  - le verrou d'état est tenu pendant l'ajout, donc le tour ne peut pas finir entre-temps (`steer.rs:34-36`).
- **Prise par la boucle**, avant le prochain appel du modèle, **dans le même run** (1.6). Au `message_start` du message (repéré par son horodatage, `uh:crates/uh-daemon/src/acp/session.rs:117-123`, `:303-313`) :
  - une marque `steer` est écrite dans le fichier de session (entrée personnalisée, `session.rs:316-322`) ;
  - puis le pont publie `turnStart { origin: "steer", rewindTo }` sous le même `promptTurnId` (`session.rs:300`, `:324-336`).
- **Fin du tour** : `drop_steers` vide la file de l'agent (`session.rs:126-132`, appelé en `uh:crates/uh-daemon/src/acp/prompt.rs:975`). Un message que le tour n'a pas pris est renvoyé par le client.
- **Mode `steering` par défaut** : `all` (`uh:crates/uh-core/src/settings/manager.rs:496-498`).

Choix qu'Unharnessed a tranchés :
- (U1) injection dans le run en cours, sans coupure ;
- (U2) même tour ACP, signalé par `turnStart` `origin: "steer"` ;
- (U3) aucune file dans le pont : refus explicite quand le tour est fini, et le client renvoie le message en prompt ;
- (U4) message non pris jeté à la fin du tour ;
- (U5) marque `steer` dans le fichier de session, pour le rejeu ;
- (U6) mode `all`.

### 3.2 L'extension d'Unharnessed

Le code est le même que le nôtre. Les lignes ci-dessous sont celles de notre copie, `extensions/vscode/src/chat.ts`.

- **Entrée pendant un tour** : le message va dans la file du client, puis part en `session/prompt` à la fin du tour (`chat.ts:281-286`, `:690-697`).
- **Cmd/Ctrl+Entrée** (`extensions/vscode/webview/Composer.tsx:341-344`) **ou le bouton « Send now »** d'un message en file (`Composer.tsx:897-906`) : `now()` place le message dans `steering`, puis la requête `steer` part avec l'identifiant du tour vivant (`chat.ts:425-480`).
- **Refus** : le message revient en tête de la file du client (`chat.ts:470-478`).
- **`turnStart` `steer`** : `joined()` ferme le tour affiché et en ouvre un nouveau dessous, avec le texte du message (`chat.ts:787-789`, `:822-850`). Un `turnStart` `steer` consomme un message de `steering` : un lot de N messages donne N tours affichés.
- **Fin de tour** : ce qui est encore dans `steering` revient en tête de file (`requeue`, `chat.ts:491-498`, appelé en `:673-675`), puis `next()` l'envoie en prompt.
- **Fin de la réponse `session/prompt`** : elle ferme `this.sent`, qui désigne le dernier message rejoint s'il y en a un (`chat.ts:609-618`).
- **Stop** : `cancel()` met la file en pause (`chat.ts:303-311`, `pause` `:481-489`). Les messages attendent « play » (`resume`, `:414-419`).

### 3.3 Notre extension aujourd'hui

- La constante vaut `_yus/session/steer` (`chat.ts:38-39`).
- La méthode est rejetée sur place, sans trame, en `-32601` (`extensions/vscode/src/ya/methods.ts:1-4`, `:32-38` ; `src/agent.ts:97`).
- `features().steer` vaut `false` (`methods.ts:45`). Le bouton « Send now » est caché pendant un tour (`Composer.tsx:901`).
- Cmd/Ctrl+Entrée reste branché sur `now()`. Pendant un tour, la requête est rejetée, le message passe en tête de file et part en prompt après le tour (`chat.ts:470-478`).
- Test : `extensions/vscode/test/unit/agent.test.ts:86`, qui vérifie le rejet.
- **Conséquence** : un message envoyé pendant un tour est déjà mis en file et pris après la fin du tour ACP. Il n'est pas pris après les tool results du tour du modèle en cours.

## 4. Effet sur le cache

### 4.1 Où le message entre

- **File du worker** (1.2, 1.3) : le run est coupé après l'ajout des résultats des tools au contexte (`run.rs:105-117`, `:132-153`). Le run suivant ajoute les lignes du lot après ces résultats (`turn.rs:1036-1047`).
- **File de l'agent** (1.6, Unharnessed) : la boucle ajoute le message après les résultats des tools (`run.rs:105-109`, puis `:163-177`, puis `:56-` au tour suivant).
- **Hypothèse** : dans les deux cas, la requête suivante a la même forme, [assistant avec tool calls, résultats des tools, N messages user]. Seule différence attendue : la file du worker relance un run, donc passe par l'admission du prompt (`turn.rs:1036-1047`). Il faut vérifier qu'aucune ligne de préfixe (annonce de ressources, lignes en attente) ne s'ajoute à tort à chaque relance. Non mesuré.
- **Aucune option ne touche au prompt système ni aux tools** : le message ne part que dans l'historique.
- **Masquage** (YA-05, coupé par défaut) : les tours sont comptés par messages assistant (`crates/pa-core/src/session_engine/masking.rs:8-15`). Couper puis relancer un run n'en change pas le compte. Lecture, non mesuré.
- **Marque `steer` d'Unharnessed** (U5) : c'est une entrée personnalisée, pas un message personnalisé. Chez nous, `build_session_context` ne reprend que les `FileEntry::CustomMessage` (`crates/pa-core/src/session/mod.rs:301`, `:365`, `:378`, `:417-430`). Une telle marque n'irait donc pas au modèle. Lecture.

### 4.2 Comment le vérifier avec nos outils

- **`crates/pa-core/tests/prefix_stability.rs`**
  - `assert_prefix_stable` (`:813-863`) compare chaque requête N à la requête N+1 :
    - prompt système, tools et paramètres identiques à l'octet ;
    - messages de N en préfixe exact de N+1, seul `cache_control` pouvant bouger.
  - Le test tourne sur le moteur `pa-core` dans le processus, sans démon. Il couvre donc **la voie de la file de l'agent** (`SessionEngine::prompt` avec `Steer` pendant un run, `admission.rs:117-131`), **pas la file du worker**.
  - Ajout possible : un scénario avec un message pendant une cellule lente.
- **Banc de capture** (`bench/tools/capture/run.py`, serveur factice, sandbox, vrai binaire, chemin du démon et du pont)
  - `analyze.py` produit la section « Préfixe N → N+1 » (`bench/tools/capture/analyze.py:124-150`, `:286-293`). Ce serait le contrôle sur un nouveau cas ACP avec `steer` : critère YA-19 « préfixe intact après injection » par la vraie voie du produit.
  - `compare.py --all`, entre `main` et la branche, montrerait que les cas existants ne changent pas (`TOUT IDENTIQUE`).
  - `prefix_paths_check.py` et `extension_frames_check.py` tournent déjà à la fin de `run.py`.
- **`bench/smoke/tools/compare_capture_prompts.py`**
  - Il compare **deux runs** du smoke bench requête par requête, appariées par nom (`:1-14`). Il ne compare pas N à N+1 dans un même run.
  - Usage ici : `main` contre la branche, sur un scénario en `--capture` sans modèle (`bench/smoke/run.py:6`). Comme `main` ne sert pas `steer`, il ne peut montrer que l'absence de changement sur les scénarios sans `steer`.

## 5. Stop pendant qu'un message est en file

| Où est le message | Notre pont (`main`), Stop d'un prompt du pont | Notre pont, Stop d'un tour lancé par l'agent | Unharnessed |
|---|---|---|---|
| File du client, extension (pas encore envoyé) | File mise en pause (`chat.ts:303-311`) ; part au « play » | Idem | Idem |
| Accepté par le pont, pas encore pris | File du worker : `AbortAndClearQueue` le retire (`commands.rs:684-698`). File de l'agent : à vider, comme `drop_steers`. L'extension le remet en tête de sa file, en pause (`chat.ts:491-498`, `:673-675`) : pas de perte, renvoi au « play » | File du worker : `AbortAndSendQueued` le livre **tout de suite** dans un nouveau run (`commands.rs:299-323`). L'extension, qui n'a pas vu `turnStart` `steer`, le remet en file et le renverrait au « play » : **doublon possible**. Hypothèse, non mesurée | Refusé si le Stop est déjà demandé (`steer.rs:50`) ; jeté à la fin du tour (`prompt.rs:975`) ; le client le remet en tête, en pause |
| Déjà pris (son tour a commencé) | C'est le tour en cours : Stop l'arrête comme un prompt | Idem | Idem |

Le TUI, lui, envoie les messages en file tout de suite après un Stop (`abort_and_send_queued`, 1.7). Unharnessed les met en pause. Les deux comportements diffèrent.

## 6. Plan

### 6.1 Options pour R4 (pont)

**R4-A : file du worker** (forme prévue au ticket 24, `docs/tickets/24-ya10-extension-minimal.md:722`)

- **Requête** : `_yus/session/steer` `{ sessionId, prompt, turn }`, mêmes refus qu'Unharnessed. Le pont pose le message dans la file `steering` du worker, par une des deux voies :
  - (A1) `PromptAndWait` avec `streamingBehavior: steer` et un identifiant d'admission, ce qui donne une fin par message (`input.rs:98-110`, `turn.rs:1182`) ;
  - (A2) commande `Steer`, sans attente (`input.rs:164-243`).
- **Le tour ACP reste ouvert** jusqu'au règlement des runs de tous les messages acceptés. Le pont ne répond à `session/prompt` qu'après, et il refuse un `steer` une fois sa fin de tour commencée. L'acceptation et la décision de finir passent sous le verrou d'état, comme `steer.rs:34-36`.
- **`turnStart` `steer`** est publié sous le même `promptTurnId`, au `message_start` du message.
  - **Point ouvert** : les événements du worker ne disent pas, d'après la lecture, quel message user vient de quel `steer`. Un appariement par ordre ou par texte serait fragile si un TUI attaché envoie aussi des messages. À vérifier dans le constat de la PR.
- **Comportement** :
  - le run est coupé après les tool results, puis un nouveau run part (nouvel `agent_start`) ;
  - les messages sont regroupés en `all`, avec N lignes user.
- **Écart à Unharnessed (U1)** : coupure et relance au lieu de la suite du même run.
- **Ce qu'elle touche** :
  - nouveau module `crates/pa-daemon/src/acp/ya_steer.rs` ;
  - accroches dans `crates/pa-daemon/src/acp/daemon.rs` (upstream) : dispatch, attente dans `prompt_turn`, événements ;
  - `docs/fork-changes.md` ;
  - **ni le worker ni la couche providers**.

**R4-B : file de l'agent, dans le run en cours** (le choix d'Unharnessed, U1 à U4)

- **Requête et refus** : les mêmes.
- **Nouvelle commande du worker** (nom à fixer, `ya_steer`), qui fait `session.agent().steer(message)` dans le moteur du run en cours. À la fin du run, la file de l'agent est vidée, comme `drop_steers`. Sans ce vidage, un message non pris passerait au run suivant, que la boucle lit au début d'un run (`run.rs:31-32`).
- **Le run continue** : le `PromptAndWait` du prompt couvre naturellement le message, et aucune attente supplémentaire n'est nécessaire dans le pont.
- **`turnStart` `steer`** : publié au `message_start` du message, repéré comme chez Unharnessed. Le pont garde la liste des messages qu'il a passés.
- **Ce qu'elle touche** :
  - fichiers upstream : dispatch du worker (`crates/pa-daemon/src/worker/commands.rs`), trait `SessionEngine` (`crates/pa-daemon/src/engine.rs`), son implémentation (`crates/pa-daemon/src/agent_engine/`), et la commande dans `pa-types` (ou des champs `rest`) ;
  - module du pont, comme en A.
- **Plus de fichiers upstream qu'en A** (règle 2). Couche providers non touchée.
- **Point à vérifier** : les messages pris par la boucle sont enregistrés par `message_end`, comme les lignes de prompt. Le TUI et les avis du worker n'en sont pas touchés : ils gardent la file du worker.

**R4-C : statu quo** (rappel, pas R4 au sens du CDC) : le message attend la fin du tour ACP dans la file de l'extension (§3.3).

Options communes à A et B, à trancher :
- **S1, forme de la requête** : celle d'Unharnessed (`{ sessionId, prompt, turn }`, réponse `{}`, `-32603` et `-32602`). Unharnessed a tranché.
- **S2, `rewindTo`** dans `turnStart` `steer` : absent tant que `_yus/session/rewind` n'est pas servi (étape 6 du ticket 24), ou fourni dès maintenant.
- **S3, marque `steer` dans le fichier de session** pour le rejeu (U5) :
  - le pont ne peut pas écrire d'entrée personnalisée : aucune commande du démon ne le fait (`AppendCustomMessage` écrit un **message**, qui irait au modèle) ;
  - sans marque, `session/load` rejoue les messages comme des prompts.
  - Choix : nouvelle commande, ou pas de marque pour l'instant.
- **S4, « regroupés en un seul bloc »** (critère YA-19) : N lignes user dans un seul tour (upstream, Unharnessed), ou un seul message concaténé (changement à nous).
- **S5, `steer` pendant un tour lancé par l'agent** (réveil, TUI attaché) : accepté, ou refusé, auquel cas l'extension garde le message en file. Le cas du Stop (§5, `AbortAndSendQueued`, doublon possible) en dépend.
- **S6, mode `steering`** : `all`, défaut du worker et d'Unharnessed, ou exposé dans les réglages.

### 6.2 X4 (extension)

- `_yus/session/steer` passe dans les requêtes servies (`src/ya/methods.ts:10-30`), donc `features().steer` vaut vrai. Le bouton « Send now » apparaît pendant un tour (`Composer.tsx:901`), et Cmd/Ctrl+Entrée envoie le message dans le tour.
- Rien d'autre à reprendre : `now`, `steer`, `joined`, `requeue` et `pause` sont déjà là (§3.2).
- Agent factice (`test/fake-agent/fake-agent.mjs`) : il sert `steer` et publie `turnStart` `steer`, aligné sur une capture (`compare_frames.py`).
- **Préfixe** : inchangé par X4 (trames ACP seulement). Preuve : `extension_frames_check.py`.

### 6.3 Tests à voir rouges puis verts

**Rust**, e2e du pont, nouveau fichier sur le modèle de `crates/pa-cli/tests/ya_acp_wake_turn_e2e.rs` : vrai binaire en ACP, provider factice, vrai kernel, aucun modèle.

1. **`steer` pendant une cellule lente** (marqueur de début, puis `sleep`) :
   - réponse `{}` ;
   - la cellule va au bout : son marqueur de fin est écrit ;
   - la requête suivante au provider contient les résultats du tool puis le message ;
   - `turnStart` `steer` sous le même `promptTurnId` ;
   - **une seule** réponse `session/prompt`, après le tour du message.
   - Rouge sur `main` : `-32601`.
2. **Deux `steer` pendant la cellule** : une seule requête au provider avec les deux lignes user (ou un seul message, selon S4).
3. **`steer` avec un `turn` périmé, ou après la fin** : `-32603`. Bloc invalide ou commande : `-32602`.
4. **Stop avec un `steer` accepté et non pris** : pas de frontière ; le message n'est pas joué après le Stop (file du worker vide, ou file de l'agent vidée) ; le prompt suivant passe.
5. **Préfixe** : les requêtes N et N+1 autour de l'injection ont le même prompt système, les mêmes tools et un historique en ajout seul, vérifiés comme `assert_prefix_stable`.
6. **Option B seulement** : un message non pris ne passe pas au run suivant.

**`prefix_stability.rs`** : scénario « message pendant une cellule » dans le moteur `pa-core`. Couvre la voie B ; la voie A n'y passe pas.

**Extension** :
- `agent.test.ts:86` passe de « rejet sur place » à « envoyé » ;
- `suite.ts` (agent factice) : « Send now » visible pendant un tour, puis tour rejoint, puis une seule fin ; refus, puis message en tête de file ;
- `suite-real.ts` : même parcours contre le vrai binaire.

**Banc de capture** : nouvelle session dans `acp_frames.py`, ou script dédié comme `wake_turns.py`, avec `steer` pendant une cellule lente. Sorties attendues : `CONFORME` et préfixe N → N+1 exact dans `analyze.py`. Ensuite, `compare.py --all` entre `main` et la branche donne `TOUT IDENTIQUE` sur les cas existants.

### 6.4 Contrôle manuel prévu

Binaire de la branche, extension de la branche, scénario de Yusuf :

1. Lancer une cellule longue, par exemple `await bash("sleep 20")`.
2. Pendant qu'elle tourne, envoyer un message par Cmd/Ctrl+Entrée ou par « Send now ».
3. Vérifier :
   - la cellule va au bout ;
   - le message s'affiche comme un tour sous le tour en cours ;
   - l'agent y répond sans nouveau prompt ;
   - le message ne part pas une seconde fois.
4. Deux messages pendant la cellule : une seule requête au modèle (journal de requêtes, ou capture).
5. Stop avec un message envoyé et pas encore pris : le message revient en tête de file, en pause, puis part au « play », une seule fois.

## 7. Hypothèses (non vérifiées)

- **Course en §2.3** : le run d'un `steer` est rattaché au prompt du pont ou ouvert comme un nouveau tour selon l'ordre des événements. Non mesuré.
- **Doublon au Stop d'un tour lancé par l'agent** (§5) : non mesuré.
- **Lignes de préfixe à chaque relance de run** dans la voie A (§4.1) : non mesuré.
- **Forme identique de la requête suivante** entre A et B (§4.1) : déduit de la lecture.
- **Repérage du message dans les événements du worker** (option A) : non vérifié.
- **Enregistrement des messages pris par la boucle** dans la voie B : non vérifié.

## 8. Choix à soumettre à Yusuf

1. **R4-A ou R4-B** :
   - Unharnessed a tranché pour B (injection dans le run en cours) ;
   - le ticket 24 prévoyait la commande `Steer` du démon (A) ;
   - B touche plus de fichiers upstream (règle 2).
2. **S1 à S6** (§6.1).
3. **Ordre** : R4 puis X4 dans deux PR, comme au ticket 24, ou une seule.

## 9. Décisions de Yusuf (25/11/2026)

- **D1 : R4-A**, la file `steering` du worker. C'est un écart à Unharnessed (U1) : nous avons le démon et sa file, que l'upstream teste et que le TUI utilise. B ajouterait des lignes upstream à quatre endroits.
  - D1 est soumis à deux conditions, à vérifier avant le code, avec `fichier:ligne` :
    - (a) le pont garde le tour ACP ouvert jusqu'au run du message, sans toucher upstream ;
    - (b) la relance n'ajoute rien au contexte du modèle par rapport à une injection dans le même run.
  - Si (a) demande de toucher upstream, ou si (b) est faux : arrêt, et l'on passe à B. Réponse au §10.
- **D2 (S1)** : forme d'Unharnessed, `{ sessionId, prompt, turn }`, réponse `{}`, refus en `-32603` et `-32602`.
- **D3 (S2)** : pas de `rewindTo`. Il faut vérifier que l'extension ne l'envoie pas en dehors du retour en arrière (§10.3).
- **D4 (S3)** : pas de marque `steer` pour le rejeu. **Limite** : après `session/load`, un message envoyé pendant un tour se rejoue comme un prompt.
- **D5 (S4)** : N messages user dans un tour. Le critère de YA-19 est corrigé dans `docs/yusAgent-cdc.md` (critère et journal des décisions).
- **D6 (S5)** : un message envoyé pendant un tour lancé par l'agent est refusé ; l'extension le garde et l'envoie en prompt après le tour. **Limite** : pendant un réveil ou un tour d'un autre client, le message attend la fin du tour.
- **D7 (S6)** : mode `all` fixe, pas de réglage.
- **D8** : une seule PR (R4 et X4), un seul contrôle manuel dans VS Code.

## 10. Conditions de D1 : (a) et (b)

Lignes de `main` à `5661bdebc`.

### 10.1 (a) Le tour ACP reste ouvert jusqu'au run du message

**Ce qui tient sans aucune ligne nouvelle :**

- Après la réponse de `PromptAndWait`, le pont appelle `fetch_autonomous_status(…, false)` (`crates/pa-daemon/src/acp/daemon.rs:1708-1710`).
  - Cet appel envoie `WaitForHeadlessCompletion` (`daemon.rs:1894-1906`).
  - Le worker y répond seulement quand la session est au repos **et que ses deux files sont vides** : `wait_until_idle`, `crates/pa-daemon/src/worker/lifecycle.rs:638-653`, condition en `:645-650`, appelé par `handle_wait_for_headless_completion`, `:712-723`.
- La boucle de fin rappelle la même attente avec `true` (`daemon.rs:1771`), puis appelle `finish_prompt` (`daemon.rs:1808`).
- Le commentaire d'upstream le dit : les suites mises en file « run inside the waits above » (`daemon.rs:1711-1712`, `:1835-1841`).
- **Conséquence** : un message posé dans la file `steering` du worker avant la fin de cette attente tourne **avant** la frontière et la réponse `session/prompt`.
  - Ses événements gardent l'identifiant du tour du prompt : `AgentTurns::open` ne rouvre rien tant que `active_prompt_turn` vaut ce tour (`crates/pa-daemon/src/acp/ya_turns.rs:100-103`).
  - La raison d'arrêt lit le dernier message de l'assistant (`daemon.rs:1835-1854`).
- **`turnStart` `steer`** : le hook `ya_turns.announce` est appelé pour chaque événement (`daemon.rs:407-410`). On peut y publier le `turnStart`, dans notre module, sans nouvelle ligne dans `daemon.rs`. L'état des messages acceptés irait dans `AgentTurns`, notre structure, déjà présente comme champ (`daemon.rs:293`).

**Ce qui demande des lignes dans `daemon.rs`, fichier upstream du pont :**

1. **Servir la méthode** : une branche de dispatch à côté de celles de `ya_history` et `ya_resources` (`daemon.rs:1150-1162`). Sinon la réponse reste `-32601` (`daemon.rs:1165-1172`). B en a besoin aussi.
2. **Fermer la fenêtre de fin de tour.** Un message accepté après la dernière attente de la boucle (`daemon.rs:1771`) et avant `finish_prompt` (`:1808`) ne serait pas attendu.
   - Il tournerait comme un tour à part, `origin: "prompt"` (`ya_turns.rs:104-111`).
   - L'extension remettrait le message en tête de sa file et l'enverrait de nouveau (`extensions/vscode/src/chat.ts:673-675`, `:491-498`) : **doublon**.
   - Sans hook dans `prompt_turn`, notre module ne peut pas savoir où en est la fin du tour.
   - **Ce que j'ai examiné** : le pont tient son verrou d'état pendant la vérification et l'envoi au worker, et refuse si le worker est au repos. Cela réduit la fenêtre sans la fermer : le worker peut passer au repos entre la lecture de son état et l'ajout en file, et `prompt_turn` aller ensuite de son dernier verrou (`daemon.rs:1783`, `:1798`) à `finish_prompt` sans nouvelle attente.
   - **Fermeture complète** : un appel de notre module dans `prompt_turn`, avant la frontière (vers `daemon.rs:1736`). Il bloquerait de nouveaux messages sous le verrou d'état, puis attendrait encore la file si un message est arrivé depuis la dernière attente. Cet emplacement garde aussi l'ordre des trames : sans lui, un message accepté entre `:1708` et `:1729` tournerait dans l'attente de `:1771`, **après** la frontière et l'enveloppe (`:1745-1764`). L'extension ne fermerait pas le tour pour autant (`chat.ts:1051` ne ferme pas `this.sent`).
3. **Hors fin de tour, à concevoir** : entre la publication de `turnStart` `prompt` (`daemon.rs:1621`) et l'ajout du prompt en file (`:1625-1658`), le worker peut être au repos.
   - Un message accepté à ce moment-là entrerait dans la file `steering` **avant** le prompt : ajout en ordre d'arrivée à priorité égale (`crates/pa-daemon/src/worker/input.rs:136-139`, `crates/pa-daemon/src/worker/queue.rs:126`).
   - L'extension peut envoyer un message dès ce `turnStart` (`chat.ts:796-802`, `steerAll`).
   - Le pont doit attendre la ligne user du prompt avant d'envoyer le message au worker. Cela se fait dans notre module (`announce` voit cette ligne), sans ligne upstream.

**Verdict sur (a).** Garder le tour ouvert ne coûte rien. Mais, au sens strict, (a) ne tient pas : un R4-A sans doublon demande **deux** ajouts dans `daemon.rs` (dispatch et un appel avant la frontière), et aucun dans le worker, le cœur ou `pa-types`. Le dispatch est commun à A et B. L'appel dans `prompt_turn` est propre à A.

### 10.2 (b) La relance n'ajoute rien au contexte, comparée à une injection dans le même run

Chemin de la relance :
- le worker ajoute ses lignes de préfixe (`crates/pa-daemon/src/worker/turn.rs:1036-1047`) ;
- puis le moteur admet le lot comme un prompt (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:364-391` → `prompt_with_images`, `crates/pa-core/src/session_engine/admission.rs:79`, lignes du prompt `:144-163`).

Une injection dans le même run ne passe par rien de tout cela : la boucle ajoute seulement les messages (`crates/pa-agent/src/agent_loop/run.rs:163-177`).

**Cas courant**, sans rien en attente : la requête suivante contient [résultats des tools, N messages user] dans les deux voies. Identique d'après la lecture, à mesurer au banc.

**Ce que seule la relance ajoute à cette requête**, quand quelque chose est en attente. Ces lignes partiraient sinon au prompt suivant ; chacune est **ajoutée en fin**, sans casser le préfixe :

1. **Annonce de skills ou de MCP** (ticket 31) : à chaque admission, le worker compare le disque à ce que le modèle sait (`turn.rs:1041` ; `crates/pa-daemon/src/agent_engine/session_engine_impl.rs:1117-1123` ; `crates/pa-core/src/session_engine/ya_resource_notice.rs:377-401`, `:440`). Un changement fait pendant le tour est annoncé avec le message, et non au prompt suivant.
2. **Lignes « tour suivant » du cœur** (`admission.rs:149`, `:243-254`) :
   - avis de démarrage d'un kernel : restauration, skills Python indisponibles, erreurs de prompts de modèle (`crates/pa-core/src/session_engine/engine.rs:541-572`, `:768-778`). Un redémarrage du kernel pendant le tour y dépose son avis (`engine.rs:1127-1129`, `admission.rs:218-239`) ;
   - contexte d'objectif (`crates/pa-core/src/session_engine/goal_boundary.rs:48-49`).
3. **Lignes « tour suivant » du worker** après une reprise sur incident, `restore_next_turn` (`crates/pa-daemon/src/session_custom.rs:59`, lues en `turn.rs:1036-1039`).
4. **Digest du harness** : seulement s'il est encore dû (`crates/pa-core/src/session_engine/harness_digest.rs:445-452`, `:460-479` ; `engine.rs:1050-1056`). Il est consommé par la première admission, donc absent après le prompt du tour, sauf si les raffinements étaient coupés puis rallumés.

**En plus, hors contenu** : la fin d'un run annule le modèle de remplacement (`engine.rs:1114-1120`). **Corrigé le 25/11/2026 (§11)** : ce remplacement vient du **routage d'image**, pas d'une bascule de fournisseur. Un tour dont le prompt porte des images part sur le modèle d'images configuré quand le modèle de la session ne lit pas les images (`crates/pa-core/src/session_engine/wiring.rs:34-62`, `crates/pa-daemon/src/image_route.rs:180-196`). Dans ce cas, la relance repart sur le modèle de base (le message ne porte pas d'images), alors qu'une injection dans le même run garderait le modèle d'images.

**Verdict sur (b).** Faux au sens strict : les quatre cas ci-dessus, et le routage d'image, font différer la requête de la relance de celle d'une injection dans le même run. Dans le cas courant, rien n'est ajouté. Aucun de ces cas ne casse le préfixe, puisque tout est ajouté en fin.

### 10.3 D3 : l'extension et `rewindTo`

- L'extension n'envoie `rewindTo` qu'en paramètre `to` de `_yus/session/rewind` (`extensions/vscode/src/chat.ts:646`). Cet appel vient de `rewind()` (`:640-651`), lui-même appelé seulement pour modifier un prompt déjà envoyé (`:590`).
- `rewindTo` ne vient que d'un `turnStart` reçu (`chat.ts:782`, `:796-797`, `:832`).
- La requête `steer` n'envoie que `{ sessionId, prompt, turn }` (`chat.ts:464`).
- Sans `rewindTo`, le tour n'est pas modifiable (`webview/Chat.tsx:183`, `webview/App.tsx:158`) et `rewind()` s'arrête avant l'appel (`chat.ts:642-644`). `_yus/session/rewind` n'est de toute façon pas servi et est rejeté sur place (`src/ya/methods.ts:46`).
- **D3 tient.**

### 10.4 Arrêt

D1 demandait de s'arrêter si (a) touche upstream ou si (b) est faux. Au sens strict, les deux conditions tombent (§10.1, §10.2). **Pas de code, pas de plan de tests** avant ta décision : A avec ces deux écarts, ou B.

## 11. Décision de Yusuf (25/11/2026) : R4-A avec les deux écarts

- **Voie A** : le message passe par la file `steering` du worker.
- **Écart 1** : deux ajouts dans `crates/pa-daemon/src/acp/daemon.rs`, détaillés au §12.1 : la branche de dispatch, et un appel de notre module dans `prompt_turn` avant la frontière. Rien dans le worker, le cœur ni `pa-types`.
- **Écart 2** : la relance peut envoyer plus tôt des lignes en attente (annonce du ticket 31, avis du kernel, lignes de reprise, digest). Le steer du TUI se comporte déjà ainsi. Ces lignes sont ajoutées en fin de contexte, préfixe intact (§10.2).
- **Limite** : si le premier run est parti sur le modèle d'images (routage d'image), la relance repart sur le modèle de base (`engine.rs:1114-1120`), avec un miss de cache possible.
  - Ce qui déclenche ce remplacement : un prompt qui porte des images quand le modèle de la session ne les lit pas (`wiring.rs:34-62`).
  - L'extension n'envoie que du texte (`features().attachments` vaut `false`, `extensions/vscode/src/ya/methods.ts:51`). Le cas ne vient donc aujourd'hui que d'un TUI attaché.
- **Notre module attend la ligne user du prompt** avant d'envoyer un message dans la file (§10.1, point 3).
- **Correction du §10.2** : j'avais écrit « bascule de modèle (failover) ». C'est le routage d'image (§10.2 corrigé).

## 12. Plan (avant tout code)

### 12.1 Code

**Notre module** : nouveau fichier `crates/pa-daemon/src/acp/ya_turns/steer.rs`, déclaré dans `ya_turns.rs` par `pub(super) mod steer;`. C'est un fichier à nous : la déclaration ne touche pas `acp/mod.rs`. L'état d'un tour vit dans `AgentTurns` (ya_turns.rs, déjà un champ de l'état de session, `daemon.rs:293`) :
- tour du pont en cours ;
- ligne user du prompt vue ou non ;
- messages en envoi ;
- messages acceptés depuis la dernière attente ;
- file des messages acceptés, pour `turnStart` ;
- acceptation fermée ou non.

Remise à zéro dans `ya_turns::begin_prompt` (`ya_turns.rs:241`), appelé pour chaque prompt (`daemon.rs:1614`).

**Écart 1, les deux ajouts dans `daemon.rs`** (lignes de `main` à `5661bdebc`) :

| Ajout | Où | Lignes | Contenu |
|---|---|---|---|
| Dispatch | `daemon.rs:1163`, avant `"session/set_config_option" =>`, dans `handle_incoming` (`:1110`) | **+4** (un commentaire et une branche de 3 lignes après rustfmt) | `super::ya_turns::steer::METHOD => { super::ya_turns::steer::serve(id, params, link, state, tx).await; }` |
| Fermeture | `daemon.rs:1710`, juste après `fetch_autonomous_status(…, false)…ok();` (`:1708-1710`), donc avant la frontière (`:1736-1752`) | **+2** (un commentaire et un appel) | `let autonomous_status = super::ya_turns::steer::seal(state, link, autonomous_status).await;` |

Le `METHOD` vaut `"_yus/session/steer"`. Au total, **+6 lignes et 0 modifiée** dans un fichier upstream. Ces ajouts sont listés dans `docs/fork-changes.md`.

**`serve`** (requête `_yus/session/steer`, D2) :

1. **Contrôles sous le verrou d'état**, refus en `-32603` « The turn is over: send it as a prompt. » si l'un échoue :
   - session connue ;
   - pas de fermeture ;
   - pas d'arrêt en cours ;
   - tour du pont présent ;
   - `turn` égal à `active_prompt_turn` ;
   - acceptation pas fermée ;
   - aucun tour lancé par l'agent en cours (D6).
2. **Blocs** analysés comme un prompt (`types::AdmittedPrompt::parse`). Refus en `-32602` avec `data.details` dans trois cas :
   - bloc invalide ;
   - image : l'extension n'en envoie pas, §5.4 ;
   - commande de session (`crate::session_commands::parse_prompt_session_command`).
3. **Attente de la ligne user du prompt** :
   - le module relit l'état toutes les 50 ms, comme `ya_turns::begin_prompt` (`ya_turns.rs:31-32`) ;
   - il refuse si le tour finit entre-temps : prompt de commande de session sans ligne user, échec, arrêt.
4. **Envoi** :
   - le message est marqué « en envoi » sous le verrou ;
   - **le verrou est relâché avant l'envoi**. Les réponses du démon passent par la boucle d'événements (`daemon.rs:131`, `:463`), qui prend ce verrou pour chaque événement (`:368`). Les garder ensemble bloquerait tout ;
   - envoi de `DaemonCommand::Steer`.
5. **Après l'accusé de réception** :
   - si un arrêt a été armé entre-temps, le module envoie `AbortAndClearQueue`, la commande du Stop. Rien du message ne tourne après le Stop. Réponse `-32603` ;
   - sinon, il compte le message comme accepté et le met dans la file `turnStart`. Réponse `{}`.

**`seal`** (appelé une fois par prompt, après l'attente du worker) :
- s'il reste des messages en envoi, il attend ;
- si des messages ont été acceptés depuis l'attente précédente, il remet le compteur à zéro et relance `fetch_autonomous_status(…, false)` (`daemon.rs:1894`) ;
- sinon, il ferme l'acceptation et rend le dernier statut.

Les messages tournent donc avant la frontière, et le statut de l'enveloppe est à jour. Quand aucun message n'a été envoyé, aucune requête n'est ajoutée.

**`turnStart` `steer`** : dans `AgentTurns::announce`, appelé pour chaque événement (`daemon.rs:407-410`) :
- la première ligne user du tour du pont est celle du prompt (marquée « vue ») ;
- chaque ligne user suivante prend la tête de la file des messages acceptés et publie `turnStart { origin: "steer" }` sous le même `promptTurnId`, sans `rewindTo` (D3), avant les mises à jour de l'événement ;
- un lot de N messages donne N `turnStart`, comme chez Unharnessed.

Les lignes user ne donnent aucune trame (`wire_events.rs:585`) : l'extension affiche le texte qu'elle a envoyé (`chat.ts:822-850`).

**Limites** (notées au ticket et dans `docs/fork-changes.md`) :
- un message d'un TUI attaché pendant le même tour décalerait l'appariement de la file, qui suit l'ordre d'arrivée ;
- un `Steer` arrivé au worker après le `AbortAndClearQueue` du Stop pourrait commencer à tourner avant l'abandon envoyé par le module. Cela suppose un envoi bloqué pendant tout un aller-retour du Stop ; aucun test ne peut le provoquer ;
- D4 (pas de marque de rejeu) et D6 (refus pendant un tour lancé par l'agent).

**Extension (X4)** :
- `"_yus/session/steer"` passe dans `REQUESTS` (`extensions/vscode/src/ya/methods.ts:10-30`), donc `features().steer` devient vrai (`:45`) ;
- rien d'autre dans `chat.ts`.
- Agent factice : sert `steer` (réponse `{}` et refus), publie `turnStart` `steer` sur la ligne suivante.

**Préfixe** : prompt système et tools inchangés. Le message entre en fin d'historique (§4). Les trames ACP gagnent `turnStart` `origin: "steer"`.

### 12.2 Tests vus rouges puis verts

Ordre des pushes :
- **push 1, tests seuls** : CI rouge sur les assertions ci-dessous. Je relève le run, le job et la ligne de chaque assertion en échec ;
- **puis le code** : CI verte.

Aucun modèle n'est appelé : provider factice (`"engine": "faux"`) et vrai kernel, comme `crates/pa-cli/tests/ya_acp_wake_turn_e2e.rs:7`.

**Rust, e2e du pont** : nouveau fichier `crates/pa-cli/tests/ya_acp_steer_e2e.rs`, harnais copié de `ya_acp_wake_turn_e2e.rs` (`Client`, `wait_for`, `turn_start`, `phase`). Toutes ces assertions sont rouges sur `main`, où `steer` répond `-32601`, sauf indication contraire.

| # | Test | Assertion (rouge sur `main`, sauf indication) |
|---|---|---|
| E1 | `a_steer_joins_the_running_prompt_turn` | Le prompt lance une cellule qui écrit `started`, dort 3 s, puis écrit `ended`. Au fichier `started`, `steer` avec le `promptTurnId`. Attendu : réponse `{}` ; `ended` écrit, donc la cellule n'est pas interrompue ; dans le fichier de session, la ligne user du message suit le résultat de la cellule ; `turnStart` `{origin:"steer"}` sous le même `promptTurnId`, sans `rewindTo` ; la frontière et la réponse `session/prompt` arrivent après la réponse au message ; **une seule** réponse `session/prompt` |
| E2 | `two_steers_go_in_one_model_turn` | Deux messages pendant la cellule : les deux lignes user se suivent dans le fichier de session, avec une seule réponse de l'assistant après elles ; deux `turnStart` `steer` |
| E3 | `a_steer_is_delivered_once` | Après E1, un second prompt : le texte du message apparaît une seule fois en ligne user dans le fichier de session ; aucun `turnStart` `origin:"prompt"` hors des deux prompts envoyés |
| E4 | `a_steer_never_goes_before_the_prompt_row` | `steer` envoyé dès la trame `turnStart` `prompt`, avant que le worker ait le prompt. La ligne du prompt précède celle du message dans le fichier de session. Sans l'attente de la ligne, le rouge n'est pas garanti : c'est une course. Le rouge déterministe est U2 |
| E5 | `steer_refusals` | `-32603` pour un `turn` périmé, après la réponse `session/prompt`, et pendant un tour de réveil (un `bash()` non lu, comme l'e2e des réveils). `-32602` pour `/compact` et pour un bloc image |
| E6 | `stop_with_a_steer_waiting` | `steer` accepté pendant la cellule, puis `session/cancel`. Attendu : `stopReason` `cancelled` ; aucune ligne user du message dans le fichier de session ; aucun tour dans les 5 s qui suivent ; le prompt suivant tourne sans le message. Le vidage de la file vient du Stop (`AbortAndClearQueue`) : sur `main`, le refus `-32601` de `steer` fait déjà échouer l'assertion « réponse `{}` » |

**Rust, tests unitaires** dans `ya_turns/steer.rs`. `seal` et `serve` prennent l'attente et l'envoi en paramètres, ce qui permet de les remplacer par des faux dans ces tests. Rouge avec un `seal` ou un `serve` vides, vert ensuite :

| # | Test | Assertion |
|---|---|---|
| U1 | `seal_waits_again_for_a_message_accepted_after_the_wait` | **La fenêtre entre la dernière attente et `finish_prompt`** : un message est accepté après le retour de l'attente. `seal` relance l'attente une fois, puis ferme. Un `steer` suivant est refusé en `-32603` |
| U2 | `a_steer_waits_for_the_prompt_row` | Avant la ligne user du prompt, aucun envoi. Après `announce` de cette ligne, un envoi |
| U3 | `seal_waits_for_a_message_in_flight` | Un message en envoi : `seal` ne ferme pas avant son accusé de réception |
| U4 | `announce_maps_rows_to_steers` | La première ligne user ne publie rien. Les deux suivantes publient chacune `turnStart` `steer`, même tour, sans `rewindTo` |
| U5 | `a_stop_during_the_send_aborts_the_message` | Arrêt armé pendant l'envoi : le faux reçoit `AbortAndClearQueue`. Réponse `-32603` |

**Extension** :
- `test/unit/agent.test.ts:86` : `_yus/session/steer` part sur le fil au lieu d'être rejeté sur place. Rouge sur l'assertion actuelle inversée ;
- `test/e2e/suite.ts` (agent factice) :
  - « Send now » visible pendant un tour ;
  - Cmd+Entrée pendant un tour envoie un `steer` avec l'identifiant du tour ;
  - tour rejoint affiché ;
  - **une seule** trame `session/prompt` pour tout le parcours ;
  - refus : le message revient en tête de file, puis part en prompt après le tour ;
  - Stop avec un message envoyé et non pris : file en pause, puis « play » l'envoie une fois ;
- `test/e2e/suite-real.ts` (vrai binaire) : même parcours que E1, avec une seule trame `session/prompt` pour le message.

**Agent factice aligné** :
- `bench/tools/capture/acp_frames.py` reçoit la session `D-steer` : scénario `steer`, `steer` envoyé au début de la cellule, puis un second `steer` avec un `turn` périmé. `CONFORME` exige :
  - `{}` puis `-32603` ;
  - un `turnStart` `steer` sous le tour du prompt ;
  - une seule réponse `session/prompt`.
- `compare_frames.py` rejoue `D-steer` contre l'agent factice et compare réponses, refus, forme et place de `turnStart`.

### 12.3 Banc de capture, préfixe N → N+1

- Deux scénarios dans `bench/tools/capture/server.py` :
  - **`steer`** : le modèle lance une cellule qui écrit `started` puis dort 4 s ; la réponse suivante est du texte ;
  - **`steer-skills`** : la même cellule crée d'abord un skill sur le disque du projet, ce qui change les skills pendant le tour.
- `acp_frames.py` envoie le `steer` à l'apparition de `started` (sessions `D-steer` et `E-steer-skills`). Il passe les requêtes capturées dans `analyze.compare_pair` (`analyze.py:124-150`) et écrit la section « Préfixe N → N+1 ».
- `CONFORME` exige, pour chaque paire, prompt système, tools et paramètres identiques, et messages de N en préfixe exact de N+1. Il exige aussi :
  - `D-steer` : la requête après la cellule finit par [résultat de la cellule, ligne user du message] ;
  - `E-steer-skills` : la même requête finit par [résultat de la cellule, annonce `[skills-changed]` du ticket 31, ligne user du message]. C'est l'écart 2, vu au banc.
- **Avant et après** :
  - binaire de `main` : `D-steer` et `E-steer-skills` relèvent le refus `-32601` ;
  - binaire de la branche : sections ci-dessus ;
  - `compare.py --all` entre `main` et la branche donne `TOUT IDENTIQUE` sur les cas existants.
- Commandes notées dans le rapport, avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`.

### 12.4 Contrôle manuel (Yusuf, VS Code, D8)

Binaire de la branche (artefact `ya-build-macos-arm64`) dans `yusagent.binaryPath`, extension de la branche, ton modèle habituel.

1. Ouvrir un chat, passer en « auto ».
2. Prompt : « Lance `await bash("sleep 20; echo fini")` puis dis-moi ce que tu as vu. »
3. Pendant le `sleep`, taper « Ajoute aussi la date du jour à la fin » et faire **Cmd+Entrée**. Attendu :
   - le message passe en « Sending » au-dessus du compositeur ;
   - la cellule va au bout (« fini ») ;
   - un nouveau tour s'ouvre sous le premier, avec ton texte ;
   - l'agent y répond sans nouveau prompt.
4. Après la fin : le message **n'apparaît pas une seconde fois**, ni en file ni en tour.
5. Relancer le prompt 2. Pendant le `sleep`, envoyer deux messages (Cmd+Entrée deux fois). Attendu : deux tours rejoints, une seule réponse de l'agent qui traite les deux.
6. Relancer le prompt 2. Pendant le `sleep`, taper un message, **Entrée** (sans Cmd), puis cliquer « Send now » sur le message en file. Attendu : même effet qu'à l'étape 3.
7. Relancer le prompt 2. Pendant le `sleep`, Cmd+Entrée sur un message, puis **Stop** aussitôt. Attendu :
   - le tour s'arrête ;
   - le message revient en tête de file, file en pause ;
   - « play » l'envoie **une fois**, comme un prompt.
8. Pendant un tour lancé par l'agent (`bash()` non lu, réveil `[bash-done]`), Cmd+Entrée sur un message. Attendu : le message reste en file et part en prompt après le tour (D6).
9. Fermer et rouvrir la conversation (`session/load`) : les messages des étapes 3 et 5 sont rejoués comme des prompts (limite D4).
