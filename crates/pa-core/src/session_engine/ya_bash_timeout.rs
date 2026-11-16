//! yusAgent (YA-09): the `[bash-done]` notice of a command stopped by its `bash(timeout=...)`.
//!
//! Ported by hand from Unharnessed (MIT, commit 569284b36, `mark_async_bash_timeout` in
//! `crates/uh-core/src/session_engine/messages.rs` and the `timeoutSeconds` check of `wakes.rs`;
//! notice in `docs/fork-changes.md`, "Code repris d'Unharnessed"). The kernel adds
//! `timeoutSeconds` to `bash.completed` when the limit stopped the command
//! (`prime-agent-runtime/src/rlm/ya_timeout.py`); the notice then gives the reason instead of a
//! bare exit code.

use std::fmt::Write as _;

use serde_json::Value;

use super::bash_notices::KernelNotice;

/// The `bash.completed` field set when the command's time limit stopped it.
pub const TIMEOUT_SECONDS_FIELD: &str = "timeoutSeconds";

/// The notice of `data`, with the timeout line and `details.timeoutSeconds` when the payload
/// carries a positive `timeoutSeconds`. Absent or null: the notice as is; any other value is an
/// invalid payload.
pub fn mark_timeout(mut notice: KernelNotice, data: &Value) -> anyhow::Result<KernelNotice> {
    let seconds = match data.get(TIMEOUT_SECONDS_FIELD) {
        None | Some(Value::Null) => return Ok(notice),
        Some(value) => value
            .as_f64()
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .ok_or_else(|| {
                anyhow::anyhow!("bash.completed timeoutSeconds must be a positive number")
            })?,
    };
    if let pa_types::ai::UserContent::Text(text) = &mut notice.row.content {
        let _ = write!(
            text,
            "\n\nKilled: still running after its {seconds} s timeout."
        );
    }
    if let Some(Value::Object(details)) = &mut notice.row.details {
        details.insert(
            TIMEOUT_SECONDS_FIELD.to_string(),
            serde_json::json!(seconds),
        );
    }
    Ok(notice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notice() -> KernelNotice {
        KernelNotice::bash_completion(42, "sleep 900", -15)
    }

    fn text(notice: &KernelNotice) -> &str {
        match &notice.row.content {
            pa_types::ai::UserContent::Text(text) => text,
            pa_types::ai::UserContent::Blocks(_) => panic!("text notice expected"),
        }
    }

    #[test]
    fn a_payload_without_timeout_keeps_the_notice() {
        let plain = notice();
        for data in [json!({}), json!({ "timeoutSeconds": null })] {
            let marked = mark_timeout(notice(), &data).unwrap();
            assert_eq!(text(&marked), text(&plain));
            assert_eq!(marked.row.details, plain.row.details);
        }
    }

    #[test]
    fn a_timed_out_command_says_so() {
        let marked = mark_timeout(notice(), &json!({ "timeoutSeconds": 600.0 })).unwrap();
        assert!(
            text(&marked).ends_with("\n\nKilled: still running after its 600 s timeout."),
            "{}",
            text(&marked)
        );
        assert!(text(&marked).starts_with("[bash-done pid:42 exit:-15]"));
        let details = marked.row.details.unwrap();
        assert_eq!(details["timeoutSeconds"], json!(600.0));
        assert_eq!(details["pid"], json!(42));
        assert_eq!(details["command"], json!("sleep 900"));
        let fraction = mark_timeout(notice(), &json!({ "timeoutSeconds": 2.5 })).unwrap();
        assert!(text(&fraction).ends_with("after its 2.5 s timeout."));
    }

    #[test]
    fn an_invalid_timeout_is_refused() {
        for value in [json!(0), json!(-1), json!("600"), json!(true)] {
            assert!(
                mark_timeout(notice(), &json!({ "timeoutSeconds": value })).is_err(),
                "{value}"
            );
        }
    }
}
