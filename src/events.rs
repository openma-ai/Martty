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
            vec![UiEvent::SubagentStarted {
                parent: root_session,
                child: session,
            }]
        }
        "tool_call" => vec![UiEvent::ToolCall {
            session,
            call_id: first_str(update, &["toolCallId", "tool_call_id"]),
            name: first_str(update, &["title", "kind", "toolName"]),
            arguments: update
                .get("rawInput")
                .or_else(|| update.get("raw_input"))
                .map(|v| v.to_string())
                .unwrap_or_default(),
        }],
        "tool_call_update"
            if subagent
                .and_then(|subagent| subagent.get("state"))
                .and_then(Value::as_str)
                == Some("finished") =>
        {
            vec![UiEvent::SubagentFinished { child: session }]
        }
        "tool_call_update" => {
            let status = first_str(update, &["status"]);
            if status != "completed" && status != "failed" {
                return Vec::new();
            }
            let text = acp_tool_output_text(update);
            vec![UiEvent::ToolResult {
                session,
                call_id: first_str(update, &["toolCallId", "tool_call_id"]),
                is_error: status == "failed",
                text,
                error: None,
            }]
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
        events.push(UiEvent::AgentPreset { session, preset });
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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_status_running_and_idle() {
        let ev = parse_notification(
            "session.status",
            &json!({"sessionId": "s1", "status": "running"}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::SessionStatus {
                session: "s1".into(),
                running: true
            }]
        );

        let ev = parse_notification(
            "session.status",
            &json!({"sessionId": "s1", "status": "idle"}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::SessionStatus {
                session: "s1".into(),
                running: false
            }]
        );
    }

    #[test]
    fn subagent_started_and_finished() {
        let ev = parse_notification(
            "subagent.started",
            &json!({"parentSessionId": "p", "childSessionId": "c"}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::SubagentStarted {
                parent: "p".into(),
                child: "c".into()
            }]
        );

        let ev = parse_notification("subagent.finished", &json!({"childSessionId": "c"}));
        assert_eq!(ev, vec![UiEvent::SubagentFinished { child: "c".into() }]);
    }

    #[test]
    fn standard_acp_subagent_tool_calls_restore_the_lifecycle() {
        let started = parse_notification(
            "session/update",
            &json!({
                "sessionId": "parent",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "subagent:run-1",
                    "title": "Start subagent child",
                    "status": "in_progress",
                    "_meta": {"dsh": {"subagent": {
                        "state": "started",
                        "childSessionId": "child"
                    }}}
                }
            }),
        );
        assert_eq!(
            started,
            vec![UiEvent::SubagentStarted {
                parent: "parent".into(),
                child: "child".into(),
            }]
        );

        let finished = parse_notification(
            "session/update",
            &json!({
                "sessionId": "parent",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "subagent:run-1",
                    "status": "completed",
                    "_meta": {"dsh": {"subagent": {
                        "state": "finished",
                        "childSessionId": "child"
                    }}}
                }
            }),
        );
        assert_eq!(
            finished,
            vec![UiEvent::SubagentFinished {
                child: "child".into(),
            }]
        );
    }

    #[test]
    fn nested_acp_updates_are_attributed_to_the_child_session() {
        let events = parse_notification(
            "session/update",
            &json!({
                "sessionId": "parent",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "child says hi"},
                    "_meta": {"dsh": {"subagent": {
                        "childSessionId": "child",
                        "parentToolCallId": "subagent:run-1"
                    }}}
                }
            }),
        );
        assert_eq!(
            events,
            vec![UiEvent::TextDelta {
                session: "child".into(),
                text: "child says hi".into(),
            }]
        );
    }

    #[test]
    fn turn_start_and_end() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/start", "data": {"turn": 3}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnStart {
                session: "s".into(),
                turn: 3
            }]
        );

        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/end", "data": {"reason": {"kind": "completed"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnEnd {
                session: "s".into(),
                kind: "completed".into()
            }]
        );

        // Missing reason -> default kind "unknown"; missing turn -> 0.
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/end", "data": {}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnEnd {
                session: "s".into(),
                kind: "unknown".into()
            }]
        );
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/start", "data": {}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnStart {
                session: "s".into(),
                turn: 0
            }]
        );
    }

    #[test]
    fn text_and_reasoning_deltas() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "text-delta", "text": "hi"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TextDelta {
                session: "s".into(),
                text: "hi".into()
            }]
        );

        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "reasoning-delta", "text": "hmm"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ReasoningDelta {
                session: "s".into(),
                text: "hmm".into()
            }]
        );
    }

    #[test]
    fn silent_chunk_types_emit_nothing() {
        for t in ["block-start", "block-end", "tool-call-delta", "finish"] {
            let ev = parse_notification(
                "session.event",
                &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                    "data": {"chunk": {"type": t}}}}),
            );
            assert!(ev.is_empty(), "chunk type {t} should emit nothing");
        }
    }

    #[test]
    fn usage_with_cached_input_tokens() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "usage", "usage": {
                    "inputTokens": 10, "outputTokens": 20,
                    "cachedInputTokens": 5, "reasoningTokens": 7}}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::Usage {
                session: "s".into(),
                input: 10,
                output: 20,
                cached: 5,
                reasoning: 7
            }]
        );
    }

    #[test]
    fn usage_with_cache_read_alternate_key_and_defaults() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "usage", "usage": {
                    "inputTokens": 1, "cacheReadInputTokens": 9}}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::Usage {
                session: "s".into(),
                input: 1,
                output: 0,
                cached: 9,
                reasoning: 0
            }]
        );
    }

    #[test]
    fn usage_with_harness_cache_read_tokens() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "usage", "usage": {
                    "inputTokens": 1, "outputTokens": 2,
                    "cacheReadTokens": 9, "reasoningTokens": 3}}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::Usage {
                session: "s".into(),
                input: 1,
                output: 2,
                cached: 9,
                reasoning: 3
            }]
        );
    }

    #[test]
    fn assistant_final_with_text_and_model() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/message",
                "data": {"message": {
                    "content": [
                        {"type": "text", "text": "hello "},
                        {"type": "tool-call", "text": "ignored"},
                        {"type": "text", "text": "world"}
                    ],
                    "source": {"model": "deepseek-v3"}}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::AssistantFinal {
                session: "s".into(),
                text: "hello world".into(),
                model: Some("deepseek-v3".into()),
            }]
        );
    }

    #[test]
    fn assistant_final_fallback_to_data_and_always_emits() {
        // No "message" key: use data itself; empty content still emits.
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/message",
                "data": {"content": [], "source": {"model": 42}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::AssistantFinal {
                session: "s".into(),
                text: String::new(),
                model: None
            }]
        );
    }

    #[test]
    fn tool_call_fields_and_defaults() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "tool/call",
                "data": {"callId": "c1", "name": "bash", "arguments": "{\"command\":\"ls\"}"}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolCall {
                session: "s".into(),
                call_id: "c1".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }]
        );

        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "tool/call", "data": {}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolCall {
                session: "s".into(),
                call_id: String::new(),
                name: String::new(),
                arguments: String::new(),
            }]
        );
    }

    #[test]
    fn tool_result_success_blocks() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "tool/result",
            "data": {"message": {"content": [
                {"type": "tool-result", "toolCallId": "c1", "content": [
                    {"type": "text", "text": "line1"}]},
                {"type": "tool-result", "toolCallId": "c2", "isError": false, "content": [
                    {"type": "text", "text": "line2"}]}
            ]}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolResult {
                session: "s".into(),
                call_id: "c1".into(),
                is_error: false,
                text: "line1\nline2".into(),
                error: None,
            }]
        );
    }

    #[test]
    fn tool_result_is_error_block_flag() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "tool/result",
            "data": {"message": {"content": [
                {"type": "tool-result", "toolCallId": "c1", "isError": true, "content": [
                    {"type": "text", "text": "boom"}]}
            ]}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolResult {
                session: "s".into(),
                call_id: "c1".into(),
                is_error: true,
                text: "boom".into(),
                error: None,
            }]
        );
    }

    #[test]
    fn tool_result_error_field_forces_is_error() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "tool/result",
                "data": {
                    "message": {"content": [
                        {"type": "tool-result", "toolCallId": "c9", "content": []}
                    ]},
                    "error": {"name": "ToolTimeout", "code": "E_TIMEOUT"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolResult {
                session: "s".into(),
                call_id: "c9".into(),
                is_error: true,
                text: String::new(),
                error: Some("ToolTimeout (E_TIMEOUT)".into()),
            }]
        );
    }

    #[test]
    fn user_message_user_kind_suppressed() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "user/message",
                "data": {"source": {"kind": "user"},
                         "content": [{"type": "text", "text": "typed by human"}]}}}),
        );
        assert!(ev.is_empty());

        // Missing kind also suppressed.
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "user/message",
                "data": {"content": [{"type": "text", "text": "x"}]}}}),
        );
        assert!(ev.is_empty());
    }

    #[test]
    fn user_message_plugin_kind_preview_truncation() {
        // 200 multi-byte chars; preview must be first 160 chars, boundary-safe.
        let long: String = "é".repeat(200);
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "user/message",
                "data": {"source": {"kind": "plugin"},
                         "content": [{"type": "text", "text": long}]}}}),
        );
        let expected: String = "é".repeat(160);
        assert_eq!(
            ev,
            vec![UiEvent::UserInjected {
                session: "s".into(),
                source: "plugin".into(),
                preview: expected,
            }]
        );
    }

    #[test]
    fn unknown_method_and_unknown_event_type_yield_nothing() {
        assert!(parse_notification("nope.method", &json!({"sessionId": "s"})).is_empty());
        assert!(parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "weird/thing", "data": {}}})
        )
        .is_empty());
    }

    #[test]
    fn tui_palette_notification_activates_ember() {
        let palette: Value =
            serde_json::from_str(include_str!("../docs/fixtures/demo-skin.v0.json")).unwrap();
        let ev = parse_notification(
            crate::cordis::THEME_UPDATE,
            &json!({"protocol": 0, "palette": palette, "activate": true}),
        );
        match &ev[..] {
            [UiEvent::Palette { pack, activate }] => {
                assert_eq!(pack.id, "ember");
                assert!(*activate);
                assert_eq!(
                    pack.theme(crate::theme::Mode::Dark).brand,
                    ratatui::style::Color::Rgb(247, 140, 60)
                );
            }
            other => panic!("expected Palette, got {other:?}"),
        }
    }

    #[test]
    fn tui_palette_wrong_protocol_or_invalid_yields_nothing() {
        let palette: Value =
            serde_json::from_str(include_str!("../docs/fixtures/demo-skin.v0.json")).unwrap();
        assert!(parse_notification(
            crate::cordis::THEME_UPDATE,
            &json!({"protocol": 1, "palette": palette, "activate": true})
        )
        .is_empty());
        assert!(parse_notification(
            crate::cordis::THEME_UPDATE,
            &json!({"protocol": 0, "palette": {"id": "x"}, "activate": true})
        )
        .is_empty());
    }

    #[test]
    fn session_update_maps_chunks_tools_and_agent_option() {
        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "hi"}
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TextDelta {
                session: "s".into(),
                text: "hi".into()
            }]
        );

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": ""},
                    "messageId": "2:3",
                    "_meta": {
                        "dsh": {"event": "assistant_message", "model": "deepseek-v4"}
                    }
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::AssistantFinal {
                session: "s".into(),
                text: String::new(),
                model: Some("deepseek-v4".into()),
            }]
        );

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "sessionUpdate": "current_mode_update",
                "currentModeId": "read-only"
            }),
        );
        assert!(ev.iter().any(
            |e| matches!(e, UiEvent::PermissionPreset { preset, .. } if preset == "read-only")
        ));

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "config_option_update",
                    "configOptions": [
                        {"type": "select", "id": "mode", "category": "mode", "currentValue": "read-only", "options": []},
                        {"type": "select", "id": "agent", "currentValue": "cordis", "options": [{"value": "cordis", "name": "Cordis"}]}
                    ]
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::AgentPreset {
                session: "s".into(),
                preset: "cordis".into()
            }]
        );
    }

    #[test]
    fn context_usage_update_is_not_misread_as_token_breakdown() {
        let events = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "usage_update",
                    "used": 170,
                    "size": 1000
                }
            }),
        );

        assert!(
            events.is_empty(),
            "ACP usage_update is context pressure; token breakdown comes from PromptResponse.usage"
        );
    }

    #[test]
    fn dsh_injected_context_metadata_restores_the_existing_ui_event() {
        let events = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "_meta": {
                        "dsh": {
                            "event": "user/message",
                            "source": "compaction",
                            "preview": "summary context"
                        }
                    }
                }
            }),
        );

        assert_eq!(
            events,
            vec![UiEvent::UserInjected {
                session: "s".into(),
                source: "compaction".into(),
                preview: "summary context".into(),
            }]
        );
    }

    #[test]
    fn dsh_resume_usage_metadata_restores_one_token_snapshot() {
        let events = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "_meta": {
                        "dsh": {
                            "event": "prompt/usage",
                            "usage": {
                                "inputTokens": 41,
                                "outputTokens": 9,
                                "thoughtTokens": 4,
                                "cachedReadTokens": 13,
                                "cachedWriteTokens": 2
                            }
                        }
                    }
                }
            }),
        );

        assert_eq!(
            events,
            vec![UiEvent::Usage {
                session: "s".into(),
                input: 41,
                output: 9,
                cached: 15,
                reasoning: 4,
            }]
        );
    }

    #[test]
    fn collaboration_config_update_restores_plan_mode_from_standard_acp() {
        let events = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "config_option_update",
                    "configOptions": [{
                        "type": "select",
                        "id": "collaboration_mode",
                        "currentValue": "plan",
                        "options": [
                            {"value": "default", "name": "Default"},
                            {"value": "plan", "name": "Plan"}
                        ]
                    }]
                }
            }),
        );

        assert_eq!(
            events,
            vec![UiEvent::PlanMode {
                session: "s".into(),
                active: true,
            }]
        );
    }

    #[test]
    fn available_plan_command_keeps_its_standard_config_action() {
        let commands = skills_from_available_commands(&json!([{
            "name": "plan",
            "description": "Enter or leave plan mode",
            "_meta": {
                "commandAction": {
                    "kind": "setConfigOption",
                    "configId": "collaboration_mode",
                    "value": "plan",
                    "resetValue": "default"
                }
            }
        }]));

        assert_eq!(
            commands[0].config_action,
            Some(crate::bus::CommandConfigAction {
                config_id: "collaboration_mode".into(),
                value: "plan".into(),
                reset_value: Some("default".into()),
            })
        );
    }

    #[test]
    fn catalog_prefers_agent_id_and_flattens_groups() {
        let options = json!([
            {"type": "select", "id": "model", "category": "model", "options": [{"value": "deepseek/m1", "name": "M1"}]},
            {"type": "select", "id": "agent", "options": [
                {"group": "system", "name": "System", "options": [
                    {"value": "cordis", "name": "Cordis", "description": "inspect"},
                    {"value": "broken", "name": "Broken", "description": "Broken: missing yaml"}
                ]}
            ]}
        ]);
        let (models, presets, id) = catalog_from_config_options(&options);
        assert_eq!(id.as_deref(), Some("agent"));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "m1");
        assert_eq!(presets.len(), 2);
        assert_eq!(presets[0].id, "cordis");
        assert!(presets[1].broken);
    }

    #[test]
    fn session_modes_read_available_and_current() {
        let modes = json!({
            "currentModeId": "workspace-write",
            "availableModes": [
                {"id": "read-only", "name": "Read only", "description": "no writes"},
                {"id": "workspace-write", "name": "Workspace write"}
            ]
        });
        let (list, current) = session_modes_from_value(&modes);
        assert_eq!(current.as_deref(), Some("workspace-write"));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "read-only");
        assert_eq!(list[1].name, "Workspace write");
    }

    #[test]
    fn malformed_payloads_yield_empty_vec() {
        // Non-object params.
        assert!(parse_notification("session.event", &json!("garbage")).is_empty());
        assert!(parse_notification("session.event", &json!(null)).is_empty());
        // Event missing type / wrong types.
        assert!(parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"data": {}}})
        )
        .is_empty());
        assert!(parse_notification(
            "session.event",
            &json!({"sessionId": 12, "event": {"type": 7}})
        )
        .is_empty());
        // session.status with non-string status.
        assert!(
            parse_notification("session.status", &json!({"sessionId": "s", "status": 1}))
                .is_empty()
        );
        // assistant/chunk with missing chunk.
        assert!(parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk", "data": {}}})
        )
        .is_empty());
    }
}

#[cfg(test)]
mod mode_event_tests {
    use super::*;
    use serde_json::json;

    fn ev(t: &str, data: serde_json::Value) -> serde_json::Value {
        json!({"sessionId": "s", "event": {"type": t, "data": data}})
    }

    #[test]
    fn plan_sandbox_approval_permission_preset() {
        assert_eq!(
            parse_notification("session.event", &ev("plan/mode", json!({"active": true}))),
            vec![UiEvent::PlanMode {
                session: "s".into(),
                active: true
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev("sandbox/mode", json!({"mode": "workspace-write"}))
            ),
            vec![UiEvent::SandboxMode {
                session: "s".into(),
                mode: "workspace-write".into()
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev("approval/policy", json!({"policy": "ask"}))
            ),
            vec![UiEvent::ApprovalPolicy {
                session: "s".into(),
                policy: "ask".into()
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev("permission/preset", json!({"preset": "danger-full-access"}))
            ),
            vec![UiEvent::PermissionPreset {
                session: "s".into(),
                preset: "danger-full-access".into()
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev("agent-preset/selected", json!({"agentPreset": "standard"}))
            ),
            vec![UiEvent::AgentPreset {
                session: "s".into(),
                preset: "standard".into()
            }]
        );
    }

    #[test]
    fn approval_asked_and_decided_shapes() {
        assert_eq!(
            parse_notification(
                "session.event",
                &ev(
                    "approval/asked",
                    json!({"id": "a1", "toolName": "bash", "reason": "rm -rf"})
                )
            ),
            vec![UiEvent::ApprovalAsked {
                session: "s".into(),
                tool: "bash".into(),
                reason: Some("rm -rf".into())
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev(
                    "approval/decided",
                    json!({"id": "a1", "outcome": "allowed-once"})
                )
            ),
            vec![UiEvent::ApprovalDecided {
                session: "s".into(),
                outcome: "allowed-once".into()
            }]
        );
        assert_eq!(
            parse_notification(
                "session.event",
                &ev(
                    "approval/decided",
                    json!({"id": "a1", "outcome": {"kind": "rejected"}})
                )
            ),
            vec![UiEvent::ApprovalDecided {
                session: "s".into(),
                outcome: "rejected".into()
            }]
        );
        // malformed plan/mode stays silent
        assert!(parse_notification("session.event", &ev("plan/mode", json!({}))).is_empty());
    }

    #[test]
    fn session_update_maps_user_title_plan_and_terminal_output() {
        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": "hello from load"}
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::UserMessage {
                session: "s".into(),
                text: "hello from load".into()
            }]
        );

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "title": "fix the tests"
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::SessionTitle {
                session: "s".into(),
                title: "fix the tests".into()
            }]
        );
        assert!(parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {"sessionUpdate": "session_info_update", "title": null}
            }),
        )
        .is_empty());

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "plan",
                    "entries": [
                        {"content": "locate fn main", "status": "completed"},
                        {"content": "read", "status": "in_progress"}
                    ]
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::Plan {
                session: "s".into(),
                summary: "locate fn main [completed] · read [in_progress]".into()
            }]
        );

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "plan",
                    "entries": []
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::Plan {
                session: "s".into(),
                summary: String::new()
            }],
            "an empty full snapshot must clear the previous plan"
        );

        let ev = parse_notification(
            "session/update",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "c1",
                    "status": "completed",
                    "_meta": {"terminal_output": {"terminal_id": "c1", "data": "ls output"}}
                }
            }),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ToolResult {
                session: "s".into(),
                call_id: "c1".into(),
                is_error: false,
                text: "ls output".into(),
                error: None,
            }]
        );
    }
}
