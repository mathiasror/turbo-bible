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
use std::time::{Duration, Instant};

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

/// One independent reading column. The fields that used to be the run
/// loop's single reading context (`pos`/`passage`/`cursor_verse`), plus
/// the visual-selection anchor and jump history, now live per-pane so
/// each compare pane scrolls, navigates, and selects on its own. The
/// focused pane's `translation` always equals `db.translation()` — see
/// [`LoopState::sync_focus_to_db`].
struct Pane {
    translation: String,
    pos: Position,
    passage: Passage,
    cursor_verse: i64,
    visual_anchor: Option<i64>,
    history: History,
    /// Set only on a pane opened from the `K` xref popup via `s`: the source
    /// reference (e.g. `"John 3:16"`) the cross-reference was followed *from*,
    /// rendered as `… ← John 3:16` in the title so the relationship is clear.
    /// `None` for the initial pane and `Ctrl-W v` translation compares (which
    /// have no single origin verse).
    origin_label: Option<String>,
}

impl Pane {
    fn new(translation: String, pos: Position, passage: Passage, cursor_verse: i64) -> Self {
        let history = History::new(pos.clone());
        Self {
            translation,
            pos,
            passage,
            cursor_verse,
            visual_anchor: None,
            history,
            origin_label: None,
        }
    }

    /// Clamp the cursor into the loaded passage's verse range. Used after
    /// seeding a pane from another translation, whose versification may
    /// have fewer verses in the same chapter.
    fn clamp_cursor(&mut self) {
        let max = self.passage.verses.last().map_or(1, |v| v.number);
        self.cursor_verse = self.cursor_verse.clamp(1, max.max(1));
    }
}

/// Which reading translation a freshly-confirmed Translations picker
/// should affect: replace the focused pane's translation (the `t` flow)
/// or spawn a new compare pane (the `Ctrl-W v` flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerIntent {
    SwitchFocused,
    OpenNewPane,
}

/// Mutable state owned by the run loop but threaded through the
/// extracted dispatch helpers. Separating this from the externally-owned
/// reader state (`AppCtx`) keeps method signatures short and lets the
/// dispatch helpers be free functions.
struct LoopState {
    books: Vec<Book>,
    translation_label: String,
    bg: Bg,
    dialog: Dialog,
    /// Reading panes, left-to-right. Always at least one. `focus` indexes
    /// the active one — the only pane that receives motion keys and the
    /// one whose translation is active in `db`.
    panes: Vec<Pane>,
    focus: usize,
    bookmarks: bookmark::BookmarkStore,
    /// Memoized bookmarked-verse sets, keyed by `(translation, book,
    /// chapter)`. Per-pane panes can show different chapters, so a single
    /// memo slot won't do; the whole map is cleared whenever
    /// `self.bookmarks` mutates.
    bookmarks_cache: std::collections::HashMap<BookmarksKey, std::collections::BTreeSet<i64>>,
    last_query: Option<String>,
    last_label_for_splash: Option<(Position, String)>,
    /// Which way the next confirmed Translations picker resolves.
    picker_intent: PickerIntent,
    show_sidebar: bool,
    /// The user's configured sidebar preference, restored when a compare
    /// split collapses back to a single pane (the sidebar is force-hidden
    /// while ≥2 panes are open).
    sidebar_pref: bool,
    /// Most recent terminal width, refreshed each draw. Lets the
    /// open-pane action refuse a split that would leave columns unreadable
    /// without reaching for the terminal handle.
    last_term_width: u16,
    /// Transient one-line status hint (e.g. "Terminal too narrow") with
    /// its set-time, cleared after a short delay.
    transient_msg: Option<(String, Instant)>,
    max_reading_width: u16,
    keys: KeyState,
}

/// Cache key for [`LoopState::bookmarks_cache`]: `(translation, book, chapter)`.
type BookmarksKey = (String, String, i64);

/// Borrowed bundle of the externally-owned reader state. The reading
/// context (position/passage/cursor) now lives in `LoopState::panes`, so
/// this only carries the externally-owned `Db` and the deferred-warning
/// sink.
struct AppCtx<'a> {
    db: &'a mut Db,
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
        let passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = 1;
        let r = run(
            guard.terminal(),
            &mut db,
            books,
            translation_label,
            &mut pos,
            passage,
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
        let passage = db.load_passage(&pos.book, pos.chapter)?;
        let mut cursor_verse: i64 = persisted.as_ref().map_or(1, |p| p.verse).max(1);
        let r = run(
            guard.terminal(),
            &mut db,
            books,
            translation_label,
            &mut pos,
            passage,
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
    passage: Passage,
    cursor_verse: &mut i64,
    initial_splash: Option<SplashSeed>,
    config: &config::Config,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut state = LoopState::new(
        books,
        translation_label,
        pos,
        passage,
        *cursor_verse,
        initial_splash,
        db.translation(),
        config,
        warnings,
    );

    loop {
        draw_frame(term, &mut state)?;

        if event::poll(Duration::from_millis(150))? {
            let term_height = term.size().map_or(24, |s| s.height);
            let raw_event = event::read()?;
            let synth: Option<KeyEvent> = match raw_event {
                Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
                Event::Mouse(me) => mouse_to_key(me, term_height, make_status(&state)),
                _ => None,
            };
            if let Some(key) = synth {
                let mut ctx = AppCtx { db, warnings };
                let step = dispatch_key(&mut state, &mut ctx, key)?;
                if matches!(step, DispatchStep::Quit) {
                    // Persist the focused pane's final position (the pane the
                    // user was last reading) back to the caller for state.toml.
                    let pane = &state.panes[state.focus];
                    *pos = pane.pos.clone();
                    *cursor_verse = pane.cursor_verse;
                    return Ok(());
                }
            }
        } else {
            state.tick();
        }
    }
}

impl LoopState {
    #[allow(
        clippy::too_many_arguments,
        reason = "constructs the loop-local state from the values `main` resolves \
                  at startup; bundling them into a struct would just move the \
                  long signature up one frame"
    )]
    fn new(
        books: Vec<Book>,
        translation_label: String,
        pos: &Position,
        passage: Passage,
        cursor_verse: i64,
        initial_splash: Option<SplashSeed>,
        translation: &str,
        config: &config::Config,
        warnings: &mut Vec<String>,
    ) -> Self {
        let keys = KeyState::with_user_bindings(&config.keys, config.input.keymap);
        let pane = Pane::new(translation.to_string(), pos.clone(), passage, cursor_verse);
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
        let bookmarks = bookmark::BookmarkStore::load(warnings);
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
            panes: vec![pane],
            focus: 0,
            bookmarks,
            bookmarks_cache: std::collections::HashMap::new(),
            last_query: None,
            last_label_for_splash,
            picker_intent: PickerIntent::SwitchFocused,
            show_sidebar: config.reading.show_sidebar,
            sidebar_pref: config.reading.show_sidebar,
            last_term_width: 0,
            transient_msg: None,
            max_reading_width: config.reading.max_width,
            keys,
        }
    }

    fn focused(&self) -> &Pane {
        &self.panes[self.focus]
    }

    fn focused_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.focus]
    }

    /// Re-point `Db` at the focused pane's translation and refresh the
    /// global `books`/`translation_label` mirrors. Must be called after
    /// every focus change and every focused-pane translation change so the
    /// search / quote / Find paths (which query the active connection)
    /// follow the focused pane. See the focus==active invariant.
    fn sync_focus_to_db(&mut self, db: &mut Db) -> Result<()> {
        let code = self.panes[self.focus].translation.clone();
        db.set_active(&code)?;
        self.books = db.list_books()?;
        self.translation_label = db.translation_label()?;
        Ok(())
    }

    /// Cycle focus by `delta` panes (wrapping). No-op with a single pane.
    fn focus_cycle(&mut self, delta: isize, db: &mut Db) -> Result<()> {
        let n = self.panes.len();
        if n <= 1 {
            return Ok(());
        }
        let cur = isize::try_from(self.focus).unwrap_or(0);
        let len = isize::try_from(n).unwrap_or(1);
        self.focus = usize::try_from((cur + delta).rem_euclid(len)).unwrap_or(0);
        self.sync_focus_to_db(db)
    }

    /// Move focus one pane left/right, clamping at the ends.
    fn focus_dir(&mut self, right: bool, db: &mut Db) -> Result<()> {
        let n = self.panes.len();
        if n <= 1 {
            return Ok(());
        }
        self.focus = if right {
            (self.focus + 1).min(n - 1)
        } else {
            self.focus.saturating_sub(1)
        };
        self.sync_focus_to_db(db)
    }

    /// Set a transient status hint, shown briefly then cleared by [`Self::tick`].
    fn set_transient(&mut self, msg: impl Into<String>) {
        self.transient_msg = Some((msg.into(), Instant::now()));
    }

    /// Per-poll housekeeping: advance the key-chord timeout and expire any
    /// transient status hint.
    fn tick(&mut self) {
        self.keys.tick();
        if let Some((_, set_at)) = &self.transient_msg
            && set_at.elapsed() > Duration::from_secs(2)
        {
            self.transient_msg = None;
        }
    }

    /// Whether the terminal is wide enough to add one more reading pane
    /// without dropping any column below [`ui::MIN_PANE_W`]. A width of 0
    /// means "not measured yet" (no draw has happened, or a sizeless PTY);
    /// allow it rather than block on an unknown — the user can always close.
    fn can_add_pane(&self) -> bool {
        if self.last_term_width == 0 {
            return true;
        }
        ui::min_pane_interior(self.last_term_width, self.panes.len() + 1) >= ui::MIN_PANE_W
    }
}

/// One pass of the draw cycle. Kept inline (vs split into per-bg
/// helpers) because the closure borrows many fields and pulling it apart
/// duplicates the dialog overlay match.
fn draw_frame(term: &mut Tty, state: &mut LoopState) -> Result<()> {
    // Refresh the open-pane width guard and the per-pane bookmark caches
    // up front, while we still hold `&mut state` — the draw closure below
    // borrows `state` immutably.
    state.last_term_width = term.size().map_or(0, |s| s.width);
    state.refresh_all_bookmark_caches();

    let status = make_status(state);
    // A transient hint (e.g. "Terminal too narrow") takes over the mode tag.
    let mode_tag = match &state.transient_msg {
        Some((msg, _)) => Cow::Owned(format!("-- {msg} --")),
        None => mode_tag_for(state),
    };
    let menu_title = format!(" Turbo Bible \u{00B7} {} ", state.translation_label);

    // Per-pane render inputs. Borrows `state.panes` + `state.bookmarks_cache`
    // (disjoint immutable borrows); `empty` covers the can't-happen miss so a
    // cache gap degrades to "no bookmark stars" rather than a panic.
    let empty_bookmarks = std::collections::BTreeSet::new();
    // The focused pane's cursor verse, echoed into each unfocused pane as a
    // passive cross-pane locator (only meaningful when comparing). This is a
    // *read-only* cue — it never moves another pane's cursor or scroll; the
    // panes stay independent by design.
    // TODO(design): verse-sync scrolling (actually moving the other panes when
    // the focused pane moves) is intentionally NOT implemented — it reverses
    // the user-confirmed "independent panes" decision and needs product
    // sign-off before we touch motion handling.
    let focused_verse = state.panes.get(state.focus).map_or(1, |p| p.cursor_verse);
    let comparing = state.panes.len() > 1;
    let pane_renders: Vec<ui::PaneRender<'_>> = state
        .panes
        .iter()
        .enumerate()
        .map(|(i, pane)| {
            let key = (
                pane.passage.translation.clone(),
                pane.passage.book_code.clone(),
                pane.passage.chapter,
            );
            let bookmarked = state.bookmarks_cache.get(&key).unwrap_or(&empty_bookmarks);
            let selection = pane.visual_anchor.map(|a| {
                let c = pane.cursor_verse;
                if a <= c { (a, c) } else { (c, a) }
            });
            let is_focused = i == state.focus;
            ui::PaneRender {
                passage: &pane.passage,
                cursor_verse: pane.cursor_verse,
                selection,
                bookmarked,
                is_focused,
                origin_label: pane.origin_label.as_deref(),
                // The cue is read-only and only for the *other* panes, so the
                // focused pane never tints itself.
                peer_verse: (comparing && !is_focused).then_some(focused_verse),
            }
        })
        .collect();

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
                crate::ui::statusbar::render(
                    status,
                    ratatui::layout::Rect::new(
                        area.x,
                        area.y + area.height.saturating_sub(1),
                        area.width,
                        1,
                    ),
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
                ui::Frame {
                    menu_title: &menu_title,
                    status,
                    status_mode: &mode_tag,
                    panes: &pane_renders,
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
    let f = state.focus;
    {
        let pane = &mut state.panes[f];
        jump_to(
            p,
            ctx.db,
            &mut pane.pos,
            &mut pane.passage,
            &mut pane.cursor_verse,
            &mut pane.history,
        )?;
    }
    update_splash_label(
        &mut state.last_label_for_splash,
        &state.books,
        &state.panes[f].pos,
        state.panes[f].cursor_verse,
    );
    state.bg = Bg::Reading;
    state.dialog = Dialog::None;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one match arm per dialog variant; the close/jump glue is tightly coupled and reads clearer inline than scattered across per-variant helpers"
)]
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
            FootnoteOutcome::OpenSplit(p) => {
                // Open the xref target beside the current verse, in the source
                // pane's translation. `p.verse` lands the new pane's cursor.
                // The new pane's title states the relationship — `← <source>` —
                // so it's clear which verse the cross-reference was followed
                // from (the focused pane is that source).
                let src = state.focused();
                let code = src.translation.clone();
                let origin = format!(
                    "{} {}:{}",
                    src.passage.book_abbrev, src.pos.chapter, src.cursor_verse
                );
                open_compare_pane(state, ctx, &code, Some(p), Some(origin))?;
                state.dialog = Dialog::None;
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
                    state.bookmarks_cache.clear();
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
                    apply_translation_pick(state, ctx, &code)?;
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
                        Ok(()) => apply_translation_pick(state, ctx, &code)?,
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

/// Resolve a confirmed Translations pick per the pending [`PickerIntent`]:
/// either swap the focused pane's translation in place, or spawn a new
/// compare pane reading `code` at the focused pane's current position.
fn apply_translation_pick(state: &mut LoopState, ctx: &mut AppCtx, code: &str) -> Result<()> {
    match state.picker_intent {
        PickerIntent::SwitchFocused => switch_focused_translation(state, ctx, code),
        // `Ctrl-W v` translation compares have no single origin verse, so no
        // origin label.
        PickerIntent::OpenNewPane => open_compare_pane(state, ctx, code, None, None),
    }
}

/// Swap the focused pane's translation (the `t` / F5 flow). Persists the
/// new code as the launch default and refreshes the splash "Continue" label.
fn switch_focused_translation(state: &mut LoopState, ctx: &mut AppCtx, code: &str) -> Result<()> {
    let f = state.focus;
    {
        let pane = &mut state.panes[f];
        switch_translation(
            ctx.db,
            &mut state.books,
            &mut state.translation_label,
            code,
            &pane.pos,
            &mut pane.passage,
            &mut pane.cursor_verse,
        )?;
        pane.translation = code.to_string();
    }
    save_or_warn(
        ctx.warnings,
        "default-translation persist",
        persist_default_translation(code),
    );
    update_splash_label(
        &mut state.last_label_for_splash,
        &state.books,
        &state.panes[f].pos,
        state.panes[f].cursor_verse,
    );
    Ok(())
}

/// Spawn a new compare pane reading `code`. `seed` gives the starting
/// position (with `verse` landing the cursor); `None` clones the focused
/// pane's position + cursor — i.e. "the same passage in another
/// translation". The new pane becomes focused and active.
fn open_compare_pane(
    state: &mut LoopState,
    ctx: &mut AppCtx,
    code: &str,
    seed: Option<Position>,
    origin: Option<String>,
) -> Result<()> {
    if !state.can_add_pane() {
        state.set_transient("Terminal too narrow for another pane");
        return Ok(());
    }
    let (seed_pos, cursor) = match seed {
        Some(p) => {
            let c = p.verse.unwrap_or(1);
            (p, c)
        }
        None => {
            let fp = state.focused();
            (fp.pos.clone(), fp.cursor_verse)
        }
    };
    let passage = ctx
        .db
        .load_passage_for(code, &seed_pos.book, seed_pos.chapter)?;
    let mut pane = Pane::new(code.to_string(), seed_pos, passage, cursor);
    pane.origin_label = origin;
    pane.clamp_cursor();
    // One-time orientation hint on the 1 -> 2 transition (the first time the
    // reader enters compare mode), not on every subsequent split.
    let entering_compare = state.panes.len() == 1;
    state.panes.push(pane);
    state.focus = state.panes.len() - 1;
    if entering_compare {
        state.set_transient("References sidebar hidden while comparing — press K for notes");
    }
    // The sidebar shares the body width the new pane needs; suppress it
    // while comparing (restored from `sidebar_pref` when the split closes).
    state.show_sidebar = false;
    state.sync_focus_to_db(ctx.db)?;
    Ok(())
}

/// Close the focused pane. A no-op (with a hint) when only one remains.
/// Re-points `Db` at the newly-focused pane and, on collapse to a single
/// pane, restores the user's sidebar preference.
fn close_focused_pane(state: &mut LoopState, ctx: &mut AppCtx) -> Result<()> {
    if state.panes.len() <= 1 {
        state.set_transient("Only one pane");
        return Ok(());
    }
    state.panes.remove(state.focus);
    state.focus = state.focus.min(state.panes.len() - 1);
    if state.panes.len() == 1 {
        state.show_sidebar = state.sidebar_pref;
    }
    state.sync_focus_to_db(ctx.db)
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
            let f = state.focus;
            {
                let pane = &mut state.panes[f];
                jump_to(
                    p,
                    ctx.db,
                    &mut pane.pos,
                    &mut pane.passage,
                    &mut pane.cursor_verse,
                    &mut pane.history,
                )?;
            }
            update_splash_label(
                &mut state.last_label_for_splash,
                &state.books,
                &state.panes[f].pos,
                state.panes[f].cursor_verse,
            );
            state.bg = Bg::Reading;
            Ok(DispatchStep::Continue)
        }
        SplashOutcome::OpenTranslations => {
            state.picker_intent = PickerIntent::SwitchFocused;
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
    fn open_footnote_dialog(&mut self) {
        let pane = &self.panes[self.focus];
        let target = format!(
            "{}.{}.{}",
            pane.pos.book, pane.pos.chapter, pane.cursor_verse
        );
        let notes: Vec<_> = pane
            .passage
            .footnotes
            .iter()
            .filter(|fn_| fn_.verse_osis == target)
            .cloned()
            .collect();
        let xrefs: Vec<_> = pane
            .passage
            .xrefs
            .iter()
            .filter(|x| x.from_verse == pane.cursor_verse)
            .cloned()
            .collect();
        let label = format!(
            "{} {}:{}",
            pane.passage.book_abbrev, pane.pos.chapter, pane.cursor_verse
        );
        self.dialog = Dialog::Footnote(FootnoteDialog::new(label, notes, xrefs));
    }

    fn history_step(&mut self, ctx: &mut AppCtx, dir: HistoryDir) -> Result<()> {
        let pane = &mut self.panes[self.focus];
        let target = match dir {
            HistoryDir::Back => pane.history.back(),
            HistoryDir::Forward => pane.history.forward(),
        };
        if let Some(p) = target {
            pane.pos = p;
            pane.passage = ctx.db.load_passage(&pane.pos.book, pane.pos.chapter)?;
            pane.cursor_verse = 1;
        }
        Ok(())
    }

    fn copy_verse(&self, ctx: &mut AppCtx) {
        let pane = self.focused();
        save_or_warn(
            ctx.warnings,
            "clipboard set",
            copy_verse_to_clipboard(&pane.passage, &pane.pos, pane.cursor_verse),
        );
    }

    fn toggle_visual(&mut self) {
        let pane = &mut self.panes[self.focus];
        pane.visual_anchor = if pane.visual_anchor.is_some() {
            None
        } else {
            Some(pane.cursor_verse)
        };
    }

    fn add_bookmark(&mut self, ctx: &mut AppCtx) {
        let (translation, book, chapter, s, e) = {
            let pane = &self.panes[self.focus];
            let cur = pane.cursor_verse;
            let (s, e) = match pane.visual_anchor {
                Some(a) if a <= cur => (a, cur),
                Some(a) => (cur, a),
                None => (cur, cur),
            };
            // Use the focused pane's own translation so the bookmark stays
            // self-consistent with the book/chapter it records, independent
            // of the focus==active invariant.
            (
                pane.translation.clone(),
                pane.pos.book.clone(),
                pane.pos.chapter,
                s,
                e,
            )
        };
        self.bookmarks.add(bookmark::Bookmark {
            translation,
            book,
            chapter,
            start_verse: s,
            end_verse: e,
            label: None,
            created_at: bookmark::now_unix(),
        });
        self.bookmarks_cache.clear();
        save_or_warn(ctx.warnings, "bookmarks save (add)", self.bookmarks.save());
        self.panes[self.focus].visual_anchor = None;
    }

    /// Ensure a bookmarked-verse set is cached for `(translation, book,
    /// chapter)`. Cheap (one `BTreeSet` build) and idempotent; the whole
    /// map is cleared on bookmark mutation.
    fn ensure_bookmark_cache(&mut self, translation: &str, book: &str, chapter: i64) {
        let key = (translation.to_string(), book.to_string(), chapter);
        if self.bookmarks_cache.contains_key(&key) {
            return;
        }
        let set = build_bookmarks_set(&self.bookmarks, translation, book, chapter);
        self.bookmarks_cache.insert(key, set);
    }

    /// Populate the bookmark cache for every open pane's chapter. Panes can
    /// show different chapters/translations, so each needs its own entry.
    fn refresh_all_bookmark_caches(&mut self) {
        for i in 0..self.panes.len() {
            let (t, b, c) = {
                let p = &self.panes[i].passage;
                (p.translation.clone(), p.book_code.clone(), p.chapter)
            };
            self.ensure_bookmark_cache(&t, &b, c);
        }
    }

    fn open_bookmarks_dialog(&mut self, ctx: &AppCtx) {
        let mut d = crate::ui::bookmarks::BookmarksDialog::new(&self.bookmarks, ctx.db);
        d.sort_canonical(&self.books);
        self.dialog = Dialog::Bookmarks(d);
    }

    fn open_translations_dialog(&mut self, ctx: &AppCtx) {
        // The `t` / F5 path replaces the focused pane's translation.
        self.picker_intent = PickerIntent::SwitchFocused;
        self.dialog = Dialog::Translations(TranslationsDialog::new(
            picker_entries(ctx.db),
            ctx.db.translation(),
        ));
    }

    /// Esc-from-reading: cancel visual selection if active, otherwise
    /// rebuild the splash view and switch the background to it.
    fn enter_splash(&mut self, ctx: &AppCtx) {
        if self.focused().visual_anchor.is_some() {
            self.focused_mut().visual_anchor = None;
            return;
        }
        let f = self.focus;
        update_splash_label(
            &mut self.last_label_for_splash,
            &self.books,
            &self.panes[f].pos,
            self.panes[f].cursor_verse,
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
    /// relative to the focused pane's cursor, jumping to the next hit (wrap).
    fn repeat_search_action(&mut self, ctx: &mut AppCtx, forward: bool) -> Result<()> {
        let Some(q) = self.last_query.as_deref() else {
            return Ok(());
        };
        let f = self.focus;
        let target = {
            let pane = &self.panes[f];
            repeat_search(
                ctx.db,
                &self.books,
                q,
                &pane.pos,
                pane.cursor_verse,
                forward,
            )
        };
        let Some(p) = target else {
            return Ok(());
        };
        {
            let pane = &mut self.panes[f];
            jump_to(
                p,
                ctx.db,
                &mut pane.pos,
                &mut pane.passage,
                &mut pane.cursor_verse,
                &mut pane.history,
            )?;
        }
        update_splash_label(
            &mut self.last_label_for_splash,
            &self.books,
            &self.panes[f].pos,
            self.panes[f].cursor_verse,
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
            let pane = state.focused();
            let book_name = state
                .books
                .iter()
                .find(|b| b.code == pane.pos.book)
                .map_or_else(|| pane.pos.book.clone(), |b| b.name.clone());
            state.dialog = Dialog::Goto(GotoDialog::with_position(
                &book_name,
                pane.pos.chapter,
                pane.cursor_verse,
                ctx.db.translation(),
            ));
        }
        Action::OpenFind => state.dialog = Dialog::Find(FindDialog::new(ctx.db.translation())),
        Action::OpenHelp => state.dialog = Dialog::Help(HelpDialog::new()),
        Action::OpenFootnote => state.open_footnote_dialog(),
        Action::JumpBack => state.history_step(ctx, HistoryDir::Back)?,
        Action::JumpForward => state.history_step(ctx, HistoryDir::Forward)?,
        Action::CopyVerse => state.copy_verse(ctx),
        Action::ToggleSidebar => {
            // Tab cycles focus when a compare split is open (the sidebar is
            // suppressed then anyway); otherwise it toggles the sidebar.
            if state.panes.len() >= 2 {
                state.focus_cycle(1, ctx.db)?;
            } else {
                state.show_sidebar = !state.show_sidebar;
                state.sidebar_pref = state.show_sidebar;
            }
        }
        Action::ToggleVisual => state.toggle_visual(),
        Action::AddBookmark => state.add_bookmark(ctx),
        Action::OpenBookmarks => state.open_bookmarks_dialog(ctx),
        Action::OpenTranslations => state.open_translations_dialog(ctx),
        Action::Back => state.enter_splash(ctx),
        Action::Quit => return Ok(DispatchStep::Quit),
        Action::SearchNext => state.repeat_search_action(ctx, true)?,
        Action::SearchPrev => state.repeat_search_action(ctx, false)?,
        Action::CompareOpen => {
            // Open the picker; its confirmation spawns a new pane.
            state.picker_intent = PickerIntent::OpenNewPane;
            state.dialog = Dialog::Translations(TranslationsDialog::new(
                picker_entries(ctx.db),
                ctx.db.translation(),
            ));
        }
        Action::FocusNext => state.focus_cycle(1, ctx.db)?,
        Action::FocusLeft => state.focus_dir(false, ctx.db)?,
        Action::FocusRight => state.focus_dir(true, ctx.db)?,
        Action::CompareClose => close_focused_pane(state, ctx)?,
        _ => {
            let f = state.focus;
            let pane = &mut state.panes[f];
            if apply_action(
                action,
                ctx.db,
                &state.books,
                &mut pane.pos,
                &mut pane.passage,
                &mut pane.cursor_verse,
                &mut pane.history,
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

fn mode_tag_for(state: &LoopState) -> Cow<'static, str> {
    match &state.dialog {
        Dialog::Goto(_) => Cow::Borrowed("-- GOTO --"),
        Dialog::Find(_) => Cow::Borrowed("-- FIND --"),
        Dialog::Footnote(_) => Cow::Borrowed("-- NOTES --"),
        Dialog::Help(_) => Cow::Borrowed("-- HELP --"),
        Dialog::Bookmarks(_) => Cow::Borrowed("-- BOOKMARKS --"),
        Dialog::Translations(_) => Cow::Borrowed("-- TRANSLATIONS --"),
        Dialog::None => match &state.bg {
            Bg::Splash(s) => match s.mode {
                crate::ui::splash::SplashMode::Normal => Cow::Borrowed("-- NORMAL --"),
                crate::ui::splash::SplashMode::Filter => Cow::Borrowed("-- FILTER --"),
            },
            // NOREFS is a persistent cue that the sidebar is toggled off, so
            // the reading area looking different on return is self-explained.
            Bg::Reading => {
                let base = if state.focused().visual_anchor.is_some() {
                    "VISUAL"
                } else {
                    "NORMAL"
                };
                // In a compare split, show which pane is focused (e.g. "2/3")
                // instead of the sidebar's NOREFS cue.
                if state.panes.len() >= 2 {
                    Cow::Owned(format!(
                        "-- {base} | {}/{} --",
                        state.focus + 1,
                        state.panes.len()
                    ))
                } else if state.show_sidebar {
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

/// Reading-view footer while a compare split is open: the sidebar toggle is
/// irrelevant (suppressed), so advertise the window-command chords instead.
const STATUS_READING_COMPARE: &[Shortcut<'static>] = &[
    Shortcut {
        key: "Tab",
        action: "Focus",
    },
    Shortcut {
        key: "^Wv",
        action: "Split",
    },
    Shortcut {
        key: "^Wq",
        action: "Close",
    },
    Shortcut {
        key: "K",
        action: "Notes",
    },
    Shortcut {
        key: "Esc",
        action: "Home",
    },
    Shortcut {
        key: "Q",
        action: "Quit",
    },
];

fn make_status(state: &LoopState) -> &'static [Shortcut<'static>] {
    match &state.bg {
        Bg::Splash(_) => STATUS_SPLASH,
        // In a visual selection the relevant actions are copy / bookmark /
        // exit, so swap the reading hints for those (mirrors how the dialogs
        // carry their own mode-specific footers).
        Bg::Reading if state.focused().visual_anchor.is_some() => STATUS_VISUAL,
        Bg::Reading if state.panes.len() >= 2 => STATUS_READING_COMPARE,
        Bg::Reading if state.show_sidebar => STATUS_READING_HIDE,
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
        | Action::SearchPrev
        // Compare-pane actions are handled in `dispatch_reading` directly
        // (they touch LoopState's pane vector, not a single reading context).
        | Action::CompareOpen
        | Action::FocusNext
        | Action::FocusLeft
        | Action::FocusRight
        | Action::CompareClose => Ok(false),
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
    let mut hits = search::search(db, query, search::REPEAT_LIMIT).ok()?;
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
