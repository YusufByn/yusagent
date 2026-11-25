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
