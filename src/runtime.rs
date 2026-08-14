//! Locate the deepseek-harness SDK runtime (`dsh-jsonrpc-agent`) and its
//! bundled cordis config, and assemble the child environment.
//!
//! Search order mirrors the Python SDK's launch resolution, minus Python:
//! 1. `--runtime-bin` flag / `DSH_RUNTIME_BIN`
//! 2. `deepseek_harness_runtime` wheel in a nearby virtualenv
//!    (`.venv`/`venv` under the workspace, the exe dir, or `$DSB_HOME`)
//! 3. `dsh-jsonrpc-agent` on `PATH`

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

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

const WHEEL_RUNTIME_DIR: &str = "deepseek_harness_runtime/runtime";

fn wheel_runtime_in(venv: &Path) -> Option<PathBuf> {
    let lib = venv.join("lib");
    let entries = std::fs::read_dir(&lib).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("python") {
            continue;
        }
        let runtime_dir = entry.path().join("site-packages").join(WHEEL_RUNTIME_DIR);
        if let Some(bin) = runtime_bin_in(&runtime_dir) {
            return Some(bin);
        }
    }
    None
}

fn runtime_bin_in(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        if name.starts_with("dsh-jsonrpc-agent") && !name.ends_with("spawn-helper") {
            best = Some(entry.path());
        }
    }
    best
}

fn sibling_cordis(bin: &Path) -> Option<String> {
    let cfg = bin.parent()?.join("cordis.yml");
    cfg.exists().then(|| cfg.to_string_lossy().into_owned())
}

/// Find the runtime binary. `flag` is `--runtime-bin` when given.
pub fn discover_runtime(flag: Option<&str>, workspace: &str) -> Result<(String, Option<String>)> {
    if let Some(bin) = flag {
        let cordis = sibling_cordis(Path::new(bin));
        return Ok((bin.to_string(), cordis));
    }
    if let Ok(bin) = std::env::var("DSH_RUNTIME_BIN") {
        if !bin.is_empty() {
            let cordis = sibling_cordis(Path::new(&bin));
            return Ok((bin, cordis));
        }
    }

    // Nearby virtualenvs: workspace, cwd, exe dir, $DSB_HOME, $HOME/.deepseek-harness-tui
    let mut roots: Vec<PathBuf> = Vec::new();
    roots.push(PathBuf::from(workspace));
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        // exe dir, and its ancestors (target/debug -> project root)
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..4 {
            if let Some(d) = dir {
                roots.push(d.clone());
                dir = d.parent().map(Path::to_path_buf);
            } else {
                break;
            }
        }
    }
    if let Ok(home) = std::env::var("DSB_HOME") {
        roots.push(PathBuf::from(home));
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join(".deepseek-harness-tui"));
    }

    for root in &roots {
        for venv_name in [".venv", "venv"] {
            if let Some(bin) = wheel_runtime_in(&root.join(venv_name)) {
                let cordis = sibling_cordis(&bin);
                return Ok((bin.to_string_lossy().into_owned(), cordis));
            }
        }
        // A bare runtime dir (e.g. $DSB_HOME/runtime/…)
        if let Some(bin) = runtime_bin_in(&root.join("runtime")) {
            let cordis = sibling_cordis(&bin);
            return Ok((bin.to_string_lossy().into_owned(), cordis));
        }
    }

    // PATH
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("dsh-jsonrpc-agent");
            if candidate.exists() {
                let cordis = sibling_cordis(&candidate);
                return Ok((candidate.to_string_lossy().into_owned(), cordis));
            }
        }
    }

    bail!(
        "cannot find the deepseek-harness runtime (dsh-jsonrpc-agent).\n\
         Install it via `python -m pip install deepseek-harness-sdk` in a .venv next to your \
         workspace, or set DSH_RUNTIME_BIN / pass --runtime-bin."
    )
}

/// Resolve the cordis config: flag > env > sibling of the runtime binary.
pub fn resolve_cordis(flag: Option<&str>, sibling: Option<String>) -> Result<String> {
    if let Some(c) = flag {
        return Ok(c.to_string());
    }
    if let Ok(c) = std::env::var("DSH_CORDIS_CONFIG") {
        if !c.is_empty() {
            return Ok(c);
        }
    }
    if let Some(c) = sibling {
        return Ok(c);
    }
    bail!("no cordis config found: pass --cordis or set DSH_CORDIS_CONFIG")
}
