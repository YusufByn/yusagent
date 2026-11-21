# Ticket 25 : début et fin des tours dans le pont ACP (tours lancés par l'agent)

Demande de Yusuf du 21/11/2026, à la suite de la PR 3 du ticket 24 (YA-10). **Constat et plan seulement** : aucun code écrit, rien compilé, aucun appel de modèle. Pas de code avant les décisions de Yusuf (section 4).

Périmètre :
- un tour que l'agent lance seul (avis `[bash-done]` d'un `bash()` non lu, et les autres réveils du worker) doit avoir un début et une fin signalés par le pont ;
- le pont signale aussi le début de chaque tour, de prompt ou non, à la place de la déduction faite par l'extension sur le `promptTurnId` du `_meta` (`extensions/vscode/src/ya/meta.ts`, PR I) ;
- le cas e2e « tour lancé par l'agent » sort de l'étape 6 de la PR 3 du ticket 24 et passe ici.

## 1. Constat

### 1.1 Ce que le binaire envoie (capture du 21/11/2026)

- **Binaire** : `main` `1b96bb645` (run 44893185253). **Capture** : `bench/tools/capture/acp_frames.py` (versé au banc avec la PR 3 du ticket 24), session A, scénario `display` en « auto », 40 s d'attente après la réponse du prompt.
- Le tour de prompt finit par `responseBoundary` (`outcome: result`), l'enveloppe de quiescence, `terminalQuiescence`, puis la réponse `end_turn`.
- **Le tour de réveil** (avis `[bash-done]` du `bash()` non lu ; 7ᵉ requête reçue par le serveur de capture) n'envoie **qu'un `agent_message_chunk`, avec `promptTurnId: 0`**. Ni début, ni `responseBoundary`, ni `terminalQuiescence`, même 40 s après.

### 1.2 Pourquoi : le pont, hérité d'upstream

- **Les lignes concernées sont upstream**, pas du fork. `git blame -L 1585,1625 crates/pa-daemon/src/acp/daemon.rs` : commits upstream « Port Prime Agent to Rust (#2524) », « acp: single daemon path — session semantics, e2e migration, in-process transport deletion, event parity (#3264) », « acp daemon session runs one prompt turn at a time with a per-turn cancel (#3256) ».
- **La fin d'un tour n'est publiée que dans le chemin d'un `session/prompt`** : `responseBoundary` et quiescence (`crates/pa-daemon/src/acp/daemon.rs:1592-1620`), frontière d'erreur (`:1724-1734`).
- **L'identifiant du tour** est alloué à l'admission d'un prompt (`begin_prompt`, `crates/pa-daemon/src/acp/producer.rs:65-70`), puis remis à `0` à sa fin (`finish_prompt`, `:79-84`). Chaque événement du worker est publié avec le tour actif (`daemon.rs:374`, `active_prompt_turn`), donc `0` hors prompt.
- **Ce que le pont reçoit déjà** : le flux d'événements du worker, dont `agent_end` (`daemon.rs:397-406`, pour rafraîchir la configuration). Le worker émet `agent_start` et `agent_end` pour chaque tour (`crates/pa-core/src/session_engine/session_events.rs:124`). **Non vérifié** : que le pont reçoive bien `agent_start` et `agent_end` pour un tour de réveil, et dans quel ordre par rapport aux mises à jour du tour. C'est la première chose à mesurer.
- **Upstream ne l'a pas corrigé** depuis notre base. `upstream/main` à `783acb5d7` (récupéré le 21/11/2026), point de fork `c83ac68b3`. `git log c83ac68b3..upstream/main -- crates/pa-daemon/src/acp/` : 4 commits, aucun ne publie de frontière pour ces tours.
  - `ad1613303` (#3866) ajoute `outstandingBackgroundJobs` à la quiescence d'un tour de prompt : le nombre de `bash()` encore en cours et d'avis `[bash-done]` en attente. Cela annonce qu'un réveil peut venir, sans le borner.
  - `852e776b9` (#3845, `session/list`, `session/load`) et `bede0c0ed` (#4118, steering) n'y touchent pas.
  - Dans `upstream/main`, les seules `ResponseBoundary` de `daemon.rs` sont toujours dans le chemin du prompt (`:1862`, `:1993`).

### 1.3 Comment Unharnessed le fait (`157dcdc1d`)

Son mode ACP tourne **dans le processus de l'agent, sans démon**. Un réveil y est un tour de la couche ACP elle-même :
- `start_wake_turn` (`crates/uh-daemon/src/acp/prompt.rs:217-245`) démarre le tour si la session est ouverte et libre. Il alloue un identifiant de tour comme pour un prompt (`begin_prompt`, `:231`) et lance `run_prompt_turn(TurnStart::Wake(…))` (`:234-242`) ;
- `run_prompt_turn` publie d'abord `turnStart` (`origin: "wake"`, `wake: <genre>`, `:332-357` ; `origin: "prompt"` pour un prompt) ;
- `settle_turn` (`:857`) publie la `responseBoundary` (`:891`) puis l'enveloppe de fin (`:925`), comme pour un prompt, mais sans réponse JSON-RPC (`id: None`) ;
- l'extension finit un tour de l'agent sur cette `responseBoundary` (`vscode/src/chat.ts`, `info`, repris chez nous). `turnEnd` ne sert qu'au rejeu d'une conversation chargée (`crates/uh-daemon/src/acp/history.rs:449`).
- Différence avec nous : chez lui, l'avis arrive à la couche ACP (`Wake::BashDone`, `admit_wake`, `prompt.rs:250-258`), qui lance le tour. Chez nous, c'est le worker du démon qui lance le tour de réveil (file `custom_message`, `crates/pa-daemon/src/worker/queue.rs:347-352`) ; le pont ne fait que relayer ses événements.

### 1.4 Effet dans l'extension (mesuré avec l'agent factice aligné sur ces trames)

Tests exploratoires jetables, retirés après la mesure : l'agent factice rejoue le chunk `promptTurnId: 0` après la réponse du prompt, sans rien d'autre.

- **Le panneau reste occupé** : `busy` vrai 2 s après le réveil ; un message envoyé reste en file (`session/prompt` reçus 1, en file 1).
- **Stop le débloque** : le tour de réveil passe « cancelled », `session/cancel` est envoyé, la file n'est pas en pause, et le message suivant part (`session/prompt` reçus 2, en file 0).
  - **Limites** : le tour, fini en réalité, s'affiche « Stopped ». Sur le vrai pont, `session/cancel` hors prompt mène à `AbortAndClearQueue` dans le worker (`daemon.rs:886-900`, `stop_session_work`), ce qui peut vider d'autres travaux en file, par exemple un autre avis `[bash-done]`. **Non mesuré sur le binaire.**
- **Deux réveils** partageraient l'identifiant `0` : le second irait dans le tour du premier (`known(0)` dans `meta.ts`).
- **Début de tour déduit** (`meta.ts`) : premier événement de contenu d'un `promptTurnId` inconnu. Un `config_option_update` porte aussi un `promptTurnId` (défaut corrigé à la PR I). La déduction est fragile ; un signal explicite du pont la remplace.

### 1.5 Autres tours que le worker lance seul (à couvrir de la même façon)

À lister et mesurer au début du ticket : réveil `[bash-done]` ; `rlm-heartbeat` (réveil planifié, skill montré avec le démon) ; continuation d'un objectif (`/goal`) ; reprise après une limite d'usage ; messages entre agents (`agent_message`). Chez Unharnessed : `Wake` a les genres `bash`, `quota`, `goal` (`vscode/src/chat.ts`, `WAKES`, repris chez nous).

## 2. Plan proposé (sans code)

1. **Mesure d'abord** : trace des événements du worker vus par le pont pendant un tour de réveil (`agent_start`, contenu, `agent_end`), sur le binaire de `main`, avec le pilote de capture. Si `agent_start`/`agent_end` n'arrivent pas tels quels, arrêt et compte rendu.
2. **Pont** (module du fork `crates/pa-daemon/src/acp/ya_turns.rs`, accroche minimale dans la boucle d'événements de `daemon.rs`, fichier upstream, listée dans `docs/fork-changes.md`) :
   - à `agent_start` hors prompt : allouer un identifiant de tour (même compteur que `begin_prompt`), publier un **début de tour** avec son origine (`wake` et son genre) ;
   - à `agent_end` de ce tour : publier `responseBoundary` (`result` ou `error`), puis `finish_prompt` ;
   - **début des tours de prompt** : le même signal, `origin: "prompt"`, publié à l'admission (avant le premier événement du tour) ;
   - un prompt qui arrive pendant un tour de réveil : refusé comme aujourd'hui pendant un tour, ou mis en attente (choix T3).
3. **Extension** : lire le signal de début et la frontière des tours de l'agent ; retirer la déduction de `meta.ts`. Agent factice aligné sur une nouvelle capture (`compare_frames.py`).
4. **Tests** :
   - Rust : e2e du pont (provider factice, `bash()` non lu) : début, contenu et fin du tour de réveil avec un identifiant propre ; deux réveils, deux identifiants ; un prompt envoyé pendant un réveil ;
   - extension, e2e contre le vrai binaire (venu de l'étape 6 de la PR 3) : tour lancé par l'agent affiché, fermé sur sa frontière, prompt suivant envoyé tout de suite.
5. **Préfixe** : rien de ce qui part au modèle ne change (trames ACP seulement). Preuve : captures `main` contre branche, `compare.py --all` TOUT IDENTIQUE, cas `acp` et `tui`.

## 3. Fichiers prévus

- `crates/pa-daemon/src/acp/ya_turns.rs` (nouveau), `crates/pa-daemon/src/acp/daemon.rs` (upstream, accroche), `crates/pa-daemon/src/acp/meta.rs` (champ de début de tour, si option T1 a).
- `extensions/vscode/src/ya/meta.ts`, `test/fake-agent/fake-agent.mjs`, tests.
- `docs/fork-changes.md`.

## 4. Choix à trancher

1. **T1, forme du signal de début** :
   - (a) dans l'espace du pont (`ai.primeintellect.prime-agent`), champ `turnStart` à la forme d'Unharnessed (`origin`, `wake`) : l'extension reprise le lit déjà ;
   - (b) sous `_yus/turnStart` (décision du 19/11/2026 pour nos ajouts, `docs/yusAgent-cdc.md:89`).
   - Recommandé : (b), qui suit la décision du 19/11, avec un adaptateur de quelques lignes dans `meta.ts`.
2. **T2, fin d'un tour de réveil** : `responseBoundary` + `terminalQuiescence` comme pour un prompt (recommandé : un seul chemin de fin dans l'extension), ou un `turnEnd` dédié.
3. **T3, prompt pendant un tour de réveil** : refusé comme un prompt pendant un tour (comportement actuel du pont pour deux prompts) ; ou mis en attente par le pont jusqu'à la fin du réveil. Recommandé : refusé, l'extension met déjà le message en file tant que le panneau est occupé (le message pendant un tour est une autre étape du découpage, R4/X4).
4. **T4, Stop pendant un tour de réveil** : garder `session/cancel` (arrête le réveil, `AbortAndClearQueue`) ; mesure de l'effet sur la file dans le ticket.

## Points ouverts

- `agent_start`/`agent_end` d'un tour de réveil vus par le pont : non mesurés (plan, étape 1).
- Effet de `session/cancel` hors prompt sur la file du worker : non mesuré.
- Tours lancés par le worker autres que `[bash-done]` : à lister et mesurer (1.5).
