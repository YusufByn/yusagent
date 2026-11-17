# Ticket 14 : YA-09, miss du cache serveur en prompt v2

Ouvert le 16/11/2026 sur le verdict de l'A/B du cache v2. Rapport de l'A/B : `bench/smoke/ab/ya09-cache-v2-20261116.md`. Le verdict de la règle est un **écart net** : d = +5,3 points, IC 95 % [+3,0 ; +7,8].

**Décision de Yusuf (16/11, 15h02)** : chercher la cause dans les journaux du run `ab-20261116T124625Z`, sans nouveau run, et regarder comment Unharnessed gère le cache.
- Cette session ne fait aucun correctif : seulement le rapport, l'analyse et les options. Yusuf tranche avant tout code.
- Règle : si la cause est dans la v2, on corrige la v2, on ne revient pas au v1.

Analyse faite en lecture seule, sur `main` à `6b5288481` (branche `ya-14-cache-miss`). Aucun appel au modèle n'a été fait.

## Résumé

- **Le préfixe envoyé ne change pas avant un miss. Une cause client par modification du préfixe est exclue (prouvé).**
  - Les 637 appels hors premier de l'A/B partent tous en suite de conversation (`previous_response_id`).
  - Le code n'envoie une suite que si le corps hors `input` est identique octet pour octet à la requête précédente, et si l'`input` prolonge exactement l'input et la sortie précédents.
  - Sur les chauffes v2 du 16/11 dont les corps sont journalisés, les 16 miss et relectures partielles ont un corps identique au précédent.
- **Le transport n'est pas en cause (prouvé pour ce run).** Une seule connexion WebSocket par session, aucune reconnexion, aucune bascule SSE.
- **Les miss dépendent de la taille du prompt et du moment dans la session (mesuré, a posteriori).**
  - En A : 30,8 % de miss quand le prompt précédent fait moins de 3 000 tokens, 0 % au-delà de 16 000.
  - Aucun miss en A après 30 s de session, aucun en B après 60 s.
  - À prompt précédent ≥ 6 000 tokens, le v2 n'a pas plus de miss que le v1 : d = −1,9 point [−4,4 ; +0,4].
  - Le v2 a plus de miss parce que ses premiers appels ont de petits prompts : ses instructions sont 4 935 tokens plus courtes.
- **Cause : côté serveur, probable. Le mécanisme est inconnu.** Une différence client reste à tester, sans preuve qu'elle joue : l'en-tête d'affinité de cache.
  - Codex CLI envoie `session-id`, avec un tiret. Son code dit que ChatGPT en tire l'affinité de cache.
  - Nous envoyons `session_id`, avec un tiret bas.
- **Ce qui est en jeu (estimation)** : sur ce banc, environ 5 % du coût rapporté de A pour les miss en excès du v2. Environ 9 % pour tous ses miss (8,7 %). Environ 22 % pour tout le cache imparfait, relectures partielles comprises, dans les deux bras.

## 1. Données disponibles (inventaire)

Run `bench/smoke/runs/ab-20261116T124625Z`, par lancement et par tâche (`arms/<run>/tasks/<tâche>/run-1/`) :

| Source | Champs utiles | Absent |
|---|---|---|
| Journal d'usage (`sessions/<id>.usage.ndjson`), un enregistrement par appel | `seq`, `turn`, `kind`, `startMs`, `durationMs`, `ttftMs`, `sincePrevMs`, `input`, `cacheRead`, `cacheWrite`, `output`, `reasoning`, `stopReason`, `providerUsage` (usage brut du backend), `undocumentedAttribution.request_fields` (tokens d'`instructions` et de `tools`, et leur part en cache, par appel), `historyReasoning` | corps de requête, en-têtes |
| Journal du reverse proxy (`proxy.ndjson`) | `ws_open` et `ws_close` (durée, octets), par requête : horodatage, transport, `session` (`prompt_cache_key`), `continuation` (présence de `previous_response_id`), `bytes_up`, `generate` ; par réponse : `response_id`, statut, compteurs d'usage | corps, en-têtes, valeur de `previous_response_id`, trames de contrôle (ping, pong) |
| Session (`sessions/<id>.jsonl`) | messages et sorties d'outil de la session | requête telle qu'envoyée |
| Capture de la sonde (`probe/capture/*.meta.json`, serveur de capture, sans modèle) | **noms** des en-têtes envoyés par le binaire `318f3e1` | valeurs masquées |

**Non journalisé dans l'A/B** : les corps de requête. `--log-bodies` est refusé avec `--ab` (README, « Commandes »). Les corps existent pour les chauffes lancées avec `--log-bodies`, dont celles du 16/11 en prompt v2 (`20261116T091720Z-f81f980`, `20261116T105840Z-cce48e6`, `20261116T110353Z-cce48e6`). Le proxy ne voit pas non plus ce que le serveur fait d'une requête : machine, entrée de cache, éviction.

## 2. Le préfixe a-t-il changé avant un miss ?

**Non, pour tous les miss et toutes les relectures partielles mesurables.**

1. **Suite de conversation partout.** Dans l'A/B, 637 appels hors premier sur 637 partent avec `previous_response_id`, sur la connexion de la session (`cache_causes.py`, « en suite de conversation 261/261 » et « 376/376 »). Le drapeau `continuation` du proxy vaut « `previous_response_id` présent dans la trame `response.create` » (`bench/smoke/proxies.py:193-203`).
2. **Le code n'envoie une suite que si le préfixe est inchangé.**
   - `build_cached_websocket_request_body` (`crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:611-632`) ne pose `previous_response_id` que si deux conditions tiennent :
     - Le corps privé d'`input` et de `previous_response_id` est égal à celui de la requête précédente (`websocket.rs:577-591`). Cette égalité couvre `instructions`, `tools`, `model`, `reasoning`, `prompt_cache_key`, `text`, `include`, `store`, `tool_choice` et `parallel_tool_calls`.
     - L'`input` commence par l'input précédent suivi des items de la réponse précédente (`websocket.rs:592-608`).
   - Sinon, il envoie le corps complet, et le proxy aurait vu `continuation: false`.
   - Le préfixe d'une suite est donc la requête précédente plus sa réponse, tenues côté serveur.
3. **Instructions et tools stables.** L'attribution du backend donne, par session, une seule valeur de tokens d'`instructions` : 1 333 (tâches Node et `py-langextract-584`) ou 1 611 (`py-tldextract-*`) en A, 6 268 ou 6 546 en B. Les `tools` valent 90 en A et 163 en B. Sur chaque miss, la part en cache des instructions et des tools est 0, comme le reste du prompt.
4. **Corps journalisés (chauffes v2 du 16/11).** Sur 55 appels hors premier, aucun corps hors `input` et `previous_response_id` ne diffère du précédent. Les 16 miss ou partiels (`f81f980` 3 + 9, `cce48e6` 4 + 0) sont tous à corps identique et en suite de conversation (`cache_causes.py`, ligne « Corps journalisés »).
5. **Les relectures partielles relisent un préfixe plus ancien.** Pour 35 sur 35 en A et 52 sur 54 en B, `cacheRead` vaut, à un bloc près, le prompt arrondi à 128 tokens d'un appel antérieur de la même session : 2 à 12 appels en arrière, le plus souvent 2 (16 en A, 21 en B). Le serveur a donc retrouvé une entrée de cache plus ancienne de la même session, pas la plus récente. Ce préfixe ancien était intact.

**Conclusion** : la cause n'est pas un préfixe modifié par le client. C'est **prouvé** pour le run, d'après le code et le journal du proxy, et confirmé par les corps des chauffes.

## 3. Croisements

`bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json`. Tableaux : miss sur appels, hors premier appel.

**Transport et connexion** : une connexion WebSocket par session (40 `ws_open` pour 40 sessions), aucune connexion neuve en cours de session, aucune requête hors WebSocket. **La connexion n'explique aucun miss.**

**Silence depuis l'appel précédent** (fin de la réponse → requête, journal d'usage ; `ya09-cache-v2-20261116-mesures.py`, section 4) : en A, 23 miss sur 25 tombent après moins de 1 s de silence, et aucun au-delà de 5 s. En B, c'est 15 sur 16. **Le silence n'explique pas les miss.** Le constat des chauffes du 16/11 tient (passation du 16/11).

**Taille du prompt précédent** (tokens) :

| Tranche | A miss | A partiels | B miss | B partiels |
|---|---|---|---|---|
| < 3 000 | 16/52 (30,8 %) | 4 | aucun appel | |
| 3 000 à 6 000 | 5/38 (13,2 %) | 6 | aucun appel | |
| 6 000 à 9 000 | 1/25 (4,0 %) | 3 | 8/100 (8,0 %) | 7 |
| 9 000 à 16 000 | 3/91 (3,3 %) | 18 | 6/130 (4,6 %) | 20 |
| ≥ 16 000 | 0/55 | 4 | 2/146 (1,4 %) | 27 |

**Âge dans la session** (requête − première requête, au proxy, en secondes) :

| Tranche | A | B |
|---|---|---|
| < 10 | 19/76 (25,0 %) | 8/87 (9,2 %) |
| 10 à 30 | 6/83 (7,2 %) | 4/105 (3,8 %) |
| 30 à 60 | 0/62 | 4/108 (3,7 %) |
| ≥ 60 | 0/40 | 0/76 |

**Rang de l'appel** : au rang 2, 8/20 (40 %) en A et 5/20 (25 %) en B. Au rang 11 et au-delà, 1/84 (1,2 %) en A et 5/196 (2,6 %) en B.

**Taille et rang ensemble** (même script, section 4) : en A, à partir du rang 5, un prompt précédent sous 6 000 tokens donne 6 miss sur 35 et un prompt au-dessus 3 sur 166. La taille joue donc aussi hors du début de session. Le rang, la taille et l'âge restent liés : le banc ne les sépare pas complètement.

**Débit de requêtes** : la doc d'OpenAI cite un débordement de routage au-delà d'environ 15 requêtes par minute et par clé (section 6). Ici, les miss sont plus fréquents quand la session a fait **peu** de requêtes dans les 60 s précédentes :
- A : 20/113 de 1 à 5 requêtes, 1/45 de 11 à 20 requêtes.
- B : 8/103 et 3/150.

C'est l'inverse d'un débordement par débit. Cette piste n'est pas étayée (même script, section 4).

**Écart A − B par sous-population** (`bench/smoke/ab/ya09-cache-v2-20261116-mesures.py`, même bootstrap que la règle, **a posteriori**) :

| Sous-population | A | B | d | IC 95 % |
|---|---|---|---|---|
| toutes (règle) | 25/261 | 16/376 | +5,3 | [+3,0 ; +7,8] |
| prompt précédent ≥ 6 000 | 4/171 | 16/376 | −1,9 | [−4,4 ; +0,4] |
| ≥ 6 000 et âge < 30 s | 4/71 | 12/190 | −0,7 | [−5,8 ; +4,7] |
| < 6 000 | 21/90 (23,3 %) | aucun | | |

**Premier appel de session en cache** (même préfixe d'instructions qu'une session précédente, clé différente) : A 0/20, B 3/20.

## 4. Témoin : Codex CLI (A/B du 14/11, PR #34 et #36)

Run `bench/smoke/runs/ab-20261114T201248Z`. Les données par appel existent pour les deux bras : usage de chaque réponse au proxy, ce qui permet un calcul identique pour yusAgent et pour Codex.

`cache_causes.py --plan bench/smoke/runs/ab-20261114T201248Z/ab.json --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383` :

| Bras (5 tâches, 3 passes) | Miss | Relectures partielles | Rang 2 | Prewarm | Premier appel en cache |
|---|---|---|---|---|---|
| yusAgent `80e20ea` (prompt v1) | 8/297 (2,7 %) | 7/297 (2,4 %) | 2/15 | 0 | 6/15 |
| Codex CLI 0.169.0 | **2/155 (1,3 %)** | 3/155 (1,9 %) | 0/15 | 15 (un par session) | 9/15 |

Sur les 10 tâches du banc : Codex 3/348 (0,9 %), yusAgent 10/677 (1,5 %).

**Limite du témoin** : tous les prompts de Codex dépassent 9 000 tokens. Dans cette tranche, les taux de yusAgent restent faibles eux aussi : 2,5 % le 14/11 (v1), 2,1 % en A le 16/11 (v2, `cache_causes.py` sur le run du 16/11, tranches ≥ 9 000 sommées), 2,9 % en B. Le témoin ne dit donc rien des petits prompts, où se trouve l'excès du v2. Le jour et les binaires diffèrent aussi.

## 5. Ce qui est envoyé pour le cache : notre harness et Codex CLI

Notre harness à `318f3e1` (binaire de l'A/B). Les lignes citées sont celles de `main` à `6b5288481` : entre les deux, rien ne change dans `crates/`. Codex CLI 0.169.0 lu dans un clone jetable (`/tmp/codex-0.169.0-ya14`, tag `rust-v0.169.0`, commit `d32063f96c2d`), sans rien exécuter, chemins sous `codex-rs/`.

| Élément | yusAgent | Codex CLI 0.169.0 |
|---|---|---|
| `prompt_cache_key` | id de session (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs:668-670`, passé par `crates/pa-core/src/session_engine/engine.rs:875`) | id de session racine, ou clé du parent pour les sous-agents (`core/src/client.rs:712-732`, `:1307`) |
| `store` | `false` (`mod.rs:651`) | `false` (`core/src/client.rs:1302`) |
| `previous_response_id` | suite si corps hors `input` identique et `input` prolongé, même connexion (`websocket.rs:577-632`) | même règle (`core/src/client.rs:1675-1706`, `:2373-2393`) ; liste des cas en corps complet `:2327-2354` |
| Instructions | champ `instructions` (`mod.rs:653-659`) | **pas de champ `instructions`** : message développeur en tête de l'`input`, à id déterministe (`core/src/client.rs:1214-1224`) ; observé : aucun corps journalisé de Codex n'a `instructions` ni `tools` (chauffe `20261114T191608Z-codex0.169.0`) |
| Tools | champ `tools` | en Responses Lite (`use_responses_lite` du catalogue), item `AdditionalTools` dans l'`input` (`core/src/client.rs:1198-1213`) |
| `reasoning` | `{effort, summary}` (`mod.rs:704-708`) | `{effort, context: "all_turns"}` en Responses Lite (`core/src/client.rs:1153-1157`) ; observé dans les corps |
| `parallel_tool_calls` | `true` (`mod.rs:672`) | `false` en Responses Lite (`core/src/client.rs:1300`) |
| Prewarm | aucun | un `response.create` à `generate: false`, entrée vide, au démarrage de la session ; la première vraie requête le prolonge par `previous_response_id` (`core/src/client.rs:17-21`, `core/src/session/session.rs:2213`, `core/src/session/startup_prewarm.rs:400-401`). Le code le décrit comme une préparation de connexion, pas du cache (`core/src/client.rs:2253`, `:2360-2361`) |
| **En-tête d'affinité** | `session_id` (tiret bas) et `x-client-request-id`, égaux à l'id de session, à l'ouverture du WebSocket (`crates/pa-ai/src/providers/openai_codex_responses/request.rs:119-120`, `mod.rs:208`). Observé (noms seulement) à la capture de la sonde de l'A/B : `session_id`, `x-client-request-id`, `openai-beta`, `originator`, `chatgpt-account-id`, `idempotency-key`, `x-acp-model-request-id` (ces deux derniers : `crates/pa-core/src/session_engine/semantic_edges.rs:37-39`) | **`session-id`** et **`thread-id`** (tirets, `codex-api/src/requests/headers.rs:5-14`), `x-client-request-id`, `x-codex-window-id`, `x-codex-turn-metadata` (`core/src/client.rs:1559-1593`). Commentaire : « ChatGPT derives cache affinity from the Responses session-id header » (`core/src/client.rs:734-735`) |
| `client_metadata` | aucun | ids de session, de fil, de tour, de fenêtre ; drapeau Responses Lite (`core/src/responses_metadata.rs:338-388`, `core/src/client.rs:2320-2326`) |
| Rétention | aucun champ sur ce chemin (aucune occurrence de `retention` sous `openai_codex_responses/`) ; `PI_CACHE_RETENTION` ne s'y applique pas | `prompt_cache_options.ttl` possible, jamais posé par l'app-server, la TUI ni `exec` (`core/src/client.rs:1308-1310`) |
| Keepalive WebSocket | lecture du flux entre deux requêtes, pong (ticket 12) | répond aux pings du serveur (`codex-api/src/endpoint/responses_websocket.rs:136-142`) |

Le proxy du banc transmet les en-têtes du client tels quels, sauf les en-têtes de saut et `sec-websocket-extensions` (`bench/smoke/proxies.py:50-51`, `:366-368`). Cela vaut pour les deux harness.

Dans le banc, le provider de Codex n'est pas « OpenAI » (`ya14-codex-plan.md`, écart 1). Codex n'envoie donc pas certains en-têtes réservés au backend Codex, dont `x-codex-routing-hint` (`core/src/client.rs:1381-1390`, `:1431`). Le témoin les omet aussi.

**Ce que nous ne savons pas** : comment le backend ChatGPT utilise ces en-têtes et champs. Le commentaire de Codex est une source côté client, pas une documentation du serveur.

## 6. Doc publique d'OpenAI (API, pas le backend ChatGPT)

Page lue le 16/11 : `https://developers.openai.com/api/docs/guides/prompt-caching` (redirection de `platform.openai.com/docs/guides/prompt-caching`, sans date de mise à jour affichée). Extraits verbatim :
- « Cached states live on individual machines, where traffic above 15 requests per minute can lead to overflow routing. »
- « A request can reuse a cached prefix only if it reaches a machine holding a matching entry that has not expired. »
- « Keys influence routing; they do not pin requests to a machine or guarantee a cache hit. »
- « Reusing context within a session can preserve a shared prompt prefix, but maintaining a session doesn't guarantee a cache hit. »
- « The minimum cacheable prompt length is 1,024 tokens for GPT-5.6 and later […] »
- La page ne parle ni de `previous_response_id`, ni de `store`, ni du délai avant qu'une entrée écrite soit relisible.

**Portée** : c'est la doc de l'API publique. Rien ne dit que le backend d'abonnement (`chatgpt.com/backend-api/codex`) suit les mêmes règles. Ces extraits ne prouvent rien pour notre cas. Ils disent seulement qu'un hit n'est pas garanti, même à préfixe identique.

## 7. Unharnessed

Clone jetable `/tmp/unharnessed-ya14`, `main` à **`fe2cf2815f78463464b91c598fc0c31c0af98f4c`** (15/11/2026 19:15 +0100, « The extension's version is 0.0.4 »). C'est le commit déjà lu pour le ticket 11 : aucun commit depuis. Rien n'a été exécuté ni compilé. Chemins de leur dépôt.

**Trouvé :**
- **Envoi pour le cache sur `openai-codex` : identique à Prime Agent `3358e0016` et au nôtre**, avec les mêmes champs, les mêmes en-têtes et la même règle de suite de conversation.
  - Corps : `prompt_cache_key` = id de session (`crates/uh-ai/src/providers/openai_codex_responses/mod.rs:678-680`), `store: false` (`:661`), `include: ["reasoning.encrypted_content"]` (`:677`).
  - En-têtes WebSocket : `session_id` et `x-client-request-id` (`request.rs:102-129`).
  - Suite de conversation : `websocket.rs:578-640`.
  - Pas de `client_metadata`, pas de prewarm, pas de rétention sur ce chemin.
- **Leur seul changement de cache, `af8046102`** (« Send the session id with every request: the OpenAI/Codex prompt cache works », 11/10/2026) :
  - Il passe l'id de session au provider (`crates/uh-core/src/session_engine/engine.rs:537-540`).
  - Il corrige le signal d'annulation des requêtes sur une connexion réutilisée.
  - Notre `main` a l'équivalent : `crates/pa-core/src/session_engine/engine.rs:875` et `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:47-50`.
- **Transport WebSocket et SSE** : `git diff -M 3358e0016 fe2cf2815` sur le dossier du provider donne 16 lignes ajoutées et 7 retirées, `af8046102` et le passage de `service_tier` (`5ce86e209`). Rien sur le keepalive ni sur le 1011. Le worker ne lit pas le flux entre deux requêtes (`websocket.rs:254-260`) : c'est le défaut corrigé chez nous au ticket 12.
- **Anneau de contexte de l'extension VS Code** (`vscode/webview/Usage.tsx`, commit `21c8f3903`) :
  - Remplissage = tokens / fenêtre, plafonné à 1 (`Usage.tsx:108-113`).
  - Les tokens sont l'usage de la dernière réponse valide (`input + output + cacheRead + cacheWrite`, cache lu compris), plus une estimation à 4 caractères par token des messages suivants (`crates/uh-core/src/session_engine/compaction.rs:44-50`, `:316-334`).
  - Au survol, « Cache N% » = cache réutilisé / réutilisable, et l'économie en dollars (`Usage.tsx:146-151`).
  - Le calcul est fait par réponse : réutilisable = min(prompt précédent, prompt courant), réutilisé = min(cacheRead, réutilisable) (`crates/uh-core/src/session/manager/queries.rs:62-112`).
  - Le calcul repart de zéro à une compaction, à un changement de modèle ou à un changement d'effort.
  - C'est leur seule mesure du cache. Elle est proche de notre taux « évitable », sans distinguer miss et relectures partielles.

**Non trouvé** :
- aucun commit ni aucune issue sur le cache, le 1011 ou le keepalive. `gh issue list --state all` avec `cache` et avec `1011` ne renvoie rien ; le dépôt a 2 issues en tout.
- aucune mesure de miss serveur, aucun prewarm, aucun en-tête `session-id`.

**Conclusion** : Unharnessed ne fait rien de plus que nous pour le cache serveur. Leur prompt est proche de notre v2, avec des instructions courtes. Ils devraient donc avoir les mêmes miss sur les petits prompts, mais ce n'est pas mesuré.

## Cause

| Hypothèse | Statut | Preuve |
|---|---|---|
| Préfixe modifié par le client (prompt, tools, réglages, `input` non prolongé) | **exclue, prouvé** | section 2 |
| Changement de connexion, reconnexion, bascule SSE, keepalive | **exclue pour ce run, prouvé** | section 3 : 1 connexion par session, 0 SSE, 0 « 1011 » |
| Long silence, expiration de l'entrée | **exclue** | section 3 : 23 miss sur 25 en A à moins de 1 s de silence |
| Débordement par débit de requêtes (doc API, environ 15 par minute et par clé) | **non étayée** | section 3 : les miss vont avec un débit faible |
| Petit prompt, début de session | **associé, mesuré a posteriori** | section 3 : 23,3 % de miss sous 6 000 tokens en A ; à taille égale, pas d'écart A − B |
| Mécanisme serveur qui produit ces miss | **inconnu** | le journal ne voit pas le serveur ; la doc de l'API ne s'applique pas forcément au backend d'abonnement |
| Affinité de cache par l'en-tête `session-id` (Codex) au lieu de `session_id` (nous) | **à tester, sans preuve** | section 5 : commentaire côté client chez Codex, rien côté serveur |

**Cause retenue : probable, côté serveur, déclenchée par les petits prompts du début de session.** Le v2 n'introduit aucune instabilité de préfixe. Il réduit les instructions de 6 268 à 1 333 tokens, ce qui place ses premiers appels dans la zone où le serveur rate souvent. Ce n'est pas un défaut du texte du v2, et la règle « corriger le v2, pas revenir au v1 » ne trouve pas de défaut à corriger dans le prompt. Grossir le prompt pour éviter la zone coûterait des tokens à chaque appel, sans mécanisme connu.

## Options de correctif

Aucune n'est codée. Toutes se mesurent par un A/B entrelacé avec une règle fixée avant le run, comme celui-ci : mêmes 5 tâches, 4 passes. Chaque variante est un réglage dans la même branche (règle 5).

Métrique de décision proposée :
- le taux de miss global ;
- le taux de miss des appels dont le prompt précédent fait moins de 6 000 tokens, où se trouve l'excès (environ 90 appels par bras sur 4 passes ; puissance à calculer dans le plan) ;
- les relectures partielles et le coût, affichés sans décider.

| Option | Ce qu'elle touche | Préfixe et cache | Mesure qui la validerait |
|---|---|---|---|
| **A. En-tête `session-id`** (tiret), égal à l'id de session, en plus de `session_id`, à l'ouverture du WebSocket et en SSE. Éventuellement `thread-id`, comme Codex | **Upstream, couche providers** : 2 lignes dans `crates/pa-ai/src/providers/openai_codex_responses/request.rs`. Variante qui évite la couche providers : en-têtes ajoutés par un module du fork via `options.base.headers`, fusionnés par `request.rs:76-78` et `:104-106`. À lister dans `fork-changes.md` (règles 2 et 3) | **Aucun changement du préfixe** (en-têtes hors prompt), pas de rupture de cache | A/B réglage on contre off, prompt v2 dans les deux bras. Gagne si les miss sous 6 000 tokens baissent nettement. Le moins cher à essayer |
| **B. Prewarm au démarrage de session** : `response.create` à `generate: false` avec les instructions, les tools et un `input` vide, puis première requête en suite de conversation, comme Codex | **Upstream, couche providers** (`mod.rs`, `websocket.rs`), ou module du fork qui appelle le provider ; corps et nombre de requêtes changent | **Pas de changement du préfixe** des vraies requêtes. Une requête en plus par session, dont le coût est à mesurer : le banc la compte déjà comme appel `prewarm` chez Codex | A/B réglage on contre off : miss aux rangs 2 à 4 et sous 6 000 tokens, coût total prewarm compris. Doute : le code de Codex décrit le prewarm comme une préparation de connexion, pas de cache (`core/src/client.rs:17-21`) |
| **C. Forme « Responses Lite »** : instructions et tools en items d'`input`, `reasoning.context: "all_turns"`, `parallel_tool_calls: false` | **Upstream, couche providers**, gros changement ; dépend d'un drapeau du catalogue | **Rupture de préfixe pour toutes les sessions neuves** (corps différent) ; risque sur le comportement du modèle | À n'envisager qu'après A et B, et seulement si un indice relie cette forme au cache. Aucun aujourd'hui |
| **D. Statu quo** : documenter, mesurer les miss dans chaque A/B (`cache_causes.py`) | rien | rien | sans objet. Gain maximal en jeu, estimé : environ 5 % du coût rapporté de A pour l'excès du v2 |

Une option est **écartée** : grossir le prompt v2 pour dépasser 6 000 tokens dès le premier appel. Elle coûte des tokens à chaque appel, son mécanisme est inconnu, et elle irait contre le gain du v2 (−29 % de coût pour le v1 → v2, soit B / A − 1 = +40,1 %).

**Gain en jeu (estimation)** : la perte de cache se décompose en 42 % sur les miss, 34 % sur les relectures partielles et 24 % ailleurs (`ya09-cache-v2-20261116-mesures.py`, section 3). Elle vaut 21,7 % du coût rapporté de A. Les miss de A sous 6 000 tokens perdent environ 57 000 tokens de cache sur 4 passes, sur 241 096. Leur perte est estimée comme le prompt précédent arrondi à 128 tokens (même script, section 5). Cela fait environ 5,1 % du coût rapporté de A. Supprimer tous les miss en ferait environ 8,7 % (estimation).

Les relectures partielles pèsent presque autant que les miss, dans les deux bras. Aucune option ci-dessus ne les vise directement.

## Points ouverts

- **Dérive des relectures partielles en prompt v1**, mêmes 5 tâches, calculée par `cache_causes.py` :
  - 2,4 % le 14/11 (`80e20ea`) ;
  - 7,9 % (`auto`) et 5,7 % (`off`) le 15/11 (`7086711`, unité écartée comprise) ;
  - 14,4 % le 16/11 (`318f3e1`).
  - Les miss passent de 2,7 %, 2,7 % et 2,5 % à 4,3 %.
  - Le jour et le binaire changent ensemble : lot Unharnessed, keepalive du ticket 12. On ne peut pas trancher sans un run qui fixe l'un des deux, par exemple le binaire `80e20ea` rejoué le même jour que `318f3e1`, à décider par Yusuf.
- **Premier appel en cache entre sessions** : 3/20 en B, 0/20 en A. Le préfixe commun de A (instructions et tools, 1 423 tokens) dépasse le minimum de 1 024 tokens de la doc de l'API, sans jamais être relu d'une session à l'autre. Ce point n'est pas expliqué.
- **Ping du serveur** : son intervalle est toujours inconnu (passation du 16/11).

## Commandes

Depuis la racine du dépôt, en lecture seule :

```bash
python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-20261116T124625Z/ab.json
python3 -I bench/smoke/ab/ya09-cache-v2-20261116-mesures.py bench/smoke/runs/ab-20261116T124625Z
python3 -I bench/smoke/tools/cache_causes.py bench/smoke/runs/20261116T091720Z-f81f980-gpt-6-luna bench/smoke/runs/20261116T105840Z-cce48e6-gpt-6-luna bench/smoke/runs/20261116T110353Z-cce48e6-gpt-6-luna
python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-20261114T201248Z/ab.json --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383
python3 -I bench/smoke/tools/cache_causes.py --plan bench/smoke/runs/ab-20261115T153500Z/ab.json --tasks node-koa-2029,node-tough-cookie-630,py-langextract-584,py-tldextract-368,py-tldextract-383
```

- La 1ʳᵉ commande donne les tranches de taille, d'âge et de rang, le transport, le préfixe relu et le premier appel en cache.
- La 2ᵉ donne les écarts par sous-population, le coût du cache imparfait, les pertes par classe, les croisements avec le silence, le débit et le rang, et la perte des miss par taille.
- La 3ᵉ donne les corps des chauffes v2.
- Les 4ᵉ et 5ᵉ donnent le témoin Codex CLI et la dérive en v1.

## Option A : en-tête `session-id` (17/11/2026)

**Décision de Yusuf (17/11, 12h05)** : option A seule, derrière un réglage coupé par défaut tant que l'A/B n'a pas tranché. Branche `ya-14-session-id` depuis `main` à `13d931e87`.

### Ce que fait Codex CLI

Lu sans rien exécuter dans `/tmp/codex-0.169.0-ya14` (tag `rust-v0.169.0`, `d32063f96c2dff1dc96af839b3952abb6acfeda4`) et dans un clone superficiel de `main`, `/tmp/codex-main-ya14-sid` (`59ac39addbbcd3499319c805a916846ca8ac5fef`, 17/11/2026). Le code est identique dans les deux ; seules les lignes changent.
- **Nom** : `insert_header(&mut headers, "session-id", &id);` (`codex-rs/codex-api/src/requests/headers.rs:8` au tag, `:28` sur `main`, fonction `build_session_headers`). La même fonction pose `thread-id`.
- **Commentaire** : « ChatGPT derives cache affinity from the Responses session-id header. Keep the actual session identity in turn metadata, hooks, and history/notes requests. » (`codex-rs/core/src/client.rs:734-735` au tag, `:760-761` sur `main`).
- **Valeur** : `responses_session_id`, juste en dessous. Pour une session racine, c'est `session_derived_cache_key` (`client.rs:720-732`) : l'id de session, comme `prompt_cache_key`.
- **Transports** : poignée de main WebSocket (`client.rs:1571-1574`) et requête HTTP (`codex-api/src/endpoint/responses.rs:90`).
- Codex n'envoie pas `session_id` (tiret bas) en en-tête : aucune occurrence hors des tests sous `codex-api` ni dans `client.rs`.

### Ce qui change

- Réglage `codexSessionIdHeader` (booléen, absent = coupé), module du fork `crates/pa-core/src/session_engine/ya_session_id_header.rs`.
- À `true`, `session-id` = id de session s'ajoute aux en-têtes de requête du `stream_fn` (`add_session_id_header`, `:47-58`), seulement si le provider est `openai-codex` et l'API `openai-codex-responses` (`is_chatgpt`, `:40-42`). Le provider fusionne ces en-têtes sur la poignée de main WebSocket et sur la requête SSE (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs:209-222`, `request.rs:76-78`, `:104-106`). `session_id` et `x-client-request-id` d'upstream restent.
- **Couche providers non touchée** : aucun fichier sous `crates/pa-ai/` dans le diff.
- Figé dans `yaRequest` (`crates/pa-core/src/session_engine/ya_request.rs:334-337`), écrit seulement à `true`.

### Corps et préfixe

**L'en-tête ne change ni le corps ni le préfixe des requêtes.** Le test `codex_websocket_session_id_header_follows_the_setting` (`crates/pa-core/tests/prefix_stability.rs`) fait tourner trois sessions du même projet sur le WebSocket Codex : réglage absent, `false`, puis `true`. Leurs trames `response.create` sont identiques octet pour octet, hors `prompt_cache_key` (l'id de session, propre à chaque session). La deuxième requête continue la première (`previous_response_id`) dans les trois cas. Le prompt système et les tools ne dépendent pas du réglage.

### Preuve sur le fil

Les serveurs locaux de `prefix_stability.rs` gardent désormais les en-têtes reçus. Les tests passent par une vraie session (`create_session`, adaptateur provider réel, kernel réel) :
- `codex_websocket_session_id_header_follows_the_setting` : la requête d'upgrade WebSocket porte `session-id` égal à `session_id` et à `prompt_cache_key` quand le réglage est activé. Il est absent quand le réglage est absent ou à `false`.
- `session_id_header_is_never_sent_to_other_providers` : réglage activé, aucune requête ne porte `session-id` pour Anthropic Messages ni pour `OpenAI` Responses (modèle hors famille Codex et `gpt-6.1-terra`). Les préfixes restent stables.

Tests unitaires : `ya_session_id_header.rs` (providers, en-têtes existants gardés, `stream_fn` rendu tel quel quand coupé) et `ya_request.rs` (gel dans `yaRequest`, chargement du réglage).

Le SSE du provider Codex n'a pas de test sur le fil : le chemin des en-têtes est le même (`additional_headers`, test upstream `additional_headers_override_model_headers`, `openai_codex_responses/mod.rs:872`). Sur le vrai binaire, le contrôle `--fil` du plan, en mode capture, vérifie chaque requête des tâches.

### Plan d'A/B et contrôle de la dérive

`bench/smoke/ab/ya09-session-id-plan.md`, à valider par Yusuf avant le run : bras réglage coupé contre activé, même binaire, 4 passes entrelacées. Métrique principale : taux de miss sous 6 000 tokens de prompt précédent. Le plan contient aussi le contrôle de la dérive des relectures partielles en v1 (binaire `80e20ea` relancé tel quel), faisable d'après la lecture du code.
