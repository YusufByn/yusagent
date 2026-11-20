# Ticket 20 : YA-11, première partie : permissions (phase 1 : constat)

Demande de Yusuf du 20/11/2026. **Phase 1 : constat seulement.** Branche `ya-20-permissions-constat`, depuis `main` à `dd3ef7381`. Aucun code du produit modifié, rien compilé, aucun appel de modèle, aucun run du banc. La PR contient ce fichier, le script de comptage de la section 3 (`docs/tickets/20-ya11-permissions/compte-confirmations.py`, lecture seule des traces locales) et une ligne du journal du CDC : **préfixe inchangé**, aucun fichier upstream touché. Pas de merge sans « merge » de Yusuf.

Chaque section sépare ce qui est **constaté** dans le code (avec `chemin:ligne`) de ce qui est **proposé**. Les choix de Yusuf sur le constat sont dans la section suivante ; ils remplacent les recommandations de la section 6 quand elles diffèrent.

## Décisions de Yusuf sur le constat (20/11/2026)

- **D1, réglage** : validé. Réglage `permissions` lu **uniquement dans les réglages globaux** (jamais dans `<cwd>/.yusagent/`), **pas dans `yaRequest`**. Changement par session via `session/set_config_option`. Décisions journalisées.
- **D2, mode manuel (révisé)** : **option (a)**. En mode manuel, **chaque appel de tool demande une confirmation, sans classement** : cellule `python_repl`, `apply_patch` (et `exec_command`/`write_stdin` dans la variante bash natif). Le classement (b) est **reporté** : à rejuger après le sandbox, sur des traces de testeurs, dans un ticket à part.
  - Justification (section 3.4) : avec la définition stricte qu'aurait demandée (b) (liste blanche sur l'AST de la cellule, refus par défaut, sans opérateur shell), le prompt v2 passe de **13** à **10** confirmations par session en médiane : gain trop faible pour le coût (classifieur, process d'analyse de l'AST, tests).
  - **Pas de process d'analyse de l'AST.**
  - Première version de D2 (même jour), remplacée : (b) + « toujours pour cette commande », définition stricte de la section 3.4. Gardée comme trace du recompte.
- **D3, « toujours pour cette commande »** : **sans objet, retiré du ticket 20.** Le passage de la session en « auto » par `session/set_config_option` (D1) couvre le besoin.
- **D4, avertissement persistant quand « auto » est actif sans sandbox** : oui.
- **D5, testeurs en manuel au départ** : oui.
- **D6, manuel sans client capable de répondre** (`-p`, RPC) : erreur au démarrage, avec un message qui donne **la ligne exacte à mettre dans les réglages globaux** (`"permissions": "auto"`) et le chemin du fichier (`~/.yusagent/settings.json`, ou le dossier fixé par `PRIME_AGENT_CODING_AGENT_DIR`).
- **D7** : le tour s'arrête après un refus.

## Décisions déjà prises par Yusuf avant le constat (20/11/2026)

- Permissions et `session/request_permission` dans le cœur, **avant l'extension** (YA-10). Sandbox du kernel plus tard (seconde partie de YA-11). Conforme au journal du 11/11/2026 (`docs/yusAgent-cdc.md:40`).
- **Manuel par défaut.** Un réglage permet de passer en « auto », **possible aussi sans sandbox**, jamais par défaut. **Écart** au verrou du 28/10/2026 (`docs/yusAgent-cdc.md:12`, `:399` : « hors sandbox ou conteneur, l'auto-approve des cellules du kernel est impossible ») et au critère de YA-11 « Auto-approve des cellules refusé hors sandbox ou conteneur, testé » (`:411`). La ligne du journal du CDC est écrite dans cette PR.
- Unharnessed a retiré les permissions : rien à reprendre de chez eux (`docs/yusAgent-cdc.md:68`).

## Résultats en bref

- **Aucun mécanisme d'approbation n'existe aujourd'hui**, dans aucun mode (TUI, ACP, démon, RPC, `-p`). Le README d'upstream le dit : les process worker et kernel « are **not** a security sandbox » (`README.md:111`).
- **Le point d'interception existe déjà** : le hook `before_tool_call` de la boucle d'agent (`crates/pa-agent/src/agent_loop/tool_call.rs:76-98`) voit les arguments complets d'un appel de tool avant son exécution et peut le bloquer avec un texte. Il ne sert qu'aux questions annexes (`crates/pa-core/src/session_engine/side_question.rs:177-194`) ; l'agent principal ne le pose pas (`crates/pa-core/src/session_engine/engine.rs:893-944`).
- **`bash()` et `edit()` ne passent pas par l'hôte avant d'agir** : `bash()` lance son `Popen` directement dans le kernel (`prime-agent-runtime/src/rlm/bash.py:461-469`), `edit()` écrit le fichier puis prévient l'hôte après coup (`skills/edit/src/edit/__init__.py:80-82`). Les intercepter un par un demande un aller-retour kernel → hôte qui n'existe pas.
- **ACP ne sait pas recevoir de réponse du client** : le pont JSON-RPC rejette toute trame sans `method` (`crates/pa-daemon/src/acp/jsonrpc.rs:57-60`). `session/request_permission` (requête agent → client) demande d'abord ce sens de circulation.
- **Comptage heuristique** sur 556 sessions locales du smoke bench (section 3). En prompt v2 (136 sessions, 1 935 cellules), confirmations par session, en médiane : (a) chaque cellule **13** ; (b) cellules à effet de bord seulement **5** (40 % des cellules) ; (b) avec « toujours pour cette commande » **3** ; (c) chaque `bash()` et chaque écriture **13** ; (c) avec les commandes en lecture seule auto-approuvées **7**.
- **Recommandation** (section 6) : option (b) avec « toujours pour cette commande » à la portée de la session, interception à la cellule entière par `before_tool_call` ; réglage global seulement (jamais depuis le dossier de projet), hors de `yaRequest` ; `-p` et RPC en manuel : erreur au démarrage ; banc en « auto » écrit par `bench/smoke/run.py`.
- **Recompte avec la définition stricte** (section 3.4), prompt v2 : **10** confirmations par session en médiane (p90 20), **18,3 %** des cellules sans question ; avec « toujours » (commande exacte) : **10** (p90 20), 18,6 %. La définition large du constat donnait 5 et 60,1 %. Première cause : un opérateur shell dans un `bash()` (693 cellules, seule raison pour 569).
- **Décision** (D2 révisé) : option (a), chaque appel de tool confirmé en manuel (13 par session en médiane en v2, p90 23), sans classement ni « toujours » ; « auto » pour la session par `session/set_config_option`.

## Méthode

Lecture du code à `dd3ef7381`, aucune exécution du binaire. Recherches :

```sh
# Mécanisme d'approbation existant (hors fixtures)
git grep -n -i -E "approv|request_permission|permission_request|ask_user|approval|auto_approve|autoApprove|yolo|skip_permissions" -- crates prime-agent-runtime ':!*/testdata/*'
git grep -n -i -E "approv|request_permission|permission_request|confirm" -- crates/pa-daemon/src crates/pa-types/src/daemon
git grep -n "before_tool_call" -- crates
# Chemins du kernel vers l'hôte
git grep -n "host_request" -- prime-agent-runtime/src
```

Résultat : les seuls `approv*` concernent la revue de `/refine` (`crates/pa-core/src/session_engine/refine.rs:67`), le gate autonome et l'approbation des points d'accès MCP (`crates/pa-core/src/mcp/connection_store.rs:2`) ; aucun ne demande l'accord de l'utilisateur avant d'exécuter un tool. Le schéma ACP a été relu sur `agentclientprotocol.com` (pages `protocol/tool-calls`, `protocol/session-modes`, `protocol/schema`, lecture partielle de la dernière), le 20/11/2026.

## 1. Existant

### 1.1 Mécanismes d'approbation (constaté)

| Mode | Ce qui existe | Où |
|---|---|---|
| Boucle d'agent (`pa-agent`) | Hook `before_tool_call` : appelé **après** validation des arguments et **avant** l'exécution ; `block: true` remplace l'exécution par un résultat d'erreur portant `reason` (défaut « Tool execution was blocked »). Attente du hook en course avec l'annulation (`race_with_abort`). | `crates/pa-agent/src/agent_loop/tool_call.rs:76-98`, type `crates/pa-agent/src/types.rs:631-637`, option `crates/pa-agent/src/agent.rs:96` |
| Session principale (`pa-core`) | Le hook n'est **pas posé** : `Agent::new` sans `before_tool_call`, le reste par `..Default::default()`. | `crates/pa-core/src/session_engine/engine.rs:893-944` |
| Questions annexes | Seul usage : bloque tous les tools. | `crates/pa-core/src/session_engine/side_question.rs:177-194` |
| `-p` (`--mode text|json`) | Moteur dans le process, sans démon (`docs/audit/audit-entree.md:203`). Aucune approbation. C'est le mode du banc (`bench/smoke/run.py:385-387`, `bench/tools/capture/run.py:224`). | |
| RPC (`--mode rpc`) | Moteur dans le process (`crates/pa-daemon/src/rpc/mod.rs:1-4`). Commandes acceptées : `prompt`, `steer`, `abort`, modèles, compaction… (`crates/pa-daemon/src/rpc/commands.rs:213-237`) ; aucune commande de réponse à une demande. | |
| Démon et TUI | Aucun message démon → client qui attende une réponse ; aucune boîte de confirmation dans le TUI (recherches de la méthode). | |
| ACP | `session/request_permission` absente (audit du 03/11 : `docs/audit/audit-entree.md:216`, `:386` « Aucune requête agent → client »). Le pont ne lit que des requêtes et notifications du client ; une réponse du client est rejetée comme « Invalid Request ». | `crates/pa-daemon/src/acp/jsonrpc.rs:57-60` |
| `exec_command` (variante bash natif) | Paramètres de sandbox et d'approbation de Codex retirés (« no sandbox before YA-11 »). | `crates/pa-core/src/tools/ya_exec.rs:10-11` |
| Garde-fous d'écriture | `apply_patch` et `edit()` refusent un chemin hors du workspace ; « a guard against mistakes, not a security boundary (`bash()` and `edit()` write anywhere until YA-11) ». | `crates/pa-core/src/tools/ya_workspace.rs:1-13`, `skills/edit/src/edit/__init__.py:52-55` |

Le **gate autonome** (`crates/pa-cli/src/headless_autonomous.rs:1-5`) n'est pas une approbation : il lance des commandes de vérification après chaque tour.

### 1.2 Ce que dit ACP (constaté sur la spec)

- `session/request_permission` est une requête **agent → client**, que l'agent « MAY » envoyer avant d'exécuter un tool. Paramètres : `sessionId`, `toolCall` (un `ToolCallUpdate`), `options` (liste de `{optionId, name, kind}`). `kind` ∈ `allow_once`, `allow_always`, `reject_once`, `reject_always` ; c'est un indice d'affichage, « the option `name` describes the scope ». **L'agent garde les décisions « always » et en fixe la portée.**
- Réponse : `{"outcome": {"outcome": "selected", "optionId": …}}` ou `{"outcome": {"outcome": "cancelled"}}`. Une issue inconnue ne doit pas valoir accord.
- Après `session/cancel`, le client **MUST** répondre `cancelled` à toute demande en attente.
- Aucune capacité client ne la conditionne dans `initialize` (contrairement à `fs/*`) : tout client ACP doit savoir y répondre.
- Statut d'un tool call : `pending` = « hasn't started running yet because the input is either streaming or awaiting approval ».
- Modes : `session/set_mode` existe, mais la spec annonce son retrait au profit des options de configuration (`session/set_config_option`). Sécurité : l'agent « SHOULD NOT switch to a mode or configuration that reduces user oversight without user action ».
- Déjà dans le fork : `session/set_config_option` avec deux sélecteurs, `model` et `thought_level` (`crates/pa-daemon/src/acp/config_options.rs:1-9`).

### 1.3 Ce que l'hôte voit d'une cellule avant de l'exécuter (constaté)

- **Le texte complet du code**, et rien d'autre. L'événement `ToolExecutionStart` part **avant** le hook (`crates/pa-agent/src/agent_loop/tools.rs:97-110` en séquentiel, `:172-182` en parallèle). Le pont ACP en fait un `tool_call` titré « Python cell », `rawInput: {"code": …}`, avec le statut **`in_progress`** (`crates/pa-daemon/src/acp/wire_events.rs:130-161`, `:156`) : le client voit la cellule avant son exécution, mais marquée comme déjà en cours.
- L'hôte ne voit **pas** ce que la cellule fera : les `bash()` dont la commande est construite (f-string, variable : 31 des 1 242 appels en v2, section 3), le Python arbitraire (`subprocess`, `os`, `shutil`, réseau), les fonctions MCP exposées dans le kernel.
- Pendant l'exécution, le kernel envoie des événements après coup : `display` du diff d'`edit()` **après** l'écriture (`skills/edit/src/edit/__init__.py:118-136`), activité `bash()` (`prime-agent-runtime/src/rlm/bash.py:839-841`), demandes `host_request` de fin de commande (`bash.completed`, `bash.py:866-875` ; `bash.consumed`, `:918-924`).

## 2. Points d'interception

Rappel du CDC : « le Python arbitraire du kernel (`subprocess`, `os`, `shutil`, `socket`, requêtes HTTP) n'est pas filtrable de façon fiable. Sans sandbox, il n'est couvert que par l'avertissement persistant ; une détection heuristique […] reste contournable et doit être présentée comme telle » (`docs/yusAgent-cdc.md:397`).

| Point | Constaté | Voit | Rate | Coût (proposé, estimation) |
|---|---|---|---|---|
| **Cellule `python_repl` entière** | Hook `before_tool_call` existant, appelé avant toute exécution (`tool_call.rs:76-98`). | Le code entier. Rien ne s'exécute avant la décision : la confirmation couvre aussi le Python arbitraire, **au sens où l'utilisateur lit le code** qu'il autorise. | Ce que le code fera vraiment (commandes construites, Python indirect) : la **classification** d'une cellule est heuristique ; un refus ou un accord, lui, est fiable. | Faible dans le cœur : un module du fork et une ligne dans `engine.rs` (fichier upstream). |
| **Chaque `bash()`** | `bash()` est **synchrone** (`bash.py:1323` : « Start a shell command immediately ») et lance `subprocess.Popen` dans le kernel (`bash.py:461-469`). Le seul chemin kernel → hôte, `host_request`, est asynchrone (`prime-agent-runtime/src/rlm/repl.md:153-158`) et ne sert qu'à la fin d'une commande. | La commande exacte, y compris construite à l'exécution. | Tout le Python qui ne passe pas par `bash()` (`subprocess`, `os.system`). | Élevé : demande `host_request` avant `Popen` dans un fichier runtime upstream, attente synchrone dans `bash()` (ou démarrage différé au premier `await`, ce qui change la sémantique des handles), une demande par branche d'un `asyncio.gather`, cas `persistent=True`, délais d'attente du kernel. |
| **`edit()` et écritures** | `edit()` est un skill Python qui écrit directement (`skills/edit/src/edit/__init__.py:59-60`, `:80`) puis émet le diff (`:118`). | Chemin et contenu exacts. | `open(…, "w")`, `Path.write_text`, `shutil`, écritures des commandes shell. | Moyen : `host_request` avant l'écriture dans un fichier du fork ; les autres écritures Python restent hors de portée. |
| **`apply_patch` natif** | Tool de premier niveau (`crates/pa-core/src/tools/ya_apply_patch.rs`), seulement avec `editTool: "native"` sur la famille Codex ; plus le défaut depuis le 16/11 (`docs/yusAgent-cdc.md:73`). | Le patch complet, par le même hook. | Rien de ce qu'il fait (il ne fait qu'écrire). | Nul en plus : couvert par le hook de cellule. Idem `exec_command`/`write_stdin` (variante bash natif). |

Conclusion du constat : **seule l'interception à la cellule (ou au tool call) existe et précède toute exécution.** Les interceptions plus fines demandent un nouveau chemin kernel → hôte et ne couvrent toujours pas le Python arbitraire ; seul le sandbox le fait (seconde partie de YA-11).

## 3. Options du mode manuel et nombre de confirmations

### 3.1 Comptage (estimation heuristique, sans run)

Commande (depuis la racine du dépôt ; runs locaux non versionnés) :

```sh
python3 -I docs/tickets/20-ya11-permissions/compte-confirmations.py
```

Le script calcule la définition large du constat (sections 3.1 à 3.3) et la définition stricte de D2 (section 3.4).

Périmètre : sessions principales des runs **réels** du smoke bench (`bench/smoke/runs/*/tasks/*/run-*/sessions/*.jsonl` et `bench/smoke/runs/*/arms/*/tasks/*/run-*/sessions/*.jsonl`, `report.json` en mode `réel`) ; hors capture, chauffe, sonde et sous-agents. 556 sessions : 420 en prompt v1 (7 602 appels de tools, dont 661 `apply_patch`, 115 `exec_command`, 103 `write_stdin`), 136 en prompt v2 (1 935 cellules, aucun autre tool). Le v2 est la configuration par défaut actuelle.

Règles du script (heuristiques, à lire comme telles) :

- **`bash()`** : chaque `bash(` du texte de la cellule ; commande lue si le premier argument est un littéral non `f`, sinon « dynamique ». Un `bash()` dans une boucle compte une fois : **borne basse** pour (c).
- **Commande en lecture seule** : découpage `shlex` (guillemets respectés) sur `;`, `&&`, `||`, `|`, `&` ; aucune redirection vers un fichier autre que `/dev/null` ; chaque commande simple dans une liste fermée (`ls`, `cat`, `head`, `tail`, `wc`, `grep`, `rg`, `find` sans `-exec`/`-delete`, `sed` avec `-n` sans `-i`, `awk`, `sort`, `diff`, `jq`…, `git status|diff|log|show|grep|ls-files|rev-parse|blame|describe`, `--version`). Les tests, builds, linters et formateurs (`npm test`, `uv run …`, `pytest`, `ruff format`…) ne sont **pas** en lecture seule : ils exécutent le code du projet.
- **Écriture Python** : `open(…, 'w'|'a'|'x'|'+')`, `write_text`, `write_bytes`, `os.remove|rename|replace|makedirs|system|popen…`, `unlink`, `mkdir`, `shutil.`, `subprocess`, `socket`, `urllib`, `requests.`, `http.client`.
- **Cellule en lecture seule** : aucun `edit(`, aucune écriture Python, aucun `bash()` dynamique, tous ses `bash()` en lecture seule. Les autres tools (`apply_patch`, `exec_command`, `write_stdin`) demandent toujours une confirmation.
- **« Toujours pour cette commande »** : clé = deux premiers mots de la commande (trois pour `npx`, `uvx`, `uv run`, `npm run`, `… -m`), plus `edit()` et le nom des autres tools ; mémorisée pour la session. Un `bash()` dynamique et une écriture Python ne sont jamais mémorisés.

Contrôle à la main sur des tirages de 12 à 15 commandes et cellules v2 : une première version qui coupait sur `|` sans respecter les guillemets classait `grep 'a\|b'` comme écriture ; corrigé par `shlex`. 4 cellules v2 classées en lecture seule sur 1 163 contiennent `__import__` ou `.write(` : toutes sont des `__import__('asyncio').gather(...)` sur des `bash()` en lecture seule.

### 3.2 Résultats

Confirmations par session (médiane, p90, max) et total :

| Option | v2 (136 sessions) | v1 (420 sessions) |
|---|---|---|
| (a) chaque cellule (chaque tool call) | **13** (p90 23, max 33) ; 1 935 | 16 (p90 30, max 69) ; 7 602 |
| (b) cellules à effet de bord | **5** (p90 11, max 22) ; 772, soit 40 % des cellules | 6 (p90 11, max 26) ; 2 701 |
| (b) + « toujours pour cette commande » | **3** (p90 6, max 11) ; 451 | 3 (p90 6, max 9) ; 1 471 |
| (c) chaque `bash()` et chaque écriture | **13** (p90 27, max 54) ; 2 061 | 10 (p90 19, max 38) ; 4 723 |
| (c) avec `bash()` en lecture seule auto-approuvés | 7 (p90 18, max 49) ; 1 286 | 7 (p90 16, max 37) ; 3 583 |
| (c) auto-approuvés + « toujours » | 4 (p90 6, max 13) ; 537 | 3 (p90 7, max 11) ; 1 625 |

Détail v2 : 1 242 appels `bash()` (775 en lecture seule, 31 dynamiques), 762 `edit()`, 57 motifs d'écriture Python dans 28 cellules ; 1 163 cellules en lecture seule sur 1 935 (60,1 %). Commandes hors lecture seule les plus fréquentes : `npm test` 82, `uv run …` 77, `.venv/bin/python -m …` 54, `npm run …` 52, `ruff format|check` 32, `npx vitest` 20, `node --test` 14.

Limites : 10 tâches courtes du smoke bench (`docs/yusAgent-cdc.md:32`), un ou deux modèles ; un humain devant l'écran ne travaillerait pas comme un modèle en `-p`. Les sessions longues de Yusuf auraient plus de cellules ; « toujours » y gagnerait plus.

### 3.3 Comparaison (proposé)

| Option | Fiabilité | Fatigue d'approbation | Coût de construction |
|---|---|---|---|
| **(a) chaque cellule** | La plus forte : rien ne s'exécute sans accord ; l'utilisateur lit tout le code. | Forte : 13 par session en médiane, dont 60 % de cellules de lecture. Le CDC rappelle 93 % d'approbations quand tout est demandé (`docs/yusAgent-cdc.md:139`) : l'accord devient réflexe. | Faible (hook + client). |
| **(b) lecture seule auto, le reste confirmé** | Accord ou refus fiables ; **la classification ne l'est pas** : une cellule classée en lecture seule à tort s'exécute sans question (Python indirect, `sed -n '1e cmd'` en GNU, `awk 'BEGIN{system(…)}'`, `rg --pre`, alias). Contournable, à présenter comme tel (`docs/yusAgent-cdc.md:397`). Le classifieur doit être une liste blanche conservatrice : tout ce qu'il ne reconnaît pas demande. | Moyenne : 5 ; 3 avec « toujours ». | Moyen : classifieur du code de cellule côté Rust (module du fork) et ses tests ; c'est ce que décrit déjà le verrou du 28/10 (« chaque cellule à effet de bord […] demande confirmation », `:399`). |
| **(c) chaque `bash()` et chaque écriture** | Plus fine pour les commandes **construites**, mais **ne couvre pas** le Python qui écrit ou lance des process sans `bash()`/`edit()` : un trou plus grave que (b), car rien ne demande. | 13 brut ; 7 avec lecture seule auto ; 4 avec « toujours ». | Élevé : aller-retour kernel → hôte dans `bash()` (synchrone) et `edit()`, fichiers runtime upstream, tests du kernel ; plusieurs demandes pour une même cellule (`gather`). |

### 3.4 Recompte avec la définition stricte de D2 (première version de D2, 20/11/2026)

**Justification de la décision finale (D2 révisé).** Cette définition a été retenue puis remplacée le même jour par l'option (a) : en v2, 10 confirmations par session en médiane contre 13 pour (a), gain trop faible pour le coût. La liste blanche ci-dessous n'est **pas** à construire dans le ticket 20 ; elle servira de point de départ au ticket qui rejugera (b) après le sandbox, sur des traces de testeurs.

Même commande, même périmètre (section 3.1) ; le script calcule les deux définitions. Compteurs `s_confirm` (b), `s2_confirm` (b + « toujours »), `cells_strict_ok`, `cells_strict_ok_always`, `cells_strict_ops_ok` et la ligne « raisons D2 strict ».

#### Liste blanche proposée (refus par défaut)

Une cellule passe sans question si **tout** son AST est dans la liste ; sinon, confirmation. Une cellule illisible (erreur de syntaxe) demande aussi.

- **Instructions** : expression, affectation (`=`, annotée, augmentée). Rien d'autre : pas d'`import`, de boucle, de `if`, de `with`, de `def`, de compréhension, d'`assert`.
- **Expressions** : noms (sauf ceux qui commencent par `__`), constantes, f-strings, tuples, listes, dictionnaires, ensembles, indices et tranches, opérateurs arithmétiques, de comparaison et booléens, `await`.
- **Appels** : seulement `read(…)`, `grep(…)`, `glob(…)`, `print(…)` (sans `file=`) et `bash(…)`, appelés par leur nom. Aucun appel de méthode ni d'attribut (`os.getcwd()`, `Path(…).read_text()`, `asyncio.gather(…)` demandent).
- **Attributs** : `.output` et `.exit_code` seulement (lecture du résultat d'un `bash()`).
- **`bash()`** : premier argument littéral (ni f-string, ni variable, ni concaténation), seul argument mot-clé `timeout=`. La commande est une **lecture sûre** si :
  - elle ne contient ni `;`, `&`, `|`, `$(`, backtick, `>`, `<`, ni saut de ligne (vérifié sur le texte, même entre guillemets : refus par défaut) ;
  - elle se découpe (`shlex`) en une seule commande, sans affectation de variable en tête ;
  - le programme est dans la liste : `ls`, `cat`, `head`, `tail`, `wc`, `grep`, `rg`, `find`, `sed`, `git`, `pwd`, `tree`, `stat`, `file`, `diff` ;
  - aucune option qui exécute ou écrit : `rg --pre`, `--pre-glob`, `-z`/`--search-zip` ; `find -exec`, `-execdir`, `-ok`, `-okdir`, `-delete`, `-fprint`, `-fprint0`, `-fprintf`, `-fls` ; `tree -o` ; `file -C` ;
  - `git` : sous-commande `status`, `diff`, `log`, `show`, `ls-files`, `rev-parse`, `blame` ou `grep` ; avant elle, seulement `--no-pager` ; jamais `-c`, `--config-env`, `--exec-path`, `--output`, `--ext-diff`, `-O`/`--open-files-in-pager` ;
  - `sed` : exactement `-n`, aucune autre option (donc ni `-i` ni `-f`), script d'impression d'une ligne ou d'un intervalle seulement (`12p`, `1,120p`, `$p`). `sed -n '1e cmd'` est refusé.
- **« Toujours »** (première version de D3, retirée) : la commande exacte d'un `bash()` littéral, sans opérateur. Une cellule passe ensuite sans question si sa structure est dans la liste et si chacune de ses commandes hors liste a déjà reçu « toujours » dans la session. Le comptage suppose que l'utilisateur choisit « toujours » à chaque confirmation, pour toutes les commandes sans opérateur de la cellule : c'est un **majorant** du gain.

**`grep()` et `glob()` n'existent pas dans le kernel aujourd'hui** : seul `read()` est défini (`prime-agent-runtime/src/rlm/read.py:32`), et le prompt v2 ne documente que lui (`crates/pa-core/src/prompts/ya_prompt_v2/core.md:11`). Aucune cellule des traces ne les appelle (461 cellules appellent `read(`, 0 `grep(` ni `glob(`) : les garder dans la liste ne change pas le compte. Constat, pas une proposition de les ajouter.

#### Résultats

| | v2 (136 sessions, 1 935 cellules) | v1 (420 sessions, 6 723 cellules et 879 autres tools) |
|---|---|---|
| (b) stricte : confirmations par session | **10** (p90 20, max 30) ; 1 580 | 15 (p90 27, max 61) ; 7 074 |
| (b) stricte : cellules sans question | **355, soit 18,3 %** | 528, soit 7,9 % |
| (b) stricte + « toujours » : confirmations par session | **10** (p90 20, max 30) ; 1 575 | 15 (p90 27, max 61) ; 7 007 |
| (b) stricte + « toujours » : cellules sans question | **360, soit 18,6 %** | 595, soit 8,9 % |
| Rappel, (b) large du constat | 5 (p90 11) ; 60,1 % | 6 (p90 11) ; 72,9 % |

**Le chiffre change beaucoup : 18,3 % des cellules v2 passent sans question, contre 60,1 % avec la définition large.** La médiane des confirmations double (10 contre 5) et « toujours » n'apporte presque rien (5 cellules de plus sur 1 935).

Raisons de confirmation en v2 (une cellule peut en avoir plusieurs ; entre parenthèses, les cellules où c'est la seule raison) :

| Raison | Cellules v2 |
|---|---|
| `bash()` avec un opérateur (`;`, `&&`, `\|`, redirection…) | 693 (569) |
| Autre appel (`Path(…)`, `os.getcwd()`, `os.path.abspath(…)`, `asyncio.gather(…)`, `str(…)`, `.read_text()`…) | 512 (85) |
| `edit()` | 454 (223) |
| Autre construction (`import`, boucle, compréhension, `assert`…) | 333 (13) |
| `bash()` sans opérateur, hors liste (tests, builds, formateurs…) | 249 (128) |
| `bash()` non littéral | 29 (0) |
| Cellule illisible | 9 (9) |

Lecture :

- **La première cause est l'opérateur shell** : seule raison pour 569 cellules (29 % des cellules v2), surtout des lectures enchaînées (`sed -n '1,120p' a; sed -n '1,90p' b`, `git status --short; grep …`, `… | head`). **Sensibilité, pour information, pas une option** : si `;`, `&&`, `||` et `|` étaient permis **entre** lectures sûres (toujours sans `$()`, backtick ni redirection), 547 cellules v2 passeraient (28,3 %), v1 790 (11,8 %).
- **« Toujours » aide peu** avec la commande exacte : dans les cellules confirmées, 213 des 271 occurrences de commandes sans opérateur hors liste n'apparaissent qu'une fois dans leur session (les tests changent d'arguments : `-k …`, fichiers), et la plupart de ces cellules demandent aussi pour une autre raison (`edit()`, `import`, appel). Compteurs `memo_cmd_occurrences` et `memo_cmd_occurrences_unique_in_session` du script (v1 : 786 sur 1 113).
- Le reste de l'écart vient du Python : le modèle lit souvent par `Path(p).read_text()`, `os.path.abspath`, `os.getcwd()` ou une boucle plutôt que par `read()`.

Contrôle à la main sur un tirage de 12 cellules v2 classées sans question : toutes sont des `read()`, `print()` ou `bash()` de lecture (`cat`, `ls -la`). Contrôles de commandes : `sed -n '1,120p' a.py` et `git status --short` passent ; `sed -n '1e rm x' a`, `rg --pre cat x`, `git -c core.pager=x log`, `find . -exec rm {} ;`, `git diff --output=x`, `ls; rm x`, `cat $(echo x)`, `npm test` demandent.

Limites : celles de la section 3.2. Les traces viennent de sessions sans permissions ; le modèle ne voit pas les confirmations (seulement les refus, section 5), donc rien ne l'incite à écrire des cellules qui passent sans question, et le prompt n'en dira rien.

## 4. Hors interface : `-p`, RPC, banc

### Constaté

- `-p` et RPC font tourner le moteur dans le process (section 1.1) : personne ne peut répondre à une demande, sauf à ajouter une commande RPC.
- Le banc lance `-p --mode json` (`bench/smoke/run.py:385-387`) et écrit `settings.json` dans un dossier agent dédié, `{"cacheRetention": …, **settings}` (`run.py:310-319`) ; le banc de capture fait de même (`bench/tools/capture/run.py:128-132`). Les fichiers de `bench/smoke/settings/` ne portent que la variante (`default.json` vaut `{}`).
- Un réglage inconnu d'un binaire est gardé tel quel, sans erreur (`crates/pa-core/src/settings/types.rs:384-386`) : écrire le réglage avant qu'un binaire le connaisse ne casse pas les anciens binaires (A/B sur binaires d'avant le ticket).
- Les réglages de projet (`<cwd>/.yusagent/settings.json`) écrasent les réglages globaux (`crates/pa-core/src/settings/manager.rs:49`). **Un dépôt cloné pourrait donc passer la session en « auto »** si le réglage était lu dans les réglages fusionnés.

### Proposé

- **Banc** : `run.py` (`write_agent_config`) et `bench/tools/capture/run.py` (`write_settings`) écrivent toujours `"permissions": "auto"`, comme `cacheRetention`, sans toucher aux 14 fichiers de `bench/smoke/settings/` ; `default.json` reste `{}`. Plus sûr que de compter sur chaque fichier de variante : un fichier oublié bloquerait le run. `known_settings` (`run.py:1126-1130`) l'ajoute au rapport.
- **Manuel sans client capable de répondre** (`-p`, `text`, `json`, RPC) : **erreur au démarrage**, avant tout appel de modèle, code de sortie non nul, message qui donne la ligne exacte et le chemin réel des réglages globaux (D6), par exemple « permissions are manual and this mode cannot ask. To run without confirmations, add this line to /Users/…/.yusagent/settings.json: "permissions": "auto" » (le chemin suit `PRIME_AGENT_CODING_AGENT_DIR`). Plutôt qu'un refus cellule par cellule : un refus en cours de session coûte des appels de modèle et ressemble à un échec de tâche au banc. Une commande RPC de réponse pourra s'ajouter plus tard si un client RPC en a besoin.
- **ACP** : toujours capable de répondre (méthode sans capacité, section 1.2).
- **Sous-agents** (`rlmMaxDepth` ≥ 1, prompt v1 seulement) : une session enfant hérite du mode et du canal de demande de son parent ; à défaut, refus au démarrage. Hors du chemin par défaut (`docs/yusAgent-cdc.md:88`).

## 5. Effet sur les requêtes

### Constaté

- Une cellule bloquée par `before_tool_call` devient un résultat de tool en erreur dont le texte est `reason` (`tool_call.rs:90-97`). Il est ajouté **à la fin de l'historique**, comme tout résultat : il ne touche ni le prompt système ni les tools, et reste identique dans les requêtes suivantes (il est écrit dans la session).
- L'attente d'une réponse a lieu entre deux appels de modèle : elle ne change pas le préfixe, mais **une attente plus longue que le TTL du cache** (`PI_CACHE_RETENTION`) rend l'appel suivant froid. Le CDC prévoit un « keep-warm optionnel pendant les attentes d'approbation » (`docs/yusAgent-cdc.md:288`).

### Proposé

- **Le réglage ne change ni le prompt ni les tools** : rien n'est ajouté au prompt v2 pour annoncer les permissions. Vérifié par `crates/pa-core/tests/prefix_stability.rs` (empreintes identiques en manuel et en auto) et, après le build, par un run `--capture` comparé à `main` (`compare_capture_prompts.py`, « TOUT IDENTIQUE » attendu).
- **Texte reçu par le modèle** pour une cellule refusée, fixe et sans donnée variable : `[permission-denied] The user declined this cell; nothing was executed.` Le marqueur en tête sert au comptage du banc, comme `[outside-workspace]` (`crates/pa-core/src/tools/ya_workspace.rs:21`). Une demande annulée (`cancelled`, `session/cancel`) : la cellule n'est pas exécutée et le tour s'arrête comme une annulation.
- **Après un refus, le tour s'arrête** et la main revient à l'utilisateur, au lieu de laisser le modèle retenter (sans quoi il peut reformuler la même action). À vérifier à l'implémentation : le chemin « bloqué » (`Preparation::Immediate`, `tool_call.rs:93-97`) ne passe peut-être pas par `after_tool_call`, qui porte `terminate` (`crates/pa-agent/src/types.rs:649`).

## 6. À trancher par Yusuf

**Tranché le 20/11/2026** : voir « Décisions de Yusuf sur le constat » en tête. Écarts avec les recommandations ci-dessous : **D2 prend l'option (a)**, chaque appel de tool confirmé sans classement ((b) reporté après le sandbox, ticket à part) ; **D3 est sans objet** (« toujours » retiré, « auto » pour la session à la place). D1, D4, D5, D6 et D7 suivent la recommandation (D6 : le message donne la ligne exacte). Le tableau est gardé tel que proposé.

| # | Question | Options | Recommandation |
|---|---|---|---|
| D1 | **Portée du réglage** | (i) figé par session dans `yaRequest` ; (ii) persistant, lu dans les réglages globaux à chaque cellule, avec changement par session à chaud. | **(ii).** `yaRequest` sert aux réglages qui façonnent les requêtes (`crates/pa-core/src/session_engine/ya_request.rs:1-2`) ; les permissions n'en changent aucune. Figé, une session créée en « auto » (au banc par exemple) resterait en « auto » à sa reprise même si l'utilisateur est repassé en manuel. Réglage `"permissions": "manual" \| "auto"` lu **dans les seuls réglages globaux** (`SettingsManager` garde `global` à part, `manager.rs:42-52`), jamais dans `<cwd>/.yusagent/` (section 4). Changement par session : sélecteur `permissions` dans `session/set_config_option` (ACP, à côté de `model` et `thought_level`), toujours à l'initiative de l'utilisateur (spec ACP) ; chaque changement et chaque décision journalisés dans la session (critère « Décisions de permission journalisées », `docs/yusAgent-cdc.md:409`), sans être relus à la reprise : une session reprise suit le réglage global. |
| D2 | **Option du mode manuel** | (a), (b), (c), section 3. | **(b) avec « toujours pour cette commande »** : 3 confirmations par session en médiane sur le banc, interception fiable à la cellule, classifieur en liste blanche conservatrice présenté comme heuristique. (c) coûte le plus et laisse le Python sans aucune question. (a) reste possible en réglage (`"permissions": "manual-all"` ?) si Yusuf veut tout voir ; non recommandé par défaut (fatigue). |
| D3 | **« Toujours pour cette commande »** | Portée : session, projet, global. Clé : commande exacte, programme + sous-commande, préfixe. Stockage : mémoire, fichier. | **Session**, en mémoire, journalisée (décision et clé) mais non relue à la reprise. Clé = programme + sous-commande (trois mots pour `npx`, `uv run`, `npm run`, `python -m`), comme le comptage. Jamais pour une commande avec redirection, un `bash()` dynamique ou une écriture Python. « Toujours » pour `edit()` = toutes les écritures d'`edit()` dans le workspace de la session. Persistance par projet (`~/.yusagent/`, jamais dans le dépôt) à rejuger après usage. |
| D4 | **Avertissement persistant quand « auto » est actif sans sandbox** (proposé, non validé) | Oui / non ; où. | **Oui.** C'est déjà un critère de YA-11 pour tout mode sans sandbox (`docs/yusAgent-cdc.md:410`). En « auto » : ligne sur stderr au démarrage en `-p` (le banc l'ignore), état dans la barre du TUI, `session_info_update` en ACP (affiché par l'extension). Texte fixe, hors du prompt. |
| D5 | **Testeurs en manuel au départ** (proposé, non validé) | Oui / non. | **Oui**, cohérent avec « manuel par défaut » et « auto jamais par défaut ». En pratique, aucun build n'ira aux testeurs avant le sandbox (`docs/yusAgent-cdc.md:36`) : la question se reposera avec lui (« auto » sous sandbox). |
| D6 | **Manuel sans client** (`-p`, RPC) | Erreur au démarrage ; refus par cellule. | **Erreur au démarrage** (section 4). |
| D7 | **Tour après un refus** | Le modèle continue ; le tour s'arrête. | **Le tour s'arrête** (section 5). |

## 7. Découpage et tests (après les décisions du 20/11/2026)

### PR

1. **PR 1, cœur et banc** (`crates/pa-core`, module du fork, par exemple `session_engine/ya_permissions.rs`) :
   - **hook `before_tool_call` branché sur la session principale** : une ligne dans `Agent::new` (`crates/pa-core/src/session_engine/engine.rs:893-944`, fichier upstream, listée dans `docs/fork-changes.md`) ; en manuel, **chaque appel de tool** passe par le demandeur, sans classement (D2 révisé) ;
   - **réglage `permissions`** (`"manual"` par défaut, `"auto"`) **lu seulement dans les réglages globaux** (D1), hors de `yaRequest` ;
   - interface « demandeur » avec deux implémentations dans cette PR : « auto » (accord immédiat) et « aucun client » ;
   - **erreur au démarrage en `-p` et RPC en manuel**, avant tout appel de modèle, avec la ligne exacte `"permissions": "auto"` et le chemin réel des réglages globaux (D6) ;
   - **tour arrêté après un refus** (D7), résultat de tool au texte fixe `[permission-denied] The user declined this cell; nothing was executed.` (section 5) ;
   - journal des décisions (accord, refus, annulation, changement de mode) dans la session ;
   - avertissement sur stderr au démarrage en « auto » sans sandbox (D4) ;
   - **banc et banc de capture en « auto »** dans la même PR : `bench/smoke/run.py` (`write_agent_config`) et `bench/tools/capture/run.py` (`write_settings`) écrivent `"permissions": "auto"` ; `bench/smoke/settings/default.json` reste `{}`.

   Entre la PR 1 et la PR 2, une session du démon (TUI, ACP) en manuel échoue à sa création avec le même message que D6 : régler `"permissions": "auto"` le temps de la PR 2.
2. **PR 2, démon et ACP** : demande worker → superviseur → client attaché (nouveau message du protocole du démon, module du fork), première réponse retenue si plusieurs clients ; `session/request_permission` côté ACP (le pont doit accepter les réponses du client, `jsonrpc.rs:57-60`), options `allow_once` et `reject_once` seulement (pas d'`allow_always` : D3 sans objet), `tool_call` en `pending` jusqu'à la décision (aujourd'hui `in_progress`, `wire_events.rs:156`), `session/cancel` qui répond `cancelled`, sélecteur `permissions` (`manual`, `auto`) dans `session/set_config_option`, à l'initiative de l'utilisateur seulement.
3. **PR 3, TUI** : boîte minimale (autoriser, refuser), moyen de passer la session en « auto », état « auto sans sandbox » dans la barre (D4). La refonte du CLI reste après YA-10 (`docs/yusAgent-cdc.md:37`).

Ensuite : sandbox du kernel (seconde partie de YA-11) ; puis, dans un ticket à part, le classement (b) rejugé sur des traces de testeurs (D2 révisé) ; keep-warm pendant l'attente si les mesures montrent des appels froids.

### Préfixe

**Le préfixe ne change pas.** Le hook agit dans la boucle d'agent, entre la réponse du modèle et l'exécution du tool : il n'ajoute rien au prompt système ni aux définitions de tools, et le réglage n'est pas dans `yaRequest`. Un refus ajoute un résultat de tool en fin d'historique, au texte fixe, identique dans les requêtes suivantes (section 5) : c'est de l'historique, pas du préfixe. En « auto », les requêtes sont identiques à celles de `main`. Vérifications prévues : `prefix_stability.rs` et la capture (tests ci-dessous). Seul effet sur le cache : une attente de réponse plus longue que le TTL rend l'appel suivant froid (section 5).

### Tests de la PR 1 (provider factice ou requêtes interceptées, aucun modèle réel)

- **Remplace le critère du 28/10** (« Auto-approve des cellules refusé hors sandbox ou conteneur, testé ») : « Sans réglage, le mode est manuel ; aucun appel de tool ne s'exécute sans accord explicite ; "auto" n'est actif que par le réglage global de l'utilisateur (un réglage de projet est ignoré) ; en "auto" sans sandbox, l'avertissement est émis. » Tests e2e avec le provider factice (`PRIME_AGENT_FAUX_SCRIPT`, `crates/pa-cli/src/print_runtime.rs:195`) et un demandeur de test : cellule `edit()`, cellule `bash("touch x")`, cellule de lecture seule (demandée elle aussi) ; réglage absent → manuel ; refus → fichier absent et résultat `[permission-denied] …` ; accord → fichier créé ; `"auto"` global → exécution sans demande et avertissement sur stderr.
- **Arrêt du tour quand le hook bloque une cellule** (D7) : provider factice scripté avec deux réponses, une cellule puis une seconde cellule ; demandeur de test qui refuse. Attendu : la première cellule n'est pas exécutée (fichier absent), le résultat de tool est `[permission-denied] …`, **le provider factice ne reçoit aucune requête de plus dans ce tour** (la seconde réponse n'est jamais demandée), le tour se termine et la session attend l'utilisateur ; un nouveau prompt reprend normalement. Variante annulation : réponse `cancelled` → même arrêt.
- **Un `settings.json` de projet ne peut pas passer la session en « auto »** (D1) : réglages globaux sans `permissions` (ou `"manual"`), `<cwd>/.yusagent/settings.json` avec `"permissions": "auto"`. Attendu : mode manuel ; en `-p`, sortie en erreur au démarrage avec le message de D6, **avant toute requête** au provider factice ; avec un demandeur de test, la cellule est demandée. Contrôle inverse : `"auto"` dans les réglages globaux → auto, quel que soit le réglage de projet. Le mode lu ne vient jamais des réglages fusionnés (`manager.rs:49`).
- **`-p` et RPC en manuel** (D6) : sortie en erreur au démarrage, code non nul, message avec la ligne exacte et le chemin, aucune requête au provider factice.
- **Préfixe** : `prefix_stability.rs`, empreintes SHA-256 des instructions et des tools identiques en manuel et en auto ; après le build, capture `py-example-001` contre `main`, `compare_capture_prompts.py` « TOUT IDENTIQUE ».
- **Journal** : chaque décision (accord, refus, annulation, changement de mode) écrite dans la session.
- **Banc** : run `--capture` d'une tâche avec le binaire de la PR 1, code de sortie 0, réglage `"permissions": "auto"` écrit par le banc.

### Tests des PR 2 et 3

- **ACP** (client factice) : forme de `session/request_permission` (options `allow_once`, `reject_once`), `pending` puis `in_progress`, `session/cancel` → `cancelled` → cellule non exécutée, issue inconnue → refus, sélecteur `permissions` qui passe la session en « auto » et le journalise ; une session reprise suit le réglage global.
- **TUI** : la boîte s'affiche pour chaque appel de tool en manuel ; l'état « auto sans sandbox » reste visible.

## PR 1 : cœur et banc (20/11/2026)

Branche `ya-20-permissions-coeur`, depuis `main` à `e8449ee32` (merge de la PR #65). Fichiers et tests : `docs/fork-changes.md`, section « Permissions, PR 1 ».

- **Arrêt du tour, vérifié avant d'écrire le reste** : possible sans toucher à `pa-agent`. Quand le hook `should_stop_after_turn` renvoie `true` après un lot de tools, la boucle émet `AgentEnd` et rend la main, sans nouvel appel au modèle ni lecture des suivis (`crates/pa-agent/src/agent_loop/run.rs:132-152`). Le gate garde l'id des appels refusés ; l'enveloppe du hook de fin de tour de l'embedding (`config.should_stop_after_turn`) arrête le tour si le lot qui vient de finir en contient un. Le bloc upstream qui compose ce hook n'est pas modifié.
- **Réglage** : `"permissions"` lu dans le document brut des réglages globaux (`SettingsManager::global_raw`), jamais dans les réglages fusionnés ; valeur autre que `"manual"` ou `"auto"` : refus à la création, avec le chemin du fichier.
- **Refus au démarrage** (D6) : dans `create_session`, juste après le chargement des réglages, avant toute requête et avant le préchauffage du kernel. S'applique à toutes les sessions sans demandeur : `-p`, RPC et, jusqu'à la PR 2, le démon (TUI, ACP). Message :

  ```
  permissions are "manual" (the default) and this mode cannot ask for confirmation. To run tool calls without confirmation (there is no sandbox yet), add this line to <dossier agent>/settings.json:
    "permissions": "auto"
  A project's .yusagent/settings.json is not read for this setting.
  ```
- **Refus d'un appel** (D7) : résultat `[permission-denied] The user declined this cell; nothing was executed.`, en erreur ; les autres appels du même message ne sont pas demandés et reçoivent le même texte ; une demande annulée vaut refus. Journal : entrée `ya-permission` du fichier de session (`toolCallId`, `toolName`, `decision` : `allow`, `deny`, `cancelled` ou `skipped`), jamais envoyée au modèle. En « auto », rien n'est journalisé par appel.
- **Avertissement** (D4) : sur stderr au démarrage d'une session racine sans démon (`-p`, RPC) quand « auto » vient des réglages de l'utilisateur. Le TUI et ACP l'afficheront dans les PR 2 et 3.
- **Tests upstream : écart à signaler.** Avec « manuel » par défaut et le refus au démarrage, les tests qui lancent une session sans demandeur (presque tous les e2e ; `git grep -l PRIME_AGENT_CODING_AGENT_DIR -- 'crates/*/tests/*'` : 88 fichiers de tests) échoueraient. Plutôt que de modifier ces fichiers upstream, `.cargo/config.toml` pose `PRIME_AGENT_TEST_PERMISSIONS = "auto"`, comme il pose déjà `PRIME_AGENT_PROMPT_VERSION` et `RLM_MAX_DEPTH` pour eux. **Cette variable n'est lue que par les builds de debug** (`cfg!(debug_assertions)`) : un binaire de release (build macOS, banc) ne lit que le réglage global (D1). Nos tests la retirent ou fixent le mode explicitement.
  Hors de `cargo`, deux chemins lancent aussi des sessions sans humain : les contrôles Python du cycle de vie de la CI (binaire de debug, environnement filtré : la variable est ajoutée à leurs listes blanches et exportée dans l'étape, après l'échec du run `ci` 44703584290) et le perf nocturne (`perf.yml`, binaire de release : `"permissions": "auto"` écrit dans le `settings.json` de son corpus, comme le banc).
- **Préfixe** : inchangé (rien dans le prompt, les tools ni `yaRequest`) ; test `permissions_manual_and_auto_send_the_same_prefix` dans `crates/pa-core/tests/prefix_stability.rs`, puis contrôle en `--capture` après le build.
- **Banc** : `bench/smoke/run.py`, `bench/tools/capture/run.py` et `bench/smoke/tools/notice-batch-repro.sh` écrivent `"permissions": "auto"` ; `bench/smoke/settings/default.json` reste `{}`. Tests Python du banc : `python3 -I -m unittest discover -s bench/smoke/tests`, 103 tests OK.
- **Pas compilé en local** (règle 8 de `CLAUDE.md`) : la compilation, clippy et les tests Rust passent par la CI.
- **CI** : run `ci` 44708605726 sur `8943999fb`, 14 checks requis verts. Deux runs précédents en échec, corrigés : clippy `large_stack_arrays` dans le test e2e (run 44703584290) ; contrôles Python du cycle de vie lancés hors de `cargo` sans la variable de test (runs 44703584290 et 44706678374, shard 1). Build macOS de la branche : run 44706671367 sur `32903c963` (code livré identique à `8943999fb`).
- **Contrôle en capture** (Yusuf, 20/11/2026) : `py-example-001`, binaires `5f596c958` (`main`, run 44610160108, code livré identique à `e8449ee32`) et `32903c963` (branche, run 44706671367) : `compare_capture_prompts.py` sur 9 requêtes, **TOUT IDENTIQUE** ; deux dossiers de run différents ; `('py-example-001', 'resolved', 0)` des deux côtés ; rien d'écrit dans `~/.yusagent` ni `~/.prime/agent`. `-p` du binaire de la branche sans réglage : code de sortie 1, message avec la ligne `"permissions": "auto"` et le chemin des réglages globaux, **0 connexion** au serveur du modèle. Même `-p` avec `PRIME_AGENT_TEST_PERMISSIONS=auto` : même refus, 0 connexion (le binaire de release ignore la variable).
- **Merge** : PR #66 mergée le 20/11/2026 (« merge » de Yusuf), `main` à `f598ef0b6`.

## PR 2 : démon et ACP (20/11/2026)

Branche `ya-20-permissions-acp`, depuis `main` à `f598ef0b6` (merge de la PR #66). Fichiers et tests : `docs/fork-changes.md`, section « Permissions, PR 2 ».

Réponses de Yusuf aux questions de la passation (20/11/2026) :

- **Sélecteur `permissions`** de `session/set_config_option` dans la PR 2, dans un commit séparé ; chaque changement journalisé (entrée `ya-permission`) ; retour en manuel permis ; préfixe inchangé.
- **Sessions créées par le TUI** : toujours refusées à la création en manuel jusqu'à la PR 3. Seul le pont ACP déclare savoir répondre.
- **Déconnexion du client ou `session/cancel` pendant une demande** : vaut refus, le tour s'arrête (D7). Les deux cas sont testés.

Ce qui est fait :

- **Qui répond.** La commande `create` du mode ACP porte `yaPermissionAsker: true` ; le superviseur la garde dans la commande `create` durable (liste blanche, `crates/pa-daemon/src/supervisor/worker_lifecycle.rs`), le worker la lit (la session sait demander) ; l'`attach` du pont porte le même champ (ce client répond : le `create` reçu par le worker n'a pas d'id de client). Sans ce champ, le moteur n'a pas de demandeur et une session manuelle est refusée comme dans la PR 1.
- **Demande** (`crates/pa-daemon/src/ya_permissions.rs`). Le demandeur du worker émet l'événement de session `ya_permission_request` (`requestId`, `toolCallId`, `toolName`, `args`) à tous les clients attachés ; la première réponse retenue (commande `ya_permissions`, `action: "respond"`, `outcome: "allow"` ; toute autre valeur est un refus) ; `ya_permission_resolved` ferme la demande chez les autres clients. Retrait (refus, le tour s'arrête) si le tour est annulé pendant l'attente, si le dernier client qui répond se détache (déconnexion : le superviseur envoie `detach`), ou aussitôt si aucun client qui répond n'est attaché. À la déconnexion d'un client ACP, deux chemins sont en course : ce retrait (résultat `[permission-denied] …`) ou l'annulation du tour par le démontage du pont (résultat « Request was aborted ») ; les deux valent refus, le tour s'arrête et la décision est journalisée `cancelled` (vu des deux façons en CI, runs 44721542873 et 44723659629).
- **Journal.** Le moteur du démon tourne sur un gestionnaire de session en mémoire (`crates/pa-daemon/src/agent_engine/lifecycle.rs`, `build_session`) : le journal du gate est injecté (`PermissionsWiring::journal`) et le worker écrit l'entrée dans son fichier (`custom`, `customType: "ya-permission"`). Une demande abandonnée par une annulation est journalisée `cancelled` (le hook est abandonné par `race_with_abort`, le journal part du destructeur).
- **ACP** (`crates/pa-daemon/src/acp/ya_permissions.rs`). `session/request_permission` (requête agent → client, id `ya-permission-<n>`) avec `toolCall: {toolCallId, status: "pending"}` et deux options, `allow-once` (`allow_once`) et `reject-once` (`reject_once`). Les réponses du client sont lues avant `jsonrpc::parse_line`, qui continue de refuser les autres réponses ; `selected` + `allow-once` autorise, `cancelled` retire la demande (journal `cancelled`), tout le reste refuse ; une réponse à une demande déjà fermée est ignorée. En manuel, le `tool_call` part en `pending`, puis `tool_call_update` `in_progress` après un accord. Le worker publie les événements du moteur depuis sa propre file : sa demande peut doubler le `tool_execution_start` de l'appel ; le pont la retient jusqu'à la publication du `tool_call` (constaté au premier run `ci` 44721542873, test e2e a).
- **Sélecteur** (commit séparé). `permissions` (`manual`, `auto`, catégorie `mode`), ajouté en dernier aux options ; `get_connection_state` porte `yaPermissions` pour une session qui sait demander. Le choix passe par `ya_permissions` `action: "set_mode"` ; avec un demandeur, le gate est posé dans les deux modes et lit le mode avant chaque appel (`PermissionSwitch`) ; le choix survit aux reconstructions du moteur dans le worker ; une session reprise suit de nouveau le réglage global (D1). Chaque changement est journalisé (`mode`, `previousMode`, `change: "user"`).
- **Préfixe** : inchangé. Rien dans le prompt, les tools ni `yaRequest` ; les entrées `ya-permission` sont ignorées à la reprise (`build_session_context`). Tests : `permissions_journal_does_not_change_the_requests_of_a_resumed_session` (`crates/pa-core/tests/prefix_stability.rs`, requêtes entières identiques hors `prompt_cache_key`, avant et après la reprise) et `journal_entries_change_nothing_in_a_resumed_context` (reprise du démon depuis le fichier du worker).
- **Pas compilé en local** (règle 8) : formatage par `rustfmt`, compilation, clippy et tests par la CI.

## Points ouverts

- PR 2 : l'avertissement « auto sans sandbox » (D4) n'a pas de `session_info_update` dédié en ACP ; seul le libellé de l'option `auto` du sélecteur le dit (« no sandbox »). Un TUI attaché à une session ACP voit les événements de demande sans pouvoir y répondre (PR 3). Après une relance du worker, le client ACP n'est plus enregistré comme répondant jusqu'à son prochain `attach` : les demandes sont refusées aussitôt.

- Arrêt du tour après un refus par le chemin `Preparation::Immediate` (section 5) : à vérifier dans `pa-agent` au début de la PR 1 ; le test de D7 le prouvera.
- **Fatigue** : en manuel, 13 confirmations par session en médiane sur le banc (p90 23, prompt v2). Le passage en « auto » pour la session est la réponse retenue ; le classement (b) sera rejugé après le sandbox (ticket à part).
- Le keep-warm du cache pendant l'attente (`docs/yusAgent-cdc.md:288`) n'est pas dans ce découpage.
- Les tools MCP en écriture (`docs/yusAgent-cdc.md:533`) passent par le kernel, donc par la cellule : demandés comme toute cellule.
