//! `turbo-bible` — a Turbo Vision–styled terminal Bible reader with
//! FTS5 search.
//!
//! This crate is a single binary. See [`README.md`] for the user-facing
//! tour and [`docs/USAGE.md`] for a feature walk-through; the source
//! tree mirrors the README's "Layout" section.
//!
//! [`README.md`]: https://github.com/rorvikxyz/turbo-bible/blob/main/README.md
//! [`docs/USAGE.md`]: https://github.com/rorvikxyz/turbo-bible/blob/main/docs/USAGE.md
#![deny(unsafe_code)]

mod bookmark;
mod config;
mod db;
mod import;
mod keys;
mod nav;
mod paths;
mod quote;
mod render;
mod search;
mod state;
mod text;
mod theme;
mod ui;

use std::borrow::Cow;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
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
/// was complex enough to trip `clippy::type_complexity`; the named struct also
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

/// Upper bound on the jump-history stack. Long reading sessions
/// shouldn't grow memory unbounded; 100 entries covers typical Ctrl-O/I
/// usage with room to spare.
const HISTORY_CAP: usize = 100;

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
            if self.stack.len() > HISTORY_CAP {
                let drop = self.stack.len() - HISTORY_CAP;
                self.stack.drain(..drop);
                self.cur = self.stack.len().saturating_sub(1);
            } else {
                self.cur = self.stack.len() - 1;
            }
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
#[command(
    version,
    about = "Turbo-Vision Bible reader",
    args_conflicts_with_subcommands = true
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

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

#[derive(Subcommand, Debug)]
enum Commands {
    /// Download translations from scrollmapper and (re)build the local DB.
    Import(import::ImportArgs),
}

type Tty = Terminal<CrosstermBackend<Stdout>>;

/// Mutable state owned by the run loop but threaded through the
/// extracted dispatch helpers. Separating this from the externally-owned
/// reader state (`AppCtx`) keeps method signatures short and lets the
/// dispatch helpers be free functions.
struct LoopState {
    books: Vec<Book>,
    translation_label: String,
    bg: Bg,
    dialog: Dialog,
    history: History,
    bookmarks: bookmark::BookmarkStore,
    last_query: Option<String>,
    last_label_for_splash: Option<(Position, String)>,
    visual_anchor: Option<i64>,
    show_sidebar: bool,
    max_reading_width: u16,
    keys: KeyState,
}

/// Borrowed bundle of the externally-owned reader state. Built fresh
/// per dispatch call so the extracted handlers don't need to take six
/// separate `&mut` parameters.
struct AppCtx<'a> {
    db: &'a mut Db,
    pos: &'a mut Position,
    passage: &'a mut Passage,
    cursor_verse: &'a mut i64,
    warnings: &'a mut Vec<String>,
}

/// Outcome of a per-key dispatch call. `Quit` ends the loop; `Continue`
/// keeps going (regardless of whether the key was consumed).
enum DispatchStep {
    Continue,
    Quit,
}

/// RAII handle for the terminal's raw-mode + alternate-screen state.
/// Restores the terminal on drop, so a panic between `init` and the
/// normal end-of-`run()` cleanup still leaves the user with a sane
/// shell instead of a corrupted display.
struct TerminalGuard {
    term: Tty,
    active: bool,
}

impl TerminalGuard {
    fn init() -> Result<Self> {
        enable_raw_mode()?;
        let inner = || -> Result<Tty> {
            let mut out = io::stdout();
            execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
            Ok(Terminal::new(CrosstermBackend::new(out))?)
        };
        match inner() {
            Ok(term) => Ok(Self { term, active: true }),
            Err(e) => {
                // Roll back raw mode so a partial init (e.g. EnterAlternateScreen
                // fails) doesn't leave the user's shell in cooked-off mode.
                // LeaveAlternateScreen is best-effort: harmless when we never
                // entered, and the alt-screen is what we'd want to leave on the
                // post-EnterAlternateScreen failure path.
                let mut out = io::stdout();
                let _ = execute!(out, DisableMouseCapture, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(e)
            }
        }
    }

    const fn terminal(&mut self) -> &mut Tty {
        &mut self.term
    }

    /// Explicit, ordered cleanup so the surrounding code can react to
    /// errors. Drop also calls this with errors swallowed.
    fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        disable_raw_mode()?;
        execute!(
            self.term.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        self.term.show_cursor()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort cleanup on panic / early exit. We can't propagate
        // errors out of Drop; if restore failed at the explicit call
        // site the user already saw that diagnostic.
        let _ = self.restore();
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "binary entry point assembles all the loop-local state in one \
              place; lifting any block into a helper would just move the \
              length up one frame without making the assembly clearer."
)]
fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(Commands::Import(import_args)) = &args.command {
        return import::run(import_args);
    }
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
            let label = books.iter().find(|b| b.code == ps.book).map_or_else(
                || format!("{} {}:{}", ps.book, ps.chapter, ps.verse),
                |b| format!("{} {}:{}", b.name, ps.chapter, ps.verse),
            );
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
    let mut guard = TerminalGuard::init()?;
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
            guard.terminal(),
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
            quote::pick(&db, db.translation()).unwrap_or(None)
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
        let mut cursor_verse: i64 = persisted.as_ref().map_or(1, |p| p.verse).max(1);
        let r = run(
            guard.terminal(),
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
    guard.restore()?;

    if let Some(p) = final_pos {
        save_or_warn(
            &mut warnings,
            "state save",
            state::save(&state::PersistedState {
                translation: db.translation().to_string(),
                book: p.book,
                chapter: p.chapter,
                verse: final_cursor_verse,
            }),
        );
        // The active translation at quit becomes the default for next launch.
        // The picker already persisted on click, but a no-picker session also
        // wants the current translation remembered.
        let mut cfg = config::load();
        cfg.default_translation = Some(db.translation().to_string());
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
    let mut p = paths::data_dir()?;
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
            "{} does not exist. Run `turbo-bible import` to create it.",
            db_path.display()
        );
    }
    let mut list = db::list_translations(db_path)?;
    if list.is_empty() {
        anyhow::bail!(
            "No translations installed in {}. Run `turbo-bible import` first.",
            db_path.display()
        );
    }
    Ok(list.remove(0).code)
}

#[allow(
    clippy::too_many_arguments,
    reason = "wired from `main()` which constructs all the loop-local state; \
              bundling into a struct would just move the long signature \
              up one level"
)]
fn run(
    term: &mut Tty,
    db: &mut Db,
    books: Vec<Book>,
    translation_label: String,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    initial_splash: Option<SplashSeed>,
    config: &config::Config,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut state = LoopState::new(
        books,
        translation_label,
        pos,
        *cursor_verse,
        initial_splash,
        db.translation(),
        config,
        warnings,
    );

    loop {
        draw_frame(term, &state, passage, *cursor_verse)?;

        if event::poll(Duration::from_millis(150))? {
            let term_height = term.size().map_or(24, |s| s.height);
            let raw_event = event::read()?;
            let synth: Option<KeyEvent> = match raw_event {
                Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
                Event::Mouse(me) => {
                    mouse_to_key(me, term_height, make_status(&state.bg, state.show_sidebar))
                }
                _ => None,
            };
            if let Some(key) = synth {
                let mut ctx = AppCtx {
                    db,
                    pos,
                    passage,
                    cursor_verse,
                    warnings,
                };
                let step = dispatch_key(&mut state, &mut ctx, key)?;
                if matches!(step, DispatchStep::Quit) {
                    return Ok(());
                }
            }
        } else {
            state.keys.tick();
        }
    }
}

impl LoopState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        books: Vec<Book>,
        translation_label: String,
        pos: &Position,
        cursor_verse: i64,
        initial_splash: Option<SplashSeed>,
        translation: &str,
        config: &config::Config,
        warnings: &mut Vec<String>,
    ) -> Self {
        let keys = KeyState::with_user_bindings(&config.keys, config.input.keymap);
        let history = History::new(pos.clone());
        let bg = match initial_splash {
            Some(seed) => Bg::Splash(Box::new(SplashView::new(
                books.clone(),
                seed.last,
                translation_label.clone(),
                translation.to_string(),
                seed.qotd,
            ))),
            None => Bg::Reading,
        };
        let bookmarks = bookmark::BookmarkStore::load();
        // Persist the migrated bookmarks immediately so the file on disk is
        // in the new TOML format with translation rewritten — survives a
        // crash before any user action triggers another save.
        save_or_warn(
            warnings,
            "bookmarks save (post-migration)",
            bookmarks.save(),
        );
        let last_label_for_splash: Option<(Position, String)> =
            books.iter().find(|b| b.code == pos.book).map(|b| {
                (
                    pos.clone(),
                    format!("{} {}:{}", b.name, pos.chapter, cursor_verse),
                )
            });
        Self {
            books,
            translation_label,
            bg,
            dialog: Dialog::None,
            history,
            bookmarks,
            last_query: None,
            last_label_for_splash,
            visual_anchor: None,
            show_sidebar: config.reading.show_sidebar,
            max_reading_width: config.reading.max_width,
            keys,
        }
    }
}

/// One pass of the draw cycle. Kept inline (vs split into per-bg
/// helpers) because the closure borrows many fields and pulling it apart
/// duplicates the dialog overlay match.
fn draw_frame(
    term: &mut Tty,
    state: &LoopState,
    passage: &Passage,
    cursor_verse: i64,
) -> Result<()> {
    let status = make_status(&state.bg, state.show_sidebar);
    let bookmarked_in_chapter = bookmarks_set(
        &state.bookmarks,
        &passage.translation,
        &Position {
            book: passage.book_code.clone(),
            chapter: passage.chapter,
            verse: None,
        },
    );
    let menu_title = format!(" Turbo Bible \u{00B7} {} ", state.translation_label);
    term.draw(|f| {
        let area = f.area();
        let buf = f.buffer_mut();
        match &state.bg {
            Bg::Splash(s) => {
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
                    mode_tag_for(&state.bg, &state.dialog, state.visual_anchor.is_some());
                crate::ui::statusbar::render(
                    status,
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
                    mode_tag_for(&state.bg, &state.dialog, state.visual_anchor.is_some());
                let selection = state.visual_anchor.map(|a| {
                    let c = cursor_verse;
                    if a <= c { (a, c) } else { (c, a) }
                });
                ui::Frame {
                    menu_title: &menu_title,
                    status,
                    status_mode: &mode_tag,
                    passage,
                    cursor_verse,
                    selection,
                    bookmarked: &bookmarked_in_chapter,
                    show_sidebar: state.show_sidebar,
                    max_reading_width: state.max_reading_width,
                }
                .render(area, buf);
            }
        }
        match &state.dialog {
            Dialog::None => {}
            Dialog::Goto(d) => d.render(area, buf, &state.books),
            Dialog::Find(d) => d.render(area, buf, &state.books),
            Dialog::Footnote(d) => d.render(area, buf),
            Dialog::Help(_) => HelpDialog::render(area, buf),
            Dialog::Bookmarks(d) => d.render(area, buf, &state.books),
            Dialog::Translations(d) => d.render(area, buf),
        }
    })?;
    Ok(())
}

/// Route a key event: dialog has first refusal, then the active
/// background. Returns `Quit` only when the user asked to leave.
fn dispatch_key(state: &mut LoopState, ctx: &mut AppCtx, key: KeyEvent) -> Result<DispatchStep> {
    if !matches!(state.dialog, Dialog::None) {
        return dispatch_dialog(state, ctx, key);
    }
    match &mut state.bg {
        Bg::Splash(_) => dispatch_splash(state, ctx, key),
        Bg::Reading => dispatch_reading(state, ctx, key),
    }
}

/// Common dialog-close-after-jump path: load the new passage, push to
/// history, refresh the splash "Continue" label, and reset bg+dialog.
fn close_with_jump(state: &mut LoopState, ctx: &mut AppCtx, p: Position) -> Result<()> {
    jump_to(
        p,
        ctx.db,
        ctx.pos,
        ctx.passage,
        ctx.cursor_verse,
        &mut state.history,
    )?;
    update_splash_label(
        &mut state.last_label_for_splash,
        &state.books,
        ctx.pos,
        *ctx.cursor_verse,
    );
    state.bg = Bg::Reading;
    state.dialog = Dialog::None;
    Ok(())
}

fn dispatch_dialog(state: &mut LoopState, ctx: &mut AppCtx, key: KeyEvent) -> Result<DispatchStep> {
    match &mut state.dialog {
        Dialog::None => Ok(DispatchStep::Continue),
        Dialog::Goto(d) => match d.handle(key, &state.books) {
            GotoOutcome::Continue => Ok(DispatchStep::Continue),
            GotoOutcome::Cancel => {
                state.dialog = Dialog::None;
                Ok(DispatchStep::Continue)
            }
            GotoOutcome::Jump(p) => {
                close_with_jump(state, ctx, p)?;
                Ok(DispatchStep::Continue)
            }
            GotoOutcome::Command(GotoCommand::Quit) => Ok(DispatchStep::Quit),
            GotoOutcome::Command(GotoCommand::Help) => {
                state.dialog = Dialog::Help(HelpDialog::new());
                Ok(DispatchStep::Continue)
            }
        },
        Dialog::Find(d) => match d.handle(key, ctx.db) {
            FindOutcome::Continue => Ok(DispatchStep::Continue),
            FindOutcome::Cancel => {
                state.dialog = Dialog::None;
                Ok(DispatchStep::Continue)
            }
            FindOutcome::Jump(p, q) => {
                state.last_query = Some(q);
                close_with_jump(state, ctx, p)?;
                Ok(DispatchStep::Continue)
            }
        },
        Dialog::Footnote(d) => match d.handle(key) {
            FootnoteOutcome::Continue => Ok(DispatchStep::Continue),
            FootnoteOutcome::Cancel => {
                state.dialog = Dialog::None;
                Ok(DispatchStep::Continue)
            }
            FootnoteOutcome::Jump(p) => {
                close_with_jump(state, ctx, p)?;
                Ok(DispatchStep::Continue)
            }
        },
        Dialog::Help(_) => {
            if matches!(HelpDialog::handle(key), HelpOutcome::Cancel) {
                state.dialog = Dialog::None;
            }
            Ok(DispatchStep::Continue)
        }
        Dialog::Bookmarks(d) => {
            use crate::ui::bookmarks::BookmarksOutcome;
            match d.handle(key) {
                BookmarksOutcome::Continue => {}
                BookmarksOutcome::Cancel => state.dialog = Dialog::None,
                BookmarksOutcome::Jump(p) => close_with_jump(state, ctx, p)?,
                BookmarksOutcome::Delete(bm) => {
                    state.bookmarks.bookmarks.retain(|b| !b.same_range(&bm));
                    save_or_warn(
                        ctx.warnings,
                        "bookmarks save (delete)",
                        state.bookmarks.save(),
                    );
                }
            }
            Ok(DispatchStep::Continue)
        }
        Dialog::Translations(d) => {
            match d.handle(key) {
                TranslationsOutcome::Continue => {}
                TranslationsOutcome::Cancel => state.dialog = Dialog::None,
                TranslationsOutcome::Select(code) => {
                    switch_translation(
                        ctx.db,
                        &mut state.books,
                        &mut state.translation_label,
                        &code,
                        ctx.pos,
                        ctx.passage,
                        ctx.cursor_verse,
                    )?;
                    save_or_warn(
                        ctx.warnings,
                        "default-translation persist",
                        persist_default_translation(&code),
                    );
                    update_splash_label(
                        &mut state.last_label_for_splash,
                        &state.books,
                        ctx.pos,
                        *ctx.cursor_verse,
                    );
                    state.dialog = Dialog::None;
                }
            }
            Ok(DispatchStep::Continue)
        }
    }
}

fn dispatch_splash(state: &mut LoopState, ctx: &mut AppCtx, key: KeyEvent) -> Result<DispatchStep> {
    let outcome = if let Bg::Splash(s) = &mut state.bg {
        s.handle(key)
    } else {
        return Ok(DispatchStep::Continue);
    };
    match outcome {
        SplashOutcome::Continue => Ok(DispatchStep::Continue),
        SplashOutcome::Quit => Ok(DispatchStep::Quit),
        SplashOutcome::OpenGoto => {
            state.dialog = Dialog::Goto(GotoDialog::new());
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenFind => {
            state.dialog = Dialog::Find(FindDialog::new());
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenBook(p) => {
            jump_to(
                p,
                ctx.db,
                ctx.pos,
                ctx.passage,
                ctx.cursor_verse,
                &mut state.history,
            )?;
            update_splash_label(
                &mut state.last_label_for_splash,
                &state.books,
                ctx.pos,
                *ctx.cursor_verse,
            );
            state.bg = Bg::Reading;
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenTranslations => {
            state.dialog = Dialog::Translations(TranslationsDialog::new(
                ctx.db.list_translations()?,
                ctx.db.translation(),
            ));
            Ok(DispatchStep::Continue)
        }
    }
}

/// Direction parameter for [`LoopState::history_step`]. Internal sugar so
/// `JumpBack` / `JumpForward` share one implementation. `Copy` so the
/// caller can pass it by value without `clippy::needless_pass_by_value`.
#[derive(Debug, Clone, Copy)]
enum HistoryDir {
    Back,
    Forward,
}

impl LoopState {
    fn open_footnote_dialog(&mut self, ctx: &AppCtx) {
        let target = format!("{}.{}.{}", ctx.pos.book, ctx.pos.chapter, *ctx.cursor_verse);
        let notes: Vec<_> = ctx
            .passage
            .footnotes
            .iter()
            .filter(|fn_| fn_.verse_osis == target)
            .cloned()
            .collect();
        let xrefs: Vec<_> = ctx
            .passage
            .xrefs
            .iter()
            .filter(|x| x.from_verse == *ctx.cursor_verse)
            .cloned()
            .collect();
        let label = format!(
            "{} {}:{}",
            ctx.passage.book_abbrev, ctx.pos.chapter, *ctx.cursor_verse
        );
        self.dialog = Dialog::Footnote(FootnoteDialog::new(label, notes, xrefs));
    }

    fn history_step(&mut self, ctx: &mut AppCtx, dir: HistoryDir) -> Result<()> {
        let target = match dir {
            HistoryDir::Back => self.history.back(),
            HistoryDir::Forward => self.history.forward(),
        };
        if let Some(p) = target {
            *ctx.pos = p;
            *ctx.passage = ctx.db.load_passage(&ctx.pos.book, ctx.pos.chapter)?;
            *ctx.cursor_verse = 1;
        }
        Ok(())
    }

    fn copy_verse(ctx: &mut AppCtx) {
        save_or_warn(
            ctx.warnings,
            "clipboard set",
            copy_verse_to_clipboard(ctx.passage, ctx.pos, *ctx.cursor_verse),
        );
    }

    const fn toggle_visual(&mut self, cursor: i64) {
        self.visual_anchor = if self.visual_anchor.is_some() {
            None
        } else {
            Some(cursor)
        };
    }

    fn add_bookmark(&mut self, ctx: &mut AppCtx) {
        let (s, e) = match self.visual_anchor {
            Some(a) if a <= *ctx.cursor_verse => (a, *ctx.cursor_verse),
            Some(a) => (*ctx.cursor_verse, a),
            None => (*ctx.cursor_verse, *ctx.cursor_verse),
        };
        self.bookmarks.add(bookmark::Bookmark {
            translation: ctx.db.translation().to_string(),
            book: ctx.pos.book.clone(),
            chapter: ctx.pos.chapter,
            start_verse: s,
            end_verse: e,
            label: None,
            created_at: bookmark::now_unix(),
        });
        save_or_warn(ctx.warnings, "bookmarks save (add)", self.bookmarks.save());
        self.visual_anchor = None;
    }

    fn open_bookmarks_dialog(&mut self) {
        let mut d = crate::ui::bookmarks::BookmarksDialog::new(&self.bookmarks);
        d.sort_canonical(&self.books);
        self.dialog = Dialog::Bookmarks(d);
    }

    fn open_translations_dialog(&mut self, ctx: &AppCtx) -> Result<()> {
        self.dialog = Dialog::Translations(TranslationsDialog::new(
            ctx.db.list_translations()?,
            ctx.db.translation(),
        ));
        Ok(())
    }

    /// Esc-from-reading: cancel visual selection if active, otherwise
    /// rebuild the splash view and switch the background to it.
    fn enter_splash(&mut self, ctx: &AppCtx) {
        if self.visual_anchor.is_some() {
            self.visual_anchor = None;
            return;
        }
        update_splash_label(
            &mut self.last_label_for_splash,
            &self.books,
            ctx.pos,
            *ctx.cursor_verse,
        );
        let qotd = quote::pick(ctx.db, ctx.db.translation()).unwrap_or(None);
        self.bg = Bg::Splash(Box::new(SplashView::new(
            self.books.clone(),
            self.last_label_for_splash.clone(),
            self.translation_label.clone(),
            ctx.db.translation().to_string(),
            qotd,
        )));
    }

    /// Re-run the most recent `/`-search in `forward` or backward order
    /// relative to the cursor, jumping to the next hit (with wrap).
    fn repeat_search_action(&mut self, ctx: &mut AppCtx, forward: bool) -> Result<()> {
        let Some(q) = self.last_query.as_deref() else {
            return Ok(());
        };
        let Some(p) = repeat_search(ctx.db, &self.books, q, ctx.pos, *ctx.cursor_verse, forward)
        else {
            return Ok(());
        };
        jump_to(
            p,
            ctx.db,
            ctx.pos,
            ctx.passage,
            ctx.cursor_verse,
            &mut self.history,
        )?;
        update_splash_label(
            &mut self.last_label_for_splash,
            &self.books,
            ctx.pos,
            *ctx.cursor_verse,
        );
        Ok(())
    }
}

fn dispatch_reading(
    state: &mut LoopState,
    ctx: &mut AppCtx,
    key: KeyEvent,
) -> Result<DispatchStep> {
    let Some(action) = state.keys.handle(key) else {
        return Ok(DispatchStep::Continue);
    };
    match action {
        Action::OpenGoto => state.dialog = Dialog::Goto(GotoDialog::new()),
        Action::OpenFind => state.dialog = Dialog::Find(FindDialog::new()),
        Action::OpenHelp => state.dialog = Dialog::Help(HelpDialog::new()),
        Action::OpenFootnote => state.open_footnote_dialog(ctx),
        Action::JumpBack => state.history_step(ctx, HistoryDir::Back)?,
        Action::JumpForward => state.history_step(ctx, HistoryDir::Forward)?,
        Action::CopyVerse => LoopState::copy_verse(ctx),
        Action::ToggleSidebar => state.show_sidebar = !state.show_sidebar,
        Action::ToggleVisual => state.toggle_visual(*ctx.cursor_verse),
        Action::AddBookmark => state.add_bookmark(ctx),
        Action::OpenBookmarks => state.open_bookmarks_dialog(),
        Action::OpenTranslations => state.open_translations_dialog(ctx)?,
        Action::Back => state.enter_splash(ctx),
        Action::Quit => return Ok(DispatchStep::Quit),
        Action::SearchNext => state.repeat_search_action(ctx, true)?,
        Action::SearchPrev => state.repeat_search_action(ctx, false)?,
        _ => {
            if apply_action(
                action,
                ctx.db,
                &state.books,
                ctx.pos,
                ctx.passage,
                ctx.cursor_verse,
                &mut state.history,
            )? {
                return Ok(DispatchStep::Quit);
            }
        }
    }
    Ok(DispatchStep::Continue)
}

/// Compute the set of bookmarked verse numbers for the given chapter.
///
/// Called per draw frame (~6 Hz). The bookmark store is small (<100
/// entries in any realistic session) so the O(n) scan + `BTreeSet`
/// allocation here is well under the noise floor; not worth the
/// borrow-checker contortions of a cached invalidation scheme.
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

fn mode_tag_for(bg: &Bg, dialog: &Dialog, visual: bool) -> Cow<'static, str> {
    match dialog {
        Dialog::Goto(_) => Cow::Borrowed("-- GOTO --"),
        Dialog::Find(_) => Cow::Borrowed("-- FIND --"),
        Dialog::Footnote(_) => Cow::Borrowed("-- NOTES --"),
        Dialog::Help(_) => Cow::Borrowed("-- HELP --"),
        Dialog::Bookmarks(_) => Cow::Borrowed("-- BOOKMARKS --"),
        Dialog::Translations(_) => Cow::Borrowed("-- TRANSLATIONS --"),
        Dialog::None => match bg {
            Bg::Splash(s) => match s.mode {
                crate::ui::splash::SplashMode::Normal => Cow::Borrowed("-- NORMAL --"),
                crate::ui::splash::SplashMode::Filter => Cow::Borrowed("-- FILTER --"),
            },
            Bg::Reading => {
                if visual {
                    Cow::Borrowed("-- VISUAL --")
                } else {
                    Cow::Borrowed("-- NORMAL --")
                }
            }
        },
    }
}

const STATUS_SPLASH: &[Shortcut<'static>] = &[
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
];

const STATUS_READING_HIDE: &[Shortcut<'static>] = &reading_shortcuts("Hide");
const STATUS_READING_REFS: &[Shortcut<'static>] = &reading_shortcuts("Refs");

const fn reading_shortcuts(tab_action: &'static str) -> [Shortcut<'static>; 8] {
    [
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
            key: "Tab",
            action: tab_action,
        },
        Shortcut {
            key: "Esc",
            action: "Home",
        },
        Shortcut {
            key: "Q",
            action: "Quit",
        },
    ]
}

const fn make_status(bg: &Bg, show_sidebar: bool) -> &'static [Shortcut<'static>] {
    match bg {
        Bg::Splash(_) => STATUS_SPLASH,
        Bg::Reading if show_sidebar => STATUS_READING_HIDE,
        Bg::Reading => STATUS_READING_REFS,
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
        .map_or_else(|| pos.book.clone(), |b| b.name.clone());
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
    let max = passage.verses.last().map_or(1, |v| v.number);
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
    passage.verses.last().map_or(1, |v| v.number)
}

/// Returns true if the loop should exit.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "pos is mutated through jump_to in the chapter/book arms below; \
              clippy can't follow the call"
)]
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
            *cursor_verse = (*cursor_verse + i64::from(n)).min(last);
            Ok(false)
        }
        Action::CursorUp(n) => {
            *cursor_verse = (*cursor_verse - i64::from(n)).max(1);
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
    let mut hits = search::search(db, db.translation(), query, 1000).ok()?;
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
    pos: &Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
) -> Result<()> {
    // Atomic swap: probe the new translation against `db` with its
    // translation field temporarily set to `code`. If any inner call
    // fails, restore the previous translation so the reader stays
    // consistent with itself.
    let prev = db.translation().to_string();
    db.set_translation_unchecked(code.to_string());
    let probe = (|| -> Result<(Vec<Book>, String, Passage)> {
        Ok((
            db.list_books()?,
            db.translation_label()?,
            db.load_passage(&pos.book, pos.chapter)?,
        ))
    })();
    match probe {
        Ok((new_books, new_label, new_passage)) => {
            *books = new_books;
            *translation_label = new_label;
            *passage = new_passage;
            // Clamp the cursor — a different translation may have fewer
            // verses for this chapter (rare in our three editions, but
            // defensive).
            let max = passage.verses.last().map_or(1, |v| v.number);
            if *cursor_verse > max {
                *cursor_verse = max.max(1);
            }
            Ok(())
        }
        Err(e) => {
            db.set_translation_unchecked(prev);
            Err(e)
        }
    }
}

fn persist_default_translation(code: &str) -> Result<()> {
    let mut cfg = config::load();
    cfg.default_translation = Some(code.to_string());
    config::save(&cfg)
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
        // Shortcut labels are short ASCII strings — fitting `usize` lengths
        // into `u16` for screen column math is safe in practice; the
        // try_from clamps in the unreachable case where it isn't.
        let key_len = u16::try_from(s.key.chars().count()).unwrap_or(u16::MAX);
        let action_len = u16::try_from(s.action.chars().count()).unwrap_or(u16::MAX);
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
