use super::*;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, InitializeRequest, InitializeResponse, NewSessionResponse, PromptResponse,
    SetSessionConfigOptionResponse, StopReason,
};

async fn wait_event(rx: &Receiver<AppEvent>, predicate: impl Fn(&AppEvent) -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            while let Ok(event) = rx.try_recv() {
                if predicate(&event) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("expected scheduler event");
}

/// Stall one control request while another session prompts, finishes, and
/// cancels. For config, also check same-session prompts cannot overtake it.
async fn check_slow_control(config: bool) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let count = Arc::new(AtomicUsize::new(0));
    let (blocked_tx, mut blocked_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release_tx, release_rx) = tokio::sync::watch::channel(false);
    let (cancel_tx, mut cancel_rx) = tokio::sync::mpsc::unbounded_channel();
    let (prompt_tx, mut prompt_rx) = tokio::sync::mpsc::unbounded_channel();
    let (prompt_release_tx, prompt_release_rx) = tokio::sync::watch::channel(false);
    let agent = Agent
        .builder()
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
            {
                let blocked = blocked_tx.clone();
                let release = release_rx.clone();
                async move |_req: NewSessionRequest, responder, cx| {
                    let index = count.fetch_add(1, Ordering::SeqCst) + 1;
                    let mut release = release.clone();
                    if index > 2 {
                        let _ = blocked.send(());
                        cx.spawn(async move {
                            release.wait_for(|ready| *ready).await.unwrap();
                            responder.respond(NewSessionResponse::new(SessionId::new(format!(
                                "s{index}"
                            ))))
                        })?;
                        Ok(())
                    } else {
                        responder
                            .respond(NewSessionResponse::new(SessionId::new(format!("s{index}"))))
                    }
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let blocked = blocked_tx;
                let release = release_rx;
                async move |_req: SetSessionConfigOptionRequest, responder, cx| {
                    let _ = blocked.send(());
                    let mut release = release.clone();
                    cx.spawn(async move {
                        release.wait_for(|ready| *ready).await.unwrap();
                        responder.respond(SetSessionConfigOptionResponse::new(vec![]))
                    })?;
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let _ = prompt_tx.send(req.session_id.to_string());
                let mut release = prompt_release_rx.clone();
                cx.spawn(async move {
                    release.wait_for(|ready| *ready).await.unwrap();
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                })?;
                Ok(())
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |req: CancelNotification, _cx| {
                let _ = cancel_tx.send(req.session_id.to_string());
                Ok(())
            },
            on_receive_notification!(),
        );
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: "/tmp".into(),
        provider: "test".into(),
        model: "test".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (bus, events) = std::sync::mpsc::channel();
    let (cmds, commands) = std::sync::mpsc::channel();
    let task = tokio::spawn(connect(agent, cfg, bus, commands));
    wait_event(&events, |event| matches!(event, AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }) if session_id == "s1")).await;
    cmds.send(Cmd::NewSession).unwrap();
    wait_event(&events, |event| matches!(event, AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }) if session_id == "s2")).await;
    cmds.send(if config {
        Cmd::SetConfigOption {
            session_id: "s2".into(),
            config_id: "model".into(),
            value: "next".into(),
        }
    } else {
        Cmd::NewSession
    })
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), blocked_rx.recv())
        .await
        .unwrap()
        .unwrap();
    if config {
        cmds.send(Cmd::Prompt {
            session_id: "s2".into(),
            text: "after config".into(),
        })
        .unwrap();
    }
    cmds.send(Cmd::Prompt {
        session_id: "s1".into(),
        text: "other session".into(),
    })
    .unwrap();
    let started = tokio::time::timeout(Duration::from_secs(3), prompt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        started, "s1",
        "other sessions progress, same-session prompts wait for config"
    );
    cmds.send(Cmd::Interrupt {
        session_id: "s1".into(),
    })
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), cancel_rx.recv())
            .await
            .unwrap()
            .unwrap(),
        "s1"
    );
    prompt_release_tx.send(true).unwrap();
    wait_event(&events, |event| matches!(event, AppEvent::Ui(crate::events::UiEvent::TurnEnd { session, .. }) if session == "s1")).await;
    if config {
        assert!(
            prompt_rx.try_recv().is_err(),
            "s2 still waits for its config response"
        );
        release_tx.send(true).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), prompt_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            "s2"
        );
    }
    // Shutdown must also work with the setup request still stalled.
    cmds.send(Cmd::Shutdown).unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_session_creation_does_not_block_other_session_prompts_or_cancel() {
    check_slow_control(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_config_only_blocks_following_prompts_on_its_own_session() {
    check_slow_control(true).await;
}

#[tokio::test(start_paused = true)]
async fn prompts_and_steers_outlive_the_control_request_deadline() {
    let (release, release_rx) = tokio::sync::watch::channel(false);
    let (started, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent.builder().on_receive_request(
        async move |_req: PromptRequest, responder, cx| {
            let _ = started.send(());
            let mut release_rx = release_rx.clone();
            cx.spawn(async move {
                release_rx.wait_for(|ready| *ready).await.unwrap();
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            })?;
            Ok(())
        },
        on_receive_request!(),
    );
    Client
        .builder()
        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
            let (bus, _) = std::sync::mpsc::channel();
            let (done, mut done_rx) = tokio::sync::mpsc::unbounded_channel();
            let (steer_done, mut steer_rx) = tokio::sync::mpsc::unbounded_channel();
            let task = spawn_session_prompt(
                cx.clone(),
                bus,
                SessionId::new("s"),
                vec!["work".into()],
                ParkedPromptKind::Text("work".into()),
                1,
                done,
            );
            spawn_steer_prompt(
                cx,
                SessionId::new("s"),
                vec!["follow up".into()],
                2,
                steer_done,
            );
            started_rx.recv().await.unwrap();
            started_rx.recv().await.unwrap();
            tokio::time::advance(REQUEST_DEADLINE + Duration::from_secs(1)).await;
            for _ in 0..5 {
                tokio::task::yield_now().await;
            }
            assert!(
                matches!(
                    done_rx.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ),
                "healthy prompt timed out"
            );
            assert!(
                matches!(
                    steer_rx.try_recv(),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty)
                ),
                "healthy steer timed out"
            );
            release.send(true).unwrap();
            assert!(done_rx.recv().await.unwrap().result.is_ok());
            assert!(steer_rx.recv().await.unwrap().result.is_ok());
            task.await.unwrap();
            Ok(())
        })
        .await
        .unwrap();
}
