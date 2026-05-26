# Changelog

All notable changes to this project will be documented here. Format
roughly follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions roughly follow [SemVer](https://semver.org/) until 1.0.

## [Unreleased]

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
