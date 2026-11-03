//! yusAgent fork defaults (YA-01): features outside the MVP start off. Each
//! one comes back through its settings key, listed in `docs/fork-changes.md`.
//! The `agent-message` and `rlm-heartbeat` skills stay on, with a switch.
//! Upstream getters read these constants instead of their literal defaults,
//! so a cherry-pick that touches one of them conflicts visibly.

/// `harnessRefinements` when unset (upstream: on).
pub const HARNESS_REFINEMENTS: bool = false;

/// `autoRefine.enabled` when unset (upstream: on).
pub const AUTO_REFINE_ENABLED: bool = false;

/// `autoRefine.compact` when unset (upstream: on).
pub const AUTO_REFINE_COMPACT: bool = false;

/// `telemetry.enabled` when no scope sets it (upstream: on, with a
/// first-run notice).
pub const TELEMETRY_ENABLED: bool = false;

/// `bundledSkills.agentMessage` and `bundledSkills.rlmHeartbeat` when unset
/// (upstream has no switch: always loaded). On: the static `core.md` layer
/// documents both, and `rlm.spawn` children answer their parent through
/// `agent_message.send`. Their fate is YA-09's call.
pub const BUNDLED_AGENT_MESSAGE: bool = true;
/// See [`BUNDLED_AGENT_MESSAGE`].
pub const BUNDLED_RLM_HEARTBEAT: bool = true;

/// The force-exclude overrides for the switchable bundled skills
/// (`bundledSkills.agentMessage`, `bundledSkills.rlmHeartbeat`): each one
/// set to `false` is left out, in the same `-<dir>/SKILL.md` form as the
/// websearch gate.
#[must_use]
pub fn disabled_bundled_skill_overrides(settings: &super::Settings) -> Vec<String> {
    let bundled = settings.bundled_skills.clone().unwrap_or_default();
    [
        (
            bundled.agent_message.unwrap_or(BUNDLED_AGENT_MESSAGE),
            "agent-message",
        ),
        (
            bundled.rlm_heartbeat.unwrap_or(BUNDLED_RLM_HEARTBEAT),
            "rlm-heartbeat",
        ),
    ]
    .into_iter()
    .filter(|(enabled, _)| !enabled)
    .map(|(_, dir)| format!("-{dir}/SKILL.md"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsManager;

    fn manager(settings_json: Option<&str>) -> (SettingsManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        if let Some(json) = settings_json {
            std::fs::write(agent_dir.join("settings.json"), json).unwrap();
        }
        (SettingsManager::create(dir.path(), &agent_dir), dir)
    }

    /// No `settings.json`: every out-of-MVP feature is off; the switchable
    /// skills stay on.
    #[test]
    fn a_fresh_install_starts_with_the_fork_defaults() {
        let (settings, _dir) = manager(None);
        assert!(!settings.get_harness_refinements());
        let gates = crate::session_engine::refine::AutoRefineGates::from_settings(
            settings.settings().auto_refine.as_ref(),
        );
        assert!(!gates.enabled);
        assert!(!gates.compact);
        assert!(!settings.get_telemetry_enabled());
        assert_eq!(settings.telemetry_enabled_setting(), None);
        assert!(disabled_bundled_skill_overrides(settings.settings()).is_empty());
    }

    /// Each feature flips through its settings key.
    #[test]
    fn each_default_is_reversible_in_settings() {
        let (settings, _dir) = manager(Some(
            r#"{
                "harnessRefinements": true,
                "autoRefine": { "enabled": true, "compact": true },
                "telemetry": { "enabled": true },
                "bundledSkills": { "agentMessage": false, "rlmHeartbeat": false }
            }"#,
        ));
        assert!(settings.get_harness_refinements());
        let gates = crate::session_engine::refine::AutoRefineGates::from_settings(
            settings.settings().auto_refine.as_ref(),
        );
        assert!(gates.enabled);
        assert!(gates.compact);
        assert!(settings.get_telemetry_enabled());
        assert_eq!(settings.telemetry_enabled_setting(), Some(true));
        assert_eq!(
            disabled_bundled_skill_overrides(settings.settings()),
            ["-agent-message/SKILL.md", "-rlm-heartbeat/SKILL.md"]
        );
    }

    /// One skill off, the other stays on; an explicit `true` keeps it.
    #[test]
    fn skill_switches_are_independent() {
        let (settings, _dir) = manager(Some(
            r#"{ "bundledSkills": { "agentMessage": true, "rlmHeartbeat": false } }"#,
        ));
        assert_eq!(
            disabled_bundled_skill_overrides(settings.settings()),
            ["-rlm-heartbeat/SKILL.md"]
        );
    }

    /// A project-scope `false` still wins over a global `true`, as upstream.
    #[test]
    fn telemetry_needs_every_setting_scope_to_agree() {
        let dir = tempfile::tempdir().unwrap();
        let agent_dir = dir.path().join("agent");
        let project_dir = dir.path().join(".prime/agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            agent_dir.join("settings.json"),
            r#"{ "telemetry": { "enabled": true } }"#,
        )
        .unwrap();
        std::fs::write(
            project_dir.join("settings.json"),
            r#"{ "telemetry": { "enabled": false } }"#,
        )
        .unwrap();
        let settings = SettingsManager::create(dir.path(), &agent_dir);
        assert!(!settings.get_telemetry_enabled());
    }
}
