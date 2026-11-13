# Passation du 13/11/2026 (PR 2 de YA-03)

Note écrite en fin de session pour la session suivante, qui reprend la PR 2 de YA-03 (ticket `docs/tickets/09-ya03-base.md`). La PR n'est pas mergée ; les corrections demandées par Yusuf (en bas) ne sont pas encore appliquées. Pas de merge de la PR #26 sans le « merge » de Yusuf.

## État de la PR #26

- Branche `ya-03-outils`, depuis `main` à `0c9a956`. Dernier commit : `58f2542e4` (« YA-03: ToolChoice::default explicite dans le test de couches »).
- `ci.yml` : run **43504367477**, vert sur `58f2542` (19 jobs `success`). Les runs précédents de la branche ont échoué sur des lints clippy et sur deux tests de couches upstream, tous corrigés (voir « Pièges »).
- Build macOS (`ya-build-macos-arm64.yml`) : run 43501958186, vert sur `25c56570c`. `58f2542` ne change qu'un test : le binaire de `25c5657` a le code final. Binaire extrait dans `~/.cache/yusagent-bin/25c56570c/x/prime-agent`.
- La description de la PR #26 contient le tableau de capture et les cases cochées.

### Commits (dans l'ordre)

1. `5449b43` tool `apply_patch` au format libre de Codex et contrôle du workspace
2. `9fbbb7d` tools `exec_command` et `write_stdin` de la variante bash natif
3. `85e1945` kernel sans `bash` dans le namespace en variante native
4. `90740f9` aiguillage des tools natifs par famille de modèle (`editTool`, `shellTool`)
5. `96e906a` prompt des tools de la session
6. `f32adfd` enregistrement des tools natifs dans la session
7. `6f0b5d0` tests de préfixe Codex et du kernel en variante native
8. `ef3bf85` métriques d'édition et réglages des bras du banc
9. `bd9cccd` documentation de la PR 2
10. `c03f1b5` cas de la famille Codex au banc de capture
11. `e96a743` tests sans `format!` collecté (clippy `format_collect`)
12. `25c5657` note des skills et variables de session pour `exec_command`, tests de couches adaptés
13. `58f2542` `ToolChoice::default` explicite (clippy `default_trait_access`)

### Ce qui est fait

- **Aiguillage par famille** (`crates/pa-core/src/session_engine/ya_tool_family.rs`) : famille Codex = modèle de `CODEX_MODELS` (les 12 modèles de `codex-rs/models-manager/models.json` au SHA `69231e1`) servi par `openai`/`openai-responses`, `openai-codex`/`openai-codex-responses` ou `azure-openai-responses` ; tout le reste est « autre » (test : modèle inconnu en `openai-responses` → autre). `editTool` absent = `apply_patch` sur Codex, kernel ailleurs ; natif explicite hors Codex refusé ; changement de famille refusé dans `set_model` et `set_model_and_thinking_level`.
- **`apply_patch`** (`crates/pa-core/src/tools/ya_apply_patch.rs`) : tool `custom` (nom, description, grammaire Lark de Codex, `crates/ya-apply-patch/src/tool_spec.rs` et `assets/apply_patch.lark`, hash de blob identique à Codex). Contrôle de workspace de chaque chemin avant toute écriture (`crates/pa-core/src/tools/ya_workspace.rs` : `WorkspaceScope`, `allowed_roots`, `check_inside`, marqueur `[outside-workspace]`). Application par `ya_apply_patch::apply_patch` (liens suivis). Résultat : format `format_exec_output_for_model` de Codex **plus une zone modifiée numérotée** (à retirer, correction 2).
- **`exec_command` / `write_stdin`** (`crates/pa-core/src/tools/ya_exec.rs`) : schémas mot pour mot de `shell_spec.rs` sans `tty`, `shell`, `login` ni paramètres de sandbox ; sans PTY (stdin fermé, `write_stdin` sonde ou interrompt avec `"\u0003"`) ; environnement filtré du kernel plus `PRIME_AGENT_CONVERSATION_LOG` et `PRIME_AGENT_BUNDLED_SKILLS_DIR` ; budget de sortie de Codex en octets (4 octets par token, marge de 20 % pour l'en-tête), politique de session = `kernel.maxOutputChars` lu en octets ; en-tête de Codex sans `Chunk ID` ; identifiants séquentiels à partir de 1000.
- **Kernel en variante native** (`crates/pa-core/src/kernel/bootstrap/ya_native_shell.rs`) : `bash` non relié, attribut `rlm.bash` remplacé par une fonction qui lève une erreur ; `probe.rs` inchangé. Option `ya_native_shell` d'`IpythonKernelProvisionerOptions`, appliquée à chaque démarrage et redémarrage.
- **Prompt** (`crates/pa-core/src/prompts/ya_tools_prompt.rs`, sans toucher aux fichiers de couches) : `read()`, `bash(env=)`, ligne de troncature pour toutes les sessions ; `apply_patch` sur Codex ; variante `exec_command`. Deux ajouts au texte validé en variante native, signalés à Yusuf : ligne « Use f-strings … in `bash` » d'`opinionated.md` retirée ; note des skills embarqués « of the Python REPL and of `exec_command` ».
- **Banc** : `bench/smoke/metrics.py` (`edit_attempts` : edits appliqués au premier essai, `apply_patch` et `edit()`, patchs refusés hors workspace), `run.py` (champ `edits`, colonnes CSV), `tools/ab_stats.py` (totaux par bras avec les cellules en erreur d'appel), tests dans `tests/test_bench.py` ; réglages `bench/smoke/settings/edit-kernel.json`, `bash-native.json`, `bash-native-edit-kernel.json` ; `bench/smoke/tools/sizes.py` (estimation tiktoken, la capture fait foi).
- **Capture** : `bench/tools/capture/run.py` gagne les cas `codex-default`, `codex-edit-kernel`, `codex-shell-native` (provider intégré `openai` surchargé dans `models.json` vers le serveur local, modèle `gpt-6.1-terra`, `OPENAI_API_KEY` factice).
- **Docs** : `docs/fork-changes.md` (lignes YA-03 et section « Tools natifs (YA-03, PR 2) », « Tests upstream touchés (YA-03, PR 2) »), journal du CDC (3 lignes du 13/11 : nom `exec_command`, famille, workspace), points ouverts du ticket 09, notes de rapport dans `bench/smoke/ab/ya03-plan.md`, `.changes/ya-03-outils.md`, README de `ya-apply-patch`.
- **Tests** : 48 tests unitaires nouveaux (verts en CI) ; `crates/pa-core/tests/prefix_stability.rs` à 5 scénarios (2 upstream + défaut Codex, `editTool: kernel`, `shellTool: native`) ; `crates/pa-core/tests/ya_shell_native_kernel.rs` (vrai kernel : démarrage, `bash` absent par le nom et par `from rlm import bash`, `read()` et `edit()`, absence après redémarrage, témoin avec `bash` ; échoue en CI si le venv manque).

### Fichiers upstream touchés

`crates/pa-core/src/session_engine/engine.rs`, `session_engine/mod.rs`, `session_engine/runtime_wiring.rs`, `kernel/provisioner.rs`, `kernel/bootstrap/mod.rs`, `prompts/system_prompt.rs`, `prompts/mod.rs`, `tools/mod.rs`, `settings/types.rs`, `settings/load.rs`, `crates/pa-core/Cargo.toml`, `Cargo.lock` ; tests upstream : `crates/pa-core/tests/prompt_guards.rs` (`cached_prefix_is_stable_across_sessions`) et le test `default_prompt_is_layered_with_static_prefix_first` de `system_prompt.rs`. Tous listés dans `docs/fork-changes.md`.

### Résultat de la capture

Commandes (`DO_NOT_TRACK=1 PI_OFFLINE=1 PI_CACHE_RETENTION=short`) :

```
python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/04b219151ccfdde24813f4dc94b6c785901ce1bc/prime-agent --out bench/tools/capture/runs/ya03pr2-main-04b2191
python3 bench/tools/capture/run.py --binary ~/.cache/yusagent-bin/25c56570c/x/prime-agent --out bench/tools/capture/runs/ya03pr2-branch-25c5657
python3 -I bench/tools/capture/compare.py bench/tools/capture/runs/ya03pr2-main-04b2191 bench/tools/capture/runs/ya03pr2-branch-25c5657 --out bench/tools/capture/runs/ya03pr2-compare-25c5657.md
```

`04b2191` a le même code que `main` (`0c9a956` n'ajoute que la passation du 13/11). Première requête de chaque cas, en octets :

| Cas | Système | Tools | 1er message |
|---|---|---|---|
| 10 cas existants (`capture-model`, hors famille Codex) | +496 (`read()`, `bash(env=)`, troncature) | 0 | identique |
| `codex-edit-kernel` | +496 | 0 | identique |
| `codex-default` | +375 | +847 (`apply_patch`, `custom`) | identique |
| `codex-shell-native` | −2 342 | +2 317 (`apply_patch`, `exec_command`, `write_stdin`) | identique |

Un seul écart par bras. Le système de `codex-shell-native` ne contient plus `bash(`. Les dossiers `runs/` sont ignorés par git.

## Décisions validées pour la PR 2 (13/11/2026)

1. Tool shell de la variante : `exec_command` / `write_stdin` de Codex, pas un tool `bash` (écart de libellé avec le CDC, porté au journal). Retirés : `tty`, `shell`, `login` ; le rapport de l'A/B 2 note ce retrait comme biais possible contre bash natif.
2. Workspace = cwd de la session seul ; `metrics.py` compte par bras les patchs refusés par le contrôle.
3. Règle « no shell loops/heredocs » retirée du prompt en variante native ; le rapport de l'A/B 2 dit que le tool et cette règle sont mesurés ensemble.
4. Branche `ya-03-outils` depuis `origin/main` (`0c9a956`).
5. Le tool shell natif suit le même aiguillage par famille qu'`apply_patch` : Codex seule, refus explicite ailleurs. Pas de format câblé en dur.
6. Le contrôle des chemins lit les racines d'une seule fonction (défaut : cwd de la session), à reprendre par YA-11 et par un workspace multi-dossiers.
7. `max_output_tokens` et « Original token count » : méthode de Codex (4 octets par token, `codex-rs/utils/string/src/truncate.rs:6`, 94-107 ; `process_manager.rs:675-678`), marge de 20 % reprise à l'identique (`context.rs:571-575`, `output-truncation/src/lib.rs:22-24`). Écart accepté : le tool natif compte en octets, `python_repl` en caractères (à noter au rapport de l'A/B 2).
8. `sizes.py` copié dans `bench/smoke/tools/` ; les tailles qui font foi sont celles de la capture.
9. `bash-over-subprocess.md` (`glm-5.3*`) laissé tel quel, noté aux points ouverts du ticket 09.
10. `shell_in_kernel` par le bootstrap, sans toucher à `probe.rs` ; test `ya_shell_native_kernel.rs`.
11. Famille déduite du provider et du modèle (tableau de `models.json` de Codex), modèle inconnu = autre.
12. Avant de remplacer `rlm.bash` : appelants internes vérifiés (`rlm/repl.py`, `rlm/factory.py` importent le sous-module par `from .bash import ...` ; le skill `edit` n'utilise pas `bash`).
13. `editTool` non réglé = natif sur Codex, kernel ailleurs ; natif explicite hors Codex refusé ; `check_model_change` dans `set_model` et `set_model_and_thinking_level`.

## Pièges rencontrés

- **Pas de build Rust local** (CLAUDE.md, règle 8) : seul `rustfmt --edition 2021 --check <fichiers>` tourne en local (crate `ya-apply-patch` : `--edition 2024`, depuis son dossier). `Cargo.lock` se met à jour sans compiler avec `cargo metadata --format-version 1 --offline >/dev/null`.
- **Clippy pédantique en `-D warnings`** (`pa-core`) : `format_collect` (`(..).map(|i| format!(..)).collect::<String>()` refusé, utiliser `.collect::<Vec<_>>().concat()`), `default_trait_access` (`Default::default()` passé en argument refusé, écrire le type), `redundant_closure_for_method_calls`, `missing_errors_doc`, `doc_markdown` (mots en CamelCase hors backticks). Le job clippy s'arrête au premier crate en erreur : corriger puis attendre un autre passage.
- **Un push annule le run en cours de la branche** : attendre la fin des shards de test avant de pousser une correction clippy, pour ne pas perdre leurs résultats.
- **Tests upstream qui figent le texte des couches** : `prompt_guards.rs` et `system_prompt.rs` comparent le préfixe aux fichiers `core.md`/`usage.md` bruts ; tout changement du prompt par défaut les casse.
- **Queue dynamique du prompt** : `session_paths::BUNDLED_SKILLS_NOTE` (inventaire des skills) parlait de `bash()` ; trouvé par `prefix_stability`, pas par les tests de couches.
- **Banc de capture** : son modèle (`capture`/`capture-model`) est hors famille Codex ; les cas Codex surchargent le provider intégré `openai` dans `models.json` (`baseUrl` local).
- **Journaux CI** : `gh run view --log` refuse tant que le run tourne ; utiliser `gh api --allow-escape-sequences repos/YusufByn/yusagent/actions/jobs/<id>/logs | sed 's/\x1b\[[0-9;]*m//g'`. Les annotations d'un job : `gh api repos/YusufByn/yusagent/check-runs/<id>/annotations`.
- **Builds macOS annulés à 15 min** : annotation GitHub « The job was not acquired by Runner of type hosted even after multiple attempts » (capacité des runners macOS arm64), pas la concurrence (`ya-build-macos-arm64.yml:26-28`) ni le timeout (`:42`, 90 min). Relance unique sans push (règle de CLAUDE.md), cause vérifiée par les annotations. Correction de la passation du 13/11 : le run 43479083325 n'a pas été relancé, un push l'a remplacé.
- **Test instable** : `pa-daemon` `rlm_children_delete_e2e` (`a_deleted_running_child_records_the_cancel_edge`, « kernel cell receipt never appeared ») a échoué au run 43499374052 puis est passé sans changement aux runs 43501970149 et 43504367477. `main` a eu un autre test instable (`protocol_breadth_b6_b9`, run 43492287310).

## Commandes utiles

```
# Sources Codex au SHA de référence (exemple)
gh api "repos/openai/codex/contents/codex-rs/core/src/tools/handlers/shell_spec.rs?ref=69231e119bfca0b14453515e97d99688ce1c47c9" -H "Accept: application/vnd.github.raw"
# Binaire de branche
gh run download <run> --repo YusufByn/yusagent --dir <dossier>/dl   # puis shasum -a 256 -c SHA256SUMS et tar -xzf
# Tests du banc (sans modèle)
python3 -I -m unittest discover -s bench/smoke/tests
# Estimation de taille du prompt (tiktoken du venv d'analyse de la capture)
~/.cache/yusagent-capture/analysis-venv/bin/python -I bench/smoke/tools/sizes.py .
# Échecs d'un run
gh run view <run> --repo YusufByn/yusagent --json jobs --jq '.jobs[]|select(.conclusion!="success" and .conclusion!="skipped")|[.name,.conclusion,.databaseId]|@tsv'
```

## Corrections demandées par Yusuf (à appliquer, texte d'origine)

1. editTool et shellTool sont figés par session (décision du 13/11) : écris-les dans le fichier de session à la création et relis-les à la reprise. Si les réglages courants diffèrent à la reprise, garde ceux de la session et affiche un avertissement. Test : reprise avec des réglages différents, mêmes tools et même préfixe qu'avant la reprise. Porte-le à fork-changes.md si un fichier upstream est touché.
2. Sortie d'apply_patch : identique à Codex au SHA 69231e1, sans la zone modifiée numérotée (écart non validé, coût en tokens à chaque edit, non mesurable séparément). edit() du kernel garde sa zone numérotée.
3. Pose le label perf-test, attends le workflow perf, et donne la taille du binaire macOS arm64 contre main.
4. Confirme que prompt_guards.rs et le test de system_prompt.rs ne changent que les chaînes attendues, sans assertion retirée (donne le diff de ces deux tests).
5. Les cas Codex de la capture passent par le provider openai. Dis si la sérialisation du tool libre et du custom_tool_call diffère sur openai-codex-responses (fichier et ligne). Si oui, ajoute un test de sérialisation sur ce chemin.
6. Ajoute rlm_children_delete_e2e aux tests instables connus.

Après les corrections : nouveau passage au banc de capture (le format d'`apply_patch` ne change pas le préfixe, la correction 1 non plus si elle tient), CI verte du dernier commit, puis « merge » de Yusuf. Ensuite : SHA du binaire noté dans `bench/smoke/ab/ya03-plan.md`, run de chauffe lancé par Yusuf, A/B 1.
