//! Transcript model: the scrollback cells and their rendering to styled lines.
//!
//! Mirrors what the deepseek-harness Web UI surfaces for a session: user
//! prompts, streaming reasoning, streaming assistant text, tool calls with
//! results, injected context, subagent lifecycle, usage accounting, and turn
//! outcomes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::events::UiEvent;
use crate::theme::Theme;

/// Collapsed tool preview height; click toggles full expansion. Mouse wheel
/// always belongs to the outer transcript.
pub const TOOL_VIEWPORT: usize = 4;
const COLLAPSED_SHELL_LINES: usize = 12;
const COLLAPSED_REASONING_PREVIEW: usize = 2;
/// Thumbnail width in cells (PNG images reserve a box of this many columns).
const THUMB_COLS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug)]
pub enum CellKind {
    User {
        text: String,
        queued: bool,
    },
    Image {
        name: String,
        caption: String,
        /// Display path for the no-thumbnail fallback.
        path: String,
        /// Encoded raster bytes (PNG for the kitty thumbnail path).
        data: Arc<[u8]>,
        id: u32,
        queued: bool,
    },
    Reasoning {
        text: String,
        done: bool,
        started: Instant,
        seconds: Option<f32>,
        agent: Option<String>,
    },
    Assistant {
        text: String,
        done: bool,
        model: Option<String>,
        agent: Option<String>,
    },
    Tool {
        name: String,
        title: String,
        result: String,
        /// None while running.
        ok: Option<bool>,
        error: Option<String>,
        agent: Option<String>,
    },
    Shell {
        command: String,
        /// None while running.
        output: Option<(Option<i32>, String)>,
    },
    Injected {
        source: String,
        preview: String,
    },
    /// Latest standard ACP plan snapshot. Replaced in place as statuses move.
    Plan {
        summary: String,
    },
    Notice {
        level: NoticeLevel,
        text: String,
    },
    /// A notice rendered through the markdown pipeline (headings, tables,
    /// inline code) instead of plain wrapped text — used by `/keys`.
    MarkdownNotice {
        text: String,
    },
}

pub struct Cell {
    pub kind: CellKind,
    pub expanded: bool,
}

impl Cell {
    fn new(kind: CellKind) -> Self {
        Cell {
            kind,
            expanded: false,
        }
    }
}

/// One image the UI should render as a kitty-graphics thumbnail, positioned
/// by its reserved line range (`line` is relative to the transcript layout;
/// the chat pane adds its banner offset).
pub struct ImageShot {
    pub id: u32,
    pub line: usize,
    pub rows: usize,
    pub cols: usize,
    pub data: Arc<[u8]>,
}

/// A rendered transcript: styled lines plus, for each line, the index of the
/// transcript cell that owns it (only tool cells report ownership, so mouse
/// clicks can toggle a specific tool preview).
pub struct TranscriptLayout {
    pub lines: Vec<Line<'static>>,
    pub owners: Vec<Option<usize>>,
    pub images: Vec<ImageShot>,
}

#[derive(Default, Clone, Copy)]
pub struct UsageTotals {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub reasoning: u64,
}

/// Native transcript timing/step facts used by session state and tests. The
/// LLM usage/timing details surface through `/session` (and the Client-side
/// `acpSessionStats` service for plugins), not a persistent status row.
#[derive(Default, Clone, Copy)]
pub struct SessionStats {
    pub turns: u64,
    pub steps: u64,
    /// Total turn wall time (turn/start → turn/end).
    pub turn_millis: u64,
    /// Time spent inside tool calls (tool/call → tool/result).
    pub tool_millis: u64,
    /// Sum of per-turn time-to-first-token, plus how many turns were sampled.
    pub ttft_total_millis: u64,
    pub ttft_count: u64,
}

pub struct Transcript {
    pub cells: Vec<Cell>,
    root_session: String,
    open_assistant: HashMap<String, usize>,
    open_reasoning: HashMap<String, usize>,
    tools: HashMap<String, usize>,
    agents: HashMap<String, String>,
    agent_seq: usize,
    image_seq: u32,
    plan_cell: Option<usize>,
    pub usage: UsageTotals,
    pub stats: SessionStats,
    turn_started: Option<Instant>,
    tool_started: HashMap<String, Instant>,
    ttft_pending: bool,
    pub last_finish: Option<String>,
    /// Provenance-reported model of the last assembled assistant message —
    /// the ground truth of what actually answered.
    pub last_model: Option<String>,
    pub expand_all: bool,
}

impl Transcript {
    pub fn new(root_session: String) -> Self {
        Transcript {
            cells: Vec::new(),
            root_session,
            open_assistant: HashMap::new(),
            open_reasoning: HashMap::new(),
            tools: HashMap::new(),
            agents: HashMap::new(),
            agent_seq: 0,
            image_seq: 0,
            plan_cell: None,
            usage: UsageTotals::default(),
            stats: SessionStats::default(),
            turn_started: None,
            tool_started: HashMap::new(),
            ttft_pending: false,
            last_finish: None,
            last_model: None,
            expand_all: false,
        }
    }

    pub fn set_root_session(&mut self, session: String) {
        self.root_session = session;
        self.open_assistant.clear();
        self.open_reasoning.clear();
        self.tools.clear();
        self.agents.clear();
        self.plan_cell = None;
        self.usage = UsageTotals::default();
        self.stats = SessionStats::default();
        self.turn_started = None;
        self.tool_started.clear();
        self.ttft_pending = false;
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.open_assistant.clear();
        self.open_reasoning.clear();
        self.tools.clear();
        self.plan_cell = None;
        self.last_finish = None;
    }

    fn agent_label(&self, session: &str) -> Option<String> {
        if session == self.root_session || session.is_empty() {
            None
        } else {
            self.agents
                .get(session)
                .cloned()
                .or_else(|| Some("agent".into()))
        }
    }

    pub fn push_user(&mut self, text: String, queued: bool) {
        self.cells.push(Cell::new(CellKind::User { text, queued }));
    }

    /// Record a user-sent image (bytes kept for the kitty thumbnail path).
    pub fn push_image(
        &mut self,
        name: String,
        caption: String,
        path: String,
        data: Arc<[u8]>,
        queued: bool,
    ) {
        self.image_seq += 1;
        let id = self.image_seq;
        self.cells.push(Cell::new(CellKind::Image {
            name,
            caption,
            path,
            data,
            id,
            queued,
        }));
    }

    pub fn push_notice(&mut self, level: NoticeLevel, text: String) {
        self.cells.push(Cell::new(CellKind::Notice { level, text }));
    }

    /// Push a notice rendered through the markdown pipeline (tables,
    /// headings, inline code). No `· ` prefix and no manual wrapping: the
    /// markdown renderer owns line layout and truncation.
    pub fn push_markdown(&mut self, text: String) {
        self.cells
            .push(Cell::new(CellKind::MarkdownNotice { text }));
    }

    pub fn push_shell(&mut self, command: String) -> usize {
        self.cells.push(Cell::new(CellKind::Shell {
            command,
            output: None,
        }));
        self.cells.len() - 1
    }

    pub fn finish_shell(&mut self, idx: usize, code: Option<i32>, output: String) {
        if let Some(cell) = self.cells.get_mut(idx) {
            if let CellKind::Shell { output: slot, .. } = &mut cell.kind {
                *slot = Some((code, output));
            }
        }
    }

    /// Mark one client-owned queued prompt group as delivered.
    pub fn mark_prompt_delivered(&mut self, cells: &[usize]) {
        for &index in cells {
            let Some(cell) = self.cells.get_mut(index) else {
                continue;
            };
            match &mut cell.kind {
                CellKind::User { queued, .. } | CellKind::Image { queued, .. } => *queued = false,
                _ => {}
            }
        }
    }

    /// Mark a Send Now bubble as a client-owned FIFO item after the agent
    /// rejects concurrent `session/prompt` delivery.
    pub fn mark_prompt_queued(&mut self, cells: &[usize]) {
        for &index in cells {
            let Some(cell) = self.cells.get_mut(index) else {
                continue;
            };
            match &mut cell.kind {
                CellKind::User { queued, .. } | CellKind::Image { queued, .. } => *queued = true,
                _ => {}
            }
        }
    }

    /// Backchat `session.tool_cancelled` on user stop: in-flight tools and
    /// streams stop spinning before `session/prompt` unwinds.
    pub fn cancel_open_work(&mut self) {
        for cell in &mut self.cells {
            match &mut cell.kind {
                CellKind::Tool {
                    ok, result, error, ..
                } if ok.is_none() => {
                    *ok = Some(false);
                    *error = Some("cancelled".into());
                    if result.is_empty() {
                        *result = "cancelled".into();
                    }
                }
                CellKind::Reasoning {
                    done,
                    started,
                    seconds,
                    ..
                } if !*done => {
                    *done = true;
                    *seconds = Some(started.elapsed().as_secs_f32());
                }
                CellKind::Assistant { done, .. } if !*done => {
                    *done = true;
                }
                CellKind::Shell { output, .. } if output.is_none() => {
                    *output = Some((None, "cancelled".into()));
                }
                _ => {}
            }
        }
        self.tools.clear();
        self.tool_started.clear();
        self.open_assistant.clear();
        self.open_reasoning.clear();
        self.last_finish = Some("cancelled".into());
    }

    fn close_open(&mut self, session: &str) {
        if let Some(idx) = self.open_assistant.remove(session) {
            if let Some(cell) = self.cells.get_mut(idx) {
                if let CellKind::Assistant { done, text, .. } = &mut cell.kind {
                    *done = true;
                    if text.trim().is_empty() {
                        // Leave the empty husk; render skips empty finished cells.
                    }
                }
            }
        }
        if let Some(idx) = self.open_reasoning.remove(session) {
            if let Some(cell) = self.cells.get_mut(idx) {
                if let CellKind::Reasoning {
                    done,
                    started,
                    seconds,
                    ..
                } = &mut cell.kind
                {
                    *done = true;
                    *seconds = Some(started.elapsed().as_secs_f32());
                }
            }
        }
    }

    fn close_all_open(&mut self) {
        let sessions: Vec<String> = self
            .open_assistant
            .keys()
            .chain(self.open_reasoning.keys())
            .cloned()
            .collect();
        for s in sessions {
            self.close_open(&s);
        }
    }

    /// Record the per-turn first-token latency when the first visible or
    /// reasoning delta for the root session arrives.
    fn note_first_token(&mut self, session: &str) {
        if session != self.root_session || !self.ttft_pending {
            return;
        }
        if let Some(t0) = self.turn_started {
            self.stats.ttft_total_millis += t0.elapsed().as_millis() as u64;
            self.stats.ttft_count += 1;
        }
        self.ttft_pending = false;
    }

    pub fn apply(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::SessionStatus { session, running } => {
                if !running && session == self.root_session {
                    self.close_all_open();
                }
            }
            UiEvent::TurnStart { session, .. } => {
                if session == self.root_session {
                    self.stats.turns += 1;
                    self.turn_started = Some(Instant::now());
                    self.ttft_pending = true;
                }
            }
            UiEvent::TurnEnd { session, kind } => {
                self.close_open(&session);
                if session == self.root_session {
                    self.last_finish = Some(kind.clone());
                    if let Some(t0) = self.turn_started.take() {
                        self.stats.turn_millis += t0.elapsed().as_millis() as u64;
                    }
                    if kind != "completed" && kind != "interrupted" {
                        let level = if kind == "error" {
                            NoticeLevel::Error
                        } else {
                            NoticeLevel::Warn
                        };
                        self.push_notice(level, format!("turn ended: {kind}"));
                    }
                }
            }
            UiEvent::TextDelta { session, text } => {
                self.note_first_token(&session);
                // Reasoning for this step is over once visible text streams.
                if let Some(idx) = self.open_reasoning.remove(&session) {
                    if let Some(cell) = self.cells.get_mut(idx) {
                        if let CellKind::Reasoning {
                            done,
                            started,
                            seconds,
                            ..
                        } = &mut cell.kind
                        {
                            *done = true;
                            *seconds = Some(started.elapsed().as_secs_f32());
                        }
                    }
                }
                let idx = match self.open_assistant.get(&session) {
                    Some(&idx) => idx,
                    None => {
                        let agent = self.agent_label(&session);
                        self.cells.push(Cell::new(CellKind::Assistant {
                            text: String::new(),
                            done: false,
                            model: None,
                            agent,
                        }));
                        let idx = self.cells.len() - 1;
                        self.open_assistant.insert(session.clone(), idx);
                        idx
                    }
                };
                if let Some(cell) = self.cells.get_mut(idx) {
                    if let CellKind::Assistant { text: buf, .. } = &mut cell.kind {
                        buf.push_str(&text);
                    }
                }
            }
            UiEvent::ReasoningDelta { session, text } => {
                self.note_first_token(&session);
                let idx = match self.open_reasoning.get(&session) {
                    Some(&idx) => idx,
                    None => {
                        let agent = self.agent_label(&session);
                        self.cells.push(Cell::new(CellKind::Reasoning {
                            text: String::new(),
                            done: false,
                            started: Instant::now(),
                            seconds: None,
                            agent,
                        }));
                        let idx = self.cells.len() - 1;
                        self.open_reasoning.insert(session.clone(), idx);
                        idx
                    }
                };
                if let Some(cell) = self.cells.get_mut(idx) {
                    if let CellKind::Reasoning { text: buf, .. } = &mut cell.kind {
                        buf.push_str(&text);
                    }
                }
            }
            UiEvent::AssistantFinal {
                session,
                text,
                model,
            } => {
                if session == self.root_session {
                    self.stats.steps += 1;
                }
                if model.is_some() && session == self.root_session {
                    self.last_model = model.clone();
                }
                let idx = self.open_assistant.remove(&session);
                match idx {
                    Some(idx) => {
                        if let Some(cell) = self.cells.get_mut(idx) {
                            if let CellKind::Assistant {
                                text: buf,
                                done,
                                model: m,
                                ..
                            } = &mut cell.kind
                            {
                                if !text.is_empty() {
                                    *buf = text;
                                }
                                *done = true;
                                *m = model;
                            }
                        }
                    }
                    None => {
                        if !text.is_empty() {
                            let agent = self.agent_label(&session);
                            self.cells.push(Cell::new(CellKind::Assistant {
                                text,
                                done: true,
                                model,
                                agent,
                            }));
                        }
                    }
                }
            }
            UiEvent::ToolCall {
                session,
                call_id,
                name,
                arguments,
            } => {
                if session == self.root_session {
                    self.tool_started.insert(call_id.clone(), Instant::now());
                }
                self.close_open(&session);
                let title = tool_title(&name, &arguments);
                let agent = self.agent_label(&session);
                self.cells.push(Cell::new(CellKind::Tool {
                    name,
                    title,
                    result: String::new(),
                    ok: None,
                    error: None,
                    agent,
                }));
                self.tools.insert(call_id, self.cells.len() - 1);
            }
            UiEvent::ToolResult {
                session,
                call_id,
                is_error,
                text,
                error,
            } => {
                if session == self.root_session {
                    if let Some(t0) = self.tool_started.remove(&call_id) {
                        self.stats.tool_millis += t0.elapsed().as_millis() as u64;
                    }
                }
                let idx = self.tools.remove(&call_id);
                match idx {
                    Some(idx) => {
                        if let Some(cell) = self.cells.get_mut(idx) {
                            if let CellKind::Tool {
                                result,
                                ok,
                                error: e,
                                ..
                            } = &mut cell.kind
                            {
                                *result = text;
                                *ok = Some(!is_error);
                                *e = error;
                            }
                        }
                    }
                    None => {
                        let agent = self.agent_label(&session);
                        self.cells.push(Cell::new(CellKind::Tool {
                            name: "tool".into(),
                            title: call_id,
                            result: text,
                            ok: Some(!is_error),
                            error,
                            agent,
                        }));
                    }
                }
            }
            UiEvent::Usage {
                input,
                output,
                cached,
                reasoning,
                ..
            } => {
                self.usage.input += input;
                self.usage.output += output;
                self.usage.cached += cached;
                self.usage.reasoning += reasoning;
            }
            UiEvent::UserInjected {
                source, preview, ..
            } => {
                self.cells
                    .push(Cell::new(CellKind::Injected { source, preview }));
            }
            UiEvent::UserMessage { text, .. } => {
                self.push_user(text, false);
            }
            UiEvent::SessionTitle { title, .. } => {
                self.push_notice(NoticeLevel::Info, format!("session · {title}"));
            }
            UiEvent::Plan { summary, .. } => {
                if let Some(idx) = self.plan_cell {
                    if let Some(Cell {
                        kind: CellKind::Plan { summary: current },
                        ..
                    }) = self.cells.get_mut(idx)
                    {
                        *current = summary.clone();
                    } else {
                        self.plan_cell = None;
                    }
                }
                if self.plan_cell.is_none() {
                    self.cells.push(Cell::new(CellKind::Plan { summary }));
                    self.plan_cell = Some(self.cells.len() - 1);
                }
            }
            UiEvent::SubagentStarted { child, .. } => {
                self.agent_seq += 1;
                let label = format!("subagent {}", self.agent_seq);
                self.agents.insert(child, label.clone());
                self.push_notice(NoticeLevel::Info, format!("⛭ {label} started"));
            }
            UiEvent::SubagentFinished { child } => {
                let label = self
                    .agents
                    .get(&child)
                    .cloned()
                    .unwrap_or_else(|| "subagent".into());
                self.push_notice(NoticeLevel::Info, format!("⛭ {label} finished"));
            }
            UiEvent::PlanMode { active, .. } => {
                self.push_notice(
                    NoticeLevel::Info,
                    format!("⌁ plan mode {}", if active { "on" } else { "off" }),
                );
            }
            UiEvent::SandboxMode { mode, .. } => {
                self.push_notice(NoticeLevel::Info, format!("⛨ file policy · {mode}"));
            }
            UiEvent::ApprovalPolicy { policy, .. } => {
                self.push_notice(NoticeLevel::Info, format!("⚖ approval policy · {policy}"));
            }
            UiEvent::PermissionPreset { preset, .. } => {
                self.push_notice(NoticeLevel::Info, format!("⛨ permission · {preset}"));
            }
            UiEvent::AgentPreset { preset, .. } => {
                self.push_notice(NoticeLevel::Info, format!("⚙ agent preset · {preset}"));
            }
            UiEvent::ApprovalAsked { tool, reason, .. } => {
                let why = reason.map(|r| format!(" · {r}")).unwrap_or_default();
                self.push_notice(
                    NoticeLevel::Warn,
                    format!("⚖ approval requested · {tool}{why}"),
                );
            }
            UiEvent::ApprovalDecided { outcome, .. } => {
                let level = if outcome.contains("reject") || outcome.contains("denied") {
                    NoticeLevel::Warn
                } else {
                    NoticeLevel::Info
                };
                self.push_notice(level, format!("⚖ approval · {outcome}"));
            }
            UiEvent::Palette { .. } => {}
        }
    }

    /// Is any assistant/reasoning cell currently streaming?
    pub fn streaming(&self) -> bool {
        !self.open_assistant.is_empty() || !self.open_reasoning.is_empty()
    }

    /// Render every cell to wrapped, styled lines for `width` columns.
    #[allow(dead_code)] // kept for tests; the UI uses `layout` for ownership
    pub fn lines(&self, theme: &Theme, width: u16, spinner: char) -> Vec<Line<'static>> {
        self.layout(theme, width, spinner, false).lines
    }

    /// Render every cell to wrapped, styled lines, plus per-line ownership so
    /// the UI can route mouse clicks to a specific tool preview (and only tool
    /// lines claim ownership). `thumbs` reserves blank
    /// lines for kitty-graphics image thumbnails and reports their placements.
    pub fn layout(
        &self,
        theme: &Theme,
        width: u16,
        spinner: char,
        thumbs: bool,
    ) -> TranscriptLayout {
        let width = width.max(8) as usize;
        let mut out: Vec<Line> = Vec::new();
        let mut owners: Vec<Option<usize>> = Vec::new();
        let mut images: Vec<ImageShot> = Vec::new();
        for (ci, cell) in self.cells.iter().enumerate() {
            let expanded = cell.expanded || self.expand_all;
            match &cell.kind {
                CellKind::User { text, queued } => {
                    emit(&mut out, &mut owners, Line::default(), None);
                    // Web UI fidelity: the user bubble uses --dsw-specific-bubble.
                    for (i, l) in wrap(text, width.saturating_sub(2)).into_iter().enumerate() {
                        let mut spans = vec![
                            Span::styled(
                                if i == 0 { "❯ " } else { "  " }.to_string(),
                                Style::default()
                                    .fg(theme.brand)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!(" {l} "),
                                Style::default()
                                    .fg(theme.bubble_fg)
                                    .bg(theme.bubble_bg)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ];
                        if i == 0 && *queued {
                            spans.push(Span::styled(
                                "  queued".to_string(),
                                Style::default().fg(theme.warn_soft()),
                            ));
                        }
                        emit(&mut out, &mut owners, Line::from(spans), None);
                    }
                }
                CellKind::Image {
                    name,
                    caption,
                    path,
                    data,
                    id,
                    queued,
                    ..
                } => {
                    emit(&mut out, &mut owners, Line::default(), None);
                    let label = if caption.is_empty() {
                        format!("🖼 {name}")
                    } else {
                        format!("🖼 {name} · {caption}")
                    };
                    let mut spans = vec![
                        Span::styled(
                            "❯ ".to_string(),
                            Style::default()
                                .fg(theme.brand)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" {label} "),
                            Style::default()
                                .fg(theme.bubble_fg)
                                .bg(theme.bubble_bg)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];
                    if *queued {
                        spans.push(Span::styled(
                            "  queued".to_string(),
                            Style::default().fg(theme.warn_soft()),
                        ));
                    }
                    emit(&mut out, &mut owners, Line::from(spans), None);

                    // Thumbnail (PNG only — clipboard screenshots are PNG;
                    // other formats fall back to the path line below).
                    let is_png = data.starts_with(b"\x89PNG\r\n\x1a\n");
                    if thumbs && is_png {
                        if let Some((w, h)) = crate::pet::image_dims(data) {
                            let cols = (width.saturating_sub(2)).min(THUMB_COLS);
                            let rows = thumb_rows(cols, w, h) as usize;
                            let line = out.len();
                            for _ in 0..rows {
                                emit(&mut out, &mut owners, Line::default(), None);
                            }
                            images.push(ImageShot {
                                id: *id,
                                line,
                                rows,
                                cols,
                                data: data.clone(),
                            });
                        } else {
                            emit(
                                &mut out,
                                &mut owners,
                                Line::from(Span::styled(
                                    format!("  {path}"),
                                    Style::default().fg(theme.caption),
                                )),
                                None,
                            );
                        }
                    } else {
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(Span::styled(
                                format!("  {path}"),
                                Style::default().fg(theme.caption),
                            )),
                            None,
                        );
                    }
                }
                CellKind::Reasoning {
                    text,
                    done,
                    started,
                    seconds,
                    agent,
                } => {
                    emit(&mut out, &mut owners, Line::default(), None);
                    let head_style = Style::default().fg(theme.caption);
                    let body_style = Style::default()
                        .fg(theme.fg_tertiary)
                        .add_modifier(Modifier::ITALIC);
                    // A reasoning stream can open with an empty/whitespace
                    // delta. The heading is enough for that frame; an empty
                    // body must not make its height jump from one row to two.
                    let body = text.trim();
                    let lines = if body.is_empty() {
                        Vec::new()
                    } else {
                        wrap(body, width.saturating_sub(2))
                    };
                    let n = lines.len();
                    if *done {
                        let dur = seconds.map(|s| format!(" · {s:.1}s")).unwrap_or_default();
                        let agent = agent_prefix(agent);
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(Span::styled(
                                format!("✻ {agent}thought{dur} · {n} line{}", plural(n)),
                                head_style,
                            )),
                            None,
                        );
                        if expanded {
                            for l in lines {
                                emit(
                                    &mut out,
                                    &mut owners,
                                    Line::from(vec![Span::raw("  "), Span::styled(l, body_style)]),
                                    None,
                                );
                            }
                        }
                    } else {
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(Span::styled(
                                format!(
                                    "✻ {}thinking… {}s",
                                    agent_prefix(agent),
                                    started.elapsed().as_secs()
                                ),
                                Style::default().fg(theme.brand_soft),
                            )),
                            None,
                        );
                        let tail: Vec<_> = if expanded {
                            lines
                        } else {
                            lines
                                .into_iter()
                                .rev()
                                .take(COLLAPSED_REASONING_PREVIEW)
                                .rev()
                                .collect()
                        };
                        for l in tail {
                            emit(
                                &mut out,
                                &mut owners,
                                Line::from(vec![Span::raw("  "), Span::styled(l, body_style)]),
                                None,
                            );
                        }
                    }
                }
                CellKind::Assistant {
                    text, done, agent, ..
                } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    emit(&mut out, &mut owners, Line::default(), None);
                    if let Some(a) = agent {
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(Span::styled(
                                format!("⛭ {a}"),
                                Style::default().fg(theme.caption),
                            )),
                            None,
                        );
                    }
                    let mut lines = crate::markdown::render(text, theme, width);
                    if !done {
                        // streaming cursor
                        if let Some(last) = lines.last_mut() {
                            last.spans.push(Span::styled(
                                "▍".to_string(),
                                Style::default().fg(theme.brand),
                            ));
                        }
                    }
                    for l in lines {
                        emit(&mut out, &mut owners, l, None);
                    }
                }
                CellKind::Tool {
                    name,
                    title,
                    result,
                    ok,
                    error,
                    agent,
                } => {
                    emit(&mut out, &mut owners, Line::default(), None);
                    let body = result.trim_end();
                    let all: Vec<String> = if body.is_empty() {
                        Vec::new()
                    } else {
                        body.lines()
                            .flat_map(|raw| wrap(raw, width.saturating_sub(2)))
                            .collect()
                    };
                    let total = all.len();
                    let has_more = total > TOOL_VIEWPORT;
                    let (glyph, gstyle) = match ok {
                        None => (spinner, Style::default().fg(theme.brand)),
                        Some(true) => ('⏺', Style::default().fg(theme.ok_soft())),
                        Some(false) => ('⏺', Style::default().fg(theme.err)),
                    };
                    let mut spans = vec![
                        Span::styled(format!("{glyph} "), gstyle),
                        Span::styled(
                            name.clone(),
                            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                        ),
                    ];
                    if let Some(a) = agent {
                        spans.push(Span::styled(
                            format!(" · {a}"),
                            Style::default().fg(theme.caption),
                        ));
                    }
                    if !title.is_empty() {
                        let prefix_w: usize = spans.iter().map(|s| s.content.width()).sum();
                        let chevron_w = if has_more { 2 } else { 0 };
                        let budget = width.saturating_sub(prefix_w + 2 + chevron_w);
                        spans.push(Span::styled(
                            format!("  {}", clamp_str(title, budget)),
                            Style::default().fg(theme.fg_tertiary),
                        ));
                    }
                    if has_more {
                        spans.push(Span::styled(
                            if expanded { " ▾" } else { " ▸" }.to_string(),
                            Style::default().fg(theme.caption),
                        ));
                    }
                    emit(&mut out, &mut owners, Line::from(spans), Some(ci));

                    if let Some(err) = error {
                        for l in wrap(err, width.saturating_sub(2)) {
                            emit(
                                &mut out,
                                &mut owners,
                                Line::from(vec![
                                    Span::styled("│ ".to_string(), Style::default().fg(theme.err)),
                                    Span::styled(l, Style::default().fg(theme.err)),
                                ]),
                                Some(ci),
                            );
                        }
                    }

                    if !body.is_empty() {
                        let bar_color = match ok {
                            Some(false) => theme.err,
                            _ => theme.border,
                        };
                        if expanded || total <= TOOL_VIEWPORT {
                            for l in all {
                                emit(
                                    &mut out,
                                    &mut owners,
                                    Line::from(vec![
                                        Span::styled(
                                            "│ ".to_string(),
                                            Style::default().fg(bar_color),
                                        ),
                                        Span::styled(l, Style::default().fg(theme.fg_tertiary)),
                                    ]),
                                    Some(ci),
                                );
                            }
                        } else {
                            let offset = total - TOOL_VIEWPORT;
                            for l in &all[offset..offset + TOOL_VIEWPORT] {
                                emit(
                                    &mut out,
                                    &mut owners,
                                    Line::from(vec![
                                        Span::styled(
                                            "│ ".to_string(),
                                            Style::default().fg(bar_color),
                                        ),
                                        Span::styled(
                                            l.clone(),
                                            Style::default().fg(theme.fg_tertiary),
                                        ),
                                    ]),
                                    Some(ci),
                                );
                            }
                            emit(
                                &mut out,
                                &mut owners,
                                Line::from(vec![
                                    Span::styled("│ ".to_string(), Style::default().fg(bar_color)),
                                    Span::styled(
                                        format!(
                                            "last {}/{} lines · click to expand",
                                            TOOL_VIEWPORT, total
                                        ),
                                        Style::default().fg(theme.caption),
                                    ),
                                ]),
                                Some(ci),
                            );
                        }
                    }
                }
                CellKind::Shell { command, output } => {
                    emit(&mut out, &mut owners, Line::default(), None);
                    let (glyph, gstyle) = match output {
                        None => (spinner, Style::default().fg(theme.warn_soft())),
                        Some((Some(0), _)) => ('!', Style::default().fg(theme.ok_soft())),
                        Some(_) => ('!', Style::default().fg(theme.err)),
                    };
                    // Terminal card: `$ cmd` chip in the header, output as
                    // full-width code_bg rows behind a gray-blue gutter.
                    emit(
                        &mut out,
                        &mut owners,
                        Line::from(vec![
                            Span::styled(format!("{glyph} "), gstyle.add_modifier(Modifier::BOLD)),
                            Span::styled(
                                format!(" $ {command} "),
                                Style::default()
                                    .fg(theme.bubble_fg)
                                    .bg(theme.bubble_bg)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                "  local shell".to_string(),
                                Style::default().fg(theme.caption),
                            ),
                        ]),
                        None,
                    );
                    if let Some((code, text)) = output {
                        let body_w = width.saturating_sub(2);
                        let card_row = |txt: &str, fg: ratatui::style::Color| -> Line<'static> {
                            let pad =
                                body_w.saturating_sub(unicode_width::UnicodeWidthStr::width(txt));
                            Line::from(vec![
                                Span::styled(
                                    "▎ ".to_string(),
                                    Style::default().fg(theme.hint).bg(theme.code_bg),
                                ),
                                Span::styled(
                                    format!("{txt}{}", " ".repeat(pad)),
                                    Style::default().fg(fg).bg(theme.code_bg),
                                ),
                            ])
                        };
                        let all: Vec<String> = text
                            .trim_end()
                            .lines()
                            .flat_map(|raw| wrap(raw, body_w))
                            .collect();
                        let total = all.len();
                        let shown = if expanded || total <= COLLAPSED_SHELL_LINES {
                            all
                        } else {
                            all.into_iter().take(COLLAPSED_SHELL_LINES).collect()
                        };
                        let shown_len = shown.len();
                        for l in shown {
                            emit(
                                &mut out,
                                &mut owners,
                                card_row(&l, theme.fg_secondary),
                                None,
                            );
                        }
                        if total > shown_len {
                            emit(
                                &mut out,
                                &mut owners,
                                card_row(
                                    &format!("… +{} lines (ctrl+o expands)", total - shown_len),
                                    theme.caption,
                                ),
                                None,
                            );
                        }
                        if let Some(c) = code {
                            if *c != 0 {
                                emit(
                                    &mut out,
                                    &mut owners,
                                    card_row(&format!("exit {c}"), theme.err),
                                    None,
                                );
                            }
                        }
                    }
                }
                CellKind::Injected { source, preview } => {
                    emit(
                        &mut out,
                        &mut owners,
                        Line::from(vec![
                            Span::styled(
                                "◦ context".to_string(),
                                Style::default().fg(theme.caption),
                            ),
                            Span::styled(
                                format!(" · {source} · "),
                                Style::default().fg(theme.caption),
                            ),
                            Span::styled(
                                clamp_str(preview, width.saturating_sub(source.len() + 14)),
                                Style::default().fg(theme.fg_tertiary),
                            ),
                        ]),
                        None,
                    );
                }
                CellKind::Plan { summary } => {
                    if summary.is_empty() {
                        continue;
                    }
                    for l in wrap(&format!("plan · {summary}"), width.saturating_sub(2)) {
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(vec![
                                Span::styled("· ".to_string(), Style::default().fg(theme.caption)),
                                Span::styled(l, Style::default().fg(theme.caption)),
                            ]),
                            None,
                        );
                    }
                }
                CellKind::Notice { level, text } => {
                    let color = match level {
                        NoticeLevel::Info => theme.caption,
                        NoticeLevel::Warn => theme.warn_soft(),
                        NoticeLevel::Error => theme.err,
                    };
                    for l in wrap(text, width.saturating_sub(2)) {
                        emit(
                            &mut out,
                            &mut owners,
                            Line::from(vec![
                                Span::styled("· ".to_string(), Style::default().fg(color)),
                                Span::styled(l, Style::default().fg(color)),
                            ]),
                            None,
                        );
                    }
                }
                CellKind::MarkdownNotice { text } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    emit(&mut out, &mut owners, Line::default(), None);
                    for l in crate::markdown::render(text, theme, width) {
                        emit(&mut out, &mut owners, l, None);
                    }
                }
            }
        }
        TranscriptLayout {
            lines: out,
            owners,
            images,
        }
    }
}

fn emit(
    out: &mut Vec<Line<'static>>,
    owners: &mut Vec<Option<usize>>,
    line: Line<'static>,
    owner: Option<usize>,
) {
    out.push(line);
    owners.push(owner);
}

/// Thumbnail height in rows for `cols` columns, preserving the image's pixel
/// aspect under the same 2:1 cell aspect the composer pet assumes.
fn thumb_rows(cols: usize, w: u32, h: u32) -> usize {
    if w == 0 || h == 0 || cols == 0 {
        return 6;
    }
    let rows = (cols as f64 * h as f64 / (2.0 * w as f64)).round() as usize;
    rows.clamp(2, 12)
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn agent_prefix(agent: &Option<String>) -> String {
    match agent {
        Some(a) => format!("{a} "),
        None => String::new(),
    }
}

/// Human title for a tool call, parsed from its raw JSON argument string.
pub fn tool_title(name: &str, arguments: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(arguments).ok();
    if let Some(v) = parsed {
        match name {
            "bash" => {
                if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                    return one_line(cmd);
                }
            }
            "str_replace_editor" => {
                let cmd = v.get("command").and_then(|c| c.as_str()).unwrap_or("");
                let path = v.get("path").and_then(|c| c.as_str()).unwrap_or("");
                if !cmd.is_empty() || !path.is_empty() {
                    return one_line(&format!("{cmd} {path}"));
                }
            }
            _ => {}
        }
        // generic: compact json
        return one_line(&v.to_string());
    }
    one_line(arguments)
}

fn one_line(s: &str) -> String {
    clamp_str(&s.replace('\n', " ⏎ "), 120)
}

pub(crate) fn clamp_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let s = expand_tabs(s);
    if s.width() <= max {
        return s;
    }
    let mut out = String::new();
    let mut w = 0usize;
    let budget = max.saturating_sub(1);
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > budget {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Expand tabs to spaces at 8-column stops so wrap/layout width matches
/// what a terminal actually paints (a raw `\t` is otherwise width 0/1).
fn expand_tabs(s: &str) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let pad = 8 - (col % 8);
            out.push_str(&" ".repeat(pad));
            col += pad;
        } else {
            out.push(ch);
            col += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    out
}

/// Greedy display-width wrap with word-boundary preference; never panics on
/// CJK/emoji. Tabs expand to spaces before wrapping so a tab-indented source
/// line cannot paint past `width` (issue #5).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let raw = expand_tabs(raw);
        let mut line = String::new();
        let mut w = 0usize;
        for ch in raw.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > width && !line.is_empty() {
                // Prefer breaking at the last space when it is not too early.
                match line.rfind(' ') {
                    Some(bidx) if line[..bidx].width() >= width / 2 => {
                        let tail = line[bidx + 1..].to_string();
                        line.truncate(bidx);
                        out.push(std::mem::replace(&mut line, tail));
                        w = line.width();
                    }
                    _ => {
                        out.push(std::mem::take(&mut line));
                        w = 0;
                    }
                }
            }
            if w + cw > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                w = 0;
            }
            line.push(ch);
            w += cw;
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/transcript__tests.rs"]
mod tests;

