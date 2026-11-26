# Ticket 41 : protection des secrets (YA-11, gate de YA-16) : constat et plan

Ticket CDC : YA-11, YA-16. Décision du 26/11/2026 (journal du CDC) : le modèle ne doit jamais pouvoir lire les identifiants de yusAgent. Le sandbox du kernel entier est abandonné. Permissions, réseau et reste du système restent libres, et l'environnement du kernel est inchangé (ticket 02).

Branche `ya-41-protection-secrets` depuis `main` à `0de7af7f3`. Ce document est un constat, sans code produit. Il est accompagné d'un script de mesure (`41-protection-secrets/mesure_macos.py`). Les lignes citées sont celles de l'arbre à `9010a9c21`.

Unharnessed (lu à `bb55464`) n'a rien sur ce point : aucun choix à reprendre, l'écart est couvert par l'exception sécurité.

## Résumé

- **Fichiers à protéger** : `auth.json` (clés API, OAuth des fournisseurs, jetons MCP `mcp:*`, clé Serper) avec son temporaire `auth.json.tmp<pid>` et son verrou `auth.json.lock`, plus `models.json` (`apiKey` et `headers` peuvent être littéraux). Pas d'autre secret trouvé dans `~/.yusagent` ni dans `<cwd>/.yusagent/`.
- **Point d'héritage** : `crates/pa-core/src/kernel/manager/startup.rs:168-181`, fichier upstream déjà modifié par le fork. Une enveloppe autour de `Command::new(&python)` y couvre le kernel et tous ses descendants.
- **macOS, mesuré** : Seatbelt (`sandbox-exec`, `(allow default)`) avec un refus par regex de préfixe sur `auth.json*` et `models.json*` tient, fichiers laissés en place. 38 essais conformes, contournements compris (§E.1, annexe).
  - Pour que ça tienne, le kernel doit aussi être coupé du socket du démon : `ExecuteBash` lance un shell **dans le worker**, hors de la protection.
- **Linux, documenté et non mesuré** : masquer un fichier ne survit pas à l'écriture atomique du démon, il faut masquer un **dossier**. Il faudrait donc déplacer `auth.json` dans un dossier à lui. bubblewrap ou un espace de noms demandent les user namespaces non privilégiés, restreints par défaut sur Ubuntu 24.04. Une sonde en CI s'impose avant de choisir.
- **Jetons MCP** : ils sont aujourd'hui **dans la mémoire du kernel**. Le Python lit `auth.json` et construit l'en-tête lui-même (`mcp.py:1088`, `:967`, `:975`). Il faut déplacer l'appel HTTP authentifié dans le worker.
- **Variables d'environnement (G)** : sur macOS, `sysctl(KERN_PROCARGS2)` lit depuis la sandbox l'environnement de tout processus parent (mesuré). Aucune règle Seatbelt essayée ne l'empêche. Je recommande une limite documentée.
- **Recommandation** : macOS d'abord (PR A : Seatbelt et socket), puis les jetons MCP hors du kernel (PR B), puis Linux après la sonde (PR C). Les questions sont au §L.

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

Profil essayé (annexe, 38 essais, 0 écart, macOS 27.0.1) :

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
- Autres clés qui feraient exécuter ou envoyer quelque chose par le worker (`packages`, `npmCommand`, fournisseurs) : non vérifiées une à une (§J).

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

- Clés passées par variable d'environnement : lisibles sur macOS (G-b), sous Linux sans espace de noms PID.
- Clés rangées sous forme `!commande` (trousseau, gestionnaire de mots de passe) : le kernel peut relancer la même commande.
- Identifiants de l'utilisateur hors de yusAgent : `~/.aws`, identifiants Google par défaut (`GOOGLE_APPLICATION_CREDENTIALS`, `env_allowlist.rs:128`), `~/.ssh`, identifiants git, trousseau via `/usr/bin/security`.
- Clés rendues volontairement au kernel : variables de `kernel.envAllowlist`, clé Serper selon la question 2, `env` des serveurs MCP stdio fournis par le client ACP (question 4).
- Lien physique créé avant la protection : jetons déjà lus valides jusqu'à expiration (F.1).
- Réglages écrits par le modèle et exécutés par le worker (F.4), selon la question 9.
- Le modèle peut toujours **utiliser** les identifiants par les voies prévues (appels MCP par le relais, websearch), sans les lire.
- Rien n'empêche le kernel de supprimer des sessions, d'arrêter le démon ou d'envoyer des données par le réseau : hors de l'objectif.
- Sur macOS, dépendance à `sandbox-exec`, déprécié mais présent.
- Plateformes où le mécanisme manque (userns refusés, `sandbox-exec` absent) : selon la question 8.

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

## M. Plan de PR (après validation)

Chaque PR a son push 1 de tests seuls, rouges sur leur assertion, puis le code. Commits `YA-11: ticket 41 : …`. Tout fichier upstream touché va dans `docs/fork-changes.md`.

- **PR A, macOS : protection des fichiers et du socket (K1)**
  - Push 1 : tests 1, 2, 3 et 7, et le nouveau job CI `macos-15`.
  - Push 2 :
    - module du fork `crates/pa-core/src/kernel/ya_secret_guard.rs` : profil, chemins canoniques, réglage selon la question 7 ;
    - enveloppe à `startup.rs:168-170`, 3 à 5 lignes upstream ;
    - exception `ps`/`top` selon la question 10 ;
    - contrôle `st_nlink` au démarrage du démon ;
    - adaptation du banc selon la question 7.
  - Captures et script de contrôle lancés sur le binaire de la branche (règle 13).
  - Préfixe inchangé.
- **PR B : jetons MCP hors du kernel (C1)**
  - Push 1 : tests 4 et 5.
  - Push 2 :
    - transport Python du fork ;
    - handler Rust du fork ;
    - `mcp.py` et `mcp_base.py` cessent de lire `auth.json` ;
    - `mcp.config` sans `headers` littéraux ;
    - websearch selon la question 2.
  - Peut précéder la PR A : sans elle, la PR A casse le MCP authentifié (le kernel ne lit plus `auth.json`). **Ordre proposé : B puis A**, ou les deux avant toute remise.
- **PR C, Linux (K3)**
  - Avant le code : sonde CI (un job qui essaie `bwrap` et `unshare -Ur` sur `ubuntu-24.04`, avec et sans sysctl), et relevé.
  - Puis déplacement d'`auth.json` (question 6), masquage du dossier et du dossier des sockets, `--unshare-pid`, test 6.
- **Doc** : avertissement « ce qui reste ouvert » (§J) dans le document que Yusuf indiquera. Gate de YA-16 cochée après les PR exigées par la question 5.

## Annexe : mesures macOS

Commande : `python3 -I docs/tickets/41-protection-secrets/mesure_macos.py`. Elle travaille dans un dossier temporaire avec des secrets factices, sans lire `~/.yusagent` ni appeler de modèle. Sortie du lancement par Claude Code (26/11/2026, arbre à `9010a9c21`) :

```
ProductName:		macOS ; ProductVersion:		27.0.1 ; BuildVersion:		26A434
ok  cat auth.json                                                      attendu : refusé          obtenu : refusé
ok  python open(auth.json)                                             attendu : refusé          obtenu : refusé
ok  cat models.json                                                    attendu : refusé          obtenu : refusé
ok  lire kernel-venv/f                                                 attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  écrire settings.json                                               attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  créer un fichier dans le dossier                                   attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  planter auth.json.tmp123                                           attendu : refusé          obtenu : refusé
ok  créer le verrou auth.json.lock                                     attendu : refusé          obtenu : refusé
ok  remplacer models.json par renommage                                attendu : refusé          obtenu : refusé
ok  renommer le dossier agent                                          attendu : refusé          obtenu : refusé
ok  renommer son parent                                                attendu : refusé          obtenu : refusé
ok  lien symbolique puis cat                                           attendu : refusé          obtenu : refusé
ok  lien physique créé dans la sandbox                                 attendu : refusé          obtenu : refusé
ok  variante de chemin …/regex/home/.ya/AUTH.JSON                      attendu : refusé          obtenu : refusé
ok  variante de chemin …/regex/home/.ya/../.ya/auth.json               attendu : refusé          obtenu : refusé
ok  variante de chemin /System/Volumes/Data…/regex/home/.ya/auth.json  attendu : refusé          obtenu : refusé
ok  accès par inode /.vol/<dev>/<ino>                                  attendu : refusé          obtenu : refusé
ok  pendant l'écriture : cat du temporaire                             attendu : refusé          obtenu : refusé
ok  pendant l'écriture : lien physique du temporaire                   attendu : refusé          obtenu : refusé
ok  après renommage : cat auth.json                                    attendu : refusé          obtenu : refusé
ok  descendant : setsid + sh -c cat                                    attendu : refusé          obtenu : refusé
ok  sandbox imbriquée (sandbox-exec dans sandbox-exec)                 attendu : refusé (rc=71)  obtenu : refusé (rc=71)
ok  exécuter /bin/ps (setuid)                                          attendu : refusé          obtenu : refusé
ok  exception (with no-sandbox) pour /bin/ps                           attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  contrôle positif : cat auth.json hors sandbox                      attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  literal : lien physique du temporaire pendant l'écriture           attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  sans règle sur les ancêtres : mv du dossier puis cat               attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  lien physique créé AVANT, hors sandbox                             attendu : permis (rc=0)   obtenu : permis (rc=0)
ok  hors sandbox : env du parent par sysctl KERN_PROCARGS2             attendu : visible         obtenu : visible
ok  hors sandbox : env du parent par ps eww                            attendu : visible         obtenu : visible
ok  hors sandbox : mémoire du parent par task_for_pid                  attendu : refusé (kr=5)   obtenu : refusé (kr=5)
ok  sous sandbox : env du parent par sysctl KERN_PROCARGS2             attendu : visible         obtenu : visible
ok  sous sandbox : env du parent par ps eww                            attendu : refusé          obtenu : refusé
ok  sous sandbox : mémoire du parent par task_for_pid                  attendu : refusé (kr=5)   obtenu : refusé (kr=5)
ok  (deny process-info* (target others)) : env par sysctl              attendu : visible         obtenu : visible
ok  connexion au socket hors sandbox                                   attendu : permis          obtenu : permis
ok  connexion au socket sous le profil                                 attendu : refusé          obtenu : refusé
ok  faux socket dans le dossier des sockets                            attendu : refusé          obtenu : refusé
38 essais, 0 écart(s)
```

La recherche sur les mécanismes (essais sur ce Mac : sandbox imbriquée, trousseau jetable, `/.vol`, `setsid`) a été faite par un agent pendant le constat. Les essais dont dépend une conclusion de ce document sont refaits par le script ci-dessus, sauf ceux du trousseau (E.2) et du `curl` (E.1), qui ne sont pas repris.

Sources lues par cette recherche :
- Codex (`github.com/openai/codex`, commit `ad7176f7878c`) ;
- sandbox-runtime (`github.com/anthropic-experimental/sandbox-runtime`, commit `0bbebed0aa9a`) ;
- docs.kernel.org/userspace-api/landlock.html ;
- pages man mount_namespaces(7), proc_pid_environ(5), proc_pid_mem(5), ptrace(2), seccomp_unotify(2), fanotify_init(2).
