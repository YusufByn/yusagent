//! yusAgent (T04, YA-13): per-call usage journal.
//!
//! One JSONL line per model call, next to the session file (`<id>.usage.ndjson`), schema
//! version [`USAGE_LOG_VERSION`]. Local only: lines are appended to that file and nothing is
//! sent anywhere. Agent-loop calls go through [`instrument_stream_fn`]; the one-off calls
//! (compaction summary, `/refine`, branch summary) through [`complete_simple`] or
//! [`AuxProbe`], which find the session's journal by session id. A session without a file
//! (in memory) journals nothing. Field semantics: `docs/usage.md`.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pa_agent::stream::{AssistantMessageEvent, ModelStream, StreamFn};
use serde_json::{json, Map, Value};

use crate::session::manager::{format_iso, read_session_header, SessionManager};

#[cfg(test)]
mod tests;

/// The `v` field of every line.
pub const USAGE_LOG_VERSION: u64 = 1;

/// How much of an existing journal is read back to continue its `seq`, `turn` and
/// `sincePrevMs` after a restart (`--continue`).
const RESTORE_TAIL_BYTES: u64 = 64 * 1024;

/// What a model call is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// An agent-loop call: `main`, or `subagent` in a session that has a parent.
    Main,
    /// The compaction summary.
    Compaction,
    /// The `/refine` call.
    Refine,
    /// Any other call (`other`), with its purpose (`side_question`, `branch_summary`).
    Other(&'static str),
}

impl CallKind {
    fn name(self, child: bool) -> &'static str {
        match self {
            CallKind::Main if child => "subagent",
            CallKind::Main => "main",
            CallKind::Compaction => "compaction",
            CallKind::Refine => "refine",
            CallKind::Other(_) => "other",
        }
    }
}

/// The usage of one call, as the provider adapter normalized it (`docs/usage.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The 1 h share of `cache_write`, when the provider reports the split.
    pub cache_write_1h: Option<u64>,
    /// yusAgent (T08, YA-13): the 30 min and 12 h shares of `cache_write`, when the provider
    /// reports the split (`OpenAI` Responses), as received.
    pub cache_write_30m: Option<u64>,
    pub cache_write_12h: Option<u64>,
    /// Reasoning tokens (a subset of `output`), when the provider reports them.
    pub reasoning: Option<u64>,
    pub total_tokens: u64,
    /// The cost computed from the model's catalog rates.
    pub cost: f64,
}

impl UsageCounts {
    /// `cacheRead / (input + cacheRead + cacheWrite)`: the share of the prompt read from the
    /// cache; `None` without any prompt token.
    #[must_use]
    // Token counts sit far below f64's 2^53 exact-integer range.
    #[allow(clippy::cast_precision_loss)]
    pub fn cache_rate(&self) -> Option<f64> {
        let prompt = self.input + self.cache_read + self.cache_write;
        (prompt > 0).then(|| self.cache_read as f64 / prompt as f64)
    }
}

impl From<&pa_agent::types::Usage> for UsageCounts {
    fn from(usage: &pa_agent::types::Usage) -> Self {
        Self {
            input: usage.input,
            output: usage.output,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            cache_write_1h: usage.cache_write_1h,
            cache_write_30m: usage.cache_write_30m,
            cache_write_12h: usage.cache_write_12h,
            reasoning: usage.reasoning,
            total_tokens: usage.total_tokens,
            cost: usage.cost.total,
        }
    }
}

impl From<&pa_types::ai::Usage> for UsageCounts {
    fn from(usage: &pa_types::ai::Usage) -> Self {
        Self {
            input: usage.input,
            output: usage.output,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            cache_write_1h: usage.cache_write_1h,
            cache_write_30m: usage.cache_write_30m,
            cache_write_12h: usage.cache_write_12h,
            reasoning: usage.reasoning,
            total_tokens: usage.total_tokens,
            cost: usage.cost.total.as_f64(),
        }
    }
}

/// The reasoning sent back to the model in the request history, measured on the exact
/// request body. Providers report no token count for it: `textTokensEst` is an estimate
/// (characters / 4, the heuristic of `compaction::estimate_tokens`), and the opaque parts
/// (signatures, encrypted reasoning) are counted in bytes only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryReasoning {
    /// Reasoning blocks or items in the request.
    pub blocks: u64,
    /// Characters of readable reasoning text.
    pub text_chars: u64,
    /// Bytes of signatures, redacted or encrypted reasoning.
    pub opaque_bytes: u64,
}

impl HistoryReasoning {
    /// Scans a request body in any of the wire formats: Anthropic Messages and Bedrock Converse
    /// (`messages[].content[]`), Chat Completions (`messages[].reasoning_*`), `OpenAI` Responses
    /// (`input[]` reasoning items), Gemini (`contents[].parts[]`).
    #[must_use]
    pub fn from_payload(payload: &Value) -> Self {
        let mut out = Self::default();
        for message in items(payload.get("messages")) {
            for field in ["reasoning_content", "reasoning", "reasoning_text"] {
                if message.get(field).is_some_and(Value::is_string) {
                    out.blocks += 1;
                    out.text(message.get(field));
                }
            }
            if let Some(details) = message.get("reasoning_details").and_then(Value::as_array) {
                out.blocks += details.len() as u64;
                out.opaque_bytes +=
                    serde_json::to_string(details).map_or(0, |text| text.len()) as u64;
            }
            for block in items(message.get("content")) {
                match block.get("type").and_then(Value::as_str) {
                    Some("thinking") => {
                        out.blocks += 1;
                        out.text(block.get("thinking"));
                        out.opaque(block.get("signature"));
                    }
                    Some("redacted_thinking") => {
                        out.blocks += 1;
                        out.opaque(block.get("data"));
                    }
                    _ => {}
                }
                if let Some(reasoning) = block.get("reasoningContent") {
                    out.blocks += 1;
                    out.text(reasoning.pointer("/reasoningText/text"));
                    out.opaque(reasoning.pointer("/reasoningText/signature"));
                    out.opaque(reasoning.get("redactedContent"));
                }
            }
        }
        for item in items(payload.get("input")) {
            if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                out.blocks += 1;
                for part in items(item.get("summary"))
                    .iter()
                    .chain(items(item.get("content")))
                {
                    out.text(part.get("text"));
                }
                out.opaque(item.get("encrypted_content"));
            }
        }
        for content in items(payload.get("contents")) {
            for part in items(content.get("parts")) {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    out.blocks += 1;
                    out.text(part.get("text"));
                }
                out.opaque(part.get("thoughtSignature"));
            }
        }
        out
    }

    fn text(&mut self, value: Option<&Value>) {
        if let Some(text) = value.and_then(Value::as_str) {
            self.text_chars += text.chars().count() as u64;
        }
    }

    fn opaque(&mut self, value: Option<&Value>) {
        if let Some(text) = value.and_then(Value::as_str) {
            self.opaque_bytes += text.len() as u64;
        }
    }

    fn to_json(self) -> Value {
        json!({
            "blocks": self.blocks,
            "textChars": self.text_chars,
            "textTokensEst": self.text_chars.div_ceil(4),
            "opaqueBytes": self.opaque_bytes,
        })
    }
}

fn items(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

/// One finished call, as [`UsageJournal::append`] writes it.
#[derive(Debug, Clone)]
pub struct CallRecord {
    pub kind: CallKind,
    pub provider: String,
    pub api: String,
    pub model: String,
    /// The requested reasoning effort (wire name), when the call sets one.
    pub effort: Option<String>,
    pub started_at: SystemTime,
    /// Request start to the first streamed content event; `None` when nothing streamed.
    pub ttft_ms: Option<f64>,
    pub duration_ms: f64,
    /// `stop`, `length`, `toolUse`, `error` or `aborted`.
    pub stop_reason: String,
    pub usage: UsageCounts,
    /// `None` when the request body was not observed (the call failed before sending).
    pub history_reasoning: Option<HistoryReasoning>,
    /// Timestamp of the last user message of the request (agent loop only): a new value starts
    /// a new turn.
    pub user_timestamp: Option<i64>,
    /// yusAgent (T05, YA-05): why the harness changed the request prefix on this call
    /// (`invalidation` field, agent-loop calls only); `None` writes nothing.
    pub invalidation: Option<Value>,
    /// yusAgent (T08, YA-13): the provider's raw usage object (`providerUsage`), the last one
    /// the provider reported through the usage hook; `None` writes nothing.
    pub provider_usage: Option<Value>,
}

#[derive(Debug, Default)]
struct JournalState {
    seq: u64,
    turn: u64,
    last_user_timestamp: Option<i64>,
    /// Start (ms since the epoch) of the previous call, per journaled `kind`.
    last_start_ms: HashMap<String, u64>,
}

impl JournalState {
    /// Continues the counters of an existing journal from its last lines.
    fn restore(path: &Path) -> Self {
        let mut state = Self::default();
        for line in read_tail(path, RESTORE_TAIL_BYTES).lines() {
            // The first line of a partial tail does not parse and is skipped.
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let field = |name: &str| entry.get(name).and_then(Value::as_u64);
            state.seq = state.seq.max(field("seq").unwrap_or(0));
            state.turn = state.turn.max(field("turn").unwrap_or(0));
            if let (Some(kind), Some(start)) =
                (entry.get("kind").and_then(Value::as_str), field("startMs"))
            {
                state.last_start_ms.insert(kind.to_string(), start);
            }
        }
        state
    }
}

/// The usage journal of one session.
#[derive(Debug)]
pub struct UsageJournal {
    path: PathBuf,
    session_id: String,
    parent_session_id: Option<String>,
    state: Mutex<JournalState>,
    /// yusAgent (T05, YA-05): an invalidation noted by the context transform, taken by the
    /// next agent-loop call.
    pending_invalidation: Mutex<Option<Value>>,
}

impl UsageJournal {
    /// The journal of a persisted session, registered for [`lookup`]; `None` for a session
    /// kept in memory.
    #[must_use]
    pub fn for_session(manager: &SessionManager) -> Option<Arc<Self>> {
        let session_file = manager.get_session_file()?;
        let parent_session_id = manager
            .get_header()
            .and_then(|header| header.parent_session.as_deref())
            .map(|parent| parent_session_id(session_file, parent));
        let journal = Arc::new(Self::open(
            usage_log_path(session_file),
            manager.get_session_id().to_string(),
            parent_session_id,
        ));
        register(&journal);
        Some(journal)
    }

    /// A journal at `path`, continuing the counters of the lines already there.
    #[must_use]
    pub fn open(path: PathBuf, session_id: String, parent_session_id: Option<String>) -> Self {
        let state = JournalState::restore(&path);
        Self {
            path,
            session_id,
            parent_session_id,
            state: Mutex::new(state),
            pending_invalidation: Mutex::new(None),
        }
    }

    /// yusAgent (T05, YA-05): notes that the request being built changed the prefix on
    /// purpose; the next agent-loop call carries it as its `invalidation` field.
    pub fn note_invalidation(&self, invalidation: Value) {
        *lock(&self.pending_invalidation) = Some(invalidation);
    }

    /// The invalidation noted for the call starting now, if any.
    #[must_use]
    pub fn take_invalidation(&self) -> Option<Value> {
        lock(&self.pending_invalidation).take()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line. Best-effort: a write failure is logged at debug level and never
    /// reaches the call being journaled.
    pub fn append(&self, record: &CallRecord) {
        let kind = record.kind.name(self.parent_session_id.is_some());
        let start_ms = epoch_ms(record.started_at);
        let mut state = lock(&self.state);
        state.seq += 1;
        if let Some(timestamp) = record.user_timestamp {
            if state.last_user_timestamp != Some(timestamp) {
                state.last_user_timestamp = Some(timestamp);
                state.turn += 1;
            }
        }
        let since_prev_ms = state
            .last_start_ms
            .insert(kind.to_string(), start_ms)
            .map(|previous| start_ms.saturating_sub(previous));
        let line = self.line(record, kind, start_ms, &state, since_prev_ms);
        // Written under the lock: lines keep their `seq` order.
        if let Err(error) = append_line(&self.path, &line) {
            tracing::debug!(path = %self.path.display(), %error, "usage journal append failed");
        }
    }

    fn line(
        &self,
        record: &CallRecord,
        kind: &str,
        start_ms: u64,
        state: &JournalState,
        since_prev_ms: Option<u64>,
    ) -> String {
        let usage = &record.usage;
        let mut entry = Map::new();
        let mut put = |key: &str, value: Value| {
            entry.insert(key.to_string(), value);
        };
        put("v", json!(USAGE_LOG_VERSION));
        // `ts`, never `timestamp`/`id`/`cwd`: a line must not parse as a session header.
        put(
            "ts",
            json!(format_iso(i64::try_from(start_ms).unwrap_or(i64::MAX))),
        );
        put("startMs", json!(start_ms));
        put("sessionId", json!(self.session_id));
        put("parentSessionId", json!(self.parent_session_id));
        put("seq", json!(state.seq));
        put("turn", json!(state.turn));
        put("kind", json!(kind));
        if let CallKind::Other(purpose) = record.kind {
            put("purpose", json!(purpose));
        }
        put("provider", json!(record.provider));
        put("api", json!(record.api));
        put("model", json!(record.model));
        put("effort", json!(record.effort));
        put("stopReason", json!(record.stop_reason));
        put("input", json!(usage.input));
        put("output", json!(usage.output));
        put("cacheRead", json!(usage.cache_read));
        put("cacheWrite", json!(usage.cache_write));
        if let Some(one_hour) = usage.cache_write_1h {
            put(
                "cacheWrite5m",
                json!(usage.cache_write.saturating_sub(one_hour)),
            );
            put("cacheWrite1h", json!(one_hour));
        }
        if let Some(thirty_minutes) = usage.cache_write_30m {
            put("cacheWrite30m", json!(thirty_minutes));
        }
        if let Some(twelve_hours) = usage.cache_write_12h {
            put("cacheWrite12h", json!(twelve_hours));
        }
        if let Some(reasoning) = usage.reasoning {
            put("reasoning", json!(reasoning));
        }
        put("totalTokens", json!(usage.total_tokens));
        put("cacheRate", json!(usage.cache_rate()));
        put("cost", json!(usage.cost));
        put("ttftMs", json!(record.ttft_ms.map(round_ms)));
        put("durationMs", json!(round_ms(record.duration_ms)));
        put("sincePrevMs", json!(since_prev_ms));
        if let Some(history) = record.history_reasoning {
            put("historyReasoning", history.to_json());
        }
        if let Some(invalidation) = &record.invalidation {
            put("invalidation", invalidation.clone());
        }
        if let Some(provider_usage) = &record.provider_usage {
            put("providerUsage", provider_usage.clone());
        }
        Value::Object(entry).to_string()
    }
}

/// `<dir>/<id>.usage.ndjson` for the session file `<dir>/<id>.jsonl`: JSON lines, but not the
/// `.jsonl` extension, which session scans read as session files.
#[must_use]
pub fn usage_log_path(session_file: &Path) -> PathBuf {
    let stem = session_file
        .file_stem()
        .map_or_else(|| "session".into(), |stem| stem.to_string_lossy());
    session_file.with_file_name(format!("{stem}.usage.ndjson"))
}

/// The parent session's id from its header; its file name when the header cannot be read.
fn parent_session_id(session_file: &Path, parent: &str) -> String {
    let path = if Path::new(parent).is_absolute() {
        PathBuf::from(parent)
    } else {
        session_file.parent().unwrap_or(Path::new("")).join(parent)
    };
    read_session_header(&path).map_or_else(
        || {
            path.file_stem().map_or_else(
                || parent.to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            )
        },
        |header| header.id,
    )
}

fn registry() -> &'static Mutex<HashMap<String, Weak<UsageJournal>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Weak<UsageJournal>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

fn register(journal: &Arc<UsageJournal>) {
    let mut journals = lock(registry());
    journals.retain(|_, journal| journal.strong_count() > 0);
    journals.insert(journal.session_id.clone(), Arc::downgrade(journal));
}

/// The journal of a live session, by session id.
#[must_use]
pub fn lookup(session_id: &str) -> Option<Arc<UsageJournal>> {
    lock(registry()).get(session_id).and_then(Weak::upgrade)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// Epoch milliseconds fit u64 for ~584 million years.
#[allow(clippy::cast_possible_truncation)]
fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// One decimal of precision.
fn round_ms(ms: f64) -> f64 {
    (ms * 10.0).round() / 10.0
}

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// The last `max_bytes` of a file, lossily decoded; empty when it cannot be read.
fn read_tail(path: &Path, max_bytes: u64) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(max_bytes)))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    read().unwrap_or_default()
}

/// The serde wire name of a unit enum value (`StopReason`, `ThinkingLevel`).
fn wire_name<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}

// One call in flight

/// What one call has measured so far; shared by the payload hook and the stream.
struct Probe {
    journal: Arc<UsageJournal>,
    kind: CallKind,
    provider: String,
    api: String,
    model: String,
    effort: Option<String>,
    user_timestamp: Option<i64>,
    /// yusAgent (T05, YA-05): the invalidation this call carries.
    invalidation: Option<Value>,
    started_at: SystemTime,
    started: Instant,
    state: Mutex<ProbeState>,
}

#[derive(Default)]
struct ProbeState {
    ttft_ms: Option<f64>,
    history_reasoning: Option<HistoryReasoning>,
    /// yusAgent (T08, YA-13): the last raw usage the provider reported.
    provider_usage: Option<Value>,
    finished: bool,
}

impl Probe {
    fn new(
        journal: Arc<UsageJournal>,
        kind: CallKind,
        (provider, api, model): (&str, &str, &str),
        effort: Option<String>,
        user_timestamp: Option<i64>,
    ) -> Self {
        Self {
            journal,
            kind,
            provider: provider.to_string(),
            api: api.to_string(),
            model: model.to_string(),
            effort,
            user_timestamp,
            invalidation: None,
            started_at: SystemTime::now(),
            started: Instant::now(),
            state: Mutex::new(ProbeState::default()),
        }
    }

    fn elapsed_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn observe_payload(&self, payload: &Value) {
        lock(&self.state).history_reasoning = Some(HistoryReasoning::from_payload(payload));
    }

    fn observe_usage(&self, usage: &Value) {
        lock(&self.state).provider_usage = Some(usage.clone());
    }

    fn mark_first_token(&self) {
        let elapsed = self.elapsed_ms();
        lock(&self.state).ttft_ms.get_or_insert(elapsed);
    }

    /// Writes the call's line once; later calls are no-ops.
    fn finish(&self, stop_reason: &str, usage: UsageCounts) {
        let (ttft_ms, history_reasoning, provider_usage) = {
            let mut state = lock(&self.state);
            if state.finished {
                return;
            }
            state.finished = true;
            (
                state.ttft_ms,
                state.history_reasoning,
                state.provider_usage.take(),
            )
        };
        self.journal.append(&CallRecord {
            kind: self.kind,
            provider: self.provider.clone(),
            api: self.api.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            started_at: self.started_at,
            ttft_ms,
            duration_ms: self.elapsed_ms(),
            stop_reason: stop_reason.to_string(),
            usage,
            history_reasoning,
            user_timestamp: self.user_timestamp,
            invalidation: self.invalidation.clone(),
            provider_usage,
        });
    }
}

// Agent-loop calls

/// Journals every call of `inner` as `kind`; `inner` unchanged without a journal. The request
/// body is observed through the payload hook and returned untouched.
#[must_use]
pub fn instrument_stream_fn(
    journal: Option<Arc<UsageJournal>>,
    kind: CallKind,
    inner: StreamFn,
) -> StreamFn {
    let Some(journal) = journal else {
        return inner;
    };
    Arc::new(move |model, context, options| {
        let journal = Arc::clone(&journal);
        let inner = Arc::clone(&inner);
        Box::pin(async move {
            let user_timestamp = context
                .messages
                .iter()
                .rev()
                .find_map(|message| match message {
                    pa_agent::types::Message::User(user) => Some(user.timestamp),
                    _ => None,
                });
            let invalidation = if kind == CallKind::Main {
                journal.take_invalidation()
            } else {
                None
            };
            let mut probe = Probe::new(
                journal,
                kind,
                (
                    model.provider.as_str(),
                    model.api.as_str(),
                    model.id.as_str(),
                ),
                wire_name(&options.reasoning),
                user_timestamp,
            );
            probe.invalidation = invalidation;
            let probe = Arc::new(probe);
            let mut options = options;
            let inner_payload = options.on_payload.take();
            let observer = Arc::clone(&probe);
            options.on_payload = Some(Arc::new(
                move |payload: Value, model: &pa_agent::types::Model| {
                    let next = inner_payload
                        .as_ref()
                        .and_then(|hook| hook(payload.clone(), model));
                    observer.observe_payload(next.as_ref().unwrap_or(&payload));
                    next
                },
            ));
            let inner_usage = options.on_usage.take();
            let observer = Arc::clone(&probe);
            options.on_usage = Some(Arc::new(
                move |usage: &Value, model: &pa_agent::types::Model| {
                    if let Some(hook) = &inner_usage {
                        hook(usage, model);
                    }
                    observer.observe_usage(usage);
                },
            ));
            match inner(model, context, options).await {
                Err(error) => {
                    probe.finish("error", UsageCounts::default());
                    Err(error)
                }
                Ok(stream) => Ok(Box::new(UsageStream {
                    inner: stream,
                    probe,
                    last_usage: UsageCounts::default(),
                }) as Box<dyn ModelStream>),
            }
        })
    })
}

/// The provider stream with its terminal usage journaled; an early end journals `aborted`
/// with the last usage seen.
struct UsageStream {
    inner: Box<dyn ModelStream>,
    probe: Arc<Probe>,
    last_usage: UsageCounts,
}

impl ModelStream for UsageStream {
    fn next_event(&mut self) -> pa_agent::BoxFut<'_, Option<AssistantMessageEvent>> {
        Box::pin(async move {
            let event = self.inner.next_event().await;
            match &event {
                None => self.probe.finish("aborted", self.last_usage),
                Some(AssistantMessageEvent::Done { reason, message }) => self.probe.finish(
                    &wire_name(reason).unwrap_or_default(),
                    UsageCounts::from(&message.usage),
                ),
                Some(AssistantMessageEvent::Error { reason, error }) => self.probe.finish(
                    &wire_name(reason).unwrap_or_default(),
                    UsageCounts::from(&error.usage),
                ),
                Some(
                    AssistantMessageEvent::Start { partial }
                    | AssistantMessageEvent::TextEnd { partial, .. }
                    | AssistantMessageEvent::ThinkingEnd { partial, .. }
                    | AssistantMessageEvent::ToolCallEnd { partial, .. },
                ) => self.last_usage = UsageCounts::from(&partial.usage),
                Some(
                    AssistantMessageEvent::TextStart { partial, .. }
                    | AssistantMessageEvent::TextDelta { partial, .. }
                    | AssistantMessageEvent::ThinkingStart { partial, .. }
                    | AssistantMessageEvent::ThinkingDelta { partial, .. }
                    | AssistantMessageEvent::ToolCallStart { partial, .. }
                    | AssistantMessageEvent::ToolCallDelta { partial, .. },
                ) => {
                    self.probe.mark_first_token();
                    self.last_usage = UsageCounts::from(&partial.usage);
                }
            }
            event
        })
    }

    fn result(
        &mut self,
    ) -> pa_agent::BoxFut<'_, anyhow::Result<pa_agent::types::AssistantMessage>> {
        Box::pin(async move {
            let result = self.inner.result().await;
            if let Ok(message) = &result {
                self.probe.finish(
                    &wire_name(&message.stop_reason).unwrap_or_default(),
                    UsageCounts::from(&message.usage),
                );
            }
            result
        })
    }

    fn close(&mut self) {
        self.probe.finish("aborted", self.last_usage);
        self.inner.close();
    }
}

impl Drop for UsageStream {
    fn drop(&mut self) {
        // A finished probe makes this a no-op.
        self.probe.finish("aborted", self.last_usage);
    }
}

// One-off calls (`pa_ai` directly)

/// Journals one call made with `pa_ai::stream_simple`: built before the stream (it hooks the
/// request body), fed the streamed events, then the final message. A no-op when the session
/// has no journal: the options are then left untouched.
pub struct AuxProbe(Option<Arc<Probe>>);

impl AuxProbe {
    /// Finds the journal by `options.base.session_id`.
    #[must_use]
    pub fn start(
        kind: CallKind,
        model: &pa_types::ai::Model,
        options: &mut pa_ai::types::SimpleStreamOptions,
    ) -> Self {
        let Some(journal) = options.base.session_id.as_deref().and_then(lookup) else {
            return Self(None);
        };
        let probe = Arc::new(Probe::new(
            journal,
            kind,
            (
                model.provider.as_str(),
                model.api.as_str(),
                model.id.as_str(),
            ),
            options.reasoning.as_ref().and_then(wire_name),
            None,
        ));
        let inner_payload = options.base.on_payload.take();
        let observer = Arc::clone(&probe);
        options.base.on_payload = Some(Arc::new(
            move |payload: Value, model: &pa_types::ai::Model| {
                let next = inner_payload
                    .as_ref()
                    .and_then(|hook| hook(payload.clone(), model));
                observer.observe_payload(next.as_ref().unwrap_or(&payload));
                next
            },
        ));
        let inner_usage = options.base.on_usage.take();
        let observer = Arc::clone(&probe);
        options.base.on_usage = Some(Arc::new(
            move |usage: &Value, model: &pa_types::ai::Model| {
                if let Some(hook) = &inner_usage {
                    hook(usage, model);
                }
                observer.observe_usage(usage);
            },
        ));
        Self(Some(probe))
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }

    pub fn observe(&self, event: &pa_types::ai::AssistantMessageEvent) {
        use pa_types::ai::AssistantMessageEvent as Event;
        if let Some(probe) = &self.0 {
            if matches!(
                event,
                Event::TextStart { .. }
                    | Event::TextDelta { .. }
                    | Event::ThinkingStart { .. }
                    | Event::ThinkingDelta { .. }
                    | Event::ToolcallStart { .. }
                    | Event::ToolcallDelta { .. }
            ) {
                probe.mark_first_token();
            }
        }
    }

    pub fn finish(&self, message: &pa_types::ai::AssistantMessage) {
        if let Some(probe) = &self.0 {
            probe.finish(
                &wire_name(&message.stop_reason).unwrap_or_default(),
                UsageCounts::from(&message.usage),
            );
        }
    }
}

/// `pa_ai::complete_simple`, journaled as `kind`. Without a journal it is exactly
/// `pa_ai::complete_simple`.
///
/// # Errors
///
/// Returns `pa_ai::complete_simple`'s error (no provider registered for `model.api`).
// The error type is `pa_ai::complete_simple`'s own (pa-ai allows `result_large_err` crate-wide).
#[allow(clippy::result_large_err)]
pub async fn complete_simple(
    kind: CallKind,
    model: &pa_types::ai::Model,
    context: &pa_types::ai::Context,
    options: pa_ai::types::SimpleStreamOptions,
) -> Result<pa_types::ai::AssistantMessage, pa_ai::ProviderError> {
    let mut options = options;
    let probe = AuxProbe::start(kind, model, &mut options);
    if !probe.is_active() {
        return pa_ai::complete_simple(model, context, Some(options)).await;
    }
    let mut stream = pa_ai::stream_simple(model, context, Some(options))?;
    while let Some(event) = stream.next_event().await {
        probe.observe(&event);
    }
    let message = stream.result().await;
    probe.finish(&message);
    Ok(message)
}
