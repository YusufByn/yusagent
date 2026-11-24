# Ticket 31 : skills, MCP et réglages dans l'extension (constat et plan)

Demande de Yusuf du 23/11/2026, sur sa décision du 21/11/2026 (journal du CDC, `docs/yusAgent-cdc.md:95` ; ticket 24, « Règle pour skills, MCP et réglages » et étape 3 du découpage, `docs/tickets/24-ya10-extension-minimal.md:677-720`). Règle : un changement s'applique dès le message suivant, sans changer de conversation. Quand c'est possible, il est annoncé en fin d'historique et le préfixe reste intact ; sinon il coûte un seul miss, journalisé. Les clés figées pour la session (`yaRequest`, `editTool`, `shellTool`) sont hors de l'écran par défaut. `permissions` reste hors de ces écrans (D1).

Branche `ya-31-skills-mcp-reglages`, depuis `main` à `9f40fe5d8`. **Constat et plan seulement, aucun code**, rien compilé, aucun appel de modèle. Unharnessed a été lu dans un clone jetable du scratchpad, sans rien exécuter.

Les décisions de Yusuf du 23/11/2026 sont au §6. Elles remplacent les propositions des §1 à §5 quand elles diffèrent ; les §3.2, §3.4, §5.1 et §5.4 sont déjà mis à jour.

## 1. Construction du prompt système et des tools aujourd'hui

### 1.1 Une seule construction par session

- **Réglages lus une fois** à la création de la session (`crates/pa-core/src/session_engine/engine.rs:240`, `load_settings`). Les valeurs utilisées ensuite sont capturées entre `:296` et `:348`.
- **Prompt système construit une fois** (`engine.rs:781-830`). Il n'est plus jamais reconstruit : `set_system_prompt` (`crates/pa-agent/src/agent.rs:872`) n'a aucun appelant (`git grep -n set_system_prompt -- crates`).
- Dans le démon, le moteur est construit une fois par worker (`ensure_core_session_async`, `crates/pa-daemon/src/agent_engine/lifecycle.rs:259-270`). Il est reconstruit dans quatre cas : nouvelle session, remplacement (`lifecycle.rs:757-799`), reprise dans un nouveau worker après passivation (`crates/pa-daemon/src/worker/turn.rs:204-215`), redémarrage du worker.
- **`Reload` ne reconstruit ni le prompt ni les tools.** Il relit l'auth des requêtes et l'état MCP, rien d'autre (`crates/pa-daemon/src/agent_engine/reload.rs:42-60`). Commentaire du handler : « Settings and the model catalog resolve per use and need no re-read » (`crates/pa-daemon/src/session_custom.rs:306-311`).
- **Conclusion : en cours de session, le prompt système reste celui de l'ouverture.** Le cas « annonce » du ticket 24 tient (point à vérifier noté en `24-ya10-extension-minimal.md:686`).

### 1.2 Ordre du prompt (`crates/pa-core/src/prompts/system_prompt.rs:130-299`)

1. Préfixe statique : `core` v2 (`ya_prompt_v2::static_prefix`, avec `editTool`/`shellTool` et `existingTestsRule`), puis `model-prompts` (`:138-158`).
2. Queue dynamique, dans cet ordre (`:208-299`) : `packages`, `project-context` (AGENTS.md), `skills-inventory` (`:229-261`), `mcp-servers` (`:263-272`), `environment` (sans date ni cwd depuis T03, `:340-361`), `session-role`, `appended-prompt`.

La queue dynamique fait elle aussi partie du préfixe de chaque requête, puisqu'elle précède tout l'historique. Changer la liste des skills ou des serveurs MCP dans le prompt fait donc manquer le cache sur toute la requête.

### 1.3 Tools

- Définitions construites à la création : `python_repl`, puis les tools natifs selon `editTool`/`shellTool` (`engine.rs:700-731`). Ces deux clés sont figées dans l'en-tête de session (`crates/pa-core/src/session_engine/ya_tool_family.rs:14-21`, `:258-273`).
- **Les tools MCP ne sont pas des définitions de tools.** Le modèle les appelle par l'objet `mcp` du REPL (`system_prompt.rs:450`). Ni les skills ni les serveurs MCP ne changent donc la liste des tools.

### 1.4 Skills

- **Chargement** à la création, par `load_resources` (`engine.rs:349-366`). Les skills viennent de `~/.yusagent/skills`, de `<cwd>/.yusagent/skills`, de `.agents/skills` et des tableaux `skills` des réglages (écriture : `crates/pa-core/src/settings/manager.rs:470-505` ; fusion : `crates/pa-core/src/settings/merge.rs:60-74`).
- **Filtres appliqués** :
  - les skills liés à un gestionnaire absent (`model_visible_skills`, `crates/pa-core/src/session_engine/runtime_wiring.rs:209-218`) ;
  - les skills `linear` et `notion` quand on n'est pas connecté à ces services (`-<server>/SKILL.md`, `crates/pa-core/src/mcp/mod.rs:579-588` ; catalogue intégré `mod.rs:57-59`).
- **Dans le prompt** : inventaire v2, une ligne par skill : ``- `name` (import `x`): description `place` `` (`crates/pa-core/src/prompts/ya_prompt_v2.rs:160-200`).
- **Skills Python** : ils sont importés dans le kernel à son démarrage (`runtime_wiring.rs:220-231`). Un skill Python ajouté en cours de session n'est donc pas importable dans le kernel déjà lancé. Seul son `SKILL.md` est lisible.
- **`GetResourceSnapshot`** renvoie `engine.skills`, la liste capturée à la création (`crates/pa-daemon/src/agent_engine/session_engine_impl.rs:1115-1150`), pas l'état du disque. Un écran qui doit montrer les skills actuels doit relire le disque lui-même.
- Le cœur n'a **ni `list_skills` ni `set_global_value`** (ceux qu'utilise Unharnessed). Seuls `set_global_resource_array` (`manager.rs:470`) et des setters par clé (`manager.rs:177-614`) existent.

### 1.5 MCP

- **Configuration** : `mcpServers`, lu dans les réglages globaux seulement (`engine.rs:162-175`, « MCP execution is user/global only »). Le catalogue intégré (`linear`, `notion`) passe par l'auth.
- **Dans le prompt** : la liste « Enabled generic MCP servers: … » est capturée à la création (`engine.rs:336-348`, puis `:814` ; texte dans `system_prompt.rs:435-460`).
- **En direct** : le gestionnaire MCP du worker relit `mcpServers` sur disque à chaque résolution (`lifecycle.rs:76-94`). Le `mcp` du kernel voit donc un serveur ajouté ou retiré sans reconstruction. Le prompt, lui, ne le dit pas.
- **Commandes du démon** (`crates/pa-daemon/src/mcp_connections.rs`) :
  - `get_mcp_connections` (`:28-85`) : relit l'auth, rafraîchit, puis renvoie la liste, les services du catalogue et les identifiants. Lectures locales seulement (`:1-8`).
  - `set_mcp_static_token` (`:90-147`) : vérifie le serveur par une poignée de main.
  - `remove_mcp_connection` (`:150-183`).

### 1.6 Annonce en fin d'historique : le chemin prévu ne suffit pas

Le ticket 24 prévoyait la commande `AppendCustomMessage` (`24-ya10-extension-minimal.md:685`, `:719`). Lecture du code :

- **Ce que fait la commande.** Le worker écrit la ligne dans le fichier de session, puis la diffuse aux clients (`session_custom.rs:17-31` ; `persist_custom_row` et `broadcast_custom_row_locked`, `crates/pa-daemon/src/worker/summary.rs:195-299`).
- **Ce qu'elle ne fait pas.** Rien sur ce chemin n'ajoute la ligne au contexte du moteur déjà construit. Les seuls appelants de `append_messages` et de `queue_next_turn_row` sont `compaction_arms.rs:471` et `goal_boundary.rs:49`.
- **Effet, lu et à confirmer par le test 1 du §5.2.** Le modèle ne voit pas l'annonce au message suivant. Elle n'entre dans le contexte qu'à une reconstruction depuis le fichier (`lifecycle.rs:481-556` ; `crates/pa-core/src/session/mod.rs:430`, puis `convert_to_llm`, `crates/pa-core/src/session_engine/messages.rs:344-380`, où elle devient un message `user`). Elle tomberait alors au milieu de l'historique : un miss de plus à la reprise.
- **Chemin qui marche déjà** : la file « tour suivant » du moteur (`queue_next_turn_row`, `crates/pa-core/src/session_engine/admission.rs:201-208`). Les lignes passent dans la requête juste avant le message de l'utilisateur (`take_next_turn_rows`, `:239-258`, consommé en `:46` et `:145`). C'est le chemin de `[python-skills-unavailable]` (`skills_unavailable_notice.rs:36-50`, adopté en `engine.rs:1114`) et de l'objectif (`goal_boundary.rs:49`).

### 1.7 Journal des invalidations (existe déjà)

Le journal d'usage, à côté du fichier de session, a un champ `invalidation` par appel. Causes existantes :

- `model_change` et `thinking_level_change` (`crates/pa-core/src/session_engine/usage_log.rs:369-374`) ;
- `resume_prefix_change` avec `parts: ["system", "tools"]`, quand l'empreinte du premier appel après un redémarrage diffère du dernier appel journalisé (`:376-380`, `:510-547` ; empreinte `PrefixPrint`, `:265-300`) ;
- la cause notée par le masquage (`note_invalidation`, `:491`, appelé en `masking.rs:177`).

**Le miss à la reprise après un changement de skills ou de MCP est donc déjà journalisé** (ticket 28, C8). Il reste à noter, par `note_invalidation`, les changements appliqués en direct qui touchent les requêtes sans être annoncés.

### 1.8 Les clés de réglage, une par une

Lecture : un worker relit `SettingsManager::create(cwd, agent_dir)` sur disque à chaque usage (`manager.rs:40-62`). Il n'y a ni cache ni observateur de fichiers. Le moteur, lui, capture à sa création ce qu'il prend (§1.1).

Classes utilisées :
- **N** : appliquée au message suivant, sans effet sur le préfixe (relue à chaque usage, ou réglable en direct sans toucher aux requêtes).
- **A** : appliquée au message suivant par une annonce, préfixe intact.
- **M** : applicable au message suivant seulement en cassant le préfixe (un miss, à journaliser).
- **F** : figée pour la session par conception, donc nouvelle conversation seulement.
- **C** : capturée à la création sans être figée. La session ouverte ne la voit pas ; une reprise après passivation la prend en silence. Pour l'appliquer au message suivant, il faut un réglage en direct (N si sans effet sur le préfixe, M sinon).

| Clé | Lue | Effet sur ce qui part | Classe | Écran (proposition) |
|---|---|---|---|---|
| `skills` (tableaux), dossiers de skills, `enableSkillCommands`, `enableBuiltinSkills`, `bundledSkills.*` | création (`engine.rs:349-366`) | inventaire du prompt ; imports du kernel | C, donc **A** (annonce) | onglet Skills |
| `mcpServers` (global) | prompt à la création (`engine.rs:336-348`) ; connexions par usage (`lifecycle.rs:76-94`) | liste du prompt ; `mcp` en direct | C pour le prompt, donc **A** | onglet MCP |
| connexion à `linear`/`notion` (auth) | création (`mcp/mod.rs:579-588`) | fait apparaître ou disparaître le skill du service | **A** | onglet MCP |
| `retry.*` | par tour ou par erreur (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:12-25`) | aucun | **N** | oui |
| `auxiliaryModel` | par usage (`auxiliary_model.rs:67-75`) | appels annexes | **N** | oui |
| `imageModel`, `images.blockImages` | par tour avec image (`crates/pa-daemon/src/image_route.rs:41-44`) | modèle du tour image | **N** | **non**, les images sont masquées (§5.4 du ticket 24) |
| `allowedModels`, `idleEvictionMinutes`, `telemetry.*`, `factory.enabled`, `mcpCatalogSources` (global) | par usage | aucun | N | non (rien à régler depuis l'extension) |
| `compaction.enabled`, `reserveTokens`, `keepRecentTokens`, `thresholdPercent` | création, puis `set_compaction_settings` (`engine.rs:296-301`, `:1046-1056`) | moment et découpe des compactions, pas le préfixe d'ici là | C ; **N** si appliquée en direct par `set_compaction_settings` | `compaction.enabled` oui (Unharnessed) ; les seuils non |
| `harnessRefinements` (global) | à chaque requête (`refinement_gate.rs:44-53`) | filtre de l'historique | M | non |
| `masking.*` | création (`engine.rs:333-335`) | historique masqué | C, M en direct | non |
| `cacheRetention` | création (`engine.rs:330-332`) | TTL de `cache_control` (effet d'un changement sur le cache **non mesuré**) | C | non |
| `kernel.envAllowlist`, `kernel.maxOutputChars`, `pythonCellTimeoutMs`, `shellPath`, `shellCommandPrefix` | création du kernel (`engine.rs:245-258`, `:314-318`) | env du kernel, coupe des sorties | C ; en direct, il faudrait redémarrer le kernel (état perdu) | non |
| `bashNoticeBatchSeconds`, `autoRefine.*`, `compaction.agentCallable`, `requestTiming`, `agentTraces.enabled` | création | historique ou journal | C | non |
| `rlmMaxDepth` (global) | création (`rlm_off.rs:44-52`, puis `engine.rs:807-812`) | prompt (section des sous-agents) | C, M en direct | non |
| `defaultModel`, `defaultProvider`, `defaultThinkingLevel`, `defaultServiceTier` | création, puis entrées du fichier de session | modèle, effort, palier | session : sélecteurs `model`, `thought_level`, `fast_mode` (déjà dans le compositeur) | non (le sélecteur suffit) |
| `reasoningSummary`, `bashWaitSeconds`, `promptVersion`, `codexSessionIdHeader`, `existingTestsRule` (`yaRequest`) ; `editTool`, `shellTool` | création, figées dans l'en-tête (`ya_request.rs:55-66`) | prompt, tools, corps, en-tête HTTP | **F** | **non** (règle du 21/11) |
| `permissions` (global) | création ; en direct par le sélecteur ACP | aucun sur le préfixe (ticket 20 §5) | hors écran (D1) | **non** ; refusée par `settings/set` |
| `steeringMode`, `followUpMode`, `enabledModels`, `sessionDir`, et les clés du TUI (`theme`, `terminal.*`, …) | worker, CLI ou TUI | aucun | — | non |

Clés citées par le classement sans lecteur hors des tests : `transport`, `thinkingBudgets`, `autonomous`, `providerBackupModel`, `subagentDefaultModel` (déduit par `git grep`).

**Le cas « un seul miss » en pratique.** Aucune clé proposée pour l'écran n'est de classe M. Le miss reste donc celui de la **reprise après reconstruction**, quand skills ou MCP ont changé, y compris par une annonce (le prompt reconstruit prend l'état courant). Ce miss est déjà journalisé (§1.7). Une reprise dans le même worker (conversation rouverte avant la veille de 90 min) garde le moteur et son prompt : pas de miss. Lu, à vérifier par capture (cas L4 et L5 du §3.3).

## 2. Unharnessed : skills, MCP et réglages

Clone `github.com/unharnessed-agent/Unharnessed`, HEAD `f8e4658dd` (22/11/2026). Entre `157dcdc1d` (base de notre import) et HEAD, `vscode/` et `crates/uh-daemon/src/acp/` ne changent que par `vscode/package.json` : notre import (`extensions/vscode/`) a les mêmes écrans.

### 2.1 Méthodes ACP

Aiguillage : `crates/uh-daemon/src/acp/mod.rs:444-455`, vers `settings.rs`, puis `mcp_servers.rs`, puis `skills.rs`.

- **Réglages** :
  - `_unharnessed/settings/get` renvoie `{ values, defaults, restart }`, avec les valeurs du fichier global seulement (`settings.rs:325-374`).
  - `_unharnessed/settings/set {key, value}` : `null` supprime la clé (`:209-221`). Il passe par une **liste blanche** `KEYS` (`:55-158`) ; une autre clé reçoit `-32602 "unknown setting: {name}"`. La validation est faite clé par clé (`:222-312`) et un refus n'écrit rien (`:376-383`).
- **Skills** : `_unharnessed/skills/{list,set,create,add_folder,remove_folder}`. Toutes renvoient `{ skills[{name, description, path, location, source, enabled, active}], folders, problems, restart }` (`skills.rs:165-196`).
  - `set` écrit `-<dossier>` dans le tableau global `skills` (`:233-264`).
  - `create` écrit `<agent_dir>/skills/<name>/SKILL.md` (`:266-310`).
- **MCP** : `_unharnessed/mcp/{list,set_token,logout}`, qui renvoient `{ servers[...] }` (`mcp_servers.rs:129-172`).
  - `list` rafraîchit d'abord le catalogue Prime, la « Marketplace » : un appel réseau d'après le commentaire, au plus une fois par heure (`:6-7`, `:87`).
  - On ajoute ou retire un serveur par `settings/set mcpServers` (`:22-23`).
- **Aucune notification, rien dans l'historique, aucune annonce au modèle** (lu dans les trois modules).

### 2.2 Écrans (repris chez nous, masqués : `extensions/vscode/src/ya/methods.ts:31-41`)

- `webview/Settings.tsx` a 4 onglets : General, MCP, Skills, Instructions (`:13-20`).
  - **General** : `compaction.enabled`, `auxiliaryModel`, `retry.*`, `retry.failover.*`, `retry.provider.waitForUsage.*`, `imageModel`, `images.autoResize`, `images.blockImages` (`GROUPS`, `:38-139` chez lui).
  - Une valeur égale au défaut est envoyée à `null`.
- `webview/SettingsSkills.tsx` :
  - groupes « Yours », « This workspace », « Built in » ;
  - recherche, « New skill », un interrupteur par skill, section « Folders » ;
  - texte « Changes apply when it restarts. »
- `webview/SettingsMcp.tsx` :
  - sections « Your servers » et « Marketplace » ;
  - formulaire de serveur (URL ou commande), interrupteur, jeton, déconnexion ;
  - texte « …Changes apply when it restarts. »
- `webview/SettingsInstructions.tsx` : `modelInstructions`, des instructions par modèle **ajoutées au prompt système**.
- Côté hôte, `src/settings.ts` : les constantes de méthode sont en `:9-18`. Il compte ce qui attend un redémarrage, affiche « Some changes apply when the agent restarts. » et le bouton « Restart agent », qui relance le processus sur le fichier de la conversation (sa reprise).

### 2.3 Ce qu'on reprend et les écarts

| Sujet | Unharnessed | Proposition | Raison de l'écart |
|---|---|---|---|
| Noms des méthodes | `_unharnessed/{settings,skills,mcp}/*` | **`_yus/{settings,skills,mcp}/*`** (décision 1) ; formes des réponses reprises | identité yusAgent ; l'extension est adaptée (`src/settings.ts:9-18`) |
| Liste blanche, validation, `null` = défaut, global seulement | oui | repris, avec **nos** clés (§1.8) | ses clés ne sont pas toutes les nôtres |
| Moment où le changement s'applique | redémarrage de l'agent, qui donne un miss complet non journalisé | **message suivant, annonce** | règle du 21/11 |
| Bouton « Restart agent », textes « …when it restarts » | oui | retirés pour skills et MCP ; plus aucune clé « restart » à l'écran (§1.8) | idem |
| Onglet Instructions (`modelInstructions`) | ajoute du texte au prompt système | **masqué** | la clé n'existe pas chez nous (`git grep -i modelInstructions -- crates` vide) ; son équivalent (`model-prompts`) est dans le préfixe statique (`system_prompt.rs:150-157`). Une modification ferait un miss complet |
| Marketplace (catalogue Prime, réseau) | oui | **catalogue local seulement** : `linear`, `notion`, `mcpCatalogSources` (`get_mcp_connections`) | pas d'appel réseau depuis la liste ; `PI_OFFLINE` |
| Permissions | aucune | sélecteur du compositeur (§4) | D1, C6 du ticket 30 |
| Journal de l'extension | `settings: <key> set to <valeur JSON>` (`src/settings.ts:77`) | la clé seulement pour `mcpServers` | règle 9 : une config de serveur peut porter un en-tête ou une variable d'environnement secrète |
| Images | `imageModel`, `images.*` | masqués | les images ne partent pas (§5.4 du ticket 24) |

## 3. Annonces en fin d'historique

### 3.1 Où l'annonce est posée et qui la calcule

**Décision 2 : le moteur annonce, à l'admission de chaque prompt.** Juste avant `take_next_turn_rows` (`admission.rs:46`, `:145`), un module du fork compare deux états :
- ce que le modèle sait : le prompt de l'ouverture, plus les annonces déjà faites ;
- l'état sur disque : skills visibles et serveurs MCP génériques activés, avec les mêmes fonctions que la création (`load_skills`, `prompt_gating`).

S'il y a une différence, il pousse une ligne par `queue_next_turn_row`. La ligne part juste avant le message de l'utilisateur. Le prompt système et les tools ne bougent pas, et l'historique ne fait que s'allonger.

- **Écart au plan du ticket 24** (`24-ya10-extension-minimal.md:685`, `:719`) : l'annonce passe par `queue_next_turn_row` et non par `AppendCustomMessage`, car ce dernier n'atteint pas le contexte du modèle (§1.6).
- **Couverture** : tout changement est vu, qu'il vienne des écrans, de l'édition de `settings.json` à la main, d'un dossier de skill ajouté dans un terminal ou d'un skill que l'agent a créé lui-même. Cela vaut pour l'extension, le TUI et `-p`.
- **Diff calculée au prompt** : allumer puis éteindre entre deux messages n'annonce rien.
- **Persistance** : la ligne passe par le tour et sera écrite comme les autres lignes du tour. Une reprise dans le même worker garde donc le même historique (à vérifier : cas L4).
- **Coût** : §3.4.

### 3.2 Textes exacts (validés par Yusuf le 23/11/2026, décision 3)

Les textes sont fixes, sans date ni compteur, pour que l'historique reste identique d'une requête à l'autre. Une ligne de skill reprend le format de l'inventaire v2 (`ya_prompt_v2.rs:194`). Son chemin est celui de `skill_location` (`crates/pa-core/src/prompts/session_paths.rs:52-73`) : `$PRIME_AGENT_BUNDLED_SKILLS_DIR/…` pour un skill fourni, chemin absolu sinon.

#### `[skills-changed]` (texte reçu par le modèle)

Ajout d'un skill Markdown :

```
[skills-changed]
Added (read its SKILL.md before using it):
- `deploy`: Deploy the app to staging. `/Users/<utilisateur>/.yusagent/skills/deploy/SKILL.md`
```

Ajout d'un skill Python (décision 4 : pas d'import automatique, mais la marche à suivre pour le charger) :

```
[skills-changed]
Added (read its SKILL.md before using it):
- `pdf-tools` (Python package `pdf_tools`, not imported in this kernel): Extract text from PDF files. `/Users/<utilisateur>/.yusagent/skills/pdf-tools/SKILL.md`
To import a Python skill added here, run in a cell:
import importlib, os, shutil, subprocess, sys
subprocess.run([shutil.which("uv") or os.path.expanduser("~/.local/bin/uv"), "pip", "install", "--python", sys.executable, os.path.expandvars("/Users/<utilisateur>/.yusagent/skills/pdf-tools")], check=True)
importlib.invalidate_caches(); import pdf_tools
```

Retrait :

```
[skills-changed]
Removed (no longer available, do not use): `old-helper`, `linear`
```

Ajout et retrait dans la même annonce : la section `Added` vient d'abord, puis `Removed`. Un skill dont la description, le chemin ou l'import change apparaît dans une section `Changed (read its SKILL.md again):`, au format d'`Added`. Une section vide n'est pas écrite.

Pourquoi cette commande de chargement :
- Le bootstrap installe les skills Python par `uv pip install --python <python du kernel> --editable <dossier>` (`crates/pa-core/src/kernel/bootstrap/venv.rs:206-218`).
- Une installation en mode `--editable` passe par un fichier `.pth`, qu'un interpréteur déjà lancé ne relit pas. Ce point est déduit, pas vérifié. C'est pourquoi l'annonce propose une installation normale, dans le `site-packages` déjà sur `sys.path`.
- `uv` est cherché dans le `PATH`, puis dans `~/.local/bin/uv` (`crates/pa-core/src/kernel/bootstrap/venv/uv.rs:79-95`).
- La commande est en Python pur, sans `bash()` : elle marche aussi avec `shellTool: native`.
- **Installation normale, pas `--editable`** (décision de Yusuf du 23/11/2026).
- **À vérifier par test dans R3** (test 14, §5.2) : sur un vrai kernel, un skill Python créé en cours de session s'importe avec les lignes de l'annonce.

**Limites :**
- **Un skill Python modifié après son installation en cours de session n'est vu qu'au redémarrage du kernel.** L'installation normale copie le paquet dans `site-packages`, et le module reste en mémoire une fois importé.
- L'installation construit le paquet avec son backend (`hatchling` pour les skills fournis, `skills/*/pyproject.toml`). `uv` le prend dans son cache, rempli par le démarrage du kernel, sinon sur le réseau. Hors ligne et avec un cache vide, le chargement échoue. C'est déduit, pas mesuré.

#### `[mcp-changed]` (texte reçu par le modèle)

Ajout :

```
[mcp-changed]
Generic MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.
Enabled generic MCP servers now: `github`, `slack`.
Added: `github`.
```

Retrait :

```
[mcp-changed]
Generic MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.
Enabled generic MCP servers now: none.
Removed (no longer available, do not use): `slack`.
```

- La phrase sur l'objet `mcp` reprend celle du prompt (`system_prompt.rs:450`, raccourcie). Elle figure toujours, parce que le prompt de l'ouverture n'a pas de section MCP quand aucun serveur n'était activé (`:443-445`).
- Une connexion ou une déconnexion à `linear` ou `notion` montre ou masque le skill du service (§1.4). Elle est annoncée dans `[skills-changed]`.
- Si skills et serveurs MCP changent ensemble, une seule ligne de l'historique porte `[skills-changed]` puis `[mcp-changed]`, séparés par une ligne vide.

#### Ligne custom et ligne visible

- **Ligne custom** : `customType` `ya-resources-changed`, `display: true`.
  - Son champ `details` porte `{"skills": {"added": [...], "removed": [...], "changed": [...]}, "mcp": {"added": [...], "removed": [...]}}`.
  - Le texte visible ci-dessous est calculé à partir de `details`, jamais du texte reçu par le modèle.
- **Au TUI (R3)** : la ligne custom s'affiche dans le panneau générique, avec le texte complet reçu par le modèle (`crates/pa-tui/src/custom_message/mod.rs:163-185`).
- **Dans l'extension (X3)** : une ligne discrète, juste au-dessus du message de l'utilisateur (ou en tête d'un tour lancé par l'agent), en anglais comme le reste de l'interface. Un seul texte partout, sans variante (décision C2 du 24/11/2026, §9.2) :
  - ajout : `Skills updated: added deploy. Sent to the agent.`
  - retrait : `Skills updated: removed old-helper, linear. Sent to the agent.`
  - ajout, modification et retrait : `Skills updated: added a; changed c; removed d. Sent to the agent.` (dans cet ordre, une partie vide n'est pas écrite)
  - MCP, ajout et retrait : `MCP servers updated: added x; removed y. Sent to the agent.`
  - skills et MCP dans la même annonce : les deux lignes, skills d'abord.
- **Le pont doit transmettre cette ligne.** Aujourd'hui, le pont n'envoie au client aucune ligne custom : `wire_events.rs:74-128` ne traduit que les messages de l'assistant et les tools. R3 ajoute une notification `_yus/notice {sessionId, kind: "resources", details}`. La même ligne doit être rejouée à `session/load` (`ya_history.rs`).

### 3.3 Vérifier que le préfixe reste intact

**Script de capture** `bench/tools/capture/acp_live_changes.py`, sur le modèle d'`acp_resume.py`. Il est appelé par `run.py` comme `acp-resume` (`bench/tools/capture/run.py:662-698`, appel et verdict) et le run échoue si le verdict est NON CONFORME. Pont ACP réel, provider factice, `sandbox-exec`, démon isolé. Cas :

| Cas | Déroulé | Attendu |
|---|---|---|
| L1 skill ajouté | prompt 1 ; `_yus/skills/create` ; prompt 2 | requête 2 : empreintes système et tools identiques à la requête 1 (`PrefixPrint`) ; messages = ceux de la requête 1 + réponse + `[skills-changed]` + prompt 2 ; aucune `invalidation` au journal d'usage |
| L2 serveur MCP ajouté | prompt 1 ; `_yus/settings/set mcpServers` ; prompt 2 | idem, avec `[mcp-changed]` |
| L3 aller-retour | prompt 1 ; skill éteint puis rallumé ; prompt 2 | aucune annonce ; requête 2 = suite exacte de la requête 1 |
| L4 reprise, même worker | L1, fermeture du pont, `session/load`, prompt 3 | requête 3 = suite exacte (préfixe et annonce identiques) ; aucune `invalidation` |
| L5 reprise après reconstruction | L1, arrêt du worker (`yusagent shutdown`), `session/load`, prompt 3 | **un seul** miss : `resume_prefix_change`, `parts: ["system"]` dans le journal d'usage ; requête 4 = suite exacte de la requête 3 |
| L6 `permissions` | `_yus/settings/set permissions` | refus ; fichier global inchangé |

Le cas L5 est le seul « miss journalisé » : aucune clé de classe M n'est à l'écran (décision 5).

Contrôle du défaut : `compare_capture_prompts.py` entre `main` et la branche sur les cas existants. Attendu : TOUT IDENTIQUE, car rien ne change sans changement de skills ou de MCP.

### 3.4 Coût de la comparaison avec le disque (décision 2)

À chaque prompt, le module relit ce que la création lit pour l'inventaire et la liste MCP, et rien d'autre : ni AGENTS.md, ni prompts, ni thèmes.

**Lectures par prompt, d'après le code :**

| Source | Accès disque | Où |
|---|---|---|
| Réglages | 2 lectures (global, projet) | `SettingsManager::create`, `crates/pa-core/src/settings/manager.rs:40-62` |
| Chaque dossier parcouru dans une racine de skills | 1 `is_dir` ; 3 tentatives de lecture de `.gitignore`, `.ignore`, `.fdignore` ; 1 ou 2 `read_dir` ; 1 `metadata` par entrée | `crates/pa-core/src/skills/discovery.rs:158-170`, `:238-330` |
| Chaque skill | 1 lecture de `SKILL.md` ; 1 `is_file` de `pyproject.toml`, plus le test de `src/<nom>/__init__.py` pour un skill Python ; 1 `canonicalize` | `discovery.rs:35-60` ; `crates/pa-core/src/skills/loader.rs:89` |
| Racines | `~/.yusagent/skills`, `<cwd>/.yusagent/skills`, chemins du tableau `skills`, `~/.agents/skills` et les `.agents/skills` des dossiers parents, skills fournis | `loader.rs:126-127` ; `crates/pa-core/src/packages/resolve/auto.rs:55` ; `crates/pa-core/src/packages/resolve/discovery.rs:420` |
| MCP | 1 lecture du magasin d'auth (`linear`, `notion`) ; `mcpServers` vient de la lecture des réglages ci-dessus | `crates/pa-core/src/mcp/mod.rs:540-578` |

**Volume sur cette machine** (estimation : on compte les fichiers, on ne mesure pas les accès). Commande reproductible :

```sh
for d in ~/.yusagent/skills ./.yusagent/skills ./.agents/skills ./skills; do [ -d "$d" ] && printf "%s : dossiers=%s SKILL.md=%s pyproject=%s\n" "$d" "$(find -L "$d" -type d -not -path '*/.*' -not -path '*/node_modules*' | wc -l | tr -d ' ')" "$(find -L "$d" -name SKILL.md | wc -l | tr -d ' ')" "$(find -L "$d" -maxdepth 2 -name pyproject.toml | wc -l | tr -d ' ')" || echo "$d : absent"; done
```

Sortie du 23/11/2026 : `~/.yusagent/skills`, `.yusagent/skills` et `.agents/skills` absents ; `./skills` (skills fournis du dépôt, pris pour ceux du binaire) : `dossiers=36 SKILL.md=13 pyproject=9`.

Ce qui donne, en estimation, par prompt :
- 13 lectures de `SKILL.md` ;
- 108 tentatives d'ouverture de fichiers d'exclusion (36 × 3, presque toutes absentes) ;
- 36 à 72 `read_dir` et environ 150 `stat` ;
- 2 lectures de réglages et 1 lecture d'auth.

**Durée : non mesurée.** R3 la mesure sur le binaire construit en CI, sans build local (règle 8) :
- le module écrit la durée de chaque comparaison, en µs, dans la trace existante (`compaction_trace::trace`, activée par `PA_COMPACTION_TRACE`, comme `crates/pa-daemon/src/agent_engine/lifecycle.rs:498-511`) ;
- `acp_live_changes.py` lance le binaire avec cette variable et donne la médiane et le maximum sur ses prompts, avec la commande.

Si la durée gêne, une optimisation (ne relire que si l'heure de modification d'une racine a changé) se tranchera sur ce chiffre, pas avant.

### 3.5 Question : une clé capturée à la création est-elle vue par `resume_prefix_change` ?

**Non, sauf si elle change le prompt système ou les tools.**

- L'empreinte ne couvre que le prompt système et les tools : `PrefixPrint::of_context` (`crates/pa-core/src/session_engine/usage_log.rs:269-286`), comparée par `changed_since` (`:293-303`).
- Elle n'est prise que pour l'appel principal (`:914`). La comparaison n'a lieu qu'au premier appel après une reprise (`:528-541`).

Vues (elles changent le prompt système) : skills, `mcpServers`, `rlmMaxDepth`, `model-prompts`, AGENTS.md.

Non vues :
- **`masking.*`** : le masquage change les messages, pas le prompt. Il note une invalidation quand l'ensemble masqué change entre deux requêtes (`crates/pa-core/src/session_engine/masking.rs:175-178`). Mais il calcule les deux frontières avec la configuration **courante** (`:206-208`). Un masquage allumé ou réglé entre la fermeture et la reprise masque donc d'anciennes sorties dès la première requête, sans entrée au journal. C'est déduit de ces lignes, pas mesuré.
- **`harnessRefinements`** : il filtre l'historique à chaque requête (`crates/pa-core/src/session_engine/refinement_gate.rs:44-53`). Aucune invalidation n'est notée.
- **`cacheRetention`** : c'est un paramètre de requête, hors de l'empreinte (`engine.rs:330-332`). L'effet d'un changement de TTL sur le cache n'est pas mesuré.

Sans effet sur ce qui est renvoyé à la reprise, donc rien à voir : `compaction.*`, `autoRefine.*`, `bashNoticeBatchSeconds`, `kernel.maxOutputChars`. Ces clés n'agissent que sur la suite de la conversation.

→ **Ticket 33, « invalidations à la reprise non vues par l'empreinte »** (`masking`, `harnessRefinements`, `cacheRetention`) : constat d'abord. **Non traité dans le 31.**

## 4. Sélecteur de permissions dans l'extension (C6 du ticket 30)

- **Côté pont, rien à faire.**
  - L'option `permissions` est déjà publiée : `select`, catégorie `mode`, `manual` et `auto` (`crates/pa-daemon/src/acp/ya_permissions.rs:331-351`).
  - Elle s'applique par `session/set_config_option` (`apply_mode`, `:369-395`).
  - Chaque changement est journalisé dans le fichier de session (`ya-permission`, `crates/pa-core/src/session_engine/ya_permissions.rs:66-68`).
  - Le préfixe ne change pas (tests du ticket 20 : `crates/pa-core/tests/prefix_stability.rs:2425`, `crates/pa-daemon/tests/ya_permissions_same_requests_e2e.rs:430`).
- **Côté extension**, aujourd'hui :
  - `ConfigChange` ne connaît que `model`, `thought_level` et `fast_mode` (`extensions/vscode/src/protocol.ts:140-142`, `src/session.ts:211-220`) ;
  - l'étiquette « auto · no sandbox » n'est pas cliquable (`webview/Composer.tsx:525`, `:1005-1015`) ;
  - la valeur est lue dans `currentValue` (`src/session.ts:168-174`).
- **Décision 6** : l'étiquette du compositeur devient un menu à deux choix, dans X3, par `session/set_config_option` avec `configId: "permissions"`.
  - En manuel, une étiquette « manual » s'affiche aussi, sinon on ne pourrait plus repasser en auto.
  - Portée : la conversation, comme `/permissions` du TUI.
  - Le réglage global reste hors de l'écran (D1), et `_yus/settings/set` le refuse.

## 5. Découpage, tests, scripts, contrôle manuel

### 5.1 PR (décision 7)

**Une seule PR R3 (Rust) : annonces, skills, MCP et réglages.** Les annonces vont dans des commits à part.

- Commits prévus, chacun préfixé `YA-10: ticket 31 :` :
  1. tests seuls (push 1) ;
  2. **annonces** : `crates/pa-core/src/session_engine/ya_resource_notice.rs`, avec diff, textes du §3.2, ligne `ya-resources-changed` et durée dans la trace, plus l'appel dans `admission.rs` ;
  3. **annonces dans le pont** : notification `_yus/notice`, traduction dans `wire_events.rs` ou un module du fork, rejeu à `session/load` (`ya_history.rs`) ;
  4. **skills** : `crates/pa-core/src/skills/ya_list.rs` (liste du disque avec `enabled`/`active`, `set`, `create`, dossiers) et `_yus/skills/*` dans `crates/pa-daemon/src/acp/ya_resources.rs` ;
  5. **MCP** : `_yus/mcp/{list,set_token,logout}` dans `ya_resources.rs`, sur `get_mcp_connections`, `set_mcp_static_token` et `remove_mcp_connection` ;
  6. **réglages** : `crates/pa-daemon/src/acp/ya_settings.rs` (`_yus/settings/{get,set}`, liste blanche `retry.*`, `auxiliaryModel`, `compaction.enabled`, validation, refus de `permissions`, des clés figées et des clés inconnues) ;
  7. écriture des réglages globaux : fonction du fork à côté de `manager.rs`, qui écrit le JSON brut global puis recharge (`persist_scope_field` est privé, `manager.rs:507`) ;
  8. `compaction.enabled` appliqué en direct par `set_compaction_settings` (`engine.rs:1046-1056`) ;
  9. capture `bench/tools/capture/acp_live_changes.py` et son appel dans `run.py` ;
  10. doc : `docs/fork-changes.md`, journal du CDC, ce ticket.
- Fichiers upstream touchés, à lister dans `docs/fork-changes.md` :
  - `admission.rs` : un appel ;
  - `acp/daemon.rs` : aiguillage, comme `ya_history`, `:1143-1150` ;
  - `acp/wire_events.rs` : la notice, sauf si un module du fork suffit ;
  - `bench/tools/capture/run.py`.

**Puis X3 (extension) :**
- méthodes `_yus/*` dans `src/settings.ts:9-18` et servies (`src/ya/methods.ts`), donc `features.settings` passe à vrai ;
- textes « restart » et bouton retirés ; onglet Instructions et réglages d'images masqués ; onglet General limité aux clés de la décision 5 ;
- ligne visible du §3.2 sur `_yus/notice` ;
- journal sans valeur pour `mcpServers` ;
- sélecteur de permissions (§4) ;
- agent factice étendu à ces méthodes.

Pour chaque PR : push 1 avec les tests seuls, rouges sur leur assertion et seuls échecs du run ; push 2 avec le code. R3 touche `crates/pa-daemon/src/acp/` et X3 touche `extensions/vscode/`, donc le job `real-binary` tourne pour les deux (`.github/workflows/ya-extension.yml:115`, filtre de chemins).

### 5.2 Tests à voir en échec sur leur assertion (push 1)

R3, cœur (`crates/pa-core/tests/prefix_stability.rs`, provider factice) :
1. `skill_added_mid_session_is_announced_and_keeps_the_prefix` : skill écrit sur disque entre deux prompts. Attendu : empreintes système et tools égales, et le texte exact du §3.2 juste avant le prompt 2. Aujourd'hui : la ligne est absente.
2. `skill_removed_mid_session_is_announced` : attendu `Removed (no longer available, do not use):`. Aujourd'hui : rien.
3. `python_skill_added_mid_session_gives_the_load_steps` : attendu la ligne sans import et les trois lignes de chargement. Aujourd'hui : rien.
4. `mcp_server_added_mid_session_is_announced` : `mcpServers` modifié. Attendu : `[mcp-changed]`. Aujourd'hui : rien.
5. Vert dès le push 1 (non-régression) : `change_undone_before_the_prompt_announces_nothing`.

R3, pont (`crates/pa-cli/tests/ya_resources_acp_e2e.rs` et `ya_settings_acp_e2e.rs`, vrai binaire, provider factice) :

6. `skills_list_answers_the_screen_shape` : attendu `skills`, `folders`, `problems`. Aujourd'hui : `-32601`.
7. `skills_set_then_prompt_announces_once` : attendu une seule annonce sur deux prompts, et une notification `_yus/notice`. Aujourd'hui : `-32601`.
8. `mcp_list_and_logout` : attendu la liste locale. Aujourd'hui : `-32601`.
9. `settings_set_refuses_permissions` : attendu un refus D1 au texte exact. Aujourd'hui : `-32601`.
10. `settings_set_refuses_frozen_and_unknown_keys`.
11. `settings_set_compaction_enabled_applies_without_restart`.
12. `notice_is_replayed_by_session_load`.
13. `append_custom_message_does_not_reach_the_live_context` : test du constat §1.6. Vert s'il confirme la lecture ; s'il est rouge, le plan est revu avant le code.

R3, kernel réel (`crates/pa-cli/tests/ya_resources_kernel_e2e.rs`) :

14. `python_skill_added_mid_session_imports_with_the_announced_steps` :
   - un skill Python est créé après le démarrage du kernel ;
   - au prompt suivant, le serveur local qui joue le modèle prend les lignes de chargement **dans l'annonce reçue** et les renvoie en cellule `python_repl` ;
   - attendu : l'annonce est là, et le résultat de la cellule montre un `import` réussi (une valeur exportée par le skill de test) ;
   - aujourd'hui : pas d'annonce, donc le test échoue sur l'assertion « la requête porte les lignes de chargement » ;
   - à voir en échec sur cette assertion au push 1, puis en succès au push 2, sur le vrai kernel de la CI (venv du kernel préparé par `ci.yml:344`).

X3 :

15. `test/unit/methods.test.ts` : `features().settings` vrai.
16. `test/unit/settings.test.ts` : méthodes `_yus/*` ; aucune clé « restart » après un changement de skill ; journal sans valeur de `mcpServers`.
17. `test/unit/agent.test.ts` : `withChange` pour `permissions`.
18. e2e sur l'agent factice (`test/e2e/suite.ts`) : l'interrupteur d'un skill envoie `_yus/skills/set` ; la ligne visible s'affiche sur `_yus/notice` ; le menu des permissions envoie `set_config_option permissions` ; le fixture des trames à l'ouverture ne change pas.
19. e2e sur le vrai binaire (`test/e2e/suite-real.ts`) : skill éteint puis prompt, et la ligne visible s'affiche ; passage en manuel, et la cellule suivante demande l'accord.

### 5.3 Scripts de contrôle (lancés par Claude Code sur le binaire de la branche, sortie jointe)

- `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short python3 bench/tools/capture/run.py --binary <dossier>/yusagent` : il inclut `acp_live_changes.py` (L1 à L6, avec la durée de comparaison) et `acp-resume`, avec verdicts CONFORME.
- `compare_capture_prompts.py` entre `main` et la branche : TOUT IDENTIQUE sur les cas existants.

### 5.4 Contrôles manuels prévus pour Yusuf

**R3, au TUI** (binaire de la branche ; R3 ne touche pas l'extension). Le skill de test est créé à la main : `mkdir -p ~/.yusagent/skills/demo-x` puis un `SKILL.md` avec `name: demo-x` et `description: Demo skill.` en en-tête.

1. `yusagent`, puis « liste tes skills ».
2. Créer `demo-x` dans un autre terminal, puis « quels skills as-tu ? ». Attendu : un panneau `[skills-changed]` au texte du §3.2 ; `demo-x` cité ; même pid de worker (`ps -axo pid,command | grep 'yusagent worker'`).
3. `mv ~/.yusagent/skills/demo-x /tmp/`, puis un message. Attendu : `Removed (no longer available, do not use): demo-x`.
4. Remettre `demo-x`, puis le retirer encore, avant d'envoyer un message. Attendu : aucune annonce.
5. Ajouter un serveur dans `mcpServers` de `~/.yusagent/settings.json` (URL locale quelconque), puis un message. Attendu : `[mcp-changed]` ; `mcp.list_connections()` dans une cellule le montre.
6. Skill Python : copier un skill fourni qui a un `pyproject.toml` sous un autre nom dans `~/.yusagent/skills/`, puis « utilise-le ». Attendu : l'annonce donne les trois lignes, et l'agent importe le skill sans redémarrer.
7. Quitter, `yusagent shutdown`, `yusagent --resume`, puis un message. Attendu : une seule entrée `resume_prefix_change` dans le journal d'usage (`grep -c resume_prefix_change ~/.yusagent/sessions/<id>.usage.ndjson` ; chemin corrigé le 24/11/2026, §9.6).

**X3, dans l'extension** (binaire de la branche R3 mergée, `.vsix` de X3) :

1. Réglages : onglets General, MCP et Skills ; pas d'onglet Instructions ; General ne montre que `retry.*`, `auxiliaryModel` et `compaction.enabled` ; aucun texte « restart » ; `permissions` absent.
2. Skills > New skill `demo-y`, puis « quels skills as-tu ? ». Attendu : `Skills updated: added demo-y. Sent to the agent.` au-dessus du message (texte de la décision C2, §9.2 ; le contrôle X3 à jour est au §9.7) ; `demo-y` cité.
3. Éteindre `demo-y`, puis un message. Attendu : `Skills updated: removed demo-y. …`.
4. MCP : ajouter un serveur, puis un message. Attendu : `MCP servers updated: added <nom>. …`.
5. General : `retry.enabled` éteint, puis un message. Attendu : aucune ligne ; aucune entrée `invalidation` au journal d'usage.
6. Menu de l'étiquette du compositeur : `manual`, puis une cellule demande l'accord ; retour en `auto`.
7. Fermer le chat, puis le rouvrir depuis l'historique. Attendu : les lignes visibles des étapes 2 à 4 sont rejouées.

### 5.5 Préfixe de prompt

Aucun changement par défaut : prompt système et tools identiques, sans changement de skills ni de MCP (vérifié par `compare_capture_prompts.py`). Quand un changement a lieu, une ligne s'ajoute en fin d'historique et le préfixe reste intact.

## 6. Décisions de Yusuf (23/11/2026)

1. **Noms** : `_yus/skills/*`, `_yus/mcp/*`, `_yus/settings/*`. C'est le harness de Yusuf : tout ce qui relève de l'identité dans le code repris d'Unharnessed passe à yusAgent (noms de méthodes, clés, chaînes, identifiants). La notice MIT et l'attribution dans `docs/fork-changes.md` restent. L'extension importée est adaptée pour appeler ces noms (X3).
2. **Le moteur annonce à chaque prompt** (§3.1). Coût de la comparaison : §3.4. La médiane et le maximum seront mesurés par `acp_live_changes.py` sur le binaire de la branche.
3. **Textes des annonces : validés** tels que proposés au §3.2 :
   - skills Markdown et Python, retrait, section `Changed` ;
   - MCP, ajout et retrait ;
   - une seule ligne d'historique quand skills et MCP changent ensemble ;
   - la phrase sur l'objet `mcp`, toujours présente ;
   - les lignes visibles, en anglais, par `_yus/notice`, rejouées à `session/load`.
4. **Skill Python ajouté en cours de session** : annonce sans import, qui dit au modèle comment le charger. Installation normale, pas `--editable`. Le test 14 est vu en échec sur son assertion, puis en succès, sur un vrai kernel. Limite : un skill Python modifié après son installation en cours de session n'est vu qu'au redémarrage du kernel (§3.2).
5. **Onglet General** : seulement les clés sans miss, `retry.*`, `auxiliaryModel` et `compaction.enabled` appliquée en direct. Aucune clé qui coûte un miss.
6. **Sélecteur de permissions** : menu sur l'étiquette du compositeur, dans X3 (§4).
7. **Découpage** : une seule PR R3 (annonces, skills, MCP, réglages), avec les annonces dans des commits à part, puis X3. R3 est contrôlée au TUI, X3 dans l'extension (§5.4).
8. **Reprise après reconstruction** : le miss est accepté ; il est déjà journalisé par `resume_prefix_change`.
9. **Onglet Instructions** : masqué.
10. **Catalogue MCP** : local seulement.

### Écart au plan du ticket 24

L'annonce passe par `queue_next_turn_row` (`admission.rs:201-208`), et non par `AppendCustomMessage` comme prévu au ticket 24 (`24-ya10-extension-minimal.md:685`, `:719`). Ce dernier écrit la ligne dans le fichier et la diffuse aux clients, mais elle n'atteint pas le contexte du modèle (§1.6). À porter au journal du CDC avec la PR R3.

### Tickets à part, non traités dans le 31

- **Ticket 32, « identité yusAgent »** : les noms Unharnessed déjà sur `main`, à passer à yusAgent comme dans la décision 1. Cela couvre `_unharnessed/sessions/delete` du ticket 28 et tout le reste : méthodes, clés, chaînes, identifiants, dont `_meta.unharnessed`, le nom `saved` et `turnStart` si Yusuf les range dans l'identité. Constat d'abord, avec la liste `fichier:ligne`.
- **Ticket 33, « invalidations à la reprise non vues par l'empreinte »** : `masking.*`, `harnessRefinements` et `cacheRetention` changés avant une reprise ne sont pas journalisés (§3.5). Constat d'abord.

## 7. Plan des pushs de R3 (validé par Yusuf le 23/11/2026, avec trois ajouts)

Branche `ya-31-skills-mcp-reglages`. La PR s'ouvre **prête dès le push 1** : un run en brouillon saute les jobs lourds.

### 7.1 Push 1 : tests seuls, plus le ticket et le script de contrôle

Aucun code du produit et aucune ébauche : les tests passent par le binaire (ACP), par le disque ou par l'API du moteur qui existe déjà.

| Commit | Contenu | Attendu au run du push 1 |
|---|---|---|
| 1 | ce ticket | — |
| 2 | `crates/pa-core/tests/prefix_stability.rs` (vraie session, vrai kernel, serveur local ; en CI, échoue si le kernel manque, `:175-179`) : tests 1 à 5 et 14 | **rouges sur leur assertion** : 1, 2, 3, 4, 14 ; **vert** : 5 |
| 3 | `crates/pa-cli/tests/ya_resources_acp_e2e.rs` et `ya_settings_acp_e2e.rs` (vrai binaire, serveur local qui joue le modèle) : tests 6 à 12 | **rouges** : 6 à 10 et 12 sur la forme de la réponse ou le texte exact, car le pont répond aujourd'hui `-32601` ; 11 sur la compaction (§7.3) |
| 4 | `crates/pa-daemon/tests/ya_announce_path_e2e.rs` (modèle : `ya_permissions_same_requests_e2e.rs`) : test 13 | **vert** (constat du §1.6 confirmé) ; s'il est rouge, arrêt et retour vers Yusuf avant tout code |
| 5 | `bench/tools/capture/acp_live_changes.py`, autonome (`run.py` ne l'appelle pas encore) | lancé par Claude Code sur le binaire du push 1 : NON CONFORME sur L1, L2, L3, L4 et L6 (L3 passe aussi par `_yus/skills/set`, absent au push 1), CONFORME sur L5 (skill écrit sur disque) ; sortie jointe à la PR |

Run attendu : seuls les tests 1 à 4, 6 à 12 et 14 échouent, chacun sur son assertion. fmt et clippy sont verts. Le job `real-binary` ne tourne pas : aucun chemin de son filtre n'est touché. Un commentaire de la PR liste ces échecs attendus.

### 7.2 Push 2 : le code, un sujet par commit

| Commit | Contenu | Tests au vert |
|---|---|---|
| a | annonces, moteur : `session_engine/ya_resource_notice.rs` (diff, textes du §3.2, ligne `ya-resources-changed`, durée dans la trace) et les points d'accroche du §7.4 | 1 à 5, 14 |
| b | annonces, pont : `acp/ya_notice.rs` (`_yus/notice`, §7.4) ; rejeu à `session/load` dans `ya_history.rs` (fichier du fork) | 12, et la moitié « notice » du 7 |
| c | skills : `skills/ya_list.rs`, `acp/ya_resources.rs` (`_yus/skills/*`), aiguillage | 6, 7 |
| d | MCP : `_yus/mcp/{list,set_token,logout}` dans `ya_resources.rs` | 8 |
| e | réglages : `acp/ya_settings.rs` (liste blanche `retry.*`, `auxiliaryModel`, `compaction.enabled`), écriture du global (fonction du fork), `compaction.enabled` relu à chaque décision (§7.3) | 9, 10, 11 |
| f | appel d'`acp_live_changes.py` dans `run.py`, comme `acp-resume` | — |
| g | doc : `docs/fork-changes.md` (fichiers upstream du §7.4), journal du CDC (décisions du 23/11, écart `queue_next_turn_row` contre `AppendCustomMessage`), réalisation dans ce ticket | — |

Run attendu : tous les checks requis verts. `real-binary` tourne (`crates/pa-daemon/src/acp/` change) et l'extension, inchangée, reste verte.

**Après le push 2, avant la remise**, Claude Code lance sur le binaire de la branche (`ya-build-macos-arm64.yml`, `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`). Les sorties sont jointes à la PR.
1. `run.py`, qui inclut `acp_live_changes.py` : L1 à L6 CONFORME, médiane et maximum de la durée de comparaison.
2. `compare_capture_prompts.py` entre `main` et la branche : TOUT IDENTIQUE sur les cas existants.
3. **Ajout 1 (le pont change)** : `acp_frames.py --binary <main>/yusagent --out <scratch>/frames-main` et `acp_frames.py --binary <branche>/yusagent --out <scratch>/frames-branche`, puis `python3 -I bench/tools/capture/compare_frames.py <scratch>/frames-branche --against <scratch>/frames-main`.
   - Attendu : `écarts : 0`. Aucun cas d'`acp_frames.py` ne change de skills ni de MCP, donc aucune notice ne part.
   - Si un cas en produit une, elle est déclarée par `--expect <chemin de clé>` et la raison est notée dans la PR.

Ensuite, contrôle de Yusuf au TUI (§5.4). Merge seulement sur `gh pr checks` vert du dernier commit.

Risque à surveiller : le test 14 installe le skill de test, qui demande `hatchling`. Pris dans le cache de `uv`, rempli par `ci.yml:344`, sinon sur le réseau. Une panne réseau avant compilation est une panne d'infrastructure : une relance du job seul, notée dans la PR.

### 7.3 Ajout 2 : voir le test 11 rouge sur la compaction, pas sur `-32601`

**Une entrée existe déjà : le fichier `~/.yusagent/settings.json` lui-même.**
- `thresholdPercent` y est déjà relu à chaque décision de compaction. Commentaire du code : « another client's settings save applies to an already-running session » (`crates/pa-core/src/session_engine/wiring.rs:166-178`).
- `enabled`, lui, reste celui de la création (`auto_compaction_enabled`, `wiring.rs:160-165`, réglé une fois en `engine.rs:1046-1056`).
- La commande RPC d'upstream `set_auto_compaction` écrit aussi le fichier avant le réglage en mémoire (`crates/pa-daemon/src/rpc/commands.rs:537-545`) : le fichier est bien la source.

**Test 11 réécrit** : `compaction_disabled_in_the_settings_file_applies_without_restart`.
- Session ACP ouverte, prompt 1.
- Écriture directe de `"compaction": {"enabled": false}` dans le fichier global.
- Le serveur local annonce un usage proche de la fenêtre, puis prompt 2.
- Attendu : aucune requête de compaction.
- Aujourd'hui, la compaction part, donc **rouge sur l'assertion qu'il doit garder**.
- Il passe au vert si `enabled` est relu à chaque décision, comme `thresholdPercent`.

`_yus/settings/set` n'est alors qu'une écriture du fichier. Son refus et son écriture restent couverts par les tests 9 et 10, plus un cas d'écriture ajouté au 10.

**Conséquence sur le plan** : `compaction.enabled` n'est plus appliqué par `set_compaction_settings` depuis le pont. Il est relu sur disque dans `wiring.rs`, ce qui est une **modification d'upstream** (§7.4) à valider. Elle vaut aussi pour le TUI et pour une édition à la main, comme `thresholdPercent`.

### 7.4 Ajout 3 : fichiers upstream touchés, lignes exactes

**`wire_events.rs` n'est pas touché.**
- La boucle des événements du pont ajoute déjà les mises à jour des modules du fork (`updates.extend(ya_permissions.updates(&event))`, `crates/pa-daemon/src/acp/daemon.rs:404`).
- Un module du fork `acp/ya_notice.rs` fait de même : il repère le `message_start` de la ligne `ya-resources-changed` et rend une mise à jour.
- Forme de la notice : `session_info_update` avec `_meta["_yus/notice"] = {kind: "resources", details}`, publiée par le producteur, comme `turnStart` (`ya_turns.rs:213-231`). Elle suit ainsi l'ordre et le tampon d'ouverture des autres mises à jour.
- Une notification JSON-RPC à part passerait hors du producteur. Elle sortirait de l'ordre (`eventSequence`) et du tampon tant que `session/new` n'a pas répondu (`producer.rs:112-135`).
- Le rejeu à `session/load` passe par `ya_history.rs`, fichier du fork.

Lignes upstream ajoutées (commentaire `yusAgent` compris) :

| Fichier | Où | Lignes | Pourquoi |
|---|---|---|---|
| `crates/pa-core/src/session_engine/admission.rs` | avant `:46` (`prompt_injected_message`) et avant `:145` (prompt de l'utilisateur), juste avant `prompt_messages.extend(self.take_next_turn_rows().await)` | 2 × 2 = **4** | comparer avec le disque et mettre l'annonce en file avant de vider la file |
| `crates/pa-daemon/src/acp/daemon.rs` | après `:404` | **2** | `updates.extend(super::ya_notice::updates(&event));` |
| `crates/pa-daemon/src/acp/daemon.rs` | avant `:1152` (`"session/set_config_option"`), sur le modèle de l'arm `ya_history` (`:1142-1151`) | **4** | aiguillage de `_yus/skills/*`, `_yus/mcp/*` et `_yus/settings/*` vers `ya_resources` |
| `crates/pa-daemon/src/acp/mod.rs` | après `:19` | **4** | `mod ya_notice;` et `mod ya_resources;` (`ya_settings` est un sous-module de `ya_resources`) |

**Fichiers upstream que le plan validé ne nommait pas**, nécessaires à la décision 2. Le moteur doit savoir ce que le modèle a reçu à l'ouverture, et rien ne garde la liste MCP du prompt :

| Fichier | Où | Lignes | Pourquoi |
|---|---|---|---|
| `crates/pa-core/src/session_engine/mod.rs` | après `:70` ; après `:212` ; après `:282` | **6** | `pub mod ya_resource_notice;` ; champ `ya_known_resources` à côté de `ya_tool_family` (même précédent, YA-03) ; son initialisation |
| `crates/pa-core/src/session_engine/engine.rs` | avant `:781` (copie de `generic_mcp_servers`, déplacé en `:814`) ; après `:1076` (`set_skills`) | **4** | inscrire les skills visibles et les serveurs MCP du prompt d'ouverture |
| `crates/pa-core/src/session_engine/wiring.rs` | `:160-165` (corps d'`auto_compaction_enabled` : 1 ligne au lieu de 4) ; dans le bloc `if let` de `:172-176` | **environ 5** | §7.3 : `enabled` relu sur disque comme `thresholdPercent` |
| `bench/tools/capture/run.py` | après l'appel d'`acp_resume` (`:662-698`) | environ 8 | commit f |

Total, estimé : environ 37 lignes ajoutées dans 7 fichiers upstream, 0 dans `wire_events.rs`. Tous listés dans `docs/fork-changes.md` (commit g).

### 7.5 Décisions de Yusuf sur les §7.3 et §7.4 (23/11/2026)

1. **`compaction.enabled` relu sur disque** dans `wiring.rs`, comme `thresholdPercent`, avec le test 11 réécrit (`compaction_disabled_in_the_settings_file_applies_without_restart`). `_yus/settings/set` devient une simple écriture du fichier.
   - **Candidat à une PR upstream plus tard** : la relecture de `compaction.enabled` sur disque.
2. **Notice** : `session_info_update` avec `_meta["_yus/notice"]`, publiée par le producteur comme `turnStart`, rejouée à `session/load` par `ya_history.rs`.
3. **Fichiers upstream ajoutés validés** : `acp/mod.rs`, `session_engine/mod.rs`, `session_engine/engine.rs`, `session_engine/wiring.rs`.
   - Tous seront listés avec `fichier:ligne` dans `docs/fork-changes.md`, au commit de doc.
   - `engine.rs` touche la zone du prompt d'ouverture : `compare_capture_prompts.py` doit donner TOUT IDENTIQUE sur les cas existants (§7.2, point 2).

## 8. Réalisation (PR #83)

### 8.1 Push 1 : tests seuls (`1f55754f0`, run `ci` 45196437703)

- **Rouges sur leur assertion, seuls échecs du run** : tests 1, 2, 3, 4, 14 (shard 6), 6, 7, 8, 12 (shard 8), 9, 10, 11 (shard 5). fmt, clippy et la vérification de compilation sont verts.
- **Verts** : test 5 (shard 6) et test 13 (shard 5, constat du §1.6 confirmé).
- **Build macOS du push 1** : aucun runner (« The job was not acquired by Runner of type hosted even after multiple attempts »), relancé une fois, second échec identique. Le script `acp_live_changes.py` a tourné sur le binaire de `main` à `9f40fe5d8`, équivalent faute de code du produit (décision de Yusuf) :
  - NON CONFORME sur L1, L2, L3, L4 et L6 : `-32601` ;
  - L5 conforme : une seule `invalidation` `resume_prefix_change`, `parts: ["system"]`.
  - L3 est NON CONFORME parce qu'il exige que ses deux appels réussissent. C'est attendu (décision de Yusuf : L3 reste tel quel). Sortie jointe à la PR.

### 8.2 Push 2 : écarts au plan validé (§7), signalés à Yusuf

1. **Écrans dans le pont, pas dans `pa-core`.** La logique des skills et des réglages est dans `crates/pa-daemon/src/acp/ya_resources.rs` et `ya_resources/settings.rs`, avec la seule API publique de `pa-core` (`load_resources`, `SettingsManager`, `FileSettingsStorage`, `InMemorySettingsStorage`, `set_global_resource_array`). Le §7.2 prévoyait `crates/pa-core/src/skills/ya_list.rs` : cela aurait demandé une déclaration de plus dans `skills/mod.rs` (upstream).
2. **`engine.rs` : 15 lignes au lieu d'environ 4.** `rustfmt` étale l'appel de `Tracker::at_creation` sur 13 lignes. `config` étant déjà en partie déplacé à cet endroit, ses champs sont passés un à un.
3. **`acp/daemon.rs` : 11 lignes au lieu de 6.** L'aiguillage prend 9 lignes, sur le modèle de celui de `ya_history`.
4. **Règle de `compaction.enabled`.** Le fichier de réglages s'impose quand sa valeur a changé depuis la dernière lecture ; sinon, le réglage vivant (`set_auto_compaction_enabled`) reste. Une session sans suivi (construite hors de `create_session_with`) ne lit pas le disque. Raison : le test d'upstream `saved_percentage_applies_to_existing_sessions_without_changing_manual_budgets` (`crates/pa-core/src/session_engine/compaction_threshold_tests.rs:108`) règle `enabled` en mémoire sans écrire le fichier.
5. **`ya_prompt_v2.rs`** (fichier du fork) : `shown_skill` sort une ligne de l'inventaire, pour que les annonces reprennent exactement le format du prompt. La sortie de `skills_inventory` est inchangée.
6. **Défauts de `_yus/settings/get`** : seul celui de `compaction.enabled` (`true`, `get_compaction_enabled`) est donné ; les autres sont `null`. À compléter en X3 si l'écran en a besoin.

### 8.3 Limites (lues ou déduites, non mesurées)

- Un skill Python modifié après son installation en cours de session n'est vu qu'au redémarrage du kernel (§3.2).
- La relecture du disque à chaque prompt passe par `load_resources`. Un paquet configuré manquant serait installé au prompt suivant, comme à la création (`crates/pa-core/src/packages/resolve/manager.rs:129`, « installing missing configured package sources »).
- Une annonce mise en file avant un tour lancé par l'agent (avis `[bash-done]`, `admission.rs:46`) est la première ligne de ce tour. L'origine du tour (`ya_turns::origin_of`) pourrait alors perdre son genre (`bash`). Déduit, à vérifier.
- Une session en prompt v1 reçoit ses annonces au format et avec le filtre de l'inventaire v2.
- Ajout du 24/11/2026 (X3, §9.3) : `_yus/mcp/set_token` n'est accessible depuis aucun écran. Le pont ne donne jamais le champ `token` d'un service (`ya_resources.rs:441-448`), donc le formulaire de jeton de l'extension ne s'ouvre pas.
- Ajout du 24/11/2026 (écart 8 de X3, §9.3) : un prompt venu du TUI attaché au même worker, précédé d'une annonce, perd son origine `prompt`. Le pont annonce le tour d'un autre client à son premier `message_start`, ici la ligne d'annonce (`crates/pa-daemon/src/acp/ya_turns.rs:126-127`), et `origin_of` en fait un réveil sans genre (`:340-347`). Hors X3 : correction dans le pont, à faire plus tard.

### 8.4 Run du push 2 (`2b1c6c668`) et option 1 (décision de Yusuf du 24/11/2026)

- **Run `ci` 45207575832** :
  - clippy (Linux et Windows) : `clippy::single_match_else` dans `ya_resources/settings.rs:172` ;
  - shard 8 : tests 7 et 12 rouges (aucune notice) ;
  - tous les autres tests du ticket verts.
  - `ya-build-macos-arm64` 45207555254 et `ya-extension` 45207575429 verts.
- **Cause** : l'abonné du moteur du démon (`crates/pa-daemon/src/agent_engine/turn/run_once.rs:162-245`) ne transmet au worker ni les lignes custom de la file du tour suivant, ni donc l'annonce. Celle-ci atteignait le modèle, sans être écrite dans le fichier ni envoyée au pont.
- **Option 1, retenue** : avant les lignes d'avance d'un tour, le worker demande l'annonce au moteur (`SessionEngine::ya_resource_notice`). Le moteur la met dans sa file pour le modèle et la rend ; le worker l'émet avec les lignes d'avance, avant la ligne du prompt.
- **Garde-fou 1, une seule annonce par changement sur tous les chemins** :
  - **démon** (extension par le pont ACP, TUI, qui s'attache au même worker) : l'appel d'`admission.rs` ne trouve plus de différence. Test `a_skill_change_reaches_the_model_once_before_the_prompt_and_the_file_in_order` (`crates/pa-daemon/tests/ya_announce_path_e2e.rs`) : une seule ligne `[skills-changed]` dans la requête, juste avant le prompt ; test 7 pour la notice et la ligne du fichier ;
  - **`-p` et RPC sans démon** : seul `admission.rs` annonce, et le moteur écrit lui-même la ligne dans l'ordre de la requête (`persist_event`, `crates/pa-core/src/session_engine/mod.rs:412-438`). Tests 1 à 5 (`prefix_stability.rs`, moteur sans démon) ;
  - **TUI** : même worker que l'extension, donc mêmes tests ; l'affichage se contrôle à la main (§5.4).
- **Garde-fou 2, historique rechargé après reconstruction du worker** :
  - le test du démon ci-dessus vérifie que la ligne d'annonce précède la ligne du prompt dans le fichier ;
  - le cas L5 d'`acp_live_changes.py` vérifie qu'après l'arrêt du démon et la reprise, la première requête du prompt 3 commence par les messages de la dernière requête du prompt 2, annonce comprise à sa place.
- **Écarts au plan** : `engine.rs` (`pa-daemon`, 6 lignes), `session_engine_impl.rs` (11 lignes) et `worker/turn.rs` (3 lignes) s'ajoutent aux fichiers upstream, comme décidé. `ya_take_resource_notice` devient public dans `pa-core`. Correctifs de lints faits d'avance : `single_match_else` (`settings.rs`, `ya_resources.rs`), tableau de test sans mélange `&str` / `&String`.

### Tickets à part, ajout du 24/11/2026

- **Ticket 34, « lignes du tour suivant absentes du fichier du worker »** : dans le démon, les lignes custom de la file du tour suivant d'upstream (`[python-state-restored]`, suite d'objectif, `[python-skills-unavailable]`) atteignent le modèle sans être écrites dans le fichier de session ni envoyées aux clients (`run_once.rs:162-245`). À la reconstruction du worker, l'historique relu les perd : un miss non journalisé possible. Lu à `upstream/main` `798d97032` (23/11/2026) : `run_once.rs` ne les transmet toujours pas. Constat d'abord ; non traité dans le 31.

### 8.5 Push 3 (`8205f0bbe`), contrôles et contrôle de Yusuf (24/11/2026)

- **CI** :
  - run `ci` 45349487727 sur `8205f0bbe` : seul le shard 8 échoue, sur deux tests d'upstream sans lien avec le ticket, maintenant listés en « Instabilité probable » dans `docs/fork-changes.md` :
    - tentative 1 : `protocol_breadth_b6_b9`, même symptôme que sur `main` au run 43492287310 ;
    - tentative 2 (relance du job seul, sans push) : `terminal_state_differential_e2e`, superviseur factice, sans prompt ;
  - run `ci` 45358650280 sur `574ed9f03` (doc seule, aucun fichier sous `crates/` modifié depuis `8205f0bbe`) : vert ; les 16 checks requis par la règle `protect-main` réussissent.
- **Binaire** : `ya-build-macos-arm64` 45349481082, `8205f0bbe4db2d9bc1e13d3f8fdf8a99a3cb080c`, SHA256 vérifié.
- **Contrôles lancés par Claude Code sur ce binaire** (`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`, sorties jointes à la PR) :
  - `bench/tools/capture/run.py` : code de sortie 0, `acp-resume` CONFORME, `acp-live` CONFORME (L1 à L6). L5 relit l'historique à l'identique après la reconstruction du worker, avec un seul `resume_prefix_change`, `parts: ["system"]`. `spawn : ok=False` existait déjà sur `main` (tickets 24 et 28).
  - **Durée de la comparaison avec le disque : médiane 1 742 µs, maximum 19 258 µs, sur 28 prompts** (trace `ya.resource_notice.compare`).
  - `compare_capture_prompts.py` : smoke bench en capture, `py-example-001`, `main` `9f40fe5d8` contre la branche, `resolved` des deux côtés, 9 requêtes : TOUT IDENTIQUE.
  - `compare_frames.py --against` (`acp_frames.py`, `main` contre la branche) : `A-display-auto` (26 trames) et `B-basic-manual-reject` (14) IDENTIQUE, écarts : 0, sans `--expect`.
- **Contrôle de Yusuf au TUI** (8 étapes, binaire `8205f0bbe`) : conforme.
  - Même pid de worker aux étapes 3 et 7.
  - Aucune annonce sur l'aller-retour de l'étape 5.
  - Skill Python importé avec les lignes annoncées (`DEMO-PY-OK`).
  - Reprise après reconstruction : panneaux rejoués à leur place, un seul miss `resume_prefix_change`, `parts: ["system"]`.
  - Remise en état faite.
- **Constat hors ticket, non traité** : `yusagent --continue` lancé dans `/tmp/ya31-tui` ouvre le sélecteur de sessions au lieu de reprendre directement la dernière conversation du dossier.

### 8.6 Points ouverts

- **X3** (extension) : écrans sur `_yus/*`, ligne visible sur `_yus/notice`, sélecteur de permissions, onglet Instructions masqué, journal sans valeur de `mcpServers`, défauts de l'écran General (`null` sauf `compaction.enabled`).
- **Tickets à part** :
  - 32, « identité yusAgent » ;
  - 33, « invalidations à la reprise non vues par l'empreinte » ;
  - 34, « lignes du tour suivant absentes du fichier du worker ».
- **Limites du §8.3.**

## 9. X3 : écrans skills, MCP et réglages dans l'extension (24/11/2026)

Branche `ya-31-x3`, depuis `main` à `2e8dfdf41`. Constat et plan présentés à Yusuf le 24/11/2026, puis ses décisions (§9.2).

### 9.1 Constat

**L'extension importée a déjà les écrans, aux formes du pont.**
- Les constantes de méthode sont en `extensions/vscode/src/settings.ts:9-18`, encore en `_unharnessed/*`.
- Les types sont en `src/protocol.ts:144-217`.
- Les écrans : `webview/Settings.tsx` (4 onglets `:15-20`, groupes de l'onglet General `:38-141`, bloc « Some changes apply when the agent restarts » `:288-301`), `SettingsSkills.tsx` et `SettingsMcp.tsx`.
- Le bouton Settings est masqué tant que `features().settings` est faux (`src/ya/methods.ts:38`, `webview/Status.tsx:94`).
- Les réponses du pont ont les mêmes formes : skills `crates/pa-daemon/src/acp/ya_resources.rs:282`, MCP `:441-448`, réglages `ya_resources/settings.rs:112`. Les 13 clés de `KEYS` (`settings.rs:50-68`) sont les champs de l'onglet General, sans le groupe Images.

**Ce qui change.**
1. **Noms** : `_yus/*` dans `src/settings.ts` et `methods.ts`. Les autres noms Unharnessed restent pour le ticket 32.
2. **Lecture à l'ouverture de l'écran.** Aujourd'hui `Settings` lit tout dès que l'agent est prêt (`src/settings.ts:50`). Une fois les méthodes servies, cela ajouterait 3 trames à l'ouverture : fixture des trames cassé (preuve 1, ticket 24 §5.4) et E7 non tenu. La lecture partira du message `screen: "settings"` de la webview (`src/view.ts:143`).
3. **Plus rien sur le redémarrage** : calcul `pending` (`settings.ts:31-34`, `:236-257`), bloc et bouton de `Settings.tsx:288-301`, textes « …when it restarts ». L'étiquette `restart` d'un skill (`SettingsSkills.tsx:118-122`) part aussi : `active` vient de `GetResourceSnapshot`, l'état du moteur à sa création, que les annonces ne mettent pas à jour.
4. **Journal** : la clé seule pour `mcpServers` (`settings.ts:77`, règle 9).

**Notice.**
- Forme reçue : `session_info_update` avec `_meta["_yus/notice"]` à la racine de `_meta` (`ya_notice.rs:21`), à lire dans `_meta` brut comme `_yus/bashRuns` (`src/ya/cells.ts:27-28`).
- En direct : `turnStart(prompt)` (`daemon.rs:1621`), puis la notice, puis le contenu (capture `C-resources`, §9.5).
- Au rejeu : la notice ouvre le tour suivant, avant son prompt (`ya_history.rs:313-318`, `:370-376`).
- Elle devient une ligne visible au-dessus du message (`webview/Chat.tsx:270`), calculée à partir de `details`.

**Sélecteur de permissions.**
- `ConfigChange` ne connaît pas `permissions` (`protocol.ts:140`) ; `withChange` non plus (`session.ts:211-220`).
- L'étiquette `auto · no sandbox` n'est pas cliquable (`webview/Composer.tsx:1005-1015`, `:525`).
- Le pont n'a rien à faire (`ya_permissions.rs:369-395`).

**Défauts de l'onglet General.** Le pont donne `null` sauf `compaction.enabled` (`settings.rs:105-111`). Or `retry.enabled` vaut `true` par défaut (`crates/pa-core/src/settings/manager.rs:702`) : l'interrupteur s'afficherait éteint.

**Catalogue MCP.** La connexion d'un service du catalogue passe par `_unharnessed/providers/login` (`src/view.ts:355`, `providers.ts:134-153`), non servi. La capture `C-resources` montre en plus que le catalogue local n'est pas seulement `linear` et `notion` : toute la liste fournie (`mcp-services.bundled.json`), en `source: "catalog"`, `signIn: true`.

**Modèle de l'agent factice.** Ses valeurs (`capture/model-a`) n'avaient pas la forme du pont (`["p","id"]`, `config_options.rs:87`). `auxiliaryModel` en aurait fait `/capture/model-a`, refusé par le pont.

### 9.2 Décisions de Yusuf (24/11/2026)

- **C1** : option A. Le pont donne les défauts de l'onglet General à partir des getters du cœur, dans `ya_resources/settings.rs`, avec un test Rust vu en échec au push 1. Le binaire de la branche devient celui des contrôles.
- **C2** : un seul texte partout, sans variante : `Skills updated: added a; changed c; removed d. Sent to the agent.` et `MCP servers updated: added x; removed y. Sent to the agent.` (§3.2 mis à jour).
- **C3** : `Changes reach the agent with your next message.` (onglets Skills et MCP).
- **C4** : section du catalogue MCP masquée jusqu'au ticket du login dans l'extension, sans texte qui renvoie au terminal. **À reporter dans ce ticket à créer** : rouvrir la section du catalogue (connexion OAuth et jeton). `set_token` inaccessible : en limite au §8.3.
- **C5** : libellés du menu lus dans l'option du pont ; `permissions` devient `{current, choices}`.
- **C6** : infobulle de l'étiquette `manual` : `Permissions: manual. Each tool call asks first.`
- **C7** : agent factice aligné sur le pont (`["capture","model-a"]`).
- **C8** : cas `C-resources` dans `acp_frames.py`.
- **C9** : onglet Instructions masqué seulement ; suppression du code au ticket 32 ou avec le visuel.

### 9.3 Écarts acceptés et limites

Écarts acceptés par Yusuf :
1. X3 touche du Rust (C1), dans un fichier du fork.
2. L'agent factice passe au push 1, pour que les tests échouent sur leur assertion et non sur `-32601`.
3. Le test de `served` et `features` va dans `test/unit/meta.test.ts` (`methods.test.ts` du §5.2 n'existe pas).
4. Un seul texte de notice, y compris sur un tour sans message (C2).
5. Services du catalogue non connectables depuis l'extension (C4) ; `set_token` inaccessible.
6. Chemin du journal d'usage : `~/.yusagent/sessions/<id>.usage.ndjson` (§5.4 corrigé, §9.6).
7. Ticket 24 tenu : rien envoyé à l'ouverture (E7), aucun `set_config_option` sans action (§5.4), `permissions` hors des écrans (D1).

Écart 8 (prompt du TUI attaché qui perd son origine après une annonce) : en limite au §8.3, hors X3.

**Fichiers upstream** : aucun. X3 touche du code repris d'Unharnessed (`extensions/vscode/src`, `webview`), des fichiers du fork (`src/ya/`, agent factice, `bench/tools/capture/acp_frames.py`, `compare_frames.py`, `README.md` du banc) et `crates/pa-daemon/src/acp/ya_resources/settings.rs` (fork).

**Préfixe de prompt** : inchangé. Aucune trame de plus à l'ouverture ; prompt inchangé ; la notice n'est que de l'affichage ; le mode de permissions n'entre pas dans les requêtes (`prefix_stability.rs:2425`) ; C1 ne change qu'une réponse de `_yus/settings/get`.

### 9.4 Plan des pushs

PR ouverte prête dès le push 1. Les tests compilent sur les types actuels (casts, `permissionsOf` et `noticesOf` dans `test/helpers.ts`), pour échouer sur leur assertion et non sur `tsc`.

**Push 1 : tests seuls**, plus le ticket, le fragment de changelog (exigé : le test Rust est sous `crates/`), l'agent factice et les scripts de trames.

| Test | Fichier | Rouge au push 1 sur |
|---|---|---|
| R1 `the_view_gives_the_core_defaults` | `crates/pa-daemon/src/acp/ya_resources/settings.rs` | `defaults` à `null` au lieu des getters du cœur |
| U1 « méthodes du ticket 31 (X3) servies, réglages affichés » | `test/unit/meta.test.ts` | `served("_yus/settings/get")` faux |
| U2 à U5 | `test/unit/settings.test.ts` (nouveau) | méthodes `_yus/*` non envoyées ; « pas encore servie » au lieu de l'acceptation ou du texte de refus du pont |
| U6 « sélecteur de permissions (ticket 31) » | `test/unit/agent.test.ts` | `manual` non affiché avant la réponse ; `choices` absents |
| U7 « notice (ticket 31) » | `test/unit/agent.test.ts` | lignes visibles absentes du tour |
| E1 à E3 | `test/e2e/suite.ts` (agent factice) | bouton Settings caché ; `_yus/skills/create` jamais reçu ; étiquette non cliquable |
| E4, E5 | `test/e2e/suite-real.ts` (vrai binaire) | mêmes causes, sur le binaire de la branche |

Restent verts : le test des trames du fixture (`agent.test.ts:64`), les 59 tests unitaires existants, `compare_frames.py`.

**Push 2 : le code**, un sujet par commit :
1. défauts dans le pont ;
2. noms et lecture à l'ouverture de l'écran ;
3. retrait du « restart » et textes C3 ;
4. onglet General, Instructions masqué, catalogue masqué ;
5. notice ;
6. sélecteur ;
7. journal ;
8. doc (`docs/fork-changes.md`, ce ticket).

L'agent factice prend les défauts du binaire de la branche ; `acp_frames.py` et `compare_frames.py` sont relancés dessus.

### 9.5 Agent factice et trames : `acp_frames.py`, puis `compare_frames.py` (avant le push 1)

Binaire `8205f0bbe` (`ya-build-macos-arm64` 45349481082). Le code sous `crates/` est identique à celui de `main` : `git diff --stat 8205f0bbe 2e8dfdf41 -- crates` est vide.

```sh
export DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short
python3 -I bench/tools/capture/acp_frames.py --binary ~/.cache/yusagent-bin/8205f0bbe4db2d9bc1e13d3f8fdf8a99a3cb080c/yusagent --out <dossier neuf>
python3 -I bench/tools/capture/compare_frames.py <dossier>
```

- `acp_frames.py` : `A-display-auto` (26 trames), `B-basic-manual-reject` (14), `C-resources` (44, refus de `permissions`, skill créé, `manual` après le sélecteur, une notice), aucun processus restant, `CONFORME`, exit 0.
- Trames réelles relevées par `C-resources` :
  - `_yus/settings/get` : `defaults` à `null` sauf `compaction.enabled: true` ;
  - refus `-32602`, `data.details` = le texte D1 exact ;
  - `_yus/skills/*` : `{skills, folders, problems, restart: false}` ; `problems` contient le skill fourni `factory` sans description ;
  - `_yus/mcp/list` : le catalogue complet en `source: "catalog"` ;
  - option `permissions` : `Ask before each tool call` et `Run tool calls without asking (no sandbox)` ;
  - notice : `turnStart`, puis notice, puis contenu.
- `compare_frames.py` (agent factice de la branche) : 25 comparaisons `IDENTIQUE`, `écarts : 0`, exit 0. Premier lancement : 9 écarts, tous sur `problems` vide dans l'agent factice ; corrigé (le skill `factory` du binaire).
- **Contrôle négatif** : `--fake` sur l'agent factice de `main` (`git show 2e8dfdf41:extensions/vscode/test/fake-agent/fake-agent.mjs`) : 9 comparaisons `DIFFÉRENT` sur le cas C, `écarts : 167`, exit 1.

### 9.6 Correction

Contrôle TUI du §5.4, étape 7 : le journal d'usage est `~/.yusagent/sessions/<id>.usage.ndjson`, sans sous-dossier (contrôle de Yusuf ; `usage_log_path`, `crates/pa-core/src/session_engine/usage_log.rs:692-696`).

### 9.7 Contrôle manuel de Yusuf (X3, dans VS Code)

0. Avant de commencer :
   - binaire de la branche : `B=~/.cache/yusagent-bin/<sha du push 2>/yusagent` ;
   - `cp ~/.yusagent/settings.json ~/.yusagent/settings.json.avant-x3` ;
   - `extensions/vscode/scripts/install-vsix.sh <run ya-extension>` ;
   - dans VS Code, `"yusagent.binaryPath": "<valeur de $B>"`, puis « Reload Window » ;
   - `mkdir -p /tmp/ya31-x3 && code /tmp/ya31-x3`.
1. Nouvelle conversation. Attendu : étiquette `auto · no sandbox` cliquable. Envoie `bonjour` : ce premier message construit la session.
2. Settings. Attendu :
   - onglets General, MCP, Skills, sans Instructions ;
   - General : Compaction, Retries, Switching providers, Usage limits, sans Images ni `permissions` ;
   - « Retry failed requests » allumé (défaut du pont) ;
   - aucun texte « restart » ;
   - MCP sans section de catalogue.
3. Skills > New skill `demo-y`, description `Demo skill.`, puis « quels skills as-tu ? ». Attendu : `Skills updated: added demo-y. Sent to the agent.` au-dessus du message, et `demo-y` cité.
4. Éteins `demo-y`, puis envoie un message. Attendu : `Skills updated: removed demo-y. Sent to the agent.`
5. Rallume puis éteins `demo-y`, puis envoie un message. Attendu : aucune ligne.
6. MCP > Add a server `demo-mcp`, URL `http://127.0.0.1:9/mcp`, puis un message. Attendu :
   - `MCP servers updated: added demo-mcp. Sent to the agent.` ;
   - dans Output > yusAgent, `settings: mcpServers set`, sans valeur.
7. General : éteins « Retry failed requests », puis un message. Attendu : aucune ligne, et `grep -c '"invalidation"' $(ls -t ~/.yusagent/sessions/*.usage.ndjson | head -1)` donne `0`.
8. Clic sur l'étiquette, `Ask before each tool call`. Attendu : étiquette `manual`, avec l'infobulle `Permissions: manual. Each tool call asks first.` Puis « lance pwd dans une cellule » : la boîte s'ouvre, Reject. Clic sur `manual`, `Run tool calls without asking (no sandbox)`, et la même demande s'exécute sans boîte.
9. Ferme le chat, rouvre-le depuis l'historique. Attendu : les lignes des étapes 3, 4 et 6 rejouées à leur place.
10. Remise en état :
    - `mv ~/.yusagent/settings.json.avant-x3 ~/.yusagent/settings.json` ;
    - `rm -rf ~/.yusagent/skills/demo-y /tmp/ya31-x3`.

### 9.8 Push 1 : tests seuls (`8ff03bf04`, PR #84)

- **`ci`** run 45392710567 :
  - **rouge attendu**, shard 3 : `the_view_gives_the_core_defaults` (`ya_resources/settings.rs:313`, `assertion left == right failed`). Défauts rendus : `null` partout sauf `compaction.enabled: true`. Défauts attendus du cœur : `retry.enabled: true`, `maxRetries: 3`, `baseDelayMs: 2000`, `provider.maxRetryDelayMs: 60000`, `failover.enabled: true`, `failover.maxRetries: 3`, `failover.baseDelayMs: 1000`, `failover.maxDelayMs: 30000`, `waitForUsage.pauseUntilReset: true`, `maxPauseMs: 86400000`, `maxParks: 8`.
  - **rouge inattendu**, shard 2 : `the_history_lists_conversations_and_deletes_one_with_its_session` (`crates/pa-cli/tests/ya_acp_resume_e2e.rs:452`, « the window snapshot is there before the delete », seuls `<id>.info-cache.json` et `<id>.jsonl` présents). Test du ticket 28, sans lien avec ce push (aucun code du produit). Il n'est pas listé en « Instabilité probable » : **pas de relance**, signalé à Yusuf.
  - fmt, clippy (Linux et Windows) et les autres shards : verts.
- **`ya-extension`** run 45392710453 :
  - tests unitaires : 66, 59 verts, **7 rouges, tous attendus et sur leur assertion** :
    - « sélecteur de permissions » : `expected 'manual'`, `actual null` ;
    - « notice » : lignes attendues, `actual undefined` ;
    - « méthodes du ticket 31 (X3) servies » : `_yus/settings/get`, `false !== true` ;
    - « écrans » : `_yus/settings/get envoyée`, `false` ;
    - « interrupteur d'un skill » et « journal » : `accepté`, `actual '_unharnessed/… : méthode pas encore servie par yusAgent'` ;
    - « refus de permissions » : texte du pont attendu, même message reçu.
  - **e2e sur l'agent factice : non lancé.** L'étape est sautée parce que les tests unitaires, plus tôt dans le même job, échouent (`ya-extension.yml`, étapes en série). Les 3 cas du ticket 31 n'ont donc pas été vus rouges.
  - e2e sur le vrai binaire : 16/18. Seuls rouges, les 2 cas du ticket 31 : « délai dépassé : clic sur « Settings » » et « délai dépassé : clic sur « auto · no sandbox » ». Aucun processus restant.
- **`ya-build-macos-arm64`** run 45392698295 : succès (binaire du push 1, sans code du produit).
- **Changelog fragment**, codebase-health, contribution gate : verts.

### Ticket à part, ajout du 24/11/2026

- **Ticket 35, « test upstream instable : `abort_and_send_queued_acks_before_the_follow_up_delivery` »**. Constat d'abord ; non traité dans le 31.
  - **Origine** : upstream. Le test est dans `crates/pa-daemon/src/worker/turn_stream_tests/queue.rs:607`, depuis le portage en Rust (`39bc99a91`, #2524, 29/09/2026). Tous les commits qui touchent ce fichier viennent d'upstream ; aucun ne vient du fork.
  - **Symptôme** : run `ci` 45376832870 sur `main` à `2e8dfdf41` (24/11/2026), shard 3, job 138385395316. Panique en `queue.rs:725:9` : « the follow-up's turn never settled on the bare abort ». Après un `abort` seul, `worker.core.busy` reste vrai au-delà de 10 s (`:719-727`).
  - **Autres échecs** : aucun dans les 40 derniers runs `ci` en échec du dépôt (17/11 au 24/11/2026), recherche dans les journaux des shards en échec.
  - **Upstream depuis notre base** : `2e8dfdf41..upstream/main` touche ce fichier (5 commits, +177/-12 lignes), sans changer cette assertion.

### 9.9 Push 1 bis (`ca2e562f5`) et test du ticket 28

- **Push 1 bis** : `if: ${{ !cancelled() && steps.changes.outputs.run == 'true' }}` sur l'étape e2e de l'agent factice. La condition existante est gardée pour la sortie rapide hors du périmètre. On y ajoute le compte rendu du push 1 et la note du ticket 35.
- **`ya-extension`** run 45398633893 :
  - tests unitaires : les mêmes 7 rouges qu'au push 1 ;
  - e2e sur le vrai binaire : 16/18, les mêmes 2 rouges, aucun processus restant ;
  - e2e sur l'agent factice, lancé cette fois : 9/14.
    - **Les 3 cas du ticket 31 sont rouges sur leur assertion** :
      - réglages : « délai dépassé : clic sur « Settings » » (bouton masqué). Le « (issue : clicked) » du message est la sonde d'avant le clic : le texte du message est calculé avant l'attente ;
      - skills : « délai dépassé : _yus/skills/create reçu » ;
      - permissions : « délai dépassé : clic sur « auto · no sandbox » (issue : covered) ».
    - **2 rouges inattendus, sur des cas de la PR 3 (ticket 24)** :
      - « boîte de permission » : « y moins d'1 s après la dernière frappe : rien » (`suite.ts:100-103`, fenêtre de calme mesurée à l'horloge du test) ;
      - « cartes » : « délai dépassé : carte d'une cellule refusée » (`suite.ts:330-331`, clic sur le dernier « Worked » juste après le tour).
    - Le code de l'extension n'a pas changé ; l'agent factice, si (valeurs du modèle, comparaison au prompt sans annonce quand rien ne change), sans effet attendu sur ces deux cas, non prouvé. Ils ne sont pas listés en instabilité : pas de relance, signalé à Yusuf. Le dernier run où cette étape a vraiment tourné, et réussi, est celui du ticket 30 (45152332356, 23/11/2026) ; tous ceux d'après l'ont sautée (aucun changement de l'extension).
- **`ya_acp_resume_e2e`, `the_history_lists_conversations_and_deletes_one_with_its_session`** (point 1 de Yusuf, lecture seule) :
  - **Écrivain réel du snapshot de fenêtre.** C'est la construction du moteur au premier prompt de « one » (`crates/pa-daemon/src/agent_engine/lifecycle.rs:455-472`, `WindowedSessionStore::open`), qui écrit le fichier par `window_cache::save` (`crates/pa-core/src/session/window.rs:587`), de façon synchrone. Ni le scan (`session_scan.rs:61-72`, info-cache seulement), ni la création du worker (conversation neuve, pas d'ouverture en mode fenêtre, `worker/create.rs:228` est pour un fichier existant), ni la reprise de `holder` (même worker vivant) ne l'écrivent.
  - **Pourquoi il peut manquer.** L'ouverture en mode fenêtre renvoie `Ok(None)` sans rien écrire quand le fichier a changé pendant sa lecture (`window.rs:583-584`, contrôle de génération taille/mtime/ctime, `window_cache.rs:46-50`). Or le worker écrit les lignes du prompt pendant cette construction (commentaire `lifecycle.rs:480`, « The worker persisted accepted rows while this lazy session built »). Le moteur repasse alors par l'ouverture complète (`lifecycle.rs:466-471`) et le processus n'a aucun snapshot en mémoire. Ensuite, `flush` n'écrit qu'un snapshot en mémoire (`window_cache.rs:164-172`, appelé à la libération, `lease.rs:356-360`) : plus personne n'écrit le fichier pendant le test. Les autres sorties `Ok(None)` de `WindowedSessionStore::open` (`window.rs:225-559`) ont le même effet.
  - **Option retenue : le test dépose le fichier s'il manque.** Une attente ne peut pas réussir quand la construction l'a sauté.
  - **Aucun écrivain après la suppression.** La suppression arrête la session et attend la sortie de son worker (`ya_history.rs:793-808`) avant de retirer le fichier et ce qui porte son nom (`:826-830`). La libération du worker, seule autre écriture possible du snapshot, a donc lieu avant le retrait. Après la suppression, aucun worker ne tient « one » ; `session/list` ne lit pas un fichier retiré (le scan part des `.jsonl`) et n'écrit que l'info-cache, que le balayage couvre. Rien ne peut rendre le test instable dans l'autre sens.
  - Le fichier n'est déposé que s'il manque. Contenu `{}` : une ouverture qui le lirait le refuserait et repasserait par la lecture complète (`window_cache.rs:121-124`). Commit séparé (`f89ac3f59`), test seul, poussé avec le push 2.

### 9.10 Limite du produit notée (décision de Yusuf du 24/11/2026)

- **Un snapshot de fenêtre sauté à la construction du moteur n'est réécrit par personne tant que vit le worker** (§9.9 : `window.rs:583-584`, `lifecycle.rs:466-471`, `window_cache.rs:164-172`). La prochaine ouverture de la session fait donc une lecture complète du fichier. Coût en temps seulement, non mesuré ; à mesurer plus tard si utile.

### 9.11 Les deux rouges inattendus de l'e2e de l'agent factice (constat et décisions du 24/11/2026)

**Constat.**
- **L'agent factice de la branche ne change rien pour ces cas.** La même suite a été jouée contre l'agent factice de `main` (`2e8dfdf41`) et celui de la branche : initialize, session/new, `accord` (accord à 200 ms), `carte`, `refus`, en manuel. Résultat : 26 trames de chaque côté. Seule la réponse de `session/new` diffère : identifiant du fichier (pid) et valeurs du modèle (C7). Durées : main 206, 0 et 0 ms ; branche 207, 1 et 1 ms. Dans `onPrompt`, les branches `accord`, `carte` et `refus` sont intactes.
- **Pas de lancement local.** L'e2e local n'a pas pu tourner dans la session de Claude Code. `@vscode/test-electron` cherche `Contents/MacOS/Electron`, qui s'appelle `Code` dans VS Code 1.146 sur macOS. Avec un lanceur qui appelle `Code`, le processus est tué aussitôt par SIGKILL, signature valide ; cause probable : le bac à sable de la session.
- **Les causes sont des dépendances au temps dans nos tests** :
  - « boîte de permission » (`test/e2e/suite.ts:84-103`, avant correction) : les fenêtres de calme sont mesurées par la webview, depuis l'ouverture de la boîte et la dernière frappe. Le test, lui, les mesurait avec sa propre horloge (`opened = Date.now()` après le sondage, `sleep(opened + 800)`, `sleep(opened + 1150)`), avec entre les deux les allers-retours de la sonde (50 ms au moins chacun) et le lot du chat ;
  - « cartes » (`suite.ts:328-331`, avant correction) : clic sur le dernier « Worked » de la page juste après le tour. Si le lot du chat qui replie le tour `refus` n'est pas encore affiché, le clic referme le « Worked » du tour `carte`.

**Décisions de Yusuf.**
- Pas de lancements en série sur une branche jetable ; pas de liste « Instabilité probable » sans son accord.
- Version déterministe, en commit de tests séparé (`a11e88e28`, push 2) :
  1. **Boîte** : la sonde lit l'état de la boîte avec les fonctions de `src/ya/permission-keys.ts` (`armed`, nouvelle `readyAt`, `buttonsReady`) et agit dans le même appel.
     - Le clic grisé sur « Allow » et `y` sont joués dans l'appel même qui ouvre la boîte (`ProbeInput.atBoxOpen`, nouveau module `webview/ya-box-state.ts`).
     - Une frappe relance les touches mais pas les boutons : `a` puis `y` dans un seul appel (`y` toujours dans la fenêtre que `a` relance), et `keysReadyAt > buttonsReadyAt`.
     - Les boutons sont attendus prêts sur l'état de la boîte, puis `a` puis `y` : rien.
     - Plus aucun `sleep` sur l'horloge du test pour un « pas encore ». Les cas « Entrée » et « sans focus » sont inchangés.
  2. **Clics sur « Worked » juste après un tour.** Le test attend que la page ait autant de « Worked » que de tours non vides dans l'état du chat (`workedCount`), puis, après le tour, un « Worked » de plus, affiché et fermé (`openNewWork`), et clique sur celui-là. Endroits touchés :
     - `suite.ts` : boîte, étape 2 (Esc puis « Declined ») ; « diffs » ; « cartes » (tour `carte` et tour `refus`) ;
     - `suite-real.ts` : « cellule avec bash() » (attente du « Worked » du tour, au lieu du premier de la page) ; « edit() » ; « suppression pure après un emoji ».
  3. **Sonde réservée aux tests**, comme avant. La webview n'agit que sur les messages `probe` et `probeInput` (`webview/ya-probe.ts:136`). L'hôte ne les envoie que depuis `view.probe` et `view.probeInput` (`src/view.ts:427`, `:445`). Ces fonctions ne sont appelées que par les commandes `yusagent._test.probe` et `yusagent._test.probeInput`, déclarées seulement si `YUSAGENT_EXTENSION_TEST === "1"` (`src/extension.ts:51-74`), que pose le seul lanceur e2e.
     - Dans le `.vsix`, la boîte pose son état dans `ya-box-state.ts` à son ouverture et note chaque touche reçue ; rien ne le lit sans sonde, et aucune action n'est enregistrée sans `probeInput`.
- **Ces corrections ne peuvent pas être vues en échec de façon déterministe** : elles retirent une dépendance au temps. Le contrôle est qu'elles passent, au push 2 et ensuite.
- Commits du ticket dans ce push : `69dc040ba` (§9.10) et celui-ci.

### 9.12 Push 2 : le code (24/11/2026)

Commits, un sujet chacun (avant celui de cette doc) :
- `f89ac3f59` : test du ticket 28, snapshot de fenêtre déposé s'il manque (§9.9) ;
- `a11e88e28` : e2e sans l'horloge du test (§9.11) ;
- `6c997a53b` : défauts du cœur dans `_yus/settings/get` (C1), repris par l'agent factice ;
- `4f759d60b` : méthodes `_yus/*` des écrans, lues à l'ouverture de l'écran ;
- `31e1d8648` : plus rien n'attend un redémarrage, textes C3 ;
- `910005477` : onglet General limité, Instructions et catalogue masqués (C4, C9) ;
- `7eb03e12e` : notice en ligne visible du tour ;
- `4b89ef00e` : sélecteur de permissions (C5, C6) ;
- `7ea5e8d2b` : journal sans la valeur de `mcpServers` ;
- `0a323f3b3` : `real-binary-check.mjs`, `permissions` dans sa nouvelle forme (écart : le changement de type de C5 le cassait) ;
- doc : `docs/fork-changes.md` (section « Skills, MCP et réglages dans l'extension, PR X3 »), `extensions/vscode/README.md`, ce ticket.

En local (Claude Code) : `cd extensions/vscode && npm run typecheck && npm run lint && npm run test:unit` donne `ℹ tests 67`, `ℹ pass 67`, `ℹ fail 0`. Le test Rust est seulement formaté (`rustfmt`), compilé en CI (règle 8).

### 9.13 Run du push 2 et contrôles sur le binaire de la branche (24/11/2026)

- **CI** :
  - `ci` 45404118237 : succès ;
  - `ya-extension` 45404118247 : unitaires 67/67, e2e de l'agent factice 14/14, e2e du vrai binaire 18/18, aucun processus restant, `.vsix` 200 848 octets ;
  - `ya-build-macos-arm64` 45404103532 : succès ;
  - `gh pr checks 84 --required` : 16 `pass`.
- **Binaire** `02c32497ecc32140c6bfa416ba50a51d5f6362d9`, SHA256 vérifié par `install-bin.sh`. `install-vsix.sh 45404118247` dans un dossier isolé : 200 848 octets, sha256 `90239ab4…fa41f`, installé.
- **`acp_frames.py`** sur ce binaire : `A-display-auto` (26 trames), `B-basic-manual-reject` (14), `C-resources` (44, refus D1, skill créé, `manual`, une notice), aucun processus restant, `CONFORME`. Défauts rendus : `retry.enabled: true`, `maxRetries: 3`, `baseDelayMs: 2000`, `provider.maxRetryDelayMs: 60000`, failover `true/3/1000/30000`, `waitForUsage` `true/86400000/8`, `compaction.enabled: true`.
- **`compare_frames.py`** contre l'agent factice : `écarts : 0`.
- **`compare_frames.py --against`** la capture de `8205f0bbe` (`main`) : A et B `IDENTIQUE` ; `C-resources` : 22 écarts, tous attendus (C1). Ce sont les 11 défauts `retry.*`, `null` sur `main`, nombre ou booléen sur la branche, dans les réponses de `_yus/settings/get` et `set` (trames 18 et 20). `--expect` ne couvre que des clés ajoutées, pas un changement de type.

### 9.14 Contrôle de Yusuf arrêté à l'étape 3 : écrans Settings non stylés (24/11/2026)

**Constat de Yusuf.** Le contenu est conforme : onglets, groupes, défauts du cœur (C1), texte C3, catalogue masqué, ligne `Skills updated: added demo-y. Sent to the agent.` au-dessus du message. Mais les écrans Settings ne sont pas stylés : boutons natifs, puces, libellé et description collés (« Compact automaticallyBefore the context is full. »), interrupteurs réduits à un trait sans état visible.

**Cause.**
- La PR I (ticket 24, `:704`, `:776`) n'a pas repris le `styles.css` d'Unharnessed. Sa feuille minimale (`extensions/vscode/webview/styles.css:1-5`) ne couvre presque aucune classe de ces écrans, jamais affichés avant X3 : 29 classes sur 34 absentes dans `Settings.tsx`, 31 sur 34 dans `SettingsSkills.tsx`, 30 sur 33 dans `SettingsMcp.tsx` (`switch`, `switch-knob`, `row-main`, `setting-help`, `tabs`, `section`, `list`…).
- Chez Unharnessed (`f8e4658dd`), ces règles existent : par exemple `.switch` en 34×20 px, avec fond et bouton déplacé à l'état allumé, et `.row-main` en colonne flex.

**Correction** (décision de Yusuf : reprendre ses styles tels quels, sans passe de design) :
- `extensions/vscode/webview/ya-settings.css` : notice MIT d'Unharnessed en tête, puis les règles de son `styles.css` pour les classes de ces écrans, ses variables de couleur et ses deux animations.
  - Toutes les règles sont limitées à l'écran par `.ya-settings` (racine de `Settings.tsx`) : le chat garde sa feuille provisoire.
  - Noms d'identité : `--uh-*` en `--ya-*`, `uh-rise` et `uh-pop` en `ya-rise` et `ya-pop`.
  - Aucun `url()` ni police chargée : la CSP ne change pas.
- **Couleurs** (pour la passe de design, rien changé) : des **valeurs fixes** d'Unharnessed, pas les variables `--vscode-*` du thème. Gris neutres et cyan en thème sombre (`--ya-bg: #1a1a1b`, `--ya-raised: #222223`, `--ya-accent: #86defc`…), une variante pour les thèmes clairs et une pour le contraste élevé, choisies d'après les classes que VS Code pose sur `body` (`vscode-light`, `vscode-high-contrast`). Aucune variable `--vscode-*` dans le fichier.
- **Test e2e** (agent factice), « réglages stylés » : libellé et description sur deux lignes dans le texte affiché ; « Retry failed requests » allumé, d'au moins 24×14 px ; après un clic, son fond calculé et la place de son bouton changent. La sonde gagne `switches`. Push A : le test seul, attendu rouge sur son assertion ; push B : la feuille.

**Le chat, comparé à `main` : pas de régression de X3.**
- Chargement des styles inchangé (`main.tsx`, `esbuild.mjs`, `package.json` identiques à `main` jusqu'au push 2).
- `styles.css` : rien de retiré, `.ya-auto-badge` étendu à `.ya-manual-badge`, deux ajouts.
- `Chat.tsx` : seulement les lignes de notice.
- `.vsix` : 200 848 octets contre 200 171 au dernier build de l'extension (ticket 30, run 45152332356 ; extension identique entre `bdf881644` et `main`).
- C'est la feuille provisoire déjà connue (« Workedfor », espaces manquants), en attente de la passe de design.

**Menu de permissions.** Il utilise `.menu` et `.menu-row` de la feuille provisoire (`styles.css:207-219`, `:381-417`) : position absolue au-dessus de l'étiquette (`.ya-permissions-picker`, relatif), fond et bordure de menu du thème, `z-index: 10`. Il est lisible et cliquable : le cas e2e « permissions » clique ses deux libellés sur l'élément réellement sous le pointeur (`elementFromPoint`, ni inerte ni recouvert). Il passe au push 2 sur l'agent factice et sur le vrai binaire (run 45404118247). Rien à ajouter.

**Le `"}` en fin de réponse vient du modèle.** Il est dans le texte de la réponse elle-même, dans le fichier de session (`~/.yusagent/sessions/01a20dd1-fa58-75c4-bad6-a052acc53701.jsonl`, dossier `/tmp/ya31-x3`). Rien à corriger.

**Autres écrans d'Unharnessed encore masqués**, dans le même cas : Providers (27 classes absentes sur 33), SignIn (22 sur 27), Local (19 sur 21), Kernel (34 sur 35), Instructions. À styler quand ils seront démasqués.
