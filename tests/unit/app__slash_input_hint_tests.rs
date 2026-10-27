//! App-level slash argument hint tests: the ghost resolves from the
//! advertised commands, the slash menu keeps its Tab precedence, the hint
//! applies when no candidate owns the Tab, a right-click in the composer
//! applies the same insert, and Escape hides the ghost without clearing
//! the draft.

use super::*;
use crate::bus::{AppEvent, SkillInfo};
use std::sync::mpsc::Receiver;

/// Unique session root per call — keeps persisted UI fixtures isolated.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-slash-hint-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_app() -> (App, Controller, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek-official".into(),
        model: "deepseek-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
        additional_directories: Vec::new(),
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

fn skill(name: &str, hint: Option<&str>) -> SkillInfo {
    SkillInfo {
        name: name.into(),
        description: format!("{name} description"),
        input_hint: hint.map(str::to_string),
        config_action: None,
        client_command: false,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn char_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn type_text(app: &mut App, ctl: &Controller, text: &str) {
    for ch in text.chars() {
        app.handle_key(char_key(ch), ctl);
    }
}

fn right_click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn the_typed_command_shows_the_advertised_hint_without_entering_the_buffer() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");

    let hint = app.slash_input_hint().expect("the ghost resolves");
    assert_eq!(hint.command_name, "review");
    assert_eq!(hint.hint, "optional review instructions");
    assert_eq!(hint.prefix, "/review");
    // The ghost is a promise, not text: the buffer holds the command only.
    assert_eq!(app.input.buf(), "/review");
}

#[test]
fn a_command_without_a_hint_has_no_ghost() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("compact", None)];
    type_text(&mut app, &ctl, "/compact");
    assert!(app.slash_input_hint().is_none());
}

#[test]
fn a_config_switch_never_offers_its_hint() {
    let (mut app, ctl, _rx) = test_app();
    let mut plan = skill("plan", Some("mode"));
    plan.config_action = Some(crate::bus::CommandConfigAction {
        config_id: "collaboration_mode".into(),
        value: "plan".into(),
        reset_value: Some("default".into()),
    });
    app.skills = vec![plan];
    type_text(&mut app, &ctl, "/plan");
    assert!(app.slash_input_hint().is_none());
}

#[test]
fn typing_the_argument_drops_the_ghost() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review focus");
    assert!(app.slash_input_hint().is_none());
}

#[test]
fn the_slash_menu_keeps_the_first_tab_and_the_hint_takes_the_next_one() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");

    // The popup owns the first Tab: it completes the command name.
    app.handle_key(key(KeyCode::Tab), &ctl);
    assert_eq!(app.input.buf(), "/review ");

    // No candidate is left, so the hint becomes real text — selected, so
    // the user can type over it.
    app.handle_key(key(KeyCode::Tab), &ctl);
    assert_eq!(app.input.buf(), "/review optional review instructions");
    assert_eq!(
        app.input.selection_text().as_deref(),
        Some("optional review instructions")
    );
}

#[test]
fn a_right_click_in_the_composer_applies_the_same_insert_as_tab() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");
    app.input_area = ratatui::layout::Rect::new(0, 0, 40, 3);

    app.handle_mouse(right_click(5, 0), &ctl);

    assert_eq!(app.input.buf(), "/review optional review instructions");
    assert_eq!(
        app.input.selection_text().as_deref(),
        Some("optional review instructions")
    );
}

#[test]
fn a_right_click_outside_the_composer_leaves_the_draft_alone() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");
    app.input_area = ratatui::layout::Rect::new(0, 0, 40, 3);

    app.handle_mouse(right_click(5, 20), &ctl);

    assert_eq!(app.input.buf(), "/review");
}

#[test]
fn escape_hides_the_ghost_and_keeps_the_draft() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");
    assert!(app.slash_input_hint().is_some());

    app.handle_key(key(KeyCode::Esc), &ctl);

    assert!(app.slash_input_hint().is_none());
    assert_eq!(app.input.buf(), "/review");
}

#[test]
fn a_new_draft_re_opens_a_dismissed_ghost() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");
    app.handle_key(key(KeyCode::Esc), &ctl);
    assert!(app.slash_input_hint().is_none());

    // Backspace changes the text: the dismissal is gone with it.
    app.handle_key(key(KeyCode::Backspace), &ctl);
    assert_eq!(app.input.buf(), "/revie");
    type_text(&mut app, &ctl, "w");
    assert!(app.slash_input_hint().is_some());
}

#[test]
fn escape_hides_the_ghost_but_tab_still_applies_the_hint() {
    let (mut app, ctl, _rx) = test_app();
    app.skills = vec![skill("review", Some("optional review instructions"))];
    type_text(&mut app, &ctl, "/review");
    // First Tab accepts the slash menu (`/review `); Escape then hides the
    // ghost. Tab must still complete the declared hint (Backchat parity).
    app.handle_key(key(KeyCode::Tab), &ctl);
    assert_eq!(app.input.buf(), "/review ");
    app.handle_key(key(KeyCode::Esc), &ctl);
    assert!(app.slash_input_hint().is_none());
    assert_eq!(app.input.buf(), "/review ");

    app.handle_key(key(KeyCode::Tab), &ctl);
    assert_eq!(app.input.buf(), "/review optional review instructions");
}
