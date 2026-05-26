//! `turbo-bible` — a Turbo Vision–styled terminal Bible reader with
//! FTS5 search.
//!
//! This crate is a single binary. See [`README.md`] for the user-facing
//! tour and [`docs/USAGE.md`] for a feature walk-through; the source
//! tree mirrors the README's "Layout" section.
//!
//! [`README.md`]: https://github.com/mathiasror/turbo-bible/blob/main/README.md
//! [`docs/USAGE.md`]: https://github.com/mathiasror/turbo-bible/blob/main/docs/USAGE.md
#![forbid(unsafe_code)]

mod bookmark;
mod bundled;
mod config;
mod db;
mod fetch;
mod install;
mod keys;
mod manifest;
mod nav;
mod paths;
mod quote;
mod reference;
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

use anyhow::{Context, Result};
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
use crate::ui::translations::{PickerEntry, TranslationsDialog, TranslationsOutcome};

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

    /// Directory holding the per-translation `<code>.db` files plus
    /// `xrefs.db`. Defaults to `$XDG_DATA_HOME/turbo-bible/translations/`
    /// (i.e. `~/.local/share/turbo-bible/translations/` on Linux/macOS).
    /// First launch auto-extracts the bundled translations into this
    /// directory; pass `install --force` to re-extract.
    #[arg(long)]
    translations_dir: Option<PathBuf>,

    /// Translation code. If omitted, falls back to the picker default
    /// stored in config.toml, then to the first installed translation.
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
    /// Extract bundled translations into the translations directory.
    /// Runs automatically on every startup when files are missing;
    /// invoke explicitly with `--force` to re-extract.
    Install(install::InstallArgs),
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
    /// Memoized set of bookmarked verses for the chapter currently in
    /// `passage`. Lazily filled by [`LoopState::bookmarks_for`]; invalidated
    /// (set to `None`) whenever `self.bookmarks` mutates. Saves rebuilding
    /// the `BTreeSet` on every draw frame.
    bookmarks_cache: Option<(BookmarksKey, std::collections::BTreeSet<i64>)>,
    last_query: Option<String>,
    last_label_for_splash: Option<(Position, String)>,
    visual_anchor: Option<i64>,
    show_sidebar: bool,
    max_reading_width: u16,
    keys: KeyState,
}

/// Cache key for [`LoopState::bookmarks_cache`]: `(translation, book, chapter)`.
type BookmarksKey = (String, String, i64);

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
    let translations_dir = resolve_translations_dir(&args)?;
    if let Some(Commands::Install(install_args)) = &args.command {
        return install::run(install_args);
    }
    // First launch (or any time files are missing) auto-extracts the
    // bundled translations into the translations directory; idempotent
    // and silent when nothing is missing. The data pipeline ships
    // FTS5 pre-optimised, so no runtime rebuild is needed any more.
    install::ensure_installed(&translations_dir)
        .with_context(|| format!("auto-install into {}", translations_dir.display()))?;
    // Non-fatal save failures collected here and replayed to stderr after
    // restore_terminal. Inside the TUI loop, eprintln would mangle the
    // alternate-screen display, so we defer.
    let mut warnings: Vec<String> = Vec::new();
    let (persisted, config) = state::load_with_migration();
    theme::init(config.theme.clone());
    let translation = resolve_translation(&args, &translations_dir, &config)?;
    // Save right away so the on-disk layout converges to the split form.
    save_or_warn(&mut warnings, "config save", config::save(&config));
    if let Some(ps) = &persisted {
        save_or_warn(&mut warnings, "state save", state::save(ps));
    }
    let mut db = Db::open_ro(&translations_dir, &translation)?;
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

/// Resolve the translations directory: explicit `--translations-dir`
/// flag wins; otherwise `paths::translations_dir()` (typically
/// `~/.local/share/turbo-bible/translations/`).
fn resolve_translations_dir(args: &Args) -> Result<PathBuf> {
    if let Some(p) = args.translations_dir.clone() {
        return Ok(p);
    }
    paths::translations_dir()
}

/// Startup translation resolution: `--translation` > config default >
/// first installed code (alphabetical).
fn resolve_translation(
    args: &Args,
    translations_dir: &Path,
    cfg: &config::Config,
) -> Result<String> {
    let installed = db::installed_codes(translations_dir)?;
    // Explicit --translation overrides everything; the caller (Db::open_ro)
    // will surface a clear error if it isn't installed.
    if let Some(t) = args.translation.as_ref() {
        return Ok(t.clone());
    }
    // Config default only wins if the file's still there. A stale
    // value (e.g. a translation the user has since deleted) falls
    // through to the bundled default so the app starts cleanly.
    if let Some(t) = &cfg.default_translation
        && installed.iter().any(|c| c == t)
    {
        return Ok(t.clone());
    }
    if installed.iter().any(|c| c == bundled::DEFAULT_TRANSLATION) {
        return Ok(bundled::DEFAULT_TRANSLATION.to_string());
    }
    installed.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "No translations installed in {}. Run `turbo-bible install --force` \
             to extract the bundled default.",
            translations_dir.display()
        )
    })
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
        draw_frame(term, &mut state, passage, *cursor_verse)?;

        if event::poll(Duration::from_millis(150))? {
            let term_height = term.size().map_or(24, |s| s.height);
            let raw_event = event::read()?;
            let synth: Option<KeyEvent> = match raw_event {
                Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
                Event::Mouse(me) => mouse_to_key(
                    me,
                    term_height,
                    make_status(&state.bg, state.show_sidebar, state.visual_anchor.is_some()),
                ),
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
            bookmarks_cache: None,
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
    state: &mut LoopState,
    passage: &Passage,
    cursor_verse: i64,
) -> Result<()> {
    let status = make_status(&state.bg, state.show_sidebar, state.visual_anchor.is_some());
    state.refresh_bookmarks_cache(passage);
    // SAFETY (logical): refresh_bookmarks_cache guarantees Some(...) on return.
    let bookmarked_in_chapter = &state
        .bookmarks_cache
        .as_ref()
        .expect("refresh_bookmarks_cache always sets the cache")
        .1;
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
                let mode_tag = mode_tag_for(
                    &state.bg,
                    &state.dialog,
                    state.visual_anchor.is_some(),
                    state.show_sidebar,
                );
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
                let mode_tag = mode_tag_for(
                    &state.bg,
                    &state.dialog,
                    state.visual_anchor.is_some(),
                    state.show_sidebar,
                );
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
                    bookmarked: bookmarked_in_chapter,
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
            Dialog::Help(d) => d.render(area, buf),
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
        // Guarded by `dispatch_key`: this function is only entered when
        // `state.dialog` is non-None. Crash loudly if that invariant breaks
        // rather than silently swallowing the keystroke.
        Dialog::None => unreachable!("dispatch_dialog called with Dialog::None"),
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
        Dialog::Help(d) => {
            if matches!(d.handle(key), HelpOutcome::Cancel) {
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
                    state.bookmarks_cache = None;
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
                TranslationsOutcome::Download(code) => {
                    // Blocks the event loop while curl fetches the
                    // ~4 MB tarball, verifies sha256, and decompresses.
                    // The dialog stays on screen until we close it
                    // below; "Downloading…" affordances live as a
                    // future enhancement (background thread + channel).
                    let dir = ctx.db.translations_dir().to_path_buf();
                    match fetch::translation(&dir, &code)
                        .and_then(|()| ctx.db.add_translation(&code))
                    {
                        Ok(()) => {
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
                        }
                        Err(e) => {
                            ctx.warnings.push(format!("download {code} failed: {e}"));
                        }
                    }
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
            state.dialog = Dialog::Goto(GotoDialog::new(ctx.db.translation()));
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenFind => {
            state.dialog = Dialog::Find(FindDialog::new(ctx.db.translation()));
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
                picker_entries(ctx.db),
                ctx.db.translation(),
            ));
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenHelp => {
            state.dialog = Dialog::Help(HelpDialog::new());
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
        self.bookmarks_cache = None;
        save_or_warn(ctx.warnings, "bookmarks save (add)", self.bookmarks.save());
        self.visual_anchor = None;
    }

    /// Rebuild `bookmarks_cache` if it doesn't already match the current
    /// passage. The set itself is small (verses bookmarked in this chapter)
    /// and rebuilds in microseconds, but at 6 Hz the per-frame allocation
    /// was wasted churn.
    fn refresh_bookmarks_cache(&mut self, passage: &Passage) {
        let key = (
            passage.translation.clone(),
            passage.book_code.clone(),
            passage.chapter,
        );
        if self
            .bookmarks_cache
            .as_ref()
            .is_some_and(|(k, _)| k == &key)
        {
            return;
        }
        let set = build_bookmarks_set(
            &self.bookmarks,
            &passage.translation,
            &passage.book_code,
            passage.chapter,
        );
        self.bookmarks_cache = Some((key, set));
    }

    fn open_bookmarks_dialog(&mut self, ctx: &AppCtx) {
        let mut d = crate::ui::bookmarks::BookmarksDialog::new(&self.bookmarks, ctx.db);
        d.sort_canonical(&self.books);
        self.dialog = Dialog::Bookmarks(d);
    }

    fn open_translations_dialog(&mut self, ctx: &AppCtx) -> Result<()> {
        self.dialog = Dialog::Translations(TranslationsDialog::new(
            picker_entries(ctx.db),
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
        Action::OpenGoto => {
            // Pre-fill with the current reference so `Enter` is a no-op
            // "stay here" and a quick edit (e.g. bumping chapter or verse)
            // costs only a few keystrokes.
            let book_name = state
                .books
                .iter()
                .find(|b| b.code == ctx.pos.book)
                .map_or(ctx.pos.book.clone(), |b| b.name.clone());
            state.dialog = Dialog::Goto(GotoDialog::with_position(
                &book_name,
                ctx.pos.chapter,
                *ctx.cursor_verse,
                ctx.db.translation(),
            ));
        }
        Action::OpenFind => state.dialog = Dialog::Find(FindDialog::new(ctx.db.translation())),
        Action::OpenHelp => state.dialog = Dialog::Help(HelpDialog::new()),
        Action::OpenFootnote => state.open_footnote_dialog(ctx),
        Action::JumpBack => state.history_step(ctx, HistoryDir::Back)?,
        Action::JumpForward => state.history_step(ctx, HistoryDir::Forward)?,
        Action::CopyVerse => LoopState::copy_verse(ctx),
        Action::ToggleSidebar => state.show_sidebar = !state.show_sidebar,
        Action::ToggleVisual => state.toggle_visual(*ctx.cursor_verse),
        Action::AddBookmark => state.add_bookmark(ctx),
        Action::OpenBookmarks => state.open_bookmarks_dialog(ctx),
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
/// Called per draw frame (~6 Hz). The set is memoized on `LoopState`
/// keyed by `(translation, book, chapter)` and invalidated when the
/// bookmark store mutates, so the rebuild only fires on a real change.
fn build_bookmarks_set(
    store: &bookmark::BookmarkStore,
    translation: &str,
    book: &str,
    chapter: i64,
) -> std::collections::BTreeSet<i64> {
    let mut out = std::collections::BTreeSet::new();
    for b in &store.bookmarks {
        if b.matches_chapter(translation, book, chapter) {
            for v in b.start_verse..=b.end_verse {
                out.insert(v);
            }
        }
    }
    out
}

fn mode_tag_for(bg: &Bg, dialog: &Dialog, visual: bool, show_sidebar: bool) -> Cow<'static, str> {
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
            // NOREFS is a persistent cue that the sidebar is toggled off, so
            // the reading area looking different on return is self-explained.
            Bg::Reading => {
                let base = if visual { "VISUAL" } else { "NORMAL" };
                if show_sidebar {
                    Cow::Owned(format!("-- {base} --"))
                } else {
                    Cow::Owned(format!("-- {base} | NOREFS --"))
                }
            }
        },
    }
}

const STATUS_SPLASH: &[Shortcut<'static>] = &[
    Shortcut {
        key: "F1",
        action: "Help",
    },
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

const STATUS_VISUAL: &[Shortcut<'static>] = &[
    Shortcut {
        key: "y",
        action: "Copy",
    },
    Shortcut {
        key: "b",
        action: "Bookmark",
    },
    Shortcut {
        key: "V",
        action: "Exit",
    },
    Shortcut {
        key: "Esc",
        action: "Cancel",
    },
];

const fn make_status(bg: &Bg, show_sidebar: bool, visual: bool) -> &'static [Shortcut<'static>] {
    match bg {
        Bg::Splash(_) => STATUS_SPLASH,
        // In a visual selection the relevant actions are copy / bookmark /
        // exit, so swap the reading hints for those (mirrors how the dialogs
        // carry their own mode-specific footers).
        Bg::Reading if visual => STATUS_VISUAL,
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
    let mut hits = search::search(db, db.translation(), query, search::REPEAT_LIMIT).ok()?;
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

/// Build the picker entry list: every translation the binary knows
/// about (from the static manifest), marked installed iff its `.db`
/// is currently loaded in `db`.
fn picker_entries(db: &Db) -> Vec<PickerEntry> {
    use std::collections::HashSet;
    let installed: HashSet<&str> = db.translations().iter().map(|t| t.code.as_str()).collect();
    crate::manifest::TRANSLATIONS
        .iter()
        .map(|t| PickerEntry {
            code: t.code.to_string(),
            name: t.name.to_string(),
            language: t.language.to_string(),
            installed: installed.contains(t.code),
            compressed_size: t.compressed_size,
        })
        .collect()
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
    // The atomic swap (with rollback on probe failure) lives on Db itself.
    // Here we own the in-memory mirrors; if the probe succeeds, copy the
    // new values across and clamp the cursor — verse counts can differ
    // between translations (rare in our three editions, but defensive).
    let (new_books, new_label, new_passage) =
        db.try_switch_translation(code, &pos.book, pos.chapter)?;
    *books = new_books;
    *translation_label = new_label;
    *passage = new_passage;
    let max = passage.verses.last().map_or(1, |v| v.number);
    if *cursor_verse > max {
        *cursor_verse = max.max(1);
    }
    Ok(())
}

fn persist_default_translation(code: &str) -> Result<()> {
    // load_quiet (not load): this runs inside the event loop, so a config-read
    // warning must not eprintln over the alternate screen.
    let mut cfg = config::load_quiet();
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
