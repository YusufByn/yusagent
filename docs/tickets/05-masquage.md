# Ticket 05 : masquage des observations anciennes

Ticket CDC : YA-05 (partie masquage). Base : audit §1.2, §5 (hook `transform_context`, `crates/pa-core/src/session_engine/engine.rs:724-729`).

## Objectif

Réduire les tokens envoyés sur les sessions longues en remplaçant les vieilles sorties de cellules par un placeholder, sans casser le cache à chaque tour. Livré derrière un réglage **désactivé par défaut** : il ne sera activé qu'après preuve sur le banc (critère CDC YA-05 : coût -30 % sans baisse significative de résolution).

## Règles

- Nouveau module `pa-core/src/session_engine/masking.rs`, branché sur `transform_context`. Le JSONL de session n'est pas modifié (append-only conservé) : seul le contexte envoyé change.
- Ne masque que le contenu des résultats de `python_repl`. Jamais les messages utilisateur, jamais les messages assistant ni le code des cellules.
- **Paliers** : une sortie est masquée si elle a plus de `masking.window` tours (défaut 10). La frontière de masquage n'avance que tous les `masking.step` tours (défaut 5). Le préfixe change donc au plus une fois par palier.
- **Déterminisme** : le placeholder ne dépend que des données enregistrées (numéro de tour, taille, identifiant d'entrée). Deux reconstructions du même contexte donnent les mêmes octets.
- Placeholder : `[sortie masquée : tour <k>, <n> caractères, relire avec <appel kernel>]`. L'appel kernel permet de relire la sortie complète depuis l'historique de session (utiliser l'API existante si elle existe, sinon l'ajouter).
- **Garde-fous** :
  - jamais la dernière sortie en erreur (exception Python ou `bash()` avec code de sortie non nul) ;
  - jamais une sortie de moins de `masking.minChars` caractères (défaut 500) : la masquer ne fait rien gagner et fait perdre de l'information.
- Le seuil de compaction s'appuie sur l'usage réel du dernier appel : il tient compte du masquage sans changement.

## Réglages

`masking.enabled` (défaut `false`), `masking.window` (10), `masking.step` (5), `masking.minChars` (500).

## Mesures

Nouveau cas dans le banc de capture : 25 tours, chaque cellule produit environ 5 000 caractères, une sortie en erreur au tour 12. Avec et sans masquage :
- nombre de ruptures de préfixe sur la session (attendu avec masquage : une par palier franchi, zéro ailleurs) ;
- tokens est. envoyés par requête et au total ;
- la sortie en erreur du tour 12 reste visible tant qu'aucune erreur plus récente n'existe.

Ce sont des mesures de tokens. Le gain réel de coût et l'effet sur la résolution se mesurent ensuite au smoke bench (YA-14).

## Critères d'acceptation

- Masquage désactivé : requêtes identiques octet par octet au comportement actuel (test).
- Masquage activé : ruptures de préfixe uniquement aux paliers (test).
- Garde-fous testés.
- Sortie masquée relisible depuis le kernel.
- `docs/fork-changes.md` à jour.

## Impact préfixe

Désactivé : aucun. Activé : une rupture voulue par palier, à journaliser comme invalidation (champ dans le journal d'usage du ticket 04 si déjà livré, sinon dans le rapport de PR).
