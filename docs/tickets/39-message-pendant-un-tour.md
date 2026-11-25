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

## 13. Premier push : tests et squelette (préparé, pas encore poussé)

Décision de Yusuf du 25/11/2026 : plan du §12 validé. Le premier push contient les tests et un squelette (branche de dispatch dans `daemon.rs`, module `ya_turns/steer.rs` avec ses signatures, sans comportement). Chaque test doit compiler et échouer sur son assertion, ni sur la compilation ni sur `-32601`. Limites acceptées : TUI attaché, Stop pendant l'envoi, D4, D6 (§12.1 et `docs/fork-changes.md`).

### 13.1 Le squelette

- **`crates/pa-daemon/src/acp/ya_turns/steer.rs`** (nouveau) :
  - `METHOD` ;
  - l'état `Steers` et ses méthodes, qui rendent une valeur neutre : `gate` → `Send`, `seal_step` → `Closed`, `row` → `None`, `waiting` et `orphaned` → `false` ;
  - `steer_start()` ;
  - `serve`, qui répond `{}` sans rien envoyer au worker ;
  - `seal`, qui rend le statut reçu.
  - `#![allow(dead_code)]` et des `allow` clippy marqués « push 1 », retirés avec le code.
- **`ya_turns.rs`** : `pub(super) mod steer;` et le champ `steers` de `AgentTurns`.
- **`daemon.rs`** (+6 lignes) :
  - dispatch en `:1163-1166` ;
  - appel de `seal` en `:1721-1722`.

### 13.2 Écarts au plan du §12, faits en préparant le push

1. **Place de `seal`.** Le plan disait `daemon.rs:1710`. L'appel est **après** la vérification d'annulation qui suit l'attente (`:1713-1716` de `main`), soit `:1721` sur la branche, toujours avant la frontière. Raison : après un « close puis new », la session de l'état est une autre ; cette vérification renvoie déjà le tour en `cancelled` avant que `seal` puisse toucher l'état d'une autre session. Toujours deux lignes.
2. **Stop pendant l'envoi.** Les champs `cancelling` et `turn` de `HostedSession` sont privés à `daemon.rs` (`:276`, `:283`). Les lire demanderait une troisième modification upstream. Le module ne peut donc ni refuser un message pendant un Stop armé, ni envoyer `AbortAndClearQueue` après l'accusé de réception (plan §12.1, point 5).
   - Il surveille plutôt chaque message accepté (relecture toutes les 50 ms). Si le tour finit **sans `seal`** (Stop, échec), soit `active_prompt_turn` différent du tour alors que la fermeture n'a pas eu lieu, il retire le message de la file `steering` du worker :
     - `get_queue` ;
     - puis `mutate_queued_message` avec `delete`, sur l'index et le texte attendu (`crates/pa-daemon/src/queue_commands.rs:25-120`, `:172-177`).
   - Le retrait est ciblé : il ne touche ni aux avis `[bash-done]` ni aux messages d'un autre client.
   - S'il a déjà commencé, c'est la limite acceptée « Stop pendant l'envoi ».
   - Le même chemin couvre un tour **en échec** avec un message accepté.
   - U5 devient `a_turn_ended_without_its_close_orphans_its_messages`.
3. **E6 lit la file du worker** par le socket du démon (`list`, puis `get_queue`, comme `ya_acp_resume_e2e.rs:575-600`). Sans cela, E6 passerait avec le squelette : rien n'est envoyé, donc rien ne tourne après le Stop.
4. **Tests de l'extension.**
   - Le gros des tests va dans `test/unit/agent.test.ts` (vrai processus de l'agent factice, sans VS Code), qui tourne en local.
   - `suite.ts` garde le test du bouton « Send now » (webview).
   - `suite-real.ts` reçoit un test en fin de liste.
5. **`serve`** prend `&id`, `&params` et `&tx`.

### 13.3 Assertion sur laquelle chaque test doit échouer au premier push

| Test | Assertion rouge attendue | Cause |
|---|---|---|
| E1 `a_steer_joins_the_running_prompt_turn` | `no turnStart origin steer` (`unwrap_or_else` sur la position du `turnStart` `steer`). Avant : réponse `{}`, `end_turn` et `ended` passent | `serve` n'envoie rien |
| E2 `two_steers_go_in_one_model_turn` | `first: …` : la ligne `first note` absente du fichier de session | idem |
| E3 `a_steer_is_delivered_once` | `one row for the message` : 0 au lieu de 1 | idem |
| E4 `a_steer_never_goes_before_the_prompt_row` | `the note: …` : la ligne `early note` absente (l'ordre est vérifié ensuite) | idem |
| E5 `steer_refusals` | `a stale turn` : `None` (réponse `{}`) au lieu de `Some(-32603)` | `serve` répond `{}` à tout |
| E6 `stop_with_a_steer_waiting` | `the message waits in the worker's lane` : `[]` au lieu de `["waiting note"]` | rien dans la file |
| U1 `seal_waits_again_for_a_message_accepted_after_the_wait` | `seal_step` vaut `Closed` au lieu de `Rewait` | squelette neutre |
| U2 `a_steer_waits_for_the_prompt_row` | `gate` vaut `Send` au lieu de `WaitRow` | idem |
| U3 `seal_waits_for_a_message_in_flight` | `seal_step` vaut `Closed` au lieu de `WaitSending` | idem |
| U4 `announce_maps_rows_to_steers` | `waiting(one) && waiting(two)` faux | idem |
| U5 `a_turn_ended_without_its_close_orphans_its_messages` | `the turn ended without its close` : `orphaned` faux | idem |
| X0 `agent.test.ts`, « l'option Send now est servie » | `features().steer` : `false` au lieu de `true` | **liste des méthodes servies** |
| X1 `agent.test.ts`, « Cmd+Entrée : un steer… » | `un steer part sur le fil` : 0 au lieu de 1 | **méthode refusée sur place** (`-32601` local, `methods.ts`) |
| X2 `agent.test.ts`, « refusé par le pont » | `le steer est parti, le pont l'a refusé` : 0 au lieu de 1 | idem |
| X3 `agent.test.ts`, « Stop avec un message envoyé » | `un steer part sur le fil` : 0 au lieu de 1 | idem |
| `suite.ts`, « Send now pendant un tour » | `« Send now » visible pendant un tour` : `covered` au lieu de `clicked` | bouton caché tant que la méthode n'est pas servie (`Composer.tsx:901`) |
| `suite-real.ts`, « message pendant un tour (ticket 39) » | `le message suit le résultat, une réponse` : le message part en prompt après le tour | méthode refusée sur place |

Les sept tests de l'extension **ne peuvent échouer que parce que l'extension ne sert pas encore la méthode**. X4 consiste justement à l'ajouter à la liste servie (`src/ya/methods.ts`, une ligne). Tout le reste du côté client existe déjà (§3.2). Ils échouent bien sur une assertion, pas sur une erreur levée.

**Mesuré en local** (règle 10), commande :
```
cd extensions/vscode && npm run compile-tests && node --require ./out/test/unit/setup.js --test --test-timeout=60000 --test-name-pattern="message pendant un tour|Stop avec un message|méthode non servie" out/test/unit/agent.test.js
```
- Sur le code actuel : X0 à X3 en échec sur les assertions du tableau ; « méthode non servie » (rewind seul) passe.
- Avec la ligne de X4 appliquée le temps de l'essai, puis retirée : 5/5 verts.

Les tests Rust n'ont pas été compilés en local (règle 8). La CI le fera.

## 14. Écart 2 du §13.2 : identification du message et tour en échec (25/11/2026)

Demande de Yusuf : (1) dire comment le module identifie le message à retirer de la file du worker, et s'arrêter avant le code si c'est par le texte ; (2) vérifier qu'après un tour en échec avec un message accepté, l'extension le garde, ni perdu ni doublé, et ajouter un test E7 si le provider factice sait faire échouer un tour. Les lignes de `daemon.rs` sont celles de la branche (`650418714`, +4 lignes après `:1162`). Les autres sont celles de `main`.

### 14.1 Identification : par le texte, donc arrêt avant le code

Le retrait prévu au §13.2, point 2, passe par `mutate_queued_message` `delete`, qui vise **un index et le texte attendu**. Le worker refuse si l'aperçu à cet index diffère du texte (`crates/pa-daemon/src/queue_commands.rs:168`). Un message identique d'un autre client (TUI attaché) dans la même file pourrait donc être supprimé à la place du nôtre. **Pas de code écrit pour ce retrait.**

Ce que le démon offre d'autre :

- **Un identifiant d'admission** :
  - un `prompt` ou un `prompt_and_wait` qui porte `admissionId` l'enregistre dans le worker et le garde sur l'élément en file (`crates/pa-daemon/src/worker/input.rs:66-76`, `:125`) ;
  - `cancel_prompt_admission { admissionId, cancelOwned: true }` retire de la file **les seuls éléments de cet identifiant**, ou arrête leur run s'il a commencé (`crates/pa-daemon/src/prompt_admission.rs:556-604`).
- **Mais le superviseur oublie l'identifiant quand la route se règle** (`prompt_admission.rs:315-321`). Pour un `prompt` sans attente, c'est dès l'acceptation : une annulation ultérieure répond `unknown` sans atteindre le worker (`:377-382`). Il faut donc envoyer le message en `prompt_and_wait`, qui ne répond qu'à la fin de son run.
- **Le lien du pont coupe ses requêtes à 30 s** (`request`, `daemon.rs:46`, `:173-175`). La variante sans délai, `request_until_close`, est privée (`daemon.rs:191-193`). Il faudrait la passer en `pub(crate)`, soit **une ligne modifiée de plus dans `daemon.rs`**, troisième touche upstream.
- **Une connexion au démon à part**, ouverte par notre module : pas de ligne upstream, mais un client du démon à réécrire (poignée de main, enveloppes). Lourd.

Options, à trancher par Yusuf :
- **(a) Identifiant d'admission.**
  - Le message part en `prompt_and_wait` avec `streamingBehavior: steer` et un `admissionId` à nous (UUID).
  - `request_until_close` passe en `pub(crate)` (+1 ligne modifiée dans `daemon.rs`).
  - `seal` attend que les `prompt_and_wait` des messages aient répondu, c'est-à-dire que leurs runs soient réglés. Le compteur « depuis la dernière attente » n'est plus nécessaire.
  - Le retrait se fait par `cancel_prompt_admission`, qui arrête aussi le run s'il a commencé.
  - `{}` part à l'envoi : il n'y a pas d'accusé de mise en file. Un refus du worker fait répondre le `prompt_and_wait` en échec ; l'extension renvoie le message à la fin du tour, sans perte ni doublon.
- **(b) Texte et index.** Comme préparé, avec le risque que tu as nommé.
- **(c) Aucun retrait.** On s'en remet au vidage de la file par le Stop (`AbortAndClearQueue`). Il reste la course déjà acceptée (« Stop pendant l'envoi »). Aucune ligne upstream, aucun texte. Le §13.2, point 2, est retiré, et U5 redevient le seul test de l'état « tour fini sans fermeture » (sans retrait).

### 14.2 Tour en échec avec un message accepté

Faits, lus dans le code :

- **Le provider factice sait faire échouer un tour** : `{"stopReason": "error", "errorMessage": "…", "delayMs": N}` (`crates/pa-ai/src/providers/faux/script.rs:124-165`). Les relances automatiques sur une erreur scriptée ne sont pas vérifiées.
- **Pont** : un `PromptAndWait` en échec fait sortir `prompt_turn` avant `seal` (`daemon.rs:1688-1705` sur la branche : frontière d'erreur, `finish_prompt`, réponse d'erreur).
- **Worker** : un échec ne suspend pas la file. Seuls les abandons et la compaction la suspendent (`crates/pa-daemon/src/worker/commands.rs:335`, `:693`, `crates/pa-daemon/src/worker/lifecycle.rs:460`). Le `TurnRunner` prend la file `steering` dès le règlement du run en échec (`crates/pa-daemon/src/worker/turn.rs:62-72`). Le message part donc dans **son propre run**, presque sûrement avant tout retrait du pont, qui n'agit qu'après la réponse d'échec et `finish_prompt`. Ce run est vu comme un tour lancé par un autre client (`origin: "prompt"`, E4, `ya_turns.rs:104-111`, `:332-352`).
- **Extension** : un tour en échec passe par `end(…, "failed")` (`extensions/vscode/src/chat.ts:618-621`), puis `requeue()` et `next()` (`:673-675`). Le message envoyé dans le tour est **renvoyé aussitôt en prompt**, sans pause.
- **Conséquence avec la voie A** : le message tourne une fois dans le worker **et** une fois au renvoi de l'extension. C'est un doublon, quelle que soit l'identification : (a) arrête le run s'il a commencé, mais sa ligne user est déjà enregistrée au début du run.

**E7 tel que demandé** (« message retiré de la file du worker, rien ne tourne, l'extension le garde ») **n'est pas atteignable avec la voie A sans changer le worker** (garder la file après un run en échec) **ou l'extension**. Je ne l'ai pas écrit : il resterait rouge.

Options, à trancher par Yusuf :
- **(F1) Extension** : après un tour en échec, les messages envoyés dans le tour reviennent en tête de file **en pause**, comme après un Stop, au lieu d'être renvoyés. La livraison du worker est alors la seule copie automatique. Doublon seulement si l'utilisateur relance, comme la limite « Stop pendant l'envoi ». Changement dans `chat.ts` (code repris d'Unharnessed), quelques lignes. E7 devient : « tour en échec : le message tourne une fois (worker), l'extension le garde en pause ». Rouge avec le squelette sur « le message tourne une fois » (rien n'est envoyé).
- **(F2) Voie B** : Unharnessed vide la file de l'agent à la fin du tour, dans le même processus (`uh:crates/uh-daemon/src/acp/session.rs:126-132`, `prompt.rs:975`). Pas de doublon, mais retour sur D1.
- **(F3) Limite acceptée** : doublon possible après un tour en échec.

## 15. Décisions de Yusuf sur le §14 (25/11/2026) et condition de F4

### 15.1 Décisions

1. **Option (c)** : aucun retrait de la file du worker. Le Stop la vide déjà (`AbortAndClearQueue`). La course « Stop pendant l'envoi » reste la limite acceptée. Le code de surveillance et de retrait par texte prévu au §13.2, point 2, est abandonné : `Steers::orphaned` et la partie « orpheline » de U5, déjà poussés dans le squelette, seront retirés avec le code.
2. **Option F4** (ni F1, ni F2, ni F3) : un message accepté par le worker est considéré comme livré. Après un tour en échec, l'extension ne renvoie pas et ne remet pas en file les messages acceptés par `_yus/session/steer` dans ce tour ; le worker les fait tourner. F1 est écarté : il remettrait en pause un message déjà lancé.
   - **Condition à vérifier avant le code** : le run lancé par le worker après l'échec doit s'afficher dans l'extension, avec la réponse du modèle, par le chemin des tours lancés par l'agent (ticket 25). Sinon : arrêt, réouverture de D1 (voie B), avec le coût exact de B et la liste des limites d'A qu'il garde.
   - Si la condition tient : test E7.

### 15.2 La condition ne tient pas : course entre la réponse d'échec et le run suivant

Lecture de `650418714` (lignes de la branche pour `daemon.rs`, de `main` ailleurs) :

1. **Côté worker, deux tâches.**
   - Le `TurnRunner` règle le run en échec, envoie son résultat au `prompt_and_wait` (`crates/pa-daemon/src/worker/turn.rs:1182-1184`), puis reprend aussitôt sa boucle et lance le message de la file `steering` (`turn.rs:62-72`).
   - La réponse du `prompt_and_wait` part d'une autre tâche, celle de la commande, quand elle reçoit ce résultat (`crates/pa-daemon/src/worker/input.rs:155-161`).
   - L'`agent_start` du run suivant peut donc partir avant la réponse d'échec.
2. **Côté pont, deux tâches.**
   - La boucle d'événements transmet la réponse à `prompt_turn` par un canal (`crates/pa-daemon/src/acp/daemon.rs:463-467`) et continue.
   - `prompt_turn` appelle `finish_prompt` seulement après la réponse, sur son chemin d'échec (`daemon.rs:1688-1697`).
3. **Si l'`agent_start` du run suivant passe avant `finish_prompt`**, `AgentTurns::open` le rattache au prompt en échec et n'ouvre pas de tour (`crates/pa-daemon/src/acp/ya_turns.rs:100-103`). La suite :
   - pas de `turnStart` propre, et ses premières mises à jour portent l'identifiant du tour en échec ;
   - après `finish_prompt`, ses mises à jour portent le tour 0 ;
   - son `agent_end` ne ferme rien : aucun tour en cours, donc pas de frontière (`ya_turns.rs:142-153`) ;
   - côté extension, le tour en échec est déjà fini à la réponse d'erreur (`extensions/vscode/src/chat.ts:618-621`). Une mise à jour du tour 0 sans tour vivant est ignorée (`chat.ts:725-726`). **La réponse du modèle ne s'affiche pas.**
4. **Si `finish_prompt` passe avant**, le run s'ouvre comme un tour lancé par l'agent (`turnStart` `origin: "prompt"`, E4) et s'affiche. Rien ne garantit cet ordre.

**Arrêt** (décision 2) : la condition ne tient pas. **Pas de code, pas de E7.**

### 15.3 Coût exact de la voie B

Principe :
- Le pont envoie le message au worker par la commande `Steer` existante, avec un champ en plus (`rest`, par exemple `yaInRun: true`). Ni `pa-types`, ni le superviseur, ni `protocol.rs` ne sont touchés. Que le superviseur transmette bien ce champ au worker est **à vérifier**.
- Le worker, sous le verrou de son état (`core`), vérifie qu'un run tourne. Il passe le message à la file de l'agent du run (`Agent::steer`, `crates/pa-agent/src/agent.rs:994-1001`). La boucle le prend après les résultats des tools, **dans le même run** (`crates/pa-agent/src/agent_loop/run.rs:163-177`).
- Le moteur transmet déjà ces lignes user en cours de run au pont et au fichier (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:234-245`, `EngineEvent::UserMessage`). Aucun changement là.
- Au règlement du run, le worker vide la file de l'agent (`Agent::clear_steering_queue`, `agent.rs:1018`) **sous le même verrou** que `core.busy = false` (`turn.rs:1076-1104`). Le vidage est atomique avec la vérification du worker, comme `drop_steers` chez Unharnessed.

| Fichier upstream | Où (ligne d'insertion) | Lignes | Contenu |
|---|---|---|---|
| `crates/pa-daemon/src/acp/daemon.rs` | `:1163-1166` (déjà sur la branche) | +4 | Dispatch, comme en A. L'appel de `seal` (+2) est retiré : le `prompt_and_wait` du prompt couvre le run. |
| `crates/pa-daemon/src/worker.rs` | après `:49` (`mod commands;`) | +2 | `mod ya_steer;` (module du worker, à nous) |
| `crates/pa-daemon/src/worker/input.rs` | après `:167`, dans `handle_queue`, avant `resume_queued_input` | +4 | Un `Steer` avec `yaInRun` part vers notre module |
| `crates/pa-daemon/src/worker/turn.rs` | avant `:1100` (`core.busy = false`, verrou tenu) | +2 | `settle_engine.ya_drop_steers();` |
| `crates/pa-daemon/src/engine.rs` | après `:428` (`ya_resource_notice`) | +10 | Deux méthodes du trait `SessionEngine` avec défaut neutre : `ya_steer` (refus) et `ya_drop_steers` |
| `crates/pa-daemon/src/agent_engine/session_engine_impl.rs` | après `:1123` | +8 | Les deux méthodes pour `AgentSessionEngine`, qui délèguent à notre module |
| `crates/pa-daemon/src/agent_engine.rs` | après `:78` | +2 | `mod ya_steer;` (module du moteur, à nous) |

**Total B : +32 lignes dans 7 fichiers upstream, 0 modifiée.** A : +6 lignes dans 1 fichier. Lignes prévues, à confirmer à l'écriture. Couche providers, `pa-core`, `pa-types`, superviseur : non touchés.

Point technique à traiter en B : `ya_resource_notice` prend le verrou de la session avec `blocking_lock` (`session_engine_impl.rs:1118`), appelé hors du runtime. Une commande du worker tourne dans le runtime : il faudra `try_lock`, ou garder une poignée sur l'agent.

Limites d'A, une à une, avec B :
- **TUI attaché** (`turnStart` rattachés dans l'ordre des lignes) : **gardée**.
- **Stop pendant l'envoi** : **disparaît**. La vérification du worker et le vidage au règlement sont sous le même verrou, et le message ne passe jamais par la file `steering`.
- **Tour en échec** : **plus de doublon**. Le message non pris est vidé au règlement ; l'extension le renvoie en prompt à la fin du tour, comme chez Unharnessed. F4 et le changement de `chat.ts` deviennent inutiles.
- **Écart 2 (lignes en attente envoyées plus tôt)** et **routage d'image** : **disparaissent**, puisque c'est le même run.
- **D4, D6** : **gardées**.
- **Nouveau en B** (comme chez Unharnessed) : un message accepté après la dernière lecture de la file par la boucle est vidé au règlement. L'extension le renvoie en prompt après le tour. Ni perte ni doublon.

Tests avec B : E1 à E5 et les tests de l'extension restent valables. E6 n'a plus de file `steering` à lire : son assertion rouge serait à revoir. U1 à U5 suivent le nouveau module (plus de `seal`). Cache : même forme de requête (§4).

## 16. Relevé du premier push (`650418714`)

Runs `ci` 45628774848 et `ya-extension` 45628774814. Compilation et clippy verts (`check + clippy (workspace)`), donc les tests compilent. Chaque test du ticket échoue sur l'assertion prévue au §13.3. Aucun autre test ne casse.

| Test | Run / job | Ligne du journal | Échec |
|---|---|---|---|
| E1 à E6 (`ya_acp_steer_e2e.rs`) | 45628774848 / 139256217802 (shard 5/8) | 1291-1349 | E1 `:436` « no turnStart origin steer » ; E2 `:506` « first: … » ; E3 `:545` 0 ≠ 1 ; E4 `:580` « the note: … » ; E5 `:606` `None` ≠ `Some(-32603)` ; E6 `:649` `[]` ≠ `["waiting note"]`. 0 passés, 6 en échec |
| U1 à U5 (`steer.rs`) | 45628774848 / 139256217738 (shard 3/8) | 1562-1566, 2748-2786 | U1 `:174` `Closed` ≠ `Rewait` ; U2 `:184` `Send` ≠ `WaitRow` ; U3 `:196` `Closed` ≠ `WaitSending` ; U4 `:214` `waiting` faux ; U5 `:239` « the turn ended without its close ». 1249 passés, 5 en échec |
| X0 à X3 (`agent.test.ts`) | 45628774814 / 139256002771 | 330-417, 722-725 | 74 tests, 70 passés, 4 en échec (les quatre du ticket) |
| `suite.ts` « Send now » | 45628774814 / 139256002771 | 1055-1068 | `covered` ≠ `clicked` ; e2e 15/16 |
| `suite-real.ts` | 45628774814 / 139256003226 | 998-1014 | « le message suit le résultat, une réponse » ; e2e vrai binaire 18/19 |
| `Check changelog fragment` | 45628774476 | — | Aucun fragment `.changes/` : il viendra avec le code |

## 17. Décision de Yusuf : voie B (25/11/2026), vérifications avant le code

Décision : **D1 rouvert, voie B** (file de l'agent dans le run en cours, comme Unharnessed). A est abandonné : après un tour en échec, la réponse au message pourrait ne pas s'afficher (§15.2), défaut visible non acceptable en limite. Arrêt avant le code si le point 1 ou le point 2 échoue, ou si B demande plus de lignes upstream que le tableau du §15.3.

### 17.1 Point 1 : le superviseur transmet le champ ajouté à `Steer`

**Tient.**
- `Steer` garde les champs inconnus dans `rest` (`#[serde(flatten)]`, `crates/pa-types/src/daemon/command.rs:377-386`).
- Il passe par la route générique du superviseur (`crates/pa-daemon/src/supervisor/routing.rs:623-624`). Le paquet envoyé au worker y est reconstruit par `client_command_payload`, qui resérialise toute la commande, `rest` compris (`crates/pa-daemon/src/supervisor/clients.rs:36-43`).
- Le worker reçoit donc le champ dans le `payload` de `handle_queue` (`crates/pa-daemon/src/worker/input.rs:164`).

### 17.2 Point 2 : verrou de session, accès direct à l'agent

**Option : accès direct à l'agent par `turn_agent`, sans `try_lock` ni `blocking_lock` sur la session.**
- Le moteur garde déjà une poignée sur l'agent vivant, « mirrored because a running turn holds the core session's mutex » (`crates/pa-daemon/src/agent_engine.rs:156-158`).
- C'est un `std::sync::Mutex`, tenu seulement le temps d'un clone ou d'une affectation : posé à la construction (`crates/pa-daemon/src/agent_engine/lifecycle.rs:426-427`), vidé au démontage (`:770`), lu par `abort_in_flight_turn` et `set_queue_modes` (`session_engine_impl.rs:1486`, `:1505`), et par le routage d'image (`crates/pa-daemon/src/image_route.rs:246`, `:516`).
- Aucune de ces sections ne prend un autre verrou ni n'attend : `lock()` passe en quelques microsecondes. Rien de bloquant dans le runtime asynchrone.
- **Un message n'est jamais refusé à cause de ce verrou** : on attend sa libération, très brève, au lieu de refuser comme le ferait `try_lock`.
- **Refus seulement si aucun run ne tourne** : poignée vide (moteur pas encore construit, ou démonté), ou worker au repos (`core.busy` faux, sous le verrou de `core`). La réponse est alors `-32603` « The turn is over: send it as a prompt. », et l'extension garde le message pour le prompt suivant (`chat.ts:470-478`).
- Ordre des verrous : `core` (worker), puis `turn_agent`. `turn_agent` n'est jamais tenu pendant la prise de `core`, donc pas d'interblocage.
- Le vidage au règlement (`turn.rs:1100`) prend les mêmes verrous dans le même ordre.

### 17.3 Constat qui dépasse le tableau du §15.3 : la ligne du message n'atteindrait ni le fichier ni le pont

En mode démon, **c'est le worker qui écrit les lignes du fichier de session**, à partir des événements du moteur (`EngineEvent::UserMessage`, `crates/pa-daemon/src/worker/turn.rs:614-620`). Le moteur ne transmet une ligne user de la boucle que si `boundary_passed` est vrai (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:234-245`). Ce drapeau ne passe à vrai qu'à l'`AgentEnd` d'un run (`:269`) : il sert à ne pas écrire deux fois la ligne du prompt, que le worker écrit déjà à l'admission (`session_engine_impl.rs:1411-1433`).

Conséquence : un message pris par la boucle dans **le premier run** d'une livraison serait envoyé au modèle, mais :
- ni écrit dans le fichier de session : à la reprise, l'historique et le préfixe ne seraient plus ceux de la requête ;
- ni transmis au pont : pas de ligne, donc pas de `turnStart` `steer`.

Il faut donc transmettre, dans le premier run, les lignes user des messages envoyés pendant le tour, en continuant d'écarter celle du prompt. **C'est un 8e fichier upstream, absent du tableau.**

| Fichier upstream | Où | Lignes |
|---|---|---|
| `crates/pa-daemon/src/agent_engine/turn/run_once.rs` | garde de `:238-241` | +1 (commentaire), 1 modifiée : la garde devient `if boundary_passed.load(…) \|\| super::super::ya_steer::is_steered(agent_message)` |

Le repérage se fait comme chez Unharnessed (horodatage du message, `uh:crates/uh-daemon/src/acp/session.rs:117-123`). Notre module du moteur garde l'horodatage et le texte de chaque message qu'il passe à la file de l'agent ; la ligne du prompt a un autre horodatage. Le registre vit dans notre module, ce qui évite un champ de plus dans une structure upstream.

**Coût révisé de B : +33 lignes dans 8 fichiers upstream, 1 ligne modifiée** (au lieu de +32 dans 7, 0 modifiée). Lignes prévues, à confirmer à l'écriture.

**Arrêt** (décision du 25/11/2026) : B demande plus de lignes upstream que le tableau. Pas de code. Le journal du CDC et `fork-changes.md` seront mis à jour avec la décision finale.

### 17.4 Tests prévus avec B (rappel, après décision)

- **E1 à E5** : inchangés.
- **E6** (Stop avec un message accepté et pas pris) : aucune ligne du message après le Stop, ni au prompt suivant.
- **E7** (tour en échec : erreur factice avec `delayMs`, relances automatiques coupées dans les réglages du test, car une erreur factice ordinaire est relancée, `crates/pa-core/src/session_engine/auto_retry.rs:99-112`) : aucune ligne du message dans le tour en échec ; le renvoi en prompt donne une seule ligne et une seule réponse.
- **E6 et E7 ne peuvent pas être rouges sur un squelette qui ne fait rien**, puisqu'ils vérifient que rien ne tourne. Proposition :
  - push 1 : squelette et tests (E1 à E5, U1 à U5 rouges ; E6 et E7 verts, ce qui est attendu) ;
  - push 2 : le code sans le vidage au règlement. E6 et E7 deviennent rouges : le message resté dans la file de l'agent part au début du run suivant (`crates/pa-agent/src/agent_loop/run.rs:31-32`) ;
  - push 3 : le vidage. Tout passe au vert.
- **E7 côté extension** (agent factice) : renvoi une fois, réponse affichée une fois. C'est déjà le comportement de `chat.ts`, donc pas rouge sur le code actuel. Il sert de garde.

## 18. Décisions de Yusuf (25/11/2026) : voie B, code et tests

### 18.1 Décisions

- **Voie B**, avec le 8e fichier : **+33 lignes dans 8 fichiers upstream, 1 ligne modifiée**, la garde de `run_once.rs`. L'agent est atteint directement par `turn_agent` (§17.2).
- **Trois pushs** :
  1. squelette et tests ;
  2. le code sans le vidage : E6 et E7 rouges ;
  3. le vidage : tout vert.
- **E7** coupe les relances automatiques dans ses réglages.
- **Exigences sur la garde de `run_once.rs`** :
  - la ligne du prompt reste écrite une seule fois, assertion dans E3 ;
  - pas de collision dans le registre (§18.2) ;
  - `ya_abort_window` reste valide (§18.3) ;
  - E3 vérifie aussi la reprise (`session/load`) ;
  - captures : `compare_capture_prompts.py` sur `acp-resume`, en plus de `D-steer`, `E-steer-skills` et `compare.py --all`.
- **Avec B, l'annonce `[skills-changed]` part au prompt suivant**, pas avec le message : l'attendu d'`E-steer-skills` est corrigé (§12.3).

### 18.2 Garde de `run_once.rs` : ligne du prompt et collisions

- **Ligne du prompt écrite une fois** : E3 l'affirme (« the prompt's row is written once »), dans le fichier de session et au rejeu après `session/load`.
- **Le registre ne peut pas confondre deux lignes** (`crates/pa-daemon/src/agent_engine/ya_steer.rs`) :
  - **Deux messages** : chacun prend un horodatage strictement supérieur au dernier pris dans le processus, même dans la même milliseconde. La clé est (horodatage, texte), donc deux messages de même texte ont deux clés. Il y a un worker par session (`WORKER_ACTIVE_SESSION_ID_ENV`, `crates/pa-daemon/src/worker/env.rs:52`). U5 couvre ce point.
  - **Un message et la ligne du prompt** :
    - le pont n'envoie un message qu'après le premier message de l'assistant du tour (U1) ;
    - or la boucle émet les lignes admises du run, celle du prompt comprise, dès son entrée (`crates/pa-agent/src/agent_loop/entry.rs:29-42`), avant `run_loop` et le premier appel du modèle (`run.rs:71`) ;
    - ces lignes sont donc passées par la garde avant qu'un message soit inscrit, et ne peuvent pas y correspondre.
  - **Un message reconnu une fois** : l'entrée est retirée à la correspondance.
  - **Le règlement efface le registre** avec la file de l'agent : aucune entrée ne survit à son run, donc ne peut correspondre à une ligne d'un tour suivant.

### 18.3 `ya_abort_window` (ticket 36)

Le test compte les appels de la sonde d'annulation : premier appel dans `turn/model.rs`, second en tête du futur d'admission de `turn/run_once.rs` (`crates/pa-daemon/src/agent_engine/tests/ya_abort_window.rs:5-29`). La garde modifiée est dans l'abonnement aux événements du run, pour les lignes `MessageEnd` user (`run_once.rs:234-246`). Elle n'appelle pas la sonde. Le compte reste donc celui du test, qui tourne dans la CI de chaque push (relevé au §19).

### 18.4 Lignes upstream écrites

| Fichier | Lignes | Où |
|---|---|---|
| `crates/pa-daemon/src/acp/daemon.rs` | +4 | dispatch `:1163-1166` ; l'appel de `seal` du squelette A est retiré |
| `crates/pa-daemon/src/worker.rs` | +2 | `mod ya_steer;` après `mod commands;` |
| `crates/pa-daemon/src/worker/input.rs` | +4 | `handle_queue`, après `require_created` |
| `crates/pa-daemon/src/worker/turn.rs` | +2 | `settle_engine.ya_drop_steers();` avant `core.busy = false`, verrou tenu |
| `crates/pa-daemon/src/engine.rs` | +9 | `ya_steer` et `ya_drop_steers` du trait `SessionEngine`, défauts neutres |
| `crates/pa-daemon/src/agent_engine/session_engine_impl.rs` | +8 | les deux méthodes, déléguées à notre module |
| `crates/pa-daemon/src/agent_engine.rs` | +2 | `mod ya_steer;` |
| `crates/pa-daemon/src/agent_engine/turn/run_once.rs` | +2, 1 modifiée | garde des lignes user : `\|\| super::super::ya_steer::is_steered(agent_message)` |

Total : **+33, 1 modifiée, 8 fichiers**, mesuré par `git diff --numstat main -- <les 8 fichiers>` : 34 insertions et 1 suppression, soit 33 lignes nouvelles et la garde de `run_once.rs` réécrite. Nouveaux fichiers à nous : `acp/ya_turns/steer.rs` (réécrit pour B), `worker/ya_steer.rs`, `agent_engine/ya_steer.rs`.

### 18.5 Tests et assertion rouge attendue par push

- **E1 à E5** : rouges au push 1, sur les mêmes assertions qu'au §16 (`serve` répond `{}` sans rien envoyer). E3 échoue d'abord sur « one row for the message » (0 ≠ 1).
- **E6** (Stop avec un message accepté, pas pris) et **E7** (tour en échec : réponse lente du provider factice puis `stopReason: "error"`, relances coupées) : **verts au push 1**, puisque rien n'est envoyé. **Rouges au push 2**, sans le vidage : le message resté dans la file de l'agent part au début du run suivant (`run.rs:31-32`). E6 échoue sur « the message never runs », E7 sur « the message runs once » (2 ≠ 1). **Verts au push 3.**
- **U1 à U4** (`acp/ya_turns/steer.rs`) : attente du premier message de l'assistant ; un `turnStart` par ligne ; message refusé ; refus de la porte. **U5** (`agent_engine/ya_steer.rs`) : registre (horodatages, reconnu une fois, effacé au règlement). Tous rouges au push 1.
- **Extension** :
  - X0 à X3, `suite.ts` et `suite-real.ts` : rouges comme au §16 (méthode pas encore servie) ;
  - **E7 côté extension** (« renvoyé une fois en prompt, réponse affichée une fois ») : vert sur le code actuel, car c'est déjà le comportement de `chat.ts`. C'est un test de garde.
  - Mesuré en local : sans X4, 4 en échec (X0 à X3) et E7 vert ; avec X4 appliqué le temps de l'essai, 5/5.

## 19. Relevés de la voie B

### 19.1 Push 1 de B (`aca908a7e`) : squelette et tests

Runs `ci` 45634791032 et `ya-extension` 45634791003. `check + clippy (workspace)` vert : le squelette et les tests compilent.

| Test | Run / job | Ligne du journal | Résultat |
|---|---|---|---|
| E1 à E5 | 45634791032 / 139277947205 (shard 5/8) | 1296-1343 | Rouges : E1 `:462` « no turnStart origin steer » ; E2 `:532` « first: … » ; E3 `:577` « one row for the message » 0 ≠ 1 ; E4 `:662` « the note: … » ; E5 `:688` `None` ≠ `Some(-32603)` |
| E6, E7 | idem | 1300 et suivantes | Verts, comme prévu (rien n'est envoyé au worker ; leur rouge vient au push 2) |
| U1 à U4 | 45634791032 / 139277947229 (shard 3/8) | 1563-1566, 2750-2772 | Rouges : U1 `:122` `Send` ≠ `WaitModel` ; U2 `:149` `None` ≠ `Some(7)` ; U3 `:171` `None` ≠ `Some(3)` ; U4 `:182` `Send` ≠ `Refuse` |
| U5 | idem | 1690, 2779 | Rouge `:96` (« strictly increasing ») ; 1249 autres tests passés |
| `ya_abort_window` | idem | — | Vert |
| X0 à X3, E7 extension | 45634791003 / 139277235160 | 728-731 | 75 tests, 71 passés, 4 en échec (X0 à X3) ; E7 côté extension vert (garde) |
| `suite.ts` « Send now » | idem | 1103-1112 | `covered` ≠ `clicked`, e2e 15/16 |
| `suite-real.ts` | 45634791003 / 139277234314 | 1001-1017 | « le message suit le résultat, une réponse » ; 18/19 |
| `Check changelog fragment` | — | — | Vert (`.changes/ya-39-steer.md`) |

### 19.2 Push 2 de B (`ca6e90985`) : code sans le vidage au règlement

Runs `ci` 45637924855 et `ya-extension` 45637924952. `ya-extension` vert (X4 servi). Un seul job en échec : shard 5/8, job 139288647900 (journal 1300-1338, 4 passés, 3 en échec).

| Test | Ligne | Résultat |
|---|---|---|
| E6 `stop_with_a_steer_waiting` | `:749`, journal 1320 | **Rouge, comme prévu** : « the message never runs » `Some(4)` ≠ `None`. Le message resté dans la file de l'agent est parti au début du run du prompt suivant |
| E7 `a_failed_turn_drops_the_message_it_did_not_take` | `:808`, journal 1312 | **Rouge, comme prévu** : « the message runs once » 2 ≠ 1 |
| E1, E3, E4, E5 | — | Verts (E3 avec la ligne du prompt écrite une fois et la reprise) |
| **E2 `two_steers_go_in_one_model_turn`** | `:533`, journal 1327 | **Rouge, non prévu.** Lignes : `[…, toolResult, user "second note", user "first note", assistant "both answered"]` |
| U1 à U5, `ya_abort_window` | shard 3/8 | Verts |

**Constat E2 : l'ordre de deux messages envoyés coup sur coup n'est pas garanti.**
- Le pont traite chaque requête hors prompt, fermeture et annulation dans une tâche à part (`tokio::spawn`, `crates/pa-daemon/src/acp/daemon.rs:558-568`). Deux `_yus/session/steer` envoyés sans attendre la réponse du premier, comme le fait l'extension (`steerAll` lance chaque envoi sans l'attendre, `extensions/vscode/src/chat.ts:448-457`), atteignent le worker dans un ordre quelconque.
- Ici l'ordre a été inversé. Le runtime exécute souvent en premier la tâche lancée le plus récemment, mais rien ne fixe l'ordre.
- Les deux messages partent bien dans un seul tour du modèle, avec une réponse. Mais l'extension rattache les `turnStart` aux messages dans l'ordre où elle les a envoyés (`joined`, `chat.ts:822-850`). Le texte affiché sous chaque tour peut donc être inversé.
- Notre module ne peut pas retrouver l'ordre des trames sans accroche dans la boucle de lecture du pont (`daemon.rs`). Unharnessed a la même concurrence côté client.

Choix à soumettre à Yusuf (§20).

### 19.3 Push 3 de B (`fe13a9f5a`) : vidage au règlement

Runs `ci` 45640624529, `ya-extension` 45640624456 et `ya-build-macos-arm64` 45640610141.

| Test | Résultat |
|---|---|
| E1, E3, E4, E5 | Verts |
| **E6, E7** | **Verts** : le vidage au règlement les fait passer (rouges au push 2) |
| **E2** | **Rouge**, même cause qu'au §19.2 (job 139298202139, shard 5/8, `:533`, journal 1301-1318) : `second note` avant `first note` |
| U1 à U5, `ya_abort_window` | Verts |
| Autres shards, clippy, fmt, Windows | Verts |
| `ya-extension` : unitaires, `suite.ts`, e2e vrai binaire (job 139297897753) | **Verts** |

**Captures sur le binaire `fe13a9f5a`** (artefact du run 45640610141, SHA256 vérifié). Commande :
```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/acp_frames.py --binary <yusagent> --out <dossier>
```
Résultat `CONFORME` (sortie complète au §21) :
- **D-steer** :
  - message accepté (`{}`), tour périmé refusé (`-32603`), `turnStart` `steer` sous le tour du prompt ;
  - préfixe N → N+1 (`0001 -> 0002`) : prompt système, tools et paramètres identiques ; messages 1 → 4, identiques au déplacement de `cache_control` près ;
  - la requête du message finit par `[tool_result, texte du message]`.
- **E-steer-skills** :
  - mêmes contrôles ;
  - deux paires, chacune préfixe exact au déplacement de `cache_control` près ;
  - la requête du message (n° 1) ne porte pas `[skills-changed]` ; l'annonce est dans la requête 2, celle du prompt 2.
- **`compare_frames.py`** (agent factice) : **0 écart**. Réponse d'un message accepté, refus d'un tour périmé (code et texte identiques), forme et place du `turnStart` `steer`.

## 20. Ordre de deux messages envoyés coup sur coup (E2) : choix à soumettre

Constat au §19.2 : deux `_yus/session/steer` envoyés sans attendre la réponse du premier atteignent le worker dans un ordre quelconque. Le pont traite chaque requête dans sa propre tâche (`daemon.rs:558-568`). L'extension les envoie ainsi (`chat.ts:448-457`) et rattache les tours aux messages dans son ordre d'envoi (`chat.ts:822-850`).

Options :
- **(a) Extension : un message après l'autre.**
  - `steerAll` n'envoie le message suivant qu'après la réponse du précédent, quelques lignes dans `chat.ts`.
  - Le pont ne répond `{}` qu'après l'accusé du worker, qui a mis le message dans la file de l'agent. L'ordre de la file est donc l'ordre d'envoi, et le rattachement des `turnStart` suit.
  - Aucune ligne upstream.
  - E2 enverrait le second message après la réponse du premier, comme l'extension.
  - Limite : un autre client qui enverrait sans attendre n'aurait pas d'ordre garanti.
- **(b) Pont : ordre des trames.** Une accroche dans la boucle de lecture de `daemon.rs` (`frame_order_prefix`) inscrit chaque `steer` dans l'ordre des trames, avant la tâche. C'est strict pour tout client, mais ajoute quelques lignes dans un fichier upstream.
- **(c) Limite acceptée** : quand deux messages partent au même instant, les textes des tours rejoints peuvent être inversés. Les deux messages partent dans un seul tour du modèle, chacun une fois. E2 vérifierait les deux lignes consécutives, dans un ordre quelconque.

## 21. Captures (voie B, binaire `fe13a9f5a` contre `main` `5661bdebc`)

Binaires :
- branche : artefact `ya-build-macos-arm64` du run 45640610141 (`0.10.2-continuous.fe13a9f5a…`) ;
- `main` : artefact du run 45616770418 (`17c7e3349`, même arbre que `5661bdebc` : `git diff --stat 17c7e3349 5661bdebc` est vide).

SHA256 vérifiés (`shasum -a 256 -c SHA256SUMS`). Variables : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`.

| Mesure | Commande | Résultat |
|---|---|---|
| Banc complet, `main` | `python3 -I bench/tools/capture/run.py --binary <main>` | `runs/20261125T150324Z`, `acp-resume` et `acp-live` `CONFORME` |
| Banc complet, branche | `python3 -I bench/tools/capture/run.py --binary <branche>` | `runs/20261125T151615Z`, `acp-resume` `CONFORME` |
| Toutes les requêtes, `main` contre branche | `python3 -I bench/tools/capture/compare.py runs/20261125T150324Z runs/20261125T151615Z --all --out <md>` | **TOUT IDENTIQUE** |
| Reprise ACP | `python3 -I bench/smoke/tools/compare_capture_prompts.py <main>/acp-resume <branche>/acp-resume`, sur une copie de chaque `acp-resume/<cas>/cap/` en `<cas>/capture/` (l'outil lit `capture/*.body.json`) | 25 requêtes de chaque côté, mêmes noms, **TOUT IDENTIQUE**. Pour le format Anthropic, `system` et `messages` sont comparés à l'identique dans « autres » |
| D-steer, E-steer-skills, branche | `python3 -I bench/tools/capture/acp_frames.py --binary <branche> --out <dossier>` | **CONFORME** (§19.3). D : paire `0001 -> 0002`, messages 1 → 4, préfixe exact au déplacement de `cache_control` près, fin `[tool_result, message]`. E : paires `0001 -> 0002` et `0002 -> 0003`, même résultat ; `[skills-changed]` dans la requête 2 seulement, celle du prompt 2 |
| D-steer, E-steer-skills, `main` (avant) | même commande, binaire `main` | `NON CONFORME`, attendu : message refusé (`-32601`), aucune requête ne porte le message (paire `0001 -> 0002`, messages 1 → 3) |
| Agent factice | `python3 -I bench/tools/capture/compare_frames.py <dossier branche>` | **0 écart** |

Le cas `spawn` du banc affiche `ok=False` sur les deux binaires. Ce n'est pas lié à ce ticket.

## 22. Décision de Yusuf sur E2 (25/11/2026) : option (a)

**L'extension envoie un message dans le tour après l'autre, chacun après la réponse au précédent.**
- **Raison** : en JSON-RPC, l'ordre de requêtes concurrentes n'est pas garanti ; un client qui veut l'ordre attend la réponse. Le pont ne répond `{}` qu'après l'accusé du worker, qui a mis le message dans la file de l'agent. L'ordre d'envoi est donc celui des lignes et des `turnStart`.
- **Limite** (`docs/fork-changes.md`) : un autre client ACP qui envoie sans attendre n'a pas d'ordre garanti.
- **E2** garde son assertion d'ordre strict, et envoie le second message après la réponse au premier.

**Correctif** :
- `extensions/vscode/src/chat.ts`, `steerAll` : les envois sont chaînés (`steerChain`). Un envoi en attente ne part plus après une relance (`run`), ni pour un message revenu en file entre-temps (absent de `steered`).
- Agent factice : `steerDelayMs` du scénario retarde la réponse à un message.

**Test unitaire, rouge puis vert en local** (un seul push, décision de Yusuf) : « deux messages pendant un tour (E2) : le second ne part qu'après la réponse au premier, dans l'ordre » (`test/unit/agent.test.ts`). Commande :
```
cd extensions/vscode && npm run compile-tests && node --require ./out/test/unit/setup.js --test --test-timeout=60000 --test-name-pattern="E2" out/test/unit/agent.test.js
```
- Avec le `chat.ts` d'avant : `✖ … (186 ms)`, `AssertionError: avant la réponse au premier, le second n'est pas parti`, `actual: [ 'un', 'deux' ]`, `expected: [ 'un' ]`.
- Avec le correctif : `✔ … (403 ms)`. Suite complète `npm run test:unit` : 76 tests, 76 passés.

**À noter, non traité ici** :
- `bench/smoke/tools/compare_capture_prompts.py` cherche `capture/*.body.json`, alors que `acp-resume` écrit dans `<cas>/cap/` : correctif d'outil plus tard. Le §21 a comparé des copies renommées.
- Le cas `spawn` du banc échoue parce que `rlm.spawn` est coupé : l'hôte répond « RLM recursion depth limit reached (RLM_DEPTH=0, RLM_MAX_DEPTH=0) » (`runs/20261125T151615Z/spawn/cap/0002.body.json`). Ce n'est pas lié au ticket 39.

## 23. Échec de l'e2e vrai binaire sur `85d8e91d2` : enquête (25/11/2026)

**Faits**

- **Première tentative** : run 45644563443, job 139311928005, 15/19 (journal 973-1029).
  - Premier test, « cellule avec bash() : boîte, accord par le bouton… » : `délai dépassé : tour fini` (`suite-real.js:207`, l'`idle()` qui suit le clic). Diagnostic : `cellule call-bash pending : {}`. L'accord n'a jamais atteint le pont, et la cellule attendait encore sa permission.
  - Après l'échec, le Stop du lanceur (`run()`, `suite-real.ts`) a refusé la demande (`[permission-denied]` au test suivant). Les trois tests suivants de la session 1 ont échoué en cascade.
- **Relance par Yusuf** (tentative 2, job 139371833601) : 19/19, premier test en 3 173 ms. Les 16 checks requis sont verts sur `85d8e91d2` (`gh pr checks 90`).
- **Le journal CI ne trace ni le clic ni les trames ACP.** Aucune ligne `permission …` de l'hôte, aucune trame. Il ne permet pas de dire si le clic a été envoyé ou si la réponse de permission est partie : impossible de conclure depuis les journaux.
- **Comment le test donne l'accord** (`extensions/vscode/test/e2e/suite-real.ts:190-195`) :
  1. carte `pending` ;
  2. texte de la boîte affiché ;
  3. `sleep(1_100)` ;
  4. `probeInput({ click: "Allow" })`, sans vérifier l'issue.

  Les boutons de la boîte ne s'arment qu'**1 s après son ouverture** (`BUTTONS_DELAY_MS = 1000`, `src/ya/permission-keys.ts:17`), par un minuteur du webview (`webview/ya-permission.tsx:69-74`). Un `click()` sur un bouton désactivé ne fait rien (`webview/ya-probe.ts:35-42`). La sonde garde l'état du bouton au clic (`box.click.disabled`, `ya-probe.ts:30`, `:40`), mais le test ne le lit pas. `suite.ts:133`, lui, attend `box.buttonsReady` avant de cliquer.
- **`steerAll` et le chaînage ne touchent pas la boîte d'accord** (`extensions/vscode/src/chat.ts`) :
  - `steerChain` (`:117`) ne s'allonge que dans la boucle de `steerAll` (`:463`), et seulement pour un message de `steering` pas encore dans `steered` (`:460-461`). `steering` ne reçoit que les messages « maintenant » (`now()`, `:427`).
  - Rien n'attend `steerChain`, et il n'y a aucun drapeau d'envoi : avec une liste vide, `steerAll` (`:455`, appelé en `:430`, `:812`, `:829`) ne fait que lire `this.run`.
  - La réponse de permission passe par `view.ts:137-141` puis `Permissions.answer` (`src/ya/permissions.ts:48`), sans lien avec `chat.ts`.
  - Entre le push 3 (job vert 139297897753) et `85d8e91d2`, seuls le test E2 Rust et `chat.ts` (`steerAll`) changent dans `crates/` et `extensions/vscode/src` (`git diff --stat fe13a9f5a 85d8e91d2`). Le binaire a donc le même code Rust.
- **Historique** : sur les 25 runs `ya-extension` en échec parmi les 100 derniers, ce test a échoué 4 fois avant le ticket 39 :
  - runs 45035376564 et 45034241467 (`ya-28-x1`) ;
  - runs 44927865325 et 44927543897 (`ya-24-extension-pr3`).

  Les symptômes étaient autres : assertions sur la carte (« carte affichée, fermée, pas inerte », « carte dépliée », « aucun bloc inerte affiché », format `exit 0 · N ms`). **Un échec sur `tour fini` avec `call-bash` en `pending` n'avait jamais été vu.**

**Hypothèse (non vérifiée)** : course dans le test. Le minuteur d'armement du webview a tourné plus tard que les 1 100 ms d'attente de l'hôte (charge du runner). Le clic est tombé sur un bouton encore désactivé et n'a rien fait. La relance verte et l'absence de changement sur ce chemin vont dans ce sens, sans le prouver.

**Proposition pour rendre le test déterministe** (dans cette PR, si Yusuf le veut, ou en ticket à part) : dans `suite-real.ts:193-194`, remplacer `sleep(1_100)` et le clic par :
1. une attente du bouton « Allow » activé dans le DOM (`probe().boxButtons`, `disabled: false`) ;
2. puis `probeInput({ click: "Allow" })` ;
3. puis l'assertion `box.click.disabled === false`.

Même chose pour les autres accords de ce fichier qui passent par `answer()` (`:144-148`, `post({ type: "permission" })` direct, qui ne dépend pas de l'armement : à garder tel quel). Si le défaut revient, l'assertion dira « clic sur un bouton désactivé » au lieu d'un délai sur « tour fini ».

**Décision de Yusuf : correction du test dans cette PR.** `suite-real.ts`, premier test :
- attente du bouton « Allow » actif dans la page (`probe().boxButtons`, `disabled: false`) ;
- clic ;
- assertion « clic sur « Allow » actif » : boîte déjà fermée (accord pris), ou clic noté sur un bouton actif (`box.click.disabled === false`).

Les autres accords du fichier, qui passent par `post({ type: "permission" })`, sont inchangés.

**Pas de rouge puis vert pour ce correctif** : la course ne se déclenche pas à volonté. Le justificatif est le journal du job 139311928005 (lignes 973-1029, `call-bash` resté `pending`) et la relance verte (job 139371833601, 19/19).

Contrôle avant le push : `git diff --stat 85d8e91d2 HEAD -- crates/ extensions/vscode/src extensions/vscode/webview` est vide. Le produit est donc identique à `85d8e91d2`.

## 24. Contrôle manuel de Yusuf (binaire et `.vsix` de `85d8e91d2`)

- **Étape 0 : conforme.** Étiquette « auto · no sandbox » sous le compositeur, sans bandeau. Output › yusAgent : `agent ready: yusagent 0.10.2-continuous.85d8e91d2b4c96df9b27bd813f9b86a9e7f513ed (protocol 1, pid 9284)`. Avertissement connu : `providers: list failed: _yus/providers/list : méthode pas encore servie`.
- **Étape 1, premier essai : non testée.**
  - Le modèle a lancé `h = bash("sleep 20; echo fini")`, **sans `await`** (fichier de session `01a213d7-…jsonl`). `bash()` a rendu une poignée aussitôt : cellule finie en 40 ms, commande en arrière-plan.
  - Le tour 1 était fini à 18:57:03. Le message est parti après, en prompt (tour 2), pas dans le tour. La fin de la commande a lancé un tour de l'agent (tour 3, `[bash-done]`).
  - Ce n'est pas `bashWaitSeconds`. L'étape est reprise avec une cellule Python qui bloque vraiment (`time.sleep(20)`).
- **À noter, hors ticket 39** : au tour 1, le modèle a répondu « La commande a affiché fini » alors que la commande tournait encore en arrière-plan. Le résultat de la cellule était vide. C'est une affirmation non fondée du modèle sur une commande détachée.
- **Étape 1, second essai (conversation [3], cellule Python bloquante) : NON CONFORME sur le raccourci.**
  - Cmd+Entrée pendant la cellule a mis le message **en file** (`19:00:13.668 chat: queued (1 waiting)`), sans état « Sending ».
  - La flèche « Send now », cliquée 11 s plus tard, l'a envoyé dans le tour : `message 5 sent into the running turn`, `turn 5 joined the running turn`, réponse « fini » et date, aucun doublon. Le parcours de l'étape 6 est donc **conforme**.
  - Autre point : le message affiché commence par « joute » (51 caractères au lieu de 52, le « A » manque).

**Enquête (lecture du code, sans push)**

- **Chemin de Cmd+Entrée, juste dans le code** :
  - `webview/Composer.tsx:341-344` : `Enter` sans Shift donne `submit(event.metaKey || event.ctrlKey)`.
  - `:277-289` : `post({ type: "send", …, now: now || undefined })`.
  - `src/view.ts:266-267` : `chat.send(text, images, edit, now)`.
  - `src/chat.ts:281-284` : `now` et pas d'`edit` donnent `now(message)`, puis tour en cours (`:427-431`), puis `steering` et `steerAll`.
  - Ce code est celui d'Unharnessed (`uh:vscode/webview/Composer.tsx:336-339`). Ni l'une ni l'autre extension ne déclare de raccourci clavier (`contributes.keybindings` absent).
- **La ligne `chat: queued (N waiting)` ne sort que de `send` sans `now`, ou avec un `edit`** (`chat.ts:285-289`). Ici, aucune modification de prompt n'était ouverte. Le webview a donc posté le message sans `now` : soit `Enter` est arrivé avec `metaKey` et `ctrlKey` à faux, soit c'est le bouton d'envoi (`Composer.tsx:565`, `submit()`).
- **Cause non établie.** Le journal ne trace pas les touches du webview. Hypothèses :
  - (H1) VS Code ne transmet pas Cmd+Entrée au webview avec `metaKey` (gestion des raccourcis du workbench, ou un raccourci `cmd+enter` de l'utilisateur) ;
  - (H2) Cmd relâché avant Entrée.
- **Caractère perdu** : rien dans l'extension ne retire une touche imprimable.
  - `onKeyDown` ne traite qu'Entrée, Tab, flèches, Échap et Retour arrière (`Composer.tsx:305-349`). `Popover.tsx:37-39` ne traite qu'Échap. `onPaste` n'intercepte que les images (`Composer.tsx:351-357`).
  - Le brouillon est un état React du webview (`App.tsx:49`). L'hôte ne l'écrase qu'au changement de conversation (`App.tsx:83-90`) ou au début d'une modification de prompt (`:109-110`), aucun des deux ici.
  - Hypothèse : la première touche est partie ailleurs, car le focus n'était pas encore dans la zone de message. Non vérifié.
