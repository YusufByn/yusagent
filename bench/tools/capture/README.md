# Banc de capture (YA-00)

Lance le binaire `prime-agent` contre un serveur HTTP local qui enregistre chaque requête modèle et renvoie des réponses factices, puis analyse le préambule, le préfixe N/N+1 et les variations entre sessions. Aucun vrai modèle, aucune clé, aucun login.

## Commande unique

```bash
python3 bench/tools/capture/run.py --binary /chemin/vers/prime-agent
```

Résultat : `bench/tools/capture/runs/<horodatage UTC>/report.md` (lisible) et `report.json` (complet), à côté des captures brutes.

Prérequis : macOS (`sandbox-exec`), `python3` ≥ 3.11, `uv`, `git`. Bibliothèque standard uniquement ; seule l'analyse utilise `tiktoken`, installé dans un venv dédié pendant la préparation.

## Obtenir le binaire

- **Binaire de HEAD (celui des mesures)** : artefact du workflow `ya-build-macos-arm64` (`.github/workflows/ya-build-macos-arm64.yml`), déclenché par un push sur une branche `ya-*` qui touche le code, ou à la main.

  ```bash
  gh run download <run-id> --repo YusufByn/yusagent --dir /tmp/pa-head
  ```

  Puis vérifier `SHA256SUMS` et extraire le `.tar.gz` (le binaire `prime-agent` est à la racine de l'archive, avec `prime-agent-runtime/` et `skills/`).
- **Release upstream (pour développer le banc)** : `gh release download v0.10.2 --repo PrimeIntellect-ai/prime-agent -p 'prime-agent-0.10.2-darwin-arm64.tar.gz' -p SHA256SUMS`.

## Déroulé

1. **Préparation** (réseau autorisé, une fois par binaire, marqueur `prepared-<sha>`) : un run de chauffe crée le venv du kernel (uv télécharge un CPython et les paquets du runtime), puis un venv d'analyse reçoit `tiktoken` et le fichier `o200k_base`. Cache : `~/.cache/yusagent-capture` (environ 560 Mo, hors du dépôt ; `--cache` pour le déplacer).
2. **Mesures** (réseau limité à localhost) : chaque lancement du binaire passe par `sandbox-exec -f sandbox.sb`, qui refuse tout trafic sortant sauf localhost, y compris le DNS. Le rapport inclut un contrôle : `curl https://example.com` dans le même profil doit échouer.
3. **Analyse** : `analyze.py`, elle aussi sous sandbox.

Environnement transmis au binaire (liste blanche, `env -i`) : `PATH`, `HOME` (isolé, dans le cache), `TMPDIR`, `LANG`, `DO_NOT_TRACK=1`, `PI_OFFLINE=1`, `PI_SKIP_VERSION_CHECK=1`, `PRIME_AGENT_CODING_AGENT_DIR` (isolé), plus deux variables factices (`ANTHROPIC_API_KEY`, `YA_CAPTURE_FAKE_TOKEN`) qui servent à vérifier si le kernel voit des noms de secrets, et une variable non sensible, `YA_CAPTURE_ALLOWED=visible`. Le `~/.prime` de l'utilisateur n'est jamais lu.

Chaque dossier agent reçoit un `settings.json` avec `"kernel": {"envAllowlist": ["YA_CAPTURE_ALLOWED", "ANTHROPIC_API_KEY"]}` (YA-02) : la première doit être visible dans le kernel, la seconde jamais (identifiant de provider). Un binaire antérieur à YA-02 ignore ce réglage.

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
| ACP | `--mode acp` sur un daemon isolé (`--daemon-socket`) : `initialize`, `session/new`, `session/prompt` |
| Sous-agent | Scénario `spawn` (hors analyse de préfixe) : le parent lance `rlm.spawn`, l'enfant relève les noms sensibles de son kernel (ligne `CHILD_ENV`) puis répond par `agent_message.send`, le parent attend avec `rlm.collect`. OK si la ligne `COLLECT` est réglée (`True`), porte la réponse finale de l'enfant, et si la réponse `agent_message` arrive dans le contexte du parent |
| `psenv` | **Hors sandbox** : `/bin/ps` est setuid et le profil `sandbox.sb` refuse de l'exécuter (code 126), ce qui casse aussi le shell persistant sur macOS (identité de démarrage lue par `ps`). Provider local, `PI_OFFLINE=1`. Ligne `PS_PARENT` (relevé YA-11) : `ps eww` lancé depuis le kernel sur le harness, avec la commande du parent, le code de sortie et les noms des variables factices visibles (jamais de valeur). Ligne `PERSISTENT` : noms sensibles vus par `printenv` dans un shell persistant |

Le prompt système contient le chemin absolu de chaque `SKILL.md` (`<location>`), donc le dossier où le binaire est extrait. Pour comparer deux binaires, les extraire au même chemin, sinon l'inventaire des skills diffère de (écart de longueur du chemin) × (nombre de skills).

## Serveur seul

```bash
python3 -I bench/tools/capture/server.py --out /tmp/cap --port 8787 --scenario basic
```

- Format Anthropic Messages : `POST …/messages`. Format OpenAI Responses : `POST …/responses`. Streaming SSE, ou JSON si `stream` est faux.
- Chaque corps reçu est écrit octet pour octet dans `NNNN.body.json`. Les métadonnées vont dans `NNNN.meta.json`, sans valeur d'authentification.
- Le tour est déduit du nombre de résultats de tool présents dans la requête : le serveur est sans état.
- Options : `--scenario basic|long|crash`, `--inflate-from-turn N --inflate-tokens T` (usage gonflé pour déclencher la compaction).
