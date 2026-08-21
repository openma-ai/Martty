//! DeepSeek Harness Web UI design tokens, mapped 1:1 from
//! `packages/client/ui-theme/src/styles/design-platform.css`.
//!
//! Static palette (`--dsw-static-*`) plus semantic themes: a cold
//! neutral-bluish base kept deliberately quiet, with a small reserved
//! accent vocabulary — DeepSeek blue for brand/actions, gray-blue for
//! hints, green for success/liveness, amber for attention, red for
//! errors. Minimal, not monotone.

use ratatui::style::Color;
use serde_json::Value;

// --- static palette -------------------------------------------------------

#[allow(dead_code)]
pub const DEEPSEEK_50: Color = Color::Rgb(237, 243, 254);
#[allow(dead_code)]
pub const DEEPSEEK_100: Color = Color::Rgb(228, 237, 253);
#[allow(dead_code)]
pub const DEEPSEEK_200: Color = Color::Rgb(211, 226, 255);
#[allow(dead_code)]
pub const DEEPSEEK_300: Color = Color::Rgb(183, 200, 254);
#[allow(dead_code)]
pub const DEEPSEEK_400: Color = Color::Rgb(103, 158, 254);
#[allow(dead_code)]
pub const DEEPSEEK_450: Color = Color::Rgb(86, 134, 254);
#[allow(dead_code)]
pub const DEEPSEEK_500: Color = Color::Rgb(65, 118, 230);
#[allow(dead_code)]
pub const DEEPSEEK_600: Color = Color::Rgb(72, 104, 178);
#[allow(dead_code)]
pub const DEEPSEEK_800: Color = Color::Rgb(52, 65, 91);
#[allow(dead_code)]
pub const DEEPSEEK_900: Color = Color::Rgb(40, 49, 66);

#[allow(dead_code)]
pub const BLUISH_00: Color = Color::Rgb(255, 255, 255);
#[allow(dead_code)]
pub const BLUISH_50: Color = Color::Rgb(249, 250, 251);
#[allow(dead_code)]
pub const BLUISH_60: Color = Color::Rgb(245, 246, 247);
#[allow(dead_code)]
pub const BLUISH_75: Color = Color::Rgb(241, 243, 245);
#[allow(dead_code)]
pub const BLUISH_100: Color = Color::Rgb(235, 238, 242);
#[allow(dead_code)]
pub const BLUISH_150: Color = Color::Rgb(233, 236, 242);
#[allow(dead_code)]
pub const BLUISH_200: Color = Color::Rgb(225, 229, 238);
#[allow(dead_code)]
pub const BLUISH_300: Color = Color::Rgb(207, 211, 214);
#[allow(dead_code)]
pub const BLUISH_400: Color = Color::Rgb(173, 178, 184);
#[allow(dead_code)]
pub const BLUISH_500: Color = Color::Rgb(151, 157, 166);
#[allow(dead_code)]
pub const BLUISH_600: Color = Color::Rgb(129, 133, 140);
#[allow(dead_code)]
pub const BLUISH_700: Color = Color::Rgb(97, 102, 107);
#[allow(dead_code)]
pub const BLUISH_750: Color = Color::Rgb(67, 69, 74);
#[allow(dead_code)]
pub const BLUISH_800: Color = Color::Rgb(53, 54, 56);
#[allow(dead_code)]
pub const BLUISH_850: Color = Color::Rgb(44, 44, 46);
#[allow(dead_code)]
pub const BLUISH_875: Color = Color::Rgb(35, 35, 36);
#[allow(dead_code)]
pub const BLUISH_900: Color = Color::Rgb(27, 27, 28);
#[allow(dead_code)]
pub const BLUISH_950: Color = Color::Rgb(21, 21, 23);
#[allow(dead_code)]
pub const BLUISH_1000: Color = Color::Rgb(15, 17, 21);

#[allow(dead_code)]
pub const RED_400: Color = Color::Rgb(242, 90, 90);
#[allow(dead_code)]
pub const RED_500: Color = Color::Rgb(239, 68, 68);
#[allow(dead_code)]
pub const RED_600: Color = Color::Rgb(236, 19, 19);
#[allow(dead_code)]
pub const GREEN_400: Color = Color::Rgb(78, 209, 126);
#[allow(dead_code)]
pub const GREEN_500: Color = Color::Rgb(34, 197, 94);
#[allow(dead_code)]
pub const AMBER_400: Color = Color::Rgb(247, 173, 49);
#[allow(dead_code)]
pub const AMBER_500: Color = Color::Rgb(245, 158, 11);
#[allow(dead_code)]
pub const AMBER_600: Color = Color::Rgb(221, 134, 41);

/// Blue-gray (slate) hint tones — gray first, a cool blue undertone;
/// clearly quieter than the DeepSeek blues.
pub const SLATE_400: Color = Color::Rgb(108, 122, 150);
pub const SLATE_600: Color = Color::Rgb(84, 96, 120);

// --- semantic theme -------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }
}

/// Closed token names for protocol 0 palettes (`tuiTheme.register` / Cordis theme update).
pub const TOKEN_NAMES: &[&str] = &[
    "bg",
    "surface",
    "panel",
    "fg",
    "fg_secondary",
    "fg_tertiary",
    "caption",
    "brand",
    "brand_soft",
    "bubble_bg",
    "bubble_fg",
    "border",
    "code_bg",
    "ok",
    "warn",
    "err",
    "hint",
    "chip_bg",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TokenMap {
    bg: Color,
    surface: Color,
    panel: Color,
    fg: Color,
    fg_secondary: Color,
    fg_tertiary: Color,
    caption: Color,
    brand: Color,
    brand_soft: Color,
    bubble_bg: Color,
    bubble_fg: Color,
    border: Color,
    code_bg: Color,
    ok: Color,
    warn: Color,
    err: Color,
    hint: Color,
    chip_bg: Color,
}

const DEFAULT_DARK: TokenMap = TokenMap {
    bg: BLUISH_1000,
    surface: BLUISH_950,
    panel: BLUISH_900,
    fg: BLUISH_50,
    fg_secondary: BLUISH_300,
    fg_tertiary: BLUISH_500,
    caption: BLUISH_600,
    brand: DEEPSEEK_450,
    brand_soft: DEEPSEEK_400,
    bubble_bg: BLUISH_900,
    bubble_fg: BLUISH_75,
    border: BLUISH_850,
    code_bg: BLUISH_950,
    ok: GREEN_400,
    warn: AMBER_400,
    err: RED_400,
    hint: SLATE_400,
    chip_bg: BLUISH_850,
};

const DEFAULT_LIGHT: TokenMap = TokenMap {
    bg: BLUISH_00,
    surface: BLUISH_50,
    panel: BLUISH_60,
    fg: BLUISH_1000,
    fg_secondary: BLUISH_750,
    fg_tertiary: BLUISH_700,
    caption: BLUISH_400,
    brand: DEEPSEEK_500,
    brand_soft: DEEPSEEK_450,
    bubble_bg: BLUISH_75,
    bubble_fg: BLUISH_1000,
    border: BLUISH_200,
    code_bg: BLUISH_60,
    ok: GREEN_500,
    warn: AMBER_600,
    err: RED_600,
    hint: SLATE_600,
    chip_bg: BLUISH_100,
};

/// Why a protocol-0 palette was rejected. Wrong `protocol` is not an error:
/// the compositor ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteError(pub String);

impl std::fmt::Display for PaletteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A named dark/light token pack. Built-in `default` plus registered Cordis packs.
#[derive(Clone, Debug, PartialEq)]
pub struct PalettePack {
    pub id: String,
    pub label: String,
    /// Dynamic Client Plugin that owns this selectable theme, when any.
    pub plugin_id: Option<String>,
    /// Whether the owning Plugin is currently mounted.
    pub loaded: bool,
    /// Optional terminal background owned by this theme pack.
    pub background: Option<ThemeBackground>,
    dark: TokenMap,
    light: TokenMap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackgroundSource {
    File { path: String },
    Data { base64: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundFit {
    Cover,
    Contain,
    Stretch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeBackground {
    pub source: BackgroundSource,
    pub fit: BackgroundFit,
    pub anchor: (f64, f64),
    pub opacity: f64,
}

/// A parsed Cordis TUI theme update (`protocol` 0).
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteNotification {
    pub pack: PalettePack,
    pub activate: bool,
}

impl PalettePack {
    pub fn builtin_default() -> Self {
        Self {
            id: "default".into(),
            label: "Default".into(),
            plugin_id: None,
            loaded: true,
            background: None,
            dark: DEFAULT_DARK,
            light: DEFAULT_LIGHT,
        }
    }

    /// Current-mode `Theme` for this pack. Toggle stays inside these maps.
    pub fn theme(&self, mode: Mode) -> Theme {
        Theme::from_maps(mode, self.dark, self.light)
    }

    /// Parse a palette object (`id` / `label` / complete `dark`+`light` maps).
    pub fn from_json(v: &Value) -> Result<Self, PaletteError> {
        let obj = v
            .as_object()
            .ok_or_else(|| PaletteError("palette must be an object".into()))?;
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "id" | "label" | "dark" | "light" | "background"
            ) {
                return Err(PaletteError(format!("unknown palette field {key}")));
            }
        }
        let id = obj.get("id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            return Err(PaletteError("palette id must be a non-empty string".into()));
        }
        let label = obj.get("label").and_then(Value::as_str).unwrap_or("");
        if label.is_empty() {
            return Err(PaletteError(
                "palette label must be a non-empty string".into(),
            ));
        }
        let dark = parse_token_map(obj.get("dark").unwrap_or(&Value::Null))?;
        let light = parse_token_map(obj.get("light").unwrap_or(&Value::Null))?;
        let background = obj.get("background").map(parse_background).transpose()?;
        Ok(Self {
            id: id.to_string(),
            label: label.to_string(),
            plugin_id: None,
            loaded: true,
            background,
            dark,
            light,
        })
    }
}

fn parse_background(v: &Value) -> Result<ThemeBackground, PaletteError> {
    let obj = v
        .as_object()
        .ok_or_else(|| PaletteError("background must be an object".into()))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "source" | "fit" | "anchor" | "opacity") {
            return Err(PaletteError(format!("unknown background field {key}")));
        }
    }
    let source_obj = obj
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| PaletteError("background source must be an object".into()))?;
    let kind = source_obj.get("kind").and_then(Value::as_str).unwrap_or("");
    let source = match kind {
        "file" => {
            if source_obj
                .keys()
                .any(|key| !matches!(key.as_str(), "kind" | "path"))
            {
                return Err(PaletteError(
                    "file background only accepts kind and path".into(),
                ));
            }
            let path = source_obj.get("path").and_then(Value::as_str).unwrap_or("");
            if path.is_empty() || !std::path::Path::new(path).is_absolute() {
                return Err(PaletteError("background file path must be absolute".into()));
            }
            BackgroundSource::File {
                path: path.to_string(),
            }
        }
        "data" => {
            if source_obj
                .keys()
                .any(|key| !matches!(key.as_str(), "kind" | "mediaType" | "base64"))
            {
                return Err(PaletteError(
                    "data background only accepts kind, mediaType and base64".into(),
                ));
            }
            if source_obj.get("mediaType").and_then(Value::as_str) != Some("image/png") {
                return Err(PaletteError(
                    "background data mediaType must be image/png".into(),
                ));
            }
            let base64 = source_obj
                .get("base64")
                .and_then(Value::as_str)
                .unwrap_or("");
            if base64.is_empty() {
                return Err(PaletteError(
                    "background data base64 must be non-empty".into(),
                ));
            }
            BackgroundSource::Data {
                base64: base64.to_string(),
            }
        }
        _ => {
            return Err(PaletteError(
                "background source kind must be file or data".into(),
            ));
        }
    };
    let fit = match obj.get("fit").and_then(Value::as_str).unwrap_or("cover") {
        "cover" => BackgroundFit::Cover,
        "contain" => BackgroundFit::Contain,
        "stretch" => BackgroundFit::Stretch,
        _ => {
            return Err(PaletteError(
                "background fit must be cover, contain or stretch".into(),
            ));
        }
    };
    let anchor_obj = match obj.get("anchor") {
        Some(value) => value
            .as_object()
            .ok_or_else(|| PaletteError("background anchor must be an object".into()))?,
        None => {
            return Ok(ThemeBackground {
                source,
                fit,
                anchor: (0.5, 0.5),
                opacity: unit_field(obj.get("opacity"), 1.0, "background opacity")?,
            })
        }
    };
    for key in anchor_obj.keys() {
        if !matches!(key.as_str(), "x" | "y") {
            return Err(PaletteError(format!(
                "unknown background anchor field {key}"
            )));
        }
    }
    Ok(ThemeBackground {
        source,
        fit,
        anchor: (
            unit_field(anchor_obj.get("x"), 0.5, "background anchor x")?,
            unit_field(anchor_obj.get("y"), 0.5, "background anchor y")?,
        ),
        opacity: unit_field(obj.get("opacity"), 1.0, "background opacity")?,
    })
}

fn unit_field(value: Option<&Value>, default: f64, at: &str) -> Result<f64, PaletteError> {
    let number = value.and_then(Value::as_f64).unwrap_or(default);
    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
        return Err(PaletteError(format!("{at} must be from 0 to 1")));
    }
    Ok(number)
}

/// Parse Cordis TUI theme-update params. `protocol != 0` (or missing) → `Ok(None)`.
pub fn parse_palette_notification(
    params: &Value,
) -> Result<Option<PaletteNotification>, PaletteError> {
    let protocol_ok = params.get("protocol").and_then(Value::as_u64) == Some(0);
    if !protocol_ok {
        return Ok(None);
    }
    let palette = params
        .get("palette")
        .ok_or_else(|| PaletteError("missing palette".into()))?;
    let mut pack = PalettePack::from_json(palette)?;
    pack.plugin_id = params
        .get("owner")
        .and_then(|owner| owner.get("pluginId"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    pack.loaded = params
        .get("loaded")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let activate = params
        .get("activate")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Some(PaletteNotification { pack, activate }))
}

fn parse_token_map(v: &Value) -> Result<TokenMap, PaletteError> {
    let map = v
        .as_object()
        .ok_or_else(|| PaletteError("token map must be an object".into()))?;
    for key in map.keys() {
        if !TOKEN_NAMES.iter().any(|n| n == key) {
            return Err(PaletteError(format!("unknown token {key}")));
        }
    }
    Ok(TokenMap {
        bg: color_field(map, "bg")?,
        surface: color_field(map, "surface")?,
        panel: color_field(map, "panel")?,
        fg: color_field(map, "fg")?,
        fg_secondary: color_field(map, "fg_secondary")?,
        fg_tertiary: color_field(map, "fg_tertiary")?,
        caption: color_field(map, "caption")?,
        brand: color_field(map, "brand")?,
        brand_soft: color_field(map, "brand_soft")?,
        bubble_bg: color_field(map, "bubble_bg")?,
        bubble_fg: color_field(map, "bubble_fg")?,
        border: color_field(map, "border")?,
        code_bg: color_field(map, "code_bg")?,
        ok: color_field(map, "ok")?,
        warn: color_field(map, "warn")?,
        err: color_field(map, "err")?,
        hint: color_field(map, "hint")?,
        chip_bg: color_field(map, "chip_bg")?,
    })
}

fn color_field(map: &serde_json::Map<String, Value>, key: &str) -> Result<Color, PaletteError> {
    let Some(v) = map.get(key) else {
        return Err(PaletteError(format!("missing token {key}")));
    };
    let Some(s) = v.as_str() else {
        return Err(PaletteError(format!("token {key} must be #RRGGBB")));
    };
    parse_hex(s).ok_or_else(|| PaletteError(format!("token {key} must be #RRGGBB")))
}

fn parse_hex(s: &str) -> Option<Color> {
    let b = s.as_bytes();
    if b.len() != 7 || b[0] != b'#' {
        return None;
    }
    let r = u8::from_str_radix(std::str::from_utf8(&b[1..3]).ok()?, 16).ok()?;
    let g = u8::from_str_radix(std::str::from_utf8(&b[3..5]).ok()?, 16).ok()?;
    let bl = u8::from_str_radix(std::str::from_utf8(&b[5..7]).ok()?, 16).ok()?;
    Some(Color::Rgb(r, g, bl))
}

/// Semantic colors — a cold monochrome remap of the Web UI neutral-bluish
/// scale (the alias slot names are kept for reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub mode: Mode,
    /// `--dsw-alias-bg-base`
    pub bg: Color,
    /// `--dsw-alias-bg-layer-1`
    #[allow(dead_code)]
    pub surface: Color,
    /// `--dsw-alias-bg-layer-2` (panels, tool cards)
    pub panel: Color,
    /// `--dsw-alias-label-primary`
    pub fg: Color,
    /// `--dsw-alias-label-secondary`
    pub fg_secondary: Color,
    /// `--dsw-alias-label-tertiary`
    pub fg_tertiary: Color,
    /// `--dsw-alias-label-caption`
    pub caption: Color,
    /// `--dsw-alias-brand-primary-new-color…` — the DeepSeek blue accent
    pub brand: Color,
    /// `--dsw-alias-state-business-primary`
    pub brand_soft: Color,
    /// `--dsw-specific-bubble` (user message bubble)
    pub bubble_bg: Color,
    /// text on the bubble
    pub bubble_fg: Color,
    /// borders (`--dsw-alias-border-l2/l3` approximated on the layer stack)
    pub border: Color,
    /// `--dsw-alias-markdown-code-block`
    pub code_bg: Color,
    /// `--dsw-alias-state-success-primary` / secondary
    pub ok: Color,
    /// `--dsw-alias-state-warn-primary` / label
    pub warn: Color,
    /// `--dsw-alias-state-error-primary`
    pub err: Color,
    /// Gray-blue hint text (tip banner, informational chips) — quieter
    /// than `brand_soft`, warmer than the neutral grays.
    pub hint: Color,
    /// selection/status chip background (picker row highlight)
    pub chip_bg: Color,
    dark: TokenMap,
    light: TokenMap,
}

impl Theme {
    fn from_maps(mode: Mode, dark: TokenMap, light: TokenMap) -> Self {
        let t = match mode {
            Mode::Dark => dark,
            Mode::Light => light,
        };
        Theme {
            mode,
            bg: t.bg,
            surface: t.surface,
            panel: t.panel,
            fg: t.fg,
            fg_secondary: t.fg_secondary,
            fg_tertiary: t.fg_tertiary,
            caption: t.caption,
            brand: t.brand,
            brand_soft: t.brand_soft,
            bubble_bg: t.bubble_bg,
            bubble_fg: t.bubble_fg,
            border: t.border,
            code_bg: t.code_bg,
            ok: t.ok,
            warn: t.warn,
            err: t.err,
            hint: t.hint,
            chip_bg: t.chip_bg,
            dark,
            light,
        }
    }

    pub fn dark() -> Self {
        Self::from_maps(Mode::Dark, DEFAULT_DARK, DEFAULT_LIGHT)
    }

    pub fn light() -> Self {
        Self::from_maps(Mode::Light, DEFAULT_DARK, DEFAULT_LIGHT)
    }

    pub fn toggled(&self) -> Self {
        match self.mode {
            Mode::Dark => Self::from_maps(Mode::Light, self.dark, self.light),
            Mode::Light => Self::from_maps(Mode::Dark, self.dark, self.light),
        }
    }

    /// Success accent used for finished tool glyphs and the idle dot.
    pub fn ok_soft(&self) -> Color {
        self.ok
    }

    /// Warn accent for chips, queue markers, and cautionary notices.
    pub fn warn_soft(&self) -> Color {
        self.warn
    }

    /// Whale banner vertical gradient stops (top → bottom), grayscale.
    pub fn whale_gradient(&self) -> (Color, Color) {
        match self.mode {
            Mode::Dark => (BLUISH_50, BLUISH_700),
            Mode::Light => (BLUISH_800, BLUISH_400),
        }
    }
}

/// Linear interpolation between two RGB colors, `t` in `[0, 1]`.
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    fn parts(c: Color) -> (f32, f32, f32) {
        match c {
            Color::Rgb(r, g, b) => (r as f32, g as f32, b as f32),
            _ => (255.0, 255.0, 255.0),
        }
    }
    let (ar, ag, ab) = parts(a);
    let (br, bg, bb) = parts(b);
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (ar + (br - ar) * t).round() as u8,
        (ag + (bg - ag) * t).round() as u8,
        (ab + (bb - ab) * t).round() as u8,
    )
}

#[cfg(test)]
mod tests {
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
        serde_json::from_str(include_str!("../docs/fixtures/demo-skin.v0.json")).unwrap()
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
            "ayu" => include_str!("../docs/fixtures/ayu.v0.json"),
            "catppuccin" => include_str!("../docs/fixtures/catppuccin.v0.json"),
            "kanagawa" => include_str!("../docs/fixtures/kanagawa.v0.json"),
            "everforest" => include_str!("../docs/fixtures/everforest.v0.json"),
            "iceberg" => include_str!("../docs/fixtures/iceberg.v0.json"),
            "solarized" => include_str!("../docs/fixtures/solarized.v0.json"),
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
                Color::Rgb(83, 189, 250), // #53BDFA Ayu dark blue
                Color::Rgb(49, 153, 225),  // #3199E1 Ayu Light blue
                Color::Rgb(11, 14, 20),   // #0B0E14
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
            "owner": { "pluginId": "night-lime-1" },
        }))
        .unwrap()
        .expect("protocol 0");

        assert_eq!(notification.pack.plugin_id.as_deref(), Some("night-lime-1"));
        assert!(!notification.pack.loaded);
    }
}
