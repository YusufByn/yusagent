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
