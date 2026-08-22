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

/// MAR uses the theme's visible brand-blue gradient while TTY follows
/// the terminal foreground (white in dark mode, black in light mode).
#[test]
fn martty_separates_ocean_mar_from_terminal_tty() {
    for theme in [Theme::dark(), Theme::light()] {
        for width in [16u16, 40, 80] {
            let lines = martty_logo_lines(&theme, width);
            for line in &lines {
                let visible = line
                    .spans
                    .iter()
                    .filter(|span| span.style.fg.is_some())
                    .collect::<Vec<_>>();
                assert_eq!(visible.len(), 2, "MAR + TTY spans at width {width}");
                assert_eq!(visible[1].style.fg, Some(theme.fg), "TTY ink");
            }
            if lines.len() == 1 {
                assert_eq!(lines[0].spans[1].style.fg, Some(theme.brand));
            } else {
                let (top, bottom) = match theme.mode {
                    crate::theme::Mode::Dark => (crate::theme::DEEPSEEK_50, theme.brand),
                    crate::theme::Mode::Light => (theme.brand, crate::theme::DEEPSEEK_200),
                };
                assert_eq!(lines[0].spans[1].style.fg, Some(top));
                assert_eq!(lines.last().unwrap().spans[1].style.fg, Some(bottom));
            }
        }
    }

    let theme = Theme::dark();
    let wide = martty_logo_lines(&theme, 80);
    let wide_ink = wide[0]
        .spans
        .iter()
        .filter(|span| span.style.fg.is_some())
        .collect::<Vec<_>>();
    assert_eq!(wide_ink[0].content, "███╗   ███╗ █████╗ ██████╗ ");
    assert_eq!(wide_ink[1].content, "████████╗████████╗██╗   ██╗");

    let medium = martty_logo_lines(&theme, 40);
    let medium_ink = medium[0]
        .spans
        .iter()
        .filter(|span| span.style.fg.is_some())
        .collect::<Vec<_>>();
    assert_eq!(medium_ink[0].content, " __  __         ");
    assert_eq!(medium_ink[1].content, " _   _        ");
}
