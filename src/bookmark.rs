//! Bookmark persistence. Stored as JSON at `~/.config/turbo-bible/bookmarks.json`.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use etcetera::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};

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
        let Ok(path) = bookmarks_path() else { return Self::default() };
        let Ok(txt) = fs::read_to_string(path) else { return Self::default() };
        serde_json::from_str(&txt).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir)?;
        let path = bookmarks_path()?;
        let txt = serde_json::to_string_pretty(self)?;
        fs::write(path, txt)?;
        Ok(())
    }

    pub fn add(&mut self, bm: Bookmark) {
        // De-dupe: if an identical range already exists, leave it alone.
        if self.bookmarks.iter().any(|b| b.same_range(&bm)) {
            return;
        }
        self.bookmarks.push(bm);
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
    p.push("bookmarks.json");
    Ok(p)
}
