use crate::input::composer::ComposerEditor;
use crate::input::vim::{VimMode, VimState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode, m: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, m)
}

fn char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn editor(text: &str) -> ComposerEditor {
    let mut e = ComposerEditor::new();
    e.insert_str(text);
    e.set_cursor_char(0);
    e
}

#[test]
fn off_mode_passes_everything_through() {
    let mut v = VimState::default();
    let mut e = editor("hello");
    assert!(!v.handle_key(&char('h'), &mut e));
    assert_eq!(v.mode, VimMode::Off);
}

#[test]
fn insert_mode_only_consumes_esc() {
    let mut v = VimState::default();
    v.set(true);
    assert_eq!(v.mode, VimMode::Insert);
    let mut e = editor("hello");
    assert!(!v.handle_key(&char('h'), &mut e), "typing passes through");
    assert!(!v.handle_key(&key(KeyCode::Char('x'), KeyModifiers::CONTROL), &mut e));
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));
    assert_eq!(v.mode, VimMode::Normal);
}

#[test]
fn normal_motions() {
    let mut v = VimState::default();
    v.set(true);
    v.mode = VimMode::Normal;
    let mut e = editor("hello world");
    // l l → col 2, w → next word start (6), b → back (0), e → word end.
    assert!(v.handle_key(&char('l'), &mut e));
    assert!(v.handle_key(&char('l'), &mut e));
    assert_eq!(e.cursor_char(), 2);
    assert!(v.handle_key(&char('w'), &mut e));
    assert_eq!(e.cursor_char(), 6);
    assert!(v.handle_key(&char('b'), &mut e));
    assert_eq!(e.cursor_char(), 0);
    assert!(v.handle_key(&char('e'), &mut e));
    assert_eq!(e.cursor_char(), 4);
    // h moves by char.
    assert!(v.handle_key(&char('h'), &mut e));
    assert_eq!(e.cursor_char(), 3);
    // j/k move by screen line: needs multiple lines.
    let mut m = editor("one\ntwo\nthree");
    m.set_cursor_char(5); // middle of "two"
    assert!(v.handle_key(&char('k'), &mut m));
    assert_eq!(m.cursor_char(), 1, "up a screen line");
    assert!(v.handle_key(&char('j'), &mut m));
    assert_eq!(m.cursor_char(), 5, "down again");
    // 0 / $ line head/tail.
    assert!(v.handle_key(&char('$'), &mut e));
    assert_eq!(e.cursor_char(), 11);
    assert!(v.handle_key(&char('0'), &mut e));
    assert_eq!(e.cursor_char(), 0);
}

#[test]
fn dd_kills_the_line_and_gg_G_jump() {
    let mut v = VimState::default();
    v.set(true);
    v.mode = VimMode::Normal;
    let mut e = editor("one\ntwo\nthree");
    e.set_cursor_char(5); // middle of "two"
    assert!(v.handle_key(&char('d'), &mut e));
    assert_eq!(e.buf(), "one\ntwo\nthree", "single d is pending");
    assert!(v.handle_key(&char('d'), &mut e));
    assert_eq!(e.buf(), "one\nthree", "dd kills the line");
    assert!(v.handle_key(&char('x'), &mut e), "x consumes");
    assert_eq!(e.buf(), "one\nhree", "and clears the pending state");
    // gg → first line head, G → last line head.
    e.set_cursor_char(8);
    assert!(v.handle_key(&char('g'), &mut e));
    assert!(v.handle_key(&char('g'), &mut e));
    assert_eq!(e.cursor_char(), 0);
    assert!(v.handle_key(&char('G'), &mut e));
    assert_eq!(e.cursor_char(), 4, "last line head");
}

#[test]
fn insert_mode_entries_and_esc_round_trip() {
    let mut v = VimState::default();
    v.set(true);
    v.mode = VimMode::Normal;
    let mut e = editor("hello world");
    e.set_cursor_char(5);
    // i types at the cursor; esc returns to normal (the insert-mode key
    // itself is not consumed — the app keymap inserts it).
    assert!(v.handle_key(&char('i'), &mut e));
    assert_eq!(v.mode, VimMode::Insert);
    assert!(!v.handle_key(&char('X'), &mut e), "insert lets typing through");
    e.insert_char('X');
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));
    assert_eq!(v.mode, VimMode::Normal);
    assert_eq!(e.buf(), "helloX world");

    // a moves one char right first.
    e.set_cursor_char(5);
    assert!(v.handle_key(&char('a'), &mut e));
    assert_eq!(v.mode, VimMode::Insert);
    assert_eq!(e.cursor_char(), 6);
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));

    // A appends at line end.
    assert!(v.handle_key(&char('A'), &mut e));
    assert_eq!(v.mode, VimMode::Insert);
    assert_eq!(e.cursor_char(), e.buf().chars().count());
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));

    // o opens a line below, O above.
    assert!(v.handle_key(&char('o'), &mut e));
    assert_eq!(v.mode, VimMode::Insert);
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));
    assert_eq!(e.buf().chars().filter(|&c| c == '\n').count(), 1, "o adds a newline");
    assert!(v.handle_key(&char('O'), &mut e));
    assert_eq!(v.mode, VimMode::Insert);
    assert!(v.handle_key(&key(KeyCode::Esc, KeyModifiers::NONE), &mut e));
    assert_eq!(e.buf().chars().filter(|&c| c == '\n').count(), 2, "O adds another");
}

#[test]
fn unknown_keys_are_consumed_not_typed() {
    let mut v = VimState::default();
    v.set(true);
    v.mode = VimMode::Normal;
    let mut e = editor("hello");
    assert!(v.handle_key(&char('q'), &mut e));
    assert!(v.handle_key(&char('5'), &mut e));
    assert_eq!(e.buf(), "hello", "normal mode never types letters");
}

#[test]
fn ctrl_chords_fall_through_in_normal_mode() {
    let mut v = VimState::default();
    v.set(true);
    v.mode = VimMode::Normal;
    let mut e = editor("hello");
    assert!(!v.handle_key(&key(KeyCode::Char('x'), KeyModifiers::CONTROL), &mut e));
    assert!(!v.handle_key(&key(KeyCode::Left, KeyModifiers::SUPER), &mut e));
    assert!(!v.handle_key(&key(KeyCode::Enter, KeyModifiers::NONE), &mut e), "enter passes");
}
