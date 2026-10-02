//! Shared launch/display configuration for the ACP client and demo painter.

use std::path::{Path, PathBuf};

pub fn martty_home_from(
    martty_home: Option<&str>,
    dsh_home: Option<&str>,
    user_home: &str,
) -> PathBuf {
    if let Some(home) = martty_home.filter(|value| !value.is_empty()) {
        return PathBuf::from(home);
    }
    if let Some(home) = dsh_home.filter(|value| !value.is_empty()) {
        return Path::new(home).join(".martty");
    }
    Path::new(user_home).join(".martty")
}

pub fn martty_home() -> PathBuf {
    let martty = std::env::var("MARTTY_HOME").ok();
    let dsh = std::env::var("DSH_HOME").ok();
    let user = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    martty_home_from(martty.as_deref(), dsh.as_deref(), &user)
}

pub fn default_session_root() -> PathBuf {
    martty_home().join("sessions")
}

pub fn settings_path(session_root: &str) -> PathBuf {
    if Path::new(session_root) == default_session_root() {
        martty_home().join("settings.json")
    } else {
        Path::new(session_root).join("settings.json")
    }
}

pub fn legacy_settings_path(session_root: &str) -> PathBuf {
    if Path::new(session_root) == default_session_root() {
        let user = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Path::new(&user)
            .join(".dsh-tui")
            .join("sessions")
            .join("dsh-tui-settings.json")
    } else {
        Path::new(session_root).join("dsh-tui-settings.json")
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub bin: String,
    pub cordis: String,
    pub workspace: String,
    pub session_root: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<u64>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

impl RuntimeConfig {
    /// Environment for the child process, mirroring the SDK's injection.
    /// Credentials fall back to the local dsh install's store (~/.dsh), so a
    /// machine with a configured dsh needs no exported DEEPSEEK_API_KEY.
    pub fn child_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("DSH_CORDIS_CONFIG".into(), self.cordis.clone()),
            ("DSH_SESSION_ROOT".into(), self.session_root.clone()),
            ("DSH_CWD".into(), self.workspace.clone()),
        ];
        if let Some(url) = &self.base_url {
            env.push(("DEEPSEEK_BASE_URL".into(), url.clone()));
        }
        if let Some(key) = &self.api_key {
            env.push(("DEEPSEEK_API_KEY".into(), key.clone()));
        } else if std::env::var("DEEPSEEK_API_KEY").is_err() {
            let local = local_dsh();
            if let Some(key) = local.api_key {
                env.push(("DEEPSEEK_API_KEY".into(), key));
            }
            if self.base_url.is_none() && std::env::var("DEEPSEEK_BASE_URL").is_err() {
                if let Some(url) = local.base_url {
                    env.push(("DEEPSEEK_BASE_URL".into(), url));
                }
            }
        }
        env
    }

    pub fn has_credentials(&self) -> bool {
        self.api_key.is_some()
            || std::env::var("DEEPSEEK_API_KEY").is_ok()
            || local_dsh().api_key.is_some()
    }

    /// Spawn argv for the ACP agent (and Terminal Auth). `demo` falls back
    /// to `dsh-acp` so `/auth` still has a command to run.
    pub fn agent_argv(&self) -> Vec<String> {
        if self.bin.is_empty() || self.bin == "demo" {
            return vec!["dsh-acp".into()];
        }
        self.bin.split_whitespace().map(str::to_string).collect()
    }

    /// Human description of where the API key comes from.
    pub fn credential_source(&self) -> Option<&'static str> {
        if self.api_key.is_some() {
            Some("--api-key flag")
        } else if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            Some("environment")
        } else if local_dsh().api_key.is_some() {
            Some("local dsh (~/.dsh)")
        } else {
            None
        }
    }
}

/// Facts borrowed from a local `dsh` installation.
#[derive(Default, Clone, Debug)]
pub struct LocalDsh {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// Read the local dsh credential store and settings (best effort).
pub fn local_dsh() -> LocalDsh {
    let Ok(home) = std::env::var("HOME") else {
        return LocalDsh::default();
    };
    let root = Path::new(&home).join(".dsh");
    let mut out = LocalDsh::default();
    if let Ok(creds) = std::fs::read_to_string(root.join(".credentials.yaml")) {
        out.api_key = yaml_top_level_env(&creds, "DEEPSEEK_API_KEY");
        out.base_url = yaml_top_level_env(&creds, "DEEPSEEK_BASE_URL");
    }
    if let Ok(settings) = std::fs::read_to_string(root.join("settings.yaml")) {
        let (provider, model) = yaml_agent_default_model(&settings);
        out.provider = provider;
        out.model = model;
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(v);
    let v = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v);
    v.to_string()
}

/// `KEY: value` at zero indentation where KEY is an env-style name.
fn yaml_top_level_env(yaml: &str, key: &str) -> Option<String> {
    for line in yaml.lines() {
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() == key {
            let v = unquote(v);
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Built-in model when `--model`, `DSH_MODEL`, and the local dsh settings
/// file all omit one. Matches the id dsh-acp 0.4.36 advertises by default.
pub const DEFAULT_MODEL: &str = "deepseek-flash";

/// Rewrite a saved model id that pi-ai 0.87.1 dropped from a specific route.
///
/// `deepseek-v4-flash` and `deepseek-v4-flash-vision-exp` remain listed on
/// some third-party routes, so only the official DeepSeek routes (and an
/// empty route, which launch resolves to `deepseek-official`) move to
/// `deepseek-flash`. Moonshot dropped `kimi-k2.5` and the older `kimi-k2*`
/// previews; those map to `kimi-k2.6` only on `moonshotai` /
/// `moonshotai-cn`. A `provider/id` value uses the embedded provider.
/// Other ids, including a still-listed id on another route, pass through.
pub fn canonical_model_id(provider: &str, model: &str) -> String {
    let (embedded, id) = match model.split_once('/') {
        Some((route, id)) if !route.is_empty() && !id.is_empty() && !id.contains('/') => {
            (Some(route), id)
        }
        _ => (None, model),
    };
    let route = embedded.unwrap_or(provider);
    let Some(mapped) = retired_model_successor(route, id) else {
        return model.to_string();
    };
    match embedded {
        Some(route) => format!("{route}/{mapped}"),
        None => mapped.to_string(),
    }
}

fn retired_model_successor(provider: &str, id: &str) -> Option<&'static str> {
    match id {
        "deepseek-v4-flash" | "deepseek-v4-flash-vision-exp"
            if matches!(provider, "" | "deepseek" | "deepseek-official") =>
        {
            Some("deepseek-flash")
        }
        "kimi-k2.5"
        | "kimi-k2-0711-preview"
        | "kimi-k2-0905-preview"
        | "kimi-k2-thinking"
        | "kimi-k2-thinking-turbo"
        | "kimi-k2-turbo-preview"
            if matches!(provider, "moonshotai" | "moonshotai-cn") =>
        {
            Some("kimi-k2.6")
        }
        _ => None,
    }
}

/// Startup model: explicit flag, then `DSH_MODEL`, then the local dsh
/// settings file, then [`DEFAULT_MODEL`]. Retired ids are rewritten for the
/// resolved provider so an existing settings file keeps launching.
pub fn resolve_launch_model(
    explicit: Option<&str>,
    env_model: Option<&str>,
    local_model: Option<&str>,
    provider: &str,
) -> String {
    let raw = explicit
        .or(env_model)
        .or(local_model)
        .unwrap_or(DEFAULT_MODEL);
    canonical_model_id(provider, raw)
}

/// provider/model under the `agent-default-model:` block.
fn yaml_agent_default_model(yaml: &str) -> (Option<String>, Option<String>) {
    let mut in_block = false;
    let mut provider = None;
    let mut model = None;
    for line in yaml.lines() {
        if !line.starts_with([' ', '\t']) {
            in_block = line.trim_end() == "agent-default-model:";
            continue;
        }
        if !in_block {
            continue;
        }
        let Some((k, v)) = line.trim().split_once(':') else {
            continue;
        };
        let v = unquote(v);
        if v.is_empty() {
            continue;
        }
        match k.trim() {
            "provider" => provider = Some(v),
            "model" => model = Some(v),
            _ => {}
        }
    }
    (provider, model)
}

#[cfg(test)]
#[path = "../tests/unit/runtime__tests.rs"]
mod tests;
