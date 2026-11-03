# Ticket 02 : environnement filtré du kernel

Tickets CDC : YA-03, YA-11 (décision du 31/10 : secrets jamais transmis au kernel ni à `bash()`). Base : audit §1.11.

## Constat

`kernel_spawn_env` part de `std::env::vars()` (`crates/pa-core/src/kernel/manager/startup.rs:532-545`) : le kernel et tous les `bash()` voient les clés API et jetons du harness. Mesuré dans le banc (tour 2 : `['ANTHROPIC_API_KEY', 'YA_CAPTURE_FAKE_TOKEN']`).

## Tâches

1. Remplacer l'héritage complet par une liste blanche :
   - base système : `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TERM`, `LANG`, `LC_*`, `TZ`, `TMPDIR` ;
   - variables que le runtime Python et le harness posent ou lisent eux-mêmes : les recenser (`grep` de `os.environ` / `getenv` dans `prime-agent-runtime/` et des `envs(...)` côté Rust), lister le résultat dans le rapport de PR ;
   - variables nécessaires à uv et au venv du kernel.
2. Réglage `kernel.envAllowlist` (liste de noms, motifs `*` acceptés) dans `settings.json`, pour les besoins d'un projet (ex. `DATABASE_URL`).
3. Les identifiants de provider que le harness utilise ne passent jamais, même s'ils sont dans `kernel.envAllowlist` : avertissement dans les logs, sans la valeur.
4. Même environnement pour le shell persistant et les sous-process de `bash()` (vérifier qu'ils héritent du kernel et de rien d'autre).

## Hors périmètre, à documenter seulement

Sans sandbox, un process du même utilisateur peut lire les fichiers d'auth du harness, et sous Linux `/proc/<pid>/environ` du parent, qui garde l'environnement initial même après `unsetenv`. Relever, sans corriger :
- où sont stockés les jetons et clés (`chemin:ligne`, chemin du fichier) ;
- sur macOS, si `ps eww <pid du harness>` lancé depuis le kernel affiche l'environnement du harness.
Ces points serviront au sandbox (YA-11).

## Mesures et tests

- Banc de capture : le tour 2 du scénario de base doit renvoyer `[]`. Ajouter au scénario une variable non sensible déclarée dans `kernel.envAllowlist` : elle doit être visible.
- Test unitaire Rust sur la construction de l'environnement (liste blanche, motifs, refus des identifiants provider).
- Non-régression : `bash("uv --version")`, `bash("git status")`, `bash("python3 -c 'print(1)'")` fonctionnent dans le kernel.
- Tests du runtime Python en local, tests Rust en CI.

## Critères d'acceptation

- Aucune clé API, aucun jeton OAuth, aucune variable `*_API_KEY`, `*_TOKEN`, `*_SECRET` visible depuis `os.environ`, `env` ou `printenv` dans le kernel et `bash()`, sauf liste blanche explicite de l'utilisateur, et jamais les identifiants provider.
- Fonctionnement du kernel inchangé sur le scénario de base.
- `docs/fork-changes.md` à jour.

## Impact préfixe

Aucun.
