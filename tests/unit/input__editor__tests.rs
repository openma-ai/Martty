use super::*;

fn editor(text: &str) -> Input {
    let mut i = Input::new();
    i.set(text.into());
    i
}

#[test]
fn word_motions_hop_whitespace_delimited_words() {
    let mut i = editor("hello brave world");
    assert_eq!(i.next_word(), 17, "already at end");
    i.cursor = 0;
    assert_eq!(i.next_word(), 5);
    i.cursor = 5;
    assert_eq!(i.next_word(), 11);
    i.cursor = 17;
    assert_eq!(i.prev_word(), 12);
    i.cursor = 12;
    assert_eq!(i.prev_word(), 6);
}

#[test]
fn kill_commands_split_at_cursor() {
    let mut i = editor("hello world");
    i.cursor = 6;
    i.kill_to_start(80);
    assert_eq!(i.buf, "world");
    assert_eq!(i.cursor, 0);

    let mut i = editor("hello world");
    i.cursor = 5;
    i.kill_to_end(80);
    assert_eq!(i.buf, "hello");
}

#[test]
fn delete_forward_and_multibyte_safety() {
    let mut i = editor("a中b");
    i.cursor = 1;
    i.delete_forward();
    assert_eq!(i.buf, "ab");
    assert_eq!(i.cursor, 1);
    i.delete_forward();
    assert_eq!(i.buf, "a");
    i.delete_forward();
    assert_eq!(i.buf, "a", "at end: no-op");
}

#[test]
fn delete_word_back_eats_trailing_whitespace_then_word() {
    let mut i = editor("one two   ");
    i.delete_word_back();
    assert_eq!(i.buf, "one ");
    assert_eq!(i.cursor, 4);
}

#[test]
fn vertical_motion_preserves_the_display_column_across_hard_lines() {
    let mut i = editor("abcd\nefgh\nij");

    i.move_vertical(20, -1);
    assert_eq!(i.cursor, 7, "row 2, display column 2");
    i.move_vertical(20, -1);
    assert_eq!(i.cursor, 2, "row 1, display column 2");
    i.move_vertical(20, 1);
    i.move_vertical(20, 1);
    assert_eq!(i.cursor, 12, "the preferred column survives round-trips");
}

#[test]
fn vertical_motion_uses_soft_wrapped_visual_rows() {
    let mut i = editor("abcdefghij");

    i.move_vertical(4, -1);
    assert_eq!(i.cursor, 6, "wrapped row 2, display column 2");
    i.move_vertical(4, -1);
    assert_eq!(i.cursor, 2, "wrapped row 1, display column 2");
}

#[test]
fn char_index_at_places_caret_before_the_clicked_char() {
    let i = editor("hello world");
    assert_eq!(i.char_index_at(80, 0, 0), 0, "click on 'h' → before it");
    assert_eq!(i.char_index_at(80, 0, 2), 2, "click on 'l' → before it");
    assert_eq!(i.char_index_at(80, 0, 10), 10, "click on 'd' → before it");
    assert_eq!(i.char_index_at(80, 0, 11), 11, "blank past the end → end");
    assert_eq!(
        i.char_index_at(80, 0, 40),
        11,
        "blank far past the end → end"
    );
    assert_eq!(i.char_index_at(80, 1, 3), 11, "row below the text → end");
    assert_eq!(i.char_index_at(80, 9, 0), 11, "far below the text → end");
}

#[test]
fn char_index_at_splits_wide_chars_at_their_midpoint() {
    let i = editor("a中b");
    assert_eq!(i.char_index_at(80, 0, 0), 0, "'a' → before it");
    assert_eq!(i.char_index_at(80, 0, 1), 1, "left half of 中 → before it");
    assert_eq!(i.char_index_at(80, 0, 2), 2, "right half of 中 → after it");
    assert_eq!(i.char_index_at(80, 0, 3), 2, "'b' → before it");
    assert_eq!(i.char_index_at(80, 0, 4), 3, "blank → end");
}

#[test]
fn char_index_at_follows_soft_wraps() {
    let i = editor("abcdefghij");
    // Width 4 wraps to abcd / efgh / ij.
    assert_eq!(i.char_index_at(4, 0, 3), 3, "'d' at the wrap edge");
    assert_eq!(i.char_index_at(4, 1, 0), 4, "'e' → the wrap boundary");
    assert_eq!(i.char_index_at(4, 1, 2), 6, "'g'");
    assert_eq!(i.char_index_at(4, 1, 3), 7, "'h' at the second wrap edge");
    assert_eq!(i.char_index_at(4, 2, 1), 9, "'j'");
    assert_eq!(i.char_index_at(4, 2, 2), 10, "blank after 'j' → end");
    assert_eq!(i.char_index_at(4, 3, 0), 10, "row below → end");
}

#[test]
fn char_index_at_handles_hard_newlines_and_empty_buffers() {
    let i = editor("ab\ncd");
    assert_eq!(i.char_index_at(80, 0, 0), 0, "'a' → before it");
    assert_eq!(
        i.char_index_at(80, 0, 2),
        2,
        "end of the first line (before \\n)"
    );
    assert_eq!(
        i.char_index_at(80, 1, 0),
        3,
        "'c' → start of the second line"
    );
    assert_eq!(i.char_index_at(80, 1, 2), 5, "'d' → end of the text");
    assert_eq!(i.char_index_at(80, 2, 5), 5, "below the text → end");

    let empty = editor("");
    assert_eq!(empty.char_index_at(80, 0, 7), 0, "empty draft");
}

#[test]
fn char_index_end_at_covers_the_clicked_cell_inclusively() {
    let i = editor("hello world");
    assert_eq!(
        i.char_index_end_at(80, 0, 2),
        3,
        "drag end on 'l' → after it"
    );
    assert_eq!(i.char_index_end_at(80, 0, 10), 11, "drag end on 'd' → end");
    assert_eq!(i.char_index_end_at(80, 0, 40), 11, "blank far past → end");
    assert_eq!(i.char_index_end_at(80, 1, 0), 11, "row below → end");

    let wide = editor("a中b");
    assert_eq!(
        wide.char_index_end_at(80, 0, 1),
        2,
        "left half of 中 → whole char"
    );
    assert_eq!(
        wide.char_index_end_at(80, 0, 2),
        2,
        "right half of 中 → after it"
    );
    assert_eq!(wide.char_index_end_at(80, 0, 3), 3, "'b' → after it");
}

#[test]
fn char_index_end_at_follows_soft_wraps() {
    let i = editor("abcdefghij");
    assert_eq!(i.char_index_end_at(4, 0, 3), 4, "'d' → the wrap boundary");
    assert_eq!(i.char_index_end_at(4, 1, 0), 5, "'e' → after it");
    assert_eq!(i.char_index_end_at(4, 2, 1), 10, "'j' → end");
    assert_eq!(i.char_index_end_at(4, 3, 0), 10, "row below → end");
}

#[test]
fn chars_between_slices_by_char_boundaries() {
    let i = editor("a中b");
    assert_eq!(i.chars_between(0, 2), "a中");
    assert_eq!(i.chars_between(2, 0), "a中", "unordered drags agree");
    assert_eq!(i.chars_between(1, 2), "中");
    assert_eq!(i.chars_between(3, 3), "", "caret-only");
    assert_eq!(i.chars_between(2, 99), "b", "clamped to the buffer");
}
