//! `turbo-bible-data compress` — zstd-compress every `<code>.db` in
//! `dist/build/` into `dist/translations/<code>.db.zst` and emit
//! `manifest.json`.
//!
//! v1 uses plain `zstd -19` (no dictionary, default window). The Bible
//! corpus already compresses to ~25–30% at this setting; the marginal
//! gain from a trained dictionary doesn't justify a checked-in
//! artifact until binary size becomes load-bearing in Phase C.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ZSTD_LEVEL: i32 = 19;

pub fn run(in_dir: &Path, out_dir: &Path) -> Result<()> {
    if !in_dir.is_dir() {
        bail!(
            "expected built .db files in {}; run `build` first",
            in_dir.display()
        );
    }
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let mut translations = Vec::new();
    let mut xrefs_entry: Option<XrefsEntry> = None;
    let mut scrollmapper_commit = String::new();

    let mut entries: Vec<_> = fs::read_dir(in_dir)
        .with_context(|| format!("read_dir {}", in_dir.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("db") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("non-utf8 file name {}", path.display()))?
            .to_owned();

        let zst_path = out_dir.join(format!("{stem}.db.zst"));
        eprintln!("→ compress {} -> {}", path.display(), zst_path.display());
        let stats = compress_file(&path, &zst_path)?;

        if stem == "xrefs" {
            xrefs_entry = Some(XrefsEntry {
                file: relative_path(&zst_path, out_dir),
                sha256: stats.decompressed_sha256,
                compressed_size: stats.compressed_size,
                decompressed_size: stats.decompressed_size,
            });
        } else {
            let meta = read_meta(&path)?;
            if scrollmapper_commit.is_empty() {
                scrollmapper_commit.clone_from(&meta.source_commit);
            } else if scrollmapper_commit != meta.source_commit {
                bail!(
                    "inconsistent source_commit across translations: {} vs {}",
                    scrollmapper_commit,
                    meta.source_commit
                );
            }
            translations.push(TranslationEntry {
                code: meta.code,
                name: meta.name,
                language: meta.language,
                license: meta.license,
                attribution: meta.attribution,
                file: relative_path(&zst_path, out_dir),
                sha256: stats.decompressed_sha256,
                compressed_size: stats.compressed_size,
                decompressed_size: stats.decompressed_size,
                verse_count: meta.verse_count,
            });
        }
    }

    if translations.is_empty() {
        bail!("no translation .db files found in {}", in_dir.display());
    }

    let built_at = i64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("system clock is before the unix epoch")?
            .as_secs(),
    )
    .context("build timestamp overflows i64")?;

    let manifest = Manifest {
        schema_version: 1,
        scrollmapper_commit,
        built_at,
        translations,
        xrefs: xrefs_entry,
    };
    let manifest_path = out_dir.join("manifest.json");
    let body = serde_json::to_string_pretty(&manifest)?;
    fs::write(&manifest_path, body)
        .with_context(|| format!("write {}", manifest_path.display()))?;
    eprintln!("→ manifest -> {}", manifest_path.display());
    Ok(())
}

struct CompressStats {
    compressed_size: u64,
    decompressed_size: u64,
    decompressed_sha256: String,
}

fn compress_file(src: &Path, dst: &Path) -> Result<CompressStats> {
    let bytes = fs::read(src).with_context(|| format!("read {}", src.display()))?;
    let decompressed_size = bytes.len() as u64;
    let decompressed_sha256 = sha256_hex(&bytes);

    let dst_file = fs::File::create(dst).with_context(|| format!("create {}", dst.display()))?;
    // No explicit window_log: every translation/xrefs `.db` is well under
    // zstd's default window at level 19 (8 MiB), so a custom window is a no-op
    // on ratio. It would also be a footgun — the TUI's decoders
    // (`fetch::decode_and_verify`, `install`) accept only zstd's default decode
    // window (log 27). A frame compressed with `window_log(28+)` would be
    // *refused at runtime* by those decoders, and nothing here would catch it
    // (small payloads clamp the frame window, hiding the mismatch in tests). If
    // a future, larger corpus ever needs a bigger window, raise the decoders'
    // `window_log_max` in lockstep.
    let mut encoder = zstd::Encoder::new(dst_file, ZSTD_LEVEL)?;
    io::copy(&mut io::Cursor::new(&bytes), &mut encoder)?;
    let mut finished = encoder.finish()?;
    finished.flush()?;
    drop(finished);

    let compressed_size = fs::metadata(dst)
        .with_context(|| format!("stat {}", dst.display()))?
        .len();
    Ok(CompressStats {
        compressed_size,
        decompressed_size,
        decompressed_sha256,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn relative_path(p: &Path, base: &Path) -> String {
    p.strip_prefix(base).map_or_else(
        |_| p.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().into_owned(),
    )
}

struct DbMeta {
    code: String,
    name: String,
    language: String,
    license: String,
    attribution: String,
    source_commit: String,
    verse_count: i64,
}

fn read_meta(db: &Path) -> Result<DbMeta> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let (code, name, language, license, attribution, source_commit, verse_count) = conn
        .query_row(
            "SELECT code, name, language, license, attribution, source_commit, verse_count \
             FROM meta LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .with_context(|| format!("read meta from {}", db.display()))?;
    Ok(DbMeta {
        code,
        name,
        language,
        license,
        attribution,
        source_commit,
        verse_count,
    })
}

#[derive(Debug, Serialize)]
struct Manifest {
    schema_version: u32,
    scrollmapper_commit: String,
    built_at: i64,
    translations: Vec<TranslationEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    xrefs: Option<XrefsEntry>,
}

#[derive(Debug, Serialize)]
struct TranslationEntry {
    code: String,
    name: String,
    language: String,
    license: String,
    attribution: String,
    file: String,
    sha256: String,
    compressed_size: u64,
    decompressed_size: u64,
    verse_count: i64,
}

#[derive(Debug, Serialize)]
struct XrefsEntry {
    file: String,
    sha256: String,
    compressed_size: u64,
    decompressed_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_known_vectors() {
        // "" → e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc" → ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn manifest_serialization_shape() {
        let m = Manifest {
            schema_version: 1,
            scrollmapper_commit: "deadbeef".into(),
            built_at: 1_716_393_600,
            translations: vec![TranslationEntry {
                code: "en-bsb".into(),
                name: "Berean Standard Bible".into(),
                language: "en".into(),
                license: "CC0-1.0".into(),
                attribution: String::new(),
                file: "en-bsb.db.zst".into(),
                sha256: "abc".into(),
                compressed_size: 1,
                decompressed_size: 2,
                verse_count: 31102,
            }],
            xrefs: Some(XrefsEntry {
                file: "xrefs.db.zst".into(),
                sha256: "def".into(),
                compressed_size: 3,
                decompressed_size: 4,
            }),
        };
        let s = serde_json::to_string(&m).unwrap();
        // Spot-check field ordering matches the documented manifest shape.
        assert!(s.contains(r#""schema_version":1"#));
        assert!(s.contains(r#""scrollmapper_commit":"deadbeef""#));
        assert!(s.contains(r#""xrefs":{"#));
        assert!(s.contains(r#""verse_count":31102"#));
    }

    #[test]
    fn compress_roundtrip_under_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("hello.db");
        let dst = tmp.path().join("hello.db.zst");
        fs::write(&src, b"the quick brown fox jumps over the lazy dog").unwrap();
        let stats = compress_file(&src, &dst).unwrap();
        assert_eq!(stats.decompressed_size, 43);
        assert!(stats.compressed_size > 0);
        // Roundtrip: decompress and verify byte-for-byte.
        let compressed = fs::read(&dst).unwrap();
        let mut decoder = zstd::Decoder::new(io::Cursor::new(&compressed)).unwrap();
        let mut roundtripped = Vec::new();
        io::copy(&mut decoder, &mut roundtripped).unwrap();
        assert_eq!(roundtripped, b"the quick brown fox jumps over the lazy dog");
        // sha256 in stats matches the decompressed sha256 (and the source).
        assert_eq!(stats.decompressed_sha256, sha256_hex(&roundtripped));
    }
}
