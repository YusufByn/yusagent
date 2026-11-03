# yusAgent

Hard fork de Prime Agent (MIT), transformé en harness d'agent de code cache-first, piloté par une extension VS Code via ACP.

- Référence : `docs/yusAgent-cdc.md`. Le journal des décisions en tête du fichier fait foi.
- Conventions de code héritées d'upstream : `AGENTS.md`.
- Travail découpé en tickets dans `docs/tickets/`. Ne traiter que le ticket demandé.

## Références

- AGENTS.md:183 est obsolète : le modèle ne voit qu’un tool, python_repl ; bash() et edit() sont des fonctions du kernel.

## Règles

1. **Périmètre** : ne modifier que ce que le ticket demande. Si un blocage impose de sortir du périmètre, s'arrêter et le signaler au lieu de modifier.
2. **Écarts avec upstream** : nos ajouts vont dans des modules séparés quand c'est possible. Toute modification d'un fichier upstream est minimale et listée dans `docs/fork-changes.md` (fichier, raison, ticket). Objectif : garder les `git cherry-pick` depuis `upstream` faciles.
3. **Couche providers** : la modifier le moins possible, c'est la zone où upstream apporte le plus.
4. **Cache de prompt** : rien de volatile dans le prompt système ni dans les définitions de tools (date, état git, cwd, état du harness). Tout changement qui modifie le préfixe envoyé au modèle est signalé explicitement dans le résumé de la tâche.
5. **Variantes** : toute variante à comparer (bash natif, `rlm`, masquage, effort) est un réglage dans la même branche, jamais une branche séparée.
6. **Modèles réels** : aucun appel à un vrai modèle sans demande explicite de Yusuf. Les tests et la CI utilisent le provider factice ou des requêtes interceptées. Aucun jeton « OAuth abonnement pour provider » en CI.
7. **Login par abonnement Claude** hérité d'upstream : ne pas y toucher, Yusuf le retire à la main.
8. **Builds** : pas de build natif lourd en local (binaires, Rust) ; ils se font en CI. Pas de nouvelle dépendance lourde dans le cœur.
9. **Secrets** : jamais dans les logs, les commits, les rapports ni les fixtures.
10. **Mesures** : aucun chiffre inventé. Chaque mesure est produite par une commande reproductible, notée dans le rapport. Une estimation est marquée comme telle.
11. **Preuves** : toute affirmation sur le code cite `chemin:ligne`.
12. **Commits** : préfixés par le ticket (`YA-04: ...`), un sujet par commit.

## Boucle de dev

1. Branche `ya-<ticket>`, commits préfixés par le ticket CDC.
2. Push : `ya-build-macos-arm64.yml` construit le binaire de la branche (environ 8 min).
3. `gh run download <run> --repo YusufByn/yusagent --dir <dossier>`, puis `python3 bench/tools/capture/run.py --binary <dossier>/prime-agent`.
4. PR vers `main` : `ci.yml` lance les tests Rust. Merge seulement si la CI est verte.
5. Mesures toujours avec `DO_NOT_TRACK=1 PI_OFFLINE=1` et `PI_CACHE_RETENTION` fixé.

## Fin de tâche

Résumé court : ce qui a été fait, fichiers touchés (upstream ou nouveaux), commandes pour tester, impact éventuel sur le préfixe de prompt, points ouverts.
