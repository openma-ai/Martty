use super::*;
use serde_json::json;

fn dsh_acp_methods() -> Value {
    json!([{
        "id": "terminal-login",
        "name": "Log in with a DeepSeek API key",
        "description": "Interactive terminal setup — saves the key to the harness credential store shared with the dsh Web UI",
        "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
    }])
}

#[test]
fn dsh_acp_terminal_login_is_agent_with_terminal_launch() {
    let methods = parse_auth_methods(
        &dsh_acp_methods(),
        &["dsh-acp".into()],
        "/tmp/ws",
        &BTreeMap::new(),
    );
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].id, "terminal-login");
    assert_eq!(methods[0].type_name, "agent");
    assert!(!methods[0].form);
    let launch = methods[0].terminal_launch.as_ref().expect("terminal-auth");
    assert_eq!(launch.command, "dsh-acp");
    assert_eq!(launch.args, ["login"]);
    assert_eq!(launch.method_id, "terminal-login");
}

#[test]
fn terminal_auth_without_command_keeps_agent_args() {
    let methods = parse_auth_methods(
        &dsh_acp_methods(),
        &["dsh".into(), "--profile".into(), "acp".into()],
        "/tmp/ws",
        &BTreeMap::new(),
    );
    let launch = methods[0].terminal_launch.as_ref().unwrap();
    assert_eq!(launch.command, "dsh");
    assert_eq!(launch.args, ["--profile", "acp", "login"]);
}

#[test]
fn terminal_auth_advertised_command_is_ignored() {
    // Issue #160: `_meta["terminal-auth"].command` is never exec'd — the
    // client-configured agent argv always runs, with the block's args
    // appended. An absolute path in args (old pi-acp) lands in argv after
    // the client's own args.
    let methods = parse_auth_methods(
        &json!([{
            "id": "pi-login",
            "name": "Log in with pi",
            "type": "terminal",
            "_meta": {
                "terminal-auth": {
                    "command": "/usr/bin/node",
                    "args": ["/opt/pi-acp/dist/bin.js", "--terminal-login"]
                }
            }
        }]),
        &["openma-pi-acp".into()],
        "/tmp/ws",
        &BTreeMap::new(),
    );
    let launch = methods[0].terminal_launch.as_ref().unwrap();
    assert_eq!(launch.command, "openma-pi-acp");
    assert_eq!(launch.args, ["/opt/pi-acp/dist/bin.js", "--terminal-login"]);
    assert_eq!(launch.method_id, "pi-login");
}

#[test]
fn terminal_type_meta_command_is_ignored() {
    let methods = parse_auth_methods(
        &json!([{
            "id": "tty-login",
            "type": "terminal",
            "_meta": { "command": "/bin/agent-login", "args": ["--go"] }
        }]),
        &["wrapper-agent".into(), "--serve".into()],
        "/tmp/ws",
        &BTreeMap::new(),
    );
    let launch = methods[0].terminal_launch.as_ref().unwrap();
    assert_eq!(launch.command, "wrapper-agent");
    assert_eq!(launch.args, ["--serve", "--go"]);
}

#[test]
fn run_terminal_auth_reports_status_and_spawn_errors() {
    let launch = |command: &str, args: Vec<&str>| TerminalAuthLaunch {
        label: "login helper".into(),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: BTreeMap::new(),
        cwd: None,
        method_id: "m".into(),
    };
    let ok = run_terminal_auth(
        &launch("sh", vec!["-c", "exit 0"]),
        crate::locale::Locale::En,
    );
    assert!(ok.is_ok(), "{ok:?}");
    let exited = run_terminal_auth(
        &launch("sh", vec!["-c", "exit 7"]),
        crate::locale::Locale::En,
    )
    .unwrap_err();
    assert!(exited.contains("exited 7"), "{exited}");
    let spawn_err = run_terminal_auth(
        &launch("definitely-not-a-real-binary-160", Vec::new()),
        crate::locale::Locale::En,
    )
    .unwrap_err();
    assert!(spawn_err.contains("could not start"), "{spawn_err}");
}

#[test]
fn run_terminal_auth_reports_a_failed_spawn() {
    // A missing working directory makes the OS spawn itself fail, so the
    // process-error surface is exercised without mutating the test env.
    let launch = TerminalAuthLaunch {
        label: "login helper".into(),
        command: "/bin/true".into(),
        args: Vec::new(),
        env: BTreeMap::new(),
        cwd: Some("/definitely-not-a-real-dir-160".into()),
        method_id: "m".into(),
    };
    let err = run_terminal_auth(&launch, crate::locale::Locale::En).unwrap_err();
    assert!(err.contains("could not start"), "{err}");
}

#[test]
fn api_key_form_encodes_authenticate_meta() {
    let methods = parse_auth_methods(
        &json!([{
            "id": "api-key",
            "name": "API Key",
            "_meta": { "api-key": { "provider": "openai" } }
        }]),
        &["fake-agent".into()],
        "/tmp",
        &BTreeMap::new(),
    );
    assert!(methods[0].form);
    let mut values = BTreeMap::new();
    values.insert("api-key".into(), "sk-test".into());
    assert_eq!(
        Value::Object(authenticate_meta_from_method(&methods[0], &values).unwrap()),
        json!({ "api-key": { "apiKey": "sk-test" } })
    );
}

#[test]
fn gateway_form_encodes_authenticate_meta() {
    let methods = parse_auth_methods(
        &json!([{
            "id": "gateway",
            "name": "Custom model gateway",
            "_meta": { "gateway": { "protocol": "openai" } }
        }]),
        &["fake-agent".into()],
        "/tmp",
        &BTreeMap::new(),
    );
    // The credential form collects values keyed by declared var name.
    let values = BTreeMap::from([
        ("baseUrl".to_string(), "https://gw.example".to_string()),
        ("api-key".to_string(), "sk-test".to_string()),
    ]);
    assert_eq!(
        Value::Object(authenticate_meta_from_method(&methods[0], &values).unwrap()),
        json!({
            "gateway": {
                "baseUrl": "https://gw.example",
                "headers": { "Authorization": "Bearer sk-test" }
            }
        })
    );
}

#[test]
fn env_var_missing_is_needs_auth_without_authenticate() {
    let raw = json!([{
        "id": "openai-key",
        "name": "OpenAI API key",
        "description": "Set the OPENAI_API_KEY environment variable.",
        "type": "env_var",
        "vars": [{ "name": "OPENAI_API_KEY", "label": "API key", "secret": true }]
    }]);
    let methods = parse_auth_methods(&raw, &["fake-agent".into()], "/tmp", &BTreeMap::new());
    let snap = snapshot_from_methods(methods, &declared_auth_methods(&raw), &BTreeMap::new());
    assert_eq!(snap.status, AuthStatus::NeedsAuth);
    assert_eq!(
        snap.message.as_deref(),
        Some("missing credential variable: OPENAI_API_KEY.")
    );
}

#[test]
fn env_var_items_with_empty_names_are_dropped() {
    let raw = json!([{
        "id": "mixed-vars",
        "type": "env_var",
        "vars": [
            { "name": "", "label": "blank" },
            { "name": "KEPT_KEY", "label": "kept" }
        ]
    }]);
    let methods = parse_auth_methods(&raw, &["fake-agent".into()], "/tmp", &BTreeMap::new());
    let vars = &methods[0].vars;
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "KEPT_KEY");
}

#[test]
fn unsupported_type_is_unknown() {
    let raw = json!([{ "id": "magic-card", "name": "Magic Card", "type": "card" }]);
    let methods = parse_auth_methods(&raw, &["fake-agent".into()], "/tmp", &BTreeMap::new());
    let snap = snapshot_from_methods(methods, &declared_auth_methods(&raw), &BTreeMap::new());
    assert_eq!(snap.status, AuthStatus::Unknown);
    assert!(snap
        .message
        .as_deref()
        .unwrap()
        .contains("unsupported methods: card"));
}

#[test]
fn empty_auth_methods_are_none() {
    let snap = snapshot_from_methods(vec![], &[], &BTreeMap::new());
    assert_eq!(snap.status, AuthStatus::None);
}

#[test]
fn auth_required_error_matches_code() {
    let err = AcpError::auth_required();
    assert!(is_auth_required_error(&err));
    assert!(is_auth_failure_message("Authentication required"));
}

#[test]
fn needs_auth_notice_points_at_auth() {
    let methods = parse_auth_methods(
        &dsh_acp_methods(),
        &["dsh-acp".into()],
        "/tmp",
        &BTreeMap::new(),
    );
    let snap = needs_auth_snapshot(methods.clone(), methods.first(), None);
    let (_, text) = snap
        .notice(crate::locale::Locale::En)
        .expect("notice");
    assert!(text.contains("auth picker"));
    assert!(
        !text.contains("/login"),
        "/login is the agent's slash, not a TUI command: {text}"
    );
    assert!(text.contains("Log in with a DeepSeek API key"));
}

#[test]
fn needs_auth_notice_never_names_the_method_type() {
    // A NeedsAuth snapshot that knows no method stays unspecific — the
    // bare type word "agent" was the old fallback (issue #160).
    let snapshot = needs_auth_snapshot(Vec::new(), None, Some("token missing".into()));
    let (_, text) = snapshot
        .notice(crate::locale::Locale::En)
        .expect("notice");
    assert!(text.contains("sign-in needed"), "{text}");
    assert!(!text.contains("agent"), "{text}");
    let (_, zh) = snapshot
        .notice(crate::locale::Locale::Zh)
        .expect("zh notice");
    assert!(zh.contains("需要登录"), "{zh}");
}

#[test]
fn structured_auth_failure_keeps_the_provider_reason() {
    let err: AcpError = serde_json::from_value(json!({
        "code": -32603, "message": "Internal error",
        "data": {
            "errorKind": "authentication_failed",
            "details": "Failed to authenticate. API Error: 403 Insufficient account balance",
            "marttyConnection": {"id": "h3", "authMethods": []}
        }
    })).unwrap();
    assert!(is_auth_required_error(&err));
    assert_eq!(acp_error_message(&err),
        "Failed to authenticate. API Error: 403 Insufficient account balance");
    let unrelated: AcpError = serde_json::from_value(json!({
        "code": -32603, "message": "Internal error",
        "data": {"errorKind": "billing_error"}
    })).unwrap();
    assert!(!is_auth_required_error(&unrelated));
}

#[test]
fn terminal_first_method_surfaces_a_sign_in_notice() {
    let methods = parse_auth_methods(
        &json!([
            {
                "id": "pi-tui",
                "name": "Terminal login",
                "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
            },
            { "id": "oauth", "name": "Browser" }
        ]),
        &["fake-agent".into()],
        "/tmp",
        &Default::default(),
    );
    let snapshot = snapshot_from_methods(methods, &[], &Default::default());
    // A terminal-first agent may still have persisted credentials — the
    // startup snapshot stays Unknown and only a real auth_required reply
    // proves sign-in is needed, so the picker no longer flashes on a
    // signed-in startup (issue #160).
    assert_eq!(format!("{:?}", snapshot.status), "Unknown");
    // method_label still resolves the advertised name for display.
    let mut named = snapshot.clone();
    named.method_id = Some("pi-tui".into());
    assert_eq!(named.method_label(), Some("Terminal login"));
}

#[test]
fn terminal_launch_runs_the_client_argv_not_the_agent_meta_command() {
    // The `command` field in _meta["terminal-auth"] is deliberately ignored:
    // the launch always runs the client's own configured agent argv so a
    // third-party agent can never name an arbitrary binary (issue #160).
    let methods = parse_auth_methods(
        &json!([{
            "id": "pi-tui",
            "name": "Terminal login",
            "_meta": {
                "terminal-auth": {
                    "command": "evil-agent",
                    "args": ["login", "--fast"],
                    "env": { "EXTRA": "1" }
                }
            }
        }]),
        &["fake-agent".into(), "--verbose".into()],
        "/tmp",
        &Default::default(),
    );
    let launch = methods[0]
        .terminal_launch
        .as_ref()
        .expect("terminal launch");
    // The advertised `command` is ignored: the client's argv runs, with
    // the advertised args appended after the client's own spawn args.
    assert_eq!(launch.command, "fake-agent");
    assert_eq!(launch.args, vec!["--verbose", "login", "--fast"]);
    assert_eq!(launch.env.get("EXTRA").map(String::as_str), Some("1"));
}

#[test]
fn api_key_meta_reads_values_by_declared_variable_names() {
    let methods = parse_auth_methods(
        &json!([{
            "id": "api-key:deepseek",
            "name": "DeepSeek API key",
            "_meta": { "api-key": { "variables": [
                { "name": "api-key", "secret": true }
            ] } }
        }]),
        &["fake-agent".into()],
        "/tmp",
        &Default::default(),
    );
    let method = methods.first().expect("method");
    // Missing values → no meta (the control lane rejects it visibly).
    assert!(authenticate_meta_from_method(method, &Default::default()).is_none());
    let values: BTreeMap<String, String> =
        [("api-key".to_string(), "sk-live".to_string())].into_iter().collect();
    let meta = authenticate_meta_from_method(method, &values).expect("meta");
    assert_eq!(meta["api-key"]["apiKey"].as_str(), Some("sk-live"));
}

#[test]
fn needs_auth_notice_gives_a_hint_per_method_kind() {
    // api-key (form) method → masked-field hint.
    let methods = parse_auth_methods(
        &json!([
            {
                "id": "api-key:x",
                "name": "Key",
                "_meta": { "api-key": { "variables": [{ "name": "api-key", "secret": true }] } }
            },
            {
                "id": "tui",
                "name": "Terminal",
                "_meta": { "terminal-auth": { "args": ["login"], "env": {} } }
            },
            { "id": "oauth", "name": "Browser" }
        ]),
        &["fake-agent".into()],
        "/tmp",
        &Default::default(),
    );
    for (id, needle) in [
        ("api-key:x", "masked field"),
        ("tui", "terminal sign-in"),
        ("oauth", "retry ACP authenticate"),
    ] {
        let method = methods.iter().find(|m| m.id == id).cloned();
        let snapshot = needs_auth_snapshot(methods.clone(), method.as_ref(), None);
        let (level, text) = snapshot
            .notice(crate::locale::Locale::En)
            .expect("needs-auth notice");
        assert_eq!(format!("{:?}", level), "Warn");
        assert!(text.contains("auth picker"), "{id}: {text}");
        assert!(text.contains(needle), "{id}: {text}");
    }
}

#[test]
fn terminal_method_type_uses_client_argv_with_method_args() {
    // `type: "terminal"` without a _meta block: still client argv[0] and
    // argv[1..] + method-level args/env merged with meta args/env (#160).
    let methods = parse_auth_methods(
        &json!([{
            "id": "t-login",
            "name": "Terminal",
            "type": "terminal",
            "args": ["auth"],
            "env": { "BASE": "1" },
            "_meta": { "args": ["login"], "env": { "EXTRA": "2" } }
        }]),
        &["fake-agent".into(), "--profile".into()],
        "/tmp",
        &Default::default(),
    );
    let launch = methods[0].terminal_launch.as_ref().expect("terminal launch");
    assert_eq!(launch.command, "fake-agent");
    assert_eq!(launch.args, vec!["--profile", "login"]);
    assert_eq!(launch.env.get("EXTRA").map(String::as_str), Some("2"));

    // A non-terminal method never gets a launch.
    let other = parse_auth_methods(
        &json!([{ "id": "oauth", "name": "Browser" }]),
        &["fake-agent".into()],
        "/tmp",
        &Default::default(),
    );
    assert!(other[0].terminal_launch.is_none());
}

#[test]
fn terminal_method_args_fall_back_when_meta_has_only_env() {
    let methods = parse_auth_methods(
        &json!([{
            "id": "t2",
            "name": "Terminal",
            "type": "terminal",
            "args": ["login"],
            "env": { "A": "1" },
            "_meta": { "env": { "B": "2" } }
        }]),
        &["fake-agent".into()],
        "/tmp",
        &Default::default(),
    );
    let launch = methods[0].terminal_launch.as_ref().expect("launch");
    assert_eq!(launch.args, vec!["login"]);
    assert_eq!(launch.env.get("B").map(String::as_str), Some("2"));
}

#[cfg(unix)]
#[test]
fn command_on_path_skips_non_executable_candidates() {
    // A file that exists on PATH but lacks +x must fail the executability
    // check and keep the search going — the short-circuit on a hit (or a
    // metadata error) is what unwrap_or(false) is for.
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("martty-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stub = dir.join("martty-stub-160");
    std::fs::write(&stub, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o644)).unwrap();
    // A directory never counts as a runnable command either.
    std::fs::create_dir_all(dir.join("martty-dircmd-160")).unwrap();
    let exe = dir.join("martty-runcmd-160");
    std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let saved = std::env::var_os("PATH");
    std::env::set_var("PATH", &dir);
    assert!(command_on_path("martty-stub-160").is_none());
    assert!(command_on_path("martty-dircmd-160").is_none());
    assert_eq!(command_on_path("martty-runcmd-160"), Some(exe));
    match &saved {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
