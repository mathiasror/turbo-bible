# Changelog

All notable changes to this project will be documented here. Format
roughly follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions roughly follow [SemVer](https://semver.org/) until 1.0.

## [Unreleased]

### Changed

- **Reading-view readability pass.** The cursor verse is toned from
  full-saturation cyan down to a calmer teal with full-bright white text so
  scripture dominates, while the cursor's verse *number* is inverse-video'd (a
  yellow chip) so position still pops; every other number keeps its yellow
  scanning rhythm. Footnote / cross-reference markers are now `*` and `+`
  (CP437-safe) and dimmed to read as secondary metadata. The verse-number
  column is a fixed 3 cells — 3-digit Psalm numbers no longer shift the body —
  wrapped lines hang-indent under the verse text, and the pane gained a
  one-cell text inset while the full-row highlight still spans border to
  border.
- **Visual selection** fills the whole anchor→cursor range with the brightest
  cyan slab (the loudest "active right now" signal) and keeps the `▸` marker on
  the cursor end; pressing `v` lights the anchor verse immediately. A
  `[ NORMAL ]` / `[ VISUAL ]` pill on the reading-view title row mirrors the
  splash mode pills.
- **Colour hierarchy.** The overloaded cyan is split into four named,
  configurable theme slots — `bright_cyan` (selection), `cyan` (list focus),
  `teal` (cursor row), `input_teal` (input fields) — referenced by role.
- Locale-aware reference formatting: the chapter–verse separator is a colon for
  English / Spanish / Portuguese and a comma for Norwegian / German / French /
  Latin, applied to the daily verse, bookmarks, sidebar, find results, and the
  Goto preview.
- Splash: the daily verse is bold and the vertical spacing tightened; the
  focused OT/NT column header and rule brighten while the other dims.
- Translations picker: `[*]` / `[ ]` install boxes and a `»` active-translation
  marker (distinct from the `▸` focus cursor) replace the old `✓`.
- The Bookmarks dialog title shows a count, e.g. `Bookmarks (3)`.
- Modal dialogs float over the desktop with the menu and status bars left
  visible (period-correct Turbo Vision), and Goto and Find share one
  input-field widget. The sidebar-off state shows a persistent
  `-- NORMAL | NOREFS --` cue in the status bar.

### Fixed

- The status-bar mode tag no longer clips to a single letter (`-- N` →
  `-- NORMAL --`); shortcut entries are elided before the pill instead.
- The splash hint line is budgeted to the dialog width — it never clips a token
  mid-word or leaves a `(…)` group unclosed.
- The Help dialog scrolls (with a pinned dismiss-hint footer and a `▲` / `▼`
  indicator) instead of truncating its later sections on short terminals.
- Find result snippets wrap to up to two lines, preserving the match
  highlights, with an ellipsis when longer — no more mid-word clipping.
- The Goto preview shows the verse Enter resolves to (`Enter opens: Genesis
  1:1`) rather than dropping it.
- The English daily verse reference reads `Romans 12:2`, not `Romans 12,2`.

## [0.1.0] - 2026-05-26

Initial release.

### Reading

- Turbo Vision–styled terminal UI (ratatui + crossterm): a splash
  screen with title art, a daily verse, and a filterable two-column
  OT/NT book picker; a prose-flow reading view with verse numbers in a
  left gutter, a `▸` cursor marker, consecutive verses running without
  blank lines, and a single-line border that carries the chapter
  reference.
- A References sidebar (shown when the terminal is ≥ ~120 cols) that
  follows the cursor verse, plus a `K` popup — both surfacing
  parallel-passage refs and cross-references.
- vim + Turbo keymap profiles: count prefixes (`5j`), multi-key chords
  (`gg`, `[b`, `]b`, `ZZ`), chapter/book/verse navigation, and a jump
  history bounded at 100 entries (`Ctrl-O` / `Ctrl-I`). Triggers are
  user-extensible via `[keys]` in `config.toml`.
- Visual selection, bookmarks, and clipboard copy (`y`) of the current
  verse and reference.

### Translations

- Eleven public-domain / CC0 / CC-BY translations across seven
  languages, derived from `scrollmapper/bible_databases` by the offline
  `turbo-bible-data` pipeline. The King James Version is embedded in the
  binary and extracted into `$XDG_DATA_HOME/turbo-bible/translations/`
  on first launch; the other ten translations and the shared
  cross-references DB are published as GitHub Release assets and fetched
  on demand, each verified against a SHA-256 in the embedded manifest.
- ~430,000 openbible.info cross-references, shipped as a prebuilt
  `xrefs.db` release asset (symmetric `A→B`/`B→A` pairs deduped, ordered
  by vote). The pipeline builds it from scrollmapper's
  `cross_references_*.db`; the binary fetches it like any other asset.
  Headings and footnotes are unsourced at the pinned commit, so their UI
  surfaces stay inert pending a future source.

### Search & navigation

- FTS5 full-text search with BM25 ranking, a diacritic-folding
  tokenizer, and a prefix index (rebuilt and cached on first launch,
  ~1 s).
- Goto dialog with multi-language book-name parsing
  (`Mark 1:1`, `MRK 1`, `Génesis 1`, `Sal 23,4`).

### Configuration & state

- XDG-style `config.toml` (theme, keybindings, reading layout),
  `state.toml` (last position), and `bookmarks.toml`, with one-time
  migration from the legacy JSON formats.
- `turbo-bible install --force` re-extracts the embedded translation.
- A `website/install.sh` curl-installer that pre-stages all eleven
  translations, so a curl-installed copy is fully offline from first
  launch.

### Internals

- `#![deny(unsafe_code)]` at the crate root.
- A RAII terminal guard that restores the terminal even if a draw
  panics, and atomic translation switching that rolls back on failure.
