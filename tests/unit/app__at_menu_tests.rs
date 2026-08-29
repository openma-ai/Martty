//! App-level `@file` menu tests: typing, key interception, token span
//! replacement, dismissal, drilling, vim and slash-menu coexistence.

use super::*;
use crate::bus::AppEvent;
use crate::controller::Controller;
use crate::runtime::RuntimeConfig;
use crate::theme::Theme;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// Unique session root per call — keeps persisted UI fixtures isolated.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-at-menu-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

/// A controlled workspace tree:
/// ```text
/// <ws>/
/// ├── src/
/// │   ├── nested/
/// │   └── main.rs
/// ├── README.md
/// └── notes.txt
/// ```
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "martty-at-menu-ws-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/nested")).expect("create tree");
        fs::write(root.join("src/main.rs"), "").expect("write");
        fs::write(root.join("README.md"), "").expect("write");
        fs::write(root.join("notes.txt"), "").expect("write");
        Self { root }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn test_app(workspace: &PathBuf) -> (App, Controller, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: workspace.to_string_lossy().into_owned(),
        session_root: fresh_root(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn char_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

#[test]
fn typing_at_opens_the_browser_and_enter_picks() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    assert!(app.file_menu.is_none());

    app.handle_key(char_key('@'), &ctl);
    let menu = app.file_menu.as_ref().expect("menu opens on @");
    // parent entry, then dirs, then files, each alphabetical
    let names: Vec<&str> = menu
        .explorer()
        .files()
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(names, ["../", "src/", "README.md", "notes.txt"]);

    // README.md is the first file after the parent and src/ dirs.
    app.handle_key(key(KeyCode::Down), &ctl);
    app.handle_key(key(KeyCode::Down), &ctl);
    app.handle_key(key(KeyCode::Enter), &ctl);

    assert!(app.file_menu.is_none(), "menu closes on settle");
    assert_eq!(app.input.buf(), "@README.md");
    assert_eq!(app.input.cursor_char(), "@README.md".chars().count());
}

#[test]
fn typing_extends_the_query_and_navigates() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(char_key('s'), &ctl);
    app.handle_key(char_key('r'), &ctl);
    app.handle_key(char_key('c'), &ctl);
    app.handle_key(char_key('/'), &ctl);
    app.handle_key(char_key('m'), &ctl);

    let menu = app.file_menu.as_ref().expect("menu stays open");
    assert_eq!(menu.explorer().cwd(), &ws.root.join("src"));
    assert_eq!(menu.explorer().current().name, "main.rs");
    assert_eq!(app.input.buf(), "@src/m");
}

#[test]
fn enter_after_query_navigation_inserts_the_full_path() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    for c in ['s', 'r', 'c', '/', 'm'] {
        app.handle_key(char_key(c), &ctl);
    }
    app.handle_key(key(KeyCode::Enter), &ctl);
    assert!(app.file_menu.is_none());
    assert_eq!(app.input.buf(), "@src/main.rs");
}

#[test]
fn tab_drills_into_a_directory_and_enter_settles_inside() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(key(KeyCode::Down), &ctl); // src/
    app.handle_key(key(KeyCode::Tab), &ctl);

    // The token was rewritten to @src/ and the browser moved inside.
    assert_eq!(app.input.buf(), "@src/");
    let menu = app.file_menu.as_ref().expect("menu stays open after drill");
    assert_eq!(menu.explorer().cwd(), &ws.root.join("src"));

    // Inside src/ the list is [../, nested/, main.rs].
    app.handle_key(key(KeyCode::Down), &ctl); // nested/
    app.handle_key(key(KeyCode::Down), &ctl); // main.rs
    app.handle_key(key(KeyCode::Enter), &ctl);
    assert!(app.file_menu.is_none());
    assert_eq!(app.input.buf(), "@src/main.rs");
}

#[test]
fn drill_updates_the_caret_and_reopens_on_typing() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(key(KeyCode::Down), &ctl); // src/
    app.handle_key(key(KeyCode::Tab), &ctl);
    assert_eq!(app.input.buf(), "@src/");
    // Caret lands after the rewritten token.
    assert_eq!(app.input.cursor_char(), "@src/".chars().count());
    // More typing extends the token inside the drilled dir.
    app.handle_key(char_key('n'), &ctl);
    assert_eq!(app.input.buf(), "@src/n");
    let menu = app.file_menu.as_ref().expect("menu re-syncs");
    assert_eq!(menu.explorer().current().name, "nested/");
}

#[test]
fn esc_dismisses_until_the_token_changes() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(char_key('r'), &ctl);
    app.handle_key(key(KeyCode::Esc), &ctl);
    assert!(app.file_menu.is_none(), "esc closes the browser");
    assert_eq!(app.input.buf(), "@r", "the draft text survives");

    // Same token, caret moves: stays dismissed.
    app.handle_key(key(KeyCode::Left), &ctl);
    assert!(app.file_menu.is_none());

    // Token text changes: the browser returns. The caret sat on the `@`
    // after Left, so the new char lands before the `r`.
    app.handle_key(char_key('e'), &ctl);
    assert!(app.file_menu.is_some(), "editing the token reopens the menu");
    assert_eq!(app.input.buf(), "@er");
}

#[test]
fn deleting_the_token_clears_the_dismissal() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(char_key('r'), &ctl);
    app.handle_key(key(KeyCode::Esc), &ctl);
    assert!(app.file_menu.is_none());

    // Deleting the whole token, then retyping the exact same text, must
    // reopen the browser — the dismissal only lives with the token.
    app.handle_key(key(KeyCode::Backspace), &ctl);
    app.handle_key(key(KeyCode::Backspace), &ctl);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(char_key('r'), &ctl);
    assert!(app.file_menu.is_some(), "fresh token reopens the browser");
}

#[test]
fn deleting_the_at_closes_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    assert!(app.file_menu.is_some());
    app.handle_key(key(KeyCode::Backspace), &ctl);
    assert!(app.file_menu.is_none());
    assert_eq!(app.input.buf(), "");
}

#[test]
fn email_like_text_never_opens_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    for c in ['h', 'i', '@', 'x'] {
        app.handle_key(char_key(c), &ctl);
    }
    assert!(app.file_menu.is_none(), "a@b must not trigger the menu");
    assert_eq!(app.input.buf(), "hi@x");
}

#[test]
fn slash_draft_does_not_open_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('/'), &ctl);
    app.handle_key(char_key('n'), &ctl);
    app.handle_key(char_key('e'), &ctl);
    // /ne matches /new → the slash menu owns the draft; the @ after a
    // word char is not a boundary anyway.
    assert!(app.slash_completion_open());
    app.handle_key(char_key('@'), &ctl);
    assert!(app.file_menu.is_none(), "slash context never opens @ menu");
}

#[test]
fn vim_normal_mode_closes_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.vim.set(true); // insert mode
    app.handle_key(char_key('@'), &ctl);
    assert!(app.file_menu.is_some());
    // Esc returns to vim normal; the browser must not linger there.
    app.handle_key(key(KeyCode::Esc), &ctl);
    assert_eq!(app.vim.mode, VimMode::Normal);
    assert!(app.file_menu.is_none());
}

#[test]
fn arrow_keys_are_owned_by_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    app.handle_key(char_key('s'), &ctl);
    // Up/Down move the browser selection, not the caret.
    let before = app.input.cursor_char();
    app.handle_key(key(KeyCode::Up), &ctl);
    app.handle_key(key(KeyCode::Down), &ctl);
    assert_eq!(app.input.cursor_char(), before);
    // Left goes to the parent directory.
    app.handle_key(key(KeyCode::Left), &ctl);
    let menu = app.file_menu.as_ref().expect("menu open");
    assert_eq!(menu.explorer().cwd(), ws.root.parent().unwrap());
}

#[test]
fn quoted_spaced_path_round_trips() {
    let ws = Workspace::new();
    fs::create_dir_all(ws.root.join("my dir")).expect("create");
    fs::write(ws.root.join("my dir/file with space.txt"), "").expect("write");
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    // [../, my dir/, src/, README.md, notes.txt]
    app.handle_key(key(KeyCode::Down), &ctl); // my dir/
    app.handle_key(key(KeyCode::Tab), &ctl); // drill → @"my dir/
    assert_eq!(app.input.buf(), "@\"my dir/");
    app.handle_key(key(KeyCode::Down), &ctl); // file with space.txt
    app.handle_key(key(KeyCode::Enter), &ctl);
    assert!(app.file_menu.is_none());
    assert_eq!(app.input.buf(), "@\"my dir/file with space.txt\"");
}

#[test]
fn ctrl_h_toggles_hidden_files_in_the_browser() {
    let ws = Workspace::new();
    fs::write(ws.root.join(".env"), "").expect("write");
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    fn hidden_state(app: &App) -> bool {
        app.file_menu
            .as_ref()
            .expect("menu open")
            .explorer()
            .show_hidden()
    }
    fn has_env(app: &App) -> bool {
        app.file_menu
            .as_ref()
            .expect("menu open")
            .explorer()
            .files()
            .iter()
            .any(|f| f.name == ".env")
    }
    assert!(!hidden_state(&app));
    assert!(!has_env(&app), ".env hidden by default");
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL), &ctl);
    assert!(hidden_state(&app), "ctrl+h reveals hidden entries");
    assert!(has_env(&app), ".env listed while hidden shown");
    // The browser is still usable afterwards (toggle is modal, not a
    // one-shot).
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL), &ctl);
    assert!(!hidden_state(&app));
    assert!(!has_env(&app), ".env hidden again");
    app.handle_key(key(KeyCode::Down), &ctl); // src/
    app.handle_key(key(KeyCode::Down), &ctl); // README.md
    app.handle_key(key(KeyCode::Enter), &ctl);
    assert_eq!(app.input.buf(), "@README.md");
}

#[test]
fn send_now_closes_the_browser() {
    let ws = Workspace::new();
    let (mut app, ctl, _rx) = test_app(&ws.root);
    app.handle_key(char_key('@'), &ctl);
    assert!(app.file_menu.is_some());
    // ctrl+enter steers immediately — the browser must not survive the
    // send (plain Enter settles a mention while the menu is open).
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), &ctl);
    assert!(app.file_menu.is_none());
}
