//! Extract the binary's bundled `.db.zst` assets (see
//! [`crate::bundled`]) into `paths::translations_dir()` on first
//! launch, plus the `turbo-bible install` CLI handler.
//!
//! Idempotent: a `.db` that already exists is left alone unless
//! `--force` is passed. The data pipeline (Phase B) owns schema and
//! FTS optimisation, so no post-extract work is needed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::bundled::{BUNDLED, BUNDLED_XREFS, BundledAsset};
use crate::paths;

/// CLI args for `turbo-bible install`.
#[derive(Debug, clap::Args)]
pub struct InstallArgs {
    /// Re-extract bundled translations even if `<code>.db` already exists.
    #[arg(long)]
    pub force: bool,
    /// Override `paths::translations_dir()` (for tests and dev).
    #[arg(long)]
    pub translations_dir: Option<PathBuf>,
}

/// CLI entry point for `turbo-bible install`.
///
/// # Errors
/// Propagates IO and zstd-decode failures.
pub fn run(args: &InstallArgs) -> Result<()> {
    let target = resolve_dir(args.translations_dir.as_deref())?;
    let stats = extract_into(&target, args.force)?;
    eprintln!(
        "install: {} extracted, {} skipped (already present), {} total",
        stats.extracted, stats.skipped, stats.total
    );
    Ok(())
}

/// Startup hook: ensure every bundled translation has been decompressed
/// into `target_dir`. Missing files are extracted; existing files are
/// left alone.
///
/// # Errors
/// Propagates IO and zstd-decode failures.
pub fn ensure_installed(target_dir: &Path) -> Result<InstallStats> {
    extract_into(target_dir, false)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InstallStats {
    pub extracted: usize,
    pub skipped: usize,
    pub total: usize,
}

fn extract_into(target_dir: &Path, force: bool) -> Result<InstallStats> {
    fs::create_dir_all(target_dir).with_context(|| format!("create {}", target_dir.display()))?;

    let mut stats = InstallStats::default();
    for asset in BUNDLED {
        if extract_asset(target_dir, asset, force)? {
            stats.extracted += 1;
        } else {
            stats.skipped += 1;
        }
        stats.total += 1;
    }
    if extract_asset(target_dir, &BUNDLED_XREFS, force)? {
        stats.extracted += 1;
    } else {
        stats.skipped += 1;
    }
    stats.total += 1;
    Ok(stats)
}

/// Returns `true` if the asset was decompressed and written, `false`
/// if it was skipped because the target file already exists.
fn extract_asset(target_dir: &Path, asset: &BundledAsset, force: bool) -> Result<bool> {
    let final_path = target_dir.join(format!("{}.db", asset.code));
    if !force && final_path.exists() {
        return Ok(false);
    }

    let decoded = zstd::decode_all(io::Cursor::new(asset.bytes))
        .with_context(|| format!("decompress {}.db.zst", asset.code))?;

    // Atomic-rename via a sibling tempfile so a partial extract never
    // leaves a half-written `<code>.db` for the runtime to ATTACH.
    let tmp = tempfile::NamedTempFile::new_in(target_dir)
        .with_context(|| format!("create temp file in {}", target_dir.display()))?;
    fs::write(tmp.path(), &decoded)
        .with_context(|| format!("write decompressed {}.db", asset.code))?;
    tmp.persist(&final_path)
        .with_context(|| format!("persist {}.db at {}", asset.code, final_path.display()))?;
    Ok(true)
}

fn resolve_dir(override_path: Option<&Path>) -> Result<PathBuf> {
    match override_path {
        Some(p) => Ok(p.to_path_buf()),
        None => paths::translations_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, OpenFlags};

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    #[test]
    fn first_run_extracts_all_bundled_assets() {
        let dir = tempdir();
        let stats = ensure_installed(dir.path()).expect("ensure_installed");
        // 11 translations + 1 xrefs.db
        assert_eq!(stats.extracted, BUNDLED.len() + 1);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.total, BUNDLED.len() + 1);
        for asset in BUNDLED.iter().chain(std::iter::once(&BUNDLED_XREFS)) {
            let p = dir.path().join(format!("{}.db", asset.code));
            assert!(p.is_file(), "missing {}", p.display());
        }
    }

    #[test]
    fn second_run_is_a_no_op() {
        let dir = tempdir();
        let _ = ensure_installed(dir.path()).expect("first run");
        let stats = ensure_installed(dir.path()).expect("second run");
        assert_eq!(stats.extracted, 0);
        assert_eq!(stats.skipped, BUNDLED.len() + 1);
    }

    #[test]
    fn force_re_extracts_everything() {
        let dir = tempdir();
        let _ = ensure_installed(dir.path()).expect("first run");
        let stats = extract_into(dir.path(), true).expect("force re-extract");
        assert_eq!(stats.extracted, BUNDLED.len() + 1);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn extracted_dbs_have_expected_invariants() {
        let dir = tempdir();
        ensure_installed(dir.path()).expect("install");
        for asset in BUNDLED {
            let p = dir.path().join(format!("{}.db", asset.code));
            let conn =
                Connection::open_with_flags(&p, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open");
            // 66 protocanonical books.
            let books: i64 = conn
                .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
                .expect("count book");
            assert_eq!(books, 66, "{}.db book count", asset.code);
            let labels: i64 = conn
                .query_row("SELECT COUNT(*) FROM book_label", [], |r| r.get(0))
                .expect("count book_label");
            assert_eq!(labels, 66, "{}.db book_label count", asset.code);
            // meta.verse_count agrees with the verse table.
            let meta_count: i64 = conn
                .query_row("SELECT verse_count FROM meta", [], |r| r.get(0))
                .expect("meta.verse_count");
            let actual: i64 = conn
                .query_row("SELECT COUNT(*) FROM verse", [], |r| r.get(0))
                .expect("verse count");
            assert_eq!(
                meta_count, actual,
                "{}.db meta.verse_count mismatch",
                asset.code
            );
        }
        // xrefs.db has its own shape.
        let xrefs = dir.path().join("xrefs.db");
        let conn = Connection::open_with_flags(&xrefs, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open xrefs");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM xref", [], |r| r.get(0))
            .expect("xref count");
        assert!(n > 100_000, "xrefs.db has {n} rows, expected >100k");
    }
}
