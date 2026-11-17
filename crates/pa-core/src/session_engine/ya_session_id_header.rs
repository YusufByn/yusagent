//! yusAgent (YA-09, ticket 14, option A): the `session-id` request header on the `ChatGPT`
//! subscription provider (`openai-codex`), behind the `codexSessionIdHeader` setting, **off by
//! default** until the A/B decides (`bench/smoke/ab/ya09-session-id-plan.md`).
//!
//! Codex CLI sends `session-id` (hyphen) with the session id, on the WebSocket handshake and on
//! the HTTP request (openai/codex `rust-v0.169.0`, `d32063f96c2d`,
//! `codex-rs/codex-api/src/requests/headers.rs:5-14`), and says, `codex-rs/core/src/client.rs:734`:
//! "`ChatGPT` derives cache affinity from the Responses session-id header." Upstream (and
//! yusAgent without the setting) sends `session_id` (underscore) and `x-client-request-id`
//! (`pa_ai::providers::openai_codex_responses::request`, `build_sse_headers`,
//! `build_websocket_headers`). On, `session-id` is added next to them, with the same value: the
//! session id, which is also the body's `prompt_cache_key`, as in Codex CLI for a root session.
//!
//! Only the `openai-codex` provider on its `openai-codex-responses` API gets the header: not the
//! `OpenAI` API (key), not Azure, not Anthropic, not any other provider on a compatible API. The
//! header rides the request headers of the stream options, which the provider adapter merges
//! into the provider's (`provider_adapter::stream_once`) and the Codex provider sends on the
//! WebSocket handshake and on the SSE request: the provider layer is not touched. The request
//! body, the system prompt and the tools are unchanged.
//!
//! Frozen per session in `yaRequest` (`ya_request::FrozenRequest::codex_session_id_header`),
//! always written (`true` or `false`), so a run's session header says which arm it ran. Listed in
//! `docs/fork-changes.md`.

use std::collections::BTreeMap;
use std::sync::Arc;

use pa_agent::stream::{StreamFn, StreamRequestOptions};
use pa_agent::types::Model;

/// The header Codex CLI sends for cache affinity (hyphen; upstream sends `session_id`).
pub const SESSION_ID_HEADER: &str = "session-id";
/// The `ChatGPT` subscription provider ("OAuth abonnement pour provider").
pub const CHATGPT_PROVIDER: &str = "openai-codex";
/// Its API.
pub const CHATGPT_API: &str = "openai-codex-responses";

/// Whether `model` is served by the `ChatGPT` subscription provider.
#[must_use]
pub fn is_chatgpt(model: &Model) -> bool {
    model.provider == CHATGPT_PROVIDER && model.api == CHATGPT_API
}

/// Adds [`SESSION_ID_HEADER`] (the session id) to the request headers of `options` when `model`
/// is served by the `ChatGPT` provider and the request carries a session id; leaves `options` as
/// is otherwise. Headers already set are kept.
pub fn add_session_id_header(model: &Model, options: &mut StreamRequestOptions) {
    if !is_chatgpt(model) {
        return;
    }
    let Some(session_id) = options.session_id.clone() else {
        return;
    };
    options
        .headers
        .get_or_insert_with(BTreeMap::new)
        .insert(SESSION_ID_HEADER.to_string(), session_id);
}

/// `inner` with [`add_session_id_header`] applied to each request when `enabled`; `inner` itself
/// otherwise: the requests are those of upstream.
#[must_use]
pub fn scoped_stream_fn(inner: StreamFn, enabled: bool) -> StreamFn {
    if !enabled {
        return inner;
    }
    Arc::new(move |model, context, mut options| {
        add_session_id_header(&model, &mut options);
        inner(model, context, options)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider: &str, api: &str) -> Model {
        Model {
            provider: provider.to_string(),
            api: api.to_string(),
            ..Model::unknown()
        }
    }

    fn options(session_id: Option<&str>) -> StreamRequestOptions {
        StreamRequestOptions {
            session_id: session_id.map(str::to_string),
            ..StreamRequestOptions::default()
        }
    }

    #[test]
    fn the_chatgpt_provider_gets_the_session_id() {
        let mut sent = options(Some("sess-1"));
        add_session_id_header(&model(CHATGPT_PROVIDER, CHATGPT_API), &mut sent);
        let headers = sent.headers.expect("headers");
        assert_eq!(
            headers.get("session-id").map(String::as_str),
            Some("sess-1")
        );
        assert_eq!(headers.len(), 1, "{headers:?}");
    }

    #[test]
    fn other_providers_are_left_as_is() {
        for (provider, api) in [
            ("openai", "openai-responses"),
            ("openai", "openai-completions"),
            ("azure-openai-responses", "azure-openai-responses"),
            ("anthropic", "anthropic-messages"),
            ("prime", "openai-completions"),
            ("openrouter", "openai-completions"),
            // Another provider on the Codex API, and the Codex provider on another API.
            ("custom-codex", CHATGPT_API),
            (CHATGPT_PROVIDER, "openai-responses"),
        ] {
            let mut sent = options(Some("sess-1"));
            add_session_id_header(&model(provider, api), &mut sent);
            assert_eq!(sent.headers, None, "{provider} / {api}");
        }
    }

    #[test]
    fn existing_headers_are_kept_and_no_session_means_no_header() {
        let mut sent = options(Some("sess-1"));
        sent.headers = Some(BTreeMap::from([(
            "Idempotency-Key".to_string(),
            "req-1".to_string(),
        )]));
        add_session_id_header(&model(CHATGPT_PROVIDER, CHATGPT_API), &mut sent);
        let headers = sent.headers.expect("headers");
        assert_eq!(
            headers.get("Idempotency-Key").map(String::as_str),
            Some("req-1")
        );
        assert_eq!(
            headers.get("session-id").map(String::as_str),
            Some("sess-1")
        );

        let mut sent = options(None);
        add_session_id_header(&model(CHATGPT_PROVIDER, CHATGPT_API), &mut sent);
        assert_eq!(sent.headers, None);
    }

    type Headers = Option<BTreeMap<String, String>>;

    /// The headers the wrapped stream function hands to the provider adapter.
    async fn sent_headers(enabled: bool, model: Model) -> Headers {
        let seen: Arc<std::sync::Mutex<Vec<Headers>>> = Arc::default();
        let observer = Arc::clone(&seen);
        let inner: StreamFn = Arc::new(move |_model, _context, options| {
            observer.lock().unwrap().push(options.headers);
            Box::pin(async move { Err(anyhow::anyhow!("recorded")) })
        });
        let wrapped = scoped_stream_fn(inner, enabled);
        let Err(error) = wrapped(
            model,
            pa_agent::stream::LlmContext::default(),
            options(Some("sess-1")),
        )
        .await
        else {
            panic!("the recording stream fn fails");
        };
        assert_eq!(error.to_string(), "recorded");
        let mut seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        seen.pop().expect("one request")
    }

    /// On: the `ChatGPT` provider gets the header, the others do not. Off (the default): no
    /// header, and the stream function is the inner one itself.
    #[tokio::test]
    async fn the_setting_switches_the_header() {
        let on = sent_headers(true, model(CHATGPT_PROVIDER, CHATGPT_API)).await;
        assert_eq!(
            on.and_then(|headers| headers.get(SESSION_ID_HEADER).cloned()),
            Some("sess-1".to_string())
        );
        assert_eq!(
            sent_headers(false, model(CHATGPT_PROVIDER, CHATGPT_API)).await,
            None
        );
        assert_eq!(
            sent_headers(true, model("openai", "openai-responses")).await,
            None
        );
        assert_eq!(
            sent_headers(true, model("anthropic", "anthropic-messages")).await,
            None
        );

        let inner: StreamFn = Arc::new(|_model, _context, _options| {
            Box::pin(async move { Err(anyhow::anyhow!("not called")) })
        });
        assert!(Arc::ptr_eq(
            &scoped_stream_fn(Arc::clone(&inner), false),
            &inner
        ));
    }
}
