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
