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

pub enum HelpOutcome {
    Continue,
    Cancel,
}

impl HelpDialog {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&self, key: KeyEvent) -> HelpOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::F(1) => {
                HelpOutcome::Cancel
            }
            _ => HelpOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w: u16 = outer.width.saturating_sub(6).min(64);
        let h: u16 = outer.height.saturating_sub(4).min(22);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_dialog(area, "Help — Bible TUI", buf);

        let bg = Style::new().bg(theme::BLUE);
        let label = Style::new().fg(theme::BRIGHT_WHITE).bg(theme::BLUE);
        let key = Style::new()
            .fg(theme::YELLOW)
            .bg(theme::BLUE)
            .add_modifier(Modifier::BOLD);
        let header = Style::new()
            .fg(theme::CYAN)
            .bg(theme::BLUE)
            .add_modifier(Modifier::BOLD);

        let entries: &[(&str, &str)] = &[
            ("h H ←", "previous chapter"),
            ("l L →", "next chapter"),
            ("[b ]b", "previous / next book"),
            ("j ↓", "next verse (cursor)"),
            ("k ↑", "previous verse"),
            ("Ctrl-D / Ctrl-U", "half-page down / up"),
            ("Ctrl-F / Ctrl-B / Space", "page down / up"),
            ("gg / G", "first / last verse"),
            ("Ctrl-O / Ctrl-I", "jump back / forward"),
            ("F2 / :", "Goto reference"),
            ("F3 / /", "Find (FTS5 search)"),
            ("K", "Footnote popup at cursor"),
            ("Tab", "toggle References sidebar"),
            ("y", "copy verse to clipboard"),
            ("F1", "this help"),
            ("q / Esc", "quit"),
        ];

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());
        lines.push(Line::from(vec![
            Span::styled("  ", bg),
            Span::styled("Keybindings", header),
        ]));
        lines.push(blank());
        for (k, desc) in entries {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(format!("{:<24}", k), key),
                Span::styled(desc.to_string(), label),
            ]));
        }
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(Line::from(vec![
            Span::styled("  ", bg),
            Span::styled(
                "Esc / Enter ",
                Style::new()
                    .fg(theme::BRIGHT_WHITE)
                    .bg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "close",
                Style::new().fg(theme::LIGHT_GREY).bg(theme::BLUE),
            ),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}
