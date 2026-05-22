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
            (
                "\u{258E}",
                Style::new().fg(theme::cyan()).bg(theme::blue()),
            )
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
