# Smoke bench local (T06, YA-14)

Lance le binaire d'une branche sur quelques tâches courtes avec un vrai modèle (« OAuth abonnement pour provider », ChatGPT), en local sur macOS, et écrit un rapport horodaté : par tâche, résolue ou non, tokens par catégorie, coût théorique (rapporté, pessimiste, à cache théorique), tours, taux de cache, transport de chaque requête. `compare.py` compare deux rapports.

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
python3 -I bench/smoke/compare.py <run A> <run B> [--baseline bench/smoke/baseline/<modèle>.json] --out <fichier>.md
python3 -I bench/smoke/baseline.py <run> [<run> ...] [--exclude <run>:<tâche>[,<tâche>...]]   # écrit bench/smoke/baseline/<modèle>.json
python3 -I bench/smoke/metrics.py <run> [<run> ...]    # miss serveur et taux évitable, runs déjà faits
python3 -I bench/smoke/run.py --rereport <run>         # rapport recalculé depuis les journaux du run
python3 -I bench/smoke/metrics.py --theory-check <run> [<run> ...]   # validation du cache théorique
python3 -I bench/smoke/tools/ab.py <dossier du binaire> <sha de référence> --a <réglages A> --b <réglages B> [--tasks ...] [--passes 3] [--seed N]
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-<horodatage>/ab.json [--baseline bench/smoke/baseline/<modèle>.json]
python3 -I -m unittest discover -s bench/smoke/tests -v   # tests sans modèle
```

- `MODEL` : liste fermée `gpt-6.1-sol`, `gpt-6.1-terra`, `gpt-6-luna`, `gpt-6-astra`. Tout autre id est refusé, même présent au catalogue. Sans `MODEL` : un run par modèle par défaut (`gpt-6.1-sol`, `gpt-6.1-terra`, `gpt-6-luna`) ; `gpt-6-astra` seulement avec `MODEL=gpt-6-astra`.
- `gpt-6.1-terra` est absent de `models.bundled.json` : sa fiche (`models/openai-codex.json`, relevée sur la doc officielle) est déclarée dans le `models.json` du dossier agent. Un modèle absent du catalogue et sans fiche complète est refusé ; le repli du harness sur la fiche du modèle par défaut n'est jamais utilisé. `contextWindow` de Terra : 272 000, provisoire (valeur du catalogue pour les modèles Codex voisins), hypothèse notée dans la fiche et dans l'en-tête du rapport.
- `EFFORT` : `high` au banc (décision du 04/11/2026), noté dans le rapport.
- Variantes : fichiers de réglages (`settings/default.json`, `settings/masking-on.json`), jamais des branches.
- Validation sans modèle : `make smoke BINARY=... CAPTURE=1 [CAPTURE_FAIL=node-example-001] [CAPTURE_SPAWN=1]`. `CAPTURE_SPAWN=1` (`--capture-spawn`) : chaque tâche joue le scénario `spawn` du serveur de capture (un sous-agent), pour vérifier que ses sessions sont ramassées. Options du runner en plus : `--capture-reject-model <id>` (le serveur refuse l'id au run de chauffe), `--capture-no-websocket` (le serveur refuse le WebSocket : repli SSE).
- Recalcul : `--rereport <run>` relit les journaux d'un run déjà fait (sans binaire ni modèle) et réécrit son rapport ; tests cachés, sortie et durée du binaire restent ceux du run. L'ancien rapport est gardé (`report.initial.*`, `tasks.initial.csv`). Pour un run antérieur à YA-07b, les sessions de sous-agents sont copiées depuis le dossier agent du banc, et l'empreinte de chaque tâche est recalculée au commit de l'en-tête depuis les fichiers suivis par git, seulement si `commit_dirty` vaut `false` (sinon version inconnue). `prices.json` doit être celui du run (sha256 de l'en-tête).
- Version des tâches : chaque résultat porte l'empreinte du dossier de sa tâche (`task_version` : `tree_hash` de `task.toml`, `prompt.md`, `hidden/` et `files/`, la clé de préparation), listée dans la section « Version des tâches » du rapport et dans `tasks.csv`. `baseline.py` refuse une tâche dont les runs retenus ont deux versions ou une version inconnue, et n'écrit alors rien ; `--exclude <run>:<tâches>` (répétable, `<run>` = nom du dossier) retire des tâches d'un run, par exemple celles dont l'énoncé a changé depuis. La baseline note l'empreinte de chaque tâche et les runs retenus.

## Déroulé

1. **Contrôles** : modèle de la liste fermée, au catalogue ou déclaré par une fiche complète ; login présent ; liste d'hôtes établie.
2. **Préparation** (une fois par binaire ou par version de tâche) : venv du kernel contre le serveur de capture ; pour chaque tâche, copie ou clone, patch, fichiers de `files`, `setup`, puis vérification sous sandbox que `fail_to_pass` échoue et `pass_to_pass` passe sur l'état cassé (sinon la tâche est invalide, arrêt). `setup` et `test` tournent sous `sandbox-task.sb` (voir « Code des tâches »). Source `git` : après `setup`, l'historique est remplacé par un seul commit, pour que le commit de correction et la suite de l'historique ne soient pas lisibles par l'agent.
3. **Sonde réseau et isolation** (profil de la phase agent, serveur de capture, aucun modèle) : `curl` direct depuis `bash()` en échec, `curl https://example.com` par le proxy CONNECT refusé et journalisé, noms des variables de proxy vues par le kernel et par `bash()`, lecture d'un test caché et de la copie pristine par chemin absolu (Python et `bash()`) en échec. Les scénarios des tâches d'exemple refont ces lectures pendant la phase agent.
4. **Run de chauffe** (sandbox, même accès que la phase agent) : le modèle doit répondre. Un id faux ou un jeton refusé fait échouer la préparation ici, pas le run mesuré. Rafraîchit le jeton si besoin.
5. **Phase agent** (sandbox `sandbox-agent.sb` : localhost seulement ; contenu de `$HOME` illisible sauf liste blanche : workspace, dossier agent du banc pour le login, dossier du binaire, `HOME` et `TMPDIR` du banc, venv du kernel, Pythons gérés par uv, toolchain Node ; écriture limitée au workspace, au dossier agent, au `HOME`/`TMPDIR` du banc et au venv du kernel ; ni lecture ni écriture de `bench/smoke/` ni du cache des tâches hors workspace, donc ni `hidden/` ni la copie pristine). La sonde tente aussi de lire `~/.ssh`, `~/.cache/uv` et `~/.npm` (les vrais) : une lecture réussie est une anomalie : copie fraîche du projet, `prime-agent -p --mode json --provider openai-codex --model <id> --thinking <effort>`, énoncé de `prompt.md`.
6. **Notation** (sandbox) : `hidden/` copié dans le workspace, `test` lancé avec la variable `SMOKE_JUNIT` (rapport JUnit). Résolue = tous les `fail_to_pass` et `pass_to_pass` passent. Statuts : `resolved`, `failed`, `timeout`, `stalled` (garde-fou d'inactivité, ci-dessous), `agent_error`, `harness_error`.

**Garde-fou d'inactivité** (YA-14) : pendant la phase agent d'une tâche, si aucun octet ne passe par le reverse proxy (dans un sens ou dans l'autre) pendant 300 s, le runner arrête la tâche, statut `stalled` (incident « flux bloqué », écarté des A/B). Le client n'a pas de délai d'inactivité sur le flux WebSocket Codex (`ab/masquage-20261106.md`, incidents) : sans ce garde-fou, un flux bloqué attend le `timeout_s` de la tâche (1800 s). Réglable par `--idle-timeout S` (0 : désactivé), noté dans l'en-tête (`idle_timeout_s`). Limite : une cellule qui tourne plus de 300 s sans aucun appel au modèle déclencherait aussi l'arrêt (le garde-fou ne distingue pas une réponse attendue d'un outil long). Testé sans modèle par `tests/test_bench.py` (amont qui accepte la connexion et ne répond jamais).

Variables fixées pour le binaire : `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PI_CACHE_RETENTION=short` (et `cacheRetention` dans les réglages), `PRIME_AGENT_CODING_AGENT_DIR`, `PRIME_AGENT_KERNEL_VENV`, `HTTPS_PROXY`, `NO_PROXY=127.0.0.1,localhost`, `HOME` et `TMPDIR` isolés.

## Accès au modèle

- **Reverse proxy** (`proxies.py`) : `baseUrl` du provider vers `http://127.0.0.1:<port>/backend-api`. Cible amont fixe, `chatgpt.com:443` en TLS ; seuls les chemins sous `/backend-api/codex/` passent. WebSocket et SSE passent par le même port : le banc mesure le transport par défaut du harness (WebSocket, avec continuation `previous_response_id`). En mode capture, la cible est le serveur de capture (127.0.0.1 seulement).
- **Proxy CONNECT** : `HTTPS_PROXY` du harness, pour le rafraîchissement du jeton (le client HTTP du harness respecte `HTTPS_PROXY`, le client WebSocket non : il ouvre une connexion directe, d'où le reverse proxy). Liste blanche `hosts/openai-codex.txt`, port 443.
- **Journaux** : `proxy.ndjson` et `connect.ndjson`, jamais d'en-tête ni de corps. Par requête : transport, chemin, statut, octets, id de session (`prompt_cache_key`) pour l'appariement avec le journal d'usage.
- Le kernel n'a pas `HTTPS_PROXY` (filtrage T02) : vérifié par la sonde.

## Rapport

`runs/<horodatage UTC>-<sha court>-<modèle>/` (ignoré par git) : `report.md`, `report.json`, `tasks.csv`, et par tâche les sessions, le journal d'usage, la sortie JSON du binaire, les journaux des proxys et de la notation.

- **Sous-agents** : le harness écrit les sessions lancées par `rlm.spawn` sous `session-artifacts/<parent>/sub-<id>/` du dossier agent, pas sous `sessions/`. Le runner les copie dans `sessions/subagents/` (imbrications comprises). Tokens, coût, transport, cache, miss serveur, cellules en erreur et sondes d'isolation portent sur la session principale et ses sous-agents ; statut et tours (appels `main`) sur la session principale. Une section « Sous-agents » donne leur part par tâche (sessions, appels, coût, part du coût, taux de cache, miss serveur), et les totaux la reprennent. En mode capture, les requêtes reçues par le serveur de capture y sont aussi (en-têtes d'authentification masqués, identifiants factices).

- **En-tête** : commit, sha256 du binaire et commit de son manifeste, sha256 de `models.bundled.json`, provider, modèle, source de sa fiche et hypothèses, effort, réglages non par défaut (`masking`, `rlm`, `cacheRetention`, `transport`), version et sha256 de `prices.json`, garde-fou d'inactivité (`idle_timeout_s`), hôtes autorisés, nombre de runs, durée des phases (`timing_s` : `prepare`, `probe`, `warmup`, `tasks`), et pour un bras d'A/B (`--ab`, posé par `tools/ab.py`) : passe, tâche, bras, position, graine, écart de réglages, sha de référence.
- **Par tâche** : statut, tokens (`input`, `output`, `cacheRead`, `cacheWrite`, ventilation 5 min / 1 h ou 30 min / 12 h, `reasoning`), pic de contexte (tokens du plus gros prompt envoyé : `input + cacheRead + cacheWrite`), appels par type, tours (appels `main`), taux de cache de session, médiane par requête de continuation et taux « évitable » (hors premier appel, invalidations journalisées et miss serveur), miss serveur (appel de boucle hors premier de session, `cacheRead = 0`, sans invalidation : délai depuis l'appel précédent, transport, continuation), invalidations et coût d'écriture associé, coût théorique, durée, TTFT médian, sorties coupées (`length`), cellules `python_repl` en erreur par type (`SyntaxError` à part), CONNECT refusés, transport de chaque requête, lectures hors workspace réussies (anomalie, mode capture).
- **Transport** : WebSocket attendu pour les appels de boucle (`main`, `subagent`). Toute requête de boucle en SSE est comptée et signalée comme anomalie. Compaction, `/refine` et résumé de branche passent en SSE par construction (`compaction_exec.rs:177`, `refine.rs:664`, `branch_summarization.rs:468`, dans `crates/pa-core/src/session_engine/`).
- **Totaux** : tâches résolues, coût total et par tâche résolue, tokens, tours médians, taux de cache, anomalies. Avec `RUNS>1` : taux de résolution, bascule et écart-type par tâche.
- **Secrets** : recherche, dans tout le dossier de run, des valeurs factices de `bench/tools/capture/run.py`, de l'identifiant factice du mode capture et, en run réel, des valeurs de `auth.json`.

Un champ absent est écrit `null`, jamais estimé.

## Coût théorique

`prices.json` : tarif API Standard d'OpenAI par modèle, avec la page source et la date de relevé. Coût = tokens × tarif, par requête, au tarif contexte long au-delà de 272 000 tokens d'entrée. Limites, notées dans chaque rapport :

- l'API Responses donne `cacheWrite` = `cache_write_tokens` depuis T08 (`docs/usage.md`), 0 quand le provider ne le renvoie pas : l'écriture est alors comptée au tarif d'entrée ;
- avec la ventilation du provider (`cacheWrite30m`, `cacheWrite12h`), chaque part est au tarif de son TTL (`cache_write_30m`, `cache_write_12h` de `prices.json`) ; sans ventilation, le corps Codex ne portant pas de rétention de cache, le tarif d'écriture 30 min est retenu.

### Coût pessimiste (T08b)

Affiché à côté du coût rapporté : même calcul, mais `input` (non caché) au tarif `cache_write_30m` au lieu du tarif d'entrée, comme si tout ce qui n'a pas été lu en cache y avait été écrit. C'est un plafond pour un provider qui n'annonce pas ses écritures (Codex : `cache_write_tokens` toujours à 0 au run réel du 04/11, `docs/usage.md`), trop haut pour un provider qui les annonce. 30 min et non 12 h : le harness ne demande jamais 12 h.

- Rapport : totaux et par tâche résolue (« rapporté ; pessimiste »), colonne « Coût pess. $ » du tableau des tâches (avec une colonne `cacheWrite`), `cost_pessimistic` dans `tasks.csv`, `cost.pessimistic_total` par résultat, `totals.cost_pessimistic` et `per_task.<tâche>.cost_pessimistic_mean` dans `report.json`.
- `compare.py` : dans la section du coût agrégé, le coût pessimiste des mêmes tâches, A → B, en %, avec le seuil du coût rapporté (au-delà ou en deçà), sans alerte ; ligne « Coût pessimiste » dans les totaux. Les alertes et `baseline.py` restent sur le coût rapporté.
- Un run antérieur n'a pas de coût pessimiste : `run.py --rereport <run>` le recalcule depuis les journaux, le coût rapporté ne change pas.

### Coût à cache théorique (YA-14)

Métrique de décision des A/B (décision du 11/11/2026). Pour chaque appel, le `cacheRead` est remplacé par ce qu'un serveur qui ne rate jamais aurait lu : le préfixe commun avec la requête précédente de la même session (chaque sous-agent dans la sienne). Le reste du prompt est au tarif d'entrée ; sortie et raisonnement inchangés ; tarifs de `prices.json`, contexte long au-delà du seuil. Les miss serveur (complets ou partiels) disparaissent ; les ruptures de préfixe causées par le harness restent comptées.

**Méthode** : approximation depuis le journal d'usage et les invalidations (`metrics.theoretical_cache`). En WebSocket avec `previous_response_id`, la requête ne porte pas tout le contexte, et le banc ne journalise aucun corps ; le provider compte pourtant le prompt entier (`input + cacheRead + cacheWrite`). La reconstruction depuis les sessions JSONL a été écartée : le masquage est appliqué à l'envoi, pas dans la session, et il faudrait le tokenizer du modèle.

| Appel | `cacheRead` théorique | Base |
|---|---|---|
| Premier appel de boucle de la session | 0 (pas de requête précédente) | `premier` |
| Continuation (contexte append-only) | min(prompt, prompt précédent + output précédent), `output` raisonnement compris | `continuation` |
| Invalidation journalisée (masquage) | `cacheRead` rapporté (préfixe commun inconnu sans le corps : borne basse) | `invalidation` |
| Appel annexe intercalé depuis l'appel de boucle précédent (compaction…), ou prompt plus court que le précédent sans invalidation | `cacheRead` rapporté | `rupture` |
| Appel annexe (compaction, `/refine`, résumé) | `cacheRead` rapporté | `annexe` |

Le prompt précédent est celui du dernier appel de boucle de la session au prompt non vide. L'output précédent compte : le serveur Codex garde la réponse, le `cacheRead` rapporté dépasse le prompt précédent dans 18,3 % des appels de continuation sans miss serveur, jamais le prompt précédent + output (mesure ci-dessous).

**Validation** (critères fixés avant la mesure, `metrics.THEORY_*`) : sur les appels de continuation sans miss serveur (`cacheRead` rapporté > 0), écart (théorique − rapporté) / prompt. La borne est fausse si le rapporté dépasse le théorique sur plus de 1 % des appels ; la méthode est trop large si l'écart médian dépasse 2 % du prompt. Verdict par run à partir de 30 appels (`THEORY_MIN_CALLS`, ajouté après la première mesure : deux runs `gpt-6-luna` de 5 et 7 appels, prompts d'environ 6 000 tokens, où la granularité de 128 tokens du cache fait seule un écart médian proche de 2 %). Mesure du 11/11/2026 sur les 11 runs réels de `runs/` (`gpt-6.1-terra` et `gpt-6-luna`, 04/11 et 06/11) :

```bash
R=bench/smoke/runs; python3 -I bench/smoke/metrics.py --theory-check $R/20261104T103739Z-4718c9f-gpt-6-luna \
  $R/20261104T144609Z-6d9561e-gpt-6.1-terra $R/20261104T174431Z-6d9561e-gpt-6.1-terra $R/20261104T180604Z-6d9561e-gpt-6.1-terra \
  $R/20261104T203149Z-6eed2da-gpt-6-luna $R/20261106T202245Z-0fa8fad-gpt-6.1-terra $R/20261106T211354Z-0fa8fad-gpt-6.1-terra \
  $R/20261106T213306Z-0fa8fad-gpt-6.1-terra $R/20261106T215117Z-0fa8fad-gpt-6.1-terra $R/20261106T221035Z-0fa8fad-gpt-6.1-terra \
  $R/20261106T222846Z-0fa8fad-gpt-6.1-terra
```

3 917 appels : rapporté > théorique 0,00 % ; écart médian +0,93 % du prompt (p05 +0,42 %, p25 +0,66 %, p75 +1,54 %, p90 +5,31 %, p95 +17,78 %, max +72,66 %) : **conforme**. L'écart courant (≈ 1 %) vient de la granularité de 128 tokens du cache et de quelques tokens de mise en forme ; la queue (10,4 % des appels au-delà de 5 %) est lue comme des miss partiels du serveur (interprétation : le rapporté y retombe sur un préfixe plus court, plus ancien de la session ou celui du prompt système partagé), que la métrique retire. Biais connu : le théorique surestime le cache d'environ 1 % du prompt sur chaque continuation, dans les deux bras d'un A/B.

Rapport : `cost.theoretical_total` et `cost.theoretical` (appels par base, tokens théoriques, écarts de validation) par résultat, colonne « Coût th. $ » et `cost_theoretical` de `tasks.csv`, `totals.cost_theoretical`, `totals.cache_theory_check`, `per_task.<tâche>.cost_theoretical_mean`, section « Coût à cache théorique » (appels par base, validation du run). En mode capture, l'usage est simulé (lecture de 90 % du prompt précédent) : la validation n'y a pas de sens. `compare.py` le donne dans la section du coût agrégé (même seuil que le coût rapporté, sans alerte) et dans les totaux. Un run antérieur : `run.py --rereport <run>`.

## Tâches

```
tasks/<id>/
  task.toml   # id, lang, source, setup, test, fail_to_pass, pass_to_pass, timeout_s, tags
  prompt.md   # énoncé donné à l'agent (symptôme, pas de solution)
  hidden/     # tests copiés après la phase agent
```

`source` : `{ path = "repo" }` (dossier local) ou `{ git = "<url>", commit = "<sha>", gold = "<sha>", patch = "break.patch" }` (`commit` : parent de la correction ; `gold` : commit de correction, pour `--validate-gold`). Champs des tâches publiques : `license`, `upstream` (dépôt, PR, issue), `files` (fichiers de `files/` copiés dans le workspace avant `setup`, par exemple le `uv.lock` figé à la revalidation quand le dépôt n'en a pas ; `files/freeze.txt` garde l'environnement de référence). Tags : `python` ou `node`, `multi-fichiers`, `difficile` ; une tâche `difficile` résolue en moins de 10 tours est signalée dans les anomalies. Les tâches du banc n'ont pas de scénario de capture qui les résout : en mode capture, elles jouent `smoke-fail`. Construction : `tools/make_task.py` depuis un candidat de `tools/candidates.csv` ; l'énoncé est réécrit à la main. `test` doit écrire un rapport JUnit dans `$SMOKE_JUNIT` ; un identifiant de test vaut `<fichier relatif>::<nom>` (ou `<classe>::<nom>`). Les tâches marquées `exemple` (`py-example-001`, `node-example-001`) valident le format et ne comptent pas dans le banc : elles ne tournent par défaut qu'en mode capture.

### Validation sans modèle

```bash
python3 -I bench/smoke/run.py --validate-gold [--tasks id1,id2] [--validate-runs 3]
```

Sans binaire ni modèle : préparation de chaque tâche, puis 3 fois l'état cassé (`fail_to_pass` en échec, `pass_to_pass` en succès) et 3 fois la correction officielle (diff de `commit` à `gold` sans les tests cachés, tout en succès), tests sous sandbox réseau coupé. `--validate-profile agent` lance les tests sous `sandbox-agent.sb` (conditions de la phase agent) au lieu de `sandbox-task.sb`. Rapport : `runs/<horodatage>-validate-<profil>/validate.md`. Une tâche instable est remplacée.

Après la remise à zéro de l'historique, la préparation vérifie la copie pristine : un seul commit atteignable (reflog compris), ni packfile, ni `ORIG_HEAD`, `FETCH_HEAD`, `packed-refs`, ni objet inaccessible, ni blob de la version corrigée d'un fichier du correctif (base d'objets ou fichier de l'arbre, `.venv` et `node_modules` compris). Une trace arrête la préparation.

Interpréteur des tâches Python figé : CPython 3.12.14 géré par uv, installé depuis python-build-standalone sur GitHub (`uv python install --mirror`, sans détour par `releases.astral.sh`, hors liste blanche), puis `uv sync --python 3.12.14 --managed-python --no-python-downloads`.

### Seuils de `compare.py`

Alerte (le run de PR est alors relancé 2 fois) : tâche stable en baseline (résolue à tous ses runs, 3 au moins) non résolue en B ; tâche résolue en A et non en B ; écart de coût ou de tokens au-delà de max(2 × CV de la tâche en baseline, 5 %) ; écart du coût agrégé (somme des coûts moyens des tâches stables de la baseline présentes dans A et B, de même version) au-delà de max(2 × CV du total, 5 %). CV du total (`aggregate` de la baseline) : le plus grand de deux estimations, tâches indépendantes (√Σσ² / Σμ) et sommes par passe (k-ième run retenu de chaque tâche), décision du 04/11/2026 ; les deux sont affichés dans la section « Coût agrégé des tâches stables » du rapport. Sans baseline, seules les régressions de résolution sont signalées. Une tâche dont l'empreinte diffère entre B et A ou la baseline, ou y est inconnue (run sans empreinte : `--rereport`), est exclue de la comparaison, sans alerte, et listée dans « Tâches exclues de la comparaison » (« version de tâche différente ») ; les totaux restent ceux des runs complets. Mode capture : `--capture-server-miss-at-turn N` fait annoncer `cacheRead = 0` au tour N (miss serveur simulé).

### A/B entrelacé par tâche (YA-14)

`tools/ab.py` remplace `ab-masking.sh` (supprimé ; il reste lisible au commit `346dbc8e0`). Appels réels au modèle : lancé par Yusuf (CLAUDE.md, règle 6), sauf `--capture` et `--dry-run`.

```bash
DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha de référence> \
  --a bench/smoke/settings/default.json --b bench/smoke/settings/<x>.json [--tasks id1,id2] [--passes 3] [--seed N] \
  [--model gpt-6.1-terra] [--effort high] [--idle-timeout 300] [--capture] [--dry-run]
```

- **Contrôles** (ceux d'`ab-masking.sh`) : binaire présent et exécutable, `--version` au sha du manifeste, sha256 de l'exécutable égal au manifeste, dossier qui porte ce sha ; sha de référence connu et sources livrées identiques (`git diff --quiet <sha> <ref> -- crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock`). L'écart de réglages entre A et B est quelconque : calculé à plat, affiché, noté dans `ab.json`, `ab.md` et l'en-tête de chaque rapport (`header.ab`). A = B est accepté (A/A).
- **Ordre** : pour chaque passe, chaque tâche (ordre de `--tasks`, par défaut les tâches du banc sans les exemples) lance ses deux bras dos à dos, A puis B ou B puis A, tiré au sort (`random.Random(seed)`, un tirage par passe et par tâche). Graine notée ; tirée de `os.urandom` si `--seed` est absent. `--dry-run` affiche les contrôles et le plan sans rien lancer.
- **Lancements** : chaque bras est un appel séparé à `run.py --runs 1 --tasks <tâche>`, donc refait sonde et chauffe (un appel au modèle). Le surcoût par lancement (durée de l'appel − `timing_s.tasks`) est noté par bras et résumé dans `ab.md` et par `ab_stats.py`. Mesure du run de vérification : voir la PR de YA-14. Deux lancements de suite en échec arrêtent l'A/B.
- **Sortie** : `runs/ab-<horodatage UTC>/` : `ab.json` (plan, graine, réglages, bras, dossiers de run, surcoût), `ab.md`, `log.txt`, runs dans `arms/`.
- **Statistiques** : `tools/ab_stats.py --plan <ab.json> [--baseline <fichier>]`. Métrique de décision : coût à cache théorique. Par tâche, moyenne de chaque bras sur ses unités retenues et écart apparié ; écart agrégé (somme B / somme A − 1) et écart apparié moyen en $, IC 95 % par bootstrap sur les tâches (10 000 tirages, graine 0, sortie reproductible), sur les tâches stables de la baseline et sur toutes ; coût rapporté et pessimiste, résolution, tours, tokens par bras ; invalidations ; incidents ; surcoût par lancement. Les A/B par passes complètes (06/11) se lisent encore en donnant les runs par paires (`ab_stats.py <A1> <B1> <A2> <B2> ...`). `--no-exclusion` : sensibilité, unités écartées gardées.

### A/B : erreurs provider

Une erreur provider sur un bras écarte la paire entière ; en A/B entrelacé par tâche, elle écarte la tâche de cette paire (ses deux bras). Erreurs visées : quota (429 ou équivalent), surcharge (« Our servers are currently overloaded »), flux bloqué (réponse arrêtée sans erreur). Règle validée le 11/11/2026, après l'A/B masquage (`ab/masquage-20261106.md`).

`tools/ab_stats.py` l'applique automatiquement et liste les unités écartées avec leur cause. Erreur provider : statut `stalled` ; `timeout` sans activité des journaux de la tâche (dernière requête de `proxy.ndjson` ou fin d'appel du `.usage.ndjson`) pendant au moins 300 s avant l'arrêt (`--stall-gap`) ; réponse en `stopReason` `error`, session principale ou sous-agent ; requête proxy à statut autre que 101 ou 200 ; lancement de `run.py` en échec. Les autres incidents (`timeout` d'une tâche longue, `agent_error` ou `harness_error` sans erreur provider) sont listés sans écarter l'unité.

### Code des tâches

`setup` et `test` exécutent le code de dépôts tiers. Ils tournent sous `sandbox-task.sb` : écriture limitée au dossier de la tâche dans le cache, aux caches uv et npm, aux Pythons gérés par uv et au `HOME`/`TMPDIR` du banc ; lecture du contenu de `$HOME` refusée sauf ces dossiers et la toolchain Node (le login du banc et `bench/smoke/` restent illisibles) ; réseau limité à localhost. `setup` passe par un proxy CONNECT à liste blanche (`hosts/setup.txt` : PyPI, npm, GitHub), `test` n'a pas de proxy.

## Fichiers

| Fichier | Rôle |
|---|---|
| `run.py` | Runner (contrôles, préparation, sonde, chauffe, phase agent, notation, rapport) |
| `proxies.py` | Reverse proxy à cible fixe et proxy CONNECT à liste blanche |
| `sandbox-agent.sb` | Profil sandbox de la phase agent (réseau localhost, `$HOME` illisible hors liste blanche, tests cachés et cache des tâches illisibles) |
| `sandbox-task.sb` | Profil sandbox de `setup` et `test` des tâches (écriture et lecture de `$HOME` limitées, réseau localhost) |
| `hosts/setup.txt` | Hôtes autorisés au proxy CONNECT de `setup` |
| `tools/mine.py`, `tools/mine.sb`, `tools/mine-hosts.txt` | Minage et validation des candidats (T07), sandbox et hôtes de la revalidation macOS |
| `tools/discovered.json`, `tools/candidates.csv` | PR découvertes, candidats validés (Linux, écarts macOS) |
| `tools/make_task.py` | Construction d'une tâche depuis un candidat |
| `tools/install-bin.sh` | Installation du binaire d'un run CI dans `~/.cache/yusagent-bin/<sha>/`, sommes et version vérifiées ; dossier existant laissé tel quel s'il passe les contrôles, sinon erreur ; `--force` le remplace après vérification de la nouvelle copie |
| `tools/ab.py` | A/B entrelacé par tâche (YA-14) : contrôles, ordre des bras tiré au sort à graine notée, un appel à `run.py` par bras, surcoût par lancement |
| `metrics.py` | Mesures depuis le journal d'usage, la session et les journaux des proxys ; cache théorique et sa validation (`--theory-check`) |
| `compare.py` | Comparaison appariée de deux runs ; alertes au-delà du bruit de la baseline |
| `baseline.py` | Baseline : bruit par tâche sur plusieurs runs de `main` (`baseline/<modèle>.json`, versionné) |
| `tools/ab_stats.py` | Statistiques d'un A/B apparié (entrelacé ou par passes) : coût à cache théorique (décision), rapporté et pessimiste, écarts appariés par tâche, IC par bootstrap sur les tâches, écart automatique sur erreur provider, mesures par bras, invalidations et incidents |
| `tests/test_bench.py` | Tests sans modèle : cache théorique, garde-fou d'inactivité, plan de l'A/B, écart sur erreur provider |
| `ab/` | Rapports d'A/B (versionnés) |
| `prices.json` | Tarifs API (source, date) |
| `models/openai-codex.json` | Fiches des modèles absents du catalogue (`gpt-6.1-terra`) |
| `hosts/openai-codex.txt` | Hôtes autorisés au proxy CONNECT (établi par `--record-hosts`) |
| `settings/*.json` | Variantes de réglages |
