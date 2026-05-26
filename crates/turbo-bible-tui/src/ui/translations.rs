//! Translations picker dialog (t / F5). Lists every translation the
//! binary knows about; installed entries show a ✓ marker, downloadable
//! ones show the compressed size. Enter on an installed translation
//! swaps; Enter on a downloadable one triggers a fetch first.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;
use crate::ui::dialog;
use crate::ui::listnav::{self, ListNav, Step};

/// One row in the translations picker. `installed` is `true` when the
/// `<code>.db` is already extracted into the user's translations dir;
/// `false` means the user needs to opt in to a download.
pub struct PickerEntry {
    pub code: String,
    pub name: String,
    pub language: String,
    pub installed: bool,
    /// Compressed size in bytes, used to render the `[↓ X MB]` hint
    /// for non-installed entries.
    pub compressed_size: u64,
}

pub struct TranslationsDialog {
    items: Vec<PickerEntry>,
    cursor: usize,
    nav: ListNav,
}

#[non_exhaustive]
pub enum TranslationsOutcome {
    Continue,
    Cancel,
    /// User picked an installed code; caller swaps translation immediately.
    Select(String),
    /// User picked a code that isn't installed yet; caller should
    /// fetch it (and switch on success).
    Download(String),
}

impl TranslationsDialog {
    pub fn new(items: Vec<PickerEntry>, current: &str) -> Self {
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
            KeyCode::Enter | KeyCode::Char('o') => {
                self.items
                    .get(self.cursor)
                    .map_or(TranslationsOutcome::Continue, |t| {
                        if t.installed {
                            TranslationsOutcome::Select(t.code.clone())
                        } else {
                            TranslationsOutcome::Download(t.code.clone())
                        }
                    })
            }
            _ => TranslationsOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w: u16 = outer.width.saturating_sub(6).min(76);
        let h: u16 = outer.height.saturating_sub(4).min(16);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_modal_dialog(outer, area, "Translations", buf);

        let bg = Style::new().bg(theme::blue());
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let hint = Style::new().fg(theme::yellow()).bg(theme::blue());
        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let sel_hint = Style::new()
            .fg(theme::yellow())
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
                    "(no translations known)",
                    dim.add_modifier(Modifier::ITALIC),
                ),
            ]));
        } else {
            let visible = (inner.height as usize)
                .saturating_sub(lines.len())
                .saturating_sub(3);
            let scroll = (self.cursor + 1).saturating_sub(visible);
            for (i, t) in self.items.iter().enumerate().skip(scroll).take(visible) {
                let on = i == self.cursor;
                let mark = if on {
                    "  \u{25B8} "
                } else if t.installed {
                    "  \u{2713} "
                } else {
                    "    "
                };
                let code_w = 14usize;
                let lang_w = 4usize;
                let code_field = format!("{:<w$}", t.code, w = code_w);
                let lang_field = format!("{:<w$}", t.language, w = lang_w);
                let name_field = t.name.clone();
                let suffix = if t.installed {
                    String::new()
                } else {
                    format!("  [\u{2193} {}]", human_size(t.compressed_size))
                };

                let used = mark.chars().count()
                    + code_field.chars().count()
                    + lang_field.chars().count()
                    + name_field.chars().count()
                    + suffix.chars().count();
                let pad_right = inner_w.saturating_sub(used);

                let row_style = if on { sel } else { label };
                let mark_style = if on { sel } else { dim };
                let lang_style = if on { sel } else { dim };
                let suffix_style = if on { sel_hint } else { hint };
                let pad_style = if on { sel } else { bg };

                lines.push(Line::from(vec![
                    Span::styled(mark.to_string(), mark_style),
                    Span::styled(code_field, row_style),
                    Span::styled(lang_field, lang_style),
                    Span::styled(name_field, row_style),
                    Span::styled(suffix, suffix_style),
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
            Span::styled("select / download  ", dim),
            Span::styled("\u{2191}\u{2193}/j k ", key_style),
            Span::styled("navigate  ", dim),
            Span::styled("Esc ", key_style),
            Span::styled("close", dim),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}
