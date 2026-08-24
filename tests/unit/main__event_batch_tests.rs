use super::*;

#[test]
fn tool_call_is_a_render_boundary_before_a_fast_result_can_fold_it() {
    let event = AppEvent::Rpc {
        method: "session/update".into(),
        params: serde_json::json!({
            "sessionId": "root",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "call-1",
                "title": "Subagent: inspect repo",
                "status": "in_progress",
                "rawInput": {"prompt": "inspect repo"}
            }
        }),
    };

    assert!(event_requires_immediate_frame(&event));
}

#[test]
fn parallel_tool_results_and_unrelated_updates_remain_batchable() {
    let result = AppEvent::Rpc {
        method: "session/update".into(),
        params: serde_json::json!({
            "sessionId": "root",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "status": "completed"
            }
        }),
    };
    let text = AppEvent::Rpc {
        method: "session/update".into(),
        params: serde_json::json!({
            "sessionId": "root",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "done"}
            }
        }),
    };

    assert!(!event_requires_immediate_frame(&result));
    assert!(!event_requires_immediate_frame(&text));
}

#[test]
fn decoded_and_legacy_tool_calls_use_the_same_frame_boundary() {
    let decoded = AppEvent::Ui(crate::events::UiEvent::ToolCall {
        session: "root".into(),
        call_id: "call-1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    let legacy = AppEvent::Rpc {
        method: "session.event".into(),
        params: serde_json::json!({
            "sessionId": "root",
            "event": {"type": "tool/call", "data": {"callId": "call-2"}}
        }),
    };

    assert!(event_requires_immediate_frame(&decoded));
    assert!(event_requires_immediate_frame(&legacy));
}
