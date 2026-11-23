//! yusAgent (YA-10, ticket 31): skills and MCP servers changed during a session are announced at
//! the end of the history (`docs/tickets/31-skills-mcp-reglages.md`, §3).
//!
//! The system prompt keeps the skills inventory and the generic MCP servers of the session's
//! opening: it is never rebuilt in a running session. At each admitted prompt, before the
//! next-turn rows are taken, the session compares what the model was told (the opening's
//! inventory and servers, then each announcement) with what the disk holds now, read as the
//! creation reads it. A difference queues one custom row ([`CUSTOM_TYPE`]) ahead of the prompt:
//! the system prompt and the tools stay the same and the history only grows, so the prompt cache
//! keeps its prefix. A change undone before the prompt announces nothing. The texts are fixed
//! (ticket 31, §3.2, validated by Yusuf): no date, no counter.
//!
//! The same tracker carries `compaction.enabled` as last read from the settings files: a saved
//! change applies to the running session at its next decision (`ya_compaction_enabled`).
//!
//! The comparison's duration goes to the compaction trace (`PA_COMPACTION_TRACE`), phase
//! [`TRACE_PHASE`], `detail.micros`. Listed in `docs/fork-changes.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::AgentSession;
use crate::prompts::ya_prompt_v2::{shown_skill, ShownSkill};
use crate::skills::Skill;

/// The custom type of an announcement row.
pub const CUSTOM_TYPE: &str = "ya-resources-changed";
/// The trace phase of one comparison.
pub const TRACE_PHASE: &str = "ya.resource_notice.compare";

const SKILLS_HEADER: &str = "[skills-changed]";
const SKILLS_ADDED: &str = "Added (read its SKILL.md before using it):";
const SKILLS_CHANGED: &str = "Changed (read its SKILL.md again):";
const REMOVED: &str = "Removed (no longer available, do not use):";
const LOAD_STEPS: &str = "To import a Python skill added here, run in a cell:";
const LOAD_IMPORTS: &str = "import importlib, os, shutil, subprocess, sys";
const MCP_HEADER: &str = "[mcp-changed]";
const MCP_SENTENCE: &str = "Generic MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.";

/// What a session reads to find its skills and MCP servers, fixed at its creation.
#[derive(Debug, Clone)]
pub struct Inputs {
    cwd: PathBuf,
    agent_dir: PathBuf,
    additional_skill_paths: Vec<String>,
    /// The embedding's built-in skill overrides, without the MCP ones (read again each time).
    extra_builtin_skill_overrides: Vec<String>,
    /// The embedding's generic MCP servers, without those of the settings (read again).
    generic_mcp_servers: Vec<String>,
    /// Skills the creation kept out of the prompt for want of their host handler.
    hidden: Vec<String>,
    /// The session can spawn or is a child (`ya_prompt_v2::skills_inventory`).
    subagents: bool,
}

/// What the model was told: the skills its inventory shows and the enabled generic MCP servers.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Known {
    skills: Vec<ShownSkill>,
    servers: Vec<String>,
}

impl Known {
    fn of(skills: &[Skill], servers: &[String], inputs: &Inputs) -> Self {
        let location = crate::prompts::session_paths::skill_location;
        let skills = skills
            .iter()
            .filter(|skill| !inputs.hidden.contains(&skill.name))
            .filter_map(|skill| shown_skill(skill, inputs.subagents, &location))
            .collect();
        let mut servers = servers.to_vec();
        servers.sort();
        servers.dedup();
        Self { skills, servers }
    }
}

/// A session's inputs and what its model was told so far.
#[derive(Debug)]
pub struct Tracker {
    inputs: Inputs,
    known: Known,
    /// `compaction.enabled` in the settings files when last read (`ya_compaction_enabled`).
    compaction_enabled_on_disk: bool,
}

impl Tracker {
    /// The tracker of a session at its creation: `loaded` the skills it loaded, `visible` those its
    /// prompt lists (`runtime_wiring::model_visible_skills`), `servers` the generic MCP servers its
    /// prompt names.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn at_creation(
        cwd: &Path,
        agent_dir: &Path,
        additional_skill_paths: &[String],
        extra_builtin_skill_overrides: &[String],
        embedding_servers: &[String],
        rlm_depth: Option<u32>,
        session_file: Option<&str>,
        loaded: &[Skill],
        visible: &[Skill],
        servers: &[String],
    ) -> Self {
        let subagents = rlm_depth.unwrap_or(0) > 0
            || crate::prompts::rlm_off::root_max_depth(rlm_depth, session_file, cwd, agent_dir)
                .is_some_and(|depth| depth > 0);
        let hidden = loaded
            .iter()
            .filter(|skill| !visible.iter().any(|shown| shown.name == skill.name))
            .map(|skill| skill.name.clone())
            .collect();
        let inputs = Inputs {
            cwd: cwd.to_path_buf(),
            agent_dir: agent_dir.to_path_buf(),
            additional_skill_paths: additional_skill_paths.to_vec(),
            extra_builtin_skill_overrides: extra_builtin_skill_overrides.to_vec(),
            generic_mcp_servers: embedding_servers.to_vec(),
            hidden,
            subagents,
        };
        let known = Known::of(visible, servers, &inputs);
        let compaction_enabled_on_disk =
            crate::settings::SettingsManager::create(cwd, agent_dir).get_compaction_enabled();
        Self {
            inputs,
            known,
            compaction_enabled_on_disk,
        }
    }
}

/// The skills and generic MCP servers on disk now, read as `engine::create_session_with` reads
/// them (settings, MCP gating, resource resolution); `None` when the resources cannot be loaded.
fn current(inputs: &Inputs) -> Option<Known> {
    let settings = crate::settings::SettingsManager::create(&inputs.cwd, &inputs.agent_dir);
    let user_servers = settings
        .global_settings()
        .mcp_servers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(server, config)| {
            serde_json::from_value(config)
                .ok()
                .map(|parsed| (server, parsed))
        })
        .collect::<HashMap<String, crate::mcp::McpServerConfig>>();
    let (mcp_overrides, mcp_servers, _manager) =
        crate::mcp::McpManager::prompt_gating(user_servers, &inputs.agent_dir);
    let mut overrides = inputs.extra_builtin_skill_overrides.clone();
    overrides.extend(mcp_overrides);
    let resources = crate::resources::load_resources(crate::resources::ResourceLoaderOptions {
        cwd: inputs.cwd.clone(),
        agent_dir: inputs.agent_dir.clone(),
        settings: Some(settings),
        extra_builtin_skill_overrides: overrides,
        additional_skill_paths: inputs.additional_skill_paths.clone(),
        no_prompt_templates: true,
        no_context_files: true,
        ..Default::default()
    })
    .ok()?;
    let mut servers = inputs.generic_mcp_servers.clone();
    for server in mcp_servers {
        if !servers.contains(&server) {
            servers.push(server);
        }
    }
    Some(Known::of(&resources.skills, &servers, inputs))
}

fn ticked(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The folder of a skill's file, as its location shows it.
fn folder(place: &str) -> String {
    Path::new(place)
        .parent()
        .map_or_else(|| place.to_string(), |dir| dir.display().to_string())
}

/// The line of a skill the running kernel has not imported: no import, the package named.
fn not_imported_line(skill: &ShownSkill, import: &str) -> String {
    format!(
        "- `{}` (Python package `{import}`, not imported in this kernel): {} `{}`",
        skill.name, skill.description, skill.place
    )
}

/// The `[skills-changed]` block, its loaded Python skills and the `details` of each kind.
fn skills_block(before: &[ShownSkill], after: &[ShownSkill]) -> (Option<String>, Value) {
    let find = |list: &[ShownSkill], name: &str| list.iter().position(|s| s.name == name);
    let added: Vec<&ShownSkill> = after
        .iter()
        .filter(|skill| find(before, skill.name.as_str()).is_none())
        .collect();
    let changed: Vec<&ShownSkill> = after
        .iter()
        .filter(|skill| find(before, skill.name.as_str()).is_some_and(|at| before[at] != **skill))
        .collect();
    let removed: Vec<&str> = before
        .iter()
        .filter(|skill| find(after, skill.name.as_str()).is_none())
        .map(|skill| skill.name.as_str())
        .collect();
    let names = |list: &[&ShownSkill]| list.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    let details = json!({ "added": names(added.as_slice()), "changed": names(changed.as_slice()), "removed": removed });
    if added.is_empty() && changed.is_empty() && removed.is_empty() {
        return (None, details);
    }
    // A Python skill the kernel did not import at its start: added, or whose import is new.
    let to_load = |skill: &ShownSkill| -> Option<String> {
        let import = skill.import.clone()?;
        let had = find(before, skill.name.as_str()).and_then(|at| before[at].import.clone());
        (had.as_deref() != Some(import.as_str())).then_some(import)
    };
    let line = |skill: &ShownSkill| match to_load(skill) {
        Some(import) => not_imported_line(skill, &import),
        None => skill.line(),
    };
    let mut lines = vec![SKILLS_HEADER.to_string()];
    for (header, list) in [(SKILLS_ADDED, &added), (SKILLS_CHANGED, &changed)] {
        if !list.is_empty() {
            lines.push(header.to_string());
            lines.extend(list.iter().map(|&skill| line(skill)));
        }
    }
    if !removed.is_empty() {
        lines.push(format!("{REMOVED} {}", ticked(&removed)));
    }
    let loads: Vec<(&ShownSkill, String)> = added
        .iter()
        .chain(&changed)
        .copied()
        .filter_map(|skill| to_load(skill).map(|import| (skill, import)))
        .collect();
    if !loads.is_empty() {
        lines.push(LOAD_STEPS.to_string());
        lines.push(LOAD_IMPORTS.to_string());
        for (skill, _) in &loads {
            lines.push(format!(
                "subprocess.run([shutil.which(\"uv\") or os.path.expanduser(\"~/.local/bin/uv\"), \"pip\", \"install\", \"--python\", sys.executable, os.path.expandvars(\"{}\")], check=True)",
                folder(&skill.place)
            ));
        }
        let imports: Vec<&str> = loads.iter().map(|(_, import)| import.as_str()).collect();
        lines.push(format!(
            "importlib.invalidate_caches(); import {}",
            imports.join(", ")
        ));
    }
    (Some(lines.join("\n")), details)
}

/// The `[mcp-changed]` block and its `details`.
fn mcp_block(before: &[String], after: &[String]) -> (Option<String>, Value) {
    let added: Vec<&str> = after
        .iter()
        .filter(|server| !before.contains(server))
        .map(String::as_str)
        .collect();
    let removed: Vec<&str> = before
        .iter()
        .filter(|server| !after.contains(server))
        .map(String::as_str)
        .collect();
    let details = json!({ "added": added, "removed": removed });
    if added.is_empty() && removed.is_empty() {
        return (None, details);
    }
    let enabled: Vec<&str> = after.iter().map(String::as_str).collect();
    let mut lines = vec![
        MCP_HEADER.to_string(),
        MCP_SENTENCE.to_string(),
        format!(
            "Enabled generic MCP servers now: {}.",
            if enabled.is_empty() {
                "none".to_string()
            } else {
                ticked(&enabled)
            }
        ),
    ];
    if !added.is_empty() {
        lines.push(format!("Added: {}.", ticked(&added)));
    }
    if !removed.is_empty() {
        lines.push(format!("{REMOVED} {}.", ticked(&removed)));
    }
    (Some(lines.join("\n")), details)
}

/// The announcement of what changed from `before` to `after`, with its `details`; `None` when
/// nothing the model sees changed.
fn announcement(before: &Known, after: &Known) -> Option<(String, Value)> {
    let (skills, skills_details) = skills_block(&before.skills, &after.skills);
    let (mcp, mcp_details) = mcp_block(&before.servers, &after.servers);
    let blocks: Vec<String> = [skills, mcp].into_iter().flatten().collect();
    (!blocks.is_empty()).then(|| {
        (
            blocks.join("\n\n"),
            json!({ "skills": skills_details, "mcp": mcp_details }),
        )
    })
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

impl AgentSession {
    /// Installs the session's tracker (`engine::create_session_with`).
    pub(crate) fn ya_set_resource_tracker(&self, tracker: Tracker) {
        *self
            .ya_resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tracker);
    }

    /// `compaction.enabled` for a decision: the settings files win when their value changed since
    /// they were last read (a save in the extension, the TUI or by hand applies to the running
    /// session, as `thresholdPercent` does, `wiring.rs`), and become the session's toggle; otherwise
    /// `live`, the session's toggle (`set_auto_compaction_enabled`) is kept. A session built
    /// without a tracker (outside `engine::create_session_with`) keeps `live`.
    pub(crate) fn ya_compaction_enabled(&self, live: bool) -> bool {
        let mut guard = self
            .ya_resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(tracker) = guard.as_mut() else {
            return live;
        };
        let on_disk = crate::settings::SettingsManager::create(
            &tracker.inputs.cwd,
            &tracker.inputs.agent_dir,
        )
        .get_compaction_enabled();
        if on_disk == tracker.compaction_enabled_on_disk {
            return live;
        }
        tracker.compaction_enabled_on_disk = on_disk;
        drop(guard);
        self.compaction
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enabled = on_disk;
        on_disk
    }

    /// Compares what the model was told with the disk; a difference queues its announcement as
    /// a next-turn row (module docs). Called at the admission of a prompt, before the next-turn
    /// rows are taken.
    pub(crate) async fn ya_announce_resources(&self) {
        let inputs = self
            .ya_resources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|tracker| tracker.inputs.clone());
        let Some(inputs) = inputs else {
            return;
        };
        let started = std::time::Instant::now();
        let current = tokio::task::spawn_blocking(move || current(&inputs))
            .await
            .ok()
            .flatten();
        super::compaction_trace::trace(
            TRACE_PHASE,
            &json!({ "micros": u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX) }),
        );
        let Some(current) = current else {
            return;
        };
        let row = {
            let mut guard = self
                .ya_resources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(tracker) = guard.as_mut() else {
                return;
            };
            let row = announcement(&tracker.known, &current);
            tracker.known = current;
            row
        };
        if let Some((text, details)) = row {
            self.queue_next_turn_row(pa_types::session::CustomMessage {
                custom_type: CUSTOM_TYPE.to_string(),
                content: pa_types::ai::UserContent::Text(text),
                display: true,
                details: Some(details),
                timestamp: now_millis(),
                rest: serde_json::Map::default(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str, import: Option<&str>) -> ShownSkill {
        ShownSkill {
            name: name.to_string(),
            description: description.to_string(),
            place: format!("/home/u/.yusagent/skills/{name}/SKILL.md"),
            import: import.map(str::to_string),
        }
    }

    fn known(skills: Vec<ShownSkill>, servers: &[&str]) -> Known {
        Known {
            skills,
            servers: servers.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn the_texts_of_section_3_2() {
        let none = known(Vec::new(), &[]);
        let deploy = skill("deploy", "Deploy the app to staging.", None);
        let (text, _) = announcement(&none, &known(vec![deploy], &[])).unwrap();
        assert_eq!(
            text,
            "[skills-changed]\nAdded (read its SKILL.md before using it):\n- `deploy`: Deploy the app to staging. `/home/u/.yusagent/skills/deploy/SKILL.md`"
        );
        let old = skill("old-helper", "Old.", None);
        let linear = skill("linear", "Linear.", None);
        let (text, details) =
            announcement(&known(vec![old, linear], &[]), &known(Vec::new(), &[])).unwrap();
        assert_eq!(
            text,
            "[skills-changed]\nRemoved (no longer available, do not use): `old-helper`, `linear`"
        );
        assert_eq!(
            details["skills"]["removed"],
            json!(["old-helper", "linear"])
        );
        let pdf = skill(
            "pdf-tools",
            "Extract text from PDF files.",
            Some("pdf_tools"),
        );
        let (text, _) = announcement(&none, &known(vec![pdf], &[])).unwrap();
        assert_eq!(
            text,
            "[skills-changed]\nAdded (read its SKILL.md before using it):\n- `pdf-tools` (Python package `pdf_tools`, not imported in this kernel): Extract text from PDF files. `/home/u/.yusagent/skills/pdf-tools/SKILL.md`\nTo import a Python skill added here, run in a cell:\nimport importlib, os, shutil, subprocess, sys\nsubprocess.run([shutil.which(\"uv\") or os.path.expanduser(\"~/.local/bin/uv\"), \"pip\", \"install\", \"--python\", sys.executable, os.path.expandvars(\"/home/u/.yusagent/skills/pdf-tools\")], check=True)\nimportlib.invalidate_caches(); import pdf_tools"
        );
        let (text, _) = announcement(
            &known(Vec::new(), &["slack"]),
            &known(Vec::new(), &["github", "slack"]),
        )
        .unwrap();
        assert_eq!(
            text,
            "[mcp-changed]\nGeneric MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.\nEnabled generic MCP servers now: `github`, `slack`.\nAdded: `github`."
        );
        let (text, _) =
            announcement(&known(Vec::new(), &["slack"]), &known(Vec::new(), &[])).unwrap();
        assert_eq!(
            text,
            "[mcp-changed]\nGeneric MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.\nEnabled generic MCP servers now: none.\nRemoved (no longer available, do not use): `slack`."
        );
    }

    #[test]
    fn skills_and_mcp_together_make_one_row_and_nothing_changed_none() {
        let before = known(vec![skill("a", "A.", None)], &[]);
        let after = known(
            vec![skill("a", "A, changed.", None), skill("b", "B.", None)],
            &["s"],
        );
        let (text, details) = announcement(&before, &after).unwrap();
        assert_eq!(
            text,
            "[skills-changed]\nAdded (read its SKILL.md before using it):\n- `b`: B. `/home/u/.yusagent/skills/b/SKILL.md`\nChanged (read its SKILL.md again):\n- `a`: A, changed. `/home/u/.yusagent/skills/a/SKILL.md`\n\n[mcp-changed]\nGeneric MCP connections are accessed through the pre-imported Python `mcp` object in the Python REPL.\nEnabled generic MCP servers now: `s`.\nAdded: `s`."
        );
        assert_eq!(details["skills"]["changed"], json!(["a"]));
        assert_eq!(details["mcp"]["added"], json!(["s"]));
        assert!(announcement(&after, &after).is_none());
    }
}
