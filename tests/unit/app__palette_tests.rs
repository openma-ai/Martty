use super::*;
use crate::theme::DEEPSEEK_450;
use ratatui::style::Color;
use serde_json::json;
use std::sync::mpsc::Receiver;

fn fresh_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dsh-tui-palette-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

fn test_app() -> (App, Controller, Receiver<AppEvent>) {
    let cfg = RuntimeConfig {
        bin: "demo".into(),
        cordis: "demo".into(),
        workspace: "/tmp".into(),
        session_root: fresh_root(),
        provider: "deepseek-official".into(),
        model: "deepseek-v4-flash".into(),
        max_tokens: None,
        base_url: None,
        api_key: None,
    };
    let (tx, rx) = std::sync::mpsc::channel::<AppEvent>();
    let ctl = Controller::start(cfg.clone(), true, None, tx.clone());
    let app = App::new(Theme::dark(), cfg, "dsh-test".into(), true, false, tx);
    (app, ctl, rx)
}

fn ember_params(activate: bool) -> serde_json::Value {
    let palette: serde_json::Value =
        serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap();
    json!({"protocol": 0, "palette": palette, "activate": activate})
}

fn gallery_params(id: &str, activate: bool) -> serde_json::Value {
    let fixture = match id {
        "ayu" => include_str!("../../docs/fixtures/ayu.v0.json"),
        "catppuccin" => include_str!("../../docs/fixtures/catppuccin.v0.json"),
        "kanagawa" => include_str!("../../docs/fixtures/kanagawa.v0.json"),
        "everforest" => include_str!("../../docs/fixtures/everforest.v0.json"),
        "iceberg" => include_str!("../../docs/fixtures/iceberg.v0.json"),
        "solarized" => include_str!("../../docs/fixtures/solarized.v0.json"),
        _ => panic!("unknown gallery fixture {id}"),
    };
    let palette: serde_json::Value = serde_json::from_str(fixture).unwrap();
    json!({"protocol": 0, "palette": palette, "activate": activate})
}

#[test]
fn starts_on_default_pack() {
    let (app, _ctl, _rx) = test_app();
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(app.theme.brand, DEEPSEEK_450);
    assert!(app.palettes.iter().any(|p| p.id == "default"));
}

#[test]
fn tui_palette_rpc_activates_ember() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(true),
        },
        &ctl,
    );
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            crossterm::event::KeyModifiers::CONTROL,
        ))),
        &ctl,
    );
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.mode, crate::theme::Mode::Light);
    assert_eq!(app.theme.brand, Color::Rgb(217, 106, 30));
    let tip = app.tip.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(
        tip.contains("ember") && tip.contains("light"),
        "tip should name the pack, got {tip:?}"
    );
}

#[test]
fn ctrl_t_toggles_mode_while_theme_picker_stays_open() {
    let (mut app, ctl, _rx) = test_app();
    app.run_slash("theme", "", &ctl);
    let picker = app.picker.as_ref().expect("theme picker opens");
    assert!(
        picker.title.contains("ctrl+t"),
        "theme picker keeps the dark/light shortcut visible"
    );

    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            KeyModifiers::CONTROL,
        ))),
        &ctl,
    );

    assert_eq!(app.theme.mode, crate::theme::Mode::Light);
    assert!(
        app.picker.is_some(),
        "toggling mode must not close the picker"
    );
}

#[test]
fn gallery_palette_rpc_activates_everforest_and_toggles_modes() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: gallery_params("everforest", true),
        },
        &ctl,
    );
    assert_eq!(app.active_palette_id, "everforest");
    assert_eq!(app.theme.brand, Color::Rgb(127, 187, 179)); // #7FBBB3 Everforest dark blue
    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            crossterm::event::KeyModifiers::CONTROL,
        ))),
        &ctl,
    );
    assert_eq!(app.active_palette_id, "everforest");
    assert_eq!(app.theme.mode, crate::theme::Mode::Light);
    assert_eq!(app.theme.brand, Color::Rgb(58, 148, 197)); // #3A94C5 Everforest light blue
    let tip = app.tip.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(
        tip.contains("everforest") && tip.contains("light"),
        "tip should name the pack, got {tip:?}"
    );
}

#[test]
fn slash_theme_switches_between_gallery_packs() {
    let (mut app, ctl, _rx) = test_app();
    for id in [
        "ayu",
        "catppuccin",
        "kanagawa",
        "everforest",
        "iceberg",
        "solarized",
    ] {
        app.handle(
            AppEvent::Rpc {
                method: crate::cordis::THEME_UPDATE.into(),
                params: gallery_params(id, false),
            },
            &ctl,
        );
    }
    assert_eq!(app.active_palette_id, "default");
    app.run_slash("theme", "solarized", &ctl);
    assert_eq!(app.active_palette_id, "solarized");
    // Solarized keeps the same blue in both modes.
    assert_eq!(app.theme.brand, Color::Rgb(38, 139, 210)); // #268BD2
}

#[test]
fn tui_palette_without_activate_registers_but_does_not_switch() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    assert!(app.palettes.iter().any(|p| p.id == "ember"));
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(app.theme.brand, DEEPSEEK_450);
}

#[test]
fn tui_palette_remove_retracts_the_native_catalog_entry() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    assert!(app.palettes.iter().any(|palette| palette.id == "ember"));

    app.handle(
        AppEvent::Rpc {
            method: "_dsh/cordis/tui/theme/remove".into(),
            params: serde_json::json!({ "protocol": 0, "id": "ember" }),
        },
        &ctl,
    );

    assert!(!app.palettes.iter().any(|palette| palette.id == "ember"));
}

#[test]
fn slash_theme_id_covers_mounted_plugin_pack() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(true),
        },
        &ctl,
    );
    app.run_slash("theme", "default", &ctl);
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(app.theme.brand, DEEPSEEK_450);
    app.run_slash("theme", "ember", &ctl);
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    app.run_slash("theme", "nope", &ctl);
    assert_eq!(app.active_palette_id, "ember");
    let tip = app.tip.as_ref().map(|(t, _)| t.as_str()).unwrap_or("");
    assert!(
        tip.contains("nope")
            || app
                .transcript
                .cells
                .iter()
                .any(|c| { format!("{:?}", c.kind).contains("nope") }),
        "unknown id should notice/tip, got tip={tip:?}"
    );
}

#[test]
fn slash_theme_selection_notifies_the_client_theme_registry() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    let (client_ctl, commands) = crate::controller::tests::test_controller();

    app.run_slash("theme", "ember", &client_ctl);

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginThemeSelected { agent_id, id })
            if agent_id == app.session_id && id == "ember"
    ));
}

#[test]
fn stopped_dynamic_theme_stays_selectable_without_painting_until_restored() {
    let (mut app, ctl, _rx) = test_app();
    let mut loaded = ember_params(true);
    loaded["owner"] = json!({ "pluginId": "night-lime-1" });
    loaded["loaded"] = json!(true);
    loaded["source"] = json!("dynamic");
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: loaded,
        },
        &ctl,
    );
    let mut stopped = ember_params(false);
    stopped["owner"] = json!({ "pluginId": "night-lime-1" });
    stopped["loaded"] = json!(false);
    stopped["source"] = json!("dynamic");
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: stopped,
        },
        &ctl,
    );

    assert_eq!(app.active_palette_id, "default");
    assert!(app.palettes.iter().any(|palette| palette.id == "ember"));
    app.open_theme_picker();
    let picker = app.picker.as_ref().expect("theme picker");
    assert_eq!(picker.items[0].meta, "static · active");
    assert_eq!(picker.items[1].meta, "dynamic · stopped");
    let (client_ctl, commands) = crate::controller::tests::test_controller();
    app.run_slash("theme", "ember", &client_ctl);
    assert_eq!(app.active_palette_id, "default");
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginThemeSelected { agent_id, id })
            if agent_id == app.session_id && id == "ember"
    ));
}

#[test]
fn slash_theme_options_match_the_picker_catalog() {
    let (mut app, ctl, _rx) = test_app();
    let mut loaded = ember_params(false);
    loaded["owner"] = json!({ "pluginId": "night-lime-1" });
    loaded["loaded"] = json!(true);
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: loaded,
        },
        &ctl,
    );
    let mut stopped = ember_params(false);
    stopped["owner"] = json!({ "pluginId": "night-lime-1" });
    stopped["loaded"] = json!(false);
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: stopped,
        },
        &ctl,
    );

    app.input.set("/theme ".into());
    let completions = app
        .slash_matches()
        .into_iter()
        .filter_map(|entry| entry.completion)
        .collect::<Vec<_>>();

    assert_eq!(completions, ["/theme default", "/theme ember"]);
}

#[test]
fn theme_picker_can_leave_and_return_to_a_dynamic_plugin_pack() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(true),
        },
        &ctl,
    );

    app.run_slash("theme", "", &ctl);
    let picker = app
        .picker
        .as_ref()
        .expect("/theme opens the palette picker");
    assert_eq!(
        picker
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["default", "ember"]
    );
    assert_eq!(picker.sel, 1, "the active dynamic pack is preselected");

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.active_palette_id, "default");

    app.run_slash("theme", "", &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &ctl);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &ctl);
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
}

#[test]
fn slash_theme_usage_mentions_pack_ids() {
    let theme = SLASH_COMMANDS.iter().find(|c| c.name == "theme").unwrap();
    assert!(
        theme.usage.contains("id"),
        "usage should mention pack ids, got {}",
        theme.usage
    );
}

#[test]
fn duplicate_palette_id_replaces_colors() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(true),
        },
        &ctl,
    );
    let mut palette: serde_json::Value =
        serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap();
    palette["dark"]["brand"] = json!("#010203");
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: json!({"protocol": 0, "palette": palette, "activate": true}),
        },
        &ctl,
    );
    assert_eq!(app.palettes.iter().filter(|p| p.id == "ember").count(), 1);
    assert_eq!(app.theme.brand, Color::Rgb(1, 2, 3));
}

#[test]
fn invalid_palette_keeps_previous_theme() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: json!({"protocol": 0, "palette": {"id": "x"}, "activate": true}),
        },
        &ctl,
    );
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(app.theme.brand, DEEPSEEK_450);
}
