//! Compile-time embedded translations. The `.db.zst` bytes live in
//! `crates/turbo-bible-tui/assets/`, populated by `just bundle-translations`
//! from the data pipeline. Embedding fails the build if any expected
//! file is missing — by design, a binary without translations is
//! useless.
//!
//! Decompression and on-disk extraction live in
//! [`crate::install`].

/// One embedded asset: a translation code (or the literal `"xrefs"`
/// for the shared cross-references DB) and its zstd-compressed bytes.
pub struct BundledAsset {
    pub code: &'static str,
    pub bytes: &'static [u8],
}

/// All translations the binary ships with, plus the shared xrefs DB.
///
/// Order matches `data/manifest_source.toml`. New entries must be
/// added here *and* in `just bundle-translations`'s output.
pub const BUNDLED: &[BundledAsset] = &[
    BundledAsset {
        code: "en-kjv",
        bytes: include_bytes!("../assets/en-kjv.db.zst"),
    },
    BundledAsset {
        code: "en-asv",
        bytes: include_bytes!("../assets/en-asv.db.zst"),
    },
    BundledAsset {
        code: "en-ylt",
        bytes: include_bytes!("../assets/en-ylt.db.zst"),
    },
    BundledAsset {
        code: "en-drc",
        bytes: include_bytes!("../assets/en-drc.db.zst"),
    },
    BundledAsset {
        code: "en-bsb",
        bytes: include_bytes!("../assets/en-bsb.db.zst"),
    },
    BundledAsset {
        code: "nb-1930",
        bytes: include_bytes!("../assets/nb-1930.db.zst"),
    },
    BundledAsset {
        code: "es-rv1909",
        bytes: include_bytes!("../assets/es-rv1909.db.zst"),
    },
    BundledAsset {
        code: "de-menge",
        bytes: include_bytes!("../assets/de-menge.db.zst"),
    },
    BundledAsset {
        code: "fr-crampon",
        bytes: include_bytes!("../assets/fr-crampon.db.zst"),
    },
    BundledAsset {
        code: "pt-blivre",
        bytes: include_bytes!("../assets/pt-blivre.db.zst"),
    },
    BundledAsset {
        code: "la-clementine",
        bytes: include_bytes!("../assets/la-clementine.db.zst"),
    },
];

/// The shared cross-references DB. Sits next to translations on disk
/// (`xrefs.db`), ATTACHed under alias `xrefs` at runtime.
pub const BUNDLED_XREFS: BundledAsset = BundledAsset {
    code: "xrefs",
    bytes: include_bytes!("../assets/xrefs.db.zst"),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_codes_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for asset in BUNDLED {
            assert!(!asset.code.is_empty(), "empty code");
            assert!(!asset.bytes.is_empty(), "empty bytes for {}", asset.code);
            assert!(
                seen.insert(asset.code),
                "duplicate bundled code: {}",
                asset.code
            );
        }
    }

    #[test]
    fn xrefs_is_nonempty() {
        assert!(!BUNDLED_XREFS.bytes.is_empty());
        assert_eq!(BUNDLED_XREFS.code, "xrefs");
    }

    /// Sanity: every bundled .db.zst starts with zstd's magic bytes
    /// (`28 B5 2F FD`). Catches truncated files or accidental swaps.
    #[test]
    fn bundled_bytes_are_zstd_frames() {
        const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
        for asset in BUNDLED.iter().chain(std::iter::once(&BUNDLED_XREFS)) {
            assert!(
                asset.bytes.starts_with(&ZSTD_MAGIC),
                "{} doesn't start with zstd magic",
                asset.code
            );
        }
    }
}
