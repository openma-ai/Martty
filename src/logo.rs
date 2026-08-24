//! The Martty lockup as terminal art.
//!
//! `MARTTY` is figlet `ansi_shadow` (the same font family the old
//! DEEPSEEK wordmark used). `MAR` carries a blue-white ocean gradient;
//! `TTY` uses the terminal foreground (white in dark mode, black in light
//! mode). Wide terminals get the block logo, narrower ones degrade to the
//! `small` figlet variant, then to plain bold text.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::{lerp, Mode, Theme, DEEPSEEK_200, DEEPSEEK_50};

/// figlet `ansi_shadow`, 54 columns.
pub const MARTTY: [&str; 6] = [
    "███╗   ███╗ █████╗ ██████╗ ████████╗████████╗██╗   ██╗",
    "████╗ ████║██╔══██╗██╔══██╗╚══██╔══╝╚══██╔══╝╚██╗ ██╔╝",
    "██╔████╔██║███████║██████╔╝   ██║      ██║    ╚████╔╝ ",
    "██║╚██╔╝██║██╔══██║██╔══██╗   ██║      ██║     ╚██╔╝  ",
    "██║ ╚═╝ ██║██║  ██║██║  ██║   ██║      ██║      ██║   ",
    "╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝      ╚═╝      ╚═╝   ",
];

/// figlet `small`, 30 columns — mixed-case "Martty" for narrower terminals.
pub const MARTTY_SMALL: [&str; 5] = [
    " __  __          _   _        ",
    "|  \\/  |__ _ _ _| |_| |_ _  _ ",
    "| |\\/| / _` | '_|  _|  _| || |",
    "|_|  |_\\__,_|_|  \\__|\\__|\\_, |",
    "                         |__/ ",
];

fn split_row(row: &str, at: usize) -> (String, String) {
    let mut chars = row.chars();
    let mar = chars.by_ref().take(at).collect();
    let tty = chars.collect();
    (mar, tty)
}

fn split_logo_lines(
    rows: &[&str],
    split: usize,
    art_width: usize,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let pad = (width as usize).saturating_sub(art_width) / 2;
    let (ocean_top, ocean_bottom) = match theme.mode {
        Mode::Dark => (DEEPSEEK_50, theme.brand),
        Mode::Light => (theme.brand, DEEPSEEK_200),
    };
    let last = rows.len().saturating_sub(1).max(1);
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let (mar, tty) = split_row(row, split);
            let ocean = lerp(ocean_top, ocean_bottom, row_index as f32 / last as f32);
            Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled(mar, Style::default().fg(ocean)),
                Span::styled(tty, Style::default().fg(theme.fg)),
            ])
        })
        .collect()
}

/// Martty logo rows, split into ocean-gradient `MAR` and terminal-ink `TTY`,
/// centered to `width`. Wide terminals get the block `ansi_shadow` lockup;
/// narrower ones degrade to the small figlet variant, then plain bold text.
pub fn martty_logo_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    if width < 56 {
        if width >= 34 {
            return split_logo_lines(&MARTTY_SMALL, 16, 30, theme, width);
        }
        if width >= 8 {
            let pad = (width as usize).saturating_sub(8) / 2;
            return vec![Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled(
                    "MAR".to_string(),
                    Style::default()
                        .fg(theme.brand)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "TTY".to_string(),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
            ])];
        }
        return Vec::new();
    }
    split_logo_lines(&MARTTY, 27, 54, theme, width)
}

#[cfg(test)]
#[path = "../tests/unit/logo__tests.rs"]
mod tests;
