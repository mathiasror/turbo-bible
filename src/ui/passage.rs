//! The reading window: a Turbo-Vision-style framed window containing a
//! scrollable chapter.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use crate::db::Passage;
use crate::render::{pad_to_width, render_passage};
use crate::theme;

pub struct PassageView<'a> {
    pub passage: &'a Passage,
    pub cursor_verse: i64,
    pub selection: Option<(i64, i64)>,
    pub bookmarked: &'a std::collections::BTreeSet<i64>,
    pub two_line_verses: bool,
}

impl<'a> Widget for PassageView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        theme::draw_shadow(buf, area);

        let title = format!(
            " {} {} \u{2550}\u{2550} {} ",
            self.passage.book_name, self.passage.chapter, self.passage.translation
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::new().fg(theme::BRIGHT_WHITE).bg(theme::BLUE))
            .title(Line::from(Span::styled(
                title,
                Style::new().fg(theme::BRIGHT_WHITE).bg(theme::BLUE),
            )))
            .style(Style::new().bg(theme::BLUE));

        let inner = block.inner(area);
        block.render(area, buf);

        for y in inner.top()..inner.bottom() {
            for x in inner.left()..inner.right() {
                let cell = &mut buf[(x, y)];
                cell.set_symbol(" ");
                cell.set_style(Style::new().bg(theme::BLUE));
            }
        }

        let rendered = render_passage(
            self.passage,
            self.cursor_verse,
            self.selection,
            self.bookmarked,
            inner.width,
            self.two_line_verses,
        );
        let cursor_line = crate::render::line_index_for_verse(&rendered, self.cursor_verse);
        let viewport = inner.height as usize;
        let target_top = cursor_line.saturating_sub(viewport / 3);
        let max_top = rendered.len().saturating_sub(viewport);
        let scroll = target_top.min(max_top) as u16;

        let lines: Vec<Line<'static>> = pad_to_width(&rendered, inner.width);
        Paragraph::new(lines)
            .style(Style::new().bg(theme::BLUE))
            .scroll((scroll, 0))
            .render(inner, buf);
    }
}
