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
    PermissionAsk {
        title: String,
        options: Vec<PermissionAskOption>,
        reply: tokio::sync::oneshot::Sender<PermissionAskReply>,
    },
    /// ACP `elicitation/create` form: the UI owns the modal and answers the
    /// request through this oneshot without blocking session updates.
    ElicitationAsk {
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
    /// session/prompt accepted into the durable inbox.
    PromptQueued { message_id: String },
    /// A Send Now request settled. Rejected concurrent prompts degrade to
    /// the client FIFO without changing the active turn lifecycle.
    SteerSettled { message_id: u64, deferred: bool },
    /// A command failed.
    Error(String),
    /// Backchat `session.cancel_requested`: user stop accepted; `session/cancel`
    /// is on the wire. The in-flight `session/prompt` has not unwound yet.
    CancelRequested,
    /// Backchat `session.cancelled`: the prompt future settled after abort.
    Interrupted,
    /// Host model catalog + advertised composition select.
    Catalog {
        models: Vec<CatalogModel>,
        presets: Vec<CatalogPreset>,
    },
    /// Host skill catalog arrived (`available_commands_update`).
    Skills { skills: Vec<SkillInfo> },
    /// Dynamic plugins owned by the current Host Agent.
    Plugins { plugins: Vec<DynamicPluginItem> },
    /// ACP session modes (permission / `session/set_mode`).
    SessionModes {
        modes: Vec<CatalogPreset>,
        current: Option<String>,
    },
    /// The agent accepted a composition switch (`session/set_config_option`).
    PresetSet { preset: String },
    /// Selectable reasoning efforts for the current model.
    Efforts {
        efforts: Vec<String>,
        default: Option<String>,
    },
    /// A client-side control call succeeded.
    TuiOpDone(String),
    /// A client-side control call failed (or is unsupported on this transport).
    TuiOpFailed(String),
    /// ACP initialize / authenticate status (Backchat probe equivalent).
    Auth(crate::acp_auth::AuthSnapshot),
    /// Open the client `/auth` surface (method picker or the one method).
    OpenAuth,
    /// Agent advertised `loadSession` (`session/load`, usually with `session/list`).
    AgentCaps { load_session: bool },
    /// `session/new` or `session/load` resolved; the UI must use this id.
    SessionBound {
        session_id: String,
        notice: Option<String>,
    },
    /// `session/list` rows (`prefix` is the `/resume` argument, if any).
    SessionList {
        sessions: Vec<SessionListItem>,
        prefix: Option<String>,
    },
    /// `session/list` missing or failed; UI falls back to local JSONL.
    SessionListUnavailable {
        prefix: Option<String>,
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

/// One backend-owned dynamic Cordis plugin exposed to the TUI picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicPluginItem {
    pub id: String,
    pub name: String,
    pub package_id: String,
    pub running: bool,
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
#[derive(Debug, Clone)]
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
    FetchCatalog,
    /// Fetch user-invocable host skills for the slash menu.
    FetchSkills,
    /// Fetch dynamic plugins owned by this Agent from the Host registry.
    FetchPlugins {
        agent_id: String,
    },
    /// Stop or restore one backend-owned dynamic plugin.
    SetPluginEnabled {
        agent_id: String,
        plugin_id: String,
        enabled: bool,
    },
    /// Invoke a Client Plugin command on the compositor plane. This must not
    /// enter the ACP session prompt/history.
    InvokePluginCommand {
        name: String,
        args: String,
    },
    /// A native `/theme` or picker selection committed on the Client tree.
    PluginThemeSelected {
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
    /// Live ACP `/resume` listing (`session/list`). `prefix` is the typed id.
    ListSessions {
        prefix: Option<String>,
    },
    /// Live ACP `/resume` pick → `session/load`.
    LoadSession {
        session_id: String,
    },
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_permission_options_cancel() {
        assert_eq!(
            permission_ask_empty_outcome(&[]),
            Some(PermissionAskReply::Cancelled)
        );
        assert_eq!(
            permission_ask_empty_outcome(&[PermissionAskOption {
                option_id: "allow".into(),
                kind: "allow_once".into(),
                name: "Allow once".into(),
            }]),
            None
        );
    }

    #[test]
    fn permission_ask_defaults_to_allow_once() {
        let options = [
            PermissionAskOption {
                option_id: "reject".into(),
                kind: "reject_once".into(),
                name: "Reject".into(),
            },
            PermissionAskOption {
                option_id: "allow".into(),
                kind: "allow_once".into(),
                name: "Allow once".into(),
            },
        ];
        assert_eq!(permission_ask_default_sel(&options), 1);
        assert_eq!(permission_ask_default_sel(&options[..1]), 0);
    }
}
