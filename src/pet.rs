//! Liang, the composer pet — a pixel-art homage to DeepSeek's founder
//! 梁文锋 (Liang Wenfeng), perched at the composer's right edge.
//!
//! Two states, driven by the run state: **idle** goes 🤫 (shush — quiet
//! research), **working** hammers away on a tiny terminal while DeepSeek
//! runs. `/liang` toggles him; see README "The /liang meme".
//!
//! High-pixel path: the kitty graphics protocol (ghostty, kitty, WezTerm)
//! in immediate transmit-and-display mode (`a=T`), re-sent on every state
//! or layout change (~43KB of base64 — nothing, and far more portable than
//! id-referencing placements). Terminals without the protocol get the
//! `WHALE_XS` half-block whale drawn by `ui` instead.

use std::io::{self, Write};
use std::sync::OnceLock;

use image::ImageEncoder;
use ratatui::layout::Rect;

/// 192×208 RGBA sprites (transparent background), from the
/// `pet-two-states` asset drop — see `assets/pet/`.
pub const LIANG_IDLE_PNG: &[u8] = include_bytes!("../assets/pet/liang-idle.png");
pub const LIANG_WORKING_PNG: &[u8] = include_bytes!("../assets/pet/liang-working.png");

/// Sprite frame pixel size — `ui::pet_rect` keeps the cell box aspect-true.
pub const SPRITE_W: u32 = 192;
pub const SPRITE_H: u32 = 208;

/// Arbitrary but stable ids so re-runs replace rather than accumulate.
const ID_IDLE: u32 = 4207;
const ID_WORKING: u32 = 4208;

/// Kitty graphics chunk payload limit (base64 chars per escape).
const CHUNK: usize = 4096;

/// Emitted by `restore_terminal` so panics also clear any placement.
pub const KITTY_DELETE_ALL: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

/// Does the terminal speak the kitty graphics protocol? Env-based sniff:
/// querying would race the crossterm input thread for the reply bytes.
pub fn kitty_supported() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_default();
        let prog = std::env::var("TERM_PROGRAM").unwrap_or_default();
        term.contains("kitty")
            || term.contains("ghostty")
            || prog.eq_ignore_ascii_case("ghostty")
            || prog.eq_ignore_ascii_case("wezterm")
    })
}

/// The two sprite states: (kitty image id, PNG), indexed by `working`.
const SPRITES: [(u32, &[u8]); 2] = [(ID_IDLE, LIANG_IDLE_PNG), (ID_WORKING, LIANG_WORKING_PNG)];

/// What the terminal currently displays for one placement: the cell box and
/// the kitty placement id showing it. The PNG itself is transmitted once
/// under its image id — moves and state toggles only re-place it.
#[derive(Clone, Copy)]
struct Placed {
    rect: Rect,
    placement: u32,
}

/// Reconciles the desired sprite with what the terminal displays. Each
/// sprite PNG is transmitted once; moving (the composer growing with a long
/// draft) and idle↔working toggles only re-place it, a few bytes each.
pub struct Pet {
    enabled: bool,
    transmitted: [bool; SPRITES.len()],
    shown: Option<(usize, Placed)>,
}

impl Pet {
    pub fn new(enabled: bool) -> Self {
        Pet {
            enabled,
            transmitted: [false; SPRITES.len()],
            shown: None,
        }
    }

    /// Make the terminal match `want` (cell box + working flag, or None to
    /// hide). Idempotent and zero-cost when nothing changed.
    pub fn sync(&mut self, out: &mut impl Write, want: Option<(Rect, bool)>) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let desired = want.map(|(rect, working)| (usize::from(working), rect));
        if self.shown.as_ref().map(|(image, placed)| (*image, placed.rect)) == desired {
            return Ok(());
        }
        if let Some((image, rect)) = desired {
            if !self.transmitted[image] {
                let (id, png) = SPRITES[image];
                transmit_kitty(out, id, png)?;
                self.transmitted[image] = true;
            }
            // Place the (possibly just transmitted) sprite at the new cell
            // before dropping the old placement, so the pet never blinks
            // out between frames.
            let next = match self.shown {
                Some((old_image, placed)) if old_image == image => {
                    placed.placement.wrapping_add(1).max(1)
                }
                _ => 1,
            };
            place_kitty(out, SPRITES[image].0, next, rect, 0)?;
            if let Some((old_image, old)) =
                self.shown.replace((image, Placed { rect, placement: next }))
            {
                delete_placement_kitty(out, SPRITES[old_image].0, old.placement)?;
            }
        } else if self.shown.take().is_some() {
            // Hide: drop both sprites' data and placements (deleting an id
            // that was never transmitted is a harmless no-op) and forget
            // the transmission state, so a re-show retransmits.
            for (id, _) in SPRITES {
                delete_kitty(out, id)?;
            }
            self.transmitted = [false; SPRITES.len()];
        }
        out.flush()
    }
}

/// Parse pixel dimensions from a PNG (the only format thumbnailed without a
/// decoder — clipboard screenshots are PNG; other formats fall back to text).
pub fn image_dims(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || &data[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    Some((w, h))
}

/// One visible image thumbnail the terminal should currently show.
pub struct ThumbShot<'a> {
    pub id: u32,
    pub rect: Rect,
    pub data: &'a [u8],
}

/// Keeps kitty-graphics thumbnail placements in sync with the visible chat
/// viewport: emit when new, re-place when scrolled, delete when scrolled
/// away. Placement moves are a few bytes (`a=p` re-uses the transmitted
/// image); a full retransmit only ever happens on first sight. Scrolling the
/// transcript used to delete and re-send the entire PNG on every step, which
/// stalled the frame write and blinked the composer caret.
#[derive(Default)]
pub struct Thumbnails {
    shown: std::collections::HashMap<u32, Placed>,
}

impl Thumbnails {
    pub fn new() -> Self {
        Thumbnails::default()
    }

    pub fn sync(&mut self, out: &mut impl Write, visible: &[ThumbShot]) -> io::Result<()> {
        use std::collections::HashSet;
        let mut ids = HashSet::new();
        for shot in visible {
            ids.insert(shot.id);
            match self.shown.get(&shot.id).copied() {
                Some(shown) if shown.rect == shot.rect => {}
                Some(shown) => {
                    // Moved (viewport scrolled): place the already
                    // transmitted image at the new cell, then drop the old
                    // placement — place first, so the thumbnail never
                    // blinks out between frames.
                    let next = shown.placement.wrapping_add(1).max(1);
                    place_kitty(out, shot.id, next, shot.rect, 0)?;
                    delete_placement_kitty(out, shot.id, shown.placement)?;
                    self.shown.insert(
                        shot.id,
                        Placed {
                            rect: shot.rect,
                            placement: next,
                        },
                    );
                }
                None => {
                    // First sight: transmit the PNG under the image id and
                    // create placement 1. Transmit (a=t) and placement
                    // (a=p) stay separate so every placement is one we
                    // track and can delete by id.
                    transmit_kitty(out, shot.id, shot.data)?;
                    place_kitty(out, shot.id, 1, shot.rect, 0)?;
                    self.shown.insert(
                        shot.id,
                        Placed {
                            rect: shot.rect,
                            placement: 1,
                        },
                    );
                }
            }
        }
        let gone: Vec<u32> = self
            .shown
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        for id in gone {
            delete_kitty(out, id)?;
            self.shown.remove(&id);
        }
        out.flush()
    }
}

/// Upload the PNG under `id` without displaying it.
fn transmit_kitty(out: &mut impl Write, id: u32, data: &[u8]) -> io::Result<()> {
    let b64 = base64(data);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(CHUNK).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 != chunks.len());
        if i == 0 {
            write!(out, "\x1b_Ga=t,f=100,i={id},q=2,m={more};")?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        write!(out, "\x1b\\")?;
    }
    Ok(())
}

/// Create one placement of an already-transmitted image at `cell`.
fn place_kitty(
    out: &mut impl Write,
    id: u32,
    placement: u32,
    cell: Rect,
    z: i32,
) -> io::Result<()> {
    // DECSC · jump to the cell · place · DECRC.
    write!(
        out,
        "\x1b7\x1b[{};{}H\x1b_Ga=p,i={id},p={placement},c={},r={},C=1,z={z},q=2\x1b\\\x1b8",
        cell.y + 1,
        cell.x + 1,
        cell.width,
        cell.height
    )
}

/// Drop one placement (the image data stays transmitted and re-placeable).
fn delete_placement_kitty(out: &mut impl Write, id: u32, placement: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=p,i={id},p={placement},q=2\x1b\\")
}

const BACKDROP_ID: u32 = 4210;

/// Reconciles the active Theme's optional PNG image behind terminal text.
pub struct Backdrop {
    enabled: bool,
    shown: Option<(crate::theme::ThemeBackground, Rect)>,
}

impl Backdrop {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            shown: None,
        }
    }

    pub fn sync(
        &mut self,
        out: &mut impl Write,
        want: Option<&crate::theme::ThemeBackground>,
        screen: Rect,
    ) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let desired = want.cloned().map(|background| (background, screen));
        if self.shown == desired {
            return Ok(());
        }
        if self.shown.take().is_some() {
            delete_kitty(out, BACKDROP_ID)?;
        }
        if let Some((background, rect)) = desired {
            let data = match &background.source {
                crate::theme::BackgroundSource::File { path } => std::fs::read(path)?,
                crate::theme::BackgroundSource::Data { base64 } => decode_base64(base64)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid PNG base64")
                    })?,
            };
            let Some(dimensions) = image_dims(&data) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "theme background must be a PNG",
                ));
            };
            let placement = backdrop_rect(&background, dimensions, rect);
            let data = prepare_background(data, &background, rect)?;
            transmit_kitty(out, BACKDROP_ID, &data)?;
            place_kitty(out, BACKDROP_ID, 1, placement, -1)?;
            self.shown = Some((background, rect));
        }
        out.flush()
    }
}

fn backdrop_rect(
    background: &crate::theme::ThemeBackground,
    dimensions: (u32, u32),
    screen: Rect,
) -> Rect {
    if background.fit != crate::theme::BackgroundFit::Contain
        || screen.width == 0
        || screen.height == 0
        || dimensions.0 == 0
        || dimensions.1 == 0
    {
        return screen;
    }
    let image_aspect = f64::from(dimensions.0) / f64::from(dimensions.1);
    let screen_aspect = f64::from(screen.width) / (f64::from(screen.height) * 2.0);
    let (width, height) = if image_aspect >= screen_aspect {
        let height = (f64::from(screen.width) / image_aspect / 2.0)
            .round()
            .clamp(1.0, f64::from(screen.height)) as u16;
        (screen.width, height)
    } else {
        let width = (f64::from(screen.height) * 2.0 * image_aspect)
            .round()
            .clamp(1.0, f64::from(screen.width)) as u16;
        (width, screen.height)
    };
    let free_x = screen.width.saturating_sub(width);
    let free_y = screen.height.saturating_sub(height);
    Rect::new(
        screen.x + (f64::from(free_x) * background.anchor.0).round() as u16,
        screen.y + (f64::from(free_y) * background.anchor.1).round() as u16,
        width,
        height,
    )
}

fn prepare_background(
    data: Vec<u8>,
    background: &crate::theme::ThemeBackground,
    screen: Rect,
) -> io::Result<Vec<u8>> {
    let crop_cover = background.fit == crate::theme::BackgroundFit::Cover
        && screen.width > 0
        && screen.height > 0;
    if background.opacity >= 1.0 && !crop_cover {
        return Ok(data);
    }
    let image = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut rgba = image.to_rgba8();
    if crop_cover {
        let width = rgba.width();
        let height = rgba.height();
        let image_aspect = f64::from(width) / f64::from(height);
        let target_aspect = f64::from(screen.width) / (f64::from(screen.height) * 2.0);
        if image_aspect > target_aspect {
            let crop_width = (f64::from(height) * target_aspect)
                .round()
                .clamp(1.0, f64::from(width)) as u32;
            let x = (f64::from(width - crop_width) * background.anchor.0).round() as u32;
            rgba = image::imageops::crop_imm(&rgba, x, 0, crop_width, height).to_image();
        } else if image_aspect < target_aspect {
            let crop_height = (f64::from(width) / target_aspect)
                .round()
                .clamp(1.0, f64::from(height)) as u32;
            let y = (f64::from(height - crop_height) * background.anchor.1).round() as u32;
            rgba = image::imageops::crop_imm(&rgba, 0, y, width, crop_height).to_image();
        }
    }
    if background.opacity < 1.0 {
        for pixel in rgba.pixels_mut() {
            pixel.0[3] =
                ((f64::from(pixel.0[3]) * background.opacity).round()).clamp(0.0, 255.0) as u8;
        }
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(encoded)
}

fn delete_kitty(out: &mut impl Write, id: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

/// Decode standard base64 (RFC 4648, with padding). Whitespace is ignored.
pub(crate) fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let chars: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if chars.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(chars.len() * 3 / 4);
    for chunk in chars.chunks(4) {
        let a = val(*chunk.first()?)?;
        let b = val(*chunk.get(1)?)?;
        out.push((a << 2) | (b >> 4));
        if chunk.len() > 2 {
            let c = val(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk.len() > 3 {
                let d = val(chunk[3])?;
                out.push((c << 6) | d);
            }
        }
    }
    Some(out)
}

/// Standard base64 (RFC 4648, with padding) — small enough to not need a dep.
pub(crate) fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = u32::from_be_bytes([
            0,
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ]);
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/pet__tests.rs"]
mod tests;

/// The XS half-block whale — the pet's fallback art for terminals without
/// a pixel protocol (drawn by `ui::draw_pet_chars` with a brand gradient).
/// Formerly part of the generated `logo_data.rs`, now owned by the pet.
pub const WHALE_XS: [&str; 3] = ["▄███▄█▄▄", "█ ▀████", "▀█▄▄██"];
