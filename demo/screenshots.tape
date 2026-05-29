# turbo-bible — labeled screenshots for the user guide.
#
# Render with:
#   vhs demo/screenshots.tape
#
# Drives the TUI through every distinct screen and emits a PNG for each.
# All shots share the same terminal dimensions / font for visual consistency.
#
# Output goes to docs/screenshots/*.png.

Output demo/_screenshots-trace.gif

Set Shell        "bash"
# ~130 cols × ~34 rows: wide enough to clear the 120-col References-sidebar
# threshold (reading-pane 80 + sidebar 34 + gaps) and tall enough that the
# Help dialog shows without scrolling.
Set Width        1600
Set Height       860
Set FontSize     19
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  20ms

# --- Setup (hidden): clean XDG so the splash starts deterministic --------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
# Pin the splash "verse of the day". quote.rs::day_index() reads
# SOURCE_DATE_EPOCH (the reproducible-builds standard, integer seconds since
# the Unix epoch) when set, so the daily verse is fixed instead of tracking
# the calendar — keeps the three splash captures byte-reproducible across
# re-renders on different days. The value below lands in mid-2024.
Type "export SOURCE_DATE_EPOCH=1717000000"
Enter
Type "mkdir -p docs/screenshots"
Enter
Type "clear"
Enter
Show

# --- Launch -------------------------------------------------------------
# Pin the translation: with XDG_CONFIG_HOME wiped, startup falls back to
# "first translation in DB" — which is `de-menge` alphabetically, and the
# "love" search later in the tape would return no matches.
Hide
Type "./target/release/turbo-bible --translation en-kjv"
Enter
Show
Sleep 2000ms

# === 01 — Splash screen =================================================
# NOTE: VHS Screenshot is asynchronous — the next input is processed *before*
# the snapshot is flushed to disk. Always Sleep 300ms+ after Screenshot to
# stop the following keystroke from leaking into the captured frame.
Screenshot "docs/screenshots/01-splash.png"
Sleep 400ms

# === 02 — Splash filter mode ============================================
Type "/"
Sleep 200ms
Type "jo"
Sleep 600ms
Screenshot "docs/screenshots/02-splash-filter.png"
Sleep 400ms

# === 16 — Splash filter, no matches =====================================
# Wipe the active "jo" filter, then type a query that resolves to zero
# books — captures the FILTER mode's empty-state in the picker columns.
# Restore "jo" before Enter so the subsequent Tab+Enter still lands on
# John.
Ctrl+U
Sleep 200ms
Type "xyzzy"
Sleep 600ms
Screenshot "docs/screenshots/16-splash-filter-empty.png"
Sleep 400ms
Ctrl+U
Sleep 200ms
Type "jo"
Sleep 400ms

Enter
Sleep 200ms

# Hop to NT, open John ---------------------------------------------------
Tab
Sleep 200ms
Enter
Sleep 1500ms

# Jump to John 3:16 so the reading shot has a recognisable verse ---------
Type ":"
Sleep 150ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1200ms

# === 03 — Reading view ==================================================
Screenshot "docs/screenshots/03-reading.png"
Sleep 400ms

# === 17 — Reading view without the References sidebar ===================
# Tab toggles the sidebar in single-pane mode. At ~130 cols it's normally
# auto-visible (shot 03); this captures the same passage with it folded
# away. Tab back immediately so the rest of the tour keeps its sidebar.
Tab
Sleep 500ms
Screenshot "docs/screenshots/17-reading-no-sidebar.png"
Sleep 400ms
Tab
Sleep 300ms

# === 04 — Goto dialog ===================================================
Type ":"
Sleep 200ms

# === 18 — Goto opened with the current verse prefilled ==================
# Goto opens with the active reference pre-populated; the first typed
# character clears it (see goto.rs::handle). Capture the prefilled state
# before any keystrokes, with its distinct "(type a reference, or Enter
# to stay here)" preview hint.
Screenshot "docs/screenshots/18-goto-prefilled.png"
Sleep 400ms

Type "Genesis 1:1"
Sleep 500ms
Screenshot "docs/screenshots/04-goto.png"
Sleep 400ms
Escape
Sleep 400ms

# === 05 — Find dialog ===================================================
Type "/"
Sleep 200ms
Type "love"
Sleep 800ms
Down
Sleep 150ms
Down
Sleep 300ms
Screenshot "docs/screenshots/05-find.png"
Sleep 400ms

# === 19 — Find with no matches ==========================================
# Wipe "love" (Find handles Backspace but not Ctrl-U — see find.rs::handle)
# and type a query no verse contains. The dialog renders an italic "(no
# matches)" line where the hits would otherwise be; that empty-state is
# the surface under audit.
Backspace
Sleep 50ms
Backspace
Sleep 50ms
Backspace
Sleep 50ms
Backspace
Sleep 100ms
Type "xyzzy"
Sleep 800ms
Screenshot "docs/screenshots/19-find-no-matches.png"
Sleep 400ms
Escape
Sleep 400ms

# === 20 — Bookmarks dialog, empty =======================================
# Open Bookmarks before any have been seeded: the dialog adapts to its
# compact "no bookmarks yet" layout (see bookmarks.rs — content_h = 3
# when items.is_empty()), distinct from the populated grid in shot 06.
Type "M"
Sleep 800ms
Screenshot "docs/screenshots/20-bookmarks-empty.png"
Sleep 400ms
Escape
Sleep 400ms

# === 06 — Bookmarks dialog (populated) ==================================
# Seed three bookmarks across three books so the dialog demonstrates
# cross-book sorting and doesn't read as a single-passage demo dataset.
# Cursor is still at John 3:16 from earlier — bookmark it, then Goto each
# of the other two and bookmark there. Final list (canon order) reads:
# Psalms 23:1, John 3:16, Romans 8:28.
Type "b"
Sleep 500ms
Type ":"
Sleep 150ms
Type "Psalms 23:1"
Sleep 400ms
Enter
Sleep 1000ms
Type "b"
Sleep 500ms
Type ":"
Sleep 150ms
Type "Romans 8:28"
Sleep 400ms
Enter
Sleep 1000ms
Type "b"
Sleep 500ms
Type "M"
Sleep 800ms
Screenshot "docs/screenshots/06-bookmarks.png"
Sleep 400ms
Escape
Sleep 400ms

# Return to John 3:16 so subsequent shots (08-visual, 10-12 compare) share
# the recognisable verse used across the rest of the tour.
Type ":"
Sleep 150ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1200ms

# === 07 — Translations picker ===========================================
Type "t"
Sleep 500ms
Screenshot "docs/screenshots/07-translations.png"
Sleep 400ms
Escape
Sleep 400ms

# === 08 — Visual selection mode =========================================
Type "v"
Sleep 200ms
Down
Sleep 150ms
Down
Sleep 150ms
Down
Sleep 600ms
Screenshot "docs/screenshots/08-visual.png"
Sleep 400ms
Escape
Sleep 400ms

# === 09 — Help overlay ==================================================
Type ":help"
Sleep 200ms
Enter
Sleep 800ms
Screenshot "docs/screenshots/09-help.png"
Sleep 400ms
Escape
Sleep 400ms

# Re-center on John 3:16 so the compare shots share a recognisable verse.
Type ":"
Sleep 150ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1200ms

# === 10 — Compare panes: two translations side by side ==================
# `Ctrl-W v` opens the Translations picker into a NEW pane; the picker
# cursor starts on the current translation (en-kjv, index 4). Two `j`s land
# on es-rv1909 (index 6, Spanish) for a visually distinct comparison.
# Result: en-kjv (left, dimmed) | es-rv1909 (right, focused — bright border
# + NORMAL pill); mode line reads "2/2".
Ctrl+W
Sleep 200ms
Type "v"
Sleep 600ms
Type "jj"
Sleep 400ms
Enter
Sleep 1300ms
Screenshot "docs/screenshots/10-compare.png"
Sleep 400ms

# === 11 — Three panes ===================================================
# Add a third column to show how N panes scale. Focused pane is now
# es-rv1909 (index 6); two `j`s reach la-clementine (index 8, Latin).
Ctrl+W
Sleep 200ms
Type "v"
Sleep 600ms
Type "jj"
Sleep 400ms
Enter
Sleep 1300ms
Screenshot "docs/screenshots/11-compare-three.png"
Sleep 400ms

# Collapse back to a single pane (close focused pane twice).
Ctrl+W
Sleep 150ms
Type "q"
Sleep 500ms
Ctrl+W
Sleep 150ms
Type "q"
Sleep 800ms

# === 21 — Footnote / cross-reference popup ==============================
# `K` opens the popup itself — a vertical list of cross-references for the
# current verse (John 3:16) with a hint footer. Capture this state before
# the `s` keystroke that turns it into shot 12's split.
Type "K"
Sleep 800ms
Screenshot "docs/screenshots/21-footnote.png"
Sleep 400ms

# === 12 — Cross-reference opened in a split =============================
# Continuing from the open popup (shot 21): `s` opens the highlighted
# cross-reference beside the current verse, instead of `Enter`, which
# would replace it. Result: the source verse (left) and the referenced
# passage (right), same translation — the "pull a referenced verse up
# next to it" flow.
Type "s"
Sleep 1300ms
Screenshot "docs/screenshots/12-xref-split.png"
Sleep 400ms

# === 13 — Poetry indent =================================================
# Close the xref split back to a single pane, then jump to Psalm 119:10 — a
# poetic passage with 2-digit verse numbers, so the verse bodies carry the
# left inset that sets poetry apart from the flush-left narrative prose of the
# earlier shots. Landing on a multi-digit verse keeps the inset reading as a
# deliberate step-in rather than a single-digit numbering artifact.
Ctrl+W
Sleep 150ms
Type "q"
Sleep 500ms
Type ":"
Sleep 150ms
Type "Psalms 119:10"
Sleep 400ms
Enter
Sleep 1200ms
Screenshot "docs/screenshots/13-poetry.png"
Sleep 400ms

# === 14 — Poetry in a compare pane ======================================
# Still at Psalm 119:10 (single en-kjv pane). Open a second pane at the
# same poetic passage in another translation: `Ctrl-W v` opens the picker
# into a NEW pane (cursor on en-kjv, index 4); two `j`s land on es-rv1909
# (index 6, Spanish). Result: en-kjv (left, dimmed) | es-rv1909 (right,
# focused), both showing Psalm 119 — the point is that the poetry inset
# holds per-pane even at the reduced column width, not only in a
# full-width single-pane reader.
Ctrl+W
Sleep 200ms
Type "v"
Sleep 600ms
Type "jj"
Sleep 400ms
Enter
Sleep 1300ms
Screenshot "docs/screenshots/14-poetry-compare.png"
Sleep 400ms

# === 15 — Visual selection over poetry ==================================
# Close the compare pane back to a single en-kjv reader (still at Psalm
# 119:10), then select a four-verse range (119:10–13) with `v` + three
# `j`s. The selection band has to span the *indented* verse bodies as one
# clean rectangle — this is the multi-row counterpart to the single
# cursor-row inset already shown in shot 13.
Ctrl+W
Sleep 150ms
Type "q"
Sleep 500ms
Type "v"
Sleep 200ms
Down
Sleep 150ms
Down
Sleep 150ms
Down
Sleep 600ms
Screenshot "docs/screenshots/15-poetry-visual.png"
Sleep 400ms
Escape
Sleep 400ms

# --- Quit ---------------------------------------------------------------
Type "q"
Sleep 400ms
