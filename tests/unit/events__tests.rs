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
    assert_eq!(
        ev,
        vec![UiEvent::SubagentFinished {
            child: "c".into(),
            failed: false,
        }]
    );
}

#[test]
fn metadata_only_subagent_lifecycle_updates_the_agent_dock() {
    let started = parse_notification(
        "session/update",
        &json!({
            "sessionId": "parent",
            "update": {
                "sessionUpdate": "session_info_update",
                "_meta": {"dsh": {
                    "event": "subagent/lifecycle",
                    "subagent": {
                        "state": "started",
                        "childSessionId": "child"
                    }
                }}
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

    let failed = parse_notification(
        "session/update",
        &json!({
            "sessionId": "parent",
            "update": {
                "sessionUpdate": "session_info_update",
                "_meta": {"dsh": {
                    "event": "subagent/lifecycle",
                    "subagent": {
                        "state": "finished",
                        "childSessionId": "child",
                        "stopReason": "error"
                    }
                }}
            }
        }),
    );
    assert_eq!(
        failed,
        vec![UiEvent::SubagentFinished {
            child: "child".into(),
            failed: true,
        }]
    );
}

#[test]
fn standard_acp_subagent_tool_calls_keep_the_request_block_and_lifecycle() {
    let started = parse_notification(
        "session/update",
        &json!({
            "sessionId": "parent",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "subagent:run-1",
                "title": "Start subagent child",
                "status": "in_progress",
                "rawInput": {"task": "inspect the renderer"},
                "_meta": {"dsh": {"subagent": {
                    "state": "started",
                    "childSessionId": "child"
                }}}
            }
        }),
    );
    assert_eq!(
        started,
        vec![
            UiEvent::ToolCall {
                session: "parent".into(),
                call_id: "subagent:run-1".into(),
                name: "Start subagent child".into(),
                arguments: r#"{"task":"inspect the renderer"}"#.into(),
            },
            UiEvent::SubagentStarted {
                parent: "parent".into(),
                child: "child".into(),
            },
        ]
    );

    let finished = parse_notification(
        "session/update",
        &json!({
            "sessionId": "parent",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "subagent:run-1",
                "status": "completed",
                "rawOutput": {"summary": "renderer inspected"},
                "_meta": {"dsh": {"subagent": {
                    "state": "finished",
                    "childSessionId": "child"
                }}}
            }
        }),
    );
    assert_eq!(
        finished,
        vec![
            UiEvent::ToolResult {
                session: "parent".into(),
                call_id: "subagent:run-1".into(),
                is_error: false,
                text: r#"{"summary":"renderer inspected"}"#.into(),
                error: None,
            },
            UiEvent::SubagentFinished {
                child: "child".into(),
                failed: false,
            },
        ]
    );
}

#[test]
fn standard_acp_nonterminal_tool_updates_refresh_the_existing_request() {
    let updates = parse_notification(
        "session/update",
        &json!({
            "sessionId": "parent",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "Subagent: inspect renderer",
                "status": "in_progress",
                "rawInput": {"task": "inspect renderer"}
            }
        }),
    );
    assert_eq!(
        updates,
        vec![UiEvent::ToolCall {
            session: "parent".into(),
            call_id: "call-1".into(),
            name: "Subagent: inspect renderer".into(),
            arguments: r#"{"task":"inspect renderer"}"#.into(),
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
    for t in ["block-end", "tool-call-delta", "finish"] {
        let ev = parse_notification(
            "session.event",
            &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                    "data": {"chunk": {"type": t}}}}),
        );
        assert!(ev.is_empty(), "chunk type {t} should emit nothing");
    }
}

#[test]
fn tool_call_block_start_exposes_hidden_argument_streaming() {
    let ev = parse_notification(
        "session.event",
        &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
        "data": {"chunk": {
            "type": "block-start",
            "blockType": "tool-call",
            "index": 1
        }}}}),
    );
    assert_eq!(
        ev,
        vec![UiEvent::ToolCallPreparing {
            session: "s".into()
        }]
    );

    let text_block = parse_notification(
        "session.event",
        &json!({"sessionId": "s", "event": {"type": "assistant/chunk",
                "data": {"chunk": {"type": "block-start", "blockType": "text"}}}}),
    );
    assert!(text_block.is_empty());
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
        serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap();
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
        serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap();
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
    assert!(ev
        .iter()
        .any(|e| matches!(e, UiEvent::PermissionPreset { preset, .. } if preset == "read-only")));

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
        "input": { "hint": "on|off" },
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
    assert_eq!(commands[0].input_hint.as_deref(), Some("on|off"));
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
        parse_notification("session.status", &json!({"sessionId": "s", "status": 1})).is_empty()
    );
    // assistant/chunk with missing chunk.
    assert!(parse_notification(
        "session.event",
        &json!({"sessionId": "s", "event": {"type": "assistant/chunk", "data": {}}})
    )
    .is_empty());
}
