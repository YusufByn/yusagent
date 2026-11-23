//! yusAgent (YA-10, ticket 31): the extension's skills, MCP and settings screens on the bridge,
//! `_yus/skills/*`, `_yus/mcp/*` and `_yus/settings/*` (`docs/tickets/31-skills-mcp-reglages.md`).
//! Methods and answers follow Unharnessed's `_unharnessed/*` (`crates/uh-daemon/src/acp/skills.rs`,
//! `mcp_servers.rs`, `settings.rs` at `f8e4658dd`), renamed for yusAgent (decision 1). Unlike
//! Unharnessed, nothing waits for a restart: the engine announces skills and MCP changes at the
//! next prompt (`pa_core::session_engine::ya_resource_notice`), and the settings of the screen
//! are read at each use (`settings`), so every answer has `restart` empty or false.
//!
//! Skills and settings are files the bridge reads and writes itself, in the global settings of
//! the agent dir; MCP connections go through the worker's commands (`get_mcp_connections`,
//! `set_mcp_static_token`, `remove_mcp_connection`), whose manager the kernel's `mcp` uses.
//! Listed in `docs/fork-changes.md`.

mod settings;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pa_core::settings::{
    FileSettingsStorage, InMemorySettingsStorage, SettingsManager, SettingsScope, SettingsStorage,
};
use pa_core::skills::{ResourceDiagnostic, Skill};
use pa_types::daemon::DaemonCommand;
use serde_json::{json, Map, Value};

use super::daemon::DaemonLink;

const SKILLS_LIST: &str = "_yus/skills/list";
const SKILLS_SET: &str = "_yus/skills/set";
const SKILLS_CREATE: &str = "_yus/skills/create";
const SKILLS_ADD_FOLDER: &str = "_yus/skills/add_folder";
const SKILLS_REMOVE_FOLDER: &str = "_yus/skills/remove_folder";
const MCP_LIST: &str = "_yus/mcp/list";
const MCP_SET_TOKEN: &str = "_yus/mcp/set_token";
const MCP_LOGOUT: &str = "_yus/mcp/logout";

/// The body of a skill created from the screen (Unharnessed's).
const NEW_SKILL_BODY: &str = "What the agent should know and do when this skill applies.";

/// What a call needs: the worker link and session, the session's folder.
pub(crate) struct Bound<'a> {
    pub(crate) link: &'a Arc<DaemonLink>,
    pub(crate) daemon_session_id: &'a str,
    pub(crate) cwd: &'a Path,
}

/// Why a call did not answer.
enum Refusal {
    /// The params: `-32602` with `details`.
    Invalid(String),
    /// Anything else: an internal error.
    Failed(String),
}

type Answer = Result<Value, Refusal>;

/// Whether [`serve`] answers `method`.
pub(crate) fn serves(method: &str) -> bool {
    [
        SKILLS_LIST,
        SKILLS_SET,
        SKILLS_CREATE,
        SKILLS_ADD_FOLDER,
        SKILLS_REMOVE_FOLDER,
        MCP_LIST,
        MCP_SET_TOKEN,
        MCP_LOGOUT,
    ]
    .contains(&method)
        || settings::serves(method)
}

/// Answers one call.
pub(crate) async fn serve(
    bound: &Bound<'_>,
    method: &str,
    id: &Value,
    params: &Value,
    tx: &super::producer::FrameSink,
) {
    let answer = match pa_types::platform::dirs::agent_dir() {
        None => Err(Refusal::Failed(
            "the agent directory does not resolve (no home directory)".to_string(),
        )),
        Some(agent_dir) => match method {
            MCP_LIST | MCP_SET_TOKEN | MCP_LOGOUT => mcp(bound, &agent_dir, method, params).await,
            _ if settings::serves(method) => {
                settings::answer(bound.cwd, &agent_dir, method, params)
            }
            _ => skills(bound, &agent_dir, method, params).await,
        },
    };
    let response = match answer {
        Ok(result) => super::jsonrpc::response(id, &result),
        Err(Refusal::Invalid(details)) => super::jsonrpc::error_response(
            id,
            super::jsonrpc::INVALID_PARAMS,
            "Invalid params",
            Some(&json!({ "details": details })),
        ),
        Err(Refusal::Failed(details)) => super::internal_error(id, &details),
    };
    let _ = tx.send(response);
}

fn string_param(params: &Value, name: &str) -> Result<String, Refusal> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Refusal::Invalid(format!("{name} must be a string")))
}

/// A path as the screen shows it: `~` for the home folder.
fn shown(path: &Path) -> String {
    match pa_types::platform::home_dir()
        .and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// `~` and `~/…` expanded.
fn expanded(path: &str) -> PathBuf {
    match (path.strip_prefix('~'), pa_types::platform::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

// Skills

/// An entry of the `skills` setting that is a pattern, not a folder (`!`, `+`, `-`).
fn is_override(entry: &str) -> bool {
    entry.starts_with(['!', '+', '-'])
}

/// The global `skills` setting.
fn global_skill_entries(cwd: &Path, agent_dir: &Path) -> Vec<String> {
    SettingsManager::create(cwd, agent_dir)
        .global_settings()
        .skills
        .clone()
        .unwrap_or_default()
}

/// The settings documents with the patterns of their `skills` arrays removed: every skill, the
/// ones turned off included.
fn without_skill_patterns(cwd: &Path, agent_dir: &Path) -> SettingsManager {
    let files = FileSettingsStorage::new(cwd, agent_dir);
    let memory = InMemorySettingsStorage::default();
    for scope in [SettingsScope::Global, SettingsScope::Project] {
        let Ok(Some(text)) = files.read(scope) else {
            continue;
        };
        let Ok(mut document) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(entries) = document.get_mut("skills").and_then(Value::as_array_mut) {
            entries.retain(|entry| entry.as_str().is_some_and(|entry| !is_override(entry)));
        }
        let content = document.to_string();
        let _ = memory.with_lock(scope, &mut |_| Some(content.clone()));
    }
    SettingsManager::from_storage(Arc::new(memory))
}

/// The skills the session would load, as `engine::create_session_with` resolves them;
/// `settings`: other settings than the files.
fn load(
    cwd: &Path,
    agent_dir: &Path,
    settings: Option<SettingsManager>,
) -> Result<pa_core::resources::LoadedResources, Refusal> {
    pa_core::resources::load_resources(pa_core::resources::ResourceLoaderOptions {
        cwd: cwd.to_path_buf(),
        agent_dir: agent_dir.to_path_buf(),
        settings,
        no_prompt_templates: true,
        no_context_files: true,
        ..Default::default()
    })
    .map_err(|error| Refusal::Failed(format!("the skills could not be read: {error}")))
}

/// The names of the skills the running session has (`get_resource_snapshot`); `None` when its
/// engine is not built yet (it loads the disk at its first prompt).
async fn active_skills(bound: &Bound<'_>) -> Option<Vec<String>> {
    let response = bound
        .link
        .request(DaemonCommand::GetResourceSnapshot {
            id: None,
            active_session_id: bound.daemon_session_id.to_string(),
            rest: Map::new(),
        })
        .await
        .ok()?;
    let names: Vec<String> = response
        .data?
        .get("skills")?
        .as_array()?
        .iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    // A built engine always has the bundled skills.
    (!names.is_empty()).then_some(names)
}

fn source(skill: &Skill, cwd: &Path, agent_dir: &Path) -> &'static str {
    if pa_core::prompts::session_paths::skill_location(&skill.file_path).starts_with('$') {
        "builtin"
    } else if skill.file_path.starts_with(agent_dir.join("skills")) {
        "user"
    } else if skill.file_path.starts_with(cwd) {
        "project"
    } else {
        "folder"
    }
}

fn problem(diagnostic: &ResourceDiagnostic) -> Value {
    match diagnostic {
        ResourceDiagnostic::Warning { message, path }
        | ResourceDiagnostic::Error { message, path } => {
            let mut problem = json!({ "message": message });
            if let Some(path) = path {
                problem["path"] = json!(shown(Path::new(path)));
            }
            problem
        }
        ResourceDiagnostic::Collision { message, path, .. } => {
            json!({ "message": message, "path": shown(Path::new(path)) })
        }
    }
}

/// The skills screen: every skill, whether the next prompt has it (`enabled`) and whether the
/// running session has it (`active`), the folders added here, what could not be loaded.
async fn skills_view(bound: &Bound<'_>, agent_dir: &Path) -> Answer {
    let cwd = bound.cwd;
    let enabled: Vec<PathBuf> = load(cwd, agent_dir, None)?
        .skills
        .into_iter()
        .map(|skill| skill.file_path)
        .collect();
    let all = load(cwd, agent_dir, Some(without_skill_patterns(cwd, agent_dir)))?;
    let active = active_skills(bound).await;
    let skills: Vec<Value> = all
        .skills
        .iter()
        .map(|skill| {
            let on = enabled.contains(&skill.file_path);
            let folder = skill
                .file_path
                .parent()
                .filter(|_| skill.file_path.ends_with("SKILL.md"))
                .unwrap_or(skill.file_path.as_path());
            json!({
                "name": skill.name,
                "description": skill.description,
                "path": skill.file_path.display().to_string(),
                "location": shown(folder),
                "source": source(skill, cwd, agent_dir),
                "enabled": on,
                "active": active.as_ref().map_or(on, |names| names.contains(&skill.name)),
            })
        })
        .collect();
    let folders: Vec<String> = global_skill_entries(cwd, agent_dir)
        .into_iter()
        .filter(|entry| !is_override(entry))
        .collect();
    let problems: Vec<Value> = all.skill_diagnostics.iter().map(problem).collect();
    Ok(json!({ "skills": skills, "folders": folders, "problems": problems, "restart": false }))
}

/// The folder a `-` / `+` entry names for a skill file: its folder for a `SKILL.md`.
fn skill_target(file: &Path) -> PathBuf {
    match (file.file_name(), file.parent()) {
        (Some(name), Some(folder)) if name == "SKILL.md" => folder.to_path_buf(),
        _ => file.to_path_buf(),
    }
}

fn write_skill_entries(cwd: &Path, agent_dir: &Path, entries: Vec<String>) {
    SettingsManager::create(cwd, agent_dir).set_global_resource_array("skills", entries);
}

/// Turns the skill of `file` on or off in the global `skills` setting: off adds `-<folder>`; on
/// removes it, and adds `+<folder>` when a `!` pattern still turns it off (Unharnessed's rule).
async fn set_skill(bound: &Bound<'_>, agent_dir: &Path, params: &Value) -> Answer {
    let file = PathBuf::from(string_param(params, "path")?);
    let enabled = params
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| Refusal::Invalid("enabled must be true or false".to_string()))?;
    let cwd = bound.cwd;
    let all = load(cwd, agent_dir, Some(without_skill_patterns(cwd, agent_dir)))?;
    if !all.skills.iter().any(|skill| skill.file_path == file) {
        return Err(Refusal::Invalid(format!("no skill at {}", shown(&file))));
    }
    let target = skill_target(&file).display().to_string();
    let (off, on) = (format!("-{target}"), format!("+{target}"));
    let mut entries: Vec<String> = global_skill_entries(cwd, agent_dir)
        .into_iter()
        .filter(|entry| *entry != off && *entry != on)
        .collect();
    if !enabled {
        entries.push(off);
    }
    write_skill_entries(cwd, agent_dir, entries.clone());
    if enabled {
        let loaded = load(cwd, agent_dir, None)?;
        if !loaded.skills.iter().any(|skill| skill.file_path == file) {
            entries.push(on);
            write_skill_entries(cwd, agent_dir, entries);
        }
    }
    skills_view(bound, agent_dir).await
}

/// A name of lower-case letters and digits in words joined by single hyphens, 64 at most.
fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Writes `<agent dir>/skills/<name>/SKILL.md` with the screen's name and description.
async fn create_skill(bound: &Bound<'_>, agent_dir: &Path, params: &Value) -> Answer {
    let name = string_param(params, "name")?;
    let description = string_param(params, "description")?.trim().to_string();
    if !valid_skill_name(&name) {
        return Err(Refusal::Invalid(
            "the name takes lower-case letters, digits and single hyphens, 64 at most".to_string(),
        ));
    }
    if description.is_empty() || description.chars().count() > 1024 || description.contains('\n') {
        return Err(Refusal::Invalid(
            "the description takes one line of 1 to 1024 characters".to_string(),
        ));
    }
    let all = load(
        bound.cwd,
        agent_dir,
        Some(without_skill_patterns(bound.cwd, agent_dir)),
    )?;
    if all.skills.iter().any(|skill| skill.name == name) {
        return Err(Refusal::Invalid(format!("a skill named {name} exists")));
    }
    let folder = agent_dir.join("skills").join(&name);
    let file = folder.join("SKILL.md");
    std::fs::create_dir_all(&folder)
        .and_then(|()| {
            std::fs::write(
                &file,
                format!("---\nname: {name}\ndescription: {description}\n---\n\n{NEW_SKILL_BODY}\n"),
            )
        })
        .map_err(|error| Refusal::Failed(format!("the skill could not be written: {error}")))?;
    let mut view = skills_view(bound, agent_dir).await?;
    view["created"] = json!(file.display().to_string());
    Ok(view)
}

/// Adds or removes a folder of skills in the global `skills` setting.
async fn skill_folder(bound: &Bound<'_>, agent_dir: &Path, params: &Value, add: bool) -> Answer {
    let path = string_param(params, "path")?;
    let mut entries = global_skill_entries(bound.cwd, agent_dir);
    if add {
        if !(Path::new(&path).is_absolute() || path.starts_with('~')) {
            return Err(Refusal::Invalid(
                "the folder takes an absolute path or one starting with ~".to_string(),
            ));
        }
        if is_override(&path) || path.contains(['*', '?', '[']) {
            return Err(Refusal::Invalid(
                "the folder takes a path, not a pattern".to_string(),
            ));
        }
        if !expanded(&path).is_dir() {
            return Err(Refusal::Invalid(format!("{path} is not a folder")));
        }
        if !entries.contains(&path) {
            entries.push(path);
        }
    } else {
        entries.retain(|entry| *entry != path);
    }
    write_skill_entries(bound.cwd, agent_dir, entries);
    skills_view(bound, agent_dir).await
}

async fn skills(bound: &Bound<'_>, agent_dir: &Path, method: &str, params: &Value) -> Answer {
    match method {
        SKILLS_SET => set_skill(bound, agent_dir, params).await,
        SKILLS_CREATE => create_skill(bound, agent_dir, params).await,
        SKILLS_ADD_FOLDER => skill_folder(bound, agent_dir, params, true).await,
        SKILLS_REMOVE_FOLDER => skill_folder(bound, agent_dir, params, false).await,
        _ => skills_view(bound, agent_dir).await,
    }
}

// MCP

/// One server of the MCP screen from a connection of the worker's roster
/// (`pa_core::mcp::McpConnectionEntry`).
fn server_row(entry: &Value, configured: &Map<String, Value>) -> Value {
    let id = entry
        .get("server")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let flag = |name: &str| entry.get(name).and_then(Value::as_bool).unwrap_or(false);
    let connected = flag("connected");
    let oauth = flag("usesOauth");
    let disabled = configured
        .get(id)
        .and_then(|config| config.get("enabled"))
        .and_then(Value::as_bool)
        == Some(false);
    let status = if connected {
        "connected"
    } else if disabled {
        "disabled"
    } else {
        "not_connected"
    };
    let credential = oauth || entry.get("authKind").and_then(Value::as_str) == Some("api key");
    json!({
        "id": id,
        "label": entry.get("label").cloned().unwrap_or_else(|| json!(id)),
        "source": if flag("userDeclared") { "user" } else { "catalog" },
        "status": status,
        "signIn": oauth && !connected,
        "signedIn": connected && credential,
    })
}

/// The MCP screen: the worker's roster, the local catalog and the `mcpServers` setting.
async fn mcp_view(bound: &Bound<'_>, agent_dir: &Path) -> Answer {
    let response = bound
        .link
        .request(DaemonCommand::GetMcpConnections {
            id: None,
            active_session_id: bound.daemon_session_id.to_string(),
            rest: Map::new(),
        })
        .await
        .map_err(|error| Refusal::Failed(error.to_string()))?;
    if !response.success {
        return Err(Refusal::Failed(
            response
                .error
                .unwrap_or_else(|| "get_mcp_connections failed".to_string()),
        ));
    }
    let configured = SettingsManager::create(bound.cwd, agent_dir)
        .global_settings()
        .mcp_servers
        .clone()
        .unwrap_or_default();
    let servers: Vec<Value> = response
        .data
        .as_ref()
        .and_then(|data| data.get("connections"))
        .and_then(Value::as_array)
        .map(|connections| {
            connections
                .iter()
                .map(|entry| server_row(entry, &configured))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "servers": servers }))
}

async fn mcp(bound: &Bound<'_>, agent_dir: &Path, method: &str, params: &Value) -> Answer {
    let command = match method {
        MCP_SET_TOKEN => Some(DaemonCommand::SetMcpStaticToken {
            id: None,
            active_session_id: bound.daemon_session_id.to_string(),
            server: string_param(params, "id")?,
            token: string_param(params, "token")?,
            rest: Map::new(),
        }),
        MCP_LOGOUT => Some(DaemonCommand::RemoveMcpConnection {
            id: None,
            active_session_id: bound.daemon_session_id.to_string(),
            server: string_param(params, "id")?,
            rest: Map::new(),
        }),
        _ => None,
    };
    if let Some(command) = command {
        let response = bound
            .link
            .request(command)
            .await
            .map_err(|error| Refusal::Failed(error.to_string()))?;
        if !response.success {
            return Err(Refusal::Invalid(
                response.error.unwrap_or_else(|| format!("{method} failed")),
            ));
        }
    }
    mcp_view(bound, agent_dir).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_names_and_overrides() {
        assert!(valid_skill_name("deploy"));
        assert!(valid_skill_name("ya31-demo-2"));
        for bad in ["", "Deploy", "a--b", "-a", "a-", "a_b", &"a".repeat(65)] {
            assert!(!valid_skill_name(bad), "{bad}");
        }
        assert!(is_override("-/x") && is_override("+/x") && is_override("!x"));
        assert!(!is_override("/x") && !is_override("~/x"));
        assert_eq!(
            skill_target(Path::new("/a/b/SKILL.md")),
            PathBuf::from("/a/b")
        );
        assert_eq!(skill_target(Path::new("/a/b.md")), PathBuf::from("/a/b.md"));
    }

    #[test]
    fn a_roster_entry_gives_its_server_row() {
        let configured: Map<String, Value> = serde_json::from_value(
            json!({ "off": { "type": "stdio", "command": "x", "enabled": false } }),
        )
        .unwrap();
        let row = server_row(
            &json!({ "server": "linear", "label": "Linear", "connected": false, "usesOauth": true,
                     "authKind": "subscription", "transport": "http", "userDeclared": false,
                     "generic": false }),
            &configured,
        );
        assert_eq!(
            row,
            json!({ "id": "linear", "label": "Linear", "source": "catalog",
                    "status": "not_connected", "signIn": true, "signedIn": false })
        );
        let row = server_row(
            &json!({ "server": "off", "label": "off", "connected": false, "usesOauth": false,
                     "authKind": "stdio", "transport": "stdio", "userDeclared": true,
                     "generic": false }),
            &configured,
        );
        assert_eq!(row["status"], "disabled");
        assert_eq!(row["source"], "user");
    }
}
