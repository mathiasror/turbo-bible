//! Footnote popup (K). Shows every footnote attached to a given verse, lets
//! the user navigate cross-references with ↑/↓ and follow them with Enter.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::{Book, Footnote};
use crate::nav::Position;
use crate::theme;
use crate::ui::dialog;

#[derive(Clone)]
pub struct XrefItem {
    pub target: Position,
    pub label: String,
    pub footnote_idx: usize,
}

pub struct FootnoteDialog {
    pub verse_label: String,
    pub footnotes: Vec<Footnote>,
    pub xrefs: Vec<XrefItem>,
    pub selected: usize,
}

pub enum FootnoteOutcome {
    Continue,
    Cancel,
    Jump(Position),
}

impl FootnoteDialog {
    pub fn new(verse_label: String, footnotes: Vec<Footnote>) -> Self {
        let mut xrefs = Vec::new();
        for (fi, fn_) in footnotes.iter().enumerate() {
            for xr in &fn_.refs {
                if let Some(pos) = parse_osis(&xr.target_osis) {
                    xrefs.push(XrefItem {
                        target: pos,
                        label: xr.label.clone(),
                        footnote_idx: fi,
                    });
                }
            }
        }
        Self {
            verse_label,
            footnotes,
            xrefs,
            selected: 0,
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> FootnoteOutcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => FootnoteOutcome::Cancel,
            KeyCode::Enter => {
                if let Some(item) = self.xrefs.get(self.selected) {
                    FootnoteOutcome::Jump(item.target.clone())
                } else {
                    FootnoteOutcome::Continue
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.xrefs.is_empty() {
                    self.selected = (self.selected + 1).min(self.xrefs.len() - 1);
                }
                FootnoteOutcome::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                FootnoteOutcome::Continue
            }
            _ => FootnoteOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer, _books: &[Book]) {
        let w: u16 = outer.width.saturating_sub(6).min(80);
        let h: u16 = outer.height.saturating_sub(4).min(22);
        let area = dialog::center(outer, w, h);
        let title = format!("Notes for {}", self.verse_label);
        let inner = dialog::draw_dialog(area, &title, buf);

        let bg = Style::new().bg(theme::BLUE);
        let label = Style::new().fg(theme::BRIGHT_WHITE).bg(theme::BLUE);
        let body_style = Style::new().fg(theme::LIGHT_GREY).bg(theme::BLUE);
        let header_style = Style::new()
            .fg(theme::YELLOW)
            .bg(theme::BLUE)
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::BRIGHT_WHITE)
            .bg(theme::CYAN)
            .add_modifier(Modifier::BOLD);
        let xref_color = Style::new()
            .fg(theme::YELLOW)
            .bg(theme::BLUE)
            .add_modifier(Modifier::UNDERLINED);

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());

        // Footnote bodies.
        for (fi, fn_) in self.footnotes.iter().enumerate() {
            let kind = if fn_.kind == "x" { "Cross-ref" } else { "Footnote" };
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(format!("{kind}:"), header_style),
            ]));
            lines.push(Line::from(vec![
                Span::styled("    ", bg),
                Span::styled(fn_.body.clone(), body_style),
            ]));
            // Cross-refs inside this note as selectable items.
            for (xi, xref) in self.xrefs.iter().enumerate().filter(|(_, x)| x.footnote_idx == fi) {
                let style = if xi == self.selected { sel } else { xref_color };
                lines.push(Line::from(vec![
                    Span::styled("      \u{2192} ", label),
                    Span::styled(xref.label.clone(), style),
                ]));
            }
            lines.push(blank());
        }

        if self.footnotes.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "(no footnotes on this verse)",
                    Style::new()
                        .fg(theme::LIGHT_GREY)
                        .bg(theme::BLUE)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Footer.
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(Line::from(vec![
            Span::styled("  ", bg),
            Span::styled(
                "Enter ",
                Style::new()
                    .fg(theme::BRIGHT_WHITE)
                    .bg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "follow xref   ",
                Style::new().fg(theme::LIGHT_GREY).bg(theme::BLUE),
            ),
            Span::styled(
                "↑↓ ",
                Style::new()
                    .fg(theme::BRIGHT_WHITE)
                    .bg(theme::BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "navigate   ",
                Style::new().fg(theme::LIGHT_GREY).bg(theme::BLUE),
            ),
            Span::styled(
                "Esc ",
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

/// OSIS string "BOOK.CHAP.VERSE" → (book, chapter). Verse is dropped (we jump
/// to chapter granularity in v1). Returns None on parse error.
pub fn parse_osis(s: &str) -> Option<Position> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let chapter: i64 = parts[1].parse().ok()?;
    Some(Position {
        book: parts[0].to_string(),
        chapter,
    })
}
