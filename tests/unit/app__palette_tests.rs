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
    let app = App::new(Some(Theme::dark()), cfg, "dsh-test".into(), true, false, tx);
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
        "one" => include_str!("../../docs/fixtures/one.v0.json"),
        "tomorrow" => include_str!("../../docs/fixtures/tomorrow.v0.json"),
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
        "one",
        "tomorrow",
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

    assert_eq!(
        completions,
        ["/theme toggle", "/theme default", "/theme ember"]
    );
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

    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))), &ctl);
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.active_palette_id, "default");

    app.run_slash("theme", "", &ctl);
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))), &ctl);
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

#[test]
fn theme_dialog_arrows_preview_and_only_enter_commits() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: gallery_params("ayu", false),
        },
        &ctl,
    );
    app.run_slash("theme", "", &ctl);
    let picker = app.picker.as_ref().expect("theme picker opens");
    assert_eq!(
        picker
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["default", "ember", "ayu"]
    );
    assert_eq!(app.active_palette_id, "default");

    // One ↓ lands on ember: the painter previews ember immediately, but the
    // committed theme is still "default" — arrows never confirm.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60)); // ember dark brand
    assert_eq!(
        app.active_palette_id, "default",
        "arrows only preview; the committed theme must stay default"
    );
    assert!(
        app.picker.is_some(),
        "preview must keep the dialog open for further browsing"
    );

    // ↓ again → ayu preview, still uncommitted.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.active_palette_id, "default");
    assert!(app.picker.is_some());

    // Home jumps back onto the committed row → the preview is gone.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, DEEPSEEK_450);
    assert_eq!(app.active_palette_id, "default");

    // ↓ to ember, then Enter confirms: dialog closes and ember commits.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert_eq!(app.active_palette_id, "default", "still only previewed");
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))), &ctl);
    assert!(app.picker.is_none());
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
}

#[test]
fn theme_dialog_esc_reverts_the_preview_to_the_committed_theme() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    app.run_slash("theme", "", &ctl);
    assert_eq!(app.theme.brand, DEEPSEEK_450);

    // Preview ember with ↓ and with the wheel…
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, DEEPSEEK_450);
    app.handle(
        AppEvent::Term(Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 40,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::NONE,
        })),
        &ctl,
    );
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert!(app.picker.is_some(), "wheel preview keeps the dialog open");

    // …Esc closes without confirming: the committed theme comes back.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))), &ctl);
    assert!(app.picker.is_none());
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(
        app.theme.brand, DEEPSEEK_450,
        "Esc must revert the preview — arrows never confirm"
    );
}

#[test]
fn slash_theme_popup_previews_and_reverts_without_enter() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    app.input.set("/theme ".into());
    app.snap_slash_sel();
    assert!(
        app.slash_completion_open(),
        "the /theme candidate popup must be open"
    );
    assert_eq!(app.slash_sel, 1, "default row is the current theme");
    assert_eq!(app.active_palette_id, "default");

    // ↓ to ember → preview only; draft and popup stay, theme uncommitted.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert_eq!(app.active_palette_id, "default");
    assert!(
        app.slash_completion_open(),
        "preview must not consume the draft"
    );

    // ↓ wraps to the dark/light toggle row — no theme highlighted, so the
    // committed theme shows again.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, DEEPSEEK_450);
    assert_eq!(app.active_palette_id, "default");

    // ↓↓ back onto ember, then Esc dismisses the popup and reverts too.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))), &ctl);
    assert!(!app.slash_completion_open());
    assert_eq!(
        app.theme.brand, DEEPSEEK_450,
        "Esc must revert the popup preview"
    );
}

#[test]
fn slash_theme_popup_enter_commits_the_previewed_palette() {
    let (mut app, ctl, _rx) = test_app();
    app.handle(
        AppEvent::Rpc {
            method: crate::cordis::THEME_UPDATE.into(),
            params: ember_params(false),
        },
        &ctl,
    );
    app.input.set("/theme ".into());
    app.snap_slash_sel();
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert_eq!(app.active_palette_id, "default");

    // Enter on the highlighted ember row is the confirmation.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))), &ctl);
    assert_eq!(app.active_palette_id, "ember");
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert!(
        app.input.is_empty(),
        "Enter on a candidate runs the command and clears the draft"
    );
}

#[test]
fn dialog_stopped_pack_preview_is_transient_and_enter_holds_it_while_loading() {
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
    let (client_ctl, commands) = crate::controller::tests::test_controller();
    app.run_slash("theme", "", &client_ctl);

    // The stopped pack's stored token map previews while the Plugin stays
    // stopped — arrows need no client round trip and confirm nothing.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &client_ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    assert_eq!(app.active_palette_id, "default");
    assert!(
        commands
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err(),
        "preview alone must not ask the client Theme registry"
    );

    // Esc without Enter reverts the preview.
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))), &client_ctl);
    assert!(app.picker.is_none());
    assert_eq!(app.theme.brand, DEEPSEEK_450);

    // Enter confirms: the client registry is asked, and the previewed
    // colors stay on screen while the Plugin loads (no flash to default).
    app.run_slash("theme", "", &client_ctl);
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))), &client_ctl);
    assert_eq!(app.theme.brand, Color::Rgb(247, 140, 60));
    app.handle(AppEvent::Term(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))), &client_ctl);
    assert!(app.picker.is_none());
    assert_eq!(app.active_palette_id, "default");
    assert_eq!(
        app.theme.brand, Color::Rgb(247, 140, 60),
        "the confirmed pack must not flash back while its Plugin loads"
    );
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_secs(1)),
        Ok(Cmd::PluginThemeSelected { agent_id, id })
            if agent_id == app.session_id && id == "ember"
    ));
}

#[test]
fn theme_mode_persists_across_restarts_unless_cli_overrides() {
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
    let (tx, _rx) = std::sync::mpsc::channel::<AppEvent>();
    let (ctl, _commands) = crate::controller::tests::test_controller();

    // Explicit CLI light wins at startup; ctrl+t flips to dark and persists.
    let mut app = App::new(
        Some(Theme::light()),
        cfg.clone(),
        "s1".into(),
        true,
        false,
        tx.clone(),
    );
    assert_eq!(app.theme.mode, crate::theme::Mode::Light);
    app.handle(
        AppEvent::Term(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            crossterm::event::KeyModifiers::CONTROL,
        ))),
        &ctl,
    );
    assert_eq!(app.theme.mode, crate::theme::Mode::Dark);
    let path = crate::runtime::settings_path(&cfg.session_root);
    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("settings.json written on toggle"),
    )
    .expect("settings.json is valid JSON");
    assert_eq!(saved["themeMode"], "dark", "{saved}");

    // Restart with no CLI flag → the persisted dark mode comes back.
    let restarted = App::new(None, cfg.clone(), "s2".into(), true, false, tx.clone());
    assert_eq!(restarted.theme.mode, crate::theme::Mode::Dark);
    assert_eq!(restarted.theme.brand, Theme::dark().brand);

    // A restart with an explicit --theme still overrides persistence.
    let cli_light = App::new(Some(Theme::light()), cfg, "s3".into(), true, false, tx);
    assert_eq!(cli_light.theme.mode, crate::theme::Mode::Light);
}
