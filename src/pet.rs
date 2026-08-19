//! Kitty graphics infrastructure: chat image thumbnails and the Theme's
//! optional PNG background layer, transmitted through the kitty graphics
//! protocol (ghostty, kitty, WezTerm) in immediate transmit-and-display
//! mode (`a=T`), re-sent on every state or layout change.

use std::io::{self, Write};
use std::sync::OnceLock;

use image::ImageEncoder;
use ratatui::layout::Rect;

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
/// viewport: emit when new/moved, delete when scrolled away.
#[derive(Default)]
pub struct Thumbnails {
    shown: std::collections::HashMap<u32, Rect>,
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
            if self.shown.get(&shot.id) != Some(&shot.rect) {
                if self.shown.remove(&shot.id).is_some() {
                    delete_kitty(out, shot.id)?;
                }
                emit_kitty(out, shot.id, shot.data, shot.rect, 0)?;
                self.shown.insert(shot.id, shot.rect);
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

fn emit_kitty(
    out: &mut impl Write,
    id: u32,
    data: &[u8],
    cell: Rect,
    z_index: i32,
) -> io::Result<()> {
    write!(out, "\x1b7\x1b[{};{}H", cell.y + 1, cell.x + 1)?;
    let b64 = base64(data);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(CHUNK).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 != chunks.len());
        if i == 0 {
            write!(
                out,
                "\x1b_Ga=T,f=100,i={id},c={},r={},C=1,z={z_index},q=2,m={more};",
                cell.width, cell.height,
            )?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        write!(out, "\x1b\\")?;
    }
    write!(out, "\x1b8")
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
            emit_kitty(out, BACKDROP_ID, &data, placement, -1)?;
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
mod tests {
    use super::*;

    fn transmitted_png(output: Vec<u8>) -> Vec<u8> {
        let shown = String::from_utf8(output).unwrap();
        let payload = shown
            .split("\x1b_G")
            .skip(1)
            .filter_map(|chunk| chunk.split_once(';').map(|(_, payload)| payload))
            .filter_map(|payload| payload.split("\x1b\\").next())
            .collect::<String>();
        decode_base64(&payload).expect("kitty PNG payload")
    }

    /// A 192×208 RGBA PNG — the same geometry the original sprites used, so
    /// the background geometry assertions stay stable without shipping any
    /// pet assets.
    fn sample_png() -> Vec<u8> {
        let mut rgba = image::RgbaImage::new(192, 208);
        for (i, pixel) in rgba.pixels_mut().enumerate() {
            let t = (i % 255) as u8;
            pixel.0 = [t, 40, 200, 255];
        }
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                rgba.as_raw(),
                192,
                208,
                image::ExtendedColorType::Rgba8,
            )
            .expect("encode sample PNG");
        out
    }

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
            assert_eq!(decode_base64(enc).as_deref().unwrap_or(&[]), raw.as_bytes());
        }
    }

    #[test]
    fn image_dims_reads_png_ihdr() {
        let mut png = vec![0u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[16..20].copy_from_slice(&320u32.to_be_bytes());
        png[20..24].copy_from_slice(&240u32.to_be_bytes());
        assert_eq!(image_dims(&png), Some((320, 240)));
        assert_eq!(image_dims(b"not a png"), None);
        assert_eq!(image_dims(&png[..20]), None, "truncated header");
    }

    #[test]
    fn theme_background_is_placed_behind_text_and_retracted() {
        let file = std::env::temp_dir().join(format!(
            "dsh-tui-background-{}-{}.png",
            std::process::id(),
            1
        ));
        std::fs::write(&file, sample_png()).unwrap();
        let spec = crate::theme::ThemeBackground {
            source: crate::theme::BackgroundSource::File {
                path: file.to_string_lossy().into_owned(),
            },
            fit: crate::theme::BackgroundFit::Cover,
            anchor: (0.75, 0.5),
            opacity: 0.42,
        };
        let mut background = Backdrop::new(true);
        let mut out = Vec::new();

        background
            .sync(&mut out, Some(&spec), Rect::new(0, 0, 100, 30))
            .unwrap();
        let shown = String::from_utf8(out.clone()).unwrap();
        assert!(shown.contains("z=-1"), "{shown}");
        assert!(shown.contains("c=100,r=30"), "{shown}");

        out.clear();
        background
            .sync(&mut out, Some(&spec), Rect::new(0, 0, 100, 30))
            .unwrap();
        assert!(out.is_empty(), "unchanged background is not retransmitted");

        background.sync(&mut out, None, Rect::default()).unwrap();
        let removed = String::from_utf8(out).unwrap();
        assert!(removed.contains("a=d,d=I"), "{removed}");
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn theme_background_opacity_changes_the_transmitted_png() {
        let encoded = base64(&sample_png());
        let spec = crate::theme::ThemeBackground {
            source: crate::theme::BackgroundSource::Data {
                base64: encoded.clone(),
            },
            fit: crate::theme::BackgroundFit::Stretch,
            anchor: (0.5, 0.5),
            opacity: 0.25,
        };
        let mut background = Backdrop::new(true);
        let mut out = Vec::new();

        background
            .sync(&mut out, Some(&spec), Rect::new(0, 0, 80, 24))
            .unwrap();

        let transmitted = base64(&transmitted_png(out));
        assert_ne!(
            transmitted, encoded,
            "opacity must produce a new alpha-adjusted PNG"
        );
    }

    #[test]
    fn contained_theme_background_preserves_aspect_and_anchor() {
        let spec = crate::theme::ThemeBackground {
            source: crate::theme::BackgroundSource::Data {
                base64: base64(&sample_png()),
            },
            fit: crate::theme::BackgroundFit::Contain,
            anchor: (1.0, 0.5),
            opacity: 1.0,
        };
        let mut background = Backdrop::new(true);
        let mut out = Vec::new();

        background
            .sync(&mut out, Some(&spec), Rect::new(0, 0, 100, 30))
            .unwrap();

        let shown = String::from_utf8(out).unwrap();
        assert!(shown.contains("\x1b[1;46H"), "right anchored: {shown}");
        assert!(shown.contains("c=55,r=30"), "aspect preserved: {shown}");
    }

    #[test]
    fn covered_theme_background_crops_to_the_terminal_aspect() {
        let spec = crate::theme::ThemeBackground {
            source: crate::theme::BackgroundSource::Data {
                base64: base64(&sample_png()),
            },
            fit: crate::theme::BackgroundFit::Cover,
            anchor: (0.5, 1.0),
            opacity: 1.0,
        };
        let mut background = Backdrop::new(true);
        let mut out = Vec::new();

        background
            .sync(&mut out, Some(&spec), Rect::new(0, 0, 100, 30))
            .unwrap();

        assert_eq!(image_dims(&transmitted_png(out)), Some((192, 115)));
    }
}
