//! `SQLite` access layer.
//!
//! At startup [`Db::open_ro`] opens **one `Connection` per installed
//! translation**, each pointing at its `<code>.db` as the main database
//! with the shared `xrefs.db` ATTACHed under alias `xrefs`. SQLite's
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
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, params};

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
    /// `<code>.db` open as `main` and the shared `xrefs.db` ATTACHed
    /// as `xrefs`. Keyed by translation code (`en-kjv`, not `en_kjv`).
    conns: HashMap<String, Connection>,
    /// Sorted by `code`. Built once at open from each per-translation
    /// `meta` table; never re-queried.
    translations: Vec<TranslationInfo>,
    active_code: String,
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
    /// `translations_dir`, each with the shared `xrefs.db` ATTACHed.
    ///
    /// # Errors
    /// Fails if the directory is missing, contains no translation files,
    /// is missing `xrefs.db`, or if any per-translation `meta` query
    /// errors (typically: stale file from an older schema_version).
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
        let xrefs_path = xrefs_path.ok_or_else(|| {
            anyhow!(
                "{}/xrefs.db missing — run `turbo-bible install --force`",
                translations_dir.display()
            )
        })?;
        translation_files.sort_by(|a, b| a.0.cmp(&b.0));

        // Open one Connection per translation, each with xrefs ATTACHed.
        let mut conns: HashMap<String, Connection> = HashMap::new();
        let mut translations: Vec<TranslationInfo> = Vec::new();
        for (code, path) in &translation_files {
            let conn = open_translation_ro(path)
                .with_context(|| format!("open {} (main)", path.display()))?;
            attach_ro(&conn, &xrefs_path, XREFS_SCHEMA)?;
            let info: TranslationInfo = conn
                .query_row(
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
                .with_context(|| format!("read meta for {code}"))?;
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
        })
    }

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
            Ok((
                self.list_books()?,
                self.translation_label()?,
                self.load_passage(book, chapter)?,
            ))
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
    pub fn load_passage(&self, book: &str, chapter: i64) -> Result<Passage> {
        let conn = self.active_conn();
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
            // ATTACHed to every per-translation connection at startup.
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

        let footnotes = self.load_footnotes(book, chapter)?;
        let xrefs = self.load_xrefs(book, chapter)?;

        Ok(Passage {
            translation: self.active_code.clone(),
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
        // upstream source. The schema and loader stay so a future
        // ingest can light the K-popup body without further plumbing.
        let prefix = format!("{book}.{chapter}.");
        let mut stmt = self.active_conn().prepare_cached(
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

    fn load_xrefs(&self, book: &str, chapter: i64) -> Result<Vec<Xref>> {
        // LEFT JOIN to_book's label table (in the active translation's
        // own `main` schema) so the UI can render localised
        // abbreviations without holding the full `books` list. Falls
        // back to the OSIS code when a label is missing.
        let mut stmt = self.active_conn().prepare_cached(
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
    // `file://...?mode=ro` is the official URI-form opt-in to a
    // read-only ATTACH. SQLite has no `ATTACH ... READONLY` syntax;
    // the URI is the workaround.
    let abs = fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    let uri = format!("file://{}?mode=ro", abs.display());
    let stmt = format!("ATTACH DATABASE '{uri}' AS {alias}");
    conn.execute(&stmt, [])
        .with_context(|| format!("ATTACH {alias} ({})", path.display()))?;
    Ok(())
}

/// Open a per-translation `.db` file read-only as a fresh
/// `Connection`. Caller is responsible for ATTACHing `xrefs.db` if
/// needed.
fn open_translation_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "query_only", "ON")?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_ro_attaches_bundled_translations() {
        // Stand up the install routine into a tempdir and then have
        // Db::open_ro discover everything.
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");

        let db = Db::open_ro(tmp.path(), "en-bsb").expect("open_ro");
        assert_eq!(db.translation(), "en-bsb");
        assert!(db.translations().len() >= 11);
        // Spot-check a known translation is in the cache.
        assert!(db.translations().iter().any(|t| t.code == "la-clementine"));

        let label = db.translation_label().expect("label");
        assert!(label.contains("Berean Standard Bible"));
        assert!(label.contains("en-bsb"));

        let books = db.list_books().expect("list_books");
        assert_eq!(books.len(), 66);

        let passage = db.load_passage("JHN", 3).expect("John 3");
        assert!(
            passage
                .verses
                .iter()
                .any(|v| v.number == 16 && v.text.contains("God") && v.text.contains("world"))
        );
    }

    #[test]
    fn try_switch_translation_routes_to_per_connection_pool() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        assert_eq!(db.translation(), "en-kjv");

        // John 1:1 in KJV begins with "In the beginning" (English).
        let kjv = db.load_passage("JHN", 1).expect("John 1 (KJV)");
        assert!(
            kjv.verses[0].text.starts_with("In the beginning"),
            "expected KJV JHN 1:1 in English, got {:?}",
            kjv.verses[0].text
        );

        let (_books, _label, nb_passage) = db
            .try_switch_translation("nb-1930", "JHN", 1)
            .expect("switch");
        assert_eq!(db.translation(), "nb-1930");
        // Same reference, different language → wholly different bytes.
        assert_ne!(kjv.verses[0].text, nb_passage.verses[0].text);
    }

    #[test]
    fn try_switch_translation_rejects_uninstalled() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let mut db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        let err = db.try_switch_translation("xx-bogus", "JHN", 3).unwrap_err();
        assert!(format!("{err}").contains("xx-bogus"));
        // Active stays unchanged.
        assert_eq!(db.translation(), "en-kjv");
    }

    #[test]
    fn open_ro_rejects_uninstalled_initial_translation() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let err = Db::open_ro(tmp.path(), "xx-bogus").unwrap_err();
        assert!(format!("{err}").contains("xx-bogus"));
    }

    #[test]
    fn load_xrefs_uses_shared_xrefs_schema() {
        let tmp = tempfile::tempdir().unwrap();
        crate::install::ensure_installed(tmp.path()).expect("install");
        let db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        // John 3 should have plenty of cross-references in openbible.info
        // data; the existing import test already asserts John 3:16
        // alone has ≥20 xrefs.
        let passage = db.load_passage("JHN", 3).expect("John 3");
        assert!(
            passage.xrefs.len() > 50,
            "expected lots of xrefs for John 3, got {}",
            passage.xrefs.len()
        );
    }
}
