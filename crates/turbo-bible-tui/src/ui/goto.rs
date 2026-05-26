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
    input: String,
    /// True while the field still holds an unmodified pre-fill (set by
    /// [`with_position`]). The first character typed replaces the whole
    /// pre-fill, mirroring the "selected text" behavior of GUI forms:
    /// `Enter`-without-edits stays put, typing fresh starts a new
    /// reference, and Backspace begins editing the pre-fill in place.
    prefilled: bool,
    /// Active translation code — drives the locale reference separator in the
    /// pre-fill and the live preview.
    translation: String,
}

#[non_exhaustive]
pub enum GotoOutcome {
    Continue,
    Cancel,
    Jump(Position),
    Command(GotoCommand),
}

#[non_exhaustive]
pub enum GotoCommand {
    Quit,
    Help,
}

impl GotoDialog {
    pub fn new(translation: &str) -> Self {
        Self {
            input: String::new(),
            prefilled: false,
            translation: translation.to_string(),
        }
    }

    /// Open with the field pre-populated to the current reference. Lets
    /// `Enter` act as "stay here" and turns "edit the chapter/verse" into
    /// a few keystrokes rather than typing the whole reference from scratch.
    pub fn with_position(book_name: &str, chapter: i64, verse: i64, translation: &str) -> Self {
        Self {
            input: crate::reference::format(book_name, chapter, verse, translation),
            prefilled: true,
            translation: translation.to_string(),
        }
    }

    pub fn handle(&mut self, key: KeyEvent, books: &[Book]) -> GotoOutcome {
        match key.code {
            KeyCode::Esc => GotoOutcome::Cancel,
            KeyCode::Enter => {
                // Vim-style commands take precedence over reference parsing.
                parse_command(&self.input).map_or_else(
                    || {
                        parse_reference(&self.input, books)
                            .map_or(GotoOutcome::Continue, GotoOutcome::Jump)
                    },
                    GotoOutcome::Command,
                )
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.prefilled = false;
                GotoOutcome::Continue
            }
            KeyCode::Char(c) => {
                if self.prefilled {
                    self.input.clear();
                    self.prefilled = false;
                }
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
        let inner = dialog::draw_modal_dialog(outer, area, "Goto reference", buf);

        // Preview must match exactly what Enter resolves to: Enter jumps to the
        // verse when one is typed (and lands the cursor there), so show it —
        // with the locale separator — behind an explicit "Enter opens:" label.
        let preview = parse_reference(&self.input, books).map_or_else(
            || "(type a book and chapter)".to_string(),
            |p| {
                let name = books
                    .iter()
                    .find(|b| b.code == p.book)
                    .map_or_else(|| p.book.clone(), |b| b.name.clone());
                let target = match p.verse {
                    Some(v) => crate::reference::format(&name, p.chapter, v, &self.translation),
                    None => format!("{name} {}", p.chapter),
                };
                format!("Enter opens: {target}")
            },
        );

        let blank = Span::styled(
            " ".repeat(inner.width as usize),
            Style::new().bg(theme::blue()),
        );
        // Shared sunken input field — frames/pads/cursors identically to Find.
        // The "John 3:16" placeholder shows inside the field while it's empty.
        let label = Span::styled(
            "  Reference: ",
            Style::new().fg(theme::bright_white()).bg(theme::blue()),
        );
        let field_w =
            u16::try_from((inner.width as usize).saturating_sub(label.content.chars().count()))
                .unwrap_or(0);
        let mut input_line = vec![label];
        input_line.extend(dialog::input_field(&self.input, "John 3:16", field_w));

        let lines = vec![
            Line::from(blank.clone()),
            Line::from(input_line),
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
        let candidates = [
            b.name.to_lowercase(),
            b.abbreviation.to_lowercase(),
            b.code.to_lowercase(),
        ];
        for cand in &candidates {
            if cand.is_empty() {
                continue;
            }
            if s.starts_with(cand.as_str()) {
                // Require word boundary after the candidate (digit or space or end).
                let after = s[cand.len()..].chars().next();
                let ok = matches!(after, None | Some(' ' | '\t'))
                    || after.is_some_and(|c| c.is_ascii_digit());
                if ok {
                    let len = cand.len();
                    if best.as_ref().is_none_or(|(n, _)| len > *n) {
                        best = Some((len, b.code.clone()));
                    }
                }
            }
        }
    }

    let (n, code) = best?;
    let rest = s[n..].trim();
    if rest.is_empty() {
        return Some(Position {
            book: code,
            chapter: 1,
            verse: None,
        });
    }
    let (chap_str, verse_str) = rest
        .find([':', ',', '.'])
        .map_or((rest, ""), |i| (rest[..i].trim(), rest[i + 1..].trim()));
    let chapter: i64 = chap_str.parse().ok()?;
    if chapter < 1 {
        return None;
    }
    let verse: Option<i64> = if verse_str.is_empty() {
        None
    } else {
        verse_str.parse().ok().filter(|v: &i64| *v >= 1)
    };
    Some(Position {
        book: code,
        chapter,
        verse,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn books() -> Vec<Book> {
        vec![
            Book {
                code: "GEN".into(),
                name: "Første Mosebok".into(),
                abbreviation: "1 Mos".into(),
                testament: "OT".into(),
                ord: 1,
                full_name: None,
            },
            Book {
                code: "MRK".into(),
                name: "Markus".into(),
                abbreviation: "Mark".into(),
                testament: "NT".into(),
                ord: 41,
                full_name: None,
            },
            Book {
                code: "MAT".into(),
                name: "Matteus".into(),
                abbreviation: "Matt".into(),
                testament: "NT".into(),
                ord: 40,
                full_name: None,
            },
            Book {
                code: "JHN".into(),
                name: "Johannes".into(),
                abbreviation: "Joh".into(),
                testament: "NT".into(),
                ord: 43,
                full_name: None,
            },
        ]
    }

    #[test]
    fn with_position_prefills_input_and_parses_back() {
        // `Enter`-without-edits has to be a no-op jump: the pre-filled
        // string must parse back into the same book/chapter/verse it was
        // built from. This pins the round-trip so future formatting tweaks
        // can't silently break the "stay here" interaction.
        let d = GotoDialog::with_position("Matteus", 5, 3, "en-kjv");
        assert_eq!(d.input, "Matteus 5:3");
        let p = parse_reference(&d.input, &books()).expect("must parse back");
        assert_eq!(p.book, "MAT");
        assert_eq!(p.chapter, 5);
        assert_eq!(p.verse, Some(3));
    }

    #[test]
    fn first_keystroke_replaces_prefilled_input() {
        // Pre-fill is conceptually "selected" — typing a fresh reference
        // must replace it in one keystroke. Without this, the e2e
        // ":John 3:16" flow would land on the wrong book because the
        // pre-fill would be silently appended to.
        let mut d = GotoDialog::with_position("Matteus", 5, 3, "en-kjv");
        let bs = books();
        d.handle(
            KeyEvent::new(KeyCode::Char('J'), crossterm::event::KeyModifiers::NONE),
            &bs,
        );
        // After the first char, the pre-fill is gone and only the new
        // character remains. Subsequent chars append normally.
        assert_eq!(d.input, "J");
        d.handle(
            KeyEvent::new(KeyCode::Char('H'), crossterm::event::KeyModifiers::NONE),
            &bs,
        );
        assert_eq!(d.input, "JH");
    }

    #[test]
    fn backspace_edits_prefilled_input_in_place() {
        // Backspace from a pre-fill should chip away at it (not wipe
        // wholesale), so a "bump the verse" flow is just a few keystrokes.
        let mut d = GotoDialog::with_position("Matteus", 5, 3, "en-kjv");
        let bs = books();
        d.handle(
            KeyEvent::new(KeyCode::Backspace, crossterm::event::KeyModifiers::NONE),
            &bs,
        );
        assert_eq!(d.input, "Matteus 5:");
        // And typing now appends rather than replacing.
        d.handle(
            KeyEvent::new(KeyCode::Char('7'), crossterm::event::KeyModifiers::NONE),
            &bs,
        );
        assert_eq!(d.input, "Matteus 5:7");
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

    // ----- Ambiguity policy -----
    //
    // parse_reference picks the LONGEST prefix match across each book's
    // (name, abbreviation, code) tuple. These tests pin the resulting
    // behavior for inputs that are deliberately ambiguous.

    /// Mixed-translation corpus: English KJV book overlaps with Norwegian
    /// Bokmål book whose name is a prefix of the KJV name. Tests where the
    /// match should land per the longest-match rule.
    fn multi_lang_books() -> Vec<Book> {
        vec![
            Book {
                code: "JHN".into(),
                name: "John".into(),
                abbreviation: "Jn".into(),
                testament: "NT".into(),
                ord: 43,
                full_name: None,
            },
            // "Johannes" contains "John" as a strict prefix.
            Book {
                code: "JHN_NB".into(),
                name: "Johannes".into(),
                abbreviation: "Joh".into(),
                testament: "NT".into(),
                ord: 99,
                full_name: None,
            },
            // "1 John" and "1 Johannes" — number-leading book names.
            Book {
                code: "1JN".into(),
                name: "1 John".into(),
                abbreviation: "1 Jn".into(),
                testament: "NT".into(),
                ord: 62,
                full_name: None,
            },
        ]
    }

    #[test]
    fn ambiguity_typing_johannes_beats_john() {
        // "Johannes" (8 chars) is longer than "John" (4 chars), so when both
        // are valid prefixes of the input "johannes 1:1", the longer wins.
        let p = parse_reference("johannes 1:1", &multi_lang_books()).unwrap();
        assert_eq!(p.book, "JHN_NB");
    }

    #[test]
    fn ambiguity_typing_john_matches_john_not_johannes() {
        // Word-boundary check: "john 1" has a space after "john", so
        // "Johannes" can't match (no boundary at char index 4).
        let p = parse_reference("john 1", &multi_lang_books()).unwrap();
        assert_eq!(p.book, "JHN");
    }

    #[test]
    fn ambiguity_typing_jn_matches_short_abbreviation() {
        // "Jn" is the abbreviation; "Joh" is also a valid prefix of
        // "Johannes" (3 chars) — but "jn" (2 chars) doesn't match "Joh" at
        // all. So only "JHN" is in the running.
        let p = parse_reference("jn 1", &multi_lang_books()).unwrap();
        assert_eq!(p.book, "JHN");
    }

    #[test]
    fn ambiguity_number_prefixed_book_wins_over_unprefixed() {
        // "1 John 1" should match the "1 John" book (length 6), not the
        // shorter "John" (length 4 starting at offset 2).
        let p = parse_reference("1 John 1", &multi_lang_books()).unwrap();
        assert_eq!(p.book, "1JN");
    }

    #[test]
    fn rejects_chapter_zero_or_negative() {
        assert!(parse_reference("Mark 0", &books()).is_none());
        // The grammar matches `[+-]?\d+` so negative parses, but is filtered.
        assert!(parse_reference("Mark -1", &books()).is_none());
    }

    #[test]
    fn rejects_verse_zero() {
        // chapter is OK but verse < 1 gets filtered.
        let p = parse_reference("Mark 1:0", &books()).unwrap();
        assert_eq!(p.verse, None, "verse 0 must be dropped, not preserved");
    }

    #[test]
    fn three_separators_are_equivalent() {
        // ':' (English), ',' (Norwegian/Spanish), '.' all bind chapter:verse.
        for sep in [':', ',', '.'] {
            let s = format!("Mark 3{sep}14");
            let p = parse_reference(&s, &books())
                .unwrap_or_else(|| panic!("failed for separator {sep:?}"));
            assert_eq!(p.book, "MRK");
            assert_eq!(p.chapter, 3);
            assert_eq!(p.verse, Some(14));
        }
    }

    // ----- Property-based tests -----

    proptest::proptest! {
        /// Every book in the corpus round-trips through its OSIS code.
        #[test]
        fn roundtrip_via_code(
            idx in 0usize..4,
            chapter in 1i64..150,
        ) {
            let bs = books();
            let book = &bs[idx];
            let input = format!("{} {}", book.code, chapter);
            let p = parse_reference(&input, &bs).expect("roundtrip");
            proptest::prop_assert_eq!(p.book, book.code.clone());
            proptest::prop_assert_eq!(p.chapter, chapter);
        }

        /// Every book in the corpus round-trips through its full name.
        #[test]
        fn roundtrip_via_name(
            idx in 0usize..4,
            chapter in 1i64..150,
        ) {
            let bs = books();
            let book = &bs[idx];
            let input = format!("{} {}", book.name, chapter);
            let p = parse_reference(&input, &bs).expect("roundtrip");
            proptest::prop_assert_eq!(p.book, book.code.clone());
            proptest::prop_assert_eq!(p.chapter, chapter);
        }

        /// Every book in the corpus round-trips through its abbreviation.
        #[test]
        fn roundtrip_via_abbreviation(
            idx in 0usize..4,
            chapter in 1i64..150,
        ) {
            let bs = books();
            let book = &bs[idx];
            let input = format!("{} {}", book.abbreviation, chapter);
            let p = parse_reference(&input, &bs).expect("roundtrip");
            proptest::prop_assert_eq!(p.book, book.code.clone());
            proptest::prop_assert_eq!(p.chapter, chapter);
        }

        /// Random alphanumeric strings that don't start with any book prefix
        /// must return None — the parser should never accept garbage.
        #[test]
        fn rejects_random_non_matching_strings(
            junk in "[xyz]{1,8}",
            n in 1i64..100,
        ) {
            let input = format!("{junk} {n}");
            proptest::prop_assert!(parse_reference(&input, &books()).is_none());
        }

        /// Determinism: the parser is a pure function of (input, books).
        #[test]
        fn deterministic(
            idx in 0usize..4,
            chapter in 1i64..150,
        ) {
            let bs = books();
            let book = &bs[idx];
            let input = format!("{} {}", book.name, chapter);
            let p1 = parse_reference(&input, &bs);
            let p2 = parse_reference(&input, &bs);
            proptest::prop_assert_eq!(p1, p2);
        }
    }
}
