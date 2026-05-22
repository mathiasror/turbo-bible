//! `SQLite` access layer. Types live next to the queries that return them.

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
///
/// # Errors
/// Propagates `rusqlite::Error` when the file can't be opened RW, when
/// the `meta` bookkeeping table can't be created, or when the FTS5
/// rebuild fails (typically: corruption, missing FTS5 in the `SQLite`
/// build).
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
    #[expect(
        dead_code,
        reason = "roadmap: shown by the Translations picker's details panel"
    )]
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Book {
    pub code: String,
    pub name: String,
    pub abbreviation: String,
    pub testament: String,
    // Canonical ordinal kept on the struct so consumers don't have to round-trip
    // through the DB. Iteration order is already enforced by the SELECT's
    // `ORDER BY b.ord` server-side; this field exists to support sort/compare
    // off the in-memory list. (Derived PartialEq now reads it too, so dead_code
    // no longer applies.)
    pub ord: i64,
    /// Full title from the source page (e.g. "Evangeliet etter Matteus").
    /// Falls back to `name` when not populated.
    pub full_name: Option<String>,
}

impl Book {
    #[must_use]
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
    #[expect(
        dead_code,
        reason = "roadmap: keyed by a future xref-joining ingest. The column \
                  stays in the schema; the Rust field stays so a future loader \
                  doesn't need to re-thread it through every caller."
    )]
    pub id: String,
    pub verse_osis: String,
    pub kind: String, // 'f' or 'x' — historical; today only 'f' is ingested
    pub body: String,
}

/// One openbible.info cross-reference: source verse (always in the
/// current passage's book/chapter) → target verse-range. The upstream
/// data is translation-independent (pure OSIS) but [`Db::load_xrefs`]
/// joins `book_label` for the active translation so `to_book_abbrev`
/// is ready for the UI without a follow-up lookup.
#[derive(Debug, Clone)]
pub struct Xref {
    pub from_verse: i64,
    pub to_book: String,
    /// Localized abbreviation for `to_book` under the active translation
    /// (e.g. `"Rom"` for `en-kjv`, `"Rom"` / `"Romerne"` for `nb-1930`).
    /// Falls back to `to_book` (the OSIS code) when no label row exists.
    pub to_book_abbrev: String,
    pub to_chapter: i64,
    pub to_verse_start: i64,
    pub to_verse_end: i64,
    #[expect(
        dead_code,
        reason = "Drives the load-order ORDER BY votes DESC so consumers \
                  see top-ranked xrefs first; the value isn't read directly \
                  today but stays on the struct so a future \"show vote score\" \
                  UI affordance doesn't need a schema change."
    )]
    pub votes: i64,
}

impl Xref {
    /// Human-readable target reference, e.g. `"Rom 8:28"` or `"1 Cor 13:1-3"`.
    #[must_use]
    pub fn target_label(&self) -> String {
        if self.to_verse_start == self.to_verse_end {
            format!(
                "{} {}:{}",
                self.to_book_abbrev, self.to_chapter, self.to_verse_start
            )
        } else {
            format!(
                "{} {}:{}-{}",
                self.to_book_abbrev, self.to_chapter, self.to_verse_start, self.to_verse_end,
            )
        }
    }
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
    /// Sorted by `from_verse` then `votes` DESC, so the UI can slice per
    /// cursor verse with `binary_search_by_key` and trust the order.
    pub xrefs: Vec<Xref>,
}

pub struct Db {
    conn: Connection,
    /// Active translation code. Use [`Db::translation`] /
    /// [`Db::set_translation_unchecked`] rather than reaching in directly
    /// so the call sites stay greppable and a future invalidation hook has
    /// somewhere to live.
    translation: String,
}

/// Open the DB read-only and list every installed translation. This is
/// the startup probe — used to populate the picker before any
/// translation has been chosen, so it deliberately does NOT construct a
/// `Db` (which would require a translation code; see [`Db::open_ro`]).
///
/// # Errors
/// Fails if the file can't be opened RO or if the `translation` table
/// query errors (typically: schema mismatch on an out-of-date DB).
pub fn list_translations(path: &Path) -> Result<Vec<TranslationInfo>> {
    let conn = open_ro_conn(path)?;
    query_translations(&conn)
}

fn open_ro_conn(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "query_only", "ON")?;
    Ok(conn)
}

fn query_translations(conn: &Connection) -> Result<Vec<TranslationInfo>> {
    let mut stmt =
        conn.prepare_cached("SELECT code, name, language, license FROM translation ORDER BY code")?;
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

impl Db {
    pub(crate) const fn conn(&self) -> &Connection {
        &self.conn
    }

    #[must_use]
    pub fn translation(&self) -> &str {
        &self.translation
    }

    /// Atomically swap the active translation to `code` and return the
    /// books / label / passage for the new translation. On any error,
    /// the previous translation is restored before returning, so a
    /// failed swap is observably a no-op from the caller's point of view.
    ///
    /// The caller still owns the in-memory copies of books/label/passage
    /// and must update them on success; this method only re-anchors
    /// `self.translation` and probes the DB. Cursor clamping is also the
    /// caller's job — verse counts can differ between translations.
    ///
    /// # Errors
    /// Fails if any of `list_books`, `translation_label`, or `load_passage`
    /// errors under the new translation.
    pub fn try_switch_translation(
        &mut self,
        code: &str,
        book: &str,
        chapter: i64,
    ) -> Result<(Vec<Book>, String, Passage)> {
        let prev = std::mem::replace(&mut self.translation, code.to_string());
        let probe = (|| -> Result<_> {
            Ok((
                self.list_books()?,
                self.translation_label()?,
                self.load_passage(book, chapter)?,
            ))
        })();
        if probe.is_err() {
            self.translation = prev;
        }
        probe
    }

    /// # Errors
    /// Fails if the `SQLite` file at `path` can't be opened read-only or
    /// if `PRAGMA query_only = ON` is rejected.
    ///
    /// # Panics
    /// In debug builds, panics if `translation` is empty — that path is
    /// reserved for the probe (`db::list_translations`).
    pub fn open_ro(path: &Path, translation: &str) -> Result<Self> {
        debug_assert!(
            !translation.is_empty(),
            "Db::open_ro requires a translation code; use db::list_translations \
             to probe the DB before any translation has been picked",
        );
        let conn = open_ro_conn(path)?;
        Ok(Self {
            conn,
            translation: translation.to_string(),
        })
    }

    /// # Errors
    /// Fails when the `translation` table query errors.
    pub fn list_translations(&self) -> Result<Vec<TranslationInfo>> {
        query_translations(&self.conn)
    }

    /// "King James Version (1769)  ·  en-kjv" — the subtitle shown on splash.
    ///
    /// # Errors
    /// Fails when the row for `self.translation` is missing (uninstalled
    /// translation code) or the query errors.
    pub fn translation_label(&self) -> Result<String> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT name FROM translation WHERE code=?1")?;
        let name: String = stmt.query_row(params![self.translation], |r| r.get(0))?;
        Ok(format!("{}  ·  {}", name, self.translation))
    }

    /// # Errors
    /// Fails when the join on `book_label` errors (typically: schema
    /// mismatch, or a translation row missing all its book labels).
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

    /// # Errors
    /// Fails when the `verse` table query errors. Returns `Ok(0)` for an
    /// unknown book code rather than an error.
    pub fn chapter_count(&self, book: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT COALESCE(MAX(chapter), 0) FROM verse WHERE translation=?1 AND book=?2",
        )?;
        let n: i64 = stmt.query_row(params![self.translation, book], |r| r.get(0))?;
        Ok(n)
    }

    /// # Errors
    /// Fails when the `book_label` lookup returns no row (unknown book
    /// for the active translation) or when any of the verse/heading/
    /// footnote queries error.
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
            // Footnotes are translation-scoped (today's `footnote` table is
            // unpopulated, so `fn` is always 0); xrefs are global, so the
            // xref count joins on book/chapter/verse without a translation
            // predicate.
            let mut stmt = self.conn.prepare_cached(
                "SELECT v.verse, v.text,
                        COALESCE((SELECT COUNT(*) FROM footnote f
                                   WHERE f.translation=v.translation
                                     AND f.verse_osis=v.osis_id
                                     AND f.kind='f'), 0) AS fn,
                        COALESCE((SELECT COUNT(*) FROM xref x
                                   WHERE x.from_book=v.book
                                     AND x.from_chapter=v.chapter
                                     AND x.from_verse=v.verse), 0) AS xn
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
        let xrefs = self.load_xrefs(book, chapter)?;

        Ok(Passage {
            translation: self.translation.clone(),
            book_code: book.to_string(),
            book_name,
            book_abbrev,
            chapter,
            verses,
            headings,
            footnotes,
            xrefs,
        })
    }

    fn load_footnotes(&self, book: &str, chapter: i64) -> Result<Vec<Footnote>> {
        // The `footnote` table is currently unpopulated — there's no
        // upstream source in the pinned scrollmapper commit. The schema
        // and loader stay so a future ingest can light the K-popup
        // body without further plumbing.
        let prefix = format!("{book}.{chapter}.");
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, verse_osis, kind, body FROM footnote
             WHERE translation=?1 AND verse_osis LIKE ?2 || '%'
             ORDER BY id",
        )?;
        let footnotes: Vec<Footnote> = stmt
            .query_map(params![self.translation, prefix], |r| {
                Ok(Footnote {
                    id: r.get(0)?,
                    verse_osis: r.get(1)?,
                    kind: r.get(2)?,
                    body: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(footnotes)
    }

    fn load_xrefs(&self, book: &str, chapter: i64) -> Result<Vec<Xref>> {
        // LEFT JOIN to_book's label table for the active translation so
        // the UI can render localized abbreviations without holding the
        // full `books` list. Falls back to the OSIS code when a label is
        // missing (a partial-import edge case rather than expected steady-
        // state).
        let mut stmt = self.conn.prepare_cached(
            "SELECT x.from_verse,
                    x.to_book,
                    COALESCE(bl.abbreviation, x.to_book) AS to_abbrev,
                    x.to_chapter,
                    x.to_verse_start,
                    x.to_verse_end,
                    x.votes
             FROM xref x
             LEFT JOIN book_label bl
               ON bl.book = x.to_book AND bl.translation = ?3
             WHERE x.from_book = ?1 AND x.from_chapter = ?2
             ORDER BY x.from_verse, x.votes DESC",
        )?;
        let xrefs: Vec<Xref> = stmt
            .query_map(params![book, chapter, self.translation], |r| {
                Ok(Xref {
                    from_verse: r.get(0)?,
                    to_book: r.get(1)?,
                    to_book_abbrev: r.get(2)?,
                    to_chapter: r.get(3)?,
                    to_verse_start: r.get(4)?,
                    to_verse_end: r.get(5)?,
                    votes: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(xrefs)
    }
}
