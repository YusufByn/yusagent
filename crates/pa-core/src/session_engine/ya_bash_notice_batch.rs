//! yusAgent (YA-09): `[bash-done]` notices that reach an idle session wait for the other running
//! commands, then wake the model once (`bashNoticeBatchSeconds`).
//!
//! A notice that arrives while the agent runs keeps the upstream path: its row joins the
//! steering queue and rides the run's next model call at no extra cost. A notice that arrives
//! while the agent is idle (its turn is over) is held here instead of waking the session. The
//! held notices are steered together, with one wake, when the last background command settles
//! (the kernel's background-settle callback), when no command is running at all (a persistent
//! shell), or when the oldest held notice has waited the window. A read of a held command's
//! result (`bash.consumed`) withdraws its notice here, as it does from the queue.
//!
//! Grouping and the window come from Unharnessed (MIT, commit 7dc73199e,
//! `crates/uh-core/src/session_engine/wakes.rs`, `NOTICE_BATCH_MS`; notice in
//! `docs/fork-changes.md`, "Code repris d'Unharnessed"). The code is ours: Unharnessed never
//! injected a notice into a running turn, so its wakes have no equivalent of the upstream
//! steering path kept here. Window 0 registers the upstream sinks unchanged.
//!
//! Daemonless embeddings only (`-p`, RPC); the daemon worker admits notices through its own
//! queue lanes and keeps the upstream behavior.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::bash_notices::{
    bound_agent, in_process_kernel_notice_sinks, register_kernel_notice_host_handlers,
    BashConsumedNotice, BashConsumedSink, BashNoticeAgentSlot, BashNoticeWake, KernelNotice,
    KernelNoticeSink,
};
use super::messages::ASYNC_BASH_COMPLETION_CUSTOM_TYPE;
use super::{session_message_to_loop, SessionAgentMessage};
use crate::kernel::shared::{BackgroundWorkSettledCallback, HostRequestHandlers};

/// `bashNoticeBatchSeconds` when unset (Unharnessed: 2 minutes).
pub const DEFAULT_BATCH_SECONDS: u32 = 120;

/// The hold window of `bashNoticeBatchSeconds`; `None` (setting 0) keeps the upstream path.
#[must_use]
pub fn batch_window(setting: Option<u32>) -> Option<Duration> {
    let seconds = setting.unwrap_or(DEFAULT_BATCH_SECONDS);
    (seconds > 0).then(|| Duration::from_secs(u64::from(seconds)))
}

/// Steers the released notices, then wakes the session once.
pub type DeliverNotices = Arc<dyn Fn(Vec<KernelNotice>) + Send + Sync>;

/// Whether a background `bash()` command is still running in the session's kernel.
pub type RunningProbe = Arc<dyn Fn() -> bool + Send + Sync>;

/// The notices held while the session is idle.
pub struct NoticeBatch {
    window: Duration,
    deliver: DeliverNotices,
    running: OnceLock<RunningProbe>,
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    notices: Vec<KernelNotice>,
    /// Bumped when a first notice is held: a timer of an earlier hold releases nothing.
    generation: u64,
}

impl NoticeBatch {
    #[must_use]
    pub fn new(window: Duration, deliver: DeliverNotices) -> Arc<Self> {
        Arc::new(Self {
            window,
            deliver,
            running: OnceLock::new(),
            held: Mutex::new(Held::default()),
        })
    }

    /// The kernel's running-command probe, bound once the kernel provisioner exists.
    pub fn set_running_probe(&self, probe: RunningProbe) {
        let _ = self.running.set(probe);
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold one notice of an idle session. With no command running, it goes at once; the first
    /// held notice arms the window.
    pub fn hold(self: &Arc<Self>, notice: KernelNotice) {
        let generation = {
            let mut held = self.held();
            held.notices.push(notice);
            if held.notices.len() > 1 {
                return;
            }
            held.generation += 1;
            held.generation
        };
        let running = self.running.get().is_none_or(|probe| probe());
        let Some(runtime) = tokio::runtime::Handle::try_current()
            .ok()
            .filter(|_| running)
        else {
            self.release();
            return;
        };
        let batch = Arc::clone(self);
        runtime.spawn(async move {
            tokio::time::sleep(batch.window).await;
            batch.release_if(|current| current == generation);
        });
    }

    /// Steer every held notice, with one wake.
    pub fn release(&self) {
        self.release_if(|_| true);
    }

    fn release_if(&self, due: impl Fn(u64) -> bool) {
        let notices = {
            let mut held = self.held();
            if !due(held.generation) {
                return;
            }
            std::mem::take(&mut held.notices)
        };
        if !notices.is_empty() {
            (self.deliver)(notices);
        }
    }

    /// Withdraw the held notice of a command whose result was read; `false` when none is held.
    pub fn withdraw(&self, consumed: &BashConsumedNotice) -> bool {
        let mut held = self.held();
        let Some(index) = held
            .notices
            .iter()
            .position(|notice| is_notice_of(notice, consumed))
        else {
            return false;
        };
        held.notices.remove(index);
        true
    }

    /// Number of held notices.
    #[must_use]
    pub fn held_count(&self) -> usize {
        self.held().notices.len()
    }
}

fn is_bash_completion(notice: &KernelNotice) -> bool {
    notice.row.custom_type == ASYNC_BASH_COMPLETION_CUSTOM_TYPE
}

fn is_notice_of(notice: &KernelNotice, consumed: &BashConsumedNotice) -> bool {
    let details = notice.row.details.as_ref();
    is_bash_completion(notice)
        && details
            .and_then(|details| details.get("pid"))
            .and_then(Value::as_u64)
            == Some(u64::from(consumed.pid))
        && details
            .and_then(|details| details.get("command"))
            .and_then(Value::as_str)
            == Some(consumed.command.as_str())
}

/// The batching sinks over the upstream ones: a `[bash-done]` notice of an idle session is held
/// when `idle()` says so; everything else passes through.
pub fn batched_sinks(
    batch: &Arc<NoticeBatch>,
    notice: KernelNoticeSink,
    consumed: BashConsumedSink,
    idle: Arc<dyn Fn() -> bool + Send + Sync>,
) -> (KernelNoticeSink, BashConsumedSink) {
    let hold = Arc::clone(batch);
    let notice: KernelNoticeSink = Arc::new(move |kernel_notice: KernelNotice| {
        if is_bash_completion(&kernel_notice) && idle() {
            hold.hold(kernel_notice);
        } else {
            notice(kernel_notice);
        }
    });
    let withdraw = Arc::clone(batch);
    let consumed: BashConsumedSink = Arc::new(move |read: BashConsumedNotice| {
        if !withdraw.withdraw(&read) {
            consumed(read);
        }
    });
    (notice, consumed)
}

/// The daemonless kernel-notice handlers: the upstream in-process sinks
/// (`bash_notices::register_in_process_kernel_notice_handlers`), batched when `window` is set.
/// The returned batch is `None` without a window.
pub fn register_in_process_handlers(
    window: Option<Duration>,
    wake: &BashNoticeWake,
    agent_slot: &BashNoticeAgentSlot,
    handlers: &mut HostRequestHandlers,
) -> Option<Arc<NoticeBatch>> {
    let (notice, consumed) = in_process_kernel_notice_sinks(wake, agent_slot);
    let Some(window) = window else {
        register_kernel_notice_host_handlers(notice, consumed, handlers);
        return None;
    };
    let deliver: DeliverNotices = {
        let slot = Arc::clone(agent_slot);
        let wake = Arc::clone(wake);
        Arc::new(move |notices: Vec<KernelNotice>| {
            let Some(agent) = bound_agent(&slot) else {
                return;
            };
            for notice in notices {
                if let Some(row) = session_message_to_loop(&SessionAgentMessage::Custom(notice.row))
                {
                    agent.steer(row);
                }
            }
            wake();
        })
    };
    let batch = NoticeBatch::new(window, deliver);
    let idle_slot = Arc::clone(agent_slot);
    // A bound agent without an active run: its turn is over.
    let idle =
        Arc::new(move || bound_agent(&idle_slot).is_some_and(|agent| agent.signal().is_none()));
    let (notice, consumed) = batched_sinks(&batch, notice, consumed, idle);
    register_kernel_notice_host_handlers(notice, consumed, handlers);
    Some(batch)
}

/// The kernel's background-settle callback: the last command settled, so the held notices go
/// (before the embedding's own callback, which wakes the session).
#[must_use]
pub fn settled_callback(
    batch: Option<&Arc<NoticeBatch>>,
    inner: Option<BackgroundWorkSettledCallback>,
) -> Option<BackgroundWorkSettledCallback> {
    let Some(batch) = batch.map(Arc::clone) else {
        return inner;
    };
    Some(Arc::new(move || {
        batch.release();
        if let Some(inner) = &inner {
            inner();
        }
    }))
}

/// Bind the running-command probe of the session's kernel (a weak reference: the engine owns
/// the provisioner).
pub fn bind_running_probe(
    batch: Option<&Arc<NoticeBatch>>,
    provisioner: &Arc<crate::kernel::provisioner::IpythonKernelProvisioner>,
) {
    let Some(batch) = batch else {
        return;
    };
    let provisioner = Arc::downgrade(provisioner);
    batch.set_running_probe(Arc::new(move || {
        provisioner
            .upgrade()
            .and_then(|provisioner| provisioner.manager())
            .is_some_and(|manager| manager.has_background_work())
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    type Delivered = Arc<Mutex<Vec<Vec<(u32, String)>>>>;

    fn completion(pid: u32, command: &str) -> KernelNotice {
        KernelNotice::bash_completion(pid, command, 0)
    }

    fn pid_command(notice: &KernelNotice) -> (u32, String) {
        let details = notice.row.details.as_ref().unwrap();
        (
            u32::try_from(details["pid"].as_u64().unwrap()).unwrap(),
            details["command"].as_str().unwrap().to_string(),
        )
    }

    fn batch(window_seconds: u64) -> (Arc<NoticeBatch>, Delivered) {
        let delivered: Delivered = Arc::default();
        let sink = Arc::clone(&delivered);
        let batch = NoticeBatch::new(
            Duration::from_secs(window_seconds),
            Arc::new(move |notices: Vec<KernelNotice>| {
                sink.lock()
                    .unwrap()
                    .push(notices.iter().map(pid_command).collect());
            }),
        );
        (batch, delivered)
    }

    fn running(flag: &Arc<AtomicBool>) -> RunningProbe {
        let flag = Arc::clone(flag);
        Arc::new(move || flag.load(Ordering::SeqCst))
    }

    #[test]
    fn the_window_comes_from_the_setting() {
        assert_eq!(batch_window(None), Some(Duration::from_secs(120)));
        assert_eq!(batch_window(Some(30)), Some(Duration::from_secs(30)));
        assert_eq!(batch_window(Some(0)), None);
    }

    #[tokio::test(start_paused = true)]
    async fn held_notices_go_together_when_the_last_command_settles() {
        let (batch, delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(true));
        batch.set_running_probe(running(&commands));
        batch.hold(completion(1, "a"));
        batch.hold(completion(2, "b"));
        assert!(delivered.lock().unwrap().is_empty());
        let settled = settled_callback(Some(&batch), None).unwrap();
        settled();
        assert_eq!(
            *delivered.lock().unwrap(),
            vec![vec![(1, "a".to_string()), (2, "b".to_string())]]
        );
        assert_eq!(batch.held_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn held_notices_go_at_the_window() {
        let (batch, delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(true));
        batch.set_running_probe(running(&commands));
        batch.hold(completion(1, "a"));
        tokio::time::sleep(Duration::from_secs(60)).await;
        batch.hold(completion(2, "b"));
        tokio::time::sleep(Duration::from_secs(59)).await;
        assert!(delivered.lock().unwrap().is_empty());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(delivered.lock().unwrap().len(), 1);
        assert_eq!(delivered.lock().unwrap()[0].len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_earlier_timer_does_not_release_a_later_hold() {
        let (batch, delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(true));
        batch.set_running_probe(running(&commands));
        batch.hold(completion(1, "a"));
        batch.release();
        tokio::time::sleep(Duration::from_secs(100)).await;
        batch.hold(completion(2, "b"));
        // The first hold's timer fires at 120 s: the second hold is only 20 s old.
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert_eq!(delivered.lock().unwrap().len(), 1);
        tokio::time::sleep(Duration::from_secs(91)).await;
        assert_eq!(delivered.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_command_running_a_notice_goes_at_once() {
        let (batch, delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(false));
        batch.set_running_probe(running(&commands));
        batch.hold(completion(7, "server"));
        assert_eq!(
            *delivered.lock().unwrap(),
            vec![vec![(7, "server".to_string())]]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_withdraws_its_held_notice_only() {
        let (batch, delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(true));
        batch.set_running_probe(running(&commands));
        batch.hold(completion(1, "a"));
        batch.hold(completion(1, "a"));
        batch.hold(completion(2, "b"));
        let read = BashConsumedNotice {
            pid: 1,
            command: "a".to_string(),
        };
        assert!(batch.withdraw(&read));
        assert_eq!(batch.held_count(), 2);
        let other = BashConsumedNotice {
            pid: 1,
            command: "b".to_string(),
        };
        assert!(!batch.withdraw(&other));
        batch.release();
        assert_eq!(
            *delivered.lock().unwrap(),
            vec![vec![(1, "a".to_string()), (2, "b".to_string())]]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_session_and_other_notices_pass_through() {
        let (batch, _delivered) = batch(120);
        let commands = Arc::new(AtomicBool::new(true));
        batch.set_running_probe(running(&commands));
        let passed = Arc::new(AtomicUsize::new(0));
        let withdrawn = Arc::new(AtomicUsize::new(0));
        let idle = Arc::new(AtomicBool::new(false));
        let (notice, consumed) = {
            let passed = Arc::clone(&passed);
            let withdrawn = Arc::clone(&withdrawn);
            let idle = Arc::clone(&idle);
            batched_sinks(
                &batch,
                Arc::new(move |_: KernelNotice| {
                    passed.fetch_add(1, Ordering::SeqCst);
                }),
                Arc::new(move |_: BashConsumedNotice| {
                    withdrawn.fetch_add(1, Ordering::SeqCst);
                }),
                Arc::new(move || idle.load(Ordering::SeqCst)),
            )
        };
        // Busy: the notice rides the running turn (upstream path).
        notice(completion(1, "a"));
        assert_eq!(passed.load(Ordering::SeqCst), 1);
        assert_eq!(batch.held_count(), 0);
        // Idle: held.
        idle.store(true, Ordering::SeqCst);
        notice(completion(2, "b"));
        assert_eq!(passed.load(Ordering::SeqCst), 1);
        assert_eq!(batch.held_count(), 1);
        // A factory milestone is never held.
        let mut milestone = completion(3, "c");
        milestone.row.custom_type = "factory_progress_notice".to_string();
        notice(milestone);
        assert_eq!(passed.load(Ordering::SeqCst), 2);
        // A read withdraws the held notice here, an unknown one upstream.
        consumed(BashConsumedNotice {
            pid: 2,
            command: "b".to_string(),
        });
        assert_eq!(withdrawn.load(Ordering::SeqCst), 0);
        assert_eq!(batch.held_count(), 0);
        consumed(BashConsumedNotice {
            pid: 1,
            command: "a".to_string(),
        });
        assert_eq!(withdrawn.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn no_batch_keeps_the_settled_callback() {
        assert!(settled_callback(None, None).is_none());
        let calls = Arc::new(AtomicUsize::new(0));
        let inner: BackgroundWorkSettledCallback = {
            let calls = Arc::clone(&calls);
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
            })
        };
        settled_callback(None, Some(inner)).unwrap()();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
