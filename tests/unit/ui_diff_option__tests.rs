use super::*;
use crate::runtime::RuntimeConfig;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::Terminal;
use std::sync::mpsc;

/// Unique session root per call — keeps the modes cache from leaking
/// between tests and runs.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-ui-diff-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_app() -> App {
    let cfg = RuntimeConfig {
        bin: "dsh-runtime".into(),
        cordis: "cordis".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx)
}

/// Issue #38: scrolled CJK content leaves orphan halves of wide chars on the
/// real screen while both ratatui buffers agree the cell is unchanged, so the
/// diff skips it. The scroll surfaces mark every cell `AlwaysUpdate` to
/// bypass that equality check. The marks are read off the freshly rendered
/// buffer inside the draw closure: the backend's copy only reflects cells
/// that travelled through the diff, which wide-char trailing cells never do.
fn assert_always_update(buf: &Buffer, area: Rect, what: &str) {
    for pos in area.positions() {
        assert_eq!(
            buf[pos].diff_option,
            CellDiffOption::AlwaysUpdate,
            "{what} cell {pos:?} must bypass the diff equality check (issue #38)"
        );
    }
}

#[test]
fn chat_cells_force_a_full_rewrite_every_frame() {
    let mut app = test_app();
    app.show_banner = false;
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|f| {
            draw(f, &mut app);
            // draw_chat publishes the exact rect it painted for mouse
            // hit-testing.
            let area = app.chat_view.area;
            assert!(area.width > 0 && area.height > 0, "chat area must be drawn");
            assert_always_update(f.buffer_mut(), area, "chat");
        })
        .expect("draw frame");
}

#[test]
fn view_overlay_body_forces_a_full_rewrite() {
    use crate::app::ViewOverlay;
    let mut app = test_app();
    app.show_banner = false;
    app.view_overlay = Some(ViewOverlay {
        id: "keys".into(),
        title: "Keys".into(),
        nodes: vec![crate::slots::TuiNode::Markdown {
            id: "content".into(),
            text: (0..40)
                .map(|i| format!("- 第 {i} 行 · 快捷键说明 · wide char テスト"))
                .collect::<Vec<_>>()
                .join("\n"),
            streaming: false,
        }],
        scroll: 0,
        notify_plugin: false,
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|f| draw(f, &mut app)).expect("draw frame");
    // The issue-#38 gesture: scroll the popup, redraw, and the body must
    // still be marked for a full rewrite.
    app.view_overlay.as_mut().expect("overlay").scroll = 5;
    terminal
        .draw(|f| {
            draw(f, &mut app);
            let area = overlay_area(f.buffer_mut());
            assert_always_update(f.buffer_mut(), area, "overlay");
        })
        .expect("redraw frame");
}

/// Locate the overlay by its title row (the only row carrying "esc close")
/// and its rounded corners — the area formula is content-dependent, so the
/// test reads the painted border back.
fn overlay_area(buf: &Buffer) -> Rect {
    let (width, height) = (buf.area.width, buf.area.height);
    let mut border = None;
    for y in 0..height {
        let mut row = String::new();
        for x in 0..width {
            row.push_str(buf[(x, y)].symbol());
        }
        if row.contains("esc close") {
            let left = (0..width)
                .find(|&x| buf[(x, y)].symbol() == "╭")
                .expect("overlay top-left corner");
            let right = (0..width)
                .rfind(|&x| buf[(x, y)].symbol() == "╮")
                .expect("overlay top-right corner");
            border = Some((y, left, right));
            break;
        }
    }
    let (top, left, right) = border.expect("overlay title row");
    let bottom = (top + 1..height)
        .find(|&y| buf[(left, y)].symbol() == "╰" && buf[(right, y)].symbol() == "╯")
        .expect("overlay bottom border");
    Rect::new(left, top, right - left + 1, bottom - top + 1)
}
