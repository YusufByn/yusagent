# Passation du 06/11/2026

État à la fermeture de la session du 06/11 (22h20, heure de Paris). `main` = `7257c4b`.

## Fait

- **#16** (T08b, YA-13), mergée (`db8b1ff`) : coût pessimiste du banc smoke à côté du coût rapporté, `input` non caché au tarif `cache_write_30m` (`bench/smoke/metrics.py`, `PESSIMISTIC_INPUT_PRICE`). Totaux, colonne du tableau des tâches, `tasks.csv`, `report.json` ; section du coût agrégé de `compare.py` avec le même détail, sans alerte. Aucun changement du harness. Détail : `bench/smoke/README.md`, « Coût pessimiste (T08b) ».
- **#17** (YA-00), mergée (`7257c4b`) : runner `macos-15` au lieu de `macos-14`, retiré de GitHub Actions le 02/11/2026 (actions/runner-images#15226), pour `ya-build-macos-arm64.yml`, le job darwin arm64 de `continuous.yml` et `release.yml:331`. Vérifié : `ya-build-macos-arm64` (run 42285074674) et le job `build-aarch64-apple-darwin` de `continuous.yml` sur `main` (run 42290071150) réussis. **`release.yml` non testé** : il ne tourne que sur un tag de release. Fichiers listés dans `docs/fork-changes.md`.

## A/B masquage en cours

- Lancé par Yusuf le 06/11 à 21h36 (heure donnée par Yusuf ; le journal de la boucle porte 20:22:23Z, soit 21h22 à Paris) :
  `bash bench/smoke/runs/ab-masking.sh ~/.cache/yusagent-bin/0fa8fad592d223fa5378453ea85cedf11a504e46 db8b1fffdc28da9aba2c6580a3c594bbd83c7832`
- Binaire `0fa8fad` (run 41908494807, dernier commit de `ya-08`) : sources livrées identiques à `db8b1ff`, vérifié par la boucle (`git diff` vide sur `crates`, `prime-agent-runtime`, `skills`, `vendor`, `install-rust.sh`, `Cargo.*`) ; mention écrite dans le journal et dans chaque rapport (section « Note de l'A/B masquage », et `ab-note.txt`). Rapports au commit `7257c4b`.
- 3 passes défaut / masquage, `gpt-6.1-terra`, effort `high`, `RUNS=1`, les 10 tâches de la baseline.
- À 22h18 : passe 1 `default` finie (`bench/smoke/runs/20261106T202245Z-0fa8fad-gpt-6.1-terra`, 7/10 résolues, coût 1.5268 $), passe 1 `masking-on` en cours (`20261106T211354Z-0fa8fad-gpt-6.1-terra`). La première a duré 51 min (20:22:45Z → 21:13:54Z) : fin des 6 passes vers 2h30, heure de Paris (**estimation**, 6 × 51 min).
- Journal : `bench/smoke/runs/ab-masking-20261106T202223Z.txt` ; il liste à la fin les dossiers de run dans l'ordre, pour `compare.py` (paires défaut / masquage, `--baseline bench/smoke/baseline/gpt-6.1-terra.json`).
- Résultats non lus : rien n'est conclu ici.

## Scripts

`bench/smoke/runs/install-bin.sh` et `bench/smoke/runs/ab-masking.sh`. **Non versionnés** : `bench/smoke/runs/` est ignoré par git (`bench/smoke/.gitignore`). Ils n'existent que sur la machine de Yusuf ; à déplacer dans `bench/smoke/tools/` et à committer s'ils doivent servir ailleurs.

- `install-bin.sh <run id> [<sha attendu>]` : télécharge l'artefact du run, vérifie `SHA256SUMS`, le `commit` du manifeste, le `executableSha256` de l'exécutable extrait et `--version`, installe dans `~/.cache/yusagent-bin/<sha>/`.
- `ab-masking.sh <dossier du binaire> [<sha de référence>]` : contrôles (binaire, version et exécutable au manifeste, dossier au sha, `masking-on.json` ne diffère de `default.json` que par `masking.enabled`, sources livrées identiques à la référence), puis la boucle.

## Défaut connu : artefact d'`install-bin.sh`

`gh run download <run>` récupère tous les artefacts du run, et le script prend le premier `SHA256SUMS` trouvé (`find … | head -1`). Pour `ya-build-macos-arm64` (un seul artefact, darwin arm64), c'est juste. Pour `continuous.yml` (4 artefacts, `artifacts-<cible>`), il peut prendre une cible Linux. Correction : `gh run download <run> -n artifacts-aarch64-apple-darwin` pour `continuous.yml`, ou un argument de nom d'artefact.

## Points ouverts (rappel)

- Relecture des 64 derniers Kio du journal d'usage au redémarrage (`RESTORE_TAIL_BYTES`, `crates/pa-core/src/session_engine/usage_log.rs:33`) : une dernière ligne plus longue remet `seq`, `turn` et `sincePrevMs` à zéro après `--continue`. Ticket 08, « Points ouverts ».
- Codex : `cache_write_tokens` toujours à 0 au run luna du 04/11, à recouper sur un run plus long (`docs/usage.md`).
- `release.yml` sur `macos-15` à vérifier à la prochaine release.
