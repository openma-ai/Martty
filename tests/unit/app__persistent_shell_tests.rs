use super::*;
use std::sync::mpsc::Receiver;

fn test_app(workspace: &std::path::Path) -> (App, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: workspace.to_string_lossy().into_owned(),
        session_root: workspace.join("sessions").to_string_lossy().into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let app = App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx);
    (app, rx)
}

fn wait_for_shell(rx: &Receiver<AppEvent>) -> (Option<i32>, String) {
    match rx
        .recv_timeout(Duration::from_secs(5))
        .expect("local shell result")
    {
        AppEvent::ShellDone { code, output, .. } => (code, output),
        _ => panic!("unexpected app event"),
    }
}

#[test]
fn local_shell_state_persists_between_invocations() {
    let workspace = std::env::temp_dir().join(format!("martty-shell-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(workspace.join("nested")).unwrap();
    let (mut app, rx) = test_app(&workspace);

    app.run_local_shell("cd nested; export MARTTY_SHELL_TEST=kept".into());
    assert_eq!(wait_for_shell(&rx).0, Some(0));
    app.run_local_shell("printf %s \"$MARTTY_SHELL_TEST\" > shell-state.txt".into());
    assert_eq!(wait_for_shell(&rx).0, Some(0));

    let state_file = workspace.join("nested/shell-state.txt");
    assert!(
        state_file.exists(),
        "the second command must run in the directory selected by the first"
    );
    assert_eq!(std::fs::read_to_string(state_file).unwrap(), "kept");
    assert!(!workspace.join("shell-state.txt").exists());
    drop(app);
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn shell_done_refreshes_the_cap_git_branch_label() {
    let workspace = std::env::temp_dir().join(format!("martty-shell-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&workspace).unwrap();
    let (mut app, _rx) = test_app(&workspace);

    // A stale label from startup; the temp workspace is not a git repo, so
    // the forced re-check after a `!` command must clear it.
    app.git_branch = Some("stale-branch".into());
    let (ctl, _commands) = crate::controller::tests::test_controller();
    app.handle(
        AppEvent::ShellDone {
            id: 1,
            code: Some(0),
            output: "".into(),
        },
        &ctl,
    );
    assert_eq!(
        app.git_branch, None,
        "a session shell command must re-check the workspace branch"
    );

    drop(app);
    let _ = std::fs::remove_dir_all(workspace);
}
