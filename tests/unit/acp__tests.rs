use super::*;
use serde_json::json;

#[test]
fn session_setup_keeps_current_mode_when_the_mode_catalog_is_empty() {
    let (bus, events) = std::sync::mpsc::channel();
    let mut surface = Surface::default();

    surface.apply_session_modes(
        &json!({
            "currentModeId": "danger-full-access",
            "availableModes": []
        }),
        &bus,
        None,
    );

    match events.try_recv() {
        Ok(AppEvent::Ctl(CtlEvent::SessionModes {
            modes,
            current,
            ..
        })) => {
            assert!(modes.is_empty());
            assert_eq!(current.as_deref(), Some("danger-full-access"));
        }
        _ => panic!("expected the authoritative current mode"),
    }
}

#[test]
fn codex_thought_level_populates_the_effort_catalog() {
    let (bus, _events) = std::sync::mpsc::channel();
    let mut surface = Surface::default();

    surface.apply_config_options(
        &json!([{
            "type": "select",
            "id": "reasoning_effort",
            "category": "thought_level",
            "currentValue": "high",
            "options": [
                {"value": "low", "name": "Low"},
                {"value": "medium", "name": "Medium"},
                {"value": "high", "name": "High"},
                {"value": "xhigh", "name": "Xhigh"},
                {"value": "max", "name": "Max"},
                {"value": "ultra", "name": "Ultra"}
            ]
        }]),
        &bus,
        Some("codex-session"),
    );

    assert_eq!(
        surface.session("codex-session").efforts,
        ["low", "medium", "high", "xhigh", "max", "ultra"]
    );
}

#[test]
fn dynamic_plugin_inventory_uses_the_backend_current_package_and_run_state() {
    let plugins = dynamic_plugins_from_value(&json!([
        {
            "pluginId": "panel-1",
            "currentPackageId": "pkg-current",
            "activeRun": { "pluginRunId": "run-1", "packageId": "pkg-current" },
            "packages": [
                { "packageId": "pkg-old", "name": "Old panel" },
                { "packageId": "pkg-current", "name": "Current panel" }
            ]
        },
        {
            "pluginId": "theme-1",
            "currentPackageId": "pkg-theme",
            "packages": [{ "packageId": "pkg-theme", "name": "Clay theme" }]
        }
    ]))
    .expect("valid backend inventory");

    assert_eq!(
        plugins,
        vec![
            crate::bus::CordisPluginItem {
                id: "panel-1".into(),
                name: "Current panel".into(),
                package_id: "pkg-current".into(),
                status: "running".into(),
                approval_request_id: None,
            },
            crate::bus::CordisPluginItem {
                id: "theme-1".into(),
                name: "Clay theme".into(),
                package_id: "pkg-theme".into(),
                status: "stopped".into(),
                approval_request_id: None,
            },
        ]
    );
}

#[test]
fn static_plugin_inventory_parses_the_web_snapshot_shape() {
    let plugins = static_plugins_from_value(&json!({
        "entries": [
            {
                "entryId": "root/include",
                "moduleName": "include",
                "enabled": true,
                "fiberPhase": "active"
            },
            {
                "entryId": "root/hmr",
                "moduleName": "hmr",
                "enabled": false,
                "fiberPhase": null
            }
        ]
    }))
    .expect("valid static inventory");

    assert_eq!(
        plugins,
        vec![
            crate::bus::StaticPluginItem {
                entry_id: "root/include".into(),
                module_name: "include".into(),
                enabled: true,
                fiber_phase: Some("active".into()),
            },
            crate::bus::StaticPluginItem {
                entry_id: "root/hmr".into(),
                module_name: "hmr".into(),
                enabled: false,
                fiber_phase: None,
            },
        ]
    );
}

#[test]
fn session_setup_emits_initial_collaboration_mode_fact() {
    let surface = Arc::new(Mutex::new(Surface::default()));
    let (tx, rx) = std::sync::mpsc::channel();

    apply_setup(
        &json!({
            "sessionId": "s1",
            "configOptions": [{
                "type": "select",
                "id": "collaboration_mode",
                "currentValue": "plan",
                "options": [
                    {"value": "default", "name": "Default"},
                    {"value": "plan", "name": "Plan"}
                ]
            }]
        }),
        None,
        &surface,
        &tx,
    );

    assert!(rx.try_iter().any(|event| matches!(
        event,
        AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active })
            if session == "s1" && active
    )));
}

#[test]
fn prompt_response_usage_reaches_the_ui() {
    let response = serde_json::from_value(json!({
        "stopReason": "end_turn",
        "usage": {
            "totalTokens": 135,
            "inputTokens": 100,
            "outputTokens": 20,
            "thoughtTokens": 5,
            "cachedReadTokens": 10
        }
    }))
    .expect("standard ACP prompt response");
    let finish = PromptFinish {
        session_id: "s".into(),
        result: Ok(response),
        payload: ParkedPromptKind::Text("hello".into()),
        gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();

    apply_prompt_finish(finish, &mut HashMap::new(), &mut parked, &tx, &[], None);

    match rx.try_recv().expect("standard usage UI event") {
        AppEvent::Ui(crate::events::UiEvent::Usage {
            session,
            input,
            output,
            cached,
            reasoning,
        }) => {
            assert_eq!(session, "s");
            assert_eq!((input, output, cached, reasoning), (100, 20, 10, 5));
        }
        _ => panic!("expected standard usage UI event"),
    }
    match rx.try_recv().expect("prompt completion UI event") {
        AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }) => {
            assert_eq!(session, "s");
            assert_eq!(kind, "completed");
        }
        _ => panic!("expected prompt completion UI event"),
    }
}

#[test]
fn prompt_error_ends_the_ui_turn_before_reporting_the_error() {
    let finish = PromptFinish {
        session_id: "s".into(),
        result: Err(serde_json::from_value(json!({
            "code": -32603, "message": "boom",
            "data": { "marttyConnection": { "id": "h3", "authMethods": [] } }
        })).unwrap()),
        payload: ParkedPromptKind::Text("hello".into()),
        gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();

    apply_prompt_finish(finish, &mut HashMap::new(), &mut parked, &tx, &[], None);

    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
            if session == "s" && kind == "error"
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ctl(CtlEvent::SessionError { session_id, message }))
            if session_id == "s" && message == "prompt: boom"
    ));
}

#[test]
fn a_refused_retry_is_dropped_visibly_not_reparked() {
    // The requeued prompt earned one retry at sign-in; a second -32000 is
    // the agent's answer to the message itself — report and drop it, or the
    // held entry stalls this session's queue forever (#160).
    let mut sessions = HashMap::from([("s".into(), SessionHandle::default())]);
    sessions.get_mut("s").unwrap().auth_retries = 1;
    let finish = PromptFinish {
        session_id: "s".into(),
        result: Err(serde_json::from_value(json!({
            "code": -32000, "message": "auth required",
            "data": { "marttyConnection": { "id": "h1", "authMethods": [] } }
        })).unwrap()),
        payload: ParkedPromptKind::Text("auth-gated".into()),
        gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();

    apply_prompt_finish(finish, &mut sessions, &mut parked, &tx, &[], None);

    assert!(parked.is_empty());
    assert_eq!(sessions["s"].auth_retries, 0);
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ctl(CtlEvent::SessionError { session_id, message }))
            if session_id == "s" && message.contains("still refused after sign-in")
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
            if session == "s" && kind == "interrupted"
    ));

    // With no retry token a first refusal still parks on its session.
    let finish = PromptFinish {
        session_id: "s".into(),
        result: Err(serde_json::from_value(json!({
            "code": -32000, "message": "auth required",
            "data": { "marttyConnection": { "id": "h1", "authMethods": [] } }
        })).unwrap()),
        payload: ParkedPromptKind::Text("plain".into()),
        gen: 1,
    };
    apply_prompt_finish(finish, &mut sessions, &mut parked, &tx, &[], None);
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].session.as_deref(), Some("s"));
}

#[test]
fn terminal_sign_in_credit_names_the_launched_method() {
    let surface = Arc::new(Mutex::new(Surface::default()));
    let mut methods = Vec::new();
    for raw in [
        json!({"id": "other", "type": "terminal", "_meta": {"terminal-auth": {}}}),
        json!({"id": "tty-login", "type": "terminal", "_meta": {"terminal-auth": {}}}),
    ] {
        methods.extend(parse_auth_methods(&json!([raw]), &["fake-agent".into()], ".", &process_env()));
    }
    let (tx, rx) = std::sync::mpsc::channel();

    // No launched id: silent.
    credit_terminal_method(&tx, &surface, &methods, &None);
    assert!(rx.try_recv().is_err());
    // A launched id the agent does not advertise: silent.
    credit_terminal_method(&tx, &surface, &methods, &Some("ghost".into()));
    assert!(rx.try_recv().is_err());
    // The launched method: the snapshot becomes Configured carrying it.
    credit_terminal_method(&tx, &surface, &methods, &Some("tty-login".into()));
    match rx.try_recv() {
        Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
        | Ok(AppEvent::Ctl(CtlEvent::SessionAuth { snapshot, .. })) => {
            assert_eq!(snapshot.status, AuthStatus::Configured);
            assert_eq!(snapshot.method_id.as_deref(), Some("tty-login"));
        }
        other => panic!("expected an auth snapshot, got {}", other.is_ok()),
    }
}

#[test]
fn initialize_advertises_backchat_auth_caps() {
    let value = serde_json::to_value(initialize_request()).expect("serialize");
    let caps = &value["clientCapabilities"];
    assert_eq!(caps["_meta"]["terminal-auth"], true);
    assert_eq!(caps["_meta"]["terminal_output"], true);
    assert_eq!(caps["_meta"]["subagent-transcript"], true);
    assert_eq!(caps["_meta"]["dsh"]["cordis"]["protocol"], 0);
    assert_eq!(caps["_meta"]["jetbrains"]["air"]["version"], 1);
    assert_eq!(
        caps["_meta"]["jetbrains"]["air"]["capabilities"],
        json!(["sessionFailure"])
    );
    assert_eq!(caps["auth"]["terminal"], true);
    assert_eq!(caps["auth"]["_meta"]["gateway"], true);
    assert_eq!(caps["session"]["configOptions"], json!({}));
    assert_eq!(caps["elicitation"]["form"], json!({}));
    assert_eq!(caps["fs"]["readTextFile"], true);
    assert_eq!(caps["fs"]["writeTextFile"], true);
    assert_eq!(caps["terminal"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_ui_events_are_compositor_notifications_not_prompts() {
    use agent_client_protocol::schema::v1::{AgentCapabilities, InitializeResponse};
    use std::time::Duration;

    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let request_tx = extension_tx.clone();
    let agent = Agent
        .builder()
        .name("plugin-command-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, cx| {
                let mut meta = serde_json::Map::new();
                meta.insert(
                    "dsh".into(),
                    json!({ "cordis": { "protocol": crate::cordis::PROTOCOL } }),
                );
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new().meta(meta))
                        .agent_info(Implementation::new("plugin-command-mock", "0")),
                )?;
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::COMMANDS_UPDATE,
                    json!({ "protocol": 0, "commands": [] }),
                )?)?;
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::OVERLAY_UPDATE,
                    json!({ "protocol": 0, "overlay": null }),
                )?)
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: UntypedMessage, responder, _cx| {
                if matches!(
                    request.method(),
                    crate::cordis::COMMAND_INVOKE
                        | crate::cordis::THEME_SELECTED
                        | crate::cordis::OVERLAY_EVENT
                        | crate::cordis::AGENTS_UPDATE
                        | crate::cordis::SESSION_ACTIVE
                ) {
                    let _ = request_tx.send((
                        "request",
                        request.method().to_string(),
                        request.params().clone(),
                    ));
                }
                responder.respond(json!({ "handled": true }))
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |notification: UntypedMessage, _cx| {
                if notification.method() == crate::cordis::OVERLAY_EVENT {
                    let _ = extension_tx.send((
                        "notification",
                        notification.method().to_string(),
                        notification.params().clone(),
                    ));
                }
                Ok(())
            },
            on_receive_notification!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut compositor_methods = std::collections::HashSet::new();
    while std::time::Instant::now() < deadline && compositor_methods.len() < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Rpc { method, .. })
                if matches!(
                    method.as_str(),
                    crate::cordis::COMMANDS_UPDATE | crate::cordis::OVERLAY_UPDATE
                ) =>
            {
                compositor_methods.insert(method);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert_eq!(
        compositor_methods,
        std::collections::HashSet::from([
            crate::cordis::COMMANDS_UPDATE.to_string(),
            crate::cordis::OVERLAY_UPDATE.to_string(),
        ])
    );

    cmd_tx
        .send(Cmd::InvokePluginCommand {
            name: "threshold".into(),
            args: String::new(),
        })
        .expect("invoke plugin command");

    let (kind, method, params) = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("command request should reach the compositor plane")
        .expect("extension channel");
    assert_eq!(kind, "request");
    assert_eq!(method, crate::cordis::COMMAND_INVOKE);
    assert_eq!(
        params,
        json!({
            "protocol": 0,
            "name": "threshold",
            "args": ""
        })
    );

    cmd_tx
        .send(Cmd::PluginThemeSelected {
            agent_id: "agent-1".into(),
            id: "night-lime".into(),
        })
        .expect("select theme Plugin");
    let (kind, method, params) = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("theme Plugin selection should reach the compositor plane")
        .expect("extension channel");
    assert_eq!(kind, "request");
    assert_eq!(method, crate::cordis::THEME_SELECTED);
    assert_eq!(
        params,
        json!({
            "protocol": 0,
            "agentId": "agent-1",
            "id": "night-lime"
        })
    );

    cmd_tx
        .send(Cmd::PluginOverlayEvent {
            id: "threshold".into(),
            event: "change".into(),
            value: Some(json!(16.0)),
        })
        .expect("send slider event");
    let (kind, method, params) = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("overlay event should reach the compositor plane")
        .expect("extension channel");
    assert_eq!(kind, "notification");
    assert_eq!(method, crate::cordis::OVERLAY_EVENT);
    assert_eq!(
        params,
        json!({
            "protocol": 0,
            "id": "threshold",
            "event": "change",
            "value": 16.0
        })
    );
    for event in ["submit", "cancel"] {
        cmd_tx
            .send(Cmd::PluginOverlayEvent {
                id: "threshold".into(),
                event: event.into(),
                value: Some(json!(32.0)),
            })
            .expect("send terminal slider event");
        let (kind, method, params) =
            tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
                .await
                .expect("terminal overlay event should reach the compositor plane")
                .expect("extension channel");
        assert_eq!(kind, "request");
        assert_eq!(method, crate::cordis::OVERLAY_EVENT);
        assert_eq!(params["event"], event);
    }
    cmd_tx
        .send(Cmd::AgentsSnapshot {
            snapshot: crate::bus::AgentsSnapshot {
                active_id: "child-1".into(),
                selected_id: None,
                items: vec![
                    crate::bus::AgentsSnapshotItem {
                        id: "s1".into(),
                        label: "main".into(),
                        kind: "main".into(),
                        status: "idle".into(),
                        current: false,
                    },
                    crate::bus::AgentsSnapshotItem {
                        id: "child-1".into(),
                        label: "subagent 1".into(),
                        kind: "subagent".into(),
                        status: "running".into(),
                        current: true,
                    },
                ],
            },
        })
        .expect("publish Agent navigation");
    let (kind, method, params) = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("Agent navigation should reach the compositor plane")
        .expect("extension channel");
    assert_eq!(kind, "request");
    assert_eq!(method, crate::cordis::AGENTS_UPDATE);
    assert_eq!(
        params,
        json!({
            "protocol": 0,
            "activeId": "child-1",
            "selectedId": null,
            "items": [
                { "id": "s1", "label": "main", "kind": "main", "status": "idle", "current": false },
                { "id": "child-1", "label": "subagent 1", "kind": "subagent", "status": "running", "current": true }
            ]
        })
    );
    cmd_tx
        .send(Cmd::ActiveSession {
            session_id: Some("s1".into()),
        })
        .expect("publish active Session");
    let (kind, method, params) = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("active Session should reach the compositor plane")
        .expect("extension channel");
    assert_eq!(kind, "request");
    assert_eq!(method, crate::cordis::SESSION_ACTIVE);
    assert_eq!(params, json!({ "protocol": 0, "sessionId": "s1" }));
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cordis_requests_stay_local_when_the_agent_did_not_advertise_cordis() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse,
    };
    use std::time::Duration;

    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("standard-acp-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("standard-acp-agent", "0")),
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
            async move |request: UntypedMessage, responder, _cx| {
                let _ = extension_tx.send(request.method().to_string());
                responder.respond(json!({ "handled": true }))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    cmd_tx
        .send(Cmd::InvokePluginCommand {
            name: "threshold".into(),
            args: String::new(),
        })
        .expect("invoke plugin command");

    assert!(
        tokio::time::timeout(Duration::from_millis(100), extension_rx.recv())
            .await
            .is_err(),
        "a standard ACP agent must not receive an unadvertised Cordis request",
    );
    assert!(
        (0..20).any(|_| matches!(
            bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Ctl(CtlEvent::TuiOpFailed(message)))
                if message.contains("client compositor is unavailable")
        )),
        "the client should report that the local compositor is unavailable",
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_compositor_catalog_does_not_require_agent_cordis_capability() {
    use agent_client_protocol::schema::v1::{AgentCapabilities, InitializeResponse};
    use std::time::Duration;

    let agent = Agent
        .builder()
        .name("standard-acp-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("standard-acp-agent", "0")),
                )?;
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::COMMANDS_UPDATE,
                    json!({
                        "protocol": 0,
                        "commands": [{
                            "name": "ui",
                            "description": "Switch UI preset"
                        }]
                    }),
                )?)
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut catalog = None;
    while std::time::Instant::now() < deadline && catalog.is_none() {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Rpc { method, params }) if method == crate::cordis::COMMANDS_UPDATE => {
                catalog = Some(params);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert_eq!(catalog.unwrap()["commands"][0]["name"], "ui");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_compositor_command_does_not_require_agent_cordis_capability() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse,
    };
    use std::time::Duration;

    let (invoke_tx, mut invoke_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("standard-acp-with-client-compositor")
        .on_receive_request(
            async move |init: InitializeRequest, responder, cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new(
                            "standard-acp-with-client-compositor",
                            "0",
                        )),
                )?;
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::COMMANDS_UPDATE,
                    json!({
                        "protocol": 0,
                        "commands": [{
                            "name": "harness",
                            "description": "Switch Harness"
                        }]
                    }),
                )?)
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
            async move |request: UntypedMessage, responder, _cx| {
                let _ = invoke_tx.send((request.method().to_string(), request.params().clone()));
                responder.respond(json!({ "ok": true }))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if matches!(
            bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Rpc { method, .. }) if method == crate::cordis::COMMANDS_UPDATE
        ) {
            break;
        }
    }
    cmd_tx
        .send(Cmd::InvokePluginCommand {
            name: "harness".into(),
            args: "path-dsh-acp".into(),
        })
        .expect("invoke local Client command");

    let (method, params) = tokio::time::timeout(Duration::from_secs(1), invoke_rx.recv())
        .await
        .expect("local compositor invocation should not be blocked by agent capability")
        .expect("invocation request");
    assert_eq!(method, crate::cordis::COMMAND_INVOKE);
    assert_eq!(params["name"], "harness");
    assert_eq!(params["args"], "path-dsh-acp");

    cmd_tx.send(Cmd::ActiveSession { session_id: Some("s1".into()) }).unwrap();
    let active = tokio::time::timeout(Duration::from_secs(1), invoke_rx.recv()).await;
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
    let (method, params) = active.expect("local session projection is independent of Agent Cordis")
        .expect("active session projection request");
    assert_eq!(method, crate::cordis::SESSION_ACTIVE);
    assert_eq!(params["sessionId"], "s1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_compositor_command_survives_a_failed_initialize() {
    use std::time::Duration;

    let (invoke_tx, mut invoke_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("init-fails-agent")
        .on_receive_request(
            async move |_init: InitializeRequest, responder, cx| {
                // The local compositor plane may already be up (the muxed
                // attach channel publishes its catalog before the agent
                // handshake settles). Mirror that ordering, then fail.
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::COMMANDS_UPDATE,
                    json!({
                        "protocol": 0,
                        "commands": [{
                            "name": "status",
                            "description": "Session run state and key stats"
                        }]
                    }),
                )?)?;
                responder.respond_with_error(AcpError::new(
                    -32603,
                    "agent process exited before completing initialize",
                )
                .data(json!({ "agentExited": true })))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: UntypedMessage, responder, _cx| {
                let _ = invoke_tx.send((request.method().to_string(), request.params().clone()));
                responder.respond(json!({ "ok": true }))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        assert!(std::time::Instant::now() < deadline, "ConnectionFailed never arrived");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::ConnectionFailed { error, .. })) => {
                assert!(error.contains("agent process exited"), "unexpected failure: {error}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    cmd_tx
        .send(Cmd::InvokePluginCommand {
            name: "status".into(),
            args: String::new(),
        })
        .expect("invoke local Client command after the failed handshake");

    let (method, params) = tokio::time::timeout(Duration::from_secs(1), invoke_rx.recv())
        .await
        .expect("a failed initialize must not kill the compositor channel")
        .expect("invocation request");
    assert_eq!(method, crate::cordis::COMMAND_INVOKE);
    assert_eq!(params["name"], "status");

    // Local compositor projections still publish: the queue snapshot carries
    // the dead-attach flag so the view can stop promising "send first".
    cmd_tx
        .send(Cmd::QueueSnapshot {
            snapshot: crate::bus::QueueSnapshot {
                count: 1,
                items: vec![crate::bus::QueueSnapshotItem {
                    id: 3,
                    ordinal: 1,
                    summary: "held prompt".into(),
                }],
                selected_id: None,
                editing_id: None,
                delete_confirm: false,
                attach_failed: true,
                auth_pending: false,
            },
        })
        .expect("queue snapshot while detached");
    let (method, params) = tokio::time::timeout(Duration::from_secs(1), invoke_rx.recv())
        .await
        .expect("queue projection should reach the wire after a failed initialize")
        .expect("queue projection request");
    assert_eq!(method, crate::cordis::QUEUE_UPDATE);
    assert_eq!(params["attachFailed"], true);
    assert_eq!(params["authPending"], false);
    assert_eq!(params["items"][0]["summary"], "held prompt");

    // A prompt in the detached state must fail fast, not park the loop
    // behind a `session/new` that can never be answered.
    cmd_tx
        .send(Cmd::Prompt { session_id: String::new(), text: "hello".into() })
        .expect("prompt while detached");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        assert!(std::time::Instant::now() < deadline, "detached prompt never settled");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Error(message))) => {
                assert!(message.contains("agent is not attached"), "unexpected error: {message}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_prompt_after_a_refused_startup_session_new_retries_the_bind() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    // Startup `session/new` is refused with a non-auth error: the agent is
    // still attached, so the next prompt retries the bind inline instead of
    // failing like `agent_absent` or parking like `session_auth_pending`.
    let news = Arc::new(AtomicUsize::new(0));
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("retry-bind-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("retry-bind-agent", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let news = Arc::clone(&news);
                async move |_req: NewSessionRequest, responder, _cx| {
                    if news.fetch_add(1, Ordering::SeqCst) == 0 {
                        responder.respond_with_error(AcpError::new(
                            -32603,
                            "startup session/new refused",
                        ))
                    } else {
                        responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest, responder, _cx| {
                let _ = prompt_tx.send(request.session_id.to_string());
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < deadline, "startup refusal never surfaced");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::ConnectionFailed { error, .. })) => {
                assert!(error.contains("session/new"), "unexpected failure: {error}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt { session_id: String::new(), text: "hello".into() })
        .expect("prompt after the refused startup bind");
    let session = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("prompt lands on the wire once the inline session/new retry binds")
        .expect("prompt request");
    assert_eq!(session, "s1");
    assert_eq!(news.load(Ordering::SeqCst), 2, "one startup attempt plus one inline retry");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[test]
fn init_was_answered_tells_an_agent_reply_from_a_dead_transport() {
    // Any error the agent itself authored counts as answered — it is
    // attached and may still authenticate — whatever its code or data.
    assert!(init_was_answered(&AcpError::new(-32603, "refused")));
    assert!(init_was_answered(
        &AcpError::new(-32000, "authentication required")
            .data(json!({ "authMethods": [{ "id": "oauth-x", "name": "Sign in" }] }))
    ));
    // A `-32001` reply carrying agent data stays answered even though our
    // own deadline uses the same code.
    assert!(init_was_answered(
        &AcpError::new(-32001, "unsupported client").data(json!({ "reason": "x" }))
    ));
    // Locally produced failures are not replies: the bare `-32001` our
    // deadline synthesizes, the pool's child-exit marker, transport EOF.
    assert!(!init_was_answered(&AcpError::new(-32001, "agent request timed out")));
    assert!(!init_was_answered(
        &AcpError::new(-32603, "exited").data(json!({ "agentExited": true }))
    ));
    let closed = AcpError::internal_error().data(json!({
        "reason": agent_client_protocol::INCOMING_TRANSPORT_CLOSED_REASON,
        "method": "initialize",
    }));
    assert!(!init_was_answered(&closed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_initialize_with_auth_methods_parks_prompts_for_sign_in() {
    use agent_client_protocol::schema::v1::{
        AuthenticateRequest, AuthenticateResponse, NewSessionRequest, NewSessionResponse,
        PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    // The agent answers `initialize` with an auth-required refusal that
    // declares its methods — it is attached and can authenticate, so
    // prompts park behind sign-in instead of failing fast.
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("init-refused-auth-agent")
        .on_receive_request(
            async move |_init: InitializeRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32000, "sign in first").data(json!({
                    "authMethods": [
                        { "id": "oauth-x", "name": "Sign in with X", "description": "OAuth sign-in" }
                    ]
                })))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: AuthenticateRequest, responder, _cx| {
                responder.respond(AuthenticateResponse::new())
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
            async move |request: PromptRequest, responder, _cx| {
                let _ = prompt_tx.send(request.session_id.to_string());
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // The refusal surfaces as a refused (still attached) connect, then
    // sign-in is offered with the methods the agent declared in its
    // error data.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut refused = false;
    let mut offered = false;
    while Instant::now() < deadline && !(refused && offered) {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::ConnectionRefused { error, .. })) => {
                assert!(error.contains("sign in first"), "unexpected refusal: {error}");
                refused = true;
            }
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => {
                if snapshot.status == AuthStatus::NeedsAuth
                    && snapshot.methods.iter().any(|method| method.id == "oauth-x")
                {
                    offered = true;
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(refused && offered, "refused initialize must surface the refusal and the sign-in methods");

    // A prompt before sign-in parks instead of touching the wire or
    // erroring like a dead attach.
    cmd_tx
        .send(Cmd::Prompt { session_id: String::new(), text: "hello".into() })
        .expect("prompt while auth is pending");
    assert!(
        tokio::time::timeout(Duration::from_millis(300), prompt_rx.recv()).await.is_err(),
        "prompt must park while authentication is pending"
    );

    // Sign in, then the parked intent binds and lands on the wire.
    cmd_tx
        .send(Cmd::Authenticate { method_id: "oauth-x".into(), values: Default::default() })
        .expect("authenticate");
    let session = tokio::time::timeout(Duration::from_secs(3), prompt_rx.recv())
        .await
        .expect("parked prompt drains once authenticate lets session/new bind")
        .expect("prompt request");
    assert_eq!(session, "s1");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_initialize_for_auth_without_methods_still_fast_fails() {
    use std::time::{Duration, Instant};

    // An auth-required refusal with no declared methods can never be
    // remedied — prompts short-circuit exactly like a dead attach.
    let agent = Agent
        .builder()
        .name("init-refused-no-methods")
        .on_receive_request(
            async move |_init: InitializeRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32000, "sign in first"))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < deadline, "ConnectionFailed never arrived");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::ConnectionFailed { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt { session_id: String::new(), text: "hello".into() })
        .expect("prompt while unrecoverably refused");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < deadline, "refused prompt never settled");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Error(message))) => {
                assert!(message.contains("agent is not attached"), "unexpected error: {message}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_initialize_without_auth_still_attempts_session_new() {
    use agent_client_protocol::schema::v1::{
        NewSessionRequest, NewSessionResponse, PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    // A non-auth refusal keeps the agent attached: a later prompt still
    // attempts `session/new` inline rather than failing like a dead peer.
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("init-refused-plain")
        .on_receive_request(
            async move |_init: InitializeRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32603, "unsupported client"))
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
            async move |request: PromptRequest, responder, _cx| {
                let _ = prompt_tx.send(request.session_id.to_string());
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < deadline, "ConnectionRefused never arrived");
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::ConnectionRefused { error, .. })) => {
                assert!(error.contains("unsupported client"), "unexpected refusal: {error}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt { session_id: String::new(), text: "hello".into() })
        .expect("prompt after a non-auth refusal");
    let session = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("prompt binds inline once session/new succeeds")
        .expect("prompt request");
    assert_eq!(session, "s1");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[test]
fn a_refusal_message_drops_only_the_appended_stderr_tail() {
    // The transport appends "Agent stderr:" + a tail onto wire errors; a
    // refused handshake keeps just its own words.
    let enriched = AcpError::new(-32603, "unsupported client")
        .data(json!({ "details": "unsupported client\nAgent stderr:\nlaunch noise" }));
    assert_eq!(init_refusal_message(&enriched), "unsupported client");

    let plain = AcpError::new(-32603, "unsupported client");
    assert_eq!(init_refusal_message(&plain), "unsupported client");

    // No tail at all — the message survives verbatim.
    let no_tail = AcpError::new(-32000, "Authentication required");
    assert_eq!(init_refusal_message(&no_tail), "Authentication required");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn harness_new_action_uses_the_native_new_tab_flow_without_reinitializing() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let initializes = Arc::new(AtomicUsize::new(0));
    let sessions = Arc::new(AtomicUsize::new(0));
    let agent = Agent
        .builder()
        .name("switchable-acp")
        .on_receive_request(
            {
                let initializes = Arc::clone(&initializes);
                async move |init: InitializeRequest, responder, cx| {
                    let generation = initializes.fetch_add(1, Ordering::SeqCst) + 1;
                    responder.respond(
                        InitializeResponse::new(init.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new(format!("agent-{generation}"), "0")),
                    )?;
                    if generation == 1 {
                        cx.send_notification(UntypedMessage::new(
                            crate::cordis::COMMANDS_UPDATE,
                            json!({
                                "protocol": 0,
                                "commands": [{
                                    "name": "harness",
                                    "description": "Switch Harness"
                                }]
                            }),
                        )?)?;
                    }
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |_req: NewSessionRequest, responder, _cx| {
                    let generation = sessions.fetch_add(1, Ordering::SeqCst) + 1;
                    responder.respond(NewSessionResponse::new(SessionId::new(format!(
                        "s{generation}"
                    ))))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: UntypedMessage, responder, _cx| {
                assert_eq!(request.method(), crate::cordis::COMMAND_INVOKE);
                responder.respond(json!({ "action": "new-session" }))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut command_ready = false;
    let mut initial_bound = false;
    while Instant::now() < deadline && !(command_ready && initial_bound) {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Rpc { method, .. }) if method == crate::cordis::COMMANDS_UPDATE => {
                command_ready = true;
            }
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                initial_bound = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(command_ready, "Client command catalog must be ready");
    assert!(initial_bound, "startup must bind the empty ACP session");
    assert_eq!(
        sessions.load(Ordering::SeqCst),
        1,
        "startup must create exactly one empty ACP session"
    );
    cmd_tx
        .send(Cmd::InvokePluginCommand {
            name: "harness".into(),
            args: "builtin-dsh".into(),
        })
        .expect("switch Harness");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut requested = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::NewSessionRequested)) => {
                requested = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(requested, "the painter must own creation of the new tab");
    assert_eq!(initializes.load(Ordering::SeqCst), 1);
    assert_eq!(sessions.load(Ordering::SeqCst), 1);
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut rebound = false;
    while Instant::now() < deadline {
        if let Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) =
            bus_rx.recv_timeout(Duration::from_millis(20))
        {
            if session_id == "s2" { rebound = true; break; }
        }
    }
    assert!(rebound);
    assert_eq!(initializes.load(Ordering::SeqCst), 1);
    assert_eq!(sessions.load(Ordering::SeqCst), 2);
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlay_cancel_reaches_the_compositor_while_submit_is_pending() {
    use agent_client_protocol::schema::v1::{InitializeResponse, NewSessionResponse};
    use std::time::Duration;

    let release = Arc::new(tokio::sync::Notify::new());
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("slow-client-compositor")
        .on_receive_request(
            async move |init: InitializeRequest, responder, cx| {
                responder.respond(InitializeResponse::new(init.protocol_version))?;
                cx.send_notification(UntypedMessage::new(
                    crate::cordis::COMMANDS_UPDATE,
                    json!({"protocol":0,"commands":[{"name":"harness","description":"Harness"}]}),
                )?)
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("initial")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let release = Arc::clone(&release);
                async move |request: UntypedMessage, responder, cx| {
                    let event = request.params()["event"].as_str().unwrap_or("").to_string();
                    let _ = events_tx.send(event.clone());
                    if event == "submit" {
                        let release = Arc::clone(&release);
                        cx.spawn(async move {
                            release.notified().await;
                            let _ = responder.respond(json!({"ok":true}));
                            Ok(())
                        })?;
                        return Ok(());
                    }
                    responder.respond(json!({"ok":true}))
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None, api_key: None,
        additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if matches!(bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. }))) { break; }
    }
    cmd_tx.send(Cmd::PluginOverlayEvent {
        id: "install".into(), event: "submit".into(), value: None,
    }).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(2), events_rx.recv()).await.unwrap().as_deref(), Some("submit"));
    cmd_tx.send(Cmd::PluginOverlayEvent {
        id: "install".into(), event: "cancel".into(), value: None,
    }).unwrap();
    let cancel = tokio::time::timeout(Duration::from_secs(1), events_rx.recv()).await;
    // Always release the fixture and shut down, including the failing baseline.
    release.notify_one();
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = tokio::time::timeout(Duration::from_secs(2), client).await;
    assert_eq!(cancel.expect("cancel must arrive before submit resolves").as_deref(), Some("cancel"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_operation_defers_agent_requests_and_queued_prompts_until_complete() {
    use agent_client_protocol::schema::v1::{
        InitializeResponse, NewSessionResponse, PromptResponse, SetSessionConfigOptionResponse,
        StopReason,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let release_operation = Arc::new(tokio::sync::Notify::new());
    let release_prompt = Arc::new(tokio::sync::Notify::new());
    let primary_pending = Arc::new(AtomicBool::new(false));
    let leaked_request = Arc::new(AtomicBool::new(false));
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent.builder().name("handoff-window-fixture")
        .on_receive_request(
            async move |init: InitializeRequest, responder, cx| {
                responder.respond(InitializeResponse::new(init.protocol_version))?;
                cx.send_notification(UntypedMessage::new(crate::cordis::COMMANDS_UPDATE,
                    json!({"protocol":0,"commands":[{"name":"harness","description":"Harness"}]}))?)
            }, on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("initial")))
            }, on_receive_request!(),
        )
        .on_receive_request({
            let events_tx = events_tx.clone();
            let release_prompt = Arc::clone(&release_prompt);
            let primary_pending = Arc::clone(&primary_pending);
            let leaked_request = Arc::clone(&leaked_request);
            async move |request: PromptRequest, responder, cx| {
                let text = request.prompt.iter().find_map(|block| match block {
                    ContentBlock::Text(text) => Some(text.text.clone()), _ => None,
                }).unwrap_or_default();
                if primary_pending.load(Ordering::SeqCst) {
                    leaked_request.store(true, Ordering::SeqCst);
                }
                let _ = events_tx.send(format!("prompt:{text}"));
                if text == "first" {
                    let release_prompt = Arc::clone(&release_prompt);
                    cx.spawn(async move {
                        release_prompt.notified().await;
                        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                        Ok(())
                    })?;
                    return Ok(());
                }
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            }
        }, on_receive_request!())
        .on_receive_request({
            let events_tx = events_tx.clone();
            let primary_pending = Arc::clone(&primary_pending);
            let leaked_request = Arc::clone(&leaked_request);
            async move |_request: SetSessionConfigOptionRequest, responder, _cx| {
                if primary_pending.load(Ordering::SeqCst) {
                    leaked_request.store(true, Ordering::SeqCst);
                }
                let _ = events_tx.send("model".to_string());
                responder.respond(SetSessionConfigOptionResponse::new(vec![]))
            }
        }, on_receive_request!())
        .on_receive_request({
            let release_operation = Arc::clone(&release_operation);
            let primary_pending = Arc::clone(&primary_pending);
            async move |request: UntypedMessage, responder, cx| {
                let event = request.params()["event"].as_str().unwrap_or("").to_string();
                if event == "submit" { primary_pending.store(true, Ordering::SeqCst); }
                let _ = events_tx.send(event.clone());
                if event == "submit" {
                    let release_operation = Arc::clone(&release_operation);
                    let primary_pending = Arc::clone(&primary_pending);
                    cx.spawn(async move {
                        release_operation.notified().await;
                        primary_pending.store(false, Ordering::SeqCst);
                        let _ = responder.respond(json!({"ok":true}));
                        Ok(())
                    })?;
                    return Ok(());
                }
                responder.respond(json!({"ok":true}))
            }
        }, on_receive_request!());
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None, api_key: None,
        additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if matches!(bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. }))) { break; }
    }
    cmd_tx.send(Cmd::Prompt { session_id: "initial".into(), text: "first".into() }).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(2), events_rx.recv()).await.unwrap().as_deref(), Some("prompt:first"));
    cmd_tx.send(Cmd::Prompt { session_id: "initial".into(), text: "queued".into() }).unwrap();
    cmd_tx.send(Cmd::PluginOverlayEvent { id: "install".into(), event: "submit".into(), value: None }).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(2), events_rx.recv()).await.unwrap().as_deref(), Some("submit"));

    // Completing an existing turn must not drain its prompt FIFO into a child
    // that may already be handed off but has not yet been initialized.
    release_prompt.notify_one();
    cmd_tx.send(Cmd::SelectModel { session_id: "initial".into(), provider: None, model: Some("next-model".into()), effort: None }).unwrap();
    cmd_tx.send(Cmd::Prompt { session_id: "initial".into(), text: "late".into() }).unwrap();
    cmd_tx.send(Cmd::PluginOverlayEvent { id: "install".into(), event: "cancel".into(), value: None }).unwrap();
    let first_during_operation = tokio::time::timeout(Duration::from_secs(1), events_rx.recv()).await;
    release_operation.notify_one();

    let mut observed = std::collections::BTreeSet::new();
    if let Ok(Some(event)) = &first_during_operation { observed.insert(event.clone()); }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !(observed.contains("model") && observed.contains("prompt:queued") && observed.contains("prompt:late")) {
        match tokio::time::timeout_at(deadline, events_rx.recv()).await {
            Ok(Some(event)) => { observed.insert(event); }
            _ => break,
        }
    }
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = tokio::time::timeout(Duration::from_secs(2), client).await;
    assert_eq!(first_during_operation.unwrap().as_deref(), Some("cancel"), "only cancel may bypass an active primary operation");
    assert!(!leaked_request.load(Ordering::SeqCst), "an Agent request escaped during the handoff window");
    assert!(observed.contains("model") && observed.contains("prompt:queued") && observed.contains("prompt:late"), "deferred requests must resume after completion: {observed:?}");
}

#[test]
fn prompt_image_flag_reads_initialize_payload() {
    assert!(prompt_image_supported(&json!({
        "agentCapabilities": { "promptCapabilities": { "image": true } }
    })));
    assert!(prompt_image_supported(&json!({
        "agentCapabilities": { "promptCapabilities": { "image": {} } }
    })));
    assert!(prompt_image_supported(&json!({
        "agentCapabilities": { "prompt": { "image": {} } }
    })));
    assert!(!prompt_image_supported(&json!({
        "agentCapabilities": { "promptCapabilities": { "image": false } }
    })));
    assert!(!prompt_image_supported(&json!({})));
}

#[test]
fn image_prompt_uses_image_blocks_when_advertised() {
    let blocks = vec![
        crate::bus::PromptBlock::Text("see".into()),
        crate::bus::PromptBlock::Image(crate::bus::ImagePart {
            data: "AQ==".into(),
            media_type: "image/png".into(),
            name: "shot.png".into(),
            path: "/tmp/shot.png".into(),
        }),
        crate::bus::PromptBlock::Text("that".into()),
    ];
    let content = prompt_content_blocks(blocks, true, "/w").expect("blocks");
    assert_eq!(content.len(), 3);
    assert!(matches!(&content[0], ContentBlock::Text(t) if t.text == "see"));
    match &content[1] {
        ContentBlock::Image(image) => {
            assert_eq!(image.data, "AQ==");
            assert_eq!(image.mime_type, "image/png");
            assert_eq!(image.uri.as_deref(), Some("file:///tmp/shot.png"));
        }
        other => panic!("expected image, got {other:?}"),
    }
    assert!(matches!(&content[2], ContentBlock::Text(t) if t.text == "that"));
}

#[test]
fn image_prompt_clipboard_image_has_bytes_only() {
    let blocks = vec![crate::bus::PromptBlock::Image(crate::bus::ImagePart {
        data: "AQ==".into(),
        media_type: "image/png".into(),
        name: "clipboard.png".into(),
        path: "clipboard".into(),
    })];
    let content = prompt_content_blocks(blocks, true, "/w").expect("blocks");
    assert_eq!(content.len(), 1);
    match &content[0] {
        ContentBlock::Image(image) => {
            assert_eq!(image.data, "AQ==");
            assert_eq!(image.mime_type, "image/png");
            assert_eq!(image.uri, None);
        }
        other => panic!("expected image, got {other:?}"),
    }
}

#[test]
fn image_prompt_falls_back_to_resource_link() {
    let blocks = vec![crate::bus::PromptBlock::Image(crate::bus::ImagePart {
        data: crate::pet::base64(&[1, 2, 3, 4]),
        media_type: "image/png".into(),
        name: "clip.png".into(),
        path: "clipboard".into(),
    })];
    let content = prompt_content_blocks(blocks, false, "/w").expect("spill");
    assert_eq!(content.len(), 1);
    match &content[0] {
        ContentBlock::ResourceLink(link) => {
            assert_eq!(link.name, "clip.png");
            assert_eq!(link.mime_type.as_deref(), Some("image/png"));
            assert!(
                link.uri.starts_with("file:///"),
                "spilled clipboard uri: {}",
                link.uri
            );
            let spilled = std::path::Path::new(link.uri.strip_prefix("file://").unwrap());
            assert!(spilled.is_absolute(), "spilled path: {}", link.uri);
            assert!(spilled.exists(), "spill missing: {}", link.uri);
        }
        other => panic!("expected resource link, got {other:?}"),
    }
}

#[test]
fn resource_link_uses_absolute_file_uri_for_disk_paths() {
    let blocks = vec![
        crate::bus::PromptBlock::Text("see".into()),
        crate::bus::PromptBlock::Image(crate::bus::ImagePart {
            data: "AQ==".into(),
            media_type: "image/png".into(),
            name: "shot.png".into(),
            path: "shots/./a.png".into(),
        }),
        crate::bus::PromptBlock::Text("that".into()),
    ];
    let content = prompt_content_blocks(blocks, false, "/w").expect("blocks");
    assert_eq!(content.len(), 3);
    assert!(matches!(&content[0], ContentBlock::Text(t) if t.text == "see"));
    match &content[1] {
        ContentBlock::ResourceLink(link) => {
            assert_eq!(link.uri, "file:///w/shots/a.png");
            assert_eq!(link.mime_type.as_deref(), Some("image/png"));
        }
        other => panic!("expected resource link, got {other:?}"),
    }
    assert!(matches!(&content[2], ContentBlock::Text(t) if t.text == "that"));
}

#[test]
fn load_session_flag_reads_initialize_payload() {
    assert!(load_session_supported(&json!({
        "agentCapabilities": { "loadSession": true }
    })));
    assert!(load_session_supported(&json!({
        "agent_capabilities": { "load_session": true }
    })));
    assert!(!load_session_supported(&json!({
        "agentCapabilities": { "loadSession": false }
    })));
    assert!(!load_session_supported(&json!({})));
}

#[test]
fn resume_session_flag_reads_initialize_payload() {
    assert!(resume_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "resume": {} } }
    })));
    assert!(resume_session_supported(&json!({
        "agent_capabilities": { "session_capabilities": { "resume": {} } }
    })));
    assert!(!resume_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "resume": null } }
    })));
    assert!(!resume_session_supported(&json!({})));
}

#[test]
fn list_session_flag_reads_initialize_payload() {
    assert!(list_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "list": {} } }
    })));
    assert!(!list_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "list": null } }
    })));
}

#[test]
fn close_session_flag_reads_initialize_payload() {
    assert!(close_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "close": {} } }
    })));
    assert!(close_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "close": true } }
    })));
    assert!(close_session_supported(&json!({
        "agent_capabilities": { "session_capabilities": { "close": {} } }
    })));
    assert!(!close_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "close": false } }
    })));
    assert!(!close_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "close": null } }
    })));
    assert!(!close_session_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": {} }
    })));
    assert!(!close_session_supported(&json!({})));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_session_reaches_the_wire_when_the_agent_advertises_it() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CloseSessionRequest, CloseSessionResponse, InitializeResponse,
        NewSessionResponse,
    };
    use std::time::{Duration, Instant};

    // `session/close` (issue #146): the agent advertised
    // `sessionCapabilities.close`, so closing the tab puts the request on
    // the wire and the agent frees the session's resources.
    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("close-capable-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                let mut response = serde_json::to_value(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("close-capable-mock", "0")),
                )
                .expect("serialize initialize response");
                response["agentCapabilities"]["sessionCapabilities"]["close"] = json!({});
                responder.respond(
                    serde_json::from_value(response).expect("deserialize initialize response"),
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
            async move |request: CloseSessionRequest, responder, _cx| {
                let _ = extension_tx.send(request.session_id.to_string());
                responder.respond(CloseSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(ready, "startup session/new must bind s1");

    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s1".into(),
        })
        .expect("close session");

    let session_id = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("session/close should reach the agent")
        .expect("extension channel");
    assert_eq!(session_id, "s1");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_session_stays_local_when_the_agent_did_not_advertise_it() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CloseSessionRequest, CloseSessionResponse, InitializeResponse,
        NewSessionResponse,
    };
    use std::time::{Duration, Instant};

    // No `sessionCapabilities.close`: the tab closes locally, the
    // server-side session survives — and must receive no request.
    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("close-unaware-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("close-unaware-mock", "0")),
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
            async move |request: CloseSessionRequest, responder, _cx| {
                let _ = extension_tx.send(request.session_id.to_string());
                responder.respond(CloseSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(ready, "startup session/new must bind s1");

    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s1".into(),
        })
        .expect("close session");

    // A second session/new rides the same setup lane right after the
    // close: when its bind lands, the close was already processed — so the
    // extension channel staying empty is proof no request went out.
    cmd_tx
        .send(Cmd::NewSession {
            requester: None,
            retry_auth: None,
        })
        .expect("open another session");
    assert!(
        (0..100).any(|_| matches!(
            bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. }))
        )),
        "the queued session/new must still bind after the close",
    );
    assert!(
        extension_rx.try_recv().is_err(),
        "an agent without sessionCapabilities.close must receive no session/close",
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_session_honors_the_per_session_connection_capability() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CloseSessionRequest, CloseSessionResponse, InitializeResponse,
        NewSessionResponse,
    };
    use std::time::{Duration, Instant};

    // A pooled connection reports its facts per session via
    // `_meta.marttyConnection` on session/new. That session-level
    // `sessionCapabilities.close` must also put session/close on the wire
    // even when initialize itself did not advertise it (issue #146).
    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("mux-close-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("mux-close-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                let mut response =
                    serde_json::to_value(NewSessionResponse::new(SessionId::new("s1")))
                        .expect("serialize session/new response");
                response["_meta"]["marttyConnection"] = json!({
                    "id": "pool-1",
                    "agentCapabilities": { "sessionCapabilities": { "close": {} } }
                });
                responder.respond(
                    serde_json::from_value(response).expect("deserialize session/new response"),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CloseSessionRequest, responder, _cx| {
                let _ = extension_tx.send(request.session_id.to_string());
                responder.respond(CloseSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(ready, "startup session/new must bind s1");

    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s1".into(),
        })
        .expect("close session");

    let session_id = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
        .await
        .expect("session/close should reach the agent on the per-session capability")
        .expect("extension channel");
    assert_eq!(session_id, "s1");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_session_wire_failure_reports_a_session_op_failure() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CloseSessionRequest, InitializeResponse, NewSessionResponse,
    };
    use std::time::{Duration, Instant};

    // The agent advertised close but rejects it on the wire: the failure
    // surfaces as a SessionOpFailed — never silently (issue #146).
    let agent = Agent
        .builder()
        .name("close-rejecting-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                let mut response = serde_json::to_value(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("close-rejecting-mock", "0")),
                )
                .expect("serialize initialize response");
                response["agentCapabilities"]["sessionCapabilities"]["close"] = json!({});
                responder.respond(
                    serde_json::from_value(response).expect("deserialize initialize response"),
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
            async move |_req: CloseSessionRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32000, "close refused"))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(ready, "startup session/new must bind s1");

    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s1".into(),
        })
        .expect("close session");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut failed = None;
    while Instant::now() < deadline && failed.is_none() {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionOpFailed { session_id, message })) => {
                failed = Some((session_id, message));
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    let (session_id, message) = failed.expect("session/close rejection must surface");
    assert_eq!(session_id, "s1");
    assert!(
        message.contains("session/close") && message.contains("close refused"),
        "wire error text reaches the tip: {message}"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_session_drops_unknown_and_non_current_ids_consistently() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CloseSessionRequest, CloseSessionResponse, InitializeResponse,
        NewSessionResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    // Bound sessions are tracked in `sessions`: closing an id that never
    // bound (a dead placeholder) must skip the wire and the repoint;
    // closing a bound-but-not-current session must not repoint `current`
    // either — only closing the current session repoints it (issue #146).
    let next = Arc::new(AtomicUsize::new(0));
    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let counter = next.clone();
    let agent = Agent
        .builder()
        .name("close-multi-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                let mut response = serde_json::to_value(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("close-multi-mock", "0")),
                )
                .expect("serialize initialize response");
                response["agentCapabilities"]["sessionCapabilities"]["close"] = json!({});
                responder.respond(
                    serde_json::from_value(response).expect("deserialize initialize response"),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                responder.respond(NewSessionResponse::new(SessionId::new(format!("s{n}"))))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CloseSessionRequest, responder, _cx| {
                let _ = extension_tx.send(request.session_id.to_string());
                responder.respond(CloseSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(ready, "startup session/new must bind s1");

    // Bind a second session — `current` becomes s2, leaving s1 bound but
    // not current.
    cmd_tx
        .send(Cmd::NewSession {
            requester: None,
            retry_auth: None,
        })
        .expect("open second session");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bound = false;
    while Instant::now() < deadline && !bound {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s2" => {
                bound = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("bus disconnected: {error}"),
        }
    }
    assert!(bound, "the second session/new must bind s2");

    // Unknown id: never bound, nothing to free agent-side — no wire close,
    // no repoint. The local drop still runs (a dead placeholder's parked
    // copies go with it).
    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s9".into(),
        })
        .expect("close unknown session");

    // Bound but not current: session/close goes out for s1 and its handle
    // drops, but `current` stays s2 — the repoint must not fire.
    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s1".into(),
        })
        .expect("close non-current session");

    // Current: the repoint moves the id-less fallback to a remaining
    // session (here: none left — current becomes None cleanly).
    cmd_tx
        .send(Cmd::CloseSession {
            session_id: "s2".into(),
        })
        .expect("close current session");

    let mut got = Vec::new();
    for _ in 0..2 {
        let id = tokio::time::timeout(Duration::from_secs(2), extension_rx.recv())
            .await
            .expect("two bound sessions close on the wire")
            .expect("extension channel");
        got.push(id);
    }
    // s9 produced nothing on the wire; s1 and s2 did — in enqueue order.
    assert_eq!(got, ["s1", "s2"], "only bound sessions reach session/close");
    assert!(
        extension_rx.try_recv().is_err(),
        "the unknown id must never reach session/close"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn form_auth_stays_configured_when_the_startup_session_succeeds() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeResponse, NewSessionResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let sessions = Arc::new(AtomicUsize::new(0));
    let mut meta = serde_json::Map::new();
    meta.insert("api-key".into(), json!({ "provider": "deepseek" }));
    let agent = Agent
        .builder()
        .name("already-authenticated-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("already-authenticated-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(
                            AuthMethodAgent::new("api-key", "DeepSeek API key").meta(meta.clone()),
                        )]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |_req: NewSessionRequest, responder, _cx| {
                    sessions.fetch_add(1, Ordering::SeqCst);
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut auth_status = None;
    let mut inferred_method = None;
    let mut opened_auth = false;
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => {
                auth_status = Some(snapshot.status);
                if snapshot.status == AuthStatus::Configured {
                    inferred_method = snapshot.method_id;
                }
            }
            Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => opened_auth = true,
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    while let Ok(event) = bus_rx.try_recv() {
        if matches!(event, AppEvent::Ctl(CtlEvent::OpenAuth)) {
            opened_auth = true;
        }
    }

    assert_eq!(auth_status, Some(AuthStatus::Configured));
    assert_eq!(inferred_method, None, "session/new must not claim the first advertised login method was used");
    assert!(ready, "startup should become ready after session/new");
    assert!(
        !opened_auth,
        "initialize alone must not open sign-in for possibly persisted credentials"
    );
    assert_eq!(
        sessions.load(Ordering::SeqCst),
        1,
        "startup session/new proves that persisted credentials work"
    );
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn needs_auth_looking_methods_never_flash_the_picker_at_startup() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeResponse, NewSessionResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    // pi-acp advertises a terminal method first and a form api-key after;
    // until session/new proves persisted credentials work, neither may be
    // treated as "sign-in needed" — that guess is what flashed the picker
    // ~0.85s into every signed-in startup (issue #160).
    let sessions = Arc::new(AtomicUsize::new(0));
    let mut meta = serde_json::Map::new();
    meta.insert("api-key".into(), json!({ "provider": "deepseek" }));
    let mut terminal_meta = serde_json::Map::new();
    terminal_meta.insert(
        "terminal-auth".into(),
        json!({ "args": ["login"], "env": {} }),
    );
    let agent = Agent
        .builder()
        .name("pi-style-startup-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("pi-style-startup-mock", "0"))
                        .auth_methods(vec![
                            AuthMethod::Agent(
                                AuthMethodAgent::new("pi-terminal", "Log in with pi")
                                    .meta(terminal_meta.clone()),
                            ),
                            AuthMethod::Agent(
                                AuthMethodAgent::new("api-key", "DeepSeek API key")
                                    .meta(meta.clone()),
                            ),
                        ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sessions = Arc::clone(&sessions);
                async move |_req: NewSessionRequest, responder, _cx| {
                    // Slow enough to reproduce the real startup gap in
                    // which the flash was observed.
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    sessions.fetch_add(1, Ordering::SeqCst);
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut statuses = Vec::new();
    let mut opened_auth = false;
    let mut ready = false;
    while Instant::now() < deadline && !ready {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => statuses.push(snapshot.status),
            Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => opened_auth = true,
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => ready = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    while let Ok(event) = bus_rx.try_recv() {
        if matches!(event, AppEvent::Ctl(CtlEvent::OpenAuth)) {
            opened_auth = true;
        }
    }

    assert!(ready, "startup should become ready after session/new");
    assert!(
        statuses.iter().all(|status| *status != AuthStatus::NeedsAuth),
        "no NeedsAuth snapshot may be emitted before a real refusal: {statuses:?}"
    );
    assert!(
        statuses.contains(&AuthStatus::Configured),
        "session/new success still reports credentials ready: {statuses:?}"
    );
    assert!(
        !opened_auth,
        "startup must not open the auth picker on its own"
    );
    assert_eq!(sessions.load(Ordering::SeqCst), 1);
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = tokio::time::timeout(Duration::from_secs(2), client)
        .await
        .expect("client task should settle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn elicitation_create_waits_for_the_tui_form_reply() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, CreateElicitationRequest, ElicitationAction, InitializeResponse,
        NewSessionResponse,
    };
    use std::time::{Duration, Instant};

    let request: CreateElicitationRequest = serde_json::from_value(json!({
        "mode": "form",
        "sessionId": "s1",
        "message": "The agent needs your input.",
        "requestedSchema": {
            "type": "object",
            "properties": {
                "question_0": {
                    "type": "string",
                    "title": "Target",
                    "oneOf": [
                        { "const": "local", "title": "Local" },
                        { "const": "remote", "title": "Remote" }
                    ]
                }
            },
            "required": ["question_0"]
        }
    }))
    .expect("standard form request");
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    let result_tx = Arc::new(Mutex::new(Some(result_tx)));
    let agent = Agent
        .builder()
        .name("elicitation-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("elicitation-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let result_tx = Arc::clone(&result_tx);
                async move |_req: NewSessionRequest, responder, cx| {
                    responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
                    let request = request.clone();
                    let tx = result_tx.lock().unwrap().take();
                    let request_cx = cx.clone();
                    cx.spawn(async move {
                        let result = request_cx.send_request(request).block_task().await;
                        if let Some(tx) = tx {
                            let _ = tx.send(result);
                        }
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("explicit new session");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut form_reply = None;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::ElicitationAsk {
                session_id,
                form,
                reply,
            }) => {
                assert_eq!(session_id.as_deref(), Some("s1"));
                assert_eq!(form.fields[0].title, "Target");
                form_reply = Some(reply);
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        form_reply.is_some(),
        "elicitation/create must open the TUI form"
    );
    let _ = form_reply
        .unwrap()
        .send(crate::elicitation::ElicitationReply::Accepted(
            std::collections::BTreeMap::from([(
                "question_0".into(),
                crate::elicitation::ElicitationValue::String("remote".into()),
            )]),
        ));

    let response = tokio::time::timeout(Duration::from_secs(2), result_rx)
        .await
        .expect("client should answer elicitation")
        .expect("result channel")
        .expect("ACP response");
    let ElicitationAction::Accept(accepted) = response.action else {
        panic!("expected accepted form response")
    };
    assert!(matches!(
        accepted.content.as_ref().and_then(|content| content.get("question_0")),
        Some(agent_client_protocol::schema::v1::ElicitationContentValue::String(value))
            if value == "remote"
    ));
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_session_binds_before_applying_initial_config() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
        SessionConfigSelectOption,
    };
    use std::time::{Duration, Instant};

    let agent = Agent
        .builder()
        .name("setup-order-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("setup-order-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                        SessionConfigOption::select(
                            "collaboration_mode",
                            "Collaboration mode",
                            "plan",
                            vec![
                                SessionConfigSelectOption::new("default", "Default"),
                                SessionConfigSelectOption::new("plan", "Plan"),
                            ],
                        ),
                    ]),
                )
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("explicit new session");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut order = Vec::new();
    while Instant::now() < deadline && order.len() < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                order.push("bound");
            }
            Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                if session == "s1" && active =>
            {
                order.push("plan");
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    assert_eq!(order, ["bound", "plan"]);
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_config_option_response_updates_client_state_without_a_notification() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
        SessionConfigSelectOption, SetSessionConfigOptionResponse,
    };
    use std::time::{Duration, Instant};

    let agent = Agent
        .builder()
        .name("config-response-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("config-response-mock", "0")),
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
            async move |_req: SetSessionConfigOptionRequest, responder, _cx| {
                responder.respond(SetSessionConfigOptionResponse::new(vec![
                    SessionConfigOption::select(
                        "collaboration_mode",
                        "Collaboration mode",
                        "plan",
                        vec![
                            SessionConfigSelectOption::new("default", "Default"),
                            SessionConfigSelectOption::new("plan", "Plan"),
                        ],
                    ),
                ]))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("explicit new session");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    cmd_tx
        .send(Cmd::SetConfigOption {
            session_id: String::new(), // legacy: address the fallback session
            config_id: "collaboration_mode".into(),
            value: "plan".into(),
        })
        .expect("set config option");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_plan = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                if session == "s1" && active =>
            {
                saw_plan = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    assert!(
        saw_plan,
        "the successful response is the authoritative state snapshot"
    );
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn effort_selection_uses_the_advertised_thought_level_config_id() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
        SessionConfigOptionCategory, SessionConfigSelectOption, SetSessionConfigOptionResponse,
    };
    use std::time::{Duration, Instant};

    let (config_tx, mut config_rx) = tokio::sync::mpsc::unbounded_channel();
    let effort_option = || {
        SessionConfigOption::select(
            "reasoning_effort",
            "Reasoning effort",
            "high",
            vec![
                SessionConfigSelectOption::new("low", "Low"),
                SessionConfigSelectOption::new("high", "High"),
                SessionConfigSelectOption::new("ultra", "Ultra"),
            ],
        )
        .category(SessionConfigOptionCategory::ThoughtLevel)
    };
    let agent = Agent
        .builder()
        .name("semantic-effort-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("semantic-effort-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s1"))
                        .config_options(vec![effort_option()]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                let _ = config_tx.send(req.config_id.to_string());
                responder.respond(SetSessionConfigOptionResponse::new(vec![effort_option()]))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("create ACP session");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if matches!(
            bus_rx.recv_timeout(Duration::from_millis(20)),
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1"
        ) {
            break;
        }
    }
    cmd_tx
        .send(Cmd::SelectModel {
            session_id: "s1".into(),
            provider: None,
            model: None,
            effort: Some("ultra".into()),
        })
        .expect("select effort");

    let config_id = tokio::time::timeout(Duration::from_secs(1), config_rx.recv())
        .await
        .expect("set_config_option request")
        .expect("config id");
    assert_eq!(config_id, "reasoning_effort");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_tree_config_set_uses_standard_acp_and_folds_response_only_state() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse, SessionConfigOption,
        SessionConfigSelectOption, SetSessionConfigOptionResponse,
    };
    use std::time::{Duration, Instant};

    let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent
        .builder()
        .name("client-config-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("client-config-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s-client")).config_options(vec![
                        SessionConfigOption::select(
                            "collaboration_mode",
                            "Collaboration mode",
                            "default",
                            vec![
                                SessionConfigSelectOption::new("default", "Default"),
                                SessionConfigSelectOption::new("plan", "Plan"),
                            ],
                        ),
                    ]),
                )?;
                let result_tx = result_tx.clone();
                tokio::spawn(async move {
                    let request = UntypedMessage::new(
                        crate::cordis::SESSION_CONFIG_SET,
                        json!({
                            "protocol": crate::cordis::PROTOCOL,
                            "sessionId": "s-client",
                            "configId": "collaboration_mode",
                            "value": "plan",
                        }),
                    )
                    .expect("valid Client compositor request");
                    let result = cx.send_request(request).block_task().await;
                    let _ = result_tx.send(result);
                });
                Ok(())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                assert_eq!(req.session_id.to_string(), "s-client");
                assert_eq!(req.config_id.to_string(), "collaboration_mode");
                assert_eq!(
                    req.value.as_value_id().map(ToString::to_string).as_deref(),
                    Some("plan")
                );
                responder.respond(SetSessionConfigOptionResponse::new(vec![
                    SessionConfigOption::select(
                        "collaboration_mode",
                        "Collaboration mode",
                        "plan",
                        vec![
                            SessionConfigSelectOption::new("default", "Default"),
                            SessionConfigSelectOption::new("plan", "Plan"),
                        ],
                    ),
                ]))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("create ACP session");

    let result = tokio::time::timeout(Duration::from_secs(2), result_rx.recv())
        .await
        .expect("Client config request settled")
        .expect("Client config response exists")
        .expect("Client config request succeeded");
    assert_eq!(
        result["configOptions"][0]["currentValue"],
        json!("plan"),
        "the internal response returns the complete standard ACP snapshot"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_plan = false;
    while Instant::now() < deadline && !saw_plan {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                if session == "s-client" && active =>
            {
                saw_plan = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        saw_plan,
        "response-only config state reaches the native client"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_session_prefers_resume_and_binds_before_applying_initial_config() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, LoadSessionResponse, NewSessionResponse,
        ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionConfigOption,
        SessionConfigSelectOption, SessionResumeCapabilities,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let load_calls = Arc::new(AtomicUsize::new(0));
    let agent = Agent
        .builder()
        .name("load-config-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new()
                                .load_session(true)
                                .session_capabilities(
                                    SessionCapabilities::new()
                                        .resume(SessionResumeCapabilities::new()),
                                ),
                        )
                        .agent_info(Implementation::new("load-config-mock", "0")),
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
            async move |_req: ResumeSessionRequest, responder, _cx| {
                responder.respond(ResumeSessionResponse::new().config_options(vec![
                    SessionConfigOption::select(
                        "collaboration_mode",
                        "Collaboration mode",
                        "plan",
                        vec![
                            SessionConfigSelectOption::new("default", "Default"),
                            SessionConfigSelectOption::new("plan", "Plan"),
                        ],
                    ),
                ]))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let load_calls = Arc::clone(&load_calls);
                async move |_req: LoadSessionRequest, responder, _cx| {
                    load_calls.fetch_add(1, Ordering::SeqCst);
                    responder.respond(LoadSessionResponse::new())
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    cmd_tx
        .send(Cmd::ResumeSession {
            session_id: "s2".into(),
        })
        .expect("resume session");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut order = Vec::new();
    while Instant::now() < deadline && order.len() < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s2" => {
                order.push("bound");
            }
            Ok(AppEvent::Ui(crate::events::UiEvent::PlanMode { session, active }))
                if session == "s2" && active =>
            {
                order.push("plan");
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    assert_eq!(order, ["bound", "plan"]);
    assert_eq!(load_calls.load(Ordering::SeqCst), 0);
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_session_falls_back_to_load_when_resume_is_rejected() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, LoadSessionResponse, NewSessionResponse,
        ResumeSessionRequest, SessionCapabilities, SessionResumeCapabilities,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let resume_calls = Arc::new(AtomicUsize::new(0));
    let load_calls = Arc::new(AtomicUsize::new(0));
    let agent = Agent
        .builder()
        .name("resume-fallback-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new()
                                .load_session(true)
                                .session_capabilities(
                                    SessionCapabilities::new()
                                        .resume(SessionResumeCapabilities::new()),
                                ),
                        )
                        .agent_info(Implementation::new("resume-fallback-mock", "0")),
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
                let resume_calls = Arc::clone(&resume_calls);
                async move |_req: ResumeSessionRequest, responder, _cx| {
                    resume_calls.fetch_add(1, Ordering::SeqCst);
                    responder.respond_with_error(AcpError::new(-32601, "resume unavailable"))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let load_calls = Arc::clone(&load_calls);
                async move |_req: LoadSessionRequest, responder, _cx| {
                    load_calls.fetch_add(1, Ordering::SeqCst);
                    responder.respond(LoadSessionResponse::new())
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let startup_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < startup_deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    cmd_tx
        .send(Cmd::ResumeSession {
            session_id: "s2".into(),
        })
        .expect("resume session");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bound_notice = None;
    while Instant::now() < deadline && bound_notice.is_none() {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, notice }))
                if session_id == "s2" =>
            {
                bound_notice = notice
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    assert_eq!(resume_calls.load(Ordering::SeqCst), 1);
    assert_eq!(load_calls.load(Ordering::SeqCst), 1);
    assert!(bound_notice
        .as_deref()
        .is_some_and(|text| text.contains("loaded s2")));
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompts_while_running_wait_in_fifo_without_session_cancel() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
    let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));

    let agent = Agent
        .builder()
        .name("queue-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("queue-mock", "0")),
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
                let release_first_rx = Arc::clone(&release_first_rx);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text);
                    let release = release_first_rx.lock().unwrap().take();
                    cx.spawn(async move {
                        if let Some(release) = release {
                            let _ = release.await;
                        }
                        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |_note: CancelNotification, _cx| {
                let _ = cancel_tx.send(());
                Ok(())
            },
            on_receive_notification!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    for text in ["first", "second", "third"] {
        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: text.into(),
            })
            .expect("prompt");
    }

    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("first prompt reaches the agent")
            .as_deref(),
        Some("first")
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "queued prompts must not reach the agent before the active prompt settles"
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "queueing a prompt must never send session/cancel"
    );
    while bus_rx.try_recv().is_ok() {}

    let _ = release_first_tx.send(());
    let second = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("second prompt reaches the agent")
        .expect("second prompt");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut idle_between_prompts = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Rpc { method, params }) if method == "session.status" => {
                if params.get("status").and_then(serde_json::Value::as_str) == Some("idle") {
                    idle_between_prompts = true;
                }
            }
            Ok(AppEvent::Ctl(CtlEvent::PromptQueued { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        !idle_between_prompts,
        "the client stays running while a FIFO follow-up is ready"
    );
    let third = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("third prompt reaches the agent")
        .expect("third prompt");
    assert_eq!([second.as_str(), third.as_str()], ["second", "third"]);

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composition_catalog_is_ready_before_the_first_prompt() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, SessionConfigOption, SessionConfigSelectOption,
        StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("startup-session-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("startup-session-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                        SessionConfigOption::select(
                            "agent",
                            "Agent",
                            "standard",
                            vec![
                                SessionConfigSelectOption::new("standard", "Standard"),
                                SessionConfigSelectOption::new("creator", "Creator"),
                            ],
                        ),
                    ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, _cx| {
                let text = req
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let _ = prompt_tx.send(text);
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    let mut catalog = None;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Catalog { presets, .. })) if !presets.is_empty() => {
                catalog = Some(
                    presets
                        .into_iter()
                        .map(|preset| preset.id)
                        .collect::<Vec<_>>(),
                );
            }
            Ok(AppEvent::Ctl(CtlEvent::Ready { .. })) => {
                ready = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(ready, "initialize completes during startup");
    assert_eq!(
        catalog,
        Some(vec!["standard".into(), "creator".into()]),
        "startup session/new must expose config options before the first prompt"
    );
    assert!(
        prompt_rx.try_recv().is_err(),
        "session/new is not a model turn"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_failure_parks_prompts_but_reports_steers_back_to_the_client() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse, PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let rejected_first = Arc::new(AtomicBool::new(false));
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("auth-queue-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("auth-queue-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
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
            async move |_req: AuthenticateRequest, responder, _cx| {
                responder.respond(AuthenticateResponse::new())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let rejected_first = Arc::clone(&rejected_first);
                async move |req: PromptRequest, responder, _cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text.clone());
                    if text == "first" && !rejected_first.swap(true, Ordering::SeqCst) {
                        responder
                            .respond_with_error(serde_json::from_value(json!({
                                "code": -32603, "message": "Internal error",
                                "data": { "errorKind": "authentication_failed",
                                    "details": "Failed to authenticate. API Error: 403 Insufficient account balance" }
                            })).unwrap())
                    } else {
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                }
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    for text in ["first", "second"] {
        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: text.into(),
            })
            .expect("prompt");
    }
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("first prompt reaches the agent")
            .as_deref(),
        Some("first")
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut needs_auth = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                needs_auth = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(needs_auth, "the active prompt requests sign-in");
    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "third".into(),
        })
        .expect("prompt entered while auth is unresolved");
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text(
                "image group during auth".into(),
            )],
        })
        .expect("image prompt entered while auth is unresolved");
    cmd_tx
        .send(Cmd::Steer {
            session_id: "s1".into(),
            message_id: 7,
            text: "steer during auth".into(),
        })
        .expect("steer entered while auth is unresolved");
    cmd_tx
        .send(Cmd::SteerImages {
            session_id: "s1".into(),
            message_id: 8,
            blocks: vec![crate::bus::PromptBlock::Text(
                "image steer during auth".into(),
            )],
        })
        .expect("image steer entered while auth is unresolved");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut deferred_steers = std::collections::BTreeSet::new();
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SteerSettled {
                message_id,
                deferred,
            })) if matches!(message_id, 7 | 8) => {
                if deferred {
                    deferred_steers.insert(message_id);
                }
                if deferred_steers.len() == 2 {
                    break;
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(
        deferred_steers,
        std::collections::BTreeSet::from([7, 8]),
        "text and image Send Now both return to the client while authentication is unresolved"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "followups stay parked while authentication is unresolved"
    );

    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "login".into(),
            values: Default::default(),
        })
        .expect("authenticate");
    let retried = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("parked prompt retries")
        .expect("retry text");
    let followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("followup drains")
        .expect("followup text");
    let later_followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("prompt entered during auth drains last")
        .expect("later followup text");
    let image_followup = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("image prompt entered during auth drains in order")
        .expect("image followup text");
    assert_eq!(
        [
            retried.as_str(),
            followup.as_str(),
            later_followup.as_str(),
            image_followup.as_str(),
        ],
        ["first", "second", "third", "image group during auth"]
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), prompt_rx.recv())
            .await
            .is_err(),
        "the transport must not retain deferred steers after reporting them to the client"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticate_rejection_reports_failure_instead_of_another_sign_in_hint() {
    use agent_client_protocol::schema::v1::{AuthMethod, AuthMethodAgent, InitializeRequest,
        InitializeResponse, NewSessionRequest};
    use std::time::{Duration, Instant};
    let reason = "Onboarding failed: account is not eligible in your location";
    let agent = Agent.builder().name("auth-rejection")
        .on_receive_request(async move |init: InitializeRequest, responder, _cx| {
            responder.respond(InitializeResponse::new(init.protocol_version)
                .agent_info(Implementation::new("auth-rejection", "0"))
                .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new("oauth-personal", "Google"))]))
        }, on_receive_request!())
        .on_receive_request(async move |_req: NewSessionRequest, responder, _cx| {
            responder.respond_with_error(AcpError::new(-32000, "Authentication required"))
        }, on_receive_request!())
        .on_receive_request(async move |req: AuthenticateRequest, responder, _cx| {
            assert_eq!(req.method_id.to_string(), "oauth-personal");
            responder.respond_with_error(AcpError::new(-32000, reason))
        }, on_receive_request!());
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None, api_key: None,
        additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline {
        if matches!(bus_rx.recv_timeout(Duration::from_millis(20)), Ok(AppEvent::Ctl(CtlEvent::Ready { .. }))) {
            ready = true;
            break;
        }
    }
    assert!(ready);
    cmd_tx.send(Cmd::Authenticate { method_id: "oauth-personal".into(), values: Default::default() }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut failure = None;
    while Instant::now() < deadline {
        if let Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) = bus_rx.recv_timeout(Duration::from_millis(20)) {
            if snapshot.message.as_deref() == Some(reason) {
                failure = Some(snapshot);
                break;
            }
        }
    }
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
    let failure = failure.expect("the agent's failure reason must reach the UI");
    assert_eq!(format!("{:?}", failure.status), "Failed");
    assert_eq!(failure.method_id.as_deref(), Some("oauth-personal"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authentication_is_visible_before_session_new_completes() {
    assert_authentication_before_session_setup(None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_session_setup_error_does_not_restore_needs_auth() {
    assert_authentication_before_session_setup(Some(-32603)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_session_setup_auth_required_restores_needs_auth() {
    assert_authentication_before_session_setup(Some(-32000)).await;
}

async fn assert_authentication_before_session_setup(setup_error: Option<i32>) {
    use agent_client_protocol::schema::v1::{
        AuthMethod, AuthMethodAgent, AuthenticateResponse, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let authenticated = Arc::new(AtomicBool::new(false));
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    let agent = Agent.builder().name("auth-session-boundary")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(InitializeResponse::new(init.protocol_version)
                    .agent_info(Implementation::new("auth-session-boundary", "0"))
                    .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new("login", "Login"))]))
            }, on_receive_request!(),
        )
        .on_receive_request({
            let authenticated = Arc::clone(&authenticated);
            async move |_req: AuthenticateRequest, responder, _cx| {
                authenticated.store(true, Ordering::SeqCst);
                responder.respond(AuthenticateResponse::new())
            }
        }, on_receive_request!())
        .on_receive_request({
            let authenticated = Arc::clone(&authenticated);
            async move |_req: NewSessionRequest, responder, cx| {
                if !authenticated.load(Ordering::SeqCst) {
                    return responder.respond_with_error(AcpError::new(-32000, "authentication required"));
                }
                let release = release_rx.lock().unwrap().take().expect("one setup retry");
                cx.spawn(async move {
                    let _ = release.await;
                    match setup_error {
                        Some(code) => responder.respond_with_error(AcpError::new(code, "setup rejected")),
                        None => responder.respond(NewSessionResponse::new(SessionId::new("authenticated-session"))),
                    }
                })?;
                Ok(())
            }
        }, on_receive_request!());
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None, api_key: None,
        additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline {
        if matches!(bus_rx.recv_timeout(Duration::from_millis(20)), Ok(AppEvent::Ctl(CtlEvent::Ready { .. }))) {
            ready = true;
            break;
        }
    }
    assert!(ready, "startup must finish with authentication required");
    cmd_tx.send(Cmd::Authenticate { method_id: "login".into(), values: Default::default() }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut configured_before_setup = false;
    while Instant::now() < deadline {
        if let Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) = bus_rx.recv_timeout(Duration::from_millis(20)) {
            if snapshot.status == AuthStatus::Configured {
                configured_before_setup = true;
                break;
            }
        }
    }
    // Release even on regression so the client can shut down cleanly.
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut final_status = if configured_before_setup { AuthStatus::Configured } else { AuthStatus::NeedsAuth };
    let mut setup_finished = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => {
                final_status = snapshot.status;
                if setup_error == Some(-32000) && final_status == AuthStatus::NeedsAuth {
                    setup_finished = true;
                    break;
                }
            }
            Ok(AppEvent::Ctl(CtlEvent::Error(message))) if message.contains("session/new after authenticate") => {
                assert_eq!(setup_error, Some(-32603));
                setup_finished = true;
                break;
            }
            Ok(AppEvent::Ctl(CtlEvent::TuiOpDone(message))) if message == "signed in" => {
                assert_eq!(setup_error, None);
                setup_finished = true;
                break;
            }
            _ => {}
        }
    }
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
    assert!(configured_before_setup, "authenticate success must update auth while session/new is still pending");
    assert!(setup_finished, "session setup must have an independent outcome");
    assert_eq!(final_status, if setup_error == Some(-32000) { AuthStatus::NeedsAuth } else { AuthStatus::Configured });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_new_auth_failure_parks_the_first_intent_without_retry_storms() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse, PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let authenticated = Arc::new(AtomicBool::new(false));
    let new_attempts = Arc::new(AtomicUsize::new(0));
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("new-auth-queue-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("new-auth-queue-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let authenticated = Arc::clone(&authenticated);
                let new_attempts = Arc::clone(&new_attempts);
                async move |_req: NewSessionRequest, responder, _cx| {
                    new_attempts.fetch_add(1, Ordering::SeqCst);
                    if authenticated.load(Ordering::SeqCst) {
                        responder.respond(NewSessionResponse::new(SessionId::new("s1")))
                    } else {
                        responder
                            .respond_with_error(AcpError::new(-32000, "authentication required"))
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let authenticated = Arc::clone(&authenticated);
                async move |_req: AuthenticateRequest, responder, _cx| {
                    authenticated.store(true, Ordering::SeqCst);
                    responder.respond(AuthenticateResponse::new())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, _cx| {
                let text = req
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let _ = prompt_tx.send(text);
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "local-draft".into(),
            text: "first".into(),
        })
        .expect("first prompt");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut needs_auth = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                needs_auth = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(needs_auth, "session/new requests sign-in");

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "local-draft".into(),
            text: "second".into(),
        })
        .expect("followup during auth");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        new_attempts.load(Ordering::SeqCst),
        1,
        "followups must not retry session/new before authentication"
    );
    assert!(prompt_rx.try_recv().is_err());

    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "login".into(),
            values: Default::default(),
        })
        .expect("authenticate");
    let first = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("parked first prompt retries")
        .expect("first text");
    let second = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("followup drains")
        .expect("second text");
    assert_eq!([first.as_str(), second.as_str()], ["first", "second"]);
    assert_eq!(new_attempts.load(Ordering::SeqCst), 2);

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_steer_rejection_is_not_retried_by_the_transport_after_auth() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse, PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_steer_tx, release_steer_rx) = tokio::sync::oneshot::channel::<()>();
    let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));
    let release_steer_rx = Arc::new(Mutex::new(Some(release_steer_rx)));
    let first_rejected = Arc::new(AtomicBool::new(false));
    let steer_rejected = Arc::new(AtomicBool::new(false));

    let agent = Agent
        .builder()
        .name("late-steer-auth-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("late-steer-auth-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
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
            async move |_req: AuthenticateRequest, responder, _cx| {
                responder.respond(AuthenticateResponse::new())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let release_first_rx = Arc::clone(&release_first_rx);
                let release_steer_rx = Arc::clone(&release_steer_rx);
                let first_rejected = Arc::clone(&first_rejected);
                let steer_rejected = Arc::clone(&steer_rejected);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text.clone());
                    if text == "first" && !first_rejected.swap(true, Ordering::SeqCst) {
                        let release = release_first_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond_with_error(AcpError::new(
                                -32000,
                                "authentication required",
                            ));
                            Ok(())
                        })?;
                        return Ok(());
                    }
                    if text == "steer" && !steer_rejected.swap(true, Ordering::SeqCst) {
                        let release = release_steer_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond_with_error(AcpError::new(
                                -32603,
                                "concurrent prompt rejected",
                            ));
                            Ok(())
                        })?;
                        return Ok(());
                    }
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                }
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "first".into(),
        })
        .expect("first prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("first starts")
            .as_deref(),
        Some("first")
    );
    cmd_tx
        .send(Cmd::Steer {
            session_id: "s1".into(),
            message_id: 9,
            text: "steer".into(),
        })
        .expect("concurrent steer");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("steer starts")
            .as_deref(),
        Some("steer")
    );

    let _ = release_first_tx.send(());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut needs_auth = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                needs_auth = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(needs_auth, "active prompt parks for auth");

    let _ = release_steer_tx.send(());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut steer_deferred = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SteerSettled {
                message_id: 9,
                deferred,
            })) => {
                steer_deferred = deferred;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(steer_deferred, "rejected steer returns to the client");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "the rejected steer is not retried while authentication is unresolved"
    );

    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "login".into(),
            values: Default::default(),
        })
        .expect("authenticate");
    let first = tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
        .await
        .expect("parked active retries")
        .expect("first retry text");
    assert_eq!(first, "first");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), prompt_rx.recv())
            .await
            .is_err(),
        "the transport must not retry the steer after the client was told to defer it"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_sends_a_concurrent_prompt_without_interrupting_the_turn() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
    let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));

    let agent = Agent
        .builder()
        .name("steer-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("steer-mock", "0")),
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
                let release_first_rx = Arc::clone(&release_first_rx);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text);
                    let release = release_first_rx.lock().unwrap().take();
                    cx.spawn(async move {
                        if let Some(release) = release {
                            let _ = release.await;
                        }
                        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |_note: CancelNotification, _cx| {
                let _ = cancel_tx.send(());
                Ok(())
            },
            on_receive_notification!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "first".into(),
        })
        .expect("first prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("first prompt reaches the agent")
            .as_deref(),
        Some("first")
    );

    cmd_tx
        .send(Cmd::Steer {
            session_id: "s1".into(),
            message_id: 1,
            text: "change course".into(),
        })
        .expect("steer");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("steer reaches the active agent")
            .as_deref(),
        Some("change course")
    );
    cmd_tx
        .send(Cmd::SteerImages {
            session_id: "s1".into(),
            message_id: 2,
            blocks: vec![
                crate::bus::PromptBlock::Text("look here".into()),
                crate::bus::PromptBlock::Image(crate::bus::ImagePart {
                    data: "AQID".into(),
                    media_type: "image/png".into(),
                    name: "shot.png".into(),
                    path: "clipboard".into(),
                }),
            ],
        })
        .expect("image steer");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("image steer reaches the active agent")
            .as_deref(),
        Some("look here")
    );
    assert!(
        cancel_rx.try_recv().is_err(),
        "steer must not send session/cancel"
    );
    while let Ok(event) = bus_rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::Ctl(CtlEvent::Interrupted { .. })),
            "steer is part of the active turn, not an interruption"
        );
    }

    let _ = release_first_tx.send(());
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_steer_reports_deferred_without_transport_retry() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
    let release_first_rx = Arc::new(Mutex::new(Some(release_first_rx)));
    let steer_attempts = Arc::new(AtomicUsize::new(0));

    let agent = Agent
        .builder()
        .name("steer-fallback-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("steer-fallback-mock", "0")),
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
                let release_first_rx = Arc::clone(&release_first_rx);
                let steer_attempts = Arc::clone(&steer_attempts);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text.clone());
                    if text == "first" {
                        let release = release_first_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                            Ok(())
                        })?;
                        return Ok(());
                    }
                    if steer_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return responder.respond_with_error(AcpError::new(
                            -32603,
                            "concurrent prompt rejected",
                        ));
                    }
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                }
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "first".into(),
        })
        .expect("first prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("first prompt reaches the agent")
            .as_deref(),
        Some("first")
    );

    cmd_tx
        .send(Cmd::Steer {
            session_id: "s1".into(),
            message_id: 1,
            text: "change course".into(),
        })
        .expect("steer");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("steer attempt reaches the active agent")
            .as_deref(),
        Some("change course")
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut deferred = false;
    while Instant::now() < deadline && !deferred {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SteerSettled {
                message_id: 1,
                deferred: true,
            })) => deferred = true,
            Ok(AppEvent::Ctl(CtlEvent::Error(err))) => panic!("steer failed the UI: {err}"),
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        deferred,
        "the App must be told to enqueue the rejected steer"
    );

    let _ = release_first_tx.send(());
    assert!(
        tokio::time::timeout(Duration::from_millis(200), prompt_rx.recv())
            .await
            .is_err(),
        "the transport must not retain its own fallback FIFO"
    );
    assert_eq!(steer_attempts.load(Ordering::SeqCst), 1);

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupt_sends_session_cancel_while_prompt_is_in_flight() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_started_tx, prompt_started_rx) = tokio::sync::oneshot::channel::<()>();
    let (cancel_seen_tx, cancel_seen_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let prompt_started_tx = Arc::new(Mutex::new(Some(prompt_started_tx)));
    let cancel_seen_tx = Arc::new(Mutex::new(Some(cancel_seen_tx)));
    let release_tx = Arc::new(Mutex::new(Some(release_tx)));
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));

    let agent = Agent
        .builder()
        .name("cancel-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("cancel-mock", "0")),
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
                let prompt_started_tx = Arc::clone(&prompt_started_tx);
                let release_rx = Arc::clone(&release_rx);
                async move |_req: PromptRequest, responder, cx| {
                    if let Some(tx) = prompt_started_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    let release = release_rx.lock().unwrap().take();
                    cx.spawn(async move {
                        if let Some(release) = release {
                            let _ = release.await;
                        }
                        let _ = responder.respond(PromptResponse::new(StopReason::Cancelled));
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            {
                let cancel_seen_tx = Arc::clone(&cancel_seen_tx);
                let release_tx = Arc::clone(&release_tx);
                async move |_note: CancelNotification, _cx| {
                    if let Some(tx) = cancel_seen_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    if let Some(tx) = release_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    Ok(())
                }
            },
            on_receive_notification!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "hello".into(),
        })
        .expect("prompt");
    tokio::time::timeout(Duration::from_secs(2), prompt_started_rx)
        .await
        .expect("agent should see session/prompt")
        .expect("prompt started");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_turn_start = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::TurnStart { session, .. }))
                if session == "s1" =>
            {
                saw_turn_start = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        saw_turn_start,
        "ACP prompt start must feed the existing footer stats"
    );

    cmd_tx
        .send(Cmd::Interrupt {
            session_id: "s1".into(),
        })
        .expect("interrupt");
    tokio::time::timeout(Duration::from_secs(2), cancel_seen_rx)
        .await
        .expect("session/cancel must be sent while prompt is still in flight")
        .expect("cancel seen");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_turn_end = false;
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
                if session == "s1" && kind == "interrupted" =>
            {
                saw_turn_end = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        saw_turn_end,
        "cancelled ACP prompts must close footer timing"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[cfg(unix)]
#[tokio::test]
async fn attach_fd_socketpair_reads_without_eagain() {
    use std::io::Write;
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream as StdUnixStream;
    use tokio::io::AsyncReadExt;

    let (mut peer, local) = StdUnixStream::pair().expect("socketpair");
    peer.write_all(b"hello\n").expect("write peer");
    let file = unsafe { std::fs::File::from_raw_fd(local.into_raw_fd()) };
    let mut stream = super::unix_stream_from_file(file).expect("wrap fd");
    let mut buf = vec![0u8; 6];
    stream
        .read_exact(&mut buf)
        .await
        .expect("read should not be EAGAIN");
    assert_eq!(&buf, b"hello\n");
}

#[test]
fn cmd_session_resolution_honors_carried_id_falls_back_and_rejects_unknown() {
    let mut sessions = HashMap::<String, SessionHandle>::new();
    sessions.insert("s1".into(), SessionHandle::default());
    let current = Some(SessionId::new("s1"));

    // An empty field falls back to the most recently bound session.
    let fallback = resolve_cmd_session(&sessions, &current, "", "prompt").expect("fallback");
    assert_eq!(fallback.expect("current session").to_string(), "s1");

    // A bound id is honored exactly as carried.
    let carried = resolve_cmd_session(&sessions, &current, "s1", "prompt").expect("carried");
    assert_eq!(carried.expect("bound session").to_string(), "s1");

    // An id this connection never bound is rejected, not rerouted.
    let err = resolve_cmd_session(&sessions, &current, "ghost", "prompt").expect_err("unknown id");
    assert!(err.contains("ghost"), "error names the id: {err}");

    // Before the first session exists, any carried id is a local placeholder
    // with no server meaning yet; it resolves to "no session" like "".
    let empty = HashMap::<String, SessionHandle>::new();
    assert!(resolve_cmd_session(&empty, &None, "dsh-draft", "prompt")
        .expect("placeholder")
        .is_none());
    assert!(resolve_cmd_session(&empty, &None, "", "prompt")
        .expect("no session")
        .is_none());
}

#[test]
fn bind_session_registers_current_and_adopts_pending_prompts() {
    let mut sessions = HashMap::<String, SessionHandle>::new();
    let mut current = None;
    let mut pending = VecDeque::from([Cmd::Prompt {
        session_id: "dsh-draft".into(),
        text: "early".into(),
    }]);

    bind_session(
        &mut sessions,
        &mut current,
        &mut pending,
        SessionId::new("s1"),
    );

    assert_eq!(current.expect("current").to_string(), "s1");
    assert!(pending.is_empty(), "pending prompts move into the session");
    let handle = sessions.get("s1").expect("registered");
    assert!(handle.inflight.is_none());
    assert_eq!(handle.queue.len(), 1);
    assert!(matches!(
        handle.queue.front(),
        Some(Cmd::Prompt { text, .. }) if text == "early"
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_run_concurrent_prompts_on_one_connection() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let new_calls = Arc::new(AtomicUsize::new(0));
    let (arrived_tx, mut arrived_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (release_s1_tx, release_s1_rx) = tokio::sync::oneshot::channel::<()>();
    let release_s1_tx = Arc::new(Mutex::new(Some(release_s1_tx)));
    let release_s1_rx = Arc::new(Mutex::new(Some(release_s1_rx)));

    let agent = Agent
        .builder()
        .name("multi-session-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("multi-session-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let new_calls = Arc::clone(&new_calls);
                async move |_req: NewSessionRequest, responder, _cx| {
                    let n = new_calls.fetch_add(1, Ordering::SeqCst);
                    responder.respond(NewSessionResponse::new(SessionId::new(format!(
                        "s{}",
                        n + 1
                    ))))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let release_s1_tx = Arc::clone(&release_s1_tx);
                let release_s1_rx = Arc::clone(&release_s1_rx);
                async move |req: PromptRequest, responder, cx| {
                    let sid = req.session_id.to_string();
                    let _ = arrived_tx.send(sid.clone());
                    if sid == "s1" {
                        // Hold s1's response until s2's prompt also arrived:
                        // with a single connection-level in-flight prompt this
                        // would deadlock, so completing proves the demux.
                        let release = release_s1_rx.lock().unwrap().take();
                        cx.spawn(async move {
                            if let Some(release) = release {
                                let _ = release.await;
                            }
                            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                            Ok(())
                        })?;
                        Ok(())
                    } else {
                        if let Some(tx) = release_s1_tx.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // Startup binds s1; /new binds s2 and becomes the fallback session.
    cmd_tx.send(Cmd::NewSession { requester: None, retry_auth: None }).expect("new session");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bound = std::collections::HashSet::new();
    while Instant::now() < deadline && !bound.contains("s2") {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) => {
                bound.insert(session_id);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(bound.contains("s2"), "second session bound: {bound:?}");

    // Prompt s1 explicitly even though s2 is the most recently bound session.
    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "a".into(),
        })
        .expect("prompt s1");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), arrived_rx.recv())
            .await
            .expect("s1 prompt reaches the agent")
            .as_deref(),
        Some("s1"),
        "the carried session id addresses the prompt, not the fallback",
    );
    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s2".into(),
            text: "b".into(),
        })
        .expect("prompt s2");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), arrived_rx.recv())
            .await
            .expect("s2 prompt reaches the agent while s1 is still in flight")
            .as_deref(),
        Some("s2"),
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut turn_ends = std::collections::HashSet::new();
    while Instant::now() < deadline && turn_ends.len() < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
                if kind == "completed" =>
            {
                turn_ends.insert(session);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(
        turn_ends,
        std::collections::HashSet::from(["s1".to_string(), "s2".to_string()]),
        "each session's completion is tagged with its own id",
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_text_prompt_finish_cannot_release_a_rebound_sessions_new_turn() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
        SessionCapabilities, SessionResumeCapabilities, StopReason,
    };
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    let (old_tx, old_rx) = tokio::sync::oneshot::channel::<()>();
    let (new_tx, new_rx) = tokio::sync::oneshot::channel::<()>();
    let (third_tx, third_rx) = tokio::sync::oneshot::channel::<()>();
    let releases = Arc::new(Mutex::new(VecDeque::from([old_rx, new_rx, third_rx])));
    let (arrived_tx, mut arrived_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let agent = Agent
        .builder()
        .name("stale-finish-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new().session_capabilities(
                            SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
                        ))
                        .agent_info(Implementation::new("stale-finish-mock", "0")),
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
            async move |_req: ResumeSessionRequest, responder, _cx| {
                responder.respond(ResumeSessionResponse::new())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let releases = Arc::clone(&releases);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = arrived_tx.send(text);
                    let release = releases.lock().unwrap().pop_front();
                    cx.spawn(async move {
                        if let Some(release) = release {
                            let _ = release.await;
                        }
                        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "old".into(),
        })
        .expect("old prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), arrived_rx.recv())
            .await
            .expect("old prompt arrival")
            .as_deref(),
        Some("old")
    );
    cmd_tx
        .send(Cmd::ForgetSession {
            session_id: "s1".into(),
        })
        .expect("forget old binding");
    cmd_tx
        .send(Cmd::ResumeSession {
            session_id: "s1".into(),
        })
        .expect("resume same id");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut rebound = false;
    while Instant::now() < deadline && !rebound {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. })) if session_id == "s1" => {
                rebound = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(rebound, "same Session id rebound");

    for text in ["new", "queued"] {
        cmd_tx
            .send(Cmd::Prompt {
                session_id: "s1".into(),
                text: text.into(),
            })
            .expect("prompt after rebound");
    }
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), arrived_rx.recv())
            .await
            .expect("new prompt arrival")
            .as_deref(),
        Some("new")
    );

    old_tx.send(()).expect("release stale prompt");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), arrived_rx.recv())
            .await
            .is_err(),
        "the stale finish must not release the queued prompt"
    );
    new_tx.send(()).expect("release current prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), arrived_rx.recv())
            .await
            .expect("queued prompt arrival")
            .as_deref(),
        Some("queued")
    );
    let _ = third_tx.send(());
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_for_an_unbound_session_is_rejected_not_rerouted() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("unknown-session-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("unknown-session-mock", "0")),
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
            async move |req: PromptRequest, responder, _cx| {
                let _ = prompt_tx.send(req.session_id.to_string());
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => break,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "ghost".into(),
            text: "boo".into(),
        })
        .expect("prompt for an unbound session");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut rejection = None;
    while Instant::now() < deadline && rejection.is_none() {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionError { session_id, message }))
                if message.contains("unknown session") =>
            {
                rejection = Some((session_id, message));
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(
        rejection,
        Some(("ghost".to_string(), "prompt: unknown session ghost".to_string()))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), prompt_rx.recv())
            .await
            .is_err(),
        "an unbound session id must not be rerouted to the fallback session",
    );

    // The bound session keeps working after the rejection.
    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "real".into(),
        })
        .expect("prompt for the bound session");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("bound session still prompts")
            .as_deref(),
        Some("s1"),
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[test]
fn file_uris_are_percent_encoded() {
    use std::path::Path;

    assert_eq!(
        super::unix_file_uri(Path::new("/tmp/shot.png")),
        "file:///tmp/shot.png",
        "plain ASCII paths stay readable"
    );
    assert_eq!(
        super::unix_file_uri(Path::new("/tmp/a b/c#d?e.png")),
        "file:///tmp/a%20b/c%23d%3Fe.png",
        "space, # and ? must not change URI semantics"
    );
    assert_eq!(
        super::unix_file_uri(Path::new("/tmp/截图.png")),
        "file:///tmp/%E6%88%AA%E5%9B%BE.png",
        "non-ASCII is encoded byte-wise (RFC 3986)"
    );
}

#[test]
fn auth_stalls_and_retries_stay_on_the_owning_harness() {
    let surface = Arc::new(Mutex::new(Surface::default()));
    {
        let mut s = surface.lock().unwrap();
        for (id, owner) in [("a", "alpha"), ("a2", "alpha"), ("b", "beta")] {
            s.session_mut(Some(id)).connection = Some(json!({"id":owner}));
        }
    }
    let mut parked = VecDeque::from([ParkedPrompt {
        session: Some("b".into()), kind: ParkedPromptKind::Text("beta retry".into()),
    }]);
    assert!(!auth_stalled_for("a", &parked, &surface));
    assert!(auth_stalled_for("b", &parked, &surface));
    let mut sessions = HashMap::from([("a".into(), SessionHandle::default()), ("b".into(), SessionHandle::default())]);
    assert_eq!(requeue_connection_prompts(&mut sessions, &None, &mut parked, &surface, Some("alpha")), 0);
    assert_eq!(parked.len(), 1);
    assert_eq!(requeue_connection_prompts(&mut sessions, &None, &mut parked, &surface, Some("beta")), 1);
    assert!(parked.is_empty());
    assert_eq!(sessions["b"].queue.len(), 1);
    assert_eq!(sessions["b"].auth_retries, 1);
    assert!(sessions["a"].queue.is_empty());
    assert_eq!(sessions["a"].auth_retries, 0);
}

#[test]
fn stream_owned_new_session_keeps_the_negotiated_connection_facts() {
    let surface = Arc::new(Mutex::new(Surface::default()));
    surface.lock().unwrap().initial_connection = Some(crate::bus::SessionConnection {
        server: Some("profile-host".into()), auth: AuthSnapshot::none(),
        load_session: true, list_session: true, resume_session: false,
        fork_session: false,
        close_session: false,
        additional_directories: false,
    });
    let (bus, events) = std::sync::mpsc::channel();
    apply_setup(&json!({"sessionId":"stream-second"}), None, &surface, &bus);
    let snapshot = events.try_iter().find_map(|event| match event {
        AppEvent::Ctl(CtlEvent::SessionConnection { session_id, connection }) if session_id == "stream-second" => Some(connection),
        _ => None,
    }).expect("Every stream-owned tab receives the same negotiated connection");
    assert_eq!(snapshot.server.as_deref(), Some("profile-host"));
    assert!(snapshot.load_session);
    assert!(snapshot.list_session);
}

#[test]
fn structured_auth_failure_opens_owning_connection_and_parks_original_prompt() {
    let finish = PromptFinish {
        session_id: "claude-session".into(),
        result: Err(serde_json::from_value(json!({
            "code": -32603, "message": "Internal error",
            "data": {
                "errorKind": "authentication_failed",
                "details": "403 Insufficient account balance",
                "marttyConnection": {
                    "id": "h3", "command": "claude-acp",
                    "authMethods": [{"id": "h3:login", "name": "Claude login"}]
                }
            }
        })).unwrap()),
        payload: ParkedPromptKind::Text("original request".into()), gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();
    apply_prompt_finish(finish, &mut HashMap::new(), &mut parked, &tx, &[], None);
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].session.as_deref(), Some("claude-session"));
    assert!(matches!(&parked[0].kind, ParkedPromptKind::Text(text) if text == "original request"));
    assert!(matches!(rx.try_recv(), Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { kind, .. })) if kind == "interrupted"));
    match rx.try_recv().unwrap() {
        AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open }) => {
            assert_eq!(session_id, "claude-session");
            assert!(open);
            assert_eq!(snapshot.status, AuthStatus::NeedsAuth);
            assert_eq!(snapshot.message.as_deref(), Some("403 Insufficient account balance"));
            assert_eq!(snapshot.methods[0].id, "h3:login");
            // The notice names the advertised method — never the bare
            // type word "agent" (issue #160).
            assert_eq!(snapshot.method_label(), Some("Claude login"));
        }
        _ => panic!("expected owning-session authentication panel"),
    }
    assert!(rx.try_recv().is_err());
}

#[test]
fn structured_auth_failure_names_the_failing_provider() {
    // The refusal names the credential that failed — the provider the
    // session runs on — never the method the user last signed in with
    // nor the bare "agent" type (issue #160).
    let selected = AuthMethodInfo {
        id: "api-key:deepseek".into(),
        name: Some("DeepSeek API key".into()),
        description: None,
        type_name: "agent".into(),
        form: true,
        vars: Vec::new(),
        link: None,
        terminal_launch: None,
    };
    let finish = PromptFinish {
        session_id: "s".into(),
        result: Err(serde_json::from_value(json!({
            "code": -32603, "message": "Internal error",
            "data": {
                "errorKind": "authentication_failed", "details": "401",
                "marttyConnection": {"id": "h", "authMethods": [{"id": "h:login", "name": "Claude login"}]}
            }
        })).unwrap()),
        payload: ParkedPromptKind::Text("x".into()),
        gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();
    apply_prompt_finish(
        finish,
        &mut HashMap::new(),
        &mut parked,
        &tx,
        std::slice::from_ref(&selected),
        Some("deepseek/deepseek-v4-pro"),
    );
    let snapshot = rx
        .try_iter()
        .find_map(|event| match event {
            AppEvent::Ctl(CtlEvent::SessionAuth { snapshot, .. }) => Some(snapshot),
            _ => None,
        })
        .expect("SessionAuth");
    assert_eq!(snapshot.status, AuthStatus::NeedsAuth);
    assert_eq!(snapshot.method_label(), Some("DeepSeek API key"));
}

#[test]
fn name_needs_auth_matches_the_failing_provider() {
    let method = |id: &str, name: &str| AuthMethodInfo {
        id: id.into(),
        name: Some(name.into()),
        description: None,
        type_name: "agent".into(),
        form: false,
        vars: Vec::new(),
        link: None,
        terminal_launch: None,
    };
    let blank = || AuthSnapshot {
        status: AuthStatus::NeedsAuth,
        method_id: None,
        method_name: None,
        methods: Vec::new(),
        message: None,
    };
    let deepseek = method("api-key:deepseek", "DeepSeek API key");
    let deepseek_official = method("api-key:deepseek-official", "DeepSeek official key");
    let openrouter = method("api-key:openrouter", "OpenRouter API key");
    let methods = vec![deepseek.clone(), deepseek_official.clone(), openrouter.clone()];
    // The session's model names the failing credential — a last-used
    // method on another provider is never repeated (issue #160).
    let mut snap = AuthSnapshot { methods: methods.clone(), ..blank() };
    name_needs_auth(&mut snap, &methods, Some("deepseek/deepseek-v4-pro"));
    assert_eq!(snap.method_label(), Some("DeepSeek API key"));
    // The outer advertised list matches when the snapshot's own lacks it.
    let mut snap = blank();
    name_needs_auth(&mut snap, &methods, Some("openrouter"));
    assert_eq!(snap.method_label(), Some("OpenRouter API key"));
    // A `provider::model` value resolves by its exact head — never by
    // hyphen-prefix or substring guessing on method ids (issue #160).
    let mut snap = blank();
    name_needs_auth(&mut snap, &methods, Some("deepseek-official::deepseek-flash"));
    assert_eq!(snap.method_label(), Some("DeepSeek official key"));
    // "deepseek-official" is not "deepseek": an id that merely contains
    // the token does not name the credential.
    let mut snap = blank();
    name_needs_auth(
        &mut snap,
        &[deepseek.clone(), openrouter.clone()],
        Some("deepseek-official"),
    );
    assert_eq!(snap.method_label(), None);
    // A lone advertised method is the only credential possible.
    let login = method("h3:login", "Claude login");
    let mut snap = AuthSnapshot { methods: vec![login], ..blank() };
    name_needs_auth(&mut snap, &[], Some("deepseek"));
    assert_eq!(snap.method_label(), Some("Claude login"));
    // An unmatched or ambiguous provider stays generic — never a guess.
    let mut snap = AuthSnapshot { methods: methods.clone(), ..blank() };
    name_needs_auth(&mut snap, &methods, Some("anthropic"));
    assert_eq!(snap.method_label(), None);
    let mut snap = blank();
    name_needs_auth(&mut snap, &methods, Some("api-key"));
    assert_eq!(snap.method_label(), None);
    // Nothing to name stays unspecific — never "agent".
    let mut snap = blank();
    name_needs_auth(&mut snap, &[], None);
    assert_eq!(snap.method_label(), None);
    // A method the snapshot already knows is never overwritten.
    let mut snap = AuthSnapshot { methods: methods.clone(), ..blank() };
    snap.method_id = Some("kept".into());
    name_needs_auth(&mut snap, &methods, Some("deepseek"));
    assert_eq!(snap.method_id.as_deref(), Some("kept"));
    assert_eq!(snap.method_name, None);
}

#[test]
fn auth_provider_never_guesses_the_launch_provider() {
    // Without a bound session the credential in play is unknown: the
    // launch-config provider is Martty's own preset, not something the
    // agent named — the notice stays generic (issue #160).
    let surface = std::sync::Arc::new(std::sync::Mutex::new(Surface::default()));
    assert_eq!(auth_provider(&surface, None), None);
    assert_eq!(auth_provider(&surface, Some("s-missing")), None);
}

#[test]
fn provider_head_reads_the_model_value_head() {
    // Provider ids come straight from the agent's model value —
    // `provider/model` or `provider::model` — never expanded into
    // hyphen-prefix guesses (issue #160).
    assert_eq!(provider_head("deepseek-official::deepseek-v4-pro"), "deepseek-official");
    assert_eq!(provider_head("openrouter/deepseek/deepseek-chat"), "openrouter");
    assert_eq!(provider_head("-edge"), "-edge");
    assert_eq!(provider_head("plain"), "plain");
}

#[test]
fn method_for_provider_matches_the_exact_provider_suffix_only() {
    // `api-key:openrouter` signs openrouter in — an exact id suffix, not a
    // substring hunt (issue #160).
    let method = |id: &str| AuthMethodInfo {
        id: id.into(),
        name: Some(id.to_string()),
        description: None,
        type_name: "agent".into(),
        form: false,
        vars: Vec::new(),
        link: None,
        terminal_launch: None,
    };
    let deepseek = method("api-key:deepseek-official");
    let openrouter = method("api-key:openrouter");
    let methods = vec![deepseek, openrouter];
    assert_eq!(
        method_for_provider(&methods, "deepseek-official").map(|m| m.id.as_str()),
        Some("api-key:deepseek-official")
    );
    assert_eq!(
        method_for_provider(&methods, "OPENROUTER").map(|m| m.id.as_str()),
        Some("api-key:openrouter")
    );
    // A different or unknown provider never borrows a name.
    assert!(method_for_provider(&methods, "deepseek").is_none());
    assert!(method_for_provider(&methods, "anthropic").is_none());
}

#[test]
fn held_prompt_notice_names_the_method() {
    // A held prompt's SessionAuth names the credential — the sole
    // advertised method when the provider cannot resolve — never the
    // bare "agent" type (issue #160).
    let method = AuthMethodInfo {
        id: "login".into(),
        name: Some("Login".into()),
        description: None,
        type_name: "agent".into(),
        form: true,
        vars: Vec::new(),
        link: None,
        terminal_launch: None,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    emit_held_prompt(&tx, "s1", std::slice::from_ref(&method), None);
    emit_held_prompt(&tx, "s1", std::slice::from_ref(&method), Some("deepseek"));
    let mut labels = Vec::new();
    for event in rx.try_iter() {
        match event {
            AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open }) => {
                assert_eq!(session_id, "s1");
                assert!(open);
                labels.push(snapshot.method_label().map(str::to_string));
            }
            _ => panic!("expected SessionAuth"),
        }
    }
    assert_eq!(labels, [Some("Login".to_string()), Some("Login".to_string())]);
}

/// A refused session/new carrying `marttyConnection` data names an
/// advertised method — never the bare type word "agent" (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn setup_refusal_snapshot_names_the_advertised_method() {
    use agent_client_protocol::schema::v1::{
        AuthMethod, AuthMethodAgent, InitializeRequest, InitializeResponse,
        NewSessionRequest,
    };
    use std::time::{Duration, Instant};

    let agent = Agent
        .builder()
        .name("setup-refusal-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_info(Implementation::new("setup-refusal-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond_with_error(serde_json::from_value(json!({
                    "code": -32000,
                    "message": "authentication required",
                    "data": {
                        "marttyConnection": {
                            "id": "h1",
                            "authMethods": [{"id": "login", "name": "Login"}]
                        }
                    }
                }))
                .unwrap())
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // The initial session/new fails the same way — the refusal opens
    // the picker through create_prompt_session.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut opened = false;
    while Instant::now() < deadline && !opened {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => opened = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(opened, "the refused initial bind opens the picker");

    // A rebind for an existing tab receives a SessionAuth snapshot that
    // names the advertised method.
    cmd_tx
        .send(Cmd::NewSession {
            requester: Some("s-old".into()),
            retry_auth: Some("login".into()),
        })
        .expect("rebind cmd");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut named = false;
    while Instant::now() < deadline && !named {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open }))
                if session_id == "s-old" && snapshot.status == AuthStatus::NeedsAuth =>
            {
                assert!(open);
                assert_eq!(snapshot.method_label(), Some("Login"));
                named = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(named, "the refusal snapshot names a method");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[test]
fn steer_route_reads_initialize_meta() {
    assert_eq!(
        steer_route(&json!({ "_meta": { "steering": { "supported": true } } })),
        SteerRoute::SessionSteering
    );
    assert_eq!(
        steer_route(&json!({
            "agentCapabilities": { "_meta": { "steering": { "supported": true } } }
        })),
        SteerRoute::SessionSteering
    );
    assert_eq!(
        steer_route(&json!({
            "_meta": { "minimax-code/extensions": {
                "version": 1,
                "methods": ["session/activate", "mcode/session/steer"]
            } }
        })),
        SteerRoute::McodeSteer
    );
    // No steer method listed: fall back to the concurrent prompt.
    assert_eq!(
        steer_route(&json!({
            "_meta": { "minimax-code/extensions": { "methods": ["session/activate"] } }
        })),
        SteerRoute::ConcurrentPrompt
    );
    assert_eq!(
        steer_route(&json!({
            "agentCapabilities": { "_meta": { "claudeCode": { "promptQueueing": true } } }
        })),
        SteerRoute::ConcurrentPrompt
    );
    assert_eq!(steer_route(&json!({})), SteerRoute::ConcurrentPrompt);
}

#[test]
fn session_steering_request_injects_or_defers() {
    let sid = SessionId::new("s");
    match steer_request(SteerRoute::SessionSteering, &sid, vec!["go".into()], 7).unwrap() {
        SteerRequest::Ext(message) => {
            assert_eq!(message.method, "_session/steering");
            assert_eq!(message.params["sessionId"], "s");
            assert_eq!(message.params["prompt"][0]["type"], "text");
            assert_eq!(message.params["prompt"][0]["text"], "go");
            assert_eq!(message.params["_meta"]["steering"]["idleBehavior"], "promptRequired");
        }
        _ => panic!("dsh steering is an extension request"),
    }
    let route = SteerRoute::SessionSteering;
    assert!(!steer_ext_deferred(route, &Ok(json!({ "outcome": "injected" }))));
    // claude-agent-acp without `idleBehavior`: the agent took it as a new turn.
    assert!(!steer_ext_deferred(route, &Ok(json!({ "outcome": "startedNewTurn" }))));
    assert!(steer_ext_deferred(
        route,
        &Ok(json!({ "outcome": "promptRequired", "reason": "noRunningTurn" }))
    ));
    assert!(steer_ext_deferred(route, &Err(AcpError::new(-32602, "bad"))));
}

#[test]
fn mcode_steer_request_is_text_only() {
    let sid = SessionId::new("s");
    match steer_request(SteerRoute::McodeSteer, &sid, vec!["a".into(), "b".into()], 9).unwrap() {
        SteerRequest::Ext(message) => {
            assert_eq!(message.method, "mcode/session/steer");
            assert_eq!(message.params["sessionId"], "s");
            assert_eq!(message.params["text"], "a\nb");
            assert_eq!(message.params["clientRequestId"], "martty-steer-9");
        }
        _ => panic!("mcode steer is an extension request"),
    }
    let with_image = vec![
        "look".into(),
        ContentBlock::ResourceLink(ResourceLink::new("shot.png", "file:///tmp/shot.png")),
    ];
    assert!(matches!(
        steer_request(SteerRoute::McodeSteer, &sid, with_image, 9).unwrap(),
        SteerRequest::Unsupported
    ));
    assert!(matches!(
        steer_request(SteerRoute::McodeSteer, &sid, vec![" ".into()], 9).unwrap(),
        SteerRequest::Unsupported
    ));
    assert!(!steer_ext_deferred(
        SteerRoute::McodeSteer,
        &Ok(json!({ "turnId": "t1", "mode": "steered" }))
    ));
    assert!(steer_ext_deferred(SteerRoute::McodeSteer, &Err(AcpError::new(-32602, "no turn"))));
    assert!(matches!(
        steer_request(SteerRoute::ConcurrentPrompt, &sid, vec!["x".into()], 1).unwrap(),
        SteerRequest::Prompt(content) if content.len() == 1
    ));
}

/// Concurrent-prompt steer against an agent that cannot join a turn
/// (pi-acp queues the second `session/prompt` and runs it as the next
/// turn): the primary prompt returns first, then the queued turn streams,
/// then the steer's own response arrives. The session must stay busy
/// through that queued turn — `session.status idle` only once the last
/// in-flight request has returned.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_steer_keeps_the_session_running_until_its_prompt_returns() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionRequest,
        NewSessionResponse, PromptResponse, StopReason,
    };
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_steer_tx, release_steer_rx) = tokio::sync::oneshot::channel::<()>();
    let releases = Arc::new(Mutex::new(VecDeque::from([release_first_rx, release_steer_rx])));

    let agent = Agent
        .builder()
        .name("queueing-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("queueing-mock", "0")),
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
                let releases = Arc::clone(&releases);
                async move |req: PromptRequest, responder, cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text);
                    let release = releases.lock().unwrap().pop_front();
                    cx.spawn(async move {
                        if let Some(release) = release {
                            let _ = release.await;
                        }
                        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                        Ok(())
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        );

    let cfg = RuntimeConfig {
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
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // Drain the bus until `predicate` matches or `wait` elapses.
    let bus_until = |predicate: &dyn Fn(&AppEvent) -> bool, wait: Duration| -> bool {
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            match bus_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(event) if predicate(&event) => return true,
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("{err}"),
            }
        }
        false
    };
    let status_is = |wanted: &'static str| {
        move |event: &AppEvent| {
            matches!(
                event,
                AppEvent::Rpc { method, params }
                    if method == "session.status"
                        && params["sessionId"] == "s1"
                        && params["status"] == wanted
            )
        }
    };
    assert!(
        bus_until(
            &|event| matches!(event, AppEvent::Ctl(CtlEvent::SessionBound { .. })),
            Duration::from_secs(2)
        ),
        "session binds"
    );

    cmd_tx
        .send(Cmd::Prompt {
            session_id: "s1".into(),
            text: "thanks".into(),
        })
        .expect("primary prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("primary prompt reaches the agent")
            .as_deref(),
        Some("thanks")
    );
    assert!(bus_until(&status_is("running"), Duration::from_secs(2)), "running");

    cmd_tx
        .send(Cmd::Steer {
            session_id: "s1".into(),
            message_id: 7,
            text: "fadf".into(),
        })
        .expect("steer");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("steer reaches the agent as a concurrent prompt")
            .as_deref(),
        Some("fadf")
    );

    // The primary prompt returns; the agent is still running the queued turn.
    let _ = release_first_tx.send(());
    assert!(
        bus_until(
            &|event| matches!(
                event,
                AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind })
                    if session == "s1" && kind == "completed"
            ),
            Duration::from_secs(2)
        ),
        "the primary turn ends"
    );
    assert!(
        !bus_until(&status_is("idle"), Duration::from_millis(400)),
        "the session must not report idle while the steer's prompt is in flight"
    );

    // The queued turn settles: the steer's `session/prompt` returns.
    let _ = release_steer_tx.send(());
    assert!(
        bus_until(
            &|event| matches!(
                event,
                AppEvent::Ctl(CtlEvent::SteerSettled { message_id: 7, deferred: false })
            ),
            Duration::from_secs(2)
        ),
        "the steer settles as delivered"
    );
    assert!(
        bus_until(&status_is("idle"), Duration::from_secs(2)),
        "idle once every in-flight prompt has returned"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_prompts_release_on_auth_skip_and_esc_visibly() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse,
        InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        PromptResponse, SessionConfigOption, SessionConfigSelectOption,
        SetSessionConfigOptionResponse, StopReason,
    };
    use std::time::{Duration, Instant};

    // The agent refuses slash input with -32000 like the real agents do, and
    // answers session/new slowly so the test can reach resolve_cmd_session's
    // empty-sessions arm deterministically (#160).
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let agent = Agent
        .builder()
        .name("auth-release-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("auth-release-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                tokio::time::sleep(Duration::from_millis(250)).await;
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: AuthenticateRequest, responder, _cx| {
                tokio::time::sleep(Duration::from_millis(300)).await;
                responder.respond(AuthenticateResponse::new())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                async move |req: PromptRequest, responder, _cx| {
                    let text = req
                        .prompt
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let _ = prompt_tx.send(text.clone());
                    if text == "auth-gated" {
                        responder.respond_with_error(AcpError::new(
                            -32000,
                            "Authentication required: authenticate with one of the advertised auth methods",
                        ))
                    } else {
                        if text.starts_with("slow") {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: SetSessionConfigOptionRequest, responder, _cx| {
                tokio::time::sleep(Duration::from_millis(300)).await;
                responder.respond(SetSessionConfigOptionResponse::new(vec![
                    SessionConfigOption::select(
                        "collaboration_mode",
                        "Collaboration mode",
                        "default",
                        vec![SessionConfigSelectOption::new("default", "Default")],
                    ),
                ]))
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |_note: CancelNotification, _cx| {
                let _ = cancel_tx.send(());
                Ok(())
            },
            on_receive_notification!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None,
        api_key: None, additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // Before the session binds the session map is empty: both commands
    // resolve to "no session" and stay silent instead of erroring.
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "ghost".into() })
        .expect("auth skip before any session");
    cmd_tx
        .send(Cmd::Interrupt { session_id: "ghost".into() })
        .expect("interrupt before any session");
    tokio::time::sleep(Duration::from_millis(80)).await;
    while let Ok(event) = bus_rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::Ctl(CtlEvent::SessionOpFailed { .. })),
            "no session means nothing to fail"
        );
    }

    // A normal prompt refused with -32000 parks behind the auth picker.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "auth-gated".into() })
        .expect("prompt");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut needs_auth = false;
    while Instant::now() < deadline && !needs_auth {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                needs_auth = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(needs_auth, "the refusal opened the picker");
    assert_eq!(
        prompt_rx.try_recv().expect("the prompt was on the wire").as_str(),
        "auth-gated"
    );

    // Esc on the picker declines sign-in: the parked prompt is dropped
    // visibly instead of stalling every later prompt.
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "s1".into() })
        .expect("auth skip");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut skipped = false;
    while Instant::now() < deadline && !skipped {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::SessionNotice { session, title, .. }))
                if session == "s1" && title.contains("sign-in skipped") =>
            {
                assert!(title.contains("1 held prompt"), "{title}");
                skipped = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(skipped, "dropping the held prompt is visible");

    // Skipping with nothing held is a silent no-op.
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "s1".into() })
        .expect("empty auth skip");
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Later input flows to the agent instead of hanging on the dead park.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "hello".into() })
        .expect("prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("hello reaches the agent after auth was skipped")
            .as_deref(),
        Some("hello")
    );

    // Another refusal re-parks; followups entered while it is held report
    // why they are waiting and never reach the wire.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "auth-gated".into() })
        .expect("prompt");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut reparked = false;
    while Instant::now() < deadline && !reparked {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                reparked = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(reparked, "second refusal re-parks");
    assert_eq!(
        prompt_rx.try_recv().expect("re-sent on the wire").as_str(),
        "auth-gated"
    );

    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "held text".into() })
        .expect("held text");
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("held image".into())],
        })
        .expect("held image");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut held = 0;
    while Instant::now() < deadline && held < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionAuth { session_id, snapshot, open }))
                if session_id == "s1"
                    && snapshot.status == AuthStatus::NeedsAuth
                    && snapshot
                        .message
                        .as_deref()
                        .is_some_and(|message| message.contains("input is held"))
                    && open =>
            {
                // Held prompts name the advertised method — never the
                // type word "agent" (issue #160).
                assert_eq!(snapshot.method_label(), Some("Login"));
                held += 1;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(held, 2, "held prompts say why they wait");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "held prompts stay off the wire"
    );

    // Esc with no live turn still resolves: held + queued sends are dropped
    // visibly and the UI's pending turn is settled.
    cmd_tx
        .send(Cmd::Interrupt { session_id: "s1".into() })
        .expect("interrupt");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut noticed = false;
    let mut interrupted = false;
    while Instant::now() < deadline && !(noticed && interrupted) {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::SessionNotice { session, title, .. }))
                if session == "s1" && title.contains("cancelled") =>
            {
                assert!(title.contains("3 pending prompts"), "{title}");
                noticed = true;
            }
            Ok(AppEvent::Ctl(CtlEvent::Interrupted { session_id }))
                if session_id == "s1" =>
            {
                interrupted = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(noticed && interrupted, "esc resolves held and queued prompts");
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "discarded prompts never reach the wire"
    );

    // Esc with nothing pending is silent.
    cmd_tx
        .send(Cmd::Interrupt { session_id: "s1".into() })
        .expect("empty interrupt");
    tokio::time::sleep(Duration::from_millis(60)).await;
    while let Ok(event) = bus_rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::Ctl(CtlEvent::Interrupted { .. })),
            "nothing pending means nothing to cancel"
        );
    }

    // The session still works afterwards.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "free".into() })
        .expect("prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("session usable after cancel")
            .as_deref(),
        Some("free")
    );

    // An image sent with no turn in flight and no control op running takes
    // the direct begin_prompt arm — the all-idle side of the busy check.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut settled = false;
    while Instant::now() < deadline && !settled {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
                if session == "s1" && kind == "completed" =>
            {
                settled = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(settled, "the free prompt's turn ended");
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("img idle".into())],
        })
        .expect("image while fully idle");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("idle image begins its own turn")
            .as_deref(),
        Some("img idle")
    );

    // Image prompts take the same paths: queued behind a live turn, sent
    // when free.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "slow-gate".into() })
        .expect("slow prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("slow prompt on the wire")
            .as_deref(),
        Some("slow-gate")
    );
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("img queued".into())],
        })
        .expect("image behind a live turn");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("queued image drains after the turn")
            .as_deref(),
        Some("img queued")
    );
    // A control-lane op in flight makes the session busy the same way.
    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "login".into(),
            values: Default::default(),
        })
        .expect("authenticate keeps the session busy");
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("img busy".into())],
        })
        .expect("image while control-busy");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("image drains once the control op settles")
            .as_deref(),
        Some("img busy")
    );
    // With no turn in flight a config-op makes the session busy and
    // queued images drain once it settles — the busy side of the check.
    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "login".into(),
            values: Default::default(),
        })
        .expect("authenticate clears the parked state");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut configured = false;
    while Instant::now() < deadline && !configured {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::Configured =>
            {
                configured = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(configured, "sign-in before the busy window");
    cmd_tx
        .send(Cmd::SetConfigOption {
            session_id: "s1".into(),
            config_id: "collaboration_mode".into(),
            value: "default".into(),
        })
        .expect("config op keeps the session busy");
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("img busy again".into())],
        })
        .expect("image while control-busy without a live turn");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("image drains once the control op settles")
            .as_deref(),
        Some("img busy again")
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    cmd_tx
        .send(Cmd::PromptImages {
            session_id: "s1".into(),
            blocks: vec![crate::bus::PromptBlock::Text("img begin".into())],
        })
        .expect("image while free");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("image sent while free")
            .as_deref(),
        Some("img begin")
    );

    // A single parked send reports the singular form.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "auth-gated".into() })
        .expect("re-park one");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut reparked = false;
    while Instant::now() < deadline && !reparked {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                reparked = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(reparked, "one prompt re-parks behind sign-in");
    assert_eq!(
        prompt_rx.try_recv().expect("the prompt was on the wire").as_str(),
        "auth-gated"
    );
    cmd_tx
        .send(Cmd::Interrupt { session_id: "s1".into() })
        .expect("interrupt one");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut singular = false;
    while Instant::now() < deadline && !singular {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::SessionNotice { session, title, .. }))
                if session == "s1" && title.contains("cancelled") =>
            {
                assert!(title.contains("1 pending prompt"), "{title}");
                assert!(!title.contains("prompts"), "{title}");
                singular = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(singular, "one dropped send uses the singular notice");

    // Unknown sessions report the failure for both commands.
    cmd_tx
        .send(Cmd::Interrupt { session_id: "ghost".into() })
        .expect("interrupt unknown");
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "ghost".into() })
        .expect("auth skip unknown");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut failures = std::collections::BTreeSet::new();
    while Instant::now() < deadline && failures.len() < 2 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionOpFailed { session_id, message }))
                if session_id == "ghost" =>
            {
                failures.insert(message);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(
        failures.iter().any(|message| message.contains("interrupt")),
        "{failures:?}"
    );
    assert!(
        failures.iter().any(|message| message.contains("auth skip")),
        "{failures:?}"
    );

    // A live turn still takes session/cancel on the wire — the
    // refused-prompt cleanup must not degrade interrupt handling (#160).
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "slow-turn".into() })
        .expect("live prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("live prompt sent")
            .as_deref(),
        Some("slow-turn")
    );
    cmd_tx
        .send(Cmd::Interrupt { session_id: "s1".into() })
        .expect("interrupt a live turn");
    tokio::time::timeout(Duration::from_secs(2), cancel_rx.recv())
        .await
        .expect("session/cancel reaches the agent");
    tokio::time::sleep(Duration::from_millis(400)).await;
    while let Ok(event) = bus_rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::Ctl(CtlEvent::SessionOpFailed { .. })),
            "interrupting a live turn does not fail"
        );
    }

    // Catalog requests just read the cached session surface.
    cmd_tx
        .send(Cmd::FetchCatalog { session_id: "s1".into() })
        .expect("catalog for s1");

    // Once the last session is gone the map is empty again: the same
    // commands resolve to "no session" and stay silent (Ok(None) arm).
    cmd_tx
        .send(Cmd::ForgetSession { session_id: "s1".into() })
        .expect("forget s1");
    tokio::time::sleep(Duration::from_millis(80)).await;
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "ghost".into() })
        .expect("auth skip with no sessions");
    cmd_tx
        .send(Cmd::Interrupt { session_id: "ghost".into() })
        .expect("interrupt with no sessions");
    tokio::time::sleep(Duration::from_millis(80)).await;
    while let Ok(event) = bus_rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::Ctl(CtlEvent::SessionOpFailed { .. })),
            "no sessions means nothing to fail"
        );
    }

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

/// A refusal names the credential the session actually runs on — never
/// the method the user signed in with last (issue #160): signed in with
/// OpenRouter while the session's model stays DeepSeek, the notice must
/// say "DeepSeek API key".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_prompt_names_the_failing_provider_not_the_last_sign_in() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateResponse,
        InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
        SessionConfigOption, SessionConfigSelectOption,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("cross-provider-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("cross-provider-mock", "0"))
                        .auth_methods(vec![
                            AuthMethod::Agent(AuthMethodAgent::new(
                                "pi-terminal",
                                "Log in with pi",
                            )),
                            AuthMethod::Agent(AuthMethodAgent::new(
                                "api-key:deepseek",
                                "DeepSeek API key",
                            )),
                            AuthMethod::Agent(AuthMethodAgent::new(
                                "api-key:openrouter",
                                "OpenRouter API key",
                            )),
                        ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                        SessionConfigOption::select(
                            "model",
                            "Model",
                            "deepseek/deepseek-v4-pro",
                            vec![
                                SessionConfigSelectOption::new(
                                    "deepseek/deepseek-v4-pro",
                                    "DeepSeek V4 Pro",
                                ),
                                SessionConfigSelectOption::new("openrouter/or-x", "OR X"),
                            ],
                        ),
                    ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: AuthenticateRequest, responder, _cx| {
                responder.respond(AuthenticateResponse::new())
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, _cx| {
                let text = req
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let _ = prompt_tx.send(text);
                responder.respond_with_error(AcpError::new(
                    -32000,
                    "Authentication required: provider returned 401",
                ))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None,
        api_key: None, additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // Sign in with OpenRouter last — the session's model still runs
    // DeepSeek, so the 401 is DeepSeek's credential.
    cmd_tx
        .send(Cmd::Authenticate {
            method_id: "api-key:openrouter".into(),
            values: Default::default(),
        })
        .expect("authenticate");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut configured = false;
    while Instant::now() < deadline && !configured {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::Configured =>
            {
                configured = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(configured, "openrouter sign-in lands");

    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "hi".into() })
        .expect("prompt");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut named = false;
    while Instant::now() < deadline && !named {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                assert_eq!(
                    snapshot.method_label(),
                    Some("DeepSeek API key"),
                    "the failing credential is DeepSeek's, not the last sign-in"
                );
                named = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(named, "the refusal names the provider's credential");
    assert_eq!(
        prompt_rx.try_recv().expect("the prompt was on the wire").as_str(),
        "hi"
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

/// Control-plane ops refused with `auth_required` emit the auth notice on
/// the provider's credential too — the session's model says DeepSeek, so
/// the notice names "DeepSeek API key" (#160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_control_ops_name_the_failing_provider() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeRequest,
        InitializeResponse, ListSessionsRequest, NewSessionRequest,
        NewSessionResponse, SessionCapabilities, SessionConfigOption,
        SessionConfigSelectOption, SessionListCapabilities, SetSessionConfigOptionRequest,
    };
    use std::time::{Duration, Instant};

    let agent = Agent
        .builder()
        .name("control-refusals-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new().session_capabilities(
                                SessionCapabilities::new().list(SessionListCapabilities::new()),
                            ),
                        )
                        .agent_info(Implementation::new("control-refusals-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "api-key:deepseek",
                            "DeepSeek API key",
                        ))]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(
                    NewSessionResponse::new(SessionId::new("s1")).config_options(vec![
                        SessionConfigOption::select(
                            "model",
                            "Model",
                            "deepseek/deepseek-v4-pro",
                            vec![SessionConfigSelectOption::new(
                                "deepseek/deepseek-v4-pro",
                                "DeepSeek V4 Pro",
                            )],
                        ),
                    ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: SetSessionConfigOptionRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32000, "Authentication required"))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: ListSessionsRequest, responder, _cx| {
                responder.respond_with_error(AcpError::new(-32000, "Authentication required"))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None,
        api_key: None, additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) =
            bus_rx.recv_timeout(Duration::from_millis(20))
        {
            break;
        }
    }
    cmd_tx
        .send(Cmd::SelectModel {
            session_id: "s1".into(),
            provider: None,
            model: Some("deepseek/deepseek-v4-flash".into()),
            effort: Some("high".into()),
        })
        .expect("select model");
    cmd_tx
        .send(Cmd::SetPreset { session_id: "s1".into(), preset: "auto".into() })
        .expect("set preset");
    cmd_tx
        .send(Cmd::SetConfigOption {
            session_id: "s1".into(),
            config_id: "mode".into(),
            value: "plan".into(),
        })
        .expect("set config option");
    cmd_tx
        .send(Cmd::ListSessions {
            requester_session_id: "s1".into(),
            prefix: None,
            limit: 5,
        })
        .expect("list sessions");

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut noticed = 0usize;
    while Instant::now() < deadline && noticed < 5 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot)))
                if snapshot.status == AuthStatus::NeedsAuth =>
            {
                assert_eq!(
                    snapshot.method_label(),
                    Some("DeepSeek API key"),
                    "control refusals name the provider's credential"
                );
                noticed += 1;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(noticed, 5, "model, effort, preset, config option, and session list refusals");

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

/// Esc on the sign-in picker drops sends queued behind a live turn too —
/// not only parked refusals — so a queued `/login` cannot pin the session
/// after the user declines to sign in (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_skip_also_discards_prompts_queued_behind_a_live_turn() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse, PromptResponse,
        StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let agent = Agent
        .builder()
        .name("auth-skip-queue-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("auth-skip-queue-mock", "0"))
                        .auth_methods(vec![AuthMethod::Agent(AuthMethodAgent::new(
                            "login", "Login",
                        ))]),
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
            async move |req: PromptRequest, responder, _cx| {
                let text = req
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let _ = prompt_tx.send(text.clone());
                if text.starts_with("slow") {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None,
        api_key: None, additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bound = false;
    while Instant::now() < deadline && !bound {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => bound = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(bound, "the session bound");

    // A live turn is in flight; the next send queues behind it.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "slow-turn".into() })
        .expect("slow prompt");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("slow prompt on the wire")
            .as_deref(),
        Some("slow-turn")
    );
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "/login".into() })
        .expect("queued prompt");

    // Esc on the picker while the queue holds the send: it is discarded
    // visibly, like a parked refusal (issue #160).
    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "s1".into() })
        .expect("auth skip");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut skipped = false;
    while Instant::now() < deadline && !skipped {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::SessionNotice { session, title, .. }))
                if session == "s1" && title.contains("sign-in skipped") =>
            {
                assert!(title.contains("1 held prompt"), "{title}");
                skipped = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(skipped, "dropping the queued send is visible");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        prompt_rx.try_recv().is_err(),
        "the queued send never reached the wire"
    );

    // The user carries on: later sends go straight to the agent.
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "carry on".into() })
        .expect("prompt after skip");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), prompt_rx.recv())
            .await
            .expect("later sends reach the agent after Esc")
            .as_deref(),
        Some("carry on")
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

/// Misc command-edge drives: bogus session ids error visibly instead of
/// silently resolving, pre-session control ops are skipped, a queued send
/// settles as a steer finish, catalog answers carry modes when the agent
/// advertises them, and the static-plugins extension reports its failure
/// (issue #160 coverage).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_command_edges() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeRequest,
        InitializeResponse, NewSessionRequest, NewSessionResponse, PromptResponse,
        SessionMode, SessionModeState, StopReason,
    };
    use std::time::{Duration, Instant};

    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    static NEWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static AUTHS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let agent = Agent
        .builder()
        .name("edges-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("edges-mock", "0"))
                        .auth_methods(vec![
                            AuthMethod::Agent(AuthMethodAgent::new("login", "Login")),
                            AuthMethod::Agent(AuthMethodAgent::new("gateway", "Gateway")),
                        ]),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                match NEWS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                    // Startup bind is auth-refused: the picker opens and a
                    // typed prompt parks behind sign-in — Esc drops it
                    // (issue #160).
                    0 => responder.respond_with_error(serde_json::from_value(json!({
                        "code": -32000,
                        "message": "authentication required",
                        "data": {
                            "marttyConnection": {
                                "id": "h1",
                                "authMethods": [{"id": "login", "name": "Login"}]
                            }
                        }
                    }))
                    .unwrap()),
                    // A non-auth session/new failure surfaces as a plain
                    // error — the loop stays alive.
                    1 => responder.respond_with_error(serde_json::from_value(json!({
                        "code": -32603,
                        "message": "session backend down"
                    }))
                    .unwrap()),
                    2 => responder.respond(
                        NewSessionResponse::new(SessionId::new("s1")).modes(
                            SessionModeState::new(
                                "default",
                                vec![SessionMode::new("default", "Default")],
                            ),
                        ),
                    ),
                    // A rebind refusal carrying the connection snapshot:
                    // non-auth failure reports as a failure, an auth one
                    // re-arms sign-in naming the credential.
                    3 => responder.respond_with_error(serde_json::from_value(json!({
                        "code": -32603,
                        "message": "backend exploded",
                        "data": {
                            "marttyConnection": {
                                "id": "h1",
                                "auth": {"status": "configured", "methodId": "login"}
                            }
                        }
                    }))
                    .unwrap()),
                    _ => responder.respond_with_error(serde_json::from_value(json!({
                        "code": -32000,
                        "message": "authentication required",
                        "data": {
                            "marttyConnection": {
                                "id": "h1",
                                "authMethods": [{"id": "login", "name": "Login"}],
                                "auth": {"status": "configured", "methodId": "login"}
                            }
                        }
                    }))
                    .unwrap()),
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: AuthenticateRequest, responder, _cx| {
                match AUTHS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                    2 => responder.respond_with_error(serde_json::from_value(json!({
                        "code": -32603,
                        "message": "key rejected"
                    }))
                    .unwrap()),
                    _ => responder.respond(serde_json::from_value(json!({})).unwrap()),
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, _cx| {
                let text = req
                    .prompt
                    .iter()
                    .find_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let _ = prompt_tx.send(text);
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(), cordis: "demo".into(), workspace: "/tmp".into(),
        session_root: "/tmp".into(), provider: "deepseek-official".into(),
        model: "deepseek-flash".into(), max_tokens: None, base_url: None,
        api_key: None, additional_directories: Vec::new(),
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect_once(agent, cfg, bus_tx, cmd_rx).await });

    // Pre-session control ops targeting a named session are skipped
    // silently — there is nothing to fail.
    cmd_tx
        .send(Cmd::SelectModel { session_id: "ghost".into(), provider: None, model: Some("x".into()), effort: None })
        .expect("pre-session model op");
    cmd_tx
        .send(Cmd::FetchCatalog { session_id: "ghost".into() })
        .expect("pre-session catalog");

    // The startup session/new is auth-refused: the picker opens and any
    // prompt typed now parks behind sign-in — Esc must drop it (issue
    // #160). Pre-session ops above must not have produced failures.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut needs_auth = false;
    while Instant::now() < deadline && !needs_auth {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => needs_auth = true,
            Ok(AppEvent::Ctl(CtlEvent::SessionOpFailed { message, .. })) => {
                panic!("pre-session op must not fail: {message}")
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(needs_auth, "auth refusal opens the picker");
    cmd_tx
        .send(Cmd::Prompt { session_id: "".into(), text: "first".into() })
        .expect("prompt while auth is pending");

    // A steer while a prompt is parked but no session exists settles
    // deferred — there is nothing in flight to cancel yet (issue #160).
    cmd_tx
        .send(Cmd::Steer { session_id: "ghost".into(), message_id: 99, text: "x".into() })
        .expect("sessionless steer");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut deferred = false;
    while Instant::now() < deadline && !deferred {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SteerSettled { message_id: 99, deferred: true })) => {
                deferred = true
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(deferred, "the sessionless steer settles deferred");

    cmd_tx
        .send(Cmd::AuthSkipped { session_id: "".into() })
        .expect("auth skipped");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut skipped = false;
    while Instant::now() < deadline && !skipped {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ui(crate::events::UiEvent::SessionNotice { title, .. })) if title.contains("held prompt") => {
                skipped = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(skipped, "the held prompt is dropped visibly");

    // A non-auth session/new failure surfaces as a plain error — the loop
    // stays alive and the next prompt still gets through (issue #160).
    cmd_tx
        .send(Cmd::Prompt { session_id: "".into(), text: "second".into() })
        .expect("prompt on a dead backend");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut errored = false;
    while Instant::now() < deadline && !errored {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Error(message))) if message.contains("session/new") => {
                errored = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(errored, "the failed session/new reports an error");

    cmd_tx
        .send(Cmd::Prompt { session_id: "".into(), text: "third".into() })
        .expect("prompt after the failure");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bound = false;
    while Instant::now() < deadline && !bound {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionBound { .. })) => bound = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(bound, "the session bound");

    // Rebind failures: a non-auth error with a snapshot stays a failure;
    // an auth one re-arms sign-in naming the credential.
    cmd_tx
        .send(Cmd::NewSession { requester: Some("s1".into()), retry_auth: None })
        .expect("rebind non-auth");
    cmd_tx
        .send(Cmd::NewSession { requester: Some("s1".into()), retry_auth: None })
        .expect("rebind auth");
    let deadline = Instant::now() + Duration::from_secs(3);
    let (mut failed_plain, mut failed_auth) = (false, false);
    while Instant::now() < deadline && !(failed_plain && failed_auth) {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::BindFailedTo { message, .. })) if message.contains("backend exploded") => {
                failed_plain = true;
            }
            Ok(AppEvent::Ctl(CtlEvent::BindFailedTo { message, .. })) if message.contains("authentication required") => {
                failed_auth = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(failed_plain && failed_auth, "both rebind failures surface");

    // A requester-less refused bind reports a bare BindFailed — no
    // session-scoped snapshot exists to name (issue #160).
    cmd_tx
        .send(Cmd::NewSession { requester: None, retry_auth: None })
        .expect("anonymous rebind");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut anon_failed = false;
    while Instant::now() < deadline && !anon_failed {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::BindFailed { message })) if message.contains("authentication required") => {
                anon_failed = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(anon_failed, "the anonymous refusal reports BindFailed");

    // s1's stored connection is NeedsAuth now — a prompt for it queues
    // behind sign-in rather than hitting the wire (issue #160).
    cmd_tx
        .send(Cmd::Prompt { session_id: "s1".into(), text: "held".into() })
        .expect("prompt on a gated session");

    // authenticate through the advertised methods: a stored-connection
    // match names the owner, a miss stays ownerless, and a refused key
    // surfaces as a failed attempt (issue #160).
    for method_id in ["login", "gateway", "login"] {
        cmd_tx
            .send(Cmd::Authenticate { method_id: method_id.into(), values: Default::default() })
            .expect("authenticate");
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    let (mut signed_in, mut auth_failed) = (0, false);
    while Instant::now() < deadline && !(signed_in >= 2 && auth_failed) {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::TuiOpDone(message))) if message.contains("signed in") => {
                signed_in += 1;
            }
            Ok(AppEvent::Ctl(CtlEvent::TuiOpFailed(message))) if message.contains("authenticate:") => {
                auth_failed = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(signed_in, 2, "both key submissions sign in");
    assert!(auth_failed, "the refused key reports a failed attempt");

    // Named commands on a session that does not exist fail visibly.
    cmd_tx
        .send(Cmd::Prompt { session_id: "ghost".into(), text: "x".into() })
        .expect("prompt ghost");
    cmd_tx
        .send(Cmd::Steer { session_id: "ghost".into(), message_id: 1, text: "x".into() })
        .expect("steer ghost");
    cmd_tx
        .send(Cmd::SteerImages {
            session_id: "ghost".into(),
            message_id: 2,
            blocks: vec![crate::bus::PromptBlock::Text("x".into())],
        })
        .expect("steer images ghost");
    cmd_tx
        .send(Cmd::Interrupt { session_id: "ghost".into() })
        .expect("interrupt ghost");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut errors = 0;
    while Instant::now() < deadline && errors < 4 {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionError { .. } | CtlEvent::SessionOpFailed { .. })) => {
                errors += 1;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert_eq!(errors, 4, "every unknown-session command fails visibly");

    // The catalog answer carries the agent-advertised modes.
    cmd_tx
        .send(Cmd::FetchCatalog { session_id: "s1".into() })
        .expect("catalog with modes");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut modes_seen = false;
    while Instant::now() < deadline && !modes_seen {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SessionModes { modes, .. })) if !modes.is_empty() => {
                modes_seen = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(modes_seen, "catalog reports the session modes");

    // A steer the agent cannot answer settles visibly and does not stall
    // the session.
    cmd_tx
        .send(Cmd::Steer { session_id: "s1".into(), message_id: 3, text: "now".into() })
        .expect("steer");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut settled = false;
    while Instant::now() < deadline && !settled {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::SteerSettled { message_id: 3, .. })) => settled = true,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(settled, "the steer settled");

    // Static plugins are an agent extension: an agent without it reports
    // the failure instead of pretending an empty catalog.
    cmd_tx.send(Cmd::FetchStaticPlugins).expect("static plugins");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut plugins_failed = false;
    while Instant::now() < deadline && !plugins_failed {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::TuiOpFailed(message)))
                if message.contains("static plugins") =>
            {
                plugins_failed = true;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    assert!(plugins_failed, "static-plugin failure surfaces");

    let _ = prompt_rx.try_recv();
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

/// `parked_prompt` maps each sendable command to its held kind — the
/// image-carrying arms included (issue #160 coverage).
#[test]
fn parked_prompt_kinds_cover_images_and_unsendables() {
    assert!(matches!(
        parked_prompt(&Cmd::PromptImages {
            session_id: "s".into(),
            blocks: vec![crate::bus::PromptBlock::Text("x".into())],
        }),
        Some(ParkedPromptKind::Images(_))
    ));
    assert!(matches!(
        parked_prompt(&Cmd::SteerImages {
            session_id: "s".into(),
            message_id: 1,
            blocks: vec![crate::bus::PromptBlock::Text("x".into())],
        }),
        Some(ParkedPromptKind::Images(_))
    ));
    assert!(matches!(
        parked_prompt(&Cmd::Steer { session_id: "s".into(), message_id: 1, text: "x".into() }),
        Some(ParkedPromptKind::Text(_))
    ));
    assert!(parked_prompt(&Cmd::Shutdown).is_none());
}

/// `plugin_op_finish_event` covers the four settle shapes: primary
/// new-session action, a failed op result, a task-level join failure, and
/// the silent (cancelled / non-primary / empty) finishes (issue #160).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_op_finish_event_maps_each_completion_shape() {
    let id = tokio::spawn(async {}).id();
    let finish = |result: std::result::Result<Value, AcpError>| {
        Some(Ok((id, PluginOperationFinish { context: "ctx", result })))
    };
    assert!(matches!(
        plugin_op_finish_event(finish(Ok(json!({"action": "new-session"}))), true, false),
        Some(CtlEvent::NewSessionRequested)
    ));
    assert!(plugin_op_finish_event(finish(Ok(json!({"action": "new-session"}))), true, true).is_none());
    assert!(plugin_op_finish_event(finish(Ok(json!({"action": "new-session"}))), false, false).is_none());
    assert!(plugin_op_finish_event(finish(Ok(json!({}))), true, false).is_none());
    assert!(matches!(
        plugin_op_finish_event(finish(Err(AcpError::new(-32603, "op blew up"))), false, false),
        Some(CtlEvent::TuiOpFailed(message)) if message == "ctx failed: op blew up"
    ));
    assert!(plugin_op_finish_event(finish(Err(AcpError::new(-32603, "op blew up"))), false, true).is_none());
    assert!(plugin_op_finish_event(None, true, false).is_none());

    // A panicked task surfaces as a task-level failure, not a hang.
    let mut set = tokio::task::JoinSet::<PluginOperationFinish>::new();
    set.spawn(async { panic!("task panic"); });
    let joined = set.join_next_with_id().await;
    assert!(matches!(
        plugin_op_finish_event(joined, true, false),
        Some(CtlEvent::TuiOpFailed(message)) if message.contains("plugin operation task failed")
    ));
    let mut set = tokio::task::JoinSet::<PluginOperationFinish>::new();
    set.spawn(async { panic!("task panic"); });
    let joined = set.join_next_with_id().await;
    assert!(plugin_op_finish_event(joined, true, true).is_none());
}
/// `signed_in_notice` counts only the prompts the sign-in actually
/// released — zero retries is a plain `signed in` (issue #160).
#[test]
fn signed_in_notice_counts_retries() {
    assert_eq!(signed_in_notice(0), "signed in");
    assert_eq!(signed_in_notice(1), "signed in — retried 1 parked prompt");
    assert_eq!(signed_in_notice(3), "signed in — retried 3 parked prompts");
}
