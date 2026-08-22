//! Official ACP client (`agent-client-protocol` 2.0).
//!
//! Speaks initialize / authenticate / session/new / session/load /
//! session/list / prompt / cancel / set_config_option / set_mode.
//! Transcript paint comes from `session/update`. Negotiated Cordis TUI
//! compositor state arrives as `_dsh/cordis/tui/*` extension notifications.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use agent_client_protocol::schema::v1::{
    AuthCapabilities, AuthenticateRequest, CancelNotification, ClientCapabilities,
    ClientSessionCapabilities, ContentBlock, CreateElicitationRequest, CreateElicitationResponse,
    CreateTerminalRequest, CreateTerminalResponse, ElicitationAcceptAction, ElicitationAction,
    ElicitationCapabilities, ElicitationContentValue, ElicitationFormCapabilities,
    FileSystemCapabilities, ImageContent, Implementation, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, ListSessionsRequest, LoadSessionRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResourceLink, SelectedPermissionOutcome,
    SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionId, SessionNotification,
    SetSessionConfigOptionRequest, SetSessionModeRequest, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    on_receive_notification, on_receive_request, AcpAgent, Agent, ByteStreams, Client, ConnectTo,
    ConnectionTo, Error as AcpError, Handled, UntypedMessage,
};

use crate::acp_auth::{
    acp_error_message, authenticate_meta_from_method, configured_snapshot, declared_auth_methods,
    is_auth_required_error, needs_auth_snapshot, parse_auth_methods, process_env,
    select_auth_method, snapshot_from_methods, AuthMethodInfo, AuthSnapshot, AuthStatus,
};
use crate::bus::{
    permission_ask_empty_outcome, AppEvent, CatalogPreset, Cmd, CordisPluginItem, CtlEvent,
    PermissionAskOption, PermissionAskReply, SessionListItem, StaticPluginItem,
};
use crate::events::{
    catalog_from_config_options, config_option_events, flatten_select_options,
    session_modes_from_value, skills_from_available_commands,
};
use crate::runtime::RuntimeConfig;

pub enum AcpEndpoint {
    Spawn(Vec<String>),
    AttachStdio {
        incoming: std::fs::File,
        outgoing: std::fs::File,
    },
    AttachTcp(std::net::TcpStream),
}

#[derive(Default)]
struct Surface {
    composition_id: Option<String>,
    models: Vec<crate::bus::CatalogModel>,
    presets: Vec<CatalogPreset>,
    skills: Vec<crate::bus::SkillInfo>,
    efforts: Vec<String>,
    modes: Vec<CatalogPreset>,
    /// Agent advertised `promptCapabilities.image` (ACP Image blocks allowed).
    prompt_image: bool,
    /// Agent negotiated the DSH Cordis ACP extension family.
    cordis: bool,
}

impl Surface {
    fn apply_config_options(&mut self, options: &Value, bus: &Sender<AppEvent>) {
        let (models, presets, composition_id) = catalog_from_config_options(options);
        self.models = models.clone();
        self.presets = presets.clone();
        if composition_id.is_some() {
            self.composition_id = composition_id;
        }
        if let Some(arr) = options.as_array() {
            if let Some(effort) = arr
                .iter()
                .find(|o| o.get("id").and_then(Value::as_str) == Some("effort"))
            {
                self.efforts =
                    flatten_select_options(effort.get("options").unwrap_or(&Value::Null))
                        .into_iter()
                        .map(|(id, _, _)| id)
                        .collect();
            }
            if self.modes.is_empty() {
                if let Some(mode) = arr
                    .iter()
                    .find(|o| o.get("id").and_then(Value::as_str) == Some("mode"))
                {
                    self.modes =
                        flatten_select_options(mode.get("options").unwrap_or(&Value::Null))
                            .into_iter()
                            .map(|(id, name, description)| CatalogPreset {
                                id,
                                name,
                                description,
                                broken: false,
                            })
                            .collect();
                    if !self.modes.is_empty() {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
                            modes: self.modes.clone(),
                            current: mode
                                .get("currentValue")
                                .or_else(|| mode.get("current_value"))
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        }));
                    }
                }
            }
        }
        let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog { models, presets }));
    }

    fn apply_session_modes(&mut self, modes: &Value, bus: &Sender<AppEvent>) {
        let (list, current) = session_modes_from_value(modes);
        if list.is_empty() {
            return;
        }
        self.modes = list.clone();
        let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
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
        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!("{err:#}"))));
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
            .name("dsh-tui")
            .connect_with(agent, |cx: ConnectionTo<Agent>| async move {
                let init = cx.send_request(initialize_request()).block_task().await?;
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
        .client_info(Implementation::new("dsh-tui", env!("CARGO_PKG_VERSION")))
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
                    ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()),
                )
                .meta(crate::acp_auth::client_capability_meta()),
        )
}

fn emit_auth(bus: &Sender<AppEvent>, snap: AuthSnapshot) {
    let _ = bus.send(AppEvent::Ctl(CtlEvent::Auth(snap)));
}

/// Parked composer payload retried after a later `authenticate` succeeds.
#[derive(Clone)]
enum ParkedPrompt {
    Text(String),
    Images(Vec<crate::bus::PromptBlock>),
}

struct PromptFinish {
    session_id: String,
    result: Result<agent_client_protocol::schema::v1::PromptResponse, AcpError>,
    parked_on_auth: ParkedPrompt,
}

struct SteerFinish {
    message_id: u64,
    fallback: Cmd,
    result: Result<agent_client_protocol::schema::v1::PromptResponse, AcpError>,
}

/// Backchat `AcpSession.#prompt`: abort does not wait for `session/prompt`.
/// It fires `agent.cancel({ sessionId })` (`session/cancel`) and lets the
/// prompt promise settle on its own.
fn abort_turn(cx: &ConnectionTo<Agent>, session_id: &Option<SessionId>, bus: &Sender<AppEvent>) {
    if let Some(sid) = session_id.clone() {
        let _ = cx.send_notification(CancelNotification::new(sid));
    }
    let _ = bus.send(AppEvent::Ctl(CtlEvent::CancelRequested));
}

fn spawn_session_prompt(
    cx: ConnectionTo<Agent>,
    bus: Sender<AppEvent>,
    sid: SessionId,
    content: Vec<ContentBlock>,
    parked_on_auth: ParkedPrompt,
) -> tokio::task::JoinHandle<PromptFinish> {
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
        }));
        let result = cx
            .send_request(PromptRequest::new(sid.clone(), content))
            .block_task()
            .await;
        PromptFinish {
            session_id: sid.to_string(),
            result,
            parked_on_auth,
        }
    })
}

/// A steer is another `session/prompt` sent while the active request is still
/// in flight. It belongs to that turn, so it must not open or close a second
/// local turn lifecycle.
fn spawn_steer_prompt(
    cx: ConnectionTo<Agent>,
    sid: SessionId,
    content: Vec<ContentBlock>,
    message_id: u64,
    fallback: Cmd,
    done: tokio::sync::mpsc::UnboundedSender<SteerFinish>,
) {
    tokio::spawn(async move {
        let result = cx
            .send_request(PromptRequest::new(sid, content))
            .block_task()
            .await;
        let _ = done.send(SteerFinish {
            message_id,
            fallback,
            result,
        });
    });
}

fn begin_prompt(
    cmd: Cmd,
    cx: &ConnectionTo<Agent>,
    bus: &Sender<AppEvent>,
    session_id: &Option<SessionId>,
    parked: &mut Option<ParkedPrompt>,
    methods: &[AuthMethodInfo],
    selected: Option<&AuthMethodInfo>,
    surface: &Arc<Mutex<Surface>>,
    workspace: &str,
) -> Option<tokio::task::JoinHandle<PromptFinish>> {
    match cmd {
        Cmd::Prompt { text, .. } => {
            let Some(sid) = session_id.clone() else {
                *parked = Some(ParkedPrompt::Text(text));
                emit_needs_auth_open(
                    bus,
                    methods.to_vec(),
                    selected,
                    Some(no_session().to_string()),
                );
                return None;
            };
            Some(spawn_session_prompt(
                cx.clone(),
                bus.clone(),
                sid,
                vec![text.clone().into()],
                ParkedPrompt::Text(text),
            ))
        }
        Cmd::Steer { text, .. } => {
            let Some(sid) = session_id.clone() else {
                *parked = Some(ParkedPrompt::Text(text));
                emit_needs_auth_open(
                    bus,
                    methods.to_vec(),
                    selected,
                    Some(no_session().to_string()),
                );
                return None;
            };
            Some(spawn_session_prompt(
                cx.clone(),
                bus.clone(),
                sid,
                vec![text.clone().into()],
                ParkedPrompt::Text(text),
            ))
        }
        Cmd::PromptImages { blocks, .. } => {
            let Some(sid) = session_id.clone() else {
                *parked = Some(ParkedPrompt::Images(blocks));
                emit_needs_auth_open(
                    bus,
                    methods.to_vec(),
                    selected,
                    Some(no_session().to_string()),
                );
                return None;
            };
            let parked_on_auth = ParkedPrompt::Images(blocks.clone());
            let prompt_image = surface
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .prompt_image;
            match prompt_content_blocks(blocks, prompt_image, workspace) {
                Ok(content) if !content.is_empty() => Some(spawn_session_prompt(
                    cx.clone(),
                    bus.clone(),
                    sid,
                    content,
                    parked_on_auth,
                )),
                Ok(_) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error("empty image prompt".into())));
                    None
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(err)));
                    None
                }
            }
        }
        Cmd::SteerImages { blocks, .. } => {
            let Some(sid) = session_id.clone() else {
                *parked = Some(ParkedPrompt::Images(blocks));
                emit_needs_auth_open(
                    bus,
                    methods.to_vec(),
                    selected,
                    Some(no_session().to_string()),
                );
                return None;
            };
            let parked_on_auth = ParkedPrompt::Images(blocks.clone());
            let prompt_image = surface
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .prompt_image;
            match prompt_content_blocks(blocks, prompt_image, workspace) {
                Ok(content) if !content.is_empty() => Some(spawn_session_prompt(
                    cx.clone(),
                    bus.clone(),
                    sid,
                    content,
                    parked_on_auth,
                )),
                Ok(_) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error("empty image prompt".into())));
                    None
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(err)));
                    None
                }
            }
        }
        _ => None,
    }
}

fn apply_prompt_finish(
    finish: PromptFinish,
    parked: &mut Option<ParkedPrompt>,
    bus: &Sender<AppEvent>,
    methods: &[AuthMethodInfo],
    selected: Option<&AuthMethodInfo>,
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
            *parked = None;
        }
        Err(err) if is_auth_required_error(&err) => {
            *parked = Some(finish.parked_on_auth);
            emit_needs_auth_open(
                bus,
                methods.to_vec(),
                selected,
                Some(acp_error_message(&err)),
            );
        }
        Err(err) => {
            let _ = bus.send(AppEvent::Ui(crate::events::UiEvent::TurnEnd {
                session: finish.session_id,
                kind: "error".into(),
            }));
            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!("prompt: {err}"))));
        }
    }
}

fn emit_needs_auth_open(
    bus: &Sender<AppEvent>,
    methods: Vec<AuthMethodInfo>,
    selected: Option<&AuthMethodInfo>,
    message: Option<String>,
) {
    emit_auth(bus, needs_auth_snapshot(methods, selected, message));
    let _ = bus.send(AppEvent::Ctl(CtlEvent::OpenAuth));
}

fn emit_open_auth_if_needed(bus: &Sender<AppEvent>, status: AuthStatus) {
    if status == AuthStatus::NeedsAuth {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::OpenAuth));
    }
}

fn parked_prompt(cmd: &Cmd) -> Option<ParkedPrompt> {
    match cmd {
        Cmd::Prompt { text, .. } | Cmd::Steer { text, .. } => {
            Some(ParkedPrompt::Text(text.clone()))
        }
        Cmd::PromptImages { blocks, .. } | Cmd::SteerImages { blocks, .. } => {
            Some(ParkedPrompt::Images(blocks.clone()))
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
) -> std::result::Result<Option<SessionId>, AcpError> {
    match cx
        .send_request(NewSessionRequest::new(cwd.to_path_buf()))
        .block_task()
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
                selected,
                Some(acp_error_message(&err)),
            );
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

fn apply_setup(
    value: &Value,
    session_hint: Option<&str>,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
) {
    if let Ok(mut surface) = surface.lock() {
        surface.modes.clear();
        if let Some(options) = value
            .get("configOptions")
            .or_else(|| value.get("config_options"))
        {
            surface.apply_config_options(options, bus);
            if let Some(session) = value
                .get("sessionId")
                .or_else(|| value.get("session_id"))
                .and_then(Value::as_str)
                .or(session_hint)
            {
                for event in config_option_events(session.to_string(), options) {
                    let _ = bus.send(AppEvent::Ui(event));
                }
            }
        }
        if let Some(modes) = value.get("modes") {
            surface.apply_session_modes(modes, bus);
        }
    }
}

fn apply_created(
    created: &agent_client_protocol::schema::v1::NewSessionResponse,
    surface: &Arc<Mutex<Surface>>,
    bus: &Sender<AppEvent>,
    notice: Option<String>,
) {
    if let Ok(value) = serde_json::to_value(created) {
        let session = created.session_id.to_string();
        apply_setup(&value, Some(&session), surface, bus);
    }
    // Queue the Host's authoritative model/effort/permission facts before
    // making the session visible; otherwise one frame renders workspace
    // cache values and then jumps to the Host state.
    emit_session_bound(bus, &created.session_id, notice);
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
        surface.apply_config_options(&options, bus);
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

fn no_session() -> AcpError {
    AcpError::new(-32000, "sign in first — /auth")
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

/// Unix `file://` + absolute path (`file:///tmp/a.png`).
fn unix_file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
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
                    if !abs.is_absolute() {
                        return Err(format!("cannot form file uri for {}", image.name));
                    }
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

async fn run(
    cfg: RuntimeConfig,
    endpoint: AcpEndpoint,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
) -> Result<()> {
    match endpoint {
        AcpEndpoint::Spawn(args) => {
            let agent = AcpAgent::from_args(args).map_err(acp_err)?;
            connect(agent, cfg, bus, cmd_rx).await.map_err(acp_err)
        }
        #[cfg(unix)]
        AcpEndpoint::AttachStdio { incoming, outgoing } => {
            let incoming = unix_stream_from_file(incoming)?.compat();
            let outgoing = unix_stream_from_file(outgoing)?.compat_write();
            connect(ByteStreams::new(outgoing, incoming), cfg, bus, cmd_rx)
                .await
                .map_err(acp_err)
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
                cmd_rx,
            )
            .await
            .map_err(acp_err)
        }
    }
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
            let package_id = row
                .get("currentPackageId")
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
                .ok_or_else(|| format!("plugins[{index}] has no package"))?;
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

async fn call_tui_extension(
    cx: &ConnectionTo<Agent>,
    method: &str,
    params: Value,
) -> std::result::Result<Value, AcpError> {
    let request = UntypedMessage::new(method, params)?;
    cx.send_request(request).block_task().await
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
    let advertised = surface
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .cordis;
    if !advertised {
        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
            "agent does not advertise _dsh/cordis".into(),
        )));
    }
    advertised
}

async fn connect<T>(
    transport: T,
    cfg: RuntimeConfig,
    bus: Sender<AppEvent>,
    cmd_rx: Receiver<Cmd>,
) -> std::result::Result<(), AcpError>
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
    let workspace = cfg.workspace.clone();
    let workspace_read = workspace.clone();
    let workspace_write = workspace.clone();
    let terms = Arc::new(crate::acp_term::TerminalBroker::new(workspace.clone()));
    let terms_create = Arc::clone(&terms);
    let terms_output = Arc::clone(&terms);
    let terms_wait = Arc::clone(&terms);
    let terms_kill = Arc::clone(&terms);
    let terms_release = Arc::clone(&terms);

    Client
        .builder()
        .name("dsh-tui")
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
                            if let Ok(mut surface) = surface_n.lock() {
                                surface.apply_config_options(options, &bus_n);
                            }
                        }
                        if let Some(commands) = update
                            .get("availableCommands")
                            .or_else(|| update.get("available_commands"))
                        {
                            let skills = skills_from_available_commands(commands);
                            if let Ok(mut surface) = surface_n.lock() {
                                surface.skills = skills.clone();
                            }
                            let _ = bus_n.send(AppEvent::Ctl(CtlEvent::Skills { skills }));
                        }
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .on_receive_notification(
            async move |msg: UntypedMessage, cx| {
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
                let form = match crate::elicitation::form_from_request(&req) {
                    Ok(form) => form,
                    Err(err) => {
                        return responder.respond_with_error(AcpError::new(-32602, err));
                    }
                };
                let (tx, rx) = tokio::sync::oneshot::channel();
                if bus_e
                    .send(AppEvent::ElicitationAsk { form, reply: tx })
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
                            crate::acp_fs::confirm_write_outside(&bus, &path).await
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
                async move |req: CreateTerminalRequest, responder, _cx| {
                    let env: Vec<(String, String)> = req
                        .env
                        .iter()
                        .map(|e| (e.name.clone(), e.value.clone()))
                        .collect();
                    match terms.create(
                        &req.command,
                        &req.args,
                        req.cwd,
                        &env,
                        req.output_byte_limit,
                    ) {
                        Ok(id) => responder.respond(CreateTerminalResponse::new(id)),
                        Err(err) => responder.respond_with_error(AcpError::new(-32603, err)),
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_output;
                async move |req: TerminalOutputRequest, responder, _cx| {
                    let (output, truncated, exit) = terms.output(&req.terminal_id.to_string());
                    let mut resp = TerminalOutputResponse::new(output, truncated);
                    if let Some(exit) = exit {
                        resp = resp.exit_status(exit);
                    }
                    responder.respond(resp)
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_wait;
                async move |req: WaitForTerminalExitRequest, responder, _cx| {
                    let terms = Arc::clone(&terms);
                    tokio::spawn(async move {
                        let status = terms.wait(&req.terminal_id.to_string()).await;
                        let _ = responder.respond(WaitForTerminalExitResponse::new(status));
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
                    terms.kill(&req.terminal_id.to_string());
                    responder.respond(KillTerminalResponse::new())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let terms = terms_release;
                async move |req: ReleaseTerminalRequest, responder, _cx| {
                    terms.release(&req.terminal_id.to_string());
                    responder.respond(ReleaseTerminalResponse::new())
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
                            .block_task()
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
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Starting {
                    runtime: "acp".into(),
                }));
                let init = cx.send_request(initialize_request()).block_task().await?;
                let agent_name = init
                    .agent_info
                    .as_ref()
                    .map(|info| info.name.clone())
                    .unwrap_or_else(|| "acp".into());

                let init_value = serde_json::to_value(&init).unwrap_or(Value::Null);
                if let Ok(mut surface) = surface.lock() {
                    surface.prompt_image = init.agent_capabilities.prompt_capabilities.image
                        || prompt_image_supported(&init_value);
                    surface.cordis = crate::cordis::advertised_by_agent(&init_value);
                }
                let load_session = init.agent_capabilities.load_session
                    || load_session_supported(&init_value);
                let _ = bus.send(AppEvent::Ctl(CtlEvent::AgentCaps { load_session }));
                let auth_raw = init_value
                    .get("authMethods")
                    .cloned()
                    .unwrap_or(Value::Null);
                let env = process_env();
                let methods = parse_auth_methods(
                    &auth_raw,
                    &cfg.agent_argv(),
                    &cfg.workspace,
                    &env,
                );
                let declared = declared_auth_methods(&auth_raw);
                let mut auth = snapshot_from_methods(methods.clone(), &declared, &env);
                let selected = select_auth_method(&methods, None).cloned();
                // A form-capable method may already have persistent credentials.
                // Without creating a throwaway session we cannot know yet, so stay
                // optimistic and let the startup session/new prove auth.
                if auth.status == AuthStatus::NeedsAuth
                    && selected.as_ref().is_some_and(|method| method.form)
                {
                    auth = configured_snapshot(methods.clone(), selected.as_ref());
                }

                let cwd = std::path::PathBuf::from(&cfg.workspace);
                let needs_open = auth.status == AuthStatus::NeedsAuth;
                emit_auth(&bus, auth);
                emit_open_auth_if_needed(&bus, if needs_open {
                    AuthStatus::NeedsAuth
                } else {
                    AuthStatus::None
                });
                let mut session_auth_pending = false;
                let mut session_id = match create_prompt_session(
                    &cx,
                    &cwd,
                    &surface,
                    &bus,
                    &methods,
                    selected.as_ref(),
                )
                .await
                {
                    Ok(Some(sid)) => Some(sid),
                    Ok(None) => {
                        session_auth_pending = true;
                        None
                    }
                    Err(err) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                            "session/new: {err}"
                        ))));
                        None
                    }
                };
                let _ = bus.send(AppEvent::Ctl(CtlEvent::Ready { server: agent_name }));
                let mut parked: Option<ParkedPrompt> = None;

                let (fwd_tx, mut fwd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
                std::thread::Builder::new()
                    .name("dsh-acp-cmds".into())
                    .spawn(move || {
                        while let Ok(cmd) = cmd_rx.recv() {
                            if fwd_tx.send(cmd).is_err() {
                                break;
                            }
                        }
                    })
                    .map_err(|err| AcpError::new(-32603, err.to_string()))?;

                let mut inflight: Option<tokio::task::JoinHandle<PromptFinish>> = None;
                let mut prompt_queue = VecDeque::<Cmd>::new();
                let (steer_done_tx, mut steer_done_rx) =
                    tokio::sync::mpsc::unbounded_channel::<SteerFinish>();
                let mut turn_aborted = false;
                loop {
                    tokio::select! {
                        cmd = fwd_rx.recv() => {
                            let Some(cmd) = cmd else { break };
                            if session_id.is_none()
                                && parked.is_none()
                                && parked_prompt(&cmd).is_some()
                            {
                                if session_auth_pending {
                                    parked = parked_prompt(&cmd);
                                    continue;
                                }
                                match create_prompt_session(
                                    &cx,
                                    &cwd,
                                    &surface,
                                    &bus,
                                    &methods,
                                    selected.as_ref(),
                                ).await {
                                    Ok(Some(sid)) => {
                                        session_id = Some(sid);
                                        session_auth_pending = false;
                                    }
                                    Ok(None) => {
                                        session_auth_pending = true;
                                        parked = parked_prompt(&cmd);
                                        continue;
                                    }
                                    Err(err) => {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                            "session/new: {err}"
                                        ))));
                                        continue;
                                    }
                                }
                            }
                            match cmd {
                        Cmd::Prompt { text, .. } => {
                            let next = Cmd::Prompt {
                                session_id: String::new(),
                                text,
                            };
                            if inflight.is_some() || parked.is_some() {
                                prompt_queue.push_back(next);
                            } else {
                                inflight = begin_prompt(
                                    next,
                                    &cx,
                                    &bus,
                                    &session_id,
                                    &mut parked,
                                    &methods,
                                    selected.as_ref(),
                                    &surface,
                                    &cfg.workspace,
                                );
                            }
                        }
                        Cmd::Steer { message_id, text, .. } => {
                            if inflight.is_some() {
                                if let Some(sid) = session_id.clone() {
                                    let fallback = Cmd::Prompt {
                                        session_id: sid.to_string(),
                                        text: text.clone(),
                                    };
                                    spawn_steer_prompt(
                                        cx.clone(),
                                        sid,
                                        vec![text.into()],
                                        message_id,
                                        fallback,
                                        steer_done_tx.clone(),
                                    );
                                }
                            } else if parked.is_some() {
                                prompt_queue.push_back(Cmd::Prompt {
                                    session_id: String::new(),
                                    text,
                                });
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred: true,
                                }));
                            } else {
                                inflight = begin_prompt(
                                    Cmd::Steer {
                                        session_id: String::new(),
                                        message_id,
                                        text,
                                    },
                                    &cx,
                                    &bus,
                                    &session_id,
                                    &mut parked,
                                    &methods,
                                    selected.as_ref(),
                                    &surface,
                                    &cfg.workspace,
                                );
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred: false,
                                }));
                            }
                        }
                        Cmd::PromptImages { blocks, .. } => {
                            let next = Cmd::PromptImages {
                                session_id: String::new(),
                                blocks,
                            };
                            if inflight.is_some() || parked.is_some() {
                                prompt_queue.push_back(next);
                            } else {
                                inflight = begin_prompt(
                                    next,
                                    &cx,
                                    &bus,
                                    &session_id,
                                    &mut parked,
                                    &methods,
                                    selected.as_ref(),
                                    &surface,
                                    &cfg.workspace,
                                );
                            }
                        }
                        Cmd::SteerImages { message_id, blocks, .. } => {
                            if inflight.is_some() {
                                if let Some(sid) = session_id.clone() {
                                    let fallback = Cmd::PromptImages {
                                        session_id: sid.to_string(),
                                        blocks: blocks.clone(),
                                    };
                                    let prompt_image = surface
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .prompt_image;
                                    match prompt_content_blocks(blocks, prompt_image, &cfg.workspace) {
                                        Ok(content) if !content.is_empty() => spawn_steer_prompt(
                                            cx.clone(),
                                            sid,
                                            content,
                                            message_id,
                                            fallback,
                                            steer_done_tx.clone(),
                                        ),
                                        Ok(_) => {
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
                                                "empty image prompt".into(),
                                            )));
                                        }
                                        Err(err) => {
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(err)));
                                        }
                                    }
                                }
                            } else if parked.is_some() {
                                prompt_queue.push_back(Cmd::PromptImages {
                                    session_id: String::new(),
                                    blocks,
                                });
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred: true,
                                }));
                            } else {
                                inflight = begin_prompt(
                                    Cmd::SteerImages {
                                        session_id: String::new(),
                                        message_id,
                                        blocks,
                                    },
                                    &cx,
                                    &bus,
                                    &session_id,
                                    &mut parked,
                                    &methods,
                                    selected.as_ref(),
                                    &surface,
                                    &cfg.workspace,
                                );
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred: false,
                                }));
                            }
                        }
                        Cmd::Interrupt { .. } => {
                            abort_turn(&cx, &session_id, &bus);
                            turn_aborted = true;
                        }
                        Cmd::SelectModel { model, effort, .. } => {
                            let Some(sid) = session_id.clone() else {
                                continue;
                            };
                            if let Some(model) = model {
                                let value = {
                                    let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                                    match surface.models.iter().find(|m| m.id == model) {
                                        Some(m) if !m.provider.is_empty() => {
                                            format!("{}/{}", m.provider, model)
                                        }
                                        _ => model.clone(),
                                    }
                                };
                                let _ = cx
                                    .send_request(SetSessionConfigOptionRequest::new(
                                        sid.clone(),
                                        "model",
                                        SessionConfigOptionValue::value_id(value),
                                    ))
                                    .block_task()
                                    .await
                                    .map_err(|err| {
                                        if is_auth_required_error(&err) {
                                            emit_needs_auth_open(
                                                &bus,
                                                methods.clone(),
                                                selected.as_ref(),
                                                Some(acp_error_message(&err)),
                                            );
                                        }
                                    });
                            }
                            if let Some(effort) = effort {
                                let _ = cx
                                    .send_request(SetSessionConfigOptionRequest::new(
                                        sid,
                                        "effort",
                                        SessionConfigOptionValue::value_id(effort),
                                    ))
                                    .block_task()
                                    .await
                                    .map_err(|err| {
                                        if is_auth_required_error(&err) {
                                            emit_needs_auth_open(
                                                &bus,
                                                methods.clone(),
                                                selected.as_ref(),
                                                Some(acp_error_message(&err)),
                                            );
                                        }
                                    });
                            }
                        }
                        Cmd::FetchCatalog => {
                            let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Catalog {
                                models: surface.models.clone(),
                                presets: surface.presets.clone(),
                            }));
                            if !surface.modes.is_empty() {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionModes {
                                    modes: surface.modes.clone(),
                                    current: None,
                                }));
                            }
                        }
                        Cmd::FetchSkills => {
                            let skills = surface
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .skills
                                .clone();
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Skills { skills }));
                        }
                        Cmd::FetchStaticPlugins => match fetch_static_plugins(&cx).await {
                            Ok(plugins) => {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::StaticPlugins { plugins }));
                            }
                            Err(error) => {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "static plugins unavailable: {error}"
                                ))));
                            }
                        },
                        Cmd::FetchCordisPlugins { agent_id } => {
                            if !ensure_agent_cordis(&surface, &bus) {
                                continue;
                            }
                            match fetch_dynamic_plugins(&cx, &agent_id).await {
                                Ok(plugins) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::CordisPlugins { plugins }));
                                }
                                Err(error) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                        "dynamic plugins unavailable: {error}"
                                    ))));
                                }
                            }
                        }
                        Cmd::SetCordisPluginEnabled {
                            agent_id,
                            plugin_id,
                            enabled,
                        } => {
                            if !ensure_agent_cordis(&surface, &bus) {
                                continue;
                            }
                            let method = if enabled {
                                crate::cordis::PLUGIN_START
                            } else {
                                crate::cordis::PLUGIN_STOP
                            };
                            let action = call_tui_extension(
                                &cx,
                                method,
                                serde_json::json!({
                                    "agentId": &agent_id,
                                    "pluginId": &plugin_id,
                                }),
                            )
                            .await;
                            match action {
                                Ok(value) if value.get("ok").and_then(Value::as_bool) == Some(false) => {
                                    let message = value
                                        .get("message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("the Host rejected the lifecycle change");
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                        message.to_string(),
                                    )));
                                }
                                Ok(_) => match fetch_dynamic_plugins(&cx, &agent_id).await {
                                    Ok(plugins) => {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::CordisPlugins { plugins }));
                                    }
                                    Err(error) => {
                                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                            "plugin changed, but inventory refresh failed: {error}"
                                        ))));
                                    }
                                },
                                Err(error) => {
                                    let action = if enabled { "restore" } else { "stop" };
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                        "plugin {action} failed: {error}"
                                    ))));
                                }
                            }
                        }
                        Cmd::RespondCordisApproval { request_id, decision } => {
                            if !ensure_agent_cordis(&surface, &bus) {
                                continue;
                            }
                            if let Err(error) = call_tui_extension(
                                &cx,
                                crate::cordis::APPROVAL_RESPOND,
                                serde_json::json!({
                                    "protocol": crate::cordis::PROTOCOL,
                                    "requestId": request_id,
                                    "decision": decision,
                                }),
                            )
                            .await
                            {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "plugin approval failed: {error}"
                                ))));
                            }
                        }
                        Cmd::InvokePluginCommand { name, args } => {
                            if !ensure_agent_cordis(&surface, &bus) {
                                continue;
                            }
                            if let Err(error) = call_tui_extension(
                                &cx,
                                crate::cordis::COMMAND_INVOKE,
                                serde_json::json!({
                                    "protocol": 0,
                                    "name": name,
                                    "args": args,
                                }),
                            )
                            .await
                            {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "plugin command failed: {error}"
                                ))));
                            }
                        }
                        Cmd::PluginThemeSelected { agent_id, id } => {
                            if !ensure_agent_cordis(&surface, &bus) {
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
                            if !ensure_agent_cordis(&surface, &bus) {
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
                            if !ensure_agent_cordis(&surface, &bus) {
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
                            let result = if event == "change" {
                                UntypedMessage::new(crate::cordis::OVERLAY_EVENT, params)
                                    .and_then(|notification| cx.send_notification(notification))
                                    .map(|_| Value::Null)
                            } else {
                                call_tui_extension(&cx, crate::cordis::OVERLAY_EVENT, params).await
                            };
                            if let Err(error) = result {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                                    "plugin overlay event failed: {error}"
                                ))));
                            }
                        }
                        Cmd::FetchEfforts { .. } => {
                            let efforts = surface
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .efforts
                                .clone();
                            let _ = bus.send(AppEvent::Ctl(CtlEvent::Efforts {
                                efforts: if efforts.is_empty() {
                                    vec!["off".into(), "high".into(), "max".into()]
                                } else {
                                    efforts.clone()
                                },
                                default: efforts.first().cloned(),
                            }));
                        }
                        Cmd::SetPermission { preset, .. } => {
                            let Some(sid) = session_id.clone() else {
                                continue;
                            };
                            match cx
                                .send_request(SetSessionModeRequest::new(sid.clone(), preset.clone()))
                                .block_task()
                                .await
                            {
                                Ok(_) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                                        "permission → {preset}"
                                    ))));
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = cx
                                        .send_request(SetSessionConfigOptionRequest::new(
                                            sid,
                                            "mode",
                                            SessionConfigOptionValue::value_id(preset.clone()),
                                        ))
                                        .block_task()
                                        .await;
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                                        "permission → {preset} ({err})"
                                    ))));
                                }
                            }
                        }
                        Cmd::SetPreset { preset, .. } => {
                            let Some(sid) = session_id.clone() else {
                                continue;
                            };
                            let config_id = surface
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .composition_id
                                .clone()
                                .unwrap_or_else(|| "agent".into());
                            match cx
                                .send_request(SetSessionConfigOptionRequest::new(
                                    sid.clone(),
                                    config_id,
                                    SessionConfigOptionValue::value_id(preset.clone()),
                                ))
                                .block_task()
                                .await
                            {
                                Ok(response) => {
                                    if let Ok(value) = serde_json::to_value(&response) {
                                        if let Some(options) = value
                                            .get("configOptions")
                                            .or_else(|| value.get("config_options"))
                                        {
                                            if let Ok(mut surface) = surface.lock() {
                                                surface.apply_config_options(options, &bus);
                                            }
                                        }
                                    }
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::PresetSet { preset }));
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                        format!("composition switch failed: {err}"),
                                    )));
                                }
                            }
                        }
                        Cmd::SetConfigOption {
                            config_id, value, ..
                        } => {
                            let Some(sid) = session_id.clone() else {
                                continue;
                            };
                            match cx
                                .send_request(SetSessionConfigOptionRequest::new(
                                    sid.clone(),
                                    config_id,
                                    SessionConfigOptionValue::value_id(value),
                                ))
                                .block_task()
                                .await
                            {
                                Ok(response) => {
                                    if let Ok(value) = serde_json::to_value(&response) {
                                        let _ = apply_config_response(
                                            &value,
                                            &sid,
                                            &surface,
                                            &bus,
                                        );
                                    }
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                        format!("config option switch failed: {err}"),
                                    )));
                                }
                            }
                        }
                        Cmd::Authenticate { method_id, values } => {
                            let method = select_auth_method(&methods, Some(&method_id))
                                .or_else(|| select_auth_method(&methods, None))
                                .cloned();
                            let Some(method) = method else {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                    if method_id.is_empty() {
                                        "no supported ACP auth method is available".into()
                                    } else {
                                        format!(
                                            "ACP auth method is unavailable or not supported: {method_id}"
                                        )
                                    },
                                )));
                                continue;
                            };
                            if method.is_env_prompt() {
                                let vars = method
                                    .vars
                                    .iter()
                                    .map(|v| v.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                    if vars.is_empty() {
                                        format!(
                                            "ACP auth method {} requires credential variables and cannot be started as a sign-in flow.",
                                            method.id
                                        )
                                    } else {
                                        format!(
                                            "ACP auth method {} requires credential variables ({vars}) and cannot be started as a sign-in flow.",
                                            method.id
                                        )
                                    },
                                )));
                                continue;
                            }
                            if method.form && authenticate_meta_from_method(&method, &values).is_none()
                            {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                    "sign-in needs values — /auth <api-key> (gateway: /auth <base-url> <api-key>)"
                                        .into(),
                                )));
                                continue;
                            }
                            let mut req = AuthenticateRequest::new(method.id.clone());
                            if let Some(meta) = authenticate_meta_from_method(&method, &values) {
                                req = req.meta(meta);
                            }
                            match cx.send_request(req).block_task().await {
                                Ok(_) => {
                                    if session_id.is_none() {
                                        match create_prompt_session(
                                            &cx,
                                            &cwd,
                                            &surface,
                                            &bus,
                                            &methods,
                                            Some(&method),
                                        )
                                        .await
                                        {
                                            Ok(Some(sid)) => {
                                                session_id = Some(sid);
                                                session_auth_pending = false;
                                            }
                                            Ok(None) => {
                                                session_auth_pending = true;
                                                continue;
                                            }
                                            Err(err) => {
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(
                                                    format!("session/new after authenticate: {err}"),
                                                )));
                                                continue;
                                            }
                                        }
                                    }
                                    emit_auth(
                                        &bus,
                                        configured_snapshot(methods.clone(), Some(&method)),
                                    );
                                            match parked.take() {
                                        None => {
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(
                                                "signed in".into(),
                                            )));
                                        }
                                        Some(prompt) => {
                                            let Some(sid) = session_id.clone() else {
                                                parked = Some(prompt);
                                                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(
                                                    "signed in — resend from the composer".into(),
                                                )));
                                                continue;
                                            };
                                            prompt_queue.push_front(match prompt {
                                                ParkedPrompt::Text(text) => Cmd::Prompt {
                                                    session_id: sid.to_string(),
                                                    text,
                                                },
                                                ParkedPrompt::Images(blocks) => Cmd::PromptImages {
                                                    session_id: sid.to_string(),
                                                    blocks,
                                                },
                                            });
                                            let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(
                                                "signed in — retried the parked prompt".into(),
                                            )));
                                        }
                                    }
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_auth(
                                        &bus,
                                        needs_auth_snapshot(
                                            methods.clone(),
                                            Some(&method),
                                            Some(acp_error_message(&err)),
                                        ),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                        "authenticate: {err}"
                                    ))));
                                }
                            }
                        }
                        Cmd::NewSession => {
                            match cx
                                .send_request(NewSessionRequest::new(cwd.clone()))
                                .block_task()
                                .await
                            {
                                Ok(created) => {
                                    session_id = Some(created.session_id.clone());
                                    session_auth_pending = false;
                                    apply_created(
                                        &created,
                                        &surface,
                                        &bus,
                                        Some(format!(
                                            "new session · {} — /agent picks its agent preset",
                                            created.session_id
                                        )),
                                    );
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                        "session/new: {err}"
                                    ))));
                                }
                            }
                        }
                        Cmd::ListSessions { prefix } => {
                            if !load_session {
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionListUnavailable {
                                    prefix,
                                    error: "agent did not advertise loadSession".into(),
                                }));
                                continue;
                            }
                            match cx
                                .send_request(ListSessionsRequest::new().cwd(cwd.clone()))
                                .block_task()
                                .await
                            {
                                Ok(listed) => {
                                    let sessions = listed
                                        .sessions
                                        .into_iter()
                                        .map(|s| SessionListItem {
                                            id: s.session_id.to_string(),
                                            title: s.title,
                                            updated_at: s.updated_at,
                                        })
                                        .collect();
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionList {
                                        sessions,
                                        prefix,
                                    }));
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionListUnavailable {
                                        prefix,
                                        error: err.to_string(),
                                    }));
                                }
                            }
                        }
                        Cmd::LoadSession { session_id: id } => {
                            let sid = SessionId::new(id.clone());
                            match cx
                                .send_request(LoadSessionRequest::new(sid.clone(), cwd.clone()))
                                .block_task()
                                .await
                            {
                                Ok(loaded) => {
                                    session_id = Some(sid.clone());
                                    if let Ok(value) = serde_json::to_value(&loaded) {
                                        let session = sid.to_string();
                                        apply_setup(&value, Some(&session), &surface, &bus);
                                    }
                                    emit_session_bound(
                                        &bus,
                                        &sid,
                                        Some(format!(
                                            "⟲ loaded {id} — transcript from session/update"
                                        )),
                                    );
                                }
                                Err(err) if is_auth_required_error(&err) => {
                                    emit_needs_auth_open(
                                        &bus,
                                        methods.clone(),
                                        selected.as_ref(),
                                        Some(acp_error_message(&err)),
                                    );
                                }
                                Err(err) => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                        "session/load: {err}"
                                    ))));
                                }
                            }
                        }
                        Cmd::Shutdown => break,
                    }
                            if inflight.is_none() && parked.is_none() {
                                if let Some(next) = prompt_queue.pop_front() {
                                    inflight = begin_prompt(
                                        next,
                                        &cx,
                                        &bus,
                                        &session_id,
                                        &mut parked,
                                        &methods,
                                        selected.as_ref(),
                                        &surface,
                                        &cfg.workspace,
                                    );
                                }
                            }
                        }
                        steer = steer_done_rx.recv() => {
                            if let Some(SteerFinish { message_id, fallback, result }) = steer {
                                let deferred = result.is_err();
                                if deferred {
                                    // Backchat delivery degradation: a rejected immediate
                                    // steer becomes an ordinary turn-end FIFO prompt. The
                                    // active request and its UI lifecycle stay untouched.
                                    prompt_queue.push_back(fallback);
                                }
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::SteerSettled {
                                    message_id,
                                    deferred,
                                }));
                            }
                            if inflight.is_none() && parked.is_none() {
                                if let Some(next) = prompt_queue.pop_front() {
                                    inflight = begin_prompt(
                                        next,
                                        &cx,
                                        &bus,
                                        &session_id,
                                        &mut parked,
                                        &methods,
                                        selected.as_ref(),
                                        &surface,
                                        &cfg.workspace,
                                    );
                                }
                            }
                        }
                        finish = async {
                            match &mut inflight {
                                Some(h) => Some(h.await),
                                None => std::future::pending().await,
                            }
                        } => {
                            inflight = None;
                            let aborted = std::mem::take(&mut turn_aborted);
                            if aborted {
                                if let Some(sid) = &session_id {
                                    let _ = bus.send(AppEvent::Ui(
                                        crate::events::UiEvent::TurnEnd {
                                            session: sid.to_string(),
                                            kind: "interrupted".into(),
                                        },
                                    ));
                                }
                                let _ = bus.send(AppEvent::Ctl(CtlEvent::Interrupted));
                            }
                            match finish {
                                Some(Ok(done)) if !aborted => apply_prompt_finish(
                                    done,
                                    &mut parked,
                                    &bus,
                                    &methods,
                                    selected.as_ref(),
                                ),
                                Some(Ok(_)) => {}
                                Some(Err(err)) if !aborted => {
                                    let _ = bus.send(AppEvent::Ctl(CtlEvent::Error(format!(
                                        "prompt task: {err}"
                                    ))));
                                }
                                Some(Err(_)) | None => {}
                            }
                            if parked.is_none() {
                                if let Some(next) = prompt_queue.pop_front() {
                                    inflight = begin_prompt(
                                        next,
                                        &cx,
                                        &bus,
                                        &session_id,
                                        &mut parked,
                                        &methods,
                                        selected.as_ref(),
                                        &surface,
                                        &cfg.workspace,
                                    );
                                } else if let Some(sid) = &session_id {
                                    let _ = bus.send(AppEvent::Rpc {
                                        method: "session.status".into(),
                                        params: json!({
                                            "sessionId": sid.to_string(),
                                            "status": "idle"
                                        }),
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
        })
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_plugin_inventory_uses_the_backend_current_package_and_run_state() {
        let plugins = dynamic_plugins_from_value(&json!([
            {
                "pluginId": "panel-1",
                "currentPackageId": "pkg-current",
                "activeRun": { "pluginRunId": "run-1", "packageId": "pkg-current" },
                "packages": [
                    { "packageId": "pkg-old", "name": "Old panel" },
                    { "packageId": "pkg-current", "name": "Current panel" }
                ]
            },
            {
                "pluginId": "theme-1",
                "currentPackageId": "pkg-theme",
                "packages": [{ "packageId": "pkg-theme", "name": "Clay theme" }]
            }
        ]))
        .expect("valid backend inventory");

        assert_eq!(
            plugins,
            vec![
                crate::bus::CordisPluginItem {
                    id: "panel-1".into(),
                    name: "Current panel".into(),
                    package_id: "pkg-current".into(),
                    status: "running".into(),
                    approval_request_id: None,
                },
                crate::bus::CordisPluginItem {
                    id: "theme-1".into(),
                    name: "Clay theme".into(),
                    package_id: "pkg-theme".into(),
                    status: "stopped".into(),
                    approval_request_id: None,
                },
            ]
        );
    }

    #[test]
    fn static_plugin_inventory_parses_the_web_snapshot_shape() {
        let plugins = static_plugins_from_value(&json!({
            "entries": [
                {
                    "entryId": "root/include",
                    "moduleName": "include",
                    "enabled": true,
                    "fiberPhase": "active"
                },
                {
                    "entryId": "root/hmr",
                    "moduleName": "hmr",
                    "enabled": false,
                    "fiberPhase": null
                }
            ]
        }))
        .expect("valid static inventory");

        assert_eq!(
            plugins,
            vec![
                crate::bus::StaticPluginItem {
                    entry_id: "root/include".into(),
                    module_name: "include".into(),
                    enabled: true,
                    fiber_phase: Some("active".into()),
                },
                crate::bus::StaticPluginItem {
                    entry_id: "root/hmr".into(),
                    module_name: "hmr".into(),
                    enabled: false,
                    fiber_phase: None,
                },
            ]
        );
    }

    #[test]
    fn session_setup_emits_initial_collaboration_mode_fact() {
        let surface = Arc::new(Mutex::new(Surface::default()));
        let (tx, rx) = std::sync::mpsc::channel();

        apply_setup(
            &json!({
                "sessionId": "s1",
                "configOptions": [{
                    "type": "select",
                    "id": "collaboration_mode",
                    "currentValue": "plan",
                    "options": [
                        {"value": "default", "name": "Default"},
                        {"value": "plan", "name": "Plan"}
                    ]
                }]
            }),
            None,
            &surface,
            &tx,
        );

        assert!(rx.try_iter().any(|event| matches!(
            event,
            AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active })
                if session == "s1" && active
        )));
    }

    #[test]
    fn prompt_response_usage_reaches_the_ui() {
        let response = serde_json::from_value(json!({
            "stopReason": "end_turn",
            "usage": {
                "totalTokens": 135,
                "inputTokens": 100,
                "outputTokens": 20,
                "thoughtTokens": 5,
                "cachedReadTokens": 10
            }
        }))
        .expect("standard ACP prompt response");
        let finish = PromptFinish {
            session_id: "s".into(),
            result: Ok(response),
            parked_on_auth: ParkedPrompt::Text("hello".into()),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parked = Some(ParkedPrompt::Text("hello".into()));

        apply_prompt_finish(finish, &mut parked, &tx, &[], None);

        match rx.try_recv().expect("standard usage UI event") {
            AppEvent::Ui(crate::events::UiEvent::Usage {
                session,
                input,
                output,
                cached,
                reasoning,
            }) => {
                assert_eq!(session, "s");
                assert_eq!((input, output, cached, reasoning), (100, 20, 10, 5));
            }
            _ => panic!("expected standard usage UI event"),
        }
        match rx.try_recv().expect("prompt completion UI event") {
            AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }) => {
                assert_eq!(session, "s");
                assert_eq!(kind, "completed");
            }
            _ => panic!("expected prompt completion UI event"),
        }
    }

    #[test]
    fn prompt_error_ends_the_ui_turn_before_reporting_the_error() {
        let finish = PromptFinish {
            session_id: "s".into(),
            result: Err(AcpError::new(-32603, "boom")),
            parked_on_auth: ParkedPrompt::Text("hello".into()),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let mut parked = None;

        apply_prompt_finish(finish, &mut parked, &tx, &[], None);

        assert!(matches!(
            rx.try_recv(),
            Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
                if session == "s" && kind == "error"
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(AppEvent::Ctl(CtlEvent::Error(_)))
        ));
    }

    #[test]
    fn initialize_advertises_backchat_auth_caps() {
        let value = serde_json::to_value(initialize_request()).expect("serialize");
        let caps = &value["clientCapabilities"];
        assert_eq!(caps["_meta"]["terminal-auth"], true);
        assert_eq!(caps["_meta"]["terminal_output"], true);
        assert_eq!(caps["_meta"]["subagent-transcript"], true);
        assert_eq!(caps["_meta"]["dsh"]["cordis"]["protocol"], 0);
        assert_eq!(caps["auth"]["terminal"], true);
        assert_eq!(caps["auth"]["_meta"]["gateway"], true);
        assert_eq!(caps["session"]["configOptions"], json!({}));
        assert_eq!(caps["elicitation"]["form"], json!({}));
        assert_eq!(caps["fs"]["readTextFile"], true);
        assert_eq!(caps["fs"]["writeTextFile"], true);
        assert_eq!(caps["terminal"], true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_ui_events_are_compositor_notifications_not_prompts() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse,
        };
        use std::time::Duration;

        let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
        let request_tx = extension_tx.clone();
        let agent = Agent
            .builder()
            .name("plugin-command-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    let mut meta = serde_json::Map::new();
                    meta.insert(
                        "dsh".into(),
                        json!({ "cordis": { "protocol": crate::cordis::PROTOCOL } }),
                    );
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new().meta(meta))
                            .agent_info(Implementation::new("plugin-command-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
                    cx.send_notification(UntypedMessage::new(
                        crate::cordis::COMMANDS_UPDATE,
                        json!({ "protocol": 0, "commands": [] }),
                    )?)?;
                    cx.send_notification(UntypedMessage::new(
                        crate::cordis::OVERLAY_UPDATE,
                        json!({ "protocol": 0, "overlay": null }),
                    )?)
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |request: UntypedMessage, responder, _cx| {
                    if matches!(
                        request.method(),
                        crate::cordis::COMMAND_INVOKE
                            | crate::cordis::THEME_SELECTED
                            | crate::cordis::OVERLAY_EVENT
                    ) {
                        let _ = request_tx.send((
                            "request",
                            request.method().to_string(),
                            request.params().clone(),
                        ));
                    }
                    responder.respond(json!({ "handled": true }))
                },
                on_receive_request!(),
            )
            .on_receive_notification(
                async move |notification: UntypedMessage, _cx| {
                    if notification.method() == crate::cordis::OVERLAY_EVENT {
                        let _ = extension_tx.send((
                            "notification",
                            notification.method().to_string(),
                            notification.params().clone(),
                        ));
                    }
                    Ok(())
                },
                on_receive_notification!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut compositor_methods = std::collections::HashSet::new();
        while std::time::Instant::now() < deadline && compositor_methods.len() < 2 {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Rpc { method, .. })
                    if matches!(
                        method.as_str(),
                        crate::cordis::COMMANDS_UPDATE | crate::cordis::OVERLAY_UPDATE
                    ) =>
                {
                    compositor_methods.insert(method);
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("bus disconnected: {error}"),
            }
        }
        assert_eq!(
            compositor_methods,
            std::collections::HashSet::from([
                crate::cordis::COMMANDS_UPDATE.to_string(),
                crate::cordis::OVERLAY_UPDATE.to_string(),
            ])
        );

        cmd_tx
            .send(Cmd::InvokePluginCommand {
                name: "threshold".into(),
                args: String::new(),
            })
            .expect("invoke plugin command");

        let (kind, method, params) =
            tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
                .await
                .expect("command request should reach the compositor plane")
                .expect("extension channel");
        assert_eq!(kind, "request");
        assert_eq!(method, crate::cordis::COMMAND_INVOKE);
        assert_eq!(
            params,
            json!({
                "protocol": 0,
                "name": "threshold",
                "args": ""
            })
        );

        cmd_tx
            .send(Cmd::PluginThemeSelected {
                agent_id: "agent-1".into(),
                id: "night-lime".into(),
            })
            .expect("select theme Plugin");
        let (kind, method, params) =
            tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
                .await
                .expect("theme Plugin selection should reach the compositor plane")
                .expect("extension channel");
        assert_eq!(kind, "request");
        assert_eq!(method, crate::cordis::THEME_SELECTED);
        assert_eq!(
            params,
            json!({
                "protocol": 0,
                "agentId": "agent-1",
                "id": "night-lime"
            })
        );

        cmd_tx
            .send(Cmd::PluginOverlayEvent {
                id: "threshold".into(),
                event: "change".into(),
                value: Some(json!(16.0)),
            })
            .expect("send slider event");
        let (kind, method, params) =
            tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
                .await
                .expect("overlay event should reach the compositor plane")
                .expect("extension channel");
        assert_eq!(kind, "notification");
        assert_eq!(method, crate::cordis::OVERLAY_EVENT);
        assert_eq!(
            params,
            json!({
                "protocol": 0,
                "id": "threshold",
                "event": "change",
                "value": 16.0
            })
        );
        for event in ["submit", "cancel"] {
            cmd_tx
                .send(Cmd::PluginOverlayEvent {
                    id: "threshold".into(),
                    event: event.into(),
                    value: Some(json!(32.0)),
                })
                .expect("send terminal slider event");
            let (kind, method, params) =
                tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
                    .await
                    .expect("terminal overlay event should reach the compositor plane")
                    .expect("extension channel");
            assert_eq!(kind, "request");
            assert_eq!(method, crate::cordis::OVERLAY_EVENT);
            assert_eq!(params["event"], event);
        }
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cordis_requests_stay_local_when_the_agent_did_not_advertise_cordis() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse,
        };
        use std::time::Duration;

        let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
        let agent = Agent
            .builder()
            .name("standard-acp-agent")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("standard-acp-agent", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |request: UntypedMessage, responder, _cx| {
                    let _ = extension_tx.send(request.method().to_string());
                    responder.respond(json!({ "handled": true }))
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => break,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("bus disconnected: {error}"),
            }
        }
        cmd_tx
            .send(Cmd::InvokePluginCommand {
                name: "threshold".into(),
                args: String::new(),
            })
            .expect("invoke plugin command");

        assert!(
            tokio::time::timeout(Duration::from_millis(100), extension_rx.recv())
                .await
                .is_err(),
            "a standard ACP agent must not receive an unadvertised Cordis request",
        );
        assert!(
            (0..20).any(|_| matches!(
                bus_rx.recv_timeout(Duration::from_millis(20)),
                Ok(AppEvent::Ctl(CtlEvent::TuiOpFailed(message)))
                    if message.contains("does not advertise _dsh/cordis")
            )),
            "the client should report that the Cordis capability is unavailable",
        );

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_compositor_catalog_does_not_require_agent_cordis_capability() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse,
        };
        use std::time::Duration;

        let agent = Agent
            .builder()
            .name("standard-acp-agent")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("standard-acp-agent", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
                    cx.send_notification(UntypedMessage::new(
                        crate::cordis::COMMANDS_UPDATE,
                        json!({
                            "protocol": 0,
                            "commands": [{
                                "name": "ui",
                                "description": "Switch UI preset"
                            }]
                        }),
                    )?)
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut catalog = None;
        while std::time::Instant::now() < deadline && catalog.is_none() {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Rpc { method, params })
                    if method == crate::cordis::COMMANDS_UPDATE =>
                {
                    catalog = Some(params);
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(error) => panic!("bus disconnected: {error}"),
            }
        }
        assert_eq!(catalog.unwrap()["commands"][0]["name"], "ui");

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[test]
    fn prompt_image_flag_reads_initialize_payload() {
        assert!(prompt_image_supported(&json!({
            "agentCapabilities": { "promptCapabilities": { "image": true } }
        })));
        assert!(prompt_image_supported(&json!({
            "agentCapabilities": { "promptCapabilities": { "image": {} } }
        })));
        assert!(prompt_image_supported(&json!({
            "agentCapabilities": { "prompt": { "image": {} } }
        })));
        assert!(!prompt_image_supported(&json!({
            "agentCapabilities": { "promptCapabilities": { "image": false } }
        })));
        assert!(!prompt_image_supported(&json!({})));
    }

    #[test]
    fn image_prompt_uses_image_blocks_when_advertised() {
        let blocks = vec![
            crate::bus::PromptBlock::Text("see".into()),
            crate::bus::PromptBlock::Image(crate::bus::ImagePart {
                data: "AQ==".into(),
                media_type: "image/png".into(),
                name: "shot.png".into(),
                path: "/tmp/shot.png".into(),
            }),
            crate::bus::PromptBlock::Text("that".into()),
        ];
        let content = prompt_content_blocks(blocks, true, "/w").expect("blocks");
        assert_eq!(content.len(), 3);
        assert!(matches!(&content[0], ContentBlock::Text(t) if t.text == "see"));
        match &content[1] {
            ContentBlock::Image(image) => {
                assert_eq!(image.data, "AQ==");
                assert_eq!(image.mime_type, "image/png");
                assert_eq!(image.uri.as_deref(), Some("file:///tmp/shot.png"));
            }
            other => panic!("expected image, got {other:?}"),
        }
        assert!(matches!(&content[2], ContentBlock::Text(t) if t.text == "that"));
    }

    #[test]
    fn image_prompt_clipboard_image_has_bytes_only() {
        let blocks = vec![crate::bus::PromptBlock::Image(crate::bus::ImagePart {
            data: "AQ==".into(),
            media_type: "image/png".into(),
            name: "clipboard.png".into(),
            path: "clipboard".into(),
        })];
        let content = prompt_content_blocks(blocks, true, "/w").expect("blocks");
        assert_eq!(content.len(), 1);
        match &content[0] {
            ContentBlock::Image(image) => {
                assert_eq!(image.data, "AQ==");
                assert_eq!(image.mime_type, "image/png");
                assert_eq!(image.uri, None);
            }
            other => panic!("expected image, got {other:?}"),
        }
    }

    #[test]
    fn image_prompt_falls_back_to_resource_link() {
        let blocks = vec![crate::bus::PromptBlock::Image(crate::bus::ImagePart {
            data: crate::pet::base64(&[1, 2, 3, 4]),
            media_type: "image/png".into(),
            name: "clip.png".into(),
            path: "clipboard".into(),
        })];
        let content = prompt_content_blocks(blocks, false, "/w").expect("spill");
        assert_eq!(content.len(), 1);
        match &content[0] {
            ContentBlock::ResourceLink(link) => {
                assert_eq!(link.name, "clip.png");
                assert_eq!(link.mime_type.as_deref(), Some("image/png"));
                assert!(
                    link.uri.starts_with("file:///"),
                    "spilled clipboard uri: {}",
                    link.uri
                );
                let spilled = std::path::Path::new(link.uri.strip_prefix("file://").unwrap());
                assert!(spilled.is_absolute(), "spilled path: {}", link.uri);
                assert!(spilled.exists(), "spill missing: {}", link.uri);
            }
            other => panic!("expected resource link, got {other:?}"),
        }
    }

    #[test]
    fn resource_link_uses_absolute_file_uri_for_disk_paths() {
        let blocks = vec![
            crate::bus::PromptBlock::Text("see".into()),
            crate::bus::PromptBlock::Image(crate::bus::ImagePart {
                data: "AQ==".into(),
                media_type: "image/png".into(),
                name: "shot.png".into(),
                path: "shots/./a.png".into(),
            }),
            crate::bus::PromptBlock::Text("that".into()),
        ];
        let content = prompt_content_blocks(blocks, false, "/w").expect("blocks");
        assert_eq!(content.len(), 3);
        assert!(matches!(&content[0], ContentBlock::Text(t) if t.text == "see"));
        match &content[1] {
            ContentBlock::ResourceLink(link) => {
                assert_eq!(link.uri, "file:///w/shots/a.png");
                assert_eq!(link.mime_type.as_deref(), Some("image/png"));
            }
            other => panic!("expected resource link, got {other:?}"),
        }
        assert!(matches!(&content[2], ContentBlock::Text(t) if t.text == "that"));
    }

    #[test]
    fn load_session_flag_reads_initialize_payload() {
        assert!(load_session_supported(&json!({
            "agentCapabilities": { "loadSession": true }
        })));
        assert!(load_session_supported(&json!({
            "agent_capabilities": { "load_session": true }
        })));
        assert!(!load_session_supported(&json!({
            "agentCapabilities": { "loadSession": false }
        })));
        assert!(!load_session_supported(&json!({})));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn form_auth_stays_configured_when_the_startup_session_succeeds() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeResponse, NewSessionResponse,
        };
        use std::time::{Duration, Instant};

        let mut meta = serde_json::Map::new();
        meta.insert("api-key".into(), json!({ "provider": "deepseek" }));
        let agent = Agent
            .builder()
            .name("already-authenticated-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("already-authenticated-mock", "0"))
                            .auth_methods(vec![AuthMethod::Agent(
                                AuthMethodAgent::new("api-key", "DeepSeek API key")
                                    .meta(meta.clone()),
                            )]),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut auth_status = None;
        let mut opened_auth = false;
        while Instant::now() < deadline && auth_status.is_none() {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => {
                    auth_status = Some(snapshot.status);
                }
                Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => opened_auth = true,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        while let Ok(event) = bus_rx.try_recv() {
            if matches!(event, AppEvent::Ctl(CtlEvent::OpenAuth)) {
                opened_auth = true;
            }
        }

        assert_eq!(auth_status, Some(AuthStatus::Configured));
        assert!(
            !opened_auth,
            "initialize alone must not open sign-in for possibly persisted credentials"
        );
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn elicitation_create_waits_for_the_tui_form_reply() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, CreateElicitationRequest, ElicitationAction, InitializeResponse,
            NewSessionResponse,
        };
        use std::time::{Duration, Instant};

        let request: CreateElicitationRequest = serde_json::from_value(json!({
            "mode": "form",
            "sessionId": "s1",
            "message": "The agent needs your input.",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "Target",
                        "oneOf": [
                            { "const": "local", "title": "Local" },
                            { "const": "remote", "title": "Remote" }
                        ]
                    }
                },
                "required": ["question_0"]
            }
        }))
        .expect("standard form request");
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let result_tx = Arc::new(Mutex::new(Some(result_tx)));
        let agent = Agent
            .builder()
            .name("elicitation-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("elicitation-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let result_tx = Arc::clone(&result_tx);
                    async move |_req: NewSessionRequest, responder, cx| {
                        responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
                        let request = request.clone();
                        let tx = result_tx.lock().unwrap().take();
                        let request_cx = cx.clone();
                        cx.spawn(async move {
                            let result = request_cx.send_request(request).block_task().await;
                            if let Some(tx) = tx {
                                let _ = tx.send(result);
                            }
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
        cmd_tx.send(Cmd::NewSession).expect("explicit new session");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut form_reply = None;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::ElicitationAsk { form, reply }) => {
                    assert_eq!(form.fields[0].title, "Target");
                    form_reply = Some(reply);
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(
            form_reply.is_some(),
            "elicitation/create must open the TUI form"
        );
        let _ = form_reply
            .unwrap()
            .send(crate::elicitation::ElicitationReply::Accepted(
                std::collections::BTreeMap::from([(
                    "question_0".into(),
                    crate::elicitation::ElicitationValue::String("remote".into()),
                )]),
            ));

        let response = tokio::time::timeout(Duration::from_secs(2), result_rx)
            .await
            .expect("client should answer elicitation")
            .expect("result channel")
            .expect("ACP response");
        let ElicitationAction::Accept(accepted) = response.action else {
            panic!("expected accepted form response")
        };
        assert!(matches!(
            accepted.content.as_ref().and_then(|content| content.get("question_0")),
            Some(agent_client_protocol::schema::v1::ElicitationContentValue::String(value))
                if value == "remote"
        ));
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_session_applies_initial_config_before_binding() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
            SessionConfigSelectOption,
        };
        use std::time::{Duration, Instant};

        let agent = Agent
            .builder()
            .name("setup-order-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("setup-order-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(
                        NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                            SessionConfigOption::select(
                                "collaboration_mode",
                                "Collaboration mode",
                                "plan",
                                vec![
                                    SessionConfigSelectOption::new("default", "Default"),
                                    SessionConfigSelectOption::new("plan", "Plan"),
                                ],
                            ),
                        ]),
                    )
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
        cmd_tx.send(Cmd::NewSession).expect("explicit new session");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut order = Vec::new();
        while Instant::now() < deadline && order.len() < 2 {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }))
                    if session_id == "s1" =>
                {
                    order.push("bound");
                }
                Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                    if session == "s1" && active =>
                {
                    order.push("plan");
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        assert_eq!(order, ["plan", "bound"]);
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_config_option_response_updates_client_state_without_a_notification() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
            SessionConfigSelectOption, SetSessionConfigOptionResponse,
        };
        use std::time::{Duration, Instant};

        let agent = Agent
            .builder()
            .name("config-response-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("config-response-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: SetSessionConfigOptionRequest, responder, _cx| {
                    responder.respond(SetSessionConfigOptionResponse::new(vec![
                        SessionConfigOption::select(
                            "collaboration_mode",
                            "Collaboration mode",
                            "plan",
                            vec![
                                SessionConfigSelectOption::new("default", "Default"),
                                SessionConfigSelectOption::new("plan", "Plan"),
                            ],
                        ),
                    ]))
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
        cmd_tx.send(Cmd::NewSession).expect("explicit new session");

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }))
                    if session_id == "s1" =>
                {
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        cmd_tx
            .send(Cmd::SetConfigOption {
                config_id: "collaboration_mode".into(),
                value: "plan".into(),
            })
            .expect("set config option");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_plan = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                    if session == "s1" && active =>
                {
                    saw_plan = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        assert!(
            saw_plan,
            "the successful response is the authoritative state snapshot"
        );
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_tree_config_set_uses_standard_acp_and_folds_response_only_state() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
            SessionConfigSelectOption, SetSessionConfigOptionResponse,
        };
        use std::time::{Duration, Instant};

        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();
        let agent = Agent
            .builder()
            .name("client-config-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("client-config-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, cx| {
                    responder.respond(
                        NewSessionResponse::new(SessionId::new("s-client")).config_options(vec![
                            SessionConfigOption::select(
                                "collaboration_mode",
                                "Collaboration mode",
                                "default",
                                vec![
                                    SessionConfigSelectOption::new("default", "Default"),
                                    SessionConfigSelectOption::new("plan", "Plan"),
                                ],
                            ),
                        ]),
                    )?;
                    let result_tx = result_tx.clone();
                    tokio::spawn(async move {
                        let request = UntypedMessage::new(
                            crate::cordis::SESSION_CONFIG_SET,
                            json!({
                                "protocol": crate::cordis::PROTOCOL,
                                "sessionId": "s-client",
                                "configId": "collaboration_mode",
                                "value": "plan",
                            }),
                        )
                        .expect("valid Client compositor request");
                        let result = cx.send_request(request).block_task().await;
                        let _ = result_tx.send(result);
                    });
                    Ok(())
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                    assert_eq!(req.session_id.to_string(), "s-client");
                    assert_eq!(req.config_id.to_string(), "collaboration_mode");
                    assert_eq!(
                        req.value.as_value_id().map(ToString::to_string).as_deref(),
                        Some("plan")
                    );
                    responder.respond(SetSessionConfigOptionResponse::new(vec![
                        SessionConfigOption::select(
                            "collaboration_mode",
                            "Collaboration mode",
                            "plan",
                            vec![
                                SessionConfigSelectOption::new("default", "Default"),
                                SessionConfigSelectOption::new("plan", "Plan"),
                            ],
                        ),
                    ]))
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let result = tokio::time::timeout(Duration::from_secs(2), result_rx.recv())
            .await
            .expect("Client config request settled")
            .expect("Client config response exists")
            .expect("Client config request succeeded");
        assert_eq!(
            result["configOptions"][0]["currentValue"],
            json!("plan"),
            "the internal response returns the complete standard ACP snapshot"
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_plan = false;
        while Instant::now() < deadline && !saw_plan {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                    if session == "s-client" && active =>
                {
                    saw_plan = true;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(
            saw_plan,
            "response-only config state reaches the native client"
        );

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn load_session_applies_initial_config_before_binding_the_loaded_id() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeResponse, LoadSessionResponse, NewSessionResponse,
            SessionConfigOption, SessionConfigSelectOption,
        };
        use std::time::{Duration, Instant};

        let agent = Agent
            .builder()
            .name("load-config-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new().load_session(true))
                            .agent_info(Implementation::new("load-config-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: LoadSessionRequest, responder, _cx| {
                    responder.respond(LoadSessionResponse::new().config_options(vec![
                        SessionConfigOption::select(
                            "collaboration_mode",
                            "Collaboration mode",
                            "plan",
                            vec![
                                SessionConfigSelectOption::new("default", "Default"),
                                SessionConfigSelectOption::new("plan", "Plan"),
                            ],
                        ),
                    ]))
                },
                on_receive_request!(),
            );
        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }))
                    if session_id == "s1" =>
                {
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        cmd_tx
            .send(Cmd::LoadSession {
                session_id: "s2".into(),
            })
            .expect("load session");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut order = Vec::new();
        while Instant::now() < deadline && order.len() < 2 {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }))
                    if session_id == "s2" =>
                {
                    order.push("bound");
                }
                Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                    if session == "s2" && active =>
                {
                    order.push("plan");
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        assert_eq!(order, ["plan", "bound"]);
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn prompts_while_running_wait_in_fifo_without_session_cancel() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
            NewSessionResponse, PromptResponse, StopReason,
        };
        use std::time::{Duration, Instant};

        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
        let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));

        let agent = Agent
            .builder()
            .name("queue-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("queue-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let release_first_rx = Arc::clone(&release_first_rx);
                    async move |req: PromptRequest, responder, cx| {
                        let text = req
                            .prompt
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let _ = prompt_tx.send(text);
                        let release = release_first_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                on_receive_request!(),
            )
            .on_receive_notification(
                async move |_note: CancelNotification, _cx| {
                    let _ = cancel_tx.send(());
                    Ok(())
                },
                on_receive_notification!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        for text in ["first", "second", "third"] {
            cmd_tx
                .send(Cmd::Prompt {
                    session_id: "s1".into(),
                    text: text.into(),
                })
                .expect("prompt");
        }

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("first prompt reaches the agent")
                .as_deref(),
            Some("first")
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            prompt_rx.try_recv().is_err(),
            "queued prompts must not reach the agent before the active prompt settles"
        );
        assert!(
            cancel_rx.try_recv().is_err(),
            "queueing a prompt must never send session/cancel"
        );
        while bus_rx.try_recv().is_ok() {}

        let _ = release_first_tx.send(());
        let second = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("second prompt reaches the agent")
            .expect("second prompt");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut idle_between_prompts = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Rpc { method, params }) if method == "session.status" => {
                    if params.get("status").and_then(serde_json::Value::as_str) == Some("idle") {
                        idle_between_prompts = true;
                    }
                }
                Ok(AppEvent::Ctl(CtlEvent::PromptQueued { .. })) => break,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(
            !idle_between_prompts,
            "the client stays running while a FIFO follow-up is ready"
        );
        let third = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("third prompt reaches the agent")
            .expect("third prompt");
        assert_eq!([second.as_str(), third.as_str()], ["second", "third"]);

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn composition_catalog_is_ready_before_the_first_prompt() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
            NewSessionResponse, PromptResponse, SessionConfigOption, SessionConfigSelectOption,
            StopReason,
        };
        use std::time::{Duration, Instant};

        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let agent = Agent
            .builder()
            .name("startup-session-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("startup-session-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(
                        NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                            SessionConfigOption::select(
                                "agent",
                                "Agent",
                                "standard",
                                vec![
                                    SessionConfigSelectOption::new("standard", "Standard"),
                                    SessionConfigSelectOption::new("creator", "Creator"),
                                ],
                            ),
                        ]),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest, responder, _cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text);
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                },
                on_receive_request!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut ready = false;
        let mut catalog = None;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Catalog { presets, .. })) if !presets.is_empty() => {
                    catalog = Some(
                        presets
                            .into_iter()
                            .map(|preset| preset.id)
                            .collect::<Vec<_>>(),
                    );
                }
                Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => {
                    ready = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(ready, "initialize completes during startup");
        assert_eq!(catalog, Some(vec!["standard".into(), "creator".into()]));
        assert!(prompt_rx.try_recv().is_err(), "session/new is not a turn");

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auth_failure_parks_the_active_prompt_and_preserves_followup_fifo() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse,
            InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
            PromptResponse, StopReason,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        let rejected_first = Arc::new(AtomicBool::new(false));
        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let agent = Agent
            .builder()
            .name("auth-queue-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("auth-queue-mock", "0"))
                            .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                                "login", "Login",
                            ))]),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: AuthenticateRequest, responder, _cx| {
                    responder.respond(AuthenticateResponse::new())
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let rejected_first = Arc::clone(&rejected_first);
                    async move |req: PromptRequest, responder, _cx| {
                        let text = req
                            .prompt
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let _ = prompt_tx.send(text.clone());
                        if text == "first" && !rejected_first.swap(true, Ordering::SeqCst) {
                            responder.respond_with_error(AcpError::new(
                                -32000,
                                "authentication required",
                            ))
                        } else {
                            responder.respond(PromptResponse::new(StopReason::EndTurn))
                        }
                    }
                },
                on_receive_request!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        for text in ["first", "second"] {
            cmd_tx
                .send(Cmd::Prompt {
                    session_id: "local-draft".into(),
                    text: text.into(),
                })
                .expect("prompt");
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("first prompt reaches the agent")
                .as_deref(),
            Some("first")
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut needs_auth = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                    if snapshot.status == AuthStatus::NeedsAuth =>
                {
                    needs_auth = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(needs_auth, "the active prompt requests sign-in");
        cmd_tx
            .send(Cmd::Prompt {
                session_id: "local-draft".into(),
                text: "third".into(),
            })
            .expect("prompt entered while auth is unresolved");
        cmd_tx
            .send(Cmd::PromptImages {
                session_id: "local-draft".into(),
                blocks: vec![crate::bus::PromptBlock::Text(
                    "image group during auth".into(),
                )],
            })
            .expect("image prompt entered while auth is unresolved");
        cmd_tx
            .send(Cmd::Steer {
                session_id: "local-draft".into(),
                message_id: 7,
                text: "steer during auth".into(),
            })
            .expect("steer entered while auth is unresolved");
        cmd_tx
            .send(Cmd::SteerImages {
                session_id: "local-draft".into(),
                message_id: 8,
                blocks: vec![crate::bus::PromptBlock::Text(
                    "image steer during auth".into(),
                )],
            })
            .expect("image steer entered while auth is unresolved");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut deferred_steers = std::collections::BTreeSet::new();
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SteerSettled {
                    message_id,
                    deferred,
                })) if matches!(message_id, 7 | 8) => {
                    if deferred {
                        deferred_steers.insert(message_id);
                    }
                    if deferred_steers.len() == 2 {
                        break;
                    }
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert_eq!(
            deferred_steers,
            std::collections::BTreeSet::from([7, 8]),
            "text and image Send Now both degrade to FIFO while authentication is unresolved"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            prompt_rx.try_recv().is_err(),
            "followups stay parked while authentication is unresolved"
        );

        cmd_tx
            .send(Cmd::Authenticate {
                method_id: "login".into(),
                values: Default::default(),
            })
            .expect("authenticate");
        let retried = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("parked prompt retries")
            .expect("retry text");
        let followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("followup drains")
            .expect("followup text");
        let later_followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("prompt entered during auth drains last")
            .expect("later followup text");
        let image_followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("image prompt entered during auth drains in order")
            .expect("image followup text");
        let deferred_steer = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("deferred steer drains after ordinary followups")
            .expect("deferred steer text");
        let deferred_image_steer = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("deferred image steer drains last")
            .expect("deferred image steer text");
        assert_eq!(
            [
                retried.as_str(),
                followup.as_str(),
                later_followup.as_str(),
                image_followup.as_str(),
                deferred_steer.as_str(),
                deferred_image_steer.as_str(),
            ],
            [
                "first",
                "second",
                "third",
                "image group during auth",
                "steer during auth",
                "image steer during auth",
            ]
        );

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_new_auth_failure_parks_the_first_intent_without_retry_storms() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse,
            InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
            PromptResponse, StopReason,
        };
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let authenticated = Arc::new(AtomicBool::new(false));
        let new_attempts = Arc::new(AtomicUsize::new(0));
        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let agent = Agent
            .builder()
            .name("new-auth-queue-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("new-auth-queue-mock", "0"))
                            .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                                "login", "Login",
                            ))]),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let authenticated = Arc::clone(&authenticated);
                    let new_attempts = Arc::clone(&new_attempts);
                    async move |_req: NewSessionRequest, responder, _cx| {
                        new_attempts.fetch_add(1, Ordering::SeqCst);
                        if authenticated.load(Ordering::SeqCst) {
                            responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                        } else {
                            responder.respond_with_error(AcpError::new(
                                -32000,
                                "authentication required",
                            ))
                        }
                    }
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let authenticated = Arc::clone(&authenticated);
                    async move |_req: AuthenticateRequest, responder, _cx| {
                        authenticated.store(true, Ordering::SeqCst);
                        responder.respond(AuthenticateResponse::new())
                    }
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest, responder, _cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text);
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                },
                on_receive_request!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "local-draft".into(),
                text: "first".into(),
            })
            .expect("first prompt");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut needs_auth = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                    if snapshot.status == AuthStatus::NeedsAuth =>
                {
                    needs_auth = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(needs_auth, "session/new requests sign-in");

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "local-draft".into(),
                text: "second".into(),
            })
            .expect("followup during auth");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            new_attempts.load(Ordering::SeqCst),
            1,
            "followups must not retry session/new before authentication"
        );
        assert!(prompt_rx.try_recv().is_err());

        cmd_tx
            .send(Cmd::Authenticate {
                method_id: "login".into(),
                values: Default::default(),
            })
            .expect("authenticate");
        let first = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("parked first prompt retries")
            .expect("first text");
        let second = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("followup drains")
            .expect("second text");
        assert_eq!([first.as_str(), second.as_str()], ["first", "second"]);
        assert_eq!(new_attempts.load(Ordering::SeqCst), 2);

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_steer_rejection_waits_behind_a_prompt_parked_for_auth() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse,
            InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
            PromptResponse, StopReason,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_steer_tx, release_steer_rx) = tokio::sync::oneshot::channel::<()>();
        let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));
        let release_steer_rx = Arc::new(Mutex::new(Some(release_steer_rx)));
        let first_rejected = Arc::new(AtomicBool::new(false));
        let steer_rejected = Arc::new(AtomicBool::new(false));

        let agent = Agent
            .builder()
            .name("late-steer-auth-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("late-steer-auth-mock", "0"))
                            .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                                "login", "Login",
                            ))]),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: AuthenticateRequest, responder, _cx| {
                    responder.respond(AuthenticateResponse::new())
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let release_first_rx = Arc::clone(&release_first_rx);
                    let release_steer_rx = Arc::clone(&release_steer_rx);
                    let first_rejected = Arc::clone(&first_rejected);
                    let steer_rejected = Arc::clone(&steer_rejected);
                    async move |req: PromptRequest, responder, cx| {
                        let text = req
                            .prompt
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let _ = prompt_tx.send(text.clone());
                        if text == "first" && !first_rejected.swap(true, Ordering::SeqCst) {
                            let release = release_first_rx.lock().unwrap().take();
                            cx.spawn(async move {
                                if let Some(release) = release {
                                    let _ = release.await;
                                }
                                let _ = responder.respond_with_error(AcpError::new(
                                    -32000,
                                    "authentication required",
                                ));
                                Ok(())
                            })?;
                            return Ok(());
                        }
                        if text == "steer" && !steer_rejected.swap(true, Ordering::SeqCst) {
                            let release = release_steer_rx.lock().unwrap().take();
                            cx.spawn(async move {
                                if let Some(release) = release {
                                    let _ = release.await;
                                }
                                let _ = responder.respond_with_error(AcpError::new(
                                    -32603,
                                    "concurrent prompt rejected",
                                ));
                                Ok(())
                            })?;
                            return Ok(());
                        }
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                },
                on_receive_request!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "local-draft".into(),
                text: "first".into(),
            })
            .expect("first prompt");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("first starts")
                .as_deref(),
            Some("first")
        );
        cmd_tx
            .send(Cmd::Steer {
                session_id: "local-draft".into(),
                message_id: 9,
                text: "steer".into(),
            })
            .expect("concurrent steer");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("steer starts")
                .as_deref(),
            Some("steer")
        );

        let _ = release_first_tx.send(());
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut needs_auth = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                    if snapshot.status == AuthStatus::NeedsAuth =>
                {
                    needs_auth = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(needs_auth, "active prompt parks for auth");

        let _ = release_steer_tx.send(());
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut steer_deferred = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SteerSettled {
                    message_id: 9,
                    deferred,
                })) => {
                    steer_deferred = deferred;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(steer_deferred, "rejected steer becomes FIFO");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            prompt_rx.try_recv().is_err(),
            "late steer fallback cannot bypass unresolved authentication"
        );

        cmd_tx
            .send(Cmd::Authenticate {
                method_id: "login".into(),
                values: Default::default(),
            })
            .expect("authenticate");
        let first = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("parked active retries")
            .expect("first retry text");
        let steer = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("steer fallback drains")
            .expect("steer retry text");
        assert_eq!([first.as_str(), steer.as_str()], ["first", "steer"]);

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn steer_sends_a_concurrent_prompt_without_interrupting_the_turn() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
            NewSessionResponse, PromptResponse, StopReason,
        };
        use std::time::{Duration, Instant};

        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
        let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));

        let agent = Agent
            .builder()
            .name("steer-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("steer-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let release_first_rx = Arc::clone(&release_first_rx);
                    async move |req: PromptRequest, responder, cx| {
                        let text = req
                            .prompt
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let _ = prompt_tx.send(text);
                        let release = release_first_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                on_receive_request!(),
            )
            .on_receive_notification(
                async move |_note: CancelNotification, _cx| {
                    let _ = cancel_tx.send(());
                    Ok(())
                },
                on_receive_notification!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: "first".into(),
            })
            .expect("first prompt");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("first prompt reaches the agent")
                .as_deref(),
            Some("first")
        );

        cmd_tx
            .send(Cmd::Steer {
                session_id: "s1".into(),
                message_id: 1,
                text: "change course".into(),
            })
            .expect("steer");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("steer reaches the active agent")
                .as_deref(),
            Some("change course")
        );
        cmd_tx
            .send(Cmd::SteerImages {
                session_id: "s1".into(),
                message_id: 2,
                blocks: vec![
                    crate::bus::PromptBlock::Text("look here".into()),
                    crate::bus::PromptBlock::Image(crate::bus::ImagePart {
                        data: "AQID".into(),
                        media_type: "image/png".into(),
                        name: "shot.png".into(),
                        path: "clipboard".into(),
                    }),
                ],
            })
            .expect("image steer");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("image steer reaches the active agent")
                .as_deref(),
            Some("look here")
        );
        assert!(
            cancel_rx.try_recv().is_err(),
            "steer must not send session/cancel"
        );
        while let Ok(event) = bus_rx.try_recv() {
            assert!(
                !matches!(event, AppEvent::Ctl(CtlEvent::Interrupted)),
                "steer is part of the active turn, not an interruption"
            );
        }

        let _ = release_first_tx.send(());
        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rejected_steer_falls_back_to_fifo_without_failing_the_active_turn() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
            NewSessionResponse, PromptResponse, StopReason,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
        let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));
        let steer_attempts = Arc::new(AtomicUsize::new(0));

        let agent = Agent
            .builder()
            .name("steer-fallback-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("steer-fallback-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let release_first_rx = Arc::clone(&release_first_rx);
                    let steer_attempts = Arc::clone(&steer_attempts);
                    async move |req: PromptRequest, responder, cx| {
                        let text = req
                            .prompt
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.clone()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        let _ = prompt_tx.send(text.clone());
                        if text == "first" {
                            let release = release_first_rx.lock().unwrap().take();
                            cx.spawn(async move {
                                if let Some(release) = release {
                                    let _ = release.await;
                                }
                                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                                Ok(())
                            })?;
                            return Ok(());
                        }
                        if steer_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                            return responder.respond_with_error(AcpError::new(
                                -32603,
                                "concurrent prompt rejected",
                            ));
                        }
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                },
                on_receive_request!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: "first".into(),
            })
            .expect("first prompt");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("first prompt reaches the agent")
                .as_deref(),
            Some("first")
        );

        cmd_tx
            .send(Cmd::Steer {
                session_id: "s1".into(),
                message_id: 1,
                text: "change course".into(),
            })
            .expect("steer");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("steer attempt reaches the active agent")
                .as_deref(),
            Some("change course")
        );

        let _ = release_first_tx.send(());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
                .await
                .expect("rejected steer is retried after the active prompt")
                .as_deref(),
            Some("change course")
        );
        while let Ok(event) = bus_rx.try_recv() {
            assert!(
                !matches!(event, AppEvent::Ctl(CtlEvent::Error(_))),
                "steer fallback must not fail the active UI turn"
            );
        }

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn interrupt_sends_session_cancel_while_prompt_is_in_flight() {
        use agent_client_protocol::schema::v1::{
            AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
            NewSessionResponse, PromptResponse, StopReason,
        };
        use std::time::{Duration, Instant};

        let (prompt_started_tx, prompt_started_rx) = tokio::sync::oneshot::channel::<()>();
        let (cancel_seen_tx, cancel_seen_rx) = tokio::sync::oneshot::channel::<()>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let prompt_started_tx = Arc::new(Mutex::new(Some(prompt_started_tx)));
        let cancel_seen_tx = Arc::new(Mutex::new(Some(cancel_seen_tx)));
        let release_tx = Arc::new(Mutex::new(Some(release_tx)));
        let release_rx = Arc::new(Mutex::new(Some(release_rx)));

        let agent = Agent
            .builder()
            .name("cancel-mock")
            .on_receive_request(
                async move |init: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("cancel-mock", "0")),
                    )
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, _cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                {
                    let prompt_started_tx = Arc::clone(&prompt_started_tx);
                    let release_rx = Arc::clone(&release_rx);
                    async move |_req: PromptRequest, responder, cx| {
                        if let Some(tx) = prompt_started_tx.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                        let release = release_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond(PromptResponse::new(StopReason::Cancelled));
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                on_receive_request!(),
            )
            .on_receive_notification(
                {
                    let cancel_seen_tx = Arc::clone(&cancel_seen_tx);
                    let release_tx = Arc::clone(&release_tx);
                    async move |_note: CancelNotification, _cx| {
                        if let Some(tx) = cancel_seen_tx.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                        if let Some(tx) = release_tx.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                        Ok(())
                    }
                },
                on_receive_notification!(),
            );

        let cfg = RuntimeConfig {
            bin: "demo".into(),
            cordis: "demo".into(),
            workspace: "/tmp".into(),
            session_root: "/tmp".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-v4-flash".into(),
            max_tokens: None,
            base_url: None,
            api_key: None,
        };
        let (bus_tx, bus_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: "hello".into(),
            })
            .expect("prompt");
        tokio::time::timeout(Duration::from_secs(2), prompt_started_rx)
            .await
            .expect("agent should see session/prompt")
            .expect("prompt started");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_turn_start = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ui(crate::events::UiEvent::TurnStart { session, .. }))
                    if session == "s1" =>
                {
                    saw_turn_start = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(
            saw_turn_start,
            "ACP prompt start must feed the existing footer stats"
        );

        cmd_tx
            .send(Cmd::Interrupt {
                session_id: "s1".into(),
            })
            .expect("interrupt");
        tokio::time::timeout(Duration::from_secs(2), cancel_seen_rx)
            .await
            .expect("session/cancel must be sent while prompt is still in flight")
            .expect("cancel seen");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_turn_end = false;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
                    if session == "s1" && kind == "interrupted" =>
                {
                    saw_turn_end = true;
                    break;
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        assert!(
            saw_turn_end,
            "cancelled ACP prompts must close footer timing"
        );

        let _ = cmd_tx.send(Cmd::Shutdown);
        let _ = client.await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn attach_fd_socketpair_reads_without_eagain() {
        use std::io::Write;
        use std::os::fd::{FromRawFd, IntoRawFd};
        use std::os::unix::net::UnixStream as StdUnixStream;
        use tokio::io::AsyncReadExt;

        let (mut peer, local) = StdUnixStream::pair().expect("socketpair");
        peer.write_all(b"hello\n").expect("write peer");
        let file = unsafe { std::fs::File::from_raw_fd(local.into_raw_fd()) };
        let mut stream = super::unix_stream_from_file(file).expect("wrap fd");
        let mut buf = vec![0u8; 6];
        stream
            .read_exact(&mut buf)
            .await
            .expect("read should not be EAGAIN");
        assert_eq!(&buf, b"hello\n");
    }
}
