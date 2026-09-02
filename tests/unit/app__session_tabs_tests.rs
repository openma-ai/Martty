use super::*;
use std::sync::mpsc::Receiver;

/// Unique session root per call — keeps persisted UI fixtures isolated.
fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-tabs-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    }
}

fn test_app() -> (App, Controller, Receiver<AppEvent>) {
    let cfg = test_cfg();
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

fn transcript_text(transcript: &mut Transcript) -> String {
    transcript
        .lines(&Theme::dark(), 80, ' ')
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect()
}

fn final_event(session: &str, text: &str) -> crate::events::UiEvent {
    crate::events::UiEvent::AssistantFinal {
        session: session.into(),
        text: text.into(),
        model: None,
    }
}

#[test]
fn parked_session_events_land_in_their_slot_only() {
    let (mut app, _ctl, _rx) = test_app();
    app.transcript.push_user("live prompt".into(), false);
    app.open_new_session("s-two".into(), true);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.parked.len(), 1);
    let live_before = transcript_text(&mut app.transcript);

    app.apply_ui(final_event("dsh-test", "old session answer"));

    assert!(
        transcript_text(&mut app.parked[0].transcript).contains("old session answer"),
        "event folded into the parked slot"
    );
    assert_eq!(
        transcript_text(&mut app.transcript),
        live_before,
        "live transcript untouched by a parked session's event"
    );
}

#[test]
fn unknown_session_events_never_reach_any_transcript() {
    let (mut app, _ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    let live_before = transcript_text(&mut app.transcript);
    let parked_before = transcript_text(&mut app.parked[0].transcript);

    app.apply_ui(final_event("s-foreign", "foreign answer"));

    assert_eq!(transcript_text(&mut app.transcript), live_before);
    assert_eq!(transcript_text(&mut app.parked[0].transcript), parked_before);
}

#[test]
fn parked_completion_edge_badges_the_tab_and_switching_clears_it() {
    let (mut app, _ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    // The parked session (tab 0) runs, then goes idle while out of view.
    app.apply_ui(crate::events::UiEvent::SessionStatus {
        session: "dsh-test".into(),
        running: true,
    });
    app.apply_ui(crate::events::UiEvent::SessionStatus {
        session: "dsh-test".into(),
        running: false,
    });

    let tabs = app.session_tabs();
    assert_eq!(tabs.len(), 2);
    assert!(tabs[0].completed_unseen, "running → idle edge badges the tab");
    assert!(!tabs[1].completed_unseen);

    app.switch_to_session(0);

    assert_eq!(app.session_id, "dsh-test");
    assert!(
        !app.session_tabs()[0].completed_unseen,
        "viewing the tab clears the badge"
    );
}

#[test]
fn switch_round_trip_preserves_transcripts_and_queues() {
    let (mut app, ctl, _rx) = test_app();
    app.transcript.push_user("alpha".into(), false);
    app.state = RunState::Running;
    app.send_agent_text("queued alpha".into(), &ctl);
    assert_eq!(app.queued, 1);

    app.run_slash("new", "s-two", &ctl);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.queued, 0, "the new tab has its own empty queue");
    assert!(!transcript_text(&mut app.transcript).contains("alpha"));
    app.transcript.push_user("beta".into(), false);

    app.switch_to_session(0);
    assert_eq!(app.session_id, "dsh-test");
    let text = transcript_text(&mut app.transcript);
    assert!(text.contains("alpha"), "first transcript survives:\n{text}");
    assert!(!text.contains("beta"));
    assert_eq!(app.queued, 1, "first session's queue survives");
    assert_eq!(app.state, RunState::Running, "running bit round-trips");

    app.switch_to_session(1);
    assert_eq!(app.session_id, "s-two");
    let text = transcript_text(&mut app.transcript);
    assert!(text.contains("beta"), "second transcript survives:\n{text}");
    assert!(!text.contains("alpha"));
}

#[test]
fn session_bound_lands_on_the_tab_that_asked() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    let placeholder = app.session_id.clone();
    assert!(
        placeholder.starts_with("dsh-") && placeholder != "dsh-test",
        "ACP /new opens a placeholder tab: {placeholder}"
    );
    assert_eq!(app.parked[0].id, "dsh-test", "previous session parked");

    // The user switches away before session/new resolves.
    app.switch_to_session(0);
    assert_eq!(app.session_id, "dsh-test");
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-9".into(),
            notice: None,
        }),
        &ctl,
    );

    assert_eq!(
        app.session_id, "dsh-test",
        "the bind must not hijack the viewed tab"
    );
    let slot = app
        .parked
        .iter()
        .find(|slot| slot.id == "acp-9")
        .expect("bind landed on the tab that asked");
    assert!(slot.session_bound);
}

#[test]
fn parked_session_idle_dispatches_its_own_queue() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.run_slash("new", "s-two", &ctl);
    while commands.try_recv().is_ok() {} // /new's FetchSkills
    app.parked[0].prompt_queue.push_back(ClientQueuedPrompt {
        id: 7,
        blocks: vec![StagedBlock::Text("followup".into())],
    });
    app.apply_ui(crate::events::UiEvent::SessionStatus {
        session: "dsh-test".into(),
        running: true,
    });

    app.handle(
        AppEvent::Ui(crate::events::UiEvent::SessionStatus {
            session: "dsh-test".into(),
            running: false,
        }),
        &ctl,
    );

    match commands.try_recv() {
        Ok(Cmd::Prompt { session_id, text }) => {
            assert_eq!(session_id, "dsh-test", "addressed by its own session id");
            assert_eq!(text, "followup");
        }
        other => panic!("expected the parked queue head to dispatch, got {other:?}"),
    }
    assert!(app.parked[0].prompt_queue.is_empty());
    assert!(app.parked[0].prompt_pending);
    assert!(
        transcript_text(&mut app.parked[0].transcript).contains("followup"),
        "dispatched prompt echoes into its own transcript"
    );
}

#[test]
fn tab_strip_renders_only_with_multiple_sessions() {
    let (mut app, ctl, _rx) = test_app();
    let single = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(
        !single.lines().next().unwrap_or_default().contains("· dsh-test"),
        "one session renders no tab strip:\n{single}"
    );

    app.run_slash("new", "s-two", &ctl);
    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    let row0 = frame.lines().next().unwrap_or_default();
    assert!(row0.contains("dsh-test"), "parked tab label:\n{frame}");
    assert!(row0.contains("s-two"), "live tab label:\n{frame}");
    assert!(
        row0.contains("dsh-test \u{e0b0}") && row0.contains("s-two \u{e0b0}"),
        "each tab ends in a Powerline arrow:\n{frame}"
    );
    assert!(!row0.contains('│'), "plain dividers are replaced:\n{frame}");

    let first_arrow_byte = row0.find('\u{e0b0}').expect("first arrow");
    let first_arrow_col =
        unicode_width::UnicodeWidthStr::width(&row0[..first_arrow_byte]) as u16;
    assert_eq!(
        app.tab_rects[0].0.right(),
        first_arrow_col + 1,
        "the visible arrow belongs to the tab's mouse target"
    );
}

#[test]
fn powerline_tabs_use_a_brand_active_segment_and_seamless_arrows() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut app, ctl, _rx) = test_app();
    app.run_slash("new", "s-two", &ctl);
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|f| crate::ui::draw(f, &mut app))
        .expect("draw frame");

    let theme = app.theme;
    let parked = app.tab_rects[0].0;
    let active = app.tab_rects[1].0;
    let buf = terminal.backend().buffer();

    assert_eq!(buf[(parked.x, parked.y)].bg, theme.panel);
    assert_eq!(buf[(active.x, active.y)].bg, theme.brand);

    let into_active = &buf[(parked.right() - 1, parked.y)];
    assert_eq!(into_active.symbol(), "\u{e0b0}");
    assert_eq!(into_active.fg, theme.panel, "arrow keeps the left fill");
    assert_eq!(into_active.bg, theme.brand, "arrow meets the active fill");

    let into_canvas = &buf[(active.right() - 1, active.y)];
    assert_eq!(into_canvas.symbol(), "\u{e0b0}");
    assert_eq!(into_canvas.fg, theme.brand);
    assert_eq!(into_canvas.bg, theme.bg);
}

#[test]
fn adjacent_inactive_powerline_tabs_alternate_neutral_fills() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let (mut app, ctl, _rx) = test_app();
    app.run_slash("new", "s-two", &ctl);
    app.run_slash("new", "s-three", &ctl);
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|f| crate::ui::draw(f, &mut app))
        .expect("draw frame");

    let theme = app.theme;
    let first = app.tab_rects[0].0;
    let second = app.tab_rects[1].0;
    let active = app.tab_rects[2].0;
    let buf = terminal.backend().buffer();

    assert_eq!(buf[(first.x, first.y)].bg, theme.panel);
    assert_eq!(buf[(second.x, second.y)].bg, theme.bg);
    assert_eq!(buf[(active.x, active.y)].bg, theme.brand);

    let separator = &buf[(first.right() - 1, first.y)];
    assert_eq!(separator.symbol(), "\u{e0b0}");
    assert_eq!(separator.fg, theme.panel);
    assert_eq!(separator.bg, theme.bg);
    assert_ne!(
        separator.fg, separator.bg,
        "adjacent inactive tabs must keep a visible arrow edge"
    );
}

#[test]
fn prompt_waits_for_session_bind_and_sends_with_the_real_id() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    while commands.try_recv().is_ok() {} // /new's NewSession etc.
    let placeholder = app.session_id.clone();
    assert!(
        placeholder.starts_with("dsh-"),
        "ACP /new opens a placeholder tab: {placeholder}"
    );
    assert!(!app.session_bound);

    // Submitted before session/new resolves: queued, nothing sent — acp.rs
    // rejects ids it never bound, so a placeholder prompt must wait.
    app.send_agent_text("hello".into(), &ctl);
    assert_eq!(app.prompt_queue.len(), 1);
    assert!(
        commands.try_recv().is_err(),
        "no prompt may leave with a placeholder id"
    );

    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-real".into(),
            notice: None,
        }),
        &ctl,
    );

    assert_eq!(app.session_id, "acp-real");
    assert!(app.session_bound);
    match commands.try_recv() {
        Ok(Cmd::Prompt { session_id, text }) => {
            assert_eq!(session_id, "acp-real", "dispatched with the bound id");
            assert_eq!(text, "hello");
        }
        other => panic!("expected the bind to dispatch the queued prompt, got {other:?}"),
    }
    assert!(app.prompt_queue.is_empty());
}

fn ask_options() -> Vec<PermissionAskOption> {
    vec![
        PermissionAskOption {
            option_id: "deny".into(),
            kind: "reject_once".into(),
            name: "Reject".into(),
        },
        PermissionAskOption {
            option_id: "allow".into(),
            kind: "allow_once".into(),
            name: "Allow once".into(),
        },
    ]
}

#[test]
fn permission_ask_follows_its_session_across_tab_switches() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            session_id: "s-two".into(),
            title: "bash".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    assert!(app.permission_ask.is_some(), "live ask shows on its own tab");

    // Switching away must take the popup off screen with its session —
    // never leave it floating over the newly viewed tab.
    app.switch_to_session(0);
    assert_eq!(app.session_id, "dsh-test");
    assert!(
        app.permission_ask.is_none(),
        "no stray popup on the tab just viewed"
    );
    let parked = app
        .parked
        .iter()
        .find(|slot| slot.id == "s-two")
        .expect("s-two parked");
    assert!(
        parked.permission_ask.is_some(),
        "the ask parks with its session"
    );
    assert!(app.session_tabs()[1].ask_pending, "its tab is badged");

    // The parked ask cannot be answered from the wrong tab and survives
    // untouched until its own tab is viewed again.
    app.switch_to_session(1);
    assert_eq!(app.session_id, "s-two");
    let ask = app.permission_ask.as_ref().expect("ask resurfaces");
    assert_eq!(ask.title, "bash");
    assert_eq!(ask.sel, 1, "selection survived the round trip");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        PermissionAskReply::Selected("allow".into())
    );
    assert!(
        !app.session_tabs().iter().any(|tab| tab.ask_pending),
        "answering clears the badge"
    );
}

#[test]
fn ask_for_a_parked_session_waits_in_its_slot() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true); // live s-two, dsh-test parked
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            session_id: "dsh-test".into(),
            title: "write".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    assert!(
        app.permission_ask.is_none(),
        "a parked session's ask never pops over the live tab"
    );
    let slot = app
        .parked
        .iter()
        .find(|slot| slot.id == "dsh-test")
        .expect("dsh-test parked");
    assert!(slot.permission_ask.is_some(), "ask parked with its owner");
    let tabs = app.session_tabs();
    assert!(
        tabs[0].ask_pending && !tabs[1].ask_pending,
        "only the owning tab is badged"
    );

    // Esc on the owning tab cancels — never silently while parked.
    app.switch_to_session(0);
    assert!(app.permission_ask.is_some(), "ask surfaced on its own tab");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        PermissionAskReply::Cancelled
    );
    assert!(!app.session_tabs()[0].ask_pending);
}

#[test]
fn elicitation_form_round_trips_through_parking_unchanged() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::ElicitationAsk {
            session_id: Some("s-two".into()),
            form: crate::elicitation::ElicitationForm {
                message: "The agent needs your input.".into(),
                fields: Vec::new(),
            },
            reply: tx,
        },
        &ctl,
    );
    assert!(app.elicitation_ask.is_some(), "form opened on its tab");

    app.switch_to_session(0);
    assert!(app.elicitation_ask.is_none(), "form left the screen");
    assert_eq!(app.session_tabs()[1].ask_pending, true);

    app.switch_to_session(1);
    let ask = app.elicitation_ask.as_ref().expect("form restored");
    assert_eq!(ask.form.message, "The agent needs your input.");

    // Session teardown (the slot is dropped) cancels the open ask.
    drop(app);
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        crate::elicitation::ElicitationReply::Cancelled
    );
}

#[test]
fn unknown_session_ask_stays_answerable_on_the_live_view() {
    let (mut app, ctl, _rx) = test_app();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            session_id: "s-foreign".into(),
            title: "bash".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    assert!(
        app.permission_ask.is_some(),
        "an unattributable ask falls back to the live view instead of cancelling"
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(
        rx.blocking_recv().expect("reply"),
        PermissionAskReply::Selected("allow".into())
    );
}

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn push_plugin_view(app: &mut App, ctl: &Controller, id: &str, text: &str) {
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "view",
                    "id": id,
                    "title": id,
                    "nodes": [{ "id": "content", "kind": "markdown", "text": text }]
                }
            }),
        },
        ctl,
    );
}

#[test]
fn painter_popups_park_with_their_session_and_resurface() {
    let (mut app, ctl, _rx) = test_app();
    app.push_keys();
    assert!(app.view_overlay.is_some(), "/keys opened");
    // The /plugins tree parks the same way (tree selection included).
    app.plugin_tree = Some(PluginTree {
        title: "plugins".into(),
        state: tui_tree_widget::TreeState::default(),
    });

    app.open_new_session("s-two".into(), true);
    assert!(
        app.view_overlay.is_none(),
        "the popup left the screen with its tab"
    );
    assert!(app.plugin_tree.is_none(), "/plugins left the screen too");
    assert!(
        app.parked[0].view_overlay.is_some(),
        "/keys parked with dsh-test"
    );
    assert!(app.parked[0].plugin_tree.is_some(), "/plugins parked too");

    app.switch_to_session(0);
    let view = app.view_overlay.as_ref().expect("/keys resurfaced");
    assert_eq!(view.id, "builtin.keys");
    assert!(!view.notify_plugin, "painter popup, not compositor-owned");
    assert!(app.plugin_tree.is_some(), "/plugins tree resurfaced");

    // Esc closes it on its own tab, painter-side (no plugin event).
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &ctl);
    assert!(app.view_overlay.is_none());
    app.switch_to_session(1);
    app.switch_to_session(0);
    assert!(
        app.view_overlay.is_none(),
        "closed popups stay closed across switches"
    );
}

#[test]
fn tab_click_cancels_a_plugin_view_with_its_esc_event() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.open_new_session("s-two".into(), true); // live s-two, dsh-test parked
    push_plugin_view(&mut app, &ctl, "plan-view", "step 1");
    assert!(app.view_overlay.is_some(), "plugin view opened");
    assert!(app.view_overlay.as_ref().unwrap().notify_plugin);

    // Tab strip geometry, as recorded by the painter each frame.
    app.tab_rects.push((ratatui::layout::Rect::new(0, 0, 10, 1), 0));
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 0), &ctl);

    assert_eq!(app.session_id, "dsh-test", "switched to the clicked tab");
    assert!(
        app.view_overlay.is_none(),
        "the compositor overlay did not ride along"
    );
    match commands.try_recv() {
        Ok(Cmd::PluginOverlayEvent { id, event, value }) => {
            assert_eq!(id, "plan-view");
            assert_eq!(event, "cancel");
            assert!(value.is_none(), "view cancel carries no value");
        }
        other => panic!("expected the cancel event, got {other:?}"),
    }
}

#[test]
fn tab_click_cancels_a_plugin_select_carrying_the_selection_value() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.open_new_session("s-two".into(), true);
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({
                "protocol": 0,
                "overlay": {
                    "kind": "select",
                    "id": "harness",
                    "title": "Harness",
                    "value": "b",
                    "options": [
                        { "value": "a", "label": "A" },
                        { "value": "b", "label": "B" }
                    ]
                }
            }),
        },
        &ctl,
    );
    assert!(app.select_overlay.is_some(), "select opened");

    app.tab_rects.push((ratatui::layout::Rect::new(0, 0, 10, 1), 0));
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 0), &ctl);

    assert!(app.select_overlay.is_none(), "select dismissed on the switch");
    match commands.try_recv() {
        Ok(Cmd::PluginOverlayEvent { id, event, value }) => {
            assert_eq!(id, "harness");
            assert_eq!(event, "cancel");
            assert_eq!(value, Some(serde_json::json!("b")));
        }
        other => panic!("expected the select cancel event, got {other:?}"),
    }
}

#[test]
fn plugin_null_ack_after_a_tab_switch_keeps_the_restored_painter_popup() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.push_keys(); // painter popup on dsh-test
    app.open_new_session("s-two".into(), true); // parks it with dsh-test
    push_plugin_view(&mut app, &ctl, "status", "idle");

    // Click tab 0: the plugin view is cancelled and /keys resurfaces.
    app.tab_rects.push((ratatui::layout::Rect::new(0, 0, 10, 1), 0));
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 0), &ctl);
    assert_eq!(app.session_id, "dsh-test");
    assert!(
        app.view_overlay.as_ref().is_some_and(|view| !view.notify_plugin),
        "the painter popup was restored"
    );

    // The compositor's null ack lands afterwards and must not eat /keys.
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::OVERLAY_UPDATE.into(),
            params: serde_json::json!({ "protocol": 0, "overlay": null }),
        },
        &ctl,
    );
    assert!(
        app.view_overlay.as_ref().is_some_and(|view| !view.notify_plugin),
        "the null ack only closes compositor-owned overlays"
    );
}
