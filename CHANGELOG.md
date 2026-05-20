# Changelog

All notable changes to this project will be documented here. Format
roughly follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions roughly follow [SemVer](https://semver.org/) until 1.0.

## [Unreleased]

### Added
- `CHANGELOG.md`.
- `#![deny(unsafe_code)]` at the crate root.
- `History` jump stack is now bounded at 100 entries; long sessions no
  longer grow the stack unbounded.
- Clipboard failures (`y` to copy verse) now surface via the deferred
  warning channel instead of being silently swallowed.
- `config::load` distinguishes "file missing" (silent default) from
  "file unreadable" (logged); previously both were silent.
- RAII `TerminalGuard` restores the terminal even if a draw panics,
  replacing the manual `init_terminal` / `restore_terminal` pair.
- Shared XDG path resolution in `src/paths.rs` (was duplicated across
  `config.rs`, `state.rs`, `bookmark.rs`).
- Shared `word_wrap` in `src/text.rs` (was duplicated between
  `render.rs` and `ui/splash.rs`).

### Changed
- `switch_translation` is now atomic: a failed translation swap restores
  the previous translation instead of leaving a half-swapped state.
- `search()` and `quote::pick()` take the translation as an explicit
  parameter rather than reading it off `Db`.
- Cast cleanups: `as i64` / `as u16` replaced with `From::from` /
  `try_from` where it makes infallibility obvious; remaining truncating
  casts carry per-site justifications.
- Bulk pedantic-clippy clean-up: `map(_).unwrap_or(_)` → `map_or`,
  inlined format args, `const fn` where applicable, identical match arms
  merged.
- Internal `pub` surface tightened to `pub(crate)`; `Db::translation`
  is now method-gated via `Db::set_translation`.

## [0.1.0] - 2026-05-20

Initial release. Three translations (KJV, Bibelen 1930, Reina-Valera
1909), FTS5 search, bookmarks, history, vim+turbo keymap profiles, XDG
state.
