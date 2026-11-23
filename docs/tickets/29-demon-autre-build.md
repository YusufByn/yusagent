# Ticket 29 : un démon lancé par un autre binaire reste en service

Constat du 22/11/2026, sur la branche `ya-28-x1` (PR #80), base `main` à `9937e4ff8`. **Constat seulement, aucun code.** Ouvert sur décision de Yusuf après le contrôle manuel de X1 (défaut 1, double ouverture acceptée) : le refus clair (b) est dans la PR #80, la détection d'un démon d'un autre build est traitée ici.

## 1. Ce qui s'est passé pendant le contrôle de X1

- À 16:43:32, la fenêtre window1 lance son premier pont avec le binaire `d2f54743e` (PR #77, avant R1). Ce pont démarre le démon : pid 64617, `~/.cache/yusagent-bin/d2f54743e…/yusagent --mode daemon` (relevé `ps`).
- À 16:44:04, `yusagent.binaryPath` passe à `9937e4ff8`. Les ponts suivants, et tous ceux de window3 et window4, sont des `9937e4ff8`. Ils se branchent sur le démon `d2f54743e` déjà en service. Ses workers sont tous lancés depuis `d2f54743e` (relevé `ps` des processus `yusagent worker`).
- Hello lu sur le socket de ce démon : `appVersion` vaut `0.10.2-continuous.d2f54743e1b4ac98f40a6a72f3e098c94bf5fe39`, `runtime.buildId` vaut `pa-daemon-rs-0.10.2`, `schemaId` vaut `protocol-7-schema-30-8e4b17c2a9f5`. Ce dernier est le même que celui de `9937e4ff8` (`crates/pa-types/src/daemon/mod.rs:16-17`).

## 2. Pourquoi le pont l'accepte

- Le mode ACP s'assure qu'un démon répond avec `ensure_daemon_running` (`crates/pa-cli/src/print_runtime.rs:104`).
- `probe_daemon` juge le démon « Current » sur deux champs du hello : la version du protocole et le `schemaId` (`crates/pa-cli/src/interactive_mode/daemon.rs:37-41`). Un démon d'un autre build, de même schéma, est donc gardé tel quel.
- Le hello porte bien de quoi distinguer les builds, mais personne ne le compare :
  - `appVersion` est la version produit (`crates/pa-daemon/src/protocol.rs:43-44` ; `crates/pa-daemon/src/supervisor/clients.rs:124`). Avec le manifeste du paquet à côté du binaire, elle contient le SHA (`0.10.2-continuous.<sha>`).
  - `runtime.buildId` vaut `pa-daemon-rs-` suivi de la version du crate (`crates/pa-daemon/src/supervisor/clients.rs:126`). Elle est la même pour tous nos builds (`pa-daemon-rs-0.10.2`) et ne les distingue pas.
  - `daemon_discovery` lit ces deux champs pour la liste des démons (`crates/pa-cli/src/daemon_discovery/mod.rs:351-366`), sans les comparer au binaire courant.
- Nos ajouts au worker (actions `ya_permissions`, journal d'usage) ne changent pas le `schemaId`. Rien ne signale donc qu'un worker ancien ne les connaît pas.

## 3. Effets constatés ou lus

- **C12** (refus d'une seconde fenêtre). Le worker `d2f54743e` ne connaît pas l'action `others` : sa liste s'arrête à `respond`, `set_mode`, `get` et `asker` (`d2f54743e:crates/pa-daemon/src/ya_permissions.rs:389`). Sur `9937e4ff8`, le pont lisait cet échec comme « aucun autre client » (`9937e4ff8:crates/pa-daemon/src/acp/ya_history.rs:162-163`). La double ouverture était donc acceptée.
  - Reproduction : `bench/tools/capture/c12_repro.py`. Démon `9937e4ff8` : B refusé dans les 4 cas. Démon `d2f54743e` avec ponts `9937e4ff8` : B accepté dans les 4 cas.
  - Depuis la PR #80, le pont refuse en disant de lancer `yusagent shutdown` (`ya_history.rs`, `OLDER_DAEMON`).
- **C8** (journal d'usage : empreinte du préfixe, `resume_prefix_change`). Lu, pas mesuré. Le journal du worker vient de `9937e4ff8` (`crates/pa-core/src/session_engine/engine.rs:438-446`, `usage_log.rs`). Un worker `d2f54743e` n'écrit ni l'empreinte ni l'invalidation de reprise, sans erreur visible.
- **Tout ajout futur au worker** (nouvelle action, nouveau champ) aura le même angle mort : le pont récent parle à un worker qui ne le connaît pas.
- **Les autres points du contrôle de X1** (1 à 6, 8) tournaient aussi sur ce démon. Entre `d2f54743e` et `9937e4ff8`, seuls `ya_permissions.rs` (action `others`) et le journal d'usage changent côté worker (`git diff --stat d2f54743e 9937e4ff8 -- crates`). Le reste de R1 est dans le pont.

## 4. Upstream

- `upstream/main` à `783acb5d7` (21/11/2026), récupéré le 22/11 : `probe_daemon` est identique (même test, même fichier, `crates/pa-cli/src/interactive_mode/daemon.rs`).
- Dernier commit upstream sur ce fichier : `3407a54d8` (14/10/2026, « scope shutdown to its own daemon tree »), sans rapport avec la détection.
- Aucun commit upstream depuis septembre ne parle de détecter un démon d'un autre build. Recherche : `git log upstream/main --since=2026-09-01 -i --grep='stale daemon\|build id\|buildId\|daemon.*version\|version.*daemon\|incompatible daemon'`. Quatre commits sortent (`15929ad03`, `069014c2e`, `e4f2e586b`, `2e9ab77b8`). Seul `2e9ab77b8` (21/09, côté TypeScript) touche `appVersion` : il le rapporte (`daemonVersion`), sans le comparer. Aucun ne compare le build du démon.
- Chez upstream, un changement de binaire passe par `update`, qui redémarre le démon (`crates/pa-cli/src/update_flow/coordinator.rs:269-274`). Chez nous, l'extension change de binaire par `yusagent.binaryPath`, sans passer par `update`, et le démon reste.

## 5. Questions ouvertes (à trancher avant tout code)

1. Comparer `appVersion` au binaire courant dans `probe_daemon` ? `buildId` ne suffit pas : il ne distingue pas nos builds. C'est un fichier upstream, avec un changement minimal.
2. Que faire d'un démon d'un autre build ?
   - Inactif : le remplacer, comme un démon de schéma différent (`shutdown_stale_daemon`). Coût : les workers résidents s'arrêtent, et la reprise suivante part d'un worker neuf (cas R-neuf, kernel restauré depuis l'instantané s'il existe).
   - Occupé : refuser avec la consigne `yusagent shutdown`.
3. Ce remplacement se déclencherait à chaque nouveau build en développement. Est-ce acceptable, ou faut-il une option ?
4. L'extension doit-elle afficher la version du démon, ou prévenir d'un écart ?

## 6. Relecture du constat sur `main` à `beecf3f38` (23/11/2026)

Branche `ya-29-demon-autre-build`, depuis `origin/main` à `beecf3f38`. Plan seulement, aucun code.

Références du §2 et du §3 vérifiées, toujours justes : `print_runtime.rs:104`, `interactive_mode/daemon.rs:37-41`, `protocol.rs:43-44`, `supervisor/clients.rs:124` et `:126`, `daemon_discovery/mod.rs:351-366`, `pa-types/src/daemon/mod.rs:16-17`, `engine.rs:438-446`. Corrections et ajouts :

- **Le TUI passe aussi par ce chemin** : `ensure_daemon_running` est appelé par le mode interactif (`crates/pa-cli/src/interactive_mode.rs:100`), pas seulement par le mode ACP. Un correctif dans `probe_daemon` touche les deux.
- **`appVersion` ne porte le SHA qu'avec un `package.json`** : la version vient du manifeste lu au démarrage (`crates/pa-cli/src/config.rs:51-57`, `:74-79`, posée par `crates/pa-cli/src/lib.rs:95`), cherché dans `PI_PACKAGE_DIR` ou à côté du binaire (`config.rs:62-72`). Sans manifeste (un `cargo build` local), tous les builds annoncent `0.10.2` (`config.rs:47`). Les archives de CI en ont un : `~/.cache/yusagent-bin/<sha>/package.json` vaut `0.10.2-continuous.<sha>` (lu sur les 4 dossiers les plus récents du cache).
- **Le hello porte un troisième champ** que le constat ne cite pas : `runtime.executablePath` (`supervisor/clients.rs:127-129`).
- **C12** : le refus est à `crates/pa-daemon/src/acp/ya_history.rs:156` (texte) et `:172-173`. Il se déclenche sur **tout** échec de l'action `others`, pas seulement sur un worker ancien. Test : `ya_history_tests.rs:425-445`.
- **Ce qu'est un démon « occupé »** pour `shutdown_stale_daemon` : une session avec `isSessionActive` vrai, soit un tour ou un bash en cours (`interactive_mode/daemon.rs:126-133`). Une session ouverte mais au repos dans une autre fenêtre ne compte pas, et le démon est arrêté quand même.
- **Arrêt du démon** : le superviseur ne lit pas `force` (`supervisor/clients.rs:627-652`). Il envoie `daemon_closing` à tous les clients (`:640-646`), puis s'arrête.
- **§4, citation de `update`** : `coordinator.rs:269-274` est le message de fin. La relance elle-même est `coordinator.rs:206-222` (successeur lancé avec le roster par l'env, puis attente du hello). Chez nous, `update` est désactivé (`crates/pa-cli/src/public_command.rs:665-668`, YA-16) : ce chemin n'existe pas.
- **Upstream depuis `783acb5d7`** : `upstream/main` à `fda67368b` (23/11/2026), 12 commits de plus. Aucun ne touche `interactive_mode/daemon.rs`. La recherche du §4 (`git log upstream/main 783acb5d7..upstream/main -i --grep=…`) ne renvoie rien, et `git diff 783acb5d7 upstream/main | grep -i 'appVersion\|app_version\|build_id'` non plus. Le défaut est toujours là upstream.

## 7. Plan (à valider)

### 7.1 Reconnaître un démon d'un autre build

| Option | Signal | Détecte | Manque ou faux positif | Fichiers upstream |
|---|---|---|---|---|
| **D-a (recommandée)** | `appVersion` du hello ≠ `crate::config::version()` du pont | Tout changement entre deux archives de CI (le SHA est dans la version) | Builds locaux sans manifeste (tous `0.10.2`) : pas détectés, comme aujourd'hui. Pas de faux positif connu : deux copies du même build ont la même version | Aucun côté démon |
| D-b | `runtime.executablePath` ≠ `current_exe()` | Un changement de `binaryPath`, même sans manifeste | Faux positif : le même build atteint par deux chemins (lien symbolique, copie) relance le démon pour rien. Manque : un binaire écrasé au même chemin | Aucun côté démon |
| D-c | SHA compilé dans `runtime.buildId` (`option_env!("GITHUB_SHA")` ou `build.rs`) | Tout build de CI, indépendamment du manifeste | Builds locaux : même manque que D-a, sauf avec un `build.rs` qui lit git | `supervisor/clients.rs:126`, hello du worker (`worker/connection.rs:335`), plus un `build.rs` ou une variable de CI |

Champs absents (vieux démon, démon TypeScript) : on ne décide pas, le contrôle de schéma existant suffit.

### 7.2 Ce que fait le pont

Emplacement recommandé : un module à nous, `crates/pa-cli/src/ya_daemon_build.rs`, appelé avant `ensure_daemon_running` dans le mode ACP seulement (`print_runtime.rs:104`, fichier déjà modifié par YA-11). Il lit le hello, décide (relance ou refus), puis laisse `ensure_daemon_running` faire le reste : s'il a arrêté le démon, celui-ci trouve un socket vide et lance le binaire courant. `interactive_mode/daemon.rs` reste intact, et le TUI garde le comportement d'upstream (choix 2).

| Option | Mode ACP (fenêtre qui démarre) | Autres fenêtres (ponts ACP) | TUI branché sur ce démon |
|---|---|---|---|
| A1 Refus toujours | Échec au démarrage, message avec les deux versions et `yusagent shutdown`, visible dans le panneau d'échec de l'extension (fin de stderr : `extensions/vscode/src/agent.ts:143`, `:257`, `:266`) | Intactes | Intact |
| A2 Relance si aucun tour en cours (`shutdown_stale_daemon` tel quel) | Démon relancé depuis le binaire courant, la session repart d'un worker neuf (R-neuf, kernel restauré depuis l'instantané s'il existe) | **Coupées sans prévenir** : le lien se ferme, puis chaque requête échoue avec « the daemon connection is closed » (`crates/pa-daemon/src/acp/daemon.rs:486-492`, `:218-228`). Le pont ne se reconnecte pas : l'utilisateur voit l'erreur au prompt suivant et doit faire Restart. Lu, pas mesuré | Reçoit `daemon_closing`, attend le retour du démon sur le même socket puis se rattache (`crates/pa-tui/src/daemon_reconnect.rs:19-23`), avec un bandeau qui signale l'écart de version (`:31-53`). Session sur un worker neuf. Lu, pas mesuré |
| **A3 (recommandée)** Relance si aucun tour en cours **et** aucun client attaché (`attachedClients` = 0 sur toutes les lignes de `list`, `crates/pa-daemon/src/types.rs:88`, `worker/summary.rs:595`), après une attente courte (3 s, comme C12) pour un pont qui vient de fermer ; refus sinon | Cas courant (une fenêtre, `binaryPath` changé, Restart) : relance automatique. Sinon refus, avec les deux versions, le nombre de sessions ouvertes et `yusagent shutdown` | Intactes : leur présence fait refuser la relance | Intact (un TUI attaché fait refuser) |
| A4 Relance avec reprise du roster (mécanisme de `update`, `coordinator.rs:206-222`) | Comme A2 | Coupées quand même : le pont ACP ne se reconnecte pas | Rattaché, sessions restaurées. Mais `update` est désactivé chez nous et ce chemin passe par la mise en place d'un candidat : trop lourd ici |

À vérifier avant le code de A3 : `attachedClients` doit bien être présent et juste dans les lignes de `list` d'un vrai démon (lu dans le code et les fixtures, pas encore relevé sur un démon).

Cas de course : deux ponts de builds différents qui démarrent en même temps se disputent le socket. La boucle de démarrage voit alors un démon « Current » d'un autre build (`interactive_mode/daemon.rs:91-97`) et l'accepte. Ce cas reste couvert par le refus de C12 (§7.3).

**Unharnessed** (lu à `f8e4658dd`, 22/11/2026) : **ne s'applique pas**. Il n'a pas de démon superviseur : le mode ACP construit le moteur de session dans le processus même (`crates/uh-cli/src/print_runtime.rs:129-148`, « build the in-process session engine »), et le crate `uh-daemon` ne garde que l'ACP, les baux et l'auth (aucun `supervisor`, aucun `interactive_mode/daemon.rs`). Chaque fenêtre tourne sur son propre binaire jusqu'à son redémarrage. Une seconde fenêtre sur la même session est refusée par un bail sur fichier (`crates/uh-daemon/src/lease.rs:1-7`, `session_already_active`), quel que soit le build. Le crate s'appelle encore `uh-daemon`, mais il ne lance pas de démon.

### 7.3 Ce que deviennent `OLDER_DAEMON` (C12) et `yusagent shutdown --force`

- `OLDER_DAEMON` reste, en garde-fou : C12 doit échouer fermé. Avec D-a, il ne se déclenche plus que dans trois cas : builds sans manifeste, course du §7.2, ou autre échec de l'action `others`. Le commentaire `ya_history.rs:153-155` (« the bridge does not detect a daemon of another build (ticket 29) ») devient faux et doit être mis à jour. Le texte du message est à trancher (choix 4).
- Le contournement `yusagent shutdown --force` n'est plus nécessaire dans le cas courant (A3 relance seule). Il reste la consigne quand le pont refuse. Les messages d'upstream (`interactive_mode/daemon.rs:102`, `:137`) ne changent pas. Notre message de refus dirait `yusagent shutdown`, puis Restart. Un `--force` n'apporte rien côté superviseur (qui ne lit pas `force`, `clients.rs:627-652`). Son effet côté CLI (`daemon_discovery/stop.rs:393-399`) est à relire avant d'écrire le message.
- Le script `bench/tools/capture/c12_repro.py --daemon-binary` change de résultat attendu : le démon ancien, au repos, est maintenant remplacé au démarrage de A. B doit alors être refusé par le C12 normal, et non plus par `OLDER_DAEMON`. Le verdict du script est à adapter.

### 7.4 Préfixe de prompt

Aucun effet attendu : le changement porte sur le choix du démon avant toute session. Prompt, tools et `yaRequest` ne changent pas. Contrôle, lancé par Claude Code avant la remise :
`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/run.py --binary <branche>/yusagent`, puis la même commande sur le binaire de `main` (`beecf3f38`, déjà dans `~/.cache/yusagent-bin/`), puis `python3 -I bench/tools/capture/compare.py <run main> <run branche> --all --out <fichier>.md`. Attendu : `TOUT IDENTIQUE`, et `prefix_paths_check` `CONFORME` (lancé par `run.py`).

### 7.5 Fichiers, tests, script, contrôle manuel

Fichiers (avec D-a, A3, ACP seul) :
- nouveaux : `crates/pa-cli/src/ya_daemon_build.rs` (lecture du hello, décision, `list`, `shutdown`, attente du socket vide, message), `bench/tools/capture/daemon_build_check.py` ;
- upstream : `crates/pa-cli/src/lib.rs` (1 ligne `mod`) et `crates/pa-cli/src/print_runtime.rs` (1 appel avant `:104`), tous deux listés dans `docs/fork-changes.md` ;
- du fork : `ya_history.rs` (commentaire, et texte si choix 4), `c12_repro.py` (verdict), `docs/fork-changes.md`, journal du CDC, ce ticket.

Tests du push 1, rouges sur leur assertion. Ils tournent contre un superviseur factice sur un socket temporaire, avec une version du pont injectée ; une ébauche qui garde le comportement actuel (`Ok`, sans rien envoyer) les fait compiler :
1. `other_build_idle_daemon_is_shut_down` : version différente, `list` sans tour ni client attaché. Attendu : `shutdown` reçu. Aujourd'hui : rien n'est envoyé.
2. `other_build_busy_daemon_is_refused` : `isSessionActive` vrai. Attendu : `Err`, avec les deux versions et `yusagent shutdown`, sans `shutdown` envoyé. Aujourd'hui : `Ok`.
3. `other_build_daemon_with_attached_client_is_refused` (A3) : `attachedClients` à 1 pendant plus de 3 s. Même attente que le test 2.
4. Verts dès le push 1 (non-régression) : `same_build_daemon_is_kept`, `daemon_without_app_version_is_left_to_the_schema_check`.
- `ya_history_tests.rs:445` change seulement si le texte d'`OLDER_DAEMON` change (choix 4).

Script de contrôle (`daemon_build_check.py --binary <branche> --other-binary <main>`), sur le modèle de `c12_repro.py` : démon isolé, modèle factice, `sandbox-exec`. Il est lancé par Claude Code, et sa sortie est jointe à la remise. Cas :
1. démon `main` sans session : le pont de la branche le remplace (pid changé, `appVersion` du hello = branche) ;
2. démon `main` avec une session au repos tenue par un pont `main` : refus (A3), pid inchangé, et le pont `main` répond toujours ;
3. démon `main` avec un tour en cours (réponse factice lente) : refus, pid inchangé, le tour se termine ;
4. même build deux fois : pas de relance ;
5. `c12_repro.py` (avec et sans `--daemon-binary`) : `CONFORME`.

Contrôle manuel prévu pour Yusuf (extension) :
1. `yusagent.binaryPath` sur `main` (`beecf3f38`), une fenêtre, une conversation. Relever le pid du démon (`ps -o pid,command | grep 'yusagent --mode daemon'`).
2. `binaryPath` sur la branche, Restart dans la même fenêtre. Attendu : nouveau pid, démon et workers lancés depuis le chemin de la branche, et la conversation reprend.
3. Deux fenêtres sur `main`, puis la seconde passe sur la branche et fait Restart pendant que la première tient sa conversation. Attendu (A3) : refus lisible dans le panneau d'échec, avec les deux versions et `yusagent shutdown` ; la première fenêtre continue.
4. `yusagent shutdown`, puis Restart de la seconde fenêtre : démon de la branche. La première fenêtre affiche une erreur au prompt suivant, puis repart après un Restart.
5. Deux fenêtres sur la branche, même conversation : C12 refuse toujours la seconde.

## 8. Choix à trancher

1. Signal : D-a (`appVersion`, recommandée), D-b ou D-c ?
2. Périmètre : mode ACP seul (recommandé, le TUI garde le comportement d'upstream, `daemon.rs` intact) ou aussi le TUI (`interactive_mode.rs:100`, risque de bascule entre un TUI et une extension de builds différents sur le même socket) ?
3. Action : A3 (recommandée), A1 ou A2 ? Avec A3, l'attente de 3 s pour un pont qui vient de fermer est-elle à garder ?
4. Texte d'`OLDER_DAEMON` : garder tel quel (cas devenu rare) ou dire « peut-être d'un autre build » ?
5. Ticket 29, question 4 (l'extension affiche la version du démon ou prévient d'un écart) : hors de ce ticket (recommandé) ou dedans ?

## 9. Décisions de Yusuf (23/11/2026)

1. Signal : `appVersion`. Le .vsix qui embarquera le binaire devra garder le manifeste à côté du binaire (point ouvert de YA-16 dans le CDC).
2. Périmètre : mode ACP seul.
3. Action : A3, avec l'attente de 3 s.
4. `OLDER_DAEMON` : le message dit « peut-être d'un autre build » et donne les deux versions et `yusagent shutdown`.
5. Version du démon dans l'extension : hors de ce ticket.

## 10. Vérifications avant le code (23/11/2026)

### A. `attachedClients` sur un vrai démon : confirmé

Commande : `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/tools/capture/attached_clients_probe.py --binary ~/.cache/yusagent-bin/beecf3f38233a79f72e90ce6ca0ddf6d496def92/yusagent --out <dossier>` (démon isolé, modèle factice).

```
hello : appVersion 0.10.2-continuous.beecf3f38233a79f72e90ce6ca0ddf6d496def92
avec_A_ouvert
  {"qui": "P (fermé, sans client)", "attachedClients": 0, "isSessionActive": false, "lifecycle": "live", "activity": "idle", "messageCount": 8, "hasRegisteredCronJob": false}
  {"qui": "A (ouvert, attaché)", "attachedClients": 1, "isSessionActive": false, "lifecycle": "live", "activity": "idle", "messageCount": 8, "hasRegisteredCronJob": false}
apres_fermeture_de_A
  {"qui": "P (fermé, sans client)", "attachedClients": 0, ...}
  {"qui": "A (ouvert, attaché)", "attachedClients": 0, ...}
```

### B. Ce que perd l'arrêt du démon pour une session sans client : **change le plan**

À l'arrêt du démon, chaque worker reçoit `shutdown` (`crates/pa-daemon/src/supervisor/signals_shutdown.rs:45-56`). C'est le même gestionnaire que la passivation. Il dispose le kernel **avec un instantané final** (`crates/pa-core/src/kernel/manager/teardown.rs:56-57`), puis arrête les `bash()` du kernel (`:62-63`). La différence avec la passivation : celle-ci refuse une session qui tient encore du travail (`crates/pa-daemon/src/agent_engine/session_engine_impl.rs:30-41`, `crates/pa-daemon/src/goal_continuation.rs:240-255`). L'arrêt du démon ne vérifie rien de cela.

| Élément | À l'arrêt | À la reprise | Récupéré | Visible dans `list` |
|---|---|---|---|---|
| Worker, kernel, variables Python | Instantané final, puis arrêt | Restauré depuis `kernel-state.dill` (objets non sérialisables, cwd et `os.environ` exclus, ticket 28) | Partiel | Non (`workerState` seulement) |
| `bash()` en arrière-plan (sans `persistent=True`) | Tué (`teardown.rs:62-63`) | Non | **Non** | **Non** : `isBashRunning` ne suit que le `!` de l'utilisateur (`crates/pa-daemon/src/worker/summary.rs:468`). Le handle n'est lu que par `has_live_background_bash_handles` (`goal_continuation.rs:231`), et aucune commande existante ne l'expose |
| `bash(persistent=True)` | Survit | L'avis de fin n'est probablement pas envoyé (déduit) | Partiel | Non |
| Crons, heartbeats, reprise après quota | Gardés sur disque, mais rien ne se déclenche tant qu'aucun worker ne tourne (`goal_continuation.rs:242-243`) | Réarmés quand la conversation est rouverte | **En retard, ou jamais** si elle ne l'est pas | **Non** : `hasRegisteredCronJob` est écrit à `false` en dur (`summary.rs:578`). Lisible par `cron_list` et `heartbeats_list` (`crates/pa-types/src/daemon/command.rs:830-847`), commandes d'upstream servies par les anciens démons |
| Suite d'un goal en mémoire | Perdue (déduit du rapport, non relu) | État du goal relu depuis le fichier | Partiel | Non |
| Avis `[bash-done]` | Pas d'avis retenu dans le worker : ils vont dans les files, ce qui rend la session active | — | Sans objet au repos | `isSessionActive` (`summary.rs:577`) |
| Journal d'usage | Écrit ligne à ligne, rien en tampon (`crates/pa-core/src/session_engine/usage_log.rs:557-577`) | Compteurs relus | Oui | — |
| Sous-agents en cours | Arrêtés | Relistés | Partiel | `hasRunningSubagents` |
| Quota en attente | Indicateur en mémoire perdu, reprise par cron | Par cron | Partiel | `isQuotaParked` |

`unfinishedActionCount` est écrit à 0 en dur (`summary.rs:618`) : il ne sert à rien.

**Conséquence.** A3, telle que prévue (champs de `list` seuls), arrêterait des `bash()` en arrière-plan et suspendrait des crons et heartbeats. Ces pertes sont invisibles dans `list` et ne se rattrapent pas. Proposition (§11), à trancher avant le code.

## 11. Proposition : ce que A3 compte comme « occupé »

Une session vivante tient le démon si l'un de ces points est vrai :
1. Champs de `list`, lisibles sur tous les builds : `attachedClients > 0` (après l'attente de 3 s), `isSessionActive` (flux, compaction, file non vide), `isBashRunning`, `isRunningTools`, `hasRunningSubagents`, `isQuotaParked`.
2. Une nouvelle action du worker, `holds`, dans notre `ya_permissions` (aucun fichier upstream touché). Elle répond comme la passivation : `bash()` en arrière-plan, travail RLM non réglé, cron ou heartbeat armé, enfants RLM (`settled_passivation_gates_pass`, `goal_continuation.rs:245-255`, plus le registre des enfants). Faisabilité à vérifier : l'accès au moteur depuis le gestionnaire de `ya_permissions`.
3. Démon d'avant ce ticket, qui ne sert pas `holds` : il faut choisir.
   - **P1a (recommandée)** : toute session vivante sans réponse compte comme occupée, et le pont refuse avec la consigne `yusagent shutdown`. Une seule intervention manuelle au passage vers le premier build qui a `holds` ; les passages suivants sont automatiques.
   - P1b : `cron_list` et `heartbeats_list` pour les crons, et les `bash()` en arrière-plan acceptés comme perdus une fois.

## 12. Décisions de Yusuf sur le §11 (23/11/2026)

1. « Occupé » = points 1 et 2 du §11. L'action `holds` **réutilise les gardes de la passivation** (`SessionEngine::can_passivate_worker`, `crates/pa-daemon/src/agent_engine/session_engine_impl.rs:30-41`, et ce qu'elle appelle : `settled_passivation_gates_pass`, `crates/pa-daemon/src/goal_continuation.rs:245-255`, `has_unsettled_rlm_work` `:220`, `has_live_background_bash_handles` `:231`, la sonde des crons et heartbeats `crates/pa-daemon/src/worker.rs:586-604`, le registre des enfants RLM). Elle ne réimplémente rien, pour que les deux ne divergent jamais. La garde des goals en fait partie.
2. P1a : un démon qui ne sert pas `holds` fait compter toute session vivante comme occupée, et le pont refuse avec la consigne `yusagent shutdown`.

### Ce que demande la réutilisation (relu avant le code)

- `can_passivate_worker` est asynchrone (`session_engine_impl.rs:30-32`). Notre gestionnaire `handle_ya_permissions` est synchrone (`crates/pa-daemon/src/ya_permissions.rs:330`). Son appel, `crates/pa-daemon/src/worker/commands.rs:163-164`, est notre ligne (YA-11, ticket 20) dans un fichier upstream, à l'intérieur d'un dispatch `async` dont les voisins font déjà `.await`. Pour l'attendre, il faut écrire `self.handle_ya_permissions(payload).await` sur cette ligne. **C'est la seule retouche d'un fichier upstream, sur une ligne déjà à nous.** En attente de l'accord de Yusuf.
- **Garde des goals** : la passivation n'a pas de garde propre aux goals. Le goal est tenu par les gardes « réglées » de `goal_continuation.rs` : la suite d'un goal attend le travail RLM non réglé ou un `bash()` en arrière-plan (`goal_continuation.rs:1-4`, `:218-235`). Une suite déjà produite est dans la file, donc `isSessionActive`. Les autres états du goal en mémoire (`crates/pa-core/src/session_engine/goal_driver.rs:48-63` : suite due, suite en attente d'admission, attente après une absence de progrès) ne sont pas lus par la passivation, et ne le seront donc pas par `holds`. Le test « goal en cours » portera sur une suite de goal différée derrière un travail RLM non réglé.

### Limite (décision 2 du 23/11/2026)

`holds` est aligné sur la passivation : les états du goal gardés seulement en mémoire (`crates/pa-core/src/session_engine/goal_driver.rs:48-63`) n'y entrent pas. Ce sont la suite due, la suite en attente d'admission et l'attente après une absence de progrès. Si A3 relance le démon, ils sont perdus, comme après une passivation. Le test « goal en cours » porte sur une suite de goal différée derrière un `bash()` en arrière-plan, le chemin du test upstream `live_background_bash_holds_the_goal_continuation_until_it_settles` (`goal_continuation.rs`). La fonction privée `goal_children_settled` n'est pas accessible depuis nos tests, d'où ce choix plutôt qu'un travail RLM. Le travail RLM a son propre test (`a_running_rlm_child_holds_the_session`).

## 13. Réalisation (PR #82)

- `crates/pa-cli/src/ya_daemon_build.rs` (nouveau) : `replace_other_build`, appelé par le mode ACP avant `ensure_daemon_running` (`crates/pa-cli/src/print_runtime.rs`).
  - Hello : même protocole et même schéma, mais `appVersion` différent de `crate::config::version()`. Sinon rien ne change : pas de démon, autre schéma (laissé à `probe_daemon`), même build, version absente.
  - `list` : seules comptent les lignes avec un `activeSessionId`. Une ligne passivée n'en a plus (`passivated_summary`, `crates/pa-daemon/src/supervisor_roster.rs`), et `lifecycle` y reste `"live"`.
  - Une session tient le démon si un client est attaché, si un des champs de `BUSY_FIELDS` est vrai, ou si `holds` répond vrai ou échoue (P1a). Les lectures se répètent toutes les 100 ms pendant 3 s au plus.
  - Si rien ne le tient : `shutdown`, puis attente que le socket ne réponde plus, 30 s au plus. Le superviseur accepte des connexions jusqu'à l'arrêt de ses workers, qui écrivent chacun un instantané : sans cette attente, un pont lancé tout de suite rejoindrait le démon mourant.
  - Sinon : refus sur stderr, qui finit dans le panneau d'échec de l'extension. Il donne les deux versions, la liste des sessions avec leurs raisons, et `yusagent shutdown`.
- `crates/pa-daemon/src/ya_permissions.rs` : `holds_work` vaut `!can_passivate_worker()`, et l'action `holds` est servie avant le contrôle `agent_engine`. `.await` ajouté sur `crates/pa-daemon/src/worker/commands.rs:163`.
- `crates/pa-daemon/src/acp/daemon.rs`, `ya_history.rs` : `DaemonLink::daemon_version`, et `older_daemon(link)` à la place d'`OLDER_DAEMON`.

## 14. Push 1 : tests seuls (`9b7a9454d`, run `ci` 45166498970)

Ébauches : `replace_other_build_as` renvoie `Ok(())` sans rien envoyer, et `holds_work` renvoie `false`. Échecs relevés par `gh run view 45166498970 --log-failed`.

**Tests rouges sur leur assertion, comme prévu (13)** :
- shard 8, `pa-cli` : 7 tests de `ya_daemon_build`.
  - `an_idle_daemon_of_another_build_is_shut_down`, `a_client_leaving_during_the_wait_lets_the_daemon_go` et `a_passivated_session_does_not_keep_the_daemon` échouent à `ya_daemon_build.rs:187` (`shutdown` attendu).
  - `a_busy_session_keeps…`, `an_attached_client_keeps…`, `a_session_holding_work_keeps…` et `a_daemon_without_holds_counts_every_live_session_as_busy` échouent à `:170` (refus attendu).
- shard 3, `pa-daemon` : 6 tests.
  - `holds::a_background_bash_holds_the_session` (`ya_permissions.rs:764`), `an_armed_cron_or_heartbeat_holds_the_session` (`:779`), `a_goal_waiting_behind_a_background_bash_holds_the_session` (`:808`, après l'assertion « suite de goal différée », qui passe) et `a_running_rlm_child_holds_the_session` (`:850`) échouent tous sur `assert!(holds(&engine))`.
  - `acp::ya_history::tests::a_daemon_that_cannot_count_the_other_windows_refuses_the_load` échoue à `ya_history_tests.rs:446` (« another build »).
- Verts dès le push 1, comme prévu : `a_daemon_of_the_same_build_is_kept`, `a_daemon_without_version_or_of_another_schema_is_left_to_upstream`, `no_daemon_is_nothing_to_do`, `holds::a_session_at_rest_holds_nothing`. Les autres tests des 8 shards passent.

**Trois autres échecs, dus au push lui-même et non au comportement** :
- `check + clippy (workspace)` : `clippy::unused_async` sur l'ébauche `holds_work`, qui n'avait pas d'`await` (`ya_permissions.rs:334`). Le correctif l'attend, la cause disparaît.
- `windows cross-check + clippy` : `tokio::net::UnixListener` n'existe pas sous Windows (`ya_daemon_build.rs:92`). Le module de tests passe sous `#[cfg(all(test, unix))]`, comme les tests à socket Unix d'upstream (`crates/pa-cli/src/update_flow/legacy_restart.rs:470`).
- `Changelog fragment` : pas encore de `.changes/ya-29-demon-autre-build.md`. Il arrive avec le push 2, comme aux tickets 28 et 30.
