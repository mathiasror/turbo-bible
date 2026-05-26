#!/usr/bin/env python3
"""Render website/og-image.png — the 1200x630 social card.

Turbo Vision look: CGA-blue dithered desktop, a bordered dialog with the
TURBO BIBLE wordmark, and the menu/status chrome from the landing page.
Regenerate via `just og-image` (needs Pillow). Committed like demo.gif;
don't hand-edit the PNG.
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Palette — mirrors website/styles.css :root.
BG = (0, 0, 170)  # #0000aa
BG_SOFT = (20, 20, 184)  # #1414b8
BG_DARK = (0, 0, 122)  # #00007a
BLACK = (0, 0, 0)
WHITE = (255, 255, 255)
YELLOW = (255, 255, 85)  # #ffff55
CYAN = (0, 212, 212)  # #00d4d4
GREY = (170, 170, 170)  # #aaaaaa
BAR = (196, 196, 196)  # #c4c4c4
BAR_DARK = (128, 128, 128)  # #808080

W, H = 1200, 630
BAR_H = 44

FONT_PATH = "/System/Library/Fonts/Menlo.ttc"


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    try:
        return ImageFont.truetype(FONT_PATH, size, index=1 if bold else 0)
    except OSError:
        return ImageFont.truetype(FONT_PATH, size)


def centered(draw, text, fnt, cx, y, **kw):
    w = draw.textlength(text, font=fnt)
    draw.text((cx - w / 2, y), text, font=fnt, **kw)
    return w


def main() -> int:
    out = Path(__file__).resolve().parent.parent / "website" / "og-image.png"

    img = Image.new("RGB", (W, H), BG)
    px = img.load()
    # 2px checkerboard, like the repeating-conic desktop dither.
    for y in range(H):
        for x in range(W):
            if ((x // 2) + (y // 2)) % 2:
                px[x, y] = BG_SOFT
    draw = ImageDraw.Draw(img)

    bar_font = font(20, bold=True)

    # Top menu bar.
    draw.rectangle([0, 0, W, BAR_H], fill=BAR)
    draw.text((24, 11), "≡", font=bar_font, fill=BAR_DARK)
    menu = "  Bible · Edit · Search · Translation · Heresy · Help"
    draw.text((24 + draw.textlength("≡", font=bar_font), 11), menu,
              font=bar_font, fill=BLACK)

    # Bottom status bar.
    draw.rectangle([0, H - BAR_H, W, H], fill=BAR)
    draw.text((24, H - BAR_H + 11),
              "Enter Open    F2 Goto    F3 Find    Esc Quit",
              font=bar_font, fill=BLACK)
    mode = "-- NORMAL --"
    draw.text((W - 24 - draw.textlength(mode, font=bar_font), H - BAR_H + 11),
              mode, font=bar_font, fill=BLACK)

    # Centered dialog with a drop shadow + double white border.
    dx0, dy0, dx1, dy1 = 110, 96, W - 110, H - 96
    draw.rectangle([dx0 + 12, dy0 + 12, dx1 + 12, dy1 + 12], fill=BG_DARK)
    draw.rectangle([dx0, dy0, dx1, dy1], fill=BG, outline=WHITE, width=3)
    draw.rectangle([dx0 + 8, dy0 + 8, dx1 - 8, dy1 - 8], outline=WHITE, width=1)
    cx = (dx0 + dx1) / 2

    # Dialog tab + version.
    tab_font = font(22, bold=True)
    draw.text((dx0 + 28, dy0 - 4), " Turbo Bible ", font=tab_font, fill=WHITE)
    draw.text((dx1 - 28 - draw.textlength("v0.1", font=tab_font), dy0 - 4),
              "v0.1", font=tab_font, fill=YELLOW)

    # Wordmark — fit to the dialog width, yellow with a white outline.
    title = "TURBO BIBLE"
    size = 150
    title_font = font(size, bold=True)
    while draw.textlength(title, font=title_font) > (dx1 - dx0) - 130 and size > 40:
        size -= 2
        title_font = font(size, bold=True)
    centered(draw, title, title_font, cx, dy0 + 96, fill=YELLOW,
             stroke_width=2, stroke_fill=WHITE)

    centered(draw, "The Bible. In your terminal.", font(34, bold=True),
             cx, dy0 + 262, fill=WHITE)
    centered(draw, "// like god intended.", font(24), cx, dy0 + 306, fill=CYAN)

    # Install command pill.
    cmd_font = font(26, bold=True)
    cmd = "$ curl -fsSL turbo.bible/install.sh | sh"
    cw = draw.textlength(cmd, font=cmd_font)
    bx0 = cx - cw / 2 - 24
    by0 = dy0 + 346
    draw.rectangle([bx0, by0, cx + cw / 2 + 24, by0 + 50],
                   fill=BG_DARK, outline=BAR_DARK, width=1)
    tx = cx - cw / 2
    draw.text((tx, by0 + 11), "$", font=cmd_font, fill=YELLOW)
    draw.text((tx + draw.textlength("$ ", font=cmd_font), by0 + 11),
              cmd[2:], font=cmd_font, fill=WHITE)

    centered(draw, "11 translations · 7 languages · offline · zero telemetry",
             font(20), cx, dy1 - 32, fill=GREY)

    img.save(out)
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
