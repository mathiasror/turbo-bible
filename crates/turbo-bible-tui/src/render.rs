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
    // Verse number: yellow+bold for idle verses. On the cursor verse we swap
    // to bright_white+bold so the number visually leads into the brighter
    // prose body below; on a non-cursor verse that's bookmarked, idle yellow
    // is still distinct from the verse-text foreground.
    let verse_num_style = |on_cursor: bool| {
        let fg = if on_cursor {
            theme::bright_white()
        } else {
            theme::yellow()
        };
        Style::new()
            .fg(fg)
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD)
    };
    // Three-tier body brightness ladder so the cursor doesn't disappear
    // inside a long visual selection: idle = light_grey, selection =
    // bright_white, cursor = bright_white + BOLD. The cursor's bolder
    // weight reads as a focus even when the entire pane is selected.
    let verse_text_style = |is_cursor: bool, in_sel: bool| {
        let fg = if is_cursor || in_sel {
            theme::bright_white()
        } else {
            theme::light_grey()
        };
        let mut s = Style::new().fg(fg).bg(theme::blue());
        if is_cursor {
            s = s.add_modifier(Modifier::BOLD);
        }
        s
    };
    let marker_style = Style::new()
        .fg(theme::yellow())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);

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
        let on_cursor = is_cursor_verse || in_selection;
        // Gutter glyph (1 col): cursor wins, then selection bar, then bookmark
        // star, else blank. Styled in cyan/yellow on the pane bg so the column
        // reads as a quiet accent rather than a row-wide highlight.
        let (gutter_glyph, gutter_style) = if is_cursor_verse {
            (
                "\u{25B8}",
                Style::new()
                    .fg(theme::cyan())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            )
        } else if in_selection {
            ("\u{258E}", Style::new().fg(theme::cyan()).bg(theme::blue()))
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
            out.push(RenderedLine {
                line: Line::from(spans),
                verse: v.number,
            });
        };
        let (first, rest) = chunks.split_first().expect("chunks is non-empty above");
        push_line(
            vec![
                Span::styled(gutter_glyph.to_string(), gutter_style),
                Span::styled(num_str, verse_num_style(on_cursor)),
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
    const SELECTION_GLYPH: &str = "\u{258E}";
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
    fn selection_glyph_appears_on_non_cursor_selected_rows() {
        let p = passage_with(
            vec![v(1, "alpha"), v(2, "beta"), v(3, "gamma"), v(4, "delta")],
            vec![],
        );
        let bookmarked = BTreeSet::new();
        let lines = render_passage(&p, 4, Some((2, 4)), &bookmarked, 80);
        assert_eq!(
            line_for_verse(&lines, 2).line.spans[0].content.as_ref(),
            SELECTION_GLYPH,
        );
        assert_eq!(
            line_for_verse(&lines, 3).line.spans[0].content.as_ref(),
            SELECTION_GLYPH,
        );
        // Cursor verse inside the selection still shows the cursor glyph.
        assert_eq!(
            line_for_verse(&lines, 4).line.spans[0].content.as_ref(),
            CURSOR_GLYPH,
        );
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
