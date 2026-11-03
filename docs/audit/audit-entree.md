# Audit d'entrée du fork (ticket 00)

- Date : 2026-11-03.
- Code audité : `upstream/main` au commit `c83ac68b3` (version de workspace `0.10.2`, `Cargo.toml:23`).
- Binaire mesuré : `0.10.2-continuous.4807b98c56eb2de868cbac824a453a62ca955a0e`, construit en CI par `.github/workflows/ya-build-macos-arm64.yml` (run 41601230885). Archive : sha256 `5d2d7cf9198877793489c7cb38e133364dff2ad8420fc7823548c58a113248a9`. Le code du harness est identique à `c83ac68b3` : `git diff --stat c83ac68b3 4807b98c5 -- crates prime-agent-runtime Cargo.toml Cargo.lock skills` ne sort rien.
- Run de mesure de référence : `bench/tools/capture/runs/20261103T083549Z/` (local, non versionné). Les chiffres sont relançables, voir §3.
- Convention : « est. » marque une estimation. Les tokens sont comptés avec tiktoken `o200k_base`, qui n'est le tokenizer de facturation d'aucun des providers capturés.

## Résumé

Le CDC décrit un hôte TypeScript avec un kernel IPython. Le code est un workspace Rust d'environ 620 k lignes (§1.1), avec un REPL CPython maison (§1.4).

Points saillants pour le cache et les tickets suivants :

- **Un seul tool exposé au modèle : `python_repl`.** Mesuré dans les captures, et visible dans `crates/pa-core/src/session_engine/engine.rs:517-529`. `bash`, `edit`, `read` sont des fonctions du kernel.
- **Prompt système trop gros.** 30 842 caractères, soit 7 500 tokens est. (cible YA-09 : < 2 000). Les trois couches statiques en représentent 5 636 tokens est., l'inventaire des skills 1 854.
- **Prompt système stable dans une session.** Il est identique octet pour octet à chaque tour, après un crash du kernel, après une reprise `--continue` le même jour et après une compaction.
- **Prompt système différent à chaque session.** Il contient la date, le cwd et le chemin du journal de session, donc l'id de session (`crates/pa-core/src/prompts/system_prompt.rs:279-281`).
- **Messages en append-only strict sur OpenAI Responses.** Côté Anthropic, ils le sont à une exception près : le marqueur `cache_control` passe du dernier message utilisateur de N à celui de N+1. Aucune autre différence.
- **Le kernel hérite de tout l'environnement du harness, secrets compris.** Mesuré : il voit `ANTHROPIC_API_KEY` et `YA_CAPTURE_FAKE_TOKEN`. Cause : `crates/pa-core/src/kernel/manager/startup.rs:532-545`.
- **La compaction ne conserve pas les messages utilisateur mot pour mot.** Mesuré : le premier prompt disparaît, remplacé par un résumé.
- **Usage sans tokens de raisonnement.** La structure `Usage` n'a pas de champ dédié (`crates/pa-types/src/ai/mod.rs:423-433`). Au `message_start` Anthropic, l'usage est lu au mauvais niveau du JSON (`crates/pa-ai/src/providers/anthropic/stream.rs:297-306`). Seul le `message_delta` le renseigne.
- **ACP opérationnel**, mais sans `session/request_permission` ni `session/load`.

---

## 1. Cartographie

### 1.1 Packages du monorepo

Workspace Cargo de 10 crates `pa-*` (`AGENTS.md:194-205`). Lignes `.rs` comptées par `find crates/<c> -name '*.rs' | xargs cat | wc -l`, tests inclus.

| Crate | Rôle | Lignes .rs |
|---|---|---|
| `pa-types` | Vocabulaire partagé : messages, usage, entrées JSONL, protocole daemon (`crates/pa-types/README.md`) | 13 436 |
| `pa-agent` | Boucle agent indépendante du provider (`crates/pa-agent/README.md`) | 6 060 |
| `pa-ai` | Providers, registre de modèles, streaming, OAuth, provider factice `faux` (`crates/pa-ai/README.md`) | 40 293 |
| `pa-models` | Catalogue de modèles en ligne, cache disque, snapshot embarqué (`crates/pa-models/README.md`) | 4 959 |
| `pa-core` | Moteur de session : tools, kernel, prompt, compaction, skills, settings, MCP côté hôte (`crates/pa-core/README.md`) | 137 160 |
| `pa-daemon` | Superviseur, un worker par session, protocole fil, serveur ACP (`crates/pa-daemon/README.md`) | 180 999 |
| `pa-tui` | Interface terminal (`crates/pa-tui/README.md`) | 152 343 |
| `pa-cli` | Binaire `prime-agent`, racine de composition, modes print/json/rpc/acp (`crates/pa-cli/README.md`) | 78 042 |
| `pa-telemetry` | Télémétrie produit, file d'attente, sinks (`crates/pa-telemetry/README.md`) | 4 987 |
| `pa-perf` | Bancs de performance, jamais livré (`crates/pa-perf/README.md`) | 4 706 |

**Runtime Python du kernel** (`prime-agent-runtime/`, 13 355 lignes dans `src/rlm`, 32 443 avec les tests) :

- C'est un REPL CPython minimal qui parle du JSON délimité par lignes sur stdio (`prime-agent-runtime/src/rlm/repl.py:1-6`), pas IPython.
- Contenu du module :
  - `bash()` : `prime-agent-runtime/src/rlm/bash.py:1298`, avec shells persistants (`bash.py:141-145`) ;
  - l'API `rlm.*` : `prime-agent-runtime/src/rlm/__init__.py` ;
  - le client MCP : `mcp.py`, `mcp_base.py`.
- Dépendances : `mcp`, `tyro` (`prime-agent-runtime/pyproject.toml`).
- Skills Python préchargés dans le kernel (`skills/`) : `edit`, `compact`, `goal`, `refine`, `agent-message`, `attach-image`, `rlm-heartbeat`, `websearch`, `factory`, `mcp`, etc.

### 1.2 Boucle agent

- Point d'entrée : `run_agent_loop` (`crates/pa-agent/src/agent_loop/entry.rs:15`).
- Boucle : `run_loop` (`crates/pa-agent/src/agent_loop/run.rs:21`), avec une boucle externe (`run.rs:45`) et une boucle interne tant qu'il reste des tool calls ou des messages en attente (`run.rs:47-103`).
- Arrêt : le modèle répond sans tool call (`has_more_tool_calls = false`, `run.rs:98`) ou un lot de tools demande l'arrêt (`run.rs:103`). Les messages de pilotage et de suivi sont ensuite relus (`run.rs:180-245`).
- Exécution des tools : en parallèle, sauf si un tool est déclaré séquentiel (`crates/pa-agent/src/agent_loop/tools.rs:55-58`). `python_repl` est séquentiel (`crates/pa-core/src/tools/ipython.rs:549`).
- Avant chaque requête, le contexte passe par `transform_context` (`crates/pa-agent/src/agent_loop/response.rs:88`) puis `convert_to_llm`. Le moteur branche un `transform_context` sans effet (`crates/pa-core/src/session_engine/engine.rs:724-729`).

### 1.3 Construction du prompt système

- Assemblage : `system_prompt_breakdown` (`crates/pa-core/src/prompts/system_prompt.rs:112-237`). Il est appelé une fois à la construction du moteur de session (`crates/pa-core/src/session_engine/engine.rs:568-603`).
- Couches statiques, dans l'ordre (`system_prompt.rs:114-147`) : `core.md`, `usage.md`, `opinionated.md` (`crates/pa-core/src/prompts/layers/`), plus les ajouts par modèle de `model_prompts.toml` si le sélecteur correspond.
- Queue dynamique, ordre figé (`system_prompt.rs:156-221`) : paquets préinstallés, contexte projet (AGENTS.md / CLAUDE.md), inventaire des skills, serveurs MCP, environnement, rôle RLM, `--append-system-prompt`.
- La section environnement contient trois lignes qui varient (`system_prompt.rs:271-294`) :
  - `Current date: YYYY-MM-DD` (date UTC, `system_prompt.rs:348`) ;
  - `Working directory:` (le cwd) ;
  - `Conversation log:` (chemin du JSONL de session, qui contient l'id de session).
- En plus du prompt système, un message utilisateur `[harness-digest]` (état du « continual harness ») est placé en tête de contexte au démarrage, à la reprise et après compaction (`crates/pa-core/src/session_engine/harness_digest.rs:1-4`).
- Ventilation mesurée : voir §4.1.

### 1.4 Tool exposé et enregistrement des tools

- `python_repl` : nom dans `crates/pa-types/src/ai/mod.rs:253`, avec les alias `python` et `ipython` acceptés (`mod.rs:257`). Schéma et description : `crates/pa-core/src/tools/ipython.rs:344-359`. Définition : `ipython.rs:509-550`.
- Enregistrement : `SessionEngineConfig.tools`, complété par `python_repl` s'il est absent (`crates/pa-core/src/session_engine/engine.rs:517-530`). Les tools passent par le pont `ToolDefinitionBridge` (`crates/pa-core/src/session_engine/tool_bridge.rs`). Le type `ToolDefinition` est défini dans `crates/pa-core/src/tools/tool_definition.rs:92-116`.
- Les chemins de production passent tous une liste vide (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1168`, `crates/pa-cli/src/print_session_command.rs:337`). C'est le point d'entrée pour un tool `edit` séparé (YA-03).
- Le prompt sait déjà adapter le texte quand un tool `bash` est présent (`system_prompt.rs:163`, `engine.rs:625`) : trace de l'ancienne surface à trois tools.
- `AGENTS.md:183` (« Tools exposed to the model: `bash`, `edit`, `python_repl` ») ne correspond plus au code.
- `edit` existe sous forme de skill du kernel (`skills/edit/SKILL.md` : `await edit(path=..., old_str=..., new_str=...)`).
- Plafond de sortie par cellule : 1 Mio de texte (`prime-agent-runtime/src/rlm/repl.py:53`), loin des 8 000 tokens du CDC (YA-03). Mesuré : une sortie de 40 000 caractères arrive entière dans le contexte (cas `compact-anthropic`).

### 1.5 Couche providers

- Adaptateurs (`crates/pa-ai/src/providers/mod.rs:3-20`) :
  - `anthropic` ;
  - `openai_responses`, `azure_openai_responses`, `openai_codex_responses` ;
  - `openai_completions` (API compatibles) ;
  - `google`, `google_vertex` ;
  - `mistral` ;
  - `bedrock` ;
  - `faux` (provider factice).
  - OpenRouter, Kimi et DeepSeek passent par `openai_completions` via le catalogue.
- Matrice des modèles : catalogue en ligne avec cache disque, snapshot embarqué `models.bundled.json` et repli compilé (`crates/pa-models/README.md`, `crates/pa-ai/src/models_generated.rs`, 155 entrées `id:`).
- Retries : politique par défaut de 3 essais, base 2 s, plafond 60 s (`crates/pa-core/src/session_engine/provider_retry.rs:26-32`). Réglages `retry.*` dans `crates/pa-core/src/settings/types.rs:159-175`. Il existe aussi une bascule (`provider_failover.rs`) et une mise en attente (`provider_park.rs`).
- Options de stream construites dans `crates/pa-core/src/session_engine/provider_adapter.rs:150-170`, avec `cache_retention: None` (`:161`), `session_id` (`:162`) et le hook `on_payload` qui peut lire ou remplacer le corps envoyé (`:164`).

### 1.6 Cache

**Anthropic** (`crates/pa-ai/src/providers/anthropic/`) :

- Trois breakpoints `cache_control` sur quatre possibles :
  - le bloc système (`params.rs:68-79`) ;
  - le dernier tool (`convert.rs:314-318`) ;
  - le dernier bloc du dernier message utilisateur (`convert.rs:244-279`).
  - En login par abonnement, un bloc système d'identité supplémentaire porte aussi un marqueur (`params.rs:42-67`), soit quatre.
- TTL : 5 min par défaut. 1 h seulement si `PI_CACHE_RETENTION=long` et si le modèle le supporte (`mod.rs:186-195`, `mod.rs:220-235`). Aucun réglage dans `settings.json`.
- `max_tokens` = valeur passée par le moteur, sinon `model.max_tokens / 3` (`params.rs:37-38`). Mesuré : 32 000, soit le `maxTokens` du modèle.

**OpenAI Responses** :

- `prompt_cache_key` = id de session (`crates/pa-ai/src/providers/openai_responses.rs:211-214`).
- `prompt_cache_retention: "24h"` seulement en rétention longue (`:71-80`, `:216-217`).
- `store: false`.

**Usage remonté** :

- `Usage { input, output, cacheRead, cacheWrite, totalTokens, cost }` (`crates/pa-types/src/ai/mod.rs:423-433`). Pas de champ pour le raisonnement.
- Anthropic : au `message_start`, les champs sont lus sur `message.*` au lieu de `message.usage.*` (`crates/pa-ai/src/providers/anthropic/stream.rs:297-306`). Seul `message_delta` remplit l'usage (`stream.rs:590-600`).
  - Constat sur la capture : avec un `message_delta` qui ne portait que `output_tokens`, l'usage enregistré valait `input: 0`.
  - L'API réelle renvoie l'usage complet dans `message_delta`, ce qui masque le problème. À confirmer sur un vrai appel, uniquement sur demande de Yusuf.
- Le coût d'écriture du cache distingue 5 min et 1 h (`crates/pa-ai/src/cache_pricing.rs:5-64`).

### 1.7 Sessions JSONL

- Format : append-only. La première ligne est l'en-tête `{"type":"session","version":3,"id",...}`, chaque ligne suivante une entrée (`crates/pa-types/src/session.rs:1-4`, `:29-50`).
- Version courante 3 (`crates/pa-core/src/session/mod.rs:19`), avec migration des versions antérieures (`mod.rs:266`).
- Types d'entrées (`session.rs:350-433`) : message, `model_change`, `thinking_level_change`, `compaction`, `custom_message`, `tool_started`, `git_state`, etc.
- Reprise : `--continue` / `--resume`. Mesurée : même prompt système et préfixe conservé (§4.3).

### 1.8 Compaction

- Seuil par défaut : 95 % de la fenêtre, plafonné par la marge de sortie. `keepRecentTokens` 20 000, `reserveTokens` 16 384 (`crates/pa-core/src/session_engine/compaction.rs:12-18`, `:152-200`).
- Déclenchement : à la frontière de tour, sur l'usage du dernier message assistant (`compaction.rs:274-317`). En mode print, avant chaque prompt et après chaque tour réglé (`crates/pa-cli/src/print_boundary.rs:93-180`).
- Sautée si rien n'est antérieur au point de coupe (`crates/pa-core/src/session_engine/compact_session/prepare.rs:111-112`).
- Contenu : tout ce qui précède le point de coupe est résumé selon un gabarit fixe (`compaction.rs:455`). Le gabarit inclut une rubrique « Constraints & Preferences », mais **les messages utilisateur ne sont pas conservés tels quels**. Mesuré, §4.4.
- Contexte reconstruit : `rebuilt_context_after_compaction` (`crates/pa-core/src/session_engine/compact_session.rs:567`). Après compaction, un message `[python-state]` liste les variables du kernel encore définies (`crates/pa-core/src/session_engine/ipython_state.rs:148`).
- Effet de bord : la compaction déclenche un appel « auto /refine gate » (réglage `autoRefine.compact`, `crates/pa-core/src/settings/types.rs:121-126`).

### 1.9 `rlm()`

- `rlm.spawn(prompt, name=..., model=..., thinking=..., cwd=...)` renvoie un **handle** (`RLMSpawnHandle`) dès que la tâche est admise (`prime-agent-runtime/src/rlm/__init__.py:28-33`, `:179-196`).
- Le résultat se récupère avec `rlm.collect(targets, timeout_ms=...)` (`__init__.py:430-467`).
- Contexte du sous-agent : une session Prime Agent complète (`rlm_depth` + 1), avec tous les tools. Ce n'est pas un mode lecture seule.
- Profondeur maximale : réglage `rlmMaxDepth` (`crates/pa-core/src/settings/types.rs:275`).

### 1.10 Gate autonome

- Options CLI : `--autonomous`, `--autonomous-gate` et limites (`prime-agent --help`). Valeurs par défaut dans `crates/pa-core/src/autonomous/mod.rs:22-27` : 3 continuations, 12 tours, 80 000 tokens, 3 retries par gate, 5 min par gate.
- « Pas de relance sans changement du workspace » : déjà implémenté pour les gates, sans option de désactivation. Un gate en échec n'est pas relancé si l'instantané git du worktree est identique (`crates/pa-core/src/autonomous/gates.rs:155-180`, message `not rerun: workspace unchanged since previous failed gate`).
- Rien d'équivalent pour les commandes de test lancées pendant la tâche (YA-07).

### 1.11 Lancement du kernel

- Process `python -m rlm.repl` lancé par `do_start` (`crates/pa-core/src/kernel/manager/startup.rs:126-180`).
- Le venv est créé par uv dans `$HOME/.prime/agent/kernel-venv`, avec un CPython téléchargé par uv. Mesuré : `PRIME_AGENT_CODING_AGENT_DIR` ne déplace pas ce venv.
- **Environnement** : `env_clear()` suivi de `envs(kernel_spawn_env(...))`, où `kernel_spawn_env` part de `std::env::vars()`, donc de tout l'environnement du harness (`startup.rs:180`, `:532-545`).
  - Les clés API et jetons présents dans l'environnement sont visibles depuis le kernel et `bash()`.
  - Mesuré : la cellule du tour 2 liste `['ANTHROPIC_API_KEY', 'YA_CAPTURE_FAKE_TOKEN']`.
- Redémarrage après crash, mesuré (`crash-anthropic`) :
  - la cellule qui tue le kernel renvoie `Kernel has been shut down` ;
  - la cellule suivante relance le kernel sans le signaler et renvoie `2\n` ;
  - aucun message « kernel redémarré, variables perdues » n'est injecté.
  - Le texte `<ipython_kernel_reset>` (`crates/pa-core/src/tools/ipython.rs:214-216`) ne concerne que le chemin « interruption puis kill » choisi par l'utilisateur.

### 1.12 Modes headless

- `--mode text|json|rpc|acp|daemon` (`crates/pa-cli/src/args.rs:49-60`).
- `json` (avec `-p`) pilote le moteur de session dans le process (`crates/pa-cli/README.md`, `print_runtime.rs`). C'est le mode utilisé par le banc.
- `rpc`, `acp` et `daemon` passent par le daemon (`crates/pa-cli/src/lib.rs:168`). Point d'entrée ACP : `run_acp_mode` (`crates/pa-cli/src/print_runtime.rs:85`).

**Méthodes ACP** (`crates/pa-daemon/src/acp/daemon.rs`) :

| Méthode | Statut | Référence |
|---|---|---|
| `initialize` | implémentée | `daemon.rs:1020` |
| `session/new` | implémentée | `daemon.rs:1023` |
| `session/prompt` | implémentée | `daemon.rs:656` |
| `session/cancel` | implémentée (notification) | `daemon.rs:700` |
| `session/update` | émise | `crates/pa-daemon/src/acp/producer.rs:175` |
| `session/close`, `session/set_config_option` | implémentées (en plus) | `daemon.rs:669`, `:1026` |
| `session/request_permission` | absente (aucune occurrence dans `crates/pa-daemon/src/acp/`) | — |
| `session/load` | absente (`loadSession: false` dans la réponse à `initialize`, mesuré) | — |

Mesuré (§4.5) : un échange `initialize` → `session/new` → `session/prompt` aboutit avec `stopReason: end_turn`, et produit des notifications `tool_call`, `tool_call_update`, `agent_message_chunk` et `session_info_update`.

### 1.13 Fonctions hors MVP et réglages existants

| Fonction | Où | Réglage pour désactiver |
|---|---|---|
| Continual Harness, `/refine` | `crates/pa-core/src/refinement/`, `session_engine/refine.rs`, skill `skills/refine` | `harnessRefinements: false`, interrupteur général, actif par défaut (`crates/pa-core/src/settings/types.rs:285`, `interactive_settings.rs:13-14`) ; `autoRefine.enabled`, `autoRefine.compact` (`types.rs:121-126`) |
| Messagerie entre agents | `crates/pa-core/src/session_engine/agent_messaging/`, skill `agent-message` | aucun réglage dédié trouvé ; skill désactivable par la config des skills |
| Heartbeats, schedules | `crates/pa-core/src/cron/`, skill `rlm-heartbeat`, commande `schedule` | aucun réglage dédié trouvé |
| Factory | skill `factory` | `factory.enabled`, désactivé par défaut (`types.rs:236-245`) |
| Daemon | `pa-daemon` | inévitable pour ACP ; le mode `-p --mode json` s'en passe |
| Télémétrie produit | `crates/pa-telemetry/`, endpoint `https://api.primeintellect.ai/api/v1/agent-analytics/events` (`crates/pa-telemetry/src/sinks/analytics.rs:20`) | `telemetry.enabled=false`, `DO_NOT_TRACK=1`, `PRIME_AGENT_TELEMETRY=0`, `PI_OFFLINE=1` (`crates/pa-cli/src/mode.rs:193`, `:295`) ; active par défaut avec notice (`crates/pa-cli/src/telemetry_notice.rs:23`, constaté sur un run ponctuel) |

### 1.14 Login par abonnement Claude (localisé, non modifié)

- Flux OAuth : `crates/pa-ai/src/oauth/anthropic.rs:19` (`https://claude.ai/oauth/authorize`), `login_anthropic` (`:49`), `refresh_anthropic_token` (`:88`), callback (`anthropic_callback.rs`).
- Appel CLI : `crates/pa-cli/src/subscription_login.rs:115`. Rafraîchissement : `crates/pa-core/src/auth/provider_oauth.rs:109`.
- Détection du jeton (`sk-ant-oat`) : `crates/pa-ai/src/providers/anthropic/mod.rs:295-296`.
- Effets sur la requête :
  - identité « Claude Code » ajoutée en tête du système (`params.rs:42-67`) ;
  - noms de tools convertis (`convert.rs:295-299`, `to_claude_code_name`).

---

## 2. Installation et tests upstream

- **Pas de `npm ci`** : il n'y a plus de paquet Node. L'installation passe par `cargo build`, interdit en local (CLAUDE.md règle 8). Les tests Rust ont donc tourné en CI uniquement.
- **CI du fork** (run 41600446529, `ci.yml` sur le push de `c83ac68b3`, 12 min 35 s de bout en bout) :
  - 18 jobs sur 20 en succès, dont `fmt`, `check + clippy`, `deny`, `runtime suite (python 3.11)` et les shards de tests 2 à 8 ;
  - **le shard 1 échoue avant ses tests** : l'étape `Prepare pinned TS frame comparison binary` télécharge la release `v0.9.8` depuis le dépôt courant (`.github/workflows/ci.yml:347-361`, ligne 355). Le fork n'a pas de release (« release not found »). Le job `test summary` échoue par conséquence.
  - Sur upstream, le même commit est vert (`gh run list --repo PrimeIntellect-ai/prime-agent --workflow ci`).
  - Commande : `gh run view 41600446529 --repo YusufByn/yusagent`.
- **Runtime Python en local** : 754 tests, OK, 0 ignoré, 53,1 s (55,7 s mur, venv compris). Commande :
  ```bash
  cd prime-agent-runtime && UV_PROJECT_ENVIRONMENT=<dossier jetable> uv run --python 3.11 python -m unittest discover -s test
  ```
- **Provider factice et fixtures** :
  - provider `faux` : `crates/pa-ai/src/providers/faux/` (`mod.rs`, `script.rs`), utilisé par les tests e2e (`crates/pa-daemon/tests/*_e2e.rs`) ;
  - transcripts ACP de référence : `crates/pa-daemon/testdata/acp/` (`rust-happy_path.jsonl`, `rust-tool_call.jsonl`, `rust-cancel.jsonl`...) ;
  - rejeu doré : `crates/pa-core/src/tools/golden_replay.rs` ;
  - boucle scriptée : `crates/pa-agent/src/scripted.rs`.

**Tailles** (mesurées avec `du -sh`, `stat -f %z`, `git count-objects -vH`) :

| Élément | Taille |
|---|---|
| Dépôt cloné, avant l'audit | 153 Mo, dont `.git` 117 Mo (`size-pack` 112,89 Mio) |
| `node_modules` | sans objet |
| Archive du binaire de HEAD (darwin-arm64) | 21 943 868 octets |
| Binaire `prime-agent` seul | 61 528 704 octets (59 Mo) |
| Dossier installé hors venv (binaire, runtime, skills, catalogues, wasm) | 62 Mo |
| Venv du kernel (`~/.prime/agent/kernel-venv`) | 220 Mo |
| CPython géré par uv | 74 Mo |
| Cache uv après installation | 237 Mo |
| Venv de test du runtime | 29 Mo |

Empreinte installée hors venv : **62 Mo mesurés**, au-dessus de la cible CDC de 50 Mo. Le binaire seul en fait 59. Le CDC compte aussi l'extension, ce qui creuse l'écart.

---

## 3. Serveur de capture

- Fichiers dans `bench/tools/capture/` :
  - `server.py` : serveur stdlib, formats Anthropic Messages et OpenAI Responses, en SSE ou JSON ;
  - `run.py` : orchestration ;
  - `analyze.py` : analyse ;
  - `sandbox.sb` : profil réseau limité à localhost ;
  - `README.md` : mode d'emploi et méthode pour pointer le harness vers le serveur (`models.json` de provider personnalisé).
- Scénario de base : tour 1 `print(1)`, tour 2 liste des noms de variables sensibles vues par le kernel, tour 3 réponse finale.
- Commande unique :
  ```bash
  python3 bench/tools/capture/run.py --binary <chemin du binaire de HEAD>
  ```
- Réseau : chaque lancement du binaire et l'analyse passent par `sandbox-exec -f bench/tools/capture/sandbox.sb`. Le contrôle intégré (`curl https://example.com` dans le même profil) échoue avec `Could not resolve host`.
- Les tentatives de connexion bloquées ne sont pas observables dans cette configuration : le journal système ne montre pas les refus du sandbox. Le harness tourne en plus avec `DO_NOT_TRACK=1 PI_OFFLINE=1`.
- La préparation (venv du kernel, tiktoken) utilise le réseau, une fois, avant les mesures.
- Le binaire v0.10.2 de la release upstream (`gh release download v0.10.2`, sha256 vérifié) a servi au développement du serveur. Aucun chiffre de ce rapport n'en vient.

---

## 4. Mesures (binaire de HEAD)

### 4.1 Préambule au premier appel

Source : `report.md` du run de référence.

| Élément | Caractères | Tokens est. (o200k) |
|---|---|---|
| Prompt système | 30 842 | 7 500 (de 7 499 à 7 508 sur deux runs : l'id de session dans le chemin du journal change la tokenisation) |
| Définition des tools (JSON Anthropic, un seul tool `python_repl`) | 854 | 182 |
| Message `[harness-digest]` (premier message utilisateur) | 2 657 | 537 |
| Corps complet de la requête | 35 138 octets | — |

Ventilation du prompt système (`prime-agent prompt --json`, octets UTF-8) :

| Couche | Type | Octets | Tokens est. |
|---|---|---|---|
| core (`layers/core.md`) | statique | 15 337 | 3 882 |
| usage (`layers/usage.md`) | statique | 6 277 | 1 278 |
| opinionated (`layers/opinionated.md`) | statique | 2 385 | 476 |
| packages | dynamique | 263 | 72 |
| skills-inventory | dynamique | 6 689 | 1 854 |
| environment | dynamique | 107 (217 en session réelle, avec le chemin du journal) | 31 |
| session-role | dynamique | 433 | 112 |

Cible YA-09 (< 2 000 tokens hors tools) : dépassée d'un facteur 3,7 est. Les couches statiques seules font 5 636 tokens est.

### 4.2 Préfixe N → N+1

Sur les 6 cas, 15 paires de requêtes consécutives avec tools :

- **Prompt système, tools et paramètres** (`model`, `max_tokens`, `stream`, `prompt_cache_key`, `store`) : identiques octet pour octet dans toutes les paires. Cela vaut aussi à travers un crash du kernel, une reprise `--continue` et une compaction.
- **Messages** :
  - OpenAI Responses : les messages de N forment un préfixe exact de ceux de N+1.
  - Anthropic : une seule différence par paire, toujours la même. Le dernier message utilisateur de N porte `cache_control`, que N+1 retire pour le placer sur son propre dernier message (`crates/pa-ai/src/providers/anthropic/convert.rs:244-279`). Sans le marqueur, le contenu est identique.
  - C'est le schéma documenté par Anthropic pour le cache incrémental. Il ne fait pas de miss côté provider, mais un test « octet par octet » naïf (YA-04) doit ignorer `cache_control`.
- **Fidélité de la comparaison** : la re-sérialisation compacte du corps parsé, ordre des clés conservé, reproduit exactement les octets reçus pour 100 % des requêtes. Les comparaisons par partie sont donc exactes à l'octet.
- **Ordre des clés** : dans le corps Anthropic, `messages` précède `system` et `tools`. Le préfixe brut du corps diverge donc dès le premier message ajouté. C'est sans effet sur le cache, que le provider construit dans l'ordre tools → système → messages.

### 4.3 D'une session à l'autre

Deux sessions successives, même projet, même dossier agent, même jour :

- Tools identiques, digest identique.
- **Prompt système différent** sur une seule ligne :
  ```text
  Conversation log: <agent>/sessions/<id de session>.jsonl
  ```
  Cause : `crates/pa-core/src/prompts/system_prompt.rs:281`.
- Non mesuré, mais certain d'après le code :
  - `Current date:` change chaque jour (`system_prompt.rs:279`, `:348`) ;
  - `Working directory:` change avec le projet (`:280`).
- Conséquence côté Anthropic : le bloc système, environ 7 500 tokens est., est un seul bloc texte avec breakpoint. Toute nouvelle session le réécrit en entier ; seuls les tools (182 tokens est.) se partagent.
- `prompt_cache_key` = id de session : la clé change aussi à chaque session.
- Reprise `--continue` le même jour : prompt système identique (même id de session), messages en préfixe exact hors `cache_control`, pas de nouveau digest.

### 4.4 Reconstruction du prompt : `/refine`, compaction, redémarrage du kernel

| Événement | Prompt système | Messages | Source |
|---|---|---|---|
| Crash et redémarrage du kernel | identique | append-only ; la cellule fautive renvoie `Kernel has been shut down`, la suivante relance le kernel sans avertir | cas `crash-anthropic` |
| Reprise de session (`--continue`) | identique le même jour, reconstruit au démarrage du process (date, voir §4.3) | préfixe conservé | cas `resume-anthropic` |
| Compaction par seuil | identique | réécrits (voir ci-dessous) | cas `compact-anthropic` |
| `/refine` | non reconstruit d'après le code : le prompt de base est fixe, l'état du harness passe par le message `[harness-digest]` | digest mis à jour aux frontières froides (`harness_digest.rs:1-4`) | statique, non déclenché dans le banc |

Après la compaction mesurée :

- Le premier message utilisateur contient le digest et le résumé (`<summary>…</summary>`).
- La queue conservée suit, puis `[python-state]`, un nouveau digest et le prompt 2.
- **Le prompt 1 (« Exécute le scénario de capture. ») n'apparaît plus nulle part.**
- Deux appels annexes, chacun avec son propre prompt système, ont été émis :
  - le résumé (« You are a context summarization assistant… ») ;
  - la porte auto `/refine` (« You are Prime Agent's automatic /refine review gate… »).

Conditions du cas :

- sorties de 40 000 caractères par cellule ;
- usage annoncé de 199 000 tokens sur une fenêtre de 200 000 ;
- réglages par défaut. Avec des sorties courtes, la compaction est sautée (« Session is too short to compact »).

### 4.5 ACP

Échange `initialize` / `session/new` / `session/prompt` sur un daemon isolé (`--daemon-socket`) :

- `protocolVersion` 1.
- `agentCapabilities` : `{"loadSession":false,"promptCapabilities":{"image":true,"embeddedContext":true},"mcpCapabilities":{"http":true},"sessionCapabilities":{"close":{}}}`.
- `session/prompt` renvoie `{"stopReason":"end_turn"}`.
- Aucune requête agent → client : pas de demande de permission.
- Aucun process restant après arrêt.

Point d'attention : sans `--daemon-socket`, le mode ACP démarre un daemon sur le socket partagé `/tmp/prime-agent-<uid>/daemon.sock`. `prime-agent shutdown` ne vise que ce socket par défaut.

---

## 5. Points d'entrée pour les tickets suivants

Écart avec upstream : faible = nouveau module et quelques lignes de branchement ; moyen = modification localisée de fichiers upstream ; fort = réécriture d'un comportement upstream.

| Ticket | Où insérer | Fichiers à toucher | Écart |
|---|---|---|---|
| YA-04 test de préfixe | Nouveau test sur les corps de requête. Option 1 : `on_payload` (`provider_adapter.rs:164`), qui expose le corps exact. Option 2 : un test d'intégration avec le provider `faux` ou ce serveur de capture. La comparaison doit ignorer `cache_control`. | nouveau fichier de test dans `pa-core` ou `pa-cli/tests`, plus la CI | faible |
| YA-04 retirer le volatil du prompt système | Sortir `Current date`, `Working directory` et `Conversation log` de `environment_section`, pour les mettre dans le premier message utilisateur (instantané d'environnement, YA-09) | `crates/pa-core/src/prompts/system_prompt.rs:271-294`, `engine.rs:568-603` | moyen (fichier upstream central du prompt) |
| YA-04 breakpoints et TTL | Déjà 3 breakpoints automatiques. Le TTL 1 h ne s'active que par `PI_CACHE_RETENTION=long`, il faut un réglage. `cache_retention: None` est passé en dur. | `crates/pa-core/src/session_engine/provider_adapter.rs:161`, réglage dans `settings/types.rs` | faible |
| YA-04 `prompt_cache_key` | Déjà égal à l'id de session (Responses, Azure, Codex). Rien à faire pour le MVP. | — | nul |
| YA-05 masquage | Le hook `transform_context` existe et le moteur y branche une fonction sans effet (`engine.rs:724-729`). Il suffit d'y brancher un module de masquage. Le masquage par paliers doit rester déterministe, sinon chaque palier casse le cache. | nouveau module `pa-core/src/session_engine/masking.rs` (ou crate dédiée), plus 3 ou 4 lignes dans `engine.rs` | faible |
| YA-05 messages utilisateur conservés | Modifier la reconstruction après compaction pour réinjecter les messages utilisateur tels quels | `compact_session.rs:567`, `compaction.rs:389-455` | moyen à fort |
| YA-03 tool `edit` séparé | Ajouter une `ToolDefinition` à `SessionEngineConfig.tools`. Le skill kernel `edit` peut servir de backend. | nouveau `pa-core/src/tools/edit.rs`, branchement dans `lifecycle.rs:1168` et `print_session_command.rs:337` (listes vides aujourd'hui) | faible à moyen |
| YA-03 helper `bash()` | Existe déjà (`prime-agent-runtime/src/rlm/bash.py:1298`, avec mode persistant). Écart avec le CDC à évaluer : shell principal persistant par défaut, magics. Le REPL n'est pas IPython, il n'y a pas de magics à rediriger. | `prime-agent-runtime/src/rlm/bash.py` | moyen |
| YA-03 `read` numéroté | Nouveau skill kernel (même modèle que `skills/edit`) | `skills/read/` (nouveau) | faible |
| YA-03 environnement filtré | Remplacer l'héritage complet par une liste blanche | `crates/pa-core/src/kernel/manager/startup.rs:532-545` | faible en lignes, mais fichier upstream |
| YA-03 plafond de sortie | 1 Mio aujourd'hui (`repl.py:53`) contre 8 000 tokens dans le CDC | `prime-agent-runtime/src/rlm/repl.py` | faible |
| YA-06 `rlm()` en lecture seule | `rlm.spawn` lance un enfant complet. Il faut une variante qui retire `edit` et l'écriture, ajoute un résumé plafonné et des extraits en variable, avec un réglage `rlm`. | `prime-agent-runtime/src/rlm/__init__.py`, hôte `crates/pa-core/src/session_engine/rlm_host.rs` | moyen |
| YA-13 journal d'usage | `on_payload` et `on_response` (`provider_adapter.rs:164`) plus l'usage des `AssistantMessage`. Ajouter un champ raisonnement (`pa-types/src/ai/mod.rs:423-433`, crate partagée) et corriger la lecture de `message_start` (`anthropic/stream.rs:297-306`). | nouveau module de journal, plus `pa-types` et `pa-ai` | moyen (touche `pa-types`, `pa-ai`) |
| YA-10 ACP pour VS Code | Ajouter `session/request_permission` (flux de permission du kernel) et éventuellement `session/load`. Isoler le daemon (`--daemon-socket`). | `crates/pa-daemon/src/acp/daemon.rs`, `producer.rs` | moyen |

---

## Écarts avec le CDC

| Sujet | CDC | Code et mesures | Conséquence |
|---|---|---|---|
| Cœur | « hôte TypeScript de Prime Agent » (journal du 31/10/2026, Résumé, YA-01) | Workspace Rust de 10 crates, environ 620 k lignes (§1.1). Le TS ne vit plus qu'au tag `v0.9.8` (`AGENTS.md:24-27`). | Toute modification du cœur est en Rust. Les `cherry-pick` porteront sur du Rust. Le journal des décisions et le ticket 00 (`npm ci`) sont à mettre à jour. |
| Tools exposés | « `python` et `edit` », pas de `bash` (YA-03). Ce ticket et `AGENTS.md:183` annoncent trois tools : `bash`, `edit`, `python_repl`. | **Un seul tool : `python_repl`**, mesuré dans toutes les captures et visible dans `engine.rs:517-529`. `bash()` et `edit()` sont des fonctions du kernel. La surface à trois tools d'`AGENTS.md:183` est obsolète. | Bash est déjà dans le kernel, conformément au CDC. `edit` reste à exposer comme tool séparé (YA-03). La variante « bash natif » du banc (YA-14) est à écrire : le code garde seulement des branches de prompt pour un tool `bash` (`system_prompt.rs:163`). |
| Kernel | IPython persistant, magics shell redirigées (YA-03) | REPL CPython minimal sur stdio (`repl.py:1-6`), sans IPython ni magics | Le critère « magics redirigées » ne s'applique pas. Une cellule `!cmd` est une erreur de syntaxe Python, à compter dans les « cellules en erreur d'appel ». |
| Télémétrie réseau | Aucune télémétrie distante par défaut (YA-13) | Télémétrie produit **active par défaut**, vers `api.primeintellect.ai` (`analytics.rs:20`), avec un identifiant d'installation. Le catalogue de modèles est récupéré en ligne (`pa-models`). Il y a une vérification de version. Désactivation : `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PRIME_AGENT_TELEMETRY=0`, `telemetry.enabled=false`. | À désactiver par défaut dans le fork (étape 3 de l'ordre de démarrage). Le banc et la CI doivent fixer `DO_NOT_TRACK=1 PI_OFFLINE=1`, comme le fait `pa-perf` (`crates/pa-perf/src/bench/runner.rs:117`). |
| Boucle de dev | Développement local, builds lourds en CI | Tout test Rust et tout binaire exige `cargo build` (le shard de tests CI compile en environ 2 min sur un runner). Interdit en local. | Boucle de dev actuelle : push sur une branche `ya-*` → binaire macOS arm64 en environ 8 min (`ya-build-macos-arm64.yml`, run 41601230885 : 8 min 12 s) → mesures locales avec le banc de capture. Les tests Rust tournent dans `ci.yml` uniquement sur `main` et les PR. Seul le runtime Python se teste en local. |
| Secrets | Jamais transmis au kernel ni à `bash()` (journal 31/10, YA-03, YA-11) | Le kernel hérite de tout l'environnement (`startup.rs:532-545`). Mesuré. | Bloquant pour YA-11 ; correctif localisé (§5). |
| Compaction | Messages utilisateur conservés mot pour mot (journal 31/10, YA-05) | Résumés ; prompt 1 perdu (mesuré, §4.4) | Écart moyen à fort sur du code upstream. |
| Prompt système | < 2 000 tokens hors tools ; rien de volatil (YA-04, YA-09, CLAUDE.md règle 4) | 7 500 tokens est. ; date, cwd et chemin du journal de session dedans | Deux chantiers : réduction (YA-09) et déplacement du volatil (YA-04). |
| `rlm()` | Lecture seule, profondeur 1, résumé et extraits | Enfant complet avec tous les tools ; handle puis `collect` | YA-06 à construire par-dessus. |
| Crash du kernel | Observation « kernel redémarré, variables perdues » (journal 28/10) | Redémarrage silencieux (mesuré) | À ajouter (YA-03). |
| Plafond de sortie de cellule | 8 000 tokens, puis chemin de fichier | 1 Mio de texte | À ajuster (YA-03). |
| Empreinte | < 50 Mo hors venv | 62 Mo mesurés (binaire 59 Mo) | Cible à revoir, ou réduction du binaire (features, strip). |
| ACP | `session/request_permission` (YA-10) | Absente ; `loadSession: false` | À ajouter (YA-10). |

---

## Recommandations

**Ordre proposé pour la suite :**

1. **Étape 3 de l'ordre de démarrage (désactivations)**, avec deux ajouts :
   - télémétrie et vérification de version coupées par défaut ;
   - CI héritée réduite. Le shard 1 de `ci.yml` échoue aujourd'hui faute de release `v0.9.8` sur le fork (`ci.yml:355`) : soit publier cette release sur le fork, soit retirer l'étape de comparaison TS.
2. **YA-04 en trois commits.**
   - D'abord le test de préfixe, à partir de ce banc ou du provider `faux`.
   - Puis le retrait de la date, du cwd et du chemin du journal du prompt système.
   - Puis un réglage du TTL.
   - Le test doit ignorer le déplacement de `cache_control`.
3. **Environnement filtré du kernel (YA-03).** Petit correctif, risque sécurité élevé. À faire avant toute utilisation avec de vraies clés.
4. **YA-13 (base).** Journal d'usage via `on_payload` / `on_response`, ajout d'un champ raisonnement et correction de la lecture de l'usage au `message_start`.
5. **YA-05 masquage** par le hook `transform_context` (faible écart). Ensuite la conservation des messages utilisateur à la compaction (écart plus fort).
6. **YA-03** (`edit` en tool, `read` numéroté, plafond de sortie, notice de redémarrage), puis **YA-06**, puis **YA-10** (permissions ACP).

**Risques pour le cache :**

- Chaque nouvelle session réécrit environ 7 500 tokens est. de prompt système, plus le digest. C'est le premier poste à réduire (YA-09).
- La date dans le prompt système invalide le cache d'une session reprise le lendemain.
- La compaction réécrit les messages, ce qui est attendu. Elle ajoute aussi un appel `/refine` gate et un appel de résumé, à journaliser comme invalidations.
- Le masquage par paliers (YA-05) doit être déterministe et ne s'appliquer qu'aux frontières de palier.
- Les variables d'environnement (`PI_CACHE_RETENTION`) changent le corps de la requête. Il faut les figer dans le banc.

**Hard fork : quantité de code upstream à toucher pour YA-04 et YA-05.**

- YA-04 :
  - environ 25 lignes dans `system_prompt.rs` (section environnement) ;
  - le branchement de l'instantané dans `engine.rs`, quelques dizaines de lignes est. ;
  - une ligne dans `provider_adapter.rs` pour le TTL ;
  - le test de préfixe reste un ajout.
- YA-05 masquage : nouveau module, plus environ 5 lignes dans `engine.rs`.
- YA-05 conservation des messages utilisateur : touche `compact_session.rs` et `compaction.rs`, des fichiers qu'upstream modifie souvent (une vingtaine de branches upstream `*compaction*`).
- Estimation : quelques centaines de lignes upstream modifiées au total. Le reste tient dans des modules séparés.
- Avis :
  - la stratégie de hard fork avec cherry-pick sélectif reste tenable pour YA-04 et le masquage, à condition de garder le texte du prompt dans des fichiers à part ;
  - la conservation des messages utilisateur à la compaction est le point de friction principal ;
  - upstream a une branche `perf/prompt-cache-miss-minimization` et plusieurs branches sur la compaction, à examiner avant d'écrire ces modifications. Un correctif équivalent pourrait arriver par cherry-pick.

---

## Annexe : commandes

```bash
# Comptage des lignes Rust par crate
for c in crates/*; do printf "%s " $c; find $c -name '*.rs' | xargs cat | wc -l; done
```

```bash
# Binaire de HEAD (CI du fork)
gh run download 41601230885 --repo YusufByn/yusagent --dir <dossier>
```

```bash
# Mesures (une seule commande)
python3 bench/tools/capture/run.py --binary <dossier>/prime-agent
```

```bash
# Tests du runtime Python
cd prime-agent-runtime && UV_PROJECT_ENVIRONMENT=<venv jetable> uv run --python 3.11 python -m unittest discover -s test
```

```bash
# CI du fork et d'upstream
gh run view 41600446529 --repo YusufByn/yusagent
gh run list --repo PrimeIntellect-ai/prime-agent --workflow ci --branch main --limit 5
```

```bash
# Tailles
du -sh . .git
git count-objects -vH
du -sh <dossier du binaire> ~/.cache/yusagent-capture/home/.prime/agent/kernel-venv
```
