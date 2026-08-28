use super::*;
use serde_json::json;

#[test]
fn parse_catalog_models_and_presets() {
    let value = json!({
        "models": [
            {"provider": "deepseek-official", "id": "m1", "name": "M One", "vision": true},
            {"provider": "deepseek-official", "name": "no id → skipped"},
        ],
        "presets": [
            {"id": "standard", "name": "Standard mode", "description": "full agent"},
            {"id": "minimal"},
            {"id": "custom", "broken": true},
            {"name": "no id → skipped"},
        ],
    });
    let (models, presets) = parse_catalog(&value);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "m1");
    assert!(models[0].vision);

    assert_eq!(presets.len(), 3);
    assert_eq!(presets[0].id, "standard");
    assert_eq!(presets[0].name, "Standard mode");
    assert_eq!(presets[0].description, "full agent");
    assert!(!presets[0].broken);
    // name falls back to the id; missing description is empty.
    assert_eq!(presets[1].name, "minimal");
    assert_eq!(presets[1].description, "");
    assert!(presets[2].broken);
}

#[test]
fn parse_catalog_without_presets_key() {
    let (models, presets) = parse_catalog(&json!({"models": []}));
    assert!(models.is_empty());
    assert!(presets.is_empty());
}

#[test]
fn parse_skills_reads_names_and_skips_nameless() {
    let skills = parse_skills(&json!({
        "skills": [
            {"name": "commit-helper", "description": "draft a commit"},
            {"name": "bare"},
            {"description": "no name → skipped"},
        ]
    }));
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0].name, "commit-helper");
    assert_eq!(skills[0].description, "draft a commit");
    assert_eq!(skills[1].name, "bare");
    assert_eq!(skills[1].description, "");
    assert!(parse_skills(&json!({})).is_empty());
}

#[test]
fn stock_presets_cover_the_four_web_ui_modes() {
    let presets = stock_presets();
    let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["standard", "code", "minimal", "cordis"]);
}

use crate::bus::Cmd;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

#[cfg(test)]
pub(crate) fn test_controller() -> (Controller, Receiver<Cmd>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    (
        Controller {
            cmd_tx,
            runtime: Arc::new(Mutex::new(None)),
            interrupted: Arc::new(AtomicBool::new(false)),
            demo: true,
            attached: false,
            acp: false,
        },
        cmd_rx,
    )
}
#[cfg(test)]
pub(crate) fn test_interruptible_controller() -> (Controller, Receiver<Cmd>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    (
        Controller {
            cmd_tx,
            runtime: Arc::new(Mutex::new(None)),
            interrupted: Arc::new(AtomicBool::new(false)),
            demo: false,
            attached: true,
            acp: true,
        },
        cmd_rx,
    )
}

fn image_block() -> crate::bus::PromptBlock {
    crate::bus::PromptBlock::Image(crate::bus::ImagePart {
        data: "AA==".into(),
        media_type: "image/png".into(),
        name: "x.png".into(),
        path: "clipboard".into(),
    })
}

fn loop_cfg() -> RuntimeConfig {
    RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: std::env::temp_dir()
            .join(format!("dsh-tui-ctl-{}", std::process::id()))
            .to_string_lossy()
            .into_owned(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    }
}

fn collect_events(rx: &Receiver<AppEvent>, n: usize) -> Vec<AppEvent> {
    let mut out = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while out.len() < n && std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ev) => out.push(ev),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    out
}

#[test]
fn legacy_image_prompt_fails_with_error_so_the_app_leaves_starting() {
    // Regression: TuiOpFailed only pushes a notice — a fresh image prompt
    // that never launches must be an Error so the app clears prompt_pending
    // and drains its queue instead of wedging in Starting.
    let (bus, rx) = mpsc::channel::<AppEvent>();
    let ctl = Controller::start(loop_cfg(), false, None, bus);
    ctl.send(Cmd::PromptImages {
        session_id: "s1".into(),
        blocks: vec![image_block()],
    });
    let events = collect_events(&rx, 1);
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            AppEvent::Ctl(CtlEvent::Error(e)) if e.contains("unavailable")
        )),
        "expected CtlEvent::Error among {} event(s)",
        events.len()
    );
}

#[test]
fn legacy_image_steer_warns_and_settles_the_steer_bookkeeping() {
    let (bus, rx) = mpsc::channel::<AppEvent>();
    let ctl = Controller::start(loop_cfg(), false, None, bus);
    ctl.send(Cmd::SteerImages {
        session_id: "s1".into(),
        message_id: 7,
        blocks: vec![image_block()],
    });
    let events = collect_events(&rx, 2);
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            AppEvent::Ctl(CtlEvent::TuiOpFailed(e)) if e.contains("unavailable")
        )),
        "expected TuiOpFailed among {} event(s)",
        events.len()
    );
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            AppEvent::Ctl(CtlEvent::SteerSettled {
                message_id: 7,
                deferred: false
            })
        )),
        "expected SteerSettled among {} event(s)",
        events.len()
    );
}
