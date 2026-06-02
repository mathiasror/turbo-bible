# turbo-bible — issue-66 Tier 1 reading-state captures (wide).
#
# Render with:
#   just ux66-screenshots            # or: vhs demo/ux66-wide.tape
#
# Standalone (not part of the screenshots.tour, like word-diff.tape /
# xrefs-fetch.tape) so re-rendering these never churns the rest of the set.
# Emits, at the tour's ~130-col width:
#   docs/screenshots/26-empty-verse.png   (the "(omitted)" placeholder, #66 #3)
#   docs/screenshots/27-find-scroll.png   (Find keeps the selection in view, #66 #1)
#
# Uses the dev's real (populated) data dir — like the compare shots in the
# tour, it needs en-bsb present (Berean Standard Bible carries the empty
# critical-text verses, e.g. Matt 17:21). The companion *-before.png baselines
# are pinned pre-fix `main` captures and are NOT regenerated here (the old
# behaviour can't be reproduced from this tree).

Output demo/_ux66-wide-trace.gif

Set Shell        "bash"
Set Width        1600
Set Height       860
Set FontSize     19
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  20ms

# --- Setup (hidden): fresh config + pinned daily verse, real data dir --------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "export SOURCE_DATE_EPOCH=1717000000"
Enter
Type "export TB_NO_UPDATE_CHECK=1"
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --translation en-bsb --book MAT --chapter 17"
Enter
Show
Sleep 3500ms

# === 26 — empty-verse "(omitted)" placeholder ===========================
# Matt 17:21 is a critical-text omission in the BSB; land the cursor on it so
# the italic "(omitted)" stand-in sits on the highlighted row.
Type ":"
Sleep 200ms
Type "Matthew 17:21"
Sleep 400ms
Enter
Sleep 1200ms
Screenshot "docs/screenshots/26-empty-verse.png"
Sleep 500ms

# === 27 — Find scrolls the selection into view ==========================
# "the" matches hundreds of verses; arrow well past the first screenful — the
# selected hit must stay highlighted in view (before this fix it scrolled
# off-screen while the "N of M" counter kept climbing).
Type "/"
Sleep 200ms
Type "the"
Sleep 900ms
Down@70ms 16
Sleep 700ms
Screenshot "docs/screenshots/27-find-scroll.png"
Sleep 500ms
Escape
Sleep 300ms

# --- Quit ---------------------------------------------------------------
Hide
Type "q"
Sleep 400ms
