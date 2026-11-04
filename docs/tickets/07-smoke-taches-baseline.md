# Ticket 07 : tâches du smoke bench et baseline

Ticket CDC : YA-14 (smoke bench). Base : T06 (`bench/smoke/`, PR #9), premier run réel du 04/11 (plomberie validée sur `gpt-6-luna`).

## Décisions (04/11/2026)

- Tâches : 10 vrais bugs corrigés dans des dépôts publics Python et TS/JS, rejoués. Les projets de Yusuf pourront s'ajouter plus tard (banc complet, held-out).
- Runs : baseline de 3 runs sur `main` pour mesurer le bruit ; ensuite 1 run par PR, 2 runs de plus seulement si `compare.py` lève une alerte.
- Modèle du smoke de PR : `gpt-6.1-terra`. `gpt-6-luna` en option (`MODEL=gpt-6-luna`), pas par défaut.

## Hors périmètre

Aucun changement du harness (`crates/`, `prime-agent-runtime/`). Aucun appel à un vrai modèle par Claude Code : les runs réels sont lancés par Yusuf.

## Phase A : candidats (s'arrêter à la fin et rapporter)

Script `bench/smoke/tools/mine.py` (bibliothèque standard + `gh`), réseau autorisé. Cherche des commits de correction qui remplissent tous ces critères :

- langage Python ou TypeScript/JavaScript ; licence permissive (MIT, Apache-2.0, BSD) ;
- commit postérieur au 01/08/2026 (proposé, pour limiter le risque que le modèle connaisse déjà la correction) ;
- le commit modifie du code source **et** des tests ; au moins un test passe de rouge à vert avec la correction ;
- installation et tests sans service externe ni Docker (`uv sync` / `npm ci`, puis tests hors ligne) ; durée des tests < 2 min ;
- dépôt petit à moyen (taille et nombre de fichiers notés).

Sortie : `bench/smoke/tools/candidates.csv` avec, par candidat : dépôt, commit, licence, langage, fichiers et lignes modifiés par la correction, tests rouge → vert, durée des tests, lien vers l'issue ou la PR. Au moins 25 candidats, dont au moins 8 Python et 6 TS/JS.

Rapporter la liste et s'arrêter : Yusuf choisit les 10.

Mélange visé pour le choix (proposé) : 7 tâches à priori réussissables (correction d'1 ou 2 fichiers), 3 plus dures, dont au moins 2 multi-fichiers (3 fichiers ou plus) pour avoir des sessions longues (masquage).

## Phase B : construction des 10 tâches

Pour chaque tâche choisie, format du T06 (`task.toml`, `prompt.md`, `hidden/`) :

- `source` : `git` + commit parent de la correction ; aucun code du dépôt versionné dans yusAgent. Si un patch est nécessaire, il reste minimal. Licence du dépôt notée dans `task.toml`.
- `hidden/` : les tests ajoutés ou modifiés par la correction.
- `prompt.md` : le symptôme, réécrit en quelques phrases (comportement attendu, comportement observé, exemple), sans indice sur la solution ni sur les fichiers à modifier. Pas de copie de l'issue.
- `fail_to_pass` et `pass_to_pass` tirés des tests.
- `tags` : `python` ou `node`, `multi-fichiers`, `difficile`.

Validation de chaque tâche, en préparation (sans modèle) :

- état cassé : `fail_to_pass` échoue, `pass_to_pass` passe (déjà fait par le runner) ;
- **correction officielle** (nouvelle option `--validate-gold`) : avec le patch du commit appliqué, tous les tests passent ;
- 3 exécutions de chaque vérification sous sandbox, réseau coupé : aucun test instable. Une tâche instable est remplacée.

## Mesure ajoutée : miss complets côté serveur

Constat du 04/11 : un appel de boucle en continuation, sur la même connexion WebSocket, avec un corps de taille normale, a eu `cacheRead = 0` en pleine session. Côté harness, la requête était identique aux autres.

- Par tâche : nombre de miss complets en cours de session (appel de boucle hors premier, `cacheRead = 0`, pas d'invalidation journalisée), avec pour chacun le délai depuis l'appel précédent, le transport et la continuation (journal du proxy).
- Taux de cache « évitable » : calculé en excluant le premier appel, les invalidations journalisées et ces miss. L'ancien taux reste affiché.

## Baseline (lancée par Yusuf)

1. `make smoke BINARY=... MODEL=gpt-6.1-terra RUNS=3` sur `main`.
2. 1 run chacun de `gpt-6.1-sol` et `gpt-6-luna` comme référence (proposé ; à sauter si le quota ne suit pas).
3. `bench/smoke/tools/baseline.py <runs>` écrit `bench/smoke/baseline/gpt-6.1-terra.json` : par tâche, taux de résolution, écart-type et coefficient de variation du coût et des tokens, nombre de miss serveur. Ce fichier est versionné.

`compare.py` lit ce fichier : une alerte se déclenche si l'écart dépasse le bruit mesuré (proposé : 2 fois le coefficient de variation de la tâche, et au moins 5 %), ou si une tâche stable (3/3 en baseline) n'est pas résolue. Le seuil provisoire de 10 % du T06 disparaît.

## Critères d'acceptation

- Phase A : `candidates.csv` rapporté, aucun dépôt cloné dans yusAgent.
- Phase B : 10 tâches validées (cassé, correction officielle, 3 exécutions stables), en mode capture et sans modèle.
- `make smoke CAPTURE=1` passe sur les 10 tâches (rapport complet, les tâches peuvent échouer puisque le serveur de capture ne les résout pas).
- Métrique des miss serveur testée en mode capture (scénario qui renvoie `cacheRead = 0` au milieu d'une session).
- CDC : journal du 04/11 complété (tâches publiques, politique de runs, modèle de PR `gpt-6.1-terra`).
- `docs/fork-changes.md` à jour si un fichier upstream est touché.

## Impact préfixe

Aucun.
