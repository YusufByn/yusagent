# Ticket 15 : YA-09, `isError` et « échouer bruyamment »

Demande de Yusuf du 18/11/2026. Branche `ya-15-erreurs-visibles`, depuis `main` à `ddbf1c91b`. Une PR, pas de merge sans son accord. Aucun appel à un vrai modèle ; aucune chauffe lancée.

## État (18/11/2026)

- PR #52 ouverte, relue par Yusuf (« bonne sur le fond »). CI : 14 checks requis verts sur `4fd1130c6`.
- Unharnessed relu au commit **`53bdd8784617fa9f2171abababd0b0f95618bfeb`** (17/11/2026), clone jetable hors du dépôt, rien exécuté ni compilé (§ B.2).
- Avant merge, demandé par Yusuf : définition d'« outil en échec » pour `python_repl` (§ A.3) ; chauffe `node-tough-cookie-630` lancée par Yusuf lui-même, commandes au § « Chauffe ».
- **Chauffe faite par Yusuf le 18/11/2026** : `check_ya15.py` sort en 3 (premier corps identique hors `prompt_cache_key`, aucun 1011, 17 requêtes en WebSocket, 0 cellule en erreur, tâche résolue). Résultat au § « Résultat de la chauffe ». Merge autorisé par Yusuf sous condition de CI verte sur le dernier commit.

## A. `isError`

### A.1 Upstream

**Pas corrigé upstream.** Remote `upstream` lu le 18/11/2026 (`upstream/main` à `61d4b4262`, 93 commits depuis notre base `c83ac68b3`). Le pont jette toujours le drapeau : `let _ = is_error;` (`git show upstream/main:crates/pa-core/src/session_engine/tool_bridge.rs`, lignes 112-115). Aucun commit de cet intervalle ne pose `is_error` dans le pont ni dans la boucle (`git log -S"is_error" c83ac68b3..upstream/main`).

Origine : le portage Rust (`39bc99a91`, #2524) a reproduit le TS. Le TS ignorait déjà le drapeau de l'outil : `packages/coding-agent/src/core/tools/ipython.ts:757` posait `isError` d'après `status`, et `packages/agent/src/agent-loop.ts:867` renvoyait `isError: false` pour tout outil qui ne levait pas (`git show 39bc99a91^:<chemin>`).

**Correctif** dans un commit séparé, noté dans `docs/fork-changes.md` comme candidat à une PR upstream. Forme reprise telle quelle d'Unharnessed, qui a tranché (`53bdd8784`, commit source `43f131ac6`, « Error flag: a raising cell's result has isError true ») :
- `AgentToolResult` reçoit `is_error`, en `#[serde(skip)]` (`crates/pa-agent/src/types.rs:489`). Les événements et le JSON des résultats ne changent pas de forme. `AgentToolResult::error()` le pose à `true` : tous ses appelants passaient déjà `is_error: true` à côté (`tool_call.rs:31`, `:48`, `:95`, `:311-313`, `:372`).
- La boucle le lit au lieu de `false` (`crates/pa-agent/src/agent_loop/tool_call.rs:197`).
- Le pont le transmet (`crates/pa-core/src/session_engine/tool_bridge.rs:113`, `:118`). Les mises à jour en cours d'exécution restent à `false` (`:106`).
- Test upstream touché : le littéral de `crates/pa-core/src/session_engine/telemetry/tests.rs:195` reçoit le champ (une ligne, sans changement d'assertion).

Qui passe à `isError: true` : une cellule `python_repl` en `status` `error` ou `aborted` (`crates/pa-core/src/tools/ipython.rs:511`, définition au § A.3), un `apply_patch` non appliqué (`tools/ya_apply_patch.rs:69`), un refus de `exec_command`/`write_stdin` (`tools/ya_exec.rs:195`, `:651`). Ces tools posaient le drapeau, que le pont jetait.

### A.3 « Outil en échec » pour `python_repl` : définition exacte

**Règle** : le résultat d'une cellule a `isError: true` si et seulement si son `details.status` vaut `error` ou `aborted` (`crates/pa-core/src/tools/ipython.rs:511`), **ou** si l'outil sort en erreur sans résultat de cellule (chemin `Err` de la boucle, `crates/pa-agent/src/agent_loop/tool_call.rs:201-222`, résultat construit par `error_tool_result` `:304-322` avec `{status: "error"|"aborted"}`, déjà `isError: true` avant ce ticket). Un `status` `ok` donne toujours `isError: false`, quoi que contienne la sortie.

Le `status` vient du kernel (`crates/pa-core/src/kernel/manager/`) :
- `error` : un événement `error` du runtime pour la cellule (`events.rs:70-86`), ou une fin de cellule `done` avec un statut autre que `ok` (`events.rs:113-123`), ou une image jointe trop grosse (`events.rs:196-209`).
- `aborted` : le signal d'abandon de la cellule a été déclenché (`execution.rs:186-192`), ou le kernel ne l'a pas terminée dans la fenêtre de grâce après l'interruption (`force_abort`, `execution.rs:42-52`, appelé par `requests.rs:345`), ou le signal était déjà déclenché avant l'envoi (`requests.rs:180-184`, `mod.rs:128-145`).

| Cas | `status` | `isError` (avant → après) | Preuve |
|---|---|---|---|
| Exception non rattrapée dans la cellule (`1/0`, `raise`, `AttributeError` d'un mauvais usage de B, refus d'`edit()`) | `error` | `false` → **`true`** | `events.rs:70-86` ; test `ya_tool_error_wire.rs` (`1/0`, `h.exit_code`, `await read`) |
| `SyntaxError` (la cellule ne compile pas) | `error` | `false` → **`true`** | même événement `error` (`prime-agent-runtime/src/rlm/repl.py:569-583`, `_error_event`) ; constaté : les 22 cellules `SyntaxError` de l'inventaire du § B.1 sont en `status` `error` ; comptées à part par `metrics.py` |
| Exception rattrapée par le code de la cellule (`try/except`) | `ok` | `false` → `false` | aucune erreur ne remonte au kernel |
| Cellule plus longue que le budget du watchdog (`cell_timeout_ms`) | `ok` si elle finit sans erreur | `false` → `false` | le watchdog n'arrête pas la cellule : il ajoute une note `[watchdog]` en stderr (`execution.rs:193-206`, `cell_watchdog_note` `:301`). Aucun délai dur n'est passé pour `python_repl` (`session_engine/runtime_wiring.rs:416-428` appelle `execute`, qui passe `execution_timeout_ms: None`, `kernel/manager/mod.rs:585-587`) |
| Interruption par l'utilisateur (Échap, abandon du tour) pendant la cellule | `aborted`, mais la boucle répond d'abord `Tool execution aborted` | `true` → `true` (inchangé) | `race_with_abort` rend l'erreur de la boucle (`tool_call.rs:304-322`, `{status: "aborted"}`) |
| `KeyboardInterrupt` dans la cellule (levé par le code, ou interruption du kernel sans abandon du tour) | `error` | `false` → **`true`** | événement `error` du runtime (`repl.py:569-583` ; `_interrupt_event` `:587-595` pour une cellule suspendue sur un `await`) |
| Mort du kernel pendant la cellule | sortie en erreur du gestionnaire (`Kernel has been shut down`, ou erreur de protocole), avec la notice `[python-kernel-restarted]` | `true` → `true` (inchangé) | `teardown.rs:182`, `repair.rs:28` (`reject_active_execution`) ; notice ajoutée au message d'erreur par `ya_kernel_restart.rs:97-102` |
| `await bash(cmd)` dont la commande sort avec un code non nul | `ok` | `false` → `false` | le code est dans `BashResult.exit_code` ; le détail `bashFailedExit` est posé (`events.rs:245`) mais ne change pas le `status`. Le masquage (YA-05) le compte comme échec à part (`masking.rs:316-317`), pas le drapeau |
| `bash(cmd)` arrêté par son `timeout` (SIGTERM puis SIGKILL) | `ok` | `false` → `false` | ligne `[yusagent: killed, ...]` dans la sortie et code de sortie non nul ; la cellule elle-même réussit (`ya_timeout.py`) |
| Commande `bash()` en arrière-plan qui échoue après la fin de la cellule | sans objet | sans objet | arrive comme avis `[bash-done]` (message custom), pas comme résultat d'outil |
| `print()` sur stderr, `RuntimeWarning` (coroutine jamais attendue) | `ok` | `false` → `false` | sortie seulement |

Donc, pour `python_repl` : **échec = exception non rattrapée (y compris `SyntaxError` et `KeyboardInterrupt`), abandon de la cellule, ou mort du kernel**. Ne sont **pas** des échecs : un délai de watchdog dépassé, un code de sortie non nul ou un timeout d'un `bash()`, une sortie sur stderr.

### A.2 Ce qui change sur le fil

Le prompt système et les tools ne changent pas : le correctif ne touche que la valeur d'un champ des résultats d'outil. `crates/pa-core/tests/prefix_stability.rs` n'est pas modifié (ses cellules ne lèvent pas : `print(1)`, `x = 2`, `THIRD_CELL`). Le nouveau test vérifie en plus que le prompt, les tools et les paramètres sont identiques d'une requête à l'autre avec des cellules en erreur.

| Provider (API) | Champ | Requêtes concernées |
|---|---|---|
| **Anthropic** (`anthropic-messages`) | `is_error` de chaque bloc `tool_result` (`crates/pa-ai/src/providers/anthropic/convert.rs:229`). Le champ était déjà envoyé, toujours à `false` ; il vaut maintenant `true` pour un outil en échec. | Toute requête dont l'historique contient le résultat d'un outil en échec, à partir de la requête qui suit cet outil. Le bloc est figé une fois écrit : le préfixe d'une session reste stable. |
| **OpenAI Responses** (`openai-responses`) | Aucun. `function_call_output` porte `type`, `call_id`, `output` (`crates/pa-ai/src/providers/openai_responses_shared.rs:357-361`) ; aucun champ d'erreur. | Aucune. Requêtes identiques octet pour octet. |
| **openai-codex** (`openai-codex-responses`, SSE et WebSocket) | Aucun : même convertisseur `convert_responses_messages` (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs:507`, `:640`). | Aucune : le corps et la suite WebSocket (`previous_response_id`) ne changent pas. Non testé de bout en bout (pas de serveur WebSocket factice dans ce test), déduit du code. |

Effets hors du fil des requêtes de tour, tous providers :
- **Compaction** : l'entrée envoyée au modèle de résumé étiquette un résultat en échec `[Tool result (python_repl, error) #n]` au lieu de `[Tool result (python_repl) #n]` (`crates/pa-core/src/session_engine/compaction_utils.rs:273`). Seulement dans les requêtes de compaction.
- **Session et événements** : `isError: true` dans le `.jsonl` de session et dans `tool_execution_end`. L'extension (ACP) reçoit le statut `failed` au lieu de `completed` pour un outil en échec (`crates/pa-daemon/src/acp/wire_events.rs:169-181`). Le TUI affiche la carte en erreur. La télémétrie (coupée par défaut) compte l'échec.
- **Masquage (YA-05)** : inchangé. Il lit `details.status` avant `isError` pour `python_repl` (`crates/pa-core/src/session_engine/masking.rs:316`).
- **Banc** : `bench/smoke/metrics.py:444-450` lit déjà `details.status` (ou `isError`) : les comptes ne bougent pas. Ligne du README mise à jour.
- Autres providers, non utilisés par le fork : Bedrock (`status`), Google, Mistral (préfixe `[tool error]`) lisent aussi le drapeau (`bedrock/convert.rs:306`, `google_shared.rs:345`, `mistral/convert.rs:276`).

## B. « Échouer bruyamment »

### B.1 Inventaire

Source : les cellules `python_repl` en erreur (`details.status == "error"`) des sessions de `bench/smoke/runs/` datées du 11/11/2026 ou après (128 cellules), et les sorties qui montrent un usage silencieux. Commande : `python3 -I bench/smoke/tools/helper_misuse.py bench/smoke/runs 20261111`. Configuration lue dans l'en-tête `yaRequest` de chaque session (`promptVersion`/`bashWaitSeconds`, champ absent = 1). Le défaut actuel est `v2/full`.

| Cas | Effet aujourd'hui | Occurrences (config) | Retenu |
|---|---|---|---|
| `h.exit_code` (ou `.duration`) sur un `BashHandle` sans `await` (cas connu) | `AttributeError: 'BashHandle' object has no attribute 'exit_code'`, sans dire que la commande peut tourner encore ni comment avoir le résultat | 1 (`v2/full`, `py-silence-002`, 17/11) | **oui** |
| `r.command` (`.pid`, `.tail`, `.poll`, `.kill`) sur un `BashResult`, souvent après `asyncio.gather(bash(...), ...)` | `AttributeError` sans dire que le résultat n'est pas le handle | 3 (`v2/30`, `v1/30`, `v1/1` ; `py-tldextract-368`, `node-testsprite-356`) | **oui** |
| `bash(cmd).output` ou `h.output` sans appel | **Résultat faux, silencieux** : la cellule réussit et imprime `<bound method BashHandle.output of <BashHandle pid=… running …>>` | 2 (`v2/full`, A/B du 17/11, `py-tldextract-*`) | **oui** |
| `await read(path)` | `TypeError: object str can't be used in 'await' expression`, sans dire que `read()` est synchrone (le prompt v2 écrit `await edit(...)` mais `read(...)`) | 3 (`v2/full` ×2, `v1/30` ; `node-tough-cookie-630`, `node-koa-2029`) | **oui** |
| `edit(path, old=..., new=...)` | `TypeError: run() got an unexpected keyword argument 'old'` : nomme `run()`, que le modèle ne connaît pas | 1 (`v2/full`, `py-tldextract-383`, 17/11) | **oui** |
| `.poll`, `.pid`, `.tail`, `.command` ou `await` sur un `BashSnapshot` | `AttributeError` / `TypeError` | 12 (`v1/1`, `v1/30`) | non : l'instantané n'existe qu'avec `bashWaitSeconds` numérique. Le défaut v2 est `"full"` (ticket 13), et le bras de contrôle `prompt-v1.json` garde l'ancien harness tel quel. |
| `.handle` sur un `BashResult` ou un `BashHandle` | `AttributeError` | 4 (`v1/1`) | non : vient de l'API de l'instantané (fenêtre numérique). |
| `.output()` sur un `str` (instantané ou résultat) | `TypeError: 'str' object is not callable` | 3 (`v1/1`) | non : observé seulement à l'époque des instantanés. Le corriger demande de changer le type de `BashResult.output`. |
| Sous-agents (`agent_observe`, `rlm.spawn` sans `await`, `RLMChildResult.name`) | `NameError`, coroutine imprimée | 7 (`v1/1`) | non : la v2 refuse les sous-agents (ticket 11). |
| `edit(...)` sans `await` | aucune modification, `RuntimeWarning: coroutine … was never awaited` dans la sortie | 0 | non : jamais observé, et l'avertissement est déjà visible. |

Hors helpers, non retenus : erreurs de syntaxe, fichiers absents, `print(...).output`, `await _`, refus d'`edit()` déjà explicites (ticket 11).

### B.2 Unharnessed

Clone jetable hors du dépôt (scratchpad de la session), lecture seule, rien exécuté ni compilé : `https://github.com/unharnessed-agent/Unharnessed` à **`53bdd8784617fa9f2171abababd0b0f95618bfeb`** (17/11/2026).

- **`isError`** : tranché (`43f131ac6`) et repris tel quel en A (`crates/uh-agent/src/types.rs:461-464`, `crates/uh-agent/src/agent_loop/tool_call.rs:208`, `crates/uh-core/src/session_engine/tool_bridge.rs:110`, `:117-122`).
- **Mauvais usages des helpers : rien de tranché.** `BashHandle` et `BashResult` n'ont ni `__getattr__` ni message propre (`unharnessed-runtime/src/rlm/bash.py:182-186`, `:236-412` ; aucun `__getattr__` dans le fichier). Leur prompt décrit la même API (`crates/uh-core/src/prompts/layers/core.md:7-9`). Ils n'ont ni `read()` ni `edit()` (skill retiré en `bd7521115`, ticket 11). Leur seul traitement des erreurs de cellule est le découpage du traceback aux frames de la cellule (`unharnessed-runtime/src/rlm/repl.py:501-509`), sans message d'aide.
- Rien à reprendre tel quel pour B : les messages ci-dessous sont propres au fork.

### B.3 Messages retenus

Tous passent par la sortie de l'outil (texte de l'exception dans le résultat de la cellule), avec `isError: true` grâce à A. **Aucun changement de prompt ni de tool.** Un usage valide ne change pas, et un attribut absent lève toujours `AttributeError`, donc `hasattr` et `getattr(x, nom, défaut)` ne changent pas non plus.

| Cas | Message | Code |
|---|---|---|
| `h.exit_code` / `h.duration` | `AttributeError: BashHandle has no .exit_code: the command may still run. Use r = await h, then r.exit_code (or h.poll(), None while running)` | `prime-agent-runtime/src/rlm/ya_hints.py:36` |
| `r.command` (`.pid`, `.tail`, `.poll`, `.kill`) | `AttributeError: BashResult (a finished command: .exit_code, .output, .duration) has no .command: .command is on the BashHandle that bash() returned` | `ya_hints.py:45` |
| `h.output` sans appel, utilisé comme texte (`print`, f-string, `str`, tranche, `in`, `len`, `bool`, `+`, méthodes de `str`) | `TypeError: BashHandle.output is a method: call h.output() for the output so far, or read (await h).output` (`AttributeError` pour une méthode de `str`) ; `repr` affiche le même texte. `h.output()` est inchangé. | `ya_hints.py:54-99` |
| `await read(path)` (et `asyncio.gather(read(...))`) | `TypeError: read() is not async: call read(path) without await` ; le texte reste un `str` partout ailleurs, et se sérialise en `str` simple (snapshot du kernel) | `prime-agent-runtime/src/rlm/read.py:19-29` |
| `edit(old=..., new=...)` (ou un argument manquant) | `TypeError: edit(path, old_str, new_str): got an unexpected keyword argument 'old'` ; signature publique de `run` inchangée (`functools.wraps`), CLI `!edit` (`tyro`) inchangée | `skills/edit/src/edit/__init__.py:87-99` |

Branchement : `bash.py` importe `ya_hints` et appelle `ya_hints.install(BashHandle, BashResult)` en fin de module (`prime-agent-runtime/src/rlm/bash.py:27`, `:1901`). `PersistentBashHandle` hérite des deux.

## Tests

Hors des fichiers upstream :
- `prime-agent-runtime/test/test_ya_hints.py` (11 tests) : chaque message, usages valides inchangés, message de l'interpréteur pour les autres attributs absents, `pickle`/`dill`/`copy` de `BashResult` et du texte de `read()`, CLI `tyro` d'`edit`. Commande : `cd prime-agent-runtime && uv run --python 3.11 python -m unittest discover -s test`, **812 tests OK** en local le 18/11/2026 (Python seulement, règle 8).
- `crates/pa-core/tests/ya_tool_error_wire.rs` : session réelle, vrai kernel, serveur HTTP local pour Anthropic Messages et OpenAI Responses. Quatre cellules : `1/0`, `print('ok')`, `h.exit_code` sans `await`, `await read(...)`. Vérifie `is_error` `[true, false, true, true]` côté Anthropic ; côté Responses, des items `function_call_output` à trois clés exactement ; dans les deux cas, les messages de B dans les sorties, et un prompt, des tools et des paramètres identiques entre requêtes. Rust validé par la CI seulement (règle 8).

Fichiers upstream touchés par les tests : `crates/pa-core/src/session_engine/telemetry/tests.rs` (une ligne, imposée par le nouveau champ).

## Chauffe

Ni A ni B n'ont besoin d'une chauffe pour être validés : A ne change rien sur le fil Codex (seul provider réel du banc), le côté Anthropic est couvert par le test de fil, et B ne se déclenche que sur un mauvais usage. **Garde-fou de non-régression demandé par Yusuf le 18/11/2026 : une chauffe `node-tough-cookie-630`**, la tâche où `await read()` revient le plus, **lancée par Yusuf dans un Terminal à part**. Rien n'a été lancé dans cette session.

Binaires (artefacts de `ya-build-macos-arm64`) :
- **branche** : run `44280651267`, commit `4fd1130c69468b185dc9e22a3499a17a58335d00`. Les commits suivants de la branche ne touchent que la doc et `bench/` : le binaire a les sources du dernier commit ;
- **`main`** : run `44086728262`, commit `bf88ebc4a5a2c586c4cc3756aeeb64216a49cfe8`. Entre ce commit et `main` (`ddbf1c91b`), seuls `CLAUDE.md`, `docs/` et `bench/` changent (`git diff --name-only bf88ebc4a ddbf1c91b`).

La référence `main` est un run en mode capture (`--capture`, serveur local, **aucun appel au modèle**). Le premier corps part avant toute réponse du modèle, il ne dépend donc pas du serveur. Les deux runs se lancent **le même jour UTC** : le bloc `<environment>` de l'`input` porte la date.

Commandes, depuis la racine du dépôt, sur la branche à jour :

```bash
cd ~/Desktop/yusagent && git switch ya-15-erreurs-visibles && git pull --ff-only
BR=$(bash bench/smoke/runs/install-bin.sh 44280651267 4fd1130c69468b185dc9e22a3499a17a58335d00)
MAIN=$(bash bench/smoke/runs/install-bin.sh 44086728262 bf88ebc4a5a2c586c4cc3756aeeb64216a49cfe8)

# 1. Référence main, mode capture (aucun appel au modèle)
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 -I bench/smoke/run.py \
  --binary "$MAIN/prime-agent" --model gpt-6-luna --effort high \
  --settings bench/smoke/settings/default.json --tasks node-tough-cookie-630 --runs 1 \
  --capture --log-bodies

# 2. Chauffe réelle, binaire de la branche (appel au modèle)
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short caffeinate -i python3 -I bench/smoke/run.py \
  --binary "$BR/prime-agent" --model gpt-6-luna --effort high \
  --settings bench/smoke/settings/default.json --tasks node-tough-cookie-630 --runs 1 \
  --log-bodies
```

Vérification (lecture seule) :

```bash
REF=$(ls -d bench/smoke/runs/*-bf88ebc-gpt-6-luna | tail -1)/tasks/node-tough-cookie-630/run-1
NEW=$(ls -d bench/smoke/runs/*-4fd1130-gpt-6-luna | tail -1)/tasks/node-tough-cookie-630/run-1
python3 -I bench/smoke/tools/check_ya15.py "$REF" "$NEW"; echo "code de sortie : $?"
```

`bench/smoke/tools/check_ya15.py` contrôle trois choses :
1. Le premier corps : toutes les clés sauf `prompt_cache_key` doivent être identiques en JSON trié. Pour `input`, le script montre le premier écart.
2. Dans la session de la branche, `isError` de chaque résultat `python_repl` doit valoir `true` exactement quand `details.status` vaut `error` ou `aborted` (§ A.3).
3. Il ne doit y avoir aucune mention de `1011` dans `out.jsonl` ni dans `err.log` (même règle que `check_warmup.py`). Le script affiche aussi les transports au proxy.

Codes de sortie : `0` si tout passe ; `1` si un contrôle échoue ; `3` si 1 et 3 passent mais que le run n'a aucune cellule en erreur (critère 2 non vérifiable sur ce run, A reste couvert par `ya_tool_error_wire.rs`).

Script essayé sur les journaux existants (sans nouveau run) :
- Il signale la fermeture en 1011 connue de `20261116T080936Z-1c9ecf4` (`node-testsprite-356`).
- Il signale l'`isError: false` d'une cellule en erreur sous un binaire d'avant ce ticket (`20261117T075921Z-713ba4b`, `py-silence-002`).
- Il signale l'`input` différent de deux tâches différentes.

Si une clé autre que `input` diffère entre la capture et le run réel, cet écart viendrait du mode capture et non de la branche. Il faudrait alors une seconde chauffe réelle sur le binaire de `main`, à décider par Yusuf.

## Résultat de la chauffe (18/11/2026)

Lancée par Yusuf avec les commandes ci-dessus. Contrôle relu dans cette session, en lecture seule :

```bash
python3 -I bench/smoke/tools/check_ya15.py \
  bench/smoke/runs/20261118T104144Z-bf88ebc-gpt-6-luna/tasks/node-tough-cookie-630/run-1 \
  bench/smoke/runs/20261118T104154Z-4fd1130-gpt-6-luna/tasks/node-tough-cookie-630/run-1
```

- **Code de sortie 3.**
- **Premier corps** : identique à la référence `main` (mode capture, `bf88ebc`) pour toutes les clés hors `prompt_cache_key` : `include`, `input`, `instructions`, `model`, `parallel_tool_calls`, `reasoning`, `store`, `stream`, `text`, `tool_choice`, `tools`, `type`.
- **1011** : aucune mention dans `out.jsonl` ni `err.log`. Les 17 requêtes au proxy sont parties en WebSocket.
- **`isError`** : non vérifiable sur ce run. Les 17 cellules `python_repl` de la session sont en `status` `ok`, et toutes ont `isError: false`. Le drapeau d'une cellule en erreur reste couvert par `crates/pa-core/tests/ya_tool_error_wire.rs` (CI).
- **Tâche** : `resolved` (`report.json`, `results`).
- Runs dans `bench/smoke/runs/` (non versionnés).

## Points ouverts

- **PR upstream** : le commit A est candidat (noté dans `docs/fork-changes.md`). Un test côté upstream serait à écrire dans leurs fichiers.
- **Bras de contrôle v1** (`prompt-v1.json`) : il reçoit aussi `isError: true` et les messages de B, qui ne changent que les sorties d'un mauvais usage. Les cas propres à l'instantané (`BashSnapshot`) ne sont pas touchés.
- **Comparaisons du banc d'avant en après** : pour Codex, aucune requête ne change hors texte des erreurs de B ; pour Anthropic, l'historique porte `is_error: true` (non mesuré au banc).
