# Ticket 16 : YA-09, `cwd=` et shell persistant de `bash()`

Demande de Yusuf du 18/11/2026. **Phase 1 : constat et options, aucun code.** Branche `ya-16-bash-cwd-shell`, depuis `main` à `1c1ac392c`. Une PR de documentation, pas de merge sans son accord. Aucun appel à un vrai modèle, aucune chauffe, rien compilé.

Le CDC a regroupé ces points avec YA-09 le 13/11/2026 : « `timeout=`, `cwd=` de `bash()` et shell principal persistant hors YA-03 de base, groupés avec YA-09 » (`docs/yusAgent-cdc.md:51`, aussi `docs/tickets/09-ya03-base.md:16` et `:151`). `timeout=` est livré (ticket 11, PR #42). Restent `cwd=` et le shell principal persistant.

Les sections 1 à 4 sont des constats. La section 5 contient les propositions.

## 1. Constat : notre `bash()` sur `main` (`1c1ac392c`)

### Signature

```python
def bash(
    command: str,
    persistent: bool = False,
    env: dict[str, str] | None = None,
    timeout: float | None = ya_timeout.UNSET,
) -> BashHandle:
```

`prime-agent-runtime/src/rlm/bash.py:1323-1328`. `persistent` vient d'upstream ; `env` est un ajout du fork (YA-03, `fb49395ea`) ; `timeout` vient du ticket 11 (`ya_timeout.py`). Upstream aujourd'hui : `def bash(command: str, persistent: bool = False) -> BashHandle` (`git show upstream/main:prime-agent-runtime/src/rlm/bash.py`, ligne 1299, `upstream/main` à `b14426151`).

- **`cwd=` : non accepté.** Le paramètre n'existe pas dans la signature. Python lève donc `TypeError` pour un argument nommé inconnu (déduit de la signature, pas exécuté).
- **`env=` : accepté.** Contrôle par `_checked_env` (`bash.py:1454-1462`, dict de `str` vers `str` exigé, sinon `TypeError`). Fusion dans `_child_env` (`:1465-1494`) : `os.environ` du kernel, puis la garde non interactive d'upstream, puis `env`, qui a la priorité (`:1492-1493`). La variable vaut pour cette commande seule, l'`os.environ` du kernel n'est pas modifié (docstring, `:1362-1364`).

### Un processus par appel

Chaque appel lance un nouveau shell : `subprocess.Popen([_shell(), "-c", script], cwd=os.getcwd(), env=_child_env(self._env), …, start_new_session=True)` (`bash.py:461-468`) ; sous Windows, `_winjob.spawn_in_job(…, cwd=os.getcwd(), env=…)` (`:471-473`). `persistent=True` passe aussi par un nouveau processus, avec le même `cwd=os.getcwd()` et `env=_child_env(env)` (`_spawn_persistent`, `:1606-1610`). Le script exécuté est la commande entourée de la clôture de statut (`_status_script`, `:1434-1451`), puis le shell se termine.

Conséquences, déduites du code et non exécutées :

- **`cd` ou `export` dans un `bash()`** : ils s'appliquent au shell de cette commande, qui se termine avec elle. **Ils ne sont pas visibles au `bash()` suivant.**
- **`os.chdir()` ou `os.environ[...]` dans le kernel** : ils sont visibles aux `bash()` lancés ensuite, puisque `cwd` et `env` sont lus à chaque lancement (`:463`, `:1477`). Le shell lui-même (`PRIME_AGENT_BASH_SHELL`) est relu à chaque appel aussi (`_shell`, `:1396-1397`).

### Deux `bash()` en même temps

Ce sont deux processus indépendants, chacun dans son groupe de processus (`start_new_session=True`, `:467`). Chacun a son tampon de sortie, son handle et son entrée dans le journal des orphelins (`:376-520`). Il n'y a ni verrou ni file : les deux tournent en parallèle. Chacun prend le `cwd` et l'`env` du kernel au moment de son lancement. Un `cd` dans l'un n'a aucun effet sur l'autre. Un `os.chdir()` placé entre deux lancements de la même cellule ne vaut que pour le second.

Tests existants : `test_awaits_do_not_hold_executor_threads` lance 3 `sleep 0.5` en parallèle et vérifie leurs codes de sortie (`prime-agent-runtime/test/test_bash.py:395-405`). Aucun test du runtime ne lie `os.chdir()` à `bash()` : `grep -rln chdir prime-agent-runtime/test` ne trouve que `test_ya_read_env.py`, `test_ya_edit.py` et `test_factory.py`, sans appel à `bash()` dans ces cas.

### Points voisins

- **`persistent=True` n'est pas le « shell principal persistant » du CDC.** C'est une commande unique qui survit aux redémarrages du kernel, du worker et du démon (upstream `e8dc89969`, #3640, 20/10/2026 ; docstring `bash.py:1349-1360`). Ce n'est pas un shell réutilisé d'un appel à l'autre. Le mot « persistant » désigne donc deux choses différentes dans le CDC et dans le code.
- **Redémarrage du kernel.** Le kernel démarre dans le cwd de la session (`crates/pa-core/src/kernel/manager/startup.rs:177-179`), avec un environnement reconstruit par l'hôte (`:180`, `env_clear().envs(env)`). `state_snapshot.rs` ne contient ni `cwd` ni `getcwd`. Déduction, non vérifiée par exécution : un `os.chdir()` ou un `os.environ[...]` fait avant un crash est perdu au redémarrage, alors que l'avis de redémarrage dit « état restauré depuis la dernière cellule réussie » (journal du CDC, 13/11/2026).
- **`edit()` et `read()`.** Le workspace d'`edit()` est fixé à l'import, sur le cwd de départ du kernel (`skills/edit/src/edit/__init__.py:21`). Un `os.chdir()` ne le déplace pas. En revanche, un chemin relatif passé à `read()` ou `edit()` se résout depuis le cwd courant du kernel.
- **Bloc `<environment>`** : il porte le cwd de la session, pas celui du kernel (`crates/pa-core/src/session_engine/environment_block.rs`, `docs/fork-changes.md:33`).

### Usage observé au smoke bench (constat local)

Commande (runs locaux, non versionnés ; le script ci-dessous est la commande complète) :

```bash
python3 -I - <<'EOF'
import glob, json, re, collections
CHDIR = re.compile(r"\bos\.chdir\(")
CD = re.compile(r"(^|[;&|(\n]\s*)cd\s+\S")
EXPORT = re.compile(r"(^|[;&|(\n]\s*)export\s+\w+=")
BASH = re.compile(r"\bbash\(")
stats = collections.defaultdict(collections.Counter)
for path in glob.glob("bench/smoke/runs/*/tasks/*/run-*/sessions/*.jsonl"):
    lines = open(path).read().splitlines()
    if not lines: continue
    ya = json.loads(lines[0]).get("yaRequest") or {}
    s = stats[f"v{ya.get('promptVersion', 1)}"]
    s["sessions"] += 1
    for l in lines[1:]:
        m = json.loads(l).get("message") or {}
        if m.get("role") != "assistant": continue
        for c in m.get("content", []):
            if c.get("type") != "toolCall" or c.get("name") != "python_repl": continue
            code = (c.get("arguments") or {}).get("code") or ""
            s["cells"] += 1
            for k, rx in (("bash", BASH), ("os.chdir", CHDIR), ("cd", CD), ("export", EXPORT)):
                if rx.search(code): s["cells_with_" + k] += 1
for v in sorted(stats): print(v, dict(stats[v]))
EOF
grep -o 'bash([^)]\{0,200\}cwd=' bench/smoke/runs/*/tasks/*/run-*/sessions/*.jsonl | wc -l
```

Résultat le 18/11/2026 : v1, 184 sessions, 2 738 cellules, dont 1 164 avec `bash(` ; v2, 14 sessions, 91 cellules, dont 55 avec `bash(`. **Aucune cellule avec `os.chdir(`, `cd <dir>` ou `export X=`, et aucun `bash(…, cwd=…)`.** Le comptage est heuristique (expressions régulières sur le code des cellules). Limite : les tâches du smoke bench travaillent à la racine du dépôt. Ce résultat ne dit rien des dépôts où il faut changer de dossier, comme les monorepos.

## 2. Ce qu'exige le CDC (YA-03)

Texte (`docs/yusAgent-cdc.md:255-257`, critère `:271`) :

> `bash` (helper async) : `await bash(cmd, timeout=..., cwd=..., env=...)` renvoie un objet résultat (…). Parallélisme via `asyncio.gather`.
> - Shell principal persistant (`cd` et `export` conservés) pour les appels séquentiels.
> - Si le shell principal est occupé, l'appel part dans un sous-process éphémère qui hérite de son cwd et de son env. Les changements d'état de ces appels ne sont pas conservés, et le prompt système le dit.
>
> Critère : Un `cd` fait via le shell principal est visible dans l'appel séquentiel suivant.

| Exigence | État | Preuve |
|---|---|---|
| `timeout=` | fait | `bash.py:1327`, `ya_timeout.py` (ticket 11) |
| `env=` | fait | `bash.py:1326`, `:1454-1494` |
| `cwd=` | **manque** | absent de `bash.py:1323-1328` |
| Parallélisme via `asyncio.gather` | fait, par construction (un processus par appel) | `bash.py:461-468` ; règle du prompt v2, `core.md:16` |
| Shell principal persistant, `cd` et `export` conservés | **manque** | chaque appel lance `[_shell(), "-c", script]` (`bash.py:461-462`) |
| Sous-process éphémère si le shell principal est occupé, qui hérite de son cwd et de son env | sans objet aujourd'hui : **tous** les appels sont éphémères ; ils héritent du cwd et de l'env **du kernel**, pas d'un shell | `bash.py:463-464` |
| « le prompt système le dit » | le prompt dit **l'inverse** : `cd` et `export` ne sont jamais conservés | v2 `ya_prompt_v2/core.md:18` ; v1 (upstream) `layers/core.md:9`, `layers/usage.md:8` |
| Critère « un `cd` via le shell principal est visible dans l'appel séquentiel suivant » | **non rempli** | même preuve |
| Critère « au moins 4 `bash()` en parallèle, sortie de chacune accessible séparément » | rempli par construction, sans test à 4 commandes qui vérifie les sorties séparées | `test_bash.py:395-405` (3 commandes, codes de sortie seulement) |

Le CDC décrit donc un shell avec état. Le code, upstream, Unharnessed et nos deux prompts décrivent un shell sans état, où l'état passe par le kernel (`os.chdir`, `os.environ`).

## 3. Unharnessed

Clone jetable hors du dépôt (dossier temporaire de la session), lecture seule, rien exécuté ni compilé. **SHA `53bdd8784617fa9f2171abababd0b0f95618bfeb`** (17/11/2026 22:56 UTC, « The model additions follow models.dev »), le même que pour le ticket 15.

### Signature et comportement

```python
def bash(command: str, timeout: float | None = _DEFAULT_TIMEOUT) -> BashHandle:
```

`unharnessed-runtime/src/rlm/bash.py:1017`.

- **`cwd=` : non accepté. `env=` : non accepté.** Pas de `persistent` non plus : `git grep -c persistent -- unharnessed-runtime/src/rlm/bash.py` ne trouve rien. Le commit upstream qui ajoute les shells persistants (`e8dc89969`) est absent de leur historique (`git cat-file -t e8dc89969` : objet inconnu). Ils n'ont donc jamais eu cette fonction ; ils ne l'ont pas retirée.
- **Un processus par appel** : `subprocess.Popen(…, cwd=os.getcwd(), env=_child_env(), …)` (`bash.py:332-335`, Windows `:343`). `_child_env()` vaut `os.environ` plus la même garde non interactive, sans ajout par appel (`:1110-1137`).
- **`cd` ou `export` dans un `bash()`** : pas visibles au suivant. **`os.chdir()` et `os.environ`** : visibles, pour la même raison que chez nous.
- **Deux `bash()` en même temps** : deux processus indépendants, chacun dans son groupe. Leur README le dit : « Each call is a new process in its own **process group** » (`README.md:133`).

### Prompt

- `crates/uh-core/src/prompts/layers/core.md:7` : « `bash(command: str, timeout: float | None = 600) -> BashHandle`: start a shell command in the background and return at once; each call is a new process, a command still running after `timeout` seconds is killed (`None`: no limit), and a notification arrives when a command ends after its cell »
- `core.md:15` : « `os.chdir()` and `os.environ` changes persist and apply to later `bash()` calls. »
- `README.md:136` : « `os.chdir()` and `os.environ` changes in Python apply to later `bash()` calls. »

### A-t-il tranché ?

**Oui pour le comportement, sans justification écrite trouvée.** Le choix « un processus par appel, l'état passe par le kernel » est écrit dans leur prompt et dans leur README. Il vient d'upstream : avant leur réécriture, le prompt d'upstream disait déjà « each `bash()` call is its own process, so shell state does not persist between calls » et « Shell state does not persist between calls, but agents can use `os.chdir(...)` … and `os.environ[...]` … » (encore présent chez nous en v1, `crates/pa-core/src/prompts/layers/core.md:9` et `layers/usage.md:8`, identiques à `upstream/main`). Leur commit `14ba49745` (« unharnessed step 9: minimal system prompt », 01/10/2026) a condensé ce texte en `core.md:15`, avec la mention « The user approved the draft as is ».

Je n'ai trouvé chez eux ni discussion d'un shell persistant, ni de `cwd=` : `git grep -i "shell state\|persistent shell\|stateful shell\|main shell"` ne renvoie rien. Ils ont gardé le choix d'upstream sans le rediscuter, du moins dans ce qui est versionné.

## 4. Notre prompt v2 sur `bash()` aujourd'hui (texte exact)

Préfixe statique, `crates/pa-core/src/prompts/ya_prompt_v2/core.md`, configuration par défaut (`python_repl` seul) :

- `:7` : « - `bash(command: str, persistent: bool = False, env: dict[str, str] | None = None, timeout: float | None = 600) -> BashHandle`: start a shell command in the background and return at once; each call is a new process; `env` adds variables for this command only; a command still running after `timeout` seconds is killed (`None`: no limit) »
- `:8` : « - `persistent=True`: the shell survives kernel and harness restarts and has no default timeout; use it only for a server or job that must outlive a restart; after a restart, `from rlm import persistent_shells; persistent_shells()` returns the handles »
- `:9` : « - `BashHandle`: `.pid`, `.running`, `.command`, `.tail(n=50)`, `.output()`, `.poll() -> BashResult | None`, `.kill(sig=SIGTERM, grace=5.0)`, and `await handle -> BashResult` »
- `:10` : « - `BashResult`: `.exit_code`, `.output`, `.duration` »
- `:16` : « - Each tool call is a round trip. Put in one call every step that does not need a result you have not seen yet: independent reads, searches and commands together (several `bash()` under `asyncio.gather`), and an edit together with the command that tests it. »
- `:17` : « - Run programs with `bash()`, not `subprocess` or `os.system`. Write logic, loops and parsing in Python; use `bash()` only to call programs. Prefer `bash("""...""")`: quotes inside need no escaping. »
- `:18` : « - `os.chdir()` and `os.environ` changes persist and apply to later `bash()` calls; `cd` and `export` inside a `bash()` call do not. »
- `:24` : « - Do not hold the turn with `sleep` or long blocking awaits: end the turn and wait for the notification. »

Partie dynamique : ligne sans démon de la v2, « This session has no daemon: heartbeats never fire. Commands that end after their cell wake you with a `[bash-done]` message; the run waits for running commands before it exits. » (`ya_prompt_v2.rs:102`). En `bashWaitSeconds: "full"` (défaut v2), il n'y a pas de ligne `await bash(cmd)` (`system_prompt.rs:388-394`, `ya_bash_wait.rs:44-47`). Description du tool : « Run Python code in the persistent REPL. Top-level `await` works; variables persist across calls. » (`ya_prompt_v2.rs:104-105`). Variante `shellTool: "native"` : `:7`-`:10` sont retirées et `:17`-`:18` remplacées par « Each `exec_command` call starts a fresh shell in the working directory or in `workdir`; `cd`, `export`, `os.chdir()` and `os.environ` do not carry over to it. » (`ya_prompt_v2.rs:49-51`).

Les modifications de prompt propres à chaque option sont décrites en section 5.

## 5. Options (propositions)

Règles communes, tirées des règles du dépôt et des décisions déjà prises, pas des choix nouveaux :
- Toute variante est un réglage (règle 5 de `CLAUDE.md`).
- La v1 reste identique octet pour octet (décision 1 du ticket 11).
- **Rupture du préfixe** : une option qui change `core.md` change le préfixe statique de la v2. Si le changement passe par un réglage figé dans `yaRequest` (comme `bashWaitSeconds`), seules les sessions neuves sont touchées. Sinon, chaque session v2 existante subit une rupture de cache à sa reprise, comme pour D7 du ticket 13.
- **A/B passés** : les A/B de YA-03 (14/11), la comparaison avec Codex CLI (14/11) et l'A/B `reasoningSummary` (15/11) ont tourné en v1, et l'A/B du cache v2 (16/11) a opposé v2 à v1. Une option qui ne change que la v2 laisse le bras de contrôle `prompt-v1.json` intact. Elle rend en revanche les runs v2 antérieurs non comparables sur les métriques sensibles à `bash()` (tours, cellules en erreur, coût). Les baselines du banc sont déjà à refaire en v2 (point ouvert du ticket 11).

### Option A0 : statu quo (comportement d'Unharnessed sur ce point, plus `env=` et `persistent`)

- **Code** : aucun. **CDC** : retirer ou marquer comme écart `cwd=`, le shell principal, le sous-process éphémère et le critère « `cd` visible » (entrée au journal).
- **Prompt** : inchangé, pas de rupture.
- **A/B passés** : aucun effet.
- **`[bash-done]` et `"full"`** : aucun effet.
- **Tests** : facultatif, un test du runtime qui fixe le contrat écrit au prompt (`os.chdir()` et `os.environ` visibles au `bash()` suivant ; `cd` et `export` non visibles).
- **Vérification** : ce test, dans la CI (`runtime suite`).

### Option A1 : Unharnessed tel quel, mot à mot

- **Code** : s'aligner sur leur signature `bash(command, timeout)`, ce qui veut dire retirer `env=` (ajout du fork, YA-03) et `persistent` (upstream), ou au minimum ne plus les documenter. Retirer `env=` va contre le CDC (`:255`) ; retirer `persistent` modifie un fichier upstream et touche les sessions qui ont des shells persistants (`persistent_shells()`).
- **Prompt** : `:7` et `:18` repris mot à mot de leur `core.md:7` et `:15` (fin de `:18` supprimée : « ; `cd` and `export` inside a `bash()` call do not. ») ; `:8` supprimée. **Rupture** du préfixe v2.
- **A/B passés** : comme la règle commune.
- **`[bash-done]` et `"full"`** : leur `:7` porte la phrase sur la notification, notre v2 la porte dans la ligne sans démon (`ya_prompt_v2.rs:102`) ; il faudrait trancher pour ne pas la dire deux fois. `"full"` n'est pas touché.
- **Tests** : tests du prompt (marqueurs de `ya_prompt_v2.rs:238`, `:254`, `prefix_stability.rs`), tests du runtime sur `env` et `persistent` si on les retire.
- **Vérification** : CI, puis une chauffe pour contrôler le corps envoyé (`compare_bodies.py`), sur autorisation.
- **CDC** : mêmes modifications qu'A0, plus `env=` si on le retire.

### Option B : ce que dit le CDC (shell principal persistant, sous-process éphémère si occupé)

- **Code** : un shell long par session, dans le kernel, qui exécute les appels séquentiels ; les appels qui arrivent pendant qu'il est occupé partent dans un sous-process éphémère lancé avec son cwd et son env ; `cwd=` en plus. À mettre derrière un réglage figé dans `yaRequest` (nom à choisir, par exemple `bashMainShell`), dans un module séparé (règle 2). Questions de conception, aucune n'est tranchée par le CDC :
  1. **Une ou deux sources de vérité pour le cwd.** `read()`, `edit()` et le Python du kernel utilisent le cwd du kernel ; un `cd` dans le shell principal ne le change pas. Il faut soit synchroniser les deux (shell vers kernel après chaque commande, kernel vers shell avant), soit accepter qu'un `read("x")` après `bash("cd sub")` lise le mauvais fichier.
  2. **Définition de « occupé ».** Un `h = bash("serveur")` sans `await` occupe le shell principal pendant toute sa vie ; chaque `cd` qui suit part alors dans un éphémère et se perd. Sous `asyncio.gather`, seul le premier appel lancé va au shell principal. Que le `cd` tienne ou non dépend donc de ce qui tourne, et le modèle ne le voit pas.
  3. **Tension entre l'état et l'isolement.** Le `kill()`, le `timeout`, l'avis `[bash-done]` et la lecture d'un résultat (`bash.consumed`) reposent aujourd'hui sur un pid et un groupe de processus par commande (`bash.py:467`, `crates/pa-core/src/session_engine/bash_notices.rs:38`, `:59-60`). Si la commande tourne dans un processus enfant du shell principal, elle a son pid, mais son `cd` ne remonte pas au shell. Si elle tourne dans le shell lui-même, son `cd` est conservé, mais elle partage le pid du shell, et un `kill()` ou un `timeout` tue le shell et son état. Les deux exigences du CDC ne tiennent pas ensemble sans un mécanisme de plus, par exemple relire `pwd` et `export -p` à la fin de chaque commande, ce qui revient à l'option D.
  4. **Clôture de statut.** `_status_script` (`bash.py:1434-1451`) suppose un shell par commande, qui se termine après elle. Un shell long demande d'autres marqueurs par commande et une autre gestion de stdin et du code de sortie.
  5. **Redémarrage.** Le shell principal meurt avec le kernel, son état est perdu, et l'avis de redémarrage doit le dire.
  6. **Windows** (`_winjob`) et **nom** : le prompt dit déjà `persistent=True` pour autre chose (`core.md:8`).
- **Prompt** : `:7` (« each call is a new process ») et `:18` deviennent faux. Il faut un texte qui dise : appels séquentiels dans un même shell, `cd` et `export` conservés ; appel lancé pendant qu'un autre tourne, shell neuf depuis cet état, changements perdus (exigence du CDC). Il faut aussi la règle qui relie `os.chdir()` au shell, selon la réponse au point 1. **Rupture** du préfixe v2, limitée aux sessions neuves si le réglage est figé.
- **A/B passés** : chaque `bash()` de la v2 change de sémantique. Les runs v2 antérieurs (A/B du cache v2, chauffes des tickets 13 et 15) ne sont plus comparables sur les métriques sensibles à `bash()`. Le bras v1 n'est pas touché.
- **`[bash-done]` et `"full"`** : avec `"full"`, `await bash("cd x")` puis `await bash("make")` est déterministe. Le prompt v2 pousse pourtant à lancer sans `await` (`:7` « return at once », `:24`), et `py-silence-001` le fait (D8 du ticket 13) : le shell principal est alors occupé et les appels suivants deviennent éphémères. Les avis et `bash.consumed` sont indexés par pid : il faut un pid distinct par commande (point 3). Le regroupement des avis (« quand aucune commande ne tourne », `ya_bash_notice_batch.rs`) reste valable si chaque commande garde son propre handle.
- **Tests** (runtime) : `cd` et `export` visibles à l'appel séquentiel suivant ; éphémère quand le shell est occupé, avec héritage du cwd et de l'env, sans fuite d'état ; 4 appels sous `gather` avec des sorties séparées ; `timeout` et `kill()` d'une commande sans perte de l'état du shell, ou relance avec message ; redémarrage du kernel ; pid unique par avis ; `bash.consumed`. Côté Rust : marqueurs du prompt, `prefix_stability.rs`, gel dans `yaRequest`, v1 identique.
- **Vérification** : tests, rejeu sur le provider factice, puis une chauffe sur une tâche qui exige un changement de dossier. **Aucune tâche du smoke bench ne l'exige** (section 1) : il faut d'abord en ajouter une. Chauffe sur autorisation.

### Option C : `cwd=` par appel seulement, sans état

- **Code** : `bash(command, …, cwd: str | None = None)`. Un chemin relatif se résout depuis le cwd du kernel ; il passe à `Popen(cwd=…)` (`bash.py:463`, `:472`, `:1609`) ; un dossier absent donne une erreur claire. Petit ajout, dans un fichier upstream, à lister dans `fork-changes.md`.
- **Prompt** : `:7` gagne `cwd: str | None = None` dans la signature et une clause du type « `cwd` runs this command in that directory ». `:18` est inchangé. **Rupture** du préfixe v2 (réglage figé ou non, au choix).
- **A/B passés** : effet faible, c'est un ajout. Aucune tâche actuelle n'en a besoin (section 1).
- **`[bash-done]` et `"full"`** : aucun effet.
- **Tests** : `cwd` relatif, absolu, absent ; combiné avec `env` et `persistent=True`.
- **Vérification** : CI ; corps envoyé contrôlé à la prochaine chauffe.
- **CDC** : la signature de `:255` est remplie ; le shell principal et le critère « `cd` visible » restent à retirer ou à marquer comme écart (comme A0). Se combine avec A0.

### Option D : le kernel reprend l'état du shell (pas de shell long)

- **Code** : chaque `bash()` reste un processus à part (pid, groupe, `kill()`, `timeout` et avis inchangés). À la fin de la commande, la clôture écrit aussi `pwd` et l'environnement exporté sur le canal de statut. Quand le résultat est lu par un `await` dans l'ordre séquentiel, le kernel applique ce qui a changé (`os.chdir`, `os.environ`). Les commandes en arrière-plan ou sous `gather` ne remontent rien (elles se comportent comme les éphémères du CDC).
- **Effets** : une seule source de vérité, le kernel, donc `read()` et `edit()` restent cohérents avec le shell ; le critère « `cd` visible à l'appel séquentiel suivant » est rempli dans les faits, sans « shell principal » (écart de forme avec le CDC). Risques : une habitude comme `cd build && make` déplace désormais le kernel pour la suite ; un `export` du modèle entre dans `os.environ` du kernel, ce qui interagit avec le filtrage de l'environnement (YA-03, YA-11) ; il faut choisir quels changements remontent (`OLDPWD`, `SHLVL`, `_` à exclure).
- **Prompt** : `:18` change (`cd` et `export` d'un `bash()` attendu remontent ; ceux d'une commande en arrière-plan non). **Rupture** du préfixe v2.
- **A/B passés** : comme B, chaque `bash()` attendu de la v2 change de sémantique.
- **`[bash-done]` et `"full"`** : `"full"` rend la remontée déterministe pour `await bash(…)`. Une commande reprise sur `[bash-done]` ne remonte rien, à moins d'une règle en plus.
- **Tests** : remontée après `await`, pas de remontée en arrière-plan ni sous `gather`, liste d'exclusion, `cd` dans une commande en échec, interaction avec `env=`.
- **Vérification** : comme B, avec une tâche qui exige un changement de dossier.

### Tableau

| | Code | Prompt v2 | Rupture | Critère « `cd` visible » | Risque sur les A/B passés | `[bash-done]` et `"full"` |
|---|---|---|---|---|---|---|
| A0 | aucun | inchangé | non | retiré du CDC | aucun | aucun effet |
| A1 | retrait d'`env`, de `persistent` ou de leur doc | `:7`, `:8`, `:18` | oui | retiré du CDC | faible | doublon de la notification à trancher |
| B | important (shell long, éphémères, pids) | `:7`, `:18` | oui | rempli | fort (sémantique de chaque `bash()`) | dépend de l'`await` ; pids des avis |
| C | petit (`cwd=`) | `:7` | oui | retiré du CDC | faible | aucun effet |
| D | moyen (remontée d'état) | `:18` | oui | rempli dans les faits, écart de forme | fort | remontée liée à l'`await` |

## Points à trancher par Yusuf

1. Option retenue (combinaisons possibles : A0 + C, notamment).
2. Pour toute option qui change le prompt : réglage figé dans `yaRequest` (sessions neuves seulement) ou rupture acceptée pour les sessions v2 existantes.
3. Pour A0, A1 ou C : texte de l'entrée au journal du CDC qui retire ou marque comme écart le shell principal et son critère.
4. Pour B ou D : ajout d'une tâche du smoke bench qui exige un changement de dossier, avant toute chauffe.

## Autre changement de cette PR

`docs/fork-changes.md` : section « Instabilité probable : `herdr::tests::the_seq_never_restarts_below_a_used_value` (YA-09) » (run 44288407647, course de temps sur la milliseconde, réussi à la relance du 18/11/2026).
