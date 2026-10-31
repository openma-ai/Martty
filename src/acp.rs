//! Official ACP client (`agent-client-protocol` 2.0).
//!
//! Speaks initialize / authenticate / session/new / session/fork / session/resume / session/load /
//! session/list / prompt / cancel / set_config_option / set_mode.
//! Transcript paint comes from `session/update`. Negotiated Cordis TUI
//! compositor state arrives as `_dsh/cordis/tui/*` extension notifications.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use agent_client_protocol::schema::v1::{
    AuthCapabilities, AuthenticateRequest, CancelNotification, ClientCapabilities,
    ClientSessionCapabilities, ContentBlock, CreateElicitationRequest, CreateElicitationResponse,
    CreateTerminalRequest, CreateTerminalResponse, ElicitationAcceptAction, ElicitationAction,
    ElicitationCapabilities, ElicitationContentValue, ElicitationFormCapabilities,
    ElicitationScope, ElicitationUrlCapabilities,
    FileSystemCapabilities, ImageContent, Implementation, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, ListSessionsRequest, LoadSessionRequest, LogoutRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResourceLink, ResumeSessionRequest,
    SelectedPermissionOutcome, SessionConfigOptionValue, SessionConfigOptionsCapabilities,
    SessionId, SessionNotification, SetSessionConfigOptionRequest, SetSessionModeRequest,
    TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    is_incoming_transport_closed, on_receive_notification, on_receive_request, AcpAgent, Agent,
    ByteStreams, Client, ConnectTo, ConnectionTo, Error as AcpError, Handled, SentRequest,
    UntypedMessage,
};

use crate::acp_auth::{
    acp_error_message, authenticate_meta_from_method, configured_snapshot, declared_auth_methods,
    is_auth_required_error, needs_auth_snapshot, parse_auth_methods, process_env,
    snapshot_from_methods, AuthMethodInfo, AuthSnapshot, AuthStatus,
};
use crate::bus::{
    permission_ask_empty_outcome, AppEvent, CatalogPreset, Cmd, CordisPluginItem, CtlEvent,
    PermissionAskOption, PermissionAskReply, SessionListItem, StaticPluginItem,
};
use crate::events::{
    catalog_from_config_options, config_option_events, flatten_select_options,
    reasoning_effort_option, session_modes_from_value, skills_from_available_commands,
};
use crate::runtime::RuntimeConfig;

mod control;
use control::{ControlFinish, ControlWorkers};

pub enum AcpEndpoint {
    Spawn(Vec<String>),
    AttachStdio {
        incoming: std::fs::File,
        outgoing: std::fs::File,
    },
    AttachTcp(std::net::TcpStream),
}

#[derive(Clone, Default)]
struct SessionSurface {
    connection: Option<Value>,
    composition_id: Option<String>,
    models: Vec<crate::bus::CatalogModel>,
    presets: Vec<CatalogPreset>,
    skills: Vec<crate::bus::SkillInfo>,
    efforts: Vec<String>,
    effort_config_id: Option<String>,
    effort_current: Option<String>,
    /// The `model` config option's currentValue — the provider in
    /// "provider/model" form is the credential this session runs on
    /// (issue #160).
    model_current: Option<String>,
    modes: Vec<CatalogPreset>,
}

#[derive(Default)]
struct Surface {
    /// Stream-owned clients have one negotiated connection for every tab.
    initial_connection: Option<crate::bus::SessionConnection>,
    auth_methods: HashMap<String, AuthMethodInfo>,
    failed_auth_setups: HashMap<String, Vec<String>>,
    active_session: Option<String>,
    sessions: HashMap<String, SessionSurface>,
    fallback: SessionSurface,
    /// A local Client compositor advertised one of its TUI projections.
    client_compositor: bool,
    /// Agent advertised `promptCapabilities.image` (ACP Image blocks allowed).
    prompt_image: bool,
    /// Agent negotiated the DSH Cordis ACP extension family.
    cordis: bool,
    /// How this agent takes a message into its active turn (Send Now).
    steer_route: SteerRoute,
    /// Providers the agent reports as credentialed via `_auth/status_update`
    /// (`providerId`, display name). `None` until the agent first reports —
    /// releases stay optimistic for agents that never send it (issue #160).
    authenticated_providers: Option<Vec<(String, String)>>,
}

impl Surface {
    fn steer_route_for(&self, id: &str) -> SteerRoute {
        self.session(id).connection.as_ref().map(steer_route).unwrap_or(self.steer_route)
    }

    fn active_connection(&self) -> Option<&Value> {
        self.active_session.as_deref().and_then(|id| self.session(id).connection.as_ref())
    }

    fn prompt_image_for(&self, id: &str) -> bool {
        self.session(id).connection.as_ref().map(prompt_image_supported).unwrap_or(self.prompt_image)
    }

    /// The active session's model provider — the credential `logout`
    /// should clear. A sole bound session is that session whether or not
    /// the UI's `ActiveSession` marker has been applied yet (issue #160).
    fn active_session_provider(&self) -> Option<String> {
        self.active_session
            .as_deref()
            .and_then(|id| self.session(id).model_current.clone())
            .or_else(|| {
                (self.sessions.len() == 1)
                    .then(|| self.sessions.values().next().and_then(|s| s.model_current.clone()))
                    .flatten()
            })
    }

    fn session_mut(&mut self, session_id: Option<&str>) -> &mut SessionSurface {
        match session_id {
            Some(id) => self.sessions.entry(id.to_string()).or_default(),
            None => &mut self.fallback,
        }
    }

    fn session(&self, session_id: &str) -> &SessionSurface {
        self.sessions.get(session_id).unwrap_or(&self.fallback)
    }

    fn apply_config_options(
        &mut self,
        options: &Value,
        bus: &Sender<AppEvent>,
        session_id: Option<&str>,
    ) {
        let (models, presets, composition_id) = catalog_from_config_options(options);
        let target = self.session_mut(session_id);
        target.models = models.clone();
        target.presets = presets.clone();
        if composition_id.is_some() {
            target.composition_id = composition_id;
        }
        if let Some(arr) = options.as_array() {
            if let Some(model) = arr
                .iter()
                .find(|o| o.get("id").and_then(Value::as_str) == Some("model"))
            {
                target.model_current = model
                    .get("currentValue")
                    .or_else(|| model.get("current_value"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            if let Some(effort) = reasoning_effort_option(options) {
                target.effort_config_id = effort
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                target.effort_current = effort
                    .get("currentValue")
                    .or_else(|| effort.get("current_value"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                target.efforts =
                    flatten_select_options(effort.get("options").unwrap_or(&Value::Null))
                        .into_iter()
                        .map(|(id, _, _)| id)
                        .collect();
            }
            // ACP config options are per-session: refresh whenever an
            // actual mode list shows up, even after an earlier session
            // filled the catalog — later sessions may carry a different
            // (or changed) set. The event still carries each session's
            // own currentValue, so per-session state is not lost.
            if let Some(mode) = arr
                .iter()
                .find(|o| o.get("id").and_then(Value::as_str) == Some("mode"))
            {
                let list: Vec<CatalogPreset> =
                    flatten_select_options(mode.get("options").unwrap_or(&Value::Null))
                        .into_iter()
                        .map(|(id, name, description)| CatalogPreset {
                            id,
                            name,
                            description,
                            broken: false,
                        })
                        .collect();
                let changed = !list.is_empty() && list != target.modes;
                if changed {
                    target.modes = list;
                }
                if !target.modes.is_empty() {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
                        session_id: session_id.map(str::to_string),
                        modes: target.modes.clone(),
                        current: mode
                            .get("currentValue")
                            .or_else(|| mode.get("current_value"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    }));
                }
            }
        }
        let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog {
            session_id: session_id.map(str::to_string),
            models,
            presets,
        }));
    }

    fn apply_session_modes(
        &mut self,
        modes: &Value,
        bus: &Sender<AppEvent>,
        session_id: Option<&str>,
    ) {
        let (list, current) = session_modes_from_value(modes);
        if list.is_empty() && current.is_none() {
            return;
        }
        if !list.is_empty() {
            self.session_mut(session_id).modes = list.clone();
        }
        let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
            session_id: session_id.map(str::to_string),
            modes: list,
            current,
        }));
    }
}

/// Run the ACP client until shutdown. Blocks the caller (controller thread).
pub fn run_blocking(
    cfg: RuntimeConfig,
    endpoint: AcpEndpoint,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                "acp runtime: {err}"
            ))));
            return;
        }
    };
    if let Err(err) = runtime.block_on(run(cfg, endpoint, bus.clone(), cmd_rx)) {
        // The painter retains the latest initialized identity, not cfg.bin from a previous Harness.
        let _ = bus.send(AppEvent::Ctl(CtlEvent::ConnectionFailed { target: String::new(), error: format!("{err:#}") }));
    }
}

/// Spawn the agent, `initialize`, print the negotiated name, exit.
pub fn check_blocking(argv: Vec<String>) -> Result<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("acp runtime")?;
    runtime.block_on(async {
        let agent = AcpAgent::from_args(argv).map_err(acp_err)?;
        Client
            .builder()
            .name("martty")
            .connect_with(agent, |cx: ConnectionTo<Agent>| async move {
                let init = cx.send_request(initialize_request()).block_task_deadline().await?;
                Ok(init
                    .agent_info
                    .map(|info| info.name)
                    .unwrap_or_else(|| "acp".into()))
            })
            .await
            .map_err(acp_err)
    })
}

fn acp_err(err: AcpError) -> anyhow::Error {
    anyhow::anyhow!("{err}")
}

pub(crate) fn initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_info(Implementation::new("martty", env!("CARGO_PKG_VERSION")))
        .client_capabilities(
            ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(true)
                .session(
                    ClientSessionCapabilities::new()
                        .config_options(SessionConfigOptionsCapabilities::new()),
                )
                .auth(
                    AuthCapabilities::new()
                        .terminal(true)
                        .meta(crate::acp_auth::auth_capability_meta()),
                )
                .elicitation(
                    ElicitationCapabilities::new()
                        .form(ElicitationFormCapabilities::new())
                        // URL mode is handled: the overlay shows the link so
                        // the user opens it manually (works headless/SSH).
                        .url(ElicitationUrlCapabilities::new()),
                )
                .meta(crate::acp_auth::client_capability_meta()),
        )
}

fn emit_auth(bus: &Sender<AppEvent>, snap: AuthSnapshot) {
    let _ = bus.send(AppEvent::Ctl(CtlEvent::Auth(snap)));
}

/// The composer payload of one prompt stalled on auth.
#[derive(Clone)]
enum ParkedPromptKind {
    Text(String),
    Images(Vec<crate::bus::PromptBlock>),
}

/// One stalled prompt with the session that owned it (`None` = it was
/// submitted before the first session existed and adopts the current
/// fallback on retry). A stall can hit several sessions' in-flight prompts
/// at once, so parked is a queue — never an overwriting slot (issue #94).
struct ParkedPrompt {
    session: Option<String>,
    kind: ParkedPromptKind,
}

struct PromptFinish {
    session_id: String,
    result: Result<agent_client_protocol::schema::v1::PromptResponse, AcpError>,
    payload: ParkedPromptKind,
    /// Turn generation: matches the `inflight` tag when this finish is the
    /// one that currently owns the session handle.
    gen: u64,
}

/// Per-session turn state behind the ACP transport. Each server allows one
/// in-flight `session/prompt` per session, so every bound session gets its own
/// handle; completions are demultiplexed by the id inside `PromptFinish`.
#[derive(Default)]
struct SessionHandle {
    /// Occupancy marker for the in-flight `session/prompt` task, tagged
    /// with the turn's generation. The task reports through the prompt-done
    /// channel, so the handle is only ever cleared, never awaited. The
    /// generation lets a stale finish (its session was forgotten and
    /// re-bound while the old task was still unwinding) be recognized and
    /// dropped instead of clearing the *new* turn's marker.
    inflight: Option<(u64, tokio::task::JoinHandle<()>)>,
    /// Follow-ups waiting for this session's active turn to settle.
    queue: VecDeque<Cmd>,
    /// `session/cancel` was sent for the in-flight prompt; its finish is
    /// reported as an interruption, not a turn result.
    turn_aborted: bool,
    /// Steers whose request has not returned yet. On the concurrent-prompt
    /// route a steer is a second `session/prompt`; agents that cannot join
    /// a turn (pi-acp) run it as the *next* turn, so its reply streams after
    /// the primary prompt returned and before its own response does. The
    /// session stays busy until every in-flight prompt has returned.
    steers_inflight: usize,
    /// Prompts requeued after an auth stall. Each one gets exactly one
    /// retry after sign-in: a second `-32000` means the agent refused the
    /// message outright, not "still unauthenticated" — re-parking it
    /// would stall the session forever (#160).
    auth_retries: usize,
}

/// Resolve the session a command addresses: the id the UI carried, or the
/// most recently bound session when the field is empty (backward compat).
/// Before the first session exists, any carried id is a local placeholder
/// with no server meaning yet, so it resolves to "no session" like an empty
/// field. Once sessions exist, an id this connection never bound is rejected
/// instead of silently rerouted to another session. `kind` names the command
/// in the rejection so the UI notice says what was dropped.
fn resolve_cmd_session(
    sessions: &HashMap<String, SessionHandle>,
    current: &Option<SessionId>,
    cmd_session: &str,
    kind: &str,
) -> std::result::Result<Option<SessionId>, String> {
    if cmd_session.is_empty() {
        return Ok(current.clone());
    }
    if sessions.contains_key(cmd_session) {
        return Ok(Some(SessionId::new(cmd_session)));
    }
    if sessions.is_empty() {
        return Ok(None);
    }
    Err(format!("{kind}: unknown session {cmd_session}"))
}

/// Register a freshly created or loaded session, make it the fallback target
/// for commands that arrive without a usable session id, and adopt any
/// prompts that queued up before the first session existed.
fn bind_session(
    sessions: &mut HashMap<String, SessionHandle>,
    current: &mut Option<SessionId>,
    pending: &mut VecDeque<Cmd>,
    sid: SessionId,
) {
    let handle = sessions.entry(sid.to_string()).or_default();
    handle.queue.append(pending);
    *current = Some(sid);
}

/// Point a prompt-like command at a freshly bound session: the id the UI
/// carried was a pre-bind placeholder with no server meaning yet.
fn retarget_session(cmd: &mut Cmd, sid: &SessionId) {
    match cmd {
        Cmd::Prompt { session_id, .. }
        | Cmd::Steer { session_id, .. }
        | Cmd::PromptImages { session_id, .. }
        | Cmd::SteerImages { session_id, .. } => *session_id = sid.to_string(),
        _ => {}
    }
}

struct SteerFinish {
    session_id: String,
    message_id: u64,
    /// The agent did not take the message into its active turn; the client
    /// FIFO re-owns it and sends it as the next `session/prompt`.
    deferred: bool,
}

/// How an agent accepts a message for its active turn (Send Now), negotiated
/// from the `initialize` response. Every route degrades to "deferred" when the
/// agent does not take the message: the client FIFO keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SteerRoute {
    /// Legacy agents with no steering advertisement: a second
    /// `session/prompt` while the first is in flight joins that turn
    /// (e.g. `@zed-industries/claude-agent-acp` <0.24 with
    /// `_meta.claudeCode.promptQueueing`).
    #[default]
    ConcurrentPrompt,
    /// The shared ACP steering extension: `_session/steering`, advertised by
    /// top-level `_meta.steering.supported` (claude-agent-acp, codex-acp,
    /// deepseek-harness-acp). `{outcome:"injected"}` joins the turn,
    /// `startedNewTurn` means the agent consumed it as a fresh turn, and
    /// `{outcome:"promptRequired"}` means the turn already ended.
    SessionSteering,
    /// MiniMax Code: `mcode/session/steer`, listed in
    /// `_meta["minimax-code/extensions"].methods`. Its `session/prompt`
    /// rejects concurrency outright, and steering carries text only.
    McodeSteer,
}

const SESSION_STEERING_METHOD: &str = "_session/steering";
const MCODE_STEER_METHOD: &str = "mcode/session/steer";

fn steer_route(init: &Value) -> SteerRoute {
    let steering_supported = |meta: Option<&Value>| {
        meta.and_then(|meta| meta.pointer("/steering/supported"))
            .and_then(Value::as_bool)
            == Some(true)
    };
    if steering_supported(init.get("_meta"))
        || steering_supported(init.pointer("/agentCapabilities/_meta"))
    {
        return SteerRoute::SessionSteering;
    }
    // JSON pointer escapes the `/` inside the extension key as `~1`.
    let mcode_methods = init
        .pointer("/_meta/minimax-code~1extensions/methods")
        .and_then(Value::as_array);
    if mcode_methods.is_some_and(|methods| {
        methods.iter().any(|m| m.as_str() == Some(MCODE_STEER_METHOD))
    }) {
        return SteerRoute::McodeSteer;
    }
    SteerRoute::ConcurrentPrompt
}

/// The wire form of one steer for a route.
enum SteerRequest {
    Prompt(Vec<ContentBlock>),
    Ext(UntypedMessage),
    /// The route cannot carry this payload (images on a text-only steer);
    /// the client FIFO sends it as the next prompt instead.
    Unsupported,
}

fn steer_request(
    route: SteerRoute,
    sid: &SessionId,
    content: Vec<ContentBlock>,
    message_id: u64,
) -> Result<SteerRequest, AcpError> {
    match route {
        SteerRoute::ConcurrentPrompt => Ok(SteerRequest::Prompt(content)),
        SteerRoute::SessionSteering => {
            let params = json!({
                "sessionId": sid.to_string(),
                "prompt": serde_json::to_value(&content)?,
                "_meta": { "steering": { "idleBehavior": "promptRequired" } },
            });
            Ok(SteerRequest::Ext(UntypedMessage::new(SESSION_STEERING_METHOD, params)?))
        }
        SteerRoute::McodeSteer => {
            let mut text = Vec::new();
            for block in &content {
                match block {
                    ContentBlock::Text(block) => text.push(block.text.as_str()),
                    _ => return Ok(SteerRequest::Unsupported),
                }
            }
            let text = text.join("\n");
            if text.trim().is_empty() {
                return Ok(SteerRequest::Unsupported);
            }
            let params = json!({
                "sessionId": sid.to_string(),
                "text": text,
                "clientRequestId": format!("martty-steer-{message_id}"),
            });
            Ok(SteerRequest::Ext(UntypedMessage::new(MCODE_STEER_METHOD, params)?))
        }
    }
}

/// Whether an extension steer left the message with the client FIFO.
fn steer_ext_deferred(route: SteerRoute, result: &Result<Value, AcpError>) -> bool {
    match (route, result) {
        (_, Err(_)) => true,
        // `injected` joined the turn; `startedNewTurn` consumed the message as
        // a new turn (only without `idleBehavior: promptRequired`). Either way
        // the agent owns it now — re-queueing would send it twice.
        (SteerRoute::SessionSteering, Ok(value)) => !matches!(
            value.get("outcome").and_then(Value::as_str),
            Some("injected" | "startedNewTurn")
        ),
        (SteerRoute::McodeSteer | SteerRoute::ConcurrentPrompt, Ok(_)) => false,
    }
}

/// Backchat `AcpSession.#prompt`: abort does not wait for `session/prompt`.
/// It fires `agent.cancel({ sessionId })` (`session/cancel`) and lets the
/// prompt promise settle on its own.
fn abort_turn(cx: &ConnectionTo<Agent>, session_id: &Option<SessionId>, bus: &Sender<AppEvent>) {
    if let Some(sid) = session_id.clone() {
        let _ = cx.send_notification(CancelNotification::new(sid.clone()));
        let _ = bus.send(AppEvent::Ctl(CtlEvent::CancelRequested {
            session_id: sid.to_string(),
        }));
    }
}

fn spawn_session_prompt(
    cx: ConnectionTo<Agent>,
    bus: Sender<AppEvent>,
    sid: SessionId,
    content: Vec<ContentBlock>,
    payload: ParkedPromptKind,
    gen: u64,
    done: tokio::sync::mpsc::UnboundedSender<PromptFinish>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnStart {
            session: sid.to_string(),
            turn: 0,
        }));
        let _ = bus.send(AppEvent::Rpc {
            method: "session.status".into(),
            params: json!({"sessionId": sid.to_string(), "status": "running"}),
        });
        let _ = bus.send(AppEvent::Ctl(CtlEvent::PromptQueued {
            message_id: sid.to_string(),
            session_id: Some(sid.to_string()),
        }));
        let result = cx
            .send_request(PromptRequest::new(sid.clone(), content))
            .block_task()
            .await;
        let _ = done.send(PromptFinish {
            session_id: sid.to_string(),
            result,
            payload,
            gen,
        });
    })
}

/// A steer belongs to the active turn, so it must not open or close a second
/// local turn lifecycle. On the concurrent-prompt route it is another
/// `session/prompt` sent while the active request is still in flight (no
/// deadline: it settles with that turn); extension routes are short control
/// requests and keep [`REQUEST_DEADLINE`].
fn spawn_steer(
    cx: ConnectionTo<Agent>,
    route: SteerRoute,
    sid: SessionId,
    content: Vec<ContentBlock>,
    message_id: u64,
    done: tokio::sync::mpsc::UnboundedSender<SteerFinish>,
) {
    let session_id = sid.to_string();
    let request = match steer_request(route, &sid, content, message_id) {
        Ok(request) => request,
        Err(_) => {
            let _ = done.send(SteerFinish { session_id, message_id, deferred: true });
            return;
        }
    };
    tokio::spawn(async move {
        let deferred = match request {
            SteerRequest::Prompt(content) => cx
                .send_request(PromptRequest::new(sid, content))
                .block_task()
                .await
                .is_err(),
            SteerRequest::Ext(request) => {
                let result = cx.send_request(request).block_task_deadline().await;
                steer_ext_deferred(route, &result)
            }
            SteerRequest::Unsupported => true,
        };
        let _ = done.send(SteerFinish { session_id, message_id, deferred });
    });
}

fn begin_prompt(
    cmd: Cmd,
    cx: &ConnectionTo<Agent>,
    bus: &Sender<AppEvent>,
    sid: SessionId,
    surface: &Arc<Mutex<Surface>>,
    workspace: &str,
    gen: u64,
    done: &tokio::sync::mpsc::UnboundedSender<PromptFinish>,
) -> Option<tokio::task::JoinHandle<()>> {
    match cmd {
        Cmd::Prompt { text, .. } | Cmd::Steer { text, .. } => Some(spawn_session_prompt(
            cx.clone(),
            bus.clone(),
            sid,
            vec![text.clone().into()],
            ParkedPromptKind::Text(text),
            gen,
            done.clone(),
        )),
        Cmd::PromptImages { blocks, .. } | Cmd::SteerImages { blocks, .. } => {
            let payload = ParkedPromptKind::Images(blocks.clone());
            let prompt_image = surface
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .prompt_image_for(&sid.0);
            match prompt_content_blocks(blocks, prompt_image, workspace) {
                Ok(content) if !content.is_empty() => Some(spawn_session_prompt(
                    cx.clone(),
                    bus.clone(),
                    sid,
                    content,
                    payload,
                    gen,
                    done.clone(),
                )),
                Ok(_) => {
                    // Session-scoped: the failure belongs to the requesting
                    // tab (possibly parked), never to the viewed one.
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                        session_id: sid.to_string(),
                        message: "empty image prompt".into(),
                    }));
                    None
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                        session_id: sid.to_string(),
                        message: err,
                    }));
                    None
                }
            }
        }
        _ => None,
    }
}

fn connection_id(surface: &Arc<Mutex<Surface>>, session: &str) -> Option<String> {
    surface.lock().unwrap_or_else(|e| e.into_inner()).session(session).connection.as_ref()
        .and_then(|value| value.get("id")).and_then(Value::as_str).map(str::to_string)
}

fn auth_stalled_for(session: &str, parked: &VecDeque<ParkedPrompt>, surface: &Arc<Mutex<Surface>>) -> bool {
    let owner = connection_id(surface, session);
    parked.iter().any(|prompt| prompt.session.as_deref()
        .map(|id| connection_id(surface, id) == owner).unwrap_or(true))
}

fn requeue_connection_prompts(
    sessions: &mut HashMap<String, SessionHandle>, current: &Option<SessionId>,
    parked: &mut VecDeque<ParkedPrompt>, surface: &Arc<Mutex<Surface>>, owner: Option<&str>,
) -> usize {
    let (mut ready, waiting): (VecDeque<_>, VecDeque<_>) = parked.drain(..).partition(|prompt| {
        owner.is_none() || prompt.session.as_deref()
            .is_some_and(|id| connection_id(surface, id).as_deref() == owner)
    });
    let retried = requeue_parked_prompts(sessions, current, &mut ready, surface);
    *parked = waiting;
    parked.extend(ready);
    retried
}

/// The authenticate success line: a bare `signed in`, the count of
/// prompts the sign-in just released, and — when the sign-in covered a
/// different account — the providers held prompts still wait for
/// (issue #160).
fn signed_in_notice(retried: usize, held: &[String]) -> String {
    let mut line = if retried > 0 {
        format!("signed in — retried {retried} parked prompt{s}", s = if retried == 1 { "" } else { "s" })
    } else {
        "signed in".into()
    };
    if !held.is_empty() {
        line += &format!(" · held prompt still waits for {}", held.join(", "));
    }
    line
}

fn emit_method_auth(bus: &Sender<AppEvent>, surface: &Arc<Mutex<Surface>>, snapshot: AuthSnapshot) {
    let targets: Vec<_> = {
        let s = surface.lock().unwrap_or_else(|e| e.into_inner());
        s.sessions.iter().filter_map(|(id, session)| {
            let connection = session.connection.as_ref()?;
            let methods = session_connection_snapshot(connection).auth.methods;
            if !methods.iter().any(|method| Some(&method.id) == snapshot.method_id.as_ref()) { return None; }
            let mut scoped = snapshot.clone();
            scoped.methods = methods;
            Some((id.clone(), scoped))
        }).collect()
    };
    if targets.is_empty() { emit_auth(bus, snapshot); }
    else { for (session_id, snapshot) in targets {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open: false }));
    } }
}

/// Start queued prompts whose session is free. Each session runs at most one
/// in-flight `session/prompt`; a busy session simply keeps its FIFO.
#[allow(clippy::too_many_arguments)]
fn drain_ready_sessions(
    controls: &ControlWorkers,
    sessions: &mut HashMap<String, SessionHandle>,
    cx: &ConnectionTo<Agent>,
    bus: &Sender<AppEvent>,
    parked: &mut VecDeque<ParkedPrompt>,
    surface: &Arc<Mutex<Surface>>,
    current: &Option<SessionId>,
    workspace: &str,
    next_gen: &mut u64,
    done: &tokio::sync::mpsc::UnboundedSender<PromptFinish>,
) {
    // A provider that became credentialed since the prompt parked (a
    // later sign-in on another account, or a model switch onto a signed-in
    // provider) releases its stall without an explicit retry. Only agents
    // that report credentialed providers (`_auth/status_update`) can prove
    // that — for the rest, releases stay driven by sign-in/success exactly
    // as before (issue #160).
    let reports_providers = surface
        .lock()
        .map(|guard| guard.authenticated_providers.is_some())
        .unwrap_or(false);
    if reports_providers && !parked.is_empty() {
        requeue_parked_prompts(sessions, current, parked, surface);
    }
    // A credential stall applies only to sessions owned by that connection.
    let keys: Vec<String> = sessions.keys().cloned().collect();
    for key in keys {
        if auth_stalled_for(&key, parked, surface) { continue; }
        let Some(handle) = sessions.get_mut(&key) else {
            continue;
        };
        if handle.inflight.is_some() || controls.busy(&key) {
            continue;
        }
        let Some(next) = handle.queue.pop_front() else {
            continue;
        };
        *next_gen += 1;
        handle.inflight = begin_prompt(
            next,
            cx,
            bus,
            SessionId::new(key.as_str()),
            surface,
            workspace,
            *next_gen,
            done,
        )
        .map(|task| (*next_gen, task));
    }
}

/// Send stalled prompts back to the sessions that owned them — an
/// `authenticate` just succeeded, or another prompt succeeded, either of
/// which proves the stall is over. Entries whose session was closed while
/// stalled (`/close` → ForgetSession) are dropped: nobody views them, and
/// retrying into the fallback session would leak the message into a
/// conversation it never belonged to. Pre-session entries (no owner yet)
/// adopt the current fallback session; with no session at all they stay
/// parked for the next release. Returns how many prompts were requeued.
fn requeue_parked_prompts(
    sessions: &mut HashMap<String, SessionHandle>,
    current: &Option<SessionId>,
    parked: &mut VecDeque<ParkedPrompt>,
    surface: &Arc<Mutex<Surface>>,
) -> usize {
    let mut retried = 0;
    let mut still_parked: VecDeque<ParkedPrompt> = VecDeque::new();
    while let Some(p) = parked.pop_front() {
        {
            let guard = surface.lock().unwrap_or_else(|e| e.into_inner());
            if !release_ready(&guard, p.session.as_deref(), current) {
                still_parked.push_back(p);
                continue;
            }
        }
        let target = match p.session {
            Some(sid) if sessions.contains_key(&sid) => Some(sid),
            Some(_) => None, // closed while stalled — drop
            None => current.as_ref().map(|c| c.to_string()),
        };
        match (target, p.kind) {
            (Some(sid), kind) => {
                let cmd = match kind {
                    ParkedPromptKind::Text(text) => Cmd::Prompt {
                        session_id: sid.clone(),
                        text,
                    },
                    ParkedPromptKind::Images(blocks) => Cmd::PromptImages {
                        session_id: sid.clone(),
                        blocks,
                    },
                };
                let handle = sessions.entry(sid).or_default();
                handle.queue.push_front(cmd);
                handle.auth_retries += 1;
                retried += 1;
            }
            (None, kind) => still_parked.push_back(ParkedPrompt { session: None, kind }),
        }
    }
    *parked = still_parked;
    retried
}

fn apply_prompt_finish(
    finish: PromptFinish,
    sessions: &mut HashMap<String, SessionHandle>,
    parked: &mut VecDeque<ParkedPrompt>,
    bus: &Sender<AppEvent>,
    methods: &[AuthMethodInfo],
    provider: Option<&str>,
) {
    match finish.result {
        Ok(response) => {
            let finish_kind = match response.stop_reason {
                agent_client_protocol::schema::v1::StopReason::EndTurn => "completed",
                agent_client_protocol::schema::v1::StopReason::MaxTokens => "max-tokens",
                agent_client_protocol::schema::v1::StopReason::MaxTurnRequests => {
                    "max-turn-requests"
                }
                agent_client_protocol::schema::v1::StopReason::Refusal => "blocked",
                agent_client_protocol::schema::v1::StopReason::Cancelled => "interrupted",
                _ => "unknown",
            };
            if let Some(usage) = response.usage {
                let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::Usage {
                    session: finish.session_id.clone(),
                    input: usage.input_tokens,
                    output: usage.output_tokens,
                    cached: usage.cached_read_tokens.unwrap_or(0)
                        + usage.cached_write_tokens.unwrap_or(0),
                    reasoning: usage.thought_tokens.unwrap_or(0),
                }));
            }
            let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnEnd {
                session: finish.session_id,
                kind: finish_kind.into(),
            }));
            // A success proves the stall is over — the release happens in
            // the caller (it owns the session map); nothing parks here.
        }
        Err(err) if is_auth_required_error(&err) => {
            // A requeued prompt refused again already had its sign-in
            // retry: the refusal is the agent's answer to the message
            // itself, so drop it visibly instead of re-parking — a held
            // entry would stall this session's queue forever (#160).
            let already_retried = sessions
                .get_mut(&finish.session_id)
                .is_some_and(|handle| {
                    if handle.auth_retries > 0 {
                        handle.auth_retries -= 1;
                        true
                    } else {
                        false
                    }
                });
            if already_retried {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                    session_id: finish.session_id.clone(),
                    message: format!(
                        "still refused after sign-in — prompt discarded: {}",
                        acp_error_message(&err)
                    ),
                }));
                let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnEnd {
                    session: finish.session_id.clone(),
                    kind: "interrupted".into(),
                }));
                // The sign-in *succeeded* — this refusal answers the
                // retried message, not the credential. Re-opening the
                // picker or flipping the status back to "sign-in needed"
                // would lie about the auth state and loop the sign-in
                // prompt after every accepted credential (issue #160).
            } else {
                // Park the payload with its owning session so the retry
                // (after `authenticate`) lands on the right tab.
                parked.push_back(ParkedPrompt {
                    session: Some(finish.session_id.clone()),
                    kind: finish.payload,
                });
                let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnEnd {
                    session: finish.session_id.clone(),
                    kind: "interrupted".into(),
                }));
                if let Some(value) = err.data.as_ref().and_then(|data| data.get("marttyConnection")) {
                    let mut snapshot = session_connection_snapshot(value).auth;
                    snapshot.status = AuthStatus::NeedsAuth;
                    snapshot.message = Some(acp_error_message(&err));
                    name_needs_auth(&mut snapshot, methods, provider);
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionAuth { session_id: finish.session_id, snapshot, open: true }));
                } else {
                    emit_needs_auth_open(bus, methods.to_vec(), provider, Some(acp_error_message(&err)));
                }
            }
        }
        Err(err) => {
            let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnEnd {
                session: finish.session_id.clone(),
                kind: "error".into(),
            }));
            let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                session_id: finish.session_id,
                message: format!("prompt: {}", acp_error_message(&err)),
            }));
        }
    }
}

/// A terminal sign-in's post-reconnect bind: credit the method the launch
/// ran — the blind "sign-in needed" snapshot cannot name it (issue #160).
fn credit_terminal_method(
    bus: &Sender<AppEvent>,
    surface: &Arc<Mutex<Surface>>,
    methods: &[AuthMethodInfo],
    method_id: &Option<String>,
) {
    let Some(id) = method_id.as_deref() else { return };
    let Some(method) = methods.iter().find(|method| method.id == id) else { return };
    emit_method_auth(bus, surface, configured_snapshot(methods.to_vec(), Some(method)));
}

/// The provider a model value runs on: the head of
/// "deepseek/deepseek-v4-pro" or "deepseek-official::deepseek-flash" —
/// the agent's own string, never expanded into guesses (issue #160).
fn provider_head(model_value: &str) -> &str {
    model_value
        .split(['/', ':'])
        .next()
        .unwrap_or(model_value)
}

/// A refusal names the credential that failed — the provider the session
/// runs on — never the method the user last picked. Only an unambiguous
/// exact match on the id's provider suffix counts: several methods
/// embedding the token, or none, means the failing credential is unknown
/// and the notice stays generic rather than guess wrong (issue #160).
fn method_for_provider<'a>(
    methods: &'a [AuthMethodInfo],
    provider: &str,
) -> Option<&'a AuthMethodInfo> {
    let provider = provider_head(provider).to_lowercase();
    let mut hits = methods
        .iter()
        .filter(|method| method_provider_id(method).map(str::to_lowercase).as_deref() == Some(provider.as_str()));
    match (hits.next(), hits.next()) {
        (Some(method), None) => Some(method),
        _ => None,
    }
}

/// One advertised method can only ever be the credential in question —
/// naming it is not a guess (issue #160).
fn sole_method(methods: &[AuthMethodInfo]) -> Option<&AuthMethodInfo> {
    (methods.len() == 1).then(|| &methods[0])
}

/// The provider a method id embeds: `api-key:openrouter` signs `openrouter`
/// in, so the suffix after the first `:` is the credential's provider — an
/// id the agent itself chose, not a substring guess (issue #160).
fn method_provider_id(method: &AuthMethodInfo) -> Option<&str> {
    method
        .id
        .split_once(':')
        .map(|(_, provider)| provider)
        .filter(|provider| !provider.is_empty())
}

/// Which provider the agent's `logout` should have cleared: the bound
/// session's model provider when it names exactly one advertised
/// credential id, else the method the user signed in with, else the only
/// provider the agent advertises. The name goes in the notice only — the
/// request itself is a bare `logout` and the agent picks the scope (issue
/// #160).
fn logout_provider(
    surface: &Arc<Mutex<Surface>>,
    methods: &[AuthMethodInfo],
    selected: Option<&AuthMethodInfo>,
) -> Option<String> {
    let mut ids: Vec<&str> = methods.iter().filter_map(method_provider_id).collect();
    ids.sort_unstable();
    ids.dedup();
    let session_provider = {
        let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
        surface.active_session_provider()
    };
    session_provider
        .as_deref()
        .map(provider_head)
        .filter(|head| ids.contains(head))
        .map(str::to_string)
        .or_else(|| selected.and_then(method_provider_id).map(str::to_string))
        .or_else(|| (ids.len() == 1).then(|| ids[0].to_string()))
}

/// A NeedsAuth snapshot carrying no method falls back to the bare word
/// "agent" in the notice — a method type, not a credential. Credit the
/// method matching the provider the failing credential belongs to (the
/// session's running model, or the launch provider before a session
/// exists); a lone advertised method names itself; anything else stays
/// generic rather than guess wrong (issue #160).
fn name_needs_auth(
    snapshot: &mut AuthSnapshot,
    methods: &[AuthMethodInfo],
    provider: Option<&str>,
) {
    if snapshot.method_id.is_some() {
        return;
    }
    let named = |m: &AuthMethodInfo| (m.id.clone(), m.name.clone());
    let picked = provider
        .and_then(|p| {
            method_for_provider(&snapshot.methods, p)
                .or_else(|| method_for_provider(methods, p))
        })
        .or_else(|| sole_method(&snapshot.methods))
        .or_else(|| sole_method(methods));
    if let Some((id, name)) = picked.map(named) {
        snapshot.method_id = Some(id);
        snapshot.method_name = name;
    }
}

/// A bound session's provider — the `model` config option's currentValue
/// ("deepseek/deepseek-v4-pro") when the agent advertises one (issue
/// #160).
fn session_provider(surface: &Arc<Mutex<Surface>>, session_id: &str) -> Option<String> {
    let guard = surface.lock().unwrap_or_else(|e| e.into_inner());
    guard.session(session_id).model_current.clone()
}

/// Held prompts on a session this bind replaces ride along to the new
/// session id; entries owned by other sessions stay put (issue #160).
fn retarget_parked(parked: &mut VecDeque<ParkedPrompt>, previous_id: &str, sid: &str) {
    for prompt in parked.iter_mut() {
        if prompt.session.as_deref() == Some(previous_id) {
            prompt.session = Some(sid.to_string());
        }
    }
}

/// Switching accounts can leave the bound session on a credential the agent
/// no longer reports: the provider of `current` is missing from the reported
/// set only when the agent reports at all (issue #160).
fn dead_provider(surface: &Arc<Mutex<Surface>>, current: &Option<SessionId>) -> bool {
    let guard = surface.lock().unwrap_or_else(|e| e.into_inner());
    match (guard.authenticated_providers.as_ref(), current.as_ref()) {
        (Some(set), Some(cur)) => {
            let provider = guard.session(&cur.0).model_current.as_deref().map(provider_head);
            provider.is_some() && !set.iter().any(|(id, _)| Some(id.as_str()) == provider)
        }
        _ => false,
    }
}

/// A success proves its provider is credentialed — credit it into the
/// reported set so same-provider held prompts can release (issue #160).
fn credit_authenticated_provider(surface: &Arc<Mutex<Surface>>, provider: &str) {
    let mut guard = surface.lock().unwrap_or_else(|e| e.into_inner());
    let Some(set) = guard.authenticated_providers.as_mut() else {
        return;
    };
    let provider = provider_head(provider).to_string();
    if set.iter().all(|(id, _)| id != &provider) {
        set.push((provider.clone(), provider));
    }
}

/// Whether a parked prompt may go back on the wire. Once the agent has
/// reported its credentialed providers (`_auth/status_update`), a held
/// prompt releases only when the session's model provider is covered —
/// signing into a different account must not burn it on a refusal that
/// cannot succeed (issue #160). Without a report — or a session naming no
/// provider — the release stays optimistic, as before.
fn release_ready(
    surface: &Surface,
    session_id: Option<&str>,
    current: &Option<SessionId>,
) -> bool {
    let Some(set) = &surface.authenticated_providers else {
        return true;
    };
    let provider = match session_id {
        Some(id) => surface.session(id).model_current.as_deref().map(provider_head),
        None => current
            .as_ref()
            .and_then(|id| surface.session(&id.0).model_current.as_deref().map(provider_head)),
    };
    match provider {
        Some(provider) => set.iter().any(|(id, _)| id == provider),
        None => true,
    }
}

/// The provider whose credential is in play: the session's current model
/// head ("deepseek/deepseek-v4-pro" → "deepseek") when bound. Without a
/// bound session the credential is unknown and callers pass `None` — the
/// notice stays generic rather than name something the agent never did
/// (issue #160).
fn auth_provider(
    surface: &Arc<Mutex<Surface>>,
    session_id: Option<&str>,
) -> Option<String> {
    session_id
        .and_then(|id| session_provider(surface, id))
        .map(|value| provider_head(&value).to_string())
}

fn emit_needs_auth_open(
    bus: &Sender<AppEvent>,
    methods: Vec<AuthMethodInfo>,
    provider: Option<&str>,
    message: Option<String>,
) {
    // Name the credential that failed — never the last method picked nor a
    // bare "agent" fallback (issue #160).
    let method = provider
        .and_then(|p| method_for_provider(&methods, p))
        .or_else(|| sole_method(&methods))
        .cloned();
    emit_auth(bus, needs_auth_snapshot(methods, method.as_ref(), message));
    let _ = bus.send(AppEvent::Ctl(CtlEvent::OpenAuth));
}

/// A prompt queued behind an auth stall must say so: the owning session's
/// tab gets the sign-in surface with the reason instead of waiting on a
/// turn that never starts (issue #160).
fn emit_held_prompt(
    bus: &Sender<AppEvent>,
    session_id: &str,
    methods: &[AuthMethodInfo],
    provider: Option<&str>,
) {
    let method = provider
        .and_then(|p| method_for_provider(methods, p))
        .or_else(|| sole_method(methods));
    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionAuth {
        session_id: session_id.to_string(),
        snapshot: needs_auth_snapshot(
            methods.to_vec(),
            method,
            Some("input is held until you sign in — pick a method in the auth picker".into()),
        ),
        open: true,
    }));
}

/// Queue a prompt-like command for the first session that binds. When the
/// queue is growing *because* sign-in is still pending, the hold must be
/// visible — a typed `/login` or prompt is never silently swallowed: the
/// auth UI comes back up so the user sees what is holding the input (#160).
fn queue_connection_prompt(
    pending: &mut VecDeque<Cmd>,
    cmd: Cmd,
    session_auth_pending: bool,
    bus: &Sender<AppEvent>,
    methods: &[AuthMethodInfo],
    provider: Option<&str>,
) {
    if session_auth_pending {
        emit_needs_auth_open(bus, methods.to_vec(), provider, None);
    }
    pending.push_back(cmd);
}

fn parked_prompt(cmd: &Cmd) -> Option<ParkedPromptKind> {
    match cmd {
        Cmd::Prompt { text, .. } | Cmd::Steer { text, .. } => {
            Some(ParkedPromptKind::Text(text.clone()))
        }
        Cmd::PromptImages { blocks, .. } | Cmd::SteerImages { blocks, .. } => {
            Some(ParkedPromptKind::Images(blocks.clone()))
        }
        _ => None,
    }
}

async fn create_prompt_session(
    cx: &ConnectionTo<Agent>,
    cwd: &std::path::Path,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
    methods: &[AuthMethodInfo],
    selected: Option<&AuthMethodInfo>,
    provider: Option<&str>,
    additional_directories: bool,
    directories: &[String],
) -> std::result::Result<Option<SessionId>, AcpError> {
    match cx
        .send_request(new_session_request(
            cwd,
            additional_directories,
            directories,
        ))
        .block_task_setup_deadline()
        .await
    {
        Ok(created) => {
            let sid = created.session_id.clone();
            apply_created(&created, surface, bus, None);
            if !methods.is_empty() {
                emit_auth(bus, configured_snapshot(methods.to_vec(), selected));
            }
            Ok(Some(sid))
        }
        Err(err) if is_auth_required_error(&err) => {
            emit_needs_auth_open(
                bus,
                methods.to_vec(),
                provider,
                Some(acp_error_message(&err)),
            );
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

fn session_connection_snapshot(value: &Value) -> crate::bus::SessionConnection {
    let mut argv = value.get("command").and_then(Value::as_str)
        .map(|command| vec![command.to_string()]).unwrap_or_default();
    argv.extend(value.get("args").and_then(Value::as_array).into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_string));
    let methods = parse_auth_methods(value.get("authMethods").unwrap_or(&Value::Null),
        &argv, value.get("cwd").and_then(Value::as_str).unwrap_or("."), &process_env());
    let auth = if methods.is_empty() { AuthSnapshot::none() } else { configured_snapshot(methods, None) };
    let load_session = load_session_supported(value);
    crate::bus::SessionConnection {
        server: value.pointer("/agentInfo/name").and_then(Value::as_str).map(str::to_string),
        auth, load_session, list_session: list_session_supported(value) || load_session,
        resume_session: resume_session_supported(value),
        fork_session: fork_session_supported(value),
        close_session: close_session_supported(value),
        additional_directories: additional_directories_supported(value),
    }
}

fn apply_setup(
    value: &Value,
    session_hint: Option<&str>,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
) {
    if let Ok(mut surface) = surface.lock() {
        let session = value
            .get("sessionId")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .or(session_hint);
        surface.session_mut(session).modes.clear();
        if let (Some(session_id), Some(connection)) = (session, value.pointer("/_meta/marttyConnection")) {
            surface.session_mut(Some(session_id)).connection = Some(connection.clone());
            for method in session_connection_snapshot(connection).auth.methods {
                surface.auth_methods.insert(method.id.clone(), method);
            }
            let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionConnection {
                session_id: session_id.to_string(), connection: session_connection_snapshot(connection),
            }));
        } else if let (Some(session_id), Some(connection)) = (session, &surface.initial_connection) {
            let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionConnection {
                session_id: session_id.to_string(), connection: connection.clone(),
            }));
        }
        if let Some(options) = value
            .get("configOptions")
            .or_else(|| value.get("config_options"))
        {
            surface.apply_config_options(options, bus, session);
            if let Some(s) = session {
                for event in config_option_events(s.to_string(), options) {
                    let _ = bus.send(AppEvent::Ui(event));
                }
            }
        }
        if let Some(modes) = value.get("modes") {
            surface.apply_session_modes(modes, bus, session);
        }
    }
}

fn apply_created(
    created: &agent_client_protocol::schema::v1::NewSessionResponse,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
    notice: Option<String>,
) {
    // Bind the authoritative id before decomposing the setup snapshot into
    // session-scoped UI facts. Otherwise App correctly rejects those facts as
    // belonging to a session that is not current yet.
    emit_session_bound(bus, &created.session_id, notice);
    if let Ok(value) = serde_json::to_value(created) {
        let session = created.session_id.to_string();
        apply_setup(&value, Some(&session), surface, bus);
    }
}

fn apply_config_response(
    value: &Value,
    session: &SessionId,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
) -> Option<Value> {
    let options = value
        .get("configOptions")
        .or_else(|| value.get("config_options"))?
        .clone();
    if let Ok(mut surface) = surface.lock() {
        surface.apply_config_options(&options, bus, Some(&session.0));
    }
    for event in config_option_events(session.to_string(), &options) {
        let _ = bus.send(AppEvent::Ui(event));
    }
    Some(options)
}

fn emit_session_bound(bus: &Sender<AppEvent>, session_id: &SessionId, notice: Option<String>) {
    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionBound {
        session_id: session_id.to_string(),
        notice,
    }));
}

fn load_session_supported(init: &Value) -> bool {
    init.get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| caps.get("loadSession").or_else(|| caps.get("load_session")))
        .and_then(Value::as_bool)
        == Some(true)
}

fn resume_session_supported(init: &Value) -> bool {
    let resume = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| {
            caps.get("sessionCapabilities")
                .or_else(|| caps.get("session_capabilities"))
        })
        .and_then(|caps| caps.get("resume"));
    matches!(resume, Some(Value::Object(_)) | Some(Value::Bool(true)))
}

fn list_session_supported(init: &Value) -> bool {
    let list = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| {
            caps.get("sessionCapabilities")
                .or_else(|| caps.get("session_capabilities"))
        })
        .and_then(|caps| caps.get("list"));
    matches!(list, Some(Value::Object(_)) | Some(Value::Bool(true)))
}

/// `{}` or `true` advertises `session/close` (issue #146): the agent
/// cancels the session's ongoing work and frees its resources. Omitted,
/// null, `false`, and other shapes do not.
fn close_session_supported(init: &Value) -> bool {
    let close = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| {
            caps.get("sessionCapabilities")
                .or_else(|| caps.get("session_capabilities"))
        })
        .and_then(|caps| caps.get("close"));
    matches!(close, Some(Value::Object(_)) | Some(Value::Bool(true)))
}

/// `{}` advertises `session/fork`. Omitted, null, booleans, and other
/// non-objects do not. Harness name and version are not consulted.
fn fork_session_supported(init: &Value) -> bool {
    let fork = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| {
            caps.get("sessionCapabilities")
                .or_else(|| caps.get("session_capabilities"))
        })
        .and_then(|caps| caps.get("fork"));
    matches!(fork, Some(Value::Object(_)))
}

/// Params aligned with [`load_session_request`]: `sessionId`, `cwd`, and
/// `mcpServers`. The SDK omits an empty `mcpServers` on `session/fork`;
/// `session/load` always sends the array, so the empty list is put back.
/// No message id and no `_meta`.
fn fork_session_params(
    session_id: SessionId,
    cwd: &std::path::Path,
    additional_directories: bool,
    directories: &[String],
) -> Value {
    let mut request = agent_client_protocol::schema::v1::ForkSessionRequest::new(
        session_id,
        cwd.to_path_buf(),
    );
    let extra = directory_paths(additional_directories, directories);
    if !extra.is_empty() {
        request = request.additional_directories(extra);
    }
    let mut params = serde_json::to_value(&request).unwrap_or(Value::Null);
    if let Some(object) = params.as_object_mut() {
        object.remove("_meta");
        object.remove("messageId");
        object.remove("message_id");
        if !object.contains_key("mcpServers") {
            object.insert("mcpServers".to_string(), Value::Array(Vec::new()));
        }
    }
    params
}

/// `{}` advertises the standard capability. Omitted, null, and non-objects do not.
fn additional_directories_supported(init: &Value) -> bool {
    let advertised = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"))
        .and_then(|caps| {
            caps.get("sessionCapabilities")
                .or_else(|| caps.get("session_capabilities"))
        })
        .and_then(|caps| {
            caps.get("additionalDirectories")
                .or_else(|| caps.get("additional_directories"))
        });
    matches!(advertised, Some(Value::Object(_)))
}

/// Paths to send, or empty when the field must be omitted.
///
/// An empty list is not serialized: omission means no additional roots.
/// `[]` is not sent to "clear" a previous list.
fn directory_paths(enabled: bool, directories: &[String]) -> Vec<std::path::PathBuf> {
    if !enabled {
        return Vec::new();
    }
    directories.iter().map(std::path::PathBuf::from).collect()
}

fn new_session_request(
    cwd: &std::path::Path,
    additional_directories: bool,
    directories: &[String],
) -> NewSessionRequest {
    let mut request = NewSessionRequest::new(cwd.to_path_buf());
    let extra = directory_paths(additional_directories, directories);
    if !extra.is_empty() {
        request = request.additional_directories(extra);
    }
    request
}

fn load_session_request(
    session_id: SessionId,
    cwd: &std::path::Path,
    additional_directories: bool,
    directories: &[String],
) -> LoadSessionRequest {
    let mut request = LoadSessionRequest::new(session_id, cwd.to_path_buf());
    let extra = directory_paths(additional_directories, directories);
    if !extra.is_empty() {
        request = request.additional_directories(extra);
    }
    request
}

fn resume_session_request(
    session_id: SessionId,
    cwd: &std::path::Path,
    additional_directories: bool,
    directories: &[String],
) -> ResumeSessionRequest {
    let mut request = ResumeSessionRequest::new(session_id, cwd.to_path_buf());
    let extra = directory_paths(additional_directories, directories);
    if !extra.is_empty() {
        request = request.additional_directories(extra);
    }
    request
}

fn prompt_image_supported(init: &Value) -> bool {
    let caps = init
        .get("agentCapabilities")
        .or_else(|| init.get("agent_capabilities"));
    let prompt = caps.and_then(|caps| {
        caps.get("promptCapabilities")
            .or_else(|| caps.get("prompt_capabilities"))
            .or_else(|| caps.get("prompt"))
    });
    match prompt.and_then(|caps| caps.get("image")) {
        Some(Value::Bool(true)) => true,
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

fn is_clipboard_path(path: &str) -> bool {
    path.is_empty() || path == "clipboard"
}

fn normalize_abs(path: std::path::PathBuf) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            rest => out.push(rest),
        }
    }
    out
}

/// Filesystem path for a staged raster. `clipboard` / empty have no path.
fn abs_fs_path(path: &str, workspace: &str) -> Option<std::path::PathBuf> {
    if is_clipboard_path(path) {
        return None;
    }
    let p = std::path::Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(workspace).join(p)
    };
    let abs = if joined.is_absolute() {
        joined
    } else {
        std::env::current_dir().ok()?.join(joined)
    };
    Some(normalize_abs(abs))
}

/// Unix `file://` + absolute path with percent-encoding (`file:///tmp/a%20b.png`).
/// `#`, `?`, `%`, spaces and non-ASCII bytes are encoded so agents parsing
/// the URI (RFC 8089) recover the original path instead of treating `#` as
/// a fragment or `?` as a query.
fn unix_file_uri(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy();
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'!' | b'$'
            | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b'@' | b':' => {
                encoded.push(*byte as char)
            }
            b'/' => encoded.push('/'),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    format!("file://{encoded}")
}

fn resolve_image_uri(path: &str, workspace: &str) -> Option<String> {
    Some(unix_file_uri(&abs_fs_path(path, workspace)?))
}

fn spill_image(image: &crate::bus::ImagePart, index: usize) -> Result<String, String> {
    let bytes = crate::pet::decode_base64(&image.data)
        .ok_or_else(|| format!("invalid base64 for {}", image.name))?;
    let dir = std::env::temp_dir().join("dsh-tui-clip");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = image
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = dir.join(format!("{}-{}-{}", std::process::id(), index, safe));
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    Ok(file.to_string_lossy().into_owned())
}

fn prompt_content_blocks(
    blocks: Vec<crate::bus::PromptBlock>,
    prompt_image: bool,
    workspace: &str,
) -> Result<Vec<ContentBlock>, String> {
    let mut out = Vec::new();
    for (i, block) in blocks.into_iter().enumerate() {
        match block {
            crate::bus::PromptBlock::Text(text) => {
                if !text.is_empty() {
                    out.push(text.into());
                }
            }
            crate::bus::PromptBlock::Image(image) => {
                if prompt_image {
                    let mut content =
                        ImageContent::new(image.data.clone(), image.media_type.clone());
                    if let Some(uri) = resolve_image_uri(&image.path, workspace) {
                        content = content.uri(uri);
                    }
                    out.push(ContentBlock::Image(content));
                } else {
                    let abs = match abs_fs_path(&image.path, workspace) {
                        Some(p) => p,
                        None => std::path::PathBuf::from(spill_image(&image, i)?),
                    };
                    out.push(ContentBlock::ResourceLink(
                        ResourceLink::new(image.name.clone(), unix_file_uri(&abs))
                            .mime_type(image.media_type.clone()),
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// Node extra stdio (`pipe`) is a socketpair, not a FIFO. `tokio::fs::File`
/// on that fd treats `EAGAIN` as a hard error (os error 35).
#[cfg(unix)]
fn unix_stream_from_file(file: std::fs::File) -> Result<tokio::net::UnixStream> {
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream as StdUnixStream;
    let std = unsafe { StdUnixStream::from_raw_fd(file.into_raw_fd()) };
    std.set_nonblocking(true)
        .context("attach fd set_nonblocking")?;
    tokio::net::UnixStream::from_std(std).context("attach fd as unix stream")
}

/// Prompt-like work that must survive a respawn of the agent transport:
/// prompts queued before the first session bound (`pending`) and prompts
/// parked on an auth stall (`parked`) carry across the reconnect so a
/// terminal sign-in does not silently drop user input (#160).
struct ConnectCarry {
    fwd_rx: tokio::sync::mpsc::UnboundedReceiver<Cmd>,
    pending: VecDeque<Cmd>,
    parked: VecDeque<ParkedPrompt>,
    session_auth_pending: bool,
    /// The terminal-auth method whose launch triggered this reconnect: the
    /// post-`initialize` snapshot stays optimistic and, once a session
    /// binds, credits this method instead of advertising "sign-in needed"
    /// for the first advertised one (issue #160).
    terminal_method: Option<String>,
}

impl ConnectCarry {
    fn new(fwd_rx: tokio::sync::mpsc::UnboundedReceiver<Cmd>) -> Self {
        Self {
            fwd_rx,
            pending: VecDeque::new(),
            parked: VecDeque::new(),
            session_auth_pending: false,
            terminal_method: None,
        }
    }
}

enum ConnectOutcome {
    /// The command channel closed or `Cmd::Shutdown` arrived — the run ends.
    Closed,
    /// Terminal sign-in succeeded on the TTY: the transport was dropped and
    /// the agent must be respawned and re-`initialize`d.
    Reconnect(ConnectCarry),
}

/// Bridge the blocking std-mpsc command channel onto an unbounded tokio
/// receiver so `connect` can `select!` it. The forwarder thread ends when
/// the channel closes or the receiver goes away.
fn spawn_cmd_bridge(cmd_rx: Receiver<Cmd>) -> tokio::sync::mpsc::UnboundedReceiver<Cmd> {
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    // A spawn failure is not fatal and never panics: the unconnected
    // receiver ends every select! loop on it, so the run closes like a
    // dropped channel (issue #160).
    let _ = std::thread::Builder::new()
        .name("dsh-acp-cmds".into())
        .spawn(move || {
            // The closure owns the sender: the channel lives as long as the
            // bridge does, and ends when `cmd_rx` closes or the last
            // receiver goes away.
            while let Ok(cmd) = cmd_rx.recv() {
                if fwd_tx.send(cmd).is_err() {
                    break;
                }
            }
        });
    fwd_rx
}

/// A single `connect` pass for callers that own no respawn path — unit
/// tests and the attach transports: the command channel is bridged in
/// here, `can_reconnect` is off (a host-owned stream cannot be respawned),
/// and the outcome collapses to `()`. A `Cmd::AgentReconnect` there retries
/// `session/new` in place (issue #160).
#[cfg(test)]
async fn connect_once<T>(
    transport: T,
    cfg: RuntimeConfig,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
) -> std::result::Result<(), AcpError>
where
    T: ConnectTo<Client> + 'static,
{
    let fwd_rx = spawn_cmd_bridge(cmd_rx);
    connect(transport, cfg, bus, ConnectCarry::new(fwd_rx), false)
        .await
        .map(|_| ())
}

async fn run(
    cfg: RuntimeConfig,
    endpoint: AcpEndpoint,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
) -> Result<()> {
    // UI → ACP forwarding lives one level above `connect` so the channel
    // survives agent respawns (terminal sign-in drops the transport).
    let fwd_rx = spawn_cmd_bridge(cmd_rx);
    match endpoint {
        AcpEndpoint::Spawn(args) => {
            spawn_agent_loop(
                move || AcpAgent::from_args(args.clone()),
                cfg,
                bus,
                ConnectCarry::new(fwd_rx),
            )
            .await
        }
        #[cfg(unix)]
        AcpEndpoint::AttachStdio { incoming, outgoing } => {
            let incoming = unix_stream_from_file(incoming)?.compat();
            let outgoing = unix_stream_from_file(outgoing)?.compat_write();
            connect(
                ByteStreams::new(outgoing, incoming),
                cfg,
                bus,
                ConnectCarry::new(fwd_rx),
                false,
            )
            .await
            .map_err(acp_err)
            .map(|_| ())
        }
        #[cfg(not(unix))]
        AcpEndpoint::AttachStdio { .. } => {
            Err(anyhow::anyhow!("stdio fd attach is only supported on Unix"))
        }
        AcpEndpoint::AttachTcp(stream) => {
            stream
                .set_nonblocking(true)
                .context("tcp set_nonblocking")?;
            let stream = tokio::net::TcpStream::from_std(stream).context("tokio tcp from_std")?;
            let (read, write) = stream.into_split();
            connect(
                ByteStreams::new(write.compat_write(), read.compat()),
                cfg,
                bus,
                ConnectCarry::new(fwd_rx),
                false,
            )
            .await
            .map_err(acp_err)
            .map(|_| ())
        }
    }
}

/// The spawn endpoint's connect → reconnect loop: each iteration drops
/// the previous transport (killing the spawned agent) and runs a fresh
/// `initialize`. Queued and parked prompts survive inside `carry` across
/// respawns (issue #160 — terminal sign-in is exactly one `Reconnect`).
async fn spawn_agent_loop<T, F>(
    mut make_agent: F,
    cfg: RuntimeConfig,
    bus: Sender<AppEvent>,
    mut carry: ConnectCarry,
) -> Result<()>
where
    T: ConnectTo<Client> + 'static,
    F: FnMut() -> std::result::Result<T, AcpError>,
{
    loop {
        let agent = make_agent().map_err(acp_err)?;
        match connect(agent, cfg.clone(), bus.clone(), carry, true)
            .await
            .map_err(acp_err)?
        {
            ConnectOutcome::Closed => return Ok(()),
            ConnectOutcome::Reconnect(next) => carry = next,
        }
    }
}

/// The package a dynamic plugin row resolves to: `currentPackageId`, then
/// `activeRun.packageId`, then the last advertised package.
fn plugin_package_id<'a>(
    row: &'a Value,
    packages: &'a [Value],
    index: usize,
) -> std::result::Result<&'a str, String> {
    row.get("currentPackageId")
        .and_then(Value::as_str)
        .or_else(|| {
            row.get("activeRun")
                .and_then(|run| run.get("packageId"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            packages
                .last()
                .and_then(|package| package.get("packageId"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| format!("plugins[{index}] has no package"))
}

fn dynamic_plugins_from_value(value: &Value) -> std::result::Result<Vec<CordisPluginItem>, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "Cordis plugins/list returned a non-array result".to_string())?;
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let plugin_id = row
                .get("pluginId")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("plugins[{index}].pluginId is missing"))?;
            let packages = row
                .get("packages")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("plugins[{index}].packages is missing"))?;
            let package_id = plugin_package_id(row, packages, index)?;
            let package = packages.iter().find(|package| {
                package.get("packageId").and_then(Value::as_str) == Some(package_id)
            });
            let name = package
                .and_then(|package| package.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(plugin_id);
            let latest = row.get("latestRun").filter(|value| value.is_object());
            let status = latest
                .and_then(|run| run.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    if row.get("activeRun").is_some_and(Value::is_object) {
                        "running"
                    } else {
                        "stopped"
                    }
                });
            let approval_request_id = latest
                .filter(|run| run.get("requiresApproval").and_then(Value::as_bool) == Some(true))
                .and_then(|run| run.get("approvalRequestId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok(CordisPluginItem {
                id: plugin_id.to_string(),
                name: name.to_string(),
                package_id: package_id.to_string(),
                status: status.to_string(),
                approval_request_id,
            })
        })
        .collect()
}

fn static_plugins_from_value(value: &Value) -> std::result::Result<Vec<StaticPluginItem>, String> {
    let rows = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "plugins/list returned no entries array".to_string())?;
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let text = |field: &str| {
                row.get(field)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| format!("entries[{index}].{field} is missing"))
            };
            Ok(StaticPluginItem {
                entry_id: text("entryId")?,
                module_name: text("moduleName")?,
                enabled: row
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!("entries[{index}].enabled is missing"))?,
                fiber_phase: row
                    .get("fiberPhase")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Deadline for one ACP request round-trip. Generous on purpose — it only
/// exists so a hung agent (half-open stdio, dead process) cannot freeze
/// the command loop: Interrupt and Shutdown are unreachable while an arm
/// awaits forever.
const REQUEST_DEADLINE: Duration = Duration::from_secs(120);
const SETUP_DEADLINE: Duration = Duration::from_secs(20 * 60);

/// [`SentRequest::block_task`] with [`REQUEST_DEADLINE`] applied. Every
/// request the command loop awaits directly goes through this.
trait BlockTaskDeadline {
    type Output;
    fn block_task_deadline(self) -> impl Future<Output = Result<Self::Output, AcpError>>;
    fn block_task_setup_deadline(self) -> impl Future<Output = Result<Self::Output, AcpError>>;
}

impl<T> BlockTaskDeadline for SentRequest<T> {
    type Output = T;
    fn block_task_deadline(self) -> impl Future<Output = Result<T, AcpError>> {
        request_with_deadline(self, REQUEST_DEADLINE)
    }
    fn block_task_setup_deadline(self) -> impl Future<Output = Result<T, AcpError>> {
        request_with_deadline(self, SETUP_DEADLINE)
    }
}

async fn request_with_deadline<T>(request: SentRequest<T>, deadline: Duration) -> Result<T, AcpError> {
    match tokio::time::timeout(deadline, request.block_task()).await {
        Ok(result) => result,
        Err(_) => Err(AcpError::new(-32001, "agent request timed out")),
    }
}

/// Whether an `initialize` failure is a reply the agent itself authored —
/// meaning it is attached and may still authenticate — rather than a locally
/// synthesized failure: transport EOF on a direct spawn, the harness pool's
/// `agentExited` marker for a dead child, or our own request deadline (a
/// bare `-32001` with no agent-supplied data).
fn init_was_answered(err: &AcpError) -> bool {
    if is_incoming_transport_closed(err) {
        return false;
    }
    if err.data.is_none() && i32::from(err.code) == -32001 {
        return false;
    }
    if let Some(data) = err.data.as_ref() {
        if data.get("agentExited").and_then(Value::as_bool) == Some(true) {
            return false;
        }
    }
    true
}

/// The message of a refusal the agent itself answered. Our transport wraps
/// agent error replies with an "Agent stderr:" diagnostics tail, but a
/// refused handshake's own words are the diagnosis — the tail is unrelated
/// launch noise, so drop it before surfacing the refusal.
fn init_refusal_message(err: &AcpError) -> String {
    let message = acp_error_message(err);
    match message.split_once("\nAgent stderr:") {
        Some((head, _)) => head.trim_end().to_string(),
        None => message,
    }
}

async fn call_tui_extension(
    cx: &ConnectionTo<Agent>,
    method: &str,
    params: Value,
) -> std::result::Result<Value, AcpError> {
    let request = UntypedMessage::new(method, params)?;
    if matches!(method, crate::cordis::COMMAND_INVOKE | crate::cordis::OVERLAY_EVENT) {
        cx.send_request(request).block_task_setup_deadline().await
    } else {
        cx.send_request(request).block_task_deadline().await
    }
}

struct PluginOperation {
    method: &'static str,
    params: Value,
    context: &'static str,
}

struct PluginOperationFinish {
    context: &'static str,
    result: std::result::Result<Value, AcpError>,
}

fn spawn_plugin_operation(
    tasks: &mut tokio::task::JoinSet<PluginOperationFinish>,
    cx: ConnectionTo<Agent>,
    operation: PluginOperation,
) -> tokio::task::Id {
    tasks.spawn(async move {
        PluginOperationFinish {
            context: operation.context,
            result: call_tui_extension(&cx, operation.method, operation.params).await,
        }
    }).id()
}

async fn fetch_dynamic_plugins(
    cx: &ConnectionTo<Agent>,
    agent_id: &str,
) -> std::result::Result<Vec<CordisPluginItem>, String> {
    let value = call_tui_extension(
        cx,
        crate::cordis::PLUGINS_LIST,
        serde_json::json!({ "agentId": agent_id }),
    )
    .await
    .map_err(|error| error.to_string())?;
    dynamic_plugins_from_value(&value)
}

async fn fetch_static_plugins(
    cx: &ConnectionTo<Agent>,
) -> std::result::Result<Vec<StaticPluginItem>, String> {
    let value = call_tui_extension(
        cx,
        crate::cordis::STATIC_PLUGINS_LIST,
        serde_json::json!({}),
    )
    .await
    .map_err(|error| error.to_string())?;
    static_plugins_from_value(&value)
}

fn ensure_agent_cordis(surface: &Arc<Mutex<Surface>>, bus: &Sender<AppEvent>) -> bool {
    let advertised = {
        let surface = surface.lock().unwrap_or_else(|error| error.into_inner());
        surface.active_connection().map(crate::cordis::advertised_by_agent).unwrap_or(surface.cordis)
    };
    if !advertised {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
            "agent does not advertise _dsh/cordis".into(),
        )));
    }
    advertised
}

fn ensure_client_compositor(surface: &Arc<Mutex<Surface>>, bus: &Sender<AppEvent>) -> bool {
    let advertised = surface
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .client_compositor;
    if !advertised {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
            "client compositor is unavailable".into(),
        )));
    }
    advertised
}

/// The local compositor owns client projections independently of Agent Cordis.
/// Without either capability, never send extension traffic or spam notices.
fn client_projection_available(surface: &Arc<Mutex<Surface>>) -> bool {
    let surface = surface.lock().unwrap_or_else(|error| error.into_inner());
    surface.client_compositor || surface.cordis
}

/// Whether a finished JoinSet entry is the operation the loop is tracking.
fn plugin_op_is_primary<T>(
    finished: &Option<std::result::Result<(tokio::task::Id, T), tokio::task::JoinError>>,
    active: Option<tokio::task::Id>,
) -> bool {
    match finished {
        Some(Ok((id, _))) => active == Some(*id),
        Some(Err(error)) => active == Some(error.id()),
        None => false,
    }
}

/// Map a settled plugin operation task to the bus event it warrants (issue
/// #160): a successful `new-session` action asks the UI to open a tab, a
/// failed result reports `{context} failed`, and a task-level join failure
/// reports the panic/abort. Cancelled and non-primary finishes are silent.
fn plugin_op_finish_event(
    finished: Option<std::result::Result<(tokio::task::Id, PluginOperationFinish), tokio::task::JoinError>>,
    primary: bool,
    cancelled: bool,
) -> Option<CtlEvent> {
    match finished {
        Some(Ok((_, PluginOperationFinish { result: Ok(value), .. }))) if primary => {
            if !cancelled && value.get("action").and_then(Value::as_str) == Some("new-session") {
                Some(CtlEvent::NewSessionRequested)
            } else {
                None
            }
        }
        Some(Ok((_, PluginOperationFinish { context, result: Err(error) }))) if !cancelled => {
            Some(CtlEvent::TuiOpFailed(format!("{context} failed: {error}")))
        }
        Some(Err(error)) if !cancelled => Some(CtlEvent::TuiOpFailed(format!(
            "plugin operation task failed: {error}"
        ))),
        _ => None,
    }
}

async fn connect<T>(
    transport: T,
    cfg: RuntimeConfig,
    bus: Sender<AppEvent>,
    carry: ConnectCarry,
    can_reconnect: bool,
) -> std::result::Result<ConnectOutcome, AcpError>
where
    T: ConnectTo<Client> + 'static,
{
    let surface = Arc::new(Mutex::new(Surface::default()));
    let bus_n = bus.clone();
    let bus_p = bus.clone();
    let bus_e = bus.clone();
    let bus_u = bus.clone();
    let bus_write = bus.clone();
    let bus_config = bus.clone();
    let surface_n = Arc::clone(&surface);
    let surface_config = Arc::clone(&surface);
    let surface_u = Arc::clone(&surface);
    let workspace = cfg.workspace.clone();
    let workspace_read = workspace.clone();
    let workspace_write = workspace.clone();
    let terms = Arc::new(crate::acp_term::TerminalBroker::new(workspace.clone()));
    let terms_create = Arc::clone(&terms);
    let bus_create = bus.clone();
    let workspace_create = workspace.clone();
    let terms_output = Arc::clone(&terms);
    let terms_wait = Arc::clone(&terms);
    let terms_kill = Arc::clone(&terms);
    let terms_release = Arc::clone(&terms);

    Client
        .builder()
        .name("martty")
        .on_receive_notification(
            {
                let bus_n = bus_n.clone();
                let surface_n = Arc::clone(&surface_n);
                async move |note: SessionNotification, _cx| {
                    if let Ok(params) = serde_json::to_value(&note) {
                        let _ = bus_n.send(AppEvent::Rpc {
                            method: "session/update".into(),
                            params: params.clone(),
                        });
                        let update = params.get("update").cloned().unwrap_or(Value::Null);
                        if let Some(options) = update
                            .get("configOptions")
                            .or_else(|| update.get("config_options"))
                        {
                            let session = params.get("sessionId").and_then(Value::as_str);
                            if let Ok(mut surface) = surface_n.lock() {
                                surface.apply_config_options(options, &bus_n, session);
                            }
                        }
                        if let Some(commands) = update
                            .get("availableCommands")
                            .or_else(|| update.get("available_commands"))
                        {
                            let skills = skills_from_available_commands(commands);
                            let session = params
                                .get("sessionId")
                                .or_else(|| params.get("session_id"))
                                .and_then(Value::as_str);
                            if let Ok(mut surface) = surface_n.lock() {
                                surface.session_mut(session).skills = skills.clone();
                            }
                            let _ = bus_n.send(AppEvent::Ctl(CtlEvent::Skills {
                                session_id: session.map(str::to_string),
                                skills,
                            }));
                        }
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .on_receive_notification(
            async move |msg: UntypedMessage, cx| {
                // `_auth/status_update` reports the agent's credentialed
                // providers — the credential truth the held-prompt gate and
                // the post-logout notice read (issue #160).
                if msg.method() == "_auth/status_update" {
                    let providers = msg
                        .params()
                        .pointer("/authStatus/providers")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(|entry| {
                                    let id = entry
                                        .get("providerId")
                                        .or_else(|| entry.get("provider_id"))
                                        .and_then(Value::as_str)?;
                                    let name = entry
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or(id);
                                    Some((id.to_string(), name.to_string()))
                                })
                                .collect()
                        });
                    if let Some(providers) = providers {
                        surface_u
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .authenticated_providers = Some(providers);
                    }
                    return Ok(Handled::Yes);
                }
                if matches!(
                    msg.method(),
                    crate::cordis::THEME_UPDATE
                        | crate::cordis::THEME_REMOVE
                        | crate::cordis::SLOTS_UPDATE
                        | crate::cordis::COMMANDS_UPDATE
                        | crate::cordis::OVERLAY_UPDATE
                        | crate::cordis::APPROVALS_UPDATE
                        | crate::cordis::UI_UPDATE
                ) {
                    if let Ok(mut surface) = surface_u.lock() {
                        surface.client_compositor = true;
                    }
                    let _ = bus_u.send(AppEvent::Rpc {
                        method: msg.method().into(),
                        params: msg.params().clone(),
                    });
                    return Ok(Handled::Yes);
                }
                Ok(Handled::No {
                    message: (msg, cx),
                    retry: false,
                })
            },
            on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                let title = req
                    .tool_call
                    .fields
                    .title
                    .clone()
                    .unwrap_or_else(|| "tool".into());
                let _ = bus_p.send(AppEvent::Rpc {
                    method: "session.event".into(),
                    params: json!({
                        "sessionId": req.session_id.to_string(),
                        "event": {
                            "type": "approval/asked",
                            "data": { "toolName": title }
                        }
                    }),
                });
                let options: Vec<PermissionAskOption> = req
                    .options
                    .iter()
                    .map(|opt| PermissionAskOption {
                        option_id: opt.option_id.to_string(),
                        kind: match opt.kind {
                            PermissionOptionKind::AllowOnce => "allow_once",
                            PermissionOptionKind::AllowAlways => "allow_always",
                            PermissionOptionKind::RejectOnce => "reject_once",
                            PermissionOptionKind::RejectAlways => "reject_always",
                            _ => "other",
                        }
                        .into(),
                        name: opt.name.clone(),
                    })
                    .collect();
                if permission_ask_empty_outcome(&options).is_some() {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                if bus_p
                    .send(AppEvent::PermissionAsk {
                        session_id: req.session_id.to_string(),
                        title,
                        options,
                        reply: tx,
                    })
                    .is_err()
                {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                // Wait off the ACP dispatch loop so session/update still paints.
                tokio::spawn(async move {
                    let reply = rx.await.unwrap_or(PermissionAskReply::Cancelled);
                    let outcome = match reply {
                        PermissionAskReply::Selected(id) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(id),
                        ),
                        PermissionAskReply::Cancelled => RequestPermissionOutcome::Cancelled,
                    };
                    let _ = responder.respond(RequestPermissionResponse::new(outcome));
                });
                Ok(())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: CreateElicitationRequest, responder, _cx| {
                let session_id = match req.scope() {
                    ElicitationScope::Session(scope) => Some(scope.session_id.to_string()),
                    // Auth/config-phase elicitation (or a future scope kind)
                    // has no session to attach to; the client shows it on
                    // the live view.
                    _ => None,
                };
                let form = match crate::elicitation::form_from_request(&req) {
                    Ok(form) => form,
                    Err(err) => {
                        return responder.respond_with_error(AcpError::new(-32602, err));
                    }
                };
                let (tx, rx) = tokio::sync::oneshot::channel();
                if bus_e
                    .send(AppEvent::ElicitationAsk {
                        session_id,
                        form,
                        reply: tx,
                    })
                    .is_err()
                {
                    return responder.respond(CreateElicitationResponse::new(
                        ElicitationAction::Cancel,
                    ));
                }
                // Keep dispatching ACP traffic while the user completes the form.
                tokio::spawn(async move {
                    let action = match rx.await {
                        Ok(crate::elicitation::ElicitationReply::Accepted(values)) => {
                            let content: std::collections::BTreeMap<
                                String,
                                ElicitationContentValue,
                            > = values
                                .into_iter()
                                .map(|(name, value)| {
                                    let value = match value {
                                        crate::elicitation::ElicitationValue::String(value) => {
                                            ElicitationContentValue::String(value)
                                        }
                                        crate::elicitation::ElicitationValue::Integer(value) => {
                                            ElicitationContentValue::Integer(value)
                                        }
                                        crate::elicitation::ElicitationValue::Number(value) => {
                                            ElicitationContentValue::Number(value)
                                        }
                                        crate::elicitation::ElicitationValue::Boolean(value) => {
                                            ElicitationContentValue::Boolean(value)
                                        }
                                        crate::elicitation::ElicitationValue::StringArray(value) => {
                                            ElicitationContentValue::StringArray(value)
                                        }
                                    };
                                    (name, value)
                                })
                                .collect();
                            ElicitationAction::Accept(
                                ElicitationAcceptAction::new().content(content),
                            )
                        }
                        Ok(crate::elicitation::ElicitationReply::Cancelled) | Err(_) => {
                            ElicitationAction::Cancel
                        }
                    };
                    let _ = responder.respond(CreateElicitationResponse::new(action));
                });
                Ok(())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let workspace = workspace_read;
                async move |req: ReadTextFileRequest, responder, _cx| {
                    let path = crate::acp_fs::resolve_with_cwd(
                        &req.path,
                        std::path::Path::new(&workspace),
                    );
                    let line = req.line;
                    let limit = req.limit;
                    tokio::spawn(async move {
                        match crate::acp_fs::read_text_file(&path, line, limit) {
                            Ok(content) => {
                                let _ = responder.respond(ReadTextFileResponse::new(content));
                            }
                            Err(err) => {
                                let _ = responder.respond_with_error(err);
                            }
                        }
                    });
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let bus = bus_write;
                let workspace = workspace_write;
                async move |req: WriteTextFileRequest, responder, _cx| {
                    let session_id = req.session_id.to_string();
                    let path = crate::acp_fs::resolve_with_cwd(
                        &req.path,
                        std::path::Path::new(&workspace),
                    );
                    let content = req.content;
                    let bus = bus.clone();
                    let workspace = workspace.clone();
                    tokio::spawn(async move {
                        let allowed = if crate::acp_fs::is_inside_cwd(&path, &workspace) {
                            true
                        } else {
                            crate::acp_fs::confirm_write_outside(&bus, &session_id, &path).await
                        };
                        let result = if allowed {
                            crate::acp_fs::write_text_file(&path, &content)
                                .map(|_| WriteTextFileResponse::new())
                        } else {
                            Err(crate::acp_fs::denied_write())
                        };
                        match result {
                            Ok(resp) => {
                                let _ = responder.respond(resp);
                            }
                            Err(err) => {
                                let _ = responder.respond_with_error(err);
                            }
                        }
                    });
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_create;
                let bus = bus_create;
                async move |req: CreateTerminalRequest, responder, _cx| {
                    // Asking the user takes an async round-trip through the
                    // permission overlay — the handler offloads like the
                    // fs/write handler does. Everything the spawned task
                    // needs is cloned per call: the handler itself must
                    // stay callable.
                    let terms = Arc::clone(&terms);
                    let env: Vec<(String, String)> = req
                        .env
                        .iter()
                        .map(|e| (e.name.clone(), e.value.clone()))
                        .collect();
                    let session_id = req.session_id.to_string();
                    let command = req.command;
                    let args = req.args;
                    let cwd = req.cwd;
                    let output_byte_limit = req.output_byte_limit;
                    let bus = bus.clone();
                    let workspace = workspace_create.clone();
                    tokio::spawn(async move {
                        let resolved_cwd = cwd
                            .map(|c| {
                                crate::acp_fs::resolve_with_cwd(
                                    &c,
                                    std::path::Path::new(&workspace),
                                )
                            })
                            .unwrap_or_else(|| workspace.clone().into());
                        let allowed = crate::acp_fs::confirm_terminal_spawn(
                            &bus,
                            &session_id,
                            &command,
                            &args,
                            &resolved_cwd,
                        )
                        .await;
                        let result = if allowed {
                            terms.create(
                                &command,
                                &args,
                                Some(resolved_cwd),
                                &env,
                                output_byte_limit,
                            )
                        } else {
                            Err("user denied createTerminal".to_string())
                        };
                        match result {
                            Ok(id) => responder.respond(CreateTerminalResponse::new(id)),
                            Err(err) => responder.respond_with_error(AcpError::new(-32603, err)),
                        }
                    });
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_output;
                async move |req: TerminalOutputRequest, responder, _cx| {
                    match terms.output(&req.terminal_id.to_string()) {
                        Ok((output, truncated, exit)) => {
                            let mut resp = TerminalOutputResponse::new(output, truncated);
                            if let Some(exit) = exit {
                                resp = resp.exit_status(exit);
                            }
                            responder.respond(resp)
                        }
                        Err(err) => responder.respond_with_error(AcpError::new(-32602, err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_wait;
                async move |req: WaitForTerminalExitRequest, responder, _cx| {
                    let terms = Arc::clone(&terms);
                    let terminal_id = req.terminal_id.to_string();
                    tokio::spawn(async move {
                        match terms.wait(&terminal_id).await {
                            Ok(status) => {
                                let _ = responder.respond(WaitForTerminalExitResponse::new(status));
                            }
                            Err(err) => {
                                let _ = responder.respond_with_error(AcpError::new(-32602, err));
                            }
                        }
                    });
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_kill;
                async move |req: KillTerminalRequest, responder, _cx| {
                    match terms.kill(&req.terminal_id.to_string()) {
                        Ok(()) => responder.respond(KillTerminalResponse::new()),
                        Err(err) => responder.respond_with_error(AcpError::new(-32602, err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_release;
                async move |req: ReleaseTerminalRequest, responder, _cx| {
                    match terms.release(&req.terminal_id.to_string()) {
                        Ok(()) => responder.respond(ReleaseTerminalResponse::new()),
                        Err(err) => responder.respond_with_error(AcpError::new(-32602, err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let bus = bus_config;
                let surface = surface_config;
                async move |request: UntypedMessage, responder, cx| {
                    if request.method() != crate::cordis::SESSION_CONFIG_SET {
                        return responder.respond_with_error(AcpError::new(
                            -32601,
                            format!("Method not found: {}", request.method()),
                        ));
                    }
                    let params = request.params();
                    if params.get("protocol").and_then(Value::as_u64)
                        != Some(crate::cordis::PROTOCOL)
                    {
                        return responder.respond_with_error(AcpError::new(
                            -32602,
                            "unsupported TUI Client config protocol",
                        ));
                    }
                    let Some(session) = params.get("sessionId").and_then(Value::as_str) else {
                        return responder.respond_with_error(AcpError::new(
                            -32602,
                            "sessionId must be a non-empty string",
                        ));
                    };
                    let Some(config_id) = params.get("configId").and_then(Value::as_str) else {
                        return responder.respond_with_error(AcpError::new(
                            -32602,
                            "configId must be a non-empty string",
                        ));
                    };
                    if session.is_empty() || config_id.is_empty() {
                        return responder.respond_with_error(AcpError::new(
                            -32602,
                            "sessionId and configId must be non-empty strings",
                        ));
                    }
                    let config_value = match params.get("value") {
                        Some(Value::String(value)) => {
                            SessionConfigOptionValue::value_id(value.clone())
                        }
                        Some(Value::Bool(value)) => SessionConfigOptionValue::boolean(*value),
                        _ => {
                            return responder.respond_with_error(AcpError::new(
                                -32602,
                                "value must be a string value id or boolean",
                            ));
                        }
                    };
                    let session = SessionId::new(session.to_string());
                    let config_id = config_id.to_string();
                    let bus = bus.clone();
                    let surface = Arc::clone(&surface);
                    tokio::spawn(async move {
                        match cx
                            .send_request(SetSessionConfigOptionRequest::new(
                                session.clone(),
                                config_id,
                                config_value,
                            ))
                            .block_task_deadline()
                            .await
                        {
                            Ok(response) => match serde_json::to_value(&response) {
                                Ok(value) => {
                                    let options = apply_config_response(
                                        &value,
                                        &session,
                                        &surface,
                                        &bus,
                                    )
                                    .unwrap_or_else(|| Value::Array(Vec::new()));
                                    let _ = responder.respond(json!({
                                        "sessionId": session.to_string(),
                                        "configOptions": options,
                                    }));
                                }
                                Err(error) => {
                                    let _ = responder.respond_with_error(AcpError::new(
                                        -32603,
                                        format!("serialize config response: {error}"),
                                    ));
                                }
                            },
                            Err(error) => {
                                let _ = responder.respond_with_error(error);
                            }
                        }
                    });
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .connect_with(transport, move |cx: ConnectionTo<Agent>| {
            let bus = bus;
            let surface = surface;
            let cfg = cfg;
            let _terms = terms;
            async move {
                let ConnectCarry {
                    mut fwd_rx,
                    mut pending,
                    mut parked,
                    mut session_auth_pending,
                    terminal_method,
                } = carry;
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Starting {
                    runtime: "acp".into(),
                }));
                let init = cx.send_request(initialize_request()).block_task_setup_deadline().await;
                // A failed handshake must not kill this task: the attach
                // channel still carries the local compositor plane, so client
                // diagnostics such as /status stay reachable. Report the
                // failure and keep the command loop running without an agent.
                // Loop state initialized up front: when the handshake failed,
                // everything below stays empty and agent-bound commands simply
                // error or park exactly like after a `session/new` failure.
                // One stdio connection drives many sessions: each bound id
                // gets its own turn state, and `current` is only the fallback
                // target for commands that arrive without a usable id.
                let mut sessions = HashMap::<String, SessionHandle>::new();
                let mut current: Option<SessionId> = None;
                // `pending` / `parked` / `session_auth_pending` came in via
                // `ConnectCarry` and survive a transport respawn.
                let mut methods = Vec::new();
                let mut selected: Option<AuthMethodInfo> = None;
                let mut additional_directories = false;
                let mut load_session = false;
                let mut resume_session = false;
                let mut list_session = false;
                // The handshake never produced an agent: prompt setup can
                // only wait on a dead peer, so prompts short-circuit instead
                // of parking the command loop behind a doomed `session/new`.
                // An agent that answered `initialize` with a refusal is still
                // attached — only failures that are not agent replies count.
                let mut agent_absent = match &init {
                    Ok(_) => false,
                    Err(err) => !init_was_answered(err),
                };
                if let Err(err) = &init {
                    if !agent_absent {
                        // A refused handshake may carry the agent's auth
                        // methods in its error data (the -32000 convention):
                        // mirror the refused-`session/new` path — surface
                        // them so the user can sign in, and hold prompts
                        // until authenticate settles. A refusal demanding
                        // auth while declaring no methods can never be
                        // remedied, so prompts short-circuit the same way.
                        let auth_raw = err
                            .data
                            .as_ref()
                            .and_then(|data| data.get("authMethods"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let env = process_env();
                        methods = parse_auth_methods(
                            &auth_raw,
                            &cfg.agent_argv(),
                            &cfg.workspace,
                            &env,
                        );
                        if is_auth_required_error(err) {
                            if methods.is_empty() {
                                agent_absent = true;
                            } else {
                                                                emit_needs_auth_open(
                                    &bus,
                                    methods.clone(),
                                    None,
                                    Some(init_refusal_message(err)),
                                );
                                session_auth_pending = true;
                            }
                        }
                    }
                    if agent_absent {
                        // Locally synthesized failure (dead transport, an
                        // exited child, our own deadline) or an auth
                        // refusal that declared no methods — no live agent
                        // can take another command.
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::ConnectionFailed {
                            target: String::new(),
                            error: acp_error_message(err),
                        }));
                    } else {
                        // The agent answered — it is attached, so the
                        // refused surface keeps the send path open: a queued
                        // prompt retries `session/new` once it reaches the
                        // command loop. Emit after `needs-auth` so the App
                        // has already recorded the auth state when it
                        // classifies the refusal.
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::ConnectionRefused {
                            target: String::new(),
                            error: init_refusal_message(err),
                        }));
                    }
                }
                let cwd = std::path::PathBuf::from(&cfg.workspace);
                if let Ok(init) = init {
                let agent_name = init
                    .agent_info
                    .as_ref()
                    .map(|info| info.name.clone())
                    .unwrap_or_else(|| "acp".into());
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Initialized { server: agent_name.clone() }));

                let init_value = serde_json::to_value(&init).unwrap_or(Value::Null);
                if let Ok(mut surface) = surface.lock() {
                    surface.prompt_image = init.agent_capabilities.prompt_capabilities.image
                        || prompt_image_supported(&init_value);
                    surface.cordis = crate::cordis::advertised_by_agent(&init_value);
                    surface.steer_route = steer_route(&init_value);
                    let mut connection = init_value.clone();
                    connection["command"] = json!(cfg.agent_argv().first());
                    connection["args"] = json!(cfg.agent_argv().into_iter().skip(1).collect::<Vec<_>>());
                    connection["cwd"] = json!(cfg.workspace);
                    surface.initial_connection = Some(session_connection_snapshot(&connection));
                }
                load_session = init.agent_capabilities.load_session
                    || load_session_supported(&init_value);
                resume_session = resume_session_supported(&init_value);
                // Before sessionCapabilities.list existed, Martty-compatible agents paired
                // session/list with the top-level loadSession flag. Keep that legacy route.
                list_session = list_session_supported(&init_value) || load_session;
                additional_directories = additional_directories_supported(&init_value);
                let fork_session = fork_session_supported(&init_value);
                let close_session = close_session_supported(&init_value);
                let auth_logout = init.agent_capabilities.auth.logout.is_some()
                    || init_value
                        .pointer("/agentCapabilities/auth/logout")
                        .is_some_and(Value::is_object);
                let _ = bus.send(AppEvent::Ctl(CtlEvent::AgentCaps {
                    load_session,
                    list_session,
                    resume_session,
                    fork_session,
                    close_session,
                    additional_directories,
                    auth_logout,
                }));
                let auth_raw = init_value
                    .get("authMethods")
                    .cloned()
                    .unwrap_or(Value::Null);
                let env = process_env();
                methods = parse_auth_methods(
                    &auth_raw,
                    &cfg.agent_argv(),
                    &cfg.workspace,
                    &env,
                );
                let declared = declared_auth_methods(&auth_raw);
                let auth = snapshot_from_methods(methods.clone(), &declared, &env);
                // Only authenticate selects a method; session/new cannot reveal
                // which persisted credential (if any) the Agent used.
                selected = None;
                // Startup never opens the picker on its own — the snapshot
                // only surfaces state; a real auth_required reply is what
                // opens it (issue #160).
                emit_auth(&bus, auth);
                // `pending` / `parked` / `session_auth_pending` came in via
                // `ConnectCarry` and survive a transport respawn.
                let mut setup_failed = false;
                                match create_prompt_session(
                    &cx,
                    &cwd,
                    &surface,
                    &bus,
                    &methods,
                    selected.as_ref(),
                    None,
                    additional_directories,
                    &cfg.additional_directories,
                )
                .await
                {
                    Ok(Some(sid)) => {
                        let owner = sid.0.clone();
                        bind_session(&mut sessions, &mut current, &mut pending, sid);
                        // A respawned connection (Terminal Auth reconnect)
                        // credits the method its launch ran (#160) and
                        // arrives with parked prompts to retry.
                        credit_terminal_method(&bus, &surface, &methods, &terminal_method);
                        requeue_connection_prompts(
                            &mut sessions, &current, &mut parked, &surface,
                            connection_id(&surface, &owner).as_deref(),
                        );
                    }
                    Ok(None) => {
                        session_auth_pending = true;
                    }
                    Err(err) => {
                        setup_failed = true;
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::ConnectionFailed {
                            target: agent_name.clone(), error: format!("session/new: {}", acp_error_message(&err))
                        }));
                    }
                }
                if !setup_failed {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Ready { server: agent_name }));
                }
                }
                let (prompt_done_tx, mut prompt_done_rx) =
                    tokio::sync::mpsc::unbounded_channel::<PromptFinish>();
                let (steer_done_tx, mut steer_done_rx) =
                    tokio::sync::mpsc::unbounded_channel::<SteerFinish>();
                // Monotonic turn generation: every started prompt task is
                // tagged, finishes carry the tag back, and a stale finish
                // can never clear a newer turn's occupancy marker.
                let mut prompt_gen: u64 = 0;
                let mut controls = ControlWorkers::default();
                let (control_done_tx, mut control_done_rx) = tokio::sync::mpsc::unbounded_channel();
                // Keep primary local operations FIFO, while allowing cancel and
                // change events through during an asynchronous install/search.
                // Dropping the JoinSet on shutdown aborts outstanding requests.
                let mut plugin_operations = tokio::task::JoinSet::new();
                let mut plugin_queue = VecDeque::<PluginOperation>::new();
                let mut deferred_commands = VecDeque::<Cmd>::new();
                let mut active_plugin_operation = None;
                let mut active_plugin_cancelled = false;
                loop {
                    if active_plugin_operation.is_none() {
                        if let Some(operation) = plugin_queue.pop_front() {
                            active_plugin_operation = Some(spawn_plugin_operation(
                                &mut plugin_operations, cx.clone(), operation,
                            ));
                            active_plugin_cancelled = false;
                        }
                    }
                    let deferred = if active_plugin_operation.is_none() {
                        deferred_commands.pop_front()
                    } else {
                        None
                    };
                    tokio::select! {
                        // A queued Esc or shutdown wins over a simultaneous reply.
                        biased;
                        cmd = async {
                            match deferred {
                                Some(cmd) => Some(cmd),
                                None => fwd_rx.recv().await,
                            }
                        } => {
                            let Some(mut cmd) = cmd else { break };
                            let bypass_primary = matches!(&cmd, Cmd::Shutdown)
                                || matches!(&cmd, Cmd::PluginOverlayEvent { event, .. }
                                    if event == "cancel" || event == "change");
                            if active_plugin_operation.is_some() && !bypass_primary {
                                // A primary callback may already have replaced the ACP
                                // child. Preserve prior command serialization until its
                                // result is consumed and any new handshake is finished.
                                deferred_commands.push_back(cmd);
                                continue;
                            }
                            if current.is_none() && parked.is_empty() {
                                if let Some(kind) = parked_prompt(&cmd) {
                                    if session_auth_pending {
                                        parked.push_back(ParkedPrompt {
                                            session: None,
                                            kind,
                                        });
                                        continue;
                                    }
                                    if agent_absent {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
                                            "agent is not attached".into(),
                                        )));
                                        continue;
                                    }
                                                                        match create_prompt_session(
                                        &cx,
                                        &cwd,
                                        &surface,
                                        &bus,
                                        &methods,
                                        selected.as_ref(),
                                        None,
                                        additional_directories,
                                        &cfg.additional_directories,
                                    )
                                    .await
                                    {
                                        Ok(Some(sid)) => {
                                            retarget_session(&mut cmd, &sid);
                                            bind_session(
                                                &mut sessions,
                                                &mut current,
                                                &mut pending,
                                                sid,
                                            );
                                            session_auth_pending = false;
                                        }
                                        setup_outcome => {
                                            // No session came back: an auth
                                            // refusal parks the prompt for
                                            // sign-in; any other failure is
                                            // reported. Either way the loop
                                            // moves to the next command.
                                            match setup_outcome {
                                                // `Ok(Some)` was handled
                                                // above — `Ok` can only be
                                                // `None` here.
                                                Ok(_) => {
                                                    session_auth_pending = true;
                                                    parked.push_back(ParkedPrompt {
                                                        session: None,
                                                        kind,
                                                    });
                                                }
                                                Err(err) => {
                                                    let _ = bus.send(AppEvent::Ctl(
                                                        CtlEvent::Error(format!(
                                                            "session/new: {err}"
                                                        )),
                                                    ));
                                                }
                                            }
                                            continue;
                                        }
                                    }
                                }
                            }
                            match cmd {
                        Cmd::Prompt { session_id: cmd_session, text } => {
                            let next = Cmd::Prompt {
                                session_id: cmd_session.clone(),
                                text,
                            };
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "prompt") {
                                Ok(Some(sid)) => {
                                    let stalled = auth_stalled_for(&sid.0, &parked, &surface);
                                    let handle = sessions.entry(sid.to_string()).or_default();
                                    if stalled {
                                        // Held for sign-in — never silently:
                                        // the tab re-opens the picker with
                                        // the reason (issue #160).
                                        handle.queue.push_back(next);
                                        let provider = auth_provider(&surface, Some(&sid.0));
                                        emit_held_prompt(&bus, &sid.0, &methods, provider.as_deref());
                                    } else if handle.inflight.is_some() || controls.busy(&sid.0) {
                                        handle.queue.push_back(next);
                                    } else {
                                        prompt_gen += 1;
                                        handle.inflight = begin_prompt(
                                            next,
                                            &cx,
                                            &bus,
                                            sid,
                                            &surface,
                                            &cfg.workspace,
                                            prompt_gen,
                                            &prompt_done_tx,
                                        )
                                        .map(|task| (prompt_gen, task));
                                    }
                                }
                                Ok(None) => {
                                                                        queue_connection_prompt(&mut pending, next, session_auth_pending,
                                        &bus, &methods, None);
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                                    session_id: cmd_session.clone(),
                                    message: err,
                                }));
                                }
                            }
                        }
                        Cmd::Steer { session_id: cmd_session, message_id, text } => {
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "steer") {
                                Ok(Some(sid)) => {
                                    let busy = sessions
                                        .get(&sid.to_string())
                                        .is_some_and(|handle| handle.inflight.is_some());
                                    if busy {
                                        let route = surface
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner())
                                            .steer_route_for(&sid.0);
                                        if let Some(handle) = sessions.get_mut(&sid.to_string()) {
                                            handle.steers_inflight += 1;
                                        }
                                        spawn_steer(
                                            cx.clone(),
                                            route,
                                            sid,
                                            vec![text.into()],
                                            message_id,
                                            steer_done_tx.clone(),
                                        );
                                    } else if auth_stalled_for(&sid.0, &parked, &surface) {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                            message_id,
                                            deferred: true,
                                        }));
                                    } else {
                                        let handle = sessions.entry(sid.to_string()).or_default();
                                        prompt_gen += 1;
                                        handle.inflight = begin_prompt(
                                            Cmd::Steer {
                                                session_id: cmd_session,
                                                message_id,
                                                text,
                                            },
                                            &cx,
                                            &bus,
                                            sid,
                                            &surface,
                                            &cfg.workspace,
                                            prompt_gen,
                                            &prompt_done_tx,
                                        )
                                        .map(|task| (prompt_gen, task));
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                            message_id,
                                            deferred: false,
                                        }));
                                    }
                                }
                                Ok(None) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                        message_id,
                                        deferred: true,
                                    }));
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                                    session_id: cmd_session.clone(),
                                    message: err,
                                }));
                                }
                            }
                        }
                        Cmd::PromptImages { session_id: cmd_session, blocks } => {
                            let next = Cmd::PromptImages {
                                session_id: cmd_session.clone(),
                                blocks,
                            };
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "prompt_images") {
                                Ok(Some(sid)) => {
                                    let stalled = auth_stalled_for(&sid.0, &parked, &surface);
                                    let handle = sessions.entry(sid.to_string()).or_default();
                                    if stalled {
                                        handle.queue.push_back(next);
                                        let provider = auth_provider(&surface, Some(&sid.0));
                                        emit_held_prompt(&bus, &sid.0, &methods, provider.as_deref());
                                    } else if handle.inflight.is_some() || controls.busy(&sid.0) {
                                        handle.queue.push_back(next);
                                    } else {
                                        prompt_gen += 1;
                                        handle.inflight = begin_prompt(
                                            next,
                                            &cx,
                                            &bus,
                                            sid,
                                            &surface,
                                            &cfg.workspace,
                                            prompt_gen,
                                            &prompt_done_tx,
                                        )
                                        .map(|task| (prompt_gen, task));
                                    }
                                }
                                Ok(None) => {
                                                                        queue_connection_prompt(&mut pending, next, session_auth_pending,
                                        &bus, &methods, None);
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                                    session_id: cmd_session.clone(),
                                    message: err,
                                }));
                                }
                            }
                        }
                        Cmd::SteerImages { session_id: cmd_session, message_id, blocks } => {
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "steer_images") {
                                Ok(Some(sid)) => {
                                    let busy = sessions
                                        .get(&sid.to_string())
                                        .is_some_and(|handle| handle.inflight.is_some());
                                    if busy {
                                        let (prompt_image, route) = {
                                            let surface = surface
                                                .lock()
                                                .unwrap_or_else(|e| e.into_inner());
                                            (surface.prompt_image_for(&sid.0), surface.steer_route_for(&sid.0))
                                        };
                                        match prompt_content_blocks(blocks, prompt_image, &cfg.workspace) {
                                            Ok(content) if !content.is_empty() => {
                                                if let Some(handle) = sessions.get_mut(&sid.to_string()) {
                                                    handle.steers_inflight += 1;
                                                }
                                                spawn_steer(
                                                    cx.clone(),
                                                    route,
                                                    sid,
                                                    content,
                                                    message_id,
                                                    steer_done_tx.clone(),
                                                )
                                            }
                                            Ok(_) => {
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
                                                    "empty image prompt".into(),
                                                )));
                                            }
                                            Err(err) => {
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                                                session_id: cmd_session.clone(),
                                                message: err,
                                            }));
                                            }
                                        }
                                    } else if auth_stalled_for(&sid.0, &parked, &surface) {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                            message_id,
                                            deferred: true,
                                        }));
                                    } else {
                                        let handle = sessions.entry(sid.to_string()).or_default();
                                        prompt_gen += 1;
                                        handle.inflight = begin_prompt(
                                            Cmd::SteerImages {
                                                session_id: cmd_session,
                                                message_id,
                                                blocks,
                                            },
                                            &cx,
                                            &bus,
                                            sid,
                                            &surface,
                                            &cfg.workspace,
                                            prompt_gen,
                                            &prompt_done_tx,
                                        )
                                        .map(|task| (prompt_gen, task));
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                            message_id,
                                            deferred: false,
                                        }));
                                    }
                                }
                                Ok(None) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                        message_id,
                                        deferred: true,
                                    }));
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionError {
                                        session_id: cmd_session.clone(),
                                        message: err,
                                    }));
                                }
                            }
                        }
                        Cmd::Interrupt { session_id: cmd_session } => {
                            // Only a live turn can be aborted. Setting the
                            // flag with no inflight prompt would poison the
                            // *next* prompt's finish (its result would be
                            // swallowed and the turn misreported as
                            // interrupted).
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "interrupt") {
                                Ok(Some(sid)) => {
                                    // `resolve_cmd_session` only returns ids
                                    // present in `sessions` — enter the slot
                                    // directly rather than an unreachable
                                    // if-let (issue #160).
                                    let handle = sessions.entry(sid.to_string()).or_default();
                                        if handle.inflight.is_some() {
                                            abort_turn(&cx, &Some(sid), &bus);
                                            handle.turn_aborted = true;
                                        } else {
                                            // No live turn, but queued or
                                            // auth-held prompts still pin
                                            // the session: Esc must resolve
                                            // those too — drop the pending
                                            // sends, say so, and settle the
                                            // turn the UI believes is in
                                            // flight (issue #160).
                                            let queued = std::mem::take(&mut handle.queue).len();
                                            let held = parked.iter().filter(|prompt| {
                                                prompt.session.as_deref() == Some(&*sid.0)
                                            }).count();
                                            parked.retain(|prompt| {
                                                prompt.session.as_deref() != Some(&*sid.0)
                                            });
                                            let dropped = queued + held;
                                            if dropped > 0 {
                                                let _ = bus.send(AppEvent::Ui(
                                                    crate::events::UiEvent::SessionNotice {
                                                        session: sid.to_string(),
                                                        severity: "warn".into(),
                                                        title: format!(
                                                            "cancelled — discarded {} pending prompt{}",
                                                            dropped,
                                                            if dropped == 1 { "" } else { "s" }
                                                        ),
                                                        details: None,
                                                    },
                                                ));
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted {
                                                    session_id: sid.to_string(),
                                                }));
                                            }
                                        }
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionOpFailed {
                                        session_id: cmd_session.clone(),
                                        message: err,
                                    }));
                                }
                            }
                        }
                        Cmd::CancelAuth { method_id } => {
                            // Issue #168: dropping the auth lane's in-flight
                            // future cancels the pending `authenticate` RPC
                            // await (a hung OAuth flow included), so the
                            // replacement method is not queued behind it.
                            // `cancel_auth` also invalidates any reply that
                            // already raced back, so it cannot clobber the
                            // new UI state. `method_id` names the superseded
                            // flow; the attempt token is what gates it.
                            let _ = method_id;
                            controls.cancel_auth();
                        }
                        Cmd::AuthSkipped { session_id: cmd_session } => {
                            // Esc cancels a pending sign-in too (issue #168):
                            // abort its in-flight request, then release any
                            // prompts parked behind the stall below.
                            controls.cancel_auth();
                            match resolve_cmd_session(&sessions, &current, &cmd_session, "auth skip") {
                                Ok(Some(sid)) => {
                                    let before = parked.len();
                                    parked.retain(|prompt| {
                                        prompt.session.as_deref() != Some(&*sid.0)
                                    });
                                    // Sends queued behind the stall go with
                                    // it — after Esc the user must be able to
                                    // carry on, not stay pinned under a queue
                                    // that only a sign-in could drain (issue
                                    // #160).
                                    let queued = sessions
                                        .get_mut(&sid.to_string())
                                        .map(|handle| std::mem::take(&mut handle.queue).len())
                                        .unwrap_or(0);
                                    let dropped = before - parked.len() + queued;
                                    if dropped > 0 {
                                        let _ = bus.send(AppEvent::Ui(
                                            crate::events::UiEvent::SessionNotice {
                                                session: sid.to_string(),
                                                severity: "warn".into(),
                                                title: format!(
                                                    "sign-in skipped — discarded {} held prompt(s)",
                                                    dropped
                                                ),
                                                details: None,
                                            },
                                        ));
                                    }
                                }
                                Ok(None) => {
                                    // No session exists yet: the prompts
                                    // that parked on the auth gate are the
                                    // anonymous pre-session ones, plus
                                    // whatever waits in `pending` for a
                                    // session to exist. Skipping sign-in
                                    // must drop them too, or the user is
                                    // still stuck (issue #160).
                                    let before = parked.len();
                                    parked.retain(|prompt| prompt.session.is_some());
                                    let dropped =
                                        before - parked.len() + std::mem::take(&mut pending).len();
                                    session_auth_pending = false;
                                    if dropped > 0 {
                                        let _ = bus.send(AppEvent::Ui(
                                            crate::events::UiEvent::SessionNotice {
                                                session: cmd_session.clone(),
                                                severity: "warn".into(),
                                                title: format!(
                                                    "sign-in skipped — discarded {} held prompt(s)",
                                                    dropped
                                                ),
                                                details: None,
                                            },
                                        ));
                                    }
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionOpFailed {
                                    session_id: cmd_session.clone(),
                                    message: err,
                                }));
                                }
                            }
                        }
                        Cmd::FetchCatalog { session_id } => {
                            let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                            let scoped = surface.session(&session_id);
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog {
                                session_id: Some(session_id.clone()),
                                models: scoped.models.clone(),
                                presets: scoped.presets.clone(),
                            }));
                            if !scoped.modes.is_empty() {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
                                    session_id: Some(session_id),
                                    modes: scoped.modes.clone(),
                                    current: None,
                                }));
                            }
                        }
                        Cmd::FetchSkills { session_id } => {
                            let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                            let skills = surface.session(&session_id).skills.clone();
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Skills {
                                session_id: Some(session_id),
                                skills,
                            }));
                        }
                        Cmd::InvokePluginCommand { name, args } => {
                            if !ensure_client_compositor(&surface, &bus) {
                                continue;
                            }
                            plugin_queue.push_back(PluginOperation {
                                method: crate::cordis::COMMAND_INVOKE,
                                params: serde_json::json!({
                                    "protocol": 0,
                                    "name": name,
                                    "args": args,
                                }),
                                context: "plugin command",
                            });
                        }
                        Cmd::PluginThemeSelected { agent_id, id } => {
                            if !ensure_client_compositor(&surface, &bus) {
                                continue;
                            }
                            let result = call_tui_extension(
                                &cx,
                                crate::cordis::THEME_SELECTED,
                                serde_json::json!({
                                    "protocol": 0,
                                    "agentId": agent_id,
                                    "id": id,
                                }),
                            )
                            .await;
                            if let Err(error) = result {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "theme Plugin selection failed: {error}"
                                ))));
                            }
                        }
                        Cmd::PluginUiSelected { agent_id, id } => {
                            if !ensure_client_compositor(&surface, &bus) {
                                continue;
                            }
                            match call_tui_extension(
                                &cx,
                                crate::cordis::UI_SELECTED,
                                serde_json::json!({
                                    "protocol": crate::cordis::PROTOCOL,
                                    "agentId": agent_id,
                                    "id": id,
                                }),
                            )
                            .await
                            {
                                Ok(_) => {}
                                Err(error) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                        "UI Plugin selection failed: {error}"
                                    ))));
                                }
                            }
                        }
                        Cmd::PluginOverlayEvent { id, event, value } => {
                            if !ensure_client_compositor(&surface, &bus) {
                                continue;
                            }
                            let mut params = serde_json::json!({
                                "protocol": 0,
                                "id": id,
                                "event": event,
                            });
                            if let Some(value) = value {
                                params["value"] = value;
                            }
                            if event == "change" {
                                if let Err(error) = UntypedMessage::new(crate::cordis::OVERLAY_EVENT, params)
                                    .and_then(|notification| cx.send_notification(notification))
                                {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                        "plugin overlay event failed: {error}"
                                    ))));
                                }
                            } else {
                                let operation = PluginOperation {
                                    method: crate::cordis::OVERLAY_EVENT,
                                    params,
                                    context: "plugin overlay event",
                                };
                                if event == "cancel" {
                                    active_plugin_cancelled = active_plugin_operation.is_some();
                                    spawn_plugin_operation(&mut plugin_operations, cx.clone(), operation);
                                } else {
                                    plugin_queue.push_back(operation);
                                }
                            }
                        }
                        Cmd::FetchEfforts { session_id, .. } => {
                            let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                            let efforts = surface.session(&session_id).efforts.clone();
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Efforts {
                                session_id: Some(session_id.clone()),
                                efforts: if efforts.is_empty() {
                                    vec!["off".into(), "high".into(), "max".into()]
                                } else {
                                    efforts.clone()
                                },
                                default: surface.session(&session_id).effort_current.clone().or_else(|| efforts.first().cloned()),
                            }));
                        }
                        Cmd::ForgetSession { session_id } => {
                            // This client stopped viewing the session.
                            // Drop its turn state and queued prompts so
                            // nothing more is sent agent-side; an
                            // in-flight prompt (if any) keeps running and
                            // its finish is dropped by the prompt_done
                            // guard. The server-side session survives and
                            // can be re-entered later via session/resume.
                            parked.retain(|p| p.session.as_deref() != Some(&session_id));
                            if sessions.remove(&session_id).is_some()
                                && current
                                    .as_ref()
                                    .is_some_and(|c| c.to_string() == session_id)
                            {
                                // Repoint the id-less fallback at any
                                // remaining session.
                                current = sessions
                                    .keys()
                                    .next()
                                    .map(|id| SessionId::new(id.clone()));
                            }
                        }
                        Cmd::CloseSession { session_id } => {
                            // User-facing close (issue #146): when the
                            // owning connection advertised
                            // `sessionCapabilities.close`, put
                            // `session/close` on the wire first (setup
                            // lane, after any queued lifecycle request);
                            // the local drop below happens either way —
                            // and is the only step for agents that never
                            // advertised the capability. Only a session
                            // that actually bound can be closed agent-
                            // side: an unbound placeholder has no server
                            // session to free.
                            let advertised = {
                                let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                                surface
                                    .session(&session_id)
                                    .connection
                                    .as_ref()
                                    .map(|connection| {
                                        session_connection_snapshot(connection).close_session
                                    })
                                    .unwrap_or_else(|| {
                                        surface
                                            .initial_connection
                                            .as_ref()
                                            .is_some_and(|connection| connection.close_session)
                                    })
                            };
                            if advertised && sessions.contains_key(&session_id) {
                                controls.enqueue(
                                    Cmd::CloseSession {
                                        session_id: session_id.clone(),
                                    },
                                    &cx,
                                    &bus,
                                    &surface,
                                    &methods,
                                    &selected,
                                    &cwd,
                                    load_session,
                                    resume_session,
                                    list_session,
                                    additional_directories,
                                    &cfg.additional_directories,
                                    &control_done_tx,
                                );
                            }
                            parked.retain(|p| p.session.as_deref() != Some(&session_id));
                            if sessions.remove(&session_id).is_some()
                                && current
                                    .as_ref()
                                    .is_some_and(|c| c.to_string() == session_id)
                            {
                                current = sessions
                                    .keys()
                                    .next()
                                    .map(|id| SessionId::new(id.clone()));
                            }
                        }
                        Cmd::AgentReconnect { method_id } => {
                            if can_reconnect {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(
                                    "terminal sign-in finished — restarting the agent…".into(),
                                )));
                                return Ok(ConnectOutcome::Reconnect(ConnectCarry {
                                    fwd_rx,
                                    pending,
                                    parked,
                                    session_auth_pending: false,
                                    terminal_method: method_id,
                                }));
                            }
                            // A host-owned stream cannot be respawned: retry
                            // session/new in place — the agent may already
                            // accept the freshly written credentials. Whatever
                            // it answers stays visible (#160).
                                                        match create_prompt_session(
                                &cx,
                                &cwd,
                                &surface,
                                &bus,
                                &methods,
                                selected.as_ref(),
                                None,
                                additional_directories,
                                &cfg.additional_directories,
                            )
                            .await
                            {
                                Ok(Some(sid)) => {
                                    let owner = sid.0.clone();
                                    bind_session(&mut sessions, &mut current, &mut pending, sid);
                                    session_auth_pending = false;
                                    credit_terminal_method(&bus, &surface, &methods, &method_id);
                                    // Fresh credentials may clear the stall
                                    // that parked prompts on this connection.
                                    requeue_connection_prompts(
                                        &mut sessions, &current, &mut parked, &surface,
                                        connection_id(&surface, &owner).as_deref(),
                                    );
                                }
                                Ok(None) => {
                                    session_auth_pending = true;
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
                                        format!("session/new after terminal sign-in: {err}"),
                                    )));
                                }
                            }
                        }
                        Cmd::Shutdown => break,
                        control => {
                            let mut control = control;
                            let target = match &mut control {
                                Cmd::SelectModel { session_id, .. }
                                | Cmd::SetPermission { session_id, .. }
                                | Cmd::SetPreset { session_id, .. }
                                | Cmd::SetConfigOption { session_id, .. } => Some(session_id),
                                _ => None,
                            };
                            if let Some(target) = target {
                                match resolve_cmd_session(&sessions, &current, target, "session operation") {
                                    Ok(Some(sid)) => *target = sid.to_string(),
                                    Ok(None) => continue,
                                    Err(message) => {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionOpFailed {
                                            session_id: target.clone(), message,
                                        }));
                                        continue;
                                    }
                                }
                            }
                            controls.enqueue(control, &cx, &bus, &surface, &methods, &selected,
                                &cwd, load_session, resume_session, list_session,
                                additional_directories, &cfg.additional_directories, &control_done_tx);
                        }
                    }
                            if active_plugin_operation.is_none() { drain_ready_sessions(
                                &controls,
                                &mut sessions,
                                &cx,
                                &bus,
                                &mut parked,
                                &surface,
                                &current,
                                &cfg.workspace,
                                &mut prompt_gen,
                                &prompt_done_tx,
                            ); }
                        }
                        finished = plugin_operations.join_next_with_id(), if !plugin_operations.is_empty() => {
                            let primary = plugin_op_is_primary(&finished, active_plugin_operation);
                            let cancelled = primary && active_plugin_cancelled;
                            if primary {
                                active_plugin_operation = None;
                                active_plugin_cancelled = false;
                            }
                            if let Some(event) = plugin_op_finish_event(finished, primary, cancelled) {
                                let _ = bus.send(AppEvent::Ctl(event));
                            }
                        }
                        Some(finish) = control_done_rx.recv(), if active_plugin_operation.is_none() => {
                            match finish {
                                ControlFinish::SessionOperationDone { session_id } => controls.settled(&session_id),
                                ControlFinish::Setup { result, requester } => match result {
                                    Ok((sid, setup, notice)) => {
                                        bind_session(&mut sessions, &mut current, &mut pending, sid.clone());
                                        session_auth_pending = false;
                                        if let Some(previous_id) = requester {
                                            // Held prompts on the session this
                                            // bind replaces ride along to the
                                            // new one (issue #160).
                                            retarget_parked(&mut parked, &previous_id, &sid.0);
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionBoundTo {
                                                previous_id, session_id: sid.to_string(), notice,
                                            }));
                                        } else { emit_session_bound(&bus, &sid, notice); }
                                        apply_setup(&setup, Some(&sid.0), &surface, &bus);
                                    }
                                    Err(err) => {
                                        let message = acp_error_message(&err);
                                        if let Some(previous_id) = &requester {
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::BindFailedTo { previous_id: previous_id.clone(), message: message.clone() }));
                                        } else {
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::BindFailed { message: message.clone() }));
                                        }
                                        if let Some(value) = err.data.as_ref().and_then(|data| data.get("marttyConnection")) {
                                            let mut connection = session_connection_snapshot(value);
                                            if is_auth_required_error(&err) {
                                                connection.auth.status = AuthStatus::NeedsAuth;
                                                connection.auth.message = Some(message.clone());
                                                let provider = auth_provider(&surface, requester.as_deref());
                                                name_needs_auth(&mut connection.auth, &methods, provider.as_deref());
                                                let mut surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                                                if let Some(id) = &requester { surface.session_mut(Some(id)).connection = Some(value.clone()); }
                                                for method in &connection.auth.methods {
                                                    surface.auth_methods.insert(method.id.clone(), method.clone());
                                                    if let Some(requester) = &requester {
                                                        surface.failed_auth_setups.entry(method.id.clone()).or_default().push(requester.clone());
                                                    }
                                                }
                                            }
                                            if let Some(session_id) = requester {
                                                let snapshot = connection.auth.clone();
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionConnection { session_id: session_id.clone(), connection }));
                                                if is_auth_required_error(&err) { let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open: true })); }
                                            }
                                        } else if is_auth_required_error(&err) {
                                                                                        emit_needs_auth_open(&bus, methods.clone(), None, Some(message));
                                        }
                                    }
                                },
                                ControlFinish::Authenticated { token, .. }
                                    if !controls.auth_token_is_current(token) =>
                                {
                                    // Issue #168: the sign-in this reply belongs
                                    // to was cancelled and replaced. Its late
                                    // Configured/Failed result must not touch the
                                    // surface or the replacement's UI state.
                                }
                                ControlFinish::Authenticated { method, result, .. } => match result {
                                    Ok(()) => {
                                        selected = Some(method.clone());
                                        emit_method_auth(&bus, &surface, configured_snapshot(methods.clone(), Some(&method)));
                                        let retries = {
                                            let mut surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                                            let retries = surface.failed_auth_setups.remove(&method.id).unwrap_or_default();
                                            for pending in surface.failed_auth_setups.values_mut() { pending.retain(|id| !retries.contains(id)); }
                                            retries
                                        };
                                        for session_id in retries {
                                            controls.enqueue(Cmd::NewSession { requester: Some(session_id), retry_auth: Some(method.id.clone()) },
                                                &cx, &bus, &surface, &methods, &selected, &cwd,
                                                load_session, resume_session, list_session,
                                                additional_directories, &cfg.additional_directories, &control_done_tx);
                                        }
                                        if current.is_none() {
                                                                                        match create_prompt_session(&cx, &cwd, &surface, &bus, &methods, Some(&method), None, additional_directories, &cfg.additional_directories).await {
                                                Ok(Some(sid)) => {
                                                    bind_session(&mut sessions, &mut current, &mut pending, sid);
                                                    session_auth_pending = false;
                                                }
                                                Ok(None) => { session_auth_pending = true; continue; }
                                                Err(err) => {
                                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!("session/new after authenticate: {err}"))));
                                                    continue;
                                                }
                                            }
                                        }
                                        let owner = {
                                            let s = surface.lock().unwrap_or_else(|e| e.into_inner());
                                            s.sessions.values().filter_map(|session| session.connection.as_ref()).find(|connection|
                                                session_connection_snapshot(connection).auth.methods.iter().any(|known| known.id == method.id))
                                                .and_then(|connection| connection.get("id")).and_then(Value::as_str).map(str::to_string)
                                        };
                                        // A status-reporting agent may not have emitted
                                        // the update yet — credit the method that just
                                        // signed in so prompts parked on its provider
                                        // can release.
                                        {
                                            let mut guard = surface
                                                .lock()
                                                .unwrap_or_else(|e| e.into_inner());
                                            if let (Some(set), Some(provider)) = (
                                                guard.authenticated_providers.as_mut(),
                                                method_provider_id(&method).map(str::to_string),
                                            ) {
                                                if !set.iter().any(|(id, _)| id == &provider) {
                                                    set.push((provider, method.name.clone().unwrap_or_else(|| method.id.clone())));
                                                }
                                            }
                                        }
                                        let retried = requeue_connection_prompts(&mut sessions, &current, &mut parked, &surface, owner.as_deref());
                                        // Prompts parked on a provider this sign-in did
                                        // not cover stay held — they wait for the
                                        // session's own credential (issue #160).
                                        let held: Vec<String> = {
                                            let guard = surface.lock().unwrap_or_else(|e| e.into_inner());
                                            let mut providers: Vec<String> = parked.iter()
                                                .filter(|prompt| !release_ready(&guard, prompt.session.as_deref(), &current))
                                                .filter_map(|prompt| prompt.session.as_deref()
                                                    .and_then(|id| guard.session(id).model_current.as_deref())
                                                    .map(provider_head).map(str::to_string))
                                                .collect();
                                            providers.sort();
                                            providers.dedup();
                                            providers
                                        };
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(signed_in_notice(retried, &held))));
                                        // Switching accounts: the agent's
                                        // credential report no longer lists
                                        // the bound session's provider — it
                                        // can never serve again — so rebind
                                        // it; the agent picks the model and
                                        // parked prompts follow the new
                                        // session (issue #160).
                                        let dead_provider = dead_provider(&surface, &current);
                                        if let (true, Some(cur)) = (dead_provider, current.as_ref()) {
                                            controls.enqueue(Cmd::NewSession { requester: Some(cur.0.to_string()), retry_auth: Some(method.id.clone()) },
                                                &cx, &bus, &surface, &methods, &selected, &cwd,
                                                load_session, resume_session, list_session,
                                                additional_directories, &cfg.additional_directories, &control_done_tx);
                                        }
                                    }
                                    Err(err) => {
                                        selected = Some(method.clone());
                                        let mut failure = needs_auth_snapshot(methods.clone(), Some(&method), Some(acp_error_message(&err)));
                                        failure.status = AuthStatus::Failed;
                                        emit_method_auth(&bus, &surface, failure);
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!("authenticate: {}", acp_error_message(&err)))));
                                    }
                                },
                                ControlFinish::LoggedOut { provider, result } => match result {
                                    Ok(_response) => {
                                        // Name the credential the request
                                        // targeted — a bare `logout` has no
                                        // standard way to report what it
                                        // cleared, and agent-specific
                                        // `_meta` stays unread (issue #160).
                                        let scoped = provider.as_ref();
                                        let signed_out = selected.take().filter(|method| {
                                            method_provider_id(method)
                                                == scoped.map(String::as_str)
                                        });
                                        // A scoped logout can leave other
                                        // credentials active — name the ones
                                        // the agent reports still signed in.
                                        let remaining: Vec<String> = surface.lock()
                                            .map(|guard| {
                                                guard
                                                    .authenticated_providers
                                                    .as_deref()
                                                    .unwrap_or(&[])
                                                    .iter()
                                                    .map(|(_, name)| name.clone())
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        let message = if remaining.is_empty() {
                                            "signed out — pick a method to sign in again".to_string()
                                        } else {
                                            format!("signed out — still signed in: {}", remaining.join(", "))
                                        };
                                        emit_method_auth(&bus, &surface, needs_auth_snapshot(
                                            methods.clone(),
                                            // The sign-in disambiguates
                                            // methods that share a provider
                                            // (an OAuth and an API key for
                                            // the same account).
                                            signed_out.as_ref()
                                                .or_else(|| scoped
                                                    .and_then(|p| method_for_provider(&methods, p)))
                                                .or_else(|| sole_method(&methods)),
                                            Some(message),
                                        ));
                                        // The picker reopens so the user can
                                        // switch to another method/account.
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::OpenAuth));
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone("signed out".into())));
                                    }
                                    Err(err) => {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                            "logout: {}", acp_error_message(&err)
                                        ))));
                                    }
                                },
                            }
                            // This arm is select-guarded on `active_plugin_operation`
                            // being none, so draining is unconditional here.
                            drain_ready_sessions(&controls, &mut sessions, &cx, &bus, &mut parked,
                                &surface, &current, &cfg.workspace, &mut prompt_gen, &prompt_done_tx);
                        }
                        steer = steer_done_rx.recv() => {
                            if let Some(SteerFinish { session_id, message_id, deferred }) = steer {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred,
                                }));
                                // The last in-flight request of a session whose
                                // primary prompt already returned: the turn the
                                // agent ran for this steer is over now.
                                let settled = sessions.get_mut(&session_id).map(|handle| {
                                    handle.steers_inflight = handle.steers_inflight.saturating_sub(1);
                                    handle.inflight.is_none()
                                        && handle.steers_inflight == 0
                                        && handle.queue.is_empty()
                                });
                                if settled == Some(true) && !auth_stalled_for(&session_id, &parked, &surface) {
                                    let _ = bus.send(AppEvent::Rpc {
                                        method: "session.status".into(),
                                        params: json!({
                                            "sessionId": session_id,
                                            "status": "idle"
                                        }),
                                    });
                                }
                            }
                            if active_plugin_operation.is_none() { drain_ready_sessions(
                                &controls,
                                &mut sessions,
                                &cx,
                                &bus,
                                &mut parked,
                                &surface,
                                &current,
                                &cfg.workspace,
                                &mut prompt_gen,
                                &prompt_done_tx,
                            ); }
                        }
                        finish = prompt_done_rx.recv() => {
                            let _ = finish.map(|done| {
                            let key = done.session_id.clone();
                            // A session closed with `/close` (ForgetSession)
                            // while its prompt was in flight: the turn ends
                            // here — its UI events would be foreign on every
                            // tab, and an auth error must not park a retry
                            // for a session nobody views. Drop the finish.
                            let Some(handle) = sessions.get_mut(&key) else {
                                return;
                            };
                            // Stale finish (the session was forgotten and
                            // re-bound; a new turn owns the handle now):
                            // clear nothing — the new turn's occupancy
                            // marker must survive a late old finish.
                            if handle.inflight.as_ref().map(|(gen, _)| *gen) != Some(done.gen) {
                                return;
                            }
                            handle.inflight = None;
                            let aborted = std::mem::take(&mut handle.turn_aborted);
                            // `session/cancel` can race an already-settling
                            // turn: trust the agent's stop reason. A real
                            // interruption surfaces as `Cancelled` (or an
                            // error); a turn that completed despite the
                            // cancel still reports its own result.
                            let agent_cancelled = matches!(
                                &done.result,
                                Ok(response) if matches!(
                                    response.stop_reason,
                                    agent_client_protocol::schema::v1::StopReason::Cancelled
                                )
                            );
                            if aborted && (done.result.is_err() || agent_cancelled) {
                                let _ = bus.send(AppEvent::Ui(
                                    crate::events::UiEvent::TurnEnd {
                                        session: key.clone(),
                                        kind: "interrupted".into(),
                                    },
                                ));
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted {
                                    session_id: key.clone(),
                                }));
                            } else {
                                let ok = matches!(done.result, Ok(_));
                                let provider = auth_provider(&surface, Some(&done.session_id));
                                apply_prompt_finish(
                                    done,
                                    &mut sessions,
                                    &mut parked,
                                    &bus,
                                    &methods,
                                    provider.as_deref(),
                                );
                                if ok && !parked.is_empty() {
                                    // A success proves its provider is
                                    // credentialed — credit it so a
                                    // status-reporting agent's held prompts
                                    // can release.
                                    if let Some(provider) = provider.as_deref() {
                                        credit_authenticated_provider(&surface, provider);
                                    }
                                    // A success releases stalls only for
                                    // sessions on the same connection.
                                    requeue_connection_prompts(
                                        &mut sessions, &current, &mut parked, &surface,
                                        connection_id(&surface, &key).as_deref(),
                                    );
                                }
                            }
                            let queued = !auth_stalled_for(&key, &parked, &surface)
                                && sessions
                                    .get(&key)
                                    .is_some_and(|handle| !handle.queue.is_empty());
                            // A steer whose `session/prompt` has not returned
                            // keeps the session busy (pi-acp runs it as the next
                            // turn); its finish reports idle instead.
                            let steering = sessions
                                .get(&key)
                                .is_some_and(|handle| handle.steers_inflight > 0);
                            if queued || steering {
                                if active_plugin_operation.is_none() { drain_ready_sessions(
                                    &controls,
                                    &mut sessions,
                                    &cx,
                                    &bus,
                                    &mut parked,
                                    &surface,
                                    &current,
                                    &cfg.workspace,
                                    &mut prompt_gen,
                                    &prompt_done_tx,
                                ); }
                            } else {
                                let _ = bus.send(AppEvent::Rpc {
                                    method: "session.status".into(),
                                    params: json!({
                                        "sessionId": key,
                                        "status": "idle"
                                    }),
                                });
                            }
                            });
                        }
                    }
                    if active_plugin_operation.is_none() {
                        drain_ready_sessions(&controls, &mut sessions, &cx, &bus, &mut parked,
                            &surface, &current, &cfg.workspace,
                            &mut prompt_gen, &prompt_done_tx);
                    }
                }
                Ok(ConnectOutcome::Closed)
            }
        })
        .await
}

#[cfg(test)]
#[path = "../tests/unit/acp__tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/acp__additional_directories_tests.rs"]
mod additional_directories_tests;

#[cfg(test)]
#[path = "../tests/unit/acp__fork_tests.rs"]
mod fork_tests;

#[cfg(test)]
#[path = "../tests/unit/acp__reconnect_tests.rs"]
mod reconnect_tests;
