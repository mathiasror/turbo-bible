# TUI-specific rules (turbo-bible / Turbo Vision)

These rules **override** the general rubric where they conflict. They encode
turbo-bible's house style and irrevocable design decisions.

## Palette discipline

The palette is CGA-derived and lives in `crates/turbo-bible-tui/src/theme.rs`.
Every color on screen should be one of the named constants:

```
blue, cyan, mid_cyan, bright_cyan, teal, input_teal,
bright_white, light_grey, dark_grey,
yellow, hotkey_red, black
```

Any off-palette color in a screenshot is a finding (Major). 24-bit RGB hex is
how they're expressed, but the *roles* are fixed.

### Color roles

| Color          | Role                                                   |
| -------------- | ------------------------------------------------------ |
| `blue`         | Desktop background (the canonical Turbo Vision blue)   |
| `cyan`         | List focus, dialog body fill                           |
| `mid_cyan`     | Structural labels — sidebar headers, dialog titles     |
| `bright_cyan`  | Visual-mode selection highlight                        |
| `teal`         | Cursor row background                                  |
| `input_teal`   | Text input wells (Goto, Find)                          |
| `bright_white` | Primary body text                                      |
| `light_grey`   | Muted secondary text                                   |
| `dark_grey`    | Drop shadows; disabled state                           |
| `yellow`       | **Verse numbers + mode pills (NORMAL/VISUAL) only.**   |
| `hotkey_red`   | Single-char hotkey letters in menus and dialog buttons |
| `black`        | Frame interiors, text on light fills                   |

### The yellow-slot rule

**Yellow is reserved for verse numbers in the scripture pane, the
NORMAL/VISUAL/FILTER mode pill in the status bar, and — inside an input
dialog only — the single *operative token*: the one answer/highlight the
dialog exists to surface.** That operative-token slot is exactly three
shipped uses: Goto's resolved reference in the "Will jump to: …" preview,
Find's matched search-term highlight inside result snippets, and the
active keycaps in the Help (`:help`) dialog. Nothing else gets yellow.

The boundary is sharp: yellow marks the *operative token*, never
list/content/structural elements. A dialog's cross-reference entries,
list rows, section headers, titles, hints, and status copy are content —
a list of items is content, never "the operative token," even in a
dialog — and use cyan tiers (`mid_cyan` for structural labels/headers,
`teal` for navigable entries; e.g. the `K` footnote/xref popup's header
is `mid_cyan` and its xref entries are `teal`, no underline, matching the
sidebar). Sidebar headers, dialog titles, status hints, and daily-quote
attribution likewise use cyan tiers. Yellow used outside the verse pane,
the mode pill, or the operative-token slot collapses the verse-number
signal and is always a finding (severity Major when on a real surface,
Blocker if it touches the verse pane itself).

This rule is repeated in `MEMORY.md → Yellow slot reserved for scripture
pane` because it's that important.

## The `▒` glyph

The Turbo Vision aesthetic depends on:

- **24-bit RGB** color depth.
- **The `▒` half-tone block** for dithered fills (drop shadows, desktop).

If a screenshot was captured in macOS `Terminal.app` or another terminal
without 24-bit support, the `▒` will render as solid blocks or `?` —
note it as a capture-environment issue, not a UI bug. Ask the user to
recapture in iTerm2, Ghostty, Alacritty, WezTerm, or Kitty.

## The grid

Every visual element snaps to character cells:

- Box-drawing frames use `┌─┐│└┘` (or their double-line variants for
  dialog frames). Don't mix Unicode and ASCII frames in one surface.
- Unicode width: assume single-width Latin. Double-width CJK breaks
  alignment — flag any place that doesn't reserve room for it.
- Off-by-one is visible. Frame corners landing on a column boundary
  rather than the cell-center boundary will look "wrong."

## Width thresholds

| Width (cols) | Behavior                                                      |
| ------------ | ------------------------------------------------------------- |
| `< 80`       | Cramped; reading pane usable but tight. Sidebar hidden.       |
| `80–119`     | Default. Reading pane at `max_width = 80`. Sidebar hidden.    |
| `≥ 120`      | References sidebar appears auto (auto-follows cursor verse).  |

The reading pane never exceeds `[reading] max_width` (default 80) — this
keeps line length in the 45–75 char body-text sweet spot (rubric §3).

Compare panes refuse to open if the terminal is too narrow to keep each
column readable. That refusal *is* the right answer; don't suggest
shrinking columns instead.

## Focus & state

- **Focused pane:** bright `cyan` border + visible `NORMAL`/`VISUAL`
  pill in the status bar.
- **Unfocused pane (compare mode, ≥2 panes):** dimmed border, no pill.
- **Cursor row:** `teal` background fill across the line.
- **Visual-mode selection:** `bright_cyan` highlight on the selected
  range. Inverts foreground if needed for contrast.
- **Drop shadow:** every dialog and popup carries a `dark_grey` shadow
  offset down-right by one cell (`theme.rs::drop_shadow`). Missing
  shadow on a dialog = finding.

## Mode pills

- Only `NORMAL` and `VISUAL` exist. No INSERT, no COMMAND.
- Pill lives bottom-right in the status bar.
- Yellow on `dark_grey` fill.

## Menu bar (top)

- Bottom edge of the desktop, top of the screen.
- Single-char chord letters rendered in `hotkey_red` (e.g., **F**ile,
  **E**dit, **V**iew). The red letter is the active hotkey; the rest
  of the word is body color.

## Status bar (bottom)

- Left: transient hints (current chord, last error, search query).
- Right: mode pill + position indicator (`Genesis 1:1` style).
- Don't crowd the middle.

## Pre-decided items — do not re-raise

Cross-check `MEMORY.md → UI review backlog` before writing findings.
Items already in **Bucket A** (shipped) or with logged decisions in
**Bucket B/C** are not new findings — at most, acknowledge them in
context. Re-raising decided items wastes the reviewer's time.

Examples of things commonly mis-flagged that are intentional:

- The `▒` desktop fill is the Turbo Vision look, not visual noise.
- The 80-col reading pane cap is intentional (line-length sweet spot).
- The drop shadow on every dialog is intentional (Turbo Vision DNA).
- Yellow verse numbers in front of body text is the *whole point* of
  the yellow-slot rule; don't flag yellow on verse numbers.
- The references sidebar disappearing under 120 cols is intentional.

## TUI-specific things to look for

These don't have direct web/desktop analogues:

- **Trailing-cell rot.** A widget that draws its background but not its
  border across the full width leaves a single-column gap at the right
  edge. Visible on any solid-fill surface.
- **Half-cell glyphs.** Mixing `▌▐▀▄` half-blocks with full-cell elements
  on the same row creates micro-misalignments.
- **Cursor terminator.** When the rendered chapter is shorter than the
  pane, what fills the empty rows? `blue` desktop fill? `black`? A
  fade? Pick one, do it consistently.
- **Resize gracefully.** What happens when the terminal narrows past
  120 → 100 → 80? The sidebar should disappear cleanly, not get
  truncated mid-character.
