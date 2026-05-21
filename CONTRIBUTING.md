# Contributing to turbo-bible

## Prerequisites

- Rust stable (the project tracks the latest stable via
  `rust-toolchain.toml`; the MSRV gate is `rust-version = "1.88"` in
  `Cargo.toml`).
- `just` task runner — `cargo install just` or
  `brew install just`. Optional but convenient.
- `cargo-audit` for the audit recipe — `cargo install cargo-audit`.
- To populate `bible.sqlite` from scrollmapper, run `turbo-bible import`
  (network required).

## Day-to-day

```sh
just check        # what CI runs: fmt + clippy + tests
just fmt          # apply rustfmt
just lint         # clippy -D warnings
just lint-fix     # apply clippy's suggested autofixes
just test         # cargo test --all-features
just audit        # cargo audit
just baseline     # the rust-review baseline; writes target/rust-review/*.log
just run          # cargo run --release
just run --book JHN --chapter 3
```

If you can't / don't want to install `just`, every recipe is a thin shell
wrapper around `cargo` — copy the relevant line out of the `justfile`.

## CI gate

`.github/workflows/ci.yml` runs on every push to `main` and every PR:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- `cargo audit` (separate job)

Pull requests need to be green before merge. The same gate runs locally
via `just check`.

## Tests

- Unit tests live next to the code in `#[cfg(test)] mod tests` blocks.
- Integration tests live in `tests/e2e.rs` and drive the real binary
  over a PTY via `rexpect`. They look for
  `~/.local/share/turbo-bible/bible.sqlite` and **skip themselves**
  rather than fail if it isn't present, so they're fine to run on a
  clean machine — they just won't add coverage there.

To populate the DB for the e2e tests:

```sh
cargo run --release -- import
```

## Style

- Default `rustfmt` config. Run `just fmt` before pushing; CI will
  reject diffs otherwise.
- New `#[allow(dead_code)]` markers need a one-line justification
  comment — see `src/db.rs` / `src/theme.rs` for the pattern.
- Errors at module boundaries flow through `anyhow::Result`; the
  binary is the only consumer so there's no need for `thiserror`
  enums today. Use `.context(...)` to add useful frames.

## Filing issues

When reporting a bug, include:

- The translation code you were reading (`en-kjv`, `nb-1930`,
  `es-rv1909`, ...).
- Terminal + OS (Turbo Vision-style rendering depends on 24-bit RGB
  and the `▒` glyph — recent terminals render it cleanly; older
  Windows console may not).
- Output of `cargo --version` and `rustc --version`.
- Steps to reproduce. The state files
  (`~/.config/turbo-bible/{state,config,bookmarks}.toml`) often help.
