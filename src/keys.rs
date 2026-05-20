//! Vim-style key-sequence state machine. Supports count prefixes (`5j`,
//! `10G`) and multi-key motions (`gg`, `[b`, `]b`). A 500 ms timeout clears
//! an ambiguous buffer (matches Vim's `timeoutlen`).

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use smallvec::SmallVec;

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
    ToggleVerseLayout,
}

pub struct KeyState {
    pending: SmallVec<[KeyEvent; 4]>,
    count: u16,
    last: Option<Instant>,
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
        }
    }

    pub fn tick(&mut self) {
        if let Some(t) = self.last {
            if t.elapsed() > Duration::from_millis(500) {
                self.reset();
            }
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
        if self.pending.is_empty() && key.modifiers.is_empty() {
            if let KeyCode::Char(c) = key.code {
                if c.is_ascii_digit() && !(self.count == 0 && c == '0') {
                    self.count = self
                        .count
                        .saturating_mul(10)
                        .saturating_add(c.to_digit(10).unwrap() as u16);
                    self.last = Some(Instant::now());
                    return None;
                }
            }
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
        if self.count == 0 {
            default
        } else {
            self.count
        }
    }

    fn try_resolve(&self) -> Resolve {
        let n = self.pending.len();
        let first = self.pending[0];
        if n == 1 {
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
