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
use crate::ui::listnav::{self, ListNav, Step};

pub struct TranslationsDialog {
    items: Vec<TranslationInfo>,
    cursor: usize,
    nav: ListNav,
}

#[non_exhaustive]
pub enum TranslationsOutcome {
    Continue,
    Cancel,
    /// User picked this code; caller swaps `db.translation` and persists.
    Select(String),
}

impl TranslationsDialog {
    pub fn new(items: Vec<TranslationInfo>, current: &str) -> Self {
        let cursor = items.iter().position(|t| t.code == current).unwrap_or(0);
        Self {
            items,
            cursor,
            nav: ListNav::default(),
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> TranslationsOutcome {
        match self.nav.handle(key) {
            Step::Down(n) => {
                if !self.items.is_empty() {
                    self.cursor = (self.cursor + n as usize).min(self.items.len() - 1);
                }
                return TranslationsOutcome::Continue;
            }
            Step::Up(n) => {
                self.cursor = self.cursor.saturating_sub(n as usize);
                return TranslationsOutcome::Continue;
            }
            Step::Top => {
                self.cursor = 0;
                return TranslationsOutcome::Continue;
            }
            Step::BottomOrAt(n) => {
                if let Some(idx) = listnav::bottom_or_at(n, self.items.len()) {
                    self.cursor = idx;
                }
                return TranslationsOutcome::Continue;
            }
            Step::Pending => return TranslationsOutcome::Continue,
            Step::Pass => {}
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => TranslationsOutcome::Cancel,
            KeyCode::Enter | KeyCode::Char('o') => self
                .items
                .get(self.cursor)
                .map_or(TranslationsOutcome::Continue, |t| {
                    TranslationsOutcome::Select(t.code.clone())
                }),
            _ => TranslationsOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w: u16 = outer.width.saturating_sub(6).min(72);
        let h: u16 = outer.height.saturating_sub(4).min(14);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_modal_dialog(outer, area, "Translations", buf);

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
                Span::styled(
                    "(no translations installed)",
                    dim.add_modifier(Modifier::ITALIC),
                ),
            ]));
        } else {
            let visible = (inner.height as usize)
                .saturating_sub(lines.len())
                .saturating_sub(2);
            let scroll = (self.cursor + 1).saturating_sub(visible);
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
