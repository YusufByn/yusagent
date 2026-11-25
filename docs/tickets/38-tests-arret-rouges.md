# Ticket 38 : deux tests upstream rouges depuis le ticket 36 (constat et plan)

Demande de Yusuf du 25/11/2026. Branche `ya-38-constat`, depuis `main` à `adfc969d6` (merge de la PR #88, ticket 32). **Constat et plan seulement** : aucun code du produit, aucun cherry-pick commité, rien dans `docs/fork-changes.md` ni dans les sections « Instabilité probable », rien compilé en local (règle 8), aucun appel de modèle.

Toutes les lignes citées sont celles de `main` à `adfc969d6`, sauf mention contraire. Les essais de cherry-pick ont été faits dans un worktree jetable, supprimé ensuite.

## Résumé

- **`a_hung_worker_is_killed_within_the_escalation_window_and_its_lease_frees`** :
  - `7ea89013e` (#3875) ne touche que **deux fichiers de test** et aucun code du produit (§1.1).
  - Il s'applique proprement sur `main` (§1.2).
  - Son message décrit le symptôme observé mot pour mot : un kill auquel le worker répond `success` (§1.3).
- **`tui_close_reason_rows_survive_late_turn_status`** :
  - **Correction d'un fait de départ : ce test ne passe pas par `run_turn_once`.** Son script n'a pas `"engine": "faux"`, donc le worker tourne sur `ScriptedEngine`, pas sur `AgentSessionEngine` (§2.1).
  - Le Kill arrête le tour par le drapeau `abort_requested` du worker, lu par `ScriptedEngine` pendant l'attente du script (§2.2).
  - Aucun fichier modifié par `a56e9ac89` n'est sur ce chemin. Aucun fichier de `crates/` modifié par les tickets 35, 36, 37 et 32 non plus (§2.3).
  - Le journal de l'échec montre que **le tour s'est bien arrêté** (« turn failed: prompt cancelled » est rendu). C'est **la ligne de fermeture qui n'arrive pas** en 30 s (§2.4).
- **`f09c6538e`** (#4178) :
  - Il corrige la branche du `select!` qui lit les entrées du plan headless. Cette branche est commune à toutes les barrières, pas seulement à WaitIdle.
  - Le chemin WaitRender de ce test **passe par cette branche** dans l'état que le commit corrige (§3.2).
  - Il s'applique proprement sur `main` (§3.3).
  - Qu'il soit la cause de l'échec observé est une hypothèse, non vérifiée.

## 1. `stop_escalation_e2e` et `7ea89013e`

### 1.1 Ce que touche le commit

`git show --stat 7ea89013e` :

| Fichier | Lignes de `main` remplacées | Ajout |
|---|---|---|
| `crates/pa-daemon/tests/stop_escalation_e2e.rs` | `:278-283` (le `kill -STOP` du test `a_hung_worker_…`, fonction à `:258`) remplacées par `freeze(worker_pid)` | `freeze` et `all_threads_stopped` après `process_alive` (`:88`) |
| `crates/pa-daemon/tests/registration_refusal_heal_e2e.rs` | le `kill -STOP` du test `an_owned_stop_kills_a_worker_that_missed_the_shutdown` (fonction à `:438`, `-STOP` à `:477`) | les deux mêmes fonctions |

Total : 2 fichiers, 68 lignes ajoutées et 12 retirées. **Tests seuls, aucun code du produit, aucun `.changes`.**

`freeze` envoie `SIGSTOP`, puis attend (10 s au plus) que chaque `/proc/<pid>/task/<tid>/stat` soit à l'état `T`.

### 1.2 Dépendances et application

- **Imports déjà présents** : `Command`, `Duration` et `Instant` sont importés dans les deux fichiers (`stop_escalation_e2e.rs:29-30`, `registration_refusal_heal_e2e.rs:14-15`).
- **Linux seulement** : les deux fichiers le sont déjà (`stop_escalation_e2e.rs:24`, `registration_refusal_heal_e2e.rs:9`), donc la lecture de `/proc` ne pose pas de problème de plateforme.
- **Aucun nom en collision** : pas de `fn freeze` sur `main` dans ces fichiers.
- **Application** dans un worktree jetable à `adfc969d6` :

  ```
  git worktree add --detach <tmp>/wt38 adfc969d6
  git cherry-pick --no-commit 7ea89013e   # exit=0, aucun conflit
  git diff --cached --stat                # 2 files changed, 68 insertions(+), 12 deletions(-)
  ```

- **Compilation non vérifiée** (règle 8 : pas de build Rust local).

### 1.3 Symptôme

- **Échec observé** : run 45602379325, job 139164539614, tentative 1, commit `167887341`. Journal lu par `gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/139164539614/logs` :

  ```
  panicked at crates/pa-daemon/tests/stop_escalation_e2e.rs:293:5:
  assertion `left == right` failed: the hung worker never answers: {"id":"kill","type":"response","command":"kill","success":true}
  test result: FAILED. 1 passed; 1 failed; ... finished in 32.13s
  ```

- **Message de `7ea89013e`** : « kill -STOP returns once the stop is queued, not once every thread of the worker parked: a runtime thread still running serves the next command (it acks the owned stop's shutdown, **or answers kill with success**) ».
- **Fait** : le symptôme correspond au texte.
- **Hypothèse, non vérifiée** : c'est la même cause. Le test n'envoie aucun prompt : sa séquence est create, kill, reopen (`stop_escalation_e2e.rs:267-338`).

## 2. `tui_close_reason_rows_survive_late_turn_status` : chemin du Kill

### 2.1 Le moteur du worker : `ScriptedEngine`, pas `AgentSessionEngine`

- **Lignes** : le test est à `crates/pa-cli/tests/interactive_daemon_e2e.rs:5670`, et la fonction appelée à `:5679`. Le `:5794` cité au ticket 32 est l'`assert!` qui a échoué (le `panicked at …:5794:5` du journal).
- **Script du test** (`:5684-5686`) : `{ "responses": [{ "text": "reply must not arrive", "delayMs": 60_000 }] }`, **sans `"engine"`**.
- **Transmission du script** :
  - `create_session_via_daemon` écrit le script sur disque et passe son chemin dans `config.script` (`:421`, `:435`) ;
  - le superviseur le transmet au worker par `PRIME_AGENT_INTERNAL_DAEMON_WORKER_SCRIPT` (`crates/pa-daemon/src/descriptor.rs:100-102`) ;
  - le worker le lit (`crates/pa-daemon/src/worker/config.rs:62-67`).
- **Choix du moteur** (`crates/pa-daemon/src/worker.rs:390-422`) :
  - `"engine": "faux"` → `AgentSessionEngine` (`:393-418`) ;
  - **tout autre script → `ScriptedEngine::from_value`** (`:420-421`), et `agent_engine` reste `None` (`:388-389`).
- **Conséquence** : `run_turn_once` n'est appelé que par `run_model_turn` (`crates/pa-daemon/src/agent_engine/turn/model.rs:370`), lui-même appelé seulement par `run_turns` (`turn/run_loop.rs:66`). Ces trois fonctions sont des méthodes d'`AgentSessionEngine`, et **ce worker n'en construit pas**.

  Relevé : `git grep -n 'fn run_turn_once\|run_turn_once(\|fn run_model_turn\|run_model_turn(' origin/main -- crates/pa-daemon/src`, hors tests.

### 2.2 Du Kill à l'arrêt du tour

1. **Tâche du test** (`interactive_daemon_e2e.rs:5699-5737`) :
   - attend `isStreaming == true` (`:5713`) ;
   - envoie `DaemonCommand::Kill` avec `rest = {"rlmCloseReason": "shutdown"}` (puis `"replaced"`) ;
   - `request_ok` doit réussir (`:5732`).
2. **Superviseur** (`crates/pa-daemon/src/supervisor/routing.rs:519-537`) :
   - session de profondeur 0 → `plain_kill = no_marker || target_depth == 0` est vrai malgré le marqueur ;
   - tombstone (`:538-550`) ;
   - puis le kill est routé au worker en `RouteAdmission::SupervisorInternal` (`:652`).
3. **Worker** : `"kill" => self.handle_kill(payload)` (`crates/pa-daemon/src/worker/commands.rs:94`) → `handle_kill` (`:720-814`) :
   - `KillCloseReason::from_payload` lit `rlmCloseReason` → `Shutdown` (`:721`, `crates/pa-daemon/src/worker/env.rs:22-27`) ;
   - `agent_engine` est `None`, donc `mark_session_closed` n'est pas appelé (`:724-726`) ;
   - **drapeau d'annulation** : `core.abort_requested = true; core.turn_cancelled = true;` (`:778-784`) ;
   - `self.engine.abort_in_flight_turn()` (`:788`) : **sans effet** pour `ScriptedEngine`, qui garde la méthode par défaut (`crates/pa-daemon/src/engine.rs:111-114`, aucune surcharge dans `engine/scripted.rs`) ;
   - attente de la fin du tour (`:792`), puis `emit_session_closed(…, Shutdown)` (`:802`, résultat ignoré), puis `response_success` (`:813`).
4. **Arrêt du tour**, dans le thread du tour :
   - la sonde passée au moteur lit `core.abort_requested || core.retry_abort_requested` (`crates/pa-daemon/src/worker/turn.rs:1027-1035`, passée à `:1066`) ;
   - `ScriptedEngine::run_prompt` attend les 60 s du script par `abortable_sleep(delay, aborted)` (`crates/pa-daemon/src/engine/scripted.rs:280-281`), en tranches de 25 ms, et consulte la sonde à chaque tranche (`:780-791`) ;
   - sonde vraie → `emit(Done(Err("prompt cancelled")))` (`:274`, `:282`), rendu au TUI comme « turn failed: prompt cancelled ».
5. **Ligne de fermeture au TUI** : `DaemonClientEvent::SessionClosed` avec `reason == "shutdown"` → `error_row("The yusAgent daemon shut down while this window was attached. …")` (`crates/pa-tui/src/session_ui/apply.rs:236-265`). C'est la chaîne qu'attend la seconde barrière WaitRender (`interactive_daemon_e2e.rs:5781`).

**Réponse à la question 2** :

- **Oui**, le Kill lit un drapeau d'annulation. Mais c'est `core.abort_requested` du worker (`commands.rs:780`, lu par `turn.rs:1032`), pas le `cancel_requested` de `run_turn_once`.
- **Non**, il ne passe par aucun code modifié par `a56e9ac89`. Ce commit ne touche, hors tests, que `agent_engine/turn/model.rs` (commentaire, `:89-90`) et `agent_engine/turn/run_once.rs` (`:78`, `:90-95`, `:108`, `:259`, `:338-344`, `:404-413`), d'après `git show --stat a56e9ac89`. Ces deux fichiers ne s'exécutent que dans `AgentSessionEngine` (§2.1).

### 2.3 Autres changements présents dans le run qui a échoué

Le run 45595703754 porte sur `c2e8d1bf9` (branche `ya-32-identite`, d'après `gh run view 45595703754 --json headSha,headBranch`). Entre `1093d646c` (merge de la PR #84) et `c2e8d1bf9`, les fichiers de `crates/` touchés sont les suivants (`git diff-tree` par commit) :

| Commit | Fichiers (hors tests) | Sur le chemin du test ? |
|---|---|---|
| `535b51f45` (ticket 35) | `pa-core/src/session_engine/ya_resource_notice.rs` | Non : `ScriptedEngine` garde `ya_resource_notice` par défaut, qui rend `None` (`engine.rs:426-428`), appelé à `turn.rs:1041` |
| `a12826d03` (ticket 35) | `pa-daemon/src/agent_engine/session_engine_impl.rs` | Non : `AgentSessionEngine` |
| `a56e9ac89` (ticket 36) | `agent_engine/turn/model.rs`, `agent_engine/turn/run_once.rs` | Non : `AgentSessionEngine` |
| `37715c83c`, `62c4f5970` (ticket 32) | `pa-daemon/src/acp/ya_history.rs`, `acp/ya_turns.rs` | Non : pont ACP, le TUI n'y passe pas |

Les autres commits ne touchent que des tests de `pa-daemon` ou `pa-cli` (`ya_announce_path_e2e`, `ya_abort_window`, `abort.rs`, `rlm_children_delete_e2e`, `ya_acp_resume_e2e`, `ya_history_tests`). Aucun n'est dans `pa-tui`.

### 2.4 Ce que montre le journal de l'échec

Run 45595703754, job 139141296209 (`gh run view 45595703754 --repo YusufByn/yusagent --job 139141296209 --log-failed`) :

```
panicked at crates/pa-cli/tests/interactive_daemon_e2e.rs:5794:5:
shutdown turn had 7 frames without the TS error row; old_info=false; final=
  …
  turn failed: prompt cancelled
  timed out waiting for the headless render condition
  …
test result: FAILED. 41 passed; 1 failed; ... finished in 43.77s
```

Faits tirés de ce journal :

- **Le tour s'est arrêté** : « turn failed: prompt cancelled » est rendu, donc la première barrière WaitRender (`:5777`) a été franchie.
- **La seconde barrière (ligne d'erreur) a expiré** : « timed out waiting for the headless render condition » est la note écrite à l'expiration d'un WaitRender (`crates/pa-tui/src/interactive/run.rs:1000-1010`).
- **`old_info=false`** : aucune frame ne contient non plus « session closed (shutdown) ». L'événement `SessionClosed` n'a donc été rendu sous aucune forme dans les 7 frames.
- **Le Kill a reçu une réponse de succès** : `kill_task.await.expect(…)` (`:5792`) précède l'`assert!` de `:5794`, et la tâche échoue si `request_ok` échoue (`:5732`).
- **Non vérifié** : si cette réponse est celle de `handle_kill` (donc après `emit_session_closed`, `commands.rs:802`) ou une réponse du superviseur.

## 3. `f09c6538e` et le chemin WaitRender

### 3.1 Ce que touche le commit

`git show --stat f09c6538e` :

| Fichier | Nature | Lignes |
|---|---|---|
| `crates/pa-tui/src/interactive/run.rs` | **code du produit** | sur `main`, la branche `maybe_input` du `select!`, `:1555-1573` : condition `if headless_done` → `if ui_input_closed` (`:1559`), commentaire `:1556-1558`, et `else { ui_input_closed = true; }` après `:1572` |
| `crates/pa-cli/tests/interactive_daemon_e2e.rs` | test ajouté `tui_wait_idle_barrier_parks_while_the_turn_runs` | 47 lignes |
| `.changes/fix-headless-lost-wakeup.md` | note de version | 1 ligne |

- **Total** : 55 lignes ajoutées et 4 retirées.
- **`ui_input_closed` existe déjà sur `main`**, dans la même fonction `run_interactive_surface` (déclarée à `run.rs:432`, utilisée par la phase d'ouverture à `:458` et `:469`). Le commit n'ajoute pas de variable.
- **Le test ajouté** utilise `libc`, déjà en dev-dependency de `pa-cli`, ainsi que `run_headless_bounded` (`:254`), `spawn_supervisor` (`:345`) et `base_options` (`:2949`), qui existent sur `main`.

### 3.2 Le chemin WaitRender de ce test passe-t-il par la branche corrigée ?

**Oui, d'après la lecture du code.**

1. **Le pilote headless** (`crates/pa-tui/src/interactive/render.rs:220-308`), tâche lancée par `tokio::spawn` :
   - envoie chaque étape au canal sans attendre : `Submit` (`:223-227`), `WaitRender` (`:258-265`) ;
   - puis `HeadlessDone` (`:307`) ;
   - puis se termine. Il possède le seul `ui_tx` en mode headless : le seul clone est pour `Renderer::Terminal` (`run.rs:1076-1077`), et le `Renderer::Headless` n'en garde pas (`render.rs:309-314`). **Le canal se ferme donc dès que les quatre messages sont envoyés.**
2. **Dans la boucle** (`run.rs`), `WaitRender` est une barrière qui reste en tête de `pending` jusqu'à ce que la chaîne soit rendue ou que le délai expire (`:955-1014`). `HeadlessDone` attend derrière elle dans `pending`, et `headless_done` ne passe à `true` qu'au traitement de `HeadlessDone` (`:1178`).
3. **Pendant toute la durée des deux barrières WaitRender**, la branche `maybe_input` est donc dans l'état décrit par `f09c6538e` :
   - `headless_done == false` → `ui_rx.recv().await` sur un canal fermé, prêt à chaque tour (`:1559-1563`) ;
   - `None` n'est pas traité (`:1565`).
   Le commit décrit le cas WaitIdle, mais la branche ne dépend pas du type de barrière.
4. **Le runtime** : `#[tokio::test]` sans `flavor` (`interactive_daemon_e2e.rs:5669`) est un runtime `current_thread`. Le test d'upstream le dit pour son cas : « starving the daemon reader on this current-thread runtime ».

### 3.3 Application

Même worktree jetable à `adfc969d6` :

```
git cherry-pick --no-commit f09c6538e   # exit=0 ; Auto-merging run.rs et interactive_daemon_e2e.rs
git diff --cached --stat                # 3 files changed, 55 insertions(+), 4 deletions(-)
# hunks de run.rs : @@ -1553,10 +1553,11 @@ et @@ -1569,6 +1570,8 @@
git cherry-pick --no-commit 7ea89013e f09c6538e   # les deux ensemble : exit=0, 5 files, 123+/16-
```

Compilation non vérifiée (règle 8).

## 4. Hypothèses (non vérifiées)

### Test d'escalade

- **H1** : la course décrite par `7ea89013e`. Un thread du worker pas encore arrêté répond au kill.
  - Pour : le texte du symptôme est identique (§1.3).
  - Contre : rien de relevé.
  - Non lié au ticket 36 : aucun prompt, donc aucun tour (§1.3).

### Test du TUI

- **H2** : la boucle headless tourne sans se garer (§3.2) et retarde la lecture des événements du démon au-delà des 30 s de la seconde barrière.
  - Pour : le chemin passe par la branche corrigée ; upstream décrit le même effet (« the turn end reached the barrier minutes late ») sur une machine chargée.
  - Non expliqué : l'événement du tour annulé, lui, est arrivé à temps.
- **H3** : l'événement `SessionClosed` n'atteint pas ce client (perte ou course côté démon ou reconnexion), indépendamment de la boucle. Rien relevé pour ou contre.
- **H4** : un lien avec `a56e9ac89`.
  - Contre : aucun chemin d'exécution (§2.1 à §2.3).
  - Calcul sur les chiffres du ticket 32 (0 échec sur 76 avant, 1 sur 9 après) : si un seul échec tombe au hasard parmi 85 runs, la probabilité qu'il tombe dans les 9 derniers est 9/85 ≈ 0,106 (`python3 -c 'print(round(9/85,3))'`).

### Commits d'upstream non examinés

Ces commits sont postérieurs à notre base d'upstream `c83ac68b3` et touchent les mêmes zones (`git log c83ac68b3..upstream/main -- <fichiers>`) :

- **`run.rs`, `render.rs`** : `19194ccb8` (#4222, WaitIdle et message « Starting »), `4dd4ae97b` (#4003, état de reconnexion), `0b1445fdb` (#3988), `b8b5c66b0` (#3997) ;
- **fermeture de session** : `e3b7f7118` (#3994).

Ils ne sont pas lus ici et pourraient compter pour H3.

## 5. Plan : options

Aucune option ne touche la couche providers, le prompt système ni les définitions de tools : **aucun effet sur le préfixe** pour toutes.

### Test d'escalade

| Option | Contenu | Upstream | Produit | Préfixe |
|---|---|---|---|---|
| **E1** | Reprise de `7ea89013e` telle quelle (`git cherry-pick -x`) | 2 fichiers de test upstream, rendus identiques à upstream sur ces hunks (moins d'écart) | aucun | aucun |
| **E2** | Ne rien faire et attendre une synchro avec upstream | — | — | — |

Avec E1, une entrée est à ajouter à `fork-changes.md`, comme pour la reprise de `5778afa02` au ticket 36 (section « Abandon perdu avant l'enregistrement du run : reprise de `5778afa02` d'upstream »). E1 couvre aussi `an_owned_stop_kills_a_worker_that_missed_the_shutdown` (`registration_refusal_heal_e2e.rs`), qui n'a pas échoué chez nous à ma connaissance.

### Test du TUI

| Option | Contenu | Upstream | Produit | Préfixe |
|---|---|---|---|---|
| **T1** | Reprise de `f09c6538e` telle quelle | `pa-tui/src/interactive/run.rs` (code du produit, 2 hunks), 1 test e2e ajouté, 1 `.changes` | **oui** : la boucle du TUI se gare quand le canal d'entrée est fermé. En terminal, le renderer garde un `ui_tx` (`render.rs:209`), donc le canal ne se ferme pas en cours de route (lecture, non testé) | aucun |
| **T2** | Mesurer avant de choisir : lancer `tui_close_reason_rows_survive_late_turn_status` N fois sur `main`, puis N fois avec T1, en CI (pas de build local) | selon la branche de mesure (non commitée sur `main`) | aucun sur `main` | aucun |
| **T3** | Examiner d'abord les commits d'upstream du §4 (reconnexion, fermeture), pour H3 | — | — | — |
| **T4** | Ne rien faire | — | — | — |

- **T2** demande un workflow ou une boucle de répétition. Je n'ai pas vérifié s'il en existe un dans `.github/workflows/`. Une mesure discriminante entre H2 et H3 demanderait aussi l'heure d'arrivée de `SessionClosed` au client, que le test ne journalise pas.
- **T1 et E1 ensemble** s'appliquent proprement (§3.3).

## 6. Décisions de Yusuf (25/11/2026)

- **D1 : E1.** Reprise de `7ea89013e` par `git cherry-pick -x` :
  - sujet préfixé `YA-10: ticket 38 :`, `patch-id` identique ;
  - entrée dans `docs/fork-changes.md`, comme la reprise de `5778afa02` au ticket 36.

  **Pas de test vu rouge puis vert** : la course (un thread du worker pas encore arrêté répond au kill) ne se déclenche pas à la main. Le justificatif est le commit d'upstream et le journal de l'échec : run 45602379325, job 139164539614, `stop_escalation_e2e.rs:293`, kill répondu `"success":true` (§1.3).
- **D2 : `tui_close_reason_rows_survive_late_turn_status`** (`interactive_daemon_e2e.rs:5670`, assert en `:5794`) est ajouté à « Instabilité probable » dans `docs/fork-changes.md`. Justification :
  - chemin `ScriptedEngine`, hors des fichiers des tickets 32, 35 et 36 (§2) ;
  - un échec en 85 exécutions ;
  - symptôme : la frame de fermeture n'est jamais rendue (§2.4).

  Options renvoyées ou écartées :
  - **T1** (`f09c6538e`) et **T3** (#4003, #3988, #3994, #4222) : renvoyés au ticket des correctifs d'upstream depuis `c83ac68b3`, pas traités dans ce ticket ;
  - **T2** écarté : à ce taux d'échec, le nombre de runs nécessaire n'est pas raisonnable.
- **D3 : préfixe.** Le commit du constat est amendé en `YA-10: ticket 38 :`. Il était déjà poussé sous `YA-38:` : le push qui le remplace est un `--force-with-lease`, lancé par Yusuf.

## 7. Commandes utilisées

```
git show --stat 7ea89013e ; git show 7ea89013e
git show --stat f09c6538e ; git show f09c6538e -- crates/pa-tui/src/interactive/run.rs
git show a56e9ac89 -- '*.rs'
git worktree add --detach <tmp>/wt38 adfc969d6 ; git cherry-pick --no-commit … ; git reset --hard ; git worktree remove --force <tmp>/wt38
gh run view 45595703754 --repo YusufByn/yusagent --job 139141296209 --log-failed
gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/139164539614/logs
gh run view 45595703754 --repo YusufByn/yusagent --json headSha,headBranch,event,createdAt
git log --oneline c83ac68b3..upstream/main -- crates/pa-tui/src/interactive/run.rs crates/pa-tui/src/interactive/render.rs
```

## 8. Exécution

**D3** : le commit du constat est amendé en local (`git commit --amend`), et son sujet devient `YA-10: ticket 38 : constat et plan, deux tests upstream rouges depuis le ticket 36`.

**D1** : reprise de `7ea89013e`.

```
git cherry-pick -x 7ea89013e            # 2 files changed, 68 insertions(+), 12 deletions(-), sans conflit
git commit --amend -F <message>          # sujet préfixé « YA-10: ticket 38 : », auteur d'upstream gardé
git show 7ea89013e | git patch-id --stable
# 7bdff4daa8690aaa35ad68c05e328368c081e259 7ea89013e2c537f08df05057641c941093799ee6
git show HEAD | git patch-id --stable
# 7bdff4daa8690aaa35ad68c05e328368c081e259 <commit de la reprise>
git diff --stat HEAD upstream/main -- crates/pa-daemon/tests/stop_escalation_e2e.rs crates/pa-daemon/tests/registration_refusal_heal_e2e.rs
# (vide : les deux fichiers sont identiques à upstream/main, 6e4b69da2)
```

Rien n'a été compilé en local (règle 8) : la compilation et les tests passent par `ci.yml` sur la PR #89.

**D2** : entrée « Instabilité probable : `interactive_daemon_e2e`, raisons de fermeture (YA-10, ticket 38) » dans `docs/fork-changes.md`.

**`docs/fork-changes.md`** : entrée « Worker gelé par SIGSTOP dans les e2e d'arrêt : reprise de `7ea89013e` d'upstream (YA-10, ticket 38) ».
