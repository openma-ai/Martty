//! Controller thread: owns the runtime lifecycle and executes UI commands.
//!
//! The UI thread never blocks on JSON-RPC. It sends [`Cmd`]s; the controller
//! spawns/initializes the runtime lazily, forwards prompts, and posts
//! [`CtlEvent`]s back over the bus. Interrupts kill the runtime process from
//! the UI thread directly (there is no interrupt RPC in the protocol); the
//! durable JSONL session survives and the next prompt respawns the runtime
//! with the same session id.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::bus::{AppEvent, CatalogModel, CatalogPreset, Cmd, CtlEvent};
use crate::proto::RuntimeProcess;
use crate::runtime::RuntimeConfig;

pub struct Controller {
    cmd_tx: Sender<Cmd>,
    runtime: Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    interrupted: Arc<AtomicBool>,
    demo: bool,
    attached: bool,
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
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Hard interrupt from the UI thread. Returns false when interrupting is
    /// impossible (demo mode / attached plugin mode).
    pub fn interrupt_now(&self) -> bool {
        if self.demo || self.attached {
            return false;
        }
        self.interrupted.store(true, Ordering::SeqCst);
        let guard = self.runtime.lock().unwrap();
        if let Some(rt) = guard.as_ref() {
            rt.kill();
        }
        true
    }

    pub fn is_attached(&self) -> bool {
        self.attached
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
            Cmd::Interrupt => {
                if attached {
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
                        match rt.request("tui/select-model", Some(params), Duration::from_secs(20)) {
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
                    // Standalone: restart semantics, same durable session.
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
                            CatalogModel { provider: "deepseek-official".into(), id: "deepseek-v4-flash".into(), name: "DeepSeek V4 Flash".into(), vision: false },
                            CatalogModel { provider: "deepseek-official".into(), id: "deepseek-v4-pro".into(), name: "DeepSeek V4 Pro".into(), vision: true },
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
                            "catalog needs plugin mode (dsh --profile tui)".into(),
                        )));
                    }
                }
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
                let result = rt
                    .filter(|rt| attached && rt.is_alive())
                    .map(|rt| {
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
                    let approval = if preset == "danger-full-access" { "never" } else { "ask" };
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
                let result = rt
                    .filter(|rt| attached && rt.is_alive())
                    .map(|rt| {
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
                        let staged = value.get("applied").and_then(Value::as_str)
                            == Some("on-first-prompt");
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
                            "permission presets need plugin mode (dsh --profile tui)".into(),
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
                let result = rt
                    .filter(|rt| attached && rt.is_alive())
                    .map(|rt| {
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
                            "agent modes need plugin mode (dsh --profile tui)".into(),
                        )));
                    }
                }
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

/// Plugin mode: the peer is the host dsh process — never spawn or kill;
/// initialize lazily once (and again after /model).
fn handle_prompt_attached(
    cfg: &RuntimeConfig,
    bus: &Sender<AppEvent>,
    runtime: &Arc<Mutex<Option<Arc<RuntimeProcess>>>>,
    initialized: &mut bool,
    session_id: &str,
    text: &str,
) {
    let Some(rt) = runtime.lock().unwrap().clone() else {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
            "harness connection is gone".into(),
        )));
        return;
    };
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
                return;
            }
        }
    }
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
                let spawned = RuntimeProcess::spawn(&cfg.bin, &cfg.child_env(), &cfg.workspace, bus.clone());
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
                name: p.get("name").and_then(Value::as_str).unwrap_or(id).to_string(),
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
    fn stock_presets_cover_the_four_web_ui_modes() {
        let presets = stock_presets();
        let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["standard", "code", "minimal", "creator"]);
    }
}
