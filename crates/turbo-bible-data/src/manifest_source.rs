//! Reader for `data/manifest_source.toml` — the curated list of
//! translations the pipeline is allowed to build. Anything not listed
//! here never gets a `.db.zst` artifact, regardless of what's in the
//! scrollmapper checkout.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ManifestSource {
    pub schema_version: u32,
    #[serde(rename = "translation", default)]
    pub translations: Vec<TranslationEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TranslationEntry {
    pub code: String,
    pub abbr: String,
    pub language: String,
    pub name: String,
    pub source_json: String,
    pub license: String,
    #[serde(default)]
    pub attribution: String,
}

impl ManifestSource {
    pub fn load(path: &Path) -> Result<Self> {
        let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let parsed: Self =
            toml::from_str(&body).with_context(|| format!("parse {}", path.display()))?;
        if parsed.schema_version != 1 {
            bail!(
                "{}: unsupported schema_version {}",
                path.display(),
                parsed.schema_version
            );
        }
        for entry in &parsed.translations {
            if entry.code.is_empty() || entry.abbr.is_empty() {
                bail!("{}: translation with empty code/abbr", path.display());
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn loads_minimal_manifest() {
        let f = write_tmp(
            r#"
schema_version = 1

[[translation]]
code        = "en-bsb"
abbr        = "BSB"
language    = "en"
name        = "Berean Standard Bible"
source_json = "formats/json/BSB.json"
license     = "CC0-1.0"
attribution = ""
"#,
        );
        let m = ManifestSource::load(f.path()).unwrap();
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.translations.len(), 1);
        assert_eq!(m.translations[0].code, "en-bsb");
        assert_eq!(m.translations[0].license, "CC0-1.0");
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let f = write_tmp("schema_version = 99\n");
        let err = ManifestSource::load(f.path()).unwrap_err();
        assert!(format!("{err}").contains("unsupported schema_version"));
    }

    #[test]
    fn loads_real_curated_manifest() {
        // The repo's checked-in manifest must always parse cleanly.
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("data")
            .join("manifest_source.toml");
        if !p.is_file() {
            eprintln!("skipping: no manifest at {}", p.display());
            return;
        }
        let m = ManifestSource::load(&p).unwrap();
        assert!(m.translations.len() >= 8, "expected the full slate");
        // Spot-check the expected codes.
        let codes: Vec<&str> = m.translations.iter().map(|t| t.code.as_str()).collect();
        for expected in [
            "en-kjv",
            "en-asv",
            "en-ylt",
            "en-drc",
            "en-bsb",
            "nb-1930",
            "es-rv1909",
            "de-menge",
            "fr-crampon",
            "pt-blivre",
            "la-clementine",
        ] {
            assert!(
                codes.contains(&expected),
                "expected {expected} in slate, got {codes:?}"
            );
        }
    }
}
