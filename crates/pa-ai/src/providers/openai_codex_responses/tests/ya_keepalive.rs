//! yusAgent (YA-09, ticket 12): a session's parked WebSocket answers the server's keepalive pings,
//! a connection closed while parked is replaced by a fresh WebSocket (not by SSE), data frames
//! received with no request in flight are counted, and a failure on an active request still falls
//! back to SSE and pins the session (upstream behaviour).
//!
//! The mock serves both transports on one port: a request head with `Upgrade: websocket` gets a
//! server-role tungstenite stream, any other request is the SSE `POST`.

use super::*;
use futures::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const WAIT: Duration = Duration::from_secs(5);

type ServerWs = WebSocketStream<TcpStream>;

/// One request the mock received: its transport, the TCP connection it came on (1-based), body.
#[derive(Debug, Clone)]
struct Received {
    transport: &'static str,
    connection: usize,
    body: Value,
}

type Log = Arc<Mutex<Vec<Received>>>;

fn options(session_id: &str) -> OpenAICodexResponsesOptions {
    OpenAICodexResponsesOptions {
        base: StreamOptions {
            api_key: Some(mock_codex_token()),
            transport: Some(Transport::WebsocketCached),
            session_id: Some(session_id.to_string()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn session_name(name: &str) -> String {
    format!("session-ya-keepalive-{name}-{}", std::process::id())
}

async fn read_head(socket: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        socket.read_exact(&mut byte).await.expect("mock head");
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).to_string()
}

fn header(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn is_upgrade(head: &str) -> bool {
    header(head, "upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

/// Accept the next TCP connection and read its request head.
async fn accept(listener: &TcpListener) -> (TcpStream, String) {
    let (mut socket, _) = tokio::time::timeout(WAIT, listener.accept())
        .await
        .expect("client connects")
        .expect("mock accept");
    let head = read_head(&mut socket).await;
    (socket, head)
}

async fn upgrade(mut socket: TcpStream, head: &str) -> ServerWs {
    use base64::Engine as _;
    use sha1::Digest as _;
    assert!(
        is_upgrade(head),
        "expected a WebSocket upgrade, got: {head}"
    );
    let key = header(head, "sec-websocket-key").expect("upgrade key");
    let accept = base64::engine::general_purpose::STANDARD
        .encode(sha1::Sha1::digest(format!("{key}{WS_GUID}").as_bytes()));
    socket
        .write_all(
            format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .expect("mock 101");
    WebSocketStream::from_raw_socket(socket, Role::Server, None).await
}

/// The next `response.create` on the socket (`None` once the client closes).
async fn next_request(ws: &mut ServerWs) -> Option<Value> {
    loop {
        match tokio::time::timeout(WAIT, ws.next()).await.ok()?? {
            Ok(WsMessage::Text(text)) => {
                return Some(serde_json::from_str(text.as_str()).expect("request json"))
            }
            Ok(WsMessage::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

async fn respond(ws: &mut ServerWs, events: Vec<Value>) {
    for event in events {
        ws.send(WsMessage::Text(event.to_string().into()))
            .await
            .expect("mock event");
    }
}

/// Send a close frame and wait for the client's reply (or the socket's end).
async fn close_with(ws: &mut ServerWs, code: CloseCode, reason: &str) {
    ws.close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }))
    .await
    .ok();
    while let Ok(Some(Ok(_))) = tokio::time::timeout(WAIT, ws.next()).await {}
}

/// Answer the SSE `POST` on `socket` with `events`, then close the connection.
async fn serve_sse(mut socket: TcpStream, head: &str, events: Vec<Value>) -> Value {
    assert!(!is_upgrade(head), "expected the SSE POST, got an upgrade");
    let length: usize = header(head, "content-length")
        .expect("content-length")
        .parse()
        .expect("length");
    let mut body = vec![0u8; length];
    socket.read_exact(&mut body).await.expect("mock body");
    let frames = events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .concat();
    socket
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{frames}",
                frames.len()
            )
            .as_bytes(),
        )
        .await
        .expect("mock sse");
    socket.shutdown().await.ok();
    serde_json::from_slice(&body).expect("sse body json")
}

fn record(log: &Log, transport: &'static str, connection: usize, body: Value) {
    log.lock().expect("log").push(Received {
        transport,
        connection,
        body,
    });
}

fn has_diagnostic(message: &AssistantMessage, kind: &str) -> bool {
    message
        .diagnostics
        .as_ref()
        .is_some_and(|diagnostics| diagnostics.iter().any(|d| d.type_ == kind))
}

/// Wait until the session's cached worker has ended (its socket closed while parked).
async fn wait_for_closed_worker(session_id: &str) {
    for _ in 0..500 {
        let closed = session::session_state()
            .lock()
            .expect("session state")
            .connections
            .get(session_id)
            .is_none_or(|entry| entry.worker.is_closed());
        if closed {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the parked worker did not end after the server's close");
}

/// Pings sent while the connection is parked get their pongs before any further request, and the
/// next turn continues the chain on the same connection.
#[tokio::test]
async fn parked_connection_answers_pings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let server_log = Arc::clone(&log);
    let (pongs_tx, pongs_rx) = tokio::sync::oneshot::channel::<Vec<Vec<u8>>>();
    tokio::spawn(async move {
        let (socket, head) = accept(&listener).await;
        let mut ws = upgrade(socket, &head).await;
        let body = next_request(&mut ws).await.expect("request 1");
        record(&server_log, "ws", 1, body);
        respond(&mut ws, codex_response_events("resp_1", "msg_1", "Hello")).await;
        // Silence: no request in flight. The client must answer from its parked worker.
        for payload in 0..3u8 {
            ws.send(WsMessage::Ping(vec![payload].into()))
                .await
                .expect("ping");
        }
        let mut pongs = Vec::new();
        while pongs.len() < 3 {
            match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
                Ok(Some(Ok(WsMessage::Pong(payload)))) => pongs.push(payload.to_vec()),
                Ok(Some(Ok(other))) => panic!("unexpected frame during the silence: {other:?}"),
                _ => break,
            }
        }
        let _ = pongs_tx.send(pongs);
        if let Some(body) = next_request(&mut ws).await {
            record(&server_log, "ws", 1, body);
            respond(&mut ws, codex_response_events("resp_2", "msg_2", "Done")).await;
        }
        let _ = next_request(&mut ws).await;
    });

    let model = codex_test_model(port);
    let session_id = session_name("pings");
    let options = options(&session_id);
    let first =
        stream_openai_codex_responses(&model, &codex_test_context("Say hello"), Some(&options))
            .result()
            .await;
    assert_eq!(
        first.stop_reason,
        StopReason::Stop,
        "{:?}",
        first.error_message
    );

    let pongs = tokio::time::timeout(WAIT, pongs_rx)
        .await
        .expect("server reports")
        .expect("pongs");
    assert_eq!(
        pongs,
        vec![vec![0], vec![1], vec![2]],
        "one pong per ping, same payload"
    );

    let second = stream_openai_codex_responses(
        &model,
        &codex_followup_context(&first, "Now finish"),
        Some(&options),
    )
    .result()
    .await;
    assert_eq!(codex_message_text(&second).as_deref(), Some("Done"));
    let received = log.lock().expect("log").clone();
    assert_eq!(received.len(), 2);
    assert!(received
        .iter()
        .all(|r| r.transport == "ws" && r.connection == 1));
    assert_eq!(
        received[1].body.get("previous_response_id"),
        Some(&json!("resp_1")),
        "the turn after the silence continues the chain"
    );
    assert!(!session::is_websocket_sse_fallback_active(Some(
        &session_id
    )));
    session::close_websocket_sessions(Some(&session_id));
}

/// A server close while parked (`1011 keepalive ping timeout`) ends the worker; the next turn opens
/// a fresh WebSocket with the full body, without a transport failure or an SSE fallback.
#[tokio::test]
async fn close_while_parked_reconnects_over_websocket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let server_log = Arc::clone(&log);
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let (socket, head) = accept(&listener).await;
        let mut ws = upgrade(socket, &head).await;
        let body = next_request(&mut ws).await.expect("request 1");
        record(&server_log, "ws", 1, body);
        respond(&mut ws, codex_response_events("resp_1", "msg_1", "Hello")).await;
        close_with(&mut ws, CloseCode::Error, "keepalive ping timeout").await;
        let _ = closed_tx.send(());

        let (socket, head) = accept(&listener).await;
        if is_upgrade(&head) {
            let mut ws = upgrade(socket, &head).await;
            if let Some(body) = next_request(&mut ws).await {
                record(&server_log, "ws", 2, body);
                respond(&mut ws, codex_response_events("resp_2", "msg_2", "Done")).await;
            }
            let _ = next_request(&mut ws).await;
        } else {
            let body = serve_sse(
                socket,
                &head,
                codex_response_events("resp_2", "msg_2", "Done"),
            )
            .await;
            record(&server_log, "sse", 2, body);
        }
    });

    let model = codex_test_model(port);
    let session_id = session_name("close");
    let options = options(&session_id);
    let first =
        stream_openai_codex_responses(&model, &codex_test_context("Say hello"), Some(&options))
            .result()
            .await;
    assert_eq!(
        first.stop_reason,
        StopReason::Stop,
        "{:?}",
        first.error_message
    );
    tokio::time::timeout(WAIT, closed_rx)
        .await
        .expect("server closed")
        .expect("close signal");
    wait_for_closed_worker(&session_id).await;

    let second = stream_openai_codex_responses(
        &model,
        &codex_followup_context(&first, "Now finish"),
        Some(&options),
    )
    .result()
    .await;
    assert_eq!(codex_message_text(&second).as_deref(), Some("Done"));
    assert!(
        !has_diagnostic(&second, "provider_transport_failure"),
        "no transport failure: {:?}",
        second.diagnostics
    );
    let received = log.lock().expect("log").clone();
    assert_eq!(received.len(), 2);
    assert_eq!(
        (received[1].transport, received[1].connection),
        ("ws", 2),
        "a fresh WebSocket, not SSE"
    );
    assert_eq!(
        received[1].body.get("previous_response_id"),
        None,
        "the chain was anchored to the closed connection: full body"
    );
    assert!(!session::is_websocket_sse_fallback_active(Some(
        &session_id
    )));
    session::close_websocket_sessions(Some(&session_id));
}

/// A data frame received while parked is not delivered to the next request: that request streams
/// its own response and carries a diagnostic with the count.
#[tokio::test]
async fn data_frame_while_parked_is_counted_not_delivered() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let server_log = Arc::clone(&log);
    let (read_tx, read_rx) = tokio::sync::oneshot::channel::<bool>();
    tokio::spawn(async move {
        let (socket, head) = accept(&listener).await;
        let mut ws = upgrade(socket, &head).await;
        let body = next_request(&mut ws).await.expect("request 1");
        record(&server_log, "ws", 1, body);
        respond(&mut ws, codex_response_events("resp_1", "msg_1", "Hello")).await;
        // A stray terminal event, then a ping: the pong proves the client read past the event.
        respond(
            &mut ws,
            vec![json!({ "type": "response.done", "response": { "id": "resp_stray" } })],
        )
        .await;
        ws.send(WsMessage::Ping(vec![9u8].into()))
            .await
            .expect("ping");
        let ponged = matches!(
            tokio::time::timeout(Duration::from_secs(2), ws.next()).await,
            Ok(Some(Ok(WsMessage::Pong(_))))
        );
        let _ = read_tx.send(ponged);
        if let Some(body) = next_request(&mut ws).await {
            record(&server_log, "ws", 1, body);
            respond(&mut ws, codex_response_events("resp_2", "msg_2", "Done")).await;
        }
        let _ = next_request(&mut ws).await;
    });

    let model = codex_test_model(port);
    let session_id = session_name("stray");
    let options = options(&session_id);
    let first =
        stream_openai_codex_responses(&model, &codex_test_context("Say hello"), Some(&options))
            .result()
            .await;
    assert_eq!(
        first.stop_reason,
        StopReason::Stop,
        "{:?}",
        first.error_message
    );
    assert!(
        tokio::time::timeout(WAIT, read_rx)
            .await
            .expect("server reports")
            .expect("signal"),
        "the parked worker read the stray event"
    );

    let second = stream_openai_codex_responses(
        &model,
        &codex_followup_context(&first, "Now finish"),
        Some(&options),
    )
    .result()
    .await;
    assert_eq!(codex_message_text(&second).as_deref(), Some("Done"));
    assert_eq!(second.response_id.as_deref(), Some("resp_2"));
    let diagnostic = second
        .diagnostics
        .as_ref()
        .and_then(|diagnostics| {
            diagnostics
                .iter()
                .find(|d| d.type_ == ya_idle::IDLE_MESSAGES_DIAGNOSTIC)
        })
        .expect("idle-message diagnostic");
    let details = diagnostic.details.as_ref().expect("details");
    assert_eq!(details.get("textMessages"), Some(&json!(1)));
    assert_eq!(details.get("binaryMessages"), Some(&json!(0)));
    assert_eq!(details.get("lastType"), Some(&json!("response.done")));
    let received = log.lock().expect("log").clone();
    assert_eq!(
        received[1].body.get("previous_response_id"),
        Some(&json!("resp_1"))
    );
    session::close_websocket_sessions(Some(&session_id));
}

/// Non-regression: a 1011 close on an active request, before any event, still falls back to SSE
/// with a transport-failure diagnostic, and pins the session to SSE (full bodies) afterwards.
#[tokio::test]
async fn close_during_a_request_still_falls_back_to_sse() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let server_log = Arc::clone(&log);
    tokio::spawn(async move {
        let (socket, head) = accept(&listener).await;
        let mut ws = upgrade(socket, &head).await;
        let body = next_request(&mut ws).await.expect("request 1");
        record(&server_log, "ws", 1, body);
        close_with(&mut ws, CloseCode::Error, "keepalive ping timeout").await;
        drop(ws);

        let (socket, head) = accept(&listener).await;
        let body = serve_sse(
            socket,
            &head,
            codex_response_events("resp_s1", "msg_1", "Hello"),
        )
        .await;
        record(&server_log, "sse", 2, body);
        let (socket, head) = accept(&listener).await;
        let body = serve_sse(
            socket,
            &head,
            codex_response_events("resp_s2", "msg_2", "Done"),
        )
        .await;
        record(&server_log, "sse", 3, body);
    });

    let model = codex_test_model(port);
    let session_id = session_name("fallback");
    let options = options(&session_id);
    let first =
        stream_openai_codex_responses(&model, &codex_test_context("Say hello"), Some(&options))
            .result()
            .await;
    assert_eq!(
        codex_message_text(&first).as_deref(),
        Some("Hello"),
        "{:?}",
        first.error_message
    );
    let failure = first
        .diagnostics
        .as_ref()
        .and_then(|diagnostics| {
            diagnostics
                .iter()
                .find(|d| d.type_ == "provider_transport_failure")
        })
        .expect("transport-failure diagnostic");
    assert!(
        failure
            .error
            .as_ref()
            .is_some_and(|error| error.message.contains("1011")),
        "{failure:?}"
    );
    let details = failure.details.as_ref().expect("details");
    assert_eq!(details.get("eventsEmitted"), Some(&json!(false)));
    assert_eq!(details.get("fallbackTransport"), Some(&json!("sse")));
    assert!(session::is_websocket_sse_fallback_active(Some(&session_id)));

    let second = stream_openai_codex_responses(
        &model,
        &codex_followup_context(&first, "Now finish"),
        Some(&options),
    )
    .result()
    .await;
    assert_eq!(codex_message_text(&second).as_deref(), Some("Done"));
    let received = log.lock().expect("log").clone();
    let transports: Vec<_> = received.iter().map(|r| r.transport).collect();
    assert_eq!(transports, vec!["ws", "sse", "sse"]);
    assert_eq!(
        received[2].body.get("previous_response_id"),
        None,
        "SSE sends full bodies"
    );
    session::reset_debug_stats(Some(&session_id));
    session::close_websocket_sessions(Some(&session_id));
}
