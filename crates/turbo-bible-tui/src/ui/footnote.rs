//! Footnote popup (K). Shows every footnote attached to a given verse, lets
//! the user navigate cross-references with ↑/↓ and follow them with Enter.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
};

use crate::db::{Footnote, Xref};
use crate::nav::Position;
use crate::theme;
use crate::ui::dialog;
use crate::ui::listnav::{self, ListNav, Step};

#[derive(Clone)]
struct XrefItem {
    target: Position,
    label: String,
}

pub struct FootnoteDialog {
    verse_label: String,
    footnotes: Vec<Footnote>,
    xrefs: Vec<XrefItem>,
    selected: usize,
    nav: ListNav,
}

#[non_exhaustive]
pub enum FootnoteOutcome {
    Continue,
    Cancel,
    /// Follow the selected cross-reference in place (Enter).
    Jump(Position),
    /// Open the selected cross-reference in a new compare pane (`s`).
    OpenSplit(Position),
}

impl FootnoteDialog {
    pub fn new(verse_label: String, footnotes: Vec<Footnote>, xrefs: Vec<Xref>) -> Self {
        let xrefs: Vec<XrefItem> = xrefs
            .into_iter()
            .map(|x| XrefItem {
                label: x.target_label(),
                target: Position {
                    book: x.to_book,
                    chapter: x.to_chapter,
                    verse: Some(x.to_verse_start),
                },
            })
            .collect();
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
            KeyCode::Enter => self
                .xrefs
                .get(self.selected)
                .map_or(FootnoteOutcome::Continue, |item| {
                    FootnoteOutcome::Jump(item.target.clone())
                }),
            // `s` opens the selected xref alongside the current verse in a
            // new compare pane, rather than replacing the current passage.
            KeyCode::Char('s') => self
                .xrefs
                .get(self.selected)
                .map_or(FootnoteOutcome::Continue, |item| {
                    FootnoteOutcome::OpenSplit(item.target.clone())
                }),
            _ => FootnoteOutcome::Continue,
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "two sections (footnotes, xrefs) + adaptive sizing + footer + \
                  empty-state branch — all inline so the dialog stays a single \
                  call site."
    )]
    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let empty = self.footnotes.is_empty() && self.xrefs.is_empty();
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
        let inner = dialog::draw_modal_dialog(outer, area, &title, buf);

        let bg = Style::new().bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let body_style = Style::new().fg(theme::light_grey()).bg(theme::blue());
        // Section labels ("Cross-references", footnote-kind headers) use the
        // mid-cyan structural-label tier, matching the sidebar and help
        // dialog. Yellow is reserved for verse numbers + mode pills (see
        // sidebar.rs and the yellow-slot rule in tui-specific.md).
        let header_style = Style::new()
            .fg(theme::mid_cyan())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::list_focus_bg())
            .add_modifier(Modifier::BOLD);
        // Cross-reference entries — dim cyan (teal), no underline; the `→`
        // arrow already signals navigability. Mirrors sidebar.rs::xref_style.
        let xref_color = Style::new().fg(theme::teal()).bg(theme::blue());

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());

        // Footnote bodies (currently always empty — the schema is in place
        // but no upstream source populates the table at the pinned commit).
        for fn_ in &self.footnotes {
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
            lines.push(blank());
        }

        // Cross-references (from `xref` table, openbible.info dataset). Record
        // the content-line index of each entry so the scroll math below can
        // window the list around `self.selected` without re-deriving the
        // layout.
        let mut entry_idx: Vec<usize> = Vec::with_capacity(self.xrefs.len());
        if !self.xrefs.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled("Cross-references:".to_string(), header_style),
            ]));
            for (xi, xref) in self.xrefs.iter().enumerate() {
                entry_idx.push(lines.len());
                let style = if xi == self.selected { sel } else { xref_color };
                lines.push(Line::from(vec![
                    Span::styled("    \u{2192} ", label),
                    Span::styled(xref.label.clone(), style),
                ]));
            }
            lines.push(blank());
        }

        if empty {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "(no notes or cross-references on this verse)",
                    Style::new()
                        .fg(theme::light_grey())
                        .bg(theme::blue())
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Pin the footer to the last inner row; everything above it scrolls.
        // Reserving the row up-front means the footer can never be pushed
        // off-screen no matter how many cross-references the verse carries
        // (John 3:16 has 27 — far more than fit at once).
        let body_h = inner.height.saturating_sub(1) as usize;
        let content_len = lines.len();
        let max_scroll = content_len.saturating_sub(body_h);

        // Window the list so the selected entry stays visible. Scroll only as
        // far as needed to bring `selected` onto the last body row — scrolling
        // the *minimum* keeps the "Cross-references:" header (which sits above
        // every entry) in view for as long as the selection allows, so the
        // list never reads as a context-free wall of references.
        let mut scroll = 0usize;
        if let Some(&sel_line) = entry_idx.get(self.selected)
            && sel_line >= body_h
        {
            // Selected entry is below the fold: pull it to the bottom row.
            scroll = sel_line + 1 - body_h;
        }
        scroll = scroll.min(max_scroll);

        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        // Footer — only advertise navigation/Enter when there's something to
        // navigate; otherwise just Esc close so the footer doesn't promise
        // actions the empty body can't deliver.
        let footer = if empty {
            vec![
                Span::styled("  ", bg),
                Span::styled("Esc ", key_style),
                Span::styled("cancel", dim),
            ]
        } else {
            vec![
                Span::styled("  ", bg),
                Span::styled("Enter ", key_style),
                Span::styled("follow   ", dim),
                Span::styled("s ", key_style),
                Span::styled("split   ", dim),
                Span::styled("\u{2191}\u{2193}/j k ", key_style),
                Span::styled("navigate   ", dim),
                Span::styled("Esc ", key_style),
                Span::styled("cancel", dim),
            ]
        };

        let body_area = Rect::new(inner.x, inner.y, inner.width, body_h as u16);
        Paragraph::new(lines)
            .style(bg)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(body_area, buf);

        let footer_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
        Paragraph::new(Line::from(footer))
            .style(bg)
            .render(footer_area, buf);

        // Period-correct scroll thumb in the right border when the list
        // overflows the body: a ░ track with a ▓ thumb, mirroring the Help
        // dialog so the two popups signal overflow identically.
        if content_len > body_h {
            let mut sb = ScrollbarState::new(content_len)
                .position(scroll)
                .viewport_content_length(body_h);
            let track = Rect::new(area.x, body_area.y, area.width, body_area.height);
            StatefulWidget::render(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("\u{2591}"))
                    .thumb_symbol("\u{2593}")
                    .track_style(Style::new().fg(theme::dark_grey()).bg(theme::blue()))
                    .thumb_style(Style::new().fg(theme::bright_white()).bg(theme::blue())),
                track,
                buf,
                &mut sb,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    /// Build a dialog with `n` synthetic cross-references (Rom 1:1, Rom 1:2,
    /// …) and no footnotes — the John 3:16 shape that overflows a short popup.
    fn dialog_with_xrefs(n: i64) -> FootnoteDialog {
        let xrefs = (1..=n)
            .map(|v| Xref {
                from_verse: 16,
                to_book: "Rom".to_string(),
                to_book_abbrev: "Rom".to_string(),
                to_chapter: 1,
                to_verse_start: v,
                to_verse_end: v,
            })
            .collect();
        FootnoteDialog::new("John 3:16".to_string(), Vec::new(), xrefs)
    }

    /// Collect every glyph in a single buffer row into a string, so a test can
    /// assert a footer / entry label survived the render.
    fn row_text(buf: &Buffer, y: u16, area: Rect) -> String {
        (area.left()..area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    fn down(dlg: &mut FootnoteDialog, n: usize) {
        for _ in 0..n {
            dlg.handle(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()));
        }
    }

    /// With far more cross-references than fit, the footer (`Enter follow · s
    /// split · …`) must still render on the last inner row — it can never be
    /// pushed off-screen. Before the scroll rework the 27 xrefs on John 3:16
    /// shoved the footer (and the trailing 9 entries) past the dialog floor.
    #[test]
    fn footer_survives_a_long_xref_list() {
        let dlg = dialog_with_xrefs(27);
        // A short-but-realistic popup: 130×34 desktop → ~22-row dialog.
        let area = Rect::new(0, 0, 130, 34);
        let mut buf = Buffer::empty(area);
        dlg.render(area, &mut buf);

        let mut found = false;
        for y in area.top()..area.bottom() {
            let row = row_text(&buf, y, area);
            if row.contains("follow") && row.contains("split") {
                found = true;
            }
        }
        assert!(
            found,
            "footer (follow / split hints) missing — clipped by the xref list"
        );
    }

    /// An overflowing list draws the Turbo-Vision scroll thumb (▓ on a ░
    /// track) in the right border, exactly like the Help dialog — the only
    /// overflow signal the popup gives.
    #[test]
    fn scroll_thumb_drawn_when_xrefs_overflow() {
        let dlg = dialog_with_xrefs(27);
        let area = Rect::new(0, 0, 130, 34);
        let mut buf = Buffer::empty(area);
        dlg.render(area, &mut buf);

        let mut track = false;
        let mut thumb = false;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                match buf[(x, y)].symbol() {
                    "\u{2591}" => track = true,
                    "\u{2593}" => thumb = true,
                    _ => {}
                }
            }
        }
        assert!(
            track && thumb,
            "overflowing xref list must draw a ░ track + ▓ thumb"
        );
    }

    /// Selecting an entry past the visible window must scroll it into view: the
    /// selection slab (`list_focus_bg`) has to land inside the body, not vanish
    /// off the bottom. Before the fix `render` applied no offset, so the
    /// highlight walked off-screen and the list looked frozen.
    #[test]
    fn high_selection_scrolls_into_view() {
        let mut dlg = dialog_with_xrefs(27);
        // Walk the cursor to the last entry — well past the visible window.
        down(&mut dlg, 26);
        let area = Rect::new(0, 0, 130, 34);
        let mut buf = Buffer::empty(area);
        dlg.render(area, &mut buf);

        let slab = theme::list_focus_bg();
        let mut slab_y = None;
        for y in area.top()..area.bottom() {
            if (area.left()..area.right()).any(|x| buf[(x, y)].bg == slab) {
                slab_y = Some(y);
            }
        }
        let y = slab_y.expect("selected entry's slab not found — it scrolled off-screen");
        // The slab must sit above the footer row (the last inner row), proving
        // the body windowed rather than the footer being clobbered.
        assert!(
            y < area.bottom().saturating_sub(2),
            "selection slab landed on/below the footer row at y={y}"
        );

        // …and the footer is still present alongside the scrolled selection.
        let mut footer = false;
        for fy in area.top()..area.bottom() {
            if row_text(&buf, fy, area).contains("follow") {
                footer = true;
            }
        }
        assert!(footer, "footer lost once the selection scrolled");
    }

    /// The empty-state branch keeps its quiet single-verb footer (`Esc
    /// cancel`) and draws no scrollbar — there's nothing to scroll.
    #[test]
    fn empty_state_has_no_thumb_and_a_cancel_footer() {
        let dlg = FootnoteDialog::new("John 3:16".to_string(), Vec::new(), Vec::new());
        let area = Rect::new(0, 0, 130, 34);
        let mut buf = Buffer::empty(area);
        dlg.render(area, &mut buf);

        let mut thumb = false;
        let mut cancel = false;
        for y in area.top()..area.bottom() {
            let row = row_text(&buf, y, area);
            if row.contains("cancel") {
                cancel = true;
            }
            for x in area.left()..area.right() {
                if buf[(x, y)].symbol() == "\u{2593}" {
                    thumb = true;
                }
            }
        }
        assert!(cancel, "empty-state footer must still show Esc cancel");
        assert!(!thumb, "empty state must not draw a scroll thumb");
    }
}
