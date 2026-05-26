//! "Bible quote of the day" for the splash screen. Picks a deterministic
//! verse based on the current calendar day, then resolves it against the DB.
//! If the curated reference isn't in the corpus yet (e.g. crawl still in
//! progress), we step to the next one.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rusqlite::params;

use crate::db::Db;

/// Curated list of well-known references — OSIS ids.
const CURATED: &[(&str, i64, i64)] = &[
    ("GEN", 1, 1),
    ("PSA", 23, 1),
    ("PSA", 46, 10),
    ("PSA", 119, 105),
    ("PRO", 3, 5),
    ("ISA", 40, 31),
    ("ISA", 41, 10),
    ("JER", 29, 11),
    ("MAT", 5, 3),
    ("MAT", 6, 33),
    ("MAT", 11, 28),
    ("MAT", 28, 19),
    ("MRK", 12, 30),
    ("LUK", 6, 31),
    ("JHN", 1, 1),
    ("JHN", 3, 16),
    ("JHN", 14, 6),
    ("ROM", 5, 8),
    ("ROM", 8, 28),
    ("ROM", 12, 2),
    ("1CO", 13, 4),
    ("1CO", 13, 13),
    ("GAL", 5, 22),
    ("EPH", 2, 8),
    ("PHP", 4, 6),
    ("PHP", 4, 13),
    ("1JN", 4, 8),
    ("HEB", 11, 1),
    ("REV", 21, 4),
    ("DEU", 6, 5),
];

#[derive(Debug, Clone)]
pub struct DailyQuote {
    pub reference: String, // e.g. "Salme 23,1"
    pub text: String,
}

/// Pick today's quote and resolve its text against the DB. Walks forward
/// through the curated list if the chosen reference isn't loaded yet.
///
/// # Errors
/// Fails when the underlying SQL preparation errors. A row-not-found is
/// not an error — it triggers the next-candidate walk.
pub fn pick(db: &Db, translation: &str) -> Result<Option<DailyQuote>> {
    if CURATED.is_empty() {
        return Ok(None);
    }
    let start = day_index() % CURATED.len();
    for offset in 0..CURATED.len() {
        let (book, chapter, verse) = CURATED[(start + offset) % CURATED.len()];
        if let Some(q) = lookup(db, translation, book, chapter, verse)? {
            return Ok(Some(q));
        }
    }
    Ok(None)
}

fn day_index() -> usize {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs / 86_400) as usize
}

fn lookup(
    db: &Db,
    translation: &str,
    book: &str,
    chapter: i64,
    verse: i64,
) -> Result<Option<DailyQuote>> {
    let mut stmt = db.conn().prepare_cached(
        "SELECT v.text, bl.name FROM verse v
         JOIN book_label bl ON bl.book = v.book
         WHERE v.book = ?1 AND v.chapter = ?2 AND v.verse = ?3",
    )?;
    let row = stmt
        .query_row(params![book, chapter, verse], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .ok();
    Ok(row.map(|(text, name)| DailyQuote {
        reference: crate::reference::format(&name, chapter, verse, translation),
        text: text.replace('\n', " "),
    }))
}
