# Usage des appels modèle (T04, T08, YA-13)

Ce document fixe la sémantique des champs d'usage après normalisation par la couche providers, et le format du journal d'usage par appel.

## Champs de `Usage`

`Usage` (`crates/pa-types/src/ai/mod.rs:423`, miroir côté boucle `crates/pa-agent/src/types.rs:149`) :

| Champ (JSON) | Sens après normalisation |
|---|---|
| `input` | Tokens d'entrée **hors cache** : ni lus ni écrits en cache. |
| `cacheRead` | Tokens d'entrée lus dans le cache. |
| `cacheWrite` | Tokens d'entrée écrits dans le cache par cette requête. |
| `cacheWrite1h` | Part de `cacheWrite` écrite avec le TTL d'une heure, quand le provider donne la ventilation. Absent sinon. Le reste (`cacheWrite - cacheWrite1h`) est à 5 min. Propre à Anthropic. |
| `output` | Tokens de sortie, raisonnement compris. |
| `reasoning` | Tokens de raisonnement, **sous-ensemble de `output`**. Absent quand le provider ne les expose pas, jamais 0 par défaut. |
| `totalTokens` | Total tel que calculé ou rapporté par l'adaptateur (inchangé par T04). |
| `cost` | Coût calculé depuis les tarifs du catalogue (inchangé par T04 et T08 : toute écriture au tarif d'écriture du catalogue, quel que soit son TTL). |

Les deux champs optionnels sont ajoutés par T04 (`crates/pa-types/src/ai/mod.rs:436`, `:444` ; `crates/pa-agent/src/types.rs:164`, `:171`). Ils ne sont pas écrits quand ils valent `None` : une session écrite avant T04 se relit sans erreur, et une session sans ces valeurs s'écrit avec exactement les mêmes champs qu'avant (`crates/pa-core/tests/usage_backcompat.rs`). T08 n'ajoute aucun champ à `Usage` : la ventilation 30 min / 12 h de l'écriture OpenAI n'est que dans le journal d'usage, lue dans l'usage brut (voir plus bas).

Conséquence de la normalisation : pour tous les providers, `input + cacheRead + cacheWrite` est la taille totale du prompt de la requête.

## Normalisation par provider

| Provider (API) | Ce que renvoie l'API | `input` | `cacheRead` | `cacheWrite` | `reasoning` | Où |
|---|---|---|---|---|---|---|
| Anthropic Messages | `input_tokens` = tokens après le dernier breakpoint ; total = `cache_read_input_tokens + cache_creation_input_tokens + input_tokens` (doc prompt caching) | `input_tokens` | `cache_read_input_tokens` | `cache_creation_input_tokens` ; `cacheWrite1h` = `cache_creation.ephemeral_1h_input_tokens` | `output_tokens_details.thinking_tokens` (message_delta final) | `crates/pa-ai/src/providers/anthropic/stream.rs:297-309` (message_start), `:596-608` (message_delta), `:711` |
| OpenAI Responses (et Azure, Codex, même processeur) | `input_tokens` inclut `input_tokens_details.cached_tokens` et `input_tokens_details.cache_write_tokens` (`cached_tokens` exclut les tokens facturés en écriture) ; ventilation `input_tokens_details.cache_write_tokens_details.ttl_30m_tokens` / `ttl_12h_tokens` ; `reasoning_tokens` compté dans `output_tokens` | `input_tokens - cached_tokens - cache_write_tokens` (saturé à 0) | `cached_tokens` | `cache_write_tokens` (T08 ; 0 si absent). La ventilation n'entre pas dans `Usage` : le journal la lit dans `providerUsage` | `output_tokens_details.reasoning_tokens` | `crates/pa-ai/src/providers/openai_responses_stream.rs:770-812` |
| Chat Completions (OpenAI, OpenRouter, DeepSeek, Kimi…) | `prompt_tokens` inclut le cache (`prompt_tokens_details.cached_tokens` ou `prompt_cache_hit_tokens`), écriture dans `cache_write_tokens` (OpenRouter) | `prompt_tokens - cacheRead - cacheWrite` | `cached_tokens` (moins `cache_write_tokens` si présent, observé sur OpenRouter) | `cache_write_tokens` | `completion_tokens_details.reasoning_tokens` | `crates/pa-ai/src/providers/openai_completions/convert.rs:404-450` |
| Gemini (Google, Vertex) | `promptTokenCount` inclut `cachedContentTokenCount` ; `thoughtsTokenCount` est un terme à part du total | `promptTokenCount - cachedContentTokenCount` | `cachedContentTokenCount` | 0 | `thoughtsTokenCount` (déjà ajouté à `output`) | `crates/pa-ai/src/providers/google_stream.rs:254-277` |
| Mistral | `prompt_tokens`, `completion_tokens` | `prompt_tokens` | 0 | 0 | absent | `crates/pa-ai/src/providers/mistral/stream.rs:511-532` |
| Bedrock Converse | `inputTokens`, `cacheReadInputTokens`, `cacheWriteInputTokens` | `inputTokens` | `cacheReadInputTokens` | `cacheWriteInputTokens` | absent | `crates/pa-ai/src/providers/bedrock/events.rs:402-430` |

Sources vérifiées (03/11/2026 ; écriture du cache Responses : 04/11/2026) :

- Anthropic : <https://platform.claude.com/docs/en/build-with-claude/streaming> (usage de `message_start` sous `message.usage`, comptes du `message_delta` cumulés, exemple de `message_delta` avec `output_tokens` seul), <https://platform.claude.com/docs/en/build-with-claude/prompt-caching> (sémantique de `input_tokens`, formule du total, objet `cache_creation`), <https://platform.claude.com/docs/en/build-with-claude/extended-thinking> (`output_tokens_details.thinking_tokens`, sur le `message_delta` final).
- OpenAI : <https://developers.openai.com/api/docs/guides/prompt-caching> (taux de cache = cached / input, donc cached inclus dans input ; T08 : `cache_write_tokens` et `cache_write_tokens_details` sous `input_tokens_details`, les deux TTL toujours renvoyés et sommés à `cache_write_tokens`, `cached_tokens` hors écriture, écriture 30 min à 1,25 × et 12 h à 2,5 × le tarif d'entrée), <https://developers.openai.com/api/docs/guides/reasoning> (raisonnement facturé en sortie, `output_tokens_details.reasoning_tokens`), types du SDK officiel `openai-python` (`response_usage.py`, `completion_usage.py`).
- Gemini : <https://aistudio.google.com/api-reference/generate-content> (`UsageMetadata`).
- DeepSeek : <https://api-docs.deepseek.com/api/create-chat-completion> (`prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens`).
- OpenRouter : <https://openrouter.ai/docs/use-cases/usage-accounting> (`cached_tokens`, `cache_write_tokens`).

Tests sur fixtures : `crates/pa-ai/src/providers/anthropic/usage_tests.rs`, `crates/pa-ai/src/providers/usage_tests.rs` (Responses avec et sans écriture, saturation de `input`, hook `on_usage`, coût des écritures au tarif du catalogue ; Gemini), `crates/pa-ai/src/providers/openai_completions/usage_tests.rs` (DeepSeek, OpenRouter). L'absence pour Mistral est couverte par le test upstream `usage_total_tokens_explicit_zero_falls_back_to_sum` (`crates/pa-ai/src/providers/mistral/stream.rs:847`), qui compare l'usage entier.

### Correctif Anthropic

Avant T04, l'usage du `message_start` était lu sur `message.*` au lieu de `message.usage.*` : seul le `message_delta` remplissait l'usage. Avec un `message_delta` qui ne porte que `output_tokens` (cas des exemples de la doc), `input`, `cacheRead` et `cacheWrite` restaient à 0. Le `message_start` est maintenant lu sous `message.usage` (`stream.rs:297`) ; le `message_delta` ne remplace que les champs qu'il porte, sans addition (`stream.rs:596-608`), puisque ses comptes sont cumulés.

Effet de bord à noter : `totalTokens` est désormais juste dans ce cas, et c'est lui qui sert d'estimation de la taille du contexte pour le seuil de compaction (`crates/pa-core/src/session_engine/compaction.rs:50-55`). Avec un flux réel dont le `message_delta` ne porte que `output_tokens`, la compaction par seuil se déclenche désormais quand elle le doit ; avant, elle voyait 8 tokens.

### OpenAI Responses : rétention et TTL 12 h (T08)

Ce que le harness envoie pour `cacheRetention` (relevé au 04/11/2026) :

| Provider | `cacheRetention: long` | `cacheRetention: short` | Où |
|---|---|---|---|
| OpenAI Responses (`openai`) | `prompt_cache_retention: "24h"` si le compat `supportsLongCacheRetention` le permet (défaut oui), plus `prompt_cache_key` | `prompt_cache_key` seul | `crates/pa-ai/src/providers/openai_responses.rs:71-80`, `:210-218` |
| Codex (`openai-codex`) | rien : `prompt_cache_key` seul, aucun paramètre de rétention | idem | `crates/pa-ai/src/providers/openai_codex_responses/mod.rs:654-656` |
| Azure | rien : `prompt_cache_key` seul | idem | `crates/pa-ai/src/providers/azure_openai_responses.rs:195-200` |

- Le guide réserve `prompt_cache_retention` (`in_memory`, `24h`) aux modèles antérieurs à GPT-5.6 ; pour GPT-5.6 et suivants, la durée se règle par `prompt_cache_options.ttl` (`"30m"` par défaut, ou `"12h"`) et `prompt_cache_breakpoint.ttl`. **Effet de `prompt_cache_retention: "24h"` sur GPT-5.6 et suivants : non documenté.** Le harness n'envoie ni `prompt_cache_options` ni `prompt_cache_breakpoint` (aucune occurrence dans `crates/`) : aucune écriture à 12 h n'est attendue, l'écriture est à 30 min.
- **TTL 12 h quand même renvoyé** (`ttl_12h_tokens > 0`) : le journal l'écrit (`cacheWrite12h`, lu dans `providerUsage`) et le signale par `costWarning` sur la ligne. **Coût du harness : toute écriture au tarif d'écriture du catalogue** (`cost.cacheWrite`, un seul tarif, celui de 30 min pour OpenAI : Luna 0,125, Terra 1,25), quel que soit son TTL (`calculate_cost_values`, `crates/pa-ai/src/models.rs:24`, inchangé). Décision de Yusuf du 04/11/2026 : `Usage` ne porte pas la ventilation. Une écriture 12 h est donc sous-estimée de moitié dans `cost` (le guide : 30 min à 1,25 × l'entrée, 12 h à 2,5 ×) ; `costWarning` le dit. Le banc smoke, lui, tarife chaque part depuis `bench/smoke/prices.json` (`cache_write_30m`, `cache_write_12h`).
- Codex (`chatgpt.com/backend-api/codex`) : la présence de `cache_write_tokens` et de sa ventilation dans sa réponse n'était pas connue avant le run réel de T08 ; l'usage brut est désormais au journal (`providerUsage`).

## Taux de cache

```
taux de cache d'une requête = cacheRead / (input + cacheRead + cacheWrite)
```

Le dénominateur est le prompt complet pour tous les providers (voir la normalisation ci-dessus), donc la formule du ticket s'applique telle quelle. Elle vaut `null` quand le dénominateur est nul. Calcul : `UsageCounts::cache_rate` (`crates/pa-core/src/session_engine/usage_log.rs:78`).

## Journal d'usage par appel

Fichier `<id>.usage.ndjson` (format JSON Lines) à côté du fichier de session `<id>.jsonl` (`usage_log_path`, `crates/pa-core/src/session_engine/usage_log.rs:406`). L'extension n'est pas `.jsonl` : une quinzaine de tests upstream prennent le premier `.jsonl` du dossier de sessions pour la session (par exemple `crates/pa-daemon/tests/thinking_level_e2e.rs:365`), et upstream évite le même problème pour son fichier annexe `<id>.info-cache.json` (`crates/pa-daemon/src/session_store/info_sidecar.rs:1`). Le ticket demandait `<id>.usage.jsonl` ; extension changée avec l'accord de Yusuf (03/11/2026). Une ligne par appel modèle, ajoutée à la fin de l'appel. **Local uniquement** : le module écrit dans ce fichier et n'a aucun autre puits (pas de réseau, pas de télémétrie). Une session en mémoire, sans fichier, n'a pas de journal.

Par précaution, une ligne ne contient jamais les clés `id`, `timestamp` ou `cwd` : même lue par un scan, elle ne passe pas pour un en-tête de session (`crates/pa-core/src/session/manager/header.rs:7`, test `a_line_is_never_read_as_a_session_header`).

| Champ | Contenu |
|---|---|
| `v` | Version du format (1). |
| `ts`, `startMs` | Début de l'appel (ISO 8601 UTC, ms depuis l'époque). |
| `sessionId`, `parentSessionId` | Session, et session parente pour un sous-agent (`null` sinon), lue dans l'en-tête du parent (`parentSession`). |
| `seq` | Numéro de l'appel dans la session, continué après un redémarrage (`--continue`). |
| `turn` | Numéro du tour : il augmente quand le dernier message utilisateur de la requête change (nouveau prompt). Les allers-retours de tools d'un même prompt gardent le même tour. Les appels annexes portent le tour courant. |
| `kind` | `main`, `subagent` (appel de boucle d'une session qui a un parent), `compaction`, `refine`, `other`. |
| `purpose` | Pour `other` : `side_question` ou `branch_summary`. |
| `provider`, `api`, `model` | Cible de la requête. |
| `effort` | Effort de raisonnement demandé (nom du niveau : `off`, `low`, `medium`…), `null` si l'appel n'en fixe pas. |
| `stopReason` | `stop`, `length`, `toolUse`, `error`, ou `aborted` (flux interrompu ou abandonné). |
| `input`, `output`, `cacheRead`, `cacheWrite` | Usage normalisé (voir plus haut). |
| `cacheWrite5m`, `cacheWrite1h` | Ventilation de `cacheWrite` par TTL, seulement quand le provider la donne (Anthropic). |
| `cacheWrite30m`, `cacheWrite12h` | (T08) Ventilation de `cacheWrite` par TTL, lue dans `providerUsage` (`input_tokens_details.cache_write_tokens_details.ttl_30m_tokens` / `ttl_12h_tokens`, format OpenAI Responses), chacune présente quand l'usage brut la donne, telle que reçue, sans correction si elle ne se somme pas à `cacheWrite`. |
| `costWarning` | (T08) Présent quand `cacheWrite12h > 0` : `"cacheWrite12h priced at the catalog write rate"` (`CACHE_WRITE_12H_WARNING`, `crates/pa-core/src/session_engine/usage_log.rs:29`). Le `cost` de la ligne compte ces tokens au tarif d'écriture du catalogue. |
| `reasoning` | Tokens de raisonnement, seulement quand le provider les donne. |
| `totalTokens`, `cost` | Total et coût calculé (`cost.total`). |
| `cacheRate` | Taux de cache de la requête, `null` sans prompt. |
| `ttftMs` | Du début de l'appel au premier événement de contenu (texte, raisonnement ou tool call), `null` si rien n'a été streamé. |
| `durationMs` | Du début de l'appel à sa fin. |
| `sincePrevMs` | Délai depuis le début de l'appel précédent **du même `kind`** dans la session (`null` pour le premier), relu dans le journal après un redémarrage. Le cache se rafraîchit à chaque lecture, donc ce délai, comparé au TTL (5 min ou 1 h), dit si le préfixe de la boucle était encore chaud. Les appels annexes ont leur propre préfixe et leur propre délai. |
| `historyReasoning` | Raisonnement renvoyé au modèle dans l'historique de cette requête, mesuré sur le corps exact envoyé : `blocks` (blocs ou items de raisonnement), `textChars` (caractères de texte lisible), `textTokensEst` (**estimation** : caractères / 4, l'heuristique de `compaction::estimate_tokens`), `opaqueBytes` (signatures, raisonnement chiffré ou masqué, en octets). Aucun provider ne donne ce compte en tokens. Absent si le corps n'a pas été observé (échec avant l'envoi). |
| `providerUsage` | (T08) Objet usage brut renvoyé par le provider pour cet appel (usage seul, aucun contenu de message), tel que reçu : `response.usage` de `response.completed` pour Responses, Azure et Codex. Absent quand le provider ne l'a pas passé au hook `on_usage` (les autres providers, pour l'instant). Le dernier reçu si l'appel en voit plusieurs. Champ optionnel : la version `v` ne change pas. |
| `invalidation` | (T05, YA-05) Le harness a changé le préfixe de cette requête exprès. Seulement sur les appels de la boucle, absent sinon. Masquage (`docs/fork-changes.md`, « Masquage (T05) ») : `{"cause": "masking", "turn": N, "frontier": F, "masked": n}`, posé quand la liste des sorties masquées change (une fois par palier au plus). Le `cacheWrite` de la même ligne est le coût de cette rupture. Champ optionnel : la version `v` ne change pas. |

Points d'accroche :

- Appels de la boucle (et de leurs retries) : `instrument_stream_fn` enveloppe le `StreamFn` de la session (`crates/pa-core/src/session_engine/engine.rs:707-717`), la question annexe a son propre enveloppement (`purpose: side_question`). Le corps est observé par le hook `on_payload`, qui le rend inchangé.
- Usage brut (T08) : hook générique `on_usage` de `pa_ai::types::StreamOptions` (`crates/pa-ai/src/types.rs:70`, `:86`), qui reçoit l'objet usage brut et le modèle, sans présumer de format. Le processeur Responses l'appelle à `response.completed` (`crates/pa-ai/src/providers/openai_responses_stream.rs:771-774`), transmis par `ResponsesStreamHooks` (`openai_responses_hooks.rs:37`). Miroir sur les options de la boucle (`crates/pa-agent/src/stream.rs:135`, `:154`), passage par `provider_adapter.rs:84`, `:180`. La sonde du journal branche le hook comme `on_payload` (`usage_log.rs:703` pour la boucle, `:840` pour les appels annexes), en appelant d'abord un hook déjà présent ; la ligne en tire `cacheWrite30m`, `cacheWrite12h` et `costWarning` (`usage_log.rs:413-431`). Rien n'est écrit dans les fichiers de session ni dans la requête.
- Appels annexes (`pa_ai` direct) : résumé de compaction (`crates/pa-core/src/session_engine/compaction_exec.rs:183-213`), `/refine` (`crates/pa-core/src/session_engine/refine.rs:668`), résumé de branche (`crates/pa-core/src/session_engine/branch_summarization.rs:473`). Ils retrouvent le journal par l'id de session qu'ils passent déjà au provider. Sans journal, `usage_log::complete_simple` est exactement `pa_ai::complete_simple`.

Exemple (banc de capture, cas `s1-anthropic`, deuxième requête) : voir `usage-report.md` d'un run (`bench/tools/capture/usage_check.py`).
