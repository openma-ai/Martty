use crate::input::composer::{ComposerEditor, LayoutMap};

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|s| s.to_string()).collect()
}

/// Render once so the widget's screen map knows its area (the mirror of
/// the visual-line motions and scroll top read it).
fn render(e: &ComposerEditor, width: u16) {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;
    let rect = Rect::new(0, 0, width, 20);
    let mut buf = Buffer::empty(rect);
    e.textarea.render(rect, &mut buf);
}

// --- LayoutMap: wrap ----------------------------------------------------

#[test]
fn glyph_wrap_breaks_at_width() {
    let map = LayoutMap::new(&lines(&["hello world"]), 5);
    assert_eq!(map.row_count(), 3);
    assert_eq!(map.rows[0].graphemes.len(), 5);
    assert_eq!(map.rows[1].graphemes.len(), 5);
    assert_eq!(map.rows[2].graphemes.len(), 1);
    assert_eq!(map.total_chars, 11);
}

#[test]
fn glyph_wrap_keeps_wide_char_together() {
    let map = LayoutMap::new(&lines(&["ab犬猫"]), 4);
    assert_eq!(map.row_count(), 2);
    // "ab犬" fits (2+2=4), "猫" wraps.
    assert_eq!(map.rows[0].graphemes.len(), 3);
    assert_eq!(map.rows[0].end_char, 3);
    assert_eq!(map.rows[1].graphemes.len(), 1);
}

#[test]
fn grapheme_cluster_stays_on_one_row() {
    // e + combining acute is one grapheme, width 1.
    let map = LayoutMap::new(&lines(&["e\u{301}x"]), 1);
    assert_eq!(map.row_count(), 2);
    assert_eq!(map.rows[0].graphemes.len(), 1);
    assert_eq!(map.rows[0].end_char, 2, "cluster covers two chars");
}

#[test]
fn tab_advances_to_stop() {
    let map = LayoutMap::new(&lines(&["\tX"]), 2);
    assert_eq!(map.row_count(), 2);
    let tab = &map.rows[0].graphemes[0];
    assert_eq!(tab.width, 4, "tab at col 0 jumps to stop 4");
    let x = &map.rows[1].graphemes[0];
    assert_eq!(x.width, 1);
}

#[test]
fn empty_line_occupies_one_row() {
    let map = LayoutMap::new(&lines(&["ab", "", "cd"]), 80);
    assert_eq!(map.row_count(), 3);
    assert_eq!(map.rows[1].graphemes.len(), 0);
    assert_eq!(map.rows[1].start_char, 3);
    assert_eq!(map.rows[1].end_char, 3);
    assert_eq!(map.total_chars, 6, "ab\\n + \\n + cd");
}

#[test]
fn wide_grapheme_stands_alone_on_overflow_row() {
    let map = LayoutMap::new(&lines(&["a猫b"]), 2);
    assert_eq!(map.row_count(), 3);
    assert_eq!(map.rows[0].graphemes.len(), 1, "a");
    assert_eq!(map.rows[1].graphemes.len(), 1, "猫 alone");
    assert_eq!(map.rows[2].graphemes.len(), 1, "b");
}

// --- LayoutMap: hit-testing ---------------------------------------------

#[test]
fn click_lands_before_the_clicked_grapheme() {
    let map = LayoutMap::new(&lines(&["hello"]), 80);
    assert_eq!(map.screen_to_char(0, 0), 0);
    assert_eq!(map.screen_to_char(0, 2), 2);
    assert_eq!(map.screen_to_char(0, 4), 4);
    assert_eq!(map.screen_to_char(0, 10), 5, "blank past end → end");
}

#[test]
fn click_right_half_of_wide_grapheme_lands_after_it() {
    let map = LayoutMap::new(&lines(&["a猫b"]), 80);
    // 猫 starts at col 1, width 2: left half → before, right half → after.
    assert_eq!(map.screen_to_char(0, 1), 1);
    assert_eq!(map.screen_to_char(0, 2), 2);
    assert_eq!(map.screen_to_char(0, 3), 2, "b is before it");
    assert_eq!(map.screen_to_char_end(0, 1), 2, "drag end covers 猫");
}

#[test]
fn click_maps_through_wrap_rows() {
    let map = LayoutMap::new(&lines(&["hello world"]), 5);
    assert_eq!(map.screen_to_char(1, 2), 7);
    assert_eq!(map.screen_to_char(2, 0), 10);
    assert_eq!(map.screen_to_char(2, 9), 11, "blank past end → end");
    assert_eq!(map.screen_to_char(9, 0), 11, "row beyond text → end");
}

#[test]
fn click_on_empty_lines() {
    let map = LayoutMap::new(&lines(&["ab", "", "cd"]), 80);
    assert_eq!(map.screen_to_char(1, 0), 3);
    assert_eq!(map.screen_to_char(1, 5), 3);
}

// --- ComposerEditor: buffer & cursor ------------------------------------

#[test]
fn cursor_char_counts_newlines() {
    let mut e = ComposerEditor::new();
    e.insert_str("ab\ncd");
    assert_eq!(e.buf(), "ab\ncd");
    assert_eq!(e.cursor_char(), 5);
    e.set_cursor_char(1);
    assert_eq!(e.textarea.cursor(), (0, 1));
    e.set_cursor_char(3);
    assert_eq!(e.textarea.cursor(), (1, 0), "newline boundary = next line head");
    e.set_cursor_char(4);
    assert_eq!(e.textarea.cursor(), (1, 1));
    e.set_cursor_char(99);
    assert_eq!(e.cursor_char(), 5, "clamps to buffer end");
}

#[test]
fn char_to_rowcol_round_trips() {
    let mut e = ComposerEditor::new();
    e.insert_str("ab\ncd\ne");
    for offset in 0..=6 {
        let (r, c) = e.char_to_rowcol(offset);
        let check = e
            .lines()
            .iter()
            .take(r)
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            + c;
        assert_eq!(check, offset, "offset {offset} → ({r},{c})");
    }
}

#[test]
fn set_and_clear_keep_newlines() {
    let mut e = ComposerEditor::new();
    e.set("a\nb\n".into());
    assert_eq!(e.lines(), ["a", "b", ""]);
    assert_eq!(e.buf(), "a\nb\n");
    assert_eq!(e.cursor_char(), 4, "cursor at end");
    e.clear();
    assert!(e.is_empty());
    assert_eq!(e.buf(), "");
}

#[test]
fn is_empty_matches_old_semantics() {
    let mut e = ComposerEditor::new();
    assert!(e.is_empty());
    e.insert_newline();
    assert!(!e.is_empty(), "a lone newline is not empty");
}

#[test]
fn visual_line_motions_follow_soft_wraps() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello world");
    render(&e, 5);
    e.set_cursor_char(6); // "w", first char of the wrapped row
    e.line_start(5);
    assert_eq!(e.cursor_char(), 5, "wrap boundary start");
    e.line_end(5);
    assert_eq!(e.cursor_char(), 5, "upstream affinity at the boundary");
    e.set_cursor_char(8);
    e.line_end(5);
    assert_eq!(e.cursor_char(), 10);
    e.line_start(5);
    assert_eq!(e.cursor_char(), 5, "back to the wrapped row head");
    e.line_start(5);
    assert_eq!(e.cursor_char(), 0, "and onward to the logical row head");
    e.line_start(5);
    assert_eq!(e.cursor_char(), 0, "idempotent");
}

#[test]
fn kill_to_end_stops_at_wrap_boundary() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello world");
    render(&e, 5);
    e.set_cursor_char(8);
    e.kill_to_end(5);
    assert_eq!(e.buf(), "hello wod");
    assert_eq!(e.cursor_char(), 8);
}

#[test]
fn kill_to_start_reaches_wrap_boundary() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello world");
    render(&e, 5);
    e.set_cursor_char(8);
    e.kill_to_start(5);
    assert_eq!(e.buf(), "hellorld", "kills the wrapped row head");
    assert_eq!(e.cursor_char(), 5);
}

#[test]
fn delete_char_range_cuts_tokens() {
    let mut e = ComposerEditor::new();
    e.insert_str("see [image 1] here");
    e.delete_char_range(4, 13);
    assert_eq!(e.buf(), "see  here");
    assert_eq!(e.cursor_char(), 4);
}

#[test]
fn scroll_top_mirror_follows_cursor() {
    let mut e = ComposerEditor::new();
    for line in ["a", "b", "c", "d", "e"] {
        e.insert_str(line);
        e.insert_newline();
    }
    // Six screen rows (last one empty); viewport of 3.
    render(&e, 80);
    e.set_cursor_char(e.buf().chars().count());
    e.update_scroll_top(3);
    assert_eq!(e.scroll_top, 3, "cursor on the last row → scroll");
    e.move_up();
    e.move_up();
    e.update_scroll_top(3);
    assert_eq!(e.scroll_top, 3, "cursor still inside the window");
    e.move_down();
    e.move_down();
    e.update_scroll_top(3);
    assert_eq!(e.scroll_top, 3);
    e.set_cursor_char(0);
    e.update_scroll_top(3);
    assert_eq!(e.scroll_top, 0, "cursor above view → scroll back");
}

#[test]
fn editing_invalidates_layout() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello");
    assert_eq!(e.layout(80).row_count(), 1);
    e.insert_char('!');
    assert_eq!(e.layout(80).row_count(), 1);
    assert_eq!(e.buf(), "hello!");
    e.backspace();
    assert_eq!(e.buf(), "hello");
}

#[test]
fn undo_redo_restores_buffer_and_cursor() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello");
    assert_eq!(e.buf(), "hello");
    e.backspace(); // "hell"
    assert_eq!(e.buf(), "hell");
    assert!(e.undo());
    assert_eq!(e.buf(), "hello");
    assert!(e.undo());
    assert_eq!(e.buf(), "");
    assert!(!e.undo(), "no more history");
    assert!(e.redo());
    assert_eq!(e.buf(), "hello");
    assert!(e.redo());
    assert_eq!(e.buf(), "hell");
    assert!(!e.redo());
}

#[test]
fn set_resets_the_undo_history() {
    let mut e = ComposerEditor::new();
    e.insert_str("one");
    e.set("two".into());
    assert!(!e.undo(), "set replaces the widget, wiping its history");
    assert_eq!(e.buf(), "two");
}

#[test]
fn shift_select_extends_and_plain_moves_cancel() {
    let mut e = ComposerEditor::new();
    e.insert_str("hello world");
    e.set_cursor_char(6);
    e.select_right(); // "w"
    e.select_right(); // "wo"
    assert_eq!(e.selection_text().as_deref(), Some("wo"));
    e.select_word_right(); // extend to end of word
    assert_eq!(e.selection_text().as_deref(), Some("world"));
    // plain move cancels the selection
    e.move_right();
    assert_eq!(e.selection_text(), None);
    // editing while selecting replaces nothing but cancels? insert keeps selection semantics:
    e.set_cursor_char(2);
    e.select_left();
    e.select_left();
    assert_eq!(e.selection_text().as_deref(), Some("he"));
    e.backspace();
    assert_eq!(
        e.buf(),
        "hello world",
        "backspace cancels the selection, then deletes before the cursor (at the selection head)"
    );
    assert_eq!(e.selection_text(), None);
}

#[test]
fn selection_spans_multiple_lines() {
    let mut e = ComposerEditor::new();
    e.insert_str("ab\ncd\nef");
    e.set_cursor_char(0);
    e.select_right();
    e.select_right();
    e.select_down();
    e.select_down();
    assert_eq!(e.selection_text().as_deref(), Some("ab\ncd\nef"));
    e.textarea_mut().cancel_selection();
    assert_eq!(e.selection_text(), None);
}

#[test]
fn kill_line_removes_the_logical_line_and_merges() {
    let mut e = ComposerEditor::new();
    e.insert_str("one\ntwo\nthree");
    e.set_cursor_char(5); // middle of "two"
    e.kill_line();
    assert_eq!(e.buf(), "one\nthree", "middle line + its newline go away");
    assert_eq!(e.cursor_char(), 4, "cursor at the merged line start");

    e.kill_line();
    assert_eq!(e.buf(), "one\n", "last content line dies");
    e.kill_line();
    assert_eq!(e.buf(), "one\n", "the trailing empty line is a no-op");

    e.set_cursor_char(0);
    e.kill_line();
    assert_eq!(e.buf(), "", "first line + newline go away");
    e.kill_line();
    assert_eq!(e.buf(), "", "empty draft: no-op");
}

#[test]
fn kill_line_on_the_last_line_keeps_its_newline_friends() {
    let mut e = ComposerEditor::new();
    e.insert_str("a\nb\n");
    e.set_cursor_char(e.buf().chars().count()); // end of the empty last line
    e.kill_line();
    assert_eq!(e.buf(), "a\nb\n", "empty last line is a no-op");
    e.set_cursor_char(2);
    e.kill_line();
    assert_eq!(e.buf(), "a\n", "last content line dies, newline stays");
}
