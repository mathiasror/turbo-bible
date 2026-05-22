//! Minimal library target. The application is in `src/main.rs`
//! (the binary target); this lib exists so integration tests in
//! `tests/` can reference symbols that would otherwise be local to
//! the binary — `cargo test` doesn't link binary targets, so a
//! `bin`-only crate can't share state with its `tests/`.
//!
//! Today the surface is empty. Resist the temptation to grow it:
//! every `pub` here lands in the rustdoc on the lib target and on
//! docs.rs once the crate is published.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
