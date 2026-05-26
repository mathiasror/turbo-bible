# turbo-bible demo — README hero. One story: full-text search → jump → read.
#
# Render with:  vhs demo/demo.tape   (or: just demo)
# Requires ./target/release/turbo-bible (run: cargo build -p turbo-bible --release).
#
# Tuning notes:
#   - One narrative arc, ~9s, 1200x720 → README/mobile-friendly, ~1MB gif.
#   - TypingSpeed 90ms reads as human/confident (not robotic).
#   - On the splash, "/" is the book filter, so Find ("/") is reached from
#     the reading view — hence the Enter (open Genesis 1) before the search.
#   - XDG_CONFIG_HOME is redirected to a temp dir → deterministic splash
#     every re-record. --translation en-kjv keeps the "love" search English
#     (a wiped config otherwise falls back to de-menge, the first DB
#     alphabetically, and the search would return no matches).
#   - Launch inside the Hide block so the gif opens already on the splash —
#     no shell prompt or `./target/...` typing flashes in the final gif.

Output demo/demo.gif

Set Shell         "bash"
Set FontSize      16
Set Width         1200
Set Height        720
Set Padding       20
Set Theme         "Catppuccin Mocha"
Set TypingSpeed   90ms
Set WindowBar     Colorful
Set WindowBarSize 32

# --- Setup (hidden) -----------------------------------------------------
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d)"
Enter
Type "clear"
Enter
Type "./target/release/turbo-bible --translation en-kjv"
Enter
Sleep 1500ms
Show

# --- Splash: let the TURBO BIBLE title art + daily verse register -------
Sleep 1500ms

# --- Open a book so the reading-view Find ("/") is available; Genesis 1
#     renders as a brief, recognizable frame --------------------------------
Enter
Sleep 1200ms

# --- Find: FTS5 search across the whole Bible ---------------------------
Type "/"
Sleep 500ms
Type "love"
Sleep 1400ms

# --- Browse the BM25-ranked results, settle on 1 John 4:18 (query
#     highlighted yellow throughout) ---------------------------------------
Down
Sleep 1200ms

# --- Jump: cursor lands on the matched verse → hero reading frame -------
Enter
Sleep 3200ms
