//! Composer editor backed by `ratatui-textarea`, plus the layout mirror
//! (`LayoutMap`) the widget does not expose: screen-cell ↔ char-offset
//! mapping for click/drag hit-testing, per-cell coordinates for inline
//! `[image n]` chip restyling, visual-line (soft-wrap) motions, and the
//! cursor-following scroll mirror.
//!
//! Editing state (buffer + cursor + undo history) lives in the inner
//! [`TextArea`]; prompt history stays here so it survives buffer clears,
//! exactly like the old hand-rolled `Input`. The layout mirror is rebuilt
//! lazily after every edit (and on every draw) from the same glyph-wrap
//! rules `ratatui-textarea` uses internally, so hit-testing and rendering
//! can never disagree with the widget's own screen map.

use ratatui_textarea::{CursorMove, DataCursor, TextArea};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// Tab stops, matching the widget's default `tab_length`.
const TAB_LEN: u8 = 4;

/// One grapheme's placement in the mirrored layout: its buffer char range
/// (global offsets, newlines counted) and its display-cell extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphemeLayout {
    pub start_char: usize,
    pub end_char: usize,
    pub start_col: usize,
    pub width: usize,
}

/// One soft-wrapped screen row of the mirrored layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowLayout {
    pub line: usize,
    /// Buffer char offset of the first grapheme on this row.
    pub start_char: usize,
    /// Buffer char offset just past the last grapheme on this row.
    pub end_char: usize,
    pub graphemes: Vec<GraphemeLayout>,
}

/// Screen-cell ↔ char-offset mirror of the widget's internal screen map,
/// rebuilt from the same glyph-wrap algorithm (`wrap.rs` in
/// ratatui-textarea): grapheme clusters, tab stops, and display-cell
/// widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutMap {
    pub rows: Vec<RowLayout>,
    /// Char count of the whole buffer, newlines included.
    pub total_chars: usize,
}

/// Display width of one grapheme starting at display column `col`
/// (tab advances to the next stop; control chars have no width).
fn grapheme_width(grapheme: &str, col: usize) -> usize {
    let mut width = 0usize;
    for c in grapheme.chars() {
        if c == '\t' {
            let tab = TAB_LEN as usize;
            width += tab - (col + width) % tab;
        } else {
            width += c.width().unwrap_or(0);
        }
    }
    width
}

impl LayoutMap {
    pub fn new(lines: &[String], width: usize) -> Self {
        let width = width.max(1);
        let mut rows: Vec<RowLayout> = Vec::new();
        let mut total_chars = 0usize;

        for (line_idx, line) in lines.iter().enumerate() {
            let line_chars = line.chars().count();
            let mut graphemes: Vec<GraphemeLayout> = Vec::new();
            let mut row_start = total_chars;
            let mut row_chars = 0usize;
            let mut col = 0usize;
            // Take the row on a grapheme that would overflow it; a
            // grapheme wider than the row stands alone on its own row.
            let flush = |rows: &mut Vec<RowLayout>,
                         line_idx: usize,
                         row_start: &mut usize,
                         row_chars: &mut usize,
                         graphemes: &mut Vec<GraphemeLayout>| {
                rows.push(RowLayout {
                    line: line_idx,
                    start_char: *row_start,
                    end_char: *row_start + *row_chars,
                    graphemes: std::mem::take(graphemes),
                });
                *row_start += *row_chars;
                *row_chars = 0;
            };

            for (byte, g) in line.grapheme_indices(true) {
                let _ = byte;
                let chars = g.chars().count();
                let mut w = grapheme_width(g, col);
                if !graphemes.is_empty() && col + w > width {
                    flush(
                        &mut rows,
                        line_idx,
                        &mut row_start,
                        &mut row_chars,
                        &mut graphemes,
                    );
                    col = 0;
                    w = grapheme_width(g, col);
                }
                graphemes.push(GraphemeLayout {
                    start_char: row_start + row_chars,
                    end_char: row_start + row_chars + chars,
                    start_col: col,
                    width: w,
                });
                col += w;
                row_chars += chars;
                if col > width {
                    flush(
                        &mut rows,
                        line_idx,
                        &mut row_start,
                        &mut row_chars,
                        &mut graphemes,
                    );
                    col = 0;
                }
            }
            if !graphemes.is_empty() {
                rows.push(RowLayout {
                    line: line_idx,
                    start_char: row_start,
                    end_char: row_start + row_chars,
                    graphemes,
                });
            } else {
                // Empty logical line still occupies one screen row.
                rows.push(RowLayout {
                    line: line_idx,
                    start_char: row_start,
                    end_char: row_start,
                    graphemes: Vec::new(),
                });
            }
            total_chars += line_chars + 1; // + the newline separator
        }

        // The final newline separator does not exist in the buffer.
        total_chars = total_chars.saturating_sub(1);
        LayoutMap { rows, total_chars }
    }

    /// Number of soft-wrapped screen rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Char boundary *before* the grapheme occupying display cell
    /// `(row, col)` — a click then inserts at the clicked position. Wide
    /// graphemes split at their midpoint: the left half selects the
    /// boundary before it, the right half the boundary after. Blank cells
    /// snap to their row's end; rows beyond the text to the buffer end.
    pub fn screen_to_char(&self, row: usize, col: usize) -> usize {
        let Some(r) = self.rows.get(row) else {
            return self.total_chars;
        };
        for g in &r.graphemes {
            let w = g.width.max(1);
            if col >= g.start_col && col < g.start_col + w {
                if w > 1 && col >= g.start_col + w / 2 {
                    return g.end_char;
                }
                return g.start_char;
            }
        }
        r.end_char
    }

    /// The boundary *after* the grapheme occupying display cell
    /// `(row, col)` — the inclusive end of a drag selection covering that
    /// cell. Blank cells snap to their row's end.
    pub fn screen_to_char_end(&self, row: usize, col: usize) -> usize {
        let Some(r) = self.rows.get(row) else {
            return self.total_chars;
        };
        for g in &r.graphemes {
            if col >= g.start_col && col < g.start_col + g.width.max(1) {
                return g.end_char;
            }
        }
        r.end_char
    }
}

/// The composer editor: a `ratatui-textarea` widget plus prompt history
/// and the layout mirror. Keeps the same method surface the old
/// hand-rolled `Input` exposed where app code used it.
pub struct ComposerEditor {
    textarea: TextArea<'static>,
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub stash: String,
    layout: Option<LayoutMap>,
    /// Mirrored cursor-following scroll top, kept in lockstep with the
    /// widget's internal viewport (same `next_scroll_top` recurrence).
    pub scroll_top: usize,
}

impl ComposerEditor {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_wrap_mode(ratatui_textarea::WrapMode::Glyph);
        // The composer draws its own selection highlight and cursor line;
        // disable the widget's underline so it cannot fight the theme. The
        // keyboard selection uses reversed cells, like the mouse drag.
        textarea.set_cursor_line_style(ratatui::style::Style::default());
        textarea.set_selection_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );
        ComposerEditor {
            textarea,
            history: Vec::new(),
            hist_pos: None,
            stash: String::new(),
            layout: None,
            scroll_top: 0,
        }
    }

    pub fn textarea_mut(&mut self) -> &mut TextArea<'static> {
        self.layout = None;
        &mut self.textarea
    }

    // --- text access ----------------------------------------------------

    pub fn lines(&self) -> &[String] {
        self.textarea.lines()
    }

    /// Whole draft as one string (newlines preserved).
    pub fn buf(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Same semantics as the old buffer check: empty iff no text at all
    /// (a lone newline does not count as empty).
    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// Global char offset of the cursor (newlines counted).
    pub fn cursor_char(&self) -> usize {
        let DataCursor(row, col) = self.textarea.cursor();
        self.lines()
            .iter()
            .take(row)
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            + col
    }

    /// Global char offset → `(line, col)` in the line model.
    pub fn char_to_rowcol(&self, mut offset: usize) -> (usize, usize) {
        let lines = self.lines();
        for (row, line) in lines.iter().enumerate() {
            let n = line.chars().count();
            if offset <= n {
                return (row, offset);
            }
            offset -= n + 1;
        }
        let last = lines.len().saturating_sub(1);
        (last, lines[last].chars().count())
    }

    /// The text between two char boundaries (unordered), for copying a
    /// drag selection.
    pub fn chars_between(&self, a: usize, b: usize) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let buf = self.buf();
        buf.chars().skip(a).take(b.saturating_sub(a)).collect()
    }

    // --- editing --------------------------------------------------------

    pub fn insert_char(&mut self, ch: char) {
        self.textarea_mut().insert_char(ch);
        self.hist_pos = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.textarea_mut().insert_str(s);
        self.hist_pos = None;
    }

    pub fn insert_newline(&mut self) {
        self.textarea_mut().insert_newline();
        self.hist_pos = None;
    }

    pub fn backspace(&mut self) {
        self.textarea_mut().cancel_selection();
        self.textarea_mut().delete_char();
        self.hist_pos = None;
    }

    pub fn delete_forward(&mut self) {
        self.textarea_mut().cancel_selection();
        self.textarea_mut().delete_next_char();
        self.hist_pos = None;
    }

    pub fn delete_word_back(&mut self) {
        self.textarea_mut().delete_word();
        self.hist_pos = None;
    }

    /// Cut the whole buffer range `[start, end)` (char offsets) —
    /// deleting into an inline `[image n]` token.
    pub fn delete_char_range(&mut self, start: usize, end: usize) {
        let (row, col) = self.char_to_rowcol(start);
        self.move_cursor(CursorMove::Jump(row as u16, col as u16));
        self.textarea_mut().delete_str(end.saturating_sub(start));
        self.hist_pos = None;
    }

    /// Delete the whole logical line under the cursor (including its
    /// trailing newline, so the next line moves up). The cursor lands at
    /// the line start. A no-op on an empty last line.
    pub fn kill_line(&mut self) {
        let DataCursor(row, _) = self.textarea.cursor();
        let lines = self.lines();
        if row >= lines.len() {
            return;
        }
        let start = lines
            .iter()
            .take(row)
            .map(|l| l.chars().count() + 1)
            .sum::<usize>();
        let line_chars = lines[row].chars().count();
        let end = if row + 1 < lines.len() {
            start + line_chars + 1 // include the newline separator
        } else {
            start + line_chars
        };
        if end > start {
            self.delete_char_range(start, end);
        }
    }

    /// Undo the last draft edit (the widget keeps a 50-step history).
    /// Returns whether the buffer changed.
    pub fn undo(&mut self) -> bool {
        let done = self.textarea_mut().undo();
        if done {
            self.hist_pos = None;
        }
        done
    }

    /// Redo the last undone draft edit. Returns whether the buffer changed.
    pub fn redo(&mut self) -> bool {
        let done = self.textarea_mut().redo();
        if done {
            self.hist_pos = None;
        }
        done
    }

    /// Paste the widget's yank buffer (the last kill/delete_str) at the
    /// cursor. Returns whether anything was pasted.
    pub fn paste_yank(&mut self) -> bool {
        let done = self.textarea_mut().paste();
        if done {
            self.hist_pos = None;
        }
        done
    }

    pub fn clear(&mut self) {
        self.textarea_mut().clear();
        self.hist_pos = None;
        self.layout = None;
    }

    /// Replace the whole draft; the cursor lands at the end. Like the old
    /// editor's `set`, this does not touch `hist_pos` — history navigation
    /// relies on it surviving buffer replacement.
    pub fn set(&mut self, s: String) {
        let mut lines: Vec<String> = s.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.textarea = TextArea::new(lines);
        self.textarea.set_wrap_mode(ratatui_textarea::WrapMode::Glyph);
        self.textarea.set_cursor_line_style(ratatui::style::Style::default());
        self.textarea.set_selection_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );
        self.move_cursor(CursorMove::Bottom);
        self.move_cursor(CursorMove::End);
        self.layout = None;
        self.scroll_top = 0;
    }

    // --- cursor motions -------------------------------------------------

    fn move_cursor(&mut self, m: CursorMove) {
        self.textarea_mut().move_cursor(m);
    }

    /// Plain moves cancel an active keyboard selection (the widget would
    /// otherwise extend it).
    pub fn move_left(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Back);
    }

    pub fn move_right(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Forward);
    }

    /// Up/down move by *screen* row (soft wraps included) and keep the
    /// display column — the widget's own cursor model.
    pub fn move_up(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Up);
    }

    pub fn move_down(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Down);
    }

    pub fn word_left(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::WordBack);
    }

    pub fn word_right(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::WordForward);
    }

    /// Logical-line motions and jumps (vim normal mode).
    pub fn line_head(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Head);
    }

    pub fn line_tail(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::End);
    }

    /// Jump to the first line's head (vim `gg`), not the column-preserving
    /// `Top` motion.
    pub fn jump_top(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Jump(0, 0));
    }

    /// Jump to the last line's head (vim `G`).
    pub fn jump_bottom(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Jump(u16::MAX, 0));
    }

    pub fn word_end(&mut self) {
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::WordEnd);
    }

    pub fn set_cursor_char(&mut self, offset: usize) {
        let (row, col) = self.char_to_rowcol(offset);
        self.textarea_mut().cancel_selection();
        self.move_cursor(CursorMove::Jump(row as u16, col as u16));
    }

    // --- text selection (shift+move) --------------------------------------

    /// Anchor at the cursor and move — the first press starts the
    /// selection, later ones extend it. Plain moves, edits and clicks
    /// cancel it (see the move/edit wrappers below).
    fn select_move(&mut self, m: CursorMove) {
        let ta = self.textarea_mut();
        if !ta.is_selecting() {
            ta.start_selection();
        }
        ta.move_cursor(m);
    }

    pub fn select_left(&mut self) {
        self.select_move(CursorMove::Back);
    }

    pub fn select_right(&mut self) {
        self.select_move(CursorMove::Forward);
    }

    pub fn select_up(&mut self) {
        self.select_move(CursorMove::Up);
    }

    pub fn select_down(&mut self) {
        self.select_move(CursorMove::Down);
    }

    pub fn select_word_left(&mut self) {
        self.select_move(CursorMove::WordBack);
    }

    pub fn select_word_right(&mut self) {
        self.select_move(CursorMove::WordForward);
    }

    pub fn select_line_start(&mut self) {
        self.select_move(CursorMove::Head);
    }

    pub fn select_line_end(&mut self) {
        self.select_move(CursorMove::End);
    }

    /// The selected text, `None` without an active selection.
    pub fn selection_text(&self) -> Option<String> {
        let (s, e) = self.textarea.selection_range()?;
        let lines = self.textarea.lines();
        if s.0 == e.0 {
            return Some(lines[s.0].chars().skip(s.1).take(e.1 - s.1).collect());
        }
        let mut out = String::new();
        out.push_str(&lines[s.0].chars().skip(s.1).collect::<String>());
        for line in &lines[s.0 + 1..e.0] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&lines[e.0].chars().take(e.1).collect::<String>());
        Some(out)
    }

    /// Copy the selection into the widget's yank buffer.
    pub fn copy_selection_to_yank(&mut self) {
        self.textarea_mut().copy();
    }

    /// Cut the selection (delete it and keep it in the yank buffer).
    pub fn cut_selection_to_yank(&mut self) {
        self.textarea_mut().cut();
        self.hist_pos = None;
    }

    // --- visual-line (soft-wrap) motions ---------------------------------

    /// Mirror of the widget's screen map; rebuilt when stale.
    pub fn layout(&mut self, wrap_width: usize) -> &LayoutMap {
        if self.layout.is_none() {
            self.layout = Some(LayoutMap::new(self.lines(), wrap_width));
        }
        self.layout.as_ref().expect("layout built")
    }

    /// Char boundary before the grapheme at screen cell `(row, col)`
    /// (rows relative to the *start of the text*, scroll offset included).
    pub fn screen_to_char(&mut self, wrap_width: usize, row: usize, col: usize) -> usize {
        let layout = self.layout(wrap_width);
        layout.screen_to_char(row, col)
    }

    /// Boundary after the grapheme at `(row, col)` — the inclusive end of
    /// a drag selection covering that cell.
    pub fn screen_to_char_end(&mut self, wrap_width: usize, row: usize, col: usize) -> usize {
        let layout = self.layout(wrap_width);
        layout.screen_to_char_end(row, col)
    }

    pub fn visual_row_count(&self, wrap_width: usize) -> usize {
        LayoutMap::new(self.lines(), wrap_width).row_count()
    }

    /// The widget's cursor position on the full screen map: `(row, col)`
    /// relative to the *start of the text* (row 0), not the viewport.
    /// Requires the widget to have been rendered (or edited) at least
    /// once so its screen map matches the current area.
    fn screen_cursor(&self) -> (usize, usize) {
        let sc = self.textarea.screen_cursor();
        (sc.row, sc.col)
    }

    /// The mirrored screen row the cursor sits in: the *upstream* row at
    /// a soft-wrap boundary (the boundary character belongs to the next
    /// row in the widget's model, but the caret stays on this row's end),
    /// matching the old editor's wrap-end affinity. Independent of the
    /// widget's render state — the layout mirror drives it.
    fn cursor_screen_row(&mut self, wrap_width: usize) -> Option<usize> {
        let c = self.cursor_char();
        let layout = self.layout(wrap_width);
        layout
            .rows
            .iter()
            .position(|r| r.start_char <= c && c <= r.end_char)
    }

    /// Move to the beginning of the current rendered row (soft wrap or
    /// hard newline), not the beginning of the whole draft.
    pub fn line_start(&mut self, wrap_width: usize) {
        let start = self
            .cursor_screen_row(wrap_width)
            .and_then(|row| self.layout(wrap_width).rows.get(row).map(|r| r.start_char));
        let Some(start) = start else { return };
        self.set_cursor_char(start);
    }

    /// Move to the end of the current rendered row.
    pub fn line_end(&mut self, wrap_width: usize) {
        let end = self
            .cursor_screen_row(wrap_width)
            .and_then(|row| self.layout(wrap_width).rows.get(row).map(|r| r.end_char));
        let Some(end) = end else { return };
        self.set_cursor_char(end);
    }

    /// Kill from the cursor to the end of the current rendered row.
    pub fn kill_to_end(&mut self, wrap_width: usize) {
        let end = self
            .cursor_screen_row(wrap_width)
            .and_then(|row| self.layout(wrap_width).rows.get(row).map(|r| r.end_char));
        let Some(end) = end else { return };
        let cur = self.cursor_char();
        if end > cur {
            self.textarea_mut().delete_str(end - cur);
            self.hist_pos = None;
        }
    }

    /// Kill from the start of the current rendered row to the cursor.
    pub fn kill_to_start(&mut self, wrap_width: usize) {
        let start = self
            .cursor_screen_row(wrap_width)
            .and_then(|row| self.layout(wrap_width).rows.get(row).map(|r| r.start_char));
        let Some(start) = start else { return };
        let cur = self.cursor_char();
        if start < cur {
            let (r, c) = self.char_to_rowcol(start);
            self.textarea_mut()
                .move_cursor(CursorMove::Jump(r as u16, c as u16));
            self.textarea_mut().delete_str(cur - start);
            self.hist_pos = None;
        }
    }

    /// Advance the scroll mirror exactly like the widget's internal
    /// `next_scroll_top` recurrence — call after rendering the textarea
    /// for the frame, so clicks and the terminal cursor stay in sync.
    pub fn update_scroll_top(&mut self, height: u16) {
        let prev = self.scroll_top as u16;
        let cursor = self.screen_cursor().0 as u16;
        let next = if cursor < prev {
            cursor
        } else if prev + height <= cursor {
            cursor + 1 - height
        } else {
            prev
        };
        self.scroll_top = next as usize;
    }
}

impl Default for ComposerEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input__composer__tests.rs"]
mod tests;
