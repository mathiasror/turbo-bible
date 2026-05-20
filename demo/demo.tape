# turbo-bible demo
#
# Render with:
#   vhs demo/demo.tape
#
# Requires:
#   - `cargo build --release` has been run (we invoke ./target/release/turbo-bible).
#   - The bible.sqlite has been populated by scripts/import_translations.py.
#
# The tape isolates config state under a temp XDG_CONFIG_HOME so the demo
# starts on a clean splash (no "Continue" entry from prior runs).

Output demo/demo.gif

Set Shell    "bash"
Set FontSize 14
Set Width    1400
Set Height   820
Set Padding  20
Set Theme    "Catppuccin Mocha"
Set TypingSpeed 55ms

# --- Setup (hidden) -----------------------------------------------------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "clear"
Enter
Show

# --- Launch -------------------------------------------------------------
Type "./target/release/turbo-bible"
Enter
Sleep 2500ms

# --- Splash: filterable book picker -------------------------------------
# Show the title art + daily verse + OT/NT columns for a beat.
Sleep 1500ms

# Filter the book list — narrows both OT (Job, Joshua, Joel, Jonah) and
# NT (John, 1/2/3 John) to anything matching "jo".
Type "/"
Sleep 400ms
Type "jo"
Sleep 1200ms
Enter
Sleep 600ms

# Hop over to the New Testament column and open John.
Tab
Sleep 600ms
Enter
Sleep 2500ms

# --- Reading: move the cursor through John 1 ----------------------------
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 600ms

# --- Goto dialog: jump straight to John 3:16 ----------------------------
Type ":"
Sleep 500ms
Type "John 3:16"
Sleep 900ms
Enter
Sleep 2500ms

# --- Find dialog: FTS5 search for "love" --------------------------------
Type "/"
Sleep 500ms
Type "love"
Sleep 1200ms
# Browse a few BM25-ranked hits, then jump to one.
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Enter
Sleep 2500ms

# --- Bookmarks: toggle on the current verse, open list, dismiss ---------
Type "b"
Sleep 800ms
Type "M"
Sleep 1800ms
Escape
Sleep 800ms

# --- Translations picker: switch to Bibelen 1930 (Norwegian) ------------
Type "t"
Sleep 1200ms
Down
Sleep 500ms
Enter
Sleep 2500ms

# --- Help overlay -------------------------------------------------------
Type ":help"
Sleep 600ms
Enter
Sleep 3000ms
Escape
Sleep 600ms

# --- Quit ---------------------------------------------------------------
Type "q"
Sleep 600ms
