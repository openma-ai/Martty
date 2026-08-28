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
fn sprites_are_pngs() {
    assert_eq!(&LIANG_IDLE_PNG[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&LIANG_WORKING_PNG[..8], b"\x89PNG\r\n\x1a\n");
    assert_ne!(LIANG_IDLE_PNG, LIANG_WORKING_PNG, "two distinct states");
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
fn sync_shows_switches_state_and_hides() {
    let mut pet = Pet::new(true);
    let cell = Rect::new(90, 30, 7, 4);

    // Idle: transmit the idle PNG once, then place it.
    let mut out = Vec::new();
    pet.sync(&mut out, Some((cell, false))).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("\x1b[31;91H"), "cursor jumps to the cell");
    assert!(s.contains("a=t,f=100,i=4207"), "idle transmitted: {s:.60}");
    assert!(
        s.contains("a=p,i=4207,p=1,c=7,r=4,"),
        "idle placed: {s:.80}"
    );

    // Same state again: no bytes at all.
    let mut out = Vec::new();
    pet.sync(&mut out, Some((cell, false))).unwrap();
    assert!(out.is_empty(), "idempotent when unchanged");

    // The composer grows with a long draft: re-place only, no retransmit.
    let moved = Rect::new(90, 29, 7, 4);
    let mut out = Vec::new();
    pet.sync(&mut out, Some((moved, false))).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(!s.contains("a=t"), "a move must not retransmit: {s}");
    assert!(s.contains("a=p,i=4207,p=2,"), "new placement: {s}");
    assert!(s.contains("a=d,d=p,i=4207,p=1"), "old placement dropped: {s}");

    // A turn starts: the working sprite is transmitted once and placed; the
    // idle image keeps its data — only its placement is dropped.
    let mut out = Vec::new();
    pet.sync(&mut out, Some((moved, true))).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("a=t,f=100,i=4208"), "working transmitted: {s:.60}");
    assert!(s.contains("a=p,i=4208,p=1,"), "working placed: {s:.80}");
    assert!(s.contains("a=d,d=p,i=4207,p=2"), "idle placement dropped: {s}");
    assert!(!s.contains("a=d,d=I"), "image data is kept: {s}");

    // Hide (`/liang` off): both sprites deleted, transmission state reset.
    let mut out = Vec::new();
    pet.sync(&mut out, None).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(
        s.contains("a=d,d=I,i=4207") && s.contains("a=d,d=I,i=4208") && !s.contains("a=t"),
        "hidden: {s}"
    );

    // Re-show after a hide: the sprite is transmitted again.
    let mut out = Vec::new();
    pet.sync(&mut out, Some((cell, false))).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("a=t,f=100,i=4207"), "retransmit after hide: {s:.60}");
}

#[test]
fn disabled_pet_stays_silent() {
    let mut pet = Pet::new(false);
    let mut out = Vec::new();
    pet.sync(&mut out, Some((Rect::new(0, 0, 7, 4), true)))
        .unwrap();
    assert!(out.is_empty());
}

#[test]
fn thumbnails_scroll_by_replacement_without_retransmit() {
    let shot = |rect: Rect| ThumbShot {
        id: 7,
        rect,
        data: LIANG_IDLE_PNG,
    };
    let mut thumbs = Thumbnails::new();

    // First sight: transmit the PNG once, then place it as placement 1.
    let mut out = Vec::new();
    thumbs
        .sync(&mut out, &[shot(Rect::new(0, 0, 8, 3))])
        .unwrap();
    assert_eq!(image_dims(&transmitted_png(out.clone())), Some((192, 208)));
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("a=t,f=100,i=7,"), "transmit once: {s:.60}");
    assert!(s.contains("a=p,i=7,p=1,c=8,r=3,"), "initial place: {s:.80}");

    // Same viewport: no bytes at all.
    let mut out = Vec::new();
    thumbs
        .sync(&mut out, &[shot(Rect::new(0, 0, 8, 3))])
        .unwrap();
    assert!(out.is_empty(), "idempotent when unchanged");

    // Viewport scrolls: the already-transmitted image is re-placed (a few
    // bytes) — never deleted and re-sent — and the old placement is dropped
    // after the new one exists, so the thumbnail never blinks out.
    let mut out = Vec::new();
    thumbs
        .sync(&mut out, &[shot(Rect::new(0, 1, 8, 3))])
        .unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(!s.contains("a=t"), "a move must not retransmit: {s}");
    assert!(s.contains("a=p,i=7,p=2,c=8,r=3,"), "new placement: {s}");
    assert!(s.contains("a=d,d=p,i=7,p=1"), "old placement dropped: {s}");

    // Scrolled away entirely: the image and its placements are deleted.
    let mut out = Vec::new();
    thumbs.sync(&mut out, &[]).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("a=d,d=I,i=7"), "scrolled away: {s}");
}

#[test]
fn theme_background_is_placed_behind_text_and_retracted() {
    let file = std::env::temp_dir().join(format!(
        "dsh-tui-background-{}-{}.png",
        std::process::id(),
        1
    ));
    std::fs::write(&file, LIANG_IDLE_PNG).unwrap();
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
    let encoded = base64(LIANG_IDLE_PNG);
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
            base64: base64(LIANG_IDLE_PNG),
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
            base64: base64(LIANG_IDLE_PNG),
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
