//! Compile-time embedded translation. Only `en-kjv` ships in the
//! binary (~4 MB zstd); the other ten translations and the
//! cross-references DB are downloaded from GitHub Releases on demand
//! by [`crate::fetch`], driven off the static catalogue in
//! [`crate::manifest`].
//!
//! Decompression and on-disk extraction live in [`crate::install`].

/// One embedded asset: a translation code and its zstd-compressed
/// bytes.
pub struct BundledAsset {
    pub code: &'static str,
    pub bytes: &'static [u8],
}

/// The translation code that's always available offline.
pub const DEFAULT_TRANSLATION: &str = "en-kjv";

/// Translations embedded in the binary. The single entry is the
/// English default; everything else is fetched at runtime.
pub const BUNDLED: &[BundledAsset] = &[BundledAsset {
    code: DEFAULT_TRANSLATION,
    bytes: include_bytes!("../assets/en-kjv.db.zst"),
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bundled_and_nonempty() {
        let kjv = BUNDLED
            .iter()
            .find(|a| a.code == DEFAULT_TRANSLATION)
            .expect("default translation must be in BUNDLED");
        assert!(!kjv.bytes.is_empty(), "embedded KJV is empty");
    }

    /// Sanity: bundled .db.zst starts with zstd's magic bytes
    /// (`28 B5 2F FD`). Catches a truncated or swapped file.
    #[test]
    fn bundled_bytes_are_zstd_frames() {
        const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
        for asset in BUNDLED {
            assert!(
                asset.bytes.starts_with(&ZSTD_MAGIC),
                "{} doesn't start with zstd magic",
                asset.code
            );
        }
    }
}
