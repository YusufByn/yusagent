//! yusAgent (YA-09): request settings frozen per session, today `reasoningSummary`.
//!
//! `reasoningSummary` in `settings.json`:
//! - unset or `"auto"`: the request is left as is. The `OpenAI` Responses providers send
//!   `reasoning.summary: "auto"` with every reasoning effort
//!   (`pa_ai::providers::openai_codex_responses`, `reasoning_summary_value`);
//! - `"off"`: `reasoning.summary` is removed from the request body, as Codex CLI does for
//!   `gpt-6-luna` (`default_reasoning_summary: "none"`, field omitted,
//!   `codex-rs/core/src/client.rs:1149-1152` at `rust-v0.169.0`). Only the Responses APIs of
//!   [`RESPONSES_APIS`] are touched. The provider then streams reasoning items without summary
//!   text: the thinking blocks keep their signature (replay) with an empty text, and nothing is
//!   shown as reasoning in the TUI or over ACP.
//!
//! The removal is a payload hook around the session's stream function ([`scoped_stream_fn`]),
//! so the provider layer stays as upstream. It runs before the hooks of the request timing and
//! the usage journal, which see the body as sent.
//!
//! Frozen for the session at creation in the session header, field [`REQUEST_HEADER_KEY`]
//! (kept by forks, `session::manager::ya_header::INHERITED_HEADER_KEYS`), and read back on
//! every resume; settings changed in between give a warning. A session from before this field
//! is `"auto"`, what it was sent with: it is recognized by the frozen tools of YA-03 in its
//! header (written to every session since YA-03). An unreadable `yaRequest` is rewritten as
//! `"auto"` too. A session from before YA-03 (neither field) is frozen with the current
//! settings, as its tools are. Listed in `docs/fork-changes.md`.

use std::sync::Arc;

use pa_agent::stream::{OnPayloadHook, StreamFn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::manager::SessionManager;

/// The `reasoningSummary` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummarySetting {
    /// The provider default: `reasoning.summary: "auto"` on the Responses APIs.
    #[default]
    Auto,
    /// `reasoning.summary` removed from the request body.
    Off,
}

/// The APIs whose request body carries `reasoning.summary`.
pub const RESPONSES_APIS: &[&str] = &[
    "openai-codex-responses",
    "openai-responses",
    "azure-openai-responses",
];

/// The session header field that freezes the request settings
/// (`SessionManager::ya_set_header_field`).
pub const REQUEST_HEADER_KEY: &str = "yaRequest";

/// The effective request settings of a session, written to its header at creation and read
/// back on every resume: an unset setting is stored as the value it resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenRequest {
    #[serde(default)]
    pub reasoning_summary: ReasoningSummarySetting,
}

impl FrozenRequest {
    /// The request settings of the current settings.
    #[must_use]
    pub fn of(reasoning_summary: Option<ReasoningSummarySetting>) -> Self {
        FrozenRequest {
            reasoning_summary: reasoning_summary.unwrap_or_default(),
        }
    }

    /// The frozen request settings of a session header field; `None` when absent or
    /// unreadable.
    #[must_use]
    pub fn from_header_value(value: Option<&Value>) -> Option<Self> {
        value.and_then(|value| serde_json::from_value(value.clone()).ok())
    }
}

/// The request settings of a session being built, and what the engine does about its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    pub frozen: FrozenRequest,
    /// To write to the header: the session had no frozen request settings.
    pub to_write: Option<FrozenRequest>,
    /// Shown at resume when the current settings differ.
    pub warning: Option<String>,
}

fn label(setting: ReasoningSummarySetting) -> String {
    serde_json::to_value(setting).map_or_else(|_| "?".to_string(), |value| value.to_string())
}

/// The request settings of a session: the frozen ones of its header (a resume), `"auto"` for a
/// session created before the field (`legacy`), else the current settings (frozen from now
/// on).
#[must_use]
pub fn session_request(
    frozen: Option<FrozenRequest>,
    legacy: bool,
    reasoning_summary: Option<ReasoningSummarySetting>,
) -> SessionRequest {
    let current = FrozenRequest::of(reasoning_summary);
    let Some(frozen) = frozen else {
        let frozen = if legacy {
            FrozenRequest::default()
        } else {
            current
        };
        return SessionRequest {
            frozen,
            to_write: Some(frozen),
            warning: legacy.then(|| warning(frozen, current)).flatten(),
        };
    };
    SessionRequest {
        frozen,
        to_write: None,
        warning: warning(frozen, current),
    }
}

fn warning(frozen: FrozenRequest, current: FrozenRequest) -> Option<String> {
    (frozen != current).then(|| {
        format!(
            "Warning: this session keeps the request settings it was created with (reasoningSummary {}); the current settings (reasoningSummary {}) apply to new sessions only.",
            label(frozen.reasoning_summary),
            label(current.reasoning_summary),
        )
    })
}

/// The request settings of the session `manager` holds: the frozen ones of its header, or the
/// current settings written to a header that has none. A header that already has the frozen
/// tools of YA-03 is from before this field, and an unreadable field is rewritten: both
/// `"auto"`. Prints the resume warning to stderr. Call it before the tools are frozen in a new
/// session.
pub fn freeze_session_request(
    manager: &mut SessionManager,
    reasoning_summary: Option<ReasoningSummarySetting>,
) -> FrozenRequest {
    let field = manager.ya_header_field(REQUEST_HEADER_KEY);
    let frozen = FrozenRequest::from_header_value(field);
    let unreadable = field.is_some() && frozen.is_none();
    let legacy = unreadable
        || manager
            .ya_header_field(super::ya_tool_family::FROZEN_TOOLS_HEADER_KEY)
            .is_some();
    let request = session_request(frozen, legacy, reasoning_summary);
    if let Some(to_write) = request.to_write {
        if let Ok(value) = serde_json::to_value(to_write) {
            manager.ya_set_header_field(REQUEST_HEADER_KEY, value);
        }
    }
    if let Some(warning) = &request.warning {
        eprintln!("{warning}");
    }
    request.frozen
}

/// `payload` without `reasoning.summary` when `api` is one of [`RESPONSES_APIS`] and the
/// body has it; `None` otherwise (the body is left as is).
#[must_use]
pub fn strip_reasoning_summary(api: &str, payload: &Value) -> Option<Value> {
    if !RESPONSES_APIS.contains(&api) {
        return None;
    }
    let has_summary = payload
        .get("reasoning")
        .and_then(Value::as_object)
        .is_some_and(|reasoning| reasoning.contains_key("summary"));
    if !has_summary {
        return None;
    }
    let mut next = payload.clone();
    if let Some(reasoning) = next.get_mut("reasoning").and_then(Value::as_object_mut) {
        reasoning.remove("summary");
    }
    Some(next)
}

/// `hook` preceded by the removal of `reasoning.summary`: the hooks after it (request timing,
/// usage journal, a loop hook) see the body as sent.
fn strip_then(hook: Option<OnPayloadHook>) -> OnPayloadHook {
    Arc::new(move |payload: Value, model: &pa_agent::types::Model| {
        let stripped = strip_reasoning_summary(&model.api, &payload);
        let changed = stripped.is_some();
        let payload = stripped.unwrap_or(payload);
        match &hook {
            Some(hook) => match hook(payload.clone(), model) {
                Some(next) => Some(next),
                None => changed.then_some(payload),
            },
            None => changed.then_some(payload),
        }
    })
}

/// `inner` with the session's frozen request settings applied to each request. `"auto"` returns
/// `inner` itself: the request is the provider's, byte for byte.
#[must_use]
pub fn scoped_stream_fn(inner: StreamFn, request: FrozenRequest) -> StreamFn {
    if request.reasoning_summary == ReasoningSummarySetting::Auto {
        return inner;
    }
    Arc::new(move |model, context, mut options| {
        options.on_payload = Some(strip_then(options.on_payload.take()));
        inner(model, context, options)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn codex_body() -> Value {
        json!({
            "model": "gpt-6-luna",
            "input": [],
            "reasoning": {"effort": "high", "summary": "auto"},
            "include": ["reasoning.encrypted_content"]
        })
    }

    #[test]
    fn the_setting_parses_auto_and_off_only() {
        assert_eq!(
            serde_json::from_value::<ReasoningSummarySetting>(json!("auto")).unwrap(),
            ReasoningSummarySetting::Auto
        );
        assert_eq!(
            serde_json::from_value::<ReasoningSummarySetting>(json!("off")).unwrap(),
            ReasoningSummarySetting::Off
        );
        for other in ["none", "concise", "detailed", "OFF"] {
            assert!(serde_json::from_value::<ReasoningSummarySetting>(json!(other)).is_err());
        }
    }

    #[test]
    fn off_removes_only_the_summary() {
        for api in RESPONSES_APIS {
            let stripped = strip_reasoning_summary(api, &codex_body()).expect("stripped");
            let mut expected = codex_body();
            expected["reasoning"] = json!({"effort": "high"});
            assert_eq!(stripped, expected);
        }
    }

    #[test]
    fn other_apis_and_bodies_without_summary_are_left_as_is() {
        assert_eq!(
            strip_reasoning_summary("anthropic-messages", &codex_body()),
            None
        );
        assert_eq!(
            strip_reasoning_summary("openai-completions", &codex_body()),
            None
        );
        let no_reasoning = json!({"model": "m", "input": []});
        assert_eq!(
            strip_reasoning_summary("openai-codex-responses", &no_reasoning),
            None
        );
        let effort_only = json!({"reasoning": {"effort": "high"}});
        assert_eq!(
            strip_reasoning_summary("openai-codex-responses", &effort_only),
            None
        );
    }

    fn model(api: &str) -> pa_agent::types::Model {
        pa_agent::types::Model {
            api: api.to_string(),
            ..pa_agent::types::Model::unknown()
        }
    }

    /// The hooks after the removal see the stripped body; a hook that replaces the body wins.
    #[test]
    fn later_hooks_see_the_stripped_body() {
        let seen: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
        let observer = Arc::clone(&seen);
        let hook: OnPayloadHook = Arc::new(move |payload: Value, _: &pa_agent::types::Model| {
            observer.lock().unwrap().push(payload);
            None
        });
        let wrapped = strip_then(Some(hook));
        let sent = wrapped(codex_body(), &model("openai-codex-responses")).expect("replaced");
        assert_eq!(sent["reasoning"], json!({"effort": "high"}));
        assert_eq!(*seen.lock().unwrap(), vec![sent]);

        let replacing: OnPayloadHook =
            Arc::new(|_: Value, _: &pa_agent::types::Model| Some(json!({"replaced": true})));
        let wrapped = strip_then(Some(replacing));
        assert_eq!(
            wrapped(codex_body(), &model("openai-codex-responses")),
            Some(json!({"replaced": true}))
        );
        // Nothing to strip and no hook: the body is left as is.
        let wrapped = strip_then(None);
        assert_eq!(wrapped(codex_body(), &model("anthropic-messages")), None);
    }

    #[test]
    fn a_new_session_freezes_the_current_settings() {
        let request = session_request(None, false, Some(ReasoningSummarySetting::Off));
        assert_eq!(
            request.frozen.reasoning_summary,
            ReasoningSummarySetting::Off
        );
        assert_eq!(request.to_write, Some(request.frozen));
        assert_eq!(request.warning, None);
        let unset = session_request(None, false, None);
        assert_eq!(unset.frozen, FrozenRequest::default());
        assert_eq!(unset.to_write, Some(FrozenRequest::default()));
    }

    /// A session from before the field was sent with `"auto"` and keeps it.
    #[test]
    fn a_legacy_session_is_auto() {
        let request = session_request(None, true, Some(ReasoningSummarySetting::Off));
        assert_eq!(
            request.frozen.reasoning_summary,
            ReasoningSummarySetting::Auto
        );
        assert_eq!(request.to_write, Some(FrozenRequest::default()));
        assert!(request.warning.is_some());
        let request = session_request(None, true, None);
        assert_eq!(request.warning, None);
    }

    #[test]
    fn a_resume_keeps_the_frozen_settings_and_warns_on_a_change() {
        let off = FrozenRequest {
            reasoning_summary: ReasoningSummarySetting::Off,
        };
        let request = session_request(Some(off), false, None);
        assert_eq!(request.frozen, off);
        assert_eq!(request.to_write, None);
        let warning = request.warning.expect("warning");
        assert!(warning.contains("reasoningSummary \"off\""), "{warning}");
        assert!(warning.contains("reasoningSummary \"auto\""), "{warning}");
        let same = session_request(Some(off), true, Some(ReasoningSummarySetting::Off));
        assert_eq!(same.warning, None);
    }

    #[test]
    fn the_header_value_round_trips_and_defaults_to_auto() {
        let off = FrozenRequest {
            reasoning_summary: ReasoningSummarySetting::Off,
        };
        let value = serde_json::to_value(off).unwrap();
        assert_eq!(value, json!({"reasoningSummary": "off"}));
        assert_eq!(FrozenRequest::from_header_value(Some(&value)), Some(off));
        assert_eq!(
            FrozenRequest::from_header_value(Some(&json!({}))),
            Some(FrozenRequest::default())
        );
        assert_eq!(FrozenRequest::from_header_value(Some(&json!("x"))), None);
        assert_eq!(FrozenRequest::from_header_value(None), None);
    }

    #[test]
    fn freeze_writes_new_sessions_and_reads_legacy_ones_as_auto() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = SessionManager::in_memory(dir.path());
        let frozen = freeze_session_request(&mut manager, Some(ReasoningSummarySetting::Off));
        assert_eq!(frozen.reasoning_summary, ReasoningSummarySetting::Off);
        assert_eq!(
            manager.ya_header_field(REQUEST_HEADER_KEY),
            Some(&json!({"reasoningSummary": "off"}))
        );
        // A resume keeps it whatever the settings say.
        let again = freeze_session_request(&mut manager, None);
        assert_eq!(again, frozen);

        // A header frozen by YA-03 but without the request field: auto.
        let mut legacy = SessionManager::in_memory(dir.path());
        legacy.ya_set_header_field(
            super::super::ya_tool_family::FROZEN_TOOLS_HEADER_KEY,
            json!({"editTool": "native", "shellTool": "kernel"}),
        );
        let frozen = freeze_session_request(&mut legacy, Some(ReasoningSummarySetting::Off));
        assert_eq!(frozen, FrozenRequest::default());
        assert_eq!(
            legacy.ya_header_field(REQUEST_HEADER_KEY),
            Some(&json!({"reasoningSummary": "auto"}))
        );
    }

    /// An unreadable `yaRequest` (here without the frozen tools) is rewritten as `"auto"`,
    /// whatever the settings say.
    #[test]
    fn an_unreadable_field_is_rewritten_as_auto() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [json!("off"), json!({"reasoningSummary": "none"}), json!(1)] {
            let mut manager = SessionManager::in_memory(dir.path());
            manager.ya_set_header_field(REQUEST_HEADER_KEY, bad);
            let frozen = freeze_session_request(&mut manager, Some(ReasoningSummarySetting::Off));
            assert_eq!(frozen, FrozenRequest::default());
            assert_eq!(
                manager.ya_header_field(REQUEST_HEADER_KEY),
                Some(&json!({"reasoningSummary": "auto"}))
            );
        }
    }
}
