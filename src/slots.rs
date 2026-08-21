//! Compositor-private TUI slot snapshots and terminal-native node rendering.

use std::collections::HashSet;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::Deserialize;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::transcript::wrap;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotSnapshot {
    pub protocol: u64,
    pub slot: String,
    #[serde(default)]
    pub rev: Option<u64>,
    pub nodes: Vec<TuiNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum TuiAction {
    Command {
        name: String,
        #[serde(default)]
        args: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum TuiNode {
    Ascii {
        id: String,
        lines: Vec<String>,
        #[serde(default)]
        tone: Option<String>,
    },
    Logo {
        id: String,
        name: String,
    },
    Text {
        id: String,
        text: String,
        #[serde(default)]
        tone: Option<String>,
    },
    Group {
        id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        tone: Option<String>,
        children: Vec<TuiNode>,
    },
    Markdown {
        id: String,
        text: String,
        #[serde(default)]
        streaming: bool,
    },
    Reasoning {
        id: String,
        text: String,
        done: bool,
        #[serde(default)]
        seconds: Option<f64>,
    },
    User {
        id: String,
        text: String,
        #[serde(default)]
        queued: bool,
    },
    Generic {
        id: String,
        title: String,
        body: String,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        action: Option<TuiAction>,
    },
    Terminal {
        id: String,
        title: String,
        body: String,
        #[serde(default)]
        exit: Option<i64>,
    },
    Diff {
        id: String,
        title: String,
        unified: String,
        #[serde(default)]
        path: Option<String>,
    },
    Image {
        id: String,
        name: String,
        mime: String,
        #[serde(rename = "dataBase64", default)]
        data_base64: Option<String>,
    },
    Notice {
        id: String,
        level: String,
        text: String,
    },
    Unknown {
        id: String,
        want: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        detail: Option<String>,
    },
}

impl TuiNode {
    fn id(&self) -> &str {
        match self {
            Self::Ascii { id, .. }
            | Self::Logo { id, .. }
            | Self::Text { id, .. }
            | Self::Group { id, .. }
            | Self::Markdown { id, .. }
            | Self::Reasoning { id, .. }
            | Self::User { id, .. }
            | Self::Generic { id, .. }
            | Self::Terminal { id, .. }
            | Self::Diff { id, .. }
            | Self::Image { id, .. }
            | Self::Notice { id, .. }
            | Self::Unknown { id, .. } => id,
        }
    }

    pub fn action(&self) -> Option<&TuiAction> {
        match self {
            Self::Generic { action, .. } => action.as_ref(),
            _ => None,
        }
    }
}

fn validate_nodes(nodes: &[TuiNode], ids: &mut HashSet<String>) -> Result<(), String> {
    for node in nodes {
        if node.id().is_empty() {
            return Err("tui snapshot node id must not be empty".into());
        }
        if !ids.insert(node.id().to_string()) {
            return Err(format!("duplicate tui snapshot node id: {}", node.id()));
        }
        match node {
            TuiNode::Ascii { lines, tone, .. } => {
                if lines.is_empty() {
                    return Err("tui ascii lines must not be empty".into());
                }
                if tone.as_deref().is_some_and(theme_color_name_invalid) {
                    return Err(format!(
                        "unknown tui theme token: {}",
                        tone.as_deref().unwrap_or("")
                    ));
                }
            }
            TuiNode::Logo { name, .. } if name != "deepseek" => {
                return Err(format!("unknown tui logo preset: {name}"));
            }
            TuiNode::Text { tone, .. } => {
                if tone.as_deref().is_some_and(theme_color_name_invalid) {
                    return Err(format!(
                        "unknown tui theme token: {}",
                        tone.as_deref().unwrap_or("")
                    ));
                }
            }
            TuiNode::Group { tone, children, .. } => {
                if tone.as_deref().is_some_and(theme_color_name_invalid) {
                    return Err(format!(
                        "unknown tui theme token: {}",
                        tone.as_deref().unwrap_or("")
                    ));
                }
                validate_nodes(children, ids)?;
            }
            TuiNode::Generic { status, action, .. } => {
                if status
                    .as_deref()
                    .is_some_and(|status| !matches!(status, "running" | "ok" | "err"))
                {
                    return Err("tui generic status must be running, ok, or err".into());
                }
                if action.as_ref().is_some_and(|action| match action {
                    TuiAction::Command { name, .. } => name.is_empty(),
                }) {
                    return Err("tui command action name must not be empty".into());
                }
            }
            TuiNode::Notice { level, .. }
                if !matches!(level.as_str(), "info" | "warn" | "error") =>
            {
                return Err("tui notice level must be info, warn, or error".into());
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn validate_node_tree(nodes: &[TuiNode]) -> Result<(), String> {
    validate_nodes(nodes, &mut HashSet::new())
}

fn theme_color_name_invalid(name: &str) -> bool {
    !matches!(
        name,
        "bg" | "surface"
            | "panel"
            | "fg"
            | "fg_secondary"
            | "fg_tertiary"
            | "caption"
            | "brand"
            | "brand_soft"
            | "bubble_bg"
            | "bubble_fg"
            | "border"
            | "code_bg"
            | "ok"
            | "warn"
            | "err"
            | "hint"
            | "chip_bg"
    )
}

pub fn parse_snapshot(value: &Value) -> Result<Option<SlotSnapshot>, String> {
    let snapshot: SlotSnapshot =
        serde_json::from_value(value.clone()).map_err(|err| err.to_string())?;
    if snapshot.protocol != 0 {
        return Ok(None);
    }
    if !matches!(
        snapshot.slot.as_str(),
        "welcome.hero" | "chrome.right" | "conversation.input.dock" | "conversation.composer.dock"
    ) {
        return Ok(None);
    }
    validate_node_tree(&snapshot.nodes)?;
    if snapshot.slot == "welcome.hero"
        && !snapshot.nodes.is_empty()
        && !matches!(
            snapshot.nodes.as_slice(),
            [TuiNode::Logo { .. }, TuiNode::Text { .. }]
        )
    {
        return Err("welcome.hero must contain one logo followed by one hint text".into());
    }
    Ok(Some(snapshot))
}

fn token_color(theme: &Theme, name: &str) -> Color {
    match name {
        "bg" => theme.bg,
        "surface" => theme.surface,
        "panel" => theme.panel,
        "fg" => theme.fg,
        "fg_secondary" => theme.fg_secondary,
        "fg_tertiary" => theme.fg_tertiary,
        "caption" => theme.caption,
        "brand" => theme.brand,
        "brand_soft" => theme.brand_soft,
        "bubble_bg" => theme.bubble_bg,
        "bubble_fg" => theme.bubble_fg,
        "border" => theme.border,
        "code_bg" => theme.code_bg,
        "ok" => theme.ok,
        "warn" => theme.warn,
        "err" => theme.err,
        "hint" => theme.hint,
        "chip_bg" => theme.chip_bg,
        _ => theme.fg_secondary,
    }
}

fn styled_wrapped(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        for line in wrap(raw, width.max(1)) {
            lines.push(Line::from(Span::styled(line, style)));
        }
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn indent(lines: Vec<Line<'static>>, prefix: &str, color: Color) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|mut line| {
            let mut spans = vec![Span::styled(prefix.to_string(), Style::default().fg(color))];
            spans.append(&mut line.spans);
            Line::from(spans)
        })
        .collect()
}

fn render_node(node: &TuiNode, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    match node {
        TuiNode::Ascii { lines, tone, .. } => styled_wrapped(
            &lines.join("\n"),
            width,
            Style::default().fg(tone
                .as_deref()
                .map(|name| token_color(theme, name))
                .unwrap_or(theme.fg)),
        ),
        TuiNode::Logo { name, .. } if name == "deepseek" => {
            crate::deepseek_logo::lines(theme, width as u16)
        }
        TuiNode::Logo { .. } => Vec::new(),
        TuiNode::Text { text, tone, .. } => styled_wrapped(
            text,
            width,
            Style::default().fg(tone
                .as_deref()
                .map(|name| token_color(theme, name))
                .unwrap_or(theme.fg_secondary)),
        ),
        TuiNode::Group {
            title,
            tone,
            children,
            ..
        } => {
            let accent = tone
                .as_deref()
                .map(|name| token_color(theme, name))
                .unwrap_or(theme.brand);
            let mut lines = Vec::new();
            if let Some(title) = title {
                lines.push(Line::from(vec![
                    Span::styled("◆ ", Style::default().fg(accent)),
                    Span::styled(
                        title.clone(),
                        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            for child in children {
                lines.extend(indent(
                    render_node(child, theme, width.saturating_sub(2)),
                    "  ",
                    theme.border,
                ));
            }
            lines
        }
        TuiNode::Markdown {
            text, streaming, ..
        } => {
            let mut lines = crate::markdown::render(text, theme, width);
            if *streaming {
                lines.push(Line::from(Span::styled(
                    "…",
                    Style::default().fg(theme.brand),
                )));
            }
            lines
        }
        TuiNode::Reasoning {
            text,
            done,
            seconds,
            ..
        } => {
            let label = seconds
                .map(|seconds| format!("thought · {seconds:.1}s"))
                .unwrap_or_else(|| "thought".into());
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    if *done { "✦ " } else { "✦ … " },
                    Style::default().fg(theme.brand),
                ),
                Span::styled(
                    label,
                    Style::default()
                        .fg(theme.caption)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])];
            lines.extend(styled_wrapped(
                text,
                width,
                Style::default()
                    .fg(theme.fg_tertiary)
                    .add_modifier(Modifier::ITALIC),
            ));
            lines
        }
        TuiNode::User { text, queued, .. } => {
            let prefix = if *queued { "› queued · " } else { "› " };
            indent(
                styled_wrapped(
                    text,
                    width.saturating_sub(prefix.len()),
                    Style::default().fg(theme.bubble_fg),
                ),
                prefix,
                theme.brand,
            )
        }
        TuiNode::Generic {
            title,
            body,
            status,
            ..
        } => {
            let (icon, color) = match status.as_deref() {
                Some("running") => ("●", theme.brand),
                Some("ok") => ("✓", theme.ok),
                Some("err") => ("×", theme.err),
                _ => ("◇", theme.caption),
            };
            let mut lines = vec![Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(
                    title.clone(),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
            ])];
            lines.extend(indent(
                styled_wrapped(
                    body,
                    width.saturating_sub(2),
                    Style::default().fg(theme.fg_secondary),
                ),
                "  ",
                theme.border,
            ));
            lines
        }
        TuiNode::Terminal {
            title, body, exit, ..
        } => {
            let suffix = exit
                .map(|code| format!(" · exit {code}"))
                .unwrap_or_default();
            let mut lines = vec![Line::from(vec![
                Span::styled("$ ", Style::default().fg(theme.brand)),
                Span::styled(
                    format!("{title}{suffix}"),
                    Style::default().fg(theme.caption),
                ),
            ])];
            lines.extend(styled_wrapped(
                body,
                width,
                Style::default().fg(theme.fg_secondary).bg(theme.code_bg),
            ));
            lines
        }
        TuiNode::Diff {
            title,
            unified,
            path,
            ..
        } => {
            let mut lines = vec![Line::from(vec![
                Span::styled("Δ ", Style::default().fg(theme.brand)),
                Span::styled(
                    path.as_deref().unwrap_or(title).to_string(),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
            ])];
            for raw in unified.lines() {
                let color = if raw.starts_with('+') && !raw.starts_with("+++") {
                    theme.ok
                } else if raw.starts_with('-') && !raw.starts_with("---") {
                    theme.err
                } else {
                    theme.fg_tertiary
                };
                lines.extend(styled_wrapped(
                    raw,
                    width,
                    Style::default().fg(color).bg(theme.code_bg),
                ));
            }
            lines
        }
        TuiNode::Image {
            name,
            mime,
            data_base64,
            ..
        } => vec![Line::from(vec![
            Span::styled("▧ ", Style::default().fg(theme.brand)),
            Span::styled(name.clone(), Style::default().fg(theme.fg)),
            Span::styled(
                format!(
                    " · {mime}{}",
                    data_base64
                        .as_ref()
                        .map(|data| format!(" · {} chars", data.len()))
                        .unwrap_or_default()
                ),
                Style::default().fg(theme.caption),
            ),
        ])],
        TuiNode::Notice { level, text, .. } => {
            let (icon, color) = match level.as_str() {
                "warn" => ("!", theme.warn),
                "error" => ("×", theme.err),
                _ => ("i", theme.hint),
            };
            indent(
                styled_wrapped(
                    text,
                    width.saturating_sub(2),
                    Style::default().fg(theme.fg_secondary),
                ),
                &format!("{icon} "),
                color,
            )
        }
        TuiNode::Unknown {
            want,
            title,
            detail,
            ..
        } => {
            let mut lines = vec![Line::from(vec![
                Span::styled("? ", Style::default().fg(theme.warn)),
                Span::styled(
                    title.as_deref().unwrap_or(want).to_string(),
                    Style::default().fg(theme.fg),
                ),
            ])];
            if let Some(detail) = detail {
                lines.extend(indent(
                    styled_wrapped(
                        detail,
                        width.saturating_sub(2),
                        Style::default().fg(theme.caption),
                    ),
                    "  ",
                    theme.border,
                ));
            }
            lines
        }
    }
}

pub fn render_nodes(nodes: &[TuiNode], theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.extend(render_node(node, theme, width));
    }
    lines
}

pub fn render(snapshot: &SlotSnapshot, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    render_nodes(&snapshot.nodes, theme, width)
}

fn centered(width: usize, mut line: Line<'static>) -> Line<'static> {
    let content_width = line
        .spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    line.spans.insert(
        0,
        Span::raw(" ".repeat(width.saturating_sub(content_width) / 2)),
    );
    line
}

/// Render the structural `logo + hint` contract of `welcome.hero`.
/// Logo painters own their internal geometry; the compositor owns the gap
/// and centers the hint independently below it.
pub fn render_welcome_hero(
    snapshot: &SlotSnapshot,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let [logo, hint] = snapshot.nodes.as_slice() else {
        return Vec::new();
    };
    let mut lines = render_node(logo, theme, width);
    lines.push(Line::default());
    lines.extend(
        render_node(hint, theme, width)
            .into_iter()
            .map(|line| centered(width, line)),
    );
    lines
}
