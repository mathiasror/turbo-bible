# Rust Review: turbo-bible (round 4)

_Generated 2026-05-22. Baseline logs in `target/rust-review/`._

Fourth-pass review. The working tree on top of `f09b51a` is a single,
focused reading-view redesign:

- Drop full-row cyan cursor highlight → 1-col gutter glyph (`▸` / `▎` /
  `★`) + brighter prose body.
- Drop the two-line / single-line toggle and its bindings
  (`Shift-T`, `[reading] two_line_verses`, `[keys] toggle_verse_layout`,
  `Action::ToggleVerseLayout`).
- Drop the in-body chapter banner (the border title carries it now).
- Demote frame border from `Double` (`═`) to `Plain` (`─`).
- Trim the reading shortcut bar from 9 → 8 items (`T Layout` removed).

The round-3 follow-up checklist did **not** land in this round. Every
item there is still open; this round's diff only touches reading-view
files. I am not re-litigating the round-3 findings here — they remain
valid as-is and are summarised in §"Round-3 carry-over" at the end of
this document for context.

Baseline ground truth (re-run today):
- `cargo build --all-targets --all-features`: clean.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo clippy -W clippy::pedantic -W clippy::nursery`: **21 warnings**
  (20 bin + 1 test). Exactly the same set as round 3.
- `cargo doc --no-deps --all-features`: clean.
- `cargo test --all-features --no-run`: builds.
- `cargo audit`: 0 advisories.
- `cargo udeps`: clean (`All deps seem to have been used`).
- `cargo tree -d`: same transitive dupes as round 3 (`bitflags 1/2` via
  `rexpect`, `hashbrown` x3, `getrandom` x3, `thiserror` x2).

## Executive summary

- **One real blocker this round** (§1): a user who has `two_line_verses`
  or `toggle_verse_layout` in their existing `config.toml` doesn't lose
  *those keys* on next launch — they lose **every customisation**. The
  combination of `deny_unknown_fields` + `config::load`'s
  `unwrap_or_else(Config::default)` means the whole config silently
  reverts to defaults (theme, default_translation, every custom binding)
  with one stderr line that pre-TUI users won't necessarily catch. The
  CHANGELOG notes it as breaking but offers no migration; the in-code
  behaviour is "rebuild your config from scratch."
- The reading-view redesign is otherwise clean. `render.rs` shrank from
  ~190 to ~150 lines, the cursor row no longer carries a unique bg
  color, and one whole code path (`two_line_verses`) is gone. Three
  small tidy-ups are worth doing while the file is fresh in memory (§2,
  §4, §5).
- One stale doc string in `src/keys.rs:10` still mentions `T` in the
  list of vim-layer keys (§3).
- The round-3 blocker (`turbo-bible import` doesn't populate
  `heading` / `footnote` / `xref`) is **still open**. It's not in scope
  for this UI round but it's the last thing standing between this crate
  and a first release; called out again in §"Round-3 carry-over".

## Blockers

### 1. Removed config keys nuke the rest of the user's config

- **Location:** `src/config.rs:49` (`#[serde(default, deny_unknown_fields)]`
  on `Config`) + `src/config.rs:359` (`toml::from_str(&txt).unwrap_or_else(|e| { eprintln!(...); Config::default() })`)
  + this round's removal of `ReadingConfig::two_line_verses` and
  `KeysConfig::toggle_verse_layout`.
- **Problem:** Today, a user whose `config.toml` looks like

  ```toml
  default_translation = "nb-1930"

  [reading]
  two_line_verses = true
  max_width       = 100

  [theme]
  blue = "#001a4d"

  [keys]
  toggle_verse_layout = ["Ctrl-l"]
  quit                = ["Ctrl-q"]
  ```

  on a `turbo-bible` upgrade gets:
  1. `toml::from_str::<Config>` fails on the first unknown field
     (`two_line_verses` or `toggle_verse_layout`, whichever the parser
     sees first).
  2. `config::load` prints one line — `config.toml: ...; using defaults`
     — to stderr, *before* the alternate-screen handshake. On a TUI
     launch the user normally sees the splash a fraction of a second
     later; the warning is easy to miss especially when launching from
     a terminal multiplexer or a wrapper script.
  3. Every other field in the file is **silently discarded** for the
     session: their `default_translation`, their `max_width`, their
     custom theme, their `quit` rebinding. None of those are bad keys,
     but `deny_unknown_fields` is all-or-nothing.
  4. On normal exit, `main()` calls `config::save(&cfg)` with the
     defaulted `Config` (`src/main.rs:382`) — i.e. it overwrites the
     user's customised file on disk with the default config. The
     legacy keys *and the user's other settings* are gone for good.
  5. The "active translation persisted on quit" path (`src/main.rs:378-383`)
     means the user might at least keep their last translation, but
     theme/keymap/etc. are blown away.
- **Fix:** Three layers, pick at least the first two:
  - **(a) Quarantine the known-renamed keys** before the strict parse.
    A one-off `migrate_legacy(&mut txt)` that strips
    `two_line_verses = ...`, `[reading] two_line_verses`, and the
    `toggle_verse_layout = ...` line, with a warning, before
    `toml::from_str`:
    ```rust
    fn migrate_legacy(txt: &str, warnings: &mut Vec<String>) -> String {
        const LEGACY_KEYS: &[&str] = &["two_line_verses", "toggle_verse_layout"];
        let mut out = String::with_capacity(txt.len());
        for line in txt.lines() {
            let trimmed = line.trim_start();
            if LEGACY_KEYS.iter().any(|k| trimmed.starts_with(k)) {
                warnings.push(format!(
                    "config.toml: dropping removed key `{}` (see CHANGELOG 0.2)",
                    trimmed.split_whitespace().next().unwrap_or(trimmed),
                ));
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }
    ```
    Routed through the same deferred-warning channel as bookmarks /
    state saves so the message survives the TUI session and prints on
    exit.
  - **(b) On parse failure, do NOT auto-overwrite the file.** Today
    `main()` always re-saves on exit. After a parse-failure-and-default
    path, the on-disk file should be left alone so the user can fix
    their own typo. Either gate the `config::save` at L273/L382 on
    "loaded cleanly" (return a richer `LoadResult { config, was_partial }`)
    or, simpler, write a `.bak` of the previous file before overwriting
    when the parse failed.
  - **(c) Loosen `deny_unknown_fields`** to per-section: keep it at the
    top level (catches typoed section names) but use
    `#[serde(default)]` without `deny_unknown_fields` on
    `ReadingConfig`, `KeysConfig`, `ThemeConfig`, `InputConfig`. Trades
    one safety check (typo detection inside a section) for resilience
    across upgrades. With (a) in place, this is the least valuable of
    the three but easiest.
- **Rationale:** The CHANGELOG flags this as breaking, which is good,
  but "breaking" should mean "rejects the now-renamed key" — not
  "rejects the user's entire customisation." The crate already has a
  matching migration path for bookmarks (`bookmarks.json` → `.toml`,
  exercised at `tests/e2e.rs:148-206`); the config layer should match
  the same standard. This is the only case in the codebase where a
  failed parse silently nukes user data.
- **Maps to:** API hygiene around breaking changes; the same principle
  that drives `#[non_exhaustive]` on public enums.

## Strong recommendations

### 2. Reading-view colour ladder loses the cursor-vs-selection distinction inside long ranges

- **Location:** `src/render.rs:52-62`, `src/render.rs:111-137`.
- **Problem:** Before this round, the cursor verse had a unique cyan
  background — distinct from any other verse on screen, including
  those inside a visual selection. After this round, both the cursor
  verse and every verse in a visual selection get the same
  `bright_white` prose foreground; the only distinguishing mark for
  the cursor inside a selection is the 1-column gutter glyph
  (`▸` for cursor vs `▎` for selection). On a long visual selection
  (e.g. `V` + `G` on Psalm 119, 176 verses), the cursor disappears
  into the range visually. The code comment at L52-54 acknowledges
  this trade-off ("the gutter glyph carries the cursor-vs-range
  distinction") but the gutter is one column on the far left of the
  pane — out of the user's eyeline once the prose flows.
- **Fix:** Either (a) keep the selection lighter than the cursor —
  e.g. cursor gets `bright_white`, selection gets a midpoint colour
  (a fourth tier between `light_grey` and `bright_white`), or (b)
  reintroduce a subtle background tint **on the cursor row only** —
  not a full-pane cyan as before, but e.g. a 1-shade darker blue
  (theme adds `cursor_blue = #001a55` or similar) so the cursor row
  reads as the focus regardless of selection length.
- **Rationale:** Taste-level, but visual ranges are a first-class
  feature (`v` is in the status bar). A first-time user pressing
  `V`+`G` and watching the cursor "vanish" will reach for `Esc` more
  than for `j`/`k`. Two-tier (cursor vs selection vs idle) was the
  pre-round-4 promise; this round collapses it to two
  (idle vs active).
- **Maps to:** UX consistency; no specific lint.

### 3. Stale `T` in the `keys.rs` module doc

- **Location:** `src/keys.rs:10`.
- **Problem:**
  ```rust
  //!   * **Vim** — gated by [`Keymap::Vim`]. Letter keys (hjkl, gg/G, n/N, K,
  //!     y, v/V, b, T, M, t, ZZ/ZQ), `:` ex-commands, counts, and chord
  //!     starters (`g`, `[`, `]`, `Z`).
  ```
  `T` is no longer a vim-layer key — the binding was removed in this
  round (`KeyCode::Char('T')` at the old `src/keys.rs:284` is gone).
  Module rustdoc still lists it.
- **Fix:** Drop `T` from the letter-key list. `Action::ToggleVerseLayout`
  was deleted from the enum and from `with_user_bindings`'s `push`
  list (`src/keys.rs:107-126`); only the doc lags.
- **Rationale:** Doc-vs-code drift, exactly the kind of thing
  `cargo doc` doesn't catch.
- **Maps to:** Mechanical fix (doc), no lint.

### 4. `marker_style` is a no-op zero-arg closure

- **Location:** `src/render.rs:63-68`.
- **Problem:** Before this round, `marker_style` took an `on_cursor:
  bool` parameter to swap the bg between `cursor_bg` and
  `theme::blue()`. After the cyan-row removal, the closure body is
  unconditional — it returns the same `Style` regardless of state.
  It's now a zero-arg, zero-capture closure:
  ```rust
  let marker_style = || {
      Style::new()
          .fg(theme::yellow())
          .bg(theme::blue())
          .add_modifier(Modifier::BOLD)
  };
  ```
  Called at `src/render.rs:190`. The closure-call form survives only
  because `verse_num_style` and `verse_text_style` next to it *do*
  take an `on_cursor: bool`.
- **Fix:** Lift it out of the per-call call:
  ```rust
  let marker_style = Style::new()
      .fg(theme::yellow())
      .bg(theme::blue())
      .add_modifier(Modifier::BOLD);
  // …later…
  spans.push(Span::styled(tail.to_string(), marker_style));
  ```
  `Style` is `Copy`, so the let-binding is free to reuse. Drops one
  closure indirection per styled marker and reads more honestly.
- **Rationale:** A closure that captures nothing and returns a
  constant value is a function. Same hygiene as
  `clippy::redundant_closure`.
- **Maps to:** `clippy::redundant_closure` (not directly fired
  because the closure is bound to a let, not passed as an argument).

### 5. `num_str.clone()` per wrapped line in `render_passage`

- **Location:** `src/render.rs:138`, `src/render.rs:173`.
- **Problem:** `num_str` is computed once per verse
  (`format!("{:>width$}  ", ...)` at L138 — a 5-byte allocation in the
  common case) and only used inside the `for (i, chunk)` loop on the
  `i == 0` branch. Today the body of the loop calls
  `Span::styled(num_str.clone(), verse_num_style(on_cursor))` —
  cloning a string that's only consumed once.
- **Fix:** Either build the verse-prefix line *outside* the loop and
  hand it the first chunk as one stitched line:
  ```rust
  // Verse prefix line (only one verse per chapter draws this).
  let (first, rest) = chunks.split_first()...;
  // build the i==0 spans with `num_str` moved in (no clone)
  // then loop over `rest` for the hanging-indent lines.
  ```
  or move the `num_str` into the loop and re-format on demand (cheap
  for a 1–3 digit number). The first form is the cleaner refactor;
  the second is a 1-line change.
- **Rationale:** This is rendered every frame (≈6 Hz). Per-frame the
  cost is `chapter.verses.len()` clones of a tiny string — not hot,
  but Sundays only ever go up. Pair with §4 for one focused render
  cleanup commit.
- **Maps to:** `clippy::redundant_clone` (not currently fired because
  the borrow checker can't prove it). Confirmed by adding
  `clippy::redundant_clone` strictness locally.

## Nice to have

- **`pad_to_width` sets `fg` on a space-only padding span**
  (`src/render.rs:233`). Padding is always `" ".repeat(pad)` — fg has
  no visual effect on a space. Drop the `.fg(theme::bright_white())`
  to communicate the intent ("background fill only"), one less call to
  reason about when reading the function.
- **`Frame::passage: Option<&Passage>` is always `Some` in the
  Reading branch** (`src/ui/mod.rs:28-30`, `src/main.rs:622-633`). The
  `Option` only exists because the splash branch goes through a
  different path entirely (`Bg::Splash`); the `Frame` struct is built
  only for `Bg::Reading`. Drop the `Option` — make it
  `passage: &'a Passage` — and the call site at `src/main.rs:622-633`
  loses one indirection. The `if let Some(p) = self.passage` block at
  `src/ui/mod.rs:42-60` becomes the function body directly.
- **`PassageView::render` clears the inner rect twice**
  (`src/ui/passage.rs:42-48`). `Block::default().style(...)` already
  paints the block's inner area, then the explicit double loop at
  L42-48 overwrites every cell again. Pick one. The explicit loop is
  defensive against ratatui versions that don't honour `.style()` for
  inner cells, but the current pinned version (`ratatui 0.30`) does,
  per the upstream block code. Even if it's kept "just in case," a
  one-line `// belt-and-braces: ratatui's Block.style() doesn't always
  paint the inner cells across versions` comment justifies it.
- **`Bg::Splash(Box<SplashView>)` is fine; consider `Box<SplashView>`
  elsewhere too.** `Dialog` enum (`src/main.rs:75-83`) holds
  six variants of varying size (`HelpDialog` is unit, `BookmarksDialog`
  is large). The largest variant determines `mem::size_of::<Dialog>()`
  on the stack. If this becomes a hot allocation later, box the
  heaviest dialog variants. Not warranted today.
- **`fn is_blank` in `render.rs`** (L212-217) iterates every span and
  every char. The function only sees blank lines built by `rl_blank()`
  (one span containing the empty string), so the check is overkill
  for the only producer. Either restrict the input contract
  (`fn is_blank(rl: &RenderedLine) -> bool { rl.line.spans.iter().all(|s| s.content.is_empty()) }`)
  or leave a comment noting the deliberate conservatism.
- **Module-level `//!` for `src/ui/help.rs`** says "Help dialog (F1) —
  keymap cheat sheet" — accurate, but doesn't surface that the dialog
  is the canonical source of truth for the runtime keymap. Worth a
  one-liner saying "edit this file when you add/remove a key binding,
  otherwise users see stale help."
- **The `Row` enum in `src/ui/help.rs:24-28`** is `Section(&'static
  str)` / `Entry(&'static str, &'static str)`. `tests/help.rs` doesn't
  exist; no test asserts that `T` doesn't appear in the rendered help
  output. A 10-line test that walks the `rows` slice would have caught
  the `T` removal automatically (and would have caught the round-3
  stale references too). Worth adding once the round-3 `T` from
  `src/keys.rs:10` is fixed.

## Patches

### Quarantine legacy config keys (§1, sketch)

```rust
// src/config.rs — before line 338

fn migrate_legacy(txt: &str, warnings: &mut Vec<String>) -> String {
    const LEGACY_KEYS: &[&str] = &["two_line_verses", "toggle_verse_layout"];
    let mut out = String::with_capacity(txt.len());
    for line in txt.lines() {
        let trimmed = line.trim_start();
        if LEGACY_KEYS.iter().any(|k| trimmed.starts_with(k)) {
            warnings.push(format!(
                "config.toml: removed key `{}` ignored (see CHANGELOG 0.2)",
                trimmed.split(|c: char| c.is_whitespace() || c == '=').next().unwrap_or(""),
            ));
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

// Updated signature so the warnings get routed back to main().
pub fn load(warnings: &mut Vec<String>) -> Config {
    // … existing read logic …
    let migrated = migrate_legacy(&txt, warnings);
    toml::from_str(&migrated).unwrap_or_else(|e| {
        warnings.push(format!("config.toml: {e}; using defaults"));
        Config::default()
    })
}
```

`main.rs` then threads `warnings` into `config::load(&mut warnings)`
instead of the current bare `config::load()`. Same channel as
`save_or_warn`. (Alternatively, return `LoadResult { config, warnings,
saved_legacy_keys: bool }` and gate the post-quit `config::save` on
`saved_legacy_keys == false` so legacy text is preserved on disk until
the user removes it.)

### Hoist `marker_style` out of the closure form (§4)

```rust
// src/render.rs — before line 63 → after L36
// (alongside heading_style, which is also pre-computed)
let marker_style = Style::new()
    .fg(theme::yellow())
    .bg(theme::blue())
    .add_modifier(Modifier::BOLD);

// later, at the old L190:
spans.push(Span::styled(tail.to_string(), marker_style));
```

### Drop the stale `T` from the keys module doc (§3)

```rust
// src/keys.rs:10 — before
//!     y, v/V, b, T, M, t, ZZ/ZQ), `:` ex-commands, counts, and chord
// after
//!     y, v/V, b, M, t, ZZ/ZQ), `:` ex-commands, counts, and chord
```

### Drop redundant fg on padding (§Nice-to-have)

```rust
// src/render.rs:233 — before
let pad_style = Style::new().fg(theme::bright_white()).bg(theme::blue());
// after
let pad_style = Style::new().bg(theme::blue());
```

## Round-3 carry-over

For completeness — these are still open. None were addressed in this
round's diff; the 21-warning pedantic count is identical to round 3.

1. **Footnote / heading / xref ingest gap** in `turbo-bible import`
   (round 3 §1). Still the only material release blocker.
2. **`download_source` validates only `len > 0`** (round 3 §2).
3. **`--db /custom/path` ↔ default `backup_dir` mismatch**
   (round 3 §3).
4. **`SCROLLMAPPER_COMMIT` duplicated** between `src/import.rs:25` and
   `tests/import.rs:17` (round 3 §4).
5. **`today_iso()` cast warnings** (round 3 §5).
6. **Mechanical pedantic sweep** (round 3 §6): `doc_markdown`,
   `unused_self` on `help::render` + `copy_verse`,
   `missing_const_for_fn` on `toggle_visual` + `make_status` +
   `tests/import.rs::binary_path`, `needless_pass_by_value` on
   `HistoryDir`, the carry-over `needless_pass_by_ref_mut` `#[allow]`.
7. **Schema round-trip test** (round 3 §7).
8. **`book_label.full_name` always NULL** (round 3 §8).
9. **`deny.toml` still missing**.

## Follow-up checklist

One commit per item, in priority order. Round 4 items only — see the
round-3 review file in git history (commit `f09b51a`) for the
carry-over list.

- [ ] 1. **Quarantine legacy config keys.** Add `migrate_legacy` and
      route through the existing deferred-warning channel. Gate the
      post-quit `config::save` on a clean load so unknown-key files
      aren't auto-overwritten. (§1)
- [ ] 2. **Drop the stale `T` from `src/keys.rs:10`.** (§3)
- [ ] 3. **Hoist `marker_style` and drop `num_str.clone()`** in
      `src/render.rs`. One commit. (§4, §5)
- [ ] 4. **Decide cursor-vs-selection visual treatment** (§2). Either
      add a third foreground tier or reintroduce a subtle cursor-row
      bg. Either is a small commit; the choice deserves a deliberate
      call rather than the default-on collapse this round shipped.
- [ ] 5. **Drop the `fg` on pad-only spans in `pad_to_width`.**
      (Nice-to-have, two-line fix.)
- [ ] 6. **Drop `Option` on `Frame::passage`.** Touches one struct +
      one call site. (Nice-to-have.)
- [ ] 7. **Add a help-rendering test** that walks the `rows` slice
      and asserts removed keys (`T`) don't appear. Catches both the
      round-3 stale references and future drift. (Nice-to-have.)
- [ ] 8. **Resume the round-3 follow-up checklist** — the
      footnote/heading/xref ingest gap (carry-over §1) is the
      remaining real blocker for a first release.

## Resolution

All seven round-4 checklist items landed in commits
`a7c6b4d..46034c6` (post-review). Pedantic warning count is unchanged
(20 bin + 1 test); the round-4 fixes neither added nor removed any.

| Item | Commit | Notes |
|---|---|---|
| §1 legacy-key migration | `a7c6b4d` | `migrate_legacy` strips removed keys + warns once per dropped key; unit-tested for clean/dirty/lookalike inputs. |
| §3 stale `T` in keys.rs:10 | `07ced71` | Also fixed a matching stale reference in `Keymap` doc at `src/config.rs:68`. |
| §4 `marker_style` closure | `d31695f` | Hoisted to a `Style` value alongside `heading_style`. |
| §5 `num_str.clone()` | `d31695f` | Loop restructured: first chunk owns the prefix line, rest hang-indent — `num_str` is moved in. |
| §2 cursor-vs-selection | `9595137` | Three-tier ladder: idle light_grey, selection bright_white, cursor bright_white + BOLD. |
| Pad-only `fg` + `Frame::passage` Option | `7c53d62` | One commit, both nice-to-haves. |
| Help regression test | `46034c6` | `ROWS` lifted to module scope; test walks it asserting `T` doesn't reappear. |

Round-3 carry-over remains open and is the next focus for a release
push.

## Coverage self-assessment

| Dimension | Confidence | Notes |
|---|---|---|
| Compiler and lint cleanliness | high | All 21 pedantic warnings read end-to-end, same set as round 3 (none introduced or fixed). |
| API design | high | This round shrinks the public-via-config surface (drops `two_line_verses`, `toggle_verse_layout`); migration-path gap noted in §1. |
| Error handling | medium | The migration path in §1 *is* an error-handling decision (silent default vs warn-and-continue). Otherwise unchanged from round 3. |
| Ownership and borrowing | high | `num_str.clone()` (§5) is the only new allocation introduced this round; nothing else added. |
| Unsafe code | high | `#![deny(unsafe_code)]` unchanged at `src/main.rs:10`. No unsafe added. |
| Concurrency | high | Single-threaded reading loop unchanged. |
| Testing | medium | `tests/e2e.rs` doesn't cover the new gutter layout. The reading view rewrite would benefit from a small render snapshot test; no such test exists yet. Round-3 testing gaps (import e2e is `#[ignore]`d, no schema round-trip) still open. |
| Documentation | medium | CHANGELOG entry is honest about the breaking change; module-level `//!` in `src/keys.rs:10` is stale (§3). `cargo doc` clean. |
| Project structure | high | No new modules; reading-view files shrank. |
| Dependencies and toolchain | high | No deps added/removed. `cargo audit` / `udeps` clean. |
| Performance | medium | Did not benchmark the redesigned `render_passage`; visually-bounded chapter sizes (≤176 verses) keep this well under the noise floor. |
| Contributor experience | medium | `just baseline` runs but the still-broken `scripts/baseline.fish` (skill bug, not crate bug) trips up fish 4 users. Round-3 `cargo deny` initialisation gap still open. |
