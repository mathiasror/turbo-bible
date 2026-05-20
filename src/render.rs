//! Build the visual representation of a chapter: interleave headings, inject
//! footnote markers, and produce a `Vec<Line>` ready for ratatui.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::db::Passage;
use crate::theme;

/// One line on screen, tagged with the verse it belongs to (if any).
#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub line: Line<'static>,
    /// Verse number this line belongs to (0 if it's a heading or blank).
    #[allow(dead_code)]
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
    two_line_verses: bool,
) -> Vec<RenderedLine> {
    let mut out: Vec<RenderedLine> = Vec::new();

    let heading_style = Style::new()
        .fg(theme::bright_white())
        .bg(theme::blue())
        .add_modifier(Modifier::BOLD);
    let cursor_bg = theme::cyan();
    let verse_num_style = |on_cursor: bool| {
        let mut s = Style::new()
            .fg(theme::yellow())
            .add_modifier(Modifier::BOLD);
        s = if on_cursor { s.bg(cursor_bg) } else { s.bg(theme::blue()) };
        s
    };
    // Non-cursor body text uses a softer fg so the cursor line is the only
    // bright-white prose on the page — easier on the eyes during a long
    // reading session, and the cursor stands out more without louder bg.
    let verse_text_style = |on_cursor: bool| {
        if on_cursor {
            Style::new().fg(theme::bright_white()).bg(cursor_bg)
        } else {
            Style::new().fg(theme::light_grey()).bg(theme::blue())
        }
    };
    let marker_style = |on_cursor: bool| {
        let s = Style::new()
            .fg(theme::yellow())
            .add_modifier(Modifier::BOLD);
        if on_cursor { s.bg(cursor_bg) } else { s.bg(theme::blue()) }
    };

    // Pre-bucket headings by `before_verse`.
    let mut headings_by_anchor: std::collections::BTreeMap<i64, Vec<&crate::db::Heading>> =
        std::collections::BTreeMap::new();
    for h in &p.headings {
        headings_by_anchor.entry(h.before_verse).or_default().push(h);
    }

    // Chapter banner. The rule underneath the heading anchors verse 1 to it
    // without a trailing blank line (which used to read as a missing verse 0).
    out.push(rl_blank());
    out.push(RenderedLine {
        line: Line::from(Span::styled(
            format!("{} {}", p.book_name, p.chapter),
            heading_style,
        )),
        verse: 0,
    });
    out.push(RenderedLine {
        line: Line::from(Span::styled(
            "─".repeat(p.book_name.len() + p.chapter.to_string().len() + 1),
            Style::new().fg(theme::cyan()).bg(theme::blue()),
        )),
        verse: 0,
    });

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
                let style = heading_style;
                if !emitted {
                    // Already a blank line from the previous verse; don't
                    // double it before the heading.
                    if !out.last().map_or(true, is_blank) {
                        out.push(rl_blank());
                    }
                    emitted = true;
                }
                out.push(RenderedLine {
                    line: Line::from(Span::styled(h.text.clone(), style)),
                    verse: 0,
                });
            }
            if emitted {
                out.push(rl_blank());
            }
        }

        let in_selection = selection.map_or(false, |(s, e)| v.number >= s && v.number <= e);
        let is_cursor_verse = v.number == cursor_verse;
        let on_cursor = is_cursor_verse || in_selection;
        // The gutter glyph: cursor wins (so the active verse is always
        // unambiguous, especially in a visual range), bookmark next, else
        // blank.
        let gutter = if is_cursor_verse {
            "\u{25B8}"
        } else if bookmarked.contains(&v.number) {
            "\u{2605}"
        } else {
            " "
        };
        let num_str = format!("{}{:>width$}", gutter, v.number, width = VERSE_NUM_WIDTH - 1);

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
            // Try to fit the marker on the last line; if not, push to a new
            // wrapped line.
            let last_len = chunks.last().map(|s| s.chars().count()).unwrap_or(0);
            if last_len + glyph.chars().count() <= body_w {
                if let Some(last) = chunks.last_mut() {
                    last.push_str(&glyph);
                }
            } else {
                chunks.push(glyph.trim_start().to_string());
            }
        }

        // In two-line mode, emit the verse number on its own line, then the
        // text wrapped + indented under it.
        if two_line_verses {
            out.push(RenderedLine {
                line: Line::from(vec![Span::styled(
                    format!("{num_str}  "),
                    verse_num_style(on_cursor),
                )]),
                verse: v.number,
            });
        }

        for (i, chunk) in chunks.iter().enumerate() {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if i == 0 && !two_line_verses {
                spans.push(Span::styled(
                    format!("{num_str}  "),
                    verse_num_style(on_cursor),
                ));
            } else {
                // Hanging indent — keep the gutter styled like text so the
                // cursor highlight (if any) reads as a coherent block.
                spans.push(Span::styled(
                    " ".repeat(VERSE_PREFIX),
                    verse_text_style(on_cursor),
                ));
            }
            // Split the marker tail back out so it gets the marker style.
            let (body, tail) = match chunk.rfind(' ') {
                Some(pos) if chunk[pos + 1..].chars().all(is_marker_glyph) => {
                    (&chunk[..pos], &chunk[pos..])
                }
                _ => (chunk.as_str(), ""),
            };
            spans.push(Span::styled(body.to_string(), verse_text_style(on_cursor)));
            if !tail.is_empty() {
                spans.push(Span::styled(tail.to_string(), marker_style(on_cursor)));
            }
            out.push(RenderedLine {
                line: Line::from(spans),
                verse: v.number,
            });
        }
        // Breathing room between verses.
        out.push(rl_blank());
    }

    out
}

fn is_marker_glyph(c: char) -> bool {
    c == 'ᶠ' || c == 'ˣ' || c == ' '
}

/// Greedy word-wrap that splits on whitespace.
fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
            continue;
        }
        if current.chars().count() + 1 + word.chars().count() <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
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

/// Pad/extend every line to the given width with the verse-text style so the
/// blue background fills the window cleanly (no terminal default bg bleeding
/// through). The cursor / selection rows extend their cyan highlight all the
/// way to the right margin so they read as a contiguous selected list item
/// (matching how two-line mode already behaved).
pub fn pad_to_width(lines: &[RenderedLine], width: u16) -> Vec<Line<'static>> {
    let cursor_bg = theme::cyan();
    let blue_bg = Style::new().fg(theme::bright_white()).bg(theme::blue());
    let cursor_pad = Style::new().fg(theme::bright_white()).bg(cursor_bg);
    lines
        .iter()
        .map(|rl| {
            let used: usize = rl.line.spans.iter().map(|s| s.content.chars().count()).sum();
            let mut spans = rl.line.spans.clone();
            if (used as u16) < width {
                let pad = (width as usize).saturating_sub(used);
                let is_cursor_row = rl
                    .line
                    .spans
                    .last()
                    .and_then(|s| s.style.bg)
                    .map_or(false, |c| c == cursor_bg);
                let pad_style = if is_cursor_row { cursor_pad } else { blue_bg };
                spans.push(Span::styled(" ".repeat(pad), pad_style));
            }
            Line::from(spans)
        })
        .collect()
}
