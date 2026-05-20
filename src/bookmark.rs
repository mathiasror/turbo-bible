//! Bookmark persistence. Stored as TOML at `~/.config/turbo-bible/bookmarks.toml`.
//!
//! v1 used JSON at `bookmarks.json`; this loader reads either and rewrites
//! to TOML on the next save. Old bookmarks tagged `translation = "nb-2024"`
//! are rewritten to `"nb-1930"` (same Protestant versification, safe rename).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use etcetera::{BaseStrategy, choose_base_strategy};
use serde::{Deserialize, Serialize};

use crate::state::{LEGACY_TRANSLATION, REPLACEMENT_TRANSLATION};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub fn matches_chapter(&self, translation: &str, book: &str, chapter: i64) -> bool {
        self.translation == translation && self.book == book && self.chapter == chapter
    }
    pub fn same_range(&self, other: &Bookmark) -> bool {
        self.translation == other.translation
            && self.book == other.book
            && self.chapter == other.chapter
            && self.start_verse == other.start_verse
            && self.end_verse == other.end_verse
    }
    pub fn reference_label(&self, book_name: &str) -> String {
        if self.start_verse == self.end_verse {
            format!("{} {}:{}", book_name, self.chapter, self.start_verse)
        } else {
            format!(
                "{} {}:{}-{}",
                book_name, self.chapter, self.start_verse, self.end_verse
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
    pub fn load() -> Self {
        // Preferred: TOML.
        if let Ok(path) = bookmarks_path()
            && let Ok(txt) = fs::read_to_string(&path)
            && let Ok(mut s) = toml::from_str::<BookmarkStore>(&txt)
        {
            s.rewrite_legacy_translation();
            return s;
        }
        // Fallback: legacy JSON file from v1.
        if let Ok(legacy) = legacy_bookmarks_path()
            && let Ok(txt) = fs::read_to_string(&legacy)
            && let Ok(mut s) = serde_json::from_str::<BookmarkStore>(&txt)
        {
            s.rewrite_legacy_translation();
            return s;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let dir = config_dir()?;
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
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn config_dir() -> Result<PathBuf> {
    let strategy = choose_base_strategy()?;
    let mut p = strategy.config_dir();
    p.push("turbo-bible");
    Ok(p)
}

fn bookmarks_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("bookmarks.toml");
    Ok(p)
}

fn legacy_bookmarks_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    p.push("bookmarks.json");
    Ok(p)
}
