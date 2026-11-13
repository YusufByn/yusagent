//! yusAgent (YA-03, ticket 09): output cap of a `python_repl` cell.
//!
//! The text the model receives for one cell (stdout, stderr, result, traceback, as composed by
//! `tools::ipython`) is capped at `kernel.maxOutputChars` characters (default 40 000, about
//! 10 000 tokens: the truncation Codex applies for `gpt-6.1-terra`, openai/codex `69231e1`,
//! `codex-rs/models-manager/models.json`; the 4 characters per token ratio is an estimate).
//! Over the cap, the start and the end of the text are kept around one line that gives the
//! total size and the path of a file holding the full text, so no information is lost: the
//! model reads the file when it needs it.
//!
//! The kernel keeps up to [`KERNEL_BUFFER_CHARS`] characters per stream (upstream: 65 536,
//! `kernel::shared::DEFAULT_MAX_OUTPUT_CHARS`), so the saved file is complete up to that size.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::tools::tool_definition::{ToolContentBlock, ToolDefinition};

/// `kernel.maxOutputChars` when unset.
pub const DEFAULT_MAX_OUTPUT_CHARS: usize = 40_000;

/// Characters the kernel keeps per stream of one cell, so the saved full output is complete
/// up to this size.
pub const KERNEL_BUFFER_CHARS: usize = 2_000_000;

/// The resolved cap: `kernel.maxOutputChars`, `0` disables it.
#[must_use]
pub fn max_output_chars(settings: &crate::settings::Settings) -> usize {
    settings
        .kernel
        .as_ref()
        .and_then(|kernel| kernel.max_output_chars)
        .unwrap_or(DEFAULT_MAX_OUTPUT_CHARS)
}

/// Where full outputs of a session go: `<session artifact dir>/outputs`, or a per-session
/// directory under the system temp dir for a session without artifact dir.
#[must_use]
pub fn output_dir(session_artifact_dir: Option<&Path>, session_id: &str) -> PathBuf {
    match session_artifact_dir {
        Some(dir) => dir.join("outputs"),
        None => std::env::temp_dir()
            .join("yusagent-outputs")
            .join(file_stem(session_id)),
    }
}

/// The cap of one session.
#[derive(Debug, Clone)]
pub struct OutputCap {
    max_chars: usize,
    dir: PathBuf,
}

impl OutputCap {
    #[must_use]
    pub fn new(max_chars: usize, dir: PathBuf) -> Self {
        Self { max_chars, dir }
    }

    /// `text` itself when it fits (or the cap is `0`); otherwise its start and end around a
    /// line naming the file that holds all of it. The result has at most `max_chars`
    /// characters unless the line alone is longer.
    #[must_use]
    pub fn cap_text(&self, text: &str, tool_call_id: &str) -> String {
        let total = text.chars().count();
        if self.max_chars == 0 || total <= self.max_chars {
            return text.to_string();
        }
        let marker = match self.save(text, tool_call_id) {
            Ok(path) => format!(
                "\n[... output truncated: {total} characters in total; full output in {} ...]\n",
                path.display()
            ),
            Err(error) => format!(
                "\n[... output truncated: {total} characters in total; the full output could not be saved: {error} ...]\n"
            ),
        };
        let budget = self.max_chars.saturating_sub(marker.chars().count());
        let head = budget / 2;
        let tail = budget - head;
        let mut capped: String = text.chars().take(head).collect();
        capped.push_str(&marker);
        capped.extend(text.chars().skip(total - tail));
        capped
    }

    fn save(&self, text: &str, tool_call_id: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{}.txt", file_stem(tool_call_id)));
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

/// A file name stem from an id: ASCII letters, digits, `-` and `_` kept, anything else `_`.
fn file_stem(id: &str) -> String {
    let stem: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() {
        "cell".to_string()
    } else {
        stem
    }
}

/// Wrap a tool's executor so the text of its result goes through `cap`.
pub fn wrap_execute(definition: &mut ToolDefinition, cap: OutputCap) {
    let inner = Arc::clone(&definition.execute);
    let cap = Arc::new(cap);
    definition.execute = Arc::new(move |tool_call_id: &str, params, signal, on_update| {
        let inner = Arc::clone(&inner);
        let cap = Arc::clone(&cap);
        let tool_call_id = tool_call_id.to_string();
        Box::pin(async move {
            let mut result = (*inner)(&tool_call_id, params, signal, on_update).await?;
            if let Some(ToolContentBlock::Text { text }) = result.content.first_mut() {
                *text = cap.cap_text(text.as_str(), &tool_call_id);
            }
            Ok(result)
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_within_the_cap_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let cap = OutputCap::new(10, dir.path().join("outputs"));
        assert_eq!(cap.cap_text("0123456789", "call_1"), "0123456789");
        assert!(
            !dir.path().join("outputs").exists(),
            "no file under the cap"
        );
    }

    #[test]
    fn long_text_keeps_start_and_end_and_saves_everything() {
        let dir = tempfile::tempdir().unwrap();
        let cap = OutputCap::new(300, dir.path().join("outputs"));
        let text: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        let capped = cap.cap_text(&text, "call_1|fc_1");
        assert!(capped.chars().count() <= 300, "{}", capped.chars().count());
        assert!(capped.starts_with("line 0\n"));
        assert!(capped.ends_with("line 999\n"));
        let path = dir.path().join("outputs").join("call_1_fc_1.txt");
        assert!(capped.contains(&format!(
            "[... output truncated: {} characters in total; full output in {} ...]",
            text.chars().count(),
            path.display()
        )));
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn cap_counts_characters_not_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cap = OutputCap::new(400, dir.path().to_path_buf());
        let text = "é".repeat(1000);
        let capped = cap.cap_text(&text, "c");
        assert!(capped.chars().count() <= 400);
        assert!(capped.starts_with('é') && capped.ends_with('é'));
    }

    #[test]
    fn zero_disables_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cap = OutputCap::new(0, dir.path().to_path_buf());
        let text = "x".repeat(100_000);
        assert_eq!(cap.cap_text(&text, "c"), text);
    }

    #[test]
    fn default_cap_and_setting() {
        assert_eq!(
            max_output_chars(&crate::settings::Settings::default()),
            40_000
        );
        let settings = crate::settings::Settings {
            kernel: Some(crate::kernel::env_allowlist::KernelSettings {
                max_output_chars: Some(1234),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(max_output_chars(&settings), 1234);
    }

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(file_stem("call_1|fc/../x"), "call_1_fc____x");
        assert_eq!(file_stem(""), "cell");
        let dir = output_dir(None, "s/1");
        assert!(dir.ends_with("yusagent-outputs/s_1"));
    }

    #[tokio::test]
    async fn wrapped_tool_caps_its_text_result() {
        use crate::tools::tool_definition::ToolExecutionResult;
        let dir = tempfile::tempdir().unwrap();
        let mut definition = ToolDefinition {
            name: "t".into(),
            label: "t".into(),
            description: String::new(),
            prompt_snippet: String::new(),
            parameters: serde_json::json!({}),
            execution_mode: None,
            prepare_arguments: None,
            execute: Arc::new(|_: &str, _, _, _| {
                Box::pin(async { Ok(ToolExecutionResult::text("y".repeat(5000))) })
            }),
        };
        wrap_execute(
            &mut definition,
            OutputCap::new(500, dir.path().to_path_buf()),
        );
        let result = (*definition.execute)("call_9", serde_json::json!({}), None, None)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.chars().count() <= 500);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("call_9.txt")).unwrap(),
            "y".repeat(5000)
        );
    }
}
