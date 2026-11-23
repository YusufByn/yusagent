//! yusAgent (YA-10, ticket 31): the bridge's notice of an announcement row
//! (`pa_core::session_engine::ya_resource_notice`, custom type `ya-resources-changed`): a
//! `session_info_update` whose `_meta["_yus/notice"]` carries `{kind: "resources", details}`.
//! Published live by the producer like `turnStart` (the bridge's event loop, `daemon.rs`), in the
//! order of the turn's updates, and replayed by `session/load` before the prompt it came with
//! (`ya_history`). The extension shows it as a line of the chat
//! (`docs/tickets/31-skills-mcp-reglages.md`, §3.2). Listed in `docs/fork-changes.md`.

use pa_core::session_engine::ya_resource_notice::CUSTOM_TYPE;
use serde_json::{json, Value};

use super::types::AcpSessionUpdate;

/// The `_meta` key of the notice.
pub(crate) const META_KEY: &str = "_yus/notice";

/// The notice of a custom row, when it is an announcement.
pub(crate) fn of_row(message: &Value) -> Option<AcpSessionUpdate> {
    (message.get("customType").and_then(Value::as_str) == Some(CUSTOM_TYPE)).then(|| {
        AcpSessionUpdate::SessionInfoUpdate {
            meta: json!({ META_KEY: {
                "kind": "resources",
                "details": message.get("details").cloned().unwrap_or(Value::Null),
            } }),
        }
    })
}

/// The notice of a worker event: the `message_start` of an announcement row.
pub(crate) fn updates(event: &Value) -> Vec<AcpSessionUpdate> {
    if event.get("type").and_then(Value::as_str) != Some("message_start") {
        return Vec::new();
    }
    event.get("message").and_then(of_row).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_announcement_row_gives_its_notice_and_another_row_none() {
        let details = json!({ "skills": { "added": ["deploy"], "changed": [], "removed": [] } });
        let event = json!({ "type": "message_start", "message": {
            "role": "custom", "customType": CUSTOM_TYPE, "content": "[skills-changed]",
            "display": true, "details": details,
        } });
        let updates = updates(&event);
        assert_eq!(updates.len(), 1);
        let AcpSessionUpdate::SessionInfoUpdate { meta } = &updates[0] else {
            panic!("a session_info_update: {updates:?}");
        };
        assert_eq!(
            meta,
            &json!({ META_KEY: { "kind": "resources", "details": details } })
        );
        let end = json!({ "type": "message_end", "message": event["message"].clone() });
        assert!(super::updates(&end).is_empty(), "message_start only");
        let other = json!({ "type": "message_start", "message": {
            "role": "custom", "customType": "async_bash_completion",
        } });
        assert!(super::updates(&other).is_empty());
    }
}
