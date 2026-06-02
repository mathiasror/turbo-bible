# turbo-bible — issue-66 Tier 2 feedback-layer captures (wide).
#
# Render with:
#   just ux66-screenshots            # or: vhs demo/ux66-tier2.tape
#
# Standalone (like ux66-wide.tape / word-diff.tape), so it never churns the
# tour. Emits, at the tour's ~130-col width:
#   docs/screenshots/29-showcmd.png        (pending-count pill, #66 #7)
#   docs/screenshots/30-bookmark-toast.png (the "Bookmarked …" transient, #66 #8)
#   docs/screenshots/31-goto-error.png     (Goto's unknown-reference error, #66 #11)
#
# The count is cleared between shots by letting it time out (>500ms) rather
# than Esc, so the same tape drives both the pre-fix `main` baseline binary and
# the fixed one. The *-before.png baselines are pinned pre-fix captures, not
# regenerated here.

Output "demo/_ux66-tier2-trace.gif"

Set Shell        "bash"
Set Width        1600
Set Height       860
Set FontSize     19
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  20ms

# --- Setup (hidden): fresh config + pinned daily verse -----------------------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "export SOURCE_DATE_EPOCH=1717000000"
Enter
Type "export TB_NO_UPDATE_CHECK=1"
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --translation en-kjv --book JHN --chapter 3"
Enter
Show
Sleep 3500ms

# Land on John 3:16 so the pill/labels read a recognisable verse.
Type ":"
Sleep 150ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1200ms

# === 29 — showcmd: a pending count is visible in the pill ===============
# Type a count prefix and hold: the pill shows "-- NORMAL 5 --" (before this
# fix the 5 was invisible). The count then times out on its own.
Type "5"
Sleep 300ms
Screenshot "docs/screenshots/29-showcmd.png"
Sleep 700ms

# === 30 — feedback transient: "Bookmarked …" ===========================
# `b` toggles the bookmark AND confirms with a transient (the feedback layer
# behind #8/#9/#10). Capture it within its ~2s window.
Type "b"
Sleep 500ms
Screenshot "docs/screenshots/30-bookmark-toast.png"
Sleep 2300ms

# === 31 — Goto names an unknown reference ===============================
# Open Goto and type something unparseable: the preview reads a red "Unknown
# book or reference" error (before this fix it showed a neutral yellow hint and
# Enter was a dead key).
Type ":"
Sleep 150ms
Type "xyzzy"
Sleep 500ms
Screenshot "docs/screenshots/31-goto-error.png"
Sleep 400ms
Escape
Sleep 200ms

# --- Quit ---------------------------------------------------------------
Hide
Type "q"
Sleep 400ms
