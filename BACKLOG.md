# Backlog

Planned work captured between sessions. Take an item, do it, delete its
entry.

## Port `scripts/import_translations.py` into a Rust subcommand

Replace the sibling Python script with `turbo-bible import [--only ...]`
so the project ships as a single binary with no Python dependency.

### Outline

- New `src/import.rs` containing:
  - `const SCROLLMAPPER_COMMIT: &str` (pinned)
  - `const SOURCES: &[(code, file, name, license, language)]`
  - `const SCROLLMAPPER_NAME_TO_OSIS: &[(&str, &str)]`
  - `const KJV_LABELS / NB_1930_LABELS / ES_RV1909_LABELS: &[(osis,
    name, abbrev)]`
  - `const SCHEMA_SQL: &str` (lift from `import_translations.py`)
- New CLI subcommand. Cleanest with `clap`'s `#[derive(Subcommand)]`:
  - `turbo-bible run` (current behaviour, default when no subcommand)
  - `turbo-bible import [--only code,code] [--db PATH] [--backup-dir
    PATH] [--cache-dir PATH] [--no-backup] [--backup-only]`
- HTTP download with `ureq` (smaller than `reqwest`; blocking is the
  natural fit for a one-shot script).
- Reuse existing rusqlite handle and `ensure_fts_optimized` — no new
  DB-access code needed.
- Backup step (`sqlite3 .dump` equivalent): rusqlite doesn't expose
  `iterdump`; either port the iterdump Python implementation
  (~80 lines) or shell out to `sqlite3` if installed. Defer the
  decision until porting; the legacy `nb-2024` dump is a one-time
  artifact and can stay where it is.
- On launch, when `db_path` doesn't exist, prompt: "No translations
  installed. Press `i` to import KJV, Norsk 1930, RV1909 (~6 MB)."
  This is Slice D of the original plan
  (`~/.claude/plans/my-idea-is-to-foamy-dawn.md`).

### Tradeoffs

- **Pros**: single distributable; no Python toolchain; the empty-DB
  bootstrap path becomes ergonomic.
- **Cons**: ~200 LoC, adds `ureq` (~50 KB) as a runtime dep; legacy
  `crawl.py` and `scripts/import_translations.py` get deprecated.

### Acceptance

- `turbo-bible import` populates `~/.local/share/turbo-bible/bible.sqlite`
  with all three translations, matching what the Python script produces
  byte-for-byte (verse counts, FTS index version, book labels).
- A fourth e2e test covers `turbo-bible import` end-to-end against a
  temp HOME (network access required; mark `#[ignore]` for offline CI
  or use the existing `~/.cache/turbo-bible/scrollmapper/` cache).
- README references `turbo-bible import` instead of the Python script.
- `scripts/import_translations.py` and `crawl.py` either deleted or
  archived under `legacy/`.

### Effort

1–2 hours; mostly mechanical translation.
