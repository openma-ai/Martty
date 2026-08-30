//! Rendering: banner, scrollback, tips row, status bar, prompt, hints, overlays.

use ratatui::layout::{Alignment, Constraint, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, FrameExt, HighlightSpacing, Paragraph, Row, Scrollbar,
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
fn composer_dock_height(app: &App, main_height: u16, child_view: bool) -> u16 {
    if !child_view
        && main_height >= 20 + navigation_dock_height(app, main_height)
        && app
            .slot_snapshots
            .contains_key("conversation.composer.dock")
    {
        1
    } else {
        0
    }
}

/// One compact session-navigation row inside the composer, immediately above
/// its meta row. A Client Plugin snapshot wins; the native Agent rail is only
/// the no-Client-tree fallback, and it auto-closes once every subagent task
/// has ended (issue #80): a failed task or an active inline selection keeps
/// it visible.
fn navigation_dock_height(app: &App, main_height: u16) -> u16 {
    if main_height >= 8 {
        let plugin_owns = app
            .slot_snapshots
            .contains_key("conversation.navigation.dock");
        let native_visible = !plugin_owns
            && !app.subagents.is_empty()
            && (app.agent_selection.is_some()
                || app.subagents.iter().any(|view| view.running || view.failed));
        if slot_has_nodes(app, "conversation.navigation.dock") || native_visible {
            return 1;
        }
    }
    0
}

/// Compact client-owned FIFO shelf between the conversation and composer.
/// It grows just enough to show a few prompts without crowding out chat.
fn queue_shelf_height(app: &App, main_height: u16, child_view: bool) -> u16 {
    let plugin_owned = app
        .slot_snapshots
        .get("conversation.input.dock")
        .is_some_and(|snapshot| {
            snapshot
                .nodes
                .iter()
                .any(|node| node.id().starts_with("queue-view:"))
        });
    if child_view || app.queued == 0 || plugin_owned {
        return 0;
    }
    if main_height >= 12 {
        let item_rows = main_height.saturating_sub(12).max(1) as usize;
        1 + app.queued.min(item_rows) as u16
    } else {
        1
    }
}

/// Structured input-dock nodes expand inside the composer card. Generic
/// summaries keep using the one-line cap, so Plan/Goal stay compact while a
/// selector such as Queue can temporarily reveal its rows above the input.
fn input_dock_body_height(app: &App, main_width: u16, main_height: u16, child_view: bool) -> u16 {
    if child_view || main_height < 14 {
        return 0;
    }
    let lines = input_dock_body_lines(app, main_width.saturating_sub(2) as usize);
    (lines.len() as u16).min(main_height.saturating_sub(12))
}

/// The pet's cell rectangle — the kitty-graphics placement target —
/// perched inside the composer box's bottom-right (one row/column clear
/// of the rounded border), matching the sprite's 192:208 aspect in 1:2
/// cells. None hides it (`/liang` off, or the terminal is too cramped to
/// give up columns).
fn pet_rect(area: Rect, app: &App) -> Option<Rect> {
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
    app.pet_want = None;
    app.caret_cell = None;
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
    // has nodes; otherwise the tip line carries it. The row is worth
    // keeping down to 10 rows tall — below that (chat would drop under
    // ~5 rows) the borderless fallback takes over instead.
    let cap_h = if !child_view && main.height >= 10 {
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
    let navigation_dock_h = navigation_dock_height(app, main.height);
    let input_dock_body_h = input_dock_body_height(app, main.width, main.height, child_view);
    let queue_shelf_h = queue_shelf_height(app, main.height, child_view);
    let chat_h = main.height.saturating_sub(
        composer_h
            + cap_h
            + input_dock_body_h
            + composer_dock_h
            + navigation_dock_h
            + queue_shelf_h
            + approval_h
            + gap_h,
    );

    let chat = Rect::new(main.x, main.y, main.width, chat_h);
    let chrome_y = main.y + chat_h + gap_h;
    let approval = Rect::new(main.x, chrome_y, main.width, approval_h);
    let queue_shelf = Rect::new(main.x, chrome_y + approval_h, main.width, queue_shelf_h);
    let composer_box = Rect::new(
        main.x,
        queue_shelf.y + queue_shelf.height,
        main.width,
        cap_h + input_dock_body_h + composer_h + navigation_dock_h,
    );
    let composer = if child_view {
        Rect::new(
            main.x,
            composer_box.y + navigation_dock_h,
            main.width,
            composer_h,
        )
    } else if cap_h == 0 {
        composer_box
    } else {
        Rect::new(
            main.x,
            composer_box.y + cap_h + input_dock_body_h,
            main.width,
            composer_h,
        )
    };
    let composer_dock = Rect::new(
        main.x,
        composer_box.y + composer_box.height,
        main.width,
        composer_dock_h,
    );
    let navigation_dock = if child_view {
        Rect::new(main.x, composer_box.y, main.width, navigation_dock_h)
    } else {
        Rect::new(
            main.x,
            composer_box.bottom().saturating_sub(1 + navigation_dock_h),
            main.width,
            navigation_dock_h,
        )
    };

    draw_chat(f, app, chat);
    if approval_h > 0 {
        draw_cordis_approval(f, app, approval);
    }
    if queue_shelf_h > 0 {
        draw_queue_shelf(f, app, queue_shelf);
    }
    // The pet floats inside the box's bottom-right; composer text keeps
    // clear of it. Anchor to the box's bottom — when the stats dock sits
    // below the box, the pet rides up with it instead of overlapping. The
    // anchor is recorded for main()'s kitty reconciler (single source of
    // truth for the pet's place — no duplicated dock math there).
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
    if app.pet_pixels {
        let working = !matches!(app.state, RunState::Idle);
        app.pet_want = pet.map(|rect| (rect, working));
    }
    let pet_pad = if pet.is_some() { PET_PAD } else { 0 };
    if child_view {
        draw_child_navigation(f, app, composer);
    } else if cap_h > 0 {
        draw_composer_box(
            f,
            app,
            composer_box,
            input_dock_body_h,
            pet_pad,
            navigation_dock_h,
        );
    } else {
        draw_composer(f, app, composer, pet_pad, navigation_dock_h);
    }
    if navigation_dock_h > 0 {
        let embedded = !child_view && cap_h > 0;
        if slot_has_nodes(app, "conversation.navigation.dock") {
            draw_navigation_dock(f, app, navigation_dock, embedded);
        } else {
            draw_agent_rail(f, app, navigation_dock, embedded);
        }
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
        let menu_anchor = if queue_shelf_h > 0 {
            queue_shelf
        } else {
            composer
        };
        draw_slash_menu(f, app, menu_anchor, chat);
        // The @file browser rides the same anchor; drawn after the slash
        // menu (the two are mutually exclusive, but the browser owns the
        // space above the composer either way).
        draw_file_menu(f, app, menu_anchor, chat);
        // Hover/cursor preview for inline [image n] chips sits above the
        // composer (drawn last so it tops the menu-free chat area).
        draw_attachment_preview(f, app, composer, area);
    }
    draw_model_picker(f, app, area);
    draw_plugin_tree(f, app, area);
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

fn draw_queue_shelf(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let theme = app.theme;
    let title = app.locale.tr("Queue", "队列");
    let title_text = format!(" {title} · {}", app.queued);
    let preview_limit = if area.height > 1 {
        area.height.saturating_sub(1) as usize
    } else {
        1
    };
    let previews = app.queue_previews(preview_limit);
    let mut lines = Vec::with_capacity(area.height as usize);

    if area.height == 1 {
        let prefix = format!("▎{title_text}  ");
        let summary = previews.first().map(|preview| preview.summary.as_str());
        let summary = summary
            .map(|text| {
                ellipsize_to(
                    text,
                    area.width.saturating_sub(prefix.width() as u16) as usize,
                )
            })
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                Style::default()
                    .fg(theme.brand)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(summary, Style::default().fg(theme.fg_secondary)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("▎", Style::default().fg(theme.brand)),
            Span::styled(
                title_text,
                Style::default()
                    .fg(theme.fg_secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if app.queue_selecting() {
                    app.locale
                        .tr(
                            "  ·  ↑/↓ choose · enter edit · esc close",
                            "  ·  ↑/↓ 选择 · enter 编辑 · esc 关闭",
                        )
                        .to_string()
                } else {
                    app.locale
                        .tr(
                            "  ·  enter send first · ⌥↑ edit",
                            "  ·  enter 发送队首 · ⌥↑ 编辑",
                        )
                        .to_string()
                },
                Style::default().fg(theme.caption),
            ),
        ]));
        for preview in previews {
            let confirming_delete = preview.editing && app.queue_delete_confirming();
            let marker = if confirming_delete {
                "×"
            } else if preview.editing {
                "✎"
            } else if preview.selected {
                "▸"
            } else {
                "›"
            };
            let prefix = format!("  {marker} {}  ", preview.ordinal);
            let summary = ellipsize_to(
                &preview.summary,
                area.width.saturating_sub(prefix.width() as u16 + 1) as usize,
            );
            let style = if confirming_delete {
                Style::default().fg(theme.err)
            } else if preview.editing {
                Style::default().fg(theme.warn)
            } else if preview.selected {
                Style::default().fg(theme.brand)
            } else {
                Style::default().fg(theme.fg_secondary)
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                Span::styled(summary, style),
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.surface)),
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

/// Mark every cell in `area` [`CellDiffOption::AlwaysUpdate`] so the frame
/// diff rewrites them unconditionally (issue #38). Scrolling CJK content can
/// leave orphan halves of wide chars on the real screen while both ratatui
/// buffers agree the cell is unchanged, so the diff skips it and the leftover
/// half-glyph stays as a black block. Forcing a full rewrite of the scroll
/// surfaces costs a few KB of output per drawn frame and clears the stale
/// cells regardless of terminal-specific wide-char erase behavior.
fn force_full_rewrite(f: &mut Frame, area: Rect) {
    let buf = f.buffer_mut();
    // Overlays compute their rect from screen math that can saturate at the
    // top edge; never index a cell outside the buffer (widgets clip via
    // `intersection`, direct indexing would panic).
    let area = area.intersection(Rect::new(0, 0, buf.area.width, buf.area.height));
    if area.is_empty() {
        return;
    }
    for pos in area.positions() {
        buf[pos].set_diff_option(ratatui::buffer::CellDiffOption::AlwaysUpdate);
    }
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
    // Popup bodies sit one indent level off the border (issue #49): wrap one
    // level narrower, then prefix every rendered row with the indent so
    // continuation lines stay aligned.
    const POPUP_INDENT: u16 = 2;
    let inner_width = width.saturating_sub(2 + POPUP_INDENT) as usize;
    let mut lines = crate::slots::render_nodes(&view.nodes, &theme, inner_width);
    let indent = " ".repeat(POPUP_INDENT as usize);
    for line in &mut lines {
        line.spans.insert(0, Span::raw(indent.clone()));
    }
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
    force_full_rewrite(f, area);
}

/// `/plugins` inventory popup: a two-level provider → plugin tree rendered
/// with tui-tree-widget (the maintained ratatui port of the widget named in
/// issue #49). The widget owns the viewport and the selection; rows are
/// rebuilt from `app.static_plugins` every frame so a palette switch
/// recolors them immediately.
fn draw_plugin_tree(f: &mut Frame, app: &mut App, screen: Rect) {
    use std::collections::{BTreeMap, HashSet};
    use tui_tree_widget::{Tree, TreeItem};

    let Some(tree) = &mut app.plugin_tree else {
        return;
    };
    let theme = app.theme;

    // Group by provider, keeping loader order inside each provider. Leaf
    // identifiers are entry ids (unique per loader), provider identifiers
    // are scopes, so the widget's paths [provider] / [provider, entryId]
    // stay stable across frames.
    let mut groups: BTreeMap<String, Vec<&crate::bus::StaticPluginItem>> = BTreeMap::new();
    for plugin in &app.static_plugins {
        groups
            .entry(crate::app::plugin_provider(&plugin.module_name))
            .or_default()
            .push(plugin);
    }
    let mut items = Vec::with_capacity(groups.len());
    let mut widest = 0usize;
    let mut leaf_rows = 0usize;
    for (provider, plugins) in &groups {
        let mut children = Vec::with_capacity(plugins.len());
        let mut seen: HashSet<&str> = HashSet::with_capacity(plugins.len());
        for plugin in plugins {
            if !seen.insert(&plugin.entry_id) {
                continue;
            }
            let short = crate::app::plugin_short_name(&plugin.module_name);
            let meta = format!(
                " · {} · {}",
                if plugin.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                plugin.fiber_phase.as_deref().unwrap_or("inactive"),
            );
            widest = widest.max(short.width() + meta.width());
            leaf_rows += 1;
            children.push(TreeItem::new_leaf(
                plugin.entry_id.clone(),
                Line::from(vec![
                    Span::styled(short, Style::default().fg(theme.fg_secondary)),
                    Span::styled(meta, Style::default().fg(theme.caption)),
                ]),
            ));
        }
        let label = format!("{provider} · {}", children.len());
        widest = widest.max(label.width());
        items.push(
            TreeItem::new(
                provider.clone(),
                Line::styled(label, Style::default().fg(theme.brand)),
                children,
            )
            .expect("provider identifiers are deduped by BTreeMap"),
        );
    }

    // Popup geometry mirrors the picker: width follows the widest row (with
    // room for the highlight symbol + node indent), height caps at the
    // screen and the tree widget scrolls the overflow.
    let total_rows = leaf_rows + groups.len();
    let h = (total_rows as u16 + 2)
        .min(screen.height.saturating_sub(2))
        .max(4);
    let cap = screen.width.saturating_sub(4).max(24);
    let overflow = total_rows as u16 + 2 > h;
    let w = if overflow {
        ((widest as u16 + 8).max(58) + 1).min(cap)
    } else {
        (widest as u16 + 8).max(58).min(cap)
    };
    let x = screen.x + (screen.width - w) / 2;
    let y = screen.y + (screen.height - h) / 3;
    let area = Rect::new(x, y, w, h);
    app.plugin_tree_page_rows = h.saturating_sub(2) as usize;

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.brand))
        .title(Span::styled(
            tree.title.clone(),
            Style::default().fg(theme.caption),
        ))
        .style(Style::default().bg(theme.panel));
    let widget = Tree::new(&items)
        .expect("plugin tree identifiers are deduped")
        .block(block)
        .highlight_symbol("▸ ")
        .highlight_style(
            Style::default()
                .fg(theme.brand)
                .bg(theme.chip_bg)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(widget, area, &mut tree.state);
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

/// Compact Plugin-owned navigation rail. The leading identity + active item
/// and the final action survive first when the terminal narrows; middle
/// entries collapse before navigation becomes ambiguous.
fn draw_navigation_dock(f: &mut Frame, app: &mut App, area: Rect, embedded: bool) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    let Some(snapshot) = app.slot_snapshots.get("conversation.navigation.dock") else {
        return;
    };
    let theme = app.theme;
    let mut sections = snapshot
        .nodes
        .iter()
        .filter_map(|node| {
            compact_node_spans(node, &theme, area.width as usize, app.spinner()).map(|spans| {
                CompactSlotSection {
                    spans,
                    action: node.action().cloned(),
                    essential: matches!(
                        node,
                        crate::slots::TuiNode::Generic {
                            tone: Some(tone),
                            ..
                        } if matches!(tone.as_str(), "brand" | "brand_soft")
                    ),
                }
            })
        })
        .collect::<Vec<_>>();
    if sections.is_empty() {
        return;
    }

    let trailing = sections
        .last()
        .is_some_and(|section| section.action.is_some())
        .then(|| sections.pop().expect("last section exists"));
    let trailing_width = trailing
        .as_ref()
        .map(|section| span_widths(&section.spans))
        .unwrap_or(0);
    let left_budget = (area.width as usize)
        .saturating_sub(4 + trailing_width)
        .max(1);
    while sections.len() > 2 {
        let width = sections
            .iter()
            .map(|section| span_widths(&section.spans))
            .sum::<usize>()
            + sections.len().saturating_sub(1) * 2;
        if width <= left_budget {
            break;
        }
        let removable = (1..sections.len())
            .rev()
            .find(|index| !sections[*index].essential);
        let Some(index) = removable else {
            break;
        };
        sections.remove(index);
    }

    let border_style = Style::default().fg(theme.border);
    let mut spans = vec![Span::styled(if embedded { "│" } else { " " }, border_style)];
    let mut x = 1usize;
    if !embedded {
        spans.push(Span::raw(" "));
        x += 1;
    }
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
            x += 2;
        }
        let width = span_widths(&section.spans);
        if let Some(action) = section.action.clone() {
            app.slot_actions.push((
                Rect::new(area.x + x as u16, area.y, width as u16, 1),
                action,
            ));
        }
        spans.extend(section.spans.iter().cloned());
        x += width;
    }

    let trailing_start = (area.width as usize).saturating_sub(2 + trailing_width);
    let fill = trailing_start.saturating_sub(x);
    spans.push(Span::styled(" ".repeat(fill), border_style));
    x += fill;
    if let Some(section) = trailing {
        if let Some(action) = section.action {
            app.slot_actions.push((
                Rect::new(area.x + x as u16, area.y, trailing_width as u16, 1),
                action,
            ));
        }
        spans.extend(section.spans);
        x += trailing_width;
    }
    if x < area.width.saturating_sub(1) as usize {
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(if embedded { "│" } else { " " }, border_style));

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.panel)),
        area,
    );
}

fn draw_agent_rail(f: &mut Frame, app: &App, area: Rect, embedded: bool) {
    let theme = app.theme;
    let choosing = app.agent_selection.is_some();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        area,
    );
    let content = if embedded && area.width >= 2 {
        let border_style = Style::default().fg(theme.border).bg(theme.panel);
        f.buffer_mut()[(area.x, area.y)]
            .set_symbol("│")
            .set_style(border_style);
        f.buffer_mut()[(area.right().saturating_sub(1), area.y)]
            .set_symbol("│")
            .set_style(border_style);
        Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        )
    } else {
        area
    };
    if !choosing {
        let current = app
            .subagents
            .iter()
            .filter(|view| app.subagent_in_current_batch(&view.id))
            .collect::<Vec<_>>();
        let completed = current.iter().filter(|view| !view.running).count();
        let running = current.iter().any(|view| view.running);
        let failed = current.iter().any(|view| view.failed);
        let title_color = if failed {
            theme.err
        } else if running {
            theme.brand
        } else {
            theme.caption
        };
        let agents = app.locale.tr("Agents", "代理");
        let expand = app.locale.tr("↓ expand", "↓ 展开");
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(
                        "{}{agents} · {completed}/{}",
                        if embedded { "· " } else { " · " },
                        current.len(),
                    ),
                    Style::default().fg(title_color),
                ),
                Span::styled(format!("  {expand}"), Style::default().fg(theme.caption)),
            ]))
            .style(Style::default().bg(theme.panel)),
            content,
        );
        return;
    }
    let agents_label = app.locale.tr("Agents", "代理");
    let mut spans = vec![Span::styled(
        if embedded {
            format!("{agents_label}  ")
        } else {
            format!(" {agents_label}  ")
        },
        Style::default()
            .fg(if choosing { theme.brand } else { theme.caption })
            .add_modifier(if choosing {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
    )];
    let main_active = app.active_subagent.is_none();
    let main_focused = app.agent_selection.as_deref() == Some(app.session_id.as_str());
    spans.push(Span::styled(
        if main_focused || (main_active && !choosing) {
            "▸ main"
        } else if main_active {
            "• main"
        } else {
            "  main"
        },
        Style::default()
            .fg(if main_active {
                theme.brand
            } else {
                theme.fg_secondary
            })
            .add_modifier(if main_active || main_focused {
                Modifier::BOLD
            } else {
                Modifier::empty()
            })
            .add_modifier(if main_focused {
                Modifier::REVERSED
            } else {
                Modifier::empty()
            }),
    ));
    let history_count = app
        .subagents
        .iter()
        .filter(|view| !app.subagent_in_current_batch(&view.id))
        .count();
    for view in app
        .subagents
        .iter()
        .filter(|view| app.subagent_in_current_batch(&view.id))
    {
        let active = app.active_subagent.as_deref() == Some(view.id.as_str());
        let focused = app.agent_selection.as_deref() == Some(view.id.as_str());
        let marker = if view.running {
            app.spinner()
        } else if view.failed {
            '×'
        } else {
            '✓'
        };
        let modifiers = if active || focused {
            Modifier::BOLD
        } else {
            Modifier::empty()
        } | if focused {
            Modifier::REVERSED
        } else {
            Modifier::empty()
        };
        let label_color = if active || focused {
            theme.brand
        } else {
            theme.fg
        };
        spans.push(Span::styled(
            format!(
                "  {}",
                if focused || (active && !choosing) {
                    "▸ "
                } else if active {
                    "• "
                } else {
                    ""
                }
            ),
            Style::default().fg(label_color).add_modifier(modifiers),
        ));
        spans.push(Span::styled(
            format!("{marker} "),
            Style::default()
                .fg(if view.running {
                    theme.brand
                } else if view.failed {
                    theme.err
                } else {
                    theme.caption
                })
                .add_modifier(modifiers),
        ));
        spans.push(Span::styled(
            view.label.clone(),
            Style::default().fg(label_color).add_modifier(modifiers),
        ));
    }
    if history_count > 0 {
        let active = app
            .active_subagent
            .as_deref()
            .is_some_and(|id| !app.subagent_in_current_batch(id));
        let focused = app.agent_selection.as_deref() == Some(crate::app::AGENT_HISTORY_ID);
        spans.push(Span::styled(
            format!(
                "  {}{} ({history_count})",
                if focused || (active && !choosing) {
                    "▸ "
                } else if active {
                    "• "
                } else {
                    ""
                },
                app.locale.tr("History", "历史"),
            ),
            Style::default()
                .fg(if active || focused {
                    theme.brand
                } else {
                    theme.caption
                })
                .add_modifier(if active || focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                })
                .add_modifier(if focused {
                    Modifier::REVERSED
                } else {
                    Modifier::empty()
                }),
        ));
    }
    spans.push(Span::styled(
        format!(
            "  {}",
            app.locale.tr("←/→ · enter · esc close", "←/→ · enter · esc 关闭")
        ),
        Style::default().fg(theme.brand_soft),
    ));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.panel)),
        content,
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
/// input well. The amber prompt owns the working indicator now (the old
/// brand-blue edge bar glow is gone). `pet_pad` columns at the right stay
/// text-free for the whale pet. Tall enough terminals get
/// `draw_composer_box` instead — the same surface wrapped in the rounded
/// frame that also carries the cap row.
fn draw_composer(f: &mut Frame, app: &mut App, area: Rect, pet_pad: u16, navigation_dock_h: u16) {
    let theme = app.theme;

    // Surface fill: contrast against the chat bg does the framing.
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        area,
    );

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
    let well = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1 + navigation_dock_h),
    );
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
    // Reasoning owns its live `thinking…` row. Assistant text may be followed
    // by tool arguments that ACP cannot render until the call is complete, so
    // the ordinary working placeholder remains visible during that stream.
    if state == RunState::Running && transcript.reasoning_streaming() {
        return None;
    }
    match state {
        RunState::Idle => return None,
        RunState::Starting | RunState::Running => {
            let waiting_for_input = app.elicitation_ask.is_some();
            spans.push(Span::styled(
                if waiting_for_input {
                    "? ".to_string()
                } else {
                    format!("{} ", app.spinner())
                },
                Style::default().fg(if waiting_for_input {
                    theme.warn_soft()
                } else {
                    theme.brand
                }),
            ));
            let label = if waiting_for_input {
                app.locale.tr("waiting for input", "等待输入").to_string()
            } else if state == RunState::Starting {
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
            if active_child.is_none() && !waiting_for_input {
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
    let key_style = Style::default().fg(theme.fg_tertiary);
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
/// immediately. Idle+empty renders nothing (`/keys` lives in the tip
/// banner, not the dock).
fn context_hints(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let key = Style::default()
        .fg(theme.fg_tertiary)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::default().fg(theme.caption);
    let running = !matches!(app.state, RunState::Idle);
    let edit_queue_key = if cfg!(target_os = "macos") {
        "⌥↑"
    } else {
        "alt+↑"
    };
    let pairs: Vec<(&str, &str)> = if app.queue_delete_confirming() {
        vec![
            ("⏎", app.locale.tr("delete", "删除")),
            ("esc", app.locale.tr("back", "返回")),
        ]
    } else if app.queue_selecting() {
        vec![
            ("↑↓", app.locale.tr("choose", "选择")),
            ("⏎", app.locale.tr("edit", "编辑")),
            ("esc", app.locale.tr("close", "关闭")),
        ]
    } else if app.slash_completion_open() {
        vec![
            ("⏎", app.locale.tr("select", "选择")),
            ("esc", app.locale.tr("close", "关闭")),
        ]
    } else if app.queue_editing() {
        vec![
            ("⏎", app.locale.tr("save", "保存")),
            ("^d", app.locale.tr("delete", "删除")),
            ("esc", app.locale.tr("cancel", "取消")),
        ]
    } else {
        match (running, app.input.is_empty()) {
            // Working, nothing typed: stop it or edit the pending FIFO.
            (true, true) if app.queued > 0 => vec![
                (edit_queue_key, app.locale.tr("edit queue", "编辑队列")),
                ("esc", app.locale.tr("interrupt", "中断")),
            ],
            (true, true) => vec![("esc", app.locale.tr("interrupt", "中断"))],
            // Working with a draft: enter queues; ctrl+enter steers without
            // cancellation.
            (true, false) => vec![
                ("⏎", app.locale.tr("queue", "排队")),
                ("ctrl+⏎", app.locale.tr("steer", "立即转向")),
                ("esc", app.locale.tr("interrupt", "中断")),
            ],
            // Idle, empty: point at the FIFO editor; otherwise nothing.
            (false, true) if app.queued > 0 => {
                vec![(edit_queue_key, app.locale.tr("edit queue", "编辑队列"))]
            }
            (false, true) => vec![],
            // Idle with a draft: enter's meaning follows the prefix.
            (false, false) if app.input.lines()[0].starts_with('/') => {
                vec![("⏎", app.locale.tr("command", "命令"))]
            }
            (false, false) if app.input.lines()[0].starts_with('!') => {
                vec![("⏎", app.locale.tr("shell", "运行命令"))]
            }
            (false, false) => vec![("⏎", app.locale.tr("send", "发送"))],
        }
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
    if !pairs.is_empty() {
        spans.push(Span::styled(
            " · ".to_string(),
            Style::default().fg(theme.border),
        ));
    }
    spans
}

/// Meta row, right side: contextual shortcut hints, model chip (brand
/// accent), and the requested reasoning effort. Token flow lives in the
/// usage footer; the session id lives in the banner and `/session`.
fn status_right(app: &App) -> Vec<Span<'static>> {
    let theme = app.theme;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    // Vim mode indicator: -- INSERT -- / -- NORMAL --.
    if app.vim.is_active() {
        let (label, color) = match app.vim.mode {
            crate::input::VimMode::Insert => ("-- INSERT --", theme.caption),
            _ => ("-- NORMAL --", theme.brand),
        };
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
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

#[derive(Clone)]
struct CompactSlotSection {
    spans: Vec<Span<'static>>,
    action: Option<crate::slots::TuiAction>,
    essential: bool,
}

fn compact_slot_sections(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
    spinner: char,
) -> Vec<CompactSlotSection> {
    let mut sections = snapshot
        .nodes
        .iter()
        .filter_map(|node| {
            compact_node_spans(node, theme, width, spinner).map(|spans| CompactSlotSection {
                spans,
                action: node.action().cloned(),
                essential: false,
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

fn compact_input_dock_sections(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
    spinner: char,
) -> Vec<CompactSlotSection> {
    let mut sections = snapshot
        .nodes
        .iter()
        .filter(|node| matches!(node, crate::slots::TuiNode::Generic { .. }))
        .filter_map(|node| {
            compact_node_spans(node, theme, width, spinner).map(|spans| CompactSlotSection {
                spans,
                action: node.action().cloned(),
                essential: false,
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

fn input_dock_body_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let Some(snapshot) = app.slot_snapshots.get("conversation.input.dock") else {
        return Vec::new();
    };
    let nodes = snapshot
        .nodes
        .iter()
        .filter(|node| !matches!(node, crate::slots::TuiNode::Generic { .. }))
        .cloned()
        .collect::<Vec<_>>();
    crate::slots::render_nodes(&nodes, &app.theme, width)
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
        Paragraph::new(compact_slot_line(
            snapshot,
            &theme,
            inner.width as usize,
            app.spinner(),
        )),
        inner,
    );
}

fn compact_slot_line(
    snapshot: &crate::slots::SlotSnapshot,
    theme: &Theme,
    width: usize,
    spinner: char,
) -> Line<'static> {
    let sections = compact_slot_sections(snapshot, theme, width, spinner);
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

fn compact_input_dock_sections_line(
    sections: &[CompactSlotSection],
    theme: &Theme,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(theme.border)));
        }
        spans.extend(section.spans.iter().cloned());
    }
    Line::from(spans)
}

fn running_progress_color(theme: &Theme, spinner: char) -> ratatui::style::Color {
    let phase = crate::app::SPINNER
        .iter()
        .position(|frame| *frame == spinner)
        .unwrap_or(0);
    if phase < crate::app::SPINNER.len() / 2 {
        theme.brand_soft
    } else {
        theme.brand
    }
}

fn compact_node_spans(
    node: &crate::slots::TuiNode,
    theme: &Theme,
    width: usize,
    spinner: char,
) -> Option<Vec<Span<'static>>> {
    if let crate::slots::TuiNode::Generic {
        title,
        body,
        status,
        tone,
        selected,
        ..
    } = node
    {
        let mut spans = Vec::new();
        if let Some(status) = status.as_deref() {
            let (icon, color) = match status {
                "running" => (spinner.to_string(), theme.brand),
                "done" => ("✓".into(), theme.caption),
                "ok" => ("✓".into(), theme.ok),
                "err" => ("×".into(), theme.err),
                _ => ("◇".into(), theme.caption),
            };
            spans.push(Span::styled(format!("{icon} "), Style::default().fg(color)));
        }
        let title_color = tone
            .as_deref()
            .map(|name| crate::slots::token_color(theme, name))
            .unwrap_or(theme.fg_tertiary);
        let mut title_style = Style::default().fg(title_color);
        if tone.as_deref() == Some("brand") {
            title_style = title_style.add_modifier(Modifier::BOLD);
        }
        if *selected {
            if !body.is_empty() {
                return Some(vec![Span::styled(
                    format!(" {title} · {body} "),
                    title_style.add_modifier(Modifier::BOLD | Modifier::REVERSED),
                )]);
            }
            let status = status.as_deref().map(|status| match status {
                "running" => format!("{spinner} "),
                "done" => "✓ ".into(),
                "ok" => "✓ ".into(),
                "err" => "× ".into(),
                _ => "◇ ".into(),
            });
            return Some(vec![Span::styled(
                format!(" {}{} ", status.unwrap_or_default(), title),
                title_style.add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )]);
        }
        if !body.is_empty() {
            let body_color = match status.as_deref() {
                Some("running") => running_progress_color(theme, spinner),
                Some("done") => theme.caption,
                Some("ok") => theme.ok,
                Some("err") => theme.err,
                _ => title_color,
            };
            return Some(vec![
                Span::styled(title.clone(), title_style),
                Span::styled(" · ", Style::default().fg(theme.border)),
                Span::styled(body.clone(), Style::default().fg(body_color)),
            ]);
        }
        spans.push(Span::styled(title.clone(), title_style));
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
    let spinner = app.spinner();
    let layout = app
        .displayed_transcript_mut()
        .layout(&theme, inner.width, spinner, thumbs);
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
    let (start, end) = if app.scroll_up == 0 {
        app.chat_view.manual_top = None;
        (total.saturating_sub(h), total)
    } else if let Some(manual_top) = app.chat_view.manual_top {
        let start = manual_top.min(max_scroll);
        let end = start.saturating_add(h).min(total);
        app.scroll_up = total.saturating_sub(end);
        if app.scroll_up == 0 {
            app.chat_view.manual_top = None;
        }
        (start, end)
    } else {
        app.scroll_up = app.scroll_up.min(max_scroll);
        let end = total - app.scroll_up.min(total);
        let start = end.saturating_sub(h);
        app.chat_view.manual_top = Some(start);
        (start, end)
    };
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
    // Last: the selection overlay above also writes these cells, so the
    // full-rewrite marker must run after it (issue #38).
    force_full_rewrite(f, inner);
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

/// Parse the current branch from the workspace's `.git/HEAD` without
/// spawning git (":branch" rides after the project path in the composer
/// cap, colon-tight). `ref: refs/heads/<branch>` → Some(branch); a raw
/// commit hash (detached HEAD), a missing `.git`, or a missing HEAD →
/// None. Handles worktrees and submodules whose `.git` is a `gitdir:`
/// pointer file; relative pointers resolve against the `.git` file's
/// parent. The TUI workspace is a regular checkout, so GIT_DIR-style
/// layouts are not supported — git itself is never invoked.
pub fn head_branch(workspace: &str) -> Option<String> {
    let dot_git = std::path::Path::new(workspace).join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else if dot_git.is_file() {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let p = std::path::Path::new(pointer.strip_prefix("gitdir:")?.trim());
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            dot_git.parent()?.join(p)
        }
    } else {
        return None;
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.strip_prefix("ref: refs/heads/")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn workspace_cap_title(app: &App, area_width: usize) -> Line<'static> {
    let title_width = (area_width / 2).clamp(8, 64);
    let path_width = title_width.saturating_sub(4);
    let mut text = format!(" · {} ", compact_workspace(&app.cfg.workspace, path_width));
    // The git branch follows the project path, colon-tight ("path:branch"),
    // only when both still fit the cap budget — narrow terminals keep just
    // the path.
    if let Some(branch) = &app.git_branch {
        let branch_tag = format!(":{branch} ");
        if text.width() - 1 + branch_tag.width() <= title_width {
            text.pop(); // drop the space between the path and the colon
            text.push_str(&branch_tag);
        }
    }
    Line::from(Span::styled(text, Style::default().fg(app.theme.caption))).right_aligned()
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
fn draw_composer_box(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    input_dock_body_h: u16,
    pet_pad: u16,
    navigation_dock_h: u16,
) {
    let theme = app.theme;
    let has_input_dock = slot_has_nodes(app, "conversation.input.dock");
    let workspace = workspace_cap_title(app, area.width as usize);
    let workspace_width = span_widths(&workspace.spans);
    let title_budget = (area.width as usize).saturating_sub(2 + workspace_width + 1);
    let dock_sections = has_input_dock
        .then(|| app.slot_snapshots.get("conversation.input.dock"))
        .flatten()
        .map(|snapshot| compact_input_dock_sections(snapshot, &theme, title_budget, app.spinner()));
    let dock_line = dock_sections
        .as_ref()
        .map(|sections| compact_input_dock_sections_line(sections, &theme));
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
        let mut x = area.x.saturating_add(1);
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

    // The pet keeps clear of the text: content is inset from the right
    // border by pet_pad columns.
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width.saturating_sub(pet_pad),
        inner.height.saturating_sub(navigation_dock_h),
    );

    let dock_h = input_dock_body_h.min(content.height);
    if dock_h > 0 {
        let dock = Rect::new(content.x, content.y, content.width, dock_h);
        let lines = input_dock_body_lines(app, dock.width as usize);
        f.render_widget(Paragraph::new(lines), dock);
    }
    let input = Rect::new(
        content.x,
        content.y.saturating_add(dock_h),
        content.width,
        content.height.saturating_sub(dock_h),
    );

    // The input well follows any expanded dock rows; both remain inside the
    // same rounded card and the meta row stays on the bottom border.
    app.att_chips.clear();
    app.att_thumbs.clear();
    app.composer_wrap_width = input.width.saturating_sub("❯ ".width() as u16).max(1) as usize;
    draw_input(f, app, input);
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
        .unwrap_or_else(|| app.locale.tr("unknown size", "未知尺寸").into());
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

/// The caret rendered as buffer cells instead of the terminal's hardware
/// cursor: a steady reversed block that moves atomically with the frame
/// diff. The hardware cursor had to be hidden for the whole diff write and
/// re-shown at frame end, so any scroll-heavy redraw (streaming auto-follow,
/// user scrolling) held it off long enough to blink the input caret; as
/// buffer cells it can never flicker or teleport, and the terminal cursor
/// stays hidden for the whole session. Wide graphemes cover both cells.
fn paint_caret(f: &mut Frame, x: u16, y: u16) {
    let buf = f.buffer_mut();
    let style = Style::default().add_modifier(Modifier::REVERSED);
    let Some(cell) = buf.cell_mut((x, y)) else {
        return;
    };
    let wide = UnicodeWidthStr::width(cell.symbol()) > 1;
    cell.set_style(style);
    if wide {
        if let Some(cell) = buf.cell_mut((x + 1, y)) {
            cell.set_style(style);
        }
    }
}

/// The input well. Long prompts wrap across the (now taller) well, and the
/// cursor follows the wrap; `/` and `!` prefixes recolor the whole line.
/// Inline `[image n]` tokens render as chips (no icon — the token itself is
/// the chip) and their screen rects land in `app.att_chips` for hover.
///
/// The draft itself is a `ratatui-textarea` widget rendered in the columns
/// right of the prompt; the wrap layout mirror (`ComposerEditor::layout`)
/// drives chip restyling, the drag-selection highlight, and the caret cell,
/// and `ComposerEditor::update_scroll_top` keeps the mirrored
/// scroll offset in lockstep with the widget's own viewport.
fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let composer_owns_cursor = app.elicitation_ask.is_none();
    if area.width < 4 || area.height == 0 {
        return;
    }
    // Mouse hit-testing for clicks/drags in the well (empty draft: the
    // whole well maps to boundary 0).
    app.input_area = area;
    let prompt = "❯ ";
    let pw = prompt.width();
    // The prompt doubles as the working indicator that used to be the brand
    // glow bar: amber while working, brand blue when idle (issue #27).
    let working = !matches!(app.state, RunState::Idle);
    let prompt_style = Style::default()
        .fg(if working { theme.warn } else { theme.brand })
        .add_modifier(Modifier::BOLD);

    if app.input.is_empty() {
        let placeholder = match app.state {
            RunState::Idle => app
                .locale
                .tr("describe what you want to build…", "描述你想构建的内容…")
                .to_string(),
            _ => app
                .locale
                .tr(
                    "queue a follow-up — ctrl+enter steers now",
                    "输入后续消息 — ctrl+enter 立即 steer",
                )
                .to_string(),
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prompt.to_string(), prompt_style),
                Span::styled(
                    placeholder.to_string(),
                    Style::default()
                        .fg(theme.caption)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])),
            area,
        );
        if composer_owns_cursor {
            paint_caret(f, area.x + pw as u16, area.y);
            app.caret_cell = Some((area.x + pw as u16, area.y));
        }
        return;
    }

    let buf = app.input.buf();
    let style = if buf.starts_with('!') {
        Style::default().fg(theme.warn_soft())
    } else if buf.starts_with('/') {
        Style::default().fg(theme.brand_soft)
    } else {
        Style::default().fg(theme.fg)
    };
    let avail = (area.width as usize).saturating_sub(pw).max(1);

    // Prompt column: "❯ " on the first row, blank continuation rows keep
    // the wrapped draft aligned with the first line.
    {
        let b = f.buffer_mut();
        b.set_string(area.x, area.y, prompt, prompt_style);
        for i in 1..area.height {
            b.set_string(area.x, area.y + i, " ".repeat(pw), Style::default());
        }
    }

    // The widget renders into the columns right of the prompt; its wrap
    // width is exactly `avail`. The caret is the widget's own reversed
    // cursor cell: it rides the frame diff like any other cell, so scroll
    // redraws can never blink it (the old hardware cursor was hidden for
    // the whole diff write). While an overlay owns input, the composer
    // shows no caret at all.
    let text_area = Rect::new(area.x + pw as u16, area.y, area.width - pw as u16, area.height);
    {
        let ta = app.input.textarea_mut();
        ta.set_style(style);
        ta.set_cursor_style(if composer_owns_cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        });
        f.render_widget(&*ta, text_area);
    }

    // Chip restyle + drag-selection highlight share the layout mirror:
    // both are buffer patches on top of the widget's own rendering. The
    // layout borrows `app.input`, so the char spans are computed first.
    let spans = app.token_spans();
    let sel_range = app.input_selection_range();
    let chip_style = Style::default()
        .fg(theme.bubble_fg)
        .bg(theme.bubble_bg)
        .add_modifier(Modifier::BOLD);
    let top = app.input_top;
    let h = area.height as usize;
    let mut chip_rects: Vec<(Rect, usize)> = Vec::new();
    {
        let layout = app.input.layout(avail);
        let buf = f.buffer_mut();
        for &(cs, ce, idx) in &spans {
            // Contiguous grapheme runs per screen row.
            let mut runs: Vec<(usize, usize, usize)> = Vec::new(); // (row, col0, col1)
            let mut run: Option<(usize, usize, usize)> = None;
            for (row, r) in layout.rows.iter().enumerate() {
                for g in &r.graphemes {
                    if g.start_char < ce && g.end_char > cs {
                        match &mut run {
                            Some((r0, c0, c1)) if *r0 == row => *c1 = g.start_col + g.width,
                            _ => {
                                if let Some((r0, c0, c1)) = run.take() {
                                    runs.push((r0, c0, c1));
                                }
                                run = Some((row, g.start_col, g.start_col + g.width));
                            }
                        }
                    } else if run.is_some() {
                        runs.push(run.take().expect("run"));
                    }
                }
                if let Some((r0, c0, c1)) = run.take() {
                    runs.push((r0, c0, c1));
                }
            }
            for (row, c0, c1) in runs {
                if row < top || row >= top + h {
                    continue;
                }
                let y = area.y + (row - top) as u16;
                for c in c0..c1 {
                    if let Some(cell) = buf.cell_mut((text_area.x + c as u16, y)) {
                        cell.set_style(chip_style);
                    }
                }
                chip_rects.push((
                    Rect::new(
                        text_area.x + c0 as u16,
                        y,
                        (c1 - c0) as u16,
                        1,
                    ),
                    idx,
                ));
            }
        }
        // Drag selection: reversed cells over the covered graphemes — the
        // same treatment as the chat pane highlight.
        if let Some((a, b)) = sel_range {
            for (row, r) in layout.rows.iter().enumerate() {
                if row < top || row >= top + h {
                    continue;
                }
                let y = area.y + (row - top) as u16;
                for g in &r.graphemes {
                    if g.start_char < b && g.end_char > a {
                        for c in g.start_col..g.start_col + g.width {
                            if let Some(cell) = buf.cell_mut((text_area.x + c as u16, y)) {
                                cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                            }
                        }
                    }
                }
            }
        }
    }
    app.att_chips = chip_rects;

    app.input.update_scroll_top(area.height);
    // `input_top` is the app-side mirror mouse hit-testing reads between
    // frames; keep it in lockstep with the editor's own scroll mirror.
    app.input_top = app.input.scroll_top;
    // Track the painted caret cell so `main` can park the hidden hardware
    // cursor on it after the frame (IME popups anchor there; the frame diff
    // leaves the cursor wherever its last cell write happened). The widget's
    // own screen cursor is authoritative — it is the cell the reversed
    // caret style landed on.
    if composer_owns_cursor {
        let (row, col) = app.input.screen_cursor();
        let top = app.input.scroll_top;
        if row >= top && row - top < area.height as usize {
            app.caret_cell = Some((
                text_area.x.saturating_add(col as u16),
                text_area.y.saturating_add((row - top) as u16),
            ));
        }
    }
}

/// Rows of menu items shown at once; the window follows the selection
/// (↑/↓ wrap) and the bottom title shows the position when clipped.
const SLASH_MENU_ROWS: usize = 12;

fn draw_slash_menu(f: &mut Frame, app: &App, input: Rect, chat: Rect) {
    if !app.slash_completion_open() {
        return;
    }
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
    // The popup floats over the transcript: scrolling it with ↑/↓ moves
    // wide (CJK) glyphs and can orphan their trailing halves on the real
    // screen while both ratatui buffers agree the cell is unchanged, so
    // the diff skips it and the leftover half-glyph stays as a black block
    // (issue #83 — same class as the #38 transcript fix). Force a full
    // rewrite of the popup every frame to flush any stale terminal cells.
    force_full_rewrite(f, area);
}

/// Rows of the `@file` browser shown at once; the explorer's own List
/// scrolls the selection within this window.
const FILE_MENU_ROWS: usize = 12;

fn draw_file_menu(f: &mut Frame, app: &mut App, input: Rect, chat: Rect) {
    let Some(menu) = &mut app.file_menu else {
        return;
    };
    if menu.explorer().files().is_empty() {
        return;
    }
    // Rebuild the explorer chrome from the app theme every frame: palette
    // packs and locale can switch under us, and the title factories need
    // the current cwd.
    let theme = app.theme;
    let locale = app.locale;
    menu.apply_chrome(&theme, locale, &app.cfg.workspace);
    let n = menu.explorer().files().len();
    let vis = FILE_MENU_ROWS
        .min(n)
        .min(chat.height.saturating_sub(2) as usize);
    if vis == 0 {
        return;
    }
    // Wider than the slash menu — entries carry paths; the List truncates
    // long names instead of hard-clipping.
    let h = vis as u16 + 2;
    let w = 72.min(input.width.saturating_sub(2));
    let y = input.y.saturating_sub(h);
    let area = Rect::new(input.x + 2, y, w, h);
    f.render_widget(Clear, area);
    f.render_widget_ref(menu.explorer().widget(), area);
    // Same class as #83 / #38: CJK file names scroll through the explorer
    // window and orphan wide-char trailing cells on the real screen; force
    // a full rewrite so the diff never trusts a half-erased cell.
    force_full_rewrite(f, area);
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
        | crate::app::PickerKind::Auth
        | crate::app::PickerKind::CordisPlugin
        | crate::app::PickerKind::CordisApproval
        | crate::app::PickerKind::AgentHistory => false,
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
    let mut field_cursor = None;

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
                    let cursor_row = bottom.len();
                    let cursor_col = "      ❯ ".width()
                        + state
                            .input
                            .buf
                            .chars()
                            .take(state.input.cursor)
                            .collect::<String>()
                            .width();
                    bottom.push(Line::from(vec![
                        Span::styled("      ❯ ", Style::default().fg(theme.brand)),
                        Span::styled(state.input.buf.clone(), Style::default().fg(theme.fg)),
                    ]));
                    field_cursor = Some((cursor_row, cursor_col));
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
            let cursor_row = bottom.len();
            let cursor_col = "❯ ".width()
                + state
                    .input
                    .buf
                    .chars()
                    .take(state.input.cursor)
                    .collect::<String>()
                    .width();
            bottom.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(theme.brand)),
                Span::styled(state.input.buf.clone(), Style::default().fg(theme.fg)),
            ]));
            field_cursor = Some((cursor_row, cursor_col));
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
    let bottom_area = Rect::new(
        inner.x,
        inner.y.saturating_add((top_h + middle_h) as u16),
        inner.width,
        bottom_h as u16,
    );
    f.render_widget(Paragraph::new(bottom), bottom_area);
    if let Some((row, col)) = field_cursor.filter(|(row, _)| *row < bottom_h) {
        let caret = (
            bottom_area
                .x
                .saturating_add(col as u16)
                .min(bottom_area.right().saturating_sub(1)),
            bottom_area.y.saturating_add(row as u16),
        );
        paint_caret(f, caret.0, caret.1);
        app.caret_cell = Some(caret);
    }
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
        format!("martty {}", env!("CARGO_PKG_VERSION")),
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

#[cfg(test)]
#[path = "../tests/unit/ui_diff_option__tests.rs"]
mod diff_option_tests;
