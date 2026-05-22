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
/// Number column + two spaces of gutter — used both for the verse prefix and
/// for the hanging indent of wrapped lines.
const VERSE_PREFIX: usize = VERSE_NUM_WIDTH + 2;

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
    wrap_width: u16,
) -> Vec<RenderedLine> {
    let mut out: Vec<RenderedLine> = Vec::new();

    let heading_style = Style::new()
        .fg(theme::bright_white())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);
    // Three-tier row palette, period-correct Turbo Vision:
    //   idle:                  blue pane bg, light_grey body, yellow num
    //   in selection (no cur): light_grey bg, black body, black BOLD num — the
    //                          classic TV "selected list item / selected text"
    //                          look (reverse-video on dialog grey)
    //   cursor:                cyan bg, bright_white BOLD body, yellow BOLD
    //                          num — the brightest row anywhere in the pane
    let verse_num_style = |is_cursor: bool, in_selection: bool| {
        if is_cursor {
            Style::new()
                .fg(theme::yellow())
                .bg(theme::cyan())
                .add_modifier(Modifier::BOLD)
        } else if in_selection {
            Style::new()
                .fg(theme::black())
                .bg(theme::light_grey())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new()
                .fg(theme::yellow())
                .bg(theme::blue())
                .add_modifier(Modifier::BOLD)
        }
    };
    let verse_text_style = |is_cursor: bool, in_sel: bool| {
        if is_cursor {
            Style::new()
                .fg(theme::bright_white())
                .bg(theme::cyan())
                .add_modifier(Modifier::BOLD)
        } else if in_sel {
            Style::new().fg(theme::black()).bg(theme::light_grey())
        } else {
            Style::new().fg(theme::light_grey()).bg(theme::blue())
        }
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
                    line: Line::from(Span::styled(h.text.clone(), heading_style)),
                    verse: 0,
                });
            }
            if emitted {
                out.push(rl_blank());
            }
        }

        let in_selection = selection.is_some_and(|(s, e)| v.number >= s && v.number <= e);
        let is_cursor_verse = v.number == cursor_verse;
        // Per-verse row bg: cyan for the cursor's full-row highlight,
        // light_grey for the rest of a visual selection (period-correct TV
        // selection), blue elsewhere. Gutter, number, body, marker and
        // right-edge padding all share this bg so each row reads as one
        // continuous bar.
        let row_bg = if is_cursor_verse {
            theme::cyan()
        } else if in_selection {
            theme::light_grey()
        } else {
            theme::blue()
        };
        // Markers (ᶠ / ˣ) ride the row bg. Cursor row keeps yellow accent;
        // selection row uses black to match the inverted body text — dark_grey
        // on light_grey was too low-contrast to spot footnote/xref glyphs at
        // the end of a selected verse.
        let marker_fg = if in_selection && !is_cursor_verse {
            theme::black()
        } else {
            theme::yellow()
        };
        let marker_style = Style::new()
            .fg(marker_fg)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD);
        // Gutter glyph (1 col): cursor's ▸ in bright_white reads as a
        // pointer, not a number-tone accent. Selection rows leave the
        // gutter blank — the row fill already marks the extent, and a
        // ▎ tick on top reads as redundant clutter. Bookmark star and
        // idle space unchanged.
        let (gutter_glyph, gutter_style) = if is_cursor_verse {
            (
                "\u{25B8}",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::cyan())
                    .add_modifier(Modifier::BOLD),
            )
        } else if in_selection {
            (" ", Style::new().bg(theme::light_grey()))
        } else if bookmarked.contains(&v.number) {
            (
                "\u{2605}",
                Style::new().fg(theme::yellow()).bg(theme::blue()),
            )
        } else {
            (" ", Style::new().bg(theme::blue()))
        };
        let num_str = format!("{:>width$}  ", v.number, width = VERSE_NUM_WIDTH - 1);

        let mut markers = String::new();
        if v.footnote_count > 0 {
            markers.push('ᶠ');
        }
        if v.xref_note_count > 0 {
            markers.push('ˣ');
        }

        // Pre-wrap the verse text so wrapped lines hang-indent under the
        // verse number gutter. Append the marker glyph to the last chunk so
        // it sits at the very end of the verse, not on a row of its own.
        let text = v.text.replace('\n', " ");
        let body_w = (wrap_width as usize).saturating_sub(VERSE_PREFIX).max(20);
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
        let body_style = verse_text_style(is_cursor_verse, in_selection);
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
            if is_cursor_verse || in_selection {
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
        push_line(
            vec![
                Span::styled(gutter_glyph.to_string(), gutter_style),
                Span::styled(num_str, verse_num_style(is_cursor_verse, in_selection)),
            ],
            first,
        );
        for chunk in rest {
            push_line(
                vec![Span::styled(" ".repeat(VERSE_PREFIX), body_style)],
                chunk,
            );
        }
    }

    out
}

const fn is_marker_glyph(c: char) -> bool {
    c == 'ᶠ' || c == 'ˣ' || c == ' '
}

/// Find the first line index that belongs to a given verse, for scroll
/// targeting. Returns 0 if no match.
pub fn line_index_for_verse(lines: &[RenderedLine], verse: i64) -> usize {
    lines.iter().position(|rl| rl.verse == verse).unwrap_or(0)
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

    #[test]
    fn cursor_verse_renders_gutter_glyph_first() {
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, 80);
        let cursor_line = line_for_verse(&lines, 2);
        assert_eq!(
            cursor_line.line.spans[0].content.as_ref(),
            CURSOR_GLYPH,
            "cursor row's first span should be the ▸ glyph; got {:?}",
            line_text(cursor_line),
        );
        // Non-cursor row's gutter is a single space, not any of the marker glyphs.
        let other = line_for_verse(&lines, 1);
        let g = other.line.spans[0].content.as_ref();
        assert_eq!(g, " ", "non-cursor gutter should be blank, got {g:?}");
    }

    #[test]
    fn bookmarked_verse_shows_star_when_not_cursor() {
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma")], vec![]);
        let mut bookmarked = BTreeSet::new();
        bookmarked.insert(2);
        let lines = render_passage(&p, 1, None, &bookmarked, 80);
        let starred = line_for_verse(&lines, 2);
        assert_eq!(
            starred.line.spans[0].content.as_ref(),
            BOOKMARK_GLYPH,
            "bookmarked non-cursor row should display ★",
        );
        // When the cursor sits on the bookmarked verse, the cursor glyph wins.
        let lines = render_passage(&p, 2, None, &bookmarked, 80);
        assert_eq!(
            line_for_verse(&lines, 2).line.spans[0].content.as_ref(),
            CURSOR_GLYPH,
            "cursor glyph must outrank the bookmark glyph on the same row",
        );
    }

    #[test]
    fn selection_gutter_is_blank_while_cursor_keeps_arrow() {
        // Selection rows leave the gutter blank — the light_grey row fill
        // already marks the extent, so a separate ▎ tick on top reads as
        // clutter. The cursor inside the selection still shows ▸.
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma"), v(4, "delta")],
            vec![],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 4, Some((2, 4)), &bookmarked, 80);
        assert_eq!(
            line_for_verse(&lines, 2).line.spans[0].content.as_ref(),
            " ",
        );
        assert_eq!(
            line_for_verse(&lines, 3).line.spans[0].content.as_ref(),
            " ",
        );
        assert_eq!(
            line_for_verse(&lines, 4).line.spans[0].content.as_ref(),
            CURSOR_GLYPH,
        );
    }

    #[test]
    fn cursor_row_has_full_width_cyan_fill() {
        // Cursor verse should render with a continuous cyan row fill: the
        // gutter, verse-number column, body text and trailing pad all share
        // the cyan bg so the highlight reads as one horizontal bar. Other
        // verses keep the pane's blue bg.
        let p = passage_with(vec![v(1, "alpha"), v(2, "beta")], vec![]);
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 2, None, &bookmarked, 40);

        let cursor_line = line_for_verse(&lines, 2);
        for (i, span) in cursor_line.line.spans.iter().enumerate() {
            assert_eq!(
                span.style.bg,
                Some(theme::cyan()),
                "cursor row span #{i} ({:?}) must sit on cyan bg for the row fill to be continuous",
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
    fn selection_rows_use_light_grey_row_fill() {
        // Visual-selection non-cursor verses get a continuous light_grey
        // row fill — the classic Turbo Vision "selected item" look. The
        // cursor inside the selection keeps the brighter cyan fill so the
        // focus is unambiguous.
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma"), v(4, "delta")],
            vec![],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 4, Some((2, 4)), &bookmarked, 40);

        let row_bg = |verse: i64| {
            line_for_verse(&lines, verse).line.spans[0]
                .style
                .bg
                .expect("span must have a bg")
        };
        assert_eq!(row_bg(2), theme::light_grey(), "selected non-cursor");
        assert_eq!(row_bg(3), theme::light_grey(), "selected non-cursor");
        assert_eq!(row_bg(4), theme::cyan(), "cursor verse keeps cyan fill");
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
        let lines = render_passage(&p, 1, None, &bookmarked, 80);
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
        let lines = render_passage(&p, 1, None, &bookmarked, 80);
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
        let lines = render_passage(&p, 1, None, &bookmarked, 40);
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
                VERSE_PREFIX,
                "wrapped line's first span should be VERSE_PREFIX spaces, got {prefix:?}",
            );
            assert!(
                prefix.chars().all(|c| c == ' '),
                "wrapped indent prefix should be all spaces",
            );
        }
    }
}
