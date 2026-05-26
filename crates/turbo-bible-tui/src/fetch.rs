//! On-demand download of translations and the cross-references DB
//! from GitHub Releases. Only `en-kjv` is embedded in the binary; the
//! other ten translations and `xrefs.db` are fetched the first time
//! the user opens them.
//!
//! Wire format: each asset is a zstd-compressed SQLite file. The
//! compile-time manifest ([`crate::manifest`]) carries the SHA-256 of
//! the *decompressed* bytes — verified before the file is moved into
//! the user's translations directory. A failed integrity check leaves
//! nothing partial on disk.
//!
//! Network: we shell out to `curl`. The install.sh already requires
//! it, so it's already present on every supported platform, and
//! avoiding an HTTP crate keeps the dependency tree small.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

use crate::manifest::{TranslationManifestEntry, XREFS, XrefsManifestEntry};

/// Base URL the binary fetches from. Override with `TB_RELEASE_URL`
/// for testing against a local mirror or a pre-release tag.
fn base_url() -> String {
    if let Ok(u) = std::env::var("TB_RELEASE_URL") {
        return u.trim_end_matches('/').to_string();
    }
    format!(
        "https://github.com/mathiasror/turbo-bible/releases/download/v{}",
        env!("CARGO_PKG_VERSION")
    )
}

/// Download a translation `<code>.db.zst` and install it as
/// `<translations_dir>/<code>.db`. No-op if already installed.
///
/// # Errors
/// - Translation code not in the manifest.
/// - `curl` missing or download failed (no network, 404).
/// - SHA-256 mismatch between the decompressed bytes and the manifest.
/// - IO failure when writing to `translations_dir`.
pub fn translation(translations_dir: &Path, code: &str) -> Result<()> {
    let entry = TranslationManifestEntry::by_code(code)
        .ok_or_else(|| anyhow!("unknown translation code: {code}"))?;
    let dest = translations_dir.join(format!("{}.db", entry.code));
    if dest.exists() {
        return Ok(());
    }
    fetch_and_install(translations_dir, entry.file, entry.sha256, &dest)
        .with_context(|| format!("fetch translation {}", entry.code))
}

/// Download the cross-references DB and install it as
/// `<translations_dir>/xrefs.db`. No-op if the real DB is already in
/// place. The install-time empty stand-in is overwritten in place.
#[allow(
    dead_code,
    reason = "wired in once the K-popup learns to fetch xrefs on demand"
)]
pub fn xrefs(translations_dir: &Path) -> Result<()> {
    xrefs_with(translations_dir, &XREFS)
}

fn xrefs_with(translations_dir: &Path, entry: &XrefsManifestEntry) -> Result<()> {
    let dest = translations_dir.join("xrefs.db");
    fetch_and_install(translations_dir, entry.file, entry.sha256, &dest).context("fetch xrefs.db")
}

fn fetch_and_install(
    translations_dir: &Path,
    asset_file: &str,
    expected_sha256: &str,
    final_path: &Path,
) -> Result<()> {
    fs::create_dir_all(translations_dir)
        .with_context(|| format!("create {}", translations_dir.display()))?;

    let url = format!("{}/{}", base_url(), asset_file);

    // Download into a tempfile in the translations dir (same FS as
    // the final destination, so the rename is atomic).
    let dl = tempfile::NamedTempFile::new_in(translations_dir)
        .with_context(|| format!("create temp file in {}", translations_dir.display()))?;

    run_curl(&url, dl.path()).with_context(|| format!("download {url}"))?;

    let compressed =
        fs::read(dl.path()).with_context(|| format!("read {}", dl.path().display()))?;
    let decoded = zstd::decode_all(io::Cursor::new(&compressed))
        .with_context(|| format!("decompress {asset_file}"))?;

    let actual = hex_sha256(&decoded);
    if !ct_eq(actual.as_bytes(), expected_sha256.as_bytes()) {
        return Err(anyhow!(
            "sha256 mismatch for {asset_file}: expected {expected_sha256}, got {actual}"
        ));
    }

    // Stage the decompressed bytes into another tempfile (same dir
    // for atomic-rename), then move into place.
    let staged = tempfile::NamedTempFile::new_in(translations_dir)
        .with_context(|| format!("create temp file in {}", translations_dir.display()))?;
    fs::write(staged.path(), &decoded)
        .with_context(|| format!("write decompressed {asset_file}"))?;
    staged
        .persist(final_path)
        .with_context(|| format!("persist {}", final_path.display()))?;
    Ok(())
}

fn run_curl(url: &str, dest: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--retry",
            "3",
            "--output",
        ])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .status()
        .context("spawn curl (is it installed?)")?;
    if !status.success() {
        return Err(anyhow!("curl exited with {status}"));
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push(NIBBLE[(b >> 4) as usize] as char);
        s.push(NIBBLE[(b & 0x0F) as usize] as char);
    }
    s
}

const NIBBLE: &[u8; 16] = b"0123456789abcdef";

/// Constant-time string comparison. Defensive — the SHA-256 hex
/// comparison would be safe under fast string equality too, but
/// this keeps the integrity-check path side-channel-free.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Compute the URL the binary would fetch for `code`. Useful for
/// surfacing "you can download this manually from <url>" hints in
/// the UI when network is unreachable.
#[allow(
    dead_code,
    reason = "wired in once we add an offline-error dialog with a manual-download hint"
)]
pub fn translation_url(code: &str) -> Option<String> {
    TranslationManifestEntry::by_code(code).map(|t| format!("{}/{}", base_url(), t.file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_kjv_sha_round_trips() {
        // Sanity: the embedded KJV's decompressed sha256 matches the
        // manifest entry. This guards against the build.rs codegen
        // drifting from `assets/en-kjv.db.zst`.
        let kjv = TranslationManifestEntry::by_code("en-kjv").expect("kjv in manifest");
        let raw = crate::bundled::BUNDLED
            .iter()
            .find(|a| a.code == "en-kjv")
            .expect("kjv in bundled")
            .bytes;
        let decoded = zstd::decode_all(io::Cursor::new(raw)).expect("decompress");
        assert_eq!(hex_sha256(&decoded), kjv.sha256);
    }

    #[test]
    fn ct_eq_basics() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }
}
