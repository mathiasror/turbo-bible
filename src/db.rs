//! SQLite access layer. Types live next to the queries that return them.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

/// Bump this when we change tokenizer settings or want to force a rebuild.
const FTS_TARGET_VERSION: &str = "2";

/// Open the DB writable and, if `verse_fts` hasn't been rebuilt with our
/// preferred tokenizer settings (`remove_diacritics 1` + prefix index),
/// rebuild it. Idempotent: prints `false` and returns quickly when already
/// up-to-date.
///
/// Returns `Ok(true)` if a rebuild happened (so the caller can surface a
/// "first launch is slow" message if desired).
pub fn ensure_fts_optimized(path: &Path) -> Result<bool> {
    let conn = Connection::open(path).with_context(|| format!("open RW {}", path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )?;
    let current: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key='fts_version'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    if current.as_deref() == Some(FTS_TARGET_VERSION) {
        return Ok(false);
    }

    // Drop the old triggers and FTS table, then recreate with our preferred
    // tokenizer. Diacritic level 1 is safe for Norwegian (folds combining
    // accents but preserves æ/ø/å).
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS verse_ai;
         DROP TRIGGER IF EXISTS verse_ad;
         DROP TRIGGER IF EXISTS verse_au;
         DROP TABLE IF EXISTS verse_fts;
         CREATE VIRTUAL TABLE verse_fts USING fts5(
             text, content='verse', content_rowid='rowid',
             tokenize='unicode61 remove_diacritics 1',
             prefix='2 3'
         );
         INSERT INTO verse_fts(rowid, text) SELECT rowid, text FROM verse;
         CREATE TRIGGER verse_ai AFTER INSERT ON verse BEGIN
             INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
         END;
         CREATE TRIGGER verse_ad AFTER DELETE ON verse BEGIN
             INSERT INTO verse_fts(verse_fts, rowid, text)
                 VALUES ('delete', old.rowid, old.text);
         END;
         CREATE TRIGGER verse_au AFTER UPDATE ON verse BEGIN
             INSERT INTO verse_fts(verse_fts, rowid, text)
                 VALUES ('delete', old.rowid, old.text);
             INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
         END;",
    )?;
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('fts_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![FTS_TARGET_VERSION],
    )?;
    Ok(true)
}

#[derive(Debug, Clone)]
pub struct TranslationInfo {
    pub code: String,
    pub name: String,
    pub language: String,
    #[expect(dead_code, reason = "roadmap: shown by the Translations picker's details panel")]
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub code: String,
    pub name: String,
    pub abbreviation: String,
    pub testament: String,
    #[expect(
        dead_code,
        reason = "roadmap: canonical ordinal exposed to consumers; iteration order is already enforced by the SELECT's `ORDER BY b.ord` server-side"
    )]
    pub ord: i64,
    /// Full title from the source page (e.g. "Evangeliet etter Matteus").
    /// Falls back to `name` when not populated.
    pub full_name: Option<String>,
}

impl Book {
    pub fn display_name(&self) -> &str {
        self.full_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Verse {
    pub number: i64,
    pub text: String,
    pub footnote_count: i64,
    pub xref_note_count: i64,
}

#[derive(Debug, Clone)]
pub struct Heading {
    pub before_verse: i64,
    pub style: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Footnote {
    pub id: String,
    pub verse_osis: String,
    pub kind: String, // 'f' or 'x'
    pub body: String,
    pub refs: Vec<Xref>,
}

#[derive(Debug, Clone)]
pub struct Xref {
    pub target_osis: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct Passage {
    pub translation: String,
    pub book_code: String,
    pub book_name: String,
    pub book_abbrev: String,
    pub chapter: i64,
    pub verses: Vec<Verse>,
    pub headings: Vec<Heading>,
    pub footnotes: Vec<Footnote>,
}

pub struct Db {
    conn: Connection,
    pub translation: String,
}

impl Db {
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn open_ro(path: &Path, translation: &str) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "query_only", "ON")?;
        Ok(Self {
            conn,
            translation: translation.to_string(),
        })
    }

    pub fn list_translations(&self) -> Result<Vec<TranslationInfo>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT code, name, language, license FROM translation ORDER BY code",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TranslationInfo {
                    code: r.get(0)?,
                    name: r.get(1)?,
                    language: r.get(2)?,
                    license: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// "King James Version (1769)  ·  en-kjv" — the subtitle shown on splash.
    pub fn translation_label(&self) -> Result<String> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT name FROM translation WHERE code=?1")?;
        let name: String = stmt.query_row(params![self.translation], |r| r.get(0))?;
        Ok(format!("{}  ·  {}", name, self.translation))
    }

    pub fn list_books(&self) -> Result<Vec<Book>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT b.code, bl.name, bl.abbreviation, b.testament, b.ord, bl.full_name
             FROM book b
             JOIN book_label bl ON bl.book = b.code AND bl.translation = ?1
             ORDER BY b.ord",
        )?;
        let rows = stmt
            .query_map(params![self.translation], |r| {
                Ok(Book {
                    code: r.get(0)?,
                    name: r.get(1)?,
                    abbreviation: r.get(2)?,
                    testament: r.get(3)?,
                    ord: r.get(4)?,
                    full_name: r.get::<_, Option<String>>(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn chapter_count(&self, book: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT COALESCE(MAX(chapter), 0) FROM verse WHERE translation=?1 AND book=?2",
        )?;
        let n: i64 = stmt.query_row(params![self.translation, book], |r| r.get(0))?;
        Ok(n)
    }

    pub fn load_passage(&self, book: &str, chapter: i64) -> Result<Passage> {
        let (book_name, book_abbrev) = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT COALESCE(full_name, name), abbreviation FROM book_label
                 WHERE translation=?1 AND book=?2",
            )?;
            stmt.query_row(params![self.translation, book], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
        };

        let verses = {
            // v.osis_id is used by the correlated subqueries server-side but
            // not projected into the Rust struct.
            let mut stmt = self.conn.prepare_cached(
                "SELECT v.verse, v.text,
                        COALESCE((SELECT COUNT(*) FROM footnote f
                                   WHERE f.translation=v.translation
                                     AND f.verse_osis=v.osis_id
                                     AND f.kind='f'), 0) AS fn,
                        COALESCE((SELECT COUNT(*) FROM footnote f
                                   WHERE f.translation=v.translation
                                     AND f.verse_osis=v.osis_id
                                     AND f.kind='x'), 0) AS xn
                 FROM verse v
                 WHERE v.translation=?1 AND v.book=?2 AND v.chapter=?3
                 ORDER BY v.verse",
            )?;
            stmt.query_map(params![self.translation, book, chapter], |r| {
                Ok(Verse {
                    number: r.get(0)?,
                    text: r.get(1)?,
                    footnote_count: r.get(2)?,
                    xref_note_count: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        let headings = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT before_verse, style, text FROM heading
                 WHERE translation=?1 AND book=?2 AND chapter=?3
                 ORDER BY before_verse, rowid",
            )?;
            stmt.query_map(params![self.translation, book, chapter], |r| {
                Ok(Heading {
                    before_verse: r.get(0)?,
                    style: r.get(1)?,
                    text: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        let footnotes = self.load_footnotes(book, chapter)?;

        Ok(Passage {
            translation: self.translation.clone(),
            book_code: book.to_string(),
            book_name,
            book_abbrev,
            chapter,
            verses,
            headings,
            footnotes,
        })
    }

    fn load_footnotes(&self, book: &str, chapter: i64) -> Result<Vec<Footnote>> {
        let prefix = format!("{book}.{chapter}.");
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, verse_osis, kind, body FROM footnote
             WHERE translation=?1 AND verse_osis LIKE ?2 || '%'
             ORDER BY id",
        )?;
        let mut footnotes: Vec<Footnote> = stmt
            .query_map(params![self.translation, prefix], |r| {
                Ok(Footnote {
                    id: r.get(0)?,
                    verse_osis: r.get(1)?,
                    kind: r.get(2)?,
                    body: r.get(3)?,
                    refs: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        if footnotes.is_empty() {
            return Ok(footnotes);
        }

        // `position` is used only for the server-side ORDER BY; the Rust
        // struct doesn't need it.
        let mut xref_stmt = self.conn.prepare_cached(
            "SELECT footnote_id, target_osis, label FROM xref
             WHERE translation=?1 AND footnote_id IN (
                 SELECT id FROM footnote
                 WHERE translation=?1 AND verse_osis LIKE ?2 || '%'
             )
             ORDER BY footnote_id, position",
        )?;
        let xref_rows: Vec<(String, Xref)> = xref_stmt
            .query_map(params![self.translation, prefix], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Xref {
                        target_osis: r.get(1)?,
                        label: r.get(2)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        for (fn_id, xref) in xref_rows {
            if let Some(fn_) = footnotes.iter_mut().find(|f| f.id == fn_id) {
                fn_.refs.push(xref);
            }
        }
        Ok(footnotes)
    }
}
