//! Walks a scrollmapper checkout and emits one CSV row per translation
//! it finds under `sources/<lang>/<ABBR>/`.
//!
//! The audit is the project's legal paper trail: the resulting CSV is
//! compared by hand against `data/manifest_source.toml` (the curated
//! slate of translations turbo-bible will ever build).

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;
use walkdir::WalkDir;

/// One row of the audit CSV.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AuditRow {
    pub language: String,
    pub abbr: String,
    pub license_raw: String,
    pub license_category: LicenseCategory,
    pub json_present: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseCategory {
    PublicDomain,
    Cc0,
    CcBy,
    CcBySa,
    Restricted,
    Unknown,
}

/// Run the audit against a scrollmapper checkout, writing the CSV to
/// `out` (or stdout when `None`).
pub fn run(scrollmapper: &Path, out: Option<&Path>) -> Result<()> {
    let rows = collect(scrollmapper)?;
    let mut writer: Box<dyn Write> = match out {
        Some(p) => {
            if let Some(parent) = p.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create parent of {}", p.display()))?;
            }
            Box::new(fs::File::create(p).with_context(|| format!("create {}", p.display()))?)
        }
        None => Box::new(std::io::stdout()),
    };
    let mut csv_writer = csv::Writer::from_writer(&mut writer);
    for row in &rows {
        csv_writer.serialize(row)?;
    }
    csv_writer.flush()?;
    Ok(())
}

/// Walk `<scrollmapper>/sources/<lang>/<abbr>/README.md` and produce
/// one [`AuditRow`] per match. Sorted by `(language, abbr)` for
/// deterministic output.
pub fn collect(scrollmapper: &Path) -> Result<Vec<AuditRow>> {
    let sources_dir = scrollmapper.join("sources");
    if !sources_dir.is_dir() {
        bail!(
            "expected scrollmapper checkout with a `sources/` directory at {}",
            scrollmapper.display()
        );
    }

    let json_dir = scrollmapper.join("formats").join("json");

    let mut rows = Vec::new();
    for entry in WalkDir::new(&sources_dir)
        .min_depth(3)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == "README.md")
    {
        // <sources>/<lang>/<abbr>/README.md  — depth 3
        let readme = entry.path();
        let abbr_dir = readme.parent().expect("README.md has a parent");
        let lang_dir = abbr_dir.parent().expect("<abbr> has a parent");

        let abbr = file_name(abbr_dir)?;
        let language = file_name(lang_dir)?;

        let body =
            fs::read_to_string(readme).with_context(|| format!("read {}", readme.display()))?;
        let license_raw = extract_license_line(&body).unwrap_or_default();
        let license_category = categorize(&license_raw);

        let json_present = json_dir.join(format!("{abbr}.json")).is_file();

        rows.push(AuditRow {
            language,
            abbr,
            license_raw,
            license_category,
            json_present,
        });
    }

    rows.sort_by(|a, b| (&a.language, &a.abbr).cmp(&(&b.language, &b.abbr)));
    Ok(rows)
}

fn file_name(p: &Path) -> Result<String> {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("non-utf8 path: {}", p.display()))
}

/// Pulls the first `License: ...` line out of a scrollmapper README,
/// tolerating both `**License:**` and `**License**:` styles plus
/// occasional missing markdown bold.
pub fn extract_license_line(body: &str) -> Option<String> {
    static_re().captures(body).map(|c| c[1].trim().to_string())
}

fn static_re() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // (?im) — case-insensitive, multi-line.
        // Tolerates `**License:** value`, `**License**: value`, and plain
        // `License: value`. The `\*{0,2}` after the colon strips the
        // trailing bold close in the first form.
        Regex::new(r"(?im)^\s*\*{0,2}\s*License\s*\*{0,2}\s*:\s*\*{0,2}\s*(.+?)\s*$")
            .expect("static license regex compiles")
    })
}

/// Bucket a raw license string into one of the project's canonical
/// categories. Order matters: more specific patterns first.
pub fn categorize(license_raw: &str) -> LicenseCategory {
    let s = license_raw.to_ascii_lowercase();

    // CC0 first, before generic "Creative Commons" catches it.
    if s.contains("cc0") || s.contains("public domain dedication") {
        return LicenseCategory::Cc0;
    }
    if s.contains("public domain") {
        return LicenseCategory::PublicDomain;
    }
    // SA before BY (every -SA is also a -BY).
    if s.contains("by-sa") || s.contains("by sa") || s.contains("attribution-sharealike") {
        return LicenseCategory::CcBySa;
    }
    // NC / ND variants are not redistributable for our purposes.
    if s.contains("by-nc") || s.contains("by-nd") || s.contains("nc-nd") || s.contains("nc-sa") {
        return LicenseCategory::Restricted;
    }
    if s.contains("creative commons") || s.contains("cc-by") || s.contains("cc by") {
        return LicenseCategory::CcBy;
    }
    if s.contains("gpl")
        || s.contains("copyright")
        || s.contains("non-commercial")
        || s.contains("non commercial")
    {
        return LicenseCategory::Restricted;
    }
    LicenseCategory::Unknown
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn extracts_standard_form() {
        let body = "# BSB: Berean Standard Bible\n\n**License:** Creative Commons CC0\n";
        assert_eq!(
            extract_license_line(body).as_deref(),
            Some("Creative Commons CC0")
        );
    }

    #[test]
    fn extracts_swapped_colon_form() {
        let body = "# NHEB\n\n**License**: Public Domain\n";
        assert_eq!(extract_license_line(body).as_deref(), Some("Public Domain"));
    }

    #[test]
    fn extracts_without_bold() {
        let body = "# LEB\n\nLicense: Copyrighted; Free non-commercial distribution\n";
        assert_eq!(
            extract_license_line(body).as_deref(),
            Some("Copyrighted; Free non-commercial distribution")
        );
    }

    #[test]
    fn extracts_first_when_repeated() {
        let body = "**License:** Public Domain\n\nNot the license: foo\n";
        assert_eq!(extract_license_line(body).as_deref(), Some("Public Domain"));
    }

    #[test]
    fn returns_none_when_absent() {
        assert!(extract_license_line("# Title\n\nNo such field here.\n").is_none());
    }

    #[test]
    fn categorizes_cc0_before_creative_commons() {
        assert_eq!(categorize("Creative Commons CC0"), LicenseCategory::Cc0);
        assert_eq!(categorize("CC0 1.0"), LicenseCategory::Cc0);
    }

    #[test]
    fn categorizes_public_domain() {
        assert_eq!(categorize("Public Domain"), LicenseCategory::PublicDomain);
    }

    #[test]
    fn categorizes_cc_by() {
        assert_eq!(
            categorize("Creative Commons Attribution 3.0 Brazil"),
            LicenseCategory::CcBy
        );
    }

    #[test]
    fn categorizes_cc_by_sa() {
        assert_eq!(
            categorize("Creative Commons: BY-SA 4.0"),
            LicenseCategory::CcBySa
        );
    }

    #[test]
    fn categorizes_nc_as_restricted() {
        assert_eq!(
            categorize("Creative Commons: BY-NC-ND 4.0"),
            LicenseCategory::Restricted
        );
    }

    #[test]
    fn categorizes_gpl_as_restricted() {
        assert_eq!(categorize("GPL"), LicenseCategory::Restricted);
    }

    #[test]
    fn categorizes_copyrighted_as_restricted() {
        assert_eq!(
            categorize("Copyrighted; Free non-commercial distribution"),
            LicenseCategory::Restricted
        );
    }

    #[test]
    fn categorizes_unknown() {
        assert_eq!(categorize("Some other license"), LicenseCategory::Unknown);
        assert_eq!(categorize(""), LicenseCategory::Unknown);
    }

    /// End-to-end against the live checkout at the user's known path,
    /// gated on its presence so CI doesn't depend on it.
    #[test]
    fn collects_against_local_checkout() {
        let path =
            PathBuf::from(std::env::var("HOME").expect("HOME")).join("git/oss/bible_databases");
        if !path.join("sources").is_dir() {
            eprintln!("skipping: no scrollmapper checkout at {}", path.display());
            return;
        }
        let rows = collect(&path).expect("collect");
        assert!(rows.len() >= 50, "expected many rows, got {}", rows.len());

        // Spot-check a known PD entry.
        let kjv = rows.iter().find(|r| r.abbr == "KJV").expect("KJV present");
        assert_eq!(kjv.language, "en");
        assert!(
            kjv.json_present,
            "expected formats/json/KJV.json to exist in the checkout"
        );

        // Spot-check a CC0 entry exists.
        let bsb = rows.iter().find(|r| r.abbr == "BSB").expect("BSB present");
        assert_eq!(bsb.license_category, LicenseCategory::Cc0);
    }
}
