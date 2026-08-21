//! Classic DeepSeek Harness welcome mark.
//!
//! Geometry and breakpoints intentionally match the pre-Martty renderer.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::{lerp, Theme};

const WHALE_XL: [&str; 19] = [
    "                     ▄▄▄███▀       ██",
    "        ▄▄████████████████        ▄███▄▄         ▄█",
    "      ▄██████████████████▄       ██████▄ ▄▄▄▄████",
    "    ▄██████████████████████▄     ███████████████▀",
    "   ████████████████████████████▄    ▀████████████▀",
    "  ██████████████████████████████▄    ▀████████▀",
    " ▄███▀▀▀▀▀▀██████████████████████████▄▄ █████▀",
    " ████         ▀▀███████████▀ ▀▀████████████",
    " ████            ▀▀█████████▀█  ▀██████████",
    " ████               ▀█████████    ████████▀",
    " ▀████               ▀█████████▄▄▄████████",
    "  █████                ▀█████████████████",
    "  ▀█████                ▀███████████████",
    "   ▀█████▄       ▄▄▄      ████████████▀",
    "     ██████▄     ▀████▄    ▀████████▀",
    "      ▀███████▄▄▄▄██████▄▄   ▀████████▄▄",
    "        ▀▀██████████████████▄▄▄█▀███████▀",
    "           ▀▀██████████████████▀▀",
    "                ▀▀▀▀▀▀▀▀▀▀",
];

const WHALE_LG: [&str; 15] = [
    "          ▄▄▄▄ ▄▄▄███      █▄",
    "     ▄▄█████████████       ███▄▄     ▄█",
    "   ▄█████████████████▄▄    █████▄██████",
    "  ▄█████████████████████▄  ▀██████████",
    " ▄████████████████████████▄  ▀████▀▀▀",
    " ██▀    ▀▀▀██████████▀▀█████▄████",
    " ███         ▀███████▀▄ ▀████████",
    " ███           ▀███████   ██████▀",
    " ████            ▀██████████████",
    "  ████            ▀████████████",
    "   ████▄     ▄▄▄    █████████▀",
    "    ▀████▄    ███▄▄  ▀██████▄",
    "      ▀█████████████▄▄▄████████",
    "        ▀▀████████████▀▀",
    "             ▀▀▀▀▀▀",
];

const WHALE_MD: [&str; 10] = [
    "    ▄▄▄▄▄▄▄██▀   ██▄     ▄",
    "  ▄███████████▄  ▀███▄███▀",
    " ███████████████▄ ▀█████▀",
    " █▀ ▀▀▀███████▀███▄███",
    "▀█▄      ▀█████ ▀████",
    " ██        ▀████▄███▀",
    " ▀██▄    ▄  ▀██████▀",
    "   ███▄  ██▄  ▀███▄▄",
    "    ▀▀█████████▀▀▀▀▀",
    "         ▀▀▀",
];

const WHALE_SM: [&str; 6] = [
    "   ▄▄▄▄▄   ▄",
    " ████████▄ ████▀",
    "█▀ ▀▀████▀█▄█",
    "▀█    ▀██▄███",
    " ▀█▄  ▄ ▀██▀",
    "   ▀█████▀▀▀",
];

const WORDMARK: [&str; 6] = [
    "██████╗ ███████╗███████╗██████╗ ███████╗███████╗███████╗██╗  ██╗",
    "██╔══██╗██╔════╝██╔════╝██╔══██╗██╔════╝██╔════╝██╔════╝██║ ██╔╝",
    "██║  ██║█████╗  █████╗  ██████╔╝███████╗█████╗  █████╗  █████╔╝ ",
    "██║  ██║██╔══╝  ██╔══╝  ██╔═══╝ ╚════██║██╔══╝  ██╔══╝  ██╔═██╗ ",
    "██████╔╝███████╗███████╗██║     ███████║███████╗███████╗██║  ██╗",
    "╚═════╝ ╚══════╝╚══════╝╚═╝     ╚══════╝╚══════╝╚══════╝╚═╝  ╚═╝",
];

const HARNESS: [&str; 6] = [
    "██╗  ██╗ █████╗ ██████╗ ███╗   ██╗███████╗███████╗███████╗",
    "██║  ██║██╔══██╗██╔══██╗████╗  ██║██╔════╝██╔════╝██╔════╝",
    "███████║███████║██████╔╝██╔██╗ ██║█████╗  ███████╗███████╗",
    "██╔══██║██╔══██║██╔══██╗██║╚██╗██║██╔══╝  ╚════██║╚════██║",
    "██║  ██║██║  ██║██║  ██║██║ ╚████║███████╗███████║███████║",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝╚══════╝╚══════╝╚══════╝",
];

const WORDMARK_SMALL: [&str; 4] = [
    " ___  ___ ___ ___  ___ ___ ___ _  __",
    "|   \\| __| __| _ \\/ __| __| __| |/ /",
    "| |) | _|| _||  _/\\__ \\ _|| _|| ' < ",
    "|___/|___|___|_|  |___/___|___|_|\\_\\",
];

fn whale_for_width(width: u16) -> &'static [&'static str] {
    match width {
        0..=25 => &WHALE_SM,
        26..=45 => &WHALE_MD,
        46..=63 => &WHALE_LG,
        _ => &WHALE_XL,
    }
}

fn whale_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let art = whale_for_width(width);
    let (top, bottom) = theme.whale_gradient();
    let rows = art.len().max(1);
    let art_width = art.iter().map(|row| row.chars().count()).max().unwrap_or(0);
    let pad = (width as usize).saturating_sub(art_width) / 2;
    art.iter()
        .enumerate()
        .map(|(index, row)| {
            let t = index as f32 / (rows.saturating_sub(1).max(1)) as f32;
            Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled(
                    (*row).to_string(),
                    Style::default().fg(lerp(top, bottom, t)),
                ),
            ])
        })
        .collect()
}

fn hollow(row: &str) -> String {
    row.replace('█', "░")
}

fn hollow_spans(row: &str, edge: Style, fill: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_is_edge = false;
    for character in hollow(row).chars() {
        let is_edge = !matches!(character, '░' | ' ');
        if !run.is_empty() && is_edge != run_is_edge {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                if run_is_edge { edge } else { fill },
            ));
        }
        run.push(character);
        run_is_edge = is_edge;
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_is_edge { edge } else { fill }));
    }
    spans
}

fn wordmark_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let (_, belly) = theme.whale_gradient();
    let ink = Style::default().fg(belly);
    let mut out = Vec::new();
    if width >= 126 {
        let pad = (width as usize).saturating_sub(124) / 2;
        let edge = Style::default().fg(theme.fg);
        for (deepseek, harness) in WORDMARK.iter().zip(HARNESS.iter()) {
            let mut spans = vec![
                Span::raw(" ".repeat(pad)),
                Span::styled((*deepseek).to_string(), ink),
                Span::raw("  "),
            ];
            spans.extend(hollow_spans(harness, edge, ink));
            out.push(Line::from(spans));
        }
        return out;
    }
    let (rows, art_width, caption): (&[&str], usize, &str) = if width >= 66 {
        (&WORDMARK, 64, "H   A   R   N   E   S   S")
    } else if width >= 38 {
        (&WORDMARK_SMALL, 36, "H A R N E S S")
    } else {
        let show_harness = width >= 17;
        let text_width = if show_harness { 16 } else { 8 };
        let pad = (width as usize).saturating_sub(text_width) / 2;
        let mut spans = vec![
            Span::raw(" ".repeat(pad)),
            Span::styled("DEEPSEEK", ink.add_modifier(Modifier::BOLD)),
        ];
        if show_harness {
            spans.push(Span::styled(" HARNESS", ink));
        }
        return vec![Line::from(spans)];
    };
    let pad = (width as usize).saturating_sub(art_width) / 2;
    for row in rows {
        out.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled((*row).to_string(), ink),
        ]));
    }
    let pad = (width as usize).saturating_sub(caption.chars().count()) / 2;
    out.push(Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(caption, ink),
    ]));
    out
}

/// Responsive logo portion of the classic hero (tagline is a separate slot node).
pub fn lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let mut out = whale_lines(theme, width);
    out.push(Line::default());
    out.extend(wordmark_lines(theme, width));
    out
}
