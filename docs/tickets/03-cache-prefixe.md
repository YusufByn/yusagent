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
