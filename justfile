# turbo-bible task runner. Requires `just` (https://just.systems).
#
# `just` (no args) lists recipes. `just check` is what CI runs.

# Show available recipes
default:
    @just --list

# What CI runs: fmt + clippy + tests across the whole workspace.
check: fmt-check lint test

# Format every Rust source file in place.
fmt:
    cargo fmt --all

# Check formatting without changing files (CI uses this).
fmt-check:
    cargo fmt --all -- --check

# Clippy with the same flags as CI.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Apply clippy's suggested autofixes (writes to disk; review before commit).
lint-fix:
    cargo clippy --workspace --fix --all-targets --all-features --allow-dirty -- -D warnings

# Unit + integration tests. PTY tests skip without a populated bible.sqlite.
test:
    cargo test --workspace --all-features

# Build a release binary.
build:
    cargo build --workspace --release

# cargo audit; requires `cargo install cargo-audit`.
audit:
    cargo audit

# cargo deny: license, bans, sources, advisories policy in deny.toml.
# Requires `cargo install cargo-deny`.
deny:
    cargo deny check

# Run the full lint+audit+test baseline the rust-review skill uses.
# Writes per-step logs to target/rust-review/.
baseline:
    mkdir -p target/rust-review
    cargo build --workspace --all-targets --all-features 2>&1 | tee target/rust-review/build.log
    cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 | tee target/rust-review/clippy.log
    cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery 2>&1 | tee target/rust-review/clippy-pedantic.log
    cargo doc --workspace --no-deps --all-features 2>&1 | tee target/rust-review/doc.log
    cargo test --workspace --all-features --no-run 2>&1 | tee target/rust-review/test-build.log
    cargo audit 2>&1 | tee target/rust-review/audit.log
    cargo tree -d 2>&1 | tee target/rust-review/tree-dupes.log

# Launch the TUI with the project's default DB resolution.
run *args:
    cargo run -p turbo-bible --release -- {{args}}

# Data pipeline shortcuts. The first positional argument points at a
# local scrollmapper/bible_databases checkout; remaining `*args` are
# forwarded to the subcommand (e.g. `just data-audit -- --out a.csv`).
data-audit scrollmapper="data/scrollmapper-checkout" *args="":
    cargo run -p turbo-bible-data -- audit-licenses --scrollmapper {{scrollmapper}} {{args}}

data-build scrollmapper="data/scrollmapper-checkout" *args="":
    cargo run -p turbo-bible-data -- build --scrollmapper {{scrollmapper}} --manifest data/manifest_source.toml {{args}}

data-compress *args="":
    cargo run -p turbo-bible-data -- compress {{args}}

# Build the data pipeline output and stage the TUI's assets/ dir so
# build.rs + include_bytes! in src/bundled.rs have fresh inputs.
# Only KJV is embedded in the binary; the manifest lets the binary
# discover the rest at runtime and fetch from GitHub Releases.
# Required before `cargo build -p turbo-bible` if assets/ is empty.
bundle-translations scrollmapper="data/scrollmapper-checkout":
    cargo run -p turbo-bible-data --release -- build --scrollmapper {{scrollmapper}} --manifest data/manifest_source.toml
    cargo run -p turbo-bible-data --release -- compress
    mkdir -p crates/turbo-bible-tui/assets
    cp dist/translations/en-kjv.db.zst crates/turbo-bible-tui/assets/
    cp dist/translations/manifest.json crates/turbo-bible-tui/assets/

# Re-record the README demo GIF. Requires `vhs` (https://github.com/charmbracelet/vhs).
demo:
    cargo build -p turbo-bible --release
    vhs demo/demo.tape

# Re-render the labelled screenshots under docs/screenshots/.
screenshots:
    cargo build -p turbo-bible --release
    vhs demo/screenshots.tape

# Re-render website/og-image.png (the 1200x630 social card) — a real VHS
# capture of the splash, same toolchain as `demo` / `screenshots`.
og-image:
    cargo build -p turbo-bible --release
    vhs demo/og-image.tape

# Re-render website/apple-touch-icon.png (the 180x180 home-screen icon). Requires Pillow.
apple-touch-icon:
    python3 demo/apple-touch-icon.py
