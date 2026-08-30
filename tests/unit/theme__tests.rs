use super::*;

/// Accent invariant: the brand accent is the DeepSeek blue — gray body,
/// blue accents (主体灰色，蓝色点缀).
#[test]
fn brand_is_deepseek_blue() {
    assert_eq!(Theme::dark().brand, DEEPSEEK_450);
    assert_eq!(Theme::light().brand, DEEPSEEK_500);
}

/// Neutral surfaces stay grayscale; the reserved accent vocabulary
/// (brand blue · gray-blue hint · green ok · amber warn · red err) is
/// deliberately colored — minimal, not monotone.
#[test]
fn neutrals_stay_gray_and_accents_stay_colored() {
    fn is_gray(c: Color) -> bool {
        match c {
            Color::Rgb(r, g, b) => {
                let (lo, hi) = (r.min(g).min(b), r.max(g).max(b));
                hi - lo <= 20 // the neutral-bluish scale has a slight cool tint
            }
            _ => false,
        }
    }
    for t in [Theme::dark(), Theme::light()] {
        for c in [
            t.bg,
            t.surface,
            t.panel,
            t.fg,
            t.fg_secondary,
            t.fg_tertiary,
            t.caption,
            t.bubble_bg,
            t.bubble_fg,
            t.border,
            t.code_bg,
            t.chip_bg,
        ] {
            assert!(is_gray(c), "expected grayscale, got {c:?}");
        }
        for c in [t.brand, t.brand_soft, t.hint, t.ok, t.warn, t.err] {
            assert!(!is_gray(c), "accents must be colored, got {c:?}");
        }
    }
}

fn ember_json() -> serde_json::Value {
    serde_json::from_str(include_str!("../../docs/fixtures/demo-skin.v0.json")).unwrap()
}

fn ember_notification(activate: bool) -> serde_json::Value {
    serde_json::json!({
        "protocol": 0,
        "palette": ember_json(),
        "activate": activate,
    })
}

#[test]
fn demo_skin_fixture_parses_both_modes() {
    let pack = PalettePack::from_json(&ember_json()).expect("ember fixture");
    assert_eq!(pack.id, "ember");
    assert_eq!(pack.label, "Ember");
    let dark = pack.theme(Mode::Dark);
    let light = pack.theme(Mode::Light);
    assert_eq!(dark.mode, Mode::Dark);
    assert_eq!(light.mode, Mode::Light);
    assert_eq!(dark.brand, Color::Rgb(247, 140, 60)); // #F78C3C
    assert_eq!(light.brand, Color::Rgb(217, 106, 30)); // #D96A1E
    assert_eq!(dark.bg, Color::Rgb(22, 16, 14)); // #16100E
    assert_eq!(light.bg, Color::Rgb(255, 246, 238)); // #FFF6EE
}

fn fixture_json(name: &str) -> serde_json::Value {
    match name {
        "ayu" => include_str!("../../docs/fixtures/ayu.v0.json"),
        "catppuccin" => include_str!("../../docs/fixtures/catppuccin.v0.json"),
        "kanagawa" => include_str!("../../docs/fixtures/kanagawa.v0.json"),
        "everforest" => include_str!("../../docs/fixtures/everforest.v0.json"),
        "iceberg" => include_str!("../../docs/fixtures/iceberg.v0.json"),
        "solarized" => include_str!("../../docs/fixtures/solarized.v0.json"),
        "one" => include_str!("../../docs/fixtures/one.v0.json"),
        "tomorrow" => include_str!("../../docs/fixtures/tomorrow.v0.json"),
        _ => panic!("unknown fixture {name}"),
    }
    .parse::<serde_json::Value>()
    .expect("fixture json")
}

#[test]
fn gallery_fixtures_parse_both_modes() {
    for (name, label, dark_brand, light_brand, dark_bg, light_bg) in [
        (
            "ayu",
            "Ayu",
            Color::Rgb(83, 189, 250),  // #53BDFA Ayu dark blue
            Color::Rgb(49, 153, 225),  // #3199E1 Ayu Light blue
            Color::Rgb(11, 14, 20),    // #0B0E14
            Color::Rgb(248, 249, 250), // #F8F9FA
        ),
        (
            "catppuccin",
            "Catppuccin",
            Color::Rgb(137, 180, 250), // #89B4FA Catppuccin Mocha blue
            Color::Rgb(30, 102, 245),  // #1E66F5 Catppuccin Latte blue
            Color::Rgb(30, 30, 46),    // #1E1E2E
            Color::Rgb(239, 241, 245), // #EFF1F5
        ),
        (
            "kanagawa",
            "Kanagawa",
            Color::Rgb(126, 156, 216), // #7E9CD8 Kanagawa Wave blue
            Color::Rgb(77, 105, 155),  // #4D699B Kanagawa Lotus blue
            Color::Rgb(31, 31, 40),    // #1F1F28
            Color::Rgb(242, 236, 188), // #F2ECBC
        ),
        (
            "everforest",
            "Everforest",
            Color::Rgb(127, 187, 179), // #7FBBB3
            Color::Rgb(58, 148, 197),  // #3A94C5
            Color::Rgb(45, 53, 59),    // #2D353B
            Color::Rgb(253, 246, 227), // #FDF6E3
        ),
        (
            "iceberg",
            "Iceberg",
            Color::Rgb(132, 160, 198), // #84A0C6
            Color::Rgb(45, 83, 158),   // #2D539E
            Color::Rgb(22, 24, 33),    // #161821
            Color::Rgb(232, 233, 236), // #E8E9EC
        ),
        (
            "solarized",
            "Solarized",
            Color::Rgb(38, 139, 210),  // #268BD2 (both modes)
            Color::Rgb(38, 139, 210),  // #268BD2
            Color::Rgb(0, 43, 54),     // #002B36
            Color::Rgb(253, 246, 227), // #FDF6E3
        ),
        (
            "one",
            "One",
            Color::Rgb(97, 175, 239),  // #61AFEF One Dark blue
            Color::Rgb(47, 90, 243),   // #2F5AF3 One Light blue
            Color::Rgb(40, 44, 52),    // #282C34
            Color::Rgb(248, 248, 248), // #F8F8F8
        ),
        (
            "tomorrow",
            "Tomorrow",
            Color::Rgb(122, 166, 218), // #7AA6DA Tomorrow Night Bright blue
            Color::Rgb(66, 113, 174),  // #4271AE Tomorrow blue
            Color::Rgb(0, 0, 0),       // #000000
            Color::Rgb(255, 255, 255), // #FFFFFF
        ),
    ] {
        let pack = PalettePack::from_json(&fixture_json(name)).expect("gallery fixture");
        assert_eq!(pack.id, name);
        assert_eq!(pack.label, label);
        let dark = pack.theme(Mode::Dark);
        let light = pack.theme(Mode::Light);
        assert_eq!(dark.mode, Mode::Dark);
        assert_eq!(light.mode, Mode::Light);
        assert_eq!(dark.brand, dark_brand, "{name} dark brand");
        assert_eq!(light.brand, light_brand, "{name} light brand");
        assert_eq!(dark.bg, dark_bg, "{name} dark bg");
        assert_eq!(light.bg, light_bg, "{name} light bg");
        // Toggling stays inside the pack's own maps.
        assert_eq!(dark.toggled().brand, light.brand, "{name} toggled");
    }
}

#[test]
fn palette_parses_an_optional_png_background() {
    let mut value = ember_json();
    value["background"] = serde_json::json!({
        "source": { "kind": "file", "path": "/opt/liang/stage-00.png" },
        "fit": "cover",
        "anchor": { "x": 0.75, "y": 0.5 },
        "opacity": 0.42
    });

    let pack = PalettePack::from_json(&value).expect("theme background");
    let debug = format!("{pack:?}");
    assert!(debug.contains("/opt/liang/stage-00.png"), "{debug}");
    assert!(debug.contains("Cover"), "{debug}");
    assert!(debug.contains("0.75"), "{debug}");
    assert!(debug.contains("0.42"), "{debug}");
}

#[test]
fn missing_token_extra_token_and_bad_hex_are_errors() {
    let mut missing = ember_json();
    missing["dark"].as_object_mut().unwrap().remove("brand");
    assert!(PalettePack::from_json(&missing).is_err());

    let mut extra = ember_json();
    extra["dark"]
        .as_object_mut()
        .unwrap()
        .insert("neon".into(), serde_json::json!("#FFFFFF"));
    assert!(PalettePack::from_json(&extra).is_err());

    for bad in ["#F78C3", "F78C3C", "#F78C3C00", "#GGGGGG", "#f78c3c0", ""] {
        let mut hex = ember_json();
        hex["dark"]["brand"] = serde_json::json!(bad);
        assert!(
            PalettePack::from_json(&hex).is_err(),
            "expected rejection for {bad}"
        );
    }
}

#[test]
fn wrong_protocol_is_ignored_without_panic() {
    let ignored = parse_palette_notification(&serde_json::json!({
        "protocol": 1,
        "palette": ember_json(),
        "activate": true,
    }))
    .expect("protocol mismatch is not a crash");
    assert!(ignored.is_none());

    let ignored = parse_palette_notification(&serde_json::json!({
        "palette": ember_json(),
        "activate": true,
    }))
    .expect("missing protocol is not a crash");
    assert!(ignored.is_none());
}

#[test]
fn toggled_ember_stays_ember() {
    let pack = PalettePack::from_json(&ember_json()).unwrap();
    let dark = pack.theme(Mode::Dark);
    let light = dark.toggled();
    assert_eq!(light.mode, Mode::Light);
    assert_eq!(light.brand, Color::Rgb(217, 106, 30));
    assert_ne!(light.brand, DEEPSEEK_450);
    assert_ne!(light.brand, DEEPSEEK_500);
    let back = light.toggled();
    assert_eq!(back.mode, Mode::Dark);
    assert_eq!(back.brand, Color::Rgb(247, 140, 60));
}

#[test]
fn default_toggled_still_uses_deepseek_blue() {
    assert_eq!(Theme::dark().toggled().brand, DEEPSEEK_500);
    assert_eq!(Theme::light().toggled().brand, DEEPSEEK_450);
    assert_eq!(Theme::dark().toggled().brand, Theme::light().brand);
}

#[test]
fn palette_notification_activate_flag() {
    let on = parse_palette_notification(&ember_notification(true))
        .unwrap()
        .expect("protocol 0");
    assert!(on.activate);
    assert_eq!(on.pack.id, "ember");

    let off = parse_palette_notification(&ember_notification(false))
        .unwrap()
        .expect("protocol 0");
    assert!(!off.activate);

    let missing = parse_palette_notification(&serde_json::json!({
        "protocol": 0,
        "palette": ember_json(),
    }))
    .unwrap()
    .expect("protocol 0");
    assert!(!missing.activate);
}

#[test]
fn palette_notification_carries_dynamic_plugin_ownership_and_load_state() {
    let notification = parse_palette_notification(&serde_json::json!({
        "protocol": 0,
        "palette": ember_json(),
        "activate": false,
        "loaded": false,
        "source": "dynamic",
        "owner": { "pluginId": "night-lime-1" },
    }))
    .unwrap()
    .expect("protocol 0");

    assert_eq!(notification.pack.plugin_id.as_deref(), Some("night-lime-1"));
    assert_eq!(notification.pack.source, "dynamic");
    assert!(!notification.pack.loaded);
}
