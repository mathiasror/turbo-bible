//! Find dialog (F3 / `/`). FTS5 search with live results.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::{Book, Db};
use crate::nav::Position;
use crate::search::{self, SearchHit};
use crate::theme;
use crate::ui::dialog;

/// Snippet indent (cols) and the maximum lines a single hit's snippet wraps to
/// before it's truncated with an ellipsis. Each hit is a reference row + 1–2
/// snippet rows + a separator blank; rows are budgeted dynamically at render.
const SNIPPET_INDENT: usize = 4;
const SNIPPET_MAX_LINES: usize = 2;

pub struct FindDialog {
    input: String,
    results: Vec<SearchHit>,
    selected: usize,
    error: Option<String>,
    /// Active translation code — drives the locale reference separator.
    translation: String,
}

#[non_exhaustive]
pub enum FindOutcome {
    Continue,
    Cancel,
    Jump(Position, String), // position + the query that produced the hit, for n/N
}

impl FindDialog {
    pub fn new(translation: &str) -> Self {
        Self {
            input: String::new(),
            results: Vec::new(),
            selected: 0,
            error: None,
            translation: translation.to_string(),
        }
    }

    pub fn handle(&mut self, key: KeyEvent, db: &Db) -> FindOutcome {
        match key.code {
            KeyCode::Esc => FindOutcome::Cancel,
            KeyCode::Enter => {
                if let Some(hit) = self.results.get(self.selected) {
                    FindOutcome::Jump(
                        Position {
                            book: hit.book.clone(),
                            chapter: hit.chapter,
                            verse: Some(hit.verse),
                        },
                        self.input.clone(),
                    )
                } else {
                    FindOutcome::Continue
                }
            }
            KeyCode::Down => {
                if !self.results.is_empty() {
                    self.selected = (self.selected + 1).min(self.results.len() - 1);
                }
                FindOutcome::Continue
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                FindOutcome::Continue
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.refresh(db);
                FindOutcome::Continue
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                self.refresh(db);
                FindOutcome::Continue
            }
            _ => FindOutcome::Continue,
        }
    }

    fn refresh(&mut self, db: &Db) {
        self.selected = 0;
        if self.input.trim().is_empty() {
            self.results.clear();
            self.error = None;
            return;
        }
        match search::search(db, db.translation(), &self.input, 50) {
            Ok(rows) => {
                self.results = rows;
                self.error = None;
            }
            Err(e) => {
                self.results.clear();
                self.error = Some(format!("{e}"));
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the body lays out the whole dialog inline (input row, error \
                  row, hit rows with two-line cells, empty-state, footer); \
                  decomposing would force callers to assemble the dialog \
                  themselves with no gain."
    )]
    pub fn render(&self, outer: Rect, buf: &mut Buffer, books: &[Book]) {
        let w: u16 = outer.width.saturating_sub(6).min(90);
        let h: u16 = outer.height.saturating_sub(4).min(22);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_modal_dialog(outer, area, "Find", buf);

        let bg = Style::new().bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let hit_style = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let ref_style = Style::new().fg(theme::cyan()).bg(theme::blue());
        let sel_bg = Style::new()
            .fg(theme::bright_white())
            .bg(theme::list_focus_bg())
            .add_modifier(Modifier::BOLD);

        let blank = || Line::from(Span::styled(" ".repeat(inner.width as usize), bg));

        let mut lines: Vec<Line<'static>> = Vec::with_capacity(inner.height as usize);
        lines.push(blank());
        // Shared sunken input field — frames/pads/cursors identically to Goto.
        let find_label = "  Find: ";
        // 2-cell inset before the inner right border — same rule as Goto so
        // both input fields end at the same margin.
        let field_w = u16::try_from(
            (inner.width as usize)
                .saturating_sub(find_label.chars().count())
                .saturating_sub(2),
        )
        .unwrap_or(0);
        let mut find_line = vec![Span::styled(find_label, label)];
        find_line.extend(dialog::input_field(&self.input, "", field_w));
        lines.push(Line::from(find_line));
        // Empty-state hint under the input — only shown before the user types.
        // Matches the Goto dialog's pattern so the two commands feel symmetric.
        if self.input.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "\u{2192} (type to search, e.g. \"love\", \"kingdom of God\")",
                    Style::new()
                        .fg(theme::yellow())
                        .bg(theme::blue())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(blank());
        }
        lines.push(blank());

        if let Some(err) = &self.error {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    err.clone(),
                    Style::new()
                        .fg(theme::hotkey_red())
                        .bg(theme::blue())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        // Lay out as many hits as fit. Each hit is a reference row + a snippet
        // wrapped to at most two lines (match ranges preserved) + a separator
        // blank. Snippet height varies, so budget rows dynamically and stop
        // before the footer rather than letting ratatui hard-clip mid-word.
        let snippet_w = (inner.width as usize).saturating_sub(SNIPPET_INDENT);
        let footer_reserve = 2; // trailing blank + footer row
        let budget = (inner.height as usize)
            .saturating_sub(lines.len())
            .saturating_sub(footer_reserve);
        let mut used_rows = 0usize;
        for (i, hit) in self.results.iter().enumerate() {
            let on = i == self.selected;
            // On a selected row the whole slab is one colour, so match and
            // plain both render as sel_bg; otherwise matches get the hit accent.
            let (plain, matched) = if on {
                (sel_bg, sel_bg)
            } else {
                (label, hit_style)
            };
            let snippet = wrap_snippet(
                &hit.text,
                &hit.hits,
                snippet_w,
                SNIPPET_MAX_LINES,
                plain,
                matched,
            );
            let cost = 1 + snippet.len() + 1; // reference + snippet lines + blank
            if used_rows + cost > budget {
                break;
            }
            used_rows += cost;

            let book_label = books
                .iter()
                .find(|b| b.code == hit.book)
                .map_or_else(|| hit.book.clone(), |b| b.abbreviation.clone());
            let reference = format!(
                " {} ",
                crate::reference::format(&book_label, hit.chapter, hit.verse, &self.translation)
            );
            let ref_line_style = if on { sel_bg } else { ref_style };

            // Reference row, full-width selectable slab.
            let used = 1 + reference.chars().count();
            let pad = (inner.width as usize).saturating_sub(used);
            lines.push(Line::from(vec![
                Span::styled(" ".to_string(), if on { sel_bg } else { bg }),
                Span::styled(reference, ref_line_style),
                Span::styled(" ".repeat(pad), if on { sel_bg } else { bg }),
            ]));

            // Snippet rows: indented, match-highlighted, wrapped.
            for row in snippet {
                let mut spans: Vec<Span<'static>> = vec![Span::styled(
                    " ".repeat(SNIPPET_INDENT),
                    if on { sel_bg } else { bg },
                )];
                spans.extend(row);
                if on {
                    // Extend the selection slab to the full pane width.
                    let w: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    let pad = (inner.width as usize).saturating_sub(w);
                    if pad > 0 {
                        spans.push(Span::styled(" ".repeat(pad), sel_bg));
                    }
                }
                lines.push(Line::from(spans));
            }

            lines.push(blank());
        }

        if self.results.is_empty() && self.error.is_none() && !self.input.trim().is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  ", bg),
                Span::styled(
                    "(no matches)",
                    Style::new()
                        .fg(theme::light_grey())
                        .bg(theme::blue())
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        // Footer
        while lines.len() < (inner.height as usize).saturating_sub(2) {
            lines.push(blank());
        }
        lines.push(Line::from(vec![
            Span::styled(
                "  Enter ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "jump   ",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
            Span::styled(
                "↑↓ ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "navigate   ",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
            Span::styled(
                "Esc ",
                Style::new()
                    .fg(theme::bright_white())
                    .bg(theme::blue())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "cancel",
                Style::new().fg(theme::light_grey()).bg(theme::blue()),
            ),
        ]));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}

/// Word-wrap a search snippet to `width` columns across at most `max_lines`
/// lines, preserving the highlighted match ranges (`hits` are byte offsets
/// into `text`). Appends `…` when the verse is longer than the budget, never
/// chopping mid-word. Returns one span list per line (match runs styled
/// `matched`, the rest `plain`); the caller prepends the indent.
fn wrap_snippet(
    text: &str,
    hits: &[(usize, usize)],
    width: usize,
    max_lines: usize,
    plain: Style,
    matched: Style,
) -> Vec<Vec<Span<'static>>> {
    let width = width.max(8);
    let is_match = |byte: usize| hits.iter().any(|&(s, e)| byte >= s && byte < e);

    // Greedy word-wrap into rows of (char, is_match) cells.
    let mut rows: Vec<Vec<(char, bool)>> = Vec::new();
    let mut cur: Vec<(char, bool)> = Vec::new();
    for (byte, raw) in text.char_indices() {
        let ch = if raw == '\n' { ' ' } else { raw };
        if ch == ' ' && cur.len() >= width {
            rows.push(std::mem::take(&mut cur)); // the break absorbs the space
            continue;
        }
        cur.push((ch, is_match(byte)));
        if cur.len() > width {
            // Overran on a long word: back up to the last space if there is one.
            if let Some(sp) = cur.iter().rposition(|&(c, _)| c == ' ') {
                let mut rest = cur.split_off(sp);
                rest.remove(0); // drop the breaking space
                rows.push(std::mem::take(&mut cur));
                cur = rest;
            } else {
                let rest = cur.split_off(width);
                rows.push(std::mem::take(&mut cur));
                cur = rest;
            }
        }
    }
    if !cur.is_empty() {
        rows.push(cur);
    }

    // Clamp to max_lines, appending '…' to the last shown row when truncated.
    let truncated = rows.len() > max_lines.max(1);
    rows.truncate(max_lines.max(1));
    if truncated && let Some(last) = rows.last_mut() {
        while last.len() + 1 > width {
            last.pop();
        }
        while matches!(last.last(), Some(&(' ', _))) {
            last.pop();
        }
        last.push(('\u{2026}', false));
    }

    // Coalesce each row's cells into styled spans by match-run.
    rows.into_iter()
        .map(|row| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut run_matched = false;
            for (ch, m) in row {
                if !run.is_empty() && m != run_matched {
                    let style = if run_matched { matched } else { plain };
                    spans.push(Span::styled(std::mem::take(&mut run), style));
                }
                if run.is_empty() {
                    run_matched = m;
                }
                run.push(ch);
            }
            if !run.is_empty() {
                let style = if run_matched { matched } else { plain };
                spans.push(Span::styled(run, style));
            }
            spans
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[Vec<Span<'static>>]) -> Vec<String> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn short_snippet_fits_on_one_line_without_ellipsis() {
        let plain = Style::new();
        let rows = wrap_snippet("God is love", &[(7, 11)], 40, 2, plain, plain);
        assert_eq!(texts(&rows), vec!["God is love"]);
    }

    #[test]
    fn long_snippet_wraps_then_truncates_with_ellipsis_on_word_boundary() {
        let plain = Style::new();
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let rows = wrap_snippet(text, &[], 12, 2, plain, plain);
        assert!(rows.len() <= 2, "must not exceed max_lines");
        let joined = texts(&rows).join("|");
        assert!(
            joined.ends_with('\u{2026}'),
            "truncation appends an ellipsis: {joined:?}"
        );
        // No line exceeds the width, and no word is chopped (every non-final
        // token is whole).
        for row in &rows {
            let w: usize = row.iter().map(|s| s.content.chars().count()).sum();
            assert!(w <= 12, "row width {w} exceeds 12");
        }
    }

    #[test]
    fn match_range_is_isolated_into_its_own_span() {
        let plain = Style::new().fg(theme::light_grey());
        let matched = Style::new().fg(theme::yellow());
        // "love" at bytes 4..8.
        let rows = wrap_snippet("God love you", &[(4, 8)], 40, 2, plain, matched);
        let row = &rows[0];
        let hit = row
            .iter()
            .find(|s| s.content == "love")
            .expect("the matched word should be its own span");
        assert_eq!(hit.style.fg, Some(theme::yellow()));
    }
}
