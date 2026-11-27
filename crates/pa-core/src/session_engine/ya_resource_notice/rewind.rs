//! yusAgent (YA-18, ticket 43): the rows a rewind of the conversation gives the model with the
//! next turn (`docs/tickets/43-retour-arriere.md`, §I.3, §L). As the announcement of this module
//! (ticket 31), they go to the engine's next-turn mailbox, which the next admission sends to the
//! model; the daemon's worker writes them in the session file with the turn's prefix rows. The
//! worker's own parked rows (`restore_next_turn`) are written to the file but never reach the
//! model's context: the first version of the ticket parked the fixed text there, and the model
//! never got it (`ya43_rewind_check.py`, C4).
//!
//! Declared in this fork module (option C of the ticket): no upstream line. Listed in
//! `docs/fork-changes.md`.

use pa_types::session::CustomMessage;

use super::super::AgentSession;

/// The custom type of the fixed text's row (D2).
pub const REWOUND_TYPE: &str = "ya-conversation-rewound";

impl AgentSession {
    /// yusAgent (YA-18, ticket 43): after a rewind, `rows` ride the next turn to the model, in
    /// order; a fixed text still waiting from an earlier rewind goes first (one fixed text).
    /// Push 5 (skeleton): queues nothing.
    pub fn ya_queue_rewind_rows(&self, rows: Vec<CustomMessage>) {
        let _ = (&self.pending_next_turn_rows, rows);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pa_agent::agent::{Agent, AgentInitialState, AgentOptions};
    use pa_agent::scripted::ScriptedProvider;
    use pa_types::session::CustomMessage;

    use super::super::super::{AgentSession, PromptOptions};
    use super::REWOUND_TYPE;
    use crate::session::manager::SessionManager;

    fn model() -> pa_agent::types::Model {
        serde_json::from_value(serde_json::json!({
            "id": "m", "name": "m", "api": "openai-completions", "provider": "test",
            "baseUrl": "http://localhost", "reasoning": false, "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 1000, "maxTokens": 100
        }))
        .unwrap()
    }

    fn row(custom_type: &str, text: &str) -> CustomMessage {
        CustomMessage {
            custom_type: custom_type.to_string(),
            content: pa_types::ai::UserContent::Text(text.to_string()),
            display: true,
            details: None,
            timestamp: 1,
            rest: serde_json::Map::default(),
        }
    }

    /// The rows queued after a rewind reach the model's next request, before the prompt; two
    /// rewinds in a row leave one fixed text.
    #[tokio::test]
    async fn the_rows_of_a_rewind_reach_the_next_request_once() {
        let provider = Arc::new(ScriptedProvider::new(model()));
        provider.push_text_turn("answered");
        let agent = Agent::new(AgentOptions {
            initial_state: AgentInitialState {
                model: Some(model()),
                ..Default::default()
            },
            stream_fn: Some(provider.stream_fn()),
            ..Default::default()
        });
        let tmp = tempfile::tempdir().unwrap();
        let session = AgentSession::new(
            Arc::new(agent),
            SessionManager::in_memory(tmp.path()),
            vec![],
        )
        .await
        .unwrap();
        session.ya_queue_rewind_rows(vec![row(REWOUND_TYPE, "REWOUND-1")]);
        session.ya_queue_rewind_rows(vec![
            row("ya-resources-changed", "ANNOUNCED"),
            row(REWOUND_TYPE, "REWOUND-2"),
        ]);
        session
            .prompt("THE-PROMPT", PromptOptions::default())
            .await
            .unwrap();
        let calls = provider.calls();
        assert_eq!(calls.len(), 1, "one request");
        let sent = serde_json::to_string(&calls[0].messages).unwrap();
        assert!(
            !sent.contains("REWOUND-1"),
            "the earlier fixed text went: {sent}"
        );
        assert_eq!(sent.matches("ANNOUNCED").count(), 1, "{sent}");
        assert_eq!(sent.matches("REWOUND-2").count(), 1, "{sent}");
        let at = |needle: &str| sent.find(needle).unwrap();
        assert!(
            at("ANNOUNCED") < at("REWOUND-2") && at("REWOUND-2") < at("THE-PROMPT"),
            "the announcement, the fixed text, then the prompt: {sent}"
        );
    }
}
