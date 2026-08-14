//! App state and input handling — the grok-build interaction homage.
//!
//! Enter sends (or queues mid-turn, protocol-natively); Alt+Enter interrupts
//! and sends now; Esc cancels a running turn with the draft preserved, and
//! double-Esc clears an idle draft; Ctrl+C clears first, then arms quit;
//! `!` runs a local shell command; `/` opens the slash menu; Up recalls
//! history on an empty prompt.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use unicode_width::UnicodeWidthChar;

use crate::bus::{AppEvent, Cmd, CtlEvent};
use crate::controller::Controller;
use crate::events::parse_notification;
use crate::runtime::RuntimeConfig;
use crate::theme::Theme;
use crate::transcript::{NoticeLevel, Transcript};

pub const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const ESC_ARM_WINDOW: Duration = Duration::from_millis(800);
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);
const CTRL_C_QUIT_WINDOW: Duration = Duration::from_millis(1500);
const TIP_TTL: Duration = Duration::from_secs(4);

pub struct SlashCommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand { name: "help", usage: "/help", desc: "show help and tips" },
    SlashCommand { name: "keys", usage: "/keys", desc: "keyboard shortcuts" },
    SlashCommand { name: "new", usage: "/new [id]", desc: "start a fresh session" },
    SlashCommand { name: "resume", usage: "/resume [id]", desc: "resume a durable session from this workspace" },
    SlashCommand { name: "clear", usage: "/clear", desc: "clear the scrollback" },
    SlashCommand { name: "model", usage: "/model [id]", desc: "switch model · live in plugin mode" },
    SlashCommand { name: "mode", usage: "/mode [id]", desc: "agent mode · standard / code / minimal / creator" },
    SlashCommand { name: "effort", usage: "/effort [off|high|max]", desc: "reasoning effort for this session" },
    SlashCommand { name: "permission", usage: "/permission [preset]", desc: "permission preset picker · shift+tab cycles" },
    SlashCommand { name: "plan", usage: "/plan [off]", desc: "host plan mode (command passthrough)" },
    SlashCommand { name: "theme", usage: "/theme [dark|light]", desc: "toggle the DeepSeek palette" },
    SlashCommand { name: "session", usage: "/session", desc: "show session + runtime info" },
    SlashCommand { name: "logo", usage: "/logo", desc: "bring the whale back" },
    SlashCommand { name: "liang", usage: "/liang [on|off]", desc: "召唤小难梁 — 🤫 idle · ⌨︎ working" },
    SlashCommand { name: "quit", usage: "/quit", desc: "exit dsh-tui" },
];

pub const MODEL_PRESETS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4",
    "deepseek-v3.2",
    "deepseek-chat",
    "deepseek-reasoner",
];

/// The four stock agent modes the Web UI ships (id, display name, one-line
/// description). They seed the mode picker; plugin mode replaces them with
/// the host's real `agentPresets` roster, which may add custom presets.
pub const AGENT_MODES: &[(&str, &str, &str)] = &[
    ("standard", "Standard mode", "full coding agent · files, shell, search, skills, subagents"),
    ("code", "Code mode", "standard tools driven from one TypeScript program"),
    ("minimal", "Minimal mode", "two tools · persistent bash + str_replace_editor"),
    ("creator", "Creator mode", "standard + runtime inspection and preset authoring"),
];

/// The stock permission presets (id, one-line meaning) — the default table
/// `@deepseek-ai/dsh-permission-presets` ships. Shift+Tab cycles them;
/// `/permission <name>` passes any other id through for profiles with a
/// custom preset table (the host validates and lists what it knows).
pub const PERMISSION_PRESETS: &[(&str, &str)] = &[
    ("workspace-write", "write inside the workspace · wider actions ask for approval"),
    ("danger-full-access", "full file access · approval prompts off — trusted dirs only"),
];

/// Map common spellings onto the stock preset ids (`full` →
/// `danger-full-access`, `ws` → `workspace-write`, …).
pub fn normalize_permission(arg: &str) -> Option<&'static str> {
    match arg.trim().to_ascii_lowercase().as_str() {
        "workspace-write" | "workspace" | "write" | "ws" | "safe" | "sandbox" => {
            Some("workspace-write")
        }
        "danger-full-access" | "full-access" | "full" | "danger" | "yolo" => {
            Some("danger-full-access")
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Starting,
    Running,
}

pub struct Input {
    pub buf: String,
    pub cursor: usize, // char index
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub stash: String,
}

impl Input {
    fn new() -> Self {
        Input {
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            stash: String::new(),
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }

    pub fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.buf.insert(at, ch);
        self.cursor += 1;
        self.hist_pos = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_at(self.cursor);
        self.buf.insert_str(at, s);
        self.cursor += s.chars().count();
        self.hist_pos = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete_word_back(&mut self) {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let start = self.byte_at(i);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor = i;
    }

    fn kill_to_end(&mut self) {
        let at = self.byte_at(self.cursor);
        self.buf.truncate(at);
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.hist_pos = None;
    }

    fn set(&mut self, s: String) {
        self.cursor = s.chars().count();
        self.buf = s;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// One endpoint of a mouse selection in chat-layout coordinates: `line`
/// indexes the full wrapped layout (`ChatView::lines`), `col` is a display
/// cell column within the chat pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelPoint {
    pub line: usize,
    pub col: usize,
}

/// In-app mouse selection — the grok-build gesture: drag highlights,
/// releasing the button copies (选中完即 copy). `anchor` is where the drag
/// started; `head` follows the pointer and may precede the anchor.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: SelPoint,
    pub head: SelPoint,
}

impl Selection {
    /// (start, end) in document order; `end` is inclusive (the cell under
    /// the pointer is part of the selection).
    pub fn ordered(&self) -> (SelPoint, SelPoint) {
        if (self.head.line, self.head.col) < (self.anchor.line, self.anchor.col) {
            (self.head, self.anchor)
        } else {
            (self.anchor, self.head)
        }
    }

    fn is_caret(&self) -> bool {
        self.anchor == self.head
    }
}

/// Snapshot of the chat pane layout from the last draw — the seam that
/// mouse hit-testing and copy extraction read (grok-build's resolved
/// selection model, scaled way down): pane rect, index of the first
/// visible layout line, and the plain text of every layout line.
#[derive(Default)]
pub struct ChatView {
    pub area: ratatui::layout::Rect,
    pub top: usize,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Model,
    Effort,
    Mode,
    Permission,
    Session,
}

#[derive(Clone)]
pub struct PickerItem {
    pub id: String,
    pub label: String,
    pub meta: String,
    pub provider: Option<String>,
}

pub struct Picker {
    pub kind: PickerKind,
    pub title: String,
    pub sel: usize,
    pub items: Vec<PickerItem>,
}

/// Folded per-session mode state (from the durable event stream — the same
/// facts the Web UI chips read).
#[derive(Default, Clone)]
pub struct Modes {
    pub plan: bool,
    pub sandbox: Option<String>,
    pub approval: Option<String>,
    pub permission: Option<String>,
    pub agent_preset: Option<String>,
}

pub struct App {
    pub theme: Theme,
    pub transcript: Transcript,
    pub input: Input,
    pub state: RunState,
    pub state_note: String,
    /// Welcome banner (whale + wordmark) — shown until the first real prompt.
    pub show_banner: bool,
    /// Pixel-art Liang at the composer's right edge (`/liang` toggles him).
    pub pet_visible: bool,
    /// True when the terminal renders the pet as real pixels (kitty graphics
    /// protocol, emitted by `main`); false draws the half-block fallback.
    pub pet_pixels: bool,
    pub run_started: Option<Instant>,
    pub spinner_idx: usize,
    pub scroll_up: usize, // lines above the bottom; 0 = follow
    /// Mouse selection over the chat pane (drag-to-select, copy on release).
    pub sel: Option<Selection>,
    /// A left-button drag is in progress.
    selecting: bool,
    last_click: Option<(Instant, u16, u16)>,
    /// Filled by `ui::draw_chat` every frame.
    pub chat_view: ChatView,
    /// Sessions offered by the open `/resume` picker (id → file lookup).
    resume_candidates: Vec<crate::sessions::SessionSummary>,
    pub slash_sel: usize,
    pub picker: Option<Picker>,
    pub modes: Modes,
    pub tip: Option<(String, Instant)>,
    pub ambient_tip_idx: usize,
    pub ambient_tip_at: Instant,
    esc_armed: Option<Instant>,
    ctrl_c_armed: Option<Instant>,
    pub session_id: String,
    pub cfg: RuntimeConfig,
    pub demo: bool,
    /// dsh plugin mode: the host owns runtime, credentials, and catalog.
    pub attached: bool,
    pub quit: bool,
    pub queued: usize,
    shell_seq: u64,
    shell_pending: Vec<(u64, usize)>, // (id, cell idx)
    bus_tx: Sender<AppEvent>,
    pub server_info: Option<String>,
    pub needs_redraw: bool,
}

pub const AMBIENT_TIPS: &[&str] = &[
    "Tip · esc interrupts a running turn — your draft survives",
    "Tip · type ! to run a local shell command without the agent",
    "Tip · enter queues a follow-up while DeepSeek works; alt+enter sends it now",
    "Tip · ctrl+e expands every thought and tool result",
    "Tip · /mode picks the agent preset — standard · code · minimal · creator",
    "Tip · /theme light mirrors the Web UI light palette",
    "Tip · ask DeepSeek about the harness docs — e.g. \"how do I add a tool?\"",
    "Tip · /new starts a fresh durable session; the old JSONL log stays on disk",
];

impl App {
    pub fn new(
        theme: Theme,
        cfg: RuntimeConfig,
        session_id: String,
        demo: bool,
        attached: bool,
        bus_tx: Sender<AppEvent>,
    ) -> Self {
        App {
            theme,
            transcript: Transcript::new(session_id.clone()),
            input: Input::new(),
            state: RunState::Idle,
            state_note: String::new(),
            show_banner: true,
            pet_visible: true,
            pet_pixels: false,
            run_started: None,
            spinner_idx: 0,
            scroll_up: 0,
            sel: None,
            selecting: false,
            last_click: None,
            chat_view: ChatView::default(),
            resume_candidates: Vec::new(),
            slash_sel: 0,
            picker: None,
            modes: Modes::default(),
            tip: None,
            ambient_tip_idx: 0,
            ambient_tip_at: Instant::now(),
            esc_armed: None,
            ctrl_c_armed: None,
            session_id,
            cfg,
            demo,
            attached,
            quit: false,
            queued: 0,
            shell_seq: 0,
            shell_pending: Vec::new(),
            bus_tx,
            server_info: None,
            needs_redraw: true,
        }
    }

    pub fn spinner(&self) -> char {
        SPINNER[self.spinner_idx % SPINNER.len()]
    }

    pub fn tick(&mut self) {
        if self.state != RunState::Idle || self.transcript.streaming() {
            self.spinner_idx = self.spinner_idx.wrapping_add(1);
            self.needs_redraw = true;
        }
        if let Some((_, at)) = &self.tip {
            if at.elapsed() > TIP_TTL {
                self.tip = None;
                self.needs_redraw = true;
            }
        }
        if self.ambient_tip_at.elapsed() > Duration::from_secs(14) {
            self.ambient_tip_at = Instant::now();
            self.ambient_tip_idx = (self.ambient_tip_idx + 1) % AMBIENT_TIPS.len();
            if self.tip.is_none() {
                self.needs_redraw = true;
            }
        }
        // disarm expired chords
        if let Some(t) = self.esc_armed {
            if t.elapsed() > ESC_ARM_WINDOW {
                self.esc_armed = None;
            }
        }
        if let Some(t) = self.ctrl_c_armed {
            if t.elapsed() > CTRL_C_QUIT_WINDOW {
                self.ctrl_c_armed = None;
            }
        }
    }

    pub fn show_tip(&mut self, text: impl Into<String>) {
        self.tip = Some((text.into(), Instant::now()));
        self.needs_redraw = true;
    }

    pub fn slash_matches(&self) -> Vec<&'static SlashCommand> {
        if !self.input.buf.starts_with('/') || self.input.buf.contains(' ') {
            return Vec::new();
        }
        let prefix = &self.input.buf[1..];
        SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(prefix))
            .collect()
    }

    pub fn handle(&mut self, ev: AppEvent, ctl: &Controller) {
        match ev {
            AppEvent::Term(term) => self.handle_term(term, ctl),
            AppEvent::Rpc { method, params } => {
                for ui in parse_notification(&method, &params) {
                    // Fold per-session mode facts for the composer chips.
                    {
                        use crate::events::UiEvent as E;
                        match &ui {
                            E::PlanMode { session, active } if *session == self.session_id => {
                                self.modes.plan = *active;
                            }
                            E::SandboxMode { session, mode } if *session == self.session_id => {
                                self.modes.sandbox = Some(mode.clone());
                            }
                            E::ApprovalPolicy { session, policy }
                                if *session == self.session_id =>
                            {
                                self.modes.approval = Some(policy.clone());
                            }
                            E::PermissionPreset { session, preset }
                                if *session == self.session_id =>
                            {
                                self.modes.permission = Some(preset.clone());
                            }
                            E::AgentPreset { session, preset }
                                if *session == self.session_id =>
                            {
                                self.modes.agent_preset = Some(preset.clone());
                            }
                            _ => {}
                        }
                    }
                    if let crate::events::UiEvent::SessionStatus { session, running } = &ui {
                        if *session == self.session_id {
                            self.state = if *running {
                                RunState::Running
                            } else {
                                RunState::Idle
                            };
                            if *running {
                                if self.run_started.is_none() {
                                    self.run_started = Some(Instant::now());
                                }
                            } else {
                                self.run_started = None;
                                self.queued = 0;
                                self.state_note.clear();
                            }
                        }
                    }
                    if let crate::events::UiEvent::TurnStart { session, .. } = &ui {
                        if *session == self.session_id && self.queued > 0 {
                            self.queued = self.queued.saturating_sub(1);
                        }
                    }
                    self.transcript.apply(ui);
                }
                self.needs_redraw = true;
            }
            AppEvent::RuntimeStderr(_line) => {
                // kept in proto's tail buffer for diagnostics; stay quiet here
            }
            AppEvent::RuntimeExited(code) => {
                if self.state != RunState::Idle {
                    self.state = RunState::Idle;
                    self.run_started = None;
                }
                if let Some(c) = code {
                    if c != 0 {
                        self.transcript.push_notice(
                            NoticeLevel::Warn,
                            format!("runtime exited with code {c} — next prompt restarts it"),
                        );
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::Ctl(ctl_ev) => {
                match ctl_ev {
                    CtlEvent::Starting { runtime } => {
                        self.state = RunState::Starting;
                        self.run_started = Some(Instant::now());
                        self.state_note = "starting runtime".into();
                        let short = runtime.rsplit('/').next().unwrap_or(&runtime).to_string();
                        self.transcript
                            .push_notice(NoticeLevel::Info, format!("⛭ spawning {short}"));
                    }
                    CtlEvent::Ready { server } => {
                        self.server_info = Some(server.clone());
                        self.state_note = "waiting for model".into();
                        self.transcript
                            .push_notice(NoticeLevel::Info, format!("⛭ connected · {server}"));
                    }
                    CtlEvent::PromptQueued { .. } => {
                        if self.state == RunState::Starting {
                            self.state = RunState::Running;
                        }
                        self.state_note.clear();
                    }
                    CtlEvent::Error(err) => {
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.transcript.push_notice(NoticeLevel::Error, err);
                    }
                    CtlEvent::Interrupted => {
                        self.state = RunState::Idle;
                        self.run_started = None;
                        self.queued = 0;
                        self.transcript.push_notice(
                            NoticeLevel::Warn,
                            "interrupted — runtime stopped; the session log is preserved and the next prompt restarts it".into(),
                        );
                    }
                    CtlEvent::Catalog { models, presets } => {
                        if let Some(picker) = &mut self.picker {
                            match picker.kind {
                                PickerKind::Model if !models.is_empty() => {
                                    let current = self.cfg.model.clone();
                                    picker.items = models
                                        .into_iter()
                                        .map(|m| PickerItem {
                                            id: m.id.clone(),
                                            label: m.id,
                                            meta: format!(
                                                "{}{}",
                                                m.name,
                                                if m.vision { " · vision" } else { "" }
                                            ),
                                            provider: Some(m.provider),
                                        })
                                        .collect();
                                    picker.sel = picker
                                        .items
                                        .iter()
                                        .position(|i| i.id == current)
                                        .unwrap_or(0);
                                }
                                PickerKind::Mode if !presets.is_empty() => {
                                    let current = self
                                        .modes
                                        .agent_preset
                                        .clone()
                                        .unwrap_or_else(|| "standard".into());
                                    picker.items = presets
                                        .into_iter()
                                        .map(|p| PickerItem {
                                            id: p.id.clone(),
                                            label: p.name,
                                            meta: if p.broken {
                                                format!("⚠ broken · {}", p.description)
                                            } else {
                                                p.description
                                            },
                                            provider: None,
                                        })
                                        .collect();
                                    picker.sel = picker
                                        .items
                                        .iter()
                                        .position(|i| i.id == current)
                                        .unwrap_or(0);
                                }
                                _ => {}
                            }
                        }
                    }
                    CtlEvent::Efforts { efforts, default } => {
                        self.open_effort_picker(efforts, default);
                    }
                    CtlEvent::PresetSet { preset } => {
                        self.modes.agent_preset = Some(preset.clone());
                        self.transcript.push_notice(
                            NoticeLevel::Info,
                            format!("⚙ mode → {preset} · composes on this session's first prompt"),
                        );
                    }
                    CtlEvent::TuiOpDone(desc) => {
                        self.transcript.push_notice(NoticeLevel::Info, desc);
                    }
                    CtlEvent::TuiOpFailed(desc) => {
                        self.transcript.push_notice(NoticeLevel::Warn, desc);
                    }
                }
                self.needs_redraw = true;
            }
            AppEvent::ShellDone { id, code, output } => {
                if let Some(pos) = self.shell_pending.iter().position(|(sid, _)| *sid == id) {
                    let (_, cell) = self.shell_pending.remove(pos);
                    self.transcript.finish_shell(cell, code, output);
                    self.needs_redraw = true;
                }
            }
        }
    }

    fn handle_term(&mut self, ev: Event, ctl: &Controller) {
        match ev {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key, ctl),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(..) => self.needs_redraw = true,
            Event::Paste(text) => {
                self.input.insert_str(&text.replace('\n', " "));
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    /// grok-build mouse semantics, scaled down: wheel scrolls; left-drag
    /// selects with a live highlight (auto-scrolling at the pane edges) and
    /// copies on release; double-click selects & copies a word. Shift+drag
    /// bypasses capture in most terminals → native selection still works.
    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_by(3),
            MouseEventKind::ScrollDown => self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.needs_redraw = true;
                let Some(p) = self.chat_hit(mouse.column, mouse.row) else {
                    // Click outside the chat pane dismisses the highlight.
                    self.sel = None;
                    self.selecting = false;
                    return;
                };
                let double = self.last_click.take().is_some_and(|(at, x, y)| {
                    at.elapsed() < DOUBLE_CLICK_WINDOW
                        && x.abs_diff(mouse.column) <= 1
                        && y == mouse.row
                });
                self.last_click = Some((Instant::now(), mouse.column, mouse.row));
                if double {
                    self.select_word_at(p);
                } else {
                    self.sel = Some(Selection { anchor: p, head: p });
                    self.selecting = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.selecting => {
                // Edge auto-scroll (grok: compute_autoscroll): dragging
                // past the pane keeps scrolling while events arrive.
                let a = self.chat_view.area;
                if mouse.row < a.y {
                    self.scroll_by(2);
                } else if mouse.row >= a.y.saturating_add(a.height) {
                    self.scroll_by(-2);
                }
                let head = self.chat_clamp(mouse.column, mouse.row);
                if let Some(sel) = &mut self.sel {
                    sel.head = head;
                }
                self.needs_redraw = true;
            }
            MouseEventKind::Up(MouseButton::Left) if self.selecting => {
                self.selecting = false;
                self.finish_selection();
            }
            _ => {}
        }
    }

    /// Hit-test a screen cell against the chat pane; `None` outside it.
    fn chat_hit(&self, col: u16, row: u16) -> Option<SelPoint> {
        let a = self.chat_view.area;
        if self.chat_view.lines.is_empty()
            || col < a.x
            || col >= a.x.saturating_add(a.width)
            || row < a.y
            || row >= a.y.saturating_add(a.height)
        {
            return None;
        }
        Some(SelPoint {
            line: (self.chat_view.top + (row - a.y) as usize)
                .min(self.chat_view.lines.len() - 1),
            col: (col - a.x) as usize,
        })
    }

    /// Like `chat_hit`, but clamps to the pane so drags outside it still
    /// extend the selection to the nearest edge.
    fn chat_clamp(&self, col: u16, row: u16) -> SelPoint {
        let a = self.chat_view.area;
        let col = col.clamp(a.x, a.x.saturating_add(a.width.saturating_sub(1)));
        let row = row.clamp(a.y, a.y.saturating_add(a.height.saturating_sub(1)));
        self.chat_hit(col, row).unwrap_or(SelPoint { line: 0, col: 0 })
    }

    /// grok `finish_text_drag`: reconstruct the dragged text and copy it —
    /// the highlight persists only when something actually reached the
    /// clipboard path. A plain click (caret) just clears the highlight.
    fn finish_selection(&mut self) {
        self.needs_redraw = true;
        let Some(sel) = self.sel else { return };
        if sel.is_caret() {
            self.sel = None;
            return;
        }
        let text = self.selection_text(sel);
        if text.trim().is_empty() {
            self.sel = None;
            return;
        }
        self.copy_text(&text);
    }

    fn copy_text(&mut self, text: &str) {
        let chars = text.chars().count();
        if crate::clipboard::copy(text) {
            self.show_tip(format!("✓ copied {chars} chars — esc clears the highlight"));
        } else {
            self.show_tip("copy failed — hold shift and drag for the terminal's native selection");
        }
    }

    /// Extract the selected text from the layout snapshot: cell-range slices
    /// per line, trailing whitespace trimmed, joined with newlines.
    pub fn selection_text(&self, sel: Selection) -> String {
        let lines = &self.chat_view.lines;
        if lines.is_empty() {
            return String::new();
        }
        let (s, e) = sel.ordered();
        let last = lines.len() - 1;
        let (sl, el) = (s.line.min(last), e.line.min(last));
        let mut out = Vec::with_capacity(el - sl + 1);
        for (li, text) in lines.iter().enumerate().take(el + 1).skip(sl) {
            let c0 = if li == sl { s.col } else { 0 };
            let c1 = if li == el { e.col + 1 } else { usize::MAX };
            out.push(slice_by_cells(text, c0, c1).trim_end().to_string());
        }
        out.join("\n")
    }

    /// Double-click: select the whitespace-delimited word under the pointer
    /// and copy it right away (grok's word select & copy).
    fn select_word_at(&mut self, p: SelPoint) {
        let Some(line) = self.chat_view.lines.get(p.line) else {
            return;
        };
        let Some((col, width, word)) = word_span(line, p.col) else {
            self.sel = None;
            return;
        };
        self.sel = Some(Selection {
            anchor: SelPoint { line: p.line, col },
            head: SelPoint { line: p.line, col: col + width - 1 },
        });
        self.selecting = false;
        let word = word.clone();
        self.copy_text(&word);
    }

    pub fn scroll_by(&mut self, delta: i64) {
        let cur = self.scroll_up as i64;
        self.scroll_up = (cur + delta).max(0) as usize; // clamped to content in ui::draw
        self.needs_redraw = true;
    }

    fn handle_key(&mut self, key: KeyEvent, ctl: &Controller) {
        self.needs_redraw = true;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // --- model picker overlay steals input first (grok modal semantics)
        if self.picker.is_some() {
            self.handle_picker_key(key, ctl);
            return;
        }

        match key.code {
            KeyCode::Enter if alt => self.send_now(ctl),
            KeyCode::Enter => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let cmd = menu[self.slash_sel.min(menu.len() - 1)];
                    self.accept_slash(cmd, ctl);
                } else {
                    self.submit(ctl);
                }
            }
            KeyCode::Esc => self.handle_esc(ctl),
            KeyCode::Char('c') if ctrl => self.handle_ctrl_c(ctl),
            KeyCode::Char('q') if ctrl => self.quit = true,
            KeyCode::Char('l') if ctrl => {
                self.transcript.clear();
                self.sel = None;
                self.transcript
                    .push_notice(NoticeLevel::Info, "scrollback cleared".into());
            }
            KeyCode::Char('t') if ctrl => {
                self.theme = self.theme.toggled();
                self.show_tip(match self.theme.mode {
                    crate::theme::Mode::Dark => "theme: deepseek dark",
                    crate::theme::Mode::Light => "theme: deepseek light",
                });
            }
            KeyCode::Char('e') if ctrl => {
                self.transcript.expand_all = !self.transcript.expand_all;
                self.show_tip(if self.transcript.expand_all {
                    "expanded all thoughts and tool results"
                } else {
                    "collapsed all thoughts and tool results"
                });
            }
            KeyCode::Char('m') if ctrl => self.open_model_picker(ctl),
            KeyCode::BackTab => self.cycle_permission(ctl),
            KeyCode::Char('u') if ctrl => self.scroll_by(10),
            KeyCode::Char('d') if ctrl => self.scroll_by(-10),
            KeyCode::Char('k') if ctrl => self.input.kill_to_end(),
            KeyCode::Char('w') if ctrl => self.input.delete_word_back(),
            KeyCode::Char('a') if ctrl => self.input.cursor = 0,
            KeyCode::Char('e') if !ctrl && alt => self.input.cursor = self.input.buf.chars().count(),
            KeyCode::PageUp => self.scroll_by(20),
            KeyCode::PageDown => self.scroll_by(-20),
            KeyCode::Home if self.input.is_empty() => self.scroll_up = usize::MAX,
            KeyCode::End if self.input.is_empty() => self.scroll_up = 0,
            KeyCode::Home => self.input.cursor = 0,
            KeyCode::End => self.input.cursor = self.input.buf.chars().count(),
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            KeyCode::Left => self.input.cursor = self.input.cursor.saturating_sub(1),
            KeyCode::Right => {
                let max = self.input.buf.chars().count();
                self.input.cursor = (self.input.cursor + 1).min(max);
            }
            KeyCode::Backspace if alt => self.input.delete_word_back(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Tab => {
                let menu = self.slash_matches();
                if !menu.is_empty() {
                    let cmd = menu[self.slash_sel.min(menu.len() - 1)];
                    self.input.set(format!("/{} ", cmd.name));
                }
            }
            KeyCode::Char(ch) if !ctrl => {
                self.input.insert(ch);
                self.slash_sel = 0;
            }
            _ => {}
        }

        // slash menu selection with arrows while menu is visible
        if self.input.buf.starts_with('/') {
            let n = self.slash_matches().len();
            if n > 0 {
                match key.code {
                    KeyCode::Up => {
                        self.slash_sel = self.slash_sel.checked_sub(1).unwrap_or(n - 1);
                    }
                    KeyCode::Down => self.slash_sel = (self.slash_sel + 1) % n,
                    _ => {}
                }
            }
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent, ctl: &Controller) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let n = picker.items.len().max(1);
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Up => picker.sel = picker.sel.checked_sub(1).unwrap_or(n - 1),
            KeyCode::Down => picker.sel = (picker.sel + 1) % n,
            KeyCode::Enter => {
                let Some(item) = picker.items.get(picker.sel).cloned() else {
                    self.picker = None;
                    return;
                };
                let kind = picker.kind;
                self.picker = None;
                match kind {
                    PickerKind::Model => self.select_model(item, ctl),
                    PickerKind::Mode => self.set_mode(item.id, ctl),
                    PickerKind::Permission => self.set_permission(item.id, ctl),
                    PickerKind::Session => self.resume_session(&item.id, ctl),
                    PickerKind::Effort => {
                        let effort = item.id;
                        ctl.send(Cmd::SelectModel {
                            session_id: self.session_id.clone(),
                            provider: None,
                            model: None,
                            effort: Some(effort.clone()),
                        });
                        self.transcript.push_notice(
                            NoticeLevel::Info,
                            format!("reasoning effort → {effort}"),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn open_model_picker(&mut self, ctl: &Controller) {
        // Ask the host for its real catalog (plugin mode); seed the picker
        // with fallback presets / env snapshot meanwhile.
        ctl.send(Cmd::FetchCatalog);
        let mut items: Vec<PickerItem> = host_catalog_models()
            .unwrap_or_else(|| MODEL_PRESETS.iter().map(|s| s.to_string()).collect())
            .into_iter()
            .map(|id| PickerItem {
                id: id.clone(),
                label: id,
                meta: String::new(),
                provider: None,
            })
            .collect();
        if !items.iter().any(|i| i.id == self.cfg.model) {
            items.insert(
                0,
                PickerItem {
                    id: self.cfg.model.clone(),
                    label: self.cfg.model.clone(),
                    meta: String::new(),
                    provider: None,
                },
            );
        }
        let sel = items.iter().position(|i| i.id == self.cfg.model).unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Model,
            title: " model · enter select · esc close ".into(),
            sel,
            items,
        });
    }

    fn open_effort_picker(&mut self, efforts: Vec<String>, default: Option<String>) {
        let mut items: Vec<PickerItem> = efforts
            .into_iter()
            .map(|e| {
                let is_default = default.as_deref() == Some(e.as_str());
                PickerItem {
                    id: e.clone(),
                    label: e,
                    meta: if is_default { "default".into() } else { String::new() },
                    provider: None,
                }
            })
            .collect();
        if items.is_empty() {
            items = ["off", "high", "max"]
                .iter()
                .map(|e| PickerItem {
                    id: e.to_string(),
                    label: e.to_string(),
                    meta: String::new(),
                    provider: None,
                })
                .collect();
        }
        self.picker = Some(Picker {
            kind: PickerKind::Effort,
            title: " reasoning effort · enter select · esc close ".into(),
            sel: 0,
            items,
        });
    }

    /// `/resume`: list this workspace's durable sessions in a picker
    /// (grok-build's session picker, JSONL-backed).
    fn open_resume_picker(&mut self) {
        let sessions = crate::sessions::list_sessions(
            &self.cfg.session_root,
            &self.cfg.workspace,
            &self.session_id,
        );
        if sessions.is_empty() {
            self.transcript.push_notice(
                NoticeLevel::Info,
                "no durable sessions for this workspace yet — finish a turn and /resume finds it"
                    .into(),
            );
            return;
        }
        let items = sessions
            .iter()
            .map(|s| PickerItem {
                id: s.id.clone(),
                label: s.id.clone(),
                meta: format!(
                    "{} · {} turn{} · {}",
                    crate::sessions::age_label(s.modified),
                    s.turns,
                    if s.turns == 1 { "" } else { "s" },
                    s.preview
                ),
                provider: None,
            })
            .collect();
        self.resume_candidates = sessions;
        self.picker = Some(Picker {
            kind: PickerKind::Session,
            title: " resume session · enter select · esc close ".into(),
            sel: 0,
            items,
        });
    }

    /// Resume a durable session: replay its JSONL into the scrollback and
    /// point the next prompt at the same id — the runtime (or host dsh)
    /// keeps appending to the same log.
    fn resume_session(&mut self, id_or_prefix: &str, ctl: &Controller) {
        if self.resume_candidates.is_empty() {
            self.resume_candidates = crate::sessions::list_sessions(
                &self.cfg.session_root,
                &self.cfg.workspace,
                &self.session_id,
            );
        }
        let matches: Vec<crate::sessions::SessionSummary> = self
            .resume_candidates
            .iter()
            .filter(|s| s.id.starts_with(id_or_prefix))
            .cloned()
            .collect();
        let session = match matches.as_slice() {
            [one] => one.clone(),
            [] => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("no session matches “{id_or_prefix}” — /resume lists them"),
                );
                return;
            }
            many => match many.iter().find(|s| s.id == id_or_prefix) {
                Some(one) => one.clone(),
                None => {
                    self.transcript.push_notice(
                        NoticeLevel::Warn,
                        format!(
                            "“{id_or_prefix}” is ambiguous ({} matches) — /resume lists them",
                            many.len()
                        ),
                    );
                    return;
                }
            },
        };
        let events = match crate::sessions::read_session_events(&session.file) {
            Ok(events) => events,
            Err(err) => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("cannot read {}: {err:#}", session.file.display()),
                );
                return;
            }
        };

        self.session_id = session.id.clone();
        self.transcript.clear();
        self.transcript.set_root_session(session.id.clone());
        self.modes = Modes::default();
        self.show_banner = false;
        self.queued = 0;
        self.sel = None;
        let mut replayed = 0usize;
        for ev in &events {
            if ev.get("type").and_then(serde_json::Value::as_str) == Some("session") {
                continue; // header line
            }
            // Live, the TUI echoes user prompts locally and the event parser
            // skips them; on replay the log is the only source, so push here.
            if let Some(text) = crate::sessions::user_text(ev) {
                self.transcript.push_user(text, false);
                replayed += 1;
                continue;
            }
            self.handle(
                AppEvent::Rpc {
                    method: "session.event".into(),
                    params: serde_json::json!({ "sessionId": session.id, "event": ev }),
                },
                ctl,
            );
            replayed += 1;
        }
        self.state = RunState::Idle;
        self.run_started = None;
        self.scroll_up = 0;
        self.resume_candidates = Vec::new();
        self.transcript.push_notice(
            NoticeLevel::Info,
            format!(
                "⟲ resumed {} · {} turn{} · {replayed} events replayed — the next prompt continues it",
                session.id,
                session.turns,
                if session.turns == 1 { "" } else { "s" },
            ),
        );
        self.needs_redraw = true;
    }

    fn open_mode_picker(&mut self, ctl: &Controller) {
        // Ask the host for its real preset roster (plugin mode); seed the
        // picker with the four stock Web UI modes meanwhile.
        ctl.send(Cmd::FetchCatalog);
        let items: Vec<PickerItem> = AGENT_MODES
            .iter()
            .map(|(id, name, desc)| PickerItem {
                id: id.to_string(),
                label: name.to_string(),
                meta: desc.to_string(),
                provider: None,
            })
            .collect();
        let current = self.current_mode();
        let sel = items.iter().position(|i| i.id == current).unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Mode,
            title: " agent mode · enter select · esc close ".into(),
            sel,
            items,
        });
    }

    /// The effective agent mode: the folded `agent-preset/selected` fact, or
    /// the stock default composition (standard).
    pub fn current_mode(&self) -> String {
        self.modes
            .agent_preset
            .clone()
            .unwrap_or_else(|| "standard".into())
    }

    /// Pick the agent preset composed on this session's first prompt. The
    /// host locks it once the session agent exists (`/new` for a fresh one).
    fn set_mode(&mut self, preset: String, ctl: &Controller) {
        if self.modes.agent_preset.as_deref() == Some(preset.as_str()) {
            self.show_tip(format!("mode already {preset}"));
            return;
        }
        ctl.send(Cmd::SetPreset {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        self.show_tip(format!("mode → {preset} …"));
    }

    fn select_model(&mut self, item: PickerItem, ctl: &Controller) {
        let model = item.id;
        let provider = item.provider;
        if model != self.cfg.model {
            self.cfg.model = model.clone();
            if let Some(p) = &provider {
                self.cfg.provider = p.clone();
            }
            ctl.send(Cmd::SelectModel {
                session_id: self.session_id.clone(),
                provider,
                model: Some(model.clone()),
                effort: None,
            });
        }
        // Stage 2: offer efforts for the chosen model.
        ctl.send(Cmd::FetchEfforts {
            provider: self.cfg.provider.clone(),
            model: self.cfg.model.clone(),
        });
    }

    fn set_model(&mut self, model: String, ctl: &Controller) {
        if model == self.cfg.model {
            return;
        }
        self.cfg.model = model.clone();
        ctl.send(Cmd::SelectModel {
            session_id: self.session_id.clone(),
            provider: None,
            model: Some(model),
            effort: None,
        });
    }

    /// grok: Shift+Tab cycles the permission preset.
    fn cycle_permission(&mut self, ctl: &Controller) {
        let current = self.current_permission().to_string();
        let idx = PERMISSION_PRESETS
            .iter()
            .position(|(p, _)| *p == current)
            .unwrap_or(0);
        let next = PERMISSION_PRESETS[(idx + 1) % PERMISSION_PRESETS.len()].0.to_string();
        self.set_permission(next, ctl);
    }

    /// The effective permission preset: the folded `permission/preset` fact,
    /// or the harness default (workspace-write) before the session reports.
    pub fn current_permission(&self) -> &str {
        self.modes
            .permission
            .as_deref()
            .unwrap_or(PERMISSION_PRESETS[0].0)
    }

    /// Ask the host to switch this session's permission preset; the durable
    /// `permission/preset` event echoes back and folds the ⛨ chip. Before
    /// the first prompt the host stages the switch and applies it when the
    /// session is created.
    fn set_permission(&mut self, preset: String, ctl: &Controller) {
        if self.modes.permission.as_deref() == Some(preset.as_str()) {
            self.show_tip(format!("permission already {preset}"));
            return;
        }
        ctl.send(Cmd::SetPermission {
            session_id: self.session_id.clone(),
            preset: preset.clone(),
        });
        self.show_tip(format!("permission → {preset} …"));
    }

    /// `/permission` — the two stock presets with their meaning, the current
    /// one preselected (picker twin of the blind shift+tab cycle).
    fn open_permission_picker(&mut self) {
        let reported = self.modes.permission.clone();
        let current = self.current_permission().to_string();
        let items: Vec<PickerItem> = PERMISSION_PRESETS
            .iter()
            .map(|(id, desc)| {
                let mark = if reported.as_deref() == Some(*id) {
                    " · current"
                } else if reported.is_none() && *id == current {
                    " · default"
                } else {
                    ""
                };
                PickerItem {
                    id: id.to_string(),
                    label: id.to_string(),
                    meta: format!("{desc}{mark}"),
                    provider: None,
                }
            })
            .collect();
        let sel = items.iter().position(|i| i.id == current).unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::Permission,
            title: " permission preset · enter apply · esc close ".into(),
            sel,
            items,
        });
    }

    fn handle_esc(&mut self, ctl: &Controller) {
        if self.picker.is_some() {
            self.picker = None;
            return;
        }
        // A lingering copy highlight is dismissed first (idle only — while
        // running, esc keeps its interrupt meaning and clears it in passing).
        if self.sel.take().is_some() && matches!(self.state, RunState::Idle) {
            self.needs_redraw = true;
            return;
        }
        if self.input.buf.starts_with('/') && !self.slash_matches().is_empty() {
            self.input.clear();
            return;
        }
        match self.state {
            RunState::Running | RunState::Starting => {
                // grok: Esc cancels immediately; the draft survives.
                if ctl.interrupt_now() {
                    ctl.send(Cmd::Interrupt);
                    self.state_note = "cancelling".into();
                } else if ctl.is_attached() {
                    self.show_tip("plugin mode: the host turn cannot be interrupted from here");
                } else {
                    self.show_tip("demo turn — it finishes on its own");
                }
            }
            RunState::Idle => {
                if !self.input.is_empty() {
                    if self.esc_armed.take().is_some() {
                        self.input.history.push(self.input.buf.clone());
                        self.input.clear();
                        self.show_tip("draft cleared — ↑ recalls it");
                    } else {
                        self.esc_armed = Some(Instant::now());
                        self.show_tip("press esc again to clear the draft");
                    }
                }
            }
        }
    }

    fn handle_ctrl_c(&mut self, ctl: &Controller) {
        if !self.input.is_empty() {
            // grok: Ctrl+C clears the draft first, keeps the turn.
            self.input.history.push(self.input.buf.clone());
            self.input.clear();
            self.show_tip("draft cleared — ↑ recalls it");
            return;
        }
        if matches!(self.state, RunState::Running | RunState::Starting) {
            if ctl.interrupt_now() {
                ctl.send(Cmd::Interrupt);
                self.state_note = "cancelling".into();
            } else if ctl.is_attached() {
                self.show_tip("plugin mode: the host turn cannot be interrupted from here");
            }
            return;
        }
        if self.ctrl_c_armed.take().is_some() {
            self.quit = true;
        } else {
            self.ctrl_c_armed = Some(Instant::now());
            self.show_tip("press ctrl+c again to exit");
        }
    }

    fn history_prev(&mut self) {
        if !self.input.is_empty() && self.input.hist_pos.is_none() {
            return; // grok: history opens from an empty prompt
        }
        if self.input.history.is_empty() {
            return;
        }
        let pos = match self.input.hist_pos {
            None => {
                self.input.stash = self.input.buf.clone();
                self.input.history.len() - 1
            }
            Some(0) => 0,
            Some(p) => p - 1,
        };
        self.input.hist_pos = Some(pos);
        self.input.set(self.input.history[pos].clone());
    }

    fn history_next(&mut self) {
        let Some(pos) = self.input.hist_pos else {
            return;
        };
        if pos + 1 >= self.input.history.len() {
            self.input.hist_pos = None;
            let stash = std::mem::take(&mut self.input.stash);
            self.input.set(stash);
        } else {
            self.input.hist_pos = Some(pos + 1);
            self.input.set(self.input.history[pos + 1].clone());
        }
    }

    fn accept_slash(&mut self, cmd: &'static SlashCommand, ctl: &Controller) {
        let line = self.input.buf.clone();
        let rest = line
            .strip_prefix('/')
            .and_then(|s| s.strip_prefix(cmd.name))
            .unwrap_or("")
            .trim()
            .to_string();
        self.input.clear();
        self.slash_sel = 0;
        self.run_slash(cmd.name, &rest, ctl);
    }

    pub fn run_slash(&mut self, name: &str, arg: &str, ctl: &Controller) {
        match name {
            "help" => self.push_help(),
            "keys" => self.push_keys(),
            "clear" => {
                self.transcript.clear();
                self.sel = None;
                self.transcript
                    .push_notice(NoticeLevel::Info, "scrollback cleared".into());
            }
            "quit" => self.quit = true,
            "logo" => {
                self.show_banner = true;
                self.transcript.push_notice(
                    NoticeLevel::Info,
                    "🐳 the whale is back — it dives on your next prompt".into(),
                );
            }
            "liang" => {
                self.pet_visible = match arg {
                    "on" | "show" => true,
                    "off" | "hide" => false,
                    _ => !self.pet_visible,
                };
                let msg = if self.pet_visible {
                    "🤫 小难梁已召唤 — 安静，他在想 AGI · /liang 收回"
                } else {
                    "小难梁去隆基市场买卡了 — /liang 再次召唤"
                };
                self.transcript.push_notice(NoticeLevel::Info, msg.into());
            }
            "theme" => {
                let next = match arg {
                    "light" => Theme::light(),
                    "dark" => Theme::dark(),
                    _ => self.theme.toggled(),
                };
                self.theme = next;
            }
            "model" => {
                if arg.is_empty() {
                    self.open_model_picker(ctl);
                } else {
                    self.set_model(arg.to_string(), ctl);
                }
            }
            "mode" => {
                if arg.is_empty() {
                    self.open_mode_picker(ctl);
                } else {
                    self.set_mode(arg.to_string(), ctl);
                }
            }
            "new" => {
                let id = if arg.is_empty() {
                    format!("dsh-{}", timestamp())
                } else {
                    arg.to_string()
                };
                self.session_id = id.clone();
                self.transcript.set_root_session(id.clone());
                // Folded per-session facts (mode, permission, plan …) belong
                // to the old session; the new one reports its own on compose.
                self.modes = Modes::default();
                self.transcript
                    .push_notice(NoticeLevel::Info, format!("new session · {id} — /mode picks its agent preset"));
            }
            "session" => self.push_session_info(),
            "resume" => {
                if arg.is_empty() {
                    self.open_resume_picker();
                } else {
                    self.resume_session(arg, ctl);
                }
            }
            "effort" => {
                if arg.is_empty() {
                    ctl.send(Cmd::FetchEfforts {
                        provider: self.cfg.provider.clone(),
                        model: self.cfg.model.clone(),
                    });
                } else {
                    ctl.send(Cmd::SelectModel {
                        session_id: self.session_id.clone(),
                        provider: None,
                        model: None,
                        effort: Some(arg.to_string()),
                    });
                }
            }
            "permission" => {
                if arg.is_empty() {
                    self.open_permission_picker();
                } else if let Some(preset) = normalize_permission(arg) {
                    self.set_permission(preset.to_string(), ctl);
                } else {
                    // Not a stock spelling — pass through for custom preset
                    // tables; the host lists what it knows on a miss.
                    self.set_permission(arg.to_string(), ctl);
                }
            }
            "plan" => {
                // Host-command passthrough: dsh-commands parses it when the
                // profile mounts plan mode; the plan/mode event echoes back.
                let text = if arg.is_empty() {
                    "/plan".to_string()
                } else {
                    format!("/plan {arg}")
                };
                self.send_agent_text(text, ctl);
            }
            other => {
                self.transcript.push_notice(
                    NoticeLevel::Warn,
                    format!("unknown command /{other} — /help lists commands"),
                );
            }
        }
    }

    fn push_help(&mut self) {
        let text = "\
DeepSeek Build (dsh-tui) — deepseek-harness in your terminal

  enter        send · queues a follow-up while a turn runs
  alt+enter    interrupt the turn and send now
  esc          interrupt (draft survives) · 2× clears the draft
  ctrl+c       clear draft · interrupt · 2× quits
  shift+tab    cycle permission (workspace-write ⇄ full access)
               · /permission opens the preset picker
  ctrl+m       model picker (host catalog) → effort picker
  /mode        agent mode · standard / code / minimal / creator
               (picked before a session's first prompt · /new to change)
  /effort      reasoning effort · /permission preset · /plan host plan mode
  /resume      pick up a durable session — transcript replays, log continues
  !cmd         run a local shell command (not the agent)
  ctrl+e       expand thoughts + tool output   ctrl+l clear
  pgup/pgdn    scroll · mouse wheel works · end follows the tail
  mouse drag   select text — copied on release · 2×click copies a word
               (hold shift to use the terminal's native selection)

Info shown per turn: streamed reasoning, streamed answer, every tool
call with its result, injected context, subagent lifecycles, token
usage (incl. cache hits), and the turn end reason.";
        self.transcript.push_notice(NoticeLevel::Info, text.into());
    }

    fn push_keys(&mut self) {
        let text = "\
keys — grok-build homage set
  enter send · alt+enter send-now · esc / ctrl+c cancel semantics
  ↑ history (empty prompt) · tab completes /commands
  ctrl+m model picker · ctrl+t theme · ctrl+e expand · ctrl+l clear
  ctrl+u/ctrl+d half-page · pgup/pgdn page · home/end top/tail
  ctrl+a/ctrl+k/ctrl+w readline editing · alt+backspace word
  drag select-copies · 2×click word-copies · shift+drag native select";
        self.transcript.push_notice(NoticeLevel::Info, text.into());
    }

    fn push_session_info(&mut self) {
        let creds = if self.demo {
            "demo mode (no API calls)".to_string()
        } else if self.cfg.has_credentials() {
            match self.cfg.credential_source() {
                Some(src) => format!("api key present · {src}"),
                None => "api key present".to_string(),
            }
        } else {
            "DEEPSEEK_API_KEY not set".to_string()
        };
        let text = format!(
            "session · {}\nprovider · {} / {}\nmode · {}{}\nworkspace · {}\nsession root · {}\nruntime · {}\nserver · {}\ncredentials · {}\ntokens · ↑{} ↓{} (cached {}, reasoning {})",
            self.session_id,
            self.cfg.provider,
            self.cfg.model,
            self.current_mode(),
            if self.modes.agent_preset.is_none() { " (default)" } else { "" },
            self.cfg.workspace,
            self.cfg.session_root,
            self.cfg.bin,
            self.server_info.as_deref().unwrap_or("not started"),
            creds,
            self.transcript.usage.input,
            self.transcript.usage.output,
            self.transcript.usage.cached,
            self.transcript.usage.reasoning,
        );
        self.transcript.push_notice(NoticeLevel::Info, text);
    }

    fn submit(&mut self, ctl: &Controller) {
        let text = self.input.buf.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(cmdline) = text.strip_prefix('/') {
            let mut parts = cmdline.splitn(2, ' ');
            let name = parts.next().unwrap_or("").to_string();
            let arg = parts.next().unwrap_or("").trim().to_string();
            self.input.history.push(text.clone());
            self.input.clear();
            self.run_slash(&name, &arg, ctl);
            return;
        }
        if let Some(cmd) = text.strip_prefix('!') {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                self.input.history.push(text.clone());
                self.input.clear();
                self.run_local_shell(cmd);
            }
            return;
        }

        self.input.history.push(text.clone());
        self.input.clear();
        self.send_agent_text(text, ctl);
    }

    /// Send raw text as an agent prompt (shared by submit and command
    /// passthroughs like /plan).
    fn send_agent_text(&mut self, text: String, ctl: &Controller) {
        self.show_banner = false;
        let running = matches!(self.state, RunState::Running | RunState::Starting);
        self.transcript.push_user(text.clone(), running);
        if running {
            self.queued += 1;
            self.show_tip("queued — lands after this turn · alt+enter would send now");
        } else {
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
            self.state_note = if self.demo { String::new() } else { "contacting runtime".into() };
        }
        self.scroll_up = 0;
        ctl.send(Cmd::Prompt {
            session_id: self.session_id.clone(),
            text,
        });
    }

    /// grok send-now: cancel the current turn and run this message next.
    fn send_now(&mut self, ctl: &Controller) {
        let text = self.input.buf.trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut interrupted = true;
        if matches!(self.state, RunState::Running | RunState::Starting) {
            interrupted = ctl.interrupt_now();
            if interrupted {
                ctl.send(Cmd::Interrupt);
            }
        }
        self.input.history.push(text.clone());
        self.input.clear();
        self.show_banner = false;
        self.transcript.push_user(text.clone(), !interrupted);
        if !interrupted && matches!(self.state, RunState::Running | RunState::Starting) {
            self.queued += 1;
            self.show_tip("cannot interrupt here — queued for the next turn instead");
        } else {
            self.state = RunState::Starting;
            self.run_started = Some(Instant::now());
        }
        self.scroll_up = 0;
        ctl.send(Cmd::Prompt {
            session_id: self.session_id.clone(),
            text,
        });
    }

    /// Submit a prompt programmatically (used by DSH_TUI_AUTOPROMPT).
    pub fn auto_prompt(&mut self, text: &str, ctl: &Controller) {
        self.show_banner = false;
        self.input.set(text.to_string());
        self.submit(ctl);
    }

    fn run_local_shell(&mut self, cmd: String) {
        self.shell_seq += 1;
        let id = self.shell_seq;
        let cell = self.transcript.push_shell(cmd.clone());
        self.shell_pending.push((id, cell));
        let tx = self.bus_tx.clone();
        let cwd = self.cfg.workspace.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("sh")
                .arg("-lc")
                .arg(&cmd)
                .current_dir(&cwd)
                .output();
            let (code, text) = match out {
                Ok(o) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                    let e = String::from_utf8_lossy(&o.stderr);
                    if !e.trim().is_empty() {
                        if !s.is_empty() {
                            s.push('\n');
                        }
                        s.push_str(&e);
                    }
                    const LIMIT: usize = 16 * 1024;
                    if s.len() > LIMIT {
                        let mut cut = LIMIT;
                        while cut > 0 && !s.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        s.truncate(cut);
                        s.push_str("\n… (truncated)");
                    }
                    (o.status.code(), s)
                }
                Err(err) => (None, format!("failed to run: {err}")),
            };
            let _ = tx.send(AppEvent::ShellDone {
                id,
                code,
                output: text,
            });
        });
    }
}

pub fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:x}")
}

/// Model ids from the host catalog snapshot: either inline JSON in
/// `DSH_TUI_MODELS` or a JSON file at `DSH_TUI_MODELS_FILE` (written by the
/// dsh plugin shim and refreshed on llm registry changes). Accepts
/// `["model-id", ...]` or `[{"id": "...", ...}, ...]`.
pub fn host_catalog_models() -> Option<Vec<String>> {
    let raw = match std::env::var("DSH_TUI_MODELS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            let path = std::env::var("DSH_TUI_MODELS_FILE").ok()?;
            std::fs::read_to_string(path).ok()?
        }
    };
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let arr = value.as_array()?;
    let mut out = Vec::new();
    for item in arr {
        match item {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Object(_) => {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                }
            }
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Slice `s` by display-cell range `[c0, c1)`: a char is included when its
/// cell span overlaps the range (so a double-width char straddling the
/// boundary is kept — matching what the highlight visually covers).
pub(crate) fn slice_by_cells(s: &str, c0: usize, c1: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0).max(1);
        if w + cw > c0 && w < c1 {
            out.push(ch);
        }
        w += cw;
        if w >= c1 {
            break;
        }
    }
    out
}

/// The whitespace-delimited word covering display column `col` of `line`:
/// `(start_col, cell_width, word)`. `None` on whitespace or past the end.
pub(crate) fn word_span(line: &str, col: usize) -> Option<(usize, usize, String)> {
    let cw = |ch: char| ch.width().unwrap_or(0).max(1);
    let chars: Vec<char> = line.chars().collect();
    let mut w = 0usize;
    let mut hit = None;
    for (i, ch) in chars.iter().enumerate() {
        if col < w + cw(*ch) {
            hit = Some(i);
            break;
        }
        w += cw(*ch);
    }
    let i = hit?;
    if chars[i].is_whitespace() {
        return None;
    }
    let (mut a, mut b) = (i, i);
    while a > 0 && !chars[a - 1].is_whitespace() {
        a -= 1;
    }
    while b + 1 < chars.len() && !chars[b + 1].is_whitespace() {
        b += 1;
    }
    let start_col: usize = chars[..a].iter().copied().map(cw).sum();
    let width: usize = chars[a..=b].iter().copied().map(cw).sum();
    Some((start_col, width, chars[a..=b].iter().collect()))
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    use std::path::PathBuf;

    fn test_app_with_root(root: &str, workspace: &str) -> (App, Controller) {
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: workspace.into(),
            session_root: root.into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
        let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
        let app = App::new(Theme::dark(), cfg, "dsh-current".into(), true, false, tx);
        (app, ctl)
    }

    fn write_fixture_session(root: &PathBuf, id: &str) {
        let dir = root
            .join(crate::sessions::workspace_slug("/w"))
            .join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let lines = [
            format!(r#"{{"type":"session","version":0,"id":"{id}","createdAt":1,"cwd":"/w"}}"#),
            r#"{"type":"permission/preset","seq":0,"data":{"preset":"workspace-write"}}"#.into(),
            r#"{"type":"turn/start","seq":1,"data":{"turn":1}}"#.into(),
            r#"{"type":"user/message","seq":2,"data":{"content":[{"text":"修复失败的测试","type":"text"}],"source":{"kind":"user"},"role":"user","id":"m1"}}"#.into(),
            r#"{"type":"assistant/chunk","seq":3,"data":{"chunk":{"type":"usage","usage":{"inputTokens":10,"outputTokens":5}}}}"#.into(),
            r#"{"type":"assistant/message","seq":4,"data":{"message":{"content":[{"type":"text","text":"tests are green now"}],"source":{"model":"deepseek-v4-flash"}}}}"#.into(),
            r#"{"type":"turn/end","seq":5,"data":{"reason":"completed"}}"#.into(),
        ];
        std::fs::write(dir.join("session.jsonl"), lines.join("\n")).unwrap();
    }

    fn tmp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dsh-resume-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn resume_replays_transcript_modes_and_usage() {
        let root = tmp_root("replay");
        write_fixture_session(&root, "dsh-past");
        let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");

        app.run_slash("resume", "dsh-past", &ctl);

        assert_eq!(app.session_id, "dsh-past", "active session switched");
        assert!(!app.show_banner, "banner dismissed on resume");
        assert_eq!(app.modes.permission.as_deref(), Some("workspace-write"));
        assert_eq!(app.transcript.usage.input, 10);
        assert_eq!(app.transcript.usage.output, 5);
        let text = app
            .transcript
            .lines(&Theme::dark(), 80, ' ')
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<String>();
        assert!(text.contains("修复失败的测试"), "user prompt replayed:\n{text}");
        assert!(text.contains("tests are green now"), "assistant reply replayed:\n{text}");
        assert!(text.contains("resumed dsh-past"), "resume notice shown:\n{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resume_picker_lists_sessions_and_prefix_resolves() {
        let root = tmp_root("picker");
        write_fixture_session(&root, "dsh-alpha");
        let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");

        app.run_slash("resume", "", &ctl);
        let picker = app.picker.as_ref().expect("picker opens");
        assert!(matches!(picker.kind, PickerKind::Session));
        assert_eq!(picker.items[0].id, "dsh-alpha");
        assert!(picker.items[0].meta.contains("1 turn"), "{}", picker.items[0].meta);
        assert!(picker.items[0].meta.contains("修复失败的测试"));
        app.picker = None;

        // unique prefix resolves; unknown id warns and keeps the session
        app.run_slash("resume", "dsh-al", &ctl);
        assert_eq!(app.session_id, "dsh-alpha");
        app.run_slash("resume", "nope", &ctl);
        assert_eq!(app.session_id, "dsh-alpha", "unknown prefix leaves session");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resume_with_no_sessions_notices_instead_of_picker() {
        let root = tmp_root("empty");
        let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");
        app.run_slash("resume", "", &ctl);
        assert!(app.picker.is_none(), "no picker without sessions");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn view(lines: &[&str]) -> ChatView {
        ChatView {
            area: ratatui::layout::Rect::new(1, 0, 60, 10),
            top: 0,
            lines: lines.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn sel(a: (usize, usize), h: (usize, usize)) -> Selection {
        Selection {
            anchor: SelPoint { line: a.0, col: a.1 },
            head: SelPoint { line: h.0, col: h.1 },
        }
    }

    fn test_app() -> App {
        let cfg = RuntimeConfig {
            bin: "dsh-runtime".into(),
            cordis: "cordis".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        App::new(
            crate::theme::Theme::dark(),
            cfg,
            "dsh-test".into(),
            true,
            false,
            tx,
        )
    }

    #[test]
    fn slice_by_cells_handles_wide_chars() {
        assert_eq!(slice_by_cells("hello world", 6, 11), "world");
        // 选=2 cells: [0,2) 中=[2,4) 即=[4,6)
        assert_eq!(slice_by_cells("选中即copy", 2, 6), "中即");
        // a boundary-straddling wide char is kept
        assert_eq!(slice_by_cells("选中", 1, 3), "选中");
        assert_eq!(slice_by_cells("abc", 0, usize::MAX), "abc");
    }

    #[test]
    fn selection_text_joins_lines_and_orders_reverse_drags() {
        let mut app = test_app();
        app.chat_view = view(&["first line  ", "second", "third"]);
        // forward drag: line0 col6 → line2 col2 (inclusive)
        let fwd = app.selection_text(sel((0, 6), (2, 2)));
        assert_eq!(fwd, "line\nsecond\nthi");
        // dragging upward yields the same text
        let rev = app.selection_text(sel((2, 2), (0, 6)));
        assert_eq!(fwd, rev);
    }

    #[test]
    fn chat_hit_maps_screen_cells_to_layout_lines() {
        let mut app = test_app();
        app.chat_view = view(&["a", "b", "c", "d"]);
        app.chat_view.top = 2;
        let p = app.chat_hit(3, 1).expect("inside pane");
        assert_eq!((p.line, p.col), (3, 2)); // top=2 + row 1, col 3-x(1)
        assert!(app.chat_hit(0, 0).is_none(), "left of pane");
        assert!(app.chat_hit(3, 10).is_none(), "below pane");
    }

    #[test]
    fn word_span_finds_word_under_column() {
        let (col, width, word) = word_span("run cargo test now", 6).expect("word");
        assert_eq!((col, width, word.as_str()), (4, 5, "cargo"));
        assert!(word_span("run cargo", 3).is_none(), "whitespace");
        assert!(word_span("run", 99).is_none(), "past end");
        let (col, width, word) = word_span("选中即复制 ok", 4).expect("cjk word");
        assert_eq!((col, width, word.as_str()), (0, 10, "选中即复制"));
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;
    use crate::bus::CatalogPreset;
    use std::sync::mpsc::Receiver;

    fn test_app() -> (App, Controller, Receiver<AppEvent>) {
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
        let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
        let app = App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx);
        (app, ctl, rx)
    }

    #[test]
    fn slash_mode_opens_picker_with_the_four_stock_modes() {
        let (mut app, ctl, _rx) = test_app();
        app.run_slash("mode", "", &ctl);
        let picker = app.picker.as_ref().expect("mode picker opens");
        assert!(matches!(picker.kind, PickerKind::Mode));
        let ids: Vec<&str> = picker.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["standard", "code", "minimal", "creator"]);
        assert_eq!(picker.sel, 0, "defaults to standard");
        assert_eq!(picker.items[0].label, "Standard mode");
    }

    #[test]
    fn host_catalog_replaces_mode_picker_items() {
        let (mut app, ctl, _rx) = test_app();
        app.modes.agent_preset = Some("code".into());
        app.run_slash("mode", "", &ctl);
        app.handle(
            AppEvent::Ctl(CtlEvent::Catalog {
                models: Vec::new(),
                presets: vec![
                    CatalogPreset { id: "standard".into(), name: "Standard mode".into(), description: "full".into(), broken: false },
                    CatalogPreset { id: "code".into(), name: "Code mode".into(), description: "ts".into(), broken: false },
                    CatalogPreset { id: "custom".into(), name: "Custom".into(), description: "mine".into(), broken: true },
                ],
            }),
            &ctl,
        );
        let picker = app.picker.as_ref().expect("picker still open");
        assert_eq!(picker.items.len(), 3);
        assert_eq!(picker.sel, 1, "selection lands on the current mode");
        assert!(picker.items[2].meta.contains("broken"));
    }

    #[test]
    fn demo_mode_selection_round_trips_the_durable_event() {
        let (mut app, ctl, rx) = test_app();
        app.run_slash("mode", "minimal", &ctl);
        // The demo controller synthesizes the agent-preset/selected fact.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.modes.agent_preset.is_none() {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("demo preset event before timeout");
            let ev = rx.recv_timeout(remaining).expect("bus event");
            app.handle(ev, &ctl);
        }
        assert_eq!(app.modes.agent_preset.as_deref(), Some("minimal"));
        assert_eq!(app.current_mode(), "minimal");
    }

    #[test]
    fn preset_ack_folds_the_chip_and_new_session_resets_it() {
        let (mut app, ctl, _rx) = test_app();
        app.handle(
            AppEvent::Ctl(CtlEvent::PresetSet { preset: "creator".into() }),
            &ctl,
        );
        assert_eq!(app.modes.agent_preset.as_deref(), Some("creator"));
        app.run_slash("new", "fresh", &ctl);
        assert_eq!(app.session_id, "fresh");
        assert_eq!(app.modes.agent_preset, None, "/new starts modeless");
        assert_eq!(app.current_mode(), "standard");
    }

    #[test]
    fn slash_permission_opens_picker_marking_current() {
        let (mut app, ctl, _rx) = test_app();
        app.run_slash("permission", "", &ctl);
        let picker = app.picker.as_ref().expect("permission picker opens");
        assert!(matches!(picker.kind, PickerKind::Permission));
        let ids: Vec<&str> = picker.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["workspace-write", "danger-full-access"]);
        assert_eq!(picker.sel, 0, "harness default preselected");
        assert!(picker.items[0].meta.contains("default"), "unreported → marked default");

        app.modes.permission = Some("danger-full-access".into());
        app.run_slash("permission", "", &ctl);
        let picker = app.picker.as_ref().expect("picker reopens");
        assert_eq!(picker.sel, 1, "selection lands on the reported preset");
        assert!(picker.items[1].meta.contains("current"));
    }

    #[test]
    fn permission_aliases_normalize() {
        assert_eq!(normalize_permission("full"), Some("danger-full-access"));
        assert_eq!(normalize_permission("YOLO"), Some("danger-full-access"));
        assert_eq!(normalize_permission(" ws "), Some("workspace-write"));
        assert_eq!(normalize_permission("read-only"), None, "custom ids pass through");
    }

    #[test]
    fn slash_permission_alias_round_trips_the_durable_event() {
        let (mut app, ctl, rx) = test_app();
        app.run_slash("permission", "full", &ctl);
        // The demo controller synthesizes the permission/sandbox/approval triplet.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.modes.permission.is_none() || app.modes.approval.is_none() {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("demo permission event before timeout");
            let ev = rx.recv_timeout(remaining).expect("bus event");
            app.handle(ev, &ctl);
        }
        assert_eq!(app.modes.permission.as_deref(), Some("danger-full-access"));
        assert_eq!(app.modes.approval.as_deref(), Some("never"));
    }

    #[test]
    fn shift_tab_cycles_between_the_stock_presets() {
        let (mut app, ctl, rx) = test_app();
        assert_eq!(app.current_permission(), "workspace-write", "assumed default");
        app.cycle_permission(&ctl);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.modes.permission.as_deref() != Some("danger-full-access") {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("cycle lands on full access");
            let ev = rx.recv_timeout(remaining).expect("bus event");
            app.handle(ev, &ctl);
        }
        app.cycle_permission(&ctl);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while app.modes.permission.as_deref() != Some("workspace-write") {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("cycle returns to workspace-write");
            let ev = rx.recv_timeout(remaining).expect("bus event");
            app.handle(ev, &ctl);
        }
        assert_eq!(app.current_permission(), "workspace-write");
    }
}
