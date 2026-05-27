//! `turbo-bible import <file.json>` — build a per-translation `SQLite`
//! `<code>.db` from a user-supplied JSON file and drop it into the
//! translations directory, ready to read on the next launch.
//!
//! Unlike the offline data pipeline ([`turbo-bible-data`]), this needs
//! no scrollmapper checkout: the JSON carries the verse text, the
//! metadata comes from CLI flags, and the schema is built here. The
//! produced file is the same shape `Db::open_ro` expects — the
//! `meta.code` is set to the `--code` value so the runtime's
//! filename/meta check passes by construction.
//!
//! See `docs/IMPORT.md` for the input format and the output schema.
//!
//! [`turbo-bible-data`]: ../../turbo-bible-data/index.html

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, params};
use serde::Deserialize;

/// Schema version stamped into `meta.schema_version`. Must track
/// [`SCHEMA_VERSION`] / `TRANSLATION_SCHEMA_SQL` in
/// `crates/turbo-bible-data/src/schema.rs`.
const SCHEMA_VERSION: i64 = 1;

/// `meta.source_commit` value for imported translations — they have no
/// scrollmapper provenance, so we record the import path instead of a
/// git SHA. The column is `NOT NULL`; nothing reads it at runtime.
const IMPORT_PROVENANCE: &str = "user-import";

/// Per-translation schema, copied verbatim from
/// `crates/turbo-bible-data/src/schema.rs` (`TRANSLATION_SCHEMA_SQL`).
/// The TUI crate is standalone and does not depend on the data crate,
/// so the two must stay in sync by hand — same deliberate duplication
/// as `turbo-bible-data/src/osis.rs`. The full schema (incl. the empty
/// `heading`/`footnote` tables and the FTS triggers) is required: the
/// runtime queries those tables, and the `verse_ai` trigger is what
/// populates `verse_fts` as rows are inserted below.
const TRANSLATION_SCHEMA_SQL: &str = "
CREATE TABLE meta (
  code           TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  language       TEXT NOT NULL,
  license        TEXT NOT NULL,
  attribution    TEXT NOT NULL,
  source_commit  TEXT NOT NULL,
  built_at       INTEGER NOT NULL,
  verse_count    INTEGER NOT NULL,
  schema_version INTEGER NOT NULL
);

CREATE TABLE book (
  code      TEXT PRIMARY KEY,
  testament TEXT NOT NULL CHECK (testament IN ('OT','NT')),
  ord       INTEGER NOT NULL UNIQUE
);

CREATE TABLE book_label (
  book         TEXT PRIMARY KEY REFERENCES book(code),
  name         TEXT NOT NULL,
  abbreviation TEXT NOT NULL,
  full_name    TEXT
);

CREATE TABLE verse (
  book    TEXT NOT NULL REFERENCES book(code),
  chapter INTEGER NOT NULL,
  verse   INTEGER NOT NULL,
  osis_id TEXT NOT NULL,
  text    TEXT NOT NULL,
  PRIMARY KEY (book, chapter, verse)
);
CREATE INDEX verse_osis_idx ON verse(osis_id);

CREATE TABLE heading (
  book          TEXT NOT NULL REFERENCES book(code),
  chapter       INTEGER NOT NULL,
  before_verse  INTEGER NOT NULL,
  style         TEXT NOT NULL,
  text          TEXT NOT NULL
);
CREATE INDEX heading_loc_idx ON heading(book, chapter, before_verse);

CREATE TABLE footnote (
  id          TEXT NOT NULL,
  verse_osis  TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('f','x')),
  body        TEXT NOT NULL,
  PRIMARY KEY (id)
);
CREATE INDEX footnote_verse_idx ON footnote(verse_osis);

CREATE VIRTUAL TABLE verse_fts USING fts5(
  text,
  content='verse',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 1',
  prefix='2 3'
);

-- Keep FTS in sync with `verse`; the AFTER INSERT trigger is what
-- indexes the verses inserted during import.
CREATE TRIGGER verse_ai AFTER INSERT ON verse BEGIN
  INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER verse_ad AFTER DELETE ON verse BEGIN
  INSERT INTO verse_fts(verse_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;
CREATE TRIGGER verse_au AFTER UPDATE ON verse BEGIN
  INSERT INTO verse_fts(verse_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
  INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
END;
";

/// One canonical Protestant book: OSIS code, default English label,
/// default abbreviation, testament, and canonical ordinal. Mirrors
/// `turbo-bible-data`'s `osis::BOOKS` + `labels::KJV_LABELS`.
struct Canon {
    osis: &'static str,
    name: &'static str,
    abbr: &'static str,
    testament: &'static str,
    ord: i64,
}

#[rustfmt::skip]
const CANON: &[Canon] = &[
    Canon { osis: "GEN", name: "Genesis",         abbr: "Gen",     testament: "OT", ord: 1 },
    Canon { osis: "EXO", name: "Exodus",          abbr: "Exo",     testament: "OT", ord: 2 },
    Canon { osis: "LEV", name: "Leviticus",       abbr: "Lev",     testament: "OT", ord: 3 },
    Canon { osis: "NUM", name: "Numbers",         abbr: "Num",     testament: "OT", ord: 4 },
    Canon { osis: "DEU", name: "Deuteronomy",     abbr: "Deut",    testament: "OT", ord: 5 },
    Canon { osis: "JOS", name: "Joshua",          abbr: "Josh",    testament: "OT", ord: 6 },
    Canon { osis: "JDG", name: "Judges",          abbr: "Judg",    testament: "OT", ord: 7 },
    Canon { osis: "RUT", name: "Ruth",            abbr: "Ruth",    testament: "OT", ord: 8 },
    Canon { osis: "1SA", name: "1 Samuel",        abbr: "1 Sam",   testament: "OT", ord: 9 },
    Canon { osis: "2SA", name: "2 Samuel",        abbr: "2 Sam",   testament: "OT", ord: 10 },
    Canon { osis: "1KI", name: "1 Kings",         abbr: "1 Kgs",   testament: "OT", ord: 11 },
    Canon { osis: "2KI", name: "2 Kings",         abbr: "2 Kgs",   testament: "OT", ord: 12 },
    Canon { osis: "1CH", name: "1 Chronicles",    abbr: "1 Chr",   testament: "OT", ord: 13 },
    Canon { osis: "2CH", name: "2 Chronicles",    abbr: "2 Chr",   testament: "OT", ord: 14 },
    Canon { osis: "EZR", name: "Ezra",            abbr: "Ezra",    testament: "OT", ord: 15 },
    Canon { osis: "NEH", name: "Nehemiah",        abbr: "Neh",     testament: "OT", ord: 16 },
    Canon { osis: "EST", name: "Esther",          abbr: "Esth",    testament: "OT", ord: 17 },
    Canon { osis: "JOB", name: "Job",             abbr: "Job",     testament: "OT", ord: 18 },
    Canon { osis: "PSA", name: "Psalms",          abbr: "Ps",      testament: "OT", ord: 19 },
    Canon { osis: "PRO", name: "Proverbs",        abbr: "Prov",    testament: "OT", ord: 20 },
    Canon { osis: "ECC", name: "Ecclesiastes",    abbr: "Eccl",    testament: "OT", ord: 21 },
    Canon { osis: "SNG", name: "Song of Solomon", abbr: "Song",    testament: "OT", ord: 22 },
    Canon { osis: "ISA", name: "Isaiah",          abbr: "Isa",     testament: "OT", ord: 23 },
    Canon { osis: "JER", name: "Jeremiah",        abbr: "Jer",     testament: "OT", ord: 24 },
    Canon { osis: "LAM", name: "Lamentations",    abbr: "Lam",     testament: "OT", ord: 25 },
    Canon { osis: "EZK", name: "Ezekiel",         abbr: "Ezek",    testament: "OT", ord: 26 },
    Canon { osis: "DAN", name: "Daniel",          abbr: "Dan",     testament: "OT", ord: 27 },
    Canon { osis: "HOS", name: "Hosea",           abbr: "Hos",     testament: "OT", ord: 28 },
    Canon { osis: "JOL", name: "Joel",            abbr: "Joel",    testament: "OT", ord: 29 },
    Canon { osis: "AMO", name: "Amos",            abbr: "Amos",    testament: "OT", ord: 30 },
    Canon { osis: "OBA", name: "Obadiah",         abbr: "Obad",    testament: "OT", ord: 31 },
    Canon { osis: "JON", name: "Jonah",           abbr: "Jonah",   testament: "OT", ord: 32 },
    Canon { osis: "MIC", name: "Micah",           abbr: "Mic",     testament: "OT", ord: 33 },
    Canon { osis: "NAM", name: "Nahum",           abbr: "Nah",     testament: "OT", ord: 34 },
    Canon { osis: "HAB", name: "Habakkuk",        abbr: "Hab",     testament: "OT", ord: 35 },
    Canon { osis: "ZEP", name: "Zephaniah",       abbr: "Zeph",    testament: "OT", ord: 36 },
    Canon { osis: "HAG", name: "Haggai",          abbr: "Hag",     testament: "OT", ord: 37 },
    Canon { osis: "ZEC", name: "Zechariah",       abbr: "Zech",    testament: "OT", ord: 38 },
    Canon { osis: "MAL", name: "Malachi",         abbr: "Mal",     testament: "OT", ord: 39 },
    Canon { osis: "MAT", name: "Matthew",         abbr: "Matt",    testament: "NT", ord: 40 },
    Canon { osis: "MRK", name: "Mark",            abbr: "Mark",    testament: "NT", ord: 41 },
    Canon { osis: "LUK", name: "Luke",            abbr: "Luke",    testament: "NT", ord: 42 },
    Canon { osis: "JHN", name: "John",            abbr: "John",    testament: "NT", ord: 43 },
    Canon { osis: "ACT", name: "Acts",            abbr: "Acts",    testament: "NT", ord: 44 },
    Canon { osis: "ROM", name: "Romans",          abbr: "Rom",     testament: "NT", ord: 45 },
    Canon { osis: "1CO", name: "1 Corinthians",   abbr: "1 Cor",   testament: "NT", ord: 46 },
    Canon { osis: "2CO", name: "2 Corinthians",   abbr: "2 Cor",   testament: "NT", ord: 47 },
    Canon { osis: "GAL", name: "Galatians",       abbr: "Gal",     testament: "NT", ord: 48 },
    Canon { osis: "EPH", name: "Ephesians",       abbr: "Eph",     testament: "NT", ord: 49 },
    Canon { osis: "PHP", name: "Philippians",     abbr: "Phil",    testament: "NT", ord: 50 },
    Canon { osis: "COL", name: "Colossians",      abbr: "Col",     testament: "NT", ord: 51 },
    Canon { osis: "1TH", name: "1 Thessalonians", abbr: "1 Thess", testament: "NT", ord: 52 },
    Canon { osis: "2TH", name: "2 Thessalonians", abbr: "2 Thess", testament: "NT", ord: 53 },
    Canon { osis: "1TI", name: "1 Timothy",       abbr: "1 Tim",   testament: "NT", ord: 54 },
    Canon { osis: "2TI", name: "2 Timothy",       abbr: "2 Tim",   testament: "NT", ord: 55 },
    Canon { osis: "TIT", name: "Titus",           abbr: "Titus",   testament: "NT", ord: 56 },
    Canon { osis: "PHM", name: "Philemon",        abbr: "Phlm",    testament: "NT", ord: 57 },
    Canon { osis: "HEB", name: "Hebrews",         abbr: "Heb",     testament: "NT", ord: 58 },
    Canon { osis: "JAS", name: "James",           abbr: "Jas",     testament: "NT", ord: 59 },
    Canon { osis: "1PE", name: "1 Peter",         abbr: "1 Pet",   testament: "NT", ord: 60 },
    Canon { osis: "2PE", name: "2 Peter",         abbr: "2 Pet",   testament: "NT", ord: 61 },
    Canon { osis: "1JN", name: "1 John",          abbr: "1 John",  testament: "NT", ord: 62 },
    Canon { osis: "2JN", name: "2 John",          abbr: "2 John",  testament: "NT", ord: 63 },
    Canon { osis: "3JN", name: "3 John",          abbr: "3 John",  testament: "NT", ord: 64 },
    Canon { osis: "JUD", name: "Jude",            abbr: "Jude",    testament: "NT", ord: 65 },
    Canon { osis: "REV", name: "Revelation",      abbr: "Rev",     testament: "NT", ord: 66 },
];

/// Resolve a JSON `book` identifier to its canonical entry. Matches the
/// OSIS code first, then the default English name; both case-insensitive.
fn resolve_book(ident: &str) -> Option<&'static Canon> {
    let t = ident.trim();
    CANON
        .iter()
        .find(|c| c.osis.eq_ignore_ascii_case(t) || c.name.eq_ignore_ascii_case(t))
}

/// CLI args for `turbo-bible import`.
#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// Path to the translation JSON file to import (see `docs/IMPORT.md`).
    pub file: PathBuf,
    /// Translation code — becomes both `meta.code` and the on-disk
    /// `<code>.db` filename. Lowercase letters, digits and hyphens only;
    /// must not be `xrefs`.
    #[arg(long)]
    pub code: String,
    /// Human-readable name shown in the translations picker.
    #[arg(long)]
    pub name: String,
    /// Language tag, e.g. `en`, `nb`, `la`.
    #[arg(long)]
    pub language: String,
    /// SPDX license expression for the text (e.g. `CC0-1.0`).
    #[arg(long, default_value = "LicenseRef-Unknown")]
    pub license: String,
    /// Attribution line (required by some licenses; empty otherwise).
    #[arg(long, default_value = "")]
    pub attribution: String,
    /// Overwrite an existing `<code>.db` instead of erroring.
    #[arg(long)]
    pub force: bool,
    /// Override `paths::translations_dir()` (for tests and dev).
    #[arg(long)]
    pub translations_dir: Option<PathBuf>,
}

/// Translation metadata destined for the `meta` row.
pub(crate) struct ImportMeta<'a> {
    pub(crate) code: &'a str,
    pub(crate) name: &'a str,
    pub(crate) language: &'a str,
    pub(crate) license: &'a str,
    pub(crate) attribution: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    books: usize,
    verses: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImportJson {
    #[serde(default)]
    books: Vec<ImportBook>,
}

#[derive(Debug, Deserialize)]
struct ImportBook {
    /// OSIS code (preferred) or English book name.
    book: String,
    /// Optional override for the displayed book name (else the English default).
    #[serde(default)]
    name: Option<String>,
    /// Optional override for the abbreviation (else the English default).
    #[serde(default, alias = "abbr")]
    abbreviation: Option<String>,
    #[serde(default)]
    chapters: Vec<ImportChapter>,
}

#[derive(Debug, Deserialize)]
struct ImportChapter {
    chapter: i64,
    #[serde(default)]
    verses: Vec<ImportVerse>,
}

#[derive(Debug, Deserialize)]
struct ImportVerse {
    verse: i64,
    text: String,
}

/// CLI entry point for `turbo-bible import`.
///
/// # Errors
/// Bad `--code`, unreadable/invalid JSON, an unknown book name, a
/// duplicate verse, an existing `<code>.db` without `--force`, or any
/// IO / `SQLite` failure while building the database.
pub fn run(args: &ImportArgs) -> Result<()> {
    validate_code(&args.code)?;
    let dir = resolve_dir(args.translations_dir.as_deref())?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    let final_path = dir.join(format!("{}.db", args.code));
    if final_path.exists() && !args.force {
        bail!(
            "{} already exists; pass --force to overwrite",
            final_path.display()
        );
    }

    let body =
        fs::read_to_string(&args.file).with_context(|| format!("read {}", args.file.display()))?;
    let parsed: ImportJson = serde_json::from_str(&body)
        .with_context(|| format!("parse {} as turbo-bible import JSON", args.file.display()))?;

    let meta = ImportMeta {
        code: &args.code,
        name: &args.name,
        language: &args.language,
        license: &args.license,
        attribution: &args.attribution,
    };

    // Build into a sibling tempfile, then atomic-rename — a partial build
    // never leaves a half-written `<code>.db` for the runtime to open.
    let tmp = tempfile::NamedTempFile::new_in(&dir)
        .with_context(|| format!("create temp file in {}", dir.display()))?;
    let stats =
        build_db(tmp.path(), &meta, &parsed).with_context(|| format!("build {}.db", args.code))?;
    tmp.persist(&final_path)
        .with_context(|| format!("persist {}", final_path.display()))?;

    eprintln!(
        "import: wrote {}.db ({} book(s), {} verse(s)) to {}",
        args.code,
        stats.books,
        stats.verses,
        dir.display()
    );
    Ok(())
}

/// Apply the schema and ingest `json` into a fresh DB at `path`.
/// Factored out of [`run`] so tests exercise the build without the CLI.
pub(crate) fn build_db(path: &Path, meta: &ImportMeta<'_>, json: &ImportJson) -> Result<Stats> {
    let mut conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch(TRANSLATION_SCHEMA_SQL)
        .context("apply translation schema")?;

    let tx = conn.transaction()?;
    let mut stats = Stats::default();
    {
        let mut book_stmt =
            tx.prepare("INSERT INTO book(code, testament, ord) VALUES (?1, ?2, ?3)")?;
        let mut label_stmt = tx.prepare(
            "INSERT INTO book_label(book, name, abbreviation, full_name) VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut verse_stmt = tx.prepare(
            "INSERT INTO verse(book, chapter, verse, osis_id, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        let mut seen: HashSet<&'static str> = HashSet::new();
        for b in &json.books {
            let canon = resolve_book(&b.book).ok_or_else(|| {
                anyhow!(
                    "unknown book {:?} — use an OSIS code (e.g. \"JHN\") or English name (e.g. \"John\")",
                    b.book
                )
            })?;
            if !seen.insert(canon.osis) {
                bail!(
                    "book {:?} (resolved to {}) is listed more than once",
                    b.book,
                    canon.osis
                );
            }
            let name = b.name.as_deref().unwrap_or(canon.name);
            let abbr = b.abbreviation.as_deref().unwrap_or(canon.abbr);

            // Insert the book/label row lazily — only once the book has a
            // verse — so a book with empty `chapters`/`verses` never produces
            // a navigable-but-empty entry the reader can't render.
            let mut book_inserted = false;
            for ch in &b.chapters {
                for v in &ch.verses {
                    if ch.chapter < 1 || v.verse < 1 {
                        bail!(
                            "{}: chapter and verse numbers must be >= 1 (got {}:{})",
                            canon.osis,
                            ch.chapter,
                            v.verse
                        );
                    }
                    if !book_inserted {
                        book_stmt.execute(params![canon.osis, canon.testament, canon.ord])?;
                        label_stmt.execute(params![canon.osis, name, abbr, name])?;
                        stats.books += 1;
                        book_inserted = true;
                    }
                    let osis_id = format!("{}.{}.{}", canon.osis, ch.chapter, v.verse);
                    verse_stmt
                        .execute(params![
                            canon.osis,
                            ch.chapter,
                            v.verse,
                            osis_id,
                            v.text.trim()
                        ])
                        .with_context(|| {
                            format!("insert {} {}:{}", canon.osis, ch.chapter, v.verse)
                        })?;
                    stats.verses += 1;
                }
            }
        }
    }

    if stats.verses == 0 {
        bail!("no verses found — the import would produce an empty translation");
    }

    let built_at = i64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("system clock is before the unix epoch")?
            .as_secs(),
    )
    .context("build timestamp overflows i64")?;
    tx.execute(
        "INSERT INTO meta
           (code, name, language, license, attribution,
            source_commit, built_at, verse_count, schema_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            meta.code,
            meta.name,
            meta.language,
            meta.license,
            meta.attribution,
            IMPORT_PROVENANCE,
            built_at,
            stats.verses,
            SCHEMA_VERSION,
        ],
    )?;
    tx.commit()?;

    conn.execute_batch("VACUUM; PRAGMA optimize;")?;
    Ok(stats)
}

/// Reject codes that wouldn't round-trip as a `<code>.db` filename, collide
/// with the reserved `xrefs` slot, or shadow a built-in translation. Keeping
/// the charset to `[a-z0-9-]` (with an alphanumeric lead) matches the bundled
/// codes (`en-kjv`, `nb-1930`, …) and rules out path separators.
fn validate_code(code: &str) -> Result<()> {
    if code.is_empty() {
        bail!("--code must not be empty");
    }
    if code == "xrefs" {
        bail!("--code \"xrefs\" is reserved for the cross-references database");
    }
    if !code
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("--code {code:?} may contain only lowercase letters, digits, and hyphens");
    }
    if !code.as_bytes()[0].is_ascii_alphanumeric() {
        bail!("--code {code:?} must start with a letter or digit");
    }
    // Refuse to shadow a bundled translation: a custom DB written to `en-kjv.db`
    // would be overwritten by `install --force`, and the picker would still
    // label it with the built-in's name. Pick a distinct code instead.
    if crate::manifest::TranslationManifestEntry::by_code(code).is_some() {
        bail!(
            "--code {code:?} is a built-in translation; choose a different code \
             (e.g. {code:?} with a suffix)"
        );
    }
    Ok(())
}

fn resolve_dir(override_path: Option<&Path>) -> Result<PathBuf> {
    match override_path {
        Some(p) => Ok(p.to_path_buf()),
        None => crate::paths::translations_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OpenFlags;

    fn meta() -> ImportMeta<'static> {
        ImportMeta {
            code: "zz-test",
            name: "Test Version",
            language: "en",
            license: "CC0-1.0",
            attribution: "",
        }
    }

    /// Mixes an OSIS code (`JHN`) and an English name (`Genesis`) with a
    /// label override, untrimmed text, and two verses in a chapter.
    fn sample_json() -> ImportJson {
        serde_json::from_str(
            r#"{
              "books": [
                { "book": "JHN", "abbreviation": "Jn", "chapters": [
                    { "chapter": 3, "verses": [
                        { "verse": 16, "text": "  For God so loved the world  " }
                    ] }
                ] },
                { "book": "Genesis", "chapters": [
                    { "chapter": 1, "verses": [
                        { "verse": 1, "text": "In the beginning" },
                        { "verse": 2, "text": "And the earth was without form" }
                    ] }
                ] }
              ]
            }"#,
        )
        .expect("parse sample json")
    }

    fn open_ro(path: &Path) -> Connection {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open ro")
    }

    #[test]
    fn build_db_writes_meta_books_and_verses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        let stats = build_db(&path, &meta(), &sample_json()).expect("build");
        assert_eq!(
            stats,
            Stats {
                books: 2,
                verses: 3
            }
        );

        let conn = open_ro(&path);
        let (code, name, lang, lic, ver, sv): (String, String, String, String, i64, i64) = conn
            .query_row(
                "SELECT code, name, language, license, verse_count, schema_version FROM meta",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(code, "zz-test");
        assert_eq!(name, "Test Version");
        assert_eq!(lang, "en");
        assert_eq!(lic, "CC0-1.0");
        assert_eq!(sv, SCHEMA_VERSION);

        let books: i64 = conn
            .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
            .unwrap();
        let labels: i64 = conn
            .query_row("SELECT COUNT(*) FROM book_label", [], |r| r.get(0))
            .unwrap();
        let verses: i64 = conn
            .query_row("SELECT COUNT(*) FROM verse", [], |r| r.get(0))
            .unwrap();
        assert_eq!((books, labels, verses), (2, 2, ver));
    }

    #[test]
    fn fts_index_is_populated_by_the_insert_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        build_db(&path, &meta(), &sample_json()).expect("build");
        let conn = open_ro(&path);
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM verse_fts WHERE verse_fts MATCH 'loved'",
                [],
                |r| r.get(0),
            )
            .expect("fts query");
        assert_eq!(hits, 1, "verse_fts should index the imported verse");
    }

    #[test]
    fn label_override_else_default_and_text_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        build_db(&path, &meta(), &sample_json()).expect("build");
        let conn = open_ro(&path);

        // JHN supplied an abbreviation override but no name → English name default.
        let (jhn_name, jhn_abbr): (String, String) = conn
            .query_row(
                "SELECT name, abbreviation FROM book_label WHERE book = 'JHN'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((jhn_name.as_str(), jhn_abbr.as_str()), ("John", "Jn"));

        // Genesis supplied neither → both English defaults.
        let gen_abbr: String = conn
            .query_row(
                "SELECT abbreviation FROM book_label WHERE book = 'GEN'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gen_abbr, "Gen");

        let v16: String = conn
            .query_row(
                "SELECT text FROM verse WHERE book = 'JHN' AND chapter = 3 AND verse = 16",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v16, "For God so loved the world");
    }

    #[test]
    fn partial_bible_inserts_only_supplied_books() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        let json: ImportJson = serde_json::from_str(
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 1, "verses": [ { "verse": 1, "text": "In the beginning was the Word" } ] } ] } ] }"#,
        )
        .unwrap();
        let stats = build_db(&path, &meta(), &json).expect("build");
        assert_eq!(stats.books, 1);
        let conn = open_ro(&path);
        let books: i64 = conn
            .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
            .unwrap();
        assert_eq!(books, 1, "only the supplied book should exist (not 66)");
    }

    #[test]
    fn book_with_no_verses_is_not_inserted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        // GEN carries empty chapters; only JHN has a verse. A navigable-but-
        // empty book would break the reader, so GEN must not be inserted.
        let json: ImportJson = serde_json::from_str(
            r#"{ "books": [
                { "book": "GEN", "chapters": [] },
                { "book": "JHN", "chapters": [
                    { "chapter": 3, "verses": [ { "verse": 16, "text": "x" } ] } ] }
            ] }"#,
        )
        .unwrap();
        let stats = build_db(&path, &meta(), &json).expect("build");
        assert_eq!(stats.books, 1, "empty GEN must not count");
        let conn = open_ro(&path);
        let gen_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM book WHERE code = 'GEN'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(gen_rows, 0, "an empty book must not be inserted");
        let labels: i64 = conn
            .query_row("SELECT COUNT(*) FROM book_label", [], |r| r.get(0))
            .unwrap();
        assert_eq!(labels, 1, "book_label must not gain an orphan empty book");
    }

    /// Guards against [`TRANSLATION_SCHEMA_SQL`] drifting from the data
    /// pipeline. Compares the import-built schema against the bundled
    /// `en-kjv.db` (produced by `turbo-bible-data`): object set (tables,
    /// indexes, triggers, FTS shadow tables) plus per-table columns.
    #[test]
    fn import_schema_matches_pipeline_built_db() {
        fn objects(conn: &Connection) -> Vec<(String, String)> {
            let mut stmt = conn
                .prepare(
                    "SELECT type, name FROM sqlite_master \
                     WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
                )
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        }
        fn columns(conn: &Connection, table: &str) -> Vec<(String, String, i64, i64)> {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(5)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        }

        let dir = tempfile::tempdir().unwrap();
        let imported = dir.path().join("zz-test.db");
        build_db(&imported, &meta(), &sample_json()).expect("build");

        let asset = crate::bundled::BUNDLED
            .iter()
            .find(|a| a.code == "en-kjv")
            .expect("en-kjv is bundled");
        let kjv = dir.path().join("en-kjv.db");
        let bytes = zstd::decode_all(std::io::Cursor::new(asset.bytes)).expect("decompress kjv");
        fs::write(&kjv, &bytes).unwrap();

        let a = open_ro(&imported);
        let b = open_ro(&kjv);
        assert_eq!(
            objects(&a),
            objects(&b),
            "import schema drifted from crates/turbo-bible-data/src/schema.rs"
        );
        for table in ["meta", "book", "book_label", "verse", "heading", "footnote"] {
            assert_eq!(
                columns(&a, table),
                columns(&b, table),
                "column drift in `{table}`"
            );
        }
    }

    #[test]
    fn unknown_book_is_rejected_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        let json: ImportJson = serde_json::from_str(
            r#"{ "books": [ { "book": "Tobit", "chapters": [
                { "chapter": 1, "verses": [ { "verse": 1, "text": "x" } ] } ] } ] }"#,
        )
        .unwrap();
        let err = build_db(&path, &meta(), &json).unwrap_err();
        assert!(format!("{err:#}").contains("Tobit"), "got: {err:#}");
    }

    #[test]
    fn non_positive_chapter_or_verse_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for (i, body) in [
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 0, "verses": [ { "verse": 1, "text": "x" } ] } ] } ] }"#,
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 3, "verses": [ { "verse": -1, "text": "x" } ] } ] } ] }"#,
        ]
        .iter()
        .enumerate()
        {
            let json: ImportJson = serde_json::from_str(body).unwrap();
            let path = dir.path().join(format!("zz-test-{i}.db"));
            let err = build_db(&path, &meta(), &json).unwrap_err();
            assert!(format!("{err:#}").contains(">= 1"), "got: {err:#}");
        }
    }

    #[test]
    fn duplicate_verse_is_surfaced_not_panicked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        let json: ImportJson = serde_json::from_str(
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 3, "verses": [
                    { "verse": 16, "text": "a" },
                    { "verse": 16, "text": "b" }
                ] } ] } ] }"#,
        )
        .unwrap();
        let err = build_db(&path, &meta(), &json).unwrap_err();
        assert!(format!("{err:#}").contains("JHN 3:16"), "got: {err:#}");
    }

    #[test]
    fn empty_input_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zz-test.db");
        let json: ImportJson = serde_json::from_str(r#"{ "books": [] }"#).unwrap();
        let err = build_db(&path, &meta(), &json).unwrap_err();
        assert!(format!("{err:#}").contains("no verses"), "got: {err:#}");
    }

    #[test]
    fn run_writes_db_and_rejects_existing_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("in.json");
        fs::write(
            &json_path,
            r#"{ "books": [ { "book": "JHN", "chapters": [
                { "chapter": 3, "verses": [ { "verse": 16, "text": "For God so loved" } ] } ] } ] }"#,
        )
        .unwrap();
        let mut args = ImportArgs {
            file: json_path,
            code: "zz-john".into(),
            name: "John".into(),
            language: "en".into(),
            license: "CC0-1.0".into(),
            attribution: String::new(),
            force: false,
            translations_dir: Some(dir.path().to_path_buf()),
        };
        run(&args).expect("first import");
        assert!(dir.path().join("zz-john.db").is_file());

        // Second run without --force errors; with --force it succeeds.
        let err = run(&args).unwrap_err();
        assert!(
            format!("{err:#}").contains("already exists"),
            "got: {err:#}"
        );
        args.force = true;
        run(&args).expect("force overwrite");
    }

    #[test]
    fn resolve_book_matches_osis_and_name_case_insensitively() {
        assert_eq!(resolve_book("JHN").unwrap().osis, "JHN");
        assert_eq!(resolve_book("jhn").unwrap().osis, "JHN");
        assert_eq!(resolve_book("John").unwrap().osis, "JHN");
        assert_eq!(resolve_book("  1 samuel ").unwrap().osis, "1SA");
        assert!(resolve_book("Tobit").is_none());
    }

    #[test]
    fn validate_code_rules() {
        assert!(validate_code("en-john").is_ok());
        assert!(validate_code("zz123").is_ok());
        assert!(validate_code("").is_err());
        assert!(validate_code("xrefs").is_err());
        assert!(validate_code("En-John").is_err()); // uppercase
        assert!(validate_code("a/b").is_err()); // path separator
        assert!(validate_code("-x").is_err()); // leading hyphen
        assert!(validate_code("en-kjv").is_err()); // shadows a built-in translation
    }

    #[test]
    fn canon_covers_all_66_books_with_dense_ordinals() {
        assert_eq!(CANON.len(), 66);
        let mut ords: Vec<i64> = CANON.iter().map(|c| c.ord).collect();
        ords.sort_unstable();
        assert_eq!(ords, (1..=66).collect::<Vec<_>>());
    }
}
