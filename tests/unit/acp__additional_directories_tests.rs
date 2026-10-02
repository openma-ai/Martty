use super::*;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, Implementation, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, ResumeSessionRequest,
    ResumeSessionResponse, SessionAdditionalDirectoriesCapabilities, SessionCapabilities,
    SessionResumeCapabilities,
};
use std::time::Duration;

fn cfg_with(directories: Vec<String>) -> RuntimeConfig {
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
        additional_directories: directories,
    }
}

fn capabilities(additional: bool, resume: bool, load: bool) -> AgentCapabilities {
    let mut session = SessionCapabilities::new();
    if additional {
        session = session.additional_directories(SessionAdditionalDirectoriesCapabilities::new());
    }
    if resume {
        session = session.resume(SessionResumeCapabilities::new());
    }
    AgentCapabilities::new()
        .load_session(load)
        .session_capabilities(session)
}

async fn wait_caps(bus_rx: &std::sync::mpsc::Receiver<AppEvent>) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match bus_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(AppEvent::Ctl(CtlEvent::AgentCaps {
                additional_directories,
                ..
            })) => return additional_directories,
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(err) => panic!("{err}"),
        }
    }
    panic!("initialize did not report agent capabilities");
}

#[test]
fn capability_object_is_required() {
    assert!(additional_directories_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "additionalDirectories": {} } }
    })));
    assert!(!additional_directories_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "list": {} } }
    })));
    assert!(!additional_directories_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "additionalDirectories": null } }
    })));
    assert!(!additional_directories_supported(&json!({
        "agentCapabilities": { "sessionCapabilities": { "additionalDirectories": true } }
    })));
}

#[test]
fn the_field_is_omitted_unless_the_agent_advertised_it_and_dirs_exist() {
    let cwd = std::path::Path::new("/tmp/martty-workspace");
    let dirs = vec!["/tmp/martty-lib".into()];
    let omitted = serde_json::to_value(new_session_request(cwd, false, &dirs)).unwrap();
    assert!(omitted.get("additionalDirectories").is_none(), "{omitted}");
    let empty = serde_json::to_value(new_session_request(cwd, true, &[])).unwrap();
    assert!(empty.get("additionalDirectories").is_none(), "{empty}");
    let sent = serde_json::to_value(new_session_request(cwd, true, &dirs)).unwrap();
    assert_eq!(sent["additionalDirectories"], json!(["/tmp/martty-lib"]));
    let resumed = serde_json::to_value(resume_session_request(
        SessionId::new("s1"),
        cwd,
        true,
        &dirs,
    ))
    .unwrap();
    assert_eq!(resumed["additionalDirectories"], json!(["/tmp/martty-lib"]));
    let loaded = serde_json::to_value(load_session_request(
        SessionId::new("s1"),
        cwd,
        false,
        &dirs,
    ))
    .unwrap();
    assert!(loaded.get("additionalDirectories").is_none(), "{loaded}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_new_omits_additional_directories_without_the_capability() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let record_new = tx.clone();
    let record_load = tx;
    let agent = Agent.builder()
        .name("plain-acp")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(capabilities(false, false, true))
                        .agent_info(Implementation::new("plain-acp", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _cx| {
                let _ = record_new.send(serde_json::to_value(&request).unwrap());
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: LoadSessionRequest, responder, _cx| {
                let _ = record_load.send(serde_json::to_value(&request).unwrap());
                responder.respond(LoadSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = cfg_with(vec!["/tmp/martty-lib".into()]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
    assert!(!wait_caps(&bus_rx).await);
    let created = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("session/new")
        .expect("request");
    assert!(
        created.get("additionalDirectories").is_none(),
        "unsupported agents must not receive the field: {created}"
    );
    cmd_tx
        .send(Cmd::ResumeSession {
            session_id: "s1".into(),
        })
        .unwrap();
    let loaded = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("session/load")
        .expect("request");
    assert!(
        loaded.get("additionalDirectories").is_none(),
        "session/load must omit the field too: {loaded}"
    );
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_lifecycle_sends_additional_directories_when_advertised() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let record_new = tx.clone();
    let record_resume = tx.clone();
    let agent = Agent.builder()
        .name("multi-root")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(capabilities(true, true, true))
                        .agent_info(Implementation::new("multi-root", "0")),
                )
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _cx| {
                let _ = record_new.send(("new", serde_json::to_value(&request).unwrap()));
                responder.respond(NewSessionResponse::new(SessionId::new("s1")))
            },
            on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ResumeSessionRequest, responder, _cx| {
                let _ = record_resume.send(("resume", serde_json::to_value(&request).unwrap()));
                responder.respond(ResumeSessionResponse::new())
            },
            on_receive_request!(),
        );
    let cfg = cfg_with(vec!["/tmp/martty-lib".into(), "/tmp/martty-docs".into()]);
    let (bus_tx, bus_rx) = std::sync::mpsc::channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let client = tokio::spawn(async move { connect(agent, cfg, bus_tx, cmd_rx).await });
    assert!(wait_caps(&bus_rx).await);
    let (kind, created) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("session/new")
        .expect("request");
    assert_eq!(kind, "new");
    assert_eq!(
        created["additionalDirectories"],
        json!(["/tmp/martty-lib", "/tmp/martty-docs"])
    );
    cmd_tx
        .send(Cmd::ResumeSession {
            session_id: "older".into(),
        })
        .unwrap();
    let (kind, resumed) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("session/resume")
        .expect("request");
    assert_eq!(kind, "resume");
    assert_eq!(
        resumed["additionalDirectories"],
        json!(["/tmp/martty-lib", "/tmp/martty-docs"])
    );
    let _ = cmd_tx.send(Cmd::Shutdown);
    let _ = client.await;
}
