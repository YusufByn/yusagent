# Ticket 23 : passivation inactive différée (constat)

Constat du 20/11/2026, sur `main` à `99e801f50`. **Constat seulement** : aucun code modifié. Pas de correctif sans décision de Yusuf.

## Origine

Point « À reprendre (avant YA-10) » du ticket 19 (`docs/tickets/19-ya16-cli-yusagent.md:324`) et section « Instabilité probable : `agent_family_e2e`, passivation (YA-16) » de `docs/fork-changes.md`. Test `passivated_child_recent_messages_reads_its_file_end_to_end`, run `ci` 44604335224, tentative 1, job 135844074120 (`test shard 1/8`). Le journal du job ne contient que la panique, sans trace du démon :

```
panicked at crates/pa-daemon/tests/agent_family_e2e.rs:1500:5:
the passivation stopped kid-a's worker
```

Commande : `gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/135844074120/logs`, lignes 1464-1478.

## 1. Constat

### 1.1 L'hypothèse du ticket 19 ne tient pas

L'hypothèse était que le superviseur répond `success` **avant** d'avoir retiré le worker des sessions résidentes. Quand l'arrêt a vraiment lieu, c'est faux :

- `success` part seulement après le retour de `stop_worker_releasing` (`crates/pa-daemon/src/supervisor/worker_lifecycle.rs:719-724`).
- `stop_worker_releasing` écrit la pierre tombale (`:753`), libère le verrou d'éviction (`:763`), envoie `shutdown`, attend la fin du processus, puis retire le worker du registre (`:797`) avant de rendre `Ok`.
- Le `list` sans `all` lit ce registre (`crates/pa-daemon/src/supervisor/sessions.rs:534-535`).

Donc, après un arrêt effectif, le `list` qui suit ne peut plus montrer kid-a.

### 1.2 Ce qui se passe : `success` veut aussi dire « différé »

`handle_worker_idle_passivation` (`crates/pa-daemon/src/supervisor/worker_lifecycle.rs:664`) répond `success` **sans rien arrêter** dans trois cas, et ces trois réponses sont identiques à celle d'un arrêt effectif (pas de `data`) :

| Cas | Ligne | Possible dans le test ? |
|---|---|---|
| `idleEvictionMinutes` à `"off"` | `:695-696` | non : le test écrit `1` (`agent_family_e2e.rs:1369-1373`) |
| seuil annoncé différent du réglage | `:703-704` | non : le test annonce `1` (`:1495`) |
| une requête routée vers le worker est en cours | `:713-717` | **oui** |

Le troisième cas est voulu. Le superviseur essaie de prendre d'un coup les 128 permis de requêtes en cours du worker (`crates/pa-daemon/src/backpressure.rs:15`). Si une seule requête en tient un, il renonce et répond `success`. Le commentaire `:706-712` le dit : « A request in flight defers the passivation; the worker's next park re-asks ». Un test unitaire couvre ce report : `idle_passivation_refuses_a_client_owned_worker`, `crates/pa-daemon/src/supervisor/tests.rs:1764-1793`.

Une requête routée tient son permis pendant toute la durée de la route, de `crates/pa-daemon/src/supervisor/routing.rs:103` jusqu'à la sortie de `route_command_on`, après la réponse du worker (`:183`). Pour une commande de client, l'admission est `ClientRequest` (`routing.rs:651-654`).

### 1.3 Ordre des événements dans le test

1. Le test lit kid-a deux fois par `recent_member` (`agent_family_e2e.rs:1459-1487`). Les deux lectures sont terminées, et leurs permis rendus avant que la réponse remonte au test.
2. Une autre requête routée vers kid-a prend un permis.
3. Le test envoie `worker_idle_passivation` (`:1490-1497`). Le superviseur valide le jeton, vérifie que le worker n'a pas de client propriétaire, relit le réglage (1 = 1), puis échoue à prendre les 128 permis (`worker_lifecycle.rs:713`) et répond `success`.
4. La requête de l'étape 2 se termine.
5. Le `list` du test (`:1500-1503`) contient encore kid-a : panique.

Le report est la seule branche non déterministe du gestionnaire dans ce test. **Non identifiée** : la requête de l'étape 2. Le journal du run ne la nomme pas, et elle ne vient pas des lectures du test (étape 1).

### 1.4 Pour l'utilisateur

- Le seul expéditeur de `worker_idle_passivation` dans le produit est le worker lui-même (`crates/pa-daemon/src/worker/turn.rs:291-300`). Il ignore la réponse (`let _ =`, `:297`). Autres occurrences dans `crates/` : le protocole (`protocol.rs:933`, `:959`), l'aiguillage (`supervisor/clients.rs:688`) et le gestionnaire. Ni ACP ni l'extension (YA-10) n'envoient cette commande : elle s'authentifie par le jeton du worker.
- Après un report, la boucle du worker revient en attente. Elle remet `last_activity_ms` à l'heure courante (`turn.rs:116-120`) et réarme une fenêtre complète (`:130-136`). Le worker reste donc résident une période `idleEvictionMinutes` de plus, puis redemande.
- Aucune perte de données, aucune erreur visible. Coût : un worker et son kernel résidents plus longtemps. Cas limite : si une requête est en cours à chaque échéance, le worker n'est jamais passivé. Non observé.

**Verdict.**
- Dans le test, la course est **réelle** : une fausse alerte intermittente, parce que le test prend `success` pour « arrêté ».
- Dans le produit, ce n'est **pas un défaut** : le report est voulu, et il coûte au plus une période de résidence en plus.

### 1.5 Upstream

- Point de fork : `c83ac68b3` (02/11/2026). `upstream/main` récupéré le 20/11/2026 : `262660387`.
- Commande : `git log --oneline c83ac68b3..upstream/main` sur `supervisor/worker_lifecycle.rs`, `supervisor/clients.rs`, `supervisor/routing.rs`, `worker/turn.rs`, `supervisor/tests.rs`, `tests/agent_family_e2e.rs` et `tests/idle_passivation_e2e.rs` (tous sous `crates/pa-daemon/`). Résultat : 8 commits. Aucun ne change la logique du gestionnaire ni l'attente du test :
  - `0ffe3aba6` (#4001) remplace `note_retired` par `transition(WorkerTransition::RetireStopped)` ; l'ordre de l'arrêt ne change pas ;
  - `12a5081ac` (#3871) réserve deux tests de `idle_passivation_e2e.rs` à Linux ;
  - `e7fd2039a` (#3863) épingle la livraison directe des messages dans les tests ;
  - les autres touchent `clients.rs` ou `turn.rs` ailleurs que la passivation.
- Le verrou d'éviction est identique dans upstream (`upstream/main:crates/pa-daemon/src/supervisor/worker_lifecycle.rs:709-711`).
- **Upstream a le même défaut, en double** : le test est inchangé (`upstream/main:…/agent_family_e2e.rs:1552`), et `c8cbb5ba7` (#3769, absent de notre historique) ajoute un second test avec le même enchaînement `success` puis `list` immédiat (`upstream/main:…/agent_family_e2e.rs:1895-1909`).
- **Rien à reporter.**

## 2. Proposition

### 2.1 Correctif (test seulement, recommandé)

Dans `crates/pa-daemon/tests/agent_family_e2e.rs`, remplacer la demande unique suivie d'un `list` immédiat (`:1489-1503`) par une fonction d'aide `passivate_until_gone` :

- envoyer la demande, exiger `success`, lire `list` ;
- tant que la session est encore résidente : attendre 100 ms et redemander, jusqu'à 30 s ;
- en cas d'échec, afficher le nombre de demandes.

C'est le contrat du produit : un report se rattrape par une nouvelle demande. Cela demande une modification minimale d'un fichier upstream, listée dans `docs/fork-changes.md`. La section « Instabilité probable » de ce test y serait alors close.

**Écarté** : distinguer le report dans la réponse (par exemple `data: {"deferred": true}`). Cela change le protocole du démon (écart avec upstream) pour un seul expéditeur, qui ignore la réponse.

### 2.2 Test qui attrape la course (rouge sans correctif, puis vert)

On rend le report déterministe en tenant une requête en cours pendant la demande :

- Nouveau fichier du fork : `crates/pa-daemon/tests/ya_idle_passivation_defer_e2e.rs`.
- Mise en place : démon, `idleEvictionMinutes: 1`, une session racine sans propriétaire, provider factice, un tour pour la mettre au repos.
- Un second client envoie `execute_bash_and_wait` avec `sleep 2` à cette session (admission `ClientRequest`, permis tenu pendant toute la commande).
- Le test envoie `worker_idle_passivation` pendant ce temps.
- **Premier push : le test seul**, avec l'enchaînement actuel (demande, `success`, `list` immédiat). Attendu : **rouge** en CI, session encore résidente.
- **Push suivant** : le test passe par `passivate_until_gone`. Attendu : **vert**, la demande suivant la fin du `sleep` arrête la session.
- Le test d'origine passe aussi par `passivate_until_gone`. On ne lui ajoute pas la requête tenue : le message de la commande bash s'ajouterait aux lectures `recent_messages` que ce test compare ensuite.

À vérifier en écrivant le test :

- qu'un client non attaché peut router `execute_bash_and_wait` vers la session par `activeSessionId` ;
- que la session créée par `create` sans `noSession` n'a pas de propriétaire (`worker_lifecycle.rs:681-689` refuserait sinon).

Pas de build local (règle 8) : rouge et vert vus en CI.

### 2.3 Préfixe

**Aucun effet sur ce qui part au modèle.** Le correctif et le test ne touchent que des tests du démon. Le gestionnaire, le prompt système et les définitions de tools ne changent pas.

## 3. À trancher par Yusuf

- **P1** : correctif 2.1 (test seulement) ou réponse distincte du report (écartée). Recommandation : **2.1**.
- **P2** : test déterministe 2.2 dans un fichier du fork, vu rouge puis vert en CI. Ou seulement l'aide `passivate_until_gone` dans le test d'origine, sans preuve du rouge.
- **P3** : quand on reprendra `c8cbb5ba7` d'upstream, appliquer la même aide à son second test (`upstream/main:…/agent_family_e2e.rs:1895-1909`).

## Points ouverts

- Requête en cours lors de l'échec du run 44604335224 : non identifiée.
- `docs/fork-changes.md` (hypothèse « répond `success` avant d'avoir retiré le worker ») et le ticket 19 (`:324`) ne sont pas encore corrigés. À faire avec le correctif, avec un renvoi à ce ticket.
