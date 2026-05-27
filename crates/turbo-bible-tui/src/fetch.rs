//! On-demand download of translations and the cross-references DB
//! from GitHub Releases. Only `en-kjv` is embedded in the binary; the
//! other ten translations and `xrefs.db` are fetched the first time
//! the user opens them.
//!
//! Wire format: each asset is a zstd-compressed `SQLite` file. The
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
use std::io::Read;
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
    fetch_and_install(
        translations_dir,
        entry.file,
        entry.sha256,
        entry.decompressed_size,
        &dest,
    )
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
    fetch_and_install(
        translations_dir,
        entry.file,
        entry.sha256,
        entry.decompressed_size,
        &dest,
    )
    .context("fetch xrefs.db")
}

fn fetch_and_install(
    translations_dir: &Path,
    asset_file: &str,
    expected_sha256: &str,
    expected_decompressed_size: u64,
    final_path: &Path,
) -> Result<()> {
    fs::create_dir_all(translations_dir)
        .with_context(|| format!("create {}", translations_dir.display()))?;

    let url = format!("{}/{}", base_url(), asset_file);

    // Download into a tempfile in the translations dir (same FS as
    // the final destination, so the rename is atomic).
    let dl = tempfile::NamedTempFile::new_in(translations_dir)
        .with_context(|| format!("create temp file in {}", translations_dir.display()))?;

    // Cap the download a little above the asset's decompressed size: the
    // compressed asset is always smaller, so a legitimate file never trips it,
    // but a hostile/MITM server can't stream gigabytes into memory before the
    // hash check runs.
    run_curl(
        &url,
        dl.path(),
        expected_decompressed_size.saturating_add(1 << 20),
    )
    .with_context(|| format!("download {url}"))?;

    let compressed =
        fs::read(dl.path()).with_context(|| format!("read {}", dl.path().display()))?;
    let decoded = decode_and_verify(
        &compressed,
        expected_sha256,
        expected_decompressed_size,
        asset_file,
    )?;

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

/// Decompress a downloaded zstd asset, enforcing the manifest's declared
/// decompressed size as a hard ceiling — so a tampered or corrupt asset can't
/// expand without bound (a zip bomb would OOM the process before the hash is
/// ever checked) — then verify the SHA-256 of the result. The decoded bytes
/// are returned only when both the size and the hash match.
#[allow(
    clippy::redundant_pub_crate,
    reason = "called from another module (install.rs); pub(crate) states the intended crate-internal visibility — redundant_pub_crate is a nursery lint we don't gate on"
)]
pub(crate) fn decode_and_verify(
    compressed: &[u8],
    expected_sha256: &str,
    expected_decompressed_size: u64,
    asset_file: &str,
) -> Result<Vec<u8>> {
    // Bound the reader one byte past the expected size: reading that extra
    // byte means the asset is oversized (or the manifest is wrong), so the
    // length check rejects it before we ever hash gigabytes.
    let mut decoded = Vec::new();
    zstd::Decoder::new(io::Cursor::new(compressed))
        .with_context(|| format!("decompress {asset_file}"))?
        .take(expected_decompressed_size.saturating_add(1))
        .read_to_end(&mut decoded)
        .with_context(|| format!("decompress {asset_file}"))?;
    if decoded.len() as u64 != expected_decompressed_size {
        return Err(anyhow!(
            "{asset_file}: decompressed to {} bytes but the manifest declares {} \
             (corrupt download or zip bomb)",
            decoded.len(),
            expected_decompressed_size
        ));
    }

    let actual = hex_sha256(&decoded);
    if !ct_eq(actual.as_bytes(), expected_sha256.as_bytes()) {
        return Err(anyhow!(
            "sha256 mismatch for {asset_file}: expected {expected_sha256}, got {actual}"
        ));
    }
    Ok(decoded)
}

fn run_curl(url: &str, dest: &Path, max_filesize: u64) -> Result<()> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            // HTTPS only — on the initial request *and* on any redirect curl
            // follows (--location). Without --proto-redir a redirect could
            // downgrade to http://; the sha256 gate still guarantees integrity,
            // but this closes the downgrade surface up front.
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--retry",
            "3",
            "--max-filesize",
        ])
        .arg(max_filesize.to_string())
        .arg("--output")
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

    #[test]
    fn decode_and_verify_accepts_matching_asset() {
        let raw = b"the quick brown fox jumps over the lazy dog";
        let compressed = zstd::encode_all(io::Cursor::new(&raw[..]), 0).expect("compress");
        let sha = hex_sha256(raw);
        let out = decode_and_verify(&compressed, &sha, raw.len() as u64, "t.db.zst")
            .expect("matching size + sha");
        assert_eq!(out, raw);
    }

    #[test]
    fn decode_and_verify_rejects_oversize_decode() {
        // Asset expands to 4096 bytes; the manifest claims 16. The bounded
        // reader must trip the size check rather than decompressing unbounded.
        let raw = vec![0u8; 4096];
        let compressed = zstd::encode_all(io::Cursor::new(&raw[..]), 0).expect("compress");
        let err = decode_and_verify(&compressed, &hex_sha256(&raw), 16, "bomb.db.zst")
            .expect_err("oversize must be rejected");
        assert!(
            format!("{err}").contains("zip bomb") || format!("{err}").contains("decompressed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decode_and_verify_rejects_sha_mismatch() {
        let raw = b"genuine bytes";
        let compressed = zstd::encode_all(io::Cursor::new(&raw[..]), 0).expect("compress");
        let err = decode_and_verify(&compressed, &"0".repeat(64), raw.len() as u64, "x.db.zst")
            .expect_err("sha mismatch must be rejected");
        assert!(format!("{err}").contains("sha256 mismatch"), "got: {err}");
    }
}
