//! End-to-end test for `turbo-bible import`.
//!
//! Marked `#[ignore]` because it depends on a populated scrollmapper
//! cache (`~/.cache/turbo-bible/scrollmapper/`) — running it against the
//! developer's machine avoids network in CI. Invoke with:
//!
//! ```sh
//! cargo test --test import -- --ignored
//! ```

use std::path::PathBuf;
use std::process::Command;

use rusqlite::Connection;
use tempfile::TempDir;

use turbo_bible::SCROLLMAPPER_COMMIT as PINNED_COMMIT;

const fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_turbo-bible")
}

fn dev_scrollmapper_cache() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".cache/turbo-bible/scrollmapper");
    let all_present = ["KJV.db", "Norsk.db", "SpaRV.db"]
        .iter()
        .all(|f| dir.join(format!("{PINNED_COMMIT}-{f}")).exists());
    all_present.then_some(dir)
}

#[test]
#[ignore = "requires ~/.cache/turbo-bible/scrollmapper/ populated; run with --ignored"]
#[allow(
    clippy::too_many_lines,
    reason = "one e2e assertion per DB invariant — keeping them in a single \
              test means one CLI invocation amortizes ~30s of import work."
)]
fn import_subcommand_builds_full_db() {
    let Some(cache) = dev_scrollmapper_cache() else {
        panic!(
            "scrollmapper cache missing for pinned commit {PINNED_COMMIT}; \
             populate ~/.cache/turbo-bible/scrollmapper/ first"
        );
    };

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("bible.sqlite");

    let out = Command::new(binary_path())
        .arg("import")
        .arg("--db")
        .arg(&db)
        .arg("--backup-dir")
        .arg(tmp.path().join("backups"))
        .arg("--cache-dir")
        .arg(&cache)
        .output()
        .expect("run turbo-bible import");
    assert!(
        out.status.success(),
        "import failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let conn = Connection::open(&db).expect("open imported DB");

    // Per-translation verse counts. All three editions OSIS-align to KJV,
    // so the counts match. Captured from a real import on 2026-05-21
    // against the pinned commit.
    let mut counts: Vec<(String, i64)> = conn
        .prepare(
            "SELECT translation, COUNT(*) FROM verse GROUP BY translation ORDER BY translation",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    counts.sort();
    assert_eq!(
        counts,
        vec![
            ("en-kjv".to_string(), 31_102),
            ("es-rv1909".to_string(), 31_102),
            ("nb-1930".to_string(), 31_102),
        ]
    );

    let books: i64 = conn
        .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
        .unwrap();
    assert_eq!(books, 66);

    let labels: i64 = conn
        .query_row("SELECT COUNT(*) FROM book_label", [], |r| r.get(0))
        .unwrap();
    assert_eq!(labels, 66 * 3);

    let fts_version: String = conn
        .query_row("SELECT value FROM meta WHERE key='fts_version'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fts_version, "2");

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM verse_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fts_count, 31_102 * 3);

    // Spot-check: the goto-by-name path needs distinct names per
    // translation, so the localized labels must actually land.
    let jhn_nb: String = conn
        .query_row(
            "SELECT name FROM book_label WHERE translation='nb-1930' AND book='JHN'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(jhn_nb, "Johannes");

    // Cross-references: captured from a real import on 2026-05-22 against
    // the pinned commit. Raw scrollmapper rows are symmetric pairs, so the
    // PK-dedupe halves the row count. Pinning the exact number guards
    // against (a) the upstream changing under the same SHA and (b) a
    // future code change accidentally re-introducing duplicates.
    let xref_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM xref", [], |r| r.get(0))
        .unwrap();
    assert_eq!(xref_count, 432_949);

    let xref_books: i64 = conn
        .query_row("SELECT COUNT(DISTINCT from_book) FROM xref", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(xref_books, 66);

    // John 3:16 is a high-density xref source — used in the rust-review
    // bring-up. If this drops to zero, the openbible name normalization
    // (Arabic vs Roman numerals, "Revelation" vs "Revelation of John")
    // has regressed.
    let jhn_3_16: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM xref \
             WHERE from_book='JHN' AND from_chapter=3 AND from_verse=16",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        jhn_3_16 >= 20,
        "JHN 3:16 has only {jhn_3_16} xrefs; expected >= 20"
    );

    // The top xref for JHN 3:16 is Romans 5:8 in the openbible data
    // (vote count 871 in this commit). Pinning it makes a "the votes
    // ordering broke" regression noisy.
    let top_target_book: String = conn
        .query_row(
            "SELECT to_book FROM xref \
             WHERE from_book='JHN' AND from_chapter=3 AND from_verse=16 \
             ORDER BY votes DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(top_target_book, "ROM");
}

#[test]
#[ignore = "requires ~/.cache/turbo-bible/scrollmapper/ populated; run with --ignored"]
fn import_only_filters_translations() {
    let Some(cache) = dev_scrollmapper_cache() else {
        panic!("scrollmapper cache missing");
    };
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("bible.sqlite");

    let out = Command::new(binary_path())
        .arg("import")
        .arg("--db")
        .arg(&db)
        .arg("--backup-dir")
        .arg(tmp.path().join("backups"))
        .arg("--cache-dir")
        .arg(&cache)
        .arg("--only")
        .arg("en-kjv")
        .output()
        .expect("run import --only");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let conn = Connection::open(&db).unwrap();
    let translations: Vec<String> = conn
        .prepare("SELECT code FROM translation ORDER BY code")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(translations, vec!["en-kjv".to_string()]);
}
