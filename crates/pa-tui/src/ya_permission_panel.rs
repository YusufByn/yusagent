//! yusAgent (YA-11, ticket 20, PR 3): the permission panel of a manual session.
//!
//! The worker asks before each tool call (`ya_permission_request`); the panel shows the whole call
//! (decision T2: every character, lines folded at the width, never cut; control and invisible
//! formatting characters shown escaped, so the code on screen is the code that runs) and takes the
//! answer:
//! - **quiet window** (decision T3): the answer keys (`y` allows once, `n` or `Esc` denies once)
//!   count only after [`QUIET`] without any keystroke; every keystroke restarts it. Until then the
//!   keys go where they would go without the panel (the editor, an open picker), so a sentence
//!   being typed when the request arrives is neither cut nor taken as an answer;
//! - `Ctrl+C` withdraws the request at once (`cancelled`: a refusal, the turn stops, D7);
//! - once the panel answers, the arrows and `PgUp`/`PgDn` scroll the call; any other key goes on
//!   to its usual owner and restarts the quiet window.
//!
//! Several requests can wait at once (a batch of calls): they are answered in order. The panel
//! renders over the bottom rows of the transcript window (`crate::view::frame`). Listed in
//! `docs/fork-changes.md`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::theme::{Theme, ThemeColor};
use crate::width::{char_width, truncate_line};
use crate::{Line, Span};

/// The pause in typing after which the panel takes the answer keys.
pub const QUIET: Duration = Duration::from_millis(500);

/// The tray's badge while the session runs in "auto" without a sandbox (decision D4).
pub const AUTO_BADGE: &str = "auto \u{b7} no sandbox";

/// The gutter in front of each row of the call.
const GUTTER: &str = "│ ";

/// One request of the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    /// What the panel shows: the call's only text argument (the code of a cell, a patch), or all
    /// its arguments as JSON.
    pub text: String,
    /// The argument shown alone, when there is one (`code`).
    pub field: Option<String>,
}

impl PermissionRequest {
    /// The request of a `ya_permission_request` event; `None` without a `requestId`.
    #[must_use]
    pub fn from_event(event: &Value) -> Option<Self> {
        let text_of = |key: &str| {
            event
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let request_id = text_of("requestId");
        if request_id.is_empty() {
            return None;
        }
        let args = event.get("args").cloned().unwrap_or(Value::Null);
        let (field, text) = shown_arguments(&args);
        Some(PermissionRequest {
            request_id,
            tool_call_id: text_of("toolCallId"),
            tool_name: text_of("toolName"),
            text,
            field,
        })
    }
}

/// An object with a single string argument shows that string; anything else shows every
/// argument as JSON, so nothing the call carries is hidden.
fn shown_arguments(args: &Value) -> (Option<String>, String) {
    if let Some(object) = args.as_object() {
        if object.len() == 1 {
            if let Some((key, Value::String(text))) = object.iter().next() {
                return (Some(key.clone()), text.clone());
            }
        }
    }
    (
        None,
        serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()),
    )
}

/// What one key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKey {
    /// Allow the current request once.
    Allow,
    /// Deny the current request once.
    Deny,
    /// Withdraw the current request (`Ctrl+C`): a refusal that stops the turn.
    Withdraw,
    /// The panel used the key (scrolling).
    Consumed,
    /// The key goes on to its usual owner.
    PassThrough,
}

impl PanelKey {
    /// The `outcome` of the `respond` action, for an answer.
    #[must_use]
    pub fn outcome(self) -> Option<&'static str> {
        match self {
            PanelKey::Allow => Some("allow"),
            PanelKey::Deny => Some("deny"),
            PanelKey::Withdraw => Some("cancelled"),
            PanelKey::Consumed | PanelKey::PassThrough => None,
        }
    }
}

/// The open panel: the requests of one session, oldest first.
#[derive(Debug, Clone)]
pub struct PermissionPanel {
    session_id: String,
    queue: VecDeque<PermissionRequest>,
    /// The last keystroke, or the moment the current request showed.
    quiet_since: Instant,
    /// The first shown row of the current request.
    scroll: usize,
    /// The code rows of the last render (one page).
    page: usize,
    /// Whether the last render showed the panel answering.
    rendered_armed: bool,
}

impl PermissionPanel {
    #[must_use]
    pub fn new(session_id: String, request: PermissionRequest, now: Instant) -> Self {
        PermissionPanel {
            session_id,
            queue: VecDeque::from([request]),
            quiet_since: now,
            scroll: 0,
            page: 1,
            rendered_armed: false,
        }
    }

    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The request the panel shows.
    #[must_use]
    pub fn current(&self) -> Option<&PermissionRequest> {
        self.queue.front()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// A new request: queued behind the others (a request already known is ignored).
    pub fn push(&mut self, request: PermissionRequest) {
        if !self
            .queue
            .iter()
            .any(|known| known.request_id == request.request_id)
        {
            self.queue.push_back(request);
        }
    }

    /// Removes `request_id`; `true` when it was waiting. When it was the one shown, the next one
    /// starts its own quiet window.
    pub fn close(&mut self, request_id: &str, now: Instant) -> bool {
        let Some(index) = self
            .queue
            .iter()
            .position(|request| request.request_id == request_id)
        else {
            return false;
        };
        self.queue.remove(index);
        if index == 0 {
            self.scroll = 0;
            self.quiet_since = now;
        }
        true
    }

    /// A keystroke or a paste that went to another owner: the quiet window restarts.
    pub fn note_input(&mut self, now: Instant) {
        self.quiet_since = now;
    }

    /// Whether the panel takes the answer keys at `now`.
    #[must_use]
    pub fn armed(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.quiet_since) >= QUIET
    }

    /// Whether the loop must keep waking to show the panel answering: the last render did not.
    #[must_use]
    pub fn waits_to_arm(&self) -> bool {
        !self.rendered_armed
    }

    /// Whether a repaint would change the panel (it armed since the last render).
    #[must_use]
    pub fn needs_repaint(&self, now: Instant) -> bool {
        self.armed(now) != self.rendered_armed
    }

    /// One key id (`crate::keys::key_event_to_id`) at `now`.
    pub fn handle_key(&mut self, id: &str, now: Instant) -> PanelKey {
        if id == "ctrl+c" {
            return PanelKey::Withdraw;
        }
        if !self.armed(now) {
            self.quiet_since = now;
            return PanelKey::PassThrough;
        }
        match id {
            "y" => PanelKey::Allow,
            "n" | "escape" => PanelKey::Deny,
            "up" => {
                self.scroll = self.scroll.saturating_sub(1);
                PanelKey::Consumed
            }
            "down" => {
                self.scroll = self.scroll.saturating_add(1);
                PanelKey::Consumed
            }
            "pageUp" => {
                self.scroll = self.scroll.saturating_sub(self.page.max(1));
                PanelKey::Consumed
            }
            "pageDown" => {
                self.scroll = self.scroll.saturating_add(self.page.max(1));
                PanelKey::Consumed
            }
            _ => {
                self.quiet_since = now;
                PanelKey::PassThrough
            }
        }
    }

    /// The panel's rows: at most `max_rows`, each at most `width` columns.
    pub fn render(
        &mut self,
        theme: &Theme,
        width: usize,
        max_rows: usize,
        now: Instant,
    ) -> Vec<Line> {
        let armed = self.armed(now);
        self.rendered_armed = armed;
        let Some(request) = self.queue.front() else {
            return Vec::new();
        };
        if max_rows == 0 || width == 0 {
            return Vec::new();
        }
        let code_width = width.saturating_sub(GUTTER.chars().count()).max(1);
        let rows = display_rows(&request.text, code_width);
        let logical = request.text.split('\n').count();

        let tool = if request.tool_name.is_empty() {
            "tool call"
        } else {
            request.tool_name.as_str()
        };
        let plural = if logical == 1 { "" } else { "s" };
        let mut label = format!("· {tool} · {logical} line{plural}");
        if let Some(field) = &request.field {
            label = format!("{label} of {field}");
        }
        if self.queue.len() > 1 {
            let waiting = self.queue.len();
            label = format!("{label} · 1 of {waiting} waiting");
        }
        let title = truncate_line(
            &vec![
                theme.bold(theme.fg_span(ThemeColor::Warning, " Permission ")),
                theme.fg_span(ThemeColor::Muted, label),
            ],
            width,
            "…",
        );

        let hint = if armed {
            theme.fg_span(
                ThemeColor::Accent,
                " y allow once · n or Esc deny · Ctrl+C withdraw",
            )
        } else {
            theme.fg_span(
                ThemeColor::Warning,
                " pause typing to answer (keys go to the editor until then) · Ctrl+C withdraws",
            )
        };
        let hint = truncate_line(&vec![hint], width, "…");

        // Rows left for the call once the title and the hint are placed.
        let room = max_rows.saturating_sub(2);
        let overflow = rows.len() > room;
        let code_rows = if overflow {
            room.saturating_sub(1)
        } else {
            rows.len()
        };
        self.page = code_rows.max(1);
        let last_start = rows.len().saturating_sub(code_rows);
        self.scroll = self.scroll.min(last_start);

        let mut lines = vec![title];
        let gutter = theme.fg_style(ThemeColor::Dim);
        for row in rows.iter().skip(self.scroll).take(code_rows) {
            lines.push(vec![
                Span::styled(GUTTER.to_string(), gutter),
                Span::raw(row.clone()),
            ]);
        }
        if overflow && code_rows > 0 {
            let first = self.scroll + 1;
            let shown_end = (self.scroll + code_rows).min(rows.len());
            let total = rows.len();
            lines.push(truncate_line(
                &vec![theme.fg_span(
                    ThemeColor::Muted,
                    format!(" rows {first}–{shown_end} of {total} · ↑/↓ PgUp/PgDn scroll"),
                )],
                width,
                "…",
            ));
        }
        lines.push(hint);
        lines.truncate(max_rows);
        lines
    }
}

/// The text folded into rows of at most `width` columns: every character is kept (a long line
/// continues on the next row), tabs become four spaces, and the control and invisible formatting
/// characters are shown escaped (`\u{1b}`), so the terminal cannot hide or reorder any of it.
#[must_use]
pub fn display_rows(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut used = 0usize;
        for c in line.chars() {
            for shown in visible(c).chars() {
                let cell = char_width(shown);
                if used + cell > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                row.push(shown);
                used += cell;
            }
        }
        rows.push(row);
    }
    rows
}

/// One character as the panel shows it.
fn visible(c: char) -> String {
    if c == '\t' {
        return "    ".to_string();
    }
    if c.is_control() || is_invisible_format(c) {
        return format!("\\u{{{:x}}}", u32::from(c));
    }
    c.to_string()
}

/// The invisible formatting characters that can hide or reorder text on screen (bidirectional
/// controls, zero-width characters, the byte-order mark, the soft hyphen).
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{ad}'
            | '\u{61c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(id: &str, code: &str) -> PermissionRequest {
        PermissionRequest::from_event(&json!({
            "type": "ya_permission_request",
            "requestId": id,
            "toolCallId": format!("call-{id}"),
            "toolName": "python_repl",
            "args": { "code": code },
        }))
        .expect("a request")
    }

    fn panel(now: Instant) -> PermissionPanel {
        PermissionPanel::new("s1".to_string(), request("r1", "print(1)"), now)
    }

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|span| span.content.clone()).collect())
            .collect()
    }

    fn theme() -> Theme {
        Theme::builtin("prime", crate::theme::ColorMode::TrueColor)
    }

    /// Typing a sentence that holds `y` and `n` (keys 100 ms apart) never answers: every key
    /// goes on to the editor.
    #[test]
    fn continuous_typing_with_y_never_answers() {
        let start = Instant::now();
        let mut panel = panel(start);
        let mut at = start;
        for key in ["m", "y", " ", "n", "a", "m", "e", " ", "y", "e", "s"] {
            at += Duration::from_millis(100);
            assert_eq!(panel.handle_key(key, at), PanelKey::PassThrough, "{key}");
        }
    }

    /// A key typed right as the request shows (a queued keystroke) goes to the editor too.
    #[test]
    fn a_key_at_the_opening_goes_to_the_editor() {
        let start = Instant::now();
        let mut panel = panel(start);
        assert_eq!(panel.handle_key("y", start), PanelKey::PassThrough);
        assert!(!panel.armed(start + Duration::from_millis(499)));
    }

    #[test]
    fn y_after_a_pause_allows() {
        let start = Instant::now();
        let mut panel = panel(start);
        assert_eq!(
            panel.handle_key("a", start + Duration::from_millis(200)),
            PanelKey::PassThrough
        );
        // The pause counts from the last keystroke, not from the opening.
        assert_eq!(
            panel.handle_key("y", start + Duration::from_millis(600)),
            PanelKey::PassThrough
        );
        assert_eq!(
            panel.handle_key("y", start + Duration::from_millis(1100)),
            PanelKey::Allow
        );
        assert_eq!(PanelKey::Allow.outcome(), Some("allow"));
    }

    #[test]
    fn n_or_escape_after_a_pause_denies() {
        let start = Instant::now();
        let mut panel = panel(start);
        let later = start + QUIET;
        assert_eq!(panel.handle_key("n", later), PanelKey::Deny);
        assert_eq!(panel.handle_key("escape", later), PanelKey::Deny);
        assert_eq!(PanelKey::Deny.outcome(), Some("deny"));
    }

    /// `Ctrl+C` withdraws at once, inside the quiet window too.
    #[test]
    fn ctrl_c_withdraws_inside_the_quiet_window() {
        let start = Instant::now();
        let mut panel = panel(start);
        assert_eq!(
            panel.handle_key("a", start + Duration::from_millis(10)),
            PanelKey::PassThrough
        );
        assert_eq!(
            panel.handle_key("ctrl+c", start + Duration::from_millis(20)),
            PanelKey::Withdraw
        );
        assert_eq!(PanelKey::Withdraw.outcome(), Some("cancelled"));
    }

    /// Armed, another key goes on to the editor and the quiet window starts again.
    #[test]
    fn another_key_disarms() {
        let start = Instant::now();
        let mut panel = panel(start);
        let armed = start + QUIET;
        assert_eq!(panel.handle_key("x", armed), PanelKey::PassThrough);
        assert_eq!(panel.handle_key("y", armed), PanelKey::PassThrough);
        assert_eq!(panel.handle_key("y", armed + QUIET), PanelKey::Allow);
    }

    /// A paste restarts the quiet window like a key.
    #[test]
    fn a_paste_restarts_the_quiet_window() {
        let start = Instant::now();
        let mut panel = panel(start);
        panel.note_input(start + Duration::from_millis(400));
        assert!(!panel.armed(start + Duration::from_millis(800)));
        assert!(panel.armed(start + Duration::from_millis(900)));
    }

    #[test]
    fn the_requests_are_answered_in_order_and_the_next_waits_its_own_pause() {
        let start = Instant::now();
        let mut panel = panel(start);
        panel.push(request("r2", "print(2)"));
        panel.push(request("r2", "print(2)"));
        let later = start + Duration::from_secs(5);
        assert!(panel.close("r1", later));
        assert_eq!(panel.current().map(|r| r.request_id.as_str()), Some("r2"));
        assert!(!panel.armed(later + Duration::from_millis(100)));
        assert!(!panel.close("r1", later));
        assert!(panel.close("r2", later));
        assert!(panel.is_empty());
    }

    /// Long lines fold and keep every character; indentation stays; nothing is cut.
    #[test]
    fn rows_fold_and_keep_every_character() {
        let code = "    x = 'abcdefghij'; import os; os.system('rm -rf build')";
        let rows = display_rows(code, 10);
        assert!(rows.iter().all(|row| row.chars().count() <= 10));
        assert_eq!(rows.concat(), code);
        assert!(rows[0].starts_with("    x"));
    }

    /// Control and invisible formatting characters show escaped.
    #[test]
    fn control_and_bidi_characters_show_escaped() {
        let rows = display_rows("a\u{1b}[2Kb\u{202e}c\td\re", 80);
        assert_eq!(rows, ["a\\u{1b}[2Kb\\u{202e}c    d\\u{d}e"]);
    }

    #[test]
    fn a_call_without_a_single_text_argument_shows_its_json() {
        let event = json!({
            "requestId": "r9", "toolCallId": "c9", "toolName": "exec_command",
            "args": { "cmd": "ls", "workdir": "/tmp" },
        });
        let request = PermissionRequest::from_event(&event).expect("a request");
        assert_eq!(request.field, None);
        assert!(request.text.contains("\"workdir\": \"/tmp\""));
        assert!(PermissionRequest::from_event(&json!({ "toolName": "x" })).is_none());
    }

    /// The render shows the whole call when it fits, and the hint follows the quiet window.
    #[test]
    fn the_render_shows_the_call_and_the_state_of_the_answer() {
        let start = Instant::now();
        let mut panel = PermissionPanel::new(
            "s1".to_string(),
            request("r1", "open('a.txt', 'w').write('x')\nprint('done')"),
            start,
        );
        let rows = text(&panel.render(&theme(), 80, 20, start));
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(
            rows[0].contains("python_repl · 2 lines of code"),
            "{rows:?}"
        );
        assert!(rows[1].ends_with("open('a.txt', 'w').write('x')"));
        assert!(rows[2].ends_with("print('done')"));
        assert!(rows[3].contains("pause typing to answer"));
        assert!(panel.waits_to_arm());
        let later = start + QUIET;
        assert!(panel.needs_repaint(later));
        let rows = text(&panel.render(&theme(), 80, 20, later));
        assert!(rows[3].contains("y allow once"), "{rows:?}");
        assert!(!panel.waits_to_arm());
        assert!(!panel.needs_repaint(later));
    }

    /// A call taller than the room scrolls; the render never exceeds its rows.
    #[test]
    fn a_tall_call_scrolls_within_its_rows() {
        let start = Instant::now();
        let code = (1..=30)
            .map(|n| format!("line_{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut panel = PermissionPanel::new("s1".to_string(), request("r1", &code), start);
        let rows = text(&panel.render(&theme(), 40, 8, start));
        assert_eq!(rows.len(), 8);
        assert!(rows[1].ends_with("line_1"));
        assert!(rows[6].contains("rows 1–5 of 30"), "{rows:?}");
        let armed = start + QUIET;
        assert_eq!(panel.handle_key("pageDown", armed), PanelKey::Consumed);
        let rows = text(&panel.render(&theme(), 40, 8, armed));
        assert!(rows[1].ends_with("line_6"), "{rows:?}");
        for _ in 0..20 {
            panel.handle_key("pageDown", armed);
        }
        let rows = text(&panel.render(&theme(), 40, 8, armed));
        assert!(rows[5].ends_with("line_30"), "{rows:?}");
    }
}
