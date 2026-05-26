#!/usr/bin/env python3
"""Render website/apple-touch-icon.png — the 180x180 home-screen icon.

A raster twin of the inline-SVG favicon in website/index.html: the TB
monogram in yellow on a CGA-blue field with a white inset border.
Full-bleed background (iOS/Android apply their own rounded-corner mask).
Regenerate via `just apple-touch-icon` (needs Pillow). Committed like
og-image.png; don't hand-edit the PNG.
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Palette — mirrors website/styles.css :root and the favicon SVG.
BG = (0, 0, 170)  # #0000aa
WHITE = (255, 255, 255)
YELLOW = (255, 255, 85)  # #ffff55

SIZE = 180
FONT_PATH = "/System/Library/Fonts/Menlo.ttc"


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    try:
        return ImageFont.truetype(FONT_PATH, size, index=1 if bold else 0)
    except OSError:
        return ImageFont.truetype(FONT_PATH, size)


def main() -> int:
    out = Path(__file__).resolve().parent.parent / "website" / "apple-touch-icon.png"

    img = Image.new("RGB", (SIZE, SIZE), BG)
    draw = ImageDraw.Draw(img)

    # White inset border (favicon SVG insets 4/64 of the side, stroke 2/64).
    inset = round(SIZE * 4 / 64)
    border = max(2, round(SIZE * 2 / 64))
    draw.rectangle(
        [inset, inset, SIZE - inset - 1, SIZE - inset - 1],
        outline=WHITE,
        width=border,
    )

    # TB monogram — yellow with a white outline, centered.
    text = "TB"
    fnt = font(96, bold=True)
    box = draw.textbbox((0, 0), text, font=fnt, stroke_width=4)
    tw, th = box[2] - box[0], box[3] - box[1]
    x = (SIZE - tw) / 2 - box[0]
    y = (SIZE - th) / 2 - box[1]
    draw.text((x, y), text, font=fnt, fill=YELLOW, stroke_width=4, stroke_fill=WHITE)

    img.save(out)
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
