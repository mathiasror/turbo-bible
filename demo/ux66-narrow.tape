# turbo-bible — issue-66 Tier 1 narrow-terminal capture.
#
# Render with:
#   just ux66-screenshots            # or: vhs demo/ux66-narrow.tape
#
# Standalone tape at a deliberately NARROW width (~95 cols, below the ~124-col
# sidebar threshold) so the references sidebar is width-suppressed. Emits:
#   docs/screenshots/28-sidebar-narrow.png   (the NARROW cue, #66 #5)
#
# The mode pill reads "NORMAL | NARROW" (vs "NOREFS" when the sidebar is
# toggled off) and the footer Tab action reads "Refs", so a width-suppressed
# sidebar no longer looks identical to a toggled-off one. The companion
# 28-sidebar-narrow-before.png is a pinned pre-fix `main` capture (plain
# "NORMAL" pill + misleading "Tab Hide"), not regenerated here.

Output demo/_ux66-narrow-trace.gif

Set Shell        "bash"
Set Width        1180
Set Height       720
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

# Land on John 3:16 (sidebar pref defaults on; width suppresses it here).
Type ":"
Sleep 200ms
Type "John 3:16"
Sleep 400ms
Enter
Sleep 1100ms
Screenshot "docs/screenshots/28-sidebar-narrow.png"
Sleep 500ms

# --- Quit ---------------------------------------------------------------
Hide
Type "q"
Sleep 400ms
