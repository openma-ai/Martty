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

#[test]
fn continuous_streaming_has_a_frame_limit_and_flushes_the_last_update() {
    let start = Instant::now();
    let mut frames = FramePacer::new(start);
    let mut painted = 0;
    // An update every millisecond must not turn into 1000 frames/second.
    for millis in 0..1000 {
        let now = start + Duration::from_millis(millis);
        if frames.ready(now, true) {
            painted += 1;
            frames.painted(now);
        }
    }
    assert_eq!(painted, 31);

    let end = start + Duration::from_millis(1000);
    let wait = frames.wait(end, true);
    assert!(wait > Duration::ZERO && wait <= FRAME_INTERVAL);
    assert!(frames.ready(end + wait, true), "flush even if no more messages arrive");
    frames.painted(end + wait);
    assert_eq!(frames.wait(end + Duration::from_secs(1), false), IDLE_WAIT);
    assert!(!frames.ready(end + Duration::from_secs(1), false));
}

#[test]
fn fast_tool_result_waits_until_the_request_gets_an_immediate_frame() {
    let now = Instant::now();
    let mut frames = FramePacer::new(now);
    frames.painted(now);
    let (tx, rx) = mpsc::channel();
    let text = AppEvent::Ui(events::UiEvent::TextDelta {
        session: "root".into(), text: "working".into(),
    });
    tx.send(AppEvent::Ui(events::UiEvent::ToolCall {
        session: "root".into(), call_id: "fast".into(), name: "bash".into(), arguments: "{}".into(),
    })).unwrap();
    tx.send(AppEvent::Ui(events::UiEvent::ToolResult {
        session: "root".into(), call_id: "fast".into(), is_error: false, text: "done".into(), error: None,
    })).unwrap();

    let (batch, immediate) = collect_event_batch(text, &rx);
    assert_eq!(batch.len(), 2);
    assert!(matches!(batch.last(), Some(AppEvent::Ui(events::UiEvent::ToolCall { .. }))));
    assert!(!frames.ready(now, true));
    frames.immediate = immediate;
    assert!(frames.ready(now, true), "request bypasses streaming throttle");
    frames.painted(now);

    let (result, immediate) = collect_event_batch(rx.try_recv().unwrap(), &rx);
    assert!(matches!(result.as_slice(), [AppEvent::Ui(events::UiEvent::ToolResult { .. })]));
    assert!(!immediate);
    assert!(!frames.ready(now, true), "ordinary updates resume pacing after the request");
}

#[test]
fn input_interrupts_a_receive_burst_and_bypasses_the_frame_interval() {
    let now = Instant::now();
    let mut frames = FramePacer::new(now);
    frames.painted(now);
    let (tx, rx) = mpsc::channel();
    let input = AppEvent::Term(crossterm::event::Event::Key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'), crossterm::event::KeyModifiers::NONE,
        ),
    ));
    tx.send(AppEvent::Ui(events::UiEvent::TextDelta {
        session: "root".into(), text: "later".into(),
    })).unwrap();

    let (batch, immediate) = collect_event_batch(input, &rx);
    assert_eq!(batch.len(), 1);
    frames.immediate = immediate;
    assert!(frames.ready(now, true));
    assert_eq!(frames.wait(now, true), Duration::ZERO);
    assert!(rx.try_recv().is_ok(), "transport after input waits for its frame");
}

#[test]
fn receive_batches_yield_with_backlog_and_preserve_every_event() {
    let (tx, rx) = mpsc::channel();
    for n in 0..600 {
        tx.send(AppEvent::Ui(events::UiEvent::TextDelta {
            session: "root".into(), text: n.to_string(),
        })).unwrap();
    }
    let (first, immediate) = collect_event_batch(rx.try_recv().unwrap(), &rx);
    assert!(!immediate);
    assert!(first.len() < 600, "the UI must regain control before draining the backlog");
    let mut texts = Vec::new();
    for event in first.into_iter().chain(rx.try_iter()) {
        let AppEvent::Ui(events::UiEvent::TextDelta { text, .. }) = event else { panic!("expected delta") };
        texts.push(text);
    }
    assert_eq!(texts, (0..600).map(|n| n.to_string()).collect::<Vec<_>>());
}
