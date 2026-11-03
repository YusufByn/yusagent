# Écarts avec upstream

Toute modification d'un fichier upstream, et tout ajout dans une zone gérée par upstream, est listé ici (CLAUDE.md règle 2).

| Fichier | Type | Raison | Ticket |
|---|---|---|---|
| `.github/workflows/ya-build-macos-arm64.yml` | ajout | Binaire macOS arm64 de HEAD en artefact CI pour les mesures du banc de capture, sans build natif local. Copie réduite du job `build-darwin` de `continuous.yml` (qui ne tourne que sur `main` et construit 4 cibles). Autorisé par Yusuf pour ce seul but. | YA-00 |
| `crates/pa-core/src/settings/fork_defaults.rs` | ajout | Défauts du fork pour les fonctions hors MVP (constantes, filtre des skills en opt-in) et leurs tests. Les getters upstream lisent ces constantes au lieu de leurs littéraux : un cherry-pick qui touche l'un d'eux entre en conflit de façon visible. | YA-01 |
| `crates/pa-core/src/settings/mod.rs` | modif. (1 ligne) | Déclare `fork_defaults`. | YA-01 |
| `crates/pa-core/src/settings/interactive_settings.rs` | modif. | `get_harness_refinements` : défaut `fork_defaults::HARNESS_REFINEMENTS` au lieu de `true`. | YA-01 |
| `crates/pa-core/src/session_engine/refine.rs` | modif. | `AutoRefineGates::default` : `enabled` et `compact` lus dans `fork_defaults`. Test des défauts mis à jour. | YA-01 |
| `crates/pa-core/src/settings/manager.rs` | modif. | `get_telemetry_enabled` : sans aucun réglage, renvoie `fork_defaults::TELEMETRY_ENABLED`. Dès qu'une portée fixe la valeur, la règle upstream s'applique (toutes les portées qui la fixent doivent dire oui). | YA-01 |
| `crates/pa-core/src/session_engine/telemetry/status.rs` | modif. | `TelemetrySwitch::Default` suit `fork_defaults::TELEMETRY_ENABLED` ; le motif affiché devient « off by default ». | YA-01 |
| `crates/pa-core/src/settings/types.rs` | modif. | `BundledSkillsSettings` : champs `agentMessage` et `rlmHeartbeat`. | YA-01 |
| `crates/pa-core/src/packages/resolve/auto.rs` | modif. (5 lignes) | Ajoute les exclusions des skills en opt-in à côté de celle de websearch. | YA-01 |
| Tests upstream (`pa-core` : `persistence_tests.rs`, `refinement_gate.rs`, `engine/tests.rs` ; `pa-cli` : `mode.rs`, `interactive_mode/tests.rs`, `interactive_mode/telemetry.rs`, `tests/telemetry_command_e2e.rs` ; `pa-daemon` : `agent_engine/tests.rs`, `model_allowlist.rs`, `supervisor/tests.rs`) | modif. | Les tests qui supposaient un défaut actif activent la fonction par réglage (même scénario qu'upstream) ; ceux qui vérifiaient le défaut vérifient le défaut du fork. Chaque modification porte un commentaire `yusAgent (YA-01)`. | YA-01 |
| `scripts/check_refine_errors.py`, `check_refine_request_headers.py`, `check_minimum_reasoning.py`, `check_late_message_receipt.py`, `check_restore_notice.py` | modif. | Checks de cycle de vie (shard 1) : la fixture active par réglage la fonction qu'elle exerce (`harnessRefinements`, `bundledSkills`). | YA-01 |
| `.github/workflows/ci.yml` | modif. (1 ligne + commentaire) | Étape `Prepare pinned TS frame comparison binary` : la release `v0.9.8` est téléchargée depuis `PrimeIntellect-ai/prime-agent` au lieu du dépôt courant. Voir ci-dessous. | YA-01 |
| `.changes/ya-01.md` | ajout | Fragment de changelog exigé par `changelog-fragment.yml` pour tout changement livré. | YA-01 |

## Désactivations par défaut (YA-01)

Session lancée sans `settings.json` ni variable d'environnement. Chaque ligne se réactive dans `settings.json` (global, `~/.prime/agent/settings.json`).

| Fonction | Défaut fork | Réactivation | Où |
|---|---|---|---|
| Télémétrie produit (et notice au premier lancement) | coupée | `"telemetry": {"enabled": true}` (ou `PRIME_AGENT_TELEMETRY=1`) | `fork_defaults.rs:17`, `manager.rs:850`, `status.rs:29` |
| Continual Harness (création de raffinements, digest du harness dans le contexte) | coupé | `"harnessRefinements": true` | `fork_defaults.rs:7`, `interactive_settings.rs:17` |
| `/refine` automatique (par intervalle de tours) | coupé | `"autoRefine": {"enabled": true}` (et `harnessRefinements`) | `fork_defaults.rs:10`, `refine.rs:42` |
| `/refine` automatique après compaction | coupé | `"autoRefine": {"compact": true}` (et `harnessRefinements`) | `fork_defaults.rs:13`, `refine.rs:44` |
| Skill `agent-message` | absent de l'inventaire et des imports du kernel | `"bundledSkills": {"agentMessage": true}` | `fork_defaults.rs:24-34`, `auto.rs:150-154` |
| Skill `rlm-heartbeat` | absent de l'inventaire et des imports du kernel | `"bundledSkills": {"rlmHeartbeat": true}` | idem |

Ce qui n'émet plus rien quand le réglage est faux :

- Télémétrie : le client n'envoie rien tant que le commutateur est faux (`crates/pa-core/src/session_engine/telemetry.rs:1162-1164`, porte de livraison sur tous les puits, miroir local compris) ; les compteurs ne comptent pas (`telemetry.rs:1185-1191`, `crates/pa-daemon/src/supervisor/notes.rs:42-48`) ; le mode print ne crée même pas de client (`crates/pa-cli/src/print_runtime.rs:730`) ; la notice n'est ni imprimée (`crates/pa-cli/src/telemetry_notice.rs:15`) ni affichée dans le TUI (`crates/pa-cli/src/client_settings.rs:297`).
- Raffinements : porte unique `crates/pa-core/src/session_engine/refinement_gate.rs:6-8`, lue à chaque requête, à la planification, à l'application et à la compaction.

Réglages cherchés sans résultat :

- **Vérification de version au lancement** : il n'y en a pas dans le portage Rust. `latest_release` (`crates/pa-core/src/update/release.rs:71`) n'est appelé que par les commandes explicites `prime-agent update` (`crates/pa-cli/src/update_flow/plan.rs:140`) et `prime-agent update --check` (`crates/pa-cli/src/installer_update.rs:269`). La vérification des mises à jour de packages (`crates/pa-core/src/packages/update.rs:54`) n'a aucun appelant. Aucun réglage ajouté.
- **Commande `schedule`** : aucun réglage dédié dans `Settings` (`crates/pa-core/src/settings/types.rs`). C'est une sous-commande CLI explicite (`crates/pa-cli/src/command_registry.rs:92`), qui ne s'exécute que si on l'appelle. Rien de modifié.

## CI : comparaison de frames avec le binaire TS (YA-01)

Le shard 1 de `ci.yml` téléchargeait la release `v0.9.8` depuis `${GITHUB_REPOSITORY}`. Le fork n'a pas de release, l'étape échouait et faisait échouer tout le shard.

Choix : télécharger la même archive depuis le dépôt upstream public (`PrimeIntellect-ai/prime-agent`, release `v0.9.8`, asset `prime-agent-0.9.8-linux-x64.tar.gz`). Le sha256 épinglé (`83fb0912…235e2`) reste vérifié dans `ci.yml` et dans les deux checks (`scripts/check_link_frames.py:170`, `scripts/check_ipython_diff_frames.py:192`).

- Aucun test retiré : les checks de frames TS/Rust continuent de tourner.
- Pas de release à publier ni à maintenir sur le fork.
- Dépendance : la CI suppose que la release upstream `v0.9.8` reste publique. Si elle disparaît, le sha256 ne correspondra plus ou le téléchargement échouera, de façon visible.
- Alternative écartée : sauter l'étape et les deux checks hors du dépôt upstream. Plus de lignes modifiées, et deux tests en moins.
