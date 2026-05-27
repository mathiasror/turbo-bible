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

**Website:** [turbo.bible](https://turbo.bible) — one-line install, screenshots, and the feature tour.

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
For a prebuilt binary, the curl-installer hosted at
[turbo.bible](https://turbo.bible) pre-stages all eleven translations, so a
curl-installed copy is fully offline from the first launch:

```sh
curl -fsSL turbo.bible/install.sh | sh
```

Re-extract the embedded translation at any time:

```sh
turbo-bible install --force
```

## Bring your own translation

Beyond the bundled eleven, you can import a translation from a JSON file.
`turbo-bible import` builds a SQLite database and installs it alongside
the others — no data pipeline needed:

```sh
turbo-bible import myversion.json \
  --code xx-myver --name "My Version" --language xx
```

The JSON is a list of books, chapters, and verses keyed by OSIS code (or
English book name):

```json
{ "books": [ { "book": "JHN", "chapters": [
  { "chapter": 3, "verses": [ { "verse": 16, "text": "For God so loved…" } ] }
] } ] }
```

It's then selectable via `--translation xx-myver` and in the `t` picker.
See [`docs/IMPORT.md`](docs/IMPORT.md) for the full input format, every
flag, and the resulting database schema.

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
| `n` / `N` | repeat last search forward / backward |
| `K` | Footnote / cross-reference popup (`s` opens the xref in a split) |
| `t` / `F5` | Translations picker |
| `M` / `F4` | Bookmarks |
| `b` | toggle bookmark on cursor verse (or visual selection) |
| `v` / `V` | enter / exit visual selection mode |
| `Tab` | toggle References sidebar (focus next pane when comparing) |
| `y` | copy current verse + reference to clipboard |
| `F1` | help |
| `Esc` | back to splash (or close dialog) |
| `q` / `ZZ` / `ZQ` / `:q` | quit |
| `:h` / `:help` | open help |

### Compare panes (side-by-side)

Read several translations — or a cross-referenced passage — side by side, vim
window-style. Each pane is an independent reader: its own translation,
position, cursor, scroll, and visual selection.

| Keys | Action |
| --- | --- |
| `Ctrl-W v` | open a compare pane (pick a translation; opens at the current passage) |
| `Ctrl-W w` | cycle focus between panes |
| `Ctrl-W h` / `Ctrl-W l` | focus the pane to the left / right |
| `Ctrl-W q` | close the focused pane |
| `Tab` | focus the next pane (while ≥2 panes are open) |
| `s` (in the `K` popup) | open the selected cross-reference in a new pane |

The focused pane keeps the bright border and the `NORMAL`/`VISUAL` pill; the
others dim. Motion keys (`j`/`k`, `h`/`l`, search, Goto) act on the focused
pane only. The References sidebar is hidden while comparing — the panes use the
width. A pane is refused (with a brief status hint) if the terminal is too
narrow to keep every column readable.

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
# Cyan/teal tiers — selection (bright_cyan), structural labels such as sidebar
# headers (mid_cyan), list focus (cyan), cursor row (teal), input wells
# (input_teal).
cyan         = "#00aaaa"
mid_cyan     = "#2ad4d4"
bright_cyan  = "#55ffff"
teal         = "#006a6a"
input_teal   = "#005f5f"
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
- Word-level translation diff (compare panes show translations side by
  side, but differences aren't highlighted)
- Mouse-driven verse selection (clicks on menu / status bar work)

## Contributing

Bug reports and PRs are welcome — see
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the workspace layout, the
`just check` dev gate (fmt + clippy + tests), and the release process.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed
as above, without any additional terms or conditions.

The bundled Bible translations and cross-reference data carry their own
terms — see [`NOTICE`](NOTICE) for per-translation licensing.
