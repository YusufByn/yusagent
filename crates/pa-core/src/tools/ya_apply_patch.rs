//! yusAgent (YA-03, ticket 09): the `apply_patch` tool of the Codex family.
//!
//! The tool is Codex's freeform `apply_patch` (openai/codex `69231e1`, name, description and
//! Lark grammar from `ya_apply_patch`), sent as a Responses `custom` tool through
//! `pa_ai::ya_freeform`. The patch is applied by `ya_apply_patch` (Codex's crate, follow
//! symlinks mode), after every path it touches was checked against the workspace
//! (`tools::ya_workspace`): one path outside refuses the whole patch and nothing is written.
//!
//! The result keeps Codex's shape for the model (`codex-rs/core/src/tools/mod.rs:92-116`,
//! `format_exec_output_for_model`: `Exit code`, `Wall time`, `Output:` then the crate's
//! stdout and stderr), followed by the changed region of each updated file with its line
//! numbers (the `cat -n` layout of `read()`), so the model needs no read-back to check it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use crate::tools::tool_definition::{
    ExecutionMode, ToolContentBlock, ToolDefinition, ToolExecutionResult,
};
use crate::tools::ya_workspace::{allowed_roots, check_inside, WorkspaceScope};

/// Name of the tool (Codex's).
pub const APPLY_PATCH_TOOL_NAME: &str = ya_apply_patch::APPLY_PATCH_TOOL_NAME;

/// Unchanged lines shown around each change in the result.
const ZONE_CONTEXT_LINES: usize = 3;

/// The `apply_patch` tool for a session working in `cwd`.
#[must_use]
pub fn create_apply_patch_tool_definition(cwd: PathBuf) -> ToolDefinition {
    let cwd = Arc::new(absolute(cwd));
    let format = json!({
        "type": "grammar",
        "syntax": "lark",
        "definition": ya_apply_patch::APPLY_PATCH_LARK_GRAMMAR,
    });
    ToolDefinition {
        name: APPLY_PATCH_TOOL_NAME.to_string(),
        label: APPLY_PATCH_TOOL_NAME.to_string(),
        description: ya_apply_patch::APPLY_PATCH_TOOL_DESCRIPTION.to_string(),
        prompt_snippet: "apply_patch - edit files with a patch".to_string(),
        parameters: pa_ai::ya_freeform::freeform_tool_parameters(&format),
        // Two patches of one batch must not interleave on the same files.
        execution_mode: Some(ExecutionMode::Sequential),
        prepare_arguments: None,
        execute: Arc::new(move |_tool_call_id: &str, params, _signal, _on_update| {
            let cwd = Arc::clone(&cwd);
            Box::pin(async move {
                let patch = params
                    .get(pa_ai::ya_freeform::FREEFORM_INPUT_ARG)
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("apply_patch requires the patch text"))?
                    .to_string();
                let outcome = tokio::task::spawn_blocking(move || run_patch(&patch, &cwd))
                    .await
                    .map_err(|join| anyhow::anyhow!("apply_patch task failed: {join}"))?;
                Ok(ToolExecutionResult {
                    content: vec![ToolContentBlock::text(outcome.text)],
                    details: None,
                    is_error: !outcome.applied,
                })
            })
        }),
    }
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    match std::env::current_dir() {
        Ok(dir) => dir.join(path),
        Err(_) => path,
    }
}

/// The text of one call and whether the patch was applied.
#[derive(Debug)]
pub(crate) struct PatchOutcome {
    pub(crate) text: String,
    pub(crate) applied: bool,
}

/// Check, apply and describe one patch.
pub(crate) fn run_patch(patch: &str, cwd: &Path) -> PatchOutcome {
    let start = Instant::now();
    if let Some(refusal) = workspace_refusal(patch, cwd) {
        return PatchOutcome {
            text: codex_exec_text(1, start, &format!("{refusal}\nNothing was applied.\n")),
            applied: false,
        };
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = ya_apply_patch::apply_patch(patch, cwd, &mut stdout, &mut stderr);
    let mut output = String::from_utf8_lossy(&stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&stderr));
    let (applied, delta) = match &result {
        Ok(delta) => (true, delta),
        Err(failure) => (false, failure.delta()),
    };
    let zones = changed_zones(delta, cwd);
    let mut text = codex_exec_text(i32::from(!applied), start, &output);
    if !zones.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&zones);
    }
    PatchOutcome { text, applied }
}

/// The refusal of the first path that leaves the workspace, `None` when all stay inside or
/// the patch does not parse (the crate then reports the parse error, writing nothing).
fn workspace_refusal(patch: &str, cwd: &Path) -> Option<String> {
    let hunks = ya_apply_patch::parse_patch(patch).ok()?.hunks;
    let roots = match allowed_roots(&WorkspaceScope { session_cwd: cwd }) {
        Ok(roots) => roots,
        Err(error) => {
            return Some(format!(
                "{} {}: the workspace cannot be resolved: {error}",
                crate::tools::ya_workspace::OUTSIDE_WORKSPACE_MARKER,
                cwd.display()
            ))
        }
    };
    for hunk in &hunks {
        let mut targets = vec![hunk.resolve_path(cwd)];
        if let ya_apply_patch::Hunk::UpdateFile {
            move_path: Some(dest),
            ..
        } = hunk
        {
            // The crate joins the destination like any hunk path (`lib.rs`, `local_fs::join`).
            targets.push(ya_apply_patch::Hunk::DeleteFile { path: dest.clone() }.resolve_path(cwd));
        }
        for target in targets {
            if let Err(outside) = check_inside(&target, &roots) {
                return Some(outside.to_string());
            }
        }
    }
    None
}

/// Codex's model text for an exec result (`format_exec_output_for_model`, without truncation:
/// the session's output cap applies to the whole result).
fn codex_exec_text(exit_code: i32, start: Instant, output: &str) -> String {
    // Rounded to 1 decimal place, as Codex does.
    let duration_seconds = (start.elapsed().as_secs_f32() * 10.0).round() / 10.0;
    format!("Exit code: {exit_code}\nWall time: {duration_seconds} seconds\nOutput:\n{output}")
}

/// The changed region of each updated or moved file, numbered from its new content.
fn changed_zones(delta: &ya_apply_patch::AppliedPatchDelta, cwd: &Path) -> String {
    let mut out = String::new();
    for change in delta.changes() {
        let ya_apply_patch::AppliedPatchFileChange::Update {
            move_path,
            old_content,
            new_content,
            ..
        } = &change.change
        else {
            continue;
        };
        let path = move_path.as_deref().unwrap_or(&change.path);
        let zone = numbered_zone(old_content, new_content);
        if zone.is_empty() {
            continue;
        }
        let shown = path.strip_prefix(cwd).unwrap_or(path);
        let _ = write!(out, "\n{} after the edit:\n{zone}", shown.display());
    }
    out
}

/// The lines of `new` around each difference from `old`, as `<number:6>\t<text>`, groups
/// separated by `...`.
fn numbered_zone(old: &str, new: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let lines: Vec<&str> = new.split_inclusive('\n').collect();
    let mut out = String::new();
    for (index, group) in diff.grouped_ops(ZONE_CONTEXT_LINES).iter().enumerate() {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let start = first.new_range().start;
        let end = last.new_range().end.min(lines.len());
        if start >= end {
            continue;
        }
        if index > 0 {
            out.push_str("...\n");
        }
        for (offset, line) in lines[start..end].iter().enumerate() {
            let text = line.strip_suffix('\n').unwrap_or(line);
            let text = text.strip_suffix('\r').unwrap_or(text);
            let _ = writeln!(out, "{:>6}\t{text}", start + offset + 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(body: &str) -> String {
        format!("*** Begin Patch\n{body}*** End Patch\n")
    }

    #[test]
    fn the_tool_is_codex_freeform_apply_patch() {
        let definition = create_apply_patch_tool_definition(PathBuf::from("/w"));
        assert_eq!(definition.name, "apply_patch");
        assert_eq!(
            definition.description,
            "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON."
        );
        let tool = definition.to_tool();
        let custom = pa_ai::ya_freeform::responses_custom_tool(&tool).expect("freeform tool");
        assert_eq!(custom["type"], "custom");
        assert_eq!(custom["format"]["type"], "grammar");
        assert_eq!(custom["format"]["syntax"], "lark");
        assert!(custom["format"]["definition"]
            .as_str()
            .unwrap()
            .starts_with("start: begin_patch hunk+ end_patch\n"));
    }

    #[test]
    fn update_reports_codex_header_and_numbered_zone() {
        let dir = tempfile::tempdir().unwrap();
        let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("a.txt"), &body).unwrap();
        let outcome = run_patch(
            &patch("*** Update File: a.txt\n@@\n line 9\n-line 10\n+line ten\n line 11\n"),
            dir.path(),
        );
        assert!(outcome.applied, "{}", outcome.text);
        assert!(
            outcome
                .text
                .starts_with("Exit code: 0\nWall time: 0 seconds\nOutput:\nSuccess. Updated the following files:\nM a.txt\n"),
            "{}",
            outcome.text
        );
        assert!(
            outcome.text.contains("\na.txt after the edit:\n"),
            "{}",
            outcome.text
        );
        assert!(
            outcome.text.contains("     7\tline 7\n"),
            "{}",
            outcome.text
        );
        assert!(
            outcome.text.contains("    10\tline ten\n"),
            "{}",
            outcome.text
        );
        assert!(
            outcome.text.contains("    13\tline 13\n"),
            "{}",
            outcome.text
        );
        assert!(!outcome.text.contains("\tline 1\n"), "{}", outcome.text);
        assert!(std::fs::read_to_string(dir.path().join("a.txt"))
            .unwrap()
            .contains("line ten\n"));
    }

    #[test]
    fn failed_patch_is_an_error_with_exit_code_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\n").unwrap();
        let outcome = run_patch(
            &patch("*** Update File: a.txt\n@@\n-missing\n+y\n"),
            dir.path(),
        );
        assert!(!outcome.applied);
        assert!(
            outcome.text.starts_with("Exit code: 1\n"),
            "{}",
            outcome.text
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn path_outside_the_workspace_refuses_the_whole_patch() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let outside = other.path().join("evil.txt");
        let outcome = run_patch(
            &patch(&format!(
                "*** Add File: inside.txt\n+ok\n*** Add File: {}\n+no\n",
                outside.display()
            )),
            dir.path(),
        );
        assert!(!outcome.applied);
        assert!(
            outcome
                .text
                .contains(crate::tools::ya_workspace::OUTSIDE_WORKSPACE_MARKER),
            "{}",
            outcome.text
        );
        assert!(outcome.text.contains("Nothing was applied."));
        assert!(!dir.path().join("inside.txt").exists(), "nothing written");
        assert!(!outside.exists());
    }

    #[test]
    fn parent_escape_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run_patch(&patch("*** Add File: ../escape.txt\n+no\n"), dir.path());
        assert!(!outcome.applied, "{}", outcome.text);
        assert!(!dir.path().parent().unwrap().join("escape.txt").exists());
    }

    #[test]
    fn move_destination_outside_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\n").unwrap();
        let outcome = run_patch(
            &patch(&format!(
                "*** Update File: a.txt\n*** Move to: {}\n@@\n-x\n+y\n",
                other.path().join("b.txt").display()
            )),
            dir.path(),
        );
        assert!(!outcome.applied, "{}", outcome.text);
        assert!(dir.path().join("a.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_leaving_the_workspace_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("link")).unwrap();
        let outcome = run_patch(&patch("*** Add File: link/x.txt\n+no\n"), dir.path());
        assert!(!outcome.applied, "{}", outcome.text);
        assert!(!other.path().join("x.txt").exists());
    }

    #[test]
    fn numbered_zone_groups_distant_changes() {
        let old: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l2\n", "L2\n").replace("l28\n", "L28\n");
        let zone = numbered_zone(&old, &new);
        assert!(zone.contains("     2\tL2\n"), "{zone}");
        assert!(zone.contains("...\n"), "{zone}");
        assert!(zone.contains("    28\tL28\n"), "{zone}");
        assert!(!zone.contains("\tl15\n"), "{zone}");
    }
}
