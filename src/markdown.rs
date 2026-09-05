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
//!   shrink their columns — narrow ones keep their natural width, wide ones
//!   share what remains proportionally — and cell text soft-wraps across box
//!   rows, so long cells stay readable instead of truncating with an ellipsis.
//!
//! Colors stay inside the DeepSeek palette: grayscale body text, brand-blue
//! accents for links and headings, red still reserved for errors.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tui_markdown::{AlertKind, ImageFallback, Options, StyleSheet};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
    let mut table: Option<TableBuffer> = None;
    let mut code_prefix: Vec<Seg> = Vec::new();
    let mut code_blocks = None;
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

        // Fence sentinels are structural spans, even when a quote prefix
        // precedes them. Track that prefix separately from code content;
        // colors alone cannot distinguish nested blocks from inline code.
        if let Some(fence) = segs.iter().position(|s| s.text.starts_with(CODE_FENCE_SENTINEL)) {
            flush_table(&mut table, &mut out, theme, width);
            if in_code {
                let frame_width = width - segments_width(&code_prefix);
                out.push(with_prefix(&code_prefix, code_frame_bottom(frame_width, theme)));
                in_code = false;
            } else {
                let lang = segs[fence].text[CODE_FENCE_SENTINEL.len()..].trim();
                code_prefix = fit_prefix(segs[..fence].to_vec(), width.saturating_sub(8), theme);
                let frame_width = width - segments_width(&code_prefix);
                out.push(with_prefix(&code_prefix, code_frame_top(lang, frame_width, theme)));
                // tui-markdown joins separate Text events in quoted code.
                // Preserve their exact newlines from the parser instead.
                let content = code_blocks.get_or_insert_with(|| code_block_texts(text))
                    .pop_front().unwrap_or_default();
                let inner = frame_width - 4;
                for raw in content.lines() {
                    let mut rows = wrap_pre(vec![Seg {
                        text: raw.to_string(), style: DeepSeekStyleSheet(*theme).code(),
                    }], inner);
                    if rows.is_empty() { rows.push(Line::default()); }
                    for row in rows {
                        out.push(with_prefix(&code_prefix, code_frame_row(row, inner, theme)));
                    }
                }
                in_code = true;
            }
            continue;
        }
        if in_code { continue; }
        // A standalone inline-code paragraph remains preformatted.
        if !segs.is_empty() && segs.iter().all(|s| s.style.bg == Some(theme.panel)) {
            flush_table(&mut table, &mut out, theme, width);
            out.extend(wrap_pre(segs, width));
            continue;
        }
        if segs.is_empty() {
            flush_table(&mut table, &mut out, theme, width);
            out.push(Line::default());
            continue;
        }
        if is_plain_rule(&segs) {
            flush_table(&mut table, &mut out, theme, width);
            out.push(Line::from(Span::styled(
                "─".repeat(width.min(120)),
                Style::default().fg(theme.border),
            )));
            continue;
        }

        // Hanging indent: quote (`>`) and list (`- ` / `1. `) prefixes stay on
        // the first wrapped line; continuations align under the body text.
        // Prefixes keep their block style — the two-tone split applies to the
        // body only, so markers don't turn into bright Latin runs.
        let segs = collapse_autolinks(segs);
        let (prefix, body) = split_prefix(segs);
        // Table frames are buffered whole: tui-markdown sizes every column
        // to its widest cell with no viewport knowledge, so fitting the
        // table to the chat width (soft-wrapping cell text across box rows)
        // needs all rows before the layout is known. Quote/list prefixes are
        // split off first and re-applied as a paint-time indent.
        if let Some((lead, first)) = table_lead(&body) {
            let indent = prefix_width(&prefix) + lead;
            let mut prefix_spans: Vec<Span<'static>> = prefix
                .iter()
                .map(|s| Span::styled(s.text.clone(), s.style))
                .collect();
            if lead > 0 {
                prefix_spans.push(Span::raw(" ".repeat(lead)));
            }
            let line = TableLine { prefix: prefix_spans, segs: strip_lead_spaces(body) };
            if first == '┌' {
                flush_table(&mut table, &mut out, theme, width);
                table = Some(TableBuffer { indent: Some(indent), lines: vec![line] });
            } else {
                let buf = table
                    .get_or_insert_with(|| TableBuffer { indent: None, lines: Vec::new() });
                buf.indent.get_or_insert(indent);
                buf.lines.push(line);
            }
            continue;
        }
        flush_table(&mut table, &mut out, theme, width);

        // Task-list checkboxes render as status glyphs instead of literal
        // `[x]` / `[ ]` markers, so lists read as rendered UI rather than
        // raw markdown source.
        let prefix = fit_prefix(task_list_glyphs(prefix, theme), width.saturating_sub(4), theme);
        let body = two_tone(body, theme);
        let indent = prefix_width(&prefix);
        let mut wrapped = wrap_segments(body, width.saturating_sub(indent).max(4));
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
    flush_table(&mut table, &mut out, theme, width);
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

/// Match tui-markdown's parser extensions so code-block order agrees even
/// inside footnotes, metadata, alerts, lists, and incomplete streamed fences.
fn code_block_texts(text: &str) -> std::collections::VecDeque<String> {
    use pulldown_cmark::{Event, Options as ParseOptions, Parser, Tag, TagEnd};
    let options = ParseOptions::ENABLE_STRIKETHROUGH | ParseOptions::ENABLE_TASKLISTS
        | ParseOptions::ENABLE_HEADING_ATTRIBUTES | ParseOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | ParseOptions::ENABLE_SUPERSCRIPT | ParseOptions::ENABLE_SUBSCRIPT
        | ParseOptions::ENABLE_MATH | ParseOptions::ENABLE_FOOTNOTES
        | ParseOptions::ENABLE_DEFINITION_LIST | ParseOptions::ENABLE_GFM | ParseOptions::ENABLE_TABLES;
    let mut blocks = std::collections::VecDeque::new();
    let mut code = None;
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::Text(text) => if let Some(code) = &mut code { code.push_str(&text); },
            Event::End(TagEnd::CodeBlock) => if let Some(code) = code.take() { blocks.push_back(code); },
            _ => {}
        }
    }
    blocks
}

fn segments_width(segs: &[Seg]) -> usize {
    segs.iter().map(|s| s.text.width()).sum()
}

fn with_prefix(prefix: &[Seg], mut line: Line<'static>) -> Line<'static> {
    let mut spans: Vec<_> = prefix.iter().map(|s| Span::styled(s.text.clone(), s.style)).collect();
    spans.append(&mut line.spans);
    Line::from(spans)
}

/// Compress excessive nesting before clipping a marker. Always leave room
/// for the body so a narrow view cannot place all content off screen.
fn fit_prefix(mut prefix: Vec<Seg>, budget: usize, theme: &Theme) -> Vec<Seg> {
    if budget == 0 { return Vec::new(); }
    let mut excess = segments_width(&prefix).saturating_sub(budget);
    for seg in &mut prefix {
        let leading = seg.text.len() - seg.text.trim_start_matches(' ').len();
        let only_spaces = leading == seg.text.len();
        let remove = excess.min(leading);
        seg.text.drain(..remove);
        excess -= remove;
        if !only_spaces || excess == 0 { break; }
    }
    truncate_line(prefix, budget, theme).spans.into_iter().map(|s| Seg {
        text: s.content.into_owned(), style: s.style,
    }).collect()
}

/// A lone `---` line with no styling is a horizontal rule (metadata-block
/// delimiters carry the metadata style, so they are excluded).
fn is_plain_rule(segs: &[Seg]) -> bool {
    segs.len() == 1 && segs[0].text == "---" && segs[0].style == Style::default()
}

/// A buffered table block: tui-markdown sizes columns to their widest cell
/// with no viewport knowledge, so the whole frame is collected before the
/// block can be fitted to the chat width. `indent` is the leading column
/// count shared by every line (set by the first line of the block).
struct TableBuffer {
    indent: Option<usize>,
    lines: Vec<TableLine>,
}

/// One buffered frame line: the quote/list prefix spans are kept verbatim
/// (a `>` marker must survive; list continuations carry plain spaces) and
/// re-applied at paint time, while leading spaces are stripped from the
/// frame body itself.
struct TableLine {
    prefix: Vec<Span<'static>>,
    segs: Vec<Seg>,
}

/// One parsed logical row plus the prefix spans to re-apply on each of its
/// painted lines.
struct TableRow {
    prefix: Vec<Span<'static>>,
    cells: Vec<TableCell>,
}

/// One parsed cell: its styled runs plus the widths of the stripped edge
/// padding spans (the alignment signal).
struct TableCell {
    segs: Vec<Seg>,
    lead: usize,
    trail: usize,
}

/// Column alignment, recovered from the original padding spans.
#[derive(Clone, Copy)]
enum TAlign {
    Left,
    Center,
    Right,
}

/// Paint and drop a buffered table block; called when any non-table line
/// (or the end of the text) interrupts the block.
fn flush_table(
    table: &mut Option<TableBuffer>,
    out: &mut Vec<Line<'static>>,
    theme: &Theme,
    width: usize,
) {
    if let Some(buf) = table.take() {
        out.extend(render_table(buf, theme, width));
    }
}

/// Leading display columns before the first non-space character of a line,
/// when that character opens a table frame line (`┌`/`│`/`├`/`└`). The
/// spaces are tui-markdown's list-continuation indent; blockquote/list
/// markers were already split off by `split_prefix`.
fn table_lead(body: &[Seg]) -> Option<(usize, char)> {
    let mut lead = 0usize;
    for seg in body {
        for c in seg.text.chars() {
            if c == ' ' {
                lead += 1;
            } else {
                return match c {
                    '┌' | '│' | '├' | '└' => Some((lead, c)),
                    _ => None,
                };
            }
        }
    }
    None
}

/// Drop the leading all-space spans of a buffered table line (list
/// continuation indent); the width is re-applied from `TableLine::prefix`
/// at paint time.
fn strip_lead_spaces(mut segs: Vec<Seg>) -> Vec<Seg> {
    let mut i = 0;
    while segs.get(i).is_some_and(|s| s.text.chars().all(|c| c == ' ')) {
        i += 1;
    }
    segs.drain(..i);
    if let Some(first) = segs.first_mut() {
        let lead = first.text.len() - first.text.trim_start_matches(' ').len();
        if lead > 0 {
            first.text = first.text.split_off(lead);
        }
    }
    segs
}

/// Total display width of the split-off prefix spans (quote/list markers).
fn prefix_width(prefix: &[Seg]) -> usize {
    prefix.iter().map(|s| UnicodeWidthStr::width(s.text.as_str())).sum()
}

/// Lay out one buffered table block. When the block fits the available
/// width it is repainted as tui-markdown drew it (two-tone cells, `├─┼─┤`
/// junctions between body rows); otherwise the columns shrink and each
/// cell's text soft-wraps across box rows, so long cells read in full
/// instead of truncating with an ellipsis.
fn render_table(buf: TableBuffer, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let Some((header, body)) = parse_table(&buf.lines) else {
        return paint_fit(&buf, theme, width);
    };
    let cols = header.iter().chain(&body).map(|r| r.cells.len()).max().unwrap_or(0);
    let mut natural = vec![1usize; cols];
    for row in header.iter().chain(&body) {
        for (i, cell) in row.cells.iter().enumerate() {
            natural[i] = natural[i].max(cell_width(&cell.segs));
        }
    }
    let avail = width.saturating_sub(buf.indent.unwrap_or(0));
    // (cols+1) border columns plus the fixed one-space padding on both
    // sides of every cell.
    let overhead = 3 * cols + 1;
    // If even one content column per cell cannot fit, retain the clipped
    // fallback rather than drawing a frame wider than the viewport.
    if overhead + cols > avail || overhead + natural.iter().sum::<usize>() <= avail {
        return paint_fit(&buf, theme, width);
    }

    // Column alignment is recovered from the original padding spans: the
    // fixed one-space pad sits on the alignment's short side, so leading
    // padding wider than one space means right-aligned, and vice versa.
    let mut aligns = vec![TAlign::Left; cols];
    for (i, align) in aligns.iter_mut().enumerate() {
        for row in header.iter().chain(&body) {
            let Some(cell) = row.cells.get(i) else { continue };
            let sig = match (cell.lead, cell.trail) {
                (l, t) if l >= 2 && t >= 2 => TAlign::Center,
                (l, _) if l >= 2 => TAlign::Right,
                (_, t) if t >= 2 => TAlign::Left,
                _ => continue,
            };
            *align = sig;
            break;
        }
    }

    let widths = shrink_columns(&natural, avail.saturating_sub(overhead));
    let border = Style::default().fg(theme.border);
    // The frame is rebuilt, so generated lines take the quote/list prefix of
    // their source line; synthesized separators take the continuation form
    // (the last buffered line — a `└` edge — never sits on the marker line).
    let empty: Vec<Span<'static>> = Vec::new();
    let cont_prefix = buf.lines.last().map(|l| &l.prefix).unwrap_or(&empty);
    let mut lines = Vec::new();
    lines.push(prefix_line(
        frame_edge('┌', '┬', '┐', &widths, border),
        &buf.lines.first().map(|l| &l.prefix).unwrap_or(&empty),
    ));
    for row in &header {
        lines.extend(paint_row(row, &widths, &aligns, theme, false));
    }
    lines.push(prefix_line(frame_edge('├', '┼', '┤', &widths, border), cont_prefix));
    for (i, row) in body.iter().enumerate() {
        if i > 0 {
            lines.push(prefix_line(frame_edge('├', '┼', '┤', &widths, border), cont_prefix));
        }
        lines.extend(paint_row(row, &widths, &aligns, theme, true));
    }
    lines.push(prefix_line(frame_edge('└', '┴', '┘', &widths, border), cont_prefix));
    lines
}

/// Prepend the quote/list prefix spans to a painted table line.
fn prefix_line(mut line: Line<'static>, prefix: &[Span<'static>]) -> Line<'static> {
    if !prefix.is_empty() {
        line.spans.splice(0..0, prefix.iter().cloned());
    }
    line
}

/// Parse the buffered frame lines into header and body rows: `│` lines are
/// rows (one physical line each — tui-markdown never wraps cells), and the
/// first `├` closes the header. Returns `None` when the block is not a
/// well-formed frame (no `┌` opener), falling back to the verbatim paint.
fn parse_table(lines: &[TableLine]) -> Option<(Vec<TableRow>, Vec<TableRow>)> {
    let mut header: Vec<TableRow> = Vec::new();
    let mut body: Vec<TableRow> = Vec::new();
    let mut in_body = false;
    let mut opened = false;
    for line in lines {
        match line.segs.first().and_then(|s| s.text.chars().next()) {
            Some('┌') => opened = true,
            Some('├') => in_body = true,
            Some('│') => {
                let row = TableRow {
                    prefix: line.prefix.clone(),
                    cells: parse_row(&line.segs),
                };
                if in_body {
                    body.push(row);
                } else {
                    header.push(row);
                }
            }
            _ => {}
        }
    }
    opened.then_some((header, body))
}

/// Split one rendered row into cells: spans of bare `│` delimit cells; the
/// all-space padding spans at each cell edge are stripped but measured —
/// together they reveal the column's source alignment.
fn parse_row(segs: &[Seg]) -> Vec<TableCell> {
    let mut cells: Vec<TableCell> = Vec::new();
    let mut cur: Vec<Seg> = Vec::new();
    let mut in_cell = false;
    for seg in segs {
        if !seg.text.is_empty() && seg.text.chars().all(|c| c == '│') {
            if in_cell {
                cells.push(finish_cell(&mut cur));
            }
            in_cell = true;
            cur.clear();
        } else if in_cell {
            cur.push(seg.clone());
        }
    }
    cells
}

fn finish_cell(cur: &mut Vec<Seg>) -> TableCell {
    let is_space = |s: &Seg| s.text.chars().all(|c| c == ' ');
    let mut lead = 0;
    while cur.first().is_some_and(is_space) {
        lead += UnicodeWidthStr::width(cur[0].text.as_str());
        cur.remove(0);
    }
    let mut trail = 0;
    while cur.last().is_some_and(is_space) {
        trail += UnicodeWidthStr::width(cur.last().unwrap().text.as_str());
        cur.pop();
    }
    TableCell {
        segs: std::mem::take(cur),
        lead,
        trail,
    }
}

fn cell_width(cell: &[Seg]) -> usize {
    cell.iter().map(|s| UnicodeWidthStr::width(s.text.as_str())).sum()
}

/// Shrink natural column widths to a `budget` of content columns: every
/// column keeps at least one, narrow columns reach their natural width
/// first, and the wide ones share what remains proportionally.
fn shrink_columns(natural: &[usize], budget: usize) -> Vec<usize> {
    let mut widths = vec![1usize; natural.len()];
    let mut remaining = budget.saturating_sub(natural.len());
    while remaining > 0 {
        let slack: usize = natural.iter().zip(&widths).map(|(n, w)| n.saturating_sub(*w)).sum();
        if slack == 0 {
            break;
        }
        let mut gave = 0;
        for i in 0..natural.len() {
            if remaining == 0 {
                break;
            }
            let slack_i = natural[i].saturating_sub(widths[i]);
            if slack_i == 0 {
                continue;
            }
            let give = (remaining * slack_i / slack).max(1).min(slack_i).min(remaining);
            widths[i] += give;
            remaining -= give;
            gave += give;
        }
        if gave == 0 {
            break;
        }
    }
    widths
}

/// A frame edge line (`┌─┬─┐` / `├─┼─┤` / `└─┴─┘`) for the given content
/// column widths, matching tui-markdown's two-space cell padding.
fn frame_edge(left: char, cross: char, right: char, widths: &[usize], border: Style) -> Line<'static> {
    let mut s = String::with_capacity(widths.iter().sum::<usize>() + widths.len() + 2);
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        for _ in 0..w + 2 {
            s.push('─');
        }
        if i + 1 < widths.len() {
            s.push(cross);
        }
    }
    s.push(right);
    Line::from(Span::styled(s, border))
}

/// Paint one logical row as one or more `│ … │` lines: each cell's styled
/// runs are two-toned and soft-wrapped to its column width; cells that fit
/// on one line keep the column's alignment, wrapped ones left-align. Every
/// physical line re-applies the row's quote/list prefix.
fn paint_row(
    row: &TableRow,
    widths: &[usize],
    aligns: &[TAlign],
    theme: &Theme,
    body: bool,
) -> Vec<Line<'static>> {
    let border = Style::default().fg(theme.border);
    let pad = if body {
        Style::default().fg(theme.fg_secondary)
    } else {
        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
    };
    let cells = &row.cells;
    let wrapped: Vec<Vec<Line<'static>>> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let segs = cells
                .get(i)
                .map(|c| two_tone(c.segs.clone(), theme))
                .unwrap_or_default();
            wrap_segments(segs, *w)
        })
        .collect();
    let height = wrapped.iter().map(|ls| ls.len().max(1)).max().unwrap_or(1);
    let mut lines = Vec::with_capacity(height);
    for k in 0..height {
        let mut spans: Vec<Span<'static>> = vec![Span::styled("│".to_string(), border)];
        for (i, w) in widths.iter().enumerate() {
            let (line, filled, align) = match wrapped[i].get(k) {
                Some(l) => {
                    let filled: usize = l.spans.iter().map(|s| s.content.width()).sum();
                    let align = if wrapped[i].len() == 1 { aligns[i] } else { TAlign::Left };
                    (l.clone(), filled, align)
                }
                None => (Line::default(), 0, TAlign::Left),
            };
            let (pl, pr) = pad_split(align, w.saturating_sub(filled));
            spans.push(Span::styled(" ".repeat(pl + 1), pad));
            spans.extend(line.spans);
            spans.push(Span::styled(" ".repeat(pr + 1), pad));
            spans.push(Span::styled("│".to_string(), border));
        }
        lines.push(prefix_line(Line::from(spans), &row.prefix));
    }
    lines
}

/// Split `free` padding columns across the alignment's two sides.
fn pad_split(align: TAlign, free: usize) -> (usize, usize) {
    match align {
        TAlign::Left => (0, free),
        TAlign::Center => (free / 2, free - free / 2),
        TAlign::Right => (free, 0),
    }
}

/// Repaint the buffered block as tui-markdown drew it: two-tone body,
/// `├─┼─┤` junctions between consecutive body rows, and — when the block is
/// not a well-formed frame (degenerate fallback) — ellipsis truncation.
/// Each line re-applies its own quote/list prefix.
fn paint_fit(buf: &TableBuffer, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut prev_row = false;
    for line in &buf.lines {
        let available = width.saturating_sub(line.prefix.iter().map(Span::width).sum::<usize>());
        let row = truncate_line(two_tone(line.segs.clone(), theme), available, theme);
        let is_row = row.spans.first().and_then(|s| s.content.chars().next()) == Some('│');
        if is_row && prev_row {
            let sep = table_row_separator(&row, theme);
            out.push(prefix_line(sep, &line.prefix));
        }
        prev_row = is_row;
        out.push(prefix_line(row, &line.prefix));
    }
    out
}

/// The `├─┼─┤` junction between table body rows, derived from the row's own
/// column layout in *display columns* (CJK/emoji cells are two columns wide)
/// so separators always line up with the cells.
fn table_row_separator(row: &Line, theme: &Theme) -> Line<'static> {
    // Display columns where the row's `│` borders sit.
    let mut bars: Vec<usize> = Vec::new();
    let mut w = 0usize;
    for s in &row.spans {
        for c in s.content.graphemes(true) {
            if c == "│" {
                bars.push(w);
            }
            w += UnicodeWidthStr::width(c);
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
        for c in lang.graphemes(true) {
            let cw = UnicodeWidthStr::width(c);
            if used + cw > max {
                break;
            }
            used += cw;
            cut += c.len();
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
        for c in seg.text.graphemes(true) {
            let cw = UnicodeWidthStr::width(c);
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
            buf.push_str(c);
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
    let fits = segments_width(&segs) <= width;
    let budget = if fits { width } else { width.saturating_sub(1) };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut w = 0usize;
    let mut stopped = false;
    for seg in segs {
        let mut buf = String::new();
        for c in seg.text.graphemes(true) {
            let cw = UnicodeWidthStr::width(c);
            if w + cw > budget {
                stopped = true;
                break;
            }
            w += cw;
            buf.push_str(c);
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
/// The width is exact: callers that need a floor apply it themselves (the
/// table layout must not exceed the shrunk column widths).
fn wrap_segments(segs: Vec<Seg>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
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
    for (i, ch) in s.grapheme_indices(true) {
        let cw = UnicodeWidthStr::width(ch);
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
