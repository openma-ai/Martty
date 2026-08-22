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
