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
