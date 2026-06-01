//! Vim-style key-sequence state machine. Supports count prefixes (`5j`,
//! `10G`) and multi-key motions (`gg`, `[b`, `]b`). A 500 ms timeout clears
//! an ambiguous buffer (matches Vim's `timeoutlen`).
//!
//! Two layers feed `try_resolve`:
//!   * **Base** — always active. Arrows, PgUp/PgDn, Home/End, F-keys, Esc,
//!     Tab, Enter, Space, `/` (find), `q` (quit). The pager-style baseline
//!     that every reader-shaped TUI shares.
//!   * **Vim** — gated by [`Keymap::Vim`]. Letter keys (hjkl, gg/G, n/N, K,
//!     y, v/V, b, M, t, ZZ/ZQ), `:` ex-commands, counts, and chord
//!     starters (`g`, `[`, `]`, `Z`).
//!
//! User-configured single-key triggers from `config.toml` are checked first
//! and apply in both profiles (additive — defaults always remain functional).
//! Chord and count handling are not configurable.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smallvec::SmallVec;

use crate::config::{KeyBind, Keymap, KeysConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    CursorUp(u16),
    CursorDown(u16),
    // Page / chapter / book motions carry a count so a prefix repeats them
    // (`2Ctrl-D`, `5l`, `3]b`) — making the README's "count prefixes work"
    // claim true across the motion family, not just j/k (issue #66, finding
    // #15). User-bound keys still step by 1 (see `with_user_bindings`).
    HalfPageUp(u16),
    HalfPageDown(u16),
    PageUp(u16),
    PageDown(u16),
    GotoTop,
    GotoBottom,
    PrevChapter(u16),
    NextChapter(u16),
    PrevBook(u16),
    NextBook(u16),
    OpenGoto,
    OpenFind,
    OpenFootnote,
    OpenHelp,
    JumpBack,
    JumpForward,
    CopyVerse,
    ToggleSidebar,
    Back,
    ToggleVisual,
    AddBookmark,
    OpenBookmarks,
    OpenTranslations,
    /// Repeat the last `/`-search forward (canonical order). No-op when no
    /// query has been entered yet. Vim-layer only.
    SearchNext,
    /// Repeat the last `/`-search backward. Vim-layer only.
    SearchPrev,
    /// `Ctrl-W v` — open a new compare pane (via the Translations picker).
    /// Vim-layer only (the `Ctrl-W` window-command chord).
    CompareOpen,
    /// `Ctrl-W w` — cycle focus to the next compare pane (wraps).
    FocusNext,
    /// `Ctrl-W h` — focus the pane to the left (clamps).
    FocusLeft,
    /// `Ctrl-W l` — focus the pane to the right (clamps).
    FocusRight,
    /// `Ctrl-W q` — close the focused compare pane (no-op with one pane).
    CompareClose,
    /// `Ctrl-W d` — toggle word-level diff highlighting across compare panes
    /// (visible only while ≥2 panes are open). Vim-layer only.
    ToggleWordDiff,
}

pub struct KeyState {
    pending: SmallVec<[KeyEvent; 4]>,
    count: u16,
    last: Option<Instant>,
    extras: Vec<(KeyBind, Action)>,
    keymap: Keymap,
}

enum Resolve {
    Action(Action),
    Partial,
    Unknown,
}

impl Default for KeyState {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyState {
    pub fn new() -> Self {
        Self {
            pending: SmallVec::new(),
            count: 0,
            last: None,
            extras: Vec::new(),
            keymap: Keymap::Vim,
        }
    }

    pub fn with_user_bindings(keys: &KeysConfig, keymap: Keymap) -> Self {
        let mut s = Self::new();
        s.keymap = keymap;
        let mut push = |binds: &[KeyBind], action: Action| {
            for &b in binds {
                s.extras.push((b, action));
            }
        };
        push(&keys.quit, Action::Quit);
        push(&keys.back, Action::Back);
        push(&keys.open_goto, Action::OpenGoto);
        push(&keys.open_find, Action::OpenFind);
        push(&keys.open_help, Action::OpenHelp);
        push(&keys.open_footnote, Action::OpenFootnote);
        push(&keys.open_bookmarks, Action::OpenBookmarks);
        push(&keys.open_translations, Action::OpenTranslations);
        push(&keys.copy_verse, Action::CopyVerse);
        push(&keys.toggle_sidebar, Action::ToggleSidebar);
        push(&keys.toggle_visual, Action::ToggleVisual);
        push(&keys.add_bookmark, Action::AddBookmark);
        push(&keys.jump_back, Action::JumpBack);
        push(&keys.jump_forward, Action::JumpForward);
        push(&keys.goto_top, Action::GotoTop);
        push(&keys.goto_bottom, Action::GotoBottom);
        // Count-bearing motions: a user-bound key always steps by 1 (the count
        // prefix is a vim-layer feature that rides the hardcoded keys only), so
        // these extras push the count-less form (issue #66, finding #15).
        push(&keys.prev_chapter, Action::PrevChapter(1));
        push(&keys.next_chapter, Action::NextChapter(1));
        push(&keys.half_page_down, Action::HalfPageDown(1));
        push(&keys.half_page_up, Action::HalfPageUp(1));
        push(&keys.page_down, Action::PageDown(1));
        push(&keys.page_up, Action::PageUp(1));
        push(&keys.cursor_down, Action::CursorDown(1));
        push(&keys.cursor_up, Action::CursorUp(1));
        // Additive single-key aliases for actions whose only defaults are
        // multi-key chords or letter keys, so a user whose terminal grabs
        // Ctrl-W (e.g. tmux) can still reach them (issue #66, finding #16).
        push(&keys.search_next, Action::SearchNext);
        push(&keys.search_prev, Action::SearchPrev);
        push(&keys.compare_open, Action::CompareOpen);
        push(&keys.focus_next, Action::FocusNext);
        push(&keys.focus_left, Action::FocusLeft);
        push(&keys.focus_right, Action::FocusRight);
        push(&keys.compare_close, Action::CompareClose);
        push(&keys.toggle_word_diff, Action::ToggleWordDiff);
        s
    }

    pub fn tick(&mut self) {
        if let Some(t) = self.last
            && t.elapsed() > Duration::from_millis(500)
        {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.pending.clear();
        self.count = 0;
        self.last = None;
    }

    pub fn handle(&mut self, key: KeyEvent) -> Option<Action> {
        self.tick();
        // Esc first aborts an in-progress command — a pending count or a
        // half-typed chord — vim-style, without leaving the screen. Only a
        // "clean" Esc (nothing pending) falls through to the base-layer Back.
        // With the showcmd indicator the cleared count/chord is visible, so
        // this no longer silently eats the keystroke (issue #66, finding #6).
        if key.code == KeyCode::Esc && (self.count != 0 || !self.pending.is_empty()) {
            self.reset();
            return None;
        }
        // Count prefix is a vim-layer feature. In turbo mode digits are inert
        // — they just fall through to the resolver which returns Unknown.
        if self.keymap == Keymap::Vim
            && self.pending.is_empty()
            && key.modifiers.is_empty()
            && let KeyCode::Char(c) = key.code
            && c.is_ascii_digit()
            && !(self.count == 0 && c == '0')
        {
            // `is_ascii_digit()` was just checked; `to_digit(10)` returns
            // a value in 0..=9 which always fits in u16. Use `unwrap_or(0)`
            // to make that infallibility loud without an unwrap.
            let digit = u16::try_from(c.to_digit(10).unwrap_or(0)).unwrap_or(0);
            self.count = self.count.saturating_mul(10).saturating_add(digit);
            self.last = Some(Instant::now());
            return None;
        }
        self.pending.push(key);
        self.last = Some(Instant::now());
        match self.try_resolve() {
            Resolve::Action(a) => {
                self.reset();
                Some(a)
            }
            Resolve::Partial => None,
            Resolve::Unknown => {
                self.reset();
                None
            }
        }
    }

    const fn count_or(&self, default: u16) -> u16 {
        if self.count == 0 { default } else { self.count }
    }

    /// The active keymap profile, so callers outside this module (the status
    /// bar, the Help dialog) can render a profile-honest cheat sheet
    /// (issue #66, findings #12 / #17).
    pub const fn keymap(&self) -> Keymap {
        self.keymap
    }

    /// The in-progress count/chord, for a vim-style `showcmd` indicator —
    /// e.g. `"5"` while a count builds, `"g"` / `"^W"` mid-chord, `"5g"` for
    /// both. `None` when nothing is pending (issue #66, finding #7).
    pub fn pending_hint(&self) -> Option<String> {
        if self.count == 0 && self.pending.is_empty() {
            return None;
        }
        let mut s = String::new();
        if self.count != 0 {
            s.push_str(&self.count.to_string());
        }
        for ev in &self.pending {
            push_key_label(&mut s, ev);
        }
        (!s.is_empty()).then_some(s)
    }

    #[cfg(test)]
    pub const fn extras_count(&self) -> usize {
        self.extras.len()
    }

    fn try_resolve(&self) -> Resolve {
        let n = self.pending.len();
        let first = self.pending[0];
        if n == 1 {
            // User-configured triggers win over the hardcoded defaults and
            // apply in both keymap profiles — the additive contract.
            for (binding, action) in &self.extras {
                if binding.matches(&first) {
                    return Resolve::Action(*action);
                }
            }
            if let Some(r) = self.resolve_base(first) {
                return r;
            }
            if self.keymap == Keymap::Vim {
                return self.resolve_vim_single(first);
            }
            return Resolve::Unknown;
        }
        // Multi-key chords are vim-only. Turbo mode never reaches `n > 1`
        // because no base-layer key returns `Partial`.
        if n == 2 && self.keymap == Keymap::Vim {
            let a = self.pending[0].code;
            let b = self.pending[1].code;
            // `Ctrl-W <key>` window commands. The CONTROL modifier rides the
            // first key only; the second arrives plain (some terminals deliver
            // Ctrl-W as the dedicated 0x17 byte, which crossterm maps to
            // `Char('w')` + CONTROL either way).
            if matches!(a, KeyCode::Char('w' | 'W'))
                && self.pending[0].modifiers.contains(KeyModifiers::CONTROL)
            {
                return match b {
                    KeyCode::Char('v') => Resolve::Action(Action::CompareOpen),
                    KeyCode::Char('w') => Resolve::Action(Action::FocusNext),
                    KeyCode::Char('h') => Resolve::Action(Action::FocusLeft),
                    KeyCode::Char('l') => Resolve::Action(Action::FocusRight),
                    KeyCode::Char('q') => Resolve::Action(Action::CompareClose),
                    KeyCode::Char('d') => Resolve::Action(Action::ToggleWordDiff),
                    _ => Resolve::Unknown,
                };
            }
            return match (a, b) {
                (KeyCode::Char('g'), KeyCode::Char('g')) => Resolve::Action(Action::GotoTop),
                // `3]b` jumps three books forward; the count rides the chord too
                // (issue #66, finding #15).
                (KeyCode::Char('['), KeyCode::Char('b')) => {
                    Resolve::Action(Action::PrevBook(self.count_or(1)))
                }
                (KeyCode::Char(']'), KeyCode::Char('b')) => {
                    Resolve::Action(Action::NextBook(self.count_or(1)))
                }
                (KeyCode::Char('Z'), KeyCode::Char('Z' | 'Q')) => Resolve::Action(Action::Quit),
                _ => Resolve::Unknown,
            };
        }
        Resolve::Unknown
    }

    /// Base layer — keys every reader-shaped TUI shares. Active in both vim
    /// and turbo profiles. Returns `None` when the key isn't ours so the
    /// caller can fall through to the vim layer (or to `Unknown`).
    fn resolve_base(&self, ev: KeyEvent) -> Option<Resolve> {
        let c = ev.code;
        let m = ev.modifiers;
        let plain = m.is_empty() || m == KeyModifiers::SHIFT;
        // Arrows / page-keys / function-keys / Tab / Esc — modifier-tolerant
        // because terminals report them inconsistently with SHIFT.
        Some(match c {
            KeyCode::Esc => Resolve::Action(Action::Back),
            KeyCode::Down => Resolve::Action(Action::CursorDown(self.count_or(1))),
            KeyCode::Up => Resolve::Action(Action::CursorUp(self.count_or(1))),
            KeyCode::Left => Resolve::Action(Action::PrevChapter(self.count_or(1))),
            KeyCode::Right => Resolve::Action(Action::NextChapter(self.count_or(1))),
            KeyCode::Home => Resolve::Action(Action::GotoTop),
            KeyCode::End => Resolve::Action(Action::GotoBottom),
            KeyCode::PageDown => Resolve::Action(Action::PageDown(self.count_or(1))),
            KeyCode::PageUp => Resolve::Action(Action::PageUp(self.count_or(1))),
            KeyCode::Tab => Resolve::Action(Action::ToggleSidebar),
            KeyCode::F(1) => Resolve::Action(Action::OpenHelp),
            KeyCode::F(2) => Resolve::Action(Action::OpenGoto),
            KeyCode::F(3) => Resolve::Action(Action::OpenFind),
            KeyCode::F(4) => Resolve::Action(Action::OpenBookmarks),
            KeyCode::F(5) => Resolve::Action(Action::OpenTranslations),
            KeyCode::Char(' ') if plain => Resolve::Action(Action::PageDown(self.count_or(1))),
            KeyCode::Char('q') if plain => Resolve::Action(Action::Quit),
            KeyCode::Char('/') if plain => Resolve::Action(Action::OpenFind),
            _ => return None,
        })
    }

    /// Vim layer — gated by [`Keymap::Vim`]. Letter keys, Ctrl-modified
    /// vim motions, `:` ex-command, chord starters, n/N repeat-search.
    fn resolve_vim_single(&self, ev: KeyEvent) -> Resolve {
        let c = ev.code;
        let m = ev.modifiers;
        let ctrl = m.contains(KeyModifiers::CONTROL);
        let plain = m.is_empty() || m == KeyModifiers::SHIFT;

        #[allow(
            clippy::match_same_arms,
            reason = "two distinct chord families both stage as Partial — keeping the \
                      arms separate documents the classification (Ctrl-W window-command \
                      starter vs. plain vim multi-key starters: gg / [b / ]b / ZZ / ZQ)"
        )]
        match (c, ctrl, plain) {
            (KeyCode::Char('j'), false, true) => {
                Resolve::Action(Action::CursorDown(self.count_or(1)))
            }
            (KeyCode::Char('k'), false, true) => {
                Resolve::Action(Action::CursorUp(self.count_or(1)))
            }
            (KeyCode::Char('h' | 'H'), false, true) => {
                Resolve::Action(Action::PrevChapter(self.count_or(1)))
            }
            (KeyCode::Char('l' | 'L'), false, true) => {
                Resolve::Action(Action::NextChapter(self.count_or(1)))
            }

            (KeyCode::Char('d'), true, _) => {
                Resolve::Action(Action::HalfPageDown(self.count_or(1)))
            }
            (KeyCode::Char('u'), true, _) => Resolve::Action(Action::HalfPageUp(self.count_or(1))),
            (KeyCode::Char('f'), true, _) => Resolve::Action(Action::PageDown(self.count_or(1))),
            (KeyCode::Char('b'), true, _) => Resolve::Action(Action::PageUp(self.count_or(1))),

            (KeyCode::Char('G'), false, true) => Resolve::Action(Action::GotoBottom),
            (KeyCode::Char('K'), false, true) => Resolve::Action(Action::OpenFootnote),
            (KeyCode::Char('y'), false, true) => Resolve::Action(Action::CopyVerse),
            (KeyCode::Char('v' | 'V'), false, true) => Resolve::Action(Action::ToggleVisual),
            (KeyCode::Char('b'), false, true) => Resolve::Action(Action::AddBookmark),
            (KeyCode::Char('M'), false, true) => Resolve::Action(Action::OpenBookmarks),
            (KeyCode::Char('t'), false, true) => Resolve::Action(Action::OpenTranslations),
            (KeyCode::Char('n'), false, true) => Resolve::Action(Action::SearchNext),
            (KeyCode::Char('N'), false, true) => Resolve::Action(Action::SearchPrev),
            (KeyCode::Char('o'), true, _) => Resolve::Action(Action::JumpBack),
            (KeyCode::Char('i'), true, _) => Resolve::Action(Action::JumpForward),
            (KeyCode::Char(':'), false, _) => Resolve::Action(Action::OpenGoto),

            // `Ctrl-W` opens the window-command chord (Ctrl-W v / w / h / l / q).
            (KeyCode::Char('w' | 'W'), true, _) => Resolve::Partial,

            // Multi-key starters.
            (KeyCode::Char('Z' | 'g' | '[' | ']'), false, true) => Resolve::Partial,

            _ => Resolve::Unknown,
        }
    }
}

/// Render one pending key for the `showcmd` hint: a Ctrl-modified key as
/// `^X` (e.g. `Ctrl-W` → `^W`), a plain char verbatim. Other key kinds don't
/// stage as pending chord keys, so they contribute nothing.
fn push_key_label(out: &mut String, ev: &KeyEvent) {
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        out.push('^');
        if let KeyCode::Char(c) = ev.code {
            out.push(c.to_ascii_uppercase());
        }
    } else if let KeyCode::Char(c) = ev.code {
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{KeyBind, KeysConfig};

    fn ev(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }
    fn evm(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn user_binding_overrides_default_lookup() {
        let cfg = KeysConfig {
            open_translations: vec![KeyBind {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::empty(),
            }],
            ..KeysConfig::default()
        };
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert!(ks.extras_count() > 0);
        assert_eq!(
            ks.handle(ev(KeyCode::Char('x'))),
            Some(Action::OpenTranslations)
        );
    }

    #[test]
    fn defaults_still_work_with_extras_present() {
        let cfg = KeysConfig {
            quit: vec![KeyBind {
                code: KeyCode::Char('Q'),
                modifiers: KeyModifiers::empty(),
            }],
            ..KeysConfig::default()
        };
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // Hardcoded 'q' still quits.
        assert_eq!(ks.handle(ev(KeyCode::Char('q'))), Some(Action::Quit));
        // And the user-added 'Q' also quits.
        assert_eq!(
            ks.handle(evm(KeyCode::Char('Q'), KeyModifiers::SHIFT)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn chord_unaffected_by_user_bindings() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // gg → top
        ks.handle(ev(KeyCode::Char('g')));
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), Some(Action::GotoTop));
    }

    #[test]
    fn n_and_shift_n_repeat_search_in_vim_mode() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(ks.handle(ev(KeyCode::Char('n'))), Some(Action::SearchNext));
        assert_eq!(ks.handle(ev(KeyCode::Char('N'))), Some(Action::SearchPrev));
    }

    /// Feed `Ctrl-W` then `second`, asserting the resolved window-command.
    fn ctrl_w_then(second: char) -> Option<Action> {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // First key is a partial chord starter → no action yet.
        assert_eq!(
            ks.handle(evm(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None,
            "Ctrl-W alone is a chord starter"
        );
        ks.handle(ev(KeyCode::Char(second)))
    }

    #[test]
    fn ctrl_w_window_commands_resolve() {
        assert_eq!(ctrl_w_then('v'), Some(Action::CompareOpen));
        assert_eq!(ctrl_w_then('w'), Some(Action::FocusNext));
        assert_eq!(ctrl_w_then('h'), Some(Action::FocusLeft));
        assert_eq!(ctrl_w_then('l'), Some(Action::FocusRight));
        assert_eq!(ctrl_w_then('q'), Some(Action::CompareClose));
        assert_eq!(ctrl_w_then('d'), Some(Action::ToggleWordDiff));
    }

    #[test]
    fn ctrl_w_then_junk_resets_without_action() {
        // An unmapped second key clears the chord buffer (no stuck state).
        assert_eq!(ctrl_w_then('z'), None);
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        ks.handle(evm(KeyCode::Char('w'), KeyModifiers::CONTROL));
        ks.handle(ev(KeyCode::Char('z'))); // junk → reset
        // Buffer is clear: a fresh 'j' moves the cursor as usual.
        assert_eq!(
            ks.handle(ev(KeyCode::Char('j'))),
            Some(Action::CursorDown(1))
        );
    }

    #[test]
    fn ctrl_w_is_inert_in_turbo_mode() {
        // Turbo mode has no vim chords, so Ctrl-W never starts one.
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Turbo);
        assert_eq!(
            ks.handle(evm(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None
        );
        // The following key is dispatched on its own, not as a chord tail.
        assert_eq!(ks.handle(ev(KeyCode::Char('v'))), None);
    }

    #[test]
    fn turbo_mode_drops_vim_letters_keeps_base() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Turbo);
        // Vim letters are inert.
        assert_eq!(ks.handle(ev(KeyCode::Char('j'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Char('h'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Char('n'))), None);
        // No chord state — second `g` would not produce GotoTop either.
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), None);
        // Base layer survives.
        assert_eq!(ks.handle(ev(KeyCode::Down)), Some(Action::CursorDown(1)));
        assert_eq!(ks.handle(ev(KeyCode::Left)), Some(Action::PrevChapter(1)));
        assert_eq!(ks.handle(ev(KeyCode::Home)), Some(Action::GotoTop));
        assert_eq!(ks.handle(ev(KeyCode::PageDown)), Some(Action::PageDown(1)));
        assert_eq!(ks.handle(ev(KeyCode::F(3))), Some(Action::OpenFind));
        assert_eq!(ks.handle(ev(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(ks.handle(ev(KeyCode::Char('/'))), Some(Action::OpenFind));
    }

    #[test]
    fn esc_aborts_pending_count_without_leaving() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // Build a count, then Esc: the count is dropped and Esc does NOT Back.
        assert_eq!(ks.handle(ev(KeyCode::Char('5'))), None);
        assert_eq!(ks.pending_hint().as_deref(), Some("5"));
        assert_eq!(ks.handle(ev(KeyCode::Esc)), None, "Esc aborts the count");
        assert_eq!(ks.pending_hint(), None);
        // The count is gone: a bare `j` now steps by 1, not 5.
        assert_eq!(
            ks.handle(ev(KeyCode::Char('j'))),
            Some(Action::CursorDown(1))
        );
        // And a clean Esc (nothing pending) still backs out.
        assert_eq!(ks.handle(ev(KeyCode::Esc)), Some(Action::Back));
    }

    #[test]
    fn esc_aborts_half_typed_chord_then_backs() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // `g` is a chord starter (gg) — pending, no action yet.
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), None);
        assert_eq!(ks.pending_hint().as_deref(), Some("g"));
        // First Esc clears the half-chord (no Back); second Esc backs.
        assert_eq!(ks.handle(ev(KeyCode::Esc)), None);
        assert_eq!(ks.pending_hint(), None);
        assert_eq!(ks.handle(ev(KeyCode::Esc)), Some(Action::Back));
    }

    #[test]
    fn pending_hint_renders_count_chord_and_ctrl_w() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(ks.pending_hint(), None);
        ks.handle(ev(KeyCode::Char('5')));
        ks.handle(ev(KeyCode::Char('g'))); // count + chord starter
        assert_eq!(ks.pending_hint().as_deref(), Some("5g"));
        // Resolve the chord (gg → top) and the hint clears.
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), Some(Action::GotoTop));
        assert_eq!(ks.pending_hint(), None);
        // Ctrl-W renders as ^W.
        ks.handle(evm(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(ks.pending_hint().as_deref(), Some("^W"));
    }

    #[test]
    fn turbo_mode_ignores_count_prefix() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Turbo);
        // `5` in turbo mode goes straight to the resolver and falls through
        // as Unknown — no count accumulation.
        assert_eq!(ks.handle(ev(KeyCode::Char('5'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Down)), Some(Action::CursorDown(1)));
    }

    #[test]
    fn turbo_mode_still_honors_user_extras() {
        let cfg = KeysConfig {
            open_translations: vec![KeyBind {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::empty(),
            }],
            ..KeysConfig::default()
        };
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Turbo);
        assert_eq!(
            ks.handle(ev(KeyCode::Char('x'))),
            Some(Action::OpenTranslations)
        );
    }

    #[test]
    fn keymap_accessor_reports_active_profile() {
        let cfg = KeysConfig::default();
        assert_eq!(
            KeyState::with_user_bindings(&cfg, Keymap::Vim).keymap(),
            Keymap::Vim
        );
        assert_eq!(
            KeyState::with_user_bindings(&cfg, Keymap::Turbo).keymap(),
            Keymap::Turbo
        );
    }

    /// Build a count then a single key, asserting the resolved action.
    fn count_then(ks: &mut KeyState, count: &str, key: KeyEvent) -> Option<Action> {
        for c in count.chars() {
            assert_eq!(ks.handle(ev(KeyCode::Char(c))), None, "digit is inert");
        }
        ks.handle(key)
    }

    #[test]
    fn count_prefix_rides_chapter_and_page_motions() {
        let cfg = KeysConfig::default();
        // `5l` → NextChapter(5), `3h` → PrevChapter(3).
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(
            count_then(&mut ks, "5", ev(KeyCode::Char('l'))),
            Some(Action::NextChapter(5))
        );
        assert_eq!(
            count_then(&mut ks, "3", ev(KeyCode::Char('h'))),
            Some(Action::PrevChapter(3))
        );
        // Arrows take the count too.
        assert_eq!(
            count_then(&mut ks, "4", ev(KeyCode::Right)),
            Some(Action::NextChapter(4))
        );
        // `2Ctrl-D` → HalfPageDown(2); `2Ctrl-F` → PageDown(2).
        assert_eq!(
            count_then(&mut ks, "2", evm(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(Action::HalfPageDown(2))
        );
        assert_eq!(
            count_then(&mut ks, "2", evm(KeyCode::Char('f'), KeyModifiers::CONTROL)),
            Some(Action::PageDown(2))
        );
        // Space (base layer) carries the count as a page-down too.
        assert_eq!(
            count_then(&mut ks, "3", ev(KeyCode::Char(' '))),
            Some(Action::PageDown(3))
        );
    }

    #[test]
    fn count_prefix_rides_book_chord() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        // `3]b` → NextBook(3).
        assert_eq!(ks.handle(ev(KeyCode::Char('3'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Char(']'))), None, "chord starter");
        assert_eq!(ks.handle(ev(KeyCode::Char('b'))), Some(Action::NextBook(3)));
        // `2[b` → PrevBook(2).
        assert_eq!(ks.handle(ev(KeyCode::Char('2'))), None);
        assert_eq!(ks.handle(ev(KeyCode::Char('['))), None, "chord starter");
        assert_eq!(ks.handle(ev(KeyCode::Char('b'))), Some(Action::PrevBook(2)));
    }

    #[test]
    fn bare_motion_keys_default_to_count_one() {
        let cfg = KeysConfig::default();
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(
            ks.handle(ev(KeyCode::Char('l'))),
            Some(Action::NextChapter(1))
        );
        assert_eq!(
            ks.handle(ev(KeyCode::Char('h'))),
            Some(Action::PrevChapter(1))
        );
        assert_eq!(
            ks.handle(evm(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(Action::HalfPageDown(1))
        );
    }

    #[test]
    fn user_bound_motion_keys_step_by_one() {
        // The count prefix is a vim-layer feature that rides only the hardcoded
        // keys — a user-bound motion alias always steps by 1.
        let cfg = KeysConfig {
            next_chapter: vec![KeyBind {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers::empty(),
            }],
            page_down: vec![KeyBind {
                code: KeyCode::Char('z'),
                modifiers: KeyModifiers::empty(),
            }],
            ..KeysConfig::default()
        };
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(
            count_then(&mut ks, "5", ev(KeyCode::Char('x'))),
            Some(Action::NextChapter(1)),
            "user-bound key ignores the count"
        );
        assert_eq!(ks.handle(ev(KeyCode::Char('z'))), Some(Action::PageDown(1)));
    }

    #[test]
    fn configured_aliases_reach_chord_and_search_actions() {
        // The Ctrl-W chords and n/N stay hardcoded, but a user whose terminal
        // grabs Ctrl-W can add single-key aliases (issue #66, finding #16).
        let cfg = KeysConfig {
            compare_open: vec![KeyBind {
                code: KeyCode::Char('s'),
                modifiers: KeyModifiers::ALT,
            }],
            compare_close: vec![KeyBind {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::ALT,
            }],
            focus_next: vec![KeyBind {
                code: KeyCode::Char('w'),
                modifiers: KeyModifiers::ALT,
            }],
            toggle_word_diff: vec![KeyBind {
                code: KeyCode::Char('='),
                modifiers: KeyModifiers::empty(),
            }],
            search_next: vec![KeyBind {
                code: KeyCode::F(8),
                modifiers: KeyModifiers::empty(),
            }],
            search_prev: vec![KeyBind {
                code: KeyCode::F(9),
                modifiers: KeyModifiers::empty(),
            }],
            ..KeysConfig::default()
        };
        let mut ks = KeyState::with_user_bindings(&cfg, Keymap::Vim);
        assert_eq!(
            ks.handle(evm(KeyCode::Char('s'), KeyModifiers::ALT)),
            Some(Action::CompareOpen)
        );
        assert_eq!(
            ks.handle(evm(KeyCode::Char('c'), KeyModifiers::ALT)),
            Some(Action::CompareClose)
        );
        assert_eq!(
            ks.handle(evm(KeyCode::Char('w'), KeyModifiers::ALT)),
            Some(Action::FocusNext)
        );
        assert_eq!(
            ks.handle(ev(KeyCode::Char('='))),
            Some(Action::ToggleWordDiff)
        );
        assert_eq!(ks.handle(ev(KeyCode::F(8))), Some(Action::SearchNext));
        assert_eq!(ks.handle(ev(KeyCode::F(9))), Some(Action::SearchPrev));
        // The hardcoded defaults still resolve alongside the aliases.
        assert_eq!(ks.handle(ev(KeyCode::Char('n'))), Some(Action::SearchNext));
        assert_eq!(
            ks.handle(evm(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None,
            "Ctrl-W still starts the window chord"
        );
        assert_eq!(ks.handle(ev(KeyCode::Char('v'))), Some(Action::CompareOpen));
    }
}
