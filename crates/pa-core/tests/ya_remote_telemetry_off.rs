//! yusAgent (YA-16, ticket 19): remote telemetry to Prime Intellect is off, with no switch.
//!
//! The setting `telemetry.enabled: true`, a client gated on it the way the product gates its
//! delivery passes, and the analytics sink pointed at a local server: nothing connects. The
//! client reads the setting itself, not through the environment: `.cargo/config.toml` sets
//! `DO_NOT_TRACK=1` for every test, which would turn the product switch off on its own.

use std::net::TcpListener;
use std::sync::Arc;

use pa_telemetry::{
    AnalyticsSink, Properties, SinkOutcome, TelemetryClient, TelemetryClientConfig, TelemetryEvent,
    TelemetrySink,
};
use serde_json::json;

const INSTALL_ID: &str = "0b5f4c1e-6a59-4c6e-9d3a-2f1e8b7c9a10";

fn connected(listener: &TcpListener) -> bool {
    match listener.accept() {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("listener: {error}"),
    }
}

fn command_used(name: &str) -> Properties {
    let mut properties = Properties::new();
    properties.set("command_name", json!(name));
    properties
}

#[tokio::test]
async fn the_enabled_setting_does_not_turn_remote_telemetry_back_on() {
    let dir = tempfile::tempdir().expect("temp dir");
    let agent_dir = dir.path().join("agent");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    let mut settings = pa_core::settings::SettingsManager::create(dir.path(), &agent_dir);
    settings
        .set_telemetry_enabled(true)
        .expect("telemetry setting on");
    assert!(settings.get_telemetry_enabled(), "the setting is on");

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    listener.set_nonblocking(true).expect("non-blocking");
    let url = format!(
        "http://{}/api/v1/agent-analytics/events",
        listener.local_addr().expect("addr")
    );

    // The sink alone refuses without a request.
    let sink = AnalyticsSink::new(url.clone());
    let event = TelemetryEvent::new("agent command used", command_used("model"));
    assert_eq!(
        sink.send_batch(INSTALL_ID, vec![event]).await,
        SinkOutcome::Dropped
    );

    // The batching client, gated on the setting.
    let mut config = TelemetryClientConfig::new(INSTALL_ID);
    config.sinks = vec![Arc::new(AnalyticsSink::new(url)) as Arc<dyn TelemetrySink>];
    let gate = settings.reopen();
    config.enabled = Some(Arc::new(move || gate.reopen().get_telemetry_enabled()));
    let client = TelemetryClient::spawn(config).expect("client");
    client.track("agent command used", command_used("model"));
    client.track("agent command used", command_used("resume"));
    client.flush().await.expect("flush");
    client.shutdown().await.expect("shutdown");

    assert!(!connected(&listener), "no connection reached the server");
}
