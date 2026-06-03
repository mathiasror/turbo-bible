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
    use crate::schema::XREF_SCHEMA_SQL;

    /// The source-shard schema `build` SELECTs from, mirrored from a real
    /// scrollmapper `cross_references_*.db` (`formats/sqlite/extras/`). The
    /// `id` autoincrement column is present-but-unread, exactly as upstream.
    const SOURCE_SHARD_SCHEMA: &str = "
        CREATE TABLE cross_references (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_book TEXT,
            from_chapter INTEGER,
            from_verse INTEGER,
            to_book TEXT,
            to_chapter INTEGER,
            to_verse_start INTEGER,
            to_verse_end INTEGER,
            votes INTEGER
        );";

    /// One synthetic source row: `(from_book, from_chapter, from_verse,
    /// to_book, to_chapter, to_verse_start, to_verse_end, votes)`.
    type ShardRow<'a> = (&'a str, i64, i64, &'a str, i64, i64, i64, i64);

    /// Write one synthetic source shard at
    /// `<extras>/cross_references_<n>.db` with the upstream schema and `rows`.
    fn write_shard(extras: &Path, n: usize, rows: &[ShardRow<'_>]) {
        let path = extras.join(format!("cross_references_{n}.db"));
        let conn = Connection::open(&path).expect("create shard db");
        conn.execute_batch(SOURCE_SHARD_SCHEMA)
            .expect("apply source shard schema");
        let mut stmt = conn
            .prepare(
                "INSERT INTO cross_references
                   (from_book, from_chapter, from_verse,
                    to_book, to_chapter, to_verse_start, to_verse_end, votes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .expect("prepare insert");
        for (fb, fc, fv, tb, tc, tvs, tve, votes) in rows {
            stmt.execute(params![fb, fc, fv, tb, tc, tvs, tve, votes])
                .expect("insert shard row");
        }
    }

    /// End-to-end of `xrefs::build` against synthetic shards — no scrollmapper
    /// checkout required (the pipeline integration test is `#[ignore]`-gated on
    /// one). Proves: a normal coordinate lands with its votes, a numbered/variant
    /// book name resolves via the OSIS variant lookup, and a garbage book name is
    /// dropped rather than corrupting the FK.
    #[test]
    fn build_resolves_variants_and_drops_unknown_books() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let extras = tmp.path().join("formats").join("sqlite").join("extras");
        std::fs::create_dir_all(&extras).expect("create extras layout");

        // Shard 0 carries the real rows; shards 1..6 must exist (build opens
        // all seven), so create them empty.
        write_shard(
            &extras,
            0,
            &[
                // (a) normal coordinate: Genesis 1:1 -> John 1:1, 50 votes.
                ("Genesis", 1, 1, "John", 1, 1, 1, 50),
                // (b) variant name "1 John" must resolve to OSIS "1JN".
                ("1 John", 1, 9, "Psalms", 32, 5, 5, 7),
                // (c) garbage from_book — must be skipped entirely.
                ("Nonexistent Book", 1, 1, "John", 1, 1, 1, 99),
                // (c') garbage to_book — must also be skipped.
                ("John", 3, 16, "Bogus", 1, 1, 1, 99),
            ],
        );
        for n in 1..XREF_SHARDS {
            write_shard(&extras, n, &[]);
        }

        let mut out = Connection::open_in_memory().expect("open out db");
        out.pragma_update(None, "foreign_keys", true)
            .expect("enable foreign keys");
        out.execute_batch(XREF_SCHEMA_SQL)
            .expect("apply xref schema");

        let inserted = build(tmp.path(), &mut out).expect("build xrefs");
        // Two of the four source rows survive (the two garbage rows drop).
        assert_eq!(inserted, 2, "only resolvable rows should be inserted");

        let total: i64 = out
            .query_row("SELECT COUNT(*) FROM xref", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);

        // (a) the normal coordinate landed with its votes and OSIS codes.
        let votes: i64 = out
            .query_row(
                "SELECT votes FROM xref
                 WHERE from_book='GEN' AND from_chapter=1 AND from_verse=1
                   AND to_book='JHN'",
                [],
                |r| r.get(0),
            )
            .expect("Genesis 1:1 -> John row present");
        assert_eq!(votes, 50);

        // (b) "1 John" resolved to "1JN" via the variant lookup.
        let from_one_john: i64 = out
            .query_row("SELECT COUNT(*) FROM xref WHERE from_book='1JN'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(from_one_john, 1, "'1 John' should resolve to OSIS 1JN");

        // (c) nothing landed for the unknown book names.
        let garbage: i64 = out
            .query_row(
                "SELECT COUNT(*) FROM xref WHERE from_book IN ('Nonexistent Book','Bogus')
                    OR to_book IN ('Nonexistent Book','Bogus')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(garbage, 0, "unknown-book rows must be dropped");
    }

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
