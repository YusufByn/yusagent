# Ticket 01 : désactivations par défaut et CI verte

Étape 3 de l'ordre de démarrage. Tickets CDC : YA-13 (télémétrie), YA-15 (Continual Harness hors MVP), YA-16 (CI). Base : `docs/audit/audit-entree.md` §1.13, §2.

## Objectif

Le fork démarre avec les fonctions hors MVP coupées et sans appel réseau hors provider, sans supprimer de code. La CI du fork est entièrement verte.

## Tâches

### 1. Valeurs par défaut (`crates/pa-core/src/settings/types.rs`)

| Réglage | Nouveau défaut | Référence audit |
|---|---|---|
| `harnessRefinements` | `false` | `types.rs:285` |
| `autoRefine.enabled`, `autoRefine.compact` | `false` | `types.rs:121-126` |
| `telemetry.enabled` | `false` (plus de notice au premier lancement) | `pa-cli/src/telemetry_notice.rs:23` |
| Vérification de version au lancement | désactivée par défaut, réglage pour la réactiver | à localiser |

Le catalogue de modèles en ligne (`pa-models`) reste actif : ce n'est pas de la télémétrie.

### 2. Skills hors MVP

Désactiver par défaut, via la config des skills existante, `agent-message` et `rlm-heartbeat` (et la commande `schedule` si un réglage existe). Ils disparaissent de l'inventaire des skills du prompt système : c'est une modification du prompt au démarrage de session, voulue.

Chaque désactivation reste réversible dans `settings.json`. Documenter les réglages dans `docs/fork-changes.md`.

### 3. CI

Le shard 1 de `ci.yml` échoue car l'étape `Prepare pinned TS frame comparison binary` (`ci.yml:347-361`) télécharge une release `v0.9.8` absente du fork. Appliquer le changement minimal qui rend le shard vert sans retirer de test utile au fork (par exemple : étape et test de comparaison TS sautés hors du repo upstream). Justifier le choix dans `docs/fork-changes.md`.

### 4. CLAUDE.md

Ajouter cette section après « Règles » :

```markdown
## Boucle de dev

1. Branche `ya-<ticket>`, commits préfixés par le ticket CDC.
2. Push : `ya-build-macos-arm64.yml` construit le binaire de la branche (environ 8 min).
3. `gh run download <run> --repo YusufByn/yusagent --dir <dossier>`, puis `python3 bench/tools/capture/run.py --binary <dossier>/prime-agent`.
4. PR vers `main` : `ci.yml` lance les tests Rust. Merge seulement si la CI est verte.
5. Mesures toujours avec `DO_NOT_TRACK=1 PI_OFFLINE=1` et `PI_CACHE_RETENTION` fixé.
```

## Mesures

- Banc de capture : préambule avant/après (caractères, tokens est.), ventilation par couche. Le delta vient de l'inventaire des skills.
- Télémétrie et vérification de version : test unitaire sur les valeurs par défaut, et `chemin:ligne` du code qui n'émet plus rien quand le réglage est faux.
- CI : lien du run vert sur la PR.

## Critères d'acceptation

- Session lancée sans `settings.json` ni variable d'environnement : ni télémétrie, ni vérification de version, ni `/refine` automatique (compaction comprise), ni skills `agent-message` / `rlm-heartbeat` dans le prompt.
- Chaque fonction se réactive par un réglage, testé.
- `ci.yml` vert sur la PR, 20 jobs sur 20.
- `docs/fork-changes.md` à jour.

## Impact préfixe

Oui, au démarrage de session seulement : l'inventaire des skills rétrécit. Aucun changement en cours de session.
