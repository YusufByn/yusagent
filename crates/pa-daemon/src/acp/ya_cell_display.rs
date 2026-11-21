//! yusAgent (YA-10, ticket 24): what a finished cell shows the editor over ACP, read from the
//! tool-result `details` (display data only, never in the model's text):
//! - each `edit()` of the cell as an ACP `diff` content (`details.diffs`, with the raw `oldStr`,
//!   `newStr` and `startLine` the host adds for this purpose; `crates/pa-core/src/tools/ipython.rs`):
//!   `path` absolute (the `edit` skill resolves it), `oldText` absent for a created file, the texts
//!   being the edit's fragments and `_meta` `_yus/startLine` their first line;
//! - the `bash()` results the cell read (`details.bashRuns`: command, exit code, output capped at
//!   8 000 characters by the kernel, duration) under the update's `_meta`, key [`BASH_RUNS_META`].
//!
//! Fork additions under `_yus/*` (decision of 19/11/2026, ticket 19). Listed in
//! `docs/fork-changes.md`.

use serde_json::{json, Value};

use super::types::ToolCallContent;

/// The `_meta` key of the `bash()` results of a cell.
pub const BASH_RUNS_META: &str = "_yus/bashRuns";
/// The `_meta` key of a diff content's first line.
pub const START_LINE_META: &str = "_yus/startLine";

fn details(result: Option<&Value>) -> Option<&Value> {
    result?.get("details")
}

/// The ACP `diff` contents of the cell's edits; edits without their raw texts (a session written
/// before ticket 24) are skipped.
#[must_use]
pub fn diff_contents(result: Option<&Value>) -> Vec<ToolCallContent> {
    let Some(diffs) = details(result)
        .and_then(|details| details.get("diffs"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    diffs
        .iter()
        .filter_map(|diff| {
            let path = diff.get("path")?.as_str()?;
            let old = diff.get("oldStr")?.as_str()?;
            let new = diff.get("newStr")?.as_str()?;
            Some(ToolCallContent::Diff {
                path: path.to_string(),
                old_text: (!old.is_empty()).then(|| old.to_string()),
                new_text: new.to_string(),
                meta: diff
                    .get("startLine")
                    .and_then(Value::as_u64)
                    .map(|line| json!({ START_LINE_META: line })),
            })
        })
        .collect()
}

/// The `bash()` results of the cell, for the update's `_meta`.
#[must_use]
pub fn bash_runs(result: Option<&Value>) -> Option<Value> {
    details(result)
        .and_then(|details| details.get("bashRuns"))
        .filter(|runs| runs.as_array().is_some_and(|runs| !runs.is_empty()))
        .cloned()
}

/// Adds [`BASH_RUNS_META`] to an update's `_meta` (created when absent).
#[must_use]
pub fn with_bash_runs(meta: Option<Value>, runs: Option<Value>) -> Option<Value> {
    let Some(runs) = runs else {
        return meta;
    };
    let mut meta = meta.unwrap_or_else(|| json!({}));
    if let Some(object) = meta.as_object_mut() {
        object.insert(BASH_RUNS_META.to_string(), runs);
    }
    Some(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(details: &Value) -> Value {
        json!({ "content": [{ "type": "text", "text": "out" }], "details": details })
    }

    #[test]
    fn edits_become_acp_diffs() {
        let cell = result(&json!({ "diffs": [
            { "path": "/w/a.py", "diff": "-3 x\n+3 y", "oldStr": "x", "newStr": "y", "startLine": 3 },
            { "path": "/w/new.py", "diff": "+1 z", "oldStr": "", "newStr": "z", "startLine": 1 },
            { "path": "/w/old.py", "diff": "-1 a\n+1 b" },
        ] }));
        let contents = diff_contents(Some(&cell));
        assert_eq!(
            serde_json::to_value(&contents).unwrap(),
            json!([
                { "type": "diff", "path": "/w/a.py", "oldText": "x", "newText": "y",
                  "_meta": { "_yus/startLine": 3 } },
                { "type": "diff", "path": "/w/new.py", "newText": "z",
                  "_meta": { "_yus/startLine": 1 } },
            ])
        );
        assert!(diff_contents(Some(&json!({ "content": [] }))).is_empty());
        assert!(diff_contents(None).is_empty());
    }

    #[test]
    fn bash_runs_ride_the_meta() {
        let runs = json!([{ "command": "ls", "exitCode": 0, "output": "a\n", "durationMs": 4 }]);
        let cell = result(&json!({ "bashRuns": runs }));
        let meta = with_bash_runs(
            Some(json!({ "ai.primeintellect.prime-agent": { "diffCount": 1 } })),
            bash_runs(Some(&cell)),
        );
        assert_eq!(
            meta,
            Some(json!({
                "ai.primeintellect.prime-agent": { "diffCount": 1 },
                "_yus/bashRuns": runs,
            }))
        );
        assert_eq!(
            with_bash_runs(None, bash_runs(Some(&cell))),
            Some(json!({ "_yus/bashRuns": runs }))
        );
        assert_eq!(
            with_bash_runs(None, bash_runs(Some(&result(&json!({ "bashRuns": [] }))))),
            None
        );
    }
}
