//! Help dialog (F1) — keymap cheat sheet.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;
use crate::ui::dialog;

pub struct HelpDialog;

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

impl HelpDialog {
    pub const fn new() -> Self {
        Self
    }

    pub const fn handle(key: KeyEvent) -> HelpOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::F(1) => {
                HelpOutcome::Cancel
            }
            _ => HelpOutcome::Continue,
        }
    }

    pub fn render(outer: Rect, buf: &mut Buffer) {
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
        let header = Style::new()
            .fg(theme::cyan())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        for row in ROWS {
            match row {
                Section(name) => {
                    lines.push(Line::from(vec![
                        Span::styled("  ", bg),
                        Span::styled(name.to_string(), header),
                    ]));
                }
                Entry(k, desc) => {
                    lines.push(Line::from(vec![
                        Span::styled("    ", bg),
                        Span::styled(format!("{k:<22}"), key),
                        Span::styled(desc.to_string(), label),
                    ]));
                }
            }
        }
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(Line::from(vec![
            Span::styled("  ", bg),
            Span::styled(
                "Esc / Enter ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "close",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
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
