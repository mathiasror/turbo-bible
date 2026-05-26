//! Bookmarks list dialog (F4). Shows saved bookmarks across the whole DB,
//! sorted in canon order. `j`/`k` navigate, `Enter` jumps, `d` deletes.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::bookmark::{Bookmark, BookmarkStore};
use crate::db::Book;
use crate::nav::Position;
use crate::theme;
use crate::ui::dialog;
use crate::ui::listnav::{self, ListNav, Step};

pub struct BookmarksDialog {
    items: Vec<Bookmark>,
    cursor: usize,
    nav: ListNav,
}

#[non_exhaustive]
pub enum BookmarksOutcome {
    Continue,
    Cancel,
    Jump(Position),
    Delete(Bookmark),
}

impl BookmarksDialog {
    pub fn new(store: &BookmarkStore) -> Self {
        // Just clone in canon-of-creation order for v1; sorting against the
        // canon would need the book list. Caller can pass sorted later.
        Self {
            items: store.bookmarks.clone(),
            cursor: 0,
            nav: ListNav::default(),
        }
    }

    pub fn sort_canonical(&mut self, books: &[Book]) {
        self.items.sort_by_key(|b| {
            let ord = books
                .iter()
                .position(|book| book.code == b.book)
                .unwrap_or(usize::MAX);
            (ord, b.chapter, b.start_verse)
        });
    }

    pub fn handle(&mut self, key: KeyEvent) -> BookmarksOutcome {
        match self.nav.handle(key) {
            Step::Down(n) => {
                if !self.items.is_empty() {
                    self.cursor = (self.cursor + n as usize).min(self.items.len() - 1);
                }
                return BookmarksOutcome::Continue;
            }
            Step::Up(n) => {
                self.cursor = self.cursor.saturating_sub(n as usize);
                return BookmarksOutcome::Continue;
            }
            Step::Top => {
                self.cursor = 0;
                return BookmarksOutcome::Continue;
            }
            Step::BottomOrAt(n) => {
                if let Some(idx) = listnav::bottom_or_at(n, self.items.len()) {
                    self.cursor = idx;
                }
                return BookmarksOutcome::Continue;
            }
            Step::Pending => return BookmarksOutcome::Continue,
            Step::Pass => {}
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => BookmarksOutcome::Cancel,
            KeyCode::Enter | KeyCode::Char('o') => {
                self.items
                    .get(self.cursor)
                    .map_or(BookmarksOutcome::Continue, |b| {
                        BookmarksOutcome::Jump(Position {
                            book: b.book.clone(),
                            chapter: b.chapter,
                            verse: Some(b.start_verse),
                        })
                    })
            }
            KeyCode::Char('d' | 'x') | KeyCode::Delete => {
                if let Some(b) = self.items.get(self.cursor).cloned() {
                    let drop = self.cursor.min(self.items.len().saturating_sub(1));
                    self.items.remove(drop);
                    if self.cursor >= self.items.len() && !self.items.is_empty() {
                        self.cursor = self.items.len() - 1;
                    }
                    BookmarksOutcome::Delete(b)
                } else {
                    BookmarksOutcome::Continue
                }
            }
            _ => BookmarksOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer, books: &[Book]) {
        let w: u16 = outer.width.saturating_sub(6).min(80);
        let h: u16 = outer.height.saturating_sub(4).min(24);
        let area = dialog::center(outer, w, h);
        let title = format!("Bookmarks ({})", self.items.len());
        let inner = dialog::draw_modal_dialog(outer, area, &title, buf);

        let bg = Style::new().bg(theme::blue());
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::list_focus_bg())
            .add_modifier(Modifier::BOLD);
        let header = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);

        let inner_w = inner.width as usize;
        let blank = || Line::from(Span::styled(" ".repeat(inner_w), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        // Subtitle directly below the title: the saved-verse count, pluralized.
        // Completes the dialog's hierarchy so the populated list doesn't read
        // as a sparse field of empty blue.
        if !self.items.is_empty() {
            let noun = if self.items.len() == 1 {
                "verse"
            } else {
                "verses"
            };
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(format!("{} saved {noun}", self.items.len()), dim),
            ]));
        }
        lines.push(blank());

        if self.items.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "(no bookmarks yet — press ",
                    dim.add_modifier(Modifier::ITALIC),
                ),
                Span::styled("b ", header),
                Span::styled("on a verse to add)", dim.add_modifier(Modifier::ITALIC)),
            ]));
        } else {
            let visible = (inner.height as usize)
                .saturating_sub(lines.len())
                .saturating_sub(2);
            let scroll = (self.cursor + 1).saturating_sub(visible);
            for (i, b) in self.items.iter().enumerate().skip(scroll).take(visible) {
                let book_name = books
                    .iter()
                    .find(|bk| bk.code == b.book)
                    .map_or(b.book.as_str(), super::super::db::Book::display_name);
                let on = i == self.cursor;
                let mark = if on { "  \u{25B8} " } else { "    " };
                let ref_str = b.reference_label(book_name);
                let used = mark.chars().count() + ref_str.chars().count();
                let pad_right = inner_w.saturating_sub(used);
                let style = if on { sel } else { label };
                let mark_style = if on { sel } else { dim };
                let pad_style = if on { sel } else { bg };
                lines.push(Line::from(vec![
                    Span::styled(mark.to_string(), mark_style),
                    Span::styled(ref_str, style),
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
            Span::styled("jump  ", dim),
            Span::styled("d ", key_style),
            Span::styled("delete  ", dim),
            Span::styled("\u{2191}\u{2193}/j k ", key_style),
            Span::styled("navigate  ", dim),
            Span::styled("Esc ", key_style),
            Span::styled("close", dim),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}
