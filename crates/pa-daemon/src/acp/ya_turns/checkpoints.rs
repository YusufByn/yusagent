//! yusAgent (YA-18, ticket 43, PR 3a): workspace snapshots in a shadow git repository, one per
//! workspace under the agent directory, taken by the bridge before a prompt of this client goes to
//! the worker and at the end of every turn; their preview and their restore
//! (`docs/tickets/43-retour-arriere.md`, §P to §P.13).
//!
//! - Snapshots: [`start`] from `publish_prompt_start`, waited for before `PromptAndWait` (the
//!   prompt's cells run after it), under the prompt's `rewindTo`; [`end`] from
//!   `AgentTurns::close` at every `agent_end`, queued without waiting. One queue per workspace
//!   keeps their order; previews and restores go through it too.
//! - Methods, routed here by the bridge's `rewind` arm: `_yus/session/restore_preview
//!   { sessionId, to, path? }` and `_yus/session/restore { sessionId, to, files, conversation,
//!   force? }`. A restore of the conversation goes through the worker's `rewind { to, files }`; a
//!   restore of the files only through its `restored`.
//! - Git runs under the guard's checkpoints variant, the only launch allowed to write the
//!   checkpoints directory (answer 8). Without git, the snapshots are off with a message (answer 5).
//! - Cleanup once per process ([`purge`]); a deleted conversation's refs go with it
//!   ([`forget_session`]).
//!
//! Limits (§P.3, §P.4): a `bash()` started in a turn may still write after its end (taken by the
//! next snapshot); the turns of an attached TUI or of `-p` have no snapshot before them; nested
//! repositories are neither captured nor restored. Listed in `docs/fork-changes.md`.

mod git;
mod limits;
mod purge;
mod repo;
mod restore;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, oneshot};

use self::restore::Refusal;
use super::super::daemon::{DaemonAcpState, DaemonLink};
use super::super::jsonrpc;
use super::super::producer::FrameSink;

/// Restore the workspace files, the conversation, or both, to before a prompt.
pub(super) const RESTORE: &str = "_yus/session/restore";
/// What a restore would change, or one file as it was.
pub(super) const RESTORE_PREVIEW: &str = "_yus/session/restore_preview";
/// The refusal of a restore over changes made outside the agent.
const MODIFIED_OUTSIDE: &str = "Files changed outside the agent since its last turn.";
const NO_AGENT_DIR: &str = "Workspace snapshots are off: no agent directory.";

/// Whether [`serve`] answers `method`.
pub(super) fn serves(method: &str) -> bool {
    matches!(method, RESTORE | RESTORE_PREVIEW)
}

/// What the worker's `leaf` tells: the leaf, the session file's id, the session's directory.
pub(crate) struct Context {
    pub(crate) leaf_id: Option<String>,
    pub(crate) session_id: String,
    pub(crate) cwd: PathBuf,
}

enum Job {
    Snapshot {
        session: String,
        refname: String,
        done: Option<oneshot::Sender<()>>,
    },
    Preview {
        session: String,
        to: String,
        path: Option<String>,
        reply: oneshot::Sender<Result<Value, Refusal>>,
    },
    Restore {
        session: String,
        to: String,
        force: bool,
        reply: oneshot::Sender<Result<Value, Refusal>>,
    },
}

/// One workspace's queue.
struct Workspace {
    jobs: mpsc::UnboundedSender<Job>,
}

/// A session of the daemon: its file's id and its workspace.
struct Session {
    file_id: String,
    workspace: Arc<Workspace>,
}

#[derive(Default)]
struct Registry {
    sessions: HashMap<String, Arc<Session>>,
    workspaces: HashMap<PathBuf, Arc<Workspace>>,
    purged: bool,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

/// A name fit for a ref: the ids of sessions and entries.
fn ref_safe(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn checkpoints_root() -> Option<PathBuf> {
    pa_types::platform::dirs::agent_dir()
        .map(|dir| canonical(&dir).join(pa_types::platform::ya_guard::CHECKPOINTS_DIR))
}

/// The shadow repository of `work` (canonical).
fn repo_dir(root: &Path, work: &Path) -> PathBuf {
    root.join(crate::paths::hash_key(&work.display().to_string(), 16))
}

/// The session of `daemon_session_id`, registered with its workspace's queue (started on first
/// use; the first one of the process starts the cleanup).
fn session(daemon_session_id: &str, context: &Context) -> Arc<Session> {
    let work = canonical(&context.cwd);
    let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    if !registry.purged {
        registry.purged = true;
        if let Some(root) = checkpoints_root() {
            tokio::spawn(purge::run(root, work.clone()));
        }
    }
    let workspace = Arc::clone(
        registry
            .workspaces
            .entry(work.clone())
            .or_insert_with(|| Arc::new(Workspace::start(work))),
    );
    let session = Arc::new(Session {
        file_id: context.session_id.clone(),
        workspace,
    });
    registry
        .sessions
        .insert(daemon_session_id.to_string(), Arc::clone(&session));
    session
}

impl Workspace {
    fn start(work: PathBuf) -> Self {
        let (jobs, mut queue) = mpsc::unbounded_channel::<Job>();
        tokio::spawn(async move {
            let mut state = open(work);
            if let Err(reason) = &state {
                eprintln!("yusagent: {reason}");
            }
            while let Some(job) = queue.recv().await {
                handle(&mut state, job).await;
            }
        });
        Workspace { jobs }
    }
}

fn open(work: PathBuf) -> Result<repo::Repo, &'static str> {
    let program = git::program()?;
    let agent = pa_types::platform::dirs::agent_dir()
        .map(|dir| canonical(&dir))
        .ok_or(NO_AGENT_DIR)?;
    let root = agent.join(pa_types::platform::ya_guard::CHECKPOINTS_DIR);
    let dir = repo_dir(&root, &work);
    Ok(repo::Repo::new(program, dir, work, agent))
}

async fn handle(state: &mut Result<repo::Repo, &'static str>, job: Job) {
    match job {
        Job::Snapshot {
            session,
            refname,
            done,
        } => {
            if let Ok(repo) = state {
                if let Err(error) = repo
                    .snapshot(&session, Some(refname.as_str()), "turn")
                    .await
                {
                    eprintln!("yusagent: workspace snapshot failed: {error}");
                }
            }
            if let Some(done) = done {
                let _ = done.send(());
            }
        }
        Job::Preview {
            session,
            to,
            path,
            reply,
        } => {
            let answer = match (state, path) {
                (Err(reason), _) => Err(Refusal::Off(*reason)),
                (Ok(repo), Some(path)) => repo.before(&session, &to, &path).await,
                (Ok(repo), None) => repo.preview(&session, &to).await,
            };
            let _ = reply.send(answer);
        }
        Job::Restore {
            session,
            to,
            force,
            reply,
        } => {
            let answer = match state {
                Err(reason) => Err(Refusal::Off(*reason)),
                Ok(repo) => repo.restore(&session, &to, force).await,
            };
            let _ = reply.send(answer);
        }
    }
}

/// Before a prompt of this client goes to the worker (`publish_prompt_start`): the snapshot under
/// the prompt's `rewindTo`, waited for.
pub(crate) async fn start(daemon_session_id: &str, context: &Context) {
    let Some(leaf) = context.leaf_id.as_deref().filter(|leaf| ref_safe(leaf)) else {
        return;
    };
    if !ref_safe(&context.session_id) {
        return;
    }
    let session = session(daemon_session_id, context);
    let (done, wait) = oneshot::channel();
    let job = Job::Snapshot {
        session: session.file_id.clone(),
        refname: format!("refs/ya/{}/debut/{leaf}", session.file_id),
        done: Some(done),
    };
    if session.workspace.jobs.send(job).is_ok() {
        let _ = wait.await;
    }
}

/// At the end of any turn (`AgentTurns::close`): the end snapshot, queued at once, not waited
/// for. A session the bridge has not seen yet (no prompt of this client) has none.
pub(crate) fn end(daemon_session_id: &str) {
    let session = registry()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .sessions
        .get(daemon_session_id)
        .cloned();
    if let Some(session) = session {
        let _ = session.workspace.jobs.send(Job::Snapshot {
            session: session.file_id.clone(),
            refname: format!("refs/ya/{}/fin", session.file_id),
            done: None,
        });
    }
}

/// A deleted conversation (`_yus/sessions/delete`): its refs and its index go.
pub(crate) async fn forget_session(workspace: &Path, session_id: &str) {
    if !ref_safe(session_id) {
        return;
    }
    let (Ok(program), Some(root)) = (git::program(), checkpoints_root()) else {
        return;
    };
    let work = canonical(workspace);
    let dir = repo_dir(&root, &work);
    if !dir.join("HEAD").exists() {
        return;
    }
    let repo = git::Git {
        program: &program,
        repo: &dir,
        work: &work,
    };
    let prefix = format!("refs/ya/{session_id}/");
    if let Ok(listed) = repo
        .run(
            None,
            &["for-each-ref", "--format=%(refname)", &prefix],
            None,
        )
        .await
    {
        let deletes: String = String::from_utf8_lossy(&listed)
            .lines()
            .map(|name| format!("delete {name}\n"))
            .collect();
        if !deletes.is_empty() {
            let _ = repo
                .run(None, &["update-ref", "--stdin"], Some(deletes.as_bytes()))
                .await;
        }
    }
    let _ = std::fs::remove_file(dir.join("ya-index").join(session_id));
}

/// Serves `_yus/session/restore_preview` and `_yus/session/restore`.
pub(super) async fn serve(
    method: &str,
    id: &Value,
    params: &Value,
    link: &Arc<DaemonLink>,
    state: &Arc<tokio::sync::Mutex<DaemonAcpState>>,
    tx: &FrameSink,
) {
    let restoring = method == RESTORE;
    let Some(daemon_session_id) =
        super::rewind::admit(id, params, link, state, tx, restoring).await
    else {
        return;
    };
    let to = params
        .get("to")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(context) = super::rewind::context(link, &daemon_session_id).await else {
        let _ = tx.send(super::super::internal_error(
            id,
            "Session is still initializing",
        ));
        return;
    };
    if !ref_safe(&context.session_id) || !ref_safe(&to) {
        let _ = tx.send(invalid(id, &format!("no snapshot for {to}")));
        return;
    }
    let session = session(&daemon_session_id, &context);
    let frame = if restoring {
        restore_frame(id, params, link, &daemon_session_id, &session, &to).await
    } else {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string);
        let (reply, answer) = oneshot::channel();
        let job = Job::Preview {
            session: session.file_id.clone(),
            to: to.clone(),
            path,
            reply,
        };
        match ask(&session, job, answer).await {
            Ok(value) => jsonrpc::response(id, &value),
            Err(Refusal::Off(reason)) => {
                jsonrpc::response(id, &json!({ "available": false, "reason": reason }))
            }
            Err(Refusal::NoSnapshot) => jsonrpc::response(
                id,
                &json!({ "available": false, "reason": format!("no snapshot for {to}") }),
            ),
            Err(Refusal::Modified(_)) => jsonrpc::response(id, &json!({ "available": true })),
            Err(Refusal::Failed(error)) => super::super::internal_error(id, &error),
        }
    };
    let _ = tx.send(frame);
}

async fn ask(
    session: &Session,
    job: Job,
    answer: oneshot::Receiver<Result<Value, Refusal>>,
) -> Result<Value, Refusal> {
    if session.workspace.jobs.send(job).is_err() {
        return Err(Refusal::Failed("the snapshots' queue is gone".to_string()));
    }
    answer
        .await
        .unwrap_or_else(|_| Err(Refusal::Failed("the snapshots' queue is gone".to_string())))
}

fn invalid(id: &Value, details: &str) -> Value {
    jsonrpc::error_response(
        id,
        jsonrpc::INVALID_PARAMS,
        "Invalid params",
        Some(&json!({ "details": details })),
    )
}

/// The restore: the files first (in the workspace's queue), then the conversation through the
/// worker (`rewind { to, files }`), or the files-only row (`restored`).
async fn restore_frame(
    id: &Value,
    params: &Value,
    link: &DaemonLink,
    daemon_session_id: &str,
    session: &Session,
    to: &str,
) -> Value {
    let flag = |name: &str| params.get(name).and_then(Value::as_bool).unwrap_or(false);
    let (files, conversation, force) = (flag("files"), flag("conversation"), flag("force"));
    let mut outcome = json!({ "restored": [], "uncovered": [] });
    if files {
        let (reply, answer) = oneshot::channel();
        let job = Job::Restore {
            session: session.file_id.clone(),
            to: to.to_string(),
            force,
            reply,
        };
        match ask(session, job, answer).await {
            Ok(value) => outcome = value,
            Err(Refusal::Off(reason)) => return super::super::internal_error(id, reason),
            Err(Refusal::NoSnapshot) => return invalid(id, &format!("no snapshot for {to}")),
            Err(Refusal::Modified(paths)) => {
                return jsonrpc::error_response(
                    id,
                    jsonrpc::INTERNAL_ERROR,
                    "Internal error",
                    Some(&json!({ "details": MODIFIED_OUTSIDE, "modifiedOutside": paths })),
                )
            }
            Err(Refusal::Failed(error)) => return super::super::internal_error(id, &error),
        }
    }
    if conversation {
        let mut rest = Map::new();
        rest.insert("to".to_string(), json!(to));
        rest.insert("files".to_string(), json!(files));
        if let Err(error) =
            super::rewind::worker_action(link, daemon_session_id, "rewind", rest).await
        {
            if files {
                let _ =
                    super::rewind::worker_action(link, daemon_session_id, "restored", Map::new())
                        .await;
            }
            let code = if error.ends_with(crate::ya_permissions::rewind::NOT_IN_CONVERSATION) {
                jsonrpc::INVALID_PARAMS
            } else {
                jsonrpc::INTERNAL_ERROR
            };
            let message = if code == jsonrpc::INVALID_PARAMS {
                "Invalid params"
            } else {
                "Internal error"
            };
            return jsonrpc::error_response(
                id,
                code,
                message,
                Some(&json!({ "details": error, "filesRestored": files })),
            );
        }
    } else if files {
        if let Err(error) =
            super::rewind::worker_action(link, daemon_session_id, "restored", Map::new()).await
        {
            return super::super::internal_error(id, &error);
        }
    }
    jsonrpc::response(id, &outcome)
}
