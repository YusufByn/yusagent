# Ticket 04 : usage fiable et journal par appel

Ticket CDC : YA-13 (base). Base : audit §1.6, §5.

## Tâches

### 1. Correctif Anthropic

Au `message_start`, l'usage est lu sur `message.*` au lieu de `message.usage.*` (`crates/pa-ai/src/providers/anthropic/stream.rs:297-306`). Corriger, puis fusionner proprement avec l'usage du `message_delta` (`stream.rs:590-600`) sans double comptage.

Vérifier le format sur la doc officielle d'Anthropic (streaming, champs d'usage, `cache_creation` avec ventilation 5 min / 1 h) et citer la page dans la PR. Fixtures de test conformes à la doc.

### 2. Champ raisonnement

- Ajouter à `Usage` (`crates/pa-types/src/ai/mod.rs:423-433`) un champ optionnel pour les tokens de raisonnement : absent quand le provider ne l'expose pas, jamais 0 par défaut.
- Sérialisation rétrocompatible : une session JSONL écrite avant le changement se relit sans erreur (test).
- Le remplir pour les providers qui l'exposent. Vérifier chaque champ sur la doc officielle du provider avant de l'utiliser (par exemple les détails de tokens de sortie chez OpenAI Responses, les tokens de pensée chez Gemini).

### 3. Normalisation des tokens d'entrée

Les providers ne comptent pas l'entrée de la même façon (tokens en cache inclus ou non dans le total d'entrée). Documenter dans `docs/usage.md` la sémantique de `input`, `cacheRead` et `cacheWrite` après normalisation, par provider, avec `chemin:ligne`. Définir à partir de là :

```
taux de cache d'une requête = cacheRead / (input + cacheRead + cacheWrite)
```

et l'ajuster si la normalisation l'impose. Tests par provider sur fixtures.

### 4. Journal d'usage

Une ligne JSONL par appel modèle, à côté du JSONL de session (`<id>.usage.jsonl`), champ `v` de version :

- horodatage, id de session, id de session parente (sous-agent), numéro de tour ;
- provider, modèle, effort de raisonnement ;
- type d'appel : `main`, `compaction`, `refine`, `subagent`, autre ;
- `input`, `output`, `cacheRead`, `cacheWrite` (ventilé 5 min / 1 h si disponible), raisonnement ;
- TTFT et durée en ms, coût calculé, taux de cache de la requête ;
- délai depuis la requête précédente de la session (état du TTL).

Point d'accroche : `on_response` / usage des `AssistantMessage` (`provider_adapter.rs:164`). Les appels annexes (résumé de compaction, gate `/refine`) doivent aussi être journalisés.

## Mesures

- Étendre `bench/tools/capture/server.py` pour renvoyer un usage réaliste (champs de cache compris) dans les deux formats.
- Banc : le journal est produit, chaque champ correspond à ce que le serveur a renvoyé, le taux de cache est correct.
- Aucun appel réel. Une vérification sur un vrai appel se fera seulement sur demande de Yusuf.

## Critères d'acceptation

- Avec un flux Anthropic dont le `message_delta` ne porte que `output_tokens`, l'usage enregistré contient les bons `input`, `cacheRead`, `cacheWrite`.
- Champ raisonnement rempli pour au moins un provider, absent pour les autres.
- Anciennes sessions toujours lisibles.
- Journal d'usage écrit pour tous les types d'appel.
- `docs/fork-changes.md` et `docs/usage.md` à jour.

## Impact préfixe

Aucun : rien de ce qui est envoyé au modèle ne change.
