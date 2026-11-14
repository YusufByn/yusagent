# A/B YA-03 (`editTool`, puis `shellTool`) : plan et règles de verdict

Règles fixées et versionnées avant tout run réel (ticket 09, `docs/tickets/09-ya03-base.md` ; journal du CDC, décisions du 13/11/2026). Deux A/B **séquentiels, non croisés**, sur le binaire de la PR 2 de YA-03. Le second se joue sur l'`editTool` retenu par le premier.

## Protocole commun

- `gpt-6.1-terra`, provider `openai-codex`, effort `high`.
- Les 10 tâches du banc (`bench/smoke/tasks/`, sans les exemples).
- 3 passes entrelacées par tâche (`tools/ab.py`) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort.
- Binaire : artefact de `ya-build-macos-arm64.yml`, installé par `tools/install-bin.sh`. SHA complet noté avant le premier run, dans « Binaire et chauffe » ci-dessous (`main` à `80e20ea5f`, qui contient la PR 2).
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité 300 s.
- **Avant tout** : run de chauffe d'une tâche sur le défaut (`bench/smoke/settings/default.json`), pour vérifier que le backend de l'abonnement accepte le tool libre `apply_patch`. Il ne compte dans aucun A/B.
- Statistiques : `python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --baseline bench/smoke/baseline/gpt-6.1-terra.json`. Tâches stables : `aggregate.tasks` de la baseline.
- **Paires écartées** : règle d'écart du README (« A/B : erreurs provider ») : une erreur provider (quota, surcharge, flux bloqué, statut `stalled`) sur un bras écarte la tâche de cette passe, ses deux bras. `ab_stats.py` l'applique et liste les unités écartées avec leur cause.

### Binaire et chauffe (14/11/2026)

- **Binaire** : `main` à `80e20ea5f353ff3855e30d19c0df7cc7d1cc9632` (merge de la PR #30 : PR 2 de YA-03, rejeu des appels libres #28, correctifs upstream du flux Responses `c12acadaa` et `42a07c13c`). Ce n'est plus le dernier commit de la PR 2 : sans #28 et #30, le tour après `apply_patch` et celui après un texte repartaient en corps complet. Build `ya-build-macos-arm64.yml` 43588586252, installé par `tools/install-bin.sh` dans `~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/`. sha256 de l'exécutable (manifeste et `shasum`) : `099f54d354c3daab653ec9502ae82658d32224600b616e95c5ffe4e2212ffc8e`. `--version` : `0.10.2-continuous.80e20ea5f353ff3855e30d19c0df7cc7d1cc9632`.
- **Run de chauffe** : `bench/smoke/runs/20261114T112415Z-80e20ea-gpt-6.1-terra`, `py-shortuuid-118`, défaut (`editTool` native, `shellTool` kernel), `gpt-6.1-terra`, effort `high` : résolue. Il ne compte dans aucun A/B.
  - Suite de conversation : requête 1 en corps complet, requêtes 2 à 6 avec `previous_response_id` (`proxy.ndjson`, champ `continuation`). La 2 suit un texte et un `python_repl`, la 5 suit un raisonnement et un `apply_patch`.
  - Session : textes avec `textSignature` (id d'item et `phase`), raisonnement du tour 4 (135 tokens de raisonnement, seul tour qui a raisonné) avec `thinkingSignature` (item complet, `encrypted_content`), `apply_patch` en `call_…|ctc_…`.
  - Reprise dans un nouveau processus (`continue_session.py`, `continue-1` puis `continue-2` avec `--log-bodies`) : corps complet accepté par le backend. Dans le corps de `continue-2` : `apply_patch` déclaré `custom`, l'appel rejoué en `custom_tool_call` (`id` `ctc_…`, `call_id`, `name`, `input`), sa sortie en `custom_tool_call_output`, après l'item `reasoning` et avant l'appel suivant ; `reasoning.effort` `high`.
  - **Cache après reprise (observation, mécanisme inconnu)** : un corps complet sur une nouvelle connexion obtient le niveau de cache de l'avant-dernière requête, pas de la dernière. `continue-1` : 11 776 tokens en cache sur 12 827, soit la longueur de la requête 5 arrondie à 128 (et non celle de la requête 6, 12 672). `continue-2` : 12 672 sur 12 846, soit la longueur de la requête 6 (et non celle de `continue-1`, 12 800). Deux observations. Le corps complet correspond à la chaîne au moins jusqu'à la fin de l'appel qui suit `apply_patch`. Sans effet sur l'A/B : chaque tâche est une session en un seul processus, en suite de conversation.

### Métriques d'édition (affichées dans les deux A/B, sans décider)

- **Edits appliqués au premier essai**, mesurés dans les deux bras : part des appels d'édition appliqués sans erreur, sur tous les appels d'édition de la session principale. Un appel d'édition est un appel du tool `apply_patch`, ou un appel de `edit()` dans une cellule du kernel. Un échec (patch refusé, `old_str` absent ou ambigu, fichier introuvable) compte comme un essai raté. Critère du CDC (YA-03) : au moins 95 %.
- **Cellules en erreur d'appel** (syntaxe, quoting), par bras.
- **Patchs refusés hors workspace** (`apply_patch` dont un chemin, après résolution des liens, sort du cwd de la session), par bras ; comptés aussi comme essais ratés.
- Calculées par `bench/smoke/metrics.py` (`edit_attempts`, `cell_errors`), sommées par bras dans la section « Par bras » d'`ab_stats.py`.

## A/B 1 : `editTool`

| Bras | Réglages | Édition | Prompt |
|---|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | tool `apply_patch` au format libre de Codex ; `edit()` du kernel gardé pour les éditions en lot | défaut de la PR 2 |
| B | `bench/smoke/settings/edit-kernel.json` (`{"editTool": "kernel"}`) | pas de tool d'édition ; `edit()` du kernel, documenté comme sur `main` | défaut de la PR 2 sans `apply_patch` |

`shellTool` vaut `kernel` dans les deux bras. Rien d'autre ne change : même binaire, mêmes tâches, même modèle, même effort.

Graine : **2290879124** (tirée de `os.urandom` le 13/11/2026 à 18:08Z, avant le run).

**Verdict par défaut : `editTool: native` (tool `apply_patch`).** La charge de la preuve est sur le bras `kernel` (B). Il est retenu seulement si l'une des deux conditions est remplie :

1. le bras `kernel` **résout au moins 3 tâches de plus** que le bras `native`, sur l'ensemble des paires valides. Lecture : « Tâches résolues par passe », totaux A et B de la section « Par bras ». Seuil fixé avant le run ; ce n'est pas un test statistique ;
2. le bras `kernel` **coûte moins** en coût à cache théorique des tâches stables, avec un IC bootstrap sur les tâches qui exclut zéro (ligne « tâches stables, théorique (décision) » de la section « Écart agrégé » d'`ab_stats.py`, somme B / somme A − 1, borne haute de l'IC 95 % < 0), **et** il ne résout pas 3 tâches ou plus de moins que le bras `native`.

**Affichés sans décider** : edits appliqués au premier essai (par bras), cellules en erreur d'appel, coût rapporté et pessimiste, tours, durée, miss serveur.

**Filet de régression (pas un verdict)** : le bras `kernel` (B), dont le prompt et les tools sont ceux de `main` à `read()`, `bash(env=)` et la note de troncature près, est aussi comparé à la baseline avec `compare.py` (effet de la PR 1, non entrelacé) : `python3 -I bench/smoke/compare.py <run A> <run B> --baseline bench/smoke/baseline/gpt-6.1-terra.json --out <fichier>.md`, avec en A un run des bras B de l'A/B `rlm.spawn` du 12/11 (source de la baseline, `ab-20261112T185001Z`) et en B un run des bras `kernel` de cet A/B. Les alertes de `compare.py` (seuils de la baseline) servent de filet ; elles ne changent pas le verdict.

### Résultats de l'A/B 1 (14/11/2026)

Le rapport complet est dans `bench/smoke/ab/ya03-ab1-20261114.md`, avec les mesures d'appoint dans `ya03-ab1-20261114-mesures.py`. Le run est `bench/smoke/runs/ab-20261114T120431Z` (lancé par Yusuf) : 60 bras, aucune unité écartée, aucun incident.

**Verdict : `editTool: native`**, validé par Yusuf le 14/11/2026. Aucune des deux conditions n'est remplie :
1. résolution : 24 tâches dans les deux bras (8 par passe), sans écart ;
2. coût à cache théorique des tâches stables, B / A − 1 : **+2,5 %, IC 95 % [−8,8 ; +16,5]**. La borne haute n'est pas sous 0.

**Affichés sans décider** :
- **Coût par tâche résolue** (théorique) : A 0,0542 $, B 0,0548 $, soit +1,0 % [−7,2 ; +11,3]. Les échecs touchent les mêmes tâches dans les deux bras.
- **Edits au premier essai** : A 80/81 (98,8 %), B 269/270 (99,6 %). Le critère du CDC (au moins 95 %) est tenu dans les deux bras.
- **Patchs refusés hors workspace** : 0 et 0.
- **Cellules en erreur d'appel** (SyntaxError) : 0 et 0. Toutes causes confondues : 1/309 dans A, 3/360 dans B.
- **Tours par session** : 14,3 pour A, 13,1 pour B (−8,4 %).
- **Tokens d'input envoyés** : −8,0 % pour B. L'input non caché augmente de 6,5 %.
- **Taux de cache** : 91,8 % pour A, 90,5 % pour B.
- **Ruptures** : 0 requête en corps complet après la première, dans les deux bras (`proxy.ndjson`).

**Métriques d'édition corrigées avant ce verdict.** `metrics.py` lisait les échecs dans `isError`, qui vaut toujours `false`. Il les lit maintenant dans `details.status` pour `python_repl` et dans le texte pour `apply_patch`, et ne compte un échec d'`edit()` que s'il est levé par le skill (commit `fe3dbfa25`). Les rapports des bras ont été recalculés par `run.py --rereport`. La cause de `isError` toujours à `false` est notée dans `docs/tickets/09-ya03-base.md`, aux points ouverts.

**Filet de régression : bras `kernel` contre la baseline.** `compare.py` ne prend qu'un rapport de chaque côté. La comparaison est donc faite dans le script de mesures, avec les mêmes seuils (2 × CV de la tâche et au moins 5 % ; agrégat 2 × `aggregate.cv`).
- **Résolution** : pas de régression, les 8 tâches stables sont à 3/3.
- **Coût agrégé** (rapporté) des tâches stables : −13,1 %, pour un seuil de ±12,1 %. C'est une baisse.
- **Alertes** : coût en baisse sur `node-tough-cookie-630` et `py-langextract-584`, tokens en hausse sur `node-koa-2029` (avec un coût en baisse).
- **Réserve** : la comparaison n'est pas entrelacée (12/11 contre 14/11, binaires `69cc894` et `80e20ea`). Elle sert de filet et ne change pas le verdict.

## A/B 2 : `shellTool`

| Bras | Réglages | Exécution |
|---|---|---|
| A | l'`editTool` retenu par l'A/B 1, `shellTool: kernel` | `bash()` dans le kernel |
| B | l'`editTool` retenu par l'A/B 1, `shellTool: native` (`bench/smoke/settings/bash-native.json`, ou sa variante avec `editTool: kernel`) | tool shell séparé d'après `exec_command` de Codex (`codex-rs/core/src/tools/handlers/shell_spec.rs`, SHA `69231e1`), sans `bash()` dans le kernel ; sous-ensemble validé avant la PR 2 |

Graine : **2205082327** (tirée de `os.urandom` le 13/11/2026 à 18:08Z, avant le run).

**Verdict par défaut : `shellTool: kernel`** (décision du 28/10/2026). Le bras bash natif (B) est retenu seulement si l'une des deux conditions est remplie :

1. il **résout au moins 3 tâches de plus** sur les paires valides (seuil fixé avant le run, pas un test statistique) ;
2. il **coûte moins** en coût à cache théorique des tâches stables, avec un IC bootstrap sur les tâches qui exclut zéro (borne haute de l'IC 95 % < 0), **et** il ne résout pas 3 tâches ou plus de moins.

Le rapport dit que **la résolution ne se tranche pas à 10 tâches**. Il note aussi, comme biais possibles ou limites (décisions du 13/11/2026, journal du CDC) : le tool de la variante est `exec_command` / `write_stdin` de Codex **sans** `tty`, `shell` ni `login` (retrait qui peut jouer contre bash natif) ; le tool natif et le retrait de la règle de prompt « pas de boucles shell ni de heredocs » sont mesurés **ensemble** ; le tool natif compte sa sortie en octets, `python_repl` en caractères (différence seulement sur du texte non ASCII). La condition 2 est un écart avec YA-14, qui ne prévoit que la résolution (journal du CDC, 13/11/2026). Si le tool shell de la variante s'écarte de `exec_command` (format maison), le rapport dit que l'A/B 2 est biaisé contre bash natif. Verdict provisoire : une seule famille de modèles (YA-14 demande au moins deux familles au banc complet).

**Affichés sans décider** : comme pour l'A/B 1.

### Résultats de l'A/B 2 (14/11/2026)

Le rapport complet est dans `bench/smoke/ab/ya03-ab2-20261114.md`, avec les mesures d'appoint dans `ya03-ab2-20261114-mesures.py`. Le run est `bench/smoke/runs/ab-20261114T140538Z` (lancé par Yusuf) : 60 bras, aucune unité écartée, aucun incident. Les deux bras utilisent `editTool: native` : A avec `default.json`, B avec `bash-native.json`.

**Verdict : `shellTool: kernel`**, validé par Yusuf le 14/11/2026, **provisoire** (une seule famille de modèles). La configuration retenue est `bench/smoke/settings/default.json`. Aucune des deux conditions n'est remplie :
1. résolution : 25 tâches dans les deux bras (A : 8, 9, 8 ; B : 9, 8, 8) ;
2. coût à cache théorique des tâches stables, B / A − 1 : **−2,9 %, IC 95 % [−14,0 ; +10,7]**. La borne haute n'est pas sous 0.

**Affichés sans décider** :
- **Coût par tâche résolue** (B / A − 1) :
  - théorique : A 0,0523 $, B 0,0500 $, soit −4,4 % [−19,7 ; +15,0] ;
  - rapporté : −3,3 % [−18,8 ; +15,1] ;
  - pessimiste : −3,1 % [−18,9 ; +15,3].
  - Les résolutions ne tombent pas sur les mêmes tâches : A résout une fois `node-filesize-376`, B une fois `node-little-coder-102`.
- **Usage du shell** : toutes les sessions s'en servent.
  - A : 147 commandes `bash()` dans 116 cellules ; 42 cellules ont lu un code non nul ; 37 ont rendu la main avant la fin d'un `bash()`.
  - B : 115 `exec_command` et 103 `write_stdin`, dont 50 sorties non nulles (surtout des tests en échec pendant le travail) et 0 erreur du tool.
- **Edits au premier essai** : 75/75 et 71/71. **Patchs refusés hors workspace** : 0 et 0.
- **Cellules en erreur d'appel** (SyntaxError) : 2 dans A, dont `files = !pwd` (shell en syntaxe IPython), et 0 dans B. Toutes causes confondues : 5/302 et 1/142.
- **Tours par session** : 14,1 et 13,9 (−1,7 %).
- **Tokens envoyés** : −4,2 % pour B ; input non caché −2,0 %.
- **Taux de cache** : 91,9 % et 91,7 %. **Miss serveur** : 5 et 6.
- **Ruptures** : 0 requête en corps complet après la première, dans les deux bras (423 et 416 requêtes).

**Reproductibilité** : le bras A de cet A/B a la même configuration et le même binaire que le bras A de l'A/B 1, joué 2 h plus tôt.
- Coût théorique des tâches stables, A2 / A1 − 1 : **+2,1 % [−4,4 ; +6,8]**. Par tâche stable, l'écart absolu a une médiane de 4,9 % et un maximum de 14,3 %.
- Résolution : une tâche non stable gagne une résolution.
- L'effet mesuré de B est donc du même ordre que le bruit entre deux runs identiques. **Le banc ne distingue pas un effet de moins de 10 % environ.** C'est une estimation tirée de ces deux runs, pas un calcul de puissance.

**Biais prévus** :
- **Schéma réduit** (sans `tty`, `shell` ni `login`). La description reprise de Codex annonce pourtant un PTY absent. Le modèle demande `yield_time_ms` à 1000 dans 99 `exec_command` sur 115, et une sonde `write_stdin` vide attend au moins 5 s : d'où 103 `write_stdin`. Ce point est noté dans `docs/tickets/10-ya03-shell-natif.md`.
- **Règle « pas de boucles shell ni de heredocs » retirée** : B n'a écrit ni heredoc ni boucle shell.
- **Octets contre caractères** : +4,1 % sur les sorties shell de B. Ses 17 sorties shell tronquées viennent d'un `max_output_tokens` demandé entre 500 et 3000.
- **Format** : l'en-tête est celui de Codex, sans format maison.

**Anomalies** : aucun bras au-delà de 2 fois la médiane de sa tâche.
- **`node-testsprite-356`, passe 2, bras B** (×1,56, 218 s) : c'est le seul bras de la tâche qui lance la suite complète, puis la sonde 16 fois.
- **Les deux résolutions inhabituelles sont de vrais correctifs.** Les 22 bras en échec sur `node-filesize-376` et `node-little-coder-102`, dans les deux A/B, réécrivent ou retirent un test existant qui passait déjà ; les deux bras résolus le laissent intact (ticket 09, points ouverts).

## Après les deux A/B

Nouvelle baseline `gpt-6.1-terra` depuis les bras de la configuration retenue (les 30 bras du bras retenu de l'A/B 2) : `python3 -I bench/smoke/baseline.py <runs> --out bench/smoke/baseline/gpt-6.1-terra.json`, comme le 12/11 (`bench/smoke/ab/rlm-spawn-20261112.md`). L'ancienne reste dans l'historique git.

### Baseline du 14/11/2026

Écrite par la même procédure que la baseline `4024960` du 12/11 :

`python3 -I bench/smoke/baseline.py <les 30 bras A de bench/smoke/runs/ab-20261114T140538Z, dans l'ordre des passes de ab.json> --out bench/smoke/baseline/gpt-6.1-terra.json`

La commande complète est dans le champ `command` du fichier.

- **Source** : bras A de l'A/B 2 (`default.json`), binaire `80e20ea`, sha256 `099f54d354c3…`, code du banc à `67c05e592`. L'en-tête donne le même modèle, le même effort, le même catalogue (`models_bundled_sha256`) et la même version de prix que l'ancienne baseline.
- **En-tête** : `rlmMaxDepth` n'est plus un réglage explicite, puisque 0 est devenu le défaut le 12/11. `settings_non_default` ne garde que `cacheRetention: short`.
- **Tâches stables** : les 8 mêmes qu'avant, à savoir `node-koa-2029`, `node-testsprite-356`, `node-tough-cookie-630`, `py-graphiti-2030`, `py-langextract-584`, `py-shortuuid-118`, `py-tldextract-368` et `py-tldextract-383`. `node-filesize-376` (1/3) et `node-little-coder-102` (0/3) restent non stables.
- **Agrégat** (coût rapporté des tâches stables) : coût moyen **0,3377 $** (passes : 0,3551, 0,3357 et 0,3222 $).
- **CV** : 0,0672 en indépendance, 0,0490 par passes ; la valeur retenue est **0,0672**. Seuil de l'alerte agrégée de `compare.py` : **±13,4 %**.
- **Écart avec l'ancienne baseline** (`4024960` : 0,3953 $, CV 0,0605, seuil 12,1 %) : **−14,6 %** sur le coût moyen des tâches stables.
  - Par tâche : de −35,9 % (`node-koa-2029`) à +18,8 % (`py-shortuuid-118`).
  - Les deux baselines diffèrent de date, de binaire (`69cc894` contre `80e20ea`) et de code (PR 1 et 2 de YA-03, #28, #30). L'écart n'est pas un effet isolé.
- L'ancienne baseline reste dans l'historique git (`4024960`). Les commandes de mesures de l'A/B 1 la relisent par `git show`.
