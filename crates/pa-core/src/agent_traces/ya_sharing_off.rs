//! yusAgent (YA-16, ticket 19): trace sharing to Prime Intellect is off, with no switch.
//!
//! The cut is the production transport: [`super::ReqwestTraceHttp`] is the only way a trace
//! leaves the machine (the one-session upload, the upload-all sweep and the continuous
//! delivery all reach it), and it refuses without opening a connection. The refusal is not
//! retried. The continuous runtime does not start its delivery or recovery tasks either, so an
//! `agentTraces.enabled: true` left in a settings file neither loops nor writes the trace log on
//! every turn. Listed in `docs/fork-changes.md`.

/// Whether traces may be sent. No setting or environment variable reads it.
pub const TRACE_SHARING: bool = false;

/// The refusal every send gets.
pub const TRACE_SHARING_OFF: &str = "Trace sharing is disabled in yusAgent.";

#[cfg(test)]
mod tests {
    use super::super::tests::{clear_trace_credentials, env_lock, Fixture};
    use super::super::{
        agent_traces_log_path, upload_trace_file, ContinuousTraceUpload, ReqwestTraceHttp,
        TraceUploadDelay, TraceUploadOptions, TraceUploadResult,
    };
    use super::TRACE_SHARING_OFF;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// The opted-in home: `agentTraces.enabled: true` and a trace credential.
    fn opted_in(fixture: &Fixture) {
        let mut settings =
            crate::settings::SettingsManager::create(&fixture.cwd, &fixture.agent_dir);
        settings.set_agent_traces_enabled(true).unwrap();
        std::fs::write(
            fixture.agent_dir.join("auth.json"),
            r#"{"prime-agent-traces":{"type":"api_key","key":"synthetic-only"}}"#,
        )
        .unwrap();
    }

    /// A loopback listener that only counts: `accept` never blocks.
    fn listener() -> (std::net::TcpListener, String) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        (listener, url)
    }

    fn connected(listener: &std::net::TcpListener) -> bool {
        match listener.accept() {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("listener: {error}"),
        }
    }

    fn log_lines(fixture: &Fixture) -> Vec<String> {
        std::fs::read_to_string(agent_traces_log_path(&fixture.agent_dir))
            .map(|text| text.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The setting on, a credential and a reachable server: the manual and the automatic
    /// uploads both get the refusal, once, with no retry and no connection.
    #[tokio::test]
    // The process env must stay stable across the awaits: the sync env lock is held for the
    // whole test, as in the upstream battery.
    #[allow(clippy::await_holding_lock)]
    async fn the_enabled_setting_does_not_turn_sharing_back_on() {
        let _env = env_lock();
        let _credentials = clear_trace_credentials();
        let fixture = Fixture::new();
        opted_in(&fixture);
        let session = fixture.write_session("s.jsonl", "sid-1");
        let (listener, base_url) = listener();
        let retries = Arc::new(AtomicUsize::new(0));
        for require_enabled in [false, true] {
            let counter = Arc::clone(&retries);
            let options = TraceUploadOptions {
                session_file: Some(&session),
                cwd: &fixture.cwd,
                agent_dir: &fixture.agent_dir,
                require_enabled,
                reload_config: true,
                base_url: Some(&base_url),
                http: &ReqwestTraceHttp,
                request_timeout_ms: 2_000,
                cancel: None,
                on_upload_delay: Some(Arc::new(move |delay: TraceUploadDelay| {
                    if matches!(delay, TraceUploadDelay::RetryBackoff(_)) {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            };
            assert_eq!(
                upload_trace_file(&options).await,
                TraceUploadResult::Failed {
                    status_code: None,
                    message: TRACE_SHARING_OFF.to_string(),
                    retry_after_ms: None,
                },
                "require_enabled: {require_enabled}"
            );
        }
        assert_eq!(
            retries.load(Ordering::SeqCst),
            0,
            "the refusal is not retried"
        );
        assert!(!connected(&listener), "no connection reached the server");
        let lines = log_lines(&fixture);
        assert_eq!(lines.len(), 2, "one line per upload: {lines:?}");
        assert!(
            lines
                .iter()
                .all(|line| line.contains(&format!("upload failed: {TRACE_SHARING_OFF}"))),
            "{lines:?}"
        );
    }

    /// The continuous mode with consent on: turns persist, nothing is delivered, nothing is
    /// logged, nothing connects.
    #[test]
    fn the_continuous_mode_neither_retries_nor_logs() {
        let _env = env_lock();
        let _credentials = clear_trace_credentials();
        let fixture = Fixture::new();
        opted_in(&fixture);
        let session = fixture.write_session("s.jsonl", "sid-2");
        let (listener, base_url) = listener();
        let saved = std::env::var_os("PRIME_AGENT_TRACES_BASE_URL");
        std::env::set_var("PRIME_AGENT_TRACES_BASE_URL", &base_url);

        let (_settings, consent) =
            ContinuousTraceUpload::load_settings(&fixture.cwd, &fixture.agent_dir);
        let controller = ContinuousTraceUpload::install(
            &fixture.cwd,
            &fixture.agent_dir,
            Some(&session),
            consent,
        )
        .expect("the registry admits the installation");
        // Three turns, then several debounce periods of the service loop (1 s each).
        for _ in 0..3 {
            controller.persisted(&session);
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        std::thread::sleep(std::time::Duration::from_millis(3_500));

        let connection = connected(&listener);
        let lines = log_lines(&fixture);
        drop(controller);
        match saved {
            Some(value) => std::env::set_var("PRIME_AGENT_TRACES_BASE_URL", value),
            None => std::env::remove_var("PRIME_AGENT_TRACES_BASE_URL"),
        }
        assert!(!connection, "no connection reached the server");
        assert!(lines.is_empty(), "the continuous mode logged: {lines:?}");
    }
}
