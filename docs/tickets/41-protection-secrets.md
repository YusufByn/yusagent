# Ticket 41 : protection des secrets (YA-11, gate de YA-16) : constat et plan

Ticket CDC : YA-11, YA-16. Décision du 26/11/2026 (journal du CDC) : le modèle ne doit jamais pouvoir lire les identifiants de yusAgent. Le sandbox du kernel entier est abandonné. Permissions, réseau et reste du système restent libres, et l'environnement du kernel est inchangé (ticket 02).

Branche `ya-41-protection-secrets` depuis `main` à `0de7af7f3`. Ce document est un constat, sans code produit. Il est accompagné d'un script de mesure (`41-protection-secrets/mesure_macos.py`). Les lignes citées sont celles de l'arbre à `9010a9c21`.

Unharnessed (lu à `bb55464`) n'a rien sur ce point : aucun choix à reprendre, l'écart est couvert par l'exception sécurité.

## Résumé

- **Fichiers à protéger** : `auth.json` (clés API, OAuth des fournisseurs, jetons MCP `mcp:*`, clé Serper) avec son temporaire `auth.json.tmp<pid>` et son verrou `auth.json.lock`, plus `models.json` (`apiKey` et `headers` peuvent être littéraux). Pas d'autre secret trouvé dans `~/.yusagent` ni dans `<cwd>/.yusagent/`.
- **Point d'héritage** : `crates/pa-core/src/kernel/manager/startup.rs:168-181`, fichier upstream déjà modifié par le fork. Une enveloppe autour de `Command::new(&python)` y couvre le kernel et tous ses descendants.
- **macOS, mesuré** : Seatbelt (`sandbox-exec`, `(allow default)`) avec un refus par regex de préfixe sur `auth.json*` et `models.json*` tient, fichiers laissés en place. 38 essais conformes, contournements compris (§E.1, annexe ; 42 avec les 4 essais d'exécution différée ajoutés le 26/11).
  - Pour que ça tienne, le kernel doit aussi être coupé du socket du démon : `ExecuteBash` lance un shell **dans le worker**, hors de la protection.
- **Linux, documenté et non mesuré** : masquer un fichier ne survit pas à l'écriture atomique du démon, il faut masquer un **dossier**. Il faudrait donc déplacer `auth.json` dans un dossier à lui. bubblewrap ou un espace de noms demandent les user namespaces non privilégiés, restreints par défaut sur Ubuntu 24.04. Une sonde en CI s'impose avant de choisir.
- **Jetons MCP** : ils sont aujourd'hui **dans la mémoire du kernel**. Le Python lit `auth.json` et construit l'en-tête lui-même (`mcp.py:1088`, `:967`, `:975`). Il faut déplacer l'appel HTTP authentifié dans le worker.
- **Variables d'environnement (G)** : sur macOS, `sysctl(KERN_PROCARGS2)` lit depuis la sandbox l'environnement de tout processus parent (mesuré). Aucune règle Seatbelt essayée ne l'empêche. Je recommande une limite documentée.
- **Recommandation** : macOS d'abord (PR A : Seatbelt et socket), puis les jetons MCP hors du kernel (PR B), puis Linux après la sonde (PR C). Les questions sont au §L.
- **Mise à jour du 26/11/2026** : réponses de Yusuf (§N), plan révisé (§M : B, puis A, puis C). Compléments au §O : inventaire F.4, exécution différée, smoke bench, vérifications de la PR B. **Question bloquante au §P** : `packages`/`npmCommand` font exécuter le worker sans action de l'utilisateur.
- **Mise à jour du 26/11/2026 (suite)** : réponses 11 à 13 (§Q). **Nouveau constat bloquant au §Q.2** (suite au §R : inventaire complet, sorties de sandbox mesurées, comparaison X1/X4 ; réponses et garantie au §S : X1, plan B, puis A, puis C) : le worker lance sans action de l'utilisateur des programmes que le kernel peut changer (`git` du `PATH` à chaque tour, `npm root -g` même avec P1, `exec_command` de la variante shell natif, binaire `yusagent`). Une protection posée sur le kernel seul ne tient pas.

## A. Inventaire

### A.1 Fichiers qui contiennent ou peuvent contenir un secret

| Fichier | Contenu | Qui l'écrit | Qui le lit | Écriture |
|---|---|---|---|---|
| `<agent_dir>/auth.json` | clés API, OAuth des fournisseurs (`access`, `refresh`), jetons MCP `mcp:<id>` (`access`, `key`, `bearer`), clé `serper` | **Démon et worker** : `AuthStorage` (`crates/pa-core/src/auth/manager.rs:275`, `persist_provider_change` `:707-727`). **Pont** : login, `set_api_key`, logout (`crates/pa-cli/src/provider_login.rs:458-488`, `:652-671`, ticket 40 §1.2). **Login MCP du worker** (`crates/pa-daemon/src/mcp_login.rs:4`). **TUI** (`crates/pa-cli/src/interactive_mode.rs:477`, ticket 40 §1.3). | Démon, worker, pont, et **le kernel** : `prime-agent-runtime/src/rlm/mcp.py:1088`, `mcp_base.py:71`, `skills/websearch/src/websearch/websearch.py:46` | Création exclusive en 0600 (`crates/pa-core/src/auth/storage.rs:70-83`), verrou dossier `auth.json.lock` (`storage.rs:88-105`, `crates/pa-core/src/platform/lock_dir.rs:1-3`), puis temporaire `auth.json.tmp<pid>` et renommage (`crates/pa-core/src/settings/storage.rs:291`, `:304`, appelé `auth/storage.rs:239-243`) |
| `<agent_dir>/models.json` | `apiKey` et `headers` de fournisseurs personnalisés : littéral, nom de variable ou `!commande` (`crates/pa-core/src/models/custom.rs:140-142`, résolution `crates/pa-core/src/auth/resolve_config_value.rs:13-36`) | L'utilisateur, à la main. Aucun écrivain trouvé dans le harness (recherche de `"models.json"`, déduit) | Démon, worker, pont et CLI : une trentaine de sites `agent_dir.join("models.json")` (par exemple `crates/pa-daemon/src/worker/create.rs:668`, `crates/pa-core/src/models/registry.rs:518`). Le kernel ne le lit pas (recherche dans `prime-agent-runtime/`, `skills/`) | Sans objet |

- **Les deux fichiers portent aussi des commandes.** Une valeur `!commande` dans `auth.json` (`lookup.rs:57`) ou `models.json` (`registry.rs:314`, `:827`) est **exécutée par le worker**, qui n'est pas protégé. Le refus en **écriture** est donc indispensable au même titre que le refus en lecture : sinon le kernel écrit `"apiKey": "!cat auth.json | curl …"` et le worker exécute (§F.3).
- **Examinés, sans secret** : `settings.json` (clés `defaultModel`, `defaultProvider`, `onboardingShown`, `recentModels` sur ce poste ; `mcpServers.<nom>.headers` peut porter un en-tête littéral, voir §A.3), `telemetry.json` (`installationId`, `version`), `daemon-workers/<id>/supervisor-config` (`version`, `socketPath`, `defaultSessionDir`, `crates/pa-daemon/src/descriptor.rs:16`), `mcp-service-catalog.v2.json` (catalogue), `models/provider-model-catalog.v1.json`, `logs/` (aucun journal de secret dans le cœur, ticket 40 §5).
  - Relevé fait sur ce poste par les noms de clés seuls, sans lire de valeur.
- **`<cwd>/.yusagent/`** : réglages du projet (`crates/pa-core/src/settings/storage.rs:69`) et skills du projet. Aucun identifiant de yusAgent n'y est rangé. C'est un fichier du workspace, écrit librement par le modèle (voir §F.4).
- **Fichier d'URL de login MCP** (`AUTH_URL_FILE_ENV`, `mcp_login.rs:25-28`, `:51`) : vérification sans navigateur des tests seulement.

### A.2 Ce qui doit rester accessible au kernel dans `~/.yusagent`

- `kernel-venv/` (Python du kernel, `crates/pa-core/src/kernel/bootstrap/venv/layout.rs:22`).
- `skills/` (skills globaux).
- `settings.json`, lu par `prime-agent-runtime/src/rlm/harness.py:139` et `factory.py:4084-4086`.
- `persistent-shells/` (`bash.py:153`, `:1558`).
- `machines/` (`factory.py:4813`).
- `sessions/` et `session-artifacts/` (journal de conversation, sorties).
- Le chemin lui-même arrive par `PRIME_AGENT_CODING_AGENT_DIR` (`crates/pa-core/src/session_engine/runtime_wiring.rs:273-276`, liste blanche `crates/pa-core/src/kernel/env_allowlist.rs:66`).
- Sockets : **aucun besoin**. Le kernel parle au worker par ses tubes (`host_request`, `prime-agent-runtime/src/rlm/repl.py:174-187`, tubes ouverts à `startup.rs:170-172`).
  - Seule exception possible : un `yusagent` lancé depuis le kernel, par exemple `prime-agent factory list` cité par `skills/factory/SKILL.md:51`. Non vérifié.

### A.3 Secrets qui passent dans le kernel autrement que par ces fichiers

- Variables nommées exactement dans `kernel.envAllowlist` : c'est le choix de l'utilisateur (ticket 02).
- `headers` littéraux d'une entrée `mcpServers` utilisateur, renvoyés au kernel par `mcp.config` (`crates/pa-core/src/mcp/mod.rs:82`, handler `:686-735`).
- `env` et `headers` littéraux des serveurs MCP **fournis par le client ACP** (`AcpMcpServerConfig`, `mod.rs:208-224`, renvoyés par `mcp.config`).

## B. Processus

- **Démon (superviseur)** : `yusagent` en mode démon (`crates/pa-cli/src/daemon_mode.rs:18`). Il est lancé par le pont ACP (processus lancé par VS Code) ou par le TUI s'il ne tourne pas.
- **Worker** : le superviseur relance son propre exécutable (`current_exe()`) avec l'argument `worker` (`crates/pa-daemon/src/supervisor/supervision.rs:573`, `:583-584`, spawn `:598-600`). L'aiguillage est à `crates/pa-cli/src/lib.rs:183-187`.
  - L'environnement du worker est celui du superviseur, plus l'environnement de lancement du client, moins `RLM_DEPTH` (`supervision.rs:62-72`).
  - `supervision.rs` est upstream et absent de `docs/fork-changes.md`.
- **Kernel** : le worker le lance dans `ReplKernelManager::do_start` par `tokio::process::Command::new(&python)` avec `-m rlm.repl`, sur tubes (`crates/pa-core/src/kernel/manager/startup.rs:168-172`), avec `env_clear().envs(env)` (`:180`) et spawn à `:181`.
  - **C'est le point d'héritage unique** : `bash()`, `subprocess`, shells persistants, serveurs MCP stdio et `yusagent` lancés par le modèle descendent tous de ce processus.
  - Fichier upstream, déjà modifié par le fork pour YA-02 (`docs/fork-changes.md:32`).
  - Changement estimé : **3 à 5 lignes** à `startup.rs:168-170` (remplacer le programme par l'enveloppe et préfixer les arguments), plus une déclaration de module dans `crates/pa-core/src/kernel/mod.rs` (1 ligne). Le profil se construit dans un module du fork.
  - Le dossier agent et `TMPDIR` se lisent dans la table `env` déjà passée au spawn, sans toucher upstream. Estimation.
- **Ce qui n'est pas descendant du kernel** et lit les secrets légitimement : le démon, le worker, le pont, le TUI. Seuls ces processus doivent garder l'accès (exigence 2).
- **Ce qui n'est pas descendant du kernel mais exécute des commandes** : le shell de `ExecuteBash` (`crates/pa-daemon/src/user_bash.rs:580-593`, worker), lancé par le `!commande` du TUI (`crates/pa-tui/src/session_ui/bash.rs`), et les `!commande` de `auth.json`/`models.json` (worker). Voir §D et §F.

## C. MCP : où vivent les jetons pendant un appel depuis le REPL

- **Le kernel fait l'appel HTTP lui-même.** Le worker sert seulement la configuration (`mcp.config`, `crates/pa-core/src/mcp/mod.rs:686-735`) et le rafraîchissement OAuth (`mcp.refresh`, `:645-682`, qui répond `{}` à `:680` : le jeton ne passe pas par le canal).
- **Client générique** :
  - `_read_auth` lit `<agent_dir>/auth.json` (`prime-agent-runtime/src/rlm/mcp.py:1085-1094`) ;
  - `_bound_auth("mcp:<serveur>")` ne rend le jeton que si son `endpoint` est égal à l'URL de l'entrée (`:893-904`) ;
  - `_headers` pose `Authorization: Bearer <jeton>` (`:956-978`, `:967`, `:975`) ;
  - le client httpx2 part avec ces en-têtes (`:292-299`).
- **Intégrations `mcp_base.py`** : même lecture (`:68-77`), jeton (`:142-165`), en-tête (`:217`).
- **Serveurs stdio** lancés depuis le kernel : `_stdio_env` (`mcp.py:981-998`) ne transmet aucun jeton d'`auth.json`. Il transmet les `env` littéraux d'un serveur ACP (`:986-990`).
- **Bilan** : les jetons MCP en clair sont dans la mémoire du kernel pendant l'appel, et lisibles sur disque. Avec la seule protection de fichier, le client MCP du kernel cesse de fonctionner (`_read_auth` rend `None`).

**Comment les sortir** (estimations, rien de mesuré) :

- **C1. Relais par `host_request` (recommandé).**
  - Côté kernel : un transport httpx2 du fork envoie chaque requête HTTP au worker sous la forme `{serveur, méthode, chemin, en-têtes, corps}`, sans `Authorization`.
  - Côté worker : il retrouve l'URL dans la configuration, applique la même règle d'`endpoint` lié que `_bound_auth`, ajoute l'en-tête, fait l'appel (`reqwest`, déjà dans `crates/pa-core/Cargo.toml:38`) et renvoie la réponse.
  - Upstream touché : `mcp.py` et `mcp_base.py` (construction des en-têtes et du client, de l'ordre de 20 à 40 lignes) et l'enregistrement du handler dans `mcp/mod.rs` (2 lignes).
  - Nouveaux modules : un transport Python, un handler Rust.
  - **Point dur** : les réponses SSE du transport « streamable HTTP » sont longues ou continues. Il faudra soit les relayer par morceaux, soit limiter le relais aux réponses complètes et le dire.
- **C2. Mandataire HTTP local dans le worker.**
  - Le kernel reçoit une URL `127.0.0.1:<port>` par serveur, plus un nonce. Le mandataire ajoute le jeton et relaie les octets, ce qui gère SSE sans cas particulier.
  - Il faut un serveur HTTP dans le worker. `hyper` 1.11 est dans `Cargo.lock` par `reqwest` ; la fonction serveur n'est pas vérifiée.
- Dans les deux cas :
  - le relais ne sert qu'aux serveurs de la configuration, jamais à une URL donnée par le kernel ;
  - la règle d'`endpoint` lié reste obligatoire, parce que le kernel peut réécrire `settings.json` (§F.4) ;
  - `mcp.config` cesse de renvoyer des `headers` littéraux au kernel.
- **Exigence 4 et serveurs stdio ACP** : leurs `env` littéraux restent dans le kernel tant qu'il lance ces serveurs (question 4).

## D. Socket du démon

- **Chemin et droits** : `$TMPDIR/yusagent-<uid>/daemon.sock`, repli `/tmp/yusagent-<uid>` si le chemin est trop long (`crates/pa-daemon/src/platform/paths.rs:20-32`, `:47-48`). Les sockets des workers sont dans le même dossier (`:61-67`).
  - Dossier en 0700 (`crates/pa-daemon/src/paths.rs:82`), socket en 0600 (`crates/pa-daemon/src/socket.rs:227-229`).
  - Observé : `/var/folders/…/T/yusagent-501` en `drwx------`, `daemon.sock` en `srw-------`.
- **Aucune authentification du client** : `serve_client` envoie le hello puis lit les commandes (`crates/pa-daemon/src/supervisor/clients.rs:101-180`). Aucun `peer_cred` ou `getpeereid` dans `crates/` (recherche).
  - Le jeton worker (`PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN`) ne garde que `WorkerRegister` (`crates/pa-types/src/daemon/command.rs:1300-1310`).
  - Le kernel ne reçoit ni le chemin ni le jeton (`env_allowlist.rs:130`), mais `TMPDIR` passe (`env_allowlist.rs:54`) et le chemin est déterministe : **le kernel peut se connecter** (déduit, non testé sur le vrai démon).
- **Commandes qui touchent aux identifiants, et ce qu'elles renvoient** :
  - `GetMcpConnections` (`command.rs:731`, `crates/pa-daemon/src/mcp_connections.rs:28-87`) : booléens et libellés (`McpConnectionEntry` `crates/pa-core/src/mcp/mod.rs:995-1011`, `McpCredentialView` `catalog_plugin_views.rs:491-498` avec `configured: bool`). **Aucun secret.**
  - `SetMcpStaticToken` (`command.rs:740-748`) : jeton en entrée seulement. Réponse `server, endpoint, verified, toolCount, error` (`mcp_connections.rs:137-143`).
  - `RemoveMcpConnection` (`command.rs:751`) : `{server, removed}` (`mcp_connections.rs:179`). Suppression possible, pas de lecture.
  - `GetModelCatalog`, `GetAvailableModels` : `Model.headers` vaut `None` pour `models.json` (`custom.rs:452`). Les clés restent dans `provider_request_configs` (`registry.rs:482-512`). Rien en clair. Les en-têtes statiques des modèles intégrés ne sont pas vérifiés.
  - `Reload` : rien (`session_custom.rs:347`).
  - **`ExecuteBash` / `ExecuteBashAndWait`** (`command.rs:513`, `:1039`, handler worker `crates/pa-daemon/src/worker/commands.rs:127-128`) : le worker lance un shell (`user_bash.rs:586-593`). Il a l'environnement complet du worker et **n'est pas un descendant du kernel**.
    - Un kernel qui joint le socket peut donc lancer `cat ~/.yusagent/auth.json` hors de toute protection. C'est le contournement principal d'une protection posée sur le kernel seul.
- **Pont ACP** : il parle à VS Code sur stdio, le kernel ne le joint pas.
  - Le champ `token` de `_yus/mcp/list` est un libellé, `format!("{label} token")` (`crates/pa-daemon/src/acp/ya_resources.rs:458`, test `:606`).
  - `_yus/providers/list` : `id, name, method, login, connected, removable, note` (`crates/pa-daemon/src/acp/daemon/ya_providers.rs:401-411`).
  - Les échecs de login renvoient des textes fixes (`crates/pa-cli/src/print_runtime/ya_acp_providers.rs:387-394`).
- **`host_request` joignables par le kernel** : `mcp.*`, `model.info`, `rlm.find_models`, `rlm.*`, `compact.*`, `agent_*`. Aucun ne rend une clé. Exception : `mcp.config` rend les `headers`/`env` littéraux du §A.3, à corriger avec C1.
- **Conséquence pour l'exigence 3** : aucune méthode trouvée ne renvoie un secret en clair, sauf `mcp.config` (§A.3). `ExecuteBash` est un exécuteur hors protection : il faut **couper le kernel du dossier des sockets**, ce qui est mesuré possible sur macOS (§E.1), ou protéger aussi ce shell.

## E. Mécanismes par plateforme

### E.1 macOS : Seatbelt (`sandbox-exec`), mesuré

Profil essayé (annexe, 0 écart, macOS 27.0.1) :

```
(version 1)(allow default)
(deny file-read* file-write* (regex #"^<agent>/auth\.json") (regex #"^<agent>/models\.json"))
(deny file-write-unlink (literal "<agent>") (literal "<parent de agent>"))
(deny network-outbound (remote unix-socket (subpath "<dossier des sockets>")))
(deny file-write* (subpath "<dossier des sockets>"))
```

- **Refus ciblé, oui.** Refusés :
  - `cat`, `open()`, symlink, lien physique créé dans la sandbox, variantes de casse, `..`, firmlink `/System/Volumes/Data`, accès par inode `/.vol/<dev>/<ino>` ;
  - le temporaire `auth.json.tmp<pid>` pendant l'écriture (lecture et lien), le verrou `auth.json.lock`, et un `models.json` remplacé par renommage ;
  - le renommage du dossier agent et de son parent.
- **Restent permis** : le reste du dossier agent (lecture, écriture, création), le réseau, `git`. La recherche lancée pendant le constat a aussi vu un `curl` réussir (réponse 200).
- **Pourquoi une regex de préfixe et pas un `literal`** : en `literal`, le temporaire du démon peut être lié physiquement pendant la fenêtre d'écriture, et l'enfant garde le secret (essai « literal », permis). La regex couvre `auth.json*`.
- **Pourquoi la règle sur les ancêtres** : sans elle, `mv` du dossier agent puis `cat` réussit (essai « sans règle sur les ancêtres », permis). Codex fait de même (`codex-rs/sandboxing/src/seatbelt.rs:589-596`, commit `ad7176f7878c`), ainsi que sandbox-runtime (`src/sandbox/macos-sandbox-utils.ts:822-856`, commit `0bbebed0aa9a`).
  - `/Users` appartient à root en 755, donc `$HOME` ne peut pas être renommé.
- **Héritage** : `setsid` suivi d'un `sh -c` est toujours refusé. Une sandbox imbriquée échoue (`sandbox_apply`, code 71).
  - `sandbox.h:25-26` du SDK 26.5 : « If the process is already in a sandbox, the new profile is ignored and sandbox_init() returns an error ».
- **Dépendances** : `/usr/bin/sandbox-exec`, présent sur macOS 27.0.1. Il est marqué « DEPRECATED » dans sa page de manuel, et `sandbox.h:7` le dit « deprecated and may be removed ».
  - Codex (`seatbelt.rs:105`) et sandbox-runtime (`macos-sandbox-utils.ts:1596`) l'utilisent tous deux. Pas de root, pas de nouvelle dépendance.
- **Ce qui casse** :
  - **Binaires setuid** : `ps`, `top`, `sudo`, `su`, `login`, `crontab`, `at`, `traceroute`, entre autres. Refusés même avec `(allow default)` seul.
    - `bash.py` s'en sert pour le shell persistant sur macOS (`/bin/ps`, `prime-agent-runtime/src/rlm/bash.py:1610`, `:1783`). Le banc le relevait déjà (`bench/tools/capture/run.py:452-454`).
    - **Mesuré** : `(allow process-exec (literal "/bin/ps") (with no-sandbox))` rend `ps` (et `top`) utilisables. Le processus `ps` tourne alors hors sandbox ; il ne lit pas de fichier, et `KERN_PROCARGS2` expose déjà l'environnement (§G). À ne pas étendre à `sudo` ou `crontab`, qui donneraient un shell hors sandbox.
  - **Sandbox imbriquée** :
    - le banc de capture lance le binaire sous `sandbox-exec` (`bench/tools/capture/sandbox.sb`, par exemple `acp_frames.py:91`), donc le kernel enveloppé échouerait à démarrer sous le banc (question 7) ;
    - un `yusagent` lancé depuis le kernel ne pourra pas protéger son propre kernel, qui échouera à démarrer (le refus est sûr).
  - **Chemins canoniques** : Seatbelt compare le chemin résolu. `TMPDIR` vaut `/var/folders/…`, lien vers `/private/var/…`, et le dossier agent peut être un lien. Le profil doit utiliser les chemins canoniques (le script de mesure les résout).
- **CI** : sandbox-runtime fait tourner sa suite sur `macos-15` (`.github/workflows/integration-tests.yml:29-34`). Chez nous, le seul job macOS est le build (`.github/workflows/ya-build-macos-arm64.yml:41`, `macos-15`). `ci.yml` est entièrement sous Ubuntu.

### E.2 macOS : trousseau, mesuré par la recherche

- Un élément créé par `security add-generic-password` porte l'ACL `/usr/bin/security`, et `security find-generic-password -w` le relit **sans invite**, depuis un `sh` enfant comme depuis une sandbox Seatbelt (trousseau jetable, créé puis supprimé).
- Une ACL par binaire ne distingue pas le pont d'un `yusagent` relancé par le kernel (déduit).
- **Ne protège pas contre un enfant du même utilisateur.** Écarté.

### E.3 Linux : bubblewrap ou espace de noms de montage, documenté et non mesuré

- **Refus ciblé, par masquage** : tmpfs sur un dossier, ou `/dev/null` lié sur un fichier.
  - Codex : `--perms 000 --tmpfs <dossier>`, et un bind vide pour un fichier (`codex-rs/linux-sandbox/src/bwrap.rs:1245-1281`).
  - sandbox-runtime : tmpfs et `--ro-bind /dev/null` (`linux-sandbox-utils.ts:2006`, `:3275`).
- **Masquer un fichier ne tient pas face à l'écriture atomique.** mount_namespaces(7) : un point de montage peut être renommé dans un espace de noms où il n'en est pas un, et il est alors « removed in the mount namespace where it was a mount point ».
  - Après un renommage par le démon (`settings/storage.rs:304`), le kernel verrait le nouveau `auth.json`.
  - **Il faut masquer un dossier**, donc déplacer `auth.json` (et `models.json`, s'il est dans le périmètre) dans un dossier à part.
  - Coût : `auth.json` est nommé dans une dizaine de sites hors tests, dont le chemin unique d'`AuthStorage` (`manager.rs:275`), plus les lectures Python, qui disparaissent avec C1. `models.json` en a une trentaine (estimation par `grep`). Il faut aussi une migration du fichier existant.
  - Un fichier ajouté plus tard dans un dossier masqué reste caché (README de sandbox-runtime, ligne 564).
- **Environnement et mémoire du parent** : avec `--unshare-pid --proc /proc`, les processus hors sandbox ne sont pas visibles (`bwrap.rs:284-291`), ce qui règle G sous Linux.
  - Sans espace de noms PID, `/proc/<pid>/environ` est soumis à `PTRACE_MODE_READ_FSCREDS` (proc_pid_environ(5)), que Yama ne restreint pas : il serait lisible.
  - `/proc/<pid>/mem` exige ATTACH (proc_pid_mem(5)), donc il est bloqué par Yama 1 pour un ancêtre.
- **Dépendances** : user namespaces non privilégiés.
  - Ubuntu 24.04 les restreint par `kernel.apparmor_restrict_unprivileged_userns=1`. La CI de sandbox-runtime le lève par `sudo sysctl -w …=0` (`integration-tests.yml:406-411`).
  - La valeur sur les runners GitHub n'est pas tranchée (actions/runner-images#10443), WSL2 et Docker ne sont pas vérifiés. bwrap est un exécutable système, hors du cœur.
  - Variante sans bwrap : `unshare(CLONE_NEWUSER|CLONE_NEWNS)` et montage du tmpfs dans un `pre_exec` du fork. Mêmes besoins, pas de binaire externe, plus de code (estimation).
- **Socket du démon** : un tmpfs sur `/tmp/yusagent-<uid>` (et sur le dossier sous `TMPDIR`) le cache au kernel.

### E.4 Linux : Landlock, documenté

- C'est une liste d'autorisations (« grants access … if at least one of its rules … grants the access », docs.kernel.org/userspace-api/landlock.html).
- Refuser `~/.yusagent/auth.json` demande d'autoriser chaque frère à chaque niveau, et un fichier créé plus tard hors des règles serait refusé (déduit de la doc).
- **Ne permet pas un refus ciblé sans restreindre le reste.** Écarté. Codex ne le garde qu'en mode « legacy » (`codex-rs/linux-sandbox/src/linux_run_main.rs:467-473`).

### E.5 Autres, écartés

- seccomp user notify : « can not be used to implement a security policy » (seccomp_unotify(2)).
- fanotify en mode permission : demande CAP_SYS_ADMIN (fanotify_init(2)).
- Fichiers appartenant à un autre utilisateur : demande une installation root et casse le « même utilisateur ».

## F. Contournements, par option

Légende : **M** = mesuré (annexe ou recherche), **D** = déduit de la doc ou du code, non mesuré.

| Contournement | Seatbelt (E.1) | bwrap, dossier masqué (E.3) |
|---|---|---|
| Lien symbolique | refusé (M) | résolu avant masquage (D, `linux-sandbox-utils.ts:157-190`) |
| Lien physique créé depuis la sandbox | refusé (M) | dossier invisible, rien à lier (D) |
| Lien physique créé **avant** la protection, hors du dossier | **lisible** (M) | **lisible** (D) |
| Renommage du dossier agent ou d'un parent | refusé avec la règle sur les ancêtres (M) | sans effet sur le masquage (D) |
| Fichier temporaire du démon pendant l'écriture | refusé avec la regex (M) ; lisible en `literal` (M) | dans le dossier masqué (D) |
| Fichier remplacé par renommage côté démon | refus maintenu (M) | maintenu si dossier ; **perdu si fichier** (D, mount_namespaces(7)) |
| Copie par un descendant (`cp`, `sh -c`, `setsid`) | refusée (M) | refusée (D) |
| Binaire `yusagent` lancé depuis le kernel | hérite ; son kernel ne démarre pas (sandbox imbriquée, M) ; joint le démon seulement si le socket est ouvert | hérite (D) |
| Socket du démon (`ExecuteBash`) | refusé avec la règle socket (M sur un socket de test) | caché par tmpfs (D) |
| Environnement d'un parent | **lisible** par `KERN_PROCARGS2` (M) ; `ps` refusé (setuid) | caché avec `--unshare-pid` (D) |
| Mémoire d'un parent | `task_for_pid` refusé, kr=5, même hors sandbox, parent signé ad hoc comme `yusagent` (M) | `/proc/<pid>/mem` : ATTACH, Yama (D) |
| Fichiers temporaires d'un login | même dossier, même préfixe (M pour `auth.json.tmp<pid>`) | même dossier (D) |

### F.1 Lien physique créé avant la protection

- Un kernel d'avant ce ticket a pu créer un lien vers `auth.json`. Le lien garde l'ancien inode : il voit le contenu jusqu'à la prochaine écriture atomique, qui crée un nouvel inode.
- Parade proposée : au démarrage, le démon vérifie `st_nlink == 1` sur `auth.json` et réécrit le fichier (nouvel inode) sinon. C'est un module du fork, de l'ordre de 10 lignes (estimation).
- Les jetons déjà lus restent connus jusqu'à leur expiration.

### F.2 Socket du démon

- C'est le contournement principal (§D). La règle socket de E.1 le ferme sur macOS. Il reste à le mesurer sur le vrai démon (test I.3).

### F.3 Commandes `!…` exécutées par le worker

- Le refus en écriture sur `auth.json` et `models.json` empêche le kernel d'y planter une commande (M : « remplacer models.json par renommage », refusé).
- Une clé rangée sous la forme `!commande` (par exemple `!security find-generic-password …`) peut être relancée telle quelle par le kernel : elle n'est pas protégée (§J).

### F.4 Réglages écrits par le kernel et lus par un processus non protégé (lu, non mesuré)

- **`settings.json` (global) et `<cwd>/.yusagent/settings.json` (projet) restent écrivables**, comme le veut l'exigence.
- `ExecuteBash` lit `shellPath` et `shellCommandPrefix` dans les réglages fusionnés, projet compris (`user_bash.rs:243-248`, `SettingsManager::create(&core.cwd, &agent_dir)`). Je n'ai trouvé aucun filtre de portée pour ces clés (`crates/pa-core/src/settings/load.rs:38-40` les liste comme connues).
  - Un modèle qui écrit `shellCommandPrefix` fait donc exécuter son préfixe **dans le worker** au prochain `!commande` tapé dans le TUI (seul émetteur de `ExecuteBash`, `crates/pa-tui/src/session_ui/bash.rs`). Cela demande une action de l'utilisateur dans le TUI ; l'extension ne l'envoie pas.
  - Question 9.
- `mcpServers.<nom>.url` réécrit vers un autre serveur : la règle d'`endpoint` lié (`mcp.py:893-904`) refuse le jeton. Le relais C1 doit la garder.
- Autres clés qui feraient exécuter ou envoyer quelque chose par le worker : inventaire terminé au §O.1 ; `packages`/`npmCommand` se déclenchent sans action de l'utilisateur (§P).

## G. Clés passées par variable d'environnement au démon ou au pont

- **Mesuré** (annexe) :
  - depuis une sandbox Seatbelt, `sysctl(KERN_PROCARGS2)` sur le PID du parent rend son environnement initial (variable factice `YA41_FAUX_SECRET`, « visible ») ;
  - `ps eww` n'échoue que parce que `ps` est setuid ;
  - `(deny process-info* (target others))` n'y change rien, et `(deny process-info*)` tue Python (code -5).
- **Banc de capture**, scénario `psenv` : hors sandbox, sur `beecf3f38`, `ps eww` du parent affiche `ANTHROPIC_API_KEY` et `YA_CAPTURE_FAKE_TOKEN`, avec le code 0 (`bench/tools/capture/runs/ya29-main-beecf3f38/run.json`, champ `psenv`). Commande : `python3 bench/tools/capture/run.py --binary <binaire>` (`run.py:450-478`).
- **Portée réelle** : une clé exportée dans le shell de l'utilisateur est dans l'environnement initial de **tous** les ancêtres (terminal, VS Code, pont, démon, worker). Le kernel la lit chez n'importe lequel d'entre eux. yusAgent ne peut nettoyer que ses propres processus.
- **Dans le périmètre ?** La décision vise « les identifiants de yusAgent ». Une clé fournie par l'environnement est un identifiant que yusAgent utilise, mais qu'il ne range pas.
- **Deux voies, à trancher (question 3)** :
  - **G-a, traité** : au démarrage, le pont, le démon et le worker écrasent en place la valeur des variables d'identifiant puis les retirent. Le relevé T03 montre que l'écrasement en place masque la valeur ; `unsetenv` seul ne suffit pas (`docs/fork-changes.md:760`). Le worker les reçoit alors par un autre canal que l'environnement.
    - Cela touche `supervision.rs:62-72` (upstream) et la lecture des clés d'environnement de `pa-ai` (couche providers, règle 3).
    - **Ne couvre pas les ancêtres** hors yusAgent (VS Code, shell).
  - **G-b, limite documentée (recommandé)** :
    - avertissement : « une clé passée par variable d'environnement reste lisible par le code du modèle ; pour la protéger, connectez le fournisseur par l'écran Providers ou `/login` » ;
    - sous Linux avec `--unshare-pid` (E.3), la limite disparaît pour les processus hors sandbox.
- Le jeton worker (`PRIME_AGENT_INTERNAL_DAEMON_WORKER_TOKEN`) est lisible de la même façon dans l'environnement du worker. Il ne sert qu'avec le socket, que la règle socket ferme.

## H. Préfixe de prompt

- **Enveloppe du kernel (E.1, E.3)** : ni le prompt système, ni les tools, ni `yaRequest` ne changent. Le modèle ne voit rien de plus.
- **Textes vus par le modèle qui peuvent changer**, seulement en cas d'échec et dans le résultat d'une cellule :
  - message `PermissionError` ou `Operation not permitted` quand le modèle tente de lire un fichier protégé ;
  - avec C1, messages d'erreur MCP (`_credentials_unavailable`, `mcp.py:965`) si le relais échoue ;
  - websearch sans clé (question 2) ;
  - `ps`/`top` refusés si l'exception de E.1 n'est pas posée.
- Aucun SKILL.md ni texte de prompt ne nomme `auth.json` (recherche dans `skills/*/SKILL.md` et `crates/pa-core/src/prompts/`).
- Si un avertissement doit être montré au modèle (par exemple « fichiers d'identifiants inaccessibles »), il changerait le préfixe. **Rien de tel n'est proposé.**

## I. Tests

Chaque test est vu rouge sur son assertion avant le code (au push 1), puis vert. Les secrets sont des valeurs factices et sentinelles, jamais de vraie clé (règle 9).

| # | Test | Rouge attendu avant le code | Contrôle positif | Où |
|---|---|---|---|---|
| 1 | Génération du profil : regex échappées, chemins canoniques (`/private/var`), dossiers de sockets sous `TMPDIR` et `/tmp`, ancêtres | le module n'existe pas : test absent, donc écrit au push 1 contre une fonction vide | sans objet | CI Linux (texte du profil, sans exécution) |
| 2 | Kernel réel (`ReplKernelManager`) avec un dossier agent factice : `open(auth.json)`, `await bash("cat …/auth.json")`, `cat models.json`, `cat auth.json.tmp<n>`, symlink, `mv` du dossier, connexion à un socket placé dans le dossier des sockets. Chacun doit échouer | chaque cellule lit la sentinelle | même cellule avec la protection coupée : la sentinelle sort. Le processus de test (côté worker) lit `auth.json` | CI macOS (nouveau job `macos-15`), local |
| 3 | Vrai binaire, pont ACP : `_yus/providers/set_api_key` (clé factice), puis un prompt vers le serveur de capture (fournisseur factice), qui reçoit la clé dans son en-tête. Dans la même session, une cellule `open(auth.json)` échoue, et une connexion du kernel à `daemon.sock` échoue | la cellule lit `auth.json` | l'en-tête reçu par le serveur de capture prouve que le pont et le worker lisent toujours | local (banc de capture, nouveau script, règle 13) ; CI macOS si le banc y tourne |
| 4 | MCP (C1) : serveur MCP HTTP factice qui exige `Bearer <sentinelle>`. Avec la protection, un appel `mcp` depuis le kernel réussit. Aucune réponse de `host_request` ne contient la sentinelle | l'appel échoue (le kernel ne lit plus `auth.json`) | le même appel, protection coupée, réussit aussi | CI Linux (sans protection, relais seul) et macOS |
| 5 | Balayage des commandes du démon et des méthodes du pont qui touchent aux identifiants (§D), sur un `auth.json` de sentinelles : aucune réponse sérialisée ne contient une sentinelle | vert dès le départ (aucune trouvée) : sert de garde-fou, à signaler comme tel | sans objet | CI Linux |
| 6 | Linux (PR C) : le test 2 sous bwrap ou espace de noms, plus `/proc/<ppid>/environ` | lecture réussie | protection coupée | CI `ubuntu-24.04` avec l'étape sysctl |
| 7 | `st_nlink` (F.1) : un lien créé avant ; après démarrage du démon, le lien ne voit plus la sentinelle suivante | le lien voit la nouvelle valeur | sans objet | CI Linux et macOS |

- **Shell persistant** : la suite existante de `prime-agent-runtime` doit passer sous la protection sur macOS (exception `ps` de E.1). Un échec sans l'exception prouve le besoin.
- **Banc de capture** : à adapter selon la question 7 (sandbox imbriquée).

## J. Ce qui reste ouvert après le ticket (pour l'avertissement de « yusAgent expliqué »)

Ce document n'existe pas encore dans le dépôt (recherche dans `docs/`) ; la liste est prête pour lui.

- **Portée de la garantie** : la protection empêche la **lecture directe** des identifiants depuis la session (le kernel et ce qu'il lance). Elle ne rend pas yusAgent résistant à une attaque préparée (voir « exécution différée »).
- **Exécution différée** (classe ajoutée le 26/11/2026, confirmée, §O.2) : le kernel peut écrire un fichier qu'un processus non protégé exécutera plus tard, et ce processus lira les identifiants. Exemples :
  - `~/.zshrc` et autres fichiers de démarrage du shell ;
  - hooks git d'un dépôt ;
  - réglages utilisateur de VS Code, dont `yusagent.binaryPath`, ou un `yusagent` placé plus tôt dans le `PATH` ;
  - `~/Library/LaunchAgents` ;
  - scripts du projet lancés ensuite par l'utilisateur ;
  - `shellCommandPrefix`/`shellPath` exécutés par le `!commande` du TUI (réponse 9) ;
  - `npmCommand` exécuté par une installation de paquet lancée par l'utilisateur (selon le §P).
- **Exécution différée hors de yusAgent** (réponse du 26/11, question 14) : VS Code et `yusagent.binaryPath` de ses réglages utilisateur (ou un `yusagent` placé plus tôt dans le `PATH`), les fichiers de démarrage du shell, launchd (`~/Library/LaunchAgents`). Le kernel peut les écrire. Ils s'exécutent plus tard, hors de toute protection, au prochain lancement de VS Code, du shell ou de la session.
- **Services locaux hors de yusAgent** (limite documentée, réponse 19) : un service lancé hors de yusAgent qui exécute une commande ou lit un fichier pour un client peut lire les identifiants à la demande du kernel. C'est immédiat, sans action de l'utilisateur, dès qu'un tel service tourne.
  - **Docker** en premier : `docker run -v ~/.yusagent:/x … cat /x/auth.json` monte le dossier agent dans un conteneur, et le démon Docker le lit pour le client. Déduit, non mesuré.
  - `screen` (mesuré, §R.2), tmux, IPC de VS Code, serveurs TCP locaux, autres (déduits).
- Clés passées par variable d'environnement : lisibles par le code du modèle sur macOS, et sous Linux sans espace de noms PID. Texte de l'avertissement (réponse 3) : « une clé passée par variable d'environnement reste lisible par le code du modèle ; pour la protéger, connectez le fournisseur par l'écran Providers ou `/login` ».
- Clés rangées sous forme `!commande` (trousseau, gestionnaire de mots de passe) : le kernel peut relancer la même commande.
- Identifiants de l'utilisateur hors de yusAgent : `~/.aws`, identifiants Google par défaut (`GOOGLE_APPLICATION_CREDENTIALS`, `env_allowlist.rs:128`), `~/.ssh`, identifiants git, trousseau via `/usr/bin/security`.
- Clé Serper ambiante : `SERPER_API_KEY` n'est jamais transmise au kernel (réponse 13), mais elle reste lisible dans l'environnement initial du worker, comme toute clé passée par variable d'environnement (G-b).
- Smoke bench : le modèle peut lire l'`auth.json` du banc, avec le login des runs à vrai modèle (réponse 12, `bench/smoke/README.md`).
- Secrets fournis par le client : `env` des serveurs MCP stdio fournis par le client ACP, qui ne sont pas des identifiants rangés par yusAgent (réponse 4). Variables que l'utilisateur nomme dans `kernel.envAllowlist`.
- Lien physique créé avant la protection : jetons déjà lus valides jusqu'à expiration (F.1).
- Le modèle peut toujours **utiliser** les identifiants par les voies prévues (appels MCP et websearch par le relais), sans les lire.
- Rien n'empêche le kernel de supprimer des sessions, d'arrêter le démon ou d'envoyer des données par le réseau : hors de l'objectif.
- Sur macOS, dépendance à `sandbox-exec`, déprécié mais présent.
- Plateforme sans mécanisme (userns refusés, `sandbox-exec` absent) : le kernel ne démarre pas, sauf si la protection est coupée par la variable d'environnement (réponses 7 et 8) ; elle est alors entièrement absente.
- Banc de capture et smoke bench : la protection y est coupée (réponse 7, §O.3).

## K. Options comparées

| Option | Tient les exigences 1 à 5 | Upstream touché (estimation) | Casse | CI |
|---|---|---|---|---|
| **K1. Seatbelt avec regex en place et règle socket (macOS)** | oui sur macOS, avec C1 pour l'exigence 4 | `startup.rs` 3 à 5 lignes, `kernel/mod.rs` 1 ligne | setuid (`sudo`…) ; `ps`/`top` gardés par exception ; sandbox imbriquée (banc) | macOS : nouveau job |
| K2. Secrets déplacés dans un dossier, refus `subpath` (macOS) | oui | K1, plus une dizaine de sites `auth.json`, une migration, et une trentaine de sites si `models.json` suit | comme K1 | macOS |
| **K3. bwrap ou espace de noms, dossier masqué (Linux)** | oui, et règle G sous Linux | `startup.rs` (même point que K1), plus le déplacement de `auth.json` (comme K2) | dossier masqué vide ; userns requis | Ubuntu 24.04, avec sysctl |
| K4. Landlock (Linux) | non : pas de refus ciblé | — | nouveaux fichiers de `$HOME` | — |
| K5. Trousseau | non : même utilisateur | — | — | — |
| **C1. Relais MCP par `host_request`** | exigence 4 | `mcp.py` et `mcp_base.py` 20 à 40 lignes, `mcp/mod.rs` 2 lignes | SSE long à traiter | Linux et macOS |
| C2. Mandataire HTTP local | exigence 4 | comme C1 côté Python | serveur HTTP dans le worker | Linux et macOS |

**Recommandation (la plus simple qui tient les exigences)** :

- K1 sur macOS, avec C1 pour les jetons MCP.
- K3 sous Linux, après une sonde CI sur les user namespaces : bwrap, ou espace de noms natif si bwrap manque sur les cibles.
- Le déplacement d'`auth.json` exigé par K3 sert aussi à macOS (on peut alors passer à un `subpath`), mais K1 n'en a pas besoin.

## L. Questions pour Yusuf

1. **`models.json`** : dans le périmètre (refus en lecture et en écriture) ?
   - Recommandé : oui. Sur macOS c'est gratuit (K1). Sous Linux, il faut soit le déplacer (une trentaine de sites upstream), soit le documenter comme limite.
   - Le refus en écriture compte plus que le refus en lecture, à cause des `!commande` (§F.3).
2. **Skill websearch** (clé Serper lue dans `auth.json` par le kernel, `websearch.py:46`) :
   - (a) relais par le worker, comme C1 ;
   - (b) clé seulement par `kernel.envAllowlist`, choix explicite de l'utilisateur, lisible par le modèle et documenté ;
   - (c) skill coupé tant que la clé n'est que dans `auth.json`.
   - Recommandé : (b), la lecture d'`auth.json` étant retirée du skill.
3. **Clés en variables d'environnement (G)** : G-b, limite documentée (recommandé), ou G-a ?
4. **Serveurs MCP fournis par le client ACP** :
   - leurs `headers` HTTP passent par le relais C1, ce qui est gratuit ;
   - leurs `env` stdio restent dans le kernel tant qu'il lance ces serveurs. Les sortir demande que le worker lance ces serveurs. Dans le périmètre, ou limite ?
5. **Linux pour la gate** : la remise aux testeurs attend-elle la PR C (Linux et WSL), ou macOS suffit-il pour une première remise ?
6. **Mécanisme Linux** : bwrap (exécutable système, comme Codex et sandbox-runtime) ou espace de noms natif dans un module du fork ? À décider après la sonde CI. Et accord sur le déplacement d'`auth.json` dans un dossier à part (nom proposé : `~/.yusagent/credentials/`) ?
7. **Réglage et banc** :
   - un réglage global `kernel.secretGuard` (règle 5, variante en réglage), vrai par défaut, pour couper la protection ?
   - pour le banc de capture qui lance le binaire sous `sandbox-exec`, au choix :
     - le banc cesse d'envelopper le démon et garde `PI_OFFLINE=1` et le fournisseur factice comme seules barrières réseau ;
     - ou le banc coupe la protection par le réglage, mais il ne mesure alors plus le kernel protégé ;
     - ou les règles réseau du banc vont dans le profil du kernel.
8. **Mécanisme absent** (`sandbox-exec` manquant, userns refusés) : refuser de lancer le kernel, avec un message clair, ou le lancer avec un avertissement persistant ? La gate demande « un avertissement clair sur ce qui reste ouvert ».
9. **`ExecuteBash` et `shellCommandPrefix`/`shellPath` du projet (F.4)** : à traiter ici, en lançant ce shell sous la même protection ou en lisant ces clés dans les réglages globaux seulement, ou en limite documentée ?
10. **Exception `ps`/`top`** (`with no-sandbox`) : d'accord ? Sinon le shell persistant de `bash.py` sur macOS est à réécrire sans `/bin/ps` (`libproc` par `ctypes`, de l'ordre de 20 à 30 lignes dans un fichier upstream, estimation).

## M. Plan de PR (ordre validé le 26/11/2026 : B, puis A, puis C après la sonde ; en attente du §P)

Chaque PR a son push 1 de tests seuls, rouges sur leur assertion, puis le code. Commits `YA-11: ticket 41 : …`. Tout fichier upstream touché va dans `docs/fork-changes.md`.

- **PR B : jetons MCP et clé Serper hors du kernel (C1, réponse 2)**
  - Push 1 : tests 4 et 5, plus un test websearch : un serveur Serper factice exige la clé sentinelle, et l'appel depuis le kernel passe par le relais.
  - Push 2 :
    - transport Python du fork (requêtes complètes, GET refusé par un 405, §O.4) ;
    - handler Rust du fork qui ajoute l'en-tête avec la règle d'`endpoint` lié ;
    - `mcp.py` et `mcp_base.py` cessent de lire `auth.json` ;
    - `mcp.config` sans `headers` littéraux ;
    - relais `websearch` (URL fixe), lecture d'`auth.json` retirée du skill, injection `SERPER_API_KEY` retirée de l'environnement du kernel (`runtime_wiring.rs:296-298`).
  - Proposition à confirmer (§O.4) : `SERPER_API_KEY` ajoutée aux noms jamais transmis au kernel, même nommée dans `kernel.envAllowlist`.
  - Préfixe : inchangé. Seuls des textes d'erreur dans le résultat d'une cellule peuvent changer, à relever.
- **PR A, macOS : protection des fichiers et du socket (K1)**
  - Push 1 : tests 1, 2, 3 et 7, et le nouveau job CI `macos-15`.
  - Push 2 :
    - module du fork `crates/pa-core/src/kernel/ya_secret_guard.rs` : profil (regex `auth.json*` et `models.json*`, ancêtres, sockets), chemins canoniques ;
    - enveloppe à `startup.rs:168-170`, 3 à 5 lignes upstream ;
    - exception `(allow process-exec (literal "/bin/ps") (literal "/usr/bin/top") (with no-sandbox))`, rien d'autre (réponse 10) ;
    - contrôle `st_nlink` au démarrage du démon.
  - **Interrupteur** (réponse 7) : variable `PRIME_AGENT_SECRET_GUARD=off`, nom proposé selon la convention `PRIME_AGENT_*` du fork.
    - Lue par le superviseur à son lancement et passée aux workers par l'environnement interne, jamais par l'environnement de lancement d'un client (`supervision.rs:62-72`).
    - Jamais transmise au kernel : ajoutée aux noms refusés de `env_allowlist.rs`, même nommée dans `kernel.envAllowlist`.
    - Sans elle, la protection est active.
    - Coupée : avertissement sur stderr du démon et du pont, que l'extension recopie dans l'Output (`extensions/vscode/src/agent.ts:143`).
  - **Mécanisme absent** (réponse 8) : refus de lancer le kernel, avec un message qui nomme la variable.
  - **Tests existants qui lancent un kernel** (réponse 8) : l'assertion de chaque test reste inchangée.
    - Sous Linux jusqu'à la PR C : `PRIME_AGENT_SECRET_GUARD = "off"` dans `[env]` de `.cargo/config.toml`, comme `PRIME_AGENT_PROMPT_VERSION` (`docs/fork-changes.md:260`).
    - Les scripts lancés hors de `cargo` avec un environnement en liste blanche (`scripts/check_*.py`, précédent `docs/fork-changes.md:267`) et l'e2e « vrai binaire » de l'extension (`.github/workflows/ya-extension.yml`) posent la variable eux aussi.
    - Les tests de la protection, sous macOS, la retirent explicitement.
  - **Bancs** (réponse 7) : le banc de capture et le smoke bench posent la variable et gardent leur `sandbox-exec`. La différence avec le produit est notée dans `bench/smoke/README.md` et `bench/tools/capture/README.md`. Le test 3 tourne sans enveloppe externe, protection active.
  - Captures et script de contrôle lancés sur le binaire de la branche (règle 13).
  - Préfixe inchangé.
- **PR C, Linux (K3)**
  - Avant le code : sonde CI (un job qui essaie `bwrap` et `unshare -Ur` sur `ubuntu-24.04`, avec et sans sysctl), et relevé.
  - Puis, selon la réponse 6 : constat de la migration vers `~/.yusagent/credentials/` avec la liste des sites, traitement de `models.json` (réponse 1), masquage du dossier et du dossier des sockets, `--unshare-pid`, test 6.
  - Retrait de `PRIME_AGENT_SECRET_GUARD` de `.cargo/config.toml`.
- **Doc** : avertissement « ce qui reste ouvert » (§J) dans le document que Yusuf indiquera. Gate de YA-16 : macOS après les PR B et A, pour des testeurs sur macOS ; Linux et WSL après la PR C (réponse 5).

## N. Réponses de Yusuf (26/11/2026)

- Deux commits au lieu d'un au départ : accepté. `x5-constat-2026-11-26.md` reste local.
- Notes « Écart (26/11/2026, ticket 41) » ajoutées au point « Usage perso vs publication » de YA-11 et à la ligne des « Limites » du CDC sur les secrets.

Réponses au §L :

1. `models.json` : dans le périmètre, en lecture et en écriture. Le traitement sous Linux se décide à la PR C.
2. websearch : (a), relais par le worker. La lecture d'`auth.json` est retirée du skill, et aucune clé n'est lisible par le modèle par défaut. Arrêt si ce relais coûte nettement plus que C1 : ce n'est pas le cas (§O.4).
3. G-b : limite documentée, avec le texte du §G (repris au §J).
4. `env` des serveurs MCP stdio du client ACP : limite documentée (secrets du client). Leurs `headers` HTTP passent par le relais.
5. Pas de blocage par plateforme : une plateforme sans mécanisme refuse de lancer le kernel. macOS suffit pour des testeurs sur macOS ; Linux et WSL attendent la PR C.
6. Après la sonde CI. Accord de principe sur `~/.yusagent/credentials/` si le masquage par dossier est retenu. La migration et la liste des sites sont à faire au constat de la PR C.
7. Pas de réglage dans `settings.json`, puisque le kernel peut l'écrire. Interrupteur par variable d'environnement lue par le démon à son lancement, jamais transmise au kernel, avec avertissement sur stderr et dans l'Output. Les bancs posent la variable ; le test 3 tourne protection active.
8. Mécanisme absent : refus de lancer le kernel, avec un message qui nomme la variable. Les tests Linux existants posent la variable jusqu'à la PR C, sans changer leurs assertions.
9. `shellCommandPrefix` et `shellPath` : limite documentée, dans la classe « exécution différée ».
10. Exception limitée à `/bin/ps` et `/usr/bin/top` en `literal`.

Plan : ordre B, puis A, puis C après la sonde. Avant le code de la PR B : vérifier qu'un relais à réponses complètes suffit (§O.4).

## O. Compléments au constat (26/11/2026)

### O.1 F.4 terminé : clés de réglages qui font exécuter ou envoyer quelque chose par le worker ou le démon

Liste des clés connues : `crates/pa-core/src/settings/load.rs:10` et suivantes (`KNOWN_FIELDS`). Les réglages globaux et ceux du projet sont fusionnés (`SettingsManager::create`, `settings()` rend `merged`, `crates/pa-core/src/settings/manager.rs:101-103`). Tout a été lu, rien n'a été mesuré.

| Clé | Effet dans un processus non protégé | Déclenchement |
|---|---|---|
| **`packages`** (avec ou sans **`npmCommand`**) | Un paquet `npm:` ou `git:` absent est **installé à la résolution des ressources** (`crates/pa-core/src/packages/resolve/manager.rs:216-229`, `:275-303`). `npm install -g <spec>` ou `git clone` puis `npm install` (`crates/pa-core/src/packages/manager.rs:518-536`, `crates/pa-core/src/packages/git.rs:43-73`) exécutent les scripts du paquet. `npmCommand` remplace le programme lancé par n'importe quel exécutable (`crates/pa-core/src/packages/npm.rs:15-23`). Le tout tourne **dans le worker**. | **Sans action de l'utilisateur** (détail au §P) |
| `shellPath`, `shellCommandPrefix` | Shell de `ExecuteBash` dans le worker (`crates/pa-daemon/src/user_bash.rs:243-248`) | Après une action : `!commande` dans le TUI. Limite documentée (réponse 9) |
| `updateChannel` | Canal de `yusagent update` (`crates/pa-cli/src/self_update.rs:115`, `public_command.rs:334-340`) | Après une action : commande `update` |
| `kernel.envAllowlist` | Ajoute à l'environnement du **kernel** des variables nommées. Les identifiants des fournisseurs ne passent jamais (`env_allowlist.rs:115-131`). La clé Serper rangée passe si `SERPER_API_KEY` est nommée (`runtime_wiring.rs:296-298`). | À la construction du provisionneur, c'est-à-dire à la création d'une session (`runtime_wiring.rs:257`, `:292`, déduit). L'injection Serper disparaît avec la PR B |
| `mcpServers` | Le worker ne lance aucun serveur. `prompt_gating` est une lecture locale (`crates/pa-core/src/mcp/mod.rs:554-577`). Le rafraîchissement OAuth part vers le `tokenEndpoint` rangé dans `auth.json` (`crates/pa-core/src/mcp/login.rs:717`), pas vers une URL des réglages. Une URL réécrite ne reçoit pas le jeton (`endpoint` lié, `mcp.py:893-904`, à garder dans le relais) | Sans objet |
| `agentTraces` | Envoi coupé par le fork, sans interrupteur (`crates/pa-core/src/agent_traces/ya_sharing_off.rs:1-14`) | Sans objet |
| `telemetry` | Champs `enabled`, `noticeShown`, `localMirror` (`crates/pa-core/src/settings/types.rs:253-259`) : aucune URL réglable. Le contenu de l'envoi n'est pas vérifié en détail | Sans objet pour les identifiants (déduit) |
| Choix de modèle (`defaultProvider`, `defaultModel`, `enabledModels`, `providerBackupModel`, `imageModel`, `auxiliaryModel`, `subagentDefaultModel`, `transport`, `codexSessionIdHeader`) | Requêtes au fournisseur avec sa clé, vers l'URL du catalogue intégré ou de `models.json` (protégé). Aucune clé de réglage ne porte d'URL de fournisseur (liste `KNOWN_FIELDS`) | Sans objet (déduit) |
| `skills`, `prompts`, `factory`, `autonomous`, `harnessRefinements`, `autoRefine`, `sessionDir` et clés d'affichage | Fichiers lus comme texte, ou code exécuté **dans le kernel** (skills, machines de la factory). `sessionDir` change seulement le dossier où le démon écrit les sessions | Sans objet (déduit) |

### O.2 Classe « exécution différée » : confirmée

- Le profil est `(allow default)` et ne refuse que les fichiers de secrets et le dossier des sockets (§E.1). Toute autre écriture reste permise.
- **Mesuré** sur un faux HOME (annexe, lignes « exécution différée ») : écrire `~/.zshrc`, un hook git, un `~/Library/LaunchAgents/*.plist` et `~/Library/Application Support/Code/User/settings.json` est permis.
- **`yusagent.binaryPath`** : le réglage est en portée `machine` (`extensions/vscode/package.json:61-65`). Le `.vscode/settings.json` du projet ne peut donc pas le fixer, mais le fichier de réglages utilisateur de VS Code, lui, est écrivable. Le nom nu par défaut est cherché dans le `PATH` (`extensions/vscode/src/binary.ts:1`).
- **LaunchAgents** : la recherche a vu `launchctl submit` échouer depuis la sandbox (code 1, non repris par le script). Le fichier écrit est chargé à la session suivante (déduit).
- La garantie est bien « pas de lecture directe depuis la session » (§J).

### O.3 Smoke bench

- `bench/smoke/run.py` **enveloppe aussi le binaire** dans `sandbox-exec` :
  - `run_binary(..., sandboxed=True)` préfixe `/usr/bin/sandbox-exec` (`bench/smoke/run.py:347-357`) ;
  - la phase agent utilise `sandbox-agent.sb` (`agent_profile`, `:328-343`), la chauffe et la sonde réseau aussi ;
  - le bras Codex utilise le même profil (`:826`).
- **Conséquence** : avec la protection, le kernel ne démarrerait pas sous le smoke bench, à cause de la sandbox imbriquée (code 71, annexe). Le smoke bench pose donc la variable et garde son profil (réponse 7).
  - Ses mesures portent sur un kernel non protégé. Le préfixe est le même ; seuls les refus (setuid, fichiers protégés) diffèrent.
- **Constat en plus** : `sandbox-agent.sb` laisse lire et écrire `AGENT_DIR`, le dossier agent du banc, qui garde le login des runs à vrai modèle. Pendant un run du smoke bench, le modèle peut donc lire l'`auth.json` du banc, aujourd'hui comme après le ticket.
  - Proposition : ajouter à `sandbox-agent.sb` les mêmes refus sur `AGENT_DIR/auth.json*` et `models.json*`, ce qui coûte quelques lignes et garde la parité avec le produit. À trancher (question 12).

### O.4 PR B : vérifications avant le code

- **Un relais à réponses complètes (C1) suffit** (déduit du code, à confirmer par le test 4) :
  - le client MCP du kernel ne connaît que `http` (streamable HTTP) et `stdio` (`prime-agent-runtime/src/rlm/mcp.py:267-273`), pas l'ancien transport SSE ; les intégrations `mcp_base.py` passent aussi par streamable HTTP (`mcp_base.py:203`) ;
  - SDK `mcp` 2.3.0 du venv du kernel : la réponse à un POST est du JSON ou un flux SSE fini, que le serveur ferme après sa réponse (`mcp/client/streamable_http.py:404`, `:438-440`) ; le relais peut le mettre en mémoire puis le rendre en entier ;
  - le flux GET des messages lancés par le serveur est facultatif : sur une erreur, le SDK réessaie au plus deux fois à une seconde d'intervalle puis abandonne (`:59-60`, `:224-273`). Le relais répond 405 au GET ;
  - ce qui est perdu : les messages lancés par le serveur hors d'une requête (notifications, demandes du serveur). Les tools appelés par le modèle n'en ont pas besoin ;
  - un appel long arrive en une fois à la fin. Le délai du relais doit dépasser celui de l'appel (`mcp.py:295`).
  - **Pas de flux SSE long nécessaire : pas d'arrêt, C2 non requis.**
- **Relais websearch : moins cher que C1** :
  - une URL fixe (`https://google.serper.dev/search`), un POST JSON avec `X-API-KEY` (`skills/websearch/src/websearch/websearch.py:119-127`) ;
  - côté worker, la résolution de la clé existe déjà (`websearch_api_key`, `crates/pa-core/src/session_engine/runtime_wiring.rs:332-360`, `!commande` compris) ;
  - **pas d'arrêt.**
- **Point à confirmer** : `SERPER_API_KEY` ambiante. Le skill la préfère aujourd'hui (`websearch.py:41`), et elle n'arrive au kernel que nommée dans `kernel.envAllowlist`, que le kernel peut lui-même écrire (§O.1).
  - Proposition : le relais lit `SERPER_API_KEY` dans l'environnement du worker ou dans `auth.json`, et le nom rejoint les noms jamais transmis au kernel (question 13).

## P. Question bloquante (26/11/2026) : `packages` et `npmCommand` se déclenchent sans action de l'utilisateur

**Chemin, lu et non mesuré :**

- Le modèle écrit dans `settings.json` (global, ou `<cwd>/.yusagent/settings.json`) une entrée `packages` absente, éventuellement avec un `npmCommand`.
- La résolution suivante l'installe dans le worker, hors protection :
  - à chaque tour, y compris **un tour lancé par l'agent** : `prompt_injected_message` (`crates/pa-core/src/session_engine/admission.rs:31-47`), chemin des avis `[bash-done]` (ticket 25), puis `ya_announce_resources` (`crates/pa-core/src/session_engine/ya_resource_notice.rs:377-387`), `load_resources` (`:166`) et `resolve()` (`crates/pa-core/src/resources/resolution.rs:180`) ;
  - à chaque prompt de l'utilisateur (`admission.rs:148`) ;
  - à la création d'une session (`crates/pa-core/src/session_engine/engine.rs:350`) ;
  - à l'ouverture de l'écran skills de l'extension (`crates/pa-daemon/src/acp/ya_resources.rs:178`).
- Seul `PI_OFFLINE=1` coupe l'installation (`crates/pa-core/src/packages/mod.rs:38`, `resolve/manager.rs:281-283`). C'est pourquoi les bancs ne l'ont jamais exercée.
- **Conséquence** : une cellule écrit le réglage, lance un `bash()` en arrière-plan, et l'avis de fin déclenche un tour où le worker exécute la commande, puis lit `auth.json`. **La protection est contournée sans action de l'utilisateur.**
- Le chemin `ya_resource_notice` vient du fork (ticket 31). `engine.rs:350` est upstream.

**Options :**

- **P1. Pas d'installation automatique dans le démon (recommandé).**
  - Les résolutions faites pour une session (création, comparaison par tour, écran skills) passent `MissingSourceAction::Skip` (`resolve/manager.rs:284-289`).
  - Un paquet absent est ignoré, avec une ligne sur stderr. Il s'installe seulement par `yusagent package install`, commande lancée par l'utilisateur.
  - Upstream : `resources/resolution.rs:180`, `manager.resolve()` devient `resolve_with_on_missing(Some(skip))` (2 à 3 lignes, estimation), ou une option passée depuis les trois appelants.
  - Reste : un `npmCommand` planté s'exécute quand l'utilisateur lance une installation. Cela entre dans l'« exécution différée ».
  - À vérifier au constat de la PR : si la résolution met aussi à jour les paquets git (« refreshed », `resources/mod.rs:125`), même traitement.
- **P2. Installations sous la même protection que le kernel.** Les lancements de `npm` et `git` de `packages/` passent par l'enveloppe de la PR A (`manager.rs:518`, `:568`, `git.rs:71`, `:206` et le clone), soit 4 à 6 sites upstream.
  - L'installation automatique est gardée, et les scripts de paquet ne lisent plus les secrets.
  - macOS seulement jusqu'à la PR C. Couvre aussi l'installation lancée par l'utilisateur.
- **P3. Refus d'écriture sur `settings.json`, global et projet.** Contraire à « reste du système libre », et le réglage du projet est un fichier du workspace. Non recommandé.
- **P4. `packages` coupé dans yusAgent**, comme l'envoi des traces (`ya_sharing_off.rs`). Les skills restent dans `~/.yusagent/skills` et dans ceux du projet, et les skills intégrés ne passent pas par `packages` (`bundled_skills_dir`, `resolution.rs:173-178`).
  - Le plus simple si les paquets ne servent pas. Coût du même ordre que P1.

**Recommandation** : P1, ou P4 si tu n'utilises pas les paquets. P2 en plus de P1 si l'installation par l'utilisateur doit aussi être protégée.

**Avant le code** : un test rouge à poser au push 1 de la PR qui traitera ce point. Un `settings.json` porte `packages: ["npm:ya41-absent"]` et un `npmCommand` vers un script marqueur ; un tour lancé par un avis `[bash-done]` doit laisser le marqueur absent.

**Où le traiter** : la PR A (à mettre avant toute remise), ou une PR à part avant la PR A (question 11).

Questions ouvertes :

11. **§P** : P1, P2, P3 ou P4, et dans quelle PR ?
12. **§O.3** : refus des fichiers de secrets dans `sandbox-agent.sb` du smoke bench ?
13. **§O.4** : `SERPER_API_KEY` dans les noms jamais transmis au kernel, le relais la lisant dans l'environnement du worker ?

## Q. Réponses 11 à 13 et nouveau constat bloquant (26/11/2026)

### Q.1 Réponses de Yusuf

11. **P1**, dans une PR à part avant la PR B :
    - push 1 : le test rouge (réglage `packages` écrit depuis une cellule, puis tour suivant ou tour lancé par un avis `[bash-done]` : aucune installation lancée) ;
    - lignes upstream annoncées avant le push 2 ;
    - le `npmCommand` planté rejoint l'« exécution différée ».
    - **Bloqué par le §Q.2.**
12. Refus dans `sandbox-agent.sb` seulement si le binaire du smoke bench peut encore lire son login. **Vérifié : non.**
    - En mode `-p`, la session tourne dans le processus du binaire (`crates/pa-cli/src/print_runtime.rs:1288-1291`, commande `bench/smoke/run.py:391-393`), enveloppé par `sandbox-exec` (`run.py:347-357`).
    - Le processus qui lit le login et celui qui lance le kernel ne font qu'un sous ce profil. Refuser `auth.json` casserait les runs.
    - Le refus n'est donc pas posé. Note ajoutée dans `bench/smoke/README.md` et au §J.
13. Oui :
    - `SERPER_API_KEY` rejoint les noms jamais transmis au kernel, même via `kernel.envAllowlist` ;
    - le relais la lit dans l'environnement du worker ;
    - ajouté au §J (lisible dans l'environnement du worker, G-b).

Choix validés et précisions :

- `PRIME_AGENT_SECRET_GUARD=off` : d'accord.
- `.cargo/config.toml [env]` : d'accord. Cette section vaut aussi sur macOS : **les tests 2 et 3 forcent explicitement la protection active**, en retirant la variable de l'environnement du processus qu'ils lancent.
- La ligne des « Limites » sur le Python arbitraire reste telle quelle.

### Q.2 Constat bloquant : le worker lance, sans action de l'utilisateur, des programmes que le kernel peut changer

Lu dans le code ; deux points mesurés (signal, droits du binaire). Rien n'a été mesuré de bout en bout.

**Q.2.1 P1 ne suffit pas.**
- Pour chaque paquet `npm:` de portée utilisateur, la résolution calcule le dossier d'installation par `global_npm_root()` (`crates/pa-core/src/packages/resolve/manager.rs:355-361`). Celle-ci lance `<npmCommand> root -g` (`crates/pa-core/src/packages/manager.rs:514-520`, `crates/pa-core/src/packages/npm.rs:37-64`) **avant** de savoir si le paquet manque.
- Le cache `global_npm_root` part de `None` à chaque `PackageManager` (`manager.rs:128`) et `global_npm_root(&self)` ne le remplit jamais : la commande tourne à chaque résolution.
- Ignorer les paquets absents (P1) laisse donc un `npmCommand` planté s'exécuter à chaque tour.

**Q.2.2 Autres lancements par le worker ou le démon, hors de la descendance du kernel.** Relevé par `grep` de `Command::new` hors tests dans `pa-core`, `pa-daemon`, `pa-ai` et `pa-cli`.

| Lancement | Programme | Déclenchement | Ce que le kernel peut changer |
|---|---|---|---|
| Contexte git de la session (`crates/pa-core/src/session/manager/git.rs:18-28`, appelé à `AgentStart` et `AgentEnd`, `crates/pa-core/src/session_engine/mod.rs:480-491`) | `git`, **par son nom** (`PATH` du worker) | **Chaque début et fin de tour**, y compris les tours lancés par l'agent | Placer un `git` dans un dossier écrivable du `PATH`. Sur ce poste, `~/.local/bin` vient en premier, et `/opt/homebrew/bin` appartient à l'utilisateur (`drwxrwxr-x`, observé) |
| Paquets (`crates/pa-core/src/packages/process.rs:19`, `:45`) | `npmCommand`, ou `npm` du `PATH` | Chaque résolution (§P, §Q.2.1) | Le réglage, ou le `PATH` |
| Tool `exec_command` de la variante `shellTool: "native"` (`crates/pa-core/src/tools/ya_exec.rs:317`) | Shell lancé par le worker | **Chaque commande du modèle** dans cette variante | Rien à changer : la variante exécute les commandes du modèle hors du kernel, donc hors de toute protection posée sur le kernel |
| Worker (`crates/pa-daemon/src/supervisor/supervision.rs:583`) | `current_exe()`, le binaire `yusagent` | À chaque nouveau worker, et après un plantage (relance déduite de `supervision.rs:86-87`) | Remplacer le binaire : `~/.cache/yusagent-bin/<sha>/yusagent` est en `-rwxr-xr-x` à l'utilisateur (observé). Un processus sous la sandbox peut terminer un autre processus du même utilisateur (mesuré : `kill -TERM` depuis `sandbox-exec`, rc 0, cible terminée) |
| Valeurs `!commande` (`crates/pa-core/src/auth/resolve_config_value.rs:70`) | `bash`, par son nom | À la résolution d'une clé `!…`, seulement si l'utilisateur en a | Le `PATH` |
| Vertex (`crates/pa-ai/src/providers/google_vertex.rs:104`), Bedrock `credential_process` (`crates/pa-ai/src/providers/bedrock/credential_process.rs:131`) | `gcloud` par son nom ; `sh -c` sur la commande lue dans la configuration AWS | À l'obtention d'un identifiant, si l'utilisateur emploie ces fournisseurs | Le `PATH` ; `~/.aws/config` |
| Sonde du venv du kernel (`crates/pa-core/src/kernel/bootstrap/venv/probe.rs:13-14`), shells persistants (`crates/pa-daemon/src/persistent_shells.rs:366`, `:394`, `sh`), portes du mode autonome (`crates/pa-core/src/autonomous/gates.rs:397-403`) | à vérifier | à vérifier | à vérifier |

Après une action de l'utilisateur seulement (classe « exécution différée ») : `ExecuteBash` (TUI), `trash` à la suppression d'une session (`crates/pa-daemon/src/saved_session_commands.rs:117`), le navigateur d'un login, les commandes `update` et `package`.

**Conséquence** : refuser les fichiers de secrets au kernel et à ses descendants ne suffit pas. Le kernel peut faire exécuter son code par le worker, qui n'est pas protégé, au tour même, sans action de l'utilisateur, et ce de plusieurs façons (au minimum `git` du `PATH` et `npmCommand`).

### Q.3 Options

- **X1. Tous les lancements du worker et du démon passent par la même enveloppe que le kernel (recommandé).**
  - Ce que le worker lance ne lit plus les secrets, quel que soit le programme : `git`, `npm`, `exec_command`, `!commande`, `gcloud`, `credential_process`, `sh`.
  - Seuls restent hors enveloppe le kernel (déjà enveloppé) et le worker lui-même.
  - Ajout au profil : refus d'écriture sur le binaire `yusagent` en cours (chemin canonique de `current_exe()`), pour que le kernel ne puisse pas remplacer le worker.
  - Coût :
    - un utilitaire du fork pour envelopper une commande ;
    - de l'ordre de 8 à 12 sites upstream d'une ou deux lignes (estimation, d'après le tableau, à fixer par l'inventaire complet) ;
    - les sites `packages/process.rs` regroupent tous les lancements des paquets.
  - Les `!commande` et `credential_process` continuent de fonctionner : le trousseau et `~/.aws` ne sont pas refusés, seuls les fichiers de secrets de yusAgent le sont.
  - Linux : même règle à la PR C.
- **X2. Chemins absolus et `PATH` réduit aux dossiers système pour les lancements du worker.** Ne couvre ni `npmCommand`, ni `exec_command`, ni `credential_process`, ni le binaire. Casse un `git` installé seulement par Homebrew. Non recommandé.
- **X3. Refuser au kernel l'écriture des dossiers du `PATH` et des fichiers exécutés.** La liste est ouverte (`PATH`, `~/.aws/config`, binaire, configuration git du dépôt…) et contraire à « reste du système libre ». Non recommandé seul. Le refus d'écriture sur le binaire, inclus dans X1, en est le seul cas ciblé.

**Conséquence pour P1 (question 15)** : avec X1, un `npmCommand` planté s'exécute sous l'enveloppe et ne lit plus les secrets. P1 n'est alors plus nécessaire pour l'objectif de sécurité. Il ne servirait plus qu'à éviter des installations non voulues.

Questions :

14. **X1**, avec le refus d'écriture sur le binaire ? Dans la PR A (même mécanisme), ou dans une PR à part avant la PR B, à la place de P1 ?
15. **P1** : abandonné (X1 couvre l'exécution de `npm`), ou gardé sous la forme P1' ? P1' : les résolutions faites pour une session ignorent `packages`, sans `npm root -g` ni installation.
16. **Inventaire complet** des lancements du worker et du démon (lignes « à vérifier » du tableau, autres crates) au constat de la PR qui fera X1, avant son code : d'accord ?

## R. Inventaire des lancements, sorties de la sandbox, comparaison X1 / X4 (26/11/2026)

Réponses de Yusuf du 26/11/2026 : P1 abandonné (question 15) ; inventaire complet avant tout code, au ticket (question 16) ; comparer X1 à X4 avant de choisir (question 14). Rien n'est codé.

### R.1 Inventaire des lancements de processus

- Méthode : recherche de `Command::new`, `process::Command`, `exec`, `fork` et `posix_spawn`, plus les assistants (`packages/process.rs`, `platform/browser.rs`, `resolve_config_value.rs`), dans toutes les crates liées à `pa-cli`, hors modules de test.
- Fait par un agent, puis les lignes qui fondent une conclusion ont été relues : `session/manager/git.rs`, `session_engine/mod.rs:480-491`, `packages/*`, `probe.rs`, `venv.rs`, `catalog_chain.rs`, `launch_env.rs`, `supervision.rs`.
- **Crates tierces** : aucune crate qui lance un processus ou charge du code (`open`, `opener`, `webbrowser`, `keyring`, `libloading`, moteurs WASM, JS ou Lua, `dotenvy`, `pyo3` : absents de `Cargo.lock`).

**Worker** (le mode `-p` exécute les mêmes chemins dans son propre processus) :

| Site | Programme (origine) | Déclencheur |
|---|---|---|
| `crates/pa-core/src/kernel/manager/startup.rs:168` | Python du venv du kernel, chemin absolu (`kernel/bootstrap/venv/layout.rs:22-28`), ou `PRIME_AGENT_KERNEL_PYTHON` | Sans action : démarrage ou redémarrage du kernel |
| `crates/pa-core/src/kernel/bootstrap/venv/probe.rs:14` | Le même Python, `-c "import <nom>"`, **sans `-I`**, dans le dossier de la session | Sans action : au démarrage du kernel. Mémorisée sur disque dans le venv (`probe.rs:158-182`), que le kernel peut écrire |
| `crates/pa-core/src/kernel/bootstrap/venv.rs:82` | `uv` du `PATH`, sinon `~/.local/bin/uv` (`venv/uv.rs:82-95`) : `python install`, `venv`, `pip install`, puis `--editable <skill>` (`venv.rs:214-236`) | Sans action : venv absent ou périmé, skill Python ajoutée ou modifiée |
| `crates/pa-core/src/packages/process.rs:19`, `:45` | `npmCommand`, sinon `npm` du `PATH` ; `git` du `PATH` | Sans action : chaque résolution (§P, §Q.2.1) |
| `crates/pa-core/src/session/manager/git.rs:24` | `git` du `PATH` (`rev-parse`, `branch --show-current`, `remote get-url origin`) | Sans action : début et fin de chaque tour (`session_engine/mod.rs:480-491`), création de session |
| `crates/pa-core/src/autonomous/gates.rs:403` | `/bin/bash -c <gate>` ; `git status`, `git diff` | Sans action : fin de tour en mode autonome. Les gates viennent de la CLI, du payload de création ou des **descripteurs persistés** (`crates/pa-daemon/src/supervisor/worker_lifecycle.rs:280-292`) |
| `crates/pa-core/src/tools/ya_exec.rs:317` | `shellPath`, sinon `/bin/sh` | Sans action : chaque `exec_command` du modèle (variante `shellTool: "native"`) |
| `crates/pa-core/src/auth/resolve_config_value.rs:70` | `bash -c` du `PATH` | Sans action : résolution d'une valeur `!…` (`auth/manager/lookup.rs:54-57`, `models/registry.rs:314`, `:827`, `runtime_wiring.rs:352`) |
| `crates/pa-ai/src/providers/google_vertex.rs:104` ; `crates/pa-ai/src/providers/bedrock/credential_process.rs:131` | `gcloud` du `PATH` ; `sh -c` sur la commande de `~/.aws/config` | Sans action : requête Vertex ou Bedrock |
| `crates/pa-core/src/platform/browser.rs:36` | `open` (macOS), `xdg-open` | Sans action : `mcp.begin_login`, **demandé par le kernel** (`crates/pa-daemon/src/mcp_login.rs:49`). L'URL est forcée en https (`mcp/oauth_discovery.rs:120`) |
| `crates/pa-daemon/src/user_bash.rs:586` | `shellPath`, sinon `/bin/bash -c` | Après action : `!commande` du TUI (n'importe quel client du socket peut l'envoyer) |
| `crates/pa-daemon/src/saved_session_commands.rs:117` | `trash` du `PATH` | Après action : suppression d'une session |
| `ps`, `taskkill`, `which`, `where` (`pa-types/src/platform/process.rs:41`, `:352`, `pa-core/src/platform/process.rs:266`, `platform/shell.rs:118`, `:134`) | Linux, Windows, ou `/bin/bash` absent | Interne ; rien sur macOS |

Les trois lignes « à vérifier » du §Q.2.2 :
- `persistent_shells.rs:366`, `:394` et `boot_reap.rs:809` sont dans des modules de test (`persistent_shells.rs:286-287`, `boot_reap.rs:693-694`) ;
- la sonde et les portes sont dans le tableau ci-dessus ;
- `workspace_snapshot/git.rs:121` n'a aucun appelant.

**Superviseur** (`daemon_mode.rs:8-18`) :

| Site | Programme | Déclencheur |
|---|---|---|
| `crates/pa-daemon/src/supervisor/supervision.rs:583` | `current_exe() worker` | Création par un client ; relance des workers depuis les descripteurs au démarrage (`adoption.rs:109`) ; relance après plantage |
| `crates/pa-core/src/auth/resolve_config_value.rs:70`, via `prime_credentials_for_dir` (`models/catalog_chain.rs:63-65`) | `bash -c` du `PATH`, pour une clé Prime Inference rangée en `!…` | **Sans action** : au démarrage, puis à chaque rafraîchissement horaire du catalogue (`crates/pa-daemon/src/supervisor.rs:328-329`, `:339-340`) |
| `crates/pa-daemon/src/saved_session_commands.rs:117` | `trash` du `PATH` | Commande client de suppression |

- Pas de mise à jour automatique : `SELF_UPDATE=false` (`crates/pa-cli/src/ya_update_off.rs:12`).
- **Environnement du worker** : l'environnement de lancement de n'importe quel client du socket, filtré du seul préfixe `PRIME_AGENT_INTERNAL_` (`crates/pa-types/src/daemon/launch_env.rs:36-44`, `supervision.rs:62-72`). Cela comprend `PATH`, `BASH_ENV` et `DYLD_*`.

**Pont ACP** :

| Site | Programme | Déclencheur |
|---|---|---|
| `crates/pa-cli/src/interactive_mode/daemon.rs:170` | `current_exe --mode daemon` | Sans action : lancement du pont sans démon (`print_runtime.rs:106`) |
| `platform/browser.rs:36` (via `ya_providers.rs:436`, `:615-621`) | `open` | Après action : `login_open` |
| `packages/process.rs` via `acp/ya_resources.rs:178` | `npmCommand`, `npm`, `git` | `_yus/skills/list` : écran Réglages ouvert, ou retour en « ready » pendant qu'il est ouvert (`extensions/vscode/src/settings.ts:43-54`) |
| `resolve_config_value.rs:70` | `bash -c` | Listes de connexion et de modèles (déduit) |

**Code chargé ou interprété sans lancement** :
- aucun `dlopen` ni moteur de script ;
- les `packages` sont lus comme texte, mais leur installation exécute les scripts npm, et les skills Python passent par le build `uv` ;
- les gabarits de prompts sont une simple substitution (`skills/prompt_templates.rs:57-110`) ;
- les `!…` n'existent qu'à `resolve_config_value.rs:14`, `:22` ;
- `bash -c` lit `BASH_ENV` hérité (déduit) ;
- la sonde Python, lancée sans `-I` dans le dossier de la session, peut importer un module du projet portant le nom demandé (sémantique de Python, déduit). Le `site` du venv exécute les lignes `import` des fichiers `.pth` (documentation de `site`).

**Lancements sans action dont le programme ou les arguments viennent de ce que le kernel peut écrire** (lu ou déduit, rien mesuré de bout en bout) :
- `packages`/`npmCommand` ;
- `git` du `PATH` à chaque tour ;
- `uv --editable` d'une skill créée par le kernel ;
- **un `.pth` ou un module de même nom pour la sonde Python** (le kernel écrit dans son venv et dans le projet) ;
- `bash` du `PATH` pour les `!…` (worker, et superviseur toutes les heures) ;
- `credential_process` de `~/.aws/config` ;
- les gates des descripteurs, rejouées par le superviseur ;
- l'environnement de lancement d'un client du socket ;
- `exec_command`.

### R.2 Sorties de la sandbox vers un processus non protégé (mesuré, annexe)

- **LaunchServices** : `open` lance une application créée sous la sandbox. Elle tourne hors sandbox et lit le faux secret, **sans action de l'utilisateur**.
  - `(deny mach-lookup (global-name-prefix "com.apple.coreservices.") (global-name-prefix "com.apple.lsd."))` ferme ce canal (`open` échoue, rien n'est lu), et `git` comme `python3` fonctionnent encore.
  - Ce qui casse : tout `open` depuis la sandbox, y compris l'ouverture d'un navigateur.
- **launchd** : `launchctl submit` et `launchctl bootstrap gui/<uid>` échouent depuis la sandbox.
- **AppleEvents** : non concluant. Un `osascript` vers le Finder sous `(deny appleevent-send)` et le refus LaunchServices est resté bloqué et a été arrêté. Retiré du script.
- **Services locaux** : un `screen` lancé hors sandbox exécute une commande envoyée depuis la sandbox, qui lit le faux secret. C'est toute une classe : tmux, Docker (`docker run -v ~/.yusagent:…`), IPC de VS Code, serveurs en TCP (déduit pour ceux-là).
  - Seul un refus large des connexions de socket Unix la fermerait, et il casserait Docker, ssh-agent, gpg-agent, une base locale et la résolution DNS si elle n'est pas exceptée.
  - Le TCP local reste ouvert tant que le réseau est libre.
  - Question 19.

Ces sorties valent pour K1, X1 et X4 : elles partent de tout processus sous l'enveloppe.

### R.3 X1 et X4

**X1, enveloppe à chaque lancement** :
- Sites à envelopper :
  - worker : `startup.rs:168`, `probe.rs:14`, `venv.rs:82`, `packages/process.rs:19`, `:45`, `session/manager/git.rs:24`, `gates.rs:403`, `ya_exec.rs:317`, `user_bash.rs:586`, `resolve_config_value.rs:70` (et `:84`), `google_vertex.rs:104`, `credential_process.rs:131` ;
  - superviseur : `saved_session_commands.rs:117`.
- Hors enveloppe, avec leur raison : le worker lui-même et le superviseur (`current_exe`, binaire protégé en écriture) ; `browser.rs:36`, URL https seulement, qui serait cassé par le refus LaunchServices ; les sites Linux et Windows.
- Environ **14 sites, de l'ordre de 20 à 40 lignes upstream** (estimation), plus un module du fork d'environ 150 lignes.
- Les sites partagés couvrent aussi le pont, le superviseur et le mode `-p`.
- **Détection d'un lancement ajouté plus tard** : un test CI du fork parcourt les sources des crates liées (hors modules de test) à la recherche des API de lancement (`Command::new`, `process::Command`, `CommandExt::exec`, `pre_exec`, `libc::{fork,vfork,execv*,posix_spawn*}`). Chaque occurrence doit figurer dans une liste revue, marquée enveloppée ou exclue avec sa raison. Toute occurrence nouvelle fait échouer le test.
  - Le même test fait échouer toute nouvelle crate de `Cargo.lock` absente d'une liste revue, puisqu'une crate tierce peut lancer un processus.
  - Limites :
    - un alias ou une macro qui cache l'API, sauf à élargir les motifs ;
    - un comportement d'une crate déjà admise qui change ;
    - un lancement manqué donne un contournement complet, puisque le worker n'est pas protégé.

**X4, enveloppe au lancement du worker** (`supervision.rs:583-600`) :
- **Écriture des identifiants par le worker** (rafraîchissement OAuth, login MCP, `persist_provider_change`) : tout passe par `AuthStorage`, construit à un seul endroit (`auth/manager.rs:270-278`). Un stockage du fork qui relaie `read` et `with_lock` au superviseur (`AuthStorageBackend`, `auth/storage.rs:15-40`) garde la logique d'upstream intacte. Le superviseur écrit, sous son verrou.
- Autres lectures directes à rediriger :
  - `models.json`, lu à un seul endroit (`models/registry.rs:454-458`) ;
  - `auth.json` lu par `websearch_enabled_by_credential` (`packages/manager.rs:203`).
- **Canal** : le jeton du worker est aujourd'hui dans son environnement (`descriptor.rs:63-70`, lu par `worker/config.rs:46` et `worker.rs:358`) et sur disque dans les descripteurs (`pa-types/src/daemon/worker.rs:233`, `descriptor.rs:203-208`), donc lisible par le kernel.
  - Il faut le passer par un descripteur hérité, lu puis fermé par le worker, et refuser les descripteurs `daemon-workers/**` sous l'enveloppe. Que le worker ne les lise pas est à vérifier.
  - Le profil doit laisser le worker joindre le socket du superviseur et créer le sien : le kernel le peut donc aussi.
  - Toute commande qui fait lancer quelque chose au superviseur doit alors être enveloppée (`trash`, `!…` de `catalog_chain`).
  - L'interrupteur `PRIME_AGENT_SECRET_GUARD` ne doit jamais être pris dans l'environnement de lancement d'un client.
- **Mémoire du worker** : elle contient les identifiants, mais elle n'est pas lisible depuis le kernel. Sur macOS, `task_for_pid` est refusé (mesuré). Sous Linux, Yama 1 interdit à un descendant de tracer son ancêtre (déduit, ptrace(2)).
- **`ExecuteBash`** et **`shellTool: "native"`** : protégés par construction (descendants du worker).
- **Relais de la PR B** : il reste dans le worker. Le worker obtient le jeton par `AuthStorage` comme aujourd'hui, et le stockage relayé est transparent. Le code de la PR B ne change pas ; seule la source du jeton change sous X4.
- **Login MCP** : le worker ouvre le navigateur (`mcp_login.rs:49`). Sous X4 avec le refus LaunchServices, il ne le peut plus. L'ouverture doit passer par le pont (`login_open` existe) ou par le superviseur.
- **E-C** : un seul détenteur sérialise les écritures, ce qui supprime la course entre processus qui motivait le verrou de `dad2232cc`. Les lectures relayées voient tout de suite un jeton rafraîchi.
  - Le rafraîchissement à l'expiration (`a05bcb695`) reste à reprendre, et la clé capturée à la construction de la session (H1, ticket 40 §4.2) n'est pas réglée.
  - Aide partielle.
- **Mode `-p` et `--mode rpc`** : la session tourne dans le processus (`print_runtime.rs:1288-1291`), sans worker. X4 ne les couvre pas : il faudrait les sites de X1, ce qui revient à X1 en entier.
  - Proposition : sous la protection, ces modes refusent de démarrer avec le message qui nomme la variable. Les bancs et les tests la posent déjà (question 18).
- **Lignes upstream (estimation)** :
  - `supervision.rs:583-600` : 3 à 5 lignes ;
  - `auth/manager.rs:274-276` : 2 à 3 ;
  - `models/registry.rs:458` : 2 à 3 ;
  - `packages/manager.rs:203` : 2 à 3 ;
  - jeton par descripteur hérité (`descriptor.rs:63-70`, `worker/config.rs:46`, `worker.rs:358`, `supervision.rs`) : 10 à 15 ;
  - lancements du superviseur et du pont enveloppés (`resolve_config_value.rs:70`, `saved_session_commands.rs:117`, `packages/process.rs:19`, `:45`) : 4 à 8 ;
  - navigateur du login MCP (`mcp_login.rs:49`) : 3 à 5 ;
  - refus de `-p` et de `rpc` (`print_runtime.rs`) : 3 à 5.
  - Soit **30 à 45 lignes**, plus des modules du fork : relais des identifiants (environ 300 lignes) et enveloppe (environ 150).
- **Détection** : un lancement ajouté dans le worker est protégé par construction. Le test CI de X1 reste utile pour les sites joignables depuis le superviseur et le pont, mais on ne sait pas par lecture du source quel rôle exécute un site partagé. La liste revue l'annote, et une erreur humaine reste possible.

**Dans les deux cas** :
- Refus d'écriture, dans le profil, sur le binaire en cours (chemin canonique de `current_exe()`) et sur son manifeste. Le manifeste est `package.json` dans `package_dir()` (`crates/pa-cli/src/config.rs:62-75`), ou `PI_PACKAGE_DIR` du démon.
- Refus `file-write-unlink` sur leurs dossiers parents, comme pour le dossier agent.
- Refus LaunchServices (§R.2).
- Ce que lance le pont : `npm`, `git` et `bash -c` passent par les sites partagés enveloppés. Le lancement du démon utilise `current_exe`, binaire protégé. Le navigateur s'ouvre après une action, sur l'URL du flux. Le pont n'a besoin d'aucun lancement propre en plus.

### R.4 Recommandation

**X4**, avec le refus de `-p` et de `rpc` sous la protection, et le refus LaunchServices :
- L'inventaire montre que le worker lance beaucoup, souvent sans action : kernel, sonde, `uv`, paquets, `git` à chaque tour, portes, `exec_command`, `!…`, `gcloud`, `credential_process`, navigateur. Upstream en ajoute régulièrement.
- X4 ramène la surface non protégée au superviseur et au pont, qui lancent peu : `current_exe`, `trash`, `!…`, `npm` par l'écran Réglages, navigateur.
- Il protège par construction tout lancement futur du worker.
- X1 coûte un peu moins de lignes, mais un seul lancement oublié, chez nous ou dans une reprise upstream, rouvre tout.
- Si le refus de `-p` et de `rpc` est inacceptable, X4 ne fait pas l'économie de X1 pour ces modes. Dans ce cas, X1 seul est plus simple.

Questions :

17. **X1 ou X4** ?
18. Avec X4, **`-p` et `--mode rpc` refusés** sous la protection, avec le message qui nomme la variable ?
19. **Services locaux** (§R.2) : limite documentée, ou refus des connexions de socket Unix sauf une liste (DNS `mDNSResponder`, à mesurer) ? Ce refus casserait Docker, ssh-agent, gpg-agent et les bases locales depuis le kernel.
20. **Refus LaunchServices** dans le profil, au prix de tout `open` depuis le kernel (le navigateur s'ouvre par le pont) ?

## S. Réponses 17 à 20 et garantie (26/11/2026)

### S.1 Garantie de la protection (décision de Yusuf, reportée au journal du CDC)

- **Ni le kernel ni aucun processus lancé par yusAgent ne peut lire ou écrire les fichiers d'identifiants.** Cela vaut pour ce que lancent le worker, le démon, le pont, les modes ACP, le TUI, `-p` et `rpc`.
- Restent en limites documentées (§J) :
  - les services locaux hors de yusAgent qui exécutent une commande ou lisent un fichier pour un client (Docker, `screen`, tmux, IPC de VS Code, serveurs TCP locaux, autres) ;
  - l'exécution différée.
- Plus de recherche de nouveaux canaux au constat. Un canal trouvé pendant le code est signalé et classé, sans élargir le ticket.

### S.2 Réponses

17. **X1.**
    - Le test CI qui parcourt le code et `Cargo.lock` est obligatoire. Il échoue dès qu'un lancement de processus apparaît hors de l'enveloppe dans les crates du worker, du démon et du pont, avec une liste d'exceptions commentée (aucune attendue).
    - Refus d'écriture sur le binaire en cours, sur son `package.json` et sur leurs dossiers parents.
    - **Écart à signaler (S.3) : deux lancements ne peuvent pas être enveloppés par construction.**
18. Sans objet (X1).
19. Limite documentée. Docker est nommé en premier au §J et marqué déduit.
20. Oui : refus de `com.apple.coreservices.*` et `com.apple.lsd.*` dans le profil (effets en S.4).

Plan : PR B (relais MCP et websearch), puis PR A (enveloppe du kernel et des lancements du worker, du démon et du pont selon X1, socket, LaunchServices, binaire, `st_nlink`), puis PR C (Linux) après la sonde.

### S.3 Écart : les relances du binaire yusAgent

Deux lancements doivent rester hors de l'enveloppe, parce que le processus lancé est yusAgent lui-même et lit les identifiants :
- le worker, lancé par le superviseur (`crates/pa-daemon/src/supervisor/supervision.rs:583`) ;
- le superviseur, lancé par le pont ou le TUI (`crates/pa-cli/src/interactive_mode/daemon.rs:170`).

Les deux lancent `current_exe()`, protégé en écriture par le profil (S.2, réponse 17).

**Proposition** : le test CI les traite comme une catégorie à part, « relance du binaire yusAgent ». Il l'admet seulement si le programme est `current_exe()`, ce qu'il vérifie à la lecture du site, et refuse toute autre exception. **À confirmer par Yusuf avant la PR A.**

**Conditions posées par Yusuf (26/11/2026), à tenir dans la PR A :**

1. **Navigateur ouvert par le worker sur une demande du kernel** (`mcp.begin_login`, `crates/pa-daemon/src/mcp_login.rs:49`) :
   - seules les URL `https://` sont acceptées, plus `http://` vers localhost si un login en a besoin ;
   - tout autre schéma, chemin de fichier ou application est refusé avec un message fixe ;
   - sinon, le kernel ferait ouvrir hors protection une application qu'il a créée, ce qui rouvrirait le canal de la réponse 20 ;
   - **test rouge au push 1 de la PR A** : une demande avec un chemin de fichier, une application ou un schéma autre que `https://` n'ouvre rien et rend le message fixe ;
   - aujourd'hui, seule l'URL d'autorisation découverte est forcée en https (`crates/pa-core/src/mcp/oauth_discovery.rs:120`), pas l'appel d'ouverture lui-même.
2. **Exception « relance du binaire yusAgent »** :
   - le refus d'écriture vise le chemin canonique renvoyé par `current_exe()` : le binaire, le `package.json` à côté de lui et leurs dossiers parents ;
   - **un test vérifie qu'une écriture sur ce chemin échoue sous l'enveloppe.**

Le navigateur (`crates/pa-core/src/platform/browser.rs:36`) n'est **pas** une exception. Il passe par l'enveloppe avec une variante du profil, qui refuse les fichiers d'identifiants mais laisse LaunchServices ouvert, puisqu'il sert justement à lancer le navigateur. Le processus `open` ne peut pas lire les identifiants, et le navigateur, lancé par LaunchServices, n'est pas un processus de yusAgent.

### S.4 Ce que casse le refus LaunchServices

Le profil complet s'applique au kernel, à ses descendants et aux lancements enveloppés du worker, du démon et du pont, sauf la variante du navigateur.

- **Cassé depuis le kernel** : `open` d'un fichier, d'une URL ou d'une application (`bash("open index.html")`), `webbrowser.open()` en Python, `osascript` (bloqué, §R.2), et tout outil en ligne de commande qui passe par LaunchServices pour trouver ou lancer une application.
- **Navigateur ouvert par le worker sur une demande du kernel** (`mcp.begin_login`, `crates/pa-daemon/src/mcp_login.rs:49`) : **pas touché**. Le worker ouvre le navigateur par `browser.rs`, sous la variante sans refus LaunchServices.
- **Login MCP depuis l'extension** : **pas touché**. Le flux tourne dans le pont (`crates/pa-cli/src/print_runtime/ya_acp_providers.rs:106-166`), qui ouvre le navigateur par `login_open` (`crates/pa-daemon/src/acp/daemon/ya_providers.rs:42`, `:436`), sous la même variante.
- **À mesurer à la PR A** : une valeur `!…` ou un `credential_process` qui s'appuie sur une application de bureau, par exemple un gestionnaire de mots de passe dont la CLI lancerait l'application par LaunchServices si elle ne tourne pas. `security` (trousseau) passe par un autre service ; c'est déduit, non mesuré.

## T. PR B (PR #93) : relevé du push 1 et plan du push 2 (26/11/2026)

### T.1 Push 1 : tests seuls

- `crates/pa-core/tests/ya_secret_relay.rs` (nouveau, fork) et `prime-agent-runtime/test/test_ya_secret_relay.py` (nouveau, fork).
- `99c3b8f4d`, run `ci` 45850576629 : **le test Rust ne compilait pas**. Il manquait les champs `client_secret` et `scopes` de `McpServerConfig::Http` (`crates/pa-core/src/mcp/mod.rs:135-140`). Clippy (workspace et Windows) et le shard 1 sont tombés sur cette erreur. Corrigé par `596783b4d`.
- `a91207b34`, run `ci` 45852970664 :
  - **clippy** (`while_let_loop`, `ya_secret_relay.rs:64`) a fait échouer `check + clippy` et le job Windows. Corrigé par le push 1 ter ;
  - **test shard 1/8** : seule l'unité `pa-core#test:ya_secret_relay` échoue. Ses 6 tests échouent chacun sur son assertion :

| Test | Assertion en échec |
|---|---|
| `mcp_http_relay_adds_the_bound_token_and_returns_no_secret` | `relay mcp.http absent: the kernel still calls the MCP server itself, with the token` |
| `mcp_http_relay_refuses_a_url_other_than_the_configured_one` | `relay mcp.http absent` |
| `mcp_http_relay_answers_the_get_stream_with_405_without_calling_the_server` | `relay mcp.http absent` |
| `mcp_config_never_hands_the_kernel_a_literal_header` | `mcp.config hands the kernel a user header: {"type":"http","url":"http://127.0.0.1:39281/mcp","headers":{"X-Api-Key":"ya41-sentinel-user-header-5e1b"}}` |
| `websearch_relay_adds_the_stored_key_and_returns_no_secret` | `relay websearch.search absent: the kernel still reads the Serper key itself` |
| `serper_api_key_never_reaches_the_kernel_even_when_named` | `SERPER_API_KEY is a harness credential (ticket 41, answer 13)` |

  - **suite du runtime** (CPython 3.11) : 823 tests, 3 échecs, exactement les 3 tests de `test_ya_secret_relay.py`, chacun sur son assertion (`the kernel read auth.json`, `the integration read auth.json`, `[] != ['websearch.search']`).
  - Autres jobs verts.

### T.2 Push 2 : le code, lignes upstream (à valider avant de coder)

Lignes de l'arbre à `a91207b34`. Les nombres de lignes sont des estimations.

**Rust**
- Nouveau module du fork `crates/pa-core/src/mcp/ya_relay.rs`, d'environ 250 lignes :
  - handlers `mcp.http` et `websearch.search` ;
  - URL du serveur prise dans la configuration (utilisateur, catalogue, ACP), jamais celle du kernel. Il faut qu'elle soit égale à l'`endpoint` lié pour un identifiant OAuth ou un jeton collé ;
  - en-têtes configurés et jeton ajoutés par le worker ;
  - GET refusé par un 405 ;
  - réponse complète `{status, headers, body}` ;
  - clé Serper prise dans `SERPER_API_KEY` du worker, sinon dans `serper` d'`auth.json` (avec résolution `!…`), puis POST vers l'URL fixe de Serper. Le test peut la remplacer par `PRIME_AGENT_TEST_SERPER_URL`.
- `crates/pa-core/src/mcp/mod.rs`, environ 8 à 10 lignes :
  - déclaration `mod ya_relay;` (après `:21`) ;
  - appel d'enregistrement à la fin de `register_host_handlers` (`:601` et suivantes, après le bloc `mcp.begin_login`) ;
  - retrait des `headers` aux trois retours de `mcp.config` (ACP `:702-710`, catalogue `:724-735`, utilisateur `:740`).
- `crates/pa-core/src/kernel/env_allowlist.rs` (fichier du fork) : `SERPER_API_KEY` dans `EXTRA_HARNESS_CREDENTIALS` (`:119-131`).
- `crates/pa-core/src/session_engine/runtime_wiring.rs` : retrait de l'injection de `SERPER_API_KEY` dans l'environnement du kernel (`:296-298`) et du paramètre `serper_api_key` (`:259`), environ 5 lignes. `websearch_api_key` (`:332-360`) reste, appelé par le relais.
- `crates/pa-core/src/session_engine/engine.rs` : retrait du calcul de la clé Serper pour le kernel (`:600-614`) et de son passage (`:653`), environ 15 lignes.
- `crates/pa-core/tests/prompt_guards.rs` : `mcp.http` et `websearch.search` dans `INTERNAL_HOST_REQUESTS` (`:180-190`), 2 lignes. Ces requêtes sont de la plomberie interne ; le prompt ne les documente pas.

**Python** (le kernel)
- Nouveau module du fork `prime-agent-runtime/src/rlm/ya_relay.py`, d'environ 80 lignes : transport `httpx2` qui envoie chaque requête par `host_request("mcp.http")`.
- `prime-agent-runtime/src/rlm/mcp.py`, de l'ordre de 60 à 90 lignes retirées ou changées :
  - `_open_http` (`:275-300`) passe par le transport du relais, sans en-têtes ;
  - retrait de `_headers` (`:956-978`), `_static_token` (`:942-953`), `_bound_auth` (`:893-904`) et `_read_auth` (`:1085-1094`) ;
  - `_auth_identity` (`:889`, `:914-940`) n'utilise plus le jeton. Clé de réutilisation proposée : serveur et URL.
- `prime-agent-runtime/src/rlm/mcp_base.py`, environ 40 lignes : `_read_auth` (`:68-77`), `_token` (`:142-165`), `_resolve_token` (`:167-190`) et `_open_session` (`:199-240`) passent par le relais.
- `skills/websearch/src/websearch/websearch.py`, environ 40 lignes :
  - `run` appelle `host_request("websearch.search", {"query", "num"})` ;
  - retrait de `_resolve_api_key`, `_resolve_config_value`, `_agent_dir` et de l'appel direct à Serper (`:10-17`, `:29-60`, `:117-136`) ;
  - le formatage des résultats ne change pas.

**Tests upstream dont l'assertion change**, parce que le comportement passe du kernel au worker :
- `prime-agent-runtime/test/test_mcp.py`, 7 tests :
  - `test_acp_headers_never_consult_or_override_host_oauth` (`:329`) ;
  - `test_endpoint_bound_credential_never_attaches_to_another_url` (`:360`) et `test_unbound_credential_requires_relogin` (`:374`) ;
  - `test_static_token_headers_attach_only_from_the_bound_credential` (`:380`), `test_static_token_failure_is_the_kernel_unavailable_error` (`:405`), `test_static_token_bearer_is_never_env_or_command_resolved` (`:419`) et `test_static_token_auth_identity_hashes_the_bound_bearer` (`:434`).
  - Les règles qu'ils vérifient (`endpoint` lié, re-login, jeton collé jamais résolu par l'environnement ni par une commande) sont reportées dans des tests Rust du relais.
- `prime-agent-runtime/test/test_mcp_base.py` : lecture du jeton (`:72-145`) et signatures d'ouverture (`:243-279`), même report.

**Préfixe** : inchangé.
- Les nouvelles requêtes sont internes (`prompt_guards.rs`), et ni `SKILL.md` ni les descriptions de skills ne changent : le `SKILL.md` de websearch renvoie déjà à `/mcp` pour la clé (`skills/websearch/SKILL.md:3`, `:12-13`).
- Seuls des textes d'erreur, dans le résultat d'une cellule, peuvent changer : relais refusé, serveur sans identifiant.

### T.3 Push 2 : réductions demandées par Yusuf et lignes finales (26/11/2026)

**Réduction 1, variante à diff minimal : retenue.**
- Seuls changent, côté kernel, les points où le client HTTP est construit. Rien n'est nettoyé.
  - `_open_http` et `_open_session` ouvrent le client relais.
  - `_headers` et `_auth_identity` ne cherchent plus de jeton pour un serveur relayé.
  - `_read_auth` reste en place, mais n'est plus appelé sur le chemin relayé.
- Lignes upstream hors tests, comptées par `git diff --numstat` : **+46, −12**, contre environ 170 à 200 pour la liste du §T.2. La réduction dépasse le tiers demandé.
- Mêmes tests : la suite Python passe en local (823 tests, `uv run --python 3.11 python -m unittest discover -s test`).

**Réduction 2, clé Serper : retenue.**
- `SERPER_API_KEY` entre dans `EXTRA_HARNESS_CREDENTIALS` (`env_allowlist.rs`, fichier du fork).
- Le retrait final au lancement, `strip_harness_credentials` (`crates/pa-core/src/kernel/manager/startup.rs:546`), écarte alors la clé injectée par le câblage.
- `runtime_wiring.rs` et `engine.rs` ne sont pas touchés.

**Lignes upstream finales** (fichier:ligne, nombre) :

| Fichier | Lignes | Nombre |
|---|---|---|
| `crates/pa-core/src/mcp/mod.rs` | `:22-23` (`mod ya_relay;`), `:709` (ACP), `:735` (catalogue), `:744-747` (réglages), `:774-775` (enregistrement) | +10, −1 |
| `prime-agent-runtime/src/rlm/mcp.py` | `:35` (import), `:284-287` (`_open_http`), `:920-921` (`_auth_identity`), `:968-969` (`_headers`) | +9 |
| `prime-agent-runtime/src/rlm/mcp_base.py` | `:28` (import), `:215-220` (`_open_session`) | +7 |
| `skills/websearch/src/websearch/websearch.py` | `:10-11` (import), `run` (`:160-183`), `_clip` extrait | +17, −11 |
| `crates/pa-core/tests/prompt_guards.rs` | `:185-187` | +3 |
| `prime-agent-runtime/test/test_mcp.py` (7 tests annoncés, 6 réellement touchés) | `:360-423` | +23, −63 |
| `prime-agent-runtime/test/test_mcp_base.py` | `:258-262` | +5, −2 |

- `test_acp_headers_never_consult_or_override_host_oauth` passe toujours : son assertion ne change pas. Il n'y a donc que 6 tests réécrits dans `test_mcp.py`, plus 1 dans `test_mcp_base.py`.
- Les règles reprises par les tests Rust sont listées dans `docs/fork-changes.md` (section « Identifiants hors du kernel, PR B »). Aucun test n'est supprimé.
- **Écart** : les 4 tests Rust qui reprennent ces règles (`…_bound_elsewhere_or_unbound`, `…_fails_closed_with_the_unavailable_message`, `…_attaches_a_pasted_token_literally`, `…_outside_the_settings_without_a_bound_credential`) arrivent avec le code, au push 2. Ils n'ont donc pas été vus rouges : ils portent des règles qui existaient déjà côté kernel.
- La règle « l'identité de génération suit le jeton » est retirée : le worker lit le jeton à chaque requête.

### T.4 Push 2 : relevé de la CI (26/11/2026)

- `c74c9d16d` (push 1 ter), run `ci` 45869754654 : clippy passe (workspace et Windows). Restent les seuls rouges attendus : le shard 1 (`ya_secret_relay`) et la suite Python (3 tests).
- `35f7bdc4b` (push 2), run `ci` 45871612250 : tout passe sauf le shard 1, à l'étape « Run the rust lifecycle checks ».
  - Reçu `kernel_env.json` : `scripts/check_kernel_env.py` attendait la clé Serper résolue dans le kernel (`resolved-fixture-key`).
  - C'est l'effet voulu de la réponse 13.
  - **Écart** : ce contrôle upstream n'était pas dans les lignes validées. Je l'ai traité comme un test dont l'assertion change (`serper: None`, +3, −2) et je l'ai listé dans `docs/fork-changes.md`.
- `f49bff54f` (push 2 bis), run `ci` 45874752019 : **tous les jobs passent**, y compris le shard 1 (vérifications de cycle de vie et tests de `ya_secret_relay`), clippy et la suite Python.
  - Échec à part : « Check changelog fragment ». Il manquait le fragment `.changes/ya-41-protection-secrets.md`, ajouté au push suivant.

### T.5 Preuve par mutation des quatre tests Rust arrivés avec le code (26/11/2026)

Demande de Yusuf : avant le merge de la PR #93, casser d'une ligne chacune des quatre règles réelles et vérifier que chaque test échoue sur son assertion, et seulement lui.
- Branche jetable `ya-41-mutation`, créée depuis `863e27b69`, dernier commit de la PR.
- Commits de diagnostic hors PR : `dd8e142e3`, puis `821134510`.
- `ci.yml` ne tourne que sur une PR vers `main` : PR temporaire **#94**, en mode prêt, fermée après le relevé, branche distante supprimée.

Mutations, dans `crates/pa-core/src/mcp/ya_relay.rs` :

| Mutation | Ligne cassée | Règle | Test |
|---|---|---|---|
| M1 | `bound_token`, bras OAuth : `endpoint == url` devient `!endpoint.is_empty()` | jeton OAuth lié à l'adresse exacte | T1 `mcp_http_relay_never_attaches_a_token_bound_elsewhere_or_unbound` |
| M2 | `bearer_for`, `Stored` : `return Err(credentials_unavailable(server))` devient `return Ok(None)` | refus net, avec le message fixe | T2 `mcp_http_relay_fails_closed_with_the_unavailable_message` |
| M3 | `bound_token`, jeton collé : `Some(bearer.trim().to_string())` devient `resolve_config_value(bearer.trim())` | jeton collé pris tel quel, jamais résolu par une variable d'environnement ni par une commande | T3 `mcp_http_relay_attaches_a_pasted_token_literally` |
| M4 | `mcp_http`, serveur hors réglages : l'absence d'identifiant n'est plus une erreur | serveur hors réglages refusé sans identifiant lié | T4 `mcp_http_relay_refuses_a_server_outside_the_settings_without_a_bound_credential` |

**Run 1** (`dd8e142e3`, M1 à M4, run `ci` 45877790177, shard 1) : exactement T1, T2, T3 et T4 échouent. Les 7 autres tests de `ya_secret_relay.rs` passent.
- T2 (`:420`) : `no credential: Object {"status": Number(200), …}`. Seule M2 peut le produire (aucun identifiant, serveur configuré) : **attribué**.
- T4 (`:474`) : `an unknown server with no bound credential is refused`. Seule M4 peut le produire : **attribué**.
- T1 (`:400`) : `no usable credential: the relay fails closed: Object {"status": Number(200), …}`. M1 comme M2 laissent partir la requête : **ambigu**.
- T3 (`:448`) : `the pasted bearer is attached as is: POST /mcp HTTP/1.1 / authorization: ***`. GitHub masque la valeur de l'en-tête dans le journal, donc la preuve n'est pas lisible.
- Clippy échoue aussi (`match_same_arms`, causé par M4). C'est sans rapport avec les tests.

**Run 2, isolement** (`821134510`, M1 et M3 seules, run `ci` 45879883671, shard 1) : **seuls T1 et T3 échouent**, avec les mêmes messages (`:400`, `:448`). T2, T4 et les 7 autres tests du relais passent.
- T1 ne peut plus échouer que par M1, puisque M2 est retirée : **attribué**.
- T3 ne peut échouer que par M3, puisque M1 ne touche que le bras OAuth et que T3 n'utilise qu'un jeton collé : **attribué**.
- Clippy, `fmt` et la suite Python passent.

**Conclusion** : chacun des quatre tests échoue sur son assertion sous la mutation de sa propre règle, et seulement lui.

## U. PR A (PR #95) : classement des lancements et lignes du push 2 (26/11/2026)

Réponses de Yusuf aux écarts du push 1 :
1. Test 1 : squelette de la fonction de profil au push 1, qui rend une chaîne vide (`crates/pa-types/src/platform/ya_secret_guard.rs`), plutôt qu'une preuve par mutation.
2. Lancements : classés comme ci-dessous, et chaque site upstream ne change qu'en un appel à une fonction du fork.
3. Sockets : le profil refuse les chemins exacts des sockets en usage (superviseur et worker, enregistrés par `register_sockets`), plus les dossiers par défaut. Le test 2 couvre un socket hors du dossier par défaut.

**Emplacement de l'enveloppe** : `pa-types` (`crates/pa-types/src/platform/ya_secret_guard.rs`, fichier du fork). C'est la seule crate dont dépendent toutes les autres : `pa-ai` et `pa-tui` ne dépendent pas de `pa-core`. Chaque site passe de `Command::new(x)` à `pa_types::platform::ya_secret_guard::command(x)`, ou à `tokio::process::Command::from(…command(x))`, sur la même ligne.

### U.1 Classement des 44 lancements trouvés par le scan

**Exemptés** (`crates/pa-core/tests/ya_secret_guard/launch_exceptions.txt`, chaque preuve vérifiée par le test) :

| Catégorie | Sites | Preuve vérifiée |
|---|---|---|
| Relance du binaire yusAgent (2, hors des 44) | `pa-daemon/src/supervisor/supervision.rs:583`, `pa-cli/src/interactive_mode/daemon.rs:170` | `current_exe()` dans le fichier (`supervision.rs:573`, `daemon.rs:64`) |
| Propre à Windows (6) | `pa-ai/.../bedrock/credential_process.rs:123`, `pa-core/src/auth/resolve_config_value.rs:84`, `:100`, `pa-core/src/platform/process.rs:266`, `pa-core/src/platform/shell.rs:134`, `pa-tui/src/external_editor.rs:67` | `cfg(windows)` dans les 40 lignes avant (`:120`, `:80`, `:80`, `:255`, `:132`, `:65`) |
| Propre à Linux, jusqu'à la PR C (3) | `pa-types/src/platform/process.rs:41`, `:352` (`ps`), `pa-tui/src/clipboard.rs:284` (`which wl-copy`) | `not(target_vendor = "apple")` (`:36`, `:350`) ; `copy_on_linux` (`:277`) |
| Inatteignable (1) | `pa-cli/src/client_update.rs:51` | `/update` du TUI rendu avant tout lancement : `client_update.rs:17` (`SELF_UPDATE`, ticket 19) |

**Enveloppés** (34 sites, une ligne chacun) :
- Les flux de mise à jour (`update_flow/*`, `pa-core/src/update/*`) sont enveloppés, faute de preuve d'inatteignabilité : `run_coordinator_mode` est appelé par `pa-cli/src/package_command.rs:460`, hors du verrou `SELF_UPDATE` (`public_command.rs:666`).
- Les deux ouvreurs de navigateur (`pa-core/src/platform/browser.rs:36`, `pa-tui/src/browser.rs:55`) passent par la variante sans refus de `LaunchServices` et par le contrôle `https://`, ou `http://` vers localhost (une ligne de plus chacun).

### U.2 Lignes upstream du push 2 (estimation, à confirmer au compte rendu)

| Fichier | Sites | Lignes |
|---|---|---|
| `crates/pa-ai/src/providers/bedrock/credential_process.rs` | `:131` | 1 |
| `crates/pa-ai/src/providers/google_vertex.rs` | `:104` | 1 |
| `crates/pa-cli/src/daemon_discovery/scan.rs` | `:152` | 1 |
| `crates/pa-cli/src/factory_command.rs` | `:270` | 1 |
| `crates/pa-cli/src/update_flow/successor.rs` | `:51` | 1 |
| `crates/pa-cli/src/update_flow/swap.rs` | `:156` | 1 |
| `crates/pa-cli/src/update_flow/update_command.rs` | `:176` | 1 |
| `crates/pa-cli/src/print_runtime.rs` | avertissement « protection coupée » sur stderr du pont, recopié dans l'Output | 1 |
| `crates/pa-core/src/auth/resolve_config_value.rs` | `:70` | 1 |
| `crates/pa-core/src/autonomous/gates.rs` | `:403` | 1 |
| `crates/pa-core/src/kernel/bootstrap/venv/probe.rs` | `:14` | 1 |
| `crates/pa-core/src/kernel/bootstrap/venv.rs` | `:82` | 1 |
| `crates/pa-core/src/kernel/manager/startup.rs` | `:168` | 1 |
| `crates/pa-core/src/packages/process.rs` | `:19`, `:45` | 2 |
| `crates/pa-core/src/platform/browser.rs` | `:36`, plus le contrôle du schéma | 2 |
| `crates/pa-core/src/platform/shell.rs` | `:118` | 1 |
| `crates/pa-core/src/session/manager/git.rs` | `:24` | 1 |
| `crates/pa-core/src/update/download.rs` | `:133` | 1 |
| `crates/pa-core/src/update/installer.rs` | `:626`, `:761` | 2 |
| `crates/pa-core/src/workspace_snapshot/git.rs` | `:121` | 1 |
| `crates/pa-daemon/src/saved_session_commands.rs` | `:117` | 1 |
| `crates/pa-daemon/src/user_bash.rs` | `:586` | 1 |
| `crates/pa-daemon/src/worker.rs` | `register_sockets` (socket du superviseur et du worker) | 1 |
| `crates/pa-daemon/src/supervisor.rs` | `register_sockets` de son socket ; contrôle `st_nlink` d'`auth.json` au démarrage ; avertissement « protection coupée » | 3 |
| `crates/pa-tui/src/browser.rs` | `:55`, plus le contrôle du schéma | 2 |
| `crates/pa-tui/src/clipboard.rs` | `:60`, `:229` | 2 |
| `crates/pa-tui/src/clipboard_image.rs` | `:50` | 1 |
| `crates/pa-tui/src/external_editor.rs` | `:61` | 1 |
| `crates/pa-tui/src/export_share.rs` | `:149`, `:156`, `:164`, `:171` | 4 |
| `crates/pa-tui/src/interactive/render.rs` | `:51` | 1 |
| `.cargo/config.toml` | `PRIME_AGENT_SECRET_GUARD = "off"` dans `[env]` | 1 |
| `scripts/lifecycle_lib.py` et contrôles qui filtrent l'environnement (liste de `docs/fork-changes.md:1114`) | `PRIME_AGENT_SECRET_GUARD` ajoutée à leur liste blanche | 1 à 8 |

- **Total estimé : environ 45 à 52 lignes upstream dans 31 à 38 fichiers**, presque toutes des remplacements d'une ligne.
- Déjà au push 1 : `crates/pa-types/src/platform/mod.rs` (+2, déclaration du module).
- `crates/pa-core/src/tools/ya_exec.rs:317` est un fichier du fork, hors de ce décompte.
- Code du fork : le module de l'enveloppe (profil, deux variantes, contexte des chemins, interrupteur, contrôle des URL, refus au lancement si `sandbox-exec` manque), `env_allowlist.rs` (`PRIME_AGENT_SECRET_GUARD` jamais transmise au kernel), les bancs (variable posée, README).

## V. PR A (PR #95) : relevé du push 1 et push 2 (26/11/2026)

### V.1 Push 1 : tests seuls (`bc31006e8`, `02d974186`, `5f9a00e03`)

- `02d974186` (push 1 bis) : squelette de `profile`, qui rend une chaîne vide, et test 1 ; catégories d'exemption du scan ; socket hors du dossier par défaut dans le test 2 ; `--no-fail-fast` dans le job macOS (au push 1, `cargo test` s'arrêtait au premier binaire en échec).
- `5f9a00e03` (push 1 ter) : extension `.rs` comparée par `Path::extension`, un lint de clippy relevé au run `ci` 45882785163.

**Run `secret guard (macos-15)` 45884801495** : chaque test échoue sur son assertion.

| Test | Assertion en échec |
|---|---|
| Test 1, deux tests (`ya_secret_guard_profile.rs:42`, `:77`) | `not a Seatbelt profile` (le squelette rend une chaîne vide) |
| Test 2 (`ya_secret_guard_kernel.rs:202`) | `the kernel reached what the guard refuses`, les 11 accès en `ok` : lecture d'`auth.json`, de `models.json` et d'`auth.json.tmp123`, lecture par symlink, remplacement de `models.json`, socket du dossier par défaut, socket hors du dossier par défaut, écriture du binaire en cours, écriture de son `package.json`, `bash cat auth.json`, renommage du dossier agent |
| Test 3 (`ya_secret_guard_acp_e2e.rs:210`) | `a session cell read auth.json: {"read auth.json":"ok","connect to the daemon socket":"ok"}` |
| Test 7 (`ya_secret_guard_hardlink_e2e.rs:68`) | `auth.json still shares its inode with a link made before the guard` (3618419 des deux côtés) |
| Scan (`ya_process_launch_scan.rs:233`) | `34 process launch(es) outside the guard` |
| Navigateur (`ya_browser_scheme.rs:76`) | `the opener launched what it must refuse` : les 7 cibles interdites |
| `every_crate_of_the_lockfile_was_reviewed` | passe (garde-fou, vert dès le départ) |

**Run `ci` 45884801558 (Linux)** : clippy, `fmt` et la suite Python passent. Les seules unités en échec sont les rouges attendus, chacun sur son assertion :
- `pa-core#test:ya_process_launch_scan` (shard 2) ;
- `pa-core#test:ya_browser_scheme` (shard 6) ;
- `pa-cli#test:ya_secret_guard_hardlink_e2e` (shard 7) ;
- `pa-types#test:ya_secret_guard_profile` (shard 8).

### V.2 Réponses de Yusuf (26/11/2026) et écarts

1. **Exemption Windows** : un marqueur explicite précède chaque lancement, et le scan vérifie qu'il est à l'intérieur d'un élément `#[cfg(windows)]`. Commit `911f1bed0`, 6 lignes dans 5 fichiers, avec un test d'auto-contrôle (marqueur retiré, `cfg` retiré, `cfg` posé sur un autre élément).
   - **Écart** : `crates/pa-core/src/platform/process.rs` (site `taskkill`) n'était pas dans la liste du §U.2. Ce commit est compté à part du push 2.
2. **Push 2** validé d'avance à trois conditions : au plus 52 lignes upstream, un seul appel par site, aucun fichier hors du §U.2. **Les trois sont tenues** (V.3).

Choix faits pour tenir ces conditions :
- **Module renommé `ya_guard`**, pour que chaque site reste sur une ligne de 100 caractères au plus. Fonctions : `command`, `tokio`, `model_tokio`, `browser(program, url)`, `sockets`, `supervisor_start`, `warn_if_off`.
- **Navigateur** : le contrôle de l'URL est fait dans `browser(program, url)`, sans ligne de plus. Une URL refusée reçoit un programme inexistant, et rien ne s'ouvre.
- **Sans mécanisme** (Linux avant la PR C) : seuls le kernel et `exec_command` refusent de démarrer (réponse 8, « refus de lancer le kernel »). Les autres lancements passent tels quels. Sans kernel, aucun code du modèle ne tourne.
- **Contrôles de cycle de vie** : lancés hors de `cargo`, la variable de `.cargo/config.toml` ne leur parvient pas. Ils posent donc `PRIME_AGENT_SECRET_GUARD=off` dans l'environnement qu'ils donnent au binaire, une ligne par fichier, sans toucher `ci.yml`.

## Annexe : mesures macOS

Commande : `python3 -I docs/tickets/41-protection-secrets/mesure_macos.py`. Elle travaille dans un dossier temporaire avec des secrets factices, sans lire `~/.yusagent` ni appeler de modèle. Sortie du lancement par Claude Code (26/11/2026, relancé après l'ajout des sorties de sandbox du §R.2, 48 essais) :

```
ProductName:		macOS ; ProductVersion:		27.0.1 ; BuildVersion:		26A434
ok  cat auth.json                                                                      attendu : refusé          obtenu : refusé
ok  python open(auth.json)                                                             attendu : refusé          obtenu : refusé
ok  cat models.json                                                                    attendu : refusé          obtenu : refusé
ok  lire kernel-venv/f                                                                 attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  écrire settings.json                                                               attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  créer un fichier dans le dossier                                                   attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  planter auth.json.tmp123                                                           attendu : refusé          obtenu : refusé
ok  créer le verrou auth.json.lock                                                     attendu : refusé          obtenu : refusé
ok  remplacer models.json par renommage                                                attendu : refusé          obtenu : refusé
ok  renommer le dossier agent                                                          attendu : refusé          obtenu : refusé
ok  renommer son parent                                                                attendu : refusé          obtenu : refusé
ok  lien symbolique puis cat                                                           attendu : refusé          obtenu : refusé
ok  lien physique créé dans la sandbox                                                 attendu : refusé          obtenu : refusé
ok  variante de chemin …/regex/home/.ya/AUTH.JSON                                      attendu : refusé          obtenu : refusé
ok  variante de chemin …/regex/home/.ya/../.ya/auth.json                               attendu : refusé          obtenu : refusé
ok  variante de chemin /System/Volumes/Data…/regex/home/.ya/auth.json                  attendu : refusé          obtenu : refusé
ok  accès par inode /.vol/<dev>/<ino>                                                  attendu : refusé          obtenu : refusé
ok  pendant l'écriture : cat du temporaire                                             attendu : refusé          obtenu : refusé
ok  pendant l'écriture : lien physique du temporaire                                   attendu : refusé          obtenu : refusé
ok  après renommage : cat auth.json                                                    attendu : refusé          obtenu : refusé
ok  descendant : setsid + sh -c cat                                                    attendu : refusé          obtenu : refusé
ok  sandbox imbriquée (sandbox-exec dans sandbox-exec)                                 attendu : refusé (rc=71)  obtenu : refusé (rc=71)
ok  exécuter /bin/ps (setuid)                                                          attendu : refusé          obtenu : refusé
ok  exécution différée : écrire ~/.zshrc                                               attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  exécution différée : écrire ~/projet/.git/hooks/pre-commit                         attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  exécution différée : écrire ~/Library/LaunchAgents/x.plist                         attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  exécution différée : écrire ~/Library/Application Support/Code/User/settings.json  attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  exception (with no-sandbox) pour /bin/ps                                           attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  contrôle positif : cat auth.json hors sandbox                                      attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  literal : lien physique du temporaire pendant l'écriture                           attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  sans règle sur les ancêtres : mv du dossier puis cat                               attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  lien physique créé AVANT, hors sandbox                                             attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  hors sandbox : env du parent par sysctl KERN_PROCARGS2                             attendu : visible         obtenu : visible
ok  hors sandbox : env du parent par ps eww                                            attendu : visible         obtenu : visible
ok  hors sandbox : mémoire du parent par task_for_pid                                  attendu : refusé (kr=5)   obtenu : refusé (kr=5)
ok  sous sandbox : env du parent par sysctl KERN_PROCARGS2                             attendu : visible         obtenu : visible
ok  sous sandbox : env du parent par ps eww                                            attendu : refusé          obtenu : refusé
ok  sous sandbox : mémoire du parent par task_for_pid                                  attendu : refusé (kr=5)   obtenu : refusé (kr=5)
ok  (deny process-info* (target others)) : env par sysctl                              attendu : visible         obtenu : visible
ok  connexion au socket hors sandbox                                                   attendu : permis          obtenu : permis
ok  connexion au socket sous le profil                                                 attendu : refusé          obtenu : refusé
ok  faux socket dans le dossier des sockets                                            attendu : refusé          obtenu : refusé
ok  open d'une application locale, sans refus LaunchServices                           attendu : secret lu       obtenu : secret lu
ok  open d'une application locale, avec refus LaunchServices                           attendu : rien            obtenu : rien
ok  launchctl submit depuis la sandbox                                                 attendu : rien            obtenu : rien
ok  serveur screen hors sandbox, commande envoyée depuis la sandbox                    attendu : secret lu       obtenu : secret lu
ok  sous refus LaunchServices : /usr/bin/git                                           attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  sous refus LaunchServices : /usr/bin/python3                                       attendu : permis (rc=0)   obtenu : permis (rc=0)
48 essais, 0 écart(s)
```

La recherche sur les mécanismes (essais sur ce Mac : sandbox imbriquée, trousseau jetable, `/.vol`, `setsid`) a été faite par un agent pendant le constat. Les essais dont dépend une conclusion de ce document sont refaits par le script ci-dessus, sauf ceux du trousseau (E.2) et du `curl` (E.1), qui ne sont pas repris.

Sources lues par cette recherche :
- Codex (`github.com/openai/codex`, commit `ad7176f7878c`) ;
- sandbox-runtime (`github.com/anthropic-experimental/sandbox-runtime`, commit `0bbebed0aa9a`) ;
- docs.kernel.org/userspace-api/landlock.html ;
- pages man mount_namespaces(7), proc_pid_environ(5), proc_pid_mem(5), ptrace(2), seccomp_unotify(2), fanotify_init(2).
