//! Bookmarks list dialog (F4). Shows saved bookmarks across the whole DB,
//! sorted in canon order, each with a verse-text preview. `j`/`k` navigate,
//! `Enter` jumps, `d` deletes.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::bookmark::{Bookmark, BookmarkStore};
use crate::db::{Book, Db};
use crate::nav::Position;
use crate::theme;
use crate::ui::dialog;
use crate::ui::listnav::{self, ListNav, Step};

/// A bookmark plus its verse text in the active translation (`None` when the
/// reference doesn't resolve there), so each row can show a preview line.
struct Row {
    bm: Bookmark,
    preview: Option<String>,
}

impl Row {
    /// Screen rows this entry occupies: the reference line, plus a preview
    /// line when the verse text resolved.
    fn height(&self) -> usize {
        1 + usize::from(self.preview.is_some())
    }
}

pub struct BookmarksDialog {
    items: Vec<Row>,
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
    /// Build the dialog, loading each bookmark's verse text from the active
    /// translation for its preview line. A failed/absent lookup just drops the
    /// preview for that row (the reference still shows).
    pub fn new(store: &BookmarkStore, db: &Db) -> Self {
        let items = store
            .bookmarks
            .iter()
            .map(|bm| Row {
                preview: db
                    .verse_text(&bm.book, bm.chapter, bm.start_verse)
                    .ok()
                    .flatten(),
                bm: bm.clone(),
            })
            .collect();
        Self {
            items,
            cursor: 0,
            nav: ListNav::default(),
        }
    }

    pub fn sort_canonical(&mut self, books: &[Book]) {
        self.items.sort_by_key(|row| {
            let ord = books
                .iter()
                .position(|book| book.code == row.bm.book)
                .unwrap_or(usize::MAX);
            (ord, row.bm.chapter, row.bm.start_verse)
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
                    .map_or(BookmarksOutcome::Continue, |row| {
                        BookmarksOutcome::Jump(Position {
                            book: row.bm.book.clone(),
                            chapter: row.bm.chapter,
                            verse: Some(row.bm.start_verse),
                        })
                    })
            }
            KeyCode::Char('d' | 'x') | KeyCode::Delete => {
                if let Some(b) = self.items.get(self.cursor).map(|row| row.bm.clone()) {
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

    /// Pick the first visible item index so the cursor's cell fits within
    /// `avail` rows. Cells are 1–2 rows (reference + optional preview), so walk
    /// the origin forward until the cursor is inside the window. Cheap for the
    /// small lists bookmarks produce.
    fn scroll_origin(&self, avail: usize) -> usize {
        let mut origin = 0;
        while origin < self.cursor {
            let mut used = 0;
            let mut last = origin;
            for (i, row) in self.items.iter().enumerate().skip(origin) {
                if used + row.height() > avail {
                    break;
                }
                used += row.height();
                last = i;
            }
            if self.cursor <= last {
                break;
            }
            origin += 1;
        }
        origin
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer, books: &[Book]) {
        let w: u16 = outer.width.saturating_sub(6).min(80);
        // Adaptive height: size to content (subtitle + blank + cells + blank +
        // footer + 2 border rows), clamped to the terminal and a cap, so a
        // short list doesn't sit in a field of empty blue. Each cell is a
        // reference row plus a preview row when the verse text resolved.
        let cell_rows: usize = self.items.iter().map(Row::height).sum();
        let content_h = if self.items.is_empty() {
            3 // blank + empty-state + blank
        } else {
            2 + cell_rows + 1 // subtitle + blank + cells + trailing blank
        };
        let max_h = outer.height.saturating_sub(4).min(24);
        let h = u16::try_from(content_h + 1 + 2) // + footer + top/bottom borders
            .unwrap_or(max_h)
            .clamp(7, max_h);
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
        // Preview text on the selected slab: white but not bold, so the
        // reference stays the louder line within the highlighted cell.
        let sel_preview = Style::new()
            .fg(theme::bright_white())
            .bg(theme::list_focus_bg());
        let header = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);

        let inner_w = inner.width as usize;
        let blank = || Line::from(Span::styled(" ".repeat(inner_w), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
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
            // Rows for cells = inner height − used-so-far − (blank + footer).
            let avail = (inner.height as usize)
                .saturating_sub(lines.len())
                .saturating_sub(2);
            let scroll = self.scroll_origin(avail);
            let mut used = 0usize;
            for (i, row) in self.items.iter().enumerate().skip(scroll) {
                if used + row.height() > avail {
                    break;
                }
                used += row.height();

                let book_name = books
                    .iter()
                    .find(|bk| bk.code == row.bm.book)
                    .map_or(row.bm.book.as_str(), Book::display_name);
                let on = i == self.cursor;
                let mark = if on { "  \u{25B8} " } else { "    " };
                let ref_str = row.bm.reference_label(book_name);
                let ref_used = mark.chars().count() + ref_str.chars().count();
                let style = if on { sel } else { label };
                let mark_style = if on { sel } else { dim };
                let slab = if on { sel } else { bg };
                lines.push(Line::from(vec![
                    Span::styled(mark.to_string(), mark_style),
                    Span::styled(ref_str, style),
                    Span::styled(" ".repeat(inner_w.saturating_sub(ref_used)), slab),
                ]));

                // Preview: one dim, truncated line of the verse text, indented
                // under the reference. On the selected row it joins the slab.
                if let Some(text) = &row.preview {
                    let indent = 6usize;
                    let snippet = truncate_chars(text, inner_w.saturating_sub(indent).max(8));
                    let prev_used = indent + snippet.chars().count();
                    lines.push(Line::from(vec![
                        Span::styled(" ".repeat(indent), slab),
                        Span::styled(snippet, if on { sel_preview } else { dim }),
                        Span::styled(" ".repeat(inner_w.saturating_sub(prev_used)), slab),
                    ]));
                }
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

/// Flatten newlines and truncate to at most `max` chars, appending an ellipsis
/// when the text is cut.
fn truncate_chars(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(book: &str, chapter: i64, verse: i64) -> Bookmark {
        Bookmark {
            translation: "en-kjv".into(),
            book: book.into(),
            chapter,
            start_verse: verse,
            end_verse: verse,
            label: None,
            created_at: 0,
        }
    }

    fn dialog_with(rows: Vec<(Bookmark, Option<&str>)>) -> BookmarksDialog {
        BookmarksDialog {
            items: rows
                .into_iter()
                .map(|(bm, p)| Row {
                    bm,
                    preview: p.map(str::to_string),
                })
                .collect(),
            cursor: 0,
            nav: ListNav::default(),
        }
    }

    #[test]
    fn scroll_origin_keeps_cursor_visible_with_two_row_cells() {
        // Ten previewed bookmarks (2 rows each) in a 6-row window: only three
        // cells fit, so the origin must advance to keep the cursor in view.
        let mut d = dialog_with(
            (0..10)
                .map(|i| (bm("GEN", 1, i + 1), Some("in the beginning")))
                .collect(),
        );
        d.cursor = 7;
        let origin = d.scroll_origin(6);
        // From the origin, the cursor's cell must fit within the 6 rows.
        let mut used = 0;
        let mut visible = false;
        for (i, row) in d.items.iter().enumerate().skip(origin) {
            if used + row.height() > 6 {
                break;
            }
            used += row.height();
            visible |= i == d.cursor;
        }
        assert!(
            visible,
            "cursor row {} not visible from origin {origin}",
            d.cursor
        );
    }

    #[test]
    fn truncate_appends_ellipsis_only_when_cut() {
        assert_eq!(truncate_chars("short", 20), "short");
        let cut = truncate_chars("a much longer verse body than fits", 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.ends_with('\u{2026}'));
    }
}
