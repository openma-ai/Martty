//! The composer line editor: buffer + cursor (char-indexed), prompt
//! history, and the word/kill motions the keymap dispatches. No app or
//! terminal knowledge — pure string surgery, unit-tested.

use unicode_width::UnicodeWidthChar;

/// Editable prompt state. `cursor` is a char index (not bytes).
pub struct Input {
    pub buf: String,
    pub cursor: usize, // char index
    pub history: Vec<String>,
    pub hist_pos: Option<usize>,
    pub stash: String,
    preferred_visual_col: Option<usize>,
    cursor_at_wrap_end: bool,
}

impl Input {
    pub fn new() -> Self {
        Input {
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            stash: String::new(),
            preferred_visual_col: None,
            cursor_at_wrap_end: false,
        }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buf
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buf.len())
    }

    /// Char count of the buffer (cursor upper bound).
    pub fn len_chars(&self) -> usize {
        self.buf.chars().count()
    }

    pub fn insert(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.buf.insert(at, ch);
        self.cursor += 1;
        self.hist_pos = None;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn insert_str(&mut self, s: &str) {
        let at = self.byte_at(self.cursor);
        self.buf.insert_str(at, s);
        self.cursor += s.chars().count();
        self.hist_pos = None;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor -= 1;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn delete_word_back(&mut self) {
        let i = self.prev_word();
        let start = self.byte_at(i);
        let end = self.byte_at(self.cursor);
        self.buf.replace_range(start..end, "");
        self.cursor = i;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    /// Kill from the cursor to the end of the current rendered row.
    pub fn kill_to_end(&mut self, width: usize) {
        let start = self.cursor;
        let cursor_at_wrap_end = self.cursor_at_wrap_end;
        self.move_to_visual_line_end(width);
        let end = self.cursor;
        if start == end {
            self.cursor_at_wrap_end = cursor_at_wrap_end;
            return;
        }
        let start_byte = self.byte_at(start);
        let end_byte = self.byte_at(end);
        self.buf.replace_range(start_byte..end_byte, "");
        self.cursor = start;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    /// Kill from the start of the current rendered row to the cursor.
    pub fn kill_to_start(&mut self, width: usize) {
        let end = self.cursor;
        let cursor_at_wrap_end = self.cursor_at_wrap_end;
        self.move_to_visual_line_start(width);
        let start = self.cursor;
        if start == end {
            self.cursor_at_wrap_end = cursor_at_wrap_end;
            return;
        }
        let start_byte = self.byte_at(start);
        let end_byte = self.byte_at(end);
        self.buf.replace_range(start_byte..end_byte, "");
        self.cursor = start;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len_chars() {
            return;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.buf.replace_range(start..end, "");
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    /// Cursor position one word left (whitespace-delimited).
    pub fn prev_word(&self) -> usize {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut i = self.cursor.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    /// Cursor position one word right.
    pub fn next_word(&self) -> usize {
        let chars: Vec<char> = self.buf.chars().collect();
        let n = chars.len();
        let mut i = self.cursor.min(n);
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        while i < n && !chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.hist_pos = None;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn set(&mut self, s: String) {
        self.cursor = s.chars().count();
        self.buf = s;
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Forget the sticky display column used by repeated vertical motions.
    pub fn reset_vertical_goal(&mut self) {
        self.preferred_visual_col = None;
        self.cursor_at_wrap_end = false;
    }

    /// Number of hard/soft-wrapped visual rows at `width` display cells.
    pub fn visual_row_count(&self, width: usize) -> usize {
        let (_, _, rows) = self.visual_layout(width.max(1));
        rows
    }

    /// Map a click at display `(row, col)` inside a text area of `width`
    /// display cells to the caret boundary, using the same soft-wrap layout
    /// as the renderer (`visual_layout`). A click lands on the boundary
    /// *before* the char occupying the clicked cell (typing then inserts at
    /// the clicked position); wide chars split at their midpoint — the left
    /// half selects the boundary before the char, the right half the
    /// boundary after it. Blank cells snap to their row's start/end
    /// boundary; rows beyond the text snap to the end of the buffer.
    pub fn char_index_at(&self, width: usize, row: usize, col: usize) -> usize {
        let width = width.max(1);
        let (chars, carets, rows) = self.visual_layout(width);
        if row >= rows {
            return self.len_chars();
        }
        let (first, last) = Self::row_boundaries(&carets, row);
        let Some(first) = first else {
            return self.len_chars();
        };
        // The char occupying the clicked cell, if any.
        for i in first..=last {
            if i >= chars.len() {
                break;
            }
            let start = carets[i].1;
            let w = UnicodeWidthChar::width(chars[i]).unwrap_or(0).max(1);
            if col >= start && col < start + w {
                if w > 1 && col >= start + w / 2 {
                    return i + 1; // right half of a wide char → after it
                }
                return i; // narrow char (or left half) → before it
            }
        }
        // Blank cells: the row's leading edge or its trailing blank.
        if col < carets[first].1 {
            first
        } else {
            last
        }
    }

    /// The boundary *after* the char occupying display cell `(row, col)` —
    /// the inclusive end of a selection covering that cell, so a drag in
    /// either direction covers exactly the dragged cells. Blank cells snap
    /// to their row's end; rows beyond the text snap to the buffer end.
    pub fn char_index_end_at(&self, width: usize, row: usize, col: usize) -> usize {
        let width = width.max(1);
        let (chars, carets, rows) = self.visual_layout(width);
        if row >= rows {
            return self.len_chars();
        }
        let (first, last) = Self::row_boundaries(&carets, row);
        let Some(first) = first else {
            return self.len_chars();
        };
        for i in first..=last {
            if i >= chars.len() {
                break;
            }
            let start = carets[i].1;
            let w = UnicodeWidthChar::width(chars[i]).unwrap_or(0).max(1);
            if col >= start && col < start + w {
                return i + 1;
            }
        }
        if col < carets[first].1 {
            first
        } else {
            last
        }
    }

    /// First and last caret boundary indices on a layout row.
    fn row_boundaries(carets: &[(usize, usize)], row: usize) -> (Option<usize>, usize) {
        let mut first = None;
        let mut last = 0usize;
        for (i, &(r, _)) in carets.iter().enumerate() {
            if r == row {
                if first.is_none() {
                    first = Some(i);
                }
                last = i;
            }
        }
        (first, last)
    }

    /// The text between two char boundaries (unordered), for copying a
    /// drag selection. Out-of-range indices clamp to the buffer.
    pub fn chars_between(&self, a: usize, b: usize) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        self.buf.chars().skip(a).take(b.saturating_sub(a)).collect()
    }

    /// Move by one visual row while preserving the original display column.
    /// Hard newlines and soft wraps share the same caret map as the renderer.
    pub fn move_vertical(&mut self, width: usize, direction: i8) {
        let width = width.max(1);
        let (candidates, rows) = self.visual_candidates(width);
        let (row, col) = self.visual_cursor(width);
        let goal = *self.preferred_visual_col.get_or_insert(col);
        let target = if direction < 0 {
            row.checked_sub(1)
        } else if direction > 0 && row + 1 < rows {
            Some(row + 1)
        } else {
            None
        };
        let Some(target) = target else {
            return;
        };

        let mut best: Option<(usize, usize, bool)> = None;
        for &(index, candidate_row, candidate_col, wrap_end) in &candidates {
            if candidate_row != target {
                continue;
            }
            let distance = candidate_col.abs_diff(goal);
            if best.is_none_or(|(_, best_distance, _)| distance < best_distance) {
                best = Some((index, distance, wrap_end));
            }
        }
        if let Some((index, _, wrap_end)) = best {
            self.cursor = index;
            self.cursor_at_wrap_end = wrap_end;
        }
    }

    /// Move to the beginning of the current rendered row, not the beginning
    /// of the whole multiline draft.
    pub fn move_to_visual_line_start(&mut self, width: usize) {
        let width = width.max(1);
        let row = self.visual_cursor(width).0;
        if let Some(&(index, _, _, wrap_end)) = self
            .visual_candidates(width)
            .0
            .iter()
            .filter(|&&(_, candidate_row, _, _)| candidate_row == row)
            .min_by_key(|&&(_, _, col, _)| col)
        {
            self.cursor = index;
            self.cursor_at_wrap_end = wrap_end;
        }
        self.preferred_visual_col = None;
    }

    /// Move to the end of the current rendered row. A soft-wrap boundary has
    /// two visual affinities; retain the upstream one so the cursor remains
    /// visibly at this row's end instead of appearing on the next row.
    pub fn move_to_visual_line_end(&mut self, width: usize) {
        let width = width.max(1);
        let row = self.visual_cursor(width).0;
        if let Some(&(index, _, _, wrap_end)) = self
            .visual_candidates(width)
            .0
            .iter()
            .filter(|&&(_, candidate_row, _, _)| candidate_row == row)
            .max_by_key(|&&(_, _, col, _)| col)
        {
            self.cursor = index;
            self.cursor_at_wrap_end = wrap_end;
        }
        self.preferred_visual_col = None;
    }

    /// Current rendered caret position, including upstream affinity at a
    /// soft-wrap line end.
    pub fn visual_cursor(&self, width: usize) -> (usize, usize) {
        let width = width.max(1);
        let (chars, carets, _) = self.visual_layout(width);
        let index = self.cursor.min(carets.len() - 1);
        if self.cursor_at_wrap_end {
            if let Some(position) = Self::wrap_end_position(index, &chars, &carets) {
                return position;
            }
        }
        carets[index]
    }

    /// All visual affinities for character boundaries. Soft wraps contribute
    /// both the previous row's end and the next row's start.
    fn visual_candidates(&self, width: usize) -> (Vec<(usize, usize, usize, bool)>, usize) {
        let (chars, carets, rows) = self.visual_layout(width);
        let mut candidates = Vec::with_capacity(carets.len() * 2);
        for (index, &(row, col)) in carets.iter().enumerate() {
            candidates.push((index, row, col, false));
            if let Some((upstream_row, upstream_col)) =
                Self::wrap_end_position(index, &chars, &carets)
            {
                candidates.push((index, upstream_row, upstream_col, true));
            }
        }
        (candidates, rows)
    }

    fn wrap_end_position(
        index: usize,
        chars: &[char],
        carets: &[(usize, usize)],
    ) -> Option<(usize, usize)> {
        let previous = *chars.get(index.checked_sub(1)?)?;
        if previous == '\n' {
            return None;
        }
        let (row, col) = carets[index - 1];
        let end = col + UnicodeWidthChar::width(previous).unwrap_or(0).max(1);
        (carets[index].0 > row).then_some((row, end))
    }

    /// Characters plus caret `(row, display-column)` for every boundary.
    fn visual_layout(&self, width: usize) -> (Vec<char>, Vec<(usize, usize)>, usize) {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut carets = vec![(0, 0); chars.len() + 1];
        let (mut row, mut col) = (0usize, 0usize);
        for (index, ch) in chars.iter().copied().enumerate() {
            if ch == '\n' {
                carets[index] = (row, col);
                row += 1;
                col = 0;
                carets[index + 1] = (row, col);
                continue;
            }
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
            if col + char_width > width {
                row += 1;
                col = 0;
            }
            carets[index] = (row, col);
            col += char_width;
            carets[index + 1] = (row, col);
        }
        if !chars.is_empty() && chars.last() != Some(&'\n') && col >= width {
            row += 1;
            carets[chars.len()] = (row, 0);
        }
        (chars, carets, row + 1)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input__editor__tests.rs"]
mod tests;
