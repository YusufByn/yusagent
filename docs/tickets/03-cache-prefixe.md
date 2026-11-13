# Ticket 03 : cache de prompt, test de préfixe et prompt système stable entre sessions

Ticket CDC : YA-04 (et la partie instantané de YA-09). Base : audit §1.3, §1.6, §4.2, §4.3, §5.

Trois commits, dans cet ordre.

## Commit 1 : test de préfixe en CI

Test d'intégration Rust lancé par `ci.yml`, pour les adaptateurs Anthropic Messages et OpenAI Responses, sur un scénario de 3 tours avec tool calls (serveur HTTP local de test ou hook `on_payload`, `provider_adapter.rs:164`) :

- prompt système, tools et paramètres identiques octet par octet entre N et N+1 ;
- messages de N préfixe exact de ceux de N+1, en ignorant uniquement le déplacement de `cache_control` (`anthropic/convert.rs:244-279`) ;
- en cas d'échec, le message du test affiche la première différence.

Le test doit passer sur le code actuel avant le commit 2.

## Commit 2 : volatil hors du prompt système

Retirer de `environment_section` (`crates/pa-core/src/prompts/system_prompt.rs:271-294`) les trois lignes qui varient :

| Ligne | Nouvelle place |
|---|---|
| `Current date` | bloc « Environnement » à la fin du premier message utilisateur de la session, figé ensuite |
| `Working directory` | même bloc |
| `Conversation log` | accessible depuis le kernel (API ou variable existante si elle existe, sinon à ajouter) ; le prompt statique dit comment l'obtenir, sans la valeur |

- Reprise d'une session un autre jour : la nouvelle date est ajoutée à la fin du prochain message utilisateur, jamais dans le prompt système ni dans un message déjà envoyé.
- Même traitement pour les sous-agents `rlm`.
- Ce bloc est le point d'accueil de l'instantané d'environnement complet de YA-09 (arborescence, langages, commandes de test, état git), qui viendra plus tard.

Ajouter au test du commit 1 : deux sessions successives dans le même projet ont un prompt système identique octet par octet.

## Commit 3 : réglage du TTL

- Réglage `cacheRetention` (`"short"` par défaut, `"long"`) dans `settings.json`, à la place du `cache_retention: None` en dur (`provider_adapter.rs:161`).
- `PI_CACHE_RETENTION` garde la priorité si elle est définie (compatibilité upstream).
- Le banc de capture fixe la valeur explicitement.

## Hors périmètre

- Réduction du prompt sous 2 000 tokens (YA-09) : après le smoke bench.
- `prompt_cache_key` : reste l'id de session (décision CDC). Le partage du cache entre sessions côté OpenAI ne se mesure qu'avec de vrais appels.

## Mesures

Banc de capture, deux sessions successives :
- prompt système identique entre les deux sessions (oui/non, diff sinon) ;
- taille du préambule avant/après ;
- premier message utilisateur : contient la date et le cwd.

## Critères d'acceptation

- Test de préfixe vert en CI pour les deux adaptateurs.
- Prompt système identique octet par octet entre deux sessions du même projet, mesuré par le banc.
- Le modèle a toujours accès à la date, au cwd et au chemin du journal.
- `docs/fork-changes.md` à jour.

## Impact préfixe

Oui, voulu : le prompt système perd trois lignes, le premier message utilisateur gagne un bloc. Aucun changement en cours de session.

## Points ouverts

- **Signatures de texte et de raisonnement jamais posées par le flux Responses** (constaté le 14/11/2026, ticket 09, chauffe de la PR 2 de YA-03). Au `response.output_item.done`, `crates/pa-ai/src/providers/openai_responses_stream.rs` retire le slot de l'item (`self.slots.remove(&index)`, `:540-542`) avant de le relire (`current_slot(output_index)`, `:548-550`) : `content_index` vaut toujours `None` dès que l'événement porte `output_index` (cas réel), et ni `text_signature` (`:634-650`) ni `thinking_signature` (`:592-596`) ne sont posées. Comportement upstream de notre base de fork (`c83ac68b3`), corrigé upstream par `c12acadaa` (08/11/2026, « Preserve Responses reasoning when finalizing streamed items », #3976), complété par `42a07c13c` (09/11/2026, #4014) ; ni l'un ni l'autre n'est dans `main`. À reprendre par cherry-pick.
  - Effets : aucune session ne porte de `textSignature` ni de `thinkingSignature` (chauffe `bench/smoke/runs/20261113T230515Z-b73efb6-gpt-6.1-terra`, run `0fa8fad` du 06/11, A/B du 12/11). Les items de raisonnement ne sont jamais rejoués. Un message assistant avec texte rejoue son id en `msg_<index>` (`openai_responses_shared.rs:226-238`) : la base de la suite WebSocket convertit le message seul (index 0, `openai_codex_responses/mod.rs:499-511`), la requête complète suivante à son index réel (1 pour le premier message), les deux diffèrent et le tour suivant repart en corps complet, sans `previous_response_id`. Dans la chauffe, la requête 2 (après le premier `python_repl`, précédé d'un texte de 130 caractères) est dans ce cas ; les tours sans texte gardent la suite.
  - Le test de `pa-ai` qui couvre la suite contourne le défaut (item `done` sans `output_index`, commentaire de `openai_codex_responses/mod.rs:1165-1167`). Le scénario WebSocket de `crates/pa-core/tests/prefix_stability.rs` (`codex_websocket_apply_patch_replays_as_custom_and_continues`) envoie des `done` avec `output_index` et accepte les deux formes de la première requête après reprise ; après le cherry-pick, vérifier qu'elle part en suite de conversation.
