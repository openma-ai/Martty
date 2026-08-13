//! Rendering: banner, scrollback, tips row, status bar, prompt, hints, overlays.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, RunState, AMBIENT_TIPS};
use crate::logo;
use crate::logo_data::WHALE_XS;
use crate::pet::{FRAME_H, FRAME_W};
use crate::theme::{lerp, Theme};

/// Columns the composer text keeps clear of the pet at its right edge
/// (widest pet form is 8 cols, plus a one-column gap).
const PET_PAD: u16 = 9;

/// Composer card height for a terminal `height` rows tall.
fn composer_height(height: u16) -> u16 {
    if height >= 15 {
        5
    } else if height >= 10 {
        4
    } else {
        3
    }
}

/// The pet's cell rectangle — the kitty-graphics placement target —
/// perched on the composer's bottom-right, matching the sprite's 192:208
/// aspect in 1:2 cells. None hides it (`/liang` off, or the terminal is too
/// cramped to give up columns).
pub fn pet_rect(area: Rect, app: &App) -> Option<Rect> {
    if !app.pet_visible || area.width < 60 || area.height < 10 {
        return None;
    }
    let rows = (composer_height(area.height) - 1).min(4);
    let cols = ((rows as u32 * 2 * FRAME_W + FRAME_H / 2) / FRAME_H) as u16;
    Some(Rect::new(
        area.right() - cols,
        area.bottom() - rows,
        cols,
        rows,
    ))
}

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

    // Composer card: status row / input well / hints row. The well gets
    // extra breathing room (and wraps long prompts) on taller terminals.
    let composer_h = composer_height(area.height);
    let tips_h = if area.height >= 16 { 1 } else { 0 };
    let chat_h = area.height.saturating_sub(composer_h + tips_h);

    let chat = Rect::new(area.x, area.y, area.width, chat_h);
    let tips = Rect::new(area.x, area.y + chat_h, area.width, tips_h);
    let composer = Rect::new(area.x, area.y + chat_h + tips_h, area.width, composer_h);

    draw_chat(f, app, chat);
    if tips_h > 0 {
        draw_tips(f, app, tips);
    }
    // The pet floats on the composer surface's right end; composer text
    // keeps clear of it.
    let pet = pet_rect(area, app);
    draw_composer(f, app, composer, if pet.is_some() { PET_PAD } else { 0 });
    if let Some(cells) = pet {
        if !app.pet_pixels {
            // No pixel protocol: the half-block whale stands in.
            draw_pet_chars(f, theme, cells);
        }
        // Otherwise main() places the favicon PNG over `cells` after draw.
    }
    draw_slash_menu(f, app, composer, chat);
    draw_model_picker(f, app, area);
}

/// Fallback pet for terminals without a pixel protocol: the XS half-block
/// whale, brand gradient, anchored bottom-right like the real image.
fn draw_pet_chars(f: &mut Frame, theme: Theme, cells: Rect) {
    let w = WHALE_XS.iter().map(|r| r.chars().count()).max().unwrap_or(0) as u16;
    let h = WHALE_XS.len() as u16;
    let rect = Rect::new(
        cells.right().saturating_sub(w),
        cells.bottom().saturating_sub(h),
        w,
        h,
    );
    let (top, bottom) = theme.whale_gradient();
    let last = WHALE_XS.len().max(2) - 1;
    let lines: Vec<Line> = WHALE_XS
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let color = lerp(top, bottom, i as f32 / last as f32);
            Line::from(Span::styled(*row, Style::default().fg(color)))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rect);
}

/// The composer card: a borderless tinted surface (panel bg) that owns the
/// status row, the input well, and the key-hint row — mirroring the Web UI's
/// distinct composer surface. A brand-blue edge bar glows while working.
/// `pet_pad` columns at the right stay text-free for the whale pet.
fn draw_composer(f: &mut Frame, app: &App, area: Rect, pet_pad: u16) {
    let theme = app.theme;
    let running = !matches!(app.state, RunState::Idle);

    // Surface fill: contrast against the chat bg does the framing.
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        area,
    );
    // Left edge bar: the working "glow" (the old border used to do this).
    if running {
        let bar: Vec<Line> = (0..area.height).map(|_| Line::from("▎")).collect();
        f.render_widget(
            Paragraph::new(bar).style(Style::default().fg(theme.brand)),
            Rect::new(area.x, area.y, 1, area.height),
        );
    }

    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2 + pet_pad),
        area.height,
    );
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    // Status row: left state + right model/tokens, collision-aware.
    let left = status_title(app);
    let mut right_spans = status_right(app);
    let w = inner.width as usize;
    let lw = left.width();
    let rw: usize = right_spans.iter().map(|s| s.content.width()).sum();
    if lw + rw + 2 > w {
        // fall back to just the model chip, then drop entirely
        let shown_model = app
            .transcript
            .last_model
            .clone()
            .unwrap_or_else(|| app.cfg.model.clone());
        let compact = format!("{shown_model} ");
        if lw + compact.width() + 2 <= w {
            right_spans = vec![Span::styled(compact, Style::default().fg(theme.brand_soft))];
        } else {
            right_spans = Vec::new();
        }
    }
    let rw: usize = right_spans.iter().map(|s| s.content.width()).sum();
    let mut spans = left.spans;
    spans.push(Span::raw(" ".repeat(w.saturating_sub(lw + rw))));
    spans.extend(right_spans);
    f.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Input well between the status and hint rows.
    let well = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 2);
    draw_input(f, app, well);

    f.render_widget(
        Paragraph::new(hints_title(app)),
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

/// Status row, left side: run state.
fn status_title(app: &App) -> Line<'static> {
    let theme = app.theme;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    match app.state {
        RunState::Idle => {
            spans.push(Span::styled("● ", Style::default().fg(theme.ok_soft())));
            spans.push(Span::styled("idle", Style::default().fg(theme.fg_tertiary)));
            // A clean finish is implied by "idle" — only surface the
            // exceptional endings, in their own accent color.
            match app.transcript.last_finish.as_deref() {
                None | Some("completed") | Some("stop") => {}
                Some(kind) => {
                    let color = if kind == "error" { theme.err } else { theme.warn_soft() };
                    spans.push(Span::styled(
                        format!(" · {kind}"),
                        Style::default().fg(color),
                    ));
                }
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

/// Status row, right side: model chip (brand accent) · token flow. The session
/// id lives in the banner and `/status` — it was pure noise here.
fn status_right(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let u = app.transcript.usage;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    if app.scroll_up > 0 {
        spans.push(Span::styled(
            format!("▲{} · ", app.scroll_up),
            Style::default().fg(theme.caption),
        ));
    }
    let shown_model = app
        .transcript
        .last_model
        .clone()
        .unwrap_or_else(|| app.cfg.model.clone());
    spans.push(Span::styled(
        shown_model,
        Style::default().fg(theme.brand_soft),
    ));
    if u.input + u.output > 0 {
        let mut t = format!(" · ↑{} ↓{}", fmt_tokens(u.input), fmt_tokens(u.output));
        if u.cached > 0 {
            t.push_str(&format!(" (cache {})", fmt_tokens(u.cached)));
        }
        spans.push(Span::styled(t, Style::default().fg(theme.caption)));
    }
    if app.demo {
        spans.push(Span::styled(
            " · demo".to_string(),
            Style::default().fg(theme.warn_soft()),
        ));
    }
    spans.push(Span::raw(" "));
    spans
}

/// Hint row: a handful of key hints — bold key, dim label, faint
/// separators. Everything else lives in `/help` and `/keys`.
fn hints_title(app: &App) -> Line<'static> {
    let theme = app.theme;
    let key = Style::default()
        .fg(theme.fg_tertiary)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::default().fg(theme.caption);
    let pairs: &[(&str, &str)] = match app.state {
        RunState::Idle => &[
            ("⏎", "send"),
            ("/", "commands"),
            ("!", "shell"),
            ("↑", "history"),
            ("^t", "theme"),
        ],
        _ => &[("⏎", "queue"), ("alt+⏎", "send now"), ("esc", "interrupt")],
    };
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, (k, l)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ".to_string(), Style::default().fg(theme.border)));
        }
        spans.push(Span::styled(k.to_string(), key));
        spans.push(Span::styled(format!(" {l}"), lbl));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
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

    // Layout snapshot for mouse selection: hit-testing and copy extraction
    // read exactly what this frame showed (grok-build's resolved selection
    // model, scaled down to plain text per wrapped line).
    app.chat_view.area = inner;
    app.chat_view.top = start;
    app.chat_view.lines = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    f.render_widget(Paragraph::new(visible), inner);
    draw_selection_overlay(f, app, inner, start);
}

/// Paint the in-app mouse selection as reversed cells — the live highlight
/// for grok-style drag-select-copy. Reversal is theme-agnostic and reads
/// like a native terminal selection.
fn draw_selection_overlay(f: &mut Frame, app: &App, inner: Rect, start: usize) {
    let Some(sel) = app.sel else { return };
    let (s, e) = sel.ordered();
    let buf = f.buffer_mut();
    for r in 0..inner.height {
        let li = start + r as usize;
        if li < s.line || li > e.line {
            continue;
        }
        let Some(text) = app.chat_view.lines.get(li) else {
            continue;
        };
        let lw = text.trim_end().width();
        let c0 = if li == s.line { s.col } else { 0 };
        let mut c1 = (if li == e.line { e.col + 1 } else { lw }).min(lw);
        if c1 <= c0 {
            if c0 == 0 {
                c1 = 1; // 1-cell sliver keeps empty rows visually continuous
            } else {
                continue;
            }
        }
        for c in c0..c1.min(inner.width as usize) {
            if let Some(cell) = buf.cell_mut((inner.x + c as u16, inner.y + r)) {
                cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
    }
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


/// The input well. Long prompts wrap across the (now taller) well, and the
/// cursor follows the wrap; `/` and `!` prefixes recolor the whole line.
fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    if area.width < 4 || area.height == 0 {
        return;
    }
    let prompt = "❯ ";
    let pw = prompt.width();
    let prompt_span = Span::styled(
        prompt.to_string(),
        Style::default().fg(theme.brand).add_modifier(Modifier::BOLD),
    );

    if app.input.is_empty() {
        let placeholder = match app.state {
            RunState::Idle => "describe what you want to build…",
            _ => "queue a follow-up — alt+⏎ sends it now",
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                prompt_span,
                Span::styled(
                    placeholder.to_string(),
                    Style::default()
                        .fg(theme.caption)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])),
            area,
        );
        f.set_cursor_position((area.x + pw as u16, area.y));
        return;
    }

    let style = if app.input.buf.starts_with('!') {
        Style::default().fg(theme.warn_soft())
    } else if app.input.buf.starts_with('/') {
        Style::default().fg(theme.brand_soft)
    } else {
        Style::default().fg(theme.fg)
    };

    // Manual display-width wrap so the cursor tracks across rows.
    let avail = (area.width as usize).saturating_sub(pw).max(1);
    let mut rows: Vec<String> = vec![String::new()];
    let mut col = 0usize;
    let mut cursor = (0usize, 0usize); // (row, display col)
    for (i, ch) in app.input.buf.chars().enumerate() {
        let cw = ch.width().unwrap_or(1).max(1);
        if col + cw > avail {
            rows.push(String::new());
            col = 0;
        }
        if i == app.input.cursor {
            cursor = (rows.len() - 1, col);
        }
        rows.last_mut().expect("wrap row").push(ch);
        col += cw;
    }
    if app.input.cursor >= app.input.buf.chars().count() {
        if col >= avail {
            rows.push(String::new());
            col = 0;
        }
        cursor = (rows.len() - 1, col);
    }

    // Keep the cursor row inside the well; otherwise show the tail.
    let h = area.height as usize;
    let mut start = rows.len().saturating_sub(h);
    if cursor.0 < start {
        start = cursor.0;
    }

    let mut lines: Vec<Line> = Vec::new();
    for (r, row) in rows.iter().enumerate().skip(start).take(h) {
        let lead = if r == 0 {
            prompt_span.clone()
        } else {
            Span::raw(" ".repeat(pw))
        };
        lines.push(Line::from(vec![lead, Span::styled(row.clone(), style)]));
    }
    f.render_widget(Paragraph::new(lines), area);

    let cy = area.y + (cursor.0 - start).min(h - 1) as u16;
    let cx = area.x + pw as u16 + cursor.1 as u16;
    f.set_cursor_position((cx.min(area.x + area.width.saturating_sub(1)), cy));
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
    // The id marked "(current)": the active model, or the effective agent mode.
    let current_mode = app.current_mode();
    let current_id: Option<&str> = match picker.kind {
        crate::app::PickerKind::Model => Some(app.cfg.model.as_str()),
        crate::app::PickerKind::Mode => Some(current_mode.as_str()),
        crate::app::PickerKind::Permission => Some(app.current_permission()),
        crate::app::PickerKind::Effort => None,
    };
    let h = (picker.items.len() as u16 + 2).min(screen.height.saturating_sub(2));
    // Fit the widest row (marker + padded label + ✓ + meta); cap to the screen.
    let needed = picker
        .items
        .iter()
        .map(|item| {
            let label_w = item.label.width()
                + if Some(item.id.as_str()) == current_id { 2 } else { 0 };
            2 + label_w.max(24) + item.meta.width()
        })
        .max()
        .unwrap_or(0) as u16;
    let cap = screen.width.saturating_sub(4).max(24);
    let w = (needed + 2).max(58).min(cap);
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
        // The current model/mode gets a ✓ pinned to its label — it survives
        // narrow terminals, unlike a right-edge tag.
        let label = if Some(item.id.as_str()) == current_id {
            format!("{} ✓", item.label)
        } else {
            item.label.clone()
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(format!("{label:<24}"), style),
        ];
        if !item.meta.is_empty() {
            spans.push(Span::styled(
                item.meta.clone(),
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
            Style::default()
                .fg(theme.fg_tertiary)
                .add_modifier(Modifier::BOLD),
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
    fn composer_is_a_tinted_surface() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = test_app();
        app.show_banner = false;
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
        let buf = terminal.backend().buffer().clone();
        let theme = app.theme;
        // height 20 → composer rows 15..20 (status / 3-row well / hints).
        assert_eq!(buf[(4, 15)].bg, theme.panel, "status row on panel surface");
        assert_eq!(buf[(40, 17)].bg, theme.panel, "input well on panel surface");
        assert_eq!(buf[(4, 19)].bg, theme.panel, "hint row on panel surface");
        assert_eq!(buf[(4, 10)].bg, theme.bg, "chat keeps the base background");
    }

    #[test]
    fn long_input_wraps_in_the_well() {
        let mut app = test_app();
        app.show_banner = false;
        app.input.buf = "a".repeat(50);
        app.input.cursor = 50;
        // 40x12 → composer height 4 → a 2-row input well, 36 text cols wide.
        let frame = dump_frame(&mut app, 40, 12);
        let wrapped = frame.lines().filter(|l| l.contains("aaaa")).count();
        assert!(wrapped >= 2, "input should wrap across well rows:\n{frame}");
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

    #[test]
    fn pet_rect_geometry() {
        let mut app = test_app();
        let area = Rect::new(0, 0, 100, 34);
        // 34 rows → 5-row composer → 4-row pet (192:208 sprite → 7 cols).
        assert_eq!(pet_rect(area, &app), Some(Rect::new(93, 30, 7, 4)));
        app.pet_visible = false;
        assert_eq!(pet_rect(area, &app), None, "/liang off hides him");
        app.pet_visible = true;
        let narrow = Rect::new(0, 0, 50, 34);
        assert_eq!(pet_rect(narrow, &app), None, "narrow terminals keep their columns");
    }

    #[test]
    fn pet_falls_back_to_half_blocks_and_toggles() {
        let mut app = test_app();
        app.show_banner = false;
        // pet_pixels=false (no kitty graphics): XS art at the right edge.
        let frame = dump_frame(&mut app, 100, 34);
        assert!(
            frame.lines().any(|l| l.ends_with("▄███▄█▄▄")),
            "XS whale flush right:\n{frame}"
        );

        // A pixel-protocol terminal draws nothing — the image goes on top.
        app.pet_pixels = true;
        let frame = dump_frame(&mut app, 100, 34);
        assert!(!frame.contains("▄███"), "cells stay clear for the PNG:\n{frame}");

        // /pet off → gone entirely.
        app.pet_pixels = false;
        app.pet_visible = false;
        let frame = dump_frame(&mut app, 100, 34);
        assert!(!frame.contains("▄███"), "hidden by /pet:\n{frame}");
    }

    #[test]
    fn mode_picker_renders_modes_and_marks_the_current_one() {
        use crate::app::{Picker, PickerItem, PickerKind, AGENT_MODES};
        let mut app = test_app();
        app.show_banner = false;
        app.modes.agent_preset = Some("minimal".into());
        app.picker = Some(Picker {
            kind: PickerKind::Mode,
            title: " agent mode · enter select · esc close ".into(),
            sel: 2,
            items: AGENT_MODES
                .iter()
                .map(|(id, name, desc)| PickerItem {
                    id: id.to_string(),
                    label: name.to_string(),
                    meta: desc.to_string(),
                    provider: None,
                })
                .collect(),
        });
        let frame = dump_frame(&mut app, 100, 30);
        for name in ["Standard mode", "Code mode", "Minimal mode", "Creator mode"] {
            assert!(frame.contains(name), "{name} listed in the picker\n{frame}");
        }
        let minimal_row = frame
            .lines()
            .find(|l| l.contains("Minimal mode"))
            .expect("minimal row");
        assert!(minimal_row.contains("Minimal mode ✓"), "current mode marked: {minimal_row}");
        assert!(minimal_row.contains("▸"), "selection marker on the current row");
        assert!(frame.contains("bash + str_replace_editor"), "descriptions visible\n{frame}");
    }

    #[test]
    fn scroll_up_survives_draw_and_shows_indicator() {
        let mut app = test_app();
        app.show_banner = false;
        for i in 0..40 {
            app.transcript.push_user(format!("line {i}"), false);
        }
        app.scroll_by(20);
        let frame = dump_frame(&mut app, 100, 14);
        assert!(app.scroll_up > 0, "scroll_up clamped to zero");
        assert!(frame.contains("▲"), "scroll indicator missing:\n{frame}");
    }

    #[test]
    fn draw_fills_the_chat_view_snapshot() {
        let mut app = test_app();
        let _ = dump_frame(&mut app, 84, 40);
        assert!(app.chat_view.area.width > 0, "chat pane rect captured");
        assert!(
            !app.chat_view.lines.is_empty(),
            "plain-text layout captured for mouse selection"
        );
        assert!(
            app.chat_view.lines.iter().any(|l| l.contains("Into the Unknown")),
            "snapshot mirrors rendered content"
        );
    }

    #[test]
    fn selection_overlay_reverses_cells() {
        use crate::app::{SelPoint, Selection};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = test_app();
        app.show_banner = false;
        app.transcript.push_user("hello selection world".into(), false);
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        // First draw fills chat_view; then select and draw again.
        terminal.draw(|f| draw(f, &mut app)).expect("warmup");
        let line = app
            .chat_view
            .lines
            .iter()
            .position(|l| l.contains("hello"))
            .expect("user line in layout");
        app.sel = Some(Selection {
            anchor: SelPoint { line, col: 0 },
            head: SelPoint { line, col: 8 },
        });
        terminal.draw(|f| draw(f, &mut app)).expect("redraw");
        let buf = terminal.backend().buffer();
        let row = app.chat_view.area.y + (line - app.chat_view.top) as u16;
        let x = app.chat_view.area.x;
        let reversed = (0..9u16)
            .filter(|c| {
                buf[(x + c, row)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            })
            .count();
        assert_eq!(reversed, 9, "anchor..=head cells are highlighted");
        assert_eq!(
            app.selection_text(app.sel.unwrap()),
            crate::app::slice_by_cells(&app.chat_view.lines[line], 0, 9)
                .trim_end(),
            "copied text matches the highlighted cells"
        );
    }
}
