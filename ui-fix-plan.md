# Turbo Bible UI Fix Plan

Synthesis of `turbo-bible-ui-review.md` (detailed) and `turbo_bible_full_review.md` (ChatGPT). Scope: **all** findings, HIGH through LOW.

Source tags: **D** = detailed review, **CG** = ChatGPT review. When both apply, both are tagged.

---

## Phase 0 — Bug verification (DONE)

Verified the three suspected bugs by reading code + re-running `scripts/screenshots.sh` + an isolated b-only VHS tape that asserted against `bookmarks.toml`.

1. **`15-bookmarks.png` blank screen — NOT AN APP BUG.** With `b` alone the dialog stays closed and `bookmarks.toml` correctly records the new bookmark. The blank PNG (and the "dialog already open" frame in the wider tape) are VHS screenshot-timing artifacts where key delivery and frame capture aren't strictly serialised. **Fix the tape, not the app** — bump the sleep before `Screenshot 15-bookmarks.png` (or split into two stages).
2. **`02-splash-filter.png` identical to splash — NOT AN APP BUG.** Re-running the tape produced a correct frame: yellow `FILTER` pill, `Psalm` typed, OT filtered to Psalms. Same VHS class of flake as #1.
3. **Find → matched verse — REAL BUG (HIGH).** Confirmed via code trace: `nav::Position` (`src/nav.rs:7-11`) has only `book` + `chapter`. `find.rs:42-50` returns `Position { book, chapter }` discarding `hit.verse`. `main.rs:707-720` `jump_to()` always sets `*cursor_verse = 1`. Three coordinated changes needed:
    - Either add a `verse: Option<i64>` to `Position`, or carry verse separately through the `FindOutcome::Jump` payload.
    - Thread the verse through `jump_to` so it doesn't unconditionally reset to 1.
    - Same fix benefits Bookmarks Jump (jumps to first verse of chapter instead of bookmark.start_verse) and Goto Jump (when user types `John 3:16`).

**Phase 1.6 scope confirmed**: only the find-jump bug. Plus a tape-hardening pass (longer sleeps or explicit waits) so review screenshots are reliable.

---

**Execution status (as of 2026-05-20)**: Phases 0, 1, 2 complete. Phase 3 mostly complete — see per-section ticks below. Deferred items called out at the end.

---

## Phase 1 — HIGH severity (DONE)

### 1.1 State propagation cluster ✓
- **Title bar** now reads from `translation_label` state — `menubar::render(title, ...)` takes a `&str`, the `MenuItem` shell is gone, and the splash + reading code paths build `Turbo Bible · {label}` from the active translation. `t`/`F5` switches translation and the title follows. `src/ui/menubar.rs`, `src/ui/mod.rs`, `src/main.rs`.
- **Testament headings** localised. New `testament_labels(code)` helper in `src/ui/splash.rs` maps the language prefix (`en`, `es`, `nb`/`nn`/`no`, `de`, `fr`) to the right pair; default falls back to English. `SplashView::new` gained a `translation_code` parameter.

### 1.2 Help dialog ✓
- Title renamed `Help — Bible TUI` → `Help`.
- Audited against `keys.rs` and added every missing row: `v`/`V` (visual), `b` (bookmark), `T` (layout), `M`/`F4` (bookmarks dialog), `t`/`F5` (translations), count prefixes (`5j 10G`), `ZZ`/`ZQ`/`:q` (quit variants).
- Grouped into sections: Movement / Selection & bookmarks / Reading view / Dialogs / Quit. Dialog height bumped to fit. `src/ui/help.rs`.

### 1.3 Visual mode legibility ✓
- **Cursor marker** added: render.rs gutter glyph is `▸` on the cursor verse, `★` on bookmarks, blank otherwise. Distinguishes the active line inside a multi-verse selection.
- **VISUAL pill** switched to yellow background (vs cyan for other modes) in `statusbar::render`. Non-textual signal alongside the `-- VISUAL --` text.
- **Sidebar range**: `SidebarView` now takes `selection: Option<(i64, i64)>` and renders `John 3:1-4  (4 verses)` instead of the cursor verse alone.

### 1.4 K affordance ✓
- Reading-view status bar now lists `K Notes`, `v Select`, `T Layout`, and toggles `Tab Hide`/`Tab Refs` based on sidebar state. `make_status` in `main.rs` takes `show_sidebar` so the Tab hint is accurate.

### 1.5 Find results density ✓
- Each result now spans three rows: reference (full-width, selectable), indented snippet with highlight, blank separator. `src/ui/find.rs`.

### 1.6 Find-jump bug ✓
- `nav::Position` gained `verse: Option<i64>`. `Find` now writes `Some(hit.verse)`, `Bookmarks` writes `Some(b.start_verse)`, `Goto` parses and writes the typed verse (`John 3:16` now actually lands on 16), and `footnote::parse_osis` keeps the verse. `jump_to` honours it and clamps to the passage size; `None` still lands on verse 1.

---

## Phase 2 — MEDIUM severity (DONE except as noted)

### 2.1 Mode/state communication ✓
- Mode tags renamed: `COMMAND` → `GOTO`, `SEARCH` → `FIND`.
- `mode_tag_for` now returns a `String` and includes the verse-layout marker (`-- NORMAL · 1L --` / `-- NORMAL · 2L --`).

### 2.2 Find workflow (partial)
- ✓ Find input now shows the empty-state hint `→ (type to search, e.g. "love", "kingdom of God")` in yellow.
- ⏸ **Deferred**: `n`/`N` next-match persistence after jump. Needs cross-mode state tracking (the search context has to survive switching from Find dialog → Reading); too much plumbing for this pass.
- ⏸ **Deferred**: sunken-bevel input affordance. Cosmetic, low impact.

### 2.3 Splash polish ✓ (focused-column already correct)
- In-dialog footer trimmed to splash-unique hints (`j k move  h l Tab column  gg G ends  / filter  t translation`). Global shortcuts (`Enter`/`F2`/`F3`/`Esc`/`Q`) live only in the bottom status bar — no more F2/F3 duplication.
- Focused-column cue already worked: `column_header_focused` style applies bright-white underline to the active column header. No additional change needed.

### 2.4 Reading view hints ✓
- Status bar adds `K Notes`, `v Select`, `T Layout`, and `Tab Hide`/`Tab Refs` (toggles based on sidebar state).

### 2.5 Sidebar — visual subordination ✓
- Border changed from `BorderType::Double` (bright-white) to `BorderType::Plain` (light-grey).
- Title `References` rendered in light-grey instead of bright-white.
- Reading pane keeps its double-border bright-white — now unambiguously primary.
- ⏸ **Deferred**: collapse-to-title-bar-when-empty. Subordination already addresses the "competes with scripture" complaint without restructuring the body layout.

### 2.6 Footnote popup empty state ✓
- Empty popup now shrinks to ~5 rows × 50 cols (vs full 22 × 80 when populated).
- Footer collapses to `Esc close` when there are no footnotes — no advertising unreachable actions.

### 2.7 Two-line layout indent
- **Skipped.** Current implementation already hang-indents wrapped lines under the verse-number column (the 5-cell `VERSE_PREFIX`). ChatGPT's example actually describes single-line behaviour; treating it as a refactor of the layout-toggle semantics is out of scope.

### 2.8 Contrast / readability ✓
- Body verse text dimmed to `light_grey`. Cursor row keeps bright-white on cyan, so the active verse is the only bright prose on screen. `render.rs::verse_text_style`.

---

## Phase 3 — LOW severity

### 3.1 Naming and casing ✓
- Splash dialog frame label `TURBO BIBLE` → `Turbo Bible`. ASCII art logo unchanged (it's typographic art, not chrome).
- Help title `Help — Bible TUI` → `Help` (P1.2).
- All other dialog titles already sentence case: `Goto reference`, `Find`, `Bookmarks`, `Translations`, `Notes for John 3:16`.

### 3.2 Reading view polish (mostly done)
- ✓ **Chapter heading gap** tightened — the trailing blank between the rule and verse 1 is gone, anchoring verse 1 to the rule.
- ✓ **Selection highlight extends to right margin** in both 1L and 2L layouts. `pad_to_width` detects cursor-cyan rows and pads with cyan.
- ⏸ **Verse number column width** — currently uses `VERSE_NUM_WIDTH = 3` (1-cell gutter glyph + 2-digit num). Already minimal for `176`. The "4-cell" complaint may have been pre-gutter; no change needed.
- ⏸ **Dialogs cover their source verse** — geometric pinning would need viewport/passage cooperation. Deferred.
- ⏸ **Reading width cap** — already exists via `config.reading.max_width` (defaults to 80). User-tunable, no code change required.
- ⏸ **Splash chrome luminance** — purely aesthetic; deferred.

### 3.3 TV chrome faithfulness
- ⏸ **Flat pills / sunken bevel / stray-cell verification** — pure cosmetics. Deferred.

### 3.4 Help polish ✓
- Absorbed into P1.2.

### 3.5 Single-line vs two-line defaults
- Decision item, not a fix. No change.

### 3.6 Goto placeholder ✓
- Empty-state Goto input now shows dim grey `John 3:16` placeholder after the cursor block; disappears on first keystroke.

---

## Deferred items (snapshot)

The work above intentionally leaves these on the table:

1. **Find result n/N persistence** — needs Find→Reading state plumbing.
2. **Sidebar collapse-when-empty** — body-layout restructuring; visual subordination already covers the headline complaint.
3. **Two-line layout fragmentation** — current behaviour is acceptable; deeper redesign conflicts with the 1L/2L toggle semantics.
4. **TV chrome bevels / pill shading / stray-cell verification** — pure aesthetics.
5. **Dialogs covering their source verse** — geometric pinning, not local to any one widget.
6. **Splash chrome luminance dim** — cosmetic.
7. **Screenshot tape hardening** — bump sleeps so review screenshots stop being flaky (the original `15-bookmarks.png` blank shot that triggered Phase 0 was a tape artifact, not an app bug).

---

## Suggested execution order

1. **Phase 0** (verification) — half a session.
2. **Phase 1.1** (state propagation, 2 fixes) — biggest visual ROI; touches every screenshot.
3. **Phase 1.2** (help dialog) — unblocks feature discoverability.
4. **Phase 1.6** (any confirmed bugs).
5. **Phase 1.3 → 1.5** (visual mode, xrefs, find density).
6. **Phase 2** — work top-to-bottom; mode-communication cluster (2.1) first, then sidebar (2.5) since it touches every reading screen.
7. **Phase 3** — single mechanical PR for 3.1 naming/casing; the rest as polish.

After each phase, re-shoot the screenshot set used by the reviews so the next pass has fresh artifacts.

---

## Open questions to resolve while implementing

- **Visual mode anchor design**: gutter `>` marker vs inverted verse number — pick whichever the existing ratatui widget makes cheapest.
- **Body text dimness**: needs an empirical contrast check on the actual terminal palette — what reads as "light grey on blue" in iTerm2 may be illegible elsewhere.
- **Reading width cap**: pick a value (78? 88?) — needs a quick reading test.
