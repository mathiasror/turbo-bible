# turbo-bible — Claude project notes

Terminal Bible reader in Rust (ratatui + crossterm, FTS5 via rusqlite).
See @README.md for the feature tour and full keymap, and
@CONTRIBUTING.md for the contributor workflow. This file captures only
what Claude can't infer from the code itself.

## The dev loop

- CI gate is **`just check`** = `cargo fmt --check` + `cargo clippy
  --all-targets --all-features -- -D warnings` + `cargo test
  --all-features`. Locally, run `just check && just audit && just deny`
  before opening a PR — same jobs CI runs.
- `just baseline` is the rust-review skill's input; only run when
  explicitly doing a review pass (writes logs to `target/rust-review/`).

## Conventions

- **Errors:** `anyhow::Result` at module boundaries; use `.context(...)`
  for useful frames. No `thiserror` enums — the binary is the only
  consumer.
- **`#[allow(dead_code)]`:** must come with a one-line justification.
  See `src/db.rs` and `src/theme.rs` for the pattern.
- **Style:** default `rustfmt` config. Run `just fmt` before pushing;
  CI rejects diffs.

## Architecture quirks

- **`src/lib.rs` is intentionally near-empty.** It exists only so
  integration tests in `tests/` can reference shared symbols (`cargo
  test` doesn't link binary targets). Every `pub` here lands on
  docs.rs — resist growing the surface.
- **`SCROLLMAPPER_COMMIT` in `src/lib.rs` is the single source of
  truth** for the data-import pin; both `src/import.rs` and
  `tests/import.rs` read it. Bump deliberately and verify the SHA
  points to a real commit.
- **`tests/e2e.rs` skips itself** when
  `~/.local/share/turbo-bible/bible.sqlite` is absent. A green `cargo
  test` with zero e2e cases on a clean machine is expected, not a bug.
  Run `cargo run --release -- import` first if you need real coverage.

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
