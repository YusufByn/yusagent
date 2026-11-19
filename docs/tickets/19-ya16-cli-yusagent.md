# Ticket 19 : YA-16, CLI renommé yusAgent (phase 1 : constat)

Demande de Yusuf du 19/11/2026. **Phase 1 : constat seulement.** Branche `ya-19-cli-yusagent`, depuis `main` à `eb6548f49`. Aucun code modifié, rien compilé, aucun appel de modèle. La PR contient ce fichier et une ligne du journal du CDC : **préfixe inchangé**, aucun fichier upstream touché. Pas de merge sans « merge » de Yusuf.

Le constat (sections 1 à 8 et « Options ») ne tranche aucune option. Les choix de Yusuf sont dans la section suivante.

## Décisions de Yusuf (19/11/2026)

Prises sur le constat ci-dessous ; elles remplacent les options non retenues.

- **Dossier** : `~/.yusagent` pour l'utilisateur, `<cwd>/.yusagent/` pour le projet. **Pas de migration** : une commande `cp` documentée. Le dossier des sockets `$TMPDIR/prime-agent-<uid>` est renommé aussi. Le venv du kernel et le runtime Python suivent le chemin passé par l'hôte : plus de repli sur `~/.prime/agent` (`layout.rs:28`, `:36` et les 5 replis Python du § 2).
- **Binaire** : cible Cargo `prime-agent` **gardée** ; fichier renommé `yusagent` à l'assemblage de l'archive (option (b)). `is_product_binary` (`crates/pa-daemon/src/boot_reap.rs:370-377`) corrigé. **Pas d'alias.**
- **Commande `update` désactivée**, avec un message clair. Précédent : Unharnessed a retiré la sous-commande `update` dans `75cfc357fe501a46f4a01a345457c0897eb88d89` (01/10/2026, « unharnessed step 1a: drop the terminal client subcommands » : « the self-update and package manager commands (update, package) […] go »), puis le module de mise à jour dans `09f760dfa4b9fbb205443f6fcf7736256ccc226f` (01/10/2026, « unharnessed step 7: drop auto-update and the package manager »). Ici, on désactive au lieu de supprimer, pour garder les cherry-picks faciles.
- **Variables `PRIME_AGENT_*` et prompt v1 : inchangés.**
- **Interface** : logo et marque Prime Agent retirés. Noms du provider Prime Intellect gardés. Login Claude laissé tel quel (règle 7). **Partage de traces vers Prime Intellect désactivé.**
- **README** : hors du ticket 19. Il sera refait en entier plus tard, avec la direction artistique de yusAgent.
- **ACP** : espace de noms `_meta` `ai.primeintellect.prime-agent` gardé. Nom d'agent dans `initialize` : yusAgent. Les ajouts futurs vont sous `_yus/*`.
- **Découpage** :
  1. **PR 1, interface** : textes, logo, `agentInfo` ACP, partage de traces.
  2. **PR 2, binaire** : archive, artefact CI, `install-bin.sh`, `ab.py`, `is_product_binary`, `update` ; le banc dans le même push.
  3. **PR 3, dossier** : `~/.yusagent`, `<cwd>/.yusagent/`, socket, venv du kernel, runtime Python.
- **Vérification après les PR 2 et 3** : build macOS de la branche (`ya-build-macos-arm64.yml`), puis run du banc en `--capture` sur une tâche, pour vérifier `-p`, l'isolement du banc et le préfixe (`crates/pa-core/tests/prefix_stability.rs`). Yusuf lance la commande ; Claude la lui donne, exacte.
- **Avant chaque push** : dire ce qui change dans les requêtes (préfixe, cache), même si rien ne change.
- **`--version`** (19/11/2026) : inchangé. Il n'affiche que la version, sans mention de Prime Agent (`crates/pa-cli/src/lib.rs:151-153`), et `scripts/release/verify_release.py:211` compare cette sortie à l'identique. Un numéro de version propre à yusAgent viendra avec la distribution (YA-16).

## PR 1 : interface (19/11/2026)

Branche `ya-19-interface`, depuis `main` à `9d53d3775`. Fichiers et tests touchés : `docs/fork-changes.md`, section « Renommage du CLI, PR 1 ».

- **Marque** : « Prime Agent » remplacé par « yusAgent » dans les textes affichés (splash, accueil, messages du démon et leurs fonctions de reconnaissance, terminal, télémétrie, `/quit`, rapport d'incident, installation du kernel, pages de login MCP et OAuth générique, consignes MCP du runtime et de `websearch`). Les mentions de la **commande** `prime-agent` (`APP_NAME`, « Run: prime-agent shutdown --force », « Attach instead: prime-agent … ») restent pour la PR 2, avec le nom du binaire.
- **Logo** : le papillon est retiré du splash (`crates/pa-tui/src/chrome.rs`) et de l'écran d'accueil (`crates/pa-tui/src/onboarding.rs`). Sa zone reste **vide, à la même taille** : la géométrie ne change pas (nombre de lignes du splash, colonne des métadonnées), donc les tests de mise en page restent valides sans compilation locale, et la place attend la marque de la DA yusAgent. Effet visible à l'accueil : le fond animé ne dessine pas sur les anciennes cellules du papillon, une silhouette vide peut se deviner parmi les points.
- **ACP** : `agentInfo` = `yusagent` / `yusAgent` ; `_meta` inchangé.
- **Traces** : coupure dans le transport réel `ReqwestTraceHttp::put` (choix de Yusuf, option 1), seul chemin par lequel une trace quitte la machine ; refus sans connexion, non rejoué. En plus, `run_service` ne démarre ni la livraison ni la reprise du mode continu : sans cela, `agentTraces.enabled: true` relancerait l'envoi toutes les 60 s et écrirait une ligne de journal à chaque tour (`crates/pa-core/src/agent_traces/continuous.rs:44-60`, `crates/pa-core/src/agent_traces.rs:714-722`). `/traces on` est refusé, l'état affiché est « off », la question de l'onboarding n'est plus posée.
- **Télémétrie distante** (ajout de Yusuf, 19/11/2026) : coupée au transport du sink, `AnalyticsSink::send_batch` (`crates/pa-telemetry/src/sinks/analytics.rs`), sans connexion, sans réglage pour la rallumer. Unharnessed l'a retirée avec l'envoi de traces dans `de437b6e2d3e945d4f56dbb675f5da692c5726ab` (« unharnessed step 6: drop telemetry and trace upload »). Le journal d'usage local YA-13 (`<id>.usage.ndjson`) ne passe pas par les sinks (écriture directe, `crates/pa-core/src/session_engine/usage_log.rs:539-547`) : intact. Le miroir local `telemetry.jsonl` suit toujours `telemetry.enabled`. Reste trompeur, hors de cette PR : en build release, `/telemetry status` affiche encore l'endpoint de Prime Intellect, et la notice dit « yusAgent sends pseudonymous usage… » (elle n'apparaît que si l'on rallume la télémétrie).
- **Laissés** : messages de la mise à jour (PR 2), nom du service « Prime Agent Traces » (provider Prime Intellect), login Claude (règle 7), prompts de `/refine` et de reprise après mise à jour (texte envoyé au modèle), docstrings Python, thème intégré nommé `prime`.

## Décision de référence

`docs/yusAgent-cdc.md:37` (journal, 08/11/2026) : « CLI : avant les testeurs, CLI renommé yusAgent (binaire `yusagent`, dossier `~/.yusagent`, version et aide, plus de mention de Prime Agent dans l'interface). […] sans casser le mode non interactif (`-p`) ni les options utilisées par `bench/smoke/run.py`. »

Contraintes associées : copyrights de Prime Agent et pi conservés dans LICENSE (`docs/yusAgent-cdc.md:478`) ; cherry-picks upstream peu coûteux (`CLAUDE.md`, règle 2 ; `docs/yusAgent-cdc.md:27`) ; login par abonnement Claude intouché (`CLAUDE.md`, règle 7).

## Résultats en bref

- **Le dossier actuel n'est pas `~/.prime-agent` mais `~/.prime/agent`.** Le nom est défini dans **4 constantes Rust** et codé en dur à **2 endroits du venv du kernel** et **5 replis Python**. Le même nom sert aussi au dossier de projet `<cwd>/.prime/agent/`.
- **Binaire** : `[[bin]] name = "prime-agent"` (`crates/pa-cli/Cargo.toml:23`). Renommer la cible Cargo touche **62 `CARGO_BIN_EXE_prime-agent` dans 40 fichiers de tests upstream** et **9 lignes `--bin prime-agent`** dans les workflows et le `Makefile`.
- **Mentions dans le code** : 1 839 lignes hors commentaires `//` contiennent le nom dans `crates/*/src` et `crates/*/tests` (dont 1 629 dans un littéral de chaîne). Ce compte inclut les modules de test en ligne. Les fixtures ACP (`crates/pa-daemon/testdata`, 28 fichiers) en contiennent 6 311, presque toutes l'espace de noms `_meta`.
- **Variables d'environnement** : **101 jetons `PRIME_AGENT_*` distincts** (dont 18 `PRIME_AGENT_INTERNAL_*` ; quelques jetons sont des noms de constantes ou des préfixes), plus 10 `PI_*`. Une seule a été créée par le fork : `PRIME_AGENT_PROMPT_VERSION`.
- **Prompt** : le prompt v2 (défaut) ne contient pas le nom dans son texte statique, qui commence déjà par `# yusAgent`. Sa queue cite **deux variables** (`PRIME_AGENT_CONVERSATION_LOG`, `PRIME_AGENT_BUNDLED_SKILLS_DIR`). Les renommer **change le préfixe v2**. Le prompt v1 contient « prime-agent » sur 9 lignes de `core.md`.
- **Banc** : `run.py` ne dépend ni du nom du binaire ni du dossier par défaut (il passe `--binary` et fixe `PRIME_AGENT_CODING_AGENT_DIR`). `ab.py`, `install-bin.sh` et le workflow de build supposent le nom `prime-agent`, ainsi que le nom de l'artefact.
- **Deux pièges hors du texte** : le nettoyage des workers orphelins reconnaît le binaire **par son nom de fichier** (`crates/pa-daemon/src/boot_reap.rs:376`) ; un binaire installé par upstream sur la même machine **partage le dossier et le socket du démon** (`$TMPDIR/prime-agent-<uid>`).

## Méthode

Lecture du code à `eb6548f49`, aucune exécution. Commandes de comptage (depuis la racine du dépôt) :

```sh
# Lignes hors commentaires `//` par crate (tests en ligne compris)
P='prime agent|prime-agent|prime_agent|primeagent'
for c in crates/*/; do
  grep -rniE "$P" $c --include='*.rs' | grep -vE '^[^:]+:[0-9]+:\s*//' | wc -l
done
# Fixtures
grep -rciE "$P" crates/*/testdata | awk -F: '{s+=$2} END {print s}'
# Jetons PRIME_AGENT_* distincts
git grep -h -o -E 'PRIME_AGENT_[A-Z0-9_]*[A-Z0-9]' -- . ':!crates/pa-daemon/testdata' ':!bench/smoke/runs' ':!docs' | sort -u | wc -l
# Tests qui lancent le binaire par son nom Cargo
git grep -c 'CARGO_BIN_EXE_prime-agent' -- crates
# Sous-chemins du dossier de l'agent
grep -rnoE 'agent_dir(\(\))?\s*\.join\("[^"]+"\)' crates --include='*.rs' | grep -v 'tests\?\.rs\|/tests/'
```

Comptes par crate (lignes / dont littéraux) : `pa-cli` 693 / 669, `pa-core` 427 / 328, `pa-daemon` 377 / 294, `pa-tui` 250 / 247, `pa-types` 67 / 67, `pa-models` 9 / 9, `pa-perf` 7 / 6, `pa-ai` 6 / 6, `pa-telemetry` 3 / 3, `pa-agent` 0, `ya-apply-patch` 0.

## 1. Mentions visibles par l'utilisateur

### Nom du binaire, aide, version

| Surface | Où | Constat |
|---|---|---|
| Nom du binaire | `crates/pa-cli/Cargo.toml:23` (`[[bin]] name = "prime-agent"`), `:6` (description) | Nom de la cible Cargo, donc du fichier produit. |
| Nom affiché | `crates/pa-cli/src/config.rs:5` (`APP_NAME = "prime-agent"`) | 51 usages dans `crates/` (aide, usages, messages). |
| `--help` | `crates/pa-cli/src/command_registry.rs:551-563` | En-tête « {APP_NAME} - AI coding assistant… », usages et pied construits sur `APP_NAME`. |
| Aide des commandes | `command_registry.rs:124` (« Prime Agent sends pseudonymous usage… », aide de `telemetry`), `:221-229` (aide de `update` : URL `app.primeintellect.ai/prime-agent/install.sh`, « configuration (~/.prime/agent) », « Restart prime-agent ») ; `crates/pa-cli/src/package_command.rs:366`, `:369` (cite `CONFIG_DIR_NAME`) | Texte en dur, hors `APP_NAME`. |
| `--version` | `crates/pa-cli/src/lib.rs:151-153`, `crates/pa-cli/src/config.rs:51-57` | N'imprime **que** la version (manifeste `package.json` à côté du binaire, sinon `CARGO_PKG_VERSION`), sans nom. La décision du 08/11 demande « version et aide » : le nom est à ajouter, pas à remplacer. |
| Drapeau caché | `crates/pa-cli/src/lib.rs:106` (`--prime-agent-bootstrap`), utilisé par `.github/workflows/ci.yml:346` | Interne (bootstrap du kernel en CI). |

### TUI

| Où | Texte |
|---|---|
| `crates/pa-tui/src/chrome.rs:378` | Écran d'accueil : `title = "prime agent"` (suivi de la version, `chrome.rs:44`). |
| `crates/pa-tui/src/chrome.rs:12` | Logo `PRIME_COMPACT_BUTTERFLY_LOGO` (le papillon de Prime Intellect). |
| `crates/pa-tui/src/onboarding.rs:327-333` | « Welcome to **PRIME** Agent ». Le test d'or `onboarding.rs:576` fige ce rendu. |
| `crates/pa-tui/src/onboarding.rs:39` | « Which account should Prime Agent use? » |
| `crates/pa-tui/src/exit_restore.rs:132`, `crates/pa-tui/src/exit_guard.rs:252` | « Prime Agent: repaired a raw terminal… », « Prime Agent: shutdown stalled… » |
| `crates/pa-tui/src/session_ui/notes.rs:190` | Notice de télémétrie (« Prime Agent sends… », cite `PRIME_AGENT_TELEMETRY=0`). |
| `crates/pa-tui/src/session_ui/apply.rs:296` | « Prime Agent is updating — restarting the daemon… » |
| `crates/pa-tui/src/daemon_client.rs:342`, `:348`, `:430`, `:677`, `:683` ; `crates/pa-tui/src/update_restart_wait.rs:21`, `:94` (et les préfixes reconnus `:47-61`) | Erreurs « … the Prime Agent daemon … ». `update_restart_wait.rs` reconnaît ces messages par leur début : le texte et sa reconnaissance doivent changer ensemble. |
| `crates/pa-tui/src/session_open_error.rs:169`, `:199`, `:204` | Commandes suggérées : `prime-agent --resume …`, `prime-agent agents`. |
| `crates/pa-tui/src/incident_notices.rs:28` | « run prime-agent incident for the timeline ». |
| `crates/pa-tui/src/clipboard_image.rs:163`, `:222` ; `crates/pa-tui/src/external_editor.rs:33` | Fichiers temporaires `prime-agent-clip-*.png`, `prime-agent-editor-*.md` (le second est visible dans l'éditeur externe). |
| `crates/pa-tui/src/onboarding.rs:14`, `crates/pa-tui/src/onboarding_flow.rs:21`, `:24` | « Share agent traces with Prime Intellect? », « Log in with Prime Intellect » : nom de l'**entreprise** et du provider, pas du produit (voir § 8, question ouverte). |

### Messages d'erreur et sorties hors TUI

- `crates/pa-cli/src/*` : 669 lignes de littéraux (tests en ligne compris). Exemple : `crates/pa-cli/src/interactive_mode/daemon.rs:64` (« resolve the prime-agent executable »).
- Erreurs que **le modèle** peut lire (sortie d'outil, pas le préfixe) : `crates/pa-core/src/kernel/bootstrap/runtime_code.rs:61` (« Remove ~/.prime/agent/kernel-venv so prime-agent can rebuild it »), `crates/pa-core/src/kernel/bootstrap/venv/layout.rs:58`, `prime-agent-runtime/src/rlm/bash.py:1407` (`PRIME_AGENT_BASH_SHELL`), `prime-agent-runtime/src/rlm/mcp_base.py:47` (« run `/mcp login` … in Prime Agent »).
- Pages de fin de login OAuth (navigateur) : `crates/pa-ai/src/oauth/callback.rs:323`, `:327` (« Prime Agent authentication completed/failed »). Couche providers (règle 3).
- **Login par abonnement Claude, à ne pas toucher (règle 7)** : `crates/pa-ai/src/oauth/anthropic_callback.rs:368`, `:372` et l'avertissement `crates/pa-tui/src/session_ui.rs:118` (« Prime Agent identifies as Claude Code… »).

### Mode ACP

| Où | Constat |
|---|---|
| `crates/pa-daemon/src/acp/types.rs:401-405` | `initialize` → `agentInfo: { name: "prime-agent", title: "Prime Agent", version }`. Test `types.rs:428`. |
| `crates/pa-daemon/src/acp/meta.rs:10` | Espace de noms `_meta` : `ai.primeintellect.prime-agent`, sur toutes les notifications enrichies (`meta.rs:246-247`, et dans `initialize`, `types.rs:406`). Ce n'est pas un texte affiché, c'est **une clé de protocole** : l'extension (YA-10, pas encore écrite) la lira. |
| `crates/pa-daemon/testdata/acp/*.jsonl` (28 fichiers) | 28 lignes portent `agentInfo` (titre « Prime Agent ») ; 6 307 occurrences de l'espace de noms. |

## 2. Chemins de données

### Racine : `~/.prime/agent`

Le nom est défini quatre fois, à l'identique :

- `crates/pa-cli/src/config.rs:7` ; résolution `get_agent_dir()` `config.rs:104-111` (variable `PRIME_AGENT_CODING_AGENT_DIR`, sinon `~/.prime/agent`).
- `crates/pa-core/src/settings/storage.rs:12` (réglages de projet, `storage.rs:69`).
- `crates/pa-core/src/skills/loader.rs:27`.
- `crates/pa-daemon/src/paths.rs:11` ; résolution `agent_dir()` `paths.rs:48-53`.

La même constante désigne aussi le **dossier de projet** `<cwd>/.prime/agent/` : `settings.json` (`storage.rs:69`), `SYSTEM.md` et `APPEND_SYSTEM.md` (`crates/pa-core/src/resources/mod.rs:282`, `:297`), skills (`loader.rs`), modèles de prompts (`crates/pa-core/src/skills/prompt_templates.rs:250`), paquets npm et git (`crates/pa-core/src/packages/npm.rs:105`, `crates/pa-core/src/packages/git.rs:37`), traces (`crates/pa-core/src/agent_traces/continuous.rs:74`). La décision du 08/11 ne dit rien du dossier de projet.

Contenu sous la racine (sous-chemins relevés par la commande de la méthode, hors tests) : `sessions/` (dont le journal d'usage `<session>.usage.ndjson`, `docs/usage.md`), `settings.json`, `models.json`, `auth.json`, `skills/`, `prompts/`, `logs/`, `session-artifacts/` (sessions des sous-agents), `sessions-archive/`, `session-leases/`, `daemon-workers/`, `worker-snapshot-cache/`, `agent-traces-outbox*`, `prime-inference-models-cache.json`, `prime-inference-private-models.json`, `keybindings.json`, `themes/`, `harness/refinements.jsonl`, `cron-jobs.json`, `mcp-services.json`, `telemetry.json(l)`, `git/`, `SYSTEM.md`, `APPEND_SYSTEM.md`, `update-restarts/`, `daemon-update-restart(s)`, et `kernel-venv/` (ci-dessous). Les shells persistants de `bash()` y vivent aussi (`prime-agent-runtime/src/rlm/bash.py:1520`).

### Ce qui ne passe pas par la résolution commune

- **Venv du kernel** : `crates/pa-core/src/kernel/bootstrap/venv/layout.rs:28` code en dur `~/.prime/agent/kernel-venv`. Il **ignore `PRIME_AGENT_CODING_AGENT_DIR`** et ne suit que `PRIME_AGENT_KERNEL_VENV` (`layout.rs:24`). Repli XDG `~/.local/share/prime/agent/kernel-venv` (`layout.rs:36`). Même chemin dans `.github/workflows/ci.yml:373` et dans deux tests (`crates/pa-core/src/session_engine/ya_kernel_restart.rs:166`, `crates/pa-core/tests/prefix_stability.rs:168`).
- **Replis Python** quand la variable est absente : `prime-agent-runtime/src/rlm/harness.py:131`, `rlm/mcp.py:1078`, `rlm/mcp_base.py:61`, `rlm/factory.py:4815`, `skills/websearch/src/websearch/websearch.py:33`, tous `~/.prime/agent`. L'hôte ne transmet `PRIME_AGENT_CODING_AGENT_DIR` au kernel **que si elle est posée** (liste blanche, `crates/pa-core/src/kernel/env_allowlist.rs:66`). Si le défaut Rust change sans que l'hôte injecte la variable, le kernel lit l'ancien dossier (par exemple `auth.json` des MCP, `mcp_base.py:56-63`).
- **Hors du dossier de l'agent, à ne pas déplacer** : `~/.prime/config.json` est le fichier du CLI `prime` de Prime Intellect (`crates/pa-core/src/auth/prime_inference.rs:183-189`), un autre produit. `~/.prime/supervisor-owners` sert la migration depuis la version TypeScript (`crates/pa-cli/src/update_flow/legacy_admission.rs:27`).

### Socket du démon et installation

- Dossier des sockets : `$TMPDIR/prime-agent-<uid>/` (`crates/pa-daemon/src/platform/paths.rs:21`), repli `/tmp/prime-agent-<uid>` ; Windows : `prime-agent-user` (`:39`) et pipe `\\.\pipe\prime-agent-daemon` (`:54`). Surcharge : `--daemon-socket` ou `PRIME_AGENT_DAEMON_SOCKET` (`crates/pa-cli/src/config.rs:17-31`).
- **Coexistence avec upstream** : un Prime Agent upstream installé sur la même machine utilise le même `~/.prime/agent` et le même socket. Les deux produits partagent alors réglages, sessions et `auth.json`, et l'un peut se connecter au démon de l'autre.
- Disposition d'installation : `install-rust.sh:5-7` (`~/.local/bin/prime-agent`, `~/.local/share/prime-agent/`) ; marqueur `prime-agent-native-v1` et lanceur `prime-agent` (`crates/pa-core/src/update/install.rs:81`, `:84-90`, `:93`) ; détection Homebrew par le nom de dossier (`crates/pa-core/src/update/homebrew.rs:35`).

### Détection du binaire par son nom

- `crates/pa-daemon/src/boot_reap.rs:370-377` : `is_product_binary` n'accepte que `prime-agent` et `pa-daemon`. Il sert au nettoyage des workers orphelins au démarrage (`boot_reap.rs:315`, `:320`, `:454`, `:500`). **Un binaire nommé `yusagent` n'y serait plus reconnu** : les workers d'un démon mort ne seraient plus récupérés (Linux ; `cfg_attr(not(target_os = "linux"), allow(dead_code))`).
- `crates/pa-daemon/src/hold_refusal.rs:100-150`, `:306` : classe le processus qui tient un fichier de session d'après les noms `prime-agent`, `prime-agent-rust`, `prime-agent.old.*`. Avec un autre nom, le message de refus serait moins précis, sans erreur.
- Le démon et les workers se relancent par `current_exe()` (`crates/pa-cli/src/interactive_mode/daemon.rs:64`, `crates/pa-daemon/src/supervisor/supervision.rs:573`), donc sans dépendre du nom.

## 3. Variables d'environnement

101 jetons `PRIME_AGENT_*` distincts (commande de la méthode). Classement :

| Groupe | Variables (définition) | Qui s'en sert hors du cœur |
|---|---|---|
| Publiques, utilisées par nous | `PRIME_AGENT_CODING_AGENT_DIR` (`crates/pa-cli/src/config.rs:9`, `crates/pa-daemon/src/paths.rs:9`), `PRIME_AGENT_SESSION_DIR` (`config.rs:11`) et son ancien nom `PRIME_AGENT_CODING_AGENT_SESSION_DIR` (`config.rs:43`), `PRIME_AGENT_KERNEL_VENV` (`layout.rs:24`), `PRIME_AGENT_KERNEL_PYTHON` (`crates/pa-core/src/kernel/bootstrap/mod.rs:143`), `PRIME_AGENT_DAEMON_SOCKET` (`config.rs:17`), `PRIME_AGENT_TELEMETRY` (`crates/pa-cli/src/mode.rs:295`), `PRIME_AGENT_FAUX_SCRIPT` (`crates/pa-cli/src/print_runtime.rs:195`) | banc (`bench/smoke/run.py:292-293`, `bench/tools/capture/run.py:122`, `bench/smoke/tools/notice-batch-repro.sh:25-28`), CI (`ci.yml:373-375`), docs |
| Créée par le fork | `PRIME_AGENT_PROMPT_VERSION` (`crates/pa-core/src/session_engine/ya_request.rs:130`, YA-09) | `.cargo/config.toml:13`, `docs/usage.md:124`, `:127` |
| Citées dans le prompt | `PRIME_AGENT_CONVERSATION_LOG` (`crates/pa-core/src/prompts/session_paths.rs:14`), `PRIME_AGENT_BUNDLED_SKILLS_DIR` (`session_paths.rs:17`) | runtime (`prime-agent-runtime/src/rlm/masked.py:26`), tests de préfixe (`crates/pa-core/tests/prefix_stability.rs:41`) |
| Internes hôte ↔ worker ↔ kernel | 18 `PRIME_AGENT_INTERNAL_*` (`crates/pa-daemon/src/worker/env.rs:50-68`, `crates/pa-daemon/src/lease.rs:13-14`, préfixe `crates/pa-types/src/daemon/launch_env.rs:28`), `PRIME_AGENT_BASH_SHELL`, `PRIME_AGENT_BASH_COMMAND_PREFIX` (`crates/pa-core/src/kernel/provisioner.rs:1100`, `:1106`), `PRIME_AGENT_KERNEL_OWNER_PID` (`env_allowlist.rs:72`) | runtime Python (`bash.py:1398`, `:1418`, `:1767-1768`, `repl.py:1627`) |
| Mise à jour et installation | `PRIME_AGENT_RUST_PREFIX`, `PRIME_AGENT_RUST_INSTALLER_URL`, `PRIME_AGENT_RELEASE_CHANNEL` (`crates/pa-core/src/update/installer.rs:24-30`), `PRIME_AGENT_DOWNLOAD_BASE_URL` (`crates/pa-cli/src/update_flow/update_command.rs:76`), `PRIME_AGENT_UPDATE_*` (`crates/pa-types/src/daemon/update_flow/budget.rs:138-160`) | `install-rust.sh`, `install.ps1` |
| Skills | `PRIME_AGENT_WEBSEARCH_TIMEOUT`, `PRIME_AGENT_WEBSEARCH_NUM_RESULTS` (`skills/websearch/SKILL.md:21`), `PRIME_AGENT_MACHINES_DIR` (`prime-agent-runtime/src/rlm/factory.py:4804`) | |

Autres noms hérités :

- `PI_*` (héritage de pi) : `PI_OFFLINE` (`crates/pa-cli/src/config.rs:81`), `PI_CACHE_RETENTION` (`crates/pa-ai/src/providers/openai_responses.rs:39`), `PI_SKIP_VERSION_CHECK` (`crates/pa-core/src/update/release.rs:77`), `PI_PACKAGE_DIR` (`config.rs:63`), `PI_CODING_AGENT_DIR` (`env_allowlist.rs:67`), `PI_OAUTH_CALLBACK_HOST` (`crates/pa-ai/src/oauth/callback.rs:11`), `PI_REQUEST_TIMING`, `PI_SHARE_VIEWER_URL`, `PI_STARTUP_BENCHMARK`, `PI_STDIN_TIMEOUT_MS`. `CLAUDE.md` (« Boucle de dev », point 5) et le banc imposent `PI_OFFLINE` et `PI_CACHE_RETENTION`.
- `PRIME_API_KEY`, `PRIME_TEAM_ID`, `PRIME_INFERENCE_*` : identifiants du provider Prime Inference, pas du produit.
- `.cargo/config.toml` : `DO_NOT_TRACK` (`:5`), `RLM_MAX_DEPTH` (`:9`), `PRIME_AGENT_PROMPT_VERSION` (`:13`). Seule la troisième porte le nom hérité.

## 4. Prompt système

Assemblage : `crates/pa-core/src/prompts/system_prompt.rs:128-308`. Le texte statique vient en premier, puis la queue (paquets, contexte du projet, skills, MCP, environnement, rôle). La queue ne change pas d'une requête à l'autre dans une session : elle fait partie du préfixe mis en cache.

**Version 2 (défaut)**

- Texte statique `crates/pa-core/src/prompts/ya_prompt_v2/core.md` : une seule occurrence cherchée, `core.md:1` « # yusAgent ». **Aucune mention de Prime Agent ni du nom du binaire.** Description du tool `python_repl` sans nom (`crates/pa-core/src/prompts/ya_prompt_v2.rs:110-112`).
- Queue, ce qui porte le nom hérité :
  - note de l'inventaire des skills « Expand `$PRIME_AGENT_BUNDLED_SKILLS_DIR`… » (`ya_prompt_v2.rs:72-73`) et emplacements `$PRIME_AGENT_BUNDLED_SKILLS_DIR/<nom>/SKILL.md` (construits par `session_paths.rs:67-70`) ;
  - section environnement : « …`os.environ["PRIME_AGENT_CONVERSATION_LOG"]`… » (`session_paths.rs:25`) ;
  - emplacement absolu des skills utilisateur ou projet, sous `~/.prime/agent/skills/` ou `<cwd>/.prime/agent/skills/` (`session_paths.rs:52-53` ; exemple dans le test `ya_prompt_v2.rs:456`), seulement s'il en existe.

**Version 1** (sessions d'avant le ticket 11, `promptVersion: 1`, figée octet pour octet)

- `crates/pa-core/src/prompts/layers/core.md:1`, `:3`, `:29`, `:75`, `:139`, `:146`, `:154`, `:163`, `:175` : « prime-agent » sur 9 lignes. `usage.md`, `opinionated.md` et `model_prompts.toml` : 0.
- `crates/pa-core/src/prompts/ya_tools_prompt.rs:24`, `:37-39` : variantes de la ligne 3 par famille de tools (« The prime-agent harness has… »).
- Queue : `session_paths.rs:20` et `:25` ; descriptions de skills intégrées à l'inventaire, dont `skills/skill-creator/SKILL.md:3` (« Prime Agent skills ») et `skills/prime-intellect/SKILL.md:3` (masquées en v2 : `ya_prompt_v2.rs:78`).

**Effet sur le préfixe**

| Changement | v2 | v1 |
|---|---|---|
| Nom du binaire, aide, version, TUI, ACP, messages | inchangé | inchangé |
| Renommer `PRIME_AGENT_CONVERSATION_LOG` ou `PRIME_AGENT_BUNDLED_SKILLS_DIR` | **change** | **change** |
| Changer le dossier `~/.prime/agent` | change **seulement** pour une session qui a des skills utilisateur ou projet (chemin absolu) | idem |
| Remplacer « prime-agent » dans `core.md` v1 | sans objet | **change** (et casse la promesse « octet pour octet » du ticket 11) |

Tests qui figent ces textes : `crates/pa-core/src/prompts/system_prompt.rs:512`, `:608`, `crates/pa-core/tests/prefix_stability.rs:41`, `:1475`, `crates/pa-core/tests/prompt_guards.rs`, et l'outil du banc `bench/smoke/tools/sizes.py:48-49`, `:96`, `:98` (texte v1).

## 5. CI et build

- `.github/workflows/ya-build-macos-arm64.yml:57` (`--bin prime-agent`), `:74` (`--binary target/…/prime-agent`), `:91` (**artefact `prime-agent-aarch64-apple-darwin-${{ github.sha }}`**), `:93` (`dist/prime-agent-*`), `:22` (filtre `prime-agent-runtime/**`).
- `scripts/release/assemble_artifacts.py:72` (`BINARY_PLACEHOLDER = "prime-agent"`), `:88-94` (nom du binaire par cible, `.exe` sous Windows), `:39-43` (archive `prime-agent-<version>-<plateforme>.tar.gz`, nom attendu par le flux de mise à jour), `:340-344` (manifeste `package.json` : `name`, `bin`, `piConfig.configDir: ".prime/agent"` ; aucun code de `crates/` ne lit `piConfig`).
- `scripts/release/verify_release.py:50`, `:107-116`, `:204`, `:213` : contenu attendu de l'archive et `--version` du binaire extrait.
- `.github/workflows/ci.yml:346` (`./target/debug/prime-agent --prime-agent-bootstrap`), `:357-363` (binaire TS de référence `prime-agent-0.9.8` d'upstream), `:372-375` (`PA_RUST_BINARY`, venv du kernel sous `~/.prime/agent`).
- Autres `--bin prime-agent` : `.github/workflows/continuous.yml:179`, `:300`, `perf.yml:80`, `release.yml:189`, `:366`, `:487`, `windows-runtime-triage.yml:113`, `Makefile:173` ; chemins `Makefile:92-95`, `:174`.
- Tests : 62 `env!("CARGO_BIN_EXE_prime-agent")` dans 40 fichiers. Cargo dérive ce nom de la cible `[[bin]]`.
- Scripts d'installation : `install-rust.sh` (272 lignes avec le nom), `install.ps1` (65). Tous deux installent depuis `app.primeintellect.ai`.
- Mise à jour : `crates/pa-core/src/update/installer.rs:41` (`OFFICIAL_INSTALLER_URL` d'upstream). `prime-agent update` remplacerait notre binaire par celui d'upstream. Ce n'est pas un sujet de renommage mais de YA-16 (distribution) : signalé seulement.

## 6. Banc

| Fichier | Dépend de | Où |
|---|---|---|
| `bench/smoke/run.py` | **ni du nom du binaire** (`--binary` quelconque, `run.py:1652`) **ni du dossier par défaut** (dossier fixé par variable, `run.py:292-293`) ; lit `sessions/`, `session-artifacts/`, `auth.json` sous ce dossier (`run.py:305`, `:390-395`, `:412`, `:1496`) ; `--version` (`run.py:830`, `:1843`) ; commande `-p --mode json --provider --model --thinking` (`bench/smoke/README.md:66`) | Renommer la variable `PRIME_AGENT_CODING_AGENT_DIR` sans repli casserait l'isolement : le binaire écrirait dans le vrai dossier de l'utilisateur. |
| `bench/smoke/tools/ab.py` | nom `prime-agent` dans le dossier du binaire (`ab.py:91`, `:160`, `:257`, `:298`) ; `--version` qui contient le sha (`ab.py:96-98`) ; liste des sources livrées avec `prime-agent-runtime` (`ab.py:66`) | |
| `bench/smoke/tools/install-bin.sh` | nom d'artefact `prime-agent-$TARGET-*` (`:16`, `:18`), binaire `prime-agent` (`:37`, `:41`, `:43`), archive `prime-agent-*-darwin-arm64.tar.gz` (`:13`, `:60`) | |
| `bench/tools/capture/run.py` | variable du dossier (`:122`), `--version` (`:464`), socket par défaut `/tmp/prime-agent-<uid>` (docstring `:281`) ; `--binary` quelconque | |
| `bench/tools/capture/analyze.py:281` | titre `prime-agent prompt --json` | texte seul |
| `bench/smoke/continue_session.py:7`, `bench/smoke/tools/notice-batch-repro.sh:9-28` | usage, variables `PRIME_AGENT_*` | |
| `bench/smoke/tools/sizes.py:48-49`, `:96`, `:98` | texte du prompt v1 | inchangé si v1 l'est |
| `Makefile:214` | message `BINARY=<dossier>/prime-agent` | texte seul |
| `bench/smoke/codex_arm.py:150-157` | rien : environnement de Codex construit en liste blanche | |

`--version` : `ab.py` et `install-bin.sh:43` cherchent le sha **dans** la sortie. Une sortie `yusagent <version>` passerait tant que le sha reste dedans. Les rapports déjà produits (`bench/smoke/runs/`, `bench/smoke/ab/*.md`) sont historiques et ne sont pas à réécrire.

## 7. Docs du dépôt

- `README.md` : README d'upstream (30 lignes avec le nom). Titre « Prime Agent: A Self-Improving RLM Harness » (`:12`), badges du dépôt upstream (`:22-32`), **installation depuis `app.primeintellect.ai`** (`:46`, `:52`), qui installe Prime Agent et non yusAgent.
- `docs/usage.md:124`, `:127` : `PRIME_AGENT_PROMPT_VERSION`.
- `CLAUDE.md:3` (« Hard fork de Prime Agent », à garder : c'est l'origine), `CLAUDE.md:32` (`--binary <dossier>/prime-agent`).
- `AGENTS.md` (11 lignes), `CONTRIBUTING.md` (6), `CHANGELOG.md` (11) : fichiers d'upstream.
- `docs/fork-changes.md:533` et suivantes (`~/.prime/agent/settings.json`, `PRIME_AGENT_TELEMETRY`) ; `bench/smoke/README.md:10`, `:11`, `:18`, `:28`, `:36`, `:41`, `:66`, `:77` ; `bench/tools/capture/README.md:3`, `:8`, `:23-24`, `:32`, `:71`.
- Skills livrés : `skills/websearch/SKILL.md:12`, `:14` (« run `/mcp` in Prime Agent »), `skills/skill-creator/SKILL.md:3`, `:8`, `:21`, `skills/factory/SKILL.md:26`, `:49-51`.

## 8. Ce qui garde le nom upstream

À ne pas renommer, pour garder les cherry-picks peu coûteux ou parce que ce n'est pas le produit :

- Crates `pa-*` (`Cargo.toml:3-14`), leurs binaires internes (`pa-daemon`, `pa-perf`, `pa-tui-replay`), modules et types (`PrimeAgent*Meta`, `crates/pa-daemon/src/acp/meta.rs:14-163`), constantes Rust.
- Dossier `prime-agent-runtime/` et son paquet (`prime-agent-runtime/pyproject.toml:2`) : livré à côté du binaire et cherché sous ce nom (`crates/pa-core/src/kernel/bootstrap/venv/runtime_source.rs:30`, `scripts/release/assemble_artifacts.py:75`), jamais affiché.
- Identifiants de protocole internes : MIME `application/vnd.prime-agent.*` et marqueur de fin de `bash()` (`prime-agent-runtime/src/rlm/bash.py:42`, `:51`, `:54`, `:841`), signature des instantanés du kernel (`prime-agent-runtime/src/rlm/repl.py:45`, la changer rend illisibles les instantanés existants), marqueur `crates/pa-cli/src/args.rs:31`, protocole du démon `prime-agent.daemon` (`crates/pa-daemon/src/protocol.rs:1162`), nom d'agent pour herdr (`crates/pa-daemon/src/herdr.rs:69`).
- `LICENSE` et copyrights (`docs/yusAgent-cdc.md:478`).
- Provider Prime Intellect : identifiant `prime-inference` (`crates/pa-types/src/ai/mod.rs:38`), `PRIME_API_KEY`, `PRIME_TEAM_ID`, `~/.prime/config.json`, catalogue `prime-agent-catalog` (`crates/pa-models/src/fetch.rs:10`, `:14`).
- Login par abonnement Claude (règle 7) : `crates/pa-ai/src/oauth/anthropic_callback.rs`, `crates/pa-tui/src/session_ui.rs:118`.
- En-têtes `User-Agent` : `crates/pa-ai/src/providers/opencode_headers.rs:9` (`prime-agent`, couche providers, envoyé à OpenCode), `crates/pa-telemetry/src/sinks/analytics.rs:107` (télémétrie coupée par défaut). Pas « l'interface » ; à décider à part.

Questions ouvertes pour Yusuf, non tranchées ici :

1. Les mentions de **Prime Intellect** (login, partage de traces, logo papillon) relèvent-elles de « plus de mention de Prime Agent dans l'interface » ? Elles nomment l'entreprise et son provider.
2. Le dossier **de projet** `<cwd>/.prime/agent/` suit-il `~/.yusagent` (par exemple `<cwd>/.yusagent/`) ?
3. L'espace de noms ACP `_meta` (`ai.primeintellect.prime-agent`) : clé de protocole invisible, mais l'extension sera écrite dessus. Le changer avant YA-10 coûte 1 constante et des fixtures ; après, il faut aussi changer l'extension.

## Options

### Ancien dossier `~/.prime/agent`

| Option | Avantages | Risques |
|---|---|---|
| **Rien** (nouveau dossier vide) | Le plus simple : 4 constantes, 2 chemins du venv, 5 replis Python. Aucun cas limite. Les testeurs n'ont pas d'ancien dossier (aucun build distribué, `docs/yusAgent-cdc.md:36`). Sépare enfin yusAgent d'un upstream installé sur la même machine. Banc non concerné (dossier explicite). | Yusuf refait login, réglages et skills, et perd l'accès direct à ses anciennes sessions (une commande `cp` documentée suffit). `~/.prime/agent/auth.json` reste sur le disque. Venv du kernel reconstruit une fois. |
| **Lecture de repli** (si `~/.yusagent` est absent et `~/.prime/agent` présent, utiliser l'ancien) | Aucune perte, aucune copie. | Deux règles à tenir en accord dans 4 résolutions Rust **et** 5 replis Python. Sinon le CLI lit l'un et le kernel l'autre (le repli Python est actif si l'hôte n'injecte pas la variable, `env_allowlist.rs:66`). Pour un utilisateur existant, le nouveau nom ne s'applique jamais. La cohabitation avec upstream continue. |
| **Copie au premier lancement** | Migration transparente, ancien dossier intact. | Code nouveau (module à part). Choisir quoi copier : `kernel-venv/` **ne se déplace pas** (un venv contient des chemins absolus, il faut le reconstruire), l'état d'exécution (`daemon-workers/`, `session-leases/`, `worker-snapshot-cache/`) non plus. `auth.json` contient des jetons : la copie double les secrets et doit garder les droits `0600`. Les sessions copiées peuvent citer des chemins absolus de l'ancien dossier (`session-artifacts/`, shells persistants `bash.py:1520`). Copie concurrente d'un démon encore actif, copie interrompue : il faut un marqueur. |

### Ancien nom du binaire `prime-agent`

| Option | Avantages | Risques |
|---|---|---|
| **Alias** (lien `prime-agent` → `yusagent` dans l'archive ou l'installation) | Habitudes et scripts conservés. | Contredit « plus de mention de Prime Agent ». Masque, ou est masqué par, un `~/.local/bin/prime-agent` installé par upstream (`install-rust.sh:5`). |
| **Rien** | Net. | `ab.py`, `install-bin.sh`, `CLAUDE.md` et les README du banc changent dans la même PR que l'artefact. |

Pour produire `yusagent`, indépendamment de l'alias :

- **(a) Renommer la cible Cargo** (`crates/pa-cli/Cargo.toml:23`) : `target/*/yusagent` partout. Touche 40 fichiers de tests upstream (62 `CARGO_BIN_EXE_prime-agent`) et 9 lignes de workflows/`Makefile`. Chaque cherry-pick d'un test e2e upstream échouera à la compilation (erreur claire, correction mécanique).
- **(b) Garder la cible Cargo, renommer à l'assemblage** (`assemble_artifacts.py`, `verify_release.py`, `ya-build-macos-arm64.yml`) : aucun test upstream touché ; seul l'artefact livré s'appelle `yusagent`. `target/debug/prime-agent` reste en dev et en CI. La détection par nom (`boot_reap.rs:376`, `hold_refusal.rs`, `homebrew.rs:35`, `install.rs:85`, `:93`) doit accepter `yusagent`.
- **(c) Deux cibles `[[bin]]` sur le même `main.rs`** : double l'édition de liens en CI. Écartée.

Dans les trois cas, `is_product_binary` (`boot_reap.rs:376`) doit reconnaître le nouveau nom.

### Variables d'environnement

| Option | Avantages | Risques |
|---|---|---|
| **Inchangées** | Coût nul, cherry-picks intacts, banc et CI intacts, **préfixe inchangé**. Les variables ne sont pas « l'interface » au sens strict. | Le nom hérité reste dans la queue du prompt v2 (2 variables), dans la notice de télémétrie du TUI (`crates/pa-tui/src/session_ui/notes.rs:190`) et dans les docs. |
| **Nouveau nom avec repli sur l'ancien** | Nom cohérent pour l'utilisateur ; scripts existants préservés. | Environ 100 lectures dispersées (`std::env::var` direct dans les crates, runtime Python, skills, installateurs) : il faudrait un lecteur commun, donc de nombreux fichiers upstream touchés. Ordre de priorité à définir quand les deux sont posées. Renommer les 2 variables citées dans le prompt **change le préfixe** v2 et v1. |
| **Mixte** : renommer avec repli les seules variables publiques (dossier, sessions, venv, Python du kernel, socket, télémétrie, version du prompt), garder internes et mise à jour | Couvre ce que l'utilisateur tape, une dizaine de points de lecture. | Deux conventions coexistent. `PRIME_AGENT_CODING_AGENT_DIR` est lue aussi par le runtime Python (5 sites) : le repli doit y être aussi, ou l'hôte doit injecter la variable. |

### Découpage en PR

Proposition, pour discussion :

1. **Textes de l'interface** : `APP_NAME`, aide, `--version` avec nom, TUI (splash, accueil, messages du démon et leur reconnaissance), `agentInfo` ACP et ses fixtures. Préfixe inchangé ; banc inchangé ; tests d'or du TUI à mettre à jour.
2. **Nom du binaire** : selon (a) ou (b), avec `is_product_binary`, l'artefact CI, `ab.py`, `install-bin.sh`, `CLAUDE.md` et les README du banc **dans la même PR** (sinon le banc ne trouve plus le binaire). Un seul push pour un seul build de 8 min.
3. **Dossier** : 4 constantes, venv du kernel, replis Python (ou injection de la variable par l'hôte), dossier des sockets, plus l'option retenue pour l'ancien dossier. Préfixe inchangé, sauf pour les sessions qui ont des skills utilisateur.
4. **Variables** (optionnel, selon l'option) : seule PR qui peut changer le préfixe ; à signaler comme telle et à mesurer avec `bench/tools/capture/run.py`.

Les PR 1 et 3 sont indépendantes. La PR 2 doit précéder toute mesure du banc qui suit.

## PR 2 : binaire (19/11/2026)

Branche `ya-19-binaire`, depuis `main` à `aea5db01b` (merge de la PR 1).

- **Textes de télémétrie** (premier commit, demande de Yusuf) : `/telemetry status` affiche « Telemetry is off (disabled in yusAgent). » sans endpoint ; `/telemetry on` (et `prime-agent telemetry on`) est refusé avec « Remote telemetry is disabled in yusAgent. » ; la notice « yusAgent sends pseudonymous usage… » ne s'affiche plus, ni dans le TUI ni dans les modes sans TUI. Textes en anglais comme le reste de l'interface (et comme le refus de `/traces on`). Limite : un `telemetry.enabled: true` écrit à la main active toujours le miroir local `telemetry.jsonl`, sans envoi.
- **Binaire** : la cible Cargo reste `prime-agent` ; l'archive de `ya-build-macos-arm64.yml` livre `yusagent` (option `--binary-name` d'`assemble_artifacts.py` et `verify_release.py`, absente par défaut : release upstream inchangée). L'archive garde son nom `prime-agent-<version>-<plateforme>.tar.gz` ; l'artefact devient `yusagent-aarch64-apple-darwin-<sha>`. `is_product_binary` reconnaît `yusagent`.
- **Commande** : `APP_NAME = "yusagent"` ; les commandes citées dans les messages (usages, `shutdown --force`, `attach`, `--resume`, `incident`…) disent `yusagent`. La reprise herdr lance `yusagent --resume`.
- **`update` désactivée** : `update` (CLI, toutes options) et `/update` (TUI) refusent avant tout téléchargement avec « Self-update is disabled in yusAgent. Install a newer yusAgent build to update. ». Précédent : Unharnessed, `75cfc357fe501a46f4a01a345457c0897eb88d89` (commande) et `09f760dfa4b9fbb205443f6fcf7736256ccc226f` (module). Sans ce refus, `/update` hors installation gérée lançait l'installateur upstream de `app.primeintellect.ai`.
- **Banc** : `install-bin.sh` et `ab.py` acceptent `yusagent` et, pour les builds d'avant le ticket 19, `prime-agent` ; chemins des README, de `CLAUDE.md` et du `Makefile` mis à jour. Options de `bench/smoke/run.py` inchangées.
- **Laissés** : `package update` (partie « mise à jour de soi » déjà refusée hors installation gérée), texte de la confirmation de `/update` (figé par un test e2e), code de mise à jour (inatteignable), nom de l'archive, `package.json` de l'archive (`name`, `bin`), outils de dev (`pa-perf`, `pa-tui-replay`).
- **À reprendre (refonte du CLI après YA-10, et YA-16 distribution)** : la confirmation de `/update` est encore demandée **avant** le refus (son texte, figé par `clipboard_auth_commands_e2e.rs:1815`, parle toujours de la migration TypeScript vers Rust) ; l'archive garde le nom `prime-agent-<version>-<plateforme>.tar.gz` (le binaire qu'elle contient s'appelle `yusagent`).
- **Contrôle en capture après la PR 2** (Yusuf, 19/11/2026) : `py-example-001`, binaires `7e2be841b` (avant le ticket), `aea5db01b` (PR 1) et `9ce92d3d0` (PR 2) : 3 runs `resolved`, code de sortie 0 ; rien d'écrit dans `~/.yusagent` ni `~/.prime/agent` ; `compare_capture_prompts.py` `TOUT IDENTIQUE` avant le ticket contre PR 1, puis PR 1 contre PR 2 (9 requêtes).

## PR 3 : dossier (19/11/2026)

Branche `ya-19-dossier`, depuis `main` à `d0eaab8bc` (merge de la PR 2).

- **Dossiers** : `~/.yusagent` (agent) et `<cwd>/.yusagent/` (projet), cinq constantes `CONFIG_DIR_NAME`. Surcharge `PRIME_AGENT_CODING_AGENT_DIR` inchangée (variables `PRIME_AGENT_*` gardées). **Pas de migration**.
- **Venv du kernel** : `~/.yusagent/kernel-venv` (repli XDG `~/.local/share/yusagent/kernel-venv`). Écart à la décision « suit le chemin passé par l'hôte » : le venv reste sous le dossier personnel, comme upstream, et ne suit pas `PRIME_AGENT_CODING_AGENT_DIR` ; sinon chaque test qui pointe cette variable vers un dossier temporaire reconstruirait un venv. `PRIME_AGENT_KERNEL_VENV` le déplace toujours (le banc le fixe). **Écart accepté par Yusuf le 19/11/2026** (journal du CDC).
- **Identifiants du banc** (vérifié avant le push) : jamais lus ni copiés depuis `~/.prime/agent` ou le dossier par défaut du binaire. `bench/smoke/run.py` et `continue_session.py` lisent `auth.json` dans le dossier agent dédié `~/.cache/yusagent-smoke/agent` (`Cache.agent`, `run.py:247` ; contrôle `run.py:1760`, `continue_session.py:56`), que le README fait remplir par un login lancé avec `PRIME_AGENT_CODING_AGENT_DIR` explicite ; le binaire y rafraîchit le jeton par la même variable (`run.py:292`). Le bras Codex CLI lit `~/.cache/yusagent-smoke/codex-home/auth.json` (`CODEX_HOME`, refus de `~/.codex` : `codex_arm.py:217-229`). `ab.py` passe par `run.py`. La sonde et le mode capture écrivent des identifiants factices (`run.py:517`, `:1759`). Rien ne casse après la PR 3, aucune correction nécessaire.
- **Runtime Python** : l'hôte passe déjà `PRIME_AGENT_CODING_AGENT_DIR` au kernel (`crates/pa-core/src/session_engine/runtime_wiring.rs:272`). Les cinq replis Python sans cette variable pointent sur `~/.yusagent` : plus aucun repli sur `~/.prime/agent`.
- **Sockets** : `$TMPDIR/yusagent-<uid>/` (Unix). Les noms Windows ne changent pas (pas d'exécution native).
- **CLI TypeScript d'upstream** (vérifié pour le test `ts_daemon_differential_cli_output`) : ni livrée ni utilisée par le fork. L'archive de `ya-build-macos-arm64.yml` ne contient que `yusagent`, le runtime, les skills, les catalogues et les fichiers de compatibilité (`scripts/release/assemble_artifacts.py:72-84`, `scripts/release/native_compat.py:43`) ; l'extension n'existe pas encore (YA-10). Le binaire TS v0.9.8 n'est téléchargé que par `ci.yml` (shard 1) pour les tests différentiels (`daemon_commands_e2e.rs`, `differential_cli.rs`, `export_live_differential.rs`, `lock_compat_e2e.rs`, `package_e2e.rs`). Seul autre usage, dormant : `release.yml` d'upstream, déclenché par un tag `v*`, empaquette un repli TS dans le paquet npm de migration (`.github/workflows/release.yml:810-822`) ; le fork n'a aucun tag `v*` et ce workflow n'a jamais tourné. À trancher avec la distribution (YA-16).
- **Effet** : un `~/.prime/agent` existant n'est plus lu (réglages, `auth.json`, `models.json`, sessions, skills, `AGENTS.md`, `SYSTEM.md`, `APPEND_SYSTEM.md`) ; un `<projet>/.prime/agent/` non plus (réglages et skills de projet, `SYSTEM.md`, `APPEND_SYSTEM.md`, modèles de prompts). `~/.prime/config.json` (CLI `prime`) reste lu pour Prime Inference.

### Migration manuelle (commande documentée, rien d'automatique)

À lancer une fois, l'ancien démon arrêté **avec l'ancien binaire** (`<binaire d'avant la PR 3> shutdown` : ce démon écoute sur `$TMPDIR/prime-agent-<uid>`, qu'un `yusagent` de la PR 3 ne voit plus), sans écraser un `~/.yusagent` existant. Le venv du kernel n'est pas copié (chemins absolus, il se reconstruit) ni l'état d'exécution (`daemon-workers`, `session-leases`, `worker-snapshot-cache`, `logs`). `cp -p` garde les droits de `auth.json` (0600).

```sh
mkdir -m 700 ~/.yusagent
cd ~/.prime/agent && for f in settings.json auth.json models.json keybindings.json mcp-services.json AGENTS.md SYSTEM.md APPEND_SYSTEM.md skills prompts themes git sessions session-artifacts harness; do [ -e "$f" ] && cp -Rp "$f" ~/.yusagent/; done
# par projet, si besoin :
git mv .prime/agent .yusagent   # ou mv, selon que le dossier est suivi
```

**Attention, chemins relatifs dans `settings.json`.** Quatre champs sont résolus depuis le dossier des réglages, pas depuis le répertoire courant : les sources locales de `packages`, et les entrées de `skills`, `prompts` et `themes` (`crates/pa-core/src/packages/resolve/mod.rs:28-31`, `crates/pa-core/src/packages/resolve/manager.rs:158-176`, `crates/pa-core/src/packages/manager.rs:429-435` et `:506-512`). Le dossier global passe de `~/.prime/agent` (deux niveaux sous le dossier personnel) à `~/.yusagent` (un niveau), le dossier de projet de `<projet>/.prime/agent` à `<projet>/.yusagent` : un chemin relatif comme `../x` désignait `~/.prime/x` (ou `<projet>/.prime/x`) et désigne `~/x` (ou `<projet>/x`) après copie. Après la copie, relire ces quatre champs dans le `settings.json` global et dans ceux des projets, et réécrire chaque chemin relatif en absolu ou en `~/…`. Les chemins absolus et en `~/…` ne changent pas. Non concernés : `sessionDir` (relatif au répertoire courant ou en `~/`, `crates/pa-core/src/settings/manager.rs:875-883`) et `shellPath` (absolu). Les paquets npm s'installent hors du dossier (racine npm globale) ; les paquets git sont dans `git/`, copié par la commande.

Limite : une session reprise peut encore citer des chemins absolus de `~/.prime/agent` (sessions de sous-agents, shells persistants).

