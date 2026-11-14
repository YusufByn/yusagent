# Passation du 14/11/2026

Note écrite après le merge de la PR #28 (`ya-03-suite` → `main`, merge `0d5710f13`), pour une nouvelle session. Ticket : `docs/tickets/09-ya03-base.md`. Aucun build ni chauffe lancés depuis ce merge. L'A/B 1 n'est pas lancé ; il ne l'est jamais par Claude.

## État de `main` (`0d5710f13`)

- PR #26 (YA-03, PR 2, merge `b73efb63e`) : tools natifs de la famille Codex (`apply_patch` libre, `exec_command` / `write_stdin` en variante), réglages `editTool` / `shellTool` figés dans l'en-tête de session (`yaTools`, démon et forks compris), sortie d'`apply_patch` identique à Codex. Détail : `docs/fork-changes.md` (« Tools natifs (YA-03, PR 2) »), passation du 13/11 (PR 2).
- PR #28 (merge `0d5710f13`, `ci.yml` vert sur `0096f6c34`, run 43528162796) : rejeu des appels libres.
- Binaire installé le plus récent : `~/.cache/yusagent-bin/b73efb63ed31efbfb25118ada1bf7a19f16c4c48` (merge de #26, **sans** la PR #28 : ne pas l'utiliser pour l'A/B). Aucun binaire de `0d5710f13`.
- `bench/smoke/ab/ya03-plan.md` : SHA du binaire pas encore noté.

## Ce que corrige la PR #28

Chauffe du 14/11 (`bench/smoke/runs/20261113T230515Z-b73efb6-gpt-6.1-terra`, `py-shortuuid-118`, défaut, `gpt-6.1-terra` effort high, résolue) : le backend de l'abonnement accepte le tool libre, `apply_patch` est appelé et appliqué au premier essai, mais le tour qui suit `apply_patch` part sans `previous_response_id` (requête 5 de `tasks/py-shortuuid-118/run-1/proxy.ndjson`, `continuation: false`, 55 018 octets).

- **Cause** : la marque d'appel libre `yaFreeformItemId` (`ToolCall.rest`, PR 1) se perdait dans la boucle de l'agent, dont le `ToolCall` n'a pas de `rest` (`crates/pa-agent/src/types.rs:119-131`) ; l'appel était rejoué en `function_call` / `function_call_output`, et la base de la suite WebSocket (sortie du provider, en `custom_tool_call`) ne correspondait plus.
- **Correction** (`crates/pa-ai/src/providers/ya_freeform.rs`, `FreeformReplay`) : un appel est libre quand le tool qu'il nomme est déclaré libre dans les tools de la requête (ceux de la session, figés par session). L'id d'item vient de l'id de l'appel (`<call_id>|<item_id>`), que la conversion ne normalise pas pour un appel du même modèle (`transform_messages.rs:181-193`). La base de la suite WebSocket reçoit les tools libres tirés du corps de la requête (`freeform_tools_of_request`, `openai_codex_responses/mod.rs:505`). `ToolCall` de `pa-agent` inchangé, aucun préfixe d'id utilisé.
- **Test** : `crates/pa-core/tests/prefix_stability.rs`, `codex_websocket_apply_patch_replays_as_custom_and_continues` (serveur WebSocket scripté, `openai-codex`, vrai `apply_patch` par la boucle de l'agent ; tour suivant en suite de conversation avec le seul `custom_tool_call_output` ; après reprise, rejeu en `custom_tool_call` avec son id d'item). Préfixe inchangé (pas de nouvelle capture : le banc de capture ne produit aucun appel libre).

## Défaut upstream : requête 2 sans suite de conversation

Dans la même chauffe, la requête 2 (après le premier `python_repl`) part aussi sans `previous_response_id`.

- **Cause** : au `response.output_item.done`, `crates/pa-ai/src/providers/openai_responses_stream.rs` retire le slot de l'item (`self.slots.remove(&index)`, `:540-542`) avant de le relire (`current_slot(output_index)`, `:548-550`). `content_index` vaut toujours `None` quand l'événement porte `output_index` (cas réel) : ni `text_signature` (`:634-650`) ni `thinking_signature` (`:592-596`) ne sont posées.
- **Effet** : un texte assistant rejoue son id en `msg_<index>` (`openai_responses_shared.rs:226-238`). La base de la suite convertit le message seul (index 0, `openai_codex_responses/mod.rs:499-511`), la requête complète à son index réel (1 pour le premier message) : la comparaison de préfixe échoue (`websocket.rs:547-579`) et le tour repart en corps complet. Le premier message de la chauffe avait un texte de 130 caractères avant son `python_repl`. Les items de raisonnement ne sont jamais rejoués : aucune session ne porte de `textSignature` ni de `thinkingSignature` (chauffe du 14/11, run `0fa8fad` du 06/11, A/B du 12/11).
- **Origine** : code upstream de notre base de fork (`c83ac68b3`), corrigé upstream par `c12acadaa` (08/11/2026, « Preserve Responses reasoning when finalizing streamed items », #3976), complété par `42a07c13c` (09/11/2026, « keep accumulated function-call arguments on a done item with empty arguments », #4014). Ni l'un ni l'autre n'est dans `main`. Noté aux points ouverts du ticket 03 (YA-04).
- Le test de suite de `pa-ai` contourne le défaut (`done` sans `output_index`, commentaire de `openai_codex_responses/mod.rs:1165-1167`). Le scénario WebSocket de `prefix_stability` envoie des `done` avec `output_index` et accepte deux formes pour la première requête après reprise (corps complet aujourd'hui, suite de conversation après le cherry-pick).

## Suite, dans cet ordre

a. **Cherry-pick** de `c12acadaa` puis `42a07c13c` sur une branche `ya-*` depuis `main` à jour (`git cherry-pick -x`), avec un test qui vérifie, sur des `done` portant `output_index` : la signature de texte posée, le rejeu de l'item de raisonnement, et la suite de conversation (`previous_response_id`) au 2e tour d'un tour qui commence par un texte. Les deux commits ne touchent que `openai_responses_stream.rs` et `.changes/` ; notre fichier diffère d'upstream (YA-03, YA-13 : tools libres, usage), prévoir des conflits à résoudre à la main et les lister dans `docs/fork-changes.md`. **Capture refaite** (le rejeu des signatures et du raisonnement change les requêtes à partir du 2e tour), comparée à `main`. `ci.yml` vert du dernier commit, puis « merge » de Yusuf.

b. **Après ce merge** : build de `main` lancé à la main (`gh workflow run ya-build-macos-arm64.yml --repo YusufByn/yusagent --ref main` : le workflow ne se déclenche pas sur un push vers `main`), relance unique si GitHub l'annule faute de runner ; `bash bench/smoke/tools/install-bin.sh <run> <sha du merge>` (sommes, commit du manifeste, sha256 de l'exécutable, `--version`). Chauffe d'une tâche, réglages par défaut (`editTool` native, `shellTool` kernel, `gpt-6.1-terra`, effort high) :
   ```
   DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short caffeinate -i python3 -I bench/smoke/run.py --binary <dossier>/prime-agent --model gpt-6.1-terra --effort high --settings bench/smoke/settings/default.json --tasks py-shortuuid-118 --runs 1 --idle-timeout 300
   ```
   Vérifier dans `tasks/<tâche>/run-1/proxy.ndjson` (`event: request`, champ `continuation`) que le tour qui suit un `python_repl` et celui qui suit un `apply_patch` partent en suite de conversation (seule la première requête de la session part en corps complet), et dans la session JSONL que l'appel `apply_patch` est rejoué en `custom_tool_call` (réponse du backend sans erreur) et que les textes portent `textSignature`. Au moindre échec : arrêt et compte rendu court.

c. **Si la chauffe est bonne** : SHA complet du binaire (commit du manifeste et sha256 de l'exécutable) noté dans `bench/smoke/ab/ya03-plan.md` par une PR de docs seule (merge autorisé sur `ci.yml` vert du dernier commit), puis commande de l'A/B 1 donnée à Yusuf, prête à coller, lancée par lui :
   ```
   DO_NOT_TRACK=1 PI_OFFLINE=1 caffeinate -i python3 -I bench/smoke/tools/ab.py ~/.cache/yusagent-bin/<sha> <sha> --a bench/smoke/settings/default.json --b bench/smoke/settings/edit-kernel.json --passes 3 --seed 2290879124 --model gpt-6.1-terra --effort high --idle-timeout 300
   ```
   (graine de l'A/B 1 fixée dans `ya03-plan.md` ; `ab.py` fixe `PI_CACHE_RETENTION=short` ; vérifier avant `--dry-run`.)

## Pièges rencontrés

- **Contexte** : la session du 13-14/11 a dépassé 550k tokens ; repartir d'une session neuve à chaque étape (a, b, c).
- **`ya-build-macos-arm64.yml` sur `main`** : pas de déclenchement sur push vers `main` (seulement `ya-*` et `workflow_dispatch`).
- **Chauffe** : `bench/smoke/run.py --capture` (sans modèle) a été refusé par le classifieur de permissions dans cette session ; les vérifications de suite se lisent dans `proxy.ndjson` d'un run réel.
- **Clippy** : `explicit_auto_deref` refuse `&mut *guard` quand la coercion suffit ; `redundant_clone` sur le dernier usage d'un champ (déplacer, si le type n'a pas de `Drop`).
- **zsh** : `"$c:chemin"` applique le modificateur `:c` de zsh ; écrire `"${c}:chemin"`.
