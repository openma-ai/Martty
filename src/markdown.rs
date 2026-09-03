//! Markdown rendering for assistant text, built on tui-markdown.
//!
//! tui-markdown (pulldown-cmark) owns CommonMark + GFM parsing and block
//! structure; this module maps its output onto the DeepSeek palette through a
//! custom `StyleSheet`, then post-processes the rendered lines:
//!
//! - body runs are split into CJK vs Latin/digit runs so Chinese and English
//!   take two different foreground colors (the transcript's two-tone body);
//! - lines wrap to the chat width, keeping hanging indents under list markers
//!   and blockquote prefixes;
//! - `---` rules are redrawn as full-width `─` lines;
//! - fenced code blocks become framed boxes: fence delimiters turn into
//!   `┌─ lang ──┐` / `└──┘` edges, content rows get `│` side borders and a
//!   padded panel background, and all code — blocks and inline spans alike —
//!   renders in upright light gray;
//! - GFM tables keep tui-markdown's box-drawing layout, with a `├─┼─┤`
//!   junction between every pair of body rows; tables wider than the viewport
//!   are truncated with an ellipsis (wrapping would break the box).
//!
//! Colors stay inside the DeepSeek palette: grayscale body text, brand-blue
//! accents for links and headings, red still reserved for errors.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tui_markdown::{AlertKind, ImageFallback, Options, StyleSheet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::{
    Theme, DEEPSEEK_200, DEEPSEEK_300, DEEPSEEK_400, DEEPSEEK_450, DEEPSEEK_500, DEEPSEEK_600,
    DEEPSEEK_800, DEEPSEEK_900,
};

/// Render markdown `text` to wrapped, styled lines for `width` columns.
pub fn render(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let width = width.max(8);
    let options =
        Options::new(DeepSeekStyleSheet(*theme)).image_fallback(ImageFallback::AltTextAndUrl);
    let parsed = tui_markdown::from_str_with_options(text, &options);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;
    let mut prev_table_row = false;
    for line in parsed.lines {
        // Flatten the line-level base style (blockquote, code block, …) into
        // each span so wrapping can move spans freely without losing it.
        // Consumed by value: tui-markdown owns the spans, so the text moves
        // out instead of being cloned per span per frame.
        let segs: Vec<Seg> = line
            .spans
            .into_iter()
            .map(|s| Seg {
                text: s.content.into_owned(),
                style: line.style.patch(s.style),
            })
            .collect();

        // Code blocks become framed boxes: fence delimiter lines turn into
        // the top/bottom edges (the top one carries the language label),
        // content rows get `│` side borders and a padded panel background.
        // Content hard-wraps without collapsing whitespace so indentation and
        // ASCII art survive; empty lines inside a block still carry the code
        // line style, so they join the box instead of punching a hole in it.
        // Checked before the rule/table heuristics — a fenced `---` or a line
        // starting with a box-drawing char is code, not a rule or table.
        let is_code = if segs.is_empty() {
            line.style.bg == Some(theme.panel)
        } else {
            segs.iter().all(|s| s.style.bg == Some(theme.panel))
        };
        if is_code {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            // Delimiters are sentinel-prefixed (see `code_block_fence`);
            // literal ```` ``` ```` content lines — including inside longer
            // fences — never toggle the frame anymore.
            if !segs.is_empty() && text.starts_with(CODE_FENCE_SENTINEL) {
                if in_code {
                    out.push(code_frame_bottom(width, theme));
                    in_code = false;
                } else {
                    out.push(code_frame_top(
                        text.strip_prefix(CODE_FENCE_SENTINEL)
                            .unwrap_or("")
                            .trim(),
                        width,
                        theme,
                    ));
                    in_code = true;
                }
                continue;
            }
            if in_code {
                let inner = width.saturating_sub(4).max(1);
                let rows = if segs.is_empty() {
                    vec![Line::default()]
                } else {
                    wrap_pre(segs, inner)
                };
                for row in rows {
                    out.push(code_frame_row(row, inner, theme));
                }
            } else {
                // A code-styled line outside any fence (e.g. a paragraph that
                // is one inline-code span): preformatted, but no frame.
                if segs.is_empty() {
                    out.push(Line::default());
                } else {
                    out.extend(wrap_pre(segs, width));
                }
            }
            continue;
        }
        if segs.is_empty() {
            out.push(Line::default());
            continue;
        }
        if is_plain_rule(&segs) {
            out.push(Line::from(Span::styled(
                "─".repeat(width.min(120)),
                Style::default().fg(theme.border),
            )));
            continue;
        }

        if is_table_line(&segs) {
            let row = truncate_line(two_tone(segs, theme), width, theme);
            // Data rows get their own frame line: tui-markdown only draws
            // the header separator, so insert a `├─┼─┤` junction whenever a
            // `│` row follows another `│` row (i.e. between body rows).
            let is_data_row = row.spans.first().and_then(|s| s.content.chars().next()) == Some('│');
            if is_data_row && prev_table_row {
                out.push(table_row_separator(&row, theme));
            }
            prev_table_row = is_data_row;
            out.push(row);
            continue;
        }
        prev_table_row = false;

        let segs = collapse_autolinks(segs);

        // Hanging indent: quote (`>`) and list (`- ` / `1. `) prefixes stay on
        // the first wrapped line; continuations align under the body text.
        // Prefixes keep their block style — the two-tone split applies to the
        // body only, so markers don't turn into bright Latin runs.
        let (prefix, body) = split_prefix(segs);
        // Task-list checkboxes render as status glyphs instead of literal
        // `[x]` / `[ ]` markers, so lists read as rendered UI rather than
        // raw markdown source.
        let prefix = task_list_glyphs(prefix, theme);
        let body = two_tone(body, theme);
        let indent: usize = prefix
            .iter()
            .map(|s| UnicodeWidthStr::width(s.text.as_str()))
            .sum();
        let mut wrapped = wrap_segments(body, width.saturating_sub(indent));
        if wrapped.is_empty() && !prefix.is_empty() {
            wrapped.push(Line::default());
        }
        for (k, mut l) in wrapped.into_iter().enumerate() {
            let mut spans: Vec<Span> = Vec::new();
            if k == 0 {
                spans.extend(prefix.iter().map(|s| Span::styled(s.text.clone(), s.style)));
            } else if indent > 0 {
                spans.push(Span::raw(" ".repeat(indent)));
            }
            spans.append(&mut l.spans);
            out.push(Line::from(spans));
        }
    }
    out
}

/// Sentinel prefix that marks fence delimiter lines (see
/// `DeepSeekStyleSheet::code_block_fence`). Two NUL bytes: never produced
/// by real markdown content.
const CODE_FENCE_SENTINEL: &str = "\u{0}\u{0}";

/// tui-markdown style sheet mapping markdown constructs onto the DeepSeek
/// palette. Only non-default choices are overridden.
#[derive(Clone)]
struct DeepSeekStyleSheet(Theme);

impl StyleSheet for DeepSeekStyleSheet {
    fn heading(&self, level: u8) -> Style {
        heading_style(&self.0, level as usize)
    }

    /// Headings read through color alone; the `#` markers are omitted.
    fn heading_marker(&self, _level: u8) -> &str {
        ""
    }

    /// Fence delimiter lines carry a sentinel prefix instead of bare
    /// backticks. tui-markdown normalizes every fence delimiter to
    /// `"```"`, which is indistinguishable from a literal ```` ``` ````
    /// *content* line inside a 4+ backtick fence (a CommonMark way to
    /// show fenced examples). The sentinel makes delimiters unambiguous:
    /// the frame logic only toggles on it, and backtick-looking content
    /// lines render as code rows. The sentinel is never displayed — the
    /// frame edges replace the delimiter lines.
    fn code_block_fence(&self) -> &str {
        CODE_FENCE_SENTINEL
    }

    /// Inline code and fenced blocks share one look: light gray on the
    /// lighter panel background.
    fn code(&self) -> Style {
        Style::default().fg(self.0.fg).bg(self.0.panel)
    }

    fn link(&self) -> Style {
        Style::default()
            .fg(self.0.brand_soft)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::default()
            .fg(self.0.fg_tertiary)
            .add_modifier(Modifier::ITALIC)
    }

    fn metadata_block(&self) -> Style {
        Style::default().fg(self.0.caption)
    }

    fn html(&self) -> Style {
        Style::default().fg(self.0.caption)
    }

    fn math_inline(&self) -> Style {
        Style::default()
            .fg(self.0.brand_soft)
            .add_modifier(Modifier::ITALIC)
    }

    fn math_display(&self) -> Style {
        Style::default().fg(self.0.brand_soft)
    }

    fn footnote_ref(&self) -> Style {
        Style::default()
            .fg(self.0.caption)
            .add_modifier(Modifier::ITALIC)
    }

    fn footnote_def(&self) -> Style {
        Style::default().fg(self.0.caption)
    }

    fn definition_term(&self) -> Style {
        Style::default().fg(self.0.fg).add_modifier(Modifier::BOLD)
    }

    fn alert(&self, kind: AlertKind) -> Style {
        let color = match kind {
            AlertKind::Note => self.0.brand,
            AlertKind::Tip => self.0.ok_soft(),
            AlertKind::Important => self.0.brand_soft,
            AlertKind::Warning => self.0.warn_soft(),
            AlertKind::Caution => self.0.err,
        };
        Style::default().fg(color)
    }

    fn table_header(&self) -> Style {
        Style::default().fg(self.0.fg).add_modifier(Modifier::BOLD)
    }

    fn table_cell(&self) -> Style {
        Style::default().fg(self.0.fg_secondary)
    }

    fn table_border(&self) -> Style {
        Style::default().fg(self.0.border)
    }

    fn image_alt(&self) -> Style {
        Style::default()
            .fg(self.0.brand)
            .add_modifier(Modifier::BOLD)
    }
}

/// One styled run of text (pre-wrapping). Wrapping is done later so styles
/// survive soft line breaks.
#[derive(Clone)]
struct Seg {
    text: String,
    style: Style,
}

/// A lone `---` line with no styling is a horizontal rule (metadata-block
/// delimiters carry the metadata style, so they are excluded).
fn is_plain_rule(segs: &[Seg]) -> bool {
    segs.len() == 1 && segs[0].text == "---" && segs[0].style == Style::default()
}

/// Table lines start with a box-drawing border character.
fn is_table_line(segs: &[Seg]) -> bool {
    segs.first()
        .and_then(|s| s.text.chars().next())
        .map(|c| matches!(c, '┌' | '├' | '└' | '│'))
        .unwrap_or(false)
}

/// The `├─┼─┤` junction between table body rows, derived from the row's own
/// column layout in *display columns* (CJK/emoji cells are two columns wide)
/// so separators always line up with the cells.
fn table_row_separator(row: &Line, theme: &Theme) -> Line<'static> {
    // Display columns where the row's `│` borders sit.
    let mut bars: Vec<usize> = Vec::new();
    let mut w = 0usize;
    for s in &row.spans {
        for c in s.content.chars() {
            if c == '│' {
                bars.push(w);
            }
            w += UnicodeWidthChar::width(c).unwrap_or(0);
        }
    }
    let mut sep = String::with_capacity(w);
    let mut bars = bars.iter().peekable();
    for col in 0..w {
        if bars.peek() == Some(&&col) {
            bars.next();
            sep.push(if col == 0 {
                '├'
            } else if col == w - 1 {
                '┤'
            } else {
                '┼'
            });
        } else {
            sep.push('─');
        }
    }
    Line::from(Span::styled(sep, Style::default().fg(theme.border)))
}

/// Split off the leading blockquote (`>` runs) or list (`- ` / `- [x] ` /
/// `1. `) marker so wrapped continuations can hang under the body text.
fn split_prefix(mut segs: Vec<Seg>) -> (Vec<Seg>, Vec<Seg>) {
    // Blockquotes: tui-markdown prefixes every quote line with one `>` span
    // per nesting level plus a separating space.
    let mut n = 0;
    let mut saw_gt = false;
    for s in &segs {
        if s.text == ">" {
            saw_gt = true;
            n += 1;
        } else if saw_gt && !s.text.is_empty() && s.text.chars().all(|c| c == ' ') {
            n += 1;
        } else {
            break;
        }
    }
    if saw_gt {
        let body = segs.split_off(n);
        return (segs, body);
    }

    // Lists: the marker lives at the start of the first span, possibly after
    // nesting indentation. Marker bytes are ASCII, so byte indexing is safe.
    let Some(first) = segs.first() else {
        return (Vec::new(), segs);
    };
    let t = first.text.as_str();
    let lead = t.len() - t.trim_start().len();
    let rest = &t[lead..];
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let marker = if rest.starts_with("- [x] ") || rest.starts_with("- [ ] ") {
        lead + 6
    } else if rest.starts_with("- ") {
        lead + 2
    } else if digits > 0 && rest[digits..].starts_with(". ") {
        lead + digits + 2
    } else {
        0
    };
    if marker == 0 {
        return (Vec::new(), segs);
    }
    let mut prefix = vec![Seg {
        text: t[..marker].to_string(),
        style: first.style,
    }];
    let mut body = Vec::new();
    let remainder = &t[marker..];
    if !remainder.is_empty() {
        body.push(Seg {
            text: remainder.to_string(),
            style: first.style,
        });
    }
    body.extend(segs.into_iter().skip(1));
    prefix.shrink_to_fit();
    (prefix, body)
}

/// Replace a task-list checkbox marker (`- [x] ` / `- [X] ` / `- [ ] `) in a
/// split-off list prefix with a status glyph: `✓` in the ok color for
/// checked items, `○` in the caption color for open ones. The dash stays in
/// the original list style so the marker reads as UI, not markdown source;
/// continuation indent follows the shorter glyph prefix automatically.
fn task_list_glyphs(prefix: Vec<Seg>, theme: &Theme) -> Vec<Seg> {
    let Some(first) = prefix.first() else {
        return prefix;
    };
    let t = first.text.as_str();
    let lead = t.len() - t.trim_start().len();
    let rest = &t[lead..];
    let glyph = if rest.starts_with("- [x] ") || rest.starts_with("- [X] ") {
        Some(("✓", theme.ok))
    } else if rest.starts_with("- [ ] ") {
        Some(("○", theme.caption))
    } else {
        None
    };
    let Some((glyph, glyph_color)) = glyph else {
        return prefix;
    };
    // Marker is `lead` spaces + `- [x] ` (6 chars); keep the dash part with
    // the original style and emit the glyph as its own colored span.
    let mut out = Vec::with_capacity(prefix.len() + 1);
    out.push(Seg {
        text: t[..lead + 2].to_string(),
        style: first.style,
    });
    out.push(Seg {
        text: format!("{glyph} "),
        style: Style::default().fg(glyph_color),
    });
    let tail = &t[lead + 6..];
    if !tail.is_empty() {
        out.push(Seg {
            text: tail.to_string(),
            style: first.style,
        });
    }
    out.extend(prefix.into_iter().skip(1));
    out
}

/// Two-tone body coloring: plain body runs (paragraph/list/table-cell text,
/// including blockquote italic) are split into CJK vs Latin/digit runs, CJK
/// keeping the muted body gray and Latin/digits taking the brighter `fg`.
/// Spans that already carry an accent (headings, links, code, captions) are
/// left untouched.
fn two_tone(segs: Vec<Seg>, theme: &Theme) -> Vec<Seg> {
    let mut out = Vec::new();
    for seg in segs {
        // Box-drawing characters are structural borders, never body text:
        // keep their own style. Palettes that alias `border` to a body gray
        // (Ayu: border == fg_secondary == #686868) would otherwise pass them
        // through the body check and repaint the table frame bright while
        // the hand-drawn junction rows stay border-colored, leaving gaps in
        // the vertical borders (issue #68).
        if is_body_style(seg.style, theme) && !is_box_drawing(&seg.text) {
            for (run, is_cjk) in split_script(&seg.text) {
                let fg = if is_cjk {
                    seg.style.fg.unwrap_or(theme.fg_secondary)
                } else {
                    theme.fg
                };
                out.push(Seg {
                    text: run,
                    style: seg.style.fg(fg),
                });
            }
        } else {
            out.push(seg);
        }
    }
    out
}

/// True when the whole run is box-drawing characters (table/code frames).
fn is_box_drawing(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(
            c,
            '─' | '━' | '│' | '┃' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬'
                | '┴' | '┼' | '╭' | '╮' | '╰' | '╯' | '═' | '║' | '╔' | '╗'
                | '╚' | '╝' | '╠' | '╣' | '╦' | '╩' | '╬' | '╒' | '╓' | '╕'
                | '╖' | '╘' | '╙' | '╛' | '╜' | '╞' | '╟' | '╡' | '╢' | '╤'
                | '╥' | '╧' | '╨' | '╪' | '╫'
        )
    })
}

/// "Plain body" styles: no background, body-gray or tertiary foreground, and
/// at most italics (blockquotes). Anything with an accent color or stronger
/// modifier keeps its own styling.
fn is_body_style(style: Style, theme: &Theme) -> bool {
    let fg_ok = match style.fg {
        None => true,
        Some(fg) => fg == theme.fg_secondary || fg == theme.fg_tertiary,
    };
    style.bg.is_none() && fg_ok && style.add_modifier.difference(Modifier::ITALIC).is_empty()
}

/// Code block frame: top edge with an optional language label
/// (`┌─ rust ──────┐`). Border glyphs use the theme's border gray, the
/// language label the caption gray.
fn code_frame_top(lang: &str, width: usize, theme: &Theme) -> Line<'static> {
    let border = Style::default().fg(theme.border);
    let label = if lang.is_empty() {
        String::new()
    } else {
        // Clamp by display width (not char count) so a hostile CJK info
        // string can't blow the frame width either.
        let max = width.saturating_sub(8);
        let mut cut = 0usize;
        let mut used = 0usize;
        for c in lang.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if used + cw > max {
                break;
            }
            used += cw;
            cut += c.len_utf8();
        }
        format!("─ {} ", &lang[..cut])
    };
    let pad = width
        .saturating_sub(2)
        .saturating_sub(UnicodeWidthStr::width(label.as_str()));
    Line::from(vec![
        Span::styled("┌".to_string(), border),
        Span::styled(label, Style::default().fg(theme.caption)),
        Span::styled("─".repeat(pad), border),
        Span::styled("┐".to_string(), border),
    ])
}

/// Bottom edge of a code block frame (`└──────┘`).
fn code_frame_bottom(width: usize, theme: &Theme) -> Line<'static> {
    let border = Style::default().fg(theme.border);
    Line::from(vec![
        Span::styled("└".to_string(), border),
        Span::styled("─".repeat(width.saturating_sub(2)), border),
        Span::styled("┘".to_string(), border),
    ])
}

/// One content row inside the frame: `│ ` + padded content + ` │`, the
/// padding painted with the panel background so the row is a full rectangle.
fn code_frame_row(line: Line<'static>, inner: usize, theme: &Theme) -> Line<'static> {
    let border = Style::default().fg(theme.border);
    let fill = Style::default().bg(theme.panel);
    let w: usize = line.spans.iter().map(|s| s.content.width()).sum();
    let mut spans = vec![
        Span::styled("│".to_string(), border),
        Span::styled(" ".to_string(), fill),
    ];
    spans.extend(line.spans);
    spans.push(Span::styled(" ".repeat(inner.saturating_sub(w) + 1), fill));
    spans.push(Span::styled("│".to_string(), border));
    Line::from(spans)
}

/// Hard-wrap preformatted text, preserving every character — including
/// leading and repeated whitespace that the prose wrapper would collapse.
fn wrap_pre(segs: Vec<Seg>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut style = segs.first().map(|s| s.style).unwrap_or_default();
    let mut w = 0usize;
    for seg in segs {
        for c in seg.text.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > width && w > 0 {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), style));
                }
                out.push(Line::from(std::mem::take(&mut spans)));
                w = 0;
            }
            // Switch style *before* pushing the char: a width flush may
            // have left `buf` empty, and the first char of the new span
            // must not inherit the previous span's style.
            if style != seg.style {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), style));
                }
                style = seg.style;
            }
            buf.push(c);
            w += cw;
        }
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, style));
    }
    if !spans.is_empty() {
        out.push(Line::from(spans));
    }
    out
}

/// tui-markdown renders every link as `label (destination)`; for autolinks
/// and bare-URL links the label *is* the destination, doubling the URL. Drop
/// the redundant ` (url)` tail when label and destination are identical.
fn collapse_autolinks(mut segs: Vec<Seg>) -> Vec<Seg> {
    let is_link = |s: &Seg| s.style.add_modifier.contains(Modifier::UNDERLINED);
    let mut i = 0;
    while i + 3 < segs.len() {
        let redundant = segs[i + 1].text == " ("
            && segs[i + 3].text == ")"
            && segs[i].text == segs[i + 2].text
            && !segs[i].text.is_empty()
            && is_link(&segs[i])
            && is_link(&segs[i + 2]);
        if redundant {
            segs.drain(i + 1..i + 4);
        }
        i += 1;
    }
    segs
}

/// Truncate a line to `width` columns, appending an ellipsis when content was
/// dropped. Used for table lines, where wrapping would break the box.
fn truncate_line(segs: Vec<Seg>, width: usize, theme: &Theme) -> Line<'static> {
    let budget = width.saturating_sub(1);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut w = 0usize;
    let mut stopped = false;
    for seg in segs {
        let mut buf = String::new();
        for c in seg.text.chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > budget {
                stopped = true;
                break;
            }
            w += cw;
            buf.push(c);
        }
        if !buf.is_empty() {
            spans.push(Span::styled(buf, seg.style));
        }
        if stopped {
            break;
        }
    }
    if stopped {
        spans.push(Span::styled(
            "…".to_string(),
            Style::default().fg(theme.caption),
        ));
    }
    Line::from(spans)
}

/// CJK + fullwidth + kana/hangul — the "Chinese" side of the script split.
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF   // CJK Unified Ideographs
            | 0x3400..=0x4DBF // CJK Extension A
            | 0x3000..=0x303F // CJK punctuation
            | 0xFF00..=0xFFEF // fullwidth forms
            | 0x3040..=0x30FF // kana
            | 0xAC00..=0xD7AF // hangul
    )
}

/// Split text into maximal CJK / non-CJK runs for two-tone body rendering.
fn split_script(text: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut cur_is_cjk: Option<bool> = None;
    for c in text.chars() {
        let is = is_cjk(c);
        match cur_is_cjk {
            None => {
                cur.push(c);
                cur_is_cjk = Some(is);
            }
            Some(prev) if prev == is => cur.push(c),
            Some(_) => {
                out.push((std::mem::take(&mut cur), cur_is_cjk.unwrap_or(false)));
                cur.push(c);
                cur_is_cjk = Some(is);
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur, cur_is_cjk.unwrap_or(false)));
    }
    out
}

fn heading_style(theme: &Theme, level: usize) -> Style {
    use crate::theme::Mode;
    // Headings run down the DeepSeek blue ramp (bright → deep) so each
    // "size" reads as a distinct color instead of a muddy gray.
    let color = match theme.mode {
        Mode::Dark => match level {
            1 => DEEPSEEK_200,
            2 => DEEPSEEK_300,
            3 => DEEPSEEK_400,
            4 => DEEPSEEK_450,
            5 => DEEPSEEK_500,
            _ => DEEPSEEK_600,
        },
        Mode::Light => match level {
            1 => DEEPSEEK_400,
            2 => DEEPSEEK_450,
            3 => DEEPSEEK_500,
            4 => DEEPSEEK_600,
            5 => DEEPSEEK_800,
            _ => DEEPSEEK_900,
        },
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

/// Word-boundary wrap that keeps per-segment styles across soft line breaks.
/// Whitespace is collapsed at the margin the same way the plain-text `wrap`
/// does, and over-long words (URLs, CJK runs) are hard-broken without panic.
fn wrap_segments(segs: Vec<Seg>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(4);
    let mut out: Vec<Line> = Vec::new();
    let mut line: Vec<Span> = Vec::new();
    let mut w = 0usize;

    for seg in segs {
        for token in tokenize(&seg.text) {
            if token.chars().all(|c| c == ' ') {
                if w > 0 && w + UnicodeWidthStr::width(token) <= width {
                    line.push(Span::styled(token.to_string(), seg.style));
                    w += UnicodeWidthStr::width(token);
                }
                continue;
            }
            let tw = UnicodeWidthStr::width(token);
            if tw > width {
                if w > 0 {
                    trim_trailing_space(&mut line, &mut w);
                    out.push(Line::from(std::mem::take(&mut line)));
                    w = 0;
                }
                let mut first = true;
                for chunk in chunk_str(token, width) {
                    if !first {
                        out.push(Line::from(std::mem::take(&mut line)));
                    }
                    line.push(Span::styled(chunk.to_string(), seg.style));
                    w = UnicodeWidthStr::width(chunk);
                    first = false;
                }
                continue;
            }
            if w + tw > width && w > 0 {
                trim_trailing_space(&mut line, &mut w);
                out.push(Line::from(std::mem::take(&mut line)));
                w = 0;
            }
            line.push(Span::styled(token.to_string(), seg.style));
            w += tw;
        }
    }
    trim_trailing_space(&mut line, &mut w);
    if !line.is_empty() {
        out.push(Line::from(line));
    }
    out
}

fn trim_trailing_space(line: &mut Vec<Span>, w: &mut usize) {
    while let Some(last) = line.last() {
        if last.content.chars().all(|c| c == ' ') {
            *w -= last.content.width();
            line.pop();
        } else {
            break;
        }
    }
}

/// Split into whitespace runs and non-whitespace runs (spaces preserved as
/// their own tokens so the wrapper can drop them at the margin).
fn tokenize(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut prev_space: Option<bool> = None;
    for (i, ch) in s.char_indices() {
        let is_space = ch == ' ';
        match prev_space {
            None => prev_space = Some(is_space),
            Some(p) if p != is_space => {
                out.push(&s[start..i]);
                start = i;
                prev_space = Some(is_space);
            }
            _ => {}
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Hard-break a string into display-width chunks, never splitting a wide char.
fn chunk_str(s: &str, width: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut w = 0usize;
    for (i, ch) in s.char_indices() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > width && w > 0 {
            out.push(&s[start..i]);
            start = i;
            w = cw;
        } else {
            w += cw;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/markdown__tests.rs"]
mod tests;
