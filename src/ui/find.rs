//! Find dialog (F3 / `/`). FTS5 search with live results.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::{Book, Db};
use crate::nav::Position;
use crate::search::{self, SearchHit};
use crate::theme;
use crate::ui::dialog;

/// Each hit renders as three rows: reference, indented snippet, separator
/// blank. The blank row in between gives the eye somewhere to rest so
/// scanning a long list doesn't feel like a wall of text.
const ROWS_PER_HIT: usize = 3;

/// Fixed chrome above and below the hit list: 1 top blank + 1 input row +
/// 1 hint/blank under input + 1 separator blank + 1 footer row = 5 rows
/// that the hit list can't use. Update both this and the layout above the
/// hits loop together.
const CHROME_ROWS: usize = 5;

pub struct FindDialog {
    input: String,
    results: Vec<SearchHit>,
    selected: usize,
    error: Option<String>,
}

#[non_exhaustive]
pub enum FindOutcome {
    Continue,
    Cancel,
    Jump(Position, String), // position + the query that produced the hit, for n/N
}

impl FindDialog {
    pub const fn new() -> Self {
        Self {
            input: String::new(),
            results: Vec::new(),
            selected: 0,
            error: None,
        }
    }

    pub fn handle(&mut self, key: KeyEvent, db: &Db) -> FindOutcome {
        match key.code {
            KeyCode::Esc => FindOutcome::Cancel,
            KeyCode::Enter => {
                if let Some(hit) = self.results.get(self.selected) {
                    FindOutcome::Jump(
                        Position {
                            book: hit.book.clone(),
                            chapter: hit.chapter,
                            verse: Some(hit.verse),
                        },
                        self.input.clone(),
                    )
                } else {
                    FindOutcome::Continue
                }
            }
            KeyCode::Down => {
                if !self.results.is_empty() {
                    self.selected = (self.selected + 1).min(self.results.len() - 1);
                }
                FindOutcome::Continue
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                FindOutcome::Continue
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.refresh(db);
                FindOutcome::Continue
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                self.refresh(db);
                FindOutcome::Continue
            }
            _ => FindOutcome::Continue,
        }
    }

    fn refresh(&mut self, db: &Db) {
        self.selected = 0;
        if self.input.trim().is_empty() {
            self.results.clear();
            self.error = None;
            return;
        }
        match search::search(db, db.translation(), &self.input, 50) {
            Ok(rows) => {
                self.results = rows;
                self.error = None;
            }
            Err(e) => {
                self.results.clear();
                self.error = Some(format!("{e}"));
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the body lays out the whole dialog inline (input row, error \
                  row, hit rows with two-line cells, empty-state, footer); \
                  decomposing would force callers to assemble the dialog \
                  themselves with no gain."
    )]
    pub fn render(&self, outer: Rect, buf: &mut Buffer, books: &[Book]) {
        let w: u16 = outer.width.saturating_sub(6).min(90);
        let h: u16 = outer.height.saturating_sub(4).min(22);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_dialog(area, "Find", buf);

        let bg = Style::new().bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let input_style = Style::new()
            .fg(theme::black())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let hit_style = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let ref_style = Style::new().fg(theme::cyan()).bg(theme::blue());
        let sel_bg = Style::new()
            .fg(theme::bright_white())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::with_capacity(inner.height as usize);
        lines.push(blank());
        lines.push(Line::from(vec![
            Span::styled("  Find: ", label),
            Span::styled(self.input.clone(), input_style),
            Span::styled("\u{2588}", input_style.fg(theme::bright_white())),
        ]));
        // Empty-state hint under the input — only shown before the user types.
        // Matches the Goto dialog's pattern so the two commands feel symmetric.
        if self.input.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "\u{2192} (type to search, e.g. \"love\", \"kingdom of God\")",
                    Style::new()
                        .fg(theme::yellow())
                        .bg(theme::blue())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(blank());
        }
        lines.push(blank());

        if let Some(err) = &self.error {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    err.clone(),
                    Style::new()
                        .fg(theme::hotkey_red())
                        .bg(theme::blue())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        let max_hits = (inner.height as usize).saturating_sub(CHROME_ROWS) / ROWS_PER_HIT;
        for (i, hit) in self.results.iter().enumerate().take(max_hits) {
            let book_label = books
                .iter()
                .find(|b| b.code == hit.book)
                .map_or_else(|| hit.book.clone(), |b| b.abbreviation.clone());
            let reference = format!(" {} {}:{} ", book_label, hit.chapter, hit.verse);
            let on = i == self.selected;
            let ref_line_style = if on { sel_bg } else { ref_style };

            // Row 1: reference, full-width selectable.
            let mut ref_spans: Vec<Span<'static>> = Vec::new();
            ref_spans.push(Span::styled(" ".to_string(), if on { sel_bg } else { bg }));
            ref_spans.push(Span::styled(reference.clone(), ref_line_style));
            let used = 1 + reference.chars().count();
            let pad = (inner.width as usize).saturating_sub(used);
            ref_spans.push(Span::styled(" ".repeat(pad), if on { sel_bg } else { bg }));
            lines.push(Line::from(ref_spans));

            // Row 2: indented snippet with highlighted match ranges.
            let mut snip_spans: Vec<Span<'static>> = Vec::new();
            snip_spans.push(Span::styled(
                "    ".to_string(),
                if on { sel_bg } else { bg },
            ));
            let mut cursor = 0;
            for &(s, e) in &hit.hits {
                if s > cursor {
                    snip_spans.push(Span::styled(
                        hit.text[cursor..s].to_string(),
                        if on { sel_bg } else { label },
                    ));
                }
                snip_spans.push(Span::styled(
                    hit.text[s..e].to_string(),
                    if on { sel_bg } else { hit_style },
                ));
                cursor = e;
            }
            if cursor < hit.text.len() {
                snip_spans.push(Span::styled(
                    hit.text[cursor..].to_string(),
                    if on { sel_bg } else { label },
                ));
            }
            lines.push(Line::from(snip_spans));

            // Row 3: separator gap.
            lines.push(blank());
        }

        if self.results.is_empty() && self.error.is_none() && !self.input.trim().is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "(no matches)",
                    Style::new()
                        .fg(theme::light_grey())
                        .bg(theme::blue())
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Footer
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(Line::from(vec![
            Span::styled(
                "  Enter ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "jump   ",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
            Span::styled(
                "↑↓ ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "navigate   ",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
            Span::styled(
                "Esc ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "cancel",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}
