# turbo-bible demo — tuned for social sharing (Snapchat / Discord).
#
# Render with:
#   vhs demo/demo.tape
#
# Requires:
#   - `cargo build --release` has been run.
#   - The bible.sqlite has been populated by scripts/import_translations.py.
#
# Tuning notes (vs. earlier readme-style tape):
#   - FontSize 20 → readable on phone screens.
#   - Terminal width sits just under the 120-col sidebar threshold, so the
#     reading pane uses the full width (no half-empty "References" gutter).
#   - WindowBar adds a chrome frame that looks polished in Discord previews.
#   - Sleeps trimmed ~40% and typing sped up so the whole loop is ~22s.
#   - XDG_CONFIG_HOME is redirected to a temp dir → splash starts clean
#     every re-record.

Output demo/demo.gif

Set Shell        "bash"
Set FontSize     20
Set Width        1400
Set Height       820
Set Padding      18
Set Theme        "Catppuccin Mocha"
Set TypingSpeed  35ms
Set WindowBar    Colorful
Set WindowBarSize 36

# --- Setup (hidden) -----------------------------------------------------
# Launch the binary inside the Hide block so recording starts with the
# splash already mounted — no shell prompt or `./target/...` typing flashes
# in the final gif. Pin the translation: with XDG_CONFIG_HOME wiped, startup
# falls back to "first translation in DB" — which is `de-menge`
# alphabetically, and the "love" search later in the tape would return no
# matches.
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --translation en-kjv"
Enter
Sleep 1500ms
Show

# --- Splash: filterable book picker -------------------------------------
Sleep 800ms

# Filter "jo" → narrows OT (Job/Joshua/Joel/Jonah) + NT (John, 1/2/3 John).
Type "/"
Sleep 250ms
Type "jo"
Sleep 700ms
Enter
Sleep 350ms

# Hop to the New Testament column and open John.
Tab
Sleep 350ms
Enter
Sleep 1400ms

# --- Reading: walk the cursor through John 1 ----------------------------
Down
Sleep 180ms
Down
Sleep 180ms
Down
Sleep 180ms
Down
Sleep 350ms

# --- Goto dialog: jump straight to John 3:16 ----------------------------
Type ":"
Sleep 250ms
Type "John 3:16"
Sleep 550ms
Enter
Sleep 1700ms

# --- Find dialog: FTS5 search for "love" --------------------------------
Type "/"
Sleep 250ms
Type "love"
Sleep 700ms
Down
Sleep 220ms
Down
Sleep 220ms
Down
Sleep 220ms
Enter
Sleep 1600ms

# --- Bookmarks: toggle then open the list -------------------------------
Type "b"
Sleep 450ms
Type "M"
Sleep 1300ms
Escape
Sleep 350ms

# --- Translations: switch to Reina-Valera 1909 (Spanish) ----------------
# Picker opens with cursor on the current translation (en-kjv).
# Alphabetical order: de-menge, en-asv, en-bsb, en-drc, en-kjv, en-ylt,
# es-rv1909, … — so two Downs lands on Reina-Valera.
Type "t"
Sleep 700ms
Down
Sleep 220ms
Down
Sleep 350ms
Enter
Sleep 1800ms

# --- Help overlay -------------------------------------------------------
Type ":help"
Sleep 350ms
Enter
Sleep 2200ms
Escape
Sleep 350ms

# --- Quit ---------------------------------------------------------------
Type "q"
Sleep 400ms
