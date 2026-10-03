use super::*;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, ForkSessionRequest, ForkSessionResponse, Implementation, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse,
    SessionAdditionalDirectoriesCapabilities, SessionCapabilities, SessionForkCapabilities,
};
use std::time::Duration;

fn cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp/martty-workspace".into(),
        session_root: "/tmp".into(),
        provider: "deepseek-official".into(),
        model: "deepseek-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
        additional_directories: vec!["/tmp/martty-lib".into()],
    }
}

fn caps(fork: bool) -> AgentCapabilities {
    let mut session = SessionCapabilities::new();
    if fork {
        session = session
            .fork(SessionForkCapabilities::new())
            .additional_directories(SessionAdditionalDirectoriesCapabilities::new());
    }
    AgentCapabilities::new().session_capabilities(session)
}

#[test]
fn fork_capability_is_only_the_object_form() {
    assert!(fork_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "fork": {} } }
    })));
    assert!(fork_session_supported(&json!({
        "agent_capabilities": { "session_capabilities": { "fork": { "future": true } } }
    })));
    assert!(!fork_session_supported(&json!({
        "agentInfo": { "name": "pi-acp", "version": "9.9.9" },
        "agentCapabilities": { "sessionCapabilities": { "resume": {} } }
    })));
    assert!(!fork_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "fork": null } }
    })));
    assert!(!fork_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "fork": true } }
    })));
    assert!(!fork_session_supported(&json!({})));
}

#[test]
fn fork_params_match_session_load_and_carry_no_meta() {
    let cwd = std::path::Path::new("/tmp/martty-workspace");
    let dirs = vec!["/tmp/martty-lib".into()];
    let params = fork_session_params(SessionId::new("sess_789xyz"), cwd, true, &dirs);
    assert_eq!(params["sessionId"], "sess_789xyz");
    assert_eq!(params["cwd"], "/tmp/martty-workspace");
    assert_eq!(params["mcpServers"], json!([]));
    assert_eq!(params["additionalDirectories"], json!(["/tmp/martty-lib"]));
    assert!(params.get("_meta").is_none(), "{params}");
    assert!(params.get("messageId").is_none(), "{params}");
    assert!(params.get("message_id").is_none(), "{params}");

    let loaded = serde_json::to_value(load_session_request(
        SessionId::new("sess_789xyz"),
        cwd,
        true,
        &dirs,
    ))
    .unwrap();
    assert_eq!(loaded["cwd"], params["cwd"]);
    assert_eq!(loaded["mcpServers"], params["mcpServers"]);
    assert_eq!(
        loaded["additionalDirectories"],
        params["additionalDirectories"]
    );

    let omitted = fork_session_params(SessionId::new("s1"), cwd, false, &dirs);
    assert!(omitted.get("additionalDirectories").is_none(), "{omitted}");
    assert_eq!(omitted["mcpServers"], json!([]));

    let message = UntypedMessage::new("session/fork", &params).unwrap();
    assert_eq!(message.method(), "session/fork");
    assert_eq!(message.params()["mcpServers"], json!([]));
    assert_eq!(message.params()["sessionId"], "sess_789xyz");
    assert!(
        message.params().get("_meta").is_none(),
        "{}",
        message.params()
    );
    assert!(
        message.params().get("messageId").is_none(),
        "{}",
        message.params()
    );
}

async fn wait_fork_cap(bus_rx: &std::sync::mpsc::Receiver<AppEvent>) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::AgentCaps { fork_session, .. })) => return fork_session,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    panic!("initialize did not report agent capabilities");
}

fn spawn_agent(
    fork: bool,
    fail: bool,
) -> (
    tokio::task::JoinHandle<std::result::Result<(), AcpError>>,
    std::sync::mpsc::Receiver<AppEvent>,
    std::sync::mpsc::Sender<Cmd>,
    tokio::sync::mpsc::UnboundedReceiver<Value>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name(if fork { "fork-agent" } else { "plain-agent" })
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(caps(fork))
                        .agent_info(Implementation::new(
                            if fork { "fork-agent" } else { "plain-agent" },
                            "0",
                        )),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ForkSessionRequest, responder, _cx| {
                let _ = tx.send(serde_json::to_value(&request).unwrap_or(Value::Null));
                if fail {
                    responder.respond_with_error(AcpError::new(-32000, "fork refused by fixture"))
                } else {
                    responder.respond(ForkSessionResponse::new(SessionId::new("s-fork")))
                }
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg(), bus_tx, cmd_rx).await });
    (client, bus_rx, cmd_tx, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_fork_capability_is_reported_and_a_later_fork_still_surfaces_the_agent_error() {
    let (client, bus_rx, cmd_tx, mut seen) = spawn_agent(false, true);
    assert!(!wait_fork_cap(&bus_rx).await);
    cmd_tx
        .send(Cmd::ForkSession {
            source_session_id: "s1".into(),
            requester: "fork-tab".into(),
        })
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(2), seen.recv())
        .await
        .expect("session/fork")
        .expect("request");
    assert_eq!(request["sessionId"], "s1");
    assert_eq!(request["cwd"], "/tmp/martty-workspace");
    assert!(request.get("_meta").is_none(), "{request}");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut message = None;
    while std::time::Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::BindFailedTo {
                previous_id,
                message: text,
            })) => {
                assert_eq!(previous_id, "fork-tab");
                message = Some(text);
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    let message = message.expect("fork error must reach the UI");
    assert!(message.contains("fork refused by fixture"), "{message}");
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advertised_fork_returns_the_new_session_without_meta() {
    let (client, bus_rx, cmd_tx, mut seen) = spawn_agent(true, false);
    assert!(wait_fork_cap(&bus_rx).await);
    cmd_tx
        .send(Cmd::ForkSession {
            source_session_id: "s1".into(),
            requester: "fork-tab".into(),
        })
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(2), seen.recv())
        .await
        .expect("session/fork")
        .expect("request");
    assert_eq!(request["sessionId"], "s1");
    assert_eq!(request["cwd"], "/tmp/martty-workspace");
    assert_eq!(request["additionalDirectories"], json!(["/tmp/martty-lib"]));
    assert!(request.get("_meta").is_none(), "{request}");
    assert!(request.get("messageId").is_none(), "{request}");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut bound = None;
    while std::time::Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBoundTo {
                previous_id,
                session_id,
                ..
            })) => {
                assert_eq!(previous_id, "fork-tab");
                bound = Some(session_id);
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(bound.as_deref(), Some("s-fork"));
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}
