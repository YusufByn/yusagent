# Ticket 21 : bail de session et chemin symbolique (constat)

Constat du 20/11/2026, relevé pendant le contrôle de la PR 2 du ticket 20 (`docs/tickets/20-ya11-permissions.md`, section « PR 2 »). **Constat seulement** : aucun code modifié, rien poussé pour ce ticket. Pas de correctif sans décision de Yusuf.

## Symptôme

Démo ACP des permissions (`bench/tools/ya_permissions_acp_demo.py`, version du commit `88c139f6e`) lancée par Yusuf sur le binaire `b5763b3f4`, dossier de travail créé par `tempfile.mkdtemp` sous `/var/folders/…` (macOS) : les cellules et les décisions sont correctes, mais le fichier de session du worker n'a que ses 5 premières lignes (en-tête, `model_change`, `thinking_level_change`, `service_tier_change`, `session_state`) : **aucun message, aucune entrée `ya-permission`**. Le journal du worker (`agent/logs/worker-<id>.stderr.log`) contient, une fois par décision :

```
pa-daemon: permission journal entry not persisted: session lease does not own append target
```

Dossiers de Yusuf : `/var/folders/1l/s22f789n1_19m4c8m59fjyj80000gn/T/ya-perm-acp-oyx9sm48` et `…/ya-perm-acp-l9fsszl6`.

## Cause (constatée)

1. Une session neuve du démon prend son bail **avant que son fichier existe** : `crates/pa-daemon/src/worker/create.rs:360-368` (`session_dir.join(session_file_name(…))`, puis `acquire_runtime_session_lease(&path, &agent_dir)`).
2. Le bail enregistre `canonical_session_path(path)` (`crates/pa-daemon/src/lease.rs:498`, `:519`). Cette fonction canonicalise le fichier, sinon son parent, sinon **garde le chemin brut** (`crates/pa-daemon/src/lease.rs:72-79`, branche `_ => path.to_path_buf()`). Pour la première session d'un agent, le dossier `sessions` n'existe pas encore : le bail garde `/var/folders/…/agent/sessions/<id>.jsonl`.
3. Une fois le fichier créé, chaque écriture compare son chemin canonique, `/private/var/folders/…` (macOS : `/var` est un lien vers `/private/var`), au chemin du bail : `crates/pa-daemon/src/lease.rs:416-421` refuse avec « session lease does not own append target ».
4. Le worker ignore cette erreur pour les messages (`crates/pa-daemon/src/worker/turn.rs:619`, `let _ = store.persist_entry("message", …)`). Le journal des permissions du ticket 20 est le seul à l'afficher (`crates/pa-daemon/src/ya_permissions.rs:300-303`).

## Mesures (binaires de CI exécutés en local, aucun build, aucun modèle)

Un tour ACP scripté (provider factice), `PRIME_AGENT_CODING_AGENT_DIR` dans un dossier neuf ; nombre de messages dans le fichier de session à la fin. Commande (Claude Code, 20/11/2026, binaires installés par `bench/smoke/tools/install-bin.sh`) :

```sh
M=docs/tickets/21-bail-chemin-symbolique/mesure.py
VAR=$(python3 -c 'import tempfile; print(tempfile.gettempdir())')   # /var/folders/…/T
mkdir -p /private/tmp/ya-bail
python3 -I $M "$BIN/yusagent" "$VAR" text            # puis /private/tmp/ya-bail, cell, --sessions-avant
```

Sortie brute (lignes, messages, entrées `custom`, puis les lignes `pa-daemon:`) : `main` `/var/folders` texte `None 5 0 []` ; `main` `/private/tmp` texte `None 6 1 []` ; branche `/var/folders` texte `end_turn 5 0 []` ; branche `/private/tmp` texte `end_turn 7 2 []` ; branche `/private/tmp` cellule `end_turn 10 4 ['ya-permission']` ; branche `/var/folders` cellule `end_turn 5 0 []` et `['pa-daemon: permission journal entry not persisted: session lease does not own append target']` ; branche `/var/folders` cellule avec `--sessions-avant` `end_turn 10 4 ['ya-permission']`. Dossiers de mesure supprimés ensuite.

| Dossier de travail | `main` `f598ef0b6` (run 44715022593) | branche `b5763b3f4` (run 44725552755) |
|---|---|---|
| sous `/var/folders` (lien symbolique), tour sans cellule | 0 message | 0 message |
| sous `/private/tmp` (canonique), tour sans cellule | 1 message | 2 messages |
| sous `/private/tmp`, une cellule acceptée | — | 4 messages, `['ya-permission']` |
| sous `/var/folders`, une cellule acceptée | — | 0 message, aucune entrée, 1 erreur `pa-daemon:` |
| sous `/var/folders`, dossier `sessions` créé d'avance, une cellule acceptée | — | 4 messages, `['ya-permission']` |

Le binaire de `main` refuse le tour (session manuelle sans demandeur, PR 1 du ticket 20), d'où un seul message (celui de l'utilisateur) sous `/private/tmp`. **Le défaut existe donc sur `main`, avant le ticket 20.**

## Impact

- **Première session d'un dossier agent dont le dossier `sessions` n'existe pas encore, quand le chemin du dossier agent passe par un lien symbolique** : tous les messages de cette session sont perdus, **sans erreur visible** (le tour se déroule normalement). La reprise de cette session ne retrouve rien ; le journal des permissions du ticket 20 est perdu aussi.
- Les sessions suivantes du même dossier agent ne sont pas touchées (le dossier `sessions` existe alors : le bail canonicalise son parent).
- Usage normal sur macOS : `~/.yusagent` est sous `/Users/…`, chemin canonique : pas touché. Touchés : dossiers temporaires (`/var/folders`, `/tmp` sur macOS), bancs et scripts qui créent un dossier agent neuf sous ces chemins, tout dossier agent sous un lien symbolique.
- La CI Linux ne le voit pas : ses dossiers temporaires n'ont pas de lien symbolique.

## Correctifs possibles

1. **Canonicaliser le plus proche parent existant** dans `canonical_session_path` (`crates/pa-daemon/src/lease.rs:72-79`) et rejoindre la fin manquante du chemin. **Recommandé** : corrige la cause pour tous les appelants du bail (création, fork, navigation), sans changer l'ordre des opérations de la création.
2. Créer le dossier de sessions avant de prendre le bail (`std::fs::create_dir_all(&session_dir)` avant `crates/pa-daemon/src/worker/create.rs:360`). Plus local, mais laisse la fonction fautive pour les autres appelants.

**Upstream l'a corrigé** depuis notre point de fork (`c83ac68b3`, 02/11/2026) : commit `12a5081aca4248e8651f9f24ee4b01c249de6492` (PR #3871, 12/11/2026, « Fix the macOS build and test suite, and two daemon bugs it exposed »), partie « pa-daemon: keep one lease path for a session created in a new directory » : exactement le correctif 1, dans `crates/pa-daemon/src/lease.rs` (48 lignes, dont le test unitaire `a_lease_taken_before_its_sessions_dir_exists_owns_the_created_file`, qui construit son propre lien symbolique et échoue donc aussi sous Linux). Ce commit n'est pas dans notre historique (`git merge-base --is-ancestor 12a5081ac HEAD` : faux). Le commit touche 71 fichiers (build et tests macOS) : reporter seulement la partie `lease.rs` (règle 2 de `CLAUDE.md`), et la lister dans `docs/fork-changes.md`.

## Tests

- Le test unitaire d'upstream ci-dessus, avec le correctif.
- **Test e2e proposé** : `HOME` (donc le dossier agent) atteint par un lien symbolique vers le dossier temporaire du test ; une session ACP neuve, un tour du provider factice, puis : le fichier de session contient les messages du tour (et, en manuel, l'entrée `ya-permission`), et le journal du worker n'a aucune ligne `pa-daemon:`. Sous Linux, `canonicalize` résout le lien : le test échoue sans le correctif.
- La démo du ticket 20 vérifie déjà ce point (messages présents, journal égal aux réponses, aucune ligne `pa-daemon:`) et travaille sous le chemin canonique de son dossier.

## Correctif (20/11/2026, PR #68)

Décision de Yusuf : reporter la partie `lease.rs` d'upstream `12a5081ac` (correctif 1) et ajouter le test e2e. Branche `ya-21-bail`, depuis `main` à `163f3d39d`.

- **Report partiel** : `crates/pa-daemon/src/lease.rs` seul, diff de ce fichier dans `12a5081ac` appliqué tel quel (`git apply`, aucune retouche, le fichier n'avait pas bougé depuis le point de fork), avec le test unitaire `a_lease_taken_before_its_sessions_dir_exists_owns_the_created_file`. Commit séparé, message citant le SHA upstream. Listé dans `docs/fork-changes.md`, section « Bail de session sous un chemin symbolique ».
- **Test e2e** : `crates/pa-cli/tests/ya_session_lease_symlink_e2e.rs` (fichier du fork). `HOME` = `<tmp>/home`, lien symbolique vers `<tmp>/real` ; dossier agent `~/.yusagent` neuf ; vrai binaire en ACP, provider factice, vrai kernel, mode manuel ; une cellule acceptée. Attendu : `stopReason` `end_turn`, une demande, rôles `user`, `assistant`, `toolResult`, `assistant` dans le fichier de session, une entrée `ya-permission` `allow`, aucune ligne `pa-daemon:` dans le journal du worker.
- **Rouge sans le correctif, vérifié en CI** (pas de build local, règle 8) : le premier push de la PR ne contenait que le test (`1ee68d8de`, sur `main`). Run `ci` 44742804539, job `test shard 2/8` : `the_first_session_under_a_symlinked_home_keeps_its_messages ... FAILED`, `left: []`, `right: ["user", "assistant", "toolResult", "assistant"]` (`ya_session_lease_symlink_e2e.rs:290`), seule unité en échec du shard. Le même run avait un échec clippy (`map_unwrap_or`) dans le test, corrigé dans un commit suivant.

### Contrôles avant le merge de la PR #68 (20/11/2026)

Même commande que la section « Mesures » (`docs/tickets/21-bail-chemin-symbolique/mesure.py`), dossier de travail sous `/var/folders/…/T` (lien symbolique), un tour texte puis un tour avec une cellule acceptée. Binaires : `main` `163f3d39d` (run 44740311939) et branche `5d3834267` (run 44743845310), installés par `bench/smoke/tools/install-bin.sh`.

```sh
M=docs/tickets/21-bail-chemin-symbolique/mesure.py
VAR=$(python3 -c 'import tempfile; print(tempfile.gettempdir())')
for k in text cell; do python3 -I $M "$BIN/yusagent" "$VAR" $k; done
```

- **Claude Code**, sortie brute (`stopReason`, lignes, messages, entrées `custom`, puis les lignes `pa-daemon:`) :
  - `main` `163f3d39d` : texte `{'stopReason': 'end_turn'} 5 0 []`, `[]` ; cellule `{'stopReason': 'end_turn'} 5 0 []`, `['pa-daemon: permission journal entry not persisted: session lease does not own append target']` ;
  - branche `5d3834267` : texte `{'stopReason': 'end_turn'} 7 2 []`, `[]` ; cellule `{'stopReason': 'end_turn'} 10 4 ['ya-permission']`, `[]`.
- **Yusuf**, sur `163f3d39d` contre `5d3834267` : conforme, identique. `main` : 0 message, avec la ligne `pa-daemon:` pour la cellule ; branche : 2 et 4 messages, `['ya-permission']`, aucune ligne `pa-daemon:`.

PR #68 mergée le 20/11/2026 (« merge » de Yusuf), `main` à `3cdccf6f7` ; build macOS de `main` : run 44746491241, succès.
