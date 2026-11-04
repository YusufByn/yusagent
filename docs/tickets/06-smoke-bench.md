# Ticket 06 : smoke bench local (runner, format de tâche, rapport)

Ticket CDC : YA-14 (partie smoke bench). Base : banc de capture (`bench/tools/capture/`), journal d'usage du T04 (`<id>.usage.ndjson`, sémantique dans `docs/usage.md`), réglages du T05 (`masking.enabled`).

## Objectif

Pouvoir lancer `make smoke` sur un binaire de branche avec un vrai modèle (« OAuth abonnement pour provider »), en local sur macOS, et obtenir un rapport horodaté qui dit, par tâche : résolue ou non, tokens par catégorie, coût théorique, tours, taux de cache. Puis comparer deux rapports.

Ce ticket livre l'outillage et 2 tâches d'exemple. Les 8 autres tâches et la première baseline font l'objet du ticket 07.

**Aucun appel à un vrai modèle dans ce ticket** (règle 6 de `CLAUDE.md`) : tout se valide contre le serveur de capture avec un scénario qui résout les tâches d'exemple. Le premier run réel est lancé par Yusuf.

## Hors périmètre

- Harbor, Terminal-Bench, SWE-bench (banc complet YA-14, ticket ultérieur). Aucun Docker.
- Toute modification du harness (`crates/`, `prime-agent-runtime/`). Si un point bloque sans modifier le harness, s'arrêter et le signaler.
- CI : le smoke bench reste local jusqu'à la v2 (décision du 28/10/2026).

## Provider, modèles, effort (décision du 04/11/2026)

- Provider : ChatGPT en « OAuth abonnement pour provider » (`openai-codex`, API `openai-codex-responses`).
- Modèles du banc : GPT-6.1 Sol, GPT-6.1 Terra, Luna. Astra reste disponible dans le harness mais n'est pas utilisé au banc.
- **À vérifier d'abord** : identifiants exacts. Dans `crates/pa-ai/src/models_generated.rs`, le provider `openai-codex` ne liste que `gpt-5.1` (vers la ligne 588) ; les modèles GPT-6.x n'y apparaissent que sous OpenRouter. Vérifier si le provider Codex accepte un identifiant hors liste (run contre le serveur de capture, `chemin:ligne`). Si non : s'arrêter et signaler, ne pas modifier la couche providers (un ajout de modèles viendra d'un cherry-pick upstream ou d'un ticket dédié).
- Effort de raisonnement fixé à **high** pour tous les runs du banc, noté dans le rapport.

## Format d'une tâche

```
bench/smoke/tasks/<id>/
  task.toml       # voir ci-dessous
  prompt.md       # énoncé donné à l'agent (style issue : symptôme, pas de solution)
  hidden/         # tests ajoutés après la phase agent, jamais visibles par l'agent
```

`task.toml` :

```toml
id = "py-fastapi-001"
lang = "python"                 # python | node
source = { git = "<url>", commit = "<sha>", patch = "break.patch" }   # ou dossier local `repo/`
setup = "uv sync"               # phase préparation, réseau autorisé
test = "uv run pytest -q tests/test_x.py"
fail_to_pass = ["tests/test_x.py::test_bug"]
pass_to_pass = ["tests/test_x.py::test_ok"]
timeout_s = 1800                # proposition, à recaler sur la baseline
tags = ["multi-fichiers"]       # dont "long" (> 10 tours attendus) et "difficile"
```

Dépôts de tâche : téléchargés en préparation dans le cache (`~/.cache/yusagent-smoke`), pas versionnés dans yusAgent. Seuls `task.toml`, `prompt.md`, `hidden/` et les patchs sont dans le dépôt.

## Runner

Nouveau dossier `bench/smoke/` (Python, bibliothèque standard ; tiktoken interdit ici, on ne compte que l'usage renvoyé par le provider).

1. **Préparation** (réseau autorisé, une fois par tâche et par binaire) : clone ou copie, patch, `setup`, vérification que `fail_to_pass` échoue et `pass_to_pass` passe sur l'état cassé (sinon la tâche est invalide, arrêt). Un appel de chauffe au modèle pour rafraîchir le jeton OAuth avant de couper le réseau.
2. **Phase agent** (réseau coupé) : copie fraîche du projet, binaire lancé comme dans `run.py` (`-p --mode json --provider <p> --model <m>`), sous `sandbox-exec` avec un profil qui n'autorise que localhost.
   - Accès au modèle : `HTTPS_PROXY=http://127.0.0.1:<port>` vers un proxy CONNECT lancé hors sandbox, qui n'accepte que les hôtes de `bench/smoke/hosts/<provider>.txt` et journalise chaque CONNECT (accepté ou refusé, hôte seul). Contenu de `hosts/openai-codex.txt` établi à partir du journal du run de chauffe (endpoint du modèle et rafraîchissement du jeton), pas de mémoire.
   - **À vérifier d'abord** : que le client HTTP du harness respecte `HTTPS_PROXY` (pas de `.no_proxy()` trouvé dans `crates/pa-ai/src`, à confirmer par un run contre le serveur de capture avec `chemin:ligne`). Sinon, repli : `baseUrl` du provider pointé vers un reverse proxy local (`resolve_codex_url` accepte une base personnalisée, `crates/pa-ai/src/providers/openai_codex_responses/request.rs`). Ne pas modifier la couche providers.
   - Le kernel n'hérite pas de `HTTPS_PROXY` (filtrage T02) : il n'a aucun réseau. Le noter dans le rapport (variable vue ou non dans le kernel, nom seul).
   - Dossier agent dédié et isolé (`PRIME_AGENT_CODING_AGENT_DIR=~/.cache/yusagent-smoke/agent`), login fait une fois par Yusuf dans ce dossier. Jamais de copie de `auth.json` dans un rapport, un log ou le dépôt.
   - Variables fixées : `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PI_CACHE_RETENTION` explicite. Effort high (voir plus haut).
3. **Notation** (réseau coupé) : copie de `hidden/` dans le workspace de l'agent, `test` lancé avec timeout. Résolue = tous les `fail_to_pass` et `pass_to_pass` passent. Statuts distincts : `resolved`, `failed`, `timeout`, `agent_error`, `harness_error`.

## Mesures par tâche

Depuis `<id>.usage.ndjson` et le JSONL de session, sans rien recalculer qui existe déjà :

- tokens : `input`, `output`, `cacheRead`, `cacheWrite` (5 min / 1 h), `reasoning` ;
- appels modèle par type (boucle, sous-agent, compaction), tours ;
- taux de cache de session et taux médian par requête de continuation (définitions du CDC, YA-04) ;
- invalidations journalisées (champ `invalidation`) et coût d'écriture associé ;
- coût théorique (voir ci-dessous) ;
- durée, TTFT médian ;
- sorties coupées par le plafond de tokens (raison d'arrêt dans le JSONL) ;
- cellules `python_repl` en erreur, par type (`SyntaxError` à part), calculées depuis le JSONL ;
- CONNECT refusés (doit être 0 sauf tentative de l'agent).

Un champ absent est écrit `null`, jamais estimé.

## Coût théorique

`bench/smoke/prices.json` : tarif API public d'OpenAI par modèle (entrée, sortie, lecture cache, écriture cache par TTL), avec URL de la page officielle et date de relevé pour chaque ligne. Coût = tokens × tarif (YA-13, coût sous abonnement). Si le modèle n'a pas de ligne, coût `null` et avertissement. Aucun tarif écrit de mémoire : à remplir depuis la doc officielle au moment du ticket, ou laissé vide pour Yusuf.

## Rapport

`bench/smoke/runs/<horodatage UTC>-<sha court>/` (ignoré par git) :

- `report.md`, `report.json`, `tasks.csv` ;
- en-tête : commit, sha256 du binaire, provider, modèle, effort, réglages non par défaut (`masking`, `rlm`, `cacheRetention`), version de `prices.json`, nombre de runs ;
- totaux : tâches résolues, coût total et par tâche résolue, tokens, tours médians, taux de cache.

`bench/smoke/compare.py <run A> <run B>` : comparaison appariée par tâche (résolu A/B, Δ coût, Δ tokens par catégorie, Δ tours). Signale en tête :
- toute tâche résolue en A et non en B ;
- une hausse de tokens ou de coût sans tâche résolue en plus (CDC YA-14) ; seuil configurable, défaut provisoire 10 % en attendant la mesure du bruit au ticket 07.

Avec plusieurs runs par configuration (`RUNS=n`) : taux de bascule par tâche et écart entre runs, publiés à côté de chaque écart.

## Commandes

```bash
make smoke BINARY=<dossier>/prime-agent PROVIDER=openai-codex MODEL=<id> EFFORT=high [SETTINGS=bench/smoke/settings/<x>.json] [RUNS=1] [TASKS=id1,id2]
python3 -I bench/smoke/compare.py <run A> <run B> --out <fichier>.md
```

Variantes = fichiers de réglages (`settings/default.json`, `settings/masking-on.json`), jamais des branches.

Ajouter à `.github/pull_request_template.md` une case « smoke bench lancé sur ce commit (lien ou chemin du rapport) ».

## Validation sans vrai modèle

Nouveau scénario dans `bench/tools/capture/server.py` (`--scenario smoke-<id>`), au format Responses (chemin `…/codex/responses`), qui résout chaque tâche d'exemple en quelques cellules, et un scénario qui échoue. Attendu : une tâche `resolved`, une `failed`, rapport complet, `compare.py` signale la régression entre les deux, CONNECT refusé journalisé quand le scénario tente `curl https://example.com` depuis `bash()`.

## Tâches d'exemple

Deux tâches minimales pour valider le format (une Python, une Node), dans un dépôt local `repo/` de quelques fichiers. Elles ne comptent pas dans les 10 tâches du smoke bench.

## Critères d'acceptation

- `make smoke` contre le serveur de capture produit le rapport ci-dessus, deux fois de suite avec des totaux de tokens identiques (serveur déterministe).
- Réseau coupé prouvé : CONNECT hors liste refusé, `curl` direct depuis le kernel en échec.
- Aucun secret dans les rapports (test : recherche des valeurs factices de `run.py` dans tout le dossier de run).
- `docs/fork-changes.md` à jour (Makefile et modèle de PR sont des fichiers upstream).
- README `bench/smoke/README.md` : prérequis, login dans le dossier agent dédié, commandes.

## Impact préfixe

Aucun : pas de changement du harness. Les réglages de variante (`masking`) ont l'impact décrit au T05.
