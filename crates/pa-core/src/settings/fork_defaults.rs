//! yusAgent fork defaults (YA-01): features outside the MVP start off. Each
//! one comes back through its settings key, listed in `docs/fork-changes.md`.
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

/// The force-exclude overrides for the opt-in bundled skills
/// (`bundledSkills.agentMessage`, `bundledSkills.rlmHeartbeat`): each one
/// not set to `true` is left out, in the same `-<dir>/SKILL.md` form as the
/// websearch gate.
#[must_use]
pub fn opt_in_bundled_skill_overrides(settings: &super::Settings) -> Vec<String> {
    let bundled = settings.bundled_skills.clone().unwrap_or_default();
    [
        (bundled.agent_message, "agent-message"),
        (bundled.rlm_heartbeat, "rlm-heartbeat"),
    ]
    .into_iter()
    .filter(|(enabled, _)| !enabled.unwrap_or(false))
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

    /// No `settings.json`: every out-of-MVP feature is off.
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
        assert_eq!(
            opt_in_bundled_skill_overrides(settings.settings()),
            ["-agent-message/SKILL.md", "-rlm-heartbeat/SKILL.md"]
        );
    }

    /// Each feature comes back through its settings key.
    #[test]
    fn each_default_is_reversible_in_settings() {
        let (settings, _dir) = manager(Some(
            r#"{
                "harnessRefinements": true,
                "autoRefine": { "enabled": true, "compact": true },
                "telemetry": { "enabled": true },
                "bundledSkills": { "agentMessage": true, "rlmHeartbeat": true }
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
        assert!(opt_in_bundled_skill_overrides(settings.settings()).is_empty());
    }

    /// One skill on, the other stays off.
    #[test]
    fn opt_in_skills_are_independent() {
        let (settings, _dir) = manager(Some(r#"{ "bundledSkills": { "rlmHeartbeat": true } }"#));
        assert_eq!(
            opt_in_bundled_skill_overrides(settings.settings()),
            ["-agent-message/SKILL.md"]
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
