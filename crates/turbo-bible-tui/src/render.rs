//! Build the visual representation of a chapter: interleave headings, inject
//! footnote markers, and produce a `Vec<Line>` ready for ratatui.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::db::Passage;
use crate::text::word_wrap;
use crate::theme;

/// One line on screen, tagged with the verse it belongs to (if any).
#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub line: Line<'static>,
    /// Verse number this line belongs to (0 if it's a heading or blank).
    pub verse: i64,
}

const VERSE_NUM_WIDTH: usize = 3;
/// One-cell gutter (cursor ▸ / bookmark ★ / blank) before the verse number.
const GUTTER_WIDTH: usize = 1;
/// Gutter + number column + two-space gap — the chrome before the verse body
/// on the first line, and the hanging indent of wrapped continuation lines so
/// they align under the body, not the number.
const VERSE_PREFIX: usize = GUTTER_WIDTH + VERSE_NUM_WIDTH + 2;
/// Horizontal padding inside the verse panel so prose doesn't run flush to the
/// inner border. The full-row highlight still spans border-to-border (the pad
/// cells carry the row background); only the text is inset.
const PANEL_PAD: usize = 1;
/// Maximum readable text-column width. Caps the verse body even when the pane
/// is wider (≥120 cols with the sidebar on), keeping lines in the comfortable
/// ~50–70 char range for sustained reading. The row fill still spans the pane.
const MAX_BODY_WIDTH: usize = 70;
/// Extra left inset applied to the verse body in poetry passages (see
/// [`crate::poetry`]). A flat, whole-verse indent that sets poetry apart from
/// prose — we have no intra-verse line data to lay out true poetic lines. The
/// verse number stays in its gutter column; only the body and its hanging
/// indent shift right, and the wrap column narrows to match.
///
/// Tuned to 3 cells: a 2-cell inset was technically applied but read as a
/// numbering artifact in review — against a single-digit Psalm verse number it
/// was hard to tell apart from the right-aligned gutter. 3 cells reads as a
/// deliberate step-in while staying clear of the ~4-cell threshold where a flat
/// inset starts to look like a block quote.
const POETRY_INDENT: usize = 3;

/// Per-verse row treatment. `Selected` (the brightest-cyan visual-selection
/// slab) outranks `Cursor` so entering visual mode — which makes the cursor
/// verse a one-verse selection — lights it immediately instead of leaving it
/// on the calmer cursor teal. `Peer` is the passive cross-pane cue (an
/// unfocused compare pane echoing the focused pane's cursor verse); it ranks
/// below the local cursor/selection so a pane's own state always wins on the
/// rare row where both coincide.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Selected,
    Cursor,
    Peer,
    Idle,
}

#[allow(
    clippy::too_many_lines,
    reason = "weaves verse/heading/marker/gutter state — splitting per concern \
              would force the shared `out`, `headings_by_anchor`, and per-verse \
              context across helper signatures without a meaningful gain."
)]
pub fn render_passage(
    p: &Passage,
    cursor_verse: i64,
    selection: Option<(i64, i64)>,
    bookmarked: &std::collections::BTreeSet<i64>,
    peer_verse: Option<i64>,
    wrap_width: u16,
) -> Vec<RenderedLine> {
    let mut out: Vec<RenderedLine> = Vec::new();

    // Whole-verse left inset for poetry passages. 0 for prose, so the prose
    // layout is byte-for-byte unchanged.
    let poetry_indent = if crate::poetry::is_poetic(&p.book_code, p.chapter) {
        POETRY_INDENT
    } else {
        0
    };

    let heading_style = Style::new()
        .fg(theme::bright_white())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);
    // Three-tier row palette, period-correct Turbo Vision:
    //   idle:     blue pane bg, light_grey body, full yellow num
    //   cursor:   darker-teal bg, bright_white body — NOT bold: a bold white
    //             slab drew too much attention against the calm pane. The teal
    //             fill, the ▸ gutter arrow, and the inverse-video yellow number
    //             chip already mark the active row, so the body stays
    //             regular-weight (still bright_white, so it reads as active).
    //   selected: brightest-cyan slab, black BOLD body/num — the classic TV
    //             reverse-video selection, and the loudest row in the pane
    let verse_num_style = |kind: RowKind| match kind {
        RowKind::Cursor => Style::new()
            .fg(theme::cursor_row_bg())
            .bg(theme::yellow())
            .add_modifier(Modifier::BOLD),
        RowKind::Selected => Style::new()
            .fg(theme::black())
            .bg(theme::selection_bg())
            .add_modifier(Modifier::BOLD),
        // Peer keeps the idle yellow-on-its-own-fill number so the verse-number
        // scanning rhythm survives; only the row fill (below) shifts to the dim
        // peer teal.
        RowKind::Peer => Style::new()
            .fg(theme::yellow())
            .bg(theme::peer_row_bg())
            .add_modifier(Modifier::BOLD),
        RowKind::Idle => Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD),
    };
    let verse_text_style = |kind: RowKind| match kind {
        RowKind::Cursor => Style::new()
            .fg(theme::bright_white())
            .bg(theme::cursor_row_bg()),
        RowKind::Selected => Style::new()
            .fg(theme::black())
            .bg(theme::selection_bg())
            .add_modifier(Modifier::BOLD),
        // Peer body stays light_grey (same as idle) so the dim teal fill is the
        // only signal — it's a passive locator, not a focus state.
        RowKind::Peer => Style::new()
            .fg(theme::light_grey())
            .bg(theme::peer_row_bg()),
        RowKind::Idle => Style::new().fg(theme::light_grey()).bg(theme::blue()),
    };

    // Pre-bucket headings by `before_verse`.
    let mut headings_by_anchor: std::collections::BTreeMap<i64, Vec<&crate::db::Heading>> =
        std::collections::BTreeMap::new();
    for h in &p.headings {
        headings_by_anchor
            .entry(h.before_verse)
            .or_default()
            .push(h);
    }

    // Single blank above the first verse so verse 1 doesn't sit flush against
    // the top border. The border title already shows `Book Chapter ── Trans`,
    // so we don't repeat the chapter banner in the body.
    out.push(rl_blank());

    for v in &p.verses {
        // Any headings that anchor before this verse get printed here.
        if let Some(hs) = headings_by_anchor.remove(&v.number) {
            let mut emitted = false;
            for h in hs {
                // Parallel-passage refs (style `r`) live in the sidebar, not
                // in the reading flow.
                if h.style == "r" {
                    continue;
                }
                if !emitted {
                    if !out.last().is_none_or(is_blank) {
                        out.push(rl_blank());
                    }
                    emitted = true;
                }
                out.push(RenderedLine {
                    line: Line::from(vec![
                        Span::styled(" ".repeat(PANEL_PAD), heading_style),
                        Span::styled(h.text.clone(), heading_style),
                    ]),
                    verse: 0,
                });
            }
            if emitted {
                out.push(rl_blank());
            }
        }

        let in_selection = selection.is_some_and(|(s, e)| v.number >= s && v.number <= e);
        let is_cursor_verse = v.number == cursor_verse;
        let is_peer_verse = peer_verse == Some(v.number);
        // Row treatment. `Selected` outranks `Cursor` so pressing `v` (which
        // makes the cursor verse a one-verse selection) lights it immediately
        // as the brightest-cyan slab; the cursor keeps its ▸ glyph so anchor
        // and cursor stay distinguishable inside a range. In normal mode
        // `selection` is None, so the cursor falls through to the teal tier.
        // `Peer` (the cross-pane cue, only ever set for unfocused panes) ranks
        // last so a pane's own cursor/selection always wins where they overlap.
        let kind = if in_selection {
            RowKind::Selected
        } else if is_cursor_verse {
            RowKind::Cursor
        } else if is_peer_verse {
            RowKind::Peer
        } else {
            RowKind::Idle
        };
        let show_cursor_arrow = is_cursor_verse;
        // Gutter, number, body, marker and right-edge padding share this bg so
        // each row reads as one continuous bar (the cursor's number chip is the
        // one intentional exception — see verse_num_style).
        let row_bg = match kind {
            RowKind::Selected => theme::selection_bg(),
            RowKind::Cursor => theme::cursor_row_bg(),
            RowKind::Peer => theme::peer_row_bg(),
            RowKind::Idle => theme::blue(),
        };
        // Footnote/xref markers (* / +) are secondary metadata: dim and never
        // bold so they don't compete with prose. light_grey reads quietly on
        // the blue pane and the teal cursor row; the brightest-cyan selection
        // needs black to stay legible (light_grey washes out on #55ffff).
        let marker_fg = match kind {
            RowKind::Selected => theme::black(),
            _ => theme::light_grey(),
        };
        let marker_style = Style::new().fg(marker_fg).bg(row_bg);
        // Gutter glyph (1 col): the cursor's ▸ pointer survives even inside a
        // visual selection, so anchor (blank gutter) and cursor (▸) stay
        // distinguishable on the same brightest-cyan fill. Non-cursor
        // selection rows leave the gutter blank — the fill marks the extent.
        // Bookmark star and idle space unchanged.
        let (gutter_glyph, gutter_style) = if show_cursor_arrow {
            let fg = if kind == RowKind::Selected {
                theme::black()
            } else {
                theme::bright_white()
            };
            (
                "\u{25B8}",
                Style::new().fg(fg).bg(row_bg).add_modifier(Modifier::BOLD),
            )
        } else if kind == RowKind::Selected
            && selection
                .is_some_and(|(s, e)| s != e && v.number == if cursor_verse == s { e } else { s })
        {
            // Anchor (fixed) end of a multi-verse selection — a subtle ┃ tick so
            // it reads as distinct from the moving cursor (▸) end.
            (
                "\u{2503}",
                Style::new()
                    .fg(theme::dark_grey())
                    .bg(theme::selection_bg()),
            )
        } else if kind == RowKind::Selected {
            (" ", Style::new().bg(theme::selection_bg()))
        } else if bookmarked.contains(&v.number) {
            // `row_bg` (not a hardcoded blue) so a bookmarked verse that's also
            // the peer-cued row keeps the dim teal fill under its ★.
            ("\u{2605}", Style::new().fg(theme::yellow()).bg(row_bg))
        } else {
            (" ", Style::new().bg(row_bg))
        };
        // Just the right-aligned number; the two-space gutter gap is a separate
        // span below so it carries the row bg, not the (cursor) number chip's
        // yellow — keeping the chip tight to its digits.
        let num_str = format!("{:>width$}", v.number, width = VERSE_NUM_WIDTH);

        let mut markers = String::new();
        if v.footnote_count > 0 {
            markers.push('*');
        }
        if v.xref_note_count > 0 {
            markers.push('+');
        }

        // Pre-wrap the verse text so wrapped lines hang-indent under the
        // verse number gutter. Append the marker glyph to the last chunk so
        // it sits at the very end of the verse, not on a row of its own.
        let text = v.text.replace('\n', " ");
        let body_w = (wrap_width as usize)
            .saturating_sub(PANEL_PAD + VERSE_PREFIX + poetry_indent)
            .clamp(20, MAX_BODY_WIDTH);
        let mut chunks = word_wrap(&text, body_w);
        if chunks.is_empty() {
            chunks.push(String::new());
        }
        if !markers.is_empty() {
            let glyph = format!(" {markers}");
            let last_len = chunks.last().map_or(0, |s| s.chars().count());
            if last_len + glyph.chars().count() <= body_w {
                if let Some(last) = chunks.last_mut() {
                    last.push_str(&glyph);
                }
            } else {
                chunks.push(glyph.trim_start().to_string());
            }
        }

        // First chunk owns the verse-number prefix; later chunks indent
        // under it. Splitting the loop lets us move `num_str` into the
        // first span instead of cloning it.
        let body_style = verse_text_style(kind);
        // Left panel inset; the highlight bar still reaches the border because
        // the pad cell carries the row bg.
        let pad_style = Style::new().bg(row_bg);
        let mut push_line = |prefix: Vec<Span<'static>>, chunk: &str| {
            let mut spans = prefix;
            let (body, tail) = match chunk.rfind(' ') {
                Some(pos) if chunk[pos + 1..].chars().all(is_marker_glyph) => {
                    (&chunk[..pos], &chunk[pos..])
                }
                _ => (chunk, ""),
            };
            spans.push(Span::styled(body.to_string(), body_style));
            if !tail.is_empty() {
                spans.push(Span::styled(tail.to_string(), marker_style));
            }
            // Highlighted rows (cursor or in-selection) pad to the full
            // wrap width so the row fill runs unbroken to the pane's right
            // edge — otherwise the highlight stops at the last word and
            // the row reads as a ragged tag instead of a clean bar.
            if kind != RowKind::Idle {
                let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                let pad = (wrap_width as usize).saturating_sub(used);
                if pad > 0 {
                    spans.push(Span::styled(" ".repeat(pad), body_style));
                }
            }
            out.push(RenderedLine {
                line: Line::from(spans),
                verse: v.number,
            });
        };
        let (first, rest) = chunks.split_first().expect("chunks is non-empty above");
        let mut first_prefix = vec![
            Span::styled(" ".repeat(PANEL_PAD), pad_style),
            Span::styled(gutter_glyph.to_string(), gutter_style),
            Span::styled(num_str, verse_num_style(kind)),
            // Two-space gutter gap in the row bg (see `num_str` above) so the
            // body text breathes after the number on every row treatment.
            Span::styled("  ".to_string(), pad_style),
        ];
        // Poetry inset rides after the gutter gap, in the row bg, so the number
        // stays put and only the body shifts right. Skipped for prose, so the
        // prose span layout is identical to before.
        if poetry_indent > 0 {
            first_prefix.push(Span::styled(" ".repeat(poetry_indent), pad_style));
        }
        push_line(first_prefix, first);
        for chunk in rest {
            push_line(
                vec![Span::styled(
                    " ".repeat(PANEL_PAD + VERSE_PREFIX + poetry_indent),
                    body_style,
                )],
                chunk,
            );
        }
    }

    out
}

const fn is_marker_glyph(c: char) -> bool {
    c == '*' || c == '+' || c == ' '
}

/// Find the first line index that belongs to a given verse, for scroll
/// targeting. Returns 0 if no match.
pub fn line_index_for_verse(lines: &[RenderedLine], verse: i64) -> usize {
    lines.iter().position(|rl| rl.verse == verse).unwrap_or(0)
}

/// The verse the cursor should land on after paging `line_delta` rendered rows
/// (negative scrolls up) from `from_verse`, given this chapter's layout at
/// `wrap_width`. This is vim `Ctrl-D` / `Ctrl-F` semantics: a "page" is a span
/// of on-screen *lines*, so a verse that wraps to several rows counts for its
/// full height rather than as a single step — the whole point of sizing the
/// jump to the viewport instead of a fixed verse count. Heading and blank rows
/// (`verse == 0`) resolve to the nearest real verse in the direction of travel.
///
/// Returns `from_verse` unchanged when the passage renders no rows.
#[must_use]
pub fn verse_after_paging(
    passage: &Passage,
    from_verse: i64,
    wrap_width: u16,
    line_delta: i64,
) -> i64 {
    // The styling inputs (cursor / selection / peer / bookmarks) don't change
    // the line *count* per verse — only `wrap_width` and the text do — so a
    // bare render recovers the same line->verse map the draw will lay out.
    let empty = std::collections::BTreeSet::new();
    let rendered = render_passage(passage, from_verse, None, &empty, None, wrap_width);
    if rendered.is_empty() {
        return from_verse;
    }
    let from_idx = i64::try_from(line_index_for_verse(&rendered, from_verse)).unwrap_or(i64::MAX);
    let last_idx = i64::try_from(rendered.len() - 1).unwrap_or(i64::MAX);
    let target = usize::try_from((from_idx + line_delta).clamp(0, last_idx)).unwrap_or(0);
    nearest_verse(&rendered, target, line_delta >= 0).unwrap_or(from_verse)
}

/// The nearest verse-bearing line to `idx`, scanning the travel direction
/// (`down`) first and the opposite direction as a fallback — so a target that
/// lands on a heading/blank row resolves to a real verse without overshooting
/// the screenful the wrong way.
fn nearest_verse(rendered: &[RenderedLine], idx: usize, down: bool) -> Option<i64> {
    let travel = if down {
        rendered[idx..].iter().find_map(real_verse)
    } else {
        rendered[..=idx].iter().rev().find_map(real_verse)
    };
    let fallback = || {
        if down {
            rendered[..idx].iter().rev().find_map(real_verse)
        } else {
            rendered[idx + 1..].iter().find_map(real_verse)
        }
    };
    travel.or_else(fallback)
}

/// The verse a rendered line belongs to, or `None` for heading/blank rows.
fn real_verse(rl: &RenderedLine) -> Option<i64> {
    (rl.verse > 0).then_some(rl.verse)
}

fn is_blank(rl: &RenderedLine) -> bool {
    rl.line
        .spans
        .iter()
        .all(|s| s.content.chars().all(|c| c == ' '))
}

fn rl_blank() -> RenderedLine {
    RenderedLine {
        line: Line::from(Span::styled(
            String::new(),
            Style::new().fg(theme::light_grey()).bg(theme::blue()),
        )),
        verse: 0,
    }
}

/// Pad every line to the given width with the pane background so the blue
/// fill is flush right (no terminal default bg bleeding through gaps after
/// short wrapped lines).
pub fn pad_to_width(lines: &[RenderedLine], width: u16) -> Vec<Line<'static>> {
    // Padding is space-only, so fg has no visual effect; bg alone communicates
    // the intent ("fill the trailing gap with the pane background").
    let pad_style = Style::new().bg(theme::blue());
    lines
        .iter()
        .map(|rl| {
            let used: usize = rl
                .line
                .spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum();
            let mut spans = rl.line.spans.clone();
            let used_u16 = u16::try_from(used).unwrap_or(u16::MAX);
            if used_u16 < width {
                let pad = (width as usize).saturating_sub(used);
                spans.push(Span::styled(" ".repeat(pad), pad_style));
            }
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::db::{Footnote, Heading, Passage, Verse, Xref};
    use std::collections::BTreeSet;

    const CURSOR_GLYPH: &str = "\u{25B8}";
    const BOOKMARK_GLYPH: &str = "\u{2605}";

    fn passage_with(verses: Vec<Verse>, headings: Vec<Heading>) -> Passage {
        Passage {
            translation: "en-kjv".into(),
            book_code: "GEN".into(),
            book_name: "Genesis".into(),
            book_abbrev: "Gen".into(),
            chapter: 1,
            verses,
            headings,
            footnotes: Vec::<Footnote>::new(),
            xrefs: Vec::<Xref>::new(),
        }
    }

    /// A passage in a specific book/chapter — for exercising the poetry inset,
    /// which keys off `book_code` + `chapter`.
    fn passage_in(book_code: &str, chapter: i64, verses: Vec<Verse>) -> Passage {
        Passage {
            translation: "en-kjv".into(),
            book_code: book_code.into(),
            book_name: book_code.into(),
            book_abbrev: book_code.into(),
            chapter,
            verses,
            headings: Vec::<Heading>::new(),
            footnotes: Vec::<Footnote>::new(),
            xrefs: Vec::<Xref>::new(),
        }
    }

    fn v(number: i64, text: &str) -> Verse {
        Verse {
            number,
            text: text.into(),
            footnote_count: 0,
            xref_note_count: 0,
        }
    }

    /// Return the first rendered line that belongs to `verse`.
    fn line_for_verse(lines: &[RenderedLine], verse: i64) -> &RenderedLine {
        lines
            .iter()
            .find(|rl| rl.verse == verse)
            .unwrap_or_else(|| panic!("no rendered line for verse {verse}"))
    }

    /// Concatenate every span's content into the raw printable text for a line.
    fn line_text(rl: &RenderedLine) -> String {
        rl.line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The gutter glyph span sits just after the `PANEL_PAD` left inset.
    fn gutter_glyph(rl: &RenderedLine) -> &str {
        rl.line.spans[PANEL_PAD].content.as_ref()
    }

    /// Cells before the verse body begins on `verse`'s first line — the summed
    /// width of every span up to (not including) the body span, which carries
    /// `body` verbatim. Measures the left inset (prose vs poetry).
    fn body_start_col(lines: &[RenderedLine], verse: i64, body: &str) -> usize {
        let rl = line_for_verse(lines, verse);
        let mut col = 0;
        for span in &rl.line.spans {
            if span.content == body {
                return col;
            }
            col += span.content.chars().count();
        }
        panic!("body span {body:?} not found on verse {verse}");
    }

    #[test]
    fn poetry_passage_indents_verse_body_past_prose() {
        // Same verse text in a prose book (GEN) vs a poetic book (PSA): the
        // poetic body sits exactly POETRY_INDENT cells further right.
        let bookmarked = BTreeSet::new();
        let prose = render_passage(
            &passage_in("GEN", 1, vec![v(1, "alpha")]),
            99,
            None,
            &bookmarked,
            None,
            80,
        );
        let poetry = render_passage(
            &passage_in("PSA", 1, vec![v(1, "alpha")]),
            99,
            None,
            &bookmarked,
            None,
            80,
        );
        let prose_col = body_start_col(&prose, 1, "alpha");
        let poetry_col = body_start_col(&poetry, 1, "alpha");
        assert_eq!(
            prose_col,
            PANEL_PAD + VERSE_PREFIX,
            "prose body must keep the standard prefix (regression guard)",
        );
        assert_eq!(
            poetry_col,
            prose_col + POETRY_INDENT,
            "poetry body should sit POETRY_INDENT cells right of prose",
        );
    }

    #[test]
    fn poetry_chapter_boundary_follows_the_classifier() {
        // Job's prose frame (ch 2) renders flush; its poetic dialogue (ch 3)
        // indents — exercising the chapter-granular branch of is_poetic.
        let bookmarked = BTreeSet::new();
        let prose = render_passage(
            &passage_in("JOB", 2, vec![v(1, "alpha")]),
            99,
            None,
            &bookmarked,
            None,
            80,
        );
        let poetry = render_passage(
            &passage_in("JOB", 3, vec![v(1, "alpha")]),
            99,
            None,
            &bookmarked,
            None,
            80,
        );
        assert_eq!(body_start_col(&prose, 1, "alpha"), PANEL_PAD + VERSE_PREFIX);
        assert_eq!(
            body_start_col(&poetry, 1, "alpha"),
            PANEL_PAD + VERSE_PREFIX + POETRY_INDENT,
        );
    }

    #[test]
    fn poetry_wrapped_lines_hang_indent_includes_inset() {
        // Wrapped continuation lines of a poetic verse hang-indent at the
        // prose prefix PLUS the poetry inset, so the body stays aligned.
        let long = "the quick brown fox jumps over the lazy dog and then \
                    keeps jumping for several more clauses just to ensure \
                    we cross the wrap boundary at the chosen width";
        let bookmarked = BTreeSet::new();
        let lines = render_passage(
            &passage_in("PSA", 1, vec![v(1, long)]),
            99,
            None,
            &bookmarked,
            None,
            40,
        );
        let v1_lines: Vec<&RenderedLine> = lines.iter().filter(|rl| rl.verse == 1).collect();
        assert!(v1_lines.len() >= 2, "verse must wrap to multiple lines");
        for cont in &v1_lines[1..] {
            let prefix = cont.line.spans[0].content.as_ref();
            assert_eq!(
                prefix.chars().count(),
                PANEL_PAD + VERSE_PREFIX + POETRY_INDENT,
                "poetry continuation indent must include the inset, got {prefix:?}",
            );
            assert!(
                prefix.chars().all(|c| c == ' '),
                "continuation indent should be all spaces",
            );
        }
    }

    #[test]
    fn poetry_cursor_row_still_fills_wrap_width() {
        // The full-row highlight must still span the pane on a poetic cursor
        // row — the inset spans count toward the fill, not against it.
        let bookmarked = BTreeSet::new();
        let lines = render_passage(
            &passage_in("PSA", 23, vec![v(1, "alpha"), v(2, "beta")]),
            2,
            None,
            &bookmarked,
            None,
            40,
        );
        let used: usize = line_for_verse(&lines, 2)
            .line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(
            used, 40,
            "poetry cursor row must pad to the full wrap width"
        );
    }

    #[test]
    fn cursor_verse_renders_gutter_arrow() {
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, None, 80);
        let cursor_line = line_for_verse(&lines, 2);
        assert_eq!(
            gutter_glyph(cursor_line),
            CURSOR_GLYPH,
            "cursor row's gutter should be the ▸ glyph; got {:?}",
            line_text(cursor_line),
        );
        // Non-cursor row's gutter is a single space, not any of the marker glyphs.
        let other = line_for_verse(&lines, 1);
        let g = gutter_glyph(other);
        assert_eq!(g, " ", "non-cursor gutter should be blank, got {g:?}");
    }

    #[test]
    fn bookmarked_verse_shows_star_when_not_cursor() {
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma")], vec![]);
        let mut bookmarked = BTreeSet::new();
        bookmarked.insert(2);
        let lines = render_passage(&p, 1, None, &bookmarked, None, 80);
        let starred = line_for_verse(&lines, 2);
        assert_eq!(
            gutter_glyph(starred),
            BOOKMARK_GLYPH,
            "bookmarked non-cursor row should display ★",
        );
        // When the cursor sits on the bookmarked verse, the cursor glyph wins.
        let lines = render_passage(&p, 2, None, &bookmarked, None, 80);
        assert_eq!(
            gutter_glyph(line_for_verse(&lines, 2)),
            CURSOR_GLYPH,
            "cursor glyph must outrank the bookmark glyph on the same row",
        );
    }

    #[test]
    fn selection_marks_anchor_and_cursor_distinctly() {
        // Multi-verse selection 2..4 with the cursor (head) at 4: verse 4 keeps
        // ▸, the anchor end (2) gets a subtle ┃ tick, and the interior (3) stays
        // blank — the bright_cyan fill marks the extent, so a tick there would
        // be clutter.
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma"), v(4, "delta")],
            vec![],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 4, Some((2, 4)), &bookmarked, None, 80);
        assert_eq!(
            gutter_glyph(line_for_verse(&lines, 2)),
            "\u{2503}",
            "anchor end carries the ┃ tick",
        );
        assert_eq!(
            gutter_glyph(line_for_verse(&lines, 3)),
            " ",
            "interior blank"
        );
        assert_eq!(
            gutter_glyph(line_for_verse(&lines, 4)),
            CURSOR_GLYPH,
            "cursor head"
        );
    }

    #[test]
    fn cursor_row_has_full_width_teal_fill() {
        // Cursor verse renders as a continuous teal bar: pad, gutter, body and
        // trailing pad all share the teal bg. The verse NUMBER is the one
        // intentional exception — an inverse-video yellow chip. Other verses
        // keep the pane's blue bg.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, None, 40);

        let cursor_line = line_for_verse(&lines, 2);
        for (i, span) in cursor_line.line.spans.iter().enumerate() {
            let bg = span.style.bg;
            assert!(
                bg == Some(theme::cursor_row_bg()) || bg == Some(theme::yellow()),
                "cursor row span #{i} ({:?}) bg {bg:?} should be the teal row fill \
                 or the yellow number chip",
                span.content,
            );
        }
        // The row fill must extend to the full wrap width — otherwise the
        // highlight ends raggedly mid-line.
        let row_width: usize = cursor_line
            .line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(
            row_width, 40,
            "cursor row should pad to the full wrap width (40), got {row_width}",
        );

        // Idle verse keeps the standard pane bg on every span.
        let idle_line = line_for_verse(&lines, 1);
        for span in &idle_line.line.spans {
            assert_eq!(
                span.style.bg,
                Some(theme::blue()),
                "non-cursor row spans must stay on blue bg",
            );
        }
    }

    #[test]
    fn peer_verse_gets_dim_teal_fill_distinct_from_cursor() {
        // The cross-pane cue: an unfocused pane faintly tints the verse whose
        // number matches the focused pane's cursor. The fill is the dim
        // peer_row_bg (input_teal) — a *different* teal from this pane's own
        // cursor row — and the verse number stays yellow (its scanning rhythm
        // survives) rather than flipping to the cursor's inverse-video chip.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma")], vec![]);
        let bookmarked = BTreeSet::new();
        // Cursor on verse 1 (this pane), peer cue on verse 3 (the other pane).
        let lines = render_passage(&p, 1, None, &bookmarked, Some(3), 40);

        let peer_line = line_for_verse(&lines, 3);
        // Every span on the peer row carries the dim peer fill (the number is
        // yellow-on-peer-fill, not a separate chip bg).
        for span in &peer_line.line.spans {
            assert_eq!(
                span.style.bg,
                Some(theme::peer_row_bg()),
                "peer row span {:?} must use the dim peer teal fill",
                span.content,
            );
        }
        // The peer fill must NOT be the local cursor's teal — that would read
        // as a second cursor in the same pane.
        assert_ne!(
            theme::peer_row_bg(),
            theme::cursor_row_bg(),
            "peer cue must be a distinct teal from the cursor row",
        );
        // The peer row carries no ▸ cursor arrow (it's passive, read-only).
        assert_eq!(
            gutter_glyph(peer_line),
            " ",
            "peer row must not draw a cursor arrow",
        );
        // The peer row pads to the full wrap width like other highlighted rows.
        let used: usize = peer_line
            .line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(used, 40, "peer row must fill the wrap width");
    }

    #[test]
    fn peer_verse_missing_in_pane_highlights_nothing() {
        // If the focused pane's cursor verse doesn't exist in this pane (the
        // common case — versification differs), nothing is highlighted: every
        // row stays idle, no error.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, Some(99), 40);
        for verse in [2] {
            let row = line_for_verse(&lines, verse);
            for span in &row.line.spans {
                assert_eq!(
                    span.style.bg,
                    Some(theme::blue()),
                    "no verse should carry the peer fill when the peer verse is absent",
                );
            }
        }
    }

    #[test]
    fn local_cursor_outranks_peer_cue_on_same_verse() {
        // Where a pane's own cursor and the peer cue land on the same verse
        // number, the local cursor wins — the pane's own state is never masked
        // by the passive cross-pane hint.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, Some(2), 40);
        let row = line_for_verse(&lines, 2);
        assert_eq!(
            gutter_glyph(row),
            CURSOR_GLYPH,
            "local cursor keeps its ▸ even when the peer cue coincides",
        );
        assert!(
            row.line
                .spans
                .iter()
                .any(|s| s.style.bg == Some(theme::cursor_row_bg())),
            "the row must read as the local cursor (teal), not the dim peer fill",
        );
    }

    #[test]
    fn selection_rows_use_bright_cyan_row_fill() {
        // Every verse in a visual selection — including the cursor end — gets
        // the brightest-cyan slab; the cursor is set apart only by its ▸ glyph
        // (see selection_gutter_is_blank_while_cursor_keeps_arrow), not a
        // different fill. Idle rows stay on the blue pane.
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma"), v(4, "delta")],
            vec![],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 4, Some((2, 4)), &bookmarked, None, 40);

        let row_bg = |verse: i64| {
            line_for_verse(&lines, verse).line.spans[0]
                .style
                .bg
                .expect("span must have a bg")
        };
        assert_eq!(row_bg(2), theme::selection_bg(), "selected non-cursor");
        assert_eq!(row_bg(3), theme::selection_bg(), "selected non-cursor");
        assert_eq!(
            row_bg(4),
            theme::selection_bg(),
            "cursor verse is part of the bright-cyan range",
        );
        assert_eq!(row_bg(1), theme::blue(), "idle row stays on pane bg");

        // Selection rows pad to full wrap width, same as the cursor row.
        let used: usize = line_for_verse(&lines, 2)
            .line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(used, 40, "selection row must fill the wrap width");
    }

    #[test]
    fn heading_anchored_before_verse_appears_in_flow() {
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta")],
            vec![Heading {
                before_verse: 2,
                style: "s1".into(),
                text: "Section heading".into(),
            }],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, None, 80);
        let heading_idx = lines
            .iter()
            .position(|rl| line_text(rl).contains("Section heading"))
            .expect("heading text should appear in rendered output");
        // Heading lands above the verse it anchors before.
        let v2_idx = lines.iter().position(|rl| rl.verse == 2).expect("verse 2");
        assert!(
            heading_idx < v2_idx,
            "heading should appear before its anchor verse (heading={heading_idx}, v2={v2_idx})",
        );
    }

    #[test]
    fn parallel_heading_style_is_suppressed_from_reading_flow() {
        // `r`-style headings go to the sidebar, not the body. The reading
        // flow must not include them.
        let p = passage_with(
            vec![v(1, "alpha")],
            vec![Heading {
                before_verse: 1,
                style: "r".into(),
                text: "Parallel: Mark 1:1".into(),
            }],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, None, 80);
        for rl in &lines {
            assert!(
                !line_text(rl).contains("Parallel: Mark 1:1"),
                "r-style heading leaked into reading flow",
            );
        }
    }

    #[test]
    fn wrapped_lines_hang_indent_under_the_verse_number() {
        // Long verse text should wrap; subsequent lines share the same
        // `verse` id but start with VERSE_PREFIX worth of spaces, not the
        // verse-number column.
        let long = "the quick brown fox jumps over the lazy dog and then \
                    keeps jumping for several more clauses just to ensure \
                    we cross the wrap boundary at the chosen width";
        let p = passage_with(vec![v(1, long)], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, None, 40);
        let v1_lines: Vec<&RenderedLine> = lines.iter().filter(|rl| rl.verse == 1).collect();
        assert!(
            v1_lines.len() >= 2,
            "expected the verse to wrap into multiple lines",
        );
        // Subsequent (wrapped) lines have a single prefix span of pure
        // spaces, exactly VERSE_PREFIX wide.
        for cont in &v1_lines[1..] {
            let prefix = cont.line.spans[0].content.as_ref();
            assert_eq!(
                prefix.chars().count(),
                PANEL_PAD + VERSE_PREFIX,
                "wrapped line's first span should be PANEL_PAD+VERSE_PREFIX spaces, got {prefix:?}",
            );
            assert!(
                prefix.chars().all(|c| c == ' '),
                "wrapped indent prefix should be all spaces",
            );
        }
    }

    #[test]
    fn pressing_v_with_no_movement_highlights_one_verse() {
        // Visual mode with anchor == cursor (`v` then no motion) yields
        // selection == Some((c, c)). That single verse must read as the
        // brightest-cyan selection (immediate confirmation) AND keep its ▸
        // glyph — not as the calmer normal-mode teal cursor.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, Some((2, 2)), &bookmarked, None, 40);
        let row = line_for_verse(&lines, 2);
        assert_eq!(gutter_glyph(row), CURSOR_GLYPH, "cursor keeps its arrow");
        assert!(
            row.line
                .spans
                .iter()
                .any(|s| s.style.bg == Some(theme::selection_bg())),
            "the single visual verse must use the brightest-cyan selection fill",
        );
        assert!(
            row.line
                .spans
                .iter()
                .all(|s| s.style.bg != Some(theme::cursor_row_bg())),
            "it must not read as the normal-mode teal cursor",
        );
    }

    #[test]
    fn cursor_verse_number_is_inverse_video() {
        // The cursor verse's number is the one inverse-video chip: teal text
        // on a yellow bg, so the position pops while other numbers keep the
        // yellow-on-blue scanning rhythm.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, None, 40);
        let row = line_for_verse(&lines, 2);
        let num = row
            .line
            .spans
            .iter()
            .find(|s| {
                let t = s.content.trim();
                !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
            })
            .expect("verse-number span");
        assert_eq!(num.style.bg, Some(theme::yellow()));
        assert_eq!(num.style.fg, Some(theme::cursor_row_bg()));
    }

    #[test]
    fn three_digit_verse_number_keeps_body_aligned() {
        // The number field is exactly VERSE_NUM_WIDTH cells for both 1- and
        // 3-digit numbers, followed by a fixed 2-space gutter gap, so
        // Psalm-119-style verses don't shift the body. Span layout:
        // [PANEL_PAD pad][gutter][number][2-space gap][body…].
        let p = passage_with(vec![v(9, "alpha"), v(119, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, None, 40);
        for verse in [9, 119] {
            let spans = &line_for_verse(&lines, verse).line.spans;
            assert_eq!(
                spans[PANEL_PAD + 1].content.chars().count(),
                VERSE_NUM_WIDTH,
                "verse {verse} number field must be a fixed {VERSE_NUM_WIDTH} cells",
            );
            assert_eq!(
                spans[PANEL_PAD + 2].content,
                "  ",
                "verse {verse} number is followed by a fixed 2-space gutter gap",
            );
        }
    }

    #[test]
    fn footnote_and_xref_markers_use_cp437_glyphs() {
        // Footnotes render `*`, cross-refs `+`, both dim (light_grey) and
        // non-bold so they read as secondary metadata, not prose.
        let mut verse = v(1, "alpha");
        verse.footnote_count = 1;
        verse.xref_note_count = 1;
        let p = passage_with(vec![verse], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 5, None, &bookmarked, None, 80);
        let row = line_for_verse(&lines, 1);
        let text = line_text(row);
        assert!(text.contains('*'), "footnote marker * missing: {text:?}");
        assert!(text.contains('+'), "xref marker + missing: {text:?}");
        let marker = row
            .line
            .spans
            .iter()
            .find(|s| s.content.contains('*') || s.content.contains('+'))
            .expect("marker span");
        assert_eq!(
            marker.style.fg,
            Some(theme::light_grey()),
            "markers are dim"
        );
        assert!(
            !marker
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD),
            "markers must not be bold",
        );
    }

    #[test]
    fn body_text_caps_at_max_width_on_wide_panes() {
        // On a very wide pane the verse body must still wrap at the readable
        // cap (MAX_BODY_WIDTH), not stretch to the full width. Without the cap
        // this ~180-char verse would fit on a single line at wrap_width 200.
        let long = "the quick brown fox jumps over the lazy dog and then keeps \
                    jumping through several more clauses written purely to ensure \
                    we comfortably clear the seventy-column boundary on a wide pane";
        let p = passage_with(vec![v(1, long)], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 1, None, &bookmarked, None, 200);
        let body_lines = lines.iter().filter(|rl| rl.verse == 1).count();
        assert!(
            body_lines >= 2,
            "wide pane must still wrap the body at the cap, got {body_lines} line(s)",
        );
    }

    #[test]
    fn paging_moves_by_lines_when_verses_are_single_line() {
        // Wide wrap → every verse is one row, so the layout is
        // [blank, v1, v2, … v10] and a `line_delta` maps 1:1 onto verses.
        let verses: Vec<Verse> = (1..=10).map(|n| v(n, "short")).collect();
        let p = passage_with(verses, vec![]);
        assert_eq!(
            verse_after_paging(&p, 1, 80, 4),
            5,
            "down 4 lines from v1 lands on v5",
        );
        assert_eq!(
            verse_after_paging(&p, 5, 80, -3),
            2,
            "up 3 lines from v5 lands on v2",
        );
    }

    #[test]
    fn paging_accounts_for_wrapped_verse_height() {
        // A verse that wraps to several rows counts for its full height: paging
        // a couple of lines down from v1 stays *inside* v2, it doesn't skip it.
        let long = "the quick brown fox jumps over the lazy dog and keeps on \
                    going well past the wrap boundary so this verse occupies \
                    several rows on a narrow pane";
        let p = passage_with(vec![v(1, "a"), v(2, long), v(3, "c")], vec![]);
        let wrap = 40;
        let rendered = render_passage(&p, 1, None, &BTreeSet::new(), None, wrap);
        assert!(
            rendered.iter().filter(|rl| rl.verse == 2).count() >= 2,
            "precondition: v2 must wrap to multiple rows",
        );
        assert_eq!(
            verse_after_paging(&p, 1, wrap, 2),
            2,
            "2 lines down from v1 lands inside the wrapped v2, not on v3",
        );
    }

    #[test]
    fn paging_clamps_at_chapter_ends() {
        let verses: Vec<Verse> = (1..=5).map(|n| v(n, "short")).collect();
        let p = passage_with(verses, vec![]);
        assert_eq!(
            verse_after_paging(&p, 5, 80, 999),
            5,
            "paging past the end clamps to the last verse",
        );
        assert_eq!(
            verse_after_paging(&p, 1, 80, -999),
            1,
            "paging past the top clamps to the first verse",
        );
    }

    #[test]
    fn paging_onto_a_heading_resolves_to_a_real_verse() {
        // The heading inserts blank/heading/blank rows before v3. A page that
        // lands in that block must resolve to a real verse, never 0.
        let p = passage_with(
            vec![v(1, "a"), v(2, "b"), v(3, "c")],
            vec![Heading {
                before_verse: 3,
                style: "s1".into(),
                text: "Section".into(),
            }],
        );
        for delta in 1..=5 {
            let got = verse_after_paging(&p, 1, 80, delta);
            assert!(got > 0, "delta {delta} resolved to a non-verse row ({got})");
        }
    }

    #[test]
    fn paging_zero_lines_stays_put() {
        let p = passage_with(vec![v(1, "a"), v(2, "b"), v(3, "c")], vec![]);
        assert_eq!(verse_after_paging(&p, 2, 80, 0), 2);
    }
}
