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
}

#[test]
fn help_mentions_demo_skin() {
    assert!(HELP.contains("--demo-skin"));
    assert!(HELP.contains("--demo"));
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
