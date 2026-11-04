# Ticket 08 : écriture du cache dans l'usage Responses, usage brut au journal

Ticket CDC : YA-13. Base : T04 (usage normalisé, journal d'usage), diagnostic du run `gpt-6.1-terra` du 04/11 (PR #14), décisions de Yusuf du 04/11/2026.

## Constat

- L'usage Responses d'OpenAI porte l'écriture du cache (guide prompt caching, https://developers.openai.com/api/docs/guides/prompt-caching, relevé le 04/11/2026) : `usage.input_tokens_details.cache_write_tokens`, ventilé en `cache_write_tokens_details.ttl_30m_tokens` et `ttl_12h_tokens` (les deux se somment à `cache_write_tokens`, renvoyés même à 0). `cached_tokens` exclut les tokens facturés en écriture. Input ordinaire = `input_tokens − cached_tokens − cache_write_tokens`.
- Le harness ne lit que `cached_tokens` et écrit `cache_write: 0` en dur (`crates/pa-ai/src/providers/openai_responses_stream.rs:770-800`). Une écriture est donc comptée en `input` (tarif 1,00 au lieu de 1,25 pour Terra), et `cacheWrite` vaut toujours 0 pour Responses, Azure et Codex (même processeur).
- Upstream ne le lit pas non plus côté Responses : sur `upstream/main` (`bba5bcf3f`, 04/11/2026), `cache_write_tokens` n'est lu que pour Chat Completions (OpenRouter, `crates/pa-ai/src/providers/openai_completions/convert.rs:417`) ; aucun commit upstream sur `openai_responses_stream.rs` depuis la base du fork (`c83ac68b3`). Rien à cherry-pick.
- Le banc ne garde pas la réponse brute : on ne sait pas encore si le backend Codex (`chatgpt.com/backend-api/codex`) renvoie ces champs.

## Décision du 04/11/2026, après la CI (fait foi sur la suite du ticket)

Premier essai : `cache_write_30m` / `cache_write_12h` en `Option<u64>` sur `Usage`. La CI (`ci.yml`, run 41899286040) est rouge sur `clippy::large_enum_variant` : l'écart entre `Assistant` et la variante suivante passe de 184 à 216 octets (seuil 200) dans `pa_types::ai::Message` (`crates/pa-types/src/ai/mod.rs:496`), `pa_types::session::AgentMessage` (`crates/pa-types/src/session.rs:141`) et l'enum de messages de `pa-agent` (`crates/pa-agent/src/types.rs:356`). Décision de Yusuf :

- `cache_write_30m` et `cache_write_12h` retirés de `Usage` et de son miroir, littéraux complets non touchés.
- Gardés : `cache_write` = `cache_write_tokens`, `input` = `input_tokens − cached_tokens − cache_write_tokens`.
- `cacheWrite30m` et `cacheWrite12h` dérivés dans le journal d'usage depuis `providerUsage` (hook `on_usage`), avec un signalement si du 12 h apparaît (`costWarning`).
- Coût du harness : toute écriture au tarif d'écriture du catalogue, documenté dans `docs/usage.md` ; le banc tarife toujours depuis `prices.json`.

Les sections ci-dessous gardent la version d'origine ; les points remplacés sont marqués.

## Mapping (validé)

| Champ Responses | Champ interne (`pa_types::ai::Usage`) |
|---|---|
| `input_tokens_details.cached_tokens` | `cache_read` (inchangé) |
| `input_tokens_details.cache_write_tokens` | `cache_write` (0 si absent, comme aujourd'hui) |
| `input_tokens − cached_tokens − cache_write_tokens` | `input` (saturé à 0) |
| `cache_write_tokens_details.ttl_30m_tokens` | ~~nouveau `cache_write_30m: Option<u64>`~~ remplacé : `cacheWrite30m` du journal, lu dans `providerUsage` |
| `cache_write_tokens_details.ttl_12h_tokens` | ~~nouveau `cache_write_12h: Option<u64>`~~ remplacé : `cacheWrite12h` du journal, lu dans `providerUsage`, avec `costWarning` si > 0 |
| `total_tokens` | `total_tokens` (inchangé) |

- `cache_write_1h` reste propre à Anthropic (reste à 5 min) : 30 min et 12 h ne sont ni 5 min ni 1 h, on n'y range rien.
- Journal d'usage : `cacheWrite30m` et `cacheWrite12h` seulement quand le provider donne la ventilation ; `cacheWrite5m` / `cacheWrite1h` restent réservés à Anthropic. Si la ventilation ne se somme pas à `cacheWrite`, les valeurs sont écrites telles que reçues, sans correction (l'usage brut journalisé le montre).
- (Remplacé : aucun champ sur `Usage`.) Même champs sur le miroir `Usage` de `pa-agent` (`crates/pa-agent/src/types.rs:149`), la frontière `pa-ai` / `pa-agent` passant par un aller-retour JSON.

## Usage brut au journal : option A (validée)

Hook `on_usage` générique sur `pa_ai::types::StreamOptions` (sur le modèle de `on_payload`, `crates/pa-ai/src/types.rs:61-79`) : il reçoit l'objet usage brut du provider (usage seul, aucun contenu de message) et le modèle. Pas propre à Responses : le type ne présume aucun format. Ce ticket le fait appeler par le processeur Responses (à `response.completed`, avec `response.usage`) ; les autres providers pourront l'appeler ensuite.

- Transport : `ResponsesStreamHooks` (`openai_responses_hooks.rs:32`) ; miroir sur les options de `pa-agent` et passage par `provider_adapter.rs` (comme `on_payload` / `on_response`, `provider_adapter.rs:167-168`).
- La sonde du journal (`crates/pa-core/src/session_engine/usage_log.rs`, boucle et `AuxProbe`) branche le hook comme elle branche `on_payload`, garde le dernier usage brut de l'appel, et l'écrit dans `<id>.usage.ndjson` sous `providerUsage`.
- Rien dans les fichiers de session ni dans la requête : aucun effet attendu sur le préfixe.
- Écartées : un champ sur `AssistantMessage` (persisté dans les sessions, circule dans l'historique) ; une entrée `diagnostics` (réservée aux échecs et reprises, lue par la logique de nouvelle tentative et de bascule).

## Conditions de Yusuf

### 1. `cacheRetention: long` et TTL 12 h

Vérifier et documenter dans `docs/usage.md` ce que `cacheRetention: long` envoie au backend Codex et à Responses. Relevé au 04/11 :

- Responses (`openai`) : `prompt_cache_retention: "24h"` si le compat le permet (défaut oui) (`crates/pa-ai/src/providers/openai_responses.rs:71-80`, `:216-218`). Le guide ne dit pas ce que fait ce paramètre sur GPT-5.6 et suivants (il recommande de le remplacer par `prompt_cache_options.ttl`) : effet non documenté, à signaler.
- Codex : aucun paramètre de rétention dans le corps (`crates/pa-ai/src/providers/openai_codex_responses/mod.rs`, seul `prompt_cache_key`, `:653`). Azure : aucun (`azure_openai_responses.rs:196-197`).
- Le guide : 12 h seulement par `prompt_cache_options.ttl: "12h"` ou `prompt_cache_breakpoint.ttl` ; sans eux, 30 min. Le harness n'envoie ni l'un ni l'autre : aucune écriture à 12 h attendue.

Si un TTL 12 h reste possible (ventilation `ttl_12h_tokens > 0` renvoyée malgré tout), le dire dans `docs/usage.md` et le tarifer correctement côté harness (`calculate_cost_values`, `crates/pa-ai/src/models.rs:24`, n'a qu'un tarif d'écriture, celui de 30 min au catalogue : Luna 0,125, Terra 1,25), ou le signaler explicitement. Proposé : tokens 12 h au double du tarif d'écriture du catalogue (le guide : écriture 30 min à 1,25 × l'input, 12 h à 2,5 ×), seulement quand `cache_write_12h` est renseigné. (Remplacé : toute écriture au tarif d'écriture du catalogue, `costWarning` au journal.)

### 2. Sérialisation rétrocompatible

(Sans objet après la décision du 04/11 : `Usage` n'a pas de nouveau champ.)

`cache_write_30m` et `cache_write_12h` en `#[serde(default, skip_serializing_if = "Option::is_none")]` (renommés `cacheWrite30m`, `cacheWrite12h`), sur `pa_types::ai::Usage` et sur le miroir de `pa-agent`. Test : une session écrite avant le changement se relit sans erreur, et un usage sans ventilation s'écrit avec les mêmes champs qu'avant (extension de `crates/pa-core/tests/usage_backcompat.rs`).

### 3. Fichiers upstream

Chaque fichier upstream touché est listé dans `docs/fork-changes.md` (fichier, raison, ticket), y compris les littéraux complets de `Usage` complétés de `cache_write_30m: None, cache_write_12h: None` (même méthode que T04, `docs/fork-changes.md`, section « Usage (T04) »).

## Tests prévus

- Fixtures Responses (`crates/pa-ai/src/providers/usage_tests.rs`) : écriture avec ventilation 30 min / 12 h ; écriture sans ventilation ; aucun champ d'écriture (comportement d'aujourd'hui inchangé) ; input ordinaire = `input_tokens − cached_tokens − cache_write_tokens`.
- Coût du harness : 30 min au tarif du catalogue, 12 h selon la condition 1 (remplacé : toute écriture au tarif du catalogue).
- Journal (`usage_log/tests.rs`) : `providerUsage` présent et égal à l'usage brut ; `cacheWrite30m` / `cacheWrite12h` présents seulement avec la ventilation ; aucun champ nouveau sans hook appelé.
- Relecture d'une ancienne session (condition 2).
- Provider factice : aucun appel réel en test ni en CI (CLAUDE.md, règle 6).
- Banc de capture : le serveur de capture renvoie `cache_write_tokens` et sa ventilation pour Responses et Codex ; vérification de bout en bout (usage normalisé, `providerUsage`) ; comparaison des corps de requête entre `main` et `ya-08` pour prouver l'absence d'impact sur le préfixe (aucun attendu).
- Banc smoke : `metrics.request_cost` tarife `cacheWrite30m` et `cacheWrite12h` séparément (`prices.json` : `cache_write_30m`, `cache_write_12h`) ; sans ventilation, comme aujourd'hui (tarif 30 min).
- Tarifs : `prices.json` porte déjà les tarifs d'écriture de Terra, relevés le 04/11/2026 sur la page du modèle (court 1,25 / 2,50, long 2,50 / 5,00), confirmés par la page des tarifs ; mise à jour seulement si une valeur diffère.

Rust : pas de build local (CLAUDE.md, règle 8) ; compilation et tests par `ci.yml` sur la PR, binaire par `ya-build-macos-arm64.yml`.

## Run réel (autorisé par Yusuf, 04/11/2026)

Une fois le binaire `ya-08` construit en CI :

```bash
DO_NOT_TRACK=1 PI_OFFLINE=1 make smoke BINARY=<dossier>/prime-agent MODEL=gpt-6-luna TASKS=py-example-001
```

Montrer à Yusuf l'usage brut (`providerUsage`) renvoyé par le backend Codex, appel par appel.

## Attendu en sortie

- PR `ya-08` vers `main`, sans merge sans le feu vert de Yusuf, `ci.yml` vert.
- `cacheWrite` et `input` corrects pour Responses quand le provider donne l'écriture ; ventilation 30 min / 12 h au journal ; `providerUsage` dans chaque ligne du journal des appels Responses.
- `docs/usage.md` : ligne Responses mise à jour (champs, normalisation, `cacheRetention: long`, TTL 12 h) ; format du journal (`cacheWrite30m`, `cacheWrite12h`, `providerUsage`).
- `docs/fork-changes.md` : chaque fichier upstream touché.
- Rapport du run luna : usage brut de chaque appel, présence ou absence de `cache_write_tokens` et de sa ventilation dans la réponse du backend Codex, coût recalculé.
- Impact préfixe : aucun attendu, vérifié au banc de capture et signalé dans le résumé.
