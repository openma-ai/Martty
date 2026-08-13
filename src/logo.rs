//! The DeepSeek whale + wordmark, as terminal art.
//!
//! The whale variants in [`crate::logo_data`] are rasterized by
//! `scripts/gen_logo.py` from the exact favicon SVG served by the
//! deepseek-harness Web UI. The wordmark is figlet `ansi_shadow`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::logo_data::{WHALE_LG, WHALE_MD, WHALE_SM, WHALE_XL};
use crate::theme::{lerp, Theme};

/// figlet `ansi_shadow`, 64 columns.
pub const WORDMARK: [&str; 6] = [
    "██████╗ ███████╗███████╗██████╗ ███████╗███████╗███████╗██╗  ██╗",
    "██╔══██╗██╔════╝██╔════╝██╔══██╗██╔════╝██╔════╝██╔════╝██║ ██╔╝",
    "██║  ██║█████╗  █████╗  ██████╔╝███████╗█████╗  █████╗  █████╔╝ ",
    "██║  ██║██╔══╝  ██╔══╝  ██╔═══╝ ╚════██║██╔══╝  ██╔══╝  ██╔═██╗ ",
    "██████╔╝███████╗███████╗██║     ███████║███████╗███████╗██║  ██╗",
    "╚═════╝ ╚══════╝╚══════╝╚═╝     ╚══════╝╚══════╝╚══════╝╚═╝  ╚═╝",
];

/// Compact wordmark for narrow terminals (figlet `small`).
pub const WORDMARK_SMALL: [&str; 4] = [
    " ___  ___ ___ ___  ___ ___ ___ _  __",
    "|   \\| __| __| _ \\/ __| __| __| |/ /",
    "| |) | _|| _||  _/\\__ \\ _|| _|| ' < ",
    "|___/|___|___|_|  |___/___|___|_|\\_\\",
];

/// Pick a whale size that fits `width` columns.
pub fn whale_for_width(width: u16) -> &'static [&'static str] {
    match width {
        0..=25 => &WHALE_SM,
        26..=45 => &WHALE_MD,
        46..=63 => &WHALE_LG,
        _ => &WHALE_XL,
    }
}

/// Whale rows with the brand vertical gradient, centered to `width`.
pub fn whale_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let art = whale_for_width(width);
    let (top, bottom) = theme.whale_gradient();
    let rows = art.len().max(1);
    let art_width = art.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let pad = (width as usize).saturating_sub(art_width) / 2;
    art.iter()
        .enumerate()
        .map(|(i, row)| {
            let t = i as f32 / (rows.saturating_sub(1).max(1)) as f32;
            let color = lerp(top, bottom, t);
            Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled((*row).to_string(), Style::default().fg(color)),
            ])
        })
        .collect()
}

/// `DEEPSEEK` wordmark + `BUILD` tag, centered to `width`.
pub fn wordmark_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let (rows, art_width): (&[&str], usize) = if width >= 66 {
        (&WORDMARK, 64)
    } else if width >= 38 {
        (&WORDMARK_SMALL, 36)
    } else {
        let pad = (width as usize).saturating_sub(14) / 2;
        out.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(
                "DEEPSEEK".to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" BUILD".to_string(), Style::default().fg(theme.brand)),
        ]));
        return out;
    };
    let pad = (width as usize).saturating_sub(art_width) / 2;
    for row in rows {
        out.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled((*row).to_string(), Style::default().fg(theme.fg)),
        ]));
    }
    out
}
