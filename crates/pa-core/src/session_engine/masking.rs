//! yusAgent (T05, YA-05): old `python_repl` outputs masked in the context sent to the model.
//! Off by default (`masking.enabled`); listed in `docs/fork-changes.md`.
//!
//! The transform runs on the loop's `transform_context` seam, so only the request changes: the
//! session JSONL keeps every output in full, and `rlm.masked.tool_output(<id>)` reads one back
//! from the kernel.
//!
//! - **Turns.** Turn `k` is the `k`-th assistant message of the context; its tool results belong
//!   to it. The request being built is turn `N` = assistant messages + 1.
//! - **Steps.** Outputs of turns `1..=F` are candidates, with the frontier
//!   `F = step * floor((N - window - 1) / step)`: every candidate is more than `window` turns
//!   old, and `F` only moves every `step` turns.
//! - **Frozen decisions.** The masked set for a frontier `F` is computed on the context as it
//!   stood when `F` was first reached (messages before assistant `F + window + 1`). Between two
//!   steps the set, hence the request prefix, does not change; at a step it changes at most once.
//! - **Determinism.** The placeholder depends only on recorded data (turn, size, tool call id):
//!   rebuilding the same context gives the same bytes.
//! - **Guards** (never masked): outputs under `minChars` characters, outputs with an image, the
//!   last failed output (Python exception, or a `bash()` result read with a non-zero exit code),
//!   the last failed test run, the last run of each diagnostic tool, outputs that touch a file
//!   edited in the last `window` turns, and the last output that touches a plan file.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use pa_agent::agent_loop::TransformContextFn;
use pa_agent::types::{AgentMessage, AssistantContent, Message, ToolResultContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::usage_log::UsageJournal;

#[cfg(test)]
mod tests;

pub const DEFAULT_WINDOW: u64 = 10;
pub const DEFAULT_STEP: u64 = 5;
pub const DEFAULT_MIN_CHARS: u64 = 500;

/// The `details` key `python_repl` sets on a cell that read a `bash()` result with a non-zero
/// exit code (`tools/ipython.rs`).
pub const BASH_FAILED_EXIT_DETAIL: &str = "bashFailedExit";

/// Commands that run tests: one token sequence each, matched on the cell code.
const TEST_COMMANDS: &[&[&str]] = &[
    &["pytest"],
    &["py.test"],
    &["unittest"],
    &["tox"],
    &["nox"],
    &["cargo", "test"],
    &["cargo", "nextest"],
    &["go", "test"],
    &["npm", "test"],
    &["npm", "run", "test"],
    &["pnpm", "test"],
    &["yarn", "test"],
    &["bun", "test"],
    &["deno", "test"],
    &["jest"],
    &["vitest"],
    &["mocha"],
    &["make", "test"],
    &["ctest"],
    &["mvn", "test"],
    &["gradle", "test"],
    &["rspec"],
    &["phpunit"],
];

/// Diagnostic tools (linters, type checkers): the last run of each stays visible.
const DIAGNOSTIC_COMMANDS: &[&[&str]] = &[
    &["ruff"],
    &["mypy"],
    &["pyright"],
    &["basedpyright"],
    &["flake8"],
    &["pylint"],
    &["eslint"],
    &["tsc"],
    &["biome"],
    &["cargo", "check"],
    &["cargo", "clippy"],
    &["go", "vet"],
    &["golangci-lint"],
    &["shellcheck"],
];

/// Plan files kept by the agent (file names, compared without case).
const PLAN_FILES: &[&str] = &["plan.md", "progress.md", "todo.md"];

/// The `masking` settings block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskingSettings {
    pub enabled: Option<bool>,
    /// Turns an output stays in full (default [`DEFAULT_WINDOW`]).
    pub window: Option<u64>,
    /// Turns between two moves of the frontier (default [`DEFAULT_STEP`], at least 1).
    pub step: Option<u64>,
    /// Outputs shorter than this are never masked (default [`DEFAULT_MIN_CHARS`]).
    pub min_chars: Option<u64>,
}

/// The resolved settings of an enabled masking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskingConfig {
    pub window: usize,
    pub step: usize,
    pub min_chars: usize,
}

impl Default for MaskingConfig {
    fn default() -> Self {
        Self::resolve(&MaskingSettings::default())
    }
}

impl MaskingConfig {
    /// `None` unless `masking.enabled` is `true`.
    #[must_use]
    pub fn from_settings(settings: Option<&MaskingSettings>) -> Option<Self> {
        settings
            .filter(|settings| settings.enabled == Some(true))
            .map(Self::resolve)
    }

    /// The sizes of `settings`, defaults filled in (the `enabled` flag is not read).
    fn resolve(settings: &MaskingSettings) -> Self {
        let size = |value: Option<u64>, default: u64| {
            usize::try_from(value.unwrap_or(default)).unwrap_or(usize::MAX)
        };
        Self {
            window: size(settings.window, DEFAULT_WINDOW),
            step: size(settings.step, DEFAULT_STEP).max(1),
            min_chars: size(settings.min_chars, DEFAULT_MIN_CHARS),
        }
    }

    /// The frontier for request turn `turn`: outputs of turns `1..=F` are candidates.
    #[must_use]
    pub fn frontier(&self, turn: usize) -> usize {
        let oldest_allowed = turn.saturating_sub(self.window.saturating_add(1));
        oldest_allowed / self.step * self.step
    }
}

/// What one transform did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskReport {
    /// The request turn `N`.
    pub turn: usize,
    pub frontier: usize,
    /// Tool call ids of the masked outputs.
    pub masked: BTreeSet<String>,
    /// The masked set differs from the previous request's: the prefix breaks here.
    pub changed: bool,
}

/// The loop's `transform_context`: `masking` applied when enabled, the messages untouched
/// otherwise. A request whose masked set changed is noted in the usage journal as an
/// invalidation.
#[must_use]
pub fn transform_context(
    config: Option<MaskingConfig>,
    journal: Option<Arc<UsageJournal>>,
) -> TransformContextFn {
    let Some(config) = config else {
        return super::request_timing::pass_through_transform();
    };
    Arc::new(move |messages, _signal| {
        let journal = journal.clone();
        Box::pin(async move {
            let (messages, report) = apply(messages, &config);
            if report.changed {
                if let Some(journal) = &journal {
                    journal.note_invalidation(invalidation_json(&report));
                }
            }
            Ok(messages)
        })
    })
}

/// The `invalidation` field of the usage journal line.
#[must_use]
pub fn invalidation_json(report: &MaskReport) -> Value {
    json!({
        "cause": "masking",
        "turn": report.turn,
        "frontier": report.frontier,
        "masked": report.masked.len(),
    })
}

/// Masks the outputs selected for the current request turn.
#[must_use]
pub fn apply(
    mut messages: Vec<AgentMessage>,
    config: &MaskingConfig,
) -> (Vec<AgentMessage>, MaskReport) {
    let turns = turn_numbers(&messages);
    let turn = messages.iter().filter(|m| is_assistant(m)).count() + 1;
    let frontier = config.frontier(turn);
    let selected = selection(&messages, &turns, config, frontier);
    let previous_frontier = config.frontier(turn.saturating_sub(1));
    let changed = previous_frontier != frontier
        && selection(&messages, &turns, config, previous_frontier)
            .keys()
            .ne(selected.keys());
    for (call_id, index) in &selected {
        if let AgentMessage::Standard(Message::ToolResult(result)) = &mut messages[*index] {
            let text = placeholder(turns[*index], text_chars(&result.content), call_id);
            result.content = vec![ToolResultContent::text(text)];
        }
    }
    let report = MaskReport {
        turn,
        frontier,
        masked: selected.into_keys().collect(),
        changed,
    };
    (messages, report)
}

/// `[sortie masquée : tour <k>, <n> caractères, relire avec <appel kernel>]`.
#[must_use]
pub fn placeholder(turn: usize, chars: usize, call_id: &str) -> String {
    let id = serde_json::to_string(call_id).unwrap_or_default();
    format!(
        "[sortie masquée : tour {turn}, {chars} caractères, relire avec \
         from rlm.masked import tool_output; print(tool_output({id}))]"
    )
}

/// The turn of each message: the number of assistant messages up to and including it.
fn turn_numbers(messages: &[AgentMessage]) -> Vec<usize> {
    let mut turn = 0;
    messages
        .iter()
        .map(|message| {
            if is_assistant(message) {
                turn += 1;
            }
            turn
        })
        .collect()
}

fn is_assistant(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Standard(Message::Assistant(_)))
}

/// One `python_repl` output of the context.
struct Output<'a> {
    index: usize,
    turn: usize,
    call_id: &'a str,
    chars: usize,
    has_image: bool,
    failed: bool,
    code: &'a str,
    edited: Vec<&'a str>,
}

/// The outputs masked for `frontier`, by tool call id (message index as value), decided on the
/// context as it stood when `frontier` was first reached.
fn selection(
    messages: &[AgentMessage],
    turns: &[usize],
    config: &MaskingConfig,
    frontier: usize,
) -> BTreeMap<String, usize> {
    if frontier == 0 {
        return BTreeMap::new();
    }
    let horizon = frontier.saturating_add(config.window);
    let outputs = outputs(messages, turns, horizon);
    let guarded = guarded(&outputs, frontier);
    outputs
        .iter()
        .filter(|output| {
            output.turn <= frontier
                && !output.has_image
                && output.chars >= config.min_chars
                && !guarded.contains(&output.index)
        })
        .map(|output| (output.call_id.to_string(), output.index))
        .collect()
}

/// The `python_repl` outputs of turns `1..=horizon`, with the code of their call.
fn outputs<'a>(messages: &'a [AgentMessage], turns: &[usize], horizon: usize) -> Vec<Output<'a>> {
    let mut code_by_call: HashMap<&str, &str> = HashMap::new();
    let mut out = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if turns[index] > horizon {
            break;
        }
        match message {
            AgentMessage::Standard(Message::Assistant(assistant)) => {
                for content in &assistant.content {
                    if let AssistantContent::ToolCall(call) = content {
                        let code = call.arguments.get("code").and_then(Value::as_str);
                        code_by_call.insert(call.id.as_str(), code.unwrap_or_default());
                    }
                }
            }
            AgentMessage::Standard(Message::ToolResult(result))
                if result.tool_name == pa_types::ai::PYTHON_TOOL_NAME =>
            {
                let details = result.details.as_ref();
                let status = details
                    .and_then(|d| d.get("status"))
                    .and_then(Value::as_str);
                let failed = status.map_or(result.is_error, |status| status == "error")
                    || details.is_some_and(|d| d.get(BASH_FAILED_EXIT_DETAIL).is_some());
                let edited = details
                    .and_then(|d| d.get("diffs"))
                    .and_then(Value::as_array)
                    .map(|diffs| {
                        diffs
                            .iter()
                            .filter_map(|diff| diff.get("path").and_then(Value::as_str))
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(Output {
                    index,
                    turn: turns[index],
                    call_id: result.tool_call_id.as_str(),
                    chars: text_chars(&result.content),
                    has_image: result
                        .content
                        .iter()
                        .any(|block| matches!(block, ToolResultContent::Image(_))),
                    failed,
                    code: code_by_call
                        .get(result.tool_call_id.as_str())
                        .copied()
                        .unwrap_or_default(),
                    edited,
                });
            }
            _ => {}
        }
    }
    out
}

/// Message indices of the guarded outputs (module docs), among `outputs`.
fn guarded(outputs: &[Output<'_>], frontier: usize) -> HashSet<usize> {
    let mut keep = HashSet::new();
    // The last failed output, and the last failed test run.
    if let Some(output) = outputs.iter().rev().find(|output| output.failed) {
        keep.insert(output.index);
    }
    if let Some(output) = outputs
        .iter()
        .rev()
        .find(|output| output.failed && runs_any(output.code, TEST_COMMANDS))
    {
        keep.insert(output.index);
    }
    // The last run of each diagnostic tool.
    for command in DIAGNOSTIC_COMMANDS {
        if let Some(output) = outputs
            .iter()
            .rev()
            .find(|output| runs_any(output.code, std::slice::from_ref(command)))
        {
            keep.insert(output.index);
        }
    }
    // Files edited in the recent turns: every output that touches one.
    let editing: Vec<&str> = outputs
        .iter()
        .filter(|output| output.turn > frontier)
        .flat_map(|output| output.edited.iter().copied())
        .collect();
    for output in outputs {
        if editing.iter().any(|path| output.touches(path)) {
            keep.insert(output.index);
        }
    }
    // The last output that touches each plan file.
    for plan in PLAN_FILES {
        if let Some(output) = outputs
            .iter()
            .rev()
            .find(|output| mentions_file(output.code, plan, true) || output.edits_file(plan))
        {
            keep.insert(output.index);
        }
    }
    keep
}

impl Output<'_> {
    /// The cell names `path` (or its file name) in its code, or edited it.
    fn touches(&self, path: &str) -> bool {
        self.edited.contains(&path)
            || self.code.contains(path)
            || file_name(path).is_some_and(|name| mentions_file(self.code, name, false))
    }

    /// The cell edited a file named `name` (compared without case).
    fn edits_file(&self, name: &str) -> bool {
        self.edited
            .iter()
            .any(|path| file_name(path).is_some_and(|file| file.eq_ignore_ascii_case(name)))
    }
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\')
}

/// The last path component, when it is long enough to be a meaningful match.
fn file_name(path: &str) -> Option<&str> {
    let name = path.rsplit(is_separator).next()?;
    (name.chars().count() >= 3).then_some(name)
}

/// `code` names the file `name` as a whole token (`plan.md`, not `myplan.md`).
fn mentions_file(code: &str, name: &str, ignore_case: bool) -> bool {
    tokens(code).any(|token| {
        let token = token.rsplit(is_separator).next().unwrap_or(token);
        if ignore_case {
            token.eq_ignore_ascii_case(name)
        } else {
            token == name
        }
    })
}

/// The words of `code`: letters, digits, and `_ - . / \`.
fn tokens(code: &str) -> impl Iterator<Item = &str> {
    code.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')))
        .filter(|token| !token.is_empty())
}

/// `code` contains one of `commands` as consecutive words.
fn runs_any(code: &str, commands: &[&[&str]]) -> bool {
    let words: Vec<&str> = code
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .filter(|word| !word.is_empty())
        .collect();
    commands.iter().any(|command| {
        words
            .windows(command.len())
            .any(|window| window == *command)
    })
}

/// Characters of the text blocks, joined by `\n` (as `rlm.masked.tool_output` returns them).
fn text_chars(content: &[ToolResultContent]) -> usize {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|block| match block {
            ToolResultContent::Text(text) => Some(text.text.as_str()),
            ToolResultContent::Image(_) => None,
        })
        .collect();
    texts.iter().map(|text| text.chars().count()).sum::<usize>() + texts.len().saturating_sub(1)
}
