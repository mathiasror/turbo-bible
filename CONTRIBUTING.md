# Contributing to turbo-bible

## Workspace layout

```
crates/
  turbo-bible-tui/    # the TUI binary
  turbo-bible-data/   # offline data pipeline (scrollmapper -> .db.zst)
website/              # hand-authored static site (GitHub Pages, no SSG)
```

The repo is a Cargo workspace; the root `Cargo.toml` only carries
`[workspace]` plumbing. Shared deps are pinned in
`[workspace.dependencies]` and the member crates inherit via
`{ workspace = true }`.

## Prerequisites

- Rust stable (the project tracks the latest stable via
  `rust-toolchain.toml`; the MSRV gate is `rust-version = "1.88"` in
  the root `Cargo.toml`).
- `just` task runner — `cargo install just` or
  `brew install just`. Optional but convenient.
- `cargo-audit` for the audit recipe — `cargo install cargo-audit`.
- `cargo-deny` for the license / bans / sources policy — `cargo install cargo-deny`.
- To build the TUI binary you need
  `crates/turbo-bible-tui/assets/*.db.zst` populated. The recipe
  `just bundle-translations [path/to/scrollmapper/checkout]` runs the
  data pipeline end-to-end and copies the resulting `.db.zst` files
  into the assets directory. Bundle once, then build many times.

## Day-to-day

```sh
just check        # what CI runs: fmt + clippy + tests (workspace-wide)
just fmt          # apply rustfmt
just lint         # clippy --workspace -D warnings
just lint-fix     # apply clippy's suggested autofixes
just test         # cargo test --workspace --all-features
just audit        # cargo audit
just deny         # cargo deny check (license + duplicate-version + source policy)
just baseline     # the rust-review baseline; writes target/rust-review/*.log
just run          # cargo run -p turbo-bible --release
just run --book JHN --chapter 3
just data-build   # cargo run -p turbo-bible-data -- build ...
```

If you can't / don't want to install `just`, every recipe is a thin shell
wrapper around `cargo` — copy the relevant line out of the `justfile`.

## CI gate

`.github/workflows/ci.yml` runs on every push to `main` and every PR:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `cargo audit` (separate job, also runs weekly via `schedule:`)
- `cargo deny check advisories bans licenses sources` (separate job;
  policy in `deny.toml`)

Pull requests need to be green before merge. The same gate runs locally
via `just check && just audit && just deny`.

## Tests

- Unit tests live next to the code in `#[cfg(test)] mod tests` blocks.
- Integration tests live in `crates/turbo-bible-tui/tests/e2e.rs` and
  drive the real binary over a PTY via `rexpect`. Each test sets
  `HOME` to a fresh tempdir and the TUI auto-extracts the bundled
  translations into it — no developer-DB precondition.
- The data pipeline has an `--ignored` end-to-end test at
  `crates/turbo-bible-data/tests/pipeline.rs` that requires a local
  scrollmapper checkout (point `TURBO_BIBLE_SCROLLMAPPER` at one, or
  default to `~/git/oss/bible_databases`). Run with
  `cargo test --workspace -- --ignored`.

## Style

- Default `rustfmt` config. Run `just fmt` before pushing; CI will
  reject diffs otherwise.
- New `#[allow(dead_code)]` markers need a one-line justification
  comment — see `crates/turbo-bible-tui/src/db.rs` /
  `crates/turbo-bible-tui/src/theme.rs` for the pattern.
- Errors at module boundaries flow through `anyhow::Result`; the
  binaries are the only consumers so there's no need for `thiserror`
  enums today. Use `.context(...)` to add useful frames.

## Cutting a release

Releases are tag-driven. `.github/workflows/release.yml` watches for
tags matching `v*` and runs three things in parallel/serial:

1. Build prebuilt binaries for five targets (x86_64 + aarch64 Linux,
   x86_64 + aarch64 macOS, x86_64 Windows). Each build job clones
   `scrollmapper/bible_databases` at the pinned `SCROLLMAPPER_REF`
   env var in the workflow, runs `just bundle-translations` to
   populate `crates/turbo-bible-tui/assets/`, then `cargo build
   --release`. Tarballs land as release assets, named
   `turbo-bible-<target>.{tar.gz,zip}`.
2. Publish `turbo-bible` to crates.io. Requires a `CRATES_IO_TOKEN`
   secret in the repo settings (Settings → Secrets and variables →
   Actions). Same pre-step bundles translations into the published
   tarball — they're gitignored locally but pulled in via the
   `include = [...]` field in `crates/turbo-bible-tui/Cargo.toml`.
3. `website/install.sh` is hand-authored and downloads whichever
   tarball matches the running platform from
   `releases/latest/download/`. The Pages workflow keeps it served
   from `turbo.bible/install.sh`.

To cut `v0.1.0`:

```sh
# 1. Bump version in crates/turbo-bible-tui/Cargo.toml
# 2. just check  (sanity-pass locally)
# 3. git commit -am 'release: v0.1.0'
# 4. git tag v0.1.0
# 5. git push origin main --tags
```

The release workflow takes ~15 min end-to-end. Watch it in
`/actions`. If a build fails (e.g. scrollmapper schema changed),
fix forward and re-tag a `v0.1.1`; tags are not edited in place.

The future cargo-dist migration: when the hand-rolled workflow
becomes painful, run `cargo install cargo-dist && cargo dist init`.
It generates a `dist-workspace.toml` and a new `release.yml`. Move
the bundle-translations step into the `github-build-setup` config
and delete this file. The install.sh becomes a thin pass-through to
the cargo-dist-generated installer.

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
