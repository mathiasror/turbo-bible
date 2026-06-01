# turbo-bible — issue-66 Tier 3 keymap-aware status bar (turbo profile, wide).
#
# Render with:
#   just ux66-screenshots            # or: vhs demo/ux66-tier3.tape
#
# Standalone tape that runs in the **turbo** keymap (writes a config.toml with
# [input] keymap = "turbo" into a fresh XDG_CONFIG_HOME). Emits:
#   docs/screenshots/32-status-turbo.png (the turbo reading footer, #66 #12)
#
# In turbo the footer drops the vim-only `K Notes`/`v Select` and shows F-key
# hints (F1 Help, F2 Goto, F3 Find, F4 Marks). The companion *-before.png is a
# pinned pre-fix capture (the old profile-blind footer that wrongly showed
# K/v), not regenerated here. The matching turbo-aware help sheet can't be
# captured by VHS (it has no function-key support, and turbo opens Help only
# via F1) — it's covered by the help.rs unit tests instead.

Output "demo/_ux66-tier3-trace.gif"

Set Shell        "bash"
Set Width        1600
Set Height       860
Set FontSize     19
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  20ms

# --- Setup (hidden): fresh config in the turbo keymap ------------------------
# Backtick-delimited Type strings so the shell command can carry the double
# quotes TOML needs without tripping VHS's string parser.
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "export SOURCE_DATE_EPOCH=1717000000"
Enter
Type "export TB_NO_UPDATE_CHECK=1"
Enter
Type "mkdir -p $XDG_CONFIG_HOME/turbo-bible"
Enter
Type `echo '[input]' > $XDG_CONFIG_HOME/turbo-bible/config.toml`
Enter
Type `echo 'keymap = "turbo"' >> $XDG_CONFIG_HOME/turbo-bible/config.toml`
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --book JHN --chapter 3"
Enter
Show
Sleep 3500ms

# Step the cursor down to verse 16 (Down is base-layer, works in turbo) so the
# shot reads a recognisable verse without needing the F-key-only Goto.
Down@40ms 15
Sleep 800ms

# === 32 — turbo reading footer (F-key hints, no K/v) ===================
Screenshot "docs/screenshots/32-status-turbo.png"
Sleep 500ms

# --- Quit ---------------------------------------------------------------
Hide
Type "q"
Sleep 400ms
