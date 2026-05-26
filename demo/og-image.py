#!/usr/bin/env python3
"""Render website/og-image.png — the 1200x630 social card.

Turbo Vision look: CGA-blue dithered desktop, a bordered dialog with the
TURBO BIBLE wordmark, and the menu/status chrome from the landing page.
Regenerate via `just og-image` (needs Pillow). Committed like demo.gif;
don't hand-edit the PNG.

Fonts are vendored under demo/fonts/ so the render matches the site (and
is reproducible off-macOS): Silkscreen for the pixel wordmark and IBM
Plex Mono for the body/chrome — the same two families website/styles.css
loads from Google Fonts. Both are SIL OFL 1.1 (see the *-OFL.txt files).
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

FONT_DIR = Path(__file__).resolve().parent / "fonts"
PLEX_REGULAR = FONT_DIR / "IBMPlexMono-Regular.ttf"
PLEX_BOLD = FONT_DIR / "IBMPlexMono-Bold.ttf"
SILKSCREEN = FONT_DIR / "Silkscreen-Bold.ttf"


def plex(size: int, bold: bool = False) -> ImageFont.FreeTypeFont:
    """IBM Plex Mono — the chrome/body face (styles.css `--mono`)."""
    return ImageFont.truetype(str(PLEX_BOLD if bold else PLEX_REGULAR), size)


def silk(size: int) -> ImageFont.FreeTypeFont:
    """Silkscreen — the pixel wordmark face (styles.css `.hero-title`)."""
    return ImageFont.truetype(str(SILKSCREEN), size)


def centered(draw, text, fnt, cx, cy, **kw):
    """Draw `text` centered on (cx, cy)."""
    draw.text((cx, cy), text, font=fnt, anchor="mm", **kw)


def tab(draw, text, x, y, fnt, fill, anchor):
    """A title-bar tab embedded in the dialog border.

    Mirrors `.panel > .title` in styles.css: a solid background swatch
    that *breaks* the border line, with the label centered on it — so the
    white double-border doesn't run through the text.
    """
    tw = draw.textlength(text, font=fnt)
    pad = 10
    if anchor.startswith("r"):
        x0, x1 = x - tw - pad, x + pad
    else:
        x0, x1 = x - pad, x + tw + pad
    half = fnt.size * 0.72
    draw.rectangle([x0, y - half, x1, y + half], fill=BG)
    draw.text((x, y), text, font=fnt, fill=fill, anchor=anchor)


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

    bar_font = plex(20, bold=True)

    # Top menu bar.
    draw.rectangle([0, 0, W, BAR_H], fill=BAR)
    # Hamburger glyph, drawn as three bars — IBM Plex Mono has no U+2261
    # and Pillow (unlike the browser) won't fall back to a face that does.
    for i in range(3):
        ly = 15 + i * 7
        draw.rectangle([24, ly, 42, ly + 2], fill=BAR_DARK)
    menu = "Bible · Edit · Search · Translation · Heresy · Help"
    draw.text((58, 11), menu, font=bar_font, fill=BLACK)

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

    # Dialog title tab + version, embedded in (breaking) the top border.
    tab_font = plex(22, bold=True)
    tab(draw, "Turbo Bible", dx0 + 34, dy0, tab_font, WHITE, "lm")
    tab(draw, "v0.1", dx1 - 34, dy0, tab_font, YELLOW, "rm")

    # Wordmark — Silkscreen, fit to the dialog width, yellow + white outline.
    title = "TURBO BIBLE"
    size = 132
    title_font = silk(size)
    while draw.textlength(title, font=title_font) > (dx1 - dx0) - 130 and size > 40:
        size -= 2
        title_font = silk(size)
    centered(draw, title, title_font, cx, dy0 + 150, fill=YELLOW,
             stroke_width=2, stroke_fill=WHITE)

    centered(draw, "The Bible. In your terminal.", plex(34, bold=True),
             cx, dy0 + 278, fill=WHITE)
    centered(draw, "// like god intended.", plex(24), cx, dy0 + 318, fill=CYAN)

    # Install command pill.
    cmd_font = plex(26, bold=True)
    cmd = "$ curl -fsSL turbo.bible/install.sh | sh"
    cw = draw.textlength(cmd, font=cmd_font)
    bx0 = cx - cw / 2 - 24
    by0 = dy0 + 350
    draw.rectangle([bx0, by0, cx + cw / 2 + 24, by0 + 50],
                   fill=BG_DARK, outline=BAR_DARK, width=1)
    tx = cx - cw / 2
    draw.text((tx, by0 + 11), "$", font=cmd_font, fill=YELLOW)
    draw.text((tx + draw.textlength("$ ", font=cmd_font), by0 + 11),
              cmd[2:], font=cmd_font, fill=WHITE)

    centered(draw, "11 translations · 7 languages · offline · zero telemetry",
             plex(20), cx, dy1 - 26, fill=GREY)

    img.save(out)
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
