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
    HalfPageUp,
    HalfPageDown,
    PageUp,
    PageDown,
    GotoTop,
    GotoBottom,
    PrevChapter,
    NextChapter,
    PrevBook,
    NextBook,
    OpenGoto,
    OpenFind,
    OpenFootnote,
    OpenHelp,
    OpenMenu,
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
        push(&keys.open_menu, Action::OpenMenu);
        push(&keys.open_bookmarks, Action::OpenBookmarks);
        push(&keys.open_translations, Action::OpenTranslations);
        push(&keys.copy_verse, Action::CopyVerse);
        push(&keys.toggle_sidebar, Action::ToggleSidebar);
        push(&keys.toggle_visual, Action::ToggleVisual);
        push(&keys.add_bookmark, Action::AddBookmark);
        push(&keys.jump_back, Action::JumpBack);
        push(&keys.jump_forward, Action::JumpForward);
        push(&keys.prev_chapter, Action::PrevChapter);
        push(&keys.next_chapter, Action::NextChapter);
        push(&keys.half_page_down, Action::HalfPageDown);
        push(&keys.half_page_up, Action::HalfPageUp);
        push(&keys.page_down, Action::PageDown);
        push(&keys.page_up, Action::PageUp);
        push(&keys.goto_top, Action::GotoTop);
        push(&keys.goto_bottom, Action::GotoBottom);
        // CursorDown/Up always step by 1 from user-bound keys; counts only
        // apply to the hardcoded j/k/Up/Down to keep semantics predictable.
        push(&keys.cursor_down, Action::CursorDown(1));
        push(&keys.cursor_up, Action::CursorUp(1));
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
            return match (a, b) {
                (KeyCode::Char('g'), KeyCode::Char('g')) => Resolve::Action(Action::GotoTop),
                (KeyCode::Char('['), KeyCode::Char('b')) => Resolve::Action(Action::PrevBook),
                (KeyCode::Char(']'), KeyCode::Char('b')) => Resolve::Action(Action::NextBook),
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
            KeyCode::Left => Resolve::Action(Action::PrevChapter),
            KeyCode::Right => Resolve::Action(Action::NextChapter),
            KeyCode::Home => Resolve::Action(Action::GotoTop),
            KeyCode::End => Resolve::Action(Action::GotoBottom),
            KeyCode::PageDown => Resolve::Action(Action::PageDown),
            KeyCode::PageUp => Resolve::Action(Action::PageUp),
            KeyCode::Tab => Resolve::Action(Action::ToggleSidebar),
            KeyCode::F(1) => Resolve::Action(Action::OpenHelp),
            KeyCode::F(2) => Resolve::Action(Action::OpenGoto),
            KeyCode::F(3) => Resolve::Action(Action::OpenFind),
            KeyCode::F(4) => Resolve::Action(Action::OpenBookmarks),
            KeyCode::F(5) => Resolve::Action(Action::OpenTranslations),
            KeyCode::F(10) => Resolve::Action(Action::OpenMenu),
            KeyCode::Char(' ') if plain => Resolve::Action(Action::PageDown),
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

        match (c, ctrl, plain) {
            (KeyCode::Char('j'), false, true) => {
                Resolve::Action(Action::CursorDown(self.count_or(1)))
            }
            (KeyCode::Char('k'), false, true) => {
                Resolve::Action(Action::CursorUp(self.count_or(1)))
            }
            (KeyCode::Char('h' | 'H'), false, true) => Resolve::Action(Action::PrevChapter),
            (KeyCode::Char('l' | 'L'), false, true) => Resolve::Action(Action::NextChapter),

            (KeyCode::Char('d'), true, _) => Resolve::Action(Action::HalfPageDown),
            (KeyCode::Char('u'), true, _) => Resolve::Action(Action::HalfPageUp),
            (KeyCode::Char('f'), true, _) => Resolve::Action(Action::PageDown),
            (KeyCode::Char('b'), true, _) => Resolve::Action(Action::PageUp),

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

            // Multi-key starters.
            (KeyCode::Char('Z' | 'g' | '[' | ']'), false, true) => Resolve::Partial,

            _ => Resolve::Unknown,
        }
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
        assert_eq!(ks.handle(ev(KeyCode::Left)), Some(Action::PrevChapter));
        assert_eq!(ks.handle(ev(KeyCode::Home)), Some(Action::GotoTop));
        assert_eq!(ks.handle(ev(KeyCode::PageDown)), Some(Action::PageDown));
        assert_eq!(ks.handle(ev(KeyCode::F(3))), Some(Action::OpenFind));
        assert_eq!(ks.handle(ev(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(ks.handle(ev(KeyCode::Char('/'))), Some(Action::OpenFind));
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
}
