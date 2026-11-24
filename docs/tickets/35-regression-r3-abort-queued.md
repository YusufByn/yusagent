# Ticket 35 : régression probable de R3 sur `abort_and_send_queued_acks_before_the_follow_up_delivery` (constat et plan)

Demande de Yusuf du 24/11/2026. Branche `ya-35-constat`, depuis `main` à `1093d646c`. **Constat et plan seulement, aucun code du produit**, rien compilé en local (règle 8), aucun appel de modèle.

## Note du ticket

- **Régression probable de R3** (PR #83, merge `2e8dfdf41`, ticket 31).
- **Les trois emplacements soupçonnés** :
  - `crates/pa-daemon/src/worker/turn.rs:1041` : `engine.ya_resource_notice()` au début de chaque tour ;
  - `crates/pa-daemon/src/agent_engine/session_engine_impl.rs:1117-1123` : `self.session.blocking_lock()`, puis `runtime.block_on` ;
  - `crates/pa-core/src/session_engine/admission.rs:47` et `:148` : `ya_announce_resources()` à chaque admission, qui relit le disque (réglages, ressources, stockage d'auth par `blocking_lock`, `crates/pa-core/src/mcp/mod.rs:539-548`).
- **Les deux runs rouges** :
  - run `ci` 45376832870, `main` à `2e8dfdf41`, tentative 1, job 138385395316 (shard 3/8) ;
  - run `ci` 45429516827, PR #84 (`ya-31-x3`, `559823811`), tentative 1, job 138570835612 (shard 3/8) ; vert à la tentative 3 (job 138648679736).
- Le test n'est **pas** listé en « Instabilité probable » et ne doit pas l'être : le constat ci-dessous désigne une cause dans notre code.

## Résumé

L'hypothèse d'une attente sur le verrou est **écartée** pour ce test : personne d'autre ne prend `self.session` pendant ce scénario (§3). La cause trouvée est autre : **un abandon perdu**.

- Upstream a une fenêtre connue où un abandon se perd : entre la vérification du drapeau d'abandon en tête de l'admission (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:333-339`) et l'enregistrement du run dans l'agent (`crates/pa-agent/src/agent.rs:606-611`). `Agent::abort` ne fait rien quand aucun run n'est enregistré (`agent.rs:1108-1112`). Le commentaire d'upstream le dit : « an abort landing in the [pickup, registration] window is otherwise lost » (`crates/pa-daemon/src/agent_engine/turn/model.rs:89-92`).
- Avant R3, rien de lent dans cette fenêtre : des clones et des verrous, pas d'entrée-sortie (lu, durée non mesurée).
- **R3 a mis une lecture du disque dans cette fenêtre** : `admission.rs:148` (prompt de l'utilisateur), `:47` (ligne injectée). Lecture des fichiers de réglages, des skills et de l'état MCP par `spawn_blocking` (`crates/pa-core/src/session_engine/ya_resource_notice.rs:381-384`, `current`, `:137-173`). Elle a été mesurée à une médiane de 1 742 µs et un maximum de 19 258 µs sur 28 prompts, sur le binaire macOS (ticket 31, §8.5). Ce n'est pas une mesure de la CI.
- Dans ce test, le provider factice retient sa réponse 60 s avant tout événement. Le premier run d'un tour ne transmet ni `agent_start`, ni `turn_start`, ni la ligne de l'utilisateur (`run_once.rs:163-178`, `:248-257`). Un abandon tombé dans la fenêtre n'est donc rattrapé par rien avant 60 s. Le tour reste occupé, et le test échoue à `queue.rs:725` après 10 s.
- `turn.rs:1041` n'est **pas** dans la fenêtre pour ce test (§2.2). `admission.rs:148` l'est.

## 1. Ce que le test attend, et dans quel délai

`crates/pa-daemon/src/worker/turn_stream_tests/queue.rs:604-765`, `#[tokio::test]`, donc un runtime à un seul thread (`:605`). Le verrou global du provider factice est pris en tête (`:608-610`).

1. Worker sur le moteur `faux`, deux réponses retenues 60 s chacune (`"delayMs": 60000`, `:624-627`). La session prend le vrai moteur, `AgentSessionEngine` sur le provider factice (`crates/pa-daemon/src/worker.rs:391-414`).
2. Prompt « held turn… », attente que le tour soit admis (`busy`, 10 s, `:648-658`), puis 400 ms (`:659`).
3. `follow_up` « ack follow-up » (`:660-662`).
4. `abort_and_send_queued` doit répondre **en moins de 5 s** (`:665-677`), alors que tout ce qui suit est retenu.
5. Le suivi doit démarrer : `busy` et file vide **en moins de 10 s** (`:678-692`).
6. Lignes de l'utilisateur dans le fichier : exactement les deux prompts (`:693-714`).
7. `abort` simple (`:718-722`), puis le tour du suivi doit se terminer : `busy` à faux **en moins de 10 s**. **C'est l'assertion qui échoue** (`:723-730`, message « the follow-up's turn never settled on the bare abort » en `:727`, panique à `:725:9`).
8. Ensuite : file vide, `queued_input_suspended` à vrai (`:731-741`), mêmes deux lignes (`:742-763`).

Les deux runs rouges ont passé les assertions 4 à 6 et échoué sur la 7. Dans les deux cas, le journal porte aussi « has been running for over 60 seconds » (job 138385395316, ligne 2718 du journal ; job 138570835612, ligne 2734). Explication déduite, non vérifiée : à la panique, le runtime du test se ferme et attend la tâche `spawn_blocking` du tour (`crates/pa-daemon/src/worker/turn.rs:433`). Cette tâche est encore dans l'attente de 60 s du provider factice. C'est cohérent avec un tour resté dans l'attente du provider, non annulé.

## 2. Les appels de R3 sont-ils sur le chemin du test, et dans quel contexte ?

### 2.1 Chemin du tour du suivi

Lu dans l'ordre d'exécution :

| Étape | Où | Thread / runtime |
|---|---|---|
| prise du suivi, `abort_requested = false` | `turn.rs:73-83` | tâche du runner, runtime du test |
| tour lancé dans `spawn_blocking` | `turn.rs:433` | thread bloquant du runtime du test |
| **R3** : `engine.ya_resource_notice()` | `turn.rs:1041` → `session_engine_impl.rs:1117-1123` | même thread ; `blocking_lock` sur `self.session` (`tokio::sync::Mutex`, `crates/pa-daemon/src/agent_engine/lifecycle.rs:158`), puis `block_on` sur le runtime multi-thread du moteur (`lifecycle.rs:33`, `crates/pa-daemon/src/async_safe_runtime.rs:17-20`), où `ya_take_resource_notice` lit le disque dans un `spawn_blocking` (`ya_resource_notice.rs:381-384`) |
| ligne du prompt acceptée et écrite | `session_engine_impl.rs:1434` (persistée par `turn.rs:615`, `:728`) | même thread |
| **R3** : `ya_compaction_enabled` (relit les réglages) | `crates/pa-core/src/session_engine/wiring.rs:167`, `:186`, via la compaction automatique avant le tour (`crates/pa-daemon/src/agent_engine/turn/run_loop.rs:48-58`) | même thread |
| 1re vérification de l'abandon | `model.rs:93` | même thread |
| `block_on` du tour | `model.rs:347` | runtime du moteur |
| 2e vérification, en tête du futur d'admission | `run_once.rs:337` | runtime du moteur |
| clone de la session | `run_once.rs:345` | runtime du moteur |
| **R3** : `ya_announce_resources()` | `admission.rs:148`, appelé par `run_once.rs:378-381` | runtime du moteur ; lecture du disque dans un `spawn_blocking` |
| enregistrement du run | `agent.rs:591-611` (`*run = Some(ActiveRun …)` en `:608`) | runtime du moteur |
| attente de 60 s du provider factice, avant tout événement | `crates/pa-ai/src/providers/faux/mod.rs:784-799` | runtime du moteur |

`admission.rs:47` (ligne injectée) est la même fenêtre pour un tour injecté. Ce test ne le prend pas : le suivi est un prompt simple (`TurnPrompt::User`, `run_once.rs:362-382`).

### 2.2 Où tombe l'abandon simple du test

- Le test envoie l'abandon juste après `get_messages` (`queue.rs:693-718`).
- `get_messages` voit déjà la ligne « ack follow-up » : l'assertion `:708-714` passe dans les deux runs rouges. La ligne n'est écrite que par `emit` (`turn.rs:615`, `:728`), appelé en `session_engine_impl.rs:1434`.
- Le thread du tour avait donc **déjà passé `turn.rs:1041`** quand l'abandon est arrivé. Cet appel de R3 n'est pas dans la fenêtre pour ce test.
- Restent, entre la ligne acceptée et l'enregistrement du run :
  - `wiring.rs` (avant les vérifications : un abandon tombé là est vu par `model.rs:93`) ;
  - **`admission.rs:148`, après la vérification de `run_once.rs:337`**.

### 2.3 Pourquoi un abandon tombé dans la fenêtre est perdu

1. `handle_abort` (`crates/pa-daemon/src/worker/commands.rs:376-379`) → `request_abort` (`:328-374`) met `abort_requested` (`:331`), puis appelle `abort_in_flight_turn` (`:371`).
2. `abort_in_flight_turn` (`session_engine_impl.rs:1485-1491`) appelle `Agent::abort` (`agent.rs:1108-1112`), qui ne fait rien sans run enregistré.
3. Le drapeau `abort_requested` n'est plus relu par l'admission après `run_once.rs:337`.
4. Le seul rattrapage restant est la porte d'`emit` (`turn.rs:513-523`) : elle rend `false` et `run_once` appelle alors `agent.abort()` (boucle `run_once.rs:397-418`, appel en `:431`). Mais au premier run d'un tour, l'abonné ne transmet ni `AgentStart` ni `TurnStart` (`run_once.rs:248-257`). Il ne transmet pas non plus la ligne de l'utilisateur (`:230-239`), seulement le début du message de l'assistant (`:163-178`).
5. Le provider factice retient ce début 60 s (`faux/mod.rs:784-799`). Aucun événement n'atteint `emit` avant 60 s.
6. Le tour reste `busy`, et `queue.rs:725` échoue à 10 s.

### 2.4 Upstream a cette fenêtre aussi

À `upstream/main` (`31bd73967`, 24/11/2026), la vérification est toujours en tête du futur (`run_once.rs:345-352` chez eux), et `Agent::abort` ne fait toujours rien sans run (`agent.rs:1209-1213` chez eux). Le défaut d'origine est donc à upstream ; R3 l'a rendu visible en mettant une entrée-sortie dans la fenêtre.

## 3. Qui tient `self.session` ?

Lu dans les chemins du scénario :

- **`abort_and_send_queued` et `abort`** (`commands.rs:299-322`, `:328-374`) : verrou `core` (std), `prompt_admissions`, `compaction.abort()`, puis `abort_in_flight_turn`, qui ne prend que `turn_agent` (std, `session_engine_impl.rs:1487`). Aucun ne prend `self.session`.
- **`get_messages`** (`commands.rs:442` et suivantes) : verrou `core` seul.
- **Fin du premier tour** : `on_turn_done` prend `self.session.lock().await` dans un `block_on` (`session_engine_impl.rs:964-975`), sur le thread du premier tour, avant que ce tour rende la main. Le runner attend la fin du tour (`turn.rs:1068`) avant de prendre le suivi.
- **Début du suivi** : tous les preneurs sont sur le même thread, l'un après l'autre :
  - `turn.rs:1041` (R3), qui **garde le verrou pendant toute la lecture du disque** (`session_engine_impl.rs:1118-1122`) ;
  - `run_loop.rs:220`, `:226` ;
  - `model.rs:122`, `:133` ;
  - `run_once.rs:345`.

Le `blocking_lock` de R3 ne peut donc attendre derrière personne dans ce test. **Hypothèse écartée pour cet échec.**

Non audité en entier : les lectures du moteur faites par la mise à jour du roster (`ModelDetails::read`, `crates/pa-daemon/src/worker/summary.rs:400-415`). Elles tournent sur le thread du test, en contexte async, où un `blocking_lock` paniquerait.

Écart à noter quand même : `session_engine_impl.rs:1118-1122` garde `self.session` pendant `block_on`, donc pendant la lecture du disque. La règle d'upstream est de ne garder le verrou que le temps du clone : « holding it across the turn serialized every client read seam (the 2026-09-22 dogfood failure) » (`run_once.rs:341-345`). En usage réel, une lecture cliente (`resource_snapshot`, `session_engine_impl.rs:1126-1131`) attendrait la fin de la lecture du disque. Ce n'est pas la cause de cet échec.

## 4. Coût de `ya_announce_resources()` à chaque admission

Ce que fait chaque appel (`current`, `ya_resource_notice.rs:137-173`) :

- lecture des réglages global et projet (`SettingsManager::create`, `:138`) ;
- `McpManager::prompt_gating` (`:151-152`, `crates/pa-core/src/mcp/mod.rs:554-577`), qui crée un `AuthStorage` neuf. Son `blocking_lock` (`mod.rs:539-548`) porte sur ce stockage neuf : sans concurrence. Il ne lit `auth.json` que pour un serveur HTTP déclaré (`mod.rs:487-537`) ;
- `load_resources` (`:155-165`) : parcours des dossiers de skills et lecture des `SKILL.md`. Un paquet configuré manquant serait installé à ce moment (ticket 31, §8.3).

Dans le démon, chaque tour compare **deux fois** : `turn.rs:1041`, puis `admission.rs:148` (ou `:47`). La seconde ne trouve jamais de différence après la première (ticket 31, §8.4, garde-fou 1), mais lit le disque en entier.

Ordre de grandeur :

- **mesuré** (ticket 31, §8.5, trace `ya.resource_notice.compare` de `acp_live_changes.py`, binaire macOS `8205f0bbe`) : médiane 1 742 µs, maximum 19 258 µs, sur 28 prompts. Ces chiffres mêlent les deux appels ;
- **non mesuré** : la CI Linux, avec un shard qui fait tourner 1 245 tests de `pa-daemon` en parallèle (« 1245 passed; 1 failed », job 138570835612). **Estimation** : du même ordre ou plus, selon la charge du disque ;
- **avant R3**, la fenêtre ne contenait aucune entrée-sortie. **Estimation** : quelques µs à quelques dizaines de µs.

Ce coût suffit-il à expliquer l'échec ?

- Le test envoie l'abandon peu après la ligne acceptée, à un moment qui ne dépend que de son sondage toutes les 25 ms (`queue.rs:691`).
- Entre la ligne acceptée et l'enregistrement du run, la fenêtre passe d'une part négligeable à la part principale du trajet : c'est là que l'abandon a le plus de chances de tomber.
- **Plausible, non prouvé** : 2 échecs sur 13 exécutions du test avec le code du produit de R3, aucun sur les 49 exécutions lues du 21/11 au 23/11 (§5). Si les 2 échecs étaient répartis au hasard sur les 62 exécutions, la probabilité qu'ils tombent tous deux dans les 13 dernières serait C(13,2)/C(62,2) = 78/1891 ≈ 0,04 (calcul, pas une mesure). L'échantillon reste petit. La preuve viendrait du test déterministe du §7.

## 5. Historique CI

Commandes (scratchpad de la tâche, scripts `scan.sh` et `scan2.sh`) :

```sh
gh api --paginate "repos/YusufByn/yusagent/actions/workflows/ci.yml/runs?per_page=100" \
  --jq '.workflow_runs[]|"\(.id) \(.run_attempt) \(.conclusion) \(.created_at) \(.head_sha[0:9]) \(.head_branch)"'
# scan.sh : pour chaque run en échec ou relancé, chaque tentative, chaque job « test shard » en échec :
gh api repos/YusufByn/yusagent/actions/runs/<run>/attempts/<n>/jobs
gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/<job>/logs | grep -c "abort_and_send_queued_acks_before_the_follow_up_delivery ... FAILED"
# scan2.sh : même chose sur TOUS les jobs « test shard » des runs depuis le 21/11/2026, en gardant la ligne du test
```

Lancés le 24/11/2026. Liste : 318 runs `ci.yml` (du 03/11 au 24/11/2026). Une tentative recopiée par GitHub lors d'une relance d'un autre job porte un nouvel identifiant de job, mais le même journal (même `md5`, même horodatage) : ces copies sont comptées une seule fois.

**Échecs du test, sur tout l'historique** :

| Run | Commit | Tentative, job | Résultat |
|---|---|---|---|
| 45376832870 | `2e8dfdf41` (`main`, merge de R3), 24/11 10:39 | 1, job 138385395316 (shard 3/8) | **échec** à `queue.rs:725`, « has been running for over 60 seconds » |
| 45429516827 | `559823811` (`ya-31-x3`, PR #84), 24/11 15:47 | 1, job 138570835612 (shard 3/8) | **échec**, même symptôme |
| 45429516827 | `559823811` | 2, job 138641191973 | copie du journal de la tentative 1 (seul le shard 8 a été relancé) |
| 45429516827 | `559823811` | 3, job 138648679736 | test réussi |

**Avant R3** :

- Tous les jobs « test shard » en échec de tout l'historique ont été lus (`scan.sh`, 124 journaux de jobs) : le test n'y échoue jamais.
- 5 journaux sont expirés (`BlobNotFound`) : runs 41702752565, 41707124985, 41817921128, 41879406724 (03-04/11, `main` et `ya-04`) et 41908500409 (`ya-08`, 04/11). Un run (43092602383) n'a de job en échec que dans `clippy`.
- Du 21/11 au 23/11, tous les jobs des shards ont été lus (`scan2.sh`) : 49 exécutions du test, toutes `ok`. Cela couvre `main` et les branches `ya-24` à `ya-30`, `ya-28-x1`, plus le push 1 de R3 (`1f55754f0`, tests seuls, sans code du produit).

**Avec le code du produit de R3** (depuis le push 2, `2b1c6c668`, 23/11 15:38) : 13 exécutions, 2 échecs.

- Branche R3 : `2b1c6c668`, `8205f0bbe`, `574ed9f03`, `34bc9b3ef` : `ok`.
- `main` : `2e8dfdf41` **échec** ; `1093d646c` (merge de X3) `ok`.
- X3 : `8ff03bf04`, `ca2e562f5`, `02c32497e`, `9baeed346`, `4fadff2c4` : `ok` ; `559823811` **échec** à la tentative 1, `ok` à la tentative 3.
- Les runs 45410067046 et 45410771304 (X3) ont été annulés avant ce test.

Sur `main`, le run 45376832870 n'a pas été relancé (`run_attempt` 1, conclusion `failure`) : le run vert suivant sur `main` est 45453230174 (`1093d646c`).

## 6. Upstream depuis notre base

- Base commune : `c83ac68b3` (02/11/2026). `upstream/main` : `31bd73967` (24/11/2026), 161 commits de plus.
- **Le test** : chez upstream, seule différence avec le nôtre, il ne prend plus `FAUX_TEST_LOCK` (`0bce1417b`, « give each scripted faux provider its own api so faux tests run in parallel », #3846). Le corps et les délais sont identiques (`diff` du corps de la fonction entre `origin/main` et `upstream/main` : 3 lignes en moins chez eux).
- **Le chemin**, commits d'upstream qui le touchent, absents de notre `main` (`git log origin/main --grep` vide pour chacun) :
  - `be86884dd` (#4124, 21/11) : retire le « parked seat » du démon, garde de fin de run dans `pa-agent`. Récrit `run_once.rs` (276 lignes) et `agent.rs`. **Ne ferme pas la fenêtre** : la vérification reste en tête du futur (`run_once.rs:345-352` chez eux).
  - `cc372f293` (#4035, 19/11) : `pa-agent` prend le slot du run avant de vider les messages en file. Même remarque.
  - `6679f881d` (#4073, 20/11) : un abandon **avant le flux du provider** rend `TurnOnce::Aborted` au lieu d'une erreur. Il suppose que l'abandon atteint le run, donc qu'il est arrivé après l'enregistrement ; il ne couvre pas la fenêtre.
  - `fda67368b` (#4102, 23/11) : un abandon demandé l'emporte sur un résultat prêt dans le même `poll` (`race_with_abort`). Autre course.
- Commits d'upstream qui touchent `queue.rs` depuis la base : `0bce1417b`, `e3b7f7118`, `9cff13ca5`, `a10c3e2a5`, `783acb5d7` (`git log c83ac68b3..upstream/main -- …/queue.rs`). Aucun ne change ce test, sauf le verrou retiré par `0bce1417b`.
- **Conclusion** : upstream n'a ni corrigé la fenêtre ni changé ce test d'une façon qui l'expliquerait. Rien à reprendre d'upstream qui réglerait seul le problème.

## 7. Voir le test rouge sur son assertion, sans dépendre de l'horloge

Le principe : placer l'abandon **dans la fenêtre** par un point d'arrêt, pas par une attente.

### 7.1 Test du défaut d'origine, au niveau du moteur (recommandé)

- **Fichier** : un nouveau fichier de test du fork, `crates/pa-daemon/src/agent_engine/tests/ya_abort_window.rs`, sur le modèle d'`abort_in_flight_turn_cancels_a_mid_provider_wait` (`crates/pa-daemon/src/agent_engine/tests/abort.rs:8-110`).
- **Ce qu'il fait** :
  - `run_prompt` reçoit une sonde `aborted` à état. Elle rend `false` aux vérifications de `model.rs:93` et `run_once.rs:337`. À la seconde, avant de rendre `false`, elle arme l'abandon comme le worker : drapeau levé et `engine.abort_in_flight_turn()` ;
  - les appels suivants rendent `true` ;
  - l'`emit` du test rend `!drapeau`, comme la porte du worker (`turn.rs:513-523`).
- **Pourquoi c'est déterministe** : l'abandon tombe exactement entre la vérification et l'enregistrement, quelle que soit la vitesse du disque.
- **Assertions, sans délai** :
  - le provider factice ne reçoit **aucune requête** (`registration.call_count() == 0`, compteur déjà utilisé en `abort.rs:90`). La réponse scriptée est immédiate (`delayMs: 0`) : si l'abandon est perdu, la requête part, et le test est rouge sur cette assertion, pas sur un délai ;
  - le tour se termine abandonné (`DoneAborted`).
- **Attendu** : rouge aujourd'hui, et rouge aussi sans R3 : il vise la fenêtre, pas la lecture du disque. Vert avec l'option C du §8 seulement.
- **Fichier upstream touché** : le `mod` du nouveau fichier dans `crates/pa-daemon/src/agent_engine/tests.rs` (`:10` y déclare `mod abort;`), **1 ligne**. À valider avant (CLAUDE.md, règle 2).

### 7.2 Le test d'upstream lui-même rouge à `queue.rs:725`, à chaque fois (à vérifier avant de l'écrire)

- **Le point d'arrêt** : bloquer la lecture du disque de `admission.rs:148` jusqu'à ce que l'abandon soit envoyé. Sans code du produit : un `SKILL.md` en FIFO dans `agent_dir/skills/<nom>/`, lu par `load_resources` à chaque comparaison (`crates/pa-core/src/skills/loader.rs:126`, lecture par `std::fs::read_to_string`, `crates/pa-core/src/skills/discovery.rs:73`). L'ouverture en lecture bloque jusqu'à ce qu'un écrivain ouvre l'autre bout.
- **Le scénario** : une copie du scénario de `queue.rs:604-765` dans un fichier du fork. Un thread sert la FIFO librement, sauf après la ligne « ack follow-up ». Là, il attend que le test ait envoyé l'`abort` simple, puis écrit. L'ouverture côté écrivain ne rend la main que quand le tour est bloqué dans la lecture : le test sait donc qu'il est dans la fenêtre.
- **Attendu** :
  - rouge à `:725` à chaque exécution avec le code d'aujourd'hui ;
  - vert avec les options A, B ou C du §8 ;
  - rouge sur `main` avant R3 impossible : aucune lecture dans la fenêtre, le test ne bloque pas.
- **À vérifier avant de l'écrire (non lu)** :
  - que la découverte des skills ne filtre pas les fichiers qui ne sont pas réguliers (`is_file`) ;
  - qu'aucun cache n'évite la relecture (le cache de `crates/pa-core/src/settings/storage.rs:190-217` ne vaut que pour les réglages) ;
  - Unix seulement (`#[cfg(unix)]`), création par `mkfifo`.
- **Fichiers** : nouveau fichier de test du fork, plus **1 ligne** `mod` dans `crates/pa-daemon/src/worker/turn_stream_tests.rs` (upstream, `:16` y déclare `mod queue;`).

## 8. Options de correction (non codées)

Contraintes communes :

- **préfixe de prompt** : aucune option ne touche au prompt système, aux tools ni au texte des annonces ; le préfixe reste inchangé ;
- **à vérifier pour chacune** : `compare_capture_prompts.py` donne TOUT IDENTIQUE, et `acp_live_changes.py` L1 à L6 CONFORME.

### Option A : dans le démon, la comparaison du worker est la seule pour la livraison (fork seul)

- **Le changement** :
  - `ya_take_resource_notice`, appelé par le worker (`turn.rs:1041`), laisse dans le tracker un jeton « comparé pour cette livraison » ;
  - la prochaine admission (`admission.rs:47`, `:148`) consomme ce jeton au lieu de relire le disque ;
  - les admissions suivantes du même tour (continuations autonomes, `run_loop.rs`) relisent comme aujourd'hui ;
  - en plus, `session_engine_impl.rs:1117-1123` clone l'`Arc` de la session puis relâche le verrou avant `block_on`, comme `run_once.rs:345`.
- **Fichiers** : `ya_resource_notice.rs` (fork) et les lignes de R3 dans `session_engine_impl.rs`, déjà listées dans `docs/fork-changes.md`. **Aucune ligne upstream de plus** : `admission.rs` garde ses deux appels.
- **Effet sur l'annonce du ticket 31** :
  - démon : inchangée. Le worker annonce avant la ligne du prompt, et l'admission ne trouvait déjà plus rien (ticket 31, §8.4) ;
  - un changement fait sur le disque entre `turn.rs:1041` et l'admission (quelques ms) est annoncé au tour suivant au lieu de celui-ci. Aujourd'hui, il atteint le modèle sans être écrit dans le fichier (ticket 34) : l'option A supprime ce cas ;
  - `-p` et RPC sans démon : inchangés.
- **Limites** :
  - la fenêtre reste élargie pour les continuations autonomes d'un même tour, et en `-p` / RPC ;
  - la fenêtre d'upstream reste.
- **Test qui doit l'attraper** :
  - celui du §7.2 : vert ;
  - plus un test du fork sans horloge : une livraison du démon fait **une** comparaison, pas deux. Il compte les traces `ya.resource_notice.compare` (`ya_resource_notice.rs:386-389`) d'un tour : 2 aujourd'hui, 1 attendu. À voir rouge sur cette assertion avant le code.

### Option B : une comparaison rapide dans la fenêtre (fork seul)

- **Le changement** : `current` ne relit tout que si une empreinte change. L'empreinte : `mtime` et taille des fichiers de réglages, des dossiers de skills et de leurs `SKILL.md`. Elle demande des `stat` seulement, pas de lecture ni de chargement.
- **Coût dans la fenêtre** : **estimé** de quelques ms à quelques dizaines de µs, selon le nombre de skills. Non mesuré.
- **Fichiers** : `ya_resource_notice.rs` seul.
- **Effet sur l'annonce** : aucun, si l'empreinte couvre tout ce que `current` lit.
- **Risques** :
  - un changement que l'empreinte ne voit pas, par exemple un `mtime` inchangé sur certains systèmes de fichiers, ou un jeton MCP qui expire. Pour l'état MCP, qui dépend de l'heure et des variables d'environnement (`mod.rs:530-533`), l'empreinte ne suffit pas : il faudrait le relire quand même ;
  - la fenêtre reste réduite, pas fermée : le §7.2 passe au vert, mais une course rare demeure.
- **Test** : le §7.2, plus un test qui modifie un `SKILL.md` en gardant sa taille et vérifie l'annonce.

### Option C : fermer la fenêtre d'upstream (fichier upstream, candidat à une PR upstream)

- **Le changement** : le drapeau d'abandon de la livraison est relu **après** l'enregistrement du run.
- **Variante C1** :
  - où : `run_once.rs`, dans la boucle de transfert (`:393-415`) ;
  - quoi : dès que le run est enregistré (`agent.signal().is_some()`), relire `aborted()` une fois, et appeler `agent.abort()` s'il est levé ;
  - lignes upstream : **estimation 6 à 10 lignes**, à donner exactement avant de toucher.
- **Variante C2** :
  - où : `pa-agent` ;
  - quoi : `Agent::abort` sans run arme un abandon que le prochain run consomme ;
  - risque : changement de sémantique (un abandon à vide tuerait le tour suivant). Il faudrait le désarmer à la prise du tour. **Plus risquée, non recommandée.**
- **Effet sur l'annonce et le préfixe** : aucun.
- **Fichiers** : `crates/pa-daemon/src/agent_engine/turn/run_once.rs` (upstream, zone récrite par `be86884dd` chez eux, donc conflit probable au prochain `cherry-pick`).
- **Ce qu'elle couvre** : tous les chemins du démon, avec ou sans lecture du disque.
- **Test** : celui du §7.1, rouge avant, vert après. C'est aussi le test à proposer à upstream.

### Recommandation

**A tout de suite, avec le test de §7.2 et le test de comptage.** A retire la cause introduite par R3 sans toucher de ligne upstream, et rend au verrou de session sa règle d'upstream.

**C à part**, comme ticket ou PR upstream, avec le test du §7.1, puisque la fenêtre est à upstream. B n'apporte rien de plus qu'A dans le démon, et laisse une course.

En attendant la décision : le test reste hors de la liste « Instabilité probable ». Un nouvel échec sur `main` avec ce symptôme est attendu tant que rien n'est corrigé ; la relance du job seul reste la règle de la CI, sans ajouter le test à la liste.

## 9. Points ouverts

- `-p` et RPC sans démon : la même fenêtre existe-t-elle pour leur commande d'abandon ? Non lu.
- Le coût de `ya_compaction_enabled` (`wiring.rs:167`, `:186`) à chaque décision de compaction : lu, non mesuré. Il n'est pas dans la fenêtre.
- Les deux vérifications du §7.2, avant d'écrire ce test.
