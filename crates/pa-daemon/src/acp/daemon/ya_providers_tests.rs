//! yusAgent (YA-10, ticket 40): the bridge's sign-in (test 6) and the guard's authentication
//! texts (test 7, H2), `docs/tickets/40-login-extension.md` §16.3.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use super::{
    Logins, ProviderSignIn, ProviderView, SignInEvent, SignInEvents, SignInFuture, SignInOutcome,
};

const STEP: Duration = Duration::from_secs(5);
const PAGE: &str = "https://auth.example.test/authorize?state=ya40-verifier";

/// A sign-in that shows its page, asks for the code, and settles `Done` on `ya40-code`,
/// `Cancelled` on anything else or on the client's cancel.
struct Scripted;

impl ProviderSignIn for Scripted {
    fn rows(&self) -> SignInFuture<Vec<ProviderView>> {
        Box::pin(async { Vec::new() })
    }

    fn set_api_key(&self, _provider_id: String, _api_key: String) -> SignInFuture<SignInOutcome> {
        Box::pin(async { SignInOutcome::Done })
    }

    fn logout(&self, _provider_id: String) -> SignInFuture<SignInOutcome> {
        Box::pin(async { SignInOutcome::Done })
    }

    fn login(&self, _provider_id: String, events: SignInEvents) -> SignInFuture<SignInOutcome> {
        Box::pin(async move {
            events.send(SignInEvent::Url {
                url: PAGE.to_string(),
                instructions: Some("Sign in, then paste the code.".to_string()),
            });
            let (reply, code) = oneshot::channel();
            events.send(SignInEvent::Prompt {
                message: "Paste the code".to_string(),
                secret: false,
                allow_empty: false,
                optional: true,
                reply,
            });
            let mut cancelled = events.cancelled();
            tokio::select! {
                code = code => match code {
                    Ok(Some(code)) if code == "ya40-code" => SignInOutcome::Done,
                    _ => SignInOutcome::Cancelled,
                },
                _ = cancelled.wait_for(|cancelled| *cancelled) => SignInOutcome::Cancelled,
            }
        })
    }
}

/// A sign-in state whose page opener records the pages it opens.
fn logins() -> (Arc<Logins>, Arc<Mutex<Vec<String>>>) {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let pages = Arc::clone(&opened);
    let logins = Logins::new(Box::new(move |url: &str| {
        pages.lock().unwrap().push(url.to_string());
    }));
    (Arc::new(logins), opened)
}

/// The next `login_update` the bridge sent.
async fn next_update(frames: &mut mpsc::UnboundedReceiver<Value>) -> Value {
    let frame = tokio::time::timeout(STEP, frames.recv())
        .await
        .expect("a login_update frame within 5 s")
        .expect("the sink stays open");
    assert_eq!(frame["method"], "_yus/providers/login_update", "{frame}");
    frame["params"].clone()
}

/// Test 6: the steps reach the screen in order, the answer reaches the flow, a second sign-in
/// is refused while one runs, and the page opened is the sign-in's own.
#[tokio::test]
async fn a_sign_in_streams_its_steps_and_takes_the_answer() {
    let (logins, opened) = logins();
    let (tx, mut frames) = mpsc::unbounded_channel();
    let sink = tx.downgrade();
    let run = tokio::spawn({
        let logins = Arc::clone(&logins);
        async move { logins.run(&Scripted, "ya40", &sink).await }
    });

    let page = next_update(&mut frames).await;
    assert_eq!(page["providerId"], "ya40");
    assert_eq!(page["update"]["kind"], "url", "{page}");
    assert_eq!(page["update"]["url"], PAGE);
    let prompt = next_update(&mut frames).await;
    assert_eq!(prompt["update"]["kind"], "prompt", "{prompt}");
    assert_eq!(prompt["update"]["secret"], false);
    assert_eq!(prompt["update"]["allowEmpty"], false);
    assert_eq!(prompt["update"]["optional"], true);
    let prompt_id = prompt["update"]["promptId"]
        .as_u64()
        .expect("a numbered prompt");

    let other = tx.downgrade();
    assert_eq!(
        logins.run(&Scripted, "xai", &other).await,
        Err("a sign-in is already running"),
        "one sign-in at a time"
    );

    logins.open().expect("the running sign-in has a page");
    assert_eq!(*opened.lock().unwrap(), vec![PAGE.to_string()]);

    assert_eq!(
        logins.answer(prompt_id, &json!({ "kind": "text", "value": "" })),
        Err("the answer is empty"),
        "an empty answer is refused when the prompt does not allow it"
    );
    logins
        .answer(prompt_id, &json!({ "kind": "text", "value": "ya40-code" }))
        .expect("the prompt takes the code");
    let outcome = tokio::time::timeout(STEP, run)
        .await
        .expect("the sign-in settles")
        .expect("the run task");
    assert_eq!(outcome, Ok(SignInOutcome::Done));
    assert_eq!(
        logins.open(),
        Err("no sign-in page to open"),
        "the page goes with its sign-in"
    );
}

/// Test 6: `login_cancel` settles the sign-in `cancelled`, and its prompt no longer answers.
#[tokio::test]
async fn login_cancel_settles_the_sign_in_cancelled() {
    let (logins, _opened) = logins();
    let (tx, mut frames) = mpsc::unbounded_channel();
    let sink = tx.downgrade();
    let run = tokio::spawn({
        let logins = Arc::clone(&logins);
        async move { logins.run(&Scripted, "ya40", &sink).await }
    });
    let _page = next_update(&mut frames).await;
    let prompt = next_update(&mut frames).await;
    let prompt_id = prompt["update"]["promptId"]
        .as_u64()
        .expect("a numbered prompt");

    logins.cancel();
    let outcome = tokio::time::timeout(STEP, run)
        .await
        .expect("the sign-in settles")
        .expect("the run task");
    assert_eq!(outcome, Ok(SignInOutcome::Cancelled));
    assert_eq!(
        logins.answer(prompt_id, &json!({ "kind": "text", "value": "ya40-code" })),
        Err("no sign-in is running")
    );
}

/// Test 7 (H2): the guard's texts for a missing or unusable credential
/// (`agent_engine/turn/model.rs:60-72`) answer `kind: "authentication"`; another failure keeps
/// the classifier's kind.
#[test]
fn the_guard_texts_are_authentication_failures() {
    let kind = |failure: &str| {
        crate::acp::turn_failure_error(&json!(1), failure, "yusagent turn failed")["error"]["data"]
            ["kind"]
            .clone()
    };
    let missing = "yusagent turn failed: No API key found for amazon-bedrock.\n\nUse /login to log into a provider via OAuth or API key. See:\n  /x/docs/providers.md\n  /x/docs/models.md";
    assert_eq!(kind(missing), "authentication", "missing credential");
    let unusable = "Authentication failed for \"openai-codex\". Credentials may have expired or network is unavailable.\n\nRun /login to update credentials.";
    assert_eq!(kind(unusable), "authentication", "unusable credential");
    assert_eq!(
        kind("HTTP 429 Too Many Requests"),
        "rate_limit",
        "another failure keeps its kind"
    );
}

/// Test 7b (ticket 40, X5, D4): the providers' own texts for a missing credential, raised at the
/// request (`pa-ai`'s `providers/bedrock/mod.rs`, and `No API key for provider:` in the `OpenAI`,
/// Azure, Codex, Mistral and Anthropic providers), answer `kind: "authentication"`. Vertex's
/// project and location texts are configuration, not a credential: they keep their kind.
#[test]
fn the_providers_missing_credential_texts_are_authentication_failures() {
    let kind = |failure: &str| {
        crate::acp::turn_failure_error(&json!(1), failure, "yusagent turn failed")["error"]["data"]
            ["kind"]
            .clone()
    };
    assert_eq!(
        kind("yusagent turn failed: No AWS credentials available for Bedrock"),
        "authentication",
        "Bedrock without AWS credentials"
    );
    assert_eq!(
        kind("yusagent turn failed: No API key for provider: openai"),
        "authentication",
        "a provider without its key"
    );
    assert_ne!(
        kind("yusagent turn failed: Vertex AI requires a project ID. Set GOOGLE_CLOUD_PROJECT/GCLOUD_PROJECT or pass project in options."),
        "authentication",
        "a configuration text keeps its kind"
    );
}
