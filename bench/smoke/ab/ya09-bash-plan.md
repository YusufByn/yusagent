# A/B YA-09 `bashWaitSeconds` (1 s contre 30 s) : plan et règle de verdict

> **Plan non exécuté.** Décision de Yusuf du 15/11/2026 à 21h40 (journal du CDC) : les choix d'Unharnessed sont repris directement, sans A/B, et mesurés plus tard sur Terminal-Bench. Cet A/B est donc sauté, et **le défaut de `bashWaitSeconds` passe à 30 s** sans verdict mesuré (écart à la règle « toute optimisation justifiable par une mesure »). Le plan, la graine et les scripts restent versionnés pour une mesure ultérieure. Les sessions créées avant le réglage gardent 1 s. Dans ce qui suit, le « bras A » (`default.json`) correspondrait désormais à 30 s : un A/B futur devrait prendre `{"bashWaitSeconds": 1}` comme bras de contrôle.

Règles fixées et versionnées avant tout run réel. Étape 1 du levier `bash()` de la « Proposition pour YA-09 » (journal du CDC, 14/11/2026). Plan validé par Yusuf le 15/11/2026 : option B (fenêtre en réglage, 30 s), avec ses ajouts (source des 77 s, référence du prompt, scripts versionnés, durée médiane par lancement, écart 59 contre 70, notice MIT).

## Question

`await bash(cmd)` rend la main après 1 s si la commande tourne encore et renvoie un instantané (`prime-agent-runtime/src/rlm/bash.py:69`, `:1076`). Le prompt le dit (`crates/pa-core/src/prompts/system_prompt.rs:343`) et demande de finir le tour pour attendre la notification `[bash-done]`. Chaque commande de plus de 1 s coûte donc au moins un appel au modèle de plus : une relève du handle, ou un réveil par la notification.

L'A/B mesure si une fenêtre de 30 s réduit les appels au modèle de la session principale sur `gpt-6-luna`, sans perte de résolution ni hausse de coût. Même binaire, seul `bashWaitSeconds` change.

## Constats d'entrée

Run YA-14 `bench/smoke/runs/ab-20261114T201248Z` (bras A : yusAgent `80e20ea`, `default.json` ; bras B : Codex CLI 0.169.0), rapport `bench/smoke/ab/ya14-codex-20261114.md`, « Attentes de `bash()` ». Sur les 30 lancements yusAgent :
- 70 cellules revenues avant la fin de leur commande, 54 tours de relève, 64 notifications `[bash-done]` ;
- la règle « ne pas relever un handle encore en cours, finir le tour » n'est pas suivie par `gpt-6-luna` (par exemple `node-testsprite-356`, passe 3, 19 tours de relève) ;
- Codex attend ses commandes : sur 252 `exec_command`, 126 donnent `yield_time_ms` dans l'appel (94 à 10 000, 27 à 30 000, 5 à 1 000).

### Écart de comptage : 59 contre 70 cellules

La lecture du 15/11 (hors dépôt) a compté 59 cellules revenues avant la fin, le rapport YA-14 en compte 70. Les deux comptes portent sur les mêmes cellules, avec deux règles de détection :
- **59** : cellules dont le stderr ou le texte porte l'avis `still running after`. Cet avis n'est écrit que **3 fois par kernel** (`_MAX_DEGRADE_HINTS`, `prime-agent-runtime/src/rlm/bash.py:86-101`) ;
- **70** : la règle de `ya14-codex-20261114-mesures.py:128-129`, qui ajoute les résultats dont le texte porte la repr `BashSnapshot running`. Les 11 cellules de plus sont des instantanés rendus après les 3 avis du kernel.

C'est la règle à 70 qui fait foi, ici et dans `ya09-bash-mesures.py`. Reproduction :

```bash
python3 -I bench/smoke/ab/ya09-bash-durees.py bench/smoke/runs/ab-20261114T201248Z
```

Sortie : « cellules revenues avant la fin : **70** = 59 avec l'avis `still running after` + 11 avec la seule repr `BashSnapshot running` ».

## Choix de la valeur : 30 s

Durées des commandes du run YA-14, même commande que ci-dessus :

| | Source | Médiane | p90 | p95 | p99 | Max | Sous 30 s |
|---|---|---|---|---|---|---|---|
| Codex CLI | `Wall time` des 348 sorties de tools (arrondi au dixième) | 0,1 s | 0,4 s | 3,2 s | 20,3 s | 31,0 s | 345 sur 348 |
| yusAgent | 43 commandes revenues avant la fin, de `tool_started` à leur notification (**borne haute**) | 6,8 s | 26,7 s | 29,6 s | 39,1 s | 39,1 s | 41 sur 43 |

- 21 notifications sur 64 n'ont pas de cellule appariée (commande lancée sans `await`, ou code qui ne contient pas la commande telle quelle) : leur durée n'est pas comptée.
- 30 s couvre p95 et p99 des deux bras. Restent au-delà : 3 sorties Codex et 2 commandes yusAgent sur 30 lancements chacun.
- 60 s couvrirait tout, mais allonge les silences sans requête sur le WebSocket (voir « Risques »).

## Réglage

- `bashWaitSeconds` dans `settings.json`, en secondes entières, au moins 1. **Absent : 1 s, comme upstream.** Une valeur hors de ce type (0, négatif, décimal, chaîne) est ignorée au chargement des réglages (`settings/load.rs`, champ mal typé = absent).
- **Figé par session** dans l'en-tête, champ `yaRequest` (`crates/pa-core/src/session_engine/ya_request.rs`). Il n'y est écrit que s'il ne vaut pas 1 : l'en-tête d'une session par défaut est celui d'avant le réglage, et un `yaRequest` sans le champ vaut 1 s. Reprise : la valeur figée est gardée, avec un avertissement si les réglages courants diffèrent. Hérité au fork (comme `yaRequest`).
- **Kernel** : après `_set_completion_notices_wired(True)`, le bootstrap pose `_YIELD_AFTER_SECONDS` du module `rlm.bash` (`crates/pa-core/src/kernel/bootstrap/ya_bash_wait.rs`). Le runtime Python n'est pas modifié. Sans le gestionnaire `bash.completed` (l'attente est alors sans borne) ou en `shellTool: "native"` (pas de `bash`), rien n'est posé.
- **Prompt** : à 1 s, la ligne `await bash(cmd)` est celle d'upstream, octet pour octet. À 30 s, seule la fenêtre change : « `await bash(cmd)` waits at most 30 seconds. » (`crates/pa-core/src/prompts/ya_bash_wait.rs`). Le reste de la ligne (instantané, finir le tour, `[bash-done]`) ne change pas.
- **Corps de requête et tools : inchangés.**

### Préfixe

- `default.json` : prompt système, tools et corps identiques à `main`. La ligne `await bash(cmd)` est celle d'upstream à 1 s comme sans réglage (`crates/pa-core/src/prompts/ya_bash_wait.rs`, test `the_upstream_line_carries_the_window_once`) ; l'en-tête d'une session par défaut ne change pas (`ya_request.rs`, test `bash_wait_defaults_to_one_second_and_is_written_only_when_set`).
- `bash-wait-30.json` : **prompt système différent** de celui de `main` par la seule fenêtre de la ligne `await bash(cmd)`, stable dans la session (valeur figée). Tools et paramètres identiques à ceux du bras A. Vérifié par `crates/pa-core/tests/prefix_stability.rs`, `codex_bash_wait_changes_only_the_await_line`.

## Risques

### WebSocket fermé en 1011 (constat du 15/11)

Sources : ticket 09, « Constat de l'A/B YA-09 `reasoningSummary` » (`docs/tickets/09-ya03-base.md:183-198`), et `bench/smoke/ab/ya09-summary-20261115.md`, « Bascule SSE de `node-testsprite-356` ». Deux durées y figurent, qui ne mesurent pas la même chose :

| Durée | Mesure | Bornes |
|---|---|---|
| **77 s** | Silence sans requête sur la connexion WebSocket avant sa fermeture par le serveur (`1011 keepalive ping timeout`) | Fin de la dernière réponse, la réponse finale de l'agent, à 16:28:05Z → fermeture à 16:29:22Z. La connexion avait 203 s. Retrouvé au proxy par `ya09-bash-mesures.py` : 77,2 s, de la dernière `response` au `ws_close` |
| **109 s** | Attente de l'agent : temps entre sa réponse finale et le réveil par `[bash-done]` d'un `npm test` lancé en arrière-plan | 16:28:05Z → 16:29:54Z. La fermeture tombe pendant cette attente, 32 s avant le réveil |

- Aucune des deux n'est un seuil. Le délai de ping du serveur n'est pas connu. D'autres silences entre appels ont dépassé 77 s au banc sans fermeture (`sincePrevMs` max 110 151 ms, `bench/smoke/README.md`, « Garde-fou d'inactivité »).
- Cause probable, côté client et non vérifiée : le worker WebSocket ne lit pas le flux entre deux requêtes, donc ne répond pas aux pings (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:229-235`). Correction possible dans la couche providers (règle 3), hors de ce levier.
- **Effet de la fenêtre** : une cellule qui attend sa commande garde le WebSocket sans requête jusqu'à 30 s (plus le temps de la cellule). Avec 1 s, le même silence existe déjà après la fin du tour, pendant l'attente de `[bash-done]`. La fenêtre déplace le silence dans la cellule ; elle ne le crée pas.
- Suivi : silences sans requête, bascules SSE et échecs de transport sont **affichés sans décider** (ci-dessous). Une bascule n'écarte pas une unité (règle d'écart du README).

### `session/cancel` en moins de 1 s (YA-10)

Critère du CDC (`docs/yusAgent-cdc.md:359`) : `session/cancel` stoppe le tour en moins de 1 s, cellule en cours et sous-process `bash()` compris. Avec 30 s, une annulation a plus de chances de tomber pendant un `await bash(...)`. L'annulation d'un `await` envoie SIGTERM au groupe, attend 0,5 s (`_CANCEL_TERM_GRACE`), puis SIGKILL (`bash.py:44-47`).

Mesure du runtime seul (kernel Python, fenêtre de 30 s, annulation 0,5 s après le début, 5 essais) : `sleep 30` s'arrête en 0,022 s ; `trap '' TERM; sleep 30` en 0,505 à 0,528 s. Tests : `prime-agent-runtime/test/test_ya_bash_wait.py` (les deux sous 1 s).

```bash
cd prime-agent-runtime && uv run --python 3.11 python -m unittest test.test_ya_bash_wait -v
```

Limite : le chemin complet (ACP `session/cancel` → interruption du kernel) n'est pas mesuré ici.

## Bras

| Bras | Réglages | Différence |
|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | fenêtre de 1 s (comme `main`) |
| B | `bench/smoke/settings/bash-wait-30.json` (`{"bashWaitSeconds": 30}`) | fenêtre de 30 s ; ligne `await bash(cmd)` du prompt à 30 s |

`default.json` vaut aujourd'hui `reasoningSummary` `off` (décision du 15/11/2026) dans les deux bras.

## Protocole

- `gpt-6-luna`, provider `openai-codex` (WebSocket), effort `high`.
- Les 10 tâches du banc, **3 passes entrelacées par tâche** (`tools/ab.py`), ordre des bras tiré au sort à chaque passe.
- **Graine : 2027638424** (tirée de `os.urandom` le 15/11/2026 à 19:42Z, avant le run).
- Binaire : artefact de `ya-build-macos-arm64.yml` au dernier commit de code de la branche `ya-09-bash`, installé par `tools/install-bin.sh`. SHA et sha256 notés ci-dessous avant le premier run.
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité de 300 s. La fenêtre de 30 s reste loin de ce garde-fou.
- Quota : mêmes conditions que `ya14-codex-plan.md`, « Quota ».
- **Paires écartées** : règle d'écart du README (erreur provider, statut `stalled`) ; `ab_stats.py` l'applique.

```bash
python3 -I bench/smoke/tools/ab.py <dossier du binaire> <sha> --b bench/smoke/settings/bash-wait-30.json \
  --model gpt-6-luna --effort high --seed 2027638424
python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --baseline bench/smoke/baseline/gpt-6-luna.json
python3 -I bench/smoke/ab/ya09-bash-mesures.py bench/smoke/runs/ab-<horodatage>
```

`baseline/gpt-6-luna.json` mesure l'ancien défaut (`summary: "auto"`, passation du 15/11) : elle ne sert ici qu'à la liste des tâches stables.

### Binaire et chauffe

- **Binaire** : branche `ya-09-bash` à `8755b2e5909e81666cf14fce143e631f1b0e4212` (CI `ci.yml` en brouillon, jobs lourds sautés : les tests Rust restent à passer). Build `ya-build-macos-arm64.yml` 43754394374, version `v0.10.2`.
  - sha256 de l'archive `prime-agent-0.10.2-darwin-arm64.tar.gz` : `045480e66374ae4f4962b21890cd1dd563fe867b3643df22cf3b3aca764d3e90` (`SHA256SUMS`, vérifié par `install-bin.sh`).
  - sha256 de l'exécutable `prime-agent` : `26b92178d5070e13188ac5efcbd5687c6ab7bc12b70373dc6b1867c6695ed2f0` (manifeste, `executableSha256`, et `shasum -a 256`).
  - **Binaire de la chauffe, pas celui de l'A/B** : un commit de code suit (`prime-agent prompt` affiche la fenêtre réglée, `crates/pa-cli/src/prompt_command.rs`, hors chemin d'une session). L'A/B tourne sur le build de `main` après merge ; son SHA et ses sommes sont notés ci-dessous. Contrôle avant le run : `git diff 8755b2e59 <SHA de main> -- crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock` ne doit montrer que ce fichier.
- **Binaire de l'A/B** : à noter après le merge (build de `main`).
- **Chauffe** (accord de Yusuf du 15/11/2026), hors A/B : `node-testsprite-356`, bras B, `gpt-6-luna`, effort high, `--log-bodies`.

  ```bash
  DO_NOT_TRACK=1 PI_OFFLINE=1 python3 -I bench/smoke/run.py --binary <dossier>/prime-agent --model gpt-6-luna \
    --effort high --settings bench/smoke/settings/bash-wait-30.json --tasks node-testsprite-356 --log-bodies
  python3 -I bench/smoke/tools/check_warmup.py <run>/tasks/node-testsprite-356/run-1 <run>/report.json
  ```

  Points vérifiés : en-tête `yaRequest {"reasoningSummary": "off", "bashWaitSeconds": 30}` ; ligne `await bash(cmd)` à 30 s dans chaque corps ; au moins une attente réelle de plus de 1 s rendue complète ; plus long silence sans requête sur le WebSocket ; aucune mention de 1011.

  **Chauffe du 15/11/2026** sur `8755b2e`, run `bench/smoke/runs/20261115T195452Z-8755b2e-gpt-6-luna` (non versionné), tâche **résolue**, 40 appels, phase agent 388 s.

  | Point | Résultat |
  |---|---|
  | En-tête de session | `yaRequest {"reasoningSummary": "off", "bashWaitSeconds": 30}` ; `yaTools` inchangé |
  | Ligne `await bash(cmd)` | « waits at most 30 seconds. » dans les 40 corps |
  | `reasoning` des corps | `{"effort": "high"}`, 40 sur 40 (défaut `off` inchangé) |
  | Attentes réelles | 4 cellules `await bash(...)` rendues complètes après plus de 1 s (2,6 à 13,8 s) ; 2 cellules revenues à 30 s (30 007 et 30 014 ms), toutes deux sur `npm test` (suite complète) |
  | Relèves, notifications | 2 relèves, 2 notifications `[bash-done]` |
  | Silences sans requête sur le WebSocket (> 5 s) | 13,8 ; 30,0 ; 47,0 ; 30,0 ; 73,9 s |
  | **Fermeture 1011** | **oui** : `1011 keepalive ping timeout`, puis 3 appels en SSE (corps complets), tâche résolue |
  | Rapport | 0 invalidation, 0 miss serveur, 0 compaction, taux de cache de la session 0,958 ; `err.log` vide ; scan de secrets propre |

  **Chronologie de la fermeture** (session et proxy du run) : à 19:59:12Z, la cellule lance la suite complète (`npm test > /tmp/testsprite-npm-test.log …`). Elle revient à 30 s avec un instantané (19:59:42Z). Le modèle relève deux fois le handle, puis finit son tour à 19:59:51Z. Le serveur ferme le WebSocket à 20:01:04Z, **73,9 s après la dernière réponse**. La notification `[bash-done]` arrive à 20:01:12Z (commande d'environ 120 s, borne haute). Plus tôt, un premier `npm test` (19:57:10Z → notification à 19:59:04Z, environ 114 s) avait laissé un silence de 47,0 s sans fermeture.

  **Lecture** : les deux longs silences sont des fins de tour qui attendent `[bash-done]`, pas des attentes dans la cellule ; les silences dus à la fenêtre font 30 s au plus et n'ont pas fermé la connexion. C'est le mécanisme du constat du 15/11 (77,2 s), pas un effet de la fenêtre. Les deux fermetures observées tombent après 73,9 et 77,2 s de silence, et un silence de 47,0 s est passé : cela suggère un délai serveur de l'ordre de 60 à 75 s, **hypothèse sur deux cas**, non vérifiée. Sur cette tâche, la suite complète dure environ 2 min, au-delà de 30 s : la fenêtre ne l'absorbe pas. Les durées du run YA-14 (max 39 s) ne contenaient pas ces suites complètes.

  **Conséquence pour l'A/B** : des bascules 1011 sont attendues dans les deux bras sur `node-testsprite-356`. Elles restent affichées sans décider (règle d'écart du README). La correction (réponse aux pings entre deux requêtes, `websocket.rs:229-235`) relève de la couche providers, hors de ce levier.

## Règle de verdict

**Verdict par défaut : 1 s** (bras A). La charge de la preuve est sur le bras 30 s (B). On applique les règles dans l'ordre, et la première qui s'applique décide.

1. **Résolution** (règle de YA-03, `ya03-plan.md`) : totaux de « Tâches résolues par passe » sur les paires valides (`ab_stats.py`, « Par bras »). Seuil fixé avant le run, pas un test statistique.
   - B résout **au moins 3 tâches de moins** que A : **A gardé**.
   - B résout **au moins 3 tâches de plus** que A : **B retenu**.
2. **Coût**, sans verdict de résolution : coût à cache théorique des **tâches stables de `bench/smoke/baseline/gpt-6-luna.json`**, écart B/A − 1, IC 95 % par bootstrap sur les tâches (`ab_stats.py`, ligne « stables, théorique (décision) », avec `--baseline`).
   - IC entièrement au-dessus de 0 (B plus cher) : **A gardé**.
   - IC qui exclut 0 **et** |écart| > 10 %, avec B moins cher : **B retenu**.
   - Sinon : coût **non distinguable**, on passe aux appels.
3. **Appels au modèle**, si le coût n'a pas tranché : B est retenu seulement si les **deux** conditions sont remplies.
   - **Appels de la session principale par lancement**, appariés par tâche : la borne haute de l'IC 95 % de B/A − 1 est sous 0 **et** l'écart est sous −10 % (sensibilité du banc, journal du CDC du 14/11/2026).
   - **Garde-fou de durée** : la borne haute de l'IC 95 % de la phase agent B/A − 1 est à +10 % au plus.
   - Sinon : **A gardé**.

   Ligne « condition remplie » de `ya09-bash-mesures.py`, population « toutes ».

Verdicts possibles : A gardé (résolution, coût ou appels) ; B retenu (résolution, coût ou appels).

### Méthode des métriques de la règle 3

Script `bench/smoke/ab/ya09-bash-mesures.py` (bibliothèque standard, lecture seule), versionné avant le run.

- **Appels** : `calls.by_kind.main` du résultat de la tâche (`report.json`). Moyenne par tâche sur ses passes valides dans chaque bras ; écart B/A − 1 sur la somme des moyennes par tâche ; écart moyen par tâche en appels.
- **Phase agent** : `agent.seconds`, même méthode.
- **IC 95 %** : bootstrap sur les tâches (`ab_stats.bootstrap`, 10 000 tirages, graine 0).
- **Population** : toutes les tâches avec au moins une paire valide. Le sous-ensemble « résolues par les deux bras » est affiché sans décider.
- **Paires écartées** : `ab_stats.units_from_plan`.
- **Validation** sur l'A/B `reasoningSummary` (`python3 -I bench/smoke/ab/ya09-bash-mesures.py bench/smoke/runs/ab-20261115T153500Z`, A = `auto`, B = `off`) : le script retrouve les chiffres du rapport `ya09-summary-20261115.md` (29 paires retenues, 22,4 contre 24,3 appels par lancement, phase agent −26,1 % [−41,7 ; −8,1]). Il retrouve aussi le constat 1011 dans le bras B : silence de 77,2 s, 1 bascule 1011, 1 requête en SSE, 27 776 tokens en cache sur 28 079 (taux 0,989).

### Affiché sans décider

- **Durée de la phase agent par lancement : moyenne et médiane**, par bras.
- Par lancement : cellules revenues avant la fin de leur commande (règle à 70 ci-dessus), tours de relève, notifications `[bash-done]`. Mécanisme attendu : les trois baissent en B.
- Silence sans requête sur le WebSocket (fin d'une réponse → requête suivante ou fermeture) : maximum, médiane des maxima par lancement, nombre de silences de plus de 60 s.
- Lancements avec bascule SSE ou échec de transport.
- **Par bras : nombre de bascules 1011** (diagnostics `provider_transport_failure` de code 1011, un par horodatage), **nombre de requêtes en SSE** et **taux de cache de ces requêtes** (`cacheRead` / (`input` + `cacheRead`) du journal d'usage).
- Sorties d'`ab_stats.py` : coût rapporté et pessimiste, taux de cache, miss serveur, edits au premier essai, cellules en erreur.

## Ce que le banc peut détecter

Estimations tirées des runs cités, pas des mesures.

- **Effet attendu sur les appels** : au plus les relèves et les réveils. Sur le bras `off` de l'A/B `reasoningSummary` (config de `default.json`) : 1,2 relève et 2,1 notifications par lancement, pour 24,3 appels. Si la fenêtre les supprimait toutes, l'écart serait d'environ −14 %. Une commande de plus de 30 s, ou lancée sans `await`, garde sa notification. L'effet réel sera plus faible.
- **Bruit** : dans ce même A/B, l'IC des appels par lancement fait environ ±13 % ([−3,9 ; +21,6] autour de +7,9 %). Un effet de −14 % est à la limite de la détection : un verdict « A gardé » faute de preuve est possible même si l'effet existe.
- **Résolution** : on n'attend pas d'effet. Le seuil de 3 tâches ne détecte qu'une forte régression.
- **Durée** : une cellule qui attend sa commande garde le temps de la commande ; un réveil coûte un appel au modèle (latence fixe d'environ 1 s sans raisonnement, plus le temps de génération). Effet attendu proche de zéro ou négatif, d'où le seul garde-fou.

## Portée

- **Un verdict B vaut pour `gpt-6-luna`, le banc et la valeur de 30 s.** Le défaut produit reste 1 s jusqu'à une décision au journal du CDC.
- Le risque 1011 et le critère `session/cancel` de YA-10 sont à reprendre avant tout changement de défaut.
- Leviers suivants, un A/B chacun, à planifier après ce verdict : règle de regroupement (lecture et exploration dans une même cellule, edit et test dans le même tour), `editTool: kernel` sur `gpt-6-luna`, `parallel_tool_calls` (`true` en dur, `crates/pa-ai/src/providers/openai_codex_responses/mod.rs:664`).
