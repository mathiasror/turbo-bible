//! Vim-style key-sequence state machine. Supports count prefixes (`5j`,
//! `10G`) and multi-key motions (`gg`, `[b`, `]b`). A 500 ms timeout clears
//! an ambiguous buffer (matches Vim's `timeoutlen`).
//!
//! User-configured single-key triggers from `config.toml` are checked first
//! (additive — defaults always remain functional). Chord and count handling
//! are not configurable.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smallvec::SmallVec;

use crate::config::{KeyBind, KeysConfig};

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
    ToggleVerseLayout,
}

pub struct KeyState {
    pending: SmallVec<[KeyEvent; 4]>,
    count: u16,
    last: Option<Instant>,
    extras: Vec<(KeyBind, Action)>,
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
        }
    }

    pub fn with_user_bindings(keys: &KeysConfig) -> Self {
        let mut s = Self::new();
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
        push(&keys.toggle_verse_layout, Action::ToggleVerseLayout);
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
        // Count prefix: digits while no pending operator. '0' is line-start,
        // not a count, when it's the first digit.
        if self.pending.is_empty()
            && key.modifiers.is_empty()
            && let KeyCode::Char(c) = key.code
            && c.is_ascii_digit()
            && !(self.count == 0 && c == '0')
        {
            self.count = self
                .count
                .saturating_mul(10)
                .saturating_add(c.to_digit(10).unwrap() as u16);
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

    fn count_or(&self, default: u16) -> u16 {
        if self.count == 0 { default } else { self.count }
    }

    #[cfg(test)]
    pub fn extras_count(&self) -> usize {
        self.extras.len()
    }

    fn try_resolve(&self) -> Resolve {
        let n = self.pending.len();
        let first = self.pending[0];
        if n == 1 {
            // User-configured triggers win over the hardcoded vim defaults.
            for (binding, action) in &self.extras {
                if binding.matches(&first) {
                    return Resolve::Action(*action);
                }
            }

            let c = first.code;
            let m = first.modifiers;
            let ctrl = m.contains(KeyModifiers::CONTROL);
            let plain = m.is_empty() || m == KeyModifiers::SHIFT;

            // Single-shot bindings.
            return match (c, ctrl, plain) {
                (KeyCode::Char('q'), false, true) => Resolve::Action(Action::Quit),
                (KeyCode::Esc, _, _) => Resolve::Action(Action::Back),

                (KeyCode::Char('j'), false, true) | (KeyCode::Down, _, _) => {
                    Resolve::Action(Action::CursorDown(self.count_or(1)))
                }
                (KeyCode::Char('k'), false, true) | (KeyCode::Up, _, _) => {
                    Resolve::Action(Action::CursorUp(self.count_or(1)))
                }
                (KeyCode::Char('h'), false, true) | (KeyCode::Left, _, _) => {
                    Resolve::Action(Action::PrevChapter)
                }
                (KeyCode::Char('l'), false, true) | (KeyCode::Right, _, _) => {
                    Resolve::Action(Action::NextChapter)
                }
                (KeyCode::Char('H'), false, true) => Resolve::Action(Action::PrevChapter),
                (KeyCode::Char('L'), false, true) => Resolve::Action(Action::NextChapter),

                (KeyCode::Char('d'), true, _) => Resolve::Action(Action::HalfPageDown),
                (KeyCode::Char('u'), true, _) => Resolve::Action(Action::HalfPageUp),
                (KeyCode::Char('f'), true, _) | (KeyCode::PageDown, _, _) => {
                    Resolve::Action(Action::PageDown)
                }
                (KeyCode::Char('b'), true, _) | (KeyCode::PageUp, _, _) => {
                    Resolve::Action(Action::PageUp)
                }
                (KeyCode::Char(' '), false, true) => Resolve::Action(Action::PageDown),

                (KeyCode::Char('G'), false, true) => Resolve::Action(Action::GotoBottom),
                (KeyCode::Char('K'), false, true) => Resolve::Action(Action::OpenFootnote),
                (KeyCode::Char('y'), false, true) => Resolve::Action(Action::CopyVerse),
                (KeyCode::Char('v'), false, true) => Resolve::Action(Action::ToggleVisual),
                (KeyCode::Char('V'), false, true) => Resolve::Action(Action::ToggleVisual),
                (KeyCode::Char('b'), false, true) => Resolve::Action(Action::AddBookmark),
                (KeyCode::Char('T'), false, true) => Resolve::Action(Action::ToggleVerseLayout),
                (KeyCode::Char('M'), false, true) => Resolve::Action(Action::OpenBookmarks),
                (KeyCode::F(4), _, _) => Resolve::Action(Action::OpenBookmarks),
                (KeyCode::Char('t'), false, true) => Resolve::Action(Action::OpenTranslations),
                (KeyCode::F(5), _, _) => Resolve::Action(Action::OpenTranslations),
                (KeyCode::Char('o'), true, _) => Resolve::Action(Action::JumpBack),
                (KeyCode::Char('i'), true, _) => Resolve::Action(Action::JumpForward),
                (KeyCode::Tab, _, _) => Resolve::Action(Action::ToggleSidebar),
                (KeyCode::Char('Z'), false, true) => Resolve::Partial,
                (KeyCode::Char(':'), false, true) | (KeyCode::Char(':'), false, false) => {
                    Resolve::Action(Action::OpenGoto)
                }
                (KeyCode::Char('/'), false, true) => Resolve::Action(Action::OpenFind),

                (KeyCode::F(1), _, _) => Resolve::Action(Action::OpenHelp),
                (KeyCode::F(2), _, _) => Resolve::Action(Action::OpenGoto),
                (KeyCode::F(3), _, _) => Resolve::Action(Action::OpenFind),
                (KeyCode::F(10), _, _) => Resolve::Action(Action::OpenMenu),

                // Multi-key starters.
                (KeyCode::Char('g'), false, true) => Resolve::Partial,
                (KeyCode::Char('['), false, true) => Resolve::Partial,
                (KeyCode::Char(']'), false, true) => Resolve::Partial,

                _ => Resolve::Unknown,
            };
        }
        if n == 2 {
            let a = self.pending[0].code;
            let b = self.pending[1].code;
            return match (a, b) {
                (KeyCode::Char('g'), KeyCode::Char('g')) => Resolve::Action(Action::GotoTop),
                (KeyCode::Char('['), KeyCode::Char('b')) => Resolve::Action(Action::PrevBook),
                (KeyCode::Char(']'), KeyCode::Char('b')) => Resolve::Action(Action::NextBook),
                (KeyCode::Char('Z'), KeyCode::Char('Z')) => Resolve::Action(Action::Quit),
                (KeyCode::Char('Z'), KeyCode::Char('Q')) => Resolve::Action(Action::Quit),
                _ => Resolve::Unknown,
            };
        }
        Resolve::Unknown
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
        let mut ks = KeyState::with_user_bindings(&cfg);
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
        let mut ks = KeyState::with_user_bindings(&cfg);
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
        let mut ks = KeyState::with_user_bindings(&cfg);
        // gg → top
        ks.handle(ev(KeyCode::Char('g')));
        assert_eq!(ks.handle(ev(KeyCode::Char('g'))), Some(Action::GotoTop));
    }
}
