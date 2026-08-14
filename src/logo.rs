//! The DeepSeek whale + wordmark, as terminal art.
//!
//! The whale variants in [`crate::logo_data`] are rasterized by
//! `scripts/gen_logo.py` from the exact favicon SVG served by the
//! deepseek-harness Web UI. The wordmark is figlet `ansi_shadow`, tinted
//! like the whale's belly; wide terminals also get a hollow `HARNESS`.

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

/// figlet `ansi_shadow` `HARNESS`, 58 columns. Rendered hollow (镂空) via
/// [`hollow`] next to the solid wordmark when the terminal is wide enough.
pub const WORDMARK_HARNESS: [&str; 6] = [
    "██╗  ██╗ █████╗ ██████╗ ███╗   ██╗███████╗███████╗███████╗",
    "██║  ██║██╔══██╗██╔══██╗████╗  ██║██╔════╝██╔════╝██╔════╝",
    "███████║███████║██████╔╝██╔██╗ ██║█████╗  ███████╗███████╗",
    "██╔══██║██╔══██║██╔══██╗██║╚██╗██║██╔══╝  ╚════██║╚════██║",
    "██║  ██║██║  ██║██║  ██║██║ ╚████║███████╗███████║███████║",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝╚══════╝╚══════╝╚══════╝",
];

/// Hollow (镂空) take on a solid `ansi_shadow` row: the fill drops to light
/// shade while the box-drawing shadow strokes remain as the outline.
fn hollow(row: &str) -> String {
    row.replace('█', "░")
}

/// Split a hollow row into styled runs: the box-drawing strokes get the
/// bright `edge` style (白边), the shade fill stays muted (空心).
fn hollow_spans(row: &str, edge: Style, fill: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_is_edge = false;
    for c in hollow(row).chars() {
        let is_edge = !matches!(c, '░' | ' ');
        if !run.is_empty() && is_edge != run_is_edge {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                if run_is_edge { edge } else { fill },
            ));
        }
        run.push(c);
        run_is_edge = is_edge;
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_is_edge { edge } else { fill }));
    }
    spans
}

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

/// `DEEPSEEK HARNESS` wordmark, centered to `width`. `DEEPSEEK` is tinted
/// like the whale's belly (the gradient's bottom stop); `HARNESS` rides
/// alongside as white-edged hollow block art when the terminal is wide
/// enough, and degrades to a letter-spaced caption / plain text otherwise.
pub fn wordmark_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let (_, belly) = theme.whale_gradient();
    let ink = Style::default().fg(belly);
    let mut out = Vec::new();

    // Wide: solid DEEPSEEK + hollow HARNESS on one line (64 + 2 + 58 cols).
    if width >= 126 {
        let pad = (width as usize).saturating_sub(124) / 2;
        let edge = Style::default().fg(theme.fg);
        for (deepseek, harness) in WORDMARK.iter().zip(WORDMARK_HARNESS.iter()) {
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
        // Tiny: plain text; "hollow" HARNESS is simply not bold.
        let show_harness = width >= 17;
        let text_width = if show_harness { 16 } else { 8 };
        let pad = (width as usize).saturating_sub(text_width) / 2;
        let mut spans = vec![
            Span::raw(" ".repeat(pad)),
            Span::styled("DEEPSEEK".to_string(), ink.add_modifier(Modifier::BOLD)),
        ];
        if show_harness {
            spans.push(Span::styled(" HARNESS".to_string(), ink));
        }
        out.push(Line::from(spans));
        return out;
    };
    let pad = (width as usize).saturating_sub(art_width) / 2;
    for row in rows {
        out.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled((*row).to_string(), ink),
        ]));
    }
    // No room for the hollow block art — letter-spaced HARNESS caption.
    let pad = (width as usize).saturating_sub(caption.chars().count()) / 2;
    out.push(Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(caption.to_string(), ink),
    ]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Both block wordmarks are rectangular (every row the same width),
    /// so the side-by-side composition can't shear.
    #[test]
    fn wordmark_rows_align() {
        for row in WORDMARK {
            assert_eq!(row.chars().count(), 64, "{row}");
        }
        for row in WORDMARK_HARNESS {
            assert_eq!(row.chars().count(), 58, "{row}");
        }
    }

    /// The hollow transform keeps geometry: same width, no solid blocks.
    #[test]
    fn hollow_is_hollow() {
        for row in WORDMARK_HARNESS {
            let h = hollow(row);
            assert_eq!(h.chars().count(), row.chars().count());
            assert!(!h.contains('█'));
            // Fill turns to light shade (the last row is pure outline).
            assert_eq!(h.contains('░'), row.contains('█'));
        }
        assert!(hollow(WORDMARK_HARNESS[0]).contains('░'));
    }

    /// DEEPSEEK is tinted like the whale's belly (gradient bottom stop),
    /// at every breakpoint, in both themes.
    #[test]
    fn wordmark_matches_whale_belly() {
        for theme in [Theme::dark(), Theme::light()] {
            let (_, belly) = theme.whale_gradient();
            for width in [16u16, 24, 50, 80] {
                for line in wordmark_lines(&theme, width) {
                    for span in line.spans.iter().filter(|s| !s.content.trim().is_empty()) {
                        assert_eq!(
                            span.style.fg,
                            Some(belly),
                            "width {width}: {:?}",
                            span.content
                        );
                    }
                }
            }
        }
    }

    /// Wide terminals get `DEEPSEEK` + hollow `HARNESS` on one line;
    /// narrower ones still surface HARNESS as a caption / plain text.
    #[test]
    fn harness_shows_when_space_allows() {
        let theme = Theme::dark();

        let wide = wordmark_lines(&theme, 140);
        assert_eq!(wide.len(), 6);
        let (_, belly) = theme.whale_gradient();
        // Solid DEEPSEEK and hollow HARNESS share every row (the final
        // row is shadow-only outline in both, hence `take(5)`).
        for line in wide.iter().take(5) {
            let row = text(line);
            assert!(row.contains('█') && row.contains('░'), "{row}");
            // DEEPSEEK stays whale-belly gray…
            assert_eq!(line.spans[1].style.fg, Some(belly));
            // …while HARNESS strokes are bright (白边) and the fill muted.
            assert!(line.spans.iter().any(|s| {
                s.style.fg == Some(theme.fg) && s.content.chars().all(|c| "╔╗╚╝═║".contains(c))
            }));
            assert!(line
                .spans
                .iter()
                .filter(|s| s.content.contains('░'))
                .all(|s| s.style.fg == Some(belly)));
        }

        let medium = wordmark_lines(&theme, 80);
        assert_eq!(
            text(medium.last().unwrap()).trim(),
            "H   A   R   N   E   S   S"
        );

        let small = wordmark_lines(&theme, 40);
        assert_eq!(text(small.last().unwrap()).trim(), "H A R N E S S");

        let tiny = wordmark_lines(&theme, 20);
        assert_eq!(text(&tiny[0]).trim(), "DEEPSEEK HARNESS");
    }
}
