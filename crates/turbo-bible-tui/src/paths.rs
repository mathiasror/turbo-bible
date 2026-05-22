//! XDG path resolution for the binary. Centralises the
//! `~/.config/turbo-bible/` and `~/.local/share/turbo-bible/` joins so
//! the three persistence modules (config, state, bookmark) don't each
//! reinvent it.

use std::path::PathBuf;

use anyhow::Result;
use etcetera::{BaseStrategy, choose_base_strategy};

/// `~/.config/turbo-bible/` on Linux / macOS via `etcetera`.
///
/// # Errors
/// Propagates `etcetera::AppStrategyArgs` failures (`HOME` unset on
/// platforms where it's required).
pub fn config_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.config_dir();
    p.push("turbo-bible");
    Ok(p)
}

/// `~/.local/share/turbo-bible/` on Linux / macOS via `etcetera`.
///
/// # Errors
/// Propagates `etcetera::AppStrategyArgs` failures (`HOME` unset on
/// platforms where it's required).
pub fn data_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.data_dir();
    p.push("turbo-bible");
    Ok(p)
}

/// `~/.local/share/turbo-bible/translations/` — per-translation `.db`
/// files plus the shared `xrefs.db`, extracted from the binary's
/// bundled assets on first launch.
///
/// # Errors
/// Propagates `etcetera::AppStrategyArgs` failures.
pub fn translations_dir() -> Result<PathBuf> {
    let mut p = data_dir()?;
    p.push("translations");
    Ok(p)
}
