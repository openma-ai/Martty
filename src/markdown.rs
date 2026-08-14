//! Lightweight markdown rendering for assistant text.
//!
//! Renders a terminal-appropriate subset of markdown to styled lines:
//! headings, fenced code blocks, blockquotes, unordered/ordered lists,
//! horizontal rules, and inline emphasis / inline code / strikethrough /
//! links / images.
//!
//! Colors stay inside the DeepSeek palette: grayscale body text, brand-blue
//! accents for links / bullets / inline code / h1-h2, red still reserved for
//! errors. That keeps the transcript in the same visual language as the rest
//! of the TUI rather than turning it into a rainbow.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::{
    Theme, DEEPSEEK_200, DEEPSEEK_300, DEEPSEEK_400, DEEPSEEK_450, DEEPSEEK_500, DEEPSEEK_600,
    DEEPSEEK_800, DEEPSEEK_900,
};
use crate::transcript::wrap;

/// One styled run of text (pre-wrapping). Wrapping is done later so styles
/// survive soft line breaks.
#[derive(Clone)]
struct Seg {
    text: String,
    style: Style,
}

/// Render markdown `text` to wrapped, styled lines for `width` columns.
pub fn render(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut out: Vec<Line> = Vec::new();
    let mut in_fence = false;

    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let fence_marker = trimmed.starts_with("```") || trimmed.starts_with("~~~");

        if fence_marker {
            in_fence = !in_fence;
            out.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(theme.caption).bg(theme.code_bg),
            )));
            continue;
        }
        if in_fence {
            for l in wrap(raw, width) {
                out.push(Line::from(Span::styled(
                    l,
                    Style::default().fg(theme.fg_secondary).bg(theme.code_bg),
                )));
            }
            continue;
        }
        if raw.trim().is_empty() {
            out.push(Line::default());
            continue;
        }
        if let Some((level, rest)) = heading(raw) {
            let style = heading_style(theme, level);
            out.extend(wrap_segments(
                inline_parse(&rest, style, theme, false),
                width,
            ));
            continue;
        }
        if let Some(rest) = strip_blockquote(raw) {
            let base = Style::default()
                .fg(theme.fg_tertiary)
                .add_modifier(Modifier::ITALIC);
            let body = wrap_segments(
                inline_parse(&rest, base, theme, true),
                width.saturating_sub(2),
            );
            for (k, mut line) in body.into_iter().enumerate() {
                let mut spans = vec![Span::styled(
                    if k == 0 { "▎ " } else { "  " }.to_string(),
                    Style::default().fg(theme.brand_soft),
                )];
                spans.append(&mut line.spans);
                out.push(Line::from(spans));
            }
            continue;
        }
        if let Some((marker, rest)) = list_item(raw) {
            let base = Style::default().fg(theme.fg_secondary);
            let segs = inline_parse(&rest, base, theme, true);
            let indent = marker.width() + 1;
            let body = wrap_segments(segs, width.saturating_sub(indent));
            for (k, mut line) in body.into_iter().enumerate() {
                let mut spans = vec![Span::styled(
                    if k == 0 {
                        format!("{marker} ")
                    } else {
                        " ".repeat(indent)
                    },
                    Style::default().fg(theme.brand),
                )];
                spans.append(&mut line.spans);
                out.push(Line::from(spans));
            }
            continue;
        }
        if is_hr(trimmed) {
            out.push(Line::from(Span::styled(
                "─".repeat(width.min(120)),
                Style::default().fg(theme.border),
            )));
            continue;
        }

        // Plain paragraph: inline markdown only.
        let base = Style::default().fg(theme.fg_secondary);
        out.extend(wrap_segments(inline_parse(raw, base, theme, true), width));
    }
    out
}

/// Inline markdown → styled segments. Emphasis is shallow (toggled flags),
/// which is exactly what chat text needs; code spans are treated verbatim so
/// identifiers and symbols inside backticks never get re-interpreted.
///
/// When `script` is true, plain body runs are split into CJK vs Latin/digit
/// runs so Chinese and English can take two different foreground colors
/// (headings, links, and inline code keep their own accent colors).
fn inline_parse(line: &str, base: Style, theme: &Theme, script: bool) -> Vec<Seg> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut segs: Vec<Seg> = Vec::new();
    let mut buf = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut code = false;

    let flush = |buf: &mut String, segs: &mut Vec<Seg>, style: Style| {
        if buf.is_empty() {
            return;
        }
        let text = std::mem::take(buf);
        if script && style.bg.is_none() && style.fg == base.fg {
            for (run, is_cjk) in split_script(&text) {
                let mut s = style;
                s = s.fg(if is_cjk {
                    base.fg.unwrap_or(theme.fg_secondary)
                } else {
                    theme.fg
                });
                segs.push(Seg {
                    text: run,
                    style: s,
                });
            }
        } else {
            segs.push(Seg { text, style });
        }
    };

    let mut i = 0;
    while i < n {
        if code {
            if chars[i] == '`' {
                flush(
                    &mut buf,
                    &mut segs,
                    seg_style(base, theme, bold, italic, strike, true),
                );
                code = false;
                i += 1;
            } else {
                buf.push(chars[i]);
                i += 1;
            }
            continue;
        }

        let style = seg_style(base, theme, bold, italic, strike, false);
        if chars[i] == '`' {
            flush(&mut buf, &mut segs, style);
            code = true;
            i += 1;
            continue;
        }
        if i + 1 < n
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_'))
        {
            flush(&mut buf, &mut segs, style);
            bold = !bold;
            i += 2;
            continue;
        }
        if i + 1 < n && chars[i] == '~' && chars[i + 1] == '~' {
            flush(&mut buf, &mut segs, style);
            strike = !strike;
            i += 2;
            continue;
        }
        if chars[i] == '*' || chars[i] == '_' {
            flush(&mut buf, &mut segs, style);
            italic = !italic;
            i += 1;
            continue;
        }
        if chars[i] == '!' && i + 1 < n && chars[i + 1] == '[' {
            if let Some((alt, url, next)) = parse_target(&chars, i + 1) {
                flush(&mut buf, &mut segs, style);
                let label = if alt.trim().is_empty() {
                    "image".to_string()
                } else {
                    alt.trim().to_string()
                };
                segs.push(Seg {
                    text: format!("🖼 {label}"),
                    style: Style::default()
                        .fg(theme.brand)
                        .add_modifier(Modifier::BOLD),
                });
                if !url.is_empty() {
                    segs.push(Seg {
                        text: format!(" ({})", clamp(&url, 40)),
                        style: Style::default().fg(theme.caption),
                    });
                }
                i = next;
                continue;
            }
        }
        if chars[i] == '[' {
            if let Some((text, url, next)) = parse_target(&chars, i) {
                flush(&mut buf, &mut segs, style);
                let label = if text.trim().is_empty() {
                    url.clone()
                } else {
                    text.clone()
                };
                segs.push(Seg {
                    text: label,
                    style: Style::default()
                        .fg(theme.brand_soft)
                        .add_modifier(Modifier::UNDERLINED),
                });
                if !url.is_empty() && url != text {
                    segs.push(Seg {
                        text: format!(" ({})", clamp(&url, 40)),
                        style: Style::default().fg(theme.caption),
                    });
                }
                i = next;
                continue;
            }
        }
        if chars[i] == '<' {
            if let Some((url, next)) = parse_autolink(&chars, i) {
                flush(&mut buf, &mut segs, style);
                segs.push(Seg {
                    text: url,
                    style: Style::default()
                        .fg(theme.brand_soft)
                        .add_modifier(Modifier::UNDERLINED),
                });
                i = next;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(
        &mut buf,
        &mut segs,
        seg_style(base, theme, bold, italic, strike, code),
    );
    segs
}

fn seg_style(
    base: Style,
    theme: &Theme,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
) -> Style {
    if code {
        Style::default().fg(theme.brand_soft).bg(theme.code_bg)
    } else {
        let mut s = base;
        if bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if strike {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        s
    }
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

/// Parse `[text](url)` starting at the `[` index (or at the `[` after `!`).
fn parse_target(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    // First `]` closes (no nested brackets in chat markdown).
    let close = chars
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, &c)| c == ']')?
        .0;
    if close + 1 >= chars.len() || chars[close + 1] != '(' {
        return None;
    }
    let end = chars
        .iter()
        .enumerate()
        .skip(close + 2)
        .find(|(_, &c)| c == ')')?
        .0;
    let text: String = chars[start + 1..close].iter().collect();
    let url: String = chars[close + 2..end].iter().collect();
    Some((text, url, end + 1))
}

fn parse_autolink(chars: &[char], start: usize) -> Option<(String, usize)> {
    if chars.get(start) != Some(&'<') {
        return None;
    }
    let tail: String = chars[start + 1..].iter().collect();
    if !(tail.starts_with("http://") || tail.starts_with("https://")) {
        return None;
    }
    let rel = chars[start + 1..].iter().position(|&c| c == '>')?;
    let end = start + 1 + rel;
    let url: String = chars[start + 1..end].iter().collect();
    if url.is_empty() {
        return None;
    }
    Some((url, end + 1))
}

fn heading(line: &str) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    let mut level = 0;
    while level < bytes.len() && bytes[level] == b'#' {
        level += 1;
    }
    if level == 0 || level > 6 {
        return None;
    }
    if level < bytes.len() && bytes[level] != b' ' {
        return None;
    }
    Some((level, line[level..].trim().to_string()))
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

fn strip_blockquote(line: &str) -> Option<String> {
    let t = line.trim_start();
    t.strip_prefix('>')
        .map(|rest| rest.trim_start().to_string())
}

fn list_item(line: &str) -> Option<(String, String)> {
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(("•".to_string(), rest.to_string()));
        }
    }
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' ' {
        let num = &line[..i];
        let rest = &line[i + 2..];
        return Some((format!("{num}."), rest.to_string()));
    }
    None
}

fn is_hr(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.chars().count() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
}

fn clamp(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
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

    #[test]
    fn inline_emphasis_styles_runs() {
        let theme = Theme::dark();
        let base = Style::default().fg(theme.fg_secondary);
        let segs = inline_parse("a **bold** b *it* c `code` d ~~x~~", base, &theme, false);
        let text: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "a bold b it c code d x");
        assert!(segs
            .iter()
            .any(|s| s.text == "bold" && s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(segs
            .iter()
            .any(|s| s.text == "it" && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(segs
            .iter()
            .any(|s| s.text == "code" && s.style.bg == Some(theme.code_bg)));
        assert!(segs
            .iter()
            .any(|s| s.text == "x" && s.style.add_modifier.contains(Modifier::CROSSED_OUT)));
    }

    #[test]
    fn link_and_image_render_as_chips() {
        let theme = Theme::dark();
        let lines = render("see [docs](https://x.dev) and ![pic](a.png)", &theme, 60);
        let text = plain(&lines);
        assert!(text.contains("docs"), "{text}");
        assert!(text.contains("https://x.dev"), "{text}");
        assert!(text.contains("🖼 pic"), "{text}");
    }

    #[test]
    fn fence_keeps_code_background() {
        let theme = Theme::dark();
        let lines = render("before\n```rust\nlet x = 1;\n```\nafter", &theme, 40);
        assert!(plain(&lines).contains("let x = 1;"));
        assert!(lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("let x") && s.style.bg == Some(theme.code_bg))
        }));
    }

    #[test]
    fn blockquote_and_list_get_markers() {
        let theme = Theme::dark();
        let lines = render("> quoted\n- item one\n2. item two", &theme, 40);
        let text = plain(&lines);
        assert!(text.contains("▎ quoted"), "{text}");
        assert!(text.contains("• item one"), "{text}");
        assert!(text.contains("2. item two"), "{text}");
    }

    #[test]
    fn wrapping_preserves_content() {
        let theme = Theme::dark();
        let long = "one two three four five six seven eight nine ten";
        let lines = render(long, &theme, 12);
        assert!(lines.len() > 1, "soft wraps at width: {lines:?}");
        let joined = plain(&lines);
        for word in long.split(' ') {
            assert!(joined.contains(word), "word {word} kept: {joined}");
        }
    }

    #[test]
    fn heading_levels_have_distinct_colors() {
        let theme = Theme::dark();
        let lines = render("# a\n## b\n### c\n#### d\n##### e\n###### f", &theme, 40);
        let mut colors: Vec<Option<ratatui::style::Color>> =
            (0..6).map(|i| lines[i].spans[0].style.fg).collect();
        colors.dedup();
        assert!(
            colors.len() >= 4,
            "heading sizes should span several colors: {colors:?}"
        );
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
}
