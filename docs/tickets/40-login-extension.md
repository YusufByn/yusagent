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

## 10. Décisions de Yusuf (25/11/2026)

0. **Politique upstream** : pas de reprise régulière de Prime Agent. De temps en temps, veille dans un clone jetable ; chaque reprise (cherry-pick ou réimplémentation) passe par un ticket avec constat. Portée au journal du CDC et en tête de `docs/fork-changes.md`.
1. **Pont** : comparer P-A (choix d'Unharnessed) et P-B, sans code : (a) lignes upstream touchées, fichier:ligne ; (b) `a05bcb695` et `dad2232cc` s'appliqueraient-ils encore après chacune (essai `git cherry-pick --no-commit` dans un worktree jetable). Règle : **P-A si elle ne gêne pas ces deux commits, sinon P-B**. Comparaison au §11 ; **choix à confirmer par Yusuf**.
2. **Connexion expirée** : un login depuis l'extension pousse la nouvelle clé dans les sessions vivantes et relit la liste des modèles (comme Unharnessed). Le test 7 (H1) vient en premier, rouge sur son assertion si H1 est vraie. Rafraîchissement automatique d'un jeton expiré (`a05bcb695`, `dad2232cc`) : ticket à part (E-C), à prioriser si H1 est confirmée.
3. **Identifiants dans `auth.json`** ; écart au CDC porté au journal.
4. **Pas de `registerUriHandler`** ; écart au CDC porté au journal ; VS Code distant : collage du code (limite).
5. **Claude par abonnement caché dans l'extension** (règle 7).
6. **Serveurs `local_*`** : écran masqué.
7. **Échec d'auth** : texte fixe qui mène à l'écran Providers (écart à Unharnessed : le texte brut renvoie au TUI). Le classement de l'erreur d'authentification dans le pont (H2) est traité dans ce ticket.
8. **`set_token` rouvert** (formulaire démasqué, méthode déjà servie). Services du catalogue par OAuth MCP : inclus seulement si le pont les couvre sans coût en plus ; sinon limite (réponse au §13.2).
9. **Une seule PR**, sauf si la partie pont est lourde : estimation au §13.3, Yusuf tranche.

**Exigence de sécurité** : aucun secret ni URL de login (le `state` est le vérifieur PKCE) dans les journaux, l'Output, les trames journalisées ni les fichiers de session ; un test le vérifie (§12, test 8).

## 11. Comparaison P-A / P-B (décision 1)

Branche `ya-40-constat`. Worktree jetable sur `1ece0ff04` (`$CLAUDE_JOB_DIR/tmp/wt40`), script `sim40.sh` du dossier temporaire de la session, rien compilé (règle 8).

### 11.1 P-A chez nous n'est pas le déplacement d'Unharnessed

- Chez Unharnessed, `4f1a40f6` a déplacé `auth_panel.rs` et `provider_auth.rs` de `uh-cli` vers `uh-daemon`, en renommage simple : 11 et 6 lignes changées (`git show --stat 4f1a40f6`).
- Chez nous, ces deux fichiers sont dans `pa-tui` et **mélangent le contrat et le rendu du TUI** :
  - `crates/pa-tui/src/auth_panel.rs` :
    - le contrat va de `:35` à `:368` : `PrimeTeamOption` `:35`, `PrimeTeamPick` `:44`, `PasteStyle` `:55`, `PastePromptTone` `:64`, `AuthPanelRequest` `:81-129`, `FlowCancel` `:131-180`, `AuthPanelHandle` `:182-368` ;
    - le panneau rendu commence à `:370` (`AuthPanel`, thème `:18`, presse-papiers `:550`, `:640-647`) ;
    - l'enum de requêtes porte `TracesSettled` (`:121-124`), qui dépend de `TraceLoginOutcome` (`crates/pa-tui/src/traces.rs:370`).
  - `crates/pa-tui/src/provider_auth.rs` :
    - le contrat va de `:17` à `:148` : `AuthType`, `AuthFlow`, `ProviderRow` `:56`, `ProviderAuthOutcome` `:73`, trait `ProviderAuthCommands` `:86-106`, constantes `:121-137` ;
    - le sélecteur rendu va de `:151` à `:533` (thème `:13`, `render` `:392`).
  - Le login MCP passe par `pa_tui::client_auth::run_mcp_auth_command` (`crates/pa-cli/src/provider_login.rs:506`), qui reste dans `pa-tui`.
- P-A chez nous = **scinder** ces deux fichiers upstream.
  - Déplacer le contrat, environ 330 et 130 lignes, estimation tirée des plages ci-dessus, vers un crate que `pa-tui` et `pa-daemon` voient tous les deux.
  - Aujourd'hui, `pa-tui` ne dépend que de `pa-types` (`crates/pa-tui/Cargo.toml:9`). La cible serait donc :
    - `pa-daemon`, comme Unharnessed, ce qui fait **une nouvelle dépendance `pa-tui` → `pa-daemon`** ;
    - ou `pa-types`, qui a déjà `tokio` (`crates/pa-types/Cargo.toml:13`).
  - Puis réexporter depuis `crates/pa-tui/src/lib.rs:32` et `:96`, pour que les 13 fichiers qui importent `pa_tui::auth_panel` ou `pa_tui::provider_auth` ne changent pas (`grep -rln`).

### 11.2 (a) Lignes upstream touchées

| | P-A | P-B |
|---|---|---|
| `crates/pa-tui/src/auth_panel.rs` | contrat `:35-368` sorti du fichier (environ 330 lignes, estimation) ; variante `TracesSettled` `:121-124` à traiter (déplacer `TraceLoginOutcome`, `crates/pa-tui/src/traces.rs:370`, ou couper l'enum) | aucune |
| `crates/pa-tui/src/provider_auth.rs` | contrat `:17-148` sorti du fichier (environ 130 lignes, estimation) | aucune |
| `crates/pa-tui/src/lib.rs` | réexports à la place de `:32` et `:96` | aucune |
| `crates/pa-tui/Cargo.toml` | + `pa-daemon` (ou rien si la cible est `pa-types`) | aucune |
| crate cible (`pa-daemon/src/lib.rs` ou `pa-types`) | + 2 modules | aucune |
| `crates/pa-daemon/src/acp/daemon.rs` | aiguillage des `_yus/providers/*`, sur le modèle de `ya_resources` (`:1154-1161`) : environ 8 lignes ; `login_cancel` et annulation à la déconnexion : quelques lignes (emplacement à lire) | idem |
| `crates/pa-cli/src/print_runtime.rs` | injection de l'implémentation au lancement du mode ACP, avant `run_daemon_attached_acp_mode` (`:109-110`) : 1 à 3 lignes | idem |
| Nouveaux fichiers du fork | `acp/ya_providers.rs` (pont) ; l'implémentation dans `pa-cli` | `crates/pa-daemon/src/acp/ya_providers.rs` (pont, trait du fork) ; `crates/pa-cli/src/ya_acp_providers.rs` (implémentation, appelle `ProviderAuth` et traduit `AuthPanelRequest` en `login_update`) |

**Fréquence des changements upstream** sur les fichiers que P-A scinde, depuis le point de fork (`git log c83ac68b3..upstream/main`) :
- `crates/pa-tui/src/auth_panel.rs` et `provider_auth.rs` : 1 commit (`12a5081ac`, 3 lignes de `provider_auth.rs`) ;
- `crates/pa-cli/src/print_runtime.rs` : 10 commits ;
- `crates/pa-daemon/src/acp/daemon.rs` : 6 commits.

### 11.3 (b) `a05bcb695` et `dad2232cc` après chacune

- **Lecture.** Ni `a05bcb695` (40 fichiers) ni `dad2232cc` (22 fichiers) ne touchent `pa-tui`, `print_runtime.rs`, un `Cargo.toml` ou `crates/pa-daemon/src/acp/` (`git show --name-only`). Aucun des deux ne nomme `auth_panel`, `provider_auth`, `AuthPanelHandle`, `ProviderAuthCommands` ou `pa_tui` (`git show <c> | grep`, 0 ligne). Le seul fichier de `pa-cli` touché est `crates/pa-cli/src/lib.rs` (`dad2232cc`, 4 lignes dans `main_impl`).
- **Essai** : `bash $CLAUDE_JOB_DIR/tmp/sim40.sh $CLAUDE_JOB_DIR/tmp/wt40`. Pour chaque état : `cherry-pick --no-commit a05bcb695`, résolution mécanique de ses conflits, commit, puis `cherry-pick --no-commit dad2232cc`.
  - P-A est simulé par le déplacement des deux fichiers entiers vers `pa-daemon`, avec réexport et dépendance (pas la scission du §11.1, qui ne touche pas plus de fichiers de ces commits).
  - P-B est simulé par deux fichiers neufs et une ligne dans `run_acp_mode`.

```
main | a05 exit=1 conflits: crates/pa-daemon/src/agent_engine/tests.rs  | dad après a05 exit=0 conflits: aucun
P-A  | a05 exit=1 conflits: crates/pa-daemon/src/agent_engine/tests.rs  | dad après a05 exit=0 conflits: aucun
P-B  | a05 exit=1 conflits: crates/pa-daemon/src/agent_engine/tests.rs  | dad après a05 exit=0 conflits: aucun
```

- Le conflit de `a05bcb695` est le même dans les trois cas. Il compte 2 blocs dans le fichier de tests :
  - `mod live_auth;` à côté de nos modules de tests ;
  - `pub(crate)` sur `write_compaction_settings`, à côté de notre commentaire YA-01.
  - Il vient du fork, pas de P-A ni de P-B.
- **Limite** : application textuelle seulement, rien compilé. Que ces commits compilent et passent sur notre arbre relève du ticket E-C.
- **Après ces commits, `Reload` existe toujours** (`"reload" =>` en `crates/pa-daemon/src/worker/commands.rs:138` et `reload_live_inputs` sur `upstream/main`). Pousser la nouvelle clé par `Reload` (décision 2) reste valable après E-C.

### 11.4 Ma lecture

- **Selon ta règle, P-A** : elle ne gêne pas les deux commits, mesuré au §11.3.
- **À peser avant de confirmer** :
  - chez nous, P-A n'est pas le renommage d'Unharnessed, mais la scission de deux fichiers upstream (environ 460 lignes déplacées, estimation) ;
  - elle demande aussi une nouvelle dépendance `pa-tui` → `pa-daemon`, ou une place dans `pa-types` ;
  - ces fichiers changent rarement upstream (1 commit de 3 lignes en 23 jours), mais tout commit futur qui les touche deviendrait un conflit sur fichier déplacé ;
  - P-B donne le même produit, avec seulement les lignes communes aux deux options (aiguillage, injection).
- Je n'en tire pas de conclusion : la règle donne P-A, et ces trois points sont des coûts que la règle ne mesurait pas.

## 12. Plan de tests (rouges puis verts sur leur assertion)

- **Fournisseurs** : le provider factice ou des requêtes interceptées, aucun vrai fournisseur (règle 6).
- **Flux de login** : avec un faux `ProviderAuth` (implémentation de test du trait) et, pour l'échange de jetons, un serveur local scripté.
- **Push 1** : tests seuls, plus l'agent factice de l'extension aligné. Chaque test échoue sur son assertion, pas sur `-32601` ni sur une compilation. Pour les tests Rust : méthodes servies par un module vide qui répond `-32603`, ou appel direct du worker, selon le test.

**Pont (Rust)**

1. **Test 7 en premier, H1** (`crates/pa-daemon/tests/ya_auth_live_e2e.rs`, nouveau) : démon et worker réels, fournisseur intercepté par un serveur HTTP local (forme de `crates/pa-core/tests/prefix_stability.rs`).
   - (a) Session construite avec la clé factice `sk-ya40-old`, puis `auth.json` réécrit avec `sk-ya40-new`. Prompt suivant : l'en-tête `Authorization` vaut `sk-ya40-new`. **Rouge attendu si H1 est vraie**, puisque la clé reste `old`.
   - (b) Identifiant OAuth expiré, serveur de jetons local qui rafraîchit en `tok-new`. Prompt suivant : en-tête `tok-new`. Rouge attendu si H1 est vraie. **Ce cas reste rouge après ce ticket** (le rafraîchissement automatique est E-C) : il part en `#[ignore]` avec le numéro du ticket E-C, une fois son rouge relevé.
2. **Clé poussée dans la session vivante** : (a) refait par `_yus/providers/set_api_key` au lieu de l'écriture directe. Rouge sur l'en-tête, vert avec le `Reload` du pont.
3. **Liste des modèles** : après `set_api_key` sur un fournisseur de plus, `config_option_update` porte ses modèles, sans nouvelle `session/new`.
4. **`_yus/providers/list`** : forme des lignes ; une clé d'environnement apparaît connectée et non supprimable ; **la ligne `anthropic` par abonnement est absente** (décision 5) ; aucun champ ne contient une valeur de clé.
5. **`set_api_key`** : `auth.json` contient l'entrée, fichier en 0600 ; la réponse `{providers, outcome: "done"}` ne contient pas la clé.
6. **Login par navigateur** (faux `ProviderAuth` qui émet `url`, `prompt`, puis règle `done`) :
   - les `login_update` arrivent dans l'ordre ;
   - `login_answer` est relayé ;
   - un second `login` est refusé pendant le premier ;
   - `login_cancel` règle en `cancelled` ;
   - la fin de stdin annule ;
   - `login_open` n'ouvre que l'URL du login en cours (ouvreur injecté).
7. **H2, classement** : les deux textes de la garde (`crates/pa-daemon/src/agent_engine/turn/model.rs:60-64`, `:67-72`) donnent `kind: "authentication"` dans la réponse de `session/prompt`. Rouge sur `"other"`. Le message affiché vient de l'extension (décision 7), `details` garde le texte.
8. **Aucun secret** :
   - Avec la clé factice repérable `sk-ya40-secret-…` et un login dont l'URL porte `state=ya40-verifier-…`, aucune de ces deux chaînes n'apparaît dans :
     - le stderr du pont ;
     - le JSONL de session et le journal d'usage (`~/.yusagent/sessions/<id>.usage.ndjson`) ;
     - les trames enregistrées par le banc de capture.
   - Le serveur de jetons scripté rend en plus un succès malformé qui contient un jeton : la réponse `login` en échec ne doit pas le contenir (§13.1).

**Extension (TypeScript, unitaires contre l'agent factice ; e2e en CI)**

9. `features().providers` vrai, bouton Providers visible. Puce « Connect a provider » quand aucun fournisseur n'est connecté.
10. SignIn : les `login_update` de l'agent factice s'affichent (URL, invite, attente) ; « Open » envoie `_yus/providers/login_open`.
11. Écran Local masqué (décision 6). Aucune ligne Claude par abonnement, même si l'agent factice en envoie une (décision 5, défense côté extension).
12. Échec d'auth : nouveau texte fixe et bouton qui ouvre l'écran Providers (décision 7). Les tests `extensions/vscode/test/unit/agent.test.ts:274-282` et `test/e2e/suite.ts:389-393` passent d'abord au rouge sur l'ancien texte.
13. `set_token` : le formulaire de jeton s'ouvre pour un service qui a `token` ; `_yus/mcp/set_token` part ; l'Output ne contient pas le jeton.
14. Output : ni la clé saisie, ni l'URL du login, ni la réponse d'une invite (le canal journalisé de l'extension est lu par le test).

**Banc de capture**

15. Cas `acp_frames.py` : `_yus/providers/list` et `set_api_key` avec une clé factice. Corps des requêtes N et N+1 identiques autour du `Reload` (préfixe) ; transcript sans la clé.

## 13. Points ouverts pour la suite du plan

### 13.1 Constat de sécurité ajouté : nos erreurs OAuth recopient le corps de la réponse

- Dans notre arbre, les erreurs d'échange ou de rafraîchissement de jetons contiennent le corps de la réponse du serveur, qui peut porter un jeton :
  - `crates/pa-ai/src/oauth/anthropic.rs:311`, `:317`, `:325-337` ;
  - `crates/pa-ai/src/oauth/openai_codex.rs:325`, `:332-353`.
- `dad2232cc` corrige exactement cela : « A malformed SUCCESS body can echo credentials back: the errors name the missing field, never the response body », avec un test.
- Par le pont, ce texte deviendrait le `message` d'un login en échec. L'extension l'affiche, et le journalise dans l'Output (`extensions/vscode/src/providers.ts:144`, `:229`).
- **Pour tenir l'exigence de sécurité sans toucher `pa-ai`** : le pont ne transmet pas le texte brut d'un échec de flux. Il envoie un texte fixe par cas (échec, annulé, port occupé), et le détail ne va nulle part. Le test 8 le vérifie.
- `dad2232cc` installe aussi un puits de journaux `pa-ai` sur stderr hors TUI (`crates/pa-cli/src/lib.rs`, 4 lignes). Une fois E-C repris, ces journaux arriveraient dans l'Output de l'extension, à vérifier par le même test.

### 13.2 Catalogue MCP par OAuth (décision 8)

- Le login `mcp:<id>` passe par la même entrée que les fournisseurs : `login_on_panel` aiguille `mcp:*` vers `TerminalMcpAuth` (`crates/pa-cli/src/provider_login.rs:498-518`). Le `Reload` qui suit relit l'auth MCP et rafraîchit l'état MCP (`crates/pa-daemon/src/agent_engine/reload.rs:52-60`).
- Coût en plus, estimation :
  - construire une ligne pour `mcp:<id>` (Unharnessed : `mcp_login_row`, `uh:crates/uh-daemon/src/acp/providers.rs:596-607`, 12 lignes) ;
  - lire l'issue, qui est `ProviderAuthOutcome::Status(String)` et non un succès typé (`provider_login.rs:513-517`) ;
  - environ 20 lignes.
- **Ma lecture : couvert pour environ 20 lignes, pas sans aucun coût.** À toi de dire si c'est « sans coût en plus ».
- **`set_token`** : la méthode est servie, mais le formulaire ne s'ouvre que si `_yus/mcp/list` donne un champ `token` pour le service (`extensions/vscode/webview/SettingsMcp.tsx:239-263`). Le pont ne le donne pas (`crates/pa-daemon/src/acp/ya_resources.rs:441-448`, fichier du fork). Rouvrir `set_token` coûte donc quelques lignes dans le pont, en plus du démasquage.

### 13.3 Taille de la partie pont (décision 9)

**Estimation**, à partir des tailles d'Unharnessed (`uh:crates/uh-daemon/src/acp/providers.rs` 625 lignes, sans `local_*`, qui est dans `uh-core`) et de nos modules du pont (`ya_resources.rs`) ; non mesurée :
- pont (`ya_providers.rs`) : 500 à 700 lignes ;
- implémentation dans `pa-cli` : 150 à 250 ;
- tests Rust (tests 1 à 8) : 600 à 900 ;
- soit 1 250 à 1 850 lignes de Rust ;
- côté extension : 100 à 200 lignes, plus 200 à 300 de tests ;
- avec P-A, il faut ajouter la scission (environ 460 lignes déplacées).

### 13.4 Ouverture du navigateur

- Les fonctions de login de `pa-cli` ouvrent le navigateur **elles-mêmes**, dès que l'URL est connue (`crates/pa-cli/src/subscription_login.rs:46`, `codex_subscription_login.rs:31`, `mcp_login.rs:263`, `prime_inference_login.rs:339`).
- Réutilisées telles quelles (P-A comme P-B), elles ouvriraient la page dès le clic sur « Sign in », sur la machine du binaire. « Open » de l'extension la rouvrirait.
- Unharnessed a retiré ces appels (`4f1a40f6`, 5 et 7 lignes dans `subscription_login.rs` et `codex_subscription_login.rs`), pour n'ouvrir que sur clic : la boîte « ouvrir un site externe ? » de VS Code.
- À trancher :
  - garder l'ouverture automatique : aucune ligne upstream ;
  - ou la couper pour le mode ACP : 4 fichiers upstream de `pa-cli`, une condition chacun.
- En VS Code distant, l'ouverture automatique se ferait sur la machine distante, sans effet ; il reste le collage (décision 4).

### 13.5 H2 : où classer

- Les deux textes de la garde ne sont reconnus par aucune règle (§4.1).
- **(i) Proposé** : une fonction du fork appelée par `turn_failure_error` (`crates/pa-daemon/src/acp/mod.rs:50-76`, upstream, 1 à 2 lignes). Elle reconnaît les deux textes et donne `kind: "authentication"`. Le classifieur de `pa-core` n'est pas touché.
- (ii) Ajout au classifieur upstream (`crates/pa-core/src/session_engine/error_classify.rs`). Il compare des messages exacts (`:330-345`) et le nom du fournisseur varie : il faudrait une règle de préfixe.
- Les textes de la garde sont les mêmes sur `upstream/main` (`turn/model.rs:72`, `:78`), donc E-C ne les change pas.

## 14. Contrôle manuel prévu (Yusuf, VS Code, binaire de la branche)

1. Sauvegarder puis déplacer `~/.yusagent/auth.json`. Ouvrir l'extension : puce « Connect a provider », écran Providers sans ligne Claude par abonnement, sans écran Local.
2. Prompt sans identifiant : texte fixe et bouton vers l'écran Providers (pas de renvoi au terminal).
3. **Appel réel, fait par toi** : connexion ChatGPT par abonnement depuis l'écran, avec l'ouverture du navigateur décidée au §13.4. Puis, **sans rouvrir la conversation**, le sélecteur de modèles se remplit et « Réponds seulement : bonjour » répond.
4. Deuxième conversation ouverte avant le login : son sélecteur se met à jour et elle répond sans être rouverte.
5. Logout depuis l'écran, puis prompt : texte fixe du point 2.
6. Clé API d'un fournisseur (si tu en as une) : connectée, sans rouvrir la conversation.
7. MCP : formulaire de jeton pour un service à jeton (si tu en as un) ; catalogue OAuth selon la décision sur le §13.2.
8. Output de l'extension : rechercher la clé saisie et `state=` : aucune occurrence.
9. Remettre `auth.json`.

Les étapes 3 à 7 font des appels réels : à ta main, pas à Claude Code (règle 6). Les scripts de contrôle remis avec la PR seront d'abord lancés par Claude Code sur le binaire de la branche (règle 13).

## 15. Décisions de Yusuf (25/11/2026, suite)

1. **P-B.**
   - **Écart à Unharnessed, justifié** : chez nous, P-A découpe deux fichiers upstream (environ 460 lignes) et ajoute une dépendance entre crates, pour le même résultat (§11).
   - Lignes upstream : aiguillage dans `acp/daemon.rs` (environ 8), branchement dans `print_runtime.rs` (1 à 3), H2 dans `turn_failure_error` (1 à 2). Lignes exactes au §16.1 ; au-delà, arrêt.
2. **Navigateur** : comportement des fonctions de login conservé.
   - La page s'ouvre au clic sur « Sign in », et aucun des 4 fichiers de `pa-cli` n'est touché.
   - Le bouton « Open » est gardé pour VS Code distant.
   - Limite : double onglet possible.
3. **Catalogue MCP par OAuth inclus** (environ 20 lignes). **`set_token` inclus**, avec le champ `token` dans le pont.
4. **Deux PR** : R5 (pont), puis X5 (extension, avec le contrôle manuel de Yusuf). Dire si R5 seul change un comportement visible dans l'extension actuelle, et si oui prévoir un contrôle court (réponse au §16.6).
5. **Sécurité** :
   - un texte fixe remplace la réponse du serveur de jetons dans les erreurs OAuth (test 8) ;
   - H2 est réglé par une fonction à nous.
6. **Test 1 (H1)** : le cas « jeton expiré » est relevé rouge sur son assertion, puis ignoré avec renvoi au ticket E-C. **Si H1 est confirmée, E-C devient le ticket suivant** (noté aussi au journal du CDC).
7. **`sim40.sh`** est gardé dans `bench/tools/` (`bench/tools/sim40.sh`, servira pour E-C).

Merge seulement sur le « merge » de Yusuf.

## 16. R5 : plan de code

R5 se fait sur la branche `ya-40-constat` (PR #91), comme le ticket 39 (constat et code dans la même PR). X5 aura sa propre branche.

### 16.1 Lignes upstream, exactes (les seules)

| Fichier | Où (lignes de `main` à `1ece0ff04`) | Lignes ajoutées | Contenu |
|---|---|---|---|
| `crates/pa-daemon/src/acp/daemon.rs` | après `:43` (fin des `use`) | 2 | commentaire + `pub mod ya_providers;` (fichier `acp/daemon/ya_providers.rs`) |
| `crates/pa-daemon/src/acp/daemon.rs` | avant `:1118` (`let Incoming::Request … else`) | 2 | commentaire + `ya_providers::notified(&incoming);` (`login_cancel`) |
| `crates/pa-daemon/src/acp/daemon.rs` | après `:1161` (bras `ya_resources`) | 4 | commentaire + bras `other if ya_providers::serves(other) => { ya_providers::serve(other, &id, &params, link, state, &tx).await; }` |
| `crates/pa-cli/src/print_runtime.rs` | après `:109` (`daemon_acp_create`) | 2 | commentaire + `pa_daemon::acp::daemon::ya_providers::install(ya_acp_providers::sign_in(&options.config));` |
| `crates/pa-cli/src/print_runtime.rs` | après `:484` (`mod faux;`) | 1 | `mod ya_acp_providers;` (fichier `print_runtime/ya_acp_providers.rs`) |
| `crates/pa-daemon/src/acp/mod.rs` | dans `turn_failure_error`, après le calcul de `kind` (`:53-59`) | 2 | commentaire + `let kind = daemon::ya_providers::auth_failure_kind(failure, kind);` (H2) |

- **Total : 8 lignes dans `daemon.rs`, 3 dans `print_runtime.rs`, 2 dans `acp/mod.rs`.**
- Module du pont déclaré comme enfant de `daemon.rs` :
  - aucune ligne dans `acp/mod.rs` pour la déclaration ;
  - le module est public pour `pa-cli` (`pa_daemon::acp::daemon::ya_providers`) ;
  - il voit les fonctions de `acp/wire_config.rs`, qui sont `pub(super)` (`:264`, `:283`, `:335`).
- Aucune ligne dans `pa-ai`, `pa-core`, `pa-tui`, ni dans les fonctions de login de `pa-cli`.

### 16.2 Nouveaux fichiers

**`crates/pa-daemon/src/acp/daemon/ya_providers.rs`** (pont)

- **Contrat public**, sans type de `pa-tui` :
  - trait `ProviderSignIn` : `rows`, `set_api_key`, `logout`, `login(provider_id, events)` ;
  - types `ProviderView`, `SignInOutcome { Done, Failed(String), Cancelled }`, `SignInEvents`, `SignInEvent { Progress, Waiting, Url, Prompt { …, reply }, Team { …, reply } }` ;
  - `install` (une seule fois, `OnceLock`).
- **Méthodes servies** :
  - `_yus/providers/list`, `reload`, `set_api_key`, `logout`, `login`, `login_answer`, `login_open` ;
  - la notification `login_cancel` ;
  - les notifications `login_update` vers le client.
  - **`local_*` non servies** (décision 6).
  - Formes d'Unharnessed (`uh:crates/uh-daemon/src/acp/providers.rs:1-39`, `:137-165`, `:332-388`, `:415-479`, `:491-505`).
- **Login** :
  - un seul à la fois ;
  - lancé dans sa propre tâche avec un émetteur **faible** (`downgrade`), pour que la fin de stdin n'attende pas un login en cours ;
  - invites numérotées ; `login_cancel` lève le drapeau du flux et lâche les invites en attente.
- **Après un `done`** (clé, déconnexion, login) et sur `reload` : `Reload` au worker de la session (`DaemonCommand::Reload`), relecture de `get_available_models` dans `HostedConfig.models`, puis `refresh_wire_config`, qui publie `config_option_update` si les options changent. Tout se fait sous `config.queue`, comme `set_config_option`.
- **Messages d'échec** : textes fixes du pont, jamais le texte d'un flux.
- **`auth_failure_kind`** (H2) : un `failure` qui contient `No API key found for ` ou `Authentication failed for "` donne `authentication`. Les autres valeurs de `kind` ne changent pas.

**`crates/pa-cli/src/print_runtime/ya_acp_providers.rs`** (implémentation, au-dessus de `ProviderAuth` de `crates/pa-cli/src/provider_login.rs:189`)

- **Lignes** : `login_options` et `logout_options`, traduites en `ProviderView` :
  - `method` = `subscription` si `AuthType::Oauth` ;
  - `login` = `browser` si `AuthFlow::TerminalFlow` ;
  - `connected` = `configured` ;
  - `removable` = l'identifiant figure dans les lignes de déconnexion ;
  - `note` = l'étiquette de statut d'une source hors `auth.json`.
  - **La ligne `anthropic` par abonnement est retirée** (décision 5).
- **Clé** : la ligne `ApiKeyPrompt` de l'identifiant, puis `login(row, Some(key))`.
- **Login** : la ligne `TerminalFlow` de l'identifiant, ou une ligne `mcp:<id>` (comme `uh:…/providers.rs:596-607`), puis `login_on_panel`.
  - Le panneau est un `AuthPanelHandle` neuf ; une tâche traduit ses `AuthPanelRequest` en `SignInEvent`.
  - Le flux tourne dans un fil à lui, avec son propre runtime, pour ne pas bloquer l'arrêt du runtime du pont.
  - La Claude par abonnement est refusée (ligne cachée).
- **Issue** :
  - `Status` → `Done` ; pour `mcp:<id>`, `Done` seulement si `auth.json` contient `mcp:<id>` ensuite (`Status` porte aussi les échecs MCP, `provider_login.rs:513-517`) ;
  - `Error(_)` → `Failed` avec un **texte fixe** (décision 5 : jamais le corps d'une réponse de jetons, §13.1) ;
  - `Cancelled` → `Cancelled`.
- **Annulation** : `cancel_flag()` du panneau, lu par les flux Claude et Codex (`subscription_login.rs:102-104`, `codex_subscription_login.rs:57-59`).
  - `FlowCancel::mark` est `pub(crate)` dans `pa-tui` (`crates/pa-tui/src/auth_panel.rs:161`), donc inaccessible à `pa-cli`.
  - **Limite** : un login MCP en attente du navigateur ne s'arrête qu'à sa prochaine invite ou à sa fin.

**`crates/pa-daemon/src/acp/ya_resources.rs`** (fichier du fork, décision 3)

- `server_row` reçoit aussi les `services` de `get_mcp_connections` (`crates/pa-daemon/src/mcp_connections.rs:73-84`).
- Un service du catalogue avec `pasteToken: true` (`crates/pa-core/src/mcp/catalog_plugin_views.rs:44`) reçoit `token: "<label> token"`.
- **Écart à Unharnessed** : son texte vient du champ du catalogue (`uh:crates/uh-core/src/mcp/manager_catalog.rs:194-203`, fonction que notre cœur n'a pas) ; le pont n'a que le nom du service.

**Tests** :
- `crates/pa-daemon/src/acp/daemon/ya_providers_tests.rs` ;
- `crates/pa-cli/tests/ya_providers_acp_e2e.rs` ;
- tests unitaires dans `ya_acp_providers.rs` et `ya_resources.rs`.

### 16.3 Tests, dans l'ordre du push 1

Fournisseur intercepté :
- un serveur HTTP local qui relève l'en-tête `Authorization` et répond 400, ce qui ne relance pas (forme de `crates/pa-cli/tests/ya_permissions_e2e.rs:33-90`) ;
- un fournisseur `ya40` en `openai-responses` dans `models.json`, avec la clé de repli `sk-ya40-fallback`. Un identifiant rangé passe avant elle (`crates/pa-core/src/models/registry.rs:818-833`).

| # | Test | Où | Rouge attendu au push 1 |
|---|---|---|---|
| 1a | H1, clé API réécrite dans `auth.json` hors du pont | e2e | 2e requête : `Bearer sk-ya40-old` au lieu de `sk-ya40-new` (si H1) |
| 1b | H1, jeton OAuth (fournisseur `ya40`) remplacé dans `auth.json` par un jeton neuf, comme le ferait un rafraîchissement par un autre processus | e2e | `Bearer tok-ya40-old` au lieu de `tok-ya40-new` (si H1) |
| 2 | clé poussée dans la session vivante par `_yus/providers/set_api_key` | e2e | `Bearer sk-ya40-old` après `done` (le squelette répond `done` sans rien faire) |
| 3 | liste des modèles : `openai` avec un modèle `ya40-extra` dans `models.json` (fournisseur intégré, pas de clé de repli), puis `set_api_key` | e2e | aucun `config_option_update` avec `["openai","ya40-extra"]` |
| 4 | `_yus/providers/list` : `openai-codex` par abonnement et navigateur, `ya40` par clé, aucune ligne `anthropic` par abonnement | e2e + unitaire `pa-cli` | `providers` vide / ligne Claude présente |
| 5 | `set_api_key` : `auth.json` en 0600 contient la clé ; la réponse ne la contient pas | e2e | entrée absente |
| 6 | login : `login_update` dans l'ordre (`url`, `prompt`), `login_answer` relayé, second `login` refusé, `login_cancel` → `cancelled`, `login_open` sur l'URL du login seulement | unitaire `pa-daemon` (faux `ProviderSignIn`) | aucune trame `login_update` |
| 7 | H2 : les deux textes de la garde donnent `kind: "authentication"` | unitaire `pa-daemon` (`turn_failure_error`) | `"other"` |
| 8 | secrets : `sk-ya40-secret-…` absente du stderr du pont, des fichiers de session et du journal d'usage ; présente dans `auth.json` et dans l'en-tête reçu par le serveur. Échec de flux avec un corps qui contient un jeton : texte fixe | e2e + unitaire `pa-cli` | précondition `auth.json` ; texte brut recopié |
| 13 | `_yus/mcp/list` : `token` pour un service `pasteToken` | unitaire `ya_resources` | champ absent |

- **Cas 1a et 1b** : ils restent rouges après R5, puisque l'écriture se fait hors du pont et relève de E-C.
  - Une fois leur rouge relevé, ils passent en `#[ignore = "E-C : …"]` (décision 6).
  - **Écart à la décision 6** : elle ne nommait que le cas « jeton expiré ».
  - Un vrai jeton expiré que l'agent rafraîchit lui-même ne se teste pas hors ligne : les URL de jetons sont en dur (`crates/pa-ai/src/oauth/openai_codex.rs:21`, `anthropic.rs:20`) et seuls les quatre abonnements se rafraîchissent (`crates/pa-core/src/auth/provider_oauth.rs:190-198`). 1b teste ce qui est observable : un jeton renouvelé sur disque n'atteint pas la session vivante.
  - Les tests d'upstream injectent une intégration OAuth (`dad2232cc`), ce qui relève de E-C.
- **Squelette du push 1** :
  - `serve` répond des formes neutres : `{providers: []}`, `{providers: [], outcome: "done"}`, `{}` ;
  - `auth_failure_kind` rend `kind` tel quel ;
  - l'implémentation de `pa-cli` rend des listes vides et recopie le texte d'échec ;
  - `server_row` ignore `services`.
  - Chaque test compile et échoue sur son assertion.

### 16.4 Captures (après le code)

- `python3 bench/tools/capture/compare.py --all` sur le binaire de la branche.
- `acp_frames.py` puis `compare_frames.py` : trames d'ouverture inchangées (rien de plus envoyé à l'ouverture), et cas des nouvelles méthodes avec une clé factice.

### 16.5 Préfixe

- Prompt système et tools : inchangés.
- `Reload` après un login ne change que la clé et les en-têtes ; le corps reste le même, à vérifier par les captures (§16.4).

### 16.6 R5 seul : comportement visible dans l'extension actuelle

**Aucun changement visible attendu.**
- L'extension rejette sur place les `_yus/providers/*` (`extensions/vscode/src/ya/methods.ts:10-33`, `agent.ts:97`) et n'envoie jamais `login_cancel`.
- Le champ `token` ne sert qu'à la section du catalogue, masquée (`webview/SettingsMcp.tsx:15`, `:137`, `:262`).
- H2 change `kind` pour les deux textes de la garde :
  - « No API key found for » donne déjà le message fixe de l'extension, qui le reconnaît dans `details` (`src/ya/auth.ts:15`) ;
  - « Authentication failed for » ne sort que pour un identifiant OAuth sans source utilisable (`crates/pa-daemon/src/agent_engine/turn/model.rs:46-64`). `has_auth` ne regarde pas l'expiration (`crates/pa-core/src/auth/manager.rs:595-603`, `:623-625`) ; seules les marques « périmé » l'écartent, et elles ne sont posées qu'en test (`crates/pa-daemon/src/model_switch.rs:500`).
- **Pas de contrôle manuel pour R5.** Les scripts de contrôle (captures) sont lancés par Claude Code sur le binaire de la branche.

## 17. R5, push 1 : relevé des rouges (`101c74157`)

Run `ci` 45687309764 :
- `check + clippy (workspace)` et `windows cross-check + clippy` en échec sur un seul lint, `clippy::unused_async_trait_impl`, sur le `run` neutre du squelette (`crates/pa-daemon/src/acp/daemon/ya_providers.rs:255`). Le code du push 2 l'enlève, puisque `run` y attend le flux.
- Les shards de tests ont compilé et tourné. **Seuls les tests du ticket échouent, chacun sur son assertion** :

| Test | Job (run 45687309764) | Ligne | Assertion rouge |
|---|---|---|---|
| 1a `h1_an_api_key_rewritten_on_disk_reaches_the_running_session` | shard 4/8 | `crates/pa-cli/tests/ya_providers_acp_e2e.rs:391` | « H1: the running session kept the key it was built with » (précondition `:384` passée) |
| 1b `h1_an_oauth_token_renewed_on_disk_reaches_the_running_session` | shard 4/8 | `:420` | « H1: the running session kept the token it was built with » (précondition `:413` passée) |
| 2 `set_api_key_pushes_the_key_into_the_running_session` | shard 4/8 | `:444` | « the key set through the bridge serves the next request » |
| 3 `a_new_key_lists_its_models_in_the_running_conversation` | shard 4/8 | `:482` | « the picker lists ["openai","ya40-extra"] once its key is stored » (seule trame : la réponse du squelette) |
| 4 `list_gives_the_providers_screen_without_the_claude_subscription` | shard 4/8 | `:508` | « the ChatGPT subscription row » : `providers: []` |
| 5 `set_api_key_stores_the_key_and_never_answers_it` | shard 4/8 | `:531` | « the key in auth.json » : `Null` |
| 8 `a_key_set_through_the_bridge_is_nowhere_but_auth_json` | shard 4/8 | `:584` | précondition « the key is stored » : `Null` |
| 4 (unitaire) `the_rows_leave_out_the_claude_subscription` | shard 8/8 | `crates/pa-cli/src/print_runtime/ya_acp_providers.rs:117` | « the ChatGPT subscription row » |
| 8 (unitaire) `a_failed_flow_answers_a_fixed_text` | shard 8/8 | `:145` | « the token leaked: OpenAI Codex token exchange response missing fields: {"access_token":"sk-ya40-echoed"} » |
| 6 `a_sign_in_streams_its_steps_and_takes_the_answer` | shard 3/8 | `crates/pa-daemon/src/acp/daemon/ya_providers_tests.rs:74` | « a login_update frame within 5 s: Elapsed » |
| 6 `login_cancel_settles_the_sign_in_cancelled` | shard 3/8 | `:74` | idem |
| 7 `the_guard_texts_are_authentication_failures` | shard 3/8 | `:174` | « missing credential » : `"other"` au lieu de `"authentication"` |
| 13 `a_pasted_token_service_says_what_to_paste` | shard 3/8 | `crates/pa-daemon/src/acp/ya_resources.rs:596` | `token` : `Null` |

- Shards 3/8 : 1 254 réussis, 4 en échec ; 8/8 : 318 réussis, 2 en échec ; 4/8 : 7 en échec, les 7 du fichier. Les autres shards sont verts.
- `Changelog fragment` en échec : fragment ajouté au push 2 (`.changes/ya-40-constat.md`).

### 17.1 H1 confirmée

- 1a et 1b échouent sur l'assertion H1, après une précondition passée : la 1re requête porte bien la clé, ou le jeton, rangé au départ.
- La 2e requête ne porte pas la valeur réécrite dans `auth.json`.
- GitHub masque les valeurs `Bearer …` dans le journal (`left: "***"`). Au push 2, les messages donnent une étiquette non secrète par requête (`old`, `renewed`, `fallback`), pour montrer la valeur sans la clé.
- **Selon la décision 6, E-C devient le ticket suivant** (noté au journal du CDC).
- 1a et 1b restent rouges au push 2 (écriture hors du pont, relève de E-C). Ils passent en `#[ignore = "E-C"]` au push suivant, une fois l'étiquette relevée.

### 17.2 Écart au push 1, corrigé au push 2 : test 4

- Le test supposait que `ya40`, avec sa seule clé de repli dans `models.json`, apparaîtrait « connecté ».
- Or les lignes du TUI lisent le statut avec un `AuthStorage` sans résolveur de repli (`crates/pa-core/src/auth/manager.rs:249`, `:512-514`), et une clé de repli y compte comme non configurée (`configured: false`, `:521`).
- Correction : la clé de `ya40` est rangée dans `auth.json` avant le lancement, et le test vérifie `connected` et `removable`. Le rouge du push 1 portait sur une assertion antérieure (`:508`, ligne ChatGPT absente).

## 18. R5, push 2 : le code (`8991ea2a6`), relevé et captures

### 18.1 CI du push 2

Run `ci` 45689938071 :
- `check + clippy (workspace)`, `windows cross-check + clippy` et `fmt` verts ; les shards 1 à 3 et 5 à 8 verts.
- **Shard 4/8** : `ya_providers_acp_e2e`, 5 réussis, 2 en échec, **seulement 1a et 1b, comme prévu**.
  - `:409` : « H1: the running session kept the key it was built with; requests: ["old", "old"] ».
  - `:438` : « H1: the running session kept the token it was built with; requests: ["old", "old"] ».
  - **H1 est confirmée par la valeur** : la 2e requête porte la clé, ou le jeton, rangé au départ, et non celui réécrit dans `auth.json`.
- Les tests 2 (clé poussée dans la session vivante), 3 (modèles relus), 4, 5 et 8 sont verts, ainsi que les tests unitaires 4, 6, 7, 8 et 13.
- `Changelog fragment` vert.

### 18.2 Push 3

- 1a et 1b passent en `#[ignore]`, avec un texte qui renvoie à E-C (`crates/pa-cli/tests/ya_providers_acp_e2e.rs:396`, `:422`).
- Le journal du CDC note H1 confirmée : **E-C devient le ticket suivant** (décision 6).

### 18.3 Captures (règle 13 : lancées par Claude Code sur les binaires, avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`)

Binaires :
- `main` : `1ece0ff04`, artefact de `ya-build-macos-arm64` 45676155249 ;
- branche : `8991ea2a6`, artefact 45689927414, `yusagent --version` → `0.10.2-continuous.8991ea2a63c251ffc840e0ccbb4a4c983404422e`.

| Contrôle | Commande | Résultat |
|---|---|---|
| Capture `main` | `python3 -I bench/tools/capture/run.py --binary <main>/yusagent` | `runs/20261125T195616Z` |
| Capture branche | `python3 -I bench/tools/capture/run.py --binary <branche>/yusagent` | `runs/20261125T201240Z` ; chemins du préfixe `CONFORME` ; trames de l'extension `TOUT IDENTIQUE` |
| Toutes les requêtes, `main` contre branche | `python3 -I bench/tools/capture/compare.py runs/20261125T195616Z runs/20261125T201240Z --all --out <md>` | **TOUT IDENTIQUE** (premier tableau : seul `prompt_cache_key` change, l'UUID de session) |
| Trames ACP, branche | `python3 -I bench/tools/capture/acp_frames.py --binary <branche>/yusagent --out <neuf>` | `CONFORME` |
| Trames ACP, `main` | idem sur `<main>/yusagent` | `CONFORME` |
| Branche contre `main` | `python3 -I bench/tools/capture/compare_frames.py <branche> --against <main>` | `écarts : 1` : `.result.servers[].token`, en plus dans la branche (trame 32, `_yus/mcp/list`) |
| Idem, écart déclaré | `… --against <main> --expect '.result.servers[].token'` | **`écarts : 0`** ; `token` présent sur 11 services du catalogue (« GitHub token », « PagerDuty token », « Zoom token »…) |
| Branche contre l'agent factice | `python3 -I bench/tools/capture/compare_frames.py <branche>` | `écarts : 1` : le même `token`, absent de l'agent factice (à aligner avec X5) |

**Préfixe** :
- toutes les requêtes des cas du banc sont identiques entre `main` et la branche ;
- les trames d'ouverture ne changent pas ;
- seul `_yus/mcp/list` gagne `token`.

### 18.4 Écart au §16.4

- `acp_frames.py` n'a pas de session nouvelle pour `_yus/providers/*`.
- Une telle session demanderait l'agent factice de l'extension, côté X5.
- Les méthodes sont prouvées sur le vrai binaire par les tests e2e 2 à 5 et 8 (CI), avec un modèle intercepté et des clés factices.
