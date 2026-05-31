# turbo-bible — word-level diff in compare panes (the before/after pair).
#
# Render with:
#   just word-diff-screenshots      # or: vhs demo/word-diff.tape
#
# Standalone (not part of the 21-shot screenshots.tape tour, like og-image.tape)
# so re-rendering this feature's captures never churns the rest of the set.
# Drives the en-kjv | en-asv split at John 3:16 — the same passage in the same
# language, so a clean handful of words diverge — and emits:
#   docs/screenshots/22-compare-word-diff.png      (highlighting ON, the feature)
#   docs/screenshots/23-compare-word-diff-off.png  (toggled OFF — identical to main)
#
# Output goes to docs/screenshots/*.png. Same dimensions / font / theme as
# screenshots.tape for visual consistency with the rest of the tour.

Output demo/_word-diff-trace.gif

Set Shell        "bash"
Set Width        1600
Set Height       860
Set FontSize     19
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  20ms

# --- Setup (hidden): clean XDG + pinned daily verse, mkdir output -----------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "export SOURCE_DATE_EPOCH=1717000000"
Enter
Type "mkdir -p docs/screenshots"
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --translation en-kjv --book JHN --chapter 3"
Enter
Show
Sleep 2000ms

# Land on John 3:16 so the cursor row is a recognisable verse.
Type ":"
Sleep 150ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1200ms

# Open a second pane in en-asv. `Ctrl-W v` opens the picker into a NEW pane;
# the cursor starts on the current translation (en-kjv, index 4). The picker is
# alphabetical by code, so three `k`s land on en-asv (index 1) — an English
# edition, so the word diff is meaningful (cross-language pairs never diff).
Ctrl+W
Sleep 200ms
Type "v"
Sleep 600ms
Type "k"
Sleep 120ms
Type "k"
Sleep 120ms
Type "k"
Sleep 300ms
Enter
Sleep 1400ms

# === 22 — Word diff ON (the feature) ====================================
# en-kjv (left, dimmed) | en-asv (right, focused). The words that differ
# between the two — "in"/"on", "everlasting"/"eternal", and more in the
# verses below — are lit in the diff_word tier; agreeing words stay calm.
Screenshot "docs/screenshots/22-compare-word-diff.png"
Sleep 500ms

# === 23 — Word diff OFF (the before/after baseline) =====================
# Ctrl-W d toggles the overlay off; the panes then render exactly as on
# `main` (the no-diff path), making this the honest "before" of the pair.
Ctrl+W
Sleep 150ms
Type "d"
Sleep 800ms
Screenshot "docs/screenshots/23-compare-word-diff-off.png"
Sleep 500ms

# --- Quit ---------------------------------------------------------------
Hide
Type "q"
Sleep 400ms
