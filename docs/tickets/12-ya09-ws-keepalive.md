# Ticket 12 : YA-09, WebSocket Codex fermé en 1011 pendant un silence

Suite du lot Unharnessed (ticket 11, point ouvert « WebSocket fermé en 1011 »). Plan écrit en lecture seule le 16/11/2026 et validé par Yusuf le même jour. Base : `main` à `ded459d13`.

## Constat

Le serveur ferme le WebSocket en `1011 keepalive ping timeout` après 73,9 à 79 s sans requête. Trois cas sont connus : A/B du 15/11 (77,2 s, `docs/tickets/09-ya03-base.md:189-195`), chauffe `8755b2e` (73,9 s) et chauffe de la PR A (79,2 s, pendant un `npm test` de 86 s, `docs/tickets/11-ya09-unharnessed.md:23`). Un silence de 47 s est passé sans fermeture. Après la fermeture, la session reste en SSE avec des corps complets jusqu'à la fin.

## Recherche upstream (`git fetch upstream`, 16/11/2026)

- `upstream/main` à `ad1613303` : 86 commits depuis notre base `c83ac68b3`, aucun sur `websocket.rs` (dernier : `e2229869b`, antérieur à la base).
- `ping|pong|keepalive` dans `websocket.rs` de toutes les branches `upstream/*` : seules des branches de septembre (portage Rust, `f492609c6`) en parlent, et elles ignorent les pings comme notre base. Il n'y a pas de correctif à reprendre.
- Commit voisin repris (décision 2) : `c4800c8eb` (#4010), libère la connexion en cache quand `send_request` échoue (abandon juste avant l'envoi). Sans lui, la connexion restait occupée pour toujours et chaque tour suivant repartait en corps complet.

## Cause (lecture du code, confirmée par les tests)

- Entre deux requêtes, le worker attendait la commande suivante sans lire le flux (`websocket.rs:233` sur la base). Le flux n'était lu que dans `read_request_events`.
- tungstenite 0.28 met un pong en file à la lecture d'un ping (`tungstenite-0.28.0/src/protocol/mod.rs:668-674`) et l'envoie à la lecture ou l'écriture suivante (`:457-466`). Sans lecture pendant un silence, aucun pong ne part.
- La trame `Close(1011)` attendait dans le tampon. La requête suivante la lisait avant tout événement, `mod.rs` classait l'échec en panne de transport et `record_websocket_failure` fixait la session en SSE (`session.rs:64-76`). Ce repli n'est levé que par `reset_debug_stats`, que personne n'appelle hors des tests (`lib.rs:34`).
- Il n'y a aucun délai d'inactivité côté client dans `read_request_events` (seul le signal d'abandon est mis en concurrence avec la lecture).

## Décisions de Yusuf (16/11/2026)

1. Correctif (a) seul : le worker lit le flux pendant le silence, et une connexion fermée pendant le silence est remplacée par un WebSocket neuf. La relance sur WebSocket neuf dans la même requête (b) ne sera faite que si une chauffe montre encore une bascule. Le retour au WebSocket après N requêtes (c) est écarté.
2. Cherry-pick de `c4800c8eb` avec `-x`, dans un commit à part, dans la même PR.
3. Tâche de chauffe versionnée, hors des tâches tirées par défaut, réutilisable pour le ticket de l'attente complète de `bash()`.
4. Les messages texte ou binaires reçus hors requête sont comptés dans les diagnostics, pas jetés sans trace.
5. Note dans `docs/fork-changes.md` : bug upstream, candidat à une PR upstream après les testeurs.

## Correctif

- `crates/pa-ai/src/providers/openai_codex_responses/ya_idle.rs` (nouveau) :
  - classement d'une lecture faite pendant le silence (`on_idle_read`) : ping (pong envoyé), pong ou trame brute (on continue), texte ou binaire (compté), fermeture ou erreur (le worker s'arrête) ;
  - compteurs par connexion et diagnostic `websocket_idle_messages_ignored` (`textMessages`, `binaryMessages`, `lastType`), posé sur la requête suivante de la même connexion ;
  - retrait de l'entrée de cache dont le worker est arrêté (`evict_closed_connection`).
- `websocket.rs` : le worker attend la commande suivante et lit le flux en même temps (`tokio::select!` avec priorité aux commandes). Après un ping, il vide la file d'envoi (`sink.flush()`). `acquire_websocket` retire d'abord une connexion morte.
- `mod.rs` : déclaration du module, diagnostic posé après `acquire_websocket`, déclaration du module de tests.

Un message de données reçu hors requête n'appartient à aucune requête : sur la base, la requête suivante le lisait comme un de ses propres événements (un `response.done` égaré la terminait à vide).

**Requêtes et préfixe** : corps, en-têtes, prompt système et tools inchangés. Seules des trames de contrôle s'ajoutent (pongs). Effet attendu : moins de requêtes en corps complet. Après une fermeture pendant un silence, la requête suivante part sur un WebSocket neuf en corps complet, alors qu'elle partait en SSE en corps complet.

**Risques** : une socket inactive vit jusqu'au TTL de 5 min (`websocket.rs:30`) au lieu de mourir vers 75 s (une par session). Si le serveur ferme entre la vérification de `acquire_websocket` et l'envoi, le comportement reste celui de la base (repli SSE).

## Tests

- `ya_idle.rs` (unitaires) : classement des trames, comptage, diagnostic pris une seule fois.
- `openai_codex_responses/tests/ya_keepalive.rs` (serveur factice WebSocket et SSE sur un même port) :
  - `parked_connection_answers_pings` : 3 pings pendant le silence, 3 pongs aux mêmes données avant toute requête, puis le tour suivant continue la chaîne sur la même connexion (`previous_response_id`) ;
  - `close_while_parked_reconnects_over_websocket` : `Close(1011)` pendant le silence, puis le tour suivant part sur un WebSocket neuf en corps complet, sans échec de transport ni repli SSE ;
  - `data_frame_while_parked_is_counted_not_delivered` : un `response.done` égaré pendant le silence ne termine pas la requête suivante et apparaît dans son diagnostic ;
  - `close_during_a_request_still_falls_back_to_sse` (non-régression) : `Close(1011)` après `response.create` et avant tout événement, puis repli SSE avec le diagnostic `provider_transport_failure` (`eventsEmitted: false`), session fixée en SSE, tour suivant en SSE en corps complet.
- Banc : `TaskSelection.test_warmup_tasks_only_on_request` (`bench/smoke/tests/test_bench.py`).
- Pas de `cargo test` en local (règle 8) : la validation se fait par `ci.yml`.

## Tâche de chauffe

`bench/smoke/tasks/py-silence-001` (tag `chauffe`) : `bash()` lance `sleep 180 && echo fini > attente.txt` et l'agent attend la fin. Elle n'est jamais tirée par défaut (`run.py`, `tools/ab.py`) et se lance par `TASKS=py-silence-001`. Première version à `sleep 130` : avec `bashWaitSeconds` à 30, la cellule rendait la main à 30 s et le silence restait sous 120 s (chauffe 1). Passage à 180 s le 16/11/2026, accord de Yusuf.

## Vérification réelle (autorisée d'avance, pas une mesure)

Critères : tous les appels de boucle en WebSocket, 0 bascule SSE, 0 `1011` dans les journaux, un silence d'au moins 120 s entre deux appels prouvé par `sincePrevMs`, et l'appel qui suit le silence en suite de conversation (`previous_response_id`).

Binaire de `cce48e6d5` (build `ya-build-macos-arm64` 43850337252, sha256 de l'exécutable `f0b99ce94b9508a82082f3a3e699480e069a1c04a3771408d4b1efb037c20f52`), `gpt-6-luna`, effort high, `default.json`, `--log-bodies`. Runs non versionnés.

Commandes :

```bash
python3 -I bench/smoke/run.py --binary <dossier>/prime-agent --provider openai-codex --model gpt-6-luna --effort high --settings bench/smoke/settings/default.json --tasks py-silence-001 --log-bodies
python3 -I bench/smoke/tools/check_warmup.py <run>/tasks/py-silence-001/run-1 <run>/report.json
python3 -I bench/smoke/metrics.py --since-prev <run>
python3 -I bench/smoke/tools/compare_bodies.py bench/smoke/runs/20261116T091720Z-f81f980-gpt-6-luna/tasks/py-shortuuid-118/run-1/proxy-bodies.ndjson <run>/tasks/py-silence-001/run-1/proxy-bodies.ndjson
```

**Avant les chauffes**, run `--capture` sans modèle (`20261116T105819Z-cce48e6-gpt-6-luna`, tâche à `sleep 130`) : préparation et état cassé validés, 4 appels en WebSocket avec suite de conversation, 0 anomalie, 0 secret trouvé. Le seul CONNECT refusé (`example.com`) vient de la sonde de sandbox du serveur de capture (`bench/tools/capture/run.py:271`).

| | Chauffe 1 (`sleep 130`) | Chauffe 2 (`sleep 180`) |
|---|---|---|
| Run | `20261116T105840Z-cce48e6-gpt-6-luna` | `20261116T110353Z-cce48e6-gpt-6-luna` |
| Statut | résolue | résolue |
| Appels de boucle, transport | 4, tous en WebSocket, une seule connexion (1 `ws_open`, 1 `ws_close` en fin de session) | 3, tous en WebSocket, une seule connexion |
| Bascule SSE, `provider_transport_failure` | 0, 0 | 0, 0 |
| Mentions de `1011` (`out.jsonl`, `err.log`) | 0 | 0 |
| `sincePrevMs` maximal | 97 685 ms (seq 4) : **critère non atteint** | **180 050 ms** (seq 3) : critère atteint |
| Plus long silence vu au proxy (fin de réponse → requête suivante) | 95,0 s | 178,8 s |
| Appel après le silence | seq 4, `previous_response_id`, `continuation: True` | seq 3, `previous_response_id`, `continuation: True` |
| Diagnostic `websocket_idle_messages_ignored` | 0 | 0 |

Les fermetures observées avant le correctif tombaient entre 73,9 et 79,2 s de silence. Les deux chauffes dépassent ce seuil sur la même connexion, la seconde de plus de 100 s.

**Miss serveur** : `cacheRead = 0` sur des appels en suite de conversation, sur la même connexion WebSocket, sans invalidation journalisée (classés « miss serveur » par le rapport) :
- chauffe 1 : appels 2 (`sincePrevMs` 34 010, prompt 1 774) et 4 (97 685, prompt 2 014). L'appel 3 lit 1 664 tokens en cache ; taux de cache de la session 0,231 ;
- chauffe 2 : appels 2 (3 062, prompt 1 657) et 3 (180 050, prompt 1 719) ; aucun appel ne lit du cache, taux de session 0,000.

Ces miss ne dépendent pas du transport : la connexion et la chaîne `previous_response_id` sont intactes. Ils restent sans explication ici, sur des prompts courts (moins de 2 100 tokens).

**Changements de requête** (`compare_bodies.py`, premier corps des deux chauffes contre celui de `py-shortuuid-118` de la chauffe `f81f980f2` de la PR B ; entre `f81f980f2` et `main`, seul `crates/pa-cli/tests/ya_prompt_v2_e2e.rs` change dans `crates/`) : `instructions` (5 434 octets), `tools`, `model`, `reasoning`, `include`, `text`, `store`, `stream`, `tool_choice`, `parallel_tool_calls` et `type` identiques octet pour octet ; seuls `input` (énoncé de la tâche) et `prompt_cache_key` (identifiant de session) diffèrent. Mêmes jeux de clés : premier corps sans `previous_response_id`, suivants avec. Relu et mergé par Yusuf le 16/11 (PR #45).

## Note pour le ticket suivant (attente complète de `bash()`)

Le client garde une connexion inactive au plus 5 min (`SESSION_WEBSOCKET_CACHE_TTL_MS`, `crates/pa-ai/src/providers/openai_codex_responses/websocket.rs:30` après ce ticket, `:29` avant). Une commande de plus de 5 min sans appel au modèle dépassera ce TTL : la connexion sera fermée par le client, et l'appel suivant partira sur un WebSocket neuf en corps complet, sans suite de conversation (pas de repli SSE depuis ce ticket). À traiter si ce ticket rend l'attente complète : allonger le TTL, ou accepter et mesurer ce corps complet. Garde-fou d'inactivité du banc (300 s) : le reverse proxy compte les octets relayés dans les deux sens, trames de contrôle comprises (`bench/smoke/proxies.py:451` et `:465`). Les pings du serveur et, depuis ce ticket, les pongs du client le remettent donc à zéro tant que la connexion vit. Après la fermeture par le TTL client, plus aucun octet ne passe : une commande de plus de 10 min (5 min de TTL, puis 300 s de garde-fou) serait arrêtée en `stalled` (estimation par lecture du code, non vérifiée). Il faudra relever `--idle-timeout` pour une telle tâche.
