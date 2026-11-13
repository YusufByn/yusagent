//! yusAgent (YA-03, ticket 09): the native tools a session gets, by model family.
//!
//! A family is read from the provider, the API and the model id together, never from the API
//! alone: a custom provider can serve any model on `openai-responses`, and an Azure deployment
//! has an arbitrary name. A model is in the Codex family only when it is listed in
//! [`CODEX_MODELS`] and served by one of the [`CODEX_ROUTES`]; anything else is
//! [`ToolFamily::Other`].
//!
//! Native tools per family (no format is wired outside this table):
//! - Codex: `apply_patch` (freeform, `tools::ya_apply_patch`) and `exec_command` / `write_stdin`
//!   (`tools::ya_exec`);
//! - Other: none; edits go through `edit()` and commands through `bash()` in the kernel.
//!
//! Settings, frozen for the session at creation (written to the session header, field
//! [`FROZEN_TOOLS_HEADER_KEY`], and read back on every resume; settings changed in between give
//! a warning, not other tools):
//! - `editTool` unset: `apply_patch` on Codex, the kernel's `edit()` elsewhere; `"kernel"`: the
//!   kernel everywhere; `"native"`: `apply_patch`, refused outside Codex.
//! - `shellTool` unset or `"kernel"`: `bash()` in the kernel; `"native"`: `exec_command` /
//!   `write_stdin` with `bash` removed from the kernel namespace, refused outside Codex.
//!
//! A model change to another family is refused for the rest of the session.

use serde::{Deserialize, Serialize};

use super::AgentSession;
use crate::session::manager::SessionManager;

/// The models of openai/codex `codex-rs/models-manager/models.json` at
/// `69231e119bfca0b14453515e97d99688ce1c47c9`, in file order. Every one has
/// `apply_patch_tool_type: freeform` and `shell_type: shell_command` there.
pub const CODEX_MODELS: &[&str] = &[
    "gpt-6-astra",
    "gpt-6.1-sol",
    "gpt-6.1-terra",
    "gpt-6-sol",
    "gpt-6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-daybreak-blue-latest",
    "gpt-daybreak-red-latest",
    "gpt-5.5",
    "codex-auto-review",
];

/// `(provider, api)` pairs that serve the Codex family: `OpenAI` itself and Azure `OpenAI`, on
/// the Responses API (the only API that carries freeform tools, `pa_ai::ya_freeform`).
pub const CODEX_ROUTES: &[(&str, &str)] = &[
    ("openai", "openai-responses"),
    ("openai-codex", "openai-codex-responses"),
    ("azure-openai-responses", "azure-openai-responses"),
];

/// The model family the native tools are chosen for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFamily {
    Codex,
    Other,
}

impl ToolFamily {
    fn label(self) -> &'static str {
        match self {
            ToolFamily::Codex => "Codex",
            ToolFamily::Other => "non-Codex",
        }
    }
}

/// The family of a model, from its provider, API and id.
#[must_use]
pub fn tool_family(provider: &str, api: &str, model_id: &str) -> ToolFamily {
    let routed = CODEX_ROUTES
        .iter()
        .any(|(route_provider, route_api)| *route_provider == provider && *route_api == api);
    if routed && CODEX_MODELS.contains(&model_id) {
        ToolFamily::Codex
    } else {
        ToolFamily::Other
    }
}

/// `editTool` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditToolSetting {
    Native,
    Kernel,
}

/// `shellTool` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellToolSetting {
    Native,
    Kernel,
}

/// The native tools of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolChoice {
    /// The `apply_patch` tool is registered (the prompt documents it).
    pub apply_patch: bool,
    /// `exec_command` and `write_stdin` are registered and `bash` leaves the kernel namespace.
    pub native_shell: bool,
}

/// The native tools for `family` under the session's settings.
///
/// # Errors
///
/// Refuses `editTool: "native"` or `shellTool: "native"` outside the Codex family, with a
/// message naming the model.
pub fn resolve_tool_choice(
    family: ToolFamily,
    edit_tool: Option<EditToolSetting>,
    shell_tool: Option<ShellToolSetting>,
    model_label: &str,
) -> anyhow::Result<ToolChoice> {
    let codex = family == ToolFamily::Codex;
    let refuse = |setting: &str| {
        anyhow::anyhow!(
            "{setting} \"native\" is only available for the Codex model family; {model_label} is not one. Remove the setting or set it to \"kernel\"."
        )
    };
    let apply_patch = match edit_tool {
        None => codex,
        Some(EditToolSetting::Kernel) => false,
        Some(EditToolSetting::Native) if codex => true,
        Some(EditToolSetting::Native) => return Err(refuse("editTool")),
    };
    let native_shell = match shell_tool {
        None | Some(ShellToolSetting::Kernel) => false,
        Some(ShellToolSetting::Native) if codex => true,
        Some(ShellToolSetting::Native) => return Err(refuse("shellTool")),
    };
    Ok(ToolChoice {
        apply_patch,
        native_shell,
    })
}

/// The session header field that freezes `editTool` and `shellTool` for the session
/// (`SessionManager::ya_set_header_field`).
pub const FROZEN_TOOLS_HEADER_KEY: &str = "yaTools";

/// The effective `editTool` and `shellTool` of a session, written to its header at creation and
/// read back on every resume: unset settings are stored as the value they resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenTools {
    pub edit_tool: EditToolSetting,
    pub shell_tool: ShellToolSetting,
}

impl FrozenTools {
    /// The settings that resolve to `choice` in its family.
    #[must_use]
    pub fn of(choice: ToolChoice) -> Self {
        FrozenTools {
            edit_tool: if choice.apply_patch {
                EditToolSetting::Native
            } else {
                EditToolSetting::Kernel
            },
            shell_tool: if choice.native_shell {
                ShellToolSetting::Native
            } else {
                ShellToolSetting::Kernel
            },
        }
    }

    /// The frozen tools of a session header field; `None` when absent or unreadable.
    #[must_use]
    pub fn from_header_value(value: Option<&serde_json::Value>) -> Option<Self> {
        value.and_then(|value| serde_json::from_value(value.clone()).ok())
    }
}

/// The tools of a session being built, and what the engine does about its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTools {
    pub choice: ToolChoice,
    /// To write to the header: the session had no frozen tools (a new session, or one created
    /// before the field existed).
    pub to_write: Option<FrozenTools>,
    /// Shown at resume when the current settings would give other tools.
    pub warning: Option<String>,
}

fn setting_label<T: Serialize>(setting: Option<T>) -> String {
    setting
        .and_then(|value| serde_json::to_value(value).ok())
        .map_or_else(|| "unset".to_string(), |value| format!("{value}"))
}

/// The tools of a session: the frozen ones of its header when it has them (a resume), else the
/// current settings (frozen from now on).
///
/// # Errors
///
/// Refuses a native setting outside the Codex family: a current one for a new session, a frozen
/// one when a session created on a Codex model is resumed on another family.
pub fn session_tools(
    family: ToolFamily,
    frozen: Option<FrozenTools>,
    edit_tool: Option<EditToolSetting>,
    shell_tool: Option<ShellToolSetting>,
    model_label: &str,
) -> anyhow::Result<SessionTools> {
    let current = resolve_tool_choice(family, edit_tool, shell_tool, model_label);
    let Some(frozen) = frozen else {
        let choice = current?;
        return Ok(SessionTools {
            choice,
            to_write: Some(FrozenTools::of(choice)),
            warning: None,
        });
    };
    let choice = resolve_tool_choice(
        family,
        Some(frozen.edit_tool),
        Some(frozen.shell_tool),
        model_label,
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "This session was created with editTool {} and shellTool {}, which need the Codex model family; {model_label} is not one. Resume it on a Codex model or start a new session.",
            setting_label(Some(frozen.edit_tool)),
            setting_label(Some(frozen.shell_tool)),
        )
    })?;
    let warning = (current.ok() != Some(choice)).then(|| {
        format!(
            "Warning: this session keeps the tools it was created with (editTool {}, shellTool {}); the current settings (editTool {}, shellTool {}) apply to new sessions only.",
            setting_label(Some(frozen.edit_tool)),
            setting_label(Some(frozen.shell_tool)),
            setting_label(edit_tool),
            setting_label(shell_tool),
        )
    });
    Ok(SessionTools {
        choice,
        to_write: None,
        warning,
    })
}

/// The tools of the session `manager` holds: the frozen ones of its header, or the current
/// settings written to a header that has none. Prints the resume warning to stderr.
///
/// # Errors
///
/// As [`session_tools`].
pub fn freeze_session_tools(
    manager: &mut SessionManager,
    family: ToolFamily,
    edit_tool: Option<EditToolSetting>,
    shell_tool: Option<ShellToolSetting>,
    model_label: &str,
) -> anyhow::Result<ToolChoice> {
    let frozen = FrozenTools::from_header_value(manager.ya_header_field(FROZEN_TOOLS_HEADER_KEY));
    let tools = session_tools(family, frozen, edit_tool, shell_tool, model_label)?;
    if let Some(to_write) = tools.to_write {
        manager.ya_set_header_field(FROZEN_TOOLS_HEADER_KEY, serde_json::to_value(to_write)?);
    }
    if let Some(warning) = &tools.warning {
        eprintln!("{warning}");
    }
    Ok(tools.choice)
}

/// Whether a session frozen on `frozen` may switch to the model `provider/model_id` on `api`.
///
/// # Errors
///
/// Refuses a model of another family, with a message naming both families.
pub fn check_model_change(
    frozen: ToolFamily,
    provider: &str,
    api: &str,
    model_id: &str,
) -> anyhow::Result<()> {
    let next = tool_family(provider, api, model_id);
    if next == frozen {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "Model change refused: this session's tools were set for the {} model family, and {provider}/{model_id} is in the {} family. Start a new session to use it.",
        frozen.label(),
        next.label()
    ))
}

impl AgentSession {
    /// Freeze the session's model family (the engine calls it once, at creation).
    pub fn ya_freeze_tool_family(&self, family: ToolFamily) {
        let _ = self.ya_tool_family.set(family);
    }

    /// The model-change guard of `set_model` and `set_model_and_thinking_level`: a no-op for a
    /// session built without the engine (no frozen family).
    pub(crate) fn ya_check_model_change(&self, model: &pa_types::ai::Model) -> anyhow::Result<()> {
        match self.ya_tool_family.get() {
            Some(frozen) => check_model_change(*frozen, &model.provider, &model.api, &model.id),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_models_on_openai_routes_are_codex() {
        for (provider, api) in CODEX_ROUTES {
            assert_eq!(
                tool_family(provider, api, "gpt-6.1-terra"),
                ToolFamily::Codex
            );
        }
        assert_eq!(CODEX_MODELS.len(), 12);
    }

    /// A model unknown to Codex's `models.json`, served on `openai-responses` (a custom
    /// provider serving GLM, say), is not Codex.
    #[test]
    fn unknown_model_on_openai_responses_is_other() {
        assert_eq!(
            tool_family("openai", "openai-responses", "glm-5.3"),
            ToolFamily::Other
        );
        assert_eq!(
            tool_family("my-proxy", "openai-responses", "glm-5.3"),
            ToolFamily::Other
        );
    }

    #[test]
    fn known_model_on_another_provider_or_api_is_other() {
        // A custom provider on the Responses API, even with a Codex model id.
        assert_eq!(
            tool_family("my-proxy", "openai-responses", "gpt-6.1-terra"),
            ToolFamily::Other
        );
        // OpenAI on Chat Completions: no freeform tools there.
        assert_eq!(
            tool_family("openai", "openai-completions", "gpt-6.1-terra"),
            ToolFamily::Other
        );
        // An Azure deployment under an arbitrary name.
        assert_eq!(
            tool_family(
                "azure-openai-responses",
                "azure-openai-responses",
                "prod-coder"
            ),
            ToolFamily::Other
        );
        assert_eq!(
            tool_family("anthropic", "anthropic-messages", "claude-opus-5-5"),
            ToolFamily::Other
        );
    }

    #[test]
    fn unset_edit_tool_is_native_on_codex_and_kernel_elsewhere() {
        let codex = resolve_tool_choice(ToolFamily::Codex, None, None, "m").unwrap();
        assert_eq!(
            codex,
            ToolChoice {
                apply_patch: true,
                native_shell: false
            }
        );
        let other = resolve_tool_choice(ToolFamily::Other, None, None, "m").unwrap();
        assert_eq!(other, ToolChoice::default());
    }

    #[test]
    fn explicit_settings_on_codex() {
        let kernel = resolve_tool_choice(
            ToolFamily::Codex,
            Some(EditToolSetting::Kernel),
            Some(ShellToolSetting::Kernel),
            "m",
        )
        .unwrap();
        assert_eq!(kernel, ToolChoice::default());
        let native = resolve_tool_choice(
            ToolFamily::Codex,
            Some(EditToolSetting::Native),
            Some(ShellToolSetting::Native),
            "m",
        )
        .unwrap();
        assert!(native.apply_patch && native.native_shell);
    }

    #[test]
    fn explicit_native_outside_codex_is_refused() {
        let edit = resolve_tool_choice(
            ToolFamily::Other,
            Some(EditToolSetting::Native),
            None,
            "anthropic/claude-opus-5-5",
        )
        .unwrap_err()
        .to_string();
        assert!(edit.contains("editTool \"native\""), "{edit}");
        assert!(edit.contains("anthropic/claude-opus-5-5"), "{edit}");
        let shell = resolve_tool_choice(
            ToolFamily::Other,
            None,
            Some(ShellToolSetting::Native),
            "x/y",
        )
        .unwrap_err()
        .to_string();
        assert!(shell.contains("shellTool \"native\""), "{shell}");
        // Kernel settings stay valid everywhere.
        assert!(resolve_tool_choice(
            ToolFamily::Other,
            Some(EditToolSetting::Kernel),
            Some(ShellToolSetting::Kernel),
            "x/y"
        )
        .is_ok());
    }

    #[test]
    fn model_change_across_families_is_refused() {
        assert!(check_model_change(
            ToolFamily::Codex,
            "openai-codex",
            "openai-codex-responses",
            "gpt-6.1-sol"
        )
        .is_ok());
        assert!(check_model_change(
            ToolFamily::Other,
            "anthropic",
            "anthropic-messages",
            "claude-opus-5-5"
        )
        .is_ok());
        let error = check_model_change(
            ToolFamily::Codex,
            "anthropic",
            "anthropic-messages",
            "claude-opus-5-5",
        )
        .unwrap_err()
        .to_string();
        assert!(error.starts_with("Model change refused"), "{error}");
        assert!(error.contains("anthropic/claude-opus-5-5"), "{error}");
        assert!(check_model_change(
            ToolFamily::Other,
            "openai-codex",
            "openai-codex-responses",
            "gpt-6.1-terra"
        )
        .is_err());
    }

    const CODEX_DEFAULT: ToolChoice = ToolChoice {
        apply_patch: true,
        native_shell: false,
    };

    #[test]
    fn new_session_freezes_its_effective_settings() {
        let tools = session_tools(ToolFamily::Codex, None, None, None, "m").unwrap();
        assert_eq!(tools.choice, CODEX_DEFAULT);
        assert_eq!(
            tools.to_write,
            Some(FrozenTools {
                edit_tool: EditToolSetting::Native,
                shell_tool: ShellToolSetting::Kernel,
            })
        );
        assert_eq!(tools.warning, None);
        let value = serde_json::to_value(tools.to_write.unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"editTool": "native", "shellTool": "kernel"})
        );
        assert_eq!(FrozenTools::from_header_value(Some(&value)), tools.to_write);
        let other = session_tools(ToolFamily::Other, None, None, None, "m").unwrap();
        assert_eq!(other.to_write, Some(FrozenTools::of(ToolChoice::default())));
        assert!(session_tools(
            ToolFamily::Other,
            None,
            Some(EditToolSetting::Native),
            None,
            "m"
        )
        .is_err());
    }

    #[test]
    fn resume_keeps_the_frozen_tools_and_warns_when_settings_differ() {
        let frozen = FrozenTools::of(CODEX_DEFAULT);
        // Unchanged settings: same tools, nothing to write, no warning.
        let same = session_tools(ToolFamily::Codex, Some(frozen), None, None, "m").unwrap();
        assert_eq!(same.choice, CODEX_DEFAULT);
        assert_eq!((same.to_write, same.warning), (None, None));
        // Other settings: the frozen tools win, with a warning naming both.
        let changed = session_tools(
            ToolFamily::Codex,
            Some(frozen),
            Some(EditToolSetting::Kernel),
            Some(ShellToolSetting::Native),
            "m",
        )
        .unwrap();
        assert_eq!(changed.choice, CODEX_DEFAULT);
        assert_eq!(changed.to_write, None);
        let warning = changed.warning.expect("a warning");
        assert!(
            warning.contains("editTool \"native\", shellTool \"kernel\""),
            "{warning}"
        );
        assert!(
            warning.contains("editTool \"kernel\", shellTool \"native\""),
            "{warning}"
        );
        // Settings invalid for the family also warn instead of failing the resume.
        let frozen_kernel = FrozenTools::of(ToolChoice::default());
        let invalid = session_tools(
            ToolFamily::Other,
            Some(frozen_kernel),
            Some(EditToolSetting::Native),
            None,
            "m",
        )
        .unwrap();
        assert_eq!(invalid.choice, ToolChoice::default());
        assert!(invalid
            .warning
            .unwrap()
            .contains("editTool \"native\", shellTool unset"));
    }

    #[test]
    fn native_frozen_tools_refuse_a_resume_on_another_family() {
        let error = session_tools(
            ToolFamily::Other,
            Some(FrozenTools::of(CODEX_DEFAULT)),
            None,
            None,
            "anthropic/claude-opus-5-5",
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.starts_with("This session was created with"),
            "{error}"
        );
        assert!(error.contains("anthropic/claude-opus-5-5"), "{error}");
    }

    #[test]
    fn unreadable_header_value_is_absent() {
        assert_eq!(FrozenTools::from_header_value(None), None);
        let bad = serde_json::json!({"editTool": "bash"});
        assert_eq!(FrozenTools::from_header_value(Some(&bad)), None);
    }

    #[test]
    fn settings_parse_lowercase() {
        let edit: EditToolSetting = serde_json::from_str("\"kernel\"").unwrap();
        assert_eq!(edit, EditToolSetting::Kernel);
        let shell: ShellToolSetting = serde_json::from_str("\"native\"").unwrap();
        assert_eq!(shell, ShellToolSetting::Native);
        assert!(serde_json::from_str::<ShellToolSetting>("\"bash\"").is_err());
    }
}
