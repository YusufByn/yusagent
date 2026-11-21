# Banc de capture (YA-00)

Lance le binaire `yusagent` (`prime-agent` avant le ticket 19) contre un serveur HTTP local qui enregistre chaque requête modèle et renvoie des réponses factices, puis analyse le préambule, le préfixe N/N+1 et les variations entre sessions. Aucun vrai modèle, aucune clé, aucun login.

## Commande unique

```bash
python3 bench/tools/capture/run.py --binary /chemin/vers/yusagent
```

Résultat : `bench/tools/capture/runs/<horodatage UTC>/report.md` (lisible) et `report.json` (complet), à côté des captures brutes.

Prérequis : macOS (`sandbox-exec`), `python3` ≥ 3.11, `uv`, `git`. Bibliothèque standard uniquement ; seule l'analyse utilise `tiktoken`, installé dans un venv dédié pendant la préparation.

## Obtenir le binaire

- **Binaire de HEAD (celui des mesures)** : artefact du workflow `ya-build-macos-arm64` (`.github/workflows/ya-build-macos-arm64.yml`), déclenché par un push sur une branche `ya-*` qui touche le code, ou à la main.

  ```bash
  gh run download <run-id> --repo YusufByn/yusagent --dir /tmp/pa-head
  ```

  Puis vérifier `SHA256SUMS` et extraire le `.tar.gz` (le binaire `yusagent`, `prime-agent` avant le ticket 19, est à la racine de l'archive, avec `prime-agent-runtime/` et `skills/`).
- **Release upstream (pour développer le banc)** : `gh release download v0.10.2 --repo PrimeIntellect-ai/prime-agent -p 'prime-agent-0.10.2-darwin-arm64.tar.gz' -p SHA256SUMS`.

## Déroulé

1. **Préparation** (réseau autorisé, une fois par binaire, marqueur `prepared-<sha>`) : un run de chauffe crée le venv du kernel (uv télécharge un CPython et les paquets du runtime), puis un venv d'analyse reçoit `tiktoken` et le fichier `o200k_base`. Cache : `~/.cache/yusagent-capture` (environ 560 Mo, hors du dépôt ; `--cache` pour le déplacer).
2. **Mesures** (réseau limité à localhost) : chaque lancement du binaire passe par `sandbox-exec -f sandbox.sb`, qui refuse tout trafic sortant sauf localhost, y compris le DNS. Le rapport inclut un contrôle : `curl https://example.com` dans le même profil doit échouer.
3. **Analyse** : `analyze.py`, elle aussi sous sandbox.

Environnement transmis au binaire (liste blanche, `env -i`) : `PATH`, `HOME` (isolé, dans le cache), `TMPDIR`, `LANG`, `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PI_CACHE_RETENTION=short`, `PRIME_AGENT_CODING_AGENT_DIR` (isolé), plus deux variables factices (`ANTHROPIC_API_KEY`, `YA_CAPTURE_FAKE_TOKEN`) qui servent à vérifier si le kernel voit des noms de secrets, et une variable non sensible, `YA_CAPTURE_ALLOWED=visible`. Le `~/.yusagent` (et l'ancien `~/.prime`) de l'utilisateur n'est jamais lu.

Chaque dossier agent reçoit un `settings.json` avec `"kernel": {"envAllowlist": ["YA_CAPTURE_ALLOWED", "ANTHROPIC_API_KEY"]}` (YA-02) et `"cacheRetention": "short"` (T03). Le TTL du cache est donc fixé deux fois : par la variable, prioritaire et lue aussi par les binaires antérieurs à T03, et par le réglage : la première doit être visible dans le kernel, la seconde jamais (identifiant de provider). Un binaire antérieur à YA-02 ignore ce réglage.

Le projet de test est créé hors du dépôt (`/private/tmp/ya-capture/project`, modifiable par `YA_CAPTURE_WORK`), parce que la découverte d'`AGENTS.md` remonte les dossiers parents.

## Pointer le harness vers le serveur

`run.py` écrit dans le dossier agent isolé un `models.json` de provider personnalisé (format : `crates/pa-core/src/models/custom.rs:136-153`) :

```json
{"providers": {"capture": {
  "baseUrl": "http://127.0.0.1:<port>",
  "apiKey": "capture-dummy",
  "api": "anthropic-messages",
  "models": [{"id": "capture-model", "contextWindow": 200000, "maxTokens": 32000,
              "reasoning": false, "input": ["text"]}]}}}
```

Pour OpenAI Responses : `"api": "openai-responses"` et `baseUrl` suffixé de `/v1`. Le binaire est lancé avec `--provider capture --model capture-model`.

## Cas mesurés

| Cas | Ce qu'il vérifie |
|---|---|
| `s1-anthropic` | Scénario de 4 tours : `print(1)` ; noms de variables sensibles vus par `os.environ` et par `env` dans `bash()`, puis `YA_CAPTURE_ALLOWED` ; `uv --version`, `git status --short` et `python3 -c 'print(1)'` dans `bash()` ; réponse finale |
| `s2-anthropic` | Même scénario, deuxième session dans le même projet et le même dossier agent |
| `s1-responses` | Même scénario au format OpenAI Responses (`prompt_cache_key`) |
| `crash-anthropic` | Le tour 2 tue le kernel (`os._exit(1)`), le tour 3 le relance |
| `resume-anthropic` | Deuxième prompt dans la même session (`--continue`) |
| `compact-anthropic` | Sorties volumineuses et usage annoncé de 199 000 tokens : compaction par seuil, résumé, porte auto `/refine` |
| `usage-split-anthropic` | (T04) Scénario de base, `message_delta` réduit à `output_tokens` : l'usage d'entrée doit venir du `message_start` |
| `refine-anthropic` | (T04) Comme `compact-anthropic`, avec `harnessRefinements` et `autoRefine` réactivés : un appel `/refine` est journalisé |
| `mask-off-anthropic` | (T05) Scénario `mask` : 25 tours, chaque cellule imprime environ 5 000 caractères, exception Python au tour 12, la cellule 25 relit la première sortie masquée (ligne `REREAD`). Cache simulé sur le préfixe réel (`--prefix-cache`). Réglage par défaut : aucun masquage |
| `mask-on-anthropic` | (T05) Même scénario avec `"masking": {"enabled": true}` (fenêtre 10, palier 5) |
| `display-anthropic` | (YA-10, ticket 24) Scénario `display` : un `bash()` lu par la cellule, un `bash()` qu'aucune cellule ne lit (`sleep 3; echo non-lu`, il finit après la réponse finale : son avis `[bash-done]` arrive toujours dans un 7ᵉ appel, tour de réveil à part), deux `edit()` (création puis modification) dans `ya-display/`, retiré à la fin. Le fichier de session est copié dans `cases/display-anthropic/sessions/` pour `display_check.py` |
| ACP | `--mode acp` sur un daemon isolé (`--daemon-socket`) : `initialize`, `session/new`, `session/prompt` |
| ACP extension | (YA-10, ticket 24, PR 2) Les trames de l'extension VS Code, rejouées telles quelles depuis `extensions/vscode/test/fixtures/acp-frames.json` (`initialize` avec les capacités de l'extension, `session/new` avec `mcpServers: []`), avec le prompt du cas ACP ; dossier `acp-extension/`, agent et socket à part |
| TUI | (YA-10, ticket 24) Le même scénario par le TUI, sur un pty (pilote et émulateur d'écran de `bench/tools/ya_permissions_tui_demo.py`), démon isolé : le prompt est tapé, la capture attend « Capture : fin du scénario. » à l'écran, puis `/quit`. Écran final dans `tui/screen.txt` |

## Préfixe des chemins avec démon (ticket 24)

`prefix_paths_check.py`, lancé par `run.py` à la fin (le run échoue s'il échoue) : chaque requête des cas `acp` et `tui` (chemins qui passent par le démon, comme l'extension et le TUI) contre la même requête de `s1-anthropic` (`-p`, chemin du smoke bench). Toutes les clés doivent être identiques (dates des messages normalisées), sauf le prompt système, dont l'écart est limité aux deux lignes liées au démon, écart accepté par Yusuf le 21/11/2026 (journal du CDC) :

- en `-p` seulement, la ligne « This session has no daemon: … » ;
- avec le démon seulement, le skill `rlm-heartbeat` dans la liste des skills.

Rapport : `prefix-paths-report.md` et `.json`, dernière ligne `CONFORME` ou `NON CONFORME`. Lancement seul : `python3 -I bench/tools/capture/prefix_paths_check.py <run>`.

## Trames de l'extension VS Code (ticket 24, PR 2)

`extension_frames_check.py`, lancé par `run.py` après `prefix_paths_check.py` (le run échoue s'il échoue) : chaque requête du cas `acp-extension` doit être identique, corps entier (UUID normalisés), à la même requête du cas `acp`, et les trames envoyées doivent être exactement celles du fixture. C'est la preuve que l'extension ne change rien à ce qui part au modèle (ticket 24, §5.4, preuve 2). Rapport : `extension-frames-report.md` et `.json`, dernière ligne `TOUT IDENTIQUE` ou `DIFFÉRENCES`. Lancement seul : `python3 -I bench/tools/capture/extension_frames_check.py <run>`.
| Sous-agent | Scénario `spawn` (hors analyse de préfixe) : le parent lance `rlm.spawn`, l'enfant relève les noms sensibles de son kernel (ligne `CHILD_ENV`) puis répond par `agent_message.send`, le parent attend avec `rlm.collect`. OK si la ligne `COLLECT` est réglée (`True`), porte la réponse finale de l'enfant, et si la réponse `agent_message` arrive dans le contexte du parent |
| `psenv` | **Hors sandbox** : `/bin/ps` est setuid et le profil `sandbox.sb` refuse de l'exécuter (code 126), ce qui casse aussi le shell persistant sur macOS (identité de démarrage lue par `ps`). Provider local, `PI_OFFLINE=1`. Ligne `PS_PARENT` (relevé YA-11) : `ps eww` lancé depuis le kernel sur le harness, avec la commande du parent, le code de sortie et les noms des variables factices visibles (jamais de valeur). Ligne `PERSISTENT` : noms sensibles vus par `printenv` dans un shell persistant |

Avant T03, le prompt système contient le chemin absolu de chaque `SKILL.md` embarqué (`<location>`), donc le dossier où le binaire est extrait. Depuis T03, ces locations s'écrivent `$PRIME_AGENT_BUNDLED_SKILLS_DIR/<nom>/SKILL.md`. Pour comparer un binaire antérieur à T03 avec un autre, les extraire au même chemin, sinon l'inventaire des skills diffère de (écart de longueur du chemin) × (nombre de skills).

## Usage et journal d'usage (T04)

Le serveur renvoie un usage réaliste (cache lu et écrit, écriture ventilée 5 min / 1 h pour Anthropic, 30 min / 12 h pour Responses et Codex selon `prompt_cache_options.ttl` ou `prompt_cache_breakpoint.ttl` de la requête depuis T08, tokens de raisonnement) et note l'usage exact renvoyé dans `NNNN.meta.json` (`reported_usage`). `run.py` copie les journaux `<id>.usage.ndjson` de chaque cas dans `<cas>/usage/`, puis lance :

```bash
python3 -I bench/tools/capture/usage_check.py <run>
```

Résultat : `<run>/usage-report.md` et `.json`. Pour chaque cas, l'usage des messages assistant (`message_end` de `out*.jsonl`) et chaque ligne de journal sont confrontés à l'usage renvoyé, champ par champ (`input`, `output`, `cacheRead`, `cacheWrite`, ventilation par TTL, raisonnement), avec le taux de cache et le type d'appel ; pour Responses, `providerUsage` du journal doit être l'usage renvoyé tel quel, la ventilation 30 min / 12 h celle de cet usage, et `costWarning` présent seulement avec du 12 h ; ces champs sont absents des lignes Anthropic (T08). Un binaire antérieur à T04 n'a pas de journal : seule la première vérification s'applique. Sémantique des champs : `docs/usage.md`.

## Masquage (T05)

`run.py` lance ensuite, sous sandbox et dans le venv d'analyse (tiktoken) :

```bash
python3 -I bench/tools/capture/masking_check.py <run>
```

Résultat : `<run>/masking-report.md` et `.json`. Pour `mask-off-anthropic` et `mask-on-anthropic` :

- chaque requête de la boucle : tour N, sorties masquées, octets et tokens est. du corps, présence de la sortie en erreur du tour 12, `cacheRead` et `cacheWrite` de la ligne de journal correspondante ;
- chaque rupture de préfixe N → N+1 (hors déplacement de `cache_control`), avec le premier message changé, le `cacheWrite` et le champ `invalidation` du journal ; les ruptures doivent être exactement les requêtes que le journal marque `invalidation` ;
- la ligne `REREAD` : identifiant relu, caractères relus, caractères annoncés par le placeholder.

Cache simulé (`server.py --prefix-cache`) : la lecture vaut le préfixe commun avec la requête précédente de la même conversation (tools, système, puis messages, sans `cache_control`), en octets / 4 ; le reste du prompt est écrit. C'est une simulation : elle montre où le préfixe casse et ce que ça écrit, pas le coût réel. Les tokens envoyés sont une estimation (o200k_base sur le corps JSON). Le gain réel de coût et l'effet sur la résolution se mesurent au smoke bench (YA-14).

## Comparer deux runs

```bash
python3 -I bench/tools/capture/compare.py <run A> <run B> --out <fichier>.md
```

Pour chaque cas commun, la première requête : prompt système, chaque paramètre hors conversation et premier message utilisateur. Chaque bloc modifié est listé avec sa position en octets dans A et dans B, les octets retirés et ajoutés. `--out` écrit aussi le `.json` complet.

`--all` (T04) compare en plus chaque requête de chaque cas, corps entier, dans l'ordre d'arrivée ; seuls les UUID (identifiant de session) et le pid des avis `[bash-done pid:N …]` sont normalisés. Depuis le ticket 24, `--all` compare aussi `acp` et `tui` (hors de `cases/`) et la section finit par `TOUT IDENTIQUE` ou `DIFFÉRENCES`.

## Données d'affichage des cellules (ticket 24)

`python3 -I bench/tools/capture/display_check.py <run>`, sur un run d'un binaire qui porte le ticket 24 (pas lancé par `run.py` : un binaire antérieur n'a pas ces champs). Dans le fichier de session de `display-anthropic` : une seule entrée `details.bashRuns`, celle du `bash()` lu ; deux `details.diffs` avec `oldStr`, `newStr` et `startLine` ; dernière requête = avis `[bash-done]` de la commande non lue. Dernière ligne `CONFORME` ou `NON CONFORME` (code de sortie 1), rapport `display-report.md`.

## Trames ACP pour l'agent factice de l'extension (ticket 24, PR 3)

`acp_frames.py` capture les trames ACP du vrai binaire, avec les fonctions de `run.py` : serveur de capture, `sandbox-exec`, démon isolé, venv du banc. Deux sessions : `A-display-auto` (scénario `display` en « auto », attente de `--linger` secondes après la réponse, 40 par défaut, pour le tour de réveil) et `B-basic-manual-reject` (scénario `basic` en « manual », demande refusée). Chaque cas écrit `transcript.jsonl` (trames avec leur sens) et les corps de requête dans `cap/`. Dernière ligne `CONFORME` (deux `end_turn`, aucun processus restant) ou `NON CONFORME` (code de sortie 1). `--out` doit être neuf.

`compare_frames.py <dossier>` lance l'agent factice de l'extension (`extensions/vscode/test/fake-agent/fake-agent.mjs`, ou `--fake`) avec les prompts `carte`, `refus` et `lent`, puis compare ses trames à celles du binaire, champ par champ : chemins de clés et types, valeurs constantes, fin d'un tour de prompt. Dernière ligne `écarts : N` (code de sortie 1 si N > 0).

```sh
export DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short
python3 -I bench/tools/capture/acp_frames.py --binary <yusagent> --out <dossier neuf>
python3 -I bench/tools/capture/compare_frames.py <dossier neuf>
```

## Tours de réveil et Stop pendant un réveil (ticket 25)

`wake_turns.py` mène six sessions ACP (scénarios `wake-stop-avant` et `wake-stop-apres` : deux `bash()` non lus, premier réveil de 15 s). Un second client du socket du démon (`attach`) relève les événements du worker (`agent_start`, `agent_end` et ce qu'il y a entre les deux) et envoie pendant le premier réveil l'une des trois commandes d'arrêt (`abort_and_clear_queue`, `abort`, `abort_and_send_queued`), avant ou après l'arrivée du second avis. Le script relève la file (`get_queue`) avant et après la commande, les tours qui démarrent ensuite, et compare chaque requête envoyée au provider après la commande à la précédente (prompt système, tools, historique en ajout seul, `analyze.compare_pair`). Rapport `wake-turns.md`, dernière ligne `CONFORME` ou `NON CONFORME` (code de sortie 1). `--only <cas>` limite les cas ; `--out` doit être neuf.

```sh
export DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short
python3 -I bench/tools/capture/wake_turns.py --binary <yusagent> --out <dossier neuf>
```

## Serveur seul

```bash
python3 -I bench/tools/capture/server.py --out /tmp/cap --port 8787 --scenario basic
```

- Format Anthropic Messages : `POST …/messages`. Format OpenAI Responses : `POST …/responses`. Streaming SSE, ou JSON si `stream` est faux.
- Chaque corps reçu est écrit octet pour octet dans `NNNN.body.json`. Les métadonnées vont dans `NNNN.meta.json`, sans valeur d'authentification.
- Le tour est déduit du nombre de résultats de tool présents dans la requête : le serveur est sans état.
- Options : `--scenario basic|long|crash|mask`, `--prefix-cache` (T05 : lecture du cache sur le préfixe réel), `--inflate-from-turn N --inflate-tokens T` (usage gonflé pour déclencher la compaction), `--usage-split` (T04 : `message_delta` Anthropic réduit à `output_tokens`).
- WebSocket Codex (T06, YA-14) : `GET …/responses` avec `Upgrade: websocket`, une trame texte `response.create` par requête, les événements Responses en trames texte. Avec `previous_response_id`, l'entrée reçue est un delta : la requête est reconstruite depuis la précédente de la même connexion (le serveur n'est plus sans état sur ce transport). Le transport est noté dans `NNNN.meta.json` (`transport`, `continuation`).
- Smoke bench (T06) : scénarios `smoke-<tâche>` (résolvent les tâches d'exemple de `bench/smoke/tasks/`), `smoke-fail`, `smoke-probe` (sonde réseau), `smoke-warmup`, `smoke-kernel` ; options `--connect-proxy-port` (port du proxy CONNECT pour la sonde), `--read-probe-path <chemin>` (chemins hors workspace que la sonde d'isolation tente de lire), `--reject-model <id>` (id faux simulé), `--no-websocket` (refuse l'upgrade : repli SSE).
