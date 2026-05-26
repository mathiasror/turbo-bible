//! Help dialog (F1) — keymap cheat sheet.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;
use crate::ui::dialog;

/// The keymap cheat sheet scrolls when the terminal is too short to show every
/// row. `scroll` is the top content row; `max_scroll` is recorded by `render`
/// (which alone knows the viewport height) so `handle` can clamp `G`/paging
/// without re-deriving the layout.
pub struct HelpDialog {
    scroll: usize,
    max_scroll: Cell<usize>,
}

#[non_exhaustive]
pub enum HelpOutcome {
    Continue,
    Cancel,
}

/// Row-type for the help table; either a section heading or a
/// `(keys, description)` entry. Module-level so `render` doesn't trip
/// `clippy::items_after_statements`.
enum Row {
    Section(&'static str),
    Entry(&'static str, &'static str),
}
use Row::{Entry, Section};

/// Canonical, source-of-truth help table. Lifted to module scope so a
/// unit test can walk it and assert removed keys (e.g. `T`) don't sneak
/// back in.
const ROWS: &[Row] = &[
    Section("Movement"),
    Entry("j  k  ↓ ↑", "next / previous verse"),
    Entry("h  l  ← →", "previous / next chapter"),
    Entry("[b  ]b", "previous / next book"),
    Entry("Ctrl-D  Ctrl-U", "half-page down / up"),
    Entry("Ctrl-F  Ctrl-B  Space", "page down / up"),
    Entry("gg  G", "first / last verse"),
    Entry("5j   10G", "count prefix (Vim-style)"),
    Entry("Ctrl-O  Ctrl-I", "jump back / forward in history"),
    Section("Selection & bookmarks"),
    Entry("v  V", "enter / exit visual selection"),
    Entry("b", "toggle bookmark on cursor / range"),
    Entry("y", "copy current verse to clipboard"),
    Section("Reading view"),
    Entry("Tab", "toggle References sidebar"),
    Entry("K", "footnote / cross-ref popup"),
    Section("Dialogs"),
    Entry("F1", "this help"),
    Entry("F2  :", "Goto reference  (e.g. John 3:16)"),
    Entry("F3  /", "Find  (FTS5 search)"),
    Entry("n  N", "repeat last find forward / backward"),
    Entry("F4  M", "Bookmarks"),
    Entry("F5  t", "Translations"),
    Section("Quit"),
    Entry("q  Esc  ZZ  ZQ  :q", "quit"),
];

/// Rows scrolled per half-page / Space, used by `handle` (which can't see the
/// real viewport height — `render` clamps the final value via `max_scroll`).
const PAGE_STEP: usize = 8;

/// Pick the top content row to actually render so a section header is never
/// the last visible body row with its entries below the fold ("keep with
/// next"). If the requested scroll would orphan a header at the bottom, nudge
/// down one row to pull the header's first entry into view. The result is
/// clamped to `[0, max_scroll]` and used for both the scroll offset and the
/// `▲`/`▼` indicator so the two always agree.
fn keep_with_next(requested: usize, body_h: usize, len: usize, is_section: &[bool]) -> usize {
    let max_scroll = len.saturating_sub(body_h);
    let scroll = requested.min(max_scroll);
    if body_h >= 2 {
        let bottom = scroll + body_h - 1;
        if bottom + 1 < len && is_section.get(bottom).copied().unwrap_or(false) {
            return (scroll + 1).min(max_scroll);
        }
    }
    scroll
}

impl HelpDialog {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            max_scroll: Cell::new(0),
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> HelpOutcome {
        let max = self.max_scroll.get();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::F(1) => {
                HelpOutcome::Cancel
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = (self.scroll + 1).min(max);
                HelpOutcome::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                HelpOutcome::Continue
            }
            KeyCode::Char('d') if ctrl => {
                self.scroll = (self.scroll + PAGE_STEP).min(max);
                HelpOutcome::Continue
            }
            KeyCode::Char('u') if ctrl => {
                self.scroll = self.scroll.saturating_sub(PAGE_STEP);
                HelpOutcome::Continue
            }
            KeyCode::Char(' ') | KeyCode::PageDown => {
                self.scroll = (self.scroll + PAGE_STEP).min(max);
                HelpOutcome::Continue
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(PAGE_STEP);
                HelpOutcome::Continue
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.scroll = 0;
                HelpOutcome::Continue
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.scroll = max;
                HelpOutcome::Continue
            }
            _ => HelpOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w: u16 = outer.width.saturating_sub(6).min(64);
        let h: u16 = outer.height.saturating_sub(4).min(30);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_modal_dialog(outer, area, "Help", buf);

        let bg = Style::new().bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let key = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        // Section headers: cyan but *not* bold. The bold-yellow key column is
        // the primary scan target; dropping bold here a half-step keeps the
        // headers as quiet grouping labels instead of buzzing against the keys.
        let header = Style::new().fg(theme::cyan()).bg(theme::blue());

        let mut content: Vec<Line<'static>> = Vec::new();
        // Parallel to `content`: marks which rows are section headers, so the
        // scroll logic can keep a header glued to its first entry.
        let mut is_section: Vec<bool> = Vec::new();
        content.push(Line::from(Span::styled(
            " ".repeat(inner.width as usize),
            bg,
        )));
        is_section.push(false); // leading blank
        for row in ROWS {
            match row {
                Section(name) => {
                    content.push(Line::from(vec![
                        Span::styled("  ", bg),
                        Span::styled((*name).to_string(), header),
                    ]));
                    is_section.push(true);
                }
                Entry(k, desc) => {
                    content.push(Line::from(vec![
                        Span::styled("    ", bg),
                        Span::styled(format!("{k:<22}"), key),
                        Span::styled((*desc).to_string(), label),
                    ]));
                    is_section.push(false);
                }
            }
        }

        // Pin the footer to the last inner row; everything above it scrolls.
        let body_h = inner.height.saturating_sub(1) as usize;
        let max_scroll = content.len().saturating_sub(body_h);
        self.max_scroll.set(max_scroll);
        // Never leave a section header stranded at the bottom of the viewport
        // with its entries below the fold (used for the offset *and* the
        // indicator below, so they agree).
        let scroll = keep_with_next(self.scroll, body_h, content.len(), &is_section);

        let body_area = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(1),
        );
        Paragraph::new(content)
            .style(bg)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(body_area, buf);

        // Footer: dismiss cue (always shown) + a right-aligned scroll indicator.
        let key_f = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let mut footer: Vec<Span<'static>> = vec![
            Span::styled("  ", bg),
            Span::styled("\u{2191}\u{2193}/j k ", key_f),
            Span::styled("scroll  ", dim),
            Span::styled("Esc / Enter ", key_f),
            Span::styled("close", dim),
        ];
        let mut indicator = String::new();
        if scroll > 0 {
            indicator.push('\u{25B2}');
        }
        if scroll < max_scroll {
            indicator.push('\u{25BC}');
        }
        if !indicator.is_empty() {
            let used: usize = footer.iter().map(|s| s.content.chars().count()).sum();
            let ind = format!(" {indicator} ");
            let pad = (inner.width as usize)
                .saturating_sub(used)
                .saturating_sub(ind.chars().count());
            if pad > 0 {
                footer.push(Span::styled(" ".repeat(pad), bg));
            }
            footer.push(Span::styled(ind, key_f));
        }
        let footer_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
        Paragraph::new(Line::from(footer))
            .style(bg)
            .render(footer_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens that have been removed from the runtime keymap. If you remove
    /// a binding from `src/keys.rs`, add it here so the help table can't
    /// silently lag behind.
    const REMOVED_KEYS: &[&str] = &["T"];

    #[test]
    fn keep_with_next_never_orphans_a_section_header() {
        // Build `is_section` exactly as `render` does: a leading blank, then
        // one flag per ROW.
        let mut is_section = vec![false];
        for row in ROWS {
            is_section.push(matches!(row, Section(_)));
        }
        let len = is_section.len();
        // Across every short viewport and every reachable scroll position, the
        // bottom visible row must never be a section header that still has
        // content beneath it — that's the "empty Quit section" artifact.
        for body_h in 2..=len {
            let max_scroll = len.saturating_sub(body_h);
            for requested in 0..=max_scroll {
                let scroll = keep_with_next(requested, body_h, len, &is_section);
                assert!(
                    scroll <= max_scroll,
                    "scroll {scroll} exceeds max {max_scroll}"
                );
                let bottom = scroll + body_h - 1;
                if bottom + 1 < len {
                    assert!(
                        !is_section[bottom],
                        "orphaned section header at row {bottom} \
                         (body_h={body_h}, requested={requested})"
                    );
                }
            }
        }
    }

    #[test]
    fn help_table_does_not_list_removed_keys() {
        for row in ROWS {
            if let Entry(keys, desc) = row {
                for token in keys.split(|c: char| c.is_whitespace() || c == ',') {
                    let token = token.trim();
                    if token.is_empty() {
                        continue;
                    }
                    assert!(
                        !REMOVED_KEYS.contains(&token),
                        "help row `{keys}` (= {desc}) still references removed key `{token}`",
                    );
                }
            }
        }
    }
}
