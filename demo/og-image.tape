# turbo-bible — the 1200x630 social card (website/og-image.png).
#
# A real capture of the splash screen, not a hand-drawn mock: same VHS
# toolchain as demo.gif / the user-guide screenshots, sized to the Open
# Graph 1.91:1 spec. Render with:
#
#   just og-image      # builds the release binary, then runs this tape
#
# The verse-of-the-day rotates by date, so re-running on a different day
# changes the quote — that's expected; it's the genuine splash.

Output demo/_og-trace.gif

Set Shell        "bash"
Set Width        1200
Set Height       630
Set Padding      0
Set Theme        "Catppuccin Mocha"
Set FontSize     16
Set TypingSpeed  20ms

# --- Setup (hidden): wipe XDG so the splash is deterministic ------------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "clear"
Enter
Show

# --- Launch en-kjv and let the splash settle ----------------------------
Hide
Type "./target/release/turbo-bible --translation en-kjv"
Enter
Show
Sleep 2500ms

# VHS Screenshot is async — Sleep after so the quit keystroke doesn't leak
# into the captured frame (see the note in screenshots.tape).
Screenshot "website/og-image.png"
Sleep 500ms

Hide
Type "q"
Sleep 300ms
