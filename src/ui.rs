//! Rendering: banner, scrollback, tips row, status bar, prompt, hints, overlays.

use ratatui::layout::{Alignment, Constraint, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, TableState,
};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, RunState};
use crate::logo;
use crate::pet::{SPRITE_H, SPRITE_W, WHALE_XS};
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

/// Height of the composer stats dock row: 1 when it fits and the stats slot is
/// mounted, even before it has data; else 0. Shared by the frame layout and
/// the kitty pixel sync so the pet anchors to the box bottom in both places.
pub fn composer_dock_height(app: &App, main_height: u16, child_view: bool) -> u16 {
    if !child_view
        && main_height >= 20
        && app
            .slot_snapshots
            .contains_key("conversation.composer.dock")
    {
        1
    } else {
        0
    }
}

/// The pet's cell rectangle — the kitty-graphics placement target —
/// perched inside the composer box's bottom-right (one row/column clear
/// of the rounded border), matching the sprite's 192:208 aspect in 1:2
/// cells. None hides it (`/liang` off, or the terminal is too cramped to
/// give up columns).
pub fn pet_rect(area: Rect, app: &App) -> Option<Rect> {
    if !app.pet_visible || area.width < 60 || area.height < 10 {
        return None;
    }
    let rows = composer_height(area.height).min(4);
    let cols = ((rows as u32 * 2 * SPRITE_W + SPRITE_H / 2) / SPRITE_H) as u16;
    Some(Rect::new(
        area.right().saturating_sub(1 + cols),
        area.bottom().saturating_sub(1 + rows),
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

    // Composer card: one rounded box wrapping the cap row (dock / tip /
    // · workspace title) and the native input surface (input well on top,
    // one meta row — run state + mode/permission chips + model — at the
    // bottom). The old shortcut-hints row is gone (the tip banner and
    // /keys carry that).
    let child_view = app.active_subagent.is_some();
    let agents_h = if app.subagents.is_empty() { 0 } else { 1 };
    let approval_h = if !child_view && !app.pending_cordis_approvals.is_empty() && main.height >= 12
    {
        1
    } else {
        0
    };
    // Keep the conversation visually detached from the composer chrome.
    // This row is intentionally left untouched so the canvas/background
    // shows through instead of becoming another panel-colored separator.
    let gap_h = if child_view { 0 } else { 1 };
    // Exactly one cap row tops the box (the `╭ … ─╮` border line). The
    // plugin input dock (plan-view's PLAN summary) wins the row when it
    // has nodes; otherwise the tip line carries it.
    let cap_h = if !child_view && main.height >= 16 {
        1
    } else {
        0
    };
    // The box's top border is the cap row and its bottom border carries
    // the meta row, so the box costs no extra rows — the input well stays
    // exactly as tall as the borderless layout.
    let composer_h = if child_view {
        1
    } else {
        resolved_composer_height(main, app)
    };
    // The composer stats dock (token / cache / timing readout) rides below
    // the box when the terminal is tall enough.
    let composer_dock_h = composer_dock_height(app, main.height, child_view);
    let chat_h = main
        .height
        .saturating_sub(composer_h + cap_h + composer_dock_h + agents_h + approval_h + gap_h);

    let chat = Rect::new(main.x, main.y, main.width, chat_h);
    let chrome_y = main.y + chat_h + gap_h;
    let agents = Rect::new(main.x, chrome_y, main.width, agents_h);
    let approval = Rect::new(main.x, chrome_y + agents_h, main.width, approval_h);
    let composer_box = Rect::new(
        main.x,
        chrome_y + agents_h + approval_h,
        main.width,
        cap_h + composer_h,
    );
    let composer = Rect::new(main.x, composer_box.y + cap_h, main.width, composer_h);
    let composer_dock = Rect::new(
        main.x,
        composer_box.y + composer_box.height,
        main.width,
        composer_dock_h,
    );

    draw_chat(f, app, chat);
    if agents_h > 0 {
        draw_agent_rail(f, app, agents);
    }
    if approval_h > 0 {
        draw_cordis_approval(f, app, approval);
    }
    // The pet floats inside the box's bottom-right; composer text keeps
    // clear of it. Anchor to the box's bottom — when the stats dock sits
    // below the box, the pet rides up with it instead of overlapping.
    let pet = if child_view {
        None
    } else {
        let pet_area = Rect::new(
            main.x,
            main.y,
            main.width,
            main.height.saturating_sub(composer_dock_h),
        );
        pet_rect(pet_area, app)
    };
    let pet_pad = if pet.is_some() { PET_PAD } else { 0 };
    if child_view {
        draw_child_navigation(f, app, composer);
    } else if cap_h > 0 {
        draw_composer_box(f, app, composer_box, pet_pad);
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
    draw_plugin_select(f, app, area);
    draw_view_overlay(f, app, area);
    draw_permission_ask(f, app, area);
    draw_elicitation_form(f, app, area);
}

fn draw_cordis_approval(f: &mut Frame, app: &App, area: Rect) {
    let Some(approval) = app.pending_cordis_approvals.first() else {
        return;
    };
    let theme = app.theme;
    let count = app.pending_cordis_approvals.len();
    let line = Line::from(vec![
        Span::styled(
            format!(" ⚠ 1/{count} {} · ", approval.name),
            Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}  ", approval.purpose),
            Style::default().fg(theme.fg_secondary),
        ),
        Span::styled(
            app.locale
                .tr(
                    "alt+1 current · alt+2 future · alt+3 reject",
                    "alt+1 当前版本 · alt+2 后续版本 · alt+3 拒绝",
                )
                .to_string(),
            Style::default().fg(theme.warn_soft()),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.panel)),
        area,
    );
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

fn draw_plugin_select(f: &mut Frame, app: &App, screen: Rect) {
    let Some(select) = &app.select_overlay else {
        return;
    };
    let theme = app.theme;
    // Fit the widest option (markers `▸ ● ` and the `    ` description
    // indent included); cap to the screen so one long option never exceeds
    // the terminal. Narrow terminals fall back to ellipsis per row.
    let needed = select
        .options
        .iter()
        .map(|option| {
            let label_w = option.label.width() + 4;
            let desc_w = option
                .description
                .as_deref()
                .filter(|text| !text.is_empty())
                .map(|text| text.width() + 4)
                .unwrap_or(0);
            label_w.max(desc_w)
        })
        .max()
        .unwrap_or(0) as u16;
    let width = (needed + 2).min(screen.width.saturating_sub(4)).max(28);
    let inner = width.saturating_sub(2) as usize;
    let label_cells = inner.saturating_sub(4);
    let desc_cells = inner.saturating_sub(4);

    let mut lines = Vec::new();
    for (index, option) in select.options.iter().enumerate() {
        let selected = index == select.sel;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(theme.brand),
            ),
            Span::styled(
                if selected { "● " } else { "○ " },
                Style::default().fg(if selected { theme.brand } else { theme.caption }),
            ),
            Span::styled(
                ellipsize_to(&option.label, label_cells),
                Style::default()
                    .fg(if selected {
                        theme.fg
                    } else {
                        theme.fg_secondary
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));
        if let Some(description) = option
            .description
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            lines.push(Line::from(Span::styled(
                format!("    {}", ellipsize_to(description, desc_cells)),
                Style::default().fg(theme.caption),
            )));
        }
    }
    let option_rows = lines.len();
    lines.push(Line::default());
    // Full text of the highlighted option, wrapped so a long choice stays
    // readable at the moment of confirming it.
    if let Some(option) = select.options.get(select.sel) {
        let mut preview = option.label.clone();
        if let Some(description) = option
            .description
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            preview.push_str(" — ");
            preview.push_str(description);
        }
        let mut wrapped = wrap_line(&preview, inner);
        if wrapped.len() > 3 {
            wrapped.truncate(3);
            let last = wrapped.last_mut().expect("three preview lines");
            *last = ellipsize_to(&format!("{last}…"), inner);
        }
        for line in wrapped {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(theme.fg_tertiary),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "↑/↓ select   enter apply   esc cancel",
        Style::default().fg(theme.caption),
    )));

    let mut height = lines.len() as u16 + 2;
    let max_height = screen.height.saturating_sub(2).max(5);
    if height > max_height {
        // Drop the earliest option rows first so the preview and the hint
        // line stay visible; the list itself has no scroll window yet.
        let overflow = (height - max_height) as usize;
        let keep = option_rows.saturating_sub(overflow);
        lines.drain(keep..option_rows);
        height = max_height;
    }
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
            format!(" {} ", select.title),
            Style::default().fg(theme.fg),
        ))
        .style(Style::default().bg(theme.panel));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Truncate `s` to at most `cells` display cells, appending `…` when it does
/// not fit. CJK/double-width aware; wide chars are never split.
fn ellipsize_to(s: &str, cells: usize) -> String {
    if s.width() <= cells {
        return s.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > cells.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

/// Greedy wrap `s` into lines of at most `cells` display cells. Never splits
/// a wide char; a single char wider than `cells` still occupies its own line.
fn wrap_line(s: &str, cells: usize) -> Vec<String> {
    if cells == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used > 0 && used + cw > cells {
            out.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(ch);
        used += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn draw_view_overlay(f: &mut Frame, app: &mut App, screen: Rect) {
    let theme = app.theme;
    let Some(view) = app.view_overlay.as_mut() else {
        return;
    };
    // A review pane, not a snackbar: wide terminals get up to 2/3 of the
    // screen (the old 84-column cap made long plans feel cramped).
    let width = screen
        .width
        .saturating_sub(4)
        .min((screen.width.saturating_mul(2) / 3).max(84))
        .max(24);
    let inner_width = width.saturating_sub(2) as usize;
    let lines = crate::slots::render_nodes(&view.nodes, &theme, inner_width);
    let height = (lines.len() as u16 + 2)
        .min(screen.height.saturating_sub(4))
        .max(4);
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 3,
        width,
        height,
    );
    // Clamp the scroll to the content: End / wheel overscroll stops at the
    // last content row instead of showing blank space below the review.
    // The stored offset is normalized here too, so scrolling back up starts
    // from the visible bottom (End parks at `usize::MAX` until the next draw).
    let max_scroll = lines
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize);
    let scroll = view.scroll.min(max_scroll) as u16;
    view.scroll = view.scroll.min(max_scroll);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(
            format!(" {} · ↑↓/wheel scroll · esc close ", view.title),
            Style::default().fg(theme.fg),
        ))
        .style(Style::default().bg(theme.panel).fg(theme.fg));
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(block), area);
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

/// The composer card fallback for short terminals (no cap row fits): a
/// borderless tinted surface (panel bg) that owns the status row and the
/// input well. A brand-blue edge bar glows while working. `pet_pad`
/// columns at the right stay text-free for the whale pet. Tall enough
/// terminals get `draw_composer_box` instead — the same surface wrapped
/// in the rounded frame that also carries the cap row.
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
    draw_meta_row(
        f,
        app,
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
    );
}

/// Meta row: run state + mode/permission chips left, model right,
/// collision-aware. The boxed layout renders this line on the bottom
/// border (`title_bottom`); the borderless fallback draws it as an inner
/// row.
fn meta_line(app: &App, width: usize) -> Line<'static> {
    let theme = app.theme;
    let left = status_title(app);
    let mut right_spans = status_right(app);
    let lw = left.width();
    let rw: usize = right_spans.iter().map(|s| s.content.width()).sum();
    if lw + rw + 2 > width {
        // Drop the contextual hints first, but keep the scroll position
        // beside the model so a longer left-side mode label never hides it.
        let shown_model = app
            .transcript
            .last_model
            .clone()
            .unwrap_or_else(|| app.cfg.model.clone());
        let mut compact = Vec::new();
        if app.scroll_up > 0 {
            compact.push(Span::styled(
                format!("▲{} · ", app.scroll_up),
                Style::default().fg(theme.caption),
            ));
        }
        compact.push(Span::styled(
            format!("{shown_model} "),
            Style::default().fg(theme.brand_soft),
        ));
        let compact_width = span_widths(&compact);
        if lw + compact_width + 2 <= width {
            right_spans = compact;
        } else if lw + shown_model.width() + 3 <= width {
            right_spans = vec![Span::styled(
                format!("{shown_model} "),
                Style::default().fg(theme.brand_soft),
            )];
        } else {
            right_spans = Vec::new();
        }
    }
    let rw: usize = right_spans.iter().map(|s| s.content.width()).sum();
    let mut spans = left.spans;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(lw + rw))));
    spans.extend(right_spans);
    Line::from(spans)
}

fn draw_meta_row(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Paragraph::new(meta_line(app, area.width as usize)), area);
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
/// the transcript tail). Chips lead with a plain dot instead of emoji —
/// the color carries the meaning (permission turns warn under full access).
fn status_title(app: &App) -> Line<'static> {
    if !app.session_bound {
        return Line::default();
    }
    let theme = app.theme;
    let mut spans: Vec<Span> = Vec::new();
    let key_style = Style::default()
        .fg(theme.brand_soft)
        .add_modifier(Modifier::BOLD);
    // Mode chips: folded from the durable event stream (same facts as the
    // Web UI chips). Stock defaults render until the host reports its own
    // facts, so the landing screen still advertises preset + permission.
    let preset = app.modes.agent_preset.as_deref().unwrap_or("standard");
    spans.push(Span::styled(
        format!("· {}", app.agent_label(preset)),
        Style::default().fg(theme.fg_tertiary),
    ));
    spans.push(Span::styled(" ctrl+shift+a", key_style));
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
        format!(" · {label}"),
        Style::default().fg(if perm == "danger-full-access" {
            theme.warn_soft()
        } else {
            theme.fg_tertiary
        }),
    ));
    spans.push(Span::styled(" shift+tab", key_style));
    if let Some(approval) = &app.modes.approval {
        spans.push(Span::styled(
            format!(" · {approval}"),
            Style::default().fg(theme.fg_tertiary),
        ));
    }
    if app.modes.plan {
        spans.push(Span::styled(
            " · plan".to_string(),
            Style::default().fg(theme.brand_soft),
        ));
    }
    Line::from(spans)
}

/// Contextual shortcut hints — a tiny state machine over (run state ×
/// draft): what Enter does *right now*, how to interrupt, how to steer
/// immediately. Idle+empty falls back to the `^K keys` discovery hint.
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
        (false, true) => vec![("^K", app.locale.tr("keys", "快捷键"))],
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

fn span_widths(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

fn slot_has_nodes(app: &App, name: &str) -> bool {
    app.slot_snapshots
        .get(name)
        .is_some_and(|snapshot| !snapshot.nodes.is_empty())
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

/// The composer stats dock below the box: one compact row of plugin
/// sections (token/cache usage, timing) in registration order; low-
/// priority tail sections simply do not fit on narrow terminals.
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

fn compact_slot_line(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let sections = compact_slot_sections(snapshot, theme, width);
    compact_slot_sections_line(&sections, theme)
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
    let mut banner_count;
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
    if app.show_banner && lines.len() < inner.height as usize {
        let remaining = inner.height as usize - lines.len();
        let top = remaining / 2;
        let bottom = remaining - top;
        let mut centered_lines = Vec::with_capacity(inner.height as usize);
        centered_lines.resize(top, Line::default());
        centered_lines.append(&mut lines);
        centered_lines.resize(centered_lines.len() + bottom, Line::default());
        lines = centered_lines;
        owners.resize(lines.len(), None);
        banner_count = lines.len();
    }
    if !app.show_banner {
        lines.extend(layout.lines);
        owners.extend(layout.owners);
    }
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
        format!(" · {} ", compact_workspace(&app.cfg.workspace, path_width)),
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

/// The composer card as one rounded box: the cap row doubles as the top
/// border (plugin input dock wins over the tip line, plus the right-
/// aligned · workspace title) and the meta row rides the bottom border —
/// the input well owns every inner row. The brand glow replaces the left
/// border while a turn runs.
fn draw_composer_box(f: &mut Frame, app: &mut App, area: Rect, pet_pad: u16) {
    let theme = app.theme;
    let running = !matches!(app.state, RunState::Idle);
    let has_input_dock = slot_has_nodes(app, "conversation.input.dock");
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
    // The dock (PLAN summary) owns the single cap row; the tip line only
    // appears when no dock is present.
    let title = ellipsize_line(
        dock_line.clone().unwrap_or_else(|| tip_line(app)),
        title_budget,
        Style::default().fg(theme.caption),
    );
    let title_width = span_widths(&title.spans) as u16;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(title)
        .title(workspace)
        .title_bottom(meta_line(app, area.width.saturating_sub(2) as usize))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width < 4 || inner.height < 2 {
        return;
    }

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

    // Running glow: the brand bar replaces the left border (corners and
    // both title rows stay intact).
    if running {
        let bar: Vec<Line> = (0..inner.height).map(|_| Line::from("▎")).collect();
        f.render_widget(
            Paragraph::new(bar).style(Style::default().fg(theme.brand)),
            Rect::new(area.x, area.y + 1, 1, inner.height),
        );
    }

    // The pet keeps clear of the text: content is inset from the right
    // border by pet_pad columns.
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width.saturating_sub(pet_pad),
        inner.height,
    );

    // Draft first: the well owns every inner row — the meta row lives on
    // the bottom border.
    app.att_chips.clear();
    app.att_thumbs.clear();
    app.composer_wrap_width = content.width.saturating_sub("❯ ".width() as u16).max(1) as usize;
    draw_input(f, app, content);
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
        let begins_section = cmd.section.is_some()
            && (i == start
                || matches
                    .get(i.saturating_sub(1))
                    .and_then(|previous| previous.section.as_deref())
                    != cmd.section.as_deref());
        // Desc column absorbs the remaining width and ellipsizes; a long
        // skill description must never hard-clip at the box edge.
        let section_cells = if begins_section {
            cmd.section.as_deref().unwrap_or_default().width() + 3
        } else {
            0
        };
        let desc_cells = (w as usize).saturating_sub(name_w + 5 + section_cells);
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.brand)),
            Span::styled(pad_or_ellipsize(&cmd.usage, name_w), name_style),
        ];
        if begins_section {
            spans.push(Span::styled(
                format!(" {} ·", cmd.section.as_deref().unwrap_or_default()),
                Style::default().fg(theme.hint),
            ));
        }
        spans.push(Span::styled(
            format!(" {}", ellipsize_to(&desc, desc_cells)),
            Style::default().fg(theme.caption),
        ));
        lines.push(Line::from(spans));
    }
    let title = if matches.iter().any(|m| m.completion.is_some()) {
        app.locale.tr(" options ", " 候选 ")
    } else if matches.iter().any(|m| m.skill) {
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

/// Left-pad to a fixed display width so picker columns line up
/// (`{:<n}` pads by chars, which misaligns CJK labels).
fn pad_to_width(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn draw_model_picker(f: &mut Frame, app: &mut App, screen: Rect) {
    let theme = app.theme;
    let Some(picker) = &app.picker else {
        return;
    };
    // The active model is identified by provider + id because multiple
    // coding plans can expose the same upstream model id.
    let kind = picker.kind;
    let title = picker.title.clone();
    let sel = picker.sel;
    let items = picker.items.clone();
    // Current-identity ids, precomputed so the row builder below never
    // borrows `app` (the ListView render needs a mutable picker).
    let current_model = app.cfg.model.clone();
    let current_provider = app.cfg.provider.clone();
    let current_mode = app.current_mode();
    let current_palette = app.active_palette_id.clone();
    let current_permission = app.current_permission().to_string();
    let is_current = move |item: &crate::app::PickerItem| match kind {
        crate::app::PickerKind::Model => {
            item.id == current_model
                && item
                    .provider
                    .as_deref()
                    .is_none_or(|provider| provider == current_provider)
        }
        crate::app::PickerKind::Mode => item.id == current_mode,
        crate::app::PickerKind::Theme => item.id == current_palette,
        crate::app::PickerKind::UiPlugin => false,
        crate::app::PickerKind::Permission => item.id == current_permission,
        crate::app::PickerKind::Effort
        | crate::app::PickerKind::Session
        | crate::app::PickerKind::Subagent
        | crate::app::PickerKind::Auth
        | crate::app::PickerKind::StaticPlugin
        | crate::app::PickerKind::CordisPlugin
        | crate::app::PickerKind::CordisApproval => false,
    };
    // The popup caps at the screen; `ListView` scrolls the overflow instead
    // of clipping it out of reach.
    let h = (items.len() as u16 + 2).min(screen.height.saturating_sub(2));
    // Fit the widest row (marker + padded label + ✓ + meta); cap to the screen.
    let needed = items
        .iter()
        .map(|item| {
            let label_w = item.label.width() + if is_current(item) { 2 } else { 0 };
            2 + label_w.max(crate::app::PICKER_LABEL_COL) + item.meta.width()
        })
        .max()
        .unwrap_or(0) as u16;
    let cap = screen.width.saturating_sub(4).max(24);
    let overflow = items.len() as u16 + 2 > h;
    // The scrollbar gets its own column inside the popup when rows overflow,
    // so the rounded border never has glyphs drawn over its corners.
    let w = if overflow {
        ((needed + 2).max(58) + 1).min(cap)
    } else {
        (needed + 2).max(58).min(cap)
    };
    let x = screen.x + (screen.width - w) / 2;
    let y = screen.y + (screen.height - h) / 3;
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);
    let item_count = items.len();
    // Rows as a ratatui Table: the selection column is always reserved
    // (marker `▸ ` on the picked row), the label column is fixed-width so
    // metas line up, and the meta column absorbs the remaining width —
    // row highlight paints the whole line, scrollbar gutter included.
    let mut rows = Vec::with_capacity(item_count);
    for item in &items {
        // The current model/mode gets a ✓ pinned to its label — it survives
        // narrow terminals, unlike a right-edge tag.
        let label = if is_current(item) {
            format!("{} ✓", item.label)
        } else {
            item.label.clone()
        };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(
                pad_to_width(&label, crate::app::PICKER_LABEL_COL),
                Style::default().fg(theme.fg_secondary),
            )),
            Cell::from(Span::styled(
                item.meta.clone(),
                Style::default().fg(theme.caption),
            )),
        ]));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(title, Style::default().fg(theme.caption)))
        .style(Style::default().bg(theme.panel));
    // TableState follows the selection into view (the viewport stays pinned
    // to the ends), matching the old ListView behavior.
    let table = Table::new(
        rows,
        [
            Constraint::Length(crate::app::PICKER_LABEL_COL as u16),
            Constraint::Min(0),
        ],
    )
    .block(block)
    .column_spacing(0)
    .highlight_symbol("▸ ")
    .highlight_spacing(HighlightSpacing::Always)
    .row_highlight_style(
        Style::default()
            .fg(theme.brand)
            .bg(theme.chip_bg)
            .add_modifier(Modifier::BOLD),
    );
    let mut table_state =
        TableState::new().with_selected(Some(sel.min(items.len().saturating_sub(1))));
    f.render_stateful_widget(table, area, &mut table_state);
    if overflow {
        let inner = area.inner(Margin::new(1, 1));
        let mut sb_state = ScrollbarState::new(item_count).position(table_state.offset());
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .style(Style::default().fg(theme.border)),
            inner,
            &mut sb_state,
        );
    }
    // The viewport is the source of truth for what is visible; selection
    // moves (mouse later, programmatic now) land back in the picker.
    if let Some(picker) = &mut app.picker {
        if let Some(selected) = table_state.selected() {
            picker.sel = selected;
        }
        // Page keys jump a screenful: the rows the popup actually shows.
        app.picker_page_rows = h.saturating_sub(2) as usize;
    }
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

fn draw_elicitation_form(f: &mut Frame, app: &mut App, screen: Rect) {
    let theme = app.theme;
    let Some(ask) = app.elicitation_ask.as_mut() else {
        return;
    };
    let Some(state) = ask.form.fields.get(ask.form.index) else {
        return;
    };
    let content_width = screen.width.saturating_sub(10).clamp(28, 74) as usize;
    // Fixed chrome: message + field title on top, answers pinned to the
    // bottom. The question detail — for a plan review that is the complete
    // plan markdown — owns a scrollable pane between them and renders
    // through the full markdown pipeline.
    let top = vec![
        Line::from(Span::styled(
            ask.form.message.clone(),
            Style::default().fg(theme.caption),
        )),
        Line::default(),
        Line::from(Span::styled(
            state.field.title.clone(),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        )),
    ];
    let desc = state
        .field
        .description
        .as_ref()
        .map(|text| crate::markdown::render(text, &theme, content_width));
    let mut bottom = Vec::new();
    bottom.push(Line::default());

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
                bottom.push(Line::from(spans));
                if option.custom && state.editing_custom {
                    bottom.push(Line::from(vec![
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
                bottom.push(Line::from(vec![
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
            bottom.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(theme.brand)),
                Span::styled(
                    format!("{}▏", state.input.buf),
                    Style::default().fg(theme.fg),
                ),
            ]));
        }
    }
    if let Some(error) = &ask.form.error {
        bottom.push(Line::default());
        bottom.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(theme.err),
        )));
    }

    let cap_w = screen.width.saturating_sub(4).max(24);
    let w = (content_width as u16 + 4).min(cap_w);
    // The pane gets whatever rows the terminal leaves after the fixed top /
    // bottom and the border; overscroll clamps to the last content row.
    let budget = screen.height.saturating_sub(2) as usize;
    let base = top.len() + bottom.len() + 2;
    let desc_len = desc.as_ref().map_or(0, Vec::len);
    let middle_h = desc_len.min(budget.saturating_sub(base));
    let max_scroll = if middle_h == 0 {
        0
    } else {
        desc_len.saturating_sub(middle_h)
    };
    let scroll = ask.scroll.min(max_scroll) as u16;
    // Normalize the stored offset so scrolling back up starts from the
    // visible bottom (End parks at `usize::MAX` until the next draw).
    ask.scroll = ask.scroll.min(max_scroll);
    let h = (base + middle_h).min(budget) as u16;
    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 3;
    let area = Rect::new(x, y, w, h);
    f.render_widget(Clear, area);
    let count = ask.form.fields.len();
    let title = if max_scroll > 0 {
        format!(
            " ask · {}/{} · pgup/pgdn/end scroll · enter next/submit · tab skip · esc cancel ",
            ask.form.index + 1,
            count,
        )
    } else {
        format!(
            " ask · {}/{} · enter next/submit · tab skip · esc cancel ",
            ask.form.index + 1,
            count,
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(title, Style::default().fg(theme.caption)))
        .style(Style::default().bg(theme.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let top_h = top.len().min(inner.height as usize);
    f.render_widget(
        Paragraph::new(top),
        Rect::new(inner.x, inner.y, inner.width, top_h as u16),
    );
    if let Some(lines) = desc {
        let middle = Rect::new(
            inner.x,
            inner.y.saturating_add(top_h as u16),
            inner.width,
            middle_h as u16,
        );
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), middle);
    }
    let bottom_h = inner.height as usize - top_h - middle_h;
    f.render_widget(
        Paragraph::new(bottom),
        Rect::new(
            inner.x,
            inner.y.saturating_add((top_h + middle_h) as u16),
            inner.width,
            bottom_h as u16,
        ),
    );
}

/// Welcome banner: Martty logo, project URL, session facts. Shown while
/// `app.show_banner` is set; it dives on the first real prompt.
fn banner_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let mut out = Vec::new();
    if let Some(hero) = app
        .slot_snapshots
        .get("welcome.hero")
        .filter(|snapshot| !snapshot.nodes.is_empty())
    {
        out.extend(crate::slots::render_welcome_hero(
            hero,
            theme,
            width as usize,
        ));
    } else if app.ui_preset == "deepseek" {
        out.extend(crate::deepseek_logo::lines(theme, width));
        out.push(Line::default());
        out.push(centered(
            width,
            vec![Span::styled(
                app.locale.tr("Into the Unknown", "探索未知").to_string(),
                Style::default()
                    .fg(theme.fg_tertiary)
                    .add_modifier(Modifier::BOLD),
            )],
        ));
    } else {
        out.extend(logo::martty_logo_lines(theme, width));
        out.push(Line::default());

        // Project URL, in the hero slot where the old slogan was.
        out.push(centered(
            width,
            vec![Span::styled(
                "https://martty.sh".to_string(),
                Style::default()
                    .fg(theme.fg_tertiary)
                    .add_modifier(Modifier::BOLD),
            )],
        ));
    }
    out.push(Line::default());

    if let Some(info) = app
        .slot_snapshots
        .get("welcome.info")
        .filter(|snapshot| !snapshot.nodes.is_empty())
    {
        if matches!(
            info.nodes.as_slice(),
            [crate::slots::TuiNode::Welcomeinfo { .. }]
        ) {
            out.extend(welcome_info_lines(app));
        } else {
            out.extend(crate::slots::render(info, theme, width as usize));
        }
    } else {
        out.extend(welcome_info_lines(app));
    }
    out
}

fn welcome_info_lines(app: &App) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let mut out = Vec::new();

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
#[path = "../tests/unit/ui__tests.rs"]
mod tests;


#[cfg(test)]
#[path = "../tests/unit/ui__rpc_probe.rs"]
mod rpc_probe;
