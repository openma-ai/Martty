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

use crate::bus::{AppEvent, Cmd, CtlEvent};
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
            Cmd::SetModel(model) => {
                cfg.model = model;
                if attached {
                    // Re-initialize on the live connection at the next prompt.
                    attached_initialized = false;
                    continue;
                }
                // initialize() is per-process: retire the current runtime so
                // the next prompt re-initializes with the new model.
                let mut guard = runtime.lock().unwrap();
                if let Some(rt) = guard.take() {
                    rt.kill();
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
