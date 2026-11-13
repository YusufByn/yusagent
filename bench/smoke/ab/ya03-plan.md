# A/B YA-03 (`editTool`, puis `shellTool`) : plan et règles de verdict

Règles fixées et versionnées avant tout run réel (ticket 09, `docs/tickets/09-ya03-base.md` ; journal du CDC, décisions du 13/11/2026). Deux A/B **séquentiels, non croisés**, sur le binaire de la PR 2 de YA-03. Le second se joue sur l'`editTool` retenu par le premier.

## Protocole commun

- `gpt-6.1-terra`, provider `openai-codex`, effort `high`.
- Les 10 tâches du banc (`bench/smoke/tasks/`, sans les exemples).
- 3 passes entrelacées par tâche (`tools/ab.py`) : à chaque passe, chaque tâche lance ses deux bras dos à dos, dans un ordre tiré au sort.
- Binaire : celui du dernier commit de la PR 2 (artefact de `ya-build-macos-arm64.yml`, installé par `tools/install-bin.sh`). Son SHA complet est noté ici avant le premier run.
- `DO_NOT_TRACK=1 PI_OFFLINE=1`, `PI_CACHE_RETENTION=short` (fixé par le runner), garde-fou d'inactivité 300 s.
- **Avant tout** : run de chauffe d'une tâche sur le défaut (`bench/smoke/settings/default.json`), pour vérifier que le backend de l'abonnement accepte le tool libre `apply_patch`. Il ne compte dans aucun A/B.
- Statistiques : `python3 -I bench/smoke/tools/ab_stats.py --plan <ab.json> --baseline bench/smoke/baseline/gpt-6.1-terra.json`. Tâches stables : `aggregate.tasks` de la baseline.
- **Paires écartées** : règle d'écart du README (« A/B : erreurs provider ») : une erreur provider (quota, surcharge, flux bloqué, statut `stalled`) sur un bras écarte la tâche de cette passe, ses deux bras. `ab_stats.py` l'applique et liste les unités écartées avec leur cause.

### Métriques d'édition (affichées dans les deux A/B, sans décider)

- **Edits appliqués au premier essai**, mesurés dans les deux bras : part des appels d'édition appliqués sans erreur, sur tous les appels d'édition de la session principale. Un appel d'édition est un appel du tool `apply_patch`, ou un appel de `edit()` dans une cellule du kernel. Un échec (patch refusé, `old_str` absent ou ambigu, fichier introuvable) compte comme un essai raté. Critère du CDC (YA-03) : au moins 95 %.
- **Cellules en erreur d'appel** (syntaxe, quoting), par bras.
- **Patchs refusés hors workspace** (`apply_patch` dont un chemin, après résolution des liens, sort du cwd de la session), par bras ; comptés aussi comme essais ratés.
- Calculées par `bench/smoke/metrics.py` (`edit_attempts`, `cell_errors`), sommées par bras dans la section « Par bras » d'`ab_stats.py`.

## A/B 1 : `editTool`

| Bras | Réglages | Édition | Prompt |
|---|---|---|---|
| A | `bench/smoke/settings/default.json` (`{}`) | tool `apply_patch` au format libre de Codex ; `edit()` du kernel gardé pour les éditions en lot | défaut de la PR 2 |
| B | `bench/smoke/settings/edit-kernel.json` (`{"editTool": "kernel"}`) | pas de tool d'édition ; `edit()` du kernel, documenté comme sur `main` | défaut de la PR 2 sans `apply_patch` |

`shellTool` vaut `kernel` dans les deux bras. Rien d'autre ne change : même binaire, mêmes tâches, même modèle, même effort.

Graine : **2290879124** (tirée de `os.urandom` le 13/11/2026 à 18:08Z, avant le run).

**Verdict par défaut : `editTool: native` (tool `apply_patch`).** La charge de la preuve est sur le bras `kernel` (B). Il est retenu seulement si l'une des deux conditions est remplie :

1. le bras `kernel` **résout au moins 3 tâches de plus** que le bras `native`, sur l'ensemble des paires valides. Lecture : « Tâches résolues par passe », totaux A et B de la section « Par bras ». Seuil fixé avant le run ; ce n'est pas un test statistique ;
2. le bras `kernel` **coûte moins** en coût à cache théorique des tâches stables, avec un IC bootstrap sur les tâches qui exclut zéro (ligne « tâches stables, théorique (décision) » de la section « Écart agrégé » d'`ab_stats.py`, somme B / somme A − 1, borne haute de l'IC 95 % < 0), **et** il ne résout pas 3 tâches ou plus de moins que le bras `native`.

**Affichés sans décider** : edits appliqués au premier essai (par bras), cellules en erreur d'appel, coût rapporté et pessimiste, tours, durée, miss serveur.

**Filet de régression (pas un verdict)** : le bras `kernel` (B), dont le prompt et les tools sont ceux de `main` à `read()`, `bash(env=)` et la note de troncature près, est aussi comparé à la baseline avec `compare.py` (effet de la PR 1, non entrelacé) : `python3 -I bench/smoke/compare.py <run A> <run B> --baseline bench/smoke/baseline/gpt-6.1-terra.json --out <fichier>.md`, avec en A un run des bras B de l'A/B `rlm.spawn` du 12/11 (source de la baseline, `ab-20261112T185001Z`) et en B un run des bras `kernel` de cet A/B. Les alertes de `compare.py` (seuils de la baseline) servent de filet ; elles ne changent pas le verdict.

## A/B 2 : `shellTool`

| Bras | Réglages | Exécution |
|---|---|---|
| A | l'`editTool` retenu par l'A/B 1, `shellTool: kernel` | `bash()` dans le kernel |
| B | l'`editTool` retenu par l'A/B 1, `shellTool: native` (`bench/smoke/settings/bash-native.json`, ou sa variante avec `editTool: kernel`) | tool shell séparé d'après `exec_command` de Codex (`codex-rs/core/src/tools/handlers/shell_spec.rs`, SHA `69231e1`), sans `bash()` dans le kernel ; sous-ensemble validé avant la PR 2 |

Graine : **2205082327** (tirée de `os.urandom` le 13/11/2026 à 18:08Z, avant le run).

**Verdict par défaut : `shellTool: kernel`** (décision du 28/10/2026). Le bras bash natif (B) est retenu seulement si l'une des deux conditions est remplie :

1. il **résout au moins 3 tâches de plus** sur les paires valides (seuil fixé avant le run, pas un test statistique) ;
2. il **coûte moins** en coût à cache théorique des tâches stables, avec un IC bootstrap sur les tâches qui exclut zéro (borne haute de l'IC 95 % < 0), **et** il ne résout pas 3 tâches ou plus de moins.

Le rapport dit que **la résolution ne se tranche pas à 10 tâches**. Il note aussi, comme biais possibles ou limites (décisions du 13/11/2026, journal du CDC) : le tool de la variante est `exec_command` / `write_stdin` de Codex **sans** `tty`, `shell` ni `login` (retrait qui peut jouer contre bash natif) ; le tool natif et le retrait de la règle de prompt « pas de boucles shell ni de heredocs » sont mesurés **ensemble** ; le tool natif compte sa sortie en octets, `python_repl` en caractères (différence seulement sur du texte non ASCII). La condition 2 est un écart avec YA-14, qui ne prévoit que la résolution (journal du CDC, 13/11/2026). Si le tool shell de la variante s'écarte de `exec_command` (format maison), le rapport dit que l'A/B 2 est biaisé contre bash natif. Verdict provisoire : une seule famille de modèles (YA-14 demande au moins deux familles au banc complet).

**Affichés sans décider** : comme pour l'A/B 1.

## Après les deux A/B

Nouvelle baseline `gpt-6.1-terra` depuis les bras de la configuration retenue (les 30 bras du bras retenu de l'A/B 2) : `python3 -I bench/smoke/baseline.py <runs> --out bench/smoke/baseline/gpt-6.1-terra.json`, comme le 12/11 (`bench/smoke/ab/rlm-spawn-20261112.md`). L'ancienne reste dans l'historique git.
