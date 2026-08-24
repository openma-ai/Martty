//! DSH Cordis ACP extension contract shared by the native client surfaces.

pub const PROTOCOL: u64 = 0;

pub const THEME_UPDATE: &str = "_dsh/cordis/tui/theme/update";
pub const THEME_REMOVE: &str = "_dsh/cordis/tui/theme/remove";
pub const THEME_SELECTED: &str = "_dsh/cordis/tui/theme/selected";
pub const SLOTS_UPDATE: &str = "_dsh/cordis/tui/slots/update";
pub const COMMANDS_UPDATE: &str = "_dsh/cordis/tui/commands/update";
pub const COMMAND_INVOKE: &str = "_dsh/cordis/tui/commands/invoke";
pub const OVERLAY_UPDATE: &str = "_dsh/cordis/tui/overlay/update";
pub const OVERLAY_EVENT: &str = "_dsh/cordis/tui/overlay/event";
pub const QUEUE_UPDATE: &str = "_dsh/cordis/tui/queue/update";
pub const AGENTS_UPDATE: &str = "_dsh/cordis/tui/agents/update";
pub const AGENTS_SELECT: &str = "_dsh/cordis/tui/agents/select";
pub const AGENTS_NAVIGATE: &str = "_dsh/cordis/tui/agents/navigate";
pub const SESSION_CONFIG_SET: &str = "_dsh/cordis/tui/session-config/set";
pub const APPROVALS_UPDATE: &str = "_dsh/cordis/tui/approvals/update";
pub const APPROVAL_RESPOND: &str = "_dsh/cordis/tui/approvals/respond";
pub const UI_UPDATE: &str = "_dsh/cordis/tui/ui/update";
pub const UI_SELECTED: &str = "_dsh/cordis/tui/ui/selected";

pub const STATIC_PLUGINS_LIST: &str = "_dsh/plugins/list";
pub const PLUGINS_LIST: &str = "_dsh/cordis/plugins/list";
pub const PLUGIN_START: &str = "_dsh/cordis/plugins/start";
pub const PLUGIN_STOP: &str = "_dsh/cordis/plugins/stop";

pub fn advertised_by_agent(initialize: &serde_json::Value) -> bool {
    initialize
        .get("agentCapabilities")
        .and_then(|capabilities| capabilities.get("_meta"))
        .and_then(|meta| meta.get("dsh"))
        .and_then(|dsh| dsh.get("cordis"))
        .and_then(|cordis| cordis.get("protocol"))
        .and_then(serde_json::Value::as_u64)
        == Some(PROTOCOL)
}
