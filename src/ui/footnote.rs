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
use crate::ui::listnav::{self, ListNav, Step};

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
    nav: ListNav,
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
            nav: ListNav::default(),
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> FootnoteOutcome {
        match self.nav.handle(key) {
            Step::Down(n) => {
                if !self.xrefs.is_empty() {
                    self.selected = (self.selected + n as usize).min(self.xrefs.len() - 1);
                }
                return FootnoteOutcome::Continue;
            }
            Step::Up(n) => {
                self.selected = self.selected.saturating_sub(n as usize);
                return FootnoteOutcome::Continue;
            }
            Step::Top => {
                self.selected = 0;
                return FootnoteOutcome::Continue;
            }
            Step::BottomOrAt(n) => {
                if let Some(idx) = listnav::bottom_or_at(n, self.xrefs.len()) {
                    self.selected = idx;
                }
                return FootnoteOutcome::Continue;
            }
            Step::Pending => return FootnoteOutcome::Continue,
            Step::Pass => {}
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => FootnoteOutcome::Cancel,
            KeyCode::Enter => {
                if let Some(item) = self.xrefs.get(self.selected) {
                    FootnoteOutcome::Jump(item.target.clone())
                } else {
                    FootnoteOutcome::Continue
                }
            }
            _ => FootnoteOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer, _books: &[Book]) {
        let empty = self.footnotes.is_empty();
        let w: u16 = if empty {
            outer.width.saturating_sub(6).min(50)
        } else {
            outer.width.saturating_sub(6).min(80)
        };
        // Empty-state dialog shrinks to ~5 rows so it doesn't read as a render
        // failure. Populated dialog gets the full 22-row max.
        let h: u16 = if empty {
            outer.height.saturating_sub(4).min(5)
        } else {
            outer.height.saturating_sub(4).min(22)
        };
        let area = dialog::center(outer, w, h);
        let title = format!("Notes for {}", self.verse_label);
        let inner = dialog::draw_dialog(area, &title, buf);

        let bg = Style::new().bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let body_style = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let header_style = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let xref_color = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::UNDERLINED);

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());

        // Footnote bodies.
        for (fi, fn_) in self.footnotes.iter().enumerate() {
            let kind = if fn_.kind == "x" {
                "Cross-ref"
            } else {
                "Footnote"
            };
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(format!("{kind}:"), header_style),
            ]));
            lines.push(Line::from(vec![
                Span::styled("    ", bg),
                Span::styled(fn_.body.clone(), body_style),
            ]));
            // Cross-refs inside this note as selectable items.
            for (xi, xref) in self
                .xrefs
                .iter()
                .enumerate()
                .filter(|(_, x)| x.footnote_idx == fi)
            {
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
                        .fg(theme::light_grey())
                        .bg(theme::blue())
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Footer — only advertise navigation/Enter when there's something to
        // navigate; otherwise just Esc close so the footer doesn't promise
        // actions the empty body can't deliver.
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let footer = if empty {
            vec![
                Span::styled("  ", bg),
                Span::styled("Esc ", key_style),
                Span::styled("close", dim),
            ]
        } else {
            vec![
                Span::styled("  ", bg),
                Span::styled("Enter ", key_style),
                Span::styled("follow xref   ", dim),
                Span::styled("\u{2191}\u{2193} ", key_style),
                Span::styled("navigate   ", dim),
                Span::styled("Esc ", key_style),
                Span::styled("close", dim),
            ]
        };
        lines.push(Line::from(footer));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}

/// OSIS string "BOOK.CHAP[.VERSE]" → Position. Returns None on parse error.
pub fn parse_osis(s: &str) -> Option<Position> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let chapter: i64 = parts[1].parse().ok()?;
    let verse: Option<i64> = parts.get(2).and_then(|v| v.parse().ok());
    Some(Position {
        book: parts[0].to_string(),
        chapter,
        verse,
    })
}
