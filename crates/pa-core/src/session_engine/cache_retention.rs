//! yusAgent (T03, YA-04): `cacheRetention` in `settings.json` (`"short"`, the provider
//! default, or `"long"`) picks the prompt-cache TTL of a session's model requests, in place
//! of the `cache_retention: None` the adapter always sent. `PI_CACHE_RETENTION`, when set,
//! keeps priority (upstream behavior: the providers read it when the option is `None`).
//! Listed in `docs/fork-changes.md`.
//!
//! The value rides a task-local around each stream call of the session (set by
//! [`scoped_stream_fn`] in `engine.rs`, read by `provider_adapter::stream_once`), so the
//! upstream stream signatures and provider target stay as they are.

use pa_agent::stream::StreamFn;
use pa_types::ai::CacheRetention;
use serde::{Deserialize, Serialize};

/// The `cacheRetention` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheRetentionSetting {
    /// The provider default (about 5 minutes on Anthropic, in-memory on `OpenAI`).
    Short,
    /// The long TTL where the provider has one (1 hour on Anthropic, 24 hours on `OpenAI`).
    Long,
}

/// The env var that overrides the setting (upstream name).
pub const ENV: &str = "PI_CACHE_RETENTION";

tokio::task_local! {
    static SESSION_RETENTION: Option<CacheRetention>;
}

/// The option the adapter sends: `None` (the providers then read `PI_CACHE_RETENTION`, else
/// short) when the env var is set or the setting is unset; the setting otherwise.
#[must_use]
pub fn resolve(
    setting: Option<CacheRetentionSetting>,
    env_value: Option<&str>,
) -> Option<CacheRetention> {
    if env_value.is_some() {
        return None;
    }
    setting.map(|setting| match setting {
        CacheRetentionSetting::Short => CacheRetention::Short,
        CacheRetentionSetting::Long => CacheRetention::Long,
    })
}

/// [`resolve`] against the process env.
#[must_use]
pub fn resolve_from_env(setting: Option<CacheRetentionSetting>) -> Option<CacheRetention> {
    resolve(setting, std::env::var(ENV).ok().as_deref())
}

/// The retention of the stream call in progress (`None` outside [`scoped_stream_fn`]).
#[must_use]
pub fn current() -> Option<CacheRetention> {
    SESSION_RETENTION
        .try_with(|retention| *retention)
        .ok()
        .flatten()
}

/// `inner` with `retention` in scope for each call: around the call itself and the future it
/// returns, so a stream opened in either place sees it.
#[must_use]
pub fn scoped_stream_fn(inner: StreamFn, retention: Option<CacheRetention>) -> StreamFn {
    std::sync::Arc::new(move |model, context, options| {
        let future = SESSION_RETENTION.sync_scope(retention, || inner(model, context, options));
        Box::pin(SESSION_RETENTION.scope(retention, future))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_var_keeps_priority() {
        assert_eq!(
            resolve(Some(CacheRetentionSetting::Long), Some("short")),
            None
        );
        assert_eq!(resolve(Some(CacheRetentionSetting::Long), Some("")), None);
        assert_eq!(
            resolve(Some(CacheRetentionSetting::Long), None),
            Some(CacheRetention::Long)
        );
        assert_eq!(
            resolve(Some(CacheRetentionSetting::Short), None),
            Some(CacheRetention::Short)
        );
        // Unset: the providers' own default (short).
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn the_setting_parses_lowercase_only() {
        let parse =
            |value: serde_json::Value| serde_json::from_value::<CacheRetentionSetting>(value).ok();
        assert_eq!(
            parse(serde_json::json!("long")),
            Some(CacheRetentionSetting::Long)
        );
        assert_eq!(
            parse(serde_json::json!("short")),
            Some(CacheRetentionSetting::Short)
        );
        assert_eq!(parse(serde_json::json!("1h")), None);
    }

    #[tokio::test]
    async fn the_stream_call_sees_the_session_retention() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = std::sync::Arc::clone(&seen);
        let inner: StreamFn = std::sync::Arc::new(move |_model, _context, _options| {
            record.lock().unwrap().push(current());
            let record = std::sync::Arc::clone(&record);
            Box::pin(async move {
                record.lock().unwrap().push(current());
                Err(anyhow::anyhow!("no stream in this test"))
            })
        });
        let scoped = scoped_stream_fn(inner, Some(CacheRetention::Long));
        let model: pa_agent::types::Model = serde_json::from_value(serde_json::json!({
            "id": "m", "api": "test", "provider": "test"
        }))
        .unwrap();
        let result = scoped(
            model,
            pa_agent::stream::LlmContext::default(),
            pa_agent::stream::StreamRequestOptions::default(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![Some(CacheRetention::Long), Some(CacheRetention::Long)]
        );
        assert_eq!(current(), None);
    }
}
