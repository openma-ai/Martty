//! The Martty lockup as terminal art.
//!
//! `MARTTY` is figlet `ansi_shadow` (the same font family the old
//! DEEPSEEK wordmark used), tinted with the whale-belly tone so the
//! banner keeps its brand gradient; wide terminals get the block logo,
//! narrower ones degrade to the `small` figlet variant, then to plain
//! bold text.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

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

/// Martty logo rows, belly-tinted and centered to `width`. Wide terminals
/// get the block `ansi_shadow` lockup; narrower ones degrade to the small
/// figlet variant, then to plain bold text.
pub fn martty_logo_lines(theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let (_, belly) = theme.whale_gradient();
    let ink = Style::default().fg(belly);
    if width < 56 {
        if width >= 34 {
            let pad = (width as usize).saturating_sub(30) / 2;
            return MARTTY_SMALL
                .iter()
                .map(|row| {
                    Line::from(vec![
                        Span::raw(" ".repeat(pad)),
                        Span::styled((*row).to_string(), ink),
                    ])
                })
                .collect();
        }
        if width >= 8 {
            let pad = (width as usize).saturating_sub(8) / 2;
            return vec![Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled("MARTTY".to_string(), ink.add_modifier(Modifier::BOLD)),
            ])];
        }
        return Vec::new();
    }
    let pad = (width as usize).saturating_sub(54) / 2;
    MARTTY
        .iter()
        .map(|row| {
            Line::from(vec![
                Span::raw(" ".repeat(pad)),
                Span::styled((*row).to_string(), ink),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Both logo variants are rectangular, so centering can't shear.
    #[test]
    fn martty_rows_align() {
        for row in MARTTY {
            assert_eq!(row.chars().count(), 54, "{row}");
        }
        for row in MARTTY_SMALL {
            assert_eq!(row.chars().count(), 30, "{row}");
        }
    }

    /// The logo is tinted like the whale's belly (gradient bottom stop)
    /// at every breakpoint, in both themes.
    #[test]
    fn martty_tint_matches_belly() {
        for theme in [Theme::dark(), Theme::light()] {
            let (_, belly) = theme.whale_gradient();
            for width in [8u16, 40, 80, 140] {
                for line in martty_logo_lines(&theme, width) {
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

    /// Wide terminals get the block `ansi_shadow` lockup; narrower ones
    /// degrade to the small figlet variant, then to plain bold text.
    #[test]
    fn martty_tiers() {
        let theme = Theme::dark();

        let wide = martty_logo_lines(&theme, 80);
        assert_eq!(wide.len(), 6);
        assert!(
            text(&wide[0]).contains('█'),
            "block art: {:?}",
            text(&wide[0])
        );

        let medium = martty_logo_lines(&theme, 40);
        assert_eq!(medium.len(), 5);
        assert!(
            medium.iter().any(|l| text(l).contains('|')),
            "small art: {:?}",
            medium.iter().map(text).collect::<Vec<_>>()
        );

        let tiny = martty_logo_lines(&theme, 16);
        assert_eq!(text(&tiny[0]).trim(), "MARTTY");

        assert!(
            martty_logo_lines(&theme, 6).is_empty(),
            "nothing below 8 cols"
        );
    }
}
