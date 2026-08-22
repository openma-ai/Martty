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
