//! Masking: off by default, frontier steps, prefix changes only at steps, deterministic
//! placeholders, guards, and the invalidation noted in the usage journal.

use super::*;

/// One cell of a scripted conversation.
#[derive(Clone)]
struct Cell {
    code: String,
    output: String,
    details: Value,
}

fn cell(output_chars: usize) -> Cell {
    Cell {
        code: "print('x' * 5000)".to_string(),
        output: "x".repeat(output_chars),
        details: json!({ "status": "ok" }),
    }
}

fn cells(count: usize) -> Vec<Cell> {
    (0..count).map(|_| cell(5000)).collect()
}

fn failed(output_chars: usize) -> Cell {
    Cell {
        details: json!({ "status": "error", "errorEname": "RuntimeError" }),
        ..cell(output_chars)
    }
}

fn with_code(mut cell: Cell, code: &str) -> Cell {
    cell.code = code.to_string();
    cell
}

fn with_details(mut cell: Cell, details: Value) -> Cell {
    cell.details = details;
    cell
}

fn message(value: Value) -> AgentMessage {
    serde_json::from_value(value).expect("agent message")
}

/// The context of request turn `cells.len() + 1`: a user prompt, then one assistant tool call
/// and its `python_repl` result per cell (call ids `c1`, `c2`, ...).
fn context(cells: &[Cell]) -> Vec<AgentMessage> {
    let mut messages = vec![message(
        json!({ "role": "user", "content": "go", "timestamp": 1 }),
    )];
    for (index, cell) in cells.iter().enumerate() {
        let id = format!("c{}", index + 1);
        messages.push(message(json!({
            "role": "assistant",
            "content": [{ "type": "toolCall", "id": id, "name": "python_repl",
                          "arguments": { "code": cell.code } }],
            "usage": {},
            "stopReason": "toolUse",
        })));
        messages.push(message(json!({
            "role": "toolResult",
            "toolCallId": id,
            "toolName": "python_repl",
            "content": [{ "type": "text", "text": cell.output }],
            "details": cell.details,
            "isError": cell.details["status"] == "error",
        })));
    }
    messages
}

fn bytes(messages: &[AgentMessage]) -> Vec<String> {
    messages
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect()
}

fn masked_ids(cells: &[Cell], config: &MaskingConfig) -> Vec<String> {
    apply(context(cells), config).1.masked.into_iter().collect()
}

fn ids(turns: &[usize]) -> Vec<String> {
    let mut ids: Vec<String> = turns.iter().map(|turn| format!("c{turn}")).collect();
    ids.sort();
    ids
}

#[test]
fn masking_is_off_unless_enabled() {
    assert_eq!(MaskingConfig::from_settings(None), None);
    assert_eq!(
        MaskingConfig::from_settings(Some(&MaskingSettings::default())),
        None
    );
    let off = MaskingSettings {
        enabled: Some(false),
        window: Some(2),
        ..MaskingSettings::default()
    };
    assert_eq!(MaskingConfig::from_settings(Some(&off)), None);
    let on = MaskingSettings {
        enabled: Some(true),
        ..MaskingSettings::default()
    };
    assert_eq!(
        MaskingConfig::from_settings(Some(&on)),
        Some(MaskingConfig {
            window: 10,
            step: 5,
            min_chars: 500
        })
    );
    let zero_step = MaskingSettings {
        enabled: Some(true),
        step: Some(0),
        ..MaskingSettings::default()
    };
    assert_eq!(
        MaskingConfig::from_settings(Some(&zero_step)).map(|config| config.step),
        Some(1)
    );
}

#[test]
fn the_settings_block_reads_camel_case() {
    let settings: crate::settings::types::Settings = serde_json::from_value(json!({
        "masking": { "enabled": true, "window": 4, "step": 2, "minChars": 10 }
    }))
    .unwrap();
    assert_eq!(
        MaskingConfig::from_settings(settings.masking.as_ref()),
        Some(MaskingConfig {
            window: 4,
            step: 2,
            min_chars: 10
        })
    );
}

#[tokio::test]
async fn disabled_the_transform_returns_the_same_bytes() {
    let cells = cells(30);
    let messages = context(&cells);
    let before = bytes(&messages);
    let transform: TransformContextFn = transform_context(None, None);
    let after = transform(messages, pa_agent::abort::AbortSignal::default())
        .await
        .unwrap();
    assert_eq!(bytes(&after), before);
}

#[test]
fn the_frontier_moves_every_step_once_past_the_window() {
    let config = MaskingConfig::default();
    let frontiers: Vec<usize> = (1..=31).map(|turn| config.frontier(turn)).collect();
    assert!(frontiers[..15].iter().all(|&frontier| frontier == 0));
    assert_eq!(frontiers[15..20], [5, 5, 5, 5, 5]);
    assert_eq!(frontiers[20..25], [10, 10, 10, 10, 10]);
    assert_eq!(frontiers[25..30], [15, 15, 15, 15, 15]);
    assert_eq!(frontiers[30], 20);
}

/// The capture case of the ticket: 25 cells of about 5 000 characters, an error at turn 12.
fn bench_cells() -> Vec<Cell> {
    (1..=25)
        .map(|turn| if turn == 12 { failed(5000) } else { cell(5000) })
        .collect()
}

#[test]
fn the_prefix_changes_only_at_steps() {
    let config = MaskingConfig::default();
    // A second error between two steps (turn 28) must not move anything before the next step.
    let mut cells = bench_cells();
    cells.extend((26..=34).map(|turn| if turn == 28 { failed(5000) } else { cell(5000) }));
    let mut breaks = Vec::new();
    let mut previous: Option<Vec<String>> = None;
    for done in 0..=cells.len() {
        let (messages, report) = apply(context(&cells[..done]), &config);
        let current = bytes(&messages);
        if let Some(previous) = &previous {
            let kept = previous.len() <= current.len() && current[..previous.len()] == previous[..];
            assert_eq!(!kept, report.changed, "request turn {}", report.turn);
            if !kept {
                breaks.push(report.turn);
            }
        }
        previous = Some(current);
    }
    assert_eq!(breaks, [16, 21, 26, 31]);
}

#[test]
fn rebuilding_the_same_context_gives_the_same_bytes() {
    let config = MaskingConfig::default();
    let cells = bench_cells();
    let first = bytes(&apply(context(&cells), &config).0);
    let second = bytes(&apply(context(&cells), &config).0);
    assert_eq!(first, second);
}

#[test]
fn a_masked_output_becomes_the_placeholder_and_nothing_else_changes() {
    let config = MaskingConfig::default();
    let cells = cells(15);
    let original = context(&cells);
    let (messages, report) = apply(original.clone(), &config);
    assert_eq!(report.turn, 16);
    assert_eq!(report.frontier, 5);
    assert_eq!(
        report.masked.iter().cloned().collect::<Vec<_>>(),
        ids(&[1, 2, 3, 4, 5])
    );
    for (index, (before, after)) in original.iter().zip(&messages).enumerate() {
        let masked = matches!(after, AgentMessage::Standard(Message::ToolResult(result))
            if report.masked.contains(&result.tool_call_id));
        if !masked {
            assert_eq!(before, after, "message {index}");
            continue;
        }
        let AgentMessage::Standard(Message::ToolResult(result)) = after else {
            unreachable!()
        };
        let turn = index / 2;
        assert_eq!(
            result.content,
            vec![ToolResultContent::text(format!(
                "[sortie masquée : tour {turn}, 5000 caractères, relire avec \
                 from rlm.masked import tool_output; print(tool_output(\"c{turn}\"))]"
            ))]
        );
        let AgentMessage::Standard(Message::ToolResult(source)) = before else {
            unreachable!()
        };
        assert_eq!(result.details, source.details, "details stay as recorded");
    }
}

#[test]
fn short_outputs_and_images_are_never_masked() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[0] = cell(499);
    cells[1] = cell(500);
    let mut messages = context(&cells);
    if let AgentMessage::Standard(Message::ToolResult(result)) = &mut messages[6] {
        result
            .content
            .push(ToolResultContent::Image(pa_agent::types::ImageContent {
                data: "AAAA".to_string(),
                mime_type: "image/png".to_string(),
            }));
    }
    let masked: Vec<String> = apply(messages, &config).1.masked.into_iter().collect();
    assert_eq!(masked, ids(&[2, 4, 5]));
}

#[test]
fn the_last_error_stays_until_a_newer_one_exists_at_a_step() {
    let config = MaskingConfig::default();
    let cells = bench_cells();
    // Request 26 (frontier 15): turn 12 is the last error, kept.
    let masked = masked_ids(&cells, &config);
    assert!(!masked.contains(&"c12".to_string()));
    assert_eq!(masked.len(), 14);
    // A newer error at turn 22: masked from the step that sees it.
    let mut cells = bench_cells();
    cells[21] = failed(5000);
    assert!(masked_ids(&cells, &config).contains(&"c12".to_string()));
}

#[test]
fn a_bash_result_with_a_non_zero_exit_is_an_error() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[2] = with_details(
        cell(5000),
        json!({ "status": "ok", BASH_FAILED_EXIT_DETAIL: 2 }),
    );
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 2, 4, 5]));
}

#[test]
fn the_last_failed_test_run_stays_after_a_newer_error() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[1] = with_details(
        with_code(
            cell(5000),
            "r = await bash('uv run pytest -q')\nprint(r.output)",
        ),
        json!({ "status": "ok", BASH_FAILED_EXIT_DETAIL: 1 }),
    );
    cells[3] = failed(5000);
    // Turn 4 is the last error, turn 2 the last failed test run.
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 3, 5]));
}

#[test]
fn the_last_run_of_each_diagnostic_tool_stays() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[0] = with_code(cell(5000), "print((await bash('ruff check .')).output)");
    cells[2] = with_code(
        cell(5000),
        "print((await bash('uv run ruff check src')).output)",
    );
    cells[3] = with_code(cell(5000), "print((await bash('cargo clippy -q')).output)");
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 2, 5]));
}

#[test]
fn outputs_about_a_file_being_edited_stay() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[1] = with_code(cell(5000), "print(open('src/app.py').read())");
    cells[2] = with_code(
        cell(5000),
        "print((await bash('sed -n 1,80p other.py')).output)",
    );
    // No recent edit: both are masked.
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 2, 3, 4, 5]));
    // `src/app.py` edited at turn 14 (inside the window): its read stays.
    cells[13] = with_details(
        cell(5000),
        json!({ "status": "ok", "diffs": [{ "path": "/work/src/app.py", "diff": "" }] }),
    );
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 3, 4, 5]));
}

#[test]
fn the_last_output_about_the_plan_stays() {
    let config = MaskingConfig::default();
    let mut cells = cells(15);
    cells[0] = with_code(cell(5000), "print(open('PLAN.md').read())");
    cells[2] = with_code(cell(5000), "print(open('notes/plan.md').read())");
    cells[3] = with_code(cell(5000), "print(open('myplan.md').read())");
    assert_eq!(masked_ids(&cells, &config), ids(&[1, 2, 4, 5]));
}

#[test]
fn user_and_assistant_messages_and_other_tools_are_never_masked() {
    let config = MaskingConfig::default();
    let mut messages = context(&cells(15));
    if let AgentMessage::Standard(Message::ToolResult(result)) = &mut messages[2] {
        result.tool_name = "other_tool".to_string();
    }
    let (masked, report) = apply(messages.clone(), &config);
    assert!(!report.masked.contains("c1"));
    for (before, after) in messages.iter().zip(&masked) {
        if !matches!(before, AgentMessage::Standard(Message::ToolResult(_))) {
            assert_eq!(before, after);
        }
    }
}

#[tokio::test]
async fn a_step_that_masks_is_noted_in_the_usage_journal() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(UsageJournal::open(
        dir.path().join("s.usage.ndjson"),
        "s".to_string(),
        None,
    ));
    let transform = transform_context(Some(MaskingConfig::default()), Some(Arc::clone(&journal)));
    let cells = cells(16);
    for done in 14..=16 {
        transform(
            context(&cells[..done]),
            pa_agent::abort::AbortSignal::default(),
        )
        .await
        .unwrap();
        let noted = journal.take_invalidation();
        if done == 15 {
            assert_eq!(
                noted,
                Some(json!({ "cause": "masking", "turn": 16, "frontier": 5, "masked": 5 }))
            );
        } else {
            assert_eq!(noted, None, "request turn {}", done + 1);
        }
    }
}
