# Backlog

Planned work captured between sessions. Take an item, do it, delete its
entry.

## Slice D: empty-DB bootstrap prompt

On launch, when `db_path` doesn't exist, prompt: "No translations
installed. Press `i` to import KJV, Norsk 1930, RV1909 (~6 MB)." The
prompt invokes the existing `turbo-bible import` logic in-process
(`crate::import::run(&ImportArgs::default())`) rather than shelling
out, so the user stays inside the TUI.

### Outline

- New TUI state on `main.rs` startup: if `resolve_db_path()` returns a
  path that doesn't exist, render a centred dialog (style: see
  `src/ui/dialog.rs`) with the prompt text and an `i` / `Esc` chord.
- On `i`: drop terminal raw-mode (so download progress prints land
  cleanly), call `import::run(...)`, restore raw-mode, then resume
  normal startup. On `Esc`: quit with a non-zero exit code.
- `import::ImportArgs::default()` (add it) returns the same defaults
  the CLI computes, so the in-process path matches `turbo-bible
  import` exactly.

### Tradeoffs

- **Pros**: removes the only manual setup step; new users get a
  working reader on first launch.
- **Cons**: ~80 LoC of TUI dialog + a teardown / rebuild of the
  terminal guard around the download. Network failures mid-import
  leave the user in a bad spot — needs a clean retry / abort path.

### Acceptance

- Launching `turbo-bible` against an empty XDG home shows the prompt.
- Pressing `i` performs the import in-process and then lands the user
  on the splash screen with all three translations available.
- Pressing `Esc` exits cleanly with exit code 1.
