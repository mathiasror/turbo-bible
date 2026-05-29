# Screenshot index

Generated UI captures of every distinct screen and dialog turbo-bible
renders. Source is `demo/screenshots.tape` (VHS); regenerate with:

```sh
just screenshots
```

This file is **the per-shot context** the `ui-review` skill reads first
when auditing the captures. Each entry names the surface, the state being
exercised, and the intentional decisions a reviewer should **not** flag
as regressions (palette discipline, dither glyph, structural chrome).
When a shot changes its meaning, update its entry here in the same
commit that touches the tape.

## Tour order (the tape's flow)

The tape walks the app in this order. Shots 16–21 are gap-fills inserted
into the original 15-shot tour at their logical insertion points
(filenames sort to the end, but they don't sit at the end of the
narrative).

1. `01-splash.png` — splash, NORMAL
2. `02-splash-filter.png` — splash, FILTER (`jo` matching)
3. `16-splash-filter-empty.png` — splash, FILTER (no matches)
4. `03-reading.png` — reading view, sidebar visible
5. `17-reading-no-sidebar.png` — reading view, sidebar toggled off
6. `04-goto.png` — Goto dialog, mid-input
7. `18-goto-prefilled.png` — Goto dialog, just-opened (prefilled)
8. `05-find.png` — Find dialog, with results
9. `19-find-no-matches.png` — Find dialog, empty state
10. `20-bookmarks-empty.png` — Bookmarks dialog, empty state
11. `06-bookmarks.png` — Bookmarks dialog, populated
12. `07-translations.png` — Translations picker
13. `08-visual.png` — reading, VISUAL selection
14. `09-help.png` — Help overlay
15. `10-compare.png` — 2 compare panes
16. `11-compare-three.png` — 3 compare panes
17. `21-footnote.png` — footnote / cross-reference popup (`K`)
18. `12-xref-split.png` — cross-reference opened in a split (`K` → `s`)
19. `13-poetry.png` — reading, poetry indent (Psalm 119)
20. `14-poetry-compare.png` — poetry in a compare pane
21. `15-poetry-visual.png` — poetry, VISUAL selection across verses

## Project-wide intentional decisions

These hold across every shot — flag them only if they're broken, not
present:

- **CGA palette discipline.** Every color on screen maps to a named
  constant in `crates/turbo-bible-tui/src/theme.rs`.
- **Yellow-slot rule.** Yellow appears on verse numbers, the
  `NORMAL`/`VISUAL`/`FILTER` mode pill, and the single *operative token*
  inside an input dialog (Goto's resolved reference, Find's matched-term
  highlight, Help's active keycaps) — never on list/content/structural
  elements. Sidebar headers, dialog titles, hint copy, and dialog list
  rows or xref entries are content: all cyan tiers.
- **▒ dither glyph.** Used for drop shadows and the desktop fill. Not
  visual noise — it's the Turbo Vision DNA.
- **Drop shadow on every dialog.** Two-cell offset down-right, dark grey.
- **Reading pane capped at 80 cols.** Default `[reading] max_width`.
  Keeps line length in the 45–75 char sweet spot.
- **References sidebar threshold is 120 cols.** Below that it's
  auto-hidden (and Tab toggles it in single-pane mode at any width).
- **Existing UI-review backlog.** See `MEMORY.md → UI review backlog`
  before raising new findings — items in Bucket A shipped, B/C have
  logged decisions.

---

## Splash

### `01-splash.png` — Splash screen, NORMAL

- **Surface:** `crates/turbo-bible-tui/src/ui/splash.rs`
- **State:** Fresh `XDG_CONFIG_HOME` → no Continue row at the top of the
  book list, daily-quote enabled, en-kjv active, NORMAL mode pill.
- **Intentional:**
  - ANSI-Shadow `TURBO` / `BIBLE` 5-row title art at the top (a
    `TURBO BIBLE` compact fallback exists for very short terminals).
  - Two-column picker: OT (39 books) left, NT (27 books) right.
  - Daily quote ("verse of the day") between title and picker.
  - `hotkey_red` first-letter accents on the testament headings.
- **Related:** `02-splash-filter.png`, `16-splash-filter-empty.png`.

### `02-splash-filter.png` — Splash, FILTER mode (matching)

- **Surface:** same; FILTER sub-mode of `SplashView`.
- **State:** `/` pressed → filter input visible, filter `jo` typed →
  John, Joshua, Job narrow the columns.
- **Intentional:**
  - `-- FILTER --` pill in the status bar (yellow on dark-grey —
    same treatment as NORMAL / VISUAL; mode pills are one of the
    sanctioned yellow uses per the yellow-slot rule).
  - Filter input field rendered as a sunken well at the top of the
    picker area.

### `16-splash-filter-empty.png` — Splash, FILTER mode (no matches)

- **Surface:** same; FILTER sub-mode with zero-match column state.
- **State:** Filter `xyzzy` typed; both OT and NT columns render empty.
- **Watch for:** the zero-result rendering — is the empty area visibly
  framed, or does it read as broken? An "(no books match)" hint would
  be a feature request, not a regression — the current rendering is
  intentional silence.

---

## Reading view

### `03-reading.png` — Reading, narrative + sidebar

- **Surface:** `crates/turbo-bible-tui/src/ui/passage.rs` (reading pane)
  + `crates/turbo-bible-tui/src/ui/sidebar.rs` (right rail).
- **State:** en-kjv, John 3:16, terminal ≥120 cols → sidebar auto-visible.
- **Intentional:**
  - Yellow verse numbers; bright-white body text.
  - Teal cursor row band across the line at the cursor verse.
  - `bright_cyan` filled title bar for the single focused pane.
  - Sidebar headers in `mid_cyan`.
- **Related:** `17-reading-no-sidebar.png` (same passage, sidebar off);
  `08-visual.png` (VISUAL selection); `13-poetry.png` (poetry layout).

### `17-reading-no-sidebar.png` — Reading, sidebar toggled off

- **Surface:** same as 03, sidebar hidden by Tab toggle.
- **State:** en-kjv, John 3:16; user pressed Tab in single-pane mode.
- **Intentional:**
  - Reading pane re-centers in the freed body width — still capped at
    `max_reading_width` (80 cols by default).
  - Sidebar's column is now blue desktop fill (`▒` dither).
- **Watch for:** centering. The pane should optically center in the
  freed body, not snap to the left edge.

### `08-visual.png` — Reading, VISUAL selection (narrative)

- **Surface:** same as 03; selection range carried by the pane.
- **State:** Selection from current verse spans four verses downward
  (`v` then 3× `Down`).
- **Intentional:**
  - `bright_cyan` highlight band across the selected verses.
  - VISUAL pill rendered yellow (high-contrast, deliberately louder than
    NORMAL/FILTER — see `statusbar.rs::render`).
  - Footer hints read `Copy  Bookmark  Cancel` — a single exit verb
    (PR #28 collapsed the prior `Exit` / `Cancel` duplicate pair).
- **Related:** `15-poetry-visual.png` (same gesture across poetry).

### `13-poetry.png` — Reading, poetry indent (Psalm 119)

- **Surface:** same as 03; rendering for known poetic passages.
- **State:** en-kjv, Psalm 119:10. Cursor on `119:10` (two-digit verse
  number so the indent doesn't read as a numbering artifact).
- **Intentional:**
  - Whole-verse left inset (3 cols) on every verse, distinguishing
    poetry from the flush-left narrative prose of shot 03.
  - Verse numbers still yellow, still in the prose body — the indent is
    the *body*, not the number.
- **Watch for:** does the poetry inset survive the cursor row's teal
  band? It should — the band fills the row, the indent shapes the body.

### `15-poetry-visual.png` — Poetry + VISUAL selection

- **Surface:** same as 13; VISUAL selection spans four indented verses.
- **State:** Psalm 119:10–13 selected (`v` + 3× `Down`).
- **Watch for:** the selection band must form a clean rectangle across
  the indented verse bodies — not a ragged left edge that ghosts the
  cursor's pre-poetry position.

---

## Dialogs

### `04-goto.png` — Goto dialog, mid-input

- **Surface:** `crates/turbo-bible-tui/src/ui/goto.rs`.
- **State:** Goto opened on John 3:16, user typed `Genesis 1:1` (the
  first keystroke cleared the prefill — see `goto.rs::handle`).
- **Intentional:**
  - "Will jump to: Genesis 1:1" preview line below the input (yellow,
    bold). The preview is the live parse result.
  - Footer hint: `Enter jump   Esc cancel`.
- **Related:** `18-goto-prefilled.png`.

### `18-goto-prefilled.png` — Goto dialog, just-opened

- **Surface:** same as 04.
- **State:** Goto opened from John 3:16, **before** any keystroke. The
  input is prefilled with the current reference; the preview line shows
  the prefill-only hint.
- **Intentional:**
  - Distinct preview copy: `(type a reference, or Enter to stay here)`
    — different from the parse-result preview in shot 04.
  - The `prefilled` flag changes the next-character behavior (clear +
    replace), but that's invisible until the user types.

### `05-find.png` — Find dialog, with results

- **Surface:** `crates/turbo-bible-tui/src/ui/find.rs`.
- **State:** Find opened, query `love`, third hit selected (two `Down`
  presses).
- **Intentional:**
  - Yellow match highlights inside the BM25-ranked snippet rows.
  - `bright_cyan` selection slab on the focused hit; reference row in
    cyan, snippet rows indented.
  - Right-aligned `3 of 50` position readout in the footer.

### `19-find-no-matches.png` — Find dialog, no matches

- **Surface:** same as 05.
- **State:** Query `xyzzy` (no verse contains it).
- **Intentional:**
  - Italic `(no matches)` line in `light_grey` where hits would render.
  - Footer drops the position readout (no results to count).
- **Watch for:** the empty-state line is intentionally muted — flagging
  it for "low contrast" misses that it's *meant* to be quiet.

### `21-footnote.png` — Footnote / cross-reference popup

- **Surface:** `crates/turbo-bible-tui/src/ui/footnote.rs`.
- **State:** `K` pressed on John 3:16 → popup open with the verse's
  cross-references listed; nothing pressed yet.
- **Intentional:**
  - Selectable vertical list of cross-references; ↑/↓ navigates,
    `Enter` follows in place, `s` opens in a split (see shot 12).
  - Footnote bodies (if any) render above the xref list.
  - Dialog drop shadow over whatever's underneath.

### `20-bookmarks-empty.png` — Bookmarks dialog, empty

- **Surface:** `crates/turbo-bible-tui/src/ui/bookmarks.rs`.
- **State:** Bookmarks opened before any have been added.
- **Intentional:**
  - Compact 3-row dialog body (`content_h = 3` — see
    `bookmarks.rs::render`).
  - Title row reads `Bookmarks`; the subtitle row reads
    `no bookmarks yet — press b on a verse to add` (the count moved
    to the subtitle as part of PR #30's header dedupe).

### `06-bookmarks.png` — Bookmarks dialog, populated

- **Surface:** same as 20.
- **State:** Three bookmarks seeded across three books (Psalms 23:1,
  John 3:16, Romans 8:28) in canon order — demonstrates cross-book
  sorting rather than a single-passage demo dataset.
- **Intentional:**
  - Title row reads `Bookmarks`; subtitle row carries the count
    (`3 saved verses`).
  - Each cell is a reference row (cyan) + a preview row of the verse
    text (light grey unless selected).
  - Selected cell uses the `list_focus_bg` slab across both rows;
    preview within the slab is white but not bold so the reference
    stays the louder line.

### `07-translations.png` — Translations picker

- **Surface:** `crates/turbo-bible-tui/src/ui/translations.rs`.
- **State:** Picker opened. en-kjv is the active translation; the
  cursor lands on its row.
- **Intentional:**
  - All 11 bundled translations listed; the focused row gets the
    selection slab.
  - Bundled vs. fetchable translations may render differently if a
    download is required to switch — the picker handles both paths.

### `09-help.png` — Help overlay

- **Surface:** `crates/turbo-bible-tui/src/ui/help.rs`.
- **State:** Help opened via `:help`.
- **Intentional:**
  - Multi-column key reference; densest dialog in the app.
  - Section headings (`Movement`, `Reading view`, `Compare panes`,
    etc.) render in `mid_cyan` + BOLD with a blank-row gap above each
    non-first section (PR #31 — three hierarchy levers stacked:
    weight, color, whitespace).
  - The Compare panes section ends with a muted-grey `Note` row
    (`Refs sidebar hides while comparing; use K for cross-refs.`),
    which surfaces what used to be the persistent compare-mode status
    hint (PR #28).
  - Drop shadow + frame discipline as elsewhere.
- **Watch for:** legibility at this density. Help is a once-a-week
  dialog so dense is fine, but unreadable is not.

---

## Compare panes

### `10-compare.png` — Two compare panes

- **Surface:** `crates/turbo-bible-tui/src/ui/passage.rs` × 2,
  laid out by `ui/mod.rs::panes_layout`.
- **State:** en-kjv (left, dimmed) | es-rv1909 (right, focused).
  References sidebar is hidden — compare mode owns the width.
- **Intentional:**
  - Focused pane: bright-cyan title bar + double-line border + NORMAL
    pill in the status bar.
  - Unfocused pane: single-line border, no pill.
  - Only the rightmost pane keeps its drop shadow (it falls on the
    blue desktop); interior panes suppress theirs so adjacent columns
    tile flush.

### `11-compare-three.png` — Three compare panes

- **Surface:** same; `panes_layout` with n=3.
- **State:** en-kjv | es-rv1909 | la-clementine (focused right).
- **Intentional:**
  - Body evenly split; remainder columns distributed left-to-right.
  - The middle pane retains its border but no shadow (interior pane).
  - 3 panes at ~130 cols ≈ 40 cols each — at or near the `MIN_PANE_W`
    floor (40) the open-pane guard enforces.

### `12-xref-split.png` — Cross-reference opened in a split

- **Surface:** same; second pane carries the `origin_label` flag from
  the `K`-then-`s` flow.
- **State:** John 3:16 (left, source) | the followed cross-reference
  (right, focused). Same translation in both — the flow pulls a
  referenced verse up beside the source, not a parallel translation.
- **Intentional:**
  - Right pane's title bar carries `… ← John 3:16` to mark its
    origin (set when opened from the `K` popup via `s`).

### `14-poetry-compare.png` — Poetry in a compare pane

- **Surface:** same as 10 + poetry indent.
- **State:** Psalm 119:10 in en-kjv (left) and es-rv1909 (right,
  focused).
- **Intentional:**
  - The poetry indent holds per-pane even at the reduced column width,
    not only in a full-width single-pane reader.

---

## Maintenance

When you change a UI surface:

1. Update / re-record the relevant tape entry in `demo/screenshots.tape`
   (the source of truth; never hand-edit a PNG).
2. Update its entry in this README — especially the "intentional"
   bullets if the change reshuffles palette / layout / chrome.
3. Run `just screenshots` to regenerate the PNGs.
4. Eyeball each affected output before committing; binary regen + code
   change should land in one commit.
5. Consider running the `ui-review` skill on the regenerated shots
   (the PostToolUse / Stop hook will nudge you when you touch a UI
   file — see `.claude/settings.json`).
