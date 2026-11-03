# Ticket 00 : audit d'entrée du fork

Étape 2 de l'ordre de démarrage (`docs/yusAgent-cdc.md`). Prépare YA-03, YA-04, YA-05, YA-06, YA-10.

## Objectif

Cartographier le code upstream et mesurer l'état du cache **avant toute modification**. Aucun changement de comportement du harness. Le rapport sert à écrire les tickets suivants avec les vrais chemins de fichiers.

## Contraintes

- Ne modifier aucun fichier upstream. Seuls ajouts autorisés : `docs/audit/`, `bench/tools/`.
- Aucun appel à un vrai modèle, aucune clé API, aucun login. Les requêtes du harness sont interceptées par un serveur local (base URL pointée vers lui).
- Pas de build natif local. Si un test upstream l'exige, le noter et passer.

## Tâches

### 1. Cartographie

Pour chaque élément : chemin(s), `chemin:ligne` des points clés, description en 2 ou 3 lignes.

- Packages du monorepo et rôle de chacun ; runtime Python du kernel (`prime-agent-runtime/`).
- Boucle agent (requête, tool calls, observations, arrêt).
- Construction du prompt système : ce qu'il contient, dans quel ordre, ce qui varie d'une session ou d'un tour à l'autre.
- Définition du tool `ipython` et mécanisme d'enregistrement des tools (pour ajouter `edit`, YA-03).
- Couche providers : adaptateurs (Anthropic, OpenAI Responses, Gemini, OpenRouter et API compatibles), streaming, retries, matrice des modèles.
- Cache : `cache_control` (où, combien de breakpoints), `prompt_cache_key`, TTL, usage remonté (`cache_read`, `cache_write`, raisonnement).
- Sessions JSONL : format, version éventuelle, reprise.
- Compaction : déclenchement, ce qui est résumé, conservation des messages utilisateur.
- `rlm()` : sémantique de retour (handle ou résultat), contexte du sous-agent.
- Gate autonome et option « pas de relance sans changement du workspace ».
- Lancement du kernel : process, environnement transmis (secrets visibles ou non), redémarrage après crash.
- Modes headless : ACP, RPC, JSON. Méthodes ACP implémentées (`initialize`, `session/new`, `session/prompt`, `session/update`, `session/request_permission`, `session/cancel`).
- Fonctions hors MVP : Continual Harness et `/refine`, messagerie entre agents, heartbeats, schedules, daemon. Existe-t-il des réglages pour les désactiver ?
- Login par abonnement Claude : localiser seulement, ne pas modifier.

### 2. Installation et tests upstream

- Installer selon `AGENTS.md` / docs de développement (`npm ci`, runtime Python).
- Lancer la suite de tests upstream ; noter durée, résultats, tests ignorés.
- Tailles : dépôt, `node_modules`, runtime Python, et estimation de l'empreinte installée hors venv (cible CDC < 50 Mo).
- Le repo contient-il déjà un provider factice ou des fixtures de réponses modèle ? Où ?

### 3. Serveur de capture

Dans `bench/tools/capture/` :

- Serveur HTTP local qui enregistre chaque corps de requête (JSON brut, horodaté) et renvoie une réponse factice valide, au minimum au format Anthropic Messages et OpenAI Responses, en streaming si le harness l'exige.
- Scénario scripté de 3 tours : tour 1 renvoie un tool call `ipython` simple (ex. `print(1)`), tour 2 un second tool call, tour 3 une réponse finale sans tool call.
- Le harness est pointé vers ce serveur par sa config de modèles ou providers personnalisés (base URL) ; documenter comment.

### 4. Mesures (via le serveur de capture)

- **Préambule** : taille du prompt système et des définitions de tools au premier appel, en caractères et en tokens estimés (tokenizer `o200k_base`, marqué comme estimation). Cible CDC YA-09 : < 2 000 tokens hors tools.
- **Préfixe** : pour chaque paire de requêtes N et N+1 du scénario, vérifier que le prompt système et les tools sont identiques octet par octet, et que les messages de N sont un préfixe exact des messages de N+1. Lister chaque différence avec sa cause (horodatage, ordre des clés, déplacement de `cache_control`, état injecté...).
- Recommencer le scénario dans une deuxième session et comparer les préambules des deux sessions (ce qui varie d'une session à l'autre).
- Vérifier si `/refine`, la compaction ou un redémarrage du kernel reconstruisent le prompt système.

### 5. Points d'entrée pour les tickets suivants

Pour chacun : où l'insérer, fichiers à toucher, estimation de l'écart avec upstream (faible, moyen, fort).

- YA-04 : test de préfixe automatisé, breakpoints, `prompt_cache_key`.
- YA-05 : masquage des observations anciennes avant envoi.
- YA-03 : tool `edit` séparé par famille, helper `bash()` dans le kernel, `read` numéroté, environnement filtré.
- YA-06 : `rlm()` en lecture seule avec résumé + extraits en variable du kernel, réglage `rlm`.
- YA-13 : journal d'usage par appel.
- YA-10 : mode ACP utilisable par l'extension VS Code.

## Livrables

- `docs/audit/audit-entree.md` : sections 1 à 5, chaque affirmation avec `chemin:ligne`, chaque mesure avec sa commande.
- `bench/tools/capture/` : serveur, scénario, README avec la commande unique pour relancer les mesures.
- Section finale « Recommandations » : ordre des tickets suivants, risques pour le cache, avis argumenté hard fork : quantité de code upstream à toucher pour YA-04 et YA-05.

## Critères d'acceptation

- Aucun fichier upstream modifié (`git diff upstream/main --stat` ne montre que `docs/` et `bench/`, plus `CLAUDE.md`).
- Mesures relançables par une seule commande documentée.
- Aucun appel réseau hors localhost pendant les mesures.
- Estimations explicitement marquées.
