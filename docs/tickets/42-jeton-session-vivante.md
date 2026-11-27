# Ticket 42 : rafraîchissement d'un jeton dans une session vivante (E-C) : constat et plan

Ticket CDC : YA-02, YA-10. E-C du ticket 40 (`docs/tickets/40-login-extension.md`, §8.2, décision 2 du §10, décision 6 du §15), devenu le ticket suivant quand H1 a été confirmée (§17.1, §18.1, 25/11/2026).

Branche `ya-42-jeton-session-vivante`, depuis `main` à `51bef376c` (merge de la PR #95). S'y ajoutent deux commits :
- le commit de doc `3939e1584` du ticket 41, reporté par cherry-pick (`8284facd4`) ;
- `bb7c662fe` : `bench/.DS_Store` et `bench/smoke/.DS_Store` retirés de l'index, `.DS_Store` ajouté au `.gitignore`.

**Constat et plan seulement** : aucun code, rien dans `docs/fork-changes.md`, rien compilé, aucun appel de modèle. Toutes les lignes citées sont celles de `bb7c662fe`, dont le code est celui de `main` à `51bef376c`.

Sources :
- **Upstream** : `upstream/main` à `32f83edb1` (récupéré le 27/11/2026 ; il était à `6e4b69da2` au ticket 40).
- **Unharnessed** : clone jetable dans le dossier temporaire de la session, à `93b4baf62` (26/11/2026), en lecture seule, rien exécuté. Les chemins de ce dépôt sont préfixés `uh:`.

## Résumé

- **Le problème sur `main`** :
  - le flux vers le modèle lit sa clé dans un emplacement rempli **une fois**, à la construction de la session ;
  - cet emplacement n'est remis à jour que par un changement de modèle, une bascule de fournisseur ou un `Reload` ;
  - un jeton ChatGPT qui expire en cours de conversation reste donc envoyé. Le tour échoue (401, une relance sur le même jeton), et l'extension affiche « Not signed in to ChatGPT, or the sign-in has expired. » avec « Open Providers » ;
  - une reconnexion depuis l'écran Providers répare la conversation (R5). Un simple `Reload` suffirait aussi, sans navigateur, tant que le jeton de rafraîchissement est valide : le `Reload` relit `auth.json` et rafraîchit le jeton expiré au passage (§A.3).
- **Durées de vie** :
  - jeton d'accès ChatGPT : **inconnue**. Aucune source officielle ne la donne, et le code prend `expires_in` dans la réponse du serveur, sans marge ;
  - jeton de rafraîchissement : **inconnue** aussi. La documentation d'OpenAI dit seulement que Codex rafraîchit la session quand `last_refresh` a plus de huit jours environ, que ce jeton **tourne** à chaque rafraîchissement, et qu'un même `auth.json` ne doit pas servir à deux tâches concurrentes (§A.1).
- **Upstream** (`a05bcb695` puis `dad2232cc`) :
  - résout la clé à chaque appel au modèle ;
  - ajoute un verrou de rafraîchissement entre processus ;
  - force le rafraîchissement d'un jeton OAuth refusé par un 401.
  - Taille sur notre arbre : 52 fichiers ; hors tests et docs, 33 fichiers, **+2 152 / −535**.
  - Application textuelle sur `main` : un seul conflit, 2 blocs dans un fichier de tests, venu du fork.
  - Ces commits touchent 10 fichiers que le fork a déjà modifiés. Ils réécrivent le chemin de la clé dans le démon, et nos tests qui construisent un moteur sans l'enregistrer échoueraient (upstream en a corrigé 48 chez lui).
- **Unharnessed** n'a repris ni l'un ni l'autre. La clé reste figée dans l'emplacement. Il se rattrape après coup :
  - jusqu'à 4 relances après un échec d'auth, chacune relisant `auth.json` (`0b2bfbc1`, `dead5334`) ;
  - un verrou de rafraîchissement par fournisseur entre processus ;
  - une relecture après un login, après `providers/reload`, et une fois par heure.
- **Recommandation : option 2**, réimplémentation minimale dans un module du fork :
  - avant chaque appel au modèle, le worker résout de nouveau la clé, comme le fait déjà `Reload`, et la pose dans l'emplacement si `auth.json` sert un identifiant valide ;
  - un verrou entre processus entoure le rafraîchissement d'un jeton expiré ;
  - environ **+4 / −1 lignes upstream** (estimation), aucun lecteur ni écrivain nouveau d'`auth.json` hors du worker, aucun lancement de processus, rien de changé dans ce qui est envoyé au modèle, hormis le jeton lui-même.
- **Tests** :
  - 1a et 1b réactivés ;
  - tests unitaires du module, avec le vrai rafraîchissement Codex sur un transport scripté (`ProviderOAuth::with_http`, `crates/pa-core/src/auth/provider_oauth.rs:54`) ;
  - un test du verrou entre deux processus.

## A. Le problème réel aujourd'hui sur `main`

### A.1 Durées de vie des jetons

| Fournisseur | Jeton d'accès | Jeton de rafraîchissement | Marge avant expiration |
|---|---|---|---|
| ChatGPT / Codex (`openai-codex`) | **inconnue** (non documentée ; le code prend `expires_in` de la réponse) | **inconnue**. Il tourne à chaque rafraîchissement : un `auth.json` partagé entre tâches concurrentes casse le rafraîchissement (doc OpenAI, ci-dessous) | **aucune** : `expires = now + expires_in` (`crates/pa-ai/src/oauth/openai_codex.rs:344`, `:364`) |
| Claude (`anthropic`) | inconnue | inconnue | 5 min (`crates/pa-ai/src/oauth/anthropic.rs:23`, `:254-256`) |
| GitHub Copilot | inconnue (le serveur rend `expires_at`) | inconnue | 5 min (`crates/pa-ai/src/oauth/github_copilot.rs:31`, `:208-219`) |
| xAI | inconnue (1 h par défaut si `expires_in` manque) | inconnue | 5 min, au plus la moitié de la durée de vie (`crates/pa-ai/src/oauth/xai.rs:287-293`, test `:517`) |

- **Source** : documentation d'OpenAI, « Maintain Codex account auth in CI/CD » (https://learn.chatgpt.com/docs/auth/ci-cd-auth, lue le 27/11/2026). Phrases relevées :
  - « if `last_refresh` is older than about 8 days, Codex refreshes the token bundle before the run continues » ;
  - « if a request gets a `401`, Codex also has a built-in refresh-and-retry path » ;
  - parmi les causes d'un rafraîchissement qui échoue : « another machine or concurrent job rotated the token first » ;
  - « Do not share the same file across concurrent jobs or multiple machines ».
  - La page ne donne la durée de vie d'aucun des deux jetons.
- Un billet tiers (classmethod.jp) parle de « quelques dizaines de minutes à une heure » pour le jeton d'accès. Ce n'est **pas une source retenue**.
- **Non mesuré.** Il faudrait lire `expires` (et l'heure d'écriture) dans le `auth.json` de Yusuf, ou décoder `iat` et `exp` du jeton. Je ne l'ai pas fait : c'est le fichier d'identifiants de Yusuf, et la règle 13 m'obligerait à lancer d'abord tout script remis (question 1).
- Unharnessed n'a aucune doc sur ces durées. Ses tests utilisent `expires_in: 3600` (`uh:crates/uh-ai/src/oauth/anthropic.rs:629`).
- Ma seule source pour l'usage de Claude par Yusuf est la règle 7 (login retiré à la main par Yusuf). Le tableau couvre les quatre fournisseurs rafraîchissables (`crates/pa-core/src/auth/provider_oauth.rs:190-198`).

### A.2 Qui rafraîchit, et quand

- **Rafraîchir n'arrive qu'à la lecture**, quand `now >= expires` (`crates/pa-core/src/auth/manager/lookup.rs:65-82`).
  - `refresh_oauth` relit le fichier. Si le jeton est encore valide, il le rend (`:155-183`).
  - Sinon, il prend un verrou **propre au processus** (`refresh_flight`, `crates/pa-core/src/auth/manager.rs:44-62`), relit, rafraîchit, puis écrit sous le verrou du document. Si un autre processus a déjà écrit un jeton plus frais, c'est le sien qui reste (`lookup.rs:184-225`).
  - **Aucun verrou entre processus** autour du rafraîchissement : deux workers peuvent dépenser le même jeton de rafraîchissement.
  - Un échec rend « pas de clé » et garde l'identifiant (`lookup.rs:203-207`, `:79-81`).
- **Lectures qui peuvent rafraîchir**, dans le worker (une `AuthStorage` neuve à chaque fois, `crates/pa-daemon/src/agent_engine/model.rs:455-461`, `:482-491`) :
  - **construction de la session** : `crates/pa-daemon/src/agent_engine/lifecycle.rs:1038-1045`, une seule fois (`crates/pa-daemon/src/agent_engine/turn/run_loop.rs:211-229`) ;
  - changement de modèle et bascule de fournisseur : `session_engine_impl.rs:448`, `turn/model.rs:509`, `:552` ;
  - `Reload` : `crates/pa-daemon/src/agent_engine/reload.rs:85-117` ;
  - compaction, résumé de branche, affinage, fin de tour : `auto_compaction.rs:96`, `overflow_compaction.rs:276`, `compact_autorefine.rs:74`, `turn/boundary.rs:62`, `session_engine_impl.rs:633`, `:720`, `:1258`, `lifecycle.rs:908`.
    - Ces appels résolvent leur clé à chaque fois. **Ils rafraîchissent donc un jeton expiré et l'écrivent dans `auth.json`, mais le flux principal garde l'ancien.**
  - relais MCP et websearch de la PR B : relecture à chaque requête (`crates/pa-core/src/mcp/ya_relay.rs:193`, `:203`).
- **La garde avant un tour ne regarde pas l'expiration.** `has_configured_auth` → `has_auth` → `available_candidate` (`crates/pa-daemon/src/agent_engine/turn/model.rs:46-58`, `crates/pa-core/src/auth/manager.rs:623-625`) : un jeton expiré passe la garde.

### A.3 Conversation ouverte quand le jeton d'accès expire (lu, non mesuré sur un vrai jeton)

1. La garde passe (A.2).
2. La session est déjà construite. Le flux lit l'emplacement (`crates/pa-core/src/session_engine/provider_adapter.rs:110-127`) et envoie l'ancien jeton.
3. Le fournisseur répond 401. C'est le comportement attendu : la doc OpenAI ci-dessus parle d'un 401 que Codex rattrape, mais je ne l'ai pas mesuré.
4. Le 401 est classé `authentication_rejected` (`crates/pa-core/src/session_engine/error_classify.rs:273`). Une relance sur la même cible, puis échec permanent (`crates/pa-core/src/session_engine/provider_retry.rs:152`).
5. Le pont répond `-32603` avec `kind: "authentication"` (`crates/pa-daemon/src/acp/mod.rs:50-59`).
6. L'extension affiche « Not signed in to ChatGPT, or the sign-in has expired. » (`extensions/vscode/src/ya/auth.ts:30-31`) et le bouton « Open Providers » (`extensions/vscode/webview/ya-auth.tsx:13-24`).
7. **Chaque prompt suivant échoue de la même façon.** Il n'y a aucun rafraîchissement forcé, et `mark_auth_stale` n'est appelé qu'en test (ticket 40, §4.2).
8. **Reprise** :
   - **reconnexion par l'écran Providers** : `done` → `_yus/providers/reload` envoyé à toutes les conversations (`extensions/vscode/src/providers.ts:151`, `:234`) → `Reload` du worker → nouvelle résolution → nouveau jeton dans l'emplacement (R5, test 2 vert). Le prompt suivant passe ;
   - **un `Reload` sans login suffirait** : la nouvelle résolution trouve le jeton expiré et le rafraîchit (`lookup.rs:69-70`). L'extension n'envoie `reload` qu'après un `done` (`providers.ts:151`, `:234`). Aujourd'hui, l'utilisateur refait donc un login dans le navigateur alors que le jeton de rafraîchissement est peut-être encore bon ;
   - changer de modèle, ou ouvrir une nouvelle conversation, répare aussi ;
   - un `/login` fait dans le TUI, ou un rafraîchissement fait par un autre worker, **n'atteint pas** la conversation (H1, tests 1a et 1b, rouges sur leur assertion, ticket 40 §18.1).

### A.4 Reproduire sans vrai modèle

- **`bench/tools/sim40.sh` ne le reproduit pas.** Il simule une application textuelle de commits upstream (`git cherry-pick --no-commit`), sans rien compiler ni exécuter. Il resservira pour l'option 1 (B.4).
- **Jeton renouvelé hors de la session** : les tests 1a et 1b (`crates/pa-cli/tests/ya_providers_acp_e2e.rs:477-527`) le reproduisent déjà. Ils utilisent le vrai binaire, un modèle intercepté qui relève `Authorization`, et le fournisseur `ya40` de `models.json`. Ils sont rouges sur `main` (`requests: ["old", "old"]`, ticket 40 §18.1).
- **Jeton expiré que l'agent rafraîchit lui-même** : pas en e2e hors ligne.
  - Les URL de jetons sont en dur (`crates/pa-ai/src/oauth/openai_codex.rs:21`, `anthropic.rs:20`), et seuls les quatre abonnements se rafraîchissent (`provider_oauth.rs:190-198`). `ya40` ne se rafraîchit pas.
  - **Plus petit moyen** : un test unitaire dans `pa-daemon`, avec un moteur dont `auth.json` porte un identifiant `openai-codex` expiré, et `ProviderOAuth::with_http(<transport scripté>)` (`crates/pa-core/src/auth/provider_oauth.rs:54`), la forme des tests de `provider_oauth.rs:213-229`.
  - Le test appelle le flux de la session et relève la clé qu'il envoie. Il passe par le vrai code de rafraîchissement Codex de `pa-ai`, sans réseau.
  - Il demande que la résolution accepte une intégration OAuth injectée (`AuthStorage::create_with_oauth`, `crates/pa-core/src/auth/manager.rs:270-278`) : c'est le cas dans l'option 2 (D.2).

## B. Upstream : `a05bcb695` et `dad2232cc`

### B.1 Ce qu'ils font

**`a05bcb695`** (« live auth refresh (#4019) », 11/11/2026) :
- la clé et les en-têtes sont résolus **à chaque appel au modèle**, par `LiveAuthResolver` / `switchable_stream_fn_with_live_auth` (nouvelle API de `pa-core`, `provider_adapter.rs` +893) ;
- l'emplacement ne garde plus que le modèle et le tier : « drop the write-only auth half of the provider slot » ;
- la résolution tourne sur un fil bloquant, en course avec l'abandon du tour ; aucune requête ne part après un Stop ;
- **verrou de rafraîchissement entre processus**, un fichier par fournisseur (`auth.json.refresh.<hash>.lock`), libéré par le système à la mort du processus ;
- une seule `AuthStorage` par moteur, rechargée à chaque appel ; une lecture ratée n'est pas une déconnexion ;
- compaction, résumé, affinage et fin de tour passent par le même chemin ;
- `Reload` ne touche plus à la clé (`reload.rs` −126 lignes environ) ;
- les tests du démon enregistrent leur moteur comme le fait le worker. Sans cela, chaque appel échoue (« request auth resolution failed ») : 48 tests rouges chez upstream.

**`dad2232cc`** (« force-refresh a rejected OAuth credential… (#4041) », 12/11/2026) :
- sur un 401 avec un jeton OAuth encore valide localement : rafraîchissement forcé sous le verrou, avant la relance d'auth ;
- un rafraîchissement raté termine le tour par une phrase qui nomme le fournisseur, la raison expurgée et `/login`, plus une ligne d'avertissement sur le stderr du worker ;
- une clé d'environnement sert quand le rafraîchissement OAuth échoue ;
- le texte des erreurs d'échange OAuth est expurgé dans `pa-ai` (`anthropic.rs`, `openai_codex.rs`, `utils_inner/log.rs`).

Suite : `6e4b69da2` (25/11/2026, `--api-key` limité à son fournisseur, 13 fichiers, +330/−40). Ce n'est pas une dépendance.

### B.2 Fichiers et lignes (mesuré)

Commande : worktree jetable sur `51bef376c`, `git cherry-pick --no-commit a05bcb695`, résolution des 2 blocs (B.4), commit, puis `git cherry-pick --no-commit dad2232cc`, commit, puis `git diff --numstat 51bef376c HEAD`. Le script est en annexe.

| | Fichiers | Lignes |
|---|---|---|
| Hors tests et docs | 33 | +2 152 / −535 |
| Tests (`tests.rs`, `*_tests.rs`, `tests/`) et `.md` | 19 | +3 683 / −749 |

- `git show --stat` donne : `a05bcb695` 40 fichiers, +4 965/−1 208 ; `dad2232cc` 22 fichiers, +904/−110.
- Couches touchées :
  - `pa-ai` : `lib.rs`, `oauth/anthropic.rs`, `oauth/openai_codex.rs`, `utils_inner/log.rs` ;
  - `pa-core` : `auth/*`, `models/registry.rs`, `platform/lock_dir.rs`, `session_engine/{provider_adapter,request_timing,semantic_edges/stream,auto_retry,provider_failover}.rs`, `mcp/login.rs` ;
  - `pa-daemon` : `agent_engine{,/lifecycle,/model,/reload,/session_engine_impl,/turn/boundary,/turn/model}.rs`, `auto_compaction.rs`, `autonomous_continuation.rs`, `compact_autorefine.rs`, `image_route.rs`, `overflow_compaction.rs`, `session_custom.rs`, `worker.rs` ;
  - `pa-cli` : `lib.rs`, 4 lignes.
- Règle 3 : `pa-ai/src/oauth` et `pa-core/src/auth` sont la couche providers.

### B.3 Dépendances

- Entre le point de fork `c83ac68b3` et `a05bcb695^`, il y a 34 commits upstream. **Aucun ne touche les 40 fichiers de `a05bcb695`** (`git log c83ac68b3..a05bcb695^ -- <fichiers>` : vide). Une dépendance par un symbole d'un autre fichier reste possible : rien n'a été compilé.
- Entre `a05bcb695` et `dad2232cc^`, trois commits touchent les fichiers de `dad2232cc`, et aucun n'est chez nous :
  - `fa6cda7f9` (`models/registry.rs`, accès privé de Prime Inference) ;
  - `34d5d0404` (équipe Prime Inference) ;
  - `5d30e7582` (délai des requêtes OAuth MCP).
  - L'application textuelle de `dad2232cc` passe quand même. Leur effet sur la compilation est inconnu.

### B.4 Conflits probables avec le fork

- **Textuel**, mesuré : `a05bcb695` exit 1, conflit seul dans `crates/pa-daemon/src/agent_engine/tests.rs`, 2 blocs :
  - `mod live_auth;` à côté de nos modules ;
  - `pub(crate)` sur `write_compaction_settings`, à côté du commentaire YA-01.
  - Même résultat qu'au ticket 40 (§11.3). Puis `dad2232cc` exit 0, et `6e4b69da2` exit 0.
- **Fichiers déjà modifiés par le fork et touchés par ces commits** (`docs/fork-changes.md`) :
  - `crates/pa-core/src/session_engine/provider_adapter.rs` (`:51` `cache_retention`, `:97` crochet d'usage) : réécrit à +893 par `a05bcb695`, sur le chemin de nos ajouts T03 et T08 ;
  - `crates/pa-daemon/src/agent_engine.rs` (`:151`, `:1133`) ;
  - `lifecycle.rs` (`:152`, `ya_header_sink`) ;
  - `session_engine_impl.rs` (`:1472`) ;
  - `turn/model.rs` (`:1542`) ;
  - `worker.rs` (`:153`) ;
  - `tests.rs` (`:568`, `:1546`) ;
  - `tests/abort.rs` (`:1543`) ;
  - `crates/pa-ai/src/lib.rs` (`:122`).
- **Ce qui casserait probablement** (déduit, non compilé) :
  - nos tests qui construisent un moteur en ligne sans l'enregistrer : `crates/pa-daemon/src/ya_permissions.rs` et `crates/pa-daemon/src/agent_engine/ya_frozen_tools.rs` appellent `AgentSessionEngine::new`. Upstream a dû enregistrer les siens ;
  - le classement H2 du pont (`auth_failure_kind`, `crates/pa-daemon/src/acp/daemon/ya_providers.rs:264-275`) reconnaît des textes fixes. La phrase de fin de tour de `dad2232cc` n'y est pas : `kind` dépendrait alors du classement du 401 ;
  - le texte d'erreur des flux OAuth change dans `pa-ai`. Notre texte fixe côté pont (ticket 40, décision 5 du §15) n'en dépend pas ;
  - `Reload` ne pose plus la clé. Le test 2 du ticket 40 resterait vert, puisque la clé est résolue à chaque appel.

## C. Unharnessed (`93b4baf62`, 26/11/2026)

- La tête a bougé d'un commit depuis `bb55464`. Ce commit ne touche que `.github/workflows/extension-release.yml` et `README.md`. Rien sur l'auth (`git log bb55464..HEAD` sur `uh-core/src/auth`, `uh-ai/src/oauth`, `uh-daemon`, `session_engine` : vide).
- **Clé figée à la construction**, comme chez nous :
  - `ProviderTarget` : `uh:crates/uh-core/src/session_engine/provider_adapter.rs:89-98` ;
  - `switchable_stream_fn` : `:109-126` ;
  - ni `LiveAuthResolver`, ni `live_auth`, ni `resolve_request_auth`.
- **Ni `a05bcb695` ni `dad2232cc` repris** (recherche de « live auth », « #4019 », « #4041 », « force-refresh » : vide). Unharnessed a ses propres correctifs :
  - `0b2bfbc1` (09/11/2026, « A retry resends with the login stored now », `turn_retry.rs` +32) : avant chaque relance, `auth.json` est relu et la clé de l'emplacement remplacée si elle a changé (`uh:crates/uh-core/src/session_engine/turn_retry.rs:347-385`) ;
  - `dead5334` (« Auth failures retry 4 times… », `provider_retry.rs` +12/−6) : `AUTH_FAILURE_RETRIES = 4`, pauses de 2, 4, 8 et 16 s (`uh:crates/uh-core/src/session_engine/provider_retry.rs:162`, `:182`).
- **Rafraîchissement** seulement à `now >= expires` (`uh:crates/uh-core/src/auth/manager/lookup.rs:66-84`). Pas de rafraîchissement forcé sur 401, et pas de marge pour Codex (`uh:crates/uh-ai/src/oauth/openai_codex.rs:449`).
- **Verrou entre processus** : un fichier par fournisseur, `auth.json.refresh-<provider>.lock`, attente au plus 45 s (`uh:crates/uh-core/src/auth/storage.rs:138-155`, `:324-347`), introduit par `df2919a2` (11/10/2026).
- **Pont** :
  - un login, un logout, `set_api_key` ou `providers/reload` réveillent `watch_models` (`uh:crates/uh-daemon/src/acp/providers.rs:491-494`, `uh:crates/uh-daemon/src/acp/mod.rs:343-378`), qui appelle `refresh_credentials` (`uh:crates/uh-daemon/src/acp/in_process_config.rs:745-768`) ;
  - même chose **une fois par heure** (`mod.rs:354`) ;
  - pas de surveillance de `auth.json`.
- **Notre situation n'est pas la même** (règle du 15/11/2026) : un worker par session et le pont dans un autre processus (ticket 40), contre un démon en processus chez Unharnessed. Ses relances à 30 s cumulées allongent aussi chaque échec d'auth réel, une clé fausse par exemple. Je ne reprends donc pas son choix tel quel ; je retiens son verrou entre processus et l'idée de relire au moment d'envoyer.

## D. Options comparées

### D.1 Option 1 : cherry-pick de `a05bcb695` et `dad2232cc`

- Lignes upstream : hors tests, +2 152/−535 dans 33 fichiers. Tests : +3 683/−749 (B.2).
- C'est le correctif complet d'upstream : résolution à chaque appel, verrou entre processus, rafraîchissement forcé sur 401.
- Ce qui casse : B.4. À compiler et à corriger en CI, d'une taille inconnue. Couche providers touchée en profondeur (règle 3), dont `pa-ai/src/oauth`.
- Tests : `live_auth.rs` (+1 360), `cross_process_refresh.rs` (+677), `reauth_live_e2e.rs` (+444), plus nos tests à adapter. 1a et 1b deviennent verts attendus.
- **Abandon d'un tour** : la résolution est en course avec l'abandon, et une requête interrompue est réglée (`request_timing`, `semantic_edges`). E8 (ticket 25) ne change pas de forme ; un tour lancé par l'agent passe par le même flux.
- **TUI** : passe par le worker, donc couvert. **`-p`** : `crates/pa-cli/src/print_runtime.rs:694` résout une fois et n'est pas touché par ces commits. Une exécution `-p` est courte.
- **Reprises futures** : les commits d'auth d'upstream suivants (`6e4b69da2`, `upstream/auth-401-forced-refresh`, `upstream/fix/auth-reload-on-401-20261122`…) s'appliqueraient plus facilement.

### D.2 Option 2 : réimplémentation minimale dans un module du fork (recommandée)

**Forme** : nouveau fichier `crates/pa-daemon/src/agent_engine/ya_live_auth.rs`.
- **Avant chaque appel au modèle**, le flux de la session :
  1. sur un fil bloquant, en course avec le signal d'abandon de l'appel (`StreamRequestOptions.signal`, `crates/pa-agent/src/stream.rs:150`) ;
  2. si aucune clé n'est imposée par la création (`current_selection().api_key`), il résout de nouveau la clé et les en-têtes du modèle de l'emplacement. C'est la règle de `Reload` (`reload.rs:85-117`) : la clé et les en-têtes ne remplacent la paire de l'emplacement que si `auth.json` sert un identifiant valide. La clé de repli de `models.json` ne remplace jamais une paire ;
  3. puis il appelle le flux d'origine (`switchable_stream_fn`), qui lit l'emplacement mis à jour.
- **Verrou entre processus** autour de la résolution, seulement quand l'identifiant rangé est un OAuth expiré d'un fournisseur rafraîchissable. Le rafraîchissement d'`AuthStorage` relit le fichier sous ce verrou (`lookup.rs:157-183`) : un worker qui attend trouve le jeton écrit par l'autre et ne dépense pas le jeton de rafraîchissement.
  - Fichier : `auth.json.ya-refresh.<fournisseur>.lock`, à côté d'`auth.json`. Il est couvert par le refus Seatbelt `auth.json*` du kernel (`crates/pa-types/src/platform/ya_guard.rs:284`).
  - Le mécanisme exact se choisit au code : `LockDir` de `crates/pa-core/src/platform/lock_dir.rs`, ou un verrou de fichier libéré par le système comme upstream et Unharnessed.
- **Résolution avec intégration OAuth injectable** : `AuthStorage::create_with_oauth` (`crates/pa-core/src/auth/manager.rs:270`), avec `ProviderOAuth::new()` en production et `ProviderOAuth::with_http(<scripté>)` en test.
  - Cela recopie environ 25 lignes de `resolve_request_key_and_headers_and_store_health` (`crates/pa-daemon/src/agent_engine/model.rs:449-480`).
  - L'autre voie est une couture de 3 lignes dans ce fichier upstream (question 4).
- **Moteur non enregistré** (`self_weak` vide, `crates/pa-daemon/src/agent_engine.rs:284`, posé par `autonomous_continuation.rs:76`) : le flux d'origine sert tel quel. Aucun test existant ne change.

**Lignes upstream** (estimation, exactes au plan de code) :

| Fichier | Où | Lignes |
|---|---|---|
| `crates/pa-daemon/src/agent_engine.rs` | déclarations de modules | +2 (commentaire + `mod ya_live_auth;`) |
| `crates/pa-daemon/src/agent_engine/lifecycle.rs` | `:1012` | +2 / −1 (commentaire + `let stream_fn = ya_live_auth::stream_fn(self, switchable_stream_fn(…));`) |

Module du fork : environ 150 à 250 lignes, plus les tests (estimation).

**Ce qui casse** :
- rien d'attendu dans les tests existants (moteurs non enregistrés inchangés) ;
- coût par appel : deux lectures de `auth.json` et une de `models.json` (`model.rs:455-466`), comme un `Reload`. Non mesuré ; à relever au push du code (question 6).

**Ce qui n'est pas couvert, contre l'option 1** :
- pas de rafraîchissement forcé sur un 401 d'un jeton encore valide localement (jeton révoqué côté serveur, horloge décalée) : le tour échoue comme aujourd'hui, puis reconnexion. Question 3 ;
- `-p` : non couvert, comme upstream ;
- compaction, résumé et fin de tour : déjà résolus à chaque appel (A.2), inchangés.

**Effets** :
- **E8 et tours lancés par l'agent** : ils passent par le même flux, donc un réveil après l'expiration rafraîchit au lieu d'échouer. La forme d'E8 ne change pas (file suspendue par l'abandon). Un Stop pendant un rafraîchissement règle l'appel tout de suite et n'envoie aucune requête : à tester (G).
- **TUI** : couvert. Un `/login` du TUI atteint les conversations de l'extension au prompt suivant (H1 corrigée).
- **Pont** : le `Reload` de R5 reste utile pour la liste des modèles (`HostedConfig.models`). Pour la clé, il devient redondant.

### D.3 Option 3 : autres

- **3a, côté extension** : sur un échec `authentication`, envoyer `_yus/providers/reload` puis proposer de renvoyer le message (E-B du ticket 40).
  - Aucune ligne upstream. Un `Reload` rafraîchit un jeton expiré (A.3).
  - Il faut d'abord un tour en échec. Le TUI n'est pas couvert, ni un jeton renouvelé par un autre processus avant l'échec, ni le verrou entre processus.
- **3b, forme d'Unharnessed** : relire `auth.json` avant chaque relance d'auth, et relancer plusieurs fois.
  - Touche `provider_retry.rs` et la relance du tour (couche providers, upstream). Allonge chaque échec d'auth réel.
  - Ne couvre pas le premier appel après l'expiration : il échoue d'abord.
- **3c, rafraîchir avant l'expiration** (minuteur, ou marge pour Codex) : la marge se trouve dans `pa-ai/src/oauth/openai_codex.rs:364` (upstream, couche providers). Un minuteur ajoute un état par session pour un gain que la résolution à chaque appel donne déjà.

### D.4 Tableau

| | Lignes upstream | Jeton expiré | Renouvelé ailleurs (TUI, autre worker) | Verrou entre processus | 401 sur jeton valide localement | Risque de casse |
|---|---|---|---|---|---|---|
| 1 cherry-pick | +2 152/−535 (hors tests) | oui | oui | oui | oui | élevé, inconnu avant compilation (B.4) |
| **2 module du fork** | **+4/−1 (estimation)** | **oui** | **oui** | **oui** | non (question 3) | faible |
| 3a extension | 0 | après un tour en échec | non | non | non | faible |
| 3b relances | quelques lignes dans la relance | après un 401 | après un 401 | non | non | moyen |

**Recommandation : option 2.**
- C'est la plus simple qui tient : elle couvre le jeton expiré, le jeton renouvelé par un autre processus et la concurrence entre workers, avec environ 4 lignes upstream.
- Elle reprend le principe d'upstream (résoudre à chaque appel) et le verrou d'upstream et d'Unharnessed, sans la refonte de l'emplacement.
- L'option 1 reste possible plus tard : si elle est reprise un jour, il suffit de retirer le module et ses 4 lignes.

## E. Protection des secrets (garantie du ticket 41, §S.1)

- **Option 2** :
  - lit et écrit `auth.json` **dans le worker seulement**, par les fonctions qui le font déjà à la construction et au `Reload` (`model.rs:455`, `lookup.rs:211`) ;
  - le fichier de verrou est créé par le worker, et le kernel n'y a pas accès (`ya_guard.rs:284`, regex de préfixe `auth.json`) ;
  - **aucun lancement de processus** : le rafraîchissement passe par un fil et `reqwest` (`provider_oauth.rs:76-178`), pas par `Command::new`. Le scan `ya_process_launch_scan` (`crates/pa-core/tests/ya_process_launch_scan.rs:34-44`) reste vert attendu. Aucune nouvelle crate dans `Cargo.lock`.
  - Messages : aucun texte de flux OAuth n'est recopié. Un échec de rafraîchissement donne « pas de clé », pas un texte (`lookup.rs:203-207`).
- **Option 1** :
  - mêmes lecteurs (le worker) ;
  - le verrou `auth.json.refresh.<hash>.lock` est couvert par la même regex ;
  - ses lancements de processus sont dans des tests (`tests/cross_process_refresh.rs`, enfants de `current_exe()`), que le scan ne parcourt pas (`ya_process_launch_scan.rs:13-14`) ;
  - pas de `Cargo.toml` touché ;
  - la ligne d'avertissement du worker de `dad2232cc` porte « la raison expurgée » : à relire avant reprise (exigence de sécurité du ticket 40, §10).
- **Option 3a** : rien de nouveau. **3b** : mêmes lecteurs.

## F. Préfixe

- Prompt système, tools et `yaRequest` : **inchangés** dans les trois options. Aucune ne touche la construction du prompt, les définitions de tools ni l'en-tête de session.
- **Ce qui part vers le modèle** :
  - option 2 : le corps de requête est inchangé ;
  - l'en-tête `Authorization` porte le jeton frais au lieu du jeton expiré : c'est le but ;
  - les en-têtes résolus (par exemple le compte ChatGPT tiré de l'identifiant) suivent l'identifiant servi, comme après un `Reload` aujourd'hui (`reload.rs:111-115`) ;
  - `prompt_cache_key` reste l'identifiant de session.
- **Option 1** : pareil, plus le « modèle de requête » xAI résolu à chaque appel (forme abonnement ou clé), qui ne concerne que xAI.
- **À prouver au push du code** : capture `main` contre branche (`bench/tools/capture/compare.py --all`), comme au ticket 40 §18.3, et `crates/pa-core/tests/prefix_stability.rs` vert.

## G. Tests (option 2, fournisseur factice ou requêtes interceptées)

| # | Test | Où | Rouge attendu au push 1 (squelette : `stream_fn` rend le flux d'origine) |
|---|---|---|---|
| 1a | réactivé : clé réécrite dans `auth.json` hors du pont → 2e requête `renewed` | `crates/pa-cli/tests/ya_providers_acp_e2e.rs:477` (retrait de `#[ignore]`) | `["old","old"]` |
| 1b | réactivé : jeton OAuth renouvelé sur disque → 2e requête `renewed` | `:503` | `["old","old"]` |
| 9 | jeton `openai-codex` expiré, transport Codex scripté qui rend `tok-new` : l'appel suivant envoie `tok-new`, `auth.json` porte `tok-new`, **un seul** échange de jeton | unitaire `ya_live_auth.rs` | clé envoyée `tok-old`, aucun échange |
| 10 | rafraîchissement raté (transport 400) : la paire de l'emplacement reste (pas de clé de repli à la place) | unitaire | vert dès le push 1 (règle de `Reload`), gardé comme garde-fou |
| 11 | clé imposée par la création : aucune résolution, la clé imposée sert | unitaire | vert dès le push 1, garde-fou |
| 12 | Stop pendant un rafraîchissement bloqué : l'appel se règle en moins de 1 s, aucune requête au modèle | unitaire (transport qui attend) | l'appel attend le transport |
| 13 | deux processus (enfants de `current_exe()` du binaire de test), même `auth.json` avec un jeton expiré, transports scriptés qui comptent dans un fichier : **un seul** échange, les deux finissent avec le même jeton | test d'intégration `pa-daemon` | deux échanges |
| 14 | moteur non enregistré : flux d'origine, aucun changement | unitaire | vert, garde-fou |
| 15 | contrôle de sécurité : le jeton frais n'apparaît ni dans le stderr du worker, ni dans les fichiers de session, ni dans le journal d'usage | e2e (forme du test 8 du ticket 40) | vert attendu, garde-fou |

- Les tests 9 à 12 et 14 n'ont **aucun réseau** : `ProviderOAuth::with_http` (`provider_oauth.rs:54`).
- Le test 13 lance des processus dans un test seulement (non scanné, E).
- Le rafraîchissement réel par `auth.openai.com` n'est pas testé (règle 6).

## Questions pour Yusuf

1. **Durée de vie du jeton ChatGPT** : la laisser « inconnue », ou la mesurer sur ton `auth.json` ? La commande lirait seulement `expires` de `openai-codex` et l'heure de modification du fichier, sans jamais afficher un jeton. La règle 13 me ferait la lancer d'abord, donc lire ton fichier d'identifiants : il faut ton accord explicite.
2. **Option 2 recommandée.** D'accord, ou préfères-tu l'option 1 (cherry-pick complet, couche providers touchée en profondeur) ?
3. **Rafraîchissement forcé sur un 401 d'un jeton encore valide localement** (la partie de `dad2232cc`) : hors de ce ticket (recommandé ; Unharnessed ne l'a pas non plus), ou inclus ? Inclus, il demande de forcer le rafraîchissement, ce que `lookup.rs` ne sait pas faire sans modification : lignes upstream dans `pa-core/src/auth` (règle 3).
4. **Résolution dans le module** : recopier environ 25 lignes de `model.rs:449-480` (aucune ligne upstream de plus, recommandé), ou une couture de 3 lignes dans `model.rs` pour passer l'intégration OAuth ?
5. **Verrou entre processus** dans cette PR (recommandé : la résolution à chaque appel fait rafraîchir toutes les conversations ouvertes au même moment, et OpenAI dit que la rotation par une tâche concurrente casse le rafraîchissement), ou dans une PR à part ?
6. **Coût par appel** (deux lectures d'`auth.json`, une de `models.json`) : le relever au push du code avec une mesure reproductible, et ne garder une `AuthStorage` par moteur que si c'est nécessaire ?
7. **Extension** : après ce ticket, le message « Not signed in… or the sign-in has expired » ne sortira plus que si le rafraîchissement échoue (jeton de rafraîchissement mort ou révoqué). Rien à changer côté extension à mon avis. D'accord ?
8. **`-p`** : non couvert (résolution unique, `print_runtime.rs:694`, comme upstream). On le laisse ?

## Plan de PR (option 2, une PR)

- **Push 1, tests seuls**, rouges sur leur assertion :
  - module squelette (`stream_fn` rend le flux d'origine) et ses 4 lignes upstream ;
  - `#[ignore]` retiré de 1a et 1b ;
  - tests 9 à 15.
  - Relevé de la CI : 1a, 1b, 9, 12 et 13 rouges sur leur assertion ; 10, 11, 14 et 15 verts.
- **Push 2, le code** : résolution à chaque appel, en course avec l'abandon, plus le verrou. Ligne dans `docs/fork-changes.md` (2 fichiers upstream, module, tests) et fragment `.changes/ya-42-jeton-session-vivante.md`.
- **Captures** (règle 13, sur les binaires `main` et branche, avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`) : `run.py`, `compare.py --all`, `acp_frames.py` puis `compare_frames.py`. Attendu : tout identique (F).
- **`secret guard (macos-15)`** vert, scan des lancements compris.
- **Contrôle manuel de Yusuf** : facultatif. Le cas réel (jeton ChatGPT qui expire dans une conversation ouverte) dépend d'une durée inconnue (A.1). Proposition : laisser une conversation ouverte au-delà de l'expiration notée dans `auth.json`, puis envoyer un prompt. Attendu : réponse normale, sans « sign-in expired ».
- Merge sur ton « merge » seulement, CI verte du dernier commit (`gh pr checks`).

## Annexe : simulation de l'option 1 (commande)

```
git fetch upstream
git worktree add --detach "$CLAUDE_JOB_DIR/tmp/wt42" 51bef376c
cd "$CLAUDE_JOB_DIR/tmp/wt42"
g(){ git -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null "$@"; }
g cherry-pick --no-commit a05bcb695; echo "a05 exit=$?"          # exit=1
git diff --name-only --diff-filter=U                            # crates/pa-daemon/src/agent_engine/tests.rs
# résolution des 2 blocs : celle de resolve_a05 dans bench/tools/sim40.sh (marqueur de fin générique)
g add -A && g commit -qm a05
g cherry-pick --no-commit dad2232cc; echo "dad exit=$?"          # exit=0, aucun conflit
g add -A && g commit -qm dad
git diff --numstat 51bef376c HEAD                               # tableau du §B.2 (tri tests/docs par chemin)
g cherry-pick --no-commit 6e4b69da2; echo "6e4 exit=$?"          # exit=0
cd - && git worktree remove --force "$CLAUDE_JOB_DIR/tmp/wt42"
```

Lancée par Claude Code le 27/11/2026. Sorties : `a05 exit=1`, conflit `crates/pa-daemon/src/agent_engine/tests.rs` (blocs aux lignes 15 et 52, 2 blocs résolus, 0 restant) ; `dad exit=0` ; `6e4 exit=0`. Tri du numstat : `test/doc 19 fichiers +3683 -749`, `prod 33 fichiers +2152 -535`.

## H. Décisions de Yusuf (27/11/2026)

1. Durée de vie du jeton ChatGPT : **inconnue**, `auth.json` de Yusuf non lu.
2. **Option 2.**
3. Rafraîchissement forcé sur un 401 d'un jeton encore valide localement : **hors de ce ticket** (limite, §H.1).
4. **Couture de 3 lignes dans `model.rs`** pour passer l'intégration OAuth, pas de copie des 25 lignes : une seule implémentation de la résolution. Total upstream attendu : environ +7 / −1, **lignes exactes annoncées avant le push 2**.
5. **Verrou entre processus dans cette PR.**
6. **Coût par appel**, mesuré au push 2 : médiane et p95 du temps ajouté par appel, sur un nombre d'appels donné, commande au ticket. Règle fixée d'avance :
   - surcoût médian < 10 ms : on garde la résolution à chaque appel ;
   - sinon : une `AuthStorage` par moteur, proposée avant de coder.
7. Extension inchangée.
8. `-p` non couvert (limite, §H.1).

Aussi : `extensions/vscode/.DS_Store` retiré du dépôt (commit à part, `bb7c662fe` étant déjà poussé), et une ligne au journal du CDC pour ces décisions.

### H.1 Limites

- **401 sur un jeton encore valide localement** (jeton révoqué côté serveur, horloge décalée) : pas de rafraîchissement forcé. Le tour échoue comme aujourd'hui (une relance, puis « Not signed in… or the sign-in has expired »), puis reconnexion par l'écran Providers.
  - Upstream le traite dans `dad2232cc` (« force-refresh a rejected OAuth credential and end the turn with re-login guidance (#4041) »).
  - Le reprendre demande de forcer le rafraîchissement, ce que `crates/pa-core/src/auth/manager/lookup.rs:65-82` ne fait qu'à `now >= expires` : il faudrait des lignes upstream dans la couche providers (règle 3).
- **`-p`** : la clé y est résolue une fois au lancement (`crates/pa-cli/src/print_runtime.rs:694`), comme chez upstream après `a05bcb695`. Une exécution `-p` est courte.

### H.2 Lignes upstream : ce qui part au push 1 et ce qui reste pour le push 2

- **Push 1** (squelette branché, sans effet) :
  - `crates/pa-daemon/src/agent_engine.rs` : +2 (commentaire + `mod ya_live_auth;`) ;
  - `crates/pa-daemon/src/agent_engine/lifecycle.rs:1012` : +2 / −1 (le flux de la session passe par `ya_live_auth::stream_fn`).
- **Push 2** : la couture de `model.rs`. Lignes exactes au §I, avant le push 2.

## I. Push 1 : relevé des rouges, et lignes upstream exactes du push 2 (27/11/2026)

### I.1 Push 1 (`f384e4879`, PR #96)

Contenu :
- `crates/pa-daemon/src/agent_engine/ya_live_auth.rs` (nouveau) : squelette branché. `stream_fn` appelle `refresh_slot` avant chaque appel, et `refresh_slot` ne fait rien ;
- `ya_live_auth_tests.rs` (nouveau) : tests 9 à 14 ;
- `crates/pa-cli/tests/ya_providers_acp_e2e.rs` : `#[ignore]` retiré de 1a et 1b, test 15 ajouté ;
- lignes upstream : `agent_engine.rs` +2 (`:81-82`), `agent_engine/lifecycle.rs` +2 (`:1013-1014`).
- `rustfmt --check` lancé en local sur les fichiers touchés (formateur seul, ni cargo ni compilation).

Runs :

| Run | Résultat |
|---|---|
| `ci` 45967819279 | `fmt`, `check + clippy (workspace)`, `windows cross-check + clippy` verts. Shards 1, 2, 5, 6, 7 et 8 verts. **Shard 3/8** : 1 263 réussis, 3 en échec. **Shard 4/8** : dans `ya_providers_acp_e2e`, 8 réussis, 3 en échec. Rien d'autre en échec |
| `ya-secret-guard-macos` 45967819219 | vert : scan des lancements compris |
| `Changelog fragment` 45967819224 | en échec, attendu (fragment au push 2) |
| `ya-extension` 45967819196, `ya-build-macos-arm64` 45967786013 | verts |

**Rouges, chacun sur son assertion** (GitHub masque le jeton factice `FRESH` en `***`, et les valeurs `Bearer …`) :

| Test | Job | Ligne | Assertion |
|---|---|---|---|
| 1a `h1_an_api_key_rewritten_on_disk_reaches_the_running_session` | shard 4/8 | `ya_providers_acp_e2e.rs:493` | « H1: the running session kept the key it was built with; requests: ["old", "old"] » |
| 1b `h1_an_oauth_token_renewed_on_disk_reaches_the_running_session` | shard 4/8 | `:523` | « … kept the token it was built with; requests: ["old", "old"] » |
| 15 `a_renewed_token_is_nowhere_but_auth_json` | shard 4/8 | `:549` | précondition « the renewed token serves the second request; requests: ["old", "old"] » |
| 9 `an_expired_chatgpt_token_is_refreshed_before_the_call` | shard 3/8 | `ya_live_auth_tests.rs:195` | « the next call serves the refreshed token; exchanges: 0 », `left: Some("tok-ya42-old")` |
| 12 `a_stop_during_a_refresh_settles_the_call_at_once` | shard 3/8 | `:278` | « no provider request after the Stop », `left: 1, right: 0` |
| 13 `two_workers_spend_the_refresh_token_once` | shard 3/8 | `:351` | « one token exchange between the two workers; exit codes: [Some(101), Some(101)] », `left: 0, right: 1`. Les deux enfants échouent à `:313` sur `Some("tok-ya42-old")` |

**Verts attendus, verts** : 10 `a_failed_refresh_keeps_the_pair`, 11 `a_pinned_key_is_never_re_resolved`, 14 `an_unregistered_engine_streams_as_before`, et l'enfant du test 13 lancé seul (`a_worker_process_refreshes`, rien à faire hors du test 13).

**Écart au §G, signalé** :
- le test 15 a une précondition (le jeton renouvelé sert la 2e requête), donc il est rouge au push 1, et non vert comme prévu au §G. Sans elle, il passerait sans rien prouver ;
- le test 13 attendait « deux échanges » au push 1 ; le squelette n'en fait aucun, d'où `left: 0`.

### I.2 Lignes upstream exactes (push 1 et push 2)

| Fichier | Ligne | Push | Changement |
|---|---|---|---|
| `crates/pa-daemon/src/agent_engine.rs` | `:81-82` | 1 (fait) | +2 : `// yusAgent (YA-02, YA-10, ticket 42): request auth re-resolved before each provider call.` et `mod ya_live_auth;` |
| `crates/pa-daemon/src/agent_engine/lifecycle.rs` | `:1013-1014`, après `let stream_fn = switchable_stream_fn(…)` (`:1012`) | 1 (fait) | +2 : `// yusAgent (YA-02, YA-10, ticket 42): request auth re-resolved before each call.` et `let stream_fn = super::ya_live_auth::stream_fn(self, stream_fn);` |
| `crates/pa-daemon/src/agent_engine/model.rs` | `:457`, dans `resolve_request_key_and_headers_and_store_health` | 2 | +2 / −1 : `let auth = pa_core::auth::AuthStorage::create(&self.config.agent_dir);` devient `// yusAgent (YA-02, YA-10, ticket 42): the OAuth integration of the refresh (\`ya_live_auth\`).` et `let auth = super::ya_live_auth::auth_storage(&self.config.agent_dir);` |

- **Total : +6 / −1** (attendu : environ +7 / −1).
- **En production, rien ne change pour les autres appelants de cette fonction** (construction, changement de modèle, `Reload`) : `auth_storage(dir)` = `AuthStorage::create_with_oauth(dir, ProviderOAuth::new())`, exactement ce que fait `AuthStorage::create` (`crates/pa-core/src/auth/manager.rs:259-266`). Une intégration scriptée remplace `ProviderOAuth::new()` seulement en test (`#[cfg(test)]`, registre par dossier agent).
- Aucune autre ligne upstream. Le reste est dans le module du fork.

### I.3 Le code du push 2 (module du fork)

`refresh_slot(engine)` :
1. clé imposée par la création (`current_selection().api_key`) : rien ;
2. sinon, modèle de l'emplacement. Si l'identifiant rangé est un OAuth expiré d'un des quatre fournisseurs rafraîchissables (lecture d'`auth.json` sans intégration OAuth, `NoOAuth`) : verrou entre processus d'abord ;
   - fichier `<agent_dir>/auth.json.ya-refresh.<fournisseur>.lock` ;
   - `pa_core::platform::try_lock_exclusive` (verrou du système, libéré à la fermeture ou à la mort du processus, `crates/pa-core/src/platform/lock_dir.rs:489-511`), retenté toutes les 50 ms pendant au plus 30 s ;
   - **verrou non obtenu à temps : rien n'est résolu**, l'emplacement garde sa paire, et l'appel part avec l'ancien jeton (401, comme aujourd'hui) ;
3. sous `reload_lock` (même règle que `Reload`), `resolve_request_key_and_headers_and_store_health(&model)`. Elle rafraîchit le jeton expiré par `lookup.rs`, qui relit le fichier sous le verrou et ne dépense pas le jeton de rafraîchissement si l'autre processus a déjà écrit ;
4. si `auth.json` sert un identifiant valide : clé et en-têtes posés dans l'emplacement, si le modèle n'a pas bougé entre-temps. Sinon, la paire reste (`reload.rs:87-117`).

### I.4 Point à trancher avant le push 2 : un Stop pendant la résolution

- La boucle d'agent abandonne l'appel au flux sur un Stop : `race_with_abort(stream_fn(…), signal)` (`crates/pa-agent/src/agent_loop/response.rs:151`).
- Or notre flux est enveloppé par deux enveloppes de `pa-core`, qui notent le début de la requête **avant** de l'attendre :
  - `semantic_edges` : `request_started` dans le registre (`crates/pa-core/src/session_engine/semantic_edges/stream.rs:21-45`) ;
  - `request_timing` : le résumé (`crates/pa-core/src/session_engine/request_timing.rs:343-403`).
- Si l'appel est abandonné pendant notre attente, la requête reste « commencée » dans le registre, sans `failed`, et son résumé de timing ne part pas.
  - C'est le défaut qu'upstream a corrigé dans `a05bcb695` par des gardes dans ces deux fichiers (« settle requests dropped mid-resolution »).
  - Effet chez nous : le registre n'est lu par rien dans le produit (« nothing in the Rust product reads or derives edges yet », `crates/pa-core/src/session_engine/semantic_edges.rs:10-12`). Le résumé de timing n'est écrit que si `PI_REQUEST_TIMING` ou le réglage est actif. Il manquerait donc une ligne de diagnostic.
- **Proposition** :
  - **Résolution sans rafraîchissement** (le cas de presque tous les appels) : synchrone, dans l'appel au flux, sans `await`. La fenêtre est fermée, et ce sont deux lectures de fichier, comme la résolution que `build_session` fait déjà sur le chemin asynchrone (`lifecycle.rs:1038`).
  - **Rafraîchissement réel** (une fois par durée de vie du jeton) : sur un fil bloquant (`spawn_blocking`), en course avec le Stop. Le test 12 tient : le Stop règle l'appel tout de suite. La fenêtre ne reste ouverte que pendant un rafraîchissement réel, avec l'effet ci-dessus.
  - Aucune ligne upstream de plus. L'autre voie est de reprendre les gardes d'upstream dans `pa-core` (+77 / −10 hors tests dans ces deux fichiers upstream, `git show --numstat a05bcb695`) : je ne la recommande pas.

### I.5 Mesure du coût par appel (décision 6), méthode proposée

- `PI_REQUEST_TIMING=1` écrit pour chaque requête la phase `request-sent` avec `phaseMs`, l'écart entre le prompt construit et la remise de la requête au client du fournisseur (`crates/pa-core/src/session_engine/request_timing/clock.rs:141-167`), dans `<agent_dir>/logs/agent.jsonl` (`request_timing.rs:71`). Notre résolution tombe dans cet écart.
- Commande prévue : un script `bench/tools/capture/ya42_cost.py`. Il lance le binaire `main` puis le binaire de la branche en ACP (pas en `-p`, non couvert), contre le serveur local du banc, avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short PI_REQUEST_TIMING=1`. Il fait N = 200 requêtes par binaire, avec un identifiant `api_key` rangé dans un `auth.json` factice, et rend médiane et p95 de `phaseMs` pour chacun, puis la différence.
- **Règle** : différence des médianes < 10 ms, on garde la résolution à chaque appel.
- Script lancé par Claude Code sur les deux binaires (règle 13), sortie au ticket.

## J. Réponses de Yusuf avant le push 2 (27/11/2026)

1. **Lignes upstream +6 / −1 validées** : `agent_engine.rs:81-82`, `lifecycle.rs:1013-1014`, `model.rs:457`.
2. **Stop** : la proposition du §I.4. Résolution synchrone sans attente dans le cas courant ; un vrai rafraîchissement passe sur un fil à part. Deux ajouts :
   - **test 16** (`a_stopped_refresh_still_serves_the_next_call`) : un Stop pendant un rafraîchissement laisse le fil finir et libérer le verrou, et l'appel suivant envoie le nouveau jeton sans second échange. Le cas est nouveau : il part seul au push 2a, sur le squelette, pour être vu rouge sur son assertion avant le code (push 2b) ;
   - **limite** (§J.1).
3. **Mesure du coût** par `bench/tools/capture/ya42_cost.py`, à deux conditions :
   - chaque requête mesurée passe par la résolution : pas de clé imposée à la création, une entrée dans `auth.json` pour le fournisseur utilisé, et une preuve ;
   - lancements de `main` et de la branche alternés.
   - Règle inchangée : surcoût médian < 10 ms. Médiane, p95 et différence, avec la commande au ticket.

### J.1 Limite : un Stop pendant un vrai rafraîchissement

- Pendant un vrai rafraîchissement (une fois par durée de vie du jeton, et seulement si l'identifiant rangé est un OAuth expiré, ou qui expire dans les 30 s), l'appel au flux attend le fil du rafraîchissement.
- Si un Stop tombe à ce moment, la boucle d'agent abandonne l'appel (`crates/pa-agent/src/agent_loop/response.rs:151`) :
  - la requête reste « commencée » sans fin dans le registre `semantic_edges` (`crates/pa-core/src/session_engine/semantic_edges/stream.rs:21-45`) ;
  - la ligne de timing de cette requête ne part pas (`crates/pa-core/src/session_engine/request_timing.rs:343-403`).
- Le registre n'est lu par rien dans le produit (`semantic_edges.rs:10-12`), et la ligne de timing n'existe qu'avec `PI_REQUEST_TIMING` ou le réglage.
- Upstream corrige ce cas par des gardes dans ces deux fichiers, dans `a05bcb695` (« settle requests dropped mid-resolution », +77 / −10 hors tests). Non repris.
- Dans le cas courant (pas de rafraîchissement), la résolution est synchrone et sans attente : la fenêtre n'existe pas.

### J.2 Preuve que chaque requête mesurée passe par la résolution

- Ni un compteur visible seulement en test, qui n'existe pas dans le binaire mesuré, ni le nombre de lectures d'`auth.json`, qui demanderait `fs_usage` ou `dtruss` en root.
- **Preuve retenue** : avant chaque requête, le script réécrit `auth.json` (dossier de mesure, clé factice) avec une clé neuve, `ya42-<binaire>-<bloc>-<n>`, et relève la clé que le serveur reçoit pour cette requête (en-tête `x-api-key`, format `anthropic-messages`).
  - Sur la branche, chaque requête doit porter la clé écrite juste avant elle. Une requête qui ne serait pas passée par la résolution porterait la clé précédente.
  - Sur `main`, la clé reste celle de la construction de la session (H1), ce qui montre la différence.
- La réécriture fait manquer le cache de lecture d'`auth.json` à chaque appel. La mesure prend donc le cas le plus lent de la lecture, ce qui est prudent.

## K. Push 2a et push 2b (27/11/2026)

### K.1 Push 2a (`f102b9378`) : test 16 seul, sur le squelette

Run `ci` 45971061847 :
- **test 16 rouge sur son assertion** (shard 3/8, `ya_live_auth_tests.rs`) : « the stopped call sends nothing, the next call serves the refreshed token », `left: [Some("tok-ya42-old"), Some("tok-ya42-old")]`, `right: [Some("***")]` (jeton factice masqué par GitHub) ;
- les 6 rouges du push 1 inchangés (1a, 1b, 15 au shard 4/8 ; 9, 12, 13 au shard 3/8) ;
- **shard 2/8** : un échec, `request_timing::payload::tests::concurrent_producers_drop_at_the_reserve_when_the_queue_is_full`, « no storm body may land: every storm producer dropped at the reserve » à `tests.rs:626`.
  - C'est le test listé comme instable dans `docs/fork-changes.md` (« Instabilité probable : `request_timing::payload` »), avec le même symptôme.
  - **Pas de relance** : le run du push 2a est rouge par construction, et le push 2b relance tout. Même cas que la PR #92. Ligne ajoutée au tableau de `fork-changes.md`.

### K.2 Push 2b : le code

- `ya_live_auth.rs` : `refresh_slot`, `resolve_into_slot`, `refresh_provider`, `refresh_lock`, `auth_storage`. Forme du §I.3 et du §J.
- `model.rs:457-458` : la couture validée (+2 / −1).
- `docs/fork-changes.md` : section du ticket 42 ; fragment `.changes/ya-42-jeton-session-vivante.md`.
- `bench/tools/capture/ya42_cost.py` : outil de mesure (§J.2).

**Écarts au §I.3, signalés** :
1. **Chemin synchrone et `reload_lock` déjà pris.** Au lieu d'abandonner la résolution (l'appel serait parti avec l'ancien jeton), l'appel passe par le chemin asynchrone, qui attend le verrou sur un fil à part, en course avec le Stop. C'est le cas d'un appel qui suit de près un rafraîchissement interrompu par un Stop (test 16), ou un `Reload` en cours.
2. **`refresh_provider` lit `auth.json` pour tout fournisseur** avant de regarder s'il est rafraîchissable. Une lecture de plus par appel pour un fournisseur à clé, pour que la mesure (fournisseur à clé `capture`) couvre le même chemin que ChatGPT.
3. **Test 16** : après le second appel, le test attend jusqu'à 2 s que le verrou de fichier soit libre. Le fil du rafraîchissement le lâche juste après l'écriture de l'emplacement.
4. **Verrou de fichier** : `std::fs::File::try_lock` plutôt que `pa_core::platform::try_lock_exclusive`. Il faut distinguer « déjà pris » (attendre) d'« impossible » (système de fichiers sans verrou : on continue sans verrou, comme aujourd'hui), et la fonction de `pa-core` confond les deux dans une `io::Error`.

## L. Push 2b : relevé de la CI, mesure du coût, captures, lignes upstream finales (27/11/2026)

### L.1 CI du push 2b (`30355bb36`)

- Run `ci` 45973386239 : **vert**. `fmt`, `check + clippy (workspace)`, `windows cross-check + clippy` et les 8 shards sont en succès, `test summary` aussi.
- `gh pr checks 96 --required` : **16 checks requis en succès, aucun en « skipping »**. Sont verts aussi : `secret guard (macos-15)` (scan des lancements compris), `Check changelog fragment`, `extension (tsc, lint, tests, vsix, e2e)`, `e2e vrai binaire` et `build-aarch64-apple-darwin`.
- **Tests du ticket, tous verts** (journaux des shards 3/8 et 4/8) :
  - 9 `an_expired_chatgpt_token_is_refreshed_before_the_call` ;
  - 10 `a_failed_refresh_keeps_the_pair` ;
  - 11 `a_pinned_key_is_never_re_resolved` ;
  - 12 `a_stop_during_a_refresh_settles_the_call_at_once` ;
  - 13 `two_workers_spend_the_refresh_token_once` (et son enfant) ;
  - 14 `an_unregistered_engine_streams_as_before` ;
  - 16 `a_stopped_refresh_still_serves_the_next_call` ;
  - 1a, 1b, 15, et le test 2 du ticket 40 (`set_api_key_pushes_the_key_into_the_running_session`).
- Le test instable du shard 2/8 (§K.1) n'est pas retombé.

### L.2 Coût par appel (décision 6)

Binaires (artefacts de `ya-build-macos-arm64`, `shasum -a 256 -c SHA256SUMS` : OK) :
- `main` : run 45961939645, `0.10.2-continuous.51bef376cf9a5b72433d3c03ea51f1470739c986` ;
- branche : run 45973378563, `0.10.2-continuous.30355bb36cf479ee37482e4d1332bb9e8373ef9f`.

Commande, lancée par Claude Code le 27/11/2026 :

```
DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short \
  python3 -I bench/tools/capture/ya42_cost.py --main <main>/yusagent --branch <branche>/yusagent \
    --out <dossier neuf> --blocks 10 --requests 20
```

Sortie (10 blocs de 20 requêtes par binaire, ordre alterné `main-00`, `branch-00`, `branch-01`, `main-01`, … ; chaque bloc finit en `end_turn`) :

| Binaire | Requêtes | Clés conformes | Échantillons `phaseMs` | Médiane (ms) | p95 (ms) | Max (ms) |
|---|---|---|---|---|---|---|
| `main` | 200 | 10 (la 1re de chaque bloc) | 200 | 0,3 | 0,4 | 2,6 |
| branche | 200 | **200** | 200 | 5,55 | 7,2 | 26,9 |

- **Différence des médianes : +5,25 ms**, sous le seuil de 10 ms. **La résolution à chaque appel est gardée** (règle de la décision 6).
- Sans la 1re requête de chaque bloc : `main` 0,3 / 0,4 ms, branche 5,5 / 7,2 ms (médiane / p95). Médianes par bloc de la branche : 6,85 ; 5,7 ; 5,5 ; 6,1 ; 5,5 ; 5,5 ; 5,5 ; 5,6 ; 5,5 ; 5,5.
- **Preuve que chaque requête passe par la résolution** (§J.2) : sur la branche, les 200 requêtes portent la clé écrite dans `auth.json` juste avant elles. Sur `main`, seule la 1re requête de chaque bloc la porte (H1).
- **Limites de la mesure** :
  - `auth.json` change avant chaque requête, donc la lecture rate son cache à chaque appel : c'est le cas le plus lent ;
  - fournisseur à clé (`capture`), sans rafraîchissement réel. Un vrai rafraîchissement, une fois par durée de vie du jeton, ajoute un aller-retour réseau, non mesuré ;
  - une seule machine, celle de Yusuf (macOS arm64), sous `sandbox-exec`.

### L.3 Préfixe (captures, règle 13)

| Contrôle | Commande | Résultat |
|---|---|---|
| Capture `main` | `python3 -I bench/tools/capture/run.py --binary <main>/yusagent` | `runs/20261127T093813Z` ; chemins du préfixe `CONFORME` ; trames de l'extension `TOUT IDENTIQUE` |
| Capture branche | idem sur `<branche>/yusagent` | `runs/20261127T093901Z` ; `CONFORME` ; `TOUT IDENTIQUE` ; `acp-resume` et `acp-live` `CONFORME` |
| Toutes les requêtes, `main` contre branche | `python3 -I bench/tools/capture/compare.py runs/20261127T093813Z runs/20261127T093901Z --all` | **TOUT IDENTIQUE**. Seul `prompt_cache_key` change (UUID de session), comme entre deux runs du même binaire |
| Trames ACP | `acp_frames.py --binary <branche>` puis `<main>`, dossiers neufs | `CONFORME` pour les deux |
| Branche contre `main` | `python3 -I bench/tools/capture/compare_frames.py <branche> --against <main>` | **`écarts : 0`** (A, B et C identiques) |

- Prompt système, tools, `yaRequest` et corps des requêtes : **inchangés**. Ce qui change est l'en-tête d'auth, quand `auth.json` porte un identifiant neuf : c'est le but.
- **Écart de procédure, signalé** : un premier lancement d'`acp_frames.py` a refusé deux dossiers de sortie qui existaient déjà dans le dossier temporaire de la session (restes antérieurs). La comparaison faite sur ces restes (`écarts : 1`, `token` de `_yus/mcp/list`, d'avant le ticket 40) ne compte pas. Relancé avec des dossiers neufs (ligne ci-dessus).

### L.4 Lignes upstream finales

Mesure : `git diff --numstat origin/main...HEAD`, restreint aux fichiers présents dans `upstream/main`.

| Fichier | Lignes | Où |
|---|---|---|
| `crates/pa-daemon/src/agent_engine.rs` | +2 | `:81-82` |
| `crates/pa-daemon/src/agent_engine/lifecycle.rs` | +2 | `:1013-1014` |
| `crates/pa-daemon/src/agent_engine/model.rs` | +2 / −1 | `:457-458` |
| `.gitignore` | +3 | ligne vide, commentaire, `.DS_Store` (hors code) |

- **Code : +6 / −1**, comme validé au §J.
- `docs/fork-changes.md` disait +2 pour `.gitignore` : corrigé en +3.
- Aucun autre fichier upstream touché dans la PR.
