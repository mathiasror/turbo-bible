//! Translations picker dialog (t / F5). Lists every row in the `translation`
//! table; Enter swaps the active translation, Esc cancels. Modeled on the
//! bookmarks dialog.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::TranslationInfo;
use crate::theme;
use crate::ui::dialog;

pub struct TranslationsDialog {
    items: Vec<TranslationInfo>,
    cursor: usize,
}

pub enum TranslationsOutcome {
    Continue,
    Cancel,
    /// User picked this code; caller swaps `db.translation` and persists.
    Select(String),
}

impl TranslationsDialog {
    pub fn new(items: Vec<TranslationInfo>, current: &str) -> Self {
        let cursor = items
            .iter()
            .position(|t| t.code == current)
            .unwrap_or(0);
        Self { items, cursor }
    }

    pub fn handle(&mut self, key: KeyEvent) -> TranslationsOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => TranslationsOutcome::Cancel,
            KeyCode::Enter | KeyCode::Char('o') => self
                .items
                .get(self.cursor)
                .map(|t| TranslationsOutcome::Select(t.code.clone()))
                .unwrap_or(TranslationsOutcome::Continue),
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.items.is_empty() {
                    self.cursor = (self.cursor + 1).min(self.items.len() - 1);
                }
                TranslationsOutcome::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                TranslationsOutcome::Continue
            }
            KeyCode::Char('g') => {
                self.cursor = 0;
                TranslationsOutcome::Continue
            }
            KeyCode::Char('G') => {
                if !self.items.is_empty() {
                    self.cursor = self.items.len() - 1;
                }
                TranslationsOutcome::Continue
            }
            _ => TranslationsOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w: u16 = outer.width.saturating_sub(6).min(72);
        let h: u16 = outer.height.saturating_sub(4).min(14);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_dialog(area, "Translations", buf);

        let bg = Style::new().bg(theme::blue());
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);

        let inner_w = inner.width as usize;
        let blank = || Line::from(Span::styled(" ".repeat(inner_w), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());

        if self.items.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled("(no translations installed)", dim.add_modifier(Modifier::ITALIC)),
            ]));
        } else {
            let visible = (inner.height as usize)
                .saturating_sub(lines.len())
                .saturating_sub(2);
            let scroll = if self.cursor + 1 > visible {
                (self.cursor + 1) - visible
            } else {
                0
            };
            for (i, t) in self.items.iter().enumerate().skip(scroll).take(visible) {
                let on = i == self.cursor;
                let mark = if on { "  \u{25B8} " } else { "    " };
                let code_w = 12usize;
                let lang_w = 4usize;
                let code_field = format!("{:<w$}", t.code, w = code_w);
                let lang_field = format!("{:<w$}", t.language, w = lang_w);
                let name_field = t.name.clone();

                let used = mark.chars().count()
                    + code_field.chars().count()
                    + lang_field.chars().count()
                    + name_field.chars().count();
                let pad_right = inner_w.saturating_sub(used);

                let row_style = if on { sel } else { label };
                let mark_style = if on { sel } else { dim };
                let lang_style = if on { sel } else { dim };
                let pad_style = if on { sel } else { bg };

                lines.push(Line::from(vec![
                    Span::styled(mark.to_string(), mark_style),
                    Span::styled(code_field, row_style),
                    Span::styled(lang_field, lang_style),
                    Span::styled(name_field, row_style),
                    Span::styled(" ".repeat(pad_right), pad_style),
                ]));
            }
        }

        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(blank());
        lines.push(Line::from(vec![
            Span::styled("  ", bg),
            Span::styled("Enter ", key_style),
            Span::styled("select  ", dim),
            Span::styled("\u{2191}\u{2193}/j k ", key_style),
            Span::styled("navigate  ", dim),
            Span::styled("Esc ", key_style),
            Span::styled("close", dim),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}
