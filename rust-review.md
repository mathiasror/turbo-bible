# Rust Review: turbo-bible (round 3)

_Generated 2026-05-21. Baseline logs in `target/rust-review/`._

Third-pass review. Round 2's 11-item follow-up landed in full across
commits `919b453..414f0d1` — every item from the prior checklist is
done, plus the importer port shipped in `414f0d1`. This pass focuses on
the new `src/import.rs` (686 lines, brand new) and on whether the
preceding refactors held up after the `dispatch_reading` split.

Baseline ground truth:
- `cargo build --all-targets --all-features`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo clippy -W clippy::pedantic -W clippy::nursery`: **21
  warnings** (20 bin + 1 test). Down from 25 in round 2. Triaged in §6.
- `cargo doc --no-deps --all-features`: clean.
- `cargo test --all-features --no-run`: builds. (Runtime: the new
  `tests/import.rs` is `#[ignore]`d without a populated scrollmapper
  cache; the rest of the suite still goes green per `just check`.)
- `cargo audit`: 0 advisories.
- `cargo udeps`: clean (`All deps seem to have been used`).
- `cargo deny`: fails — no `deny.toml` in the repo, so the tool falls
  back to a default policy that rejects every license. Not a finding
  against the crate; flagged again in §9.
- `cargo tree -d`: same transitive dupes as round 2 — `bitflags 1/2`
  via `rexpect`, otherwise inherited from `ratatui`/`arboard`.

## Executive summary

- **One real blocker** this round: `turbo-bible import` builds a DB
  that's missing the `heading`, `footnote`, and `xref` rows the TUI
  reads to power §"Footnotes and cross-references" in `docs/USAGE.md`.
  After a fresh import, `K` will pop an empty popup and the References
  sidebar will be blank. The integration test at `tests/import.rs`
  only checks verse / book / FTS counts, so the regression slips
  through. See §1.
- The round-2 follow-ups all landed cleanly. `dispatch_reading` is
  now ~70 lines; `splash::render` was decomposed; `Bookmark`
  equality is reconciled; `Db::list_translations` is a free
  function; `make_status` is `&'static`; `mode_tag_for` returns
  `Cow<'static, str>`. The remaining pedantic 21 warnings are mostly
  new (introduced by the splits and by `import.rs`), not survivors.
- `import.rs` is otherwise reasonable, but has three smaller release
  hazards: the download cache validates only `len > 0` (§2), the
  `--db /custom/path` ↔ default `backup_dir` mismatch (§3), and a
  reimplemented Howard-Hinnant date routine that fires three `cast_*`
  warnings (§5). Each is a one-commit fix.
- `SCROLLMAPPER_COMMIT` is duplicated between `src/import.rs:25` and
  `tests/import.rs:17`. A drift here would silently green-light a
  wrong-cache test run (§4).
- The schema written by `recreate_schema` and the schema implied by
  `db.rs` queries can drift undetected. A round-trip schema test
  would have caught the missing footnote/heading import (§1).

## Blockers

### 1. `turbo-bible import` leaves footnotes, cross-refs, and headings empty

- **Location:** `src/import.rs:413-525` (`recreate_schema`,
  `import_translation`) vs `src/db.rs:333-414` (`load_passage` reads
  `heading`, `footnote`, `xref` tables) vs `docs/USAGE.md:205-226`
  (advertises footnote popup `K` and References sidebar).
- **Problem:** `import_translation` populates `translation`,
  `book_label`, and `verse` only. The schema in `SCHEMA_SQL`
  (`import.rs:224-293`) creates `heading`, `footnote`, and `xref`
  tables but no code path ever inserts into them. `db.rs::load_passage`
  unconditionally queries all three; the queries return zero rows
  for every chapter post-import, so:
  - **`K` (footnote popup)** always says "no footnotes on this verse"
    and closes.
  - **References sidebar** is permanently empty.
  - **Psalms / Pauline-epistle headings** never render (USAGE.md:91).
  - **Verse markers** for `footnote_count`/`xref_note_count` are
    always zero (`db.rs:312-318`) — no `†` / `‡` glyphs after verses.
  - `tests/import.rs:67-117` only asserts on `verse`, `book`,
    `book_label`, `verse_fts` counts; the regression doesn't trip CI.
- **Fix:** Two options:
  - (a) **Port the footnote/heading/xref ingestion** that the original
    Python script presumably did. The scrollmapper SQLite editions
    don't carry these (just `*_books` + `*_verses`), so the source
    must come from elsewhere — probably the same upstream USFM /
    osis-xml the Python crawler scraped. Until that source is
    identified and ported, `turbo-bible import` is a regression
    against whatever process built the DB used during round-2
    development.
  - (b) **Cut the features from v0.1 explicitly.** Delete the
    `heading` / `footnote` / `xref` tables from `SCHEMA_SQL`, gate
    `K` / sidebar code paths behind a feature flag (or simply remove
    them from `dispatch_reading`), and amend USAGE.md /
    CHANGELOG.md to say "not in v1". Less work, but it walks
    backwards on features that already work when the DB is
    pre-populated.
  - Pick one. If (a), file a tracking issue and put `--with-notes`
    behind a `--scrollmapper-only` default so users who don't want
    the network footwork still get the verses; if (b), tighten the
    test in `tests/import.rs` to assert that `footnote` / `heading` /
    `xref` are empty so the absence becomes intentional.
- **Rationale:** This is the only post-round-2 regression I'd block
  release on. Everything else is style. The README explicitly tells
  new users `turbo-bible import` is the setup step — a user
  following the README today gets a degraded TUI without realising
  it. The footnote/sidebar features are first-class in
  `docs/USAGE.md` and the demo GIF.
- **Maps to:** Spec-vs-implementation gap, not a clippy lint.

## Strong recommendations

### 2. `download_source` only validates `len > 0`

- **Location:** `src/import.rs:394-411`.
- **Problem:** Cache reuse: `if cached.exists() && std::fs::metadata(&cached)?.len() > 0`. A previous run that died mid-`io::copy` after the temp file was persisted (the persist is atomic via
  `NamedTempFile::persist` so it can only become the final cache
  entry once `io::copy` returned — that's reasonable) is fine. But
  a partial download where the server returns 200 with a truncated
  body (TCP RST, S3 503-then-200 retry weirdness, CDN
  partial-response edge cases) will succeed on `io::copy`, persist,
  and be reused forever. `ureq` doesn't enforce `Content-Length`
  unless you ask it to.
- **Fix:** Two layers, low cost each:
  - Pin a SHA-256 alongside `SCROLLMAPPER_COMMIT` for each of the
    three files. Verify after download, before
    `tmp.persist(...)`:
    ```rust
    struct Source {
        // ...
        sha256: &'static str,  // hex
    }
    // after io::copy:
    let actual = sha256_hex(tmp.as_file_mut())?;
    if !actual.eq_ignore_ascii_case(source.sha256) {
        bail!("sha256 mismatch for {}: got {actual}, want {}", source.file, source.sha256);
    }
    ```
    `sha2` is the conventional crate.
  - Alternative if a hash dep is too much: open the cached SQLite
    file with `Connection::open_with_flags(..., READ_ONLY)` and
    `PRAGMA integrity_check;` before accepting it. Catches truncated
    DBs and won't trip on legitimate revisions.
- **Rationale:** The "pinned commit" promise in the doc comment is
  half-kept — the URL pins the source, but the cache layer doesn't
  pin the bytes. Sites with retry storms or CDN flakiness will
  poison the cache eventually.
- **Maps to:** Defense-in-depth around an external trust boundary.

### 3. `--db /custom/path.sqlite` writes backups to the default location

- **Location:** `src/import.rs:325-336`.
- **Problem:** When the user passes `--db /tmp/test.sqlite` but no
  `--backup-dir`, `backup_dir` resolves to `paths::data_dir()?.join("backups")`
  — i.e. `~/.local/share/turbo-bible/backups`, not `/tmp/backups` next
  to the custom DB. The user runs an import against their throwaway
  path and accidentally leaks backups of their real DB (or, worse,
  backups *of an unrelated test DB* land in their real backups
  directory). It's also asymmetric with `cache_dir`, which has the
  same pattern.
- **Fix:** Anchor `backup_dir` and `cache_dir` to the chosen
  `db_path`'s parent when the user provides a `--db` but no explicit
  `--backup-dir` / `--cache-dir`:
  ```rust
  let backup_dir = match (&args.backup_dir, &args.db) {
      (Some(p), _) => p.clone(),
      (None, Some(db)) => db.parent().unwrap_or(Path::new(".")).join("backups"),
      (None, None) => paths::data_dir()?.join("backups"),
  };
  ```
  Same for `cache_dir` (anchor to `paths::cache_dir()` only when
  `--db` is also default — otherwise put it next to the DB or under
  a tempdir, since a custom DB path implies a one-off run).
- **Rationale:** The current behavior surprises in exactly the case
  where users want clear separation (test runs, throwaway imports).
  README explicitly documents `--db /custom/path.sqlite` as a feature.
- **Maps to:** API ergonomics, no specific lint.

### 4. `SCROLLMAPPER_COMMIT` lives in two files

- **Location:** `src/import.rs:25` and `tests/import.rs:17`.
- **Problem:** Both contain the literal `"a228a19a29099a41c196c2a310cd93e50a390e30"`. If the production constant moves and the test
  constant doesn't, `dev_scrollmapper_cache()` checks for files with
  the old prefix while the importer downloads with the new one — and
  if the user happens to have both caches present, the test will
  silently exercise the wrong dataset.
- **Fix:** Expose the constant from the crate root or a shared module
  and import in the test:
  ```rust
  // src/lib.rs (or src/import.rs)
  pub const SCROLLMAPPER_COMMIT: &str = "...";

  // tests/import.rs
  use turbo_bible::SCROLLMAPPER_COMMIT;
  ```
  The crate is currently `publish = false` so the public surface
  cost is nil. Alternatively, write the constant into a generated
  `.rs` from `build.rs` and `include!()` it both sides — heavier
  hammer for the same nail.
- **Rationale:** Duplicated single-source-of-truth values rot. The
  cost to fix is one re-export.
- **Maps to:** General single-source-of-truth principle.

### 5. `today_iso()` reimplements a date library inline

- **Location:** `src/import.rs:580-596`. Fires three pedantic
  warnings: `cast_possible_wrap` at L584, `cast_sign_loss` at L587,
  `cast_possible_wrap` at L589.
- **Problem:** Hinnant's algorithm is correct but it's 16 lines of
  unaudited bit-shifting in a binary that already pulls in `serde`,
  `toml`, `clap`, and `ureq` (each of which transitively pulls in
  small date crates indirectly). The casts compile clean today but
  the warnings are real — the `(secs / 86_400) as i64` step assumes
  `secs` fits in 63 bits (true until year 292 billion), which is
  fine, but the silent assumption deserves either a doc line or a
  per-cast `#[allow(clippy::cast_possible_wrap, reason = "...")]`.
- **Fix:** Two options:
  - (a) Add `time = { version = "0.3", default-features = false }`
    (or `jiff`) and `time::OffsetDateTime::now_utc().date().to_string()`.
    Adds one small dep (no proc-macros, no chrono), drops 16 LoC, and
    you stop owning the date logic.
  - (b) Keep the inline impl but wrap with
    `#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss, reason = "Hinnant's algorithm; see ..."]`
    above the function and add a one-line "why" pointing at the
    algorithm reference.
- **Rationale:** Backup filenames are not safety-critical, but
  carrying a hand-rolled calendar in a TUI crate is more code than
  it's worth. (a) is the cleaner call.
- **Maps to:** `clippy::cast_possible_wrap`, `clippy::cast_sign_loss`.

### 6. Remaining 21 pedantic warnings, triaged

| Pattern | Locations | Recommendation |
|---|---|---|
| `clippy::doc_markdown` | `src/import.rs:24,297,321`, `src/main.rs:1014` | Add the backticks around `scrollmapper/bible_databases`, `$XDG_DATA_HOME`, `SQLite`, `BTreeSet`. Mechanical. |
| `clippy::too_many_lines` | `src/render.rs:24` (160), `src/ui/find.rs` (150), `src/ui/splash.rs:247` (`handle_normal`, 106), `src/main.rs:255` (`main`, 119) | `render.rs:render_passage` and `find::render` were already noted last round. `main()` grew slightly in this round; the splash-vs-reading branch is the only worthwhile split point (factor into `fn start_reading(...)` / `fn start_splash(...)`). `handle_normal` is now 106/100 — close enough to ignore or to allow at function scope. |
| `clippy::unused_self` | `src/ui/help.rs:44` (`render`), `src/main.rs:868` (`copy_verse`) | `copy_verse` is new this round (extracted in `d89a498`); the `&self` was preserved for symmetry with the other extracted methods. Convert both to associated functions — they don't read `self`. |
| `clippy::missing_const_for_fn` | `src/main.rs:876` (`toggle_visual`), `src/main.rs:1123` (`make_status`), `tests/import.rs:19` (`binary_path`) | Add `const fn`. Three trivial wins. |
| `clippy::needless_pass_by_value` | `src/main.rs:855` — `HistoryDir` passed by value | Either derive `Copy` on `HistoryDir` (it's a two-variant fieldless enum, no reason not to) or take `&HistoryDir`. `Copy` is the right call. |
| `clippy::needless_pass_by_ref_mut` | `src/main.rs:1325` — `pos: &mut Position` in `apply_action` | False positive carried over from round 2 — clippy can't see through the `jump_to` call that writes `*pos`. Add `#[allow(clippy::needless_pass_by_ref_mut, reason = "written via jump_to call below")]`. Was listed last round; still here. |

All but the `too_many_lines` cluster collapse into one mechanical PR.

### 7. Schema lives in two places and can drift

- **Location:** `src/import.rs:224-293` (`SCHEMA_SQL` const) vs
  `src/db.rs` SELECTs that reference column names of every table.
- **Problem:** §1 above is partially this — the import side and the
  query side evolved separately, and there's no test that exercises
  both at once. Today the schema in `SCHEMA_SQL` matches the queries
  in `db.rs`, but the next column addition has a 50/50 chance of
  going one-sided.
- **Fix:** A round-trip integration test that doesn't require the
  scrollmapper cache:
  ```rust
  // tests/schema_roundtrip.rs
  #[test]
  fn import_schema_supports_db_queries() {
      let tmp = tempfile::tempdir().unwrap();
      let db_path = tmp.path().join("test.sqlite");

      // 1. Build the schema via the public import path.
      turbo_bible::import::recreate_schema_for_tests(&db_path).unwrap();

      // 2. Insert a synthetic row in every TUI-readable table.
      // ... (the tiny minimum set to make load_passage return something)

      // 3. Exercise every db.rs accessor.
      let mut db = Db::open_ro(&db_path, "test-tx").unwrap();
      let _ = db.list_books()?;
      let _ = db.load_passage("GEN", 1)?;
      // ...
  }
  ```
  This catches both (a) missing tables / columns and (b) the
  footnote-import gap in §1. `recreate_schema` is `fn` (private);
  expose a `pub(crate) fn recreate_schema_for_tests` or accept that
  the test lives in `src/import.rs::tests`.
- **Rationale:** Schema is the contract between two modules; today
  it's documented only by the schema string. CI should encode the
  contract.

### 8. `book_label.full_name` is always inserted as NULL

- **Location:** `src/import.rs:485-490`.
- **Problem:** `book_label.full_name` column exists; `db.rs:99`
  reads it; `Book::display_name` (`db.rs:104`) falls back to `name`
  when it's `None` — which today is always. The whole `full_name`
  column is dead. Reads of `display_name` always return `name`.
  Either populate `full_name` from somewhere (USFM book titles?
  scrollmapper's `*_books_info` if it exists?) or drop the column
  + the fallback.
- **Fix:** If keeping the column for a future ingest path, leave a
  trailing comment in the schema + the import call explaining "to
  be populated by `import_titles()`, see ticket #N". If not, delete
  the column, the field, and `display_name`'s fallback.
- **Rationale:** A column that never has a non-NULL value is dead
  weight in the schema and a foot-gun for the next contributor —
  same Chesterton's-fence flavor as the `bookmarks_translation`
  finding from round 2.

## Nice to have

- **`recreate_schema` is destructive** (`src/import.rs:419-430`
  removes the DB plus `-wal`/`-shm`/`-journal` sidecars) **but no
  CLI confirmation is required**. Today the backup runs first
  (good), but `--no-backup` + `--db ~/path/to/existing-thing.sqlite`
  silently obliterates an arbitrary file. Add a guard:
  `if existing && !args.no_confirm && !path_is_default { prompt }`
  — or at minimum print the about-to-delete paths before deleting.
- **Hard-coded canon size 66** at `src/import.rs:464`
  (`if books.len() != 66`). Comment says "Protestant 66-book canon"
  and the panic message says so too — fine. But the integer literal
  is repeated in tests (`tests/import.rs:89`, `tests/import.rs:94`)
  and in the schema constants (`BOOKS.len() == 66`,
  `KJV_LABELS.len() == 66`). One `const N_BOOKS: usize = 66;` on
  `import.rs` and re-exported into the tests would centralize it.
- **Five static-table allocations** (`BOOKS`, `KJV_LABELS`,
  `NB_1930_LABELS`, `ES_RV1909_LABELS`, `SCROLLMAPPER_NAME_TO_OSIS`)
  could be one `phf_map!` per lookup, but the cost is a build-time
  proc-macro; current linear scans through 66 entries run twice per
  translation (so 6× total) and are trivially fast. Worth nothing —
  flagging only because the same data is keyed five ways.
- **`labels_for(code)` returns `Option<...>` even though `SOURCES`
  only contains codes that the match in `labels_for` recognises**
  (`src/import.rs:552-559`). The `Option` is defensive but
  unreachable: `import_translation` is only called with a source
  from `SOURCES`, and every `SOURCES` entry has a matching arm. A
  `match` on a typed enum (or making `code: SourceCode`) would let
  the compiler enforce the relation. Today's `Option` + `bail!` is
  fine but the failure mode it protects against is "someone added
  a row to `SOURCES` and forgot the label table" — exactly the
  thing a `match` would catch at compile time.
- **`import_translation` reads the source DB's table names by string
  concatenation** (`format!("SELECT id, name FROM {table_prefix}_books ...")` at
  `src/import.rs:458`). `table_prefix` is derived from the trusted
  `source.file: &'static str` so SQL injection is moot, but `?`-bound
  table names aren't a thing in SQLite anyway. Leave a comment so
  future-you doesn't pattern-match this onto user-supplied data.
- **`cargo deny` is uninitialised**. Round 2 flagged it; still
  missing a `deny.toml`. A minimal policy:
  ```toml
  # deny.toml
  [licenses]
  version = 2
  allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
           "BSD-2-Clause", "BSD-3-Clause", "0BSD", "ISC", "Unicode-3.0",
           "Unicode-DFS-2016", "Zlib", "CC0-1.0"]
  confidence-threshold = 0.93

  [bans]
  multiple-versions = "warn"
  ```
  unlocks the deny step in `just baseline` and gives you a license
  policy gate.
- **`tests/import.rs` only runs against the developer's `~/.cache/turbo-bible/scrollmapper`**.
  Self-contained alternative: ship a tiny fixture SQLite at
  `tests/fixtures/kjv-mini.db` (5 books, 50 verses) and a fixture
  `Source` so the test can run in CI without network. The current
  `#[ignore]` discipline is fine for a personal project but you'll
  want the CI gate when contributors arrive.
- **`recreate_schema` could emit `PRAGMA journal_mode = WAL`** itself
  rather than relying on the follow-up `Connection` (`import.rs:370-375`)
  to do it. Today it works because the same process opens the DB
  immediately after. Cosmetic.
- **The doc comment on `import::run` (L320-323) says** "On any error,
  the partially-built DB is left as-is (the caller should re-run the
  importer rather than launching the TUI against half-imported
  data)." That's the contract, but `recreate_schema` already wiped
  the previous DB, so "partially-built" means "newer than the
  backup, older than usable" — i.e. unusable. Worth amending the
  doc to point at the backup path explicitly: "On error, restore
  from the file in `--backup-dir`."
- **Three `unwrap_or(0)` casts in chord-count math have been routed
  through `ListNav` per round 2 §2** — confirmed at
  `src/ui/splash.rs:64-67`. Round 3 still has one left at
  `src/keys.rs:159` (`u16::try_from(c.to_digit(10).unwrap_or(0))`)
  that didn't get the consolidation; minor.
- **`scripts/baseline.fish` is still broken** for fish 4 (`if $cmd > $logfile`
  fails the redirect because `$cmd` is an array — fish 4 enforces
  single-token redirect targets). Skill bug, not crate bug; flagged
  again. The bash equivalent works.

## Patches

### Hash-verify cached downloads (§2 sketch)

```rust
// src/import.rs — Source gains a sha256 field
struct Source {
    code: &'static str,
    file: &'static str,
    name: &'static str,
    license: &'static str,
    language: &'static str,
    sha256: &'static str,
}

// In download_source, after io::copy:
use sha2::{Digest, Sha256};
let mut hasher = Sha256::new();
let mut f = tmp.reopen()?;
io::copy(&mut f, &mut hasher)?;
let digest = hex::encode(hasher.finalize());
if !digest.eq_ignore_ascii_case(source.sha256) {
    bail!(
        "sha256 mismatch for {}: got {digest}, want {}",
        source.file, source.sha256,
    );
}
```

### Anchor `backup_dir` / `cache_dir` to `--db` (§3)

```rust
// src/import.rs — replace L325-336
let db_path = args.db.clone().unwrap_or(paths::data_dir()?.join("bible.sqlite"));
let db_parent = db_path.parent().map(Path::to_path_buf);
let backup_dir = args.backup_dir.clone().unwrap_or_else(|| match &args.db {
    Some(_) => db_parent
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("backups"),
    None => paths::data_dir().unwrap().join("backups"),  // fallback identical to before
});
```

(Cleaner with a helper; this is the shape.)

### Make `HistoryDir` `Copy` (§6)

```rust
// src/main.rs:833 — before
enum HistoryDir { Back, Forward }

// after
#[derive(Clone, Copy)]
enum HistoryDir { Back, Forward }
```

### Drop `&self` from `copy_verse` and `help::render` (§6)

```rust
// src/main.rs:868 — before
fn copy_verse(&self, ctx: &mut AppCtx) {

// after
fn copy_verse(ctx: &mut AppCtx) {
```

`HelpDialog::render` analogously; call sites become
`HelpDialog::render(rect, buf)`.

## Follow-up checklist

One commit per item, in priority order.

- [ ] 1. **Decide footnote/heading/xref direction** (§1). Either port
      the ingest (option a) or formally cut the feature (option b).
      Whichever is chosen, tighten `tests/import.rs` so the choice is
      verified.
- [ ] 2. Add SHA-256 verification (or `PRAGMA integrity_check`) to
      `download_source`. (§2)
- [ ] 3. Anchor `backup_dir` / `cache_dir` defaults to the `--db`
      parent when `--db` is non-default. (§3)
- [ ] 4. Re-export `SCROLLMAPPER_COMMIT` so `tests/import.rs` no
      longer hard-codes a duplicate. (§4)
- [ ] 5. Either pull in `time`/`jiff` for `today_iso()` or add the
      per-cast `#[allow(reason = "...")]`. (§5)
- [ ] 6. Mechanical pedantic sweep: `doc_markdown` (4 sites),
      `unused_self` (2 sites), `missing_const_for_fn` (3 sites),
      `needless_pass_by_value` on `HistoryDir`, the carry-over
      `needless_pass_by_ref_mut` `#[allow]`. (§6)
- [ ] 7. Add `tests/schema_roundtrip.rs` exercising `recreate_schema`
      + every `db.rs` accessor. (§7)
- [ ] 8. Populate `book_label.full_name` from a source, or drop the
      column. (§8)
- [ ] 9. Add `deny.toml` so `cargo deny check` runs in CI / baseline.
- [ ] 10. (Optional / contributor-facing) Replace the `#[ignore]`
      import e2e with a self-contained fixture-DB run so CI exercises
      it.

## Coverage self-assessment

| Dimension | Confidence | Notes |
|---|---|---|
| Compiler and lint cleanliness | high | All 21 pedantic warnings read end-to-end; clippy default is clean. |
| API design | high | New `import.rs` API (`ImportArgs`, `run`) reviewed against the rubric. `labels_for`/`Option` unreachable-by-construction noted. |
| Error handling | high | Every `?` and `with_context` in `import.rs` traced. Two real ergonomic gaps: `recreate_schema`'s destructive path (§Nice-to-have) and the partial-import contract (§Nice-to-have). |
| Ownership and borrowing | high | No new clones or unnecessary allocations in `import.rs`. Round-2 per-frame allocation findings all addressed (`make_status`, `mode_tag_for`, `bookmarks_set` left with documented justification). |
| Unsafe code | high | `#![deny(unsafe_code)]` still in force at `src/main.rs:1`. No new unsafe. |
| Concurrency | high | Still single-threaded; the import path explicitly drops the import `Connection` before `ensure_fts_optimized` to avoid `SQLITE_BUSY` (`import.rs:384-388`) — nice. |
| Testing | medium | Round-2 testing strengths preserved (proptest, PTY e2e). `tests/import.rs` is `#[ignore]`d and only checks shapes, missing the §1 regression. No fuzz target. |
| Documentation | high | Round-2 follow-up landed `//!` + `# Errors` + `#[must_use]` + `#[non_exhaustive]`; `cargo doc` clean. Only doc-debt is the `doc_markdown` backticks (§6) and the misleading "leaves DB as-is" line in `import::run`. |
| Project structure | high | `import.rs` is a clean module with its own constants table. Module count is up by one (was already covered by §"Layout" in README). |
| Dependencies and toolchain | high | `cargo audit` clean, `cargo udeps` clean, `cargo deny` still uninitialised (skill / config issue). One dep proposal (`sha2` / `time`) in §2 and §5. |
| Performance | medium | `import.rs` runs once and isn't hot; not profiled. Per-frame paths already cleaned. Could not verify cache reuse latency. |
| Contributor experience | medium | `just baseline` runs but `cargo deny` fails on the default policy — would block CI if wired in. The `tests/import.rs` `#[ignore]` gate is fine for a solo dev, less so for a contributor. |
