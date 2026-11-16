# Ticket 11 : YA-09, lot Unharnessed

Plan écrit en lecture seule le 16/11/2026, validé par Yusuf le même jour avec les décisions ci-dessous (journal du CDC, 16/11/2026). Base du plan : `main` à `03c1a3fce` (défauts : `reasoningSummary` off, `bashWaitSeconds` 30, `rlmMaxDepth` 0, `editTool` native pour Codex). Unharnessed lu à `fe2cf2815` (clone jetable, aucun de leurs scripts lancé). Tokenizer `o200k_base` **supposé** : tous les comptes sont des estimations, à confirmer par l'attribution du backend Codex sur la chauffe (`bench/smoke/ab/ya09-bash-count.py --usage`).

Brouillons du prompt v2 comptés au §4 : `docs/tickets/11-ya09-unharnessed/` (`default.md`, `native-edit.md`, `native-shell.md`).

## Décisions de Yusuf (16/11/2026)

1. **Sessions créées avant le lot : option A.** Un `yaRequest` sans version vaut v1, octet pour octet, sans `timeout` par défaut.
2. **Avis `[bash-done]`** : l'ajout gratuit d'un avis à l'appel modèle suivant reste, quand un appel a lieu. Le regroupement est repris (`bashNoticeBatchSeconds`, défaut 120 s) ; un réveil séparé ne part que si l'agent a fini son tour. **Écart à Unharnessed**, justifié : ils n'ont jamais eu ce mécanisme (un avis n'y rejoint jamais une exécution en cours).
3. **`edit()` du kernel** : reprendre la façon d'éditer d'Unharnessed, à défaut au minimum : création de fichier possible et retour avec quelques lignes de contexte autour de l'edit. Pas de contrôle de workspace comme sécurité (le sandbox de YA-11 s'en charge), mais une erreur claire pour un chemin hors du workspace. **Ajout du 16/11** : une erreur explicite quand le texte à remplacer est absent ou présent plusieurs fois (nombre d'occurrences et chemin dans le message ; aujourd'hui `ValueError` « string not found in <path> » ou « found N occurrences », `skills/edit/src/edit/__init__.py:33-40`, à garder et à tester). Écart à Unharnessed noté au journal : un edit qui échoue en silence coûte des tours. **Constat** : Unharnessed n'a pas de fonction d'edit. Son commit `bd7521115` (« drop the remaining extra skills ») a retiré le skill `edit` et la règle « edit with exact old/new strings » ; son modèle écrit les fichiers en Python. Rien à reprendre : la PR B applique le minimum ci-dessus.
4. **`persistent=True`** : pas de délai par défaut ; un `timeout` explicite est respecté.
5. **Skills masqués dans l'inventaire v2**, dont `agent-message` : oui, liste dans la PR B.
6. **Description courte de `python_repl` en v2** : oui.
7. Ce plan est versionné avec la PR A.

**Ordre** : PR A, puis un ticket séparé sur la fermeture du WebSocket en 1011 (lancé par Yusuf), puis PR B. **Chauffes autorisées d'avance** (vérifications, pas des mesures) : `node-testsprite-356` après la PR A ; 3 tâches après la PR B (`node-testsprite-356`, une Python et une Node). Résumé des changements de requête relu par Yusuf avant chaque merge.

## PR A livrée (`ya-09-bash-runtime`)

Écarts au plan ci-dessous, après lecture du code :
- **Avis groupés : hôte sans démon seulement** (`-p`, RPC, banc). **Exigence reportée à YA-10** (critère d'acceptation ajouté à la section YA-10 du CDC, accord de Yusuf du 16/11) : le banc, lancé en `-p`, teste un comportement que l'extension n'aura qu'à YA-10. Le worker du démon (extension, TUI) admet les avis par ses propres files (`crates/pa-daemon/src/worker/queue.rs:531`, `admit_kernel_notice`) et livre une ligne custom seule par tour (`queue.rs:345-350`) avec une seule `custom_message` par requête (`crates/pa-daemon/src/worker/turn.rs:346`). Y grouper plusieurs avis demande de changer ce format de tour : à faire avec YA-10, où ce chemin est exercé. Le démon garde le comportement upstream.
- **Mécanisme** (`crates/pa-core/src/session_engine/ya_bash_notice_batch.rs`) : un avis qui arrive pendant une exécution (`Agent::signal()` présent) suit le chemin upstream et rejoint l'appel suivant ; à l'arrêt, il est retenu, puis tous les avis retenus sont mis en file ensemble avec un seul réveil quand la dernière commande se termine (callback de fin du kernel), quand aucune commande ne tourne (shell persistant), ou quand le plus ancien a attendu la fenêtre. Une lecture du résultat (`bash.consumed`) retire l'avis retenu. `bashNoticeBatchSeconds: 0` enregistre les sinks upstream tels quels. Le réglage n'est pas figé : le prompt n'en parle pas.
- **Timeout** (`prime-agent-runtime/src/rlm/ya_timeout.py`) : sans `timeout`, la limite est `ya_timeout.DEFAULT_TIMEOUT`, `None` dans la PR A ; la PR B la pose à 600 pour les sessions v2 par le bootstrap. Le minuteur est annulé quand la commande se termine (pas au réapage du groupe comme chez Unharnessed : les restes d'un `cmd &` après la fin ne sont pas tués, comme pour une commande normale).

## 0. Constats qui orientent le plan

- Notre base est plus récente que la leur : `3358e0016` est un ancêtre de notre `main`, 318 commits avant notre point commun avec upstream (`c83ac68b3`). Leur gestion des réveils (`crates/uh-core/src/session_engine/wakes.rs`, `SessionWakes`) n'existe pas chez nous. Les points 4 et 5 sont donc **réécrits** sur nos points d'entrée, pas recopiés.
- Leur `await handle` attend jusqu'au bout (`unharnessed-runtime/src/rlm/bash.py:975`). Nous gardons la fenêtre de 30 s : le prompt doit garder la ligne du snapshot.
- Leur ligne de regroupement a un historique : retirée par `7dc73199e` (A/B Terminal-Bench du 08/11, 8 tâches × 2 : 0,89 $ contre 0,85 $, 39 contre 41 tours), puis remise « comme conseil » par `5e9659638` le même jour. Son effet n'est pas démontré.
- Dans nos sessions sans démon (`-p`, banc), un avis qui arrive pendant une exécution rejoint aujourd'hui l'appel modèle suivant de cette exécution (`bash_notices.rs:203-204` → `agent.steer`, lu par `crates/pa-agent/src/agent_loop/run.rs:163-164`). « Envoyé après la réponse » est donc un vrai changement chez nous. Chez Unharnessed, un avis n'a jamais été injecté en cours d'exécution (`wakes.rs:10-12`) ; `814d67695` ne change que la frontière des tours ACP (`acp/prompt.rs` ~`:625`).
- `edit()` du kernel renvoie `Edited <path>`, sans zone numérotée (`skills/edit/src/edit/__init__.py:45`), refuse un fichier absent (`:30-31`) et n'a pas le contrôle de workspace d'`apply_patch` (décision du 13/11 au journal, `ya_workspace.rs`).
- `python_repl` et `apply_patch` sont des tools séquentiels (`crates/pa-core/src/tools/ipython.rs:555`, `ya_apply_patch.rs:53`) : plusieurs appels d'une même réponse s'exécutent dans l'ordre émis (`crates/pa-agent/src/agent_loop/tools.rs:54-58`).

## 1. Découpage : 2 PR

| PR | Points | Préfixe | Taille (est.) |
|---|---|---|---|
| **A** `ya-09-bash-runtime` | 4 (avis groupés après la réponse), 5 (mécanisme `timeout`) | **inchangé** : prompt et tools identiques ; seul le contenu de la conversation change (moment et texte des `[bash-done]`) | ~400 lignes de code + tests |
| **B** `ya-09-prompt-v2` | 1 (prompt court), 2 (regroupement), 3 (un seul tool), défaut `timeout=600` | **change** pour les nouvelles sessions (version 2 figée dans l'en-tête) | ~350 lignes + textes + tests |

A d'abord : B documente `timeout` et doit le trouver en place ; et B reste la seule PR qui touche au préfixe (une seule rupture de cache, une seule version).

## 2. PR A : `bash()` timeout et avis groupés

### Point 5, `timeout` (d'après `569284b36`)

- **Runtime** `prime-agent-runtime/src/rlm/bash.py` (upstream, ~35 lignes marquées `# yusAgent (YA-09)`, est.) : paramètre `timeout` **en dernier** (`bash(command, persistent=False, env=None, timeout=<défaut>)`) ; `_arm_timeout` (`threading.Timer` démon) ; `_time_out` → ligne `[yusagent: killed, still running after its N s timeout]` dans la sortie puis `self.kill()` (SIGTERM au groupe, SIGKILL après 5 s : notre `kill()` existe déjà, `bash.py:546-569`) ; annulation au réapage ; `timeoutSeconds` dans le payload `bash.completed` (`:868-873`, `:1300-1302`). Validation (bool, ≤ 0, nan, inf refusés) dans un nouveau `rlm/ya_timeout.py`. La règle « runtime non modifié » de `ya_bash_wait` ne tient pas ici : le champ doit partir avec le payload.
- **Défaut dans PR A : aucune limite.** Une globale `_YA_DEFAULT_TIMEOUT` (None) n'est posée à 600 que par le bootstrap d'une session v2 (PR B), comme `ya_bash_wait`. Un `timeout=` explicite marche dès A.
- **`persistent=True`** : pas de délai par défaut (le timer ne survit pas à un redémarrage, et ce shell est fait pour durer) ; `timeout` explicite refusé avec `ValueError`. **À valider.**
- **Démarrage du kernel** : `RUNTIME_READY_CHECK` exige la signature exacte (`crates/pa-core/src/kernel/bootstrap/venv/probe.rs:32`) → 1 ligne ; repli sans runtime (`kernel/bootstrap/runtime_code.rs:83`) → 1 ligne. Sans cela le kernel ne démarre pas (même panne qu'au run 43479147466).
- **Avis** : nouveau `crates/pa-core/src/session_engine/ya_bash_timeout.rs` (`mark_timeout` : ajoute `Killed: still running after its N s timeout.` au texte et `timeoutSeconds` aux `details`) ; appel depuis `validate_completion` (`bash_notices.rs:119-131`, ~5 lignes). Le démon passe par la même fonction (`crates/pa-daemon/src/bash_notices.rs:53`).
- `shellTool: native` : `exec_command` n'a pas de durée de vie (`ya_exec.rs:223-234`), non concerné (écart noté).

### Point 4, avis groupés après la réponse (d'après `7dc73199e`, partie avis, et `814d67695`, partie réveil)

- **Nouveau** `crates/pa-core/src/session_engine/ya_bash_notice_batch.rs` : tampon devant les deux arrivées (avis, lecture du résultat).
  - retient tant que l'exécution tourne (`is_streaming` sans démon, `core.busy` avec démon) ;
  - libère quand la session est au repos **et** qu'aucune commande ne tourne (liste du kernel, `engine.rs:1255`), ou quand le plus ancien avis attend depuis la fenêtre ;
  - livre un seul `AgentMessageBatch::Batch` (`crates/pa-agent/src/agent.rs:157`) : un réveil, quel que soit `steeringMode` ;
  - un `bash.consumed` retire l'avis du tampon ; `factory.progress` passe tel quel.
- **Réglage** `bashNoticeBatchSeconds` (défaut 120 comme eux ; 0 = comportement actuel). Non figé : le prompt v2 reste vrai dans les deux cas (texte neutre, voir §4), donc pas de champ d'en-tête.
- **Hors demande, non repris** : `kill_all` à la fin d'un goal, continuation `output_limit`, `steer`, panneau. Aucun n'est un prérequis.
- Fichiers upstream touchés : `session_engine/engine.rs:502` (~3 lignes), `crates/pa-daemon/src/worker.rs:626` (~3), `crates/pa-cli/src/print_rlm.rs:356` (1-2 : `-p` ne finit pas avec des avis retenus), `crates/pa-daemon/src/worker/queue.rs:345` (~5 : lignes `[bash-done]` consécutives dans un même tour). **À vérifier avant code** : le traitement de plusieurs lignes custom par `run_turn` (`worker/turn.rs:113`), non lu.

### Fichiers PR A

Upstream (tous dans `fork-changes.md`) : `bash.py`, `probe.rs`, `runtime_code.rs`, `bash_notices.rs`, `engine.rs`, `pa-daemon/src/worker.rs`, `worker/queue.rs`, `pa-cli/src/print_rlm.rs`, `settings/types.rs` (+3), `settings/load.rs` (+1).
Nouveaux : `rlm/ya_timeout.py`, `ya_bash_timeout.rs`, `ya_bash_notice_batch.rs`, tests, `.changes/ya-09-bash-runtime.md`.
Docs : `fork-changes.md` (lignes + table « Code repris d'Unharnessed » : `569284b36`, `7dc73199e`, `814d67695`, mention « réécrit »), journal du CDC (reprise sans A/B, fenêtre de 2 min).

### Tests PR A (hors fichiers upstream)

- `prime-agent-runtime/test/test_ya_bash_timeout.py` : `timeout=1` sur `sleep 30` (arrêt ~1 s, enfant `sleep &` du groupe tué, ligne dans la sortie) ; `trap '' TERM` → SIGKILL à 5 s ; `timeoutSeconds` dans le payload (host factice) ; commande finie avant → timer annulé ; `None` ; valeurs refusées ; `persistent=True`.
- Unitaires Rust dans `ya_bash_timeout.rs` et `ya_bash_notice_batch.rs` (`tokio::time::pause`) : retenue pendant l'exécution, libération à la dernière commande, à la fenêtre, retrait d'un avis lu, un seul lot en `OneAtATime`, fenêtre 0 = actuel.
- `crates/pa-core/tests/ya_bash_timeout_kernel.rs` (vrai kernel, sur le modèle de `ya_bash_wait_kernel.rs`) : le probe passe, `bash("sleep 60", timeout=2)` donne un avis avec la raison.
- `crates/pa-cli/tests/ya_bash_notice_batch_e2e.rs` (provider factice, vrai kernel) : 3 `bash()` détachés → un seul appel de réveil avec 3 lignes ; aucun avis dans un corps avant la réponse finale ; `-p` ne finit pas avec des avis retenus.
- Démon : nouveau fichier de test, deux avis libérés → un tour.
- `prefix_stability.rs` (fichier du fork) : la requête de réveil prolonge le corps précédent ; prompt et tools identiques entre fenêtre 0 et défaut.

### Changements de requête PR A (à relire avant merge)

Instructions et tools : aucun. Conversation : les `[bash-done]` d'une exécution arrivent après la réponse, groupés ; texte `Killed: …` sur un dépassement. Nombre d'appels : peut changer (voir risques).

## 3. PR B : prompt v2 et un seul tool

### Version de prompt figée par session

- Réglage `promptVersion` (`1` | `2`, défaut `2`), figé dans `yaRequest` (`ya_request.rs`, fichier du fork) au champ `prompt`. `1` reste un réglage (règle 5) : il servira à mesurer v1 contre v2 au Terminal-Bench.
- Sessions antérieures : voir §5 (option A ou B).
- `BuildSystemPromptOptions` gagne `ya_prompt_version` ; **sa valeur par défaut est v1**, pour que les tests upstream de `system_prompt.rs` ne bougent pas ; `engine.rs` passe la version figée.

### Point 3, un seul tool

- `ya_tool_family.rs:128` (fichier du fork) : `None => codex` devient `None => false`. `editTool: "native"` reste un réglage (Codex seulement). Les sessions dont l'en-tête a `yaTools` gardent leur tool (gel existant).
- **Écart au CDC à inscrire au journal** : décision du 28/10 (« `edit` reste séparé, au format natif de chaque famille »), décision du 13/11 (`apply_patch` par défaut) et verdict de l'A/B 1 du 14/11 (`native` resté défaut). Éléments à citer : A/B 1 non distinguable (+2,5 % [-8,8 ; +16,5] pour `kernel`, edits au premier essai 99,6 % contre 98,8 %), justification révisée le 14/11 (Codex en `code_mode_only`), décision de reprise d'Unharnessed du 15/11.

### Point 1, prompt v2

- Nouveau `crates/pa-core/src/prompts/ya_prompt_v2.rs` + `prompts/ya_prompt_v2/core.md` et fragments par tools (`apply_patch`, `exec_command`). Les couches upstream (`core.md`, `usage.md`, `opinionated.md`) ne bougent pas : v1 les garde.
- `system_prompt.rs` (upstream, ~15 lignes) : en v2, le préfixe statique vient de `ya_prompt_v2` ; l'inventaire des skills et les lignes de rôle passent par ses hooks ; `packages` et `environment` inchangés.
- Inventaire des skills v2 : liste compacte (un item par skill) au lieu du XML, préambule de 2 phrases. Skills fournis **affichés** : `attach-image`, `compact`, `goal`, `mcp` (+ `rlm-heartbeat` avec démon ; `refine` si `harnessRefinements` est actif). **Masqués de l'inventaire seulement** (toujours importables, kernel inchangé, `SKILL.md` intacts) : `edit` (dans le prompt), `refine`, `skill-creator`, `prime-intellect`, `agent-message` (avec `rlmMaxDepth` 0). Descriptions courtes des skills fournis dans le module ; les skills de l'utilisateur gardent leur description.
- Ligne `Recursive agent depth: 0 (root)` retirée en v2 pour une racine à `rlmMaxDepth` 0 (gardée pour un enfant).
- Bootstrap : en v2, `_YA_DEFAULT_TIMEOUT = 600` posé comme `ya_bash_wait` (`kernel/bootstrap/ya_bash_timeout.rs`, nouveau ; `provisioner.rs` +~5, `runtime_wiring.rs` +~2).
- `prime-agent prompt` (`crates/pa-cli/src/prompt_command.rs`, 2 lignes) : affiche la version des réglages courants.
- **Optionnel, à ta main** : description de `python_repl` raccourcie en v2 via le hook existant (`engine.rs:602-603`, `ya_tools_prompt::python_repl_description`), par ex. « Run Python code in the persistent REPL. Top-level `await` works; variables persist across calls. » : 171 → ~25 tokens (est.).

### Audit du prompt c770c5a (6 230 tokens, compte local)

| Section | Tokens | Sort en v2 | Raison |
|---|---|---|---|
| Intro `# prime-agent harness` | 135 | réécrite (3 phrases) | « two tools » faux avec un seul tool ; « PTC » inutile |
| Core tools (`bash`, `read`, `edit`, `attach_image`) | 568 | **gardé, condensé** | API réelle ; `persistent` en 1 ligne ; `BashHandle` sur une ligne comme Unharnessed |
| Bloc a2a (`agent_message`, `agent_observe`) | 192 | retiré | sans sous-agents (`rlmMaxDepth` 0) ; le skill reste importable |
| Continual Harness | 1 374 | **retiré** | `harnessRefinements` coupé (`fork_defaults.rs:8`) : le digest des mémoires n'est jamais relivré (`harness_digest.rs:463`), l'auto-refine est coupé ; doc déjà dans le skill `refine` |
| Compaction | 112 | → skill `compact` (déjà) | 1 règle gardée : variables > 16 MiB |
| Goal | 105 | → skill `goal` (déjà) | |
| Heartbeat | 229 | → skill `rlm-heartbeat` (déjà) | ne se déclenche pas sans démon |
| MCP | 230 | → skill `mcp` (déjà) | |
| Skills (préambule core) | 178 | retiré | redit l'inventaire |
| usage : règles goal/mémoire | 40 | retiré | dans les skills |
| usage : `bash()`, quoting, `os.chdir` | 139 | gardé, condensé | `bash("""…""")` réduit les cellules en erreur d'appel (KPI) |
| usage : edit, lecture en variables | 107 | gardé, condensé | |
| usage : tour texte, pas d'appel vide | 109 | gardé, 1 ligne | |
| usage : délégation, règles racine, terminologie, refine | 330 | retiré, sauf « progress updates » en 1 ligne | fonctions coupées |
| usage : environnement du projet | 163 | gardé, 1 ligne (Unharnessed) | |
| opinionated (style, liste d'écueils) | 475 | **1 ligne** | Unharnessed l'a retiré ; la liste d'écueils résumée en une règle |
| packages | 71 | inchangé | |
| inventaire des skills | 1 182 | 229 | 4 skills affichés, format compact |
| environnement | 98 | inchangé | lignes du fork (`session_paths`) |
| rôle (profondeur, démon, `await bash`) | 153 | 141 | profondeur retirée, avis en texte neutre, ligne 30 s gardée |

### Point 2, règle de regroupement

Première règle de la section Rules (texte ci-dessous). Variante `apply_patch` : l'edit et l'appel `python_repl` qui teste vont dans la même réponse (tools séquentiels, exécutés dans l'ordre, `tools.rs:54-58`).

### Fichiers PR B

Upstream : `prompts/system_prompt.rs`, `prompts/mod.rs` (+2), `session_engine/engine.rs` (~8), `settings/types.rs` (+3), `settings/load.rs` (+1), `kernel/provisioner.rs`, `session_engine/runtime_wiring.rs`, `kernel/bootstrap/mod.rs` (+1), `pa-cli/src/prompt_command.rs`.
Fork : `ya_tool_family.rs`, `ya_request.rs`, `ya_header.rs` (si besoin), `pa-daemon/.../ya_frozen_tools.rs` (tests), `prefix_stability.rs`.
Nouveaux : `prompts/ya_prompt_v2.rs`, `prompts/ya_prompt_v2/*.md`, `kernel/bootstrap/ya_bash_timeout.rs`, tests, `.changes/ya-09-prompt-v2.md`.
Docs : `fork-changes.md` (lignes + table Unharnessed : `core.md` de `fe2cf2815`, texte adapté), journal du CDC (écart `editTool`, critère YA-09 atteint, version de prompt), `bench/smoke/settings/prompt-v1.json` (`{"promptVersion": 1}`) pour la mesure future.
`pa-ai` : rien.

### Tests PR B

- Unitaires `ya_prompt_v2.rs` : les 4 combinaisons de tools donnent un texte cohérent (aucun `apply_patch` sans le tool, aucun `bash` en shell natif) ; garde de taille en caractères (5 289 aujourd'hui pour le défaut ; borne 7 600, soit ~1 900 tokens à 4,05 caractères par token mesuré sur ce texte, est.).
- `ya_tool_family.rs` : défaut `kernel` pour Codex ; `yaTools` natif gardé à la reprise.
- `prefix_stability.rs` : nouvelle session par défaut (un tool, prompt v2, préfixe stable sur 3 prompts, `yaRequest.prompt = 2`) ; `editTool: "native"` sur Codex en v2 ; `shellTool: "native"` en v2 ; `promptVersion: 1` = prompt c770c5a avec la ligne 30 s ; reprise d'une session antérieure (selon l'option) ; fork qui hérite de la version ; WebSocket Codex en suite de conversation.
- Kernel réel : v2 → `bash()` sans `timeout` a le délai de 600 ; v1 → aucun.
- `prompt_command.rs` : test de la version affichée.

### Changements de requête PR B (à relire avant merge)

- `instructions` : 6 230 → **1 309 tokens** (est.) par défaut.
- `tools` : `apply_patch` retiré par défaut, 423 → **171 tokens** (`python_repl` seul ; ~25 avec l'option de description).
- Total préfixe : 6 653 → **1 480** (est.), −78 %.
- `reasoning`, `input`, bloc `<environment>` : inchangés.

## 4. Nouveau prompt complet (défaut : un tool, sans démon, comme au banc)

Compte `o200k_base` supposé : **1 309 tokens**, 5 289 caractères (statique 770, packages 71, skills 229, environnement 98, rôle 141 ; le rôle est déduit du total, 1 321 avant le raccourcissement de la ligne du démon). Variante `editTool: "native"` : 1 338. Variante `shellTool: "native"` : 1 001. Commande, depuis la racine du dépôt : `uv run -q --no-project --with tiktoken==0.14.0 python -I -c 'import sys,tiktoken; e=tiktoken.get_encoding("o200k_base"); print(len(e.encode(open(sys.argv[1]).read().strip())))' docs/tickets/11-ya09-unharnessed/default.md` (idem `native-edit.md`, `native-shell.md`). Le découpage par section est une estimation faite sur le même texte.

```text
# yusAgent

You have one tool, `python_repl`: a persistent CPython REPL where top-level `await` works. Every other capability is a Python function in that REPL. Messages that start with `[<kind>]` come from the harness, not from the user.

## Functions in the REPL

- `bash(command: str, persistent: bool = False, env: dict[str, str] | None = None, timeout: float | None = 600) -> BashHandle`: start a shell command in the background and return at once; each call is a new process; `env` adds variables for this command only; a command still running after `timeout` seconds is killed (`None`: no limit)
  - `persistent=True`: the shell survives kernel and harness restarts and has no timeout; use it only for a server or job that must outlive a restart; after a restart, `from rlm import persistent_shells; persistent_shells()` returns the handles
  - `BashHandle`: `.pid`, `.running`, `.command`, `.tail(n=50)`, `.output()`, `.poll() -> BashResult | None`, `.kill(sig=SIGTERM, grace=5.0)`, and `await handle`
  - `BashResult`: `.exit_code`, `.output`, `.duration`
- `read(path, offset=1, limit=2000) -> str`: lines numbered like `cat -n`; when lines remain, a last line gives the offset to continue from
- `await edit(path, old_str, new_str) -> str`: replaces one exact, unique occurrence of `old_str` in an existing file; raises if it is missing or not unique

## Rules

- Each tool call is a round trip. Put in one call every step that does not need a result you have not seen yet: independent reads, searches and commands together (several `bash()` under `asyncio.gather`), and an edit together with the command that tests it.
- Run programs with `bash()`, not `subprocess` or `os.system`. Write logic, loops and parsing in Python; use `bash()` only to call programs. Prefer `bash("""...""")`: quotes inside need no escaping.
- `os.chdir()` and `os.environ` changes persist and apply to later `bash()` calls; `cd` and `export` inside a `bash()` call do not.
- Store what you read or compute in named variables instead of reading it again.
- Edit existing files with `edit()`, with `old_str` and `new_str` taken from text you have read; write new files from Python.
- Output of one call beyond a size limit keeps its beginning and end; the result names the file with the full output.
- Python variables survive compaction, except single values larger than 16 MiB.
- Run an external project through its own environment (`uv run`, `.venv/bin/python`, its documented commands); do not install its dependencies into the kernel. Its failures there are the result that counts.
- Do not hold the turn with `sleep` or repeated awaits of a running command: end the turn and wait for the notification.
- A turn can end with text only. A message or harness notice that needs no action gets at most one short line, and no tool call.
- Make the smallest change that solves the problem at the right layer: no speculative fallbacks, compatibility shims, defensive code or extra tests.
- On long work, give short progress updates: what is done, what blocks, what is next.
- When you are done, stop calling tools and give the final answer, with any assumption you made and any constant you changed.

Pre-installed Python packages: requests, httpx, yaml (PyYAML), tomli, dotenv (python-dotenv), pandas, numpy, scipy, bs4 (Beautiful Soup), lxml, pydantic, tyro.
Install additional packages with `uv pip install <pkg>` (this is a uv-managed venv with no pip module).

## Skills

When a task matches a skill, read its SKILL.md first. A skill with an import name is already imported in the REPL. Resolve a relative path in a skill against the skill's directory; expand `$PRIME_AGENT_BUNDLED_SKILLS_DIR` with `os.path.expandvars`.

- `attach-image` (import `attach_image`): see an image file (PNG, JPEG, GIF, WebP); needs a vision-capable model. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/attach-image/SKILL.md`
- `compact` (import `compact`): check context usage, or compact the conversation when much work remains. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/compact/SKILL.md`
- `goal` (import `goal`): start a goal when the user asks for one, check it, or mark it complete. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/goal/SKILL.md`
- `mcp`: call tools on the user's MCP services. `$PRIME_AGENT_BUNDLED_SKILLS_DIR/mcp/SKILL.md`

The current date (UTC) and the working directory arrive in an `<environment>` block at the end of a user message; a later block with a new date supersedes the earlier date.
The conversation log (this session's JSONL transcript) path is `os.environ["PRIME_AGENT_CONVERSATION_LOG"]` in the Python REPL; it is empty when the session is not persisted.
Image input: this model can see images; `attach_image` loads them into context.

This session has no daemon: heartbeats never fire. Commands that end after their cell wake you with a `[bash-done]` message; the run waits for running commands before it exits.
`await bash(cmd)` waits at most 30 seconds. If the command is still running, it returns a snapshot: `r.running` is True, `r.exit_code` is None, `r.output` is the output so far, and `r.handle` is the live handle. A completed result has `r.running` False and an integer `r.exit_code`. The command keeps running after a snapshot; end the turn and collect the final result on the `[bash-done]` notification.
```

Différences des variantes (fichiers complets dans `docs/tickets/11-ya09-unharnessed/`) :
- `editTool: "native"` : « You have two tools: `python_repl` … and `apply_patch`, which creates and edits files » ; `edit()` « for batches of edits driven by Python » ; règle d'edit : « Create and edit files with `apply_patch`; use `edit()` for the same change across many files » ; regroupement : « an `apply_patch` call, then the `python_repl` call that tests it, in the same response: they run in order ».
- `shellTool: "native"` : trois tools ; bloc `bash` retiré ; règles `exec_command` (shell neuf à chaque appel, `workdir`, `write_stdin` avec `chars` vide pour suivre) ; ligne `await bash` absente ; ligne du démon réduite à « heartbeats never fire ».
- Avec démon (extension) : pas de ligne « no daemon » ; `rlm-heartbeat` dans l'inventaire.

## 5. Sessions créées avant le lot : deux options

Dans les deux cas, `yaTools` déjà figé garde `apply_patch` aux sessions qui l'avaient, et une session d'avant YA-03 (sans `yaTools` ni `yaRequest`) prend les réglages courants (écart déjà accepté).

**Option A, l'ancien prompt à la reprise.** Un `yaRequest` sans `prompt` vaut v1 : prompt c770c5a octet pour octet (fenêtre `await bash` figée comme aujourd'hui), pas de `timeout` par défaut.
- Coût cache : nul.
- Coût code : le chemin v1 existe déjà et reste de toute façon comme réglage (`promptVersion: 1`) ; en plus : la règle « champ absent = v1 », ~5 lignes, et un scénario de `prefix_stability.rs`. Chaque futur changement de prompt créera une v3 à côté.
- Comportement : les avis groupés (PR A) s'appliquent aussi, la ligne v1 « queued `[bash-done]` turn » reste vraie.

**Option B, le prompt change à la reprise.** Un `yaRequest` sans `prompt` passe en v2 (écrit dans l'en-tête à la reprise).
- Coût cache : une reconstruction du préfixe par session reprise, soit contexte × (écriture 30 min − lecture en cache). Pour 100 000 tokens de contexte (calcul sur `bench/smoke/prices.json`, contexte court) : `gpt-6.1-terra` (1,25 − 0,05) × 0,1 = **0,12 $** ; `gpt-6.1-sol` **0,24 $** ; `gpt-6-luna` **0,012 $**. **Coût nul en plus** si le cache a déjà expiré à la reprise (au-delà de 30 min, ou 12 h en `cacheRetention: long`), ce qui est le cas courant d'une reprise.
- Coût code : un peu moins (pas de règle « absent = v1 »), mais le chemin v1 reste pour le réglage.
- Comportement : le modèle d'une session en cours voit ses règles changer au milieu (ex. le délai de 600 s s'applique soudain à ses commandes), sans que l'historique le montre.

Recommandation : **A**, parce que le chemin v1 reste de toute façon pour la mesure future et que A ne coûte qu'une règle et un test. **Retenue par Yusuf le 16/11/2026.**

## 6. Garde-fous après chaque PR

1. CI verte (`gh pr checks`, tous les checks requis en succès sur le dernier commit, aucun en skipping).
2. `prefix_stability.rs` vert (dans la CI).
3. Chauffe sur `node-testsprite-356` avec le binaire de la PR, `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION` fixé, `--log-bodies` puis `check_warmup.py` : vérification, pas mesure. **Appel au vrai modèle : je demande ton accord au moment de la lancer.**
4. Résumé des changements de requête (§2 et §3), confronté au corps capturé, relu par toi avant merge. Pour B : compte des `instructions` et des `tools` par `ya09-bash-count.py --usage` (attribution du backend Codex).

## 7. Risques

- **Qualité non mesurée.** Le prompt perd 4 900 tokens de consignes (liste d'écueils, style d'écriture, règles de délégation) sans A/B (décision du 15/11). `promptVersion: 1` permet la mesure au Terminal-Bench.
- **Règle de regroupement d'effet non démontré** chez Unharnessed (retirée puis remise le 08/11).
- **`editTool: kernel` par défaut** : plus de contrôle de workspace (`apply_patch` refusait un chemin hors workspace, journal du 13/11) ; `edit()` ne renvoie pas la zone modifiée (critère de YA-03 non tenu par `edit()`) ; un nouveau fichier s'écrit en Python. Modèles Codex entraînés sur `apply_patch` ; l'A/B 1 n'a pas vu d'écart sur `gpt-6.1-terra`.
- **`timeout=600`** : des builds et des suites longues seront tués ; le modèle doit penser à `timeout=None`, l'avis lui donne la raison. Au banc, le garde-fou d'inactivité de 300 s se déclenche avant : le délai n'y est pas observable.
- **Fenêtre de 2 min des avis et WebSocket 1011** : une fermeture a été vue après ~77 s sans requête (`docs/tickets/09-ya03-base.md:189-195`, cause supposée côté client, `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:229-235`). Des silences plus longs donnent probablement plus de bascules SSE en corps complet (est.). Le correctif du keepalive est dans la couche providers (règle 3) : hors de ce lot, à ouvrir à part si les chauffes le montrent.
- **Appels en plus possibles** : un avis qui rejoignait gratuitement l'appel suivant d'une exécution déclenche désormais un réveil après la réponse.
- **Avis retenus perdus** si le worker plante (ils ne passent pas par le journal de reprise du démon, `worker/queue.rs:531`). Ordre avis puis continuation du goal (`goal_continuation.rs:139`) à tester.
- **Skills masqués** toujours importables : un modèle qui les connaît peut les appeler ; aucun effet sur le kernel.
- **Comptes en `o200k_base` supposé** : le tokenizer de `gpt-6-luna` n'est pas publié ; contrôle par l'attribution du backend sur la chauffe.

## 8. Points à trancher avant code (tranchés le 16/11/2026, voir « Décisions de Yusuf »)

1. Sessions antérieures : option A ou B (§5).
2. `persistent=True` sans délai, `timeout` explicite refusé (§2).
3. Liste des skills masqués de l'inventaire v2, dont `agent-message` (§3).
4. Description courte de `python_repl` en v2 (optionnelle, §3).
5. Nom et défaut du réglage `bashNoticeBatchSeconds` (120 comme Unharnessed).
