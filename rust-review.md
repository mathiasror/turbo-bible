# Rust Review: turbo-bible (round 2)

_Generated 2026-05-21. Baseline logs in `target/rust-review/`._

Second-pass review of the same crate after the 13-item follow-up sweep
landed in `f76fabd`. The previous review's blockers (atomic
`switch_translation`, bounded `History`, `App`-style refactor of
`run()`, deduped `word_wrap`/`config_dir`, RAII `TerminalGuard`,
`#![deny(unsafe_code)]`, …) are all in. This pass focuses on what's
left after the easy wins.

Baseline ground truth:
- `cargo build --all-targets --all-features`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo clippy -W clippy::pedantic -W clippy::nursery`: **25 warnings**
  (down from 102 last round). Triaged below.
- `cargo doc --no-deps --all-features`: clean.
- `cargo test --all-features`: 71 unit + 5 e2e tests, all pass.
- `cargo audit`: 0 advisories.
- `cargo tree -d`: unchanged transitive duplicates (mostly via `rexpect`
  and `ratatui`).

## Executive summary

- The crate is in genuinely good shape. Zero blockers this round —
  everything that follows is "raise the bar," not "fix a real bug."
- The refactor of `run()` into `LoopState` + `AppCtx` + per-mode
  dispatchers worked. `dispatch_reading` is now the only function still
  over 100 lines (146) and `main()` itself is at 116 — both expected
  given the surface area they coordinate, but worth the next pass.
- The remaining 25 pedantic warnings are highly clustered (5 ×
  `similar_names` in splash, 4 × `items_after_statements` in help,
  3 × `match_same_arms`, 3 × `unused_self`, 6 × `too_many_lines`,
  plus singletons). With the easy ones done, the rest are either taste
  calls or genuinely worth touching — see §3.
- `SplashView` has carried over its own bespoke chord+count state
  (`pending_g`, `count`) instead of using the `ListNav` you already
  built for the list dialogs. Consolidating would remove the third copy
  of `u16::try_from(c.to_digit(10).unwrap_or(0)).unwrap_or(0)` in the
  tree (which clippy points at as `cast_lossless` candidates).
- `Bookmark` derives `PartialEq`/`Hash` AND has a custom `same_range`
  method that compares a strict subset of fields. Today no caller
  notices, but the two equalities will drift; pick one.

## Blockers

None.

## Strong recommendations

### 1. `dispatch_reading` is the new outlier at 146 lines

- **Location:** `src/main.rs:816-969`
- **Problem:** The Bg/Reading branch of the run loop now lives here. It
  works, but every `Action::*` arm reaches into the same locals
  (`state.bg`, `state.dialog`, `state.last_label_for_splash`,
  `state.bookmarks`, `state.visual_anchor`, `ctx.pos`, `ctx.passage`,
  `ctx.cursor_verse`, `ctx.warnings`). Adding a new action means
  editing one more arm and reaching the same five fields. `clippy::too_many_lines`
  flags it (146/100).
- **Fix:** Split out the high-cardinality groups into `impl LoopState`
  methods, mirroring what was done for the dispatcher boundaries:
  - `fn open_footnote(&mut self, ctx: &mut AppCtx) -> Dialog`
  - `fn jump_back(&mut self, ctx: &mut AppCtx) -> Result<()>` /
    `fn jump_forward(...)` — already isomorphic to each other; could
    share an inner `step(direction: HistoryStep)` helper.
  - `fn enter_splash(&mut self, ctx: &mut AppCtx)` for the `Action::Back`
    fall-through that rebuilds `SplashView`.
  - `fn add_bookmark(&mut self, ctx: &mut AppCtx)` — owns the
    visual-anchor arithmetic.
  - `fn enter_visual(&mut self, cursor: i64)` / `fn exit_visual(&mut self)`.
  This shrinks the arm bodies to one-line method calls and exposes the
  units that are actually testable in isolation.
- **Rationale:** Same logic as last round's `run()` finding: refactor
  before the BACKLOG.md `import` subcommand lands and the table grows
  further. The previous reviewer was right that this kind of file
  rewards aggressive method extraction.

### 2. `SplashView` reinvents `ListNav`

- **Location:** `src/ui/splash.rs:64-67, 163-389` vs
  `src/ui/listnav.rs:1-99`.
- **Problem:** `SplashView` carries its own `pending_g: Option<Instant>`
  and `count: u16` plus an inline state machine in `handle_normal` for
  digit accumulation, `gg`, `G`, `Ctrl-D`/`U`/`F`/`B`, `PageUp`/`Down`,
  `Home`/`End`. The list dialogs (`bookmarks`, `translations`,
  `footnote`) have all been routed through `ListNav` already; this is
  the third copy. The third instance of `u16::try_from(c.to_digit(10).unwrap_or(0)).unwrap_or(0)`
  (also in `keys.rs:159` and `listnav.rs:41`) lives here.
- **Fix:** Extend `ListNav` with the splash-shaped extras and replace
  the inline state machine. The honest blockers are:
  - **Two-column cursor.** Splash has `cursor_ot` + `cursor_nt` + a
    `focus: SplashColumn` selector. `ListNav` is single-column. Either
    parameterise `ListNav` over a `Cursor` trait (overkill) or hold
    two `ListNav`s on `SplashView` and dispatch by focus.
  - **`Continue` row above the columns.** Splash's `on_continue` flag
    intercepts navigation. Wrap the `ListNav::Step` return in a thin
    `match` that consumes `on_continue` for the top boundary case.
  - **Time-based chord reset.** `SplashView::handle` walks `pending_g`'s
    `Instant`; `ListNav` resets purely on the next keypress. The
    list-dialog behavior is arguably better (no hidden timeout); adopt
    it on splash and drop the `Instant` field.
- **Rationale:** Three implementations of the same vim-list state
  machine is the threshold where the abstraction earns its keep. The
  test fixture in `splash.rs` (`move_up_from_top_lands_on_continue`)
  already pins the only splash-specific behaviour worth preserving.

### 3. The remaining 25 pedantic warnings, triaged

| Pattern | Count | Recommendation |
|---|---|---|
| `clippy::similar_names` in splash (`books_ot`/`books_nt`, `entries_ot`/`entries_nt`, `i_ot`/`i_nt`, …) | 5 | Allow at the module level (`#![allow(clippy::similar_names)]` in `splash.rs`); the OT/NT distinction is the whole point and renaming would obscure intent. |
| `clippy::items_after_statements` in `help.rs:51-55` (the `Row` enum + `use Row::{...}` declared mid-function) | 4 | Hoist the `enum Row` and `use` to the top of `render`. Trivial. |
| `clippy::match_same_arms` — keys.rs:294-297 (4 chord starters → `Resolve::Partial`), splash.rs:270-274 (F2/`:` → OpenGoto, F5/`t` → OpenTranslations) | 3 | Merge with `|` patterns. Clippy's auto-suggestion is correct; the only reason these are still here is they're cosmetic. |
| `clippy::too_many_lines` — `render::render_passage` (160), `find::render` (150), `splash::handle_normal` (124), `splash::render` (262), `main::main` (116), `main::dispatch_reading` (146) | 6 | See §1 for `dispatch_reading`. `splash::render` is the biggest; split into helper `fn render_title`, `fn render_filter_row`, `fn render_columns`. The renderers are pure functions of `&self` + a `Buffer`, so extraction is mechanical. |
| `clippy::unused_self` — `help::HelpDialog::handle`/`render`, `main::LoopState::bookmarks_translation` | 3 | `HelpDialog` is a unit struct; make `handle`/`render` associated functions (or implement `Widget` for `&HelpDialog`). `bookmarks_translation` is dead-weight — see §4. |
| `clippy::option_if_let_else` in `goto.rs:223` (the `match rest.find([':',',','.'])`) | 1 | Apply the suggestion (`map_or`); same semantics, less indentation. |
| `clippy::or_fun_call` in `statusbar.rs:22` — `unwrap_or(theme::light_grey())` | 1 | Swap for `unwrap_or_else(theme::light_grey)`. One-character fix that actually avoids the eager call. |
| `clippy::useless_let_if_seq` in `splash.rs:522` (the `cursor_extra` accumulator) | 1 | Rewrite as `let cursor_extra = if ... { 1 } else { 0 };` — the suggestion is correct. |
| `clippy::equatable_if_let` in `main.rs:716` — `if let HelpOutcome::Cancel = d.handle(key)` | 1 | Use `matches!`. |
| `clippy::needless_pass_by_ref_mut` on `apply_action(pos: &mut Position, ...)` at `main.rs:1264` | 1 | False positive — `pos` is written through `jump_to`'s `&mut Position` parameter; clippy can't follow the call. Add `#[allow(clippy::needless_pass_by_ref_mut, reason = "written via jump_to call below")]`. |

Aim: take everything except `similar_names` and `too_many_lines` in one
mechanical PR (those two need a refactor or a module-level allow with a
justification, not a touch-each-line sweep).

### 4. `LoopState::bookmarks_translation` is a no-op wrapper

- **Location:** `src/main.rs:628-637`
- **Problem:** The method signature is `fn bookmarks_translation<'a>(&self, passage: &'a Passage) -> &'a str` and the body is literally `&passage.translation`. `self` is unused (`clippy::unused_self`). The doc comment explains it exists to avoid borrowing `&db` simultaneously with the mutable draw borrow — but since the body doesn't touch `self`, that justification doesn't hold.
- **Fix:** Inline `&passage.translation` at the call site
  (`main.rs:542`) and delete the method. The comment about the
  borrowing constraint can move to the inlined call site as a one-line
  comment.
- **Rationale:** Wrapper methods that don't do anything are
  Chesterton's-fence bait — the next reviewer will spend a minute
  wondering whether the wrapper protects an invariant before realising
  it doesn't.

### 5. `Bookmark` has two equality definitions

- **Location:** `src/bookmark.rs:17` (derives `PartialEq, Eq, Hash`) and
  `src/bookmark.rs:36` (`same_range` method).
- **Problem:** The derive includes `label` and `created_at`. `same_range`
  is the position-only equality. The only writer (`BookmarkStore::add`,
  line 96) uses `same_range`; the only equality test that uses the
  derived `PartialEq` is the round-trip test at line 213. So today the
  two coexist peacefully — but if `iter().any(|b| b == &bm)` ever
  appears (and it will, the derive invites it), the dedup will leak
  past a `created_at` difference.
- **Fix:** Either (a) drop `PartialEq, Eq, Hash` from the derive and
  make round-trip tests use field-by-field comparison; or (b) implement
  `PartialEq` manually with the `same_range` semantics and remove the
  named method. (b) is preferred — there's exactly one notion of
  "two bookmarks point at the same range" and the trait should carry it.
- **Rationale:** Two equalities on the same type is a foot-gun that
  pays off later.

### 6. `render_entry_cell` has a dead parameter and 10 arguments

- **Location:** `src/ui/splash.rs:716-771`
- **Problem:** `dim_cursor: Style` is explicitly discarded inside the
  function (`let _ = dim_cursor;` at line 735) — the comment says "no
  ghost cursor on the unfocused column" but the parameter is still
  required at the call site. Beyond that, ten positional arguments is
  past the readability threshold; the call sites at lines 597-620 have
  to count commas.
- **Fix:** Drop `dim_cursor` from the signature + call sites. Then
  bundle the four `Style` parameters (`sel`, `label`, `dim`, `bg`) into
  a small `RenderEntryStyles` struct local to the module.
- **Rationale:** The `#[allow(clippy::too_many_arguments)]` on line 716
  is currently masking the symptom rather than fixing it.

### 7. Per-frame allocations in `draw_frame`

- **Locations:**
  - `src/main.rs:1014` — `make_status` returns a fresh `Vec<Shortcut<'static>>`
    every draw call (~6Hz). Every field is `&'static str`; the
    `Vec`'s content is constant per `bg` variant.
  - `src/main.rs:987` — `mode_tag_for` returns `String` from
    `format!("-- NORMAL · {} --", layout)` etc. The reading-view path
    runs every frame; the strings are tiny but the allocation is
    real.
  - `src/main.rs:971` — `bookmarks_set` allocates a fresh `BTreeSet<i64>`
    every frame and re-scans every bookmark in the store. The set only
    changes when the chapter or bookmark-store mutates.
- **Fix:**
  - `make_status` → either two `static` `Shortcut` arrays (`STATUS_SPLASH`,
    `STATUS_READING`) plus a tiny `match` to mutate the one element
    whose label depends on `show_sidebar`, or build the `Vec` once in
    `LoopState::new` and re-build only when `show_sidebar` toggles.
  - `mode_tag_for` → return `Cow<'static, str>`; the splash/dialog arms
    can be `Cow::Borrowed("-- NORMAL --")` while only the reading
    arm allocates.
  - `bookmarks_set` → cache it on `LoopState` and invalidate from
    `add_bookmark` / `Delete` / `jump_to` (chapter change). Or, since
    the only reader is `draw_frame` and the chapter-comparison cost
    is tiny per bookmark, leave it and add a comment that the cache
    is intentional churn given the modest bookmark count.
- **Rationale:** Not a hot path yet, but the project's "Turbo Vision
  feel" hinges on the draw never stuttering — a flame graph at first
  paint would already implicate these three. Cheaper to fix now than
  to retrofit after `make_status` gains state.

### 8. `Db::open_ro` accepts an empty-string translation sentinel

- **Location:** `src/db.rs:168` + `src/main.rs:400` (`probe`) +
  `src/main.rs:251` (real open).
- **Problem:** `resolve_translation` opens a "probe" `Db` with an
  empty translation just to call `list_translations()`. The empty
  string never matches any row, so any call to `translation_label` /
  `list_books` / `load_passage` on the probe would silently return
  zero rows. The previous review flagged this and it's still there.
- **Fix:** Split into two constructors:
  ```rust
  impl Db {
      /// Open RO without an active translation. Only translation-list
      /// queries (`list_translations`) are valid on the returned handle.
      pub fn open_probe(path: &Path) -> Result<DbProbe> { ... }
  }
  pub struct DbProbe { conn: Connection }
  impl DbProbe {
      pub fn list_translations(&self) -> Result<Vec<TranslationInfo>> { ... }
      pub fn into_db(self, code: &str) -> Db { ... }
  }
  ```
  Or keep one type but make `open_ro` require a non-empty translation
  and add `pub fn list_translations(path: &Path)` as a free function.
- **Rationale:** Sentinels rot. Today the probe handle is dropped before
  any wrong method is called on it, but a future refactor that hangs
  onto it will silently return empty results — the worst kind of bug.

## Nice to have

- **Crate-level rustdoc**: `src/main.rs:1` has `#![deny(unsafe_code)]`
  and modules but no `//!` doc explaining what the binary is. For a
  binary crate the README is the user-facing docs, but a one-paragraph
  `//!` block helps anyone reading via `cargo doc`.
- **`# Errors` rustdoc sections** on the ~15 public `fn -> Result<...>`
  in `db.rs`, `paths.rs`, `bookmark.rs`, `config.rs`, `state.rs`,
  `search.rs`, `quote.rs`. None today. `clippy::missing_errors_doc`
  fires under pedantic; the doc-with-context exercise tends to surface
  "wait, this can fail in three ways" realizations.
- **`#[must_use]` on `Position::same_chapter`, `Bookmark::matches_chapter`,
  `Bookmark::same_range`, `Bookmark::reference_label`, `Book::display_name`,
  `Db::translation_label`, `Db::list_books`, `Db::list_translations`**.
  All are pure queries whose return value is the whole point;
  accidentally dropping it is a bug.
- **`#[non_exhaustive]` on the public outcome enums** (`SplashOutcome`,
  `FindOutcome`, `GotoOutcome`, `BookmarksOutcome`, `TranslationsOutcome`,
  `FootnoteOutcome`, `HelpOutcome`). They live in dialogs and grow
  whenever a dialog gains a new outcome; without `#[non_exhaustive]`
  every dispatch site silently breaks on the new variant. Internal
  to the binary so the protection is mostly for the next contributor,
  but matches the same hygiene as the rest of the cleanup.
- **`unused fields` lurking under `#[expect(dead_code)]`**: `db.rs:72-76`
  marks `TranslationInfo::license` as roadmap material. Once the
  Translations picker grows a details panel, drop the expect; until
  then it's accurate. Worth a quick audit that no other roadmap
  markers have drifted out of date.
- **A `Cargo.toml` `repository` field** would help `cargo audit` and
  `cargo-deny` policy queries; the metadata block has `keywords`,
  `categories`, `license`, but no `repository`. `publish = false` so
  the crate isn't going to crates.io, but the field is consumed by
  multiple downstream tools.
- **`Bookmark::add` is `O(n)` per add via `iter().any(|b| b.same_range(...))`**.
  Trivial today (10s of bookmarks at most), but a future "import
  bookmarks from a study Bible" flow could blow up. A `HashSet<RangeKey>`
  alongside `bookmarks: Vec<Bookmark>` keeps the ordering and adds
  `O(1)` dedup. Not worth fixing until it bites.
- **`xref_rows: Vec<(String, Xref)>` in `Db::load_footnotes:339`** is
  collected up-front then iterated to attach refs to footnotes via
  `find`. With many footnotes this is `O(n × m)`. For sane chapter
  sizes it doesn't matter; a `HashMap<String, Vec<Xref>>` would be
  `O(n + m)`. Note for the day someone profiles `load_passage`.
- **Test naming**: most tests describe behaviour well, but
  `dump_default_config` at `config.rs:503` is an `eprintln` smoke
  test, not an assertion (asserts only that `[theme]`/`[reading]`/`[keys]`
  appear). Either delete it or move the eprintln to a doc example
  where it serves as living documentation.
- **`SplashView` exposes 11 `pub` fields** (`filter`, `focus`, `cursor_ot`,
  `cursor_nt`, `on_continue`, `translation_name`, `translation_code`,
  `mode`, `quote`). The test suite in the same file is the only
  external mutator; production code only constructs via `new` and
  calls `handle`/`render`. Most could be `pub(crate)`, several could
  be private. Same playbook as last round's `Db.translation` fix.
- **`scripts/baseline.fish` is still broken** (fish-3 redirection
  rules — `if $cmd > file` fails because `$cmd` is an array). The
  bash equivalent works. The skill bundles both; flagged again for
  the skill owner.

## Patches

### Drop the no-op wrapper

```rust
// src/main.rs:628-637 — delete
impl LoopState {
    fn bookmarks_translation<'a>(&self, passage: &'a Passage) -> &'a str {
        &passage.translation
    }
}

// src/main.rs:542 — inline
-        state.bookmarks_translation(passage),
+        &passage.translation,
```

### Fix the `statusbar.rs` eager call

```rust
// src/ui/statusbar.rs:22 — before
.bg(theme::menubar_style().bg.unwrap_or(theme::light_grey()))

// after
.bg(theme::menubar_style().bg.unwrap_or_else(theme::light_grey))
```

### Hoist `Row` in `help.rs::render`

```rust
// src/ui/help.rs — before (4 clippy::items_after_statements warnings)
pub fn render(&self, outer: Rect, buf: &mut Buffer) {
    // ... 20 lines of style setup ...
    enum Row { Section(&'static str), Entry(&'static str, &'static str) }
    use Row::{Entry, Section};
    let rows: &[Row] = &[ ... ];

// after
enum Row { Section(&'static str), Entry(&'static str, &'static str) }

impl HelpDialog {
    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        use Row::{Entry, Section};
        let rows: &[Row] = &[ ... ];
        // ... 20 lines of style setup ...
```

(Move the enum above the `impl` block.)

## Follow-up checklist

One commit per item, in priority order.

- [ ] 1. Drop `LoopState::bookmarks_translation`; inline at call site. (§4)
- [ ] 2. Pick one `Bookmark` equality. Remove either the derive or the
  `same_range` method. (§5)
- [ ] 3. Drop the unused `dim_cursor` parameter from `render_entry_cell`
  and bundle the remaining four `Style`s into a `RenderEntryStyles`. (§6)
- [ ] 4. Mechanical pedantic sweep: `or_fun_call` (statusbar.rs),
  `option_if_let_else` (goto.rs), `equatable_if_let` (main.rs),
  `useless_let_if_seq` (splash.rs), `match_same_arms` (3 sites),
  `items_after_statements` (help.rs). Add module-level
  `#[allow(clippy::similar_names)]` on `splash.rs`. (§3)
- [ ] 5. Split `Db::open_ro` into a probe path + a translation-aware
  open. (§8)
- [ ] 6. Refactor `dispatch_reading` into per-action methods on
  `LoopState`. (§1)
- [ ] 7. Refactor `SplashView` to share `ListNav` (or extract the chord
  + count machinery). (§2)
- [ ] 8. Refactor `splash::render` (262 lines) into a handful of helper
  fns. (§3)
- [ ] 9. Per-frame allocations: hoist `make_status` to `&'static`,
  switch `mode_tag_for` to `Cow<'static, str>`, cache `bookmarks_set`
  on `LoopState`. (§7)
- [ ] 10. Add crate-level `//!` doc to `main.rs`. Add `# Errors`
  sections to the ~15 public `Result`-returning fns. Sprinkle
  `#[must_use]` on the pure queries. Add `#[non_exhaustive]` on the
  six dialog-outcome enums. (Nice-to-have cluster)
- [ ] 11. Add `repository = "..."` to `Cargo.toml`. (Nice-to-have)

## Coverage self-assessment

| Dimension | Confidence | Notes |
|---|---|---|
| Compiler and lint cleanliness | high | All 25 pedantic warnings read end-to-end and triaged. clippy default is clean. |
| API design | high | This is a binary; rubric reweighted toward internal surface hygiene. Two real findings (§4 wrapper, §6 dead parameter) plus the `SplashView` field-visibility note. |
| Error handling | high | All `unwrap`/`expect` outside `cfg(test)` re-checked. `theme::init`'s `expect` and the three `c.to_digit(10).unwrap_or(0)` sites are the only ones, all guarded. `# Errors` docs missing — flagged. |
| Ownership and borrowing | medium | No obvious egregious clones. Per-frame allocation hotspots in §7. Did not profile. |
| Unsafe code | high | `#![deny(unsafe_code)]` confirmed at `main.rs:1`. |
| Concurrency | high | Single-threaded, no async. `OnceLock` for the theme is the only shared state. |
| Testing | high | 71 unit + 5 e2e tests; proptest on the parser; PTY e2e covers translation swap, migration, find-result jump, goto-with-verse. Still no fuzz target for `build_query` / `parse_osis` — would be the next layer. |
| Documentation | medium | README + USAGE.md + CHANGELOG.md + CONTRIBUTING.md all current. Crate-level `//!` and `# Errors` sections still missing. |
| Project structure | high | Modules well-segregated; `text.rs` + `paths.rs` extracted last round and held up. Only `splash.rs` (936 lines) and `dispatch_reading` are outliers. |
| Dependencies and toolchain | high | `cargo audit` clean. MSRV declared, toolchain pinned. Dupes inherited from `rexpect`/`ratatui`. `repository` missing from Cargo.toml — flagged. |
| Performance | medium | Three per-frame allocation sites identified by inspection (§7); not profiled. No obvious quadratics in user-facing paths. |
| Contributor experience | high | `just` recipes match CI 1:1, `BACKLOG.md` is concrete, `CONTRIBUTING.md` covers the test-skip semantics. The fish baseline script is still broken (skill bug, not crate bug). |
