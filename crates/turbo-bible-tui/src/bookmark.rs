//! Bookmark persistence. Stored as TOML at `~/.config/turbo-bible/bookmarks.toml`.
//!
//! v1 used JSON at `bookmarks.json`; this loader reads either and rewrites
//! to TOML on the next save. Old bookmarks tagged `translation = "nb-2024"`
//! are rewritten to `"nb-1930"` (same Protestant versification, safe rename).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::state::{LEGACY_TRANSLATION, REPLACEMENT_TRANSLATION};

// PartialEq/Eq/Hash are deliberately NOT derived: production code uses
// `same_range` (position-only equality) and a derived `==` would mean
// label/created_at participated too. Two equalities on one type is a
// foot-gun, so the type only has one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub translation: String,
    pub book: String,
    pub chapter: i64,
    pub start_verse: i64,
    /// Inclusive. Equal to `start_verse` for a single-verse bookmark.
    pub end_verse: i64,
    #[serde(default)]
    pub label: Option<String>,
    /// Unix seconds; used to sort the bookmark list by recency.
    #[serde(default)]
    pub created_at: u64,
}

impl Bookmark {
    #[must_use]
    pub fn matches_chapter(&self, translation: &str, book: &str, chapter: i64) -> bool {
        self.translation == translation && self.book == book && self.chapter == chapter
    }
    #[must_use]
    pub fn same_range(&self, other: &Self) -> bool {
        self.translation == other.translation
            && self.book == other.book
            && self.chapter == other.chapter
            && self.start_verse == other.start_verse
            && self.end_verse == other.end_verse
    }
    #[must_use]
    pub fn reference_label(&self, book_name: &str) -> String {
        if self.start_verse == self.end_verse {
            crate::reference::format(book_name, self.chapter, self.start_verse, &self.translation)
        } else {
            crate::reference::format_range(
                book_name,
                self.chapter,
                self.start_verse,
                self.end_verse,
                &self.translation,
            )
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BookmarkStore {
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl BookmarkStore {
    /// Load the bookmark store. A present-but-unparsable `bookmarks.toml` is
    /// surfaced via `warnings` (replayed to stderr after the TUI exits) rather
    /// than silently discarded — otherwise the user loses their bookmarks from
    /// view and the next save overwrites the file. Warnings are collected, not
    /// printed, because this runs while the alternate screen is active.
    pub fn load(warnings: &mut Vec<String>) -> Self {
        // Preferred: TOML.
        if let Ok(path) = bookmarks_path() {
            match fs::read_to_string(&path) {
                Ok(txt) => match toml::from_str::<Self>(&txt) {
                    Ok(mut s) => {
                        s.rewrite_legacy_translation();
                        return s;
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "bookmarks.toml is unparsable ({e}); starting with no \
                             bookmarks — it will be overwritten on the next change"
                        ));
                        return Self::default();
                    }
                },
                // No TOML file yet — fall through to the legacy JSON migration.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    warnings.push(format!(
                        "could not read bookmarks.toml ({e}); starting with no bookmarks"
                    ));
                    return Self::default();
                }
            }
        }
        // Fallback: legacy JSON file from v1.
        if let Ok(legacy) = legacy_bookmarks_path()
            && let Ok(txt) = fs::read_to_string(&legacy)
            && let Ok(mut s) = serde_json::from_str::<Self>(&txt)
        {
            s.rewrite_legacy_translation();
            return s;
        }
        Self::default()
    }

    /// # Errors
    /// Fails when the config dir can't be created, the TOML serialization
    /// errors, or the write to `bookmarks.toml` fails (typically:
    /// permission denied, disk full).
    pub fn save(&self) -> Result<()> {
        let dir = paths::config_dir()?;
        fs::create_dir_all(&dir)?;
        let path = bookmarks_path()?;
        let txt = toml::to_string_pretty(self)?;
        fs::write(path, txt)?;
        if let Ok(legacy) = legacy_bookmarks_path() {
            let _ = fs::remove_file(legacy);
        }
        Ok(())
    }

    pub fn add(&mut self, bm: Bookmark) {
        // De-dupe: if an identical range already exists, leave it alone.
        if self.bookmarks.iter().any(|b| b.same_range(&bm)) {
            return;
        }
        self.bookmarks.push(bm);
    }

    fn rewrite_legacy_translation(&mut self) {
        for bm in &mut self.bookmarks {
            if bm.translation == LEGACY_TRANSLATION {
                bm.translation = REPLACEMENT_TRANSLATION.into();
            }
        }
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn bookmarks_path() -> Result<PathBuf> {
    let mut p = paths::config_dir()?;
    p.push("bookmarks.toml");
    Ok(p)
}

fn legacy_bookmarks_path() -> Result<PathBuf> {
    let mut p = paths::config_dir()?;
    p.push("bookmarks.json");
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(start: i64, end: i64) -> Bookmark {
        Bookmark {
            translation: "en-kjv".into(),
            book: "JHN".into(),
            chapter: 3,
            start_verse: start,
            end_verse: end,
            label: None,
            created_at: 0,
        }
    }

    #[test]
    fn same_range_compares_position_only() {
        let mut a = bm(1, 5);
        let mut b = bm(1, 5);
        assert!(a.same_range(&b));
        // label and created_at don't affect same_range...
        a.label = Some("foo".into());
        b.created_at = 42;
        assert!(a.same_range(&b));
        // ...but verse range does.
        b.end_verse = 6;
        assert!(!a.same_range(&b));
    }

    #[test]
    fn add_dedupes_via_same_range() {
        let mut store = BookmarkStore::default();
        store.add(bm(1, 1));
        store.add(bm(1, 1)); // identical
        store.add(bm(2, 2)); // different
        assert_eq!(store.bookmarks.len(), 2);
    }

    #[test]
    fn rewrite_legacy_translation_migrates_only_nb2024() {
        let mut store = BookmarkStore::default();
        let mut legacy = bm(1, 1);
        legacy.translation = "nb-2024".into();
        let kept = bm(2, 2); // en-kjv, should stay
        store.bookmarks = vec![legacy, kept];
        store.rewrite_legacy_translation();
        assert_eq!(store.bookmarks[0].translation, "nb-1930");
        assert_eq!(store.bookmarks[1].translation, "en-kjv");
    }

    #[test]
    fn loads_v1_json_shape() {
        // The v1 JSON file omits `label` and `created_at`; serde defaults
        // must fill them in.
        let txt = r#"{
            "bookmarks": [
                {
                    "translation": "en-kjv",
                    "book": "JHN",
                    "chapter": 3,
                    "start_verse": 16,
                    "end_verse": 16
                }
            ]
        }"#;
        let s: BookmarkStore = serde_json::from_str(txt).expect("legacy json should parse");
        assert_eq!(s.bookmarks.len(), 1);
        assert_eq!(s.bookmarks[0].label, None);
        assert_eq!(s.bookmarks[0].created_at, 0);
    }

    #[test]
    fn round_trips_through_toml() {
        let store = BookmarkStore {
            bookmarks: vec![bm(1, 3), {
                let mut b = bm(7, 7);
                b.label = Some("favourite".into());
                b.created_at = 1_700_000_000;
                b
            }],
        };
        let txt = toml::to_string_pretty(&store).unwrap();
        let back: BookmarkStore = toml::from_str(&txt).unwrap();
        // Compare field-by-field rather than via derived PartialEq — the
        // type intentionally has only `same_range`, and the round-trip needs
        // to assert ALL fields survive serialization (including the ones
        // same_range omits: label, created_at).
        assert_eq!(store.bookmarks.len(), back.bookmarks.len());
        for (a, b) in store.bookmarks.iter().zip(back.bookmarks.iter()) {
            assert!(a.same_range(b), "range mismatch: {a:?} vs {b:?}");
            assert_eq!(a.translation, b.translation);
            assert_eq!(a.label, b.label);
            assert_eq!(a.created_at, b.created_at);
        }
    }

    #[test]
    fn reference_label_formats_single_and_range() {
        let single = bm(7, 7);
        assert_eq!(single.reference_label("John"), "John 3:7");
        let range = bm(1, 3);
        assert_eq!(range.reference_label("John"), "John 3:1-3");
    }

    #[test]
    fn malformed_json_returns_none_via_serde() {
        // BookmarkStore::load's JSON branch uses serde_json::from_str; verify
        // it fails (returning None) on malformed input rather than panicking.
        let bad = r#"{ "bookmarks": "not-an-array" }"#;
        assert!(serde_json::from_str::<BookmarkStore>(bad).is_err());
    }
}
