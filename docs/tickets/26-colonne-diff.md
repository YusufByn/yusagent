# Ticket 26 : colonne de départ dans le contenu `diff` du pont ACP

Demande de Yusuf du 21/11/2026 (ticket 24, PR 3, choix C14 option b), **à faire après le ticket 25**. **Constat et plan seulement** : aucun code écrit, rien compilé, aucun appel de modèle. Pas de code avant les décisions de Yusuf (section 4).

Objectif : que l'extension retrouve l'« avant » de chaque `edit()` sans ambiguïté, pour que « Revert » marche aussi sur une suppression pure (`new_str` vide). **Rien de ce qui part au modèle ne change** : prompt et requêtes identiques, seules les trames `diff` du pont changent.

## 1. Constat

### 1.1 La limite dans l'extension (ticket 24, PR 3, étape 4)

- L'extension défait chaque fragment à partir de `_yus/startLine` (`extensions/vscode/src/ya/diffs.ts`). Sans colonne, plusieurs places sont possibles sur la ligne ; avec C14 (a), plusieurs « avant » possibles → pas de « Revert », avec la raison.
- **Suppression pure** (`new_str` vide) : ambiguë **même pour des lignes entières**. Après la suppression de `retirer\n`, le texte `garder\nfin\n` admet aussi l'« avant » `garder\nfretirer\nin\n` (test `undo : position sur la ligne, suppression pure…`, `extensions/vscode/test/unit/diffs.test.ts`). Seule exception : la ligne d'arrivée est vide.
- `new_str` présent plusieurs fois sur sa ligne : ambigu aussi, plus rare.

### 1.2 Où la position est connue, et où elle se perd

| Étape | Fichier | Statut | Ce qui passe |
|---|---|---|---|
| `edit()` | `skills/edit/src/edit/__init__.py:76-82` | upstream, déjà modifié par le fork (YA-09, `docs/fork-changes.md`) | `match_index` (décalage dans le fichier) est calculé ; seul `start_line` en est tiré et émis (`_emit_diff`, `:118-133`, MIME `application/vnd.prime-agent.diff+json`, avec `"text/plain": "Edited <path>"`) |
| Lecture du MIME par l'hôte | `crates/pa-core/src/kernel/shared.rs:122-130` (`KernelDiffDisplay`), `:404-421` (`parse_diff_display`) | upstream (#2524) | `path`, `old_str`, `new_str`, `start_line` |
| Agrégation par cellule | `crates/pa-core/src/kernel/manager/events.rs:189` | upstream | la liste des diffs de la cellule |
| `details.diffs` du résultat | `crates/pa-core/src/tools/ipython.rs:504-523` | upstream, ajout du fork (ticket 24, PR 1) | `path`, `diff` rendu, `oldStr`, `newStr`, `startLine` |
| Contenu ACP `diff` | `crates/pa-daemon/src/acp/ya_cell_display.rs:28-52` | fork | `path`, `oldText`, `newText`, `_meta._yus/startLine` |
| Autre constructeur | `crates/pa-core/src/tools/golden_replay.rs:279-283` | upstream | `KernelDiffDisplay` construit pour le rejeu |

- **Unités** : Python compte en points de code, JavaScript en unités UTF-16. Une colonne en points de code, appliquée telle quelle en JavaScript, se tromperait après un caractère hors du plan de base (emoji…). La convention est à fixer (choix K2).
- **Ce qui pourrait atteindre le modèle** : le texte `text/plain` du MIME (`Edited <path>`, inchangé) et le texte de retour d'`edit()` (inchangé). `details` ne part pas au provider ; les lecteurs côté cœur ne lisent que le chemin (`compaction_utils.rs`, `masking.rs`, constat de la PR 1 du ticket 24). À prouver par capture.
- **Venv du kernel** : `skills/edit` est un skill, pas le runtime. Non vérifié : si son changement modifie l'empreinte qui reconstruit le venv (`crates/pa-core/src/kernel/bootstrap/venv/runtime_source.rs:63`). À lire au début du ticket ; ça compte pour les captures `main` contre branche (préparation à refaire).

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

- Upstream, ajouts minimaux listés dans `docs/fork-changes.md` : `skills/edit/src/edit/__init__.py`, `crates/pa-core/src/kernel/shared.rs`, `crates/pa-core/src/tools/ipython.rs`, `crates/pa-core/src/tools/golden_replay.rs`.
- Fork : `crates/pa-daemon/src/acp/ya_cell_display.rs` ; `extensions/vscode/src/ya/cells.ts`, `diffs.ts`, agent factice, tests ; outils du banc si K3 le demande.

## 4. Choix à trancher

1. **K1, forme** : (a) colonne sur la ligne de départ (recommandé : plus petite, se vérifie avec `startLine`) ; (b) décalage absolu dans le fichier.
2. **K2, unité** : (a) points de code, convertis par l'extension (recommandé : ce que Python compte, sans calcul dans le runtime) ; (b) unités UTF-16, calculées par le runtime.
3. **K3, preuve « seules les trames diff changent »** : (a) option `--binaries` de `compare_frames.py` (binaire contre binaire, écarts attendus listés) ; (b) script à part.

## Points ouverts

- Empreinte du venv du kernel et changement d'un skill : non lu (1.2).
- Sessions enregistrées avant ce ticket : sans colonne, l'extension garde C14 (a).
