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
    i.kill_to_start();
    assert_eq!(i.buf, "world");
    assert_eq!(i.cursor, 0);

    let mut i = editor("hello world");
    i.cursor = 5;
    i.kill_to_end();
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
