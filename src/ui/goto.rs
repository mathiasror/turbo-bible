//! Goto-reference dialog (F2 / `:`). Free-text book reference parser.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::Book;
use crate::nav::Position;
use crate::theme;
use crate::ui::dialog;

pub struct GotoDialog {
    pub input: String,
}

pub enum GotoOutcome {
    Continue,
    Cancel,
    Jump(Position),
    Command(GotoCommand),
}

pub enum GotoCommand {
    Quit,
    Help,
}

impl GotoDialog {
    pub fn new() -> Self {
        Self { input: String::new() }
    }

    pub fn handle(&mut self, key: KeyEvent, books: &[Book]) -> GotoOutcome {
        match key.code {
            KeyCode::Esc => GotoOutcome::Cancel,
            KeyCode::Enter => {
                // Vim-style commands take precedence over reference parsing.
                match parse_command(&self.input) {
                    Some(cmd) => GotoOutcome::Command(cmd),
                    None => match parse_reference(&self.input, books) {
                        Some(p) => GotoOutcome::Jump(p),
                        None => GotoOutcome::Continue,
                    },
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                GotoOutcome::Continue
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                GotoOutcome::Continue
            }
            _ => GotoOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer, books: &[Book]) {
        let w: u16 = 60;
        let h: u16 = 9;
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_dialog(area, "Goto reference", buf);

        let preview = parse_reference(&self.input, books)
            .map(|p| {
                let name = books
                    .iter()
                    .find(|b| b.code == p.book)
                    .map(|b| b.name.clone())
                    .unwrap_or(p.book.clone());
                format!("\u{2192} {} {}", name, p.chapter)
            })
            .unwrap_or_else(|| "\u{2192} (type a book and chapter)".into());

        let label = Span::styled(
            " Reference: ",
            Style::new().fg(theme::bright_white()).bg(theme::blue()),
        );
        let input_style = Style::new()
            .fg(theme::black())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let cursor_style = Style::new()
            .fg(theme::black())
            .bg(theme::bright_white())
            .add_modifier(Modifier::BOLD);

        // Empty-state placeholder shown inside the field — disappears the
        // moment the user starts typing. Faster to read than the hint below.
        let placeholder_style = Style::new()
            .fg(theme::dark_grey())
            .bg(theme::cyan());
        let typed_len = self.input.chars().count();
        let mut input_spans: Vec<Span<'static>> = Vec::new();
        if self.input.is_empty() {
            input_spans.push(Span::styled(" ".to_string(), input_style));
            input_spans.push(Span::styled("\u{2588}", cursor_style));
            input_spans.push(Span::styled("John 3:16".to_string(), placeholder_style));
        } else {
            input_spans.push(Span::styled(format!(" {}", self.input), input_style));
            input_spans.push(Span::styled("\u{2588}", cursor_style));
        }
        let placeholder_len = if self.input.is_empty() { 9 } else { 0 };
        let pad = (inner.width as usize)
            .saturating_sub(typed_len + 2 + 12 + placeholder_len);
        if pad > 0 {
            input_spans.push(Span::styled(" ".repeat(pad), input_style));
        }
        let blank = Span::styled(
            " ".repeat(inner.width as usize),
            Style::new().bg(theme::blue()),
        );

        let lines = vec![
            Line::from(blank.clone()),
            Line::from(vec![label, Span::raw("")]),
            Line::from({
                let mut v = vec![Span::styled(
                    "  ",
                    Style::new().bg(theme::blue()),
                )];
                v.extend(input_spans);
                v
            }),
            Line::from(blank.clone()),
            Line::from(vec![
                Span::styled("  ", Style::new().bg(theme::blue())),
                Span::styled(
                    preview,
                    Style::new()
                        .fg(theme::yellow())
                        .bg(theme::blue())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(blank.clone()),
            Line::from(vec![
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
            ]),
        ];

        Paragraph::new(lines)
            .style(Style::new().bg(theme::blue()))
            .render(inner, buf);
    }
}

/// Recognize `:q`, `:quit`, `:h`, `:help` (with or without leading colon).
pub fn parse_command(input: &str) -> Option<GotoCommand> {
    let s = input.trim().trim_start_matches(':').to_lowercase();
    match s.as_str() {
        "q" | "quit" | "exit" => Some(GotoCommand::Quit),
        "h" | "help" => Some(GotoCommand::Help),
        _ => None,
    }
}

/// Parse a free-text reference like "Mark 1:1", "MRK 1", "matt 5,3", "1 mos 1".
/// Books match case-insensitively against name, abbreviation, or OSIS code.
/// Norwegian convention uses `,` for chapter-verse separator; we accept `:`,
/// `,`, and `.` too.
pub fn parse_reference(input: &str, books: &[Book]) -> Option<Position> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    let mut best: Option<(usize, String)> = None;
    for b in books {
        let candidates = [b.name.to_lowercase(), b.abbreviation.to_lowercase(), b.code.to_lowercase()];
        for cand in &candidates {
            if cand.is_empty() {
                continue;
            }
            if s.starts_with(cand.as_str()) {
                // Require word boundary after the candidate (digit or space or end).
                let after = s[cand.len()..].chars().next();
                let ok = matches!(after, None | Some(' ') | Some('\t'))
                    || after.map_or(false, |c| c.is_ascii_digit());
                if ok {
                    let len = cand.len();
                    if best.as_ref().map_or(true, |(n, _)| len > *n) {
                        best = Some((len, b.code.clone()));
                    }
                }
            }
        }
    }

    let (n, code) = best?;
    let rest = s[n..].trim();
    if rest.is_empty() {
        return Some(Position { book: code, chapter: 1, verse: None });
    }
    let (chap_str, verse_str) = match rest.find(|c: char| c == ':' || c == ',' || c == '.') {
        Some(i) => (rest[..i].trim(), rest[i + 1..].trim()),
        None => (rest, ""),
    };
    let chapter: i64 = chap_str.parse().ok()?;
    if chapter < 1 {
        return None;
    }
    let verse: Option<i64> = if verse_str.is_empty() {
        None
    } else {
        verse_str.parse().ok().filter(|v: &i64| *v >= 1)
    };
    Some(Position { book: code, chapter, verse })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn books() -> Vec<Book> {
        vec![
            Book { code: "GEN".into(), name: "Første Mosebok".into(), abbreviation: "1 Mos".into(), testament: "OT".into(), ord: 1, full_name: None },
            Book { code: "MRK".into(), name: "Markus".into(), abbreviation: "Mark".into(), testament: "NT".into(), ord: 41, full_name: None },
            Book { code: "MAT".into(), name: "Matteus".into(), abbreviation: "Matt".into(), testament: "NT".into(), ord: 40, full_name: None },
            Book { code: "JHN".into(), name: "Johannes".into(), abbreviation: "Joh".into(), testament: "NT".into(), ord: 43, full_name: None },
        ]
    }

    #[test]
    fn parses_osis() {
        let p = parse_reference("MRK 1", &books()).unwrap();
        assert_eq!(p.book, "MRK");
        assert_eq!(p.chapter, 1);
    }

    #[test]
    fn parses_abbreviation_with_space() {
        let p = parse_reference("1 Mos 5", &books()).unwrap();
        assert_eq!(p.book, "GEN");
        assert_eq!(p.chapter, 5);
    }

    #[test]
    fn parses_full_name_lowercase() {
        let p = parse_reference("markus 3:14", &books()).unwrap();
        assert_eq!(p.book, "MRK");
        assert_eq!(p.chapter, 3);
    }

    #[test]
    fn picks_longest_match() {
        // "matt" must beat shorter prefixes
        let p = parse_reference("matt 5", &books()).unwrap();
        assert_eq!(p.book, "MAT");
    }

    #[test]
    fn handles_norwegian_comma() {
        let p = parse_reference("joh 3,16", &books()).unwrap();
        assert_eq!(p.book, "JHN");
        assert_eq!(p.chapter, 3);
    }

    #[test]
    fn rejects_unknown() {
        assert!(parse_reference("xyzzy 1", &books()).is_none());
    }
}
