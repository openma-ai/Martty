//! Controller thread: executes UI commands.
//!
//! Live sessions delegate to the ACP client in `acp.rs`. The JSON-RPC branch
//! remains only for the private demo-skin compositor attach.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::bus::{AppEvent, CatalogModel, CatalogPreset, Cmd, CtlEvent, SkillInfo};
use crate::proto::RuntimeProcess;
use crate::runtime::RuntimeConfig;

pub struct Controller {
    cmd_tx: Sender<Cmd>,
    runtime: Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    interrupted: Arc<AtomicBool>,
    demo: bool,
    attached: bool,
    acp: bool,
}

impl Controller {
    pub fn start(
        cfg: RuntimeConfig,
        demo: bool,
        attached_rt: Option<Arc<RuntimeProcess>>,
        bus: Sender<AppEvent>,
    ) -> Controller {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let attached = attached_rt.is_some();
        let runtime: Arc<Mutex<Option<Arc<RuntimeProcess>>>> = Arc::new(Mutex::new(attached_rt));
        let interrupted = Arc::new(AtomicBool::new(false));

        {
            let runtime = Arc::clone(&runtime);
            let interrupted = Arc::clone(&interrupted);
            std::thread::Builder::new()
                .name("dsh-controller".into())
                .spawn(move || {
                    controller_loop(cfg, demo, attached, bus, cmd_rx, runtime, interrupted)
                })
                .expect("spawn controller");
        }

        Controller {
            cmd_tx,
            runtime,
            interrupted,
            demo,
            attached,
            acp: false,
        }
    }

    /// Live ACP: official rust-sdk client on spawn or inherited fds.
    /// Does not steal fds with [`RuntimeProcess::attach`].
    pub fn start_acp(
        cfg: RuntimeConfig,
        endpoint: crate::acp::AcpEndpoint,
        bus: Sender<AppEvent>,
    ) -> Controller {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let runtime: Arc<Mutex<Option<Arc<RuntimeProcess>>>> = Arc::new(Mutex::new(None));
        let interrupted = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new()
            .name("dsh-controller".into())
            .spawn(move || crate::acp::run_blocking(cfg, endpoint, bus, cmd_rx))
            .expect("spawn acp controller");
        Controller {
            cmd_tx,
            runtime,
            interrupted,
            demo: false,
            attached: true,
            acp: true,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Interrupt from the UI thread. Demo mode has no live turn; ACP mode
    /// delivers `Cmd::Interrupt` to the agent.
    pub fn interrupt_now(&self) -> bool {
        if self.demo {
            return false;
        }
        if self.attached || self.acp {
            return true;
        }
        self.interrupted.store(true, Ordering::SeqCst);
        let guard = self.runtime.lock().unwrap();
        if let Some(rt) = guard.as_ref() {
            rt.kill();
        }
        true
    }

    #[allow(dead_code)]
    pub fn runtime_alive(&self) -> bool {
        self.runtime
            .lock()
            .unwrap()
            .as_ref()
            .map(|rt| rt.is_alive())
            .unwrap_or(false)
    }
}

#[cfg(test)]
pub(crate) fn test_controller() -> (Controller, Receiver<Cmd>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    (
        Controller {
            cmd_tx,
            runtime: Arc::new(Mutex::new(None)),
            interrupted: Arc::new(AtomicBool::new(false)),
            demo: true,
            attached: false,
            acp: false,
        },
        cmd_rx,
    )
}

fn controller_loop(
    mut cfg: RuntimeConfig,
    demo: bool,
    attached: bool,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
    runtime: Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    interrupted: Arc<AtomicBool>,
) {
    let mut attached_initialized = false;
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Prompt { session_id, text } => {
                if demo {
                    crate::demo::run_demo_turn(bus.clone(), session_id, text);
                    continue;
                }
                if attached {
                    handle_prompt_attached(
                        &cfg,
                        &bus,
                        &runtime,
                        &mut attached_initialized,
                        &session_id,
                        &text,
                    );
                } else {
                    handle_prompt(&mut cfg, &bus, &runtime, &interrupted, &session_id, &text);
                }
            }
            Cmd::Steer {
                session_id, text, ..
            } => {
                if demo {
                    crate::demo::run_demo_turn(bus.clone(), session_id, text);
                    continue;
                }
                if attached {
                    handle_prompt_attached(
                        &cfg,
                        &bus,
                        &runtime,
                        &mut attached_initialized,
                        &session_id,
                        &text,
                    );
                } else {
                    handle_prompt(&mut cfg, &bus, &runtime, &interrupted, &session_id, &text);
                }
            }
            Cmd::PromptImages { session_id, blocks } => {
                if demo {
                    let text = blocks
                        .iter()
                        .filter_map(|block| match block {
                            crate::bus::PromptBlock::Text(text) => Some(text.as_str()),
                            crate::bus::PromptBlock::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    crate::demo::run_demo_turn(bus.clone(), session_id, text);
                    continue;
                }
                if attached {
                    handle_prompt_images_attached(
                        &cfg,
                        &bus,
                        &runtime,
                        &mut attached_initialized,
                        &session_id,
                        &blocks,
                    );
                } else {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                        "image attachments are unavailable on the legacy demo transport".into(),
                    )));
                }
            }
            Cmd::SteerImages {
                session_id, blocks, ..
            } => {
                if demo {
                    let text = blocks
                        .iter()
                        .filter_map(|block| match block {
                            crate::bus::PromptBlock::Text(text) => Some(text.as_str()),
                            crate::bus::PromptBlock::Image(_) => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    crate::demo::run_demo_turn(bus.clone(), session_id, text);
                    continue;
                }
                if attached {
                    handle_prompt_images_attached(
                        &cfg,
                        &bus,
                        &runtime,
                        &mut attached_initialized,
                        &session_id,
                        &blocks,
                    );
                } else {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                        "image attachments are unavailable on the legacy demo transport".into(),
                    )));
                }
            }
            Cmd::Interrupt { session_id } => {
                if attached {
                    // Forward to the host runner: agent.cancel({ kind: 'user' })
                    // aborts the active turn; a followup sent after this lands
                    // as the next turn.
                    let guard = runtime.lock().unwrap();
                    if let Some(rt) = guard.as_ref() {
                        let params = json!({ "sessionId": session_id });
                        let _ =
                            rt.request("session/interrupt", Some(params), Duration::from_secs(10));
                    }
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted));
                    continue;
                }
                // The kill itself happens in interrupt_now(); this is the
                // bookkeeping path so state changes are reported in order.
                interrupted.store(false, Ordering::SeqCst);
                let mut guard = runtime.lock().unwrap();
                if let Some(rt) = guard.take() {
                    rt.kill();
                }
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted));
            }
            Cmd::SelectModel {
                session_id,
                provider,
                model,
                effort,
            } => {
                if let Some(m) = &model {
                    cfg.model = m.clone();
                }
                if let Some(p) = &provider {
                    cfg.provider = p.clone();
                }
                if demo {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                        "model → {} (demo)",
                        cfg.model
                    ))));
                    continue;
                }
                if attached {
                    let rt = runtime.lock().unwrap().clone();
                    if let Some(rt) = rt {
                        let mut params = json!({ "sessionId": session_id });
                        if let Some(p) = provider {
                            params["provider"] = json!(p);
                        }
                        if let Some(m) = model {
                            params["model"] = json!(m);
                        }
                        if let Some(e) = effort {
                            params["reasoningEffort"] = json!(e);
                        }
                        match rt.request("tui/select-model", Some(params), Duration::from_secs(20))
                        {
                            Ok(_) => {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                                    "model → {} · live for this session",
                                    cfg.model
                                ))));
                            }
                            Err(err) => {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "hot switch failed: {err:#}"
                                ))));
                            }
                        }
                    }
                } else {
                    // Legacy spawned JSON-RPC runtime: restart, same durable session.
                    let mut guard = runtime.lock().unwrap();
                    if let Some(rt) = guard.take() {
                        rt.kill();
                    }
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                        "model → {} (runtime restarts on next prompt)",
                        cfg.model
                    ))));
                }
            }
            Cmd::FetchCatalog => {
                if demo {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog {
                        models: vec![
                            CatalogModel {
                                provider: "deepseek-official".into(),
                                id: "deepseek-v4-flash".into(),
                                name: "DeepSeek V4 Flash".into(),
                                vision: false,
                            },
                            CatalogModel {
                                provider: "deepseek-official".into(),
                                id: "deepseek-v4-pro".into(),
                                name: "DeepSeek V4 Pro".into(),
                                vision: true,
                            },
                        ],
                        presets: stock_presets(),
                    }));
                    continue;
                }
                let rt = runtime.lock().unwrap().clone();
                let result = rt
                    .filter(|rt| attached && rt.is_alive())
                    .map(|rt| rt.request("tui/catalog", Some(json!({})), Duration::from_secs(20)));
                match result {
                    Some(Ok(value)) => {
                        let (models, presets) = parse_catalog(&value);
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog { models, presets }));
                    }
                    Some(Err(err)) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "catalog unavailable: {err:#}"
                        ))));
                    }
                    None => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                            "catalog is unavailable on the legacy demo transport".into(),
                        )));
                    }
                }
            }
            Cmd::FetchSkills => {
                if demo {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Skills {
                        skills: vec![
                            SkillInfo {
                                name: "commit-helper".into(),
                                description: "draft a conventional commit from the diff".into(),
                                config_action: None,
                            },
                            SkillInfo {
                                name: "code-review".into(),
                                description: "structured review of the working tree".into(),
                                config_action: None,
                            },
                        ],
                    }));
                    continue;
                }
                let rt = runtime.lock().unwrap().clone();
                let result = rt
                    .filter(|rt| attached && rt.is_alive())
                    .map(|rt| rt.request("tui/skills", Some(json!({})), Duration::from_secs(20)));
                // The legacy demo transport has no skill registry — an empty catalog
                // simply leaves the slash menu with the builtins.
                let skills = match result {
                    Some(Ok(value)) => parse_skills(&value),
                    _ => Vec::new(),
                };
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Skills { skills }));
            }
            Cmd::FetchPlugins { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Plugins {
                    plugins: Vec::new(),
                }));
            }
            Cmd::SetPluginEnabled { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "dynamic plugins require the ACP transport".into(),
                )));
            }
            Cmd::InvokePluginCommand { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "client plugin commands require the ACP compositor transport".into(),
                )));
            }
            Cmd::PluginThemeSelected { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "client themes require the ACP compositor transport".into(),
                )));
            }
            Cmd::PluginOverlayEvent { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "client plugin overlays require the ACP compositor transport".into(),
                )));
            }
            Cmd::FetchEfforts { provider, model } => {
                if demo {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Efforts {
                        efforts: vec!["off".into(), "high".into(), "max".into()],
                        default: Some("high".into()),
                    }));
                    continue;
                }
                let rt = runtime.lock().unwrap().clone();
                let result = rt.filter(|rt| attached && rt.is_alive()).map(|rt| {
                    rt.request(
                        "tui/model-info",
                        Some(json!({ "provider": provider, "model": model })),
                        Duration::from_secs(20),
                    )
                });
                match result {
                    Some(Ok(value)) => {
                        let efforts: Vec<String> = value
                            .pointer("/reasoning/efforts")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|e| {
                                        e.get("id").and_then(Value::as_str).map(str::to_string)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let default = value
                            .pointer("/reasoning/defaultEffort")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Efforts { efforts, default }));
                    }
                    Some(Err(err)) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "model info unavailable: {err:#}"
                        ))));
                    }
                    None => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Efforts {
                            efforts: vec!["off".into(), "high".into(), "max".into()],
                            default: None,
                        }));
                    }
                }
            }
            Cmd::SetPermission { session_id, preset } => {
                if demo {
                    // Synthesize the event triplet the host would append.
                    let approval = if preset == "danger-full-access" {
                        "never"
                    } else {
                        "ask"
                    };
                    for (t, data) in [
                        ("permission/preset", json!({"preset": preset})),
                        ("sandbox/mode", json!({"mode": preset})),
                        ("approval/policy", json!({"policy": approval})),
                    ] {
                        let _ = bus.send(AppEvent::Rpc {
                            method: "session.event".into(),
                            params: json!({"sessionId": session_id, "event": {"type": t, "data": data}}),
                        });
                    }
                    continue;
                }
                let rt = runtime.lock().unwrap().clone();
                let result = rt.filter(|rt| attached && rt.is_alive()).map(|rt| {
                    rt.request(
                        "tui/permission",
                        Some(json!({ "sessionId": session_id, "preset": preset })),
                        Duration::from_secs(20),
                    )
                });
                match result {
                    Some(Ok(value)) => {
                        // The host stages a pre-session switch and applies it
                        // when the session is created on the first prompt.
                        let staged =
                            value.get("applied").and_then(Value::as_str) == Some("on-first-prompt");
                        let desc = if staged {
                            format!("permission → {preset} · staged, applies from the first prompt")
                        } else {
                            format!("permission → {preset}")
                        };
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(desc)));
                    }
                    Some(Err(err)) => {
                        let hint = if format!("{err}").contains("unknown permission preset")
                            || format!("{err}").contains("unknown preset")
                        {
                            " — /permission opens the preset picker"
                        } else {
                            ""
                        };
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "permission switch failed: {err:#}{hint}"
                        ))));
                    }
                    None => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                            "permission presets are unavailable on the legacy demo transport"
                                .into(),
                        )));
                    }
                }
            }
            Cmd::SetPreset { session_id, preset } => {
                if demo {
                    // Synthesize the durable fact the host records when the
                    // preset composes on the session's first prompt.
                    let _ = bus.send(AppEvent::Rpc {
                        method: "session.event".into(),
                        params: json!({"sessionId": session_id, "event": {
                            "type": "agent-preset/selected",
                            "data": {"agentPreset": preset}}}),
                    });
                    continue;
                }
                let rt = runtime.lock().unwrap().clone();
                let result = rt.filter(|rt| attached && rt.is_alive()).map(|rt| {
                    rt.request(
                        "tui/preset",
                        Some(json!({ "sessionId": session_id, "agentPreset": preset })),
                        Duration::from_secs(20),
                    )
                });
                match result {
                    Some(Ok(_)) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::PresetSet { preset }));
                    }
                    Some(Err(err)) => {
                        // The host locks the preset once the session's agent
                        // exists; a fresh session is the way out.
                        let hint = if format!("{err}").contains("locked") {
                            " — /new starts a fresh session, then pick the mode"
                        } else {
                            ""
                        };
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "mode switch failed: {err:#}{hint}"
                        ))));
                    }
                    None => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                            "agent modes are unavailable on the legacy demo transport".into(),
                        )));
                    }
                }
            }
            Cmd::Authenticate { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "ACP authenticate is only available on a live ACP connection".into(),
                )));
            }
            Cmd::SetConfigOption { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "session/set_config_option needs a live ACP connection".into(),
                )));
            }
            Cmd::NewSession | Cmd::ListSessions { .. } | Cmd::LoadSession { .. } => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "session/new, session/list, and session/load need a live ACP connection".into(),
                )));
            }
            Cmd::Shutdown => {
                let mut guard = runtime.lock().unwrap();
                if let Some(rt) = guard.take() {
                    rt.shutdown();
                }
                break;
            }
        }
    }
}

/// Private demo compositor attach: never spawn or kill; initialize lazily.
fn handle_prompt_attached(
    cfg: &RuntimeConfig,
    bus: &Sender<AppEvent>,
    runtime: &Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    initialized: &mut bool,
    session_id: &str,
    text: &str,
) {
    let Some(rt) = ensure_attached_ready(cfg, bus, runtime, initialized) else {
        return;
    };
    let params = json!({
        "sessionId": session_id,
        "contentBlocks": [{ "type": "text", "text": text }],
    });
    send_attached_prompt(&rt, bus, params);
}

/// Legacy compositor image prompt: commit each staged raster through the peer
/// attachment store, then send one prompt whose content blocks keep draft
/// order (text and images interleaved).
fn handle_prompt_images_attached(
    cfg: &RuntimeConfig,
    bus: &Sender<AppEvent>,
    runtime: &Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    initialized: &mut bool,
    session_id: &str,
    parts: &[crate::bus::PromptBlock],
) {
    let Some(rt) = ensure_attached_ready(cfg, bus, runtime, initialized) else {
        return;
    };
    let mut blocks = Vec::new();
    for part in parts {
        match part {
            crate::bus::PromptBlock::Text(text) => {
                if !text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
            }
            crate::bus::PromptBlock::Image(img) => {
                let attach = json!({
                    "data": img.data,
                    "mediaType": img.media_type,
                    "name": img.name,
                });
                match rt.request("tui/attach-image", Some(attach), Duration::from_secs(120)) {
                    Ok(result) => match result.get("attachment").cloned() {
                        Some(a) => blocks.push(json!({ "type": "image", "attachment": a })),
                        None => {
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                "tui/attach-image returned no attachment for {}",
                                img.name
                            ))));
                            return;
                        }
                    },
                    Err(err) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "tui/attach-image failed for {}: {err:#}",
                            img.name
                        ))));
                        return;
                    }
                }
            }
        }
    }
    let params = json!({ "sessionId": session_id, "contentBlocks": blocks });
    send_attached_prompt(&rt, bus, params);
}

/// Ensure the attached demo peer is initialized; returns the
/// live process, or None after reporting the failure.
fn ensure_attached_ready(
    cfg: &RuntimeConfig,
    bus: &Sender<AppEvent>,
    runtime: &Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    initialized: &mut bool,
) -> Option<Arc<RuntimeProcess>> {
    let rt = runtime.lock().unwrap().clone()?;
    if !*initialized {
        let mut params = json!({
            "cwd": cfg.workspace,
            "provider": cfg.provider,
            "model": cfg.model,
        });
        if let Some(max) = cfg.max_tokens {
            params["maxTokens"] = json!(max);
        }
        match rt.request("initialize", Some(params), Duration::from_secs(60)) {
            Ok(result) => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Ready {
                    server: describe_server(&result),
                }));
                *initialized = true;
            }
            Err(err) => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                    "initialize failed: {err:#}"
                ))));
                return None;
            }
        }
    }
    Some(rt)
}

fn send_attached_prompt(rt: &Arc<RuntimeProcess>, bus: &Sender<AppEvent>, params: Value) {
    match rt.request("session/prompt", Some(params), Duration::from_secs(120)) {
        Ok(result) => {
            let message_id = result
                .get("messageId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let _ = bus.send(AppEvent::Ctl(CtlEvent::PromptQueued { message_id }));
        }
        Err(err) => {
            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                "session/prompt failed: {err:#}"
            ))));
        }
    }
}

fn handle_prompt(
    cfg: &mut RuntimeConfig,
    bus: &Sender<AppEvent>,
    runtime: &Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    interrupted: &Arc<AtomicBool>,
    session_id: &str,
    text: &str,
) {
    interrupted.store(false, Ordering::SeqCst);

    // Ensure a live, initialized runtime.
    let rt = {
        let alive = runtime
            .lock()
            .unwrap()
            .as_ref()
            .filter(|rt| rt.is_alive())
            .cloned();
        match alive {
            Some(rt) => rt,
            None => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Starting {
                    runtime: cfg.bin.clone(),
                }));
                let spawned =
                    RuntimeProcess::spawn(&cfg.bin, &cfg.child_env(), &cfg.workspace, bus.clone());
                let rt = match spawned {
                    Ok(rt) => Arc::new(rt),
                    Err(err) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!("{err:#}"))));
                        return;
                    }
                };
                *runtime.lock().unwrap() = Some(Arc::clone(&rt));

                let mut params = json!({
                    "cwd": cfg.workspace,
                    "provider": cfg.provider,
                    "model": cfg.model,
                });
                if let Some(max) = cfg.max_tokens {
                    params["maxTokens"] = json!(max);
                }
                match rt.request("initialize", Some(params), Duration::from_secs(180)) {
                    Ok(result) => {
                        let server = describe_server(&result);
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Ready { server }));
                    }
                    Err(err) => {
                        rt.kill();
                        *runtime.lock().unwrap() = None;
                        if interrupted.swap(false, Ordering::SeqCst) {
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted));
                        } else {
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                "initialize failed: {err:#}"
                            ))));
                        }
                        return;
                    }
                }
                rt
            }
        }
    };

    // Queue the prompt into the durable inbox. While a turn is running this
    // is grok-style follow-up queueing, natively.
    let params = json!({
        "sessionId": session_id,
        "contentBlocks": [{ "type": "text", "text": text }],
    });
    match rt.request("session/prompt", Some(params), Duration::from_secs(120)) {
        Ok(result) => {
            let message_id = result
                .get("messageId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let _ = bus.send(AppEvent::Ctl(CtlEvent::PromptQueued { message_id }));
        }
        Err(err) => {
            if interrupted.swap(false, Ordering::SeqCst) {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted));
            } else {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                    "session/prompt failed: {err:#}"
                ))));
            }
        }
    }
}

fn describe_server(result: &Value) -> String {
    let name = result
        .pointer("/serverInfo/name")
        .and_then(Value::as_str)
        .unwrap_or("deepseek-harness");
    let version = result
        .pointer("/serverInfo/version")
        .and_then(Value::as_str)
        .unwrap_or("?");
    format!("{name} v{version}")
}

/// Parse the `tui/catalog` response into displayable models and presets.
/// `tui/skills` → the user-invocable skill catalog for the slash menu.
fn parse_skills(value: &Value) -> Vec<SkillInfo> {
    let mut out = Vec::new();
    if let Some(arr) = value.get("skills").and_then(Value::as_array) {
        for s in arr {
            let Some(name) = s.get("name").and_then(Value::as_str) else {
                continue;
            };
            let description = s
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            out.push(SkillInfo {
                name: name.to_string(),
                description,
                config_action: None,
            });
        }
    }
    out
}

fn parse_catalog(value: &Value) -> (Vec<CatalogModel>, Vec<CatalogPreset>) {
    let mut out = Vec::new();
    if let Some(arr) = value.get("models").and_then(Value::as_array) {
        for m in arr {
            let provider = m
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let Some(id) = m.get("id").and_then(Value::as_str) else {
                continue;
            };
            let name = m
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string();
            let vision = m.get("vision").and_then(Value::as_bool).unwrap_or(false);
            out.push(CatalogModel {
                provider,
                id: id.to_string(),
                name,
                vision,
            });
        }
    }
    let mut presets = Vec::new();
    if let Some(arr) = value.get("presets").and_then(Value::as_array) {
        for p in arr {
            let Some(id) = p.get("id").and_then(Value::as_str) else {
                continue;
            };
            presets.push(CatalogPreset {
                id: id.to_string(),
                name: p
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                description: p
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                broken: p.get("broken").and_then(Value::as_bool).unwrap_or(false),
            });
        }
    }
    (out, presets)
}

/// The four stock Web UI agent modes, used by the demo catalog.
fn stock_presets() -> Vec<CatalogPreset> {
    crate::app::AGENT_MODES
        .iter()
        .map(|(id, name, desc)| CatalogPreset {
            id: id.to_string(),
            name: name.to_string(),
            description: desc.to_string(),
            broken: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_catalog_models_and_presets() {
        let value = json!({
            "models": [
                {"provider": "deepseek-official", "id": "m1", "name": "M One", "vision": true},
                {"provider": "deepseek-official", "name": "no id → skipped"},
            ],
            "presets": [
                {"id": "standard", "name": "Standard mode", "description": "full agent"},
                {"id": "minimal"},
                {"id": "custom", "broken": true},
                {"name": "no id → skipped"},
            ],
        });
        let (models, presets) = parse_catalog(&value);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "m1");
        assert!(models[0].vision);

        assert_eq!(presets.len(), 3);
        assert_eq!(presets[0].id, "standard");
        assert_eq!(presets[0].name, "Standard mode");
        assert_eq!(presets[0].description, "full agent");
        assert!(!presets[0].broken);
        // name falls back to the id; missing description is empty.
        assert_eq!(presets[1].name, "minimal");
        assert_eq!(presets[1].description, "");
        assert!(presets[2].broken);
    }

    #[test]
    fn parse_catalog_without_presets_key() {
        let (models, presets) = parse_catalog(&json!({"models": []}));
        assert!(models.is_empty());
        assert!(presets.is_empty());
    }

    #[test]
    fn parse_skills_reads_names_and_skips_nameless() {
        let skills = parse_skills(&json!({
            "skills": [
                {"name": "commit-helper", "description": "draft a commit"},
                {"name": "bare"},
                {"description": "no name → skipped"},
            ]
        }));
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "commit-helper");
        assert_eq!(skills[0].description, "draft a commit");
        assert_eq!(skills[1].name, "bare");
        assert_eq!(skills[1].description, "");
        assert!(parse_skills(&json!({})).is_empty());
    }

    #[test]
    fn stock_presets_cover_the_four_web_ui_modes() {
        let presets = stock_presets();
        let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["standard", "code", "minimal", "cordis"]);
    }
}
