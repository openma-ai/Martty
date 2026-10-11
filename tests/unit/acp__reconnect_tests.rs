use super::*;
use agent_client_protocol::schema::v1::{
    AgentAuthCapabilities, AgentCapabilities, InitializeRequest, InitializeResponse,
    LogoutCapabilities, LogoutRequest, LogoutResponse, NewSessionResponse,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn test_cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: "/tmp".into(),
        provider: "deepseek-official".into(),
        model: "deepseek-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
        additional_directories: Vec::new(),
    }
}

async fn wait_event(rx: &Receiver<AppEvent>, predicate: impl Fn(&AppEvent) -> bool) {
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            while let Ok(event) = rx.try_recv() {
                seen.push(match &event {
                    AppEvent::Ctl(CtlEvent::OpenAuth) => "OpenAuth".to_string(),
                    AppEvent::Ctl(CtlEvent::Auth(_)) => "Auth".to_string(),
                    AppEvent::Ctl(CtlEvent::SessionBound { .. }) => "SessionBound".to_string(),
                    AppEvent::Ctl(CtlEvent::SessionError { message, .. }) => {
                        format!("SessionError({message})")
                    }
                    AppEvent::Ctl(CtlEvent::TuiOpFailed(message)) => {
                        format!("TuiOpFailed({message})")
                    }
                    AppEvent::Ctl(CtlEvent::TuiOpDone(message)) => {
                        format!("TuiOpDone({message})")
                    }
                    _ => "other".to_string(),
                });
                if predicate(&event) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("expected bus event; seen: {}", seen.join(" | ")));
}

fn counting_agent(
    inits: Arc<AtomicUsize>,
    news: Arc<AtomicUsize>,
    fail_first_session: bool,
    auth_methods: Vec<agent_client_protocol::schema::v1::AuthMethod>,
) -> impl ConnectTo<Client> {
    Agent
        .builder()
        .name("reconnect-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                inits.fetch_add(1, Ordering::SeqCst);
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .auth_methods(auth_methods.clone()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                let n = news.fetch_add(1, Ordering::SeqCst) + 1;
                if fail_first_session && n == 1 {
                    responder.respond_with_error(AcpError::new(
                        i32::from(AcpError::auth_required().code),
                        "sign in required",
                    ))
                } else {
                    responder.respond(NewSessionResponse::new(SessionId::new(format!("s{n}"))))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: PromptRequest, responder, _cx| {
                responder.respond(agent_client_protocol::schema::v1::PromptResponse::new(agent_client_protocol::schema::v1::StopReason::EndTurn))
            },
            on_receive_request!(),
        )
}

fn tty_login_method() -> agent_client_protocol::schema::v1::AuthMethod {
    let method = agent_client_protocol::schema::v1::AuthMethodTerminal::new(
        "tty-login",
        "TTY login",
    );
    agent_client_protocol::schema::v1::AuthMethod::Terminal(method)
}

/// Terminal sign-in ends in `Cmd::AgentReconnect`: the spawn loop must drop
/// the transport, spawn a fresh agent, re-run `initialize`, and re-bind a
/// session — the only spec-compliant continuation (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_sign_in_reconnect_respawns_and_reinitializes() {
    let inits = Arc::new(AtomicUsize::new(0));
    let news = Arc::new(AtomicUsize::new(0));
    let make = {
        let inits = Arc::clone(&inits);
        let news = Arc::clone(&news);
        move || -> std::result::Result<_, AcpError> {
            Ok(counting_agent(Arc::clone(&inits), Arc::clone(&news), true, vec![
                tty_login_method(),
            ]))
        }
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (fwd_tx, fwd_rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(spawn_agent_loop(
        make,
        test_cfg(),
        bus_tx,
        ConnectCarry::new(fwd_rx),
    ));

    // First connect: session/new is auth_required → the picker opens.
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    fwd_tx
        .send(Cmd::AgentReconnect { method_id: Some("tty-login".into()) })
        .expect("reconnect cmd");

    // Second connect: a brand-new initialize ran and session/new bound.
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    assert_eq!(inits.load(Ordering::SeqCst), 2, "one initialize per spawn");
    assert_eq!(news.load(Ordering::SeqCst), 2, "one session/new per connect");
    // The bind credits the launched method: a Configured snapshot names it
    // instead of a blind "sign-in needed" for the first advertised (#160).
    wait_event(&bus_rx, |e| {
        matches!(
            e,
            AppEvent::Ctl(CtlEvent::Auth(snapshot))
            | AppEvent::Ctl(CtlEvent::SessionAuth { snapshot, .. })
                if snapshot.status == crate::acp_auth::AuthStatus::Configured
                    && snapshot.method_id.as_deref() == Some("tty-login")
        )
    })
    .await;

    fwd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    task.await.expect("loop join").expect("loop result");
}

/// Attach transports cannot respawn (the FDs came from the caller), so an
/// AgentReconnect there retries `session/new` in place — the auth-required
/// startup path then re-opens the picker (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_reconnect_on_attach_retries_session_new_in_place() {
    let inits = Arc::new(AtomicUsize::new(0));
    let news = Arc::new(AtomicUsize::new(0));
    let agent = counting_agent(Arc::clone(&inits), Arc::clone(&news), true, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx.send(Cmd::AgentReconnect { method_id: None }).expect("reconnect cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    assert_eq!(inits.load(Ordering::SeqCst), 1, "attach keeps the same agent");
    assert_eq!(news.load(Ordering::SeqCst), 2, "session/new retried in place");

    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    client.await.expect("connect join").expect("connect result");
}

/// A successful ACP `logout` reply is surfaced as signed-out NeedsAuth state
/// and the method picker opens again (issue #160 — the user is never left on
/// a dead session).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_reply_surfaces_needs_auth_and_reopens_the_picker() {
    let (logout_tx, mut logout_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("logout-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new().auth(
                                AgentAuthCapabilities::new()
                                    .logout(LogoutCapabilities::new()),
                            ),
                        )
                        .auth_methods(vec![]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: LogoutRequest, responder, _cx| {
                let _ = logout_tx.send(());
                responder.respond(LogoutResponse::new())
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Logout).expect("logout cmd");

    // In order: NeedsAuth snapshot → OpenAuth → "signed out" op-done.
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::Auth(_)))
    })
    .await;
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::TuiOpDone(message)) if message.contains("signed out"))
    })
    .await;
    tokio::time::timeout(Duration::from_secs(2), logout_rx.recv())
        .await
        .expect("logout request reached the agent");

    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    client.await.expect("connect join").expect("connect result");
}

/// Signing out names the method the user signed in with — "signed out"
/// must not fall back to the bare type word "agent" (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_names_the_signed_in_method() {
    let agent = Agent
        .builder()
        .name("logout-named-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new().auth(
                                AgentAuthCapabilities::new()
                                    .logout(LogoutCapabilities::new()),
                            ),
                        )
                        .auth_methods(vec![
                            agent_client_protocol::schema::v1::AuthMethod::Agent(
                                agent_client_protocol::schema::v1::AuthMethodAgent::new(
                                    "api-key:deepseek",
                                    "DeepSeek API key",
                                ),
                            ),
                        ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: agent_client_protocol::schema::v1::AuthenticateRequest,
                        responder,
                        _cx| {
                responder.respond(
                    agent_client_protocol::schema::v1::AuthenticateResponse::new(),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: LogoutRequest, responder, _cx| {
                responder.respond(LogoutResponse::new())
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "api-key:deepseek".into(),
            values: Default::default(),
        })
        .expect("authenticate cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::Auth(snapshot))
            if snapshot.status == crate::acp_auth::AuthStatus::Configured)
    })
    .await;
    cmd_tx.send(Cmd::Logout).expect("logout cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::Auth(snapshot))
            if snapshot.status == crate::acp_auth::AuthStatus::NeedsAuth
                && snapshot.method_label() == Some("DeepSeek API key"))
    })
    .await;

    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    client.await.expect("connect join").expect("connect result");
}

/// A prompt typed while sign-in is pending is queued — never sent — and the
/// picker is re-opened so the user sees why nothing ran (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_typed_while_auth_pending_is_queued_not_sent() {
    let news = Arc::new(AtomicUsize::new(0));
    let agent = counting_agent(Arc::new(AtomicUsize::new(0)), Arc::clone(&news), true, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "hello".into(),
        })
        .expect("prompt cmd");
    // The picker is already open; the prompt parks without re-announcing
    // and without reaching the agent.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        news.load(Ordering::SeqCst),
        1,
        "only the startup session/new ran; the parked prompt stayed parked"
    );
    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    client.await.expect("connect join").expect("connect result");
}

/// Drive `run` over a host-attached TCP stream: the same connect machinery
/// (bridge + carry) works when the transport came from outside (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_attach_tcp_connects_and_shuts_down() {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).expect("connect");
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(run(
        test_cfg(),
        AcpEndpoint::AttachTcp(client),
        bus_tx,
        cmd_rx,
    ));

    let (agent, _) = listener.accept().expect("accept");
    let mut reader = BufReader::new(agent.try_clone().expect("clone"));
    let mut writer = agent;
    let mut line = String::new();
    reader.read_line(&mut line).expect("initialize");
    let req: serde_json::Value = serde_json::from_str(line.trim()).expect("json");
    assert_eq!(req["method"], "initialize");
    writeln!(
        writer,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "protocolVersion": req["params"]["protocolVersion"],
                "agentCapabilities": {},
                "authMethods": [],
                "agentInfo": { "name": "tcp-fake", "version": "0" }
            }
        })
    )
    .expect("initialize reply");
    line.clear();
    reader.read_line(&mut line).expect("session/new");
    let req: serde_json::Value = serde_json::from_str(line.trim()).expect("json");
    assert_eq!(req["method"], "session/new");
    writeln!(
        writer,
        "{}",
        json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "s1" } })
    )
    .expect("session reply");

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    drop((reader, writer));
    task.await.expect("run join").expect("run result");
}

/// `run` over caller-owned stdio fds (the unix attach path) — the TUI's own
/// TTY rides fds 3/4 while ACP stays on stdin/stdout (issue #160).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_attach_stdio_connects_and_shuts_down() {
    use std::io::{BufRead, BufReader, Write};

    use std::os::fd::{FromRawFd, IntoRawFd};
    let (client, agent_stream) = std::os::unix::net::UnixStream::pair().expect("pair");
    let client: std::fs::File = unsafe { std::fs::File::from_raw_fd(client.into_raw_fd()) };
    let incoming = client.try_clone().expect("clone fd");
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(run(
        test_cfg(),
        AcpEndpoint::AttachStdio {
            incoming,
            outgoing: client,
        },
        bus_tx,
        cmd_rx,
    ));

    let mut reader = BufReader::new(agent_stream.try_clone().expect("clone"));
    let mut writer = agent_stream;
    let mut line = String::new();
    reader.read_line(&mut line).expect("initialize");
    let req: serde_json::Value = serde_json::from_str(line.trim()).expect("json");
    writeln!(
        writer,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "protocolVersion": req["params"]["protocolVersion"],
                "agentCapabilities": {},
                "authMethods": [],
                "agentInfo": { "name": "fd-fake", "version": "0" }
            }
        })
    )
    .expect("initialize reply");
    line.clear();
    reader.read_line(&mut line).expect("session/new");
    let req: serde_json::Value = serde_json::from_str(line.trim()).expect("json");
    writeln!(
        writer,
        "{}",
        json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "s1" } })
    )
    .expect("session reply");

    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown cmd");
    drop((reader, writer));
    task.await.expect("run join").expect("run result");
}

/// A spawn argv that dies immediately fails the run instead of hanging the
/// client — the connect error propagates to the caller (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_spawn_fails_fast_when_the_agent_dies() {
    let (bus_tx, _bus_rx) = std::sync::mpsc::channel();
    let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        run(
            test_cfg(),
            AcpEndpoint::Spawn(vec!["sh".into(), "-c".into(), "exit 0".into()]),
            bus_tx,
            cmd_rx,
        ),
    )
    .await
    .expect("no hang");
    assert!(result.is_err(), "spawned agent that exits must error");
}

/// A prompt with no bound session and no pending sign-in is parked with a
/// visible "sign in to continue" notice — never sent (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_without_a_session_reports_sign_in_needed() {
    let agent = Agent
        .builder()
        .name("no-session-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32601, "no session support"))
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));

    // The session/new error lands first; a typed prompt then retries
    // session/new and surfaces the failure visibly.
    wait_event(&bus_rx, |e| {
        matches!(
            e,
            AppEvent::Ctl(CtlEvent::SessionError { .. }
                | CtlEvent::Error(_)
                | CtlEvent::ConnectionFailed { .. })
        )
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "hi".into(),
        })
        .expect("prompt");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::Error(message)) if message.contains("session/new"))
    })
    .await;

    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

#[test]
fn queue_connection_prompt_announces_only_while_auth_pends() {
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let mut pending: VecDeque<Cmd> = VecDeque::new();
    let methods: Vec<AuthMethodInfo> = Vec::new();
    queue_connection_prompt(
        &mut pending,
        Cmd::Prompt {
            session_id: String::new(),
            text: "queued".into(),
        },
        false,
        &bus_tx,
        &methods,
        None,
    );
    assert_eq!(pending.len(), 1);
    assert!(bus_rx.try_recv().is_err(), "no notice while not pending");
    queue_connection_prompt(
        &mut pending,
        Cmd::Prompt {
            session_id: String::new(),
            text: "still queued".into(),
        },
        true,
        &bus_tx,
        &methods,
        None,
    );
    assert_eq!(pending.len(), 2);
    assert!(matches!(
        bus_rx.recv().expect("auth event"),
        AppEvent::Ctl(CtlEvent::Auth(_))
    ));
    assert!(matches!(
        bus_rx.recv().expect("open event"),
        AppEvent::Ctl(CtlEvent::OpenAuth)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_cmd_bridge_forwards_and_stops_when_the_client_drops() {
    let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
    let mut bridge = spawn_cmd_bridge(rx);
    tx.send(Cmd::Shutdown).expect("send");
    let got = tokio::time::timeout(Duration::from_secs(2), bridge.recv())
        .await
        .expect("forwarded command");
    assert!(matches!(got, Some(Cmd::Shutdown)));
    drop(bridge);
    tx.send(Cmd::Shutdown).expect("still open channel");
}

/// The agent-side `logout` request can fail; the failure must surface in the
/// transcript instead of pretending the sign-out happened (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_failure_reports_tui_op_failed() {
    let agent = counting_agent(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), false, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Logout).expect("logout cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::TuiOpFailed(message)) if message.contains("logout"))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

fn always_auth_agent() -> impl ConnectTo<Client> {
    Agent
        .builder()
        .name("always-auth-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(
                    i32::from(AcpError::auth_required().code),
                    "sign in required",
                ))
            },
            on_receive_request!(),
        )
}

/// Attach-mode `AgentReconnect` retries `session/new` in place; an
/// auth_required reply keeps the client pending instead of failing (issue
/// #160 — the attach path cannot respawn the agent).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_reconnect_keeps_needs_auth_when_session_new_still_fails() {
    let agent = always_auth_agent();
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx.send(Cmd::AgentReconnect { method_id: None }).expect("reconnect cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

fn mixed_agent() -> impl ConnectTo<Client> {
    let n = Arc::new(AtomicUsize::new(0));
    Agent
        .builder()
        .name("mixed-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                if n.fetch_add(1, Ordering::SeqCst) == 0 {
                    responder.respond_with_error(AcpError::new(
                        i32::from(AcpError::auth_required().code),
                        "sign in required",
                    ))
                } else {
                    responder.respond_with_error(AcpError::new(-32601, "no sessions"))
                }
            },
            on_receive_request!(),
        )
}

/// The same in-place retry surfaces a non-auth `session/new` failure as a
/// visible error event (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_reconnect_reports_session_new_errors() {
    let agent = mixed_agent();
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx.send(Cmd::AgentReconnect { method_id: None }).expect("reconnect cmd");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::Error(message)) if message.contains("session/new"))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// Once a prompt is parked, the next sessionless prompt takes the
/// resolve→queue path and re-emits the needs-auth notice (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_prompt_while_auth_pending_reemits_the_picker() {
    let agent = always_auth_agent();
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "one".into(),
        })
        .expect("prompt");
    tokio::time::sleep(Duration::from_millis(200)).await;
    // second prompt (image payload): parked is non-empty, so it reaches the
    // resolve→queue path and re-announces auth.
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: String::new(),
            blocks: vec![crate::bus::PromptBlock::Text("two".into())],
        })
        .expect("prompt images");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "three".into(),
        })
        .expect("prompt");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::OpenAuth))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// The dynamic-plugin package id falls back from `currentPackageId` to
/// `activeRun.packageId` to the last advertised package, then errors.
#[test]
fn plugin_package_id_falls_back_through_every_arm() {
    let packages = json!([{"packageId": "p1"}, {"packageId": "p2"}]);
    let packages = packages.as_array().unwrap();
    let current = json!({"currentPackageId": "p0", "activeRun": {"packageId": "pr"}});
    assert_eq!(plugin_package_id(&current, packages, 0).unwrap(), "p0");
    let active = json!({"activeRun": {"packageId": "pr"}});
    assert_eq!(plugin_package_id(&active, packages, 0).unwrap(), "pr");
    let last = json!({"activeRun": {}});
    assert_eq!(plugin_package_id(&last, packages, 0).unwrap(), "p2");
    let missing = json!({});
    let empty: Vec<serde_json::Value> = vec![];
    assert!(plugin_package_id(&missing, &empty, 3)
        .unwrap_err()
        .contains("plugins[3] has no package"));
}

/// A relative workspace joins under `current_dir()` — the `joined` path is
/// not absolute in that arm.
#[test]
fn abs_fs_path_resolves_relative_workspace_against_cwd() {
    let abs = abs_fs_path("some/file.txt", "rel-workspace").expect("resolvable");
    assert!(abs.is_absolute());
    assert!(abs.ends_with(
        std::path::Path::new("rel-workspace").join("some/file.txt")
    ));
}

/// `plugin_op_is_primary` matches the tracked id on Ok and panicked
/// finishes, and rejects an empty join set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_op_is_primary_matches_ok_err_and_empty() {
    let mut set = tokio::task::JoinSet::new();
    let id = set.spawn(async { 7u8 }).id();
    let finished = set.join_next_with_id().await;
    assert!(plugin_op_is_primary(&finished, Some(id)));
    assert!(!plugin_op_is_primary(&finished, None));
    assert!(!plugin_op_is_primary::<u8>(&None, Some(id)));

    let mut failing = tokio::task::JoinSet::new();
    let panicking = failing.spawn(async { panic!("op panicked") }).id();
    let finished = failing.join_next_with_id().await;
    assert!(matches!(&finished, Some(Err(_))));
    assert!(plugin_op_is_primary(&finished, Some(panicking)));
}

/// A `session/update` notification reaches the `availableCommands` /
/// `available_commands` reader in either spelling, and updates without it
/// take the absent arm.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_update_notifications_feed_available_commands() {
    let agent = Agent
        .builder()
        .name("notify-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: PromptRequest, responder, cx| {
                use agent_client_protocol::schema::v1 as acpv1;
                cx.send_notification(acpv1::SessionNotification::new(
                    SessionId::new("s1"),
                    acpv1::SessionUpdate::AvailableCommandsUpdate(
                        acpv1::AvailableCommandsUpdate::new(vec![
                            acpv1::AvailableCommand::new("one", "d"),
                        ]),
                    ),
                ))
                .expect("commands notify");
                // An update without the key takes the absent arm.
                cx.send_notification(acpv1::SessionNotification::new(
                    SessionId::new("s1"),
                    acpv1::SessionUpdate::AgentMessageChunk(acpv1::ContentChunk::new(
                        acpv1::ContentBlock::Text(acpv1::TextContent::new("chunk")),
                    )),
                ))
                .expect("chunk notify");
                responder.respond(
                    agent_client_protocol::schema::v1::PromptResponse::new(
                        agent_client_protocol::schema::v1::StopReason::EndTurn,
                    ),
                )
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "go".into(),
        })
        .expect("prompt");
    // The prompt finishes once every session/update notification was seen.
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Rpc { method, params }
            if method == "session/update" && params.get("update").is_some())
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// A steer against the bound session reaches `begin_prompt` and settles.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_with_a_bound_session_begins_the_prompt() {
    let agent = counting_agent(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), false, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx
        .send(Cmd::Steer {
            session_id: String::new(),
            message_id: 9,
            text: "nudge".into(),
        })
        .expect("steer");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SteerSettled { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// Session ops resolve their target: an unknown id on an empty map is
/// skipped, and an unknown id with bound sessions reports SessionOpFailed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_ops_resolve_or_report_their_target() {
    let agent = counting_agent(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), false, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    // Before any session binds, an unknown target resolves to None → skip.
    cmd_tx
        .send(Cmd::SetConfigOption {
            session_id: "ghost".into(),
            config_id: "c".into(),
            value: "v".into(),
        })
        .expect("config op");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    // With a session bound, an unknown target reports a visible failure.
    cmd_tx
        .send(Cmd::SetPermission {
            session_id: "bogus".into(),
            preset: "p".into(),
        })
        .expect("permission op");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionOpFailed { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// SteerImages against the bound session reaches the image-payload arm of
/// `begin_prompt`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_images_with_a_bound_session_begins_the_prompt() {
    let agent = counting_agent(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), false, vec![]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx
        .send(Cmd::SteerImages {
            session_id: String::new(),
            message_id: 9,
            blocks: vec![crate::bus::PromptBlock::Image(crate::bus::ImagePart {
                data: "aGk=".into(),
                media_type: "image/png".into(),
                name: "i.png".into(),
                path: "/tmp/i.png".into(),
            })],
        })
        .expect("steer images");
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SteerSettled { .. }))
    })
    .await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}

/// A prompt whose session is forgotten while in flight finishes into the
/// missing-handle arm — its events are dropped, never parked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_finish_for_a_forgotten_session_is_dropped() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(Some(release_rx)));
    let agent = Agent
        .builder()
        .name("hold-mock")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let release_rx = std::sync::Arc::clone(&release_rx);
                async move |_req: PromptRequest, responder, _cx| {
                    let taken = release_rx.lock().unwrap().take();
                    if let Some(rx) = taken {
                        let _ = rx.await;
                    }
                    responder.respond(
                        agent_client_protocol::schema::v1::PromptResponse::new(
                            agent_client_protocol::schema::v1::StopReason::EndTurn,
                        ),
                    )
                }
            },
            on_receive_request!(),
        );
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(connect_once(agent, test_cfg(), bus_tx, cmd_rx));
    wait_event(&bus_rx, |e| {
        matches!(e, AppEvent::Ctl(CtlEvent::SessionBound { .. }))
    })
    .await;
    cmd_tx
        .send(Cmd::Prompt {
            session_id: String::new(),
            text: "hold me".into(),
        })
        .expect("prompt");
    tokio::time::sleep(Duration::from_millis(300)).await;
    cmd_tx
        .send(Cmd::ForgetSession {
            session_id: "s1".into(),
        })
        .expect("forget");
    tokio::time::sleep(Duration::from_millis(300)).await;
    release_tx.send(()).expect("release");
    tokio::time::sleep(Duration::from_millis(300)).await;
    cmd_tx.send(Cmd::Shutdown).expect("shutdown");
    client.await.expect("connect join").expect("connect result");
}
