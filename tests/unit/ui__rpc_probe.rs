use super::*;
use crate::runtime::RuntimeConfig;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::sync::mpsc;

fn probe_app() -> App {
    let cfg = RuntimeConfig {
        bin: "dsh-runtime".into(),
        cordis: "cordis".into(),
        workspace: "/w".into(),
        session_root: std::env::temp_dir()
            .join(format!("dsh-tui-rpc-probe-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, _rx) = mpsc::channel();
    App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx)
}

fn push_view(app: &mut App, ctl: &crate::controller::Controller, text: &str) {
    app.handle(
        crate::bus::AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "view",
                    "id": "plan-view",
                    "title": "Plan",
                    "nodes": [{ "id": "content", "kind": "markdown", "text": text }]
                }
            }),
        },
        ctl,
    );
}

#[test]
fn real_rpc_path_renders_markdown_and_scrolls() {
    use crate::controller::tests::test_controller;
    let mut app = probe_app();
    app.show_banner = false;
    let (ctl, _commands) = test_controller();
    let long = (0..80)
        .map(|i| format!("- [{}] task {i:02}", if i % 3 == 0 { 'x' } else { ' ' }))
        .collect::<Vec<_>>()
        .join("\n");
    push_view(&mut app, &ctl, &format!("## Plan · 27/80\n\n{long}"));
    assert!(app.view_overlay.is_some(), "view opened via RPC");

    let frame = dump_frame(&mut app, 100, 30);
    assert!(frame.contains("Plan · 27/80"), "heading rendered:\n{frame}");
    assert!(frame.contains("task 00"), "task list rendered:\n{frame}");
    assert!(frame.contains("✓ task 00"), "checked glyph:\n{frame}");
    assert!(frame.contains("○ task 01"), "open glyph:\n{frame}");

    let before = app.view_overlay.as_ref().unwrap().scroll;
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))),
        &ctl,
    );
    let after = app.view_overlay.as_ref().unwrap().scroll;
    assert_eq!(after, before + 1, "Down scrolls the view");
    let frame2 = dump_frame(&mut app, 100, 30);
    assert!(
        !frame2.lines().any(|l| l.contains("Plan · 27/80")),
        "scrolling pushed the heading out:\n{frame2}"
    );
    assert!(
        frame2.contains("task 02"),
        "content followed the scroll:\n{frame2}"
    );
}

#[test]
fn arrow_keys_with_modifiers_still_scroll_the_view() {
    use crate::controller::tests::test_controller;
    let mut app = probe_app();
    app.show_banner = false;
    let (ctl, _commands) = test_controller();
    push_view(
        &mut app,
        &ctl,
        &format!(
            "## Plan\n\n{}",
            (0..60)
                .map(|i| format!("- [ ] task {i:02}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    let before = app.view_overlay.as_ref().unwrap().scroll;
    // Some terminals report arrows with modifier bits (kitty keyboard
    // protocol); those must still scroll the view.
    for modifiers in [
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
    ] {
        app.handle(
            crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, modifiers))),
            &ctl,
        );
    }
    assert_eq!(
        app.view_overlay.as_ref().unwrap().scroll,
        before + 3,
        "modified arrows scroll"
    );
}

#[test]
fn wheel_scrolls_the_view_overlay() {
    use crate::controller::tests::test_controller;
    let mut app = probe_app();
    app.show_banner = false;
    let (ctl, _commands) = test_controller();
    push_view(
        &mut app,
        &ctl,
        &format!(
            "## Plan\n\n{}",
            (0..60)
                .map(|i| format!("- [ ] task {i:02}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    let before = app.view_overlay.as_ref().unwrap().scroll;
    app.handle(
        crate::bus::AppEvent::Term(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 50,
            row: 15,
            modifiers: KeyModifiers::NONE,
        })),
        &ctl,
    );
    assert_eq!(
        app.view_overlay.as_ref().unwrap().scroll,
        before + 3,
        "wheel scrolls the view"
    );
    app.handle(
        crate::bus::AppEvent::Term(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 50,
            row: 15,
            modifiers: KeyModifiers::NONE,
        })),
        &ctl,
    );
    assert_eq!(
        app.view_overlay.as_ref().unwrap().scroll,
        before,
        "wheel up reverses the view scroll"
    );
}

#[test]
fn end_clamps_the_view_to_the_last_content_row() {
    use crate::controller::tests::test_controller;
    let mut app = probe_app();
    app.show_banner = false;
    let (ctl, _commands) = test_controller();
    let long = (0..80)
        .map(|i| format!("- [ ] task {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    push_view(&mut app, &ctl, &format!("## Plan\n\n{long}"));
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))),
        &ctl,
    );
    let frame = dump_frame(&mut app, 100, 30);
    assert!(frame.contains("task 79"), "bottom row visible:\n{frame}");
    // Overscroll stays clamped: End then more Down shows no blank tail.
    for _ in 0..10 {
        app.handle(
            crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::NONE,
            ))),
            &ctl,
        );
    }
    let frame2 = dump_frame(&mut app, 100, 30);
    assert!(frame2.contains("task 79"), "still at the bottom:\n{frame2}");
    assert!(
        frame2.lines().any(|l| l.contains("task 79")),
        "last row remains visible:\n{frame2}"
    );
}

#[test]
fn plan_review_elicitation_renders_markdown_and_scrolls() {
    use crate::controller::tests::test_controller;
    let mut app = probe_app();
    app.show_banner = false;
    let (ctl, _commands) = test_controller();
    // The real plan-review path: dsh-acp folds userQuestions into one
    // standard ACP form field whose description carries the full plan
    // markdown (question + detail).
    let detail = (0..60)
        .map(|i| format!("- [{}] step {i:02}", if i == 0 { 'x' } else { ' ' }))
        .collect::<Vec<_>>()
        .join("\n");
    let request: agent_client_protocol::schema::v1::CreateElicitationRequest =
            serde_json::from_value(serde_json::json!({
                "mode": "form",
                "sessionId": "s1",
                "message": "The agent needs your input.",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "question_0": {
                            "type": "string",
                            "title": "Plan review",
                            "description": format!(
                                "Approve this plan and leave plan mode?\n\n# Implementation Plan\n\n{detail}"
                            ),
                            "oneOf": [
                                { "const": "option_0", "title": "Approve" },
                                { "const": "option_1", "title": "Keep planning" },
                                { "const": "custom_0", "title": "Other" }
                            ]
                        },
                        "question_0_custom": {
                            "type": "string",
                            "title": "Other",
                            "description": "Type a custom answer."
                        }
                    },
                    "required": ["question_0"]
                }
            }))
            .expect("standard ACP form");
    let form = crate::elicitation::form_from_request(&request).expect("supported form");
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    app.handle(
        crate::bus::AppEvent::ElicitationAsk { form, reply: tx },
        &ctl,
    );
    assert!(app.elicitation_ask.is_some(), "elicitation opened");

    let frame = dump_frame(&mut app, 100, 30);
    assert!(frame.contains("Plan review"), "field title:\n{frame}");
    assert!(
        frame.contains("Implementation Plan"),
        "markdown heading rendered:\n{frame}"
    );
    assert!(
        !frame.contains("# Implementation Plan"),
        "heading markers dropped:\n{frame}"
    );
    assert!(frame.contains("✓ step 00"), "task glyph:\n{frame}");
    assert!(
        frame.contains("Approve") && frame.contains("Keep planning"),
        "answers pinned at the bottom:\n{frame}"
    );
    assert!(
        frame.contains("pgup/pgdn/end scroll"),
        "scroll affordance in the title:\n{frame}"
    );

    // PageDown scrolls the markdown pane; the answers stay pinned.
    let before = app.elicitation_ask.as_ref().unwrap().scroll;
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    assert_eq!(
        app.elicitation_ask.as_ref().unwrap().scroll,
        before + 5,
        "PageDown scrolls the description pane"
    );
    let frame2 = dump_frame(&mut app, 100, 30);
    assert!(
        frame2.contains("Approve"),
        "answers stay pinned while scrolling:\n{frame2}"
    );

    // End reaches the last row and clamps there; scrolling back up must
    // start from the visible bottom, not from the `usize::MAX` sentinel.
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))),
        &ctl,
    );
    let frame3 = dump_frame(&mut app, 100, 30);
    assert!(frame3.contains("step 59"), "bottom row visible:\n{frame3}");
    assert!(
        frame3.contains("Approve"),
        "answers visible at the bottom:\n{frame3}"
    );
    let at_bottom = app.elicitation_ask.as_ref().unwrap().scroll;
    assert!(
        at_bottom < 1000,
        "End offset normalized by the draw:\n{at_bottom}"
    );
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::PageUp,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    assert_eq!(
        app.elicitation_ask.as_ref().unwrap().scroll,
        at_bottom.saturating_sub(5),
        "PageUp scrolls back up after End"
    );
    let frame4 = dump_frame(&mut app, 100, 30);
    assert!(
        !frame4.contains("step 59"),
        "scrolled up from the bottom:\n{frame4}"
    );
    app.handle(
        crate::bus::AppEvent::Term(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 50,
            row: 15,
            modifiers: KeyModifiers::NONE,
        })),
        &ctl,
    );
    assert_eq!(
        app.elicitation_ask.as_ref().unwrap().scroll,
        at_bottom - 8,
        "wheel scrolls the description pane"
    );

    // Up/Down still move the option cursor; Enter answers the form.
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))),
        &ctl,
    );
    app.handle(
        crate::bus::AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        &ctl,
    );
    match rx.try_recv() {
        Ok(crate::elicitation::ElicitationReply::Accepted(values)) => {
            assert_eq!(
                values.get("question_0"),
                Some(&crate::elicitation::ElicitationValue::String(
                    "option_1".into()
                )),
                "Down + Enter selected Keep planning"
            );
        }
        other => panic!("expected the accepted form reply, got {other:?}"),
    }
}
