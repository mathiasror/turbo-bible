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
mod import;
mod install;
mod keys;
mod manifest;
mod nav;
mod paths;
mod poetry;
mod quote;
mod reference;
mod render;
mod search;
mod state;
mod text;
mod theme;
mod ui;
mod update;
mod worddiff;

use std::borrow::Cow;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
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
use ratatui::layout::Rect;

use crate::db::{Book, Db, Passage, TranslationInfo};
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
    long_about = "A Turbo Vision–styled terminal Bible reader. Ships eleven \
public-domain / CC translations across seven languages with instant full-text \
search (FTS5/BM25), side-by-side compare panes, ~430k cross-references, and a \
vim or \"turbo\" keymap profile. Reads offline: the King James Version is \
embedded in the binary, and the other ten translations plus the shared \
cross-references database are fetched on demand and verified against an \
embedded manifest.",
    after_help = r#"Examples:
  turbo-bible                                   Open the splash screen
  turbo-bible --book JHN --chapter 3            Jump straight to a passage
  turbo-bible --translation nb-1930             Start in a specific translation
  turbo-bible import my.json --code xx-ver \
      --name "My Version" --language xx         Import a custom translation

See `turbo-bible import --help` for the import format, or docs/IMPORT.md."#,
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
    /// Build a translation `<code>.db` from a JSON file and install it
    /// into the translations directory. See `docs/IMPORT.md` for the
    /// input format and resulting schema.
    Import(import::ImportArgs),
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
    /// The pane's last-rendered text interior, refreshed every [`draw_frame`]
    /// from [`ui::pane_viewports`]. `wrap_width` feeds the line→verse map and
    /// `viewport_height` sizes half-/full-page motion (`Ctrl-D`/`Ctrl-F`/
    /// `Space`) to the visible rows. Both are 0 before the first draw — paging
    /// falls back to a fixed verse step until then (it can't happen in the run
    /// loop, where a draw always precedes the first key).
    wrap_width: u16,
    viewport_height: u16,
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
            wrap_width: 0,
            viewport_height: 0,
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

/// A live left-button drag in the reading view, tracked between the `Down`
/// that starts it and the `Up` that ends it. `anchor` is the verse the press
/// landed on — the fixed end of the visual selection the drag grows; `edge`
/// records whether the pointer is currently held past the top or bottom of the
/// pane, which drives auto-scroll on idle ticks (crossterm emits no `Drag`
/// while the pointer is held still past an edge, so the scroll has to advance
/// itself).
#[derive(Clone, Copy, Debug)]
struct MouseDrag {
    pane: usize,
    anchor: i64,
    edge: EdgeScroll,
}

/// Which way (if any) a drag is currently spilling past its pane's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeScroll {
    None,
    Up,
    Down,
}

/// Severity of a transient status hint. `Warn` paints the status pill red and
/// lingers a little longer so a refusal isn't missed; `Info` is the neutral,
/// quick-fading default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransientKind {
    Info,
    Warn,
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
    /// Transient one-line status hint (e.g. "Too narrow…") with its set-time
    /// and severity, cleared after a short delay by [`LoopState::tick`]
    /// (warnings linger a touch longer and paint the status pill red).
    transient_msg: Option<(String, Instant, TransientKind)>,
    /// In-flight background download (a translation, or the shared xrefs DB),
    /// if any. Polled each loop turn by [`poll_download`]; rendered as an
    /// animated `-- Downloading … --` mode tag. Only one runs at a time.
    download: Option<DownloadJob>,
    /// In-flight startup update check, if one was spawned. Polled each loop
    /// turn by [`poll_update_check`]; on a newer-version result it seeds the
    /// splash banner. Only spawned on the splash screen. See [`crate::update`].
    update_check: Option<UpdateCheckJob>,
    max_reading_width: u16,
    /// Highlight the words that diverge between same-language compare panes.
    /// Initial state from `[reading] compare_word_diff`; `Ctrl-W d` flips it
    /// for the session. Only has a visible effect while ≥2 panes are open.
    compare_word_diff: bool,
    keys: KeyState,
    /// In-flight reading-view mouse drag, if the left button is down. `None`
    /// when no drag is active. See [`MouseDrag`].
    mouse_drag: Option<MouseDrag>,
}

/// What a background download is fetching, and how to apply it once the
/// bytes are on disk. Both variants share the single in-flight slot and the
/// same animated indicator; only the apply step differs (see [`poll_download`]).
enum DownloadKind {
    /// A translation `<code>.db`. On success the new connection is registered
    /// with the [`Db`] and the picker's pick is applied per `intent`.
    Translation {
        /// Translation code being fetched (e.g. `nb-1930`).
        code: String,
        /// Whether the originating picker meant to switch the focused pane
        /// (`t`) or open a new compare pane (`Ctrl-W v`), captured so reopening
        /// the picker with a different intent mid-fetch doesn't redirect this
        /// apply. Only the *intent* is captured, not the originating pane: a
        /// `SwitchFocused` apply still resolves against the live `state.focus`,
        /// so moving focus while the fetch runs lands the switch on whatever
        /// pane is focused when the result arrives (no crash — just a
        /// different target).
        intent: PickerIntent,
    },
    /// The shared `xrefs.db`, triggered from the K-popup affordance when the
    /// cross-references dataset isn't installed yet. On success it's
    /// re-ATTACHed onto every connection and the visible passages are reloaded
    /// so markers, the sidebar, and the popup reflect the new data.
    Xrefs,
}

impl DownloadKind {
    /// Display name for the animated indicator and the outcome copy
    /// (`nb-1930` / `cross-references`).
    fn display_name(&self) -> &str {
        match self {
            Self::Translation { code, .. } => code,
            Self::Xrefs => "cross-references",
        }
    }
}

/// A background download running on a worker thread. The worker does only
/// the filesystem-bound fetch (`curl` + sha256 + zstd-decompress + atomic
/// rename); it never touches the [`Db`], so the connection set stays
/// single-threaded. The main loop applies the result — registering a
/// connection or re-attaching xrefs — when it lands (see [`poll_download`]).
struct DownloadJob {
    /// What's being fetched and how to apply it.
    kind: DownloadKind,
    /// When the worker was spawned — drives the indicator's animation.
    started: Instant,
    /// Yields exactly one value: `Ok(())` when the `.db` is installed, or
    /// the fetch error. A disconnect without a value means the worker
    /// panicked.
    rx: std::sync::mpsc::Receiver<Result<()>>,
}

/// A notify-only update check running on a worker thread. The worker does
/// only the network-bound `curl` to the `releases/latest` redirect and parses
/// the tag; it never touches the [`Db`] or the UI. The main loop drains the
/// one-shot result via [`poll_update_check`] and, if it's newer, seeds the
/// splash banner. See [`crate::update`].
struct UpdateCheckJob {
    /// Yields exactly one value: the newest release [`update::Version`], or the
    /// error (offline / curl missing / unparseable). A disconnect without a
    /// value means the worker panicked — treated as a silent failure.
    rx: std::sync::mpsc::Receiver<Result<update::Version>>,
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
    match &args.command {
        Some(Commands::Install(install_args)) => return install::run(install_args),
        Some(Commands::Import(import_args)) => return import::run(import_args),
        None => {}
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

    // Resolve persisted state for the Continue option. Only offer it when the
    // persisted book actually exists in the active translation — a partial /
    // imported translation may not contain it, and resuming into a missing
    // book would fail to load.
    let last_for_splash: Option<(Position, String)> = persisted
        .as_ref()
        .filter(|ps| ps.translation == translation)
        .and_then(|ps| {
            let b = books.iter().find(|b| b.code == ps.book)?;
            Some((
                Position {
                    book: ps.book.clone(),
                    chapter: ps.chapter,
                    verse: None,
                },
                format!("{} {}:{}", b.name, ps.chapter, ps.verse),
            ))
        });

    // Starting screen: if --book was passed explicitly, go straight to reading.
    let mut guard = TerminalGuard::init()?;
    let final_pos: Option<Position>;
    let final_cursor_verse: i64;
    let result = if let Some(book_code) = args.book.clone() {
        if !books.iter().any(|b| b.code == book_code) {
            bail!("book {book_code:?} is not in translation {translation:?}");
        }
        let mut pos = Position {
            book: book_code,
            chapter: args.chapter,
            verse: None,
        };
        pos.chapter = clamp_chapter(&db, &pos.book, pos.chapter)?;
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
        // We still need *some* initial passage state for the run loop.
        // `last_for_splash` is already known to exist in this translation; the
        // fallback default may not (partial/imported translation), so clamp to
        // the first available book rather than a hard-coded Genesis.
        let mut pos = match &last_for_splash {
            Some((p, _)) => p.clone(),
            None => initial_book_position(&books),
        };
        pos.chapter = clamp_chapter(&db, &pos.book, pos.chapter)?;
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

    // Only the splash screen carries the update banner; launching straight
    // into a passage (`--book`) means `Bg::Reading` and no check at all.
    if matches!(state.bg, Bg::Splash(_)) {
        start_update_check(&mut state, config);
    }

    loop {
        // Expire transient status messages on every iteration: their TTL is
        // wall-clock based, so leaving this only on the idle branch let a
        // transient overstay under a continuous event stream (mouse drag/move)
        // until input paused.
        state.tick();

        draw_frame(term, &mut state, db)?;

        // Apply a finished background download (non-blocking) before
        // gating on input, so a completed fetch lands within one poll
        // interval whether or not the user is touching the keyboard.
        poll_download(&mut state, db, warnings)?;
        // Likewise drain a finished update check; it only ever seeds the
        // splash banner, never blocks.
        poll_update_check(&mut state, warnings);

        if event::poll(Duration::from_millis(150))? {
            let size = term.size().unwrap_or(ratatui::layout::Size {
                width: 80,
                height: 24,
            });
            let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
            let raw_event = event::read()?;
            let mut ctx = AppCtx { db, warnings };
            let synth: Option<KeyEvent> = match raw_event {
                Event::Key(k) if k.kind == KeyEventKind::Press => Some(k),
                // Passage/splash clicks and drags mutate state directly (they
                // name a pane + verse, which a synthetic key can't); only the
                // scroll wheel and status-bar clicks fall through to a key.
                Event::Mouse(me) => {
                    if handle_mouse(&mut state, &mut ctx, me, area)? {
                        None
                    } else {
                        mouse_to_key(me, area.height, make_status(&state))
                    }
                }
                _ => None,
            };
            if let Some(key) = synth {
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
            // Keep a drag held past the pane edge scrolling while the pointer
            // is still (crossterm emits no Drag events until it moves again).
            state.autoscroll_drag();
        }
    }
}

/// What a finished background download resolved to. The fetch and the
/// `add_translation` registration fail for different reasons, so they get
/// distinct user-facing messages (see [`download_outcome`]).
enum DownloadResult {
    /// The `.db` was fetched and registered with the [`Db`]; apply the pick.
    Ready,
    /// The worker's `curl` + sha256 + zstd-decompress step failed.
    FetchFailed(anyhow::Error),
    /// The fetch succeeded but applying it did not — registering the new
    /// translation connection, or re-ATTACHing the freshly-fetched xrefs DB.
    RegisterFailed(anyhow::Error),
    /// The worker dropped the sender without sending a value — it panicked.
    WorkerExited,
}

/// The user-facing copy for a finished download: a stderr-trail warning
/// (always) and an in-TUI transient hint (always). Pure (no I/O), so the
/// message wiring is unit-testable without a `Db` or a live channel.
struct DownloadMessages {
    /// Detailed line appended to `warnings`, flushed to stderr on quit.
    warning: String,
    /// Short one-liner shown immediately via [`LoopState::set_transient`].
    transient: String,
}

/// A best-effort classification of a failed on-demand fetch, derived from the
/// `.context(...)` frames `fetch.rs` already attaches (issue #66, finding #23).
/// The wire format is `curl` → sha256 → zstd-decompress, and each stage frames
/// its error distinctly, so a substring scan of the [`anyhow::Error`] chain is
/// enough to tell the categories apart without a typed error enum (the repo's
/// "anyhow-only at boundaries" stance — no `thiserror`). Anything unrecognised
/// falls back to [`FetchErrorKind::Other`], which keeps the old generic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchErrorKind {
    /// `curl` isn't installed (the worker couldn't even spawn it).
    CurlMissing,
    /// `curl` ran but failed — no network, DNS, TLS, 404, or a timeout.
    Network,
    /// The bytes arrived but failed the sha256 / decompressed-size gate —
    /// corrupt, truncated, or stale against the embedded manifest.
    Verification,
    /// Unrecognised — keep the generic copy.
    Other,
}

impl FetchErrorKind {
    /// The category-specific, actionable transient. Names the item so the user
    /// knows which download broke; the full cause stays in the warnings trail.
    fn transient(self, name: &str) -> String {
        match self {
            Self::CurlMissing => "curl not found \u{2014} install curl".to_string(),
            Self::Network => {
                format!("{name}: couldn't reach GitHub \u{2014} check your connection")
            }
            Self::Verification => {
                format!("{name}: verification failed \u{2014} corrupt or stale download")
            }
            Self::Other => format!("Download of {name} failed"),
        }
    }
}

/// Walk the [`anyhow::Error`] chain and bucket a failed fetch by category,
/// matching the `.context(...)` frames `fetch.rs` attaches. Verification is
/// checked first because a corrupt download is the most actionable signal;
/// curl-missing before curl-exited because the spawn frame is more specific
/// than the generic exit frame (issue #66, finding #23).
fn classify_fetch_error(e: &anyhow::Error) -> FetchErrorKind {
    let chain: String = e
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let has = |needle: &str| chain.contains(needle);
    // Note: "decompress " (verb + space) matches the zstd-decode frame
    // (`fetch.rs::decode_and_verify`'s `.context("decompress {asset}")`) but NOT
    // the staging "write decompressed {asset}" IO frame, which must stay Other.
    if has("sha256 mismatch") || has("zip bomb") || has("decompressed to") || has("decompress ") {
        FetchErrorKind::Verification
    } else if has("spawn curl") || has("is it installed?") {
        FetchErrorKind::CurlMissing
    } else if has("curl exited") {
        FetchErrorKind::Network
    } else {
        FetchErrorKind::Other
    }
}

/// Map a finished download to its warning + transient copy. Success gets a
/// brief "ready" transient (and no warning); each failure mode gets a message
/// that names what actually broke — fetch vs. registration vs. a dead worker.
fn download_outcome(name: &str, result: &DownloadResult) -> DownloadMessages {
    match result {
        DownloadResult::Ready => DownloadMessages {
            warning: format!("{name} ready"),
            transient: format!("{name} ready"),
        },
        DownloadResult::FetchFailed(e) => DownloadMessages {
            // The warning trail keeps the full cause; the in-TUI transient is
            // category-specific so the user knows whether to retry, check the
            // network, or install curl (issue #66, finding #23).
            warning: format!("download {name} failed: {e}"),
            transient: classify_fetch_error(e).transient(name),
        },
        DownloadResult::RegisterFailed(e) => DownloadMessages {
            warning: format!("registering {name} failed: {e}"),
            transient: format!("Could not open {name}"),
        },
        DownloadResult::WorkerExited => DownloadMessages {
            warning: format!("download {name} failed: worker exited"),
            transient: format!("Download of {name} failed"),
        },
    }
}

/// Drain a finished background download, if one is in flight. Non-blocking:
/// `try_recv` returns immediately whether the worker is still running, has
/// landed a result, or has gone away. On success the new `.db` is registered
/// with the [`Db`] (main-thread only) and the pick is applied using the intent
/// captured when the download began. Every terminal outcome — success or any
/// failure — is surfaced in-TUI via a transient hint and also queued to the
/// stderr-on-quit `warnings` trail, then the in-flight slot is cleared so the
/// next download can start.
fn poll_download(state: &mut LoopState, db: &mut Db, warnings: &mut Vec<String>) -> Result<()> {
    use std::sync::mpsc::TryRecvError;

    // Peek first: if no job is in flight, or the job is still running,
    // return without consuming the slot.
    let recv = match state.download.as_ref() {
        Some(job) => job.rx.try_recv(),
        None => return Ok(()),
    };
    if matches!(recv, Err(TryRecvError::Empty)) {
        return Ok(());
    }
    // The job has produced a terminal value (or its worker has died);
    // take it by value so the rest of the function moves freely.
    let job = state.download.take().expect(
        "guarded by the as_ref() above — `state.download` cannot have transitioned to None \
         between the peek and the take on a single-threaded event loop",
    );
    // The "apply" step differs by kind: a translation registers a new
    // connection; xrefs re-ATTACHes the freshly-downloaded file onto every
    // open connection (the ATTACH is bound at connection-open, so the swap
    // isn't visible until then). Both can fail post-fetch → RegisterFailed.
    let result = match recv {
        Ok(Ok(())) => match &job.kind {
            DownloadKind::Translation { code, .. } => match db.add_translation(code) {
                Ok(()) => DownloadResult::Ready,
                Err(e) => DownloadResult::RegisterFailed(e),
            },
            DownloadKind::Xrefs => {
                let path = db.translations_dir().join("xrefs.db");
                match db.attach_xrefs(&path) {
                    Ok(()) => DownloadResult::Ready,
                    Err(e) => DownloadResult::RegisterFailed(e),
                }
            }
        },
        Ok(Err(e)) => DownloadResult::FetchFailed(e),
        // Worker dropped the sender without a value — it panicked.
        Err(TryRecvError::Disconnected) => DownloadResult::WorkerExited,
        Err(TryRecvError::Empty) => unreachable!("returned early above"),
    };
    let DownloadMessages { warning, transient } =
        download_outcome(job.kind.display_name(), &result);
    // Keep the stderr trail (warnings) and give immediate in-TUI feedback
    // (transient) — the async fetch removed the old synchronous freeze that
    // used to signal "something happened".
    warnings.push(warning);
    state.set_transient(transient);

    if matches!(result, DownloadResult::Ready) {
        match job.kind {
            DownloadKind::Translation { code, intent } => {
                state.picker_intent = intent;
                let mut ctx = AppCtx { db, warnings };
                apply_translation_pick(state, &mut ctx, &code)?;
            }
            // xrefs are now attached: reload every pane so the just-fetched
            // refs show up (markers, sidebar, and a re-opened K-popup).
            DownloadKind::Xrefs => reload_panes_after_xrefs(state, db)?,
        }
    }
    Ok(())
}

/// Re-load every pane's current chapter after the cross-references DB was
/// swapped in, so xref markers, the References sidebar, and the K-popup all
/// reflect the freshly-attached data (the passages loaded before the swap
/// carry the empty stand-in's zero refs). Each pane reloads in its *own*
/// translation; cursor and scroll are untouched — only the passage payload
/// refreshes.
fn reload_panes_after_xrefs(state: &mut LoopState, db: &Db) -> Result<()> {
    for pane in &mut state.panes {
        pane.passage = db.load_passage_for(&pane.translation, &pane.pos.book, pane.pos.chapter)?;
    }
    Ok(())
}

/// Resolve how this binary was installed, defaulting to the curl/manual hint
/// when the executable path can't be read (the safest fallback — re-running
/// the installer always works).
fn current_install_method() -> update::InstallMethod {
    std::env::current_exe().map_or(update::InstallMethod::CurlOrManual, |p| {
        update::detect_install_method(&p)
    })
}

/// Seed the splash banner from the cache (offline-graceful) and, if the 24h
/// window has elapsed and checks aren't opted out, spawn a worker thread to
/// fetch the latest release tag. Best-effort throughout: a missing current
/// version or a disabled check simply leaves `state.update_check` as `None`.
fn start_update_check(state: &mut LoopState, config: &config::Config) {
    let Some(current) = update::Version::current() else {
        return;
    };
    let method = current_install_method();
    let cache = update::load_cache();

    // Show a previously-discovered update straight away, even offline.
    if let Some(text) = update::cached_banner(&cache, current, method)
        && let Bg::Splash(s) = &mut state.bg
    {
        s.set_update_banner(text);
    }

    if !update::should_spawn(config.updates.check, &cache, update::now_unix()) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(update::latest_release_tag());
    });
    state.update_check = Some(UpdateCheckJob { rx });
}

/// Drain a finished update check (non-blocking). On a newer-than-current
/// result it records the discovery in the cache and seeds the splash banner;
/// any failure (offline / curl missing / panic) is silent and — crucially —
/// does NOT touch the cache, so the check is retried on the next launch
/// rather than being throttled away for 24h after a transient outage.
fn poll_update_check(state: &mut LoopState, warnings: &mut Vec<String>) {
    use std::sync::mpsc::TryRecvError;

    let recv = match state.update_check.as_ref() {
        Some(job) => job.rx.try_recv(),
        None => return,
    };
    if matches!(recv, Err(TryRecvError::Empty)) {
        return;
    }
    state.update_check = None;

    let Ok(Ok(latest)) = recv else {
        return; // fetch error or worker panic: silent, retry next launch.
    };

    // A successful check is authoritative: record it (throttles the next 24h)
    // whether or not it's newer.
    let cache = update::UpdateCache {
        last_checked_unix: update::now_unix(),
        latest_seen: latest.to_string(),
    };
    if let Err(e) = update::write_cache(&cache) {
        warnings.push(format!("update cache save failed: {e}"));
    }

    let Some(current) = update::Version::current() else {
        return;
    };
    if update::is_newer(latest, current)
        && let Bg::Splash(s) = &mut state.bg
    {
        s.set_update_banner(update::banner_text(&latest, current_install_method()));
    }
}

/// The animated mode-tag text for an in-flight download — a trailing
/// ellipsis that grows 0→3 dots roughly every 300 ms so the user sees the
/// UI is alive, not frozen. Pure (time in, text out) so it's unit-testable.
fn download_label(name: &str, elapsed: Duration) -> String {
    let dots = (elapsed.as_millis() / 300) % 4;
    // `dots` is `% 4`, so always 0..=3; the `try_from` (over a plain `as`)
    // dodges `clippy::cast_possible_truncation` and the `unwrap_or` is
    // unreachable — the value always fits a `usize`.
    format!(
        "-- Downloading {name}{} --",
        ".".repeat(usize::try_from(dots).unwrap_or(0))
    )
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
            download: None,
            update_check: None,
            max_reading_width: config.reading.max_width,
            compare_word_diff: config.reading.compare_word_diff,
            keys,
            mouse_drag: None,
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

    /// Set a transient *info* hint, shown briefly then cleared by [`Self::tick`].
    fn set_transient(&mut self, msg: impl Into<String>) {
        self.transient_msg = Some((msg.into(), Instant::now(), TransientKind::Info));
    }

    /// Set a transient *warning* hint — painted red in the status pill and held
    /// a little longer than an info hint so a refusal isn't missed.
    fn set_transient_warn(&mut self, msg: impl Into<String>) {
        self.transient_msg = Some((msg.into(), Instant::now(), TransientKind::Warn));
    }

    /// Per-poll housekeeping: advance the key-chord timeout and expire any
    /// transient status hint. Warnings linger longer than info hints.
    fn tick(&mut self) {
        self.keys.tick();
        let expired = self
            .transient_msg
            .as_ref()
            .is_some_and(|(_, set_at, kind)| {
                let ttl = match kind {
                    TransientKind::Warn => Duration::from_secs(4),
                    TransientKind::Info => Duration::from_secs(2),
                };
                set_at.elapsed() > ttl
            });
        if expired {
            self.transient_msg = None;
        }
    }

    /// Advance a drag that's spilling past its pane's edge by one verse, so
    /// holding the pointer below (or above) the pane keeps growing the visual
    /// selection even though crossterm emits no further `Drag` events while the
    /// pointer is still. Called on each idle poll tick; a no-op unless a drag is
    /// active with a live [`EdgeScroll`] direction. The derived scroll follows
    /// the cursor on the next draw, revealing the newly-selected verse.
    fn autoscroll_drag(&mut self) {
        let Some(drag) = self.mouse_drag else { return };
        let step = match drag.edge {
            EdgeScroll::Up => -1,
            EdgeScroll::Down => 1,
            EdgeScroll::None => return,
        };
        // Guard the pane index: a pane could in principle have closed between
        // events (it can't mid-drag today, but stay defensive).
        let Some(pane) = self.panes.get_mut(drag.pane) else {
            self.mouse_drag = None;
            return;
        };
        let last = pane.passage.verses.last().map_or(1, |v| v.number);
        pane.cursor_verse = (pane.cursor_verse + step).clamp(1, last);
        pane.visual_anchor = Some(drag.anchor);
    }

    /// Whether the terminal is wide enough to add one more reading pane
    /// without dropping any column below [`ui::MIN_PANE_W`]. A width of 0
    /// means "not measured yet" (no draw has happened, or a sizeless PTY);
    /// allow it rather than block on an unknown — the user can always close.
    fn can_add_pane(&self) -> bool {
        pane_fits_width(self.last_term_width, self.panes.len())
    }

    /// Whether the references sidebar is *actually* on screen: the user asked
    /// for it, no compare split is suppressing it, and the terminal is wide
    /// enough to fit it. Drives the mode pill and footer so a width-suppressed
    /// sidebar reads differently from one toggled off — and so `Tab` never
    /// looks dead (issue #66, finding #5). `last_term_width == 0` (not yet
    /// measured) reports not-visible, matching the centered single-pane layout.
    fn sidebar_visible(&self) -> bool {
        self.show_sidebar
            && self.panes.len() < 2
            && ui::sidebar_fits(self.last_term_width, self.max_reading_width)
    }
}

/// Pure width guard behind [`LoopState::can_add_pane`]: would going from
/// `current_panes` to `current_panes + 1` keep every column at or above
/// [`ui::MIN_PANE_W`]? A `total_width` of 0 means "not measured yet" (no draw
/// has happened, or a sizeless PTY) — allow it rather than block on an unknown.
/// Factored out of the method so it's directly unit-testable without
/// constructing a full [`LoopState`].
fn pane_fits_width(total_width: u16, current_panes: usize) -> bool {
    if total_width == 0 {
        return true;
    }
    ui::min_pane_interior(total_width, current_panes + 1) >= ui::MIN_PANE_W
}

/// The status-bar tag and whether it should paint the high-attention red
/// `warn` pill. An in-flight download, then a transient hint (e.g. "Too
/// narrow"), then the mode pill — first match wins the tag slot. Only a `Warn`
/// transient that actually won the slot reddens the pill (a download keeps the
/// neutral pill).
fn status_tag(state: &LoopState) -> (Cow<'static, str>, bool) {
    state.download.as_ref().map_or_else(
        // No download in flight: a transient hint outranks the mode pill, and
        // only a `Warn` transient reddens the pill.
        || match &state.transient_msg {
            Some((msg, _, kind)) => (
                Cow::Owned(format!("-- {msg} --")),
                *kind == TransientKind::Warn,
            ),
            None => (mode_tag_for(state), false),
        },
        // A download owns the tag slot and keeps the neutral pill.
        |job| {
            (
                Cow::Owned(download_label(
                    job.kind.display_name(),
                    job.started.elapsed(),
                )),
                false,
            )
        },
    )
}

/// The cross-pane word-diff inputs — the diverging-word spans for each pane,
/// computed where every pane's text is in hand. Returns one `PaneDiff` per
/// pane; the caller gates this on `comparing && compare_word_diff`.
fn compute_pane_diffs(panes: &[Pane], db: &Db) -> Vec<worddiff::PaneDiff> {
    let language_of = |code: &str| {
        db.translations()
            .iter()
            .find(|t| t.code == code)
            .map_or("", |t| t.language.as_str())
    };
    // Owned per-pane verse lists, borrowed by the `DiffInput`s below.
    let verse_lists: Vec<Vec<(i64, &str)>> = panes
        .iter()
        .map(|p| {
            p.passage
                .verses
                .iter()
                .map(|v| (v.number, v.text.as_str()))
                .collect()
        })
        .collect();
    let inputs: Vec<worddiff::DiffInput<'_>> = panes
        .iter()
        .zip(&verse_lists)
        .map(|(p, verses)| worddiff::DiffInput {
            language: language_of(&p.translation),
            book_code: &p.passage.book_code,
            chapter: p.passage.chapter,
            verses,
        })
        .collect();
    worddiff::compute(&inputs)
}

/// Assemble the per-pane render inputs. Each `PaneRender` borrows its pane's
/// passage, bookmark set, and word diff; `focused_verse` is echoed into the
/// unfocused panes as a read-only cross-pane locator (only while comparing).
fn build_pane_renders<'a>(
    panes: &'a [Pane],
    bookmarks_cache: &'a std::collections::HashMap<BookmarksKey, std::collections::BTreeSet<i64>>,
    empty_bookmarks: &'a std::collections::BTreeSet<i64>,
    pane_diffs: &'a [worddiff::PaneDiff],
    focus: usize,
    comparing: bool,
    focused_verse: i64,
) -> Vec<ui::PaneRender<'a>> {
    panes
        .iter()
        .enumerate()
        .map(|(i, pane)| {
            let key = (
                pane.passage.translation.clone(),
                pane.passage.book_code.clone(),
                pane.passage.chapter,
            );
            let bookmarked = bookmarks_cache.get(&key).unwrap_or(empty_bookmarks);
            let selection = pane.visual_anchor.map(|a| {
                let c = pane.cursor_verse;
                if a <= c { (a, c) } else { (c, a) }
            });
            let is_focused = i == focus;
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
                word_diff: pane_diffs.get(i),
            }
        })
        .collect()
}

/// One pass of the draw cycle. The per-pane render inputs are assembled up
/// front by [`compute_pane_diffs`] and [`build_pane_renders`] (while we still
/// hold `&mut state`); the `term.draw` closure itself stays inline because it
/// borrows many `state` fields and the dialog overlay match doesn't factor out
/// cleanly.
fn draw_frame(term: &mut Tty, state: &mut LoopState, db: &Db) -> Result<()> {
    // Refresh the open-pane width guard, per-pane render geometry, and the
    // per-pane bookmark caches up front, while we still hold `&mut state` — the
    // draw closure below borrows `state` immutably.
    let size = term.size().unwrap_or_default();
    state.last_term_width = size.width;
    // Each pane's on-screen text interior, from the same layout the draw uses,
    // so viewport-relative paging (Ctrl-D/F/Space) tracks the visible rows.
    // (The splash ignores these; they're only read by the reading view, where
    // this layout matches what's drawn.)
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let viewports = ui::pane_viewports(
        area,
        state.panes.len(),
        state.max_reading_width,
        state.show_sidebar,
    );
    for (pane, (w, h)) in state.panes.iter_mut().zip(viewports) {
        pane.wrap_width = w;
        pane.viewport_height = h;
    }
    state.refresh_all_bookmark_caches();

    let status = make_status(state);
    let (mode_tag, status_warn) = status_tag(state);
    let menu_title = format!(" Turbo Bible \u{00B7} {} ", state.translation_label);

    // Per-pane render inputs. `empty_bookmarks` covers the can't-happen cache
    // miss so a gap degrades to "no bookmark stars" rather than a panic.
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
    // Cross-pane word diff only while comparing with the toggle on; otherwise
    // an empty Vec so every `.get(i)` is `None` and the panes render as before.
    let pane_diffs = if comparing && state.compare_word_diff {
        compute_pane_diffs(&state.panes, db)
    } else {
        Vec::new()
    };
    let pane_renders = build_pane_renders(
        &state.panes,
        &state.bookmarks_cache,
        &empty_bookmarks,
        &pane_diffs,
        state.focus,
        comparing,
        focused_verse,
    );

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
                    status_warn,
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
                    status_warn,
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
    // Remember what was requested so we can tell if jump_to had to clamp it.
    let req_chapter = p.chapter;
    let req_verse = p.verse;
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
    // If the requested chapter/verse was out of range (e.g. `:John 999`),
    // jump_to clamps it — say where it actually landed instead of silently
    // dropping the user somewhere else (issue #66, finding #11).
    let landed_ch = state.panes[f].pos.chapter;
    let landed_v = state.panes[f].cursor_verse;
    if req_chapter != landed_ch || req_verse.is_some_and(|v| v != landed_v) {
        let name = state
            .books
            .iter()
            .find(|b| b.code == state.panes[f].pos.book)
            .map_or_else(|| state.panes[f].pos.book.clone(), |b| b.name.clone());
        state.set_transient(format!("Clamped to {name} {landed_ch}:{landed_v}"));
    }
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
                state.dialog = Dialog::Help(HelpDialog::new(state.keys.keymap()));
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
            FootnoteOutcome::FetchXrefs => {
                // `d` on the affordance: download xrefs.db off the event loop,
                // exactly like the translation picker (curl + sha256 +
                // zstd-decompress on a worker thread, drained by
                // `poll_download`), sharing the single in-flight slot. On
                // success poll_download re-ATTACHes the file and reloads the
                // panes so refs appear; close the popup now — the animated
                // "-- Downloading cross-references… --" pill takes the status
                // slot, and the user re-opens `K` to read the landed refs.
                if state.download.is_some() {
                    state.set_transient("A download is already in progress");
                } else {
                    let dir = ctx.db.translations_dir().to_path_buf();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        // Receiver gone (app quit) just drops the result.
                        let _ = tx.send(fetch::xrefs(&dir));
                    });
                    state.download = Some(DownloadJob {
                        kind: DownloadKind::Xrefs,
                        started: Instant::now(),
                        rx,
                    });
                }
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
                    // Fetch off the event loop: a worker thread runs curl +
                    // sha256 + zstd-decompress (~4 MB, multi-second on a slow
                    // link) and sends the result back over a channel that
                    // `poll_download` drains each turn. The UI keeps painting
                    // an animated "Downloading…" tag and stays responsive
                    // instead of freezing. The worker only writes the `.db`
                    // file; the connection is registered on the main thread
                    // when the result lands, so `Db` stays single-threaded.
                    if state.download.is_some() {
                        state.set_transient("A download is already in progress");
                    } else {
                        let dir = ctx.db.translations_dir().to_path_buf();
                        let (tx, rx) = std::sync::mpsc::channel();
                        let code_for_worker = code.clone();
                        std::thread::spawn(move || {
                            // Receiver gone (app quit) just drops the result.
                            let _ = tx.send(fetch::translation(&dir, &code_for_worker));
                        });
                        state.download = Some(DownloadJob {
                            kind: DownloadKind::Translation {
                                code,
                                intent: state.picker_intent,
                            },
                            started: Instant::now(),
                            rx,
                        });
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
            &mut pane.pos,
            &mut pane.passage,
            &mut pane.cursor_verse,
        )?;
        pane.translation = code.to_string();
        // The jump history still holds positions from the *previous*
        // translation; a book/chapter valid there may be absent here, and
        // `history_step` loads without clamping — so a stale entry could blank
        // the pane or (for an imported partial translation) error out of the
        // run loop on Ctrl-O. Reseed history on the landed position, exactly as
        // `Pane::new` does for a fresh pane.
        pane.history = History::new(pane.pos.clone());
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
        state.set_transient_warn(format!(
            "Too narrow — need {}+ cols (have {})",
            ui::min_total_width(state.panes.len() + 1),
            state.last_term_width
        ));
        return Ok(());
    }
    let (seed_pos, cursor) = if let Some(p) = seed {
        let c = p.verse.unwrap_or(1);
        (p, c)
    } else {
        let fp = state.focused();
        (fp.pos.clone(), fp.cursor_verse)
    };
    // The seed book may be absent from `code` — a partial / imported
    // translation (Ctrl-W v into a John-only edition), or an xref target whose
    // canonical KJV book the source translation doesn't carry. `*_clamped_for`
    // falls back to the translation's first book and clamps the chapter rather
    // than erroring out of the run loop (which would crash the whole TUI).
    let passage = ctx
        .db
        .load_passage_clamped_for(code, &seed_pos.book, seed_pos.chapter)?;
    // Sync the pane's position to what actually loaded — the clamp may have
    // landed on a different book/chapter than requested.
    let mut seed_pos = seed_pos;
    let book_changed = seed_pos.book != passage.book_code;
    seed_pos.book.clone_from(&passage.book_code);
    seed_pos.chapter = passage.chapter;
    // Reset the cursor when the book changed: the seed verse belongs to the
    // requested book, not the fallback one.
    let cursor = if book_changed { 1 } else { cursor };
    let mut pane = Pane::new(code.to_string(), seed_pos, passage, cursor);
    pane.origin_label = origin;
    pane.clamp_cursor();
    state.panes.push(pane);
    state.focus = state.panes.len() - 1;
    // The sidebar shares the body width the new pane needs; suppress it
    // while comparing (restored from `sidebar_pref` when the split closes).
    // The "References sidebar hidden while comparing" hint that used to fire
    // here as a 2s transient lingered into every compare-mode screenshot
    // (review C-2: it displaced the mode pill's `2/2` focus indicator); it
    // now lives in F1 help under the Compare panes section instead.
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
    apply_splash_outcome(state, ctx, outcome)
}

/// Apply a [`SplashOutcome`] — opening a book, resuming, or surfacing a dialog.
/// Shared by the keyboard ([`dispatch_splash`]) and the mouse click path so a
/// clicked book opens through the exact same load + history + bg-transition
/// steps as `Enter`.
fn apply_splash_outcome(
    state: &mut LoopState,
    ctx: &mut AppCtx,
    outcome: SplashOutcome,
) -> Result<DispatchStep> {
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
            state.dialog = Dialog::Help(HelpDialog::new(state.keys.keymap()));
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
    /// Open the `K` notes/cross-reference popup for the cursor verse.
    /// `can_fetch_xrefs` (the caller's `Db::has_xrefs()` negated) turns the
    /// popup into a one-key download affordance when the cross-references
    /// dataset isn't installed yet — see [`FootnoteDialog`].
    fn open_footnote_dialog(&mut self, can_fetch_xrefs: bool) {
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
        // When the xrefs dataset is installed (not fetchable) but this verse has
        // nothing to show, prefer a transient over an empty modal — opening a
        // popup that just says "(none)" is worse than a one-line cue (issue #66,
        // finding #22). The fetch-affordance path (can_fetch_xrefs == true)
        // still opens: that "empty-ish" popup intentionally offers `d` to
        // download (kept from #67).
        if !can_fetch_xrefs && notes.is_empty() && xrefs.is_empty() {
            self.set_transient("No cross-references for this verse");
            return;
        }
        self.dialog = Dialog::Footnote(FootnoteDialog::new(label, notes, xrefs, can_fetch_xrefs));
    }

    fn history_step(&mut self, ctx: &mut AppCtx, dir: HistoryDir) -> Result<()> {
        let pane = &mut self.panes[self.focus];
        let target = match dir {
            HistoryDir::Back => pane.history.back(),
            HistoryDir::Forward => pane.history.forward(),
        };
        if let Some(p) = target {
            // Restore the stored cursor verse so a `Ctrl-O`/`Ctrl-I` round-trip
            // lands where you left, like vim's jumplist — not always verse 1
            // (issue #66, finding #4). Clamp to the passage in case a stored
            // entry out-ranges a re-loaded chapter.
            let target_verse = p.verse;
            pane.pos = p;
            pane.passage = ctx.db.load_passage(&pane.pos.book, pane.pos.chapter)?;
            pane.cursor_verse = target_verse.unwrap_or(1).clamp(1, max_verse(&pane.passage));
        }
        Ok(())
    }

    fn copy_verse(&mut self, ctx: &mut AppCtx) {
        let (label, result) = {
            let pane = self.focused();
            let label = format!(
                "{} {}:{}",
                pane.passage.book_name, pane.pos.chapter, pane.cursor_verse
            );
            (
                label,
                copy_verse_to_clipboard(&pane.passage, &pane.pos, pane.cursor_verse),
            )
        };
        // Confirm in-app on success and surface a failure right away, instead
        // of only logging to stderr at exit — on SSH/headless the user would
        // otherwise press `y`, see nothing, and learn it failed after quitting
        // (issue #66, finding #9).
        match result {
            Ok(()) => self.set_transient(format!("Copied {label}")),
            Err(e) => {
                self.set_transient_warn(format!("Copy failed: {e}"));
                ctx.warnings.push(format!("clipboard set: {e}"));
            }
        }
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
        // Reference label for the confirmation, built before `book` moves into
        // the Bookmark below.
        let name = self
            .books
            .iter()
            .find(|b| b.code == book)
            .map_or(book.as_str(), |b| b.name.as_str());
        let label = if s == e {
            format!("{name} {chapter}:{s}")
        } else {
            format!("{name} {chapter}:{s}\u{2013}{e}")
        };
        // `b` toggles: a second press on an already-bookmarked range removes
        // it, so the reading view gets an un-bookmark without the `M` dialog —
        // and either way a transient confirms what happened (issue #66, #8).
        let added = self.bookmarks.toggle(bookmark::Bookmark {
            translation,
            book,
            chapter,
            start_verse: s,
            end_verse: e,
            label: None,
            created_at: bookmark::now_unix(),
        });
        self.bookmarks_cache.clear();
        save_or_warn(
            ctx.warnings,
            "bookmarks save (toggle)",
            self.bookmarks.save(),
        );
        self.panes[self.focus].visual_anchor = None;
        self.set_transient(if added {
            format!("Bookmarked {label}")
        } else {
            format!("Removed bookmark {label}")
        });
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
        // Prune to the currently-visible panes first so the map stays
        // O(panes): without this, every chapter ever rendered would linger
        // until the next bookmark mutation cleared the whole cache.
        let live: std::collections::HashSet<BookmarksKey> = self
            .panes
            .iter()
            .map(|p| {
                (
                    p.passage.translation.clone(),
                    p.passage.book_code.clone(),
                    p.passage.chapter,
                )
            })
            .collect();
        self.bookmarks_cache.retain(|key, _| live.contains(key));
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
        let Some(q) = self.last_query.clone() else {
            // No `/`-search has run yet — vim's E35 (issue #66, finding #10).
            self.set_transient_warn("No previous search");
            return Ok(());
        };
        let f = self.focus;
        let outcome = {
            let pane = &self.panes[f];
            repeat_search(ctx.db, &q, &pane.pos, pane.cursor_verse, forward)
        };
        let Some((p, wrapped)) = outcome else {
            self.set_transient_warn("Pattern not found");
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
        // Mirror vim's wrap message so the user knows `n`/`N` looped around.
        if wrapped {
            self.set_transient(if forward {
                "search hit BOTTOM, continuing at TOP"
            } else {
                "search hit TOP, continuing at BOTTOM"
            });
        }
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
        Action::OpenHelp => state.dialog = Dialog::Help(HelpDialog::new(state.keys.keymap())),
        // Offer the fetch affordance only when the xrefs dataset isn't on disk
        // (the empty stand-in, not the real openbible.info data).
        Action::OpenFootnote => state.open_footnote_dialog(!ctx.db.has_xrefs()),
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
                // If the user just asked for the sidebar but the terminal is
                // too narrow to fit it, Tab would otherwise look dead — say why
                // (issue #66, finding #5).
                if state.show_sidebar && !state.sidebar_visible() {
                    // Warn (red, held longer), matching the analogous
                    // compare-pane "Too narrow" refusal — both are
                    // "you asked, can't, here's why".
                    let need = ui::sidebar_min_width(state.max_reading_width);
                    state.set_transient_warn(format!(
                        "Too narrow for sidebar \u{2014} need {need}+ cols (have {})",
                        state.last_term_width
                    ));
                }
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
        Action::ToggleWordDiff => {
            // Session toggle; the `[reading] compare_word_diff` config sets the
            // initial state. A transient confirms it even when no diverging
            // words are on screen. With a single pane the overlay has nothing
            // to act on yet, so the on-hint points at how to see it — mirroring
            // the context-aware `Ctrl-W q` "Only one pane" message.
            state.compare_word_diff = !state.compare_word_diff;
            let msg = match (state.compare_word_diff, state.panes.len() >= 2) {
                (true, true) => "Word diff on",
                (true, false) => "Word diff on \u{2014} open a compare pane (Ctrl-W v) to see it",
                (false, _) => "Word diff off",
            };
            state.set_transient(msg);
        }
        _ => {
            let f = state.focus;
            let result = {
                let pane = &mut state.panes[f];
                let (wrap_width, viewport_height) = (pane.wrap_width, pane.viewport_height);
                apply_action(
                    action,
                    ctx.db,
                    &state.books,
                    &mut pane.pos,
                    &mut pane.passage,
                    &mut pane.cursor_verse,
                    &mut pane.history,
                    wrap_width,
                    viewport_height,
                )?
            };
            match result {
                ActionResult::Quit => return Ok(DispatchStep::Quit),
                // A chapter/book motion that moved nothing because the cursor
                // was already at the very first/last passage in the canon: name
                // the edge so the key doesn't feel broken (issue #66, finding
                // #21). Per-verse j/k clamping stays silent (vim convention).
                ActionResult::Boundary(edge) => state.set_transient(match edge {
                    CanonEdge::Start => "Start of the Bible",
                    CanonEdge::End => "End of the Bible",
                }),
                ActionResult::Continue => {}
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
        Dialog::Footnote(_) => Cow::Borrowed("-- XREFS --"),
        Dialog::Help(_) => Cow::Borrowed("-- HELP --"),
        Dialog::Bookmarks(_) => Cow::Borrowed("-- BOOKMARKS --"),
        Dialog::Translations(_) => Cow::Borrowed("-- TRANSLATIONS --"),
        Dialog::None => match &state.bg {
            Bg::Splash(s) => match s.mode {
                crate::ui::splash::SplashMode::Normal => Cow::Borrowed("-- NORMAL --"),
                crate::ui::splash::SplashMode::Filter => Cow::Borrowed("-- FILTER --"),
            },
            // A persistent cue tells the reader why the sidebar is (or isn't)
            // there: NOREFS = toggled off, NARROW = wanted but the terminal is
            // too narrow to fit it. Both are absent when it's actually visible.
            Bg::Reading => {
                let base = if state.focused().visual_anchor.is_some() {
                    "VISUAL"
                } else {
                    "NORMAL"
                };
                // In a compare split, show which pane is focused (e.g. "2/3");
                // otherwise the sidebar cue (NARROW = too narrow, NOREFS =
                // toggled off, absent = actually visible).
                let cue = if state.panes.len() >= 2 {
                    format!(" | {}/{}", state.focus + 1, state.panes.len())
                } else if state.sidebar_visible() {
                    String::new()
                } else if state.show_sidebar {
                    " | NARROW".to_string()
                } else {
                    " | NOREFS".to_string()
                };
                // showcmd: the in-progress count/chord (vim's last-line cue),
                // so a pending `5` or `g` is visible rather than silent (#66 #7).
                let showcmd = state
                    .keys
                    .pending_hint()
                    .map_or_else(String::new, |h| format!(" {h}"));
                Cow::Owned(format!("-- {base}{cue}{showcmd} --"))
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

// Vim reading footer: the vim-letter hints (`K Notes`, `v Select`) are honest
// here because the vim layer is live.
const STATUS_READING_HIDE: &[Shortcut<'static>] = &reading_shortcuts("Hide");
const STATUS_READING_REFS: &[Shortcut<'static>] = &reading_shortcuts("Refs");

// Turbo reading footer: turbo drops the vim letter keys, so `K`/`v` would be
// dead hints. Surface the F-key/base affordances that actually work instead —
// F4 Marks and Tab Refs/Hide — keeping F1 Help one keystroke away in both
// profiles (issue #66, findings #12 / #13 / #17).
const STATUS_READING_HIDE_TURBO: &[Shortcut<'static>] = &reading_shortcuts_turbo("Hide");
const STATUS_READING_REFS_TURBO: &[Shortcut<'static>] = &reading_shortcuts_turbo("Refs");

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
            action: "Xrefs",
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

const fn reading_shortcuts_turbo(tab_action: &'static str) -> [Shortcut<'static>; 7] {
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
            key: "F4",
            action: "Marks",
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

// VISUAL footer: act-on-selection verbs (Copy, Bookmark) plus a single
// exit row. `V` also leaves VISUAL (vim toggle) but advertising it next to
// `Esc Cancel` reads as duplicate exit verbs — the review's `shot-08`
// finding. Esc is the app-wide exit key (Goto, Find, dialogs all use
// `Esc cancel` / `Esc close`), so it's the one we surface here.
const STATUS_VISUAL: &[Shortcut<'static>] = &[
    // `y` copies only the cursor verse, not the whole selection (the range
    // "isn't yanked in v1" per the README) — so the hint says "Copy verse",
    // not the bare "Copy" that implied the selection (issue #66, finding #2).
    Shortcut {
        key: "y",
        action: "Copy verse",
    },
    Shortcut {
        key: "b",
        action: "Bookmark",
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
        action: "Xrefs",
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
    // The reading footer is profile-aware: in turbo the vim letter keys are
    // off, so we never advertise `K`/`v` there (issue #66, findings #12 / #17).
    // VISUAL and the compare-split footer are vim-only states (turbo can't
    // enter visual or open a pane without a binding), so they stay as-is.
    let turbo = state.keys.keymap() == crate::config::Keymap::Turbo;
    match &state.bg {
        Bg::Splash(_) => STATUS_SPLASH,
        // In a visual selection the relevant actions are copy / bookmark /
        // exit, so swap the reading hints for those (mirrors how the dialogs
        // carry their own mode-specific footers).
        Bg::Reading if state.focused().visual_anchor.is_some() => STATUS_VISUAL,
        Bg::Reading if state.panes.len() >= 2 => STATUS_READING_COMPARE,
        // Drive the Tab label off the *actual* layout: "Hide" only when the
        // sidebar is really on screen, "Refs" otherwise (toggled off or
        // width-suppressed) so pressing Tab matches the verb shown.
        Bg::Reading if state.sidebar_visible() => {
            if turbo {
                STATUS_READING_HIDE_TURBO
            } else {
                STATUS_READING_HIDE
            }
        }
        Bg::Reading => {
            if turbo {
                STATUS_READING_REFS_TURBO
            } else {
                STATUS_READING_REFS
            }
        }
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
    mut p: Position,
    db: &Db,
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    history: &mut History,
) -> Result<()> {
    // Clamp the requested chapter into the book's range before loading, so a
    // Goto like `:John 999`, a short-book overshoot, or a stale bookmark from a
    // differently-versified translation lands on the last chapter instead of an
    // empty passage with a cursor on a nonexistent verse 1. Mirrors the
    // `--chapter` startup path, which already routes through `clamp_chapter`.
    p.chapter = clamp_chapter(db, &p.book, p.chapter)?;
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

/// Which absolute edge of the canon a no-op chapter/book motion ran into, so
/// the caller can name it ("Start of the Bible" / "End of the Bible") (issue
/// #66, finding #21). Per-verse cursor clamping stays silent (vim convention);
/// only chapter/book motions that move *nothing* at the very edge surface this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonEdge {
    Start,
    End,
}

/// Outcome of an [`apply_action`] call. Replaces the old bare `bool` (quit) so
/// a chapter/book motion that no-ops at the absolute canon edge can report it
/// (issue #66, finding #21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionResult {
    /// Keep running; the action moved (or was a non-motion).
    Continue,
    /// `Action::Quit` — end the loop.
    Quit,
    /// A chapter/book motion that made *zero* moves because the cursor was
    /// already at the named canon edge.
    Boundary(CanonEdge),
}

/// Apply a reading-view motion/action to the focused pane's reading context.
/// Returns [`ActionResult::Quit`] to end the loop, [`ActionResult::Boundary`]
/// when a chapter/book motion no-ops at the very first/last passage in the
/// canon, or [`ActionResult::Continue`] otherwise.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "pos is mutated through jump_to in the chapter/book arms below; \
              clippy can't follow the call"
)]
#[allow(
    clippy::too_many_arguments,
    reason = "operates on the focused pane's individual reading-context fields \
              (see the type comment on apply_action's caller); the wrap_width / \
              viewport_height pair sizes paging to the visible rows"
)]
fn apply_action(
    action: Action,
    db: &Db,
    books: &[Book],
    pos: &mut Position,
    passage: &mut Passage,
    cursor_verse: &mut i64,
    history: &mut History,
    wrap_width: u16,
    viewport_height: u16,
) -> Result<ActionResult> {
    let nav_ = Navigator::new(books);
    let last = max_verse(passage);
    // Half-/full-page motion scrolls by the visible row count, so a screenful
    // tracks the terminal size instead of a fixed verse step. Before the first
    // draw (viewport_height == 0 — unreachable in the run loop, where a draw
    // always precedes the first key) fall back to a sane fixed line step.
    let page_lines = if viewport_height == 0 {
        20
    } else {
        i64::from(viewport_height)
    };
    let half_lines = (page_lines / 2).max(1);
    // A page-motion count multiplies the line step (`2Ctrl-F` = two screenfuls);
    // 0 is treated as 1 so a stray count never freezes the motion.
    let count = |n: u16| i64::from(n.max(1));
    match action {
        Action::Quit => Ok(ActionResult::Quit),
        Action::CursorDown(n) => {
            *cursor_verse = (*cursor_verse + i64::from(n)).min(last);
            Ok(ActionResult::Continue)
        }
        Action::CursorUp(n) => {
            *cursor_verse = (*cursor_verse - i64::from(n)).max(1);
            Ok(ActionResult::Continue)
        }
        // Page motions scale the line step by the count (`2Ctrl-D` scrolls two
        // half-pages) rather than re-paging N times (issue #66, finding #15).
        Action::HalfPageDown(n) => {
            *cursor_verse =
                render::verse_after_paging(passage, *cursor_verse, wrap_width, half_lines * count(n));
            Ok(ActionResult::Continue)
        }
        Action::HalfPageUp(n) => {
            *cursor_verse = render::verse_after_paging(
                passage,
                *cursor_verse,
                wrap_width,
                -half_lines * count(n),
            );
            Ok(ActionResult::Continue)
        }
        Action::PageDown(n) => {
            *cursor_verse =
                render::verse_after_paging(passage, *cursor_verse, wrap_width, page_lines * count(n));
            Ok(ActionResult::Continue)
        }
        Action::PageUp(n) => {
            *cursor_verse = render::verse_after_paging(
                passage,
                *cursor_verse,
                wrap_width,
                -page_lines * count(n),
            );
            Ok(ActionResult::Continue)
        }
        Action::GotoTop => {
            *cursor_verse = 1;
            Ok(ActionResult::Continue)
        }
        Action::GotoBottom => {
            *cursor_verse = last;
            Ok(ActionResult::Continue)
        }
        // Chapter / book motions step N times. Each helper returns an
        // unchanged position at the canon edge, so we break once movement
        // stops — capping real work at the canon size regardless of the count
        // (a stray `999l` doesn't grind through thousands of redundant loads)
        // (issue #66, finding #15). When the FIRST step is already at the edge
        // (zero moves), report the boundary so the caller can show "Start/End of
        // the Bible" — but a partial count that moved some then hit the edge is
        // a normal move, not a dead-end (issue #66, finding #21).
        Action::PrevChapter(n) => Ok(step_chapter_or_book(n, CanonEdge::Start, |pos| {
            nav_.prev_chapter(db, pos)
        })
        .run(db, pos, passage, cursor_verse, history)?),
        Action::NextChapter(n) => Ok(step_chapter_or_book(n, CanonEdge::End, |pos| {
            nav_.next_chapter(db, pos)
        })
        .run(db, pos, passage, cursor_verse, history)?),
        Action::PrevBook(n) => {
            Ok(step_chapter_or_book(n, CanonEdge::Start, |pos| nav_.prev_book(pos))
                .run(db, pos, passage, cursor_verse, history)?)
        }
        Action::NextBook(n) => {
            Ok(step_chapter_or_book(n, CanonEdge::End, |pos| nav_.next_book(pos))
                .run(db, pos, passage, cursor_verse, history)?)
        }
        Action::CopyVerse
        | Action::OpenGoto
        | Action::OpenFind
        | Action::OpenFootnote
        | Action::OpenHelp
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
        | Action::CompareClose
        | Action::ToggleWordDiff => Ok(ActionResult::Continue),
    }
}

/// A configured chapter/book step: how many times to advance, which edge a
/// zero-move no-op corresponds to, and the per-step navigator call. Bundled so
/// the four motion arms share one loop (and one boundary check) instead of
/// repeating it (issue #66, finding #21).
struct ChapterBookStep<F> {
    count: u16,
    edge: CanonEdge,
    next: F,
}

const fn step_chapter_or_book<F>(count: u16, edge: CanonEdge, next: F) -> ChapterBookStep<F>
where
    F: Fn(&Position) -> Result<Position>,
{
    ChapterBookStep { count, edge, next }
}

impl<F> ChapterBookStep<F>
where
    F: Fn(&Position) -> Result<Position>,
{
    /// Walk up to `count` steps, jumping the reading context each time the
    /// position actually changes. Returns [`ActionResult::Boundary`] iff the
    /// very first step was already at the canon edge (zero moves); otherwise
    /// [`ActionResult::Continue`] (including a partial count that moved some
    /// then stopped).
    fn run(
        &self,
        db: &Db,
        pos: &mut Position,
        passage: &mut Passage,
        cursor_verse: &mut i64,
        history: &mut History,
    ) -> Result<ActionResult> {
        let mut moved = false;
        for _ in 0..self.count.max(1) {
            let new_pos = (self.next)(pos)?;
            if new_pos.same_chapter(pos) {
                break;
            }
            jump_to(new_pos, db, pos, passage, cursor_verse, history)?;
            moved = true;
        }
        if moved {
            Ok(ActionResult::Continue)
        } else {
            Ok(ActionResult::Boundary(self.edge))
        }
    }
}

/// Step `n`/`N` through the last `/`-search in the **same BM25 relevance
/// order the Find list showed** — not a canonical re-sort — so repeating the
/// search continues the list the user just scrolled (issue #66, finding #18).
///
/// `search::search` already returns hits ordered by `bm25(verse_fts)`; we keep
/// that order and locate the cursor's current `(book, chapter, verse)` in it.
/// `n` steps to the next index, `N` to the previous, wrapping at either end
/// (the `wrapped` bool drives the vim-style "search hit BOTTOM…" cue, issue
/// #66, finding #10). When the cursor isn't sitting on any hit (the user
/// navigated away after the Find), there's no position in the list to advance
/// from, so we start at the first hit going forward / the last going backward,
/// with `wrapped = false`. Returns the target position and the wrap flag;
/// `None` only when the query has no matches at all.
fn repeat_search(
    db: &Db,
    query: &str,
    pos: &Position,
    cursor_verse: i64,
    forward: bool,
) -> Option<(Position, bool)> {
    // Keep BM25 order (do NOT re-sort): this is the order the Find dialog laid
    // out, and `n`/`N` continue exactly that list.
    let hits = search::search(db, query, search::REPEAT_LIMIT).ok()?;
    if hits.is_empty() {
        return None;
    }
    let on = |h: &search::SearchHit| {
        h.book == pos.book && h.chapter == pos.chapter && h.verse == cursor_verse
    };
    let here = hits.iter().position(on);
    let (pick, wrapped) = match here {
        // Cursor is on a hit: step within the BM25 list, wrapping at the edge.
        Some(i) if forward => {
            if i + 1 < hits.len() {
                (hits.get(i + 1), false)
            } else {
                (hits.first(), true)
            }
        }
        Some(i) => {
            if i > 0 {
                (hits.get(i - 1), false)
            } else {
                (hits.last(), true)
            }
        }
        // Cursor isn't on any hit (navigated away): re-enter the list at its
        // start (forward) or end (backward); no wrap.
        None if forward => (hits.first(), false),
        None => (hits.last(), false),
    };
    pick.map(|h| {
        (
            Position {
                book: h.book.clone(),
                chapter: h.chapter,
                verse: Some(h.verse),
            },
            wrapped,
        )
    })
}
fn picker_entries(db: &Db) -> Vec<PickerEntry> {
    merge_picker_entries(db.translations())
}

/// First-launch landing position when there's no resumable state. Prefers
/// Genesis 1 (the first book of a full Bible), but falls back to the first
/// book the active translation actually contains — a partial / imported
/// translation may not include Genesis, and loading a missing book errors.
/// `books` is ordered by canonical `ord` (see [`Db::list_books`]).
fn initial_book_position(books: &[Book]) -> Position {
    let book = books
        .first()
        .map_or_else(|| "GEN".to_string(), |b| b.code.clone());
    Position {
        book,
        chapter: 1,
        verse: None,
    }
}

/// Clamp `chapter` into the book's valid range `[1, chapter_count]`. A
/// persisted or `--chapter` value can point past the end of a book (more
/// likely in a partial / imported translation), which would otherwise open an
/// empty chapter.
fn clamp_chapter(db: &Db, book: &str, chapter: i64) -> Result<i64> {
    let max = db.chapter_count(book)?.max(1);
    Ok(chapter.clamp(1, max))
}

/// Build the picker entry list: every translation the binary knows about
/// (the static manifest), each marked installed iff its `.db` is on disk,
/// followed by any on-disk translations *not* in the manifest — e.g. ones
/// produced by `turbo-bible import`, which would otherwise be reachable
/// only via `--translation`. The latter are always installed (they exist
/// on disk by definition) and carry no download size.
fn merge_picker_entries(installed: &[TranslationInfo]) -> Vec<PickerEntry> {
    use std::collections::HashSet;
    let installed_codes: HashSet<&str> = installed.iter().map(|t| t.code.as_str()).collect();
    let mut entries: Vec<PickerEntry> = crate::manifest::TRANSLATIONS
        .iter()
        .map(|t| PickerEntry {
            code: t.code.to_string(),
            name: t.name.to_string(),
            language: t.language.to_string(),
            installed: installed_codes.contains(t.code),
            compressed_size: t.compressed_size,
        })
        .collect();
    for t in installed {
        if crate::manifest::TranslationManifestEntry::by_code(&t.code).is_none() {
            entries.push(PickerEntry {
                code: t.code.clone(),
                name: t.name.clone(),
                language: t.language.clone(),
                installed: true,
                compressed_size: 0,
            });
        }
    }
    entries
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
    // The atomic swap (with rollback on probe failure) lives on Db itself.
    // Here we own the in-memory mirrors; if the probe succeeds, copy the
    // new values across and clamp the cursor — verse counts can differ
    // between translations (rare in our three editions, but defensive).
    let (new_books, new_label, new_passage) =
        db.try_switch_translation(code, &pos.book, pos.chapter)?;
    *books = new_books;
    *translation_label = new_label;
    // A partial / imported translation may not contain the current book, in
    // which case the swap lands on its first book instead. Sync the position
    // to whatever actually loaded and reset the cursor when the book changed.
    let book_changed = pos.book != new_passage.book_code;
    pos.book.clone_from(&new_passage.book_code);
    pos.chapter = new_passage.chapter;
    *passage = new_passage;
    if book_changed {
        *cursor_verse = 1;
    }
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

/// Handle a mouse event against the reading view or the splash, mutating state
/// directly — a click names a pane and a verse, which a synthetic key can't
/// carry. Returns `true` when the event was consumed; `false` leaves it for
/// [`mouse_to_key`] (the scroll-wheel → ↑/↓ and status-bar-click fallback).
/// `area` is the full terminal rect.
fn handle_mouse(
    state: &mut LoopState,
    ctx: &mut AppCtx,
    me: MouseEvent,
    area: Rect,
) -> Result<bool> {
    // A modal dialog owns the screen: let the scroll wheel / status bar fall
    // through, but don't treat body clicks as passage or splash hits.
    if !matches!(state.dialog, Dialog::None) {
        return Ok(false);
    }
    match me.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The status bar (bottom row) keeps its existing click handling.
            if me.row + 1 == area.height {
                return Ok(false);
            }
            if matches!(state.bg, Bg::Splash(_)) {
                let body = ui::body_area(area);
                let outcome = if let Bg::Splash(s) = &mut state.bg {
                    s.click(body, me.column, me.row)
                } else {
                    return Ok(false);
                };
                // `click` only ever yields `Continue` or `OpenBook` (it can't quit
                // or open a dialog the way a key can), so the resulting step is
                // always `Continue` — there's nothing for the run loop to act on.
                // Apply it for its side effects and report the event consumed.
                apply_splash_outcome(state, ctx, outcome)?;
                Ok(true)
            } else {
                reading_mouse_down(state, ctx, me, area)
            }
        }
        MouseEventKind::Drag(MouseButton::Left) if matches!(state.bg, Bg::Reading) => {
            reading_mouse_drag(state, me, area);
            Ok(true)
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // End any drag. The resulting selection stays put (sticky), so the
            // keyboard can keep extending it — matching `v`/`V` + motion.
            Ok(state.mouse_drag.take().is_some())
        }
        _ => Ok(false),
    }
}

/// Index of the pane whose text interior contains `(col, row)`, if any.
fn pane_at(rects: &[Rect], col: u16, row: u16) -> Option<usize> {
    rects
        .iter()
        .position(|r| col >= r.left() && col < r.right() && row >= r.top() && row < r.bottom())
}

/// The verse at terminal `row` within a pane whose text interior is `rect`, or
/// `None` if the row is past the rendered chapter (e.g. the blank space below a
/// short chapter). Re-renders the pane exactly as the draw does — same wrap
/// width, same cursor-anchored scroll — so the row→verse map matches the
/// screen. Styling inputs don't change the line *count*, so a bare render with
/// no selection recovers the same map the draw laid out.
fn verse_at_pane_point(pane: &Pane, rect: Rect, row: u16) -> Option<i64> {
    let empty = std::collections::BTreeSet::new();
    // Wrap at `rect.width` — the same interior width the draw used. This equals
    // the cached `pane.wrap_width`, but it's re-derived from the very rect the
    // click was hit-tested against, so the two can't desync; don't "simplify"
    // it to read the cached field.
    let rendered = render::render_passage(
        &pane.passage,
        pane.cursor_verse,
        None,
        &empty,
        None,
        rect.width,
    );
    let cursor_line = render::line_index_for_verse(&rendered, pane.cursor_verse);
    let scroll = render::scroll_offset(rendered.len(), cursor_line, rect.height as usize);
    render::verse_at_screen_row(&rendered, scroll, rect.top(), row)
}

/// A left-press in the reading view: focus the clicked pane and move its cursor
/// to the clicked verse. Shift extends the current selection to that verse; a
/// plain click clears any selection (vim's "click moves the cursor"). Records
/// the drag anchor so a following drag can grow a selection from here.
fn reading_mouse_down(
    state: &mut LoopState,
    ctx: &mut AppCtx,
    me: MouseEvent,
    area: Rect,
) -> Result<bool> {
    let rects = ui::pane_content_rects(
        area,
        state.panes.len(),
        state.max_reading_width,
        state.show_sidebar,
    );
    let Some(i) = pane_at(&rects, me.column, me.row) else {
        // Off every pane (menu strip, inter-pane gap): nothing to do.
        state.mouse_drag = None;
        return Ok(false);
    };
    // Click-to-focus, mirroring vim's click-in-another-window behaviour.
    if state.focus != i {
        state.focus = i;
        state.sync_focus_to_db(ctx.db)?;
    }
    let Some(verse) = verse_at_pane_point(&state.panes[i], rects[i], me.row) else {
        // Clicked the pane but below its last verse: focus only, no move, and
        // no drag (a drag from empty space shouldn't select).
        state.mouse_drag = None;
        return Ok(true);
    };
    let shift = me.modifiers.contains(KeyModifiers::SHIFT);
    let pane = &mut state.panes[i];
    if shift {
        // Extend: keep (or set) the anchor, move the cursor to the click.
        let anchor = pane.visual_anchor.unwrap_or(pane.cursor_verse);
        pane.visual_anchor = Some(anchor);
        pane.cursor_verse = verse;
        state.mouse_drag = Some(MouseDrag {
            pane: i,
            anchor,
            edge: EdgeScroll::None,
        });
    } else {
        // Plain click: move the cursor, drop any selection.
        pane.cursor_verse = verse;
        pane.visual_anchor = None;
        state.mouse_drag = Some(MouseDrag {
            pane: i,
            anchor: verse,
            edge: EdgeScroll::None,
        });
    }
    Ok(true)
}

/// A left-drag in the reading view: grow the visual selection from the press
/// anchor to the verse under the pointer, entering visual mode if it wasn't
/// already. A drag past the pane's top/bottom edge arms auto-scroll (see
/// [`LoopState::autoscroll_drag`]); back inside the pane it disarms it. The
/// drag is confined to the pane it started in.
fn reading_mouse_drag(state: &mut LoopState, me: MouseEvent, area: Rect) {
    let Some(mut drag) = state.mouse_drag else {
        return;
    };
    let rects = ui::pane_content_rects(
        area,
        state.panes.len(),
        state.max_reading_width,
        state.show_sidebar,
    );
    let Some(rect) = rects.get(drag.pane).copied() else {
        return;
    };
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    // Edge detection drives idle auto-scroll; clamp the row into the pane so a
    // drag above/below selects to the first/last visible verse, not nothing.
    drag.edge = if me.row < rect.top() {
        EdgeScroll::Up
    } else if me.row >= rect.bottom() {
        EdgeScroll::Down
    } else {
        EdgeScroll::None
    };
    let clamped_row = me.row.clamp(rect.top(), rect.bottom().saturating_sub(1));
    let pane = &mut state.panes[drag.pane];
    if let Some(verse) = verse_at_pane_point(pane, rect, clamped_row) {
        // A drag is a selection: anchor the fixed end, follow with the cursor
        // (even when still on the anchor verse — that's a one-verse selection).
        pane.visual_anchor = Some(drag.anchor);
        pane.cursor_verse = verse;
    }
    state.mouse_drag = Some(drag);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn info(code: &str) -> TranslationInfo {
        TranslationInfo {
            code: code.to_string(),
            name: format!("Name {code}"),
            language: "en".to_string(),
        }
    }

    /// The history stack must carry the cursor-verse hint through back/forward
    /// so `history_step` (Ctrl-O / Ctrl-I) can restore it instead of snapping to
    /// verse 1 — the regression behind issue #66, finding #4.
    #[test]
    fn history_round_trip_preserves_the_cursor_verse() {
        let at = |book: &str, chapter: i64, verse: i64| Position {
            book: book.into(),
            chapter,
            verse: Some(verse),
        };
        let mut h = History::new(at("GEN", 1, 1));
        h.push(at("JHN", 3, 16));
        h.push(at("ROM", 8, 28));
        // Back lands on the stored verse, not a forced 1.
        assert_eq!(h.back().and_then(|p| p.verse), Some(16), "JHN 3:16");
        assert_eq!(h.back().and_then(|p| p.verse), Some(1), "GEN 1:1");
        // Forward returns with the verse intact.
        assert_eq!(h.forward().and_then(|p| p.verse), Some(16), "JHN 3:16");
    }

    /// A click in the reading body must resolve to the verse drawn on that row,
    /// honouring the pane's content-rect offset (not just origin 0,0). Exercises
    /// the reading-side composition — render → scroll → row→verse — through a
    /// real `Pane` and an offset `Rect`, the path `reading_mouse_down` relies on.
    #[test]
    fn verse_at_pane_point_resolves_clicks_through_a_real_pane() {
        let verses = (1..=10)
            .map(|n| db::Verse {
                number: n,
                text: format!("verse {n}"),
                footnote_count: 0,
                xref_note_count: 0,
            })
            .collect();
        let passage = db::Passage {
            translation: "en-kjv".into(),
            book_code: "GEN".into(),
            book_name: "Genesis".into(),
            book_abbrev: "Gen".into(),
            chapter: 1,
            verses,
            headings: vec![],
            footnotes: vec![],
            xrefs: vec![],
        };
        let pane = Pane::new(
            "en-kjv".into(),
            Position {
                book: "GEN".into(),
                chapter: 1,
                verse: None,
            },
            passage,
            1,
        );
        // Content rect offset from the origin (top=3) and tall enough that the
        // 10 short verses don't scroll, so row→line is `row - top`.
        let rect = Rect::new(2, 3, 40, 20);
        // render_passage opens with a blank row, so the interior top snaps to v1.
        assert_eq!(verse_at_pane_point(&pane, rect, rect.top()), Some(1));
        // Line index k (k>0) is verse k for this single-line-per-verse passage.
        assert_eq!(verse_at_pane_point(&pane, rect, rect.top() + 5), Some(5));
        assert_eq!(verse_at_pane_point(&pane, rect, rect.top() + 10), Some(10));
        // Below the last rendered line (11 lines: blank + 10 verses) → no verse.
        assert_eq!(verse_at_pane_point(&pane, rect, rect.top() + 11), None);
        // Above the interior → no verse.
        assert_eq!(verse_at_pane_point(&pane, rect, rect.top() - 1), None);
    }

    #[test]
    fn picker_lists_manifest_marking_installed_and_appends_custom() {
        // en-kjv is bundled/installed; zz-john is an imported translation
        // present on disk but absent from the static manifest.
        let installed = [info("en-kjv"), info("zz-john")];
        let entries = merge_picker_entries(&installed);

        let kjv = entries
            .iter()
            .find(|e| e.code == "en-kjv")
            .expect("manifest entry present");
        assert!(kjv.installed, "en-kjv is on disk → installed");

        let custom = entries
            .iter()
            .find(|e| e.code == "zz-john")
            .expect("imported translation surfaced in picker");
        assert!(custom.installed);
        assert_eq!(custom.name, "Name zz-john");
        assert_eq!(custom.compressed_size, 0);

        // Exactly the manifest set plus the one custom entry, no dupes.
        assert_eq!(entries.len(), manifest::TRANSLATIONS.len() + 1);

        // A manifest translation that isn't on disk is listed, not installed.
        let absent = manifest::TRANSLATIONS
            .iter()
            .find(|t| t.code != "en-kjv")
            .expect("more than one manifest translation");
        let entry = entries
            .iter()
            .find(|e| e.code == absent.code)
            .expect("absent manifest entry still listed");
        assert!(!entry.installed);
    }

    #[test]
    fn pane_fits_width_guards_the_split() {
        // Width 0 means "not measured yet" (sizeless PTY, pre-first-draw):
        // always allow, regardless of current pane count.
        assert!(pane_fits_width(0, 1), "unmeasured width must allow a split");
        assert!(
            pane_fits_width(0, 4),
            "unmeasured width allows at any count"
        );

        // A width too narrow to keep each of n+1 columns at MIN_PANE_W must
        // refuse. Going 1 -> 2 panes needs min_pane_interior(total, 2) >=
        // MIN_PANE_W (40); at total=84 that's (84-1)/2 - 2 = 39 < 40 → refuse.
        assert!(
            ui::min_pane_interior(84, 2) < ui::MIN_PANE_W,
            "precondition: 84 cols yields a sub-readable 2nd column"
        );
        assert!(
            !pane_fits_width(84, 1),
            "84 cols can't fit a readable second pane"
        );

        // A comfortably wide terminal allows the split.
        assert!(
            ui::min_pane_interior(200, 2) >= ui::MIN_PANE_W,
            "precondition: 200 cols is comfortably wide for two panes"
        );
        assert!(pane_fits_width(200, 1), "200 cols fits a second pane");
    }

    #[test]
    fn download_label_animates_ellipsis() {
        // 0..300ms → no dots, then one dot per 300ms window, wrapping at 4.
        assert_eq!(
            download_label("nb-1930", Duration::from_millis(0)),
            "-- Downloading nb-1930 --"
        );
        assert_eq!(
            download_label("nb-1930", Duration::from_millis(350)),
            "-- Downloading nb-1930. --"
        );
        assert_eq!(
            download_label("nb-1930", Duration::from_millis(650)),
            "-- Downloading nb-1930.. --"
        );
        assert_eq!(
            download_label("nb-1930", Duration::from_millis(950)),
            "-- Downloading nb-1930... --"
        );
        // Wraps back to zero dots in the next window.
        assert_eq!(
            download_label("nb-1930", Duration::from_millis(1250)),
            "-- Downloading nb-1930 --"
        );
    }

    #[test]
    fn download_outcome_ready_is_quiet_and_confirms() {
        let m = download_outcome("nb-1930", &DownloadResult::Ready);
        assert_eq!(m.warning, "nb-1930 ready");
        assert_eq!(m.transient, "nb-1930 ready");
    }

    #[test]
    fn download_outcome_fetch_failure_names_the_download() {
        let m = download_outcome(
            "nb-1930",
            &DownloadResult::FetchFailed(anyhow::anyhow!("sha256 mismatch")),
        );
        // Detailed warning carries the cause for the stderr trail; the in-TUI
        // hint is now category-specific (a sha256 mismatch is a verification
        // failure) and names the download (issue #66, finding #23).
        assert_eq!(m.warning, "download nb-1930 failed: sha256 mismatch");
        assert_eq!(
            m.transient,
            "nb-1930: verification failed \u{2014} corrupt or stale download"
        );
    }

    #[test]
    fn download_outcome_register_failure_is_distinct_from_fetch() {
        let m = download_outcome(
            "nb-1930",
            &DownloadResult::RegisterFailed(anyhow::anyhow!("disk full")),
        );
        // A successful fetch that then fails to register must NOT read
        // "download failed" — the bytes are on disk; opening them broke.
        assert_eq!(m.warning, "registering nb-1930 failed: disk full");
        assert_eq!(m.transient, "Could not open nb-1930");
    }

    #[test]
    fn download_outcome_worker_exit_is_surfaced() {
        let m = download_outcome("nb-1930", &DownloadResult::WorkerExited);
        assert_eq!(m.warning, "download nb-1930 failed: worker exited");
        assert_eq!(m.transient, "Download of nb-1930 failed");
    }

    /// The xrefs job borrows the same label/outcome machinery as translations;
    /// lock its user-facing copy so a careless edit can't ship "Downloading
    ///  --" or "Download of xrefs failed" instead of the friendly name.
    #[test]
    fn xrefs_download_kind_copy_is_friendly() {
        let kind = DownloadKind::Xrefs;
        assert_eq!(kind.display_name(), "cross-references");
        assert_eq!(
            download_label(kind.display_name(), Duration::from_millis(0)),
            "-- Downloading cross-references --"
        );
        let ready = download_outcome(kind.display_name(), &DownloadResult::Ready);
        assert_eq!(ready.transient, "cross-references ready");
        let failed = download_outcome(
            kind.display_name(),
            &DownloadResult::FetchFailed(anyhow::anyhow!("no network")),
        );
        assert_eq!(failed.transient, "Download of cross-references failed");
    }

    /// Build a real read-only KJV `Db` from a fresh install dir — the same
    /// pattern `db.rs` tests use. The bundled KJV is embedded, so this needs no
    /// developer-DB precondition.
    fn kjv_db() -> (tempfile::TempDir, Db) {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::install::ensure_installed(tmp.path()).expect("install bundled kjv");
        let db = Db::open_ro(tmp.path(), "en-kjv").expect("open_ro");
        (tmp, db)
    }

    fn at(book: &str, chapter: i64, verse: i64) -> Position {
        Position {
            book: book.into(),
            chapter,
            verse: Some(verse),
        }
    }

    /// `n`/`N` must walk the Find list's BM25 relevance order, not a canonical
    /// re-sort — with the cursor sitting on a hit, the next step lands on the
    /// immediately-following hit in `search::search`'s own order (issue #66,
    /// finding #18).
    #[test]
    fn repeat_search_steps_in_bm25_order_when_cursor_on_a_hit() {
        let (_tmp, db) = kjv_db();
        let query = "shepherd";
        let hits = search::search(&db, query, search::REPEAT_LIMIT).expect("search");
        assert!(hits.len() >= 3, "need several hits to test stepping");

        // Cursor on hits[0] → forward lands on hits[1] (the BM25 successor).
        let cursor = at(&hits[0].book, hits[0].chapter, hits[0].verse);
        let (next, wrapped) =
            repeat_search(&db, query, &cursor, hits[0].verse, true).expect("a next hit");
        assert!(!wrapped, "stepping mid-list does not wrap");
        assert_eq!(next.book, hits[1].book);
        assert_eq!(next.chapter, hits[1].chapter);
        assert_eq!(next.verse, Some(hits[1].verse));

        // Backward from hits[1] returns to hits[0].
        let mid = at(&hits[1].book, hits[1].chapter, hits[1].verse);
        let (prev, wrapped) =
            repeat_search(&db, query, &mid, hits[1].verse, false).expect("a prev hit");
        assert!(!wrapped);
        assert_eq!(prev.book, hits[0].book);
        assert_eq!(prev.chapter, hits[0].chapter);
        assert_eq!(prev.verse, Some(hits[0].verse));
    }

    /// Off any hit (the user navigated away after the Find): forward re-enters
    /// the BM25 list at its first hit, backward at its last — with no wrap cue
    /// (issue #66, finding #18).
    #[test]
    fn repeat_search_off_hit_starts_at_first_or_last() {
        let (_tmp, db) = kjv_db();
        let query = "shepherd";
        let hits = search::search(&db, query, search::REPEAT_LIMIT).expect("search");
        assert!(hits.len() >= 2);

        // Genesis 1:2 is not a "shepherd" hit (Genesis 1 has no shepherds).
        let off = at("GEN", 1, 2);
        assert!(
            !hits
                .iter()
                .any(|h| h.book == "GEN" && h.chapter == 1 && h.verse == 2),
            "test precondition: cursor must be off every hit",
        );

        let (fwd, wrapped) = repeat_search(&db, query, &off, 2, true).expect("first hit");
        assert!(!wrapped, "re-entering the list off-hit is not a wrap");
        assert_eq!(fwd.book, hits[0].book);
        assert_eq!(fwd.verse, Some(hits[0].verse));

        let last = hits.last().expect("nonempty");
        let (back, wrapped) = repeat_search(&db, query, &off, 2, false).expect("last hit");
        assert!(!wrapped);
        assert_eq!(back.book, last.book);
        assert_eq!(back.verse, Some(last.verse));
    }

    /// Stepping off the end of the BM25 list wraps to the other end and sets the
    /// wrap flag, so the caller can surface vim's "search hit BOTTOM…" cue
    /// (issue #66, findings #18 + #10).
    #[test]
    fn repeat_search_wraps_at_the_ends() {
        let (_tmp, db) = kjv_db();
        let query = "shepherd";
        let hits = search::search(&db, query, search::REPEAT_LIMIT).expect("search");
        assert!(hits.len() >= 2);

        // Forward from the last hit wraps to the first.
        let last = hits.last().expect("nonempty");
        let on_last = at(&last.book, last.chapter, last.verse);
        let (wrapped_fwd, fwd_flag) =
            repeat_search(&db, query, &on_last, last.verse, true).expect("wrap to first");
        assert!(fwd_flag, "forward off the end must set the wrap flag");
        assert_eq!(wrapped_fwd.book, hits[0].book);
        assert_eq!(wrapped_fwd.verse, Some(hits[0].verse));

        // Backward from the first hit wraps to the last.
        let on_first = at(&hits[0].book, hits[0].chapter, hits[0].verse);
        let (wrapped_back, back_flag) =
            repeat_search(&db, query, &on_first, hits[0].verse, false).expect("wrap to last");
        assert!(back_flag, "backward off the start must set the wrap flag");
        assert_eq!(wrapped_back.book, last.book);
        assert_eq!(wrapped_back.verse, Some(last.verse));
    }

    /// Build a reading context (pos / passage / cursor / history) for a chapter,
    /// so `apply_action`'s chapter/book arms can be driven directly.
    fn reading_ctx(db: &Db, book: &str, chapter: i64) -> (Position, Passage, i64, History) {
        let passage = db.load_passage(book, chapter).expect("load passage");
        let pos = Position {
            book: book.into(),
            chapter,
            verse: None,
        };
        let history = History::new(pos.clone());
        (pos, passage, 1, history)
    }

    /// Drive one motion `action` through `apply_action` and return its result.
    fn run_motion(
        db: &Db,
        action: Action,
        ctx: &mut (Position, Passage, i64, History),
    ) -> ActionResult {
        apply_action(
            action,
            db,
            &db.list_books().unwrap(),
            &mut ctx.0,
            &mut ctx.1,
            &mut ctx.2,
            &mut ctx.3,
            70,
            20,
        )
        .expect("apply_action")
    }

    /// Prev-chapter / prev-book at Genesis 1 and next-chapter / next-book at the
    /// last passage in the canon are dead-ends: when the motion moves nothing,
    /// `apply_action` reports the canon edge so the caller can cue it (issue #66,
    /// finding #21).
    #[test]
    fn chapter_book_motions_report_the_canon_edges() {
        let (_tmp, db) = kjv_db();
        let books = db.list_books().expect("books");
        let last_book = books.last().expect("nonempty canon").code.clone();
        let last_chapter = db.chapter_count(&last_book).expect("chapter count").max(1);

        // Genesis 1: backward is a Start dead-end; forward moves normally.
        let mut at_gen1 = reading_ctx(&db, "GEN", 1);
        assert_eq!(
            run_motion(&db, Action::PrevChapter(1), &mut at_gen1),
            ActionResult::Boundary(CanonEdge::Start),
        );
        let mut at_gen1 = reading_ctx(&db, "GEN", 1);
        assert_eq!(
            run_motion(&db, Action::PrevBook(1), &mut at_gen1),
            ActionResult::Boundary(CanonEdge::Start),
        );
        let mut at_gen1 = reading_ctx(&db, "GEN", 1);
        assert_eq!(
            run_motion(&db, Action::NextChapter(1), &mut at_gen1),
            ActionResult::Continue,
        );

        // Revelation's last chapter: forward is an End dead-end; backward moves.
        let mut at_end = reading_ctx(&db, &last_book, last_chapter);
        assert_eq!(
            run_motion(&db, Action::NextChapter(1), &mut at_end),
            ActionResult::Boundary(CanonEdge::End),
        );
        let mut at_end = reading_ctx(&db, &last_book, last_chapter);
        assert_eq!(
            run_motion(&db, Action::NextBook(1), &mut at_end),
            ActionResult::Boundary(CanonEdge::End),
        );
        let mut at_end = reading_ctx(&db, &last_book, last_chapter);
        assert_eq!(
            run_motion(&db, Action::PrevChapter(1), &mut at_end),
            ActionResult::Continue,
        );
    }

    /// A count motion that moves *some* before hitting the edge is a normal move,
    /// not a dead-end — only zero movement fires the boundary cue (issue #66,
    /// finding #21).
    #[test]
    fn partial_count_motion_is_not_a_boundary() {
        let (_tmp, db) = kjv_db();
        // From Genesis 2, `5[prev-chapter]` can only step once (to Genesis 1)
        // then stops at the canon edge — but it did move, so: Continue.
        let mut at_gen2 = reading_ctx(&db, "GEN", 2);
        assert_eq!(
            run_motion(&db, Action::PrevChapter(5), &mut at_gen2),
            ActionResult::Continue,
        );
        assert_eq!(at_gen2.0.book, "GEN");
        assert_eq!(at_gen2.0.chapter, 1, "landed on the first chapter");
    }

    /// Build a minimal `LoopState` in the reading view on the given passage, so
    /// `open_footnote_dialog` can be driven directly.
    fn reading_loop_state(db: &Db, book: &str, chapter: i64) -> LoopState {
        let passage = db.load_passage(book, chapter).expect("load passage");
        let pos = Position {
            book: book.into(),
            chapter,
            verse: None,
        };
        let cfg = config::Config::default();
        let mut warnings = Vec::new();
        LoopState::new(
            db.list_books().expect("books"),
            db.translation_label().unwrap_or_else(|_| "en-kjv".into()),
            &pos,
            passage,
            1,
            None, // start in the reading view, not the splash
            "en-kjv",
            &cfg,
            &mut warnings,
        )
    }

    /// When the cross-references dataset is installed (not fetchable) but the
    /// verse has zero footnotes and zero cross-references, `K` shows a transient
    /// instead of opening an empty modal (issue #66, finding #22).
    #[test]
    fn open_footnote_dialog_empty_with_dataset_present_uses_a_transient() {
        let (_tmp, db) = kjv_db();
        // A fresh install has no xrefs.db, so every KJV passage has empty xrefs
        // (and the footnote table is never populated). Passing can_fetch=false
        // simulates "the dataset IS present" — the empty-modal case to avoid.
        let mut state = reading_loop_state(&db, "GEN", 1);
        state.open_footnote_dialog(false);
        assert!(
            matches!(state.dialog, Dialog::None),
            "no modal should open for an empty verse with the dataset present"
        );
        assert_eq!(
            state.transient_msg.as_ref().map(|(t, _, _)| t.as_str()),
            Some("No cross-references for this verse"),
        );
    }

    /// The #67 fetch-affordance path is preserved: when the dataset isn't on
    /// disk (`can_fetch_xrefs = true`), `K` still opens the popup so it can offer
    /// `d` to download — even on an otherwise-empty verse (issue #66, finding
    /// #22 keeps #67).
    #[test]
    fn open_footnote_dialog_still_opens_the_fetch_affordance() {
        let (_tmp, db) = kjv_db();
        let mut state = reading_loop_state(&db, "GEN", 1);
        state.open_footnote_dialog(true);
        assert!(
            matches!(state.dialog, Dialog::Footnote(_)),
            "the fetch-affordance popup must still open when xrefs aren't installed"
        );
    }

    /// The fetch-error classifier buckets representative `anyhow` chains by the
    /// `.context(...)` frames `fetch.rs` attaches, so the in-TUI transient is
    /// actionable per category (issue #66, finding #23).
    #[test]
    fn classify_fetch_error_buckets_by_category() {
        use anyhow::{Context, anyhow};

        // Verification: sha256 mismatch (the exact fetch.rs wording).
        let sha = anyhow!("sha256 mismatch for nb-1930.db.zst: expected aa, got bb");
        assert_eq!(classify_fetch_error(&sha), FetchErrorKind::Verification);

        // Verification: oversize / zip-bomb guard.
        let bomb = anyhow!(
            "nb-1930.db.zst: decompressed to 9 bytes but the manifest declares 8 (corrupt download or zip bomb)"
        );
        assert_eq!(classify_fetch_error(&bomb), FetchErrorKind::Verification);

        // Verification: a zstd decode failure framed by `.context("decompress …")`.
        let decode: anyhow::Error = Err::<(), _>(anyhow!("unexpected end of input"))
            .context("decompress nb-1930.db.zst")
            .unwrap_err();
        assert_eq!(classify_fetch_error(&decode), FetchErrorKind::Verification);

        // CurlMissing: the spawn frame wraps the OS "not found".
        let missing: anyhow::Error = Err::<(), _>(anyhow!("No such file or directory"))
            .context("spawn curl (is it installed?)")
            .unwrap_err();
        assert_eq!(classify_fetch_error(&missing), FetchErrorKind::CurlMissing);

        // Network: curl ran and exited non-zero, wrapped by the download frame.
        let net: anyhow::Error = Err::<(), _>(anyhow!("curl exited with exit status: 6"))
            .context("download https://example/nb-1930.db.zst")
            .unwrap_err();
        assert_eq!(classify_fetch_error(&net), FetchErrorKind::Network);

        // Other: an unrecognised post-fetch IO failure keeps the generic copy.
        let io = anyhow!("permission denied").context("write decompressed nb-1930.db.zst");
        assert_eq!(classify_fetch_error(&io), FetchErrorKind::Other);
    }

    /// Each category renders a distinct, actionable transient that names the
    /// download (except curl-missing, whose fix is global) (issue #66, #23).
    #[test]
    fn fetch_error_transients_are_distinct_and_actionable() {
        assert_eq!(
            FetchErrorKind::Verification.transient("nb-1930"),
            "nb-1930: verification failed \u{2014} corrupt or stale download"
        );
        assert_eq!(
            FetchErrorKind::Network.transient("nb-1930"),
            "nb-1930: couldn't reach GitHub \u{2014} check your connection"
        );
        assert_eq!(
            FetchErrorKind::CurlMissing.transient("nb-1930"),
            "curl not found \u{2014} install curl"
        );
        assert_eq!(
            FetchErrorKind::Other.transient("nb-1930"),
            "Download of nb-1930 failed"
        );
        // All four are mutually distinct.
        let all = [
            FetchErrorKind::Verification.transient("x"),
            FetchErrorKind::Network.transient("x"),
            FetchErrorKind::CurlMissing.transient("x"),
            FetchErrorKind::Other.transient("x"),
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "categories {i} and {j} collided");
            }
        }
    }
}
