//! Chord + count state for list-dialog vim motions. The Bookmarks,
//! Translations, and Footnote dialogs all want the same `5j` / `gg` / `5G`
//! feel — owning the bookkeeping here keeps each dialog's handler a flat
//! match on the dialog-specific keys (Enter, Esc, d, …).

use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct ListNav {
    count: u16,
    pending_g: bool,
}

pub enum Step {
    /// Advance the cursor by this many rows (always >= 1).
    Down(u16),
    /// Retreat the cursor by this many rows (always >= 1).
    Up(u16),
    /// Jump to row 0.
    Top,
    /// `0` raw count means "go to last row". A nonzero count means "jump to
    /// 1-based row index `n`" (vim's `10G`); caller clamps to length.
    BottomOrAt(u16),
    /// Key was consumed into the chord/count state — no cursor change this
    /// tick. The dialog should treat it as a no-op (return Continue).
    Pending,
    /// Not ours — caller handles dialog-specific keys (Enter, Esc, etc.).
    Pass,
}

impl ListNav {
    /// Interpret `key` as a vim-style list motion. Anything not recognized
    /// returns [`Step::Pass`] and clears the pending chord/count state so
    /// `g` followed by Enter doesn't leave a stale `pending_g`.
    pub fn handle(&mut self, key: KeyEvent) -> Step {
        match key.code {
            // Digit accumulation. A bare leading `0` is not a count (matches
            // vim's "0 = start of line" carve-out) — we let it fall through.
            KeyCode::Char(c) if c.is_ascii_digit() && !(self.count == 0 && c == '0') => {
                self.count = self
                    .count
                    .saturating_mul(10)
                    .saturating_add(c.to_digit(10).unwrap() as u16);
                Step::Pending
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.consume_count();
                Step::Down(n)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.consume_count();
                Step::Up(n)
            }
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.reset();
                    Step::Top
                } else {
                    self.pending_g = true;
                    Step::Pending
                }
            }
            KeyCode::Char('G') => {
                let n = self.count;
                self.reset();
                Step::BottomOrAt(n)
            }
            _ => {
                // Any unrecognized key clears the pending state — matches
                // vim's chord-reset behavior so `g<Esc>` doesn't linger.
                self.reset();
                Step::Pass
            }
        }
    }

    fn consume_count(&mut self) -> u16 {
        let n = if self.count == 0 { 1 } else { self.count };
        self.reset();
        n
    }

    fn reset(&mut self) {
        self.count = 0;
        self.pending_g = false;
    }
}

/// Helper for `BottomOrAt` — turn a raw count into a 0-based cursor index,
/// clamped to `len`. Returns `None` when the list is empty.
pub fn bottom_or_at(raw: u16, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if raw == 0 {
        Some(len - 1)
    } else {
        Some((raw as usize).saturating_sub(1).min(len - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn ev(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
    }

    #[test]
    fn single_g_is_pending() {
        let mut n = ListNav::default();
        assert!(matches!(n.handle(ev('g')), Step::Pending));
    }

    #[test]
    fn gg_goes_top() {
        let mut n = ListNav::default();
        n.handle(ev('g'));
        assert!(matches!(n.handle(ev('g')), Step::Top));
    }

    #[test]
    fn count_then_j_steps_n() {
        let mut n = ListNav::default();
        n.handle(ev('5'));
        match n.handle(ev('j')) {
            Step::Down(5) => {}
            other => panic!("expected Down(5), got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn unrecognized_clears_pending() {
        let mut n = ListNav::default();
        n.handle(ev('g'));
        // Anything not a motion clears the pending `g`.
        assert!(matches!(n.handle(ev('x')), Step::Pass));
        // Subsequent single `g` must be Pending again, not Top.
        assert!(matches!(n.handle(ev('g')), Step::Pending));
    }

    #[test]
    fn bare_zero_does_not_count() {
        let mut n = ListNav::default();
        // First press of `0` is not a count — it falls through.
        assert!(matches!(n.handle(ev('0')), Step::Pass));
        // After a nonzero digit, subsequent `0` IS a count digit.
        n.handle(ev('1'));
        assert!(matches!(n.handle(ev('0')), Step::Pending));
        match n.handle(ev('j')) {
            Step::Down(10) => {}
            other => panic!(
                "expected Down(10), got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn bottom_or_at_clamps() {
        assert_eq!(bottom_or_at(0, 5), Some(4));
        assert_eq!(bottom_or_at(1, 5), Some(0));
        assert_eq!(bottom_or_at(3, 5), Some(2));
        assert_eq!(bottom_or_at(99, 5), Some(4));
        assert_eq!(bottom_or_at(0, 0), None);
    }
}
