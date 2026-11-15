# A/B YA-09 `reasoningSummary` (`auto` contre `off`) : plan et règle de verdict

Règles fixées et versionnées avant tout run réel. Plan validé par Yusuf le 15/11/2026, avec ses décisions sur la clé d'en-tête, la règle de coût, la portée du verdict et le périmètre.

## Question

Le run YA-14 (`bench/smoke/ab/ya14-codex-20261114.md`, « Latence par appel ») a relevé environ 2,9 s de latence fixe en plus par appel avec raisonnement chez yusAgent par rapport à Codex CLI sur `gpt-6-luna`. C'est une **corrélation** avec `reasoning.summary: "auto"`, que yusAgent envoie avec chaque effort de raisonnement (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs:95-99`, `:684-701`). Codex omet ce champ pour Luna (`default_reasoning_summary: "none"`, `codex-rs/models-manager/models.json:733` ; champ omis quand le résumé vaut `none`, `codex-rs/core/src/client.rs:1149-1152`, tag `rust-v0.169.0`).

L'A/B isole ce levier : même binaire et même prompt, seul le champ `reasoning.summary` change.

## Bras

| Bras | Réglages | Corps de requête |
|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | `reasoning: {"effort": "high", "summary": "auto"}` (comme `main`) |
| B | `bench/smoke/settings/reasoning-summary-off.json` (`{"reasoningSummary": "off"}`) | `reasoning: {"effort": "high"}`, champ `summary` omis comme chez Codex. `"none"` n'est pas envoyé : aucune source ne l'atteste comme valeur acceptée sur le fil |

Le réglage est figé par session dans l'en-tête (`yaRequest`, `crates/pa-core/src/session_engine/ya_request.rs`). Prompt système et tools identiques dans les deux bras. Les tests le vérifient : `crates/pa-core/tests/prefix_stability.rs`, scénarios `codex_reasoning_summary_*` et `codex_websocket_reasoning_summary_follows_the_setting`.

## Protocole

- `gpt-6-luna`, provider `openai-codex` (WebSocket), effort `high`.
- Les 10 tâches du banc (`bench/smoke/tasks/`, sans les exemples), **3 passes entrelacées par tâche** (`tools/ab.py`) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort.
- **Graine : 3221902756** (tirée de `os.urandom` le 15/11/2026 à 13:46Z, avant le run).
- Binaire : artefact de `ya-build-macos-arm64.yml` au dernier commit de la branche `ya-09-summary`, installé par `tools/install-bin.sh`. Le SHA complet et le sha256 de l'exécutable seront notés ci-dessous avant le premier run.
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité de 300 s.
- Quota : mêmes conditions que YA-14 (`ya14-codex-plan.md`, « Quota ») : fenêtre de 5 h utilisée à 10 % au plus avant de lancer, aucun autre usage du compte pendant l'A/B, arrêt sur erreur de quota.
- **Paires écartées** : règle d'écart du README (« A/B : erreurs provider ») : une erreur provider (quota, surcharge, flux bloqué, statut `stalled`) sur un bras écarte la tâche de cette passe, ses deux bras. `ab_stats.py` l'applique et liste les unités écartées.

```bash
python3 -I bench/smoke/tools/ab.py <dossier du binaire> <sha> --b bench/smoke/settings/reasoning-summary-off.json \
  --model gpt-6-luna --effort high --seed 3221902756
python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --baseline bench/smoke/baseline/gpt-6-luna.json
```

### Binaire et chauffe

- **Binaire** : branche `ya-09-summary` à `7086711263ebfcd44b33fe7e3381630337243bbe` (dernier commit de code ; CI `ci.yml` verte, run 43723001064). Build `ya-build-macos-arm64.yml` 43722995206, artefact `prime-agent-aarch64-apple-darwin-7086711263ebfcd44b33fe7e3381630337243bbe`, version `v0.10.2`.
  - sha256 de l'archive `prime-agent-0.10.2-darwin-arm64.tar.gz` : `49001dcfe185bf72814d80eb43dac7c7c5c3d89ea1029a4be661ef0f7d873611` (manifeste, `SHA256SUMS` et `shasum -a 256`).
  - sha256 de l'exécutable `prime-agent` : `04c27dd29b2252e89dce47afb6c01bc3c70b9d7bc1ae6a4cae4d1178d57c393f` (manifeste, `executableSha256`, et `shasum -a 256` sur l'exécutable extrait).
  - Les commits suivants de la branche ne touchent que `bench/` : `git diff --quiet 7086711 HEAD -- crates prime-agent-runtime skills vendor install-rust.sh Cargo.toml Cargo.lock` doit sortir avec le code 0 avant le run.
- **Pas de chauffe sans l'accord de Yusuf.**

Chauffe prévue : une tâche (`py-shortuuid-118`), bras B, `gpt-6-luna`, effort high. Elle ne compte dans aucun A/B. Elle vérifie au vrai backend :
- le corps sans `summary` est accepté sur le WebSocket ;
- `proxy.ndjson` ne montre aucun `summary` ;
- les items `reasoning` arrivent encore avec `encrypted_content` ;
- `reasoning_tokens` > 0 reste remonté dans l'usage (le découpage « avec raisonnement » ci-dessous en dépend) ;
- la suite de conversation (`previous_response_id`) fonctionne ;
- l'en-tête de session porte `"yaRequest": {"reasoningSummary": "off"}`.

Une passe du bras A sur le même binaire vérifie que son corps garde `summary: "auto"`.

## Règle de verdict

**Verdict par défaut : `auto`** (bras A). La charge de la preuve est sur le bras `off` (B). On applique les règles dans l'ordre, et la première qui s'applique décide.

1. **Résolution** (même règle que YA-03, `ya03-plan.md`) : totaux de « Tâches résolues par passe » sur les paires valides (section « Par bras » d'`ab_stats.py`). Seuil fixé avant le run, pas un test statistique.
   - B résout **au moins 3 tâches de moins** que A : **A gardé**.
   - B résout **au moins 3 tâches de plus** que A : **B retenu**.
2. **Coût**, s'il n'y a pas de verdict de résolution (règle alignée sur YA-14) : coût à cache théorique des **tâches stables de `bench/smoke/baseline/gpt-6-luna.json`** (`node-koa-2029`, `node-tough-cookie-630`, `py-langextract-584`, `py-shortuuid-118`, `py-tldextract-368`, `py-tldextract-383`). Écart B/A − 1, IC 95 % par bootstrap sur les tâches : ligne « stables, théorique (décision) » de la section « Écart agrégé » d'`ab_stats.py`, avec `--baseline`.
   - IC entièrement au-dessus de 0 (B plus cher) : **A gardé**.
   - IC qui exclut 0 **et** |écart| > 10 %, avec B moins cher : **B retenu**. Le seuil de 10 % vient de la sensibilité du banc (journal du CDC, constat du 14/11/2026).
   - Sinon : coût **non distinguable**, on passe à la latence.
3. **Latence**, si le coût n'a pas tranché : B est retenu seulement si les **deux** conditions sont remplies.
   - **Durée de la phase agent**, appariée par tâche : l'IC 95 % de B/A − 1 exclut 0 **et** |écart| > 10 %, avec B plus court. Même seuil que le coût, pour la même raison (sensibilité du banc).
   - **Latence fixe par appel avec raisonnement** (mécanisme) : la borne haute de l'IC 95 % de B − A est sous 0.
   - Sinon : **A gardé**.

   Ligne « condition remplie » de `ya09-summary-mesures.py`, population « toutes ».

Verdicts possibles : A gardé (résolution, coût ou latence) ; B retenu (résolution, coût ou latence).

### Méthode des métriques de latence

Méthode de `ya14-codex-20261114-mesures.py` (`load`, `ols`), appliquée aux deux bras yusAgent.

- **Appels** : requêtes de génération lues au reverse proxy (`tasks/<tâche>/run-1/proxy.ndjson`), chaque `request` appariée à sa `response` dans l'ordre, prewarm exclu (`generate: false`). Temps modèle = horodatage de la réponse − horodatage de la requête. Tokens de sortie et de raisonnement lus dans l'usage de la réponse.
- **Appel avec raisonnement** : `reasoning_tokens` > 0.
- **Latence fixe** d'un bras : ordonnée à l'origine des moindres carrés du temps modèle sur les tokens de sortie, sur ses appels avec raisonnement des paires valides. Différence B − A, en secondes.
- **Phase agent** : `agent.seconds` du résultat de la tâche (`report.json`). Moyenne par tâche sur les passes valides de chaque bras, puis écart B/A − 1 sur la somme des moyennes par tâche, appariée par tâche (mêmes tâches dans les deux bras). L'écart moyen par tâche en secondes est affiché sans décider.
- **Population** : toutes les tâches avec au moins une paire valide. La latence par appel ne dépend pas de la résolution. Le sous-ensemble « résolues par les deux bras » est affiché sans décider.
- **IC 95 %** : bootstrap sur les tâches (`ab_stats.bootstrap`), 10 000 tirages, graine 0. À chaque tirage, on tire les tâches avec remise, on prend tous les appels (ou toutes les durées) des tâches tirées dans chaque bras, et on refait l'ajustement.
- **Paires écartées** : `ab_stats.units_from_plan`, même règle d'écart qu'`ab_stats.py`.
- **Script** : `bench/smoke/ab/ya09-summary-mesures.py` (bibliothèque standard, lecture seule), versionné avant le run :

  ```bash
  python3 -I bench/smoke/ab/ya09-summary-mesures.py bench/smoke/runs/ab-<horodatage>
  ```

- **Validation sur le run YA-14** (`python3 -I bench/smoke/ab/ya09-summary-mesures.py bench/smoke/runs/ab-20261114T201248Z`, A = yusAgent, B = Codex CLI). Sur la population « communes », celle du rapport YA-14, le script retrouve les chiffres du rapport :
  - avec raisonnement, A : 191 appels, 3,62 s + 14,3 ms/token ; B : 100 appels, 0,73 s + 18,1 ms/token ;
  - sans raisonnement, A : 64 appels, 0,84 s + 17,6 ms/token ; B : 35 appels, 0,79 s + 18,3 ms/token ;
  - phase agent : 83,0 s contre 44,2 s ;
  - temps modèle médian par appel : 4,26 s contre 2,89 s.

  Sur la population « toutes » (10 tâches), latence fixe B − A −2,56 s [−2,91 ; −2,13], phase agent −34,9 % [−49,3 ; −18,5].

### Affiché sans décider

- Latence fixe et pente des appels **sans** raisonnement (contrôle : on n'attend pas d'écart).
- Pente (ms par token) des appels avec raisonnement.
- Temps modèle médian par appel, nombre d'appels et tours par lancement.
- Tokens de sortie et de raisonnement par lancement.
- TTFT : **non comparable** entre les bras. Il est marqué au premier événement de raisonnement (`crates/pa-core/src/session_engine/usage_log.rs:778-786`), et en `off` cet événement ne porte plus de texte.
- Coût rapporté et pessimiste, taux de cache, miss serveur, edits au premier essai, cellules en erreur.

## Ce que le banc peut détecter

Ces chiffres sont des estimations tirées de YA-14, pas des mesures.

- **Résolution** : on n'attend pas d'effet. Le seuil de 3 tâches ne détecte qu'une forte régression.
- **Coût** : effet attendu inconnu.
  - Rien dans le code ne dit si le résumé est facturé en tokens de sortie.
  - En suite de conversation, seuls les nouveaux items partent, donc le texte de résumé rejoué ne pèse que sur les corps complets.
  - Le banc ne distingue pas un effet de moins de 10 % environ : un verdict de coût est peu probable.
- **Latence fixe** : YA-14 l'a estimée sur 191 appels avec raisonnement (A) contre 100 (B), avec un écart de 2,9 s. Ici, environ 300 à 400 appels avec raisonnement par bras (30 lancements par bras). Un effet de cet ordre devrait sortir de l'IC s'il est réel.
- **Phase agent** : effet attendu d'environ −37 s par lancement si toute la latence fixe en plus disparaît (12,7 appels avec raisonnement × 2,9 s, YA-14), pour une phase agent moyenne de 83 s sur les tâches communes de YA-14. C'est détectable si l'effet est réel et si le nombre d'appels ne change pas ; la latence fixe sépare les deux causes.

## Portée du verdict

**Un verdict B vaut pour `default.json` et le banc seulement.** Le défaut produit reste `"auto"` jusqu'à une décision au journal du CDC.

Raison : en `off`, les blocs de raisonnement ont un texte vide. Ils gardent leur signature, donc le rejeu est intact (`crates/pa-ai/src/providers/transform_messages.rs:162-164`, `openai_responses_shared.rs:210-219`). Mais rien n'est affiché comme raisonnement :
- TUI : `crates/pa-tui/src/session.rs:210` ;
- ACP : `agent_thought_chunk`, `crates/pa-daemon/src/acp/wire_events.rs:108`.

Cela contredit YA-10, « raisonnement visible » (`docs/yusAgent-cdc.md:339`).

Le verdict est aussi limité à `gpt-6-luna` (un seul modèle) et aux 10 tâches courtes du banc.
