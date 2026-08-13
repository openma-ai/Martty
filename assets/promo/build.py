#!/usr/bin/env python3
"""Compose assets/promo/social-preview.png (2560x1280, GitHub 2:1).

Seamless single-terminal narrative: the brand banner and a real plugin-mode
turn share one terminal background, so interior crops pasted on a canvas of
the same color read as one tall terminal — whale and wordmark above, a live
turn and the composer bar below. No borders, no re-rendering: the product's
own pixels.

Sources (committed):
  assets/screenshots/banner.jpg       whale + DEEPSEEK HARNESS + slogan
  assets/screenshots/plugin-turn.jpg  a real dsh plugin-mode turn (v4-pro)
"""

from PIL import Image, ImageDraw, ImageFont

BG = (15, 16, 21)            # the screenshots' terminal background (JPEG-true)
INK_DIM = (97, 102, 107)     # BLUISH_700 — whale belly / captions
TICK = (32, 34, 39)          # corner ticks, barely-there
W, H = 2560, 1280
S = 0.86                     # one scale for every crop → consistent type size


def window(path: str) -> Image.Image:
    """Crop the dark terminal window out of the light desktop frame."""
    im = Image.open(path).convert("RGB")
    bbox = im.convert("L").point(lambda v: 255 if v < 60 else 0).getbbox()
    return im.crop(bbox)


def quiet(im: Image.Image, tol: int = 7) -> Image.Image:
    """Flatten JPEG mottle: anything within `tol` of BG becomes exact BG."""
    px = im.load()
    for y in range(im.height):
        for x in range(im.width):
            p = px[x, y]
            if (abs(p[0] - BG[0]) <= tol and abs(p[1] - BG[1]) <= tol
                    and abs(p[2] - BG[2]) <= tol):
                px[x, y] = BG
    return im


def scaled(im: Image.Image, box: tuple[int, int, int, int]) -> Image.Image:
    part = quiet(im.crop(box))
    return part.resize(
        (round(part.width * S), round(part.height * S)), Image.LANCZOS
    )


def main() -> None:
    banner = window("assets/screenshots/banner.jpg")        # 1569x1224
    turn_src = window("assets/screenshots/plugin-turn.jpg")  # 1579x1223

    brand = scaled(banner, (30, 24, 1544, 752))     # whale + wordmark + slogan
    turn = scaled(turn_src, (8, 18, 1571, 518))     # prompt → first read call
    bar = scaled(turn_src, (8, 1054, 1571, 1206))  # tip + streaming composer

    canvas = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(canvas)

    # vertical rhythm: brand breathes, the log sits tight above the bar
    y_brand, y_turn = 36, 686
    y_bar = y_turn + turn.height + 12
    x_log = (W - bar.width) // 2                    # turn/bar share one origin
    canvas.paste(brand, ((W - brand.width) // 2, y_brand))
    canvas.paste(turn, (x_log, y_turn))

    # the composer band bleeds edge-to-edge and down to the bottom, so the
    # whole plate reads as one terminal whose footer is the canvas footer.
    # The band is multi-toned and its edge columns may carry glyph pixels,
    # so continue each row sideways with that row's dominant (background)
    # color rather than a sampled column or one flat rectangle.
    def row_bg(y: int) -> tuple[int, int, int]:
        row = bar.crop((0, y, bar.width, y + 1))
        return max(row.getcolors(bar.width), key=lambda c: c[0])[1]

    ch = bar.height
    band_top = next(y for y in range(ch) if row_bg(y) != BG)
    for y in range(band_top, ch):
        c = row_bg(y)
        draw.line([(0, y_bar + y), (x_log, y_bar + y)], fill=c)
        draw.line([(x_log + bar.width, y_bar + y), (W, y_bar + y)], fill=c)
    draw.rectangle([0, y_bar + ch, W, H], fill=row_bg(ch - 6))
    canvas.paste(bar, (x_log, y_bar))

    # whisper line: the address, set in the terminal's own mono — sitting in
    # the footer band like one more status hint
    font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 22, index=0)
    url = "github.com/openma-ai/deepseek-harness-tui"
    tw = draw.textlength(url, font=font)
    draw.text((W - tw - 64, H - 40), url, font=font, fill=INK_DIM)

    # clinical corner ticks — calibration marks on the instrument plate
    # (top corners only; the footer band owns the bottom edge)
    m, t = 34, 26
    for cx, cy, dx in ((m, m, 1), (W - m, m, -1)):
        draw.line([(cx, cy), (cx + dx * t, cy)], fill=TICK, width=2)
        draw.line([(cx, cy), (cx, cy + t)], fill=TICK, width=2)

    canvas.save("assets/promo/social-preview.png", optimize=True)
    print("wrote assets/promo/social-preview.png", canvas.size)


if __name__ == "__main__":
    main()
