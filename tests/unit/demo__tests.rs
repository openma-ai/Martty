use super::*;
use std::sync::mpsc;

/// Collect all Rpc events from a finished demo turn.
fn collect(prompt: &str) -> Vec<(String, Value)> {
    let (tx, rx) = mpsc::channel();
    run_demo_turn(tx, "s-test".to_string(), prompt.to_string());
    let mut out = Vec::new();
    // Sender is moved into the thread; channel closes when it finishes.
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(5)) {
        if let AppEvent::Rpc { method, params } = ev {
            out.push((method, params));
        }
    }
    out
}

fn event_type(params: &Value) -> &str {
    params["event"]["type"].as_str().unwrap_or("")
}

#[test]
fn first_running_last_idle() {
    let events = collect("show me main");
    let (first_m, first_p) = &events[0];
    assert_eq!(first_m, "session.status");
    assert_eq!(first_p["status"], "running");
    let (last_m, last_p) = events.last().unwrap();
    assert_eq!(last_m, "session.status");
    assert_eq!(last_p["status"], "idle");
}

#[test]
fn tool_call_precedes_tool_result() {
    let events = collect("show me main");
    let call = events
        .iter()
        .position(|(m, p)| m == "session.event" && event_type(p) == "tool/call")
        .expect("tool/call emitted");
    let result = events
        .iter()
        .position(|(m, p)| m == "session.event" && event_type(p) == "tool/result")
        .expect("tool/result emitted");
    assert!(call < result, "tool/call must precede tool/result");
}

#[test]
fn error_prompt_ends_turn_with_error() {
    let events = collect("trigger an error please");
    let (_, end) = events
        .iter()
        .find(|(m, p)| m == "session.event" && event_type(p) == "turn/end")
        .expect("turn/end emitted");
    assert_eq!(end["event"]["data"]["reason"]["kind"], "error");
}
