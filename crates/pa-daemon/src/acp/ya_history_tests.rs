//! yusAgent (YA-10, ticket 28): the replay, the list rows and the `saved` payload.

use super::*;
use serde_json::json;

fn user(text: &str) -> Value {
    json!({ "role": "user", "content": [{ "type": "text", "text": text }], "timestamp": 1 })
}

fn custom(kind: &str, text: &str) -> Value {
    json!({ "role": "custom", "customType": kind, "content": text, "display": true, "timestamp": 1 })
}

fn assistant(blocks: Value, stop: &str) -> Value {
    json!({ "role": "assistant", "content": blocks, "stopReason": stop, "timestamp": 1 })
}

fn cell(id: &str, code: &str) -> Value {
    json!({ "type": "toolCall", "id": id, "name": "python_repl", "arguments": { "code": code } })
}

fn result(id: &str, text: &str, details: Value) -> Value {
    json!({
        "role": "toolResult", "toolCallId": id, "toolName": "python_repl",
        "content": [{ "type": "text", "text": text }], "details": details,
        "isError": false, "timestamp": 1,
    })
}

/// Each update as its `sessionUpdate` tag (and the text of a chunk).
fn kinds(turn: &ReplayTurn) -> Vec<String> {
    turn.updates
        .iter()
        .map(|update| {
            let value = update.to_bare_value();
            let tag = value["sessionUpdate"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            match value["content"]["text"].as_str() {
                Some(text) => format!("{tag}:{text}"),
                None => tag,
            }
        })
        .collect()
}

fn end(state: &str) -> Option<TurnEndMeta> {
    Some(TurnEndMeta {
        state: state.to_string(),
        error: None,
    })
}

#[test]
fn a_prompt_turn_replays_its_prompt_reasoning_text_and_cells() {
    let messages = vec![
        user("Bonjour"),
        assistant(
            json!([{ "type": "thinking", "thinking": "je lance" }, cell("c1", "print(1)")]),
            "toolUse",
        ),
        result("c1", "1", json!({ "bashRuns": [] })),
        assistant(json!([{ "type": "text", "text": "Fini." }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].start, TurnStartMeta::new("prompt", None));
    assert_eq!(
        kinds(&turns[0]),
        vec![
            "user_message_chunk:Bonjour",
            "agent_thought_chunk:je lance",
            "tool_call",
            "tool_call_update",
            "agent_message_chunk:Fini.",
        ]
    );
    assert_eq!(turns[0].end, end("done"));
    // The cell as live: title, raw input, then its result with the output text.
    let call = turns[0].updates[2].to_bare_value();
    assert_eq!(call["title"], json!("Python cell"));
    assert_eq!(call["rawInput"], json!({ "code": "print(1)" }));
    let done = turns[0].updates[3].to_bare_value();
    assert_eq!(done["status"], json!("completed"));
    assert_eq!(done["content"][0]["content"]["text"], json!("1"));
}

#[test]
fn a_cell_result_carries_its_diffs_as_live() {
    let details = json!({ "diffs": [{
        "path": "/p/a.txt", "oldText": "un", "newText": "deux", "startLine": 1, "startColumn": 0,
    }] });
    let messages = vec![
        user("modifie"),
        assistant(
            json!([cell("c1", "await edit('a.txt', 'un', 'deux')")]),
            "toolUse",
        ),
        result("c1", "ok", details.clone()),
        assistant(json!([{ "type": "text", "text": "ok" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    let live = wire_events::wire_updates(
        &json!({ "type": "tool_execution_end", "toolCallId": "c1", "isError": false,
                 "result": { "content": [{ "type": "text", "text": "ok" }], "details": details } }),
        &mut WireMappingState::default(),
    );
    assert_eq!(turns[0].updates[2], live[0]);
}

#[test]
fn a_bash_done_notice_after_a_turn_starts_a_wake() {
    let messages = vec![
        user("lance"),
        assistant(json!([{ "type": "text", "text": "lancé" }]), "stop"),
        custom("async_bash_completion", "[bash-done pid:1] fini"),
        assistant(json!([{ "type": "text", "text": "le job a fini" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[1].start, TurnStartMeta::new("wake", Some("bash")));
    assert_eq!(kinds(&turns[1]), vec!["agent_message_chunk:le job a fini"]);
    assert_eq!(turns[1].end, end("done"));
}

#[test]
fn a_notice_that_rides_a_prompt_belongs_to_it() {
    let messages = vec![
        user("un"),
        assistant(json!([{ "type": "text", "text": "a" }]), "stop"),
        custom("ipython_state_restored", "[python-state-restored]"),
        user("deux"),
        assistant(json!([{ "type": "text", "text": "b" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    assert_eq!(turns.len(), 2, "no empty wake turn for the notice");
    assert_eq!(turns[1].start.origin, "prompt");
    assert_eq!(kinds(&turns[1])[0], "user_message_chunk:deux");
}

#[test]
fn a_notice_inside_a_running_turn_is_not_a_turn() {
    let messages = vec![
        user("un"),
        assistant(json!([cell("c1", "x")]), "toolUse"),
        custom("async_bash_completion", "[bash-done]"),
        result("c1", "ok", Value::Null),
        assistant(json!([{ "type": "text", "text": "fin" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].end, end("done"));
}

#[test]
fn how_a_turn_ended() {
    let stopped = replay_turns(
        &[
            user("a"),
            assistant(json!([{ "type": "text", "text": "x" }]), "aborted"),
        ],
        false,
    );
    assert_eq!(stopped[0].end, end("cancelled"));
    let mut failed = assistant(json!([]), "error");
    failed["errorMessage"] = json!("rate limited");
    let failed = replay_turns(&[user("a"), failed], false);
    assert_eq!(
        failed[0].end,
        Some(TurnEndMeta {
            state: "failed".to_string(),
            error: Some("rate limited".to_string()),
        })
    );
    // Cut while it ran (a tool call without its answer, a prompt without a reply).
    let cut = replay_turns(
        &[user("a"), assistant(json!([cell("c1", "x")]), "toolUse")],
        false,
    );
    assert_eq!(cut[0].end, end("cancelled"));
    let unanswered = replay_turns(&[user("a")], false);
    assert_eq!(unanswered[0].end, end("cancelled"));
}

#[test]
fn a_turn_the_worker_still_runs_stays_open() {
    let messages = vec![
        user("a"),
        assistant(json!([{ "type": "text", "text": "x" }]), "stop"),
        user("b"),
        assistant(json!([cell("c1", "x")]), "toolUse"),
    ];
    let turns = replay_turns(&messages, true);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].end, end("done"));
    assert_eq!(turns[1].end, None);
    // A wake whose notice is the last row: its turn is the running one.
    let waking = replay_turns(
        &[
            user("a"),
            assistant(json!([{ "type": "text", "text": "x" }]), "stop"),
            custom("async_bash_completion", "[bash-done]"),
        ],
        true,
    );
    assert_eq!(waking.len(), 2);
    assert_eq!(waking[1].start, TurnStartMeta::new("wake", Some("bash")));
    assert_eq!(waking[1].end, None);
}

#[test]
fn a_message_sent_into_a_running_turn_ends_it_as_done() {
    let messages = vec![
        user("a"),
        assistant(json!([cell("c1", "x")]), "toolUse"),
        result("c1", "ok", Value::Null),
        user("plutôt ça"),
        assistant(json!([{ "type": "text", "text": "ok" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].end, end("done"));
    assert_eq!(turns[1].start.origin, "prompt");
}

#[test]
fn a_quota_resume_is_a_wake_without_a_prompt() {
    let quota = user(pa_core::session_engine::provider_park::QUOTA_RESUME_MARKER_TEXT);
    let turns = replay_turns(
        &[
            quota,
            assistant(json!([{ "type": "text", "text": "repris" }]), "stop"),
        ],
        false,
    );
    assert_eq!(turns[0].start, TurnStartMeta::new("wake", Some("quota")));
    assert_eq!(kinds(&turns[0]), vec!["agent_message_chunk:repris"]);
}

#[test]
fn a_compaction_rides_its_turn() {
    let messages = vec![
        user("compacte"),
        json!({ "role": "compactionSummary", "summary": "résumé", "tokensBefore": 1234,
                "timestamp": 1 }),
        assistant(json!([{ "type": "text", "text": "ok" }]), "stop"),
    ];
    let turns = replay_turns(&messages, false);
    let compaction = turns[0].updates[1].to_bare_value();
    assert_eq!(
        compaction["_meta"][meta::PRIME_AGENT_META_NAMESPACE]["compaction"],
        json!({ "tokensBefore": 1234, "summary": "résumé" })
    );
}

#[test]
fn a_prompt_replays_its_text_and_image_blocks() {
    let message = json!({ "role": "user", "timestamp": 1, "content": [
        { "type": "text", "text": "regarde" },
        { "type": "image", "data": "aGk=", "mimeType": "image/png" },
    ] });
    let turns = replay_turns(&[message], false);
    let blocks: Vec<Value> = turns[0]
        .updates
        .iter()
        .map(|update| update.to_bare_value()["content"].clone())
        .collect();
    assert_eq!(
        blocks,
        vec![
            json!({ "type": "text", "text": "regarde" }),
            json!({ "type": "image", "data": "aGk=", "mimeType": "image/png" }),
        ]
    );
}

fn row(id: &str, modified: &str, count: u64) -> Value {
    json!({
        "id": id, "path": format!("/s/{id}.jsonl"), "cwd": "/p",
        "messageCount": count, "modified": modified, "firstMessage": format!("prompt {id}"),
        "rlmDepth": 0,
    })
}

#[test]
fn the_list_keeps_conversations_newest_first() {
    let mut child = row("child", "2026-11-22T10:00:00.000Z", 3);
    child["parentSessionPath"] = json!("/s/a.jsonl");
    child["rlmDepth"] = json!(1);
    let rows = vec![
        row("a", "2026-11-22T08:00:00.000Z", 4),
        row("draft", "2026-11-22T11:00:00.000Z", 0),
        child,
        row("b", "2026-11-22T09:00:00.000Z", 2),
    ];
    let listed = list_rows(&rows, Some("b"));
    let ids: Vec<&str> = listed
        .iter()
        .map(|row| row["sessionId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["b", "a"]);
    assert_eq!(
        listed[0],
        json!({
            "sessionId": "b", "cwd": "/p", "title": "prompt b",
            "updatedAt": "2026-11-22T09:00:00.000Z",
            "_meta": { "ai.primeintellect.prime-agent": { "path": "/s/b.jsonl", "current": true } },
        })
    );
    assert!(listed[1]["_meta"]["ai.primeintellect.prime-agent"]
        .get("current")
        .is_none());
}

#[test]
fn a_title_is_folded_and_cut_at_80_characters() {
    let mut long = row("a", "2026-11-22T08:00:00.000Z", 1);
    long["firstMessage"] = json!(format!("  un\n\tdeux {}", "x".repeat(100)));
    let title = list_rows(&[long], None)[0]["title"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(title.starts_with("un deux x"), "{title}");
    assert_eq!(title.chars().count(), 80);
    assert!(title.ends_with('…'));
    let mut named = row("b", "2026-11-22T08:00:00.000Z", 1);
    named["name"] = json!("Mon nom");
    assert_eq!(list_rows(&[named], None)[0]["title"], json!("Mon nom"));
}

#[test]
fn the_file_rides_the_response_meta() {
    let file = SessionFile::from_summary(Some(&json!({
        "sessionId": "019a", "sessionFile": "/s/019a.jsonl", "activeSessionId": "x",
    })));
    let mut result = json!({ "sessionId": "acp-1", "configOptions": [] });
    add_saved(&mut result, &file);
    assert_eq!(
        result["_meta"],
        json!({ "ai.primeintellect.prime-agent": {
            "saved": { "sessionId": "019a", "path": "/s/019a.jsonl" } } })
    );
    // Beside the cwd report.
    let mut beside = json!({ "_meta": { "ai.primeintellect.prime-agent": {
        "cwd": { "requested": "/a", "actual": "/b" } } } });
    add_saved(&mut beside, &file);
    assert_eq!(
        beside["_meta"]["ai.primeintellect.prime-agent"]["cwd"]["actual"],
        json!("/b")
    );
    // No file id: nothing.
    let mut bare = json!({});
    add_saved(&mut bare, &SessionFile::from_summary(None));
    assert_eq!(bare, json!({}));
}

#[tokio::test]
async fn the_replay_publishes_each_turn_between_its_start_and_end() {
    let (sink, mut frames) = tokio::sync::mpsc::unbounded_channel();
    let producer = UpdateProducer::new("019a", sink);
    producer.commit_session_new_response().await;
    let turns = replay_turns(
        &[
            user("a"),
            assistant(json!([{ "type": "text", "text": "x" }]), "stop"),
            user("b"),
        ],
        true,
    );
    let open = publish(turns, &producer).await;
    assert_eq!(open, Some(2));
    assert_eq!(
        producer.active_prompt_turn().await,
        2,
        "the open turn stays active"
    );
    let mut seen = Vec::new();
    while let Ok(frame) = frames.try_recv() {
        let update = &frame["params"]["update"];
        let ours = &update["_meta"]["ai.primeintellect.prime-agent"];
        assert_eq!(frame["params"]["sessionId"], json!("019a"));
        let what = if ours.get("turnStart").is_some() {
            "start".to_string()
        } else if ours.get("turnEnd").is_some() {
            format!("end:{}", ours["turnEnd"]["state"].as_str().unwrap())
        } else {
            update["sessionUpdate"].as_str().unwrap().to_string()
        };
        seen.push(format!("{}:{what}", ours["promptTurnId"]));
    }
    assert_eq!(
        seen,
        vec![
            "1:start",
            "1:user_message_chunk",
            "1:agent_message_chunk",
            "1:end:done",
            "2:start",
            "2:user_message_chunk",
        ]
    );
}
