# A/B YA-09 cache du prompt v2 (`promptVersion` 2 contre 1) : plan et règle de verdict

Règles fixées et versionnées avant tout run réel, à la demande de Yusuf (16/11/2026). Yusuf lance le run lui-même.

## Question

Les chauffes du 16/11/2026 (`docs/tickets/12-ya09-ws-keepalive.md`) montrent des appels en suite de conversation avec `cacheRead = 0` (miss) sur les binaires du prompt v2. Comptés par `bench/smoke/tools/cache_calls.py` :
- `f81f980` (PR B, 3 tâches) : 3 miss sur 50 appels hors premier (6 %), et 9 relectures partielles (18 %, définition ci-dessous) ;
- `cce48e6` (`py-silence-001`, 2 chauffes) : 4 miss sur 5.

4 des 7 miss tombent après un silence de 0 s : les longs silences n'expliquent pas tout. Ce sont des chauffes isolées, pas une mesure.

**Le prompt v1 a aussi des miss.** Taux par appel sur les 5 tâches de cet A/B, avec la même définition :

| Source (prompt v1) | Miss | Relectures partielles |
|---|---|---|
| Baseline Luna du 14/11 (`80e20ea`, 15 runs) | 8/297 (2,7 %) | 7/297 (2,4 %) |
| A/B `reasoningSummary` du 15/11 (`7086711`, deux bras, 30 runs) | 17/609 (2,8 %) | 41/609 (6,7 %) |

```bash
python3 -I bench/smoke/tools/cache_calls.py --plan bench/smoke/runs/ab-20261115T153500Z/ab.json   # toutes les tâches de cet A/B
```

Le constat du 16/11 (« 0 miss sur 43 appels en v1 ») venait de deux chauffes seulement. La question devient : **le prompt v2 a-t-il nettement plus de miss que le v1, même binaire, même jour ?**

## Bras

| Bras | Réglages | Différence |
|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | prompt version 2 (défaut de `main`) |
| B | `bench/smoke/settings/prompt-v1.json` (`{"promptVersion": 1}`) | prompt version 1 : couches `core`, `usage` et `opinionated` d'upstream, description longue de `python_repl` (`docs/fork-changes.md`, « Prompt version 2 ») |

Écart de réglages calculé par `ab.py` : `{"promptVersion": [null, 1]}`. Tout le reste est identique : un seul tool (`python_repl`), `edit()` du kernel, `bashWaitSeconds` 30, avis groupés, `reasoningSummary` off, `rlmMaxDepth` 0. Le levier testé est donc « prompt v2 entier » (instructions et description du tool), pas une phrase isolée.

## Protocole

- Binaire de `main` `318f3e118b71bc5734ac81507d001ecf82adaf87` : build `ya-build-macos-arm64` 43857318671, version `v0.10.2`, sha256 de l'exécutable `1c1923832681f6179e485a4797a3de85942644078b2e95a35f15ca4f35685dcb` (manifeste et `shasum -a 256`). Installation : `bash bench/smoke/tools/install-bin.sh 43857318671 318f3e118b71bc5734ac81507d001ecf82adaf87`.
- `gpt-6-luna`, provider `openai-codex` (WebSocket), effort `high`.
- **5 tâches** stables de la baseline Luna (résolues 3 fois sur 3) : `node-koa-2029`, `node-tough-cookie-630`, `py-langextract-584`, `py-tldextract-368`, `py-tldextract-383`. Écartées : `node-testsprite-356` (instable), `py-shortuuid-118` (sessions courtes, environ 10 appels : peu d'appels par unité), et les tâches instables (`node-filesize-376`, `node-little-coder-102`, `py-graphiti-2030`).
- **4 passes entrelacées** (`tools/ab.py`), même jour (2 passes au premier jet du plan, passées à 4 par Yusuf le 16/11/2026 avant le run) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort.
- **Graine : 145634427** (tirée de `os.urandom` le 16/11/2026 à 11:53Z, avant le run). Ordre obtenu par `--dry-run` (16/11/2026, 12:20Z) : passe 1 `koa AB, tough-cookie BA, langextract AB, tldextract-368 BA, tldextract-383 BA` ; passe 2 `koa AB, tough-cookie AB, langextract AB, tldextract-368 AB, tldextract-383 AB` ; passe 3 `koa AB, tough-cookie AB, langextract BA, tldextract-368 AB, tldextract-383 BA` ; passe 4 `koa AB, tough-cookie BA, langextract BA, tldextract-368 BA, tldextract-383 AB`. Les passes 1 et 2 sont celles du premier jet (même graine).
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixés par le runner), garde-fou d'inactivité de 300 s.
- Quota : mêmes conditions que les A/B précédents (`ya14-codex-plan.md`, « Quota ») : fenêtre de 5 h utilisée à 10 % au plus avant de lancer, aucun autre usage du compte pendant l'A/B, arrêt sur erreur de quota.
- **Unités écartées** : règle du README (« A/B : erreurs provider ») appliquée par `ab_stats.py` (`units_from_plan`), reprise telle quelle par `cache_calls.py --plan`. Une erreur provider sur un bras écarte l'unité (la tâche de cette passe, ses deux bras).

Commandes (validées par `--dry-run` le 16/11/2026 à 12:20Z avec `--passes 4`, contrôles du binaire passés) :

```bash
bash bench/smoke/tools/install-bin.sh 43857318671 318f3e118b71bc5734ac81507d001ecf82adaf87
python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/318f3e118b71bc5734ac81507d001ecf82adaf87 318f3e118b71bc5734ac81507d001ecf82adaf87 \
  --a bench/smoke/settings/default.json --b bench/smoke/settings/prompt-v1.json \
  --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383 \
  --passes 4 --model gpt-6-luna --effort high --seed 145634427
python3 -I bench/smoke/tools/cache_calls.py --plan bench/smoke/runs/ab-<horodatage>/ab.json
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-<horodatage>/ab.json --baseline bench/smoke/baseline/gpt-6-luna.json
```

Coût estimé (estimation) : 40 lancements, environ 0,4 $ en coût rapporté. Base : l'A/B du 15/11 a coûté 0,25 et 0,28 $ par bras pour 29 unités retenues, soit environ 0,009 $ par lancement. Le quota de l'abonnement est la vraie limite. Durée : environ 1 h 25 (estimation : 7 406 s pour les 60 lancements du 15/11, soit environ 123 s par lancement, `ab.json`, `overhead.wall_total_s`).

## Métriques

Définitions de `bench/smoke/tools/cache_calls.py`, par appel d'une session de tâche, l'appel précédent étant celui de `seq` inférieur dans la même session :
- **miss** : `cacheRead = 0` hors premier appel de la session ;
- **relecture partielle** : `0 < cacheRead < ⌊prompt précédent⌋₁₂₈ − 256`, hors premier appel. Le backend relit d'ordinaire le prompt précédent arrondi au bloc de 128 tokens, parfois un bloc de moins : la marge de deux blocs écarte ce décalage ;
- **taux de miss** d'un bras : Σ miss / Σ appels hors premier, sur les unités retenues ;
- **taux de cache de la session** : Σ `cacheRead` / Σ prompt (`input + cacheRead + cacheWrite`), global et médiane par unité.

Affichés sans entrer dans le verdict : coût rapporté, tâches résolues, appels, compactions, et par unité les miss, relectures partielles et taux de cache de chaque bras. `ab_stats.py` donne en plus le coût à cache théorique, les tokens et les incidents.

## Règle de verdict (fixée avant le run)

Écart **d = taux de miss de A (v2) − taux de miss de B (v1)**, en points, sur les unités retenues. IC 95 % par bootstrap sur les unités : 10 000 tirages avec remise des unités, recalcul de d sur chaque tirage, graine 0 (`cache_calls.py --plan`, sortie reproductible). Le bootstrap sur les unités tient compte de ce que les miss d'une même session ne sont pas indépendants.

| Résultat | Verdict |
|---|---|
| d ≥ 3 points **et** borne basse de l'IC > 0 | écart net : **le cache en v2 devient un ticket prioritaire** |
| d ≥ 3 points, IC qui contient 0 | non concluant : Yusuf décide (passes en plus, ou ticket quand même) |
| d < 3 points | **pas d'écart net détecté** : pas de ticket prioritaire sur ce point. Le rapport affiche la borne haute de l'IC : l'écart réel peut aller jusqu'à cette valeur |

**Pourquoi 3 points** : le taux v1 mesuré sur ces 5 tâches est de 2,7 à 2,8 % (tableau ci-dessus). 3 points de plus, c'est environ le double. C'est aussi la moitié du taux vu sur `f81f980` (6 %) : un écart de cet ordre se verrait en coût.

**Puissance (estimation, sans la corrélation intra-session, qui l'abaisse)** : environ 20 appels hors premier par unité, soit environ 400 par bras sur 20 unités. Avec 2,8 % contre 5,8 %, l'erreur type de d vaut environ 1,4 point, et la demi-largeur de l'IC environ 2,8 points. Un écart réel de 4 points sortirait « net » environ 8 fois sur 10 ; un écart réel de 3 points, environ une fois sur deux. Un « pas d'écart net détecté » n'exclut donc pas un écart réel : sa borne haute dit jusqu'où il peut aller.

**Vérification de l'outil** : sur l'A/B `reasoningSummary` du 15/11 (prompt v1 dans les deux bras), `cache_calls.py --plan` donne d = +0,6 point, IC [−0,4, +1,6], verdict « pas d'écart net détecté » (borne haute +1,6 point), comme attendu pour deux bras de même prompt.

## Résultats

Run `bench/smoke/runs/ab-20261116T124625Z` (16/11/2026, 40 lancements, 20 unités retenues sur 20). Rapport : `bench/smoke/ab/ya09-cache-v2-20261116.md`.

- Miss : A (v2) 25/261 (9,6 %), B (v1) 16/376 (4,3 %). **d = +5,3 points, IC 95 % [+3,0 ; +7,8] : écart net, ticket prioritaire** (`docs/tickets/14-ya09-cache-miss.md`).
- Relectures partielles : A 35/261 (13,4 %), B 54/376 (14,4 %).
- Coût, sans décider : B / A − 1 = +40,1 % [+22,8 ; +60,7] en coût à cache théorique ; 20/20 résolues par bras.
