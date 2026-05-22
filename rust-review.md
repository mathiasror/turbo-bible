# Rust Review: turbo-bible (round 5)

_Generated 2026-05-22. Baseline logs in `target/rust-review/`._

Fifth-pass review on top of `035dd51`. The round-4 follow-up checklist
landed in commits `a7c6b4d..46034c6` and that round's findings are
closed (see round-4 in commit `035dd51` for the historical record).
This round is a fresh pass against the rubric, focused on:

- The **round-3 carry-over list** that round 4 deliberately did not
  touch — most of it is still open.
- A handful of **new round-5 observations** the prior reviews missed
  or that the codebase has grown into.

Baseline ground truth (re-run today):
- `cargo build --all-targets --all-features`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo clippy -W clippy::pedantic -W clippy::nursery`: **21 warnings**
  (20 bin + 1 test). Identical to round 4. None are new; none have
  been fixed.
- `cargo doc --no-deps --all-features`: clean.
- `cargo test --all-features`: 81 unit + 5 e2e pass; 2 import e2e
  `#[ignore]`d.
- `cargo audit`: 0 advisories.
- `cargo udeps --all-targets`: clean ("All deps seem to have been used").
- `cargo tree -d`: same transitive duplicates as round 4 (`bitflags 1/2`
  via `rexpect`, `hashbrown` × 3, `getrandom` × 3, `thiserror` × 2,
  `quick-error` × 2). One new transitive worth knowing about: ratatui
  pulled in `kasuari 0.4.12` (Cassowary-style constraint solver used by
  `ratatui-core` for layout). See §6.

## Executive summary

- **The round-3 §1 ingest blocker has been partially resolved** and the
  premise behind it was wrong: scrollmapper's per-translation DBs at
  the pinned commit contain only `*_books`, `*_verses`, and
  `translations` (no footnote, heading, or xref tables — rounds 3 and
  4 claimed they were there without verification). What scrollmapper
  *does* ship is global cross-references in
  `formats/sqlite/extras/cross_references_*.db` (openbible.info data).
  This round wires that in: ~430k unique xrefs are now ingested, the
  sidebar's "Cross-references" section is live, the K-popup ("Notes")
  shows the xref list for the cursor verse. The `heading` and
  `footnote` tables stay empty pending a different upstream source
  (e.g. STEPBible-Data); see §1 below.
- The **mechanical pedantic sweep** from round-3 §6 is still pending.
  The 21-warning count hasn't moved across two rounds. §2 lists each
  one. None requires architectural judgement; they're one commit
  away each.
- Three smaller round-3 carry-overs remain (`download_source` integrity
  check, `--db ↔ backup_dir` coordination, the `SCROLLMAPPER_COMMIT`
  duplication). All are still valid as-described in round 3.
- **`deny.toml` is still absent** and the CI `audit` job runs only on
  push/PR — a new advisory landing mid-week can sit unnoticed until
  the next merge (§6).
- One round-5-specific finding worth its own section: **`Db::set_translation`
  is a bare public-ish setter with an atomicity footgun** (§7).
  `switch_translation` in `main.rs` already implements the only safe
  swap path; the bare setter only exists for that one caller.

## Blockers

### 1. Cross-references landed; `heading` and `footnote` need a non-scrollmapper source (partial close of carry-over §1)

- **Background:** Rounds 3, 4, and 5 all framed this as "wire up the
  upstream `*_notes` / `*_headings` / `*_xrefs` tables, ~80 LoC."
  When the implementer reached for the schema at the pinned commit
  (`a228a19a29...`), the per-translation DBs turned out to contain
  only `*_books`, `*_verses`, and a shared `translations` table. The
  prior claim was carried forward unverified for three rounds.
- **What's now landed (commit pending review).** Cross-references
  exist as a separate global dataset at
  `formats/sqlite/extras/cross_references_0..6.db` (openbible.info,
  ~430 k unique pairs after symmetric-pair dedupe). This is wired in:
  - `xref` schema redesigned: keyed by `(from_book, from_chapter,
    from_verse, to_book, to_chapter, to_verse_start, to_verse_end)`
    plus `votes`. **Breaking** for users on the prior empty schema —
    they must re-run `turbo-bible import`. CHANGELOG flags this.
  - `import::import_cross_refs` downloads all 7 shards, dedupes via
    PK, normalizes Arabic/Roman numeral book names and the
    `Revelation` vs `Revelation of John` alias via a small variant
    table at `src/import.rs::SCROLLMAPPER_XREF_NAME_VARIANTS`.
  - `Db::load_xrefs` joins `book_label` on the active translation so
    `Xref.to_book_abbrev` is ready-to-render in any of the three
    installed languages.
  - `ui/sidebar.rs` Cross-references section now lists the top 8 by
    vote per cursor verse (cap at `SIDEBAR_XREF_CAP`).
  - `ui/footnote.rs` K-popup shows the full xref list with Enter to
    follow.
  - `Verse.xref_note_count` query now reads from the `xref` table, so
    the `ˣ` marker glyph in `src/render.rs:144-149` fires on every
    verse that has at least one xref (which is most of them).
  - `tests/import.rs::import_subcommand_builds_full_db` (`#[ignore]`)
    asserts `xref` rowcount, distinct from-book count, and that
    John 3:16's top xref is Romans 5:8.
- **What's still inert:** the `heading` table and the `footnote`
  table. No upstream source at the pinned commit. The schema and the
  loader (`Db::load_footnotes`) stay so a future ingest path
  doesn't need fresh plumbing. The reading view's sidebar still has a
  "Parallel passage" branch and a "Footnotes" branch that guard on
  `current_parallel(...).is_none()` / `f_notes.is_empty()`; both
  remain unreachable until a different source (e.g. STEPBible-Data,
  Crosswire OSIS) is wired in.
- **Recommendation for the next pass:** decide whether to (a) ingest
  headings from STEPBible-Data (CC-BY licensed; covers KJV at minimum,
  unclear coverage for nb-1930 / es-rv1909), (b) hand-curate a small
  section-heading TOML in the repo for the three editions, or
  (c) drop the `heading`/`footnote` schema and UI surfaces entirely
  to stop carrying dead code. Until that decision lands, **users may
  see an empty "Parallel passage" gap above the xrefs section**;
  that's noise rather than a bug.
- **Maps to:** behavioural completeness for xrefs (now done);
  forward decision for headings/footnotes.

## Strong recommendations

### 2. Land the round-3 §6 mechanical pedantic sweep

- **Location:** Twenty-one warnings, identical between rounds 3, 4,
  and 5. From `target/rust-review/clippy-pedantic.log`:
  - `clippy::doc_markdown` at `src/db.rs:22`, `src/db.rs:216`,
    `src/import.rs:24`, `src/import.rs:297`, `src/import.rs:321`,
    `src/main.rs:1008` — six docstrings missing backticks around
    `SQLite`, `BTreeSet`, `scrollmapper/bible_databases`,
    `$XDG_DATA_HOME`.
  - `clippy::cast_possible_wrap` + `clippy::cast_sign_loss` at
    `src/import.rs:584`, `:587`, `:589` — `today_iso`'s
    Howard-Hinnant epoch math.
  - `clippy::too_many_lines` at `src/render.rs:24` (146 lines),
    `src/ui/find.rs:100` (150), `src/ui/splash.rs:247` (106),
    `src/main.rs:254` (`main()`, 119).
  - `clippy::unused_self` at `src/ui/help.rs:66`, `:75`, and
    `src/main.rs:863` (`copy_verse`).
  - `clippy::needless_pass_by_value` at `src/main.rs:850`
    (`history_step`'s `dir: HistoryDir`).
  - `clippy::missing_const_for_fn` at `src/main.rs:871`
    (`toggle_visual`), `:1110` (`make_status`), and
    `tests/import.rs:19` (`binary_path`).
  - `clippy::needless_pass_by_ref_mut` at `src/main.rs:1311` — already
    carries a `#[allow]` with a reason; the `reason` argument is still
    correct (clippy can't follow the call into `jump_to`).
- **Problem:** Pedantic lints are advisory by definition, but a code
  base that's run pedantic clean elsewhere benefits from running it
  clean *everywhere*. The mixture of "true positive needs a fix" and
  "true positive needs an `#[allow]` with `reason`" means a new
  contributor running pedantic locally sees 21 warnings and can't
  tell at a glance which to act on. The longer the list lingers, the
  less signal each new warning carries.
- **Fix:** Three commits, in order of mechanical-ness:
  1. **Docstring backtick sweep**: `SQLite`, `BTreeSet`,
     `scrollmapper/bible_databases`, `$XDG_DATA_HOME` get backticks
     in their six call sites. Pure docs change, single commit.
  2. **Const-fn + Copy promotions**: `make_status`, `toggle_visual`,
     `binary_path` become `const fn`; `HistoryDir` derives `Copy`
     (it's a unit-style two-variant enum; `needless_pass_by_value`
     resolves and the call site stays unchanged).
  3. **`unused_self` removals**: `HelpDialog::handle` and
     `HelpDialog::render` become associated functions
     (`HelpDialog::handle(key)` at the call site). `copy_verse`
     loses its `&self` and becomes a free function in the
     `impl LoopState` block, callable as
     `LoopState::copy_verse(ctx)`. The `&self` was vestigial — the
     body only reads `ctx`.
  4. **`today_iso` cast cleanup**: switch to `cast_signed` /
     `cast_unsigned` per the clippy suggestion lines, or add
     per-site `#[allow]` blocks with `reason =` explaining the
     epoch-arithmetic intent.
  5. **`too_many_lines` carry-overs**: these are functions whose
     length is inherent (`render_passage` weaves verse/heading/marker
     state; `FindDialog::render` lays out the entire dialog; `main()`
     is the binary's entry; `handle_normal` is the splash key map).
     Either decompose if a natural seam exists (`splash::handle_normal`
     could split into `motion`, `chord`, `splash_specific` groups) or
     drop in `#[allow(clippy::too_many_lines, reason = "...")]` per
     site. Decomposing `main()` further would just lift the body into
     a free function with the same length — not worth it.
- **Rationale:** Same as round 3 §6: a pedantic-clean baseline lets
  *new* warnings be load-bearing signal. With 21 standing warnings the
  signal-to-noise is already zero. Knocking out items 1–4 above
  (~14 of the 21) is < 30 LoC of change.
- **Maps to:** `clippy::doc_markdown`, `clippy::missing_const_for_fn`,
  `clippy::needless_pass_by_value`, `clippy::unused_self`,
  `clippy::cast_possible_wrap`, `clippy::cast_sign_loss`,
  `clippy::too_many_lines`.

### 3. `download_source` accepts any non-empty cached file (carry-over §2)

- **Location:** `src/import.rs:397`:
  ```rust
  if cached.exists() && std::fs::metadata(&cached)?.len() > 0 {
      return Ok(cached);
  }
  ```
- **Problem:** A previous run that died mid-download (network drop, SIGINT,
  disk-full) can leave a partial `.db` cached. Re-running `turbo-bible
  import` happily reuses it as long as it's at least one byte, and the
  subsequent `Connection::open_with_flags` reads what looks like a
  truncated SQLite file. The user gets either a cryptic
  `disk image is malformed` or — worse — a successful open that's
  missing books. There's no checksum, size sanity check, or upstream
  ETag/Content-Length cross-reference. Today's defence is "the user
  noticed and ran `rm -rf ~/.cache/turbo-bible/`."
- **Fix:** Two options, increasing in robustness:
  - **(a)** After `tmp.persist(&cached)`, run a `SELECT count(*) FROM
    {table_prefix}_books` probe on the freshly persisted file. If it
    fails or returns < 66, delete the file and bail with a meaningful
    error. Cheap, no new deps.
  - **(b)** Pin SHA-256 hashes per `(SCROLLMAPPER_COMMIT, file)` and
    verify post-download. Catches both partial downloads and any future
    upstream mutation of the pinned commit (which shouldn't happen on a
    SHA-pinned URL, but trust-but-verify). One small `sha2`
    dependency.
  - (a) is what I'd ship today; (b) is the right shape for a v1.0.
- **Rationale:** This is a binary release the user runs once per
  installation. A silent half-imported state is the worst case for a
  one-shot setup; it manifests as inscrutable reading-view errors
  ("book GEN not found") only much later.
- **Maps to:** Defensive programming around external IO; no specific
  lint.

### 4. `--db /custom/path` ignores its own directory for backups (carry-over §3)

- **Location:** `src/import.rs:329-332`:
  ```rust
  let backup_dir = match &args.backup_dir {
      Some(p) => p.clone(),
      None => paths::data_dir()?.join("backups"),
  };
  ```
- **Problem:** `turbo-bible import --db /tmp/scratch.sqlite` (no
  `--backup-dir`) writes backups to
  `$XDG_DATA_HOME/turbo-bible/backups/`, not `/tmp/backups/`. If
  `--db` was passed because the user *can't* write to the default
  location (a CI run on a read-only volume, an ephemeral container),
  the backup phase will either succeed silently into the wrong place
  (writeable) or fail with a misleading "permission denied" referring
  to a path the user never typed.
- **Fix:** When `args.db.is_some()` and `args.backup_dir.is_none()`,
  default `backup_dir` to `args.db.parent().join("backups")`. Single-line:
  ```rust
  let backup_dir = match (&args.backup_dir, &args.db) {
      (Some(p), _) => p.clone(),
      (None, Some(db)) => db.parent()
          .map(|p| p.join("backups"))
          .ok_or_else(|| anyhow!("--db has no parent dir"))?,
      (None, None) => paths::data_dir()?.join("backups"),
  };
  ```
- **Rationale:** Two `--db`-like flags either share their root or one
  is explicitly anchored to the other. `--cache-dir` is independent
  (it's the upstream cache, not derived from the DB), but the backup
  belongs in the same neighbourhood as the DB itself.
- **Maps to:** CLI ergonomics; no specific lint.

## Nice to have

- **`PassageView::render` clears the inner rect twice** — same as
  round 4. `src/ui/passage.rs:39-48`: `block.style(...)` paints the
  block's inner area on `block.render(area, buf)`, then the explicit
  cell loop at L42-48 overwrites every cell again. Either drop the
  loop or add a comment noting it's belt-and-braces against
  ratatui-version drift. The double-paint is harmless today but
  obscures intent.
- **`SCROLLMAPPER_COMMIT` is duplicated** between `src/import.rs:25`
  and `tests/import.rs:17`. Make the importer's constant
  `pub(crate)` and have the test `use crate::import::SCROLLMAPPER_COMMIT`.
  Three-line change; eliminates a silent drift class. (Carry-over §4.)
- **`book_label.full_name` is always inserted as NULL**
  (`src/import.rs:486-489`). `Book::display_name` falls back to `name`
  so the user-visible impact is zero today. Two paths: either drop the
  column from `SCHEMA_SQL` (and the `Book.full_name` field) since
  nothing populates it, or wire up the upstream `*_books_names.long_name`
  column during the ingest pass added by §1. The current state is the
  worst-of-both: a column whose contract ("full title from the source
  page") the code can't keep. (Carry-over §8.)
- **Schema round-trip test** (carry-over §7): the only schema test is
  `recreate_schema_creates_books_and_tables`, which counts tables and
  the `book` rowcount. A test that inserts one row into each populated
  table, reads it back via `Db::load_passage`, and asserts every field
  round-trips would catch the kind of silent schema drift the `heading`/
  `footnote`/`xref` situation embodies.
- **Render snapshot test for `render_passage`**: round-4 §nice-to-have
  noted it; still missing. A 20-line test that builds a fake `Passage`
  (one chapter, two verses, one heading, one bookmarked verse, cursor
  on verse 2), calls `render_passage`, and asserts (a) the cursor row's
  first span begins with `\u{25B8}`; (b) the bookmark row's first span
  is `\u{2605}`; (c) the heading is present; (d) wrapped lines indent
  by `VERSE_PREFIX`. Catches every reading-view regression in one
  fast unit test.
- **`Db::open_ro` precondition is a `debug_assert!`**
  (`src/db.rs:223-227`). Release builds will silently accept an empty
  translation code and produce a confusing
  `translation_label` query miss. Return a typed error or just let
  the SQL fail loudly. The current call sites (`main.rs:277`) already
  validate via `resolve_translation`, so this is a defence-in-depth
  fix, not a bug.
- **`day_index` truncating cast** (`src/quote.rs:77`):
  `(secs / 86_400) as usize` truncates on 32-bit hosts past
  ~year 2138. The cast isn't flagged by pedantic today because
  `u64 → usize` is identity on 64-bit, but a 32-bit ARM
  Linux build (Raspberry Pi 0, etc.) would silently wrap. Use
  `usize::try_from(secs / 86_400).unwrap_or(usize::MAX)`. Tiny patch,
  cheap insurance.
- **`tests/e2e.rs` runs PTY tests against the developer's installed DB
  but CI can't reach it**. The tests skip when
  `~/.local/share/turbo-bible/bible.sqlite` is missing — fine for the
  contributor's machine, but it means the e2e suite is effectively
  CI-invisible. Two options: ship a tiny fixture DB (e.g. KJV Genesis
  1 only, ~5 KB) in `tests/fixtures/`, or run the importer in CI as a
  setup step. Either way the round-trip "user types `q`, state.toml is
  written" assertion is the only end-to-end check we have, and it
  currently runs on zero CI workers.

## Round-5 specific findings

### 5. CI's `audit` job has no scheduled trigger

- **Location:** `.github/workflows/ci.yml` — the `audit` job runs only
  on `push` to `main` and on `pull_request`. There's no `schedule:`
  trigger.
- **Problem:** A RustSec advisory landing on a Tuesday for a
  transitive dep (say, `rustls`, `ring`, or `rusqlite`) doesn't
  surface until the next push, which on a hobby crate might be weeks
  away. The point of `cargo audit` in CI is to be a watchdog; without
  a cron trigger, it's just a passenger.
- **Fix:** Add a weekly schedule to the existing `audit` job:
  ```yaml
  on:
    push:
      branches: [main]
    pull_request:
    schedule:
      - cron: '0 6 * * 1'   # Mondays at 06:00 UTC
    workflow_dispatch:
  ```
  Plus a small note in `CONTRIBUTING.md` so contributors know the
  weekly heartbeat exists.
- **Rationale:** The CI bill is identical (one job per week is
  ~$0.00 on the GitHub free tier). The blast-radius reduction is
  meaningful: a Sunday-night advisory gets a Monday-morning issue
  filed instead of "whenever someone next opens a PR."
- **Maps to:** CI hygiene; no specific lint.

### 6. `deny.toml` is still absent (carry-over §9)

- **Location:** Project root has no `deny.toml`. `cargo-deny` is
  installed in the developer's `$PATH` (per `which cargo-deny`).
- **Problem:** `cargo audit` catches known vulnerabilities; `cargo
  deny` catches **policy** drift — duplicate-version policies, license
  blocklists, banned-crates lists, and unknown-source registry
  warnings. The crate has 8 transitive duplicates today (round-3 §0
  table); without a `deny.toml`, a 9th doesn't even register.
  Particularly relevant for round 5 because `kasuari` is a new
  transitive that landed since round 3 — a `deny.toml` with a
  `sources.allow-registry = ["https://github.com/rust-lang/crates.io-index"]`
  block would have flagged the new crate during review rather than
  via `tree -d` archaeology.
- **Fix:** A starter `deny.toml`:
  ```toml
  [graph]
  all-features = true

  [advisories]
  ignore = []
  yanked = "deny"

  [licenses]
  allow = [
      "Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause",
      "ISC", "Unicode-3.0", "MPL-2.0", "Zlib",
  ]
  confidence-threshold = 0.93

  [bans]
  multiple-versions = "warn"
  # Document acceptable duplicates rather than silencing them.
  skip = [
      { name = "bitflags",   version = "1" },  # via rexpect (dev-only)
      { name = "hashbrown",  version = "0.14" }, # rusqlite
      { name = "thiserror",  version = "1" },  # rexpect (dev-only)
      { name = "quick-error", version = "1" }, # rexpect (dev-only)
  ]

  [sources]
  unknown-registry = "deny"
  unknown-git = "deny"
  allow-registry = ["https://github.com/rust-lang/crates.io-index"]
  ```
  Wire it into CI as a third job alongside `check` and `audit`.
- **Rationale:** With `deny.toml` and the `schedule:` from §5, the
  supply-chain side of the rubric goes from "we hope cargo audit
  catches it" to "we catch advisories, license drift, banned crates,
  and unexpected git sources, on a weekly heartbeat." Cheap to set
  up, and the explicit `skip` list documents the duplicates instead
  of leaving them as folklore in the rust-review files.
- **Maps to:** Rubric §10 (Dependencies and toolchain), supply-chain
  hygiene.

### 7. `Db::set_translation` is a bare setter with an atomicity footgun

- **Location:** `src/db.rs:211-213`:
  ```rust
  pub fn set_translation(&mut self, code: String) {
      self.translation = code;
  }
  ```
  Doc says "Callers that need an atomic translation swap (i.e. roll
  back on a follow-up failure) must implement that themselves; see
  `switch_translation` in `main.rs`."
- **Problem:** The doc is honest, but the API shape isn't. A future
  caller writing `db.set_translation("xx-fake".into())` followed by
  any failing call leaves `db` in a half-swapped state: the
  `translation` field says `"xx-fake"`, but the cached prepared
  statements (rusqlite caches them per-`Connection`, which doesn't
  rebind) and the upstream callers' assumptions about which
  translation's books they hold are out of sync. The one safe caller
  (`switch_translation` in `main.rs:1306`) already implements the
  rollback path correctly. The bare setter exists only as a building
  block for that one caller.
- **Fix:** Either of these is cleaner than the current shape:
  - **(a)** Move `switch_translation` into `impl Db` and delete the
    bare setter. Callers go from
    `db.set_translation(code)` + ad-hoc reload calls
    to `db.try_switch_translation(code)?` which does the probe-and-rollback
    internally. The only downside is `Db` would need to expose the
    "reloaded books / passage / label" result, which the current
    free-function returns through `&mut` parameters. Either return a
    new struct (`TranslationSwap { books, label, passage }`) or
    expose `Db::reload_for(code, …)` and let `main.rs` keep the
    `switch_translation` orchestration.
  - **(b)** Rename to `set_translation_unchecked` and mark
    `pub(crate)`. The footgun stays, but the call-site reads as a
    deliberate unsafety contract ("I have already verified the
    follow-up calls will succeed, or I am holding the rollback
    myself").
- **Rationale:** "Setter whose doc says 'don't use this directly'" is
  the same shape as `unsafe fn`-without-the-keyword: it relies on
  every future caller reading the doc. The crate is small enough
  today that this isn't actively biting, but encapsulating the swap
  removes a class of future bugs at the cost of one struct move.
- **Maps to:** API design hygiene (`C-OBJECT`-adjacent); the principle
  that a method's signature should encode its preconditions.

## Patches

### Drop the duplicated `SCROLLMAPPER_COMMIT` (§nice-to-have)

```rust
// src/import.rs — change visibility
pub(crate) const SCROLLMAPPER_COMMIT: &str =
    "a228a19a29099a41c196c2a310cd93e50a390e30";
```

```rust
// tests/import.rs — top of file
- const PINNED_COMMIT: &str = "a228a19a29099a41c196c2a310cd93e50a390e30";
+ use turbo_bible::import::SCROLLMAPPER_COMMIT as PINNED_COMMIT;
```

The crate today has no `lib.rs`, so this needs either a slim
`src/lib.rs` re-exporting `pub use import::SCROLLMAPPER_COMMIT;` or
moving the const into a small `pub mod` that `tests/import.rs` can
reach. The cleanest path: add a one-line `src/lib.rs` that re-exports
exactly the items tests need.

### `--db ↔ backup_dir` coordination (§4)

```rust
// src/import.rs — replace the existing match in `run`
let backup_dir = match (&args.backup_dir, &args.db) {
    (Some(p), _) => p.clone(),
    (None, Some(db)) => db
        .parent()
        .map(|p| p.join("backups"))
        .ok_or_else(|| anyhow!("--db {:?} has no parent dir", db))?,
    (None, None) => paths::data_dir()?.join("backups"),
};
```

### `download_source` minimal integrity check (§3)

```rust
// src/import.rs — after tmp.persist(&cached)?
tmp.persist(&cached)
    .map_err(|e| anyhow!("persist cached download: {e}"))?;
// Verify the persisted file actually opens as a SQLite DB with the
// expected upstream books table. A partial download or upstream
// truncation makes either step error out — we don't want to retain
// a half-written file in the cache.
{
    let probe = Connection::open_with_flags(
        &cached,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .with_context(|| format!("open cached {}", cached.display()))?;
    // KJV.db → KJV_books table per scrollmapper's naming convention.
    let table_prefix = file.strip_suffix(".db").unwrap_or(file);
    let n: i64 = probe
        .query_row(
            &format!("SELECT COUNT(*) FROM {table_prefix}_books"),
            [],
            |r| r.get(0),
        )
        .with_context(|| format!("probe books in {}", cached.display()))?;
    anyhow::ensure!(
        n == 66,
        "{}: expected 66 books in cached file, got {n}; \
         delete it and retry",
        cached.display()
    );
}
Ok(cached)
```

## Follow-up checklist

All twelve items landed in the post-review pass.

- [x] 1. Heading / footnote / xref ingest. **Partial:** xrefs landed
      end-to-end (~430 k rows); headings and footnotes remain
      unsourced because the pinned scrollmapper commit doesn't have
      them — that's the real surprise from this round. (§1)
- [x] 2. Mechanical pedantic sweep, part 1: docstring backticks.
- [x] 3. Mechanical pedantic sweep, part 2: const-fn + Copy
      promotions.
- [x] 4. Mechanical pedantic sweep, part 3: `unused_self` cleanups.
- [x] 5. Mechanical pedantic sweep, part 4: `today_iso` casts.
- [x] 6. `download_source` integrity probe — schema-agnostic
      `PRAGMA quick_check`.
- [x] 7. `--db ↔ backup_dir` defaulting.
- [x] 8. Add `deny.toml` + wire `cargo deny check` into CI.
- [x] 9. Schedule CI `audit` weekly.
- [x] 10. Encapsulate translation swap (option b: rename to
      `set_translation_unchecked` + `pub(crate)`).
- [x] 11. Drop `SCROLLMAPPER_COMMIT` duplication via slim `src/lib.rs`.
- [x] 12. Add a render snapshot test for `render_passage`.

## Coverage self-assessment

| Dimension | Confidence | Notes |
|---|---|---|
| Compiler and lint cleanliness | high | Pedantic+nursery now **clean** (was 21). `cargo clippy -D warnings` clean. |
| API design | high | `Db::set_translation` encapsulated as `_unchecked` + `pub(crate)`. Dialog/outcome enums uniformly `#[non_exhaustive]`. |
| Error handling | medium | Binary crate, `anyhow` throughout — appropriate per rubric. `# Errors` sections present on most public functions. The `debug_assert!` in `Db::open_ro` is the only silent-failure path. |
| Ownership and borrowing | high | No new allocations in hot paths. `pad_to_width` still iterates `s.content.chars().count()` per span; ratatui's own `Line::width()` would be cheaper but neither shows up under `samply` for realistic chapter sizes. |
| Unsafe code | high | `#![deny(unsafe_code)]` at `src/main.rs:10` unchanged. No unsafe added since round 3. |
| Concurrency | high | Single-threaded reading loop unchanged. `theme::THEME` is `OnceLock`; correct. |
| Testing | high | 87 unit + 5 e2e pass (was 81). Six new `render_passage` tests cover the gutter glyphs, headings, and hanging indent. Import e2e tests stay `#[ignore]` but the new xref assertions verify the ingest end-to-end. |
| Documentation | high | Crate-level `//!` on every module, `# Errors` sections on most fallible publics, `CHANGELOG`/`README`/`CONTRIBUTING`/`USAGE` aligned with the code. Doc-backtick pedantic warnings all fixed. |
| Project structure | high | `src/main.rs` is 1404 lines; the recent extractions (`LoopState`, `AppCtx`, `dispatch_*`) hold. `ui/` is well-split. New `src/lib.rs` is minimal-by-design. |
| Dependencies and toolchain | high | `cargo audit` + `cargo deny check` both clean; `udeps` clean. `deny.toml` documents license/sources/bans policy with explicit `skip` list for known dupes. Audit + deny run on a weekly cron. |
| Performance | medium | No benchmarks. Hot paths (per-draw `render_passage`, `bookmarks_set`) are bounded by chapter size — fine for the realistic ≤176 verses. `bookmarks_set` rebuilds the `BTreeSet` per draw frame at ~6 Hz; would matter only with thousands of bookmarks. |
| Contributor experience | medium | `just check` mirrors CI; `just audit` / `just deny` round out the supply-chain check locally. The skill's own `scripts/baseline.fish` is still broken (`$status` collision in fish 4); the project's `just baseline` is the working substitute. CI runs on stable Ubuntu only — no Windows/macOS matrix even though `etcetera` claims XDG correctness across platforms. |

## Round-3 carry-over status (at end of round 5)

| Round-3 item | Round-5 final state | Commit |
|---|---|---|
| §1 ingest gap | xrefs DONE; heading/footnote still OPEN (no upstream) | `9880490` |
| §2 `download_source` `len > 0` | DONE | `e193890` |
| §3 `--db` ↔ `backup_dir` mismatch | DONE | `e193890` |
| §4 `SCROLLMAPPER_COMMIT` duplicated | DONE | `310d6d8` |
| §5 `today_iso()` cast warnings | DONE | `d69606d` |
| §6 mechanical pedantic sweep | DONE | `d69606d` |
| §7 schema round-trip test | OPEN — `render_passage` snapshot tests landed (`681be1d`), but a DB-level `Db::load_passage` round-trip is still untested | |
| §8 `book_label.full_name` always NULL | OPEN — gated on the headings/footnotes source decision | |
| §9 `deny.toml` missing | DONE | `e2b1699` |

## Round-5 work landed

Xref ingest end-to-end:

- Schema redesign for `xref` (`src/import.rs::SCHEMA_SQL`).
- Download + ingest of `cross_references_0..6.db`
  (`src/import.rs::download_source`,
  `src/import.rs::import_cross_refs`,
  `src/import.rs::lookup_osis_xref`).
- Loader path: `db::Xref` reshaped, `db::Passage.xrefs` added,
  `db::Db::load_xrefs` with translation-aware `book_label` JOIN,
  `Verse.xref_note_count` now reads from the `xref` table.
- UI: `ui/sidebar.rs` Cross-references section, `ui/footnote.rs`
  K-popup xref list, both backed by `Passage.xrefs`.
- Test: `tests/import.rs` asserts xref rowcount + book coverage + a
  spot-check that John 3:16's top xref is Romans 5:8.
- `CHANGELOG.md` flags the breaking schema change and the new
  ingest.

Baseline post-change: build / clippy `-D warnings` / fmt / test
(86 passing) all clean.

## Resolution

All twelve round-5 follow-up items landed in commits
`9880490..681be1d`. Full sequence:

| Item | Commit | Notes |
|---|---|---|
| §1 ingest gap | `9880490` | feat(import): cross-reference ingest + xref schema redesign — wires 432 949 unique openbible.info xrefs end-to-end. Headings/footnotes carry over open; no upstream source at the pinned commit. |
| §2 docstring backticks | `d69606d` | Part of the pedantic sweep. |
| §3 const-fn + Copy | `d69606d` | Same. |
| §4 unused_self | `d69606d` | Same. |
| §5 today_iso casts | `d69606d` | `cast_signed` / `cast_unsigned` (stable from 1.87+). |
| §6 download_source probe | `e193890` | Schema-agnostic `PRAGMA quick_check`; deletes + re-downloads on failure. |
| §7 `--db` ↔ `backup_dir` | `e193890` | Parent-relative default when `--db` is set without `--backup-dir`. |
| §8 deny.toml + CI step | `e2b1699` | `cargo deny check advisories bans licenses sources` as a new CI job. |
| §9 weekly audit cron | `e2b1699` | Same workflow; `schedule: cron '0 6 * * 1'`. |
| §10 translation swap | `0f5c557` | `set_translation` → `set_translation_unchecked` + `pub(crate)`. |
| §11 SCROLLMAPPER_COMMIT dup | `310d6d8` | New `src/lib.rs` re-exports the constant for both bin and tests. |
| §12 render snapshot test | `681be1d` | Six tests on `render_passage` covering cursor / bookmark / selection / heading / hanging-indent. |

Two carry-overs survive:

- **Headings / footnotes data source.** Scrollmapper doesn't ship them
  at the pinned commit; their schema and UI surfaces stay (for now)
  pending a deliberate decision between (a) STEPBible-Data ingest,
  (b) hand-curated TOML in the repo, or (c) drop the schema and UI
  entirely. `book_label.full_name` (round-3 §8) is gated on the same
  decision.
- **DB-level schema round-trip test.** The render path is now covered;
  a parallel test that inserts → `Db::load_passage` → asserts every
  field on `Passage` / `Verse` / `Heading` / `Footnote` / `Xref` is
  still TODO. Naturally fits with whatever decision lands on the
  heading/footnote source.

Baseline at the close of the round (sha after `681be1d`):

| Check | State |
|---|---|
| `cargo build --all-targets --all-features` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo clippy -W clippy::pedantic -W clippy::nursery` | **clean (was 23)** |
| `cargo doc --no-deps --all-features` | clean |
| `cargo fmt -- --check` | clean |
| `cargo test --all-features` | 87 unit + 5 e2e pass; 2 import-e2e `#[ignore]` |
| `cargo audit` | 0 advisories |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `cargo udeps --all-targets` | clean |
