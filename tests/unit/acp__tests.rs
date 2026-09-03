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

    apply_prompt_finish(finish, &mut parked, &tx, &[], None);

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
        result: Err(AcpError::new(-32603, "boom")),
        payload: ParkedPromptKind::Text("hello".into()),
        gen: 1,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut parked = VecDeque::new();

    apply_prompt_finish(finish, &mut parked, &tx, &[], None);

    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, kind }))
            if session == "s" && kind == "error"
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::Ctl(CtlEvent::SessionError { session_id, .. }))
            if session_id == "s"
    ));
}

#[test]
fn initialize_advertises_backchat_auth_caps() {
    let value = serde_json::to_value(initialize_request()).expect("serialize");
    let caps = &value["clientCapabilities"];
    assert_eq!(caps["_meta"]["terminal-auth"], true);
    assert_eq!(caps["_meta"]["terminal_output"], true);
    assert_eq!(caps["_meta"]["subagent-transcript"], true);
    assert_eq!(caps["_meta"]["dsh"]["cordis"]["protocol"], 0);
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
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse,
    };
    use std::time::Duration;

    let (extension_tx, mut extension_rx) = tokio::sync::mpsc::unbounded_channel();
    let request_tx = extension_tx.clone();
    let agent = Agent
        .builder()
        .name("plugin-command-mock")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                let mut meta = serde_json::Map::new();
                meta.insert(
                    "dsh".into(),
                    json!({ "cordis": { "protocol": crate::cordis::PROTOCOL } }),
                );
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new().meta(meta))
                        .agent_info(Implementation::new("plugin-command-mock", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
                if message.contains("does not advertise _dsh/cordis")
        )),
        "the client should report that the Cordis capability is unavailable",
    );

    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_compositor_catalog_does_not_require_agent_cordis_capability() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, InitializeResponse, NewSessionResponse,
    };
    use std::time::Duration;

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
            async move |_req: NewSessionRequest, responder, cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))?;
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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn form_auth_stays_configured_when_the_startup_session_succeeds() {
    use agent_client_protocol::schema::v1::{
        AgentCapabilities, AuthMethod, AuthMethodAgent, InitializeResponse, NewSessionResponse,
    };
    use std::time::{Duration, Instant};

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
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: "/tmp".into(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut auth_status = None;
    let mut opened_auth = false;
    while Instant::now() < deadline && auth_status.is_none() {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::Auth(snapshot))) => {
                auth_status = Some(snapshot.status);
            }
            Ok(AppEvent::Ctl(CtlEvent::OpenAuth)) => opened_auth = true,
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
    assert!(
        !opened_auth,
        "initialize alone must not open sign-in for possibly persisted credentials"
    );
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession).expect("explicit new session");

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession).expect("explicit new session");

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
    cmd_tx.send(Cmd::NewSession).expect("explicit new session");

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
    assert_eq!(catalog, Some(vec!["standard".into(), "creator".into()]));
    assert!(prompt_rx.try_recv().is_err(), "session/new is not a turn");

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
                            .respond_with_error(AcpError::new(-32000, "authentication required"))
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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

    // Startup binds s1; /new binds s2 and becomes the fallback session.
    cmd_tx.send(Cmd::NewSession).expect("new session");
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
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });

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
