use super::*;
use crate::bus::SkillInfo;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn test_cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: std::env::temp_dir()
            .join(format!("martty-fork-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
        additional_directories: Vec::new(),
    }
}

fn live_app() -> (App, Controller, std::sync::mpsc::Receiver<Cmd>) {
    let cfg = test_cfg();
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let mut app = App::new(
        Some(Theme::dark()),
        cfg,
        "orig-session".into(),
        false,
        true,
        tx,
    );
    app.demo = false;
    app.session_bound = true;
    app.startup_bound = true;
    app.attached = true;
    let (ctl, commands) = crate::controller::tests::test_controller();
    (app, ctl, commands)
}

fn skill(name: &str, description: &str) -> SkillInfo {
    SkillInfo {
        name: name.into(),
        description: description.into(),
        input_hint: None,
        config_action: None,
        client_command: false,
    }
}

fn notice_text(app: &App) -> String {
    app.transcript
        .cells
        .iter()
        .filter_map(|cell| match &cell.kind {
            crate::transcript::CellKind::Notice { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn enable_fork(app: &mut App, ctl: &Controller) {
    app.handle(
        AppEvent::Ctl(CtlEvent::AgentCaps {
            load_session: false,
            list_session: false,
            resume_session: false,
            fork_session: true,
            additional_directories: false,
        }),
        ctl,
    );
}

#[test]
fn fork_is_hidden_from_action_until_the_agent_advertises_it() {
    let (mut app, ctl, commands) = live_app();
    app.input.set("/fork".into());
    let menu = app.slash_matches();
    assert_eq!(menu.len(), 1);
    assert!(menu[0].disabled);
    assert_eq!(menu[0].name, "fork");
    assert!(
        menu[0].desc.contains("sessionCapabilities.fork"),
        "{}",
        menu[0].desc
    );
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert!(
        matches!(
            commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "a disabled /fork must not send session/fork"
    );
    assert!(notice_text(&app).is_empty() || notice_text(&app).contains("sessionCapabilities.fork"));
}

#[test]
fn advertised_fork_opens_a_new_tab_and_keeps_the_source() {
    let (mut app, ctl, commands) = live_app();
    app.transcript.push_user("keep me".into(), false);
    enable_fork(&mut app, &ctl);
    app.input.set("/f".into());
    let menu = app.slash_matches();
    let fork = menu
        .iter()
        .find(|entry| entry.name == "fork")
        .expect("fork row");
    assert!(!fork.disabled, "{}", fork.desc);
    assert_eq!(fork.usage, "/fork");

    app.input.set("/fork".into());
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);

    let Cmd::ForkSession {
        source_session_id,
        requester,
    } = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("session/fork command")
    else {
        panic!("expected ForkSession");
    };
    assert_eq!(source_session_id, "orig-session");
    assert!(requester.starts_with("fork-"), "{requester}");
    assert_eq!(app.session_tab_count(), 2);
    assert_eq!(app.session_id, requester);
    assert!(!app.session_bound, "the new tab waits for the forked id");

    app.handle(
        AppEvent::Ctl(CtlEvent::SessionBoundTo {
            previous_id: requester,
            session_id: "forked-1".into(),
            notice: Some("forked from orig-session · forked-1".into()),
        }),
        &ctl,
    );
    assert_eq!(app.session_id, "forked-1");
    assert!(app.session_bound);
    assert_eq!(app.session_tab_count(), 2);
    app.switch_to_session(0);
    assert_eq!(app.session_id, "orig-session");
    assert!(
        app.transcript.cells.iter().any(|cell| matches!(
            &cell.kind,
            crate::transcript::CellKind::User { text, .. } if text == "keep me"
        )),
        "the source transcript stays on its tab"
    );
}

#[test]
fn fork_error_is_shown_and_the_source_tab_stays() {
    let (mut app, ctl, commands) = live_app();
    app.transcript.push_user("still here".into(), false);
    enable_fork(&mut app, &ctl);
    app.run_slash("fork", "", &ctl);
    let Cmd::ForkSession { requester, .. } = commands
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("fork command")
    else {
        panic!("expected ForkSession");
    };
    let source = app
        .parked
        .iter()
        .find(|slot| slot.id == "orig-session")
        .expect("source parked");
    assert!(source.transcript.cells.iter().any(|cell| matches!(
        &cell.kind,
        crate::transcript::CellKind::User { text, .. } if text == "still here"
    )));

    app.handle(
        AppEvent::Ctl(CtlEvent::BindFailedTo {
            previous_id: requester,
            message: "session/fork: fork refused by fixture".into(),
        }),
        &ctl,
    );
    assert!(
        notice_text(&app).contains("fork refused by fixture"),
        "{}",
        notice_text(&app)
    );
    assert_eq!(app.session_tab_count(), 2);
    app.switch_to_session(0);
    assert_eq!(app.session_id, "orig-session");
    assert!(app.transcript.cells.iter().any(|cell| matches!(
        &cell.kind,
        crate::transcript::CellKind::User { text, .. } if text == "still here"
    )));
}

#[test]
fn harness_commands_that_reuse_builtin_names_are_prefixed_and_sent() {
    let (mut app, ctl, commands) = live_app();
    app.server_info = Some("pi-acp".into());
    app.skills = vec![
        skill("model", "pi model picker"),
        skill("session", "pi session command"),
        skill("fork", "custom fork prompt"),
        skill("commit-helper", "unrelated"),
    ];
    app.input.set("/model".into());
    let menu = app.slash_matches();
    assert!(
        menu.iter().any(|entry| !entry.skill
            && entry.name == "model"
            && entry.usage.starts_with("/model")),
        "builtin /model keeps the bare name"
    );
    let prefixed = menu
        .iter()
        .find(|entry| entry.skill && entry.name == "model")
        .expect("prefixed model");
    assert_eq!(prefixed.usage, "pi-acp /model");
    let index = menu
        .iter()
        .position(|entry| entry.skill && entry.name == "model")
        .unwrap();
    app.slash_sel = index;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    match commands.recv_timeout(std::time::Duration::from_secs(1)) {
        Ok(Cmd::Prompt { text, .. }) => assert_eq!(text, "/model"),
        other => panic!("prefixed /model must be sent as a prompt, got {other:?}"),
    }

    app.prompt_pending = false;
    app.state = RunState::Idle;
    app.input.set("/session next".into());
    let menu = app.slash_matches();
    assert!(
        menu.iter()
            .any(|entry| entry.skill && entry.usage == "pi-acp /session"),
        "prefixed /session stays available with arguments"
    );
    assert!(
        menu.iter()
            .any(|entry| !entry.skill && entry.name == "session"),
        "Enter on the first row still runs the builtin"
    );
    let index = menu.iter().position(|entry| entry.skill).unwrap();
    app.slash_sel = index;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    match commands.recv_timeout(std::time::Duration::from_secs(1)) {
        Ok(Cmd::Prompt { text, .. }) => assert_eq!(text, "/session next"),
        other => panic!("prefixed /session must send the typed line, got {other:?}"),
    }

    app.prompt_pending = false;
    app.state = RunState::Idle;
    app.input.set("/fork summarize".into());
    let menu = app.slash_matches();
    assert!(menu
        .iter()
        .any(|entry| !entry.skill && entry.name == "fork"));
    assert!(menu
        .iter()
        .any(|entry| entry.skill && entry.usage == "pi-acp /fork"));
    let index = menu.iter().position(|entry| entry.skill).unwrap();
    app.slash_sel = index;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    match commands.recv_timeout(std::time::Duration::from_secs(1)) {
        Ok(Cmd::Prompt { text, .. }) => assert_eq!(text, "/fork summarize"),
        other => panic!("prefixed /fork must not call session/fork, got {other:?}"),
    }
}

#[test]
fn slash_menu_paints_fork_and_the_prefixed_collision() {
    let (mut app, ctl, _commands) = live_app();
    enable_fork(&mut app, &ctl);
    app.show_banner = false;
    app.server_info = Some("pi-acp".into());
    app.skills = vec![skill("model", "pi model"), skill("session", "pi session")];
    app.input.set("/".into());
    let frame = crate::ui::dump_frame(&mut app, 120, 40);
    assert!(frame.contains("/fork"), "{frame}");
    app.input.set("/model".into());
    let frame = crate::ui::dump_frame(&mut app, 120, 40);
    assert!(frame.contains("pi-acp /model"), "{frame}");
    app.input.set("/session".into());
    let frame = crate::ui::dump_frame(&mut app, 120, 40);
    assert!(frame.contains("pi-acp /session"), "{frame}");
}
