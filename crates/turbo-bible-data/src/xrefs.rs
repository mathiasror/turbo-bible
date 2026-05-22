//! Build `xrefs.db` from scrollmapper's openbible.info shards.
//!
//! Cross-references are translation-agnostic — they're pure OSIS
//! coordinates — so a single `xrefs.db` ships alongside every
//! translation file.

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, params};

use crate::osis::{BOOKS, lookup_osis};

const XREF_SHARDS: usize = 7;

/// Scrollmapper's xref dataset spells numbered book names with Arabic
/// numerals (`1 John`, `2 Corinthians`) and the Apocalypse as plain
/// `Revelation`; the per-translation JSON uses Roman numerals
/// (`I John`) and `Revelation of John`. This variant table covers the
/// deltas so the xref importer can reach OSIS codes without
/// allocating per row. Looked up *before* falling back to the main
/// name map.
#[rustfmt::skip]
const SCROLLMAPPER_XREF_NAME_VARIANTS: &[(&str, &str)] = &[
    ("1 Chronicles", "1CH"), ("2 Chronicles", "2CH"),
    ("1 Corinthians", "1CO"), ("2 Corinthians", "2CO"),
    ("1 John", "1JN"), ("2 John", "2JN"), ("3 John", "3JN"),
    ("1 Kings", "1KI"), ("2 Kings", "2KI"),
    ("1 Peter", "1PE"), ("2 Peter", "2PE"),
    ("1 Samuel", "1SA"), ("2 Samuel", "2SA"),
    ("1 Thessalonians", "1TH"), ("2 Thessalonians", "2TH"),
    ("1 Timothy", "1TI"), ("2 Timothy", "2TI"),
    ("Revelation", "REV"),
];

fn lookup_osis_xref(name: &str) -> Option<&'static str> {
    SCROLLMAPPER_XREF_NAME_VARIANTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, o)| *o)
        .or_else(|| lookup_osis(name))
}

/// Walk `<scrollmapper>/formats/sqlite/extras/cross_references_{0..6}.db`
/// and populate `out_db` (which already has the `xref` + `book` schema
/// applied). Returns the number of rows inserted (after dedup).
pub fn build(scrollmapper: &Path, out_db: &mut Connection) -> Result<u64> {
    let extras = scrollmapper.join("formats").join("sqlite").join("extras");
    if !extras.is_dir() {
        bail!("missing scrollmapper extras at {}", extras.display());
    }

    let tx = out_db.transaction()?;
    populate_book(&tx)?;
    let mut total: u64 = 0;
    {
        let mut insert = tx.prepare_cached(
            "INSERT OR IGNORE INTO xref
               (from_book, from_chapter, from_verse,
                to_book, to_chapter, to_verse_start, to_verse_end, votes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for n in 0..XREF_SHARDS {
            let shard = extras.join(format!("cross_references_{n}.db"));
            total += import_shard(&shard, &mut insert)?;
        }
    }
    tx.commit()?;
    Ok(total)
}

fn populate_book(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let mut stmt =
        tx.prepare_cached("INSERT INTO book(code, testament, ord) VALUES (?1, ?2, ?3)")?;
    for (code, testament, ord) in BOOKS {
        stmt.execute(params![code, testament, ord])?;
    }
    Ok(())
}

fn import_shard(shard: &Path, insert: &mut rusqlite::CachedStatement<'_>) -> Result<u64> {
    let src = Connection::open_with_flags(shard, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", shard.display()))?;
    let mut stmt = src.prepare(
        "SELECT from_book, from_chapter, from_verse,
                to_book, to_chapter, to_verse_start, to_verse_end, votes
         FROM cross_references",
    )?;
    let mut rows = stmt.query([])?;
    let mut count: u64 = 0;
    while let Some(row) = rows.next()? {
        let from_name: String = row.get(0)?;
        let from_chapter: i64 = row.get(1)?;
        let from_verse: i64 = row.get(2)?;
        let to_name: String = row.get(3)?;
        let to_chapter: i64 = row.get(4)?;
        let to_verse_start: i64 = row.get(5)?;
        let to_verse_end: i64 = row.get(6)?;
        let votes: i64 = row.get(7)?;
        // Skip rows whose book names we don't recognize. Future
        // scrollmapper bumps that introduce deuterocanonical entries
        // downgrade silently here instead of corrupting the FK.
        let (Some(from), Some(to)) = (lookup_osis_xref(&from_name), lookup_osis_xref(&to_name))
        else {
            continue;
        };
        let n = insert.execute(params![
            from,
            from_chapter,
            from_verse,
            to,
            to_chapter,
            to_verse_start,
            to_verse_end,
            votes,
        ])?;
        count += n as u64;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xref_variants_resolve() {
        assert_eq!(lookup_osis_xref("1 Chronicles"), Some("1CH"));
        assert_eq!(lookup_osis_xref("Revelation"), Some("REV"));
        // Falls through to the main map.
        assert_eq!(lookup_osis_xref("Genesis"), Some("GEN"));
        // Both variants are mappable so we never lose data.
        assert_eq!(lookup_osis_xref("I Chronicles"), Some("1CH"));
        assert_eq!(lookup_osis_xref("Revelation of John"), Some("REV"));
        assert_eq!(lookup_osis_xref("Tobit"), None);
    }
}
