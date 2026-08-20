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
    for line in &parsed.lines {
        // Flatten the line-level base style (blockquote, code block, …) into
        // each span so wrapping can move spans freely without losing it.
        let segs: Vec<Seg> = line
            .spans
            .iter()
            .map(|s| Seg {
                text: s.content.to_string(),
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
            if !segs.is_empty() && text.starts_with("```") {
                if in_code {
                    out.push(code_frame_bottom(width, theme));
                    in_code = false;
                } else {
                    out.push(code_frame_top(text.trim_start_matches('`').trim(), width, theme));
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
            let is_data_row = row
                .spans
                .first()
                .and_then(|s| s.content.chars().next())
                == Some('│');
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
        let indent: usize = prefix.iter().map(|s| UnicodeWidthStr::width(s.text.as_str())).sum();
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
        Style::default().fg(self.0.brand).add_modifier(Modifier::BOLD)
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
    segs
        .first()
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
            w += UnicodeWidthChar::width(c).unwrap_or(0).max(1);
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
fn split_prefix(segs: Vec<Seg>) -> (Vec<Seg>, Vec<Seg>) {
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
        return (segs[..n].to_vec(), segs[n..].to_vec());
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
        if is_body_style(seg.style, theme) {
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

/// "Plain body" styles: no background, body-gray or tertiary foreground, and
/// at most italics (blockquotes). Anything with an accent color or stronger
/// modifier keeps its own styling.
fn is_body_style(style: Style, theme: &Theme) -> bool {
    let fg_ok = match style.fg {
        None => true,
        Some(fg) => fg == theme.fg_secondary || fg == theme.fg_tertiary,
    };
    style.bg.is_none()
        && fg_ok
        && style.add_modifier.difference(Modifier::ITALIC).is_empty()
}

/// Code block frame: top edge with an optional language label
/// (`┌─ rust ──────┐`). Border glyphs use the theme's border gray, the
/// language label the caption gray.
fn code_frame_top(lang: &str, width: usize, theme: &Theme) -> Line<'static> {
    let border = Style::default().fg(theme.border);
    let label = if lang.is_empty() {
        String::new()
    } else {
        // Language names are ASCII in practice; clamp anyway so a hostile
        // info string can't blow the frame width.
        let max = width.saturating_sub(8);
        let lang: String = lang.chars().take(max).collect();
        format!("─ {lang} ")
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
            let cw = UnicodeWidthChar::width(c).unwrap_or(0).max(1);
            if w + cw > width && w > 0 {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), style));
                }
                out.push(Line::from(std::mem::take(&mut spans)));
                w = 0;
            }
            if !buf.is_empty() && style != seg.style {
                spans.push(Span::styled(std::mem::take(&mut buf), style));
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
fn collapse_autolinks(segs: Vec<Seg>) -> Vec<Seg> {
    let is_link = |s: &Seg| s.style.add_modifier.contains(Modifier::UNDERLINED);
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    let mut i = 0;
    while i < segs.len() {
        if i + 3 < segs.len()
            && segs[i + 1].text == " ("
            && segs[i + 3].text == ")"
            && segs[i].text == segs[i + 2].text
            && !segs[i].text.is_empty()
            && is_link(&segs[i])
            && is_link(&segs[i + 2])
        {
            out.push(segs[i].clone());
            i += 4;
            continue;
        }
        out.push(segs[i].clone());
        i += 1;
    }
    out
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
            let cw = UnicodeWidthChar::width(c).unwrap_or(0).max(1);
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
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
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
mod tests {
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
        let line_text = |l: &Line| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
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
        let mut colors: Vec<Option<ratatui::style::Color>> = heads
            .iter()
            .map(|l| l.spans[0].style.fg)
            .collect();
        colors.dedup();
        assert!(
            colors.len() >= 4,
            "heading sizes should span several colors: {colors:?}"
        );
        assert!(heads.iter().all(|l| l.spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)));
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
        assert_eq!(latin.style.fg, Some(theme.fg), "Latin/digits render brighter");
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
        assert!(lines.iter().any(|l| l
            .spans
            .iter()
            .any(|s| { s.content.contains("样式") && s.style.add_modifier.contains(Modifier::BOLD) })));
        assert!(lines.iter().any(|l| l
            .spans
            .iter()
            .any(|s| { s.content.contains("**b**") && s.style.bg == Some(theme.panel) })));
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
        assert!(joined[i1..i3].contains('├'), "separator between rows: {joined}");
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
                *w, first,
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

    #[test]
    fn overwide_table_truncates_with_ellipsis() {
        let md = "| column | another |\n|---|---|\n| aaaaaaaaaa | bbbbbbbbbb |";
        let lines = render_dark(md, 16);
        let text = plain(&lines);
        assert!(text.contains('…'), "truncated: {text}");
        for l in &lines {
            assert!(line_width(l) <= 16, "fits: {l:?}");
        }
    }

    #[test]
    fn smoke_complex_markdown_demo() {
        let demo = include_str!("../complex-markdown-demo.md");
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
        assert!(lines.iter().flat_map(|l| l.spans.iter()).any(|s| {
            s.content == ">" && s.style.fg == Some(theme.fg_tertiary)
        }));
    }
}










