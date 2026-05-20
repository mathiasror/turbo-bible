//! Persistent right-hand pane: shows the parallel-passage reference (the
//! `r`-style heading) for the current section, plus all footnotes and
//! cross-references attached to the cursor verse. Auto-follows the cursor.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use crate::db::{Heading, Passage};
use crate::theme;

pub struct SidebarView<'a> {
    pub passage: &'a Passage,
    pub cursor_verse: i64,
}

impl<'a> Widget for SidebarView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        theme::draw_shadow(buf, area);

        let title = " References ";
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

        let lines = build_lines(self.passage, self.cursor_verse, inner.width);
        Paragraph::new(lines)
            .style(Style::new().bg(theme::BLUE))
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

fn build_lines(p: &Passage, cursor_verse: i64, _width: u16) -> Vec<Line<'static>> {
    let bg = Style::new().bg(theme::BLUE);
    let body = Style::new().fg(theme::BRIGHT_WHITE).bg(theme::BLUE);
    let dim = Style::new().fg(theme::LIGHT_GREY).bg(theme::BLUE);
    let header = Style::new()
        .fg(theme::CYAN)
        .bg(theme::BLUE)
        .add_modifier(Modifier::BOLD);
    let accent = Style::new()
        .fg(theme::YELLOW)
        .bg(theme::BLUE)
        .add_modifier(Modifier::BOLD);
    let xref_style = Style::new()
        .fg(theme::YELLOW)
        .bg(theme::BLUE)
        .add_modifier(Modifier::UNDERLINED);

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("", bg)));

    // Verse label header
    let verse_label = format!(
        " {} {}:{}",
        p.book_abbrev, p.chapter, cursor_verse
    );
    lines.push(Line::from(Span::styled(verse_label, accent)));
    lines.push(Line::from(Span::styled("", bg)));

    // 1) Parallel passage (most recent `r` heading ≤ cursor_verse)
    if let Some(parallel) = current_parallel(p, cursor_verse) {
        lines.push(Line::from(Span::styled(" Parallel passage", header)));
        lines.push(Line::from(vec![
            Span::styled("   ", bg),
            Span::styled(parallel.text.clone(), dim),
        ]));
        lines.push(Line::from(Span::styled("", bg)));
    }

    let cursor_osis = format!("{}.{}.{}", p.book_code, p.chapter, cursor_verse);
    let notes: Vec<_> = p
        .footnotes
        .iter()
        .filter(|fn_| fn_.verse_osis == cursor_osis)
        .collect();

    // 2) Footnotes
    let f_notes: Vec<_> = notes.iter().filter(|n| n.kind == "f").collect();
    if !f_notes.is_empty() {
        lines.push(Line::from(Span::styled(" Footnotes", header)));
        for n in &f_notes {
            lines.push(Line::from(vec![
                Span::styled("   ", bg),
                Span::styled(n.body.clone(), body),
            ]));
            lines.push(Line::from(Span::styled("", bg)));
        }
    }

    // 3) Cross-references (collected from BOTH `x` and `f` notes)
    let mut xrefs: Vec<(String, String)> = Vec::new(); // (label, target_osis)
    for n in &notes {
        for x in &n.refs {
            xrefs.push((x.label.clone(), x.target_osis.clone()));
        }
    }
    if !xrefs.is_empty() {
        lines.push(Line::from(Span::styled(" Cross-references", header)));
        for (label, target) in xrefs {
            lines.push(Line::from(vec![
                Span::styled("   \u{2192} ", body),
                Span::styled(label, xref_style),
                Span::styled(format!("  ({target})"), dim),
            ]));
        }
        lines.push(Line::from(Span::styled("", bg)));
    }

    if notes.is_empty() && current_parallel(p, cursor_verse).is_none() {
        lines.push(Line::from(Span::styled(
            " (nothing for this verse)",
            Style::new()
                .fg(theme::LIGHT_GREY)
                .bg(theme::BLUE)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    lines
}

fn current_parallel(p: &Passage, cursor_verse: i64) -> Option<&Heading> {
    p.headings
        .iter()
        .filter(|h| h.style == "r" && h.before_verse <= cursor_verse)
        .last()
}
