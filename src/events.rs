//! Parsing of deepseek-harness SDK-runtime JSON-RPC notifications into UI events.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    SessionStatus { session: String, running: bool },
    TurnStart { session: String, turn: u64 },
    TurnEnd { session: String, kind: String },
    TextDelta { session: String, text: String },
    ReasoningDelta { session: String, text: String },
    AssistantFinal { session: String, text: String, model: Option<String> },
    ToolCall { session: String, call_id: String, name: String, arguments: String },
    ToolResult { session: String, call_id: String, is_error: bool, text: String, error: Option<String> },
    Usage { session: String, input: u64, output: u64, cached: u64, reasoning: u64 },
    UserInjected { session: String, source: String, preview: String },
    SubagentStarted { parent: String, child: String },
    SubagentFinished { child: String },
    /// `plan/mode` — dsh-plan-mode collaboration state (last one wins).
    PlanMode { session: String, active: bool },
    /// `sandbox/mode` — file policy: read-only | workspace-write | danger-full-access.
    SandboxMode { session: String, mode: String },
    /// `approval/policy` — ask | never.
    ApprovalPolicy { session: String, policy: String },
    /// `permission/preset` — bundled permission preset name.
    PermissionPreset { session: String, preset: String },
    /// `agent-preset/selected` — agent composition preset (standard/code/…).
    AgentPreset { session: String, preset: String },
    /// `approval/asked` — one pending approval request.
    ApprovalAsked { session: String, tool: String, reason: Option<String> },
    /// `approval/decided` — its outcome.
    ApprovalDecided { session: String, outcome: String },
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
            vec![UiEvent::AssistantFinal { session, text, model }]
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
            let cached = u64_field(usage, "cachedInputTokens")
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
        texts.push(concat_text_blocks(block.get("content").unwrap_or(&Value::Null)));
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
            vec![UiEvent::SessionStatus { session: "s1".into(), running: true }]
        );

        let ev = parse_notification(
            "session.status",
            &json!({"sessionId": "s1", "status": "idle"}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::SessionStatus { session: "s1".into(), running: false }]
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
            vec![UiEvent::SubagentStarted { parent: "p".into(), child: "c".into() }]
        );

        let ev = parse_notification("subagent.finished", &json!({"childSessionId": "c"}));
        assert_eq!(ev, vec![UiEvent::SubagentFinished { child: "c".into() }]);
    }

    #[test]
    fn turn_start_and_end() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/start", "data": {"turn": 3}}}),
        );
        assert_eq!(ev, vec![UiEvent::TurnStart { session: "s".into(), turn: 3 }]);

        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/end", "data": {"reason": {"kind": "completed"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnEnd { session: "s".into(), kind: "completed".into() }]
        );

        // Missing reason -> default kind "unknown"; missing turn -> 0.
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/end", "data": {}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::TurnEnd { session: "s".into(), kind: "unknown".into() }]
        );
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "turn/start", "data": {}}}),
        );
        assert_eq!(ev, vec![UiEvent::TurnStart { session: "s".into(), turn: 0 }]);
    }

    #[test]
    fn text_and_reasoning_deltas() {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "text-delta", "text": "hi"}}}}),
        );
        assert_eq!(ev, vec![UiEvent::TextDelta { session: "s".into(), text: "hi".into() }]);

        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "reasoning-delta", "text": "hmm"}}}}),
        );
        assert_eq!(
            ev,
            vec![UiEvent::ReasoningDelta { session: "s".into(), text: "hmm".into() }]
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
            vec![UiEvent::Usage { session: "s".into(), input: 10, output: 20, cached: 5, reasoning: 7 }]
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
            vec![UiEvent::Usage { session: "s".into(), input: 1, output: 0, cached: 9, reasoning: 0 }]
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
            vec![UiEvent::AssistantFinal { session: "s".into(), text: String::new(), model: None }]
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
        assert!(parse_notification("session.status", &json!({"sessionId": "s", "status": 1})).is_empty());
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
            vec![UiEvent::PlanMode { session: "s".into(), active: true }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("sandbox/mode", json!({"mode": "workspace-write"}))),
            vec![UiEvent::SandboxMode { session: "s".into(), mode: "workspace-write".into() }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("approval/policy", json!({"policy": "ask"}))),
            vec![UiEvent::ApprovalPolicy { session: "s".into(), policy: "ask".into() }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("permission/preset", json!({"preset": "danger-full-access"}))),
            vec![UiEvent::PermissionPreset { session: "s".into(), preset: "danger-full-access".into() }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("agent-preset/selected", json!({"agentPreset": "standard"}))),
            vec![UiEvent::AgentPreset { session: "s".into(), preset: "standard".into() }]
        );
    }

    #[test]
    fn approval_asked_and_decided_shapes() {
        assert_eq!(
            parse_notification(
                "session.event",
                &ev("approval/asked", json!({"id": "a1", "toolName": "bash", "reason": "rm -rf"}))
            ),
            vec![UiEvent::ApprovalAsked { session: "s".into(), tool: "bash".into(), reason: Some("rm -rf".into()) }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("approval/decided", json!({"id": "a1", "outcome": "allowed-once"}))),
            vec![UiEvent::ApprovalDecided { session: "s".into(), outcome: "allowed-once".into() }]
        );
        assert_eq!(
            parse_notification("session.event", &ev("approval/decided", json!({"id": "a1", "outcome": {"kind": "rejected"}}))),
            vec![UiEvent::ApprovalDecided { session: "s".into(), outcome: "rejected".into() }]
        );
        // malformed plan/mode stays silent
        assert!(parse_notification("session.event", &ev("plan/mode", json!({}))).is_empty());
    }
}
