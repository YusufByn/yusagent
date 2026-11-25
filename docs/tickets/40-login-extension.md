# Ticket 40 : login dans l'extension VS Code (constat et plan)

Demande de Yusuf du 25/11/2026. Étape 5 du découpage du ticket 24 (`docs/tickets/24-ya10-extension-minimal.md:724`, « Login dans l'extension : ticket à part, constat d'abord. Règle 3 […] et règle 7 ») ; écart au §4 du ticket 24 porté au journal du CDC (`docs/yusAgent-cdc.md:94`) ; reprise de la section du catalogue MCP (C4, `docs/tickets/31-skills-mcp-reglages.md:697`) ; noms `_yus/providers/*` fixés au ticket 32 (`docs/tickets/32-identite-yusagent.md:192`). CDC YA-10 : « Clés dans `SecretStorage`, callback OAuth via `registerUriHandler` » (`docs/yusAgent-cdc.md:393`).

Branche `ya-40-constat`, depuis `main` à `1ece0ff04` (merge de la PR #90), plus les commits de doc du contrôle du ticket 39 (§24) reportés de `ya-39-doc-a-reporter`. **Constat et plan seulement** : aucun code, rien dans `docs/fork-changes.md`, rien compilé, aucun appel de modèle.

Toutes les lignes citées sont celles de `main` à `1ece0ff04`, sauf mention contraire. Unharnessed : clone jetable de `github.com/unharnessed-agent/Unharnessed` à `5494a33a5f6773d74f8affdbaedb27ce6e1f6869` (25/11/2026), dans le dossier temporaire de la session, en lecture seule, rien exécuté. Entre `157dcdc1d` (base de notre import) et ce commit, `vscode/`, `crates/uh-daemon` et `crates/uh-ai` ne changent que par une ligne de `vscode/package.json` (`git diff --stat 157dcdc1d 5494a33a5 -- vscode crates/uh-daemon crates/uh-ai`). Ses chemins sont préfixés `uh:`. Upstream : `upstream/main` à `6e4b69da2` (25/11/2026), point de fork `c83ac68b3` (02/11/2026).

## Résumé

- **Le cœur sait se connecter, mais seulement depuis le TUI.** Les flux OAuth sont dans `pa-ai`, indépendants de l'interface (Claude, ChatGPT/Codex, Copilot, xAI, Prime Inference, serveurs MCP). Les fonctions qui les lancent et rangent le résultat sont dans `pa-cli`, liées au panneau du TUI (`pa_tui::auth_panel::AuthPanelHandle`). Le pont ACP vit dans `pa-daemon`, qui ne dépend ni de `pa-cli` ni de `pa-tui` : **il ne peut pas les appeler tel quel** (§1).
- **Identifiants** : `~/.yusagent/auth.json`, fichier 0600 sous verrou, une entrée par fournisseur (`api_key`, `oauth`, `mcp_static_token`), MCP sous `mcp:<serveur>` (§1.4).
- **Le pont ne sert aucune des 13 méthodes `_yus/providers/*`** ; l'extension les a déjà toutes, reprises d'Unharnessed, avec les écrans Providers et SignIn. Le bouton Providers est caché, la section du catalogue MCP aussi (C4), et le formulaire de jeton MCP ne s'ouvre jamais (§2).
- **Unharnessed a tranché la plupart des choix** : méthodes `_unharnessed/providers/*` (pas `authMethods`/`authenticate` d'ACP), identifiants dans `auth.json` (pas de `SecretStorage`), pas de `registerUriHandler`, navigateur ouvert seulement sur clic, un login à la fois, session vivante remise à jour après un login (§3).
- **Point dur, indépendant de l'extension** : la clé d'une session est capturée **une fois**, à la construction de la session. Ni le pont ni le TUI n'envoient `Reload` après un login, et un jeton OAuth n'est rafraîchi qu'à une nouvelle lecture. Un `/login` fait ailleurs ne répare donc pas une session ACP déjà construite (lu, non mesuré). Upstream a corrigé ce défaut après notre point de fork (`a05bcb695`, `dad2232cc`), sans que ces commits soient chez nous (§4).
- **Aujourd'hui, sur un échec d'auth**, l'extension affiche « Authentification requise : lancez <chemin> dans un terminal, puis /login. » (§4.3).
- **Préfixe** : un login ne change ni le prompt système ni les tools. Il peut changer la liste des modèles proposés. Connecter un service MCP du catalogue fait partir les annonces `[mcp-changed]` et `[skills-changed]` du ticket 31 en fin d'historique (§6).

## 1. Ce que le cœur offre

### 1.1 Les flux de connexion (`pa-ai`, `pa-core`)

| Fournisseur (id) | Entrée | Comment le code arrive | Ce qui est écrit |
|---|---|---|---|
| Claude Pro/Max (`anthropic`) | `login_anthropic`, `crates/pa-ai/src/oauth/anthropic.rs:49` | PKCE ; rappel local sur le port fixe **53692** (`crates/pa-ai/src/oauth/anthropic_callback.rs:15`, `:18`), en course avec un collage du code ; un port déjà pris est une erreur (`:70-79`). Le vérifieur PKCE sert de `state` (`anthropic.rs:4`, `:120`) | `oauth {access, refresh, expires}` |
| ChatGPT / Codex (`openai-codex`) | `login_openai_codex`, `crates/pa-ai/src/oauth/openai_codex.rs:69` | PKCE ; rappel sur le port fixe **1455** (`crates/pa-ai/src/oauth/callback.rs:15`, `openai_codex.rs:23`) ; port pris : serveur « mort », collage seul (`callback.rs:66-73`) | `oauth {…, accountId}` |
| GitHub Copilot (`github-copilot`) | `login_github_copilot`, `crates/pa-ai/src/oauth/github_copilot.rs:117` | domaine Enterprise demandé (`:125-131`), puis code d'appareil (`:144-149`) | `oauth {…, enterpriseUrl}` |
| xAI (`xai`) | `login_xai`, `crates/pa-ai/src/oauth/xai.rs:53` | code d'appareil (`:17`, `:100`) | `oauth {…}` |
| Prime Inference (`prime-inference`) | `login_prime_inference`, `crates/pa-core/src/auth/prime_inference_login.rs:69` | identifiants de prime-cli, sinon défi dans le navigateur avec attente (`:78-125`) | `api_key {key, primeTeam}` |
| Serveur MCP (`mcp:<serveur>`) | `mcp_login`, `crates/pa-core/src/mcp/oauth.rs:136` ; `McpLoginContext::run`, `crates/pa-core/src/mcp/login.rs:31-43` | rappel sur les ports 53700 à 53709 (`crates/pa-core/src/mcp/oauth_callback.rs:20-21`) | `oauth {…, endpoint, tokenEndpoint, clientId…}` sous `mcp:<serveur>` |

- Interface commune des flux : `OAuthLoginUi` (`on_auth(url, instructions)`, `on_prompt`, `on_progress`, `on_manual_code_input`, `is_cancelled` ; `crates/pa-ai/src/oauth/types.rs:17-36`) ; Codex a la sienne, `CodexLoginUi` (`openai_codex.rs:47-60`).
- **Les flux `pa-ai` n'ouvrent jamais le navigateur eux-mêmes** : l'interface le fait dans `on_auth`, par `pa_core::platform::browser::open_in_browser` (`crates/pa-core/src/platform/browser.rs:34-42`, `open` sous macOS). Exemple : `crates/pa-cli/src/subscription_login.rs:44-46`.
- **Pas d'OAuth Gemini** : `google` passe par une clé (`GEMINI_API_KEY`, `crates/pa-ai/src/env_api_keys.rs:17`).
- **Clés par variable d'environnement** : table `get_api_key_env_vars` (`crates/pa-ai/src/env_api_keys.rs:6-43`).

### 1.2 Les fonctions qui lancent un login et rangent le résultat (`pa-cli`, liées au TUI)

- `ProviderAuth` (`crates/pa-cli/src/provider_login.rs:189-204`) implémente le trait du TUI `ProviderAuthCommands` (`crates/pa-tui/src/provider_auth.rs:86-106`) : `login_options`, `logout_options`, `login(row, api_key)`, `login_on_panel(row, AuthPanelHandle)`, `logout`, `anthropic_subscription_warning`.
- Lignes proposées : les 4 abonnements (`SUBSCRIPTION_PROVIDERS`, `provider_login.rs:19-24`), puis un fournisseur à clé par fournisseur connu ou personnalisé du registre (`login_rows_blocking`, `:325-405`).
- Clé : `login_blocking` (`:458-488`) → `AuthStorage::create(agent_dir).set(id, ApiKey {…})`. Déconnexion : `logout_blocking` (`:652-671`) ; ne touche ni l'environnement ni `models.json`.
- Abonnements : `run_anthropic_login` (`subscription_login.rs:109`, écriture `:141`), `run_codex_subscription_login` (`codex_subscription_login.rs:64`, écriture `:86`), aiguillage `login_blocking_on_panel` (`provider_login.rs:492-650`).
- **Liées au TUI** : ces fichiers importent `pa_tui::auth_panel::AuthPanelHandle` (`subscription_login.rs:18`, `codex_subscription_login.rs:15`). `AuthPanelHandle` est une structure qui envoie des `AuthPanelRequest` sur un canal (`crates/pa-tui/src/auth_panel.rs:81-130`, `:182-248`) : `Progress`, `Waiting`, `AuthUrl`, `PastePrompt` (avec réponse), `SelectTeam` (avec réponse), `ProviderSettled`.
- **Dépendances** : `pa-daemon` dépend de `pa-types`, `pa-core`, `pa-agent`, `pa-ai`, `pa-telemetry`, `pa-models` (`crates/pa-daemon/Cargo.toml:9-13`, `:40`) ; `pa-cli` dépend de `pa-daemon` et de `pa-tui` (`crates/pa-cli/Cargo.toml:13-14`) ; `pa-tui` ne dépend que de `pa-types` (`crates/pa-tui/Cargo.toml:9`). Le mode ACP tourne dans le binaire `pa-cli` (`crates/pa-cli/src/print_runtime.rs:62`, `:85-93`), mais le code du pont est dans `pa-daemon`.

### 1.3 Le TUI

- `/login` et `/logout` : `crates/pa-tui/src/session_ui/commands.rs:286`, `:297` → `open_provider_auth` (`crates/pa-tui/src/session_ui/auth.rs:34-66`).
- Clé : la ligne passe en saisie « Enter API key: » (`crates/pa-tui/src/provider_auth.rs:354-361`, `:416-434`), puis `login(provider, Some(api_key))` (`session_ui/auth.rs:101-104`).
- Le login s'exécute **dans le processus du TUI**, qui écrit `auth.json` lui-même, pas par le démon (`crates/pa-cli/src/interactive_mode.rs:476-478`).
- Après un login, le TUI relit seulement le catalogue de modèles (`session_ui/auth.rs:222-248`, `session_ui/model_picker.rs:182-192`). **Il n'envoie pas `Reload`.**

### 1.4 Où sont rangés les identifiants

- `<agent_dir>/auth.json`, `agent_dir` = `$PRIME_AGENT_CODING_AGENT_DIR` sinon `~/.yusagent` (`crates/pa-types/src/platform/dirs.rs:35-49`).
- `AuthStorage` : `crates/pa-core/src/auth/manager.rs:222` (fichier vu comme binaire par `grep`, lire avec `grep -a`). `set` `:695`, `remove` et `logout` `:699-703`, écriture sous verrou puis relecture `:707-727`, `get_auth_status` sans valeur secrète `:628` (`AuthStatus`, `crates/pa-core/src/auth/types.rs:178-184`).
- Types : `AuthCredential` (`crates/pa-core/src/auth/types.rs:51-115`).
- Droits : fichier 0600, dossier 0700 (`crates/pa-core/src/platform/perms.rs:9`, `:11`), création exclusive (`crates/pa-core/src/auth/storage.rs:58-84`), écriture atomique (`crates/pa-core/src/settings/storage.rs:290`, `:295`).
- Ordre de résolution d'une clé : surcharge `--api-key`, identifiant rangé, environnement, `models.json` (`crates/pa-core/src/auth/manager/lookup.rs:17-133`).
- Rafraîchissement OAuth : **seulement quand `now >= expires`**, à la lecture (`lookup.rs:65-81`). Un échec garde l'identifiant et ne rend pas de clé (`:79-81`). Seuls `openai-codex`, `anthropic`, `github-copilot`, `xai` sont rafraîchis (`crates/pa-core/src/auth/provider_oauth.rs:190-198`).
- Commandes du démon : `GetMcpConnections`, `SetMcpStaticToken`, `RemoveMcpConnection`, `GetModelCatalog`, `GetAvailableModels`, `SetModel`, `Reload` (`crates/pa-types/src/daemon/command.rs:731`, `:740-758`, `:768`, `:775`, `:905`, `:1047`). **Aucune commande Login, SetApiKey, ListProviders ou Logout.**

## 2. Le pont et l'extension aujourd'hui

### 2.1 Le pont

- `initialize` n'annonce aucun `authMethods` (`crates/pa-daemon/src/acp/types.rs:12-18`, `:410-435`) ; `authenticate` reçoit `-32601`, comme toute méthode inconnue (`crates/pa-daemon/src/acp/daemon.rs:1121-1178`).
- Méthodes `_yus/*` servies : `_yus/sessions/delete`, `_yus/session/steer`, `_yus/settings/*`, `_yus/skills/*`, `_yus/mcp/{list,set_token,logout}` (`crates/pa-daemon/src/acp/ya_resources.rs:28-35`). **Aucune `_yus/providers/*`.**
- `_yus/mcp/list` ne donne jamais de champ `token` pour un service (`ya_resources.rs:441-448`), d'où le formulaire de jeton jamais ouvert (§2.2).
- Aucune notification liée à l'auth.

### 2.2 L'extension

- **13 méthodes, reprises d'Unharnessed, non servies** (`extensions/vscode/src/providers.ts:8-20`) : `list`, `reload`, `set_api_key`, `logout`, `login`, `login_answer`, `login_open`, `local_scan`, `local_probe`, `local_save`, `local_remove` (requêtes), `login_cancel` (notification envoyée), et `login_update` (notification reçue, écoutée en `:47`).
- **Rejet sur place** : `served()` ne les contient pas (`extensions/vscode/src/ya/methods.ts:10-33`). `request` les rejette sans trame (`extensions/vscode/src/agent.ts:97`), `notify` les ignore (`:105-108`).
  - À chaque démarrage prêt, `Providers.refresh` appelle `list` (`providers.ts:55`, `:74`), est rejeté et journalise `providers: list failed: … méthode pas encore servie par yusAgent` (`:79`).
- **Écran Providers** : bouton caché par `hidden={!features.providers}` (`extensions/vscode/webview/Status.tsx:81-82`), avec `features.providers = served("_yus/providers/list")` (`methods.ts:50`).
- **Écran SignIn** : pas masqué par un drapeau. Il s'affiche dès qu'un login est en cours ou a échoué (`extensions/vscode/webview/App.tsx:167`, `:182-183`). Rien ne le déclenche aujourd'hui, puisque Providers et le catalogue sont cachés.
  - S'il était lancé, `startLogin` poserait l'état puis recevrait le rejet, et SignIn afficherait l'erreur (`providers.ts:138`, `:150-155`).
- **Puce « Connect a provider »** : le compositeur l'affiche quand aucun fournisseur n'est connecté (`extensions/vscode/webview/Composer.tsx:970-976`). `unconnected` exige une liste reçue (`App.tsx:163`) ; la liste reste `null`, donc la puce n'apparaît pas.
- **Catalogue MCP (C4)** : `const CATALOG = false` (`extensions/vscode/webview/SettingsMcp.tsx:12-15`, rendu `:137`).
  - Un service du catalogue se connecte par `mcpSignIn` → `startLogin("mcp:<id>")` (`SettingsMcp.tsx:243`, `extensions/vscode/src/view.ts:223-224`, `:347-351`), c'est-à-dire par `_yus/providers/login`.
  - Le formulaire de jeton ne s'ouvre que pour un service qui a un champ `token` (`SettingsMcp.tsx:239-263`), que le pont ne donne pas (§2.1). Le chemin `mcpSetToken` → `_yus/mcp/set_token` existe et est servi (`view.ts:226-229`, `extensions/vscode/src/settings.ts:119-131`).
- **Après un login**, l'extension prévient les autres conversations par `broadcast(RELOAD)` (`providers.ts:149`, `:232` ; `extensions/vscode/src/shown.ts:73-78`).
- **Ouverture de la page** : `openPage` envoie `login_open` (le pont ouvre le navigateur de sa machine), ou appelle `vscode.env.openExternal` si VS Code est distant (`providers.ts:181-193`).
- **Écran « local »** (`webview/Local.tsx`) : passe par `local_*`. Notre cœur n'a pas de détection de serveur local : `11434` n'apparaît que dans des tests de `crates/pa-core/src/models/custom.rs:492`, `:563`. Chez Unharnessed, c'est un module à lui, `uh:crates/uh-core/src/models/local.rs` (681 lignes).

## 3. Unharnessed

### 3.1 Le pont (`uh:crates/uh-daemon/src/acp/providers.rs`, 625 lignes, introduit par `4f1a40f6` du 02/10/2026)

- **Aiguillage** : les requêtes passent d'abord par les fournisseurs (`uh:crates/uh-daemon/src/acp/mod.rs:419-425`). Contrat en tête du fichier (`providers.rs:1-39`), noms `:62-74`, routeur `:184-219`.
- **`list`** : `{providers: [{id, name, method: subscription|apiKey, login: browser|apiKey, connected, removable, note?, warning?, local?}]}` (`:137-165`, `:508-572`).
  - Une clé venue de l'environnement est connectée mais non supprimable, avec la note `env: OPENAI_API_KEY` (`:144-149`).
  - La ligne Claude par abonnement porte un avertissement de risque de bannissement (`:559-570` ; `uh:crates/uh-cli/src/provider_login.rs:230-240`).
- **Réponses de `set_api_key`, `logout` et `login`** : toutes en `{providers, outcome: done|failed|cancelled, message?}` (`:491-505`).
- **`login`** :
  - Il répond quand le flux se règle (`:199-205`, `:283-321`).
  - **Un seul login à la fois** (`"a login is already running"`, `:288-291`).
  - Le navigateur ne s'ouvre jamais de lui-même (`:29-31`).
- **`login_update {providerId, update}`** (`:332-388`) : `progress`, `waiting`, `url {url, instructions}`, `prompt {promptId, message, secret, allowEmpty, optional}`, `team`.
- **`login_answer`** (`:415-479`), **`login_open`**, qui n'ouvre que l'URL du login en cours et jamais une URL du client (`:402-413`), **`login_cancel`** (`:223-227`). Une déconnexion du client annule aussi le login (`mod.rs:288`).
- **Rangement** : `auth.json` en 0600 (`uh:crates/uh-core/src/auth/storage.rs:98`, `:329`). Une clé de serveur local va dans `models.json` (`providers.rs:522-524`).
- **Comment il appelle les flux** : `4f1a40f6` a **déplacé** `auth_panel.rs` et `provider_auth.rs` de `uh-cli` vers `uh-daemon`.
  - Le trait `ProviderAuthCommands` (`uh:crates/uh-daemon/src/provider_auth.rs:101`) est implémenté dans `uh-cli` (`uh:crates/uh-cli/src/provider_login.rs:243`) et injecté au lancement du mode ACP (4 lignes de `uh-cli/src/print_runtime.rs`).
  - Chez nous, ces deux fichiers sont dans `pa-tui` (§1.2).
- **Après un login réussi** : le signal `auth_changes` (`providers.rs:492-494`) réveille `watch_models` (`uh:crates/uh-daemon/src/acp/mod.rs:343-379`).
  - **La clé de la session vivante est remplacée** (`refresh_credentials`, `uh:crates/uh-daemon/src/acp/in_process_config.rs:745-768`).
  - **La liste des modèles est relue**, puis `config_option_update` part si elle change (`in_process_config.rs:681-741`, `:589` ; `uh:crates/uh-daemon/src/acp/config_options.rs:249-272`).
  - Si le modèle courant n'est plus disponible, il passe au modèle par défaut (`in_process_config.rs:731-737`).
  - Son mode ACP tourne dans le processus de l'agent, sans démon (ticket 24, `docs/tickets/24-ya10-extension-minimal.md:550`).
- **Un serveur MCP connecté** n'est pris qu'au redémarrage (`uh:crates/uh-daemon/src/acp/mcp_servers.rs:23-24`).
- **`0b2bfbc1`** (09/11/2026) : avant une relance, l'agent relit l'identifiant rangé, parce qu'un rafraîchissement par un autre processus qui partage `auth.json` révoque le jeton en cours.

### 3.2 Choix déjà tranchés par Unharnessed

| Sujet | Unharnessed | Preuve |
|---|---|---|
| Protocole | méthodes d'extension `_unharnessed/providers/*`, ni `authMethods` ni `authenticate` | `uh:crates/uh-daemon/src/acp/types.rs:425-447` |
| Rangement des secrets | `auth.json` (0600), pas de `SecretStorage` (`git grep SecretStorage context.secrets` vide) | §3.1 |
| Rappel OAuth | serveur local ou code collé, **pas de `registerUriHandler`** ni `vscode://` (grep vide) | `uh:crates/uh-ai/src/oauth/anthropic_callback.rs:26` |
| Navigateur | ouvert seulement sur clic : `login_open` en local, `openExternal` à distance (la boîte « ouvrir un site externe ? » de VS Code) | `4f1a40f6` ; `uh:vscode/src/providers.ts:181-193` |
| Concurrence | un login à la fois | `providers.rs:288-291` |
| Session vivante | clé remplacée, modèles relus, `config_option_update` | §3.1 |
| Échec d'auth d'un prompt | aucun traitement : `data.details` affiché tel quel ; SignIn ne s'ouvre pas sur erreur (grep `isAuth`, `401`, `unauthori` vide dans `vscode/`) | `uh:vscode/src/agent.ts:249-257` |
| Journal de l'extension | **chaque trame**, avec seulement `set_api_key.apiKey` et `login_answer.answer` masqués (`uh:vscode/src/agent.ts:19-23`, `:199-219`). En clair : le jeton de `mcp/set_token`, l'`apiKey` de `local_probe` et `local_save`, l'URL de `login_update`, dont le `state` est le vérifieur PKCE pour Claude | `uh:vscode/src/settings.ts:149`, `uh:vscode/src/providers.ts:107`, `:123` |
| Claude par abonnement | proposé, avec avertissement | `uh:crates/uh-cli/src/provider_login.rs:230-240` |
| Serveurs locaux | `local_*`, sonde de n'importe quelle adresse donnée par le client | `uh:crates/uh-core/src/models/local.rs:28-36`, `:146-200` |

## 4. Erreur d'authentification et connexion expirée

### 4.1 Classement dans le pont

- Un prompt en échec répond `-32603` avec `data {details, kind, category, retryable}` (`turn_failure_error`, `crates/pa-daemon/src/acp/mod.rs:50-76`). `kind` vient du classifieur (`crates/pa-core/src/session_engine/error_classify.rs`), dans cet ordre :
  1. statut HTTP : 401 et 403 donnent `authentication_rejected` (`:271-274`) ;
  2. code : `invalid_api_key`, `invalid_token`, `invalid_grant` (`credential_invalid`), `token_expired`, `expired_token` (`credential_expired`), `missing_api_key`, `authentication_error`, `unauthorized` (`:126-137`) ;
  3. puis préfixes de transport, puis message exact (`:312-345`).
- Ces sous-types donnent la catégorie `authentication` (`:188-192`).
- **Identifiant absent** : le texte « No API key found for <fournisseur>. Use /login … » (`crates/pa-daemon/src/agent_engine/turn/model.rs:67-72`) n'est reconnu par aucune règle. Il sort en `kind: "other"`, mesuré au ticket 24 (`docs/tickets/24-ya10-extension-minimal.md:497-501`).
- **Autre texte de la même garde** : « Authentication failed for "<fournisseur>". Credentials may have expired… Run /login to update credentials. » (`turn/model.rs:60-64`), quand l'identifiant rangé est OAuth.
  - Lu, non mesuré : aucune règle ne le reconnaît non plus, donc `other`.
  - Il ne contient pas « No API key found for », donc `isAuthFailure` serait faux.
  - Quand cette branche se déclenche n'est pas établi : `has_auth` ne regarde pas l'expiration (`crates/pa-core/src/auth/manager.rs:623-625`).

### 4.2 Clé capturée à la construction de la session (lu, non mesuré)

- Le flux vers le modèle lit sa clé dans un emplacement partagé, `ProviderTarget` (`crates/pa-core/src/session_engine/provider_adapter.rs:92-128`).
- Cet emplacement est rempli :
  - à la construction de la session (`crates/pa-daemon/src/agent_engine/lifecycle.rs:1038-1045`), une seule fois (`crates/pa-daemon/src/agent_engine/turn/run_loop.rs:211-229`) ;
  - au changement de modèle ;
  - au basculement de fournisseur ;
  - et par **`Reload`** (`crates/pa-daemon/src/agent_engine/reload.rs:42-120`, qui résout de nouveau la clé, `:70-110`).
- **Le pont n'envoie jamais `Reload`** (`grep -rn Reload crates/pa-daemon/src/acp/` vide). Le TUI non plus après `/login` (§1.3).
- Le hook `get_api_key` de la boucle d'agent n'est branché nulle part (`crates/pa-agent/src/agent_loop/response.rs:104-112`, d'après la recherche ; à confirmer).
- Un échec d'auth est relancé une fois, sur la même cible (`crates/pa-core/src/session_engine/provider_retry.rs:152`). Aucun rafraîchissement forcé sur 401 : `mark_auth_stale` n'est appelé qu'en test (`crates/pa-daemon/src/model_switch.rs:500`, sous `#[cfg(test)]`).
- **Conséquences attendues (hypothèses, §7)** :
  - un jeton OAuth expiré en cours de session reste envoyé, et 401 en boucle, jusqu'à un changement de modèle ou un nouveau processus ;
  - un login fait ailleurs n'atteint pas une session déjà construite ;
  - une session qui a échoué à la garde avant toute construction (identifiant absent) prend le nouvel identifiant au prompt suivant.
- **Upstream décrit le même défaut** dans `a05bcb695` (11/11/2026, « live auth refresh ») et `dad2232cc` (12/11/2026) : « a /login from another process or an OAuth token expiring mid-session never reached the live session — every request kept the stale token until a daemon restart » (message de `dad2232cc`).
  - Correctif : résolution de la clé à chaque appel au modèle, rafraîchissement forcé d'un identifiant rejeté, verrou de rafraîchissement entre processus.
  - Taille : 40 fichiers, +4 965/−1 208 pour `a05bcb695` ; 22 fichiers, +904/−110 pour `dad2232cc` (`git show --stat`).
  - Suite : `6e4b69da2` (25/11/2026) ; branches non fusionnées `upstream/auth-401-forced-refresh` et `upstream/fix/auth-reload-on-401-20261122`.
  - Aucun de ces commits n'est chez nous (`git log --grep` sur leurs SHA et leurs sujets : 0).

### 4.3 Liste des modèles du pont

- Le pont lit les modèles disponibles une fois, à `session/new` (`crates/pa-daemon/src/acp/daemon.rs:1291-1301`). Il garde ensuite cette liste en cache (`HostedConfig.models`) et la réutilise à chaque republication (`crates/pa-daemon/src/acp/wire_config.rs:346`).
- Un modèle n'est disponible que si son fournisseur a un identifiant (`crates/pa-core/src/models/registry.rs:214-226`, `:246-249`). Un login ne change donc pas le sélecteur d'une conversation ouverte avant un changement de modèle ou une nouvelle conversation.

### 4.4 Côté extension

- `isAuthFailure` (`extensions/vscode/src/ya/auth.ts:12-16`) est vrai sur `kind: "authentication"` ou sur un `details` qui contient « No API key found for ».
- Le tour en échec affiche alors `authText` (« Authentification requise : lancez <chemin> dans un terminal, puis /login. », `auth.ts:18-20`, `extensions/vscode/src/chat.ts:632`), sinon `errorText`.
- Rien d'autre ne se passe : pas d'écran ouvert, pas de relecture.
- Tests : agent factice `auth` et `sanscle` (`extensions/vscode/test/fake-agent/fake-agent.mjs:279-293`), `test/unit/agent.test.ts:274-282`, e2e `test/e2e/suite.ts:389-393`.
- **Aujourd'hui, connexion expirée (déduit de §4.1 et §4.2)** : un 401 donne `kind: "authentication"`, donc le message « lancez … /login ». Un `/login` dans le terminal écrit `auth.json`, mais la session ACP déjà construite garde l'ancienne clé, et le message revient. En pratique, il faut fermer la conversation et la rouvrir (nouveau pont), ou changer de modèle.

## 5. Sécurité

- **Kernel** : variables d'environnement en liste blanche, identifiants des fournisseurs retirés (`crates/pa-core/src/kernel/env_allowlist.rs:135-147`, `:223-225`, appliqué `crates/pa-core/src/kernel/manager/startup.rs:535`, `:546`). Le kernel peut lire `auth.json` sur disque : limite connue, réglée par le sandbox (`docs/yusAgent-cdc.md:627`).
- **Journaux du cœur** : aucun `tracing` ni `eprintln!` dans `crates/pa-core/src/auth`, `crates/pa-ai/src/oauth`, les `crates/pa-cli/src/*login*.rs` (recherche par grep). Exceptions :
  - le login MCP du worker écrit l'URL d'autorisation sur stderr (`crates/pa-daemon/src/mcp_login.rs:43`, `:50`) : `state` et `code_challenge`, pas de jeton ;
  - pour Claude, le `state` de l'URL **est le vérifieur PKCE** (`crates/pa-ai/src/oauth/anthropic.rs:4`, `:120`) : l'URL d'un login ne doit être journalisée nulle part ;
  - une erreur d'échange de code peut contenir le corps de la réponse (`anthropic.rs:627`), qui irait dans `message`.
- **Le pont** écrit sur stderr le texte d'un tour en échec (`crates/pa-daemon/src/acp/daemon.rs:1695`). L'extension recopie chaque ligne de stderr dans l'Output (`extensions/vscode/src/agent.ts:254-255`). Tout ce que le pont met sur stderr pendant un login arrive donc dans l'Output.
- **L'extension** ne journalise que le nom et le numéro des trames, jamais leurs paramètres (`agent.ts:148`, `:153`, `:216`). La clé de `set_api_key`, la réponse de `login_answer` et l'URL de `login_update` n'y sont pas, contrairement à Unharnessed (§3.2).
  - Lignes de `providers.ts` qui écrivent un message de l'agent : `:144` et `:229`, `(${result.message})`. À vérifier : `message` ne doit pas porter de secret.
  - `local_probe` journalise l'adresse et le message d'erreur (`:113`), pas la clé.
  - `setToken` MCP journalise l'id seulement (`extensions/vscode/src/settings.ts:123`).
- **Trames journalisées** : le banc de capture garde les trames ACP dans des `transcript*.jsonl` (`bench/tools/capture/acp_live_changes.py:232`, `acp_resume.py:227`) ; la fixture `extensions/vscode/test/fixtures/acp-frames.json` aussi. Les tests du login doivent n'y mettre que des clés factices. Un test doit vérifier qu'aucune trame enregistrée ne contient la clé saisie.
- **Fichiers de session** : les méthodes `_yus/providers/*` ne passent que par le pont et `auth.json`, rien n'entre dans le JSONL de la conversation (pas de commande du démon qui écrit dans la session, §1.4). À prouver par un test : la clé factice absente du fichier de session et du journal d'usage.
- **Navigateur depuis l'extension** : les flux attendent un rappel sur `localhost` (53692, 1455, 53700-53709) ou un collage. Il faut donc ouvrir la page **sur la machine où tourne le binaire**.
  - En local, c'est la même machine : `login_open` → `open_in_browser`, comme Unharnessed.
  - VS Code distant (SSH, WSL) : `openExternal` ouvre la page sur le poste de l'utilisateur. Le rappel vers `localhost:<port>` n'atteint alors le binaire que si VS Code relaie le port. Il reste le collage du code (Claude, Codex), et les codes d'appareil (Copilot, xAI) marchent partout. Non testé.
  - `registerUriHandler` n'est utile qu'à un fournisseur qui imposerait un rappel `vscode://`, ce qu'aucun ne fait (§3.2 ; ticket 24 §4).
- **Ports fixes** : un login Claude dans le TUI et un autre dans l'extension en même temps se disputent le port 53692. Le second échoue (`anthropic_callback.rs:70-79`).
- **Rafraîchissement concurrent** : plusieurs workers lisent le même `auth.json`. Sans le verrou entre processus d'upstream (`dad2232cc`), deux processus peuvent dépenser le même jeton de rafraîchissement à usage unique (hypothèse tirée du message de `dad2232cc` et de `0b2bfbc1` d'Unharnessed ; non mesuré chez nous).

## 6. Effet sur le préfixe de prompt

- **Prompt système et tools : inchangés.** Rien dans `_yus/providers/*` n'entre dans la construction du prompt (`crates/pa-core/src/prompts/system_prompt.rs`, ticket 31 §1.2) ni dans les tools.
- **`Reload` après un login** : il remplace la clé et les en-têtes, relit l'auth MCP et rafraîchit l'état MCP (`reload.rs:52-60`). Corps de requête attendu identique (même modèle), à prouver par capture N → N+1 avec un `Reload` entre les deux.
- **Modèle** : un logout peut retirer le modèle courant. Unharnessed passe alors au modèle par défaut (§3.1), ce qui est un changement de modèle, déjà journalisé `model_change` (`docs/yusAgent-cdc.md:93`). Un nouveau modèle choisi par l'utilisateur, de même.
- **Catalogue MCP** :
  - un service connecté devient utilisable (`is_authed`, `crates/pa-core/src/mcp/mod.rs:487-537`, `:899-923`), et son skill (`linear`, `notion`) n'est plus masqué (`mod.rs:579-588`) ;
  - au prompt suivant, les annonces du ticket 31 (`[mcp-changed]`, `[skills-changed]`) partent **en fin d'historique** : le modèle reçoit ces lignes, le préfixe ne change pas (ticket 31 §3.3) ;
  - c'est le seul effet de ce ticket sur ce que reçoit le modèle.
- **Si les commits upstream de §4.2 sont repris** : ils touchent la couche providers et la résolution de l'auth, pas le texte des requêtes (lecture des messages de commit). À prouver par le contrôle permanent du préfixe (`bench/tools/capture/prefix_paths_check.py`) et le test `crates/pa-core/tests/prefix_stability.rs`.

## 7. Hypothèses (non vérifiées)

1. Une session ACP déjà construite garde sa clé après un login fait ailleurs, et un jeton OAuth expiré en cours de session reste envoyé (§4.2). Lu dans le code, et décrit par upstream pour le code d'avant `a05bcb695` ; **pas reproduit chez nous**.
2. Le message « Authentication failed for … Run /login » sort en `other` (§4.1). Lu, non mesuré.
3. En VS Code distant, le rappel `localhost` n'atteint pas le binaire sans relais de port (§5).
4. Le hook `get_api_key` de l'agent n'est branché nulle part (§4.2). Vu par recherche, non relu en entier.
5. Les commits d'upstream de §4.2 s'appliquent sur notre arbre. Rien n'a été essayé : ils touchent des fichiers que le fork a modifiés (`crates/pa-daemon/src/agent_engine/turn/model.rs`, `crates/pa-core/src/session_engine/provider_adapter.rs`, listés dans `docs/fork-changes.md:44`, `:90`, `:1532`).

## 8. Plan

### 8.1 Options pour le pont

**P-A, forme d'Unharnessed, fichiers déplacés.** Déplacer `crates/pa-tui/src/auth_panel.rs` (675 lignes) et `provider_auth.rs` (922 lignes) vers `pa-daemon`, avec réexport dans `pa-tui`, comme `4f1a40f6`. Servir `_yus/providers/*` dans un module du fork, avec l'implémentation de `pa-cli` injectée au lancement du mode ACP.
- Upstream touché : deux fichiers déplacés. `pa-tui` dépendrait alors de `pa-daemon` (nouvelle dépendance), ou les modules iraient dans `pa-types`. Chaque `cherry-pick` qui touche ces fichiers devient plus difficile.
- Produit : le même qu'Unharnessed.
- Préfixe : aucun effet.

**P-B, trait du fork injecté (proposition).**
- `crates/pa-daemon/src/acp/ya_providers.rs` (nouveau) : un trait minimal (lignes, clé, login avec puits d'événements, réponse à une invite, annulation, logout) et les 13 méthodes. Formes des trames d'Unharnessed, noms `_yus/*`.
- `crates/pa-cli/src/ya_acp_providers.rs` (nouveau) : l'implémentation, qui appelle `ProviderAuth` tel quel. Elle crée un `AuthPanelHandle` et traduit les `AuthPanelRequest` reçues en `login_update`, puis les réponses de `login_answer` en `reply`.
- Injection : quelques lignes dans `crates/pa-cli/src/print_runtime.rs` (fichier upstream, à lister dans `fork-changes.md`).
- Upstream touché : ni `pa-ai`, ni `pa-core/auth`, ni `pa-tui`, ni les fonctions de login de `pa-cli` (règles 2, 3, 7) ; une injection de quelques lignes. Le login Claude hérité est **appelé, pas modifié**.
- Produit : le même qu'Unharnessed, avec les écarts du §8.3.
- Préfixe : aucun effet.

**P-C, flux appelés directement depuis le pont.** Écrire dans `pa-daemon` une implémentation ACP de `OAuthLoginUi` et `CodexLoginUi`, et recopier le rangement (environ 15 lignes par fournisseur) et la liste des lignes (`login_rows_blocking`).
- Upstream touché : aucun fichier.
- Code recopié de `pa-cli` : les deux copies divergeront à chaque correctif upstream, comme `26e79b2fc` (« the Codex login returns only after its callback listener closed »).
- Préfixe : aucun effet.

**Écarté : `authMethods` + `authenticate` d'ACP** (option (b) du ticket 24 §4). Unharnessed ne s'en sert pas, notre extension ne les appelle pas, et ces méthodes ne portent ni progression, ni invite, ni liste.

Dans les trois options, **après un `done`, le pont envoie `Reload` au worker** : l'opération existe déjà (`DaemonCommand::Reload`, `crates/pa-daemon/src/worker/commands.rs:138`). Il relit ensuite `GetAvailableModels`, met à jour `HostedConfig.models` et republie les options (`config_option_update`). C'est l'équivalent de `refresh_credentials` d'Unharnessed. `_yus/providers/reload`, que l'extension envoie aux autres conversations, fait la même chose sans login.

### 8.2 Connexion expirée (indépendant du choix ci-dessus)

**E-A.** Rien de plus que §8.1. Le `Reload` après un login répare les sessions de l'extension, mais pas un jeton expiré en cours de session, ni un `/login` fait dans le TUI.

**E-B.** L'extension, sur un échec `authentication`, envoie `_yus/providers/reload` puis propose de renvoyer le message. Le `Reload` relit `auth.json`, ce qui rafraîchit un jeton OAuth expiré (`lookup.rs:69-70`).
- Code du fork seulement, aucune relance automatique.
- Le message reste affiché, mais pointe vers l'écran Providers au lieu du terminal.

**E-C.** Reprendre d'upstream `a05bcb695`, `dad2232cc` (et `6e4b69da2`) par `git cherry-pick -x`, dans un ticket à part, avant ou après celui-ci.
- C'est la récupération sélective prévue pour l'auth (`docs/yusAgent-cdc.md:27`).
- Taille et conflits inconnus (§7, hypothèse 5) ; couche providers et `pa-core/auth` touchées.
- Préfixe attendu inchangé, à prouver.

### 8.3 Extension

1. `_yus/providers/*` dans `served()` (`methods.ts:10-33`) : bouton Providers, puce « Connect a provider », SignIn.
2. `CATALOG = true` (`SettingsMcp.tsx:15`) : connexion OAuth des services du catalogue par `login` `mcp:<id>`. Le formulaire de jeton suppose que le pont donne `token` (`ya_resources.rs:441-448`, à ajouter si on rouvre le jeton).
3. `authText` (`auth.ts:18-20`) : un texte qui renvoie à l'écran Providers, avec un bouton, au lieu du terminal. À décider (§9).
4. `local_*` : écran Local masqué tant que le cœur n'a pas de détection de serveur local. Porter `uh:crates/uh-core/src/models/local.rs` serait du code neuf.
5. Le journal reste sans paramètres de trame (stricter qu'Unharnessed). Messages de l'agent : relire `providers.ts:144`, `:229`.

### 8.4 Découpage en PR

- **Proposition : deux PR**, comme pour les tickets 31 et 39.
  - **R5, pont** : `_yus/providers/*`, `Reload` et relecture des modèles après un login, tests Rust. Contrôlable seul par script ACP sur le binaire de la branche.
  - **X5, extension** : `served()`, catalogue, texte d'échec.
  - Le binaire de R5 sert aux contrôles de X5.
- Une seule PR est possible, mais le run `ya-extension` et le build du binaire tourneraient ensemble à chaque push (règle « regrouper les corrections »).
- E-C, s'il est retenu : PR à part, d'abord sa propre CI.

### 8.5 Tests à voir rouges puis verts sur leur assertion

Le fournisseur factice ou des requêtes interceptées partout, aucun vrai fournisseur. Les flux OAuth sont testés sur leur forme, avec un serveur OAuth local ou un faux `ProviderAuth`. Un push 1 avec les tests seuls, comme les tickets 31 et 39 ; l'agent factice est aligné au push 1, pour que les tests échouent sur l'assertion et non sur `-32601`.

Pont (Rust) :
1. `_yus/providers/list` : forme des lignes, sans valeur secrète. Une clé d'environnement apparaît comme connectée et non supprimable.
2. `set_api_key` avec une clé factice : `auth.json` contient l'entrée en 0600. La réponse `{providers, outcome: "done"}` ne contient pas la clé.
3. **Session vivante** : une session construite sans clé, puis `set_api_key` : le prompt suivant envoie la nouvelle clé (en-tête `Authorization` intercepté). Rouge aujourd'hui sur l'en-tête, pas sur la méthode.
4. Liste des modèles : après `set_api_key` sur un fournisseur de plus, `config_option_update` porte ses modèles.
5. Login par navigateur avec un faux `ProviderAuth` qui émet `url`, `prompt`, puis `done` : trames `login_update` dans l'ordre, `login_answer` relayé, un second `login` refusé pendant le premier, `login_cancel` qui règle en `cancelled`.
6. `login_open` n'ouvre que l'URL du login en cours (ouvreur injecté).
7. **Expiration** (selon E-B ou E-C) : identifiant OAuth expiré, serveur de jetons local qui rafraîchit. Le prompt suivant envoie le jeton neuf. Rouge aujourd'hui si l'hypothèse 1 tient. Ce test vérifie l'hypothèse.
8. Aucun secret : une clé factice repérable (`sk-ya40-...`) n'apparaît ni sur stderr du pont, ni dans le JSONL de session, ni dans le journal d'usage.

Extension (TypeScript) :

9. `features().providers` vrai ; le bouton Providers visible (unitaire + e2e contre l'agent factice).
10. SignIn : trames `login_update` de l'agent factice affichées (URL, invite, attente), « Open » envoie `login_open`.
11. Catalogue : section visible, connexion d'un service par `login` `mcp:<id>`.
12. Échec `authentication` : nouveau texte (selon §9). Les tests `agent.test.ts:274-282` et `suite.ts:389-393` passent au rouge sur l'ancien texte, puis au vert.
13. Journal : l'Output ne contient pas la clé saisie, ni l'URL du login.

Banc de capture :

14. Cas `acp_frames.py` : `_yus/providers/list` et `set_api_key` factice. Corps de la requête N et N+1 identiques autour du `Reload` (préfixe).

### 8.6 Contrôle manuel prévu (Yusuf, VS Code, binaire de la branche)

1. Déplacer `~/.yusagent/auth.json` (sauvegarde), ouvrir l'extension : puce « Connect a provider ». L'écran Providers liste les fournisseurs.
2. **Appel réel, fait par toi** : connexion ChatGPT par abonnement depuis l'écran. La page s'ouvre sur clic, le login se termine et le sélecteur de modèles se remplit sans rouvrir la conversation. « Réponds seulement : bonjour » répond.
3. Deuxième conversation ouverte pendant le login : son sélecteur se met à jour (`reload`).
4. Logout depuis l'écran : le prompt suivant échoue avec le nouveau texte, qui mène à l'écran Providers.
5. Catalogue MCP : un service visible ; connexion d'un service (si tu en as un) ; annonce `[mcp-changed]` au message suivant.
6. Output : aucune clé, aucune URL de login.
7. Remettre `auth.json`.

Les étapes 2 à 5 font des appels réels : à ta main, pas à Claude Code (règle 6).

## 9. Choix à soumettre à Yusuf

1. **Pont** : P-A (fichiers déplacés comme Unharnessed), P-B (trait du fork injecté, aucun fichier de login touché) ou P-C (flux appelés depuis le pont, code recopié).
2. **Connexion expirée** : E-A, E-B, E-C, ou E-B maintenant et E-C en ticket à part.
3. **Rangement des secrets** : `auth.json` comme Unharnessed et le TUI. Le CDC dit `SecretStorage` (`docs/yusAgent-cdc.md:393`) : écart à porter au journal. `SecretStorage` voudrait dire passer la clé au binaire à chaque lancement (variable d'environnement ou méthode), et le TUI ne la verrait pas.
4. **`registerUriHandler`** : ne pas le faire, comme Unharnessed (aucun fournisseur ne l'impose). Écart au CDC (`:393`) à porter au journal.
5. **Claude par abonnement dans l'extension** : ligne proposée comme chez Unharnessed (appel du login hérité, sans le modifier, avec l'avertissement du cœur), ou ligne cachée dans l'extension (règle 7 ; `docs/yusAgent-cdc.md:163`).
6. **Serveurs locaux (`local_*`)** : écran Local masqué (pas de code dans le cœur), ou portage du module d'Unharnessed.
7. **Texte d'échec d'auth** : le texte d'Unharnessed (`details` tel quel), ou un texte fixe qui mène à l'écran Providers.
8. **Jeton MCP** (`set_token`) : rouvert dans ce ticket (le pont donne `token` pour les services `authKind: "api key"`), ou laissé en limite.
9. **Découpage** : deux PR (R5 puis X5) ou une seule.
