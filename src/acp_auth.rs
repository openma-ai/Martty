//! ACP authentication, matching Backchat's probe (`packages/acp/src/probe.ts`).
//!
//! Supported method types: `agent` | `terminal` | `env_var`. Type comes from
//! `_meta.type` or `type`, else `agent`. In-app forms submit through
//! `authenticate` `_meta` (`api-key` / `gateway`). Terminal Auth is an
//! out-of-band child (`_meta["terminal-auth"]` or `type: "terminal"`); the
//! TTY stands in for Backchat's external terminal. `/login` stays the agent's
//! slash when advertised.

use std::collections::BTreeMap;
use std::process::Stdio;

use serde_json::{json, Map, Value};

use agent_client_protocol::schema::v1::{Error as AcpError, ErrorCode, Meta};

pub const ACP_AUTH_REQUIRED_CODE: i32 = -32000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Configured,
    NeedsAuth,
    None,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthVar {
    pub name: String,
    pub label: Option<String>,
    pub secret: Option<bool>,
    pub optional: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalAuthLaunch {
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub method_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethodInfo {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub type_name: String,
    pub form: bool,
    pub vars: Vec<AuthVar>,
    pub link: Option<String>,
    pub terminal_launch: Option<TerminalAuthLaunch>,
}

impl AuthMethodInfo {
    pub fn is_env_prompt(&self) -> bool {
        self.type_name == "env_var"
    }

    pub fn missing_env(&self, env: &BTreeMap<String, String>) -> Vec<String> {
        self.vars
            .iter()
            .filter(|v| v.optional != Some(true) && !env.contains_key(&v.name))
            .map(|v| v.name.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSnapshot {
    pub status: AuthStatus,
    pub method_id: Option<String>,
    pub method_name: Option<String>,
    pub methods: Vec<AuthMethodInfo>,
    pub message: Option<String>,
}

impl AuthSnapshot {
    pub fn none() -> Self {
        Self {
            status: AuthStatus::None,
            method_id: None,
            method_name: None,
            methods: Vec::new(),
            message: None,
        }
    }

    pub fn notice(&self) -> Option<(crate::transcript::NoticeLevel, String)> {
        use crate::transcript::NoticeLevel;
        match self.status {
            AuthStatus::Configured | AuthStatus::None => None,
            AuthStatus::Unknown => Some((
                NoticeLevel::Warn,
                self.message
                    .clone()
                    .unwrap_or_else(|| "no supported ACP auth method is available".into()),
            )),
            AuthStatus::NeedsAuth => Some((NoticeLevel::Warn, needs_auth_notice(self))),
        }
    }
}

pub fn client_capability_meta(interaction_mode: &str) -> Meta {
    let mut meta = Map::new();
    meta.insert("terminal-auth".into(), json!(true));
    meta.insert("terminal_output".into(), json!(true));
    meta.insert("subagent-transcript".into(), json!(true));
    meta.insert(
        "dsh".into(),
        json!({
            "cordis": { "protocol": crate::cordis::PROTOCOL },
            "interaction": { "mode": interaction_mode },
        }),
    );
    meta
}

pub fn auth_capability_meta() -> Meta {
    let mut meta = Map::new();
    meta.insert("gateway".into(), json!(true));
    meta
}

pub fn parse_auth_methods(
    auth_methods: &Value,
    agent_argv: &[String],
    cwd: &str,
    env: &BTreeMap<String, String>,
) -> Vec<AuthMethodInfo> {
    supported_auth_methods(auth_methods)
        .into_iter()
        .map(|raw| public_auth_method(&raw, agent_argv, cwd, env))
        .collect()
}

pub fn declared_auth_methods(auth_methods: &Value) -> Vec<Value> {
    let Some(arr) = auth_methods.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
        })
        .cloned()
        .collect()
}

pub fn supported_auth_methods(auth_methods: &Value) -> Vec<Value> {
    declared_auth_methods(auth_methods)
        .into_iter()
        .filter(|item| is_supported_type(&auth_method_type(item)))
        .collect()
}

pub fn unsupported_auth_method_types(auth_methods: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for item in declared_auth_methods(auth_methods) {
        let ty = auth_method_type(&item);
        if !is_supported_type(&ty) && !out.iter().any(|x| x == &ty) {
            out.push(ty);
        }
    }
    out
}

pub fn select_auth_method<'a>(
    methods: &'a [AuthMethodInfo],
    method_id: Option<&str>,
) -> Option<&'a AuthMethodInfo> {
    match method_id {
        Some(id) if !id.is_empty() => methods.iter().find(|m| m.id == id),
        _ => methods.first(),
    }
}

pub fn snapshot_from_methods(
    methods: Vec<AuthMethodInfo>,
    declared: &[Value],
    env: &BTreeMap<String, String>,
) -> AuthSnapshot {
    if methods.is_empty() {
        if declared.is_empty() {
            return AuthSnapshot::none();
        }
        let unsupported = unsupported_auth_method_types(&Value::Array(declared.to_vec()));
        let message = if unsupported.is_empty() {
            "no supported ACP auth method is available".into()
        } else {
            format!(
                "no supported ACP auth method is available. unsupported methods: {}.",
                unsupported.join(", ")
            )
        };
        return AuthSnapshot {
            status: AuthStatus::Unknown,
            method_id: None,
            method_name: None,
            methods,
            message: Some(message),
        };
    }
    let method = methods[0].clone();
    if method.is_env_prompt() {
        let missing = method.missing_env(env);
        if !missing.is_empty() {
            let message = if missing.len() == 1 {
                format!("missing credential variable: {}.", missing[0])
            } else {
                format!("missing credential variables: {}.", missing.join(", "))
            };
            return needs_auth_snapshot(methods, Some(&method), Some(message));
        }
    }
    if method.form {
        return needs_auth_snapshot(methods, Some(&method), None);
    }
    AuthSnapshot {
        status: AuthStatus::Configured,
        method_id: Some(method.id.clone()),
        method_name: method.name.clone(),
        methods,
        message: None,
    }
}

pub fn needs_auth_snapshot(
    methods: Vec<AuthMethodInfo>,
    method: Option<&AuthMethodInfo>,
    message: Option<String>,
) -> AuthSnapshot {
    AuthSnapshot {
        status: AuthStatus::NeedsAuth,
        method_id: method.map(|m| m.id.clone()),
        method_name: method.and_then(|m| m.name.clone()),
        methods,
        message,
    }
}

pub fn configured_snapshot(
    methods: Vec<AuthMethodInfo>,
    method: Option<&AuthMethodInfo>,
) -> AuthSnapshot {
    AuthSnapshot {
        status: AuthStatus::Configured,
        method_id: method.map(|m| m.id.clone()),
        method_name: method.and_then(|m| m.name.clone()),
        methods,
        message: None,
    }
}

pub fn authenticate_meta_from_method(
    method: &AuthMethodInfo,
    values: &BTreeMap<String, String>,
) -> Option<Meta> {
    if !method.form {
        return None;
    }
    let has_gateway = method.vars.iter().any(|v| v.name == "baseUrl")
        && method.vars.iter().any(|v| v.name == "api-key");
    if has_gateway {
        let base_url = values.get("baseUrl").map(|s| s.trim()).unwrap_or("");
        if base_url.is_empty() {
            return None;
        }
        let key = values.get("api-key").map(|s| s.trim()).unwrap_or("");
        let provider = values
            .get("providerName")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        let mut gateway = Map::new();
        gateway.insert("baseUrl".into(), json!(base_url));
        gateway.insert(
            "headers".into(),
            if key.is_empty() {
                json!({})
            } else {
                json!({ "Authorization": format!("Bearer {key}") })
            },
        );
        if let Some(name) = provider {
            gateway.insert("providerName".into(), json!(name));
        }
        let mut meta = Map::new();
        meta.insert("gateway".into(), Value::Object(gateway));
        return Some(meta);
    }
    let secret = values.get("api-key").map(|s| s.trim()).unwrap_or("");
    if secret.is_empty() {
        return None;
    }
    let mut meta = Map::new();
    meta.insert("api-key".into(), json!({ "apiKey": secret }));
    Some(meta)
}

pub fn values_from_auth_arg(method: &AuthMethodInfo, arg: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let arg = arg.trim();
    if arg.is_empty() {
        return values;
    }
    if method.vars.iter().any(|v| v.name == "baseUrl") {
        let mut parts = arg.splitn(2, char::is_whitespace);
        if let Some(url) = parts.next() {
            values.insert("baseUrl".into(), url.to_string());
        }
        if let Some(key) = parts.next().map(str::trim).filter(|s| !s.is_empty()) {
            values.insert("api-key".into(), key.to_string());
        }
        return values;
    }
    values.insert("api-key".into(), arg.to_string());
    values
}

pub fn is_auth_required_error(err: &AcpError) -> bool {
    if err.code == ErrorCode::AuthRequired || i32::from(err.code) == ACP_AUTH_REQUIRED_CODE {
        return true;
    }
    is_auth_failure_message(&acp_error_message(err))
}

pub fn acp_error_message(err: &AcpError) -> String {
    if let Some(data) = &err.data {
        if let Some(details) = data.get("details").and_then(Value::as_str) {
            if !details.is_empty() {
                return details.to_string();
            }
        }
        if let Some(message) = data.get("message").and_then(Value::as_str) {
            if !message.is_empty() {
                return message.to_string();
            }
        }
    }
    err.message.clone()
}

pub fn is_auth_failure_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("authentication required")
        || lower.contains("authentication fails")
        || lower.contains("invalid api key")
        || (lower.contains("api key") && lower.contains("invalid"))
}

pub fn process_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

pub fn run_terminal_auth(launch: &TerminalAuthLaunch) -> Result<(), String> {
    let mut cmd = std::process::Command::new(&launch.command);
    cmd.args(&launch.args);
    if let Some(cwd) = &launch.cwd {
        cmd.current_dir(cwd);
    }
    for (key, value) in &launch.env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "{} exited {}",
            launch.label,
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signaled".into())
        )),
        Err(err) => Err(format!("could not start {}: {err}", launch.label)),
    }
}

fn needs_auth_notice(snapshot: &AuthSnapshot) -> String {
    let name = snapshot
        .method_name
        .as_deref()
        .or(snapshot.method_id.as_deref())
        .unwrap_or("agent");
    let method = snapshot
        .method_id
        .as_ref()
        .and_then(|id| snapshot.methods.iter().find(|m| m.id == *id));
    let mut lines = vec![format!("sign-in needed · {name}")];
    if let Some(message) = &snapshot.message {
        if !message.is_empty() {
            lines.push(message.clone());
        }
    }
    if let Some(method) = method {
        if let Some(desc) = &method.description {
            if snapshot.message.is_none() {
                lines.push(desc.clone());
            }
        }
        if method.form {
            lines.push(
                "  /auth <api-key>  submits through ACP authenticate (dsh-tui does not keep it)"
                    .into(),
            );
        } else if method.terminal_launch.is_some() {
            lines.push("  /auth  runs the agent's terminal login, then ACP authenticate".into());
        } else {
            lines.push("  /auth  retries ACP authenticate after out-of-band login".into());
        }
        if let Some(link) = &method.link {
            lines.push(format!("  credential source · {link}"));
        }
    }
    lines.join("\n")
}

fn is_supported_type(ty: &str) -> bool {
    matches!(ty, "agent" | "terminal" | "env_var")
}

fn auth_method_type(method: &Value) -> String {
    if let Some(ty) = method.get("type").and_then(Value::as_str) {
        if !ty.is_empty() {
            return ty.to_string();
        }
    }
    if let Some(ty) = method
        .get("_meta")
        .or_else(|| method.get("meta"))
        .and_then(Value::as_object)
        .and_then(|m| m.get("type"))
        .and_then(Value::as_str)
    {
        if !ty.is_empty() {
            return ty.to_string();
        }
    }
    "agent".into()
}

fn auth_method_name(method: &Value) -> Option<String> {
    method
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn auth_method_description(method: &Value) -> Option<String> {
    method
        .get("description")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn auth_method_meta(method: &Value) -> Option<&Map<String, Value>> {
    method
        .get("_meta")
        .or_else(|| method.get("meta"))
        .and_then(Value::as_object)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string_record(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(obj) = value.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    obj.iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect()
}

fn public_auth_method(
    method: &Value,
    agent_argv: &[String],
    cwd: &str,
    env: &BTreeMap<String, String>,
) -> AuthMethodInfo {
    let id = method
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let api_key = auth_method_meta(method)
        .and_then(|m| m.get("api-key"))
        .and_then(Value::as_object)
        .is_some();
    let gateway = auth_method_meta(method)
        .and_then(|m| m.get("gateway"))
        .and_then(Value::as_object)
        .is_some();
    let mut vars = if gateway {
        vec![
            AuthVar {
                name: "baseUrl".into(),
                label: Some("Base URL".into()),
                secret: None,
                optional: None,
            },
            AuthVar {
                name: "api-key".into(),
                label: Some("API key".into()),
                secret: Some(true),
                optional: None,
            },
            AuthVar {
                name: "providerName".into(),
                label: Some("Provider".into()),
                secret: None,
                optional: Some(true),
            },
        ]
    } else if api_key {
        vec![AuthVar {
            name: "api-key".into(),
            label: Some("API key".into()),
            secret: Some(true),
            optional: None,
        }]
    } else {
        credential_vars(method)
    };
    let mut type_name = auth_method_type(method);
    if is_credential_prompt(method) {
        type_name = "env_var".into();
        if vars.is_empty() {
            vars = credential_vars(method);
        }
    }
    let form = api_key || gateway;
    let terminal_launch = if type_name == "terminal" || terminal_auth_block(method).is_some() {
        terminal_auth_from_method(method, agent_argv, cwd, env)
    } else {
        None
    };
    AuthMethodInfo {
        id: id.clone(),
        name: auth_method_name(method),
        description: auth_method_description(method),
        type_name,
        form,
        vars,
        link: method
            .get("link")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        terminal_launch: terminal_launch.map(|mut launch| {
            launch.method_id = id;
            launch
        }),
    }
}

fn terminal_auth_block(method: &Value) -> Option<&Map<String, Value>> {
    auth_method_meta(method)
        .and_then(|m| m.get("terminal-auth"))
        .and_then(Value::as_object)
}

fn terminal_auth_from_method(
    method: &Value,
    agent_argv: &[String],
    cwd: &str,
    env: &BTreeMap<String, String>,
) -> Option<TerminalAuthLaunch> {
    let label = auth_method_name(method).unwrap_or_else(|| "Login".into());
    let agent_command = agent_argv
        .first()
        .cloned()
        .unwrap_or_else(|| "dsh-acp".into());
    if let Some(block) = terminal_auth_block(method) {
        let command = block
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| agent_command.clone());
        let mut args = string_array(block.get("args"));
        if block
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .is_none()
            && agent_argv.len() > 1
        {
            let mut prefixed = agent_argv[1..].to_vec();
            prefixed.extend(args);
            args = prefixed;
        }
        let mut merged = env.clone();
        merged.extend(string_record(block.get("env")));
        return Some(TerminalAuthLaunch {
            label: block
                .get("label")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(&label)
                .to_string(),
            command,
            args,
            env: merged,
            cwd: Some(cwd.to_string()),
            method_id: String::new(),
        });
    }
    if auth_method_type(method) != "terminal" {
        return None;
    }
    let meta = auth_method_meta(method);
    let command = meta
        .and_then(|m| m.get("command"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(agent_command);
    let meta_args = string_array(meta.and_then(|m| m.get("args")));
    let args = if !meta_args.is_empty() {
        meta_args
    } else {
        string_array(method.get("args"))
    };
    let mut merged = env.clone();
    merged.extend(string_record(method.get("env")));
    merged.extend(string_record(meta.and_then(|m| m.get("env"))));
    Some(TerminalAuthLaunch {
        label,
        command,
        args,
        env: merged,
        cwd: Some(cwd.to_string()),
        method_id: String::new(),
    })
}

fn credential_vars(method: &Value) -> Vec<AuthVar> {
    if auth_method_type(method) == "env_var" {
        if let Some(vars) = method.get("vars").and_then(Value::as_array) {
            let parsed: Vec<AuthVar> = vars
                .iter()
                .filter_map(|item| {
                    let name = item.get("name").and_then(Value::as_str)?;
                    if name.is_empty() {
                        return None;
                    }
                    Some(AuthVar {
                        name: name.to_string(),
                        label: item
                            .get("label")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                        secret: item.get("secret").and_then(Value::as_bool),
                        optional: item.get("optional").and_then(Value::as_bool),
                    })
                })
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    inferred_credential_vars(method)
}

fn is_credential_prompt(method: &Value) -> bool {
    let ty = auth_method_type(method);
    ty == "env_var" || (ty == "terminal" && !inferred_credential_vars(method).is_empty())
}

fn inferred_credential_vars(method: &Value) -> Vec<AuthVar> {
    let Some(description) = auth_method_description(method) else {
        return Vec::new();
    };
    let lower = description.to_ascii_lowercase();
    if !(lower.contains("environment variable")
        || lower.contains("env var")
        || lower.contains("environment var"))
    {
        return Vec::new();
    }
    shouty_env_names(&description)
        .into_iter()
        .map(|name| AuthVar {
            secret: Some(secret_env_name(&name)),
            name,
            label: None,
            optional: None,
        })
        .collect()
}

fn shouty_env_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if raw.len() < 3
            || !raw.contains('_')
            || !raw
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            || !raw.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            continue;
        }
        if !out.iter().any(|x| x == raw) {
            out.push(raw.to_string());
        }
    }
    out
}

fn secret_env_name(name: &str) -> bool {
    name.split('_')
        .any(|part| matches!(part, "KEY" | "TOKEN" | "SECRET" | "PASSWORD" | "CREDENTIAL"))
}

#[cfg(test)]
mod tests {
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
}
