# turbo-bible

## Populate full book titles

The crawler captures verse content but not the book title element (e.g.
`Evangeliet etter Matteus`). Run this once after a fresh crawl to fetch each
book's full title in parallel:

```sh
python3 ../crawl.py --update-titles --workers 8
```

This makes 66 HTTP requests (one per book), populates `book.full_name`, and is
safe to re-run.


Turbo Vision-styled terminal Bible reader written in Rust. Reads the
`bible.sqlite` database produced by `../crawl.py` (Bibel 2024, bokmål).

## Run

```sh
cargo run --release
# or, with explicit options:
cargo run --release -- --db ../bible.sqlite --translation nb-2024 --book MRK --chapter 1
```

First launch rebuilds the FTS5 index with a diacritic-folding tokenizer and a
prefix index — takes ~1 s and is cached.

## Keymap

### Reading

| Keys | Action |
| --- | --- |
| `h` / `H` / `←` | previous chapter |
| `l` / `L` / `→` | next chapter |
| `[b` / `]b` | previous / next book |
| `j` / `↓` | next verse (cursor) |
| `k` / `↑` | previous verse |
| `Ctrl-D` / `Ctrl-U` | half-page down / up |
| `Ctrl-F` / `Ctrl-B` / `Space` | page down / up |
| `gg` / `G` | first / last verse |
| `Ctrl-O` / `Ctrl-I` | jump back / forward in history |

Count prefixes work: `5j` moves the cursor down 5 verses.

### Search & navigation

| Keys | Action |
| --- | --- |
| `F2` / `:` | Goto dialog (`Mark 1:1`, `MRK 1`, `1 Mos 1`) |
| `F3` / `/` | Find dialog (FTS5; BM25-ranked) |
| `K` | Footnote / cross-reference popup for current verse |
| `Tab` | toggle References sidebar |
| `y` | copy current verse + reference to clipboard |
| `F1` | this help |
| `Esc` | back to splash (or close dialog) |
| `q` / `ZZ` / `ZQ` / `:q` | quit |
| `:h` / `:help` | open help |

### Splash screen

The TURBO BIBLE splash is the home screen. It shows the title art, a daily
verse, and a filterable book picker. It has two modes:

The book list is split into two columns: **Det gamle testamentet** (GT, 39
books) on the left and **Det nye testamentet** (NT, 27 books) on the right.

**NORMAL** (default):

- `h` / `←`: focus GT column
- `l` / `→`: focus NT column
- `Tab`: toggle between columns
- `j` / `k` (or `↓` / `↑`): move cursor within the focused column
- `gg` / `G`: top / bottom of the focused column (or Continue / last book)
- `Ctrl-D` / `Ctrl-U`: half-page
- `Ctrl-F` / `Ctrl-B`: full page
- Count prefix works: `5j` / `10G`
- `Enter` / `o`: open the selected book (or "Continue")
- `/`: enter FILTER mode
- `:`: opens Goto, `F2` / `F3` for Goto / Find dialogs
- `q` / `Esc`: quit

Each column scrolls independently. The unfocused column's cursor is shown
in dim grey so you can see where it'll land when you switch.

**FILTER**:

- Type to narrow the list
- `Enter`: accept filter, back to NORMAL (j/k navigates filtered list)
- `Esc`: clear filter, back to NORMAL
- `Ctrl-U`: wipe the filter

The **References sidebar** sits to the right of the reading pane and
auto-follows the cursor verse. It shows the parallel-passage refs
(`(Matt 3,1–12; ...)`), footnote bodies, and cross-references for the
current verse — so you don't have to break your reading flow to consult
them. It appears when the terminal is at least ~120 columns wide.

### Inside dialogs

`Enter` confirms, `Esc` cancels. In Find, `↑`/`↓` navigate results.
In the Footnote popup, `↑`/`↓` selects a cross-reference and `Enter`
follows it.

## State

Last position is persisted to `~/.config/turbo-bible/state.json` on quit
and restored on next launch.

## Notes on terminals

The Turbo Vision look uses 24-bit RGB and a `▒` dither. Recent
terminals render it cleanly (iTerm2, Ghostty, Alacritty, WezTerm,
Kitty, modern xterm). macOS `Terminal.app` has flaky Alt-key handling;
prefer iTerm2 or Ghostty.

## Layout

- `src/main.rs` — arg parsing, terminal setup, main loop, mode dispatch
- `src/app.rs` — (currently inlined in main)
- `src/db.rs` — rusqlite + prepared statements + types + FTS5 rebuild
- `src/render.rs` — chapter render pass (heading interleave, markers)
- `src/nav.rs` — book/chapter walking
- `src/search.rs` — FTS5 query, BM25 ranking, byte-offset highlights
- `src/keys.rs` — vim sequence state machine with count prefix
- `src/state.rs` — `state.json` load/save
- `src/theme.rs` — CGA palette + drop-shadow primitive
- `src/ui/` — desktop, menubar, statusbar, passage view, dialogs

## What's not in v1

- Poetry indentation (Psalms render as prose)
- Inline (mid-verse) footnote markers — all markers sit at end of verse
- Bookmarks, highlights, notes
- Multiple translations side-by-side
- Theming (the palette is hardcoded for full retro)
- Mouse support
