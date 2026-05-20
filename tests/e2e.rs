//! PTY-driven end-to-end tests.
//!
//! Each test launches the real `turbo-bible` binary inside a freshly-created
//! `tempfile::TempDir` set as `HOME`, so XDG-resolved paths land inside the
//! tempdir and never touch the developer's real `~/.config/turbo-bible/`.
//!
//! These tests depend on a populated `~/.local/share/turbo-bible/bible.sqlite`
//! (the dev's installed DB). They skip if it's missing rather than fail —
//! CI can populate it via `scripts/import_translations.py` if desired.
//!
//! Reading the rendered TUI characters is unreliable (each cell is positioned
//! individually), so assertions read the side-effect files (state.toml,
//! config.toml, bookmarks.toml) after `exp_eof`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use rexpect::session::{PtySession, spawn_command};
use tempfile::TempDir;

/// Real installed DB. Tests skip if missing.
fn project_db() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join(".local/share/turbo-bible/bible.sqlite");
    p.exists().then_some(p)
}

const fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_turbo-bible")
}

/// Spawn `turbo-bible` with `HOME` pointed at `tmp`, so all XDG paths
/// (config, data, cache) resolve underneath the tempdir.
fn launch(tmp: &TempDir, extra: &[&str]) -> PtySession {
    let mut cmd = Command::new(binary_path());
    cmd.env_clear();
    cmd.env("HOME", tmp.path());
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("TERM", "xterm-256color");
    cmd.env("LANG", "en_US.UTF-8");
    for a in extra {
        cmd.arg(a);
    }
    spawn_command(cmd, Some(8000)).expect("spawn turbo-bible")
}

fn config_path(tmp: &TempDir) -> PathBuf {
    tmp.path().join(".config/turbo-bible/config.toml")
}
fn state_path(tmp: &TempDir) -> PathBuf {
    tmp.path().join(".config/turbo-bible/state.toml")
}
fn bookmarks_path_toml(tmp: &TempDir) -> PathBuf {
    tmp.path().join(".config/turbo-bible/bookmarks.toml")
}
fn bookmarks_path_json(tmp: &TempDir) -> PathBuf {
    tmp.path().join(".config/turbo-bible/bookmarks.json")
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// rexpect's `send` doesn't flush; for raw-mode TUIs we need each byte to
/// hit the child's stdin immediately. This helper sends one keystroke and
/// then pauses long enough for the TUI's 150 ms event-poll to see it.
fn key(p: &mut PtySession, s: &str) {
    p.send(s).unwrap();
    p.flush().unwrap();
    sleep(Duration::from_millis(250));
}

#[test]
fn picker_swap_persists_default_translation() {
    let Some(db) = project_db() else {
        eprintln!("skip: ~/.local/share/turbo-bible/bible.sqlite required");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let mut p = launch(
        &tmp,
        &[
            "--db",
            db.to_str().unwrap(),
            "--translation",
            "en-kjv",
            "--book",
            "JHN",
            "--chapter",
            "3",
        ],
    );
    // Wait for the TUI to initialise before sending keys.
    sleep(Duration::from_millis(500));
    key(&mut p, "t");
    key(&mut p, "j");
    key(&mut p, "j");
    key(&mut p, "\r"); // Enter — select nb-1930
    key(&mut p, "q");
    p.exp_eof().unwrap();

    let cfg = read(&config_path(&tmp));
    assert!(
        cfg.contains("default_translation = \"nb-1930\""),
        "expected nb-1930 in config.toml, got:\n{cfg}"
    );
    let st = read(&state_path(&tmp));
    assert!(
        st.contains("translation = \"nb-1930\""),
        "expected nb-1930 in state.toml, got:\n{st}"
    );
}

#[test]
fn quit_persists_state_book_chapter() {
    let Some(db) = project_db() else {
        eprintln!("skip: ~/.local/share/turbo-bible/bible.sqlite required");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let mut p = launch(
        &tmp,
        &[
            "--db",
            db.to_str().unwrap(),
            "--translation",
            "es-rv1909",
            "--book",
            "ROM",
            "--chapter",
            "8",
        ],
    );
    sleep(Duration::from_millis(500));
    key(&mut p, "q");
    p.exp_eof().unwrap();

    let st = read(&state_path(&tmp));
    assert!(st.contains("translation = \"es-rv1909\""), "got:\n{st}");
    assert!(st.contains("book = \"ROM\""), "got:\n{st}");
    assert!(st.contains("chapter = 8"), "got:\n{st}");
}

#[test]
fn bookmark_json_is_migrated_to_toml_with_nb1930_rename() {
    let Some(db) = project_db() else {
        eprintln!("skip: ~/.local/share/turbo-bible/bible.sqlite required");
        return;
    };
    let tmp = TempDir::new().unwrap();
    // Seed a legacy bookmarks.json under the nb-2024 translation code.
    let cfg_dir = tmp.path().join(".config/turbo-bible");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::write(
        cfg_dir.join("bookmarks.json"),
        r#"{
          "bookmarks": [
            {
              "translation": "nb-2024",
              "book": "JHN",
              "chapter": 3,
              "start_verse": 16,
              "end_verse": 16,
              "label": null,
              "created_at": 1700000000
            }
          ]
        }"#,
    )
    .unwrap();

    let mut p = launch(
        &tmp,
        &[
            "--db",
            db.to_str().unwrap(),
            "--translation",
            "en-kjv",
            "--book",
            "JHN",
            "--chapter",
            "3",
        ],
    );
    sleep(Duration::from_millis(500));
    key(&mut p, "q");
    p.exp_eof().unwrap();

    // Legacy JSON should be gone; new TOML should reference nb-1930.
    assert!(
        !bookmarks_path_json(&tmp).exists(),
        "legacy bookmarks.json should be removed"
    );
    let toml = read(&bookmarks_path_toml(&tmp));
    assert!(
        toml.contains("translation = \"nb-1930\""),
        "expected migration to nb-1930, got:\n{toml}"
    );
    assert!(
        !toml.contains("nb-2024"),
        "nb-2024 should not survive migration, got:\n{toml}"
    );
}

/// Parse `verse = N` out of state.toml.
fn parsed_verse(toml: &str) -> i64 {
    for line in toml.lines() {
        if let Some(rest) = line.trim().strip_prefix("verse = ") {
            return rest.trim().parse().unwrap_or_else(|_| {
                panic!("could not parse verse from line {line:?} in:\n{toml}")
            });
        }
    }
    panic!("no verse= line in state.toml:\n{toml}");
}

/// Regression test for the Goto-with-verse path. `:John 3:16` used to land
/// the cursor on verse 1 of John 3 because `parse_reference` discarded the
/// verse component and `jump_to` always reset `cursor_verse` to 1. With
/// `Position.verse` plumbed end-to-end, the cursor should land on verse 16.
#[test]
fn goto_with_verse_lands_on_typed_verse() {
    let Some(db) = project_db() else {
        eprintln!("skip: ~/.local/share/turbo-bible/bible.sqlite required");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let mut p = launch(
        &tmp,
        &[
            "--db",
            db.to_str().unwrap(),
            "--translation",
            "en-kjv",
            "--book",
            "GEN",
            "--chapter",
            "1",
        ],
    );
    sleep(Duration::from_millis(500));
    // `:` opens the Goto dialog from Reading.
    key(&mut p, ":");
    p.send("John 3:16").unwrap();
    p.flush().unwrap();
    sleep(Duration::from_millis(300));
    key(&mut p, "\r"); // Enter — jump.
    key(&mut p, "q"); // Quit.
    p.exp_eof().unwrap();

    let st = read(&state_path(&tmp));
    assert!(st.contains("book = \"JHN\""), "expected JHN, got:\n{st}");
    assert!(st.contains("chapter = 3"), "expected chapter 3, got:\n{st}");
    assert_eq!(parsed_verse(&st), 16, "expected verse 16, got:\n{st}");
}

/// Regression test for the Find-result jump path. Hitting Enter on a match
/// used to drop `hit.verse` and land on verse 1 of the result's chapter. The
/// fix carries the verse through `FindOutcome::Jump`'s Position. We assert
/// the cursor moved to a verse other than 1 — which proves the verse wasn't
/// silently reset (the old bug's signature). A specific verse would couple
/// the test to FTS5 BM25 ranking; "verse != 1" is the minimum that
/// distinguishes "fixed" from "broken".
#[test]
fn find_jump_lands_on_matched_verse_not_one() {
    let Some(db) = project_db() else {
        eprintln!("skip: ~/.local/share/turbo-bible/bible.sqlite required");
        return;
    };
    let tmp = TempDir::new().unwrap();
    let mut p = launch(
        &tmp,
        &[
            "--db",
            db.to_str().unwrap(),
            "--translation",
            "en-kjv",
            "--book",
            "GEN",
            "--chapter",
            "1",
        ],
    );
    sleep(Duration::from_millis(500));
    // `/` opens Find from Reading.
    key(&mut p, "/");
    // Use a phrase that's well-attested mid-chapter so the top BM25 hit is
    // very unlikely to be verse 1 of anything.
    p.send("everlasting life").unwrap();
    p.flush().unwrap();
    sleep(Duration::from_millis(700)); // let FTS5 populate results
    key(&mut p, "\r"); // Enter — jump to the top hit.
    key(&mut p, "q");
    p.exp_eof().unwrap();

    let st = read(&state_path(&tmp));
    let verse = parsed_verse(&st);
    assert!(
        verse > 1,
        "expected find-jump to preserve hit verse (>1), got verse={verse} in:\n{st}"
    );
}
