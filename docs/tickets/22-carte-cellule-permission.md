# Ticket 22 : YA-11, carte de la cellule pendant une demande de permission (constat et plan)

Demande de Yusuf du 20/11/2026. Branche `ya-22-carte-permission`, depuis `main` à `f2ba28fc4` (merge de la PR #69, build macOS 44788981604 en succès). **Constat et plan seulement** : aucun code du produit modifié, rien compilé, aucun appel de modèle. Pas de code avant les décisions de Yusuf.

Défaut vu sur la PR #69 (ticket 20, PR 3, points ouverts) : en mode manuel, pendant qu'une demande attend, la carte de la cellule dans la transcription du TUI affiche `✓ python · …` et le chargeur affiche « Executing » alors que l'appel n'est pas autorisé. Le panneau, lui, dit la vérité. À corriger avant les testeurs.

## Décisions de Yusuf (20/11/2026)

- **K1** : option A (la carte reste « non lancée » tant que la demande attend).
- **K2** : libellé du chargeur « Awaiting permission ».
- **K3** : durée comptée depuis l'accord.
- **K4** : vérifier dans le worker si `tool_execution_start` arrive toujours avant la demande ; si oui, rien dans `apply.rs` ; sinon, garder l'appel. Le test headless couvre les deux ordres dans tous les cas.
  - **Constaté : l'ordre n'est pas garanti.** Le moteur pousse ses événements dans une file (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:81`, `tx.send`), vidée par la boucle du tour (`run_once.rs:399-416`) qui les publie depuis la tâche du tour (`crates/pa-daemon/src/worker/turn.rs:433`, numérotation `turn.rs:994-1010`). La demande est publiée directement par le hook (`crates/pa-daemon/src/ya_permissions.rs:196-202`, `emit` posé `:284-289` sur `crate::user_bash::emit_session_event_frame`, `crates/pa-daemon/src/user_bash.rs:917-944`) : elle peut doubler le `tool_execution_start` encore dans la file. La retenue de la PR 2 est **dans le pont ACP**, pas dans le worker : le pont garde la demande jusqu'à la publication du `tool_call` (`crates/pa-daemon/src/acp/ya_permissions.rs:17-19`, `:168-197`). Le TUI reçoit les événements du worker sans ce pont : **l'appel dans `apply.rs` est gardé.**
- **K5** : ticket à part, rattaché à YA-10 ; hors périmètre ici (section 3.3).
- **Fenêtre de calme** (ticket 20, T3) : passer à **1 s**, dans cette PR (`crates/pa-tui/src/ya_permission_panel.rs`, fichier du fork).

## 1. Constat

### 1.1 D'où vient l'état de la carte

- **Ordre côté moteur** : `ToolExecutionStart` est émis **avant** la préparation de l'appel, donc avant le hook qui pose la question (`crates/pa-agent/src/agent_loop/tools.rs:97-110`, puis `prepare_tool_call` `:104`). La demande part ensuite du hook (`crates/pa-daemon/src/ya_permissions.rs:196-202`).
- **Côté TUI**, l'arm `TurnUpdate::ToolExecutionStart` (`crates/pa-tui/src/session_ui/apply.rs:398-420`) :
  - appelle `apply_tool_execution_start`, qui met `card.started = true` et `card.started_at = now` (`crates/pa-tui/src/snapshot/tool_fold.rs:100-102`, ou une carte neuve `:113-120`) ;
  - met l'activité du chargeur à « Executing » (`apply.rs:418`) et compte un minuteur vivant (`apply.rs:419`, télémétrie seulement, `apply.rs:914-921`).
- **Rendu de la carte `python`** (`crates/pa-tui/src/tool_card/ipython.rs`) : `CardStatus::of` rend `Done` dès que `card.started` est vrai, sans résultat et hors résultat partiel (`ipython.rs:81-83`) ; `Done` s'affiche `✓` (`ipython.rs:254`). Une carte lancée passe à `Running` (spinner) seulement au premier résultat partiel du kernel (`ipython.rs:84-85`). Pendant une demande, aucun partiel n'arrive : le `✓` reste. Le minuteur de la ligne compte depuis `started_at` (`ipython.rs:294-303`), donc il compte l'attente.
- **Autres cartes** (`apply_patch`, `exec_command`/`write_stdin` en variante bash natif) : rendu générique (`crates/pa-tui/src/tool_card/mod.rs:129-134`). L'en-tête de panneau vaut « running » dès `card.started` (`mod.rs:160-161`) et la ligne « Elapsed » tourne (`crates/pa-tui/src/tool_card/generic.rs:76-78`). Même défaut, sous une autre forme.
- **En mode « auto »**, la même règle d'`ipython.rs:81` montre un `✓` entre `tool_execution_start` et le premier partiel du kernel. C'est le comportement d'upstream, bref en pratique ; **non traité ici**.

### 1.2 États et événements qui existent déjà

- **`pa-types`** (`crates/pa-types/src/daemon/ya_permissions.rs`) : `ya_permission_request` (`requestId`, `toolCallId`, `toolName`, `args`), `ya_permission_resolved` (`requestId`, `toolCallId`, `outcome` : `allow`, `deny`, `cancelled`), `ya_permission_mode`.
- **Worker** : `ya_permission_resolved` part **toujours**, quel que soit le répondant, y compris pour la réponse du TUI lui-même, par le `Drop` de `Waiting` (`crates/pa-daemon/src/ya_permissions.rs:251-262`). Une demande retirée (tour annulé, client parti) finit aussi en `cancelled`.
- **TUI** (`crates/pa-tui/src/session_ui/ya_permissions.rs`) : la demande ouvre le panneau (`:40-53`) ; `ya_permission_resolved` ne sert qu'à fermer un panneau encore ouvert (`:55-71`) : la réponse locale ferme le panneau avant que l'événement arrive (`:110`), qui est alors ignoré. Le `toolCallId` est gardé dans chaque demande du panneau (`crates/pa-tui/src/ya_permission_panel.rs:39-48`) mais **rien ne relie la demande à la carte**.
- **Carte** (`crates/pa-tui/src/tool_card/mod.rs:22-40`) : aucun état « en attente » ; seulement `started`, `started_at`, `ended_at`, `result`, `result_partial`, `aborted`. L'état « non lancé » existe : `Queued`, `◇` gris pour `python` (`ipython.rs:86-88`, `:259`), « queued » pour le rendu générique (`mod.rs:160-164`, `:181`), sans minuteur.
- **Après un refus** : le résultat `[permission-denied] The user declined this cell; nothing was executed.` arrive par `tool_execution_end` en erreur ; la carte passe à `✗` rouge avec ce texte (`ipython.rs:65-67`, `:252`). Le tour s'arrête (D7). Ce rendu dit déjà la vérité.
- **Ordre des deux événements** : `tool_execution_start` part par le chemin des événements du tour (`crates/pa-daemon/src/worker/turn.rs:994-1010`), la demande par `emit_session_event_frame` (`crates/pa-daemon/src/user_bash.rs:917-944`). Les deux prennent un numéro de séquence sous le même verrou, mais **je n'ai pas vérifié** que le premier est toujours numéroté avant le second. Le plan tient les deux ordres.

### 1.3 Ce qui part au modèle

Rien. `pa-tui` ne construit aucune requête : ce qui part au modèle ne dépend que du moteur du worker, commun à tous les clients (test T9 `manual_and_auto_send_the_same_requests`, ticket 20). Le correctif ne touche ni le prompt système, ni les tools, ni le texte du résultat d'un refus. **Préfixe inchangé** ; contrôle par capture contre `main` après le build (section 4).

## 2. Ce que la carte doit afficher (proposé)

| Moment | Carte | Chargeur |
|---|---|---|
| Demande ouverte | « non lancée » : `◇` gris + aperçu du code (`python`), « queued » (générique) ; **pas de minuteur** | « Awaiting permission » au lieu d'« Executing » |
| Après accord (`ya_permission_resolved`, `allow`) | lancée **au moment de l'accord** : `started_at` = accord, donc la durée n'inclut pas l'attente ; puis chemin normal (spinner, `✓` + durée) | « Executing » |
| Après refus (`deny`) ou retrait (`cancelled`) | reste « non lancée » jusqu'au `tool_execution_end` en erreur : `✗` rouge + `[permission-denied] …` (rendu existant), **sans durée** | chemin existant (fin du tour, D7) |
| Mode « auto » | inchangé (aucune demande) | inchangé |

La carte suit l'issue donnée par le worker (`ya_permission_resolved`), pas la touche locale : un accord donné par un autre client (ACP) relance aussi la carte.

## 3. Plan

### 3.1 Option A (recommandée) : la carte reste « non lancée » tant que la demande attend

Aucun nouvel état dans la carte : le module du fork remet la carte dans l'état `Queued` existant.

- `crates/pa-tui/src/session_ui/ya_permissions.rs` (fork) :
  - à `ya_permission_request` : trouver la carte du `toolCallId`, `started = false`, `started_at = None` (avec `prepare_entry_mutation` / `mark_entry_stale`, `crates/pa-tui/src/view.rs:611`, `:621`, comme `tool_fold.rs:98-110`), activité du chargeur « Awaiting permission » ;
  - à `ya_permission_resolved` (traité **toujours**, plus seulement quand le panneau était ouvert) : `allow` → `apply_tool_execution_start` (`started_at` = maintenant), activité « Executing » ; `deny` ou `cancelled` → rien sur la carte, `tool_execution_end` la règle.
- `crates/pa-tui/src/ya_permission_panel.rs` (fork) : dire si une demande ouverte porte un `toolCallId` donné.
- `crates/pa-tui/src/session_ui/apply.rs` (**upstream**, déjà touché par la PR 3) : un appel au module du fork à la fin de l'arm `ToolExecutionStart` (`apply.rs:398-420`), qui remet la carte « non lancée » si une demande ouverte porte son `toolCallId`. Raison pour `fork-changes.md` : « la carte d'un appel en attente de permission reste non lancée même si `tool_execution_start` arrive après la demande ». Si l'ordre de la section 1.2 est prouvé, cet appel n'est pas nécessaire.
- L'activité du chargeur est posée par `set_working_activity`, privée à `apply.rs` (`apply.rs:943-948`) : le module du fork écrit `view.working.activity` lui-même plutôt que d'ouvrir cette fonction (à confirmer à l'implémentation).

Coût en upstream : **un fichier, un appel**. Toutes les cartes (`python`, `apply_patch`, `exec_command`) sont couvertes, puisqu'on agit sur l'état et pas sur le rendu.

Limite : la carte ne dit pas « en attente de permission » elle-même, seulement « non lancée » ; le panneau et le chargeur le disent.

### 3.2 Option B : un état explicite sur la carte

Un champ « en attente de permission » dans `ToolCallCard`, un statut et un libellé dédiés (`◇` couleur d'avertissement + « awaiting permission » sur la ligne de la carte ; « denied » après un refus).

Fichiers **upstream** en plus de l'option A : `crates/pa-tui/src/tool_card/mod.rs` (champ et en-tête générique), `crates/pa-tui/src/tool_card/ipython.rs` (statut et marqueur), `crates/pa-tui/src/tool_card/generic.rs` (ligne « Elapsed »). C'est le code de rendu le plus remanié par upstream : risque de conflits aux `cherry-pick` (règle 2). Je ne la recommande pas pour la première version.

### 3.3 Hors périmètre (constaté, non traité)

- Le `✓` bref en mode « auto » avant le premier partiel (`ipython.rs:81`), comportement d'upstream.
- ACP : la cellule y est annoncée `in_progress` avant la décision (`crates/pa-daemon/src/acp/wire_events.rs:156`, ticket 20 section 1.3) ; équivalent ACP du même défaut, pour YA-10.
- **K5, ticket à part (YA-10)** : un TUI qui s'attache pendant qu'une demande attend : le worker ne renvoie pas la demande ouverte à l'`attach` (`crates/pa-daemon/src/ya_permissions.rs:107-115` enregistre le répondant sans rejouer `pending`). **Non vérifié** à l'écran : ni panneau, ni carte juste, probablement. À juger à part.

## 4. Tests et contrôle (proposé)

### 4.1 Test qui attrape le défaut

**Test headless** `crates/pa-tui/tests/ya_permission_card_headless.rs` (nouveau, fork), sur le modèle de `crates/pa-tui/tests/tool_card_live_timer_headless.rs` (démon simulé, `run_interactive` en `HeadlessPlan`, lecture de l'écran rendu) ; aucun kernel, aucun modèle. Trois cas :

1. **Attente** : `message_end` avec un appel `python_repl`, `tool_execution_start`, puis `ya_permission_request` du même `toolCallId`, puis silence. Attendu : la ligne de la carte ne contient pas `✓` ; le chargeur ne contient pas « Executing » et contient « Awaiting permission » ; pas de minuteur sur la carte. **Échoue sans le correctif** : `✓ python · …` et « Executing » (vus à l'écran de la démo sur `47423fffc`).
2. **Ordre inverse** : `ya_permission_request` avant `tool_execution_start`. Mêmes attentes.
3. **Accord puis refus** : `ya_permission_resolved` `allow`, partiel, `tool_execution_end` → `✓` ; second appel, `deny`, `tool_execution_end` en erreur `[permission-denied] …` → `✗`, jamais `✓`, pas de durée.

Séquence : commit du test seul poussé d'abord, run `ci.yml` **rouge** sur ce test (comme T8), puis commit du correctif, run **vert**. Les deux runs notés dans le ticket.

**Bout en bout** : dans `crates/pa-cli/tests/ya_permissions_tui_pty_e2e.rs` (fork, PR 3), pendant `WAITING`, vérifier que l'écran ne contient ni `✓ python` ni « Executing » ; après `y`, `✓ python` ; après `n`, `✗ python`. Ce test passe par le vrai worker : il tranche aussi l'ordre réel des deux événements.

### 4.2 Script de contrôle (règle 13)

Lancés par Claude Code sur le binaire de la branche (build `ya-build-macos-arm64.yml`), sorties jointes au rendu :

1. `python3 -I bench/tools/ya_permissions_tui_demo.py --binary <dossier>/yusagent --pilote`, étendue pour lire l'écran pendant l'attente (pas de `✓ python`, pas d'« Executing », « Awaiting permission » présent), après l'accord (`✓ python`) et après le refus (`✗ python`) ; code de sortie 1 si l'une échoue, en plus des vérifications existantes.
2. Capture contre `main` (`f2ba28fc4`, run 44788981604), préfixe :
   ```sh
   DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=<valeur des captures précédentes> \
     python3 bench/tools/capture/run.py --binary <dossier>/yusagent
   python3 bench/smoke/tools/compare_capture_prompts.py <capture main> <capture branche>
   ```
   Attendu : **TOUT IDENTIQUE**, `find ~/.yusagent ~/.prime/agent -newer` vide.

## 5. À trancher par Yusuf

- **K1** : option A (carte « non lancée », un appel dans un fichier upstream) ou option B (état explicite sur la carte, trois fichiers upstream de rendu en plus). Recommandation : **A**.
- **K2** : libellé du chargeur pendant l'attente : « Awaiting permission » (proposé).
- **K3** : après un accord, durée comptée depuis l'accord (proposé) ou depuis `tool_execution_start` (attente incluse).
- **K4** : l'appel dans `apply.rs` pour l'ordre inverse : le garder d'office (proposé, l'ordre n'est pas prouvé) ou attendre que le test pty montre l'ordre réel.
- **K5** : le TUI attaché pendant une demande ouverte (section 3.3) : ticket à part ou dans celui-ci.
