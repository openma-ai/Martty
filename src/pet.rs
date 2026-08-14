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

/// What's on screen: the sprite's cell box and whether he's working.
type Shown = (Rect, bool);

/// Reconciles the desired sprite with what the terminal displays.
pub struct Pet {
    enabled: bool,
    shown: Option<Shown>,
}

impl Pet {
    pub fn new(enabled: bool) -> Self {
        Pet { enabled, shown: None }
    }

    /// Make the terminal match `want` (cell box + working flag, or None to
    /// hide). Idempotent and zero-cost when nothing changed.
    pub fn sync(&mut self, out: &mut impl Write, want: Option<Shown>) -> io::Result<()> {
        if !self.enabled || self.shown == want {
            return Ok(());
        }
        if let Some((_, was_working)) = self.shown.take() {
            let id = if was_working { ID_WORKING } else { ID_IDLE };
            write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")?;
        }
        if let Some((cell, working)) = want {
            let (id, png) = if working {
                (ID_WORKING, LIANG_WORKING_PNG)
            } else {
                (ID_IDLE, LIANG_IDLE_PNG)
            };
            // DECSC · jump to the cell · transmit-and-display · DECRC.
            write!(out, "\x1b7\x1b[{};{}H", cell.y + 1, cell.x + 1)?;
            let data = base64(png);
            let chunks: Vec<&[u8]> = data.as_bytes().chunks(CHUNK).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                let more = u8::from(i + 1 != chunks.len());
                if i == 0 {
                    write!(
                        out,
                        "\x1b_Ga=T,f=100,i={id},c={},r={},C=1,z=0,q=2,m={more};",
                        cell.width, cell.height
                    )?;
                } else {
                    write!(out, "\x1b_Gm={more};")?;
                }
                out.write_all(chunk)?;
                write!(out, "\x1b\\")?;
            }
            write!(out, "\x1b8")?;
            self.shown = Some((cell, working));
        }
        out.flush()
    }
}

/// Standard base64 (RFC 4648, with padding) — small enough to not need a dep.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = u32::from_be_bytes([0, chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)]);
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn sprites_are_pngs() {
        assert_eq!(&LIANG_IDLE_PNG[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&LIANG_WORKING_PNG[..8], b"\x89PNG\r\n\x1a\n");
        assert_ne!(LIANG_IDLE_PNG, LIANG_WORKING_PNG, "two distinct states");
    }

    #[test]
    fn sync_shows_switches_state_and_hides() {
        let mut pet = Pet::new(true);
        let cell = Rect::new(90, 30, 7, 4);

        // Idle: one-shot transmit-and-display under the idle id.
        let mut out = Vec::new();
        pet.sync(&mut out, Some((cell, false))).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\x1b[31;91H"), "cursor jumps to the cell");
        assert!(s.contains("a=T,f=100,i=4207,c=7,r=4,C=1,z=0,q=2,m=1;"), "idle sprite: {s:.90}");

        // Same state again: no bytes at all.
        let mut out = Vec::new();
        pet.sync(&mut out, Some((cell, false))).unwrap();
        assert!(out.is_empty(), "idempotent when unchanged");

        // A turn starts: idle sprite deleted, working sprite displayed.
        let mut out = Vec::new();
        pet.sync(&mut out, Some((cell, true))).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("a=d,d=I,i=4207"), "idle deleted: {s:.90}");
        assert!(s.contains("a=T,f=100,i=4208,"), "working shown: {s:.90}");

        // Hide (`/liang` off): delete only.
        let mut out = Vec::new();
        pet.sync(&mut out, None).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("a=d,d=I,i=4208") && !s.contains("a=T"), "hidden: {s}");
    }

    #[test]
    fn disabled_pet_stays_silent() {
        let mut pet = Pet::new(false);
        let mut out = Vec::new();
        pet.sync(&mut out, Some((Rect::new(0, 0, 7, 4), true))).unwrap();
        assert!(out.is_empty());
    }
}
