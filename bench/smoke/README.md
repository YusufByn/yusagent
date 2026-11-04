# Smoke bench local (T06, YA-14)

Lance le binaire d'une branche sur quelques tâches courtes avec un vrai modèle (« OAuth abonnement pour provider », ChatGPT), en local sur macOS, et écrit un rapport horodaté : par tâche, résolue ou non, tokens par catégorie, coût théorique, tours, taux de cache, transport de chaque requête. `compare.py` compare deux rapports.

Le premier run réel est lancé par Yusuf (CLAUDE.md, règle 6). Tout l'outillage se valide sans modèle, contre le serveur de capture (`--capture`).

## Prérequis

- macOS (`sandbox-exec`), `python3` ≥ 3.11 (bibliothèque standard seule), `uv`, `git`, `curl`, `node` ≥ 22 (tâches Node : `node --test` avec motif et rapport JUnit).
- Un binaire de la branche : artefact de `ya-build-macos-arm64.yml`, extrait avec `models.bundled.json` à côté de `prime-agent` (voir `bench/tools/capture/README.md`). Le catalogue des modèles est ce fichier, son sha256 est noté dans le rapport.
- Cache : `~/.cache/yusagent-smoke` (`--cache` pour le déplacer). Dépôts de tâche, venvs, venv du kernel (`PRIME_AGENT_KERNEL_VENV`, un par binaire), `HOME` isolé. Rien de cela n'est dans le dépôt.

## Login (une fois, par Yusuf)

Le banc utilise un dossier agent dédié : `~/.cache/yusagent-smoke/agent`. Y faire le login ChatGPT une fois, en interactif :

```bash
PRIME_AGENT_CODING_AGENT_DIR=~/.cache/yusagent-smoke/agent HOME=~/.cache/yusagent-smoke/home <dossier>/prime-agent
```

puis le panneau d'authentification, provider « ChatGPT Plus/Pro (Codex Subscription) » (`openai-codex`). Le runner ne lit jamais `auth.json` sauf pour une chose : rechercher ses valeurs dans le dossier de run à la fin (le rapport ne donne que des noms de fichiers et des comptes). Il réécrit `models.json` et `settings.json` de ce dossier à chaque session : ne rien y mettre à la main.

## Hôtes autorisés (une fois par provider)

Le proxy CONNECT n'accepte que les hôtes de `hosts/openai-codex.txt` (rafraîchissement du jeton). La liste s'établit au run de chauffe, pas de mémoire :

```bash
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary <dossier>/prime-agent --model gpt-6.1-sol --record-hosts
```

C'est un appel réel au modèle. Le proxy CONNECT accepte alors tout hôte sur le port 443 et écrit ceux qu'il a vus dans `hosts/openai-codex.txt`, à relire avant de le committer. Un rafraîchissement n'a lieu que si le jeton est près d'expirer : si aucun hôte n'est vu, le fichier reste vide, à refaire plus tard.

## Commandes

```bash
make smoke BINARY=<dossier>/prime-agent PROVIDER=openai-codex MODEL=<id> EFFORT=high [SETTINGS=bench/smoke/settings/<x>.json] [RUNS=1] [TASKS=id1,id2]
python3 -I bench/smoke/compare.py <run A> <run B> --out <fichier>.md
```

- `MODEL` : liste fermée `gpt-6.1-sol`, `gpt-6.1-terra`, `gpt-6-luna`, `gpt-6-astra`. Tout autre id est refusé, même présent au catalogue. Sans `MODEL` : un run par modèle par défaut (`gpt-6.1-sol`, `gpt-6.1-terra`, `gpt-6-luna`) ; `gpt-6-astra` seulement avec `MODEL=gpt-6-astra`.
- `gpt-6.1-terra` est absent de `models.bundled.json` : sa fiche (`models/openai-codex.json`, relevée sur la doc officielle) est déclarée dans le `models.json` du dossier agent. Un modèle absent du catalogue et sans fiche complète est refusé ; le repli du harness sur la fiche du modèle par défaut n'est jamais utilisé. `contextWindow` de Terra : 272 000, provisoire (valeur du catalogue pour les modèles Codex voisins), hypothèse notée dans la fiche et dans l'en-tête du rapport.
- `EFFORT` : `high` au banc (décision du 04/11/2026), noté dans le rapport.
- Variantes : fichiers de réglages (`settings/default.json`, `settings/masking-on.json`), jamais des branches.
- Validation sans modèle : `make smoke BINARY=... CAPTURE=1 [CAPTURE_FAIL=node-example-001]`. Options du runner en plus : `--capture-reject-model <id>` (le serveur refuse l'id au run de chauffe), `--capture-no-websocket` (le serveur refuse le WebSocket : repli SSE).

## Déroulé

1. **Contrôles** : modèle de la liste fermée, au catalogue ou déclaré par une fiche complète ; login présent ; liste d'hôtes établie.
2. **Préparation** (réseau autorisé, une fois par binaire ou par version de tâche) : venv du kernel contre le serveur de capture ; pour chaque tâche, copie ou clone, patch, `setup`, puis vérification sous sandbox que `fail_to_pass` échoue et `pass_to_pass` passe sur l'état cassé (sinon la tâche est invalide, arrêt).
3. **Sonde réseau et isolation** (profil de la phase agent, serveur de capture, aucun modèle) : `curl` direct depuis `bash()` en échec, `curl https://example.com` par le proxy CONNECT refusé et journalisé, noms des variables de proxy vues par le kernel et par `bash()`, lecture d'un test caché et de la copie pristine par chemin absolu (Python et `bash()`) en échec. Les scénarios des tâches d'exemple refont ces lectures pendant la phase agent.
4. **Run de chauffe** (sandbox, même accès que la phase agent) : le modèle doit répondre. Un id faux ou un jeton refusé fait échouer la préparation ici, pas le run mesuré. Rafraîchit le jeton si besoin.
5. **Phase agent** (sandbox `sandbox-agent.sb` : localhost seulement, et ni lecture ni écriture de `bench/smoke/` ni du cache des tâches hors workspace, donc ni `hidden/` ni la copie pristine) : copie fraîche du projet, `prime-agent -p --mode json --provider openai-codex --model <id> --thinking <effort>`, énoncé de `prompt.md`.
6. **Notation** (sandbox) : `hidden/` copié dans le workspace, `test` lancé avec la variable `SMOKE_JUNIT` (rapport JUnit). Résolue = tous les `fail_to_pass` et `pass_to_pass` passent. Statuts : `resolved`, `failed`, `timeout`, `agent_error`, `harness_error`.

Variables fixées pour le binaire : `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PI_CACHE_RETENTION=short` (et `cacheRetention` dans les réglages), `PRIME_AGENT_CODING_AGENT_DIR`, `PRIME_AGENT_KERNEL_VENV`, `HTTPS_PROXY`, `NO_PROXY=127.0.0.1,localhost`, `HOME` et `TMPDIR` isolés.

## Accès au modèle

- **Reverse proxy** (`proxies.py`) : `baseUrl` du provider vers `http://127.0.0.1:<port>/backend-api`. Cible amont fixe, `chatgpt.com:443` en TLS ; seuls les chemins sous `/backend-api/codex/` passent. WebSocket et SSE passent par le même port : le banc mesure le transport par défaut du harness (WebSocket, avec continuation `previous_response_id`). En mode capture, la cible est le serveur de capture (127.0.0.1 seulement).
- **Proxy CONNECT** : `HTTPS_PROXY` du harness, pour le rafraîchissement du jeton (le client HTTP du harness respecte `HTTPS_PROXY`, le client WebSocket non : il ouvre une connexion directe, d'où le reverse proxy). Liste blanche `hosts/openai-codex.txt`, port 443.
- **Journaux** : `proxy.ndjson` et `connect.ndjson`, jamais d'en-tête ni de corps. Par requête : transport, chemin, statut, octets, id de session (`prompt_cache_key`) pour l'appariement avec le journal d'usage.
- Le kernel n'a pas `HTTPS_PROXY` (filtrage T02) : vérifié par la sonde.

## Rapport

`runs/<horodatage UTC>-<sha court>-<modèle>/` (ignoré par git) : `report.md`, `report.json`, `tasks.csv`, et par tâche les sessions, le journal d'usage, la sortie JSON du binaire, les journaux des proxys et de la notation. En mode capture, les requêtes reçues par le serveur de capture y sont aussi (en-têtes d'authentification masqués, identifiants factices).

- **En-tête** : commit, sha256 du binaire et commit de son manifeste, sha256 de `models.bundled.json`, provider, modèle, source de sa fiche et hypothèses, effort, réglages non par défaut (`masking`, `rlm`, `cacheRetention`, `transport`), version et sha256 de `prices.json`, hôtes autorisés, nombre de runs.
- **Par tâche** : statut, tokens (`input`, `output`, `cacheRead`, `cacheWrite`, ventilation 5 min / 1 h, `reasoning`), pic de contexte (tokens du plus gros prompt envoyé : `input + cacheRead + cacheWrite`), appels par type, tours (appels `main`), taux de cache de session et médiane par requête de continuation, invalidations et coût d'écriture associé, coût théorique, durée, TTFT médian, sorties coupées (`length`), cellules `python_repl` en erreur par type (`SyntaxError` à part), CONNECT refusés, transport de chaque requête, lectures hors workspace réussies (anomalie, mode capture).
- **Transport** : WebSocket attendu pour les appels de boucle (`main`, `subagent`). Toute requête de boucle en SSE est comptée et signalée comme anomalie. Compaction, `/refine` et résumé de branche passent en SSE par construction (`compaction_exec.rs:177`, `refine.rs:664`, `branch_summarization.rs:468`, dans `crates/pa-core/src/session_engine/`).
- **Totaux** : tâches résolues, coût total et par tâche résolue, tokens, tours médians, taux de cache, anomalies. Avec `RUNS>1` : taux de résolution, bascule et écart-type par tâche.
- **Secrets** : recherche, dans tout le dossier de run, des valeurs factices de `bench/tools/capture/run.py`, de l'identifiant factice du mode capture et, en run réel, des valeurs de `auth.json`.

Un champ absent est écrit `null`, jamais estimé.

## Coût théorique

`prices.json` : tarif API Standard d'OpenAI par modèle, avec la page source et la date de relevé. Coût = tokens × tarif, par requête, au tarif contexte long au-delà de 272 000 tokens d'entrée. Limites, notées dans chaque rapport :

- l'API Responses normalisée donne `cacheWrite` = 0 (`docs/usage.md`) : l'écriture en cache est comptée au tarif d'entrée ;
- le corps Codex ne porte pas de rétention de cache : le tarif d'écriture 30 min est retenu.

## Tâches

```
tasks/<id>/
  task.toml   # id, lang, source, setup, test, fail_to_pass, pass_to_pass, timeout_s, tags
  prompt.md   # énoncé donné à l'agent (symptôme, pas de solution)
  hidden/     # tests copiés après la phase agent
```

`source` : `{ path = "repo" }` (dossier local) ou `{ git = "<url>", commit = "<sha>", patch = "break.patch" }`. `test` doit écrire un rapport JUnit dans `$SMOKE_JUNIT` ; un identifiant de test vaut `<fichier relatif>::<nom>` (ou `<classe>::<nom>`). Les tâches marquées `exemple` (`py-example-001`, `node-example-001`) valident le format et ne comptent pas dans le banc : elles ne tournent par défaut qu'en mode capture.

## Fichiers

| Fichier | Rôle |
|---|---|
| `run.py` | Runner (contrôles, préparation, sonde, chauffe, phase agent, notation, rapport) |
| `proxies.py` | Reverse proxy à cible fixe et proxy CONNECT à liste blanche |
| `sandbox-agent.sb` | Profil sandbox de la phase agent (réseau localhost, tests cachés et cache des tâches illisibles) |
| `metrics.py` | Mesures depuis le journal d'usage, la session et les journaux des proxys |
| `compare.py` | Comparaison appariée de deux runs |
| `prices.json` | Tarifs API (source, date) |
| `models/openai-codex.json` | Fiches des modèles absents du catalogue (`gpt-6.1-terra`) |
| `hosts/openai-codex.txt` | Hôtes autorisés au proxy CONNECT (établi par `--record-hosts`) |
| `settings/*.json` | Variantes de réglages |
