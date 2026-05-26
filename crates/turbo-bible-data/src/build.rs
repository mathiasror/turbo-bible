//! `turbo-bible-data build` — parse scrollmapper JSON exports and
//! produce one self-contained `<code>.db` per translation, plus a
//! shared `xrefs.db`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Deserialize;

use crate::labels;
use crate::manifest_source::{ManifestSource, TranslationEntry};
use crate::osis::{BOOKS, lookup_osis};
use crate::schema::{SCHEMA_VERSION, TRANSLATION_SCHEMA_SQL, XREF_SCHEMA_SQL};
use crate::xrefs;

/// Public entry point for the `build` subcommand.
pub fn run(scrollmapper: &Path, manifest_path: &Path, out: &Path, only: &[String]) -> Result<()> {
    let source = ManifestSource::load(manifest_path)?;
    let scrollmapper_commit = git_head_commit(scrollmapper)?;
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let filter: Option<Vec<&str>> =
        (!only.is_empty()).then(|| only.iter().map(String::as_str).collect());

    for entry in &source.translations {
        if let Some(f) = &filter
            && !f.contains(&entry.code.as_str())
        {
            continue;
        }
        let db_path = out.join(format!("{}.db", entry.code));
        eprintln!("→ build {} -> {}", entry.code, db_path.display());
        build_translation(scrollmapper, entry, &db_path, &scrollmapper_commit)
            .with_context(|| format!("build {}", entry.code))?;
    }

    // xrefs only when nothing was filtered or `xrefs` was implied.
    if filter.is_none() {
        let xrefs_path = out.join("xrefs.db");
        eprintln!("→ build xrefs -> {}", xrefs_path.display());
        build_xrefs(scrollmapper, &xrefs_path).context("build xrefs.db")?;
    }
    Ok(())
}

fn build_translation(
    scrollmapper: &Path,
    entry: &TranslationEntry,
    db_path: &Path,
    scrollmapper_commit: &str,
) -> Result<()> {
    if db_path.exists() {
        fs::remove_file(db_path).with_context(|| format!("remove stale {}", db_path.display()))?;
    }
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch(TRANSLATION_SCHEMA_SQL)?;

    let json_path = scrollmapper.join(&entry.source_json);
    let parsed = load_translation_json(&json_path)
        .with_context(|| format!("parse {}", json_path.display()))?;

    let tx = conn.transaction()?;
    populate_book(&tx)?;
    populate_book_label(&tx, &entry.code)?;
    let verse_count = populate_verse(&tx, &entry.code, &parsed)?;
    populate_meta(&tx, entry, scrollmapper_commit, verse_count)?;
    tx.commit()?;

    conn.execute_batch("VACUUM; PRAGMA optimize;")?;
    Ok(())
}

fn build_xrefs(scrollmapper: &Path, db_path: &Path) -> Result<()> {
    if db_path.exists() {
        fs::remove_file(db_path).with_context(|| format!("remove stale {}", db_path.display()))?;
    }
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch(XREF_SCHEMA_SQL)?;
    let count = xrefs::build(scrollmapper, &mut conn)?;
    eprintln!("   xrefs inserted: {count}");
    conn.execute_batch("VACUUM; PRAGMA optimize;")?;
    Ok(())
}

fn populate_book(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let mut stmt =
        tx.prepare_cached("INSERT INTO book(code, testament, ord) VALUES (?1, ?2, ?3)")?;
    for (code, testament, ord) in BOOKS {
        stmt.execute(params![code, testament, ord])?;
    }
    Ok(())
}

fn populate_book_label(tx: &rusqlite::Transaction<'_>, code: &str) -> Result<()> {
    let table = labels::labels_for(code);
    let mut stmt = tx.prepare_cached(
        "INSERT INTO book_label(book, name, abbreviation, full_name) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (osis, _, _) in BOOKS {
        let (name, abbr) = labels::lookup(table, osis)
            .ok_or_else(|| anyhow::anyhow!("label table missing {osis} for {code}"))?;
        stmt.execute(params![osis, name, abbr, name])?;
    }
    Ok(())
}

fn populate_verse(
    tx: &rusqlite::Transaction<'_>,
    code: &str,
    parsed: &TranslationJson,
) -> Result<i64> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO verse(book, chapter, verse, osis_id, text) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut count: i64 = 0;
    let mut skipped_books: Vec<&str> = Vec::new();
    for book in &parsed.books {
        // Skip deuterocanonical / unknown books rather than failing — the
        // schema is Protestant-canon-only by design (matches xrefs and
        // bookmarks). DRC and the Vulgate carry Tobit/Judith/Wisdom/etc.;
        // dropping them gives us the 66-book subset.
        let Some(osis) = lookup_osis(&book.name) else {
            skipped_books.push(&book.name);
            continue;
        };
        for chapter in &book.chapters {
            for verse in &chapter.verses {
                let osis_id = format!("{osis}.{}.{}", chapter.chapter, verse.verse);
                let text = verse.text.trim();
                stmt.execute(params![osis, chapter.chapter, verse.verse, osis_id, text])?;
                count += 1;
            }
        }
    }
    if !skipped_books.is_empty() {
        eprintln!(
            "   {code}: skipped {} non-Protestant book(s): {}",
            skipped_books.len(),
            skipped_books.join(", "),
        );
    }
    Ok(count)
}

fn populate_meta(
    tx: &rusqlite::Transaction<'_>,
    entry: &TranslationEntry,
    scrollmapper_commit: &str,
    verse_count: i64,
) -> Result<()> {
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
            entry.code,
            entry.name,
            entry.language,
            entry.license,
            entry.attribution,
            scrollmapper_commit,
            built_at,
            verse_count,
            SCHEMA_VERSION,
        ],
    )?;
    Ok(())
}

/// `git -C <scrollmapper> rev-parse HEAD`. Recorded into per-translation
/// `meta.source_commit` so reproducibility doesn't depend on environment.
fn git_head_commit(scrollmapper: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(scrollmapper)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("invoke git on {}", scrollmapper.display()))?;
    if !output.status.success() {
        bail!(
            "git -C {} rev-parse HEAD failed: {}",
            scrollmapper.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)
        .context("git output not utf-8")?
        .trim()
        .to_string())
}

#[derive(Debug, Deserialize)]
struct TranslationJson {
    #[allow(dead_code)] // wrapping metadata — not consumed downstream
    translation: Option<String>,
    books: Vec<BookJson>,
}

#[derive(Debug, Deserialize)]
struct BookJson {
    name: String,
    chapters: Vec<ChapterJson>,
}

#[derive(Debug, Deserialize)]
struct ChapterJson {
    chapter: i64,
    verses: Vec<VerseJson>,
}

#[derive(Debug, Deserialize)]
struct VerseJson {
    verse: i64,
    text: String,
}

fn load_translation_json(path: &Path) -> Result<TranslationJson> {
    let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed: TranslationJson = serde_json::from_str(&body)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single most fragile parsing branch: `populate_verse` drops books
    /// `lookup_osis` doesn't recognise (deuterocanonical / a future scrollmapper
    /// rename). Pin it on a synthetic fixture so a regression that silently
    /// dropped a *canonical* book — shipping a truncated Bible — fails in CI
    /// without needing a scrollmapper checkout.
    #[test]
    fn populate_verse_skips_noncanonical_books_and_trims_text() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(crate::schema::TRANSLATION_SCHEMA_SQL)
            .expect("apply schema");
        let tx = conn.transaction().expect("begin tx");
        populate_book(&tx).expect("populate canonical books");

        // Genesis (canonical) + Tobit (deuterocanonical — must be dropped).
        let json = r#"{
            "translation": "test",
            "books": [
                { "name": "Genesis", "chapters": [
                    { "chapter": 1, "verses": [
                        { "verse": 1, "text": "In the beginning" },
                        { "verse": 2, "text": "  and the earth was without form  " }
                    ] }
                ] },
                { "name": "Tobit", "chapters": [
                    { "chapter": 1, "verses": [ { "verse": 1, "text": "deuterocanonical" } ] }
                ] }
            ]
        }"#;
        let parsed: TranslationJson = serde_json::from_str(json).expect("parse synthetic json");
        let count = populate_verse(&tx, "en-test", &parsed).expect("populate_verse");
        tx.commit().expect("commit");

        // Tobit is skipped; only Genesis 1:1–2 are counted and stored.
        assert_eq!(count, 2, "only the two canonical verses should count");
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM verse", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);
        // Nothing landed outside the canonical book set.
        let noncanon: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM verse WHERE book NOT IN (SELECT code FROM book)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(noncanon, 0, "no verse may reference a non-canonical book");
        // Verse text is trimmed before insert.
        let v2: String = conn
            .query_row(
                "SELECT text FROM verse WHERE book = 'GEN' AND chapter = 1 AND verse = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v2, "and the earth was without form");
    }
}
