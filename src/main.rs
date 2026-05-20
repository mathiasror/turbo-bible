mod bookmark;
mod config;
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
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use etcetera::{BaseStrategy, choose_base_strategy};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::db::{Book, Db, Passage};
use crate::keys::{Action, KeyState};
use crate::nav::{Navigator, Position};
use crate::ui::find::{FindDialog, FindOutcome};
use crate::ui::footnote::{FootnoteDialog, FootnoteOutcome};
use crate::ui::goto::{GotoCommand, GotoDialog, GotoOutcome};
use crate::ui::help::{HelpDialog, HelpOutcome};
use crate::ui::splash::{SplashOutcome, SplashView};
use crate::ui::statusbar::Shortcut;
use crate::ui::translations::{TranslationsDialog, TranslationsOutcome};

enum Bg {
    // SplashView carries three Vec<Book>-derived fields, the QOTD, two
    // translation strings, and chord/count state — ~280 bytes. Box the variant
    // so `Bg::Reading` (which is 95% of the loop's lifetime) doesn't pay for
    // it. Triggers clippy::large_enum_variant otherwise.
    Splash(Box<SplashView>),
    Reading,
}

/// What to seed the splash screen with on startup: the optional "Continue"
/// target (most recently read position + its label) plus the optional verse
/// of the day. None of these are required, but their tuple-of-options shape
/// was complex enough to trip clippy::type_complexity; the named struct also
/// reads better at the call site.
struct SplashSeed {
    last: Option<(Position, String)>,
    qotd: Option<crate::quote::DailyQuote>,
}

enum Dialog {
    None,
    Goto(GotoDialog),
    Find(FindDialog),
    Footnote(FootnoteDialog),
    Help(HelpDialog),
    Bookmarks(crate::ui::bookmarks::BookmarksDialog),
    Translations(TranslationsDialog),
}

struct History {
    stack: Vec<Position>,
    cur: usize,
}

impl History {
    fn new(initial: Position) -> Self {
        Self {
            stack: vec![initial],
            cur: 0,
        }
    }
    fn push(&mut self, p: Position) {
        self.stack.truncate(self.cur + 1);
        if self.stack.last().is_none_or(|last| !last.same_chapter(&p)) {
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
    /// Path to bible.sqlite. Defaults to `$XDG_DATA_HOME/turbo-bible/bible.sqlite`
    /// (i.e. `~/.local/share/turbo-bible/bible.sqlite` on Linux/macOS).
    #[arg(long)]
    db: Option<PathBuf>,

    /// Translation code. If omitted, falls back to the picker default
    /// stored in state.toml, then to the first translation in the DB.
    #[arg(long)]
    translation: Option<String>,

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
    let db_path = resolve_db_path(&args)?;
    match db::ensure_fts_optimized(&db_path) {
        Ok(true) => eprintln!("Optimizing search index (one-time)…"),
        Ok(false) => {}
        Err(e) => eprintln!("warning: FTS optimize skipped: {e}"),
    }
    // Non-fatal save failures collected here and replayed to stderr after
    // restore_terminal. Inside the TUI loop, eprintln would mangle the
    // alternate-screen display, so we defer.
    let mut warnings: Vec<String> = Vec::new();
    let (persisted, config) = state::load_with_migration();
    theme::init(config.theme.clone());
    let translation = resolve_translation(&args, &db_path, &config)?;
    // Save right away so the on-disk layout converges to the split form.
    save_or_warn(&mut warnings, "config save", config::save(&config));
    if let Some(ps) = &persisted {
        save_or_warn(&mut warnings, "state save", state::save(ps));
    }
    let mut db = Db::open_ro(&db_path, &translation)?;
    let books = db.list_books()?;
    let translation_label = db.translation_label()?;

    // Resolve persisted state for the Continue option.
    let last_for_splash: Option<(Position, String)> = persisted
        .as_ref()
        .filter(|ps| ps.translation == translation)
        .map(|ps| {
            let label = books
                .iter()
                .find(|b| b.code == ps.book)
                .map(|b| format!("{} {}:{}", b.name, ps.chapter, ps.verse))
                .unwrap_or_else(|| format!("{} {}:{}", ps.book, ps.chapter, ps.verse));
            (
                Position {
                    book: ps.book.clone(),
                    chapter: ps.chapter,
                    verse: None,
                },
                label,
            )
        });

    // Starting screen: if --book was passed explicitly, go straight to reading.
    let mut term = init_terminal()?;
    let final_pos: Option<Position>;
    let final_cursor_verse: i64;
    let result = if let Some(book_code) = args.book.clone() {
        let mut pos = Position {
            book: book_code,
            chapter: args.chapter,
            verse: None,
        };
        let mut passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = 1;
        let r = run(
            &mut term,
            &mut db,
            books,
            translation_label,
            &mut pos,
            &mut passage,
            &mut cursor_verse,
            None,
            &config,
            &mut warnings,
        );
        final_pos = Some(pos);
        final_cursor_verse = cursor_verse;
        r
    } else {
        let qotd = if config.reading.show_daily_quote {
            quote::pick(&db).unwrap_or(None)
        } else {
            None
        };
        // We still need *some* initial passage state for the run loop; load
        // the persisted-or-default position lazily-ish.
        let mut pos = match &last_for_splash {
            Some((p, _)) => p.clone(),
            None => Position {
                book: "GEN".into(),
                chapter: 1,
                verse: None,
            },
        };
        let mut passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = persisted.as_ref().map(|p| p.verse).unwrap_or(1).max(1);
        let r = run(
            &mut term,
            &mut db,
            books,
            translation_label,
            &mut pos,
            &mut passage,
            &mut cursor_verse,
            Some(SplashSeed {
                last: last_for_splash,
                qotd,
            }),
            &config,
            &mut warnings,
        );
        final_pos = Some(pos);
        final_cursor_verse = cursor_verse;
        r
    };
    restore_terminal(&mut term)?;

    if let Some(p) = final_pos {
        save_or_warn(
            &mut warnings,
            "state save",
            state::save(&state::PersistedState {
                translation: db.translation.clone(),
                book: p.book,
                chapter: p.chapter,
                verse: final_cursor_verse,
            }),
        );
        // The active translation at quit becomes the default for next launch.
        // The picker already persisted on click, but a no-picker session also
        // wants the current translation remembered.
        let mut cfg = config::load();
        cfg.default_translation = Some(db.translation.clone());
        save_or_warn(&mut warnings, "config save", config::save(&cfg));
    }
    // Replay deferred save warnings now that the alternate screen is gone.
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    result
}

/// Push a one-line message into `out` when `r` is an error, otherwise no-op.
/// The collector pattern keeps in-TUI failures from mangling the
/// alternate-screen display — they get printed after `restore_terminal`.
fn save_or_warn<T>(out: &mut Vec<String>, what: &str, r: anyhow::Result<T>) {
    if let Err(e) = r {
        out.push(format!("{what} failed: {e:#}"));
    }
}

/// Resolve the DB path: explicit `--db` flag wins; otherwise
/// `$XDG_DATA_HOME/turbo-bible/bible.sqlite` (typically `~/.local/share/...`).
fn resolve_db_path(args: &Args) -> Result<PathBuf> {
    if let Some(p) = args.db.clone() {
        return Ok(p);
    }
    let strategy = choose_base_strategy()?;
    let mut p = strategy.data_dir();
    p.push("turbo-bible");
    p.push("bible.sqlite");
    Ok(p)
}

/// Startup translation resolution: `--translation` > config default > first DB row.
fn resolve_translation(args: &Args, db_path: &Path, cfg: &config::Config) -> Result<String> {
    if let Some(t) = args.translation.as_ref() {
        return Ok(t.clone());
    }
    if let Some(t) = cfg.default_translation.clone() {
        return Ok(t);
    }
    // Probe the DB for the first installed translation.
    if !db_path.exists() {
        anyhow::bail!(
            "{} does not exist. Run `python3 scripts/import_translations.py` to create it.",
            db_path.display()
        );
    }
    let probe = Db::open_ro(db_path, "")?;
    let mut list = probe.list_translations()?;
    if list.is_empty() {
        anyhow::bail!(
            "No translations installed in {}. Run scripts/import_translations.py first.",
            db_path.display()
        );
    }
    Ok(list.remove(0).code)
}

#[allow(clippy::too_many_arguments)]
fn run(
    term: &mut Tty,
    db: &mut Db,
    mut books: Vec<Book>,
    mut translation_label: String,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    initial_splash: Option<SplashSeed>,
    config: &config::Config,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut keys = KeyState::with_user_bindings(&config.keys, config.input.keymap);
    // Last `/`-search query — populated whenever the Find dialog produces a
    // jump, consumed by `n` / `N` to step canonically through the hits.
    let mut last_query: Option<String> = None;
    let mut history = History::new(pos.clone());
    let mut bg = match initial_splash {
        Some(seed) => Bg::Splash(Box::new(SplashView::new(
            books.clone(),
            seed.last,
            translation_label.clone(),
            db.translation.clone(),
            seed.qotd,
        ))),
        None => Bg::Reading,
    };
    let mut dialog: Dialog = Dialog::None;
    let mut show_sidebar = config.reading.show_sidebar;
    let max_reading_width = config.reading.max_width;
    let mut visual_anchor: Option<i64> = None;
    let mut bookmarks = bookmark::BookmarkStore::load();
    // Persist the migrated bookmarks immediately so the file on disk is in the
    // new TOML format with translation rewritten — survives a crash before any
    // user action triggers another save.
    save_or_warn(
        warnings,
        "bookmarks save (post-migration)",
        bookmarks.save(),
    );
    let mut verse_layout_two_line = config.reading.two_line_verses;
    let mut last_label_for_splash: Option<(Position, String)> =
        books.iter().find(|b| b.code == pos.book).map(|b| {
            (
                pos.clone(),
                format!("{} {}:{}", b.name, pos.chapter, *cursor_verse),
            )
        });

    loop {
        let status = make_status(&bg, show_sidebar);
        let bookmarked_in_chapter = bookmarks_set(&bookmarks, &db.translation, pos);
        let menu_title = format!(" Turbo Bible \u{00B7} {} ", translation_label);
        term.draw(|f| {
            let area = f.area();
            let buf = f.buffer_mut();
            match &bg {
                Bg::Splash(s) => {
                    // Plain blue desktop behind the splash window.
                    crate::ui::desktop::render(
                        ratatui::layout::Rect::new(
                            area.x,
                            area.y + 1,
                            area.width,
                            area.height.saturating_sub(2),
                        ),
                        buf,
                    );
                    crate::ui::menubar::render(
                        &menu_title,
                        ratatui::layout::Rect::new(area.x, area.y, area.width, 1),
                        buf,
                    );
                    let mode_tag =
                        mode_tag_for(&bg, &dialog, visual_anchor.is_some(), verse_layout_two_line);
                    crate::ui::statusbar::render(
                        &status,
                        ratatui::layout::Rect::new(area.x, area.y + area.height - 1, area.width, 1),
                        buf,
                        &mode_tag,
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
                    let mode_tag =
                        mode_tag_for(&bg, &dialog, visual_anchor.is_some(), verse_layout_two_line);
                    let selection = visual_anchor.map(|a| {
                        let c = *cursor_verse;
                        if a <= c { (a, c) } else { (c, a) }
                    });
                    ui::Frame {
                        menu_title: &menu_title,
                        status: &status,
                        status_mode: &mode_tag,
                        passage: Some(passage),
                        cursor_verse: *cursor_verse,
                        selection,
                        bookmarked: &bookmarked_in_chapter,
                        show_sidebar,
                        two_line_verses: verse_layout_two_line,
                        max_reading_width,
                    }
                    .render(area, buf);
                }
            }
            match &dialog {
                Dialog::None => {}
                Dialog::Goto(d) => d.render(area, buf, &books),
                Dialog::Find(d) => d.render(area, buf, &books),
                Dialog::Footnote(d) => d.render(area, buf, &books),
                Dialog::Help(d) => d.render(area, buf),
                Dialog::Bookmarks(d) => d.render(area, buf, &books),
                Dialog::Translations(d) => d.render(area, buf),
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
                        match d.handle(key, &books) {
                            GotoOutcome::Continue => {}
                            GotoOutcome::Cancel => dialog = Dialog::None,
                            GotoOutcome::Jump(p) => {
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(
                                    &mut last_label_for_splash,
                                    &books,
                                    pos,
                                    *cursor_verse,
                                );
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
                            FindOutcome::Jump(p, q) => {
                                last_query = Some(q);
                                jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                update_splash_label(
                                    &mut last_label_for_splash,
                                    &books,
                                    pos,
                                    *cursor_verse,
                                );
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
                                update_splash_label(
                                    &mut last_label_for_splash,
                                    &books,
                                    pos,
                                    *cursor_verse,
                                );
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
                                    &books,
                                    pos,
                                    *cursor_verse,
                                );
                                bg = Bg::Reading;
                                dialog = Dialog::None;
                            }
                            BookmarksOutcome::Delete(bm) => {
                                bookmarks.bookmarks.retain(|b| !b.same_range(&bm));
                                save_or_warn(warnings, "bookmarks save (delete)", bookmarks.save());
                            }
                        }
                        continue;
                    }
                    Dialog::Translations(d) => {
                        match d.handle(key) {
                            TranslationsOutcome::Continue => {}
                            TranslationsOutcome::Cancel => dialog = Dialog::None,
                            TranslationsOutcome::Select(code) => {
                                switch_translation(
                                    db,
                                    &mut books,
                                    &mut translation_label,
                                    &code,
                                    pos,
                                    passage,
                                    cursor_verse,
                                )?;
                                save_or_warn(
                                    warnings,
                                    "default-translation persist",
                                    persist_default_translation(&code),
                                );
                                update_splash_label(
                                    &mut last_label_for_splash,
                                    &books,
                                    pos,
                                    *cursor_verse,
                                );
                                dialog = Dialog::None;
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
                                &books,
                                pos,
                                *cursor_verse,
                            );
                            bg = Bg::Reading;
                        }
                        SplashOutcome::OpenTranslations => {
                            dialog = Dialog::Translations(TranslationsDialog::new(
                                db.list_translations()?,
                                &db.translation,
                            ));
                        }
                    },
                    Bg::Reading => {
                        if let Some(action) = keys.handle(key) {
                            match action {
                                Action::OpenGoto => dialog = Dialog::Goto(GotoDialog::new()),
                                Action::OpenFind => dialog = Dialog::Find(FindDialog::new()),
                                Action::OpenHelp => dialog = Dialog::Help(HelpDialog::new()),
                                Action::OpenFootnote => {
                                    let target =
                                        format!("{}.{}.{}", pos.book, pos.chapter, *cursor_verse);
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
                                Action::ToggleVerseLayout => {
                                    verse_layout_two_line = !verse_layout_two_line
                                }
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
                                    save_or_warn(
                                        warnings,
                                        "bookmarks save (add)",
                                        bookmarks.save(),
                                    );
                                    visual_anchor = None;
                                }
                                Action::OpenBookmarks => {
                                    let mut d =
                                        crate::ui::bookmarks::BookmarksDialog::new(&bookmarks);
                                    d.sort_canonical(&books);
                                    dialog = Dialog::Bookmarks(d);
                                }
                                Action::OpenTranslations => {
                                    dialog = Dialog::Translations(TranslationsDialog::new(
                                        db.list_translations()?,
                                        &db.translation,
                                    ));
                                }
                                Action::Back => {
                                    // In visual mode, Esc cancels the selection
                                    // instead of returning to splash.
                                    if visual_anchor.is_some() {
                                        visual_anchor = None;
                                    } else {
                                        update_splash_label(
                                            &mut last_label_for_splash,
                                            &books,
                                            pos,
                                            *cursor_verse,
                                        );
                                        let qotd = quote::pick(db).unwrap_or(None);
                                        bg = Bg::Splash(Box::new(SplashView::new(
                                            books.clone(),
                                            last_label_for_splash.clone(),
                                            translation_label.clone(),
                                            db.translation.clone(),
                                            qotd,
                                        )));
                                    }
                                }
                                Action::Quit => return Ok(()),
                                Action::SearchNext | Action::SearchPrev => {
                                    if let Some(q) = last_query.as_deref()
                                        && let Some(p) = repeat_search(
                                            db,
                                            &books,
                                            q,
                                            pos,
                                            *cursor_verse,
                                            matches!(action, Action::SearchNext),
                                        )
                                    {
                                        jump_to(p, db, pos, passage, cursor_verse, &mut history)?;
                                        update_splash_label(
                                            &mut last_label_for_splash,
                                            &books,
                                            pos,
                                            *cursor_verse,
                                        );
                                    }
                                }
                                _ => {
                                    if apply_action(
                                        action,
                                        db,
                                        &books,
                                        pos,
                                        passage,
                                        cursor_verse,
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

fn mode_tag_for(bg: &Bg, dialog: &Dialog, visual: bool, two_line: bool) -> String {
    match dialog {
        Dialog::Goto(_) => "-- GOTO --".into(),
        Dialog::Find(_) => "-- FIND --".into(),
        Dialog::Footnote(_) => "-- NOTES --".into(),
        Dialog::Help(_) => "-- HELP --".into(),
        Dialog::Bookmarks(_) => "-- BOOKMARKS --".into(),
        Dialog::Translations(_) => "-- TRANSLATIONS --".into(),
        Dialog::None => match bg {
            Bg::Splash(s) => match s.mode {
                crate::ui::splash::SplashMode::Normal => "-- NORMAL --".into(),
                crate::ui::splash::SplashMode::Filter => "-- FILTER --".into(),
            },
            // Reading view: include the verse-layout marker so the user can
            // tell 1L from 2L without counting blank lines between verses.
            Bg::Reading => {
                let layout = if two_line { "2L" } else { "1L" };
                if visual {
                    format!("-- VISUAL \u{00B7} {layout} --")
                } else {
                    format!("-- NORMAL \u{00B7} {layout} --")
                }
            }
        },
    }
}

fn make_status(bg: &Bg, show_sidebar: bool) -> Vec<Shortcut<'static>> {
    match bg {
        Bg::Splash(_) => vec![
            Shortcut {
                key: "Enter",
                action: "Open",
            },
            Shortcut {
                key: "F2",
                action: "Goto",
            },
            Shortcut {
                key: "F3",
                action: "Find",
            },
            Shortcut {
                key: "Esc",
                action: "Quit",
            },
        ],
        Bg::Reading => vec![
            Shortcut {
                key: "F1",
                action: "Help",
            },
            Shortcut {
                key: "F2",
                action: "Goto",
            },
            Shortcut {
                key: "F3",
                action: "Find",
            },
            Shortcut {
                key: "K",
                action: "Notes",
            },
            Shortcut {
                key: "v",
                action: "Select",
            },
            Shortcut {
                key: "T",
                action: "Layout",
            },
            Shortcut {
                key: "Tab",
                action: if show_sidebar { "Hide" } else { "Refs" },
            },
            Shortcut {
                key: "Esc",
                action: "Home",
            },
            Shortcut {
                key: "Q",
                action: "Quit",
            },
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
    let target_verse = p.verse;
    *pos = p;
    *passage = db.load_passage(&pos.book, pos.chapter)?;
    // Find / Bookmarks / `:John 3:16` set p.verse so the cursor lands on the
    // match instead of always snapping to verse 1. Clamp to the passage size.
    let max = passage.verses.last().map(|v| v.number).unwrap_or(1);
    *cursor_verse = target_verse.unwrap_or(1).clamp(1, max);
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
    books: &[Book],
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    history: &mut History,
) -> Result<bool> {
    let nav_ = Navigator::new(books);
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
        | Action::OpenTranslations
        | Action::ToggleVerseLayout
        | Action::SearchNext
        | Action::SearchPrev => Ok(false),
    }
}

/// Repeat the last `/`-search. Runs the query, sorts hits canonically
/// (book canon, chapter, verse), and returns the next or previous hit
/// relative to `(pos, cursor_verse)`. Wraps around when the end is reached,
/// matching vim's default `wrapscan` behavior. `None` when the query yields
/// no hits at all (or only the hit at the current verse).
fn repeat_search(
    db: &Db,
    books: &[Book],
    query: &str,
    pos: &Position,
    cursor_verse: i64,
    forward: bool,
) -> Option<Position> {
    let mut hits = search::search(db, query, 1000).ok()?;
    if hits.is_empty() {
        return None;
    }
    let canon: std::collections::HashMap<&str, usize> = books
        .iter()
        .enumerate()
        .map(|(i, b)| (b.code.as_str(), i))
        .collect();
    let key = |book: &str, ch: i64, v: i64| -> (usize, i64, i64) {
        (canon.get(book).copied().unwrap_or(usize::MAX), ch, v)
    };
    hits.sort_by_key(|h| key(&h.book, h.chapter, h.verse));
    let here = key(&pos.book, pos.chapter, cursor_verse);
    let pick = if forward {
        hits.iter()
            .find(|h| key(&h.book, h.chapter, h.verse) > here)
            .or_else(|| hits.first())
    } else {
        hits.iter()
            .rev()
            .find(|h| key(&h.book, h.chapter, h.verse) < here)
            .or_else(|| hits.last())
    };
    pick.map(|h| Position {
        book: h.book.clone(),
        chapter: h.chapter,
        verse: Some(h.verse),
    })
}

fn switch_translation(
    db: &mut Db,
    books: &mut Vec<Book>,
    translation_label: &mut String,
    code: &str,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
) -> Result<()> {
    db.translation = code.to_string();
    *books = db.list_books()?;
    *translation_label = db.translation_label()?;
    *passage = db.load_passage(&pos.book, pos.chapter)?;
    // Clamp the cursor — a different translation may have fewer verses for
    // this chapter (rare in our three editions, but defensive).
    let max = passage.verses.last().map(|v| v.number).unwrap_or(1);
    if *cursor_verse > max {
        *cursor_verse = max.max(1);
    }
    Ok(())
}

fn persist_default_translation(code: &str) -> Result<()> {
    let mut cfg = config::load();
    cfg.default_translation = Some(code.to_string());
    config::save(&cfg)
}

fn init_terminal() -> Result<Tty> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(out))?)
}

fn restore_terminal(term: &mut Tty) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        term.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    term.show_cursor()?;
    Ok(())
}

/// Translate a mouse event into a synthetic key event so clicks on the
/// menubar / statusbar reuse the existing keyboard dispatch path. Scroll wheel
/// turns into ↑/↓.
fn mouse_to_key(me: MouseEvent, term_height: u16, status: &[Shortcut<'_>]) -> Option<KeyEvent> {
    match me.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The top row is now an informational title strip — no clicks.
            if me.row + 1 == term_height {
                return click_in_statusbar(me.column, status);
            }
            None
        }
        MouseEventKind::ScrollDown => Some(KeyEvent::new(KeyCode::Down, KeyModifiers::empty())),
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
