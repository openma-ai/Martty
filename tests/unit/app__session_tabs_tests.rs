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
fn closing_tabs_keeps_the_rest_in_order() {
    let (mut app, ctl, _rx) = test_app();
    app.run_slash("new", "s-two", &ctl);
    app.run_slash("new", "s-three", &ctl);
    // Tabs: [dsh-test, s-two, s-three]; viewing s-three.
    assert_eq!(app.session_tab_count(), 3);
    assert_eq!(app.session_id, "s-three");

    // Closing the viewed tab moves to the left neighbor (it was last).
    app.close_session(2);
    assert_eq!(app.session_id, "s-two");
    let labels: Vec<String> = app.session_tabs().into_iter().map(|t| t.label).collect();
    assert_eq!(labels, ["dsh-test", "s-two"]);

    // Closing a parked tab left of the live one keeps the view put.
    app.close_session(0);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.session_tab_count(), 1);

    // The last remaining session is never closed.
    app.close_session(0);
    assert_eq!(app.session_id, "s-two");
    assert_eq!(app.session_tab_count(), 1);
}

#[test]
fn tab_strip_renders_only_with_multiple_sessions() {
    let (mut app, ctl, _rx) = test_app();
    let single = crate::ui::dump_frame(&mut app, 100, 30);
    assert!(
        !single.lines().next().unwrap_or_default().contains('+'),
        "one session renders no tab strip:\n{single}"
    );

    app.run_slash("new", "s-two", &ctl);
    let frame = crate::ui::dump_frame(&mut app, 100, 30);
    let row0 = frame.lines().next().unwrap_or_default();
    assert!(row0.contains("dsh-test"), "parked tab label:\n{frame}");
    assert!(row0.contains("s-two"), "live tab label:\n{frame}");
    assert!(row0.contains('+'), "plus cell:\n{frame}");
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
