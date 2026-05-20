mod bookmark;
mod db;
mod keys;
mod nav;
mod quote;
mod render;
mod search;
mod state;
mod theme;
mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::db::{Book, Db, Passage};
use crate::keys::{Action, KeyState};
use crate::nav::{Navigator, Position};
use crate::ui::find::{FindDialog, FindOutcome};
use crate::ui::footnote::{FootnoteDialog, FootnoteOutcome};
use crate::ui::goto::{GotoCommand, GotoDialog, GotoOutcome};
use crate::ui::help::{HelpDialog, HelpOutcome};
use crate::ui::menubar::MenuItem;
use crate::ui::splash::{SplashOutcome, SplashView};
use crate::ui::statusbar::Shortcut;

enum Bg {
    Splash(SplashView),
    Reading,
}

enum Dialog {
    None,
    Goto(GotoDialog),
    Find(FindDialog),
    Footnote(FootnoteDialog),
    Help(HelpDialog),
    Bookmarks(crate::ui::bookmarks::BookmarksDialog),
}

struct History {
    stack: Vec<Position>,
    cur: usize,
}

impl History {
    fn new(initial: Position) -> Self {
        Self { stack: vec![initial], cur: 0 }
    }
    fn push(&mut self, p: Position) {
        self.stack.truncate(self.cur + 1);
        if self
            .stack
            .last()
            .map_or(true, |last| last.book != p.book || last.chapter != p.chapter)
        {
            self.stack.push(p);
            self.cur = self.stack.len() - 1;
        }
    }
    fn back(&mut self) -> Option<Position> {
        if self.cur == 0 {
            return None;
        }
        self.cur -= 1;
        Some(self.stack[self.cur].clone())
    }
    fn forward(&mut self) -> Option<Position> {
        if self.cur + 1 >= self.stack.len() {
            return None;
        }
        self.cur += 1;
        Some(self.stack[self.cur].clone())
    }
}

#[derive(Parser, Debug)]
#[command(version, about = "Turbo-Vision Bible reader")]
struct Args {
    /// Path to bible.sqlite (defaults to ../bible.sqlite relative to bin)
    #[arg(long, default_value = "../bible.sqlite")]
    db: PathBuf,

    /// Translation code (default: nb-2024)
    #[arg(long, default_value = "nb-2024")]
    translation: String,

    /// Book to open initially (OSIS code). When provided, skips the splash.
    #[arg(long)]
    book: Option<String>,

    /// Chapter to open initially. Requires --book.
    #[arg(long, default_value_t = 1)]
    chapter: i64,
}

type Tty = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    let args = Args::parse();
    match db::ensure_fts_optimized(&args.db) {
        Ok(true) => eprintln!("Optimizing search index (one-time)…"),
        Ok(false) => {}
        Err(e) => eprintln!("warning: FTS optimize skipped: {e}"),
    }
    let db = Db::open_ro(&args.db, &args.translation)?;
    let books = db.list_books()?;
    let nav_ = Navigator::new(&books);

    let translation_label = format!("Bibel 2024 (bokmål)  ·  {}", args.translation);

    // Resolve persisted state for the Continue option.
    let persisted = state::load();
    let last_for_splash: Option<(Position, String)> = persisted
        .as_ref()
        .filter(|ps| ps.translation == args.translation)
        .map(|ps| {
            let label = books
                .iter()
                .find(|b| b.code == ps.book)
                .map(|b| format!("{} {}:{}", b.name, ps.chapter, ps.verse))
                .unwrap_or_else(|| format!("{} {}:{}", ps.book, ps.chapter, ps.verse));
            (
                Position { book: ps.book.clone(), chapter: ps.chapter },
                label,
            )
        });

    // Starting screen: if --book was passed explicitly, go straight to reading.
    let mut term = init_terminal()?;
    let final_pos: Option<Position>;
    let final_cursor_verse: i64;
    let result = if let Some(book_code) = args.book.clone() {
        let mut pos = Position { book: book_code, chapter: args.chapter };
        let mut passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = 1;
        let r = run(
            &mut term,
            &db,
            &nav_,
            &books,
            translation_label.clone(),
            &mut pos,
            &mut passage,
            &mut cursor_verse,
            Bg::Reading,
        );
        final_pos = Some(pos);
        final_cursor_verse = cursor_verse;
        r
    } else {
        let qotd = quote::pick(&db).unwrap_or(None);
        let splash = SplashView::new(
            books.clone(),
            last_for_splash.clone(),
            translation_label.clone(),
            qotd,
        );
        // We still need *some* initial passage state for the run loop; load
        // the persisted-or-default position lazily-ish.
        let mut pos = match &last_for_splash {
            Some((p, _)) => p.clone(),
            None => Position { book: "GEN".into(), chapter: 1 },
        };
        let mut passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = persisted.as_ref().map(|p| p.verse).unwrap_or(1).max(1);
        let r = run(
            &mut term,
            &db,
            &nav_,
            &books,
            translation_label.clone(),
            &mut pos,
            &mut passage,
            &mut cursor_verse,
            Bg::Splash(splash),
        );
        final_pos = Some(pos);
        final_cursor_verse = cursor_verse;
        r
    };
    restore_terminal(&mut term)?;

    if let Some(p) = final_pos {
        let _ = state::save(&state::PersistedState {
            translation: args.translation.clone(),
            book: p.book,
            chapter: p.chapter,
            verse: final_cursor_verse,
        });
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn run(
    term: &mut Tty,
    db: &Db,
    nav_: &Navigator<'_>,
    books: &[Book],
    translation_label: String,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    initial_bg: Bg,
) -> Result<()> {
    let menu = [
        MenuItem { label: "File", hotkey_idx: 0 },
        MenuItem { label: "Search", hotkey_idx: 0 },
        MenuItem { label: "Goto", hotkey_idx: 0 },
        MenuItem { label: "Help", hotkey_idx: 0 },
    ];
    let mut keys = KeyState::new();
    let mut history = History::new(pos.clone());
    let mut bg = initial_bg;
    let mut dialog: Dialog = Dialog::None;
    let mut show_sidebar = true;
    let mut visual_anchor: Option<i64> = None;
    let mut bookmarks = bookmark::BookmarkStore::load();
    let mut verse_layout_two_line = true;
    let mut last_label_for_splash: Option<(Position, String)> = books
        .iter()
        .find(|b| b.code == pos.book)
        .map(|b| {
            (
                pos.clone(),
                format!("{} {}:{}", b.name, pos.chapter, *cursor_verse),
            )
        });

    loop {
        let status = make_status(&bg);
        let bookmarked_in_chapter = bookmarks_set(&bookmarks, &db.translation, pos);
        term.draw(|f| {
            let area = f.area();
            let buf = f.buffer_mut();
            match &bg {
                Bg::Splash(s) => {
                    // Plain blue desktop behind the splash window.
                    crate::ui::desktop::render(
                        ratatui::layout::Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2)),
                        buf,
                    );
                    crate::ui::menubar::render(
                        &menu,
                        ratatui::layout::Rect::new(area.x, area.y, area.width, 1),
                        buf,
                    );
                    let mode_tag = mode_tag_for(&bg, &dialog, visual_anchor.is_some());
                    crate::ui::statusbar::render(
                        &status,
                        ratatui::layout::Rect::new(area.x, area.y + area.height - 1, area.width, 1),
                        buf,
                        mode_tag,
                    );
                    let body = ratatui::layout::Rect::new(
                        area.x,
                        area.y + 1,
                        area.width,
                        area.height.saturating_sub(2),
                    );
                    s.render(body, buf);
                }
                Bg::Reading => {
                    let mode_tag = mode_tag_for(&bg, &dialog, visual_anchor.is_some());
                    let selection = visual_anchor.map(|a| {
                        let c = *cursor_verse;
                        if a <= c { (a, c) } else { (c, a) }
                    });
                    ui::Frame {
                        menu: &menu,
                        status: &status,
                        status_mode: mode_tag,
                        passage: Some(passage),
                        cursor_verse: *cursor_verse,
                        selection,
                        bookmarked: &bookmarked_in_chapter,
                        show_sidebar,
                        two_line_verses: verse_layout_two_line,
                    }
                    .render(area, buf);
                }
            }
            match &dialog {
                Dialog::None => {}
                Dialog::Goto(d) => d.render(area, buf, books),
                Dialog::Find(d) => d.render(area, buf, books),
                Dialog::Footnote(d) => d.render(area, buf, books),
                Dialog::Help(d) => d.render(area, buf),
                Dialog::Bookmarks(d) => d.render(area, buf, books),
            }
        })?;

        if event::poll(Duration::from_millis(150))? {
            let term_height = term.size().map(|s| s.height).unwrap_or(24);
            let raw_event = event::read()?;
            let synth: Option<KeyEvent> = match raw_event {
                Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
                Event::Mouse(me) => mouse_to_key(me, term_height, &status),
                _ => None,
            };
            if let Some(key) = synth {
                // Dialogs consume input first.
                match &mut dialog {
                    Dialog::None => {}
                    Dialog::Goto(d) => {
                        match d.handle(key, books) {
                            GotoOutcome::Continue => {}
                            GotoOutcome::Cancel => dialog = Dialog::None,
                            GotoOutcome::Jump(p) => {
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(&mut last_label_for_splash, books, pos, *cursor_verse);
                                bg = Bg::Reading;
                                dialog = Dialog::None;
                            }
                            GotoOutcome::Command(GotoCommand::Quit) => return Ok(()),
                            GotoOutcome::Command(GotoCommand::Help) => {
                                dialog = Dialog::Help(HelpDialog::new());
                            }
                        }
                        continue;
                    }
                    Dialog::Find(d) => {
                        match d.handle(key, db) {
                            FindOutcome::Continue => {}
                            FindOutcome::Cancel => dialog = Dialog::None,
                            FindOutcome::Jump(p, _q) => {
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(&mut last_label_for_splash, books, pos, *cursor_verse);
                                bg = Bg::Reading;
                                dialog = Dialog::None;
                            }
                        }
                        continue;
                    }
                    Dialog::Footnote(d) => {
                        match d.handle(key) {
                            FootnoteOutcome::Continue => {}
                            FootnoteOutcome::Cancel => dialog = Dialog::None,
                            FootnoteOutcome::Jump(p) => {
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(&mut last_label_for_splash, books, pos, *cursor_verse);
                                bg = Bg::Reading;
                                dialog = Dialog::None;
                            }
                        }
                        continue;
                    }
                    Dialog::Help(d) => {
                        match d.handle(key) {
                            HelpOutcome::Continue => {}
                            HelpOutcome::Cancel => dialog = Dialog::None,
                        }
                        continue;
                    }
                    Dialog::Bookmarks(d) => {
                        use crate::ui::bookmarks::BookmarksOutcome;
                        match d.handle(key) {
                            BookmarksOutcome::Continue => {}
                            BookmarksOutcome::Cancel => dialog = Dialog::None,
                            BookmarksOutcome::Jump(p) => {
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(
                                    &mut last_label_for_splash,
                                    books,
                                    pos,
                                    *cursor_verse,
                                );
                                bg = Bg::Reading;
                                dialog = Dialog::None;
                            }
                            BookmarksOutcome::Delete(bm) => {
                                bookmarks
                                    .bookmarks
                                    .retain(|b| !b.same_range(&bm));
                                let _ = bookmarks.save();
                            }
                        }
                        continue;
                    }
                }

                // No dialog: route to current background.
                match &mut bg {
                    Bg::Splash(s) => match s.handle(key) {
                        SplashOutcome::Continue => {}
                        SplashOutcome::Quit => return Ok(()),
                        SplashOutcome::OpenGoto => dialog = Dialog::Goto(GotoDialog::new()),
                        SplashOutcome::OpenFind => dialog = Dialog::Find(FindDialog::new()),
                        SplashOutcome::OpenBook(p) => {
                            jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                            update_splash_label(
                                &mut last_label_for_splash,
                                books,
                                pos,
                                *cursor_verse,
                            );
                            bg = Bg::Reading;
                        }
                    },
                    Bg::Reading => {
                        if let Some(action) = keys.handle(key) {
                            match action {
                                Action::OpenGoto => dialog = Dialog::Goto(GotoDialog::new()),
                                Action::OpenFind => dialog = Dialog::Find(FindDialog::new()),
                                Action::OpenHelp => dialog = Dialog::Help(HelpDialog::new()),
                                Action::OpenFootnote => {
                                    let target = format!(
                                        "{}.{}.{}",
                                        pos.book, pos.chapter, *cursor_verse
                                    );
                                    let notes: Vec<_> = passage
                                        .footnotes
                                        .iter()
                                        .filter(|fn_| fn_.verse_osis == target)
                                        .cloned()
                                        .collect();
                                    let label = format!(
                                        "{} {}:{}",
                                        passage.book_abbrev, pos.chapter, *cursor_verse
                                    );
                                    dialog = Dialog::Footnote(FootnoteDialog::new(label, notes));
                                }
                                Action::JumpBack => {
                                    if let Some(p) = history.back() {
                                        *pos = p;
                                        *passage = db.load_passage(&pos.book, pos.chapter)?;
                                        *cursor_verse = 1;
                                    }
                                }
                                Action::JumpForward => {
                                    if let Some(p) = history.forward() {
                                        *pos = p;
                                        *passage = db.load_passage(&pos.book, pos.chapter)?;
                                        *cursor_verse = 1;
                                    }
                                }
                                Action::CopyVerse => {
                                    let _ = copy_verse_to_clipboard(passage, pos, *cursor_verse);
                                }
                                Action::ToggleSidebar => show_sidebar = !show_sidebar,
                                Action::ToggleVerseLayout => verse_layout_two_line = !verse_layout_two_line,
                                Action::ToggleVisual => {
                                    visual_anchor = if visual_anchor.is_some() {
                                        None
                                    } else {
                                        Some(*cursor_verse)
                                    };
                                }
                                Action::AddBookmark => {
                                    let (s, e) = match visual_anchor {
                                        Some(a) if a <= *cursor_verse => (a, *cursor_verse),
                                        Some(a) => (*cursor_verse, a),
                                        None => (*cursor_verse, *cursor_verse),
                                    };
                                    bookmarks.add(bookmark::Bookmark {
                                        translation: db.translation.clone(),
                                        book: pos.book.clone(),
                                        chapter: pos.chapter,
                                        start_verse: s,
                                        end_verse: e,
                                        label: None,
                                        created_at: bookmark::now_unix(),
                                    });
                                    let _ = bookmarks.save();
                                    visual_anchor = None;
                                }
                                Action::OpenBookmarks => {
                                    let mut d =
                                        crate::ui::bookmarks::BookmarksDialog::new(&bookmarks);
                                    d.sort_canonical(books);
                                    dialog = Dialog::Bookmarks(d);
                                }
                                Action::Back => {
                                    // In visual mode, Esc cancels the selection
                                    // instead of returning to splash.
                                    if visual_anchor.is_some() {
                                        visual_anchor = None;
                                    } else {
                                        update_splash_label(
                                            &mut last_label_for_splash,
                                            books,
                                            pos,
                                            *cursor_verse,
                                        );
                                        let qotd = quote::pick(db).unwrap_or(None);
                                        bg = Bg::Splash(SplashView::new(
                                            books.to_vec(),
                                            last_label_for_splash.clone(),
                                            translation_label.clone(),
                                            qotd,
                                        ));
                                    }
                                }
                                Action::Quit => return Ok(()),
                                _ => {
                                    if apply_action(
                                        action, db, nav_, pos, passage, cursor_verse,
                                        &mut history,
                                    )? {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            keys.tick();
        }
    }
}

fn bookmarks_set(
    store: &bookmark::BookmarkStore,
    translation: &str,
    pos: &Position,
) -> std::collections::BTreeSet<i64> {
    let mut out = std::collections::BTreeSet::new();
    for b in &store.bookmarks {
        if b.matches_chapter(translation, &pos.book, pos.chapter) {
            for v in b.start_verse..=b.end_verse {
                out.insert(v);
            }
        }
    }
    out
}

fn mode_tag_for(bg: &Bg, dialog: &Dialog, visual: bool) -> &'static str {
    match dialog {
        Dialog::Goto(_) => "-- COMMAND --",
        Dialog::Find(_) => "-- SEARCH --",
        Dialog::Footnote(_) => "-- NOTES --",
        Dialog::Help(_) => "-- HELP --",
        Dialog::Bookmarks(_) => "-- BOOKMARKS --",
        Dialog::None => match bg {
            Bg::Splash(s) => match s.mode {
                crate::ui::splash::SplashMode::Normal => "-- NORMAL --",
                crate::ui::splash::SplashMode::Filter => "-- FILTER --",
            },
            Bg::Reading if visual => "-- VISUAL --",
            Bg::Reading => "-- NORMAL --",
        },
    }
}

fn make_status(bg: &Bg) -> Vec<Shortcut<'static>> {
    match bg {
        Bg::Splash(_) => vec![
            Shortcut { key: "Enter", action: "Open" },
            Shortcut { key: "F2", action: "Goto" },
            Shortcut { key: "F3", action: "Find" },
            Shortcut { key: "Esc", action: "Quit" },
        ],
        Bg::Reading => vec![
            Shortcut { key: "F1", action: "Help" },
            Shortcut { key: "F2", action: "Goto" },
            Shortcut { key: "F3", action: "Find" },
            Shortcut { key: "Esc", action: "Home" },
            Shortcut { key: "Q", action: "Quit" },
        ],
    }
}

fn update_splash_label(
    target: &mut Option<(Position, String)>,
    books: &[Book],
    pos: &Position,
    verse: i64,
) {
    let name = books
        .iter()
        .find(|b| b.code == pos.book)
        .map(|b| b.name.clone())
        .unwrap_or_else(|| pos.book.clone());
    *target = Some((pos.clone(), format!("{} {}:{}", name, pos.chapter, verse)));
}

fn jump_to(
    p: Position,
    db: &Db,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    history: &mut History,
) -> Result<()> {
    history.push(p.clone());
    *pos = p;
    *passage = db.load_passage(&pos.book, pos.chapter)?;
    *cursor_verse = 1;
    Ok(())
}

fn copy_verse_to_clipboard(passage: &Passage, pos: &Position, verse: i64) -> Result<()> {
    let v = passage
        .verses
        .iter()
        .find(|v| v.number == verse)
        .ok_or_else(|| anyhow::anyhow!("verse not in passage"))?;
    let text = v.text.replace('\n', " ");
    let payload = format!(
        "{} {}:{} \u{2014} {}",
        passage.book_name, pos.chapter, verse, text
    );
    let mut cb = arboard::Clipboard::new()?;
    cb.set_text(payload)?;
    Ok(())
}

fn max_verse(passage: &Passage) -> i64 {
    passage.verses.last().map(|v| v.number).unwrap_or(1)
}

/// Returns true if the loop should exit.
fn apply_action(
    action: Action,
    db: &Db,
    nav_: &Navigator<'_>,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    history: &mut History,
) -> Result<bool> {
    let last = max_verse(passage);
    let half: i64 = 10;
    let page: i64 = 20;
    match action {
        Action::Quit => Ok(true),
        Action::CursorDown(n) => {
            *cursor_verse = (*cursor_verse + n as i64).min(last);
            Ok(false)
        }
        Action::CursorUp(n) => {
            *cursor_verse = (*cursor_verse - n as i64).max(1);
            Ok(false)
        }
        Action::HalfPageDown => {
            *cursor_verse = (*cursor_verse + half).min(last);
            Ok(false)
        }
        Action::HalfPageUp => {
            *cursor_verse = (*cursor_verse - half).max(1);
            Ok(false)
        }
        Action::PageDown => {
            *cursor_verse = (*cursor_verse + page).min(last);
            Ok(false)
        }
        Action::PageUp => {
            *cursor_verse = (*cursor_verse - page).max(1);
            Ok(false)
        }
        Action::GotoTop => {
            *cursor_verse = 1;
            Ok(false)
        }
        Action::GotoBottom => {
            *cursor_verse = last;
            Ok(false)
        }
        Action::PrevChapter => {
            let new_pos = nav_.prev_chapter(db, pos)?;
            jump_to(new_pos, db, pos, passage, cursor_verse, history)?;
            Ok(false)
        }
        Action::NextChapter => {
            let new_pos = nav_.next_chapter(db, pos)?;
            jump_to(new_pos, db, pos, passage, cursor_verse, history)?;
            Ok(false)
        }
        Action::PrevBook => {
            let new_pos = nav_.prev_book(pos)?;
            jump_to(new_pos, db, pos, passage, cursor_verse, history)?;
            Ok(false)
        }
        Action::NextBook => {
            let new_pos = nav_.next_book(pos)?;
            jump_to(new_pos, db, pos, passage, cursor_verse, history)?;
            Ok(false)
        }
        Action::CopyVerse
        | Action::OpenGoto
        | Action::OpenFind
        | Action::OpenFootnote
        | Action::OpenHelp
        | Action::OpenMenu
        | Action::JumpBack
        | Action::JumpForward
        | Action::ToggleSidebar
        | Action::Back
        | Action::ToggleVisual
        | Action::AddBookmark
        | Action::OpenBookmarks
        | Action::ToggleVerseLayout => Ok(false),
    }
}

fn init_terminal() -> Result<Tty> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(out))?)
}

fn restore_terminal(term: &mut Tty) -> Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    term.show_cursor()?;
    Ok(())
}

/// Translate a mouse event into a synthetic key event so clicks on the
/// menubar / statusbar reuse the existing keyboard dispatch path. Scroll wheel
/// turns into ↑/↓.
fn mouse_to_key(
    me: MouseEvent,
    term_height: u16,
    status: &[Shortcut<'_>],
) -> Option<KeyEvent> {
    match me.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The top row is now an informational title strip — no clicks.
            if me.row + 1 == term_height {
                return click_in_statusbar(me.column, status);
            }
            None
        }
        MouseEventKind::ScrollDown => {
            Some(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()))
        }
        MouseEventKind::ScrollUp => Some(KeyEvent::new(KeyCode::Up, KeyModifiers::empty())),
        _ => None,
    }
}

/// Status bar items: 1-space pad, then each item is `<key> <action>  ` —
/// click anywhere on the block to trigger the key.
fn click_in_statusbar(x: u16, status: &[Shortcut<'_>]) -> Option<KeyEvent> {
    let mut col: u16 = 1;
    for s in status {
        let key_len = s.key.chars().count() as u16;
        let action_len = s.action.chars().count() as u16;
        let block = key_len + 1 + action_len + 2;
        if x >= col && x < col + block {
            return shortcut_label_to_key(s.key);
        }
        col += block;
    }
    None
}

fn shortcut_label_to_key(label: &str) -> Option<KeyEvent> {
    let code = match label {
        "F1" => KeyCode::F(1),
        "F2" => KeyCode::F(2),
        "F3" => KeyCode::F(3),
        "F10" => KeyCode::F(10),
        "Q" => KeyCode::Char('q'),
        "Esc" => KeyCode::Esc,
        "Enter" => KeyCode::Enter,
        _ => return None,
    };
    Some(KeyEvent::new(code, KeyModifiers::empty()))
}
