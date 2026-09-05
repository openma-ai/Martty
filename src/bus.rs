//! Unified event bus for the single-threaded UI loop.

use serde_json::Value;

/// Everything the app loop can receive.
pub enum AppEvent {
    /// Process termination signal; settle through the normal terminal guard.
    Terminate,
    /// Terminal input.
    Term(crossterm::event::Event),
    /// A protocol fact already decoded into the TUI's semantic event model.
    Ui(crate::events::UiEvent),
    /// JSON-RPC notification from the harness runtime.
    Rpc { method: String, params: Value },
    /// One line of runtime stderr (kept for diagnostics).
    RuntimeStderr(String),
    /// Runtime subprocess exited.
    RuntimeExited(Option<i32>),
    /// Controller lifecycle updates.
    Ctl(CtlEvent),
    /// ACP `session/request_permission`: UI picks an option, then replies
    /// on the oneshot. The ACP task waits; the UI thread does not.
    /// `session_id` names the owning session so the ask can follow its tab
    /// instead of floating over whatever session is on screen.
    PermissionAsk {
        session_id: String,
        title: String,
        options: Vec<PermissionAskOption>,
        reply: tokio::sync::oneshot::Sender<PermissionAskReply>,
    },
    /// ACP `elicitation/create` form: the UI owns the modal and answers the
    /// request through this oneshot without blocking session updates.
    /// Request-scoped elicitations (auth/config phases, no session yet)
    /// carry `None` and surface on the live view.
    ElicitationAsk {
        session_id: Option<String>,
        form: crate::elicitation::ElicitationForm,
        reply: tokio::sync::oneshot::Sender<crate::elicitation::ElicitationReply>,
    },
    /// Output of a local `!` shell command.
    ShellDone {
        id: u64,
        code: Option<i32>,
        output: String,
    },
}

/// One option from ACP `session/request_permission`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAskOption {
    pub option_id: String,
    pub kind: String,
    pub name: String,
}

/// UI decision for a pending ACP permission ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionAskReply {
    Selected(String),
    Cancelled,
}

/// Empty option lists cannot be selected — cancel instead of inventing AllowOnce.
pub fn permission_ask_empty_outcome(options: &[PermissionAskOption]) -> Option<PermissionAskReply> {
    options.is_empty().then_some(PermissionAskReply::Cancelled)
}

/// Highlight AllowOnce when the agent offered it; otherwise the first row.
pub fn permission_ask_default_sel(options: &[PermissionAskOption]) -> usize {
    options
        .iter()
        .position(|o| o.kind == "allow_once")
        .unwrap_or(0)
}

/// Controller → UI status updates.
#[derive(Debug, Clone)]
#[allow(dead_code)] // message_id: protocol fidelity; surfaced in debug logs only
pub enum CtlEvent {
    /// Spawning + initializing the runtime.
    Starting { runtime: String },
    /// initialize returned.
    Ready { server: String },
    /// session/prompt accepted into the durable inbox. `session_id` names
    /// the session the prompt belongs to when the sender knows it — the
    /// App must not settle the viewed tab's state for another session's
    /// prompt. Legacy attach transports (no session context) send `None`.
    PromptQueued {
        message_id: String,
        session_id: Option<String>,
    },
    /// A Send Now request settled. Rejected concurrent prompts degrade to
    /// the client FIFO without changing the active turn lifecycle.
    SteerSettled { message_id: u64, deferred: bool },
    /// A command failed.
    Error(String),
    /// Backchat `session.cancel_requested`: user stop accepted; `session/cancel`
    /// is on the wire. The in-flight `session/prompt` has not unwound yet.
    CancelRequested { session_id: String },
    /// Backchat `session.cancelled`: the prompt future settled after abort.
    /// `session_id` names the session whose turn was interrupted; one
    /// connection can run turns for several sessions concurrently.
    Interrupted { session_id: String },
    /// Host model catalog + advertised composition select.
    Catalog {
        session_id: Option<String>,
        models: Vec<CatalogModel>,
        presets: Vec<CatalogPreset>,
    },
    /// Host skill catalog arrived (`available_commands_update`).
    Skills {
        session_id: Option<String>,
        skills: Vec<SkillInfo>,
    },
    /// Read-only Loader inventory, matching the Web plugin list.
    StaticPlugins { plugins: Vec<StaticPluginItem> },
    /// Dynamic Cordis plugins owned by the current Host Agent.
    CordisPlugins { plugins: Vec<CordisPluginItem> },
    /// ACP session modes (permission / `session/set_mode`).
    SessionModes {
        session_id: Option<String>,
        modes: Vec<CatalogPreset>,
        current: Option<String>,
    },
    /// The agent accepted a composition switch (`session/set_config_option`).
    PresetSet {
        session_id: String,
        preset: String,
    },
    /// Selectable reasoning efforts for the current model.
    Efforts {
        session_id: Option<String>,
        efforts: Vec<String>,
        default: Option<String>,
    },
    /// A failed session operation that does not finish the active prompt.
    SessionOpFailed {
        session_id: String,
        message: String,
    },
    /// A failed prompt: settle this session's delivery state and report the error.
    SessionError {
        session_id: String,
        message: String,
    },
    /// A client-side control call succeeded.
    TuiOpDone(String),
    /// A client-side control call failed (or is unsupported on this transport).
    TuiOpFailed(String),
    /// ACP initialize / authenticate status (Backchat probe equivalent).
    Auth(crate::acp_auth::AuthSnapshot),
    /// Open the client `/auth` surface (method picker or the one method).
    OpenAuth,
    /// Agent-advertised session lifecycle capabilities.
    AgentCaps {
        load_session: bool,
        list_session: bool,
        resume_session: bool,
    },
    /// `session/new`, `session/resume`, or `session/load` resolved; the UI must use this id.
    SessionBound {
        session_id: String,
        notice: Option<String>,
    },
    /// A `session/new` or `session/resume` request failed outright (not an
    /// auth stall — auth keeps the request parked). acp completes bind
    /// requests in order, so the awaiting-bind FIFO head owns the failure;
    /// the UI drops that entry instead of letting a later bind land on a
    /// dead request (issue #94 bind poisoning).
    BindFailed {
        message: String,
    },
    /// `session/list` rows (`prefix` is the `/resume` argument, if any).
    /// Rows from ACP `session/list`, plus the `/resume n` limit carried by
    /// the request (applied after the current session is filtered out).
    SessionList {
        requester_session_id: String,
        sessions: Vec<SessionListItem>,
        prefix: Option<String>,
        limit: usize,
    },
    /// `session/list` missing or failed; UI falls back to local JSONL with
    /// the same `limit` the request carried.
    SessionListUnavailable {
        requester_session_id: String,
        prefix: Option<String>,
        limit: usize,
        error: String,
    },
}

/// One row from ACP `session/list`.
#[derive(Debug, Clone)]
pub struct SessionListItem {
    pub id: String,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

/// One configured Loader entry exposed by the Host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticPluginItem {
    pub entry_id: String,
    pub module_name: String,
    pub enabled: bool,
    pub fiber_phase: Option<String>,
}

/// One backend-owned dynamic Cordis plugin exposed to the TUI picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CordisPluginItem {
    pub id: String,
    pub name: String,
    pub package_id: String,
    pub status: String,
    pub approval_request_id: Option<String>,
}

/// One model-requested dynamic Cordis activation awaiting a user decision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCordisApproval {
    pub request_id: String,
    pub agent_id: String,
    pub plugin_id: String,
    pub package_id: String,
    pub mode: String,
    pub name: String,
    pub purpose: String,
}

/// One ACP-carried UI Plugin catalog row from the Client registry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct UiPluginItem {
    pub id: String,
    pub label: String,
    pub source: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub vision: bool,
}

/// One advertised composition choice (`agent` / `preset` / `agent-preset`,
/// or the first extra uncategorized select). Demo seeds stock ids including
/// `cordis`.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub broken: bool,
}

/// One user-invocable command from `available_commands_update`: typing
/// `/name …` as a prompt makes the agent inject the skill body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandConfigAction {
    pub config_id: String,
    pub value: String,
    pub reset_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// Standard ACP `AvailableCommand.input.hint`. ACP does not currently
    /// define enumerated argument candidates here.
    pub input_hint: Option<String>,
    pub config_action: Option<CommandConfigAction>,
    /// ACP advertised this as a command handled by the local Client plane.
    pub client_command: bool,
}

/// One staged image on its way to the host (base64 payload).
#[derive(Debug, Clone)]
pub struct ImagePart {
    pub data: String,
    pub media_type: String,
    pub name: String,
    /// Local path when the raster came from a file; `"clipboard"` otherwise.
    pub path: String,
}

/// One content block of a composer prompt, in draft (chip) order.
#[derive(Debug, Clone)]
pub enum PromptBlock {
    Text(String),
    Image(ImagePart),
}

/// Native Queue projection consumed by the Client-side `queue-view` Plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub count: usize,
    pub items: Vec<QueueSnapshotItem>,
    pub selected_id: Option<u64>,
    pub editing_id: Option<u64>,
    pub delete_confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSnapshotItem {
    pub id: u64,
    pub ordinal: usize,
    pub summary: String,
}

/// Native Agent/session navigation projected into the Client-side
/// `tuiAgents` service. It is local compositor state, not ACP history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsSnapshot {
    pub active_id: String,
    pub selected_id: Option<String>,
    pub items: Vec<AgentsSnapshotItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsSnapshotItem {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub status: String,
    pub current: bool,
}

/// UI → controller commands.
#[derive(Debug, Clone)]
pub enum Cmd {
    Prompt {
        session_id: String,
        text: String,
    },
    /// Send another ACP `session/prompt` immediately while a turn is active.
    Steer {
        session_id: String,
        message_id: u64,
        text: String,
    },
    /// Send a prompt whose text and images stay in draft order (图文交替).
    PromptImages {
        session_id: String,
        blocks: Vec<PromptBlock>,
    },
    /// Image-capable form of [`Cmd::Steer`].
    SteerImages {
        session_id: String,
        message_id: u64,
        blocks: Vec<PromptBlock>,
    },
    /// Interrupt the active turn over ACP.
    Interrupt {
        session_id: String,
    },
    /// Select a model/config option for the ACP session.
    SelectModel {
        session_id: String,
        provider: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    },
    FetchCatalog {
        session_id: String,
    },
    /// Fetch user-invocable host skills for the slash menu.
    FetchSkills {
        session_id: String,
    },
    /// Fetch the Host's read-only Loader inventory.
    FetchStaticPlugins,
    /// Fetch dynamic Cordis plugins owned by this Agent from the Host registry.
    FetchCordisPlugins {
        agent_id: String,
    },
    /// Stop or restore one backend-owned dynamic plugin.
    SetCordisPluginEnabled {
        agent_id: String,
        plugin_id: String,
        enabled: bool,
    },
    /// Answer one pending model-requested dynamic Cordis activation.
    RespondCordisApproval {
        request_id: String,
        decision: String,
    },
    /// Invoke a Client Plugin command on the compositor plane. This must not
    /// enter the ACP session prompt/history.
    InvokePluginCommand {
        name: String,
        args: String,
    },
    /// Publish client-owned Queue state to the local Cordis compositor. This
    /// is presentation state, never an ACP Session prompt or update.
    QueueSnapshot {
        snapshot: QueueSnapshot,
    },
    /// Publish Agent transcript navigation to the local Client compositor.
    AgentsSnapshot {
        snapshot: AgentsSnapshot,
    },
    /// Publish the currently visible native tab into the Client-side
    /// per-session projections. `None` means the tab is awaiting a bind.
    ActiveSession {
        session_id: Option<String>,
    },
    /// A native `/theme` or picker selection committed on the Client tree.
    PluginThemeSelected {
        agent_id: String,
        id: String,
    },
    /// A native `/ui` selection committed through the Client ACP plane.
    PluginUiSelected {
        agent_id: String,
        id: String,
    },
    /// Semantic event from a native Plugin overlay. Raw terminal keys and
    /// coordinates never cross this boundary.
    PluginOverlayEvent {
        id: String,
        event: String,
        value: Option<Value>,
    },
    FetchEfforts {
        session_id: String,
        provider: String,
        model: String,
    },
    SetPermission {
        session_id: String,
        preset: String,
    },
    /// Pick the agent preset (mode) composed on the session's first prompt.
    SetPreset {
        session_id: String,
        preset: String,
    },
    /// Agent-advertised command action (`_meta.commandAction`) mapped onto
    /// standard ACP `session/set_config_option`.
    SetConfigOption {
        session_id: String,
        config_id: String,
        value: String,
    },
    /// Backchat-style `authenticate`: optional in-app `_meta` values after
    /// `/auth`, or a retry after Terminal Auth / out-of-band login.
    Authenticate {
        method_id: String,
        values: std::collections::BTreeMap<String, String>,
    },
    /// Live ACP `/new` → `session/new` (cwd = workspace).
    NewSession,
    /// Live ACP `/resume` listing (`session/list`). `prefix` is the typed id;
    /// `limit` caps how many entries come back (`/resume n`).
    ListSessions {
        requester_session_id: String,
        prefix: Option<String>,
        limit: usize,
    },
    /// Live ACP `/resume` pick → `session/resume`, with legacy `session/load` fallback.
    ResumeSession {
        session_id: String,
    },
    /// `/close`: this client stops viewing a session. ACP has no
    /// session/close, so the server-side session survives; the controller
    /// drops the session's turn state and queued prompts so nothing more is
    /// sent into the void, and its later events settle at the router.
    ForgetSession {
        session_id: String,
    },
    Shutdown,
}

#[cfg(test)]
#[path = "../tests/unit/bus__tests.rs"]
mod tests;
