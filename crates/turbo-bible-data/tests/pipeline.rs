//! End-to-end pipeline tests. Gated on `--ignored` because they need a
//! local scrollmapper checkout — point `TURBO_BIBLE_SCROLLMAPPER` at
//! one, or set it to `~/git/oss/bible_databases` (the default).
//!
//! These exercise the full `build` -> `compress` flow against a single
//! translation to keep wallclock cost reasonable; per-language coverage
//! is tested by the unit-level OSIS / label / regex tests.

use std::path::PathBuf;
use std::process::Command;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

fn scrollmapper_path() -> Option<PathBuf> {
    let p = std::env::var("TURBO_BIBLE_SCROLLMAPPER").ok().map_or_else(
        || {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join("git/oss/bible_databases"))
        },
        |s| Some(PathBuf::from(s)),
    )?;
    p.join("sources").is_dir().then_some(p)
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = .../crates/turbo-bible-data
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn run_cli(args: &[&str]) {
    let status = Command::new(env!("CARGO_BIN_EXE_turbo-bible-data"))
        .args(args)
        .status()
        .expect("spawn turbo-bible-data");
    assert!(status.success(), "turbo-bible-data {args:?} failed");
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut h = Sha256::new();
    h.update(bytes);
    let mut s = String::with_capacity(64);
    for b in h.finalize() {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// build → compress → decompress: the sha256 over the decompressed
/// bytes matches what the manifest recorded, the DB has the expected
/// verse count, FTS smoke-tests fire, and the 66 books are present.
#[test]
#[ignore = "needs TURBO_BIBLE_SCROLLMAPPER pointing at a scrollmapper checkout"]
fn end_to_end_en_bsb() {
    let Some(scrollmapper) = scrollmapper_path() else {
        eprintln!("skipping: no scrollmapper checkout");
        return;
    };
    let manifest_path = workspace_root().join("data").join("manifest_source.toml");
    assert!(
        manifest_path.is_file(),
        "missing {}",
        manifest_path.display()
    );

    let tmp = tempfile::tempdir().unwrap();
    let build_dir = tmp.path().join("build");
    let dist_dir = tmp.path().join("translations");

    run_cli(&[
        "build",
        "--scrollmapper",
        scrollmapper.to_str().unwrap(),
        "--manifest",
        manifest_path.to_str().unwrap(),
        "--out",
        build_dir.to_str().unwrap(),
        "--only",
        "en-bsb",
    ]);
    run_cli(&[
        "compress",
        "--in",
        build_dir.to_str().unwrap(),
        "--out",
        dist_dir.to_str().unwrap(),
    ]);

    // Verse count + OSIS coverage on the uncompressed build artifact.
    let db_path = build_dir.join("en-bsb.db");
    let conn = Connection::open(&db_path).unwrap();
    let verse_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM verse", [], |r| r.get(0))
        .unwrap();
    assert_eq!(verse_count, 31_102, "Protestant 66 KJV-aligned count");

    let book_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM book", [], |r| r.get(0))
        .unwrap();
    assert_eq!(book_count, 66);
    let label_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM book_label", [], |r| r.get(0))
        .unwrap();
    assert_eq!(label_count, 66);

    // FTS smoke — "God" must return hits for an English Bible.
    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM verse_fts WHERE verse_fts MATCH ?1",
            ["God"],
            |r| r.get(0),
        )
        .unwrap();
    assert!(hits > 100, "expected many FTS hits for 'God', got {hits}");

    // Roundtrip: sha256 of the uncompressed .db equals the manifest's
    // sha256 and equals the sha256 of what zstd decompresses.
    let manifest_path = dist_dir.join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let entry = manifest["translations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["code"] == "en-bsb")
        .expect("en-bsb in manifest");
    let manifest_sha = entry["sha256"].as_str().unwrap();
    let db_bytes = std::fs::read(&db_path).unwrap();
    assert_eq!(sha256_hex(&db_bytes), manifest_sha);

    // Verify a known verse text round-trips through the pipeline.
    let john_3_16: String = conn
        .query_row(
            "SELECT text FROM verse WHERE book = 'JHN' AND chapter = 3 AND verse = 16",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        john_3_16.contains("God") && john_3_16.contains("world"),
        "John 3:16 looks wrong: {john_3_16:?}"
    );

    // Now decompress the .db.zst and confirm byte-identical.
    let zst_path = dist_dir.join("en-bsb.db.zst");
    let mut decoder = zstd::Decoder::new(std::fs::File::open(zst_path).unwrap()).unwrap();
    let mut roundtripped = Vec::new();
    std::io::copy(&mut decoder, &mut roundtripped).unwrap();
    assert_eq!(sha256_hex(&roundtripped), manifest_sha);

    // Manifest invariants: every translation entry's sha256 matches the
    // matching .db.zst on disk (decompressed bytes); compressed_size
    // matches the file size.
    for t in manifest["translations"].as_array().unwrap() {
        let code = t["code"].as_str().unwrap();
        let zst = dist_dir.join(format!("{code}.db.zst"));
        let on_disk_size = std::fs::metadata(&zst).unwrap().len();
        assert_eq!(
            on_disk_size,
            t["compressed_size"].as_u64().unwrap(),
            "compressed_size mismatch for {code}"
        );
    }
}
