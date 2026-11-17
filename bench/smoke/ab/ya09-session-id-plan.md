# A/B YA-09 en-tête `session-id` (ticket 14, option A) : plan et règle de verdict

Plan écrit le 17/11/2026, avant tout run, à la demande de Yusuf (décision du 17/11, 12h05 : option A seule). **À valider par Yusuf avant le run.** Les runs se lancent sur `main` après le merge de la PR : l'A/B d'abord, puis le contrôle de la dérive (section dédiée), l'un après l'autre.

## Question

Le ticket 14 (`docs/tickets/14-ya09-cache-miss.md`) montre l'excès de miss du prompt v2 sur les petits prompts : en A (v2) le 16/11, 21 miss sur 90 appels dont le prompt précédent fait moins de 6 000 tokens (23,3 %), contre 4 sur 171 au-dessus (2,3 %). Codex CLI envoie l'en-tête `session-id` (tiret), et son code dit que ChatGPT en tire l'affinité de cache. Nous envoyons `session_id` (tiret bas).

**Avec l'en-tête `session-id`, les appels sous 6 000 tokens ont-ils nettement moins de miss ?**

### Ce que fait Codex CLI

Lu dans deux clones jetables, sans rien exécuter ni compiler :
- `/tmp/codex-0.169.0-ya14`, tag `rust-v0.169.0`, commit `d32063f96c2dff1dc96af839b3952abb6acfeda4` (version du témoin du 14/11) ;
- `/tmp/codex-main-ya14-sid`, `main` à `59ac39addbbcd3499319c805a916846ca8ac5fef` (17/11/2026 08:08 UTC), clone superficiel. Mêmes extraits, aux lignes indiquées entre parenthèses.

Nom de l'en-tête, `codex-rs/codex-api/src/requests/headers.rs:5-14` (`main` : `:25-34`) :

```rust
pub fn build_session_headers(session_id: Option<String>, thread_id: Option<String>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(id) = session_id {
        insert_header(&mut headers, "session-id", &id);
    }
    if let Some(id) = thread_id {
        insert_header(&mut headers, "thread-id", &id);
    }
    headers
}
```

Valeur et commentaire, `codex-rs/core/src/client.rs:734-742` (`main` : `:760-768`) :

```rust
    // ChatGPT derives cache affinity from the Responses session-id header. Keep the
    // actual session identity in turn metadata, hooks, and history/notes requests.
    fn responses_session_id(&self, metadata: &CodexResponsesMetadata) -> String {
        if self.state.session_source.is_non_root_agent() {
            metadata.session_id.clone()
        } else {
            self.session_derived_cache_key(metadata)
        }
    }
```

Pour une session racine, la valeur est `session_derived_cache_key` (`client.rs:720-732`) : l'id de session, sauf surcharge. C'est aussi la valeur de `prompt_cache_key` (`client.rs:711-718`, politique `SessionDerived`). L'en-tête part sur la poignée de main WebSocket (`client.rs:1571-1574`, `build_websocket_headers`) et sur la requête HTTP (`client.rs:1635`, puis `codex-api/src/endpoint/responses.rs:90`). Codex n'envoie pas `session_id` (tiret bas) en en-tête.

**Non repris** : `thread-id` (hors de l'option A). Noté sur `main` de Codex, hors périmètre : `ChatGptRoutingAffinity` (`codex-rs/http-client/src/chatgpt_routing_affinity.rs`), qui garde les cookies de routage `__oailb` et `__cflb` reçus à la poignée de main d'une connexion. Le commentaire de Codex est une source côté client, pas une documentation du serveur.

## Bras

| Bras | Réglages | Différence |
|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | réglage absent : pas d'en-tête `session-id` (défaut de `main`) |
| B | `bench/smoke/settings/session-id-header.json` (`{"codexSessionIdHeader": true}`) | en-tête `session-id` = id de session sur le provider `openai-codex`, en plus de `session_id` |

Écart de réglages calculé par `ab.py` : `{"codexSessionIdHeader": [null, true]}`. **Même binaire**, prompt v2 dans les deux bras, tout le reste identique. Le corps des requêtes et le préfixe sont identiques entre les bras (test `codex_websocket_session_id_header_follows_the_setting`, `crates/pa-core/tests/prefix_stability.rs`). Chaque session de B porte `"codexSessionIdHeader": true` dans `yaRequest`, chaque session de A `"codexSessionIdHeader": false`.

## Protocole

- Binaire de `main` après le merge de la PR : sha, run de `ya-build-macos-arm64` et sha256 de l'exécutable notés dans le rapport au moment du run. Installation : `bash bench/smoke/tools/install-bin.sh <run> <sha>`.
- `gpt-6-luna`, provider `openai-codex` (WebSocket), effort `high`.
- **5 tâches**, celles de l'A/B du 16/11 : `node-koa-2029`, `node-tough-cookie-630`, `py-langextract-584`, `py-tldextract-368`, `py-tldextract-383`.
- **4 passes entrelacées** (`tools/ab.py`), même jour : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort.
- **Graine : 4199508898**, tirée de `os.urandom` le 17/11/2026 à 11:25Z, avant le run. Ordre calculé par `ab.plan` (même fonction que `--dry-run`, sans le binaire) : passe 1 `koa AB, tough-cookie BA, langextract BA, tldextract-368 AB, tldextract-383 BA` ; passe 2 `AB` partout ; passe 3 `koa BA, tough-cookie BA, langextract AB, tldextract-368 BA, tldextract-383 AB` ; passe 4 `koa AB, tough-cookie BA, langextract AB, tldextract-368 BA, tldextract-383 AB`. Le `--dry-run` avec le binaire doit redonner cet ordre.
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (posés par le runner), garde-fou d'inactivité de 300 s.
- Quota : mêmes conditions que les A/B précédents. Fenêtre de 5 h utilisée à 10 % au plus avant de lancer, aucun autre usage du compte pendant l'A/B, arrêt sur erreur de quota.
- **Unités écartées** : règle du README (« A/B : erreurs provider »), appliquée par `ab_stats.units_from_plan`. Une erreur provider sur un bras écarte l'unité (la tâche de cette passe, ses deux bras).

**Contrôle sur le fil avant le run, sans modèle** : même binaire, même commande en `--capture` sur une tâche. Le serveur de capture garde les en-têtes de chaque requête. Le contrôle passe si toutes les requêtes de B portent `session-id` égal à `session_id`, et aucune de A. Sinon, pas de run.

```bash
bash bench/smoke/tools/install-bin.sh <run> <sha>
# 1. Contrôle sur le fil (serveur de capture, aucun modèle)
python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha> \
  --a bench/smoke/settings/default.json --b bench/smoke/settings/session-id-header.json \
  --tasks node-koa-2029 --passes 1 --model gpt-6-luna --effort high --capture
python3 -I bench/smoke/ab/ya09-session-id-mesures.py --fil bench/smoke/runs/ab-<horodatage capture>
# 2. Ordre (doit redonner celui du plan)
python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha> \
  --a bench/smoke/settings/default.json --b bench/smoke/settings/session-id-header.json \
  --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383 \
  --passes 4 --model gpt-6-luna --effort high --seed 4199508898 --dry-run
# 3. Run réel (Yusuf)
python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha> \
  --a bench/smoke/settings/default.json --b bench/smoke/settings/session-id-header.json \
  --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383 \
  --passes 4 --model gpt-6-luna --effort high --seed 4199508898
# 4. Mesures
python3 -I bench/smoke/ab/ya09-session-id-mesures.py bench/smoke/runs/ab-<horodatage>
python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-<horodatage>/ab.json
python3 -I bench/smoke/tools/cache_calls.py --plan bench/smoke/runs/ab-<horodatage>/ab.json
python3 -I bench/smoke/tools/ab_stats.py --plan bench/smoke/runs/ab-<horodatage>/ab.json --baseline bench/smoke/baseline/gpt-6-luna.json
```

**Estimations** : 40 lancements, environ 55 min (3 228 s pour les 40 lancements du 16/11, `ab.json`, `overhead.wall_total_s`), environ 0,2 $ en coût rapporté (0,10 et 0,14 $ par bras le 16/11). Le quota de l'abonnement est la vraie limite.

## Métriques

Définitions de `bench/smoke/tools/cache_causes.py`, lues au journal du reverse proxy (`proxy.ndjson`), par appel de génération hors premier d'une session, sur les unités retenues :
- **miss** : `cached_tokens = 0` ;
- **relecture partielle** : `0 < cached_tokens < ⌊prompt précédent⌋₁₂₈ − 256` ;
- **prompt précédent** : `input_tokens` de l'appel précédent de la session.

**Métrique principale** : taux de miss des appels dont le prompt précédent fait **moins de 6 000 tokens**, par bras. Écart **d = taux de A (sans en-tête) − taux de B (avec en-tête)**, en points : positif si l'en-tête réduit les miss.

**Métriques secondaires** :
- miss global (tous les appels hors premier), qui sert aussi de garde-fou ;
- relectures partielles par bras, globales, sous et au-dessus de 6 000 tokens, affichées sans décider ;
- miss au-dessus de 6 000 tokens, affiché sans décider ;
- coût rapporté par bras et B / A − 1, affiché sans décider. Le coût à cache théorique et son IC viennent d'`ab_stats.py`.

Toutes sont calculées par `bench/smoke/ab/ya09-session-id-mesures.py`, écrit avec ce plan. Le script vérifie aussi la trace : `yaRequest.codexSessionIdHeader` vaut `true` dans toutes les sessions de B et `false` dans toutes celles de A, sinon le verdict est suspendu.

**Vérification de l'outil** (`--sans-trace`, sur des A/B antérieurs au réglage) :
- 16/11 (`ab-20261116T124625Z`) : il redonne la règle de cet A/B (miss global +5,3 points [+3,0 ; +7,8]), 21/90 sous 6 000 tokens en A et −1,9 [−4,4 ; +0,4] au-dessus, comme le ticket 14 ;
- 15/11 (`ab-20261115T153500Z`, prompt v1 dans les deux bras) : miss global +0,6 [−0,4 ; +1,6], comme la vérification du plan du 16/11 ;
- `--fil` : vérifié sur un A/B factice, sans modèle ni binaire (un cas conforme, un cas en écart). Pas encore sur un vrai binaire : c'est le contrôle 1 ci-dessus.
- `verdict` : vérifié sur six cas construits (un par ligne de la règle, plus un garde-fou à une seule condition, qui ne déclenche pas).

## Règle de verdict (proposée, à valider avant le run)

Notations, en points de pourcentage, sur les unités retenues :
- **d** = taux de miss de A (sans en-tête) − taux de miss de B (avec en-tête), appels à prompt précédent < 6 000 tokens. Positif : moins de miss avec l'en-tête.
- **d_global** = la même différence sur tous les appels hors premier. Négatif : plus de miss avec l'en-tête.
- **IC** : IC 95 % par bootstrap sur les unités. 10 000 tirages avec remise, recalcul de l'écart sur chaque tirage, graine 0. C'est la méthode de l'A/B du 16/11 : elle tient compte de ce que les miss d'une même session ne sont pas indépendants. « Borne basse » et « borne haute » sont celles de cet IC.

Les cas sont examinés dans l'ordre ; le premier qui s'applique donne le verdict. Les cas 2 à 5 couvrent toutes les valeurs de d et de sa borne basse.

| Ordre | Condition | Verdict |
|---|---|---|
| 1 | **Dégradation** : d_global ≤ −3 points **et** borne haute de l'IC de d_global < 0. **Les deux conditions à la fois** ; une seule ne suffit pas | **l'en-tête dégrade le cache** : le réglage reste coupé, quel que soit le résultat principal |
| 2 | d ≥ 10 points **et** borne basse > 0 | **gain net** : proposer `codexSessionIdHeader` activé par défaut (décision de Yusuf, changement de défaut dans un commit à part) |
| 3 | 0 < d < 10 points **et** borne basse > 0 | **non concluant** : effet probable mais sous le seuil ; Yusuf décide |
| 4 | d ≥ 10 points **et** borne basse ≤ 0 | **non concluant** : Yusuf décide (passes en plus, ou rien) |
| 5 | d < 10 points **et** borne basse ≤ 0 | **pas de gain net détecté** : le réglage reste coupé. Le rapport donne la borne haute de l'IC, qui borne le gain réel plausible |

Au cas 3, d est forcément positif : une borne basse > 0 implique d > 0. `ya09-session-id-mesures.py` (fonction `verdict`) applique ces cas dans cet ordre.

### Taux de base de la métrique principale

Run `ab-20261116T124625Z`, bras A (prompt v2, binaire `318f3e1`), appels hors premier dont le prompt précédent fait moins de 6 000 tokens : **21 miss sur 90 appels, 23,3 %**. Ces appels se trouvent dans les 20 unités retenues. L'IC 95 % par bootstrap sur les unités va de 15,7 à 32,6 % (10 000 tirages, graine 0). Au-dessus de 6 000 tokens, le même bras rate **4 appels sur 171 (2,3 %)**.

```bash
python3 -I bench/smoke/ab/ya09-session-id-mesures.py --sans-trace bench/smoke/runs/ab-20261116T124625Z   # 21/90, 4/171
python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json         # bras A : 16/52 (< 3k) + 5/38 (3k-6k)
```

L'IC du taux de base (15,7 à 32,6 %) vient d'un bootstrap sur les unités, calculé pour ce plan avec les fonctions de `bench/smoke/tools/power.py` (`base_units`, `rate`, graine 0). Il n'est pas affiché par l'outil.

### Pourquoi 10 points

- **Ordre de grandeur de l'excès** : sous 6 000 tokens, le v2 rate 23,3 % des appels, contre 2,3 % au-dessus. L'excès vaut environ 21 points. **10 points en retirent environ la moitié**, soit 43 % des miss de cette tranche. Un effet de cache d'affinité qui joue sur ces appels devrait atteindre cet ordre ; un effet plus faible ne changerait pas la décision de produit.
- **Ce que le banc peut voir** : à 4 passes, l'écart type de d vaut environ 5,6 points, donc la demi-largeur de l'IC environ 11 points. C'est une estimation : 4,3 points d'écart type par bootstrap pour le bras A, environ 3,6 points pour un bras B à 12 % (même effet de grappe), combinés comme deux bras indépendants. Un écart de moins de 10 points ne peut pas exclure 0 à ce niveau de bruit : ce seuil est le plus bas que ce banc peut trancher à 4 passes.
- **Ce qui est en jeu en coût** (estimation du ticket 14) : tous les miss de A sous 6 000 tokens valent environ 5 % de son coût rapporté. 10 points de moins en valent environ 2 % ; moins que cela est sous ce que le banc distingue en coût (environ 10 %, constat du journal du CDC du 14/11).

**Pourquoi un garde-fou à 3 points** : c'est le seuil de l'A/B du 16/11 pour un écart de miss global qui se voit en coût. Les deux conditions sont exigées ensemble pour ne pas couper sur du bruit (d_global ≤ −3 avec un IC qui contient 0) ni sur un écart réel mais minime (IC qui exclut 0 avec d_global proche de 0).

### Puissance (estimation par simulation)

Outil versionné : `bench/smoke/tools/power.py`.

```bash
python3 -I bench/smoke/tools/power.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json --arm A
```

Paramètres par défaut, sortie reproductible : 600 A/B simulés par cas, bootstrap de 400 tirages, seuil 10 points, graine 1 ; durée 16 s.
- **Données** : les unités du bras A du 16/11 sous 6 000 tokens.
- **Effet de grappe mesuré** : 0,95. L'écart type par bootstrap sur les unités (4,3 points) est proche de l'écart type binomial (4,5 points).
- **Méthode** : chaque bras tire 5 unités par passe, avec remise ; B garde chaque miss avec une probabilité 1 − r. La règle principale (cas 2 à 5) est appliquée ; le garde-fou n'est pas simulé.

| Passes | Réduction réelle (d réel) | gain net | non concluant (cas 3 ou 4) | pas de gain net détecté |
|---|---|---|---|---|
| 4 | 0 % (0 point) | 3 % | 2 % | 94 % |
| 4 | 50 % (11,7 points) | 56 % | 10 % | 33 % |
| 4 | 75 % (17,5 points) | 94 % | 3 % | 3 % |
| 4 | 90 % (21,0 points) | 100 % | 0 % | 0 % |
| 6 | 0 % | 2 % | 2 % | 96 % |
| 6 | 50 % | 64 % | 16 % | 20 % |
| 6 | 75 % | 98 % | 2 % | 1 % |
| 8 | 0 % | 0 % | 3 % | 96 % |
| 8 | 50 % | 68 % | 22 % | 11 % |
| 8 | 75 % | 99 % | 1 % | 0 % |

Lecture :
- À 4 passes, un en-tête qui supprime les trois quarts de ces miss sort « gain net » environ 9 fois sur 10.
- Un en-tête qui en supprime la moitié sort « gain net » environ une fois sur deux. Plus de passes aident peu : le seuil de 10 points est proche de l'effet réel, et c'est le seuil qui limite. Les passes en plus déplacent surtout des cas vers « non concluant ».
- Sans effet réel, la règle conclut à tort au « gain net » environ 3 fois sur 100 à 4 passes.
- **Proposition : 4 passes**, comme le 16/11.
- Un « pas de gain net détecté » n'exclut pas un effet réel : sa borne haute dit jusqu'où il peut aller.
- Limite : on suppose que le bras A d'aujourd'hui ressemble au bras A du 16/11. Le binaire a changé depuis : ticket 13, et prompt v2 réaligné.

## Contrôle de la dérive des relectures partielles en prompt v1

Contrôle à part de l'A/B, sans modification du code, à lancer après l'A/B.

**Question** : en prompt v1, sur les mêmes 5 tâches, les relectures partielles sont passées de 2,4 % le 14/11 à 14,4 % le 16/11. Est-ce le serveur, ou notre binaire ?

| Source | Binaire | Relectures partielles | Miss |
|---|---|---|---|
| 14/11, bras A de `ab-20261114T201248Z`, 3 passes (baseline Luna) | `80e20ea` | **7/297 (2,4 %)** | 8/297 (2,7 %) |
| 16/11, bras B de `ab-20261116T124625Z`, 4 passes | `318f3e1`, `promptVersion` 1 | **54/376 (14,4 %)** | 16/376 (4,3 %) |

Calculé par `python3 -I bench/smoke/tools/cache_causes.py --plan <ab.json> --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383` (14/11) et `--plan bench/smoke/runs/ab-20261116T124625Z/ab.json` (16/11).

### Faisabilité : oui, vérifiée par la lecture du code et par un run en mode capture (17/11)

- **Binaire** : `~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/prime-agent`, toujours présent. Son sha256, `099f54d354c3daab653ec9502ae82658d32224600b616e95c5ffe4e2212ffc8e` (`shasum -a 256`, 17/11), est celui que note `ab-20261114T201248Z/ab.json` (`binaries.A.sha256`). `models.bundled.json` est à côté.
- **Réglages d'origine** : la baseline a tourné avec `settings/default.json` (`{}`). Ses seuls réglages hors défaut étaient `cacheRetention: short`, posé par le runner (`baseline/gpt-6-luna.json`, `header.settings_non_default`). Avec `{}`, ce binaire prend ses propres défauts, ceux du 14/11 : prompt v1, `reasoningSummary` `"auto"`, fenêtre `bash()` de 1 s, pas de keepalive. Le banc d'aujourd'hui écrit le même `settings.json`.
- **Outils du banc** : `git diff 11ed7c38a 13d931e87 -- bench/smoke/run.py bench/smoke/metrics.py bench/smoke/proxies.py bench/smoke/sandbox-*.sb bench/smoke/hosts` (`11ed7c38a` : commit du banc de la baseline) ne montre que deux changements, sans effet sur ce chemin :
  - `run.py` : `load_tasks` écarte les tâches `chauffe` de la sélection par défaut ; ici, les tâches sont nommées ;
  - `metrics.py` : la regex `EDIT_FAILURE` reconnaît plus de messages ; elle compte les échecs d'`edit()`, pas le cache.
  - Sandbox, proxies et hôtes autorisés : inchangés.
- **Tâches** : aucun commit sur les 5 tâches depuis le 14/11 (`git log --since=2026-11-14T20:00 -- bench/smoke/tasks/<tâche>`, vide pour les 5).
- **Étape 0 faite, sans modèle** (autorisée par Yusuf le 17/11). Run `bench/smoke/runs/20261117T120533Z-80e20ea-gpt-6-luna` (mode `capture`, serveur de capture en amont, aucun appel au modèle) :
  - binaire : sha256 `099f54d3…ffc8e`, commit du manifeste `80e20ea5f353`, version `0.10.2-continuous.80e20ea5f353`, comme le 14/11 ;
  - réglages : `{}`, hors défaut `cacheRetention: short`, comme le 14/11 ;
  - outils : Python 3.14.7, `node` v24.21.0, `uv` 0.12.17, identiques à ceux des rapports du 14/11 (`header.tools`) ;
  - sonde : code 0, réseau coupé, 12 lectures interdites toutes refusées ; chauffe aboutie ;
  - 5 tâches lancées, 4 appels chacune, tous en WebSocket, aucune erreur amont. Statut `failed` attendu : en mode capture, les tâches du banc jouent `smoke-fail` ;
  - recherche de secrets : aucun fichier touché.
- **Non vérifié** : le login réel et le venv du kernel en run réel, qui ne servent pas en mode capture. Le run réel s'arrête à la chauffe s'ils manquent.
- **Différences de conditions** :
  - le 14/11, chaque tâche alternait avec le bras Codex CLI (A/B entrelacé, 3 passes) ;
  - ici, un seul bras : `run.py --runs 4` lance les 4 runs d'une tâche à la suite, puis passe à la tâche suivante ;
  - le taux de référence vient de 3 passes, celui du jour en aura 4.

Commandes :

```bash
# 0. Contrôle sans modèle : le banc d'aujourd'hui lance le binaire du 14/11 (serveur de capture)
python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/prime-agent \
  --model gpt-6-luna --effort high --runs 1 --capture \
  --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383
# 1. Run réel (Yusuf), 4 passes, réglages d'origine
shasum -a 256 ~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/prime-agent   # 099f54d3...ffc8e
python3 -I bench/smoke/run.py --binary ~/.cache/yusagent-bin/80e20ea5f353ff3855e30d19c0df7cc7d1cc9632/prime-agent \
  --settings bench/smoke/settings/default.json --model gpt-6-luna --effort high --runs 4 \
  --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383
# 2. Taux du jour (une ligne « ## <run> », 20 sessions)
python3 -I bench/smoke/tools/cache_causes.py bench/smoke/runs/<horodatage>-80e20ea-gpt-6-luna
```

Estimations : 20 lancements de tâche, plus la sonde et la chauffe, soit environ 40 min (110 s par lancement pour ces 5 tâches le 14/11, `ab.json`, `arms[].wall_s`). Environ 0,2 $ en coût rapporté.

### Règle fixée d'avance

Taux du jour = relectures partielles / appels hors premier, sur les 20 sessions, sortie de `cache_causes.py`.

| Taux du jour | Lecture |
|---|---|
| ≥ 8,4 % (proche du 16/11) | **dérive extérieure à notre binaire** : serveur, ou conditions du banc (compte, réseau, machine). Pas de ticket sur le binaire |
| ≤ 5,4 % (proche du 14/11) | **cause dans notre binaire**, entre `80e20ea` et `318f3e1` : ticket à ouvrir |
| entre 5,4 et 8,4 % | **non concluant** : Yusuf décide |

Bornes :
- **8,4 %** est le milieu entre 2,4 et 14,4 %.
- **5,4 %** vaut 2,4 % plus 3 points. Avec 20 sessions, l'écart type par bootstrap sur les sessions vaut 0,8 point autour de 2,4 % et 3,2 points autour de 14,4 % (estimation par rééchantillonnage des sessions du 14/11 et du 16/11, outil versionné `bench/smoke/tools/drift_se.py`, commandes ci-dessous).
- Si le vrai taux est celui du 16/11, le taux du jour tombe sous 8,4 % environ 3 fois sur 100, et sous 5,4 % environ 1 fois sur 1 000.
- Si le vrai taux est celui du 14/11, il ne dépasse 5,4 % dans aucun des 20 000 tirages.

Le 15/11 (5,7 et 7,9 %, `7086711`) tombe dans la zone grise. Une dérive progressive donnerait aussi un taux intermédiaire.

```bash
T=node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383
python3 -I bench/smoke/tools/drift_se.py --plan bench/smoke/runs/ab-20261114T201248Z/ab.json --arm A --tasks $T
# 7/297 (2,4 %) ; écart type 0,8 point ; sous 5,4 % : 100,0 % des tirages
python3 -I bench/smoke/tools/drift_se.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json --arm B --tasks $T
# 54/376 (14,4 %) ; écart type 3,2 points ; sous 8,4 % : 2,8 % ; sous 5,4 % : 0,1 %
```
