use super::*;
use crate::bus::SessionListItem;
use crate::transcript::CellKind;

fn test_app() -> (App, Controller) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp/workspace".into(),
        session_root: std::env::temp_dir()
            .join(format!("martty-add-dir-ui-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
        additional_directories: vec!["/tmp/lib".into(), "/tmp/docs".into()],
    };
    let _ = std::fs::create_dir_all(&cfg.session_root);
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl)
}

fn notice_text(app: &App) -> String {
    app.transcript
        .cells
        .iter()
        .filter_map(|cell| match &cell.kind {
            CellKind::Notice { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn status_text(app: &App) -> String {
    let overlay = app.view_overlay.as_ref().expect("status overlay");
    let crate::slots::TuiNode::Markdown { text, .. } = &overlay.nodes[0] else {
        panic!("status overlay node");
    };
    text.clone()
}

#[test]
fn advertised_directories_show_on_the_tab_and_in_status() {
    let (mut app, ctl) = test_app();
    app.handle(
        AppEvent::Ctl(CtlEvent::AgentCaps {
            load_session: true,
            list_session: true,
            resume_session: true,
            additional_directories: true,
        }),
        &ctl,
    );
    assert!(app.additional_directories_cap);
    assert!(notice_text(&app).is_empty(), "{}", notice_text(&app));
    assert!(
        app.session_tabs()[0].label.starts_with("+2 "),
        "{}",
        app.session_tabs()[0].label
    );
    app.run_slash("status", "", &ctl);
    let text = status_text(&app);
    assert!(text.contains("/tmp/lib"), "{text}");
    assert!(text.contains("/tmp/docs"), "{text}");
    app.run_slash("session", "view", &ctl);
    let text = status_text(&app);
    assert!(text.contains("/tmp/lib"), "{text}");
}

#[test]
fn missing_capability_explains_that_add_dir_was_not_sent() {
    let (mut app, ctl) = test_app();
    app.locale = crate::locale::Locale::En;
    app.handle(
        AppEvent::Ctl(CtlEvent::AgentCaps {
            load_session: false,
            list_session: false,
            resume_session: false,
            additional_directories: false,
        }),
        &ctl,
    );
    assert!(!app.additional_directories_cap);
    assert!(
        !app.session_tabs()[0].label.starts_with('+'),
        "{}",
        app.session_tabs()[0].label
    );
    let notes = notice_text(&app);
    assert!(
        notes.contains("did not advertise") && notes.contains("not sent"),
        "{notes}"
    );
    app.run_slash("status", "", &ctl);
    let text = status_text(&app);
    assert!(text.contains("not sent"), "{text}");
    assert!(!text.contains("/tmp/lib"), "{text}");
}

#[test]
fn session_list_picker_shows_reported_additional_directories() {
    let (mut app, ctl) = test_app();
    app.demo = false;
    app.list_session = true;
    app.handle(
        AppEvent::Ctl(CtlEvent::SessionList {
            requester_session_id: app.session_id.clone(),
            sessions: vec![SessionListItem {
                id: "s-old".into(),
                title: Some("notes".into()),
                updated_at: Some("2026-10-02T00:00:00Z".into()),
                additional_directories: vec!["/tmp/lib".into(), "/tmp/docs".into()],
            }],
            prefix: None,
            limit: usize::MAX,
        }),
        &ctl,
    );
    let picker = app.picker.as_ref().expect("picker");
    assert!(
        picker.items[0].meta.contains("+2 dirs"),
        "{}",
        picker.items[0].meta
    );
}
