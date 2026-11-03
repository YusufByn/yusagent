//! yusAgent (T03, YA-04; home of the YA-09 environment snapshot): the session facts that vary
//! between sessions leave the system prompt and ride an `<environment>` block at the end of a
//! user turn. Listed in `docs/fork-changes.md`.
//!
//! The blocks are derived at the LLM boundary from the converted conversation, never stored:
//! - the first user turn of the context carries the date (UTC, from that turn's own
//!   timestamp) and the working directory;
//! - a later user turn whose date differs from the last announced one carries the new date.
//!
//! A turn's block depends only on that turn and the turns before it, so every request repeats
//! the earlier blocks byte for byte. A session resumed another day gets the new date on its
//! next user turn, never in the system prompt nor in a turn already sent. Converted custom rows
//! (an `rlm` child's `[task from parent]` kickoff, notices) are user turns here too, so children
//! get the same treatment. After a compaction, the first user turn of the rebuilt context
//! carries the full block again.

use std::sync::Arc;

use pa_agent::agent_loop::ConvertToLlmFn;
use pa_agent::types::{Message, TextContent, UserContent, UserPart};

/// The block's opening line; the static prompt names it (`system_prompt.rs`).
pub const OPEN_TAG: &str = "<environment>";
const CLOSE_TAG: &str = "</environment>";

/// `convert` followed by [`with_environment_blocks`] for the session working directory.
#[must_use]
pub fn wrap_convert_to_llm(convert: ConvertToLlmFn, cwd: &std::path::Path) -> ConvertToLlmFn {
    let cwd = cwd.display().to_string().replace('\\', "/");
    Arc::new(move |messages| {
        let convert = Arc::clone(&convert);
        let cwd = cwd.clone();
        Box::pin(async move {
            let messages = convert(messages).await?;
            Ok(with_environment_blocks(messages, &cwd))
        })
    })
}

/// Appends the `<environment>` blocks to the user turns that carry one (module docs).
#[must_use]
pub fn with_environment_blocks(mut messages: Vec<Message>, cwd: &str) -> Vec<Message> {
    let mut first = true;
    let mut announced: Option<String> = None;
    for message in &mut messages {
        let Message::User(user) = message else {
            continue;
        };
        let mut lines = Vec::new();
        if let Some(date) = utc_date(user.timestamp)
            .filter(|date| first || announced.as_deref() != Some(date.as_str()))
        {
            lines.push(format!("Current date: {date}"));
            announced = Some(date);
        }
        if first {
            lines.push(format!("Working directory: {cwd}"));
            first = false;
        }
        if !lines.is_empty() {
            append_text(
                &mut user.content,
                format!("{OPEN_TAG}\n{}\n{CLOSE_TAG}", lines.join("\n")),
            );
        }
    }
    messages
}

/// The block goes in its own text part, after the turn's own content (left untouched).
fn append_text(content: &mut UserContent, text: String) {
    let block = UserPart::Text(TextContent {
        text,
        text_signature: None,
    });
    match content {
        UserContent::Text(own) => {
            let own = std::mem::take(own);
            *content = UserContent::Parts(vec![
                UserPart::Text(TextContent {
                    text: own,
                    text_signature: None,
                }),
                block,
            ]);
        }
        UserContent::Parts(parts) => parts.push(block),
    }
}

/// `YYYY-MM-DD` (UTC) of a Unix timestamp in milliseconds; `None` for a missing (non-positive)
/// timestamp.
fn utc_date(timestamp_ms: i64) -> Option<String> {
    if timestamp_ms <= 0 {
        return None;
    }
    let days = timestamp_ms / 86_400_000;
    // Civil-from-days algorithm (Howard Hinnant).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOV_3: i64 = 1_793_664_000_000;
    const NOV_4: i64 = 1_793_750_400_000;

    fn user(text: &str, timestamp: i64) -> Message {
        serde_json::from_value(json!({"role": "user", "content": text, "timestamp": timestamp}))
            .unwrap()
    }

    /// A non-user turn (the block skips it).
    fn tool_result(text: &str) -> Message {
        serde_json::from_value(json!({
            "role": "toolResult", "toolCallId": "call-1", "toolName": "python_repl",
            "content": [{"type": "text", "text": text}], "isError": false, "timestamp": NOV_3
        }))
        .unwrap()
    }

    fn texts(message: &Message) -> Vec<String> {
        let Message::User(user) = message else {
            return Vec::new();
        };
        match &user.content {
            UserContent::Text(text) => vec![text.clone()],
            UserContent::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    UserPart::Text(text) => Some(text.text.clone()),
                    UserPart::Image(_) => None,
                })
                .collect(),
        }
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc_date(NOV_3).as_deref(), Some("2026-11-03"));
        assert_eq!(utc_date(NOV_4 - 1).as_deref(), Some("2026-11-03"));
        assert_eq!(utc_date(951_782_400_000).as_deref(), Some("2000-02-29"));
        assert_eq!(utc_date(946_598_400_000).as_deref(), Some("1999-12-31"));
        assert_eq!(utc_date(0), None);
    }

    #[test]
    fn first_turn_gets_date_and_cwd_then_only_date_changes() {
        let messages = with_environment_blocks(
            vec![
                user("one", NOV_3),
                tool_result("a"),
                user("two", NOV_3 + 60_000),
                user("three", NOV_4 + 1),
            ],
            "/work",
        );
        assert_eq!(
            texts(&messages[0]),
            vec![
                "one".to_string(),
                "<environment>\nCurrent date: 2026-11-03\nWorking directory: /work\n</environment>"
                    .to_string()
            ]
        );
        assert_eq!(texts(&messages[2]), vec!["two".to_string()]);
        assert_eq!(
            texts(&messages[3]),
            vec![
                "three".to_string(),
                "<environment>\nCurrent date: 2026-11-04\n</environment>".to_string()
            ]
        );
        assert_eq!(messages[1], tool_result("a"));
    }

    #[test]
    fn a_missing_timestamp_still_gets_the_cwd() {
        let messages = with_environment_blocks(vec![user("one", 0), user("two", NOV_3)], "/w");
        assert_eq!(
            texts(&messages[0])[1],
            "<environment>\nWorking directory: /w\n</environment>"
        );
        assert_eq!(
            texts(&messages[1])[1],
            "<environment>\nCurrent date: 2026-11-03\n</environment>"
        );
    }

    /// The blocks of a conversation's prefix are the prefix of its blocks: a request never
    /// rewrites what an earlier request sent.
    #[test]
    fn blocks_are_prefix_stable() {
        let conversation = vec![
            user("one", NOV_3),
            tool_result("a"),
            user("two", NOV_4),
            tool_result("b"),
            user("three", NOV_4 + 5),
        ];
        let full = with_environment_blocks(conversation.clone(), "/w");
        for len in 0..=conversation.len() {
            let prefix = with_environment_blocks(conversation[..len].to_vec(), "/w");
            assert_eq!(prefix[..], full[..len]);
        }
    }

    #[tokio::test]
    async fn the_wrapper_runs_after_the_conversion() {
        let convert: ConvertToLlmFn = Arc::new(|_messages| {
            Box::pin(async move { Ok(vec![user("from the converter", NOV_3)]) })
        });
        let wrapped = wrap_convert_to_llm(convert, std::path::Path::new("/p"));
        let out = wrapped(Vec::new()).await.unwrap();
        assert_eq!(
            texts(&out[0])[1],
            "<environment>\nCurrent date: 2026-11-03\nWorking directory: /p\n</environment>"
        );
    }
}
