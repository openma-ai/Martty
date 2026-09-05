use super::*;

fn plain(lines: &[Line]) -> String {
    lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect()
}

fn render_dark(md: &str, width: usize) -> Vec<Line<'static>> {
    render(md, &Theme::dark(), width)
}

fn line_width(line: &Line) -> usize {
    line.spans.iter().map(|s| s.content.width()).sum()
}

#[test]
fn inline_emphasis_styles_runs() {
    let theme = Theme::dark();
    let lines = render("a **bold** b *it* c `code` d ~~x~~", &theme, 60);
    let text = plain(&lines);
    assert_eq!(text, "a bold b it c code d x");
    let spans: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
    assert!(spans
        .iter()
        .any(|s| s.content == "bold" && s.style.add_modifier.contains(Modifier::BOLD)));
    assert!(spans
        .iter()
        .any(|s| s.content == "it" && s.style.add_modifier.contains(Modifier::ITALIC)));
    assert!(spans.iter().any(|s| {
        s.content == "code"
            && s.style.bg == Some(theme.panel)
            && s.style.fg == Some(theme.fg)
            && !s.style.add_modifier.contains(Modifier::ITALIC)
    }));
    assert!(spans
        .iter()
        .any(|s| s.content == "x" && s.style.add_modifier.contains(Modifier::CROSSED_OUT)));
}

#[test]
fn link_and_image_render_with_urls() {
    let lines = render_dark("see [docs](https://x.dev) and ![pic](a.png)", 80);
    let text = plain(&lines);
    assert!(text.contains("docs"), "{text}");
    assert!(text.contains("(https://x.dev)"), "{text}");
    assert!(text.contains("[img] pic"), "{text}");
    assert!(text.contains("(a.png)"), "{text}");
}

#[test]
fn code_block_renders_as_framed_box_with_lang_label() {
    let theme = Theme::dark();
    let lines = render("before\n\n```rust\nlet x = 1;\n```\n\nafter", &theme, 40);
    let text = plain(&lines);
    assert!(text.contains("let x = 1;"), "{text}");
    // Fence delimiters become frame edges; the top one names the language.
    assert!(!text.contains("```"), "fences replaced: {text}");
    assert!(text.contains("┌─ rust ─"), "top edge with label: {text}");
    assert!(text.contains('└'), "bottom edge: {text}");
    let code_line = lines
        .iter()
        .find(|l| plain(std::slice::from_ref(l)).contains("let x = 1;"))
        .expect("code line");
    assert_eq!(code_line.spans[0].content, "│");
    assert_eq!(code_line.spans.last().unwrap().content, "│");
    let code_span = code_line
        .spans
        .iter()
        .find(|s| s.content.contains("let x = 1;"))
        .expect("code span");
    // Light gray, upright (no italics), on the panel background.
    assert_eq!(code_span.style.fg, Some(theme.fg));
    assert_eq!(code_span.style.bg, Some(theme.panel));
    assert!(!code_span.style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(line_width(code_line), 40, "row fills the width");
}

#[test]
fn code_line_with_box_chars_is_not_mistaken_for_a_table() {
    let theme = Theme::dark();
    let md = "```\n│ diagram text that runs well past the wrap column\n```";
    let lines = render(md, &theme, 20);
    let text = plain(&lines);
    // Box-drawing code content wraps (mid-word if needed) instead of
    // truncating…
    assert!(!text.contains('…'), "no ellipsis: {text}");
    assert!(text.contains("diagram text"), "{text}");
    assert!(text.contains("wrap colu"), "{text}");
    // …and every wrapped row stays inside the frame.
    let rows: Vec<&Line> = lines
        .iter()
        .filter(|l| l.spans.first().map(|s| s.content.as_ref()) == Some("│"))
        .collect();
    assert!(rows.len() > 1, "wrapped rows: {text}");
    for row in rows {
        assert_eq!(line_width(row), 20, "framed row: {row:?}");
    }
}

#[test]
fn blockquote_gets_prefix_and_italic() {
    let theme = Theme::dark();
    let lines = render("> quoted", &theme, 40);
    let text = plain(&lines);
    assert!(text.contains("> quoted"), "{text}");
    let spans: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
    // The `>` marker keeps the muted quote color…
    assert!(spans.iter().any(|s| {
        s.content == ">"
            && s.style.fg == Some(theme.fg_tertiary)
            && s.style.add_modifier.contains(Modifier::ITALIC)
    }));
    // …while Latin body text takes the brighter two-tone foreground.
    assert!(spans.iter().any(|s| {
        s.content.contains("quoted")
            && s.style.fg == Some(theme.fg)
            && s.style.add_modifier.contains(Modifier::ITALIC)
    }));
}

#[test]
fn list_markers_render() {
    let lines = render_dark("- item one\n2. item two", 40);
    let text = plain(&lines);
    assert!(text.contains("- item one"), "{text}");
    assert!(text.contains("2. item two"), "{text}");
}

#[test]
fn list_wrap_hangs_under_marker() {
    let lines = render_dark("- one two three four five", 12);
    let text = plain(&lines);
    assert!(lines.len() > 1, "wraps: {text:?}");
    assert!(text.contains("- one two"), "{text}");
    // Continuation lines align under the body, two columns in.
    assert!(
        lines[1..].iter().any(|l| l.spans[0].content == "  "),
        "hanging indent: {text:?}"
    );
}

#[test]
fn task_list_checkboxes_render_as_status_glyphs() {
    let theme = Theme::dark();
    let lines = render("- [x] done\n- [ ] pending\n- [X] also done", &theme, 40);
    let text = plain(&lines);
    assert!(text.contains("✓ done"), "checked:\n{text}");
    assert!(text.contains("○ pending"), "open:\n{text}");
    assert!(text.contains("✓ also done"), "uppercase marker:\n{text}");
    assert!(
        !text.contains("[x]") && !text.contains("[X]") && !text.contains("[ ]"),
        "no literal markers:\n{text}"
    );
    // The glyph carries its status color; the dash keeps the list style.
    let line_text = |l: &Line| {
        l.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    };
    let done = lines
        .iter()
        .find(|l| line_text(l).contains("✓ done"))
        .expect("checked line");
    let glyph = done
        .spans
        .iter()
        .find(|s| s.content == "✓ ")
        .expect("glyph span");
    assert_eq!(glyph.style.fg, Some(theme.ok));
    let pending = lines
        .iter()
        .find(|l| line_text(l).contains("○ pending"))
        .expect("open line");
    let glyph = pending
        .spans
        .iter()
        .find(|s| s.content == "○ ")
        .expect("glyph span");
    assert_eq!(glyph.style.fg, Some(theme.caption));
}

#[test]
fn task_list_continuations_hang_under_the_glyph_prefix() {
    let lines = render_dark("- [ ] one two three four five six", 14);
    assert!(lines.len() > 1, "wraps: {lines:?}");
    assert!(
        lines[1..].iter().all(|l| l.spans[0].content == "    "),
        "continuations align under the body (4 cols): {lines:?}"
    );
}

#[test]
fn rule_renders_full_width() {
    let theme = Theme::dark();
    let lines = render("above\n\n---\n\nbelow", &theme, 30);
    let rule = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains('─')))
        .expect("rule line");
    assert_eq!(line_width(rule), 30);
    assert_eq!(rule.spans[0].style.fg, Some(theme.border));
}

#[test]
fn wrapping_preserves_content() {
    let long = "one two three four five six seven eight nine ten";
    let lines = render_dark(long, 12);
    assert!(lines.len() > 1, "soft wraps at width: {lines:?}");
    let joined = plain(&lines);
    for word in long.split(' ') {
        assert!(joined.contains(word), "word {word} kept: {joined}");
    }
    for l in &lines {
        assert!(line_width(l) <= 12, "line fits: {l:?}");
    }
}

#[test]
fn heading_levels_have_distinct_colors() {
    let lines = render_dark("# a\n## b\n### c\n#### d\n##### e\n###### f", 40);
    let heads: Vec<&Line> = lines.iter().filter(|l| !l.spans.is_empty()).collect();
    assert_eq!(heads.len(), 6, "six headings: {}", plain(&lines));
    let mut colors: Vec<Option<ratatui::style::Color>> =
        heads.iter().map(|l| l.spans[0].style.fg).collect();
    colors.dedup();
    assert!(
        colors.len() >= 4,
        "heading sizes should span several colors: {colors:?}"
    );
    assert!(heads
        .iter()
        .all(|l| l.spans[0].style.add_modifier.contains(Modifier::BOLD)));
}

#[test]
fn cjk_and_latin_body_use_different_colors() {
    let theme = Theme::dark();
    let lines = render("你好 world 123", &theme, 40);
    let segs: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
    let cjk = segs
        .iter()
        .find(|s| s.content.contains("你好"))
        .expect("cjk run");
    let latin = segs
        .iter()
        .find(|s| s.content.contains("world"))
        .expect("latin run");
    assert_eq!(
        cjk.style.fg,
        Some(theme.fg_secondary),
        "CJK stays the body gray"
    );
    assert_eq!(
        latin.style.fg,
        Some(theme.fg),
        "Latin/digits render brighter"
    );
    assert_ne!(cjk.style.fg, latin.style.fg);
}

#[test]
fn table_renders_as_box_with_header_and_rows() {
    let theme = Theme::dark();
    let md = "| 样式 | 语法 |\n|------|------|\n| 粗体 | `**b**` |";
    let lines = render(md, &theme, 40);
    let text = plain(&lines);
    assert!(text.contains('┌'), "top border: {text}");
    assert!(text.contains('├'), "header separator: {text}");
    assert!(text.contains('└'), "bottom border: {text}");
    assert!(text.contains("样式"), "header cell: {text}");
    assert!(text.contains("粗体"), "body cell: {text}");
    // The `---` delimiter row must not leak through as literal text.
    assert!(!text.contains("---"), "delimiter hidden: {text}");
    // Header cells are bold; inline code in a cell keeps its chip style.
    assert!(lines.iter().any(|l| l.spans.iter().any(|s| {
        s.content.contains("样式") && s.style.add_modifier.contains(Modifier::BOLD)
    })));
    assert!(lines.iter().any(|l| l
        .spans
        .iter()
        .any(|s| { s.content.contains("**b**") && s.style.bg == Some(theme.panel) })));
}

/// Palettes that alias `border` onto a body gray (Ayu: border ==
/// fg_secondary == #686868) must not get their table frames repainted by
/// the two-tone pass: every box-drawing span keeps the border color, so the
/// hand-drawn `├─┼─┤` junction rows between body rows match the frame (the
/// vertical borders keep no gaps; issue #68). Cell text still two-tones.
#[test]
fn table_borders_keep_border_color_when_a_palette_aliases_it() {
    let mut theme = Theme::dark();
    theme.border = theme.fg_secondary;
    let md = "| A |\n|---|\n| 1 |\n| 2 |";
    let lines = render(md, &theme, 40);
    for l in &lines {
        for s in &l.spans {
            let box_drawing = s
                .content
                .chars()
                .all(|c| matches!(c, '│' | '─' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┼'));
            if box_drawing {
                assert_eq!(
                    s.style.fg,
                    Some(theme.border),
                    "box-drawing span keeps the border color: {:?}",
                    s.content
                );
            }
        }
    }
    // The junction rows meet the columns and match the frame color.
    let junction = lines
        .iter()
        .find(|l| plain(std::slice::from_ref(l)).starts_with('├'))
        .expect("junction row");
    assert_eq!(junction.spans[0].style.fg, Some(theme.border));
    // Cell text is still two-toned: Latin digits render bright.
    let digit = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content == "1")
        .expect("digit span");
    assert_eq!(digit.style.fg, Some(theme.fg));
}

#[test]
fn table_body_rows_are_separated_by_frame_lines() {
    let theme = Theme::dark();
    let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |";
    let lines = render(md, &theme, 40);
    let texts: Vec<String> = lines
        .iter()
        .map(|l| plain(std::slice::from_ref(l)))
        .collect();
    // Header separator + one junction between each pair of body rows.
    let junctions: Vec<&String> = texts.iter().filter(|t| t.starts_with('├')).collect();
    assert_eq!(junctions.len(), 3, "junction lines: {texts:?}");
    // Every junction spans the full table width and meets the columns.
    for j in junctions {
        assert!(j.ends_with('┤'), "junction closed: {j}");
        assert!(j.contains('┼'), "junction crosses columns: {j}");
        assert_eq!(
            UnicodeWidthStr::width(j.as_str()),
            UnicodeWidthStr::width(texts[0].as_str()),
            "width: {j}"
        );
    }
    // Row order: 1, separator, 3, separator, 5.
    let order: Vec<&str> = texts
        .iter()
        .map(|t| t.as_str())
        .filter(|t| t.contains('│') || t.starts_with('├'))
        .collect();
    let joined = order.join("\n");
    let i1 = joined.find("│ 1").unwrap();
    let i3 = joined.find("│ 3").unwrap();
    assert!(
        joined[i1..i3].contains('├'),
        "separator between rows: {joined}"
    );
}

#[test]
fn table_junctions_match_cjk_row_widths() {
    // CJK and emoji cells are two display columns wide; the junction
    // lines must still match the box exactly.
    let md = "| 功能 | 状态 |\n|---|---|\n| 用户认证 | ✅ 完成 |\n| 报表导出 | ⏳ 待开始 |";
    let lines = render_dark(md, 60);
    let widths: Vec<usize> = lines.iter().map(|l| line_width(l)).collect();
    let first = widths[0];
    for (l, w) in lines.iter().zip(&widths) {
        assert_eq!(
            *w,
            first,
            "all frame lines share the box width: {}",
            plain(std::slice::from_ref(l))
        );
    }
    assert!(widths.len() >= 6, "top/head/sep/rows/bottom: {widths:?}");
}

#[test]
fn table_alignment_pads_right_and_center_columns() {
    let md = "| a | bb | ccc |\n|:--|--:|:--:|\n| x | y | z |";
    let lines = render_dark(md, 40);
    let text = plain(&lines);
    assert!(text.contains("│  y │"), "right-aligned: {text}");
    assert!(text.contains("│  z  │"), "center-aligned: {text}");
}

/// Tables wider than the viewport no longer truncate to an ellipsis:
/// columns shrink (narrow ones keep their natural width) and each cell's
/// text soft-wraps across box rows, so every character stays readable.
#[test]
fn overwide_table_soft_wraps_cell_text() {
    // x/y never appear in the header, so surviving-character counts are exact.
    let md = "| column | another |\n|---|---|\n| xxxxxxxxxx | yyyyyyyyyy |";
    let lines = render_dark(md, 16);
    let text = plain(&lines);
    assert!(!text.contains('…'), "no ellipsis: {text}");
    for l in &lines {
        assert!(line_width(l) <= 16, "fits: {l:?}");
    }
    assert_eq!(text.chars().filter(|c| *c == 'x').count(), 10, "{text}");
    assert_eq!(text.chars().filter(|c| *c == 'y').count(), 10, "{text}");
    assert!(text.contains('┌'), "frame kept: {text}");
    assert!(text.contains('├'), "header separator kept: {text}");
    assert!(text.contains('└'), "bottom border kept: {text}");
}

/// A wrapped table's frame geometry survives the re-layout: every line
/// shares the shrunk box width and the junction rows between body rows meet
/// the columns exactly.
#[test]
fn wrapped_table_junctions_match_the_shrunk_box() {
    let md = "| c | d |\n|---|---|\n| aaaaaaaaaa | x |\n| bbbbbbbbbb | y |";
    let lines = render_dark(md, 16);
    let texts: Vec<String> = lines.iter().map(|l| plain(std::slice::from_ref(l))).collect();
    let width = UnicodeWidthStr::width(texts[0].as_str());
    assert!(width <= 16, "box fits: {texts:?}");
    for t in &texts {
        assert_eq!(
            UnicodeWidthStr::width(t.as_str()),
            width,
            "shared frame width: {t}"
        );
    }
    // Header separator plus one junction between the two body rows.
    let junctions: Vec<&String> = texts.iter().filter(|t| t.starts_with('├')).collect();
    assert_eq!(junctions.len(), 2, "{texts:?}");
    for j in junctions {
        assert!(j.contains('┼'), "junction crosses columns: {j}");
    }
}

/// Column alignment survives the re-layout: the source's right-aligned
/// column still pads its short cells to the right even after another
/// column's text wraps.
#[test]
fn wrapped_table_keeps_column_alignment() {
    let md = "| name | score |\n|---|---:|\n| averylongname!! | 420 |\n| short | 7 |";
    let lines = render_dark(md, 24);
    let text = plain(&lines);
    assert!(!text.contains('…'), "no ellipsis: {text}");
    for l in &lines {
        assert!(line_width(l) <= 24, "fits: {l:?}");
    }
    // Right-aligned short cell sits against the column border…
    assert!(text.contains("   7 │"), "right-aligned: {text}");
    // …while the wide cell wraps across rows without losing characters.
    assert!(text.contains("averylongname!"), "wrapped cell: {text}");
    assert!(text.contains("│ ! "), "wrap tail: {text}");
}

/// Tables inside blockquotes are laid out whole: every frame line keeps the
/// `>` prefix and the box stays intact instead of being wrapped apart
/// line-by-line like prose.
#[test]
fn quoted_table_keeps_prefix_and_frame() {
    let md = "> | A |\n> |---|\n> | 1 |\n> | 2 |";
    let lines = render_dark(md, 40);
    let texts: Vec<String> = lines.iter().map(|l| plain(std::slice::from_ref(l))).collect();
    for t in &texts {
        assert!(t.starts_with("> "), "prefix: {t}");
    }
    assert!(texts.iter().any(|t| t.contains("│ A │")), "{texts:?}");
    // Junction between the two body rows, prefixed like the rest of the box.
    assert!(texts.iter().any(|t| t.starts_with("> ├───┤")), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("> └───┘")), "{texts:?}");
}

#[test]
fn smoke_complex_markdown_demo() {
    let demo = include_str!("../../complex-markdown-demo.md");
    for width in [24usize, 40, 80, 120] {
        let lines = render_dark(demo, width);
        assert!(!lines.is_empty(), "width {width} renders");
        for l in &lines {
            assert!(
                line_width(l) <= width,
                "width {width}: line fits ({} cols): {l:?}",
                line_width(l)
            );
        }
    }
    let text = plain(&render_dark(demo, 100));
    for token in [
        "fetchData",
        "npm install",
        "E = mc^2",
        "┌",
        "│",
        "[img]",
        "✓",
        "术语",
    ] {
        assert!(text.contains(token), "token {token} survives");
    }
    // The markdown source's table delimiter rows must not leak through.
    assert!(!text.contains("|:-----"), "table delimiters hidden");
}

#[test]
fn code_block_preserves_indentation() {
    let md = "```python\nif ok:\n    body()\n        deep()\n```";
    let lines = render_dark(md, 60);
    let text = plain(&lines);
    assert!(text.contains("    body()"), "indent kept: {text}");
    assert!(text.contains("        deep()"), "deep indent kept: {text}");
}

#[test]
fn code_block_wraps_without_losing_chars() {
    let md = "```\nabcdefghij klmnopqrst\n```";
    let lines = render_dark(md, 12);
    let text = plain(&lines);
    // Inner width is 12 − 4 frame columns = 8: content hard-wraps whole.
    for chunk in ["abcdefgh", "ij klmno", "pqrst"] {
        assert!(text.contains(chunk), "chunk {chunk} kept: {text}");
    }
    for l in &lines {
        assert!(line_width(l) <= 12, "fits: {l:?}");
    }
}

#[test]
fn autolink_is_not_duplicated() {
    let lines = render_dark("see <https://example.com> now", 80);
    let text = plain(&lines);
    assert_eq!(
        text.matches("https://example.com").count(),
        1,
        "url appears once: {text}"
    );
    // Explicit links with a distinct label keep the destination.
    let lines = render_dark("[docs](https://x.dev)", 80);
    assert!(plain(&lines).contains("docs (https://x.dev)"));
}

#[test]
fn nested_blockquote_keeps_markers_together() {
    let theme = Theme::dark();
    let lines = render("> outer\n> > inner", &theme, 60);
    let text = plain(&lines);
    assert!(text.contains(">> inner"), "nested markers: {text}");
    assert!(lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .any(|s| { s.content == ">" && s.style.fg == Some(theme.fg_tertiary) }));
}

#[test]
fn four_backtick_fences_survive_nested_backtick_content() {
    // Regression: a ```` fence whose *content* contains ``` lines used to
    // toggle the frame logic on the content lines, breaking the block into
    // two frames with an unframed middle.
    let text = "````\n```rust\nlet x = 1;\n```\nplain\n````\n";
    let lines = render(text, &Theme::dark(), 40);
    let texts: Vec<String> = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();

    let tops = texts.iter().filter(|t| t.starts_with('┌')).count();
    let bottoms = texts.iter().filter(|t| t.starts_with('└')).count();
    assert_eq!(tops, 1, "exactly one frame opens: {texts:?}");
    assert_eq!(bottoms, 1, "exactly one frame closes: {texts:?}");
    // The frame holds every content line, including the literal ``` line.
    assert!(texts.iter().any(|t| t.contains("let x = 1;")), "{texts:?}");
    assert!(
        texts.iter().any(|t| t.contains("```rust")),
        "the ``` content line must render inside the frame: {texts:?}"
    );
    assert!(texts.iter().any(|t| t.contains("plain")), "{texts:?}");
    // Order: top edge first, bottom edge last.
    let top = texts.iter().position(|t| t.starts_with('┌')).unwrap();
    let bottom = texts.iter().position(|t| t.starts_with('└')).unwrap();
    assert!(top < bottom);
}
