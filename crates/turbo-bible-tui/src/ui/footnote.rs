//! Footnote popup (K). Shows every footnote attached to a given verse, lets
//! the user navigate cross-references with ↑/↓ and follow them with Enter.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

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

        // Cross-references (from `xref` table, openbible.info dataset).
        if !self.xrefs.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled("Cross-references:".to_string(), header_style),
            ]));
            for (xi, xref) in self.xrefs.iter().enumerate() {
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
                Span::styled("follow   ", dim),
                Span::styled("s ", key_style),
                Span::styled("split   ", dim),
                Span::styled("\u{2191}\u{2193}/j k ", key_style),
                Span::styled("navigate   ", dim),
                Span::styled("Esc ", key_style),
                Span::styled("close", dim),
            ]
        };
        lines.push(Line::from(footer));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}
