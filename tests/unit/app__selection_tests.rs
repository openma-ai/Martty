use super::*;

/// Unique session root per call — keeps the modes cache from leaking
/// between tests and runs.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-sel-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn view(lines: &[&str]) -> ChatView {
    ChatView {
        area: ratatui::layout::Rect::new(1, 0, 60, 10),
        top: 0,
        manual_top: None,
        lines: lines.iter().map(|s| s.to_string()).collect(),
        owners: vec![None; lines.len()],
        images: Vec::new(),
    }
}

fn sel(a: (usize, usize), h: (usize, usize)) -> Selection {
    Selection {
        anchor: SelPoint {
            line: a.0,
            col: a.1,
        },
        head: SelPoint {
            line: h.0,
            col: h.1,
        },
    }
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
    let (tx, _rx) = std::sync::mpsc::channel();
    App::new(
        crate::theme::Theme::dark(),
        cfg,
        "dsh-test".into(),
        true,
        false,
        tx,
    )
}

#[test]
fn slice_by_cells_handles_wide_chars() {
    assert_eq!(slice_by_cells("hello world", 6, 11), "world");
    // 选=2 cells: [0,2) 中=[2,4) 即=[4,6)
    assert_eq!(slice_by_cells("选中即copy", 2, 6), "中即");
    // a boundary-straddling wide char is kept
    assert_eq!(slice_by_cells("选中", 1, 3), "选中");
    assert_eq!(slice_by_cells("abc", 0, usize::MAX), "abc");
}

#[test]
fn selection_text_joins_lines_and_orders_reverse_drags() {
    let mut app = test_app();
    app.chat_view = view(&["first line  ", "second", "third"]);
    // forward drag: line0 col6 → line2 col2 (inclusive)
    let fwd = app.selection_text(sel((0, 6), (2, 2)));
    assert_eq!(fwd, "line\nsecond\nthi");
    // dragging upward yields the same text
    let rev = app.selection_text(sel((2, 2), (0, 6)));
    assert_eq!(fwd, rev);
}

#[test]
fn chat_hit_maps_screen_cells_to_layout_lines() {
    let mut app = test_app();
    app.chat_view = view(&["a", "b", "c", "d"]);
    app.chat_view.top = 2;
    let p = app.chat_hit(3, 1).expect("inside pane");
    assert_eq!((p.line, p.col), (3, 2)); // top=2 + row 1, col 3-x(1)
    assert!(app.chat_hit(0, 0).is_none(), "left of pane");
    assert!(app.chat_hit(3, 10).is_none(), "below pane");
}

#[test]
fn word_span_finds_word_under_column() {
    let (col, width, word) = word_span("run cargo test now", 6).expect("word");
    assert_eq!((col, width, word.as_str()), (4, 5, "cargo"));
    assert!(word_span("run cargo", 3).is_none(), "whitespace");
    assert!(word_span("run", 99).is_none(), "past end");
    let (col, width, word) = word_span("选中即复制 ok", 4).expect("cjk word");
    assert_eq!((col, width, word.as_str()), (0, 10, "选中即复制"));
}

#[test]
fn tool_click_toggles_output_expansion() {
    let mut app = test_app();
    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: "dsh-test".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    app.transcript.apply(crate::events::UiEvent::ToolResult {
        session: "dsh-test".into(),
        call_id: "c1".into(),
        is_error: false,
        text: "a\nb\nc\nd\ne\nf\ng\nh".into(),
        error: None,
    });
    app.chat_view.area = ratatui::layout::Rect::new(1, 0, 40, 10);
    app.chat_view.top = 0;
    app.chat_view.lines = vec!["tool line".into()];
    app.chat_view.owners = vec![Some(0)];

    assert_eq!(app.tool_at(2, 0), Some(0), "tool line owns its cell");
    assert!(!app.transcript.cells[0].expanded);
    app.toggle_tool(0);
    assert!(app.transcript.cells[0].expanded, "click expands");
    app.toggle_tool(0);
    assert!(!app.transcript.cells[0].expanded, "click collapses");
}

#[test]
fn wheel_over_collapsed_tool_scrolls_the_transcript() {
    let mut app = test_app();
    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: "dsh-test".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    app.transcript.apply(crate::events::UiEvent::ToolResult {
        session: "dsh-test".into(),
        call_id: "c1".into(),
        is_error: false,
        text: "a\nb\nc\nd\ne\nf\ng\nh".into(),
        error: None,
    });
    app.chat_view.area = ratatui::layout::Rect::new(1, 0, 40, 10);
    app.chat_view.top = 0;
    app.chat_view.lines = vec!["tool line".into()];
    app.chat_view.owners = vec![Some(0)];

    app.mouse_scroll(3, 2, 0);

    assert_eq!(app.scroll_up, 3, "tool cards must not swallow the wheel");
}

#[test]
fn tool_click_in_a_child_view_targets_the_child_transcript() {
    let mut app = test_app();
    let mut transcript = Transcript::new("child-1".into());
    transcript.apply(crate::events::UiEvent::ToolCall {
        session: "child-1".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    app.subagents.push(SubagentView {
        id: "child-1".into(),
        parent: "dsh-test".into(),
        label: "subagent 1".into(),
        running: true,
        failed: false,
        transcript,
    });
    app.active_subagent = Some("child-1".into());

    app.toggle_tool(0);

    assert!(app.subagents[0].transcript.cells[0].expanded);
    assert!(app.transcript.cells.is_empty());
}
