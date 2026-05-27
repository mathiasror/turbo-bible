---
name: bundle-translations
description: Rebuild the bundled translation databases from a scrollmapper checkout, or add a new translation to the slate. Use when the user asks to "refresh the bundled data", "rebuild the translation DBs", "update scrollmapper data", "repopulate assets/", "the assets/ dir is empty and the build fails at include_bytes!", "add a translation", or "regenerate manifest.json". Covers the offline data pipeline in crates/turbo-bible-data and the assets/ staging the TUI binary embeds.
---

# Bundle translations (data pipeline → assets/)

The dataflow is documented in `CLAUDE.md` → "Bundle dataflow". This skill is
the operator's runbook for it. The binary embeds **only `en-kjv`**; the other
ten translations + `xrefs.db` ship as GitHub Release assets and are fetched on
demand, verified against the sha256s in the embedded `manifest.json`.

## Refresh the bundled data (most common)

1. **Locate a scrollmapper checkout.** The pipeline reads a local clone of
   `scrollmapper/bible_databases`. Resolution order:
   - explicit path argument to the recipe,
   - `data/scrollmapper-checkout` (the justfile default),
   - `$TURBO_BIBLE_SCROLLMAPPER`, else `~/git/oss/bible_databases` (what the
     pipeline's `--ignored` test uses).
   If none exists, ask the user to clone it and tell you the path. Do not
   fabricate one.
2. **Run the end-to-end recipe:**
   ```sh
   just bundle-translations [path/to/scrollmapper/checkout]
   ```
   This runs `turbo-bible-data build` (→ `dist/build/*.db`), then `compress`
   (→ `dist/translations/*.db.zst` + `manifest.json`), then copies **only**
   `en-kjv.db.zst` + `manifest.json` into `crates/turbo-bible-tui/assets/`.
   That directory is gitignored; the two files are the minimum the binary's
   `include_bytes!` needs.
3. **Verify the binary builds** against the fresh assets:
   ```sh
   cargo build -p turbo-bible
   ```
   An empty/stale `assets/` fails loudly at the `include_bytes!` site in
   `src/bundled.rs` — that's the signal step 2 didn't run.

## Add a translation to the slate

The legal paper trail lives in `data/manifest_source.toml` — only translations
listed there are ever built.

1. Add a `[[translation]]` entry: `code`, `abbr`, `language`, `name`,
   `source_json` (path inside the scrollmapper checkout), `license` (SPDX, or
   `LicenseRef-PublicDomain` for pre-1923 texts), and `attribution` (required
   for CC-BY-family entries).
2. Keep these in sync or it ships inconsistent:
   - the **README translation table** (code + title + language + license row),
   - the **`NOTICE`** file (per-translation licensing),
   - `SQLITE_MAX_ATTACHED` is 10 — 11 translations + xrefs already saturate the
     per-connection attach budget (see CLAUDE.md "Architecture quirks"). Adding
     a 12th translation means the one-connection-per-translation model still
     holds, but double-check nothing assumes a fixed count.
3. Rebuild via "Refresh the bundled data" above, then run the pipeline's
   end-to-end test: `cargo test --workspace -- --ignored`.

## Watch-outs

- **`SCROLLMAPPER_REF` must be identical in `.github/workflows/ci.yml` and
  `.github/workflows/release.yml`.** A mismatch means CI tested a different
  dataset than the release ships. If you re-pin the dataset, bump both.
- The 10 fetched DBs + `xrefs` only reach users via a *release* (they're
  uploaded as `.db.zst` assets + a `translations.tar.gz` bundle). Rebuilding
  locally refreshes `dist/` and `assets/en-kjv.db.zst` but does **not**
  republish the fetched set — that's the release workflow's job.
- License audit: `cargo run -p turbo-bible-data -- audit-licenses --scrollmapper <path>`
  (or `just data-audit <path>`) before adding/altering an entry.
