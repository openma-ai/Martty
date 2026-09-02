use super::*;
use std::path::PathBuf;

fn test_app_with_root(root: &str, workspace: &str) -> (App, Controller) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: workspace.into(),
        session_root: root.into(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Some(Theme::dark()), cfg, "dsh-current".into(), true, false, tx);
    (app, ctl)
}

fn write_fixture_session(root: &PathBuf, id: &str) {
    let dir = root.join(crate::sessions::workspace_slug("/w")).join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let lines = [
            format!(r#"{{"type":"session","version":0,"id":"{id}","createdAt":1,"cwd":"/w"}}"#),
            r#"{"type":"permission/preset","seq":0,"data":{"preset":"workspace-write"}}"#.into(),
            r#"{"type":"turn/start","seq":1,"data":{"turn":1}}"#.into(),
            r#"{"type":"user/message","seq":2,"data":{"content":[{"text":"修复失败的测试","type":"text"}],"source":{"kind":"user"},"role":"user","id":"m1"}}"#.into(),
            r#"{"type":"session/title","seq":3,"data":{"title":"fix failing tests","source":{"kind":"provider","provider":"session-title-first-prompt-llm"}}}"#.into(),
            r#"{"type":"assistant/chunk","seq":4,"data":{"chunk":{"type":"usage","usage":{"inputTokens":10,"outputTokens":5}}}}"#.into(),
            r#"{"type":"assistant/message","seq":5,"data":{"message":{"content":[{"type":"text","text":"tests are green now"}],"source":{"model":"deepseek-v4-flash"}}}}"#.into(),
            r#"{"type":"turn/end","seq":6,"data":{"reason":"completed"}}"#.into(),
        ];
    std::fs::write(dir.join("session.jsonl"), lines.join("\n")).unwrap();
}

fn tmp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("dsh-resume-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn resume_replays_transcript_modes_and_usage() {
    let root = tmp_root("replay");
    write_fixture_session(&root, "dsh-past");
    let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");

    app.run_slash("resume", "dsh-past", &ctl);

    assert_eq!(app.session_id, "dsh-past", "active session switched");
    assert!(!app.show_banner, "banner dismissed on resume");
    assert_eq!(app.modes.permission.as_deref(), Some("workspace-write"));
    assert_eq!(app.transcript.usage.input, 10);
    assert_eq!(app.transcript.usage.output, 5);
    let text = app
        .transcript
        .lines(&Theme::dark(), 80, ' ')
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect::<String>();
    assert!(
        text.contains("修复失败的测试"),
        "user prompt replayed:\n{text}"
    );
    assert!(
        text.contains("tests are green now"),
        "assistant reply replayed:\n{text}"
    );
    assert!(
        text.contains("resumed dsh-past"),
        "resume notice shown:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn keys_slash_opens_a_local_modal_without_polluting_the_timeline() {
    let root = tmp_root("keys");
    let (mut app, _demo_ctl) = test_app_with_root(root.to_str().unwrap(), "/w");
    let (ctl, commands) = crate::controller::tests::test_controller();
    let cells_before = app.transcript.cells.len();
    app.input.set("/keys".into());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    assert_eq!(
        app.transcript.cells.len(),
        cells_before,
        "/keys is chrome and must not enter the conversation timeline"
    );
    assert!(app.view_overlay.is_some(), "/keys modal should open");
    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(
        frame.contains("Keyboard shortcuts"),
        "modal title:\n{frame}"
    );
    assert!(frame.contains("ctrl+q"), "quit binding missing:\n{frame}");

    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE), &ctl);
    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(
        frame.contains("shift+tab") && frame.contains("permission"),
        "permission binding missing:\n{frame}"
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(app.view_overlay.is_none(), "esc closes the local modal");
    assert!(
        matches!(
            commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "closing a builtin modal must not emit a plugin overlay event"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ctrl_k_no_longer_opens_the_keys_modal_on_an_empty_prompt() {
    let root = tmp_root("keys-shortcut");
    let (mut app, _demo_ctl) = test_app_with_root(root.to_str().unwrap(), "/w");
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let cells_before = app.transcript.cells.len();

    app.handle_key(
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        &ctl,
    );

    assert!(
        app.view_overlay.is_none(),
        "ctrl+k belongs to the text editor now — /keys opens the modal"
    );
    assert_eq!(app.transcript.cells.len(), cells_before);

    app.run_slash("keys", "", &ctl);
    assert!(app.view_overlay.is_some(), "/keys opens the modal");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn acp_resume_usage_snapshot_reaches_the_footer_once() {
    let root = tmp_root("acp-usage");
    let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");

    // Multi-session routing (issue #94): updates only reach the footer of
    // the session they are tagged with, so load the session first.
    app.load_acp_session("dsh-loaded", &ctl);
    app.handle(
        AppEvent::Rpc {
            method: "session/update".into(),
            params: serde_json::json!({
                "sessionId": "dsh-loaded",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "_meta": {
                        "dsh": {
                            "event": "prompt/usage",
                            "usage": {
                                "inputTokens": 41,
                                "outputTokens": 9,
                                "thoughtTokens": 4,
                                "cachedReadTokens": 13,
                                "cachedWriteTokens": 2
                            }
                        }
                    }
                }
            }),
        },
        &ctl,
    );

    assert_eq!(app.transcript.usage.input, 41);
    assert_eq!(app.transcript.usage.output, 9);
    assert_eq!(app.transcript.usage.cached, 15);
    assert_eq!(app.transcript.usage.reasoning, 4);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resume_picker_lists_sessions_and_prefix_resolves() {
    let root = tmp_root("picker");
    write_fixture_session(&root, "dsh-alpha");
    let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");

    app.run_slash("resume", "", &ctl);
    let picker = app.picker.as_ref().expect("picker opens");
    assert!(matches!(picker.kind, PickerKind::Session));
    assert_eq!(picker.items[0].id, "dsh-alpha");
    // The human handle is the label; the meta carries short id, age, turns.
    assert_eq!(picker.items[0].label, "fix failing tests", "title as label");
    assert!(
        picker.items[0].meta.contains("1 turn"),
        "{}",
        picker.items[0].meta
    );
    assert!(
        picker.items[0].meta.contains("dsh-alp"),
        "short id in meta: {}",
        picker.items[0].meta
    );
    assert!(
        !picker.items[0].meta.contains("修复失败的测试"),
        "preview moved out of meta: {}",
        picker.items[0].meta
    );
    assert!(
        picker.title.contains('1'),
        "picker title counts sessions: {}",
        picker.title
    );
    app.picker = None;

    // unique prefix resolves; unknown id warns and keeps the session
    app.run_slash("resume", "dsh-al", &ctl);
    assert_eq!(app.session_id, "dsh-alpha");
    app.run_slash("resume", "nope", &ctl);
    assert_eq!(app.session_id, "dsh-alpha", "unknown prefix leaves session");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resume_with_no_sessions_notices_instead_of_picker() {
    let root = tmp_root("empty");
    let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");
    app.run_slash("resume", "", &ctl);
    assert!(app.picker.is_none(), "no picker without sessions");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn picker_page_keys_jump_a_screenful_and_home_end_pin_the_ends() {
    let root = tmp_root("page");
    let (mut app, ctl) = test_app_with_root(root.to_str().unwrap(), "/w");
    app.picker = Some(Picker {
        kind: PickerKind::Session,
        title: " resume session · 40 sessions · enter select · esc close ".into(),
        sel: 0,
        items: (0..40)
            .map(|i| PickerItem {
                id: format!("s{i:02}"),
                label: format!("session {i:02}"),
                meta: String::new(),
                provider: None,
            })
            .collect(),
    });
    // The draw pass records how many rows the popup shows; page keys
    // move exactly that far (a 10-row popup in this test).
    app.picker_page_rows = 10;

    let key = |app: &mut App, code: KeyCode| {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE), &ctl);
    };

    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.picker.as_ref().unwrap().sel, 10, "page down");
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.picker.as_ref().unwrap().sel, 20, "page down again");
    key(&mut app, KeyCode::End);
    assert_eq!(app.picker.as_ref().unwrap().sel, 39, "end pins the tail");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.picker.as_ref().unwrap().sel, 29, "page up");
    key(&mut app, KeyCode::Home);
    assert_eq!(app.picker.as_ref().unwrap().sel, 0, "home pins the head");
    key(&mut app, KeyCode::PageUp);
    assert_eq!(
        app.picker.as_ref().unwrap().sel,
        0,
        "page up sticks at head"
    );
    key(&mut app, KeyCode::Up);
    assert_eq!(app.picker.as_ref().unwrap().sel, 39, "↑ still wraps");
    let _ = std::fs::remove_dir_all(&root);
}
