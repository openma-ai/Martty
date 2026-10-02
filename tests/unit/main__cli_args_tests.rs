use super::*;
use std::path::{Path, PathBuf};

#[test]
fn agent_flag_and_args() {
    let args = parse_args_from([
        "--agent".into(),
        "dsh".into(),
        "--agent-arg".into(),
        "--profile".into(),
        "--agent-arg".into(),
        "acp".into(),
    ])
    .unwrap();
    assert_eq!(agent_argv(&args), vec!["dsh", "--profile", "acp"]);
}

#[test]
fn help_mentions_agent() {
    assert!(HELP.contains("--agent"));
    assert!(HELP.contains("--agent-arg"));
    assert!(HELP.contains("--add-dir"));
}

#[test]
fn add_dir_is_repeatable_and_rejects_a_relative_path() {
    let root = std::env::temp_dir().join(format!("martty-cli-add-dir-{}", std::process::id()));
    let workspace = root.join("app");
    let extra = root.join("lib");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&extra).unwrap();
    let args = parse_args_from([
        "--workspace".into(),
        workspace.to_string_lossy().into_owned(),
        "--add-dir".into(),
        extra.to_string_lossy().into_owned(),
        "--add-dir".into(),
        extra.to_string_lossy().into_owned(),
    ])
    .unwrap();
    assert_eq!(args.add_dirs.len(), 2);
    let cfg = build_config(&args).unwrap();
    assert_eq!(cfg.additional_directories.len(), 1);
    assert!(cfg.additional_directories[0].ends_with("lib"), "{cfg:?}");

    let bad = parse_args_from([
        "--workspace".into(),
        workspace.to_string_lossy().into_owned(),
        "--add-dir".into(),
        "relative".into(),
    ])
    .unwrap();
    let err = build_config(&bad).unwrap_err();
    assert!(
        err.to_string().contains("absolute path"),
        "{err:#}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn help_mentions_demo_skin() {
    assert!(HELP.contains("--demo-skin"));
    assert!(HELP.contains("--demo"));
}

#[test]
fn help_defaults_to_the_current_flash_model() {
    assert!(HELP.contains("deepseek-flash"));
    assert!(!HELP.contains("deepseek-v4-flash"));
}

#[test]
fn martty_home_precedence_owns_the_default_session_root() {
    assert_eq!(
        crate::runtime::martty_home_from(Some("/opt/martty"), Some("/opt/dsh"), "/Users/test",),
        PathBuf::from("/opt/martty")
    );
    assert_eq!(
        crate::runtime::martty_home_from(None, Some("/opt/dsh"), "/Users/test"),
        PathBuf::from("/opt/dsh/.martty")
    );
    assert_eq!(
        crate::runtime::martty_home_from(None, None, "/Users/test"),
        PathBuf::from("/Users/test/.martty")
    );
    assert_eq!(
        crate::runtime::martty_home_from(None, None, "/Users/test").join("sessions"),
        PathBuf::from("/Users/test/.martty/sessions")
    );
}

#[test]
fn removed_runtime_aliases_are_rejected() {
    for flag in ["--runtime-bin", "--cordis"] {
        let err = match parse_args_from([flag.into(), "legacy".into()]) {
            Ok(_) => panic!("{flag} unexpectedly remained accepted"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("unknown argument"),
            "{flag} must not remain as a hidden legacy option: {err:#}"
        );
    }
}

#[test]
fn demo_skin_implies_demo() {
    let args = parse_args_from(["--demo-skin".into()]).unwrap();
    assert!(args.demo_skin);
    assert!(args.demo);
    let args = parse_args_from(["--demo".into()]).unwrap();
    assert!(args.demo);
    assert!(!args.demo_skin);
}

#[test]
fn strip_demo_skin_keeps_other_flags() {
    let stripped = argv_without_demo_skin([
        "--workspace".into(),
        "/tmp".into(),
        "--demo-skin".into(),
        "--theme".into(),
        "light".into(),
    ]);
    assert_eq!(stripped, vec!["--workspace", "/tmp", "--theme", "light"]);
}

#[test]
fn demo_skin_script_candidates_prefer_source_then_vendor_layout() {
    let manifest = Path::new("/crate");
    let exe = Path::new("/crate/npm/vendor/darwin-arm64/dsh-tui");
    let c = demo_skin_script_candidates(manifest, exe);
    assert_eq!(c[0], PathBuf::from("/crate/npm/lib/demo-skin.js"));
    assert!(c.iter().any(|p| p.ends_with("lib/demo-skin.js")));
    assert!(c
        .iter()
        .any(|p| p.components().any(|c| c.as_os_str() == "vendor")
            || p.to_string_lossy().contains("..")));
}

#[test]
fn dump_frame_defaults_and_explicit_dims() {
    let args = parse_args_from(["--dump-frame".into()]).unwrap();
    assert_eq!(args.dump_frame, Some((100, 34)));
    let args = parse_args_from(["--dump-frame".into(), "80x24".into()]).unwrap();
    assert_eq!(args.dump_frame, Some((80, 24)));
}

#[test]
fn dump_frame_does_not_swallow_the_next_flag() {
    // `--dump-frame --theme light`: --theme is a flag, not dimensions.
    let args = parse_args_from([
        "--dump-frame".into(),
        "--theme".into(),
        "light".into(),
        "--demo".into(),
    ])
    .unwrap();
    assert_eq!(args.dump_frame, Some((100, 34)));
    assert_eq!(args.theme.as_deref(), Some("light"));
    assert!(args.demo);
}
