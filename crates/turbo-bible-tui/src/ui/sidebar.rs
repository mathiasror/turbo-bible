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

/// Maximum xref rows in the sidebar per cursor verse. The K-popup ("Notes")
/// is the place to scan a long xref list; the sidebar's job is the top few
/// by openbible vote so it doesn't push the parallel passage / footnotes
/// off-screen on a heavily-referenced verse (e.g. JHN 3:16 has ~27).
const SIDEBAR_XREF_CAP: usize = 8;

pub struct SidebarView<'a> {
    pub passage: &'a Passage,
    pub cursor_verse: i64,
    pub selection: Option<(i64, i64)>,
}

impl Widget for SidebarView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        theme::draw_shadow(buf, area);

        // Subordinate the sidebar visually: dim border + dim title so the
        // reading pane is unambiguously the primary surface. Single-line
        // border (vs the reading pane's double) further demotes it.
        let title = " References ";
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::new().fg(theme::light_grey()).bg(theme::blue()))
            .title(Line::from(Span::styled(
                title,
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            )))
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

        let lines = build_lines(self.passage, self.cursor_verse, self.selection);
        Paragraph::new(lines)
            .style(Style::new().bg(theme::blue()))
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

fn build_lines(
    p: &Passage,
    cursor_verse: i64,
    selection: Option<(i64, i64)>,
) -> Vec<Line<'static>> {
    let bg = Style::new().bg(theme::blue());
    let body = Style::new().fg(theme::bright_white()).bg(theme::blue());
    let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
    let header = Style::new()
        .fg(theme::cyan())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);
    // Verse-label heading (e.g. "John 3:16") — a medium-emphasis cyan
    // structural anchor. Yellow is reserved for the scripture pane (verse
    // numbers, mode pills); the sidebar carries no yellow.
    let accent = Style::new()
        .fg(theme::cyan())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);
    // Cross-reference entries — dim cyan (teal), one tier below the cyan
    // section labels, and no underline: DOS/Turbo Vision TUIs never
    // underlined whole entries (the `→` arrow already signals navigability).
    let xref_style = Style::new().fg(theme::teal()).bg(theme::blue());

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("", bg)));

    // Verse label header — in visual mode show the whole range so the user
    // can see how many verses they have selected without doing arithmetic.
    let verse_label = match selection {
        Some((s, e)) if s != e => format!(
            " {}  ({} verses)",
            crate::reference::format_range(&p.book_abbrev, p.chapter, s, e, &p.translation),
            e - s + 1
        ),
        _ => format!(
            " {}",
            crate::reference::format(&p.book_abbrev, p.chapter, cursor_verse, &p.translation)
        ),
    };
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

    // 2) Footnotes (table currently unpopulated — see db::load_footnotes).
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

    // 3) Cross-references for this verse. Capped by SIDEBAR_XREF_CAP; the
    // load order is votes-DESC so we keep the highest-ranked xrefs. The
    // K-popup ("Notes") shows the full list.
    let xrefs: Vec<&crate::db::Xref> = p
        .xrefs
        .iter()
        .filter(|x| x.from_verse == cursor_verse)
        .take(SIDEBAR_XREF_CAP)
        .collect();
    if !xrefs.is_empty() {
        lines.push(Line::from(Span::styled(" Cross-references", header)));
        for x in &xrefs {
            lines.push(Line::from(vec![
                Span::styled("   \u{2192} ", xref_style),
                Span::styled(x.target_label(), xref_style),
            ]));
        }
        lines.push(Line::from(Span::styled("", bg)));
    }

    if notes.is_empty() && xrefs.is_empty() && current_parallel(p, cursor_verse).is_none() {
        lines.push(Line::from(Span::styled(
            " (nothing for this verse)",
            Style::new()
                .fg(theme::light_grey())
                .bg(theme::blue())
                .add_modifier(Modifier::ITALIC),
        )));
    }

    lines
}

fn current_parallel(p: &Passage, cursor_verse: i64) -> Option<&Heading> {
    p.headings
        .iter()
        .rfind(|h| h.style == "r" && h.before_verse <= cursor_verse)
}
