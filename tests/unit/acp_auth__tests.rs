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
    let values = values_from_auth_arg(&methods[0], "https://gw.example sk-test");
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
    let (_, text) = snap.notice().expect("notice");
    assert!(text.contains("/auth"));
    assert!(
        !text.contains("/login"),
        "/login is the agent's slash, not a TUI command: {text}"
    );
    assert!(text.contains("Log in with a DeepSeek API key"));
}
