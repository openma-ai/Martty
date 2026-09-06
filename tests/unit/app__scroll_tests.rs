use super::*;
use std::sync::mpsc::Receiver;

fn test_app() -> (App, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: std::env::temp_dir()
            .join(format!("dsh-tui-scroll-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let app = App::new(Some(Theme::dark()), cfg, "s1".into(), true, false, tx);
    (app, rx)
}

#[test]
fn scroll_by_after_jump_top_resolves_the_sentinel_against_the_last_frame() {
    let (mut app, _rx) = test_app();
    // Last rendered frame: 200 layout lines in a 20-row pane → max_scroll =
    // 180. The snapshot is viewport-sized; `total` carries the line count.
    app.chat_view.area = ratatui::layout::Rect::new(0, 0, 80, 20);
    app.chat_view.total = 200;
    app.chat_view.top = 180;
    app.chat_view.lines = (180..200).map(|i| format!("line {i}")).collect();

    // JumpTop leaves the usize::MAX sentinel until the next draw; a wheel
    // flick arriving in the same event burst must stay relative to the top
    // (MAX as i64 would wrap to -1 and teleport the viewport to the tail).
    app.scroll_up = usize::MAX;
    app.scroll_by(3);
    assert_eq!(app.scroll_up, 183, "wheel-up from the top clamps at the top");

    app.scroll_up = usize::MAX;
    app.scroll_by(-3);
    assert_eq!(app.scroll_up, 177, "wheel-down from the top moves 3 lines");
}

#[test]
fn scroll_by_without_the_sentinel_is_untouched() {
    let (mut app, _rx) = test_app();
    app.scroll_up = 10;
    app.scroll_by(5);
    assert_eq!(app.scroll_up, 15);
    app.scroll_by(-100);
    assert_eq!(app.scroll_up, 0, "clamps at the streaming tail");
}
