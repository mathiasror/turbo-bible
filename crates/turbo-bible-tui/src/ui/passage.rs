//! The reading window: a Turbo-Vision-style framed window containing a
//! scrollable chapter.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
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
}

impl Widget for PassageView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        theme::draw_shadow(buf, area);

        let title = format!(
            " {} {} \u{2500}\u{2500} {} ",
            self.passage.book_name, self.passage.chapter, self.passage.translation
        );
        // Mode pill on the title row, mirroring the splash NORMAL/FILTER pills:
        // VISUAL is loud (yellow), NORMAL subdued (the standard mode cyan). The
        // reading view is where mode matters most, so it gets a permanent cue.
        let visual = self.selection.is_some();
        // Reading-view pills are *status indicators*, so they're quieter than
        // the splash's NORMAL/FILTER *control* pills: no [ ] brackets, and
        // NORMAL drops to dim teal (splash uses bright cyan). VISUAL keeps the
        // louder yellow — an active selection mode warrants the attention.
        let (pill_text, pill_bg) = if visual {
            (" VISUAL ", theme::yellow())
        } else {
            (" NORMAL ", theme::teal())
        };
        let pill = Line::from(Span::styled(
            pill_text,
            Style::new()
                .fg(theme::black())
                .bg(pill_bg)
                .add_modifier(Modifier::BOLD),
        ))
        .right_aligned();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::new().fg(theme::bright_white()).bg(theme::blue()))
            .title(Line::from(Span::styled(
                title,
                Style::new().fg(theme::bright_white()).bg(theme::blue()),
            )))
            .title(pill)
            .style(Style::new().bg(theme::blue()));

        let inner = block.inner(area);
        block.render(area, buf);

        for y in inner.top()..inner.bottom() {
            for x in inner.left()..inner.right() {
                let cell = &mut buf[(x, y)];
                cell.set_symbol(" ");
                cell.set_style(Style::new().bg(theme::blue()));
            }
        }

        let rendered = render_passage(
            self.passage,
            self.cursor_verse,
            self.selection,
            self.bookmarked,
            inner.width,
        );
        let cursor_line = crate::render::line_index_for_verse(&rendered, self.cursor_verse);
        let viewport = inner.height as usize;
        let target_top = cursor_line.saturating_sub(viewport / 3);
        let max_top = rendered.len().saturating_sub(viewport);
        // Scroll fits in `u16` because the rendered chapter is bounded by
        // visible rows × wrap width; clamp to u16::MAX in the (unreachable)
        // case where it doesn't.
        let scroll = u16::try_from(target_top.min(max_top)).unwrap_or(u16::MAX);

        let lines: Vec<Line<'static>> = pad_to_width(&rendered, inner.width);
        Paragraph::new(lines)
            .style(Style::new().bg(theme::blue()))
            .scroll((scroll, 0))
            .render(inner, buf);
    }
}
