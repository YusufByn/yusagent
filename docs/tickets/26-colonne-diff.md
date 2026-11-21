# Ticket 26 : colonne de départ dans le contenu `diff` du pont ACP

Demande de Yusuf du 21/11/2026 (ticket 24, PR 3, choix C14 option b), **à faire après le ticket 25**. Plan vérifié sur `main` à jour (`ea3908688`, merge du ticket 25) et décisions de Yusuf du 21/11/2026 (section 4) ; réalisation : section 5. Aucun appel de modèle.

Objectif : que l'extension retrouve l'« avant » de chaque `edit()` sans ambiguïté, pour que « Revert » marche aussi sur une suppression pure (`new_str` vide). **Rien de ce qui part au modèle ne change** : prompt et requêtes identiques, seules les trames `diff` du pont changent.

## 1. Constat

### 1.1 La limite dans l'extension (ticket 24, PR 3, étape 4)

- L'extension défait chaque fragment à partir de `_yus/startLine` (`extensions/vscode/src/ya/diffs.ts`). Sans colonne, plusieurs places sont possibles sur la ligne ; avec C14 (a), plusieurs « avant » possibles → pas de « Revert », avec la raison.
- **Suppression pure** (`new_str` vide) : ambiguë **même pour des lignes entières**. Après la suppression de `retirer\n`, le texte `garder\nfin\n` admet aussi l'« avant » `garder\nfretirer\nin\n` (test `undo : position sur la ligne, suppression pure…`, `extensions/vscode/test/unit/diffs.test.ts`). Seule exception : la ligne d'arrivée est vide.
- `new_str` présent plusieurs fois sur sa ligne : ambigu aussi, plus rare.

### 1.2 Où la position est connue, et où elle se perd

| Étape | Fichier | Statut | Ce qui passe |
|---|---|---|---|
| `edit()` | `skills/edit/src/edit/__init__.py:77-82` (`match_index` `:77`, `start_line` `:78`, appel `:82` ; création `:61`, ligne 1) | upstream, déjà modifié par le fork (YA-09, `docs/fork-changes.md`) | `match_index` (décalage dans le fichier) est calculé ; seul `start_line` en est tiré et émis (`_emit_diff`, `:118-133`, MIME `application/vnd.prime-agent.diff+json`, avec `"text/plain": "Edited <path>"`) |
| Lecture du MIME par l'hôte | `crates/pa-core/src/kernel/shared.rs:124-131` (`KernelDiffDisplay`), `:405-421` (`parse_diff_display`) | upstream (#2524) | `path`, `old_str`, `new_str`, `start_line` |
| Agrégation par cellule | `crates/pa-core/src/kernel/manager/events.rs:190-191` | upstream | la liste des diffs de la cellule |
| `details.diffs` du résultat | `crates/pa-core/src/tools/ipython.rs:504-523` (`startLine` vaut 1 quand il manque, `:514-517`) | upstream, ajout du fork (ticket 24, PR 1) | `path`, `diff` rendu, `oldStr`, `newStr`, `startLine` |
| Contenu ACP `diff` | `crates/pa-daemon/src/acp/ya_cell_display.rs:28-52` (`START_LINE_META` `:20`, tests `:88-99`) | fork | `path`, `oldText`, `newText`, `_meta._yus/startLine` |
| Commentaire du contenu `diff` | `crates/pa-daemon/src/acp/types.rs:221-224` | upstream, déjà modifié par le fork (`fork-changes.md`, ticket 24) | décrit `_yus/startLine` ; à compléter |
| Autre constructeur | `crates/pa-core/src/tools/golden_replay.rs:279-284` | upstream | code de test seulement (`#![cfg(test)]`, `:7`) : `KernelDiffDisplay` construit dans le test `ipython_details_carry_the_cell_edit_diffs` (`:278`), dont l'assertion exacte de `details.diffs` (`:303-317`) est aussi à compléter ; pas de rejeu en production |
| Extension | `extensions/vscode/src/ya/cells.ts:17`, `:22`, `:47-52` ; `diffs.ts:30-47`, `MAX_LINE` `:17` ; `test/unit/diffs.test.ts:59` ; agent factice `test/fake-agent/fake-agent.mjs:163-164`, `:219-220` | fork | lecture de `_yus/startLine`, recherche sur la ligne |

Vérification sur `main` à jour (`ea3908688`) : le ticket 25 (`git diff --stat ea3908688^1 ea3908688`) n'a touché aucun des fichiers Rust ou Python ci-dessus ; seuls `fake-agent.mjs` et `compare_frames.py` sont communs aux deux tickets. Corrections de références : celles du tableau (lignes décalées, `types.rs` et la portée de `golden_replay.rs` ajoutées).

- **Unités** : Python compte en points de code, JavaScript en unités UTF-16. Une colonne en points de code, appliquée telle quelle en JavaScript, se tromperait après un caractère hors du plan de base (emoji…). La convention est à fixer (choix K2).
- **Ce qui pourrait atteindre le modèle** : le texte `text/plain` du MIME (`Edited <path>`, inchangé) et le texte de retour d'`edit()` (inchangé). `details` ne part pas au provider ; les lecteurs côté cœur ne lisent que le chemin (`compaction_utils.rs`, `masking.rs`, constat de la PR 1 du ticket 24). À prouver par capture.
- **Venv du kernel** (lu le 21/11/2026) : l'empreinte qui reconstruit le venv (`crates/pa-core/src/kernel/bootstrap/venv/runtime_source.rs:62-80`) ne couvre que `pyproject.toml` et `src/rlm` du runtime (`:66-69`). Les skills sont installés en éditable (`venv.rs:215-217`), repérés par nom et chemin (`venv/version.rs:24-26`), réinstallés seulement si leur `pyproject` change (`venv.rs:194-198`). Changer `__init__.py` ne reconstruit pas le venv ; la préparation du banc reste « une fois par binaire ».
- **Fins de ligne** : `Path.read_text` convertit `\r\n` en `\n` et le fichier est réécrit ainsi (vérifié sur Python 3.14) ; la colonne est donc calculée sur le texte que l'extension relit. `edit()` convertit tout le fichier : ticket à part, `docs/tickets/27-edit-fins-de-ligne.md`.

## 2. Plan proposé (sans code)

1. **Runtime** (`skills/edit/src/edit/__init__.py`, ajout minimal) : `start_column` dans le MIME, tiré de `match_index` (colonne du début d'`old_str` sur sa ligne) ; création : 0.
2. **Hôte** (`crates/pa-core/src/kernel/shared.rs`) : champ `start_column: Option<u64>` lu s'il est là (tolérant : un kernel plus ancien ne l'envoie pas) ; `golden_replay.rs` : `start_column: None`. `ipython.rs` : `startColumn` dans `details.diffs`.
3. **Pont** (`crates/pa-daemon/src/acp/ya_cell_display.rs`) : `_meta._yus/startColumn` à côté de `_yus/startLine`.
4. **Extension** (`extensions/vscode/src/ya/cells.ts`, `diffs.ts`) : avec la colonne, une seule place essayée, toujours vérifiée (`new_str` à cette place) ; sans colonne (session ou binaire plus ancien), le comportement actuel (C14 a). Agent factice : la colonne dans ses contenus `diff`.
5. **Tests** :
   - runtime (`unittest`) : colonne émise pour un remplacement en milieu de ligne, une suppression pure, une création ;
   - cœur : `parse_diff_display` avec et sans colonne ; `details.diffs` porte `startColumn` ;
   - pont : `_yus/startColumn` dans le contenu `diff` ;
   - extension : suppression pure en milieu de ligne et de lignes entières → « Revert » possible ; colonne fausse → refus avec la raison.
6. **Captures, preuve que rien d'autre ne change** (Claude Code, puis contrôle de Yusuf, binaires `main` et branche) :
   - banc de capture : `compare.py --all` sur les 16 lignes (cas `-p`, `display-anthropic`, `acp`, `tui`) **TOUT IDENTIQUE** : prompt système, tools, messages et paramètres de chaque requête inchangés ; `prefix_paths_check.py` **CONFORME** ; `display_check.py` **CONFORME** (complété pour `startColumn`) ;
   - smoke bench `py-example-001` : `compare_capture_prompts.py` **TOUT IDENTIQUE** ;
   - trames ACP : `bench/tools/capture/acp_frames.py` sur les deux binaires ; **seules les trames `diff` changent** (ajout de `_meta._yus/startColumn`). Vérification : soit une option de `compare_frames.py`, soit un petit script de comparaison binaire contre binaire, à choisir (K3) ;
   - `compare_frames.py` (agent factice contre binaire de la branche) : `écarts : 0` après l'ajout de la colonne à l'agent factice.

## 3. Fichiers prévus

- Upstream, ajouts minimaux listés dans `docs/fork-changes.md` : `skills/edit/src/edit/__init__.py`, `crates/pa-core/src/kernel/shared.rs`, `crates/pa-core/src/tools/ipython.rs`, `crates/pa-core/src/tools/golden_replay.rs`, `crates/pa-daemon/src/acp/types.rs` (commentaire).
- Fork : `crates/pa-daemon/src/acp/ya_cell_display.rs` ; `extensions/vscode/src/ya/cells.ts`, `diffs.ts`, agent factice, tests ; outils du banc si K3 le demande.

## 4. Choix (décisions de Yusuf du 21/11/2026)

1. **K1, forme : (a)** colonne sur la ligne de départ, comptée à partir de 0 (création : 0). Noms : `start_column` (MIME), `startColumn` (`details`), `_yus/startColumn` (`_meta`). Pas de colonne par défaut : omise quand elle manque. Écarté : (b) décalage absolu dans le fichier (`startLine` redondant, conversion d'unités sur tout le texte qui précède).
2. **K2, unité : (a)** points de code (ce que Python compte), convertis par l'extension sur la ligne. Écarté : (b) unités UTF-16 calculées par le runtime. Constat : dans `c😀d`, `d` est à la colonne 2 en points de code, 3 en UTF-16. Pour une suppression pure (`new_str` vide), la vérification « `new_str` à cette place » est toujours vraie : une colonne mal convertie donnerait en silence un mauvais « avant ». D'où un test avec un emoji avant une suppression pure (runtime, extension, e2e vrai binaire) ; le refus d'une colonne fausse se teste sur un remplacement.
3. **K3, preuve : (a)** option `--against <dossier main>` de `compare_frames.py`, avec `--expect <chemin de clé>` ; échec si un écart attendu est absent. Stabilité constatée avant le choix : captures de `418fdb75d` et `ea3908688` (sous `crates/` et `skills/`, seul un fichier de test diffère) : 26 et 14 trames, même suite de genres, 0 trame de schéma différent.

Tests demandés : runtime et extension, un emoji avant une suppression pure et le refus d'une colonne fausse sur un remplacement, chacun vu en échec sans le correctif ; un cas e2e vrai binaire (`extensions/vscode/test/e2e/suite-real.ts`) : ligne avec un emoji avant le texte supprimé, suppression pure par `edit()`, « Revert » par un clic sur l'élément réellement sous le pointeur, fichier revenu octet pour octet. Scénario `display` du banc inchangé. CRLF hors périmètre (ticket 27).

## Points ouverts

- Sessions enregistrées avant ce ticket : sans colonne, l'extension garde C14 (a).
- `edit()` convertit les fins de ligne `\r\n` de tout le fichier : `docs/tickets/27-edit-fins-de-ligne.md` (constat seulement).

## 5. Réalisation (branche `ya-26-colonne-diff`)

- **Runtime** (`skills/edit/src/edit/__init__.py:80`, envoi `:84`) : `start_column = match_index - (content.rfind("\n", 0, match_index) + 1)`, envoyée dans le MIME ; création : 0 (`:61`).
- **Hôte** : `KernelDiffDisplay.start_column` lu par `parse_diff_display` (`crates/pa-core/src/kernel/shared.rs`) ; `startColumn` dans `details.diffs` seulement quand il est là (`crates/pa-core/src/tools/ipython.rs`).
- **Pont** : `_yus/startColumn` à côté de `_yus/startLine`, chacun quand il est là (`crates/pa-daemon/src/acp/ya_cell_display.rs`, `position_meta`) ; commentaire de `types.rs`.
- **Extension** : `fragments` lit `_yus/startColumn` (entier positif, sinon ignorée) (`extensions/vscode/src/ya/cells.ts`) ; `undo` (`src/ya/diffs.ts`) : avec la colonne, une seule place (`columnIndex` : points de code → unités UTF-16 sur la ligne), `new_str` à cette place et `old_str` une seule fois dans le candidat, sinon refus avec la raison ; sans colonne, C14 (a). Agent factice : `_yus/startColumn` sur ses contenus `diff`.
- **Banc** : `compare_frames.py --against … --expect …` ; `display_check.py` vérifie `startColumn`.

### Tests

- Runtime (`prime-agent-runtime/test/test_ya_edit.py`, `test_the_diff_display_carries_the_start_column`) : suppression pure après un emoji (ligne 2, colonne 4), remplacement en milieu de ligne, création (colonne 0). **Vu en échec sans le correctif** (skill de `main`, test de la branche : `KeyError: 'start_column'`, `FAILED (errors=1)`), puis `OK` (6 tests) ; suite du runtime : `OK`.
- Cœur (`golden_replay.rs`) : `diff_display_reads_the_start_column_when_present` (avec, sans, valeur non entière) ; `ipython_details_carry_the_cell_edit_diffs` : `startColumn` présent quand le kernel l'envoie, absent sinon. Pont (`ya_cell_display.rs`) : contenus avec et sans colonne.
- Extension, unitaires (55 tests) : « undo avec la colonne : emoji avant une suppression pure, lignes entières, milieu de ligne » et « colonne fausse sur un remplacement, refus avec la raison ». **Vus en échec sans le correctif** (`src/ya/diffs.ts` de `main`, type `Fragment` de la branche : `ℹ fail 2` ; le premier rend `the edit's place on line 2 is ambiguous` au lieu de l'« avant », le second rend `ok: true` au lieu du refus), puis `ℹ pass 55`. Les cas C14 (a) sans colonne sont gardés (`withoutColumn`).
- e2e vrai binaire (`extensions/vscode/test/e2e/suite-real.ts`) : `emoji.txt` = `garder\n😀 un deux\nfin\n`, `edit('emoji.txt', 'un ', '')`, « Revert » par des clics sur l'élément réellement sous le pointeur (`Worked`, `Edited`, `Revert`), fichier comparé octet pour octet. CI `ya-extension` run 44945205641 : `ok - suppression pure après un emoji (ticket 26) : Revert par un clic, fichier revenu octet pour octet`, `e2e vrai binaire : 7/7 réussis`, `processus restants : aucun` ; e2e agent factice `11/11`.

### Contrôles du binaire de la branche (21/11/2026)

Binaire `750b9885e` (build macOS run 44945162571, `SHA256SUMS` vérifié). Binaire de `main` : `ea3908688`. Commandes, avec `DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short` (sorties dans `bench/tools/capture/runs/`, non versionné) :

```
python3 -I bench/tools/capture/run.py --binary <ea3908688>/yusagent --out bench/tools/capture/runs/ya26-prefix-main-ea3908688
python3 -I bench/tools/capture/run.py --binary <750b9885e>/yusagent --out bench/tools/capture/runs/ya26-prefix-branch-750b9885e
python3 -I bench/tools/capture/compare.py bench/tools/capture/runs/ya26-prefix-main-ea3908688 bench/tools/capture/runs/ya26-prefix-branch-750b9885e --all --out bench/tools/capture/runs/ya26-compare.md
python3 -I bench/tools/capture/display_check.py bench/tools/capture/runs/ya26-prefix-branch-750b9885e
python3 -I bench/tools/capture/acp_frames.py --binary <750b9885e>/yusagent --out bench/tools/capture/runs/ya26-frames-750b9885e
python3 -I bench/tools/capture/compare_frames.py bench/tools/capture/runs/ya26-frames-750b9885e --against bench/tools/capture/runs/ya25-frames-main-ea3908688 --expect '.params.update.content[]._meta._yus/startColumn'
python3 -I bench/tools/capture/compare_frames.py bench/tools/capture/runs/ya26-frames-750b9885e
```

- **Préfixe** : `compare.py --all` **TOUT IDENTIQUE** (17 cas, chaque requête, UUID normalisés ; `acp`, `acp-extension`, `tui` : 4 requêtes de chaque côté, 0 différente ; `display-anthropic` : 7 et 7). Seul écart des tableaux par morceau : `prompt_cache_key`, identifiant de session, différent à chaque run. `prefix_paths_check` de la branche : `CONFORME` ; trames de l'extension (`acp-extension`) : `TOUT IDENTIQUE`. Le préfixe envoyé au modèle ne change pas.
- **Affichage** : `display_check.py` sur la branche : `CONFORME` (`startColumn 0 sur chaque diff`) ; sur `main` : `NON CONFORME` sur ce seul point, attendu.
- **Trames, binaire contre binaire** (`--against`, capture de `main` du ticket 25) : `A-display-auto` 26 trames et `B-basic-manual-reject` 14 trames `IDENTIQUE`, écart attendu dans 2 trames de A, `écart attendu présent : .params.update.content[]._meta._yus/startColumn (valeurs [0, 0])`, `écarts : 0`. Avant le choix de K3, le même outil sur deux captures sans ce changement (`418fdb75d`, `ea3908688`) : `écarts : 0`, et `écart attendu ABSENT` avec `--expect` (code de sortie 1).
- **Agent factice contre binaire de la branche** : `compare_frames.py` : `écarts : 0`.
- Note : le run de `main` signale des processus restants pour `acp` (`18217`, `18535`, `19819`) : le démon de Yusuf lancé à 20:43 avec le binaire `ea3908688` (socket par défaut) et ses workers, antérieurs au banc (`ps -o pid,ppid,lstart,command`). Le run de la branche : aucun.

### CI

Commit `750b9885e` : `ci.yml` run 44945205612 vert (shards 1 à 8, `runtime suite (python 3.11)`, `fmt`, `check + clippy`), `ya-extension` run 44945205641 vert (tsc, lint, unitaires, `.vsix`, e2e agent factice et vrai binaire), build macOS run 44945162571.
