# Changelog

All notable changes to this project will be documented here. Format
roughly follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions roughly follow [SemVer](https://semver.org/) until 1.0.

## [Unreleased]

## [0.3.0] - 2026-09-17

### Fixed

- **UX audit (#66): 23 intuitiveness fixes across keymaps, navigation, and
  reading.**
  - Find dialog scrolls the selection into view (it used to highlight nothing
    while the counter climbed).
  - Empty critical-text verses render an italic "(omitted)" instead of a bare
    number.
  - `Ctrl-O`/`Ctrl-I` restore the cursor verse.
  - A width-suppressed references sidebar reads `NARROW` (vs `NOREFS` when
    toggled off) so `Tab` no longer looks dead.
  - A transient feedback layer + a vim `showcmd` indicator end the
    silent-feedback problems: pending counts/chords are visible; `Esc` aborts
    them in one press.
  - `b` truly toggles bookmarks; `y` confirms "Copied …"; `n`/`N` say "Pattern
    not found" and announce wraps; Goto names an unknown reference instead of
    dead-`Enter`.
  - Help and the status bar are keymap-aware — the `turbo` profile no longer
    advertises vim keys it can't fire.
  - Count prefixes drive chapter/book/page motions; `Ctrl-W`/`n`/`N` gained
    `[keys]` aliases; the informational menubar is de-emphasised.
  - `n`/`N` follow the Find list's order; the splash filter auto-focuses the
    matching column and opens on one `Enter`.
  - `max_width` is capped so it can't push the sidebar out of reach.
  - Chapter/book motion cues "Start/End of the Bible" at the canon edges.
  - The `K` popup is titled "Cross-references" (the footnote table has no
    source); on-demand download failures now give distinct, actionable messages.
- **Cross-references now actually download on demand.** `cargo` / `brew`
  installs started with an empty `xrefs.db` stand-in and never fetched the real
  ~430k-entry dataset, so cross-references silently never appeared (only the
  curl installer pre-staged them). The `K` notes popup now offers a one-key
  (`d`) download when the data is missing: it fetches `xrefs.db` from GitHub
  Releases on a background thread (sha256-verified against the embedded
  manifest, like every other asset), then re-attaches it and refreshes the open
  passages so markers, the sidebar, and the popup populate. Offline or on
  failure it degrades quietly, matching the translation-download UX.
- **Windows: the prebuilt binary failed at startup.** `fs::canonicalize`
  returns verbatim paths (`\\?\C:\…`) that produced an invalid SQLite URI for
  the cross-references `ATTACH` ("invalid uri authority"). Paths are now
  rewritten to the `/C:/…` URI form. The cross-references download also could
  never land on Windows (it renamed over a file every connection held open); it
  now stages the file and swaps it in with the connections detached. The test
  suite runs on Windows in CI from here on.
- **Downloads can no longer hang forever.** Translation / cross-reference
  fetches gained a connect timeout and stall detection, so a dead connection
  fails instead of wedging the single download slot for the session. The size
  cap is now the manifest's compressed size (it was the much looser
  decompressed size).
- **Goto and the splash filter fold diacritics, like Find.** `genesis` now
  resolves `Génesis` in the Spanish / Portuguese / French / Latin translations
  instead of dead-ending.
- **Config, state, bookmarks, and the update cache are written atomically** — a
  crash or full disk mid-save can no longer truncate your bookmark store.
  An out-of-range jump (`:John 999`, a stale bookmark, `Ctrl-O` after switching
  translation) is clamped to a chapter that exists instead of opening a blank
  pane.
- **Wide-glyph layout.** Reading pane, dialogs, and the splash grid measure
  text in display cells rather than chars, so an imported CJK / fullwidth
  translation wraps and aligns correctly. Bundled translations render
  identically.
- Goto / Find honour `Ctrl-U` (clear) and no longer insert a literal letter for
  `Ctrl`-modified keys.
- The too-narrow compare-pane refusal is a red warning pill naming the width
  needed ("Too narrow — need 85+ cols (have 80)") instead of a blink-and-miss
  hint; transient status messages now expire on time under continuous mouse
  input.
- The daily-verse lookup surfaces real database errors instead of swallowing
  them; an unreadable entry in the translations directory no longer aborts
  first launch.
- Removed the dead `F10` / `open_menu` action (the menu it opened is long
  gone). An existing `open_menu` line in `config.toml` is stripped with a
  warning rather than resetting the config.

### Changed

- The Find dialog caches per-hit row measurements across redraws, and the
  chapter renderer allocates per verse only when it must — less work per frame.
- `turbo-bible --help` gained a long description and worked examples; the
  README gained Install, Uninstall, Features, and Troubleshooting sections.
- Data pipeline: builds are reproducible (one deterministic `built_at`, from
  `SOURCE_DATE_EPOCH` or the scrollmapper commit date), all 66 canonical books
  are asserted present, and foreign keys are actually enforced.

### Security

- `anyhow` bumped to 1.0.104 for RUSTSEC-2026-0190 (unsound
  `downcast_mut` after `context`; not reachable from this codebase).
- The clipboard dependency (`arboard`) drops its unused image support, removing
  the `image` / `tiff` dependency subtree.

## [0.2.0] - 2026-05-31

### Added

- **`turbo-bible import`** — build and install a custom translation from a JSON
  file (books → chapters → verses keyed by OSIS code or English book name),
  without the data pipeline. It compiles a SQLite DB alongside the bundled
  translations, selectable via `--translation` and in the `t` picker.
- **Side-by-side compare panes.** vim-style `Ctrl-W` window-splits read several
  translations — or a cross-referenced passage — at once; each pane is an
  independent reader with its own translation, position, cursor, scroll, and
  visual selection. `s` in the `K` cross-reference popup opens the selected xref
  in a new pane.
- **Word-level diff highlighting across compare panes.** When two or more panes
  show the same passage in the same language, the words that diverge between
  them are emphasised so the wordings that part company stand out at a glance.
  On by default (`[reading] compare_word_diff`), toggled per session with
  `Ctrl-W d`, and themed by a dedicated `diff_word` palette slot.
- **Mouse-driven verse selection** in the reading view and on the splash: click
  to move the cursor, click-drag to select a range (auto-scrolling at the
  edges), shift-click to extend the selection, the scroll wheel to scroll, and a
  click on a splash book to open it.
- **Notify-only update banner** on the splash: checks GitHub for a newer release
  at most once every 24 hours and surfaces the right upgrade command for how the
  copy was installed (brew / cargo / curl). It never downloads or replaces the
  binary, and is suppressed by `[updates] check = false`, `TB_NO_UPDATE_CHECK`,
  or `CI`.
- **Homebrew tap** as a third install method (`brew install` from
  `mathiasror/tap`).
- **Off-event-loop translation downloads** — fetching the non-bundled
  translations and the cross-references DB now happens on a background thread, so
  the UI stays responsive and download outcomes (and distinct failure modes) are
  surfaced in-TUI.
- **Poetry-passage indent** — known poetic passages (Psalms, Proverbs, Song of
  Solomon, Lamentations, and Job's dialogue) get a whole-verse left indent to set
  them apart from prose.
- **Localized book-name labels** for German / French / Portuguese / Latin, plus
  Portuguese and Latin testament headings on the splash.
- **Viewport-sized page motion** — `Ctrl-D` / `Ctrl-U` / `Ctrl-F` / `Ctrl-B`
  now scale their half-/full-page jumps to the number of visible rows.

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
  tokenizer, and a prefix index (shipped prebuilt in each translation
  database — no runtime rebuild).
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

[Unreleased]: https://github.com/mathiasror/turbo-bible/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/mathiasror/turbo-bible/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/mathiasror/turbo-bible/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mathiasror/turbo-bible/releases/tag/v0.1.0
