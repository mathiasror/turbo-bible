# Rust Review: turbo-bible

_Generated 2026-05-20. Baseline logs in `target/rust-review/`._

Scope: a single binary crate (`publish = false`), Rust 2024 edition, MSRV
1.88, ~3.4k lines of source plus ~3.6k lines of tests. No `unsafe` code.
The CI gate (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test
--all-features`, and `cargo audit`) is already enforced.

Baseline ground truth:
- `cargo build --all-targets --all-features`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo clippy -W clippy::pedantic -W clippy::nursery`: **102 warnings**
  (94 from the bin + 8 unique to tests). Triaged below.
- `cargo doc --no-deps --all-features`: clean.
- `cargo test --no-run`: clean.
- `cargo audit`: 0 advisories.
- `cargo tree -d`: several known transitive duplicates (see §10).

## Executive summary

- The binary is in solid shape: zero `unsafe`, clean clippy, clean audit,
  good test coverage including proptest on the reference parser and PTY
  e2e for state migrations. CI runs the same gate contributors run
  locally — exemplary.
- The largest blocker for "release readiness" is `run()` in
  `src/main.rs:314` weighing in at **402 lines** with deeply nested
  dialog/bg routing. It works but is hard to extend and review.
- `switch_translation` (`src/main.rs:1032`) mutates `db.translation`
  before the follow-up calls that depend on it; if any of those fail the
  reader is left holding a `Db` pointing at one translation while the
  in-memory `books`/`passage`/`label` describe another. This is a real
  inconsistency window, not just clippy noise.
- `History::push` (`src/main.rs:83`) is unbounded — a multi-hour
  reading session walking chapters will grow the stack forever. Trivial
  bound to add.
- There is duplication that's worth removing now while the API surface
  is still small: `word_wrap` exists in both `render.rs:233` and
  `splash.rs:688`; `config_dir()` is reimplemented in three modules
  (`config.rs`, `state.rs`, `bookmark.rs`).
- 102 pedantic/nursery warnings cluster into a small number of
  recurring patterns (`map(_).unwrap_or(_)` → `map_or`, `as` casts that
  should be `try_from`, function-too-many-lines, identical match arms).
  None are alone a blocker, but they're worth taking in one sweep to
  shrink the noise floor before adopting them as CI gates.

## Blockers

### 1. `switch_translation` mutates `Db` before fallible follow-ups

- **Location:** `src/main.rs:1032-1052`
- **Problem:** `db.translation = code.to_string();` happens before
  `db.list_books()?`, `db.translation_label()?`, and `db.load_passage(...)?`.
  If any of those fail (e.g. a translation row missing labels or verses
  for the requested book/chapter), `db.translation` is left pointing at
  the new code while `books`, `translation_label`, `passage`, and
  `cursor_verse` still describe the old translation. The reader is then
  out of sync with itself and the persisted state at quit reflects the
  partially-swapped world.
- **Fix:** Either (a) compute everything against a candidate then commit
  atomically:

  ```rust
  let original = std::mem::replace(&mut db.translation, code.to_string());
  let do_swap = || -> Result<_> {
      let new_books = db.list_books()?;
      let new_label = db.translation_label()?;
      let new_passage = db.load_passage(&pos.book, pos.chapter)?;
      Ok((new_books, new_label, new_passage))
  };
  match do_swap() {
      Ok((b, l, p)) => { *books = b; *translation_label = l; *passage = p; }
      Err(e) => { db.translation = original; return Err(e); }
  }
  ```

  Or (b) treat `Db` as immutable in its translation: build a fresh `Db`
  for the new translation and swap. The second is cleaner long-term but
  forces the caller to plumb the new handle through.
- **Rationale:** A failed translation switch should leave the reader in
  the pre-switch state, not in a half-swapped state that the next save
  will persist as corrupt.

### 2. `run()` is 402 lines with deeply nested match/match/match

- **Location:** `src/main.rs:314-741`
- **Problem:** `clippy::too_many_lines` (402/100). The function holds the
  draw closure, the event poll loop, all dialog routing, all background
  routing, and all action dispatch in a single body. Every match arm
  reaches into the same locals (`bg`, `dialog`, `pos`, `passage`,
  `cursor_verse`, `history`, `bookmarks`, `last_label_for_splash`,
  `warnings`, `visual_anchor`). Adding a new dialog means editing five
  places.
- **Fix:** Lift each `Dialog::*` arm into a method on a small `App`
  struct that owns the mutable state. The shape would be:

  ```rust
  struct App<'a> { /* the fields currently passed around in run() */ }
  impl App<'_> {
      fn handle_goto(&mut self, ev: GotoOutcome) -> Result<DialogTransition> { ... }
      fn handle_find(&mut self, ev: FindOutcome) -> Result<DialogTransition> { ... }
      // ... one per dialog
      fn dispatch_action(&mut self, action: Action) -> Result<Loop> { ... }
  }
  enum DialogTransition { Close, KeepOpen, Replace(Dialog) }
  enum Loop { Continue, Exit }
  ```

  This shrinks `run()` to "poll → route → draw" and makes each handler
  individually testable.
- **Rationale:** The function is already at the boundary where reviewers
  miss subtle changes (the dialog-side `update_splash_label` calls are
  identical across six arms — easy place to forget one). Refactoring
  before adding more dialogs (the BACKLOG.md `import` subcommand will
  add at least one) is materially cheaper than after.

### 3. `History` stack grows without bound

- **Location:** `src/main.rs:71-104`
- **Problem:** `History::push` only `truncate`s entries ahead of the
  cursor on a new push; entries behind the cursor accumulate forever. A
  long reading session that walks `]b`/`]b`/`]b` and back over hours
  will retain every visited chapter.
- **Fix:** Cap the stack at e.g. 100 entries and drop from the front
  when pushing past the cap. Adjust `cur` accordingly:

  ```rust
  const HISTORY_CAP: usize = 100;
  fn push(&mut self, p: Position) {
      self.stack.truncate(self.cur + 1);
      if self.stack.last().is_none_or(|last| !last.same_chapter(&p)) {
          self.stack.push(p);
          if self.stack.len() > HISTORY_CAP {
              let drop = self.stack.len() - HISTORY_CAP;
              self.stack.drain(..drop);
              self.cur = self.cur.saturating_sub(drop);
          } else {
              self.cur = self.stack.len() - 1;
          }
      }
  }
  ```

- **Rationale:** Trivial to bound; the user-visible behavior of `Ctrl-O`/`Ctrl-I`
  is unchanged for any session shorter than 100 jumps. Today the only
  thing that holds the leak in check is closing the program.

## Strong recommendations

### 1. Eliminate the inconsistency window in `theme::init`

- **Location:** `src/theme.rs:14-20`
- **Problem:** `init()` ignores the `Err` from `OnceLock::set`. If init
  is called twice (e.g. a future test setup), the second call is
  silently dropped and the theme remains whatever it was first set to.
  Worse: any code that calls a theme accessor *before* `init` runs (eg.
  via lazy initialization in `theme()`) will lock in the default palette
  forever, then `init()` becomes a no-op.
- **Fix:** Either pre-condition the init at startup with an `expect` so
  double-init is loud, or expose a `set_or_log` that warns. For a
  binary-only crate, `expect("theme initialized twice")` is fine.
- **Rationale:** The fire-and-forget `let _ = ...` is a known-loud
  failure mode silenced. With `OnceLock` plus a single call site this
  isn't biting today, but it's a "Chesterton's fence" for the next
  contributor.

### 2. Deduplicate `word_wrap` and `config_dir`

- **Locations:**
  - `src/render.rs:233-256` and `src/ui/splash.rs:688-713` — same
    greedy word-wrap implementation, byte-for-byte except for the
    panic-on-empty case in `splash.rs`. They will drift.
  - `src/config.rs:336-341`, `src/state.rs:42-47`,
    `src/bookmark.rs:118-123` — three copies of the same XDG
    config-dir resolution. If `etcetera`'s strategy ever changes (or
    we want to honor `$TURBO_BIBLE_CONFIG_DIR` for tests), it has to
    change in three places.
- **Fix:** `word_wrap` belongs in a `src/text.rs` (or top of
  `render.rs`, re-exported). `config_dir`/`data_dir` belong on a
  single `src/paths.rs` that owns XDG resolution and the
  `turbo-bible` subdirectory join. Make them `pub(crate)` and import
  from the call sites.
- **Rationale:** Three copies is the threshold where a small abstraction
  is unambiguously cheaper than the duplication.

### 3. Clippy pedantic clean-up — high-value pass

The 102 pedantic/nursery warnings cluster as follows. Take the bulk in
one mechanical sweep; the others can become CI gates afterward.

| Pattern | Count | Auto-fix? |
|---|---|---|
| `map(f).unwrap_or(v)` → `map_or(v, f)` | 14 | yes |
| `as` cast that could truncate | 14 | partial |
| `this could be a const fn` | 17 | yes |
| `format!("{}", x)` → `format!("{x}")` | 5 | yes |
| `unnecessary structure name repetition` | 5 | yes |
| `binding's name too similar` | 5 | rename |
| `identical match arms` | 8 | merge |
| `unnested or-patterns` (`Foo \| Bar` in let) | 4 | yes |
| `function has too many lines` | 6 | refactor |
| `unused self argument` | 2 | drop |
| `clippy::cast_possible_truncation` for screen widths | several | usually safe |

Suggestion: take the first eight rows as one PR (`cargo clippy --fix
... --allow-dirty -- -W clippy::pedantic` produces 55 autofixes), then
bump `justfile`'s `lint` to include `clippy::pedantic` selectively
with allowlist entries for the noisy ones. The remaining
`cast_possible_truncation` warnings are nearly all on `chars().count()
as u16` for screen positions; replace with `u16::try_from(...).unwrap_or(u16::MAX)`
or accept the lint with a per-call `#[allow]` + justification — silent
truncation here would only manifest on a >65k-column terminal, which
warrants a comment, not paranoia code.

### 4. Truncation casts: a real one and many cosmetic ones

- **Real one:** `src/ui/splash.rs:842,852` cast `usize` → `i64` to set
  `ord` on a fake-book builder used only in tests. Safe in practice but
  flagged by `cast_possible_wrap`. Use `i64::try_from(i).unwrap()`.
- **Cosmetic:** `src/main.rs:917,921` cast a count `u16` → `i64`. Use
  `i64::from(n)` — infallible, clippy-clean.
- **Slightly risky:** `c.to_digit(10).unwrap() as u16` at
  `src/keys.rs:159`, `src/ui/listnav.rs:43`, `src/ui/splash.rs:262`.
  `c.is_ascii_digit()` is checked just above, so `.unwrap()` can't
  panic, and `to_digit(10)` returns at most 9 so the `as u16` can't
  truncate. Still: prefer `u16::from(c.to_digit(10).unwrap() as u8)` or
  rewrite as `c as u32 - '0' as u32` → `try_from` to avoid the
  brittleness invariant — one rename elsewhere and an audit becomes
  necessary.
- **Rationale:** Casts that *can't* truncate today are still landmines
  when a contributor changes a nearby type. Make them obviously
  infallible.

### 5. Silent error swallowing in user-visible paths

- **Locations:**
  - `src/main.rs:629` — `let _ = copy_verse_to_clipboard(...)`. The
    user pressed `y` expecting clipboard population; if the clipboard
    backend fails (Wayland without `wl-clipboard`, headless SSH
    session, etc.) they get no feedback.
  - `src/config.rs:349-359` — `load()` returns `Config::default()` on
    *any* error from `config_path()` or `fs::read_to_string`. The toml
    parse error is reported via `eprintln!`, but a permission-denied
    or other read error vanishes.
  - `src/quote.rs:88` — `lookup(...)` returns `Ok(None)` on row-not-found
    but also on any SQL error (`.ok()` strips the diagnostic).
- **Fix:** Push these into the existing `warnings` collector used in
  `main.rs:141`. For `copy_verse_to_clipboard`, replace `let _ = …`
  with `save_or_warn(warnings, "clipboard", copy_verse_to_clipboard(...))`.
  For `config::load`, distinguish "file not found" (silent default)
  from "found but unreadable" (warn).
- **Rationale:** "Disappeared into the void" is the worst failure mode
  for user-facing state writes. The collector pattern already exists;
  use it consistently.

### 6. `SidebarView::build_lines` takes an unused `width` parameter

- **Location:** `src/ui/sidebar.rs:60` — `_width: u16` is leading-
  underscored to satisfy clippy, but the function takes it on every call
  from `SidebarView::render` (line 48). Either use it (the function
  could pre-truncate parallel-passage labels rather than relying on
  `Wrap { trim: false }`) or drop it.
- **Fix:** Drop the parameter and the call-site argument.
- **Rationale:** Cosmetic, but it's a load-bearing function that's
  pretending to be configurable. Either be configurable or commit.

### 7. Translation reads from `Db::translation` mid-flight rather than
through the search/quote signatures

- **Location:** `src/search.rs:51` `db.translation` and
  `src/quote.rs:84` `db.translation`. The functions take a `&Db`
  parameter; they then read mutable state off it.
- **Fix:** Pass the translation explicitly: `search(db: &Db, translation:
  &str, input: &str, limit: usize)`. This makes the data flow obvious
  and lets callers eventually switch to a `&str` parameter without
  juggling `Db` ownership.
- **Rationale:** The current design works (translation is set on Db at
  startup and on translation-picker confirmation), but the implicit
  read makes `switch_translation`'s atomicity bug (Blocker #1) harder
  to see.

### 8. Public surface hygiene

- **Location:** Most `pub` declarations in `src/db.rs`, `src/nav.rs`,
  `src/render.rs`, `src/search.rs`.
- **Problem:** This is a binary, but the modules export `pub` types and
  fields where `pub(crate)` would do. `Db::translation: pub String`
  invites the exact mutation bug in Blocker #1 — there's no API
  boundary blocking arbitrary writes.
- **Fix:** Sweep `pub → pub(crate)` and gate mutable field access
  behind methods (e.g. `Db::set_translation(&mut self, code: &str)`).
- **Rationale:** Internal API hygiene catches bugs even in a binary;
  `pub` should mean "I really do intend for external code to depend on
  this," not "I needed a getter and reached for the shortest keyword."

### 9. CHANGELOG.md missing

- **Location:** project root.
- **Problem:** Rubric §8 calls for `CHANGELOG.md`. `Cargo.toml` has
  `version = "0.1.0"`, README documents features comprehensively, but
  there's no per-release diff record.
- **Fix:** Adopt Keep-a-Changelog. Backfill is cheap (the git history
  shows the structure already — bookmarks → translations picker →
  USAGE.md → keymap profile).
- **Rationale:** "Release readiness" is the framing of this review, and
  every release pipeline expects a changelog.

### 10. Dependency duplication

- **Location:** `target/rust-review/tree-dupes.log`.
- **Problem:**
  - `bitflags 1.3.2` (via `nix 0.25` → `rexpect`) alongside
    `bitflags 2.11` (everywhere else). Dev-only, but ageing rexpect's
    transitive `nix` is 4 majors behind.
  - `thiserror 1.0` (via rexpect) + `thiserror 2.0` (via kasuari,
    ratatui-core).
  - `hashbrown 0.14`, `0.16`, `0.17` and `getrandom 0.3`, `0.4`
    simultaneously.
  - `quick-error 1.2` + `2.0`.
- **Fix:** Most are upstream transitive — track but don't fight. The
  one worth a try is bumping `rexpect` to its latest 0.6.x line (if
  out) to deduplicate `nix`/`bitflags` v1.
- **Rationale:** Affects binary size and compile time more than
  correctness. Worth listing in the changelog when upstream lets you
  fix it.

## Nice to have

- `#![deny(unsafe_code)]` at crate root. Free protection given the crate
  has zero `unsafe`.
- `#![warn(missing_docs)]` once the crate stabilises — public items in
  `db.rs` have no rustdoc beyond field comments.
- `BACKLOG.md` notes a planned `turbo-bible import` subcommand; when
  that lands, take the opportunity to introduce a `Subcommand` enum and
  drop the implicit "no subcommand → run" pattern that today's
  `clap::Parser` derives.
- `init_terminal`/`restore_terminal` (`src/main.rs:1060-1076`) should be
  paired via RAII so a panic between them still leaves the terminal
  sane. Today a panic in the draw closure won't run `restore_terminal`,
  and the user is left with a corrupted terminal. A `TerminalGuard {
  fn drop(&mut self) { disable_raw_mode(); leave_alternate_screen(); } }`
  closes that gap.
- The `extras.push(...)` loop in `KeyState::with_user_bindings`
  (`src/keys.rs:92`) is `O(n)` per keypress in the `for (binding,
  action) in &self.extras` lookup. With ~20 bindings × ~6 keys/sec it's
  irrelevant; if it ever matters, a `HashMap<KeyBind, Action>` (with a
  fixup for the SHIFT-normalisation in `KeyBind::matches`) is the next
  step.
- `scripts/baseline.fish` (the rust-review baseline runner) has a fish
  redirection bug: `if $cmd > $logfile` fails because `$cmd` is an
  array. Use `eval $cmd` or `command $cmd[1] $cmd[2..-1]`. Not a bug in
  the crate, but the rust-review skill bundles it.
- The `KeyState::tick` invocation inside `KeyState::handle` then again
  in the outer loop (`src/main.rs:738`) is correct but redundant. The
  first call clears expired chord buffer before processing the new key;
  the outer call clears it when the poll times out. Document one or
  remove one.
- `Db::open_ro` accepts an empty `translation` string from `resolve_translation`
  (`main.rs:302`) so the probe can list translations. Make the probe a
  separate `Db::open_probe` that doesn't pretend to hold a translation —
  the empty-string sentinel works, but it's the kind of state that
  causes a silent bug later (e.g. someone calls `translation_label`
  before resolving).

## Patches

Top three patches to apply directly.

### `switch_translation` becomes atomic

```rust
// src/main.rs:1032 — before
fn switch_translation(
    db: &mut Db,
    books: &mut Vec<Book>,
    translation_label: &mut String,
    code: &str,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
) -> Result<()> {
    db.translation = code.to_string();
    *books = db.list_books()?;
    *translation_label = db.translation_label()?;
    *passage = db.load_passage(&pos.book, pos.chapter)?;
    let max = passage.verses.last().map(|v| v.number).unwrap_or(1);
    if *cursor_verse > max {
        *cursor_verse = max.max(1);
    }
    Ok(())
}

// after
fn switch_translation(
    db: &mut Db,
    books: &mut Vec<Book>,
    translation_label: &mut String,
    code: &str,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
) -> Result<()> {
    let prev = std::mem::replace(&mut db.translation, code.to_string());
    let result = (|| -> Result<(Vec<Book>, String, Passage)> {
        Ok((
            db.list_books()?,
            db.translation_label()?,
            db.load_passage(&pos.book, pos.chapter)?,
        ))
    })();
    match result {
        Ok((new_books, new_label, new_passage)) => {
            *books = new_books;
            *translation_label = new_label;
            *passage = new_passage;
            let max = passage.verses.last().map_or(1, |v| v.number);
            if *cursor_verse > max { *cursor_verse = max.max(1); }
            Ok(())
        }
        Err(e) => {
            db.translation = prev;
            Err(e)
        }
    }
}
```

### Bound `History`

```rust
// src/main.rs:71 — before
struct History { stack: Vec<Position>, cur: usize }
impl History {
    fn push(&mut self, p: Position) {
        self.stack.truncate(self.cur + 1);
        if self.stack.last().is_none_or(|last| !last.same_chapter(&p)) {
            self.stack.push(p);
            self.cur = self.stack.len() - 1;
        }
    }
    ...
}

// after
const HISTORY_CAP: usize = 100;
struct History { stack: Vec<Position>, cur: usize }
impl History {
    fn push(&mut self, p: Position) {
        self.stack.truncate(self.cur + 1);
        if self.stack.last().is_none_or(|last| !last.same_chapter(&p)) {
            self.stack.push(p);
            if self.stack.len() > HISTORY_CAP {
                let drop = self.stack.len() - HISTORY_CAP;
                self.stack.drain(..drop);
                self.cur = self.stack.len().saturating_sub(1);
            } else {
                self.cur = self.stack.len() - 1;
            }
        }
    }
    ...
}
```

### Surface clipboard failures

```rust
// src/main.rs:628 — before
Action::CopyVerse => {
    let _ = copy_verse_to_clipboard(passage, pos, *cursor_verse);
}

// after
Action::CopyVerse => {
    save_or_warn(
        warnings,
        "clipboard set",
        copy_verse_to_clipboard(passage, pos, *cursor_verse),
    );
}
```

## Follow-up checklist

One commit per item, in priority order.

- [ ] 1. Make `switch_translation` atomic — restore `db.translation` on
  any inner-call failure. (Blocker #1)
- [ ] 2. Bound `History` at 100 entries. (Blocker #3)
- [ ] 3. Extract `App` struct from `run()`; lift each dialog arm into a
  `handle_*` method. (Blocker #2; bigger commit — split into "extract
  App" first, then "lift dialog arms" if it gets unwieldy.)
- [ ] 4. Pipe `copy_verse_to_clipboard` errors through `save_or_warn`,
  and distinguish "file missing" vs "file unreadable" in `config::load`.
  (§5)
- [ ] 5. Deduplicate `word_wrap` (move to `src/text.rs` or top of
  `render.rs`) and `config_dir`/`data_dir` (new `src/paths.rs`). (§2)
- [ ] 6. Drop the `_width: u16` parameter from `sidebar::build_lines`
  and the corresponding call site. (§6)
- [ ] 7. Tighten public surface: `pub Db.translation` → method-gated,
  `Db::set_translation(&mut self, code: &str)`; sweep `pub →
  pub(crate)` across `db.rs`, `nav.rs`, `render.rs`, `search.rs`. (§8)
- [ ] 8. Pass translation explicitly to `search()` / `quote::pick()`
  instead of reading `db.translation`. (§7)
- [ ] 9. Auto-apply the safe clippy pedantic bulk: `cargo clippy --fix
  --bin turbo-bible --allow-dirty -- -W clippy::pedantic` then review
  the diff. Add an `#[allow(...)]` cluster at the crate root for the
  warnings you want to permanently silence (with one-liner
  justifications). (§3)
- [ ] 10. Replace `as i64`/`as u16` casts on values that don't actually
  truncate with `i64::from(...)` / `u16::try_from(...).unwrap_or(...)`
  to make infallibility obvious. (§4)
- [ ] 11. Replace the `init_terminal`/`restore_terminal` pair with an
  RAII `TerminalGuard`. (Nice-to-have)
- [ ] 12. Add `#![deny(unsafe_code)]` and `CHANGELOG.md`. (§9 + nice-to-have)
- [ ] 13. `theme::init` — replace `let _ = THEME.set(t);` with an
  `expect("theme initialized twice")` so the second-init panic is
  loud. (§1)

## Coverage self-assessment

| Dimension | Confidence | Notes |
|---|---|---|
| Compiler and lint cleanliness | high | clippy logs read end-to-end; 102 pedantic warnings triaged into categories. |
| API design | medium | This is a binary, so the API-guideline rubric (newtypes, sealed traits, `From`/`TryFrom`) carries less weight; I focused on internal surface hygiene. |
| Error handling | high | All `unwrap`/`expect` outside `cfg(test)` checked. The three `.unwrap()` calls on `c.to_digit(10)` are guarded by `is_ascii_digit()` and safe; called out for brittleness. |
| Ownership and borrowing | medium | The crate avoids the common `Arc<Mutex<_>>` antipatterns. `BookmarkStore::add` clones; `Frame` borrows everything. No obvious wasteful allocation hotspots; perf is not exercised. |
| Unsafe code | high | Confirmed zero `unsafe` in `src/`. |
| Concurrency | high | Single-threaded, no async. The 150ms event poll with `tick()`-based timeout is well-suited. |
| Testing | high | Unit tests next to code, proptest on the parser, PTY e2e on state migrations. The PTY tests skip cleanly without a populated DB. No fuzz targets for FTS5 query building or OSIS parsing — would be the next layer. |
| Documentation | high | Module-level docstrings throughout. `README.md` and `docs/USAGE.md` are excellent. Missing: `CHANGELOG.md`, rustdoc on most `pub` items in `db.rs`. |
| Project structure | high | One-module-per-concern, `lib.rs`-style. `run()` is the one outlier (Blocker #2). |
| Dependencies and toolchain | high | MSRV declared, toolchain pinned to `stable`, audit clean. Dupes are inherited from rexpect/ratatui and not actionable directly. |
| Performance | medium | Not profiled. Visible candidates flagged (allocating `make_status` on every draw, `make_status` is fine since it runs ~6Hz). No obvious quadratic loops. |
| Contributor experience | high | `just` + CI parity + `BACKLOG.md` + `CONTRIBUTING.md` is the gold-standard setup. The `scripts/baseline.fish` redirection bug in the rust-review skill itself bit me here, but that's not on the crate. |
