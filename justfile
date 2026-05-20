# turbo-bible task runner. Requires `just` (https://just.systems).
#
# `just` (no args) lists recipes. `just check` is what CI runs.

# Show available recipes
default:
    @just --list

# What CI runs: fmt + clippy + tests.
check: fmt-check lint test

# Format every Rust source file in place.
fmt:
    cargo fmt --all

# Check formatting without changing files (CI uses this).
fmt-check:
    cargo fmt --all -- --check

# Clippy with the same flags as CI.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Apply clippy's suggested autofixes (writes to disk; review before commit).
lint-fix:
    cargo clippy --fix --all-targets --all-features --allow-dirty -- -D warnings

# Unit + integration tests. PTY tests skip without a populated bible.sqlite.
test:
    cargo test --all-features

# Build a release binary.
build:
    cargo build --release

# cargo audit; requires `cargo install cargo-audit`.
audit:
    cargo audit

# Run the full lint+audit+test baseline the rust-review skill uses.
# Writes per-step logs to target/rust-review/.
baseline:
    mkdir -p target/rust-review
    cargo build --all-targets --all-features 2>&1 | tee target/rust-review/build.log
    cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tee target/rust-review/clippy.log
    cargo clippy --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery 2>&1 | tee target/rust-review/clippy-pedantic.log
    cargo doc --no-deps --all-features 2>&1 | tee target/rust-review/doc.log
    cargo test --all-features --no-run 2>&1 | tee target/rust-review/test-build.log
    cargo audit 2>&1 | tee target/rust-review/audit.log
    cargo tree -d 2>&1 | tee target/rust-review/tree-dupes.log

# Launch the TUI with the project's default DB resolution.
run *args:
    cargo run --release -- {{args}}
