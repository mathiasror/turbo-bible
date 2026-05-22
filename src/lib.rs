//! Minimal library target. The application is in `src/main.rs`
//! (the binary target); this lib exists so integration tests in
//! `tests/` can reference symbols that would otherwise be local to
//! the binary — `cargo test` doesn't link binary targets, so a
//! `bin`-only crate can't share state with its `tests/`.
//!
//! Today the entire surface is one constant. Resist the temptation
//! to grow it: every `pub` here lands in the rustdoc on the lib
//! target and on docs.rs once the crate is published.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Pinned `scrollmapper/bible_databases` commit.
///
/// Single source of truth for both the importer (`src/import.rs`) and
/// the integration test (`tests/import.rs`); bump deliberately, verify
/// the SHA matches a real commit before changing.
pub const SCROLLMAPPER_COMMIT: &str = "a228a19a29099a41c196c2a310cd93e50a390e30";
