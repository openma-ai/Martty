//! DeepSeek Harness Web UI design tokens, mapped 1:1 from
//! `packages/client/ui-theme/src/styles/design-platform.css`.
//!
//! Static palette (`--dsw-static-*`) plus semantic themes that are a
//! cold monochrome remap of the Web UI neutral-bluish scale: strictly
//! black/gray/white, with red as the only colored accent (errors).

use ratatui::style::Color;

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

// --- semantic theme -------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

/// Semantic colors — a cold monochrome remap of the Web UI neutral-bluish
/// scale (the alias slot names are kept for reference).
#[derive(Clone, Copy)]
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
    /// selection/status chip background
    #[allow(dead_code)]
    pub chip_bg: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Theme {
            mode: Mode::Dark,
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
            ok: BLUISH_200,
            warn: BLUISH_500,
            err: RED_400,
            chip_bg: BLUISH_850,
        }
    }

    pub fn light() -> Self {
        Theme {
            mode: Mode::Light,
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
            ok: BLUISH_750,
            warn: BLUISH_600,
            err: RED_600,
            chip_bg: BLUISH_100,
        }
    }

    pub fn toggled(&self) -> Self {
        match self.mode {
            Mode::Dark => Theme::light(),
            Mode::Light => Theme::dark(),
        }
    }

    /// Success accent used for finished tool glyphs (monochrome: same as `ok`).
    pub fn ok_soft(&self) -> Color {
        self.ok
    }

    /// Warn accent for chips (monochrome: same as `warn`).
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

    /// The only colored accent left is red for errors.
    #[test]
    fn err_is_the_only_colored_accent() {
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
                t.bg, t.surface, t.panel, t.fg, t.fg_secondary, t.fg_tertiary,
                t.caption, t.bubble_bg, t.bubble_fg,
                t.border, t.code_bg, t.ok, t.warn, t.chip_bg,
            ] {
                assert!(is_gray(c), "expected grayscale, got {c:?}");
            }
            assert!(!is_gray(t.err), "err must stay red");
        }
    }
}
