//! XDG path resolution for the binary. Centralises the
//! `~/.config/turbo-bible/` and `~/.local/share/turbo-bible/` joins so
//! the three persistence modules (config, state, bookmark) don't each
//! reinvent it.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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

/// Write `bytes` to `path` atomically: stage into a sibling temp file in the
/// same directory, then `persist` (rename) over the target. A crash, power
/// loss, or `ENOSPC` mid-write therefore can never leave a truncated or empty
/// file at `path` — a reader sees either the old contents or the complete new
/// ones, never a partial write. This is the durability guarantee the
/// persistence modules (config, state, bookmarks, update cache) rely on so a
/// botched save can't destroy the user's data; it mirrors the staging that
/// `fetch`/`install` already do for downloaded databases.
///
/// # Errors
/// Fails if the temp file can't be created in `path`'s directory, the staged
/// write fails (e.g. disk full), or the atomic rename onto `path` fails.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    // Same directory as the target so `persist` is a rename within one
    // filesystem (atomic), not a cross-device copy.
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("create temp file in {}", dir.display()))?;
    tmp.write_all(bytes)
        .with_context(|| format!("write staged contents for {}", path.display()))?;
    tmp.persist(path)
        .with_context(|| format!("persist {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_then_overwrites_without_leaking_temps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.toml");

        atomic_write(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");

        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");

        // The staged temp file must be renamed away, never left behind.
        let entries = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(entries, 1, "atomic_write leaked a temp file");
    }
}
