//! yusAgent (YA-10, ticket 31, decision 5): `_yus/settings/get` and `_yus/settings/set`, the
//! global settings the extension's General tab edits, plus `mcpServers` for its MCP tab.
//! Only keys that apply at the next message without a cache miss are on the list: `retry.*` and
//! `auxiliaryModel` are read at each use, `compaction.enabled` at each compaction decision
//! (`pa_core::session_engine::ya_resource_notice`), `mcpServers` is announced at the next prompt.
//! Refused with their exact texts: `permissions` (D1, changed per conversation by the
//! selector), the keys frozen for a session (`yaRequest`, `editTool`, `shellTool`), any other
//! key. A `null` value removes the key (its default applies again). Writes go through the
//! settings storage's lock; the values are never logged (rule 9: `mcpServers` may hold a
//! header or an environment variable). As Unharnessed's `settings.rs` (`f8e4658dd`), with our
//! keys. Listed in `docs/fork-changes.md`.

use std::path::Path;

use pa_core::settings::{FileSettingsStorage, SettingsScope, SettingsStorage};
use serde_json::{json, Map, Value};

use super::{string_param, Answer, Refusal};

const GET: &str = "_yus/settings/get";
const SET: &str = "_yus/settings/set";

/// The refusal of `permissions` (D1).
const PERMISSIONS_REFUSAL: &str = "permissions is not set here: change it for this conversation with the permissions menu, or in ~/.yusagent/settings.json for every new session.";

/// Keys frozen for a session at its creation (`yaRequest`, the native tools).
const FROZEN: &[&str] = &[
    "reasoningSummary",
    "bashWaitSeconds",
    "promptVersion",
    "codexSessionIdHeader",
    "existingTestsRule",
    "editTool",
    "shellTool",
];

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    /// A whole number from 0 to the bound.
    Count(u64),
    /// A `provider/model` reference.
    Model,
    McpServers,
}

const HOUR_MS: u64 = 3_600_000;

/// The keys of the screen, as dotted paths of the global settings file.
const KEYS: &[(&str, Kind)] = &[
    ("compaction.enabled", Kind::Bool),
    ("auxiliaryModel", Kind::Model),
    ("retry.enabled", Kind::Bool),
    ("retry.maxRetries", Kind::Count(100)),
    ("retry.baseDelayMs", Kind::Count(HOUR_MS)),
    ("retry.provider.maxRetryDelayMs", Kind::Count(24 * HOUR_MS)),
    ("retry.failover.enabled", Kind::Bool),
    ("retry.failover.maxRetries", Kind::Count(100)),
    ("retry.failover.baseDelayMs", Kind::Count(HOUR_MS)),
    ("retry.failover.maxDelayMs", Kind::Count(HOUR_MS)),
    ("retry.provider.waitForUsage.pauseUntilReset", Kind::Bool),
    (
        "retry.provider.waitForUsage.maxPauseMs",
        Kind::Count(7 * 24 * HOUR_MS),
    ),
    ("retry.provider.waitForUsage.maxParks", Kind::Count(100)),
    ("mcpServers", Kind::McpServers),
];

pub(super) fn serves(method: &str) -> bool {
    method == GET || method == SET
}

fn pointer(key: &str) -> String {
    format!("/{}", key.replace('.', "/"))
}

/// The global settings document; `{}` when absent.
fn global_document(cwd: &Path, agent_dir: &Path) -> Result<Value, Refusal> {
    let text = FileSettingsStorage::new(cwd, agent_dir)
        .read(SettingsScope::Global)
        .map_err(|error| Refusal::Failed(format!("the settings could not be read: {error}")))?;
    let Some(text) = text else {
        return Ok(json!({}));
    };
    serde_json::from_str(&text).map_err(|error| {
        Refusal::Failed(format!(
            "the global settings file is not valid JSON: {error}"
        ))
    })
}

/// The screen: each key's value in the global file (`null` when unset), the defaults the
/// screen can show, and the keys waiting for a restart (none).
fn view(cwd: &Path, agent_dir: &Path) -> Result<Value, Refusal> {
    let document = global_document(cwd, agent_dir)?;
    let mut values = Map::new();
    for (key, _) in KEYS {
        let value = document
            .pointer(&pointer(key))
            .cloned()
            .unwrap_or(Value::Null);
        values.insert((*key).to_string(), value);
    }
    let mut defaults = Map::new();
    for (key, _) in KEYS {
        defaults.insert((*key).to_string(), Value::Null);
    }
    // The one default the settings manager states (`get_compaction_enabled`); the others are
    // not given here (`null`).
    defaults.insert("compaction.enabled".to_string(), json!(true));
    Ok(json!({ "values": values, "defaults": defaults, "restart": [] }))
}

fn valid_server_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_model_reference(reference: &str) -> bool {
    reference
        .split_once('/')
        .is_some_and(|(provider, model)| !provider.is_empty() && !model.is_empty())
}

fn check_servers(key: &str, value: &Value) -> Result<(), String> {
    let servers = value
        .as_object()
        .ok_or_else(|| format!("{key} takes an object of servers"))?;
    for (name, config) in servers {
        if !valid_server_name(name) {
            return Err(format!(
                "{key}: {name} is not a server name (letters, digits, _ and -, 64 at most)"
            ));
        }
        serde_json::from_value::<pa_core::mcp::McpServerConfig>(config.clone())
            .map_err(|error| format!("{key}: {name}: {error}"))?;
    }
    Ok(())
}

fn check(key: &str, kind: Kind, value: &Value) -> Result<(), String> {
    let (ok, expected) = match kind {
        Kind::Bool => (value.is_boolean(), "true or false".to_string()),
        Kind::Count(max) => (
            value.as_u64().is_some_and(|count| count <= max),
            format!("a whole number from 0 to {max}"),
        ),
        Kind::Model => (
            value.as_str().is_some_and(is_model_reference),
            "a provider/model reference".to_string(),
        ),
        Kind::McpServers => return check_servers(key, value),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{key} takes {expected}"))
    }
}

/// Sets `key` (a dotted path) to `value`, or removes it and the objects it leaves empty.
fn set_dotted(document: &mut Value, key: &str, value: Option<Value>) {
    let parts: Vec<&str> = key.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        return;
    };
    if let Some(value) = value {
        let mut node = document;
        for part in parents {
            let Some(map) = node.as_object_mut() else {
                return;
            };
            let child = map.entry((*part).to_string()).or_insert_with(|| json!({}));
            if !child.is_object() {
                *child = json!({});
            }
            node = child;
        }
        if let Some(map) = node.as_object_mut() {
            map.insert((*last).to_string(), value);
        }
    } else {
        let parent = if parents.is_empty() {
            Some(&mut *document)
        } else {
            document.pointer_mut(&pointer(&parents.join(".")))
        };
        if let Some(map) = parent.and_then(Value::as_object_mut) {
            map.remove(*last);
        }
        for depth in (1..=parents.len()).rev() {
            let path = parents[..depth].join(".");
            let empty = document
                .pointer(&pointer(&path))
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty);
            if !empty {
                break;
            }
            let parent = if depth == 1 {
                Some(&mut *document)
            } else {
                document.pointer_mut(&pointer(&parents[..depth - 1].join(".")))
            };
            if let Some(map) = parent.and_then(Value::as_object_mut) {
                map.remove(parents[depth - 1]);
            }
        }
    }
}

fn write(cwd: &Path, agent_dir: &Path, key: &str, value: Option<&Value>) -> Result<(), Refusal> {
    let mut failure: Option<String> = None;
    let written =
        FileSettingsStorage::new(cwd, agent_dir).with_lock(SettingsScope::Global, &mut |current| {
            let mut document = match current.as_deref().map(serde_json::from_str::<Value>) {
                None => json!({}),
                Some(Ok(document)) if document.is_object() => document,
                Some(_) => {
                    failure = Some("the global settings file is not a JSON object".to_string());
                    return None;
                }
            };
            set_dotted(&mut document, key, value.cloned());
            Some(format!(
                "{}\n",
                serde_json::to_string_pretty(&document).unwrap_or_default()
            ))
        });
    if let Err(error) = written {
        return Err(Refusal::Failed(format!(
            "the settings could not be written: {error}"
        )));
    }
    failure.map_or(Ok(()), |failure| Err(Refusal::Failed(failure)))
}

pub(super) fn answer(cwd: &Path, agent_dir: &Path, method: &str, params: &Value) -> Answer {
    if method == GET {
        return view(cwd, agent_dir);
    }
    let key = string_param(params, "key")?;
    if key == "permissions" {
        return Err(Refusal::Invalid(PERMISSIONS_REFUSAL.to_string()));
    }
    if FROZEN.contains(&key.as_str()) {
        return Err(Refusal::Invalid(format!(
            "{key} is fixed for each conversation when it starts: it is not set here."
        )));
    }
    let Some((_, kind)) = KEYS.iter().find(|(name, _)| *name == key) else {
        return Err(Refusal::Invalid(format!("unknown setting: {key}")));
    };
    let value = params.get("value").filter(|value| !value.is_null());
    if let Some(value) = value {
        check(&key, *kind, value).map_err(Refusal::Invalid)?;
    }
    write(cwd, agent_dir, &key, value)?;
    view(cwd, agent_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_keys_set_and_remove_with_their_empty_parents() {
        let mut document = json!({ "theme": "dark" });
        set_dotted(
            &mut document,
            "retry.provider.maxRetryDelayMs",
            Some(json!(5)),
        );
        set_dotted(&mut document, "retry.enabled", Some(json!(false)));
        assert_eq!(
            document,
            json!({ "theme": "dark", "retry": { "enabled": false, "provider": { "maxRetryDelayMs": 5 } } })
        );
        set_dotted(&mut document, "retry.provider.maxRetryDelayMs", None);
        assert_eq!(
            document,
            json!({ "theme": "dark", "retry": { "enabled": false } })
        );
        set_dotted(&mut document, "retry.enabled", None);
        assert_eq!(document, json!({ "theme": "dark" }));
        set_dotted(&mut document, "auxiliaryModel", Some(json!("a/b")));
        set_dotted(&mut document, "auxiliaryModel", None);
        assert_eq!(document, json!({ "theme": "dark" }));
    }

    /// Ticket 31 (X3, C1): the General tab shows what applies when a key is unset, so the bridge
    /// gives the core's defaults (`SettingsManager` getters on empty settings), not `null`.
    #[test]
    fn the_view_gives_the_core_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (cwd, agent_dir) = (dir.path().join("project"), dir.path().join("agent"));
        std::fs::create_dir_all(&cwd).expect("project dir");
        std::fs::create_dir_all(&agent_dir).expect("agent dir");
        let Ok(view) = view(&cwd, &agent_dir) else {
            panic!("the view of empty settings");
        };
        let core = pa_core::settings::SettingsManager::from_storage(std::sync::Arc::new(
            pa_core::settings::InMemorySettingsStorage::default(),
        ));
        let retry = core.get_provider_retry_policy();
        let failover = core.get_provider_failover_policy();
        let park = core.get_provider_park_policy();
        assert_eq!(
            view["defaults"],
            json!({
                "compaction.enabled": core.get_compaction_enabled(),
                "auxiliaryModel": null,
                "retry.enabled": retry.enabled,
                "retry.maxRetries": retry.max_retries,
                "retry.baseDelayMs": retry.base_delay_ms,
                "retry.provider.maxRetryDelayMs": retry.max_retry_delay_ms,
                "retry.failover.enabled": failover.enabled,
                "retry.failover.maxRetries": failover.max_retries,
                "retry.failover.baseDelayMs": failover.base_delay_ms,
                "retry.failover.maxDelayMs": failover.max_delay_ms,
                "retry.provider.waitForUsage.pauseUntilReset": park.pause_until_reset,
                "retry.provider.waitForUsage.maxPauseMs": park.max_pause_ms,
                "retry.provider.waitForUsage.maxParks": park.max_parks,
                "mcpServers": null,
            })
        );
        assert_eq!(
            view["defaults"]["retry.enabled"],
            json!(true),
            "retries are on by default"
        );
    }

    #[test]
    fn values_are_checked() {
        assert!(check("retry.enabled", Kind::Bool, &json!(true)).is_ok());
        assert_eq!(
            check("retry.enabled", Kind::Bool, &json!(1)).unwrap_err(),
            "retry.enabled takes true or false"
        );
        assert!(check("retry.maxRetries", Kind::Count(100), &json!(100)).is_ok());
        assert_eq!(
            check("retry.maxRetries", Kind::Count(100), &json!(101)).unwrap_err(),
            "retry.maxRetries takes a whole number from 0 to 100"
        );
        assert!(check("auxiliaryModel", Kind::Model, &json!("openai/gpt")).is_ok());
        assert!(check("auxiliaryModel", Kind::Model, &json!("gpt")).is_err());
        assert!(check(
            "mcpServers",
            Kind::McpServers,
            &json!({ "a-1": { "type": "stdio", "command": "x" } })
        )
        .is_ok());
        assert!(check("mcpServers", Kind::McpServers, &json!({ "-a": {} })).is_err());
    }
}
