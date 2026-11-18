# Ticket 17 : YA-09, anti-gaspillage (phase 1 : constat)

Demande de Yusuf du 18/11/2026. **Phase 1 : constat seulement.** Branche `ya-17-anti-gaspillage`, depuis `main` à `057534a36`. Aucun run de modèle, aucune chauffe, rien compilé. Prompt, tools et code livré inchangés : **préfixe inchangé**. La PR contient ce ticket, l'outil `bench/smoke/tools/waste.py`, ses tests `bench/smoke/tests/test_waste.py` et deux lignes du tableau des fichiers de `bench/smoke/README.md`. Aucun fichier upstream touché. Pas de merge sans « merge » de Yusuf.

Ce ticket ne propose ni texte de prompt ni plan d'A/B : ils seront écrits hors de Claude Code, après lecture du constat.

## Décision de Yusuf (18/11/2026) : clos sans changement du prompt

Raisons :

- **Gaspillage mesuré sous le seuil du banc** : 3,4 % du coût à cache théorique, 6,6 % au plus selon les seuils (section 4). Le seuil de détection du banc est d'environ 10 % (constat du 14/11/2026, `docs/yusAgent-cdc.md:58`).
- **Règle 1 déjà en place** : elle est dans le prompt v2 (`ya_prompt_v2/core.md:19`), mot pour mot comme chez Unharnessed (`core.md:16`, `53bdd8784`).
- **Règles 2 et 3 absentes chez Unharnessed** : les ajouter serait un écart à Unharnessed, qui exigerait un A/B que ce banc ne peut pas trancher.

Conséquences :

- Prompt, tools et code livré inchangés, préfixe inchangé.
- Le détecteur `bench/smoke/tools/waste.py` est gardé, à relancer sur les traces de Terminal-Bench.
- CDC : ligne du 18/11/2026 au journal. Elle précise l'effet sur la puce « Principes anti-gaspillage » de YA-09 et sur la ligne du 31/10/2026 : règle 1 en place, autres principes non ajoutés, à rejuger à Terminal-Bench.

## Contexte

Repris du document de recherche (levier 6), **non vérifié par nous**. Une étude sur les agents de code relève trois comportements de gaspillage, présents dans 79 à 98 % des tâches et responsables de jusqu'à 22,75 % du coût :

1. relire un fichier déjà lu et non modifié depuis ;
2. écrire à la suite des scripts presque identiques ;
3. relancer des tests sans aucun changement entre-temps.

Sept principes courts, écrits à la main dans le prompt, y baissent le coût de 7,88 à 41,73 % dans 6 configurations sur 8.

Le CDC prévoit déjà ces règles : journal du 31/10/2026, « principes anti-gaspillage dans le prompt » (`docs/yusAgent-cdc.md:20`), et puce de YA-09 « Principes anti-gaspillage dans le prompt système (6 ou 7 règles courtes et générales) » (`docs/yusAgent-cdc.md:346`).

**Condition de Yusuf (18/11/2026)** : ne tester ces règles que si elles ne rendent pas inutile le travail du modèle et ne baissent pas la qualité. Relire après une modification et relancer les tests après un edit restent attendus. Ce n'est pas du gaspillage.

**Seuil de détection du banc** (constat du 14/11/2026, `docs/yusAgent-cdc.md:58`) : le smoke bench ne distingue pas un effet de moins de 10 % environ du coût à cache théorique.

## Questions

1. Unharnessed a-t-il déjà tranché (règle du 15/11 : dans ce cas, on reprend tel quel) ?
2. Que dit notre prompt v2 sur ces trois comportements ?
3. Quelle est leur fréquence de base sur nos traces v2, et quelle part des tokens d'entrée et du coût à cache théorique pèsent-ils ?
4. Le gaspillage mesuré dépasse-t-il environ 10 % du coût, seuil de détection du banc ?

## Résultats en bref

- **Unharnessed** : une règle d'une ligne contre la relecture (comportement 1). Rien contre les scripts presque identiques (2). Rien dans le prompt contre les tests relancés (3) : seul le harness, en mode autonome, ne relance pas une *gate* sur un workspace inchangé.
- **Notre v2** : la même règle, mot pour mot (`core.md:19`). Rien sur 2 et 3.
- **Fréquence** (66 sessions v2, 847 cellules) : 33 runs de tâche sur 66 ont au moins un cas. On compte 47 relectures redondantes, 5 cellules presque identiques et 2 tests relancés sans changement, soit 42 cellules en cause.
- **Poids estimé** (borne haute) : **2,2 % des tokens d'entrée, 3,4 % du coût à cache théorique**. Avec les seuils les plus larges essayés, 6,1 % et 6,6 %.
- **Conclusion** : non, le gaspillage mesuré ne dépasse pas 10 % du coût (section 5).

## 1. Unharnessed

Clone jetable hors du dépôt (dossier temporaire de la session), lecture seule, rien exécuté ni compilé. **SHA `53bdd8784617fa9f2171abababd0b0f95618bfeb`** (17/11/2026 22:56 UTC, « The model additions follow models.dev »). C'est le dernier commit de `main` au moment du clone, le même que pour les tickets 15 et 16.

Fichiers lus : le prompt système statique `crates/uh-core/src/prompts/layers/core.md` (20 lignes, seule couche statique : `layers.rs:11`), son assemblage `crates/uh-core/src/prompts/system_prompt.rs`, la description du tool `crates/uh-core/src/tools/ipython.rs` et `README.md`. Recherche : `grep -n -i "again\|re-read\|reread\|re-run\|rerun\|repeat\|redundan\|duplicat\|already\|same file\|named variable\|waste\|instead of reading\|cache"` sur ces fichiers, puis lecture des passages trouvés.

| Comportement | Règle trouvée (mot pour mot) | Où |
|---|---|---|
| 1. Relecture | « Store what you read or compute in named variables instead of reading it again. » | `crates/uh-core/src/prompts/layers/core.md:16` ; recopiée dans `README.md:111` (section « Rules », `README.md:106-115`, qui reprend le prompt) |
| 2. Scripts presque identiques | **aucune** | — |
| 3. Test relancé sans changement | **aucune dans le prompt** | — |

À côté, sans viser l'un des trois comportements :

- `core.md:13` : « Keep in mind that each tool call is a round trip: steps that do not need a result you have not seen yet can go in one call. » Cette règle porte sur le regroupement des étapes, pas sur la répétition.
- `README.md:221`, mode autonome (section `README.md:201-221`, option `--autonomous-gate`) : « The harness snapshots the git worktree before each gate run: a gate that failed is not re-run over an unchanged workspace, so a model that changes nothing can't burn retries. » C'est un **mécanisme du harness** pour les *gates* que l'utilisateur fixe, pas une règle du prompt sur les tests que le modèle lance lui-même.
- Description du tool : « Run Python code in the persistent REPL. » (`crates/uh-core/src/tools/ipython.rs:317-319`). Rien sur ces comportements.

Origine de la règle de `core.md:16` : commit `14ba49745` (01/10/2026, « unharnessed step 9: minimal system prompt and the Q4 depth leftovers »). Son message dit « The user approved the draft as is ». Elle condense deux règles de la couche `usage` d'upstream, que ce commit retire chez eux. Chez nous, elles sont encore dans le prompt v1 (`git show upstream/main:crates/pa-core/src/prompts/layers/usage.md`, identique à `crates/pa-core/src/prompts/layers/usage.md`) :

- `usage.md:10` : « Agents use Python for reading files and searching in them — it gives them reusable variables they can slice, filter, and act on without re-reading; using Python variables to find or produce, and to save the strings used in `edit` is also encouraged. »
- `usage.md:11` : « Agents always assign read/search results to named variables so they can revisit them later. »

Les couches upstream `core.md` et `opinionated.md` n'ont rien sur ces trois comportements (même recherche).

**A-t-il tranché ?** Oui pour la relecture, avec cette seule règle. Non pour les scripts presque identiques. Non dans le prompt pour les tests relancés : seul le harness s'en occupe, et seulement pour les gates du mode autonome. Aucun des « sept principes » de l'étude n'y figure au-delà de la règle de `core.md:16`.

## 2. Notre prompt v2

Préfixe statique : `crates/pa-core/src/prompts/ya_prompt_v2/core.md`, adapté par `ya_prompt_v2.rs` à la configuration des tools. On a lu aussi la description du tool `python_repl` et la queue dynamique, telles qu'envoyées. Contrôle sur un corps capturé : `bench/smoke/runs/ab-20261117T131130Z/arms/20261117T131130Z-bf88ebc-gpt-6-luna/probe/capture/0001.body.json` (binaire `bf88ebc`, `instructions` de 4 970 caractères). Recherche dans `instructions` : `again|re-?read|re-?run|repeat|already|test|variable|duplicate`.

| Comportement | Règle (mot pour mot) | Où |
|---|---|---|
| 1. Relecture | « Store what you read or compute in named variables instead of reading it again. » | `ya_prompt_v2/core.md:19`, identique à Unharnessed `core.md:16` (reprise de `fe2cf2815` par `02ed3b4fc`) |
| 2. Scripts presque identiques | **aucune** | — |
| 3. Test relancé sans changement | **aucune** | — |

Passages voisins :

- `core.md:16` : « Each tool call is a round trip. Put in one call every step that does not need a result you have not seen yet: independent reads, searches and commands together (several `bash()` under `asyncio.gather`), and an edit together with the command that tests it. » Cette règle demande de lancer le test **avec** l'edit. Elle n'interdit pas de le relancer sans changement.
- `core.md:20` : « Create and edit files with `edit()`, with `old_str` taken from text you have read. » Elle pousse à lire avant d'éditer. Or 26 des 35 cellules à relecture redondante sont dans la cellule de l'`edit()` ou juste avant (section 4).
- `core.md:26` : « … no speculative fallbacks, compatibility shims, defensive code or extra tests. » Elle porte sur les tests **écrits**, pas sur les tests relancés.
- Description du tool : « Run Python code in the persistent REPL. Top-level `await` works; variables persist across calls. » (`crates/pa-core/src/prompts/ya_prompt_v2.rs:104-105`).
- Queue dynamique (paquets, contexte du projet, environnement, skills) : rien sur ces comportements dans le corps capturé.

## 3. Détecteur `bench/smoke/tools/waste.py`

Lecture seule, bibliothèque standard plus `bench/smoke/metrics.py` (prix, taille des prompts, journaux d'usage). tiktoken n'est pas une dépendance du dépôt : `uv` l'installe dans un environnement jetable. Le comptage est **heuristique** : expressions régulières et AST sur le code des cellules.

### Unité et ordre

- Un run de tâche = `<run>/tasks/<tâche>/run-N/sessions/*.jsonl` (session principale et sous-agents).
- Cellules : appels du tool `python_repl`. Les appels `apply_patch` comptent comme écritures.
- Les opérations d'une cellule sont lues dans l'ordre du code (AST Python, `await` de premier niveau accepté), puis d'une cellule à la suivante.
- Une cellule en erreur (`details.status == "error"`) garde ses écritures, par prudence. Ses lectures et ses tests ne comptent pas comme faits, faute de savoir où elle s'est arrêtée.
- Une cellule dont la sortie est tronquée (`[... output truncated:`, `crates/pa-core/src/tools/ya_output_cap.rs:71-72`) ne compte pas ses lectures.
- Une cellule qui ne se parse pas n'a rien exécuté : elle est ignorée.

### Écritures

Elles remettent à zéro l'état des lectures du fichier, et celui de tous les tests.

- **Écritures repérées** : `edit()` et `edit.run()`, `apply_patch`, `open(path, …)` en mode `w`, `a`, `x` ou `+`, `.write_text()` et `.write_bytes()`. Dans un `bash()` : redirection `>` ou `>>`, `tee`, `sed -i`, `cp`, `mv`, `rm`, `touch`, `mkdir`, `ln`.
- **Écriture de tout fichier** : un chemin non résolu (variable non liée à une constante, f-string), `git checkout|restore|stash|apply|reset|…`, `patch`, un formateur (`--write`, `--fix`, `black`, `ruff format`, …), ou un script en ligne (`python -c`, heredoc, `node -e`) qui contient `write`.
- **Écriture du workspace sans fichier source** : installation de dépendances (`npm install`, `pip install`, `uv sync`, …) et build (`npm run build`, `tsc`, `make` hors `make test`). Elle efface l'état des tests, pas celui des lectures.
- `/tmp/`, `/dev/`, `/var/folders/` et `/private/` ne sont pas le workspace.

### 1. Relecture redondante

- **Lectures** :
  - `read(path, offset, limit)` ;
  - `open(path).read()` et `Path(path).read_text()` ;
  - dans un `bash()` : `cat`, `nl`, `sed -n 'a,bp'` (plusieurs plages possibles), `head [-n] N`, `tail -n N` et `tail -n +K`, seuls ou en tête de tube suivis de `head`, `tail` ou `sed -n`. `cat f | grep …` est une recherche, pas une lecture.
- **Chemins** : constante, nom lié à une constante dans la cellule, ou variable d'une boucle `for` sur une liste de constantes.
- **Plages** : en lignes. Un fichier entier vaut les lignes 1 à 2 000, soit le défaut de `read(limit=2000)`. `tail -n N` vaut les N dernières lignes : il n'est comparé qu'aux autres `tail` et aux lectures entières.
- **Lecture vue** : une lecture Python ne compte, comme première lecture ou comme relecture, que si sa valeur est **affichée en entier**. Trois cas : `print(read(…))`, une variable passée telle quelle à `print`, ou la dernière expression de la cellule. Avec `s = read(p); print(s[i:j])`, le modèle n'a vu qu'une tranche. Les lectures faites par `bash()` comptent toujours comme vues.
- **Cas compté** : une lecture vue dont au moins **50 %** des lignes (`REREAD_MIN_COVERED`) ont déjà été lues et vues depuis la dernière écriture du fichier. Justification : en dessous, la lecture montre surtout du neuf. Une lecture qui prolonge ou chevauche un peu la précédente n'est pas une relecture.

### 2. Cellules presque identiques

- **Mesure** : le code de chaque cellule est découpé en mots (`\w+` ou un signe isolé). On calcule le ratio `difflib.SequenceMatcher` (2 × éléments communs / total, dans l'ordre) avec chacune des **3** cellules `python_repl` précédentes de la session, et on garde la meilleure.
- **Cas compté** : ratio ≥ **0,9** (`SIMILAR_RATIO`), les deux cellules ayant au moins **20 mots** (`SIMILAR_MIN_TOKENS`).
- **Justification** : au plus un mot sur dix change, ce qui suffit pour une même commande avec un argument retouché. En dessous de 20 mots, une cellule est un seul appel court : `print(read('a.py', 1, 35))` fait 18 mots, et 2 ou 3 mots y changent toute la cible. La mesure est simple et sans dépendance ; la fenêtre de 3 cellules couvre « consécutives ou proches ».
- **Mesure écartée** : un premier essai au caractère, avec le même seuil, a été écarté après relecture des cas. Il rapprochait des cellules courtes qui lisent deux fichiers différents.
- **Écartées** (colonne « écartées », hors gaspillage) :
  - la précédente a échoué (erreur Python, `exit_code` ou `exit` non nul, `command not found` ou `Traceback` dans sa sortie) : la suivante corrige ;
  - le workspace a été écrit depuis la précédente : relancer après un changement est attendu.

### 3. Test relancé sans changement

- **Commandes de test** : dans un `bash()`, une étape de commande qui contient `pytest`, `py.test`, `tox`, `nox`, `jest`, `vitest`, `mocha`, `ava`, `python -m pytest` ou `python -m unittest`, `npm`/`pnpm`/`yarn`/`bun` `[run] test[:…]`, `go test`, `cargo test`, `node --test`, `make test` ou `make check`.
- **Clé** : la suite de ces étapes, sans leurs redirections, espaces réduits. `pytest -q 2>&1 | tail -30` et `pytest -q > /tmp/x` ont la même clé.
- **Cas compté** : un lancement dont la clé est déjà sortie dans la session, sans écriture du workspace entre les deux.

### Tokens et coût (estimations)

- **Tokenizer supposé** : `o200k_base`. Celui de `gpt-6-luna` n'est pas publié.
- **Cellule en cause** : une cellule où l'on trouve au moins un cas, dans l'union des trois comportements (ventilée aussi par comportement). On compte son code et le texte de son résultat. C'est une **borne haute du gain** : la cellule entière compte, même si une partie seulement est redondante.
- **Tokens d'entrée** : (code + résultat) × nombre d'appels au modèle qui suivent dans la session, puisqu'ils les portent tous dans leur prompt. On les rapporte au prompt total des appels de la boucle (`input + cacheRead + cacheWrite`, journal d'usage).
- **Coût à cache théorique** (même modèle que `metrics.theoretical_cache`) :
  - l'appel qui a écrit la cellule paie sa sortie au tarif de sortie. On prend la sortie rapportée par le provider, raisonnement compris, si l'appel ne contient que cette cellule, sinon le code estimé ;
  - l'appel suivant paie le résultat au tarif d'entrée et le code au tarif de lecture du cache ;
  - chaque appel ensuite paie code et résultat au tarif de lecture du cache ;
  - tarifs : ceux de `bench/smoke/prices.json`, palier de chaque appel (`metrics.price_row`). La part est rapportée au `cost.theoretical_total` du `report.json` du run.
- Le numérateur est une estimation `o200k_base`, le dénominateur un compte du provider : **la part est une estimation**.

### Limites connues

**Relecture redondante**

- **Faux positifs** :
  - Relecture courte juste avant un `edit()` pour recopier `old_str` (ce que demande `core.md:20`). Elle est comptée : sur les 35 cellules à relecture, 26 contiennent un `edit(` ou précèdent immédiatement une cellule qui en contient un (recherche de la sous-chaîne `edit(`). Sur les 47 plages relues, 31 font 40 lignes ou moins.
  - Fichier modifié par un programme non repéré : script lancé par `bash()`, générateur, formateur lancé par un script. La lecture suivante est alors comptée à tort.
  - Lecture par `bash()` dont la sortie n'est pas affichée : elle compte quand même comme vue.
- **Faux négatifs** :
  - chemins non résolus : f-string, chemin calculé, `glob`, `pathlib.Path(…)` écrit avec le module ;
  - vues non reconnues : `awk`, `grep -A/-B/-C`, `rg -C`, `git show`, `git diff` ;
  - lectures Python affichées par tranche (`print(s[i:j])`) : jamais comptées ;
  - longueur réelle du fichier inconnue (un fichier entier vaut 2 000 lignes), et `tail` n'est pas comparé aux plages.

**Cellules presque identiques**

- **Faux positifs** : même habillage sur une autre cible. Sur les 5 cas relus à la main, **3** affichent un autre fichier ou une autre plage avec la même forme de code :
  - `20261117T132735Z` node-tough-cookie-630, cellules 8 et 10 ;
  - `20261118T104154Z` node-tough-cookie-630, cellule 8.

  Les **2** autres refont la même commande parce que la première avait oublié `await` (sortie : un `BashHandle` encore en cours) : `20261116T125706Z` py-tldextract-383, cellule 10 ; `20261117T135654Z` node-koa-2029, cellule 2. Deux vérifications différentes de même forme (`tox -e lint` puis `tox -e typecheck`) seraient aussi comptées. Ici, elles sont écartées parce que la précédente avait échoué.
- **Faux négatifs** : script réécrit plus de 3 cellules plus loin, ou avec plus de 10 % de mots changés ; cellules de moins de 20 mots.

**Test relancé sans changement**

- **Faux positifs** :
  - Test dans une chaîne `a && test` où `a` a échoué : le test n'a pas tourné. C'est **1 des 2 cas** : `20261117T131817Z` py-tldextract-368, cellule 8 (`uv run ruff format . && uv run tox -e py310`, échec de `uv run` hors ligne), puis cellule 9.
  - Test tué par un délai puis relancé avec un délai plus long.
  - Test instable relancé exprès.
  - Écriture faite par un programme non repéré.
- **Faux négatifs** :
  - même test écrit autrement (`pytest -k a` contre `pytest tests/x.py::a`, `npm test` contre `npx vitest run`) ;
  - lanceur hors de la liste (`node test.js`, `./run_tests.sh`) ;
  - écriture repérée qui ne change rien (`mkdir -p` d'un dossier existant).
- **L'autre cas est vrai** : `20261116T130753Z` py-tldextract-383, cellule 14. Elle relance `tox --parallel …` alors que celui de la cellule 13 tournait encore (instantané à 30 s, binaire `318f3e1` avec `bashWaitSeconds: 30`).

**Tokens et coût**

- La borne haute ne compte pas ce que le modèle aurait fait à la place, ni la latence de l'aller-retour.
- Sessions compactées : le calcul suppose un contexte qui ne fait que croître. Aucune des 66 n'a de compaction ni de sous-agent (`compactions` et `subagents.sessions` à 0 dans `report.json`).

### Tests

`bench/smoke/tests/test_waste.py`, 25 tests sans modèle ni tiktoken. Ils couvrent :

- le repérage des lectures, des écritures et des tests dans le shell ;
- la règle d'affichage, les boucles et les noms liés ;
- l'ordre dans la cellule ;
- les cellules en erreur et tronquées, `apply_patch` ;
- les seuils de relecture et de similarité, et les exclusions ;
- le calcul du coût ;
- un run complet sur fichiers temporaires.

```bash
python3 -I -m unittest discover -s bench/smoke/tests -p 'test_waste.py' -v   # 25 tests OK
python3 -I -m unittest discover -s bench/smoke/tests                         # 73 tests OK
```

Le banc n'est pas lancé par `ci.yml` (ses tests sont locaux, `bench/smoke/README.md:48`).

## 4. Fréquence de base sur les traces v2

Traces du prompt v2 seulement. Le filtre `--prompt-version 2` lit `yaRequest.promptVersion` dans l'en-tête de chaque session. Il retire le bras B (v1) de `ab-20261116T124625Z` et les runs `80e20ea` du 17/11, qui sont en v1.

| Source | Binaire | Sessions | Cellules |
|---|---|---|---|
| `ab-20261116T124625Z`, bras A (v2 ; B est en v1) | `318f3e1` | 20 | 274 |
| `ab-20261117T131130Z`, bras A et B (les deux en v2) | `bf88ebc` | 40 | 545 |
| Chauffes v2 du 17/11 : `20261117T075548Z`, `20261117T075921Z` (py-silence-001 et 002) | `713ba4b` | 2 | 4 |
| Chauffes v2 du 17/11 : `20261117T093127Z`, `20261117T095751Z` (py-silence-001 et 002) | `a7d97ec` | 2 | 4 |
| Chauffe v2 du 18/11 : `20261118T104144Z` (node-tough-cookie-630) | `bf88ebc` | 1 | 3 |
| Chauffe v2 du 18/11 : `20261118T104154Z` (node-tough-cookie-630, ticket 15) | `4fd1130` | 1 | 17 |
| **Total** | 5 binaires | **66** | **847** |

Une session par run de tâche, aucun sous-agent. Modèle `gpt-6-luna`, effort high. Deux runs sont en échec, et gardés : `ab-20261117T131130Z`, passe 4, py-tldextract-368, bras B (`20261117T140347Z`) ; et la chauffe `20261118T104144Z` (node-tough-cookie-630). Les 64 autres sont résolus (`status` dans `report.json`).

### Par tâche

Sortie de la commande ci-dessous, le 18/11/2026. Lecture des colonnes :

- **Runs touchés** : runs de tâche avec au moins un cas.
- **Relectures**, **Presque identiques**, **Tests relancés** : nombre de cas.
- **(écartées)** : paires similaires retirées du gaspillage (précédente en échec ou écriture entre les deux).
- **Cellules en cause** : union des trois comportements. Une cellule peut porter plusieurs relectures.
- **Parts** : estimations, borne haute.

| Tâche | Runs | Runs touchés | Cellules | Relectures | Presque identiques (écartées) | Tests relancés | Cellules en cause | Part tokens d'entrée | Part coût théorique |
|---|---|---|---|---|---|---|---|---|---|
| node-koa-2029 | 12 | 9 | 113 | 14 | 1 (3) | 0 | 9 | 1.7 % | 3.3 % |
| node-tough-cookie-630 | 14 | 8 | 222 | 11 | 3 (5) | 0 | 11 | 2.8 % | 3.5 % |
| py-langextract-584 | 12 | 3 | 165 | 4 | 0 (0) | 0 | 4 | 1.1 % | 1.5 % |
| py-silence-001 | 2 | 0 | 5 | 0 | 0 (0) | 0 | 0 | 0.0 % | 0.0 % |
| py-silence-002 | 2 | 0 | 3 | 0 | 0 (0) | 0 | 0 | 0.0 % | 0.0 % |
| py-tldextract-368 | 12 | 5 | 129 | 8 | 0 (5) | 1 | 7 | 2.1 % | 3.0 % |
| py-tldextract-383 | 12 | 8 | 210 | 10 | 1 (1) | 1 | 11 | 2.4 % | 4.8 % |
| **Total** | 66 | 33 | 847 | 47 | 5 (14) | 2 | 42 | 2.2 % | 3.4 % |

### Par comportement

| Comportement (total) | Runs touchés | Cellules | Part tokens d'entrée | Part coût théorique |
|---|---|---|---|---|
| Relectures | 28 | 35 | 2.1 % | 3.2 % |
| Presque identiques | 4 | 5 | 0.1 % | 0.1 % |
| Tests relancés | 2 | 2 | 0.0 % | 0.0 % |
| Union | 33 | 42 | 2.2 % | 3.4 % |

En valeur : 195 320 tokens d'entrée estimés sur 8 949 254, et 0,0079 $ sur 0,2318 $ de coût à cache théorique.

Par source, les mêmes parts sont :

| Source | Part tokens d'entrée | Part coût théorique |
|---|---|---|
| `ab-20261116T124625Z` A | 1,2 % | 2,2 % |
| `ab-20261117T131130Z` | 2,7 % | 4,1 % |
| Chauffes | 1,2 % | 1,4 % |

### Après relecture à la main

Tous les cas ont été relus à la main, avec la ou les cellules antérieures en cause (`--json`).

- **Tests relancés** : 1 vrai sur 2.
- **Presque identiques** : 2 sur 5 refont la même commande ; les 3 autres sont des faux positifs (section 3).
- **Relectures** : chaque cas affiche à nouveau une plage déjà affichée depuis la dernière écriture. Aucun faux positif évident n'a été trouvé, mais 26 cellules sur 35 sont liées à un `edit()` (section 3, limites).

Retirer les faux positifs relus ne change la part de coût que dans la deuxième décimale : presque identiques et tests relancés pèsent ensemble 0,1 %.

### Sensibilité aux seuils

Même commande, avec `--reread-covered` et `--similar-ratio` :

| Relecture (part déjà lue) | Similarité | Cellules en cause | Part tokens d'entrée | Part coût théorique |
|---|---|---|---|---|
| 0,5 (défaut) | 0,9 (défaut) | 42 | 2,2 % | 3,4 % |
| 1,0 (entièrement déjà lue) | 0,9 | 38 | 1,9 % | 2,8 % |
| 0,01 (tout recouvrement) | 0,9 | 59 | 4,5 % | 5,2 % |
| 0,5 | 0,8 | 56 | 4,3 % | 5,2 % |
| 0,5 | 0,95 | 37 | 2,1 % | 3,3 % |
| 0,01 | 0,8 | 70 | 6,1 % | 6,6 % |

### Commande pour refaire le comptage

Les runs ne sont pas versionnés (`bench/smoke/.gitignore:1`, `runs/`). Depuis la racine du dépôt, avec ces runs présents dans `bench/smoke/runs/` :

```bash
R=bench/smoke/runs
uv run --no-project --with tiktoken==0.14.0 python -I bench/smoke/tools/waste.py \
  --prompt-version 2 --json /tmp/waste-v2.json \
  $R/ab-20261116T124625Z $R/ab-20261117T131130Z \
  $R/20261117T075548Z-713ba4b-gpt-6-luna $R/20261117T075921Z-713ba4b-gpt-6-luna \
  $R/20261117T093127Z-a7d97ec-gpt-6-luna $R/20261117T095751Z-a7d97ec-gpt-6-luna \
  $R/20261118T104144Z-bf88ebc-gpt-6-luna $R/20261118T104154Z-4fd1130-gpt-6-luna
```

Sensibilité : ajouter `--reread-covered X` et `--similar-ratio X`. Le fichier `--json` donne chaque cas (session, cellule, fichier, plage, cellules antérieures, début du code) pour la relecture à la main.

## 5. Conclusion

**Non : le gaspillage mesuré (3,4 % du coût à cache théorique, 6,6 % avec les seuils les plus larges, borne haute) reste sous le seuil de détection du banc d'environ 10 %.**

## Points ouverts

- Ces chiffres valent pour `gpt-6-luna` et 7 tâches courtes du smoke bench. Ils ne disent rien des tâches longues ni des autres familles de modèles (YA-14).
- Les « 79 à 98 % des tâches » et « jusqu'à 22,75 % du coût » de l'étude ne sont pas vérifiés par nous. Notre définition est plus stricte : lecture vue, relance seulement après écriture, exclusions. Les deux chiffres ne se comparent pas directement.
- Le binaire `318f3e1` attendait `bash()` 30 s (`bashWaitSeconds: 30`). Le seul vrai test relancé vient de là. Depuis le ticket 13, l'attente est complète par défaut en v2.
