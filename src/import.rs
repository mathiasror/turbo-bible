//! `turbo-bible import` — populate `bible.sqlite` from the pinned
//! scrollmapper edition.
//!
//! Ported from `scripts/import_translations.py` (see the git history
//! of the sibling sandbox repo). Slice A imported KJV only; this Rust
//! port handles all three translations in one transaction. The
//! `meta.fts_version` stamp is written by `db::ensure_fts_optimized`
//! so that the TUI's first launch is fast.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, params};

use crate::db;
use crate::paths;

/// Pinned upstream commit. Bump deliberately; verify the SHA matches a
/// real commit on `scrollmapper/bible_databases` before changing.
const SCROLLMAPPER_COMMIT: &str = "a228a19a29099a41c196c2a310cd93e50a390e30";

const SCROLLMAPPER_URL_BASE: &str =
    "https://raw.githubusercontent.com/scrollmapper/bible_databases";

/// Number of `cross_references_{n}.db` shards in `formats/sqlite/extras/`.
/// Scrollmapper splits the openbible.info xref dump alphabetically by source
/// book across 7 files; we need all of them for full per-verse coverage.
const XREF_SHARDS: usize = 7;

struct Source {
    code: &'static str,
    file: &'static str,
    name: &'static str,
    license: &'static str,
    language: &'static str,
}

/// We ignore the upstream `translations.license` field because it's
/// inconsistent across editions (KJV is labelled "GPL" upstream; the
/// 1769 text is public domain).
const SOURCES: &[Source] = &[
    Source {
        code: "en-kjv",
        file: "KJV.db",
        name: "King James Version (1769)",
        license: "Public Domain",
        language: "en",
    },
    Source {
        code: "nb-1930",
        file: "Norsk.db",
        name: "Bibelen 1930 (Bokmål)",
        license: "Public Domain",
        language: "nb",
    },
    Source {
        code: "es-rv1909",
        file: "SpaRV.db",
        name: "Reina-Valera 1909",
        license: "Public Domain",
        language: "es",
    },
];

/// Canonical Protestant 66-book metadata. Lifted from the upstream
/// `crawl.py` to stay license-clean (no scrollmapper book table).
#[rustfmt::skip]
const BOOKS: &[(&str, &str, i64)] = &[
    ("GEN", "OT", 1), ("EXO", "OT", 2), ("LEV", "OT", 3), ("NUM", "OT", 4),
    ("DEU", "OT", 5), ("JOS", "OT", 6), ("JDG", "OT", 7), ("RUT", "OT", 8),
    ("1SA", "OT", 9), ("2SA", "OT", 10), ("1KI", "OT", 11), ("2KI", "OT", 12),
    ("1CH", "OT", 13), ("2CH", "OT", 14), ("EZR", "OT", 15), ("NEH", "OT", 16),
    ("EST", "OT", 17), ("JOB", "OT", 18), ("PSA", "OT", 19), ("PRO", "OT", 20),
    ("ECC", "OT", 21), ("SNG", "OT", 22), ("ISA", "OT", 23), ("JER", "OT", 24),
    ("LAM", "OT", 25), ("EZK", "OT", 26), ("DAN", "OT", 27), ("HOS", "OT", 28),
    ("JOL", "OT", 29), ("AMO", "OT", 30), ("OBA", "OT", 31), ("JON", "OT", 32),
    ("MIC", "OT", 33), ("NAM", "OT", 34), ("HAB", "OT", 35), ("ZEP", "OT", 36),
    ("HAG", "OT", 37), ("ZEC", "OT", 38), ("MAL", "OT", 39),
    ("MAT", "NT", 40), ("MRK", "NT", 41), ("LUK", "NT", 42), ("JHN", "NT", 43),
    ("ACT", "NT", 44), ("ROM", "NT", 45), ("1CO", "NT", 46), ("2CO", "NT", 47),
    ("GAL", "NT", 48), ("EPH", "NT", 49), ("PHP", "NT", 50), ("COL", "NT", 51),
    ("1TH", "NT", 52), ("2TH", "NT", 53), ("1TI", "NT", 54), ("2TI", "NT", 55),
    ("TIT", "NT", 56), ("PHM", "NT", 57), ("HEB", "NT", 58), ("JAS", "NT", 59),
    ("1PE", "NT", 60), ("2PE", "NT", 61), ("1JN", "NT", 62), ("2JN", "NT", 63),
    ("3JN", "NT", 64), ("JUD", "NT", 65), ("REV", "NT", 66),
];

/// Map scrollmapper's English book names → OSIS. Bail loudly on any
/// unknown name so we never silently accept a non-Protestant canon.
#[rustfmt::skip]
const SCROLLMAPPER_NAME_TO_OSIS: &[(&str, &str)] = &[
    ("Genesis", "GEN"), ("Exodus", "EXO"), ("Leviticus", "LEV"), ("Numbers", "NUM"),
    ("Deuteronomy", "DEU"), ("Joshua", "JOS"), ("Judges", "JDG"), ("Ruth", "RUT"),
    ("I Samuel", "1SA"), ("II Samuel", "2SA"), ("I Kings", "1KI"), ("II Kings", "2KI"),
    ("I Chronicles", "1CH"), ("II Chronicles", "2CH"), ("Ezra", "EZR"),
    ("Nehemiah", "NEH"), ("Esther", "EST"), ("Job", "JOB"), ("Psalms", "PSA"),
    ("Proverbs", "PRO"), ("Ecclesiastes", "ECC"), ("Song of Solomon", "SNG"),
    ("Isaiah", "ISA"), ("Jeremiah", "JER"), ("Lamentations", "LAM"), ("Ezekiel", "EZK"),
    ("Daniel", "DAN"), ("Hosea", "HOS"), ("Joel", "JOL"), ("Amos", "AMO"),
    ("Obadiah", "OBA"), ("Jonah", "JON"), ("Micah", "MIC"), ("Nahum", "NAM"),
    ("Habakkuk", "HAB"), ("Zephaniah", "ZEP"), ("Haggai", "HAG"), ("Zechariah", "ZEC"),
    ("Malachi", "MAL"), ("Matthew", "MAT"), ("Mark", "MRK"), ("Luke", "LUK"),
    ("John", "JHN"), ("Acts", "ACT"), ("Romans", "ROM"), ("I Corinthians", "1CO"),
    ("II Corinthians", "2CO"), ("Galatians", "GAL"), ("Ephesians", "EPH"),
    ("Philippians", "PHP"), ("Colossians", "COL"), ("I Thessalonians", "1TH"),
    ("II Thessalonians", "2TH"), ("I Timothy", "1TI"), ("II Timothy", "2TI"),
    ("Titus", "TIT"), ("Philemon", "PHM"), ("Hebrews", "HEB"), ("James", "JAS"),
    ("I Peter", "1PE"), ("II Peter", "2PE"), ("I John", "1JN"), ("II John", "2JN"),
    ("III John", "3JN"), ("Jude", "JUD"), ("Revelation of John", "REV"),
];

/// (osis, name, abbreviation)
#[rustfmt::skip]
const KJV_LABELS: &[(&str, &str, &str)] = &[
    ("GEN", "Genesis", "Gen"), ("EXO", "Exodus", "Exo"),
    ("LEV", "Leviticus", "Lev"), ("NUM", "Numbers", "Num"),
    ("DEU", "Deuteronomy", "Deut"), ("JOS", "Joshua", "Josh"),
    ("JDG", "Judges", "Judg"), ("RUT", "Ruth", "Ruth"),
    ("1SA", "1 Samuel", "1 Sam"), ("2SA", "2 Samuel", "2 Sam"),
    ("1KI", "1 Kings", "1 Kgs"), ("2KI", "2 Kings", "2 Kgs"),
    ("1CH", "1 Chronicles", "1 Chr"), ("2CH", "2 Chronicles", "2 Chr"),
    ("EZR", "Ezra", "Ezra"), ("NEH", "Nehemiah", "Neh"),
    ("EST", "Esther", "Esth"), ("JOB", "Job", "Job"),
    ("PSA", "Psalms", "Ps"), ("PRO", "Proverbs", "Prov"),
    ("ECC", "Ecclesiastes", "Eccl"), ("SNG", "Song of Solomon", "Song"),
    ("ISA", "Isaiah", "Isa"), ("JER", "Jeremiah", "Jer"),
    ("LAM", "Lamentations", "Lam"), ("EZK", "Ezekiel", "Ezek"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Hosea", "Hos"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Amos"),
    ("OBA", "Obadiah", "Obad"), ("JON", "Jonah", "Jonah"),
    ("MIC", "Micah", "Mic"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habakkuk", "Hab"), ("ZEP", "Zephaniah", "Zeph"),
    ("HAG", "Haggai", "Hag"), ("ZEC", "Zechariah", "Zech"),
    ("MAL", "Malachi", "Mal"), ("MAT", "Matthew", "Matt"),
    ("MRK", "Mark", "Mark"), ("LUK", "Luke", "Luke"),
    ("JHN", "John", "John"), ("ACT", "Acts", "Acts"),
    ("ROM", "Romans", "Rom"), ("1CO", "1 Corinthians", "1 Cor"),
    ("2CO", "2 Corinthians", "2 Cor"), ("GAL", "Galatians", "Gal"),
    ("EPH", "Ephesians", "Eph"), ("PHP", "Philippians", "Phil"),
    ("COL", "Colossians", "Col"), ("1TH", "1 Thessalonians", "1 Thess"),
    ("2TH", "2 Thessalonians", "2 Thess"), ("1TI", "1 Timothy", "1 Tim"),
    ("2TI", "2 Timothy", "2 Tim"), ("TIT", "Titus", "Titus"),
    ("PHM", "Philemon", "Phlm"), ("HEB", "Hebrews", "Heb"),
    ("JAS", "James", "Jas"), ("1PE", "1 Peter", "1 Pet"),
    ("2PE", "2 Peter", "2 Pet"), ("1JN", "1 John", "1 John"),
    ("2JN", "2 John", "2 John"), ("3JN", "3 John", "3 John"),
    ("JUD", "Jude", "Jude"), ("REV", "Revelation", "Rev"),
];

#[rustfmt::skip]
const NB_1930_LABELS: &[(&str, &str, &str)] = &[
    ("GEN", "Første Mosebok", "1 Mos"), ("EXO", "Andre Mosebok", "2 Mos"),
    ("LEV", "Tredje Mosebok", "3 Mos"), ("NUM", "Fjerde Mosebok", "4 Mos"),
    ("DEU", "Femte Mosebok", "5 Mos"), ("JOS", "Josva", "Jos"),
    ("JDG", "Dommerne", "Dom"), ("RUT", "Rut", "Rut"),
    ("1SA", "Første Samuelsbok", "1 Sam"), ("2SA", "Andre Samuelsbok", "2 Sam"),
    ("1KI", "Første Kongebok", "1 Kong"), ("2KI", "Andre Kongebok", "2 Kong"),
    ("1CH", "Første Krønikebok", "1 Krøn"), ("2CH", "Andre Krønikebok", "2 Krøn"),
    ("EZR", "Esra", "Esra"), ("NEH", "Nehemja", "Neh"),
    ("EST", "Ester", "Est"), ("JOB", "Job", "Job"),
    ("PSA", "Salmene", "Sal"), ("PRO", "Ordspråkene", "Ordsp"),
    ("ECC", "Forkynneren", "Fork"), ("SNG", "Høysangen", "Høys"),
    ("ISA", "Jesaja", "Jes"), ("JER", "Jeremia", "Jer"),
    ("LAM", "Klagesangene", "Klag"), ("EZK", "Esekiel", "Esek"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Hosea", "Hos"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Am"),
    ("OBA", "Obadja", "Obad"), ("JON", "Jona", "Jona"),
    ("MIC", "Mika", "Mi"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habakkuk", "Hab"), ("ZEP", "Sefanja", "Sef"),
    ("HAG", "Haggai", "Hag"), ("ZEC", "Sakarja", "Sak"),
    ("MAL", "Malaki", "Mal"), ("MAT", "Matteus", "Matt"),
    ("MRK", "Markus", "Mark"), ("LUK", "Lukas", "Luk"),
    ("JHN", "Johannes", "Joh"), ("ACT", "Apostlenes gjerninger", "Apg"),
    ("ROM", "Romerne", "Rom"), ("1CO", "Første Korinterbrev", "1 Kor"),
    ("2CO", "Andre Korinterbrev", "2 Kor"), ("GAL", "Galaterne", "Gal"),
    ("EPH", "Efeserne", "Ef"), ("PHP", "Filipperne", "Fil"),
    ("COL", "Kolosserne", "Kol"), ("1TH", "Første Tessalonikerbrev", "1 Tess"),
    ("2TH", "Andre Tessalonikerbrev", "2 Tess"), ("1TI", "Første Timoteusbrev", "1 Tim"),
    ("2TI", "Andre Timoteusbrev", "2 Tim"), ("TIT", "Titus", "Tit"),
    ("PHM", "Filemon", "Filem"), ("HEB", "Hebreerne", "Hebr"),
    ("JAS", "Jakob", "Jak"), ("1PE", "Første Petersbrev", "1 Pet"),
    ("2PE", "Andre Petersbrev", "2 Pet"), ("1JN", "Første Johannesbrev", "1 Joh"),
    ("2JN", "Andre Johannesbrev", "2 Joh"), ("3JN", "Tredje Johannesbrev", "3 Joh"),
    ("JUD", "Judas", "Jud"), ("REV", "Johannes' åpenbaring", "Åp"),
];

#[rustfmt::skip]
const ES_RV1909_LABELS: &[(&str, &str, &str)] = &[
    ("GEN", "Génesis", "Gn"), ("EXO", "Éxodo", "Ex"),
    ("LEV", "Levítico", "Lv"), ("NUM", "Números", "Nm"),
    ("DEU", "Deuteronomio", "Dt"), ("JOS", "Josué", "Jos"),
    ("JDG", "Jueces", "Jue"), ("RUT", "Rut", "Rt"),
    ("1SA", "1 Samuel", "1 S"), ("2SA", "2 Samuel", "2 S"),
    ("1KI", "1 Reyes", "1 R"), ("2KI", "2 Reyes", "2 R"),
    ("1CH", "1 Crónicas", "1 Cr"), ("2CH", "2 Crónicas", "2 Cr"),
    ("EZR", "Esdras", "Esd"), ("NEH", "Nehemías", "Neh"),
    ("EST", "Ester", "Est"), ("JOB", "Job", "Job"),
    ("PSA", "Salmos", "Sal"), ("PRO", "Proverbios", "Pr"),
    ("ECC", "Eclesiastés", "Ec"), ("SNG", "Cantares", "Cnt"),
    ("ISA", "Isaías", "Is"), ("JER", "Jeremías", "Jer"),
    ("LAM", "Lamentaciones", "Lm"), ("EZK", "Ezequiel", "Ez"),
    ("DAN", "Daniel", "Dn"), ("HOS", "Oseas", "Os"),
    ("JOL", "Joel", "Jl"), ("AMO", "Amós", "Am"),
    ("OBA", "Abdías", "Abd"), ("JON", "Jonás", "Jon"),
    ("MIC", "Miqueas", "Mi"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habacuc", "Hab"), ("ZEP", "Sofonías", "Sof"),
    ("HAG", "Hageo", "Hag"), ("ZEC", "Zacarías", "Zac"),
    ("MAL", "Malaquías", "Mal"), ("MAT", "Mateo", "Mt"),
    ("MRK", "Marcos", "Mr"), ("LUK", "Lucas", "Lc"),
    ("JHN", "Juan", "Jn"), ("ACT", "Hechos", "Hch"),
    ("ROM", "Romanos", "Ro"), ("1CO", "1 Corintios", "1 Co"),
    ("2CO", "2 Corintios", "2 Co"), ("GAL", "Gálatas", "Gá"),
    ("EPH", "Efesios", "Ef"), ("PHP", "Filipenses", "Fil"),
    ("COL", "Colosenses", "Col"), ("1TH", "1 Tesalonicenses", "1 Ts"),
    ("2TH", "2 Tesalonicenses", "2 Ts"), ("1TI", "1 Timoteo", "1 Ti"),
    ("2TI", "2 Timoteo", "2 Ti"), ("TIT", "Tito", "Tit"),
    ("PHM", "Filemón", "Flm"), ("HEB", "Hebreos", "He"),
    ("JAS", "Santiago", "Stg"), ("1PE", "1 Pedro", "1 P"),
    ("2PE", "2 Pedro", "2 P"), ("1JN", "1 Juan", "1 Jn"),
    ("2JN", "2 Juan", "2 Jn"), ("3JN", "3 Juan", "3 Jn"),
    ("JUD", "Judas", "Jud"), ("REV", "Apocalipsis", "Ap"),
];

const SCHEMA_SQL: &str = "
CREATE TABLE translation (
    code     TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    language TEXT NOT NULL,
    license  TEXT NOT NULL
);

CREATE TABLE book (
    code      TEXT PRIMARY KEY,
    testament TEXT NOT NULL CHECK (testament IN ('OT','NT')),
    ord       INTEGER NOT NULL UNIQUE
);

CREATE TABLE book_label (
    translation  TEXT NOT NULL REFERENCES translation(code),
    book         TEXT NOT NULL REFERENCES book(code),
    name         TEXT NOT NULL,
    abbreviation TEXT NOT NULL,
    full_name    TEXT,
    PRIMARY KEY (translation, book)
);

CREATE TABLE verse (
    translation TEXT NOT NULL REFERENCES translation(code),
    book        TEXT NOT NULL REFERENCES book(code),
    chapter     INTEGER NOT NULL,
    verse       INTEGER NOT NULL,
    osis_id     TEXT NOT NULL,
    text        TEXT NOT NULL,
    PRIMARY KEY (translation, book, chapter, verse)
);
CREATE INDEX verse_osis_idx ON verse(translation, osis_id);

CREATE TABLE heading (
    translation  TEXT NOT NULL,
    book         TEXT NOT NULL,
    chapter      INTEGER NOT NULL,
    before_verse INTEGER NOT NULL,
    style        TEXT NOT NULL,
    text         TEXT NOT NULL
);
CREATE INDEX heading_loc_idx
    ON heading(translation, book, chapter, before_verse);

CREATE TABLE footnote (
    translation TEXT NOT NULL REFERENCES translation(code),
    id          TEXT NOT NULL,
    verse_osis  TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('f','x')),
    body        TEXT NOT NULL,
    PRIMARY KEY (translation, id)
);
CREATE INDEX footnote_verse_idx
    ON footnote(translation, verse_osis);

-- Cross-references are translation-independent: they live entirely in
-- terms of OSIS book codes and (book, chapter, verse) coordinates, sourced
-- from scrollmapper's openbible.info extras. `votes` is the openbible
-- crowd-sourced strength score (higher = more authoritative); the UI
-- truncates to the top N per source verse.
CREATE TABLE xref (
    from_book       TEXT NOT NULL REFERENCES book(code),
    from_chapter    INTEGER NOT NULL,
    from_verse      INTEGER NOT NULL,
    to_book         TEXT NOT NULL REFERENCES book(code),
    to_chapter      INTEGER NOT NULL,
    to_verse_start  INTEGER NOT NULL,
    to_verse_end    INTEGER NOT NULL,
    votes           INTEGER NOT NULL,
    PRIMARY KEY (from_book, from_chapter, from_verse,
                 to_book, to_chapter, to_verse_start, to_verse_end)
);
CREATE INDEX xref_from_idx
    ON xref(from_book, from_chapter, from_verse, votes DESC);

CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// Path to the `SQLite` DB (default: `$XDG_DATA_HOME/turbo-bible/bible.sqlite`).
    #[arg(long)]
    pub db: Option<PathBuf>,
    /// Comma-separated list of translation codes to import.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
    /// Directory for DB backups (default: $XDG_DATA_HOME/turbo-bible/backups).
    #[arg(long)]
    pub backup_dir: Option<PathBuf>,
    /// Directory for cached scrollmapper downloads
    /// (default: $XDG_CACHE_HOME/turbo-bible/scrollmapper).
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    /// Skip backing up the existing DB before wiping.
    #[arg(long)]
    pub no_backup: bool,
    /// Dump current DB to the backup dir and exit.
    #[arg(long)]
    pub backup_only: bool,
}

/// Entry point dispatched from `main` when `Commands::Import` is parsed.
///
/// # Errors
/// Propagates network, filesystem, and `SQLite` failures. On any error,
/// the partially-built DB is left as-is (the caller should re-run the
/// importer rather than launching the TUI against half-imported data).
pub fn run(args: &ImportArgs) -> Result<()> {
    let db_path = match &args.db {
        Some(p) => p.clone(),
        None => paths::data_dir()?.join("bible.sqlite"),
    };
    // When `--db /custom/path` is passed without `--backup-dir`, default
    // backups to the same parent as the DB. Two flags either share a root
    // or one is explicitly anchored to the other; the previous behavior
    // (backups always at $XDG_DATA_HOME/turbo-bible/backups/) silently
    // ignored `--db` and surfaced misleading "permission denied" errors
    // on read-only/ephemeral mounts.
    let backup_dir = match (&args.backup_dir, &args.db) {
        (Some(p), _) => p.clone(),
        (None, Some(db)) => db
            .parent()
            .map(|p| p.join("backups"))
            .ok_or_else(|| anyhow!("--db {} has no parent directory", db.display()))?,
        (None, None) => paths::data_dir()?.join("backups"),
    };
    let cache_dir = match &args.cache_dir {
        Some(p) => p.clone(),
        None => paths::cache_dir()?.join("scrollmapper"),
    };

    if args.backup_only {
        backup_existing(&db_path, &backup_dir)?;
        return Ok(());
    }
    if !args.no_backup {
        backup_existing(&db_path, &backup_dir)?;
    }

    let selected: Vec<&Source> = if args.only.is_empty() {
        SOURCES.iter().collect()
    } else {
        let want: std::collections::HashSet<&str> = args.only.iter().map(String::as_str).collect();
        SOURCES.iter().filter(|s| want.contains(s.code)).collect()
    };
    if selected.is_empty() {
        bail!("no matching translations in --only={:?}", args.only);
    }

    let mut src_paths = Vec::with_capacity(selected.len());
    for source in &selected {
        let p = download_source(source.file, &cache_dir, "formats/sqlite")
            .with_context(|| format!("download {}", source.file))?;
        src_paths.push(p);
    }

    // The xref shards are global (translation-independent) so they're
    // downloaded once regardless of `--only`. The 7 shards split source
    // books alphabetically; we need all of them for full per-verse
    // coverage.
    let mut xref_paths: Vec<PathBuf> = Vec::with_capacity(XREF_SHARDS);
    for i in 0..XREF_SHARDS {
        let file = format!("cross_references_{i}.db");
        let p = download_source(&file, &cache_dir, "formats/sqlite/extras")
            .with_context(|| format!("download {file}"))?;
        xref_paths.push(p);
    }

    recreate_schema(&db_path)?;

    {
        let mut conn = Connection::open(&db_path)?;
        // foreign_keys + temp_store are per-connection. journal_mode and
        // synchronous persist with the DB file. Set all four up front so
        // the TUI inherits WAL on next open.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; \
             PRAGMA journal_mode = WAL; \
             PRAGMA synchronous = NORMAL; \
             PRAGMA temp_store = MEMORY;",
        )?;

        let tx = conn.transaction()?;
        for (source, src_path) in selected.iter().zip(&src_paths) {
            let n = import_translation(&tx, source, src_path)
                .with_context(|| format!("import {}", source.code))?;
            println!("imported {}: {n} verses", source.code);
        }
        let xref_count = import_cross_refs(&tx, &xref_paths).context("import cross-references")?;
        println!("imported {xref_count} cross-references");
        tx.commit()?;
    } // conn drops here, releasing the file lock

    // FTS rebuild opens its own Connection — must run after the import
    // conn is dropped, or we hit SQLITE_BUSY in WAL mode.
    if db::ensure_fts_optimized(&db_path)? {
        println!("FTS index built");
    }
    Ok(())
}

fn download_source(file: &str, cache_dir: &Path, url_subdir: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)?;
    let cached = cache_dir.join(format!("{SCROLLMAPPER_COMMIT}-{file}"));
    if cached.exists() && std::fs::metadata(&cached)?.len() > 0 {
        // Belt-and-braces: a previous run might have died after `persist`
        // but before downstream consumers validated the file; or the disk
        // might have flipped a bit since. `quick_check` is fast and catches
        // truncation + most corruption without us needing to know the
        // file's schema (translations vs xref shards). On failure, drop
        // the cached file and re-download.
        if probe_sqlite_ok(&cached).is_ok() {
            return Ok(cached);
        }
        eprintln!(
            "cache: {} fails quick_check; re-downloading",
            cached.display()
        );
        let _ = std::fs::remove_file(&cached);
    }
    let url = format!("{SCROLLMAPPER_URL_BASE}/{SCROLLMAPPER_COMMIT}/{url_subdir}/{file}");
    println!("download {url}");

    let mut tmp = tempfile::NamedTempFile::new_in(cache_dir)?;
    let response = ureq::get(&url).call().context("HTTP GET")?;
    let mut reader = response.into_body().into_reader();
    io::copy(&mut reader, tmp.as_file_mut())?;

    tmp.persist(&cached)
        .map_err(|e| anyhow!("persist cached download: {e}"))?;
    probe_sqlite_ok(&cached).with_context(|| {
        format!(
            "downloaded {} but it failed SQLite quick_check — \
             likely a partial download or upstream corruption; \
             delete the file under {cache_dir:?} and retry",
            cached.display(),
        )
    })?;
    Ok(cached)
}

/// Open the file read-only and run `PRAGMA quick_check`. Returns `Ok(())`
/// when the DB reports `"ok"`, otherwise an error describing the
/// integrity-check failure. Doesn't care about schema — works for both
/// translation files and xref shards.
fn probe_sqlite_ok(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {}", path.display()))?;
    let status: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .with_context(|| format!("quick_check on {}", path.display()))?;
    if status == "ok" {
        Ok(())
    } else {
        Err(anyhow!("{}: quick_check returned {status}", path.display()))
    }
}

fn recreate_schema(db_path: &Path) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Remove the DB and any WAL/SHM sidecars so a fresh import doesn't
    // inherit stale WAL pages from the prior DB.
    for sidecar in [
        db_path.to_path_buf(),
        append_suffix(db_path, "-wal"),
        append_suffix(db_path, "-shm"),
        append_suffix(db_path, "-journal"),
    ] {
        match std::fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("remove {}", sidecar.display())),
        }
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let mut stmt = conn.prepare("INSERT INTO book(code, testament, ord) VALUES (?1, ?2, ?3)")?;
    for (code, testament, ord) in BOOKS {
        stmt.execute(params![code, testament, ord])?;
    }
    Ok(())
}

fn import_translation(
    tx: &rusqlite::Transaction<'_>,
    source: &Source,
    src_path: &Path,
) -> Result<u64> {
    let table_prefix = source
        .file
        .strip_suffix(".db")
        .ok_or_else(|| anyhow!("source file {} does not end with .db", source.file))?;

    tx.execute(
        "INSERT INTO translation(code, name, language, license) VALUES (?1, ?2, ?3, ?4)",
        params![source.code, source.name, source.language, source.license],
    )?;

    let src = Connection::open_with_flags(src_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let books_sql = format!("SELECT id, name FROM {table_prefix}_books ORDER BY id");
    let books: Vec<(i64, String)> = {
        let mut stmt = src.prepare(&books_sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if books.len() != 66 {
        bail!("{}: expected 66 books, got {}", source.code, books.len());
    }

    let mut id_to_osis: HashMap<i64, &'static str> = HashMap::with_capacity(66);
    for (id, name) in &books {
        let osis = lookup_osis(name).ok_or_else(|| {
            anyhow!(
                "{}: unknown scrollmapper book name {name:?} — \
                 refusing to import (likely non-Protestant canon)",
                source.code
            )
        })?;
        id_to_osis.insert(*id, osis);
    }

    let labels = labels_for(source.code)
        .ok_or_else(|| anyhow!("no label table for translation {}", source.code))?;
    for osis in id_to_osis.values() {
        let (name, abbrev) = labels_lookup(labels, osis)
            .ok_or_else(|| anyhow!("{}: no label for {osis}", source.code))?;
        tx.execute(
            "INSERT INTO book_label(translation, book, name, abbreviation, full_name) \
             VALUES (?1, ?2, ?3, ?4, NULL)",
            params![source.code, osis, name, abbrev],
        )?;
    }

    let verses_sql = format!(
        "SELECT book_id, chapter, verse, text FROM {table_prefix}_verses \
         ORDER BY book_id, chapter, verse"
    );
    let mut verse_stmt = src.prepare(&verses_sql)?;
    let mut rows = verse_stmt.query([])?;

    let mut insert = tx.prepare_cached(
        "INSERT INTO verse(translation, book, chapter, verse, osis_id, text) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    let mut count: u64 = 0;
    while let Some(row) = rows.next()? {
        let book_id: i64 = row.get(0)?;
        let chapter: i64 = row.get(1)?;
        let verse_num: i64 = row.get(2)?;
        let text: String = row.get(3)?;
        let book = *id_to_osis
            .get(&book_id)
            .ok_or_else(|| anyhow!("{}: unknown source book_id {book_id}", source.code))?;
        let osis_id = format!("{book}.{chapter}.{verse_num}");
        insert.execute(params![
            source.code,
            book,
            chapter,
            verse_num,
            osis_id,
            text
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Scrollmapper's xref dataset spells numbered book names with Arabic
/// numerals (`1 John`, `2 Corinthians`) and the Apocalypse as plain
/// `Revelation`; the per-translation files use Roman numerals (`I John`)
/// and `Revelation of John`. This variant table covers the deltas so the
/// xref importer can reach OSIS codes without allocating a String per
/// row. Looked up *before* falling back to the main name map.
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

fn import_cross_refs(tx: &rusqlite::Transaction<'_>, shard_paths: &[PathBuf]) -> Result<u64> {
    let mut insert = tx.prepare_cached(
        "INSERT OR IGNORE INTO xref
           (from_book, from_chapter, from_verse,
            to_book, to_chapter, to_verse_start, to_verse_end, votes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut count: u64 = 0;
    for shard in shard_paths {
        let src = Connection::open_with_flags(shard, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut stmt = src.prepare(
            "SELECT from_book, from_chapter, from_verse,
                    to_book, to_chapter, to_verse_start, to_verse_end, votes
             FROM cross_references",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let from_name: String = row.get(0)?;
            let from_chapter: i64 = row.get(1)?;
            let from_verse: i64 = row.get(2)?;
            let to_name: String = row.get(3)?;
            let to_chapter: i64 = row.get(4)?;
            let to_verse_start: i64 = row.get(5)?;
            let to_verse_end: i64 = row.get(6)?;
            let votes: i64 = row.get(7)?;
            // Skip rows whose book names we don't recognize. The Protestant
            // 66 covers everything in the shards we sampled, but defending
            // here means a future scrollmapper bump that introduces
            // deuterocanonical entries downgrades silently instead of
            // corrupting the FK.
            let (Some(from), Some(to)) = (lookup_osis_xref(&from_name), lookup_osis_xref(&to_name))
            else {
                continue;
            };
            let rows = insert.execute(params![
                from,
                from_chapter,
                from_verse,
                to,
                to_chapter,
                to_verse_start,
                to_verse_end,
                votes,
            ])?;
            count += rows as u64;
        }
    }
    Ok(count)
}

fn backup_existing(db_path: &Path, backup_dir: &Path) -> Result<()> {
    if !db_path.exists() {
        eprintln!("no existing DB at {}; skipping backup", db_path.display());
        return Ok(());
    }
    std::fs::create_dir_all(backup_dir)?;
    let target = backup_dir.join(format!("bible-{}.sqlite", today_iso()));

    // Backup::new + run_to_completion handles WAL sidecars correctly;
    // a plain fs::copy would miss uncheckpointed pages.
    let src = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut dst = Connection::open(&target)?;
    let backup = Backup::new(&src, &mut dst)?;
    backup.run_to_completion(1000, Duration::from_millis(0), None)?;
    println!("backup → {}", target.display());
    Ok(())
}

fn lookup_osis(name: &str) -> Option<&'static str> {
    SCROLLMAPPER_NAME_TO_OSIS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, o)| *o)
}

fn labels_for(code: &str) -> Option<&'static [(&'static str, &'static str, &'static str)]> {
    match code {
        "en-kjv" => Some(KJV_LABELS),
        "nb-1930" => Some(NB_1930_LABELS),
        "es-rv1909" => Some(ES_RV1909_LABELS),
        _ => None,
    }
}

fn labels_lookup(
    labels: &'static [(&'static str, &'static str, &'static str)],
    osis: &str,
) -> Option<(&'static str, &'static str)> {
    labels
        .iter()
        .find(|(o, _, _)| *o == osis)
        .map(|(_, n, a)| (*n, *a))
}

fn append_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s: OsString = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// `YYYY-MM-DD` in UTC. Howard Hinnant's days-from-epoch → civil-date
/// algorithm (public domain). Used only for backup filenames, so UTC
/// vs local doesn't matter. The `cast_*` casts encode the algorithm's
/// inherent sign juggling — `days` is signed (days BEFORE 1970 are
/// negative); `doe` and `yoe` are u64 because the modular arithmetic
/// keeps them non-negative in the valid range.
fn today_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400).cast_signed();
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097).cast_unsigned();
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe.cast_signed() + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn osis_codes() -> HashSet<&'static str> {
        BOOKS.iter().map(|(c, _, _)| *c).collect()
    }

    #[test]
    fn books_has_66_entries() {
        assert_eq!(BOOKS.len(), 66);
    }

    #[test]
    fn name_to_osis_has_66_entries_and_maps_to_known_codes() {
        assert_eq!(SCROLLMAPPER_NAME_TO_OSIS.len(), 66);
        let codes = osis_codes();
        for (name, osis) in SCROLLMAPPER_NAME_TO_OSIS {
            assert!(
                codes.contains(osis),
                "unknown OSIS code {osis} (from {name})"
            );
        }
    }

    #[test]
    fn label_maps_cover_every_book() {
        let codes = osis_codes();
        for (label, name) in [
            (KJV_LABELS, "KJV"),
            (NB_1930_LABELS, "NB_1930"),
            (ES_RV1909_LABELS, "ES_RV1909"),
        ] {
            assert_eq!(label.len(), 66, "{name}: expected 66 entries");
            let label_codes: HashSet<&str> = label.iter().map(|(o, _, _)| *o).collect();
            assert_eq!(label_codes, codes, "{name}: OSIS set mismatch");
        }
    }

    #[test]
    fn labels_for_handles_known_codes() {
        assert!(labels_for("en-kjv").is_some());
        assert!(labels_for("nb-1930").is_some());
        assert!(labels_for("es-rv1909").is_some());
        assert!(labels_for("bogus").is_none());
    }

    #[test]
    fn recreate_schema_creates_books_and_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.sqlite");
        recreate_schema(&db).unwrap();

        let conn = Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 66);

        // Spot-check three tables that must exist for the TUI to work.
        for table in [
            "translation",
            "book_label",
            "verse",
            "heading",
            "footnote",
            "xref",
            "meta",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {table} missing");
        }
    }

    #[test]
    fn today_iso_has_expected_shape() {
        let s = today_iso();
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }
}
