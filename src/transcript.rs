//! Transcript model: the scrollback cells and their rendering to styled lines.
//!
//! Mirrors what the deepseek-harness Web UI surfaces for a session: user
//! prompts, streaming reasoning, streaming assistant text, tool calls with
//! results, injected context, subagent lifecycle, usage accounting, and turn
//! outcomes.

use std::collections::HashMap;
use std::time::Instant;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::events::UiEvent;
use crate::theme::Theme;

const COLLAPSED_TOOL_LINES: usize = 6;
const COLLAPSED_REASONING_PREVIEW: usize = 2;

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
    Notice {
        level: NoticeLevel,
        text: String,
    },
}

pub struct Cell {
    pub kind: CellKind,
    pub expanded: bool,
}

#[derive(Default, Clone, Copy)]
pub struct UsageTotals {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub reasoning: u64,
}

pub struct Transcript {
    pub cells: Vec<Cell>,
    root_session: String,
    open_assistant: HashMap<String, usize>,
    open_reasoning: HashMap<String, usize>,
    tools: HashMap<String, usize>,
    agents: HashMap<String, String>,
    agent_seq: usize,
    pub usage: UsageTotals,
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
            usage: UsageTotals::default(),
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
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.open_assistant.clear();
        self.open_reasoning.clear();
        self.tools.clear();
        self.last_finish = None;
    }

    fn agent_label(&self, session: &str) -> Option<String> {
        if session == self.root_session || session.is_empty() {
            None
        } else {
            self.agents.get(session).cloned().or_else(|| Some("agent".into()))
        }
    }

    pub fn push_user(&mut self, text: String, queued: bool) {
        self.cells.push(Cell {
            kind: CellKind::User { text, queued },
            expanded: false,
        });
    }

    pub fn push_notice(&mut self, level: NoticeLevel, text: String) {
        self.cells.push(Cell {
            kind: CellKind::Notice { level, text },
            expanded: false,
        });
    }

    pub fn push_shell(&mut self, command: String) -> usize {
        self.cells.push(Cell {
            kind: CellKind::Shell {
                command,
                output: None,
            },
            expanded: false,
        });
        self.cells.len() - 1
    }

    pub fn finish_shell(&mut self, idx: usize, code: Option<i32>, output: String) {
        if let Some(cell) = self.cells.get_mut(idx) {
            if let CellKind::Shell { output: slot, .. } = &mut cell.kind {
                *slot = Some((code, output));
            }
        }
    }

    /// Mark queued user cells as delivered (a turn started consuming input).
    fn clear_queued_marks(&mut self) {
        for cell in &mut self.cells {
            if let CellKind::User { queued, .. } = &mut cell.kind {
                *queued = false;
            }
        }
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

    pub fn apply(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::SessionStatus { session, running } => {
                if !running && session == self.root_session {
                    self.close_all_open();
                }
            }
            UiEvent::TurnStart { session, .. } => {
                if session == self.root_session {
                    self.clear_queued_marks();
                }
            }
            UiEvent::TurnEnd { session, kind } => {
                self.close_open(&session);
                if session == self.root_session {
                    self.last_finish = Some(kind.clone());
                    if kind != "completed" {
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
                        self.cells.push(Cell {
                            kind: CellKind::Assistant {
                                text: String::new(),
                                done: false,
                                model: None,
                                agent,
                            },
                            expanded: false,
                        });
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
                let idx = match self.open_reasoning.get(&session) {
                    Some(&idx) => idx,
                    None => {
                        let agent = self.agent_label(&session);
                        self.cells.push(Cell {
                            kind: CellKind::Reasoning {
                                text: String::new(),
                                done: false,
                                started: Instant::now(),
                                seconds: None,
                                agent,
                            },
                            expanded: false,
                        });
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
                            self.cells.push(Cell {
                                kind: CellKind::Assistant {
                                    text,
                                    done: true,
                                    model,
                                    agent,
                                },
                                expanded: false,
                            });
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
                self.close_open(&session);
                let title = tool_title(&name, &arguments);
                let agent = self.agent_label(&session);
                self.cells.push(Cell {
                    kind: CellKind::Tool {
                        name,
                        title,
                        result: String::new(),
                        ok: None,
                        error: None,
                        agent,
                    },
                    expanded: false,
                });
                self.tools.insert(call_id, self.cells.len() - 1);
            }
            UiEvent::ToolResult {
                session,
                call_id,
                is_error,
                text,
                error,
            } => {
                let idx = self.tools.remove(&call_id);
                match idx {
                    Some(idx) => {
                        if let Some(cell) = self.cells.get_mut(idx) {
                            if let CellKind::Tool {
                                result, ok, error: e, ..
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
                        self.cells.push(Cell {
                            kind: CellKind::Tool {
                                name: "tool".into(),
                                title: call_id,
                                result: text,
                                ok: Some(!is_error),
                                error,
                                agent,
                            },
                            expanded: false,
                        });
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
                self.cells.push(Cell {
                    kind: CellKind::Injected { source, preview },
                    expanded: false,
                });
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
        }
    }

    /// Is any assistant/reasoning cell currently streaming?
    pub fn streaming(&self) -> bool {
        !self.open_assistant.is_empty() || !self.open_reasoning.is_empty()
    }

    /// Render every cell to wrapped, styled lines for `width` columns.
    pub fn lines(&self, theme: &Theme, width: u16, spinner: char) -> Vec<Line<'static>> {
        let width = width.max(8) as usize;
        let mut out: Vec<Line> = Vec::new();
        for cell in &self.cells {
            let expanded = cell.expanded || self.expand_all;
            match &cell.kind {
                CellKind::User { text, queued } => {
                    out.push(Line::default());
                    // Web UI fidelity: the user bubble uses --dsw-specific-bubble.
                    for (i, l) in wrap(text, width.saturating_sub(2)).into_iter().enumerate() {
                        let mut spans = vec![
                            Span::styled(
                                if i == 0 { "❯ " } else { "  " }.to_string(),
                                Style::default().fg(theme.brand).add_modifier(Modifier::BOLD),
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
                        out.push(Line::from(spans));
                    }
                }
                CellKind::Reasoning {
                    text,
                    done,
                    seconds,
                    agent,
                    ..
                } => {
                    out.push(Line::default());
                    let head_style = Style::default().fg(theme.caption);
                    let body_style = Style::default()
                        .fg(theme.fg_tertiary)
                        .add_modifier(Modifier::ITALIC);
                    let lines = wrap(text.trim_end(), width.saturating_sub(2));
                    let n = lines.len();
                    if *done {
                        let dur = seconds.map(|s| format!(" · {s:.1}s")).unwrap_or_default();
                        let agent = agent_prefix(agent);
                        out.push(Line::from(Span::styled(
                            format!("✻ {agent}thought{dur} · {n} line{}", plural(n)),
                            head_style,
                        )));
                        if expanded {
                            for l in lines {
                                out.push(Line::from(vec![
                                    Span::raw("  "),
                                    Span::styled(l, body_style),
                                ]));
                            }
                        }
                    } else {
                        out.push(Line::from(Span::styled(
                            format!("✻ {}thinking…", agent_prefix(agent)),
                            Style::default().fg(theme.brand_soft),
                        )));
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
                            out.push(Line::from(vec![Span::raw("  "), Span::styled(l, body_style)]));
                        }
                    }
                }
                CellKind::Assistant {
                    text, done, agent, ..
                } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    out.push(Line::default());
                    if let Some(a) = agent {
                        out.push(Line::from(Span::styled(
                            format!("⛭ {a}"),
                            Style::default().fg(theme.caption),
                        )));
                    }
                    let mut in_fence = false;
                    for raw in text.lines() {
                        let fence = raw.trim_start().starts_with("```");
                        if fence {
                            in_fence = !in_fence;
                            out.push(Line::from(Span::styled(
                                raw.to_string(),
                                Style::default().fg(theme.caption).bg(theme.code_bg),
                            )));
                            continue;
                        }
                        let style = if in_fence {
                            Style::default().fg(theme.fg_secondary).bg(theme.code_bg)
                        } else {
                            Style::default().fg(theme.fg_secondary)
                        };
                        if raw.is_empty() {
                            out.push(Line::default());
                            continue;
                        }
                        for l in wrap(raw, width) {
                            out.push(Line::from(Span::styled(l, style)));
                        }
                    }
                    if !done {
                        // streaming cursor
                        if let Some(last) = out.last_mut() {
                            last.spans.push(Span::styled(
                                "▍".to_string(),
                                Style::default().fg(theme.brand),
                            ));
                        }
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
                    out.push(Line::default());
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
                        spans.push(Span::styled(
                            format!("  {}", clamp_str(title, width.saturating_sub(name.len() + 6))),
                            Style::default().fg(theme.fg_tertiary),
                        ));
                    }
                    out.push(Line::from(spans));
                    if let Some(err) = error {
                        out.push(Line::from(vec![
                            Span::styled("│ ".to_string(), Style::default().fg(theme.err)),
                            Span::styled(err.clone(), Style::default().fg(theme.err)),
                        ]));
                    }
                    let body = result.trim_end();
                    if !body.is_empty() {
                        let bar_color = match ok {
                            Some(false) => theme.err,
                            _ => theme.border,
                        };
                        let all: Vec<String> = body
                            .lines()
                            .flat_map(|raw| wrap(raw, width.saturating_sub(2)))
                            .collect();
                        let total = all.len();
                        let shown = if expanded || total <= COLLAPSED_TOOL_LINES {
                            all
                        } else {
                            all.into_iter().take(COLLAPSED_TOOL_LINES).collect()
                        };
                        let shown_len = shown.len();
                        for l in shown {
                            out.push(Line::from(vec![
                                Span::styled("│ ".to_string(), Style::default().fg(bar_color)),
                                Span::styled(l, Style::default().fg(theme.fg_tertiary)),
                            ]));
                        }
                        if total > shown_len {
                            out.push(Line::from(vec![
                                Span::styled("│ ".to_string(), Style::default().fg(bar_color)),
                                Span::styled(
                                    format!("… +{} lines (ctrl+e expands)", total - shown_len),
                                    Style::default().fg(theme.caption),
                                ),
                            ]));
                        }
                    }
                }
                CellKind::Shell { command, output } => {
                    out.push(Line::default());
                    let (glyph, gstyle) = match output {
                        None => (spinner, Style::default().fg(theme.warn_soft())),
                        Some((Some(0), _)) => ('!', Style::default().fg(theme.ok_soft())),
                        Some(_) => ('!', Style::default().fg(theme.err)),
                    };
                    out.push(Line::from(vec![
                        Span::styled(format!("{glyph} "), gstyle.add_modifier(Modifier::BOLD)),
                        Span::styled(
                            command.clone(),
                            Style::default().fg(theme.warn_soft()).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("  (local shell)".to_string(), Style::default().fg(theme.caption)),
                    ]));
                    if let Some((code, text)) = output {
                        let all: Vec<String> = text
                            .trim_end()
                            .lines()
                            .flat_map(|raw| wrap(raw, width.saturating_sub(2)))
                            .collect();
                        let total = all.len();
                        let shown = if expanded || total <= COLLAPSED_TOOL_LINES * 2 {
                            all
                        } else {
                            all.into_iter().take(COLLAPSED_TOOL_LINES * 2).collect()
                        };
                        let shown_len = shown.len();
                        for l in shown {
                            out.push(Line::from(vec![
                                Span::styled("│ ".to_string(), Style::default().fg(theme.border)),
                                Span::styled(l, Style::default().fg(theme.fg_tertiary)),
                            ]));
                        }
                        if total > shown_len {
                            out.push(Line::from(vec![
                                Span::styled("│ ".to_string(), Style::default().fg(theme.border)),
                                Span::styled(
                                    format!("… +{} lines (ctrl+e expands)", total - shown_len),
                                    Style::default().fg(theme.caption),
                                ),
                            ]));
                        }
                        if let Some(c) = code {
                            if *c != 0 {
                                out.push(Line::from(Span::styled(
                                    format!("  exit {c}"),
                                    Style::default().fg(theme.err),
                                )));
                            }
                        }
                    }
                }
                CellKind::Injected { source, preview } => {
                    out.push(Line::from(vec![
                        Span::styled("◦ context".to_string(), Style::default().fg(theme.caption)),
                        Span::styled(
                            format!(" · {source} · "),
                            Style::default().fg(theme.caption),
                        ),
                        Span::styled(
                            clamp_str(preview, width.saturating_sub(source.len() + 14)),
                            Style::default().fg(theme.fg_tertiary),
                        ),
                    ]));
                }
                CellKind::Notice { level, text } => {
                    let color = match level {
                        NoticeLevel::Info => theme.caption,
                        NoticeLevel::Warn => theme.warn_soft(),
                        NoticeLevel::Error => theme.err,
                    };
                    for l in wrap(text, width.saturating_sub(2)) {
                        out.push(Line::from(vec![
                            Span::styled("· ".to_string(), Style::default().fg(color)),
                            Span::styled(l, Style::default().fg(color)),
                        ]));
                    }
                }
            }
        }
        out
    }
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
    let mut t = s.replace('\n', " ⏎ ");
    if t.chars().count() > 120 {
        t = t.chars().take(119).collect::<String>() + "…";
    }
    t
}

fn clamp_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

/// Greedy display-width wrap with word-boundary preference; never panics on
/// CJK/emoji.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut line = String::new();
        let mut w = 0usize;
        for ch in raw.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > width && !line.is_empty() {
                // Prefer breaking at the last space when it is not too early.
                match line.rfind(' ') {
                    Some(bidx)
                        if unicode_width::UnicodeWidthStr::width(&line[..bidx]) >= width / 2 =>
                    {
                        let tail = line[bidx + 1..].to_string();
                        line.truncate(bidx);
                        out.push(std::mem::replace(&mut line, tail));
                        w = unicode_width::UnicodeWidthStr::width(line.as_str());
                    }
                    _ => {
                        out.push(std::mem::take(&mut line));
                        w = 0;
                    }
                }
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
mod tests {
    use super::*;

    fn t(session: &str) -> Transcript {
        Transcript::new(session.to_string())
    }

    #[test]
    fn streaming_text_appends_and_finalizes() {
        let mut tr = t("s");
        tr.apply(UiEvent::TextDelta {
            session: "s".into(),
            text: "Hello ".into(),
        });
        tr.apply(UiEvent::TextDelta {
            session: "s".into(),
            text: "world".into(),
        });
        assert!(tr.streaming());
        tr.apply(UiEvent::AssistantFinal {
            session: "s".into(),
            text: "Hello world!".into(),
            model: Some("m".into()),
        });
        assert!(!tr.streaming());
        match &tr.cells[0].kind {
            CellKind::Assistant { text, done, .. } => {
                assert_eq!(text, "Hello world!");
                assert!(done);
            }
            other => panic!("unexpected cell {other:?}"),
        }
    }

    #[test]
    fn tool_call_pairs_with_result() {
        let mut tr = t("s");
        tr.apply(UiEvent::ToolCall {
            session: "s".into(),
            call_id: "c1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        });
        tr.apply(UiEvent::ToolResult {
            session: "s".into(),
            call_id: "c1".into(),
            is_error: false,
            text: "a\nb".into(),
            error: None,
        });
        match &tr.cells[0].kind {
            CellKind::Tool {
                name, title, ok, ..
            } => {
                assert_eq!(name, "bash");
                assert_eq!(title, "ls");
                assert_eq!(*ok, Some(true));
            }
            other => panic!("unexpected cell {other:?}"),
        }
    }

    #[test]
    fn reasoning_closes_when_text_starts() {
        let mut tr = t("s");
        tr.apply(UiEvent::ReasoningDelta {
            session: "s".into(),
            text: "plan".into(),
        });
        tr.apply(UiEvent::TextDelta {
            session: "s".into(),
            text: "answer".into(),
        });
        match &tr.cells[0].kind {
            CellKind::Reasoning { done, .. } => assert!(done),
            other => panic!("unexpected cell {other:?}"),
        }
    }

    #[test]
    fn usage_accumulates() {
        let mut tr = t("s");
        for _ in 0..2 {
            tr.apply(UiEvent::Usage {
                session: "s".into(),
                input: 10,
                output: 5,
                cached: 3,
                reasoning: 1,
            });
        }
        assert_eq!(tr.usage.input, 20);
        assert_eq!(tr.usage.output, 10);
        assert_eq!(tr.usage.cached, 6);
    }

    #[test]
    fn wrap_handles_cjk() {
        let lines = wrap("深度求索深度求索", 8);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_smoke() {
        let mut tr = t("s");
        tr.push_user("hi".into(), false);
        tr.apply(UiEvent::TextDelta {
            session: "s".into(),
            text: "yo".into(),
        });
        let theme = Theme::dark();
        let lines = tr.lines(&theme, 40, '⠋');
        assert!(lines.len() >= 3);
    }
}
