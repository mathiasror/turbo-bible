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

# === 04 — Goto dialog ===================================================
Type ":"
Sleep 200ms
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
Escape
Sleep 400ms

# === 06 — Bookmarks dialog ==============================================
# Seed three bookmarks in-place via single Down presses, then open list.
# Cursor is still at John 3:16 from earlier.
Type "b"
Sleep 500ms
Down
Sleep 350ms
Type "b"
Sleep 500ms
Down
Sleep 350ms
Down
Sleep 350ms
Type "b"
Sleep 500ms
Type "M"
Sleep 800ms
Screenshot "docs/screenshots/06-bookmarks.png"
Sleep 400ms
Escape
Sleep 400ms

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

# === 12 — Cross-reference opened in a split =============================
# `K` opens the footnote / cross-reference popup; `s` opens the highlighted
# cross-reference beside the current verse (instead of `Enter`, which would
# replace it). Result: the source verse (left) and the referenced passage
# (right), same translation — the "pull a referenced verse up next to it"
# flow.
Type "K"
Sleep 800ms
Type "s"
Sleep 1300ms
Screenshot "docs/screenshots/12-xref-split.png"
Sleep 400ms

# === 13 — Poetry indent =================================================
# Close the xref split back to a single pane, then jump to Psalm 23 — a
# poetic passage, so the verse bodies carry the small left inset that sets
# poetry apart from the flush-left narrative prose of the earlier shots.
Ctrl+W
Sleep 150ms
Type "q"
Sleep 500ms
Type ":"
Sleep 150ms
Type "Psalms 23:1"
Sleep 400ms
Enter
Sleep 1200ms
Screenshot "docs/screenshots/13-poetry.png"
Sleep 400ms

# --- Quit ---------------------------------------------------------------
Type "q"
Sleep 400ms
