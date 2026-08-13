#!/usr/bin/env python3
"""Generate the DeepSeek whale ASCII art from the harness Web UI favicon.

Parses the single <path d="..."> from assets_favicon.svg (the exact whale
mark shipped by the deepseek-harness Web UI), flattens the cubic béziers,
rasterizes with the even-odd rule on a supersampled grid, and emits
half-block (▀ ▄ █) character art at several widths into src/logo_data.rs.

Run:  python scripts/gen_logo.py
"""

from __future__ import annotations

import re
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
SVG = ROOT / "assets_favicon.svg"
OUT = ROOT / "src" / "logo_data.rs"

VIEWBOX = 50.0
GRID = 800  # rasterization resolution (pixels per side)
SUPERSAMPLE = 2


def parse_subpaths(d: str) -> list[list[tuple[float, float]]]:
    """Flatten an absolute/relative M/C/L/Z path into polyline subpaths."""
    seq: list[str | float] = []
    for letter, num in re.findall(r"([MCLZmclz])|(-?\d*\.?\d+(?:e-?\d+)?)", d):
        seq.append(letter if letter else float(num))

    subpaths: list[list[tuple[float, float]]] = []
    cur: list[tuple[float, float]] = []
    pos = (0.0, 0.0)
    cmd: str | None = None
    i = 0
    while i < len(seq):
        tok = seq[i]
        if isinstance(tok, str):
            cmd = tok
            i += 1
            if cmd in "Zz":
                if cur:
                    subpaths.append(cur)
                    cur = []
                cmd = None
            continue
        assert cmd is not None, "path data before any command"
        if cmd in "Mm":
            x, y = seq[i], seq[i + 1]  # type: ignore[assignment]
            i += 2
            if cmd == "m":
                x += pos[0]
                y += pos[1]
            if cur:
                subpaths.append(cur)
            cur = [(x, y)]
            pos = (x, y)
            cmd = "L" if cmd == "M" else "l"
        elif cmd in "Ll":
            x, y = seq[i], seq[i + 1]  # type: ignore[assignment]
            i += 2
            if cmd == "l":
                x += pos[0]
                y += pos[1]
            cur.append((x, y))
            pos = (x, y)
        elif cmd in "Cc":
            x1, y1, x2, y2, x3, y3 = seq[i : i + 6]  # type: ignore[assignment]
            i += 6
            if cmd == "c":
                x1 += pos[0]; y1 += pos[1]
                x2 += pos[0]; y2 += pos[1]
                x3 += pos[0]; y3 += pos[1]
            p0 = np.array(pos)
            p1, p2, p3 = np.array([x1, y1]), np.array([x2, y2]), np.array([x3, y3])
            for t in np.linspace(0.0, 1.0, 28)[1:]:
                pt = (1 - t) ** 3 * p0 + 3 * (1 - t) ** 2 * t * p1 + 3 * (1 - t) * t**2 * p2 + t**3 * p3
                cur.append((float(pt[0]), float(pt[1])))
            pos = (x3, y3)
        else:  # pragma: no cover
            raise ValueError(f"unhandled path command {cmd!r}")
    if cur:
        subpaths.append(cur)
    return subpaths


def rasterize(subpaths: list[list[tuple[float, float]]]) -> np.ndarray:
    """Even-odd scanline fill -> float coverage mask of shape (GRID, GRID)."""
    w = GRID * SUPERSAMPLE
    ys, xs = np.mgrid[0:w, 0:w]
    px = (xs + 0.5) * VIEWBOX / w
    py = (ys + 0.5) * VIEWBOX / w
    crossings = np.zeros((w, w), dtype=np.int32)
    for sp in subpaths:
        pts = np.array(sp + [sp[0]])
        for (ex0, ey0), (ex1, ey1) in zip(pts[:-1], pts[1:]):
            if ey0 == ey1:
                continue
            cond = (py >= min(ey0, ey1)) & (py < max(ey0, ey1))
            xint = ex0 + (py - ey0) * (ex1 - ex0) / (ey1 - ey0)
            crossings += (cond & (px < xint)).astype(np.int32)
    mask = (crossings % 2 == 1).astype(np.float64)
    return mask.reshape(GRID, SUPERSAMPLE, GRID, SUPERSAMPLE).mean(axis=(1, 3))


def halfblock_art(mask: np.ndarray, cols: int, thresh: float = 0.5) -> list[str]:
    """Render the mask as half-block art `cols` characters wide."""
    n = mask.shape[0]
    rows = max(1, round(cols * 0.5))
    h = rows * 2
    lines: list[str] = []
    for r in range(rows):
        line = []
        for c in range(cols):
            def cov(pr: int) -> float:
                y0 = int(pr * n / h)
                y1 = max(y0 + 1, int((pr + 1) * n / h))
                x0 = int(c * n / cols)
                x1 = max(x0 + 1, int((c + 1) * n / cols))
                return float(mask[y0:y1, x0:x1].mean())

            top, bot = cov(2 * r) > thresh, cov(2 * r + 1) > thresh
            line.append("█" if top and bot else "▀" if top else "▄" if bot else " ")
        lines.append("".join(line).rstrip())
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    return lines


def bbox_crop(mask: np.ndarray, eps: float = 0.02) -> np.ndarray:
    """Crop the mask to the whale's bounding box (for tiny variants)."""
    ys, xs = np.where(mask > eps)
    return mask[ys.min() : ys.max() + 1, xs.min() : xs.max() + 1]


def _cov(mask: np.ndarray, ph: int, pw: int) -> np.ndarray:
    """Area-average the mask down to a (ph, pw) coverage grid."""
    h, w = mask.shape
    out = np.zeros((ph, pw))
    for r in range(ph):
        y0, y1 = int(r * h / ph), max(int(r * h / ph) + 1, int((r + 1) * h / ph))
        for c in range(pw):
            x0, x1 = int(c * w / pw), max(int(c * w / pw) + 1, int((c + 1) * w / pw))
            out[r, c] = float(mask[y0:y1, x0:x1].mean())
    return out


def tiny_halfblock(mask: np.ndarray, rows: int, thresh: float = 0.4) -> list[str]:
    """Half-block art exactly `rows` tall, cropped to the whale bbox.

    Half-block subpixels are ~square, so width follows the bbox aspect.
    """
    crop = bbox_crop(mask)
    ph = rows * 2
    pw = max(1, round(ph * crop.shape[1] / crop.shape[0]))
    cov = _cov(crop, ph, pw) > thresh
    lines = []
    for r in range(rows):
        top, bot = cov[2 * r], cov[2 * r + 1]
        lines.append(
            "".join(
                "█" if t and b else "▀" if t else "▄" if b else " "
                for t, b in zip(top, bot)
            ).rstrip()
        )
    return lines


def tiny_braille(mask: np.ndarray, rows: int, thresh: float = 0.35) -> list[str]:
    """Braille art exactly `rows` tall (4 dot-rows per cell), bbox-cropped.

    Braille dots in a 1:2 cell are ~square, so width follows the aspect.
    """
    crop = bbox_crop(mask)
    ph = rows * 4
    pw = max(2, round(ph * crop.shape[1] / crop.shape[0]))
    if pw % 2:
        pw += 1
    cov = _cov(crop, ph, pw) > thresh
    bits = [(0, 0, 0x01), (1, 0, 0x02), (2, 0, 0x04), (0, 1, 0x08),
            (1, 1, 0x10), (2, 1, 0x20), (3, 0, 0x40), (3, 1, 0x80)]
    lines = []
    for r in range(rows):
        line = []
        for c in range(pw // 2):
            code = 0x2800
            for dr, dc, bit in bits:
                if cov[4 * r + dr, 2 * c + dc]:
                    code |= bit
            line.append(chr(code))
        lines.append("".join(line).rstrip("\u2800"))
    return lines


def main() -> None:
    d = re.search(r'\bd="([^"]+)"', SVG.read_text()).group(1)  # type: ignore[union-attr]
    subpaths = parse_subpaths(d)
    mask = rasterize(subpaths)

    variants = {
        "WHALE_XL": halfblock_art(mask, 52, thresh=0.42),
        "WHALE_LG": halfblock_art(mask, 40, thresh=0.45),
        "WHALE_MD": halfblock_art(mask, 26, thresh=0.45),
        "WHALE_SM": halfblock_art(mask, 16, thresh=0.5),
        # Composer-pet fallback for terminals without pixel graphics
        # (bbox-cropped for maximum detail at 3 rows).
        "WHALE_XS": tiny_halfblock(mask, 3, thresh=0.4),
    }

    body = [
        "// Generated by scripts/gen_logo.py from assets_favicon.svg — do not edit.",
        "",
    ]
    for name, lines in variants.items():
        body.append(f"pub const {name}: [&str; {len(lines)}] = [")
        for ln in lines:
            escaped = ln.replace("\\", "\\\\").replace('"', '\\"')
            body.append(f'    "{escaped}",')
        body.append("];")
        body.append("")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(body) + "\n")
    print(f"wrote {OUT}")
    for name, lines in variants.items():
        width = max((len(l) for l in lines), default=0)
        print(f"  {name}: {len(lines)} rows x {width} cols")


if __name__ == "__main__":
    main()
