//! `SQLite` access layer.
//!
//! At startup [`Db::open_ro`] opens **one `Connection` per installed
//! translation**, each pointing at its `<code>.db` as the main database
//! with the shared `xrefs.db` attached under alias `xrefs`. `SQLite`'s
//! compile-time `SQLITE_MAX_ATTACHED` defaults to 10, so a single
//! connection couldn't hold all 11 translations + xrefs at once; the
//! per-translation-connection model sidesteps the limit entirely and
//! gives every translation its own `prepare_cached` cache.
//!
//! Translation tables (`verse`, `verse_fts`, `book_label`, `book`,
//! `heading`, `footnote`) are referenced unqualified — they're in the
//! active connection's `main` schema. The cross-references table is
//! `xrefs.xref`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, params};

#[derive(Debug, Clone)]
pub struct TranslationInfo {
    pub code: String,
    pub name: String,
    /// Read by `merge_picker_entries` to label on-disk translations that
    /// aren't in the static manifest (e.g. `turbo-bible import` output).
    pub language: String,
    #[expect(
        dead_code,
        reason = "roadmap: shown by the Translations picker's details panel"
    )]
    pub license: String,
    #[expect(
        dead_code,
        reason = "roadmap: surfaced by a future \"About this translation\" view; \
                  non-empty for CC-BY-family entries (e.g. pt-blivre)"
    )]
    pub attribution: String,
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

/// ATTACH alias under which each per-translation connection holds the
/// shared `xrefs.db`.
const XREFS_SCHEMA: &str = "xrefs";

#[derive(Debug)]
pub struct Db {
    /// One Connection per installed translation. Each has its
    /// `<code>.db` open as `main`; if `xrefs.db` is present it's
    /// attached as `xrefs`. Keyed by translation code.
    conns: HashMap<String, Connection>,
    /// Sorted by `code`. Built from each translation's `meta` table
    /// as connections are opened (initially or via [`Db::add_translation`]).
    translations: Vec<TranslationInfo>,
    active_code: String,
    /// Where `<code>.db` files live on disk. Stored so
    /// [`Db::add_translation`] can resolve a code → path without the
    /// caller threading the value through.
    translations_dir: PathBuf,
    /// Path to `xrefs.db` attached on every connection in
    /// [`Self::conns`]. Always populated — when the real DB isn't on
    /// disk, [`create_empty_xrefs_db`] seeds an empty stand-in so the
    /// `xref_note_count` subquery in [`Self::load_passage`] keeps
    /// resolving.
    xrefs_path: PathBuf,
}

impl Db {
    /// The currently active translation's connection. Search and quote
    /// modules go through this to share the `prepare_cached` pool with
    /// the rest of `db.rs`.
    pub(crate) fn conn(&self) -> &Connection {
        self.active_conn()
    }

    fn active_conn(&self) -> &Connection {
        self.conns
            .get(&self.active_code)
            .expect("active_code is always installed by construction")
    }

    /// The connection for an arbitrary installed translation. All
    /// per-translation connections are open for the `Db`'s lifetime, so
    /// reading a non-active translation (e.g. a second compare pane) is
    /// just a `HashMap` lookup — no open/close.
    fn conn_for(&self, code: &str) -> Result<&Connection> {
        self.conns
            .get(code)
            .ok_or_else(|| anyhow!("translation {code:?} not installed"))
    }

    /// Re-point the active translation without the books/label/passage
    /// probe that [`Self::try_switch_translation`] performs. Compare-pane
    /// focus changes call this so the search / quote / Find paths (which
    /// query the active connection) follow the focused pane.
    ///
    /// # Errors
    /// Fails if `code` is not installed.
    pub fn set_active(&mut self, code: &str) -> Result<()> {
        if !self.conns.contains_key(code) {
            bail!("translation {code:?} not installed");
        }
        self.active_code = code.to_string();
        Ok(())
    }

    #[must_use]
    pub fn translation(&self) -> &str {
        &self.active_code
    }

    /// All translations installed under the translations directory at
    /// the time [`Db::open_ro`] was called. Populated from each
    /// per-translation `meta` table.
    #[must_use]
    pub fn translations(&self) -> &[TranslationInfo] {
        &self.translations
    }

    /// Open one read-only `Connection` per `<code>.db` under
    /// `translations_dir`, each with the shared `xrefs.db` attached.
    ///
    /// # Errors
    /// Fails if the directory is missing, contains no translation files,
    /// is missing `xrefs.db`, or if any per-translation `meta` query
    /// errors (typically: stale file from an older `schema_version`).
    ///
    /// # Panics
    /// In debug builds, panics if `initial_translation` is empty.
    pub fn open_ro(translations_dir: &Path, initial_translation: &str) -> Result<Self> {
        debug_assert!(
            !initial_translation.is_empty(),
            "Db::open_ro requires a non-empty translation code; the install \
             routine guarantees at least one .db file is present"
        );

        if !translations_dir.is_dir() {
            bail!(
                "translations directory missing: {}",
                translations_dir.display()
            );
        }

        // Discover installed translations + the xrefs DB.
        let mut translation_files: Vec<(String, std::path::PathBuf)> = Vec::new();
        let mut xrefs_path: Option<std::path::PathBuf> = None;
        for entry in fs::read_dir(translations_dir)
            .with_context(|| format!("read_dir {}", translations_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("db") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow!("non-utf8 filename {}", path.display()))?;
            if stem == "xrefs" {
                xrefs_path = Some(path);
            } else {
                translation_files.push((stem.to_string(), path));
            }
        }
        if translation_files.is_empty() {
            bail!(
                "no translation .db files found in {} — run `turbo-bible install`",
                translations_dir.display()
            );
        }
        translation_files.sort_by(|a, b| a.0.cmp(&b.0));

        // Every per-translation connection needs `xrefs.xref` attached
        // because load_passage's `xref_note_count` subquery references
        // it unconditionally. If the real xrefs.db isn't on disk, seed
        // an empty stand-in so the ATTACH succeeds; load_xrefs() then
        // returns 0 rows until fetch::xrefs replaces the file.
        let xrefs_path = if let Some(p) = xrefs_path {
            p
        } else {
            let p = translations_dir.join("xrefs.db");
            create_empty_xrefs_db(&p)?;
            p
        };

        let mut conns: HashMap<String, Connection> = HashMap::new();
        let mut translations: Vec<TranslationInfo> = Vec::new();
        for (code, path) in &translation_files {
            let conn = open_translation_ro(path)
                .with_context(|| format!("open {} (main)", path.display()))?;
            attach_ro(&conn, &xrefs_path, XREFS_SCHEMA)?;
            let info = read_meta(&conn).with_context(|| format!("read meta for {code}"))?;
            if info.code != *code {
                bail!(
                    "translations/{code}.db has meta.code = {:?} — file/meta mismatch",
                    info.code
                );
            }
            translations.push(info);
            conns.insert(code.clone(), conn);
        }

        // Validate the requested initial translation actually exists.
        if !conns.contains_key(initial_translation) {
            bail!(
                "translation {:?} is not installed (have: {})",
                initial_translation,
                translations
                    .iter()
                    .map(|t| t.code.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(Self {
            conns,
            translations,
            active_code: initial_translation.to_string(),
            translations_dir: translations_dir.to_path_buf(),
            xrefs_path,
        })
    }

    /// Open and register a newly-downloaded translation. Idempotent —
    /// a no-op if the code is already loaded.
    ///
    /// # Errors
    /// Fails if `<code>.db` is missing under the translations dir or
    /// has an inconsistent `meta` table.
    pub fn add_translation(&mut self, code: &str) -> Result<()> {
        if self.conns.contains_key(code) {
            return Ok(());
        }
        let path = self.translations_dir.join(format!("{code}.db"));
        let conn = open_translation_ro(&path)
            .with_context(|| format!("open {} (main)", path.display()))?;
        attach_ro(&conn, &self.xrefs_path, XREFS_SCHEMA)?;
        let info = read_meta(&conn).with_context(|| format!("read meta for {code}"))?;
        if info.code != code {
            bail!(
                "translations/{code}.db has meta.code = {:?} — file/meta mismatch",
                info.code
            );
        }
        self.translations.push(info);
        self.translations.sort_by(|a, b| a.code.cmp(&b.code));
        self.conns.insert(code.to_string(), conn);
        Ok(())
    }

    /// ATTACH `xrefs.db` onto every translation connection. Used after
    /// the xrefs DB has been downloaded post-startup.
    ///
    /// # Errors
    /// Fails if the file can't be canonicalised or any ATTACH
    /// statement errors out.
    #[allow(
        dead_code,
        reason = "wired in once the K-popup learns to fetch xrefs on demand; \
                  the empty stand-in keeps everything working until then"
    )]
    pub fn attach_xrefs(&mut self, xrefs_path: &Path) -> Result<()> {
        for conn in self.conns.values() {
            // Drop the empty stand-in (or a previously attached real
            // file) before pointing at the new one.
            conn.execute(&format!("DETACH DATABASE {XREFS_SCHEMA}"), [])
                .context("DETACH xrefs (old)")?;
            attach_ro(conn, xrefs_path, XREFS_SCHEMA)?;
        }
        self.xrefs_path = xrefs_path.to_path_buf();
        Ok(())
    }

    /// `true` when the attached `xrefs.xref` table has any rows — i.e.
    /// the real openbible.info data is on disk, not the install-time
    /// empty stand-in. One short query per call; `SQLite` stops at the
    /// first row so the cost is independent of table size.
    #[must_use]
    #[allow(
        dead_code,
        reason = "wired in once the K-popup surfaces a 'fetch cross-references' affordance"
    )]
    pub fn has_xrefs(&self) -> bool {
        self.active_conn()
            .query_row::<i64, _, _>(
                &format!("SELECT 1 FROM {XREFS_SCHEMA}.xref LIMIT 1"),
                [],
                |r| r.get(0),
            )
            .is_ok()
    }

    /// The translations dir this `Db` was opened against. Useful for
    /// the fetcher, which writes new `<code>.db` files next to the
    /// existing ones.
    #[must_use]
    pub fn translations_dir(&self) -> &Path {
        &self.translations_dir
    }
}

fn read_meta(conn: &Connection) -> Result<TranslationInfo> {
    conn.query_row(
        "SELECT code, name, language, license, attribution \
         FROM meta LIMIT 1",
        [],
        |r| {
            Ok(TranslationInfo {
                code: r.get(0)?,
                name: r.get(1)?,
                language: r.get(2)?,
                license: r.get(3)?,
                attribution: r.get(4)?,
            })
        },
    )
    .map_err(Into::into)
}

impl Db {
    /// "King James Version (1769)  ·  en-kjv" — the subtitle shown on splash.
    pub fn translation_label(&self) -> Result<String> {
        let info = self
            .translations
            .iter()
            .find(|t| t.code == self.active_code)
            .ok_or_else(|| anyhow!("active translation {} not in cache", self.active_code))?;
        Ok(format!("{}  ·  {}", info.name, info.code))
    }

    /// Atomically swap the active translation to `code` and return the
    /// books / label / passage for the new translation. On any error,
    /// the previous translation is restored before returning, so a
    /// failed swap is observably a no-op from the caller's point of view.
    ///
    /// # Errors
    /// Fails if `code` is not installed or any of `list_books` /
    /// `translation_label` / `load_passage` errors under the new
    /// translation.
    pub fn try_switch_translation(
        &mut self,
        code: &str,
        book: &str,
        chapter: i64,
    ) -> Result<(Vec<Book>, String, Passage)> {
        if !self.conns.contains_key(code) {
            bail!("translation {code:?} not installed");
        }
        let prev_code = std::mem::replace(&mut self.active_code, code.to_string());
        let probe = (|| -> Result<_> {
            let books = self.list_books()?;
            let label = self.translation_label()?;
            // The reading position may not exist in the new translation (a
            // partial / imported edition that omits some books). Fall back to
            // its first book so switching never fails just because the current
            // book isn't shared; the caller reads the landed book back from
            // `Passage::book_code`.
            let present = books.iter().any(|b| b.code.as_str() == book);
            let (target_book, target_chapter): (String, i64) = if present {
                (book.to_string(), chapter)
            } else {
                match books.first() {
                    Some(b) => (b.code.clone(), 1),
                    None => (book.to_string(), chapter),
                }
            };
            // Clamp the chapter into the target book's range — the source
            // chapter can exceed it (e.g. a shorter book in the new edition).
            let max_chapter = self.chapter_count(&target_book)?.max(1);
            let passage = self.load_passage(&target_book, target_chapter.clamp(1, max_chapter))?;
            Ok((books, label, passage))
        })();
        if probe.is_err() {
            self.active_code = prev_code;
        }
        probe
    }

    /// # Errors
    /// Fails when the join on `book_label` errors.
    pub fn list_books(&self) -> Result<Vec<Book>> {
        let mut stmt = self.active_conn().prepare_cached(
            "SELECT b.code, bl.name, bl.abbreviation, b.testament, b.ord, bl.full_name
             FROM book b
             JOIN book_label bl ON bl.book = b.code
             ORDER BY b.ord",
        )?;
        let rows = stmt
            .query_map([], |r| {
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
        let mut stmt = self
            .active_conn()
            .prepare_cached("SELECT COALESCE(MAX(chapter), 0) FROM verse WHERE book=?1")?;
        let n: i64 = stmt.query_row(params![book], |r| r.get(0))?;
        Ok(n)
    }

    /// # Errors
    /// Fails when the `book_label` lookup returns no row or any of the
    /// verse/heading/footnote queries error.
    /// Fetch a single verse's body text in the active translation. `Ok(None)`
    /// when the reference doesn't resolve there (e.g. a bookmark whose
    /// versification differs from the current translation). Cheaper than
    /// loading the whole chapter; used for bookmark previews.
    pub fn verse_text(&self, book: &str, chapter: i64, verse: i64) -> Result<Option<String>> {
        let conn = self.active_conn();
        let mut stmt = conn
            .prepare_cached("SELECT text FROM verse WHERE book=?1 AND chapter=?2 AND verse=?3")?;
        match stmt.query_row(params![book, chapter, verse], |r| r.get::<_, String>(0)) {
            Ok(text) => Ok(Some(text)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn load_passage(&self, book: &str, chapter: i64) -> Result<Passage> {
        Self::load_passage_conn(self.active_conn(), &self.active_code, book, chapter)
    }

    /// Load a chapter from an arbitrary installed translation, regardless
    /// of which one is currently active. Used to seed a compare pane in a
    /// translation other than the focused one.
    ///
    /// # Errors
    /// Fails if `code` is not installed, the `book_label` lookup returns
    /// no row, or any verse/heading/footnote/xref query errors.
    pub fn load_passage_for(&self, code: &str, book: &str, chapter: i64) -> Result<Passage> {
        Self::load_passage_conn(self.conn_for(code)?, code, book, chapter)
    }

    /// Load a chapter from an arbitrary installed translation, clamping the
    /// `(book, chapter)` into what that translation actually contains. A
    /// *partial* / imported edition may omit `book` entirely (e.g. a John-only
    /// import); rather than erroring on the missing-book lookup, this falls
    /// back to the translation's first book and clamps the chapter into the
    /// target book's range. Mirrors the fallback in
    /// [`Self::try_switch_translation`], but for a non-active translation, so a
    /// compare pane can be seeded into any installed edition without crashing.
    /// The caller reads the landed book/chapter back from the returned
    /// [`Passage`]'s `book_code` / `chapter`.
    ///
    /// # Errors
    /// Fails if `code` is not installed, or any verse/heading/footnote/xref
    /// query errors. The missing-book case degrades gracefully (it does *not*
    /// error); only genuinely unexpected query failures propagate.
    pub fn load_passage_clamped_for(
        &self,
        code: &str,
        book: &str,
        chapter: i64,
    ) -> Result<Passage> {
        let (target_book, target_chapter) = {
            let conn = self.conn_for(code)?;
            // Is `book` present in this translation? (A partial edition may not
            // carry it.) `book` rows exist only for books with at least one
            // verse, so a present row guarantees the chapter query resolves.
            let present: bool = conn
                .prepare_cached("SELECT 1 FROM book WHERE code=?1 LIMIT 1")?
                .exists(params![book])?;
            let target_book: String = if present {
                book.to_string()
            } else {
                // Fall back to the translation's first book (canonical order).
                match conn
                    .prepare_cached("SELECT code FROM book ORDER BY ord LIMIT 1")?
                    .query_row([], |r| r.get::<_, String>(0))
                {
                    Ok(first) => first,
                    // No books at all is degenerate (an empty DB shouldn't be
                    // installable), but don't crash the reader over it — let
                    // the load below surface a clear error instead.
                    Err(rusqlite::Error::QueryReturnedNoRows) => book.to_string(),
                    Err(e) => return Err(e.into()),
                }
            };
            // Clamp the requested chapter into the target book's range.
            let max_chapter: i64 = conn
                .prepare_cached("SELECT COALESCE(MAX(chapter), 0) FROM verse WHERE book=?1")?
                .query_row(params![target_book], |r| r.get(0))?;
            (target_book, chapter.clamp(1, max_chapter.max(1)))
        };
        self.load_passage_for(code, &target_book, target_chapter)
    }

    fn load_passage_conn(
        conn: &Connection,
        code: &str,
        book: &str,
        chapter: i64,
    ) -> Result<Passage> {
        let (book_name, book_abbrev) = {
            let mut stmt = conn.prepare_cached(
                "SELECT COALESCE(full_name, name), abbreviation \
                 FROM book_label WHERE book=?1",
            )?;
            stmt.query_row(params![book], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
        };

        let verses = {
            // Footnotes are scoped to this translation (its own `main`
            // schema). Xrefs live in the shared `xrefs.xref` table,
            // attached to every per-translation connection at startup.
            let mut stmt = conn.prepare_cached(
                "SELECT v.verse, v.text,
                        COALESCE((SELECT COUNT(*) FROM footnote f
                                   WHERE f.verse_osis=v.osis_id
                                     AND f.kind='f'), 0) AS fn,
                        COALESCE((SELECT COUNT(*) FROM xrefs.xref x
                                   WHERE x.from_book=v.book
                                     AND x.from_chapter=v.chapter
                                     AND x.from_verse=v.verse), 0) AS xn
                 FROM verse v
                 WHERE v.book=?1 AND v.chapter=?2
                 ORDER BY v.verse",
            )?;
            stmt.query_map(params![book, chapter], |r| {
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
            let mut stmt = conn.prepare_cached(
                "SELECT before_verse, style, text FROM heading
                 WHERE book=?1 AND chapter=?2
                 ORDER BY before_verse, rowid",
            )?;
            stmt.query_map(params![book, chapter], |r| {
                Ok(Heading {
                    before_verse: r.get(0)?,
                    style: r.get(1)?,
                    text: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        let footnotes = Self::load_footnotes_conn(conn, book, chapter)?;
        let xrefs = Self::load_xrefs_conn(conn, book, chapter)?;

        Ok(Passage {
            translation: code.to_string(),
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

    fn load_footnotes_conn(conn: &Connection, book: &str, chapter: i64) -> Result<Vec<Footnote>> {
        // The `footnote` table is currently unpopulated — there's no
        // upstream source. The schema and loader stay so a future
        // ingest can light the K-popup body without further plumbing.
        let prefix = format!("{book}.{chapter}.");
        let mut stmt = conn.prepare_cached(
            "SELECT id, verse_osis, kind, body FROM footnote
             WHERE verse_osis LIKE ?1 || '%'
             ORDER BY id",
        )?;
        let footnotes: Vec<Footnote> = stmt
            .query_map(params![prefix], |r| {
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

    fn load_xrefs_conn(conn: &Connection, book: &str, chapter: i64) -> Result<Vec<Xref>> {
        // The empty xrefs.db stand-in (seeded at install) has the same
        // schema as the real one, so this query returns Vec::new()
        // naturally when the user hasn't fetched the real DB yet. No
        // pre-check needed.
        // LEFT JOIN to_book's label table (in the passage translation's
        // own `main` schema) so the UI can render localised
        // abbreviations without holding the full `books` list. Falls
        // back to the OSIS code when a label is missing.
        let mut stmt = conn.prepare_cached(
            "SELECT x.from_verse,
                    x.to_book,
                    COALESCE(bl.abbreviation, x.to_book) AS to_abbrev,
                    x.to_chapter,
                    x.to_verse_start,
                    x.to_verse_end,
                    x.votes
             FROM xrefs.xref x
             LEFT JOIN book_label bl
               ON bl.book = x.to_book
             WHERE x.from_book = ?1 AND x.from_chapter = ?2
             ORDER BY x.from_verse, x.votes DESC",
        )?;
        let xrefs: Vec<Xref> = stmt
            .query_map(params![book, chapter], |r| {
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

/// Cheap startup probe: list every translation code present in
/// `translations_dir` (one entry per `<code>.db`, excluding the
/// reserved `xrefs.db`), sorted alphabetically. Used by
/// `resolve_translation` to pick a default before [`Db::open_ro`]
/// runs.
///
/// # Errors
/// Propagates `read_dir` failures.
pub fn installed_codes(translations_dir: &Path) -> Result<Vec<String>> {
    if !translations_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut codes = Vec::new();
    for entry in fs::read_dir(translations_dir)
        .with_context(|| format!("read_dir {}", translations_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("db") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem != "xrefs" {
            codes.push(stem.to_string());
        }
    }
    codes.sort();
    Ok(codes)
}

fn attach_ro(conn: &Connection, path: &Path, alias: &str) -> Result<()> {
    // `file:...?mode=ro` is the official URI-form opt-in to a read-only
    // ATTACH — SQLite has no `ATTACH ... READONLY` syntax. The filename is
    // bound as a parameter (so a `'` in the path can't break out of the SQL
    // string literal) and the path is percent-encoded (so a space or a
    // `?`/`#`/`%` can't be misread as a URI delimiter). `alias` is a
    // compile-time constant identifier (XREFS_SCHEMA), not user input, so it
    // stays inline — identifiers can't be bound.
    let abs = fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    let uri = format!("file://{}?mode=ro", encode_uri_path(&abs.to_string_lossy()));
    conn.execute(&format!("ATTACH DATABASE ?1 AS {alias}"), params![uri])
        .with_context(|| format!("ATTACH {alias} ({})", path.display()))?;
    Ok(())
}

/// Percent-encode the characters that would otherwise break a `SQLite` `file:`
/// URI path: a space, the `?`/`#` URI delimiters, and `%` itself (so an
/// existing `%` isn't read as an escape introducer). `/` and non-ASCII UTF-8
/// pass through unchanged — `SQLite`'s URI parser accepts them as-is, matching
/// the pre-encoding behaviour for ordinary paths.
fn encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            _ => out.push(c),
        }
    }
    out
}

/// Create an empty `xrefs.db` stand-in at `path` with the columns
/// [`Db::load_passage`] queries against. Used as the seed at install
/// time when no real xrefs DB is present yet: the file exists on
/// disk so every per-translation connection can ATTACH it read-only
/// like the real thing, and [`Db::load_xrefs`] returns 0 rows until
/// [`fetch::xrefs`] replaces it atomically.
///
/// # Errors
/// Propagates IO and `SQLite` open / CREATE TABLE failures.
pub fn create_empty_xrefs_db(path: &Path) -> Result<()> {
    let conn = Connection::open(path)
        .with_context(|| format!("create empty xrefs.db at {}", path.display()))?;
    conn.execute(
        "CREATE TABLE xref (
            from_book TEXT, from_chapter INT, from_verse INT,
            to_book TEXT, to_chapter INT,
            to_verse_start INT, to_verse_end INT,
            votes INT
        )",
        [],
    )
    .context("create empty xref table")?;
    Ok(())
}

/// Open a per-translation `.db` file read-only as a fresh
/// `Connection`. Caller is responsible for attaching `xrefs.db` (or
/// an empty in-memory stand-in via [`attach_empty_xrefs`]).
///
/// We don't set `query_only=ON` because the empty in-memory xrefs
/// stand-in needs writes during setup; per-attach readonly flags
/// (`SQLITE_OPEN_READ_ONLY` on the main DB, `file://?mode=ro` on the
/// real xrefs) already prevent on-disk modification.
fn open_translation_ro(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decompress an extra `<code>.db.zst` into `dir` from the data
    /// pipeline's output at `<workspace>/dist/translations/`. Used by
    /// tests that need a translation other than the bundled KJV (and
    /// `xrefs.db`). Returns `Ok(false)` if the source file is missing
    /// — caller can use `#[ignore]` semantics to skip.
    fn install_extra(dir: &Path, file: &str) -> Result<bool> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .ok_or_else(|| anyhow!("can't resolve workspace root"))?;
        let src = workspace_root.join("dist/translations").join(file);
        if !src.exists() {
            return Ok(false);
        }
        let compressed = fs::read(&src).with_context(|| format!("read {}", src.display()))?;
        let decoded = zstd::decode_all(std::io::Cursor::new(&compressed))
            .with_context(|| format!("decompress {}", src.display()))?;
        let stem = Path::new(file)
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".zst"))
            .ok_or_else(|| anyhow!("unexpected file name: {file}"))?;
        fs::write(dir.join(stem), &decoded)?;
        Ok(true)
    }

    #[test]
    fn open_ro_attaches_bundled_default() {
        // Fresh `cargo install` state: only KJV is extracted, xrefs
        // is not yet present. Db should still open cleanly.
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");

        let db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        assert_eq!(db.translation(), "en-kjv");
        assert!(db.translations().iter().any(|t| t.code == "en-kjv"));
        assert!(!db.has_xrefs(), "fresh install has no xrefs.db yet");

        let label = db.translation_label().expect("label");
        assert!(label.contains("King James"));
        assert!(label.contains("en-kjv"));

        let books = db.list_books().expect("list_books");
        assert_eq!(books.len(), 66);

        let passage = db.load_passage("JHN", 3).expect("John 3");
        assert!(
            passage
                .verses
                .iter()
                .any(|v| v.number == 16 && v.text.contains("God") && v.text.contains("world"))
        );
        // No xrefs.db means an empty xref list — not an error.
        assert!(passage.xrefs.is_empty());
    }

    #[test]
    fn load_passage_for_matches_active_and_leaves_it_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");

        // `load_passage_for(active, ..)` matches `load_passage(..)`.
        let active = db.load_passage("JHN", 3).expect("active John 3");
        let via_for = db.load_passage_for("en-kjv", "JHN", 3).expect("for John 3");
        assert_eq!(active.verses.len(), via_for.verses.len());
        assert_eq!(active.verses[15].text, via_for.verses[15].text);

        // set_active round-trips and is observable via load_passage.
        db.set_active("en-kjv").expect("set_active");
        assert_eq!(db.translation(), "en-kjv");

        // A second translation (when bundled) is readable via `_for` without
        // changing the active one.
        if install_extra(tmp.path(), "nb-1930.db.zst").expect("install_extra") {
            db.add_translation("nb-1930").expect("add nb-1930");
            let nb = db.load_passage_for("nb-1930", "JHN", 3).expect("nb John 3");
            assert_eq!(nb.translation, "nb-1930");
            assert_ne!(nb.verses[0].text, active.verses[0].text);
            assert_eq!(db.translation(), "en-kjv", "_for must not change active");
        }
    }

    #[test]
    fn set_active_rejects_uninstalled() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        assert!(db.set_active("zz-nope").is_err());
        assert_eq!(db.translation(), "en-kjv", "failed set_active is a no-op");
    }

    #[test]
    fn add_translation_registers_a_new_db() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        if !install_extra(tmp.path(), "nb-1930.db.zst").expect("install_extra") {
            eprintln!(
                "skip: dist/translations/nb-1930.db.zst missing — run `just bundle-translations`"
            );
            return;
        }

        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        assert!(db.translations().iter().any(|t| t.code == "en-kjv"));

        // Capture the KJV verse before swapping — load_passage reads
        // from whichever translation is active.
        let kjv = db.load_passage("JHN", 1).expect("kjv John 1");

        db.add_translation("nb-1930").expect("add_translation");
        assert!(db.translations().iter().any(|t| t.code == "nb-1930"));

        let (_books, _label, nb_passage) = db
            .try_switch_translation("nb-1930", "JHN", 1)
            .expect("switch");
        assert_eq!(db.translation(), "nb-1930");
        assert_ne!(nb_passage.verses[0].text, kjv.verses[0].text);
    }

    #[test]
    fn try_switch_translation_rejects_uninstalled() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        let err = db.try_switch_translation("xx-bogus", "JHN", 3).unwrap_err();
        assert!(format!("{err}").contains("xx-bogus"));
        assert_eq!(db.translation(), "en-kjv");
    }

    /// Build a partial (John-only) translation DB at `<dir>/<code>.db` using
    /// the import pipeline, so tests can register an edition that omits most
    /// books without depending on bundled assets.
    fn install_john_only(dir: &Path, code: &str) {
        let json: crate::import::ImportJson = serde_json::from_str(
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 1, "verses": [
                    { "verse": 1, "text": "In the beginning was the Word" } ] },
                { "chapter": 3, "verses": [
                    { "verse": 16, "text": "For God so loved the world" } ] } ] } ] }"#,
        )
        .expect("parse partial import json");
        let meta = crate::import::ImportMeta {
            code,
            name: "John only",
            language: "en",
            license: "",
            attribution: "",
        };
        crate::import::build_db(&dir.join(format!("{code}.db")), &meta, &json)
            .expect("build partial db");
    }

    #[test]
    fn load_passage_clamped_for_falls_back_to_first_book_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");

        // A John-only edition that lacks Genesis. The compare-pane open path
        // used to `?`-propagate the missing-book lookup, crashing the TUI.
        install_john_only(tmp.path(), "zz-john");
        db.add_translation("zz-john").expect("register zz-john");

        // Requesting Genesis (absent) must land on the first available book
        // (JHN), not error.
        let p = db
            .load_passage_clamped_for("zz-john", "GEN", 1)
            .expect("absent book must clamp, not error");
        assert_eq!(p.book_code, "JHN", "absent book falls back to first book");
        assert_eq!(p.translation, "zz-john");
        assert!(
            !p.verses.is_empty(),
            "the fallback chapter must have verses"
        );

        // A present book with an out-of-range chapter clamps into range
        // (John has chapters 1 and 3 here; chapter 99 clamps to the max).
        let clamped = db
            .load_passage_clamped_for("zz-john", "JHN", 99)
            .expect("present book, over-range chapter must clamp");
        assert_eq!(clamped.book_code, "JHN");
        assert!(
            clamped.chapter <= 3,
            "chapter must clamp into the book's range, got {}",
            clamped.chapter
        );

        // A present book + chapter loads verbatim, leaving the active
        // translation untouched (the `_for` family never re-points active).
        let exact = db
            .load_passage_clamped_for("zz-john", "JHN", 3)
            .expect("present book + chapter");
        assert_eq!(exact.chapter, 3);
        assert!(exact.verses.iter().any(|v| v.number == 16));
        assert_eq!(
            db.translation(),
            "en-kjv",
            "_clamped_for must not change active"
        );
    }

    #[test]
    fn load_passage_clamped_for_rejects_uninstalled() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        let err = db
            .load_passage_clamped_for("xx-bogus", "JHN", 3)
            .unwrap_err();
        assert!(format!("{err}").contains("xx-bogus"));
    }

    #[test]
    fn open_ro_rejects_uninstalled_initial_translation() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let err = Db::open_ro(tmp.path(), "xx-bogus").unwrap_err();
        assert!(format!("{err}").contains("xx-bogus"));
    }

    #[test]
    fn open_ro_handles_paths_with_space_and_apostrophe() {
        // Regression: `attach_ro` used to interpolate the canonicalized path
        // into both a SQL string literal and a `file:` URI. A path with a
        // single quote broke the literal; a space/`?`/`#` broke the URI parse.
        // Open a Db under a dir whose path has both a space and an apostrophe
        // and confirm the xrefs ATTACH still resolves (load_passage queries
        // the attached `xrefs.xref` table, so a failed ATTACH would surface).
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("we ird's bibles");
        fs::create_dir_all(&dir).expect("create funky dir");
        crate::install::ensure_installed(&dir).expect("install into funky path");

        let db = Db::open_ro(&dir, "en-kjv").expect("open_ro under space+apostrophe path");
        let passage = db
            .load_passage("JHN", 3)
            .expect("load John 3 (ATTACH must resolve)");
        assert!(passage.verses.iter().any(|v| v.number == 16));
    }

    #[test]
    fn encode_uri_path_escapes_only_uri_breakers() {
        assert_eq!(encode_uri_path("/Users/bob/bibles"), "/Users/bob/bibles");
        assert_eq!(encode_uri_path("/Users/O'Brien/x"), "/Users/O'Brien/x"); // `'` is fine in a URI
        assert_eq!(encode_uri_path("/a b/c?d#e%f"), "/a%20b/c%3Fd%23e%25f");
        assert_eq!(encode_uri_path("/Bøker/blå"), "/Bøker/blå"); // non-ASCII passes through
    }

    #[test]
    fn attach_xrefs_after_startup_enables_load() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        // Skip when dist/ isn't populated.
        if !install_extra(tmp.path(), "xrefs.db.zst").expect("install_extra") {
            eprintln!(
                "skip: dist/translations/xrefs.db.zst missing — run `just bundle-translations`"
            );
            return;
        }
        // Open WITHOUT xrefs first, then attach to verify the
        // post-startup attach path used by the K-popup download flow.
        let xrefs_target = tmp.path().join("xrefs.db");
        let staged = tmp.path().join("xrefs.db.staged");
        fs::rename(&xrefs_target, &staged).expect("hide xrefs");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        assert!(!db.has_xrefs());
        assert!(db.load_passage("JHN", 3).expect("john 3").xrefs.is_empty());
        fs::rename(&staged, &xrefs_target).expect("unhide");
        db.attach_xrefs(&xrefs_target).expect("attach_xrefs");
        assert!(db.has_xrefs());
        let passage = db.load_passage("JHN", 3).expect("John 3 again");
        assert!(
            passage.xrefs.len() > 50,
            "expected lots of xrefs for John 3, got {}",
            passage.xrefs.len()
        );
    }
}
