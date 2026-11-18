# YA-09 : A/B de l'en-tête `session-id` et contrôle de la dérive des relectures partielles, `gpt-6-luna` (17/11/2026)

Plan et règles versionnés avant les runs : `bench/smoke/ab/ya09-session-id-plan.md` (PR #50, mergée à `bf88ebc4a`).

**A/B de l'en-tête `session-id`, verdict de la règle : pas de gain net détecté, pas de dégradation. `codexSessionIdHeader` reste désactivé.**
- Taux de miss des appels dont le prompt précédent fait moins de 6 000 tokens : A (sans en-tête) **26/97 (26,8 %)**, B (avec en-tête) **24/99 (24,2 %)**.
- **d = +2,6 points, IC 95 % [−7,1 ; +11,3]**. Cas 5 de la règle : d < 10 points et borne basse ≤ 0. Le gain réel plausible va jusqu'à +11,3 points (borne haute).
- Garde-fou (miss global) : d_global = +3,2 points [−1,2 ; +7,5]. Pas de dégradation : d_global n'est pas ≤ −3 points.

**Contrôle de la dérive, verdict de la règle : dérive extérieure à notre binaire.**
- Le binaire du 14/11 (`80e20ea`), relancé tel quel le 17/11, fait **100/396 (25,3 %)** de relectures partielles, contre **7/297 (2,4 %)** le 14/11.
- 25,3 % ≥ 8,4 % : la cause est côté serveur ou dans les conditions du banc (compte, réseau, machine). Pas de ticket sur le binaire.

**Portée** : 5 tâches faciles, un seul modèle (`gpt-6-luna`, effort high), un seul jour. Le verdict porte sur les miss du cache serveur, pas sur la qualité.

## 1. A/B de l'en-tête `session-id`

### Protocole

- **Bras**, même binaire `bf88ebc4a5a2c586c4cc3756aeeb64216a49cfe8` (`main` après le merge de la PR #50, build `ya-build-macos-arm64` 44086728262, sha256 de l'exécutable `40981318f078…`, identique dans les deux bras d'`ab.json`, `binaries`) :
  - A : `bench/smoke/settings/default.json` (`{}`) : pas d'en-tête `session-id`.
  - B : `bench/smoke/settings/session-id-header.json` (`{"codexSessionIdHeader": true}`) : en-tête `session-id` = id de session, en plus de `session_id`.
  - Écart de réglages calculé par `ab.py` : `{"codexSessionIdHeader": [null, true]}`.
- **Code du banc** : `main` à `bf88ebc4a`, arbre propre (`commit`, `commit_dirty: false` d'`ab.json`). `ab.py` a vérifié que les sources livrées du binaire sont identiques à `bf88ebc` (`shipped_note`).
- **Modèle** : `gpt-6-luna`, provider `openai-codex` (WebSocket), effort `high`. Prompt v2 dans les deux bras.
- **Tâches** : `node-koa-2029`, `node-tough-cookie-630`, `py-langextract-584`, `py-tldextract-368`, `py-tldextract-383`.
- **Environnement** : `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité de 300 s.
- **Entrelacement** : 4 passes, graine `4199508898`. Les 20 ordres d'`ab.json` (`units`) sont ceux du plan : 12 AB et 8 BA.
- **Lancement** : par Yusuf le 17/11, de 13:11:30Z à 14:12Z environ. Au total, 3 664 s cumulées sur 40 lancements, dont 4,4 % de surcoût par lancement (`ab.json`, `overhead`).

Le run est dans `bench/smoke/runs/ab-20261117T131130Z/`, un dossier ignoré par git, sur la machine de Yusuf.

### Contrôle sur le fil avant le run (serveur de capture, aucun modèle)

Run `bench/smoke/runs/ab-20261117T130904Z` (mode `capture`, même binaire, `node-koa-2029`, 1 passe). Le contrôle passe : toutes les requêtes de B portent `session-id` égal à `session_id`, aucune de A.

```text
$ python3 -I bench/smoke/ab/ya09-session-id-mesures.py --fil bench/smoke/runs/ab-20261117T130904Z
# En-tête session-id sur le fil (capture) : bench/smoke/runs/ab-20261117T130904Z/ab.json

| Bras | Requêtes | avec `session-id` | égal à `session_id` | attendu |
|---|---|---|---|---|
| A | 4 | 0 | 0 | aucune (ok) |
| B | 4 | 4 | 4 | toutes (ok) |
```

### Mesures et verdict

Sortie complète de `ya09-session-id-mesures.py` (code de sortie 0) :

```text
$ python3 -I bench/smoke/ab/ya09-session-id-mesures.py bench/smoke/runs/ab-20261117T131130Z
# A/B en-tête session-id : bench/smoke/runs/ab-20261117T131130Z/ab.json

- A : `bench/smoke/settings/default.json` ; B : `bench/smoke/settings/session-id-header.json` ; binaire `bf88ebc4a5a2c586c4cc3756aeeb64216a49cfe8`
- Unités : 20, retenues 20

## Trace (`yaRequest.codexSessionIdHeader`)

- Bras A : 20 sessions, attendu False, différentes 0
- Bras B : 20 sessions, attendu True, différentes 0

## 1. Métrique principale

| Population | A (coupé) | B (en-tête) | d = A − B (points) | IC 95 % |
|---|---|---|---|---|
| miss, prompt précédent < 6000 | 26/97 (26.8 %) | 24/99 (24.2 %) | +2.6 | [-7.1 ; +11.3] |

## 2. Secondaires

| Population | A (coupé) | B (en-tête) | d = A − B (points) | IC 95 % |
|---|---|---|---|---|
| miss, tous les appels (garde-fou) | 37/246 (15.0 %) | 32/270 (11.9 %) | +3.2 | [-1.2 ; +7.5] |
| relectures partielles, tous les appels | 60/246 (24.4 %) | 67/270 (24.8 %) | -0.4 | [-9.9 ; +10.7] |
| miss, prompt précédent ≥ 6000 | 11/149 (7.4 %) | 8/171 (4.7 %) | +2.7 | [-2.2 ; +7.1] |
| relectures partielles, prompt précédent < 6000 | 23/97 (23.7 %) | 18/99 (18.2 %) | +5.5 | [-10.3 ; +23.3] |
| relectures partielles, prompt précédent ≥ 6000 | 37/149 (24.8 %) | 49/171 (28.7 %) | -3.8 | [-12.9 ; +4.9] |

## 3. Coût rapporté (unités retenues, affiché sans décider)

- A 0.1058 $, B 0.1124 $, B / A − 1 = +6.3 % (IC : `ab_stats.py --plan`)

## Verdict

- pas de gain net détecté : le réglage reste coupé ; gain réel plausible jusqu'à +11.3 points (borne haute)
```

Lecture selon la règle du plan (« Règle de verdict », cas examinés dans l'ordre) :
1. **Dégradation** : d_global = +3,2 points, donc pas ≤ −3 points, et la borne haute de son IC (+7,5) n'est pas < 0. Ne s'applique pas.
2. d = +2,6 < 10 points. Ne s'applique pas.
3. Borne basse −7,1 ≤ 0. Ne s'applique pas.
4. d < 10 points. Ne s'applique pas.
5. **d < 10 points et borne basse ≤ 0 : pas de gain net détecté.** `codexSessionIdHeader` reste désactivé (défaut inchangé). La borne haute, +11,3 points, borne le gain réel plausible.

La trace confirme les bras : 20 sessions de A à `"codexSessionIdHeader": false`, 20 sessions de B à `true`.

### Coût et activité (`ab_stats.py`, affiché sans décider)

Sortie de `python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-20261117T131130Z/ab.json --baseline bench/smoke/baseline/gpt-6-luna.json` (code de sortie 0) :

```text
A/B entrelacé par tâche ; unités (passe, tâche) : 20, retenues 20, écartées 0 ; tâches retenues : 5 (stables : 5)
Graine de l'ordre des bras : `4199508898` (--seed) ; écart de réglages (A → B) : `codexSessionIdHeader` null → true ; garde-fou d'inactivité 300 s
Métrique de décision : coût à cache théorique. Bootstrap sur les tâches, 10000 tirages, graine 0.

## Unités écartées (erreur provider)

- aucune

## Par tâche

Moyenne de chaque bras sur les unités retenues de la tâche.

| Tâche | Stable | Unités | A th. $ | B th. $ | Écart th. % | A $ | B $ | Écart % | A pess. $ | B pess. $ | Écart pess. % | Résolue A | Résolue B | Tours A | Tours B |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| node-koa-2029 | oui | 4 | 0.0026 | 0.0030 | +14.2 | 0.0035 | 0.0039 | +10.8 | 0.0042 | 0.0046 | +10.1 | 4/4 | 4/4 | 10.0 | 11.0 |
| node-tough-cookie-630 | oui | 4 | 0.0050 | 0.0046 | -8.0 | 0.0078 | 0.0067 | -14.8 | 0.0090 | 0.0077 | -15.1 | 4/4 | 4/4 | 18.5 | 16.2 |
| py-langextract-584 | oui | 4 | 0.0031 | 0.0033 | +8.0 | 0.0046 | 0.0052 | +13.4 | 0.0054 | 0.0061 | +13.0 | 4/4 | 4/4 | 11.8 | 16.0 |
| py-tldextract-368 | oui | 4 | 0.0022 | 0.0024 | +7.8 | 0.0032 | 0.0038 | +17.9 | 0.0037 | 0.0044 | +18.9 | 4/4 | 3/4 | 11.2 | 11.0 |
| py-tldextract-383 | oui | 4 | 0.0046 | 0.0061 | +31.5 | 0.0073 | 0.0085 | +17.2 | 0.0084 | 0.0097 | +14.7 | 4/4 | 4/4 | 15.0 | 18.2 |

## Écart agrégé

Somme B / somme A − 1 sur les tâches (moyennes par tâche ci-dessus), et moyenne des écarts appariés B − A en $ ; IC 95 % par bootstrap sur les tâches.

- **stables, théorique (décision) : +10.6 % [-3.3 ; +25.0]** ; écart apparié moyen +0.0004 $ [-0.0001 ; +0.0010] ; médiane des écarts par tâche +8.0 %
- stables, rapporté : +6.3 % [-7.8 ; +16.7] ; écart apparié moyen +0.0003 $ [-0.0005 ; +0.0009] ; médiane des écarts par tâche +13.4 %
- stables, pessimiste : +5.5 % [-7.8 ; +15.6] ; écart apparié moyen +0.0003 $ [-0.0005 ; +0.0010] ; médiane des écarts par tâche +13.0 %
- **toutes, théorique (décision) : +10.6 % [-3.3 ; +25.0]** ; écart apparié moyen +0.0004 $ [-0.0001 ; +0.0010] ; médiane des écarts par tâche +8.0 %
- toutes, rapporté : +6.3 % [-7.8 ; +16.7] ; écart apparié moyen +0.0003 $ [-0.0005 ; +0.0009] ; médiane des écarts par tâche +13.4 %
- toutes, pessimiste : +5.5 % [-7.8 ; +15.6] ; écart apparié moyen +0.0003 $ [-0.0005 ; +0.0010] ; médiane des écarts par tâche +13.0 %

## Par passe

Sommes sur les unités retenues de la passe.

| Passe | Unités | Coût | A $ | B $ | Écart % |
|---|---|---|---|---|---|
| 1 | 5 | théorique | 0.0192 | 0.0176 | -8.8 |
| 1 | 5 | rapporté | 0.0271 | 0.0222 | -18.4 |
| 1 | 5 | pessimiste | 0.0313 | 0.0252 | -19.5 |
| 2 | 5 | théorique | 0.0157 | 0.0205 | +31.1 |
| 2 | 5 | rapporté | 0.0227 | 0.0308 | +35.4 |
| 2 | 5 | pessimiste | 0.0263 | 0.0355 | +34.8 |
| 3 | 5 | théorique | 0.0171 | 0.0201 | +17.5 |
| 3 | 5 | rapporté | 0.0283 | 0.0332 | +17.4 |
| 3 | 5 | pessimiste | 0.0329 | 0.0386 | +17.2 |
| 4 | 5 | théorique | 0.0178 | 0.0190 | +6.8 |
| 4 | 5 | rapporté | 0.0276 | 0.0263 | -4.8 |
| 4 | 5 | pessimiste | 0.0322 | 0.0303 | -6.1 |

## Par bras

Sommes sur les unités retenues, moyennées par passe (résolution : par passe).

| Mesure | A | B | Écart % |
|---|---|---|---|
| Tâches résolues par passe | 5, 5, 5, 5 (total 20) | 5, 5, 5, 4 (total 19) | |
| Tours de la session principale | 66.5 | 72.5 | +9.0 |
| Tokens envoyés (input + cacheRead + cacheWrite) | 656121.5 | 763520.2 | +16.4 |
| Input non caché | 170137.5 | 171232.2 | +0.6 |
| Cache lu (rapporté) | 485984.0 | 592288.0 | +21.9 |
| Cache lu (théorique) | 585911.2 | 690271.0 | +17.8 |
| Cache écrit | 0.0 | 0.0 | n/a |
| Miss serveur | 9.2 | 8.0 | -13.5 |
| Part du coût rapporté des sous-agents | 0.0 % | 0.0 % | |
| Edits appliqués au premier essai (total) | 76/76 (100.0 %) | 69/69 (100.0 %) | |
| Patchs refusés hors workspace (total) | 0 | 0 | |
| Cellules en erreur d'appel (total, dont SyntaxError) | 10 (5) | 5 (0) | |

## Invalidations journalisées

- bras A (toutes unités) : aucune ; sessions touchées par le masquage : principales 0/20, sous-agents 0/0
- bras B (toutes unités) : aucune ; sessions touchées par le masquage : principales 0/20, sous-agents 0/0

## Incidents

Écartants (erreur provider) : section « Unités écartées ». Non écartants :

- aucun

## Surcoût par lancement

- 40 lancement(s) mesuré(s) : médiane 3.29 s, total 162.49 s sur 3664.23 s (4.4 %)
```

Le coût ne décide pas ici : la règle du plan porte sur les miss. B fait plus de tours (+9,0 %) et envoie plus de tokens (+16,4 %), avec un IC du coût théorique qui contient 0 (+10,6 % [−3,3 ; +25,0]). Rien n'indique que l'en-tête change le comportement du modèle : le corps des requêtes est identique entre les bras (test `codex_websocket_session_id_header_follows_the_setting`, `crates/pa-core/tests/prefix_stability.rs`).

### Unité B en échec : passe 4, `py-tldextract-368`

Run `bench/smoke/runs/ab-20261117T131130Z/arms/20261117T140347Z-bf88ebc-gpt-6-luna`, bras B en position 1 (ordre BA), 31,06 s au total (`ab.json`, `wall_s`), dont 27,25 s pour la tâche. Code de sortie 0. Le bras A de la même unité (`20261117T140418Z`) est résolu en 83,23 s.

**Cause : le modèle termine son tour sans rien modifier. Ce n'est ni une erreur provider, ni un blocage du flux, ni un problème du harness.**
- `report.md` du run : statut `failed`, 0/1 tâche résolue, 7 tours, 2 miss serveur (seq 2 et 3), aucune anomalie, aucun appel en SSE, aucun CONNECT refusé, aucune valeur secrète trouvée dans le dossier.
- Session principale (`tasks/py-tldextract-368/run-1/sessions/01a1e9d2-3e98-7711-a3b7-bd4c185b2e31.jsonl`) : 7 appels au modèle. Les 6 premiers finissent en `toolUse` et lancent des lectures seulement (`AGENTS.md`, `README.md`, `tldextract/tldextract.py`, `tests/main_test.py`, liste de `tests/`). Le 7ᵉ finit en `stop` (phase `final_answer`) avec un seul texte : « Je vais filtrer les composants vides avant de les joindre, puis ajouter des tests pour les quatre cas limites et le domaine ordinaire. » Aucun `edit()`, aucune écriture.
- Notation (`grading/test.out`, `grading/junit.xml`) : 38 tests réussis, 1 échec, `test_reverse_domain_name_notation_no_suffix` à `tests/main_test.py:486` : `assert '.localhost' == 'localhost'`. Le code de la tâche n'a pas été corrigé.
- `err.log` vide.

L'unité reste retenue : la règle du README n'écarte que les erreurs provider (`ab_stats.units_from_plan`, « Unités écartées : aucune »). Ses appels comptent dans les mesures : 6 appels hors premier côté B.

## 2. Contrôle de la dérive des relectures partielles en prompt v1

### Protocole

- **Binaire** : `~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/prime-agent`, sha256 `099f54d354c3daab653ec9502ae82658d32224600b616e95c5ffe4e2212ffc8e`, celui du 14/11 (`report.md`, `binary_sha256` ; manifeste `80e20ea5f353`, version `0.10.2-continuous.80e20ea5f353…`).
- **Réglages** : `bench/smoke/settings/default.json` (`{}`). Seul réglage hors défaut : `cacheRetention: short`, posé par le runner, comme le 14/11.
- **Outils** : Python 3.14.7, `node` v24.21.0, `uv` 0.12.17, comme le 14/11 et l'étape 0.
- **Code du banc** : `bf88ebc4a`, arbre propre.
- **Run** : `python3 -I bench/smoke/run.py --binary <80e20ea> --settings bench/smoke/settings/default.json --model gpt-6-luna --effort high --runs 4 --tasks <les 5 tâches>`. Lancé par Yusuf le 17/11 à 14:39:34Z, après l'A/B ; 2 511 s pour les tâches (`timing_s`).
- **Résultat** : 20/20 tâches résolues, aucune anomalie, aucune valeur secrète trouvée dans le dossier.

Le run est dans `bench/smoke/runs/20261117T143934Z-80e20ea-gpt-6-luna/`, sur la machine de Yusuf.

### Mesure

Sortie complète de `cache_causes.py` (code de sortie 0) :

```text
$ python3 -I bench/smoke/tools/cache_causes.py bench/smoke/runs/20261117T143934Z-80e20ea-gpt-6-luna
## 20261117T143934Z-80e20ea-gpt-6-luna

- Sessions 20, appels 416, hors premier 396 ; miss 27/396 (6.8 %), relectures partielles 100/396 (25.3 %)
- Hors premier : en suite de conversation 396/396 ; requêtes hors WebSocket 0 ; sur une connexion neuve 0 (dont miss 0, partiels 0)
- Prewarm (`generate: false`) : 0 ; premiers appels qui relisent du cache : 2/20

| taille du prompt précédent (tokens) | miss | partiels | appels | taux de miss |
|---|---|---|---|---|
| 6k-9k | 17 | 11 | 86 | 19.8 % |
| 9k-16k | 6 | 44 | 135 | 4.4 % |
| >= 16k | 4 | 45 | 175 | 2.3 % |

| âge dans la session (s) | miss | partiels | appels | taux de miss |
|---|---|---|---|---|
| < 10 | 13 | 0 | 46 | 28.3 % |
| 10-30 | 9 | 25 | 79 | 11.4 % |
| 30-60 | 4 | 24 | 91 | 4.4 % |
| >= 60 | 1 | 51 | 180 | 0.6 % |

| rang de l'appel | miss | partiels | appels | taux de miss |
|---|---|---|---|---|
| 2 | 6 | 0 | 20 | 30.0 % |
| 3-4 | 8 | 2 | 40 | 20.0 % |
| 5-10 | 10 | 35 | 118 | 8.5 % |
| >= 11 | 3 | 63 | 218 | 1.4 % |

Préfixe relu par les relectures partielles (1 = appel précédent, 2 = celui d'avant, aucun = pas d'appel antérieur à ce compte) : 2 : 36, 3 : 27, 4 : 16, 5 : 5, 6 : 6, 7 : 2, 8 : 3, 10 : 2, 15 : 1, 19 : 1, aucun : 1
```

| Source | Binaire | Relectures partielles | Miss |
|---|---|---|---|
| 14/11, bras A de `ab-20261114T201248Z`, 3 passes (baseline Luna) | `80e20ea` | 7/297 (2,4 %) | 8/297 (2,7 %) |
| 16/11, bras B de `ab-20261116T124625Z`, 4 passes | `318f3e1`, `promptVersion` 1 | 54/376 (14,4 %) | 16/376 (4,3 %) |
| **17/11, `20261117T143934Z-80e20ea-gpt-6-luna`, 4 runs** | **`80e20ea`** | **100/396 (25,3 %)** | 27/396 (6,8 %) |

Références du 14/11 et du 16/11 : plan, section « Contrôle de la dérive ».

### Verdict

Règle fixée d'avance (plan, « Règle fixée d'avance ») : taux du jour ≥ 8,4 % → **dérive extérieure à notre binaire**. Avec 25,3 %, le même binaire qui faisait 2,4 % le 14/11 dépasse aussi le taux du 16/11. La hausse ne vient pas des changements entre `80e20ea` et `318f3e1`. Pas de ticket sur le binaire.

Le contrôle ne dit pas lequel des facteurs extérieurs joue : serveur, compte, réseau ou machine.

Observation, sans décision : le même jour, les deux bras de l'A/B (`bf88ebc`, prompt v2) font 24,4 % et 24,8 % de relectures partielles. Le bras A en prompt v2 du 16/11 en faisait 13,4 % (`ya09-cache-v2-20261116.md`). Les miss montent aussi : 6,8 % pour `80e20ea` le 17/11, contre 2,7 % le 14/11. Conséquence pour la suite : les taux de cache de deux jours différents ne se comparent pas (ticket 14, « Clôture »).

## Commandes

Depuis la racine du dépôt, en lecture seule :

```bash
python3 -I bench/smoke/ab/ya09-session-id-mesures.py --fil bench/smoke/runs/ab-20261117T130904Z
python3 -I bench/smoke/ab/ya09-session-id-mesures.py bench/smoke/runs/ab-20261117T131130Z
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-20261117T131130Z/ab.json --baseline bench/smoke/baseline/gpt-6-luna.json
python3 -I bench/smoke/tools/cache_causes.py bench/smoke/runs/20261117T143934Z-80e20ea-gpt-6-luna
```

Unité B en échec : `bench/smoke/runs/ab-20261117T131130Z/arms/20261117T140347Z-bf88ebc-gpt-6-luna/report.md`, `tasks/py-tldextract-368/run-1/grading/test.out` et la session de `tasks/py-tldextract-368/run-1/sessions/`.
