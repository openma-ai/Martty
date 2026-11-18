use super::*;
use crate::bus::SessionListItem;
use crate::transcript::CellKind;

fn test_app() -> (App, Controller) {
    let (app, ctl, _commands) = test_app_with_commands();
    (app, ctl)
}

fn test_app_with_commands() -> (App, Controller, std::sync::mpsc::Receiver<Cmd>) {
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
    let (ctl, commands) = crate::controller::tests::test_controller();
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, commands)
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "martty-add-dir-cmd-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
            fork_session: false,
            close_session: false,
            additional_directories: true,
            auth_logout: false,
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
            fork_session: false,
            close_session: false,
            additional_directories: false,
            auth_logout: false,
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

#[test]
fn add_dir_without_the_capability_is_refused_without_changes() {
    let (mut app, ctl, commands) = test_app_with_commands();
    app.demo = false;
    app.locale = crate::locale::Locale::En;
    app.additional_directories_cap = false;
    app.additional_directories_known = true;
    let before = app.cfg.additional_directories.clone();
    let dir = scratch_dir("refused");
    app.run_slash("add-dir", dir.to_str().unwrap(), &ctl);

    assert_eq!(app.cfg.additional_directories, before);
    let sent: Vec<_> = commands.try_iter().collect();
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, Cmd::NewSession { .. })),
        "a refused /add-dir must not open a session: {sent:?}"
    );
    let notes = notice_text(&app);
    assert!(notes.contains("additionalDirectories"), "{notes}");
    assert!(notes.contains("cannot extend"), "{notes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_dir_rejects_relative_root_and_home_paths() {
    let (mut app, ctl, commands) = test_app_with_commands();
    app.demo = false;
    app.locale = crate::locale::Locale::En;
    app.additional_directories_cap = true;
    app.additional_directories_known = true;
    let before = app.cfg.additional_directories.clone();

    app.run_slash("add-dir", "relative/lib", &ctl);
    let notes = notice_text(&app);
    assert!(notes.contains("absolute path"), "{notes}");
    app.run_slash("add-dir", "/", &ctl);
    let notes = notice_text(&app);
    assert!(notes.contains("filesystem root"), "{notes}");
    app.run_slash("add-dir", &app.cfg.workspace.clone(), &ctl);
    let notes = notice_text(&app);
    assert!(notes.contains("is the workspace"), "{notes}");
    if let Some(home) = std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty() && home != "/")
    {
        app.run_slash("add-dir", &home, &ctl);
        let notes = notice_text(&app);
        assert!(notes.contains("home directory"), "{notes}");
    }

    assert_eq!(app.cfg.additional_directories, before);
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, Cmd::NewSession { .. })),
        "invalid paths must not open a session"
    );
}

#[test]
fn add_dir_appends_and_opens_a_new_session_with_the_extended_list() {
    let (mut app, ctl, commands) = test_app_with_commands();
    app.demo = false;
    app.locale = crate::locale::Locale::En;
    app.additional_directories_cap = true;
    app.additional_directories_known = true;
    let dir = scratch_dir("accepted");
    let canonical = std::fs::canonicalize(&dir).unwrap();
    let canonical = canonical.to_string_lossy().into_owned();

    app.run_slash("add-dir", dir.to_str().unwrap(), &ctl);

    assert!(
        app.cfg.additional_directories.contains(&canonical),
        "{:?}",
        app.cfg.additional_directories
    );
    let extended = app.cfg.additional_directories.clone();
    assert_eq!(extended.len(), 3);
    let sent: Vec<_> = commands.try_iter().collect();
    let requested = sent
        .iter()
        .find_map(|command| match command {
            Cmd::NewSession { directories, .. } => Some(directories.clone()),
            _ => None,
        })
        .expect("a valid /add-dir opens session/new");
    assert_eq!(requested, Some(extended));
    let notes = notice_text(&app);
    assert!(notes.contains("current session keeps its roots"), "{notes}");
    assert!(notes.contains("3 extra root"), "{notes}");

    // A repeat is deduplicated: no second root, no second session.
    app.run_slash("add-dir", dir.to_str().unwrap(), &ctl);
    assert_eq!(app.cfg.additional_directories.len(), 3);
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, Cmd::NewSession { .. })),
        "a duplicate path must not open another session"
    );
    let notes = notice_text(&app);
    assert!(notes.contains("already an additional directory"), "{notes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bare_add_dir_lists_the_roots_or_says_none_with_usage() {
    let (mut app, ctl, commands) = test_app_with_commands();
    app.locale = crate::locale::Locale::En;

    app.run_slash("add-dir", "", &ctl);
    let notes = notice_text(&app);
    assert!(notes.contains("/tmp/lib"), "{notes}");
    assert!(notes.contains("/tmp/docs"), "{notes}");
    assert!(notes.contains("/add-dir <absolute path>"), "{notes}");

    app.cfg.additional_directories.clear();
    app.run_slash("add-dir", "   ", &ctl);
    let notes = notice_text(&app);
    assert!(notes.contains("additional directories: none"), "{notes}");
    assert!(notes.contains("usage:"), "{notes}");
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, Cmd::NewSession { .. })),
        "listing roots must not open a session"
    );
}
