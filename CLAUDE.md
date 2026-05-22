# turbo-bible — Claude project notes

Terminal Bible reader in Rust (ratatui + crossterm, FTS5 via rusqlite).
See @README.md for the feature tour and full keymap, and
@CONTRIBUTING.md for the contributor workflow. This file captures only
what Claude can't infer from the code itself.

## Workspace layout

Cargo workspace under `crates/`:

- `crates/turbo-bible-tui/` — the TUI binary (`cargo run -p turbo-bible`).
  Houses every file that used to live in `src/` and `tests/` at the
  repo root.
- `crates/turbo-bible-data/` — offline data pipeline
  (`cargo run -p turbo-bible-data`). Builds per-translation SQLite
  files from a local `scrollmapper/bible_databases` checkout into
  `dist/translations/`. Does not touch `~/.local/share/turbo-bible/`.
  Output is copied into `crates/turbo-bible-tui/assets/` by
  `just bundle-translations` so the TUI's `include_bytes!` macros
  have something to embed.

`website/` is a hand-authored static site (no SSG, no build step);
GitHub Pages deploy not wired up yet.

## Bundle dataflow

```
scrollmapper checkout
        │
        ▼
crates/turbo-bible-data  ──build──▶  dist/build/*.db
        │                                  │
        │                              compress
        ▼                                  ▼
dist/translations/*.db.zst   ──just bundle-translations──▶
        │
        ▼
crates/turbo-bible-tui/assets/*.db.zst
        │
        ▼ include_bytes!
turbo-bible binary
        │
        ▼ install::ensure_installed (first launch)
~/.local/share/turbo-bible/translations/*.db
        │
        ▼ Db::open_ro
runtime queries
```

## The dev loop

- CI gate is **`just check`** = `cargo fmt --check` + `cargo clippy
  --workspace --all-targets --all-features -- -D warnings` + `cargo
  test --workspace --all-features`. Locally, run `just check && just
  audit && just deny` before opening a PR — same jobs CI runs.
- `just baseline` is the rust-review skill's input; only run when
  explicitly doing a review pass (writes logs to `target/rust-review/`).

## Conventions

- **Errors:** `anyhow::Result` at module boundaries; use `.context(...)`
  for useful frames. No `thiserror` enums — the binaries are the only
  consumers.
- **`#[allow(dead_code)]`:** must come with a one-line justification.
  See `crates/turbo-bible-tui/src/db.rs` and
  `crates/turbo-bible-tui/src/theme.rs` for the pattern.
- **Style:** default `rustfmt` config. Run `just fmt` before pushing;
  CI rejects diffs.

## Architecture quirks

- **The TUI binary embeds all 11 translations + xrefs** as
  zstd-compressed bytes via `include_bytes!` in
  `crates/turbo-bible-tui/src/bundled.rs`. First launch decompresses
  them into `~/.local/share/turbo-bible/translations/`. The
  `crates/turbo-bible-tui/assets/` directory is gitignored and
  populated by `just bundle-translations` — building the binary with
  an empty `assets/` fails loudly at the `include_bytes!` site.
- **One `Connection` per translation.** `Db::open_ro` opens N read-only
  connections (one per `<code>.db`), each with the shared `xrefs.db`
  ATTACHed under alias `xrefs`. SQLite's compile-time
  `SQLITE_MAX_ATTACHED` is 10, so we can't fit 11 translations + xrefs
  in a single connection. Translation tables are referenced
  unqualified (`verse`, `verse_fts`, ...); xref is `xrefs.xref`.
- **`crates/turbo-bible-tui/src/lib.rs` is intentionally empty.** It
  exists only so integration tests in
  `crates/turbo-bible-tui/tests/` can reference shared symbols
  (`cargo test` doesn't link binary targets). Resist growing the
  surface — every `pub` here lands on docs.rs.
- **`crates/turbo-bible-tui/tests/e2e.rs` is self-contained**: each
  test sets `HOME` to a fresh tempdir, the TUI's auto-install
  populates `<tmp>/.local/share/turbo-bible/translations/`, and the
  PTY drives the binary from there. No developer-DB precondition.

## Generated artifacts

- `demo/demo.gif` → regenerate via `just demo` (requires `vhs`).
- `docs/screenshots/*.png` → regenerate via `just screenshots`.
- Don't hand-edit either; the `.tape` files under `demo/` are the
  source.

## Dependencies

- New crates must satisfy `deny.toml` (licenses, bans, sources). Run
  `just deny` locally before committing the `Cargo.lock` change.
- The Turbo Vision look depends on 24-bit RGB + the `▒` glyph; theme
  changes should be eyeballed in a modern terminal (iTerm2, Ghostty,
  Alacritty, WezTerm) before shipping.
