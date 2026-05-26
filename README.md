# turbo-bible

Turbo Vision–styled terminal Bible reader written in Rust. Ships
eleven public-domain (and CC0 / CC-BY) translations across seven
languages:

| Code            | Title                                | Language   | License            |
| --------------- | ------------------------------------ | ---------- | ------------------ |
| `en-kjv`        | King James Version (1769)            | English    | Public Domain      |
| `en-asv`        | American Standard Version (1901)     | English    | Public Domain      |
| `en-ylt`        | Young's Literal Translation (1898)   | English    | Public Domain      |
| `en-drc`        | Douay-Rheims-Challoner               | English    | Public Domain      |
| `en-bsb`        | Berean Standard Bible                | English    | CC0-1.0            |
| `nb-1930`       | Bibelen 1930 (Bokmål)                | Norwegian  | Public Domain      |
| `es-rv1909`     | Reina-Valera 1909                    | Spanish    | Public Domain      |
| `de-menge`      | Menge-Bibel (1939)                   | German     | Public Domain      |
| `fr-crampon`    | La Bible Crampon (1923)              | French     | Public Domain      |
| `pt-blivre`     | Bíblia Livre                         | Portuguese | CC-BY 3.0 BR       |
| `la-clementine` | Clementine Vulgate (1592)            | Latin      | Public Domain      |

<!-- Absolute URL so the image renders on crates.io and docs.rs, which
     don't resolve relative repo paths. -->
![turbo-bible demo](https://raw.githubusercontent.com/mathiasror/turbo-bible/main/demo/demo.gif)

For a narrative walk-through of every feature, see
[`docs/USAGE.md`](docs/USAGE.md). The keymap and config layout below are
the reference; the guide is the tutorial.

All eleven translations are derived from
[`scrollmapper/bible_databases`][scrollmapper] by the offline data
pipeline in `crates/turbo-bible-data`. The King James Version is
embedded in the binary as a zstd-compressed asset; the other ten
translations and the shared cross-references DB are published as
GitHub Release assets and fetched on demand.

[scrollmapper]: https://github.com/scrollmapper/bible_databases

## Setup

Nothing to install — the King James Version is embedded in the binary
and extracted into `$XDG_DATA_HOME/turbo-bible/translations/` (typically
`~/.local/share/turbo-bible/translations/`) on first launch, so reading
works offline straight away. The other ten translations and the shared
cross-references DB are downloaded from GitHub Releases the first time
you open them, each verified against a SHA-256 in the embedded manifest.
The `install.sh` curl-installer pre-stages all eleven, so a
curl-installed copy is fully offline from the first launch.

Re-extract the embedded translation at any time:

```sh
turbo-bible install --force
```

## Run

```sh
cargo run -p turbo-bible --release
# Pick a translation explicitly:
cargo run -p turbo-bible --release -- --translation nb-1930
# Or jump straight into a passage:
cargo run -p turbo-bible --release -- --book JHN --chapter 3
```

Translation resolution at startup:

```
--translation flag  >  config.default_translation  >  first translation in DB
```

First launch rebuilds the FTS5 index with a diacritic-folding tokenizer and a
prefix index — takes ~1 s and is cached.

## Switching translations

Press `t` (or `F5`) in either the splash or the reading view to open the
**Translations** picker. `j`/`k`/`Enter`/`Esc` work as in any dialog. The
selected translation becomes the default for the next launch.

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
| `F2` / `:` | Goto dialog (`Mark 1:1`, `MRK 1`, `Génesis 1`) |
| `F3` / `/` | Find dialog (FTS5; BM25-ranked) |
| `K` | Footnote / cross-reference popup for current verse |
| `t` / `F5` | Translations picker |
| `M` / `F4` | Bookmarks |
| `b` | toggle bookmark on cursor verse (or visual selection) |
| `v` / `V` | enter / exit visual selection mode |
| `Tab` | toggle References sidebar |
| `y` | copy current verse + reference to clipboard |
| `F1` | help |
| `Esc` | back to splash (or close dialog) |
| `q` / `ZZ` / `ZQ` / `:q` | quit |
| `:h` / `:help` | open help |

### Splash screen

The TURBO BIBLE splash is the home screen. It shows the title art, a daily
verse, and a filterable book picker. The book list is split into two columns:
**Old Testament** (39 books) on the left and **New Testament** (27 books) on
the right.

**NORMAL** (default):

- `h` / `←` and `l` / `→`: focus OT / NT column (or `Tab` to toggle)
- `j` / `k`: move cursor within the focused column
- `gg` / `G`: top / bottom of the focused column (or Continue / last book)
- `Ctrl-D` / `Ctrl-U` / `Ctrl-F` / `Ctrl-B`: half-page / full-page
- Count prefix works: `5j` / `10G`
- `Enter` / `o`: open the selected book (or "Continue")
- `/`: enter FILTER mode
- `:` / `F2` / `F3` / `t`: Goto / Goto / Find / Translations dialogs
- `q` / `Esc`: quit

**FILTER**:

- Type to narrow the list; `Enter` accepts, `Esc` clears, `Ctrl-U` wipes.

The **References sidebar** sits to the right of the reading pane and
auto-follows the cursor verse. It shows the parallel-passage refs, footnote
bodies, and cross-references for the current verse — appears when the
terminal is at least ~120 columns wide.

### Inside dialogs

`Enter` confirms, `Esc` cancels. In Find, `↑`/`↓` navigate results and
`Enter` jumps the cursor to the matched verse (not just the chapter).
Goto with a verse component — `John 3:16`, `Sal 23,4` — likewise lands
the cursor on the verse. In the Footnote popup, `↑`/`↓` selects a
cross-reference and `Enter` follows it.

## State and configuration

XDG-style paths:

| Path                                    | Purpose |
| --------------------------------------- | ------- |
| `~/.config/turbo-bible/state.toml`      | last-position bookkeeping (book/chapter/verse) — written on quit |
| `~/.config/turbo-bible/bookmarks.toml`  | saved bookmarks |
| `~/.config/turbo-bible/config.toml`     | user preferences (theme, keybindings, reading layout) |
| `~/.local/share/turbo-bible/translations/` | per-translation `<code>.db` files + shared `xrefs.db` (extracted from bundled assets on first launch) |

Legacy `state.json` / `bookmarks.json` under `~/.config/turbo-bible/` are
migrated to TOML on first launch and removed.

### `config.toml` layout

```toml
default_translation = "en-kjv"

[reading]
show_sidebar     = true   # initial (Tab to toggle)
show_daily_quote = true   # splash "verse of the day" on/off
max_width        = 80     # reading pane max width in cols

[theme]
# CGA palette by default. Any 24-bit hex color works.
blue         = "#0000aa"
cyan         = "#00aaaa"
bright_white = "#ffffff"
light_grey   = "#aaaaaa"
dark_grey    = "#555555"
yellow       = "#ffff55"
hotkey_red   = "#aa0000"
black        = "#000000"

[keys]
# Additive triggers — vim-style defaults always remain functional.
# Key syntax: "q", "Ctrl-d", "Shift-Tab", "Alt-x", "F5", "Esc", "Enter",
#             "Space", "Tab", "Up"/"Down"/"Left"/"Right",
#             "Home"/"End", "PageUp"/"PageDown", "Backspace"/"Delete".
open_translations = ["F5"]    # example: adds F5 as an alias for `t`
quit              = ["Ctrl-q"]
```

Multi-key chords (`gg`, `[b`, `]b`, `ZZ`) and the count prefix are not
remappable.

## Notes on terminals

The Turbo Vision look uses 24-bit RGB and a `▒` dither. Recent terminals
render it cleanly (iTerm2, Ghostty, Alacritty, WezTerm, Kitty, modern xterm).
macOS `Terminal.app` has flaky Alt-key handling; prefer iTerm2 or Ghostty.

## Layout

The repo is a Cargo workspace.

```
crates/
  turbo-bible-tui/    # the TUI binary (cargo run -p turbo-bible)
  turbo-bible-data/   # offline data pipeline (scrollmapper -> .db.zst)
website/              # hand-authored static site (GitHub Pages, no SSG)
```

Inside `crates/turbo-bible-tui/src/`:

- `main.rs` — arg parsing, terminal setup, main loop, mode dispatch
- `db.rs` — rusqlite + prepared statements, schema, FTS5 rebuild
- `render.rs` — chapter render pass (heading interleave, markers)
- `nav.rs` — book/chapter walking
- `search.rs` — FTS5 query, BM25 ranking, byte-offset highlights
- `keys.rs` — vim sequence state machine with count prefix + user bindings
- `state.rs` — `state.toml` load/save + JSON/legacy-translation migration
- `config.rs` — `config.toml` schema (theme, keys, reading)
- `bookmark.rs` — `bookmarks.toml` load/save
- `theme.rs` — runtime-configurable CGA palette + drop-shadow primitive
- `ui/translations.rs` — translation picker dialog
- `ui/` — desktop, menubar, statusbar, passage view, sidebar, dialogs

## What's not in v1

- Poetry indentation (Psalms render as prose)
- Inline (mid-verse) footnote markers — markers sit at end of verse
- Side-by-side translation diff
- Mouse-driven verse selection (clicks on menu / status bar work)
