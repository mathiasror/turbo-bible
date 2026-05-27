---
name: regen-assets
description: Regenerate the project's generated marketing/doc images from their VHS .tape sources — the demo GIF, the labelled screenshots, the OpenGraph social card, and the home-screen icon. Use when the user asks to "regenerate the demo gif", "update the screenshots", "refresh the og-image", "re-record the demo", or says the splash / reading view / theme changed and the captures are now stale. Do not hand-edit the generated images; edit the .tape source and re-run.
---

# Regenerate generated assets (VHS)

The images under `demo/`, `docs/screenshots/`, and `website/` are **generated
artifacts**, not source. The `.tape` files in `demo/` are the source of truth.
Hand-editing a `.gif`/`.png` is always wrong — change the tape and re-render.

## Prerequisites

- `vhs` (https://github.com/charmbracelet/vhs) on PATH. Check with
  `command -v vhs`; if missing, tell the user to `brew install vhs` rather
  than guessing.
- Each recipe builds `cargo build -p turbo-bible --release` first, so the
  capture reflects current code.
- The Turbo Vision look needs 24-bit RGB + the `▒` glyph; eyeball every output
  in a modern terminal before committing (iTerm2, Ghostty, Alacritty, WezTerm).

## The recipes

| Recipe | Source tape | Output |
| --- | --- | --- |
| `just demo` | `demo/demo.tape` | `demo/demo.gif` (README hero) |
| `just screenshots` | `demo/screenshots.tape` | `docs/screenshots/*.png` (labelled tour) |
| `just og-image` | `demo/og-image.tape` | `website/og-image.png` (1200×630 social card) |
| `just apple-touch-icon` | `demo/apple-touch-icon.py` | `website/apple-touch-icon.png` (180×180; **Pillow**, not VHS) |

## When to regenerate (from the release gate)

Per `/release-checklist` §7, regenerate before tagging when the relevant
surface changed since the last tag:

- **reading/render or splash changed** → `just demo` **and** `just screenshots`.
  Stale captures ship a wrong first impression.
- **splash or social copy changed** → `just og-image`.
- **theme/palette changed** → re-render whatever shows the affected colors, and
  remember yellow is reserved for verse numbers + mode pills only (sidebar and
  dialogs use the cyan tiers) — sanity-check the capture against that.

## After rendering

1. Open the output and confirm it looks right (don't trust the exit code alone
   for a visual artifact).
2. Commit the regenerated binary file(s) alongside the code change that made
   them stale — never in a drive-by "update assets" commit divorced from the
   change.
