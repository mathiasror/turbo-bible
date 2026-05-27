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
    /// In a compare split, the focused pane wears a filled title bar, a
    /// double-line border, and the mode pill; unfocused panes dim their
    /// (single-line) border to recede. Always `true` for the single-pane
    /// reading view.
    pub is_focused: bool,
    /// Whether this pane is one of several in a compare split. The loud
    /// focus chrome (filled title bar + double border) only fires when
    /// `compare_mode && is_focused` — a lone reading pane has no focus to
    /// disambiguate, so it keeps the calm plain border + bright_white title.
    pub compare_mode: bool,
    /// When the pane was opened from the `K` cross-reference popup via `s`,
    /// the source reference (e.g. `"John 3:16"`); rendered in the title as
    /// `… ← John 3:16` so the relationship is glanceable. `None` for the
    /// single-pane view and for `Ctrl-W v` translation compares.
    pub origin_label: Option<&'a str>,
    /// In a compare split, the focused pane's current cursor verse, threaded
    /// into each *unfocused* pane so it can faintly tint the matching verse
    /// (a passive cross-pane locator — never moves a cursor). `None` for the
    /// focused pane and the single-pane view.
    pub peer_verse: Option<i64>,
    /// Suppress the per-pane drop shadow. Set on interior compare panes so
    /// adjacent columns tile flush instead of smudging a shadow onto the
    /// neighbor's border; the single pane and the group's right edge keep
    /// their shadow over the blue desktop.
    pub suppress_shadow: bool,
}

impl Widget for PassageView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Interior compare panes suppress their shadow so columns tile flush
        // (a per-pane shadow would fall onto the neighbor's border and read as
        // a smudge between columns). The single pane and the group's rightmost
        // pane keep it — there it falls on the blue desktop, as intended.
        if !self.suppress_shadow {
            theme::draw_shadow(buf, area);
        }

        // Title: `Book Chapter ── Translation`, plus an xref-split origin
        // suffix (` ← John 3:16`) when present so the relationship is clear.
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
        // The loud focus chrome only fires for the focused pane *of a compare
        // split* — a lone reading pane has no sibling to disambiguate from, so
        // it keeps the calm classic look (plain border, bright_white title).
        let loud_focus = self.is_focused && self.compare_mode;
        // Focus signalling is layered so it survives both colorblindness and a
        // greyscale terminal:
        //   1. SHAPE — the focused pane gets a double-line (╔═╗) border, the
        //      unfocused panes a single line. A color-independent cue.
        //   2. COLOR — the focused pane's whole title bar fills with the
        //      bright_cyan selection tier (black bold text); unfocused panes
        //      keep a dim dark_grey border on the blue pane.
        // bright_cyan is the palette's selection tier, so it's house-legal here
        // (yellow stays reserved for verse numbers + the mode pill).
        let (border_type, title_fg, title_bg) = if loud_focus {
            (BorderType::Double, theme::black(), theme::bright_cyan())
        } else if self.is_focused {
            // Single-pane reading view: the original bright_white chrome.
            (BorderType::Plain, theme::bright_white(), theme::blue())
        } else {
            (BorderType::Plain, theme::dark_grey(), theme::blue())
        };
        // Bold the location reference so the eye lands on "where am I" first.
        let title_style = Style::new()
            .fg(title_fg)
            .bg(title_bg)
            .add_modifier(Modifier::BOLD);
        let mut title_spans = vec![Span::styled(title, title_style)];
        if let Some(origin) = self.origin_label {
            // `← <ref>` states the xref-split relationship. The arrow takes the
            // mid_cyan structural-label tier; on the loud focus fill it sits on
            // bright_cyan (where dark_grey would vanish) — black keeps it
            // legible there. The reference text rides the title style.
            let arrow_fg = if loud_focus {
                theme::black()
            } else {
                theme::mid_cyan()
            };
            title_spans.push(Span::styled(
                "\u{2190} ",
                Style::new().fg(arrow_fg).bg(title_bg),
            ));
            title_spans.push(Span::styled(
                format!("{origin} "),
                Style::new().fg(title_fg).bg(title_bg),
            ));
        }
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::new().fg(title_fg).bg(title_bg))
            .title(Line::from(title_spans))
            .style(Style::new().bg(theme::blue()));
        // The focused pane wears the NORMAL/VISUAL pill (single-pane reading
        // view included — mode matters most there); unfocused compare panes
        // drop it (it would be noise, and misleading since motions don't apply).
        if self.is_focused {
            block = block.title(pill);
        }

        let inner = block.inner(area);
        block.render(area, buf);

        // Block paints the *border-line* cells with `border_style`, but the
        // title-row cells the title text doesn't cover (the gap between title
        // and pill) keep the pane's blue `style` bg. For the loud-focus pane we
        // want the *entire* title bar bright_cyan, so overpaint that row's
        // background — preserving each cell's glyph and the yellow/teal mode
        // pill, which we detect by its distinctive bg.
        if loud_focus {
            let pill_bg_color = pill_bg;
            let y = area.top();
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                if cell.bg == pill_bg_color {
                    continue; // leave the mode pill's own fill intact
                }
                cell.set_fg(theme::black());
                cell.set_bg(theme::bright_cyan());
            }
        }

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
            self.peer_verse,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Verse;

    fn passage() -> Passage {
        Passage {
            translation: "en-kjv".into(),
            book_code: "ROM".into(),
            book_name: "Romans".into(),
            book_abbrev: "Rom".into(),
            chapter: 5,
            verses: vec![Verse {
                number: 1,
                text: "Therefore being justified by faith".into(),
                footnote_count: 0,
                xref_note_count: 0,
            }],
            headings: vec![],
            footnotes: vec![],
            xrefs: vec![],
        }
    }

    /// Render a `PassageView` into a fresh buffer and return it for inspection.
    fn render(view: PassageView<'_>, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        buf
    }

    /// Concatenate the printable glyphs of buffer row `y`.
    fn row_text(buf: &Buffer, y: u16) -> String {
        (buf.area.left()..buf.area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn focused_pane_fills_title_bar_bright_cyan() {
        let p = passage();
        let bm = std::collections::BTreeSet::new();
        let buf = render(
            PassageView {
                passage: &p,
                cursor_verse: 1,
                selection: None,
                bookmarked: &bm,
                is_focused: true,
                compare_mode: true,
                origin_label: None,
                peer_verse: None,
                suppress_shadow: true,
            },
            40,
            10,
        );
        // The whole title row (y == 0) is bright_cyan, except the yellow/teal
        // mode pill. Every non-pill cell carries the bright_cyan fill.
        let mut saw_cyan = false;
        for x in 0..40u16 {
            let bg = buf[(x, 0)].bg;
            if bg == theme::bright_cyan() {
                saw_cyan = true;
            } else {
                assert!(
                    bg == theme::teal() || bg == theme::yellow(),
                    "focused title-bar cell {x} bg {bg:?} should be the bright_cyan \
                     fill or the mode pill (teal/yellow)",
                );
            }
        }
        assert!(saw_cyan, "focused title bar must fill with bright_cyan");
    }

    #[test]
    fn single_pane_keeps_calm_title_bar() {
        // The lone reading pane (compare_mode == false, always focused) must
        // NOT wear the loud bright_cyan focus fill — there's no sibling pane to
        // disambiguate from, so it keeps the classic plain chrome.
        let p = passage();
        let bm = std::collections::BTreeSet::new();
        let buf = render(
            PassageView {
                passage: &p,
                cursor_verse: 1,
                selection: None,
                bookmarked: &bm,
                is_focused: true,
                compare_mode: false,
                origin_label: None,
                peer_verse: None,
                suppress_shadow: false,
            },
            40,
            10,
        );
        for x in 0..40u16 {
            assert_ne!(
                buf[(x, 0)].bg,
                theme::bright_cyan(),
                "single pane must not fill its title bar bright_cyan (cell {x})",
            );
        }
    }

    #[test]
    fn unfocused_pane_keeps_dim_title_bar() {
        let p = passage();
        let bm = std::collections::BTreeSet::new();
        let buf = render(
            PassageView {
                passage: &p,
                cursor_verse: 1,
                selection: None,
                bookmarked: &bm,
                is_focused: false,
                compare_mode: true,
                origin_label: None,
                peer_verse: Some(1),
                suppress_shadow: true,
            },
            40,
            10,
        );
        // No bright_cyan title fill on an unfocused pane, and no mode pill.
        for x in 0..40u16 {
            assert_ne!(
                buf[(x, 0)].bg,
                theme::bright_cyan(),
                "unfocused pane must not fill its title bar bright_cyan (cell {x})",
            );
        }
    }

    #[test]
    fn xref_split_pane_shows_origin_label_in_title() {
        let p = passage();
        let bm = std::collections::BTreeSet::new();
        let buf = render(
            PassageView {
                passage: &p,
                cursor_verse: 1,
                selection: None,
                bookmarked: &bm,
                is_focused: true,
                compare_mode: true,
                origin_label: Some("John 3:16"),
                peer_verse: None,
                suppress_shadow: true,
            },
            60,
            10,
        );
        let title = row_text(&buf, 0);
        assert!(
            title.contains("Romans 5"),
            "title should still show the passage reference: {title:?}",
        );
        assert!(
            title.contains('\u{2190}') && title.contains("John 3:16"),
            "xref-split title should state the origin as `\u{2190} John 3:16`: {title:?}",
        );
    }

    #[test]
    fn pane_without_origin_has_no_arrow_in_title() {
        let p = passage();
        let bm = std::collections::BTreeSet::new();
        let buf = render(
            PassageView {
                passage: &p,
                cursor_verse: 1,
                selection: None,
                bookmarked: &bm,
                is_focused: true,
                compare_mode: true,
                origin_label: None,
                peer_verse: None,
                suppress_shadow: true,
            },
            60,
            10,
        );
        assert!(
            !row_text(&buf, 0).contains('\u{2190}'),
            "a non-xref pane must not show the origin arrow",
        );
    }
}
