# Ticket 13 : YA-09, attente complète de `bash()` comme Unharnessed (option A)

Suite du ticket 12 (« Note pour le ticket suivant »). Plan écrit en lecture seule le 16/11/2026 pendant l'A/B du cache v2, sur `main` à `5d8bb92b3`. Décisions de Yusuf du même jour reportées dans chaque section (D1 à D6). Rien n'est codé à ce stade.

## Décisions de Yusuf (16/11/2026)

1. **D1, valeur** : `"full"`.
2. **D2, défaut** : `"full"` par défaut directement, sans A/B (règle Unharnessed du 15/11, confirmée le 16/11). Garde-fou : les chauffes. La section « Mesure » est écartée.
3. **D3, prompt v1** : `"full"` refusé en prompt v1 (restreint au `"full"` explicite par D6). `bench/smoke/settings/prompt-v1.json` fixe `bashWaitSeconds` à 30 pour que le bras de contrôle reste l'ancien harness. Un test vérifie qu'il démarre.
4. **D4, TTL** : T1, TTL client à 15 min, listé dans `docs/fork-changes.md`.
5. **D5, watchdog** : note du watchdog adaptée dans ce ticket (l'extension passe par le démon).
6. **D6, refus en v1 limité au `"full"` explicite** (16/11, 15h12) : le refus de D3 ne vaut que si `"full"` est écrit explicitement dans les réglages. Si `"full"` vient du défaut et que la session est en prompt v1 (sessions créées avant le lot Unharnessed, sous-agents), elle prend 30 s. Raison : ne pas casser la reprise des anciennes sessions v1. D6 remplace l'ajout « le refus vaut aussi quand `"full"` vient du défaut ».

Chauffes `py-silence-001` et `py-silence-002` autorisées d'avance.

## Implémentation (16/11/2026, branche `ya-13-bash-full-wait`)

Relecture d'Unharnessed à `fe2cf2815` (clone jetable `/tmp/unharnessed-fe2cf2815-ya13`, rien exécuté) :
- `__await__` attend sans borne (`unharnessed-runtime/src/rlm/bash.py:975-1000`) : pas de fenêtre ni d'instantané.
- Leur prompt n'a pas de phrase sur la durée de l'`await` : `await handle -> BashResult`, le `timeout` par défaut (`crates/uh-core/src/prompts/layers/core.md:6-8`) et « Do not hold the turn with `sleep` or long blocking awaits » (`core.md:20`).

Ce qui est fait :
- **Réglage** : `BashWait` (`session_engine/ya_request.rs`), nombre ou `"full"`, même clé `bashWaitSeconds` dans les réglages et dans `yaRequest`. Défaut par `resolve_bash_wait` : `"full"` en v2, 30 s en v1 (D6). `check_bash_wait` refuse, avant le gel, une session qui tournerait en v1 avec `"full"`. Le défaut ne donne jamais cette paire : seul un `"full"` explicite (D3, D6) ou un en-tête écrit à la main y mène. Une session reprise garde sa valeur figée, et un `"full"` explicite ne refuse pas une session v1 déjà figée.
- **Kernel** : `_YIELD_AFTER_SECONDS = None` après le basculement des avis (`kernel/bootstrap/ya_bash_wait.rs`). Runtime Python intact.
- **Prompt** : en `"full"`, la ligne `await bash(cmd)` entière est remplacée (`prompts/ya_bash_wait.rs`, `FULL_WAIT_LINE`) : « `await bash(cmd)` waits until the command ends or its `timeout` kills it, and returns its `BashResult`. To keep working while a command runs, start it without `await` and collect the result on its `[bash-done]` notification. Never `await` a server: keep its handle, with `timeout=None` or `persistent=True` so the time limit does not kill it. »
- **Watchdog (D5)** : en `"full"`, la note dit « Start long shell work with bash() without await and collect it on its [bash-done] notification ». Elle est réécrite dans `KernelManagerExecutor` (`runtime_wiring.rs`) par `ya_bash_wait::watchdog_note`. `execution.rs` n'est pas modifié ; un test épingle sa phrase.
- **TTL (D4)** : `SESSION_WEBSOCKET_CACHE_TTL_MS` à 15 min (`websocket.rs:30-32`).
- **Banc** : `prompt-v1.json` à 30 s, tâche `py-silence-002`, `check_warmup.py` (ligne `full`, cellules complètes après 30 s, connexions, préfixe), README.

Écarts au plan :
1. **Fichiers upstream en plus de la liste** : `engine.rs` (appel du refus), `runtime_wiring.rs`, `provisioner.rs`, `system_prompt.rs` (type du champ du fork, accesseur, réécriture de la note), `pa-cli/src/prompt_command.rs` (résolution du défaut avec la version du prompt). Tous imposés par le changement de type ou par D5. `settings/load.rs` n'a pas changé : la clé y était déjà.
2. **D5 sans toucher `execution.rs`** : la note est réécrite après coup par le fork, plutôt que modifiée dans le fichier upstream.
3. **Texte de la ligne** : la proposition disait « start it with `persistent=True` or keep its handle ». En v2, un handle non persistant est tué au timeout de 600 s par défaut, d'où « keep its handle, with `timeout=None` or `persistent=True` ».
4. **Refus** : vérifié sur la requête effective de la session plutôt que sur le seul réglage. Même résultat pour les sessions neuves ; un en-tête écrit à la main en v1 avec `"full"` est refusé aussi.
5. **Règle 8** : un `cargo check` a été lancé une fois en local au début de l'implémentation, avant de revenir à la règle. La validation Rust se fait par `ci.yml`.

Tests : listés dans `docs/fork-changes.md` (ticket 13). Runtime en local : `cd prime-agent-runtime && uv run --python 3.11 python -m unittest discover -s test -p 'test_ya_bash_*.py'` : 24 tests OK. Banc : `python3 -I -m unittest discover -s bench/smoke/tests` : 48 tests OK.

## Constat de départ

- **Aujourd'hui** : dans une session qui reçoit les avis `[bash-done]`, `await bash(cmd)` rend la main au bout de `_YIELD_AFTER_SECONDS` (1 s chez upstream, `prime-agent-runtime/src/rlm/bash.py:69` et `:1078`). Il renvoie alors un instantané : `r.running` vaut True et la commande continue (`bash.py:1105-1106`, `:1109-1119`).
- **Notre défaut** : `bashWaitSeconds` vaut 30. Il est figé par session (`crates/pa-core/src/session_engine/ya_request.rs:190`), posé par une ligne de bootstrap (`crates/pa-core/src/kernel/bootstrap/ya_bash_wait.rs:23`) et annoncé par la ligne `await bash(cmd)` du prompt (`crates/pa-core/src/prompts/ya_bash_wait.rs:16-25`).
- **Chez Unharnessed** : `await handle` attend jusqu'au bout (`unharnessed-runtime/src/rlm/bash.py:975`, lu au commit `fe2cf2815`, `docs/tickets/11-ya09-unharnessed.md:64`). Il n'y a plus de clone local : la première étape relit leur `bash.py` et leur prompt à ce commit, dans un clone jetable, sans rien exécuter.
- **Le runtime sait déjà attendre sans borne** : avec `bound = None`, `_bounded_wait` fait un simple `await wait` (`bash.py:113-114`). C'est le chemin des sessions sans avis, sur lequel upstream a ses tests. Poser `_YIELD_AFTER_SECONDS = None` suffit, puisque `bound = _YIELD_AFTER_SECONDS if wired else None`. **Le runtime Python reste donc intact.**

## Réglage

- **Nouvelle valeur de `bashWaitSeconds`** : `"full"` (D1). Ce reste une variante dans la même branche (règle 5). Le type passe de `NonZeroU32` à une énumération : un nombre (comme aujourd'hui) ou `"full"` (`settings/types.rs:334`, `settings/load.rs:76`, `ya_request.rs:190-250`).
- **Défaut** (D2) : réglage absent = `"full"`. `DEFAULT_BASH_WAIT_SECONDS` (`ya_request.rs:190`) devient la valeur `"full"`. Un nombre explicite garde la fenêtre bornée.
- **En-tête et reprise** : la valeur est figée dans `yaRequest`, `"full"` compris. Une session reprise garde sa valeur, comme aujourd'hui : une session figée à 30 s reste à 30 s, un `yaRequest` sans le champ reste à 1 s (`LEGACY_BASH_WAIT_SECONDS`, `ya_request.rs:194`). À vérifier pendant l'implémentation : le cas d'une session sans `yaRequest`, qui prend les réglages courants (écart déjà accepté pour `reasoningSummary` le 15/11), en prompt v1 : elle prend 30 s si `"full"` vient du défaut, refus seulement si `"full"` est explicite (D6).
- **Kernel** : `ya_bash_wait.rs` émet `…._YIELD_AFTER_SECONDS = None` après le basculement des avis. Sans gestionnaire `bash.completed`, ou en `shellTool: "native"`, rien ne change.
- **Refus en prompt v1** (D3) : une session en `promptVersion` 1 avec `bashWaitSeconds` `"full"` **écrit explicitement dans les réglages** est refusée au démarrage (D6), sur le modèle du refus des sous-agents en v2 (`check_subagents`, `ya_request.rs:149-174`) : vérifié avant le gel des réglages, message qui nomme le réglage (`"bashWaitSeconds": 30`). Raison : le prompt v1 n'a pas de timeout par défaut (`ya_timeout.py:25`, `:40`), donc un `await` sur un serveur bloquerait sans fin. **Défaut en v1 (D6)** : si `"full"` vient du défaut (réglage absent), une session v1 prend 30 s au lieu d'être refusée : sessions créées avant le lot Unharnessed reprises sans `yaRequest`, sous-agents en v1. Raison : ne pas casser la reprise des anciennes sessions v1.

## Changements dans les requêtes et le préfixe

**Seules les sessions créées avec `"full"` voient leur prompt système changer. Avec D2, c'est toute session v2 neuve sans réglage.**

- **Ligne `await bash(cmd)`** (`system_prompt.rs:388-393`, partagée par v1 et v2 ; en pratique v2 seule, D3) : la phrase de la fenêtre et de l'instantané est remplacée. Proposition, à caler sur le texte d'Unharnessed : « `await bash(cmd)` waits until the command finishes or its `timeout` kills it, and returns the result. To keep working while a command runs, start it without `await` and collect it on the `[bash-done]` notification. Never `await` a server: start it with `persistent=True` or keep its handle. »
- **Lignes qui restent vraies et ne changent pas** :
  - `RULE_POLL` de la v2 (`ya_prompt_v2.rs:61`) ;
  - `DAEMONLESS_V2` (`:102`) : une commande sans `await` réveille toujours le modèle.
- **Changement de préfixe** : une rupture de cache par session neuve créée en `"full"`, stable ensuite dans la session. Les sessions existantes gardent leur préfixe (valeur figée). **Le préfixe par défaut change** (D2) : à signaler dans le résumé de la PR (règle 4).
- **Tools, paramètres et corps de requête** : inchangés.
- **Conversation** : moins d'instantanés et d'avis `[bash-done]`, des sorties de cellule complètes donc plus longues, et des silences sans requête plus longs pendant les cellules.

## Interactions

**`bashWaitSeconds`**
- `"full"` remplace la fenêtre et ne se combine avec aucune valeur numérique.
- Une session à 30 s, ou à 1 s en héritage (`LEGACY`, `ya_request.rs:194`), garde son comportement.

**`bashNoticeBatchSeconds` (défaut 120, `ya_bash_notice_batch.rs:36`)**
- Il ne s'applique plus qu'aux commandes lancées sans `await`, ou à un handle attendu ailleurs que dans la cellule qui l'a créé.
- Une commande attendue jusqu'au bout par sa cellule ne produit pas d'avis (`_awaited_by_creating_cell`, `bash.py:1096-1101`, retrait à `:863`).
- Pas de changement de code. À vérifier par test : aucun avis après une attente complète.

**Timeout de 600 s (prompt v2, `kernel/bootstrap/ya_bash_timeout.rs:11`)**
- Il borne l'attente : une cellule bloque au plus environ 605 s (SIGTERM à 600 s, puis SIGKILL 5 s plus tard, `ya_timeout.py:5`).
- En prompt v1, il n'y a pas de timeout par défaut (`ya_timeout.py:25`, `:40`) : d'où le refus de D3.
- `persistent=True` n'a pas de limite. Le prompt dit déjà de ne pas l'attendre.

**Annulation (`session/cancel` de YA-10)**
- Les annulations tomberont plus souvent pendant un `await`. Le chemin est celui d'upstream : SIGTERM au groupe, 0,5 s de grâce, puis SIGKILL (`bash.py:44-47`).
- Mesuré sous 1 s au runtime seul pour la fenêtre de 30 s (`bench/smoke/ab/ya09-bash-plan.md`, « `session/cancel` »). Refaire la même mesure sans borne.

**Watchdog de cellule (D5)**
- Sessions avec démon seulement, 60 s par défaut (`settings/types.rs:263`, `settings/manager.rs:650-658`).
- Au-delà de 60 s, il ajoute une note qui dit « bash() … auto-backgrounds past its yield window » (`kernel/manager/execution.rs:301-314`). C'est faux en `"full"`.
- Le watchdog n'interrompt pas la cellule : il ajoute la note sur stderr et la cellule garde son statut (`execution.rs:295-300`). L'attente complète n'est donc pas coupée dans l'extension, seule la note est fausse.
- Traité dans ce ticket, puisque l'extension passe par le démon : en `"full"`, la note dit de lancer la commande longue sans `await` et de la relever sur l'avis `[bash-done]`. Modification minimale du fichier upstream, ou texte fourni par un module du fork, à lister dans `fork-changes.md`.

## TTL client de 5 min (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:30`)

- **Ce qui se passe aujourd'hui** :
  - à chaque fin de requête, une expiration est programmée à 5 min d'inactivité (`session.rs:163-190`, appelée à `websocket.rs:569`) ;
  - une cellule de plus de 300 s ferme donc la connexion côté client ;
  - l'appel suivant part sur un WebSocket neuf, en corps complet et sans `previous_response_id` ;
  - grâce au ticket 12, il n'y a ni 1011 ni repli SSE.
- **En `"full"` avec le timeout de 600 s**, ce cas devient courant pour les commandes de 5 à 10 min.
- **Retenu (D4) : T1**, constante à 15 min, au-dessus de 605 s plus la latence du modèle. Une ligne dans un fichier upstream de la couche providers, listée dans `fork-changes.md` (règles 2 et 3). Sans effet sur les requêtes.
- Écartées : T2 (TTL calculé dans un module du fork à partir du timeout de `bash()`) et T3 (ne rien changer et mesurer).
- **Inconnu** : l'âge maximal d'une connexion côté serveur. Jusqu'ici, 178,8 s de silence ont tenu (ticket 12, chauffe 2). La chauffe `py-silence-002` le vérifie au-delà de 5 min.

## Garde-fou du banc et commandes de plus de 10 min

- **Comment il compte** : le reverse proxy compte les octets relayés dans les deux sens, trames de contrôle comprises (`bench/smoke/proxies.py:451`, `:465`). Les pings du serveur et nos pongs le remettent à zéro tant que la connexion vit.
- **Inconnu à vérifier** : l'intervalle des pings du serveur. Les fermetures vers 75 s suggèrent des pings plus serrés que ça (hypothèse). Le proxy ne journalise pas les trames de contrôle.
- **Avec T1** :
  - une cellule de 605 s au plus ne déclenche pas le garde-fou de 300 s si les pings passent ;
  - à vérifier par `py-silence-002` ;
  - si le garde-fou se déclenche quand même, l'unité sort en `stalled` et elle est écartée.
- **Commandes de plus de 10 min** (`timeout` explicite au-delà de 600, ou `None`) : avec T1, la connexion se ferme au TTL (15 min). Plus aucun octet ne passe ensuite, et le garde-fou arrête la tâche 300 s plus tard, soit vers 20 min de silence (estimation).
- **Règle** : aucune tâche du banc n'a de telles commandes (la plus longue de YA-14 dure environ 2 min, `bench/smoke/ab/ya09-bash-durees.py`, à revérifier). Le défaut de 300 s reste. Pour un run qui en a besoin : `--idle-timeout` au moins égal à la plus longue commande plus 120 s, ou `0`, noté dans le plan du run. Une ligne dans `bench/smoke/README.md`, « Garde-fou d'inactivité » (`README.md:68`).
- **Risque noté sans changement** : pendant une requête, des pings qui continuent pourraient masquer un flux bloqué. Ce n'est pas nouveau : le flux était déjà lu pendant les requêtes avant le ticket 12. Le blocage du 06/11 a d'ailleurs été détecté.

## Découpage et fichiers

Une PR, avec ces modules :
- **Fork** :
  - `session_engine/ya_request.rs` (valeur `"full"`, défaut, refus en v1 du `"full"` explicite, 30 s en v1 par défaut) ;
  - `kernel/bootstrap/ya_bash_wait.rs` (ligne `None`) ;
  - `prompts/ya_bash_wait.rs` (ligne du prompt).
- **Upstream** (modifications minimales, listées dans `fork-changes.md`) :
  - `settings/types.rs` et `settings/load.rs` (type du champ) ;
  - `websocket.rs:30` (T1) ;
  - `kernel/manager/execution.rs` (D5).
- **Banc** :
  - `settings/prompt-v1.json` : `{"promptVersion": 1, "bashWaitSeconds": 30}` (D3) ;
  - `settings/bash-wait-30.json` : `{"bashWaitSeconds": 30}`, l'ancien défaut v2, pour comparer ou rejouer (conséquence de D2 : `default.json` vaut désormais `"full"`, et le `bash-wait-full.json` du premier jet devient inutile) ;
  - tâche `py-silence-002` (tag `chauffe`, `sleep 400`, attente dans la cellule) ;
  - `tools/check_warmup.py` (cellules rendues complètes après plus de 30 s, durée de la plus longue) ;
  - README (garde-fou, réglages).
- **Docs** : `fork-changes.md`, ce ticket, journal du CDC, `CHANGELOG` par fragment `.changes/`.

## Tests

- **Rust** :
  - lecture et écriture de `"full"` dans les réglages et dans l'en-tête, et reprise (30 s figé reste 30 s, sans champ reste 1 s) ;
  - défaut `"full"` ;
  - ligne de bootstrap ;
  - ligne du prompt (un seul remplacement, reste octet pour octet) ;
  - `prefix_stability.rs` : une session `"full"` stable ;
  - refus d'un `"full"` explicite en prompt v1, message qui nomme `bashWaitSeconds` (D6) ;
  - reprise d'une session v1 sans `yaRequest`, réglage absent : pas de refus, 30 s (D6) ;
  - sous-agent en prompt v1, réglage absent : pas de refus, 30 s (D6) ;
  - démarrage avec les réglages de `bench/smoke/settings/prompt-v1.json` (D3) : pas de refus, 30 s figé ;
  - TTL de 15 min ;
  - note du watchdog en `"full"` (D5).
- **Runtime**, dans `test_ya_bash_wait.py` (fichier du fork) :
  - avec les avis branchés et `_YIELD_AFTER_SECONDS = None`, un `sleep 3` attendu rend un résultat complet, sans avis ;
  - l'annulation reste sous 1 s, y compris avec `trap '' TERM`.
- **E2E en `-p`, provider factice, vrai kernel** : une cellule qui attend 3 s rend `exit_code` 0, sans réveil `[bash-done]`.
- **Vérification réelle (autorisée d'avance, pas une mesure)** :
  - `py-silence-001` en `"full"` (défaut) : une cellule de 180 s, l'appel suivant en suite de conversation, 0 « 1011 » ;
  - `py-silence-002` : 400 s, au-delà de l'ancien TTL. Avec T1, même connexion et pas de `stalled` avec le garde-fou de 300 s, ce qui prouve que les pings passent ;
  - `compare_bodies.py` : seule la ligne `await bash(cmd)` diffère dans `instructions`.

## Mesure : écartée (D2)

Le premier jet prévoyait un A/B entrelacé `"full"` contre la fenêtre de 30 s (tours de la session principale, coût à cache théorique, résolution). Écarté par D2 : `"full"` devient le défaut sans A/B, comme les reprises d'Unharnessed du ticket 11. Garde-fou : les chauffes ci-dessus. `bench/smoke/settings/bash-wait-30.json` permet un A/B plus tard si un constat le demande.

## Points ouverts

- **Baselines** : `bench/smoke/baseline/gpt-6-luna.json` mesure l'ancien défaut. Avec D2, le défaut change encore : les alertes de `compare.py` qui s'y comparent mêlent deux configurations (point déjà ouvert le 15/11, passation du 15/11).
- **Démon (YA-10)** : avis `[bash-done]` groupés dans le démon, toujours à faire (critère de YA-10). Sans effet sur une attente complète, qui ne produit pas d'avis.
