//! Parsing of deepseek-harness SDK-runtime JSON-RPC notifications into UI events.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    SessionStatus {
        session: String,
        running: bool,
    },
    TurnStart {
        session: String,
        turn: u64,
    },
    TurnEnd {
        session: String,
        kind: String,
    },
    TextDelta {
        session: String,
        text: String,
    },
    ReasoningDelta {
        session: String,
        text: String,
    },
    /// A tool-call block started, but its streamed arguments are not yet a
    /// complete request. This is a presentation transition only: the later
    /// `ToolCall` remains the authoritative request event.
    ToolCallPreparing {
        session: String,
    },
    AssistantFinal {
        session: String,
        text: String,
        model: Option<String>,
    },
    ToolCall {
        session: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        session: String,
        call_id: String,
        is_error: bool,
        text: String,
        error: Option<String>,
    },
    Usage {
        session: String,
        input: u64,
        output: u64,
        cached: u64,
        reasoning: u64,
    },
    UserInjected {
        session: String,
        source: String,
        preview: String,
    },
    SubagentStarted {
        parent: String,
        child: String,
    },
    SubagentFinished {
        child: String,
        failed: bool,
    },
    /// ACP `user_message_chunk` (session/load replay needs user lines).
    UserMessage {
        session: String,
        text: String,
    },
    /// ACP `session_info_update` title.
    SessionTitle {
        session: String,
        title: String,
    },
    /// ACP `plan` / `plan_update` snapshot (todo entries, not `plan/mode`).
    Plan {
        session: String,
        summary: String,
    },
    /// `plan/mode` — dsh-plan-mode collaboration state (last one wins).
    PlanMode {
        session: String,
        active: bool,
    },
    /// `sandbox/mode` — file policy: read-only | workspace-write | danger-full-access.
    SandboxMode {
        session: String,
        mode: String,
    },
    /// `approval/policy` — ask | never.
    ApprovalPolicy {
        session: String,
        policy: String,
    },
    /// `permission/preset` — bundled permission preset name.
    PermissionPreset {
        session: String,
        preset: String,
    },
    /// `agent-preset/selected` — agent composition preset (standard/code/…).
    AgentPreset {
        session: String,
        preset: String,
    },
    /// ACP `configOptions.effort.currentValue` — Host-owned reasoning default.
    ReasoningEffort {
        session: String,
        effort: String,
    },
    /// `approval/asked` — one pending approval request.
    ApprovalAsked {
        session: String,
        tool: String,
        reason: Option<String>,
    },
    /// `approval/decided` — its outcome.
    ApprovalDecided {
        session: String,
        outcome: String,
    },
    /// Cordis TUI theme update (not a session log event).
    Palette {
        pack: crate::theme::PalettePack,
        activate: bool,
    },
}

/// Parse a JSON-RPC notification into zero or more UI events.
/// Unknown methods and malformed payloads yield an empty vec; never panics.
pub fn parse_notification(method: &str, params: &Value) -> Vec<UiEvent> {
    match method {
        "session.status" => {
            let session = str_field(params, "sessionId");
            match params.get("status").and_then(Value::as_str) {
                Some(status) => vec![UiEvent::SessionStatus {
                    session,
                    running: status == "running",
                }],
                None => Vec::new(),
            }
        }
        "subagent.started" => vec![UiEvent::SubagentStarted {
            parent: str_field(params, "parentSessionId"),
            child: str_field(params, "childSessionId"),
        }],
        "subagent.finished" => vec![UiEvent::SubagentFinished {
            child: str_field(params, "childSessionId"),
            failed: false,
        }],
        "session.event" => parse_session_event(params),
        "session/update" => parse_session_update(params),
        crate::cordis::THEME_UPDATE => match crate::theme::parse_palette_notification(params) {
            Ok(Some(n)) => vec![UiEvent::Palette {
                pack: n.pack,
                activate: n.activate,
            }],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn parse_session_event(params: &Value) -> Vec<UiEvent> {
    let session = str_field(params, "sessionId");
    let event = match params.get("event") {
        Some(e) => e,
        None => return Vec::new(),
    };
    let event_type = match event.get("type").and_then(Value::as_str) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let data = event.get("data").unwrap_or(&Value::Null);

    match event_type {
        "turn/start" => {
            let turn = data.get("turn").and_then(Value::as_u64).unwrap_or(0);
            vec![UiEvent::TurnStart { session, turn }]
        }
        "turn/end" => {
            let kind = data
                .get("reason")
                .and_then(|r| r.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            vec![UiEvent::TurnEnd { session, kind }]
        }
        "assistant/chunk" => parse_assistant_chunk(session, data),
        "plan/mode" => match data.get("active").and_then(Value::as_bool) {
            Some(active) => vec![UiEvent::PlanMode { session, active }],
            None => Vec::new(),
        },
        "sandbox/mode" => match data.get("mode").and_then(Value::as_str) {
            Some(mode) => vec![UiEvent::SandboxMode {
                session,
                mode: mode.to_string(),
            }],
            None => Vec::new(),
        },
        "approval/policy" => match data.get("policy").and_then(Value::as_str) {
            Some(policy) => vec![UiEvent::ApprovalPolicy {
                session,
                policy: policy.to_string(),
            }],
            None => Vec::new(),
        },
        "permission/preset" => match data.get("preset").and_then(Value::as_str) {
            Some(preset) => vec![UiEvent::PermissionPreset {
                session,
                preset: preset.to_string(),
            }],
            None => Vec::new(),
        },
        "agent-preset/selected" => match data.get("agentPreset").and_then(Value::as_str) {
            Some(preset) => vec![UiEvent::AgentPreset {
                session,
                preset: preset.to_string(),
            }],
            None => Vec::new(),
        },
        "approval/asked" => {
            let tool = data
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let reason = data
                .get("reason")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            vec![UiEvent::ApprovalAsked {
                session,
                tool,
                reason,
            }]
        }
        "approval/decided" => {
            let outcome = match data.get("outcome") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Object(o)) => o
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("decided")
                    .to_string(),
                _ => "decided".to_string(),
            };
            vec![UiEvent::ApprovalDecided { session, outcome }]
        }
        "assistant/message" => {
            let msg = data.get("message").unwrap_or(data);
            let text = concat_text_blocks(msg.get("content").unwrap_or(&Value::Null));
            let model = msg
                .get("source")
                .and_then(|s| s.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
            vec![UiEvent::AssistantFinal {
                session,
                text,
                model,
            }]
        }
        "tool/call" => vec![UiEvent::ToolCall {
            session,
            call_id: str_field(data, "callId"),
            name: str_field(data, "name"),
            arguments: str_field(data, "arguments"),
        }],
        "tool/result" => parse_tool_result(session, data),
        "user/message" => parse_user_message(session, data),
        _ => Vec::new(),
    }
}

fn parse_session_update(params: &Value) -> Vec<UiEvent> {
    let root_session = str_field(params, "sessionId");
    let update = params.get("update").unwrap_or(params);
    let subagent = update
        .get("_meta")
        .and_then(|meta| meta.get("dsh"))
        .and_then(|dsh| dsh.get("subagent"));
    let session = subagent
        .and_then(|subagent| subagent.get("childSessionId"))
        .and_then(Value::as_str)
        .unwrap_or(&root_session)
        .to_string();
    let kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or("");
    match kind {
        "agent_message_chunk" => {
            let text = acp_text_content(update.get("content"));
            let completion = update
                .get("_meta")
                .and_then(|meta| meta.get("dsh"))
                .filter(|dsh| {
                    dsh.get("event").and_then(Value::as_str) == Some("assistant_message")
                });
            let mut events = Vec::new();
            if !text.is_empty() {
                events.push(UiEvent::TextDelta {
                    session: session.clone(),
                    text,
                });
            }
            if let Some(dsh) = completion {
                events.push(UiEvent::AssistantFinal {
                    session,
                    text: String::new(),
                    model: dsh.get("model").and_then(Value::as_str).map(str::to_string),
                });
            }
            events
        }
        "agent_thought_chunk" => {
            let text = acp_text_content(update.get("content"));
            if text.is_empty() {
                Vec::new()
            } else {
                vec![UiEvent::ReasoningDelta { session, text }]
            }
        }
        "user_message_chunk" => {
            let text = acp_text_content(update.get("content"));
            if text.is_empty() {
                Vec::new()
            } else {
                vec![UiEvent::UserMessage { session, text }]
            }
        }
        "session_info_update" => {
            let dsh = update.get("_meta").and_then(|meta| meta.get("dsh"));
            if let Some(subagent) = dsh
                .filter(|dsh| {
                    dsh.get("event").and_then(Value::as_str) == Some("subagent/lifecycle")
                })
                .and_then(|dsh| dsh.get("subagent"))
            {
                let child = str_field(subagent, "childSessionId");
                return match subagent.get("state").and_then(Value::as_str) {
                    Some("started") => vec![UiEvent::SubagentStarted {
                        parent: root_session,
                        child,
                    }],
                    Some("finished") => vec![UiEvent::SubagentFinished {
                        child,
                        failed: subagent.get("stopReason").and_then(Value::as_str)
                            != Some("completed"),
                    }],
                    _ => Vec::new(),
                };
            }
            if let Some(usage) = dsh
                .filter(|dsh| dsh.get("event").and_then(Value::as_str) == Some("prompt/usage"))
                .and_then(|dsh| dsh.get("usage"))
            {
                return vec![UiEvent::Usage {
                    session,
                    input: u64_field(usage, "inputTokens").unwrap_or(0),
                    output: u64_field(usage, "outputTokens").unwrap_or(0),
                    cached: u64_field(usage, "cachedReadTokens").unwrap_or(0)
                        + u64_field(usage, "cachedWriteTokens").unwrap_or(0),
                    reasoning: u64_field(usage, "thoughtTokens").unwrap_or(0),
                }];
            }
            if let Some(dsh) =
                dsh.filter(|dsh| dsh.get("event").and_then(Value::as_str) == Some("user/message"))
            {
                return vec![UiEvent::UserInjected {
                    session,
                    source: str_field(dsh, "source"),
                    preview: str_field(dsh, "preview"),
                }];
            }
            let title = update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                Vec::new()
            } else {
                vec![UiEvent::SessionTitle {
                    session,
                    title: title.to_string(),
                }]
            }
        }
        "plan" | "plan_update" => {
            let summary = plan_summary(update);
            if summary.is_empty() && update.get("entries").and_then(Value::as_array).is_none() {
                Vec::new()
            } else {
                vec![UiEvent::Plan { session, summary }]
            }
        }
        "plan_removed" => vec![UiEvent::Plan {
            session,
            summary: String::new(),
        }],
        "tool_call"
            if subagent
                .and_then(|subagent| subagent.get("state"))
                .and_then(Value::as_str)
                == Some("started") =>
        {
            vec![
                acp_tool_call(root_session.clone(), update),
                UiEvent::SubagentStarted {
                    parent: root_session,
                    child: session,
                },
            ]
        }
        "tool_call" => vec![acp_tool_call(session, update)],
        "tool_call_update"
            if subagent
                .and_then(|subagent| subagent.get("state"))
                .and_then(Value::as_str)
                == Some("finished") =>
        {
            let mut events = acp_tool_result(root_session, update)
                .into_iter()
                .collect::<Vec<_>>();
            events.push(UiEvent::SubagentFinished {
                child: session,
                failed: update.get("status").and_then(Value::as_str) == Some("failed"),
            });
            events
        }
        "tool_call_update" => {
            let status = first_str(update, &["status"]);
            if status == "completed" || status == "failed" {
                acp_tool_result(session, update).into_iter().collect()
            } else if (status == "pending" || status == "in_progress")
                && (update.get("rawInput").is_some()
                    || update.get("raw_input").is_some()
                    || update.get("title").is_some())
            {
                vec![acp_tool_call(session, update)]
            } else {
                Vec::new()
            }
        }
        "current_mode_update" => {
            let mode = first_str(update, &["currentModeId", "current_mode_id"]);
            if mode.is_empty() {
                Vec::new()
            } else {
                vec![
                    UiEvent::PermissionPreset {
                        session: session.clone(),
                        preset: mode.clone(),
                    },
                    UiEvent::SandboxMode { session, mode },
                ]
            }
        }
        "config_option_update" => {
            let options = update
                .get("configOptions")
                .or_else(|| update.get("config_options"));
            options
                .map(|options| config_option_events(session, options))
                .unwrap_or_default()
        }
        // Standard ACP `usage_update` reports context-window pressure as
        // `used`/`size`; turn token totals come from `PromptResponse.usage`.
        "usage_update" => Vec::new(),
        _ => Vec::new(),
    }
}

fn acp_text_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    concat_text_blocks(content)
}

fn acp_tool_call(session: String, update: &Value) -> UiEvent {
    UiEvent::ToolCall {
        session,
        call_id: first_str(update, &["toolCallId", "tool_call_id"]),
        name: first_str(update, &["title", "kind", "toolName"]),
        arguments: update
            .get("rawInput")
            .or_else(|| update.get("raw_input"))
            .map(Value::to_string)
            .unwrap_or_default(),
    }
}

fn acp_tool_result(session: String, update: &Value) -> Option<UiEvent> {
    let status = first_str(update, &["status"]);
    if status != "completed" && status != "failed" {
        return None;
    }
    Some(UiEvent::ToolResult {
        session,
        call_id: first_str(update, &["toolCallId", "tool_call_id"]),
        is_error: status == "failed",
        text: acp_tool_output_text(update),
        error: None,
    })
}

fn acp_tool_output_text(update: &Value) -> String {
    if let Some(data) = update
        .get("_meta")
        .and_then(|m| m.get("terminal_output"))
        .and_then(|t| t.get("data"))
        .and_then(Value::as_str)
    {
        if !data.is_empty() {
            return data.to_string();
        }
    }
    if let Some(raw) = update.get("rawOutput").or_else(|| update.get("raw_output")) {
        if let Some(s) = raw.as_str() {
            return s.to_string();
        }
        if let Some(s) = raw.get("output").and_then(Value::as_str) {
            return s.to_string();
        }
        return raw.to_string();
    }
    if let Some(content) = update.get("content") {
        return concat_text_blocks(content);
    }
    String::new()
}

fn plan_summary(update: &Value) -> String {
    let entries = update
        .get("entries")
        .or_else(|| update.get("plan").and_then(|p| p.get("entries")))
        .and_then(Value::as_array);
    if let Some(entries) = entries {
        let parts: Vec<String> = entries
            .iter()
            .filter_map(|entry| {
                let content = entry.get("content").and_then(Value::as_str)?;
                let status = entry.get("status").and_then(Value::as_str).unwrap_or("");
                if status.is_empty() {
                    Some(content.to_string())
                } else {
                    Some(format!("{content} [{status}]"))
                }
            })
            .collect();
        return parts.join(" · ");
    }
    update
        .get("plan")
        .and_then(|p| p.get("content"))
        .and_then(Value::as_str)
        .or_else(|| update.get("content").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

fn first_str(v: &Value, keys: &[&str]) -> String {
    for key in keys {
        let s = str_field(v, key);
        if !s.is_empty() {
            return s;
        }
    }
    String::new()
}

/// Flatten ACP select options, including `{ group, options }` rows.
pub fn flatten_select_options(options: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let Some(arr) = options.as_array() else {
        return out;
    };
    for item in arr {
        if let Some(nested) = item.get("options") {
            out.extend(flatten_select_options(nested));
            continue;
        }
        let Some(value) = item.get("value").and_then(Value::as_str) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(value)
            .to_string();
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        out.push((value.to_string(), name, description));
    }
    out
}

/// The extra composition select, if the agent advertised one.
/// Prefers `agent` / `preset` / `agent-preset`; else the first uncategorized
/// select that is not mode/model/effort.
pub fn composition_option<'a>(options: &'a Value) -> Option<&'a Value> {
    let arr = options.as_array()?;
    let named = arr.iter().find(|option| {
        matches!(
            option.get("id").and_then(Value::as_str),
            Some("agent" | "preset" | "agent-preset")
        )
    });
    if named.is_some() {
        return named;
    }
    arr.iter().find(|option| {
        option.get("type").and_then(Value::as_str) == Some("select")
            && option.get("category").and_then(Value::as_str).is_none()
            && !matches!(
                option.get("id").and_then(Value::as_str),
                Some("mode" | "model" | "effort" | "collaboration_mode")
            )
    })
}

fn composition_current_value(options: Option<&Value>) -> Option<String> {
    let options = options?;
    let option = composition_option(options)?;
    option
        .get("currentValue")
        .or_else(|| option.get("current_value"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn collaboration_mode_active(options: Option<&Value>) -> Option<bool> {
    let option = options?
        .as_array()?
        .iter()
        .find(|option| option.get("id").and_then(Value::as_str) == Some("collaboration_mode"))?;
    option
        .get("currentValue")
        .or_else(|| option.get("current_value"))
        .and_then(Value::as_str)
        .map(|value| value == "plan")
}

/// Fold a complete ACP `configOptions` snapshot into the client-facing facts
/// that drive existing composer chrome. Used for both setup responses and
/// later `config_option_update` notifications.
pub fn config_option_events(session: String, options: &Value) -> Vec<UiEvent> {
    let mut events = Vec::new();
    if let Some(active) = collaboration_mode_active(Some(options)) {
        events.push(UiEvent::PlanMode {
            session: session.clone(),
            active,
        });
    }
    if let Some(preset) = composition_current_value(Some(options)) {
        events.push(UiEvent::AgentPreset {
            session: session.clone(),
            preset,
        });
    }
    if let Some(effort) = options.as_array().and_then(|entries| {
        entries
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some("effort"))
            .and_then(|option| {
                option
                    .get("currentValue")
                    .or_else(|| option.get("current_value"))
                    .and_then(Value::as_str)
            })
    }) {
        events.push(UiEvent::ReasoningEffort {
            session,
            effort: effort.to_string(),
        });
    }
    events
}

/// Models + composition choices from ACP `configOptions`.
pub fn catalog_from_config_options(
    options: &Value,
) -> (
    Vec<crate::bus::CatalogModel>,
    Vec<crate::bus::CatalogPreset>,
    Option<String>,
) {
    let mut models = Vec::new();
    if let Some(arr) = options.as_array() {
        if let Some(model) = arr
            .iter()
            .find(|o| o.get("id").and_then(Value::as_str) == Some("model"))
        {
            for (id, name, _) in
                flatten_select_options(model.get("options").unwrap_or(&Value::Null))
            {
                let (provider, model_id) = match id.split_once('/') {
                    Some((p, m)) => (p.to_string(), m.to_string()),
                    None => (String::new(), id.clone()),
                };
                models.push(crate::bus::CatalogModel {
                    provider,
                    id: model_id,
                    name,
                    vision: false,
                });
            }
        }
    }
    let composition = composition_option(options);
    let composition_id = composition
        .and_then(|o| o.get("id").and_then(Value::as_str))
        .map(str::to_string);
    let mut presets = Vec::new();
    if let Some(option) = composition {
        for (id, name, description) in
            flatten_select_options(option.get("options").unwrap_or(&Value::Null))
        {
            let broken = description.starts_with("Broken:");
            presets.push(crate::bus::CatalogPreset {
                id,
                name,
                description,
                broken,
            });
        }
    }
    (models, presets, composition_id)
}

/// ACP session modes (`session/new.modes` / `availableModes`).
pub fn session_modes_from_value(modes: &Value) -> (Vec<crate::bus::CatalogPreset>, Option<String>) {
    let current = first_str(modes, &["currentModeId", "current_mode_id"]);
    let current = if current.is_empty() {
        None
    } else {
        Some(current)
    };
    let mut out = Vec::new();
    let Some(arr) = modes
        .get("availableModes")
        .or_else(|| modes.get("available_modes"))
        .and_then(Value::as_array)
    else {
        return (out, current);
    };
    for mode in arr {
        let Some(id) = mode.get("id").and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            other => other.as_str().map(str::to_string),
        }) else {
            continue;
        };
        out.push(crate::bus::CatalogPreset {
            name: mode
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            description: mode
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            id,
            broken: false,
        });
    }
    (out, current)
}

/// Slash entries from `available_commands_update`.
pub fn skills_from_available_commands(commands: &Value) -> Vec<crate::bus::SkillInfo> {
    let mut out = Vec::new();
    let Some(arr) = commands.as_array() else {
        return out;
    };
    for command in arr {
        let Some(name) = command.get("name").and_then(Value::as_str) else {
            continue;
        };
        out.push(crate::bus::SkillInfo {
            name: name.to_string(),
            description: command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            input_hint: command
                .pointer("/input/hint")
                .and_then(Value::as_str)
                .map(str::to_string),
            config_action: command_config_action(command),
            client_command: command_client_action(command),
        });
    }
    out
}

fn command_client_action(command: &Value) -> bool {
    [command.get("metadata"), command.get("_meta")]
        .into_iter()
        .flatten()
        .filter_map(|carrier| carrier.get("commandAction"))
        .any(|action| action.get("kind").and_then(Value::as_str) == Some("clientCommand"))
}

fn command_config_action(command: &Value) -> Option<crate::bus::CommandConfigAction> {
    for carrier in [command.get("metadata"), command.get("_meta")]
        .into_iter()
        .flatten()
    {
        let action = carrier.get("commandAction")?;
        if action.get("kind").and_then(Value::as_str) != Some("setConfigOption") {
            continue;
        }
        let config_id = action.get("configId")?.as_str()?;
        let value = action.get("value")?.as_str()?;
        let reset_value = action
            .get("resetValue")
            .and_then(Value::as_str)
            .map(str::to_string);
        return Some(crate::bus::CommandConfigAction {
            config_id: config_id.to_string(),
            value: value.to_string(),
            reset_value,
        });
    }
    None
}

fn parse_assistant_chunk(session: String, data: &Value) -> Vec<UiEvent> {
    let chunk = match data.get("chunk") {
        Some(c) => c,
        None => return Vec::new(),
    };
    match chunk.get("type").and_then(Value::as_str) {
        Some("text-delta") => vec![UiEvent::TextDelta {
            session,
            text: str_field(chunk, "text"),
        }],
        Some("reasoning-delta") => vec![UiEvent::ReasoningDelta {
            session,
            text: str_field(chunk, "text"),
        }],
        Some("block-start")
            if chunk.get("blockType").and_then(Value::as_str) == Some("tool-call") =>
        {
            vec![UiEvent::ToolCallPreparing { session }]
        }
        Some("usage") => {
            let usage = chunk.get("usage").unwrap_or(&Value::Null);
            let cached = u64_field(usage, "cacheReadTokens")
                .or_else(|| u64_field(usage, "cachedInputTokens"))
                .or_else(|| u64_field(usage, "cacheReadInputTokens"))
                .unwrap_or(0);
            vec![UiEvent::Usage {
                session,
                input: u64_field(usage, "inputTokens").unwrap_or(0),
                output: u64_field(usage, "outputTokens").unwrap_or(0),
                cached,
                reasoning: u64_field(usage, "reasoningTokens").unwrap_or(0),
            }]
        }
        _ => Vec::new(),
    }
}

fn parse_tool_result(session: String, data: &Value) -> Vec<UiEvent> {
    let empty: Vec<Value> = Vec::new();
    let blocks = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    let mut is_error = false;
    let mut call_id = String::new();
    let mut have_call_id = false;
    let mut texts: Vec<String> = Vec::new();

    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("tool-result") {
            continue;
        }
        if block.get("isError").and_then(Value::as_bool) == Some(true) {
            is_error = true;
        }
        if !have_call_id {
            call_id = str_field(block, "toolCallId");
            have_call_id = true;
        }
        texts.push(concat_text_blocks(
            block.get("content").unwrap_or(&Value::Null),
        ));
    }

    let mut error = None;
    if let Some(err) = data.get("error") {
        if !err.is_null() {
            let name = str_field(err, "name");
            let code = match err.get("code") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            error = Some(format!("{name} ({code})"));
            is_error = true;
        }
    }

    vec![UiEvent::ToolResult {
        session,
        call_id,
        is_error,
        text: texts.join("\n"),
        error,
    }]
}

fn parse_user_message(session: String, data: &Value) -> Vec<UiEvent> {
    let kind = match data
        .get("source")
        .and_then(|s| s.get("kind"))
        .and_then(Value::as_str)
    {
        Some(k) => k,
        None => return Vec::new(),
    };
    if kind == "user" {
        return Vec::new();
    }
    let full = concat_text_blocks(data.get("content").unwrap_or(&Value::Null));
    let preview: String = full.chars().take(160).collect();
    vec![UiEvent::UserInjected {
        session,
        source: kind.to_string(),
        preview,
    }]
}

/// Concatenate the `.text` of blocks with `type == "text"` in a content array.
fn concat_text_blocks(content: &Value) -> String {
    let mut out = String::new();
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    out.push_str(t);
                }
            }
        }
    }
    out
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn u64_field(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

#[cfg(test)]
#[path = "../tests/unit/events__tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/events__mode_event_tests.rs"]
mod mode_event_tests;
