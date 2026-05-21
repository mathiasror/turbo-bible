//! Startup splash: title art, daily verse, and a two-column book picker
//! with testament headings localised off the active translation's language
//! prefix. Vim-style navigation.

// The OT/NT pairing is the file's whole subject — binding name pairs like
// `books_ot`/`books_nt` and `cursor_ot`/`cursor_nt` are intentional and
// renaming them to satisfy clippy would obscure intent.
#![allow(clippy::similar_names)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::db::Book;
use crate::nav::Position;
use crate::quote::DailyQuote;
use crate::text::word_wrap;
use crate::theme;
use crate::ui::dialog;
use crate::ui::listnav::{ListNav, Step};

const TITLE_TURBO: &[&str] = &[
    "████████╗██╗   ██╗██████╗ ██████╗  ██████╗ ",
    "╚══██╔══╝██║   ██║██╔══██╗██╔══██╗██╔═══██╗",
    "   ██║   ██║   ██║██████╔╝██████╔╝██║   ██║",
    "   ██║   ██║   ██║██╔══██╗██╔══██╗██║   ██║",
    "   ██║   ╚██████╔╝██║  ██║██████╔╝╚██████╔╝",
    "   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝  ╚═════╝ ",
];
const TITLE_BIBLE: &[&str] = &[
    "██████╗ ██╗██████╗ ██╗     ███████╗",
    "██╔══██╗██║██╔══██╗██║     ██╔════╝",
    "██████╔╝██║██████╔╝██║     █████╗  ",
    "██╔══██╗██║██╔══██╗██║     ██╔══╝  ",
    "██████╔╝██║██████╔╝███████╗███████╗",
    "╚═════╝ ╚═╝╚═════╝ ╚══════╝╚══════╝",
];
const TITLE_COMPACT: &str = "T U R B O   B I B L E";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SplashMode {
    Normal,
    Filter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SplashColumn {
    OT,
    NT,
}

pub struct SplashView {
    books_ot: Vec<Book>,
    books_nt: Vec<Book>,
    last: Option<(Position, String)>,
    pub filter: String,
    pub focus: SplashColumn,
    pub cursor_ot: usize,
    pub cursor_nt: usize,
    /// True when the cursor is on the "Continue" row above the columns.
    pub on_continue: bool,
    pub translation_name: String,
    pub translation_code: String,
    pub mode: SplashMode,
    pub quote: Option<DailyQuote>,
    /// Chord + count state for `gg`, `G`, `5j`, etc. Shared with the
    /// list dialogs so the third copy of this state machine doesn't
    /// have to live here. Splash-specific keys (Ctrl-D/U/F/B,
    /// PageUp/Down, Home/End, column-switch, `o`/Enter) bypass it.
    nav: ListNav,
}

pub enum SplashOutcome {
    Continue,
    OpenBook(Position),
    OpenGoto,
    OpenFind,
    OpenTranslations,
    Quit,
}

impl SplashView {
    pub fn new(
        books: Vec<Book>,
        last: Option<(Position, String)>,
        translation_name: String,
        translation_code: String,
        quote: Option<DailyQuote>,
    ) -> Self {
        let (books_ot, books_nt): (Vec<Book>, Vec<Book>) =
            books.into_iter().partition(|b| b.testament == "OT");
        let on_continue = last.is_some();
        Self {
            books_ot,
            books_nt,
            last,
            filter: String::new(),
            focus: SplashColumn::OT,
            cursor_ot: 0,
            cursor_nt: 0,
            on_continue,
            translation_name,
            translation_code,
            mode: SplashMode::Normal,
            quote,
            nav: ListNav::default(),
        }
    }

    fn matches(&self, b: &Book) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let f = self.filter.to_lowercase();
        let hay = format!(
            "{} {} {}",
            b.name.to_lowercase(),
            b.abbreviation.to_lowercase(),
            b.code.to_lowercase()
        );
        hay.contains(&f)
    }

    fn entries(&self, col: SplashColumn) -> Vec<&Book> {
        let src = match col {
            SplashColumn::OT => &self.books_ot,
            SplashColumn::NT => &self.books_nt,
        };
        src.iter().filter(|b| self.matches(b)).collect()
    }

    const fn current_cursor(&self) -> usize {
        match self.focus {
            SplashColumn::OT => self.cursor_ot,
            SplashColumn::NT => self.cursor_nt,
        }
    }

    const fn set_current_cursor(&mut self, value: usize) {
        match self.focus {
            SplashColumn::OT => self.cursor_ot = value,
            SplashColumn::NT => self.cursor_nt = value,
        }
    }

    fn current_max_idx(&self) -> usize {
        self.entries(self.focus).len().saturating_sub(1)
    }

    fn switch_focus(&mut self, to: SplashColumn) {
        self.focus = to;
        let max = self.current_max_idx();
        let cur = self.current_cursor();
        self.set_current_cursor(cur.min(max));
        self.on_continue = false;
    }

    pub fn handle(&mut self, key: KeyEvent) -> SplashOutcome {
        match self.mode {
            SplashMode::Filter => self.handle_filter(key),
            SplashMode::Normal => self.handle_normal(key),
        }
    }

    fn handle_filter(&mut self, key: KeyEvent) -> SplashOutcome {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.cursor_ot = 0;
                self.cursor_nt = 0;
                self.mode = SplashMode::Normal;
                SplashOutcome::Continue
            }
            KeyCode::Enter => {
                self.mode = SplashMode::Normal;
                SplashOutcome::Continue
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.cursor_ot = 0;
                self.cursor_nt = 0;
                SplashOutcome::Continue
            }
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' => {
                self.filter.clear();
                self.cursor_ot = 0;
                self.cursor_nt = 0;
                SplashOutcome::Continue
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.cursor_ot = 0;
                self.cursor_nt = 0;
                SplashOutcome::Continue
            }
            _ => SplashOutcome::Continue,
        }
    }

    fn open_current(&self) -> Option<Position> {
        if self.on_continue {
            return self.last.as_ref().map(|(p, _)| p.clone());
        }
        let entries = self.entries(self.focus);
        entries.get(self.current_cursor()).map(|b| Position {
            book: b.code.clone(),
            chapter: 1,
            verse: None,
        })
    }

    fn move_down(&mut self, step: usize) {
        if self.on_continue {
            self.on_continue = false;
            // Land at the top of the focused column.
            self.set_current_cursor(0);
            if step > 1 {
                let new = step - 1;
                let max = self.current_max_idx();
                self.set_current_cursor(new.min(max));
            }
            return;
        }
        let max = self.current_max_idx();
        let new = (self.current_cursor() + step).min(max);
        self.set_current_cursor(new);
    }

    const fn move_up(&mut self, step: usize) {
        if self.on_continue {
            return;
        }
        let cur = self.current_cursor();
        if step > cur && self.last.is_some() {
            // Stepped past the top → land on Continue.
            self.on_continue = true;
            self.set_current_cursor(0);
        } else {
            self.set_current_cursor(cur.saturating_sub(step));
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> SplashOutcome {
        // Digit + j/k/g/G go through ListNav so the chord+count state
        // lives in one place across this view and the list dialogs.
        // 'n'/'N' are splash aliases for j/k — route them in.
        let nav_key = match key.code {
            KeyCode::Char('n') => KeyEvent::new(KeyCode::Char('j'), key.modifiers),
            KeyCode::Char('N') => KeyEvent::new(KeyCode::Char('k'), key.modifiers),
            _ => key,
        };
        match self.nav.handle(nav_key) {
            Step::Down(n) => {
                self.move_down(n as usize);
                return SplashOutcome::Continue;
            }
            Step::Up(n) => {
                self.move_up(n as usize);
                return SplashOutcome::Continue;
            }
            Step::Top => {
                if self.last.is_some() {
                    self.on_continue = true;
                }
                self.set_current_cursor(0);
                return SplashOutcome::Continue;
            }
            Step::BottomOrAt(n) => {
                self.on_continue = false;
                let max = self.current_max_idx();
                let target = if n == 0 {
                    max
                } else {
                    (n as usize).saturating_sub(1).min(max)
                };
                self.set_current_cursor(target);
                return SplashOutcome::Continue;
            }
            Step::Pending => return SplashOutcome::Continue,
            Step::Pass => {}
        }

        // Splash-specific keys. ListNav's Pass arm already cleared any
        // pending chord/count, so each arm here is self-contained.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') if !ctrl => SplashOutcome::Quit,
            KeyCode::Char('c') if ctrl => SplashOutcome::Quit,
            KeyCode::F(2) | KeyCode::Char(':') => SplashOutcome::OpenGoto,
            KeyCode::F(3) => SplashOutcome::OpenFind,
            KeyCode::F(5) | KeyCode::Char('t') => SplashOutcome::OpenTranslations,
            KeyCode::Char('/') => {
                self.mode = SplashMode::Filter;
                self.filter.clear();
                self.cursor_ot = 0;
                self.cursor_nt = 0;
                SplashOutcome::Continue
            }
            KeyCode::Enter | KeyCode::Char('o') => self
                .open_current()
                .map_or(SplashOutcome::Continue, SplashOutcome::OpenBook),
            KeyCode::Char('h') | KeyCode::Left => {
                self.switch_focus(SplashColumn::OT);
                SplashOutcome::Continue
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.switch_focus(SplashColumn::NT);
                SplashOutcome::Continue
            }
            KeyCode::Tab => {
                let to = match self.focus {
                    SplashColumn::OT => SplashColumn::NT,
                    SplashColumn::NT => SplashColumn::OT,
                };
                self.switch_focus(to);
                SplashOutcome::Continue
            }
            KeyCode::Char('d') if ctrl => {
                self.move_down(10);
                SplashOutcome::Continue
            }
            KeyCode::Char('u') if ctrl => {
                self.move_up(10);
                SplashOutcome::Continue
            }
            KeyCode::Char('f') if ctrl => {
                self.move_down(20);
                SplashOutcome::Continue
            }
            KeyCode::Char('b') if ctrl => {
                self.move_up(20);
                SplashOutcome::Continue
            }
            KeyCode::PageDown => {
                self.move_down(20);
                SplashOutcome::Continue
            }
            KeyCode::PageUp => {
                self.move_up(20);
                SplashOutcome::Continue
            }
            KeyCode::Home => {
                if self.last.is_some() {
                    self.on_continue = true;
                }
                self.set_current_cursor(0);
                SplashOutcome::Continue
            }
            KeyCode::End => {
                self.on_continue = false;
                self.set_current_cursor(self.current_max_idx());
                SplashOutcome::Continue
            }
            _ => SplashOutcome::Continue,
        }
    }

    pub fn render(&self, outer: Rect, buf: &mut Buffer) {
        let w = outer.width.saturating_sub(6).min(110);
        let h = outer.height.saturating_sub(2);
        let area = dialog::center(outer, w, h);
        let inner = dialog::draw_dialog(area, "Turbo Bible", buf);

        let bg = Style::new().bg(theme::blue());
        let title_style = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let subtitle = Style::new()
            .fg(theme::cyan())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(theme::light_grey()).bg(theme::blue());
        let label = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let key_style = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let sel = Style::new()
            .fg(theme::bright_white())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let filter_style = Style::new()
            .fg(theme::black())
            .bg(theme::cyan())
            .add_modifier(Modifier::BOLD);
        let mode_style = match self.mode {
            SplashMode::Filter => Style::new()
                .fg(theme::black())
                .bg(theme::yellow())
                .add_modifier(Modifier::BOLD),
            SplashMode::Normal => Style::new()
                .fg(theme::black())
                .bg(theme::cyan())
                .add_modifier(Modifier::BOLD),
        };
        let column_header = Style::new()
            .fg(theme::yellow())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD);
        let column_header_focused = Style::new()
            .fg(theme::bright_white())
            .bg(theme::blue())
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        let inner_w = inner.width as usize;
        let blank = || Line::from(Span::styled(" ".repeat(inner_w), bg));
        let center_padded = |row: &str, st: Style| -> Line<'static> {
            let pad_left = inner_w.saturating_sub(row.chars().count()) / 2;
            let pad_right = inner_w
                .saturating_sub(pad_left)
                .saturating_sub(row.chars().count());
            Line::from(vec![
                Span::styled(" ".repeat(pad_left), bg),
                Span::styled(row.to_string(), st),
                Span::styled(" ".repeat(pad_right), bg),
            ])
        };

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(blank());

        // Title — prefer side-by-side ("TURBO  BIBLE" on one 6-row logo).
        // Fall back to stacked, then to plain text on narrow terminals.
        let avail = inner.height as usize;
        let combined_w = TITLE_TURBO[0].chars().count() + 2 + TITLE_BIBLE[0].chars().count();
        if inner_w >= combined_w && avail >= 12 {
            for (t, b) in TITLE_TURBO.iter().zip(TITLE_BIBLE.iter()) {
                lines.push(center_padded(&format!("{t}  {b}"), title_style));
            }
        } else if inner_w >= TITLE_TURBO[0].chars().count() && avail >= 22 {
            for row in TITLE_TURBO.iter().chain(TITLE_BIBLE.iter()) {
                lines.push(center_padded(row, title_style));
            }
        } else {
            lines.push(center_padded(TITLE_COMPACT, title_style));
        }
        lines.push(blank());
        lines.push(center_padded(
            &format!("· {} ·", self.translation_name),
            subtitle,
        ));

        // Daily verse — word-wrapped to fit the column, then a reference
        // line. Not truncated; uses as many lines as it needs.
        if let Some(q) = &self.quote {
            lines.push(blank());
            let max_width = inner_w.saturating_sub(8).max(20);
            // Wrap the body so it renders as one block; the open and close
            // curly quotes hug the first/last words.
            let mut body_lines = word_wrap(&q.text, max_width);
            if let Some(first) = body_lines.first_mut() {
                *first = format!("\u{201C}{first}");
            }
            if let Some(last) = body_lines.last_mut() {
                *last = format!("{last}\u{201D}");
            }
            for body_line in &body_lines {
                lines.push(center_padded(body_line, label));
            }
            lines.push(center_padded(&format!("\u{2014} {}", q.reference), dim));
        }

        // Filter row
        lines.push(blank());
        let mode_label = match self.mode {
            SplashMode::Normal => " NORMAL ",
            SplashMode::Filter => " FILTER ",
        };
        let filter_display = if self.filter.is_empty() {
            match self.mode {
                SplashMode::Filter => " (type to filter) ".to_string(),
                SplashMode::Normal => " /  to filter ".to_string(),
            }
        } else {
            format!(" {} ", self.filter)
        };
        let mut filter_row = vec![
            Span::styled("  ", bg),
            Span::styled(mode_label, mode_style),
            Span::styled("  ", bg),
            Span::styled(filter_display.clone(), filter_style),
        ];
        let used_filter: usize =
            2 + mode_label.chars().count() + 2 + filter_display.chars().count();
        let cursor_extra = if self.mode == SplashMode::Filter {
            filter_row.push(Span::styled(
                "\u{2588}",
                filter_style.fg(theme::bright_white()),
            ));
            1
        } else {
            0
        };
        if (used_filter + cursor_extra) < inner_w {
            filter_row.push(Span::styled(
                " ".repeat(inner_w - used_filter - cursor_extra),
                bg,
            ));
        }
        lines.push(Line::from(filter_row));
        lines.push(blank());

        // Continue line — full width, highlighted if on_continue.
        if let Some((_p, label_str)) = &self.last {
            let on = self.on_continue;
            let row_style = if on { sel } else { label };
            let mark = if on { "  \u{25B8} " } else { "    " };
            let content = format!("Continue: {label_str}");
            let used = mark.chars().count() + content.chars().count();
            let pad = inner_w.saturating_sub(used);
            lines.push(Line::from(vec![
                Span::styled(mark, if on { sel } else { dim }),
                Span::styled(content, row_style),
                Span::styled(" ".repeat(pad), if on { sel } else { bg }),
            ]));
            lines.push(blank());
        }

        // Column headers
        let entries_ot = self.entries(SplashColumn::OT);
        let entries_nt = self.entries(SplashColumn::NT);
        let total_count = entries_ot.len() + entries_nt.len();

        let (col_left, col_right, gap) = split_columns(inner_w);
        let (ot_label, nt_label) = testament_labels(&self.translation_code);
        let ot_header = format!(" {}  ({}) ", ot_label, entries_ot.len());
        let nt_header = format!(" {}  ({}) ", nt_label, entries_nt.len());
        let ot_header_style = if self.focus == SplashColumn::OT && !self.on_continue {
            column_header_focused
        } else {
            column_header
        };
        let nt_header_style = if self.focus == SplashColumn::NT && !self.on_continue {
            column_header_focused
        } else {
            column_header
        };
        lines.push(Line::from(vec![
            Span::styled(left_padded(&ot_header, col_left), ot_header_style),
            Span::styled(" ".repeat(gap), bg),
            Span::styled(left_padded(&nt_header, col_right), nt_header_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("─".repeat(col_left), dim),
            Span::styled(" ".repeat(gap), bg),
            Span::styled("─".repeat(col_right), dim),
        ]));

        // Entries: side-by-side.
        let header_len = lines.len();
        let visible_rows = (inner.height as usize)
            .saturating_sub(header_len)
            .saturating_sub(1); // footer

        let scroll_ot = scroll_for(self.cursor_ot, entries_ot.len(), visible_rows);
        let scroll_nt = scroll_for(self.cursor_nt, entries_nt.len(), visible_rows);

        let entry_styles = EntryStyles {
            sel,
            label,
            dim,
            bg,
        };
        for row in 0..visible_rows {
            let i_ot = scroll_ot + row;
            let i_nt = scroll_nt + row;
            let left = render_entry_cell(
                entries_ot.get(i_ot).copied(),
                i_ot,
                self.cursor_ot,
                self.focus == SplashColumn::OT && !self.on_continue,
                col_left,
                &entry_styles,
            );
            let right = render_entry_cell(
                entries_nt.get(i_nt).copied(),
                i_nt,
                self.cursor_nt,
                self.focus == SplashColumn::NT && !self.on_continue,
                col_right,
                &entry_styles,
            );
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.extend(left);
            spans.push(Span::styled(" ".repeat(gap), bg));
            spans.extend(right);
            lines.push(Line::from(spans));
        }

        // Footer hint.
        let count_text = if self.on_continue {
            "Continue".to_string()
        } else {
            let entries_focused = match self.focus {
                SplashColumn::OT => &entries_ot,
                SplashColumn::NT => &entries_nt,
            };
            let len = entries_focused.len();
            if len == 0 {
                format!("0/0 ({total_count} total)")
            } else {
                format!(
                    "{}/{} ({} total)",
                    self.current_cursor() + 1,
                    len,
                    total_count
                )
            }
        };
        // The in-dialog footer carries only what's unique to this dialog —
        // splash-local motions and the live cursor/total readout. Global
        // shortcuts (Enter / F2 / F3 / Esc) live in the bottom status bar so
        // we don't show them twice.
        let footer = match self.mode {
            SplashMode::Normal => vec![
                Span::styled("  ", bg),
                Span::styled("j k ", key_style),
                Span::styled("move  ", dim),
                Span::styled("h l Tab ", key_style),
                Span::styled("column  ", dim),
                Span::styled("gg G ", key_style),
                Span::styled("ends  ", dim),
                Span::styled("/ ", key_style),
                Span::styled("filter  ", dim),
                Span::styled("t ", key_style),
                Span::styled("translation   ", dim),
                Span::styled(count_text, key_style),
            ],
            SplashMode::Filter => vec![
                Span::styled("  ", bg),
                Span::styled("type ", key_style),
                Span::styled("to filter  ", dim),
                Span::styled("Enter ", key_style),
                Span::styled("done  ", dim),
                Span::styled("Esc ", key_style),
                Span::styled("clear  ", dim),
                Span::styled("Ctrl-U ", key_style),
                Span::styled("wipe   ", dim),
                Span::styled(count_text, key_style),
            ],
        };
        lines.push(Line::from(footer));

        Paragraph::new(lines).style(bg).render(inner, buf);
    }
}

const fn split_columns(inner_w: usize) -> (usize, usize, usize) {
    // gap of 4 between columns; split remainder roughly evenly.
    let gap = 4;
    let usable = inner_w.saturating_sub(gap);
    let left = usable / 2;
    let right = usable - left;
    (left, right, gap)
}

fn scroll_for(cursor: usize, total: usize, visible: usize) -> usize {
    if visible == 0 || total <= visible {
        return 0;
    }
    let max_top = total - visible;
    if cursor < visible {
        0
    } else {
        ((cursor + 1) - visible).min(max_top)
    }
}

fn left_padded(s: &str, width: usize) -> String {
    let used = s.chars().count();
    if used >= width {
        s.chars().take(width).collect()
    } else {
        format!("{}{}", s, " ".repeat(width - used))
    }
}

/// Style bundle for [`render_entry_cell`]. Grouped so the entry-cell
/// signature stays manageable; all four styles are derived once per
/// render pass and shared across every cell.
struct EntryStyles {
    sel: Style,
    label: Style,
    dim: Style,
    bg: Style,
}

fn render_entry_cell(
    book: Option<&Book>,
    idx: usize,
    cursor_idx: usize,
    column_has_focus: bool,
    width: usize,
    styles: &EntryStyles,
) -> Vec<Span<'static>> {
    let EntryStyles {
        sel,
        label,
        dim,
        bg,
    } = *styles;
    let Some(b) = book else {
        return vec![Span::styled(" ".repeat(width), bg)];
    };
    // Only render the cursor on the column that currently has focus. The
    // unfocused column remembers its position internally, but nothing visible
    // hints at it — avoids the "ghost cursor" effect.
    let is_cursor = idx == cursor_idx && column_has_focus;

    let row_style = if is_cursor { sel } else { label };
    let mark_style = if is_cursor { sel } else { dim };
    let detail_style = if is_cursor { sel } else { dim };

    let mark = if is_cursor { "\u{25B8} " } else { "  " };
    let mark_w = mark.chars().count();
    let abbr_w = 8usize.min(width.saturating_sub(mark_w));
    let abbr_padded = format!(
        "{:<w$}",
        truncate(&b.abbreviation, abbr_w.saturating_sub(1)),
        w = abbr_w
    );

    let name_w = width
        .saturating_sub(mark_w)
        .saturating_sub(abbr_padded.chars().count());
    let name_field = truncate(b.display_name(), name_w);
    let name_padded = format!("{name_field:<name_w$}");

    let used = mark_w + name_padded.chars().count() + abbr_padded.chars().count();
    let pad_right = width.saturating_sub(used);

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(mark.to_string(), mark_style));
    spans.push(Span::styled(name_padded, row_style));
    spans.push(Span::styled(abbr_padded, detail_style));
    if pad_right > 0 {
        spans.push(Span::styled(
            " ".repeat(pad_right),
            if is_cursor { row_style } else { bg },
        ));
    }
    spans
}

/// Localise the splash testament headings off the active translation's
/// language prefix (`en-kjv` → English, `nb-1930` → Norwegian Bokmål, ...).
/// Unknown languages fall back to English so the labels are always intelligible
/// alongside the (also-English) Book names which serve as a baseline.
fn testament_labels(code: &str) -> (&'static str, &'static str) {
    let lang = code.split('-').next().unwrap_or("");
    match lang {
        "es" => ("Antiguo Testamento", "Nuevo Testamento"),
        "nb" | "nn" | "no" | "da" | "sv" => ("Det gamle testamentet", "Det nye testamentet"),
        "de" => ("Altes Testament", "Neues Testament"),
        "fr" => ("Ancien Testament", "Nouveau Testament"),
        _ => ("Old Testament", "New Testament"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    fn fake_books(n_ot: usize, n_nt: usize) -> Vec<Book> {
        let mut out = Vec::new();
        for i in 0..n_ot {
            out.push(Book {
                code: format!("O{i:02}"),
                name: format!("OT Book {i}"),
                abbreviation: format!("OT{i}"),
                testament: "OT".into(),
                ord: i64::try_from(i).expect("test ord fits i64"),
                full_name: None,
            });
        }
        for i in 0..n_nt {
            out.push(Book {
                code: format!("N{i:02}"),
                name: format!("NT Book {i}"),
                abbreviation: format!("NT{i}"),
                testament: "NT".into(),
                ord: i64::try_from(n_ot + i).expect("test ord fits i64"),
                full_name: None,
            });
        }
        out
    }

    fn find_cursor_row(buf: &Buffer) -> Option<u16> {
        let cyan = Color::Rgb(0, 170, 170);
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].bg == cyan && buf[(x, y)].fg == Color::Rgb(255, 255, 255) {
                    return Some(y);
                }
            }
        }
        None
    }

    #[test]
    fn cursor_visible_in_ot_column() {
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        for target in [0usize, 5, 20, 38] {
            splash.focus = SplashColumn::OT;
            splash.cursor_ot = target;
            splash.on_continue = false;
            let area = Rect::new(0, 0, 110, 36);
            let mut buf = Buffer::empty(area);
            splash.render(area, &mut buf);
            assert!(
                find_cursor_row(&buf).is_some(),
                "OT cursor invisible at {target}"
            );
        }
    }

    #[test]
    fn cursor_visible_in_nt_column() {
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        for target in [0usize, 5, 15, 26] {
            splash.focus = SplashColumn::NT;
            splash.cursor_nt = target;
            splash.on_continue = false;
            let area = Rect::new(0, 0, 110, 36);
            let mut buf = Buffer::empty(area);
            splash.render(area, &mut buf);
            assert!(
                find_cursor_row(&buf).is_some(),
                "NT cursor invisible at {target}"
            );
        }
    }

    #[test]
    fn title_renders_side_by_side_when_wide_enough() {
        let splash = SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        let area = Rect::new(0, 0, 110, 30);
        let mut buf = Buffer::empty(area);
        splash.render(area, &mut buf);
        // The combined-art row contains TURBO art ending with "██████╗ " and
        // immediately afterwards (after the "  " gap) BIBLE art starting with
        // "██████╗". On a single row we should see both signatures.
        let mut found_combined = false;
        for y in 0..area.height {
            let mut row_text = String::new();
            for x in 0..area.width {
                row_text.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            // The TURBO art's row 1 ends "██████╗  ██████╗ "; the BIBLE art's
            // row 1 begins "██████╗ ██╗██████╗". Look for the unique BIBLE
            // signature "██╗██████╗ ██╗     ███████╗" which only appears in
            // BIBLE's first row, and verify the row ALSO contains TURBO's
            // "╗██╗   ██╗" signature.
            if row_text.contains("██╗     ███████╗") && row_text.contains("██████╗  ██████╗ ")
            {
                found_combined = true;
                break;
            }
        }
        assert!(
            found_combined,
            "expected TURBO and BIBLE block letters on the same row"
        );
    }

    #[test]
    fn switch_focus_clamps_cursor() {
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        splash.cursor_ot = 35; // valid in OT
        splash.switch_focus(SplashColumn::NT);
        assert!(splash.cursor_nt <= 26);
    }

    #[test]
    fn move_up_from_top_lands_on_continue() {
        let last = Some((
            Position {
                book: "MRK".into(),
                chapter: 1,
                verse: None,
            },
            "Markus 1:1".into(),
        ));
        let mut splash =
            SplashView::new(fake_books(39, 27), last, "t".into(), "en-kjv".into(), None);
        splash.on_continue = false;
        splash.cursor_ot = 0;
        splash.move_up(1);
        assert!(splash.on_continue);
    }
}
