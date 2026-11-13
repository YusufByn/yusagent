//! yusAgent (YA-03, ticket 09): the `apply_patch` tool of the Codex family.
//!
//! The tool is Codex's freeform `apply_patch` (openai/codex `69231e1`, name, description and
//! Lark grammar from `ya_apply_patch`), sent as a Responses `custom` tool through
//! `pa_ai::ya_freeform`. The patch is applied by `ya_apply_patch` (Codex's crate, follow
//! symlinks mode), after every path it touches was checked against the workspace
//! (`tools::ya_workspace`): one path outside refuses the whole patch and nothing is written.
//!
//! The result is Codex's text for the model, without additions:
//! - a patch that does not parse or cannot apply (missing file, lines not found, two hunks on
//!   one path) fails Codex's verification before anything is written
//!   (`codex-rs/core/src/tools/handlers/apply_patch.rs:477-519`, `prepare_patch_request`;
//!   `codex-rs/apply-patch/src/invocation.rs:179-251`, `verify_apply_patch_args`): the whole
//!   result is `apply_patch verification failed: <error>`;
//! - an applied (or failed) patch gives `format_exec_output_for_model`
//!   (`codex-rs/core/src/tools/mod.rs:92-117`): `Exit code`, `Wall time`, `Output:` then the
//!   crate's stdout and stderr (`codex-rs/core/src/tools/runtimes/apply_patch.rs:202-238`).
//!
//! The workspace refusal (`[outside-workspace]`, exit code 1, nothing applied) is ours; it runs
//! after the parse and before the verification, so no file outside the workspace is read.

use std::collections::HashSet;
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
    let hunks = match ya_apply_patch::parse_patch(patch) {
        Ok(args) => args.hunks,
        Err(error) => return verification_failure(&error),
    };
    if let Some(refusal) = workspace_refusal(&hunks, cwd) {
        return PatchOutcome {
            text: codex_exec_text(1, start, &format!("{refusal}\nNothing was applied.\n")),
            applied: false,
        };
    }
    if let Err(error) = verify_hunks(&hunks, cwd) {
        return verification_failure(&error);
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = ya_apply_patch::apply_patch(patch, cwd, &mut stdout, &mut stderr);
    let applied = result.is_ok();
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    PatchOutcome {
        text: codex_exec_text(i32::from(!applied), start, &output),
        applied,
    }
}

/// Codex's result for a patch that fails its verification.
fn verification_failure(error: &dyn std::fmt::Display) -> PatchOutcome {
    PatchOutcome {
        text: format!("apply_patch verification failed: {error}"),
        applied: false,
    }
}

/// Codex's `try_verify_apply_patch_args` (`codex-rs/apply-patch/src/invocation.rs:179-251`),
/// on `std::fs`: two hunks on one path are refused, a deleted file must be readable, an
/// updated file must take its chunks. Nothing is written.
fn verify_hunks(hunks: &[ya_apply_patch::Hunk], cwd: &Path) -> Result<(), String> {
    let mut seen = HashSet::new();
    for hunk in hunks {
        let path = hunk.resolve_path(cwd);
        if !seen.insert(path.clone()) {
            return Err(ya_apply_patch::ParseError::InvalidPatchError(format!(
                "multiple operations target {}",
                path.display()
            ))
            .to_string());
        }
        match hunk {
            ya_apply_patch::Hunk::AddFile { .. } => {}
            ya_apply_patch::Hunk::DeleteFile { .. } => {
                std::fs::read_to_string(&path)
                    .map_err(|source| format!("Failed to read {}: {source}", path.display()))?;
            }
            ya_apply_patch::Hunk::UpdateFile { chunks, .. } => {
                ya_apply_patch::unified_diff_from_chunks(&path, chunks)
                    .map_err(|error| format!("{error}"))?;
            }
        }
    }
    Ok(())
}

/// The refusal of the first path that leaves the workspace, `None` when all stay inside.
fn workspace_refusal(hunks: &[ya_apply_patch::Hunk], cwd: &Path) -> Option<String> {
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
    for hunk in hunks {
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

    /// The whole result is Codex's, byte for byte (the wall time of a local patch rounds to 0).
    #[test]
    fn update_result_is_codex_text_only() {
        let dir = tempfile::tempdir().unwrap();
        let body = (1..=20)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(dir.path().join("a.txt"), &body).unwrap();
        let outcome = run_patch(
            &patch("*** Update File: a.txt\n@@\n line 9\n-line 10\n+line ten\n line 11\n"),
            dir.path(),
        );
        assert!(outcome.applied, "{}", outcome.text);
        assert_eq!(
            outcome.text,
            "Exit code: 0\nWall time: 0 seconds\nOutput:\nSuccess. Updated the following files:\nM a.txt\n"
        );
        assert!(std::fs::read_to_string(dir.path().join("a.txt"))
            .unwrap()
            .contains("line ten\n"));
    }

    #[test]
    fn add_and_delete_results_are_codex_text_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "x\n").unwrap();
        let outcome = run_patch(
            &patch("*** Add File: new.txt\n+hi\n*** Delete File: old.txt\n"),
            dir.path(),
        );
        assert!(outcome.applied, "{}", outcome.text);
        assert_eq!(
            outcome.text,
            "Exit code: 0\nWall time: 0 seconds\nOutput:\nSuccess. Updated the following files:\nA new.txt\nD old.txt\n"
        );
    }

    /// Lines not found: Codex's verification fails before any write, without the exec header.
    #[test]
    fn unmatched_update_fails_codex_verification() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x\n").unwrap();
        let outcome = run_patch(
            &patch("*** Update File: a.txt\n@@\n-missing\n+y\n"),
            dir.path(),
        );
        assert!(!outcome.applied);
        assert!(
            outcome
                .text
                .starts_with("apply_patch verification failed: Failed to find expected lines in "),
            "{}",
            outcome.text
        );
        assert!(!outcome.text.contains("Exit code"), "{}", outcome.text);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn invalid_patch_fails_codex_verification() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run_patch("*** Update File: a.txt\n", dir.path());
        assert!(!outcome.applied);
        assert_eq!(
            outcome.text,
            "apply_patch verification failed: invalid patch: The first line of the patch must be '*** Begin Patch'"
        );
    }

    /// A missing file, two hunks on one path: refused by the verification, nothing written.
    #[test]
    fn verification_refuses_missing_files_and_repeated_paths() {
        let dir = tempfile::tempdir().unwrap();
        let missing = run_patch(&patch("*** Delete File: gone.txt\n"), dir.path());
        assert!(!missing.applied);
        assert!(
            missing.text.starts_with(&format!(
                "apply_patch verification failed: Failed to read {}: ",
                dir.path().join("gone.txt").display()
            )),
            "{}",
            missing.text
        );
        let repeated = run_patch(
            &patch("*** Add File: a.txt\n+1\n*** Add File: a.txt\n+2\n"),
            dir.path(),
        );
        assert_eq!(
            repeated.text,
            format!(
                "apply_patch verification failed: invalid patch: multiple operations target {}",
                dir.path().join("a.txt").display()
            )
        );
        assert!(!dir.path().join("a.txt").exists(), "nothing written");
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
}
