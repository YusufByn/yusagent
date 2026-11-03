# Usage des appels modèle (T04, YA-13)

Ce document fixe la sémantique des champs d'usage après normalisation par la couche providers, et le format du journal d'usage par appel.

## Champs de `Usage`

`Usage` (`crates/pa-types/src/ai/mod.rs:423`, miroir côté boucle `crates/pa-agent/src/types.rs:149`) :

| Champ (JSON) | Sens après normalisation |
|---|---|
| `input` | Tokens d'entrée **hors cache** : ni lus ni écrits en cache. |
| `cacheRead` | Tokens d'entrée lus dans le cache. |
| `cacheWrite` | Tokens d'entrée écrits dans le cache par cette requête. |
| `cacheWrite1h` | Part de `cacheWrite` écrite avec le TTL d'une heure, quand le provider donne la ventilation. Absent sinon. Le reste (`cacheWrite - cacheWrite1h`) est à 5 min. |
| `output` | Tokens de sortie, raisonnement compris. |
| `reasoning` | Tokens de raisonnement, **sous-ensemble de `output`**. Absent quand le provider ne les expose pas, jamais 0 par défaut. |
| `totalTokens` | Total tel que calculé ou rapporté par l'adaptateur (inchangé par T04). |
| `cost` | Coût calculé depuis les tarifs du catalogue (inchangé par T04). |

Les deux champs optionnels sont ajoutés par T04 (`crates/pa-types/src/ai/mod.rs:436`, `:444` ; `crates/pa-agent/src/types.rs:164`, `:171`). Ils ne sont pas écrits quand ils valent `None` : une session écrite avant T04 se relit sans erreur, et une session sans ces valeurs s'écrit avec exactement les mêmes champs qu'avant (`crates/pa-core/tests/usage_backcompat.rs`).

Conséquence de la normalisation : pour tous les providers, `input + cacheRead + cacheWrite` est la taille totale du prompt de la requête.

## Normalisation par provider

| Provider (API) | Ce que renvoie l'API | `input` | `cacheRead` | `cacheWrite` | `reasoning` | Où |
|---|---|---|---|---|---|---|
| Anthropic Messages | `input_tokens` = tokens après le dernier breakpoint ; total = `cache_read_input_tokens + cache_creation_input_tokens + input_tokens` (doc prompt caching) | `input_tokens` | `cache_read_input_tokens` | `cache_creation_input_tokens` ; `cacheWrite1h` = `cache_creation.ephemeral_1h_input_tokens` | `output_tokens_details.thinking_tokens` (message_delta final) | `crates/pa-ai/src/providers/anthropic/stream.rs:297-309` (message_start), `:596-608` (message_delta), `:711` |
| OpenAI Responses (et Azure, Codex, même processeur) | `input_tokens` inclut `input_tokens_details.cached_tokens` ; `reasoning_tokens` compté dans `output_tokens` | `input_tokens - cached_tokens` | `cached_tokens` | 0 | `output_tokens_details.reasoning_tokens` | `crates/pa-ai/src/providers/openai_responses_stream.rs:770-800` |
| Chat Completions (OpenAI, OpenRouter, DeepSeek, Kimi…) | `prompt_tokens` inclut le cache (`prompt_tokens_details.cached_tokens` ou `prompt_cache_hit_tokens`), écriture dans `cache_write_tokens` (OpenRouter) | `prompt_tokens - cacheRead - cacheWrite` | `cached_tokens` (moins `cache_write_tokens` si présent, observé sur OpenRouter) | `cache_write_tokens` | `completion_tokens_details.reasoning_tokens` | `crates/pa-ai/src/providers/openai_completions/convert.rs:404-450` |
| Gemini (Google, Vertex) | `promptTokenCount` inclut `cachedContentTokenCount` ; `thoughtsTokenCount` est un terme à part du total | `promptTokenCount - cachedContentTokenCount` | `cachedContentTokenCount` | 0 | `thoughtsTokenCount` (déjà ajouté à `output`) | `crates/pa-ai/src/providers/google_stream.rs:254-277` |
| Mistral | `prompt_tokens`, `completion_tokens` | `prompt_tokens` | 0 | 0 | absent | `crates/pa-ai/src/providers/mistral/stream.rs:511-532` |
| Bedrock Converse | `inputTokens`, `cacheReadInputTokens`, `cacheWriteInputTokens` | `inputTokens` | `cacheReadInputTokens` | `cacheWriteInputTokens` | absent | `crates/pa-ai/src/providers/bedrock/events.rs:402-430` |

Sources vérifiées (03/11/2026) :

- Anthropic : <https://platform.claude.com/docs/en/build-with-claude/streaming> (usage de `message_start` sous `message.usage`, comptes du `message_delta` cumulés, exemple de `message_delta` avec `output_tokens` seul), <https://platform.claude.com/docs/en/build-with-claude/prompt-caching> (sémantique de `input_tokens`, formule du total, objet `cache_creation`), <https://platform.claude.com/docs/en/build-with-claude/extended-thinking> (`output_tokens_details.thinking_tokens`, sur le `message_delta` final).
- OpenAI : <https://developers.openai.com/api/docs/guides/prompt-caching> (taux de cache = cached / input, donc cached inclus dans input), <https://developers.openai.com/api/docs/guides/reasoning> (raisonnement facturé en sortie, `output_tokens_details.reasoning_tokens`), types du SDK officiel `openai-python` (`response_usage.py`, `completion_usage.py`).
- Gemini : <https://aistudio.google.com/api-reference/generate-content> (`UsageMetadata`).
- DeepSeek : <https://api-docs.deepseek.com/api/create-chat-completion> (`prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens`).
- OpenRouter : <https://openrouter.ai/docs/use-cases/usage-accounting> (`cached_tokens`, `cache_write_tokens`).

Tests sur fixtures : `crates/pa-ai/src/providers/anthropic/usage_tests.rs`, `crates/pa-ai/src/providers/usage_tests.rs` (Responses, Gemini), `crates/pa-ai/src/providers/openai_completions/usage_tests.rs` (DeepSeek, OpenRouter). L'absence pour Mistral est couverte par le test upstream `usage_total_tokens_explicit_zero_falls_back_to_sum` (`crates/pa-ai/src/providers/mistral/stream.rs:847`), qui compare l'usage entier.

### Correctif Anthropic

Avant T04, l'usage du `message_start` était lu sur `message.*` au lieu de `message.usage.*` : seul le `message_delta` remplissait l'usage. Avec un `message_delta` qui ne porte que `output_tokens` (cas des exemples de la doc), `input`, `cacheRead` et `cacheWrite` restaient à 0. Le `message_start` est maintenant lu sous `message.usage` (`stream.rs:297`) ; le `message_delta` ne remplace que les champs qu'il porte, sans addition (`stream.rs:596-608`), puisque ses comptes sont cumulés.

Effet de bord à noter : `totalTokens` est désormais juste dans ce cas, et c'est lui qui sert d'estimation de la taille du contexte pour le seuil de compaction (`crates/pa-core/src/session_engine/compaction.rs:50-55`). Avec un flux réel dont le `message_delta` ne porte que `output_tokens`, la compaction par seuil se déclenche désormais quand elle le doit ; avant, elle voyait 8 tokens.

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
| `cacheWrite5m`, `cacheWrite1h` | Ventilation de `cacheWrite` par TTL, seulement quand le provider la donne. |
| `reasoning` | Tokens de raisonnement, seulement quand le provider les donne. |
| `totalTokens`, `cost` | Total et coût calculé (`cost.total`). |
| `cacheRate` | Taux de cache de la requête, `null` sans prompt. |
| `ttftMs` | Du début de l'appel au premier événement de contenu (texte, raisonnement ou tool call), `null` si rien n'a été streamé. |
| `durationMs` | Du début de l'appel à sa fin. |
| `sincePrevMs` | Délai depuis le début de l'appel précédent **du même `kind`** dans la session (`null` pour le premier), relu dans le journal après un redémarrage. Le cache se rafraîchit à chaque lecture, donc ce délai, comparé au TTL (5 min ou 1 h), dit si le préfixe de la boucle était encore chaud. Les appels annexes ont leur propre préfixe et leur propre délai. |
| `historyReasoning` | Raisonnement renvoyé au modèle dans l'historique de cette requête, mesuré sur le corps exact envoyé : `blocks` (blocs ou items de raisonnement), `textChars` (caractères de texte lisible), `textTokensEst` (**estimation** : caractères / 4, l'heuristique de `compaction::estimate_tokens`), `opaqueBytes` (signatures, raisonnement chiffré ou masqué, en octets). Aucun provider ne donne ce compte en tokens. Absent si le corps n'a pas été observé (échec avant l'envoi). |

Points d'accroche :

- Appels de la boucle (et de leurs retries) : `instrument_stream_fn` enveloppe le `StreamFn` de la session (`crates/pa-core/src/session_engine/engine.rs:704-714`), la question annexe a son propre enveloppement (`purpose: side_question`). Le corps est observé par le hook `on_payload`, qui le rend inchangé.
- Appels annexes (`pa_ai` direct) : résumé de compaction (`crates/pa-core/src/session_engine/compaction_exec.rs:183-213`), `/refine` (`crates/pa-core/src/session_engine/refine.rs:668`), résumé de branche (`crates/pa-core/src/session_engine/branch_summarization.rs:473`). Ils retrouvent le journal par l'id de session qu'ils passent déjà au provider. Sans journal, `usage_log::complete_simple` est exactement `pa_ai::complete_simple`.

Exemple (banc de capture, cas `s1-anthropic`, deuxième requête) : voir `usage-report.md` d'un run (`bench/tools/capture/usage_check.py`).
