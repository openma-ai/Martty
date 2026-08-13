//! Rendering: banner, scrollback, tips row, status bar, prompt, hints, overlays.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, RunState, AMBIENT_TIPS};
use crate::logo;
use crate::theme::Theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let theme = app.theme;
    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );
    if area.height < 6 || area.width < 24 {
        f.render_widget(
            Paragraph::new("terminal too small — need ≥ 24x6").style(Style::default().fg(theme.warn)),
            area,
        );
        return;
    }

    let composer_h = 3u16; // rounded frame: status title / input / hints edge
    let tips_h = if area.height >= 16 { 1 } else { 0 };
    let chat_h = area.height.saturating_sub(composer_h + tips_h);

    let chat = Rect::new(area.x, area.y, area.width, chat_h);
    let tips = Rect::new(area.x, area.y + chat_h, area.width, tips_h);
    let composer = Rect::new(area.x, area.y + chat_h + tips_h, area.width, composer_h);

    draw_chat(f, app, chat);
    if tips_h > 0 {
        draw_tips(f, app, tips);
    }
    draw_composer(f, app, composer);
    draw_slash_menu(f, app, composer, chat);
    draw_model_picker(f, app, area);
}

/// The composer card: a rounded frame that owns status (top edge), the input
/// line (inside), and key hints (bottom edge) — mirroring the Web UI's
/// distinct composer surface. The frame glows brand-blue while working.
fn draw_composer(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let running = !matches!(app.state, RunState::Idle);
    let border_color = if running { theme.brand } else { theme.border };

    // Manual title collision avoidance: ratatui overlaps titles when narrow.
    let left = status_title(app);
    let mut right_spans = status_right(app);
    let lw = left.width() as u16;
    let rw: u16 = right_spans.iter().map(|s| s.content.width() as u16).sum();
    if lw + rw + 4 > area.width {
        // fall back to just the model chip, then drop entirely
        let shown_model = app
            .transcript
            .last_model
            .clone()
            .unwrap_or_else(|| app.cfg.model.clone());
        let compact = format!(" {shown_model} ");
        if lw + compact.width() as u16 + 4 <= area.width {
            right_spans = vec![Span::styled(compact, Style::default().fg(theme.caption))];
        } else {
            right_spans = Vec::new();
        }
    }

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(left)
        .title_bottom(hints_title(app));
    if !right_spans.is_empty() {
        block = block.title(Line::from(right_spans).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    draw_input(f, app, inner);
}

/// Left top-edge title: run state.
fn status_title(app: &App) -> Line<'static> {
    let theme = app.theme;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    match app.state {
        RunState::Idle => {
            spans.push(Span::styled("● ", Style::default().fg(theme.ok_soft())));
            spans.push(Span::styled("idle", Style::default().fg(theme.fg_tertiary)));
            if let Some(kind) = &app.transcript.last_finish {
                spans.push(Span::styled(
                    format!(" · last turn {kind}"),
                    Style::default().fg(theme.caption),
                ));
            }
        }
        RunState::Starting | RunState::Running => {
            spans.push(Span::styled(
                format!("{} ", app.spinner()),
                Style::default().fg(theme.brand),
            ));
            let label = if app.state == RunState::Starting {
                if app.state_note.is_empty() {
                    "starting".to_string()
                } else {
                    app.state_note.clone()
                }
            } else if app.transcript.streaming() {
                "streaming".to_string()
            } else if !app.state_note.is_empty() {
                app.state_note.clone()
            } else {
                "working".to_string()
            };
            spans.push(Span::styled(label, Style::default().fg(theme.brand_soft)));
            if let Some(t0) = app.run_started {
                spans.push(Span::styled(
                    format!(" {}s", t0.elapsed().as_secs()),
                    Style::default().fg(theme.caption),
                ));
            }
            if app.queued > 0 {
                spans.push(Span::styled(
                    format!(" · {} queued", app.queued),
                    Style::default().fg(theme.warn_soft()),
                ));
            }
        }
    }
    spans.push(Span::raw(" "));
    // Mode chips: folded from the durable event stream (same facts as the
    // Web UI chips). Only facts the host actually reported are shown.
    if let Some(preset) = &app.modes.agent_preset {
        spans.push(Span::styled(
            format!("⚙ {preset} "),
            Style::default().fg(theme.fg_tertiary),
        ));
    }
    if let Some(perm) = &app.modes.permission {
        let short = perm.replace("danger-full-access", "full-access");
        spans.push(Span::styled(
            format!("⛨ {short} "),
            Style::default().fg(if short == "full-access" {
                theme.warn_soft()
            } else {
                theme.fg_tertiary
            }),
        ));
    } else if let Some(sandbox) = &app.modes.sandbox {
        spans.push(Span::styled(
            format!("⛨ {sandbox} "),
            Style::default().fg(theme.fg_tertiary),
        ));
    }
    if let Some(approval) = &app.modes.approval {
        spans.push(Span::styled(
            format!("⚖ {approval} "),
            Style::default().fg(theme.fg_tertiary),
        ));
    }
    if app.modes.plan {
        spans.push(Span::styled(
            "⌁ plan ".to_string(),
            Style::default().fg(theme.brand_soft),
        ));
    }
    Line::from(spans)
}

/// Right top-edge title: model · session · tokens.
fn status_right(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let u = app.transcript.usage;
    let mut right = String::from(" ");
    if app.scroll_up > 0 {
        right.push_str(&format!("▲{} · ", app.scroll_up));
    }
    let shown_model = app
        .transcript
        .last_model
        .clone()
        .unwrap_or_else(|| app.cfg.model.clone());
    right.push_str(&format!("{} · {}", shown_model, app.session_id));
    if u.input + u.output > 0 {
        right.push_str(&format!(" · ↑{} ↓{}", fmt_tokens(u.input), fmt_tokens(u.output)));
        if u.cached > 0 {
            right.push_str(&format!(" (cache {})", fmt_tokens(u.cached)));
        }
    }
    if app.demo {
        right.push_str(" · DEMO");
    }
    right.push(' ');
    vec![Span::styled(right, Style::default().fg(theme.caption))]
}

/// Bottom-edge title: key hints.
fn hints_title(app: &App) -> Line<'static> {
    let theme = app.theme;
    let hints = match app.state {
        RunState::Idle => " enter send · /help · ↑ history · ctrl+e expand · ctrl+t theme · ctrl+c ×2 quit ",
        _ => " enter queue · alt+enter send now · esc interrupt · pgup/pgdn scroll ",
    };
    Line::from(Span::styled(
        hints.to_string(),
        Style::default().fg(theme.caption),
    ))
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let mut lines = if app.show_banner {
        banner_lines(app, inner.width)
    } else {
        Vec::new()
    };
    lines.extend(app.transcript.lines(&theme, inner.width, app.spinner()));

    let total = lines.len();
    let h = inner.height as usize;
    let max_scroll = total.saturating_sub(h);
    if app.scroll_up > max_scroll {
        app.scroll_up = max_scroll;
    }
    let end = total - app.scroll_up.min(total);
    let start = end.saturating_sub(h);
    let visible: Vec<Line> = lines[start..end].to_vec();
    f.render_widget(Paragraph::new(visible), inner);
}

fn draw_tips(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let text = match &app.tip {
        Some((t, _)) => t.clone(),
        None => AMBIENT_TIPS[app.ambient_tip_idx % AMBIENT_TIPS.len()].to_string(),
    };
    let style = if app.tip.is_some() {
        Style::default().fg(theme.warn_soft())
    } else {
        Style::default().fg(theme.caption)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("  {text}"), style))),
        area,
    );
}


fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let prompt = "❯ ";
    let mut spans = vec![Span::styled(
        prompt.to_string(),
        Style::default().fg(theme.brand).add_modifier(Modifier::BOLD),
    )];
    if app.input.is_empty() {
        let placeholder = match app.state {
            RunState::Idle => "describe what you want to build — / commands · ! shell",
            _ => "queue a follow-up… (alt+enter interrupts and sends now)",
        };
        spans.push(Span::styled(
            placeholder.to_string(),
            Style::default().fg(theme.caption),
        ));
    } else {
        let style = if app.input.buf.starts_with('!') {
            Style::default().fg(theme.warn_soft())
        } else if app.input.buf.starts_with('/') {
            Style::default().fg(theme.brand_soft)
        } else {
            Style::default().fg(theme.fg)
        };
        spans.push(Span::styled(app.input.buf.clone(), style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // cursor: display-width based
    let before: String = app.input.buf.chars().take(app.input.cursor).collect();
    let x = area.x + prompt.width() as u16 + before.width() as u16;
    f.set_cursor_position((x.min(area.x + area.width.saturating_sub(1)), area.y));
}


fn draw_slash_menu(f: &mut Frame, app: &App, input: Rect, chat: Rect) {
    let matches = app.slash_matches();
    if matches.is_empty() {
        return;
    }
    let theme = app.theme;
    let h = (matches.len() as u16 + 2).min(chat.height);
    let w = 56.min(input.width.saturating_sub(2));
    let y = input.y.saturating_sub(h);
    let area = Rect::new(input.x + 2, y, w, h);
    f.render_widget(Clear, area);
    let mut lines = Vec::new();
    for (i, cmd) in matches.iter().enumerate() {
        let selected = i == app.slash_sel.min(matches.len() - 1);
        let marker = if selected { "▸ " } else { "  " };
        let name_style = if selected {
            Style::default().fg(theme.brand).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg_secondary)
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(format!("{:<20}", cmd.usage), name_style),
            Span::styled(cmd.desc.to_string(), Style::default().fg(theme.caption)),
        ]));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(" commands ", Style::default().fg(theme.caption)))
        .style(Style::default().bg(theme.panel));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_model_picker(f: &mut Frame, app: &App, screen: Rect) {
    let Some(picker) = &app.picker else {
        return;
    };
    let theme = app.theme;
    let h = (picker.items.len() as u16 + 2).min(screen.height.saturating_sub(2));
    let w = 58.min(screen.width.saturating_sub(4));
    let x = screen.x + (screen.width - w) / 2;
    let y = screen.y + (screen.height - h) / 3;
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);
    let mut lines = Vec::new();
    for (i, item) in picker.items.iter().enumerate() {
        let selected = i == picker.sel;
        let marker = if selected { "▸ " } else { "  " };
        let style = if selected {
            Style::default().fg(theme.brand).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg_secondary)
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(format!("{:<24}", item.label), style),
        ];
        if !item.meta.is_empty() {
            spans.push(Span::styled(
                item.meta.clone(),
                Style::default().fg(theme.caption),
            ));
        }
        if item.id == app.cfg.model && matches!(picker.kind, crate::app::PickerKind::Model) {
            spans.push(Span::styled(
                "  (current)".to_string(),
                Style::default().fg(theme.caption),
            ));
        }
        lines.push(Line::from(spans));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(
            picker.title.clone(),
            Style::default().fg(theme.caption),
        ))
        .style(Style::default().bg(theme.panel));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Welcome banner: whale, wordmark, slogans, session facts. Shown while
/// `app.show_banner` is set; the whale dives on the first real prompt.
fn banner_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let mut out = vec![Line::default()];
    out.extend(logo::whale_lines(theme, width));
    out.push(Line::default());
    out.extend(logo::wordmark_lines(theme, width));
    out.push(Line::default());

    // Hero slogan, mirroring the Web UI: whale + "Into the Unknown".
    out.push(centered(
        width,
        vec![Span::styled(
            "Into the Unknown".to_string(),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        )],
    ));
    out.push(Line::default());

    let kv = |k: &str, v: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {k:<10}"), Style::default().fg(theme.caption)),
            Span::styled(v, Style::default().fg(theme.fg_secondary)),
        ])
    };
    out.push(kv("version", format!("dsh-tui {}", env!("CARGO_PKG_VERSION"))));
    out.push(kv("model", format!("{} · {}", app.cfg.provider, app.cfg.model)));
    out.push(kv("workspace", shorten_home(&app.cfg.workspace)));
    out.push(kv("session", app.session_id.clone()));
    let runtime_short = app
        .cfg
        .bin
        .rsplit('/')
        .next()
        .unwrap_or(&app.cfg.bin)
        .to_string();
    out.push(kv("runtime", runtime_short));
    if app.demo {
        out.push(kv("mode", "demo — scripted turns, no API calls".into()));
    } else if app.attached {
        out.push(kv("credentials", "host dsh (credential seam)".into()));
    } else if let Some(source) = app.cfg.credential_source() {
        out.push(kv("credentials", source.into()));
    } else {
        out.push(Line::from(vec![
            Span::styled("  ⚠ ".to_string(), Style::default().fg(theme.warn)),
            Span::styled(
                "DEEPSEEK_API_KEY not set — export it, or relaunch with --demo".to_string(),
                Style::default().fg(theme.warn_soft()),
            ),
        ]));
    }
    out.push(Line::from(Span::styled(
        "  full workspace access — prefer a disposable checkout".to_string(),
        Style::default().fg(theme.caption),
    )));
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        "  /help commands · /keys shortcuts · ! shell · esc interrupt".to_string(),
        Style::default().fg(theme.caption),
    )));
    out
}

fn centered(width: u16, spans: Vec<Span<'static>>) -> Line<'static> {
    let w: usize = spans.iter().map(|s| s.content.width()).sum();
    let pad = (width as usize).saturating_sub(w) / 2;
    let mut v = vec![Span::raw(" ".repeat(pad))];
    v.extend(spans);
    Line::from(v)
}

fn shorten_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1e6)
    } else if n >= 10_000 {
        format!("{:.0}k", n as f64 / 1e3)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// Render one frame into a plain string (used by `--dump-frame` and tests).
pub fn dump_frame(app: &mut App, width: u16, height: u16) -> String {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, app)).expect("draw frame");
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for row in 0..buffer.area.height {
        let mut line = String::new();
        for col in 0..buffer.area.width {
            line.push_str(buffer[(col, row)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[allow(dead_code)]
pub fn theme_for(name: &str) -> Theme {
    match name {
        "light" => Theme::light(),
        _ => Theme::dark(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeConfig;
    use std::sync::mpsc;

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
        let (tx, _rx) = mpsc::channel();
        App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx)
    }

    #[test]
    fn banner_shows_until_first_prompt() {
        let mut app = test_app();
        assert!(app.show_banner, "banner defaults to on");
        assert!(
            dump_frame(&mut app, 84, 40).contains("██████"),
            "wordmark visible on launch"
        );
        app.show_banner = false;
        assert!(
            !dump_frame(&mut app, 84, 40).contains("██████"),
            "whale dives once the banner is dismissed"
        );
    }
}
