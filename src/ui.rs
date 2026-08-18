//! Rendering: banner, scrollback, tips row, status bar, prompt, hints, overlays.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, RunState};
use crate::logo;
use crate::logo_data::WHALE_XS;
use crate::pet::{SPRITE_H, SPRITE_W};
use crate::theme::{lerp, Theme};

/// Columns the composer text keeps clear of the pet at its right edge
/// (widest pet form is 8 cols, plus a one-column gap).
const PET_PAD: u16 = 9;

/// Composer card height for a terminal `height` rows tall.
/// Composer height: the input well plus one bottom meta row (state ·
/// mode/permission chips · model). Taller terminals get a taller well.
fn composer_height(height: u16) -> u16 {
    if height >= 15 {
        4
    } else if height >= 10 {
        3
    } else {
        2
    }
}

/// Grow with hard/soft-wrapped draft rows, while leaving at least half of a
/// normal terminal to the conversation. Beyond the cap, `draw_input` keeps a
/// cursor-following viewport inside the composer.
fn resolved_composer_height(area: Rect, app: &App) -> u16 {
    let minimum = composer_height(area.height);
    let pet_pad = if app.pet_visible && area.width >= 60 && area.height >= 10 {
        PET_PAD
    } else {
        0
    };
    let inner_width = area.width.saturating_sub(2 + pet_pad);
    let prompt_width = "❯ ".width() as u16;
    let wrap_width = inner_width.saturating_sub(prompt_width).max(1) as usize;
    let maximum = (area.height / 2).max(minimum).min(12);
    let desired = app
        .input
        .visual_row_count(wrap_width)
        .saturating_add(1)
        .min(maximum as usize) as u16;
    desired.max(minimum).min(maximum)
}

/// The pet's cell rectangle — the kitty-graphics placement target —
/// perched on the composer's bottom-right, matching the sprite's 192:208
/// aspect in 1:2 cells. None hides it (`/liang` off, or the terminal is too
/// cramped to give up columns).
pub fn pet_rect(area: Rect, app: &App) -> Option<Rect> {
    if !app.pet_visible || area.width < 60 || area.height < 10 {
        return None;
    }
    let rows = composer_height(area.height).min(4);
    let cols = ((rows as u32 * 2 * SPRITE_W + SPRITE_H / 2) / SPRITE_H) as u16;
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
    app.slot_actions.clear();
    f.render_widget(
        Block::default().style(
            Style::default()
                .bg(app.canvas_background_color())
                .fg(theme.fg),
        ),
        area,
    );
    if area.height < 6 || area.width < 24 {
        f.render_widget(
            Paragraph::new("terminal too small — need ≥ 24x6")
                .style(Style::default().fg(theme.warn)),
            area,
        );
        return;
    }

    let (main, right) = shell_areas(area, app);

    // Composer card: input well on top, one meta row (run state + mode /
    // permission chips + model) at the bottom — the old shortcut-hints row
    // is gone (the tip banner and /keys carry that). Client plugins own the
    // additive docks immediately above and below this native input surface.
    let child_view = app.active_subagent.is_some();
    let composer_h = if child_view {
        1
    } else {
        resolved_composer_height(main, app)
    };
    let input_dock_h =
        if !child_view && main.height >= 16 && slot_has_nodes(app, "conversation.input.dock") {
            1
        } else {
            0
        };
    let composer_dock_h =
        if !child_view && main.height >= 20 && slot_has_nodes(app, "conversation.composer.dock") {
            1
        } else {
            0
        };
    let agents_h = if app.subagents.is_empty() { 0 } else { 1 };
    // Keep the conversation visually detached from the composer chrome.
    // This row is intentionally left untouched so the canvas/background
    // shows through instead of becoming another panel-colored separator.
    let gap_h = if child_view { 0 } else { 1 };
    // One-row rounded cap fused with the text (`╭ Tip · … ─╮`): the
    // 盖子 at its shortest — the tip floats inside the border line.
    let tips_h = if !child_view && main.height >= 16 {
        1
    } else {
        0
    };
    let cap_h = input_dock_h + tips_h;
    let chat_h = main
        .height
        .saturating_sub(composer_h + cap_h + composer_dock_h + agents_h + gap_h);

    let chat = Rect::new(main.x, main.y, main.width, chat_h);
    let chrome_y = main.y + chat_h + gap_h;
    let agents = Rect::new(main.x, chrome_y, main.width, agents_h);
    let cap = Rect::new(main.x, chrome_y + agents_h, main.width, cap_h);
    let composer = Rect::new(main.x, chrome_y + agents_h + cap_h, main.width, composer_h);
    let composer_dock = Rect::new(
        main.x,
        chrome_y + agents_h + cap_h + composer_h,
        main.width,
        composer_dock_h,
    );

    draw_chat(f, app, chat);
    if agents_h > 0 {
        draw_agent_rail(f, app, agents);
    }
    if cap_h > 0 {
        draw_composer_cap(f, app, cap, input_dock_h > 0);
    }
    // The pet floats on the composer surface's right end; composer text
    // keeps clear of it.
    let pet = if child_view {
        None
    } else {
        pet_rect(main, app)
    };
    let pet_pad = if pet.is_some() { PET_PAD } else { 0 };
    if child_view {
        draw_child_navigation(f, app, composer);
    } else {
        draw_composer(f, app, composer, pet_pad);
    }
    if composer_dock_h > 0 {
        draw_composer_dock(f, app, composer_dock, pet_pad);
    }
    if let Some(right) = right {
        draw_right_slot(f, app, right);
    }
    if let Some(cells) = pet {
        if !app.pet_pixels {
            // No pixel protocol: the half-block whale stands in.
            draw_pet_chars(f, theme, cells);
        }
        // Otherwise main() places the favicon PNG over `cells` after draw.
    }
    if !child_view {
        draw_slash_menu(f, app, composer, chat);
        // Hover/cursor preview for inline [image n] chips sits above the
        // composer (drawn last so it tops the menu-free chat area).
        draw_attachment_preview(f, app, composer, area);
    }
    draw_model_picker(f, app, area);
    draw_plugin_slider(f, app, area);
    draw_plugin_view(f, app, area);
    draw_permission_ask(f, app, area);
    draw_elicitation_form(f, app, area);
}

fn slider_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn draw_plugin_slider(f: &mut Frame, app: &App, screen: Rect) {
    let Some(slider) = &app.slider_overlay else {
        return;
    };
    let theme = app.theme;
    let width = screen.width.saturating_sub(4).min(72).max(24);
    let track_width = width.saturating_sub(8).max(12) as usize;
    let span = slider.max - slider.min;
    let position = if span > 0.0 {
        (((slider.value - slider.min) / span) * (track_width.saturating_sub(1) as f64))
            .round()
            .clamp(0.0, track_width.saturating_sub(1) as f64) as usize
    } else {
        0
    };
    let mut track = vec!['─'; track_width];
    for cell in track.iter_mut().take(position) {
        *cell = '━';
    }
    for mark in &slider.marks {
        let at = (((mark.value - slider.min) / span) * (track_width.saturating_sub(1) as f64))
            .round()
            .clamp(0.0, track_width.saturating_sub(1) as f64) as usize;
        track[at] = '●';
    }
    track[position] = '◆';

    let marks = slider
        .marks
        .iter()
        .map(|mark| format!("{} {}", slider_number(mark.value), mark.label))
        .collect::<Vec<_>>()
        .join("   ·   ");
    let mut lines = vec![
        Line::from(Span::styled(
            track.into_iter().collect::<String>(),
            Style::default().fg(theme.brand),
        )),
        Line::from(vec![
            Span::styled(
                slider_number(slider.value),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" / {}", slider_number(slider.max)),
                Style::default().fg(theme.caption),
            ),
        ]),
    ];
    if !marks.is_empty() {
        lines.push(Line::from(Span::styled(
            marks,
            Style::default().fg(theme.fg_secondary),
        )));
    }
    lines.push(Line::from(Span::styled(
        "←/→ preview   enter apply   esc cancel",
        Style::default().fg(theme.caption),
    )));

    let height = (lines.len() as u16 + 2).min(screen.height.saturating_sub(2));
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 3,
        width,
        height,
    );
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(
            format!(" {} ", slider.title),
            Style::default().fg(theme.fg),
        ))
        .style(Style::default().bg(theme.panel));
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}

fn draw_plugin_view(f: &mut Frame, app: &App, screen: Rect) {
    let Some(view) = &app.view_overlay else {
        return;
    };
    let theme = app.theme;
    let width = screen.width.saturating_sub(4).min(84).max(24);
    let inner_width = width.saturating_sub(2) as usize;
    let lines = crate::slots::render_nodes(&view.nodes, &theme, inner_width);
    let height = (lines.len() as u16 + 2)
        .min(screen.height.saturating_sub(2))
        .max(4);
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 3,
        width,
        height,
    );
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(
            format!(" {} · esc close ", view.title),
            Style::default().fg(theme.fg),
        ))
        .style(Style::default().bg(theme.panel).fg(theme.fg));
    f.render_widget(
        Paragraph::new(lines)
            .scroll((view.scroll.min(u16::MAX as usize) as u16, 0))
            .block(block),
        area,
    );
}

/// Reserve a root-level right rail only while a plugin has content. Narrow
/// terminals keep the original full-width shell and reveal the rail again
/// automatically when enough columns are available.
fn shell_areas(area: Rect, app: &App) -> (Rect, Option<Rect>) {
    let has_nodes = slot_has_nodes(app, "chrome.right");
    if !has_nodes || area.width < 88 {
        return (area, None);
    }
    let right_width = (area.width / 3).clamp(30, 44);
    let main = Rect::new(area.x, area.y, area.width - right_width, area.height);
    let right = Rect::new(main.right(), area.y, right_width, area.height);
    (main, Some(right))
}

fn draw_right_slot(f: &mut Frame, app: &App, area: Rect) {
    let Some(snapshot) = app.slot_snapshots.get("chrome.right") else {
        return;
    };
    let theme = app.theme;
    let inner_width = area.width.saturating_sub(4) as usize;
    let lines = crate::slots::render(snapshot, &theme, inner_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " chrome.right ",
            Style::default().fg(theme.caption),
        ))
        .style(Style::default().bg(theme.surface).fg(theme.fg));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_child_navigation(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let label = app
        .active_subagent
        .as_deref()
        .and_then(|id| app.subagents.iter().find(|view| view.id == id))
        .map(|view| view.label.as_str())
        .unwrap_or("subagent");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {label} · read-only"),
                Style::default().fg(theme.fg_secondary),
            ),
            Span::styled(
                "   esc back · ↓ switch agents",
                Style::default().fg(theme.caption),
            ),
        ]))
        .style(Style::default().bg(theme.panel)),
        area,
    );
}

fn draw_agent_rail(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let mut spans = vec![Span::styled(
        " agents  ",
        Style::default().fg(theme.caption),
    )];
    let main_active = app.active_subagent.is_none();
    spans.push(Span::styled(
        if main_active { "▸ main" } else { "  main" },
        Style::default()
            .fg(if main_active {
                theme.brand
            } else {
                theme.fg_secondary
            })
            .add_modifier(if main_active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
    ));
    for view in &app.subagents {
        let active = app.active_subagent.as_deref() == Some(view.id.as_str());
        let marker = if view.running { '●' } else { '✓' };
        spans.push(Span::styled(
            format!(
                "  {}{marker} {}",
                if active { "▸ " } else { "" },
                view.label
            ),
            Style::default()
                .fg(if view.running { theme.brand } else { theme.ok })
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    }
    spans.push(Span::styled(
        "  ↓ switch",
        Style::default().fg(theme.caption),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.surface)),
        area,
    );
}

/// Fallback pet for terminals without a pixel protocol: the XS half-block
/// whale, brand gradient, anchored bottom-right like the real image.
fn draw_pet_chars(f: &mut Frame, theme: Theme, cells: Rect) {
    let w = WHALE_XS
        .iter()
        .map(|r| r.chars().count())
        .max()
        .unwrap_or(0) as u16;
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
fn draw_composer(f: &mut Frame, app: &mut App, area: Rect, pet_pad: u16) {
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
    if inner.width == 0 || inner.height < 2 {
        return;
    }

    // Draft first: the input well fills everything above the meta row.
    // Inline [image n] chips live in the draft text; draw_input restyles
    // them and records their rects for hover/preview hit-tests.
    app.att_chips.clear();
    app.att_thumbs.clear();
    let well = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
    app.composer_wrap_width = inner.width.saturating_sub("❯ ".width() as u16).max(1) as usize;
    draw_input(f, app, well);

    // Bottom meta row: run state + mode/permission chips left, model
    // right, collision-aware.
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
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

/// The active run-state line — rendered as the transcript's always-last line
/// while work is in progress. Quiet sessions need no redundant `● idle` row.
fn state_line(app: &App) -> Option<Line<'static>> {
    let theme = app.theme;
    let mut spans: Vec<Span> = Vec::new();
    let active_child = app
        .active_subagent
        .as_deref()
        .and_then(|id| app.subagents.iter().find(|view| view.id == id));
    let transcript = app.displayed_transcript();
    let state = active_child
        .map(|view| {
            if view.running {
                RunState::Running
            } else {
                RunState::Idle
            }
        })
        .unwrap_or(app.state);
    // Open reasoning/assistant cells already own the live presentation
    // (`thinking…` or the streaming cursor). Adding another `streaming` row
    // made the same phase alternate between one and two status rows.
    if state == RunState::Running && transcript.streaming() {
        return None;
    }
    match state {
        RunState::Idle => return None,
        RunState::Starting | RunState::Running => {
            spans.push(Span::styled(
                format!("{} ", app.spinner()),
                Style::default().fg(theme.brand),
            ));
            let label = if state == RunState::Starting {
                if app.state_note.is_empty() {
                    app.locale.tr("starting", "启动中").to_string()
                } else {
                    app.state_note.clone()
                }
            } else if !app.state_note.is_empty() {
                app.state_note.clone()
            } else {
                app.locale.tr("working", "工作中").to_string()
            };
            spans.push(Span::styled(label, Style::default().fg(theme.brand_soft)));
            if active_child.is_none() {
                if let Some(t0) = app.run_started {
                    spans.push(Span::styled(
                        format!(" {}s", t0.elapsed().as_secs()),
                        Style::default().fg(theme.caption),
                    ));
                }
            }
            if active_child.is_none() && app.queued > 0 {
                spans.push(Span::styled(
                    format!(" · {} {}", app.queued, app.locale.tr("queued", "条排队中")),
                    Style::default().fg(theme.warn_soft()),
                ));
            }
        }
    }
    Some(Line::from(spans))
}

/// Meta row, left side: the session's mode chips only (run state lives at
/// the transcript tail).
fn status_title(app: &App) -> Line<'static> {
    if !app.session_bound {
        return Line::default();
    }
    let theme = app.theme;
    let mut spans: Vec<Span> = Vec::new();
    // Mode chips: folded from the durable event stream (same facts as the
    // Web UI chips). Stock defaults render until the host reports its own
    // facts, so the landing screen still advertises preset + permission.
    let preset = app.modes.agent_preset.as_deref().unwrap_or("standard");
    spans.push(Span::styled(
        format!("⚙ {} ", app.agent_label(preset)),
        Style::default().fg(theme.fg_tertiary),
    ));
    let perm = app
        .modes
        .permission
        .clone()
        .or_else(|| app.modes.sandbox.clone())
        .unwrap_or_else(|| app.current_permission().to_string());
    let label = if app.locale == crate::locale::Locale::Zh {
        match perm.as_str() {
            "read-only" => "只读".to_string(),
            "workspace-write" => "工作区可写".to_string(),
            "danger-full-access" => "完全访问".to_string(),
            _ => crate::app::permission_label(&perm),
        }
    } else {
        crate::app::permission_label(&perm)
    };
    spans.push(Span::styled(
        format!("⛨ {label}"),
        Style::default().fg(if perm == "danger-full-access" {
            theme.warn_soft()
        } else {
            theme.fg_tertiary
        }),
    ));
    // ⇧⇥ cycles the permission preset — keep the affordance visible
    // now that the shortcut-hints row is gone.
    spans.push(Span::styled(
        "·⇧⇥ ".to_string(),
        Style::default().fg(theme.caption),
    ));
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

/// Contextual shortcut hints — a tiny state machine over (run state ×
/// draft): what Enter does *right now*, how to interrupt, how to steer
/// immediately. Idle+empty falls back to the `^. keys` discovery hint.
fn context_hints(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let key = Style::default()
        .fg(theme.fg_tertiary)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::default().fg(theme.caption);
    let running = !matches!(app.state, RunState::Idle);
    let pairs: Vec<(&str, &str)> = match (running, app.input.is_empty()) {
        // Working, nothing typed: the only move is stopping it.
        (true, true) => vec![("esc", app.locale.tr("interrupt", "中断"))],
        // Working with a draft: enter queues; ^x steers without cancellation.
        (true, false) => vec![
            ("⏎", app.locale.tr("queue", "排队")),
            ("^x", "steer"),
            ("esc", app.locale.tr("interrupt", "中断")),
        ],
        // Idle, empty: point at the full shortcut list.
        (false, true) => vec![("^.", app.locale.tr("keys", "快捷键"))],
        // Idle with a draft: enter's meaning follows the prefix.
        (false, false) if app.input.buf.starts_with('/') => {
            vec![("⏎", app.locale.tr("command", "命令"))]
        }
        (false, false) if app.input.buf.starts_with('!') => vec![("⏎", "shell")],
        (false, false) => vec![("⏎", app.locale.tr("send", "发送"))],
    };
    let mut spans = Vec::new();
    for (i, (k, l)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " · ".to_string(),
                Style::default().fg(theme.border),
            ));
        }
        spans.push(Span::styled(k.to_string(), key));
        spans.push(Span::styled(format!(" {l}"), lbl));
    }
    spans.push(Span::styled(
        " · ".to_string(),
        Style::default().fg(theme.border),
    ));
    spans
}

/// Meta row, right side: contextual shortcut hints, model chip (brand
/// accent), and the requested reasoning effort. Token flow lives in the
/// usage footer; the session id lives in the banner and `/session`.
fn status_right(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    if app.scroll_up > 0 {
        spans.push(Span::styled(
            format!("▲{} · ", app.scroll_up),
            Style::default().fg(theme.caption),
        ));
    }
    spans.extend(context_hints(app));
    if app.session_bound {
        // Chip precedence: an explicit /model pick (until a turn realizes it)
        // → the model that actually streamed last → the configured default.
        let shown_model = app
            .selected_model
            .clone()
            .or_else(|| app.transcript.last_model.clone())
            .unwrap_or_else(|| app.cfg.model.clone());
        spans.push(Span::styled(
            shown_model,
            Style::default().fg(theme.brand_soft),
        ));
        if let Some(effort) = &app.modes.effort {
            spans.push(Span::styled(
                format!(" · {effort}"),
                Style::default().fg(theme.caption),
            ));
        }
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

/// Additive Client-plugin dock below the native composer. Contributions are
/// compact sections in registration order; low-priority tail sections simply
/// do not fit on narrow terminals, matching the Web composer's stats dock.
fn draw_composer_dock(f: &mut Frame, app: &App, area: Rect, pet_pad: u16) {
    let theme = app.theme;
    f.render_widget(
        Block::default().style(Style::default().bg(app.canvas_background_color())),
        area,
    );
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2 + pet_pad),
        area.height,
    );
    if inner.width < 12 {
        return;
    }
    let Some(snapshot) = app.slot_snapshots.get("conversation.composer.dock") else {
        return;
    };
    f.render_widget(
        Paragraph::new(compact_slot_line(snapshot, &theme, inner.width as usize)),
        inner,
    );
}

fn span_widths(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

fn slot_has_nodes(app: &App, name: &str) -> bool {
    app.slot_snapshots
        .get(name)
        .is_some_and(|snapshot| !snapshot.nodes.is_empty())
}

fn compact_slot_line(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let sections = compact_slot_sections(snapshot, theme, width);
    compact_slot_sections_line(&sections, theme)
}

struct CompactSlotSection {
    spans: Vec<Span<'static>>,
    action: Option<crate::slots::TuiAction>,
}

fn compact_slot_sections(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
) -> Vec<CompactSlotSection> {
    let mut sections = snapshot
        .nodes
        .iter()
        .filter_map(|node| {
            compact_node_spans(node, theme, width).map(|spans| CompactSlotSection {
                spans,
                action: node.action().cloned(),
            })
        })
        .collect::<Vec<_>>();
    while sections.len() > 1 {
        let total = sections
            .iter()
            .map(|section| span_widths(&section.spans))
            .sum::<usize>()
            + (sections.len() - 1) * 3;
        if total <= width.saturating_sub(2) {
            break;
        }
        sections.pop();
    }
    sections
}

fn compact_slot_sections_line(sections: &[CompactSlotSection], theme: &Theme) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(theme.border)));
        }
        spans.extend(section.spans.iter().cloned());
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

fn compact_node_spans(
    node: &crate::slots::TuiNode,
    theme: &Theme,
    width: usize,
) -> Option<Vec<Span<'static>>> {
    if let crate::slots::TuiNode::Generic { title, status, .. } = node {
        let mut spans = Vec::new();
        if let Some(status) = status.as_deref() {
            let (icon, color) = match status {
                "running" => ("●", theme.brand),
                "ok" => ("✓", theme.ok),
                "err" => ("×", theme.err),
                _ => ("◇", theme.caption),
            };
            spans.push(Span::styled(format!("{icon} "), Style::default().fg(color)));
        }
        spans.push(Span::styled(
            title.clone(),
            Style::default().fg(theme.fg_secondary),
        ));
        return Some(spans);
    }
    crate::slots::render_nodes(std::slice::from_ref(node), theme, width)
        .into_iter()
        .next()
        .map(|line| line.spans)
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let mut owners: Vec<Option<usize>> = Vec::new();
    let banner_count;
    let mut lines = if app.show_banner {
        let banner = banner_lines(app, inner.width);
        banner_count = banner.len();
        owners.resize(banner_count, None);
        banner
    } else {
        banner_count = 0;
        Vec::new()
    };
    let thumbs = crate::pet::kitty_supported();
    let layout = app
        .displayed_transcript()
        .layout(&theme, inner.width, app.spinner(), thumbs);
    lines.extend(layout.lines);
    owners.extend(layout.owners);
    // Active work rides as the transcript's last line — hugging the newest
    // message (no separator; it scrolls with the list). Idle draws nothing.
    if !app.show_banner {
        if let Some(line) = state_line(app) {
            lines.push(line);
            owners.push(None);
        }
    }

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
    app.chat_view.owners = owners;

    // Visible image thumbnails → screen rects (banner lines shift transcript
    // line indices; partially visible thumbnails clip to the pane).
    app.chat_view.images = layout
        .images
        .iter()
        .filter_map(|shot| {
            let abs = banner_count + shot.line;
            let rel = abs as isize - start as isize;
            if rel < 0 || rel as usize >= h {
                return None;
            }
            let top = inner.y + rel as u16;
            let rows = (shot.rows as u16).min(inner.height.saturating_sub(rel as u16));
            if rows == 0 {
                return None;
            }
            Some(crate::app::ThumbPlacement {
                id: shot.id,
                rect: ratatui::layout::Rect::new(inner.x, top, shot.cols as u16, rows),
                data: shot.data.clone(),
            })
        })
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

fn tip_line(app: &App) -> Line<'static> {
    let theme = app.theme;
    let transient = app.tip.is_some();
    let text = match &app.tip {
        Some((t, _)) => t.clone(),
        None => app.locale.ambient_tip(app.ambient_tip_idx).to_string(),
    };

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    if transient {
        // Action feedback reads brighter than the rotating hints.
        spans.push(Span::styled(text, Style::default().fg(theme.fg)));
    } else {
        spans.push(Span::styled(
            app.locale.tr("Tip", "提示").to_string(),
            Style::default()
                .fg(theme.brand_soft)
                .add_modifier(Modifier::BOLD),
        ));
        // Rotating hints read a tier below the chat body text above — the
        // banner is furniture, not content. Gray-blue keeps it on-brand.
        spans.push(Span::styled(
            format!(" · {text}"),
            Style::default().fg(theme.hint),
        ));
    }
    spans.push(Span::raw(" "));

    Line::from(spans)
}

fn compact_workspace(path: &str, max_width: usize) -> String {
    let path = shorten_home(path);
    if path.width() <= max_width {
        return path;
    }
    let leaf = path
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(path.as_str());
    let leaf_path = format!("…/{leaf}");
    if leaf_path.width() <= max_width {
        return leaf_path;
    }
    if max_width <= 1 {
        return "…".into();
    }
    let mut suffix = String::new();
    let mut used = 0;
    for ch in leaf.chars().rev() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width > max_width - 1 {
            break;
        }
        suffix.insert(0, ch);
        used += width;
    }
    format!("…{suffix}")
}

fn workspace_cap_title(app: &App, area_width: usize) -> Line<'static> {
    let title_width = (area_width / 2).clamp(8, 64);
    let path_width = title_width.saturating_sub(4);
    Line::from(Span::styled(
        format!(" ⌂ {} ", compact_workspace(&app.cfg.workspace, path_width)),
        Style::default().fg(app.theme.caption),
    ))
    .right_aligned()
}

fn ellipsize_line(line: Line<'static>, max_width: usize, style: Style) -> Line<'static> {
    if span_widths(&line.spans) <= max_width {
        return line;
    }
    if max_width == 0 {
        return Line::default();
    }
    let mut spans = Vec::new();
    let mut remaining = max_width - 1;
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let mut text = String::new();
        for ch in span.content.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width > remaining {
                break;
            }
            text.push(ch);
            remaining -= width;
        }
        if !text.is_empty() {
            spans.push(Span::styled(text, span.style));
        }
    }
    spans.push(Span::styled("…", style));
    Line::from(spans)
}

fn draw_composer_cap(f: &mut Frame, app: &mut App, area: Rect, has_input_dock: bool) {
    let theme = app.theme;
    let workspace = workspace_cap_title(app, area.width as usize);
    let workspace_width = span_widths(&workspace.spans);
    let title_budget = (area.width as usize).saturating_sub(2 + workspace_width + 1);
    let dock_sections = has_input_dock
        .then(|| app.slot_snapshots.get("conversation.input.dock"))
        .flatten()
        .map(|snapshot| compact_slot_sections(snapshot, &theme, title_budget));
    let dock_line = dock_sections
        .as_ref()
        .map(|sections| compact_slot_sections_line(sections, &theme));
    let title = ellipsize_line(
        dock_line.clone().unwrap_or_else(|| tip_line(app)),
        title_budget,
        Style::default().fg(theme.caption),
    );
    let title_width = span_widths(&title.spans) as u16;
    let block = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(title)
        .title(workspace)
        .style(Style::default().bg(app.canvas_background_color()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(sections) = dock_sections {
        let mut x = area.x.saturating_add(2);
        let visible_right = area
            .x
            .saturating_add(1)
            .saturating_add(title_width.min(area.width.saturating_sub(2)));
        for (index, section) in sections.into_iter().enumerate() {
            if index > 0 {
                x = x.saturating_add(3);
            }
            let section_width = span_widths(&section.spans) as u16;
            if let Some(action) = section.action {
                let action_width = visible_right.saturating_sub(x).min(section_width);
                if action_width > 0 {
                    app.slot_actions
                        .push((Rect::new(x, area.y, action_width, 1), action));
                }
            }
            x = x.saturating_add(section_width);
        }
    }
    if dock_line.is_some() && inner.height > 0 {
        f.render_widget(Paragraph::new(tip_line(app)), inner);
    }
}

/// grok-style hover preview: when the pointer rests on an inline chip (or
/// the text cursor sits in one), pop a card above the composer with a
/// kitty thumbnail (PNG + pixel terminals) and basic metadata.
fn draw_attachment_preview(f: &mut Frame, app: &mut App, composer: Rect, screen: Rect) {
    let Some(idx) = app.preview_att() else {
        return;
    };
    let Some(att) = app.pending_images.get(idx) else {
        return;
    };
    let theme = app.theme;
    let dims = crate::pet::image_dims(&att.data);
    let pixels = app.pet_pixels && att.is_png();
    let thumb_h: u16 = if pixels { 5 } else { 0 };
    let w: u16 = 36.min(screen.width.saturating_sub(2));
    let h: u16 = thumb_h + 3 + 2; // meta lines + borders
    if screen.height <= h || w < 12 {
        return;
    }
    // Anchor near the chip, clamped on-screen, sitting above the composer.
    let anchor_x = app
        .att_chips
        .iter()
        .find(|(_, i)| *i == idx)
        .map(|(r, _)| r.x)
        .unwrap_or(composer.x + 2);
    let x = anchor_x.min(screen.x + screen.width - w);
    let y = composer.y.saturating_sub(h);
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();
    for _ in 0..thumb_h {
        lines.push(Line::default()); // reserved rows the kitty image covers
    }
    lines.push(Line::from(Span::styled(
        att.name.clone(),
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
    )));
    let dims_txt = dims
        .map(|(iw, ih)| format!("{iw}×{ih} px"))
        .unwrap_or_else(|| "unknown size".into());
    let kb = att.data.len() as f64 / 1024.0;
    let size_txt = if kb >= 1024.0 {
        format!("{:.1} MB", kb / 1024.0)
    } else {
        format!("{kb:.0} KB")
    };
    lines.push(Line::from(Span::styled(
        format!("{dims_txt} · {size_txt} · {}", att.media_type),
        Style::default().fg(theme.caption),
    )));
    lines.push(Line::from(Span::styled(
        "⌫ on the chip removes · enter sends".to_string(),
        Style::default().fg(theme.caption),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" {} ", att.token),
            Style::default().fg(theme.brand_soft),
        ))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
    if pixels && thumb_h > 0 {
        // Aspect-fit the thumbnail into the reserved rows (cells ≈ 1:2).
        let cols = dims
            .filter(|&(_, ih)| ih > 0)
            .map(|(iw, ih)| ((thumb_h as u64 * 2 * iw as u64 + ih as u64 / 2) / ih as u64) as u16)
            .unwrap_or(10)
            .clamp(4, inner.width.max(4));
        app.att_thumbs.push(crate::app::ThumbPlacement {
            id: att.id,
            rect: Rect::new(inner.x, inner.y, cols.min(inner.width), thumb_h),
            data: att.data.clone(),
        });
    }
}

/// The input well. Long prompts wrap across the (now taller) well, and the
/// cursor follows the wrap; `/` and `!` prefixes recolor the whole line.
/// Inline `[image n]` tokens render as chips (no icon — the token itself is
/// the chip) and their screen rects land in `app.att_chips` for hover.
fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    if area.width < 4 || area.height == 0 {
        return;
    }
    let prompt = "❯ ";
    let pw = prompt.width();
    let prompt_span = Span::styled(
        prompt.to_string(),
        Style::default()
            .fg(theme.brand)
            .add_modifier(Modifier::BOLD),
    );

    if app.input.is_empty() {
        let placeholder = match app.state {
            RunState::Idle => app
                .locale
                .tr("describe what you want to build…", "描述你想构建的内容…")
                .to_string(),
            _ => app
                .locale
                .tr(
                    "queue a follow-up — ctrl+x steers now",
                    "输入后续消息 — ctrl+x 立即 steer",
                )
                .to_string(),
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
    // Inline chips: which char indices belong to which [image n] token.
    let spans = app.token_spans();
    let chip_of = |i: usize| -> Option<usize> {
        spans
            .iter()
            .find(|(s, e, _)| i >= *s && i < *e)
            .map(|&(_, _, idx)| idx)
    };
    let chip_style = Style::default()
        .fg(theme.bubble_fg)
        .bg(theme.bubble_bg)
        .add_modifier(Modifier::BOLD);

    // Manual wrap: hard-break on `\n`, soft-wrap on display width, and track
    // the cursor's (row, col) across both. Rows keep char indices so chips
    // can be restyled and hit-tested after wrapping.
    let avail = (area.width as usize).saturating_sub(pw).max(1);
    let mut rows: Vec<Vec<(usize, char)>> = vec![Vec::new()];
    let mut col = 0usize;
    for (i, ch) in app.input.buf.chars().enumerate() {
        if ch == '\n' {
            rows.push(Vec::new());
            col = 0;
            continue;
        }
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if col + cw > avail {
            rows.push(Vec::new());
            col = 0;
        }
        rows.last_mut().expect("input row").push((i, ch));
        col += cw;
    }
    let cursor = app.input.visual_cursor(avail);
    while rows.len() <= cursor.0 {
        rows.push(Vec::new());
    }

    // Keep the cursor row inside the well; otherwise show the tail.
    let h = area.height as usize;
    let mut start = rows.len().saturating_sub(h);
    if cursor.0 < start {
        start = cursor.0;
    }

    let mut chip_rects: Vec<(Rect, usize)> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for (r, row) in rows.iter().enumerate().skip(start).take(h) {
        let lead = if r == 0 {
            prompt_span.clone()
        } else {
            Span::raw(" ".repeat(pw))
        };
        let mut segs: Vec<Span> = vec![lead];
        let y = area.y + (r - start) as u16;
        let mut seg = String::new();
        let mut seg_chip: Option<usize> = None;
        let mut seg_col = 0usize; // display col where the current segment starts
        let mut colw = 0usize;
        let flush = |segs: &mut Vec<Span>,
                     chip_rects: &mut Vec<(Rect, usize)>,
                     seg: &mut String,
                     seg_chip: Option<usize>,
                     seg_col: usize,
                     colw: usize| {
            if seg.is_empty() {
                return;
            }
            match seg_chip {
                Some(idx) => {
                    segs.push(Span::styled(std::mem::take(seg), chip_style));
                    chip_rects.push((
                        Rect::new(
                            area.x + (pw + seg_col) as u16,
                            y,
                            (colw - seg_col) as u16,
                            1,
                        ),
                        idx,
                    ));
                }
                None => segs.push(Span::styled(std::mem::take(seg), style)),
            }
        };
        for &(i, ch) in row {
            let chip = chip_of(i);
            if chip != seg_chip {
                flush(
                    &mut segs,
                    &mut chip_rects,
                    &mut seg,
                    seg_chip,
                    seg_col,
                    colw,
                );
                seg_chip = chip;
                seg_col = colw;
            }
            seg.push(ch);
            colw += UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        }
        flush(
            &mut segs,
            &mut chip_rects,
            &mut seg,
            seg_chip,
            seg_col,
            colw,
        );
        lines.push(Line::from(segs));
    }
    f.render_widget(Paragraph::new(lines), area);
    app.att_chips = chip_rects;

    let cy = area.y + (cursor.0 - start).min(h - 1) as u16;
    let cx = area.x + pw as u16 + cursor.1 as u16;
    f.set_cursor_position((cx.min(area.x + area.width.saturating_sub(1)), cy));
}

/// Rows of menu items shown at once; the window follows the selection
/// (↑/↓ wrap) and the bottom title shows the position when clipped.
const SLASH_MENU_ROWS: usize = 12;

fn draw_slash_menu(f: &mut Frame, app: &App, input: Rect, chat: Rect) {
    let matches = app.slash_matches();
    if matches.is_empty() {
        return;
    }
    let theme = app.theme;
    let n = matches.len();
    let sel = app.slash_sel.min(n - 1);
    // Cap the popup: tall skill catalogs scroll instead of swallowing the
    // chat pane.
    let vis = SLASH_MENU_ROWS
        .min(n)
        .min(chat.height.saturating_sub(2) as usize);
    if vis == 0 {
        return;
    }
    // Follow-window: selection stays visible, window pinned to the ends.
    let start = if sel < vis { 0 } else { sel + 1 - vis }.min(n - vis);

    // Name column sized to what's listed (padded, never glued to the desc).
    let name_w = matches
        .iter()
        .map(|m| m.usage.width())
        .max()
        .unwrap_or(14)
        .clamp(14, 26);

    let h = vis as u16 + 2;
    let w = 64.min(input.width.saturating_sub(2));
    let y = input.y.saturating_sub(h);
    let area = Rect::new(input.x + 2, y, w, h);
    f.render_widget(Clear, area);
    let mut lines = Vec::new();
    for (i, cmd) in matches.iter().enumerate().skip(start).take(vis) {
        let selected = i == sel;
        let marker = if selected { "▸ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme.brand)
                .add_modifier(Modifier::BOLD)
        } else if cmd.skill {
            // Host skills read one shade apart from the builtins — the
            // gray-blue hint tone, not a loud accent.
            Style::default().fg(theme.hint)
        } else {
            Style::default().fg(theme.fg_secondary)
        };
        let desc = if cmd.skill {
            format!("{} · {}", cmd.desc, app.locale.tr("skill", "技能"))
        } else {
            cmd.desc.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(pad_or_ellipsize(&cmd.usage, name_w), name_style),
            Span::styled(format!(" {desc}"), Style::default().fg(theme.caption)),
        ]));
    }
    let title = if matches.iter().any(|m| m.skill) {
        app.locale.tr(" commands · skills ", " 命令 · 技能 ")
    } else {
        app.locale.tr(" commands ", " 命令 ")
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(title, Style::default().fg(theme.caption)))
        .style(Style::default().bg(theme.panel));
    if n > vis {
        // Clipped: show position + scroll affordance in the bottom border.
        let above = start > 0;
        let below = start + vis < n;
        let arrows = match (above, below) {
            (true, true) => "↑↓",
            (true, false) => "↑",
            _ => "↓",
        };
        block = block.title_bottom(Span::styled(
            format!(" {}/{n} {arrows} ", sel + 1),
            Style::default().fg(theme.caption),
        ));
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Pad `s` to exactly `w` display cells, ellipsizing when longer — keeps
/// the desc column aligned even for long skill names.
fn pad_or_ellipsize(s: &str, w: usize) -> String {
    let sw = s.width();
    if sw <= w {
        return format!("{s}{}", " ".repeat(w - sw));
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > w.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    let ow = out.width();
    format!("{out}{}", " ".repeat(w.saturating_sub(ow)))
}

fn draw_model_picker(f: &mut Frame, app: &App, screen: Rect) {
    let Some(picker) = &app.picker else {
        return;
    };
    let theme = app.theme;
    // The active model is identified by provider + id because multiple
    // coding plans can expose the same upstream model id.
    let current_mode = app.current_mode();
    let is_current = |item: &crate::app::PickerItem| match picker.kind {
        crate::app::PickerKind::Model => {
            item.id == app.cfg.model
                && item
                    .provider
                    .as_deref()
                    .is_none_or(|provider| provider == app.cfg.provider)
        }
        crate::app::PickerKind::Mode => item.id == current_mode,
        crate::app::PickerKind::Theme => item.id == app.active_palette_id,
        crate::app::PickerKind::Permission => item.id == app.current_permission(),
        crate::app::PickerKind::Effort
        | crate::app::PickerKind::Session
        | crate::app::PickerKind::Subagent
        | crate::app::PickerKind::Auth
        | crate::app::PickerKind::Plugin => false,
    };
    let h = (picker.items.len() as u16 + 2).min(screen.height.saturating_sub(2));
    // Fit the widest row (marker + padded label + ✓ + meta); cap to the screen.
    let needed = picker
        .items
        .iter()
        .map(|item| {
            let label_w = item.label.width() + if is_current(item) { 2 } else { 0 };
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
            Style::default()
                .fg(theme.brand)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg_secondary)
        };
        // The current model/mode gets a ✓ pinned to its label — it survives
        // narrow terminals, unlike a right-edge tag.
        let label = if is_current(item) {
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

fn draw_permission_ask(f: &mut Frame, app: &App, screen: Rect) {
    let Some(ask) = &app.permission_ask else {
        return;
    };
    let theme = app.theme;
    let h = (ask.options.len() as u16 + 2).min(screen.height.saturating_sub(2));
    let needed = ask
        .options
        .iter()
        .map(|opt| 2 + opt.name.width().max(24) + 1 + opt.kind.width())
        .max()
        .unwrap_or(0) as u16;
    let cap = screen.width.saturating_sub(4).max(24);
    let w = (needed + 2).max(58).min(cap);
    let x = screen.x + (screen.width - w) / 2;
    let y = screen.y + (screen.height - h) / 3;
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);
    let mut lines = Vec::new();
    for (i, opt) in ask.options.iter().enumerate() {
        let selected = i == ask.sel;
        let marker = if selected { "▸ " } else { "  " };
        let reject = opt.kind.starts_with("reject");
        let style = if selected {
            Style::default()
                .fg(if reject { theme.err } else { theme.brand })
                .add_modifier(Modifier::BOLD)
        } else if reject {
            Style::default().fg(theme.err)
        } else {
            Style::default().fg(theme.fg_secondary)
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(format!("{:<24}", opt.name), style),
            Span::styled(opt.kind.clone(), Style::default().fg(theme.caption)),
        ]));
    }
    let title = format!(" approval · {} · enter select · esc cancel ", ask.title);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.warn))
        .title(Span::styled(title, Style::default().fg(theme.caption)))
        .style(Style::default().bg(theme.panel));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_elicitation_form(f: &mut Frame, app: &App, screen: Rect) {
    let Some(ask) = &app.elicitation_ask else {
        return;
    };
    let Some(state) = ask.form.fields.get(ask.form.index) else {
        return;
    };
    let theme = app.theme;
    let content_width = screen.width.saturating_sub(10).clamp(28, 74) as usize;
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        ask.form.message.clone(),
        Style::default().fg(theme.caption),
    )));
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        state.field.title.clone(),
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
    )));
    if let Some(description) = &state.field.description {
        for paragraph in description.lines() {
            lines.push(Line::from(Span::styled(
                pad_or_ellipsize(paragraph, content_width),
                Style::default().fg(theme.fg_secondary),
            )));
        }
    }
    lines.push(Line::default());

    match &state.field.kind {
        crate::elicitation::ElicitationFieldKind::Single { options, .. }
        | crate::elicitation::ElicitationFieldKind::Multi { options, .. } => {
            let multi = matches!(
                state.field.kind,
                crate::elicitation::ElicitationFieldKind::Multi { .. }
            );
            for (index, option) in options.iter().enumerate() {
                let focused = index == state.cursor;
                let mark = if multi {
                    if state.selected.get(index).copied().unwrap_or(false) {
                        "[x]"
                    } else {
                        "[ ]"
                    }
                } else if focused {
                    "(●)"
                } else {
                    "( )"
                };
                let mut spans = vec![
                    Span::styled(
                        if focused { "▸ " } else { "  " }.to_string(),
                        Style::default().fg(theme.brand),
                    ),
                    Span::styled(
                        format!("{mark} {}", option.label),
                        Style::default()
                            .fg(if focused {
                                theme.brand
                            } else {
                                theme.fg_secondary
                            })
                            .add_modifier(if focused {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    ),
                ];
                if let Some(description) = &option.description {
                    spans.push(Span::styled(
                        format!(" · {description}"),
                        Style::default().fg(theme.caption),
                    ));
                }
                lines.push(Line::from(spans));
                if option.custom && state.editing_custom {
                    lines.push(Line::from(vec![
                        Span::styled("      ❯ ", Style::default().fg(theme.brand)),
                        Span::styled(
                            format!("{}▏", state.input.buf),
                            Style::default().fg(theme.fg),
                        ),
                    ]));
                }
            }
        }
        crate::elicitation::ElicitationFieldKind::Boolean { .. } => {
            for (index, label) in ["Yes", "No"].iter().enumerate() {
                let focused = index == state.cursor;
                let chosen = state.selected.get(index).copied().unwrap_or(false);
                lines.push(Line::from(vec![
                    Span::styled(
                        if focused { "▸ " } else { "  " }.to_string(),
                        Style::default().fg(theme.brand),
                    ),
                    Span::styled(
                        format!("{} {label}", if chosen { "(●)" } else { "( )" }),
                        Style::default().fg(if focused {
                            theme.brand
                        } else {
                            theme.fg_secondary
                        }),
                    ),
                ]));
            }
        }
        _ => {
            lines.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(theme.brand)),
                Span::styled(
                    format!("{}▏", state.input.buf),
                    Style::default().fg(theme.fg),
                ),
            ]));
        }
    }
    if let Some(error) = &ask.form.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme.err),
        )));
    }

    let cap_w = screen.width.saturating_sub(4).max(24);
    let w = (content_width as u16 + 4).min(cap_w);
    let h = (lines.len() as u16 + 2).min(screen.height.saturating_sub(2));
    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 3;
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);
    let count = ask.form.fields.len();
    let title = format!(
        " ask · {}/{} · enter next/submit · tab skip · esc cancel ",
        ask.form.index + 1,
        count,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(title, Style::default().fg(theme.caption)))
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
            app.locale.tr("Into the Unknown", "探索未知").to_string(),
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
    out.push(kv(
        app.locale.tr("version", "版本"),
        format!("dsh-tui {}", env!("CARGO_PKG_VERSION")),
    ));
    out.push(kv(
        app.locale.tr("model", "模型"),
        format!("{} · {}", app.cfg.provider, app.cfg.model),
    ));
    out.push(kv(
        app.locale.tr("workspace", "工作区"),
        shorten_home(&app.cfg.workspace),
    ));
    out.push(kv(app.locale.tr("session", "会话"), app.session_id.clone()));
    let runtime_short = app
        .cfg
        .bin
        .rsplit('/')
        .next()
        .unwrap_or(&app.cfg.bin)
        .to_string();
    out.push(kv(app.locale.tr("runtime", "运行时"), runtime_short));
    if app.demo {
        out.push(kv(
            app.locale.tr("mode", "模式"),
            app.locale
                .tr(
                    "demo — scripted turns, no API calls",
                    "演示 — 脚本轮次，不调用 API",
                )
                .into(),
        ));
    } else if app.auth.status == crate::acp_auth::AuthStatus::NeedsAuth {
        let detail = app
            .auth
            .method_name
            .as_deref()
            .or(app.auth.method_id.as_deref())
            .unwrap_or("agent");
        out.push(Line::from(vec![
            Span::styled("  ⚠ ".to_string(), Style::default().fg(theme.warn)),
            Span::styled(
                if app.locale == crate::locale::Locale::Zh {
                    format!("需要登录 · {detail} — 使用 /auth（Agent 的 /login 仍作为消息发送）")
                } else {
                    format!("sign-in needed · {detail} — /auth (agent /login stays a prompt)")
                },
                Style::default().fg(theme.warn_soft()),
            ),
        ]));
    } else if app.auth.status == crate::acp_auth::AuthStatus::Configured {
        let detail = app
            .auth
            .method_name
            .as_deref()
            .or(app.auth.method_id.as_deref())
            .unwrap_or("ACP");
        out.push(kv(
            app.locale.tr("credentials", "凭据"),
            format!("ACP authenticate · {detail}"),
        ));
    } else if app.attached {
        out.push(kv(
            app.locale.tr("credentials", "凭据"),
            app.locale
                .tr("host dsh (credential seam)", "Host dsh（凭据边界）")
                .into(),
        ));
    } else if let Some(source) = app.cfg.credential_source() {
        out.push(kv(app.locale.tr("credentials", "凭据"), source.into()));
    } else {
        out.push(Line::from(vec![
            Span::styled("  ⚠ ".to_string(), Style::default().fg(theme.warn)),
            Span::styled(
                app.locale
                    .tr(
                        "DEEPSEEK_API_KEY not set — export it, or relaunch with --demo",
                        "未设置 DEEPSEEK_API_KEY — 请导出变量，或使用 --demo 重启",
                    )
                    .to_string(),
                Style::default().fg(theme.warn_soft()),
            ),
        ]));
    }
    out.push(Line::from(Span::styled(
        app.locale
            .tr(
                "  full workspace access — prefer a disposable checkout",
                "  可访问完整工作区 — 建议使用可丢弃的 checkout",
            )
            .to_string(),
        Style::default().fg(theme.caption),
    )));
    out.push(Line::default());
    out.push(Line::from(Span::styled(
        app.locale
            .tr(
                "  /help commands · /keys shortcuts · ! shell · esc interrupt",
                "  /help 命令 · /keys 快捷键 · ! shell · esc 中断",
            )
            .to_string(),
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

    /// Unique session root per call — keeps the modes cache from leaking
    /// between tests and runs.
    fn fresh_root() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dsh-tui-ui-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    fn test_app() -> App {
        let cfg = RuntimeConfig {
            bin: "dsh-runtime".into(),
            cordis: "cordis".into(),
            workspace: "/tmp".into(),
            session_root: fresh_root(),
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (tx, _rx) = mpsc::channel();
        App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx)
    }

    fn live_test_app() -> App {
        let cfg = RuntimeConfig {
            bin: "dsh-acp".into(),
            cordis: "cordis".into(),
            workspace: "/tmp".into(),
            session_root: fresh_root(),
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (tx, _rx) = mpsc::channel();
        App::new(Theme::dark(), cfg, "pending".into(), false, true, tx)
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
        // With no composer-dock contribution, height 20 gives the tip cap row
        // 15 and composer rows 16..19 (3-row well + bottom meta row). No empty
        // native stats/footer row is reserved.
        assert_eq!(
            buf[(40, 16)].bg,
            theme.panel,
            "input well top on panel surface"
        );
        assert_eq!(buf[(40, 17)].bg, theme.panel, "input well on panel surface");
        assert_eq!(buf[(4, 19)].bg, theme.panel, "meta row on panel surface");
        assert_eq!(
            buf[(4, 15)].bg,
            theme.bg,
            "tip cap uses the base background"
        );
        assert_eq!(buf[(4, 10)].bg, theme.bg, "chat keeps the base background");
    }

    #[test]
    fn composer_cap_persistently_shows_the_workspace() {
        let mut app = test_app();
        app.show_banner = false;
        app.cfg.workspace = "/work/acme/projects/deepseek-harness-tui-plan-view".into();

        let frame = dump_frame(&mut app, 120, 20);
        let cap = frame
            .lines()
            .find(|line| line.contains("Tip"))
            .expect("composer cap");

        assert!(
            cap.contains("⌂ /work/acme/projects/deepseek-harness-tui-plan-view"),
            "{cap}"
        );
    }

    #[test]
    fn composer_cap_preserves_the_workspace_tail_on_narrow_terminals() {
        let mut app = test_app();
        app.show_banner = false;
        app.cfg.workspace = "/work/acme/very-long-directory-name/deepseek-harness".into();

        let frame = dump_frame(&mut app, 60, 20);
        let cap = frame
            .lines()
            .find(|line| line.contains("Tip"))
            .expect("composer cap");

        assert!(cap.contains("⌂ …/deepseek-harness"), "{cap}");
    }

    #[test]
    fn active_image_background_clears_only_the_base_canvas() {
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;
        use ratatui::Terminal;

        let mut app = test_app();
        app.show_banner = false;
        app.pet_pixels = true;
        let mut palette: serde_json::Value =
            serde_json::from_str(include_str!("../docs/fixtures/demo-skin.v0.json")).unwrap();
        palette["background"] = serde_json::json!({
            "source": { "kind": "file", "path": "/opt/liang/stage-00.png" },
            "fit": "cover",
            "opacity": 0.42
        });
        let (ctl, _commands) = crate::controller::test_controller();
        app.handle(
            crate::bus::AppEvent::Rpc {
                method: crate::cordis::THEME_UPDATE.into(),
                params: serde_json::json!({
                    "protocol": 0,
                    "palette": palette,
                    "activate": true
                }),
            },
            &ctl,
        );

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
        let buf = terminal.backend().buffer();

        assert_eq!(
            buf[(4, 10)].bg,
            Color::Reset,
            "chat reveals the image layer"
        );
        assert_eq!(
            buf[(4, 15)].bg,
            Color::Reset,
            "tip cap reveals the image layer"
        );
        assert_eq!(
            buf[(40, 16)].bg,
            app.theme.panel,
            "composer remains readable"
        );
    }

    #[test]
    fn model_chip_prefers_fresh_pick_until_a_turn_realizes_it() {
        let flat = |spans: Vec<Span>| -> String {
            spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        };
        let mut app = test_app();
        // A previous turn streamed on pro; the user just picked flash.
        app.transcript.last_model = Some("deepseek-v4-pro".into());
        app.selected_model = Some("deepseek-v4-flash".into());
        let s = flat(status_right(&app));
        assert!(s.contains("deepseek-v4-flash"), "{s}");
        assert!(!s.contains("deepseek-v4-pro"), "{s}");
        // Without a pick, the streamed model rules.
        app.selected_model = None;
        let s = flat(status_right(&app));
        assert!(s.contains("deepseek-v4-pro"), "{s}");
    }

    #[test]
    fn live_meta_row_hides_session_options_until_session_bound() {
        let flat_line = |line: Line| -> String {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };
        let flat_spans = |spans: Vec<Span>| -> String {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };
        let mut app = live_test_app();
        app.modes.agent_preset = Some("minimal".into());
        app.modes.permission = Some("danger-full-access".into());
        app.modes.effort = Some("high".into());

        assert_eq!(flat_line(status_title(&app)), "");
        let pending = flat_spans(status_right(&app));
        assert!(pending.contains("^. keys"), "{pending}");
        assert!(!pending.contains("deepseek-chat"), "{pending}");
        assert!(!pending.contains("high"), "{pending}");

        let (ctl, _commands) = crate::controller::test_controller();
        app.handle(
            crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::SessionBound {
                session_id: "acp-session".into(),
                notice: None,
            }),
            &ctl,
        );

        let bound_left = flat_line(status_title(&app));
        let bound_right = flat_spans(status_right(&app));
        assert!(bound_left.contains("minimal"), "{bound_left}");
        assert!(bound_left.contains("Full access"), "{bound_left}");
        assert!(bound_right.contains("deepseek-chat"), "{bound_right}");
        assert!(bound_right.contains("high"), "{bound_right}");
    }

    #[test]
    fn cordis_protocol_id_is_rendered_as_creator() {
        let mut app = test_app();
        let (ctl, _commands) = crate::controller::test_controller();
        app.handle(
            crate::bus::AppEvent::Ctl(crate::bus::CtlEvent::Catalog {
                models: Vec::new(),
                presets: vec![crate::bus::CatalogPreset {
                    id: "cordis".into(),
                    name: "Creator from ACP".into(),
                    description: String::new(),
                    broken: false,
                }],
            }),
            &ctl,
        );
        app.modes.agent_preset = Some("cordis".into());

        let rendered = status_title(&app)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("Creator from ACP"), "{rendered}");
        assert!(!rendered.contains("cordis"), "{rendered}");
    }

    #[test]
    fn meta_row_hints_follow_the_state_machine() {
        let flat = |spans: Vec<Span>| -> String {
            spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        };
        let mut app = test_app();
        // idle · empty → discovery hint
        assert!(flat(context_hints(&app)).contains("^. keys"));
        // idle · draft → enter sends
        app.input.set("hello".into());
        let s = flat(context_hints(&app));
        assert!(s.contains("⏎ send"), "{s}");
        assert!(!s.contains("keys"), "{s}");
        // idle · slash / shell drafts relabel enter
        app.input.set("/mo".into());
        assert!(flat(context_hints(&app)).contains("⏎ command"));
        app.input.set("!ls".into());
        assert!(flat(context_hints(&app)).contains("⏎ shell"));
        // running · empty → interrupt only
        app.state = RunState::Running;
        app.input.clear();
        let s = flat(context_hints(&app));
        assert!(s.contains("esc interrupt"), "{s}");
        assert!(!s.contains("⏎"), "{s}");
        // running · draft → queue + send-now + interrupt
        app.input.set("follow-up".into());
        let s = flat(context_hints(&app));
        assert!(
            s.contains("⏎ queue") && s.contains("^x steer") && s.contains("esc interrupt"),
            "{s}"
        );
    }

    #[test]
    fn usage_footer_shows_turn_stats_and_cache() {
        let mut app = test_app();
        app.show_banner = false;
        app.slot_snapshots.insert(
            "conversation.composer.dock".into(),
            serde_json::from_value(serde_json::json!({
                "protocol": 0,
                "slot": "conversation.composer.dock",
                "rev": 1,
                "nodes": [
                    { "id": "stats:tokens", "kind": "generic", "title": "Input 1.8K tok · Output 412 tok", "body": "" },
                    { "id": "stats:counts", "kind": "generic", "title": "1 turn · 67 steps", "body": "" },
                    { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" }
                ]
            })).expect("slot snapshot"),
        );
        let frame = dump_frame(&mut app, 100, 20);
        let footer = frame.lines().last().expect("footer row");
        assert!(footer.contains("1 turn"), "{footer}");
        assert!(footer.contains("67 steps"), "{footer}");
        assert!(footer.contains("Cache hit 65%"), "{footer}");
        assert!(footer.contains("Input 1.8K tok"), "{footer}");
        assert!(footer.contains("Output 412 tok"), "{footer}");
    }

    #[test]
    fn agent_rail_lists_created_subagents_and_their_status() {
        let mut app = test_app();
        app.show_banner = false;
        app.subagents.push(crate::app::SubagentView {
            id: "child-1".into(),
            parent: "dsh-test".into(),
            label: "subagent 1".into(),
            running: true,
            transcript: crate::transcript::Transcript::new("child-1".into()),
        });
        app.subagents.push(crate::app::SubagentView {
            id: "child-2".into(),
            parent: "dsh-test".into(),
            label: "subagent 2".into(),
            running: false,
            transcript: crate::transcript::Transcript::new("child-2".into()),
        });

        let frame = dump_frame(&mut app, 100, 24);
        assert!(frame.contains("agents"), "{frame}");
        assert!(frame.contains("● subagent 1"), "{frame}");
        assert!(frame.contains("✓ subagent 2"), "{frame}");
    }

    #[test]
    fn child_view_replaces_the_composer_with_read_only_navigation() {
        let mut app = test_app();
        app.show_banner = false;
        let mut transcript = crate::transcript::Transcript::new("child-1".into());
        transcript.apply(crate::events::UiEvent::TextDelta {
            session: "child-1".into(),
            text: "child-only output".into(),
        });
        app.subagents.push(crate::app::SubagentView {
            id: "child-1".into(),
            parent: "dsh-test".into(),
            label: "subagent 1".into(),
            running: true,
            transcript,
        });
        app.active_subagent = Some("child-1".into());

        let frame = dump_frame(&mut app, 100, 24);
        assert!(frame.contains("child-only output"), "{frame}");
        assert!(frame.contains("read-only"), "{frame}");
        assert!(frame.contains("esc back"), "{frame}");
        assert!(
            !frame.contains("describe what you want to build"),
            "{frame}"
        );
        assert!(!frame.contains("send a prompt"), "{frame}");
    }

    #[test]
    fn active_running_child_does_not_show_the_main_idle_state() {
        let mut app = test_app();
        app.show_banner = false;
        app.subagents.push(crate::app::SubagentView {
            id: "child-1".into(),
            parent: "dsh-test".into(),
            label: "subagent 1".into(),
            running: true,
            transcript: crate::transcript::Transcript::new("child-1".into()),
        });
        app.active_subagent = Some("child-1".into());

        let frame = dump_frame(&mut app, 100, 24);
        assert!(frame.contains("working"), "{frame}");
        assert!(!frame.contains("● idle"), "{frame}");
    }

    #[test]
    fn usage_footer_drops_sections_on_narrow_screens() {
        let mut app = test_app();
        app.show_banner = false;
        app.slot_snapshots.insert(
            "conversation.composer.dock".into(),
            serde_json::from_value(serde_json::json!({
                "protocol": 0,
                "slot": "conversation.composer.dock",
                "rev": 1,
                "nodes": [
                    { "id": "stats:tokens", "kind": "generic", "title": "Input 1.8K tok · Output 412 tok", "body": "" },
                    { "id": "stats:counts", "kind": "generic", "title": "1 turn · 67 steps", "body": "" },
                    { "id": "stats:cache", "kind": "generic", "title": "Cache hit 65%", "body": "" },
                    { "id": "stats:time", "kind": "generic", "title": "LLM 15m9s · Tool call 0s", "body": "" },
                    { "id": "stats:speed", "kind": "generic", "title": "TTFT avg 1.5s · 0.5 tok/s", "body": "" }
                ]
            })).expect("slot snapshot"),
        );

        // Narrow: low-priority timing sections are dropped, tokens survive.
        let footer = dump_frame(&mut app, 40, 20);
        let footer = footer.lines().last().expect("footer row");
        assert!(footer.contains("Input"), "tokens survive: {footer}");
        assert!(footer.contains("Output"), "tokens survive: {footer}");
        assert!(
            !footer.contains("TTFT"),
            "TTFT dropped when narrow: {footer}"
        );
        assert!(!footer.contains("LLM"), "LLM dropped when narrow: {footer}");
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
    fn multiline_input_breaks_on_newlines() {
        let mut app = test_app();
        app.show_banner = false;
        app.input.buf = "hello\nworld".into();
        app.input.cursor = "hello\nworld".chars().count();
        let frame = dump_frame(&mut app, 40, 12);
        let hello = frame
            .lines()
            .position(|l| l.contains("hello"))
            .expect("first line");
        let world = frame
            .lines()
            .position(|l| l.contains("world"))
            .expect("second line");
        assert!(
            world > hello,
            "hard newline pushes the second line down:\n{frame}"
        );
    }

    #[test]
    fn composer_grows_to_show_a_multiline_draft_until_its_cap() {
        let mut app = test_app();
        app.show_banner = false;
        let _ = dump_frame(&mut app, 100, 30);
        let empty_chat_height = app.chat_view.area.height;

        app.input.set("one\ntwo\nthree\nfour\nfive\nsix".into());
        let frame = dump_frame(&mut app, 100, 30);

        assert!(
            app.chat_view.area.height < empty_chat_height,
            "composer should take rows from chat as the draft grows:\n{frame}"
        );
        assert!(frame.contains("one") && frame.contains("six"), "{frame}");
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
        assert_eq!(
            pet_rect(narrow, &app),
            None,
            "narrow terminals keep their columns"
        );
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
        assert!(
            !frame.contains("▄███"),
            "cells stay clear for the PNG:\n{frame}"
        );

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
        assert!(
            minimal_row.contains("Minimal mode ✓"),
            "current mode marked: {minimal_row}"
        );
        assert!(
            minimal_row.contains("▸"),
            "selection marker on the current row"
        );
        assert!(
            frame.contains("bash + str_replace_editor"),
            "descriptions visible\n{frame}"
        );
    }

    #[test]
    fn model_picker_marks_only_the_current_provider_model_pair() {
        use crate::app::{Picker, PickerItem, PickerKind};
        let mut app = test_app();
        app.show_banner = false;
        app.cfg.provider = "coding-plan-b".into();
        app.cfg.model = "deepseek-v4".into();
        app.picker = Some(Picker {
            kind: PickerKind::Model,
            title: " model ".into(),
            sel: 1,
            items: vec![
                PickerItem {
                    id: "deepseek-v4".into(),
                    label: "deepseek-v4".into(),
                    meta: "coding-plan-a · DeepSeek V4".into(),
                    provider: Some("coding-plan-a".into()),
                },
                PickerItem {
                    id: "deepseek-v4".into(),
                    label: "deepseek-v4".into(),
                    meta: "coding-plan-b · DeepSeek V4".into(),
                    provider: Some("coding-plan-b".into()),
                },
            ],
        });

        let frame = dump_frame(&mut app, 100, 24);
        let marked: Vec<&str> = frame.lines().filter(|line| line.contains("✓")).collect();
        assert_eq!(
            marked.len(),
            1,
            "only one provider/model row is current:\n{frame}"
        );
        assert!(
            marked[0].contains("coding-plan-b"),
            "current marker follows the provider: {}",
            marked[0]
        );
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
            app.chat_view
                .lines
                .iter()
                .any(|l| l.contains("Into the Unknown")),
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
        app.transcript
            .push_user("hello selection world".into(), false);
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
            crate::app::slice_by_cells(&app.chat_view.lines[line], 0, 9).trim_end(),
            "copied text matches the highlighted cells"
        );
    }

    #[test]
    fn permission_ask_overlay_lists_kind_name_and_title() {
        use crate::app::PermissionAskOverlay;
        use crate::bus::PermissionAskOption;
        let mut app = test_app();
        app.show_banner = false;
        app.permission_ask = Some(PermissionAskOverlay {
            title: "bash".into(),
            sel: 1,
            options: vec![
                PermissionAskOption {
                    option_id: "reject".into(),
                    kind: "reject_once".into(),
                    name: "Reject".into(),
                },
                PermissionAskOption {
                    option_id: "allow".into(),
                    kind: "allow_once".into(),
                    name: "Allow once".into(),
                },
            ],
            reply: None,
        });
        let frame = dump_frame(&mut app, 100, 30);
        assert!(frame.contains("bash"), "tool title in overlay\n{frame}");
        assert!(frame.contains("Reject"), "option name\n{frame}");
        assert!(frame.contains("Allow once"), "option name\n{frame}");
        assert!(frame.contains("reject_once"), "option kind\n{frame}");
        assert!(frame.contains("allow_once"), "option kind\n{frame}");
        let allow_row = frame
            .lines()
            .find(|l| l.contains("Allow once"))
            .expect("allow row");
        assert!(
            allow_row.contains("▸"),
            "selection on allow_once: {allow_row}"
        );
    }

    #[test]
    fn plugin_slider_overlay_renders_track_marks_and_keyboard_affordances() {
        use crate::app::{SliderMark, SliderOverlay};

        let mut app = test_app();
        app.show_banner = false;
        app.slider_overlay = Some(SliderOverlay {
            id: "liang-effort".into(),
            title: "Liang reasoning effort".into(),
            min: 0.0,
            max: 30.0,
            step: 1.0,
            marks: vec![
                SliderMark {
                    value: 0.0,
                    id: Some("off".into()),
                    label: "Off".into(),
                },
                SliderMark {
                    value: 15.0,
                    id: Some("high".into()),
                    label: "High".into(),
                },
                SliderMark {
                    value: 30.0,
                    id: Some("max".into()),
                    label: "Max".into(),
                },
            ],
            snap_to_marks: true,
            value: 16.0,
        });

        let frame = dump_frame(&mut app, 100, 30);

        assert!(frame.contains("Liang reasoning effort"), "title\n{frame}");
        for label in ["Off", "High", "Max"] {
            assert!(frame.contains(label), "mark {label}\n{frame}");
        }
        assert!(frame.contains("16 / 30"), "current numeric value\n{frame}");
        assert!(
            frame.contains("←/→ preview")
                && frame.contains("enter apply")
                && frame.contains("esc cancel"),
            "keyboard affordances\n{frame}"
        );
    }

    #[test]
    fn elicitation_overlay_renders_a_real_question_form() {
        use crate::app::ElicitationAskOverlay;
        use crate::elicitation::{
            ElicitationField, ElicitationFieldKind, ElicitationForm, ElicitationFormState,
            ElicitationOption,
        };

        let mut app = test_app();
        app.show_banner = false;
        app.elicitation_ask = Some(ElicitationAskOverlay {
            form: ElicitationFormState::new(ElicitationForm {
                message: "The agent needs your input.".into(),
                fields: vec![ElicitationField {
                    name: "question_0".into(),
                    custom_name: Some("question_0_custom".into()),
                    title: "Target".into(),
                    description: Some("Where should this run?".into()),
                    required: true,
                    kind: ElicitationFieldKind::Single {
                        options: vec![
                            ElicitationOption {
                                value: "local".into(),
                                label: "Local".into(),
                                description: Some("Run here.".into()),
                                custom: false,
                            },
                            ElicitationOption {
                                value: "other".into(),
                                label: "Other".into(),
                                description: None,
                                custom: true,
                            },
                        ],
                        default: None,
                    },
                }],
            }),
            reply: None,
        });

        let frame = dump_frame(&mut app, 100, 30);

        assert!(frame.contains("Target"), "field header\n{frame}");
        assert!(
            frame.contains("Where should this run?"),
            "question\n{frame}"
        );
        assert!(frame.contains("Local"), "choice\n{frame}");
        assert!(frame.contains("Other"), "custom choice\n{frame}");
        assert!(frame.contains("enter"), "keyboard affordance\n{frame}");
    }
}
