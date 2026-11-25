# Ticket 37 : annonce de skills ou de MCP qui atteint le modèle sans être écrite dans le fichier de session (constat et plan)

Demande de Yusuf du 25/11/2026. Branche `ya-37-constat`, depuis `main` à `8e0ea28d4` (merge de la PR #86, ticket 36). **Constat et plan seulement, aucun code du produit**, aucun fichier upstream touché, rien compilé en local (règle 8), aucun appel de modèle.

Suite du ticket 35, §13, point 3. Lu : ticket 31 (§3.2, §6 à §8), ticket 35 (§11, §12), ticket 36 (§1).

## Résumé

- **Les chemins cités au ticket 35 (§13.3) ne produisent pas le défaut** :
  - **continuations autonomes et d'objectif du démon** : ce sont des livraisons séparées, avec leur propre comparaison par le worker (§1.1). L'admission ne compare pas ;
  - **`continue_run`** : aucune admission, donc aucune comparaison (§1.2) ;
  - **`-p` et RPC** : l'admission compare et produit l'annonce, mais le moteur l'écrit lui-même dans le fichier, dans l'ordre de la requête. Ce que disait le ticket 31 est **juste**, mais les tests 1 à 5 qu'il cite ne regardent pas le fichier (§2.2).
- **Deux chemins réels, étroits, dans le démon** :
  - **A. Moteur pas encore construit quand le worker compare** (`session_engine_impl.rs:1118`, rend `None`). Pas de jeton, donc l'admission compare (`admission.rs:47`, `:148`). Si le disque change entre la lecture faite par la construction et l'admission, l'annonce part au modèle. L'abonné de `run_once` ne la transmet pas au worker (`run_once.rs:247`) : elle n'est ni écrite, ni publiée (§1.3).
  - **B. Stop entre la prise de la livraison et l'écriture des lignes d'avance** (`turn.rs:69` ou `:80`, puis `:1044`), donc aussi pendant la comparaison du worker. L'annonce est produite par le worker et mise dans la boîte du tour suivant (`ya_resource_notice.rs:440`). La porte d'abandon d'`emit` refuse de l'écrire (`turn.rs:513-519`), et elle reste dans la boîte. **L'admission de la livraison suivante l'envoie au modèle** (`admission.rs:149`), sans qu'elle soit jamais écrite (§1.4). Ce cas contredit le ticket 35, §11.1 (« déjà écrite dans le fichier »), qui ne vaut que pour un Stop arrivé après `turn.rs:1044`.
- **Ce que voit l'utilisateur** : aucune ligne « Skills updated » ni « MCP servers updated » dans l'extension, ni au TUI, et rien n'est rejoué à `session/load`. Le modèle, lui, a reçu l'annonce (§3.1).
- **Coût en cache à la reprise** (§3.2) :
  - même worker : aucun ;
  - worker reconstruit : la plupart du temps, aucun coût de plus. Le prompt système est reconstruit depuis le disque, d'où un miss total déjà journalisé (`resume_prefix_change`, `parts: ["system"]`, accepté au ticket 31, décision 8) ;
  - un seul cas de miss non journalisé : l'état du disque revenu à celui de l'ouverture (changement annoncé hors fichier, puis défait). Le miss porte alors sur l'historique à partir de la ligne manquante. Estimation, non mesuré.
- **Tests sans horloge** : T1 au niveau du moteur du démon pour A, T2 en e2e du démon pour B, avec une FIFO qui tient la comparaison du worker (§4).
- **Corrections proposées** (§5), sans effet sur le préfixe ni sur le texte des annonces :
  - **A, option O1** : dans le démon, l'admission ne compare jamais. Un drapeau posé à la construction remplace le jeton du ticket 35. 2 lignes upstream dans `lifecycle.rs` ;
  - **B, option O2** : une annonce que la porte refuse est retirée de la boîte et réannoncée à la livraison suivante. Environ 7 lignes ajoutées et 2 retirées dans `worker/turn.rs`, toutes sur des lignes du fork ou à côté.
- **Ticket 35, §11.3 corrigé** dans ce commit (§6.2).

## 1. Où l'admission peut produire l'annonce

Rappel du chemin normal dans le démon depuis le ticket 35 :

- le worker compare avec le disque avant la ligne du prompt (`crates/pa-daemon/src/worker/turn.rs:1041` → `crates/pa-daemon/src/agent_engine/session_engine_impl.rs:1117-1123` → `crates/pa-core/src/session_engine/ya_resource_notice.rs:394-401`) ;
- le jeton `delivery_compared` est levé, et l'annonce est mise dans la boîte du tour suivant (`ya_resource_notice.rs:440`) ;
- le worker l'émet avec les lignes d'avance (`turn.rs:1043-1047`) : `emit` l'écrit (`turn.rs:634-645`) et la publie au pont en `message_start` / `message_end` (`turn.rs:837-840`, puis `crates/pa-daemon/src/acp/ya_notice.rs:29-35` par `crates/pa-daemon/src/acp/daemon.rs:406`) ;
- l'admission prend le jeton sans lire le disque (`ya_resource_notice.rs:377-386`), puis vide la boîte dans la requête (`crates/pa-core/src/session_engine/admission.rs:47-48`, `:148-149`).

Appels d'admission, relevés par `grep -rn "prompt_injected_message\|prompt_with_images" crates/pa-daemon/src crates/pa-cli/src` (hors tests) :

| Appelant | Ligne | Chemin |
|---|---|---|
| démon | `crates/pa-daemon/src/agent_engine/turn/run_once.rs:387`, `:393` | toutes les admissions du démon |
| RPC | `crates/pa-daemon/src/rpc/prompt_commands.rs:39`, `:197` | prompt ; suite d'une commande de session |
| `-p` | `crates/pa-cli/src/print_runtime.rs:1618` ; `crates/pa-cli/src/print_goal.rs:612`, `:655` | prompt ; tours d'objectif |

Dans `pa-core` et `pa-tui`, aucun autre appelant hors tests (même `grep` sur `crates/pa-core/src crates/pa-tui/src`).

### 1.1 Démon : continuations autonomes et d'objectif, `[bash-done]`

**L'admission ne compare pas : ce sont des livraisons séparées.** Vérifié sur `main` :

- `run_turns` ne reboucle que sur la reprise après dépassement (`crates/pa-daemon/src/agent_engine/turn/run_loop.rs:98-101`, seul `continue`).
- **Suite autonome** :
  - remise au puits (`run_loop.rs:191-205` ; `crates/pa-daemon/src/autonomous_continuation.rs:257-265`) ;
  - le puits du worker (`crates/pa-daemon/src/worker.rs:467-475`) la pousse dans `follow_up` (`crates/pa-daemon/src/worker/queue.rs:428-447`).
- **Suite d'objectif** : `goal_admission_sink` (`crates/pa-daemon/src/goal_continuation.rs:442-448`), installé par le worker (`worker.rs:567`).
- **Chaque livraison passe par la prise** (`turn.rs:62-83`), puis par la comparaison de `turn.rs:1041`, qui lève le jeton. Elle est ensuite admise par `run_once.rs:393` (ligne injectée).
- **Conséquence** : la première admission de chaque livraison prend le jeton. Le jeton est lié à sa livraison : ticket 35, §11.1, sauf le cas résiduel de `continue_run` suivi d'une continuation dans le même `run_prompt`, qui n'existe pas (ticket 36, §1.2).

### 1.2 Démon, RPC, `-p` : `continue_run`

**Pas d'admission, donc pas de comparaison.**

- démon : `run_once.rs:398` (`agent.continue_run()`), pour les tentatives suivantes et la reprise après dépassement (`run_loop.rs:61-64`) ;
- RPC : la pompe de la file (`crates/pa-daemon/src/rpc/commands.rs:175`) ;
- `-p` : le pilote d'un sous-agent en ligne (`crates/pa-cli/src/print_rlm.rs:392-396`).

`continue_run` ne vide pas non plus la boîte : seules les deux admissions la vident (`admission.rs:48`, `:149` ; `grep -rn "take_next_turn_rows" crates/pa-core/src crates/pa-daemon/src`).

### 1.3 Démon, chemin A : moteur pas encore construit quand le worker compare

- **Le worker ne compare pas** : `ya_resource_notice` rend `None` tant que le cœur n'est pas construit (`session_engine_impl.rs:1118`, `self.session.blocking_lock().clone()?`). Le jeton n'est pas levé.
- **Quand le cœur n'est pas construit à `turn.rs:1041`** :
  - la construction tient `session_build`, pas `session` (`crates/pa-daemon/src/agent_engine/lifecycle.rs:259-270`). Une construction en cours rend donc aussi `None` ;
  - à la création, la construction part en tâche de fond (`crates/pa-daemon/src/worker/create.rs:640-647`), et de même au remplacement de session (`crates/pa-daemon/src/worker/lifecycle.rs:271-282`). Un premier prompt qui arrive avant la fin de la construction tombe dans ce cas ;
  - sinon, le cœur est construit dans `run_prompt` : par `session_agent` (`run_loop.rs:219-225`) ou par l'expansion d'un `/skill:` (`session_engine_impl.rs:1344-1345`, puis `crates/pa-daemon/src/agent_engine/lifecycle.rs:240`). Les deux sont après `turn.rs:1041`.
- **Ensuite, l'admission compare** (`admission.rs:148` ou `:47`, jeton absent, `ya_resource_notice.rs:384-385`).
  - Ce que le modèle « sait » vient de la construction (`Tracker::at_creation`, `crates/pa-core/src/session_engine/engine.rs:781-782`).
  - Il y a une différence si le disque a changé entre cette lecture et l'admission. La fenêtre couvre la fin de la construction et le début de `run_prompt`. Sa durée n'est pas mesurée.
- **Effet** : l'annonce est mise dans la boîte, puis tout de suite prise dans la requête (`admission.rs:149`).
  - Le moteur l'écrit dans **son** gestionnaire de session, qui est en mémoire dans le démon (`lifecycle.rs:1056-1072` : « The engine session stays non-persisted (the worker owns the durable file) »).
  - L'abonné de `run_once` ne transmet que les lignes de l'assistant, les résultats d'outils et les lignes de l'utilisateur d'un run suivant (`run_once.rs:210-247`). Une ligne custom tombe dans `_ => {}` (`:247`).
  - Le worker ne la voit donc pas : elle n'est **ni écrite, ni publiée**.

### 1.4 Démon, chemin B : livraison abandonnée avant l'admission

**Le cas du ticket 35, §11.1, en deux temps.**

- **Stop après l'écriture des lignes d'avance** (après `turn.rs:1044`), puis abandon vu par `session_engine_impl.rs:1433`, `turn/model.rs:91` ou `run_once.rs:344` :
  - l'annonce est écrite, puis reste dans la boîte ; la livraison suivante l'envoie au modèle, avant son prompt ;
  - fichier et requête ont la même suite (`…, annonce, prompt suivant`) : la ligne du prompt abandonné n'est écrite ni dans le fichier ni dans le moteur. La porte refuse aussi la ligne acceptée (`turn.rs:513-519`, `session_engine_impl.rs:1433-1435`) ;
  - **pas de défaut** pour l'annonce. Le ticket 35, §11.1, est juste pour ce cas.
- **Stop entre la prise et l'écriture des lignes d'avance** (`turn.rs:69` ou `:80` remet le drapeau à faux, puis `turn.rs:1044`) :
  - `request_abort` lève `abort_requested` sous le verrou, avant de répondre (`crates/pa-daemon/src/worker/commands.rs:328-336`, `:376-378`) ;
  - la comparaison du worker est dans cette fenêtre (`turn.rs:1041`). Durée mesurée : médiane 1 742 µs, maximum 19 258 µs sur 28 prompts (ticket 31, §8.5) ; médiane 1 831,5 µs, maximum 1 981 µs sur 14 comparaisons (ticket 35, §14.2) ;
  - si elle trouve une différence, l'annonce est **déjà dans la boîte** (`ya_resource_notice.rs:440`) quand le worker l'émet ;
  - la porte d'`emit` la refuse : `abort_requested` est levé, et une ligne custom n'est ni une ligne de fin d'abandon ni une ligne annulée (`turn.rs:482-491`, `:513-519`). Le `return false` vient avant l'écriture de `turn.rs:634` et avant la publication. La boucle s'arrête (`turn.rs:1044-1045`) ;
  - `run_prompt` rend la main sans admission (`session_engine_impl.rs:1433-1435`) ;
  - **rien ne vide la boîte** sur un abandon : seules les admissions la prennent (§1.2) ;
  - **livraison suivante** : le worker recompare. `known` a déjà été mis à jour (`ya_resource_notice.rs:428-429`) : pas de nouvelle différence, rien à écrire. L'admission prend le jeton, puis **vide la boîte dans la requête, annonce comprise** (`admission.rs:149`) ;
  - **l'annonce atteint le modèle et n'est jamais écrite**.
- **Déclencheurs** : « Arrêter » dans l'extension (`session/cancel`, ticket 36, §1.2), Échap au TUI, ou `abort_and_send_queued`, dans les quelques ms qui suivent la prise d'une livraison, avec un changement de skills ou de MCP en attente. Probabilité non mesurée.

### 1.5 `-p`

- L'admission compare (pas de worker, pas de jeton) : `print_runtime.rs:1618`, `print_goal.rs:612`, `:655`.
- La ligne est écrite par le moteur (§2.2).
- Les sous-agents en ligne ont un gestionnaire en mémoire (`print_rlm.rs:781`) : pas de fichier, pas de reprise. Sans objet.

### 1.6 RPC (`pa-daemon --rpc`, sans démon)

- L'admission compare : `prompt_commands.rs:39` et `:197`. Un prompt envoyé pendant qu'un tour tourne passe par `steer` ou `follow_up`, sans comparaison (`admission.rs:126-132`).
- Le moteur RPC est construit comme celui de `-p` (`crates/pa-cli/src/print_runtime.rs:242-256`, `build_headless_engine_parts`), avec le même choix de gestionnaire de session (`print_runtime.rs:776`).

## 2. Écrite dans le fichier, publiée, rejouée à `session/load` ?

### 2.1 Tableau

| Chemin | Qui produit l'annonce | Écrite dans le fichier | Publiée | Rejouée à `session/load` |
|---|---|---|---|---|
| démon, première admission de chaque livraison (§1.1) | worker, `turn.rs:1041` | oui, avant la ligne du prompt (`turn.rs:634-645`) | oui, `_yus/notice` (`ya_notice.rs:29-35`) | oui (`crates/pa-daemon/src/acp/ya_history.rs:312-318`) |
| `continue_run` (§1.2) | personne | — | — | — |
| **démon, A : cœur non construit à `turn.rs:1041` (§1.3)** | **admission** | **non** (`run_once.rs:247`) | **non** | **non** |
| démon, Stop après `turn.rs:1044` (§1.4) | worker | oui | oui | oui |
| **démon, B : Stop entre la prise et `turn.rs:1044` (§1.4)** | **worker, envoyée par l'admission suivante** | **non** (`turn.rs:513-519`) | **non** | **non** |
| `-p` (§1.5) | admission | oui, dans l'ordre de la requête (§2.2) | sans objet (pas de pont) | oui, si la session est rouverte dans l'extension (déduit : `ya_history.rs` lit le fichier) |
| RPC (§1.6) | admission | oui, idem | au client RPC : `message_start` / `message_end` de la ligne (`crates/pa-daemon/src/rpc/session.rs:184-198`, `crates/pa-core/src/session_engine/session_events.rs:138-158`) | idem `-p` |

### 2.2 `-p` et RPC : le moteur écrit-il la ligne ? Oui

- **Toute `AgentSession` s'abonne à sa persistance** à la construction (`crates/pa-core/src/session_engine/mod.rs:253-262`).
- La boucle émet `MessageStart` puis `MessageEnd` pour chaque message du prompt, dans l'ordre : ligne de digest, lignes du tour suivant, annonce comprise, puis ligne de l'utilisateur (`crates/pa-agent/src/agent_loop/entry.rs:33-42`).
- `persist_event` écrit une ligne custom à son `MessageEnd` (`mod.rs:417-436`), en entrée `custom_message`, persistée tout de suite (`crates/pa-core/src/session/manager/append.rs:338-349`).
- **Le gestionnaire est le fichier de la session** en `-p` et en RPC : `select_session_manager_with_lease`, puis `SessionManager::open` (`print_runtime.rs:416-420`, `:776`, `:1443-1453`). Il est en mémoire seulement avec `--no-session`, et il n'y a alors pas de fichier à reprendre.
- **Test d'upstream qui montre le mécanisme** : `prompt_injected_message_persists_one_custom_row` (`crates/pa-core/src/session_engine/tests.rs:577`). Une ligne custom admise dans le prompt est écrite une seule fois dans les entrées de la session.
- **Ce que le ticket 31 disait** (§8.4, garde-fou 1) : « le moteur écrit lui-même la ligne dans l'ordre de la requête (`persist_event`, `mod.rs:412-438`). Tests 1 à 5 ».
  - **L'affirmation est juste.**
  - **La preuve citée ne la couvre pas** : les tests 1 à 5 ne comparent que les requêtes (`assert_ya31_announced`, `crates/pa-core/tests/prefix_stability.rs:2737-2764`), jamais le fichier.
  - Précision ajoutée au ticket 31 dans ce commit (§6.1).

## 3. Chemins A et B : ce que voit l'utilisateur, ce que coûte la reprise

### 3.1 Ce que voit l'utilisateur

- **Extension** : la ligne « Skills updated: … Sent to the agent. » ou « MCP servers updated: … » n'apparaît pas. Elle vient de `_yus/notice`, qui ne part que du `message_start` de la ligne émise par le worker (`ya_notice.rs:29-35`).
  - **A** : le modèle a l'annonce dès ce prompt. L'écran n'en dit rien.
  - **B** : le tour est annulé. Au prompt suivant, le modèle reçoit l'annonce. L'écran n'en dit rien.
- **TUI** : pas de panneau de la ligne custom, pour la même raison (même worker).
- **`session/load`** : rien n'est rejoué. `ya_history.rs` rejoue depuis le fichier (`ya_history.rs:312-318`).
- **Modèle** : il sait ce qui a changé. Son comportement n'est pas touché.

### 3.2 Coût en cache à la reprise

**Même worker** (fermeture du pont, `session/load`, cas L4 du ticket 31) : le moteur vivant garde l'annonce. La requête suivante prolonge la précédente : **aucun miss**.

**Worker reconstruit** (arrêt du démon, démon lancé par un autre build du ticket 29, plantage) :

- **L'historique est relu depuis le fichier, sans l'annonce** (`build_session_context`, `lifecycle.rs:545-549`).
- **Le prompt système est reconstruit depuis le disque.**
  - Si l'état des skills et du MCP diffère de celui de l'ouverture, le prompt système change.
  - D'où un miss sur tout le préfixe, **journalisé** : `resume_prefix_change`, `parts: ["system"]` (`crates/pa-core/src/session_engine/usage_log.rs:268-303`, `:537`). C'est le cas L5 du ticket 31, accepté (décision 8).
  - **L'annonce manquante n'ajoute rien à ce miss.** Le nouveau prompt système décrit déjà l'état courant.
- **Un seul cas ajoute un miss non journalisé** :
  - l'état du disque est revenu à celui de l'ouverture, par exemple un skill ajouté (annoncé hors fichier par A ou B), puis retiré (annoncé normalement) ;
  - le prompt système est alors identique à l'ouverture : la partie système et les tools font un hit ;
  - les messages divergent à la place de la ligne manquante. **Miss sur l'historique, de cette place jusqu'à la fin**, sans entrée au journal : l'empreinte ne couvre que le prompt système et les tools (`usage_log.rs:268-280`).
  - L'historique relu montre aussi un « Removed » sans le « Added » qui le précédait.
  - Estimation par lecture, non mesuré.

## 4. Voir le défaut rouge sur son assertion, sans horloge

### 4.1 T1, chemin A : test du moteur du démon

Nouveau fichier du fork `crates/pa-daemon/src/agent_engine/tests/ya_announce_admission.rs`. Ligne `mod ya_announce_admission;` dans `crates/pa-daemon/src/agent_engine/tests.rs` (upstream), après `mod ya_abort_window;` (`:23`) : **1 ligne**. Variante sans ligne upstream : ajouter le test à `ya_abort_window.rs`, au prix d'un nom de fichier qui ne dit plus son contenu.

**Déroulé** (esquisse, non compilée) :

1. `faux_engine_with_settings` (`tests.rs:100-123`) avec deux réponses immédiates. L'agent est dans `<dir>/agent`, donc les skills dans `<dir>/agent/skills`.
2. `engine.ya_resource_notice()` : `None`, cœur non construit (`session_engine_impl.rs:1118`). C'est l'appel du worker à `turn.rs:1041`.
3. `engine.ensure_core_session(&engine.resolve_model()?)` : la construction lit le disque, sans skill.
4. Écrire `<dir>/agent/skills/ya37/SKILL.md` : le changement tombe entre la construction et l'admission.
5. `engine.run_prompt(…)`, `emit` qui garde les événements et rend `true`.
6. Livraison suivante, comme le worker : `engine.ya_resource_notice()`, émise si elle existe, puis un second `run_prompt`.

**Assertions** :

- **invariant** : le nombre de lignes `ya-resources-changed` dans l'état de l'agent après les deux tours (ce que le modèle a reçu) est égal au nombre de `EngineEvent::CustomMessage` `ya-resources-changed` émis (ce que le worker écrit) ;
- **et ce nombre vaut 1.**
- **Aujourd'hui** : 1 contre 0, **rouge sur l'invariant**. L'admission du premier tour a annoncé (étape 5), le worker n'a rien reçu, et l'étape 6 ne trouve plus de différence.
- **Avec O1** : 1 et 1. L'annonce est faite à l'étape 6 par le worker, et émise.
- **Sans horloge** : réponses immédiates, aucun thread, aucun délai.
- **À vérifier en l'écrivant** : que la construction de `faux_engine_with_settings` passe par `create_session_with` avec un suivi (`engine.rs:781`) et lit `<dir>/agent/skills`.

### 4.2 T2, chemin B : e2e du démon

Dans le fichier du fork `crates/pa-daemon/tests/ya_announce_path_e2e.rs` (vrai `pa-daemon`, serveur local qui joue le modèle). **Aucune ligne upstream.** `#[cfg(unix)]`.

**Le point d'arrêt sans horloge** : une FIFO `<agent>/skills/.ignore`.

- La découverte lit `.ignore` par `std::fs::read_to_string`, sans test de type (`crates/pa-core/src/skills/discovery.rs:166-170`). Le ticket 35, §12.2, P2, l'a lu.
- Ouvrir une FIFO en écriture bloque jusqu'à ce qu'un lecteur l'ouvre. Quand l'ouverture rend la main, la comparaison est donc en cours.
- Le défaut de P2 (compter la N-ième ouverture) ne joue pas ici : la FIFO est créée juste avant le prompt 2. La seule lecture des skills qui suit est la comparaison de `turn.rs:1041` pour cette livraison. L'admission ne lit plus le disque (jeton, ticket 35).

**Déroulé** (esquisse) :

1. Session ouverte, prompt 1 jusqu'à son `agent_end` (`Client::turn`).
2. Écrire `<agent>/skills/ya37-b/SKILL.md`, puis `mkfifo <agent>/skills/.ignore`.
3. Un thread ouvre la FIFO en écriture et signale l'ouverture par un canal.
4. Envoyer le prompt 2 sans attendre, puis attendre le canal : la comparaison du worker est en cours.
5. Envoyer `abort` et attendre sa réponse : le drapeau est levé (`commands.rs:328-336`).
6. Fermer la FIFO : la lecture rend EOF, la comparaison se termine et trouve le skill. La porte refuse la ligne.
7. Attendre la fin de la livraison 2, puis supprimer la FIFO.
8. Lever la suspension de la file si nécessaire (`queued_input_suspended`, `commands.rs:335`), puis envoyer le prompt 3 jusqu'à son `agent_end`.

**Assertions** :

- **invariant** : le nombre d'annonces `[skills-changed]` dans les requêtes reçues par le serveur est égal au nombre de lignes `ya-resources-changed` dans le fichier de session ;
- **et ce nombre vaut 1.**
- **Aujourd'hui** : 1 dans la requête du prompt 3, 0 dans le fichier, **rouge sur l'invariant**.
- **Avec O2** : 1 et 1. L'annonce est retirée à l'étape 6, puis refaite et écrite par le worker au prompt 3.
- **Sans horloge** : l'ordre est tenu par la FIFO et par les réponses du démon. Les délais de lecture du client restent un filet, pas une assertion. Les corps de requête sont lus après l'`agent_end` du prompt 3, sans la période de calme de `settled_bodies`.

**À vérifier en l'écrivant** :

- la trame qui marque la fin d'une livraison abandonnée sans admission (pas d'`agent_end`) ;
- la commande qui lève `queued_input_suspended` avant un prompt simple ;
- que rien d'autre ne lit les skills entre l'étape 2 et la comparaison (sinon l'abandon tomberait ailleurs ; T2 le dirait par une requête du prompt 2 reçue par le serveur) ;
- que la commande `abort` est traitée pendant que le runner est bloqué dans la comparaison (le runner tourne hors de la boucle des commandes, ticket 36, §1.2).

## 5. Options de correction (non codées)

Aucune option ne touche au prompt système, aux tools ni au texte des annonces (§3.2 du ticket 31). Dans le chemin normal, les requêtes sont identiques octet pour octet. Seuls changent le moment et la présence d'une ligne d'annonce, quand A ou B se produit. Contrôle prévu : `compare_capture_prompts.py`, attendu TOUT IDENTIQUE.

### 5.1 Chemin A

**O1 (recommandée) : dans le démon, seul le worker compare.**

- **Le changement** :
  - un drapeau `worker_compares` dans le suivi (`ya_resource_notice.rs`, fichier du fork), posé une fois à la construction du cœur par le démon ;
  - `ya_announce_resources` rend tout de suite quand il est posé ;
  - le jeton `delivery_compared` devient inutile et peut être retiré.
- **Où le poser** : dans la construction du démon, à côté du point d'accroche YA-03, dans le `inspect` qui suit `create_session_with` (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1278-1280`). **+2 lignes upstream** (commentaire et appel).
  - Variante sans ligne upstream : l'appeler depuis `ya_sync_frozen_tools` (`crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs`, déjà appelé là). Elle mélange deux sujets : non recommandée.
- **Lignes du fork** : environ +10 / −8 dans `ya_resource_notice.rs` ; `session_engine_impl.rs:1115-1124` (lignes de R3) à peine touché. Estimation.
- **Effet** :
  - A disparaît : un changement tombé pendant la construction est annoncé à la livraison suivante, par le worker, et écrit ;
  - toute admission du démon, y compris un chemin futur d'upstream, ne lit plus le disque. C'était l'objectif du ticket 35 ;
  - `-p` et RPC ne sont pas touchés : moteur construit par `pa-cli`, pas par `lifecycle.rs` (§1.6).
- **Ce qui change pour le modèle** : dans la fenêtre A seulement, l'annonce arrive un prompt plus tard.

**O1-bis : le worker construit le cœur avant de comparer.**

- `session_engine_impl.rs:1118` attend la construction (`ensure_core_session`) au lieu de rendre `None`.
- Annonce dans la même livraison. Lignes de R3 seulement (environ +4, estimation).
- **Mais** : la construction remonte avant la ligne acceptée. Une livraison qui ne construisait pas encore construit maintenant, par exemple une commande de session qui échoue avant. Et le jeton reste nécessaire. Non recommandée.

### 5.2 Chemin B

**O2 (recommandée) : une annonce que la porte refuse retourne à son état d'avant la comparaison.**

- **Le changement** :
  - `ya_resource_notice.rs` (fork) : la comparaison garde `known` d'avant. Une nouvelle fonction `ya_withdraw_resource_notice` retire la ligne de la boîte et remet `known` à cet état. Environ +25 lignes, estimation ;
  - `crates/pa-daemon/src/engine.rs:424-428` (méthode du fork dans le trait upstream) : une méthode par défaut vide à côté. +5 lignes ;
  - `session_engine_impl.rs`, à côté des lignes de R3 : son implémentation. Environ +8 lignes ;
  - `crates/pa-daemon/src/worker/turn.rs` : nos deux lignes `:1040-1041` sont retirées. L'annonce est émise à part, juste après la boucle des lignes d'avance d'upstream (`:1043-1047`), drapeau `emitting_prefix_rows` levé ; un refus appelle le retrait. **Environ +7 / −2, aucune ligne d'upstream modifiée** :

```rust
            // yusAgent (YA-10, tickets 31 and 37): the skills or MCP announcement rides as the
            // last prefix row; one the abort gate refuses goes back, announced at the next delivery.
            if let Some(row) = engine.ya_resource_notice() {
                emitting_prefix_rows.set(true);
                if !emit(EngineEvent::CustomMessage(row)) {
                    engine.ya_withdraw_resource_notice();
                }
            }
```

- **Ordre** : inchangé. L'annonce était la dernière ligne d'avance (`parked.extend`, `turn.rs:1041`). Si une ligne d'upstream est refusée avant, la nôtre l'est aussi (même drapeau), puis elle est retirée.
- **Pas de course** : le retrait a lieu sur le fil du runner, avant tout `run_prompt` de cette livraison. Les livraisons sont sérialisées (ticket 35, §11.1).
- **Effet** : B disparaît. Le changement est annoncé et écrit à la livraison suivante. Un changement défait entre-temps n'annonce rien, comme aujourd'hui (« A change undone before the prompt announces nothing », `ya_resource_notice.rs:10`).
- **Moment de la comparaison** : après l'émission des lignes d'upstream au lieu d'avant. Écart de quelques µs (estimation), aucun effet sur le contenu.

**O2-bis : ne mettre la ligne dans la boîte qu'une fois écrite.** Le worker rend la ligne sans la mettre en file, puis confirme après l'`emit`. Même effet, mais deux appels du worker au lieu d'un, et `known` doit quand même revenir en arrière si l'écriture est refusée. Pas plus simple.

### 5.3 Écartée : transmettre les lignes custom du prompt depuis l'abonné de `run_once`

- Changer `run_once.rs:247` (upstream) pour transmettre les lignes custom admises couvrirait A et le ticket 34.
- **Mais l'ordre serait faux** :
  - la ligne acceptée est émise avant l'admission (`session_engine_impl.rs:1433`). Le fichier aurait `prompt, annonce`, la requête `annonce, prompt`, et une reprise créerait le miss non journalisé qu'on veut éviter ;
  - dans le chemin normal, le worker écrit déjà l'annonce : il faudrait dédoublonner.
- C'est la piste du ticket 34, avec ce problème d'ordre à traiter là-bas.

## 6. Corrections apportées aux tickets 31 et 35 (ce commit)

### 6.1 Ticket 31, §8.4, garde-fou 1

L'affirmation sur `-p` et RPC est juste (§2.2). Ajout d'une précision : les tests 1 à 5 ne vérifient que les requêtes, l'écriture dans le fichier est lue dans le code, et le test d'upstream `prompt_injected_message_persists_one_custom_row` montre le mécanisme.

### 6.2 Ticket 35, §11.3

Réécrit. Dans le démon, les continuations autonomes et d'objectif sont des livraisons séparées : elles repassent par la comparaison du worker et ne relisent pas le disque dans la fenêtre (constat du ticket 36, §1.2). Les chemins réels d'une annonce hors fichier sont ceux de ce ticket.

**Non modifié, à décider** : le §11.1 (« Livraison abandonnée… déjà écrite dans le fichier ») ne vaut que pour un Stop arrivé après `turn.rs:1044` (§1.4). Une note peut y renvoyer à ce ticket.

## 7. Points ouverts

- **Durée de la fenêtre A** : non mesurée. Elle couvre la fin de la construction (le démarrage du kernel en fait-il partie ? non lu).
- **Rows perdues si `agent.prompt` refuse après la prise de la boîte** (`admission.rs:184-200`) : la boîte est vidée et les lignes sont perdues, annonce comprise, alors que `known` est déjà à jour. Le changement ne serait jamais annoncé. Vrai pour toutes les lignes du tour suivant, donc défaut d'upstream pour les leurs. Non suivi : la course n'a pas été cherchée.
- **Ticket 34** : les lignes du tour suivant d'upstream ont le même défaut de fond (`run_once.rs:247`), et le problème d'ordre du §5.3.
- **Ligne acceptée écrite puis abandon avant l'admission** : le fichier aurait une ligne de l'utilisateur que le moteur n'a jamais reçue. Ce n'est pas l'annonce, c'est d'upstream ; non suivi.

## 8. Décisions attendues de Yusuf

1. **Corriger ou fermer.**
   - Corriger : O1 (A) et O2 (B), avec T1 et T2.
   - Fermer : noter A et B en limites dans `docs/fork-changes.md`. L'effet visible se limite à une ligne d'annonce absente de l'écran ; le coût en cache ne compte que dans le cas du §3.2.
2. Si on corrige :
   - **O1** : les 2 lignes upstream dans `lifecycle.rs:1280` (ou la variante dans `ya_frozen_tools.rs`) ; le retrait du jeton `delivery_compared` ;
   - **O2** : la forme du §5.2 dans `turn.rs` (+7 / −2, sans toucher aux lignes d'upstream) ;
   - **T1** : la ligne `mod ya_announce_admission;` dans `agent_engine/tests.rs:24`, ou le test dans `ya_abort_window.rs` ;
   - **T2** : la FIFO dans l'e2e, `#[cfg(unix)]`.
3. Le §11.1 du ticket 35 : note de renvoi ou rien (§6.2).

## 9. Décision de Yusuf (25/11/2026) : fermé sans correction

**A et B sont des limites connues**, notées aussi dans `docs/fork-changes.md` (section « Annonce de skills ou de MCP hors fichier : limites »).

- **Effet** : ligne d'annonce absente de l'écran (extension, TUI) et du rejeu à `session/load` ; le modèle est bien informé.
- **Cache** : un miss non journalisé possible, dans un seul cas (état du disque revenu à celui de l'ouverture), estimé, non mesuré (§3.2). Sinon aucun coût de plus.
- **Gardé pour le cas où on l'observerait** : O1 et O2 (§5), T1 et T2 (§4).
- **Ticket 35, §11.1** : note de renvoi ajoutée (vrai pour un Stop après `turn.rs:1044`, voir le cas B).
- **`docs/fork-changes.md`, section du ticket 35** : la phrase « pour les continuations autonomes du démon, l'admission compare comme avant » est corrigée comme le §11.3 du ticket 35.
