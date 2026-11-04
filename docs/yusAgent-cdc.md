# Cahier des charges yusAgent

Harness d'agent de code minimaliste, cache-first, piloté depuis VS Code. Version 4.1 (03/11/2026) : constats de l'audit d'entrée du fork (cœur Rust, un seul tool `python_repl`). Version 4 (31/10/2026) : base = fork de Prime Agent (MIT), plus de contrainte de langage pour le cœur. Version 3 (28/10/2026) : kernel Python comme outil d'exécution unique, bash appelé en async depuis le kernel. Version 2 (24/10/2026) : priorité à la capacité maximale du modèle.

## Journal des décisions

| Date | Décision | Statut | Tickets touchés |
|---|---|---|---|
| 24/10/2026 | Priorité à la capacité maximale du modèle, coût minimal à qualité égale | Validé | YA-00, YA-12, YA-17 |
| 28/10/2026 | Pas de tool `bash` séparé : le kernel IPython est l'outil d'exécution unique, bash est une fonction async du kernel (`await bash(...)`) | Validé, à confirmer par le banc | YA-03, YA-11, YA-12, YA-14, YA-17 |
| 28/10/2026 | Le tool `edit` reste séparé, au format natif de chaque famille | Validé, à confirmer par le banc | YA-03, YA-14, YA-17 |
| 28/10/2026 | Sandbox du kernel : P1 en usage perso avec auto-approve des cellules verrouillé hors sandbox ou conteneur ; obligatoire et bloquant pour la publication publique | Validé | YA-11, YA-16 |
| 28/10/2026 | Kernel dans le venv du harness ; le code du projet s'exécute via `bash()` dans son propre environnement (ex. `uv run pytest`), rien n'est installé dans l'environnement de l'utilisateur | Validé | YA-03, YA-07 |
| 28/10/2026 | Crash du kernel : redémarrage auto, helpers rechargés, observation « kernel redémarré, variables perdues » en fin de contexte, aucune cellule rejouée | Validé | YA-01, YA-03 |
| 28/10/2026 | Providers : ChatGPT, Gemini, Kimi, Mistral, DeepSeek, OpenRouter, Claude, au choix de l'utilisateur. Chaque provider utilise le mode d'auth qu'il permet : « OAuth abonnement pour provider » quand c'est autorisé, clé API sinon. Builds publiés en v2 : clés API uniquement | Validé | YA-02, YA-13, YA-16 |
| 28/10/2026 | Tests : la CI n'utilise que le provider factice (lint, tests unitaires et d'intégration, build). Les tests avec un vrai modèle (smoke bench, banc) tournent en local avec « OAuth abonnement pour provider » en MVP et v1, puis passent en CI en v2 avec les clés API | Validé | YA-01, YA-14, YA-16 |
| 28/10/2026 | Smoke bench local tracé : rapport horodaté avec hash du commit, case dédiée dans le modèle de PR | Validé | YA-14 |
| 31/10/2026 | Cache : objectifs relevés (session ≥ 90 % MVP, ≥ 95 % v1 ; ≥ 98 % médian par requête de continuation), misses évitables et coût des reconstructions mesurés. Indicateur dans l'extension : barre de contexte en %, tokens / fenêtre du modèle, « Cache XX % » de la dernière requête avec détail au survol | Validé | YA-04, YA-10, YA-13 |
| 31/10/2026 | Effort de raisonnement : sélecteur à côté du modèle, limité aux niveaux que le modèle supporte (low, medium, high, xhigh, max selon le provider) ; xhigh par défaut à l'ouverture du chat (ou le plus haut niveau disponible en dessous) ; changement en cours de session journalisé comme invalidation de cache ; le banc mesure chaque niveau par modèle | Validé | YA-02, YA-10, YA-12, YA-17 |
| 31/10/2026 | Anti-gaspillage : relance après sortie coupée, détection de boucle, `read()` numéroté et `edit()` qui renvoie la zone modifiée, test non relancé sans changement du workspace, principes anti-gaspillage dans le prompt, instantané d'environnement dans le premier message | Validé, à confirmer par le banc | YA-01, YA-03, YA-07, YA-09 |
| 31/10/2026 | `rlm()` dès le MVP en version minimale (lecture seule, profondeur 1, résumé + extraits exacts en variable du kernel), réglage `rlm` figé par session et désactivé par défaut jusqu'au banc, option « appliquer et continuer » ; A/B avec et sans sur le même commit | Validé | YA-06, YA-14 |
| 31/10/2026 | Banc : réseau coupé en phase agent, taux de bascule publié, résultats en différence appariée (équivalence ou supériorité avec IC), suivi tokens et coût dans le smoke bench | Validé | YA-14 |
| 31/10/2026 | Compaction : messages utilisateur conservés mot pour mot ; exécution ordonnée des tool calls à effet de bord d'un même tour ; mesure du raisonnement renvoyé par tour | Validé | YA-01, YA-05, YA-13 |
| 31/10/2026 | Secrets jamais transmis au kernel ni à `bash()` (environnement filtré, secrets retirés de l'environnement du harness) ; mode CLI avant l'extension, utilisé par le banc ; adaptateur Harbor dans `bench/` ; format JSONL des sessions versionné | Validé | YA-01, YA-03, YA-10, YA-11, YA-14 |
| 28/10/2026 | Windows : WSL2 obligatoire pour l'exécution, pas de backend Windows natif. En Windows natif, l'extension reste installable mais l'exécution est désactivée avec proposition de rouvrir le dossier dans WSL | Validé | YA-03, YA-10, YA-11, YA-16 |
| 31/10/2026 | Base du harness : fork de Prime Agent (MIT). La plomberie commune (providers, « OAuth abonnement pour provider » et clés API, streaming, sessions JSONL, kernel, `rlm()`, gate autonome) est reprise ; les choix de ce CDC sont construits dessus. Remplace le choix « pas de fork » de la v3. Plus de contrainte de langage pour le cœur (hôte TypeScript de Prime Agent, runtime du kernel en Python). Code Rust autorisé ; les builds lourds restent en CI | Validé | YA-00, YA-01, YA-02, YA-03, YA-10, YA-16 |
| 31/10/2026 | Mode de fork : hard fork, sans rebase sur upstream ; récupération sélective des commits upstream utiles (`git cherry-pick` : correctifs, nouveaux modèles, auth). Couche providers gardée au plus près d'upstream ; ajouts du CDC (cache, masquage, `edit`, banc) dans des modules séparés autant que possible. Repo neuf sous YusufByn avec upstream en remote (pas le bouton Fork de GitHub, pour pouvoir passer le repo en privé plus tard) ; public au départ | Validé | YA-02, YA-04, YA-05, YA-16 |
| 03/11/2026 | Constat de l'audit d'entrée (`docs/audit/audit-entree.md`) : le cœur upstream est un workspace Rust (10 crates `pa-*`, environ 620 k lignes), pas un hôte TypeScript comme écrit le 31/10 ; le kernel est un REPL CPython maison, pas IPython ; le modèle ne voit qu'un tool, `python_repl`, avec `bash()` et `edit()` dans le kernel (conforme à la décision du 28/10 sur bash). Le code TypeScript ne subsiste qu'au tag v0.9.8 | Constat | YA-00, YA-01, YA-03, YA-16 |
| 04/11/2026 | Retrait des secrets de l'environnement du harness abandonné : inefficace (mesuré le 03/11 : ps eww sur macOS, /proc sous Linux lisent l'environnement de départ). Le filtrage de l'environnement du kernel reste la règle ; protection complète par le sandbox (YA-11) | Validé | YA-03, YA-11 |
| 04/11/2026 | Smoke bench (YA-14) : provider ChatGPT en « OAuth abonnement pour provider », modèles `gpt-6.1-sol`, `gpt-6.1-terra` et `gpt-6-luna` (aucun modèle antérieur, ni 5.x ni `gpt-6-sol`). `gpt-6-astra` est autorisé par le runner mais hors des runs par défaut (consommation trop élevée sur l'abonnement). Effort de raisonnement fixé à high au banc | Validé | YA-12, YA-14 |
| 04/11/2026 | Mention « clé API uniquement » propre à un provider retirée du CDC ; le mode d'auth reste celui que chaque provider permet | Validé | YA-02 |

## Résumé

- **Position** : fork de Prime Agent (MIT ; cœur en Rust, runtime du kernel en Python) transformé en harness inspiré de mini-SWE-agent et des RLM : REPL Python persistant comme outil d'exécution unique, bash appelé en async depuis le kernel, outil d'édition adapté à chaque famille de modèles. Piloté par une extension VS Code TypeScript via le protocole ACP. La plomberie commune de Prime Agent est reprise ; la valeur ajoutée est dans le contexte, le cache, les formats d'édition et le banc.
- **Objectif n°1** : extraire 100 % de la capacité du modèle utilisé. Le harness ne doit jamais être le facteur limitant.
- **Objectif n°2** : à qualité égale ou supérieure, coût minimal. Leviers : taux de hit du cache de prompt (préfixe stable, contexte append-only), masquage des observations anciennes, réduction des tours inutiles. La compaction par résumé LLM vient en dernier recours.
- **Preuve de valeur** : banc d'évaluation reproductible (sous-ensemble SWE-bench Verified + Terminal-Bench 4.0 + bench maison), à modèle égal contre mini-SWE-agent, Claude Code et Codex CLI.

## État de l'art 2025-2026

### 1. Minimal vs riche en tools : l'écart se resserre

- mini-SWE-agent (bash uniquement, environ 100 lignes pour la classe agent) dépasse 74 % sur SWE-bench Verified selon son README et sert de référence au leaderboard bash-only de SWE-bench.
- arXiv 2606.20683 : Claude Sonnet 4 obtient 64,9 % avec mini-SWE-agent contre 72,4 % avec SWE-agent + tools, mais Claude Opus 4.5 obtient 76,8 % avec mini-SWE-agent contre 73,2 % avec SWE-agent + tools et 80,9 % avec le scaffold du fournisseur.
- Interprétation : pour un modèle frontier, un harness minimal bien conçu atteint environ 95 % de la performance du harness vendeur. Les derniers points viennent de l'ajustement fin (formats d'édition entraînés, prompts, contexte, fonctions natives), pas du nombre de tools. C'est là que yusAgent doit aller chercher le surplus.

### 2. Le harness compte autant que le modèle

- Terminal-Bench 2.1 : Gemini 3 Pro passe de 65,8 % (Gemini CLI) à 73,9 % (Terminus 2), soit 8,1 points à modèle égal. GPT-5.5 obtient 83,1 % dans Codex contre 78,0 % dans Terminus 2. Terminus 2 n'a qu'un seul outil : un terminal headless.
- Anthropic (« Quantifying infrastructure noise in agentic coding evals ») mesure 6 points d'écart sur Terminal-Bench 2.0 uniquement en faisant varier CPU/RAM du sandbox. Tout banc maison doit fixer et documenter les ressources.
- SWE-ABS (arXiv 2603.00520) : les scores bash-only chutent de 15 à 19 points quand les tests de SWE-bench Verified sont renforcés. Le taux de réussite brut surestime la qualité réelle des patchs.

### 3. RLM et Prime Agent : le REPL comme outil unique

- Recursive Language Models (arXiv 2512.24601) : le prompt long est une variable dans un REPL Python, le modèle s'appelle récursivement sur des fragments. RLM-Qwen3-8B dépasse Qwen3-8B de 28,3 % en moyenne.
- Reproduction (arXiv 2603.02615) : la récursion de profondeur 2 fait sur-réfléchir le modèle et fait exploser temps (3,6 s à 344,5 s) et coût. Profondeur 1 par défaut.
- Prime Agent (Prime Intellect, août 2026, MIT) : un seul outil (kernel IPython persistant), sous-agents via `await rlm(...)`, sessions en JSONL append-only, compaction au seuil, état d'harness modifiable (prompt, sous-agents, skills, mémoire), `/refine` avec plus petite modification justifiée, prompt de base immuable, rollback par ID.
- Prime Intellect revendique 95,5 % sur ARC-AGI-3 avec Opus 5 et moins de tokens que les harness natifs. Résultats auto-déclarés, aucun modèle n'est entraîné pour ce harness.

### 4. Coût : le cache d'abord

- Manus : le taux de hit du KV-cache est la métrique n°1 d'un agent en production (ratio entrée/sortie de l'ordre de 100:1, environ 50 appels de tools par tâche). Règles : préfixe stable, contexte append-only, sérialisation JSON déterministe, masquer les tools plutôt que les retirer.
- Anthropic : écriture 1,25x (TTL 5 min) ou 2x (TTL 1 h), lecture 0,1x. OpenAI : cache automatique dès 1 024 tokens, `prompt_cache_key`. Gemini : cache implicite automatique et cache explicite avec stockage payant.
- OpenAI (« Unrolling the Codex agent loop ») : les données envoyées croissent de façon quadratique avec le nombre de tours.
- « The Complexity Trap » (JetBrains Research, arXiv 2508.21433) : le masquage des observations anciennes (fenêtre M=10) réduit le coût de 50,9 % à 57,1 % sur 4 configurations sur 5 sans perte de résolution sur SWE-bench Verified, et égale le résumé LLM. L'hybride masquage + résumé gagne encore 7 à 11 %.
- Taux de cache mesurés par des tiers (oct. 2026) : environ 99,4 à 99,8 % par requête en régime établi sur Codex (ticket openai/codex #56050), médiane de 98 % par requête de continuation et 95,3 % sur toutes les requêtes pour Claude Code (gateway Wokey), 92 % pour Claude Code et 89 % pour OpenCode en agrégé (Requesty, avril 2026). Les pertes viennent surtout des reprises après pause (TTL expiré), des premiers tours et des sous-agents. Un taux élevé peut masquer des reconstructions coûteuses : un miss coûte environ 12 fois un hit (kvleak).
- Anthropic « advanced tool use » : Tool Search -85 % de tokens de définitions, Programmatic Tool Calling -37 % sur tâches complexes (valide l'approche REPL).

### 5. Qualité : formats d'édition, vérification, sous-agents

- Aider : le passage à l'unified diff a fait passer GPT-4 Turbo de 20 % à 61 % ; désactiver le patch « flexible » multiplie par 9 les erreurs d'édition. Diff-XYZ (arXiv 2510.12487) : search/replace est le format le plus fiable pour les modèles forts.
- Les modèles Codex sont entraînés sur `apply_patch` (V4A) : le format d'édition doit dépendre de la famille du modèle.
- Anthropic (multi-agent research system) : +90,2 % avec orchestrateur Opus et sous-agents Sonnet, mais environ 15x les tokens d'un chat. Les sous-agents sont rentables pour l'exploration parallèle, pas pour l'implémentation.
- Anthropic (« Effective harnesses for long-running agents ») : un fichier de progression et l'historique git permettent à une nouvelle fenêtre de contexte de reprendre le travail.

### 6. Sécurité

- Anthropic : 93 % des demandes de permission sont approuvées (fatigue d'approbation). Le sandbox OS (bubblewrap Linux, Seatbelt macOS, écriture limitée au workspace, réseau via proxy) réduit de 84 % les demandes. Runtime open source `@anthropic-ai/sandbox-runtime`. Pas de support Windows natif (WSL2).

### 7. Intégration éditeur

- Agent Client Protocol (ACP, Zed, août 2025) : JSON-RPC 2.0 sur stdio (`initialize`, `session/new`, `session/prompt`, `session/update`, `session/request_permission`, `session/cancel`), SDK Python `agent-client-protocol`.
- Cline : Extension, Webview React, Controller, Task ; clés dans `SecretStorage`.
- API VS Code utiles : `languages.getDiagnostics` / `onDidChangeDiagnostics`, shell integration (`executeCommand`, `onDidEndTerminalShellExecution`), `vscode.diff` et `vscode.changes`, `workspace.applyEdit`, `SecretStorage`, `registerUriHandler`, webviews avec CSP stricte. La Language Model API (`vscode.lm`) dépend de Copilot et ne gère pas les messages système : inadaptée comme couche provider.

### 8. Auto-amélioration et authentification

- Meta-Harness (arXiv 2603.28052) optimise le code du harness à partir des traces brutes et dépasse les harness faits main sur TerminalBench-2 (76,4 % contre 74,7 % avec Opus 4.6). Risque principal : surapprentissage au benchmark.
- Certains fournisseurs interdisent l'usage des jetons d'abonnement dans des outils tiers (Anthropic depuis janvier-février 2026). Le mode « OAuth abonnement pour provider » est un mode de développement personnel, à vérifier provider par provider, jamais un mode de distribution.

---

## EPIC YA-00 : yusAgent

### Vision

Un harness d'agent de code qui permet d'utiliser un modèle frontier à 100 % de ses capacités, sans que le harness soit jamais le facteur limitant, et qui, à qualité égale ou supérieure, coûte moins cher en tokens et en tours que les harness de référence. Le tout mesuré de façon reproductible. Harness = fork de Prime Agent ; interface = extension VS Code. Nom provisoire : yusAgent (alternative : yusAi-agent).

### Hiérarchie des objectifs

1. **Capacité maximale du modèle** : utiliser toutes ses fonctions natives, ses formats entraînés et tout le raisonnement nécessaire.
2. **Coût minimal à qualité égale ou supérieure** : le coût baisse par l'ingénierie (cache, contexte propre, moins de tours inutiles), jamais en bridant le modèle.
3. Une optimisation de coût n'est activée par défaut que si le banc montre zéro perte de taux de résolution (IC 95 %).

### Contexte

- Développeur : Yusuf (GitHub YusufByn), stack Python/FastAPI, Node/React, LangGraph, multi-agents, RAG, MCP.
- Projet précédent : yusAi, fork de l'IDE agentique Sinew (Tauri 2, React, Rust, Monaco), tools MCP custom (SupabaseQuery, DatabaseQuery, HttpRequest, LogsWatch, Deploy), OAuth en Rust, intégration Prime Agent commencée. Abandonné notamment à cause du `target/` Rust d'environ 40 Go.
- Nouvelle direction : extension VS Code (TypeScript) qui pilote un fork de Prime Agent centré sur un REPL persistant façon RLM, avec bash appelé depuis le REPL, et réutilisation des tools MCP de yusAi.
- Projet portfolio : la méthodologie d'évaluation publique est un livrable à part entière.

### KPI

Cibles à valider par le banc YA-14, à modèle et ressources identiques.

| KPI | Définition | Cible MVP | Cible v1 | Cible v2 |
|---|---|---|---|---|
| Taux de résolution | % de tâches résolues (50 SWE-bench Verified + 20 Terminal-Bench 4.0 + 20 maison) | ≥ mini-SWE-agent | ≥ harness du fournisseur | > harness du fournisseur |
| Résolution tâches difficiles | même mesure sur le sous-ensemble difficile, mode effort maximal | mesuré | ≥ harness du fournisseur | > harness du fournisseur |
| Coût par tâche résolue | $ total / tâches résolues | ≤ mini-SWE-agent | -30 % vs fournisseur, à résolution égale ou supérieure | idem |
| Taux de cache (session) | tokens lus en cache / tokens d'entrée, sur la session entière | ≥ 90 % | ≥ 95 % | ≥ 95 % |
| Taux de cache par requête | médiane sur les requêtes de continuation (hors premier tour et invalidations journalisées) | ≥ 98 % | ≥ 98 % | ≥ 98 % |
| Coût des reconstructions | $ dépensés en réécriture de cache (TTL expiré, invalidations), affiché à part | mesuré | mesuré | mesuré |
| Tours par tâche | médiane des appels modèle | mesuré | -20 % vs MVP sans perte de résolution | idem |
| Edits appliqués | % d'éditions appliquées au premier essai | ≥ 95 % | ≥ 98 % | ≥ 98 % |
| Cellules en erreur d'appel | % de cellules du kernel qui échouent sur une erreur de syntaxe Python ou de quoting autour d'un `bash()` | mesuré | ≤ 2 % | ≤ 1 % |
| Empreinte disque | extension + harness, hors venv (62 Mo mesurés pour le harness seul au 03/11, cible à revoir) | < 50 Mo | < 50 Mo | < 50 Mo |
| Démarrage | nouvelle session jusqu'au premier token | < 2 s | < 1 s | < 1 s |

### Périmètre

- Harness (fork de Prime Agent) : boucle agentique, tools (REPL Python avec bash async, édition), gestion de contexte, sous-agents, vérification, télémétrie.
- Exploitation maximale des capacités de chaque famille de modèles (YA-17).
- Couche providers multi-fournisseurs (ChatGPT, Gemini, Kimi, Mistral, DeepSeek, OpenRouter, Claude) : « OAuth abonnement pour provider » quand le provider l'autorise, clé API sinon ; builds publiés en clés API uniquement (v2).
- Extension VS Code : chat, diffs, permissions, diagnostics, terminal, réglages.
- Banc d'évaluation reproductible et publication des résultats.
- Auto-amélioration contrôlée (v2).

### Hors périmètre

- IDE propre ou fork d'éditeur.
- Entraînement ou fine-tuning de modèles.
- Exécution cloud multi-utilisateurs, facturation, comptes.
- Exécution sous Windows natif (backend PowerShell ou cmd). Sous Windows, l'exécution passe obligatoirement par WSL2 ; Docker reste une alternative.
- Redistribution d'un accès « OAuth abonnement pour provider » à des tiers.

### Contraintes

- Stockage local léger : builds lourds (binaires natifs, code Rust éventuel) construits en CI GitHub Actions et publiés en release, jamais en local.
- Boucle de dev (constat de l'audit du 03/11) : push sur une branche `ya-*`, binaire macOS arm64 construit en CI (environ 8 min, `ya-build-macos-arm64.yml`), mesures locales avec le banc de capture (`bench/tools/capture/`). Tests Rust en CI ; seul le runtime Python se teste en local.
- Pas de dépendance lourde ajoutée au cœur (pas de LangGraph).
- UI et logs en français et en anglais.
- Pas de secrets en clair : `SecretStorage` côté extension, variables d'environnement côté harness.

### Choix techniques recommandés

| Décision | Choix | Justification |
|---|---|---|
| Base | Fork de Prime Agent (MIT), choix du CDC construits dessus (décision du 31/10/2026) | Plomberie commune (providers, auth, streaming, sessions, kernel) déjà écrite et testée ; le temps de build va aux différences qui comptent |
| Cœur | Boucle de Prime Agent (Rust, crates `pa-agent` et `pa-core`), REPL Python persistant de Prime Agent comme outil d'exécution unique (tool `python_repl`) | RLM et Prime Agent ; Programmatic Tool Calling |
| Bash | Fonction async du kernel (`await bash(...)`), pas de tool `bash` séparé | Commandes parallèles et filtrage des sorties en code dans un même tour ; un seul point d'exécution à sécuriser ; à confirmer face à un bash natif sur le banc (YA-14) |
| Protocole | ACP (JSON-RPC 2.0, NDJSON sur stdio) + extensions `_yus/*` ; mode ACP de Prime Agent (opérationnel ; manquent `session/request_permission` et `session/load`, audit du 03/11) | Standard ouvert, compatible Zed et JetBrains |
| Format d'édition | Tool `edit` séparé, par famille : `apply_patch` V4A (Codex), `str_replace` (Claude), search/replace (défaut) | Formats entraînés ; Diff-XYZ ; Aider |
| Contexte | Append-only + masquage, compaction LLM en dernier recours | Manus ; « The Complexity Trap » |
| Providers | Couche providers de Prime Agent (héritée de pi), auditée pour le cache ; ajustements ciblés là où l'audit montre un manque (breakpoints, `prompt_cache_key`, usage cache lu/écrit) | Évite de réécrire streaming et auth ; le contrôle fin du cache reste exigé (YA-04) |
| Sandbox | `@anthropic-ai/sandbox-runtime` ou conteneur ; optionnel en usage perso, obligatoire pour l'auto-approve des cellules et pour la publication | -84 % de demandes de permission ; seul moyen de contenir le Python arbitraire du kernel (YA-11) ; utilisateurs tiers sur leurs propres machines et projets |

---

## Sous-tickets

### YA-01 : Boucle agentique et modèle de session

- **Description** : boucle unique « requête modèle, tool calls, observations, requête », arrêt quand le modèle répond sans tool call ou appelle `finish()`. Session en JSONL append-only (messages, changements de modèle, compactions, événements) avec numéro de version du format, branchement par pointeur de feuille. Budgets configurables (tours, tokens, temps, $), aucun par défaut qui coupe le modèle en plein travail.
- **Justification** : mini-SWE-agent ; Prime Agent (JSONL append-only) ; Anthropic Managed Agents (session hors fenêtre de contexte, interrogeable).
- **Critères d'acceptation** :
  - Boucle de Prime Agent réutilisée ; chaque modification par rapport à upstream est isolée et listée, testée avec un provider factice.
  - Reprise sans perte depuis le JSONL après kill -9.
  - Champ de version du format dans chaque session ; une session d'une version antérieure reste lisible par le banc.
  - Budget atteint : arrêt propre et événement `budget_exceeded`.
  - Tool calls parallèles exécutés en concurrence s'ils sont en lecture seule ; les tool calls à effet de bord d'un même tour sont exécutés dans l'ordre émis, avec arrêt au premier échec (éditer puis tester en un seul appel modèle).
  - Sortie coupée par le plafond de tokens : le harness relance le modèle pour qu'il continue au lieu de clore la tâche ; sessions terminées sur une coupure proches de zéro.
  - Détection de boucle : 5 cellules identiques avec le même résultat déclenchent un rappel injecté en fin de contexte ; 8 échecs identiques arrêtent la tâche avec un événement `loop_detected`.
- **Dépendances** : aucune. **Priorité** : P0. **Complexité** : M.

### YA-02 : Couche providers, « OAuth abonnement pour provider » et clés API

- **Description** : base = couche providers de Prime Agent (héritée de pi). Interface interne unique (messages, blocs texte/raisonnement/tool_use/tool_result, usage avec cache lu/écrit). Adaptateurs Anthropic Messages, OpenAI Responses, Gemini, OpenRouter, plus les API compatibles (Kimi, Mistral, DeepSeek). Le modèle se choisit dans les réglages. Deux modes d'auth, selon ce que chaque provider autorise : « OAuth abonnement pour provider » (usage perso) et clé API (seul mode des builds publiés). Normalisation du streaming, des erreurs, des retries, et matrice de capacités par modèle (fenêtre, cache, parallélisme, raisonnement, vision, format d'édition).
- **Justification** : mécanismes de cache et de raisonnement différents selon les providers ; restrictions des fournisseurs sur les jetons d'abonnement dans les outils tiers.
- **Critères d'acceptation** :
  - Un même scénario (lecture, édition, test) passe sur au moins 3 providers sans changement de la boucle.
  - Usage remonté : `input`, `output`, `cache_read`, `cache_write`, `reasoning` quand exposés.
  - Login par abonnement Claude hérité de Prime Agent : conservé pour l'instant, Yusuf le retire à la main plus tard (hors du plan de build).
  - « OAuth abonnement pour provider » désactivé par défaut dans les builds publiés, avec avertissement sur les conditions du provider.
  - Clés jamais écrites dans les logs.
- **Dépendances** : YA-01. **Priorité** : P0. **Complexité** : M.

### YA-03 : Tools de base (REPL Python avec bash async, édition)

- **Description** :
  - **Tools exposés au modèle** : `python` (exécute une cellule dans le kernel) et `edit` (format natif de la famille). Aucun tool `bash` séparé.
  - `python` (tool `python_repl` de Prime Agent) : REPL Python persistant (CPython, pas IPython, constat de l'audit du 03/11), un par session, `await` au niveau supérieur. Il tourne dans le venv du harness (helpers et ipykernel garantis). Le code du projet n'est jamais importé dans le kernel : il s'exécute via `bash()` dans l'environnement du projet (`uv run`, `npm test`, venv détecté). Rien n'est installé dans l'environnement de l'utilisateur. Helpers préchargés : `bash`, `read`, `grep`, `glob`, `edit`, `diagnostics()`, `run_tests()`, `rlm()`, `web`, tools MCP. Le modèle traite les données en code au lieu de les lire dans le contexte.
  - `bash` (helper async) : `await bash(cmd, timeout=..., cwd=..., env=...)` renvoie un objet résultat (code de sortie, tête et queue de la sortie, chemin du fichier de sortie complète, durée). Parallélisme via `asyncio.gather`.
    - Shell principal persistant (`cd` et `export` conservés) pour les appels séquentiels.
    - Si le shell principal est occupé, l'appel part dans un sous-process éphémère qui hérite de son cwd et de son env. Les changements d'état de ces appels ne sont pas conservés, et le prompt système le dit.
    - Pas de magics shell (le REPL n'est pas IPython, audit du 03/11) : une cellule `!cmd` est une erreur de syntaxe, comptée dans les cellules en erreur d'appel.
  - `edit` : format selon la famille de modèle, correspondance tolérante aux espaces, erreurs exploitables. Renvoie la zone modifiée avec ses numéros de ligne, pour éviter une relecture de vérification. Aussi disponible comme helper du kernel pour les éditions en lot.
  - `read` : renvoie le contenu avec numéros de ligne, pour éviter les relectures de repérage.
  - Pont MCP : tools yusAi (SupabaseQuery, DatabaseQuery, HttpRequest, LogsWatch, Deploy) exposés comme fonctions du kernel.
  - `web` : recherche et lecture de pages (docs officielles, changelogs, issues) exposées comme fonctions du kernel, contenu marqué non fiable (YA-11). Un modèle très capable mais sans accès à la doc à jour raisonne sur des connaissances périmées.
  - Crash du kernel : redémarrage automatique, rechargement des helpers, observation « kernel redémarré, variables perdues » injectée en fin de contexte. Aucune cellule rejouée (risque de rejouer des effets de bord).
  - Environnement filtré : le kernel et les `bash()` ne reçoivent ni clés API, ni jetons OAuth, ni autres secrets (liste blanche de variables). Le retrait des secrets de l'environnement du harness est abandonné (voir le journal des décisions, 04/11/2026).
- **Justification** : Prime Agent ; RLM ; Programmatic Tool Calling (-37 %) ; Aider (patch flexible) ; guide Codex `apply_patch`. Le choix bash-dans-le-kernel s'écarte des références code (mini-SWE-agent, Terminus 2, bash direct) : il est validé par décision et doit être confirmé par le banc (YA-14).
- **Critères d'acceptation** :
  - Jeu de tools figé par session.
  - Aucune sortie de cellule > plafond configuré (défaut 8 000 tokens) ; au-delà, chemin de fichier renvoyé.
  - Edits appliqués au premier essai ≥ 95 %.
  - Au moins 4 commandes `bash()` lancées en parallèle dans une même cellule, sortie de chacune accessible séparément.
  - Un `cd` fait via le shell principal est visible dans l'appel séquentiel suivant.
  - Taux de cellules en erreur d'appel (syntaxe, quoting) mesuré et remonté en télémétrie (YA-13).
  - Kernel tué en pleine session : redémarrage et reprise du tour en < 5 s.
  - Tools MCP appelables et filtrables en Python avant d'entrer dans le contexte.
- **Dépendances** : YA-01. **Priorité** : P0. **Complexité** : L.

### YA-04 : Optimisation du cache de prompt

- **Description** : ordre figé (système, tools, AGENTS.md/skills, historique) ; rien de volatile dans le préfixe (date et état git en fin de message utilisateur) ; JSON trié ; breakpoints Anthropic automatiques ; `prompt_cache_key` = ID de session chez OpenAI ; TTL 5 min ou 1 h selon le délai médian entre tours ; keep-warm optionnel pendant les attentes d'approbation.
- **Justification** : Manus ; tarifs officiels des providers.
- **Critères d'acceptation** :
  - Taux de cache de session ≥ 90 % (MVP) puis ≥ 95 % (v1) sur sessions de plus de 10 tours.
  - Taux médian par requête de continuation ≥ 98 %, hors invalidations journalisées.
  - Misses évitables (tokens déjà envoyés au tour précédent et non lus en cache, hors invalidation journalisée) proches de zéro ; chaque cas est tracé avec sa cause.
  - Test de non-régression : le préfixe de la requête N+1 commence octet par octet par celui de la requête N.
  - La liste des helpers du kernel décrite dans le prompt système est figée en début de session (un redémarrage du kernel ne la modifie pas).
  - Tableau de bord : coût réel et coût théorique sans cache.
- **Dépendances** : YA-02. **Priorité** : P0. **Complexité** : M.

### YA-05 : Gestion du contexte et compaction

- **Description** :
  1. Masquage : au-delà de N tours (défaut 10), les observations anciennes sont remplacées par un placeholder avec chemin de la sortie complète, appliqué par paliers pour limiter les invalidations de cache. **Garde-fou qualité** : jamais sur les sorties encore utiles (dernier échec de test, fichiers en cours d'édition, plan, diagnostics actifs).
  2. Notes structurées : `progress.md` et plan tenus par l'agent, relus en début de fenêtre.
  3. Compaction LLM : au seuil (défaut 80 % de la fenêtre) ou à la demande, résumé typé (objectif, état, fichiers modifiés, décisions, prochaines étapes) ; historique complet interrogeable depuis le kernel. **Les messages de l'utilisateur sont conservés mot pour mot**, jamais résumés : les compacteurs évalués ne gardent en moyenne que 17 % des consignes de l'utilisateur (arXiv 2608.11242).
- **Justification** : « The Complexity Trap » ; Anthropic (prise de notes, progression + git) ; Prime Agent.
- **Critères d'acceptation** :
  - Masquage : coût -30 % minimum vs agent brut, sans baisse significative de résolution (IC 95 %), sinon désactivé par défaut.
  - Chaque compaction journalisée (tokens avant/après, coût d'invalidation).
  - Une session de plus de 3 fenêtres aboutit sur une tâche maison longue.
  - Test de consignes : des contraintes injectées dans les messages utilisateur (ex. « ne supprime rien sans confirmation ») sont toutes présentes après compaction.
- **Dépendances** : YA-01, YA-04. **Priorité** : P0 (masquage), P1 (compaction LLM). **Complexité** : M.

### YA-06 : Sous-agents et routage de modèles

- **Description** :
  - `await rlm(task, model=..., budget=...)` depuis le kernel, contexte isolé, profondeur 1. Le modèle décide lui-même quand l'appeler (jamais systématique).
  - **Version MVP minimale** : sous-agents en lecture seule (`read`, `grep`, `glob`, `bash()` en lecture), pas d'`edit`, budget strict, appels parallèles possibles. Retour : un résumé court dans le contexte et les extraits exacts (fichier, lignes, code) dans une variable du kernel, pour que l'agent principal n'ait pas à relire les mêmes fichiers.
  - **Réglage `rlm`** (activé ou désactivé) dans la config, en option CLI et dans les réglages de l'extension. Une seule branche de code : chaque variante à tester est un réglage, jamais une branche. Désactivé par défaut jusqu'au résultat du banc.
  - Le réglage est figé par session (la liste des helpers du prompt système en dépend). Changement en cours de conversation : option « appliquer et continuer » qui garde l'historique, coûte une reconstruction de cache affichée avant confirmation et journalisée comme invalidation volontaire (YA-04).
  - Prompt système et liste de tools des sous-agents identiques d'un appel à l'autre, pour réutiliser leur préfixe en cache.
  - **Garde-fou qualité** : jamais de petit modèle sur la planification, le diagnostic, l'édition ou la revue. Le petit modèle est limité à l'exploration en lecture seule et aux résumés.
- **Justification** : Anthropic multi-agents ; RLM et Prime Agent ; reproduction RLM (profondeur 2 nuisible) ; Anthropic (22/10/2026) recommande de replier les sous-agents dans la boucle principale sauf gain mesuré ; chez Claude Code, la moitié des relectures redondantes viennent de code déjà lu par un sous-agent qui n'a renvoyé qu'un résumé (arXiv 2609.30725).
- **Critères d'acceptation** :
  - Budget strict par sous-agent, coût imputé à la session parente.
  - Résumé plafonné (défaut 2 000 tokens) ; extraits disponibles en variable du kernel.
  - A/B avec et sans `rlm()` publié dans le premier rapport de banc (YA-14).
  - Taux de cache des sous-agents mesuré (préfixe réutilisé entre appels).
  - Routage petit modèle activé par défaut uniquement si le banc ne montre aucune perte de résolution.
- **Dépendances** : YA-02, YA-03. **Priorité** : P0 (version minimale en lecture seule, MVP), P1 (extensions, v1). **Complexité** : M.

### YA-07 : Boucle de vérification

- **Description** : détection auto des commandes de test, lint, typecheck (package.json, pyproject, Makefile, AGENTS.md). Gate avant fin de tâche : la vérification doit passer, sinon sortie bornée renvoyée à l'agent. Pas de relance sans changement du workspace, tentatives limitées. En cours de tâche aussi : une commande de test identique relancée sans changement du workspace n'est pas réexécutée, le harness renvoie le chemin du résultat précédent avec une mention ; `force=True` permet de relancer (tests instables). L'agent est incité à écrire un test de reproduction avant de corriger un bug.
- **Justification** : gate autonome de Prime Agent ; Aider Polyglot ; SWE-ABS.
- **Critères d'acceptation** :
  - Gate détecté sur au moins 5 types de projets (Python, Node, TS, Go, Rust).
  - Pas de fin de tâche gate rouge sans confirmation explicite.
  - Gain de résolution mesuré sur le banc.
- **Dépendances** : YA-03. **Priorité** : P0. **Complexité** : S.

### YA-08 : Intégration LSP et diagnostics

- **Description** : après chaque édition, l'extension récupère les diagnostics VS Code des fichiers touchés et les renvoie au harness via une méthode ACP d'extension. Hors VS Code : repli sur pyright, tsc, ruff, eslint.
- **Justification** : API VS Code ; ne remonter que `event.uris` pour ne pas saturer le contexte.
- **Critères d'acceptation** :
  - Seuls les diagnostics nouveaux ou modifiés des fichiers édités, plafonnés à 30 lignes.
  - Attente configurable (défaut 1,5 s).
  - Taux d'erreurs de type introduites mesuré avec et sans diagnostics.
- **Dépendances** : YA-03, YA-10. **Priorité** : P1. **Complexité** : M.

### YA-09 : Context engineering (AGENTS.md, mémoire, skills)

- **Description** : chargement de `AGENTS.md` (et CLAUDE.md) à la racine et dans les sous-dossiers concernés ; skills `SKILL.md` à la demande (seule la description dans le préfixe) ; mémoire projet dans `.yusagent/` ; repo map compacte (tree-sitter) à la demande, plafonnée et mesurée (un retrieval structurel verbeux peut augmenter le coût).
  - Principes anti-gaspillage dans le prompt système (6 ou 7 règles courtes et générales) : réutiliser ce qui est déjà lu, garder un script dans une fonction du kernel et le modifier plutôt que le réécrire, identifier la commande de test une seule fois, ne relancer un test qu'après un changement ou une nouvelle information, sortir d'une impasse plutôt que répéter.
  - Instantané d'environnement au démarrage (dossier, arborescence de premier niveau, langages, gestionnaires de paquets, commandes de test détectées par YA-07, état git), placé dans le premier message utilisateur et figé ensuite, jamais dans le prompt système.
- **Justification** : Anthropic context engineering ; Tool Search ; Prime Agent.
- **Critères d'acceptation** :
  - Prompt système de base < 2 000 tokens hors tools.
  - Ajout d'un skill sans modification du préfixe en cours de session.
  - `.yusagent/` dans le `.gitignore` proposé.
- **Dépendances** : YA-01, YA-04. **Priorité** : P1. **Complexité** : M.

### YA-10 : Extension VS Code (chat, diff, permissions)

- **Description** :
  - Harness lancé via `child_process`, piloté en ACP sur stdio, redémarrage auto en cas de crash.
  - Webview React avec CSP stricte : streaming, plan, raisonnement visible, tool calls repliables, coût et taux de cache en direct.
  - Indicateur de contexte et de cache, mis à jour à chaque requête :
    - barre de contexte remplie selon tokens du dernier prompt / fenêtre du modèle actif, pourcentage affiché au-dessus (ex. « 37 % ») ;
    - compteur de tokens sur la fenêtre (ex. « 370k / 1M »), dénominateur lu dans la matrice de capacités (YA-02), pas fixé à 1M ;
    - « Cache 99 % » = taux de la dernière requête ; au survol : taux de la session, misses évitables, coût des reconstructions, état du cache (chaud ou froid) et temps restant avant expiration du TTL.
    - Calculé uniquement à partir de l'usage renvoyé par le provider : aucun impact sur le prompt ni sur le cache.
  - Sélecteur de modèle et, à côté, sélecteur d'effort de raisonnement (niveaux lus dans la matrice de capacités, xhigh par défaut, YA-12).
  - Cellules du kernel affichées avec leur code et, pour chaque `bash()`, la commande, le code de sortie et la sortie repliable.
  - Diffs via `vscode.diff` / `vscode.changes`, acceptation ou rejet par hunk, application via `workspace.applyEdit`.
  - Permissions via `session/request_permission` (une fois, toujours pour cette commande, refuser).
  - Commandes visibles dans un terminal via shell integration, repli côté harness.
  - Clés dans `SecretStorage`, callback OAuth via `registerUriHandler`.
  - Captures d'écran et images transmissibles au modèle (vision).
  - Windows natif détecté : exécution désactivée, message explicite et proposition de rouvrir le dossier dans WSL (extension WSL de Microsoft). En mode WSL, le harness tourne côté Linux sans code spécifique.
- **Justification** : architecture Cline ; spec ACP ; doc VS Code.
- **Critères d'acceptation** :
  - .vsix < 5 Mo.
  - Aucune écriture disque sans flux diff/permission, sauf auto-approve explicite.
  - `session/cancel` stoppe le tour en < 1 s, y compris une cellule en cours (interruption du kernel et des sous-process `bash()`).
  - Harness fonctionnel dans Zed via ACP (test manuel documenté).
  - Sous Windows natif, aucune cellule ne s'exécute ; le passage en WSL est proposé en un clic.
- **Dépendances** : YA-01, YA-02, YA-03. **Priorité** : P0. **Complexité** : L.

### YA-11 : Sécurité et sandbox

- **Description** :
  - Permissions par défaut : lecture libre dans le workspace, écriture via diff, commandes classées (lecture seule auto-approuvée ; `rm -rf`, `git push --force`, `DROP`, déploiements toujours confirmés), réseau sur liste blanche.
  - Le classifieur de commandes s'applique à chaque `bash()` (YA-03). **Limite** : le Python arbitraire du kernel (`subprocess`, `os`, `shutil`, `socket`, requêtes HTTP) n'est pas filtrable de façon fiable. Sans sandbox, il n'est couvert que par l'avertissement persistant ; une détection heuristique (imports et appels sensibles, confirmation de la cellule) peut s'ajouter mais reste contournable et doit être présentée comme telle.
  - Sandbox OS (`@anthropic-ai/sandbox-runtime`, WSL2 sous Windows, ou Docker), appliqué au process du kernel entier, pas seulement aux commandes `bash()`.
  - **Verrou auto-approve** : hors sandbox ou conteneur, l'auto-approve des cellules du kernel est impossible. Chaque cellule à effet de bord (écriture, réseau, process) demande confirmation, quel que soit le réglage de l'utilisateur.
  - **Usage perso vs publication** : en usage perso (MVP), le sandbox reste optionnel grâce au verrou. Pour la publication publique, il est obligatoire et bloquant (YA-16) : les utilisateurs tiers exécutent l'agent sur leurs propres machines et projets, avec des réglages par défaut qu'ils ne liront pas forcément.
  - Réglages par défaut sûrs pour tous : auto-approve désactivé, sandbox activé quand la plateforme le permet, détection et message clair quand elle ne le permet pas.
  - Contenus externes (web, issues, fichiers non suivis) marqués non fiables.
  - Interdiction de lire `~/.ssh`, `.env` hors workspace, magasins de secrets.
  - Deploy et DatabaseQuery en écriture : confirmation systématique.
- **Justification** : Anthropic (fatigue d'approbation, -84 % avec sandbox) ; « trifecta létale » (données privées + contenu non fiable + exfiltration). Avec le kernel comme outil d'exécution unique, le sandbox devient le seul contrôle complet. Prime Agent précise lui-même que ses process worker et kernel ne sont pas un sandbox.
- **Critères d'acceptation** :
  - Suite de tests d'injection (README, page web, commentaire) sans commande destructrice ni exfiltration en mode sandbox, y compris via du Python direct (`subprocess`, `os.remove`, requête réseau).
  - Aucune commande shell exécutée hors du backend `bash()` par les helpers du harness.
  - Décisions de permission journalisées.
  - Avertissement persistant en mode sans sandbox.
  - Auto-approve des cellules refusé hors sandbox ou conteneur, testé.
  - Aucun secret du harness (clés API, jetons OAuth) visible depuis le kernel ou `bash()` (`os.environ`, `env`, `/proc`), testé.
  - Sandbox fonctionnel sur Linux et macOS, et sous Windows via WSL2 ou Docker.
- **Dépendances** : YA-03, YA-10. **Priorité** : P0 (permissions et verrou auto-approve), P1 (sandbox en usage perso), P0 bloquant pour la publication (YA-16). **Complexité** : M.

### YA-12 : Budget de raisonnement et réduction des tours

- **Description** : **effort sélectionnable à côté du modèle**, limité aux niveaux qu'il supporte (low, medium, high, xhigh, max selon le provider), **xhigh par défaut** à l'ouverture du chat (ou le plus haut niveau disponible en dessous). Le banc mesure chaque niveau par modèle (résolution et coût par tâche résolue) pour informer le choix. Une éventuelle réduction automatique d'effort ne concerne que les actions mécaniques (lecture, listing, commandes triviales). Prompt système qui incite à regrouper le travail dans une même cellule (`asyncio.gather` sur plusieurs `bash()`, lectures multi-fichiers, filtrage des sorties en code avant affichage). Signatures de raisonnement conservées entre tours quand le provider l'exige.
- **Justification** : croissance quadratique des données (OpenAI) ; Programmatic Tool Calling.
- **Critères d'acceptation** :
  - A/B sur le banc : effort fixe maximal vs adaptatif ; l'adaptatif n'est activé que s'il ne perd rien en résolution.
  - Tours médians -20 % entre MVP et v1 sans perte de résolution.
  - Part des cellules qui lancent plusieurs `bash()` en parallèle mesurée.
- **Dépendances** : YA-02, YA-14. **Priorité** : P1. **Complexité** : S.

### YA-13 : Télémétrie coût et tokens

- **Description** : chaque appel modèle journalisé localement (OpenTelemetry ou JSONL) : tokens par catégorie, cache lu/écrit, TTFT, $, tour, sous-agent, tokens de raisonnement renvoyés dans l'historique à chaque tour (à garder au minimum exigé par le provider). Côté kernel : cellules exécutées, cellules en erreur (par type), appels `bash()` par cellule et durée. Côté cache : taux par requête et par session, misses évitables avec leur cause, coût des reconstructions, délai depuis la dernière requête (pour l'état du TTL). Ces données alimentent l'indicateur de contexte et de cache de YA-10. Table de prix versionnée. Tableau de bord dans l'extension, export CSV. Aucune télémétrie distante par défaut.
- **Justification** : taux de cache et coût par tâche résolue sont des KPI centraux.
- **Coût sous abonnement** : en « OAuth abonnement pour provider », il n'y a pas de facture à l'appel. Le coût affiché et utilisé par le banc est un coût théorique (tokens × tarif API public de la table de prix). La comparaison avec une facture réelle n'est possible qu'avec des clés API (v2).
- **Critères d'acceptation** :
  - Écart coût calculé vs facture provider < 2 % sur 100 appels (v2, avec clés API).
  - Taux de cache visible en temps réel.
- **Dépendances** : YA-02. **Priorité** : P0. **Complexité** : S.

### YA-14 : Banc d'évaluation

- **Description** :
  - Terminal-Bench 4.0 via Harbor, version épinglée (`harbor run -d terminal-bench/terminal-bench@4.0.0`) : sous-ensemble de 20 tâches en développement, run complet des 66 tâches seulement aux jalons (coût et durée élevés : timeout de 8 h par tâche). Terminal-Bench est désormais un benchmark continu à versions sémantiques : toute nouvelle version impose de relancer les références avant de comparer.
  - Adaptateur Harbor dans `bench/` : agent externe chargé par chemin d'import Python, qui lance le harness en mode CLI sans VS Code.
  - Harness officiel SWE-bench (Docker), plus 20 tâches maison réalistes (FastAPI, React, TS, migrations, bugs multi-fichiers) issues des projets de Yusuf, avec un sous-ensemble « tâches difficiles » identifié.
  - Comparaison à modèle égal : mini-SWE-agent, Claude Code, Codex CLI, OpenCode. Prime Agent d'origine (validé 31/10/2026) : lancé aux jalons uniquement, jamais sur chaque PR, pour isoler l'apport des modifications du fork à coût limité.
  - **Validation du choix d'exécution (décision du 28/10/2026)** : variante interne « bash natif » (tool `bash` séparé, sans `bash()` dans le kernel) comparée à la configuration par défaut « kernel seul ». Si la variante bash natif fait mieux en résolution (IC 95 %), retour à un tool bash natif et mise à jour du CDC.
  - **A/B avec et sans `rlm()`** (même commit, seul le réglage `rlm` change), uniquement sur la configuration par défaut « kernel seul », pas croisé avec l'A/B bash natif. Le bench maison inclut des tâches d'exploration sur un gros dépôt (analyse multi-fichiers), sinon le résultat est biaisé contre `rlm()`.
  - Variantes testées = réglages, jamais des branches ; chaque rapport note le hash du commit et les réglages.
  - Réseau coupé pendant la phase agent (helper `web` désactivé, seul l'endpoint du modèle autorisé) : sur SWE-bench, des agents avec réseau récupèrent le correctif officiel sur GitHub (71 % des essais DeepSeek et Qwen dans une étude).
  - 3 runs minimum par configuration, IC 95 % par bootstrap, ressources CPU/RAM fixées et publiées. Taux de bascule entre deux relances de la même configuration publié à côté de chaque écart.
  - Formulation des résultats : différence appariée avec IC 95 % ; « équivalent » par test d'équivalence (marge annoncée à l'avance), « meilleur » seulement si l'IC exclut zéro. Avec environ 90 tâches, seul un grand écart de résolution est détectable ; le coût par tâche résolue, bien moins bruité, se compare avec peu de tâches.
  - Métriques : résolution (globale et difficile), coût par tâche résolue, tokens par catégorie, tours, latence, taux de cache, edits échoués, cellules en erreur d'appel.
  - Ensemble held-out jamais utilisé pour le réglage.
- **Justification** : bruit d'infrastructure (Anthropic) ; SWE-ABS ; leaderboards harness+modèle ; écarts entre scores officiels et reproductions ; absence de comparaison publique « kernel seul » vs « bash natif » sur du code.
- **Critères d'acceptation** :
  - `make bench MODEL=... HARNESS=...` reproductible, rapport HTML + CSV.
  - Comparaison « kernel seul » vs « bash natif » publiée dans le premier rapport (MVP), sur au moins 2 familles de modèles.
  - Comparaison avec et sans `rlm()` publiée dans le premier rapport (MVP).
  - Le smoke bench suit aussi tokens et coût par tâche : une hausse de tokens sans gain de résolution par rapport au rapport précédent est signalée sur la PR.
  - Run complet de la suite interne < 30 $ avec un modèle milieu de gamme.
  - Smoke bench de 10 tâches sur chaque PR touchant prompt, tools ou contexte : en local (`make smoke`) en MVP et v1, en CI à partir de la v2 avec les clés API. Aucun jeton « OAuth abonnement pour provider » dans la CI.
  - `make smoke` écrit un rapport horodaté avec le hash du commit testé ; le modèle de PR contient une case « smoke bench lancé sur ce commit ». Les rapports sont conservés pour comparer les versions.
- **Dépendances** : YA-01, YA-03, YA-13. **Priorité** : P0. **Complexité** : L.

### YA-15 : Auto-amélioration (phase ultérieure)

- **Description** : état d'harness versionné (notes de prompt, skills, mémoire, specs de sous-agents). `/refine` analyse la trajectoire et propose la plus petite modification justifiée. Rien n'est appliqué sans passer le held-out ; rollback par ID ; prompt système de base immuable. Option : boucle hors ligne façon Meta-Harness (traces brutes, scores, code des variantes, patch proposé, le banc décide).
- **Justification** : Prime Agent ; Meta-Harness ; risque de surapprentissage au benchmark.
- **Critères d'acceptation** :
  - Chaque modification porte déclencheur, diff et effet mesuré.
  - Aucune régression > 1 point sur le held-out.
  - Modifications appliquées en frontière de session uniquement (préserve le cache).
  - Désactivé par défaut.
- **Dépendances** : YA-09, YA-14. **Priorité** : P2. **Complexité** : L.

### YA-16 : Packaging, CI et publication

- **Description** : harness distribué comme Prime Agent (installeur avec binaires de release construits en CI, ou npm), extension sur VS Code Marketplace et Open VSX. CI GitHub Actions : lint, tests avec provider factice, build .vsix ; smoke bench ajouté en v2 avec les clés API. Binaires natifs construits en CI, jamais en local. Bascule de « OAuth abonnement pour provider » vers clés API avant publication. README avec résultats du banc, guide d'architecture, démo vidéo.
- **Justification** : contrainte de stockage ; politiques des fournisseurs.
- **Critères d'acceptation** :
  - Installation < 2 min sur machine vierge.
  - < 50 Mo sur disque hors environnement Python.
  - Aucune étape de build local > 1 Go.
  - Licence MIT, copyrights de Prime Agent et pi conservés dans LICENSE, attribution de mini-SWE-agent.
  - **Gate de publication** : aucune publication publique tant que le sandbox du kernel (YA-11) n'est pas fonctionnel et que la suite de tests d'injection ne passe pas en mode sandbox.
  - Doc d'installation Windows : WSL2 requis, projets à placer dans le système de fichiers Linux plutôt que sur `/mnt/c` (accès lent).
  - Build publié : clés API uniquement, « OAuth abonnement pour provider » absent ou désactivé (YA-02), auto-approve désactivé par défaut.
- **Dépendances** : YA-10, YA-11, YA-14. **Priorité** : P1. **Complexité** : M.

### YA-17 : Exploitation maximale des capacités du modèle

- **Description** :
  - Utiliser les fonctions natives de chaque provider : raisonnement entrelacé avec les tool calls, signatures de raisonnement conservées, tool calls parallèles, contexte long, vision (captures d'UI pour le front).
  - Utiliser les formats d'édition sur lesquels chaque famille est entraînée (`str_replace`, `apply_patch`) plutôt que des formats maison.
  - **Exception assumée (décision du 28/10/2026)** : bash n'est pas exposé comme tool natif mais comme fonction du kernel, alors que plusieurs familles sont entraînées sur un tool shell. Le prompt système par famille montre l'usage de `await bash(...)` avec des exemples. Si le banc (YA-14) montre une perte de résolution, retour à un tool bash natif pour la famille concernée.
  - Prompts système ajustés par famille de modèle selon les guides officiels des fournisseurs.
  - Mode « effort maximal » pour les tâches difficiles : plan explicite validé, auto-revue du diff par une instance à contexte vierge avant le gate, best-of-N optionnel avec sélection par tests.
  - Aucun plafond arbitraire (sortie, thinking, tours) qui coupe le modèle en plein raisonnement sans budget explicite de l'utilisateur.
  - Paramètres au maximum supporté par défaut : modèle le plus capable disponible pour la boucle principale, effort xhigh par défaut (max disponible au choix, YA-12), max output tokens au plafond du modèle avec relance si la sortie est coupée (YA-01), plus grande fenêtre de contexte disponible.
  - Le plafond de 8 000 tokens par sortie de cellule (YA-03) ne retire aucune information : la sortie complète reste lisible en fichier ou en variable du kernel, le modèle décide de ce qu'il charge.
- **Justification** : l'écart restant entre harness minimal et harness vendeur vient de l'ajustement fin au modèle (arXiv 2606.20683, Terminal-Bench) ; formats entraînés (guide Codex, Aider, Diff-XYZ).
- **Critères d'acceptation** :
  - Matrice des capacités par modèle tenue à jour et testée (thinking, parallélisme, vision, format d'édition).
  - Sur le sous-ensemble difficile du banc, le mode effort maximal atteint (v1) puis dépasse (v2) le harness du fournisseur.
  - Toute fonction native désactivée ou remplacée (dont le tool shell) est justifiée par une mesure.
- **Dépendances** : YA-02, YA-03, YA-14. **Priorité** : P0. **Complexité** : L.

### YA-18 : Checkpoints et annulation

- **Description** : avant chaque tour qui modifie le workspace, snapshot automatique sans toucher à l'historique git de l'utilisateur (ref cachée `refs/yusagent/<session>/<tour>` ou dépôt shadow dans `.yusagent/`). Dans l'extension : bouton « revenir à ce tour » sur chaque message, avec aperçu du diff avant restauration. Restauration du code seul, de la conversation seule, ou des deux. Fonctionne aussi sur les projets sans git. Comme le kernel peut écrire sur le disque sans passer par `edit`, le snapshot couvre toute cellule exécutée, pas seulement les éditions.
- **Justification** : un agent qui part sur une mauvaise piste modifie souvent de nombreux fichiers ; l'annulation rapide rend l'auto-approve utilisable et limite le coût d'une erreur.
- **Critères d'acceptation** :
  - Aucun commit, branche ou stash créé dans le dépôt de l'utilisateur.
  - Restauration d'un tour en < 2 s sur un dépôt de 10 000 fichiers.
  - Les fichiers modifiés par l'utilisateur entre-temps sont signalés avant restauration (pas d'écrasement silencieux).
  - Nettoyage automatique des snapshots de plus de 30 jours.
- **Dépendances** : YA-01, YA-10. **Priorité** : P0. **Complexité** : M.

### YA-19 : Pilotage en cours d'exécution

- **Description** : l'utilisateur peut envoyer un message pendant que l'agent travaille, sans annuler la tâche. Le message est mis en file et injecté en fin de contexte au prochain tour (après les tool results en cours), ce qui préserve le cache. Distinction dans l'UI entre « corriger la direction » (injection) et « stop » (`session/cancel`).
- **Justification** : réorienter tôt évite de gâcher des tours sur une mauvaise piste ; fonction présente dans Claude Code.
- **Critères d'acceptation** :
  - Un message envoyé pendant un tool call est pris en compte au tour suivant, sans interrompre le tool.
  - Le préfixe de cache reste intact après injection (test YA-04).
  - Plusieurs messages en file sont regroupés en un seul bloc.
- **Dépendances** : YA-01, YA-04, YA-10. **Priorité** : P1. **Complexité** : S.

### YA-20 : Client MCP générique

- **Description** : connexion à n'importe quel serveur MCP (stdio et HTTP) déclaré dans `.yusagent/mcp.json` (projet) ou dans la config utilisateur. Les tools MCP sont exposés comme fonctions du kernel Python (filtrage des résultats en code avant d'entrer dans le contexte), avec chargement à la demande des définitions pour ne pas gonfler le préfixe. Permissions par serveur et par tool (YA-11). Les tools yusAi deviennent un serveur MCP comme les autres.
- **Justification** : donne au modèle un contexte réel (tickets, erreurs de prod, BDD, docs) sans code spécifique ; Tool Search d'Anthropic (-85 % de tokens de définitions) et Programmatic Tool Calling (-37 %).
- **Critères d'acceptation** :
  - Au moins 3 serveurs MCP publics testés (ex. GitHub, Postgres, filesystem).
  - L'ajout d'un serveur ne modifie le préfixe qu'en début de session.
  - Les tools d'écriture MCP passent par le flux de permission.
- **Dépendances** : YA-03, YA-09, YA-11. **Priorité** : P1. **Complexité** : M.

### YA-21 : Bascule de provider en cas de panne

- **Description** : sur erreur de saturation ou de quota (429, 529, 5xx répétés), après les retries de YA-02, bascule automatique vers un modèle de capacité équivalente chez un autre provider, défini dans une liste de repli ordonnée. Jamais de repli vers un modèle moins capable sans accord explicite. Notification dans l'UI ; retour au provider principal quand il redevient disponible. L'historique est converti au format du nouveau provider (les signatures de raisonnement non portables sont retirées proprement).
- **Justification** : les quotas « OAuth abonnement pour provider » sont plus serrés que ceux des clés API ; une session longue ne doit pas mourir sur une saturation passagère. Principe qualité d'abord (YA-00) : la bascule ne doit pas dégrader l'intelligence disponible.
- **Critères d'acceptation** :
  - Bascule en < 5 s sans perte de session.
  - Coût de cache froid lié à la bascule visible dans la télémétrie (YA-13).
  - Liste de repli configurable par niveau de capacité, vide par défaut pour les modèles sans équivalent.
- **Dépendances** : YA-02, YA-13, YA-17. **Priorité** : P1. **Complexité** : M.

---

## Audit d'entrée (03/11/2026)

Rapport complet : `docs/audit/audit-entree.md` (binaire de HEAD construit en CI, réseau limité à localhost). Tokens estimés avec `o200k_base`, qui n'est le tokenizer de facturation d'aucun provider.

| Sujet | Constat | Ticket |
|---|---|---|
| Prompt système | environ 7 500 tokens est. (couches statiques 5 636, inventaire des skills 1 854) ; cible < 2 000 | YA-09 |
| Cache dans une session | prompt système et tools identiques octet par octet à chaque tour, après crash, reprise et compaction ; seul `cache_control` se déplace (schéma Anthropic normal) | YA-04 |
| Cache entre sessions | prompt système différent à chaque session : date, cwd, chemin du journal de session | YA-04, YA-09 |
| Breakpoints et TTL | 3 breakpoints automatiques ; TTL 1 h seulement via `PI_CACHE_RETENTION=long` ; `prompt_cache_key` = id de session | YA-04 |
| Secrets | le kernel hérite de tout l'environnement, clés API comprises | YA-03, YA-11 |
| Compaction | seuil 95 % ; premier message utilisateur perdu ; déclenche un appel `/refine` | YA-05 |
| Usage | pas de champ raisonnement ; usage Anthropic mal lu au `message_start` | YA-13 |
| Crash du kernel | redémarrage silencieux, aucune observation injectée | YA-03 |
| Plafond de sortie de cellule | 1 Mio de texte contre 8 000 tokens dans le CDC | YA-03 |
| `rlm()` | sous-agent complet (tous les tools), handle puis `collect` | YA-06 |
| ACP | opérationnel ; manquent `session/request_permission` et `session/load` | YA-10 |
| Télémétrie | active par défaut vers `api.primeintellect.ai` | YA-13 |
| Empreinte | 62 Mo installés hors venv (binaire 59 Mo) | KPI |
| CI du fork | 17 jobs sur 19 ; shard 1 en échec (release v0.9.8 absente) | YA-16 |
| Déjà présent | provider factice `faux`, transcripts ACP, gate « pas de relance sans changement du workspace », hook `transform_context` pour le masquage | YA-05, YA-07 |

---

## Roadmap

| Phase | Contenu | Sortie attendue |
|---|---|---|
| MVP (4 à 6 semaines) | YA-01, YA-02 (2 providers + « OAuth abonnement pour provider »), YA-03 (kernel + `bash()` async + `edit`), YA-04, YA-05 (masquage), YA-07, YA-10 (chat + diff + permissions), YA-11 (permissions), YA-06 (`rlm()` minimal en lecture seule, désactivable, en fin de MVP), YA-13, YA-14 (suite interne + mini-SWE-agent + comparaison kernel seul vs bash natif + A/B avec et sans `rlm()`), YA-17 (formats natifs, raisonnement entrelacé, prompts par famille), YA-18 | Harness utilisable au quotidien dans VS Code ; premier rapport de banc vs mini-SWE-agent ; choix d'exécution confirmé ou révisé |
| v1 (6 à 8 semaines) | YA-02 (providers supplémentaires, « OAuth abonnement pour provider »), YA-05 (compaction LLM), YA-06 (extensions selon le banc), YA-08, YA-09, YA-11 (sandbox OS, bloquant pour la publication), YA-12, YA-14 (vs Claude Code, Codex CLI, OpenCode), YA-17 (mode effort maximal, auto-revue, vision), YA-19, YA-20, YA-21 | Résolution ≥ harness du fournisseur ; rapport de banc public |
| v2 (publication) | YA-02 (clés API, OAuth retiré des builds publiés), YA-13 (vérification coût vs facture), YA-16 (publication Marketplace, Open VSX, distribution du harness), YA-15, YA-17 (best-of-N), boucle Meta-Harness hors ligne, ACP multi-éditeurs, tools MCP yusAi avancés | Publication ; résolution > harness du fournisseur ; article technique |

## Ordre de démarrage du build (proposé, 31/10/2026 ; mis à jour 03/11/2026)

Aucun modèle réel nécessaire pour ces étapes : tout se teste avec le provider factice ou des requêtes interceptées (base URL pointée vers un serveur local).

1. **Fait (03/11).** Repo neuf `YusufByn/yusagent` (public) créé depuis un clone de `PrimeIntellect-ai/prime-agent`, upstream en remote, LICENSE conservée, ajout de `bench/` (banc de capture, `bench/tools/capture/`). `extension/` et l'adaptateur Harbor ne sont pas encore créés (constat du 04/11). CI héritée, réduite à lint + tests avec provider factice.
2. **Fait (03/11), rapport dans `docs/audit/audit-entree.md`.** Audit d'entrée : taille du préambule au premier appel, test de préfixe octet par octet N+1 vs N (YA-04), état du mode ACP, taille sur disque, points d'entrée pour le masquage (YA-05) et le tool `edit` (YA-03).
3. Ticket 01 : désactivations par défaut (Continual Harness, `/refine` automatique, messagerie entre agents, heartbeats, télémétrie distante, vérification de version) et correctif du shard CI 1.
4. Ticket 02 : environnement filtré du kernel et de `bash()` (YA-03, YA-11).
5. Ticket 03 : YA-04, test de préfixe en CI, volatil (date, cwd, chemin du journal) sorti du prompt système, réglage du TTL.
6. Ticket 04 : YA-13 base, journal d'usage par appel, champ raisonnement, correctif de l'usage Anthropic.
7. Ticket 05 : YA-05 masquage par le hook `transform_context`, réglage désactivé par défaut jusqu'au banc.
8. Ensuite : smoke bench (YA-14) avant toute réduction du prompt (YA-09), puis YA-03 (`edit` en tool, `read` numéroté, plafond de sortie, notice de redémarrage), conservation des messages utilisateur à la compaction (YA-05), YA-06, YA-10 (permissions ACP). Si le planning glisse, c'est l'extension qui recule, pas le banc.

## Recommandations

- Commencer par le banc (YA-13, YA-14) en parallèle de la boucle : sans mesure, ni la supériorité de qualité ni le gain de coût ne sont démontrables.
- Forker Prime Agent pour la plomberie, et garder les écarts avec upstream isolés et listés : chaque ajout du CDC est un réglage ou un module, pas une réécriture diffuse. Hard fork avec cherry-pick sélectif (décision du 31/10/2026) : surveiller les releases upstream.
- Choisir le format d'édition et le prompt par famille de modèle.
- Garder la variante « bash natif » prête dans le code (quelques dizaines de lignes) pour pouvoir trancher par le banc sans refonte. Plus généralement, toute variante à tester est un réglage dans la même branche, jamais une branche séparée.
- En cas de conflit entre qualité et coût, la qualité gagne.
- Traiter « OAuth abonnement pour provider » comme strictement personnel et temporaire.

## Limites

- Plusieurs chiffres viennent des éditeurs (Anthropic, Prime Intellect, Manus) et n'ont pas été reproduits indépendamment ; YA-14 doit les vérifier.
- Aucune comparaison publique connue entre « kernel seul avec bash en fonction » et « tool bash natif » à modèle égal sur un benchmark de code. Le choix du 28/10/2026 repose sur Prime Agent (résultats auto-déclarés, hors code) et Programmatic Tool Calling ; YA-14 le confirme ou l'infirme.
- Sans sandbox, le kernel exécute du Python arbitraire que le classifieur de commandes ne voit pas (YA-11).
- Les remises de cache varient selon le modèle et la date ; la table de prix de YA-13 se met à jour depuis la doc officielle de chaque provider.
- Les leaderboards évoluent chaque mois et dépendent des ressources et de la version (Terminal-Bench 2.x, 3.0 et 4.0 non comparables entre eux ; les chiffres 2.1 de l'état de l'art sont historiques).
- Fork : upstream évolue vite ; plus le contexte et le cache sont réécrits, plus les cherry-picks coûtent. Point de friction principal identifié par l'audit : la compaction (`compact_session.rs`, `compaction.rs`), souvent modifiée upstream.
- Sans sandbox, filtrer l'environnement du kernel ne suffit pas à cacher les secrets : un process du même utilisateur peut lire les fichiers d'auth du harness et, sous Linux, `/proc/<pid>/environ` du parent (qui garde l'environnement initial même après `unsetenv`). La protection complète vient du sandbox (YA-11).
- Les KPI sont des hypothèses à recalibrer après le premier rapport de banc.
- Un harness ne peut pas dépasser le plafond du modèle. « 100 % de la capacité » se mesure concrètement par l'écart avec le meilleur harness connu pour ce modèle (en général celui du fournisseur) : l'objectif est de l'égaler puis de le dépasser.

## Sources

- arXiv 2606.20683 (From Question Answering to Task Completion) : https://arxiv.org/pdf/2606.20683
- Anthropic, Infrastructure noise : https://www.anthropic.com/engineering/infrastructure-noise
- SWE-ABS, arXiv 2603.00520 : https://arxiv.org/pdf/2603.00520
- Recursive Language Models, arXiv 2512.24601 : https://arxiv.org/abs/2512.24601
- Reproduction RLM, arXiv 2603.02615 : https://arxiv.org/pdf/2603.02615
- Prime Agent (Prime Intellect) : https://www.primeintellect.ai/blog/prime-agent
- Prime Agent, dépôt : https://github.com/PrimeIntellect-ai/prime-agent
- Manus, Context Engineering for AI Agents : https://medium.com/@peakji/context-engineering-for-ai-agents-lessons-from-building-manus-71883f0a67f2
- Anthropic, tarifs : https://platform.claude.com/docs/en/about-claude/pricing
- OpenRouter, prompt caching : https://openrouter.ai/docs/guides/best-practices/prompt-caching
- The Complexity Trap (JetBrains Research) : https://github.com/JetBrains-Research/the-complexity-trap
- Anthropic, Advanced tool use : https://anthropic.com/engineering/advanced-tool-use
- Anthropic, Programmatic tool calling : https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling
- Aider, unified diffs : https://aider.chat/docs/unified-diffs.html
- Diff-XYZ, arXiv 2510.12487 : https://arxiv.org/html/2510.12487v2
- Codex apply_patch V4A : https://codex.danielvaughan.com/2026/04/28/codex-models-third-party-harnesses-apply-patch-v4a-portable-agent/
- Anthropic, Effective harnesses for long-running agents : https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents
- Anthropic, Effective context engineering : https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- Anthropic, Managed agents : https://www.anthropic.com/engineering/managed-agents
- Agent Client Protocol : https://agentic-ai.readthedocs.io/en/latest/Standards/agent-client-protocol/
- Cline, architecture : https://deepwiki.com/cline/cline/2.3-controller-and-orchestration
- VS Code API : https://code.visualstudio.com/api/references/vscode-api
- VS Code webviews : https://code.visualstudio.com/api/extension-guides/webview
- VS Code Language Model API : https://code.visualstudio.com/api/extension-guides/ai/language-model
- Meta-Harness, arXiv 2603.28052 : https://arxiv.org/pdf/2603.28052
- Terminal-Bench 2.1 (Snorkel) : https://snorkel.ai/leaderboard/terminal-bench-2-1/
- Terminal-Bench 4.0 (annonce) : https://www.tbench.ai/news/terminal-bench-4-0
- Terminal-Bench 4.0 (leaderboard Snorkel) : https://snorkel.ai/leaderboard/terminal-bench-4-0/
- Codex, cache après pause (issue #56050) : https://github.com/openai/codex/issues/56050
- Codex, affichage du taux de cache (issue #43615) : https://github.com/openai/codex/issues/43615
- Claude Code, taux de cache mesuré (Wokey) : https://wokey.ai/claude-code-cache
- Taux de cache par agent de code (Requesty, avril 2026) : https://www.requesty.ai/data/coding-agent-cache-hit-rate-apr-2026
- Coût des reconstructions de cache (kvleak) : https://pypi.org/project/kvleak/
