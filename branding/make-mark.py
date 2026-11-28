#!/usr/bin/env python3
"""Builds the launcher's in-app mark (vu-mark-*.png) from vu-icon.png.

The icon is a dark rounded tile with the VU glyph on it. Inside the launcher the tile shows up as a
dark box on a gray window, so the launcher uses the glyph alone on a transparent background: the tile
colour is turned into transparency ("colour to alpha"), the glyph is cropped tightly and resized with
premultiplied alpha. The light-theme variant (dark letters) is derived in the launcher at startup.

    python3 branding/make-mark.py        (needs Pillow and numpy)
"""
from pathlib import Path
import numpy as np
from PIL import Image

HERE = Path(__file__).parent
TILE = np.array([18.0, 20.0, 24.0])  # the icon's tile colour
SIZES = {"vu-mark-small.png": 64, "vu-mark-large.png": 192}  # glyph heights in pixels


def color_to_alpha(rgb: np.ndarray) -> np.ndarray:
    c = rgb.astype(float)
    up = np.where(c > TILE, (c - TILE) / (255.0 - TILE), (TILE - c) / TILE)
    alpha = np.clip(up.max(axis=2), 0.0, 1.0)
    safe = np.maximum(alpha, 1e-6)[..., None]
    fg = np.clip(TILE + (c - TILE) / safe, 0, 255)
    return np.dstack([fg, alpha * 255.0]).round().astype(np.uint8)


def main() -> None:
    icon = np.array(Image.open(HERE / "vu-icon.png").convert("RGBA"))
    out = color_to_alpha(icon[:, :, :3])
    # Only where the tile is opaque; outside it (the rounded corners) stays transparent.
    out[:, :, 3] = np.where(icon[:, :, 3] > 200, out[:, :, 3], 0)
    ys, xs = np.where(out[:, :, 3] > 24)
    # The tile's own edge is brighter than its face; keep to the glyph inside the tile.
    inner = (xs > 120) & (xs < 904) & (ys > 120) & (ys < 904)
    x0, x1, y0, y1 = xs[inner].min(), xs[inner].max(), ys[inner].min(), ys[inner].max()
    glyph = Image.fromarray(out[y0 : y1 + 1, x0 : x1 + 1], "RGBA")
    print("glyph box", x0, x1, y0, y1, glyph.size)
    for name, height in SIZES.items():
        width = round(glyph.width * height / glyph.height)
        small = glyph.convert("RGBa").resize((width, height), Image.LANCZOS).convert("RGBA")
        small.save(HERE / name, optimize=True)
        print(name, small.size)


if __name__ == "__main__":
    main()
