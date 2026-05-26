//! The static catalogue of every translation the binary knows about.
//!
//! `assets/manifest.json` (emitted by the data pipeline) is the source
//! of truth; `build.rs` reads it at compile time and writes the
//! const tables below into `$OUT_DIR/translations_manifest.rs`.
//!
//! Translations whose `.db.zst` isn't bundled in the binary are
//! resolved at runtime by [`crate::fetch`].

/// One translation as described by the data-pipeline manifest.
///
/// Mirrors the JSON entry — every byte that crosses into the binary
/// is verified against [`Self::sha256`] before being decompressed.
#[allow(
    dead_code,
    reason = "license / attribution / decompressed_size are read by future affordances \
              (About dialog, disk-space warnings). The struct mirrors the JSON 1:1 so the \
              build.rs codegen stays uniform."
)]
pub struct TranslationManifestEntry {
    pub code: &'static str,
    pub name: &'static str,
    pub language: &'static str,
    pub license: &'static str,
    pub attribution: &'static str,
    /// Relative filename inside the release asset set (e.g. `en-kjv.db.zst`).
    pub file: &'static str,
    /// Hex-encoded SHA-256 of the *decompressed* `.db` bytes.
    pub sha256: &'static str,
    pub compressed_size: u64,
    pub decompressed_size: u64,
}

/// Manifest entry for the shared cross-references DB.
#[allow(
    dead_code,
    reason = "fields are consumed by fetch::xrefs once the K-popup wires download-on-demand"
)]
pub struct XrefsManifestEntry {
    pub file: &'static str,
    pub sha256: &'static str,
    pub compressed_size: u64,
    pub decompressed_size: u64,
}

include!(concat!(env!("OUT_DIR"), "/translations_manifest.rs"));

impl TranslationManifestEntry {
    /// Look up an entry by translation code, e.g. `"en-kjv"`.
    pub fn by_code(code: &str) -> Option<&'static TranslationManifestEntry> {
        TRANSLATIONS.iter().find(|t| t.code == code)
    }
}
