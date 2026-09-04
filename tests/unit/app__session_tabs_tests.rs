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
fn cancel_requested_only_closes_work_in_the_named_session() {
    let (mut app, ctl, _rx) = test_app();
    app.state = RunState::Running;
    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: "dsh-test".into(),
        call_id: "old".into(),
        name: "old-tool".into(),
        arguments: "{}".into(),
    });
    app.open_new_session("s-two".into(), true);
    app.state = RunState::Running;
    app.transcript.apply(crate::events::UiEvent::ToolCall {
        session: "s-two".into(),
        call_id: "live".into(),
        name: "live-tool".into(),
        arguments: "{}".into(),
    });

    app.handle(
        AppEvent::Ctl(CtlEvent::CancelRequested {
            session_id: "dsh-test".into(),
        }),
        &ctl,
    );

    assert_ne!(
        app.state_note, "cancelling",
        "the viewed tab stays untouched"
    );
    match &app.transcript.cells.last().unwrap().kind {
        crate::transcript::CellKind::Tool { ok, .. } => assert_eq!(*ok, None),
        other => panic!("expected live tool, got {other:?}"),
    }
    app.switch_to_session(0);
    assert_eq!(app.state_note, "cancelling");
    match &app.transcript.cells.last().unwrap().kind {
        crate::transcript::CellKind::Tool { ok, error, .. } => {
            assert_eq!(*ok, Some(false));
            assert_eq!(error.as_deref(), Some("cancelled"));
        }
        other => panic!("expected cancelled tool, got {other:?}"),
    }
}

#[test]
fn session_catalogs_park_and_restore_with_their_tabs() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.open_new_session("s-two".into(), true);
    app.handle(
        AppEvent::Ctl(CtlEvent::Catalog {
            session_id: Some("dsh-test".into()),
            models: vec![crate::bus::CatalogModel {
                provider: "provider-one".into(),
                id: "model-one".into(),
                name: "Model One".into(),
                vision: false,
            }],
            presets: Vec::new(),
        }),
        &ctl,
    );
    app.handle(
        AppEvent::Ctl(CtlEvent::Skills {
            session_id: Some("dsh-test".into()),
            skills: vec![crate::bus::SkillInfo {
                name: "session-one-skill".into(),
                description: String::new(),
                input_hint: None,
                config_action: None,
                client_command: false,
            }],
        }),
        &ctl,
    );

    assert!(app.last_models.is_empty());
    assert!(app.skills.is_empty());
    app.switch_to_session(0);
    assert_eq!(app.last_models[0].id, "model-one");
    assert_eq!(app.skills[0].name, "session-one-skill");
}

#[test]
fn binding_the_visible_tab_publishes_the_active_session() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.session_bound = false;
    app.startup_bound = false;

    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "s-bound".into(),
            notice: None,
        }),
        &ctl,
    );

    let sent: Vec<_> = commands.try_iter().collect();
    assert!(sent.iter().any(|command| matches!(
        command,
        Cmd::ActiveSession {
            session_id: Some(session_id)
        } if session_id == "s-bound"
    )));
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
fn parked_subagent_events_land_in_parked_subagent_transcript() {
    let (mut app, _ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "sub-1".into(),
    });
    assert_eq!(app.subagents.len(), 1);

    app.open_new_session("s-two".into(), true);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.parked[0].id, "dsh-test");
    assert_eq!(app.parked[0].subagents.len(), 1);

    app.apply_ui(final_event("sub-1", "subagent answer in background"));

    let sub_text = transcript_text(&mut app.parked[0].subagents[0].transcript);
    assert!(
        sub_text.contains("subagent answer in background"),
        "subagent event reached parked subagent transcript:\n{sub_text}"
    );
    assert!(!transcript_text(&mut app.transcript).contains("subagent answer"));
}

#[test]
fn parked_subagent_permission_ask_does_not_clobber_live_tab() {
    let (mut app, _ctl, _rx) = test_app();
    app.apply_ui(crate::events::UiEvent::SubagentStarted {
        parent: "dsh-test".into(),
        child: "sub-1".into(),
    });
    app.open_new_session("s-two".into(), true);

    let (live_tx, _live_rx) = tokio::sync::oneshot::channel();
    app.open_permission_ask("s-two", "live prompt ask".into(), Vec::new(), live_tx);
    assert!(app.permission_ask.is_some());

    let (sub_tx, _sub_rx) = tokio::sync::oneshot::channel();
    app.open_permission_ask("sub-1", "subagent ask".into(), Vec::new(), sub_tx);

    assert_eq!(app.permission_ask.as_ref().unwrap().title, "live prompt ask");
    assert!(app.parked[0].permission_ask.is_some());
    assert_eq!(
        app.parked[0].permission_ask.as_ref().unwrap().title,
        "subagent ask"
    );
}

#[test]
fn shell_pending_updated_on_session_bound() {
    let (mut app, ctl, _rx) = test_app();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    let placeholder = app.session_id.clone();
    assert!(placeholder.starts_with("dsh-"));

    app.run_local_shell("echo hi".into());
    assert_eq!(app.shell_pending[0].1, placeholder);

    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-real".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "acp-real");
    assert_eq!(
        app.shell_pending[0].1, "acp-real",
        "shell pending updated to bound id"
    );
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

#[test]
fn close_removes_the_viewed_tab_and_views_a_neighbor() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.open_new_session("s-two".into(), true); // live s-two, dsh-test parked
    app.open_new_session("s-three".into(), true); // live s-three (rightmost)
    while commands.try_recv().is_ok() {}

    // Close the rightmost tab: the left neighbor (s-two) takes its place.
    app.run_slash("close", "", &ctl);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.session_tab_count(), 2);
    assert_eq!(
        app.parked.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["dsh-test"]
    );
    match commands.try_recv() {
        Ok(crate::bus::Cmd::ForgetSession { session_id }) => {
            assert_eq!(session_id, "s-three", "acp forgets the closed session");
        }
        other => panic!("expected ForgetSession, got {other:?}"),
    }
}

#[test]
fn close_leftmost_tab_keeps_the_same_slot_index() {
    let (mut app, _ctl, _rx) = test_app();
    // Live dsh-test is the leftmost tab; s-two parked to its right.
    app.open_new_session("s-two".into(), true);
    app.switch_to_session(0);
    assert_eq!(app.session_id, "dsh-test");
    app.transcript.push_user("left prompt".into(), false);

    app.run_slash("close", "", &_ctl);
    assert_eq!(app.session_id, "s-two", "right neighbor takes slot 0");
    assert_eq!(app.session_tab_count(), 1);
    let text = transcript_text(&mut app.transcript);
    assert!(
        !text.contains("left prompt"),
        "closed tab's transcript is gone:\n{text}"
    );
}

#[test]
fn close_refuses_the_last_tab() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.run_slash("close", "", &ctl);
    assert_eq!(app.session_id, "dsh-test");
    assert_eq!(app.session_tab_count(), 1);
    assert!(
        commands.try_recv().is_err(),
        "closing the last tab must not emit ForgetSession"
    );
}

#[test]
fn close_discards_draft_queue_and_cancels_the_parked_ask() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true); // live s-two
    // A draft + queued prompts + a pending ask on the doomed live tab.
    app.input.insert_str("half-typed");
    app.state = RunState::Running;
    app.send_agent_text("queued one".into(), &ctl);
    assert_eq!(app.queued, 1);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.handle(
        AppEvent::PermissionAsk {
            session_id: "s-two".into(),
            title: "rm".into(),
            options: ask_options(),
            reply: tx,
        },
        &ctl,
    );
    assert!(app.permission_ask.is_some());

    app.run_slash("close", "", &ctl);
    assert_eq!(app.session_id, "dsh-test", "back on the parked tab");
    assert_eq!(
        rx.blocking_recv().expect("ask reply"),
        PermissionAskReply::Cancelled,
        "closing a tab cancels its pending ACP ask (Drop)"
    );
    assert!(app.input.is_empty(), "doomed draft did not leak");
    assert_eq!(app.queued, 0, "doomed queue did not leak");
    assert!(!app.session_tabs().iter().any(|t| t.ask_pending));
}

#[test]
fn close_of_an_unbound_tab_discards_its_coming_bind() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    while commands.try_recv().is_ok() {} // drain /new's NewSession + FetchSkills
    let placeholder = app.session_id.clone();
    assert!(placeholder.starts_with("dsh-") && !app.session_bound);

    // Close the placeholder before session/new resolves.
    app.run_slash("close", "", &ctl);
    while commands.try_recv().is_ok() {} // drain close's ForgetSession
    assert_eq!(app.session_id, "dsh-test", "back on the bound tab");

    // The bind resolves after the close: it must be discarded (and its
    // session forgotten), never rebind the viewed tab.
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-late".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(
        app.session_id, "dsh-test",
        "a dead bind must not hijack the viewed tab"
    );
    assert!(!app.session_bound || app.session_id == "dsh-test");
    match commands.try_recv() {
        Ok(crate::bus::Cmd::ForgetSession { session_id }) => {
            assert_eq!(session_id, "acp-late");
        }
        other => panic!("expected ForgetSession for the late bind, got {other:?}"),
    }

    // A later /new still binds normally — the tombstone kept the FIFO clean.
    app.run_slash("new", "", &ctl);
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-next".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "acp-next", "the next bind lands normally");
}

#[test]
fn composer_draft_scroll_model_and_banner_are_bound_to_the_tab() {
    let (mut app, ctl, _rx) = test_app();
    app.input.insert_str("draft for dsh-test");
    app.scroll_up = 42;
    app.selected_model = Some("deepseek-v3".into());
    app.show_banner = false; // this tab already prompted

    // A fresh tab starts clean — no draft, no scroll, no model pick.
    app.run_slash("new", "", &ctl);
    assert!(app.input.is_empty(), "fresh tab has an empty composer");
    assert_eq!(app.scroll_up, 0);
    assert_eq!(app.selected_model, None);
    assert!(app.show_banner, "a fresh tab still shows the welcome");

    // Round trip restores everything as left.
    app.switch_to_session(0);
    assert_eq!(app.session_id, "dsh-test");
    assert_eq!(app.input.lines().join(""), "draft for dsh-test");
    assert_eq!(app.scroll_up, 42);
    assert_eq!(app.selected_model.as_deref(), Some("deepseek-v3"));
    assert!(!app.show_banner);
}

#[test]
fn shell_output_lands_in_the_session_that_ran_it() {
    let (mut app, ctl, _rx) = test_app();
    app.run_local_shell("echo hello".into());
    let (shell_id, session, _cell, _gen) = app.shell_pending[0].clone();
    assert_eq!(session, "dsh-test");

    // Switch away while the command is still running.
    app.open_new_session("s-two".into(), true);
    assert!(app.shell_pending[0].1 == "dsh-test");

    app.handle(
        AppEvent::ShellDone {
            id: shell_id,
            code: Some(0),
            output: "hello".into(),
        },
        &ctl,
    );
    assert!(app.shell_pending.is_empty());
    let text = transcript_text(&mut app.transcript);
    assert!(
        !text.contains("hello"),
        "result must not land on the tab in view"
    );
    app.switch_to_session(0);
    let text = transcript_text(&mut app.transcript);
    assert!(text.contains("hello"), "result landed in its own session:\n{text}");
}

#[test]
fn deferred_steer_settles_into_the_owning_parked_slot() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true); // live s-two
    app.switch_to_session(0); // back on dsh-test
    // A Send Now is in flight for the viewed session; the user then leaves.
    app.pending_steer_cells.insert(
        77,
        PendingSteer {
            cells: Vec::new(),
            blocks: vec![StagedBlock::Text("steer me".into())],
            requeue_front: true,
            gen: app.transcript.gen(),
        },
    );
    app.switch_to_session(1); // away again: entry parked with dsh-test
    assert_eq!(app.session_id, "s-two");

    // The agent defers the steer while its owner is parked: the follow-up
    // must return to that session's own queue, not vanish.
    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id: 77,
            deferred: true,
        }),
        &ctl,
    );
    let slot = app
        .parked
        .iter()
        .find(|slot| slot.id == "dsh-test")
        .expect("dsh-test parked");
    assert!(
        slot.pending_steer_cells.is_empty(),
        "settled entry removed from its slot"
    );
    assert_eq!(slot.prompt_queue.len(), 1, "deferred steer requeued");
    assert!(matches!(
        slot.prompt_queue[0].blocks.as_slice(),
        [StagedBlock::Text(text)] if text == "steer me"
    ));
    assert!(app.prompt_queue.is_empty(), "never leaks onto the viewed tab");

    // A non-deferred settle for a parked owner just releases the entry.
    app.switch_to_session(0);
    app.pending_steer_cells.insert(78, PendingSteer {
        cells: Vec::new(),
        blocks: vec![StagedBlock::Text("ok".into())],
        requeue_front: true,
        gen: app.transcript.gen(),
    });
    app.switch_to_session(1);
    app.handle(
        AppEvent::Ctl(CtlEvent::SteerSettled {
            message_id: 78,
            deferred: false,
        }),
        &ctl,
    );
    let slot = app.parked.iter().find(|s| s.id == "dsh-test").unwrap();
    assert!(slot.pending_steer_cells.is_empty());
    assert_eq!(slot.prompt_queue.len(), 1, "accepted steer stays sent");
}

#[test]
fn error_on_an_unbound_tab_does_not_burn_the_queued_prompts() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    while commands.try_recv().is_ok() {}
    app.send_agent_text("first".into(), &ctl);
    app.send_agent_text("second".into(), &ctl);
    assert_eq!(app.queued, 2);

    // A rejection for the placeholder id + a generic connection error must
    // not pop the queue head and burn the whole FIFO through repeated
    // rejections — the prompts wait for the real bind.
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionError {
            session_id: app.session_id.clone(),
            message: "prompt: unknown session dsh-x".into(),
        }),
        &ctl,
    );
    app.handle(AppEvent::Ctl(CtlEvent::Error("boom".into())), &ctl);
    assert_eq!(app.queued, 2, "queue intact while the tab is unbound");
    assert!(
        commands.try_recv().is_err(),
        "no prompt may leave with a placeholder id"
    );

    // The bind dispatches the head normally.
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-real".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(app.queued, 1);
    assert!(matches!(
        commands.try_recv(),
        Ok(crate::bus::Cmd::Prompt { session_id, text })
            if session_id == "acp-real" && text == "first"
    ));
}

#[test]
fn bind_failure_clears_the_awaiting_entry_without_poisoning_the_next_bind() {
    let (mut app, _demo_ctl, _rx) = test_app();
    let (ctl, commands) = crate::controller::tests::test_controller();
    app.demo = false;
    app.run_slash("new", "", &ctl);
    while commands.try_recv().is_ok() {}
    let placeholder = app.session_id.clone();

    app.handle(
        AppEvent::Ctl(CtlEvent::BindFailed {
            message: "mock bind error".into(),
        }),
        &ctl,
    );
    assert_eq!(
        app.awaiting_binds.len(),
        0,
        "the dead request no longer owns a FIFO slot"
    );
    assert_eq!(app.session_id, placeholder, "tab stays open");
    assert!(!app.session_bound);
    assert!(
        transcript_text(&mut app.transcript).contains("mock bind error"),
        "the ACP failure detail is retained in the requesting tab",
    );

    // A later /new binds normally: the failed request cannot shadow it.
    app.run_slash("new", "", &ctl);
    while commands.try_recv().is_ok() {}
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-2".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "acp-2", "next bind lands on its own tab");
}

#[test]
fn startup_bind_while_a_new_is_in_flight_lands_on_the_parked_startup_tab() {
    let cfg = test_cfg();
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let (ctl, _commands) = crate::controller::tests::test_controller();
    let mut app = App::new(Some(Theme::dark()), cfg, "dsh-start".into(), false, false, tx);
    // The user /new's before the startup session/new resolved: the startup
    // tab (dsh-start) is parked and unbound; the placeholder is live.
    app.run_slash("new", "", &ctl);
    let placeholder = app.session_id.clone();
    assert_eq!(app.parked[0].id, "dsh-start");

    // The unrequested startup bind arrives first. It must rebind the
    // parked startup tab — never consume the /new placeholder's FIFO slot.
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-s0".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(
        app.session_id, placeholder,
        "the /new placeholder keeps its own pending bind"
    );
    assert_eq!(app.parked[0].id, "acp-s0", "startup session found its tab");
    assert!(app.parked[0].session_bound);

    // The /new bind then lands on its own tab, in order.
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBound {
            session_id: "acp-s1".into(),
            notice: None,
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "acp-s1");
    assert_eq!(app.session_tab_count(), 2);
}

#[test]
fn session_errors_land_in_the_owning_parked_transcript() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true); // live s-two
    for slot in &mut app.parked {
        if slot.id == "dsh-test" {
            slot.prompt_pending = true;
        }
    }
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionError {
            session_id: "dsh-test".into(),
            message: "prompt: boom".into(),
        }),
        &ctl,
    );
    let live_text = transcript_text(&mut app.transcript);
    assert!(
        !live_text.contains("boom"),
        "a parked session's failure must not spill onto the viewed tab"
    );
    let slot = app.parked.iter_mut().find(|s| s.id == "dsh-test").unwrap();
    assert!(!slot.prompt_pending, "the failed session's delivery settles");
    let parked_text = transcript_text(&mut slot.transcript);
    assert!(parked_text.contains("boom"), "notice lands in its own tab");
}

#[test]
fn session_slash_prev_next_cycle_tabs_and_view_shows_info() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    app.open_new_session("s-three".into(), true);
    assert_eq!(app.session_id, "s-three"); // index 2

    // Prev from the last tab steps back to the middle one…
    app.run_slash("session", "prev", &ctl);
    assert_eq!(app.session_id, "s-two");

    // …and next returns to the last.
    app.run_slash("session", "next", &ctl);
    assert_eq!(app.session_id, "s-three");

    // next from the last wraps to the first.
    app.run_slash("session", "next", &ctl);
    assert_eq!(app.session_id, "dsh-test");

    // prev from the first wraps to the last.
    app.run_slash("session", "prev", &ctl);
    assert_eq!(app.session_id, "s-three");

    // `view` (and a bare /session) opens the info popup, never switches.
    app.run_slash("session", "view", &ctl);
    assert_eq!(app.session_id, "s-three");
    assert_eq!(
        app.view_overlay.as_ref().expect("/session view opens a popup").id,
        "builtin.session"
    );
    app.run_slash("session", "", &ctl);
    assert_eq!(app.session_id, "s-three");
    assert!(app.view_overlay.is_some(), "bare /session still shows info");
}

#[test]
fn tab_strip_renders_overflow_indicator_when_narrow() {
    let (mut app, ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true);
    app.open_new_session("s-three".into(), true);
    app.open_new_session("s-four".into(), true);

    // On width 30, not all tabs fit. The live session is the newest one
    // (s-four), so the window parks at the tail without an indicator —
    // everything from the window to the end fits.
    let frame = crate::ui::dump_frame(&mut app, 30, 15);
    let row0 = frame.lines().next().unwrap_or_default();
    assert!(row0.contains("s-four"), "live tab on screen:\n{row0}");

    // Jumping back to the head hides the tail behind the window: the +N
    // indicator must render again.
    app.switch_view_to_tab(0, &ctl);
    let frame = crate::ui::dump_frame(&mut app, 30, 15);
    let row0 = frame.lines().next().unwrap_or_default();
    assert!(row0.contains('+'), "overflow indicator rendered in:\n{row0}");
    assert!(row0.contains("dsh-test"), "head tab on screen:\n{row0}");
}

/// 8 sessions (dsh-test + sess-01..sess-07). Tab 0 is 13 display cols
/// (`dsh-test` + padding/arrow), the rest 12 (`sess-0X` + padding/arrow),
/// so a 64-col row shows a window of five tabs and overflows.
fn open_eight_tabs() -> (App, Controller, Receiver<AppEvent>) {
    let (mut app, ctl, rx) = test_app();
    for i in 1..8 {
        app.open_new_session(format!("sess-{i:02}"), true);
    }
    (app, ctl, rx)
}

fn strip_window(app: &mut App) -> Vec<usize> {
    let _ = crate::ui::dump_frame(app, 64, 30);
    app.tab_rects.iter().map(|(_, idx)| *idx).collect()
}

/// The drawn rect of one tab (fresh draw happens inside `strip_window`).
fn drawn_rect(app: &App, tab: usize) -> ratatui::layout::Rect {
    app.tab_rects
        .iter()
        .find(|(_, idx)| *idx == tab)
        .expect("tab is on screen")
        .0
}

fn click_tab(app: &mut App, ctl: &Controller, tab: usize) {
    let rect = drawn_rect(app, tab);
    app.handle_mouse(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x + rect.width / 2,
            0,
        ),
        ctl,
    );
}

fn id_of(tab: usize) -> String {
    if tab == 0 {
        "dsh-test".into()
    } else {
        format!("sess-{tab:02}")
    }
}

#[test]
fn mouse_edge_clicks_walk_the_strip_to_every_tab_and_back() {
    let (mut app, ctl, _rx) = open_eight_tabs();
    // The live session is the newest (tab 7), so the first draw parks the
    // window at the tail with it at the right edge.
    let window = strip_window(&mut app);
    assert_eq!(window, vec![3, 4, 5, 6, 7], "live tab revealed on open");
    assert_eq!(app.tab_strip_offset, 3);

    // Walk left to the head: clicking the leftmost visible tab switches
    // to it and reveals its left neighbor.
    for step in (0..3).rev() {
        let first = *strip_window(&mut app).first().expect("window");
        assert!(first > 0, "still tabs to reveal on the left");
        click_tab(&mut app, &ctl, first);
        assert_eq!(app.session_id, id_of(first), "edge click switches");
        assert_eq!(app.tab_strip_offset, step, "strip walked one tab left");
        let window = strip_window(&mut app);
        assert!(
            window.contains(&first) && window.contains(&(first - 1)),
            "clicked tab stays visible and its left neighbor appears: {window:?}"
        );
    }
    click_tab(&mut app, &ctl, 0);
    assert_eq!(app.session_id, "dsh-test", "back on the first session");
    assert_eq!(app.tab_strip_offset, 0, "tab 0 cannot nudge left");
    assert_eq!(strip_window(&mut app), vec![0, 1, 2, 3, 4], "head window");

    // Walk right to the tail: clicking the rightmost visible tab switches
    // to it and reveals its right neighbor.
    for step in 1..=3 {
        let last = *strip_window(&mut app).last().expect("window");
        assert!(last < 7, "still tabs to reveal");
        click_tab(&mut app, &ctl, last);
        assert_eq!(app.session_id, id_of(last), "edge click switches");
        assert_eq!(app.tab_strip_offset, step, "strip walked one tab right");
        let window = strip_window(&mut app);
        assert!(
            window.contains(&last) && window.contains(&(last + 1)),
            "clicked tab stays visible and its neighbor appears: {window:?}"
        );
    }

    // The final tab is on screen now; clicking it switches but cannot
    // nudge further right.
    let before = app.tab_strip_offset;
    click_tab(&mut app, &ctl, 7);
    assert_eq!(app.session_id, "sess-07");
    assert_eq!(app.tab_strip_offset, before, "no nudge past the last tab");
}

#[test]
fn middle_tab_clicks_do_not_nudge_and_switches_out_of_view_reanchor() {
    let (mut app, ctl, _rx) = open_eight_tabs();
    assert_eq!(strip_window(&mut app), vec![3, 4, 5, 6, 7]);
    assert_eq!(app.tab_strip_offset, 3);

    // A middle click switches but leaves the strip where it is.
    click_tab(&mut app, &ctl, 5);
    assert_eq!(app.session_id, "sess-05", "switched to the middle tab");
    assert_eq!(app.tab_strip_offset, 3, "middle click does not nudge");
    assert_eq!(strip_window(&mut app), vec![3, 4, 5, 6, 7]);

    // Keyboard-style jump far out of the window (back to the head): the
    // draw re-anchors so the live tab is visible again.
    app.switch_view_to_tab(0, &ctl);
    assert_eq!(strip_window(&mut app), vec![0, 1, 2, 3, 4]);
    assert_eq!(app.tab_strip_offset, 0, "live tab 0 pins the head");

    // And a jump to the far tail parks the live tab at the right edge.
    app.switch_view_to_tab(7, &ctl);
    let window = strip_window(&mut app);
    assert_eq!(window, vec![3, 4, 5, 6, 7], "live tab at the right edge");
    assert_eq!(app.tab_strip_offset, 3);
}

#[test]
fn strip_offset_snaps_to_the_head_once_everything_fits_again() {
    let (mut app, _ctl, _rx) = test_app();
    app.open_new_session("s-two".into(), true); // 2 tabs: fits on a 64-col row
    app.tab_strip_offset = 3; // stale offset from a wider strip
    assert_eq!(strip_window(&mut app), vec![0, 1]);
    assert_eq!(app.tab_strip_offset, 0, "no scroll state when nothing overflows");
}
