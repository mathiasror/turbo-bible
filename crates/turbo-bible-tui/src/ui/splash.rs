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

// 6-row ANSI-Shadow block letters — the full glyphs. The 4th row carries the
// B's lower bowl and the E's middle bar, so dropping it (as a past "compact"
// pass did) mangles those letterforms; keep all six. Both arrays must stay the
// same height; `render_title` zips them side by side.
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
    filter: String,
    focus: SplashColumn,
    cursor_ot: usize,
    cursor_nt: usize,
    /// True when the cursor is on the "Continue" row above the columns.
    on_continue: bool,
    translation_name: String,
    translation_code: String,
    /// Read by `main::mode_tag_for` to populate the status bar's mode pill.
    pub(crate) mode: SplashMode,
    quote: Option<DailyQuote>,
    /// One-line "update available" banner, set by `main` when the startup
    /// update check (or its cache) finds a newer release. `None` = no banner.
    /// Splash-only: the reading view never shows it. See [`crate::update`].
    update_banner: Option<String>,
    /// Chord + count state for `gg`, `G`, `5j`, etc. Shared with the
    /// list dialogs so the third copy of this state machine doesn't
    /// have to live here. Splash-specific keys (Ctrl-D/U/F/B,
    /// PageUp/Down, Home/End, column-switch, `o`/Enter) bypass it.
    nav: ListNav,
}

#[non_exhaustive]
pub enum SplashOutcome {
    Continue,
    OpenBook(Position),
    OpenGoto,
    OpenFind,
    OpenTranslations,
    OpenHelp,
    Quit,
}

/// Screen geometry of the on-screen book picker, recomputed for a mouse
/// hit-test by replaying the same header builders [`SplashView::render`] uses —
/// so the first-book row and column spans can't drift from what was drawn.
/// All coordinates are absolute terminal cells.
struct BookGrid {
    /// Row of the "Continue" entry, when one is shown.
    continue_row: Option<u16>,
    /// Row of the first book entry (below the column header + underline).
    first_book_row: u16,
    /// Number of book rows visible below the headers.
    visible_rows: usize,
    /// Left edge and width of the Old-Testament column.
    ot_x: u16,
    ot_w: u16,
    /// Left edge and width of the New-Testament column.
    nt_x: u16,
    nt_w: u16,
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
            update_banner: None,
            nav: ListNav::default(),
        }
    }

    /// Set the "update available" banner line shown above the daily verse.
    /// Called by `main` once the startup update check resolves.
    pub fn set_update_banner(&mut self, text: String) {
        self.update_banner = Some(text);
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

    /// Recompute the book-picker geometry for a mouse hit-test by replaying the
    /// header builders [`Self::render`] runs, then reading off where the first
    /// book row lands. `inner` is the dialog's interior rect (see
    /// [`dialog::inner_of`]).
    fn book_grid(&self, inner: Rect) -> BookGrid {
        let styles = RenderStyles::new(self.mode);
        let inner_w = inner.width as usize;
        // A throwaway buffer of lines built with the exact same calls render()
        // makes, so `scratch.len()` tracks the real header height through the
        // dynamic title art / quote / banner / Continue rows.
        let mut scratch: Vec<Line<'static>> = Vec::new();
        self.render_title(&styles, inner_w, inner.height as usize, &mut scratch);
        self.render_update_banner(&styles, inner_w, &mut scratch);
        self.render_quote(&styles, inner_w, &mut scratch);
        self.render_filter_row(&styles, inner_w, &mut scratch);
        // The Continue entry, when present, is the next line render_continue_row
        // pushes (it then pushes a trailing blank).
        let continue_idx = self.last.as_ref().map(|_| scratch.len());
        self.render_continue_row(&styles, inner_w, &mut scratch);
        // render_columns always pushes two rows (the column header + its
        // underline) before the first book row; mirror that `+ 2` here.
        let first_book_idx = scratch.len() + 2;
        let visible_rows = (inner.height as usize)
            .saturating_sub(first_book_idx)
            .saturating_sub(1); // trailing footer row
        let (col_left, col_right, gap) = split_columns(inner_w);
        let to_u16 = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
        let ot_w = to_u16(col_left);
        let nt_x = inner
            .left()
            .saturating_add(ot_w)
            .saturating_add(to_u16(gap));
        BookGrid {
            continue_row: continue_idx.map(|i| inner.top().saturating_add(to_u16(i))),
            first_book_row: inner.top().saturating_add(to_u16(first_book_idx)),
            visible_rows,
            ot_x: inner.left(),
            ot_w,
            nt_x,
            nt_w: to_u16(col_right),
        }
    }

    /// Handle a left-click at terminal `(col, row)`, with the splash dialog
    /// centered inside `outer` (the body region). A click on a book row opens
    /// that book exactly as `Enter`/`o` would; a click on "Continue" resumes the
    /// last position; anything else (chrome, the inter-column gap, an empty cell
    /// below the list) is a no-op. Filter- and scroll-aware: it maps to the same
    /// (possibly filtered, possibly scrolled) book the draw showed at that row.
    pub fn click(&mut self, outer: Rect, col: u16, row: u16) -> SplashOutcome {
        // Recover the dialog interior without drawing — mirrors render().
        let w = outer.width.saturating_sub(6).min(110);
        let h = outer.height.saturating_sub(2);
        let area = dialog::center(outer, w, h);
        let inner = dialog::inner_of(area);
        let inside = col >= inner.left()
            && col < inner.right()
            && row >= inner.top()
            && row < inner.bottom();
        if !inside {
            return SplashOutcome::Continue;
        }

        let grid = self.book_grid(inner);

        // The "Continue" entry sits above the columns.
        if grid.continue_row == Some(row) {
            self.on_continue = true;
            return self
                .open_current()
                .map_or(SplashOutcome::Continue, SplashOutcome::OpenBook);
        }

        // Book rows.
        if row < grid.first_book_row {
            return SplashOutcome::Continue;
        }
        let book_row = usize::from(row - grid.first_book_row);
        if book_row >= grid.visible_rows {
            return SplashOutcome::Continue;
        }
        let column = if col >= grid.ot_x && col < grid.ot_x.saturating_add(grid.ot_w) {
            SplashColumn::OT
        } else if col >= grid.nt_x && col < grid.nt_x.saturating_add(grid.nt_w) {
            SplashColumn::NT
        } else {
            // The gap between the two columns.
            return SplashOutcome::Continue;
        };

        let entries = self.entries(column);
        let cursor = match column {
            SplashColumn::OT => self.cursor_ot,
            SplashColumn::NT => self.cursor_nt,
        };
        // The scroll the draw used is keyed on the *current* cursor, so resolve
        // the clicked index against that before moving the cursor.
        let idx = scroll_for(cursor, entries.len(), grid.visible_rows) + book_row;
        if idx >= entries.len() {
            // An empty cell below the (possibly filtered) list.
            return SplashOutcome::Continue;
        }

        // Commit the click to the selection so open_current() — and the visible
        // cursor, should the open somehow no-op — reflect it, then open as Enter.
        self.focus = column;
        self.on_continue = false;
        match column {
            SplashColumn::OT => self.cursor_ot = idx,
            SplashColumn::NT => self.cursor_nt = idx,
        }
        self.open_current()
            .map_or(SplashOutcome::Continue, SplashOutcome::OpenBook)
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

    #[allow(
        clippy::too_many_lines,
        reason = "flat keymap for splash mode — every arm is one outcome, \
                  decomposing into per-arm helpers would just move the code \
                  out of the dispatch's match without changing the total."
    )]
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
            KeyCode::F(1) => SplashOutcome::OpenHelp,
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

        let styles = RenderStyles::new(self.mode);
        let inner_w = inner.width as usize;
        let mut lines: Vec<Line<'static>> = Vec::new();

        self.render_title(&styles, inner_w, inner.height as usize, &mut lines);
        self.render_update_banner(&styles, inner_w, &mut lines);
        self.render_quote(&styles, inner_w, &mut lines);
        self.render_filter_row(&styles, inner_w, &mut lines);
        self.render_continue_row(&styles, inner_w, &mut lines);
        let entries_ot = self.entries(SplashColumn::OT);
        let entries_nt = self.entries(SplashColumn::NT);
        self.render_columns(
            &styles,
            inner_w,
            inner.height,
            &entries_ot,
            &entries_nt,
            &mut lines,
        );
        self.render_footer(&styles, inner_w, &entries_ot, &entries_nt, &mut lines);

        Paragraph::new(lines).style(styles.bg).render(inner, buf);
    }

    fn render_title(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        avail: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        // The full block-letter banner is the home screen's "moment" — shown for
        // returning users as well as first-launch, as long as the terminal has
        // the room. We reserve `BANNER_RESERVE` rows below the art for the daily
        // verse, filter row, Continue row, and book picker so the banner never
        // squeezes the picker off-screen; when even the stacked banner wouldn't
        // leave that room, fall back to the one-line title. Side-by-side first,
        // then stacked, then the one-liner.
        const BANNER_RESERVE: usize = 12;
        // One blank row of "luft" so the art doesn't butt against the dialog's
        // top border; counted into the height budget below.
        const TOP_LUFT: usize = 1;
        let turbo_w = TITLE_TURBO[0].chars().count();
        let combined_w = turbo_w + 2 + TITLE_BIBLE[0].chars().count();
        let side_by_side_h = TOP_LUFT + TITLE_TURBO.len() + 1; // luft + art + subtitle
        let stacked_h = TOP_LUFT + TITLE_TURBO.len() + TITLE_BIBLE.len() + 1;
        for _ in 0..TOP_LUFT {
            lines.push(blank_line(inner_w, styles.bg));
        }
        if inner_w >= combined_w && avail >= side_by_side_h + BANNER_RESERVE {
            for (t, b) in TITLE_TURBO.iter().zip(TITLE_BIBLE.iter()) {
                lines.push(center_padded(
                    inner_w,
                    styles.bg,
                    &format!("{t}  {b}"),
                    styles.title,
                ));
            }
        } else if inner_w >= turbo_w && avail >= stacked_h + BANNER_RESERVE {
            for row in TITLE_TURBO.iter().chain(TITLE_BIBLE.iter()) {
                lines.push(center_padded(inner_w, styles.bg, row, styles.title));
            }
        } else {
            lines.push(center_padded(
                inner_w,
                styles.bg,
                TITLE_COMPACT,
                styles.title,
            ));
        }
        lines.push(center_padded(
            inner_w,
            styles.bg,
            &format!("· {} ·", self.translation_name),
            styles.subtitle,
        ));
    }

    /// Render the "update available" banner between the title and the daily
    /// verse, wrapped in blank rows so it reads as its own strip. Nothing is
    /// emitted when no update is pending.
    fn render_update_banner(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        let Some(banner) = &self.update_banner else {
            return;
        };
        lines.push(blank_line(inner_w, styles.bg));
        lines.push(center_padded(inner_w, styles.bg, banner, styles.update));
        lines.push(blank_line(inner_w, styles.bg));
    }

    fn render_quote(&self, styles: &RenderStyles, inner_w: usize, lines: &mut Vec<Line<'static>>) {
        let Some(q) = &self.quote else { return };
        // No leading blank: the subtitle sits directly above the verse so the
        // book picker rises (the verse's own dim reference line below provides
        // separation from the filter row).
        let max_width = inner_w.saturating_sub(8).max(20);
        // Wrap the body so it renders as one block; the open and close curly
        // quotes hug the first/last words.
        let mut body_lines = word_wrap(&q.text, max_width);
        if let Some(first) = body_lines.first_mut() {
            *first = format!("\u{201C}{first}");
        }
        if let Some(last) = body_lines.last_mut() {
            *last = format!("{last}\u{201D}");
        }
        // The daily verse is the second-strongest element after the title —
        // bold it so it doesn't read at the same weight as the book picker.
        for body_line in &body_lines {
            lines.push(center_padded(
                inner_w,
                styles.bg,
                body_line,
                styles.label.add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(center_padded(
            inner_w,
            styles.bg,
            &format!("\u{2014} {}", q.reference),
            styles.dim,
        ));
    }

    fn render_filter_row(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        // A blank row sets the filter input apart from the daily-verse
        // attribution (or the subtitle, when the quote is off) directly above,
        // so the verse block and the input affordance don't read as one strip.
        // The trailing blank below still separates it from Continue / columns.
        lines.push(blank_line(inner_w, styles.bg));
        let mode_label = match self.mode {
            SplashMode::Normal => " NORMAL ",
            SplashMode::Filter => " FILTER ",
        };
        let filter_display = if self.filter.is_empty() {
            match self.mode {
                SplashMode::Filter => " (type to filter) ".to_string(),
                SplashMode::Normal => " press / to filter ".to_string(),
            }
        } else {
            format!(" {} ", self.filter)
        };
        // Mode pill bevel: bright_white left + dark_grey right edge cells
        // give the pill a raised look. The pill's own bg fills the rest of
        // each bevel cell so the highlight reads as a soft rim.
        let pill_bg = styles.mode.bg.unwrap_or_else(theme::mode_pill_bg);
        let bevel_left = Style::new().fg(theme::bright_white()).bg(pill_bg);
        let bevel_right = Style::new().fg(theme::dark_grey()).bg(pill_bg);
        // Input field "sunken" edges: dark sliver on the left rim, bright
        // sliver on the right rim. Mirrors the bevel direction so an input
        // reads as inset where a pill reads as raised.
        let input_bg = styles.filter.bg.unwrap_or_else(theme::input_field_bg);
        let input_edge_left = Style::new().fg(theme::dark_grey()).bg(input_bg);
        let input_edge_right = Style::new().fg(theme::bright_white()).bg(input_bg);
        let mut filter_row = vec![
            Span::styled("  ", styles.bg),
            Span::styled("\u{258C}", bevel_left),
            Span::styled(mode_label, styles.mode),
            Span::styled("\u{2590}", bevel_right),
            Span::styled("  ", styles.bg),
            Span::styled("\u{258F}", input_edge_left),
            Span::styled(filter_display.clone(), styles.filter),
        ];
        let cursor_extra = if self.mode == SplashMode::Filter {
            filter_row.push(Span::styled(
                "\u{2588}",
                styles.filter.fg(theme::bright_white()),
            ));
            1
        } else {
            0
        };
        // Give the input a deliberate width (mode pill + a cushion) so the
        // field doesn't pinch around short queries like "jo": pad the well's
        // own background out to `well_target`, then close the right rim.
        let well_target = mode_label.chars().count() + 32;
        let well_used = filter_display.chars().count() + cursor_extra;
        if well_used < well_target {
            filter_row.push(Span::styled(
                " ".repeat(well_target - well_used),
                styles.filter,
            ));
        }
        filter_row.push(Span::styled("\u{2595}", input_edge_right));
        // Lead = "  " + ▌ + mode + ▐ + "  " + ▏ = mode + 7; then the well and
        // the closing ▕ (1). Fill the rest of the row with the desktop bg.
        let used = mode_label.chars().count() + 7 + well_used.max(well_target) + 1;
        if used < inner_w {
            filter_row.push(Span::styled(" ".repeat(inner_w - used), styles.bg));
        }
        lines.push(Line::from(filter_row));
        lines.push(blank_line(inner_w, styles.bg));
    }

    fn render_continue_row(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        let Some((_p, label_str)) = &self.last else {
            return;
        };
        let on = self.on_continue;
        let row_style = if on { styles.sel } else { styles.label };
        let mark = if on { "  \u{25B8} " } else { "    " };
        let content = format!("Continue: {label_str}");
        let used = mark.chars().count() + content.chars().count();
        let pad = inner_w.saturating_sub(used);
        lines.push(Line::from(vec![
            Span::styled(mark, if on { styles.sel } else { styles.dim }),
            Span::styled(content, row_style),
            Span::styled(" ".repeat(pad), if on { styles.sel } else { styles.bg }),
        ]));
        lines.push(blank_line(inner_w, styles.bg));
    }

    fn render_columns(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        inner_h: u16,
        entries_ot: &[&Book],
        entries_nt: &[&Book],
        lines: &mut Vec<Line<'static>>,
    ) {
        let (col_left, col_right, gap) = split_columns(inner_w);
        let (ot_label, nt_label) = testament_labels(&self.translation_code);
        // When filtering, show "matched / total" so the count reads as a result
        // ("4 of 39 match"); unfiltered, show the plain total as a static label.
        let (ot_count, nt_count) = if self.filter.is_empty() {
            (
                self.books_ot.len().to_string(),
                self.books_nt.len().to_string(),
            )
        } else {
            (
                format!("{} / {}", entries_ot.len(), self.books_ot.len()),
                format!("{} / {}", entries_nt.len(), self.books_nt.len()),
            )
        };
        let ot_header = format!(" {ot_label}  ({ot_count}) ");
        let nt_header = format!(" {nt_label}  ({nt_count}) ");
        let ot_focused = self.focus == SplashColumn::OT && !self.on_continue;
        let nt_focused = self.focus == SplashColumn::NT && !self.on_continue;
        let ot_header_style = if ot_focused {
            styles.column_focused
        } else {
            styles.column_header
        };
        let nt_header_style = if nt_focused {
            styles.column_focused
        } else {
            styles.column_header
        };
        lines.push(Line::from(vec![
            Span::styled(left_padded(&ot_header, col_left), ot_header_style),
            Span::styled(" ".repeat(gap), styles.bg),
            Span::styled(left_padded(&nt_header, col_right), nt_header_style),
        ]));
        // The focused column's rule stays bright; the other dims to dark_grey
        // so the underline echoes which side h/l/Tab is acting on.
        let bright_rule = Style::new().fg(theme::bright_white()).bg(theme::blue());
        let dim_rule = Style::new().fg(theme::dark_grey()).bg(theme::blue());
        lines.push(Line::from(vec![
            Span::styled(
                "─".repeat(col_left),
                if ot_focused { bright_rule } else { dim_rule },
            ),
            Span::styled(" ".repeat(gap), styles.bg),
            Span::styled(
                "─".repeat(col_right),
                if nt_focused { bright_rule } else { dim_rule },
            ),
        ]));

        // Entries: side-by-side.
        let header_len = lines.len();
        let visible_rows = (inner_h as usize)
            .saturating_sub(header_len)
            .saturating_sub(1); // footer

        let scroll_ot = scroll_for(self.cursor_ot, entries_ot.len(), visible_rows);
        let scroll_nt = scroll_for(self.cursor_nt, entries_nt.len(), visible_rows);

        let entry_styles = EntryStyles {
            sel: styles.sel,
            dim: styles.dim,
            bg: styles.bg,
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
            spans.push(Span::styled(" ".repeat(gap), styles.bg));
            spans.extend(right);
            lines.push(Line::from(spans));
        }
    }

    fn render_footer(
        &self,
        styles: &RenderStyles,
        inner_w: usize,
        entries_ot: &[&Book],
        entries_nt: &[&Book],
        lines: &mut Vec<Line<'static>>,
    ) {
        let total_count = entries_ot.len() + entries_nt.len();
        // Split the readout into an always-kept core ("n/m" or "Continue") and
        // a droppable "(N total)" suffix so the budgeter can shed the suffix as
        // a unit rather than clip it mid-token.
        let (count_core, count_suffix) = if self.on_continue {
            ("Continue".to_string(), String::new())
        } else {
            let entries_focused = match self.focus {
                SplashColumn::OT => entries_ot,
                SplashColumn::NT => entries_nt,
            };
            let len = entries_focused.len();
            let cur = if len == 0 {
                0
            } else {
                self.current_cursor() + 1
            };
            (format!("{cur}/{len}"), format!(" ({total_count} total)"))
        };
        // The in-dialog footer carries only what's unique to this dialog —
        // splash-local motions and the live cursor/total readout. Global
        // shortcuts (Enter / F2 / F3 / Esc) live in the bottom status bar so
        // we don't show them twice. Highest-priority hint group first; the
        // budgeter drops from the end when the line would overflow.
        let groups: &[(&str, &str)] = match self.mode {
            SplashMode::Normal => &[
                ("j k ", "move  "),
                ("h l Tab ", "column  "),
                ("gg G ", "ends  "),
                ("/ ", "filter  "),
                ("t ", "translation  "),
            ],
            SplashMode::Filter => &[
                ("type ", "to filter  "),
                ("Enter ", "done  "),
                ("Esc ", "cancel  "),
                ("Ctrl-U ", "clear  "),
            ],
        };
        lines.push(assemble_footer(
            groups,
            &count_core,
            &count_suffix,
            inner_w,
            styles,
        ));
    }
}

/// Lay out the splash footer within `inner_w`: always keep the readout core,
/// add the `(N total)` suffix only if it fits whole, then fill hint groups in
/// priority order, dropping low-priority groups (and all after them) rather
/// than letting ratatui hard-clip the line mid-token.
fn assemble_footer(
    groups: &[(&str, &str)],
    count_core: &str,
    count_suffix: &str,
    inner_w: usize,
    styles: &RenderStyles,
) -> Line<'static> {
    const LEAD: usize = 2;
    let core_w = count_core.chars().count();
    let suffix_w = count_suffix.chars().count();
    // Never start a `(…)` we can't close: show the suffix only if it fits
    // alongside the core.
    let show_suffix = !count_suffix.is_empty() && LEAD + core_w + suffix_w <= inner_w;
    let count_w = core_w + if show_suffix { suffix_w } else { 0 };

    let mut spans: Vec<Span<'static>> = vec![Span::styled(" ".repeat(LEAD), styles.bg)];
    let mut groups_w = 0;
    for (k, l) in groups {
        let gw = k.chars().count() + l.chars().count();
        if LEAD + groups_w + gw + count_w > inner_w {
            break; // drop this group and every lower-priority one after it
        }
        spans.push(Span::styled((*k).to_string(), styles.key));
        spans.push(Span::styled((*l).to_string(), styles.dim));
        groups_w += gw;
    }
    // Right-align the readout in its own slot at the line's end, so it reads as
    // a position indicator rather than one more trailing hint group.
    let pad = inner_w.saturating_sub(LEAD + groups_w + count_w);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), styles.bg));
    }
    let count = if show_suffix {
        format!("{count_core}{count_suffix}")
    } else {
        count_core.to_string()
    };
    spans.push(Span::styled(count, styles.key));
    Line::from(spans)
}

/// Style table built once per render pass and shared across helpers.
/// Lives module-side rather than in a `lazy_static` because some styles
/// depend on `self.mode`.
struct RenderStyles {
    bg: Style,
    title: Style,
    subtitle: Style,
    dim: Style,
    label: Style,
    /// "Update available" banner — mid_cyan + bold. Deliberately NOT yellow:
    /// yellow is reserved for the title, mode pills, and single operative
    /// tokens, so an informational banner uses a structural cyan tier.
    update: Style,
    key: Style,
    sel: Style,
    filter: Style,
    mode: Style,
    column_header: Style,
    column_focused: Style,
}

impl RenderStyles {
    fn new(mode: SplashMode) -> Self {
        let bold = Modifier::BOLD;
        Self {
            bg: Style::new().bg(theme::blue()),
            title: Style::new()
                .fg(theme::yellow())
                .bg(theme::blue())
                .add_modifier(bold),
            subtitle: Style::new()
                .fg(theme::cyan())
                .bg(theme::blue())
                .add_modifier(bold),
            dim: Style::new().fg(theme::light_grey()).bg(theme::blue()),
            label: Style::new().fg(theme::bright_white()).bg(theme::blue()),
            update: Style::new()
                .fg(theme::mid_cyan())
                .bg(theme::blue())
                .add_modifier(bold),
            key: Style::new()
                .fg(theme::bright_white())
                .bg(theme::blue())
                .add_modifier(bold),
            sel: Style::new()
                .fg(theme::bright_white())
                .bg(theme::list_focus_bg())
                .add_modifier(bold),
            filter: Style::new()
                .fg(theme::black())
                .bg(theme::input_field_bg())
                .add_modifier(bold),
            mode: match mode {
                SplashMode::Filter => Style::new()
                    .fg(theme::black())
                    .bg(theme::yellow())
                    .add_modifier(bold),
                SplashMode::Normal => Style::new()
                    .fg(theme::black())
                    .bg(theme::mode_pill_bg())
                    .add_modifier(bold),
            },
            // Unfocused column header — dimmed so the focused (bright_white +
            // underline) side reads as "this is where j/k moves".
            column_header: Style::new()
                .fg(theme::light_grey())
                .bg(theme::blue())
                .add_modifier(bold),
            column_focused: Style::new()
                .fg(theme::bright_white())
                .bg(theme::blue())
                .add_modifier(bold | Modifier::UNDERLINED),
        }
    }
}

fn blank_line(inner_w: usize, bg: Style) -> Line<'static> {
    Line::from(Span::styled(" ".repeat(inner_w), bg))
}

fn center_padded(inner_w: usize, bg: Style, row: &str, st: Style) -> Line<'static> {
    let pad_left = inner_w.saturating_sub(row.chars().count()) / 2;
    let pad_right = inner_w
        .saturating_sub(pad_left)
        .saturating_sub(row.chars().count());
    Line::from(vec![
        Span::styled(" ".repeat(pad_left), bg),
        Span::styled(row.to_string(), st),
        Span::styled(" ".repeat(pad_right), bg),
    ])
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
    let EntryStyles { sel, dim, bg } = *styles;
    let Some(b) = book else {
        return vec![Span::styled(" ".repeat(width), bg)];
    };
    // Only render the cursor on the column that currently has focus. The
    // unfocused column remembers its position internally, but nothing visible
    // hints at it — avoids the "ghost cursor" effect.
    let is_cursor = idx == cursor_idx && column_has_focus;

    // Non-cursor book names sit at light_grey, one step below the bright_white
    // daily verse above the columns — so the verse reads as the second-
    // strongest element after the title (bright_white is already the ceiling,
    // so the hierarchy comes from dimming the picker, not brightening the verse).
    // The cursor row stays full-bright on its cyan slab.
    let row_style = if is_cursor { sel } else { dim };
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
        "pt" => ("Antigo Testamento", "Novo Testamento"),
        "la" => ("Vetus Testamentum", "Novum Testamentum"),
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

    /// The screen row carrying the focused cursor marker `▸` — the row the
    /// draw actually put the selected entry on, found independently of the
    /// hit-test geometry. (`find_cursor_row` is unreliable here: the filter
    /// input field shares the selection background, so it matches first.)
    fn marker_row(buf: &Buffer) -> Option<u16> {
        (0..buf.area.height)
            .find(|&y| (0..buf.area.width).any(|x| buf[(x, y)].symbol() == "\u{25B8}"))
    }

    #[test]
    fn click_on_a_book_row_opens_that_book() {
        // Render with the OT cursor on a known book, find the row the draw put
        // its `▸` marker on, then click that row+column. The geometry the
        // hit-test recomputes must agree with the draw, so the click resolves
        // back to the same book — across targets that force a scroll offset.
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        let area = Rect::new(0, 0, 110, 36);
        for target in [0usize, 5, 20, 38] {
            splash.focus = SplashColumn::OT;
            splash.cursor_ot = target;
            splash.on_continue = false;
            let mut buf = Buffer::empty(area);
            splash.render(area, &mut buf);
            let row = marker_row(&buf).expect("OT cursor marker drawn");
            // col 5 sits inside the OT column for a 110-wide splash.
            match splash.click(area, 5, row) {
                SplashOutcome::OpenBook(p) => {
                    assert_eq!(p.book, format!("O{target:02}"), "clicked OT row {row}");
                }
                _ => panic!("expected OpenBook for OT target {target} at row {row}"),
            }
        }
    }

    #[test]
    fn click_on_continue_row_resumes_last_position() {
        let last = (
            Position {
                book: "N03".into(),
                chapter: 2,
                verse: Some(4),
            },
            "NT Book 3 2:4".to_string(),
        );
        // A returning user: `on_continue` starts true, so the Continue row wears
        // the `▸` marker.
        let mut splash = SplashView::new(
            fake_books(39, 27),
            Some(last),
            "t".into(),
            "en-kjv".into(),
            None,
        );
        let area = Rect::new(0, 0, 110, 36);
        let mut buf = Buffer::empty(area);
        splash.render(area, &mut buf);
        let row = marker_row(&buf).expect("Continue marker drawn");
        match splash.click(area, 5, row) {
            SplashOutcome::OpenBook(p) => {
                assert_eq!(p.book, "N03");
                assert_eq!(p.chapter, 2);
            }
            _ => panic!("expected OpenBook resuming the last position"),
        }
    }

    #[test]
    fn click_off_the_picker_is_a_no_op() {
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        let area = Rect::new(0, 0, 110, 36);
        let mut buf = Buffer::empty(area);
        splash.render(area, &mut buf);
        // The very top-left corner is dialog chrome / desktop, never a book.
        assert!(matches!(splash.click(area, 0, 0), SplashOutcome::Continue));
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
    fn returning_user_still_gets_full_banner() {
        // A saved reading position (Continue target) no longer collapses the
        // banner: the block-letter art is the home screen's "moment" for
        // returning users too, as long as the terminal has the room.
        let last = Some((
            Position {
                book: "O00".into(),
                chapter: 1,
                verse: None,
            },
            "OT Book 0 1".to_string(),
        ));
        let splash = SplashView::new(fake_books(39, 27), last, "t".into(), "en-kjv".into(), None);
        let styles = RenderStyles::new(splash.mode);
        let mut lines = Vec::new();
        splash.render_title(&styles, 110, 30, &mut lines);
        assert_eq!(
            lines.len(),
            TITLE_TURBO.len() + 2,
            "full side-by-side banner is one luft row + {} art rows + the subtitle, got {}",
            TITLE_TURBO.len(),
            lines.len(),
        );
        // lines[0] is the blank luft row; the art starts at lines[1].
        let title: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !title.contains("T U R B O"),
            "expected the block-letter art, not the compact one-liner, got {title:?}",
        );
    }

    #[test]
    fn short_terminal_falls_back_to_compact_title() {
        // Too few rows to leave room for the picker below either the
        // side-by-side or stacked banner: collapse to the one-line title.
        let splash = SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        let styles = RenderStyles::new(splash.mode);
        let mut lines = Vec::new();
        splash.render_title(&styles, 110, 8, &mut lines);
        assert_eq!(
            lines.len(),
            3,
            "compact title is one luft row + one art row + the subtitle, got {}",
            lines.len(),
        );
        // lines[0] is the blank luft row; the compact title is lines[1].
        let title: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            title.contains("T U R B O"),
            "expected the compact one-liner, got {title:?}",
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

    /// Read the whole rendered buffer as one big string (rows concatenated),
    /// so a banner that spans wrapped/centered cells is still findable.
    fn rendered_text(splash: &SplashView, area: Rect) -> String {
        let mut buf = Buffer::empty(area);
        splash.render(area, &mut buf);
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn update_banner_hidden_by_default_then_shows_when_set() {
        let area = Rect::new(0, 0, 110, 30);
        let mut splash =
            SplashView::new(fake_books(39, 27), None, "t".into(), "en-kjv".into(), None);
        assert!(
            !rendered_text(&splash, area).contains("Update available"),
            "no banner before set_update_banner"
        );

        splash
            .set_update_banner("Update available: v9.9.9 \u{00b7} brew upgrade turbo-bible".into());
        let text = rendered_text(&splash, area);
        assert!(
            text.contains("Update available: v9.9.9"),
            "banner text should render once set"
        );
        assert!(
            text.contains("brew upgrade turbo-bible"),
            "banner should carry the upgrade command"
        );
    }
}
